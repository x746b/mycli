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
    // Maths first: a terminal cannot typeset LaTeX, and leaving it raw buries
    // the answer in backslashes. See `latex::render_math`.
    let text = crate::latex::render_math(text);

    // Repair one-line tables before anything else looks at the text.
    let normalized: Vec<String> = text
        .split_inclusive('\n')
        .flat_map(|line| split_inline_table(line).unwrap_or_else(|| vec![line.to_string()]))
        .collect();

    let mut out = String::new();
    let mut prose: Vec<&str> = Vec::new();
    let lines: Vec<&str> = normalized.iter().map(String::as_str).collect();
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

/// Longest word kept unbroken when a column has to shrink.
const MAX_UNBROKEN_WORD: usize = 30;

/// Does this line look like a table row? A row needs an unescaped `|`, but the
/// outer pipes are optional — `a | b` is as valid as `| a | b |`.
fn looks_like_row(line: &str) -> bool {
    !split_cells(line).is_empty() && line.contains('|')
}

/// Split a row into cells on unescaped pipes, dropping the optional outer ones.
fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    if t.is_empty() {
        return Vec::new();
    }
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for c in t.chars() {
        if escaped {
            // Keep the pipe, drop the backslash that protected it.
            if c != '|' {
                cells.last_mut().unwrap().push('\\');
            }
            cells.last_mut().unwrap().push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '|' {
            cells.push(String::new());
        } else {
            cells.last_mut().unwrap().push(c);
        }
    }
    // A leading or trailing pipe produces an empty edge cell; drop those.
    if t.starts_with('|') && !cells.is_empty() {
        cells.remove(0);
    }
    if t.ends_with('|') && !cells.is_empty() {
        cells.pop();
    }
    cells.iter().map(|c| c.trim().to_string()).collect()
}

/// Is this cell an alignment marker — `---`, `:--`, `:-:`?
fn is_alignment_cell(cell: &str) -> bool {
    let body = cell.trim().trim_start_matches(':').trim_end_matches(':');
    !body.is_empty() && body.chars().all(|ch| ch == '-')
}

/// Split a table that a model emitted entirely on one line.
///
/// Smaller models routinely drop the newlines and write
/// `| a | b ||---|---|| 1 | 2 |` as a single line. Left alone that reaches
/// termimad as one enormous row and comes back as a column per cell — the
/// worst-looking output the renderer can produce. The giveaway is a run of
/// alignment cells sitting inside the line: what precedes it is the header,
/// what follows is the body.
///
/// This is a salvage path, so it cannot be perfect: the `||` between rows
/// yields an empty cell, and telling those apart from a genuinely empty cell
/// is not possible after the newlines are gone. Empty cells are dropped.
fn split_inline_table(line: &str) -> Option<Vec<String>> {
    let cells = split_cells(line);
    let rule_start = cells.iter().position(|c| is_alignment_cell(c))?;
    let rule_end = cells[rule_start..]
        .iter()
        .position(|c| !is_alignment_cell(c))
        .map(|i| rule_start + i)
        .unwrap_or(cells.len());
    let ncols = rule_end - rule_start;
    if ncols < 2 {
        return None;
    }

    // The header is the `ncols` non-empty cells immediately before the rule.
    // Anything further left is a sentence that ran into the table.
    let mut header_idx: Vec<usize> = Vec::new();
    for i in (0..rule_start).rev() {
        if cells[i].is_empty() {
            continue;
        }
        header_idx.push(i);
        if header_idx.len() == ncols {
            break;
        }
    }
    if header_idx.len() != ncols {
        return None;
    }
    header_idx.reverse();

    let header: Vec<&str> = header_idx.iter().map(|&i| cells[i].as_str()).collect();
    let prefix: Vec<&str> = cells[..header_idx[0]]
        .iter()
        .filter(|c| !c.is_empty())
        .map(String::as_str)
        .collect();

    let body: Vec<&str> = cells[rule_end..]
        .iter()
        .filter(|c| !c.is_empty())
        .map(String::as_str)
        .collect();
    // One cell left over after whole rows is the sentence that followed the
    // table, not a row with a single column filled in.
    let (body, suffix) = if body.len() % ncols == 1 {
        (&body[..body.len() - 1], body.last().copied())
    } else {
        (&body[..], None)
    };

    let row = |cells: &[&str]| format!("| {} |\n", cells.join(" | "));
    let mut out = Vec::new();
    if !prefix.is_empty() {
        out.push(format!("{}\n\n", prefix.join(" | ")));
    }
    out.push(row(&header));
    out.push(row(&cells[rule_start..rule_end]
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()));
    for chunk in body.chunks(ncols) {
        let mut cells = chunk.to_vec();
        cells.resize(ncols, "");
        out.push(row(&cells));
    }
    if let Some(suffix) = suffix {
        out.push(format!("\n{suffix}\n"));
    }
    Some(out)
}

/// The `|---|:--:|` line under a header, which is what makes a run of pipes a
/// table rather than prose that happens to contain one.
fn is_separator_row(line: &str) -> bool {
    let cells = split_cells(line);
    if cells.is_empty() || !line.contains('|') {
        return false;
    }
    cells.iter().all(|c| is_alignment_cell(c))
}

/// Length of the table starting at `lines[0]`, or `None` if there isn't one.
fn table_run_len(lines: &[&str]) -> Option<usize> {
    if lines.len() < 2 || !looks_like_row(lines[0]) || !is_separator_row(lines[1]) {
        return None;
    }
    let mut len = 2;
    while len < lines.len() && looks_like_row(lines[len]) {
        len += 1;
    }
    Some(len)
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

/// Width of the longest single word, so a column is never squeezed narrower
/// than the text it must hold without hyphenation.
fn longest_word(text: &str) -> usize {
    text.split_whitespace()
        .map(crate::ui::display_width)
        .max()
        .unwrap_or(1)
        .clamp(1, MAX_UNBROKEN_WORD)
}

/// Column widths that fit `avail` columns of cell space.
///
/// When the natural widths fit, they are used as-is. When they do not, every
/// column shrinks toward its longest word in proportion to how much slack it
/// has, so one wide column does not starve the rest — truncating the widest
/// column instead loses exactly the content the table was written to show.
fn fit_columns(natural: &[usize], minimum: &[usize], avail: usize) -> Vec<usize> {
    let ncols = natural.len();
    if natural.iter().sum::<usize>() <= avail {
        return natural
            .iter()
            .zip(minimum)
            .map(|(n, m)| *n.max(m))
            .collect();
    }

    // Below the sum of minimums there is nothing left to protect: give every
    // column one column and share what remains by weight.
    let min_total: usize = minimum.iter().sum();
    if min_total > avail {
        let mut widths = vec![1usize; ncols];
        let remaining = avail.saturating_sub(ncols);
        let weight: usize = minimum.iter().map(|m| m.saturating_sub(1)).sum();
        if remaining > 0 && weight > 0 {
            for i in 0..ncols {
                widths[i] += minimum[i].saturating_sub(1) * remaining / weight;
            }
        }
        return widths;
    }

    let slack: usize = (0..ncols).map(|i| natural[i].saturating_sub(minimum[i])).sum();
    let extra = avail - min_total;
    let mut widths: Vec<usize> = (0..ncols)
        .map(|i| {
            let grow = if slack > 0 {
                natural[i].saturating_sub(minimum[i]) * extra / slack
            } else {
                0
            };
            minimum[i] + grow
        })
        .collect();

    // Integer division leaves a few columns unspent; hand them out.
    let mut leftover = avail.saturating_sub(widths.iter().sum::<usize>());
    while leftover > 0 {
        let mut grew = false;
        for i in 0..ncols {
            if leftover == 0 {
                break;
            }
            if widths[i] < natural[i] {
                widths[i] += 1;
                leftover -= 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    widths
}

fn render_table(lines: &[&str], width: usize) -> String {
    use crate::ui::{display_width, wrap, BOLD, DIM, RESET};

    let skin = skin();
    let aligns = alignments(lines[1]);
    let ncols = aligns.len().max(split_cells(lines[0]).len()).max(1);

    // Line 1 is the alignment rule, which carries no data.
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

    // Borders cost `│ ` before each column, ` ` after, and a closing `│`.
    let overhead = 3 * ncols + 1;
    let avail = width.saturating_sub(overhead).max(ncols);

    let natural: Vec<usize> = (0..ncols)
        .map(|c| rows.iter().map(|r| display_width(&r[c])).max().unwrap_or(1).max(1))
        .collect();
    let minimum: Vec<usize> = (0..ncols)
        .map(|c| rows.iter().map(|r| longest_word(&r[c])).max().unwrap_or(1))
        .collect();
    let widths = fit_columns(&natural, &minimum, avail);

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
    let separator = rule('├', '┼', '┤');

    for (r, row) in rows.iter().enumerate() {
        // A cell too wide for its column wraps rather than being truncated,
        // so the row grows downward instead of losing text.
        let wrapped: Vec<Vec<String>> = (0..ncols)
            .map(|c| {
                let lines = wrap(&row[c], widths[c]);
                if lines.is_empty() {
                    vec![String::new()]
                } else {
                    lines
                }
            })
            .collect();
        let height = wrapped.iter().map(|c| c.len()).max().unwrap_or(1);

        for line_idx in 0..height {
            let mut line = String::new();
            for c in 0..ncols {
                line.push_str(&format!("{DIM}│{RESET} "));
                let plain = wrapped[c].get(line_idx).cloned().unwrap_or_default();
                // Style after wrapping: `wrap` measures columns and would
                // count the bytes of an escape sequence.
                let styled = skin.inline(&plain).to_string();
                let slack = widths[c].saturating_sub(display_width(&styled));
                let (before, after) = match aligns.get(c).copied().unwrap_or(Align::Left) {
                    Align::Left => (0, slack),
                    Align::Right => (slack, 0),
                    Align::Center => (slack / 2, slack - slack / 2),
                };
                line.push_str(&" ".repeat(before));
                if r == 0 && !styled.is_empty() {
                    line.push_str(&format!("{BOLD}{styled}{RESET}"));
                } else {
                    line.push_str(&styled);
                }
                line.push_str(&" ".repeat(after));
                line.push(' ');
            }
            line.push_str(&format!("{DIM}│{RESET}\n"));
            out.push_str(&line);
        }

        if r + 1 < rows.len() {
            out.push_str(&separator);
        }
    }
    out.push_str(&rule('╰', '┴', '╯'));
    out
}

// ─── Streaming boundaries ───────────────────────────────────────────────────

/// Byte offset of a `$$` or `\[` that has no closing partner yet.
fn open_math_block(text: &str) -> Option<usize> {
    for (open, close) in [("$$", "$$"), (r"\[", r"\]")] {
        let mut search = 0usize;
        while let Some(rel) = text[search..].find(open) {
            let start = search + rel;
            let after = start + open.len();
            match text[after..].find(close) {
                Some(len) => search = after + len + close.len(),
                None => return Some(start),
            }
        }
    }
    None
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
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

    // An unterminated display-math block: hold it, so the delimiters are not
    // rendered as literal text before the closing pair arrives.
    if let Some(start) = open_math_block(complete) {
        return start;
    }

    // A table running to the end of the buffer may still be growing; hold it
    // back so the columns are sized against every row at once.
    let mut run_start = complete_end;
    let mut run: Vec<&str> = Vec::new();
    for &start in line_offsets.iter().rev() {
        let line = complete[start..].split('\n').next().unwrap_or("");
        if looks_like_row(line) {
            run_start = start;
            run.insert(0, line);
        } else {
            break;
        }
    }
    // One line could be a header whose rule has not arrived; two or more are
    // only held when the second really is a rule. Otherwise this is prose that
    // happens to contain a pipe, and holding it would stall the stream.
    let pending_table = match run.len() {
        0 => false,
        1 => true,
        _ => is_separator_row(run[1]),
    };
    if pending_table {
        run_start
    } else {
        complete_end
    }
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

    /// A cell too wide for its column wraps onto more lines instead of being
    /// truncated — the long column is usually the one worth reading.
    #[test]
    fn table_wraps_cells_instead_of_truncating() {
        let md = "| a | b |\n|---|---|\n| this cell is very long indeed | short |\n";
        let lines = table_lines(md, 30);
        for l in &lines {
            assert!(crate::ui::display_width(l) <= 30, "{l:?}");
        }
        let body: String = lines.join(" ");
        for word in ["this", "cell", "very", "long", "indeed"] {
            assert!(body.contains(word), "lost {word:?} in {lines:?}");
        }
        assert!(!body.contains('…'), "should wrap, not truncate: {lines:?}");
    }

    /// A table a model wrote without newlines is rebuilt rather than handed to
    /// termimad, which renders it as one column per cell.
    #[test]
    fn one_line_table_is_split_back_into_rows() {
        let md = "| Method | Idempotent ||--------|:----------:|| GET | Yes || POST | No |\n";
        let lines = table_lines(md, 60);
        assert!(lines[0].starts_with('╭'), "{lines:?}");
        assert!(lines[1].contains("Method") && lines[1].contains("Idempotent"), "{lines:?}");
        let body = lines.join(" ");
        for word in ["GET", "Yes", "POST", "No"] {
            assert!(body.contains(word), "lost {word:?} in {lines:?}");
        }
    }

    /// Models also run the surrounding prose into the same line. The sentence
    /// before the table and the one after it have to survive.
    #[test]
    fn one_line_table_keeps_the_prose_around_it() {
        let md = "A prime has two divisors.| n | prime ||---|-------|| 1 | No || 2 | Yes |In short: 2 is prime.\n";
        let out = crate::ui::strip_ansi(&render(md, 60));
        assert!(out.contains("A prime has two divisors."), "{out}");
        assert!(out.contains("In short: 2 is prime."), "{out}");
        let framed: Vec<&str> = out.lines().filter(|l| l.starts_with('╭') || l.starts_with('╰')).collect();
        assert_eq!(framed.len(), 2, "{out}");
    }

    #[test]
    fn prose_with_pipes_is_not_mistaken_for_a_one_line_table() {
        assert!(split_inline_table("use `a | b` or `c | d` here").is_none());
        assert!(split_inline_table("a --- b | c").is_none());
    }

    #[test]
    fn table_rules_between_every_row() {
        let md = "| a |\n|---|\n| 1 |\n| 2 |\n";
        let lines = table_lines(md, 40);
        let rules = lines.iter().filter(|l| l.starts_with('├')).count();
        // One under the header, one between the two body rows.
        assert_eq!(rules, 2, "{lines:?}");
    }

    /// Rows without outer pipes are still a table; so is a cell containing an
    /// escaped pipe.
    #[test]
    fn table_accepts_bare_and_escaped_pipes() {
        assert_eq!(table_run_len(&["a | b\n", "---|---\n", "1 | 2\n"]), Some(3));
        assert_eq!(split_cells(r"| a \| b | c |"), vec!["a | b", "c"]);
    }

    /// Prose containing a pipe must not stall the stream waiting for a table
    /// that never arrives.
    #[test]
    fn prose_with_a_pipe_is_not_held_back() {
        let s = "run `ls | wc -l` first\nthen check\n";
        assert_eq!(safe_prefix_len(s), s.len());
    }

    /// Pipes in prose are not a table without an alignment rule under them.
    #[test]
    fn pipes_without_a_rule_are_not_a_table() {
        assert_eq!(table_run_len(&["| a | b |\n", "not a rule\n"]), None);
        assert_eq!(table_run_len(&["| a | b |\n", "|---|---|\n"]), Some(2));
    }

    /// A display block must not be flushed before its closing delimiter, or
    /// half of it renders as literal `$$`.
    #[test]
    fn holds_back_an_unterminated_math_block() {
        let s = "Result:\n\n$$\n\\frac{1}{2}\n";
        assert_eq!(&s[..safe_prefix_len(s)], "Result:\n\n");

        let closed = "Result:\n\n$$\n\\frac{1}{2}\n$$\n\n";
        assert_eq!(safe_prefix_len(closed), closed.len());
    }

    #[test]
    fn prose_streams_line_by_line() {
        let s = "one\ntwo\nthree";
        assert_eq!(&s[..safe_prefix_len(s)], "one\ntwo\n");
    }
}



