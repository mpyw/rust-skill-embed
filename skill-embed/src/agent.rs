use std::borrow::Cow;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{Error, Result, Scope, paths};

/// The directory agreed on by every agent except Claude Code.
const SHARED_PROJECT_DIR: &str = ".agents/skills";

/// What separates the roots in a variable that may name several.
///
/// It is one character per platform, and not both of them. Splitting on `:`
/// everywhere cut `C:\\Users\\...` at the drive letter, so `CLAUDE_CONFIG_DIR`
/// resolved to `C\\skills` on Windows.
const LIST_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

/// How an agent's user scope directory is found.
#[derive(Clone)]
enum UserDir {
    /// Under the home directory.
    Home(&'static [&'static str]),
    /// Under whatever an environment variable names, falling back to the home
    /// directory.
    EnvOrHome {
        env: &'static str,
        env_parts: &'static [&'static str],
        home_parts: &'static [&'static str],
    },
    /// Whatever the caller says.
    Custom(Arc<dyn Fn() -> io::Result<PathBuf> + Send + Sync>),
    /// The agent has none.
    None,
}

impl UserDir {
    fn resolve(&self) -> io::Result<PathBuf> {
        match self {
            Self::Home(parts) => Ok(join(paths::home()?, parts)),
            Self::EnvOrHome { env, env_parts, home_parts } => {
                // A config dir may hold several separated roots. The first wins.
                let root = std::env::var(env).ok().and_then(|v| {
                    let first = v.split(LIST_SEPARATOR).next().unwrap_or("").trim().to_owned();
                    (!first.is_empty()).then_some(first)
                });
                match root {
                    Some(root) => Ok(join(PathBuf::from(root), env_parts)),
                    None => Ok(join(paths::home()?, home_parts)),
                }
            }
            Self::Custom(f) => f(),
            Self::None => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the agent has no user scope directory",
            )),
        }
    }
}

fn join(mut base: PathBuf, parts: &[&str]) -> PathBuf {
    base.extend(parts);
    base
}

/// A coding agent that reads skills from a known directory.
///
/// The built-in agents mirror the directories used by `gh skill install`.
#[derive(Clone)]
pub struct Agent {
    name: Cow<'static, str>,
    title: Cow<'static, str>,
    project_dir: Cow<'static, str>,
    user_dir: UserDir,
}

impl Agent {
    /// GitHub Copilot.
    pub const GITHUB_COPILOT: Self = Self::builtin(
        "github-copilot",
        "GitHub Copilot",
        SHARED_PROJECT_DIR,
        UserDir::Home(&[".copilot", "skills"]),
    );
    /// Claude Code, which relocates its whole configuration with
    /// `CLAUDE_CONFIG_DIR`.
    pub const CLAUDE_CODE: Self = Self::builtin(
        "claude-code",
        "Claude Code",
        ".claude/skills",
        UserDir::EnvOrHome {
            env: "CLAUDE_CONFIG_DIR",
            env_parts: &["skills"],
            home_parts: &[".claude", "skills"],
        },
    );
    /// Cursor.
    pub const CURSOR: Self = Self::builtin(
        "cursor",
        "Cursor",
        SHARED_PROJECT_DIR,
        UserDir::Home(&[".cursor", "skills"]),
    );
    /// Codex.
    pub const CODEX: Self =
        Self::builtin("codex", "Codex", SHARED_PROJECT_DIR, UserDir::Home(&[".codex", "skills"]));
    /// Gemini CLI.
    pub const GEMINI: Self = Self::builtin(
        "gemini",
        "Gemini CLI",
        SHARED_PROJECT_DIR,
        UserDir::Home(&[".gemini", "skills"]),
    );
    /// Antigravity.
    pub const ANTIGRAVITY: Self = Self::builtin(
        "antigravity",
        "Antigravity",
        SHARED_PROJECT_DIR,
        UserDir::Home(&[".gemini", "antigravity", "skills"]),
    );

    /// Every built-in agent, in the order `gh skill install` documents them.
    ///
    /// The constants above and this list are two places, and the table in
    /// `tests/agent.rs` is what holds them together: it names every agent, its
    /// title and its project directory, and asserts the length, so an agent
    /// added to one and not the other is a failing test rather than an agent
    /// `--agent all` never reaches.
    #[must_use]
    pub fn builtins() -> Vec<Self> {
        vec![
            Self::GITHUB_COPILOT,
            Self::CLAUDE_CODE,
            Self::CURSOR,
            Self::CODEX,
            Self::GEMINI,
            Self::ANTIGRAVITY,
        ]
    }

    const fn builtin(
        name: &'static str,
        title: &'static str,
        project_dir: &'static str,
        user_dir: UserDir,
    ) -> Self {
        Self {
            name: Cow::Borrowed(name),
            title: Cow::Borrowed(title),
            project_dir: Cow::Borrowed(project_dir),
            user_dir,
        }
    }

    /// An agent this library does not know about.
    ///
    /// `project_dir` is relative to the project root and separated with `/`. A
    /// project install that would land outside the project root is refused
    /// whoever wrote the path, so this is not a way around that bound;
    /// [`Installer::with_project_root`] and `--dir` are.
    ///
    /// [`Installer::with_project_root`]: crate::Installer::with_project_root
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        title: impl Into<String>,
        project_dir: impl Into<String>,
    ) -> Self {
        Self {
            name: Cow::Owned(name.into()),
            title: Cow::Owned(title.into()),
            project_dir: Cow::Owned(project_dir.into()),
            user_dir: UserDir::None,
        }
    }

    /// Sets where user scope writes. Without one the agent has project scope
    /// alone.
    #[must_use]
    pub fn with_user_dir(
        mut self,
        f: impl Fn() -> io::Result<PathBuf> + Send + Sync + 'static,
    ) -> Self {
        self.user_dir = UserDir::Custom(Arc::new(f));
        self
    }

    /// The `--agent` value, such as `claude-code`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The human readable name, such as `Claude Code`.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The skills directory relative to the project root, separated with `/`.
    #[must_use]
    pub fn project_dir(&self) -> &str {
        &self.project_dir
    }

    /// Resolves the skills directory for a scope.
    ///
    /// `project_root` applies to [`Scope::Project`] alone, and [`None`] means
    /// the working directory.
    ///
    /// # Errors
    ///
    /// Fails when the home directory is not known, or when the agent has no
    /// user scope directory.
    pub fn dir(&self, scope: Scope, project_root: Option<&Path>) -> Result<PathBuf> {
        match scope {
            Scope::Project => {
                let root = match project_root {
                    Some(root) => root.to_path_buf(),
                    None => std::env::current_dir()
                        .map_err(|e| Error::io("read the working directory", e))?,
                };
                Ok(join(root, &self.project_dir.split('/').collect::<Vec<_>>()))
            }
            Scope::User => self.user_dir.resolve().map_err(|e| {
                Error::io(format!("resolve the user scope directory for {}", self.name), e)
            }),
        }
    }
}

impl fmt::Debug for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Agent")
            .field("name", &self.name)
            .field("title", &self.title)
            .field("project_dir", &self.project_dir)
            .finish_non_exhaustive()
    }
}

/// Names one agent, or a group of them, as `--agent` accepts it.
///
/// The two group words cannot be agent names, which is why this is its own
/// type rather than a list of [`Agent`].
#[derive(Clone, Debug, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum AgentSelector {
    /// Every agent the installer offers, present or not.
    All,
    /// The agents whose directory is already there. Falls back to
    /// [`AgentSelector::All`] when it finds none.
    Detected,
    /// One agent, by name.
    #[strum(to_string = "{0}")]
    Named(String),
}

impl AgentSelector {
    /// Reads a `--agent` value, which may be a comma separated list.
    ///
    /// Empty entries are dropped, so `--agent ""` selects nothing rather than
    /// naming an agent called "".
    pub fn parse_list(value: &str) -> impl Iterator<Item = Self> + '_ {
        value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| match s {
            "all" => Self::All,
            "detected" => Self::Detected,
            name => Self::Named(name.to_owned()),
        })
    }
}

impl From<&Agent> for AgentSelector {
    fn from(a: &Agent) -> Self {
        Self::Named(a.name().to_owned())
    }
}

/// Renders the `--agent` help string, such as `{a|b|c}`.
pub(crate) fn choices(agents: &[Agent]) -> String {
    let names: Vec<&str> = agents.iter().map(Agent::name).collect();
    format!("{{{}}}", names.join("|"))
}

/// Maps `--agent` values to agents.
///
/// [`AgentSelector::All`] expands to every agent the installer offers.
/// [`AgentSelector::Detected`] keeps the ones whose directory already exists.
///
/// An empty `values` is "nothing was asked for", which the caller tells apart
/// from "what was asked for resolved to nothing".
pub(crate) fn resolve(
    known: &[Agent],
    values: &[AgentSelector],
    detected: &dyn Fn(&Agent) -> bool,
) -> Result<Vec<Agent>> {
    let mut out: Vec<Agent> = Vec::new();
    let push = |a: &Agent, out: &mut Vec<Agent>| {
        if !out.iter().any(|k| k.name() == a.name()) {
            out.push(a.clone());
        }
    };
    for v in values {
        match v {
            AgentSelector::All => {
                for a in known {
                    push(a, &mut out);
                }
            }
            AgentSelector::Detected => {
                for a in known.iter().filter(|a| detected(a)) {
                    push(a, &mut out);
                }
            }
            AgentSelector::Named(name) => {
                let Some(a) = known.iter().find(|a| a.name() == name) else {
                    let mut valid: Vec<String> =
                        known.iter().map(|a| a.name().to_owned()).collect();
                    valid.sort();
                    return Err(Error::UnknownAgent { name: name.clone(), valid });
                };
                push(a, &mut out);
            }
        }
    }
    Ok(out)
}

/// The answer when [`AgentSelector::Detected`] found nothing.
///
/// On a machine with no agent directory any guess is as good as another, and
/// writing nothing would read as a failure.
pub(crate) fn fall_back_to_all(
    known: &[Agent],
    resolved: Vec<Agent>,
    values: &[AgentSelector],
) -> Vec<Agent> {
    if resolved.is_empty() && values.contains(&AgentSelector::Detected) {
        return known.to_vec();
    }
    resolved
}
