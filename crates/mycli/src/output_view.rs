//! Human-only output history. Model messages and budgets are never changed.
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{LazyLock, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use parking_lot::Mutex;
use crossterm::{cursor, event::{self, Event, KeyCode, KeyModifiers}, execute, terminal};
use serde_json::Value;

const HISTORY: usize = 32;
#[derive(Clone)]
struct Output {
    name: String,
    preview: String,
    paths: Vec<PathBuf>,
    capped: bool,
    is_error: bool,
    duration: Duration,
}
static OUTPUTS: LazyLock<Mutex<VecDeque<Output>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));
static REQUESTED: AtomicBool = AtomicBool::new(false);
pub fn request() { REQUESTED.store(true, Ordering::Relaxed); }
pub fn take_requested() -> bool { REQUESTED.swap(false, Ordering::Relaxed) }

pub fn record(name: &str, result: &str, metadata: Option<&Value>, is_error: bool, duration: Duration) {
    let paths = metadata.and_then(|m| m.get("output_files")).and_then(Value::as_array)
        .map(|a| a.iter().take(2).filter_map(Value::as_str).map(PathBuf::from).collect()).unwrap_or_default();
    let capped = metadata.and_then(|m| m.get("archive_truncated")).and_then(Value::as_bool).unwrap_or(false);
    let mut outputs = OUTPUTS.lock();
    outputs.push_back(Output { name: safe_text(name), preview: result.into(), paths, capped, is_error, duration });
    while outputs.len() > HISTORY { outputs.pop_front(); }
}

/// Strip escape sequences and replace remaining control bytes; captured output
/// is data, even if it contains clear-screen or clipboard-control sequences.
fn safe_text(text: &str) -> String {
    crate::ui::strip_ansi(text).chars().map(|c| {
        if c.is_control() && c != '\n' && c != '\t' { '?' } else { c }
    }).collect()
}
fn load(output: &Output) -> String {
    if output.paths.is_empty() { return safe_text(&output.preview); }
    let mut text = String::new();
    if output.capped { text.push_str("[Capture limit reached: archives contain only the first 16 MiB per stream.]\n"); }
    for (index, path) in output.paths.iter().enumerate() {
        let label = if output.paths.len() == 2 { if index == 0 { "stdout" } else { "stderr" } } else { "output" };
        text.push_str(&format!("--- {label} ---\n"));
        // Bounded reads also protect against a replaced or modified archive.
        let read = (|| -> io::Result<Vec<u8>> {
            let file = std::fs::File::open(path)?;
            if !file.metadata()?.is_file() { return Err(io::Error::other("not a regular file")); }
            let mut bytes = Vec::new();
            file.take(cersei_tools::output::ARCHIVE_BYTES as u64).read_to_end(&mut bytes)?;
            Ok(bytes)
        })();
        match read {
            Ok(bytes) => {
                text.push_str(&safe_text(&String::from_utf8_lossy(&bytes)));
                if !text.ends_with('\n') { text.push('\n'); }
            }
            Err(_) => {
                text.push_str("[Archive unavailable or evicted; retained model excerpt follows.]\n");
                text.push_str(&safe_text(&output.preview));
                text.push('\n');
            }
        }
    }
    text
}

/// Extract terminal columns without allocating a second copy of a huge line.
fn columns(line: &str, left: usize, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut result = String::new();
    let mut column = 0;
    for c in line.chars() {
        let w = if c == '\t' { 4 - column % 4 } else { c.width().unwrap_or(0) };
        if column >= left.saturating_add(width) { break; }
        if column >= left && column + w <= left.saturating_add(width) {
            if c == '\t' { result.push_str(&" ".repeat(w)); } else { result.push(c); }
        } else if column + w > left && column < left.saturating_add(width) {
            result.push_str(&" ".repeat((column + w).min(left.saturating_add(width)) - column.max(left)));
        }
        column += w;
    }
    result
}

struct Screen;
impl Screen {
    fn enter() -> io::Result<Self> {
        crate::keys::enter();
        let mut stderr = io::stderr();
        // Construct the guard before terminal writes so failures restore state.
        let guard = Self;
        execute!(stderr, terminal::EnterAlternateScreen, cursor::Hide)?;
        write!(stderr, "\x1b[r")?;
        Ok(guard)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), terminal::LeaveAlternateScreen, cursor::Show);
        crate::keys::exit();
        crate::status::restore_after_viewer();
    }
}

/// Called only while the REPL owns the keyboard. During a model turn Ctrl+T
/// merely sets REQUESTED; the next prompt opens the viewer after the key watcher
/// and approval dialogs have released the terminal.
pub fn show() {
    if !crate::keys::stdin_is_tty() { return; }
    let outputs: Vec<Output> = OUTPUTS.lock().iter().cloned().collect();
    if let Err(error) = show_outputs(&outputs) {
        eprintln!("\r\n  Output viewer: {error}");
    }
}
fn show_outputs(outputs: &[Output]) -> io::Result<()> {
    let _screen = Screen::enter()?;
    let mut selected = outputs.len().saturating_sub(1);
    let mut text = outputs.get(selected).map(load).unwrap_or_else(|| "No completed tool output yet.".into());
    let mut line_count = text.lines().count().max(1);
    let mut top = 0usize;
    let mut left = 0usize;
    loop {
        let (width, height) = terminal::size().unwrap_or((80, 24));
        let width = width.saturating_sub(1) as usize;
        let height = height.saturating_sub(4).max(1) as usize;
        top = top.min(line_count.saturating_sub(height));
        let title = outputs.get(selected).map(|o| format!("{} · {} · {:.1}s · result {}/{}{}", o.name,
            if o.is_error { "error" } else { "success" }, o.duration.as_secs_f64(), selected + 1, outputs.len(),
            if o.capped { " · CAPTURE CAPPED" } else { "" })).unwrap_or_else(|| "Tool output".into());
        let mut frame = format!("\x1b[H\x1b[2J{}\r\n{}\r\n\r\n", columns(&title, 0, width),
            columns("↑↓ scroll · PgUp/PgDn · Home/End · ←→ pan · [ ] results · q/Esc/Ctrl+T close", 0, width));
        for line in text.lines().skip(top).take(height) {
            frame.push_str(&columns(line, left, width)); frame.push_str("\r\n");
        }
        frame.push_str(&format!("\x1b[{};1H{}", (height + 4).min(u16::MAX as usize),
            columns(&format!("Lines {}–{} / {} · column {}", top + 1, (top + height).min(line_count), line_count, left + 1), 0, width)));
        io::stderr().write_all(frame.as_bytes())?; io::stderr().flush()?;
        let Event::Key(key) = event::read()? else { continue; };
        if key.kind == event::KeyEventKind::Release { continue; }
        let previous = selected;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => break,
            KeyCode::Char('t' | 'T' | 'c' | 'd') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Up | KeyCode::Char('k') => top = top.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => top = top.saturating_add(1),
            KeyCode::PageUp => top = top.saturating_sub(height),
            KeyCode::PageDown | KeyCode::Char(' ') => top = top.saturating_add(height),
            KeyCode::Home | KeyCode::Char('g') => { top = 0; left = 0; }
            KeyCode::End | KeyCode::Char('G') => top = line_count.saturating_sub(height),
            KeyCode::Left | KeyCode::Char('h') => left = left.saturating_sub(8),
            KeyCode::Right | KeyCode::Char('l') => left = left.saturating_add(8),
            KeyCode::Char('[') => selected = selected.saturating_sub(1),
            KeyCode::Char(']') => selected = (selected + 1).min(outputs.len().saturating_sub(1)),
            _ => {}
        }
        if selected != previous {
            text = load(&outputs[selected]); line_count = text.lines().count().max(1); top = 0; left = 0;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(preview: &str) -> Output {
        Output { name: "Bash".into(), preview: preview.into(), paths: vec![], capped: false, is_error: false, duration: Duration::ZERO }
    }
    #[test]
    fn all_88_lines_survive_the_inline_preview_limit() {
        let text = (1..=88).map(|i| format!("line {i}\n")).collect::<String>();
        let loaded = load(&output(&text));
        assert_eq!(loaded.lines().count(), 88); assert!(loaded.contains("line 88\n"));
    }
    #[test]
    fn removes_terminal_controls_and_pans_unicode() {
        assert_eq!(safe_text("a\x1b[2Jb\x1b]52;c;bad\x07c\x00"), "abc?");
        assert_eq!(columns("ab漢字cd", 2, 4), "漢字");
        assert_eq!(columns("ab漢字cd", 6, 2), "cd");
        assert_eq!(columns("a\tb", 0, 5), "a   b");
    }
    #[tokio::test]
    async fn loads_archive_beyond_model_excerpt_and_reports_eviction() {
        let mut capture = cersei_tools::output::Capture::new().unwrap();
        let text = format!("head\n{}\ntail", "x".repeat(50000));
        capture.drain(text.as_bytes()).await.unwrap();
        let mut result = output("short excerpt"); result.paths = vec![capture.path.clone()];
        assert!(load(&result).contains(&text));
        std::fs::remove_file(&capture.path).unwrap();
        let missing = load(&result);
        assert!(missing.contains("evicted") && missing.contains("short excerpt"));
    }
}
