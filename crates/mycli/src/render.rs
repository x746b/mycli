//! Streaming terminal renderer: assistant text, reasoning, and tool activity.
//!
//! Output is line-based rather than a full-screen TUI, so the transcript stays
//! in the scrollback and can be scrolled, selected, and piped.

use crate::markdown;
use crate::ui::{self, ACCENT, BOLD, DIM, GREEN, ITALIC, RED, RESET, YELLOW};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Whether reasoning is streamed to the terminal. On by default; toggled with
/// Ctrl+O at the prompt or `/thinking`.
static THINKING_VISIBLE: AtomicBool = AtomicBool::new(true);

pub fn thinking_visible() -> bool {
    THINKING_VISIBLE.load(Ordering::Relaxed)
}

pub fn set_thinking_visible(on: bool) {
    THINKING_VISIBLE.store(on, Ordering::Relaxed);
}

/// What the current model has actually been seen to do. `chat_template_kwargs`
/// is accepted and silently ignored by oMLX for a template that has no thinking
/// flag, so a successful request proves nothing — only watching whether
/// reasoning ever arrives distinguishes a model that reasons from one that
/// cannot. Reset on every model switch.
static REASONING_SEEN: AtomicBool = AtomicBool::new(false);
static TURNS_SEEN: AtomicU32 = AtomicU32::new(0);

pub fn note_reasoning() {
    REASONING_SEEN.store(true, Ordering::Relaxed);
}

pub fn note_turn() {
    TURNS_SEEN.fetch_add(1, Ordering::Relaxed);
}

pub fn forget_model_observations() {
    REASONING_SEEN.store(false, Ordering::Relaxed);
    TURNS_SEEN.store(0, Ordering::Relaxed);
}

/// `Some(true)` if this model has produced reasoning, `Some(false)` if it has
/// completed turns and never has, `None` while there is nothing to go on.
pub fn model_reasons() -> Option<bool> {
    if REASONING_SEEN.load(Ordering::Relaxed) {
        Some(true)
    } else if TURNS_SEEN.load(Ordering::Relaxed) > 0 {
        Some(false)
    } else {
        None
    }
}

/// Flip reasoning visibility. Returns the new state.
pub fn toggle_thinking() -> bool {
    let next = !thinking_visible();
    set_thinking_visible(next);
    next
}

/// Emit model output verbatim instead of rendering it as markdown.
/// Enabled with MYCLI_RAW=1 (any value except empty or "0").
fn raw_mode() -> bool {
    static RAW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RAW.get_or_init(|| {
        std::env::var("MYCLI_RAW")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

/// How many lines of a successful tool's output to echo inline.
const RESULT_PREVIEW: usize = 6;
/// How many lines of a failed tool's output to echo inline.
const ERROR_PREVIEW: usize = 10;

pub struct Renderer {
    buffer: String,
    in_thinking: bool,
    in_tool: bool,
    /// Reasoning text for the current block, kept so a hidden block can still
    /// be printed later with `/thinking`.
    think_text: String,
    /// Partial (unwrapped) reasoning line being accumulated.
    think_line: String,
    /// Reasoning from the most recent completed block.
    last_thinking: String,
    /// Set once per turn, so the assistant's first text block gets a marker.
    text_block_open: bool,
    /// Non-blank reasoning lines emitted in the current block.
    think_emitted: usize,
    /// A blank reasoning line is held back until a non-blank one follows, so
    /// leading and trailing padding in the model's reasoning is not rendered.
    think_pending_blank: bool,
    /// When set, output is paused (permission prompt is active).
    pub pause_flag: Option<&'static AtomicBool>,
    /// Bumped by the permission policy once it has decided about a tool call.
    ///
    /// The runner emits `ToolStart` *before* consulting the policy, so without
    /// this handshake the tool header races the approval dialog and can print
    /// above it.
    pub decision_seq: Option<&'static AtomicU64>,
    /// Bumped by this renderer when it starts handling a `ToolStart`, once all
    /// earlier output is on screen. The policy waits for it before drawing an
    /// approval dialog, so a dialog can never split the previous tool's result
    /// block in half.
    pub tool_start_seq: Option<&'static AtomicU64>,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_thinking: false,
            in_tool: false,
            think_text: String::new(),
            think_line: String::new(),
            last_thinking: String::new(),
            text_block_open: false,
            think_emitted: 0,
            think_pending_blank: false,
            pause_flag: None,
            decision_seq: None,
            tool_start_seq: None,
        }
    }

    /// Check if output should be paused (permission prompt active).
    fn is_paused(&self) -> bool {
        self.pause_flag
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    // ─── Assistant text ─────────────────────────────────────────────────────

    pub fn push_text(&mut self, delta: &str) {
        // In raw mode (benchmarks, piping to a file) we must not silently drop
        // assistant text emitted while a tool is running — doing so made whole
        // responses look empty in captured transcripts.
        if self.is_paused() || (self.in_tool && !raw_mode()) {
            return; // Drop text while permission prompt or tool execution is active
        }
        if self.in_thinking {
            self.end_thinking();
        }
        if !raw_mode() && self.is_leading_padding(delta) {
            return;
        }
        self.buffer.push_str(delta);
        // Flush only what markdown can lay out correctly without the rest —
        // prose streams line by line, tables and code fences wait for their
        // closing line. See `markdown::safe_prefix_len`.
        let cut = if raw_mode() {
            self.buffer.rfind('\n').map(|i| i + 1).unwrap_or(0)
        } else {
            markdown::safe_prefix_len(&self.buffer)
        };
        if cut > 0 {
            let to_flush = self.buffer[..cut].to_string();
            self.buffer.drain(..cut);
            self.print_markdown(&to_flush);
        }
    }

    /// Is this delta blank padding that arrived before any real content?
    ///
    /// Models routinely emit a couple of bare newlines before their first
    /// token, and rendering those leaves the transcript full of stray blank
    /// lines. Only the *leading* run qualifies: the buffer must still be empty
    /// as well. Testing `text_block_open` alone dropped every newline-only
    /// delta up to the first flush — and since a flush needs a newline in the
    /// buffer, the first flush never came and the whole response arrived as a
    /// single line, with headings glued to the prose before them and tables
    /// collapsed onto one row.
    fn is_leading_padding(&self, delta: &str) -> bool {
        !self.text_block_open && self.buffer.is_empty() && delta.trim().is_empty()
    }

    // ─── Reasoning ──────────────────────────────────────────────────────────

    pub fn push_thinking(&mut self, delta: &str) {
        note_reasoning();
        if self.is_paused() || raw_mode() {
            self.think_text.push_str(delta);
            return;
        }
        if !self.in_thinking {
            self.begin_thinking();
        }
        self.think_text.push_str(delta);

        if !thinking_visible() {
            self.draw_collapsed_thinking();
            return;
        }

        let width = self.thinking_width();
        for ch in delta.chars() {
            if ch == '\r' {
                continue;
            }
            if ch == '\n' {
                self.emit_thinking_line();
                continue;
            }
            self.think_line.push(ch);
            if ui::display_width(&self.think_line) >= width {
                // Prefer breaking at the last space so words stay intact.
                match self.think_line.rfind(' ') {
                    Some(pos) if pos > width / 3 => {
                        let rest = self.think_line[pos + 1..].to_string();
                        self.think_line.truncate(pos);
                        self.emit_thinking_line();
                        self.think_line = rest;
                    }
                    _ => self.emit_thinking_line(),
                }
            }
        }
    }

    fn thinking_width(&self) -> usize {
        ui::panel_width().saturating_sub(4).max(20)
    }

    fn begin_thinking(&mut self) {
        self.in_thinking = true;
        self.think_text.clear();
        self.think_line.clear();
        self.think_emitted = 0;
        self.think_pending_blank = false;
        self.flush_text_buffer();
        self.text_block_open = false;
        let mut err = io::stderr();
        if thinking_visible() {
            let _ = write!(err, "\n  {DIM}{ITALIC}✻ Thinking{RESET}\n");
        } else {
            // Give the collapsed indicator a line of its own — it redraws with
            // \r, so it would otherwise overwrite whatever precedes it.
            let _ = write!(err, "\n");
        }
        let _ = err.flush();
    }

    fn emit_thinking_line(&mut self) {
        let line = std::mem::take(&mut self.think_line);
        if line.trim().is_empty() {
            // Hold it back: only render a gap that turns out to be interior.
            self.think_pending_blank = self.think_emitted > 0;
            return;
        }
        let mut err = io::stderr();
        if self.think_pending_blank {
            let _ = write!(err, "  {DIM}│{RESET}\n");
            self.think_pending_blank = false;
        }
        let _ = write!(err, "  {DIM}│ {line}{RESET}\n");
        let _ = err.flush();
        self.think_emitted += 1;
    }

    /// Single self-overwriting line shown when reasoning is hidden, so the user
    /// still sees that the model is working.
    fn draw_collapsed_thinking(&self) {
        let chars = self.think_text.chars().count();
        let _ = write!(
            io::stderr(),
            "\r\x1b[K  {DIM}{ITALIC}✻ Thinking… {chars} chars · ctrl+o to show{RESET}"
        );
        let _ = io::stderr().flush();
    }

    fn end_thinking(&mut self) {
        if !self.in_thinking {
            return;
        }
        self.in_thinking = false;
        if thinking_visible() && !raw_mode() {
            if !self.think_line.is_empty() {
                self.emit_thinking_line();
            }
            self.think_pending_blank = false;
        } else if !raw_mode() {
            self.think_line.clear();
            let _ = write!(io::stderr(), "\n");
        }
        let _ = io::stderr().flush();
        self.last_thinking = std::mem::take(&mut self.think_text);
    }

    /// Print the most recent reasoning block. Used by `/thinking` after a block
    /// was streamed in collapsed form.
    pub fn replay_last_thinking(&self) -> bool {
        if self.last_thinking.trim().is_empty() {
            return false;
        }
        let width = self.thinking_width();
        let mut err = io::stderr();
        let _ = write!(err, "\n  {DIM}{ITALIC}✻ Thinking (replay){RESET}\n");
        for line in ui::wrap(&self.last_thinking, width) {
            let _ = write!(err, "  {DIM}│ {line}{RESET}\n");
        }
        let _ = err.flush();
        true
    }

    // ─── Tools ──────────────────────────────────────────────────────────────

    pub fn tool_start(&mut self, name: &str, input: &serde_json::Value) {
        self.in_tool = true;
        // Everything the previous events produced must be on screen before the
        // policy is allowed to draw a dialog.
        self.flush();
        if let Some(seq) = self.tool_start_seq {
            seq.fetch_add(1, Ordering::SeqCst);
        }
        self.await_permission_decision();
        self.text_block_open = false;

        let (icon, color) = ui::tool_style(name);
        let width = ui::panel_width();
        let summary = ui::tool_summary(name, input, width.saturating_sub(name.len() + 6));
        let mut out = format!("\r\n  {color}{BOLD}{icon} {name}{RESET}");
        if !summary.is_empty() {
            out.push_str(&format!("  {DIM}{summary}{RESET}"));
        }
        out.push_str("\r\n");
        // One write, so a concurrent dialog cannot land mid-header.
        let mut err = io::stderr();
        let _ = err.write_all(out.as_bytes());
        let _ = err.flush();
    }

    pub fn tool_end(&mut self, name: &str, result: &str, is_error: bool, duration: Duration) {
        self.in_tool = false;
        let (color, icon) = if is_error { (RED, "\u{2717}") } else { (GREEN, "\u{2713}") };
        let lines = result.lines().count();

        let mut out = format!(
            "  {color}\u{23bf} {icon}{RESET} {DIM}{}{RESET}\r\n",
            summarize_result(name, result, lines, is_error, duration)
        );

        let take = if is_error { ERROR_PREVIEW } else { RESULT_PREVIEW };
        let body_color = if is_error { RED } else { DIM };
        let width = ui::panel_width().saturating_sub(6);
        let mut shown = 0;
        for line in result.lines() {
            if line.trim().is_empty() && shown == 0 {
                continue; // skip leading blank lines
            }
            if shown >= take {
                break;
            }
            out.push_str(&format!(
                "    {body_color}{}{RESET}\r\n",
                ui::truncate(line, width)
            ));
            shown += 1;
        }
        if lines > shown && shown > 0 {
            out.push_str(&format!(
                "    {DIM}\u{2026} +{} line{}{RESET}\r\n",
                lines - shown,
                if lines - shown == 1 { "" } else { "s" }
            ));
        }

        // One write: a concurrent approval dialog must not split this block.
        let mut err = io::stderr();
        let _ = err.write_all(out.as_bytes());
        let _ = err.flush();
    }

    // \u{2500}\u{2500}\u{2500} Diagnostics \u{2500}\u{2500}\u{2500}

    /// Block until the permission policy has ruled on the pending call, so the
    /// header always lands below any approval dialog.
    ///
    /// Two phases: wait for the policy to *reach* a decision (bounded — a tool
    /// the runner rejects as unknown never reaches the policy at all), then
    /// wait out any dialog it raised, which is only over when the user answers.
    fn await_permission_decision(&self) {
        if let Some(seq) = self.decision_seq {
            let start = seq.load(Ordering::SeqCst);
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while seq.load(Ordering::SeqCst) == start && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        while self.is_paused() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn error(&mut self, msg: &str) {
        self.flush();
        let mut panel = ui::Panel::new("Error", RED);
        panel.text(msg, RED);
        let _ = write!(io::stderr(), "\n{}", panel.render());
        let _ = io::stderr().flush();
    }

    /// A dim, single-line notice (mode switches, hints).
    pub fn notice(&mut self, msg: &str) {
        let _ = write!(io::stderr(), "  {DIM}{msg}{RESET}\n");
        let _ = io::stderr().flush();
    }

    // ─── Lifecycle ──────────────────────────────────────────────────────────

    fn flush_text_buffer(&mut self) {
        if !self.buffer.is_empty() {
            let remaining = std::mem::take(&mut self.buffer);
            self.print_markdown(&remaining);
        }
    }

    pub fn flush(&mut self) {
        self.end_thinking();
        self.flush_text_buffer();
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
    }

    pub fn complete(&mut self) {
        self.flush();
        self.text_block_open = false;
        let _ = write!(io::stdout(), "\n");
        let _ = io::stdout().flush();
    }

    fn print_markdown(&mut self, text: &str) {
        // Raw mode emits the model's bytes verbatim. termimad rewrites markdown
        // for display, which mangles technical output: `*` is consumed as
        // emphasis (so `{{7*7}}` prints as `{{77}}`) and tables become box
        // drawing. Benchmarks and redirected output need the original text.
        if raw_mode() {
            print!("{text}");
            let _ = io::stdout().flush();
            return;
        }
        let rendered = markdown::render(text, ui::wrap_width());
        if !self.text_block_open {
            if rendered.trim().is_empty() {
                return;
            }
            self.text_block_open = true;
            // A block element — a table, a code fence — starts with its own
            // frame, so the marker goes on the line above rather than butting
            // up against the border.
            let starts_block = ui::strip_ansi(&rendered)
                .trim_start()
                .starts_with(|c| "╭┌├└╰│─".contains(c));
            print!("\n{ACCENT}●{RESET}{}", if starts_block { "\n" } else { " " });
        }
        print!("{rendered}");
        let _ = io::stdout().flush();
    }
}

/// Told the user their Esc was seen. Printed from the key watcher thread, so
/// it is one write and deliberately short.
pub fn interrupt_notice() {
    let mut err = io::stderr();
    let _ = err.write_all(format!("\n  {YELLOW}\u{2718} interrupted{RESET}\n").as_bytes());
    let _ = err.flush();
}

/// One-line outcome for a finished tool: what it produced and how long it took.
fn summarize_result(
    name: &str,
    result: &str,
    lines: usize,
    is_error: bool,
    duration: Duration,
) -> String {
    let secs = duration.as_secs_f64();
    let took = if secs >= 1.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}ms", duration.as_millis())
    };
    if is_error {
        return format!("{name} failed · {took}");
    }
    let shape = if result.trim().is_empty() {
        "no output".to_string()
    } else if lines <= 1 {
        format!("{} chars", result.chars().count())
    } else {
        format!("{lines} lines")
    };
    format!("{shape} · {took}")
}

// ─── Startup banner ─────────────────────────────────────────────────────────

/// Print the logo. Emitted before the agent is constructed so that provider,
/// MCP, and tool-tier lines appear underneath it rather than above.
/// Spaces between the logo's descender line and the version. The descender
/// leaves the right half of that line empty, which is where the version goes —
/// flush with the logo's own right edge, so it reads as part of the block
/// rather than a line tacked underneath. At least one space always separates
/// them, however long the version string grows.
fn version_pad(logo: &str, last_line: &str, version: &str) -> usize {
    let width = logo.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    width
        .saturating_sub(last_line.chars().count() + version.chars().count())
        .max(1)
}

pub fn logo() {
    const LOGO: &str = r#"                   _____ _     __
                  / ____| |   /_ |
  _ __ ___  _   _| |    | |    | |
 | '_ ` _ \| | | | |    | |    | |
 | | | | | | |_| | |____| |____| |
 |_| |_| |_|\__, |\_____|______|_|
             __/ |
            |___/"#;

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let mut lines: Vec<String> = LOGO.lines().map(str::to_string).collect();
    if let Some(last) = lines.pop() {
        let pad = " ".repeat(version_pad(LOGO, &last, &version));
        lines.push(format!("{last}{pad}{RESET}{DIM}{version}"));
    }

    let _ = write!(
        io::stderr(),
        "\n{ACCENT}{BOLD}{}{RESET}\n\n",
        lines.join("\n")
    );
    let _ = io::stderr().flush();
}

/// Print the session context line and key hints, after the agent is built.
pub fn session_info(config: &crate::config::Config, model_display: &str) {
    let cwd = ui::short_path(&config.working_dir.display().to_string());
    let mut err = io::stderr();
    let _ = write!(
        err,
        "  {DIM}{} · {} · tools:{} · max_turns:{} · {}{RESET}\n",
        config.provider,
        model_display,
        crate::config::resolve_tool_tier(config),
        config.max_turns,
        cwd,
    );
    let _ = write!(
        err,
        "  {DIM}ctrl+c interrupt · ctrl+d exit · / commands · ctrl+o thinking · ctrl+u clear input{RESET}\n"
    );
    let _ = err.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_summary_reports_shape_and_timing() {
        let d = Duration::from_millis(120);
        assert_eq!(summarize_result("Bash", "", 0, false, d), "no output · 120ms");
        assert_eq!(summarize_result("Bash", "hi", 1, false, d), "2 chars · 120ms");
        assert_eq!(summarize_result("Bash", "a\nb\nc", 3, false, d), "3 lines · 120ms");
        assert_eq!(
            summarize_result("Bash", "boom", 1, true, Duration::from_millis(2500)),
            "Bash failed · 2.5s"
        );
    }

    /// The regression that made every response arrive as one line.
    #[test]
    fn newlines_survive_before_the_first_flush() {
        let mut r = Renderer::new();
        // Blank deltas before any content are padding.
        assert!(r.is_leading_padding("\n"));
        assert!(r.is_leading_padding("\n\n"));

        r.buffer.push_str("# Heading");
        // Now a newline-only delta is content: it is what makes the next line
        // a new block rather than a continuation of this one.
        assert!(!r.is_leading_padding("\n"));
        assert!(!r.is_leading_padding("\n\n"));
    }

    /// End to end: deltas in, buffer keeps every newline the model sent.
    #[test]
    fn streamed_deltas_keep_their_structure() {
        let mut r = Renderer::new();
        for delta in ["\n\n", "# Title", "\n", "\n", "## Part 1", "\n", "body"] {
            r.push_text(delta);
        }
        // Whatever has not flushed yet plus what has must reconstruct the
        // source; the trailing partial line is what remains buffered.
        assert!(r.buffer.ends_with("body"), "{:?}", r.buffer);
        assert!(!r.buffer.starts_with('\n'), "leading padding kept: {:?}", r.buffer);
    }

    /// These statics are process-wide, so anything touching them serialises —
    /// the alternative is a test that passes alone and fails in a full run.
    static OBSERVATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// oMLX accepts `chat_template_kwargs` for a template with no thinking flag
    /// and ignores it, so "the request succeeded" is not evidence the switch
    /// did anything. Only observed reasoning is.
    #[test]
    fn model_reasoning_is_reported_only_once_observed() {
        let _guard = OBSERVATION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        forget_model_observations();
        assert_eq!(model_reasons(), None, "nothing seen yet is not a verdict");

        // A completed turn with no reasoning: a Gemma-style model.
        note_turn();
        assert_eq!(model_reasons(), Some(false));

        // Reasoning arriving settles it, and outweighs any number of quiet turns.
        note_reasoning();
        assert_eq!(model_reasons(), Some(true));
        note_turn();
        assert_eq!(model_reasons(), Some(true));

        // Observations belong to a model, not to the session.
        forget_model_observations();
        assert_eq!(model_reasons(), None);
    }

    /// The version sits flush with the logo's right edge, and never runs into
    /// the ASCII art no matter how long the version string gets.
    #[test]
    fn version_sits_flush_with_the_logo_edge() {
        let logo = "  aaaaaaaaaaaaaaaaaa\n bbbb";
        let width = 20;

        let pad = version_pad(logo, " bbbb", "v1.0.0");
        assert_eq!(" bbbb".len() + pad + "v1.0.0".len(), width, "right edges differ");

        // A version too long to fit still keeps a separating space.
        assert_eq!(version_pad(logo, " bbbb", "v100.200.300-rc1"), 1);
    }

    #[test]
    fn thinking_toggle_round_trips() {
        let start = thinking_visible();
        let flipped = toggle_thinking();
        assert_eq!(flipped, !start);
        set_thinking_visible(start);
        assert_eq!(thinking_visible(), start);
    }
}
