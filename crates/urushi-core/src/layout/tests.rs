use super::*;

fn p(n: u128) -> PaneId {
    PaneId::from_u128(n)
}

fn ratios(layout: &Layout) -> Vec<f64> {
    match layout.root() {
        Node::Split { children, .. } => children.iter().map(|c| c.ratio).collect(),
        Node::Pane { .. } => vec![1.0],
    }
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn split_root_creates_pair() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    assert_eq!(l.panes(), vec![p(1), p(2)]);
    l.check_invariants().unwrap();

    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Up).unwrap();
    assert_eq!(l.panes(), vec![p(2), p(1)], "Up places the new pane before");
}

#[test]
fn repeated_same_axis_split_stays_flat() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Right).unwrap();
    let Node::Split { axis, children } = l.root() else {
        panic!("expected split")
    };
    assert_eq!(*axis, Axis::Row);
    assert_eq!(children.len(), 3);
    assert!(approx(ratios(&l).iter().sum(), 1.0));
    assert_eq!(ratios(&l), vec![0.5, 0.25, 0.25]);
}

#[test]
fn cross_axis_split_nests() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Down).unwrap();
    assert_eq!(l.panes(), vec![p(1), p(2), p(3)]);
    l.check_invariants().unwrap();
    let Node::Split { children, .. } = l.root() else {
        panic!()
    };
    assert!(matches!(
        children[1].node,
        Node::Split {
            axis: Axis::Column,
            ..
        }
    ));
}

#[test]
fn split_errors() {
    let mut l = Layout::single(p(1));
    assert_eq!(
        l.split(p(9), p(2), Direction::Right),
        Err(LayoutError::PaneNotFound(p(9)))
    );
    assert_eq!(
        l.split(p(1), p(1), Direction::Right),
        Err(LayoutError::DuplicatePane(p(1)))
    );
}

#[test]
fn split_refuses_tiny_panes() {
    let mut l = Layout::single(p(1));
    let mut next = 2;
    let mut target = p(1);
    let result = loop {
        let r = l.split(target, p(next), Direction::Right);
        if r.is_err() {
            break r;
        }
        target = p(next);
        next += 1;
    };
    assert_eq!(result, Err(LayoutError::TooSmall));
    l.check_invariants().unwrap();
}

#[test]
fn close_gives_space_to_previous_sibling_and_collapses() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Down).unwrap();
    // Closing 3 leaves a single-child column, which collapses back into the row.
    assert_eq!(l.close(p(3)).unwrap(), p(2));
    assert_eq!(l.panes(), vec![p(1), p(2)]);
    l.check_invariants().unwrap();
    assert_eq!(l.close(p(1)).unwrap(), p(2));
    assert_eq!(l, Layout::single(p(2)));
    assert_eq!(l.close(p(2)), Err(LayoutError::LastPane));
}

#[test]
fn close_flattens_into_grandparent() {
    // row[1, column[2, row[3, 4]]] → close 2 → row[1, 3, 4]
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Down).unwrap();
    l.split(p(3), p(4), Direction::Right).unwrap();
    l.close(p(2)).unwrap();
    let Node::Split { axis, children } = l.root() else {
        panic!()
    };
    assert_eq!(*axis, Axis::Row);
    assert_eq!(children.len(), 3);
    l.check_invariants().unwrap();
}

#[test]
fn resize_moves_edges_and_clamps() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.resize(p(1), Direction::Right, 0.1).unwrap();
    assert!(approx(ratios(&l)[0], 0.6));
    l.resize(p(2), Direction::Left, 0.1).unwrap(); // grow 2 leftwards
    assert!(approx(ratios(&l)[0], 0.5));
    l.resize(p(1), Direction::Right, 5.0).unwrap();
    assert!(approx(ratios(&l)[1], MIN_RATIO));
    assert_eq!(
        l.resize(p(1), Direction::Left, 0.1),
        Err(LayoutError::NoAdjacentEdge)
    );
    assert_eq!(
        l.resize(p(1), Direction::Down, 0.1),
        Err(LayoutError::NoAdjacentEdge)
    );
    assert!(l.resize(p(1), Direction::Right, f64::NAN).is_err());
}

#[test]
fn resize_uses_outer_split_when_inner_has_no_neighbour() {
    // row[1, column[2, 3]]: moving pane 3's left edge should move the outer divider.
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Down).unwrap();
    l.resize(p(3), Direction::Left, 0.2).unwrap();
    assert!(approx(ratios(&l)[0], 0.3));
}

#[test]
fn move_divider_sets_absolute_position() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Right).unwrap(); // [0.5, 0.25, 0.25]
    let g = l.geometry(Rect::UNIT);
    let second = g.dividers.iter().find(|d| d.anchor == p(2)).unwrap();
    assert!(approx(second.position, 0.75));
    l.move_divider(second.anchor, second.axis, 0.6).unwrap();
    let r = ratios(&l);
    assert!(approx(r[0], 0.5) && approx(r[1], 0.1) && approx(r[2], 0.4));
}

#[test]
fn equalize_resets_ratios() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Right).unwrap();
    l.equalize();
    assert!(ratios(&l).iter().all(|r| approx(*r, 1.0 / 3.0)));
}

#[test]
fn geometry_tiles_bounds() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Down).unwrap();
    let g = l.geometry(Rect {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
    });
    let rect = |id| g.panes.iter().find(|r| r.pane == id).unwrap().rect;
    assert_eq!(
        rect(p(1)),
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0
        }
    );
    assert_eq!(
        rect(p(2)),
        Rect {
            x: 100.0,
            y: 0.0,
            width: 100.0,
            height: 50.0
        }
    );
    assert_eq!(
        rect(p(3)),
        Rect {
            x: 100.0,
            y: 50.0,
            width: 100.0,
            height: 50.0
        }
    );
    assert_eq!(g.dividers.len(), 2);
}

#[test]
fn focus_neighbour_is_spatial() {
    // row[1, column[2, 3]]
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Down).unwrap();
    assert_eq!(l.focus_neighbour(p(3), Direction::Left), Some(p(1)));
    assert_eq!(l.focus_neighbour(p(2), Direction::Down), Some(p(3)));
    assert_eq!(l.focus_neighbour(p(3), Direction::Up), Some(p(2)));
    assert_eq!(l.focus_neighbour(p(1), Direction::Left), None);
    // Pane 1 touches both 2 and 3 equally; the tie goes to the first found (2).
    assert_eq!(l.focus_neighbour(p(1), Direction::Right), Some(p(2)));
}

#[test]
fn from_root_validates_and_flattens() {
    let leaf = |n| Child {
        ratio: 0.5,
        node: Node::Pane { id: p(n) },
    };
    let dup = Node::Split {
        axis: Axis::Row,
        children: vec![leaf(1), leaf(1)],
    };
    assert_eq!(
        Layout::from_root(dup),
        Err(LayoutError::DuplicatePane(p(1)))
    );

    let single = Node::Split {
        axis: Axis::Row,
        children: vec![Child {
            ratio: 1.0,
            node: Node::Pane { id: p(1) },
        }],
    };
    assert_eq!(
        Layout::from_root(single),
        Err(LayoutError::UnderfilledSplit)
    );

    let bad_sum = Node::Split {
        axis: Axis::Row,
        children: vec![
            leaf(1),
            Child {
                ratio: 0.9,
                node: Node::Pane { id: p(2) },
            },
        ],
    };
    assert!(matches!(
        Layout::from_root(bad_sum),
        Err(LayoutError::InvalidRatio(_))
    ));

    let nested = Node::Split {
        axis: Axis::Row,
        children: vec![
            leaf(1),
            Child {
                ratio: 0.5,
                node: Node::Split {
                    axis: Axis::Row,
                    children: vec![leaf(2), leaf(3)],
                },
            },
        ],
    };
    let l = Layout::from_root(nested).unwrap();
    l.check_invariants().unwrap();
    assert_eq!(ratios(&l), vec![0.5, 0.25, 0.25]);
}

#[test]
fn serde_round_trip_and_validation() {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    let json = serde_json::to_string(&l).unwrap();
    let back: Layout = serde_json::from_str(&json).unwrap();
    assert_eq!(back, l);

    let invalid = r#"{"kind":"split","axis":"row","children":[{"ratio":1.0,"node":{"kind":"pane","id":"00000000-0000-0000-0000-000000000001"}}]}"#;
    assert!(serde_json::from_str::<Layout>(invalid).is_err());
}

#[test]
fn resize_between_two_tiny_panes_does_not_panic() {
    // Regression (found by proptest): both neighbours below MIN_RATIO with float rounding
    // made the clamp bounds cross.
    let mut l = Layout::single(p(1));
    let mut target = p(1);
    for n in 2..=7 {
        l.split(target, p(n), Direction::Right).unwrap_or(());
        target = p(n);
    }
    for _ in 0..50 {
        for id in l.panes() {
            let _ = l.resize(id, Direction::Right, 0.37);
            let _ = l.resize(id, Direction::Left, -0.91);
        }
        l.check_invariants().unwrap();
    }
}
