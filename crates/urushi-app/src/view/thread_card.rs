//! A thread's card in the sidebar: its title (with a dot when its agent finished or
//! waits, and the Jira and GitHub marks of its task's issue and pull request, which
//! open them in a browser, ADR 0036), the repo and branch it's on, and its lifecycle
//! badge. Click to open it,
//! right-click for its menu, drag to file it under another project. A project's
//! parent-branch thread (ADR 0029) stays at its project's top: it can't be dragged, and
//! is closed from its menu rather than a delete button.

use urushi_client::{ThreadLinks, v1};
use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use crate::brand_icons::{Brand, mark};
use crate::msg::Msg;
use crate::organize::{Dragged, MenuTarget};
use crate::state::AppState;

/// Size of a brand mark on a card.
const MARK: f32 = 12.0;

/// A thread: title, then which repo and branch it's on. Click to open, right-click
/// (two-finger click) for its menu, drag to move it to another project. `links` are
/// its task's Jira issue and pull request.
pub(super) fn thread_card(
    state: &AppState,
    theme: &Theme,
    thread: &v1::Thread,
    links: Option<&ThreadLinks>,
) -> Node<Msg> {
    let selected = state.selected_thread.as_deref() == Some(&thread.id);
    let dragged = state
        .dragging
        .as_ref()
        .is_some_and(|(d, _)| matches!(d, Dragged::Thread(t) if *t == thread.id));
    let badge = match v1::ThreadState::try_from(thread.state) {
        Ok(v1::ThreadState::Ready) => None,
        Ok(v1::ThreadState::DeletionFailed) => Some(("deletion failed", theme.danger)),
        Ok(v1::ThreadState::Deleting) => Some(("deleting", theme.warning)),
        _ => Some(("creating", theme.warning)),
    };
    let attention = state.thread_attention(&thread.id);
    let mut details = Node::column()
        .width(Length::Fill(1.0))
        .gap(1.0)
        .child(thread_title(theme, thread, (selected, attention), links))
        .child(Node::text(
            thread_meta(state, thread),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ));
    // The thread's lifecycle (creating, deleting) matters more than its agent.
    let badge = badge.or_else(|| {
        attention.map(|k| {
            (
                super::attention::label(k),
                super::attention::color(theme, k),
            )
        })
    });
    if let Some((label, color)) = badge {
        details = details.child(Node::text(
            label,
            Font::UiMedium,
            theme.font_size_small,
            color,
        ));
    }
    let background = if selected || dragged {
        theme.selected
    } else {
        Color::TRANSPARENT
    };
    let card = Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 6.0,
            right: 4.0,
            bottom: 6.0,
            left: 12.0,
        })
        .gap(4.0)
        .align(Align::Center)
        .radius(theme.radius)
        .background(background)
        .hover_background(if selected {
            theme.selected
        } else {
            theme.hover
        })
        .key(key(&["thread", &thread.id]))
        .on_click(Msg::SelectThread(thread.id.clone()))
        .on_secondary_click(Msg::OpenMenu(MenuTarget::Thread(thread.id.clone())))
        .role(
            Role::Item,
            format!(
                "Thread {}{}{}",
                thread.title,
                if thread.project_base {
                    ", parent branch"
                } else if thread.pinned {
                    ", pinned"
                } else {
                    ""
                },
                attention.map_or(String::new(), |k| format!(
                    ", {}",
                    super::attention::label(k)
                )),
            ),
        )
        .child(details);
    if thread.project_base {
        return card;
    }
    card.on_drag(Msg::DragThread(thread.id.clone()))
        .child(icon_button(
            theme,
            "×",
            &format!("Delete thread {}", thread.title),
            Msg::RequestDeleteThread(thread.id.clone()),
        ))
}

/// A thread's title, then the marks of its links (ADR 0036), then a dot when its
/// agent finished or waits (ADR 0017).
fn thread_title(
    theme: &Theme,
    thread: &v1::Thread,
    (selected, attention): (bool, Option<v1::AttentionKind>),
    links: Option<&ThreadLinks>,
) -> Node<Msg> {
    let color = if selected {
        theme.text
    } else {
        theme.text.mix(theme.text_muted, 0.3)
    };
    Node::row()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center)
        .child(
            Node::text(&thread.title, Font::UiMedium, theme.font_size, color)
                .width(Length::Fill(1.0)),
        )
        .children(
            links
                .into_iter()
                .flat_map(|l| link_marks(theme, &thread.id, l)),
        )
        .children(attention.map(|k| super::attention::indicator(theme, k, super::attention::DOT)))
}

/// The Jira mark for the thread's issue and the GitHub mark for its pull request. Each
/// opens its page; a click there doesn't also open the thread.
fn link_marks(theme: &Theme, thread: &str, links: &ThreadLinks) -> Vec<Node<Msg>> {
    let issue = links.issue.as_ref().map(|i| {
        let label = format!("Open {} in Jira", i.key);
        (Brand::Jira, "jira", label, i.url.clone())
    });
    let pr = links.pull_request.as_ref().map(|p| {
        let label = if p.title.is_empty() {
            format!("Open pull request {}", p.label)
        } else {
            format!("Open pull request {}: {}", p.label, p.title)
        };
        (Brand::GitHub, "pr", label, p.url.clone())
    });
    issue
        .into_iter()
        .chain(pr)
        .filter_map(|(brand, name, label, url)| {
            let image = mark(brand, theme.text_muted)?;
            Some(
                Node::row()
                    .padding(Insets::symmetric(3.0, 3.0))
                    .radius(theme.radius)
                    .hover_background(theme.hover)
                    .key(key(&["thread-link", thread, name]))
                    .on_click(Msg::OpenUrl(url))
                    .role(Role::Button, label)
                    .child(Node::image(image, MARK, MARK)),
            )
        })
        .collect()
}

/// Which repo and branch a thread is on. Pinned threads leave their status section, so
/// they show the status here. A parent-branch thread says so.
fn thread_meta(state: &AppState, thread: &v1::Thread) -> String {
    let repo = state
        .workspace
        .repo(&thread.project_id)
        .map_or("unknown repo", |r| r.name.as_str());
    let mut meta = format!("{repo} · {}", thread.primary_branch);
    if thread.pinned && !thread.status.is_empty() {
        meta = format!("{} · {meta}", thread.status);
    }
    if thread.project_base {
        meta = format!("Parent branch · {meta}");
    } else if thread.pinned {
        meta = format!("Pinned · {meta}");
    }
    meta
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use urushi_client::v1;
    use urushi_ui::{CellMetrics, Point, Rect, Role, TextSystem, Theme, build};

    use crate::config::ConnectTarget;
    use crate::msg::{Effect, Msg};
    use crate::state::AppState;

    const ISSUE: &str = "https://acme.atlassian.net/browse/ACME-1";
    const PR: &str = "https://github.com/acme/app/pull/7";
    const LOCAL_PR: &str = "https://github.com/acme/app/pull/9";

    /// Threads `jira` (started from Jira issue ACME-1, which has an open pull request),
    /// `local` (from a local task with a pull request link) and `plain` (no task).
    fn state() -> AppState {
        let mut state = AppState::new(
            ConnectTarget::Local(None),
            CellMetrics {
                width: 8.0,
                height: 16.0,
            },
        );
        let thread = |id: &str| v1::Thread {
            id: id.into(),
            project_id: "r".into(),
            title: format!("Thread {id}"),
            primary_branch: format!("urushi/{id}"),
            state: v1::ThreadState::Ready.into(),
            ..Default::default()
        };
        let issue = v1::JiraIssue {
            key: "ACME-1".into(),
            url: ISSUE.into(),
            pull_requests: vec![v1::PullRequest {
                url: PR.into(),
                title: "ACME-1 fix it".into(),
                status: v1::PullRequestStatus::Open.into(),
            }],
            ..Default::default()
        };
        state.update(Msg::Connected {
            welcome: v1::Welcome::default(),
            workspace: v1::Workspace {
                projects: vec![v1::Project {
                    id: "r".into(),
                    name: "api".into(),
                    ..Default::default()
                }],
                threads: vec![thread("jira"), thread("local"), thread("plain")],
                boards: vec![v1::Board {
                    id: "b".into(),
                    columns: vec![v1::BoardColumn {
                        id: "c".into(),
                        tasks: vec![
                            v1::Task {
                                id: "t1".into(),
                                thread_id: "jira".into(),
                                issue: Some(Box::new(issue)),
                                ..Default::default()
                            },
                            v1::Task {
                                id: "t2".into(),
                                thread_id: "local".into(),
                                pr_url: LOCAL_PR.into(),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
        });
        state
    }

    fn render(state: &AppState) -> urushi_ui::Frame<Msg> {
        let mut text = TextSystem::bundled_only();
        build(
            &crate::view::view(state, &Theme::dark(), &HashMap::new()),
            Rect::new(0.0, 0.0, 1200.0, 800.0),
            &mut text,
            None::<Point>,
        )
    }

    fn labels(frame: &urushi_ui::Frame<Msg>) -> Vec<&str> {
        frame
            .access
            .iter()
            .filter(|a| {
                a.role == Role::Button
                    && (a.label.starts_with("Open pull request") || a.label.ends_with("in Jira"))
            })
            .map(|a| a.label.as_str())
            .collect()
    }

    #[test]
    fn cards_show_the_marks_of_their_threads_issue_and_pull_request() {
        let frame = render(&state());
        assert_eq!(
            labels(&frame),
            [
                "Open ACME-1 in Jira",
                "Open pull request #7: ACME-1 fix it",
                "Open pull request #9",
            ]
        );
        assert!(
            frame.scene.dump().matches("image").count() >= 3,
            "{}",
            frame.scene.dump()
        );
    }

    #[test]
    fn clicking_a_mark_opens_its_page_and_not_the_thread() {
        let mut state = state();
        let frame = render(&state);
        let mark = frame
            .hits
            .iter()
            .find(|h| matches!(&h.on_click, Some(Msg::OpenUrl(u)) if u == ISSUE))
            .expect("the Jira mark");
        let center = Point::new(
            mark.rect.x + mark.rect.width / 2.0,
            mark.rect.y + mark.rect.height / 2.0,
        );
        let msg = frame.click(center).expect("a click");
        assert!(matches!(&msg, Msg::OpenUrl(u) if u == ISSUE), "{msg:?}");
        let selected = state.selected_thread.clone();
        let fx = state.update(msg);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::OpenUrl(u) if u == ISSUE))
        );
        assert_eq!(state.selected_thread, selected, "the thread isn't opened");
        assert!(
            frame
                .hits
                .iter()
                .any(|h| matches!(&h.on_click, Some(Msg::OpenUrl(u)) if u == LOCAL_PR))
        );
    }
}
