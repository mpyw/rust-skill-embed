//! A tool that parses its own arguments and still has a skill command.
//!
//! `intercept` runs before anything else reads the command line, so it works
//! for a tool built on a framework that offers no hook once it has started.

use std::process::ExitCode;
use std::sync::LazyLock;

use include_dir::{Dir, include_dir};
use skill_embed::{Installer, SkillSet};

static SKILLS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/skills");

static SKILLS: LazyLock<Installer> = LazyLock::new(|| {
    Installer::new(SkillSet::from_include_dir(&SKILLS_DIR).expect("the embedded skills are valid"))
        .with_tool_name("demo-lint")
        .with_version(env!("CARGO_PKG_VERSION"))
});

fn main() -> ExitCode {
    if let Some(code) = SKILLS.intercept() {
        return code;
    }

    // `args_os`, because a file name is not always UTF-8 and `args` panics on
    // one. A tool that takes file names has to reach them as the operating
    // system spells them.
    let files: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if files.is_empty() {
        // Nothing else mentions the skill command, so this line has to.
        eprintln!("usage: demo-lint [file...]\n\n{}", SKILLS.usage_hint());
        return ExitCode::FAILURE;
    }
    for file in files {
        println!("{}: nothing to report", file.display());
    }
    ExitCode::SUCCESS
}
