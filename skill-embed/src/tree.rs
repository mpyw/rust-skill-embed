//! Hashes and materialises a skill directory.
//!
//! A skill reaches this module as a flat list of regular files in path order,
//! whether it came from the binary or from the disk. That is the whole of what
//! a digest covers and the whole of what a write puts back, so one type serves
//! both sides of every comparison.
//!
//! An embedded file carries no mode, so [`Tree::write`] decides the mode
//! itself, and anything that is not a regular file is refused.

use sha2::{Digest as _, Sha256};
use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::manifest;

/// Files an operating system or a file browser leaves behind. A skill author
/// never writes one on purpose.
const JUNK_NAMES: [&str; 4] = [".DS_Store", "Thumbs.db", "desktop.ini", ".localized"];

/// Reports whether `path` names one of those files.
///
/// They are skipped on the way in and on the way out. An installed skill sits
/// in a directory a user may open in a file browser. A `.DS_Store` appearing
/// beside it is not the user editing the skill.
pub(crate) fn is_junk(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    JUNK_NAMES.contains(&name)
}

/// One regular file inside a skill directory.
#[derive(Clone, Debug)]
pub(crate) struct File {
    /// Slash separated, relative to the skill's own directory.
    pub(crate) path: String,
    pub(crate) data: Cow<'static, [u8]>,
    /// Whether the file carries the executable bit. Always `false` for an
    /// embedded file, which has no mode at all, and on a platform with no such
    /// bit.
    pub(crate) executable: bool,
}

/// A skill directory's regular files, in path order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tree {
    files: Vec<File>,
}

/// Decides which files are written with the executable bit.
pub(crate) type ExecutableRule = dyn Fn(&str, &[u8]) -> bool + Send + Sync;

/// Rewrites a file's contents on the way out. [`None`] leaves them alone.
pub(crate) type Transform<'a> = dyn Fn(&str, &[u8]) -> Option<Vec<u8>> + 'a;

/// The default [`ExecutableRule`]: a file starting with a `#!` shebang.
pub(crate) fn has_shebang(_name: &str, data: &[u8]) -> bool {
    data.starts_with(b"#!")
}

impl Tree {
    /// Collects files into a tree, dropping the ones an operating system left
    /// behind and putting the rest in walk order.
    pub(crate) fn from_files(files: impl IntoIterator<Item = File>) -> Self {
        let mut files: Vec<File> = files.into_iter().filter(|f| !is_junk(&f.path)).collect();
        files.sort_by(|a, b| walk_order(&a.path).cmp(&walk_order(&b.path)));
        Self { files }
    }

    /// Reads a real directory.
    ///
    /// Anything that is not a regular file is an error. A symbolic link inside
    /// an installed skill is the case that happens, and the caller turns the
    /// error into a state rather than a failed run.
    pub(crate) fn read_dir(root: &Path) -> io::Result<Self> {
        Ok(Self::from_files(read_dir_raw(root)?))
    }

    pub(crate) fn files(&self) -> &[File] {
        &self.files
    }

    /// Returns the subtree rooted at `dir`, with the prefix removed.
    pub(crate) fn subtree(&self, dir: &str) -> Self {
        let prefix = format!("{dir}/");
        Self {
            files: self
                .files
                .iter()
                .filter_map(|f| {
                    Some(File {
                        path: f.path.strip_prefix(&prefix)?.to_owned(),
                        data: f.data.clone(),
                        executable: f.executable,
                    })
                })
                .collect(),
        }
    }

    /// Reads one file's contents.
    pub(crate) fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.iter().find(|f| f.path == path).map(|f| &*f.data)
    }

    /// The SHA-256 of the directory: every regular file's path, size and
    /// contents, in path order.
    ///
    /// The manifest is hashed with the injected metadata removed, so a
    /// directory installed from an embedded skill hashes equal to the skill
    /// itself.
    pub(crate) fn digest(&self) -> String {
        let mut h = Sha256::new();
        for f in &self.files {
            let data = if f.path == manifest::FILE_NAME {
                manifest::normalize(&f.data)
            } else {
                Cow::Borrowed(&*f.data)
            };
            h.update(f.path.as_bytes());
            h.update([0]);
            h.update(data.len().to_string().as_bytes());
            h.update([0]);
            h.update(&data);
        }
        format!("sha256:{:x}", h.finalize())
    }

    /// Reports whether every file carries the executable bit the rule asks for.
    ///
    /// The digest cannot answer this. An embedded file has no mode at all, so
    /// the two sides of a comparison never agree on one. The rule reads the
    /// contents instead. The digest has already matched those contents, so both
    /// sides reach the same answer.
    ///
    /// A platform with no executable bit has nothing to compare, and answers
    /// yes rather than asking for a repair that would not change anything.
    pub(crate) fn executable_bits_match(&self, rule: &ExecutableRule) -> bool {
        if !cfg!(unix) {
            return true;
        }
        self.files.iter().all(|f| rule(&f.path, &f.data) == f.executable)
    }

    /// Materialises the tree at `dest`, replacing whatever is there.
    ///
    /// The tree is staged in a sibling directory. The swap is two renames, with
    /// the old directory moved aside in between and moved back if the second
    /// rename fails. Removing the destination first is not atomic, so a failure
    /// part way through would leave `dest` with no installation at all.
    pub(crate) fn write(
        &self,
        dest: &Path,
        transform: Option<&Transform<'_>>,
        executable: &ExecutableRule,
    ) -> io::Result<()> {
        let parent = dest.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("{} has no parent", dest.display()))
        })?;
        let leaf = dest
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} has no name", dest.display()),
                )
            })?
            .to_owned();
        fs::create_dir_all(parent)?;

        let staging =
            tempfile::Builder::new().prefix(&format!(".{leaf}.tmp-")).tempdir_in(parent)?;
        for f in &self.files {
            let target = safe_join(staging.path(), &f.path)?;
            let rewritten = transform.and_then(|t| t(&f.path, &f.data));
            let data: &[u8] = rewritten.as_deref().unwrap_or(&f.data);
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(&target, data)?;
            set_executable(&target, executable(&f.path, data))?;
        }

        // A name to move the old directory to, next to it so the rename stays
        // on one file system. Creating the directory is only how the name is
        // claimed, so it is removed again before anything is moved to it.
        let aside =
            tempfile::Builder::new().prefix(&format!(".{leaf}.old-")).tempdir_in(parent)?.keep();
        fs::remove_dir(&aside)?;
        let moved = match fs::symlink_metadata(dest) {
            Ok(_) => {
                fs::rename(dest, &aside)?;
                true
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e),
        };

        if let Err(e) = fs::rename(staging.path(), dest) {
            if moved && fs::rename(&aside, dest).is_err() {
                // `dest` holds neither tree, and the only copy of the old one
                // is where it was moved to. It is kept and named, because
                // removing it here would be the whole loss.
                return Err(io::Error::new(
                    e.kind(),
                    format!("{e} (the previous installation is at {})", aside.display()),
                ));
            }
            return Err(e);
        }
        // The staging directory is `dest` now, so nothing is left to clean up.
        let _ = staging.keep();

        // `dest` holds the new tree, so the write has succeeded. Removing the
        // one that was moved aside is cleanup, and a failure there is not a
        // failed install.
        if moved {
            let _ = fs::remove_dir_all(&aside);
        }
        Ok(())
    }
}

#[cfg(unix)]
fn set_executable(path: &Path, on: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(if on { 0o755 } else { 0o644 }))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _on: bool) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    false
}

/// Reads a real directory without dropping anything, so that a caller which
/// has to refuse a file an operating system left behind can still see it.
pub(crate) fn read_dir_raw(root: &Path) -> io::Result<Vec<File>> {
    let mut files = Vec::new();
    collect(root, "", &mut files)?;
    Ok(files)
}

fn collect(dir: &Path, prefix: &str, out: &mut Vec<File>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: name is not UTF-8", entry.path().display()),
            ));
        };
        let path = if prefix.is_empty() { name.to_owned() } else { format!("{prefix}/{name}") };
        // `symlink_metadata`, so a symbolic link is seen as itself rather than
        // as whatever it points at.
        let meta = entry.path().symlink_metadata()?;
        if meta.is_dir() {
            collect(&entry.path(), &path, out)?;
        } else if meta.is_file() {
            out.push(File {
                data: Cow::Owned(fs::read(entry.path())?),
                executable: is_executable(&meta),
                path,
            });
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{path}: not a regular file"),
            ));
        }
    }
    Ok(())
}

/// The key that puts paths in the order a directory walk reaches them.
///
/// Sorting the paths as strings is not that order. A directory `b` is reached
/// before a file `b.md`, because `ReadDir` sorts `b` before `b.md` and the walk
/// descends at once. Comparing the strings puts `b.md` first, since `.` sorts
/// before `/`.
///
/// The digest covers the files in this order, and go-skill-embed hashes the
/// same skill through its own directory walk. A skill holding both `b.md` and
/// `b/` hashed differently in the two until this existed.
fn walk_order(path: &str) -> Vec<&str> {
    path.split('/').collect()
}

/// Rejects a path that would escape `root`.
fn safe_join(root: &Path, path: &str) -> io::Result<PathBuf> {
    let mut out = root.to_path_buf();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsafe path {path:?} in embedded skill"),
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    fn temp(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("skill-embed-tree-{label}-"))
            .tempdir()
            .expect("a temporary directory")
    }

    fn file(path: &str, data: &'static str) -> File {
        File { path: path.to_owned(), data: Cow::Borrowed(data.as_bytes()), executable: false }
    }

    fn demo() -> Tree {
        Tree::from_files([
            file("SKILL.md", "---\nname: demo\n---\nbody\n"),
            file("scripts/run.sh", "#!/bin/sh\necho hi\n"),
            file("reference/tips.md", "tips\n"),
        ])
    }

    /// An installed copy carries four keys the embedded original does not, and
    /// the two still have to hash alike.
    #[test]
    fn the_digest_ignores_the_injected_metadata() {
        let plain = demo();
        let stamped = Tree::from_files(plain.files().iter().map(|f| {
            if f.path == manifest::FILE_NAME {
                File {
                    data: Cow::Owned(manifest::with(
                        &f.data,
                        &[manifest::Entry {
                            key: manifest::KEY_EMBEDDED_BY,
                            value: "mytool".to_owned(),
                        }],
                    )),
                    ..f.clone()
                }
            } else {
                f.clone()
            }
        }));
        assert_eq!(plain.digest(), stamped.digest());
    }

    /// The order the digest covers the files in, which is a directory walk's
    /// and not a string sort's.
    #[test]
    fn a_directory_is_reached_before_a_file_whose_name_extends_it() {
        let tree = Tree::from_files([
            file("b.md", "top level b\n"),
            file("b/c.md", "inside b\n"),
            file("ab.md", "ab\n"),
        ]);
        let paths: Vec<&str> = tree.files().iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["ab.md", "b/c.md", "b.md"]);
    }

    #[test]
    fn the_digest_notices_an_edit() {
        let edited = Tree::from_files([
            file("SKILL.md", "---\nname: demo\n---\nbody\n"),
            file("scripts/run.sh", "#!/bin/sh\necho hi\n"),
            file("reference/tips.md", "tips, edited\n"),
        ]);
        assert_ne!(demo().digest(), edited.digest());
    }

    #[test]
    fn the_digest_ignores_what_an_operating_system_leaves_behind() {
        let with_junk = Tree::from_files(
            demo().files().iter().cloned().chain([file("reference/.DS_Store", "\0")]),
        );
        assert_eq!(demo().digest(), with_junk.digest());
    }

    #[test]
    fn a_write_replaces_what_is_there() {
        let temp = temp("replace");
        let root = temp.path();
        let dest = root.join("demo");
        fs::create_dir_all(dest.join("stale")).expect("a stale directory");
        fs::write(dest.join("stale/old.md"), "gone").expect("a stale file");

        demo().write(&dest, None, &has_shebang).expect("the write");
        assert!(dest.join("SKILL.md").exists());
        assert!(!dest.join("stale").exists(), "what was there survived the write");
    }

    #[cfg(unix)]
    #[test]
    fn a_write_restores_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = temp("exec");
        let root = temp.path();
        let dest = root.join("demo");
        demo().write(&dest, None, &has_shebang).expect("the write");

        let mode =
            |rel: &str| fs::metadata(dest.join(rel)).expect(rel).permissions().mode() & 0o111;
        assert_ne!(mode("scripts/run.sh"), 0, "a shebang script is not executable");
        assert_eq!(mode("SKILL.md"), 0, "a manifest is executable");

        // The check that decides whether an installed copy still matches has to
        // fall back to the same rule the write used.
        let found = Tree::read_dir(&dest).expect("the installed tree");
        assert!(found.executable_bits_match(&has_shebang));
        fs::set_permissions(dest.join("scripts/run.sh"), fs::Permissions::from_mode(0o644))
            .expect("the chmod");
        let found = Tree::read_dir(&dest).expect("the installed tree");
        assert!(!found.executable_bits_match(&has_shebang), "a lost bit went unnoticed");
    }

    #[test]
    fn a_transform_rewrites_on_the_way_out() {
        let temp = temp("transform");
        let root = temp.path();
        let dest = root.join("demo");
        let stamp =
            |name: &str, data: &[u8]| (name == "SKILL.md").then(|| [data, b"stamped\n"].concat());
        demo().write(&dest, Some(&stamp), &has_shebang).expect("the write");

        assert!(
            fs::read_to_string(dest.join("SKILL.md")).expect("the manifest").ends_with("stamped\n")
        );
        assert_eq!(fs::read_to_string(dest.join("reference/tips.md")).expect("a file"), "tips\n");
    }

    /// The last thing between a path inside an embedded skill and a write
    /// outside the destination. An `include_dir!` tree cannot produce one of
    /// these, but a caller may hand over any files it likes, so the check is
    /// the guarantee rather than the source.
    #[test]
    fn an_escaping_path_is_rejected() {
        let temp = temp("safe-join");
        let root = temp.path().to_path_buf();
        for p in ["..", "../evil", "../../evil", "a/../../evil", "/etc/passwd"] {
            assert!(safe_join(&root, p).is_err(), "safe_join({p:?}) was accepted");
        }
        assert_eq!(safe_join(&root, ".").expect("."), root);
        assert_eq!(safe_join(&root, "a/b/c.md").expect("a path"), root.join("a/b/c.md"));
        assert_eq!(safe_join(&root, "./a.md").expect("a path"), root.join("a.md"));
    }

    /// A write that fails part way through leaves the installation that was
    /// there, and leaves nothing beside it either.
    #[test]
    fn a_failed_write_leaves_the_old_tree() {
        let temp = temp("failed");
        let root = temp.path();
        let dest = root.join("demo");
        demo().write(&dest, None, &has_shebang).expect("the first write");

        let escaping =
            Tree::from_files(demo().files().iter().cloned().chain([file("../evil", "no")]));
        let err =
            escaping.write(&dest, None, &has_shebang).expect_err("the write reported success");
        assert!(err.to_string().contains("unsafe path"), "{err}");

        for p in ["SKILL.md", "scripts/run.sh", "reference/tips.md"] {
            assert!(dest.join(p).exists(), "{p} did not survive the failed write");
        }
        assert!(!root.join("evil").exists(), "the file landed outside the destination");
        for entry in fs::read_dir(root).expect("the parent") {
            let name = entry.expect("an entry").file_name();
            assert!(
                !name.to_string_lossy().starts_with(".demo."),
                "a staging or aside directory was left behind: {name:?}"
            );
        }
    }

    /// Removing the tree that was moved aside is cleanup. The new tree is
    /// already in place by then, so a failure there is not a failed install.
    #[cfg(unix)]
    #[test]
    fn a_write_succeeds_when_the_aside_tree_cannot_be_removed() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = temp("aside");
        let root = temp.path();
        let dest = root.join("demo");
        demo().write(&dest, None, &has_shebang).expect("the first write");

        // A subdirectory nothing may unlink from.
        let locked = dest.join("locked");
        fs::create_dir_all(&locked).expect("the locked directory");
        fs::write(locked.join("f"), "x").expect("a file inside it");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).expect("the chmod");

        demo()
            .write(&dest, None, &has_shebang)
            .expect("a cleanup failure was reported as a failure");
        assert!(dest.join("SKILL.md").exists(), "the new tree is not in place");

        // The locked directory moved aside with the old tree, so it is found by
        // walking rather than by the name it started under.
        for entry in fs::read_dir(root).expect("the parent").flatten() {
            let _ =
                fs::set_permissions(entry.path().join("locked"), fs::Permissions::from_mode(0o755));
        }
    }
}
