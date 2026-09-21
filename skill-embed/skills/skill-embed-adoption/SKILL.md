---
name: skill-embed-adoption
description: Ship Agent Skills inside a Rust binary with skill-embed, and give that binary a skill install command. Read this when adding the crate to a tool, when choosing a front end, or when an install reports a state that is hard to act on. Covers the wiring for each front end, the traps that are silent, and how to check the result.
license: MIT
---

# Adopting skill-embed

Written against **skill-embed 0.1**. Check the version first.

Read [the README](https://github.com/mpyw/rust-skill-embed#readme) for the API.
This covers the decisions and the traps.

## Decide three things first

| Decision | Options | Default |
| --- | --- | --- |
| Front end | `intercept`, `run`, or the clap adapter | none |
| Agents | every built-in agent, or a subset through `with_agents` | every one |
| Command name | any word | `skill` |

The front end follows from what the tool already is.

| The tool is | Use |
| --- | --- |
| A [clap](https://docs.rs/clap) tool | `skill_embed_clap::command` |
| A tool with no framework | `intercept`, as the first thing `main` does |
| A tool built on something with no hook | `intercept`, before that thing starts |
| Something that parses its own arguments | `run`, which returns errors instead of exiting |

## Wire it

Skills live under `skills/<name>/SKILL.md`. That layout is the
[Agent Skills specification](https://agentskills.io/specification), and the
discovery reads no other.

```rust
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
    ExitCode::SUCCESS
}
```

`SkillSet::from_include_dir` is one of three ways in. Use `SkillSet::read_dir`
for a tool that does not embed its skills. Use `SkillSet::from_files` for any
other embedding crate, and turn the `include_dir` feature off.

## Four traps

Each is silent. None produces an error at the point where it goes wrong.

**An embedded file carries no mode.** A script arrives read-only. Any file
starting with `#!` is installed `0755`, and `with_executable` decides for a
script with no shebang.

**`include_dir!` takes everything it finds.** A `.DS_Store` committed beside a
skill is embedded with it. `SkillSet` refuses that skill and names the file, so
the failure is loud. It is still worth a `.gitignore` entry.

**Installing writes into the skill's own `SKILL.md`.** Four `x-embedded-*` keys
go into the frontmatter, and they are what a later run compares against.
`with_metadata(false)` removes them, and every installed copy then reads as
`foreign`.

**`intercept` reads the first argument and nothing else.** `mytool skill
install` reaches it. `mytool -v skill install` does not. A tool's own help says
nothing about the command either, so print `usage_hint` where your usage goes.

## What a state means

| State | The installed copy | What install does |
| --- | --- | --- |
| `missing` | Not there | Writes it |
| `up-to-date` | Matches | Skips it |
| `outdated` | Not what this binary would write | Overwrites it |
| `modified` | Edited after installing | Skips it, and reports `Error::NeedsForce` |
| `foreign` | Not something this tool wrote | Skips it, and reports `Error::NeedsForce` |
| `orphaned` | Written by this tool, unchanged, and no longer embedded | Removes it |

`outdated` covers a newer copy in the binary, and a file that lost its
executable bit. `foreign` covers a hand-written skill, a directory with no
`SKILL.md`, and one that cannot be read at all.

> [!WARNING]
> A full `install` deletes directories, not only writes them. Once your tool
> drops or renames a skill, nothing else would ever reach the copy an earlier
> version installed. `install` therefore removes it. Say so in your own release
> notes when you drop one.
>
> It is claimed only when `x-embedded-by` names your tool, the contents still
> hash to the recorded digest, and the binary has no skill of that name.
> Anything edited since is left alone. So is anything another tool wrote, a
> hand-written skill, and anything `.`-prefixed. `--force` does not change
> that, and `install <name>` sweeps nothing it was not told to.

A skipped skill does not stop the others. `install` returns one result per
skill either way. The outcome beside them only says that something was left
alone, so print the results first.

## Check the result

Run the real binary. The states are what the command reports, so a wiring
mistake shows up here and in no test.

```bash
cargo build || exit 1
BIN=target/debug/mytool

$BIN skill                      # the command, and the skills it carries
$BIN skill install --dry-run    # where they would go
$BIN skill install
$BIN skill list                 # every embedded row should read up-to-date
```

An `orphaned` row is a directory an earlier build of your tool installed and
this one no longer carries. On a first adoption there should be none. If there
is one, check that `with_tool_name` matches what the earlier build used.

> [!IMPORTANT]
> Project scope resolves against the project, not the working directory. The
> search walks up to the repository root, and the home directory is refused.
> Run the check from a subdirectory as well. Every project scope run prints
> `Project root:`, so compare that line between the two runs.

## Traps when measuring

**A skill installed by an older build may read as `modified`.** The recorded
digest was computed by that build. Changing what the digest covers changes the
answer, and the user sees a file they never edited.

**`--dir` skips the agent table.** A check that passes with `--dir` says
nothing about where a real install lands.

**`--agent detected` depends on the machine.** It keeps the agents whose
directory is already there, and falls back to every agent when it finds none.
Two machines give two answers for the same command.
