//! Reordering user-ordered lists (projects, boards, threads). Each list's stored order is
//! the order it is shown in, so moving an item is moving it in the list.

/// Moves the first item matching `is` to `index` (counted without it; clamped to the
/// end). Returns `false`, changing nothing, if no item matches.
pub fn move_where<T>(list: &mut Vec<T>, is: impl Fn(&T) -> bool, index: usize) -> bool {
    let Some(at) = list.iter().position(is) else {
        return false;
    };
    let item = list.remove(at);
    list.insert(index.min(list.len()), item);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_move_up_and_down_counted_without_themselves() {
        let mut list = vec!['a', 'b', 'c', 'd'];
        assert!(move_where(&mut list, |c| *c == 'd', 1));
        assert_eq!(list, ['a', 'd', 'b', 'c']);
        assert!(move_where(&mut list, |c| *c == 'a', 2));
        assert_eq!(list, ['d', 'b', 'a', 'c']);
    }

    #[test]
    fn past_the_end_is_the_end_and_unknown_items_change_nothing() {
        let mut list = vec!['a', 'b', 'c'];
        assert!(move_where(&mut list, |c| *c == 'a', 99));
        assert_eq!(list, ['b', 'c', 'a']);
        assert!(!move_where(&mut list, |c| *c == 'z', 0));
        assert_eq!(list, ['b', 'c', 'a']);
    }
}
