//! How tasks get their message (UR-30): sent or drafted, and the user's own message,
//! in Settings and in what Start thread says it will do.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::bodies;
use super::setup::on;
use crate::msg::{Followup, Msg};
use crate::state::{AppState, Field, SettingToggle};

/// Settings with Claude as the task agent, and the message settings given.
fn settings(draft: bool, custom: bool, message: &str) -> Msg {
    Msg::Response {
        followup: Followup::Settings,
        result: Ok(Outcome::Settings(v1::Settings {
            agents: Some(v1::AgentSettings {
                task_agent: v1::Agent::Claude.into(),
                task_draft: draft,
                task_message_custom: custom,
                task_message: message.into(),
                ..Default::default()
            }),
            ..Default::default()
        })),
    }
}

/// The one settings patch `msg` sends, if any.
fn patch_of(s: &mut AppState, msg: Msg) -> Option<v1::SettingsPatch> {
    match bodies(&s.update(msg))[..] {
        [Body::UpdateSettings(v1::UpdateSettingsRequest { patch: Some(p) })] => Some(p.clone()),
        [] => None,
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn the_switches_change_the_setting_right_away() {
    let mut s = on(19);
    s.update(settings(false, false, ""));
    let p = patch_of(&mut s, Msg::Toggle(SettingToggle::TaskSend)).unwrap();
    assert_eq!(p.task_draft, Some(true), "sending is on, so this drafts");
    assert_eq!(p.task_message_custom, None);
    let p = patch_of(&mut s, Msg::Toggle(SettingToggle::TaskMessageCustom)).unwrap();
    assert_eq!(p.task_message_custom, Some(true));
    assert_eq!(p.task_draft, None);

    s.update(settings(true, true, "Do {key}"));
    let p = patch_of(&mut s, Msg::Toggle(SettingToggle::TaskSend)).unwrap();
    assert_eq!(p.task_draft, Some(false));
    let p = patch_of(&mut s, Msg::Toggle(SettingToggle::TaskMessageCustom)).unwrap();
    assert_eq!(p.task_message_custom, Some(false));
}

#[test]
fn an_older_daemon_keeps_sending_the_default_message() {
    let mut s = on(18);
    // Even if a newer client stored these, a 1.18 daemon ignores them.
    s.update(settings(true, true, "Do {key}"));
    assert!(!s.task_delivery().is_draft());
    assert_eq!(s.task_message(), (false, ""));
    assert_eq!(s.task_message_phrase(), "with this task as its prompt");
    assert!(patch_of(&mut s, Msg::Toggle(SettingToggle::TaskSend)).is_none());
    assert!(patch_of(&mut s, Msg::Toggle(SettingToggle::TaskMessageCustom)).is_none());
    s.update(Msg::OpenSettings);
    let p = patch_of(&mut s, Msg::Submit).unwrap();
    assert_eq!(
        p.task_message, None,
        "not sent to a daemon that can't keep it"
    );
}

#[test]
fn the_message_is_edited_in_settings_and_saved_with_save() {
    let mut s = on(19);
    s.update(settings(false, false, "Old {key}"));
    s.update(Msg::OpenSettings);
    assert!(
        !s.dialog_fields().contains(&Field::TaskMessage),
        "no message box while it is off"
    );
    // Turning it on puts the cursor in the box, which shows the saved text.
    patch_of(&mut s, Msg::Toggle(SettingToggle::TaskMessageCustom)).unwrap();
    assert_eq!(s.focus, Some(Field::TaskMessage));
    s.update(settings(false, true, "Old {key}"));
    assert_eq!(s.dialog_fields()[0], Field::TaskMessage);
    assert_eq!(
        s.field().map(|f| f.text().to_owned()),
        Some("Old {key}".into())
    );
    s.update(Msg::Key(super::named(crate::keys::Named::Enter)));
    s.update(Msg::Text("Plan first.".into()));
    assert!(s.dialog.is_some(), "Enter adds a line, it doesn't save");
    let p = patch_of(&mut s, Msg::Submit).unwrap();
    assert_eq!(p.task_message.as_deref(), Some("Old {key}\nPlan first."));
    assert!(s.dialog.is_none());
}

#[test]
fn start_thread_says_how_the_task_gets_its_message() {
    let mut s = on(19);
    for (draft, custom, text, phrase) in [
        (false, false, "", "with this task as its prompt"),
        (false, true, "Do {key}", "with your task message"),
        (false, true, "  ", "with this task as its prompt"),
        (
            true,
            false,
            "",
            "with this task's message drafted for you to send",
        ),
        (
            true,
            true,
            "Do {key}",
            "with your task message drafted for you to send",
        ),
    ] {
        s.update(settings(draft, custom, text));
        assert_eq!(s.task_message_phrase(), phrase, "{draft} {custom} {text:?}");
    }
}
