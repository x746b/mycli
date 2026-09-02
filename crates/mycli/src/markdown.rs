//! Markdown rendering for streamed assistant text.
//!
//! The renderer receives text a token at a time and used to hand each finished
//! *line* to termimad on its own. That works for prose but destroys anything
//! whose layout depends on lines that have not arrived yet: a table's column
//! widths come from every row, so rendering rows one at a time produced
//! unaligned cells and a header outside the box, and a code fence rendered a
//! line at a time never sees its own delimiters.
//!
//! So text is buffered and only the prefix that can be rendered *now* without
//! splitting such a construct is flushed. Prose still streams line by line;
//! tables and fenced blocks are held until complete.

use termimad::crossterm::style::Color;
use termimad::{MadSkin, StyledChar, ROUNDED_TABLE_BORDER_CHARS};

/// The skin used for all assistant output.
pub fn skin() -> MadSkin {
    let mut skin = MadSkin::default();

    skin.set_headers_fg(Color::Cyan);
    skin.inline_code.set_fg(Color::Cyan);
    skin.code_block.set_fg(Color::Cyan);

    // Tables: rounded borders in grey, so the data reads louder than the box.
    skin.table_border_chars = ROUNDED_TABLE_BORDER_CHARS;
    skin.table.set_fg(Color::DarkGrey);

    skin.bullet = StyledChar::from_fg_char(Color::Cyan, '•');
    skin.quote_mark = StyledChar::from_fg_char(Color::DarkGrey, '│');
    // The default rule is a heavy full-width run of `―`, which dominates the
    // transcript. A thin grey line separates just as well.
    skin.horizontal_rule = StyledChar::from_fg_char(Color::DarkGrey, '─');

    skin
}

/// Render `text` to a string at `width` columns.
///
/// Tables are drawn here rather than by termimad, which only frames a table
/// when the source is written its own way (a rule line above the header as
/// well as below) and never insets cell contents, so a GitHub-flavoured table
/// comes out as bare pipes around tight columns. Everything else goes to
/// termimad unchanged.
pub fn render(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut prose: Vec<&str> = Vec::new();
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut i = 0;

    while i < lines.len() {
        match table_run_len(&lines[i..]) {
            Some(len) => {
                if !prose.is_empty() {
                    out.push_str(&skin().text(&prose.concat(), Some(width)).to_string());
                    prose.clear();
                }
                out.push_str(&render_table(&lines[i..i + len], width));
                i += len;
            }
            None => {
                prose.push(lines[i]);
                i += 1;
            }
        }
    }
    if !prose.is_empty() {
        out.push_str(&skin().text(&prose.concat(), Some(width)).to_string());
    }
    out
}

// ─── Tables ─────────────────────────────────────────────────────────────────

/// Length of the table starting at `lines[0]`, or `None` if there isn't one.
///
/// A table is a header row, an alignment rule, and at least zero body rows.
/// Without the rule it is just text that happens to contain pipes.
fn table_run_len(lines: &[&str]) -> Option<usize> {
    if lines.len() < 2 || !is_table_line(lines[0]) || !is_separator_row(lines[1]) {
        return None;
    }
    let mut len = 2;
    while len < lines.len() && is_table_line(lines[len]) {
        len += 1;
    }
    Some(len)
}

fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator_row(line: &str) -> bool {
    if !is_table_line(line) {
        return false;
    }
    let cells = split_cells(line);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim();
            let body = c.trim_start_matches(':').trim_end_matches(':');
            !body.is_empty() && body.chars().all(|ch| ch == '-')
        })
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Align {
    Left,
    Center,
    Right,
}

fn alignments(sep: &str) -> Vec<Align> {
    split_cells(sep)
        .iter()
        .map(|c| {
            let c = c.trim();
            match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => Align::Center,
                (false, true) => Align::Right,
                _ => Align::Left,
            }
        })
        .collect()
}

fn render_table(lines: &[&str], width: usize) -> String {
    use crate::ui::{display_width, truncate, BOLD, DIM, RESET};

    let skin = skin();
    let aligns = alignments(lines[1]);
    let ncols = aligns.len().max(split_cells(lines[0]).len()).max(1);

    // Row 1 is the alignment rule, which carries no data.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 1 {
            continue;
        }
        let mut cells = split_cells(line);
        cells.resize(ncols, String::new());
        rows.push(cells);
    }
    if rows.is_empty() {
        return String::new();
    }

    let mut widths: Vec<usize> = (0..ncols)
        .map(|c| {
            rows.iter()
                .map(|r| display_width(&r[c]))
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();

    // Each column occupies `│ cell `, so it costs its width plus 3, and the
    // row closes with one more `│`. Shrink the widest column until it fits.
    let total = |ws: &[usize]| 1 + ws.iter().map(|w| w + 3).sum::<usize>();
    while total(&widths) > width {
        let Some(widest) = (0..ncols).max_by_key(|&c| widths[c]) else {
            break;
        };
        if widths[widest] <= 3 {
            break;
        }
        widths[widest] -= 1;
    }

    let rule = |left: char, mid: char, right: char| {
        let mut s = String::from(DIM);
        s.push(left);
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                s.push(mid);
            }
            s.push_str(&"─".repeat(w + 2));
        }
        s.push(right);
        s.push_str(RESET);
        s.push('\n');
        s
    };

    let mut out = rule('╭', '┬', '╮');
    for (i, row) in rows.iter().enumerate() {
        let mut line = String::new();
        for (c, cell) in row.iter().enumerate().take(ncols) {
            line.push_str(&format!("{DIM}│{RESET} "));
            // Truncate the source, then style: `truncate` measures display
            // columns and would count the bytes of an escape sequence.
            let plain = truncate(cell, widths[c]);
            let styled = skin.inline(&plain).to_string();
            let slack = widths[c].saturating_sub(display_width(&styled));
            let (before, after) = match aligns.get(c).copied().unwrap_or(Align::Left) {
                Align::Left => (0, slack),
                Align::Right => (slack, 0),
                Align::Center => (slack / 2, slack - slack / 2),
            };
            line.push_str(&" ".repeat(before));
            if i == 0 {
                line.push_str(&format!("{BOLD}{styled}{RESET}"));
            } else {
                line.push_str(&styled);
            }
            line.push_str(&" ".repeat(after));
            line.push(' ');
        }
        line.push_str(&format!("{DIM}│{RESET}\n"));
        out.push_str(&line);
        if i == 0 {
            out.push_str(&rule('├', '┼', '┤'));
        }
    }
    out.push_str(&rule('╰', '┴', '╯'));
    out
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// A table row, or the `|---|---|` separator under a header.
fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// Length of the prefix of `buf` that is safe to render now.
///
/// Returns 0 when nothing can be flushed yet. The held-back remainder is
/// always a whole number of lines plus, possibly, one partial line.
pub fn safe_prefix_len(buf: &str) -> usize {
    // Never render a partial line: its own markdown may still be incomplete.
    let complete_end = match buf.rfind('\n') {
        Some(i) => i + 1,
        None => return 0,
    };
    let complete = &buf[..complete_end];

    // An unterminated code fence: hold everything from the opening delimiter.
    let mut in_fence = false;
    let mut fence_start = 0usize;
    let mut offset = 0usize;
    let mut line_offsets: Vec<usize> = Vec::new();
    for line in complete.split_inclusive('\n') {
        line_offsets.push(offset);
        if is_fence(line) {
            if in_fence {
                in_fence = false;
            } else {
                in_fence = true;
                fence_start = offset;
            }
        }
        offset += line.len();
    }
    if in_fence {
        return fence_start;
    }

    // A table running to the end of the buffer may still be growing; hold it
    // back so termimad sizes the columns against every row at once.
    let mut cut = complete_end;
    for &start in line_offsets.iter().rev() {
        let line = complete[start..].split('\n').next().unwrap_or("");
        if is_table_line(line) {
            cut = start;
        } else {
            break;
        }
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_back_a_partial_line() {
        assert_eq!(safe_prefix_len("hello wor"), 0);
        assert_eq!(safe_prefix_len("hello\nwor"), 6);
    }

    #[test]
    fn holds_back_an_open_code_fence() {
        let s = "intro\n\n```rust\nfn main() {\n";
        assert_eq!(&s[..safe_prefix_len(s)], "intro\n\n");
    }

    #[test]
    fn releases_a_closed_code_fence() {
        let s = "intro\n\n```rust\nfn main() {}\n```\n";
        assert_eq!(safe_prefix_len(s), s.len());
    }

    /// The bug this module exists for: a table streamed row by row is rendered
    /// as several one-row tables with mismatched column widths.
    #[test]
    fn holds_back_a_growing_table() {
        let s = "Result:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        assert_eq!(&s[..safe_prefix_len(s)], "Result:\n\n");
    }

    #[test]
    fn releases_a_table_once_a_non_table_line_follows() {
        let s = "| a | b |\n|---|---|\n| 1 | 2 |\n\n";
        assert_eq!(safe_prefix_len(s), s.len());
    }

    fn table_lines(md: &str, width: usize) -> Vec<String> {
        render(md, width)
            .lines()
            .map(|l| crate::ui::strip_ansi(l))
            .filter(|l| !l.trim().is_empty())
            .collect()
    }

    #[test]
    fn table_is_framed_and_columns_line_up() {
        let md = "| Step | a | b |\n|---|---|---|\n| 1 | 1914 | 899 |\n| 2 | 899 | 116 |\n";
        let lines = table_lines(md, 80);
        assert!(lines[0].starts_with('╭') && lines[0].ends_with('╮'), "{lines:?}");
        assert!(lines[2].starts_with('├'), "{lines:?}");
        assert!(lines.last().unwrap().starts_with('╰'), "{lines:?}");
        // Every row is exactly as wide as the frame.
        let w = crate::ui::display_width(&lines[0]);
        for l in &lines {
            assert_eq!(crate::ui::display_width(l), w, "{l:?} in {lines:?}");
        }
        // Cells are inset by a space on each side.
        assert!(lines[1].starts_with("│ Step "), "{:?}", lines[1]);
    }

    #[test]
    fn table_honours_column_alignment() {
        let md = "| l | c | r |\n|:---|:---:|---:|\n| x | x | x |\n";
        let lines = table_lines(md, 80);
        assert_eq!(lines[3], "│ x │ x │ x │", "{lines:?}");
    }

    #[test]
    fn table_shrinks_to_fit_a_narrow_terminal() {
        let md = "| a | b |\n|---|---|\n| this cell is very long indeed | short |\n";
        let lines = table_lines(md, 30);
        for l in &lines {
            assert!(crate::ui::display_width(l) <= 30, "{l:?}");
        }
    }

    /// Pipes in prose are not a table without an alignment rule under them.
    #[test]
    fn pipes_without_a_rule_are_not_a_table() {
        assert_eq!(table_run_len(&["| a | b |\n", "not a rule\n"]), None);
        assert_eq!(table_run_len(&["| a | b |\n", "|---|---|\n"]), Some(2));
    }

    #[test]
    fn prose_streams_line_by_line() {
        let s = "one\ntwo\nthree";
        assert_eq!(&s[..safe_prefix_len(s)], "one\ntwo\n");
    }
}

