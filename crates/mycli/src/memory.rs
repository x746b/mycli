//! Explicit global Markdown memory, separate from session transcripts and skills.
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};

pub fn path() -> PathBuf {
    crate::config::global_config_dir().join("MEMORY.md")
}
pub fn topics() -> PathBuf {
    crate::config::global_config_dir().join("memory")
}
pub fn initialize() -> Result<()> {
    fs::create_dir_all(crate::config::global_config_dir())?;
    if !topics().exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(topics())?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path()) {
        Ok(mut file) => {
            file.write_all(b"# Global memory\n")?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
pub fn read_bounded(limit: usize) -> Result<String> {
    let mut bytes = Vec::new();
    match fs::File::open(path()) {
        Ok(file) => {
            file.take((limit + 4) as u64).read_to_end(&mut bytes)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error.into()),
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = text[..end].lines().take(200).collect::<Vec<_>>().join("\n");
    if bytes.len() > end || text.lines().count() > 200 {
        result.push_str("\n[Memory excerpt truncated; read MEMORY.md for more.]");
    }
    Ok(result)
}
pub fn prompt(window: u64, max_output: u32) -> String {
    let limit = (window.saturating_sub(u64::from(max_output) + 1024) / 8).min(8192) as usize;
    let text =
        read_bounded(limit).unwrap_or_else(|e| format!("[Unable to read global memory: {e}]"));
    format!("\n# Global Memory\nFile: {}\nTopic files: {}\nThis is user-wide durable memory. Apply project-specific notes only to the named project. When asked to remember a fact, update this file using ordinary file tools. Read linked topics only when relevant.\n{text}\n", path().display(), topics().display())
}
pub fn remember(fact: &str) -> Result<bool> {
    let fact = fact.trim();
    if fact.is_empty() {
        bail!("Usage: /remember <durable fact>");
    }
    initialize()?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
    }
    let lock = options.open(crate::config::global_config_dir().join(".memory.lock"))?;
    lock.lock_exclusive()?;
    let original = fs::read_to_string(path()).context("Cannot read MEMORY.md")?;
    if original.len() > 1_048_576 {
        bail!("MEMORY.md exceeds 1 MiB; organize it into topic files before appending");
    }
    let line = format!("- {}", fact.replace('\n', "\n  "));
    if format!("\n{}\n", original.trim_end()).contains(&format!("\n{line}\n")) {
        return Ok(false);
    }
    let content = format!("{}\n{}\n", original.trim_end(), line);
    let mut temporary = tempfile::NamedTempFile::new_in(crate::config::global_config_dir())?;
    temporary.write_all(content.as_bytes())?;
    temporary.as_file().sync_all()?;
    if fs::read_to_string(path())? != original {
        bail!("MEMORY.md changed during the update; retry /remember");
    }
    temporary.persist(path()).map_err(|e| e.error)?;
    Ok(true)
}
