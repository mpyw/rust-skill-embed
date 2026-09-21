use crate::Installer;
use crate::cli::flag_block;
use crate::textfmt::{self, Columns};

impl Installer {
    /// The help text for the skill command. A tool that writes its own help can
    /// print it, so that the two agree.
    #[must_use]
    pub fn usage(&self) -> String {
        self.usage_for(None)
    }

    /// One line naming the skill command, for a tool whose own help would
    /// otherwise never mention it.
    ///
    /// ```no_run
    /// # let skills: skill_embed::Installer = unimplemented!();
    /// eprintln!("{}", skills.usage_hint());
    /// ```
    ///
    /// ```text
    /// Run "mytool skill" to install the 2 agent skills embedded in mytool.
    /// ```
    #[must_use]
    pub fn usage_hint(&self) -> String {
        let n = self.skills().len();
        let plural = if n == 1 { "skill" } else { "skills" };
        format!(
            "Run \"{tool} {cmd}\" to install the {n} agent {plural} embedded in {tool}.",
            tool = self.tool_name(),
            cmd = self.command_name(),
        )
    }

    /// The help for one subcommand, or for the command itself when `sub` is
    /// [`None`].
    #[must_use]
    pub fn usage_for(&self, sub: Option<&str>) -> String {
        let (summary, lines) = self.headings(sub);
        let default_agents: Vec<String> =
            self.default_agents().iter().map(ToString::to_string).collect();

        let mut skills = Columns::default();
        for sk in self.skills().skills() {
            skills.push([format!("  {}", sk.name()), textfmt::first_line(sk.description())]);
        }

        let flags =
            flag_block(&self.agent_choices(), &default_agents.join(","), self.default_scope());
        let out = format!(
            "{summary}\n\nUsage:\n{lines}\nFlags:\n{flags}\nEmbedded skills:\n{}",
            skills.render()
        );
        textfmt::trim_lines(&out)
    }

    /// The first line and the usage line of each subcommand's help.
    fn headings(&self, sub: Option<&str>) -> (String, String) {
        let (tool, cmd) = (self.tool_name(), self.command_name());
        let line =
            |name: &str, pad: &str| format!("  {tool} {cmd} {name}{pad} [flags] [skill...]\n");
        match sub {
            // Naming the removal here because `--help` is the only surface many
            // people read, and a subcommand called install deleting a directory
            // is not something to leave to the README.
            Some("install") => (
                format!(
                    "Install the agent skills embedded in {tool}, \
                     and remove the ones it no longer carries."
                ),
                line("install", ""),
            ),
            Some("uninstall") => {
                (format!("Remove the agent skills embedded in {tool}."), line("uninstall", ""))
            }
            Some("list") => (
                format!("Show the agent skills embedded in {tool}, and where each one stands."),
                line("list", ""),
            ),
            _ => (
                format!("Manage the agent skills embedded in {tool}."),
                line("install", "  ") + &line("uninstall", "") + &line("list", "     "),
            ),
        }
    }
}
