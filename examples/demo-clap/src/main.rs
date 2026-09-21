//! A clap tool with the skill command beneath its root.

use std::process::ExitCode;
use std::sync::LazyLock;

use clap::{Arg, ArgAction, Command};
use include_dir::{Dir, include_dir};
use skill_embed::{Error, Installer, SkillSet};

static SKILLS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/skills");

static SKILLS: LazyLock<Installer> = LazyLock::new(|| {
    Installer::new(SkillSet::from_include_dir(&SKILLS_DIR).expect("the embedded skills are valid"))
        .with_tool_name("demo-clap")
        .with_version(env!("CARGO_PKG_VERSION"))
});

fn main() -> ExitCode {
    let cli = Command::new("demo-clap")
        .about("A tool that ships its own agent skills")
        .subcommand_required(true)
        .subcommand(
            Command::new("check")
                .about("Check some files")
                .arg(Arg::new("file").action(ArgAction::Append)),
        )
        .subcommand(skill_embed_clap::command(&SKILLS));

    let matches = cli.get_matches();
    match matches.subcommand() {
        Some(("check", args)) => {
            for file in args.get_many::<String>("file").into_iter().flatten() {
                println!("{file}: nothing to report");
            }
            ExitCode::SUCCESS
        }
        Some((name, args)) if name == SKILLS.command_name() => {
            match skill_embed_clap::run(&SKILLS, args) {
                // Help was asked for and printed, which is not a failure.
                Ok(()) | Err(Error::Help) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("demo-clap: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => ExitCode::FAILURE,
    }
}
