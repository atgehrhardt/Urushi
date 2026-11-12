//! Tabs and panes on their way in (ADR 0055). A new tab or a split waits for the daemon
//! to start a shell and save the layout; until it answers, the GUI shows what is coming:
//! a "Starting…" chip in the tab bar for a tab, and for a split the layout already split
//! (by the core's own `PaneGroup::split`, so nothing is decided here) with an empty pane
//! where the new one will be.
//!
//! A split's placeholder goes once the daemon's workspace has the new pane, or the
//! request failed, or (if another client changed the tab meanwhile) one workspace after
//! the answer that isn't followed by the pane: the daemon announces a change before it
//! answers, so the second workspace after the answer is never older than it.

use urushi_core::{Direction, PaneGroup, PaneId};

/// The pane id that stands for a pane the daemon is still starting. Real pane ids are
/// time-ordered UUIDs, never zero.
pub const PLACEHOLDER: PaneId = PaneId::from_u128(0);

/// A tab the daemon is opening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingTab {
    /// This app's number for it.
    pub key: u64,
    /// The thread it opens in.
    pub thread: String,
    /// What it runs, for its chip ("Terminal", "Claude", …).
    pub title: String,
}

/// A split the daemon is making.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingSplit {
    /// The tab (or quick terminal) split.
    pub group: String,
    /// The pane split.
    pub pane: PaneId,
    /// Where the new pane goes.
    pub side: Direction,
    /// The new pane's id, once the daemon answered.
    pub created: Option<PaneId>,
    /// Workspaces seen since the answer.
    workspaces_after: u8,
}

/// Tabs and splits the daemon hasn't finished yet.
#[derive(Debug, Default)]
pub struct Starting {
    last: u64,
    tabs: Vec<StartingTab>,
    split: Option<StartingSplit>,
}

impl Starting {
    /// A tab is being opened in `thread`; returns its key.
    pub fn tab(&mut self, thread: String, title: String) -> u64 {
        self.last += 1;
        self.tabs.push(StartingTab {
            key: self.last,
            thread,
            title,
        });
        self.last
    }

    /// The daemon answered for the tab with `key`.
    pub fn tab_done(&mut self, key: u64) {
        self.tabs.retain(|t| t.key != key);
    }

    /// Tabs being opened in `thread`.
    pub fn tabs_of<'a>(&'a self, thread: &'a str) -> impl Iterator<Item = &'a StartingTab> + 'a {
        self.tabs.iter().filter(move |t| t.thread == thread)
    }

    /// Starts showing `pane` of `group` split. Returns `false` (and changes nothing)
    /// while another split is still on its way, so a repeated key doesn't split twice.
    pub fn split(&mut self, group: String, pane: PaneId, side: Direction) -> bool {
        if self.split.is_some() {
            return false;
        }
        self.split = Some(StartingSplit {
            group,
            pane,
            side,
            created: None,
            workspaces_after: 0,
        });
        true
    }

    /// The daemon made the split's new pane (`Some`), or refused (`None`).
    pub fn split_done(&mut self, created: Option<PaneId>) {
        match (created, &mut self.split) {
            (Some(pane), Some(split)) => split.created = Some(pane),
            _ => self.split = None,
        }
    }

    /// A workspace came from the daemon; `group` is the split group in it. Drops the
    /// placeholder once the real pane is there (or can't be expected any more).
    pub fn workspace(&mut self, group: Option<&PaneGroup>) {
        let Some(split) = &mut self.split else {
            return;
        };
        let Some(created) = split.created else {
            return;
        };
        split.workspaces_after = split.workspaces_after.saturating_add(1);
        let arrived = group.is_none_or(|g| g.layout().contains(created));
        if arrived || split.workspaces_after >= 2 {
            self.split = None;
        }
    }

    /// The group whose split is shown ahead of the daemon, if any.
    #[must_use]
    pub fn split_group(&self) -> Option<&str> {
        self.split.as_ref().map(|s| s.group.as_str())
    }

    /// `group` (the daemon's layout of `id`) as it should show: split, with the
    /// placeholder, until the daemon's own split arrives.
    #[must_use]
    pub fn apply(&self, id: &str, mut group: PaneGroup) -> PaneGroup {
        if let Some(s) = self.split.as_ref().filter(|s| s.group == id)
            && !s.created.is_some_and(|c| group.layout().contains(c))
        {
            let focused = group.focused();
            if group.split(s.pane, PLACEHOLDER, s.side).is_ok() {
                // The placeholder only takes room; focus stays where the daemon has it.
                let _ = group.focus(focused);
            }
        }
        group
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> PaneGroup {
        PaneGroup::new(PaneId::from_u128(1))
    }

    #[test]
    fn a_split_shows_at_once_and_only_once() {
        let mut s = Starting::default();
        assert!(s.split("tab".into(), PaneId::from_u128(1), Direction::Right));
        assert!(!s.split("tab".into(), PaneId::from_u128(1), Direction::Right));
        let shown = s.apply("tab", group());
        assert_eq!(
            shown.layout().panes(),
            vec![PaneId::from_u128(1), PLACEHOLDER]
        );
        assert_eq!(shown.focused(), PaneId::from_u128(1));
        assert_eq!(s.apply("other", group()).layout().panes().len(), 1);
    }

    #[test]
    fn the_placeholder_goes_when_the_real_pane_arrives() {
        let mut s = Starting::default();
        s.split("tab".into(), PaneId::from_u128(1), Direction::Right);
        let real = PaneId::from_u128(2);
        s.split_done(Some(real));
        let before = group();
        s.workspace(Some(&before));
        assert!(s.split_group().is_some(), "an older workspace");
        let mut after = group();
        after
            .split(PaneId::from_u128(1), real, Direction::Right)
            .unwrap();
        // Shown as the daemon has it: no placeholder on top.
        assert_eq!(s.apply("tab", after.clone()).layout().panes().len(), 2);
        s.workspace(Some(&after));
        assert!(s.split_group().is_none());
    }

    #[test]
    fn a_refused_split_shows_nothing() {
        let mut s = Starting::default();
        s.split("tab".into(), PaneId::from_u128(1), Direction::Right);
        s.split_done(None);
        assert!(s.split_group().is_none());
        assert!(s.split("tab".into(), PaneId::from_u128(1), Direction::Down));
    }

    #[test]
    fn tabs_are_kept_per_thread_until_answered() {
        let mut s = Starting::default();
        let a = s.tab("t1".into(), "Claude".into());
        s.tab("t2".into(), "Terminal".into());
        assert_eq!(s.tabs_of("t1").count(), 1);
        s.tab_done(a);
        assert_eq!(s.tabs_of("t1").count(), 0);
        assert_eq!(s.tabs_of("t2").count(), 1);
    }
}
