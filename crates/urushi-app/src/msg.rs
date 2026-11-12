//! Messages (everything that can happen) and effects (everything the app asks the
//! outside world to do). `AppState::update` turns one into the other, and does no IO.

use urushi_client::v1::{self, notification, request, response::Outcome};
use urushi_ui::Point;

use crate::config::ConnectTarget;
use crate::keys::KeyPress;
use crate::organize::{DropTarget, MenuPage, MenuTarget, Prompt, Select, SidebarTab};
use crate::state::{Field, FieldCommand, Launch, SettingToggle, TaskLink};

mod board;
mod filter;
mod issue;
mod jira;
mod pane;
mod quick;
mod sidebar;
mod threads;
mod updates;

pub use board::BoardMsg;
pub use filter::FilterMsg;
pub use issue::IssueMsg;
pub use jira::JiraMsg;
pub use pane::PaneMsg;
pub use quick::QuickMsg;
pub use sidebar::SidebarMsg;
pub use threads::ThreadMsg;
pub use updates::UpdateMsg;

/// Something that happened.
#[derive(Clone, Debug)]
pub enum Msg {
    // ---- connection ------------------------------------------------------------------
    /// Connected and received the first workspace snapshot.
    Connected {
        /// Handshake reply.
        welcome: v1::Welcome,
        /// Initial workspace.
        workspace: v1::Workspace,
    },
    /// The connection dropped or couldn't be made. The backend retries.
    Disconnected(String),
    /// An event from the daemon.
    Daemon(v1::event::Body),
    /// A request finished.
    Response {
        /// What to do with the result.
        followup: Followup,
        /// The outcome, or an error message.
        result: Result<Outcome, String>,
    },

    // ---- sidebar ---------------------------------------------------------------------
    /// Show a thread.
    SelectThread(String),
    /// Open a web page (a thread's Jira issue or pull request) in the system browser.
    OpenUrl(String),
    /// The window became narrow (`true`) or wide.
    SetCompact(bool),
    /// In compact mode, go back to the thread list.
    ShowSidebar,
    /// Open the "add project" dialog.
    OpenAddProject,
    /// Open the "new thread" dialog. `repo: None` picks the first repo; `project` files
    /// the new thread under that project.
    OpenNewThread {
        /// Repo id.
        repo: Option<String>,
        /// Project id.
        project: Option<String>,
    },
    /// Show projects or repos in the sidebar.
    SidebarTab(SidebarTab),
    /// Collapse or expand a sidebar section.
    ToggleSection(String),
    /// Where a mouse press happened (sent by the runner before the click it causes).
    Pointer(Point),
    /// Open a context menu at the last press.
    OpenMenu(MenuTarget),
    /// Switch the context menu's page.
    MenuPage(MenuPage),
    /// Close the context menu.
    CloseMenu,
    /// Pin or unpin a thread.
    TogglePin(String),
    /// A thread's unread mark or notes, or a project's color (`update/thread_extras.rs`).
    Thread(ThreadMsg),
    /// Set (or with `None`, clear) a thread's status.
    SetStatus {
        /// Thread id.
        thread: String,
        /// Status label.
        status: Option<String>,
    },
    /// File a thread under a project (`None`: ungroup it).
    MoveThread {
        /// Thread id.
        thread: String,
        /// Project id.
        project: Option<String>,
    },
    /// Ask for a line of text (rename, new status, new project).
    OpenPrompt(Prompt),
    /// Delete a project (its threads become ungrouped).
    DeleteProject(String),
    /// Open a project's settings: its parent branch and parent-branch thread.
    OpenProjectSettings(String),
    /// A change in the project-settings dialog (`update/project_settings.rs`).
    ProjectSettings(crate::project_settings::ProjectSettingsMsg),
    /// The desktop sidebar: collapsing, resizing, sliding (`update/sidebar.rs`).
    Sidebar(SidebarMsg),
    /// A thread started being dragged.
    DragThread(String),
    /// The pointer moved during a drag.
    DragMove(Point),
    /// The drag was released over a drop target.
    DropThread(DropTarget),
    /// A dragged thread was released over another thread (by id): it takes its place
    /// and joins its group (project, pin and status).
    DropOnThread(String),
    /// Move a thread to `index` in the workspace's list of threads (counted without it).
    ReorderThread {
        /// Thread id.
        thread: String,
        /// New position.
        index: u32,
    },
    /// The drag was released anywhere else.
    DragCancel,
    /// A project's header started being dragged.
    DragProject(String),
    /// A dragged project was released over another project (by id): it takes its place.
    DropProject(String),
    /// Move a project to `index` in the list (counted without it).
    MoveProject {
        /// Project id.
        project: String,
        /// New position.
        index: u32,
    },
    /// A board's tab started being dragged.
    DragBoard(String),
    /// A dragged board was released over another board's tab (by id): it takes its place.
    DropBoard(String),
    /// Open or close a selector in the new-thread dialog.
    ToggleSelect(Select),
    /// Choose the repo in the new-thread dialog.
    PickRepo(String),
    /// Choose the branch to start from (`None`: the repo's default).
    PickBase(Option<String>),
    /// Choose the project to file the new thread under (`None`: no project).
    PickProject(Option<String>),
    /// Ask the daemon what deleting a thread would destroy, then show the warning.
    RequestDeleteThread(String),
    /// Open settings.
    OpenSettings,
    /// Open the "connect to host" dialog.
    OpenConnect,
    /// Connect to a remote host offered in the connect dialog: one connected to
    /// before, or a LAN host this device paired with (its `host:port`).
    ConnectTo(String),
    /// Pairing with a LAN host finished: its `host:port`, or why it failed.
    LanPaired(Result<String, String>),
    /// The LAN hosts this device paired with.
    LanHosts(Vec<PairedHost>),
    /// The remote hosts this device connected to, most recent first (ADR 0043).
    RecentHosts(Vec<String>),
    /// LAN access on the host (Settings).
    Lan(LanMsg),
    /// Updating the app (`update/updates.rs`).
    AppUpdate(UpdateMsg),

    // ---- the quick terminal (`update/quick.rs`) --------------------------------------
    /// The quick-terminal panel.
    Quick(QuickMsg),

    // ---- task boards (`update/board.rs`) --------------------------------------------
    /// Slide the board panel out, or back in.
    ToggleBoard,
    /// The panel's slide animation advanced (0 = hidden, 1 = shown).
    BoardReveal(f32),
    /// A click on the panel's background (it covers the terminals; nothing happens).
    BoardBackground,
    /// Show a board.
    SelectBoard(String),
    /// On a phone, which shows one column at a time: show this column (its tab).
    ShowColumn(String),
    /// A finger swiped sideways by `dx` logical pixels (negative: to the left). On a
    /// phone's board it shows the next column (left) or the previous one (right).
    Swipe(f32),
    /// Open the editor for a new task in a column.
    NewTask {
        /// Board id.
        board: String,
        /// Column id.
        column: String,
    },
    /// A task card was pressed (releasing without dragging opens it).
    PressTask(String),
    /// The mouse button came up after a press outside the terminals, without a drag.
    PressEnded,
    /// A task card started being dragged.
    DragTask(String),
    /// A dragged task was released over a column (`before`: the card it landed on).
    DropTask {
        /// Column id.
        column: String,
        /// The task it goes above; `None` for the bottom.
        before: Option<String>,
    },
    /// While a task is dragged: what is under the pointer (its `on_drop` message, if
    /// any), so the board can show where the task would land.
    DragOver(Option<Box<Msg>>),
    /// In the task editor: choose the column.
    PickColumn(String),
    /// Check or uncheck a subtask.
    ToggleSubtask(String),
    /// Delete a subtask.
    DeleteSubtask(String),
    /// Remove a not-yet-created subtask from a new task.
    RemoveDraftSubtask(usize),
    /// Start a thread for a task or subtask, or go to the one already started.
    StartThread(TaskLink),
    /// In the new-thread dialog for a task: start it in an existing thread (`true`) or
    /// a new one.
    UseExistingThread(bool),
    /// In the new-thread dialog for a task: the existing thread to start it in.
    PickExistingThread(String),
    /// In the task editor: delete the task.
    DeleteTask,
    /// A card acted on from outside the editor (its context menu).
    Board(BoardMsg),
    /// Jira: signing in, importing and syncing boards (`update/jira.rs`).
    Jira(JiraMsg),
    /// Filtering the shown board (`update/filter.rs`).
    Filter(FilterMsg),
    /// The task editor's Jira side (`update/issue.rs`).
    Issue(IssueMsg),
    /// The window's height changed (logical pixels).
    WindowHeight(f32),
    /// Settings: choose a color scheme (`None`: custom colors).
    LookScheme(Option<urushi_core::Scheme>),
    /// Settings: choose how the logo is drawn.
    LookLogo(urushi_core::LogoStyle),
    /// Ask to delete a board.
    OpenDeleteBoard(String),
    /// Confirmed: delete the board.
    ConfirmDeleteBoard,
    /// Time passed (ms since the epoch): refresh the agent usage.
    Tick(i64),
    /// The window gained (`true`) or lost keyboard focus.
    WindowFocused(bool),
    /// Running indicators' brightness moved (0 to 1), while an agent works.
    Pulse(f32),

    // ---- tabs and panes --------------------------------------------------------------
    /// Show a tab.
    SelectTab(String),
    /// Choose what a new tab in the current thread runs.
    NewTab,
    /// Open a tab in the current thread running this.
    Launch(Launch),
    /// Close a tab.
    CloseTab(String),
    /// Split the focused pane.
    Split(v1::Direction),
    /// Close the focused pane.
    ClosePane,
    /// A pane acted on from its context menu.
    Pane(PaneMsg),
    /// The terminal area moved or resized (logical pixels).
    AreaChanged(urushi_ui::Rect),
    /// Mouse pressed in the terminal area.
    AreaPress(Point),
    /// Mouse moved while pressed.
    AreaDrag(Point),
    /// Mouse released.
    AreaRelease(Point),
    /// The pointer moved over the terminal area with no button held.
    AreaHover(Point),
    /// The right or middle button went down or up over the terminal area.
    AreaButton {
        /// Where.
        at: Point,
        /// Which button.
        button: v1::MouseButton,
        /// Down (`true`) or up.
        pressed: bool,
    },
    /// The modifier keys held changed (Shift+drag selects even when an app takes the mouse).
    Modifiers(crate::keys::Mods),
    /// Wheel over the terminal area, in lines (positive = back into history).
    AreaWheel(Point, i32),

    // ---- dialogs -----------------------------------------------------------------------
    /// A press in the focused text field at byte offset `caret`: `clicks` counts quick
    /// successive clicks (2 selects a word, 3 everything); `extend` is Shift.
    FieldPress {
        /// Where in the text.
        caret: usize,
        /// Extend the selection (Shift held).
        extend: bool,
        /// 1, 2 or 3.
        clicks: u8,
    },
    /// Dragging from a press in a text field reached byte offset `caret`.
    FieldDrag(usize),
    /// An editing command on the focused text field (shortcut or context menu).
    EditField(FieldCommand),
    /// Focus a dialog field.
    FocusField(Field),
    /// Submit the open dialog.
    Submit,
    /// Close the open dialog.
    CloseDialog,
    /// Confirm deletion in the warning dialog.
    ConfirmDelete,
    /// Ask to restart the local daemon (shows what will close).
    OpenRestartDaemon,
    /// Confirmed: stop the daemon, then reconnect (which starts this app's daemon).
    ConfirmRestartDaemon,
    /// The old daemon has stopped (or stopping failed).
    DaemonStopped(Result<(), String>),
    /// Flip a settings switch.
    Toggle(SettingToggle),
    /// A choice in the setup wizard.
    Setup(crate::task_agent::SetupMsg),
    /// Settings: the agent threads started from tasks run.
    SetTaskAgent(urushi_core::Agent),
    /// Settings: the mode the task agent starts in.
    SetTaskMode(urushi_core::AgentMode),
    /// Open the system folder picker (desktop).
    BrowseFolder,
    /// The folder picker returned.
    FolderPicked(Option<String>),
    /// A file or folder was dropped on the window.
    FileDropped {
        /// Its path, on this machine.
        path: String,
        /// Whether it's a folder.
        is_dir: bool,
        /// Whether it was dropped over the terminal area.
        over_terminal: bool,
    },
    /// Dismiss the error.
    DismissError,

    // ---- raw input -----------------------------------------------------------------------
    /// A key press.
    Key(KeyPress),
    /// Committed text (typing, IME).
    Text(String),
    /// Clipboard contents to paste.
    Paste(String),
    /// Copy the selection.
    Copy,
}

/// A LAN host this device paired with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedHost {
    /// `host:port`, to connect to.
    pub address: String,
    /// The host's name.
    pub host_name: String,
}

/// LAN access on the host, from the Settings dialog (ADR 0041).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanMsg {
    /// Show a one-time pairing code.
    StartPairing,
    /// Withdraw the pairing code.
    CancelPairing,
    /// Forget a paired device (its id) and close its connections.
    Revoke(String),
}

/// What to do when a request completes.
#[derive(Clone, Debug, PartialEq)]
pub enum Followup {
    /// Nothing beyond showing errors.
    None,
    /// The daemon attached a pane (outcome: first screen).
    Attached(String),
    /// Select the new thread.
    ThreadCreated,
    /// Show the deletion warning; when closing a parent-branch thread to change its
    /// project's settings, save them once it is closed.
    DeletionPlan(Option<Box<crate::project_settings::PendingSettings>>),
    /// The daemon answered `DeleteThread` for this thread: on failure, show it again
    /// and say why. Never touches dialogs or focus.
    ThreadDeleted(String),
    /// The daemon answered `DeleteThread` for this project's parent-branch thread, closed
    /// to change the project's settings: save them, or on failure show the thread again
    /// and say why. Never touches dialogs or focus.
    ProjectBaseClosed(String, Box<crate::project_settings::PendingSettings>),
    /// Select the new tab.
    TabCreated,
    /// A task was started in an existing thread: go to its new tab.
    TaskTabCreated,
    /// Focus the new pane.
    PaneCreated,
    /// The daemon opened (or returned) its quick terminal.
    QuickTerminal,
    /// Show settings.
    Settings,
    /// Show tailnet status in settings.
    TailnetStatus,
    /// Show LAN access status in settings.
    LanStatus,
    /// LAN access was switched: show the settings, then ask for the LAN status (only
    /// now, since the daemon answers requests concurrently).
    LanSwitched,
    /// Remember which agents are installed.
    Agents,
    /// Remember a repo's branches (for the new-thread dialog).
    Branches(String),
    /// Move this thread into the project just created.
    ProjectCreated(Option<String>),
    /// Close the dialog that made the request.
    CloseDialog,
    /// Show the board just created.
    BoardCreated,
    /// Remember the usage of the agent in this pane.
    Usage(String),
    /// Keep this attachment's thumbnail.
    Thumbnail(String),
    /// Open the attachment (its id and file name) with the system's app for it.
    OpenAttachment {
        /// Attachment id.
        id: String,
        /// File name.
        name: String,
    },
    /// Remember who the daemon is signed in to Jira as.
    JiraAccount,
    /// Keep the daemon's Jira sign-ins (1.29).
    JiraAccounts,
    /// Signed in from the connect dialog: go on to importing a board.
    JiraConnected,
    /// Show the site's boards in the import dialog.
    JiraBoards,
    /// The issues of this board its quick filters let through, as asked by the query
    /// with this number (`BoardPanel::quick_query`).
    QuickFilter(String, u64),
    /// A quick filter was saved on this board: turn it on, or say why it failed.
    QuickFilterSaved(String),
    /// A quick filter was changed: close the dialog, or say why it failed.
    QuickFilterChanged,
    /// Show this task's issue in full in its editor.
    JiraIssue(String),
    /// Show what this task's editor's picker searched for.
    JiraFound(String),
    /// Show the fields a new issue of this kind (a wire `TaskKind`) has.
    JiraCreateFields(i32),
    /// Show what looking for a new release found (Settings).
    UpdateChecked,
    /// The release is installed: restart the daemon, then the app.
    UpdateInstalled,
}

/// Something the app needs done.
#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "requests carry generated protobuf bodies; effects are few and short-lived, so boxing would only add an allocation per request"
)]
pub enum Effect {
    /// Send a request; deliver the outcome as [`Msg::Response`].
    Request {
        /// The request.
        body: request::Body,
        /// What to do with the result.
        followup: Followup,
    },
    /// Send a notification (input, acks, scrolling).
    Notify(notification::Body),
    /// Put text on the clipboard.
    SetClipboard(String),
    /// Read the clipboard and deliver it as [`Msg::Paste`].
    RequestPaste,
    /// Show the folder picker and deliver [`Msg::FolderPicked`].
    PickFolder,
    /// Drop the connection and connect to another daemon.
    Reconnect(ConnectTarget),
    /// Pair with a LAN host using its one-time code; deliver [`Msg::LanPaired`].
    PairLan {
        /// `host` or `host:port`.
        host: String,
        /// The code the host shows.
        code: String,
    },
    /// Drop the connection, stop the local daemon, and deliver [`Msg::DaemonStopped`].
    StopDaemon,
    /// Open the app bundle at this path once this app has quit, and quit (after an
    /// update, ADR 0049).
    Relaunch(String),
    /// Show the system file picker, then read the chosen files and deliver each as
    /// [`BoardMsg::Attach`] for `task`.
    PickFiles {
        /// Task id.
        task: String,
    },
    /// Read these files and deliver each as [`BoardMsg::Attach`] for `task`.
    ReadFiles {
        /// Task id.
        task: String,
        /// Paths on this machine.
        paths: Vec<String>,
    },
    /// Post a system notification (a thread's agent finished or waits, ADR 0017).
    SystemNotification {
        /// The thread's title.
        title: String,
        /// What happened, e.g. "Claude finished".
        body: String,
    },
    /// Open a web page (e.g. an issue in Jira) in the system's browser.
    OpenUrl(String),
    /// Save an attachment's bytes to a temporary file and open it with the system's app.
    OpenFile {
        /// Attachment id (keeps files of the same name apart).
        id: String,
        /// File name.
        name: String,
        /// Contents.
        bytes: Vec<u8>,
    },
}
