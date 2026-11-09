//! Restore points: what each pane was doing, saved so a restarted daemon can bring it
//! back as it was.
//!
//! A daemon restart (or a reboot, or a crash) ends every pane's process. Layouts are in
//! the workspace document; this module keeps the rest, one small JSON file per pane in
//! `<home>/restore/`: the terminal size, the working directory, the agent that was
//! running and which conversation it had open, or else the command running in the
//! foreground, and the recent output. The service
//! writes them periodically and on shutdown (`service/restore.rs`), and reads them when
//! it starts the panes again. They're separate from the workspace document because they
//! change all the time and no client needs them.

pub(crate) mod sessions;

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use urushi_core::{Agent, AgentSessionId, PaneId};

/// Lines of output kept per pane.
pub(crate) const SCREEN_LINES: usize = 2_000;

/// What a pane was doing when it was last saved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneRecord {
    /// Terminal columns.
    pub cols: u16,
    /// Terminal rows.
    pub rows: u16,
    /// The shell's working directory, if it could be read.
    pub cwd: Option<PathBuf>,
    /// The agent running in the pane, if any.
    pub agent: Option<AgentRecord>,
    /// The job that was running in the foreground, when it wasn't an agent: each
    /// stage's exact arguments (several for a pipeline). Empty at the prompt.
    pub job: Vec<Vec<String>>,
    /// The pane's recent output as terminal bytes (`urushi_mux::replay::to_ansi`), to
    /// show above the new shell. Empty when there's nothing worth replaying.
    pub screen: String,
}

/// An agent running in a pane, and the conversation it had open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// Which agent.
    pub agent: Agent,
    /// Its conversation, as far as the agent tells.
    #[serde(default)]
    pub conversation: Conversation,
}

/// The conversation an agent had open, as far as the daemon can tell.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conversation {
    /// The agent doesn't say (Codex, Cursor): resume its latest conversation in the
    /// pane's directory.
    #[default]
    Unknown,
    /// Nothing was said yet, so the agent saved nothing: start it afresh.
    Empty,
    /// This saved conversation: resume it.
    Saved(AgentSessionId),
}

/// How a restored pane starts its agent again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStart<'a> {
    /// As a new agent, like a new tab does.
    Fresh,
    /// Resuming this conversation, or with `None` the latest one in the directory.
    Resume(Option<&'a AgentSessionId>),
}

impl AgentRecord {
    /// How to start the agent again in a restored pane.
    pub fn start(&self) -> AgentStart<'_> {
        match &self.conversation {
            Conversation::Unknown => AgentStart::Resume(None),
            Conversation::Empty => AgentStart::Fresh,
            Conversation::Saved(id) => AgentStart::Resume(Some(id)),
        }
    }
}

/// The restore points on disk: `<dir>/<pane id>.json`.
#[derive(Clone, Debug)]
pub struct RestoreStore {
    dir: PathBuf,
}

impl RestoreStore {
    /// Restore points kept in `dir` (normally `<home>/restore`).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, pane: PaneId) -> PathBuf {
        self.dir.join(format!("{pane}.json"))
    }

    /// The saved state of `pane`. Missing or unreadable files give `None`: the pane
    /// then starts fresh, as before restore points existed.
    pub fn load(&self, pane: PaneId) -> Option<PaneRecord> {
        let bytes = std::fs::read(self.path(pane)).ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(record) => Some(record),
            Err(e) => {
                tracing::warn!(%pane, error = %e, "ignoring an unreadable restore point");
                None
            }
        }
    }

    /// Saves `record` for `pane`, replacing the old one atomically.
    pub fn save(&self, pane: PaneId, record: &PaneRecord) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_vec(record).map_err(std::io::Error::other)?;
        crate::store::write_atomically(&self.path(pane), &json)
    }

    /// Deletes the restore points of panes not in `keep` (closed panes, deleted
    /// threads), so their output doesn't linger on disk.
    pub fn retain(&self, keep: &HashSet<PaneId>) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let pane = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<PaneId>().ok());
            let stale = match pane {
                Some(pane) => !keep.contains(&pane),
                // Leftover temp files from an interrupted save.
                None => path.extension().is_some_and(|e| e == "tmp"),
            };
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(n: u128) -> PaneId {
        PaneId::from_uuid(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn records_are_saved_loaded_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let store = RestoreStore::new(dir.path().join("restore"));
        assert_eq!(store.load(pane(1)), None, "nothing saved yet");
        let record = PaneRecord {
            cols: 100,
            rows: 30,
            cwd: Some("/tmp/x".into()),
            agent: Some(AgentRecord {
                agent: Agent::Claude,
                conversation: Conversation::Saved(AgentSessionId::new("abc-1").unwrap()),
            }),
            job: vec![vec!["vim".into(), "my notes.md".into()]],
            screen: "\x1b[0;31mred\x1b[0m\r\n".into(),
        };
        store.save(pane(1), &record).unwrap();
        store.save(pane(2), &PaneRecord::default()).unwrap();
        assert_eq!(store.load(pane(1)), Some(record));
        std::fs::write(dir.path().join("restore/x.json.tmp"), "half").unwrap();
        store.retain(&HashSet::from([pane(2)]));
        assert_eq!(store.load(pane(1)), None, "closed pane forgotten");
        assert!(store.load(pane(2)).is_some());
        let left: Vec<_> = std::fs::read_dir(dir.path().join("restore"))
            .unwrap()
            .collect();
        assert_eq!(left.len(), 1, "temp file removed too");
        // A corrupt file is ignored rather than failing the restore.
        std::fs::write(store.path(pane(3)), "{not json").unwrap();
        assert_eq!(store.load(pane(3)), None);
    }

    #[test]
    fn agents_resume_saved_conversations_and_start_empty_ones_afresh() {
        let record = |conversation| AgentRecord {
            agent: Agent::Claude,
            conversation,
        };
        let id = AgentSessionId::new("s").unwrap();
        assert_eq!(
            record(Conversation::Saved(id.clone())).start(),
            AgentStart::Resume(Some(&id))
        );
        assert_eq!(record(Conversation::Empty).start(), AgentStart::Fresh);
        assert_eq!(
            record(Conversation::Unknown).start(),
            AgentStart::Resume(None)
        );
        // Restore points written without a conversation resume the latest one.
        let old: AgentRecord = serde_json::from_str(r#"{"agent":"codex"}"#).unwrap();
        assert_eq!(old.start(), AgentStart::Resume(None));
    }
}
