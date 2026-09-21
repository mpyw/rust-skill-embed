//! The command the core front end gives a tool, and the text it prints.

mod common;

use common::{Captured, TempDir, captured, skills};
use expect_test::expect;
use skill_embed::{Error, Installer};

fn run(skills: &Installer, args: &[&str]) -> skill_embed::Result<()> {
    skills.run(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
}

fn intercept(skills: &Installer, argv: &[&str]) -> Option<std::process::ExitCode> {
    skills.intercept_args(&argv.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
}

fn demo() -> (Installer, Captured, Captured) {
    captured(skills(&["demo-skill"]))
}

#[test]
fn the_guard_is_the_first_argument_and_nothing_else() {
    let (skills, _, _) = demo();
    assert!(intercept(&skills, &["mylint", "./..."]).is_none());
    assert!(intercept(&skills, &["mylint", "-v", "skill", "install"]).is_none());
    assert!(intercept(&skills, &["mylint"]).is_none());
    assert!(intercept(&skills, &["mylint", "skill"]).is_some());
}

#[test]
fn the_command_name_can_be_changed() {
    let (skills, _, _) = demo();
    let skills = skills.with_command_name("agent-skills");
    assert!(intercept(&skills, &["mylint", "skill"]).is_none());
    assert!(intercept(&skills, &["mylint", "agent-skills"]).is_some());
}

#[test]
fn no_subcommand_prints_the_help() {
    let (skills, out, _) = demo();
    assert!(matches!(run(&skills, &[]), Err(Error::Help)));
    assert!(out.text().contains("Usage:"), "{}", out.text());
}

#[test]
fn an_unknown_subcommand_complains_on_the_error_stream() {
    let (skills, out, err) = demo();
    let Err(Error::Usage(message)) = run(&skills, &["instal"]) else {
        panic!("an unknown subcommand was accepted");
    };
    assert!(message.contains("instal"), "{message}");
    assert!(err.text().contains("Usage:"), "the usage did not go to the error stream");
    assert!(out.text().is_empty(), "the usage went to the report stream too");
}

/// `mytool skill list > skills.txt` puts the list in the file and the
/// complaint on the terminal.
#[test]
fn the_report_and_the_complaint_go_to_different_streams() {
    let tmp = TempDir::new("streams");
    let (skills, out, err) = demo();
    run(&skills, &["list", "--dir", &tmp.join("skills").display().to_string()])
        .expect("the listing");
    assert!(out.text().contains("demo-skill"), "{}", out.text());
    assert!(err.text().is_empty(), "a successful run wrote to the error stream");
}

#[test]
fn list_and_uninstall_answer_to_their_aliases() {
    let tmp = TempDir::new("aliases");
    let dir = tmp.join("skills").display().to_string();
    let (skills, out, _) = demo();
    run(&skills, &["install", "--dir", &dir]).expect("the install");
    run(&skills, &["ls", "--dir", &dir]).expect("ls");
    assert!(out.text().contains("up-to-date"), "{}", out.text());
    run(&skills, &["remove", "--dir", &dir]).expect("remove");
    assert!(!tmp.join("skills/demo-skill").exists(), "remove left the skill behind");
}

/// The `flag` package and urfave/cli v2 read a flag written after a positional
/// argument as a second name. Nothing here does.
#[test]
fn a_flag_after_a_name_is_still_a_flag() {
    let tmp = TempDir::new("flag-order");
    let dir = tmp.join("skills").display().to_string();
    let (skills, _, _) = demo();
    run(&skills, &["install", "demo-skill", "--dry-run", "--dir", &dir]).expect("the dry run");
    assert!(!tmp.join("skills").exists(), "a flag after a name was read as a name");
}

#[test]
fn a_double_dash_ends_the_flags() {
    let tmp = TempDir::new("double-dash");
    let dir = tmp.join("skills").display().to_string();
    let (skills, _, _) = demo();
    let Err(Error::UnknownSkill { name, .. }) =
        run(&skills, &["list", "--dir", &dir, "--", "--dry-run"])
    else {
        panic!("`--` did not end the flags");
    };
    assert_eq!(name, "--dry-run");
}

#[test]
fn an_unknown_flag_is_refused_with_the_usage() {
    let (skills, _, err) = demo();
    let Err(Error::Usage(message)) = run(&skills, &["install", "--agnet", "cursor"]) else {
        panic!("an unknown flag was accepted");
    };
    assert!(message.contains("--agnet"), "{message}");
    assert!(err.text().contains("Flags:"), "the usage did not go with the complaint");
}

#[test]
fn a_flag_that_needs_a_value_says_so() {
    let (skills, _, _) = demo();
    let Err(Error::Usage(message)) = run(&skills, &["install", "--agent"]) else {
        panic!("a flag with no value was accepted");
    };
    assert!(message.contains("needs a value"), "{message}");
}

#[test]
fn an_empty_scope_is_a_mistake_rather_than_a_default() {
    let (skills, _, _) = demo();
    assert!(matches!(run(&skills, &["install", "--scope="]), Err(Error::Usage(_))));
    assert!(matches!(run(&skills, &["install", "--scope", "nowhere"]), Err(Error::Usage(_))));
}

#[test]
fn an_unknown_agent_survives_the_front_end() {
    let (skills, _, _) = demo();
    let Err(Error::UnknownAgent { name, .. }) = run(&skills, &["install", "--agent", "nope"])
    else {
        panic!("an unknown agent was accepted");
    };
    assert_eq!(name, "nope");
}

#[test]
fn the_usage_hint_counts_the_skills() {
    let (one, _, _) = demo();
    assert_eq!(
        one.usage_hint(),
        r#"Run "mytool skill" to install the 1 agent skill embedded in mytool."#
    );
    let (two, _, _) = captured(skills(&["a-skill", "b-skill"]));
    assert_eq!(
        two.usage_hint(),
        r#"Run "mytool skill" to install the 2 agent skills embedded in mytool."#
    );
}

#[test]
fn the_help_describes_the_command() {
    let (skills, _, _) = demo();
    expect![[r"
        Manage the agent skills embedded in mytool.

        Usage:
          mytool skill install   [flags] [skill...]
          mytool skill uninstall [flags] [skill...]
          mytool skill list      [flags] [skill...]

        Flags:
              --agent <AGENT>  Target agent: {github-copilot|claude-code|cursor|codex|gemini|antigravity}, or all, or detected (repeatable) [default: detected]
              --dir <DIR>      Install to a custom directory (overrides --agent and --scope)
              --scope <SCOPE>  Installation scope: {project|user} [default: project]
          -f, --force          Overwrite existing skills
              --dry-run        Report what would happen without writing
          -h, --help           Show this help

        Embedded skills:
          demo-skill  A skill this test installs.
    "]]
    .assert_eq(&skills.usage());
}

/// Each subcommand's help names that subcommand, and install's names the
/// removal, because `--help` is the only surface many people read.
#[test]
fn each_subcommand_names_itself() {
    let (skills, out, _) = demo();
    assert!(matches!(run(&skills, &["install", "--help"]), Err(Error::Help)));
    expect![[r"
        Install the agent skills embedded in mytool, and remove the ones it no longer carries.

        Usage:
          mytool skill install [flags] [skill...]

    "]]
    .assert_eq(&out.text()[..=out.text().find("\nFlags:").expect("a flag block")]);

    let (skills, out, _) = demo();
    assert!(matches!(run(&skills, &["list", "--help"]), Err(Error::Help)));
    assert!(out.text().starts_with("Show the agent skills embedded in mytool"), "{}", out.text());
}
