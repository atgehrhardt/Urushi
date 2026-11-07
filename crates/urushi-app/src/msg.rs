//! Messages (everything that can happen) and effects (everything the app asks the
//! outside world to do). `AppState::update` turns one into the other, and does no IO.

use urushi_client::v1::{self, notification, request, response::Outcome};
use urushi_ui::Point;

use crate::config::ConnectTarget;
use crate::keys::KeyPress;
use crate::state::{Field, SettingToggle};

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
    /// The window became narrow (`true`) or wide.
    SetCompact(bool),
    /// In compact mode, go back to the thread list.
    ShowSidebar,
    /// Open the "add project" dialog.
    OpenAddProject,
    /// Open the "new thread" dialog for a project.
    OpenNewThread(String),
    /// Ask the daemon what deleting a thread would destroy, then show the warning.
    RequestDeleteThread(String),
    /// Open settings.
    OpenSettings,
    /// Open the "connect to host" dialog.
    OpenConnect,

    // ---- tabs and panes --------------------------------------------------------------
    /// Show a tab.
    SelectTab(String),
    /// Open a tab in the current thread.
    NewTab,
    /// Close a tab.
    CloseTab(String),
    /// Split the focused pane.
    Split(v1::Direction),
    /// Close the focused pane.
    ClosePane,
    /// The terminal area moved or resized (logical pixels).
    AreaChanged(urushi_ui::Rect),
    /// Mouse pressed in the terminal area.
    AreaPress(Point),
    /// Mouse moved while pressed.
    AreaDrag(Point),
    /// Mouse released.
    AreaRelease(Point),
    /// Wheel over the terminal area, in lines (positive = back into history).
    AreaWheel(Point, i32),

    // ---- dialogs -----------------------------------------------------------------------
    /// Focus a dialog field.
    FocusField(Field),
    /// Submit the open dialog.
    Submit,
    /// Close the open dialog.
    CloseDialog,
    /// Confirm deletion in the warning dialog.
    ConfirmDelete,
    /// Flip a settings switch.
    Toggle(SettingToggle),
    /// Open the system folder picker (desktop).
    BrowseFolder,
    /// The folder picker returned.
    FolderPicked(Option<String>),
    /// Hide the error banner.
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

/// What to do when a request completes.
#[derive(Clone, Debug, PartialEq)]
pub enum Followup {
    /// Nothing beyond showing errors.
    None,
    /// The daemon attached a pane (outcome: first screen).
    Attached(String),
    /// Select the new thread.
    ThreadCreated,
    /// Show the deletion warning.
    DeletionPlan,
    /// Select the new tab.
    TabCreated,
    /// Focus the new pane.
    PaneCreated,
    /// Show settings.
    Settings,
    /// Show tailnet status in settings.
    TailnetStatus,
    /// Close the dialog that made the request.
    CloseDialog,
}

/// Something the app needs done.
#[derive(Clone, Debug)]
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
}
