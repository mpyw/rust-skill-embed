//! Path arithmetic the standard library leaves to the caller.
//!
//! Nothing here touches the disk except where it says so.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Removes `.` and resolves `..` without touching the disk.
///
/// A `..` is resolved against the path as written, which is what makes it
/// lexical: a symbolic link in front of one is not followed. [`real`] is the
/// function that follows links, and the two answer different questions.
fn clean(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(c),
            },
            other => parts.push(other),
        }
    }
    let mut out = PathBuf::new();
    for c in parts {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Makes `path` absolute against the working directory, and cleans it.
pub(crate) fn absolute(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        return Ok(clean(path));
    }
    Ok(clean(&std::env::current_dir()?.join(path)))
}

/// The user's home directory, as the agents' own tools read it.
pub(crate) fn home() -> io::Result<PathBuf> {
    std::env::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the home directory is not known"))
}

/// Reports whether two paths name the same directory, with symbolic links
/// resolved. A home directory reached through one is still the home directory.
pub(crate) fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(ra), Ok(rb)) if ra == rb)
}

/// Reports whether two existing paths are the same file.
///
/// A file system that folds case or normalizes Unicode answers to more than
/// one spelling of a name, and asking it is the only way to know. On Unix that
/// is the device and inode pair. Elsewhere it is the two canonical paths,
/// which cannot see a hard link but does see every spelling.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        match (fs::metadata(a), fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
    }
}

/// Bounds the links [`real`] follows by hand, the way a kernel bounds its own.
///
/// A cycle reaches [`fs::canonicalize`] as `ELOOP` rather than as a missing
/// file and is returned as the error it is, so this is a backstop and not the
/// guard.
const MAX_LINK_HOPS: u32 = 64;

/// Resolves the symbolic links in `path`, which it first makes absolute so that
/// two results can be compared.
///
/// [`fs::canonicalize`] needs the whole path to exist, and a skills directory
/// usually does not yet. The existing part is resolved and the rest appended,
/// which is where the directories about to be created will end up.
pub(crate) fn real(path: &Path) -> io::Result<PathBuf> {
    let abs = absolute(path)?;
    let mut at = abs.clone();
    let mut rest: Vec<OsString> = Vec::new();
    let mut hops = 0;
    loop {
        match fs::canonicalize(&at) {
            Ok(resolved) => {
                return Ok(rest.iter().rev().fold(resolved, |acc, part| acc.join(part)));
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            Err(_) => {}
        }
        // A link whose target is not there yet still says where a write would
        // go, and `canonicalize` will not read one: it reports the target as
        // missing, which is indistinguishable from an ordinary missing
        // directory. Reading it by hand keeps a dangling link from passing as
        // one. Only these steps are counted, since walking up to an existing
        // ancestor is bounded by the depth of the path.
        if let Ok(target) = fs::read_link(&at) {
            hops += 1;
            if hops > MAX_LINK_HOPS {
                return Err(io::Error::other(format!(
                    "{}: too many symbolic links",
                    path.display()
                )));
            }
            at = if target.is_absolute() {
                clean(&target)
            } else {
                clean(&at.parent().unwrap_or(Path::new("/")).join(target))
            };
            continue;
        }
        let Some(parent) = at.parent() else {
            // The walk reached the volume root without finding anything that
            // exists. Nothing can be resolved, so the path stands as it is.
            return Ok(abs);
        };
        let Some(name) = at.file_name() else {
            return Ok(abs);
        };
        rest.push(name.to_owned());
        at = parent.to_path_buf();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_resolves_parents_lexically() {
        assert_eq!(clean(Path::new("/a/b/../c")), Path::new("/a/c"));
        assert_eq!(clean(Path::new("/a/./b/")), Path::new("/a/b"));
        assert_eq!(clean(Path::new("/../..")), Path::new("/"));
        assert_eq!(clean(Path::new("a/../..")), Path::new(".."));
        assert_eq!(clean(Path::new("")), Path::new("."));
    }

    /// The bound a project install is checked against is component wise, so a
    /// sibling whose name merely starts with the root's is outside it.
    #[test]
    fn starts_with_is_component_wise() {
        assert!(!Path::new("/a/project-notes").starts_with("/a/project"));
        assert!(Path::new("/a/project/x").starts_with("/a/project"));
    }
}
