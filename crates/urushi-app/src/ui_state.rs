//! What the window showed, remembered across app launches: the selected thread and the
//! tab chosen in each thread. Reopening the app shows the same thread and tabs again
//! (the terminals themselves live in the daemon and are still running, or were
//! restored from their restore points).
//!
//! Which thread a window shows is the window's own business, not the workspace's, so it
//! stays in the app (ADR 0002). It's saved for the local daemon only, as
//! `<daemon home>/app-state` with one `thread <id>` or `tab <thread> <tab>` per line.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::ConnectTarget;
use crate::state::AppState;

/// The remembered selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiState {
    /// The thread shown.
    pub thread: Option<String>,
    /// The tab chosen in each thread.
    pub tabs: HashMap<String, String>,
}

impl UiState {
    /// Where it's kept for `target`; `None` for remote daemons.
    pub fn path(target: &ConnectTarget) -> Option<PathBuf> {
        match target {
            ConnectTarget::Local(home) => urushi_client::local_daemon_endpoint(home.as_deref())
                .ok()
                .map(|(home, _)| home.join("app-state")),
            ConnectTarget::Remote(_) | ConnectTarget::Ask => None,
        }
    }

    /// The selection in `state`.
    pub fn of(state: &AppState) -> Self {
        Self {
            thread: state.selected_thread.clone(),
            tabs: state.selected_tab.clone(),
        }
    }

    /// Selects what was remembered. Threads or tabs that no longer exist are ignored
    /// once the workspace arrives.
    pub fn apply(self, state: &mut AppState) {
        state.selected_thread = self.thread;
        state.selected_tab = self.tabs;
    }

    /// Reads a saved selection; missing or unreadable files give an empty one.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// Saves the selection (best effort: losing it only means showing the first thread).
    pub fn save(&self, path: &Path) {
        if let Err(e) = std::fs::write(path, self.to_text()) {
            tracing::debug!(error = %e, "couldn't remember the selected thread");
        }
    }

    fn parse(text: &str) -> Self {
        let mut state = Self::default();
        for line in text.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words[..] {
                ["thread", id] => state.thread = Some(id.to_owned()),
                ["tab", thread, tab] => {
                    state.tabs.insert(thread.to_owned(), tab.to_owned());
                }
                _ => {}
            }
        }
        state
    }

    fn to_text(&self) -> String {
        let mut lines: Vec<String> = self.thread.iter().map(|t| format!("thread {t}")).collect();
        let mut tabs: Vec<_> = self.tabs.iter().collect();
        tabs.sort();
        lines.extend(
            tabs.into_iter()
                .map(|(thread, tab)| format!("tab {thread} {tab}")),
        );
        lines.join("\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selection_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("urushi-ui-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app-state");
        assert_eq!(
            UiState::load(&path),
            UiState::default(),
            "nothing saved yet"
        );
        let state = UiState {
            thread: Some("t2".into()),
            tabs: HashMap::from([("t1".into(), "a".into()), ("t2".into(), "b".into())]),
        };
        state.save(&path);
        assert_eq!(UiState::load(&path), state);
        assert_eq!(
            UiState::parse("thread x\ngarbage\ntab only-two\n"),
            UiState {
                thread: Some("x".into()),
                tabs: HashMap::new()
            }
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(UiState::path(&ConnectTarget::Remote("box".into())).is_none());
    }
}
