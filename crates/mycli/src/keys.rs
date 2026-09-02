//! Reading single keypresses while the model is working.
//!
//! Esc has to cancel a running turn, which means reading the keyboard *during*
//! streaming. The obvious route — crossterm's `enable_raw_mode` — is wrong
//! here for two reasons: raw mode clears `ISIG`, so Ctrl+C stops raising
//! SIGINT and the existing cancel handler goes dead, and it clears `OPOST`, so
//! every `\n` the renderer writes stops returning the carriage and the whole
//! transcript stair-steps down the screen.
//!
//! What is actually needed is only "deliver keys without waiting for Enter":
//! clear `ICANON` and `ECHO` and leave everything else — signals, output
//! processing — alone. That is a direct termios change, so it is done here
//! rather than through crossterm. crossterm still decodes the bytes.

use crate::render;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use nix::sys::termios::{self, LocalFlags, SetArg};

/// Nesting depth of [`enter`] calls, and the terminal settings to restore.
#[cfg(unix)]
static SAVED: Mutex<Option<(usize, termios::Termios)>> = Mutex::new(None);

/// A watcher is running, so [`park`] has someone to wait for.
static WATCHING: AtomicBool = AtomicBool::new(false);
/// Someone else wants the keyboard; the watcher should stop reading.
static PARK_REQUESTED: AtomicBool = AtomicBool::new(false);
/// The watcher has observed [`PARK_REQUESTED`] and is no longer reading.
static PARKED: AtomicBool = AtomicBool::new(false);

/// Characters typed while the model was working.
///
/// Reading the keyboard during a turn consumes those keystrokes, which would
/// otherwise have sat in the terminal buffer and appeared at the next prompt.
/// They are collected here and seeded back into the next input line, so typing
/// ahead still works.
static TYPEAHEAD: Mutex<String> = Mutex::new(String::new());

/// Longest run of type-ahead kept. A key held down should not grow without
/// bound.
const TYPEAHEAD_CAP: usize = 4096;

/// Take everything typed during the last turn, clearing the buffer.
pub fn take_typeahead() -> String {
    std::mem::take(&mut *TYPEAHEAD.lock())
}

pub fn stdin_is_tty() -> bool {
    #[cfg(unix)]
    {
        nix::unistd::isatty(0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Put the terminal in character-at-a-time mode. Reference counted, so nested
/// users (the watcher and an approval dialog) do not fight over it.
pub fn enter() {
    #[cfg(unix)]
    {
        if !stdin_is_tty() {
            return;
        }
        let mut saved = SAVED.lock();
        if let Some((depth, _)) = saved.as_mut() {
            *depth += 1;
            return;
        }
        let stdin = std::io::stdin();
        let Ok(original) = termios::tcgetattr(&stdin) else {
            return;
        };
        let mut raw = original.clone();
        // Only these two. ISIG stays on so Ctrl+C still raises SIGINT, and
        // OPOST stays on so "\n" still returns the carriage.
        raw.local_flags.remove(LocalFlags::ICANON | LocalFlags::ECHO);
        if termios::tcsetattr(&stdin, SetArg::TCSANOW, &raw).is_ok() {
            *saved = Some((1, original));
        }
    }
}

/// Undo one [`enter`]; the terminal is restored when the last one exits.
pub fn exit() {
    #[cfg(unix)]
    {
        let mut saved = SAVED.lock();
        let Some((depth, original)) = saved.as_mut() else {
            return;
        };
        if *depth > 1 {
            *depth -= 1;
            return;
        }
        let _ = termios::tcsetattr(&std::io::stdin(), SetArg::TCSANOW, original);
        *saved = None;
    }
}

/// Ask the watcher to stop reading, and wait until it has.
///
/// Used by the approval dialog, which needs the keyboard for itself. Bounded:
/// a missed handshake must not hang the prompt.
pub fn park() {
    if !WATCHING.load(Ordering::SeqCst) {
        return;
    }
    PARK_REQUESTED.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_millis(300);
    while !PARKED.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Hand the keyboard back to the watcher.
pub fn unpark() {
    PARK_REQUESTED.store(false, Ordering::SeqCst);
}

/// Watches the keyboard for the duration of one model turn.
///
/// Dropping it stops the thread and restores the terminal, so it cannot be
/// leaked by an early return or an error path.
pub struct KeyWatcher {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl KeyWatcher {
    /// Start watching. Returns `None` when there is no terminal to read.
    pub fn start(cancel: CancellationToken) -> Option<Self> {
        if !stdin_is_tty() {
            return None;
        }
        enter();
        let stop = Arc::new(AtomicBool::new(false));
        WATCHING.store(true, Ordering::SeqCst);
        PARK_REQUESTED.store(false, Ordering::SeqCst);
        PARKED.store(false, Ordering::SeqCst);

        let thread_stop = Arc::clone(&stop);
        let join = std::thread::spawn(move || watch(thread_stop, cancel));
        Some(Self {
            stop,
            join: Some(join),
        })
    }
}

impl Drop for KeyWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        WATCHING.store(false, Ordering::SeqCst);
        PARKED.store(false, Ordering::SeqCst);
        exit();
    }
}

fn watch(stop: Arc<AtomicBool>, cancel: CancellationToken) {
    use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

    let mut cancelled = false;
    while !stop.load(Ordering::SeqCst) {
        if PARK_REQUESTED.load(Ordering::SeqCst) {
            PARKED.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(3));
            continue;
        }
        PARKED.store(false, Ordering::SeqCst);

        // A short poll, so a park request or a finished turn is noticed
        // promptly rather than blocking on the next keystroke.
        match event::poll(Duration::from_millis(30)) {
            Ok(true) => {}
            _ => continue,
        }
        // The park flag may have been set while we were polling.
        if PARK_REQUESTED.load(Ordering::SeqCst) {
            continue;
        }
        let Ok(Event::Key(KeyEvent {
            code, modifiers, ..
        })) = event::read()
        else {
            continue;
        };

        match code {
            KeyCode::Esc if !cancelled => {
                cancelled = true;
                cancel.cancel();
                render::interrupt_notice();
            }
            KeyCode::Char('o') | KeyCode::Char('O')
                if modifiers.contains(KeyModifiers::CONTROL) =>
            {
                render::toggle_thinking();
                crate::status::draw();
            }
            KeyCode::Char(c)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let mut buf = TYPEAHEAD.lock();
                if buf.len() < TYPEAHEAD_CAP {
                    buf.push(c);
                }
            }
            KeyCode::Backspace => {
                TYPEAHEAD.lock().pop();
            }
            _ => {}
        }
    }
}
