//! Application state: everything the view needs, and nothing it doesn't.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use urushi_client::{ScreenState, WorkspaceMirror, v1};
use urushi_ui::{CellMetrics, Rect, TextField};

use crate::config::ConnectTarget;

/// Connection status.
#[derive(Clone, Debug, PartialEq)]
pub enum Connection {
    /// Trying to connect.
    Connecting,
    /// Connected to a daemon on `host`.
    Connected {
        /// The daemon's host name.
        host: String,
    },
    /// Not connected. The reason is shown, and the backend keeps retrying.
    Disconnected(String),
}

/// A dialog field that can take keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// Add project: repository path.
    ProjectPath,
    /// New thread: title.
    ThreadTitle,
    /// New thread: base branch.
    ThreadBase,
    /// Settings: branch prefix.
    BranchPrefix,
    /// Settings: default shell.
    DefaultShell,
    /// Connect: host.
    Host,
}

/// A settings switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingToggle {
    /// Delete remote branches with threads.
    DeleteRemoteBranches,
    /// Serve the daemon on the tailnet.
    ServeTailnet,
}

/// The open dialog, if any.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// Add a repository.
    AddProject {
        /// Path field.
        path: TextField,
    },
    /// Create a thread in `project`.
    NewThread {
        /// Project id.
        project: String,
        /// Title field.
        title: TextField,
        /// Base branch field (empty = project default).
        base: TextField,
    },
    /// The deletion warning. Shows exactly what will be destroyed.
    DeleteThread {
        /// Thread title, for the heading.
        title: String,
        /// What the daemon will delete.
        plan: v1::DeletionPlan,
        /// The delete request is in flight.
        deleting: bool,
    },
    /// Settings.
    Settings {
        /// Branch prefix field.
        prefix: TextField,
        /// Default shell field.
        shell: TextField,
    },
    /// Choose a daemon to connect to.
    Connect {
        /// Host field.
        host: TextField,
    },
}

/// A text selection in a pane, in cell coordinates (row, column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the drag started.
    pub anchor: (u16, u16),
    /// Where it is now.
    pub head: (u16, u16),
}

impl Selection {
    /// `(start, end)` in reading order, end inclusive.
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Whether the cell is selected.
    pub fn contains(&self, row: u16, col: u16) -> bool {
        let (start, end) = self.ordered();
        (row, col) >= start && (row, col) <= end
    }
}

/// What a mouse drag in the terminal area is doing.
#[derive(Clone, Debug, PartialEq)]
pub enum Drag {
    /// Moving a divider.
    Divider {
        /// Tab id.
        tab: String,
        /// The divider's anchor pane.
        anchor: String,
        /// Split axis.
        axis: urushi_core::Axis,
        /// The split's rectangle (logical pixels).
        split: Rect,
    },
    /// Selecting text in a pane.
    Select {
        /// Pane id.
        pane: String,
    },
}

/// The whole application state.
pub struct AppState {
    /// Which daemon to use.
    pub target: ConnectTarget,
    /// Connection status.
    pub connection: Connection,
    /// The workspace as last reported.
    pub workspace: WorkspaceMirror,
    /// The thread shown in the main area.
    pub selected_thread: Option<String>,
    /// The tab shown per thread.
    pub selected_tab: HashMap<String, String>,
    /// Screens of attached panes.
    pub screens: HashMap<String, Arc<ScreenState>>,
    /// Panes with an attach request in flight.
    pub attaching: HashSet<String>,
    /// Terminal size last sent per pane.
    pub sizes: HashMap<String, (u16, u16)>,
    /// The terminal area (logical pixels), once laid out.
    pub area: Option<Rect>,
    /// Size of one terminal cell (logical pixels).
    pub cell: CellMetrics,
    /// The open dialog.
    pub dialog: Option<Dialog>,
    /// The focused dialog field.
    pub focus: Option<Field>,
    /// An error to show in the banner.
    pub error: Option<String>,
    /// Text selection: pane and range.
    pub selection: Option<(String, Selection)>,
    /// The drag in progress.
    pub drag: Option<Drag>,
    /// Settings, once loaded.
    pub settings: Option<v1::Settings>,
    /// Tailnet status, once loaded.
    pub tailnet: Option<v1::TailnetStatus>,
    /// Pane focused locally before the daemon confirms it.
    pub focused_override: HashMap<String, String>,
    /// Narrow window (phones): one column at a time.
    pub compact: bool,
    /// In compact mode, whether the thread list is showing (instead of the terminals).
    pub sidebar_open: bool,
}

impl AppState {
    /// Initial state.
    pub fn new(target: ConnectTarget, cell: CellMetrics) -> Self {
        let dialog = (target == ConnectTarget::Ask).then(|| Dialog::Connect {
            host: TextField::default(),
        });
        Self {
            focus: dialog.as_ref().map(|_| Field::Host),
            target,
            connection: Connection::Connecting,
            workspace: WorkspaceMirror::default(),
            selected_thread: None,
            selected_tab: HashMap::new(),
            screens: HashMap::new(),
            attaching: HashSet::new(),
            sizes: HashMap::new(),
            area: None,
            cell,
            dialog,
            error: None,
            selection: None,
            drag: None,
            settings: None,
            tailnet: None,
            focused_override: HashMap::new(),
            compact: false,
            sidebar_open: true,
        }
    }

    /// The tab shown in the main area.
    pub fn current_tab(&self) -> Option<&v1::Tab> {
        let thread = self.selected_thread.as_ref()?;
        let tabs = self.workspace.tabs_of(thread);
        let selected = self.selected_tab.get(thread);
        tabs.iter()
            .find(|t| Some(&t.id) == selected)
            .or_else(|| tabs.first())
            .copied()
    }

    /// The focused pane of the current tab.
    pub fn focused_pane(&self) -> Option<String> {
        let tab = self.current_tab()?;
        Some(
            self.focused_override
                .get(&tab.id)
                .cloned()
                .unwrap_or_else(|| tab.focused_pane_id.clone()),
        )
    }

    /// Panes of the current tab, with their rectangles in the terminal area.
    pub fn pane_rects(&self) -> Vec<(String, Rect)> {
        let (Some(area), Some(tab)) = (self.area, self.current_tab()) else {
            return Vec::new();
        };
        self.workspace
            .tab(&tab.id)
            .map(|t| crate::terminal::pane_rects(t.layout(), area))
            .unwrap_or_default()
    }
}
