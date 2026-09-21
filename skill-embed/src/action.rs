/// What install or uninstall did at one destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum Action {
    /// The skill was written where nothing was.
    Installed,
    /// An existing installation was replaced.
    Updated,
    /// The skill directory was deleted.
    Removed,
    /// Nothing was done. [`InstallResult::reason`] says why.
    ///
    /// [`InstallResult::reason`]: crate::InstallResult::reason
    Skipped,
}

impl Action {
    /// The name this action prints under.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}
