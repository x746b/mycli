//! Persistent two-line footer.
//!
//! The last two terminal rows are held outside the scroll region, so the
//! transcript scrolls underneath them:
//!
//! ```text
//! /opt/mycli/bin (main)
//! ↑2.8k ↓36 · ctx 2.8%/128k · think:on            (omlx) mlx-community_Qwen3.8-27B
//! ```
//!
//! State is global because the footer has to be repainted from places that do
//! not own the REPL loop — the Ctrl+O handler inside rustyline, and the key
//! watcher thread during a turn.

use crate::ui::{self, DIM, RESET};
use parking_lot::Mutex;
use std::io::{self, Write};
use std::path::Path;

/// Rows held back from the scroll region.
const FOOTER_ROWS: u16 = 2;

struct State {
    enabled: bool,
    model: String,
    provider: String,
    persona: String,
    cwd: String,
    branch: String,
    total_in: u64,
    total_out: u64,
    last_in: u64,
    prev_cumulative_in: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            enabled: false,
            model: String::new(),
            provider: String::new(),
            persona: String::new(),
            cwd: String::new(),
            branch: String::new(),
            total_in: 0,
            total_out: 0,
            last_in: 0,
            prev_cumulative_in: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

/// Reserve the footer rows by shrinking the scroll region.
///
/// The rows are made by scrolling existing content up, not by jumping the
/// cursor to a fixed row: the cursor may already be above that row after a
/// short banner, and moving it there would overwrite what was just printed.
pub fn setup() {
    let Ok((_, rows)) = crossterm::terminal::size() else {
        return;
    };
    if rows < FOOTER_ROWS + 2 {
        return;
    }
    let out = format!(
        "{}\x1b[1;{}r\x1b[{}A",
        "\n".repeat(FOOTER_ROWS as usize),
        rows - FOOTER_ROWS,
        FOOTER_ROWS,
    );
    let mut err = io::stderr();
    let _ = err.write_all(out.as_bytes());
    let _ = err.flush();
    STATE.lock().enabled = true;
}

/// Restore the full-screen scroll region and clear the footer.
pub fn teardown() {
    let mut state = STATE.lock();
    if !state.enabled {
        return;
    }
    state.enabled = false;
    let mut out = String::from("\x1b[r");
    if let Ok((_, rows)) = crossterm::terminal::size() {
        for row in (rows - FOOTER_ROWS + 1)..=rows {
            out.push_str(&format!("\x1b[{row};1H\x1b[K"));
        }
        out.push_str(&format!("\x1b[{};1H", rows - FOOTER_ROWS));
    }
    let mut err = io::stderr();
    let _ = err.write_all(out.as_bytes());
    let _ = err.flush();
}

/// Update the session context. Re-reads the git branch, so call it when the
/// working directory or the session configuration changes, not per token.
pub fn set_context(model: &str, provider: &str, persona: &str, cwd: &Path) {
    let branch = git_branch(cwd);
    let mut state = STATE.lock();
    state.model = model.to_string();
    state.provider = provider.to_string();
    state.persona = persona.to_string();
    state.cwd = cwd.display().to_string();
    state.branch = branch;
}

fn git_branch(cwd: &Path) -> String {
    std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Clear token counters (on a model or provider switch).
pub fn reset_tokens() {
    let mut state = STATE.lock();
    state.total_in = 0;
    state.total_out = 0;
    state.last_in = 0;
    state.prev_cumulative_in = 0;
}

/// Fold in a fresh cumulative usage report and repaint.
pub fn update_usage(usage: &cersei_types::Usage) {
    {
        let mut state = STATE.lock();
        state.total_in = usage.input_tokens;
        state.total_out = usage.output_tokens;
        // The last turn's input tokens approximate the live conversation size.
        let delta = usage.input_tokens.saturating_sub(state.prev_cumulative_in);
        if delta > 0 {
            state.last_in = delta;
        }
        state.prev_cumulative_in = usage.input_tokens;
    }
    draw();
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Repaint the footer. Safe to call at any time, including while rustyline is
/// editing a line: the cursor is saved and restored around the write.
pub fn draw() {
    let state = STATE.lock();
    if !state.enabled {
        return;
    }
    let (cols, rows) = match crossterm::terminal::size() {
        Ok((c, r)) if r >= FOOTER_ROWS + 2 => (c as usize, r),
        _ => return,
    };

    // Line 1: working directory and git branch.
    let mut location = ui::short_path(&state.cwd);
    if !state.branch.is_empty() {
        location.push_str(&format!(" ({})", state.branch));
    }

    // Line 2: token flow and context on the left, model on the right.
    let ctx_window = cersei_agent::compact::context_window_for_model(&state.model);
    let ctx = if ctx_window > 0 && state.last_in > 0 {
        let pct = (state.last_in as f64 / ctx_window as f64 * 100.0).min(100.0);
        let color = if pct >= 80.0 {
            ui::RED
        } else if pct >= 50.0 {
            ui::YELLOW
        } else {
            ui::GREEN
        };
        format!(
            "{color}{pct:.1}%{RESET}{DIM}/{}",
            fmt_tokens(ctx_window as u64)
        )
    } else {
        format!("–/{}", fmt_tokens(ctx_window as u64))
    };

    let left = format!(
        "↑{} ↓{} · ctx {} · {} · think:{}",
        fmt_tokens(state.total_in),
        fmt_tokens(state.total_out),
        ctx,
        state.persona,
        if crate::render::thinking_visible() { "on" } else { "off" },
    );
    let right = format!("({}) {}", state.provider, state.model);

    // Pad between the halves so the model sits flush right; drop the right
    // half entirely rather than let the line wrap onto the scroll region.
    let used = ui::display_width(&left) + ui::display_width(&right);
    let stats = if used + 2 <= cols {
        format!("{left}{}{right}", " ".repeat(cols - used))
    } else {
        ui::truncate(&left, cols)
    };

    // One write. `write!` emits a syscall per format fragment, so a footer
    // built piecewise can be cut in half by the agent thread's output and land
    // in the middle of a panel.
    let out = format!(
        "\x1b[s\x1b[{};1H\x1b[K{DIM}{}{RESET}\x1b[{};1H\x1b[K{DIM}{}{RESET}\x1b[u",
        rows - 1,
        ui::truncate(&location, cols),
        rows,
        stats,
    );
    let mut err = io::stderr();
    let _ = err.write_all(out.as_bytes());
    let _ = err.flush();
}
