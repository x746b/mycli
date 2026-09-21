//! Append-only session journal plus atomic active-context checkpoint.
use crate::config::Config;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use cersei_memory::Memory;
use cersei_types::{MemoryEntry, Message, SessionInfo, Usage};
use fs2::FileExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub cwd: PathBuf,
    pub model: String,
    pub provider: String,
    pub persona: String,
    pub tool_tier: String,
    pub reasoning_effort: Option<String>,
    pub thinking: Option<bool>,
    #[serde(default)]
    pub show_thinking: bool,
    pub max_tokens: u32,
    pub context_window: u64,
}
fn absolute_directory(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        }
    })
}

impl Metadata {
    pub fn new(config: &Config, model: &str, window: u64) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Untitled session".into(),
            created_at: now.clone(),
            updated_at: now,
            cwd: absolute_directory(&config.working_dir),
            model: model.into(),
            provider: config.provider.clone(),
            persona: config.persona.clone(),
            tool_tier: config.tool_tier.clone(),
            reasoning_effort: config.reasoning_effort.clone(),
            thinking: config.thinking,
            show_thinking: config.show_thinking,
            max_tokens: config.max_tokens,
            context_window: window,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub sequence: u64,
    pub metadata: Metadata,
    pub messages: Vec<Message>,
    pub usage: Usage,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    version: u32,
    sequence: u64,
    /// Append extends the active context; checkpoint replaces it (e.g. compaction).
    kind: String,
    metadata: Metadata,
    messages: Vec<Message>,
    usage: Usage,
}
struct State {
    file: File, // An exclusive advisory lock is held until every handle is dropped.
    snapshot: Snapshot,
    deleted: bool,
}
impl Drop for State {
    fn drop(&mut self) {
        // Release explicitly: a concurrently spawned child can briefly inherit
        // the file description until exec, even though the parent has closed it.
        let _ = FileExt::unlock(&self.file);
    }
}

#[derive(Clone)]
pub struct Journal {
    dir: PathBuf,
    state: Arc<Mutex<State>>,
}

pub fn root() -> PathBuf {
    crate::config::global_config_dir().join("sessions")
}
fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn validate_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id)
        .map(|u| u.to_string())
        .ok()
        .as_deref()
        != Some(id)
    {
        bail!("Invalid session ID");
    }
    Ok(())
}
fn open_log(path: &Path, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).append(true).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(nix::libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    file.try_lock_exclusive()
        .context("Session is already open in another mycli process")?;
    Ok(file)
}
fn write_context(dir: &Path, snapshot: &Snapshot) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut temp, snapshot)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(dir.join("context.json"))
        .map_err(|e| e.error)?;
    File::open(dir)?.sync_all()?;
    Ok(())
}
impl Journal {
    pub fn create(metadata: Metadata) -> Result<Self> {
        Self::create_in(&root(), metadata)
    }
    fn create_in(root: &Path, metadata: Metadata) -> Result<Self> {
        validate_id(&metadata.id)?;
        private_dir(root)?;
        let dir = root.join(&metadata.id);
        fs::create_dir(&dir)?;
        private_dir(&dir)?;
        let file = open_log(&dir.join("transcript.jsonl"), true)?;
        let snapshot = Snapshot {
            version: 1,
            sequence: 0,
            metadata,
            messages: vec![],
            usage: Usage::default(),
        };
        let journal = Self {
            dir,
            state: Arc::new(Mutex::new(State {
                file,
                snapshot,
                deleted: false,
            })),
        };
        journal.save_inner(None, &[], &Usage::default(), true)?;
        Ok(journal)
    }
    pub fn open(id: &str) -> Result<Self> {
        Self::open_in(&root(), id)
    }
    fn open_in(root: &Path, id: &str) -> Result<Self> {
        validate_id(id)?;
        let dir = root.join(id);
        let info = fs::symlink_metadata(&dir)?;
        if !info.is_dir() || info.file_type().is_symlink() {
            bail!("Invalid session directory");
        }
        let file = open_log(&dir.join("transcript.jsonl"), false)?;
        // Replay the journal: this also recovers a checkpoint interrupted after append.
        let mut reader = BufReader::new(File::open(dir.join("transcript.jsonl"))?);
        let mut snapshot: Option<Snapshot> = None;
        let mut offset = 0u64;
        loop {
            let mut line = Vec::new();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            if !line.ends_with(b"\n") {
                file.set_len(offset)?;
                file.sync_data()?;
                break;
            }
            let entry: Entry =
                serde_json::from_slice(&line).context("Corrupt session journal; not resumed")?;
            if entry.version != 1
                || entry.metadata.id != id
                || entry.sequence != snapshot.as_ref().map(|s| s.sequence + 1).unwrap_or(1)
            {
                bail!("Invalid session journal version, ID, or sequence");
            }
            let mut messages = snapshot.take().map(|s| s.messages).unwrap_or_default();
            match entry.kind.as_str() {
                "append" => messages.extend(entry.messages),
                "checkpoint" => messages = entry.messages,
                _ => bail!("Unknown session event"),
            }
            snapshot = Some(Snapshot {
                version: 1,
                sequence: entry.sequence,
                metadata: entry.metadata,
                messages,
                usage: entry.usage,
            });
            offset += read as u64;
        }
        let snapshot = snapshot.context("Empty session journal")?;
        write_context(&dir, &snapshot)?;
        Ok(Self {
            dir,
            state: Arc::new(Mutex::new(State {
                file,
                snapshot,
                deleted: false,
            })),
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().snapshot.clone()
    }
    pub fn path(&self) -> &Path {
        &self.dir
    }
    fn save_inner(
        &self,
        metadata: Option<Metadata>,
        messages: &[Message],
        usage: &Usage,
        force: bool,
    ) -> Result<()> {
        let mut state = self.state.lock();
        if state.deleted || !self.dir.is_dir() {
            bail!("Session has been deleted");
        }
        let old = serde_json::to_value(&state.snapshot.messages)?;
        let new = serde_json::to_value(messages)?;
        let same_usage =
            serde_json::to_value(usage)? == serde_json::to_value(&state.snapshot.usage)?;
        if !force
            && metadata
                .as_ref()
                .map(|m| m == &state.snapshot.metadata)
                .unwrap_or(true)
            && old == new
            && same_usage
        {
            return Ok(());
        }
        let previous = old.as_array().unwrap();
        let next = new.as_array().unwrap();
        let append = next.starts_with(previous);
        let event_messages = if append {
            messages[previous.len()..].to_vec()
        } else {
            messages.to_vec()
        };
        let mut metadata = metadata.unwrap_or_else(|| state.snapshot.metadata.clone());
        metadata.updated_at = chrono::Utc::now().to_rfc3339();
        let sequence = state.snapshot.sequence + 1;
        let entry = Entry {
            version: 1,
            sequence,
            kind: if append { "append" } else { "checkpoint" }.into(),
            metadata: metadata.clone(),
            messages: event_messages,
            usage: usage.clone(),
        };
        let mut bytes = serde_json::to_vec(&entry)?;
        bytes.push(b'\n');
        let previous_len = state.file.metadata()?.len();
        if let Err(error) = state
            .file
            .write_all(&bytes)
            .and_then(|_| state.file.sync_data())
        {
            let _ = state.file.set_len(previous_len);
            return Err(error.into());
        }
        // The durable journal is authoritative even if atomic snapshot replacement fails.
        state.snapshot = Snapshot {
            version: 1,
            sequence,
            metadata,
            messages: messages.to_vec(),
            usage: usage.clone(),
        };
        if let Err(error) = write_context(&self.dir, &state.snapshot) {
            eprintln!("Warning: session journal saved, but context checkpoint needs recovery on resume: {error}");
        }
        Ok(())
    }
    pub fn sync(&self, agent: &cersei::Agent, config: &Config, model: &str) -> Result<()> {
        let mut meta = self.snapshot().metadata;
        meta.model = model.into();
        meta.provider = config.provider.clone();
        meta.cwd = absolute_directory(&config.working_dir);
        meta.persona = config.persona.clone();
        meta.tool_tier = config.tool_tier.clone();
        meta.reasoning_effort = config.reasoning_effort.clone();
        meta.thinking = agent.thinking_enabled();
        meta.show_thinking = crate::render::thinking_visible();
        meta.max_tokens = config.max_tokens;
        meta.context_window = agent.context_window();
        self.save_inner(Some(meta), &agent.messages(), &agent.usage(), false)
    }
    pub fn attach(&self, agent: &mut cersei::Agent, restore: bool) {
        let snapshot = self.snapshot();
        agent.attach_session(
            Arc::new(self.clone()),
            snapshot.metadata.id,
            if restore { snapshot.messages } else { vec![] },
            snapshot.usage,
        );
    }
    pub fn rename(&self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            bail!("Use a name of 1–120 characters without control characters");
        }
        let snapshot = self.snapshot();
        let mut meta = snapshot.metadata;
        meta.name = name.into();
        self.save_inner(Some(meta), &snapshot.messages, &snapshot.usage, false)
    }
    pub fn delete(&self) -> Result<()> {
        let mut state = self.state.lock();
        // Only this UUID directory is removed, while its exclusive lock is held.
        validate_id(&state.snapshot.metadata.id)?;
        fs::remove_dir_all(&self.dir)?;
        state.deleted = true;
        Ok(())
    }
}
#[async_trait]
impl Memory for Journal {
    async fn store(&self, id: &str, messages: &[Message]) -> cersei_types::Result<()> {
        let usage = self.snapshot().usage;
        self.checkpoint(id, messages, &usage).await
    }
    async fn checkpoint(
        &self,
        id: &str,
        messages: &[Message],
        usage: &Usage,
    ) -> cersei_types::Result<()> {
        if self.snapshot().metadata.id != id {
            return Err(cersei_types::CerseiError::Config(
                "Session ID mismatch".into(),
            ));
        }
        self.save_inner(None, messages, usage, false)
            .map_err(cersei_types::CerseiError::Other)
    }
    async fn load(&self, id: &str) -> cersei_types::Result<Vec<Message>> {
        let snapshot = self.snapshot();
        if snapshot.metadata.id != id {
            return Err(cersei_types::CerseiError::Config(
                "Session ID mismatch".into(),
            ));
        }
        Ok(snapshot.messages)
    }
    async fn search(&self, _: &str, _: usize) -> cersei_types::Result<Vec<MemoryEntry>> {
        Ok(vec![])
    }
    async fn sessions(&self) -> cersei_types::Result<Vec<SessionInfo>> {
        let state = self.state.lock();
        if state.deleted {
            return Ok(vec![]);
        }
        let meta = &state.snapshot.metadata;
        Ok(vec![SessionInfo {
            id: meta.id.clone(),
            created_at: chrono::DateTime::parse_from_rfc3339(&meta.created_at)
                .map(|date| date.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            message_count: state.snapshot.messages.len(),
            model: Some(meta.model.clone()),
        }])
    }
    async fn delete(&self, id: &str) -> cersei_types::Result<()> {
        if self.snapshot().metadata.id != id {
            return Err(cersei_types::CerseiError::Config(
                "Session ID mismatch".into(),
            ));
        }
        Journal::delete(self).map_err(cersei_types::CerseiError::Other)
    }
}

pub struct Listing {
    pub metadata: Metadata,
    pub message_count: usize,
}

pub fn list() -> Result<Vec<Listing>> {
    let mut entries = Vec::new();
    if !root().exists() {
        return Ok(entries);
    }
    for entry in fs::read_dir(root())? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if validate_id(&id).is_err() {
            continue;
        }
        let path = entry.path().join("context.json");
        if fs::symlink_metadata(&path)
            .map(|s| s.file_type().is_symlink())
            .unwrap_or(false)
        {
            continue;
        }
        let snapshot = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Snapshot>(&bytes).ok())
            .filter(|s| s.version == 1 && s.metadata.id == id)
            .or_else(|| Journal::open_in(&root(), &id).ok().map(|j| j.snapshot()));
        if let Some(snapshot) = snapshot {
            entries.push(Listing {
                message_count: snapshot.messages.len(),
                metadata: snapshot.metadata,
            });
        }
    }
    entries.sort_by(|a, b| b.metadata.updated_at.cmp(&a.metadata.updated_at));
    Ok(entries)
}
pub fn resolve(selector: &str) -> Result<String> {
    if validate_id(selector).is_ok() {
        return Ok(selector.into());
    }
    let entries = list()?;
    if selector == "latest" {
        return entries
            .first()
            .map(|s| s.metadata.id.clone())
            .context("No saved sessions");
    }
    let matches: Vec<_> = entries
        .iter()
        .filter(|s| {
            s.metadata.name.eq_ignore_ascii_case(selector) || s.metadata.id.starts_with(selector)
        })
        .collect();
    if matches.len() != 1 {
        bail!(
            "Session selection matches {} entries; use a full session ID",
            matches.len()
        );
    }
    Ok(matches[0].metadata.id.clone())
}

pub fn resume_config(meta: &Metadata) -> Result<Config> {
    let fresh = crate::config::load_for_dir(&meta.cwd);
    if !meta.cwd.is_dir() {
        bail!(
            "Session project directory no longer exists: {}",
            meta.cwd.display()
        );
    }
    let mut config = if let Some(name) = meta.provider.strip_prefix("local:") {
        fresh.with_local_profile(name, &fresh)?
    } else if meta.provider == "omlx" {
        fresh.clone()
    } else {
        let cloud = fresh
            .resolve_cloud(&meta.provider)
            .context("Saved cloud profile is not configured")?;
        let mut config = fresh.clone();
        config.provider = cloud.name;
        config.base_url = cloud.base_url;
        config.api_key = cloud.api_key;
        config.context_window = cloud.context_window.unwrap_or(0);
        config.reasoning_levels = None;
        config.temperature = None;
        config.top_p = None;
        config.min_p = None;
        config.thinking = None;
        config
    };
    config.model = meta.model.clone();
    config.provider = meta.provider.clone();
    config.working_dir = meta.cwd.clone();
    config.persona = meta.persona.clone();
    config.tool_tier = meta.tool_tier.clone();
    config.reasoning_effort = meta.reasoning_effort.clone();
    config.thinking = meta.thinking;
    config.show_thinking = meta.show_thinking;
    config.max_tokens = config.max_tokens.min(meta.max_tokens);
    // Resolve the current serving window anew; an old checkpoint must not enlarge it.
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> Metadata {
        let config = Config {
            api_key: "DO-NOT-STORE-THIS-KEY".into(),
            ..Config::default()
        };
        Metadata::new(&config, "mock-model", 32768)
    }

    #[test]
    fn journal_retains_original_messages_after_compaction_and_rename() {
        let root = tempfile::tempdir().unwrap();
        let journal = Journal::create_in(root.path(), metadata()).unwrap();
        let id = journal.snapshot().metadata.id;
        let original = vec![
            Message::user("ORIGINAL history"),
            Message::assistant("answer"),
        ];
        journal
            .save_inner(None, &original, &Usage::default(), false)
            .unwrap();
        let before = fs::read(journal.path().join("transcript.jsonl")).unwrap();
        let compacted = vec![Message::user("compact summary")];
        journal
            .save_inner(
                None,
                &compacted,
                &Usage {
                    input_tokens: 100,
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        journal.rename("Named session α").unwrap();
        let after = fs::read(journal.path().join("transcript.jsonl")).unwrap();
        assert!(after.starts_with(&before));
        let text = String::from_utf8(after).unwrap();
        assert!(text.contains("ORIGINAL history") && text.contains("compact summary"));
        assert!(!text.contains("DO-NOT-STORE-THIS-KEY"));
        drop(journal);
        let reopened = Journal::open_in(root.path(), &id).unwrap();
        assert_eq!(reopened.snapshot().metadata.name, "Named session α");
        assert_eq!(reopened.snapshot().messages.len(), 1);
        assert_eq!(
            reopened.snapshot().messages[0].get_all_text(),
            "compact summary"
        );
        assert_eq!(reopened.snapshot().usage.input_tokens, 100);
    }

    #[test]
    fn exclusive_lock_and_torn_tail_recovery_preserve_committed_entries() {
        let root = tempfile::tempdir().unwrap();
        let journal = Journal::create_in(root.path(), metadata()).unwrap();
        let id = journal.snapshot().metadata.id;
        journal
            .save_inner(None, &[Message::user("saved")], &Usage::default(), false)
            .unwrap();
        assert!(Journal::open_in(root.path(), &id).is_err());
        let dir = journal.path().to_owned();
        let size = fs::metadata(dir.join("transcript.jsonl")).unwrap().len();
        drop(journal);
        OpenOptions::new()
            .append(true)
            .open(dir.join("transcript.jsonl"))
            .unwrap()
            .write_all(b"{\"partial\":")
            .unwrap();
        fs::remove_file(dir.join("context.json")).unwrap();
        let recovered = Journal::open_in(root.path(), &id).unwrap();
        assert_eq!(recovered.snapshot().messages[0].get_all_text(), "saved");
        assert_eq!(
            fs::metadata(dir.join("transcript.jsonl")).unwrap().len(),
            size
        );
        assert!(dir.join("context.json").is_file());
    }

    #[test]
    fn deletion_is_confined_to_the_active_uuid_directory() {
        let root = tempfile::tempdir().unwrap();
        let first = Journal::create_in(root.path(), metadata()).unwrap();
        let second = Journal::create_in(root.path(), metadata()).unwrap();
        first.delete().unwrap();
        assert!(!first.path().exists());
        assert!(second.path().join("context.json").is_file());
        assert!(Journal::open_in(root.path(), "../outside").is_err());
        assert!(first
            .save_inner(None, &[], &Usage::default(), false)
            .is_err());
        assert!(second.rename("\n").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(second.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(second.path().join("context.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(second.path().join("transcript.jsonl"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn complete_corrupt_records_fail_without_silently_deleting_history() {
        let root = tempfile::tempdir().unwrap();
        let journal = Journal::create_in(root.path(), metadata()).unwrap();
        let id = journal.snapshot().metadata.id;
        let path = journal.path().join("transcript.jsonl");
        drop(journal);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"not-json\n")
            .unwrap();
        let before = fs::read(&path).unwrap();
        assert!(Journal::open_in(root.path(), &id).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
