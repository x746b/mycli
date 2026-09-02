//! Shared terminal UI primitives: theme, measurement, wrapping, and panels.
//!
//! Everything here is plain ANSI written to a `Write`. There is no alternate
//! screen and no full-screen redraw: output stays in the terminal scrollback so
//! it can be scrolled, copied, and piped like ordinary CLI output.

use unicode_width::UnicodeWidthStr;

// ─── Theme ──────────────────────────────────────────────────────────────────

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const ITALIC: &str = "\x1b[3m";
pub const DIM: &str = "\x1b[90m";
pub const ACCENT: &str = "\x1b[36m";
pub const GREEN: &str = "\x1b[32m";
pub const RED: &str = "\x1b[31m";
pub const YELLOW: &str = "\x1b[33m";
pub const MAGENTA: &str = "\x1b[35m";
pub const BLUE: &str = "\x1b[34m";

// Box drawing
const TL: char = '╭';
const TR: char = '╮';
const BL: char = '╰';
const BR: char = '╯';
const H: char = '─';
const V: char = '│';

/// Widest panel we will draw. Full-width boxes on an ultrawide terminal are
/// harder to read than a fixed measure, so cap it.
const MAX_PANEL: usize = 96;

/// Usable terminal width, with a sane fallback when the size is unknown.
///
/// A pty with no window size set reports 0 columns rather than failing, so an
/// implausible answer has to fall back the same way an error does.
pub fn term_width() -> usize {
    crossterm::terminal::size()
        .map(|(c, _)| c as usize)
        .ok()
        .filter(|&c| c >= 20)
        .unwrap_or(80)
}

/// Width for dialogs: the terminal, minus a small margin, capped at
/// [`MAX_PANEL`]. A dialog is a fixed measure so a short command does not sit
/// alone in a box stretched across an ultrawide terminal.
pub fn panel_width() -> usize {
    term_width().saturating_sub(2).clamp(24, MAX_PANEL)
}

/// Width for flowing content — markdown, and the rules around the prompt.
/// Uses the whole terminal, so tables and code blocks get the room they need.
pub fn text_width() -> usize {
    term_width().saturating_sub(1).max(20)
}

// ─── Measurement ────────────────────────────────────────────────────────────

/// Drop ANSI CSI/OSC sequences so a styled string can be measured.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: ends on a byte in @..~
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ends on BEL or ST
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        let _ = chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Rendered column width of a possibly-styled string.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(strip_ansi(s).as_str())
}

/// Truncate to `max` display columns, appending `…` when shortened.
/// Unicode-safe: never slices mid-codepoint.
pub fn truncate(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = UnicodeWidthStr::width(c.to_string().as_str());
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Word-wrap plain text to `width` columns. Existing newlines are preserved;
/// words longer than `width` are hard-split.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        if para.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut cur = String::new();
        let mut cur_w = 0;
        for word in para.split(' ') {
            let ww = UnicodeWidthStr::width(word);
            if ww > width {
                // Hard-split an over-long token (paths, base64, minified JSON).
                if !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                }
                let mut chunk = String::new();
                let mut chunk_w = 0;
                for c in word.chars() {
                    let cw = UnicodeWidthStr::width(c.to_string().as_str());
                    if chunk_w + cw > width {
                        lines.push(std::mem::take(&mut chunk));
                        chunk_w = 0;
                    }
                    chunk.push(c);
                    chunk_w += cw;
                }
                cur = chunk;
                cur_w = chunk_w;
                continue;
            }
            let sep = if cur.is_empty() { 0 } else { 1 };
            if cur_w + sep + ww > width {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
                cur.push_str(word);
                cur_w += ww;
            } else {
                if sep == 1 {
                    cur.push(' ');
                }
                cur.push_str(word);
                cur_w += sep + ww;
            }
        }
        lines.push(cur);
    }
    lines
}

// ─── Panels ─────────────────────────────────────────────────────────────────

/// A rounded box with an optional title in the top border.
///
/// Rows may contain ANSI styling; widths are computed on the stripped text so
/// the right border stays aligned.
pub struct Panel {
    width: usize,
    color: &'static str,
    title: String,
    rows: Vec<String>,
}

impl Panel {
    pub fn new(title: impl Into<String>, color: &'static str) -> Self {
        Self {
            width: panel_width(),
            color,
            title: title.into(),
            rows: Vec::new(),
        }
    }

    /// Content columns available inside the borders.
    pub fn inner_width(&self) -> usize {
        self.width.saturating_sub(4)
    }

    pub fn row(&mut self, content: impl Into<String>) -> &mut Self {
        self.rows.push(content.into());
        self
    }

    pub fn blank(&mut self) -> &mut Self {
        self.rows.push(String::new());
        self
    }

    /// Add plain text, wrapped to the panel width and styled with `style`.
    pub fn text(&mut self, text: &str, style: &str) -> &mut Self {
        for line in wrap(text, self.inner_width()) {
            if style.is_empty() {
                self.rows.push(line);
            } else {
                self.rows.push(format!("{style}{line}{RESET}"));
            }
        }
        self
    }

    pub fn render(&self) -> String {
        let c = self.color;
        let inner = self.inner_width();
        let mut out = String::new();

        // Top border, with the title inlined: ╭─ Title ──────╮
        if self.title.is_empty() {
            out.push_str(&format!(
                "{c}{TL}{}{TR}{RESET}\n",
                H.to_string().repeat(self.width - 2)
            ));
        } else {
            let title = truncate(&self.title, inner);
            let tw = display_width(&title);
            // ╭ ─ ␣ title ␣ …fill… ╮  →  5 fixed columns plus the title.
            let fill = self.width.saturating_sub(5 + tw);
            out.push_str(&format!(
                "{c}{TL}{H} {BOLD}{title}{RESET}{c} {}{TR}{RESET}\n",
                H.to_string().repeat(fill)
            ));
        }

        for row in &self.rows {
            let row = if display_width(row) > inner {
                truncate(row, inner)
            } else {
                row.clone()
            };
            let pad = inner.saturating_sub(display_width(&row));
            out.push_str(&format!(
                "{c}{V}{RESET} {row}{} {c}{V}{RESET}\n",
                " ".repeat(pad)
            ));
        }

        out.push_str(&format!(
            "{c}{BL}{}{BR}{RESET}\n",
            H.to_string().repeat(self.width - 2)
        ));
        out
    }
}

// ─── Tool presentation ──────────────────────────────────────────────────────

/// Icon and colour for a tool, so the transcript is scannable at a glance.
pub fn tool_style(name: &str) -> (&'static str, &'static str) {
    match name {
        "Bash" | "bash" | "PowerShell" => ("❯", MAGENTA),
        "Read" | "file_read" => ("◇", BLUE),
        "Write" | "file_write" => ("✎", YELLOW),
        "Edit" | "file_edit" => ("✎", YELLOW),
        "Glob" | "glob" | "Grep" | "grep" => ("⌕", ACCENT),
        "WebFetch" | "web_fetch" | "WebSearch" | "web_search" => ("⇣", ACCENT),
        "Skill" | "skill" => ("✳", MAGENTA),
        _ => ("◈", ACCENT),
    }
}

/// One-line summary of a tool call, for the transcript header.
pub fn tool_summary(name: &str, input: &serde_json::Value, max: usize) -> String {
    let s = match name {
        "Bash" | "bash" | "PowerShell" => str_field(input, "command"),
        "Read" | "file_read" | "Write" | "file_write" | "Edit" | "file_edit" => {
            str_field(input, "file_path")
        }
        "Glob" | "glob" | "Grep" | "grep" => str_field(input, "pattern"),
        "WebFetch" | "web_fetch" => str_field(input, "url"),
        _ => serde_json::to_string(input).unwrap_or_default(),
    };
    // Collapse newlines so a heredoc doesn't blow up the header line.
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&flat, max)
}

fn str_field(input: &serde_json::Value, key: &str) -> String {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Human-readable path, with `$HOME` collapsed to `~`.
pub fn short_path(p: &str) -> String {
    let home = dirs::home_dir()
        .map(|h| h.display().to_string())
        .unwrap_or_default();
    if !home.is_empty() && p.starts_with(&home) {
        format!("~{}", &p[home.len()..])
    } else {
        p.to_string()
    }
}

/// Detailed, tool-aware rendering of a pending call — the body of the approval
/// dialog. Returns styled rows sized to `width`.
pub fn tool_detail(name: &str, input: &serde_json::Value, width: usize) -> Vec<String> {
    let mut rows = Vec::new();

    match name {
        "Bash" | "bash" | "PowerShell" => {
            let cmd = str_field(input, "command");
            for line in wrap(&cmd, width) {
                rows.push(format!("{BOLD}{line}{RESET}"));
            }
            if let Some(t) = input.get("timeout").and_then(|v| v.as_u64()) {
                rows.push(String::new());
                rows.push(format!("{DIM}timeout: {}s{RESET}", t / 1000));
            }
        }
        "Write" | "file_write" => {
            let path = str_field(input, "file_path");
            let content = str_field(input, "content");
            let total = content.lines().count();
            rows.push(format!("{BOLD}{}{RESET}", truncate(&short_path(&path), width)));
            rows.push(format!(
                "{DIM}{total} line{} · {} bytes{RESET}",
                if total == 1 { "" } else { "s" },
                content.len()
            ));
            rows.push(String::new());
            for line in content.lines().take(PREVIEW_LINES) {
                rows.push(format!("{GREEN}+{RESET} {}", truncate(line, width - 2)));
            }
            if total > PREVIEW_LINES {
                rows.push(format!("{DIM}  … {} more lines{RESET}", total - PREVIEW_LINES));
            }
        }
        "Edit" | "file_edit" => {
            let path = str_field(input, "file_path");
            rows.push(format!("{BOLD}{}{RESET}", truncate(&short_path(&path), width)));
            rows.push(String::new());
            let old = str_field(input, "old_string");
            let new = str_field(input, "new_string");
            if old.is_empty() {
                // Line-range mode: no old_string to diff against.
                if let (Some(a), Some(b)) = (
                    input.get("start_line").and_then(|v| v.as_u64()),
                    input.get("end_line").and_then(|v| v.as_u64()),
                ) {
                    rows.push(format!("{DIM}replacing lines {a}–{b}{RESET}"));
                    rows.push(String::new());
                }
                for line in new.lines().take(PREVIEW_LINES) {
                    rows.push(format!("{GREEN}+{RESET} {}", truncate(line, width - 2)));
                }
                let n = new.lines().count();
                if n > PREVIEW_LINES {
                    rows.push(format!("{DIM}  … {} more lines{RESET}", n - PREVIEW_LINES));
                }
            } else {
                rows.extend(diff_rows(&old, &new, width));
                if input
                    .get("replace_all")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    rows.push(String::new());
                    rows.push(format!("{YELLOW}replaces every occurrence{RESET}"));
                }
            }
        }
        "Read" | "file_read" => {
            let path = str_field(input, "file_path");
            rows.push(format!("{BOLD}{}{RESET}", truncate(&short_path(&path), width)));
            let offset = input.get("offset").and_then(|v| v.as_u64());
            let limit = input.get("limit").and_then(|v| v.as_u64());
            if offset.is_some() || limit.is_some() {
                rows.push(format!(
                    "{DIM}from line {} · {} lines{RESET}",
                    offset.unwrap_or(1),
                    limit.map(|l| l.to_string()).unwrap_or_else(|| "all".into())
                ));
            }
        }
        "Glob" | "glob" | "Grep" | "grep" => {
            rows.push(format!("{BOLD}{}{RESET}", truncate(&str_field(input, "pattern"), width)));
            let path = str_field(input, "path");
            if !path.is_empty() {
                rows.push(format!("{DIM}in {}{RESET}", truncate(&short_path(&path), width - 3)));
            }
        }
        "WebFetch" | "web_fetch" => {
            rows.push(format!("{BOLD}{}{RESET}", truncate(&str_field(input, "url"), width)));
        }
        _ => {
            let json = serde_json::to_string_pretty(input).unwrap_or_default();
            for line in json.lines().take(PREVIEW_LINES * 2) {
                rows.push(format!("{DIM}{}{RESET}", truncate(line, width)));
            }
        }
    }

    if rows.is_empty() {
        rows.push(format!("{DIM}(no arguments){RESET}"));
    }
    rows
}

const PREVIEW_LINES: usize = 12;
const DIFF_LINES: usize = 18;

/// Compact line diff, `-` old / `+` new, with unchanged lines dimmed.
fn diff_rows(old: &str, new: &str, width: usize) -> Vec<String> {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(old, new);
    let mut rows = Vec::new();
    let mut shown = 0;
    let mut hidden = 0;

    for change in diff.iter_all_changes() {
        let text = change.value().trim_end_matches('\n');
        if shown >= DIFF_LINES {
            hidden += 1;
            continue;
        }
        let (sign, style) = match change.tag() {
            ChangeTag::Delete => ("-", RED),
            ChangeTag::Insert => ("+", GREEN),
            ChangeTag::Equal => (" ", DIM),
        };
        rows.push(format!(
            "{style}{sign} {}{RESET}",
            truncate(text, width.saturating_sub(2))
        ));
        shown += 1;
    }
    if hidden > 0 {
        rows.push(format!("{DIM}  … {hidden} more diff lines{RESET}"));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_before_measuring() {
        assert_eq!(display_width("\x1b[31mabc\x1b[0m"), 3);
        assert_eq!(display_width("plain"), 5);
    }

    /// The old renderer sliced with `&s[..max]`, which panics whenever the cut
    /// lands inside a multi-byte character — easy to hit with a path or a
    /// command containing any non-ASCII text.
    #[test]
    fn truncate_is_utf8_safe() {
        let s = "ααααααααα";
        assert_eq!(truncate(s, 4), "ααα…");
        assert_eq!(truncate("héllo wörld", 100), "héllo wörld");
    }

    #[test]
    fn wraps_on_word_boundaries_and_hard_splits_long_tokens() {
        assert_eq!(wrap("one two three", 8), vec!["one two", "three"]);
        let long = "a".repeat(20);
        assert_eq!(wrap(&long, 8).len(), 3);
    }

    #[test]
    fn panel_rows_are_padded_to_a_constant_width() {
        let mut p = Panel::new("Title", ACCENT);
        p.row(format!("{RED}short{RESET}")).row("a longer row");
        let rendered = p.render();
        let widths: Vec<usize> = rendered.lines().map(display_width).collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
    }

    #[test]
    fn tool_summary_flattens_multiline_commands() {
        let input = serde_json::json!({ "command": "echo a\n  echo b" });
        assert_eq!(tool_summary("Bash", &input, 80), "echo a echo b");
    }
}

#[cfg(test)]
mod demo {
    use super::*;
    use crate::render::Renderer;
    use std::time::Duration;

    #[test]
    #[ignore]
    fn visual_demo() {
        // Prompt
        let rule = "─".repeat(text_width());
        println!("\n{DIM}{rule}{RESET}");
        println!(" {ACCENT}{BOLD}›{RESET} write an expression parser and test it");
        println!("{DIM}{rule}{RESET}");

        let mut r = Renderer::new();
        r.push_thinking("The user wants a recursive-descent parser. Grammar: expr -> term (('+'|'-') term)*, term -> factor, factor -> unary, atom -> NUMBER | '(' expr ')'. I'll write a tokenizer first, then four mutually-recursive functions, one per precedence level.\n");
        r.flush();

        // Bash approval
        let req = serde_json::json!({"command": "cd /opt/mycli/bin && python3 code_expr_parser.py && echo done"});
        let mut p = Panel::new(format!("{}  Bash", tool_style("Bash").0), YELLOW);
        let inner = p.inner_width();
        for row in tool_detail("Bash", &req, inner) { p.row(row); }
        p.blank();
        p.row(format!("{DIM}runs a command · approval required{RESET}"));
        print!("\n{}", p.render());
        println!("   \x1b[7m Yes \x1b[0m {DIM} Yes, don't ask again {RESET} {DIM} No {RESET}   {DIM}←→ move · enter confirm · esc deny{RESET}");

        // Edit approval with a diff
        let req = serde_json::json!({
            "file_path": "/opt/mycli/bin/code_expr_parser.py",
            "old_string": "    assert evaluate('-(2+3) * -(4-1)') == -15.0\n    print('ok')\n",
            "new_string": "    assert evaluate('-(2+3) * -(4-1)') == 15.0\n    print('ok')\n",
        });
        let mut p = Panel::new(format!("{}  Edit", tool_style("Edit").0), YELLOW);
        let inner = p.inner_width();
        for row in tool_detail("Edit", &req, inner) { p.row(row); }
        p.blank();
        p.row(format!("{DIM}modifies files · approval required{RESET}"));
        print!("\n{}", p.render());

        // Tool transcript
        let mut r = Renderer::new();
        r.tool_start("Bash", &serde_json::json!({"command": "cd /opt/mycli/bin && python3 code_expr_parser.py"}));
        r.tool_end("Bash", "3 + 4 * 2        = 11.0\n(1 + 2) * -(3 + 4) = -21.0\n1 / 0 -> ValueError\nall 12 checks passed\nextra line a\nextra line b\nextra line c", false, Duration::from_millis(2330));
        r.tool_start("Read", &serde_json::json!({"file_path": "/opt/mycli/missing.rs"}));
        r.tool_end("Read", "No such file or directory (os error 2)", true, Duration::from_millis(4));

        // Markdown: a table, rendered whole.
        let md = "Result:\n\n| Step | a | b | q |\n|---|---:|---:|:-:|\n\
                  | 1 | 1914 | 899 | 2 |\n| 2 | 899 | 116 | 7 |\n\n\
                  So `gcd = 29`.\n";
        print!("\n{ACCENT}●{RESET} {}", crate::markdown::render(md, text_width()));
    }
}
