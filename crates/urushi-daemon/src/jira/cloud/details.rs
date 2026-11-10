//! [`CloudJira`]'s calls about one issue in full: what the editor shows (several
//! requests: edit metadata, the values of those fields, comments, watchers, work log,
//! transitions, link kinds and the board's sprints), create metadata, user and issue
//! searches, comments, and attachments up and down.

use serde_json::{Value, json};
use urushi_core::board::Person;

use super::{CloudJira, segment};
use crate::http::{Method, Upload, encode_query};
use crate::jira::details::{Field, FieldKind, IssueDetails, IssueRef};
use crate::jira::{Account, JiraError, JiraResult, adf, parse};

/// Comments, worklogs and people read at most.
const MANY: usize = 100;

impl CloudJira {
    pub(super) fn read_details(
        &self,
        account: &Account,
        key: &str,
        board: u64,
    ) -> JiraResult<IssueDetails> {
        let issue = segment(key);
        let meta = self.get(account, &format!("/rest/api/3/issue/{issue}/editmeta"))?;
        let mut fields = parse::details::fields(&meta["fields"]);
        let ids: Vec<&str> = fields
            .iter()
            .map(|f| f.id.as_str())
            .chain(["attachment", "issuelinks", "timetracking"])
            .collect();
        let values = self.get(
            account,
            &format!(
                "/rest/api/3/issue/{issue}?fields={}",
                encode_query(&ids.join(","))
            ),
        )?;
        let v = &values["fields"];
        for field in &mut fields {
            field.value = parse::details::value(field.kind, &v[&field.id]);
        }
        // Sprints to choose from are the board's open and future ones.
        if let Some(sprint) = fields.iter_mut().find(|f| f.kind == FieldKind::Sprint) {
            sprint.allowed = self
                .get(
                    account,
                    &format!("/rest/agile/1.0/board/{board}/sprint?state=active,future"),
                )
                .map(|v| parse::details::sprints(&v))
                .unwrap_or_default();
        }
        let comments = self.get(
            account,
            &format!("/rest/api/3/issue/{issue}/comment?orderBy=created&maxResults={MANY}"),
        )?;
        let watchers = self.get(account, &format!("/rest/api/3/issue/{issue}/watchers"))?;
        let worklogs = self.get(
            account,
            &format!("/rest/api/3/issue/{issue}/worklog?maxResults={MANY}"),
        )?;
        let watching = watchers["isWatching"].as_bool() == Some(true);
        Ok(IssueDetails {
            fields,
            transitions: parse::transitions(
                &self.get(account, &format!("/rest/api/3/issue/{issue}/transitions"))?,
            ),
            comments: parse::details::comments(&comments),
            attachments: parse::details::attachments(&v["attachment"]),
            links: parse::details::links(&v["issuelinks"]),
            link_types: self
                .get(account, "/rest/api/3/issueLinkType")
                .map(|v| parse::details::link_types(&v))
                .unwrap_or_default(),
            watchers: parse::details::people(&watchers["watchers"]),
            watching,
            worklogs: parse::details::worklogs(&worklogs),
            time: parse::details::time_tracking(&v["timetracking"]),
        })
    }

    pub(super) fn read_create_fields(
        &self,
        account: &Account,
        project: &str,
        type_id: &str,
    ) -> JiraResult<Vec<Field>> {
        let meta = self.get(
            account,
            &format!(
                "/rest/api/3/issue/createmeta/{}/issuetypes/{}?maxResults=200",
                segment(project),
                segment(type_id)
            ),
        )?;
        // Newer answers list `fields`; some list `values`.
        let list = if meta["fields"].is_array() {
            &meta["fields"]
        } else {
            &meta["values"]
        };
        // The type is chosen already, and the project is the board's.
        Ok(parse::details::fields(list)
            .into_iter()
            .filter(|f| f.id != "issuetype" && f.id != "parent")
            .collect())
    }

    pub(super) fn find_users(
        &self,
        account: &Account,
        query: &str,
        key: Option<&str>,
    ) -> JiraResult<Vec<Person>> {
        let path = match key {
            Some(key) => format!(
                "/rest/api/3/user/assignable/search?issueKey={}&query={}&maxResults={MANY}",
                encode_query(key),
                encode_query(query)
            ),
            None => format!(
                "/rest/api/3/user/search?query={}&maxResults={MANY}",
                encode_query(query)
            ),
        };
        // Apps and deactivated accounts aren't people to pick.
        let found = self.get(account, &path)?;
        Ok(found
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|u| u["accountType"].as_str().is_none_or(|t| t == "atlassian"))
                    .filter(|u| u["active"].as_bool() != Some(false))
                    .filter_map(parse::person)
                    .collect()
            })
            .unwrap_or_default())
    }

    pub(super) fn find_issues(
        &self,
        account: &Account,
        project: &str,
        query: &str,
        limit: usize,
    ) -> JiraResult<Vec<IssueRef>> {
        let query = query.trim();
        let quoted = urushi_core::board::jql::escape(query);
        let mut jql = format!("project = \"{}\"", project.replace('"', ""));
        if !query.is_empty() {
            let looks_like_key = query.contains('-')
                && query
                    .rsplit('-')
                    .next()
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()));
            jql = if looks_like_key {
                format!("{jql} AND (key = \"{quoted}\" OR text ~ \"{quoted}*\")")
            } else {
                format!("{jql} AND text ~ \"{quoted}*\"")
            };
        }
        let path = format!(
            "/rest/api/3/search/jql?jql={}&fields=summary,status,issuetype&maxResults={limit}",
            encode_query(&format!("{jql} ORDER BY updated DESC"))
        );
        let found = self.get(account, &path)?;
        Ok(found["issues"]
            .as_array()
            .map(|a| a.iter().filter_map(parse::details::issue_ref).collect())
            .unwrap_or_default())
    }

    /// Adds (`id: None`), changes, or (`body: None`) deletes a comment.
    pub(super) fn comment(
        &self,
        account: &Account,
        key: &str,
        id: Option<&str>,
        body: Option<&str>,
    ) -> JiraResult<()> {
        let base = format!("/rest/api/3/issue/{}/comment", segment(key));
        let doc = |text: &str| {
            adf::from_markdown(text)
                .map(|body| json!({ "body": body }))
                .ok_or_else(|| JiraError::Rejected("a comment needs some text".into()))
        };
        match (id, body) {
            (None, Some(text)) => self.call(account, Method::Post, &base, Some(&doc(text)?)),
            (Some(id), Some(text)) => self.call(
                account,
                Method::Put,
                &format!("{base}/{}", segment(id)),
                Some(&doc(text)?),
            ),
            (Some(id), None) => self.call(
                account,
                Method::Delete,
                &format!("{base}/{}", segment(id)),
                None,
            ),
            (None, None) => Ok(Value::Null),
        }
        .map(drop)
    }

    pub(super) fn upload(
        &self,
        account: &Account,
        key: &str,
        name: &str,
        data: &[u8],
    ) -> JiraResult<()> {
        let url = format!(
            "{}/rest/api/3/issue/{}/attachments",
            account.site,
            segment(key)
        );
        let mut request = Self::request(account, Method::Post, &url);
        // Jira refuses attachments without this (a guard against cross-site forms).
        request
            .headers
            .push(("X-Atlassian-Token".into(), "no-check".into()));
        request.upload = Some(Upload {
            field: "file".into(),
            name: name.into(),
            data: data.to_vec(),
        });
        self.send(&request).map(drop)
    }

    /// An attachment's contents. Only addresses on the account's site are fetched with
    /// its credentials; Jira redirects to where the file is served.
    pub(super) fn fetch(&self, account: &Account, url: &str) -> JiraResult<Vec<u8>> {
        if !url.starts_with(&format!("{}/", account.site)) {
            return Err(JiraError::Rejected(
                "that file isn't on the Jira site".into(),
            ));
        }
        let mut request = Self::request(account, Method::Get, url);
        request.headers.retain(|(name, _)| name != "Accept");
        request.follow = true;
        self.send(&request)
    }
}
