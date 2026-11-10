//! The quick terminal's drop-down panel (ADR 0031): the shortcut and the daemon request,
//! attaching while it's down and detaching once it's away, where typing, Escape, the
//! mouse and the pane shortcuts go, and attention.

use urushi_client::v1::{self, request::Body};
use urushi_core::{Direction, PaneId, QuickTerminal, TabId};
use urushi_proto::convert;
use urushi_ui::{Point, Rect};

use super::{attached, bodies, id, key, named, shortcut, workspace};
use crate::keys::{Mods, Named};
use crate::msg::{Effect, Followup, Msg, QuickMsg};
use crate::state::AppState;

/// The quick terminal's id.
fn quick_id() -> String {
    TabId::from_u128(30).to_string()
}

/// The usual workspace, plus a quick terminal of panes 300 (and 301, split down).
fn with_quick(split: bool, attention: Vec<v1::PaneAttention>) -> v1::Workspace {
    let mut quick = QuickTerminal::new(TabId::from_u128(30), PaneId::from_u128(300));
    if split {
        quick
            .group_mut()
            .split(
                PaneId::from_u128(300),
                PaneId::from_u128(301),
                Direction::Down,
            )
            .unwrap();
        quick.group_mut().focus(PaneId::from_u128(300)).unwrap();
    }
    let mut w = workspace();
    w.revision = 2;
    let mut wire = convert::quick_terminal_to_proto(&quick);
    wire.attention = attention;
    w.quick_terminal = Some(wire);
    w
}

/// Showing the first thread's terminal, connected to a daemon with quick terminals.
fn ready() -> AppState {
    let mut s = attached();
    s.daemon_minor = urushi_proto::PROTOCOL_MINOR;
    s
}

/// `ready`, with the daemon's quick terminal in the workspace.
fn ready_with_quick(split: bool) -> AppState {
    let mut s = ready();
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(with_quick(
        split,
        Vec::new(),
    ))));
    s
}

/// The panel's terminal area, the same size as the main area (100 × 24 cells).
fn quick_area() -> Rect {
    Rect::new(0.0, 0.0, 816.0, 400.0)
}

/// Drops the panel down and lets it settle: what the runner does.
fn drop_down(s: &mut AppState) -> Vec<Effect> {
    let mut fx = s.update(Msg::Key(key("`", shortcut())));
    fx.extend(s.update(Msg::Quick(QuickMsg::Reveal(1.0))));
    fx.extend(s.update(Msg::Quick(QuickMsg::Area(quick_area()))));
    fx
}

fn opens(fx: &[Effect]) -> usize {
    bodies(fx)
        .iter()
        .filter(|b| matches!(b, Body::OpenQuickTerminal(_)))
        .count()
}

fn attached_panes(fx: &[Effect]) -> Vec<String> {
    bodies(fx)
        .iter()
        .filter_map(|b| match b {
            Body::AttachPane(r) => Some(r.pane_id.clone()),
            _ => None,
        })
        .collect()
}

fn detached_panes(fx: &[Effect]) -> Vec<String> {
    bodies(fx)
        .iter()
        .filter_map(|b| match b {
            Body::DetachPane(r) => Some(r.pane_id.clone()),
            _ => None,
        })
        .collect()
}

fn typed_into(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Notify(v1::notification::Body::Input(i)) => Some(i.pane_id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_shortcut_drops_the_panel_and_opens_the_quick_terminal_once() {
    let mut s = ready();
    let fx = s.update(Msg::Key(key("`", shortcut())));
    assert!(s.quick.open);
    assert_eq!(opens(&fx), 1);
    // Shift turns ` into ~: the same shortcut. It puts the panel away.
    s.update(Msg::Key(key("~", shortcut())));
    assert!(!s.quick.open);
    // Down again before the daemon answered: no second request.
    assert_eq!(opens(&s.update(Msg::Quick(QuickMsg::Toggle))), 0);
    let fx = s.update(Msg::Response {
        followup: Followup::QuickTerminal,
        result: Ok(v1::response::Outcome::QuickTerminal(
            with_quick(false, Vec::new()).quick_terminal.unwrap(),
        )),
    });
    assert!(!s.quick.starting);
    assert!(
        fx.iter()
            .all(|e| !matches!(e, Effect::SystemNotification { .. }))
    );
}

#[test]
fn a_failed_open_puts_the_panel_away_and_says_why() {
    let mut s = ready();
    s.update(Msg::Quick(QuickMsg::Toggle));
    s.update(Msg::Response {
        followup: Followup::QuickTerminal,
        result: Err("no shell".into()),
    });
    assert!(!s.quick.open && !s.quick.starting);
    assert_eq!(s.error.as_deref(), Some("no shell"));
}

#[test]
fn older_daemons_have_no_quick_terminal() {
    let mut s = attached();
    s.daemon_minor = 19;
    let fx = s.update(Msg::Key(key("`", shortcut())));
    assert!(!s.quick.open);
    assert_eq!(opens(&fx), 0);
}

#[test]
fn its_panes_attach_while_down_and_detach_only_once_it_is_away() {
    let mut s = ready_with_quick(false);
    let fx = drop_down(&mut s);
    assert_eq!(opens(&fx), 0, "there is one already");
    assert_eq!(attached_panes(&fx), [id(300)]);
    assert!(
        detached_panes(&fx).is_empty(),
        "the tab under it stays attached"
    );

    // Putting it away keeps it attached while it slides up, then lets go of it only.
    let fx = s.update(Msg::Key(key("`", shortcut())));
    assert!(detached_panes(&fx).is_empty());
    let fx = s.update(Msg::Quick(QuickMsg::Reveal(0.0)));
    assert_eq!(detached_panes(&fx), [id(300)]);

    // Down again: attached at once, at the size it had.
    let fx = s.update(Msg::Quick(QuickMsg::Toggle));
    assert_eq!(attached_panes(&fx), [id(300)]);
}

#[test]
fn typing_and_escape_go_to_the_quick_terminal_even_over_the_board() {
    let mut s = ready_with_quick(false);
    s.update(Msg::ToggleBoard);
    drop_down(&mut s);
    assert_eq!(
        typed_into(&s.update(Msg::Key(key("a", Mods::default())))),
        [id(300)]
    );
    assert_eq!(
        typed_into(&s.update(Msg::Key(named(Named::Escape)))),
        [id(300)]
    );
    assert!(s.quick.open && s.board.open, "Escape closes neither");
    assert_eq!(typed_into(&s.update(Msg::Text("é".into()))), [id(300)]);
    // Put away, the board has the keyboard again.
    s.update(Msg::Quick(QuickMsg::Hide));
    assert!(typed_into(&s.update(Msg::Key(key("a", Mods::default())))).is_empty());
}

#[test]
fn a_dialog_takes_the_keys_over_the_panel() {
    let mut s = ready_with_quick(false);
    drop_down(&mut s);
    s.update(Msg::OpenSettings);
    assert!(typed_into(&s.update(Msg::Key(key("a", Mods::default())))).is_empty());
}

#[test]
fn pane_shortcuts_act_on_the_quick_terminal() {
    let mut s = ready_with_quick(true);
    drop_down(&mut s);
    assert_eq!(s.focused_pane(), Some(id(300)));
    let fx = s.update(Msg::Key(key("d", shortcut())));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::SplitPane(v1::SplitPaneRequest { tab_id, pane_id, .. })]
            if *tab_id == quick_id() && *pane_id == id(300)
    ));
    let fx = s.update(Msg::Key(alt_arrow(Named::Down)));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::FocusPane(v1::FocusPaneRequest { tab_id, pane_id })]
            if *tab_id == quick_id() && *pane_id == id(301)
    ));
    assert_eq!(
        s.focused_pane(),
        Some(id(301)),
        "at once, before the daemon"
    );
    let fx = s.update(Msg::Key(key("w", shortcut())));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::ClosePane(v1::ClosePaneRequest { tab_id, pane_id })]
            if *tab_id == quick_id() && *pane_id == id(301)
    ));
}

/// The shortcut with Alt and an arrow, which moves focus that way.
fn alt_arrow(arrow: Named) -> crate::keys::KeyPress {
    let mut k = named(arrow);
    k.mods = Mods {
        alt: true,
        ..shortcut()
    };
    k
}

#[test]
fn the_mouse_reaches_the_panel_and_never_the_tab_under_it() {
    let mut s = ready_with_quick(false);
    drop_down(&mut s);
    s.update(Msg::Response {
        followup: Followup::Attached(id(300)),
        result: Ok(v1::response::Outcome::Screen(v1::ScreenUpdate {
            pane_id: id(300),
            seq: 1,
            full: true,
            cols: 100,
            rows: 24,
            ..Default::default()
        })),
    });
    // The quick terminal lies over the main area here, so a press goes to its pane.
    s.update(Msg::AreaPress(Point::new(20.0, 20.0)));
    s.update(Msg::AreaDrag(Point::new(60.0, 20.0)));
    assert_eq!(s.selection.as_ref().map(|(p, _)| p.clone()), Some(id(300)));
    // Outside the panel's terminals, nothing in the tab under it is touched.
    s.selection = None;
    s.update(Msg::AreaPress(Point::new(20.0, 600.0)));
    assert!(s.selection.is_none());
    assert!(!s.terminal_at(Point::new(20.0, 600.0)));
}

#[test]
fn the_panel_goes_away_with_the_quick_terminal() {
    let mut s = ready_with_quick(false);
    drop_down(&mut s);
    // Its last shell exited: the daemon reports no quick terminal.
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace())));
    assert!(!s.quick.open);
}

#[test]
fn attention_is_cleared_while_down_and_announced_while_away() {
    let raised = |since| {
        vec![v1::PaneAttention {
            pane_id: id(300),
            kind: v1::AttentionKind::Waiting.into(),
            since_ms: since,
        }]
    };
    let mut s = ready_with_quick(false);
    s.update(Msg::WindowFocused(false));
    let fx = s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(with_quick(
        false,
        raised(1),
    ))));
    let notes: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::SystemNotification { title, body } => Some((title.as_str(), body.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(notes, [("Quick terminal", "The terminal needs your input")]);
    assert_eq!(s.quick_attention(), Some(v1::AttentionKind::Waiting));

    s.update(Msg::WindowFocused(true));
    let fx = drop_down(&mut s);
    let cleared: Vec<_> = bodies(&fx)
        .iter()
        .flat_map(|b| match b {
            Body::ClearAttention(r) => r.pane_ids.clone(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(cleared, [id(300)]);
}
