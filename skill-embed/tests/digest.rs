//! The digest, against the value another implementation of it produces.
//!
//! go-skill-embed hashes the same skill through its own directory walk, and an
//! installation made by either library has to read as `up-to-date` to the
//! other. Nothing but a fixed value can hold the two together from this side.

mod common;

use common::TempDir;
use skill_embed::SkillSet;

/// Written by
/// `go run . /tmp/skills` against go-skill-embed's `SkillsFromFS`, over the
/// tree this test builds. The names interleave on purpose: `b` is a directory
/// and `b.md` is a file, and a directory walk reaches them in an order that
/// sorting the paths as strings does not produce.
const GO_DIGEST: &str = "sha256:b899d17b90272404a4abed72c9e93779d2902f4912a2bbc06562df023bb2638e";

#[test]
fn the_digest_matches_go_skill_embed() {
    let tmp = TempDir::new("digest");
    tmp.write(
        "probe-skill/SKILL.md",
        "---\nname: probe-skill\ndescription: A skill used to compare two implementations of the digest.\n---\n\n# Probe\n",
    );
    tmp.write("probe-skill/a.md", "a\n");
    tmp.write("probe-skill/ab.md", "ab\n");
    tmp.write("probe-skill/b.md", "top level b\n");
    tmp.write("probe-skill/b/c.md", "inside b\n");

    let set = SkillSet::read_dir(tmp.path()).expect("the fixture is a valid skill set");
    assert_eq!(set.skills()[0].digest(), GO_DIGEST);
}
