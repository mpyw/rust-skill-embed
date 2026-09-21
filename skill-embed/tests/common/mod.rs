//! Fixtures the integration tests share.

#![allow(dead_code, reason = "each test binary uses a different part of this")]

use std::fs;
use std::path::{Path, PathBuf};

use skill_embed::{File, InstallOptions, Installer, SkillSet};

/// A temporary directory, with the few conveniences these tests keep needing.
pub struct TempDir(tempfile::TempDir);

impl TempDir {
    pub fn new(label: &str) -> Self {
        Self(
            tempfile::Builder::new()
                .prefix(&format!("skill-embed-{label}-"))
                .tempdir()
                .expect("a temporary directory"),
        )
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

    pub fn join(&self, p: impl AsRef<Path>) -> PathBuf {
        self.0.path().join(p)
    }

    /// Writes a file, creating the directories above it.
    pub fn write(&self, rel: impl AsRef<Path>, contents: &str) -> PathBuf {
        let path = self.join(rel);
        fs::create_dir_all(path.parent().expect("a parent")).expect("the parent directories");
        fs::write(&path, contents).expect("the file");
        path
    }

    pub fn mkdir(&self, rel: impl AsRef<Path>) -> PathBuf {
        let path = self.join(rel);
        fs::create_dir_all(&path).expect("the directory");
        path
    }
}

/// The manifest of a skill that says only what it is called.
pub fn manifest(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
}

/// One skill, with whatever extra files are named.
pub fn skill_files(name: &str, extra: &[(&str, &str)]) -> Vec<File> {
    let mut files = vec![File::new(
        format!("{name}/SKILL.md"),
        manifest(name, "A skill this test installs.").into_bytes(),
    )];
    for (path, contents) in extra {
        files.push(File::new(format!("{name}/{path}"), contents.as_bytes().to_vec()));
    }
    files
}

/// A set holding one skill of each name.
pub fn skills(names: &[&str]) -> SkillSet {
    let files: Vec<File> = names.iter().flat_map(|n| skill_files(n, &[])).collect();
    SkillSet::from_files(files).expect("the fixture is a valid skill set")
}

/// An installer that writes into `dir` and answers to `mytool`.
pub fn installer(set: SkillSet) -> Installer {
    Installer::new(set)
        .with_tool_name("mytool")
        .with_version("v1.0.0")
        .with_output(Vec::new())
        .with_error_output(Vec::new())
}

/// Options that write into one named directory, which is the one destination
/// that needs neither a project root nor a home directory.
pub fn into_dir(dir: &Path) -> InstallOptions {
    InstallOptions { dir: Some(dir.to_path_buf()), ..Default::default() }
}

/// The set read from `testdata/skills`.
pub fn testdata() -> SkillSet {
    SkillSet::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/skills").as_path())
        .expect("the testdata skills are valid")
}

/// A writer a test can hand to an [`Installer`] and still read from.
#[derive(Clone, Default)]
pub struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Captured {
    pub fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8(bytes.clone()).expect("the output is UTF-8")
    }
}

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An installer whose two streams a test can read back.
pub fn captured(set: SkillSet) -> (Installer, Captured, Captured) {
    let (out, err) = (Captured::default(), Captured::default());
    let installer = Installer::new(set)
        .with_tool_name("mytool")
        .with_version("v1.0.0")
        .with_output(out.clone())
        .with_error_output(err.clone());
    (installer, out, err)
}
