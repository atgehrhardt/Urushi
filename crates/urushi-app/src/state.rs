//! Application state: everything the view needs, and nothing it doesn't.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use urushi_client::{ScreenState, WorkspaceMirror, v1};
use urushi_ui::{CellMetrics, Point, Rect, TextField};

use crate::config::ConnectTarget;
use crate::organize::{ContextMenu, Prompt, Select, SidebarTab};

/// The protocol minor from which the daemon can fetch a new thread's base from its
/// remote first (ADR 0028).
pub const FRESH_BASE_SINCE_MINOR: u32 = 16;

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

/// A task on a board, or one of its subtasks (at any depth), that a thread is started
/// for. This is the shape of `CreateThread{task_id, subtask_id}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskLink {
    /// The card's id.
    pub task: String,
    /// The subtask's id, when the thread is for a subtask of the card.
    pub subtask: Option<String>,
}

impl TaskLink {
    /// The link for a task found on a board: its card, and itself if it is a subtask.
    pub fn of(found: &urushi_client::FoundTask<'_>) -> Self {
        let task = found.task();
        Self {
            task: found.card.id.clone(),
            subtask: (!task.is_card()).then(|| task.id().to_owned()),
        }
    }

    /// The task the thread is for.
    pub fn target(&self) -> &str {
        self.subtask.as_deref().unwrap_or(&self.task)
    }
}

/// Where the new-thread dialog sends a board task: a new thread (the only choice
/// without a task), or a new tab in an existing thread (ADR 0018).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ThreadTarget {
    /// Create a thread.
    #[default]
    New,
    /// Start in this existing thread; `None` until one is picked.
    Existing(Option<String>),
}

/// The subtask part of the task editor. Subtasks added in the editor are kept here as
/// drafts and created on Save (with a new task, or one by one for an existing one), so
/// nothing reaches the board, or Jira, before Save. Checking an existing subtask off
/// and deleting one are immediate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubtaskEditor {
    /// Subtasks to create on Save, each with the details written in its own editor.
    pub drafts: Vec<crate::drafts::SubtaskDraft>,
    /// The "add a subtask" field.
    pub new: TextField,
}

/// How the task editor got to the task on screen. Opening a subtask (or any other task)
/// from the editor keeps the editor it came from, unsaved edits included, so **Back**
/// returns to it as it was; Save saves every editor here, Cancel discards them all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorNav {
    /// The editors opened before this one, oldest first (each a `Dialog::Task`).
    pub back: Vec<Dialog>,
    /// Closing was asked with unsaved edits: the editor asks to confirm.
    pub confirm_discard: bool,
    /// Deleting a Jira issue (this task or one of its subtasks) was asked: the editor
    /// asks to confirm (ADR 0034).
    pub confirm_delete: Option<IssueDeletion>,
    /// This editor writes a new subtask of the editor it was opened from (the last in
    /// `back`), to go at this place in its drafts. Back and Save put it there.
    pub draft_of_previous: Option<usize>,
}

impl EditorNav {
    /// Drops any question the editor was asking (discard, delete).
    pub fn settle(&mut self) {
        self.confirm_discard = false;
        self.confirm_delete = None;
    }
}

/// A Jira issue about to be deleted, as the question before it names it. Deleting it
/// in Jira is for good and for everyone, so Urushi always asks (ADR 0034).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueDeletion {
    /// The task showing it.
    pub task: String,
    /// Its key (`ACME-2`).
    pub key: String,
    /// Its title.
    pub title: String,
    /// How many subtasks (at any depth) go with it.
    pub subtasks: usize,
}

impl IssueDeletion {
    /// "ACME-2", "ACME-2 and its subtask" or "ACME-2 and its 3 subtasks".
    pub fn what(&self) -> String {
        match self.subtasks {
            0 => self.key.clone(),
            1 => format!("{} and its subtask", self.key),
            n => format!("{} and its {n} subtasks", self.key),
        }
    }
}

/// A dialog field that can take keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// Add project: repository path.
    ProjectPath,
    /// New thread: title.
    ThreadTitle,
    /// New thread: name of the branch to create.
    ThreadBranch,
    /// A one-line prompt (rename, new status, new project).
    Prompt,
    /// A thread's notes.
    ThreadNotes,
    /// Settings: branch prefix.
    BranchPrefix,
    /// Settings: default shell.
    DefaultShell,
    /// Connect: host.
    Host,
    /// Connect: the one-time code from a LAN host, to pair first (ADR 0041).
    PairingCode,
    /// Task editor: title.
    TaskTitle,
    /// Task editor: notes.
    TaskNotes,
    /// Task editor: the "add a subtask" field.
    NewSubtask,
    /// "Make subtask of…": the search field.
    NestQuery,
    /// Task editor: the "add a tag" field.
    TaskTag,
    /// Task editor: the pull request link.
    TaskPrUrl,
    /// Connect Jira: the site.
    JiraSite,
    /// Connect Jira: the account's email.
    JiraEmail,
    /// Connect Jira: the API token (a secret field).
    JiraToken,
    /// Import from Jira: the search field.
    JiraQuery,
    /// The board's search field (outside any dialog).
    BoardSearch,
    /// Task editor (Jira): a text-like field, by its place in the issue's fields.
    JiraText(u16),
    /// Task editor (Jira): the open picker's search.
    JiraSearch,
    /// Task editor (Jira): a new comment.
    JiraComment,
    /// Task editor (Jira): the comment being changed.
    JiraCommentEdit,
    /// Task editor (Jira): time to log.
    JiraWork,
    /// Settings: a custom color (`appearance::COLOR_NAMES`), as `#rrggbb`.
    LookColor(u8),
    /// Settings: the user's own task message.
    TaskMessage,
    /// Quick filter: its name.
    QuickFilterName,
    /// Quick filter (Jira board): its JQL.
    QuickFilterJql,
}

/// An editing command on the focused text field, from a shortcut or its context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldCommand {
    /// Copy the selection to the clipboard and delete it.
    Cut,
    /// Copy the selection to the clipboard.
    Copy,
    /// Insert the clipboard, replacing the selection.
    Paste,
    /// Select all the text.
    SelectAll,
    /// Undo the last edit.
    Undo,
    /// Redo the last undone edit.
    Redo,
}

/// A settings switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingToggle {
    /// Delete remote branches with threads.
    DeleteRemoteBranches,
    /// Fetch a new thread's base from its remote first.
    UpdateBaseFromRemote,
    /// Serve the daemon on the tailnet.
    ServeTailnet,
    /// Let paired devices connect over the LAN (ADR 0041).
    LanAccess,
    /// Start an agent with its skip-permissions flag.
    SkipPermissions(urushi_core::Agent),
    /// Send a task's message to its agent, or only draft it there (protocol 1.19).
    TaskSend,
    /// Start tasks with the user's own message instead of the default (1.19).
    TaskMessageCustom,
}

/// What a new tab runs: a plain shell, or an agent CLI inside one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Just the shell.
    Terminal,
    /// An agent CLI, typed into the shell.
    Agent(urushi_core::Agent),
}

impl Launch {
    /// Label in pickers.
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Agent(agent) => agent.display_name(),
        }
    }
}

/// The open dialog, if any.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// Add a repository: pick a folder (or, where no picker can reach the daemon's
    /// files, type its path).
    AddProject {
        /// Path field (remote and mobile only).
        path: TextField,
    },
    /// Create a thread in a repo, optionally filed under a project.
    NewThread {
        /// Repo id.
        repo: String,
        /// Project to file it under.
        project: Option<String>,
        /// Title field.
        title: TextField,
        /// Branch field. Follows the title until the user edits it.
        branch: TextField,
        /// The user typed their own branch name, so the title no longer changes it.
        branch_edited: bool,
        /// Branch to start from; `None` = the repo's default (its current branch).
        base: Option<String>,
        /// The selector that is open, if any.
        open: Option<Select>,
        /// The board task (and maybe subtask) the thread is started for.
        task: Option<TaskLink>,
        /// For a task: a new thread, or a new tab in one that exists.
        target: ThreadTarget,
    },
    /// A thread's notes, edited over several lines (ADR 0048).
    ThreadNotes {
        /// Thread id.
        thread: String,
        /// The notes.
        notes: TextField,
    },
    /// A one-line text prompt.
    Prompt {
        /// What it's for.
        prompt: Prompt,
        /// The text.
        field: TextField,
    },
    /// Choose what a new tab runs.
    NewTab {
        /// The highlighted choice (index into [`AppState::launch_choices`]).
        highlighted: usize,
    },
    /// The deletion warning. Shows exactly what will be destroyed.
    DeleteThread {
        /// Thread title, for the heading.
        title: String,
        /// Thread id.
        thread: String,
        /// What the daemon will delete; `None` while it works that out (it takes a few
        /// git commands), so the warning opens at once.
        plan: Option<v1::DeletionPlan>,
        /// Project settings to save once the thread (a parent-branch thread) is closed.
        then: Option<Box<crate::project_settings::PendingSettings>>,
    },
    /// A project's settings: its parent branch and parent-branch thread.
    ProjectSettings(Box<crate::project_settings::ProjectSettingsDraft>),
    /// Settings.
    Settings {
        /// Branch prefix field.
        prefix: TextField,
        /// Default shell field.
        shell: TextField,
        /// The user's own task message (multiline), saved with Save.
        message: TextField,
        /// The Appearance section's draft, shown live until Save or Cancel.
        look: Box<crate::appearance::LookDraft>,
    },
    /// Confirm updating the app, which restarts the daemon (ADR 0049).
    InstallUpdate(Box<crate::updates::UpdateDialog>),
    /// Confirm restarting the local daemon, which closes every terminal.
    RestartDaemon {
        /// Open terminals that will close.
        terminals: usize,
        /// Threads they are in.
        threads: usize,
        /// The restart is under way.
        restarting: bool,
    },
    /// Choose a daemon to connect to, and pair with a LAN host first if a code is given.
    Connect {
        /// Host field.
        host: TextField,
        /// Pairing code field (LAN hosts, first time only).
        code: TextField,
        /// Pairing is under way.
        busy: bool,
        /// Why pairing failed.
        error: Option<String>,
    },
    /// Create or edit a task on a board.
    Task {
        /// Board id.
        board: String,
        /// The column it goes in (for an existing task: where it moves on save).
        column: String,
        /// The task being edited; `None` creates one.
        task: Option<String>,
        /// Title field.
        title: TextField,
        /// Notes field.
        notes: TextField,
        /// The editor's subtask fields (boxed: they hold two more text fields, and
        /// would make every dialog as big).
        subtasks: Box<SubtaskEditor>,
        /// The color and tags, sent with Save.
        labels: Box<crate::board::TaskLabels>,
        /// The editors this one was opened from, and whether closing asks first.
        nav: Box<EditorNav>,
        /// For a Jira issue: the issue in full, and its drafts.
        issue: Box<crate::issue::IssueEditor>,
    },
    /// The first-run setup wizard: which agent (and mode) threads started from tasks run.
    Setup {
        /// The step shown.
        step: crate::task_agent::SetupStep,
        /// The agent chosen so far.
        agent: urushi_core::Agent,
        /// The mode chosen so far.
        mode: urushi_core::AgentMode,
    },
    /// "Make subtask of…": choose the task to put a task under.
    NestTask {
        /// The task to move.
        task: String,
        /// Narrows the list by title.
        query: TextField,
    },
    /// Confirm removing a column; its tasks move to the column next to it.
    DeleteColumn {
        /// Column id.
        column: String,
        /// Its name, for the heading.
        name: String,
        /// How many tasks move.
        tasks: usize,
        /// The name of the column they move to.
        into: String,
    },
    /// Sign the daemon in to Jira Cloud.
    ConnectJira {
        /// The site ("acme" or its address).
        site: TextField,
        /// The Atlassian account's email.
        email: TextField,
        /// The API token (a secret field).
        token: TextField,
        /// Waiting for Jira to check the sign-in.
        busy: bool,
        /// Why the last try failed.
        error: Option<String>,
    },
    /// Choose a Jira board to import.
    ImportJira {
        /// The sign-in whose site's boards are listed (`JiraAccount.key`; empty: the
        /// daemon's first).
        account: String,
        /// Narrows the list by name (sent to Jira with Enter).
        query: TextField,
        /// The boards found; `None` while loading.
        boards: Option<Vec<v1::JiraBoardInfo>>,
        /// Why listing or importing failed.
        error: Option<String>,
    },
    /// Save what is chosen on the board as a quick filter, or change one (ADR 0037).
    QuickFilter(Box<crate::filter::editor::QuickFilterEditor>),
    /// Confirm deleting a quick filter.
    DeleteQuickFilter {
        /// Board id.
        board: String,
        /// The filter.
        filter: crate::filter::quick::QuickRef,
        /// Its name, for the heading.
        name: String,
    },
    /// Confirm deleting a Jira issue (from its card's menu), in Jira, for good.
    DeleteIssue(IssueDeletion),
    /// Confirm deleting a board and its tasks.
    DeleteBoard {
        /// Board id.
        board: String,
        /// Its name, for the heading.
        name: String,
        /// How many tasks go with it.
        tasks: usize,
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
    /// A button held over a pane whose application takes the mouse: moves and the
    /// release are reported to it.
    Report {
        /// Pane id.
        pane: String,
        /// The button held.
        button: urushi_client::v1::MouseButton,
    },
}

/// The whole application state.
pub struct AppState {
    /// Which daemon to use.
    pub target: ConnectTarget,
    /// Connection status.
    pub connection: Connection,
    /// The workspace as shown: the daemon's last report without threads being deleted
    /// (see [`crate::deletions`]).
    pub workspace: WorkspaceMirror,
    /// The daemon's last reported workspace, and the deletions the app waits on.
    pub deletions: crate::deletions::Deletions,
    /// The thread shown in the main area.
    pub selected_thread: Option<String>,
    /// Threads this app asked the daemon to create, shown until it answers (ADR 0056).
    pub creations: crate::creations::Creations,
    /// The pending create shown in the main area, while no thread is selected.
    pub selected_pending: Option<u64>,
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
    /// The dialog as it was submitted, while the daemon saves it (`update/saving.rs`).
    pub submitting: Option<Box<Dialog>>,
    /// The error to show, on whatever surface is on top (`errors.rs`).
    pub error: Option<String>,
    /// Text selection: pane and range.
    pub selection: Option<(String, Selection)>,
    /// The drag in progress.
    pub drag: Option<Drag>,
    /// A divider dragged on screen ahead of the daemon (`divider.rs`).
    pub divider: crate::divider::DividerDrag,
    /// Tabs and splits the daemon is still making, shown ahead of it (`starting.rs`).
    pub starting: crate::starting::Starting,
    /// Settings, once loaded.
    pub settings: Option<v1::Settings>,
    /// The daemon's Jira sign-ins, once asked (protocol 1.12; several since 1.29, ADR
    /// 0051). Only connected ones.
    pub jira_accounts: Vec<v1::JiraAccount>,
    /// Tailnet status, once loaded.
    pub tailnet: Option<v1::TailnetStatus>,
    /// LAN access status, once loaded (only from the daemon on this machine).
    pub lan: Option<v1::LanStatus>,
    /// LAN hosts this device paired with, for the connect dialog.
    pub lan_hosts: Vec<crate::msg::PairedHost>,
    /// Remote hosts this device connected to, most recent first, for the connect
    /// dialog (ADR 0043).
    pub recent_hosts: Vec<String>,
    /// Whether the app went back to the host it used last, unasked, and what came of
    /// it.
    pub resume: Resume,
    /// Pane focused locally (tab id → pane id) before the daemon confirms it. Dropped
    /// once the daemon agrees, or when the pane is gone.
    pub focused_override: HashMap<String, String>,
    /// Narrow window (phones): one column at a time.
    pub compact: bool,
    /// Attention already announced or cleared, and whether the window is in the
    /// background (`attention.rs`).
    pub attention: crate::attention::AttentionTracker,
    /// How bright running indicators are right now, 0 to 1: the runner moves it while an
    /// agent works, so they pulse (`running.rs`).
    pub pulse: f32,
    /// In compact mode, whether the thread list is showing (instead of the terminals).
    pub sidebar_open: bool,
    /// Outside compact mode: the sidebar's width, and whether it's collapsed.
    pub sidebar: crate::sidebar::SidebarLayout,
    /// Agent CLIs installed on the daemon's machine, as last reported.
    pub agents: Vec<urushi_core::Agent>,
    /// The daemon's protocol minor version (capabilities, see `docs/protocol.md`).
    pub daemon_minor: u32,
    /// The daemon's version, as it reported it.
    pub daemon_version: String,
    /// Which list the sidebar shows.
    pub sidebar_tab: SidebarTab,
    /// Collapsed sidebar sections (see [`crate::organize::section_key`]).
    pub collapsed: HashSet<String>,
    /// The open context menu.
    pub menu: Option<ContextMenu>,
    /// The thread or project being dragged in the sidebar, and where the pointer is.
    pub dragging: Option<(crate::organize::Dragged, Point)>,
    /// While a project is dragged: the project whose place it would take if dropped now.
    pub project_over: Option<String>,
    /// While a thread is dragged: the thread whose place it would take if dropped now.
    pub thread_over: Option<String>,
    /// Where the last mouse press was (anchors context menus).
    pub last_press: Option<Point>,
    /// Branches of a repo, for the new-thread dialog: `(repo id, branches)`.
    pub branches: Option<(String, v1::Branches)>,
    /// Modifier keys currently held.
    pub mods: crate::keys::Mods,
    /// The cell last reported as hovered, so movement is reported once per cell.
    pub hover: Option<(String, (u16, u16))>,
    /// The task-board panel.
    pub board: crate::board::BoardPanel,
    /// The quick terminal's drop-down panel.
    pub quick: crate::quick_terminal::QuickPanel,
    /// Plan usage of the agent in a pane, as last reported: `(pane, usage)`.
    pub usage: Option<(String, v1::AgentUsage)>,
    /// A usage request is in flight (one at a time).
    pub usage_pending: bool,
    /// Wall-clock time of the last tick (ms since the epoch), for "resets in …".
    pub now_ms: i64,
    /// The window's height in logical pixels (0 before the first frame).
    pub window_height: f32,
    /// When the setup wizard may open.
    pub setup: crate::task_agent::SetupGate,
    /// Checks for app updates asked for in Settings (ADR 0049).
    pub update_checks: crate::updates::UpdateChecks,
}

impl AppState {
    /// Initial state.
    pub fn new(target: ConnectTarget, cell: CellMetrics) -> Self {
        let dialog = (target == ConnectTarget::Ask).then(Dialog::connect);
        Self {
            focus: dialog.as_ref().map(|_| Field::Host),
            target,
            connection: Connection::Connecting,
            workspace: WorkspaceMirror::default(),
            deletions: crate::deletions::Deletions::default(),
            selected_thread: None,
            creations: crate::creations::Creations::default(),
            selected_pending: None,
            selected_tab: HashMap::new(),
            screens: HashMap::new(),
            attaching: HashSet::new(),
            sizes: HashMap::new(),
            area: None,
            cell,
            dialog,
            submitting: None,
            error: None,
            selection: None,
            drag: None,
            divider: crate::divider::DividerDrag::default(),
            starting: crate::starting::Starting::default(),
            settings: None,
            jira_accounts: Vec::new(),
            tailnet: None,
            lan: None,
            lan_hosts: Vec::new(),
            recent_hosts: Vec::new(),
            resume: Resume::No,
            focused_override: HashMap::new(),
            compact: false,
            attention: crate::attention::AttentionTracker::default(),
            pulse: 1.0,
            sidebar_open: true,
            sidebar: crate::sidebar::SidebarLayout::default(),
            agents: Vec::new(),
            daemon_minor: 0,
            daemon_version: String::new(),
            sidebar_tab: SidebarTab::default(),
            collapsed: HashSet::new(),
            menu: None,
            dragging: None,
            project_over: None,
            thread_over: None,
            last_press: None,
            branches: None,
            mods: crate::keys::Mods::default(),
            hover: None,
            board: crate::board::BoardPanel::default(),
            quick: crate::quick_terminal::QuickPanel::default(),
            usage: None,
            usage_pending: false,
            now_ms: 0,
            window_height: 0.0,
            setup: crate::task_agent::SetupGate::default(),
            update_checks: crate::updates::UpdateChecks::default(),
        }
    }

    /// Initial state going back to `address`, the remote host used last time, without
    /// asking. If it can't be reached, the connect dialog asks (ADR 0043).
    pub fn resuming(address: String, cell: CellMetrics) -> Self {
        let mut state = Self::new(ConnectTarget::Remote(address), cell);
        state.resume = Resume::Trying;
        state
    }

    /// What a new tab can run: always a terminal, then each installed agent.
    pub fn launch_choices(&self) -> Vec<Launch> {
        std::iter::once(Launch::Terminal)
            .chain(self.agents.iter().copied().map(Launch::Agent))
            .collect()
    }

    /// Whether the daemon speaks an older protocol minor than this app, so some features
    /// (see `docs/protocol.md`) are missing until it restarts.
    pub fn daemon_outdated(&self) -> bool {
        matches!(self.connection, Connection::Connected { .. })
            && self.daemon_minor < urushi_proto::PROTOCOL_MINOR
    }

    /// Whether the daemon stores agent settings and starts agents (protocol 1.1).
    pub fn daemon_supports_agents(&self) -> bool {
        self.daemon_minor >= 1
    }

    /// Whether the daemon can fetch a new thread's base from its remote first (protocol
    /// 1.16, ADR 0028).
    pub fn daemon_supports_fresh_base(&self) -> bool {
        self.daemon_minor >= FRESH_BASE_SINCE_MINOR
    }

    /// Whether the daemon keeps subtasks and links threads to tasks (protocol 1.6).
    pub fn daemon_supports_subtasks(&self) -> bool {
        self.daemon_minor >= crate::board::SUBTASKS_SINCE_MINOR
    }

    /// Whether the daemon keeps colors, tags and changeable columns (protocol 1.8).
    pub fn daemon_supports_board_extras(&self) -> bool {
        self.daemon_minor >= crate::board::BOARD_EXTRAS_SINCE_MINOR
    }

    /// Whether the daemon follows Jira boards and keeps task kinds (protocol 1.12).
    pub fn daemon_supports_jira(&self) -> bool {
        self.daemon_minor >= crate::board::JIRA_SINCE_MINOR
    }

    /// Whether the daemon keeps several Jira sign-ins (protocol 1.29, ADR 0051).
    pub fn daemon_supports_jira_accounts(&self) -> bool {
        self.daemon_minor >= crate::board::JIRA_ACCOUNTS_SINCE_MINOR
    }

    /// Whether the daemon creates epics on Jira boards, as cards (protocol 1.15).
    pub fn daemon_supports_epic_cards(&self) -> bool {
        self.daemon_minor >= crate::board::EPIC_CARDS_SINCE_MINOR
    }

    /// Whether the daemon links pull requests to tasks and reads a Jira issue's from
    /// Jira (protocol 1.22, ADR 0036).
    pub fn daemon_supports_pr_links(&self) -> bool {
        self.daemon_minor >= crate::board::PR_LINKS_SINCE_MINOR
    }

    /// Whether the daemon deletes Jira issues (protocol 1.21, ADR 0034).
    pub fn daemon_supports_jira_deletion(&self) -> bool {
        self.daemon_minor >= crate::board::JIRA_DELETION_SINCE_MINOR
    }

    /// Whether Delete is offered for a task: any of Urushi's own, and Jira issues on a
    /// daemon that deletes them.
    pub fn can_delete(&self, task: urushi_client::TaskRef<'_>) -> bool {
        task.issue().is_none() || self.daemon_supports_jira_deletion()
    }

    /// Whether the daemon keeps a quick terminal (protocol 1.20).
    pub fn daemon_supports_quick_terminal(&self) -> bool {
        self.daemon_minor >= crate::quick_terminal::QUICK_TERMINAL_SINCE_MINOR
    }

    /// Whether the daemon can reorder projects (protocol 1.11).
    pub fn daemon_supports_project_order(&self) -> bool {
        self.daemon_minor >= crate::organize::PROJECT_ORDER_SINCE_MINOR
    }

    /// Whether the daemon keeps unread marks, thread notes and project colors (protocol
    /// 1.27).
    pub fn daemon_supports_thread_extras(&self) -> bool {
        self.daemon_minor >= crate::organize::THREAD_EXTRAS_SINCE_MINOR
    }

    /// Whether the daemon can reorder threads (protocol 1.24).
    pub fn daemon_supports_thread_order(&self) -> bool {
        self.daemon_minor >= crate::organize::THREAD_ORDER_SINCE_MINOR
    }

    /// Whether the daemon keeps quick filters and writes them to Jira (protocol 1.23).
    pub fn daemon_supports_quick_filters(&self) -> bool {
        self.daemon_minor >= urushi_client::QUICK_FILTERS_SINCE_MINOR
    }

    /// Whether the daemon can reorder boards (protocol 1.17).
    pub fn daemon_supports_board_order(&self) -> bool {
        self.daemon_minor >= crate::board::BOARD_ORDER_SINCE_MINOR
    }

    /// Whether the daemon keeps project settings: a parent branch and its thread
    /// (protocol 1.18).
    pub fn daemon_supports_project_settings(&self) -> bool {
        self.daemon_minor >= crate::organize::PROJECT_SETTINGS_SINCE_MINOR
    }

    /// Whether `agent` is started with its skip-permissions flag (per the daemon's settings).
    pub fn skips_permissions(&self, agent: urushi_core::Agent) -> bool {
        self.settings
            .as_ref()
            .and_then(|s| s.agents.as_ref())
            .is_some_and(|a| match agent {
                urushi_core::Agent::Claude => a.claude_skip_permissions,
                urushi_core::Agent::Codex => a.codex_skip_permissions,
                urushi_core::Agent::Cursor => a.cursor_skip_permissions,
            })
    }

    /// The selected thread, if it exists and has no tabs (so the launcher shows instead).
    pub fn empty_thread(&self) -> Option<&v1::Thread> {
        let thread = self.workspace.thread(self.selected_thread.as_ref()?)?;
        self.workspace
            .tabs_of(&thread.id)
            .is_empty()
            .then_some(thread)
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

    /// On phones (`compact`), whether the page shown is the thread list rather than the
    /// terminals: it was asked for, or there is nothing else to show.
    pub fn shows_thread_list_page(&self) -> bool {
        self.sidebar_open || self.current_tab().is_none() && self.selected_thread.is_none()
    }
}

impl Dialog {
    /// An empty connect dialog.
    pub(crate) fn connect() -> Self {
        Self::connect_to("", None)
    }

    /// A connect dialog with `host` filled in, saying why connecting failed if it did.
    pub(crate) fn connect_to(host: &str, error: Option<String>) -> Self {
        Self::Connect {
            host: TextField::new(host),
            code: TextField::default(),
            busy: false,
            error,
        }
    }
}

/// Going back to the host used last time, at launch, without asking (ADR 0043).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Resume {
    /// The host was chosen here (or is the local daemon).
    #[default]
    No,
    /// Connecting to the remembered host. If that fails, the connect dialog opens.
    Trying,
    /// It failed and the connect dialog asked. If the retries get through after all,
    /// that dialog closes.
    Asked,
}
