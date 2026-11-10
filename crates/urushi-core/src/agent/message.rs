//! The user's own message for a task's agent, used instead of Urushi's default one
//! (UR-30). Its placeholders are filled in per task by `board::custom_task_prompt`.

use serde::{Deserialize, Serialize};

/// Longest custom task message, in characters (as long as a task's notes).
pub const MAX_TASK_MESSAGE: usize = 10_000;

/// The placeholders a custom task message may use, as written in it:
/// - `{key}`: the Jira issue's key, else the task's id
/// - `{title}`: the task's title
/// - `{url}`: the Jira issue's address, else nothing
/// - `{show}`: the command that prints the task in full (`urushi board show …`)
pub const TASK_MESSAGE_PLACEHOLDERS: [&str; 4] = ["{key}", "{title}", "{url}", "{show}"];

/// A custom task message and whether it is on. The text is kept while it is off, so
/// switching it back on restores it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskMessage {
    /// Use [`Self::text`] instead of the default message. Off by default.
    pub custom: bool,
    /// The message, with placeholders ([`TASK_MESSAGE_PLACEHOLDERS`]).
    pub text: String,
}

impl TaskMessage {
    /// The text to start tasks with: `None` when it is off or blank, which means the
    /// default message.
    pub fn template(&self) -> Option<&str> {
        (self.custom && !self.text.trim().is_empty()).then_some(self.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_enabled_non_blank_message_replaces_the_default() {
        let mut m = TaskMessage::default();
        assert_eq!(m.template(), None);
        m.text = "Do {key}".into();
        assert_eq!(m.template(), None, "off keeps the default");
        m.custom = true;
        assert_eq!(m.template(), Some("Do {key}"));
        m.text = "  \n ".into();
        assert_eq!(m.template(), None, "blank means the default");
    }
}
