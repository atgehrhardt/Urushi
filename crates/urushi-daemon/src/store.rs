//! Durable workspace state.
//!
//! The whole workspace (settings, repos, projects, threads, tabs, boards) is one document,
//! rewritten atomically: write a temp file, fsync, rename. It is written by one saver,
//! which covers a burst of changes with one write (`service/saver.rs`, ADR 0054). With
//! synced Jira boards it runs to a few hundred kilobytes, which this still handles with no
//! C dependency. The [`Store`] trait lets us move to an embedded database if that ever
//! changes (see ADR 0008).

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use urushi_core::{Board, BranchName, Project, QuickTerminal, Repo, Settings, Tab, Thread};

/// Current document format. Bump it and add a migration in [`JsonFileStore::load`] when
/// a change isn't backward-compatible.
pub const FORMAT_VERSION: u32 = 1;

/// A thread plus daemon-private facts that aren't part of the shared domain model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadRecord {
    /// The thread.
    pub thread: Thread,
    /// Local branches that existed when the thread was created. These are never owned
    /// by it, so never deleted with it.
    #[serde(default)]
    pub preexisting_branches: BTreeSet<BranchName>,
}

/// Everything the daemon persists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Format version of this document.
    pub format: u32,
    /// Global settings.
    #[serde(default)]
    pub settings: Settings,
    /// Repos, in sidebar order. Stored as `projects` before repos were renamed, so that
    /// key is still read (ADR 0010).
    #[serde(default, alias = "projects")]
    pub repos: Vec<Repo>,
    /// Projects (groups of threads), in sidebar order. Stored as `folders`, since the old
    /// `projects` key holds repos in older files.
    #[serde(default, rename = "folders")]
    pub projects: Vec<Project>,
    /// Threads, oldest first.
    #[serde(default)]
    pub threads: Vec<ThreadRecord>,
    /// All tabs of all threads.
    #[serde(default)]
    pub tabs: Vec<Tab>,
    /// Task boards, in tab order. Older files have none.
    #[serde(default)]
    pub boards: Vec<Board>,
    /// The quick terminal, if one is open (ADR 0031). Older files have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_terminal: Option<QuickTerminal>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            settings: Settings::default(),
            repos: Vec::new(),
            projects: Vec::new(),
            threads: Vec::new(),
            tabs: Vec::new(),
            boards: Vec::new(),
            quick_terminal: None,
        }
    }
}

/// Storage failures.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Reading or writing failed.
    #[error("store i/o error at {path}: {source}")]
    Io {
        /// File involved.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
    },
    /// The file exists but isn't a valid document.
    #[error("store file {path} is corrupt: {source}")]
    Corrupt {
        /// File involved.
        path: PathBuf,
        /// Cause.
        source: serde_json::Error,
    },
    /// The file was written by a newer Urushi.
    #[error("store format {found} is newer than supported format {FORMAT_VERSION}; upgrade Urushi")]
    TooNew {
        /// Version found on disk.
        found: u32,
    },
}

/// Loads and saves the workspace document.
pub trait Store: Send + Sync {
    /// Loads the document. A missing store yields the default document.
    fn load(&self) -> Result<Document, StoreError>;
    /// Replaces the stored document atomically.
    fn save(&self, document: &Document) -> Result<(), StoreError>;
}

/// [`Store`] backed by a JSON file.
#[derive(Clone, Debug)]
pub struct JsonFileStore {
    path: PathBuf,
}

impl JsonFileStore {
    /// A store at `path` (normally `<home>/workspace.json`).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn io(&self, source: std::io::Error) -> StoreError {
        StoreError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

impl Store for JsonFileStore {
    fn load(&self) -> Result<Document, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Document::default()),
            Err(e) => return Err(self.io(e)),
        };
        let document: Document =
            serde_json::from_slice(&bytes).map_err(|source| StoreError::Corrupt {
                path: self.path.clone(),
                source,
            })?;
        if document.format > FORMAT_VERSION {
            return Err(StoreError::TooNew {
                found: document.format,
            });
        }
        Ok(document)
    }

    fn save(&self, document: &Document) -> Result<(), StoreError> {
        let json = serde_json::to_vec_pretty(document).map_err(|source| StoreError::Corrupt {
            path: self.path.clone(),
            source,
        })?;
        write_atomically(&self.path, &json).map_err(|e| self.io(e))
    }
}

/// Replaces the file at `path` with `bytes`: write a temp file next to it, fsync, rename.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_default_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonFileStore::new(dir.path().join("workspace.json"));
        let mut doc = store.load().unwrap();
        assert_eq!(doc, Document::default());
        doc.settings.delete_remote_branches = false;
        store.save(&doc).unwrap();
        assert_eq!(store.load().unwrap(), doc);
        assert!(!dir.path().join("workspace.json.tmp").exists());
    }

    #[test]
    fn files_from_before_the_repo_rename_still_load() {
        // `projects` held repos, and threads named their repo `project`.
        let old = r#"{
            "format": 1,
            "projects": [{"id": "00000000-0000-0000-0000-000000000001", "name": "r",
                          "repo_root": "/r", "default_base": null,
                          "added_at": 0}],
            "threads": []
        }"#;
        let doc: Document = serde_json::from_str(old).unwrap();
        assert_eq!(doc.repos.len(), 1);
        assert_eq!(doc.repos[0].name, "r");
        assert!(doc.projects.is_empty());

        // Saved again, repos and projects get their own keys.
        let json = serde_json::to_string(&doc).unwrap();
        assert!(
            json.contains("\"repos\"") && json.contains("\"folders\""),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), doc);
    }

    #[test]
    fn corrupt_and_future_files_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.json");
        let store = JsonFileStore::new(&path);
        fs::write(&path, b"{not json").unwrap();
        assert!(matches!(store.load(), Err(StoreError::Corrupt { .. })));
        fs::write(&path, br#"{"format": 99}"#).unwrap();
        assert!(matches!(
            store.load(),
            Err(StoreError::TooNew { found: 99 })
        ));
    }
}
