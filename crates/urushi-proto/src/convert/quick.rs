//! The quick terminal: core ⇄ wire (since 1.20, ADR 0031).

use urushi_core::{PaneId, QuickTerminal, TabId};

use super::{ConvertError, layout_from_proto, layout_to_proto, parse_id};
use crate::v1;

/// Core quick terminal → wire. Attention is left empty; the daemon adds it from its own
/// state, as for tabs.
pub fn quick_terminal_to_proto(q: &QuickTerminal) -> v1::QuickTerminal {
    v1::QuickTerminal {
        id: q.id().to_string(),
        layout: Some(layout_to_proto(q.layout())),
        focused_pane_id: q.focused().to_string(),
        attention: Vec::new(),
    }
}

/// Wire quick terminal → core, checked like a tab.
pub fn quick_terminal_from_proto(q: &v1::QuickTerminal) -> Result<QuickTerminal, ConvertError> {
    let layout = q
        .layout
        .as_ref()
        .ok_or_else(|| ConvertError::new("quick_terminal.layout", "missing"))?;
    QuickTerminal::from_parts(
        parse_id::<TabId>(&q.id, "quick_terminal.id")?,
        layout_from_proto(layout)?,
        parse_id::<PaneId>(&q.focused_pane_id, "quick_terminal.focused_pane_id")?,
    )
    .map_err(|e| ConvertError::new("quick_terminal.focused_pane_id", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use urushi_core::Direction;

    #[test]
    fn a_quick_terminal_survives_the_wire() {
        let mut q = QuickTerminal::new(TabId::from_u128(1), PaneId::from_u128(2));
        q.group_mut()
            .split(PaneId::from_u128(2), PaneId::from_u128(3), Direction::Right)
            .unwrap();
        let wire = quick_terminal_to_proto(&q);
        assert_eq!(quick_terminal_from_proto(&wire).unwrap(), q);
    }

    #[test]
    fn a_broken_quick_terminal_is_refused() {
        let q = QuickTerminal::new(TabId::from_u128(1), PaneId::from_u128(2));
        let mut wire = quick_terminal_to_proto(&q);
        wire.focused_pane_id = PaneId::from_u128(9).to_string();
        assert!(quick_terminal_from_proto(&wire).is_err());
        wire.layout = None;
        assert!(quick_terminal_from_proto(&wire).is_err());
    }
}
