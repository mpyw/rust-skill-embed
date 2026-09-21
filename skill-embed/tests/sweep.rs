//! Removing what this tool wrote and the binary no longer carries.
//!
//! Nothing else finds those directories. Every other walk starts from the
//! embedded set, so a skill dropped between two versions would be installed
//! once and then read by the agent for good.

mod common;

use std::fs;
use std::path::Path;

use common::{TempDir, installer, into_dir, skills};
use skill_embed::{Action, Error, InstallOptions, SKILL_FILE, State};

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the destination");
    for entry in fs::read_dir(from).expect("the source") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("the copy");
        }
    }
}

fn row<'a>(
    results: &'a [skill_embed::InstallResult],
    name: &str,
) -> Option<&'a skill_embed::InstallResult> {
    results.iter().find(|r| r.skill == name)
}

/// The set before and after a release that drops `bare-skill`.
fn before() -> skill_embed::SkillSet {
    skills(&["bare-skill", "demo-skill"])
}
fn after() -> skill_embed::SkillSet {
    skills(&["demo-skill"])
}

#[test]
fn an_upgrade_removes_a_skill_the_binary_no_longer_carries() {
    let tmp = TempDir::new("sweep-upgrade");
    let dest = tmp.join("skills");
    let options = into_dir(&dest);
    installer(before()).install(&options).1.expect("the first install");
    assert!(dest.join("bare-skill").exists(), "the first install did not land");

    let next = installer(after());

    // list reports it.
    let statuses = next.status(&options).expect("status");
    let orphan = statuses.iter().find(|st| st.skill == "bare-skill").expect("a row");
    assert_eq!(orphan.state, State::Orphaned);
    assert_eq!(orphan.installed_by.as_deref(), Some("mytool"));

    // A named run does not sweep.
    let named = InstallOptions { names: vec!["demo-skill".to_owned()], ..options.clone() };
    next.install(&named).1.expect("the named run");
    assert!(
        dest.join("bare-skill").exists(),
        "`install demo-skill` removed a skill it was not asked about"
    );

    // A dry run reports the removal without making it.
    let dry = InstallOptions { dry_run: true, ..options.clone() };
    let (results, outcome) = next.install(&dry);
    outcome.expect("the dry run");
    assert_eq!(row(&results, "bare-skill").expect("a row").action, Action::Removed);
    assert!(dest.join("bare-skill").exists(), "the dry run removed it");

    // A full install sweeps.
    let (results, outcome) = next.install(&options);
    outcome.expect("the full install");
    let swept = row(&results, "bare-skill").expect("a row");
    assert_eq!(swept.action, Action::Removed);
    assert_eq!(swept.reason.as_deref(), Some("no longer embedded in mytool"));
    assert!(!dest.join("bare-skill").exists(), "the dropped skill survived");
    assert!(dest.join("demo-skill").exists(), "the skill still carried was removed too");
}

/// `uninstall` means everything this tool put there, including what it no
/// longer carries. Otherwise a dropped skill can never be reached again.
#[test]
fn uninstall_takes_orphans_with_it() {
    let tmp = TempDir::new("sweep-uninstall");
    let dest = tmp.join("skills");
    let options = into_dir(&dest);
    installer(before()).install(&options).1.expect("the first install");

    installer(after()).uninstall(&options).1.expect("the uninstall");
    for name in ["demo-skill", "bare-skill"] {
        assert!(!dest.join(name).exists(), "uninstall left {name} behind");
    }
}

/// The sweep claims a directory on this tool's own stamp with a digest that
/// still matches, and on nothing weaker. Everything else in the skills
/// directory has to survive it, with or without `--force`.
#[test]
fn the_sweep_claims_only_this_tools_own_work() {
    let tmp = TempDir::new("sweep-claims");
    let dest = tmp.join("skills");
    let options = into_dir(&dest);
    installer(before()).install(&options).1.expect("the first install");

    fs::create_dir_all(dest.join("handwritten")).expect("a directory");
    fs::write(dest.join("handwritten").join(SKILL_FILE), "---\nname: handwritten\n---\nmine\n")
        .expect("a manifest");
    fs::create_dir_all(dest.join("not-a-skill")).expect("a directory");
    fs::write(dest.join("notes.md"), "mine\n").expect("a file");

    // A second tool built on this library, writing its own skills. Its stamps
    // are as well formed as this one's and its digests check out, so
    // `x-embedded-by` is the only thing between its work and this sweep. A
    // hand-made fixture with a bogus digest does not test that: the digest
    // would turn it away first, whether or not the name was ever read.
    let other_dir = tmp.join("othertool");
    installer(skills(&["othertools-skill"]))
        .with_tool_name("othertool")
        .install(&into_dir(&other_dir))
        .1
        .expect("the other tool's install");
    copy_tree(&other_dir.join("othertools-skill"), &dest.join("othertools-skill"));

    let forced = InstallOptions { force: true, ..options };
    installer(after()).install(&forced).1.expect("the forced install");

    for name in ["handwritten", "othertools-skill", "not-a-skill", "notes.md"] {
        assert!(dest.join(name).exists(), "the sweep took {name}, which is not this tool's");
    }
}

/// A directory this tool wrote and somebody has since edited is no longer what
/// this tool left there, which is exactly what a shipped skill copied and then
/// made into one of the user's own looks like. Nothing is being installed over
/// it, so there is no conflict for `--force` to resolve, and `--force` must not
/// reach it: the tool asks for `--force` whenever anything is modified, so a
/// sweep that escalated with it would destroy that work on the tool's own
/// advice.
#[test]
fn an_edited_fork_is_never_swept() {
    let tmp = TempDir::new("sweep-fork");
    let dest = tmp.join("skills");
    let options = into_dir(&dest);
    installer(before()).install(&options).1.expect("the first install");

    let fork = dest.join("bare-skill-mine");
    copy_tree(&dest.join("bare-skill"), &fork);
    let body = fs::read_to_string(fork.join(SKILL_FILE)).expect("the fork's manifest");
    fs::write(fork.join(SKILL_FILE), body + "\nMy own additions.\n").expect("the edit");

    let next = installer(after());
    for force in [false, true] {
        let o = InstallOptions { force, ..options.clone() };
        for (label, results) in
            [("install", next.install(&o).0), ("uninstall", next.uninstall(&o).0)]
        {
            assert!(
                !results.iter().any(|r| r.path == fork),
                "{label} (force={force}) reported the fork"
            );
            assert!(fork.join(SKILL_FILE).exists(), "{label} (force={force}) took the fork");
        }
    }
}

/// A file system that folds case answers to more than one spelling, so a skill
/// renamed to another spelling of itself reads as an embedded row and as an
/// orphan at once, and both rows are the same directory. Removing the orphan
/// after writing the skill deleted what the run had just installed, and said
/// "updated" and "removed" on its way to reporting success.
#[test]
fn a_spelling_of_an_installed_skill_is_not_swept_away() {
    let tmp = TempDir::new("sweep-case");
    let probe = tmp.mkdir("CaseProbe");
    let folds = tmp.join("caseprobe").exists();
    fs::remove_dir_all(probe).expect("the probe");
    if !folds {
        eprintln!("this file system does not fold case, so the two names are two directories");
        return;
    }

    let dest = tmp.join("skills");
    let options = into_dir(&dest);
    let skills = installer(before());
    skills.install(&options).1.expect("the first install");
    fs::rename(dest.join("bare-skill"), dest.join("Bare-Skill")).expect("the rename");

    let (results, outcome) = skills.install(&options);
    outcome.expect("the second install");
    assert!(
        !results.iter().any(|r| r.action == Action::Removed),
        "the run removed the skill it had just installed: {results:?}"
    );
    assert!(dest.join("bare-skill").join(SKILL_FILE).exists(), "the installed skill is gone");
}

/// When a write cannot put the new tree in place it leaves the old one beside
/// the destination and names it in the error, for the user to go and recover.
/// It is a verbatim copy of an installation, stamp and digest and all, so the
/// sweep would otherwise claim it and take away the only copy.
#[test]
fn the_sweep_leaves_a_rescued_installation_alone() {
    let tmp = TempDir::new("sweep-rescue");
    let dest = tmp.join("skills");
    let options = into_dir(&dest);
    installer(before()).install(&options).1.expect("the first install");

    let rescued = dest.join(".bare-skill.old-1234");
    fs::rename(dest.join("bare-skill"), &rescued).expect("the rename");

    let (results, outcome) = installer(after()).install(&options);
    outcome.expect("the install");
    assert!(!results.iter().any(|r| r.path == rescued), "the sweep reported the rescue copy");
    assert!(
        rescued.join(SKILL_FILE).exists(),
        "the rescue copy the error told the user to recover is gone"
    );
}

/// `list` prints an orphan's name, so `uninstall` has to accept it. Resolving
/// names against the embedded set alone told the user that a skill the tool had
/// just listed does not exist.
#[test]
fn an_orphan_can_be_named() {
    let tmp = TempDir::new("sweep-named");
    let dest = tmp.join("skills");
    let options = into_dir(&dest);
    installer(before()).install(&options).1.expect("the first install");

    let next = installer(after());
    let named = InstallOptions { names: vec!["bare-skill".to_owned()], ..options.clone() };
    let (results, outcome) = next.uninstall(&named);
    outcome.expect("naming an orphan");
    assert_eq!(row(&results, "bare-skill").expect("a row").action, Action::Removed);
    assert!(!dest.join("bare-skill").exists(), "the named orphan survived");
    assert!(dest.join("demo-skill").exists(), "a skill that was not named was removed");

    let unknown = InstallOptions { names: vec!["never-existed".to_owned()], ..options };
    assert!(matches!(next.status(&unknown), Err(Error::UnknownSkill { .. })));
}
