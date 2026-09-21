//! The clap example, run as a user would.

use std::process::{Command, Output};

fn demo_clap(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_demo-clap"))
        .args(args)
        .output()
        .expect("the example binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("the output is UTF-8")
}

#[test]
fn the_tools_own_subcommand_still_works() {
    let out = demo_clap(&["check", "src/main.rs"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "src/main.rs: nothing to report\n");
}

#[test]
fn the_root_help_lists_the_skill_command() {
    let out = demo_clap(&["--help"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("skill"), "{}", stdout(&out));
}

#[test]
fn install_and_uninstall_reach_the_named_directory() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let dir = tmp.path().join("skills");
    let dir_arg = dir.display().to_string();

    let out = demo_clap(&["skill", "install", "--dir", &dir_arg]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.join("example-adoption/SKILL.md").exists());

    let out = demo_clap(&["skill", "ls", "--dir", &dir_arg]);
    assert!(stdout(&out).contains("up-to-date"), "{}", stdout(&out));

    let out = demo_clap(&["skill", "uninstall", "--dir", &dir_arg]);
    assert!(out.status.success());
    assert!(!dir.join("example-adoption").exists());
}

/// clap refuses a scope it does not know before the core is reached.
#[test]
fn an_unknown_scope_is_refused_by_clap() {
    let out = demo_clap(&["skill", "install", "--scope", "nowhere"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid value"), "{err}");
}
