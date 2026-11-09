//! `AppState::update`: the only place state changes. Pure: it returns [`Effect`]s
//! instead of doing IO, so every UI flow is unit-testable.

use std::sync::Arc;

use urushi_client::v1::{notification, request::Body, response::Outcome};
use urushi_client::{ScreenState, WorkspaceMirror, v1};
use urushi_ui::TextField;

use crate::config::ConnectTarget;
use crate::keys::{KeyPress, Named};
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
        let mut fx = Vec::new();
        match msg {
            // A dialog field's Cut/Copy/Paste menu: `update/fields.rs`.
            Msg::OpenMenu(crate::organize::MenuTarget::Field(field)) => {
                self.open_field_menu(field);
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
            | Msg::DragCancel
            | Msg::ToggleSelect(_)
            | Msg::PickRepo(_)
            | Msg::PickBase(_)
            | Msg::PickProject(_)) => self.organize(msg, &mut fx),
            Msg::Connected { welcome, workspace } => self.on_connected(welcome, workspace, &mut fx),
            Msg::Disconnected(reason) => {
                self.connection = Connection::Disconnected(reason);
                self.reset_panes();
            }
            Msg::Daemon(event) => self.on_event(event, &mut fx),
            Msg::Response { followup, result } => self.on_response(followup, result, &mut fx),
            Msg::SelectThread(id) => self.select_thread(id, &mut fx),
            Msg::SetCompact(compact) => self.compact = compact,
            Msg::ShowSidebar => self.sidebar_open = true,
            Msg::SelectTab(id) => self.select_tab(id, &mut fx),
            Msg::OpenAddProject => self.open_add_repo(),
            Msg::OpenNewThread { repo, project } => self.open_new_thread(repo, project, &mut fx),
            Msg::RequestDeleteThread(thread_id) => {
                self.menu = None;
                fx.push(request(
                    Body::PlanThreadDeletion(v1::PlanThreadDeletionRequest { thread_id }),
                    Followup::DeletionPlan,
                ));
            }
            Msg::OpenSettings => self.open_settings(&mut fx),
            Msg::OpenConnect => self.open(
                Dialog::Connect {
                    host: TextField::default(),
                },
                Some(Field::Host),
            ),
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
            Msg::AreaChanged(rect) => {
                if self.area != Some(rect) {
                    self.area = Some(rect);
                    self.sync_panes(&mut fx);
                }
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
            Msg::Tick(now_ms) => self.tick(now_ms, &mut fx),
            Msg::WindowFocused(focused) => self.attention.in_background = !focused,
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
        self.attend(&mut fx);
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
                prefix: TextField::new(s.branch_prefix),
                shell: TextField::new(s.default_shell),
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
    }

    fn confirm_delete(&mut self, fx: &mut Vec<Effect>) {
        if let Some(Dialog::DeleteThread { plan, deleting, .. }) = &mut self.dialog
            && !*deleting
        {
            *deleting = true;
            let body = Body::DeleteThread(v1::DeleteThreadRequest {
                thread_id: plan.thread_id.clone(),
                token: plan.token.clone(),
            });
            fx.push(request(body, Followup::CloseDialog));
        }
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
        } else if !self.terminals_covered()
            && let Some(pane) = self.focused_pane()
        {
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
        // Closing the setup wizard means "not now".
        if matches!(self.dialog, Some(Dialog::Setup { .. })) {
            self.setup.dismissed = true;
        }
        self.dialog = None;
        self.focus = None;
    }

    pub(super) fn reset_panes(&mut self) {
        self.screens.clear();
        self.attaching.clear();
        self.sizes.clear();
        self.selection = None;
        self.drag = None;
    }

    fn on_workspace(&mut self, workspace: v1::Workspace, fx: &mut Vec<Effect>) {
        self.workspace = WorkspaceMirror::new(workspace);
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
                let confirmed = self
                    .workspace
                    .raw()
                    .tabs
                    .iter()
                    .any(|t| &t.id == tab && &t.focused_pane_id == pane);
                !confirmed && self.tab_has_pane(tab, pane)
            })
            .collect();
        self.sync_panes(fx);
    }

    /// Attaches the current tab's panes, detaches others, and keeps terminal sizes in step
    /// with pane rectangles.
    fn sync_panes(&mut self, fx: &mut Vec<Effect>) {
        let wanted = self.pane_rects();
        if self.area.is_none() || !matches!(self.connection, Connection::Connected { .. }) {
            return;
        }
        let wanted_ids: Vec<&String> = wanted.iter().map(|(id, _)| id).collect();
        let stale: Vec<String> = self
            .screens
            .keys()
            .chain(self.attaching.iter())
            .filter(|id| !wanted_ids.contains(id))
            .cloned()
            .collect();
        for pane_id in stale {
            self.screens.remove(&pane_id);
            self.attaching.remove(&pane_id);
            self.sizes.remove(&pane_id);
            fx.push(request(
                Body::DetachPane(v1::DetachPaneRequest { pane_id }),
                Followup::None,
            ));
        }
        for (pane, rect) in wanted {
            let (cols, rows) = terminal::grid_size(rect, self.cell);
            if self.screens.contains_key(&pane) {
                if self.sizes.get(&pane) != Some(&(cols, rows)) {
                    self.sizes.insert(pane.clone(), (cols, rows));
                    let body = Body::SetPaneSize(v1::SetPaneSizeRequest {
                        pane_id: pane,
                        cols: cols.into(),
                        rows: rows.into(),
                    });
                    fx.push(request(body, Followup::None));
                }
            } else if self.attaching.insert(pane.clone()) {
                self.sizes.insert(pane.clone(), (cols, rows));
                let body = Body::AttachPane(v1::AttachPaneRequest {
                    pane_id: pane.clone(),
                    cols: cols.into(),
                    rows: rows.into(),
                });
                fx.push(request(body, Followup::Attached(pane)));
            }
        }
    }

    fn on_event(&mut self, event: v1::event::Body, fx: &mut Vec<Effect>) {
        match event {
            v1::event::Body::WorkspaceChanged(workspace) => self.on_workspace(workspace, fx),
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
            Err(message) => {
                if let Followup::Attached(pane) = &followup {
                    self.attaching.remove(pane);
                }
                if let Some(Dialog::DeleteThread { deleting, .. }) = &mut self.dialog {
                    *deleting = false;
                }
                self.error = Some(message);
                return;
            }
        };
        let Some((followup, outcome)) = self.attachment_answered(followup, outcome, fx) else {
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
            (Followup::DeletionPlan, Outcome::DeletionPlan(plan)) => {
                let title = self
                    .workspace
                    .thread(&plan.thread_id)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                self.open(
                    Dialog::DeleteThread {
                        title,
                        plan,
                        deleting: false,
                    },
                    None,
                );
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
            Some(Dialog::NewTab { highlighted }) => {
                let choices = self.launch_choices();
                let choice = choices[(*highlighted).min(choices.len() - 1)];
                self.launch(choice, fx);
            }
            Some(Dialog::Settings { prefix, shell }) => {
                let patch = v1::SettingsPatch {
                    branch_prefix: Some(prefix.text().to_owned()),
                    default_shell: Some(shell.text().trim().to_owned()),
                    ..Default::default()
                };
                fx.push(request(
                    Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(patch) }),
                    Followup::Settings,
                ));
                self.dialog = None;
                self.focus = None;
            }
            Some(Dialog::Connect { host }) if !host.text().trim().is_empty() => {
                let target = ConnectTarget::Remote(host.text().trim().to_owned());
                self.target = target.clone();
                self.dialog = None;
                self.focus = None;
                self.connection = Connection::Connecting;
                self.workspace = WorkspaceMirror::default();
                self.selected_thread = None;
                self.reset_panes();
                fx.push(Effect::Reconnect(target));
            }
            // Enter never confirms a deletion: the user must click the Delete button.
            _ => {}
        }
    }

    fn toggle(&mut self, toggle: SettingToggle, fx: &mut Vec<Effect>) {
        match toggle {
            SettingToggle::DeleteRemoteBranches => {
                let current = self
                    .settings
                    .as_ref()
                    .is_some_and(|s| s.delete_remote_branches);
                let patch = v1::SettingsPatch {
                    delete_remote_branches: Some(!current),
                    ..Default::default()
                };
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
            SettingToggle::ServeTailnet => {
                let serving = self.tailnet.as_ref().is_some_and(|t| t.serving);
                fx.push(request(
                    Body::SetTailnetServe(v1::SetTailnetServeRequest { serve: !serving }),
                    Followup::TailnetStatus,
                ));
            }
        }
    }

    fn on_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        if key.named == Some(Named::Escape) && (self.dismiss_overlay() || self.board_escape()) {
            return;
        }
        if matches!(self.dialog, Some(Dialog::NewTab { .. }))
            && !(key.mods.is_shortcut() || key.mods.logo)
        {
            self.launcher_dialog_key(key, fx);
        } else if self.dialog.is_some() {
            self.dialog_key(key, fx);
        } else if key.mods.is_shortcut() || key.mods.logo {
            self.shortcut(key, fx);
        } else if self.board.visible() {
            // The panel covers the terminals; typing doesn't reach them.
        } else if self.empty_thread_key(key, fx) {
        } else if let Some(pane) = self.focused_pane()
            && let Some(kind) = key.to_terminal()
        {
            self.selection = None;
            fx.push(input(pane, kind));
        }
    }

    fn shortcut(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        let apple = cfg!(any(target_os = "macos", target_os = "ios"));
        if key.mods.alt
            && let Some(named) = key.named
        {
            let side = match named {
                Named::Left => urushi_core::Direction::Left,
                Named::Right => urushi_core::Direction::Right,
                Named::Up => urushi_core::Direction::Up,
                Named::Down => urushi_core::Direction::Down,
                _ => return,
            };
            self.focus_towards(side, fx);
            return;
        }
        let Some(c) = key.character.as_deref().map(str::to_lowercase) else {
            return;
        };
        let more = match c.as_str() {
            "t" => self.update(Msg::NewTab),
            "w" => self.update(Msg::ClosePane),
            "d" if apple && key.mods.shift => self.update(Msg::Split(v1::Direction::Down)),
            "d" => self.update(Msg::Split(v1::Direction::Right)),
            "e" => self.update(Msg::Split(v1::Direction::Down)),
            "c" => self.update(Msg::Copy),
            "v" => vec![Effect::RequestPaste],
            "," => self.update(Msg::OpenSettings),
            "b" => self.update(Msg::ToggleBoard),
            "n" => {
                // Same repo and project as the thread on screen.
                let current = self
                    .selected_thread
                    .as_ref()
                    .and_then(|t| self.workspace.thread(t));
                let repo = current.map(|t| t.project_id.clone());
                let project = current
                    .map(|t| t.folder_id.clone())
                    .filter(|f| self.workspace.project(f).is_some());
                self.update(Msg::OpenNewThread { repo, project })
            }
            "[" | "{" => self.cycle_tab(-1),
            "]" | "}" => self.cycle_tab(1),
            _ => Vec::new(),
        };
        fx.extend(more);
    }

    fn cycle_tab(&mut self, step: isize) -> Vec<Effect> {
        let Some(thread) = self.selected_thread.clone() else {
            return Vec::new();
        };
        let tabs: Vec<String> = self
            .workspace
            .tabs_of(&thread)
            .iter()
            .map(|t| t.id.clone())
            .collect();
        let Some(current) = self.current_tab().map(|t| t.id.clone()) else {
            return Vec::new();
        };
        let Some(i) = tabs.iter().position(|t| *t == current) else {
            return Vec::new();
        };
        let len = isize::try_from(tabs.len()).unwrap_or(1);
        let next =
            usize::try_from((isize::try_from(i).unwrap_or(0) + step).rem_euclid(len)).unwrap_or(0);
        self.update(Msg::SelectTab(tabs[next].clone()))
    }

    fn focus_towards(&mut self, side: urushi_core::Direction, fx: &mut Vec<Effect>) {
        let Some(tab) = self.current_tab().and_then(|t| self.workspace.tab(&t.id)) else {
            return;
        };
        let Some(current) = self.focused_pane().and_then(|p| p.parse().ok()) else {
            return;
        };
        if let Some(next) = tab.layout().focus_neighbour(current, side) {
            self.focus_pane(&tab.id().to_string(), &next.to_string(), fx);
        }
    }

    fn focus_pane(&mut self, tab: &str, pane: &str, fx: &mut Vec<Effect>) {
        if self.focused_pane().as_deref() != Some(pane) {
            self.focused_override
                .insert(tab.to_owned(), pane.to_owned());
            let body = Body::FocusPane(v1::FocusPaneRequest {
                tab_id: tab.to_owned(),
                pane_id: pane.to_owned(),
            });
            fx.push(request(body, Followup::None));
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
mod fields;
mod intake;
mod launcher;
mod organize;
mod panes;
mod pointer;
mod setup;
mod task_threads;
mod tasks;
mod usage;

#[cfg(test)]
mod tests;
