# rust-skill-embed

[![CI](https://github.com/mpyw/rust-skill-embed/actions/workflows/ci.yml/badge.svg)](https://github.com/mpyw/rust-skill-embed/actions/workflows/ci.yml)

| Crate | Reference |
| --- | --- |
| `skill-embed` | [![docs.rs](https://img.shields.io/docsrs/skill-embed)](https://docs.rs/skill-embed) |
| `skill-embed-clap` | [![docs.rs](https://img.shields.io/docsrs/skill-embed-clap)](https://docs.rs/skill-embed-clap) |

Ship agent skills inside a Rust binary, and give that binary a `skill install`
command.

`gh skill install` fetches skills from a GitHub repository. This library does
the same job from the other side. A tool carries its own skills in the
executable, and writes them wherever the user's agent reads from. The flags
match `gh skill install`. A user who knows that command already knows this one.

## Install

```bash
cargo add skill-embed include_dir
```

## Quick start

Skills live under `skills/<name>/SKILL.md`. That is the layout defined by the
[Agent Skills specification](https://agentskills.io/specification).

```rust,no_run
use std::process::ExitCode;
use std::sync::LazyLock;

use include_dir::{Dir, include_dir};
use skill_embed::{Installer, SkillSet};

static SKILLS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/skills");

static SKILLS: LazyLock<Installer> = LazyLock::new(|| {
    Installer::new(SkillSet::from_include_dir(&SKILLS_DIR).expect("valid skills"))
        .with_tool_name("mytool")
        .with_version(env!("CARGO_PKG_VERSION"))
});

fn main() -> ExitCode {
    if let Some(code) = SKILLS.intercept() {
        return code;
    }
    // the rest of your tool
    ExitCode::SUCCESS
}
```

### Files an operating system leaves behind

`include_dir!` takes everything it finds. That includes `.DS_Store`,
`Thumbs.db`, `desktop.ini` and `.localized`.

| Where the file is | What happens |
| --- | --- |
| Inside an embedded skill | `SkillSet` refuses the skill and names the file |
| Beside the skills, inside no skill | Ignored. Nothing installs it |
| Beside an installed skill | Ignored. The skill still reads as `up-to-date` |

One inside a skill was committed, and it ships to everyone. One beside the
skills ships too, and is never installed, so refusing it would stop a binary
over a file its skills do not contain. An installed skill sits in a directory a
user may open in a file browser. Such a file appears there on its own.

## Where skills go

The directories match `gh skill install`.

| Agent | Constant | Project scope | User scope |
| --- | --- | --- | --- |
| `github-copilot` | `Agent::GITHUB_COPILOT` | `.agents/skills` | `~/.copilot/skills` |
| `claude-code` | `Agent::CLAUDE_CODE` | `.claude/skills` | `~/.claude/skills` |
| `cursor` | `Agent::CURSOR` | `.agents/skills` | `~/.cursor/skills` |
| `codex` | `Agent::CODEX` | `.agents/skills` | `~/.codex/skills` |
| `gemini` | `Agent::GEMINI` | `.agents/skills` | `~/.gemini/skills` |
| `antigravity` | `Agent::ANTIGRAVITY` | `.agents/skills` | `~/.gemini/antigravity/skills` |

> [!IMPORTANT]
> Project scope resolves against the project. It does not resolve against the
> working directory.
>
> | Where the command runs | Where the skills go |
> | --- | --- |
> | A directory already holding `.agents` or `.claude` | That directory |
> | Anywhere else inside a repository | The repository root |
> | Outside a repository | The working directory |
> | The home directory | Refused, with `Error::ProjectIsHome` |
> | Outside the project, through a symbolic link | Refused, with `Error::ProjectEscapes` |
>
> The search walks up from the working directory. It stops at the repository
> root. The home directory holds the user scope directories. A project
> installation there would sit in front of every other project. `--scope user`
> writes there on purpose, and `--dir` names any directory outright.
>
> A project install writes where the project's own tree says. Every write
> follows a path component, whatever the path reads as. A symbolic link at
> `.claude/skills` therefore decides where the bytes land. So does one at any
> directory above it. A link is something a repository can carry: git stores
> one as mode `120000`, so it survives a clone. The destination is resolved and
> refused when it leaves the project root. A link that stays inside the project
> is the project's own arrangement, and is followed.
>
> The bound is project scope alone. User scope and `--dir` are the user naming
> a place. A home directory moved with a link keeps working.
>
> The answer depends on where the command was run. Every project scope run
> therefore names it.
>
> ```text
> Project root: /home/me/repo
> ```

Five of the six share `.agents/skills` at project scope. Selecting several of
them resolves to one directory. Each skill is written there once.

`--agent` also takes two words.

| Value | Variant | Meaning |
| --- | --- | --- |
| `detected` | `AgentSelector::Detected` | The agents whose directory is already there. The default |
| `all` | `AgentSelector::All` | Every agent, present or not |

`--agent` is repeatable, and one value may be a comma separated list.
`--agent claude-code --agent cursor` and `--agent claude-code,cursor` name the
same two.

`InstallOptions::agents` holds `AgentSelector` values. A selector for one agent
comes from the agent itself.

```rust
use skill_embed::{Agent, AgentSelector, InstallOptions};

let options = InstallOptions {
    agents: vec![AgentSelector::from(&Agent::CLAUDE_CODE)],
    ..InstallOptions::default()
};
```

`detected` falls back to `all` when it finds nothing. A fresh repository still
gets its skills. In a repository that already holds `.claude`, only Claude Code
is written to. In a home directory it is the agents in use, rather than six
directories of which most are litter.

Claude Code moves its whole configuration with `CLAUDE_CONFIG_DIR`. User scope
follows that variable when it is set.

> [!NOTE]
> `gh skill install` defaults to `github-copilot`, and prompts for the agent
> when it can. A tool that embeds its skills is rarely able to prompt. That
> default writes only `.agents/skills`, which Claude Code does not read.
> `Installer::with_default_agents` restores the `gh` behaviour.

## The command

```text
$ demo-lint skill
Manage the agent skills embedded in demo-lint.

Usage:
  demo-lint skill install   [flags] [skill...]
  demo-lint skill uninstall [flags] [skill...]
  demo-lint skill list      [flags] [skill...]

Flags:
      --agent <AGENT>  Target agent: {github-copilot|claude-code|cursor|codex|gemini|antigravity}, or all, or detected (repeatable) [default: detected]
      --dir <DIR>      Install to a custom directory (overrides --agent and --scope)
      --scope <SCOPE>  Installation scope: {project|user} [default: project]
  -f, --force          Overwrite existing skills
      --dry-run        Report what would happen without writing
  -h, --help           Show this help

Embedded skills:
  example-adoption  Stand-in skill for the demo-lint example. A real linter ships the skill that explains how to act ...
```

That is `examples/demo-lint` in this repository, run for real.
`demo-lint skill install --help` answers the same way, for that subcommand
alone. `list` and `uninstall` also answer to `ls` and `remove`, in both front
ends.

A run reports one line per skill per destination.

```text
$ demo-lint skill install --agent claude-code
Project root: /private/tmp/readme-demo/repo

installed  example-adoption  /private/tmp/readme-demo/repo/.claude/skills/example-adoption
```

```text
$ demo-lint skill list --agent claude-code
Project root: /private/tmp/readme-demo/repo

example-adoption
  Stand-in skill for the demo-lint example. A real linter ships the skill that explains how to act ...

SKILL             STATE       PATH
example-adoption  up-to-date  /private/tmp/readme-demo/repo/.claude/skills/example-adoption
```

### Naming the command in your own help

> [!WARNING]
> Your tool's own help says nothing about the skill command. `intercept` runs
> before your arguments are parsed. Nobody finds the command unless you name
> it.

`usage_hint` is that line. It tracks the command name and the skill count, so
it cannot drift from what the command actually does.

```rust
use skill_embed::Installer;

fn usage(skills: &Installer) {
    eprintln!("usage: mytool [file...]\n\n{}", skills.usage_hint());
}
```

```text
Run "mytool skill" to install the 2 agent skills embedded in mytool.
```

`Installer::usage` returns the full help text, for a tool that writes its own.

## What install does

Every installed `SKILL.md` gains four frontmatter keys.

```yaml
x-embedded-by: demo-lint
x-embedded-version: 0.0.0
x-embedded-at: "2026-09-21T00:51:51Z"
x-embedded-digest: "sha256:ec3efba1135f122842cb9215c4396cdad467af7dd11323e4bbbb2583c2836852"
```

The digest is what makes a second run safe. It covers the whole skill
directory. The manifest is hashed with these four keys removed. An installed
copy and its embedded original therefore hash the same.

| State | Meaning | What install does |
| --- | --- | --- |
| `missing` | Nothing is there | Writes it |
| `up-to-date` | The installed copy matches | Skips it |
| `outdated` | Not what this binary would write | Overwrites it |
| `modified` | The user edited it after installing | Skips it, and reports `Error::NeedsForce` |
| `foreign` | Not something this tool wrote | Skips it, and reports `Error::NeedsForce` |
| `orphaned` | This tool wrote it, it is unchanged, and the binary no longer carries it | Removes it |

> [!IMPORTANT]
> A version that drops or renames a skill leaves the old directory behind.
> Nothing would ever reach it again. Every walk starts from the embedded set.
> `install` would pass it by, `list` would not mention it, and `uninstall`
> would leave it there for good. The agent, meanwhile, goes on reading it.
>
> `install` therefore removes it. All three of these have to hold, and each one
> is doing work:
>
> | | |
> | --- | --- |
> | `x-embedded-by` names this tool | Nothing anybody else put there is in reach, including another tool built on this library |
> | The contents still hash to the recorded digest | It is byte for byte what this tool left. Nothing is lost that the binary could not write again |
> | The binary has no skill of that name | It is not something still being installed |
>
> A directory that fails the digest is not removed, and not reported. It held
> this tool's work once and holds something else now. That is what a shipped
> skill copied and then edited into one of the user's own looks like.
>
> `--force` has no part in this. It exists to overwrite what is in the way of
> an installation. Nothing is being installed over an orphan, so there is no
> conflict for it to resolve.
>
> | | |
> | --- | --- |
> | `install` | Removes it, and says so |
> | `install <name>` | Leaves it, unless it is one of the names |
> | `uninstall <name>` | Reaches one, since `list` prints them |
> | `install --dry-run` | Reports the removal without making it |
> | `uninstall` | Removes it, so a full uninstall leaves nothing of this tool's |
>
> This applies to both scopes, and to `--dir`.

A skipped skill does not stop the others. `install` writes everything it can.
It returns one `InstallResult` per skill either way. The outcome beside them is
`Error::NeedsForce` when it left anything alone.

```rust
use skill_embed::{Error, InstallOptions, Installer, Result, render_results};

fn install(skills: &Installer, options: &InstallOptions) -> Result<()> {
    let (results, outcome) = skills.install(options);
    print!("{}", render_results(&results, options.dry_run));
    if let Err(Error::NeedsForce(blocked)) = &outcome {
        eprintln!("{blocked}");
    }
    outcome
}
```

`foreign` is wider than "another tool put it there". It also covers a
hand-written skill. It covers a directory with no `SKILL.md`, and one whose
`SKILL.md` has no `x-embedded-*` keys. It covers one this tool cannot read at
all, such as a directory holding a symlink. Nothing can be said about any of
them, so nothing is claimed. `--force` remains the way through.

> [!WARNING]
> `Installer::with_metadata(false)` turns the four keys off. Install can then
> no longer tell an outdated copy from an edited one. Every existing directory
> reads as `foreign`.

## Front ends

| Framework | Crate | How |
| --- | --- | --- |
| None, or one with no hook | `skill-embed` | `SKILLS.intercept()` |
| [clap](https://docs.rs/clap) | `skill-embed-clap` | `root.subcommand(skill_embed_clap::command(&SKILLS))` |

> [!IMPORTANT]
> `intercept` takes the first argument. Check that `skill` does not already
> mean something in your tool.
>
> | Your tool | Can `skill` already mean something else? |
> | --- | --- |
> | A tool with subcommands | No. The first argument is a subcommand |
> | A tool that takes file names | **Yes**, if a file is called `skill` |
>
> For the last row, reach the file as `./skill`. The other way out is
> `Installer::with_command_name`.

### Without a framework

`intercept` looks at the first argument and nothing else. `mytool skill
install` reaches it. `mytool -v skill install` does not. Put your own flags
after the subcommand, or before a normal run.

`Installer::run` is for a tool that has already parsed its own arguments. It
never exits the process, so a driver keeps control.

### clap

```rust
use clap::Command;
use skill_embed::{Installer, Result};

fn run(skills: &Installer) -> Result<()> {
    let cli = Command::new("mytool").subcommand(skill_embed_clap::command(skills));
    let matches = cli.get_matches();
    if let Some((name, args)) = matches.subcommand()
        && name == skills.command_name()
    {
        skill_embed_clap::run(skills, args)?;
    }
    Ok(())
}
```

`skill_embed_clap::options` reads the flags without running anything. A tool
that builds its own `Command` uses it to reach the same options.

## Options

Every option is a method on `Installer`, and each one returns the installer.

| Method | Default | |
| --- | --- | --- |
| `with_tool_name` | The binary's name | Recorded in `x-embedded-by` |
| `with_version` | Empty | Recorded in `x-embedded-version` |
| `with_command_name` | `skill` | The subcommand `run` and `intercept` answer to |
| `with_agents` | All six | Restricts what `--agent` accepts, and which directories the project root search looks for |
| `with_default_agents` | `detected` | Used when `--agent` is absent |
| `with_default_scope` | `Scope::Project` | Used when `--scope` is absent |
| `with_project_root` | The searched project root | What project scope resolves against |
| `with_metadata` | On | Writes the four `x-embedded-*` keys |
| `with_executable` | Shebang test | Decides which files become executable |
| `with_output` | Standard output | Where every front end writes the report, and where `run` writes the help |
| `with_error_output` | Standard error | Where `run` writes a complaint and the usage |

> [!CAUTION]
> An embedded file carries no mode. Every one of them arrives read-only. A
> script installed without repair cannot be run by the agent. The default marks
> any file starting with `#!` as executable. Pass `with_executable` when your
> scripts have no shebang.

## Using it as a library

`Installer::status` reports without changing anything. `install` and
`uninstall` return one `InstallResult` per skill per destination.

```rust
use skill_embed::{Agent, AgentSelector, InstallOptions, Installer, Result, Scope, render_results};

fn install_for_claude_code(skills: &Installer) -> Result<()> {
    let (results, outcome) = skills.install(&InstallOptions {
        agents: vec![AgentSelector::from(&Agent::CLAUDE_CODE)],
        scope: Some(Scope::User),
        ..InstallOptions::default()
    });
    print!("{}", render_results(&results, false));
    outcome
}
```

`render_results` and `render_status` turn those values into the text the
built-in command prints. `Installer::write_report` sends that text where
`with_output` points, so a front end that calls all three reports the same way
to the same place. The clap adapter does.

`InstallOptions::cancel` stops a run between skills. A single skill is written
whole or not at all, so the flag is read between them rather than during one.

Every error a user's own input can cause is a variant of `Error`. A front end
can tell a mistyped flag from a disk that is full.

| Variant | Cause |
| --- | --- |
| `UnknownAgent` | `--agent` named no agent |
| `UnknownScope` | `--scope` was neither `project` nor `user` |
| `UnknownSkill` | A named skill is not embedded |
| `NoAgentSelected` | The values resolved to nothing |
| `NeedsForce` | A destination was left alone. `ForceRequired` names them |
| `ProjectIsHome` | Project scope resolved to the home directory |
| `ProjectEscapes` | A project scope destination is outside the project root |
| `Cancelled` | `InstallOptions::cancel` was set |
| `Help` | Help was printed. Not a failure |

## Embedding some other way

`SkillSet::from_include_dir` is one of three ways in. All three end in the same
place.

| Source | Constructor |
| --- | --- |
| `include_dir!` | `SkillSet::from_include_dir` |
| A directory on disk | `SkillSet::read_dir` |
| Anything else, such as `rust-embed` | `SkillSet::from_files` |

`from_files` takes a `File` per path, so a tool that embeds its skills some
other way needs no feature flag. Turn the `include_dir` feature off when you
use it.

```toml
skill-embed = { version = "0.1", default-features = false }
```

## The skill for this library

`skill-embed/skills/skill-embed-adoption/SKILL.md` covers adopting the library.
It names which front end to choose, the traps that are silent, and how to check
the result. Install it into a repository that is about to embed skills.

```bash
gh skill install mpyw/rust-skill-embed skill-embed-adoption --agent claude-code
```

## Releasing

Dispatch **Tag and Release** with a version, such as `v0.1.0`. It tags, then
publishes to crates.io, then cuts the GitHub release.

That order matters. A published version can never be published again, and a
GitHub release freezes its tag. Creating the release first would burn the
version number on a run that failed half way, so it is the last thing that
happens.

Every step is safe to run again with the same version.

| | On a second run |
| --- | --- |
| Tag | Left alone when it is already on this commit, refused when it is on another |
| Publish | Each crate is asked about, and the ones already on crates.io are excluded |
| Release | Left alone when it is already there |

So a run that failed part way is finished by dispatching **Release** on its
own with the same version.

### The first release, and the crates.io settings

The first version of each crate goes up from a laptop, because a trusted
publisher can only be configured for a crate that already exists.

```bash
cargo login              # a token, once, on this machine
cargo publish --workspace
```

Then, in each crate's Settings on crates.io, under Trusted Publishing, add
**two** configurations:

| Field | Usual path | Retry path |
| --- | --- | --- |
| Repository owner | `mpyw` | `mpyw` |
| Repository name | `rust-skill-embed` | `rust-skill-embed` |
| Workflow filename | `tag_and_release.yml` | `release.yml` |
| Environment | `release` | `release` |

Two, because crates.io matches the `workflow_ref` claim. That claim names the
top level workflow rather than the reusable one it called, so the usual path
arrives as `tag_and_release.yml` and a retry dispatched on its own arrives as
`release.yml`.

No token is stored in this repository. The workflow asks GitHub for one that
says which workflow is running, hands it to crates.io, and gets back one that
lasts thirty minutes. Turning on "require Trusted Publishing" in the same
settings then refuses a publish from a token at all.

## Development

```bash
./test_all.sh
```

The toolchain is pinned in `rust-toolchain.toml`. Help text is asserted with
[`expect-test`](https://docs.rs/expect-test). Run
`UPDATE_EXPECT=1 cargo test` after changing a flag or a default.

Every Rust block in this file is compiled as a doctest, so an example cannot
drift from a signature.

## Relation to go-skill-embed

[go-skill-embed](https://github.com/mpyw/go-skill-embed) is the same library
for Go. The two agree on what they write: the agent directories, the four
frontmatter keys, and the digest. Under one tool name, an installation made by
either reads as `up-to-date` to the other, and the two trees are identical
apart from `x-embedded-at`. `tests/digest.rs` pins the digest to the value the
Go library produces.

## License

MIT
