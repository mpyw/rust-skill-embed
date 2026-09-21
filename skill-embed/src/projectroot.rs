//! Finds the directory a project scope install writes into.
//!
//! Nothing here knows about skills. It walks a file system and answers with a
//! directory.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths;

/// Why a root could not be chosen.
#[derive(Debug)]
pub(crate) enum FindError {
    /// The search landed on the home directory.
    IsHome(PathBuf),
    Io(io::Error),
}

impl fmt::Display for FindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IsHome(dir) => {
                write!(f, "the search landed on the home directory ({})", dir.display())
            }
            Self::Io(e) => e.fmt(f),
        }
    }
}

impl From<io::Error> for FindError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Chooses the root, starting at `dir` and walking up.
///
/// The walk stops at the repository root. The first directory that already
/// holds one of the markers wins. Without one, the repository root does.
/// Outside a repository the walk is one step, because there is no bound to walk
/// within.
///
/// The bound matters. A home directory holds a marker for every agent its owner
/// uses, so an unbounded walk from anywhere under it reaches one and turns a
/// project installation into a user-wide one. Landing on the home directory
/// itself is [`FindError::IsHome`] for the same reason.
pub(crate) fn find(dir: &Path, markers: &[PathBuf]) -> Result<PathBuf, FindError> {
    let abs = paths::absolute(dir)?;
    let bound = repo_root(&abs).unwrap_or_else(|| abs.clone());

    let mut chosen = bound.clone();
    let mut at: &Path = &abs;
    loop {
        if has_marker(at, markers) {
            chosen = at.to_path_buf();
            break;
        }
        if at == bound {
            break;
        }
        match at.parent() {
            Some(parent) if parent != at => at = parent,
            _ => break,
        }
    }

    if let Ok(home) = paths::home()
        && paths::same_dir(&chosen, &home)
    {
        return Err(FindError::IsHome(chosen));
    }
    Ok(chosen)
}

/// The nearest ancestor of `dir` holding a `.git`, if any.
///
/// A worktree and a submodule have a `.git` file rather than a directory, so
/// both count.
fn repo_root(dir: &Path) -> Option<PathBuf> {
    let mut at = dir;
    loop {
        if at.join(".git").symlink_metadata().is_ok() {
            return Some(at.to_path_buf());
        }
        at = at.parent().filter(|p| *p != at)?;
    }
}

fn has_marker(dir: &Path, markers: &[PathBuf]) -> bool {
    markers.iter().any(|m| dir.join(m).is_dir())
}

/// A destination that leaves the root it was resolved against.
///
/// A symbolic link is what usually does it, but the text does not say so: an
/// agent whose project directory climbs out with `..` reaches this too, and
/// blaming a link that is not there sends the reader looking for one.
#[derive(Debug)]
pub(crate) struct OutsideRoot {
    pub(crate) dir: PathBuf,
    pub(crate) real: PathBuf,
}

impl fmt::Display for OutsideRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the destination is outside the project root ({} is really {})",
            self.dir.display(),
            self.real.display()
        )
    }
}

/// Why a destination could not be accepted.
#[derive(Debug)]
pub(crate) enum WithinError {
    Outside(OutsideRoot),
    Io(io::Error),
}

impl From<io::Error> for WithinError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Checks that `dir` stays inside `root` once symbolic links are resolved.
///
/// A path component is followed by every write, so a link at `dir` or above it
/// decides where the bytes land, whatever the path reads as. Comparing the two
/// literal strings cannot see that, and neither can a check on `dir` alone: the
/// link is usually a parent, and usually one the caller never names.
///
/// [`Path::starts_with`] compares whole components, so a sibling named
/// `project-notes` is outside a root named `project` without a separator having
/// to be spliced on to say so.
///
/// The bound is the reason. A project root is whatever was cloned, so a link
/// committed into it is someone else's choice, unlike a home directory moved
/// with one.
pub(crate) fn within(root: &Path, dir: &Path) -> Result<(), WithinError> {
    let real_root = paths::real(root)?;
    let real_dir = paths::real(dir)?;
    if real_dir.starts_with(&real_root) {
        return Ok(());
    }
    Err(WithinError::Outside(OutsideRoot { dir: dir.to_path_buf(), real: real_dir }))
}
