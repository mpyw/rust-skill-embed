//! The places where this library and go-skill-embed had drifted apart.
//!
//! Each of these was a real difference found by reading the two side by side.
//! They are collected here because what holds them together is not a feature
//! but a promise: the two libraries do the same thing.

mod common;

use std::fs;

use common::{TempDir, captured, installer, into_dir, skills, testdata};
use skill_embed::{Error, InstallOptions, Installer, SKILL_FILE, State};

/// `std::env::args` panics on an argument that is not UTF-8, and `intercept`
/// runs before the tool has looked at its own command line. A file name the
/// tool would have handled took the whole process down.
#[test]
fn a_non_utf8_argument_is_passed_over_rather_than_panicked_on() {
    use std::ffi::OsString;

    let (skills, _, _) = captured(skills(&["demo-skill"]));
    let argv = [OsString::from("mylint"), common::not_unicode("file")];
    assert!(skills.intercept_args(&argv).is_none(), "an ordinary argument was intercepted");
}

/// A skill stamped `x-embedded-by: ""` is foreign to every run, including the
/// one that wrote it.
#[test]
fn an_empty_tool_name_keeps_the_default() {
    let skills = Installer::new(skills(&["demo-skill"])).with_tool_name("");
    assert!(!skills.tool_name().is_empty());
}

/// A key that is there and empty carries no claim, so it is no claim.
#[test]
fn an_empty_recorded_digest_is_foreign() {
    let tmp = TempDir::new("empty-digest");
    tmp.write(
        "skills/demo-skill/SKILL.md",
        "---\nname: demo-skill\nx-embedded-by: mytool\nx-embedded-digest: \"\"\n---\nbody\n",
    );
    let skills = installer(skills(&["demo-skill"]));
    let statuses = skills.status(&into_dir(&tmp.join("skills"))).expect("status");
    assert_eq!(statuses[0].state, State::Foreign);
}

/// A link standing in for an installed skill is one the agent reads through, so
/// it is described by what it points at.
#[test]
fn a_symlinked_destination_is_read_through() {
    if !common::symlinks_available() {
        return;
    }
    let tmp = TempDir::new("dest-link");
    let real = tmp.join("real");
    let skills = installer(skills(&["demo-skill"]));
    skills.install(&into_dir(&real)).1.expect("the first install");

    let linked = tmp.mkdir("linked");
    common::link_dir(&real.join("demo-skill"), &linked.join("demo-skill")).expect("the link");
    let statuses = skills.status(&into_dir(&linked)).expect("status");
    assert_eq!(statuses[0].state, State::UpToDate, "a link to an installation read as foreign");

    // A link that points nowhere is nothing at all.
    let dangling = tmp.mkdir("dangling");
    common::link_dir(&tmp.join("nowhere"), &dangling.join("demo-skill")).expect("the link");
    let statuses = skills.status(&into_dir(&dangling)).expect("status");
    assert_eq!(statuses[0].state, State::Missing);
}

/// The sweep removes what it finds, so it must not reach through a link the
/// user put there.
#[test]
fn the_sweep_does_not_reach_through_a_symlink() {
    if !common::symlinks_available() {
        return;
    }
    let tmp = TempDir::new("orphan-link");
    let store = tmp.join("store");
    installer(skills(&["gone-skill"])).install(&into_dir(&store)).1.expect("the store");

    let dest = tmp.mkdir("skills");
    common::link_dir(&store.join("gone-skill"), &dest.join("gone-skill")).expect("the link");

    let next = installer(skills(&["demo-skill"]));
    let (results, outcome) = next.install(&into_dir(&dest));
    outcome.expect("the install");
    assert!(
        !results.iter().any(|r| r.action == skill_embed::Action::Removed),
        "the sweep claimed a symlink: {results:?}"
    );
    assert!(dest.join("gone-skill").symlink_metadata().is_ok(), "the symlink was removed");
}

/// A row says what was done, so it is only added once that is true. Adding it
/// first made a failed write print `installed` above its own error.
#[test]
fn a_write_that_fails_is_not_reported_as_done() {
    let tmp = TempDir::new("failed-write");
    // The destination's parent cannot be created, because a file is in its way.
    tmp.write("in-the-way", "not a directory\n");
    let dest = tmp.join("in-the-way/skills");

    let skills = installer(skills(&["demo-skill"]));
    let (results, outcome) = skills.install(&into_dir(&dest));
    assert!(outcome.is_err(), "a write into a file succeeded");
    assert!(results.is_empty(), "a failed write was reported as done: {results:?}");
}

/// An empty value for either flag is a mistake, and only the front end knows
/// the flag was given at all.
#[test]
fn an_empty_agent_or_directory_is_refused() {
    let (skills, _, _) = captured(skills(&["demo-skill"]));
    let run = |args: &[&str]| skills.run(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>());

    assert!(
        matches!(run(&["install", "--agent", "", "--dry-run"]), Err(Error::Usage(_))),
        "an empty --agent was taken as the default"
    );
    assert!(
        matches!(run(&["install", "--dir", "", "--dry-run"]), Err(Error::Usage(_))),
        "an empty --dir was taken as the working directory"
    );

    // The library says the same thing to a caller that builds the options.
    let options = InstallOptions { dir: Some(String::new().into()), ..Default::default() };
    assert!(matches!(skills.targets(&options), Err(Error::Usage(_))));
}

/// The check exists to catch a file shipped *inside* a skill. One beside the
/// skills is never installed, and failing over it would crash a user's binary.
#[test]
fn an_installed_skill_keeps_its_own_manifest_bytes() {
    let tmp = TempDir::new("bytes");
    let dest = tmp.join("skills");
    let skills = installer(testdata());
    let options = InstallOptions { names: vec!["demo-skill".to_owned()], ..into_dir(&dest) };
    skills.install(&options).1.expect("the install");

    let installed = fs::read_to_string(dest.join("demo-skill").join(SKILL_FILE)).expect("read");
    assert!(installed.contains("license: MIT"), "a field the tool does not know was lost");
    assert!(installed.contains("# Demo skill"), "the body was lost");
}

/// The write stages into a fresh directory and creates each file new, so a
/// second file at one path would fail half way through a run with nothing but
/// "file exists". The set is where the path can be named.
#[test]
fn two_files_at_one_path_are_refused_with_the_path() {
    let files = [
        skill_embed::File::new("demo/SKILL.md", b"---\nname: demo\n---\nbody\n".to_vec()),
        skill_embed::File::new("demo/dup.md", b"first\n".to_vec()),
        skill_embed::File::new("demo/dup.md", b"second\n".to_vec()),
    ];
    let Err(Error::Skills(message)) = skill_embed::SkillSet::from_files(files) else {
        panic!("one path was given twice and accepted");
    };
    assert!(message.contains("demo/dup.md"), "{message}");
}

/// A lossy `--dir` names a different directory, and two distinct names collide
/// on one replacement character. The command refuses what it cannot read.
#[test]
fn an_argument_this_command_cannot_read_is_refused() {
    use std::ffi::OsString;

    let (skills, _, err) = captured(skills(&["demo-skill"]));
    let argv = [
        OsString::from("mylint"),
        OsString::from("skill"),
        OsString::from("list"),
        OsString::from("--dir"),
        common::not_unicode("a-directory"),
    ];
    assert_eq!(skills.intercept_args(&argv), Some(std::process::ExitCode::FAILURE));
    assert!(err.text().contains("is not UTF-8"), "{}", err.text());
}
