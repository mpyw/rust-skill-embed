use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::InstallStatus;

/// Everything this library refuses to do, and why.
///
/// Each variant a user's own input can cause is one a front end can match on,
/// so a mistyped flag is told apart from a disk that is full:
///
/// ```no_run
/// # let skills: skill_embed::Installer = unimplemented!();
/// # let options = skill_embed::InstallOptions::default();
/// let (results, outcome) = skills.install(&options);
/// print!("{}", skill_embed::render_results(&results, options.dry_run));
/// if let Err(skill_embed::Error::NeedsForce(_)) = outcome {
///     // tell the user to re-run with --force
/// }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// `--agent` named no agent this tool offers.
    #[error("unknown agent {name:?} (want one of {}, or all, or detected)", valid.join(", "))]
    UnknownAgent {
        /// The name that was given.
        name: String,
        /// The names that would have been accepted.
        valid: Vec<String>,
    },

    /// `--scope` was neither `project` nor `user`.
    #[error("unknown scope {value:?} (want project or user)")]
    UnknownScope {
        /// The value that was given.
        value: String,
    },

    /// A named skill is not embedded.
    #[error("unknown skill {name:?} (embedded: {})", embedded.join(", "))]
    UnknownSkill {
        /// The name that was given.
        name: String,
        /// The names the binary carries.
        embedded: Vec<String>,
    },

    /// The `--agent` values resolved to nothing at all.
    #[error("no agent selected")]
    NoAgentSelected,

    /// A destination held something this tool did not write, or something
    /// edited after it did.
    ///
    /// Install leaves those skills alone and writes the rest, so the results it
    /// returns alongside this are still worth reporting.
    #[error(transparent)]
    NeedsForce(#[from] ForceRequired),

    /// Project scope resolved to the home directory.
    ///
    /// The user scope directories live there. A project installation written
    /// into them would put one project's skills in front of every other
    /// project, and `--scope user` already writes there on purpose.
    #[error(
        "the search landed on the home directory ({}), so pass --scope user to write there \
         on purpose or --dir to name another directory",
        dir.display()
    )]
    ProjectIsHome {
        /// The directory the search landed on.
        dir: PathBuf,
    },

    /// A project scope destination is outside the project root.
    ///
    /// The path a project install writes to comes from the project, so a link
    /// committed at `.claude/skills`, or at any directory above it, aims the
    /// write and a later forced removal wherever it points. Cloning a
    /// repository and running the tool once is the whole of it. User scope and
    /// `--dir` are the user naming a place, and are not bounded this way.
    #[error(
        "the destination is outside the project root ({} is really {}), so pass --dir to \
         write there on purpose",
        dir.display(),
        real.display()
    )]
    ProjectEscapes {
        /// The destination as it was resolved.
        dir: PathBuf,
        /// Where it really lands.
        real: PathBuf,
    },

    /// The command line could not be read.
    #[error("{0}")]
    Usage(String),

    /// The embedded skills could not be read as skills. It is a build-time
    /// mistake rather than a runtime condition.
    #[error("{0}")]
    Skills(String),

    /// The run was cancelled through [`InstallOptions::cancel`].
    ///
    /// [`InstallOptions::cancel`]: crate::InstallOptions::cancel
    #[error("cancelled")]
    Cancelled,

    /// Help was asked for and printed. It is not a failure.
    #[error("help requested")]
    Help,

    /// Something the file system refused.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// What the file system said.
        #[source]
        source: io::Error,
    },
}

impl Error {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io { context: context.into(), source }
    }
}

/// Every destination install left alone, and why the list is worth printing.
///
/// Its message runs to several lines, one per destination, which is why it is a
/// type of its own rather than a variant with a one line format string.
#[derive(Debug)]
pub struct ForceRequired {
    /// The state of each destination that was not written.
    pub blocked: Vec<InstallStatus>,
}

impl fmt::Display for ForceRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "refusing to overwrite skills this tool did not install, \
             or that were edited after installing:\n",
        )?;
        for st in &self.blocked {
            writeln!(f, "  {} ({})", st.path.display(), st.state)?;
        }
        f.write_str("re-run with --force to overwrite")
    }
}

impl std::error::Error for ForceRequired {}

/// The result of anything in this crate that can be refused.
pub type Result<T, E = Error> = std::result::Result<T, E>;
