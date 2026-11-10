//! Checks on the snapshot scenarios: what each screen shows, at desktop and phone sizes.

use super::*;

fn dump(name: &str, width: f32) -> String {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    scenario(name, &theme, &mut text, width, 820.0)
        .unwrap()
        .dump()
}

#[test]
fn every_scenario_renders_at_desktop_and_phone_sizes() {
    for name in SCENARIOS {
        for width in [1280.0, 402.0] {
            assert!(!dump(name, width).is_empty(), "{name} at {width}");
        }
    }
}

#[test]
fn settings_say_how_tasks_get_their_message() {
    let settings = dump("settings", 1280.0);
    for text in [
        "\"Send the task's message when the agent starts\"",
        "\"Use my own task message\"",
    ] {
        assert!(settings.contains(text), "{text} missing: {settings}");
    }
    assert!(
        !settings.contains("Filled in per task"),
        "no message box while it is off: {settings}"
    );
    let custom = dump("settings-task-message", 1280.0);
    for text in [
        "Off: the message is typed into Claude for you to edit and send.",
        "Pick up {key}",
        "Filled in per task",
    ] {
        assert!(custom.contains(text), "{text} missing: {custom}");
    }
    let start = dump("task-in-thread", 1280.0);
    assert!(
        start.contains("with this task as its prompt"),
        "sent by default: {start}"
    );
}

#[test]
fn the_setup_wizard_offers_installed_agents_then_their_modes() {
    let agents = dump("setup", 1280.0);
    for text in [
        "\"Set up Urushi\"",
        "\"Claude\"",
        "\"Codex\"",
        "\"Later\"",
        "\"Next\"",
    ] {
        assert!(agents.contains(text), "{text} missing: {agents}");
    }
    assert!(
        !agents.contains("\"Cursor\""),
        "only installed agents: {agents}"
    );
    let modes = dump("setup-mode", 1280.0);
    for text in [
        "\"How should Claude start?\"",
        "\"Normal\"",
        "\"Plan\"",
        "\"Save\"",
    ] {
        assert!(modes.contains(text), "{text} missing: {modes}");
    }
}

#[test]
fn task_cards_are_all_the_same_size() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let frame = scenario_frame("board", &theme, &mut text, 1280.0, 820.0).unwrap();
    // The sample has cards with no notes, one line, two lines, and subtasks.
    let cards: Vec<_> = frame
        .access
        .iter()
        .filter(|a| a.label.starts_with("Task "))
        .map(|a| a.rect)
        .collect();
    assert_eq!(cards.len(), 4, "{cards:?}");
    for card in &cards {
        // Columns split the width in fractions of a pixel; allow float rounding.
        assert!(
            (card.width - cards[0].width).abs() < 0.01
                && (card.height - cards[0].height).abs() < 0.01,
            "{cards:?}"
        );
    }
    let board = frame.scene.dump();
    assert!(
        board.contains("\"Waiting on legal\""),
        "notes on the card: {board}"
    );
    assert!(
        board.contains("\"then ship\""),
        "a second line of notes: {board}"
    );
}

#[test]
fn a_cards_context_menu_offers_open_move_thread_and_delete() {
    let menu = dump("task-menu", 1280.0);
    for text in [
        "\"Open\"",
        "\"Move to\"",
        "\"In progress  ›\"",
        "\"Go to thread\"",
        "\"Delete task\"",
    ] {
        assert!(menu.contains(text), "{text} missing: {menu}");
    }
}

#[test]
fn colors_tags_and_column_actions_show() {
    let board = dump("board", 1280.0);
    for text in [
        "\"backend\"",
        "\"copy\"",
        "\"Filters  ▾\"",
        "\"Search this board\"",
        "\"+ Column\"",
        "\"2 files\"",
    ] {
        assert!(board.contains(text), "{text} missing: {board}");
    }
    let task = dump("task", 1280.0);
    for text in [
        "\"Color\"",
        "\"Tags\"",
        "\"legal\"",
        "\"+ backend\"",
        "\"Attachments\"",
        "\"tiers-mockup.png\"",
        "\"PDF\"",
        "\"47.1 KB\"",
        "\"Add files…\"",
    ] {
        assert!(task.contains(text), "{text} missing: {task}");
    }
    let menu = dump("column-menu", 1280.0);
    for text in [
        "\"Rename…\"",
        "\"Teal  ›\"",
        "\"Move left\"",
        "\"Remove column…\"",
    ] {
        assert!(menu.contains(text), "{text} missing: {menu}");
    }
}

#[test]
fn a_subtask_opens_with_where_it_is_and_its_own_subtasks() {
    let nested = dump("task-nested", 1280.0);
    for text in [
        "\"Edit subtask\"",
        "\"Pricing page copy\"",
        "\"Not done\"",
        "\"Make standalone\"",
        "\"EU terms\"",
        "\"Subtasks  1/2\"",
        "\"Delete subtask\"",
    ] {
        assert!(nested.contains(text), "{text} missing: {nested}");
    }
    let nest = dump("nest", 1280.0);
    for text in [
        "\"Pricing page copy › Legal review › EU terms\"",
        "\"Stripe sandbox keys\"",
    ] {
        assert!(nest.contains(text), "{text} missing: {nest}");
    }
    // Once, on its card behind the dialog: it isn't offered as its own parent.
    assert_eq!(nest.matches("\"Invoice PDF export\"").count(), 1, "{nest}");
}

#[test]
fn a_new_subtask_has_an_editor_of_its_own() {
    let new = dump("task-new-subtask", 1280.0);
    for text in [
        "\"New subtask\"",
        "\"Translate the FAQ\"",
        "\"Notes\"",
        "\"Tags\"",
        "\"Remove subtask\"",
        "\"Done\"",
        "\"Save\"",
    ] {
        assert!(new.contains(text), "{text} missing: {new}");
    }
    assert!(!new.contains("\"Column\""), "it goes under its task: {new}");
    assert!(!new.contains("\"Add a subtask…\""), "{new}");
    let task = dump("task", 1280.0);
    assert!(task.contains("\"New subtask…\""), "{task}");
}

#[test]
fn the_task_editors_buttons_stay_on_screen_in_a_short_window() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    for name in ["task", "jira-issue"] {
        for height in [520.0, 640.0] {
            let frame = scenario_frame(name, &theme, &mut text, 1280.0, height).unwrap();
            let save = frame
                .access
                .iter()
                .find(|a| a.label == "Save")
                .unwrap_or_else(|| panic!("no Save in {name}"));
            assert!(
                save.rect.y >= 0.0 && save.rect.y + save.rect.height <= height,
                "{name} at {height}: Save at {:?}",
                save.rect
            );
        }
    }
}

#[test]
fn long_subtask_titles_are_cut_short_inside_the_editor() {
    let task = dump("task", 1280.0);
    assert!(
        !task.contains(LONG_SUBTASK),
        "the whole title overflows the editor: {task}"
    );
    assert!(
        task.contains("\"Publish the pricing page"),
        "the start of the title shows: {task}"
    );
}

#[test]
fn the_board_status_bar_and_task_editor_show_their_content() {
    let board = dump("board", 1280.0);
    for text in [
        "\"Billing launch\"",
        "\"In progress\"",
        "\"Retry failed webhooks\"",
    ] {
        assert!(board.contains(text), "{text} missing: {board}");
    }
    let status = dump("status", 1280.0);
    assert!(status.contains("\"Claude · Max 20x\""), "{status}");
    assert!(status.contains("\"41%\""), "{status}");
    let task = dump("task", 1280.0);
    for text in [
        "\"Edit task\"",
        "\"Subtasks  1/3\"",
        "\"Legal review\"",
        "\"Go to thread\"",
        "\"Start thread\"",
    ] {
        assert!(task.contains(text), "{text} missing: {task}");
    }
    assert!(
        board.contains("\"1/3\""),
        "subtask progress on the card: {board}"
    );
    assert!(board.contains("image "), "the logo is drawn: {board}");
}

#[test]
fn a_task_can_start_in_an_existing_thread() {
    let dump = dump("task-in-thread", 1280.0);
    for text in [
        "\"Start in a thread\"",
        "\"New thread\"",
        "\"Existing thread\"",
        "\"Start in thread\"",
        "\"A new tab there starts Claude in plan mode with this task as its prompt.\"",
    ] {
        assert!(dump.contains(text), "{text} missing: {dump}");
    }
    assert!(dump.contains(" › "), "threads are listed by repo: {dump}");
    assert!(!dump.contains("\"Branch\""), "no new-thread fields: {dump}");
}

#[test]
fn panes_split_and_close_from_their_context_menu_not_buttons() {
    let menu = dump("pane-menu", 1280.0);
    for text in [
        "\"Split right\"",
        "\"Split down\"",
        "\"Copy\"",
        "\"Paste\"",
        "\"Close pane\"",
    ] {
        assert!(menu.contains(text), "{text} missing: {menu}");
    }
    // Desktop has a secondary click, so the tab bar has no split buttons.
    let status = dump("status", 1280.0);
    for text in ["\"→\"", "\"↓\""] {
        assert!(!status.contains(text), "{text} in the tab bar: {status}");
    }
}

#[test]
fn threads_show_when_their_agent_finished_or_waits() {
    // "Pricing page" has a finished agent, "Checkout copy" one waiting for permission.
    let sidebar = dump("status", 1280.0);
    for text in ["\"finished\"", "\"needs input\""] {
        assert!(sidebar.contains(text), "{text} missing: {sidebar}");
    }
}

#[test]
fn cards_show_when_an_agent_on_their_threads_finished_or_waits() {
    // "Invoice PDF export" is linked to a waiting thread; a nested subtask of "Pricing
    // page copy" to a finished one.
    let board = dump("board", 1280.0);
    for text in ["\"Needs input\"", "\"Finished\""] {
        assert!(board.contains(text), "{text} missing: {board}");
    }
}

#[test]
fn the_new_thread_dialog_offers_every_project_or_none() {
    let closed = dump("new-thread", 1280.0);
    for text in ["\"Project\"", "\"Billing launch\""] {
        assert!(closed.contains(text), "{text} missing: {closed}");
    }
    let open = dump("new-thread-project", 1280.0);
    for text in ["\"None\"", "\"Onboarding\""] {
        assert!(open.contains(text), "{text} missing: {open}");
    }
    // The sidebar behind the dialog lists it too; the open list adds one more.
    assert_eq!(
        open.matches("\"Onboarding\"").count(),
        closed.matches("\"Onboarding\"").count() + 1,
        "{open}"
    );
}

#[test]
fn jira_epics_are_cards_and_new_ones_get_no_subtasks() {
    let board = dump("jira-board", 1280.0);
    assert!(board.contains("\"ACME-7\""), "the epic's card: {board}");
    let labels = access_labels("jira-new-epic");
    for kind in ["Type Task", "Type Story", "Type Bug", "Type Epic"] {
        assert!(
            labels.iter().any(|l| l == kind),
            "{kind} missing: {labels:?}"
        );
    }
    let editor = dump("jira-new-epic", 1280.0);
    assert!(
        editor.contains("\"Jira epics hold stories, not sub-tasks.\""),
        "{editor}"
    );
    assert!(!editor.contains("\"New subtask…\""), "{editor}");
}

#[test]
fn jira_boards_show_their_issues_and_leave_jiras_part_to_jira() {
    let board = dump("jira-board", 1280.0);
    for text in [
        "\"ACME-142\"",
        "\"Checkout with saved cards\"",
        "\"Payments\"",
        "\"AL\"",
        "\"Synced 2 min ago\"",
        "\"Sync now\"",
        "\"Open in Jira\"",
        "\"Remove\"",
        "\"Filters  ▾\"",
        "\"Only my issues\"",
        "\"Recently updated\"",
    ] {
        assert!(board.contains(text), "{text} missing: {board}");
    }
    for text in ["\"Rename\"", "\"+ Column\""] {
        assert!(!board.contains(text), "{text} shown: {board}");
    }

    let task = dump("jira-task", 1280.0);
    for text in [
        "\"ACME-139\"",
        "\"Grace Hopper\"",
        "\"In Progress\"",
        "\"Billing (ACME-8)\"",
    ] {
        assert!(task.contains(text), "{text} missing: {task}");
    }
    for text in ["\"Delete task\"", "\"Add files…\""] {
        assert!(!task.contains(text), "{text} shown: {task}");
    }

    let connect = dump("jira-connect", 1280.0);
    assert!(connect.contains("\"Connect Jira\"") && connect.contains("\"API token\""));
    let import = dump("jira-import", 1280.0);
    assert!(import.contains("\"Mobile sprint\"") && import.contains("imported"));
}

#[test]
fn ordinary_tasks_offer_a_type() {
    let task = dump("task", 1280.0);
    for text in ["\"Type\"", "\"Story\"", "\"Bug\"", "\"Epic\""] {
        assert!(task.contains(text), "{text} missing: {task}");
    }
}

#[test]
fn jira_cards_are_the_same_size_as_any_other() {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    let frame = scenario_frame("jira-board", &theme, &mut text, 1280.0, 820.0).unwrap();
    let heights: Vec<f32> = frame
        .access
        .iter()
        .filter(|a| a.label.starts_with("Task "))
        .map(|a| a.rect.height)
        .collect();
    assert!(heights.len() >= 4, "{heights:?}");
    assert!(
        heights.windows(2).all(|w| (w[0] - w[1]).abs() < 0.5),
        "{heights:?}"
    );
}

/// The accessibility labels of a scenario at desktop size.
fn access_labels(name: &str) -> Vec<String> {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    scenario_frame(name, &theme, &mut text, 1280.0, 820.0)
        .unwrap()
        .access
        .iter()
        .map(|a| a.label.clone())
        .collect()
}

#[test]
fn jira_subtasks_cant_be_deleted_here() {
    let ordinary = access_labels("task");
    assert!(
        ordinary.iter().any(|l| l.starts_with("Delete subtask ")),
        "{ordinary:?}"
    );
    let jira = access_labels("jira-task");
    assert!(
        jira.iter()
            .any(|l| l == "Task Template" || l.contains("Template")),
        "{jira:?}"
    );
    assert!(
        !jira.iter().any(|l| l.starts_with("Delete subtask ")),
        "{jira:?}"
    );
}

#[test]
fn a_subtask_opened_from_its_parent_heads_its_editor_with_where_it_is() {
    for width in [1280.0, 402.0] {
        let sub = dump("jira-subtask", width);
        for text in ["\"‹\"", "\"Billing\"", "\"ACME-139\"", "\"ACME-141\""] {
            assert!(sub.contains(text), "{text} missing at {width}: {sub}");
        }
    }
    let task = dump("jira-task", 1280.0);
    // The parent's editor lists its subtasks with their keys.
    assert!(task.contains("\"ACME-141\""), "{task}");
}

#[test]
fn a_jira_board_filters_by_people_and_its_quick_filters() {
    let filtered = dump("jira-filters", 1280.0);
    for text in [
        "\"Bugs\"",
        "\"Blocked\"",
        "\"Filters (2)  ▾\"",
        "\"Clear\"",
        "\"GH\"",
    ] {
        assert!(filtered.contains(text), "{text} missing: {filtered}");
    }
    // Only Ada's (recently updated) issues.
    assert!(
        filtered.contains("\"Checkout with saved cards\""),
        "{filtered}"
    );
    assert!(!filtered.contains("\"Invoice PDFs\""), "{filtered}");
    let menu = dump("jira-filter-menu", 1280.0);
    for text in [
        "\"Assignee\"",
        "\"Grace Hopper\"",
        "\"Ada Lovelace\"",
        "\"✓\"",
    ] {
        assert!(menu.contains(text), "{text} missing: {menu}");
    }
    let phone = dump("jira-filters", 402.0);
    assert!(
        phone.contains("\"Filters (2)  ▾\"") && !phone.contains("\"Blocked\""),
        "{phone}"
    );
}

#[test]
fn an_issue_in_full_edits_every_field_and_shows_the_conversation() {
    for width in [1280.0, 402.0] {
        let issue = dump("jira-issue", width);
        for text in [
            "\"Issue Type\"",
            "\"Sprint 6\"",
            "\"Story points\"",
            "\"Watching (1)\"",
            "\"Comments 2\"",
            "\"Add a comment… (Markdown)\"",
        ] {
            assert!(issue.contains(text), "{text} missing at {width}: {issue}");
        }
    }
    let picker = dump("jira-issue-picker", 1280.0);
    for text in [
        "\"Assign to me\"",
        "\"Alan Turing\"",
        "\"Find a person (Enter)\"",
    ] {
        assert!(picker.contains(text), "{text} missing: {picker}");
    }
}

#[test]
fn the_look_is_chosen_in_settings_and_shown_at_once() {
    let look = dump("settings-appearance", 1280.0);
    for text in [
        "\"Appearance\"",
        "\"Ocean\"",
        "\"Custom\"",
        "\"Logo\"",
        "\"Hidden\"",
    ] {
        assert!(look.contains(text), "{text} missing: {look}");
    }
    // The preview's accent (Ocean's blue) is on the Save button already.
    assert!(look.contains("#2f86e0"), "{look}");
    let custom = dump("settings-custom", 1280.0);
    for text in ["\"Glow 2\"", "\"#e0b84c\""] {
        assert!(custom.contains(text), "{text} missing: {custom}");
    }
    assert!(
        custom.contains("#e0b84c"),
        "the typed accent shows: {custom}"
    );
    // A hidden logo is gone from the sidebar; the default one is there.
    let logo = |name: &str| dump(name, 1280.0).contains("image 20,20 24x24");
    assert!(logo("launcher"));
    assert!(!logo("logo-hidden"));
}

#[test]
fn saving_the_look_sends_it_only_when_changed() {
    use crate::msg::Msg;
    use urushi_client::v1::request::Body;
    let mut s = crate::state::AppState::new(
        crate::config::ConnectTarget::Local(None),
        urushi_ui::TextSystem::bundled_only().cell_metrics(13.0),
    );
    s.daemon_minor = urushi_proto::PROTOCOL_MINOR;
    s.update(Msg::OpenSettings);
    let patch = |fx: Vec<crate::msg::Effect>| {
        fx.into_iter()
            .find_map(|e| match e {
                crate::msg::Effect::Request {
                    body: Body::UpdateSettings(r),
                    ..
                } => r.patch,
                _ => None,
            })
            .unwrap()
    };
    assert!(
        patch(s.update(Msg::Submit)).appearance.is_none(),
        "unchanged"
    );
    s.update(Msg::OpenSettings);
    s.update(Msg::LookScheme(Some(urushi_core::Scheme::Ember)));
    assert_eq!(
        s.appearance().scheme,
        urushi_core::Scheme::Ember,
        "a live preview"
    );
    s.update(Msg::CloseDialog);
    assert_eq!(
        s.appearance(),
        urushi_core::Appearance::default(),
        "Close reverts it"
    );
    s.update(Msg::OpenSettings);
    s.update(Msg::LookScheme(Some(urushi_core::Scheme::Ember)));
    let sent = patch(s.update(Msg::Submit)).appearance.unwrap();
    assert_eq!(
        sent.scheme,
        i32::from(urushi_client::v1::ColorScheme::Ember)
    );
}

#[test]
fn a_projects_parent_branch_thread_and_its_settings_are_shown() {
    let sidebar = dump("project-base-thread", 1280.0);
    let parent = sidebar
        .find("Parent branch · urushi · main")
        .expect(&sidebar);
    let pinned = sidebar.find("Pinned · urushi · urushi/t").expect(&sidebar);
    assert!(parent < pinned, "first in its project: {sidebar}");
    let dialog = dump("project-settings", 1280.0);
    for text in [
        "Billing launch settings",
        "\"develop\"",
        "\"origin/release\"",
    ] {
        assert!(dialog.contains(text), "{text} missing: {dialog}");
    }
}
