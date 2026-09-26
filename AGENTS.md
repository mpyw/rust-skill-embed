# Repository instructions

rust-skill-embed ports go-skill-embed to Rust. Preserve behavioral parity where the port intends it, including manifest byte handling and skill installation semantics. The rationale, rejected approaches, platform cases, and deliberate differences are in [implementation notes](design/implementation.md); read the relevant section before changing behavior.

Keep README and source comments focused on current behavior. Run `./test_all.sh` before claiming the full gate passes; it covers every crate. `.claude/skills` and `.agents/skills` are installation destinations the library manages, not this repository's own instruction files.
