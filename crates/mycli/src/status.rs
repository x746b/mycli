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
use std::time::Duration;

/// Rows held back from the scroll region.
const FOOTER_ROWS: u16 = 2;

struct State {
    enabled: bool,
    model: String,
    provider: String,
    persona: String,
    cwd: String,
    branch: String,
    /// Tokens the model can hold, as resolved by the agent — the provider's
    /// figure when it states one, a guess from the model name otherwise.
    context_window: u64,
    total_in: u64,
    total_out: u64,
    last_in: u64,
    prev_cumulative_in: u64,
    /// Prompt tokens the server actually had to process, and the time it took,
    /// accumulated over the turns of the prompt in flight.
    pp_tokens: u64,
    pp_time: Duration,
    tg_tokens: u64,
    tg_time: Duration,
    /// `input_tokens` reported by the previous turn, session-wide.
    prev_turn_input: Option<u64>,
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
            context_window: 0,
            total_in: 0,
            total_out: 0,
            last_in: 0,
            prev_cumulative_in: 0,
            pp_tokens: 0,
            pp_time: Duration::ZERO,
            tg_tokens: 0,
            tg_time: Duration::ZERO,
            prev_turn_input: None,
        }
    }
}

/// Tokens per second, or `None` when the sample is too small to mean anything.
///
/// A turn that generated nothing, or finished faster than the clock can
/// usefully resolve, would otherwise report a wild number.
pub fn rate(tokens: u64, elapsed: Duration) -> Option<f64> {
    let secs = elapsed.as_secs_f64();
    if tokens == 0 || secs < 0.005 {
        return None;
    }
    Some(tokens as f64 / secs)
}

/// Format a throughput compactly: whole numbers once it is fast enough that a
/// decimal place stops carrying information.
fn fmt_rate(tps: f64) -> String {
    if tps >= 100.0 {
        format!("{tps:.0}")
    } else {
        format!("{tps:.1}")
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

/// Reserve the footer rows by shrinking the scroll region.
///
/// Two things have to be got right here.
///
/// The rows are made by scrolling existing content up, not by jumping the
/// cursor to a fixed row: after a short banner the cursor sits well above that
/// row, and moving it down there would strand the banner and leave a screen of
/// blank space.
///
/// And DECSTBM (`ESC [ t ; b r`) homes the cursor to the top-left of the new
/// region as a side effect. Without saving and restoring around it, everything
/// printed afterwards — the input rule, the prompt — lands at the top of the
/// screen, on top of the banner.
pub fn setup() {
    let Ok((_, rows)) = crossterm::terminal::size() else {
        return;
    };
    if rows < FOOTER_ROWS + 2 {
        return;
    }
    let out = format!(
        "{newlines}\x1b[s\x1b[1;{bottom}r\x1b[u\x1b[{up}A",
        newlines = "\n".repeat(FOOTER_ROWS as usize),
        bottom = rows - FOOTER_ROWS,
        up = FOOTER_ROWS,
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
    // Clear the footer, then drop the region — and restore the cursor after,
    // because resetting the region homes it just as setting one does.
    let mut out = String::from("\x1b[s");
    if let Ok((_, rows)) = crossterm::terminal::size() {
        for row in (rows - FOOTER_ROWS + 1)..=rows {
            out.push_str(&format!("\x1b[{row};1H\x1b[K"));
        }
    }
    out.push_str("\x1b[r\x1b[u");
    let mut err = io::stderr();
    let _ = err.write_all(out.as_bytes());
    let _ = err.flush();
}

/// Update the session context. Re-reads the git branch, so call it when the
/// working directory or the session configuration changes, not per token.
pub fn set_context(
    model: &str,
    provider: &str,
    persona: &str,
    cwd: &Path,
    context_window: u64,
) {
    let branch = git_branch(cwd);
    let mut state = STATE.lock();
    state.context_window = context_window;
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
    state.prev_turn_input = None;
    begin_prompt_locked(&mut state);
}

fn begin_prompt_locked(state: &mut State) {
    state.pp_tokens = 0;
    state.pp_time = Duration::ZERO;
    state.tg_tokens = 0;
    state.tg_time = Duration::ZERO;
}

/// Start a fresh throughput measurement for a new user prompt.
///
/// The rates shown describe the request in flight, not a session-long average
/// that would bury a model switch or a change in prompt size.
pub fn begin_prompt() {
    begin_prompt_locked(&mut STATE.lock());
}

/// Fold in one completed turn and repaint.
///
/// The provider's own figures are used wherever it reports them, so the
/// numbers match what the server says it did:
///
/// * **Prefill tokens.** `input_tokens` is the whole prompt the turn was sent,
///   which on an agentic task re-sends the conversation every turn. What was
///   actually processed is that minus whatever the cache served. When the
///   provider reports no cache count, prompt *growth* since the previous turn
///   stands in — right for a backend that reuses a KV cache, low for one that
///   does not.
/// * **Durations.** A server that times its own phases is measuring compute;
///   the client-side window additionally contains the HTTP round trip and any
///   queueing. Prefer the server's.
///
/// `client_timing` is `None` for a turn that produced no text or reasoning — a
/// bare tool call, which streams structured arguments rather than deltas, so
/// there is no first token to divide the turn at. Such a turn is still
/// measurable when the provider reports durations. Either way it advances the
/// prompt-size chain below: skipping it would make the *next* turn's growth
/// look like two turns' worth.
pub fn record_turn(usage: &cersei_types::Usage, client_timing: Option<(Duration, Duration)>) {
    {
        let mut state = STATE.lock();

        // Which tokens count depends on which clock is being used.
        let (prefill_tokens, prefill) = match usage.prefill_seconds() {
            // The provider timed the work it actually did, so its rate is the
            // whole prompt over that time — cache hits cost it no compute and
            // no time. Dividing uncached tokens by the same duration would
            // report a number the server never claimed.
            Some(secs) => (usage.input_tokens, Some(Duration::from_secs_f64(secs))),
            // Client-timed instead: the window is wall clock and covers the
            // whole request, so counting cached tokens would flatter a warm
            // cache. Use what the server can't have had ready.
            None => {
                let processed = match usage.cached_input_tokens() {
                    Some(cached) => usage.input_tokens.saturating_sub(cached),
                    None => match state.prev_turn_input {
                        Some(prev) if usage.input_tokens > prev => usage.input_tokens - prev,
                        Some(_) => 0,
                        None => usage.input_tokens,
                    },
                };
                (processed, client_timing.map(|(prefill, _)| prefill))
            }
        };
        state.prev_turn_input = Some(usage.input_tokens);

        let decode = usage
            .decode_seconds()
            .map(Duration::from_secs_f64)
            .or_else(|| client_timing.map(|(_, decode)| decode));

        if let Some(prefill) = prefill {
            if prefill_tokens > 0 {
                state.pp_tokens += prefill_tokens;
                state.pp_time += prefill;
            }
        }
        if let Some(decode) = decode {
            state.tg_tokens += usage.output_tokens;
            state.tg_time += decode;
        }
    }
    draw();
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
    let ctx_window = state.context_window;
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

    // Throughput is omitted until a turn has produced a real measurement,
    // rather than shown as a placeholder.
    let mut speed = String::new();
    if let Some(pp) = rate(state.pp_tokens, state.pp_time) {
        speed.push_str(&format!(" · pp {} t/s", fmt_rate(pp)));
    }
    if let Some(tg) = rate(state.tg_tokens, state.tg_time) {
        speed.push_str(&format!(" · tg {} t/s", fmt_rate(tg)));
    }

    let left = format!(
        "↑{} ↓{}{speed} · ctx {} · {} · think:{}",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_needs_a_usable_sample() {
        assert_eq!(rate(0, Duration::from_secs(1)), None);
        assert_eq!(rate(100, Duration::from_micros(500)), None);
        assert_eq!(rate(50, Duration::from_secs(2)), Some(25.0));
    }

    /// These tests drive the process-wide footer state, so they cannot run
    /// beside each other. Without this they interleave and read each other's
    /// tokens — which passed or failed depending on thread scheduling.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn rates() -> (Option<f64>, Option<f64>) {
        let s = STATE.lock();
        (rate(s.pp_tokens, s.pp_time), rate(s.tg_tokens, s.tg_time))
    }

    fn usage(input: u64, output: u64, provider: serde_json::Value) -> cersei_types::Usage {
        cersei_types::Usage {
            input_tokens: input,
            output_tokens: output,
            provider_usage: provider,
            ..Default::default()
        }
    }

    const NONE: serde_json::Value = serde_json::Value::Null;

    /// Without a cache count, re-sent context must not be charged as prefill
    /// work again, or an agentic task reports a prompt rate several times too
    /// high.
    #[test]
    fn only_prompt_growth_counts_when_the_cache_is_unreported() {
        let _guard = TEST_LOCK.lock();
        reset_tokens();
        let second = Duration::from_secs(1);

        record_turn(&usage(1000, 10, NONE), Some((second, second)));
        assert_eq!(rates().0, Some(1000.0));

        // Turn 2 re-sends those 1000 plus 200 new ones. Counting all 1200
        // again would claim 1100 t/s; only the 200 are new work.
        record_turn(&usage(1200, 10, NONE), Some((second, second)));
        assert_eq!(rates().0, Some(600.0)); // 1200 new tokens over 2s

        // Generation accumulates in full — none of it is re-sent.
        assert_eq!(rates().1, Some(10.0)); // 20 tokens over 2s
        reset_tokens();
    }

    /// On the client clock, a reported cache count is exact and beats the
    /// growth guess.
    #[test]
    fn a_reported_cache_count_is_used_verbatim() {
        let _guard = TEST_LOCK.lock();
        reset_tokens();
        let second = Duration::from_secs(1);
        let cached = |n: u64| serde_json::json!({"prompt_tokens_details": {"cached_tokens": n}});

        record_turn(&usage(1000, 10, cached(0)), Some((second, second)));
        assert_eq!(rates().0, Some(1000.0));

        // The prompt grew by 200, but the server had to reprocess 700 of it.
        record_turn(&usage(1200, 10, cached(500)), Some((second, second)));
        assert_eq!(rates().0, Some(850.0)); // (1000 + 700) over 2s
        reset_tokens();
    }

    /// On the provider's own clock the whole prompt counts, cache hits
    /// included — that duration covers only the work it really did, so this
    /// reproduces the figure the server itself reports.
    #[test]
    fn a_provider_clock_counts_the_whole_prompt() {
        let _guard = TEST_LOCK.lock();
        reset_tokens();
        record_turn(
            &usage(
                2772,
                100,
                serde_json::json!({
                    "prompt_tokens_details": {"cached_tokens": 2048},
                    "prompt_eval_duration": 0.72,
                }),
            ),
            Some((Duration::from_secs(9), Duration::from_secs(9))),
        );
        // 2772 / 0.72, not (2772 - 2048) / 0.72.
        assert_eq!(rates().0.map(|r| r.round()), Some(3850.0));
        reset_tokens();
    }

    /// When the server times its own phases, its numbers win over the
    /// client-side window, which also contains the round trip.
    #[test]
    fn provider_durations_beat_client_timing() {
        let _guard = TEST_LOCK.lock();
        reset_tokens();
        let slow = Duration::from_secs(10);
        record_turn(
            &usage(
                1000,
                50,
                serde_json::json!({
                    "prompt_eval_duration": 0.5,
                    "generation_duration": 2.0,
                }),
            ),
            Some((slow, slow)),
        );
        assert_eq!(rates().0, Some(2000.0)); // 1000 / 0.5, not 1000 / 10
        assert_eq!(rates().1, Some(25.0)); // 50 / 2.0, not 50 / 10
        reset_tokens();
    }

    /// A tool-only turn has no first token, but the provider may still have
    /// timed it — and either way it must advance the prompt-size chain.
    #[test]
    fn an_unmeasurable_turn_still_advances_the_chain() {
        let _guard = TEST_LOCK.lock();
        reset_tokens();
        let second = Duration::from_secs(1);
        record_turn(&usage(1000, 10, NONE), Some((second, second)));
        record_turn(&usage(1500, 20, NONE), None); // tool call, no deltas
        record_turn(&usage(1700, 10, NONE), Some((second, second)));
        // Only 200 tokens are new at the third turn, not 700.
        assert_eq!(rates().0, Some(600.0)); // (1000 + 200) over 2s
        reset_tokens();
    }

    /// A compacted context is not a shared prefix any more.
    #[test]
    fn a_shrinking_prompt_counts_in_full_again() {
        let _guard = TEST_LOCK.lock();
        reset_tokens();
        let second = Duration::from_secs(1);
        record_turn(&usage(1000, 1, NONE), Some((second, second)));
        record_turn(&usage(400, 1, NONE), Some((second, second)));
        // The shrunk turn adds no prefill work; the rate still reflects turn 1.
        assert_eq!(rates().0, Some(1000.0));
        reset_tokens();
    }

    #[test]
    fn fast_rates_drop_the_decimal() {
        assert_eq!(fmt_rate(18.42), "18.4");
        assert_eq!(fmt_rate(99.9), "99.9");
        assert_eq!(fmt_rate(412.6), "413");
    }
}
