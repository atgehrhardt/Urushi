//! The memo answers as the filter itself would, and starts again when the board or the
//! filter changes.

use urushi_client::v1;

use super::FilterCache;
use crate::filter::{BoardFilter, Context, Dim, board_dims, board_values};

fn task(id: &str, title: &str, tags: &[&str]) -> v1::Task {
    v1::Task {
        id: id.into(),
        title: title.into(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        ..Default::default()
    }
}

fn board(tasks: Vec<v1::Task>) -> v1::Board {
    v1::Board {
        id: "b".into(),
        columns: vec![v1::BoardColumn {
            id: "c".into(),
            tasks,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn shown_ids(cache: &FilterCache, b: &v1::Board, f: &BoardFilter) -> Vec<String> {
    let shown = cache.shown(b, f, Context::default());
    let mut ids: Vec<String> = shown.iter().cloned().collect();
    ids.sort();
    ids
}

#[test]
fn it_answers_as_the_filter_does() {
    let b = board(vec![
        task("1", "Checkout", &["Payments"]),
        task("2", "Invoices", &["payments", "pdf"]),
        task("3", "Login", &[]),
    ]);
    let cache = FilterCache::default();
    let mut f = BoardFilter::default();
    assert_eq!(shown_ids(&cache, &b, &f), ["1", "2", "3"]);
    f.toggle(Dim::Label, "payments");
    assert_eq!(shown_ids(&cache, &b, &f), ["1", "2"]);
    f.search = urushi_ui::TextField::new("inv");
    assert_eq!(shown_ids(&cache, &b, &f), ["2"]);
    for dim in [Dim::Label, Dim::Type] {
        assert_eq!(&*cache.values(&b, dim), board_values(&b, dim).as_slice());
    }
    assert_eq!(cache.dims(&b), board_dims(&b));
    assert_eq!(cache.count(&b, &b.columns[0], &f, Context::default()), 1);
}

#[test]
fn a_changed_board_or_filter_is_worked_out_again() {
    let mut b = board(vec![task("1", "Checkout", &["Payments"])]);
    let cache = FilterCache::default();
    let mut f = BoardFilter::default();
    f.toggle(Dim::Label, "payments");
    assert_eq!(shown_ids(&cache, &b, &f), ["1"]);
    b.columns[0].tasks.push(task("2", "Refunds", &["payments"]));
    assert_eq!(shown_ids(&cache, &b, &f), ["1", "2"]);
    assert_eq!(
        cache.values(&b, Dim::Label).len(),
        1,
        "one label, spelled twice"
    );
    f.toggle(Dim::Label, "payments");
    f.toggle(Dim::Label, "nothing");
    assert!(shown_ids(&cache, &b, &f).is_empty());
    // A copy of the state doesn't carry the memo, nor change equality.
    assert_eq!(cache.clone(), cache);
}
