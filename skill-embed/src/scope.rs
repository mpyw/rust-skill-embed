use std::str::FromStr;

use crate::Error;

/// Where skills are installed.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, strum::Display, strum::IntoStaticStr,
)]
#[strum(serialize_all = "lowercase")]
pub enum Scope {
    /// The current project directory.
    #[default]
    Project,
    /// The user's home directory.
    User,
}

impl Scope {
    /// The name the `--scope` flag takes.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Parsed by hand rather than derived, so that a mistyped value comes back as
/// [`Error::UnknownScope`] and a front end never has to name another crate's
/// error type to match on it.
impl FromStr for Scope {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "project" => Ok(Self::Project),
            "user" => Ok(Self::User),
            _ => Err(Error::UnknownScope { value: s.to_owned() }),
        }
    }
}
