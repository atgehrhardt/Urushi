//! [`CloudJira`]'s requests for a board's quick filters (ADR 0033).
//!
//! Reading uses the agile API (`GET /rest/agile/1.0/board/{id}/quickfilter`). Jira Cloud
//! documents no way to write quick filters, so writing uses the endpoint Jira's own
//! board settings page calls (`/rest/greenhopper/1.0/quickfilters`). It is
//! undocumented and may change or go away: every request it makes is in this file, and
//! a refusal reaches the service as an ordinary [`super::JiraError`], which keeps the
//! filter in Urushi instead.

use serde_json::{Value, json};
use urushi_core::board::JiraQuickFilter;

use super::{CloudJira, QuickFilterDraft, parse};
use crate::http::Method;
use crate::jira::{Account, JiraResult};

/// Most quick filters read for a board.
const QUICK_FILTERS: usize = 50;

/// Where Jira's board settings write a board's quick filters.
fn settings_path(board: u64) -> String {
    format!("/rest/greenhopper/1.0/quickfilters/{board}")
}

impl CloudJira {
    /// A board's quick filters, in Jira's order.
    pub(super) fn read_quick_filters(
        &self,
        account: &Account,
        board: u64,
    ) -> JiraResult<Vec<JiraQuickFilter>> {
        self.get(
            account,
            &format!("/rest/agile/1.0/board/{board}/quickfilter?maxResults={QUICK_FILTERS}"),
        )
        .map(|v| parse::quick_filters(&v))
    }

    /// Creates (`id` unset) or replaces a quick filter; a new one's id, if Jira's answer
    /// says it.
    pub(super) fn write_quick_filter(
        &self,
        account: &Account,
        board: u64,
        id: Option<u64>,
        filter: &QuickFilterDraft,
    ) -> JiraResult<Option<u64>> {
        let mut body = json!({
            "name": filter.name,
            "query": filter.jql,
            "description": "",
        });
        let answer = match id {
            None => self.call(account, Method::Post, &settings_path(board), Some(&body))?,
            Some(id) => {
                body["id"] = json!(id);
                let path = format!("{}/{id}", settings_path(board));
                self.call(account, Method::Put, &path, Some(&body))?
            }
        };
        Ok(new_id(&answer))
    }

    /// Deletes a quick filter.
    pub(super) fn remove_quick_filter(
        &self,
        account: &Account,
        board: u64,
        id: u64,
    ) -> JiraResult<()> {
        let path = format!("{}/{id}", settings_path(board));
        self.call(account, Method::Delete, &path, None).map(drop)
    }
}

/// The id in a write's answer: the filter itself (`{"id": 3}`, the id as a number or
/// a string), or none.
fn new_id(answer: &Value) -> Option<u64> {
    let id = &answer["id"];
    id.as_u64().or_else(|| id.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use serde_json::json;

    use super::super::tests::{Script, account};
    use super::*;
    use crate::jira::{JiraApi, JiraError};

    fn draft() -> QuickFilterDraft {
        QuickFilterDraft {
            name: "Bugs".into(),
            jql: "type = \"Bug\"".into(),
        }
    }

    fn body(request: &crate::http::Request) -> Value {
        serde_json::from_str(request.json.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn quick_filters_are_read_from_the_agile_api() {
        let http = Script::answering([(
            200,
            json!({ "values": [{ "id": 4, "name": "Mine", "jql": "assignee = currentUser()" }] }),
        )]);
        let list = CloudJira::new(http.clone())
            .quick_filters(&account(), 7)
            .unwrap();
        assert_eq!((list[0].id, list[0].name.as_str()), (4, "Mine"));
        let seen = http.seen();
        assert!(matches!(seen[0].method, Method::Get));
        assert!(
            seen[0]
                .url
                .ends_with("/rest/agile/1.0/board/7/quickfilter?maxResults=50")
        );
    }

    #[test]
    fn writes_go_to_the_board_settings_endpoint() {
        let http = Script::answering([
            (200, json!({ "id": 12, "name": "Bugs" })),
            (200, json!({ "id": 12 })),
            (204, Value::Null),
        ]);
        let jira = CloudJira::new(http.clone());
        assert_eq!(
            jira.create_quick_filter(&account(), 7, &draft()).unwrap(),
            Some(12)
        );
        jira.update_quick_filter(&account(), 7, 12, &draft())
            .unwrap();
        jira.delete_quick_filter(&account(), 7, 12).unwrap();
        let seen = http.seen();
        let base = "https://acme.atlassian.net/rest/greenhopper/1.0/quickfilters/7";
        assert!(matches!(seen[0].method, Method::Post));
        assert_eq!(seen[0].url, base);
        assert_eq!(
            body(&seen[0]),
            json!({ "name": "Bugs", "query": "type = \"Bug\"", "description": "" })
        );
        assert!(matches!(seen[1].method, Method::Put));
        assert_eq!(seen[1].url, format!("{base}/12"));
        assert_eq!(body(&seen[1])["id"], json!(12));
        assert!(matches!(seen[2].method, Method::Delete));
        assert_eq!(seen[2].url, format!("{base}/12"));
    }

    #[test]
    fn refusals_arrive_as_jira_errors() {
        let http = Script::answering([
            (
                403,
                json!({ "errorMessages": ["Only board admins can do this"] }),
            ),
            (404, Value::Null),
        ]);
        let jira = CloudJira::new(http);
        assert!(matches!(
            jira.create_quick_filter(&account(), 7, &draft()),
            Err(JiraError::Forbidden(m)) if m.contains("board admins")
        ));
        assert!(matches!(
            jira.create_quick_filter(&account(), 7, &draft()),
            Err(JiraError::NotFound(_))
        ));
    }

    #[test]
    fn the_new_id_is_read_as_a_number_or_a_string() {
        assert_eq!(new_id(&json!({"id": 3, "name": "Bugs"})), Some(3));
        assert_eq!(new_id(&json!({"id": "12"})), Some(12));
        assert_eq!(new_id(&Value::Null), None);
    }
}
