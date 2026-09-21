/// What install or uninstall did at one destination.
///
/// The name it prints comes from the variant, through [`std::fmt::Display`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, strum::Display)]
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
