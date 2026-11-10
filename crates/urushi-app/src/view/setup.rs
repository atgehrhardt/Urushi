//! The task agent's screens: the first-run setup wizard (agent, then mode), and the rows
//! in Settings that change the choice later, with how tasks get their message.

use urushi_core::{Agent, AgentMode};
use urushi_ui::widgets::{ButtonKind, button, toggle};
use urushi_ui::{Font, Insets, Length, Node, Role, TextField, Theme, key};

use super::dialogs::{area, hint, label};
use crate::msg::Msg;
use crate::state::{AppState, Field, SettingToggle};
use crate::task_agent::{SetupMsg, SetupStep, mode_detail};
use crate::view::modal::modal;

/// The setup wizard, on `step`, with `agent` and `mode` chosen so far.
pub(super) fn wizard(
    state: &AppState,
    theme: &Theme,
    step: SetupStep,
    agent: Agent,
    mode: AgentMode,
) -> Node<Msg> {
    let body = Node::column().width(Length::Fill(1.0)).gap(8.0);
    let (body, actions) = match step {
        SetupStep::Agent => (
            body.child(heading(theme, "Which agent should work on your tasks?"))
                .child(hint(
                    theme,
                    "Start thread on a task runs this agent with the task as its prompt.",
                ))
                .children(state.agents.iter().map(|&a| {
                    option(
                        theme,
                        ("setup-agent", a.display_name()),
                        a.display_name(),
                        &format!("Runs `{}` in the thread's worktree", a.program()),
                        a == agent,
                        Msg::Setup(SetupMsg::PickAgent(a)),
                    )
                })),
            vec![
                button(
                    theme,
                    "Later",
                    ButtonKind::Secondary,
                    Msg::Setup(SetupMsg::Later),
                ),
                button(
                    theme,
                    "Next",
                    ButtonKind::Primary,
                    Msg::Setup(SetupMsg::Next),
                ),
            ],
        ),
        SetupStep::Mode => (
            body.child(heading(theme, &format!("How should {agent} start?")))
                .children(
                    AgentMode::ALL
                        .into_iter()
                        .filter(|m| agent.supports(*m))
                        .map(|m| {
                            option(
                                theme,
                                ("setup-mode", m.display_name()),
                                m.display_name(),
                                mode_detail(m),
                                m == agent.effective_mode(mode),
                                Msg::Setup(SetupMsg::PickMode(m)),
                            )
                        }),
                )
                .child(hint(theme, "You can change this any time in Settings.")),
            vec![
                button(
                    theme,
                    "Back",
                    ButtonKind::Secondary,
                    Msg::Setup(SetupMsg::Back),
                ),
                button(
                    theme,
                    "Save",
                    ButtonKind::Primary,
                    Msg::Setup(SetupMsg::Finish),
                ),
            ],
        ),
    };
    modal(
        state,
        theme,
        "Set up Urushi",
        520.0,
        body,
        actions,
        Some(Msg::Setup(SetupMsg::Later)),
    )
}

/// Lines the task message box shows.
const MESSAGE_LINES: u8 = 4;

/// Settings rows for the task agent and its mode (a 1.7 daemon only), and how tasks get
/// their message (1.19), with `message` the user's own being edited.
pub(super) fn settings_rows(
    state: &AppState,
    theme: &Theme,
    message: &TextField,
) -> Vec<Node<Msg>> {
    if !state.daemon_supports_task_agent() {
        return Vec::new();
    }
    let chosen = state.task_agent();
    let agents = Node::row()
        .gap(8.0)
        .children(Agent::ALL.into_iter().map(|a| {
            let kind = if chosen.is_some_and(|(c, _)| c == a) {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            button(theme, a.display_name(), kind, Msg::SetTaskAgent(a))
        }));
    let mut rows = vec![label(theme, "Tasks start with"), agents];
    match chosen {
        Some((agent, mode)) => {
            let modes = Node::row().gap(8.0).children(
                AgentMode::ALL
                    .into_iter()
                    .filter(|m| agent.supports(*m))
                    .map(|m| {
                        let kind = if m == mode {
                            ButtonKind::Primary
                        } else {
                            ButtonKind::Secondary
                        };
                        button(theme, m.display_name(), kind, Msg::SetTaskMode(m))
                    }),
            );
            rows.push(modes);
            rows.push(hint(theme, mode_detail(mode)));
            rows.extend(message_rows(state, theme, agent, message));
        }
        None => rows.push(hint(
            theme,
            "None yet: threads started from tasks open on the launcher.",
        )),
    }
    rows
}

/// Whether a task's message is sent or drafted, and the user's own message: two
/// switches, independent of each other, and the message while it is on (UR-30).
fn message_rows(
    state: &AppState,
    theme: &Theme,
    agent: Agent,
    message: &TextField,
) -> Vec<Node<Msg>> {
    if !state.daemon_supports_task_message() {
        return Vec::new();
    }
    let send = !state.task_delivery().is_draft();
    let mut rows = vec![toggle(
        theme,
        send,
        "Send the task's message when the agent starts",
        Msg::Toggle(SettingToggle::TaskSend),
    )];
    if !send {
        rows.push(hint(
            theme,
            &format!("Off: the message is typed into {agent} for you to edit and send."),
        ));
    }
    let custom = state.task_message().0;
    rows.push(toggle(
        theme,
        custom,
        "Use my own task message",
        Msg::Toggle(SettingToggle::TaskMessageCustom),
    ));
    if custom {
        rows.push(area(
            state,
            theme,
            message,
            Field::TaskMessage,
            ("Pick up {key} \"{title}\": {url}", MESSAGE_LINES),
        ));
        rows.push(hint(
            theme,
            "Saved with Save. Filled in per task: {key} (issue key or task id), {title}, \
             {url} (the Jira issue's address) and {show} (the command that prints the task). \
             Left empty, the default message is used.",
        ));
    }
    rows
}

fn heading(theme: &Theme, text: &str) -> Node<Msg> {
    Node::text(text, Font::UiMedium, theme.font_size, theme.text)
}

/// A selectable card: a title and a line of detail, outlined in the accent when chosen.
fn option(
    theme: &Theme,
    id: (&str, &str),
    title: &str,
    detail: &str,
    selected: bool,
    on_click: Msg,
) -> Node<Msg> {
    Node::column()
        .width(Length::Fill(1.0))
        .padding(Insets::symmetric(10.0, 12.0))
        .gap(2.0)
        .radius(theme.radius)
        .background(theme.surface)
        .hover_background(theme.hover)
        .border(
            if selected { 2.0 } else { 1.0 },
            if selected { theme.accent } else { theme.border },
        )
        .key(key(&[id.0, id.1]))
        .on_click(on_click)
        .role(
            Role::Button,
            format!("{title}{}", if selected { " (chosen)" } else { "" }),
        )
        .child(Node::text(
            title,
            Font::UiMedium,
            theme.font_size,
            theme.text,
        ))
        .child(Node::text(
            detail,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
}
