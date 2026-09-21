//! What install, uninstall and list do at a destination.

mod common;

use std::fs;
use std::path::Path;

use common::{TempDir, installer, into_dir, manifest, skill_files, skills, testdata};
use skill_embed::{Action, Error, InstallOptions, SkillSet, State, meta};

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Only the symlink test reads a state by name, and that test is Unix only.
#[cfg(unix)]
fn state_of(statuses: &[skill_embed::InstallStatus], name: &str) -> State {
    statuses.iter().find(|st| st.skill == name).unwrap_or_else(|| panic!("no row for {name}")).state
}

fn action_of(results: &[skill_embed::InstallResult], name: &str) -> Action {
    results.iter().find(|r| r.skill == name).unwrap_or_else(|| panic!("no row for {name}")).action
}

#[test]
fn install_lifecycle() {
    let tmp = TempDir::new("lifecycle");
    let dest = tmp.join("skills");
    let skills = installer(testdata());
    let options = InstallOptions { names: vec!["demo-skill".to_owned()], ..into_dir(&dest) };

    let (results, outcome) = skills.install(&options);
    outcome.expect("the first install");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].action, Action::Installed);

    let installed = dest.join("demo-skill/SKILL.md");
    let body = read(&installed);
    assert!(body.contains("Body text that must survive"), "body not copied:\n{body}");
    for key in [meta::EMBEDDED_BY, meta::EMBEDDED_DIGEST] {
        assert!(body.contains(&format!("{key}:")), "{key} missing:\n{body}");
    }

    // Installing again is a no-op.
    let statuses = skills.status(&options).expect("status");
    assert_eq!(statuses[0].state, State::UpToDate);
    let (results, outcome) = skills.install(&options);
    outcome.expect("the second install");
    assert_eq!(results[0].action, Action::Skipped);

    // An edit is noticed, and is not overwritten without --force.
    fs::write(&installed, body + "\nedited by hand\n").expect("the edit");
    let statuses = skills.status(&options).expect("status");
    assert_eq!(statuses[0].state, State::Modified);
    let (_, outcome) = skills.install(&options);
    assert!(
        matches!(outcome, Err(Error::NeedsForce(_))),
        "install overwrote an edited skill without --force"
    );

    let forced = InstallOptions { force: true, ..options.clone() };
    let (results, outcome) = skills.install(&forced);
    outcome.expect("the forced install");
    assert_eq!(results[0].action, Action::Updated);

    let (removed, outcome) = skills.uninstall(&options);
    outcome.expect("the uninstall");
    assert_eq!(removed[0].action, Action::Removed);
    assert!(!dest.join("demo-skill").exists(), "the skill directory survived uninstall");
}

#[test]
fn a_blocked_destination_does_not_stop_the_others() {
    let tmp = TempDir::new("blocked");
    let dest = tmp.join("skills");
    // Something else owns demo-skill's name.
    tmp.write("skills/demo-skill/SKILL.md", "hand written\n");

    let skills = installer(testdata());
    let (results, outcome) = skills.install(&into_dir(&dest));

    let Err(Error::NeedsForce(blocked)) = outcome else {
        panic!("outcome = {outcome:?}, want NeedsForce");
    };
    assert_eq!(blocked.blocked.len(), 1);
    assert_eq!(blocked.blocked[0].skill, "demo-skill");
    let message = blocked.to_string();
    assert!(message.contains("re-run with --force"), "{message}");

    assert_eq!(results.len(), 2);
    assert_eq!(action_of(&results, "demo-skill"), Action::Skipped);
    assert_eq!(action_of(&results, "bare-skill"), Action::Installed);
    assert!(dest.join("bare-skill/SKILL.md").exists(), "the unblocked skill was not written");
    assert_eq!(read(&dest.join("demo-skill/SKILL.md")), "hand written\n");
}

/// Anything in an installed directory that cannot be hashed, such as a symlink
/// a user dropped in, used to fail the whole run for every skill at that
/// target, `--force` included. The only way out was `rm -rf`.
#[cfg(unix)]
#[test]
fn an_unreadable_install_stays_repairable() {
    let tmp = TempDir::new("unreadable");
    let dest = tmp.join("skills");
    let skills = installer(testdata());
    let options = into_dir(&dest);

    skills.install(&options).1.expect("the first install");
    let link = dest.join("demo-skill/link.txt");
    std::os::unix::fs::symlink(tmp.join("elsewhere"), &link).expect("the symlink");

    let statuses = skills.status(&options).expect("status survives a symlink");
    assert_eq!(state_of(&statuses, "demo-skill"), State::Foreign);
    assert_eq!(
        state_of(&statuses, "bare-skill"),
        State::UpToDate,
        "an unrelated skill was dragged in"
    );

    let forced = InstallOptions { force: true, ..options };
    let (results, outcome) = skills.install(&forced);
    outcome.expect("a forced install survives a symlink");
    assert_eq!(action_of(&results, "demo-skill"), Action::Updated);
    assert!(link.symlink_metadata().is_err(), "the symlink survived a forced install");

    skills.uninstall(&forced).1.expect("a forced uninstall");
}

#[test]
fn a_skill_from_another_tool_is_left_alone() {
    let tmp = TempDir::new("foreign");
    let dest = tmp.join("skills");
    let theirs = installer(skills(&["demo-skill"])).with_tool_name("theirtool");
    theirs.install(&into_dir(&dest)).1.expect("their install");

    let mine = installer(skills(&["demo-skill"]));
    let statuses = mine.status(&into_dir(&dest)).expect("status");
    assert_eq!(statuses[0].state, State::Foreign);
    assert_eq!(statuses[0].installed_by.as_deref(), Some("theirtool"));
}

#[test]
fn dry_run_writes_nothing() {
    let tmp = TempDir::new("dry-run");
    let dest = tmp.join("skills");
    let skills = installer(testdata());
    let options = InstallOptions { dry_run: true, ..into_dir(&dest) };

    let (results, outcome) = skills.install(&options);
    outcome.expect("the dry run");
    assert!(results.iter().all(|r| r.action == Action::Installed));
    assert!(!dest.exists(), "a dry run created {}", dest.display());
}

#[test]
fn a_skill_without_frontmatter_installs_clean() {
    let tmp = TempDir::new("bare");
    let dest = tmp.join("skills");
    let skills = installer(testdata());
    let options = InstallOptions { names: vec!["bare-skill".to_owned()], ..into_dir(&dest) };

    skills.install(&options).1.expect("the install");
    let statuses = skills.status(&options).expect("status");
    assert_eq!(
        statuses[0].state,
        State::UpToDate,
        "a manifest that had no frontmatter reads as edited the moment it is installed"
    );
}

#[cfg(unix)]
#[test]
fn a_lost_executable_bit_is_repaired_without_force() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = TempDir::new("exec");
    let dest = tmp.join("skills");
    let skills = installer(testdata());
    let options = InstallOptions { names: vec!["demo-skill".to_owned()], ..into_dir(&dest) };
    skills.install(&options).1.expect("the install");

    let script = dest.join("demo-skill/scripts/run.sh");
    assert!(
        fs::metadata(&script).expect("the script").permissions().mode() & 0o111 != 0,
        "the shebang script was installed without the executable bit"
    );

    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).expect("the chmod");
    let statuses = skills.status(&options).expect("status");
    assert_eq!(
        statuses[0].state,
        State::Outdated,
        "a lost executable bit reads as edited, so repairing it would need --force"
    );

    let (results, outcome) = skills.install(&options);
    outcome.expect("the repair");
    assert_eq!(results[0].action, Action::Updated);
    assert!(fs::metadata(&script).expect("the script").permissions().mode() & 0o111 != 0);
}

#[test]
fn metadata_off_makes_everything_foreign() {
    let tmp = TempDir::new("no-metadata");
    let dest = tmp.join("skills");
    let skills = installer(skills(&["demo-skill"])).with_metadata(false);
    skills.install(&into_dir(&dest)).1.expect("the install");

    let body = read(&dest.join("demo-skill/SKILL.md"));
    assert!(!body.contains(meta::EMBEDDED_BY), "the stamp was written anyway:\n{body}");

    let statuses = skills.status(&into_dir(&dest)).expect("status");
    assert_eq!(statuses[0].state, State::Foreign);
}

#[cfg(unix)]
#[test]
fn the_executable_rule_decides_the_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = TempDir::new("exec-rule");
    let dest = tmp.join("skills");
    let set = SkillSet::from_files(skill_files(
        "demo-skill",
        &[("scripts/run", "no shebang here\n"), ("notes.md", "plain\n")],
    ))
    .expect("the fixture");
    let skills = installer(set).with_executable(|name, _| name.starts_with("scripts/"));
    skills.install(&into_dir(&dest)).1.expect("the install");

    let mode = |rel: &str| {
        fs::metadata(dest.join("demo-skill").join(rel)).expect(rel).permissions().mode() & 0o111
    };
    assert_ne!(mode("scripts/run"), 0, "the rule's file is not executable");
    assert_eq!(mode("notes.md"), 0, "a file the rule passed over is executable");
}

#[test]
fn naming_a_skill_that_is_not_embedded_is_refused() {
    let tmp = TempDir::new("unknown");
    let skills = installer(skills(&["demo-skill"]));
    let options =
        InstallOptions { names: vec!["nowhere".to_owned()], ..into_dir(&tmp.join("skills")) };
    let Err(Error::UnknownSkill { name, embedded }) = skills.status(&options) else {
        panic!("an unknown name was accepted");
    };
    assert_eq!(name, "nowhere");
    assert_eq!(embedded, ["demo-skill"]);
}

#[test]
fn names_differing_only_in_case_are_rejected() {
    // Two directories whose manifests claim one name, spelled two ways.
    let files = [
        skill_embed::File::new("demo/SKILL.md", manifest("Demo", "one").into_bytes()),
        skill_embed::File::new("other/SKILL.md", manifest("demo", "two").into_bytes()),
    ];
    let Err(Error::Skills(message)) = SkillSet::from_files(files) else {
        panic!("two spellings of one name were accepted");
    };
    assert!(message.contains("differ only in case"), "{message}");
}

#[test]
fn two_directories_claiming_one_name_are_rejected() {
    let files = [
        skill_embed::File::new("a/SKILL.md", manifest("demo", "one").into_bytes()),
        skill_embed::File::new("b/SKILL.md", manifest("demo", "two").into_bytes()),
    ];
    let Err(Error::Skills(message)) = SkillSet::from_files(files) else {
        panic!("one name was declared twice and accepted");
    };
    assert!(message.contains("both declare the skill name"), "{message}");
}

#[test]
fn an_embedded_junk_file_is_refused() {
    let mut files = skill_files("demo-skill", &[]);
    files.push(skill_embed::File::new("demo-skill/.DS_Store", b"\x00".to_vec()));
    let Err(Error::Skills(message)) = SkillSet::from_files(files) else {
        panic!("a file an operating system left behind was embedded");
    };
    assert!(message.contains(".DS_Store"), "{message}");
}

/// One that is not inside any skill ships too, and is never installed.
/// Failing the whole set over it would crash a user's binary over a file the
/// skills do not contain.
#[test]
fn a_junk_file_outside_every_skill_is_left_alone() {
    let mut files = skill_files("demo-skill", &[]);
    files.push(skill_embed::File::new(".DS_Store", b"\x00".to_vec()));
    files.push(skill_embed::File::new("demo-skill-notes/.DS_Store", b"\x00".to_vec()));
    let set = SkillSet::from_files(files).expect("junk outside a skill is not the skill's");
    assert_eq!(set.len(), 1);
}

#[test]
fn an_installed_junk_file_is_ignored() {
    let tmp = TempDir::new("junk");
    let dest = tmp.join("skills");
    let skills = installer(skills(&["demo-skill"]));
    skills.install(&into_dir(&dest)).1.expect("the install");

    // What opening the directory in a file browser leaves behind.
    fs::write(dest.join("demo-skill/.DS_Store"), b"\x00").expect("the junk file");

    let statuses = skills.status(&into_dir(&dest)).expect("status");
    assert_eq!(
        statuses[0].state,
        State::UpToDate,
        "opening the directory in a file browser made the skill read as edited"
    );
}

#[test]
fn a_root_manifest_reads_as_one_skill() {
    let set = SkillSet::from_files([skill_embed::File::new(
        "SKILL.md",
        manifest("only-skill", "The whole tree is one skill.").into_bytes(),
    )])
    .expect("the fixture");
    assert_eq!(set.len(), 1);
    assert_eq!(set.skills()[0].name(), "only-skill");
    assert_eq!(set.skills()[0].dir(), "");
}

#[test]
fn a_set_needs_a_skill() {
    let Err(Error::Skills(message)) =
        SkillSet::from_files([skill_embed::File::new("notes.md", b"nothing here".to_vec())])
    else {
        panic!("a tree with no manifest was accepted");
    };
    assert!(message.contains("SKILL.md"), "{message}");
}

#[test]
fn the_name_falls_back_to_the_directory() {
    let set = SkillSet::from_files([skill_embed::File::new(
        "from-the-directory/SKILL.md",
        b"no frontmatter at all\n".to_vec(),
    )])
    .expect("the fixture");
    assert_eq!(set.skills()[0].name(), "from-the-directory");
}

#[test]
fn a_skill_name_that_could_escape_is_refused() {
    for name in ["..", ".hidden", "a/b"] {
        let files =
            [skill_embed::File::new("dir/SKILL.md", manifest(name, "whatever").into_bytes())];
        assert!(
            matches!(SkillSet::from_files(files), Err(Error::Skills(_))),
            "the name {name:?} was accepted"
        );
    }
}

#[test]
fn cancellation_stops_between_skills() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let tmp = TempDir::new("cancel");
    let dest = tmp.join("skills");
    let cancel = Arc::new(AtomicBool::new(true));
    let skills = installer(skills(&["a-skill", "b-skill"]));
    let options = InstallOptions { cancel: Some(Arc::clone(&cancel)), ..into_dir(&dest) };

    let (results, outcome) = skills.install(&options);
    assert!(matches!(outcome, Err(Error::Cancelled)), "outcome = {outcome:?}");
    assert!(results.is_empty());
    assert!(!dest.exists(), "a cancelled run wrote something");

    cancel.store(false, Ordering::Relaxed);
    skills.install(&options).1.expect("the run after the flag is cleared");
}
