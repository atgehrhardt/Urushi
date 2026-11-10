//! Writing values into JQL, Jira's query language, so text a person typed (a search, a
//! label, a type's name) can't end a string early or change what the query asks.

/// `value` as a JQL string literal: in double quotes, with `\` and `"` escaped.
pub fn string(value: &str) -> String {
    format!("\"{}\"", escape(value))
}

/// `value` with `\` and `"` escaped, to go inside a JQL string literal.
pub fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `field in (a, b)` for several values, each quoted; `None` without any.
pub fn any_of<S: AsRef<str>>(field: &str, values: &[S]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let list = values
        .iter()
        .map(|v| string(v.as_ref()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("{field} in ({list})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_quoted_and_cant_break_out() {
        assert_eq!(string("ui"), "\"ui\"");
        assert_eq!(string(r#"a "b" \ c"#), r#""a \"b\" \\ c""#);
        assert_eq!(escape(r#"x"y"#), r#"x\"y"#);
    }

    #[test]
    fn lists_are_quoted_and_empty_lists_are_nothing() {
        assert_eq!(
            any_of("labels", &["ui", "back end"]).as_deref(),
            Some(r#"labels in ("ui", "back end")"#)
        );
        assert_eq!(any_of::<&str>("labels", &[]), None);
    }
}
