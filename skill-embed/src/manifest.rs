//! Reads and rewrites the frontmatter of a `SKILL.md`.
//!
//! It deliberately does not parse YAML. The installer needs a handful of top
//! level scalars, and it has to rewrite its own keys without disturbing
//! anything else. A byte level edit preserves the rest of the file exactly.

use std::borrow::Cow;
use std::collections::BTreeMap;

/// The manifest every skill directory must contain, as defined by the
/// [Agent Skills specification](https://agentskills.io/specification).
pub(crate) const FILE_NAME: &str = "SKILL.md";

/// Records the tool that installed the skill.
pub(crate) const KEY_EMBEDDED_BY: &str = "x-embedded-by";
/// Records the version of that tool.
pub(crate) const KEY_EMBEDDED_VERSION: &str = "x-embedded-version";
/// Records when the skill was installed.
pub(crate) const KEY_EMBEDDED_AT: &str = "x-embedded-at";
/// Records the digest of what was installed.
pub(crate) const KEY_EMBEDDED_DIGEST: &str = "x-embedded-digest";

/// The keys [`with`] writes and [`strip`] removes.
///
/// They are namespaced so they never collide with the source tracking keys
/// `gh skill install` writes.
pub(crate) const INJECTED_KEYS: [&str; 4] =
    [KEY_EMBEDDED_BY, KEY_EMBEDDED_VERSION, KEY_EMBEDDED_AT, KEY_EMBEDDED_DIGEST];

const DELIM: &[u8] = b"---";
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const SPACE: &[u8] = b" \t\r";

/// One key and value to inject.
pub(crate) struct Entry {
    pub(crate) key: &'static str,
    pub(crate) value: String,
}

/// The YAML frontmatter at the top of a manifest.
struct Block {
    /// The offset of the opening delimiter line.
    open: usize,
    /// Bounds the YAML, excluding the delimiter lines.
    start: usize,
    end: usize,
    /// The offset just past the closing delimiter line.
    after: usize,
}

fn locate(src: &[u8]) -> Option<Block> {
    let rest = src.strip_prefix(BOM).unwrap_or(src); // tolerate a BOM
    let offset = src.len() - rest.len();

    let (line, next) = read_line(rest, 0);
    if !is_delim(line) {
        return None;
    }
    let start = next;
    let mut pos = next;
    while pos < rest.len() {
        let (line, closed) = read_line(rest, pos);
        if is_delim(line) {
            return Some(Block {
                open: offset,
                start: offset + start,
                end: offset + pos,
                after: offset + closed,
            });
        }
        pos = closed;
    }
    None
}

fn is_delim(line: &[u8]) -> bool {
    trim_end(line, SPACE) == DELIM
}

/// Returns the line starting at `pos` without its newline, and the offset of
/// the line after it.
fn read_line(src: &[u8], pos: usize) -> (&[u8], usize) {
    match src[pos..].iter().position(|&b| b == b'\n') {
        Some(i) => (&src[pos..pos + i], pos + i + 1),
        None => (&src[pos..], src.len()),
    }
}

fn trim_end<'a>(mut b: &'a [u8], set: &[u8]) -> &'a [u8] {
    while let Some((&last, head)) = b.split_last() {
        if !set.contains(&last) {
            break;
        }
        b = head;
    }
    b
}

/// Reads the top level `key: value` scalars. Nested mappings, sequences and
/// block scalars are skipped rather than misread.
///
/// A line that is not UTF-8 is skipped with them. Nothing this reads for is
/// spelled in anything else, and the bytes it cannot name it also must not
/// rewrite.
pub(crate) fn fields(src: &[u8]) -> BTreeMap<String, String> {
    let Some(b) = locate(src) else {
        return BTreeMap::new();
    };
    let body = &src[b.start..b.end];
    let mut fields = BTreeMap::new();
    let mut pos = 0;
    while pos < body.len() {
        let (line, next) = read_line(body, pos);
        pos = next;
        let Ok(text) = std::str::from_utf8(trim_end(line, SPACE)) else {
            continue;
        };
        if text.is_empty() || text.starts_with(['#', ' ', '\t', '-']) {
            continue; // a comment, or nested content
        }
        let Some((key, value)) = text.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || value.is_empty() {
            continue;
        }
        fields.insert(key.to_owned(), unquote(value).to_owned());
    }
    fields
}

fn unquote(s: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|s| s.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// Wraps a value in double quotes when plain style would be ambiguous.
fn quote(s: &str) -> Cow<'_, str> {
    const AMBIGUOUS: [char; 21] = [
        ':', '#', '\n', '"', '\'', '{', '}', '[', ']', ',', '&', '*', '?', '|', '<', '>', '=', '!',
        '%', '@', '`',
    ];
    if s.is_empty() {
        return Cow::Borrowed(r#""""#);
    }
    if !s.contains(AMBIGUOUS) && s.trim() == s {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str(r#"\""#),
            '\n' => out.push_str(r"\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    Cow::Owned(out)
}

/// Returns `src` with the injected keys replaced by `entries`. A manifest
/// without frontmatter gains one.
pub(crate) fn with(src: &[u8], entries: &[Entry]) -> Vec<u8> {
    let src = strip(src);
    let src = src.as_ref();

    let mut added = Vec::new();
    for e in entries {
        added.extend_from_slice(e.key.as_bytes());
        added.extend_from_slice(b": ");
        added.extend_from_slice(quote(&e.value).as_bytes());
        added.push(b'\n');
    }

    let Some(b) = locate(src) else {
        // No blank line after the closing delimiter. `strip` has to restore
        // the file byte for byte, and a line it did not write is a line it
        // cannot know to remove.
        let mut out = Vec::with_capacity(src.len() + added.len() + 8);
        out.extend_from_slice(b"---\n");
        out.extend_from_slice(&added);
        out.extend_from_slice(b"---\n");
        out.extend_from_slice(src);
        return out;
    };

    let mut out = Vec::with_capacity(src.len() + added.len() + 1);
    out.extend_from_slice(&src[..b.end]);
    if b.end > b.start && src[b.end - 1] != b'\n' {
        out.push(b'\n');
    }
    out.extend_from_slice(&added);
    out.extend_from_slice(&src[b.end..]);
    out
}

/// Returns the bytes two manifests must agree on to be the same skill: the
/// injected keys removed, and a frontmatter block that holds nothing else
/// removed with them.
///
/// That second step is why it is not just [`strip`]. [`with`] writes a block
/// when the source had none, and `strip` alone cannot tell that block from one
/// the source already had. Removing an empty block on both sides makes the two
/// spellings of "no frontmatter" hash alike. A skill whose manifest carries an
/// empty block therefore reads as up-to-date once installed.
pub(crate) fn normalize(src: &[u8]) -> Cow<'_, [u8]> {
    let stripped = strip(src);
    let Some(b) = locate(&stripped) else {
        return stripped;
    };
    if !stripped[b.start..b.end].iter().all(u8::is_ascii_whitespace) {
        return stripped;
    }
    let mut out = Vec::with_capacity(stripped.len());
    out.extend_from_slice(&stripped[..b.open]);
    out.extend_from_slice(&stripped[b.after..]);
    Cow::Owned(out)
}

/// Removes the injected keys and nothing else. [`with`] uses it so that
/// stamping twice does not accumulate duplicates.
pub(crate) fn strip(src: &[u8]) -> Cow<'_, [u8]> {
    let Some(b) = locate(src) else {
        return Cow::Borrowed(src);
    };
    let body = &src[b.start..b.end];
    let mut kept = Vec::new();
    let mut dropped = false;
    let mut pos = 0;
    while pos < body.len() {
        let (line, next) = read_line(body, pos);
        let (start, end) = (pos, next);
        pos = next;
        if is_injected(line) {
            dropped = true;
        } else {
            kept.extend_from_slice(&body[start..end]);
        }
    }
    if !dropped {
        return Cow::Borrowed(src);
    }
    let mut out = Vec::with_capacity(src.len());
    out.extend_from_slice(&src[..b.start]);
    out.extend_from_slice(&kept);
    out.extend_from_slice(&src[b.end..]);
    Cow::Owned(out)
}

fn is_injected(line: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(trim_end(line, SPACE)) else {
        return false;
    };
    // An indented line belongs to whatever is above it. That may be a block
    // scalar holding a line that looks exactly like one of these keys.
    // `fields` skips those, and this has to skip them for the same reason.
    if text.starts_with([' ', '\t']) {
        return false;
    }
    text.split_once(':').is_some_and(|(key, _)| INJECTED_KEYS.contains(&key.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_FRONTMATTER: &str = "\
---
name: demo-skill
description: \"A demo, with a colon: right here\"
---

# Body
";

    fn entry(key: &'static str, value: &str) -> Entry {
        Entry { key, value: value.to_owned() }
    }

    fn text(b: &[u8]) -> &str {
        std::str::from_utf8(b).expect("test data is UTF-8")
    }

    #[test]
    fn fields_reads_top_level_scalars() {
        let got = fields(WITH_FRONTMATTER.as_bytes());
        assert_eq!(got["name"], "demo-skill");
        assert_eq!(got["description"], "A demo, with a colon: right here");
    }

    #[test]
    fn fields_without_frontmatter_is_empty() {
        assert!(fields(b"# Just a body\n").is_empty());
    }

    #[test]
    fn fields_skips_nested_content() {
        let got = fields(b"---\nname: demo\nallowed-tools:\n  - Read\n  - Bash\n---\n");
        assert!(!got.contains_key("- Read"), "sequence item read as a field: {got:?}");
        assert_eq!(got["name"], "demo");
    }

    #[test]
    fn with_then_strip_round_trips() {
        let entries = [entry(KEY_EMBEDDED_BY, "mytool"), entry(KEY_EMBEDDED_DIGEST, "sha256:abc")];

        let stamped = with(WITH_FRONTMATTER.as_bytes(), &entries);
        assert!(text(&stamped).contains("x-embedded-by: mytool"), "{}", text(&stamped));
        assert_eq!(fields(&stamped)["name"], "demo-skill", "existing field lost");

        // Stamping twice must not accumulate duplicates.
        let twice = with(&stamped, &entries);
        assert_eq!(text(&twice).matches("x-embedded-by:").count(), 1, "{}", text(&twice));

        assert_eq!(text(&strip(&twice)), WITH_FRONTMATTER);
    }

    #[test]
    fn with_creates_frontmatter() {
        let stamped = with(b"# Body only\n", &[entry(KEY_EMBEDDED_BY, "mytool")]);
        assert!(text(&stamped).starts_with("---\n"), "{}", text(&stamped));
        assert_eq!(fields(&stamped)[KEY_EMBEDDED_BY], "mytool");
        assert!(text(&stamped).contains("# Body only"), "body lost");
    }

    /// A manifest with no frontmatter gains one on install, and a manifest with
    /// an empty one keeps it. `normalize` has to make the two hash alike, or a
    /// skill reads as modified the moment it is installed.
    #[test]
    fn normalize_makes_the_two_empty_forms_agree() {
        let entries = [entry(KEY_EMBEDDED_BY, "mytool"), entry(KEY_EMBEDDED_DIGEST, "sha256:abc")];

        for src in [
            "A manifest that carries no frontmatter at all.\n",
            "---\n---\nA manifest whose frontmatter block is empty.\n",
            "---\n\n  \n---\nA manifest whose frontmatter block is blank.\n",
        ] {
            let stamped = with(src.as_bytes(), &entries);
            assert_eq!(fields(&stamped)[KEY_EMBEDDED_BY], "mytool");
            assert_eq!(
                normalize(&stamped),
                normalize(src.as_bytes()),
                "the installed copy does not normalize to its source: {src:?}"
            );
        }
    }

    /// A block scalar may hold a line that looks exactly like an injected key.
    /// Removing it would change the file the author wrote.
    #[test]
    fn strip_leaves_nested_content_alone() {
        let src = format!(
            "---\nname: demo\nexample: |\n  {KEY_EMBEDDED_DIGEST}: \"sha256:x\"\n  second line\n---\nbody\n"
        );

        let stamped = with(src.as_bytes(), &[entry(KEY_EMBEDDED_DIGEST, "sha256:real")]);
        assert!(
            text(&stamped).contains(&format!("  {KEY_EMBEDDED_DIGEST}: \"sha256:x\"")),
            "the block scalar lost a line:\n{}",
            text(&stamped)
        );
        assert_eq!(text(&strip(&stamped)), src);
        // The real key is still the one that reads back.
        assert_eq!(fields(&stamped)[KEY_EMBEDDED_DIGEST], "sha256:real");
    }

    #[test]
    fn quote_wraps_only_what_needs_it() {
        assert_eq!(quote("mytool"), "mytool");
        assert_eq!(quote(""), r#""""#);
        assert_eq!(quote("sha256:abc"), r#""sha256:abc""#);
        assert_eq!(quote(" padded "), r#"" padded ""#);
    }
}
