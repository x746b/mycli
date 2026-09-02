//! Persistent bottom status line.
//!
//! The bar lives on the last terminal row, outside the scroll region, so normal
//! output scrolls underneath it. State is global because it has to be redrawn
//! from places that do not own the REPL loop — notably the Ctrl+O key handler
//! inside rustyline, which needs to reflect the new thinking mode immediately.

use crate::ui;
use parking_lot::Mutex;
use std::io::{self, Write};
use std::path::Path;

struct State {
    enabled: bool,
    model: String,
    provider: String,
    persona: String,
    cwd: String,
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
            total_in: 0,
            total_out: 0,
            last_in: 0,
            prev_cumulative_in: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

/// Reserve the bottom line by shrinking the scroll region.
pub fn setup() {
    if let Ok((_, rows)) = crossterm::terminal::size() {
        if rows < 3 {
            return;
        }
        let mut err = io::stderr();
        let _ = write!(err, "\x1b[1;{}r", rows - 1);
        let _ = write!(err, "\x1b[{};1H", rows - 1);
        let _ = err.flush();
        STATE.lock().enabled = true;
    }
}

/// Restore the full-screen scroll region and clear the bar.
pub fn teardown() {
    let mut state = STATE.lock();
    if !state.enabled {
        return;
    }
    state.enabled = false;
    let mut err = io::stderr();
    let _ = write!(err, "\x1b[r");
    if let Ok((_, rows)) = crossterm::terminal::size() {
        let _ = write!(err, "\x1b[{};1H\x1b[K", rows);
    }
    let _ = err.flush();
}

/// Update the session context shown on the left of the bar.
pub fn set_context(model: &str, provider: &str, persona: &str, cwd: &Path) {
    let mut state = STATE.lock();
    state.model = model.to_string();
    state.provider = provider.to_string();
    state.persona = persona.to_string();
    state.cwd = cwd.display().to_string();
}

/// Clear token counters (on a model or provider switch).
pub fn reset_tokens() {
    let mut state = STATE.lock();
    state.total_in = 0;
    state.total_out = 0;
    state.last_in = 0;
    state.prev_cumulative_in = 0;
}

/// Fold in a fresh cumulative usage report and redraw.
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

/// Repaint the bar. Safe to call at any time, including while rustyline is
/// editing a line: the cursor is saved and restored around the write.
pub fn draw() {
    let state = STATE.lock();
    if !state.enabled {
        return;
    }
    let rows = match crossterm::terminal::size() {
        Ok((_, r)) if r >= 3 => r,
        _ => return,
    };

    let ctx_window = cersei_agent::compact::context_window_for_model(&state.model);
    let ctx_pct = if ctx_window > 0 && state.last_in > 0 {
        (state.last_in as f64 / ctx_window as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let ctx_color = if ctx_pct >= 80.0 {
        ui::RED
    } else if ctx_pct >= 50.0 {
        ui::YELLOW
    } else {
        ui::GREEN
    };

    let thinking = if crate::render::thinking_visible() {
        "think:on"
    } else {
        "think:off"
    };

    // \x1b[7m is reverse video; every embedded colour must re-assert it or the
    // rest of the bar loses its background.
    let content = format!(
        " {} · {} · {} · {}ctx {:.0}%\x1b[0;7m · in {} out {} · {} · {}",
        state.model,
        state.provider,
        state.persona,
        ctx_color,
        ctx_pct,
        fmt_tokens(state.total_in),
        fmt_tokens(state.total_out),
        thinking,
        ui::short_path(&state.cwd),
    );

    let mut err = io::stderr();
    let _ = write!(err, "\x1b[s\x1b[{rows};1H\x1b[7m\x1b[K{content}\x1b[0m\x1b[u");
    let _ = err.flush();
}
