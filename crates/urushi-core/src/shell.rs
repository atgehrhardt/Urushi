//! Writing command lines for a POSIX-style shell: quoting words, and rebuilding the
//! command a pane was running from its processes' arguments, so a restored pane can run
//! it again (ADR 0013).

/// Quotes `word` for POSIX shells, unless it needs no quoting.
pub fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '/' | '.' | '_' | '-' | ':' | '\\' | '=' | '@' | '%' | '+' | ','
                )
        });
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// The command line that runs a job again: each stage's arguments quoted, stages joined
/// by ` | `. `None` if there's nothing to run, or an argument holds a control character
/// (which could end the line early when typed into a shell).
///
/// A program that rewrote its title (`npm` shows as one argument, `npm run dev`) has a
/// single argument containing spaces, perhaps followed by empty ones; it's split back
/// into words.
pub fn job_command_line(stages: &[Vec<String>]) -> Option<String> {
    let mut parts = Vec::new();
    for stage in stages {
        let words: Vec<&str> = match stage.split_first() {
            Some((title, rest))
                if title.contains(char::is_whitespace) && rest.iter().all(String::is_empty) =>
            {
                title.split_whitespace().collect()
            }
            _ => stage.iter().map(String::as_str).collect(),
        };
        if words.is_empty() || words.iter().any(|w| w.chars().any(char::is_control)) {
            return None;
        }
        parts.push(words.into_iter().map(quote).collect::<Vec<_>>().join(" "));
    }
    (!parts.is_empty()).then(|| parts.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn words_are_quoted_only_when_needed() {
        assert_eq!(quote("/usr/bin/vim"), "/usr/bin/vim");
        assert_eq!(quote("--port=3000"), "--port=3000");
        assert_eq!(quote("my notes.md"), "'my notes.md'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("$(id)"), "'$(id)'");
    }

    #[test]
    fn jobs_are_rebuilt_from_their_arguments() {
        assert_eq!(
            job_command_line(&[stage(&["vim", "my notes.md"])]).as_deref(),
            Some("vim 'my notes.md'")
        );
        assert_eq!(
            job_command_line(&[stage(&["tail", "-f", "log"]), stage(&["grep", "a b"])]).as_deref(),
            Some("tail -f log | grep 'a b'")
        );
        assert_eq!(
            job_command_line(&[stage(&["npm run dev", "", ""])]).as_deref(),
            Some("npm run dev"),
            "a rewritten title is split back into words"
        );
        assert_eq!(job_command_line(&[]), None);
        assert_eq!(job_command_line(&[stage(&[])]), None);
        assert_eq!(job_command_line(&[stage(&["echo", "a\nrm -rf ~"])]), None);
    }
}
