//! Threads: naming, lifecycle (with shelving) and the stored form.

use super::*;

fn thread_spec() -> NewThread {
    NewThread {
        id: ThreadId::from_u128(0xabcd_1234),
        repo: RepoId::from_u128(1),
        project: None,
        title: "Refactor auth".into(),
        worktree_path: PathBuf::from("/data/worktrees/p/t"),
        base_ref: "main".into(),
        base_commit: "0123abcd".into(),
        created_at: Timestamp::from_unix_millis(0),
        branch: None,
    }
}

fn thread() -> Thread {
    Thread::new(thread_spec(), &BranchPrefix::default()).unwrap()
}

#[test]
fn a_named_branch_is_used_as_is() {
    let spec = NewThread {
        branch: Some(BranchName::new("Primary").unwrap()),
        ..thread_spec()
    };
    let t = Thread::new(spec, &BranchPrefix::default()).unwrap();
    assert_eq!(t.primary_branch.as_str(), "Primary");
    assert_eq!(t.slug.as_str(), "refactor-auth");
}

#[test]
fn threads_saved_before_the_repo_rename_still_load() {
    let mut json = serde_json::to_value(thread()).unwrap();
    let map = json.as_object_mut().unwrap();
    let repo = map.remove("repo").unwrap();
    map.insert("project".into(), repo);
    assert!(!map.contains_key("folder") && !map.contains_key("status"));
    let loaded: Thread = serde_json::from_value(json).unwrap();
    assert_eq!(loaded.repo, RepoId::from_u128(1));
    assert_eq!(loaded.project, None);
    assert!(!loaded.pinned && !loaded.unread && loaded.notes.is_empty());

    let mut filed = thread();
    filed.project = Some(ProjectId::from_u128(7));
    filed.status = Some(Status::new("Blocked").unwrap());
    let json = serde_json::to_string(&filed).unwrap();
    assert!(json.contains("\"folder\""), "{json}");
    assert_eq!(serde_json::from_str::<Thread>(&json).unwrap(), filed);
}

#[test]
fn unread_and_notes_are_stored_only_when_set() {
    let plain = serde_json::to_string(&thread()).unwrap();
    assert!(
        !plain.contains("unread") && !plain.contains("notes"),
        "{plain}"
    );

    let mut marked = thread();
    marked.unread = true;
    marked.notes = "Ask about the retry policy".into();
    marked.notes_color = Some(LabelColor::Teal);
    let json = serde_json::to_string(&marked).unwrap();
    assert!(json.contains("\"unread\":true"), "{json}");
    assert!(json.contains("\"notes_color\":\"teal\""), "{json}");
    assert_eq!(serde_json::from_str::<Thread>(&json).unwrap(), marked);
}

#[test]
fn a_project_base_thread_works_on_the_given_branch_and_round_trips() {
    let main = BranchName::new("main").unwrap();
    let t = Thread::project_base(thread_spec(), main.clone(), true);
    assert_eq!(t.primary_branch, main);
    assert!(t.is_project_base() && t.is_attached());
    let json = serde_json::to_string(&t).unwrap();
    assert_eq!(serde_json::from_str::<Thread>(&json).unwrap(), t);

    let plain = serde_json::to_string(&thread()).unwrap();
    assert!(
        !plain.contains("\"kind\""),
        "ordinary threads store no kind: {plain}"
    );
    let loaded: Thread = serde_json::from_str(&plain).unwrap();
    assert_eq!(loaded.kind, ThreadKind::Branch);
    assert!(!loaded.is_project_base() && !loaded.is_attached());
}

#[test]
fn new_thread_names_its_branch() {
    let t = thread();
    assert_eq!(t.primary_branch.as_str(), "urushi/refactor-auth-abcd1234");
    assert_eq!(t.state, ThreadState::Creating);
}

#[test]
fn lifecycle_happy_path_and_retry() {
    let mut t = thread();
    assert!(t.begin_deletion().is_err(), "cannot delete while creating");
    t.mark_ready().unwrap();
    assert!(t.is_ready());
    t.begin_deletion().unwrap();
    t.deletion_failed("offline").unwrap();
    assert!(matches!(t.state, ThreadState::DeletionFailed { .. }));
    t.begin_deletion().unwrap();
    assert_eq!(t.state, ThreadState::Deleting);
}

#[test]
fn invalid_transitions_are_rejected() {
    let mut t = thread();
    assert!(t.deletion_failed("x").is_err());
    t.mark_ready().unwrap();
    let err = t.mark_ready().unwrap_err();
    assert_eq!(err.from, ThreadState::Ready);
    assert!(t.unshelve().is_err(), "only a shelved thread unshelves");
}

#[test]
fn shelving_and_unshelving() {
    let mut t = thread();
    assert!(t.shelve().is_err(), "cannot shelve while creating");
    t.mark_ready().unwrap();
    t.shelve().unwrap();
    assert!(t.is_shelved());
    assert!(!t.is_ready(), "a shelved thread takes no terminals");
    assert_eq!(t.shelve().unwrap_err().from, ThreadState::Shelved);
    t.unshelve().unwrap();
    assert!(t.is_ready());
    t.shelve().unwrap();
    t.begin_deletion().unwrap();
    assert!(t.unshelve().is_err());
    assert!(t.shelve().is_err());
}

#[test]
fn a_shelved_thread_round_trips() {
    let mut t = thread();
    t.mark_ready().unwrap();
    t.shelve().unwrap();
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json["state"]["state"], "shelved");
    let back: Thread = serde_json::from_value(json).unwrap();
    assert_eq!(back.state, ThreadState::Shelved);
}
