//! `AppState::update`: the only place state changes. Pure: it returns [`Effect`]s
//! instead of doing IO, so every UI flow is unit-testable.

use std::sync::Arc;

use urushi_client::v1::{notification, request::Body, response::Outcome};
use urushi_client::{ScreenState, WorkspaceMirror, v1};
use urushi_ui::{Point, TextField};

use crate::config::ConnectTarget;
use crate::keys::{KeyPress, Named};
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Connection, Dialog, Drag, Field, Selection, SettingToggle};
use crate::terminal;

fn request(body: Body, followup: Followup) -> Effect {
    Effect::Request { body, followup }
}

impl AppState {
    /// Applies `msg` and returns what needs doing.
    pub fn update(&mut self, msg: Msg) -> Vec<Effect> {
        let mut fx = Vec::new();
        match msg {
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
            Msg::OpenAddProject => self.open(
                Dialog::AddProject {
                    path: TextField::default(),
                },
                Some(Field::ProjectPath),
            ),
            Msg::OpenNewThread(project) => self.open(
                Dialog::NewThread {
                    project,
                    title: TextField::default(),
                    base: TextField::default(),
                },
                Some(Field::ThreadTitle),
            ),
            Msg::RequestDeleteThread(thread_id) => {
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
            Msg::NewTab => self.new_tab(&mut fx),
            Msg::CloseTab(tab_id) => fx.push(request(
                Body::CloseTab(v1::CloseTabRequest { tab_id }),
                Followup::None,
            )),
            Msg::Split(side) => self.split(side, &mut fx),
            Msg::ClosePane => self.close_pane(&mut fx),
            Msg::AreaChanged(rect) => {
                if self.area != Some(rect) {
                    self.area = Some(rect);
                    self.sync_panes(&mut fx);
                }
            }
            Msg::AreaPress(p) => self.area_press(p, &mut fx),
            Msg::AreaDrag(p) => self.area_drag(p, &mut fx),
            Msg::AreaRelease(_) => self.area_release(),
            Msg::AreaWheel(p, lines) => self.area_wheel(p, lines, &mut fx),
            Msg::FocusField(field) => self.focus = Some(field),
            Msg::Submit => self.submit(&mut fx),
            Msg::CloseDialog => self.close_dialog(),
            Msg::ConfirmDelete => self.confirm_delete(&mut fx),
            Msg::Toggle(toggle) => self.toggle(toggle, &mut fx),
            Msg::BrowseFolder => fx.push(Effect::PickFolder),
            Msg::FolderPicked(path) => {
                if let (Some(path), Some(Dialog::AddProject { path: field })) =
                    (path, &mut self.dialog)
                {
                    field.set(path);
                }
            }
            Msg::DismissError => self.error = None,
            Msg::Key(key) => self.on_key(&key, &mut fx),
            Msg::Text(text) => self.insert_text(text, v1::input::Kind::Text, &mut fx),
            Msg::Paste(text) => self.insert_text(text, v1::input::Kind::Paste, &mut fx),
            Msg::Copy => self.copy(&mut fx),
        }
        fx
    }

    fn on_connected(
        &mut self,
        welcome: v1::Welcome,
        workspace: v1::Workspace,
        fx: &mut Vec<Effect>,
    ) {
        self.connection = Connection::Connected {
            host: welcome.host_name,
        };
        self.reset_panes();
        self.on_workspace(workspace, fx);
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

    fn new_tab(&self, fx: &mut Vec<Effect>) {
        if let Some(thread_id) = self.selected_thread.clone() {
            fx.push(request(
                Body::CreateTab(v1::CreateTabRequest {
                    thread_id,
                    title: String::new(),
                }),
                Followup::TabCreated,
            ));
        }
    }

    fn split(&self, side: v1::Direction, fx: &mut Vec<Effect>) {
        if let (Some(tab), Some(pane)) = (
            self.current_tab().map(|t| t.id.clone()),
            self.focused_pane(),
        ) {
            let body = Body::SplitPane(v1::SplitPaneRequest {
                tab_id: tab,
                pane_id: pane,
                side: side.into(),
            });
            fx.push(request(body, Followup::PaneCreated));
        }
    }

    fn close_pane(&self, fx: &mut Vec<Effect>) {
        if let (Some(tab), Some(pane)) = (
            self.current_tab().map(|t| t.id.clone()),
            self.focused_pane(),
        ) {
            fx.push(request(
                Body::ClosePane(v1::ClosePaneRequest {
                    tab_id: tab,
                    pane_id: pane,
                }),
                Followup::None,
            ));
        }
    }

    fn area_release(&mut self) {
        self.drag = None;
        if self
            .selection
            .as_ref()
            .is_some_and(|(_, s)| s.anchor == s.head)
        {
            self.selection = None;
        }
    }

    fn area_wheel(&self, p: Point, lines: i32, fx: &mut Vec<Effect>) {
        if let Some(pane) = self.pane_at(p) {
            fx.push(Effect::Notify(notification::Body::ScrollPane(
                v1::ScrollPane {
                    pane_id: pane,
                    lines,
                    to_bottom: false,
                },
            )));
        }
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
        } else if let Some(pane) = self.focused_pane() {
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

    fn open(&mut self, dialog: Dialog, focus: Option<Field>) {
        self.dialog = Some(dialog);
        self.focus = focus;
    }

    fn close_dialog(&mut self) {
        // The connect dialog is the only way forward when no daemon is chosen yet.
        if self.target == ConnectTarget::Ask && matches!(self.dialog, Some(Dialog::Connect { .. }))
        {
            return;
        }
        self.dialog = None;
        self.focus = None;
    }

    fn reset_panes(&mut self) {
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
        self.focused_override
            .retain(|tab, _| self.workspace.raw().tabs.iter().any(|t| &t.id == tab));
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
            (Followup::ThreadCreated, Outcome::Thread(thread)) => {
                self.selected_thread = Some(thread.id);
                self.close_dialog();
                self.sync_panes(fx);
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
            (Followup::TabCreated, Outcome::Tab(tab)) => {
                self.selected_tab.insert(tab.thread_id, tab.id);
                self.sync_panes(fx);
            }
            (Followup::PaneCreated, Outcome::PaneCreated(created)) => {
                self.focused_override
                    .insert(created.tab_id, created.pane_id);
            }
            (Followup::Settings, Outcome::Settings(settings)) => self.settings = Some(settings),
            (Followup::TailnetStatus, Outcome::TailnetStatus(status)) => {
                self.tailnet = Some(status);
            }
            (Followup::CloseDialog, _) => {
                self.dialog = None;
                self.focus = None;
            }
            _ => {}
        }
    }

    fn submit(&mut self, fx: &mut Vec<Effect>) {
        match &self.dialog {
            Some(Dialog::AddProject { path }) if !path.text().trim().is_empty() => {
                let body = Body::AddProject(v1::AddProjectRequest {
                    path: path.text().trim().to_owned(),
                    name: String::new(),
                });
                fx.push(request(body, Followup::CloseDialog));
            }
            Some(Dialog::NewThread {
                project,
                title,
                base,
            }) => {
                let body = Body::CreateThread(v1::CreateThreadRequest {
                    project_id: project.clone(),
                    title: title.text().trim().to_owned(),
                    base_ref: base.text().trim().to_owned(),
                });
                fx.push(request(body, Followup::ThreadCreated));
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
            SettingToggle::ServeTailnet => {
                let serving = self.tailnet.as_ref().is_some_and(|t| t.serving);
                fx.push(request(
                    Body::SetTailnetServe(v1::SetTailnetServeRequest { serve: !serving }),
                    Followup::TailnetStatus,
                ));
            }
        }
    }

    fn field_mut(&mut self) -> Option<&mut TextField> {
        let focus = self.focus?;
        match (&mut self.dialog, focus) {
            (Some(Dialog::AddProject { path }), Field::ProjectPath) => Some(path),
            (Some(Dialog::NewThread { title, .. }), Field::ThreadTitle) => Some(title),
            (Some(Dialog::NewThread { base, .. }), Field::ThreadBase) => Some(base),
            (Some(Dialog::Settings { prefix, .. }), Field::BranchPrefix) => Some(prefix),
            (Some(Dialog::Settings { shell, .. }), Field::DefaultShell) => Some(shell),
            (Some(Dialog::Connect { host }), Field::Host) => Some(host),
            _ => None,
        }
    }

    fn dialog_fields(&self) -> &'static [Field] {
        match self.dialog {
            Some(Dialog::AddProject { .. }) => &[Field::ProjectPath],
            Some(Dialog::NewThread { .. }) => &[Field::ThreadTitle, Field::ThreadBase],
            Some(Dialog::Settings { .. }) => &[Field::BranchPrefix, Field::DefaultShell],
            Some(Dialog::Connect { .. }) => &[Field::Host],
            _ => &[],
        }
    }

    fn on_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        if self.dialog.is_some() {
            self.dialog_key(key, fx);
        } else if key.mods.is_shortcut() || key.mods.logo {
            self.shortcut(key, fx);
        } else if let Some(pane) = self.focused_pane()
            && let Some(kind) = key.to_terminal()
        {
            self.selection = None;
            fx.push(input(pane, kind));
        }
    }

    fn dialog_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        if key.mods.is_shortcut() || key.mods.logo {
            match key.character.as_deref().map(str::to_lowercase).as_deref() {
                Some("v") => fx.push(Effect::RequestPaste),
                Some("w") => self.close_dialog(),
                _ => {}
            }
            return;
        }
        match key.named {
            Some(Named::Escape) => self.close_dialog(),
            Some(Named::Enter) => self.submit(fx),
            Some(Named::Tab) => {
                let fields = self.dialog_fields();
                if !fields.is_empty() {
                    let i = self
                        .focus
                        .and_then(|f| fields.iter().position(|x| *x == f))
                        .map_or(0, |i| (i + 1) % fields.len());
                    self.focus = Some(fields[i]);
                }
            }
            Some(named) => {
                if let Some(field) = self.field_mut() {
                    match named {
                        Named::Backspace => field.backspace(),
                        Named::Delete => field.delete(),
                        Named::Left => field.left(),
                        Named::Right => field.right(),
                        Named::Home | Named::Up => field.home(),
                        Named::End | Named::Down => field.end(),
                        Named::Space => field.insert(" "),
                        _ => {}
                    }
                }
            }
            None => {
                if let (Some(text), false) = (
                    key.text.clone().or_else(|| key.character.clone()),
                    key.mods.ctrl,
                ) && let Some(field) = self.field_mut()
                {
                    field.insert(&text);
                }
            }
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
            "n" => match self
                .selected_thread
                .as_ref()
                .and_then(|t| self.workspace.thread(t))
                .map(|t| t.project_id.clone())
            {
                Some(project) => self.update(Msg::OpenNewThread(project)),
                None => self.update(Msg::OpenAddProject),
            },
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

    fn pane_at(&self, p: Point) -> Option<String> {
        self.pane_rects()
            .into_iter()
            .find(|(_, r)| r.contains(p))
            .map(|(id, _)| id)
    }

    fn area_press(&mut self, p: Point, fx: &mut Vec<Effect>) {
        if self.dialog.is_some() {
            return;
        }
        let (Some(area), Some(tab_id)) = (self.area, self.current_tab().map(|t| t.id.clone()))
        else {
            return;
        };
        if let Some(tab) = self.workspace.tab(&tab_id)
            && let Some(hit) = terminal::divider_at(tab.layout(), area, p)
        {
            self.drag = Some(Drag::Divider {
                tab: tab_id,
                anchor: hit.anchor,
                axis: hit.axis,
                split: hit.split,
            });
            return;
        }
        let Some((pane, rect)) = self.pane_rects().into_iter().find(|(_, r)| r.contains(p)) else {
            return;
        };
        self.focus_pane(&tab_id, &pane, fx);
        if let Some(screen) = self.screens.get(&pane) {
            let cell = terminal::cell_at(rect, self.cell, screen, p);
            self.selection = Some((
                pane.clone(),
                Selection {
                    anchor: cell,
                    head: cell,
                },
            ));
            self.drag = Some(Drag::Select { pane });
        }
    }

    fn area_drag(&mut self, p: Point, fx: &mut Vec<Effect>) {
        match self.drag.clone() {
            Some(Drag::Divider {
                tab,
                anchor,
                axis,
                split,
            }) => {
                let position = terminal::divider_position(axis, split, p);
                let axis = urushi_proto::convert::axis_to_proto(axis);
                let body = Body::MoveDivider(v1::MoveDividerRequest {
                    tab_id: tab,
                    anchor_pane_id: anchor,
                    axis: axis.into(),
                    position,
                });
                fx.push(request(body, Followup::None));
            }
            Some(Drag::Select { pane }) => {
                let rect = self
                    .pane_rects()
                    .into_iter()
                    .find(|(id, _)| *id == pane)
                    .map(|(_, r)| r);
                if let (Some(rect), Some(screen)) = (rect, self.screens.get(&pane)) {
                    let cell = terminal::cell_at(rect, self.cell, screen, p);
                    if let Some((_, selection)) = &mut self.selection {
                        selection.head = cell;
                    }
                }
            }
            None => {}
        }
    }
}

fn input(pane_id: String, kind: v1::input::Kind) -> Effect {
    Effect::Notify(notification::Body::Input(v1::Input {
        pane_id,
        kind: Some(kind),
    }))
}

#[cfg(test)]
mod tests;
