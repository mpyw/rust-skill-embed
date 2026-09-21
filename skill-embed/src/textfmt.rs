//! Shapes strings for a terminal listing.
//!
//! Nothing here knows anything about skills. It is one module so that the
//! files rendering the command's output do not each carry a copy.

use std::fmt::Write as _;

/// How much of a description a listing shows, counted in characters.
const WIDTH: usize = 100;

/// Shortens `s` to one line of at most [`WIDTH`] characters.
///
/// The count is in characters and the cut falls on a character boundary. Bytes
/// would cut a CJK description at a third of the length, and in the middle of a
/// character. A tab becomes a space, because the caller is laying out columns.
pub(crate) fn first_line(s: &str) -> String {
    let line = s.split(['\n', '\r']).next().unwrap_or("").replace('\t', " ");
    if line.chars().count() <= WIDTH {
        return line;
    }
    line.chars().take(WIDTH - 3).collect::<String>() + "..."
}

/// Removes the padding column layout leaves at the end of a line when the last
/// column is empty.
///
/// Nothing should print trailing whitespace.
pub(crate) fn trim_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end_matches([' ', '\t']));
    }
    out
}

/// Rows of cells rendered as columns, each one as wide as its widest cell plus
/// a two space gap.
///
/// The width is counted in characters, so a CJK description does not throw the
/// column off by the difference between bytes and characters. It is not
/// counted in terminal cells, which no answer in the standard library knows.
#[derive(Default)]
pub(crate) struct Columns {
    rows: Vec<Vec<String>>,
}

impl Columns {
    pub(crate) fn push(&mut self, row: impl IntoIterator<Item = String>) {
        self.rows.push(row.into_iter().collect());
    }

    pub(crate) fn render(&self) -> String {
        let columns = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..columns)
            .map(|i| {
                self.rows
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.chars().count())
                    .max()
                    .unwrap_or(0)
            })
            .collect();

        let mut out = String::new();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i + 1 == row.len() {
                    out.push_str(cell);
                } else {
                    let pad = widths[i] - cell.chars().count() + 2;
                    let _ = write!(out, "{cell}{:pad$}", "");
                }
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_counts_characters() {
        let cjk = "あ".repeat(120);
        assert_eq!(first_line(&cjk).chars().count(), WIDTH);
        assert_eq!(first_line("one\ntwo"), "one");
        assert_eq!(first_line("a\tb"), "a b");
    }

    #[test]
    fn columns_align_on_the_widest_cell() {
        let mut c = Columns::default();
        c.push(["skill".to_owned(), "missing".to_owned()]);
        c.push(["a-much-longer-name".to_owned(), "up-to-date".to_owned()]);
        assert_eq!(c.render(), "skill               missing\na-much-longer-name  up-to-date\n");
    }

    #[test]
    fn trim_lines_removes_padding() {
        assert_eq!(trim_lines("a  \nb\t\n"), "a\nb\n");
    }
}
