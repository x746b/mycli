//! Bounded command capture and UTF-8-safe excerpts for model input.
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::LazyLock;
use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub const MODEL_OUTPUT_BYTES: usize = 16 * 1024;
pub const ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
// At most 256 MiB on disk per process, removed on eviction or CLI shutdown.
static ARCHIVES: LazyLock<Mutex<VecDeque<tempfile::TempPath>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// A hard byte ceiling, including the marker. Retain context at both ends.
pub fn excerpt(text: &str, limit: usize) -> String {
    if text.len() <= limit { return text.to_owned(); }
    let marker = "\n[... output truncated; request a narrower range ...]\n";
    if limit < marker.len() { return "[truncated]".chars().take(limit).collect(); }
    let remaining = limit - marker.len();
    let mut head = remaining / 2;
    while !text.is_char_boundary(head) { head -= 1; }
    let mut tail = text.len() - (remaining - remaining / 2);
    while !text.is_char_boundary(tail) { tail += 1; }
    format!("{}{}{}", &text[..head], marker, &text[tail..])
}

pub struct Capture {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    pub total: u64,
    file: tokio::fs::File,
    pub path: PathBuf,
    saved: usize,
}
impl Capture {
    pub fn new() -> std::io::Result<Self> {
        let temp = tempfile::Builder::new().prefix("mycli-output-").tempfile_in("/tmp")?;
        let (file, path) = temp.into_parts();
        let name = path.to_path_buf();
        let mut archives = ARCHIVES.lock();
        archives.push_back(path);
        while archives.len() > 16 { archives.pop_front(); }
        Ok(Self { head: Vec::new(), tail: VecDeque::new(), total: 0,
            file: tokio::fs::File::from_std(file), path: name, saved: 0 })
    }
    pub async fn drain(&mut self, mut reader: impl AsyncRead + Unpin) -> std::io::Result<()> {
        let mut buf = [0u8; 8192];
        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 { break; }
            self.total += n as u64;
            let save = n.min(ARCHIVE_BYTES.saturating_sub(self.saved));
            if save > 0 {
                self.file.write_all(&buf[..save]).await?;
                self.saved += save;
            }
            let head = n.min((MODEL_OUTPUT_BYTES / 2).saturating_sub(self.head.len()));
            self.head.extend_from_slice(&buf[..head]);
            self.tail.extend(&buf[head..n]);
            let excess = self.tail.len().saturating_sub(MODEL_OUTPUT_BYTES / 2);
            self.tail.drain(..excess);
        }
        self.file.flush().await
    }
    pub fn text(&self) -> String {
        let tail: Vec<u8> = self.tail.iter().copied().collect();
        if self.total <= MODEL_OUTPUT_BYTES as u64 {
            let mut bytes = self.head.clone(); bytes.extend(tail);
            return String::from_utf8_lossy(&bytes).into_owned();
        }
        format!("{}\n[... {} bytes omitted; captured output: {}{} ...]\n{}",
            String::from_utf8_lossy(&self.head),
            self.total.saturating_sub((self.head.len() + tail.len()) as u64),
            self.path.display(),
            if self.total > ARCHIVE_BYTES as u64 { "; archive limited to first 16 MiB" } else { "" },
            String::from_utf8_lossy(&tail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excerpt_handles_unicode_and_tiny_budgets() {
        let text = format!("HEAD{}TAIL", "🦀漢字".repeat(10000));
        for limit in [0, 1, 10, 100, 1001, MODEL_OUTPUT_BYTES] {
            let result = excerpt(&text, limit);
            assert!(result.len() <= limit);
            if limit > 100 { assert!(result.starts_with("HEAD") && result.ends_with("TAIL")); }
        }
    }
    #[tokio::test]
    async fn drains_large_stream_with_bounded_capture() {
        let mut capture = Capture::new().unwrap();
        let bytes = vec![b'x'; ARCHIVE_BYTES + 12345];
        capture.drain(bytes.as_slice()).await.unwrap();
        assert_eq!(capture.total, bytes.len() as u64);
        assert!(capture.text().len() < MODEL_OUTPUT_BYTES + 300);
        assert_eq!(std::fs::metadata(&capture.path).unwrap().len(), ARCHIVE_BYTES as u64);
        assert!(capture.text().contains("archive limited"));
    }
    #[tokio::test]
    async fn small_output_is_exact() {
        let mut capture = Capture::new().unwrap();
        capture.drain("hello 🦀\n".as_bytes()).await.unwrap();
        assert_eq!(capture.text(), "hello 🦀\n");
    }
}

/// Scope guard for a CLI lifetime. Static storage itself is not dropped at exit.
pub struct ArchiveCleanup;
impl Drop for ArchiveCleanup {
    fn drop(&mut self) { ARCHIVES.lock().clear(); }
}
