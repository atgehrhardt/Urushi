//! Property tests: no sequence of layout operations may break the layout invariants.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use proptest::prelude::*;
use urushi_core::layout::{MIN_RATIO, Rect};
use urushi_core::{Axis, Direction, Layout, LayoutError, PaneId};

#[derive(Clone, Debug)]
enum Op {
    Split {
        target: usize,
        side: Direction,
    },
    Close {
        target: usize,
    },
    Resize {
        target: usize,
        side: Direction,
        amount: f64,
    },
    MoveDivider {
        divider: usize,
        position: f64,
    },
    Equalize,
}

fn direction() -> impl Strategy<Value = Direction> {
    prop_oneof![
        Just(Direction::Left),
        Just(Direction::Right),
        Just(Direction::Up),
        Just(Direction::Down)
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (any::<usize>(), direction()).prop_map(|(target, side)| Op::Split { target, side }),
        2 => any::<usize>().prop_map(|target| Op::Close { target }),
        2 => (any::<usize>(), direction(), -1.0..1.0f64)
            .prop_map(|(target, side, amount)| Op::Resize { target, side, amount }),
        2 => (any::<usize>(), -0.5..1.5f64).prop_map(|(divider, position)| Op::MoveDivider { divider, position }),
        1 => Just(Op::Equalize),
    ]
}

fn pick(layout: &Layout, index: usize) -> PaneId {
    let panes = layout.panes();
    panes[index % panes.len()]
}

fn assert_tiles_unit_square(layout: &Layout) {
    let geometry = layout.geometry(Rect::UNIT);
    assert_eq!(geometry.panes.len(), layout.len());
    let area: f64 = geometry
        .panes
        .iter()
        .map(|p| p.rect.width * p.rect.height)
        .sum();
    assert!((area - 1.0).abs() < 1e-6, "pane areas sum to {area}");
    for p in &geometry.panes {
        let r = p.rect;
        assert!(r.width > 0.0 && r.height > 0.0, "degenerate rect {r:?}");
        assert!(
            r.x >= -1e-9 && r.y >= -1e-9 && r.right() <= 1.0 + 1e-9 && r.bottom() <= 1.0 + 1e-9
        );
    }
    assert_eq!(geometry.dividers.len(), divider_count(layout));
}

/// A tree with n leaves has (number of children - 1) dividers per split.
fn divider_count(layout: &Layout) -> usize {
    fn walk(node: &urushi_core::layout::Node) -> usize {
        match node {
            urushi_core::layout::Node::Pane { .. } => 0,
            urushi_core::layout::Node::Split { children, .. } => {
                children.len() - 1 + children.iter().map(|c| walk(&c.node)).sum::<usize>()
            }
        }
    }
    walk(layout.root())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn operations_preserve_invariants(ops in prop::collection::vec(op(), 1..60)) {
        let mut layout = Layout::single(PaneId::from_u128(0));
        let mut next_id = 1u128;

        for op in ops {
            let before = layout.clone();
            let count = layout.len();
            match op {
                Op::Split { target, side } => {
                    let target = pick(&layout, target);
                    let new = PaneId::from_u128(next_id);
                    next_id += 1;
                    match layout.split(target, new, side) {
                        Ok(()) => {
                            prop_assert_eq!(layout.len(), count + 1);
                            prop_assert!(layout.contains(new));
                        }
                        Err(LayoutError::TooSmall) => prop_assert_eq!(&layout, &before),
                        Err(e) => prop_assert!(false, "unexpected split error {e}"),
                    }
                }
                Op::Close { target } => {
                    let target = pick(&layout, target);
                    match layout.close(target) {
                        Ok(successor) => {
                            prop_assert_eq!(layout.len(), count - 1);
                            prop_assert!(!layout.contains(target));
                            prop_assert!(layout.contains(successor));
                        }
                        Err(LayoutError::LastPane) => prop_assert_eq!(count, 1),
                        Err(e) => prop_assert!(false, "unexpected close error {e}"),
                    }
                }
                Op::Resize { target, side, amount } => {
                    let target = pick(&layout, target);
                    if layout.resize(target, side, amount).is_err() {
                        prop_assert_eq!(&layout, &before);
                    }
                    prop_assert_eq!(layout.panes(), before.panes());
                }
                Op::MoveDivider { divider, position } => {
                    let dividers = layout.geometry(Rect::UNIT).dividers;
                    if !dividers.is_empty() {
                        let d = dividers[divider % dividers.len()];
                        layout.move_divider(d.anchor, d.axis, position).unwrap();
                    }
                    prop_assert_eq!(layout.panes(), before.panes());
                }
                Op::Equalize => layout.equalize(),
            }
            layout.check_invariants().map_err(|e| TestCaseError::fail(e.to_string()))?;
            assert_tiles_unit_square(&layout);
        }

        let json = serde_json::to_string(&layout).unwrap();
        let back: Layout = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.panes(), layout.panes());
    }

    #[test]
    fn focus_neighbour_stays_in_layout(splits in prop::collection::vec((any::<usize>(), direction()), 0..20),
                                       from in any::<usize>(), side in direction()) {
        let mut layout = Layout::single(PaneId::from_u128(0));
        for (i, (target, dir)) in splits.into_iter().enumerate() {
            let target = pick(&layout, target);
            let _ = layout.split(target, PaneId::from_u128(i as u128 + 1), dir);
        }
        let source = pick(&layout, from);
        if let Some(n) = layout.focus_neighbour(source, side) {
            prop_assert!(layout.contains(n));
            prop_assert_ne!(n, source);
        }
    }

    #[test]
    fn user_resizes_respect_min_ratio(amount in -2.0..2.0f64) {
        let a = PaneId::from_u128(1);
        let b = PaneId::from_u128(2);
        let mut layout = Layout::single(a);
        layout.split(a, b, Direction::Right).unwrap();
        layout.resize(a, Direction::Right, amount).unwrap();
        let geometry = layout.geometry(Rect::UNIT);
        for p in geometry.panes {
            prop_assert!(p.rect.width >= MIN_RATIO - 1e-9);
        }
        prop_assert!(geometry.dividers.iter().all(|d| d.axis == Axis::Row));
    }
}
