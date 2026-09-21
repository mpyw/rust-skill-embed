//! A [clap] command for the agent skills embedded by [`skill_embed`].
//!
//! ```no_run
//! use clap::Command;
//! # use std::sync::LazyLock;
//! # static SKILLS: LazyLock<skill_embed::Installer> = LazyLock::new(|| unimplemented!());
//!
//! let cli = Command::new("mytool").subcommand(skill_embed_clap::command(&SKILLS));
//! let matches = cli.get_matches();
//! if let Some((name, args)) = matches.subcommand()
//!     && name == SKILLS.command_name()
//! {
//!     skill_embed_clap::run(&SKILLS, args)?;
//! }
//! # Ok::<_, skill_embed::Error>(())
//! ```
//!
//! The flags are the ones `gh skill install` defines, so a user who knows that
//! command already knows this one. What each subcommand prints comes from
//! [`skill_embed::render_results`] and [`skill_embed::render_status`], and goes
//! to the writer [`skill_embed::Installer::with_output`] names, so every front
//! end reports the same way to the same place.
//!
//! [clap]: https://docs.rs/clap

use clap::{Arg, ArgAction, ArgMatches, Command};
use skill_embed::{AgentSelector, Error, InstallOptions, Installer, Result, Scope};

/// The skill command, with `install`, `uninstall` and `list` beneath it.
///
/// Its name is the installer's command name, `skill` unless it was changed.
#[must_use]
pub fn command(skills: &Installer) -> Command {
    let tool = skills.tool_name().to_owned();
    Command::new(skills.command_name().to_owned())
        .about(format!("Manage the agent skills embedded in {tool}"))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(flags(
            Command::new("install").about(format!(
                "Install the agent skills embedded in {tool}, \
                     and remove the ones it no longer carries"
            )),
            skills,
        ))
        .subcommand(flags(
            Command::new("uninstall")
                .visible_alias("remove")
                .about(format!("Remove the agent skills embedded in {tool}")),
            skills,
        ))
        .subcommand(flags(
            Command::new("list").visible_alias("ls").about(format!(
                "Show the agent skills embedded in {tool}, and where each one stands"
            )),
            skills,
        ))
}

/// The flags every subcommand shares, and the skill names it takes.
fn flags(command: Command, skills: &Installer) -> Command {
    let defaults: Vec<String> = skills.default_agents().iter().map(ToString::to_string).collect();
    command
        .arg(
            Arg::new("agent")
                .long("agent")
                .value_name("AGENT")
                .action(ArgAction::Append)
                .value_delimiter(',')
                .default_value(defaults.join(","))
                .help(format!("Target agent: {}, or all, or detected", skills.agent_choices())),
        )
        .arg(
            Arg::new("dir")
                .long("dir")
                .value_name("DIR")
                .help("Install to a custom directory (overrides --agent and --scope)"),
        )
        .arg(
            Arg::new("scope")
                .long("scope")
                .value_name("SCOPE")
                .value_parser(["project", "user"])
                .default_value(skills.default_scope().as_str())
                .help("Installation scope"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .short('f')
                .action(ArgAction::SetTrue)
                .help("Overwrite existing skills"),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .action(ArgAction::SetTrue)
                .help("Report what would happen without writing"),
        )
        .arg(
            Arg::new("skill")
                .value_name("SKILL")
                .action(ArgAction::Append)
                .help("The skills to act on, or every embedded skill"),
        )
}

/// Reads the flags clap parsed.
///
/// A front end that builds its own [`Command`] uses this to reach the same
/// options without repeating the flag names.
///
/// # Errors
///
/// Fails when `--scope` names neither scope, which a [`Command`] built by
/// [`command`] refuses before this is reached.
pub fn options(matches: &ArgMatches) -> Result<InstallOptions> {
    let given: Vec<&String> = matches.get_many::<String>("agent").into_iter().flatten().collect();
    let agents: Vec<AgentSelector> =
        given.iter().flat_map(|v| AgentSelector::parse_list(v).collect::<Vec<_>>()).collect();
    // A value that names nothing is a mistake, and an empty list reads as "use
    // the default" everywhere below. Only a front end knows the flag was given.
    if !given.is_empty() && agents.is_empty() {
        return Err(Error::NoAgentSelected);
    }
    let scope = match matches.get_one::<String>("scope") {
        Some(s) => Some(s.parse::<Scope>()?),
        None => None,
    };
    Ok(InstallOptions {
        agents,
        scope,
        dir: matches.get_one::<String>("dir").map(Into::into),
        force: matches.get_flag("force"),
        dry_run: matches.get_flag("dry-run"),
        names: matches.get_many::<String>("skill").into_iter().flatten().cloned().collect(),
        cancel: None,
    })
}

/// Runs whichever subcommand clap matched, and prints what it did.
///
/// `matches` are the skill command's own, so a tool that nests it deeper passes
/// the matches from that level.
///
/// # Errors
///
/// Whatever the run refused, including [`Error::NeedsForce`] when a destination
/// was left alone. The report is printed before the error is returned, because
/// a run that wrote three skills and refused a fourth has to say so.
pub fn run(skills: &Installer, matches: &ArgMatches) -> Result<()> {
    let Some((name, args)) = matches.subcommand() else {
        return Err(Error::Usage(format!("{} needs a subcommand", skills.command_name())));
    };
    let options = options(args)?;
    match name {
        "list" => {
            let statuses = skills.status(&options)?;
            skills.write_report(&skill_embed::render_status(&statuses))
        }
        "install" | "uninstall" => {
            let (results, outcome) = if name == "install" {
                skills.install(&options)
            } else {
                skills.uninstall(&options)
            };
            // Reported first. The results describe everything that happened
            // before the error, and a run that wrote three skills and refused a
            // fourth has to say so.
            skills.write_report(&skill_embed::render_results(&results, options.dry_run))?;
            outcome
        }
        _ => Err(Error::Usage(format!("unknown {} subcommand {name:?}", skills.command_name()))),
    }
}
