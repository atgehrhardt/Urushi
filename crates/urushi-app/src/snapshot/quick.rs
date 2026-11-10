//! The quick terminal's snapshot scenario: its panel dropped down over a thread's
//! terminal, split in two.

use urushi_client::v1;
use urushi_core::{Direction, PaneId, QuickTerminal, TabId};

use crate::msg::{Msg, QuickMsg};
use crate::state::AppState;

/// The panel dropped down over the `status` scenario's terminal.
pub(super) fn show(state: &mut AppState) {
    super::show_board_or_usage(state, "status");
    let (first, second) = (PaneId::from_u128(0xc1), PaneId::from_u128(0xc2));
    let mut quick = QuickTerminal::new(TabId::from_u128(0xc0), first);
    if quick
        .group_mut()
        .split(first, second, Direction::Right)
        .is_err()
    {
        return;
    }
    let mut workspace = state.workspace.raw().clone();
    workspace.quick_terminal = Some(urushi_proto::convert::quick_terminal_to_proto(&quick));
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    state.update(Msg::Quick(QuickMsg::Toggle));
    state.update(Msg::Quick(QuickMsg::Reveal(1.0)));
    for pane in [first, second] {
        state
            .screens
            .insert(pane.to_string(), std::sync::Arc::default());
    }
}
