//! Reading edit metadata and field values shaped like Jira Cloud's.

use serde_json::json;

use super::*;

#[test]
fn edit_metadata_gives_each_field_its_kind_in_a_useful_order() {
    let meta = json!({
        "summary": { "name": "Summary", "schema": { "type": "string", "system": "summary" } },
        "customfield_1": { "name": "Story points", "schema": { "type": "number" } },
        "assignee": { "name": "Assignee", "schema": { "type": "user", "system": "assignee" } },
        "components": { "name": "Components", "schema": { "type": "array", "items": "component" },
            "allowedValues": [{ "id": "10", "name": "API" }] },
        "customfield_2": { "name": "Notes", "schema": { "type": "string", "custom": "com.atlassian.jira.plugin.system.customfieldtypes:textarea" } },
        "customfield_3": { "name": "Cascade", "schema": { "type": "option-with-child" } },
        "customfield_4": { "name": "Rank", "schema": { "type": "any" } },
        "parent": { "name": "Parent", "schema": { "type": "issuelink", "system": "parent" } },
        "duedate": { "name": "Due date", "required": true, "schema": { "type": "date" } },
        "customfield_5": { "name": "Owners", "schema": { "type": "array", "items": "user" } }
    });
    let f = fields(&meta);
    let shape: Vec<(&str, FieldKind)> = f.iter().map(|f| (f.name.as_str(), f.kind)).collect();
    assert_eq!(
        shape,
        [
            ("Assignee", FieldKind::User),
            ("Parent", FieldKind::Issue),
            ("Due date", FieldKind::Date),
            ("Components", FieldKind::Options),
            ("Cascade", FieldKind::Other),
            ("Notes", FieldKind::RichText),
            ("Owners", FieldKind::Users),
            ("Story points", FieldKind::Number),
        ]
    );
    assert!(f[2].required);
    assert_eq!(f[3].allowed[0].name, "API");
    // Create metadata lists fields by `fieldId`.
    let created = fields(
        &json!([{ "fieldId": "customfield_9", "name": "Team", "required": true,
        "schema": { "type": "option" }, "allowedValues": [{ "id": "t1", "value": "Payments" }] }]),
    );
    assert_eq!(created[0].allowed[0].name, "Payments");
}

#[test]
fn values_read_by_kind() {
    let v = value(
        FieldKind::User,
        &json!({ "accountId": "acc-1", "displayName": "Ada" }),
    );
    assert_eq!(
        (v.options[0].id.as_str(), v.options[0].name.as_str()),
        ("acc-1", "Ada")
    );
    assert_eq!(value(FieldKind::Number, &json!(3.0)).text, "3");
    assert_eq!(
        value(FieldKind::Labels, &json!(["a", "b"])).options.len(),
        2
    );
    let parent = value(
        FieldKind::Issue,
        &json!({ "key": "A-7", "fields": { "summary": "Epic" } }),
    );
    assert_eq!(
        (parent.text.as_str(), parent.options[0].name.as_str()),
        ("A-7", "Epic")
    );
    assert!(value(FieldKind::Option, &Value::Null).is_empty());
    assert_eq!(
        value(FieldKind::Other, &json!({ "value": "Parent", "id": "1" })).text,
        "Parent"
    );
}

#[test]
fn links_read_from_this_issues_side() {
    let l = links(&json!([
        { "id": "1", "type": { "inward": "is blocked by", "outward": "blocks" },
          "inwardIssue": { "id": "2", "key": "A-2", "fields": { "summary": "Two" } } },
        { "id": "3", "type": {}, "outwardIssue": { "key": "no-id" } }
    ]));
    assert_eq!(l.len(), 1);
    assert_eq!(
        (l[0].relation.as_str(), l[0].issue.key.as_str()),
        ("is blocked by", "A-2")
    );
}
