//! Minimal streaming terminal renderer.

use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::execute;
use std::io::{self, Write};
use std::time::Duration;

const DIM: Color = Color::DarkGrey;
const ACCENT: Color = Color::Cyan;
const SUCCESS: Color = Color::Green;
const ERROR: Color = Color::Red;
const TOOL_BADGE: Color = Color::Magenta;

pub struct Renderer {
    buffer: String,
    in_thinking: bool,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_thinking: false,
        }
    }

    pub fn push_text(&mut self, delta: &str) {
        if self.in_thinking {
            self.end_thinking();
        }
        self.buffer.push_str(delta);
        // Flush on newline boundaries
        if let Some(last_nl) = self.buffer.rfind('\n') {
            let to_flush = self.buffer[..=last_nl].to_string();
            self.buffer = self.buffer[last_nl + 1..].to_string();
            self.print_markdown(&to_flush);
        }
    }

    pub fn push_thinking(&mut self, _delta: &str) {
        if !self.in_thinking {
            self.in_thinking = true;
            let _ = execute!(
                io::stderr(),
                SetForegroundColor(DIM),
                SetAttribute(Attribute::Italic),
                Print("  thinking... "),
            );
        }
    }

    pub fn tool_start(&mut self, name: &str, input: &serde_json::Value) {
        self.flush();
        let summary = tool_summary(name, input);
        let _ = execute!(
            io::stderr(),
            Print("\n"),
            SetForegroundColor(TOOL_BADGE),
            SetAttribute(Attribute::Bold),
            Print(format!("  [{name}]")),
            ResetColor,
            SetForegroundColor(DIM),
            Print(format!(" {summary}")),
            ResetColor,
            Print("\n"),
        );
    }

    pub fn tool_end(&mut self, name: &str, result: &str, is_error: bool, duration: Duration) {
        let (color, icon) = if is_error { (ERROR, "x") } else { (SUCCESS, "+") };
        let ms = duration.as_millis();
        let _ = execute!(
            io::stderr(),
            SetForegroundColor(color),
            Print(format!("  {icon} {name}")),
            ResetColor,
            SetForegroundColor(DIM),
            Print(format!(" ({ms}ms)")),
            ResetColor,
        );
        if is_error {
            let preview: String = result.chars().take(200).collect();
            let _ = execute!(
                io::stderr(),
                Print("\n"),
                SetForegroundColor(ERROR),
                Print(format!("    {preview}")),
                ResetColor,
            );
        }
        let _ = execute!(io::stderr(), Print("\n"));
    }

    pub fn error(&mut self, msg: &str) {
        self.flush();
        let _ = execute!(
            io::stderr(),
            Print("\n"),
            SetForegroundColor(ERROR),
            SetAttribute(Attribute::Bold),
            Print("  Error: "),
            ResetColor,
            SetForegroundColor(ERROR),
            Print(msg),
            ResetColor,
            Print("\n"),
        );
    }

    pub fn flush(&mut self) {
        self.end_thinking();
        if !self.buffer.is_empty() {
            let remaining = std::mem::take(&mut self.buffer);
            self.print_markdown(&remaining);
        }
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
    }

    pub fn complete(&mut self) {
        self.flush();
        let _ = execute!(io::stdout(), Print("\n"));
    }

    fn end_thinking(&mut self) {
        if self.in_thinking {
            self.in_thinking = false;
            let _ = execute!(
                io::stderr(),
                ResetColor,
                SetAttribute(Attribute::Reset),
                Print("\n"),
            );
        }
    }

    fn print_markdown(&self, text: &str) {
        let mut skin = termimad::MadSkin::default();
        skin.code_block.set_fg(termimad::crossterm::style::Color::Cyan);
        skin.inline_code.set_fg(termimad::crossterm::style::Color::Cyan);
        let rendered = skin.term_text(text);
        print!("{rendered}");
        let _ = io::stdout().flush();
    }
}

/// Print banner on startup
pub fn banner(config: &crate::config::Config, model_display: &str) {
    let logo = r#"
                   _____ _     __ 
                  / ____| |   /_ |
  _ __ ___  _   _| |    | |    | |
 | '_ ` _ \| | | | |    | |    | |
 | | | | | | |_| | |____| |____| |
 |_| |_| |_|\__, |\_____|______|_|
             __/ |                
            |___/                       
             "#;

    let _ = execute!(
        io::stderr(),
        SetForegroundColor(ACCENT),
        SetAttribute(Attribute::Bold),
        Print(logo),
        ResetColor,
        Print("\n"),
        SetForegroundColor(DIM),
        Print(format!(
            "  {} | {} | tools:{} | max_turns:{}",
            config.provider,
            model_display,
            crate::config::resolve_tool_tier(config),
            config.max_turns,
        )),
        ResetColor,
        Print("\n"),
        SetForegroundColor(DIM),
        Print("  Type /help for commands, Ctrl+C to cancel, Ctrl+D to exit"),
        ResetColor,
        Print("\n\n"),
    );
}

fn tool_summary(name: &str, input: &serde_json::Value) -> String {
    let s = match name {
        "Bash" | "bash" => input.get("command").and_then(|v| v.as_str()).unwrap_or(""),
        "Read" | "file_read" | "Write" | "file_write" | "Edit" | "file_edit" => {
            input.get("file_path").and_then(|v| v.as_str()).unwrap_or("")
        }
        "Glob" | "glob" => input.get("pattern").and_then(|v| v.as_str()).unwrap_or(""),
        "Grep" | "grep" => input.get("pattern").and_then(|v| v.as_str()).unwrap_or(""),
        _ => return truncate(&serde_json::to_string(input).unwrap_or_default(), 80),
    };
    truncate(s, 80)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
