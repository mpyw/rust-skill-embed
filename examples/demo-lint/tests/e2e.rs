//! The real binary, run both ways.
//!
//! A subprocess is the only place the environment and the working directory can
//! be set without the rest of the suite seeing it, so the scopes that read them
//! are exercised here.

use std::process::{Command, Output};

/// The variable `std::env::home_dir` reads. Unix reads `HOME` and Windows reads
/// `USERPROFILE`, so a test that sets one of them sets nothing on the other.
const HOME_VAR: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

fn demo_lint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_demo-lint"))
        .args(args)
        .output()
        .expect("the example binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("the output is UTF-8")
}

#[test]
fn a_normal_run_is_left_alone() {
    let out = demo_lint(&["src/main.rs"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "src/main.rs: nothing to report\n");
}

/// The guard is the first argument and nothing else, so a tool's own flags are
/// never read as a subcommand.
#[test]
fn a_flag_before_the_subcommand_is_not_intercepted() {
    let out = demo_lint(&["-v", "skill", "install"]);
    assert!(stdout(&out).contains("nothing to report"), "{}", stdout(&out));
}

#[test]
fn the_skill_command_prints_its_help_and_succeeds() {
    let out = demo_lint(&["skill"]);
    assert!(out.status.success(), "help exited {:?}", out.status.code());
    let text = stdout(&out);
    assert!(text.contains("Manage the agent skills embedded in demo-lint"), "{text}");
    assert!(text.contains("example-adoption"), "{text}");
}

#[test]
fn an_unknown_subcommand_fails() {
    let out = demo_lint(&["skill", "instal"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("demo-lint: unknown skill subcommand"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn install_writes_the_skill_and_a_second_run_is_a_no_op() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let dir = tmp.path().join("skills");
    let dir_arg = dir.display().to_string();

    let out = demo_lint(&["skill", "install", "--dir", &dir_arg]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).starts_with("installed  example-adoption"), "{}", stdout(&out));

    let installed = std::fs::read_to_string(dir.join("example-adoption/SKILL.md"))
        .expect("the installed manifest");
    assert!(installed.contains("x-embedded-by: demo-lint"), "{installed}");
    assert!(installed.contains("x-embedded-version:"), "{installed}");

    let out = demo_lint(&["skill", "install", "--dir", &dir_arg]);
    assert!(stdout(&out).contains("already up to date"), "{}", stdout(&out));

    let out = demo_lint(&["skill", "list", "--dir", &dir_arg]);
    assert!(stdout(&out).contains("up-to-date"), "{}", stdout(&out));
}

/// A project scope run resolves against the project, and says which one.
#[test]
fn a_project_run_names_the_root_it_resolved_to() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("a repository");
    std::fs::create_dir_all(repo.join("sub")).expect("a subdirectory");

    let out = Command::new(env!("CARGO_BIN_EXE_demo-lint"))
        .args(["skill", "install", "--agent", "claude-code"])
        // Run from a subdirectory: the search has to walk up to the root.
        .current_dir(repo.join("sub"))
        .env(HOME_VAR, tmp.path().join("home"))
        .output()
        .expect("the example binary runs");

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).starts_with("Project root: "), "{}", stdout(&out));
    assert!(repo.join(".claude/skills/example-adoption/SKILL.md").exists());
}

/// The user scope directories live in the home directory, so a project install
/// landing there is refused rather than written in silence.
#[test]
fn a_project_run_from_the_home_directory_is_refused() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).expect("a home directory");

    let out = Command::new(env!("CARGO_BIN_EXE_demo-lint"))
        .args(["skill", "install"])
        .current_dir(&home)
        .env(HOME_VAR, &home)
        .output()
        .expect("the example binary runs");

    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("home directory"), "{err}");
    assert!(err.contains("--scope user"), "{err}");
}

#[test]
fn user_scope_writes_under_the_home_directory() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).expect("a home directory");

    let out = Command::new(env!("CARGO_BIN_EXE_demo-lint"))
        .args(["skill", "install", "--scope", "user", "--agent", "claude-code"])
        .env(HOME_VAR, &home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .expect("the example binary runs");

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(home.join(".claude/skills/example-adoption/SKILL.md").exists());
    // User scope has no project, so nothing names one.
    assert!(!stdout(&out).contains("Project root:"), "{}", stdout(&out));
}

/// The line a tool prints itself, because its own help never mentions the
/// skill command.
#[test]
fn the_usage_hint_reaches_a_user_who_ran_it_wrong() {
    let out = demo_lint(&[]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(
            r#"Run "demo-lint skill" to install the 1 agent skill embedded in demo-lint."#
        ),
        "{err}"
    );
}
