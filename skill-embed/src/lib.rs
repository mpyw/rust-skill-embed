//! Ship agent skills inside a Rust binary, and give that binary a
//! `skill install` command.
//!
//! It is the reverse of `gh skill install`. A tool carries its own skills in
//! the executable, and writes them into the directory the user's agent reads
//! from. The agent directories and the flag set are taken from
//! `gh skill install`, so a user who knows that command already knows this one.
//!
//! Skills live under `skills/<name>/SKILL.md`, which is the layout defined by
//! the [Agent Skills specification](https://agentskills.io/specification).
//!
//! ```no_run
//! use std::process::ExitCode;
//! use std::sync::LazyLock;
//!
//! use include_dir::{Dir, include_dir};
//! use skill_embed::{Installer, SkillSet};
//!
//! static SKILLS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/skills");
//!
//! static SKILLS: LazyLock<Installer> = LazyLock::new(|| {
//!     Installer::new(SkillSet::from_include_dir(&SKILLS_DIR).expect("valid skills"))
//!         .with_tool_name("mytool")
//!         .with_version(env!("CARGO_PKG_VERSION"))
//! });
//!
//! fn main() -> ExitCode {
//!     if let Some(code) = SKILLS.intercept() {
//!         return code;
//!     }
//!     // the rest of your tool
//!     ExitCode::SUCCESS
//! }
//! ```
//!
//! [`Installer::intercept`] gives a tool the skill subcommand in one line.
//! [`Installer::run`] is for a tool that parses its own arguments, and the
//! `skill-embed-clap` crate wires the same commands into
//! [clap](https://docs.rs/clap).

mod action;
mod agent;
mod cli;
mod error;
mod install;
mod manifest;
mod paths;
mod projectroot;
mod scope;
mod skill;
mod state;
mod textfmt;
mod tree;
mod usage;

pub use action::Action;
pub use agent::{Agent, AgentSelector};
pub use cli::{render_results, render_status};
pub use error::{Error, ForceRequired, Result};
pub use install::{InstallOptions, InstallResult, InstallStatus, InstallTarget, Installer};
pub use scope::Scope;
pub use skill::{File, SKILL_FILE, Skill, SkillSet};
pub use state::State;

/// Frontmatter keys written into an installed [`SKILL_FILE`].
///
/// They let a later run tell an outdated copy from one that was edited by hand,
/// and they are namespaced so they never collide with the source tracking keys
/// `gh skill install` writes.
pub mod meta {
    use crate::manifest;

    /// Names the tool that installed the skill.
    pub const EMBEDDED_BY: &str = manifest::KEY_EMBEDDED_BY;
    /// The version of that tool.
    pub const EMBEDDED_VERSION: &str = manifest::KEY_EMBEDDED_VERSION;
    /// When the skill was installed, in RFC 3339 and UTC.
    pub const EMBEDDED_AT: &str = manifest::KEY_EMBEDDED_AT;
    /// The digest of what was installed.
    pub const EMBEDDED_DIGEST: &str = manifest::KEY_EMBEDDED_DIGEST;
}
