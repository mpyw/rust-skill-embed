/// What is already present at a destination.
///
/// The name it prints comes from the variant, through [`std::fmt::Display`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum State {
    /// Nothing is installed there yet.
    Missing,
    /// The installed copy matches the embedded skill.
    UpToDate,
    /// This tool installed it and it is not what the binary would write now.
    /// The binary carries a newer copy, or a file lost the executable bit it
    /// was installed with.
    Outdated,
    /// This tool installed it and the files were edited afterwards.
    Modified,
    /// Something else owns a skill of that name there.
    Foreign,
    /// This tool wrote it, it is still byte for byte what was written, and the
    /// binary no longer carries a skill of that name. An earlier version
    /// installed it and nothing would reach the directory again.
    Orphaned,
}

impl State {
    /// Reports whether overwriting this state would destroy work that this tool
    /// did not create.
    #[must_use]
    pub const fn needs_force(self) -> bool {
        matches!(self, Self::Modified | Self::Foreign)
    }
}
