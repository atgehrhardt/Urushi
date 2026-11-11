//! `AppState::update`: the only place state changes. Pure: it returns [`Effect`]s
//! instead of doing IO, so every UI flow is unit-testable.

use std::sync::Arc;

use urushi_client::v1::{notification, request::Body, response::Outcome};
use urushi_client::{ScreenState, v1};
use urushi_ui::TextField;

use crate::config::ConnectTarget;
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Connection, Dialog, Field, SettingToggle, ThreadTarget};
use crate::terminal;

fn request(body: Body, followup: Followup) -> Effect {
    Effect::Request { body, followup }
}

impl AppState {
    /// Applies `msg` and returns what needs doing.
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per message kind keeps routing in one place"
    )]
    pub fn update(&mut self, msg: Msg) -> Vec<Effect> {
        // A frame of the running indicators' pulse changes nothing else.
        if let Msg::Pulse(brightness) = msg {
            self.pulse = brightness.clamp(0.0, 1.0);
            return Vec::new();
        }
        let before = crate::errors::BeforeUpdate::of(self);
        let mut fx = Vec::new();
        match msg {
            // A dialog field's Cut/Copy/Paste menu: `update/fields.rs`.
            Msg::OpenMenu(crate::organize::MenuTarget::Field(field)) => {
                self.open_field_menu(field);
            }
            // The desktop sidebar's collapse and width: `update/sidebar.rs`.
            Msg::Sidebar(msg) => self.sidebar_msg(msg),
            msg @ (Msg::DragMove(_) | Msg::DragCancel) if self.sidebar.resizing() => {
                self.sidebar_drag(&msg);
            }
            // Organizing threads and the sidebar: `update/organize.rs`.
            msg @ (Msg::SidebarTab(_)
            | Msg::ToggleSection(_)
            | Msg::Pointer(_)
            | Msg::OpenMenu(_)
            | Msg::MenuPage(_)
            | Msg::CloseMenu
            | Msg::TogglePin(_)
            | Msg::SetStatus { .. }
            | Msg::MoveThread { .. }
            | Msg::OpenPrompt(_)
            | Msg::DeleteProject(_)
            | Msg::DragThread(_)
            | Msg::DragMove(_)
            | Msg::DropThread(_)
            | Msg::DropOnThread(_)
            | Msg::ReorderThread { .. }
            | Msg::DragCancel
            | Msg::DragProject(_)
            | Msg::DropProject(_)
            | Msg::MoveProject { .. }
            | Msg::ToggleSelect(_)
            | Msg::PickRepo(_)
            | Msg::PickBase(_)
            | Msg::PickProject(_)) => self.organize(msg, &mut fx),
            // Project settings: `update/project_settings.rs`.
            msg @ (Msg::OpenProjectSettings(_) | Msg::ProjectSettings(_)) => {
                self.project_settings_msg(msg, &mut fx);
            }
            Msg::Connected { welcome, workspace } => self.on_connected(welcome, workspace, &mut fx),
            Msg::Disconnected(reason) => {
                self.connection = Connection::Disconnected(reason);
                self.reset_panes();
                // A new connection watches nothing until told.
                self.board.watching = None;
            }
            Msg::Daemon(event) => self.on_event(event, &mut fx),
            Msg::Response { followup, result } => self.on_response(followup, result, &mut fx),
            Msg::SelectThread(id) => self.select_thread(id, &mut fx),
            Msg::OpenUrl(url) => fx.push(Effect::OpenUrl(url)),
            Msg::SetCompact(compact) => self.compact = compact,
            Msg::WindowHeight(height) => self.window_height = height,
            Msg::LookScheme(scheme) => {
                if let Some(Dialog::Settings { look, .. }) = &mut self.dialog {
                    look.choose(scheme);
                }
            }
            Msg::LookLogo(style) => {
                if let Some(Dialog::Settings { look, .. }) = &mut self.dialog {
                    look.logo = style;
                }
            }
            Msg::Issue(msg) => self.issue_msg(msg, &mut fx),
            Msg::ShowSidebar => self.sidebar_open = true,
            Msg::SelectTab(id) => self.select_tab(id, &mut fx),
            Msg::OpenAddProject => self.open_add_repo(),
            Msg::OpenNewThread { repo, project } => self.open_new_thread(repo, project, &mut fx),
            Msg::RequestDeleteThread(thread_id) => {
                self.menu = None;
                fx.push(request(
                    Body::PlanThreadDeletion(v1::PlanThreadDeletionRequest { thread_id }),
                    Followup::DeletionPlan(None),
                ));
            }
            Msg::OpenSettings => self.open_settings(&mut fx),
            Msg::OpenConnect => self.open(Dialog::connect(), Some(Field::Host)),
            msg @ (Msg::ConnectTo(_) | Msg::LanPaired(_) | Msg::LanHosts(_)) => {
                self.lan_connect_msg(msg, &mut fx);
            }
            Msg::Lan(msg) => self.lan_msg(msg, &mut fx),
            Msg::NewTab => self.open_launcher(&mut fx),
            Msg::Launch(choice) => self.launch(choice, &mut fx),
            Msg::CloseTab(tab_id) => fx.push(request(
                Body::CloseTab(v1::CloseTabRequest { tab_id }),
                Followup::None,
            )),
            // Splitting and closing panes, and the pane menu: `update/panes.rs`.
            Msg::Split(side) => {
                if let Some(pane) = self.focused_pane() {
                    self.split(pane, side, &mut fx);
                }
            }
            Msg::ClosePane => {
                if let Some(pane) = self.focused_pane() {
                    self.close_pane(&pane, &mut fx);
                }
            }
            Msg::Pane(msg) => self.pane_action(msg, &mut fx),
            // The quick terminal's panel: `update/quick.rs`.
            Msg::Quick(msg) => self.quick_msg(msg, &mut fx),
            Msg::AreaChanged(rect) => {
                if self.area != Some(rect) {
                    self.area = Some(rect);
                    self.sync_panes(&mut fx);
                }
            }
            // A dragged project shows where it would land: `update/organize.rs`.
            Msg::DragOver(target)
                if matches!(
                    self.dragging,
                    Some((crate::organize::Dragged::Project(_), _))
                ) =>
            {
                self.project_drag_over(target.map(|m| *m));
            }
            // A dragged thread shows where it would land: `update/organize.rs`.
            Msg::DragOver(target)
                if matches!(
                    self.dragging,
                    Some((crate::organize::Dragged::Thread(_), _))
                ) =>
            {
                self.thread_drag_over(target.map(|m| *m));
            }
            // A dragged board tab shows where it would land: `update/board.rs`.
            Msg::DragOver(target)
                if matches!(self.dragging, Some((crate::organize::Dragged::Board(_), _))) =>
            {
                self.board_tab_over(target.map(|m| *m));
            }
            // The task-board panel: `update/board.rs`.
            msg @ (Msg::ToggleBoard
            | Msg::BoardReveal(_)
            | Msg::BoardBackground
            | Msg::SelectBoard(_)
            | Msg::NewTask { .. }
            | Msg::PressTask(_)
            | Msg::PressEnded
            | Msg::DragTask(_)
            | Msg::DropTask { .. }
            | Msg::DragBoard(_)
            | Msg::DropBoard(_)
            | Msg::DragOver(_)
            | Msg::PickColumn(_)
            | Msg::ToggleSubtask(_)
            | Msg::DeleteSubtask(_)
            | Msg::RemoveDraftSubtask(_)
            | Msg::StartThread(_)
            | Msg::UseExistingThread(_)
            | Msg::PickExistingThread(_)
            | Msg::DeleteTask
            | Msg::Board(_)
            | Msg::OpenDeleteBoard(_)
            | Msg::ConfirmDeleteBoard) => self.board_msg(msg, &mut fx),
            Msg::Jira(msg) => self.jira_msg(msg, &mut fx),
            Msg::Filter(msg) => self.filter_msg(msg, &mut fx),
            Msg::Tick(now_ms) => self.tick(now_ms, &mut fx),
            Msg::WindowFocused(focused) => self.attention.in_background = !focused,
            Msg::Pulse(_) => {}
            // The mouse over terminals: `update/pointer.rs`.
            msg @ (Msg::AreaPress(_)
            | Msg::AreaDrag(_)
            | Msg::AreaRelease(_)
            | Msg::AreaWheel(..)
            | Msg::AreaHover(_)
            | Msg::AreaButton { .. }
            | Msg::Modifiers(_)) => self.pointer(&msg, &mut fx),
            Msg::FocusField(field) => self.focus = Some(field),
            // Editing text fields with the mouse and shortcuts: `update/fields.rs`.
            Msg::FieldPress {
                caret,
                extend,
                clicks,
            } => self.field_press(caret, extend, clicks),
            Msg::FieldDrag(caret) => {
                if let Some(field) = self.field_mut() {
                    field.place(caret, true);
                }
            }
            Msg::EditField(command) => self.edit_field(command, &mut fx),
            Msg::Submit => self.submit(&mut fx),
            Msg::CloseDialog => self.close_dialog(),
            Msg::ConfirmDelete => self.confirm_delete(&mut fx),
            Msg::OpenRestartDaemon => self.open_restart_daemon(),
            Msg::ConfirmRestartDaemon => self.confirm_restart_daemon(&mut fx),
            Msg::DaemonStopped(result) => self.daemon_stopped(result, &mut fx),
            Msg::Toggle(toggle) => self.toggle(toggle, &mut fx),
            // The task agent: `update/setup.rs`.
            Msg::Setup(msg) => self.setup_msg(msg, &mut fx),
            Msg::SetTaskAgent(agent) => self.set_task_agent(agent, &mut fx),
            Msg::SetTaskMode(mode) => self.set_task_mode(mode, &mut fx),
            Msg::BrowseFolder => fx.push(Effect::PickFolder),
            Msg::FolderPicked(path) => {
                if let (Some(path), Some(Dialog::AddProject { .. })) = (path, &self.dialog) {
                    Self::add_project(&path, &mut fx);
                }
            }
            Msg::FileDropped {
                path,
                is_dir,
                over_terminal,
            } => self.file_dropped(&path, is_dir, over_terminal, &mut fx),
            Msg::DismissError => self.error = None,
            Msg::Key(key) => self.on_key(&key, &mut fx),
            Msg::Text(text) => self.insert_text(text, v1::input::Kind::Text, &mut fx),
            Msg::Paste(text) => self.insert_text(text, v1::input::Kind::Paste, &mut fx),
            Msg::Copy => self.copy(&mut fx),
        }
        self.want_thumbnails(&mut fx);
        self.watch_shown_board(&mut fx);
        self.attend(&mut fx);
        // An error shown in a dialog goes with it (`errors.rs`).
        crate::errors::forget_dialog_error(self, &before);
        fx
    }

    fn on_connected(
        &mut self,
        welcome: v1::Welcome,
        workspace: v1::Workspace,
        fx: &mut Vec<Effect>,
    ) {
        self.daemon_minor = welcome.protocol_minor;
        self.daemon_version.clone_from(&welcome.daemon_version);
        self.connection = Connection::Connected {
            host: welcome.host_name,
        };
        self.reset_panes();
        self.on_workspace(workspace, fx);
        self.seed_attention();
        self.request_agents(fx);
        self.usage = None;
        self.usage_pending = false;
        self.poll_usage(fx);
        fx.push(request(
            Body::GetSettings(v1::GetSettingsRequest {}),
            Followup::Settings,
        ));
        fx.push(request(
            Body::TailnetStatus(v1::TailnetStatusRequest {}),
            Followup::TailnetStatus,
        ));
        self.lan = None;
        self.request_lan_status(fx);
        self.jira = None;
        self.request_jira_account(fx);
    }

    fn select_thread(&mut self, id: String, fx: &mut Vec<Effect>) {
        self.selected_thread = Some(id);
        self.sidebar_open = false;
        self.selection = None;
        self.sync_panes(fx);
    }

    fn select_tab(&mut self, id: String, fx: &mut Vec<Effect>) {
        if let Some(thread) = self.selected_thread.clone() {
            self.selected_tab.insert(thread, id);
            self.selection = None;
            self.sync_panes(fx);
        }
    }

    fn open_settings(&mut self, fx: &mut Vec<Effect>) {
        let s = self.settings.clone().unwrap_or_default();
        self.open(
            Dialog::Settings {
                look: Box::new(crate::appearance::LookDraft::of(crate::appearance::saved(
                    Some(&s),
                ))),
                prefix: TextField::new(s.branch_prefix),
                shell: TextField::new(s.default_shell),
                message: TextField::multiline(s.agents.map(|a| a.task_message).unwrap_or_default()),
            },
            None,
        );
        fx.push(request(
            Body::GetSettings(v1::GetSettingsRequest {}),
            Followup::Settings,
        ));
        fx.push(request(
            Body::TailnetStatus(v1::TailnetStatusRequest {}),
            Followup::TailnetStatus,
        ));
        self.request_lan_status(fx);
        self.request_jira_account(fx);
    }

    /// Shows the deletion warning for `plan`; `then`: project settings to save once the
    /// thread is closed.
    fn show_deletion_plan(
        &mut self,
        plan: v1::DeletionPlan,
        then: Option<Box<crate::project_settings::PendingSettings>>,
    ) {
        let title = self
            .workspace
            .thread(&plan.thread_id)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        self.open(Dialog::DeleteThread { title, plan, then }, None);
    }

    /// Typed or pasted text goes to the focused dialog field, otherwise to the focused pane.
    fn insert_text(
        &mut self,
        text: String,
        to_terminal: fn(String) -> v1::input::Kind,
        fx: &mut Vec<Effect>,
    ) {
        if let Some(field) = self.field_mut() {
            field.insert(&text);
            self.after_field_edit();
        } else if self.keyboard_surface().is_some()
            && let Some(pane) = self.focused_pane()
        {
            self.claim_sizes(fx);
            fx.push(input(pane, to_terminal(text)));
        }
    }

    fn copy(&self, fx: &mut Vec<Effect>) {
        if let Some((pane, selection)) = &self.selection
            && let Some(screen) = self.screens.get(pane)
        {
            fx.push(Effect::SetClipboard(terminal::selection_text(
                screen, selection,
            )));
        }
    }

    pub(super) fn open(&mut self, dialog: Dialog, focus: Option<Field>) {
        self.dialog = Some(dialog);
        self.focus = focus;
    }

    fn close_dialog(&mut self) {
        // The connect dialog is the only way forward when no daemon is chosen yet, and a
        // restart under way can't be cancelled.
        if self.target == ConnectTarget::Ask && matches!(self.dialog, Some(Dialog::Connect { .. }))
            || matches!(
                self.dialog,
                Some(Dialog::RestartDaemon {
                    restarting: true,
                    ..
                })
            )
        {
            return;
        }
        // Unsaved task edits: ask first.
        if self.hold_editor_close() {
            return;
        }
        // Closing the setup wizard means "not now".
        if matches!(self.dialog, Some(Dialog::Setup { .. })) {
            self.setup.dismissed = true;
        }
        self.dialog = None;
        self.focus = None;
    }

    fn on_workspace(&mut self, workspace: v1::Workspace, fx: &mut Vec<Effect>) {
        self.deletions.report(workspace);
        self.apply_workspace(fx);
    }

    /// Shows the daemon's last reported workspace, without the threads being deleted,
    /// and keeps the selection and attached panes in step with it.
    fn apply_workspace(&mut self, fx: &mut Vec<Effect>) {
        let had_quick = self.workspace.quick_terminal_raw().is_some();
        self.workspace = self.deletions.visible();
        self.follow_quick_terminal(had_quick);
        let thread_exists = |id: &String| self.workspace.thread(id).is_some();
        if !self.selected_thread.as_ref().is_some_and(thread_exists) {
            self.selected_thread = self.workspace.raw().threads.first().map(|t| t.id.clone());
        }
        // A local focus lasts until the daemon confirms it, and never outlives its pane
        // (closed, or its process exited): the daemon has already moved focus on.
        let overrides = std::mem::take(&mut self.focused_override);
        self.focused_override = overrides
            .into_iter()
            .filter(|(tab, pane)| {
                let confirmed = self.confirmed_focus(tab).as_ref() == Some(pane);
                !confirmed && self.group_has_pane(tab, pane)
            })
            .collect();
        self.sync_panes(fx);
    }

    fn on_event(&mut self, event: v1::event::Body, fx: &mut Vec<Effect>) {
        match event {
            v1::event::Body::WorkspaceChanged(workspace) => {
                self.on_workspace(workspace, fx);
                self.refresh_quick_filter(fx);
            }
            v1::event::Body::ScreenUpdate(update) => {
                if let Some(screen) = self.screens.get_mut(&update.pane_id) {
                    let (pane_id, seq) = (update.pane_id.clone(), update.seq);
                    Arc::make_mut(screen).apply(update);
                    fx.push(Effect::Notify(notification::Body::AckScreen(
                        v1::AckScreen { pane_id, seq },
                    )));
                }
            }
            v1::event::Body::PaneExited(exited) => {
                self.screens.remove(&exited.pane_id);
                self.sizes.remove(&exited.pane_id);
            }
            v1::event::Body::PaneBell(_) => {}
        }
    }

    fn on_response(
        &mut self,
        followup: Followup,
        result: Result<Outcome, String>,
        fx: &mut Vec<Effect>,
    ) {
        if followup == Followup::QuickTerminal {
            return self.quick_terminal_answered(result.err(), fx);
        }
        let followup = match followup {
            Followup::ThreadDeleted(thread) => {
                self.thread_deleted(&thread, result.err(), fx);
                return;
            }
            Followup::ProjectBaseClosed(thread, pending) => {
                self.project_base_closed(&thread, *pending, result.err(), fx);
                return;
            }
            other => other,
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            // An older daemon, or no agents: the picker just offers a terminal.
            Err(_) if followup == Followup::Agents => return,
            // Without branches the selector just offers the repo's default.
            Err(_) if matches!(followup, Followup::Branches(_)) => return,
            // No thumbnail: the attachment shows as a file (it isn't asked for again).
            Err(_) if matches!(followup, Followup::Thumbnail(_)) => return,
            // The status bar just shows no usage.
            Err(_) if matches!(followup, Followup::Usage(_)) => {
                if let Followup::Usage(pane) = followup {
                    self.usage_answered(pane, None);
                }
                return;
            }
            Err(message) if self.jira_failed(&followup, &message) => return,
            Err(message) if self.quick_filter_failed(&followup, &message) => return,
            Err(message) => {
                if let Followup::Attached(pane) = &followup {
                    self.attaching.remove(pane);
                }
                self.error = Some(message);
                return;
            }
        };
        let Some((followup, outcome)) = self.attachment_answered(followup, outcome, fx) else {
            return;
        };
        let Some((followup, outcome)) = self.jira_answered(followup, outcome, fx) else {
            return;
        };
        match (followup, outcome) {
            (Followup::Attached(pane), Outcome::Screen(update)) => {
                if self.attaching.remove(&pane) {
                    let seq = update.seq;
                    let mut screen = ScreenState::default();
                    screen.apply(update);
                    self.screens.insert(pane.clone(), Arc::new(screen));
                    fx.push(Effect::Notify(notification::Body::AckScreen(
                        v1::AckScreen { pane_id: pane, seq },
                    )));
                }
            }
            (Followup::ThreadCreated, Outcome::Thread(thread)) => self.thread_created(thread, fx),
            (Followup::Agents, Outcome::Agents(agents)) => {
                self.set_agents(&agents);
                self.maybe_open_setup();
            }
            (Followup::Branches(repo), Outcome::Branches(branches)) => {
                self.branches = Some((repo, branches));
            }
            (Followup::ProjectCreated(thread), Outcome::Folder(project)) => {
                self.project_created(thread, &project, fx);
            }
            (Followup::DeletionPlan(then), Outcome::DeletionPlan(plan)) => {
                self.show_deletion_plan(plan, then);
            }
            (Followup::TaskTabCreated, Outcome::Tab(tab)) => self.task_tab_created(tab, fx),
            (Followup::TabCreated, Outcome::Tab(tab)) => {
                self.selected_tab.insert(tab.thread_id, tab.id);
                self.sync_panes(fx);
            }
            (Followup::PaneCreated, Outcome::PaneCreated(created)) => {
                self.focused_override
                    .insert(created.tab_id, created.pane_id);
            }
            (Followup::Settings, Outcome::Settings(settings)) => {
                self.settings = Some(settings);
                self.maybe_open_setup();
            }
            (Followup::TailnetStatus, Outcome::TailnetStatus(status)) => {
                self.tailnet = Some(status);
            }
            (followup @ (Followup::LanStatus | Followup::LanSwitched), outcome) => {
                self.lan_answered(&followup, outcome, fx);
            }
            (Followup::Usage(pane), Outcome::AgentUsage(usage)) => {
                self.usage_answered(pane, Some(usage));
            }
            (Followup::BoardCreated, Outcome::Board(board)) => {
                self.board.selected = Some(board.id);
                self.dialog = None;
                self.focus = None;
            }
            (Followup::CloseDialog, _) => {
                self.dialog = None;
                self.focus = None;
            }
            _ => {}
        }
    }

    /// The new thread exists: show it.
    fn thread_created(&mut self, thread: v1::Thread, fx: &mut Vec<Effect>) {
        // Started from a task: the board makes way for the new thread.
        if matches!(self.dialog, Some(Dialog::NewThread { task: Some(_), .. })) {
            self.board.open = false;
        }
        self.selected_thread = Some(thread.id);
        self.close_dialog();
        self.sync_panes(fx);
        // The new thread shows the launcher; make sure it lists what's installed.
        self.request_agents(fx);
    }

    fn submit(&mut self, fx: &mut Vec<Effect>) {
        match &self.dialog {
            Some(Dialog::AddProject { path }) if !path.text().trim().is_empty() => {
                Self::add_project(path.text(), fx);
            }
            Some(Dialog::AddProject { .. }) if self.can_pick_folders() => {
                fx.push(Effect::PickFolder);
            }
            Some(Dialog::NewThread {
                target: ThreadTarget::Existing(_),
                ..
            }) => self.submit_task_in_thread(fx),
            Some(Dialog::NewThread {
                repo,
                project,
                title,
                branch,
                base,
                task,
                ..
            }) => {
                // From a task, with a task agent chosen: the daemon starts it with the
                // task as its prompt. Otherwise the thread opens on the launcher.
                let start_task_agent = task.is_some() && self.task_agent().is_some();
                let body = Body::CreateThread(v1::CreateThreadRequest {
                    project_id: repo.clone(),
                    title: title.text().trim().to_owned(),
                    base_ref: base.clone().unwrap_or_default(),
                    branch: branch.text().trim().to_owned(),
                    // The thread opens on the launcher, so the user picks what runs first.
                    skip_initial_tab: true,
                    folder_id: project.clone().unwrap_or_default(),
                    task_id: task.as_ref().map(|l| l.task.clone()).unwrap_or_default(),
                    subtask_id: task
                        .as_ref()
                        .and_then(|l| l.subtask.clone())
                        .unwrap_or_default(),
                    start_task_agent,
                });
                fx.push(request(body, Followup::ThreadCreated));
            }
            Some(Dialog::Prompt { .. }) => self.submit_prompt(fx),
            Some(Dialog::ProjectSettings(_)) => self.submit_project_settings(fx),
            Some(Dialog::Setup { .. }) => self.setup_enter(fx),
            Some(Dialog::NestTask { task, query }) => {
                let first = crate::board::nest_candidates(&self.workspace, task, query.text())
                    .into_iter()
                    .next();
                if let Some((parent, _)) = first {
                    let task = task.clone();
                    self.card_action(crate::msg::BoardMsg::Nest { task, parent }, fx);
                }
            }
            Some(Dialog::Task { .. }) => self.submit_task(fx),
            Some(Dialog::ConnectJira { .. } | Dialog::ImportJira { .. }) => self.submit_jira(fx),
            Some(Dialog::QuickFilter(_)) => self.submit_quick_filter(fx),
            Some(Dialog::NewTab { highlighted }) => {
                let choices = self.launch_choices();
                let choice = choices[(*highlighted).min(choices.len() - 1)];
                self.launch(choice, fx);
            }
            Some(Dialog::Settings {
                prefix,
                shell,
                message,
                look,
            }) => {
                // Only a changed look is sent: older daemons don't keep one.
                let look = look.appearance();
                let changed = look != crate::appearance::saved(self.settings.as_ref());
                let patch = v1::SettingsPatch {
                    branch_prefix: Some(prefix.text().to_owned()),
                    default_shell: Some(shell.text().trim().to_owned()),
                    appearance: changed.then(|| urushi_proto::convert::appearance_to_proto(look)),
                    // Older daemons don't keep one.
                    task_message: self
                        .daemon_supports_task_message()
                        .then(|| message.text().to_owned()),
                    ..Default::default()
                };
                fx.push(request(
                    Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
                    Followup::Settings,
                ));
                self.dialog = None;
                self.focus = None;
            }
            Some(Dialog::Connect { .. }) => self.submit_connect(fx),
            // Enter never confirms a deletion: the user must click the Delete button.
            _ => {}
        }
    }

    fn toggle(&mut self, toggle: SettingToggle, fx: &mut Vec<Effect>) {
        match toggle {
            SettingToggle::DeleteRemoteBranches | SettingToggle::UpdateBaseFromRemote => {
                let s = self.settings.clone().unwrap_or_default();
                let mut patch = v1::SettingsPatch::default();
                if toggle == SettingToggle::DeleteRemoteBranches {
                    patch.delete_remote_branches = Some(!s.delete_remote_branches);
                } else {
                    patch.update_base_from_remote = Some(!s.update_base_from_remote);
                }
                fx.push(request(
                    Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
                    Followup::Settings,
                ));
            }
            SettingToggle::SkipPermissions(_) if !self.daemon_supports_agents() => {}
            SettingToggle::SkipPermissions(agent) => {
                let current = self.skips_permissions(agent);
                let mut patch = v1::SettingsPatch::default();
                let flag = match agent {
                    urushi_core::Agent::Claude => &mut patch.claude_skip_permissions,
                    urushi_core::Agent::Codex => &mut patch.codex_skip_permissions,
                    urushi_core::Agent::Cursor => &mut patch.cursor_skip_permissions,
                };
                *flag = Some(!current);
                fx.push(request(
                    Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
                    Followup::Settings,
                ));
            }
            SettingToggle::TaskSend | SettingToggle::TaskMessageCustom => {
                self.toggle_task_message(toggle, fx);
            }
            SettingToggle::LanAccess => self.toggle_lan(fx),
            SettingToggle::ServeTailnet => {
                let serving = self.tailnet.as_ref().is_some_and(|t| t.serving);
                fx.push(request(
                    Body::SetTailnetServe(v1::SetTailnetServeRequest { serve: !serving }),
                    Followup::TailnetStatus,
                ));
            }
        }
    }
}

impl AppState {
    /// Whether the system folder picker can choose a folder the daemon can open: only on
    /// desktop, connected to the daemon on this machine.
    /// Whether this device can choose files to attach (desktop; files dropped on the
    /// window work too).
    pub(crate) fn can_pick_files() -> bool {
        crate::platform::HAS_FILE_PICKER
    }

    pub(crate) fn can_pick_folders(&self) -> bool {
        crate::platform::HAS_FOLDER_PICKER && matches!(self.target, ConnectTarget::Local(_))
    }
}

fn input(pane_id: String, kind: v1::input::Kind) -> Effect {
    Effect::Notify(notification::Body::Input(v1::Input {
        pane_id,
        kind: Some(kind),
    }))
}

mod attachments;
mod attention;
mod board;
mod cards;
mod daemon;
mod deletion;
mod fields;
mod filter;
mod intake;
mod issue;
mod issue_deletion;
mod jira;
mod keys;
pub(crate) mod lan;
mod launcher;
mod organize;
mod panes;
mod pointer;
mod project_settings;
mod quick;
mod quick_filter;
mod setup;
mod sidebar;
mod sidebar_drag;
mod subtask_drafts;
mod task_nav;
mod task_threads;
mod tasks;
mod usage;

#[cfg(test)]
mod tests;
