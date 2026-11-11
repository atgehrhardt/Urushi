//! The keyboard: keys for the open dialog, the app's shortcuts (Cmd on Apple platforms,
//! Ctrl+Shift elsewhere), and everything else for the terminal that takes typing (the
//! quick terminal while its panel is down, else the current tab).

use urushi_client::v1;

use super::input;
use crate::keys::{KeyPress, Named};
use crate::msg::{Effect, Msg, QuickMsg};
use crate::state::{AppState, Dialog};
use crate::surface::Surface;

impl AppState {
    /// A key press: to the open dialog, the app's shortcuts, or the terminal that takes
    /// typing.
    pub(super) fn on_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        // Escape closes a menu, then the board, but never the quick terminal: programs
        // in it need Escape (vim, agents).
        if key.named == Some(Named::Escape)
            && (self.dismiss_overlay() || (!self.quick.open && self.board_escape()))
        {
            return;
        }
        if matches!(self.dialog, Some(Dialog::NewTab { .. }))
            && !(key.mods.is_shortcut() || key.mods.logo)
        {
            self.launcher_dialog_key(key, fx);
        } else if self.dialog.is_some() || self.searching() {
            self.dialog_key(key, fx);
        } else if key.mods.is_shortcut() || key.mods.logo {
            self.shortcut(key, fx);
        } else if self.keyboard_surface() != Some(Surface::Quick)
            && (self.board.visible() || self.empty_thread_key(key, fx))
        {
            // The board covers the terminals, or the launcher took the key.
        } else if let Some(pane) = self.focused_pane()
            && let Some(kind) = key.to_terminal()
        {
            self.selection = None;
            self.claim_sizes(fx);
            fx.push(input(pane, kind));
        }
    }

    pub(super) fn shortcut(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
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
            "\\" | "|" => self.update(Msg::Sidebar(crate::msg::SidebarMsg::Toggle)),
            // Shift turns ` into ~ on many layouts (and the shortcut is Ctrl+Shift off
            // Apple platforms).
            "`" | "~" => self.update(Msg::Quick(QuickMsg::Toggle)),
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
}
