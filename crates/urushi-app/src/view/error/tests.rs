//! The error shows, and can be dismissed, on whatever is on top of the window (UR-44).

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::{CellMetrics, Frame, Point, Rect, TextSystem, Theme, build};

use crate::config::ConnectTarget;
use crate::msg::Msg;
use crate::state::AppState;

/// Connected to a daemon with boards, showing a thread's terminal, with a board
/// holding task "a".
fn workspace_with_a_board() -> AppState {
    let mut state = AppState::new(
        ConnectTarget::Local(None),
        CellMetrics {
            width: 8.0,
            height: 16.0,
        },
    );
    let layout = urushi_proto::convert::layout_to_proto(&urushi_core::Layout::single(
        urushi_core::PaneId::from_u128(100),
    ));
    state.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: v1::Workspace {
            projects: vec![v1::Project {
                id: "r".into(),
                name: "repo".into(),
                ..Default::default()
            }],
            threads: vec![v1::Thread {
                id: "t".into(),
                project_id: "r".into(),
                title: "Fix bug".into(),
                tab_ids: vec!["tab".into()],
                state: v1::ThreadState::Ready.into(),
                ..Default::default()
            }],
            tabs: vec![v1::Tab {
                id: "tab".into(),
                thread_id: "t".into(),
                title: "Terminal".into(),
                layout: Some(layout),
                ..Default::default()
            }],
            boards: vec![v1::Board {
                id: "b".into(),
                name: "Sprint".into(),
                columns: vec![v1::BoardColumn {
                    id: "todo".into(),
                    name: "To do".into(),
                    tasks: vec![v1::Task {
                        id: "a".into(),
                        title: "A".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    });
    state.daemon_minor = crate::board::BOARDS_SINCE_MINOR;
    state.update(Msg::SelectThread("t".into()));
    state
}

fn render(state: &AppState, text: &mut TextSystem) -> Frame<Msg> {
    build(
        &super::super::view(state, &Theme::dark(), &HashMap::new()),
        Rect::new(0.0, 0.0, 1200.0, 800.0),
        text,
        None::<Point>,
    )
}

/// The banner's "Dismiss" buttons drawn, and how many of them a click reaches (not
/// covered by a dialog, the board or the quick terminal).
fn dismiss_buttons(frame: &Frame<Msg>) -> (usize, usize) {
    let drawn: Vec<_> = frame
        .hits
        .iter()
        .filter(|h| matches!(h.on_click, Some(Msg::DismissError)))
        .collect();
    let reachable = drawn
        .iter()
        .filter(|h| matches!(frame.click(h.rect.center()), Some(Msg::DismissError)))
        .count();
    (drawn.len(), reachable)
}

fn assert_shows_once(state: &AppState, text: &mut TextSystem, case: &str) {
    let frame = render(state, text);
    assert_eq!(
        dismiss_buttons(&frame),
        (1, 1),
        "{case}: one banner, and a click reaches it"
    );
    assert!(
        frame.scene.dump().contains("Jira refused"),
        "{case}: {}",
        frame.scene.dump()
    );
}

const ERROR: &str = "Jira refused the issue";

#[test]
fn the_error_shows_under_the_terminals_when_nothing_covers_them() {
    let mut text = TextSystem::bundled_only();
    let mut state = workspace_with_a_board();
    assert_eq!(dismiss_buttons(&render(&state, &mut text)), (0, 0));
    state.error = Some(ERROR.into());
    assert_shows_once(&state, &mut text, "main area");
    state.update(Msg::DismissError);
    assert_eq!(dismiss_buttons(&render(&state, &mut text)), (0, 0));
}

#[test]
fn the_error_shows_in_the_task_editor_beside_its_buttons() {
    let mut text = TextSystem::bundled_only();
    let mut state = workspace_with_a_board();
    state.update(Msg::ToggleBoard);
    state.update(Msg::BoardReveal(1.0));
    state.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    state.error = Some(ERROR.into());
    assert_shows_once(&state, &mut text, "task editor");
    // The button that failed stays on screen and clickable under it.
    let frame = render(&state, &mut text);
    let save = frame
        .access
        .iter()
        .find(|a| a.label == "Add task")
        .expect("an Add task button");
    assert!(save.rect.y + save.rect.height <= 800.0);
    assert!(matches!(frame.click(save.rect.center()), Some(Msg::Submit)));
}

#[test]
fn a_long_error_wraps_in_a_dialog_instead_of_being_cut_off() {
    let mut text = TextSystem::bundled_only();
    let mut state = workspace_with_a_board();
    state.update(Msg::OpenPrompt(crate::organize::Prompt::NewBoard));
    state.error = Some(
        "Jira refused the issue: Field 'customfield_10020' cannot be set. It is not on the \
         appropriate screen, or unknown. Ask your Jira admin to add it, then try again."
            .into(),
    );
    let dump = render(&state, &mut text).scene.dump();
    assert!(dump.contains("again.\""), "the last word shows: {dump}");
    assert!(!dump.contains('…'), "nothing is cut short: {dump}");
}

#[test]
fn the_error_shows_on_the_task_board_over_the_terminals() {
    let mut text = TextSystem::bundled_only();
    let mut state = workspace_with_a_board();
    state.update(Msg::ToggleBoard);
    state.update(Msg::BoardReveal(1.0));
    state.error = Some(ERROR.into());
    assert_shows_once(&state, &mut text, "task board");
}

#[test]
fn the_error_shows_in_the_quick_terminal_over_everything() {
    let mut text = TextSystem::bundled_only();
    let mut state = workspace_with_a_board();
    state.update(Msg::ToggleBoard);
    state.update(Msg::BoardReveal(1.0));
    state.quick.open = true;
    state.quick.reveal = 1.0;
    state.error = Some(ERROR.into());
    assert_shows_once(&state, &mut text, "quick terminal");
}

#[test]
fn on_phones_the_error_shows_on_the_thread_list() {
    let mut text = TextSystem::bundled_only();
    let mut state = workspace_with_a_board();
    state.compact = true;
    state.sidebar_open = true;
    state.error = Some(ERROR.into());
    assert_shows_once(&state, &mut text, "thread list");
    // Showing the terminals moves it under them.
    state.sidebar_open = false;
    assert_shows_once(&state, &mut text, "terminals");
}
