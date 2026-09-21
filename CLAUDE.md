# Notes for agents working on this repository

Run `./test_all.sh` before claiming anything passes. It covers every crate.

| Section | What it holds |
| --- | --- |
| Where prose goes | Which file takes history, and which takes only the present |
| Rejected designs | What was tried, and the failure that ruled it out |
| Things that look wrong but are not | Deliberate oddities, so nobody "fixes" them |
| Known and left alone | Raised in review, and accepted |
| What the port changed | Where this differs from go-skill-embed, and why |

This library is a port of [go-skill-embed](https://github.com/mpyw/go-skill-embed).
The behaviour is the same by design, so a rationale recorded there still holds
here unless this file says otherwise.

## Where prose goes

| File | What belongs there |
| --- | --- |
| `README.md`, doc comments, source comments | The current state only. Brief |
| `CLAUDE.md` | History and rationale, including rejected designs |

A comment says why the code has its present shape, in the present tense. An
account of a past bug belongs here instead. A comment that only restates the
code is worth less than no comment.

## Rejected designs

### Dependencies

The rule is one line: take a dependency when the alternative is code this
project would have to get right and keep right. Write the code when the need is
a few dozen lines with a fixed definition and no edge cases the standard
library is hiding. Dev-dependencies are not weighed at all.

**Writing `Display` and `Error` by hand.** It was the first shape, and it
worked. `thiserror` replaced it because the impls have to stay in step with the
variants, and nothing checks that they do. `ForceRequired` is a type of its own
rather than a variant with a format string, because its message runs to one
line per blocked destination.

**Formatting RFC 3339 by hand.** `civil_from_days` is a named algorithm and the
tests covered it. `jiff` replaced it anyway: one timestamp is not worth a
calendar of this project's own. `default-features = false` keeps the bundled
time zone database out, since UTC is the whole of the need.

**A hand-rolled loop that reserves a unique staging directory name.**
`tempfile` does it, and it also removes the directory when the write fails.
That second part was a hand-written `Drop` guard before.

**The `home` or `dirs` crate.** `std::env::home_dir` was un-deprecated with its
Windows behaviour fixed, and the minimum supported Rust version is already past
that. A crate for one function that the standard library now answers is a
dependency that will outlive its reason.

### Types

**`State`, `Action` and `Scope` as string newtypes.** That is what Go has,
because Go has no better answer. Here they are enums, and `strum` derives
`Display` and `&'static str` from `#[strum(serialize_all = "kebab-case")]`.
`up-to-date` comes out of the variant name, so the table of strings is gone.

**Deriving `FromStr for Scope` with `strum::EnumString`.** It works, and it
makes `<Scope as FromStr>::Err` be `strum::ParseError`. A front end matching on
a bad `--scope` would then have to name another crate's type. The parse is four
lines and returns `Error::UnknownScope`.

**`AgentSelector` as a string.** Go uses one because an `Agent` value cannot
carry the words `all` and `detected`. An enum says exactly that, and
`AgentSelector::Named` holds the rest.

**`Agent` as an enum.** `with_agents` takes agents the library does not know
about, so `Agent` is a struct with associated constants. That is also why
`strum::EnumIter` and `VariantArray` do not apply to it: they are for enums.

**A `builtin_agents!` macro declaring the six constants and `builtins()`
together.** It removes one real mistake: a seventh agent added as a constant
and left out of the list is never reached by `--agent all`. It was still
rejected. Six constants inside a `macro_rules!` body read worse than six
constants, and the table in `tests/agent.rs` names every agent and asserts the
length, so the omission is a failing test.

**An `fs::FS`-shaped trait for the embedded tree.** Go needs one because
`embed.FS` is an `fs.FS`. Here a skill is a flat list of files in path order,
which is the whole of what a digest covers and the whole of what a write puts
back. One type serves the embedded side and the installed side, so both sides
of every comparison are built the same way. `Cow<'static, [u8]>` keeps an
embedded file borrowed and an installed one owned.

**Exposing the internal file type.** `SkillSet::from_files` takes a public
`File` that wraps it. The internal one carries a mode that only a read from disk
fills in, and a caller that hands over files has no business setting it.

### Frontmatter and digests

**A YAML library for the frontmatter.** `manifest.rs` edits bytes instead.
Install has to add four keys to a `SKILL.md` without disturbing the rest of it.
A parse and re-emit round trip rewrites quoting, key order and block scalars,
which produces a diff the skill author never wrote. It also means the digest
cannot be compared against the embedded original.

**Reading a manifest line as lossy UTF-8.** `String::from_utf8_lossy` changes
the bytes, and those bytes are what the digest covers. A line that is not UTF-8
is skipped instead. Nothing this reads for is spelled in anything else.

**Putting the files in path order by sorting the strings.** Measured against
go-skill-embed over a skill holding both `b.md` and `b/c.md`: the two digests
differed. A directory walk reaches `b` before `b.md`, because a directory
listing sorts `b` first and the walk descends at once. Sorting the paths as
strings puts `b.md` first, since `.` sorts before `/`. `walk_order` splits on
`/` and compares the components, which is the walk's order. `tests/digest.rs`
pins the value the Go library produces.

**Hashing the installed manifest as it stands.** The digest strips the four
injected keys first. Without that, an installed copy never hashes equal to the
skill it came from, and `up-to-date` can never be reported.

**A blank line between the added frontmatter and the body.** `manifest::with`
writes the closing `---` and then the body, with nothing between them, when it
creates a block that was not there. It reads slightly worse. It is also the
only way `strip` can restore the file byte for byte. Without that, a skill
whose `SKILL.md` had no frontmatter reads as `modified` the moment it is
installed.

**Letting `strip` remove a frontmatter block that turned out empty.** It could
not tell a block `with` had written from one the source already had. A skill
whose manifest carried an empty block then read as `modified` the moment it was
installed. `normalize` removes an empty block on both sides instead, which
makes the two spellings of "no frontmatter" hash alike. `strip` now only ever
removes the injected keys, and skips indented lines, because a block scalar may
hold a line that looks exactly like one.

**Hashing what the file system says about a mode.** An embedded file has no
mode at all, so the two sides of a comparison could never agree on one.
`executable_bits_match` applies the rule to the contents instead, which the
digest has already matched, so both sides reach the same answer. A lost
executable bit reads as `outdated` rather than `modified`, because the user did
not do it and repairing it should not need `--force`.

### Installing

**Removing the destination before renaming the staging directory in.** The
removal is not atomic either. One that failed half way left the old
installation destroyed and the new one unwritten, which is the opposite of what
the comment above it promised. The swap is two renames, with the old directory
moved aside in between and moved back if the second one fails.

**Refusing the whole run when one destination needs `--force`.** One `foreign`
directory stopped every other skill from installing anywhere. `install` reports
the same situation per skill now, as `Action::Skipped` with a reason, and the
outcome is `Error::NeedsForce`. Both verbs return results that mean something
even when the outcome is an error, and every front end prints them first.

**Bounding project scope with a string comparison.** A string prefix cannot see
a symbolic link, which is the only thing that moves a destination. Both sides
are resolved with `paths::real` first, as far as they exist, because a skills
directory usually does not yet.

**Splicing a separator on to keep `/a/project-notes` out of `/a/project`.**
`Path::starts_with` compares whole components, so nothing has to be spliced.
Go reaches the same answer through `filepath.Rel`, because its own prefix test
breaks at a volume root.

**Refusing every symbolic link on the way to a project destination.** A
repository that keeps its skills elsewhere in its own tree and links to them is
doing nothing wrong. What matters is where the link lands, not that there is
one.

**Bounding user scope the same way.** `~/.claude` moved onto another disk with
a link is a normal arrangement, and the user made it. The bound exists because
a project root is whatever was cloned, which is someone else's choice.

**Exempting a custom `Agent` whose project directory climbs out with `..`.** It
could be read as the embedding tool's own choice, the way `--dir` is. It is
refused with everything else, because a project install landing outside the
project is the thing being stopped, and who wrote the path does not change
that. `with_project_root` and `--dir` are how a tool aims elsewhere. The
refusal's text says "outside the project root" rather than naming a link, so
this reader is not sent looking for one.

**Re-checking at write time.** The check runs in `targets`, so a link created
between there and the rename is not caught. The case it is for is a link
committed into a repository, which is there before the run starts.

### Reading the environment

**Splitting a config directory variable on both `:` and `;`.** It reads as
covering either platform. A Windows path holds a `:` of its own, so
`CLAUDE_CONFIG_DIR=C:\\Users\\me\\.claude` was cut at the drive letter and user
scope resolved to `C\\skills`. Go reaches the right answer through
`filepath.ListSeparator`, which is one character per platform. `LIST_SEPARATOR`
is the same thing. The Windows CI job is what found it.

### Tests that read the platform

**Setting `HOME` to point a test at a temporary home directory.**
`std::env::home_dir` reads `HOME` on Unix and `USERPROFILE` on Windows. A test
that sets `HOME` sets nothing on Windows, and the subject under test then reads
the real home directory. Two end-to-end tests wrote a user scope install into
the runner's own home directory, and a project run from the temporary "home"
was not refused. `HOME_VAR` names the variable the platform reads, and both
`tests/scope.rs` and the example's tests use it.

**Comparing a resolved project root against `fs::canonicalize`.** It was added
because macOS answers `current_dir` with `/private/var/...` where the temporary
directory was handed out as `/var/...`. On Windows `canonicalize` answers with
a `\\?\` path, which nothing under test produces, so the fix for one platform
broke another. The search starts from the working directory, so `Env::cd`
answers with the working directory as the operating system gives it back, and
the expected paths are built from that. It needs no platform in it at all.

**Guarding a Unix-only test body with `#[cfg(not(unix))] return;`.** On Windows
the body is compiled out and the function is a bare `return;`, which clippy
reports. `#[cfg(unix)]` on the function is what every other test here uses.

### Cancellation

**A `context::Context` of this project's own.** Go passes one into every call.
Rust has no such convention for blocking code, and inventing one would put a
parameter on five methods that most callers would pass nothing to.

**Dropping cancellation entirely.** A long install over six agent directories
is worth stopping. `InstallOptions::cancel` is an `Option<Arc<AtomicBool>>`, so
a caller that does not want it writes nothing: every field has a default.

**Checking the flag between files inside one skill.** A half written skill is
worse than a skill that was never started. The check runs between skills, and
the doc comment says so.

### Sweeping

**Giving `--force` a part in it.** `--force` means "something is in the way of
what I am installing; overwrite it". Nothing is being installed over an orphan,
so there is no conflict. A flag that let the sweep take a directory whose
contents had changed was authorising a deletion nothing required. Worse, the
tool asks for `--force` whenever anything reads as modified, so following its
own advice destroyed a user's edited fork of a shipped skill. Contents that no
longer match are the evidence the directory is not this tool's any more. The
sweep just stops there, and the flag keeps one meaning.

**Finding them from a manifest of what was installed.** State lives in each
installed `SKILL.md`, which is why a half finished run is always recoverable
and why a version mismatch can never orphan a payload. The sweep reads the same
stamps, so nothing new has to be kept in step.

**Sweeping on `install <name>`.** Only a run over the whole set knows what is
missing from it. A named run is about those names. Removing a skill it was
never asked about on the way past is not something the user could have
predicted. Naming an orphan does reach it, because `list` prints one, and
refusing a name the tool just printed is the worse answer.

**Matching an orphan to an installed skill by folded name.**
`str::to_lowercase` is not the file system's equivalence relation, which is
recorded below. `paths::same_file` asks the file system instead. A skill
renamed to another spelling of itself is one directory whichever way it is
spelled. Left alone, the sweep deleted what the same run had just written and
reported success.

**Sweeping the `.tmp-` and `.old-` directories a write leaves.** The rescue
copy is a whole installation, stamp and digest and all, so it reads as an
orphan exactly. It is also the only copy left, and the error that abandoned it
names it for the user to recover. Every dot-prefixed entry is passed over.

**Testing the `x-embedded-by` check with a hand-made fixture.** A directory
carrying another name in `x-embedded-by` and a digest that does not match its
own contents is turned away by the digest, whether or not the name is ever
read, so the test passed with the check deleted. The fixture is a real
installation made by a second `Installer` with another tool name, where the
stamp is well formed and the digest checks out, and the name is the only thing
left between it and the sweep.

**Letting a digest failure out of `inspect`.** A symlink inside an installed
skill, which is a thing a user does, made the whole run fail. `install` and
`uninstall` both ask for the status first, so `--force` failed too and the only
way out was `rm -rf`. One such directory took every other skill at that target
with it. An unreadable copy now reads as `foreign`: nothing can be said about
what is there, so nothing is claimed.

### Embedding

**Trusting `include_dir!` to leave junk behind.** It takes everything it finds,
including `.DS_Store`. Junk files are handled from two directions on purpose.
`SkillSet::from_files` refuses one, because an embedded `.DS_Store` was
committed and ships. The digest and the write skip them, because an installed
skill sits where a file browser can reach it. Without the second part, opening
`~/.claude/skills/<name>` in Finder made the skill read as `modified`, and
install refused without `--force`.

### The command line

**Reading the arguments with clap in the core crate.** This library is embedded
in other people's tools. A clap version in the core would reach every
consumer's dependency graph, and a tool pinned to another major version could
not use it. The core parses its own flags, and `skill-embed-clap` is a crate of
its own.

**Stopping flag parsing at the first positional argument.** That is what Go's
`flag` package does, and the Go README has to warn about it. A hand written
parser has no reason to inherit it. `skill install demo --dry-run` means what
it looks like in both front ends here.

**Taking `gh skill install`'s default agent along with its flags.** It is
`github-copilot`, and `gh` prompts for the agent whenever it can. A tool that
embeds its skills is rarely able to prompt, and that default writes only
`.agents/skills`, which Claude Code does not read. A Claude Code user running
`mylint skill install` would have seen a success and got nothing. The default
is `detected`, which falls back to `all`.

**Exiting the process from `intercept`.** Go's version calls `os.Exit`, which a
Rust `main` returning `ExitCode` has no need for. `intercept` answers with
`Option<ExitCode>`, so the caller decides. `None` means the arguments were not
for this command.

## What an adversarial review found

Two reviewers read this port against go-skill-embed line by line, built probe
binaries for both libraries, and ran the same fixtures through each. Everything
below was measured, not argued. It is recorded because every one of these is a
place a port drifts silently, and the next port of this library will drift
there too.

### The digest, which is the promise

| | |
| --- | --- |
| A frontmatter block holding only U+00A0, a vertical tab or U+2028 | `is_ascii_whitespace` does not know them, so the block stayed and the two digests differed. `is_blank` decodes and asks `char::is_whitespace`, which is the `White_Space` property Go's `unicode.IsSpace` uses |
| An injected line whose value is not UTF-8 | The whole line failed to decode, so `strip` left it, the digest differed, and a second stamp wrote a duplicate key. Only the key is read as text now |
| A skill holding both `b.md` and `b/` | Recorded above under the sort |

`tests/digest.rs` pins the value the Go library produces. The fixture it builds
holds the interleaving names. The rest are unit tests in `manifest.rs`.

### What lands on disk

| | |
| --- | --- |
| A write that failed reported `installed` | The row was added before the I/O. It is added after now, so a failure prints its error and no row |
| The destination is a symbolic link | `symlink_metadata` does not follow one, so a link standing in for an installation read as `foreign` and a dangling one read as `foreign` rather than `missing`. `fs::metadata` follows, which is what the agent does |
| An orphan candidate is a symbolic link | `Path::is_dir` follows one, so the sweep claimed a link the user had put there and removed it. `DirEntry::file_type` does not follow, which is the entry's own type |
| A file's mode | `fs::write` then `set_permissions` ignores the umask, so under `umask 077` a manifest landed at 0644. The mode belongs to the creation |
| `x-embedded-digest: ""` | A key that is there and empty read as a claim, so an edited copy read as `modified` rather than `foreign` |
| `with_tool_name("")` | Stamped `x-embedded-by: ""`, which is foreign to every run including this tool's own. An empty name is passed over |

### The command line

| | |
| --- | --- |
| Any argument that is not UTF-8 | `std::env::args` panics, and `intercept` runs before the tool has read its own command line. A file name the tool would have handled took the whole process down. `args_os` throughout, and `intercept_args` takes `AsRef<OsStr>` |
| `--agent ""` | Parsed to nothing, and an empty list reads as "use the default" everywhere below, so the run silently installed for every agent. Only the front end knows the flag was given |
| `--dir ""` | Resolved to the working directory, so every skill landed loose in whatever directory the tool was run from |

### Deliberate divergences this review settled

These differ from go-skill-embed on purpose. Each is a place where the Go
behaviour looked accidental rather than chosen.

| | |
| --- | --- |
| A junk file that is not inside any skill | Go never walks it, because it only descends into directories holding a `SKILL.md`. Refusing the whole set over it crashed a user's binary over a file the skills do not contain. The check applies inside a skill alone |
| A custom agent whose project directory is one component, such as `skills` | Go takes `filepath.Dir("skills")`, which is `"."`, and a marker of `.` always exists, so the search stops at the working directory. Here such an agent contributes no marker and the search walks to the repository root |
| A skill at the root of the tree with no `name` field | Go names it after whatever string the caller passed as the root, which `include_dir!` cannot supply. It is an error that says so |
| `safe_join` refuses `a/../b` | Go's `path.Clean` accepts it as `b`. Only a caller writing its own file list can produce one, and a `..` inside an embedded path is never meant |
| The installed skill directory is 0755 | Go's `os.MkdirTemp` makes it 0700, which its own notes list as an accepted wart. A project checkout that another account cannot read is worse than the wart |

### Visibility

`File::executable` was a public setter for a field nothing on the embedded side
reads, and its own doc comment said so. It is gone. `INJECTED_KEYS`,
`manifest::strip` and `paths::clean` are read in one file each and are private.
`State::as_str` and `Action::as_str` duplicated what `Display` answers and had
no caller. `Scope::as_str` stays, because the clap adapter needs a
`&'static str`.

`#[non_exhaustive]` is on `Error` alone. On `InstallTarget`, `InstallStatus` and
`InstallResult` it forbade construction, and those three are plain data with
every field public. A front end that wants to hand `render_results` a row it
built, to test its own output, could not. Nothing outside the crate matches
them exhaustively, so the attribute protected nobody.

## Things that look wrong but are not

**`Error::Help` is a variant of the error type.** Help was asked for and
printed, which is not a failure. It is an error so that `run` has one return
type, and every front end maps it to a successful exit.

**`install` and `uninstall` return a tuple rather than a `Result`.** The
results describe everything that happened before the error. A `Result` would
make a caller choose between them.

**The `unsafe` in `tests/scope.rs`.** Setting an environment variable is
`unsafe` in edition 2024, because another thread may be reading it. Every test
in that file takes one lock first, and no other test binary shares the process.
It is the only file that touches the environment or the working directory, and
the workspace lint is `deny` rather than `forbid` so that it can say so.

**The workspace depends on `skill-embed` with `default-features = false`.** A
member that wants `include_dir` asks for it. Without this, the clap adapter
would pull in a proc macro it never uses.

**The README's examples carry their whole setup.** rustdoc hides a doctest line
that starts with `# `, and the source's own doc comments use that. `README.md`
must not: GitHub and crates.io render it as written, so a hidden line shows up
as a literal `#`, which is not Rust comment syntax at all. Each block there is
a function a reader could paste, and the parameter is where the installer comes
from.

**`doctests` is a crate with no code in it.** It exists so that every Rust
block in `README.md` is compiled. A hand written example drifts when a
signature changes, and nothing else would say so. `doctests/skills` exists
because the quick start calls `include_dir!`, which needs a directory to read.

**`test_all.sh` lints against another target.** A `cfg` block is always
compiled on the platform a contributor is on, so a lint that fires only
elsewhere passes locally and fails in CI. An unused import behind `cfg(unix)`
went out that way. Measured: `cargo clippy` on macOS is clean over it, and
`cargo clippy --target x86_64-pc-windows-msvc` names it. The step skips when
that target is not installed, because the three-platform matrix covers it
either way.

**Help text is asserted with `expect-test`.** A flag or a default changing
moves several blocks at once. Editing them by hand invites a typo that reads as
a real difference. `UPDATE_EXPECT=1 cargo test` rewrites them.

**Resolving project scope against the working directory.** A linter is run from
a subdirectory as often as from the root, and a plain working directory put the
skills wherever that was. The search walks up to the repository root instead.

**Allowing the search to land on the home directory.** The user scope
directories are there, so a project installation would sit in front of every
other project. `~/.claude/skills` in particular is exactly where `--scope user`
writes. Landing there is refused rather than allowed, because the alternative
is a user-wide install that nothing announced.

**Leaving the resolved project root to be inferred from a leaf path.** The
search depends on where the command was run. A run from a subdirectory and a
run from the repository root can disagree, and each row's destination is the
only sign of it. Every project scope run names the root instead.

**Restricting the project root markers to the selected agents.** It makes
`--agent claude-code` and `--agent all` resolve `.claude/skills` to different
directories in one repository. The markers come from every agent the installer
offers.

**The executable bit check answers yes on a platform that has no such bit.**
Windows reports no mode, so the rule and the file could never agree, and every
shebang script would read as `outdated` for good. Go has this bug. Here the
check is skipped and the digest decides alone.

**`render_status` and `render_results` are free functions.** They are the whole
of what a subcommand prints, not a part of it, so that every front end that
calls them says the same thing. The skills come from the statuses rather than
from the set, so a run narrowed by name describes only what it was asked about.

**`Columns` counts characters rather than terminal cells.** A CJK description
would otherwise throw the column off by the difference between bytes and
characters. Terminal width is a question nothing in the standard library
answers, and getting it half right is worse than being consistent.

## Known and left alone

An adversarial review of the Go original raised these. They are recorded so the
next reader does not raise them again.

The threat model is the reason. Whoever can write to the skills directory is
almost always the person running the tool, and what breaks is their own
`~/.claude/skills`. Each of these needs the user to work against themselves,
and none of them corrupts anything they did not touch.

"Almost always" is the project root. Its contents come from whoever the
repository was cloned from. A symbolic link committed at `.claude/skills` or
above it aims a project install, and a later `--force` removal, anywhere on the
disk. That one is not on the list below: `projectroot::within` refuses it.

| | |
| --- | --- |
| Concurrent installs into one directory | Some raced runs fail on the rename. No corruption, and the last one wins |
| A killed run leaves `.<name>.tmp-*` or `.<name>.old-*` behind | Nothing picks either up. The leading dot keeps them out of the agents' way. A failed restore leaves one on purpose, and names it |
| A container whose `HOME` is the repository root cannot install at project scope | The refusal cannot tell that shape from a real home directory. Weighed and kept: the refusal fails loudly and names the way out, and allowing it writes a project install into the user scope directory in silence |
| An empty directory inside an embedded skill is not created | A skill is a list of files. An empty directory carries nothing an agent reads, and the digest never saw it either |
| `AgentSelector::Named` can hold a comma | `parse_list` splits on commas, so such a name never round trips. It needs a custom agent through `with_agents` |
| A `SKILL.md` that is a fifo or `/dev/zero` | It is read without a size or type guard, so it hangs or grows without bound |
| Extra `x-embedded-at` lines carry arbitrary text | `strip` drops every injected key before hashing, so the digest cannot see them |
| A file starting with `#![no_std]` becomes executable | The shebang test cannot tell it from a script. `with_executable` is the way out |
| A user's own `chmod +x` is reverted | The state is `outdated` either way, and install repairs it without asking |
| Names differing only by Unicode normalization are not caught | `str::to_lowercase` is not the file system's equivalence relation. It catches the ASCII case, which is the one that happens |
| `--force` with `--dir` can remove an unrelated directory | It needs a skill whose name collides with something in that directory |
| `InstallOptions::names` is not deduplicated | Naming a skill twice writes it twice |
| A BOM moves into the body | Only when `with` creates a frontmatter block that was not there |
| `quote` and `unquote` are asymmetric | A tool name holding a quote or a backslash never reads back, so the skill stays `foreign` |

## What the port changed

These are the places where this library is deliberately not a transcription of
go-skill-embed. Nothing here changes what ends up on disk.

| | |
| --- | --- |
| `//go:embed` has two forms, one of which silently drops dotfiles | `include_dir!` has one. The README section explaining the choice is gone |
| Four front ends print `-agent` or `--agent`, depending on the framework | Both front ends here print `--agent` |
| Three of the four front ends read a flag after a positional as a name | Neither front end here does |
| `context.Context` on every call | `InstallOptions::cancel`, checked between skills |
| `scripts/checkdocs.py` compiles the documents' Go blocks | The `doctests` crate does it with rustdoc |
| `scripts/regolden.py` rewrites the `// Output:` comments | `UPDATE_EXPECT=1 cargo test` |
| declscope enforces declaration scopes | Rust has module privacy |
| `os.Exit` inside `Intercept` | `Option<ExitCode>` |
| A cobra, a urfave/cli v2 and a urfave/cli v3 adapter | One clap adapter. Nothing else has that share of the Rust ecosystem |
