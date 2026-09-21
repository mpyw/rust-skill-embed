use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use crate::error::ForceRequired;
use crate::tree::{self, Tree};
use crate::{
    Action, Agent, AgentSelector, Error, Result, SKILL_FILE, Scope, Skill, SkillSet, State, agent,
    manifest, paths, projectroot,
};

/// Installs embedded skills into agent directories.
///
/// Everything is set once, on the way in:
///
/// ```no_run
/// use std::sync::LazyLock;
///
/// use include_dir::{Dir, include_dir};
/// use skill_embed::{Installer, SkillSet};
///
/// static SKILLS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/skills");
///
/// static SKILLS: LazyLock<Installer> = LazyLock::new(|| {
///     Installer::new(SkillSet::from_include_dir(&SKILLS_DIR).expect("valid skills"))
///         .with_tool_name("mytool")
///         .with_version(env!("CARGO_PKG_VERSION"))
/// });
/// ```
pub struct Installer {
    set: SkillSet,
    tool_name: String,
    version: String,
    command_name: String,
    agents: Vec<Agent>,
    default_agents: Vec<AgentSelector>,
    default_scope: Scope,
    project_root: Option<PathBuf>,
    metadata: bool,
    executable: Arc<tree::ExecutableRule>,
    out: Mutex<Box<dyn Write + Send>>,
    err_out: Mutex<Box<dyn Write + Send>>,
}

impl Installer {
    /// Creates an installer for a set of embedded skills.
    ///
    /// The tool name defaults to the running binary's name, the subcommand to
    /// `skill`, the scope to [`Scope::Project`] and the agents to `detected`.
    #[must_use]
    pub fn new(set: SkillSet) -> Self {
        Self {
            set,
            tool_name: binary_name(),
            version: String::new(),
            command_name: "skill".to_owned(),
            agents: Agent::builtins(),
            default_agents: vec![AgentSelector::Detected],
            default_scope: Scope::Project,
            project_root: None,
            metadata: true,
            executable: Arc::new(tree::has_shebang),
            out: Mutex::new(Box::new(io::stdout())),
            err_out: Mutex::new(Box::new(io::stderr())),
        }
    }

    /// Sets the name recorded in installed skills and shown in help.
    ///
    /// An empty name is not a name, and is passed over. A skill stamped
    /// `x-embedded-by: ""` is foreign to every run, including this tool's own.
    #[must_use]
    pub fn with_tool_name(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if !name.is_empty() {
            self.tool_name = name;
        }
        self
    }

    /// Sets the version recorded in installed skills.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Sets the subcommand name [`Installer::run`] and [`Installer::intercept`]
    /// answer to. It defaults to `skill`.
    #[must_use]
    pub fn with_command_name(mut self, name: impl Into<String>) -> Self {
        self.command_name = name.into();
        self
    }

    /// Restricts the agents the tool offers. It defaults to every built-in one.
    #[must_use]
    pub fn with_agents(mut self, agents: impl IntoIterator<Item = Agent>) -> Self {
        self.agents = agents.into_iter().collect();
        self
    }

    /// Sets the agents used when `--agent` is not given. It defaults to
    /// [`AgentSelector::Detected`].
    ///
    /// `detected` keeps the agents whose directory is already there, and falls
    /// back to `all` when it finds none. In a fresh repository that is two
    /// directories and reaches everything. In a home directory it is the agents
    /// in use, rather than six directories of which most are litter. Pass
    /// [`Agent::GITHUB_COPILOT`] for the default `gh skill install` uses.
    #[must_use]
    pub fn with_default_agents(mut self, agents: impl IntoIterator<Item = AgentSelector>) -> Self {
        self.default_agents = agents.into_iter().collect();
        self
    }

    /// Sets the scope used when `--scope` is not given. It defaults to
    /// [`Scope::Project`], matching `gh skill install`.
    #[must_use]
    pub fn with_default_scope(mut self, scope: Scope) -> Self {
        self.default_scope = scope;
        self
    }

    /// Sets the directory project scope resolves against.
    ///
    /// Without it the root is searched for: the walk starts at the working
    /// directory and stops at the repository root, and the first directory
    /// already holding an agent directory wins. Outside a repository the
    /// working directory is the only candidate. The home directory is refused,
    /// since the user scope directories live there.
    #[must_use]
    pub fn with_project_root(mut self, dir: impl Into<PathBuf>) -> Self {
        self.project_root = Some(dir.into());
        self
    }

    /// Controls whether installed skills carry `x-embedded-*` frontmatter. It
    /// is on by default.
    ///
    /// Without it, install cannot tell an outdated copy from an edited one and
    /// every existing directory reads as [`State::Foreign`].
    #[must_use]
    pub fn with_metadata(mut self, on: bool) -> Self {
        self.metadata = on;
        self
    }

    /// Decides which files are written with the executable bit.
    ///
    /// An embedded file carries no mode, so the default marks any file starting
    /// with a `#!` shebang.
    #[must_use]
    pub fn with_executable(
        mut self,
        rule: impl Fn(&str, &[u8]) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.executable = Arc::new(rule);
        self
    }

    /// Sets where [`Installer::run`] writes what was asked for: the report, and
    /// the help text when help was requested. It defaults to standard output.
    #[must_use]
    pub fn with_output(mut self, w: impl Write + Send + 'static) -> Self {
        self.out = Mutex::new(Box::new(w));
        self
    }

    /// Sets where [`Installer::run`] writes diagnostics: the message for a bad
    /// flag or an unknown subcommand, and the usage that goes with it. It
    /// defaults to standard error.
    ///
    /// The two are separate for the sake of redirection. `mytool skill list >
    /// skills.txt` puts the list in the file and the complaint on the terminal.
    #[must_use]
    pub fn with_error_output(mut self, w: impl Write + Send + 'static) -> Self {
        self.err_out = Mutex::new(Box::new(w));
        self
    }

    /// The subcommand name.
    #[must_use]
    pub fn command_name(&self) -> &str {
        &self.command_name
    }

    /// The name recorded in installed skills.
    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// The scope used when none is given.
    #[must_use]
    pub fn default_scope(&self) -> Scope {
        self.default_scope
    }

    /// The agents used when `--agent` is not given.
    #[must_use]
    pub fn default_agents(&self) -> &[AgentSelector] {
        &self.default_agents
    }

    /// The skills the binary carries.
    #[must_use]
    pub fn skills(&self) -> &SkillSet {
        &self.set
    }

    /// Renders the `--agent` help string, such as `{a|b|c}`.
    ///
    /// An adapter that writes its own flag help uses it to name the same
    /// agents.
    #[must_use]
    pub fn agent_choices(&self) -> String {
        agent::choices(&self.agents)
    }

    /// Writes a report to wherever [`Installer::with_output`] points.
    ///
    /// A front end that renders its own output calls it, so that one setting
    /// reaches every one of them. [`crate::render_results`] and
    /// [`crate::render_status`] are what it is given.
    ///
    /// # Errors
    ///
    /// Whatever the writer refused.
    pub fn write_report(&self, text: &str) -> Result<()> {
        self.write_out(text).map_err(|e| Error::io("write the report", e))
    }

    pub(crate) fn write_out(&self, s: &str) -> io::Result<()> {
        let mut w = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        w.write_all(s.as_bytes())?;
        w.flush()
    }

    pub(crate) fn write_err(&self, s: &str) -> io::Result<()> {
        let mut w = self.err_out.lock().unwrap_or_else(PoisonError::into_inner);
        w.write_all(s.as_bytes())?;
        w.flush()
    }

    /// What project scope resolves against.
    ///
    /// [`Installer::with_project_root`] wins, and without it the root is
    /// searched for.
    fn project_root_of(&self, scope: Scope) -> Result<Option<PathBuf>> {
        if scope != Scope::Project || self.project_root.is_some() {
            return Ok(self.project_root.clone());
        }
        let wd = std::env::current_dir().map_err(|e| Error::io("read the working directory", e))?;
        // Derived from the agent table rather than written out, so a seventh
        // agent is searched for without touching this. Five of the six share
        // `.agents`, so the list is deduplicated before it becomes one question
        // per entry.
        let mut markers: Vec<PathBuf> = Vec::new();
        for a in &self.agents {
            let marker = PathBuf::from(a.project_dir().replace('/', std::path::MAIN_SEPARATOR_STR));
            let Some(parent) = marker.parent().filter(|p| !p.as_os_str().is_empty()) else {
                continue;
            };
            if !markers.iter().any(|m| m == parent) {
                markers.push(parent.to_path_buf());
            }
        }
        match projectroot::find(&wd, &markers) {
            Ok(root) => Ok(Some(root)),
            // The search says what happened and where. What to do about it is
            // in this tool's vocabulary, which is not the search's to know.
            Err(projectroot::FindError::IsHome(dir)) => Err(Error::ProjectIsHome { dir }),
            Err(projectroot::FindError::Io(e)) => Err(Error::io("find the project root", e)),
        }
    }

    /// Resolves the destination directories for `options`.
    ///
    /// At project scope every agent but Claude Code shares `.agents/skills`, so
    /// those are merged into one target and a skill is never written there
    /// twice.
    ///
    /// # Errors
    ///
    /// Fails when `--agent` names nothing, when the project root cannot be
    /// found, or when a project scope destination would land outside it.
    pub fn targets(&self, options: &InstallOptions) -> Result<Vec<InstallTarget>> {
        let scope = options.scope.unwrap_or(self.default_scope);

        if let Some(dir) = &options.dir {
            // An empty name is not a directory. It resolves to the working
            // directory, which would put every skill loose in whatever
            // directory the tool was run from.
            if dir.as_os_str().is_empty() {
                return Err(Error::Usage("the install directory must be named".to_owned()));
            }
            // Validated before `dir` wins, so that a bad `--agent` alongside it
            // is a diagnosis rather than silence.
            agent::resolve(&self.agents, &options.agents, &|_| true)?;
            let dir = paths::absolute(dir)
                .map_err(|e| Error::io(format!("resolve {}", dir.display()), e))?;
            return Ok(vec![InstallTarget { dir, agents: Vec::new(), root: None }]);
        }

        let names = if options.agents.is_empty() { &self.default_agents } else { &options.agents };
        let root = self.project_root_of(scope)?;

        // An agent counts as present when the directory holding its skills
        // directory is there. The skills directory itself need not be.
        let detected = |a: &Agent| {
            a.dir(scope, root.as_deref()).is_ok_and(|dir| dir.parent().is_some_and(Path::is_dir))
        };

        let agents = agent::resolve(&self.agents, names, &detected)?;
        let agents = agent::fall_back_to_all(&self.agents, agents, names);
        if agents.is_empty() {
            return Err(Error::NoAgentSelected);
        }

        // Only a project install has a project. User scope writes into the home
        // directory, and a named directory is the destination outright.
        let stamped = (scope == Scope::Project).then(|| root.clone()).flatten();

        let mut targets: Vec<InstallTarget> = Vec::new();
        for a in agents {
            let dir = a.dir(scope, root.as_deref())?;
            let dir = paths::absolute(&dir)
                .map_err(|e| Error::io(format!("resolve {}", dir.display()), e))?;
            // Checked per agent rather than per target, so that the agent whose
            // directory escapes is the one reached before any of them is
            // written.
            if let Some(root) = root.as_deref().filter(|_| scope == Scope::Project) {
                match projectroot::within(root, &dir) {
                    Ok(()) => {}
                    Err(projectroot::WithinError::Outside(o)) => {
                        return Err(Error::ProjectEscapes { dir: o.dir, real: o.real });
                    }
                    Err(projectroot::WithinError::Io(e)) => {
                        return Err(Error::io(format!("resolve {}", dir.display()), e));
                    }
                }
            }
            match targets.iter_mut().find(|t| t.dir == dir) {
                Some(t) => t.agents.push(a),
                None => targets.push(InstallTarget { dir, agents: vec![a], root: stamped.clone() }),
            }
        }
        Ok(targets)
    }

    /// Resolves `options.names` against the embedded set.
    ///
    /// `also_installed` holds the names found at the destinations that the set
    /// does not carry. A name in there is an orphan, which has a row of its own
    /// and no embedded skill behind it, so it is accepted and skipped rather
    /// than called unknown. Listing a name and then refusing it is the worse
    /// answer.
    fn selected(&self, options: &InstallOptions, also_installed: &[String]) -> Result<Vec<Skill>> {
        if options.names.is_empty() {
            return Ok(self.set.skills().to_vec());
        }
        let mut out = Vec::with_capacity(options.names.len());
        for name in &options.names {
            match self.set.get(name) {
                Some(sk) => out.push(sk.clone()),
                None if also_installed.iter().any(|n| n == name) => {}
                None => {
                    let mut embedded: Vec<String> = self.set.names().map(str::to_owned).collect();
                    embedded.sort();
                    return Err(Error::UnknownSkill { name: name.clone(), embedded });
                }
            }
        }
        Ok(out)
    }

    /// Reports what is installed where, without changing anything.
    ///
    /// It also reports what a destination holds that this tool wrote and the
    /// binary no longer carries, as [`State::Orphaned`].
    ///
    /// # Errors
    ///
    /// Fails for the reasons [`Installer::targets`] does, when a named skill is
    /// not embedded, or when a destination cannot be read at all.
    pub fn status(&self, options: &InstallOptions) -> Result<Vec<InstallStatus>> {
        let targets = self.targets(options)?;

        // Found before the names are resolved. `list` prints these, so a name
        // it printed has to be one uninstall accepts, and the embedded set
        // alone cannot say whether a name exists at the destination.
        let mut orphans_at = Vec::with_capacity(targets.len());
        let mut also_installed = Vec::new();
        for t in &targets {
            let orphans = self.orphaned(t, options)?;
            also_installed.extend(orphans.iter().map(|st| st.skill.clone()));
            orphans_at.push(orphans);
        }

        let skills = self.selected(options, &also_installed)?;

        let mut out = Vec::with_capacity(targets.len() * (skills.len() + 1));
        for (t, orphans) in targets.iter().zip(orphans_at) {
            for sk in &skills {
                options.check_cancelled()?;
                out.push(self.inspect(t, sk)?);
            }
            for st in orphans {
                // A named run acts on what it was told to. `install demo` is
                // about demo, and sweeping the directory on the way past would
                // remove skills the run was never asked about.
                if options.names.is_empty() || options.names.contains(&st.skill) {
                    out.push(st);
                }
            }
        }
        Ok(out)
    }

    /// Lists the directories at `target` that this tool wrote and the binary no
    /// longer carries.
    ///
    /// Nothing else finds them. Every other walk starts from the embedded set,
    /// so a skill dropped between two versions is never looked at again:
    /// install passes it by, list does not mention it, and uninstall leaves it
    /// behind for good.
    ///
    /// A directory is claimed on three things together, and each is necessary.
    /// Its `x-embedded-by` names this tool, so nothing anyone else put there is
    /// in reach. Its contents still hash to the digest that was recorded when it
    /// was written, so it is byte for byte what this tool left and removing it
    /// loses nothing the binary could not write again. And the binary has no
    /// skill of that name to put back.
    ///
    /// A directory that fails the digest is not claimed and is not reported. It
    /// held this tool's work once and holds something else now, which is what a
    /// skill copied and then edited into one of the user's own looks like.
    /// There is no version of that this tool should delete: nothing is being
    /// installed over it, so there is no conflict for `--force` to resolve, and
    /// `--force` therefore has no part in the sweep at all.
    fn orphaned(
        &self,
        target: &InstallTarget,
        options: &InstallOptions,
    ) -> Result<Vec<InstallStatus>> {
        let entries = match fs::read_dir(&target.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(format!("read {}", target.dir.display()), e)),
        };

        // Where the embedded skills live at this target. A name that is not in
        // the set can still be one of these directories: a file system that
        // folds case or normalizes Unicode answers to more than one spelling,
        // so a skill renamed to another spelling of itself reads as both an
        // embedded row and an orphan, and both name the same directory.
        // Removing the orphan would then delete the skill the same run just
        // wrote. Asking the file system settles it without this having to guess
        // what it treats as equal.
        let embedded: Vec<PathBuf> = self
            .set
            .skills()
            .iter()
            .map(|sk| target.dir.join(sk.name()))
            .filter(|p| p.exists())
            .collect();

        let mut out = Vec::new();
        for entry in entries {
            // The one place the sweep does real work, a directory at a time.
            options.check_cancelled()?;
            let entry =
                entry.map_err(|e| Error::io(format!("read {}", target.dir.display()), e))?;
            // The entry's own type, which does not follow a symbolic link. A
            // link is something the user put there, and the sweep removes what
            // it finds, so it must not reach through one.
            // An entry whose type cannot be read is one the sweep cannot claim,
            // so it is passed over rather than made to end the run. Go reads
            // the type off the directory entry, where it cannot fail at all.
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            // A name that is not UTF-8 is not a skill name, so nothing embedded
            // can share it and nothing here can claim it.
            let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            if self.set.get(&name).is_some() {
                continue;
            }
            // A write stages and rescues under a leading dot, and the rescue is
            // a verbatim copy of an installation, stamp and all, so it matches
            // everything below. The error that leaves one behind names it for
            // the user to go and recover, and the next run must not take it
            // away. Agents pass over these too.
            if name.starts_with('.') {
                continue;
            }
            let dest = target.dir.join(&name);
            if embedded.iter().any(|p| paths::same_file(p, &dest)) {
                continue;
            }
            let Ok(installed) = fs::read(dest.join(SKILL_FILE)) else {
                continue; // not a skill directory, or not one that can be read
            };
            let fields = manifest::fields(&installed);
            // A key that is there and empty carries no claim, so it is no claim.
            let recorded = fields.get(manifest::KEY_EMBEDDED_DIGEST).filter(|d| !d.is_empty());
            if recorded.is_none()
                || fields.get(manifest::KEY_EMBEDDED_BY).map(String::as_str)
                    != Some(&self.tool_name)
            {
                continue; // someone else's, or carrying no claim at all
            }
            let Ok(actual) = Tree::read_dir(&dest).map(|t| t.digest()) else {
                continue;
            };
            if recorded != Some(&actual) {
                continue; // not what this tool left there, so not this tool's to take
            }

            out.push(InstallStatus {
                skill: name,
                description: String::new(),
                target: target.clone(),
                path: dest,
                state: State::Orphaned,
                installed_by: fields.get(manifest::KEY_EMBEDDED_BY).cloned(),
                installed_version: fields.get(manifest::KEY_EMBEDDED_VERSION).cloned(),
            });
        }
        out.sort_by(|a, b| a.skill.cmp(&b.skill));
        Ok(out)
    }

    fn inspect(&self, target: &InstallTarget, sk: &Skill) -> Result<InstallStatus> {
        let dest = target.dir.join(sk.name());
        let mut st = InstallStatus {
            skill: sk.name().to_owned(),
            description: sk.description().to_owned(),
            target: target.clone(),
            path: dest.clone(),
            state: State::Missing,
            installed_by: None,
            installed_version: None,
        };

        // `metadata`, which follows a symbolic link, because a link standing in
        // for an installed skill is one the agent reads through. A link is
        // therefore described by what it points at, and a dangling one is
        // nothing at all.
        match fs::metadata(&dest) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(st),
            // Not being able to describe the destination at all is not a state.
            // It says nothing about what is there, and the run cannot go on.
            Err(e) => return Err(Error::io(format!("read {}", dest.display()), e)),
            Ok(meta) if !meta.is_dir() => {
                st.state = State::Foreign;
                return Ok(st);
            }
            Ok(_) => {}
        }

        let Ok(installed) = fs::read(dest.join(SKILL_FILE)) else {
            // A directory with no manifest is not ours to replace silently.
            st.state = State::Foreign;
            return Ok(st);
        };
        let fields = manifest::fields(&installed);
        st.installed_by = fields.get(manifest::KEY_EMBEDDED_BY).cloned();
        st.installed_version = fields.get(manifest::KEY_EMBEDDED_VERSION).cloned();
        // A key that is there and empty carries no claim, so it is no claim.
        let Some(recorded) = fields.get(manifest::KEY_EMBEDDED_DIGEST).filter(|d| !d.is_empty())
        else {
            st.state = State::Foreign;
            return Ok(st);
        };
        if st.installed_by.as_deref() != Some(&self.tool_name) {
            st.state = State::Foreign;
            return Ok(st);
        }

        // Something there cannot be read or hashed, such as a symlink a user
        // dropped in. Nothing can be said about the copy, so it is not claimed
        // as this tool's, and `--force` stays the way out. An error here would
        // instead fail the whole run for every other skill at this target.
        let Ok(found) = Tree::read_dir(&dest) else {
            st.state = State::Foreign;
            return Ok(st);
        };

        st.state = if found.digest() != *recorded {
            State::Modified
        } else if *recorded != sk.digest() {
            State::Outdated
        } else if found.executable_bits_match(&*self.executable) {
            State::UpToDate
        } else {
            // The contents match. The executable bit is not in the digest, and
            // a script that lost it cannot be run by the agent. That reads as
            // outdated rather than modified, because the user did not do it and
            // repairing it should not need `--force`.
            State::Outdated
        };
        Ok(st)
    }

    /// Writes the selected skills into the resolved targets.
    ///
    /// A run over the whole set also removes what this tool wrote and the
    /// binary no longer carries, reported as [`State::Orphaned`]. Nothing else
    /// would ever reach those directories again. A run naming skills installs
    /// those and sweeps nothing it was not told to.
    ///
    /// A destination this tool did not write, or one edited after it did, is
    /// left alone unless [`InstallOptions::force`] is set. Those skills come
    /// back as [`Action::Skipped`] with a reason, exactly as
    /// [`Installer::uninstall`] reports them, and the outcome is
    /// [`Error::NeedsForce`]. One blocked destination does not stop the others
    /// from being written.
    ///
    /// The results are worth reporting even when the outcome is an error. They
    /// describe everything that happened before it.
    pub fn install(&self, options: &InstallOptions) -> (Vec<InstallResult>, Result<()>) {
        let statuses = match self.status(options) {
            Ok(statuses) => statuses,
            Err(e) => return (Vec::new(), Err(e)),
        };

        let mut blocked = Vec::new();
        let mut results = Vec::with_capacity(statuses.len());
        for st in statuses {
            if let Err(e) = options.check_cancelled() {
                return (results, Err(e));
            }
            let embedded = self.set.get(&st.skill);
            let (action, reason) = match st.state {
                // The binary has no copy to put back, so removing it is the
                // whole action. No `--force`: the directory is byte for byte
                // what this tool wrote, nothing is being installed over it, and
                // leaving it is the one outcome nobody wants.
                State::Orphaned => {
                    (Action::Removed, Some(format!("no longer embedded in {}", self.tool_name)))
                }
                State::Modified if !options.force => {
                    blocked.push(st.clone());
                    (Action::Skipped, Some("edited after installing; use --force".to_owned()))
                }
                State::Foreign if !options.force => {
                    blocked.push(st.clone());
                    (Action::Skipped, Some("installed by something else; use --force".to_owned()))
                }
                State::UpToDate if !options.force => {
                    (Action::Skipped, Some("already up to date".to_owned()))
                }
                State::Missing => (Action::Installed, None),
                _ => (Action::Updated, None),
            };
            let result = InstallResult {
                skill: st.skill.clone(),
                target: st.target.clone(),
                path: st.path.clone(),
                before: st.state,
                action,
                reason,
            };
            if !options.dry_run {
                // Switched on the action rather than on "not skipped", because
                // an orphan has no embedded skill behind it and nothing to
                // write.
                let done = match action {
                    Action::Removed => fs::remove_dir_all(&st.path).map_err(|e| {
                        Error::io(
                            format!("remove {} from {}", st.skill, st.target.dir.display()),
                            e,
                        )
                    }),
                    // Only an orphan has no embedded skill behind it, and an
                    // orphan is removed rather than written.
                    Action::Installed | Action::Updated => embedded.map_or(Ok(()), |sk| {
                        self.write(sk, &st.path).map_err(|e| {
                            Error::io(
                                format!("install {} into {}", st.skill, st.target.dir.display()),
                                e,
                            )
                        })
                    }),
                    Action::Skipped => Ok(()),
                };
                if let Err(e) = done {
                    // The row says what was done, so it is added once that is
                    // true. Adding it first made a failed write print
                    // `installed` above its own error.
                    return (results, Err(e));
                }
            }
            results.push(result);
        }
        let outcome =
            if blocked.is_empty() { Ok(()) } else { Err(ForceRequired { blocked }.into()) };
        (results, outcome)
    }

    /// Removes the selected skills from the resolved targets. Skills this tool
    /// did not install are left alone unless [`InstallOptions::force`] is set.
    ///
    /// A run over the whole set also removes what this tool wrote and the
    /// binary no longer carries, so that a full uninstall leaves nothing of this
    /// tool's behind.
    ///
    /// The results are worth reporting even when the outcome is an error.
    pub fn uninstall(&self, options: &InstallOptions) -> (Vec<InstallResult>, Result<()>) {
        let statuses = match self.status(options) {
            Ok(statuses) => statuses,
            Err(e) => return (Vec::new(), Err(e)),
        };
        let mut results = Vec::with_capacity(statuses.len());
        for st in statuses {
            if let Err(e) = options.check_cancelled() {
                return (results, Err(e));
            }
            let (action, reason) = match st.state {
                State::Missing => (Action::Skipped, Some("not installed".to_owned())),
                State::Foreign if !options.force => {
                    (Action::Skipped, Some("installed by something else; use --force".to_owned()))
                }
                State::Modified if !options.force => {
                    (Action::Skipped, Some("edited after installing; use --force".to_owned()))
                }
                _ => (Action::Removed, None),
            };
            if action == Action::Removed
                && !options.dry_run
                && let Err(e) = fs::remove_dir_all(&st.path)
            {
                // The row is added once the removal is true, so a failure never
                // prints `removed` above its own error.
                return (results, Err(Error::io(format!("remove {}", st.path.display()), e)));
            }
            results.push(InstallResult {
                skill: st.skill,
                target: st.target,
                path: st.path,
                before: st.state,
                action,
                reason,
            });
        }
        (results, Ok(()))
    }

    /// Materialises one skill at `dest`.
    fn write(&self, sk: &Skill, dest: &Path) -> io::Result<()> {
        let stamp = self.stamp(sk);
        sk.tree().write(dest, stamp.as_deref(), &*self.executable)
    }

    /// Records who installed the skill, from what version, and what it held, in
    /// the manifest's frontmatter.
    fn stamp<'a>(&'a self, sk: &'a Skill) -> Option<Box<tree::Transform<'a>>> {
        if !self.metadata {
            return None;
        }
        Some(Box::new(move |name: &str, data: &[u8]| {
            if name != SKILL_FILE {
                return None;
            }
            Some(manifest::with(
                data,
                &[
                    manifest::Entry {
                        key: manifest::KEY_EMBEDDED_BY,
                        value: self.tool_name.clone(),
                    },
                    manifest::Entry {
                        key: manifest::KEY_EMBEDDED_VERSION,
                        value: self.version.clone(),
                    },
                    manifest::Entry {
                        key: manifest::KEY_EMBEDDED_AT,
                        value: installed_at(SystemTime::now()),
                    },
                    manifest::Entry {
                        key: manifest::KEY_EMBEDDED_DIGEST,
                        value: sk.digest().to_owned(),
                    },
                ],
            ))
        }))
    }
}

/// The one timestamp this crate writes: when the skill was installed, in UTC
/// and to the second.
///
/// A clock the calendar cannot describe reads as the epoch. Nothing installs a
/// skill then, and it is not worth a second return value.
fn installed_at(t: SystemTime) -> String {
    jiff::Timestamp::try_from(t)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn binary_name() -> String {
    let arg0 = std::env::args_os().next().map(PathBuf::from);
    arg0.as_deref()
        .and_then(Path::file_name)
        .map_or_else(|| "tool".to_owned(), |n| n.to_string_lossy().into_owned())
}

/// The inputs shared by install, uninstall and list.
///
/// Every field has a default, so a caller names only what it means to change:
///
/// ```no_run
/// use skill_embed::{Agent, AgentSelector, InstallOptions, Scope};
///
/// let options = InstallOptions {
///     agents: vec![AgentSelector::from(&Agent::CLAUDE_CODE)],
///     scope: Some(Scope::User),
///     ..InstallOptions::default()
/// };
/// ```
#[derive(Clone, Debug, Default)]
pub struct InstallOptions {
    /// Selects the destinations. Empty means the installer default.
    pub agents: Vec<AgentSelector>,
    /// [`None`] means the installer default.
    pub scope: Option<Scope>,
    /// Installs into this directory, overriding `agents` and `scope`.
    pub dir: Option<PathBuf>,
    /// Overwrites skills that were edited, or that something else installed.
    pub force: bool,
    /// Reports what would happen without touching the file system.
    pub dry_run: bool,
    /// Selects skills by name. Empty means every embedded skill.
    pub names: Vec<String>,
    /// Stops the run between skills once it is set.
    ///
    /// A single skill is written whole or not at all, so the flag is read
    /// between them rather than during one.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl InstallOptions {
    fn check_cancelled(&self) -> Result<()> {
        match &self.cancel {
            Some(c) if c.load(Ordering::Relaxed) => Err(Error::Cancelled),
            _ => Ok(()),
        }
    }
}

/// One destination directory and the agents that read from it.
#[derive(Clone, Debug)]
pub struct InstallTarget {
    /// The absolute skills directory.
    pub dir: PathBuf,
    /// The agents that read from [`InstallTarget::dir`]. It is empty when
    /// [`InstallOptions::dir`] was used.
    pub agents: Vec<Agent>,
    /// The project directory [`InstallTarget::dir`] was resolved against. It is
    /// [`None`] for user scope, and when [`InstallOptions::dir`] named the
    /// destination outright.
    pub root: Option<PathBuf>,
}

impl InstallTarget {
    /// Renders the target for human readable output.
    #[must_use]
    pub fn label(&self) -> String {
        if self.agents.is_empty() {
            return self.dir.display().to_string();
        }
        let titles: Vec<&str> = self.agents.iter().map(Agent::title).collect();
        format!("{} ({})", self.dir.display(), titles.join(", "))
    }
}

/// The state of one skill at one destination.
#[derive(Clone, Debug)]
pub struct InstallStatus {
    /// The skill's name. An orphan has one and no embedded skill behind it.
    pub skill: String,
    /// The skill's description, empty for an orphan.
    pub description: String,
    /// The destination this row is about.
    pub target: InstallTarget,
    /// The skill's own directory inside [`InstallTarget::dir`].
    pub path: PathBuf,
    /// What is there.
    pub state: State,
    /// From the installed frontmatter.
    pub installed_by: Option<String>,
    /// From the installed frontmatter.
    pub installed_version: Option<String>,
}

/// The outcome for one skill at one destination.
#[derive(Clone, Debug)]
pub struct InstallResult {
    /// The skill's name.
    pub skill: String,
    /// The destination this row is about.
    pub target: InstallTarget,
    /// The skill's own directory inside [`InstallTarget::dir`].
    pub path: PathBuf,
    /// The state found at [`InstallResult::path`].
    pub before: State,
    /// What was done.
    pub action: Action,
    /// Why nothing was done.
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// The stamp is read back by a human and compared by nothing, but it is in
    /// a file this tool writes, so its shape is part of the format.
    #[test]
    fn the_stamp_is_rfc_3339_in_utc_and_to_the_second() {
        let at = |s| installed_at(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(1_774_195_793), "2026-03-22T16:09:53Z");
        // A leap day, and one in a century that is a leap year after all.
        assert_eq!(at(1_709_208_000), "2024-02-29T12:00:00Z");
        assert_eq!(at(951_826_800), "2000-02-29T12:20:00Z");
    }
}
