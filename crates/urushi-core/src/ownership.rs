//! Which branches a thread *owns*, and so may delete when the thread is deleted.
//!
//! Deleting a thread removes branches locally and, by default, on remotes. Getting this
//! wrong destroys someone's work, so the rules are pure, kept in one place, and tested
//! exhaustively. The daemon gathers the git facts and this module makes the decision.
//!
//! # Rules
//! A thread owns its primary branch, plus every branch that was checked out in its
//! worktree. It never owns a branch that:
//! - existed before the thread was created,
//! - is protected (the thread's base branch, or a remote's default branch such as `main`),
//! - is currently checked out in another worktree, or
//! - no longer exists.

use std::collections::BTreeSet;

use crate::BranchName;

/// Git facts the daemon collects before deciding which branches a thread owns.
#[derive(Clone, Debug, Default)]
pub struct OwnershipFacts {
    /// The branch created for the thread.
    pub primary: Option<BranchName>,
    /// Branches checked out in the thread's worktree, from the worktree's own HEAD reflog.
    pub checked_out_in_worktree: Vec<BranchName>,
    /// Branches that existed when the thread was created (saved at creation time).
    pub preexisting: BTreeSet<BranchName>,
    /// The base branch, remote default branches, and any other branch never to delete.
    pub protected: BTreeSet<BranchName>,
    /// Branches currently checked out in any *other* worktree, including the main one.
    pub checked_out_elsewhere: BTreeSet<BranchName>,
    /// Local branches that exist right now.
    pub existing: BTreeSet<BranchName>,
}

/// The local branches the thread owns, primary first, then sorted by name.
pub fn owned_branches(facts: &OwnershipFacts) -> Vec<BranchName> {
    let allowed = |b: &BranchName| {
        facts.existing.contains(b)
            && !facts.preexisting.contains(b)
            && !facts.protected.contains(b)
            && !facts.checked_out_elsewhere.contains(b)
    };

    let mut out: Vec<BranchName> = facts
        .primary
        .iter()
        .filter(|b| allowed(b))
        .cloned()
        .collect();
    let others: BTreeSet<&BranchName> = facts
        .checked_out_in_worktree
        .iter()
        .filter(|b| Some(*b) != facts.primary.as_ref() && allowed(b))
        .collect();
    out.extend(others.into_iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str) -> BranchName {
        BranchName::new(name).unwrap()
    }

    fn set(names: &[&str]) -> BTreeSet<BranchName> {
        names.iter().map(|n| b(n)).collect()
    }

    fn base_facts() -> OwnershipFacts {
        OwnershipFacts {
            primary: Some(b("urushi/t-1")),
            checked_out_in_worktree: vec![b("urushi/t-1"), b("feature/x"), b("main"), b("old")],
            preexisting: set(&["main", "old", "shared"]),
            protected: set(&["main"]),
            checked_out_elsewhere: set(&["shared"]),
            existing: set(&["urushi/t-1", "feature/x", "main", "old", "shared"]),
        }
    }

    #[test]
    fn owns_primary_and_branches_created_in_worktree() {
        assert_eq!(
            owned_branches(&base_facts()),
            vec![b("urushi/t-1"), b("feature/x")]
        );
    }

    #[test]
    fn never_owns_preexisting_protected_or_elsewhere() {
        let mut f = base_facts();
        f.checked_out_in_worktree.push(b("shared"));
        let owned = owned_branches(&f);
        for forbidden in ["main", "old", "shared"] {
            assert!(
                !owned.contains(&b(forbidden)),
                "{forbidden} must not be owned"
            );
        }
    }

    #[test]
    fn primary_is_dropped_if_protected_elsewhere_or_gone() {
        let mut f = base_facts();
        f.checked_out_elsewhere.insert(b("urushi/t-1"));
        assert_eq!(owned_branches(&f), vec![b("feature/x")]);

        let mut f = base_facts();
        f.existing.remove(&b("urushi/t-1"));
        assert_eq!(owned_branches(&f), vec![b("feature/x")]);

        let mut f = base_facts();
        f.protected.insert(b("urushi/t-1"));
        assert_eq!(owned_branches(&f), vec![b("feature/x")]);
    }

    #[test]
    fn duplicates_in_reflog_are_collapsed() {
        let mut f = base_facts();
        f.checked_out_in_worktree
            .extend([b("feature/x"), b("urushi/t-1")]);
        assert_eq!(owned_branches(&f), vec![b("urushi/t-1"), b("feature/x")]);
    }

    #[test]
    fn empty_facts_own_nothing() {
        assert!(owned_branches(&OwnershipFacts::default()).is_empty());
    }
}
