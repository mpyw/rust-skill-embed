use std::process::ExitCode;
use std::str::FromStr as _;

use crate::textfmt::{self, Columns};
use crate::{
    AgentSelector, Error, InstallOptions, InstallResult, InstallStatus, Installer, Result, Scope,
    State,
};

/// What install and uninstall have in common: they differ only in what they
/// call and in nothing else.
type ActionRun = fn(&Installer, &InstallOptions) -> (Vec<InstallResult>, Result<()>);

/// One flag the built-in command takes.
struct Flag {
    long: &'static str,
    short: Option<char>,
    /// The placeholder a value goes in, or [`None`] for a switch.
    value: Option<&'static str>,
    help: &'static str,
}

const FLAGS: [Flag; 6] = [
    Flag {
        long: "agent",
        short: None,
        value: Some("AGENT"),
        help: "Target agent: {AGENTS}, or all, or detected (repeatable) [default: {DEFAULT_AGENTS}]",
    },
    Flag {
        long: "dir",
        short: None,
        value: Some("DIR"),
        help: "Install to a custom directory (overrides --agent and --scope)",
    },
    Flag {
        long: "scope",
        short: None,
        value: Some("SCOPE"),
        help: "Installation scope: {project|user} [default: {DEFAULT_SCOPE}]",
    },
    Flag { long: "force", short: Some('f'), value: None, help: "Overwrite existing skills" },
    Flag {
        long: "dry-run",
        short: None,
        value: None,
        help: "Report what would happen without writing",
    },
    Flag { long: "help", short: Some('h'), value: None, help: "Show this help" },
];

impl Installer {
    /// Executes the skill command.
    ///
    /// `args` are the arguments after the command name, so `mytool skill
    /// install --scope user` passes `["install", "--scope", "user"]`.
    ///
    /// It never exits the process, so a driver keeps control. It is what the
    /// adapters call underneath, and what [`Installer::intercept`] wraps.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Help`] when help was asked for, which is not a failure.
    /// Everything else is.
    pub fn run(&self, args: &[String]) -> Result<()> {
        let Some((sub, rest)) = args.split_first() else {
            self.print_out(&self.usage())?;
            return Err(Error::Help);
        };
        match sub.as_str() {
            "install" => self.run_action(sub, rest, Self::install),
            "uninstall" | "remove" => self.run_action("uninstall", rest, Self::uninstall),
            "list" | "ls" => self.run_list(rest),
            "help" | "-h" | "--help" => {
                self.print_out(&self.usage())?;
                Err(Error::Help)
            }
            _ => {
                self.print_err(&self.usage())?;
                Err(Error::Usage(format!("unknown {} subcommand {sub:?}", self.command_name())))
            }
        }
    }

    /// Runs the skill command when it is the first argument, and reports the
    /// exit code the process should end with.
    ///
    /// [`None`] means the arguments were not for this command and the tool
    /// should carry on:
    ///
    /// ```no_run
    /// use std::process::ExitCode;
    /// use std::sync::LazyLock;
    /// # use skill_embed::Installer;
    /// # static SKILLS: LazyLock<Installer> = LazyLock::new(|| unimplemented!());
    ///
    /// fn main() -> ExitCode {
    ///     if let Some(code) = SKILLS.intercept() {
    ///         return code;
    ///     }
    ///     // the rest of your tool
    ///     ExitCode::SUCCESS
    /// }
    /// ```
    ///
    /// The guard is the first argument and nothing else. `mytool skill install`
    /// reaches it. `mytool -v skill install` does not. Put your own flags after
    /// the subcommand, or before a normal run.
    #[must_use]
    pub fn intercept(&self) -> Option<ExitCode> {
        let argv: Vec<String> = std::env::args().collect();
        self.intercept_args(&argv)
    }

    /// [`Installer::intercept`] against arguments the caller supplies, starting
    /// with the program name.
    #[must_use]
    pub fn intercept_args(&self, argv: &[String]) -> Option<ExitCode> {
        if argv.get(1).map(String::as_str) != Some(self.command_name()) {
            return None;
        }
        Some(match self.run(&argv[2..]) {
            Ok(()) | Err(Error::Help) => ExitCode::SUCCESS,
            Err(e) => {
                let _ = self.print_err(&format!("{}: {e}\n", self.tool_name()));
                ExitCode::FAILURE
            }
        })
    }

    fn run_action(&self, sub: &str, args: &[String], run: ActionRun) -> Result<()> {
        let options = self.parse(sub, args)?;
        // Report first. Install describes everything it did before the error,
        // and a run that wrote three skills and refused a fourth has to say so.
        let (results, outcome) = run(self, &options);
        self.print_out(&render_results(&results, options.dry_run))?;
        outcome
    }

    fn run_list(&self, args: &[String]) -> Result<()> {
        let options = self.parse("list", args)?;
        let statuses = self.status(&options)?;
        self.print_out(&render_status(&statuses))
    }

    fn print_out(&self, s: &str) -> Result<()> {
        self.write_out(s).map_err(|e| Error::io("write the report", e))
    }

    fn print_err(&self, s: &str) -> Result<()> {
        self.write_err(s).map_err(|e| Error::io("write the report", e))
    }

    /// Reads one subcommand's arguments.
    ///
    /// Flags are accepted anywhere, so `skill install demo --dry-run` means
    /// what it looks like. A `--` ends them, for a skill whose name starts with
    /// a dash.
    fn parse(&self, sub: &str, args: &[String]) -> Result<InstallOptions> {
        let mut options =
            InstallOptions { scope: Some(self.default_scope()), ..Default::default() };
        let mut rest = args.iter();
        let mut positional_only = false;

        while let Some(arg) = rest.next() {
            if positional_only || !arg.starts_with('-') || arg == "-" {
                options.names.push(arg.clone());
                continue;
            }
            if arg == "--" {
                positional_only = true;
                continue;
            }
            let body = arg.trim_start_matches('-');
            let (name, inline) = match body.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (body, None),
            };
            let Some(flag) = FLAGS.iter().find(|f| {
                f.long == name || f.short.is_some_and(|s| name.len() == 1 && name.starts_with(s))
            }) else {
                return self.usage_error(sub, format!("unknown flag {arg:?}"));
            };
            if flag.value.is_none() && inline.is_some() {
                return self.usage_error(sub, format!("--{} takes no value", flag.long));
            }
            let value = match flag.value {
                None => None,
                Some(_) => match inline.or_else(|| rest.next().cloned()) {
                    Some(v) => Some(v),
                    None => {
                        return self.usage_error(sub, format!("--{} needs a value", flag.long));
                    }
                },
            };
            match (flag.long, value) {
                ("agent", Some(v)) => options.agents.extend(AgentSelector::parse_list(&v)),
                ("dir", Some(v)) => options.dir = Some(v.into()),
                ("scope", Some(v)) => match Scope::from_str(&v) {
                    Ok(scope) => options.scope = Some(scope),
                    Err(e) => return self.usage_error(sub, e.to_string()),
                },
                ("force", None) => options.force = true,
                ("dry-run", None) => options.dry_run = true,
                ("help", None) => {
                    self.print_out(&self.usage_for(Some(sub)))?;
                    return Err(Error::Help);
                }
                _ => unreachable!("every flag is handled"),
            }
        }
        Ok(options)
    }

    fn usage_error(&self, sub: &str, message: String) -> Result<InstallOptions> {
        self.print_err(&self.usage_for(Some(sub)))?;
        Err(Error::Usage(message))
    }
}

/// Renders the flag block, which is written here rather than taken from a
/// framework, because the core parses the command line itself.
pub(crate) fn flag_block(agents: &str, default_agents: &str, default_scope: Scope) -> String {
    let mut c = Columns::default();
    for f in &FLAGS {
        let left = match (f.short, f.value) {
            (Some(s), Some(v)) => format!("  -{s}, --{} <{v}>", f.long),
            (Some(s), None) => format!("  -{s}, --{}", f.long),
            (None, Some(v)) => format!("      --{} <{v}>", f.long),
            (None, None) => format!("      --{}", f.long),
        };
        let help = f
            .help
            .replace("{AGENTS}", agents)
            .replace("{DEFAULT_AGENTS}", default_agents)
            .replace("{DEFAULT_SCOPE}", default_scope.as_str());
        c.push([left, help]);
    }
    c.render()
}

/// Names the project a run resolved to.
///
/// The destinations in each row say it too, but a reader has to infer it from a
/// leaf path, and a run from a subdirectory can resolve somewhere they did not
/// expect. Naming the decision is cheaper than noticing it.
fn project_line(target: Option<&crate::InstallTarget>) -> String {
    match target.and_then(|t| t.root.as_deref()) {
        Some(root) => format!("Project root: {}\n\n", root.display()),
        None => String::new(),
    }
}

/// Everything `list` prints: each skill once with its description, then a row
/// per destination.
///
/// It is the whole of the subcommand's output, not a part of it, so that every
/// front end that calls it says the same thing. The skills come from the
/// statuses rather than from the set, so a run narrowed by name describes only
/// what it was asked about.
#[must_use]
pub fn render_status(statuses: &[InstallStatus]) -> String {
    let mut out = project_line(statuses.first().map(|st| &st.target));

    let mut seen: Vec<&str> = Vec::new();
    for st in statuses {
        // The heading is what the binary carries. An orphan is on the disk and
        // not in the binary, so it belongs in the table below and nowhere else.
        if st.state == State::Orphaned || seen.contains(&st.skill.as_str()) {
            continue;
        }
        seen.push(&st.skill);
        out.push_str(&st.skill);
        out.push('\n');
        if !st.description.is_empty() {
            out.push_str("  ");
            out.push_str(&textfmt::first_line(&st.description));
            out.push('\n');
        }
    }
    out.push('\n');

    let mut c = Columns::default();
    c.push(["SKILL".to_owned(), "STATE".to_owned(), "PATH".to_owned()]);
    for st in statuses {
        c.push([st.skill.clone(), st.state.to_string(), st.path.display().to_string()]);
    }
    out.push_str(&c.render());
    textfmt::trim_lines(&out)
}

/// Lists what install or uninstall did at each destination.
///
/// The adapters use it so that every front end reports the same way.
#[must_use]
pub fn render_results(results: &[InstallResult], dry_run: bool) -> String {
    let prefix = if dry_run { "would be " } else { "" };
    let mut out = project_line(results.first().map(|r| &r.target));
    let mut c = Columns::default();
    for r in results {
        let note = match &r.reason {
            Some(reason) => format!(" ({reason})"),
            None => String::new(),
        };
        c.push([
            format!("{prefix}{}", r.action),
            r.skill.clone(),
            format!("{}{note}", r.path.display()),
        ]);
    }
    out.push_str(&c.render());
    textfmt::trim_lines(&out)
}
