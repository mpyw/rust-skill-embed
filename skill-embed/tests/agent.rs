//! The agent table, and the vocabulary `--agent` takes.

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, installer, skills};
use skill_embed::{Agent, AgentSelector, Error, InstallOptions, Scope};

/// The directories match `gh skill install`. A change here is a change in where
/// a user's skills land, so the table is written out rather than derived.
#[test]
fn the_built_in_agents_match_gh_skill_install() {
    let expected = [
        ("github-copilot", "GitHub Copilot", ".agents/skills"),
        ("claude-code", "Claude Code", ".claude/skills"),
        ("cursor", "Cursor", ".agents/skills"),
        ("codex", "Codex", ".agents/skills"),
        ("gemini", "Gemini CLI", ".agents/skills"),
        ("antigravity", "Antigravity", ".agents/skills"),
    ];
    let builtins = Agent::builtins();
    assert_eq!(builtins.len(), expected.len());
    for (agent, (name, title, project_dir)) in builtins.iter().zip(expected) {
        assert_eq!(agent.name(), name);
        assert_eq!(agent.title(), title);
        assert_eq!(agent.project_dir(), project_dir);
    }
}

/// A title is for a human to read in a listing, not for a flag to take.
#[test]
fn a_title_is_not_a_flag_value() {
    for agent in Agent::builtins() {
        assert_ne!(agent.title(), agent.name(), "{} has no title of its own", agent.name());
    }
}

#[test]
fn project_scope_defaults_to_the_working_directory() {
    let dir = Agent::CLAUDE_CODE.dir(Scope::Project, None).expect("the directory");
    let here = std::env::current_dir().expect("a working directory");
    assert_eq!(dir, here.join(".claude/skills"));
}

#[test]
fn a_custom_agent_can_name_its_own_user_directory() {
    let tmp = TempDir::new("custom-agent");
    let elsewhere = tmp.join("elsewhere");
    let target = elsewhere.clone();
    let agent =
        Agent::new("mine", "Mine", ".mine/skills").with_user_dir(move || Ok(target.clone()));

    assert_eq!(agent.dir(Scope::User, None).expect("the user directory"), elsewhere);
    assert_eq!(
        agent.dir(Scope::Project, Some(Path::new("/repo"))).expect("the project directory"),
        PathBuf::from("/repo/.mine/skills")
    );
}

/// Without one the agent has project scope alone, and saying so is better than
/// writing into a directory nobody named.
#[test]
fn a_custom_agent_without_a_user_directory_says_so() {
    let agent = Agent::new("mine", "Mine", ".mine/skills");
    assert!(matches!(agent.dir(Scope::User, None), Err(Error::Io { .. })));
}

#[test]
fn a_selector_reaches_every_form() {
    let parse = |s: &str| AgentSelector::parse_list(s).collect::<Vec<_>>();
    assert_eq!(parse("all"), [AgentSelector::All]);
    assert_eq!(parse("detected"), [AgentSelector::Detected]);
    assert_eq!(
        parse("claude-code,cursor"),
        [AgentSelector::Named("claude-code".to_owned()), AgentSelector::Named("cursor".to_owned())]
    );
    // Empty entries are dropped, so `--agent ""` selects nothing rather than
    // naming an agent called "".
    assert!(parse("").is_empty());
    assert_eq!(parse(" cursor , ,codex ").len(), 2);

    assert_eq!(AgentSelector::from(&Agent::CLAUDE_CODE).to_string(), "claude-code");
    assert_eq!(AgentSelector::All.to_string(), "all");
}

#[test]
fn a_repeated_flag_and_a_comma_list_name_the_same_agents() {
    let tmp = TempDir::new("selectors");
    let root = tmp.mkdir("repo");
    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);

    let one = InstallOptions {
        agents: AgentSelector::parse_list("claude-code,cursor").collect(),
        ..Default::default()
    };
    let other = InstallOptions {
        agents: vec![
            AgentSelector::Named("claude-code".to_owned()),
            AgentSelector::Named("cursor".to_owned()),
        ],
        ..Default::default()
    };
    let dirs = |o| {
        skills.targets(o).expect("the targets").iter().map(|t| t.dir.clone()).collect::<Vec<_>>()
    };
    assert_eq!(dirs(&one), dirs(&other));
    assert_eq!(dirs(&one).len(), 2);
}

#[test]
fn naming_the_same_agent_twice_writes_once() {
    let tmp = TempDir::new("duplicate");
    let root = tmp.mkdir("repo");
    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    let options = InstallOptions {
        agents: AgentSelector::parse_list("cursor,codex,cursor").collect(),
        ..Default::default()
    };
    let targets = skills.targets(&options).expect("the targets");
    assert_eq!(targets.len(), 1, "two agents sharing a directory made two targets");
    assert_eq!(targets[0].agents.len(), 2);
}

#[test]
fn restricting_the_agents_restricts_what_is_offered() {
    let tmp = TempDir::new("restricted");
    let root = tmp.mkdir("repo");
    let skills = installer(skills(&["demo-skill"]))
        .with_agents([Agent::CLAUDE_CODE])
        .with_project_root(&root);

    assert_eq!(skills.agent_choices(), "{claude-code}");
    let options = InstallOptions {
        agents: vec![AgentSelector::Named("cursor".to_owned())],
        ..Default::default()
    };
    let Err(Error::UnknownAgent { name, valid }) = skills.targets(&options) else {
        panic!("an agent the tool does not offer was accepted");
    };
    assert_eq!(name, "cursor");
    assert_eq!(valid, ["claude-code"]);

    let all = InstallOptions { agents: vec![AgentSelector::All], ..Default::default() };
    assert_eq!(skills.targets(&all).expect("the targets").len(), 1);
}

/// `gh skill install` defaults to `github-copilot`, which writes only
/// `.agents/skills`. A tool that embeds its skills is rarely able to prompt,
/// so the default here is `detected`, and this is how the `gh` behaviour is
/// restored.
#[test]
fn the_default_agents_can_be_replaced() {
    let tmp = TempDir::new("default-agents");
    let root = tmp.mkdir("repo");
    let skills = installer(skills(&["demo-skill"]))
        .with_default_agents([AgentSelector::from(&Agent::GITHUB_COPILOT)])
        .with_project_root(&root);

    let targets = skills.targets(&InstallOptions::default()).expect("the targets");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].dir, root.join(".agents/skills"));
}

#[test]
fn the_default_scope_can_be_replaced() {
    let tmp = TempDir::new("default-scope");
    let root = tmp.mkdir("repo");
    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    assert_eq!(skills.default_scope(), Scope::Project);
    assert_eq!(skills.with_default_scope(Scope::User).default_scope(), Scope::User);
}

/// `--dir` overrides the agents, but a value that names no agent is still a
/// mistake, and a diagnosis is better than silence.
#[test]
fn a_bad_agent_beside_dir_is_still_a_mistake() {
    let tmp = TempDir::new("dir-and-agent");
    let skills = installer(skills(&["demo-skill"]));
    let options = InstallOptions {
        dir: Some(tmp.join("skills")),
        agents: vec![AgentSelector::Named("nope".to_owned())],
        ..Default::default()
    };
    assert!(matches!(skills.targets(&options), Err(Error::UnknownAgent { .. })));
}

#[test]
fn a_target_names_the_agents_that_read_from_it() {
    let tmp = TempDir::new("label");
    let root = tmp.mkdir("repo");
    let skills = installer(skills(&["demo-skill"])).with_project_root(&root);
    let options = InstallOptions {
        agents: AgentSelector::parse_list("cursor,codex").collect(),
        ..Default::default()
    };
    let label = skills.targets(&options).expect("the targets")[0].label();
    assert!(label.ends_with(" (Cursor, Codex)"), "{label}");

    // A named directory has no agents behind it, so it is named alone.
    let named = InstallOptions { dir: Some(tmp.join("skills")), ..Default::default() };
    let label = skills.targets(&named).expect("the targets")[0].label();
    assert!(!label.contains('('), "{label}");
}
