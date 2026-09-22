//! Where a run resolves to: the project root search, the two scopes, and the
//! bound that keeps a project install inside its project.
//!
//! Everything here reads the environment or the working directory, which are
//! one per process. The tests take a lock and put both back, so this file is
//! the only one that touches them.

#![allow(unsafe_code, reason = "the environment is process-wide, and `Env` is what serialises it")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use common::{TempDir, installer, skills};
use skill_embed::{Action, Agent, AgentSelector, Error, InstallOptions, Scope, State};

static LOCK: Mutex<()> = Mutex::new(());

/// The variable `std::env::home_dir` reads. Unix reads `HOME` and Windows reads
/// `USERPROFILE`, so a test that sets one of them sets nothing on the other.
const HOME_VAR: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

/// Holds the process-wide state for one test and puts it back afterwards.
struct Env {
    _guard: MutexGuard<'static, ()>,
    home: Option<String>,
    config: Option<String>,
    cwd: PathBuf,
}

impl Env {
    fn new() -> Self {
        Self {
            _guard: LOCK.lock().unwrap_or_else(PoisonError::into_inner),
            home: std::env::var(HOME_VAR).ok(),
            config: std::env::var("CLAUDE_CONFIG_DIR").ok(),
            cwd: std::env::current_dir().expect("a working directory"),
        }
    }

    fn set(key: &str, value: Option<&Path>) {
        // SAFETY: `LOCK` is held, and no other test binary shares this process.
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }

    fn home(&self, dir: &Path) -> &Self {
        Self::set(HOME_VAR, Some(dir));
        self
    }

    /// Moves to `dir` and answers with the working directory as the operating
    /// system gives it back.
    ///
    /// That is the path the project root search starts from, and it is not
    /// always the one the test passed in: macOS answers `/private/var/...`
    /// where the temporary directory was handed out as `/var/...`. Building
    /// the expected paths from the answer keeps the comparison honest without
    /// a test having to know which platform it is on.
    #[expect(
        clippy::unused_self,
        reason = "taking the guard is how a caller shows it holds the lock"
    )]
    fn cd(&self, dir: &Path) -> PathBuf {
        std::env::set_current_dir(dir).expect("the working directory");
        std::env::current_dir().expect("a working directory")
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for (key, value) in
            [(HOME_VAR, self.home.clone()), ("CLAUDE_CONFIG_DIR", self.config.clone())]
        {
            // SAFETY: as above, and this is the last use of the lock.
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
        let _ = std::env::set_current_dir(&self.cwd);
    }
}

fn claude_only() -> InstallOptions {
    InstallOptions { agents: vec![AgentSelector::from(&Agent::CLAUDE_CODE)], ..Default::default() }
}

#[test]
fn the_search_prefers_a_directory_that_already_holds_an_agent_directory() {
    let tmp = TempDir::new("root-marker");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    tmp.mkdir("repo/.git");
    tmp.mkdir("repo/sub/.claude");
    let deeper = env.cd(&tmp.mkdir("repo/sub/deeper"));
    let sub = deeper.parent().expect("repo/sub");

    let skills = installer(skills(&["demo-skill"]));
    let targets = skills.targets(&claude_only()).expect("the targets");
    assert_eq!(targets[0].root.as_deref(), Some(sub));
    assert_eq!(targets[0].dir, sub.join(".claude/skills"));
}

#[test]
fn the_search_stops_at_the_repository_root() {
    let tmp = TempDir::new("root-repo");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    // A marker above the repository, which the walk must not reach.
    tmp.mkdir(".claude");
    tmp.mkdir("repo/.git");
    let sub = env.cd(&tmp.mkdir("repo/sub"));
    let repo = sub.parent().expect("the repository root");

    let skills = installer(skills(&["demo-skill"]));
    let targets = skills.targets(&claude_only()).expect("the targets");
    assert_eq!(targets[0].root.as_deref(), Some(repo));
}

/// The user scope directories live there. A project installation written into
/// them would sit in front of every other project.
#[test]
fn the_search_refuses_to_land_on_the_home_directory() {
    let tmp = TempDir::new("root-home");
    let env = Env::new();
    let home = tmp.mkdir("home");
    env.home(&home);
    env.cd(&home);

    let skills = installer(skills(&["demo-skill"]));
    let Err(Error::ProjectIsHome { dir }) = skills.targets(&claude_only()) else {
        panic!("a project install landed on the home directory");
    };
    assert!(dir.ends_with("home"), "the refusal does not name where it landed: {}", dir.display());
    let message = Error::ProjectIsHome { dir }.to_string();
    assert!(message.contains("--scope user"), "{message}");
    assert!(message.contains("--dir"), "{message}");
}

#[test]
fn the_project_root_option_wins_over_the_search() {
    let tmp = TempDir::new("root-option");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let named = tmp.mkdir("named");
    env.cd(&tmp.mkdir("elsewhere"));

    let skills = installer(skills(&["demo-skill"])).with_project_root(&named);
    let targets = skills.targets(&claude_only()).expect("the targets");
    assert_eq!(targets[0].dir, named.join(".claude/skills"));
}

#[test]
fn five_of_the_six_agents_share_one_project_directory() {
    let tmp = TempDir::new("shared");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let root = tmp.mkdir("repo");

    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    let options = InstallOptions { agents: vec![AgentSelector::All], ..Default::default() };
    let targets = skills.targets(&options).expect("the targets");

    assert_eq!(targets.len(), 2, "{targets:#?}");
    let shared = targets
        .iter()
        .find(|t| t.dir == root.join(".agents/skills"))
        .expect("the shared directory");
    assert_eq!(shared.agents.len(), 5);
    assert!(shared.label().contains("GitHub Copilot"), "{}", shared.label());

    let (results, outcome) = skills.install(&options);
    outcome.expect("the install");
    assert_eq!(results.len(), 2, "a skill was written to one directory twice");
}

#[test]
fn user_scope_writes_under_the_home_directory() {
    let tmp = TempDir::new("user-scope");
    let env = Env::new();
    let home = tmp.mkdir("home");
    env.home(&home);
    Env::set("CLAUDE_CONFIG_DIR", None);

    let skills = installer(skills(&["demo-skill"]));
    let options = InstallOptions { scope: Some(Scope::User), ..claude_only() };
    let targets = skills.targets(&options).expect("the targets");
    assert_eq!(targets[0].dir, home.join(".claude/skills"));
    assert_eq!(targets[0].root, None, "user scope named a project root");
}

/// Claude Code moves its whole configuration with `CLAUDE_CONFIG_DIR`, and a
/// config dir may hold several separated roots. The first wins.
#[test]
fn claude_config_dir_is_honoured() {
    let tmp = TempDir::new("config-dir");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let first = tmp.mkdir("first");
    // The separator is `;` on Windows and `:` elsewhere. A Windows path holds a
    // `:` of its own, so a value joined with `:` there names one root and not
    // two.
    let separator = if cfg!(windows) { ';' } else { ':' };
    Env::set(
        "CLAUDE_CONFIG_DIR",
        Some(&PathBuf::from(format!(
            "{}{separator}{}",
            first.display(),
            tmp.join("second").display()
        ))),
    );

    let dir = Agent::CLAUDE_CODE.dir(Scope::User, None).expect("the user directory");
    assert_eq!(dir, first.join("skills"));
}

#[test]
fn detected_falls_back_to_every_agent() {
    let tmp = TempDir::new("detect");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let root = tmp.mkdir("repo");

    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    let detected = InstallOptions { agents: vec![AgentSelector::Detected], ..Default::default() };

    // Nothing is there, so every agent is written to.
    let targets = skills.targets(&detected).expect("the targets");
    assert_eq!(targets.len(), 2, "a fresh repository did not reach everything");

    // With one agent's directory present, it is the only one.
    tmp.mkdir("repo/.claude");
    let targets = skills.targets(&detected).expect("the targets");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].dir, root.join(".claude/skills"));
}

/// A link committed at `.claude/skills`, or at any directory above it, aims a
/// project install and a later forced removal wherever it points.
#[test]
fn a_project_install_stays_inside_the_project() {
    if !common::symlinks_available() {
        return;
    }
    let tmp = TempDir::new("escape");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let root = tmp.mkdir("repo");
    let outside = tmp.mkdir("outside");
    common::link_dir(&outside, &root.join(".claude")).expect("the link");

    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    let Err(Error::ProjectEscapes { real, .. }) = skills.targets(&claude_only()) else {
        panic!("a link took a project install out of the project");
    };
    assert!(real.starts_with(std::fs::canonicalize(&outside).expect("the real path")));
    assert!(
        Error::ProjectEscapes { dir: root, real }.to_string().contains("--dir"),
        "the refusal does not say how to write there on purpose"
    );
}

/// A repository that keeps its skills elsewhere in its own tree and links to
/// them is doing nothing wrong. What matters is where the link lands.
#[test]
fn a_link_that_stays_inside_the_project_is_followed() {
    if !common::symlinks_available() {
        return;
    }
    let tmp = TempDir::new("inside-link");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let root = tmp.mkdir("repo");
    tmp.mkdir("repo/shared");
    common::link_dir(&root.join("shared"), &root.join(".claude")).expect("the link");

    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    skills.targets(&claude_only()).expect("a link inside the project was refused");
}

/// One directory reached by two agent paths, which is what a repository does
/// when it links `.claude/skills` at `.agents/skills` so that every agent reads
/// one tree. `targets` merges destinations by path, so the two spellings are
/// two of them, and each skill is written twice.
///
/// The behaviour is pinned rather than merged. Both writes carry the same
/// bytes, so the second is a rewrite and nothing is lost, and the run after it
/// reads up to date at both. Merging by identity would print one destination,
/// under whichever spelling the agent order reached first. That is
/// `.agents/skills`, so the path the reader linked would appear nowhere.
#[test]
fn one_directory_reached_by_two_agent_paths() {
    if !common::symlinks_available() {
        return;
    }
    let tmp = TempDir::new("shared-dir");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let root = tmp.mkdir("repo");
    let shared = tmp.mkdir("repo/.agents/skills");
    tmp.mkdir("repo/.claude");
    let link = root.join(".claude/skills");
    common::link_dir(&shared, &link).expect("the link");

    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    let options = InstallOptions {
        agents: vec![AgentSelector::All],
        scope: Some(Scope::Project),
        ..Default::default()
    };

    let targets = skills.targets(&options).expect("the targets");
    let dirs: Vec<&Path> = targets.iter().map(|t| t.dir.as_path()).collect();
    assert_eq!(dirs, [shared.as_path(), link.as_path()], "one directory, two destinations");

    let (results, outcome) = skills.install(&options);
    outcome.expect("the install");
    assert_eq!(results.len(), 2, "one row per destination: {results:?}");
    for (r, dir) in results.iter().zip([&shared, &link]) {
        assert_eq!(r.action, Action::Installed);
        assert_eq!(r.path, dir.join("demo-skill"));
    }

    // Both rows are the same directory, and it holds one skill rather than two.
    let real = |p: &Path| std::fs::canonicalize(p).expect("the real path");
    assert_eq!(real(&results[0].path), real(&results[1].path));
    let mut entries: Vec<_> = std::fs::read_dir(&shared)
        .expect("the shared directory")
        .map(|e| e.expect("an entry").file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, ["demo-skill"], "the shared directory holds more than the one skill");
    assert!(
        shared.join("demo-skill/SKILL.md").is_file(),
        "the second write did not leave a readable skill"
    );

    // The rewrite is a rewrite: the run after it has nothing to do at either
    // spelling, so the doubled row never becomes a doubled change.
    let (results, outcome) = skills.install(&options);
    outcome.expect("the second install");
    for r in &results {
        assert_eq!(
            (r.before, r.action),
            (State::UpToDate, Action::Skipped),
            "{}",
            r.path.display()
        );
    }
}

/// It could be read as the embedding tool's own choice, the way `--dir` is. It
/// is refused with everything else, because a project install landing outside
/// the project is the thing being stopped and who wrote the path does not
/// change that.
#[test]
fn a_custom_agent_cannot_climb_out_of_the_project() {
    let tmp = TempDir::new("climb");
    let env = Env::new();
    env.home(&tmp.mkdir("home"));
    let root = tmp.mkdir("repo");

    let escaping = Agent::new("escaping", "Escaping", "../elsewhere/skills");
    let skills =
        installer(skills(&["demo-skill"])).with_agents([escaping]).with_project_root(&root);
    let options = InstallOptions { agents: vec![AgentSelector::All], ..Default::default() };
    assert!(matches!(skills.targets(&options), Err(Error::ProjectEscapes { .. })));
}

/// User scope and `--dir` are the user naming a place, so a home directory
/// moved with a link keeps working.
#[test]
fn user_scope_is_not_bounded_the_same_way() {
    if !common::symlinks_available() {
        return;
    }
    let tmp = TempDir::new("user-link");
    let env = Env::new();
    let real_home = tmp.mkdir("real-home");
    let linked = tmp.join("home");
    common::link_dir(&real_home, &linked).expect("the link");
    env.home(&linked);
    Env::set("CLAUDE_CONFIG_DIR", None);

    let skills = installer(skills(&["demo-skill"]));
    let options = InstallOptions { scope: Some(Scope::User), ..claude_only() };
    skills.install(&options).1.expect("a linked home directory was refused");
    assert!(real_home.join(".claude/skills/demo-skill/SKILL.md").exists());
}
