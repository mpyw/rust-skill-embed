//! The clap command, driven the way a tool's `main` drives it.

use clap::Command;
use skill_embed::{Error, File, Installer, SkillSet, State};

fn skills() -> Installer {
    let set = SkillSet::from_files([
        File::new(
            "demo-skill/SKILL.md",
            b"---\nname: demo-skill\ndescription: A skill this test installs.\n---\n\n# Demo\n"
                .to_vec(),
        ),
        File::new("demo-skill/scripts/run.sh", b"#!/bin/sh\necho hi\n".to_vec()),
    ])
    .expect("the fixture is a valid skill set");
    Installer::new(set).with_tool_name("mytool").with_version("v1.0.0")
}

/// Parses `mytool skill ...` the way a root command with the skill command
/// beneath it would, and answers with the skill command's own matches, which
/// is what [`skill_embed_clap::run`] takes.
fn matches(skills: &Installer, args: &[&str]) -> clap::ArgMatches {
    let root = Command::new("mytool").subcommand(skill_embed_clap::command(skills));
    let all = root
        .try_get_matches_from(std::iter::once("mytool").chain(args.iter().copied()))
        .expect("the arguments parse");
    all.subcommand_matches("skill").expect("the skill command").clone()
}

/// One subcommand's own matches, which is what [`skill_embed_clap::options`]
/// takes.
fn sub(skills: &Installer, args: &[&str]) -> clap::ArgMatches {
    matches(skills, args).subcommand().expect("a subcommand").1.clone()
}

#[test]
fn install_list_and_uninstall_reach_the_same_directory() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let dir = tmp.path().join("skills");
    let dir_arg = dir.display().to_string();
    let skills = skills();

    skill_embed_clap::run(&skills, &matches(&skills, &["skill", "install", "--dir", &dir_arg]))
        .expect("the install");
    assert!(dir.join("demo-skill/SKILL.md").exists());

    let options = skill_embed_clap::options(&sub(&skills, &["skill", "list", "--dir", &dir_arg]))
        .expect("the options");
    let statuses = skills.status(&options).expect("status");
    assert_eq!(statuses[0].state, State::UpToDate);

    skill_embed_clap::run(&skills, &matches(&skills, &["skill", "remove", "--dir", &dir_arg]))
        .expect("the alias");
    assert!(!dir.join("demo-skill").exists());
}

#[test]
fn the_flags_are_the_ones_gh_skill_install_defines() {
    let skills = skills();
    let options = skill_embed_clap::options(&sub(
        &skills,
        &[
            "skill",
            "install",
            "--agent",
            "claude-code,cursor",
            "--scope",
            "user",
            "-f",
            "--dry-run",
            "demo-skill",
        ],
    ))
    .expect("the options");

    assert_eq!(options.agents.len(), 2);
    assert_eq!(options.scope, Some(skill_embed::Scope::User));
    assert!(options.force);
    assert!(options.dry_run);
    assert_eq!(options.names, ["demo-skill"]);
}

/// clap accepts a flag written after a positional argument, so
/// `skill install demo --dry-run` means what it looks like.
#[test]
fn a_flag_after_a_name_is_still_a_flag() {
    let skills = skills();
    let options =
        skill_embed_clap::options(&sub(&skills, &["skill", "install", "demo-skill", "--dry-run"]))
            .expect("the options");
    assert!(options.dry_run);
    assert_eq!(options.names, ["demo-skill"]);
}

/// Without `--agent`, the default is the installer's, and clap prints it.
#[test]
fn the_default_agents_come_from_the_installer() {
    let skills = skills();
    let options =
        skill_embed_clap::options(&sub(&skills, &["skill", "list"])).expect("the options");
    assert_eq!(options.agents, [skill_embed::AgentSelector::Detected]);

    let copilot = skills.with_default_agents([skill_embed::AgentSelector::from(
        &skill_embed::Agent::GITHUB_COPILOT,
    )]);
    let options =
        skill_embed_clap::options(&sub(&copilot, &["skill", "list"])).expect("the options");
    assert_eq!(options.agents, [skill_embed::AgentSelector::Named("github-copilot".to_owned())]);
}

#[test]
fn a_scope_clap_does_not_know_never_reaches_the_core() {
    let skills = skills();
    let root = Command::new("mytool").subcommand(skill_embed_clap::command(&skills));
    let err = root
        .try_get_matches_from(["mytool", "skill", "install", "--scope", "nowhere"])
        .expect_err("an unknown scope was accepted");
    assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
}

#[test]
fn a_blocked_destination_is_reported_and_refused() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let dir = tmp.path().join("skills");
    std::fs::create_dir_all(dir.join("demo-skill")).expect("a directory");
    std::fs::write(dir.join("demo-skill").join(skill_embed::SKILL_FILE), "hand written\n")
        .expect("a manifest");

    let skills = skills();
    let outcome = skill_embed_clap::run(
        &skills,
        &matches(&skills, &["skill", "install", "--dir", &dir.display().to_string()]),
    );
    assert!(matches!(outcome, Err(Error::NeedsForce(_))), "{outcome:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join("demo-skill").join(skill_embed::SKILL_FILE))
            .expect("the manifest"),
        "hand written\n"
    );
}

#[test]
fn the_help_names_the_tool_and_the_agents() {
    let skills = skills();
    let mut command = skill_embed_clap::command(&skills);
    let overview = command.render_long_help().to_string();
    assert!(overview.contains("Manage the agent skills embedded in mytool"), "{overview}");
    assert!(overview.contains("[alias: remove]"), "{overview}");

    let install = command
        .find_subcommand_mut("install")
        .expect("the install subcommand")
        .render_long_help()
        .to_string();
    assert!(install.contains("--agent"), "{install}");
    assert!(install.contains("github-copilot|claude-code"), "{install}");
    assert!(install.contains("[default: detected]"), "{install}");
}

/// `with_output` reaches this front end too, so a tool that redirects the
/// report gets it from both.
#[test]
fn the_report_goes_to_the_installers_own_writer() {
    #[derive(Clone, Default)]
    struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("the buffer").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let tmp = tempfile::tempdir().expect("a temporary directory");
    let dir = tmp.path().join("skills").display().to_string();
    let out = Captured::default();
    let skills = skills().with_output(out.clone());

    skill_embed_clap::run(&skills, &matches(&skills, &["skill", "install", "--dir", &dir]))
        .expect("the install");

    let text = String::from_utf8(out.0.lock().expect("the buffer").clone()).expect("UTF-8");
    assert!(text.starts_with("installed  demo-skill"), "{text}");
}
