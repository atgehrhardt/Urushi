//! The daemon's Jira sign-in: a site, an Atlassian account's email and an API token
//! (ADR 0021).
//!
//! It is kept in `<home>/jira-credentials.json`, readable only by the user (0600, in the
//! 0700 home), and replaced atomically. It is never part of the workspace or the
//! settings, never logged ([`Account`]'s `Debug` hides the token) and never sent to a
//! client: they only learn the site, the email and the display name.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A Jira Cloud sign-in.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// The site's address, e.g. `https://acme.atlassian.net`.
    pub site: String,
    /// The Atlassian account's email.
    pub email: String,
    /// Its API token.
    pub token: String,
    /// Its name in Jira, as last read.
    #[serde(default)]
    pub display_name: String,
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account")
            .field("site", &self.site)
            .field("email", &self.email)
            .field("token", &"…")
            .finish_non_exhaustive()
    }
}

/// A site's address from what a user typed: `acme`, `acme.atlassian.net` or
/// `https://acme.atlassian.net/jira/…` all give `https://acme.atlassian.net`. Plain
/// `http` is refused: the API token would travel in the clear.
pub fn site_address(typed: &str) -> Result<String, String> {
    let typed = typed.trim();
    if typed.starts_with("http://") {
        return Err("the site must use https".into());
    }
    let host = typed.strip_prefix("https://").unwrap_or(typed);
    let host = host.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.to_ascii_lowercase();
    let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':');
    if host.is_empty() || !host.chars().all(valid) || host.starts_with(['.', '-']) {
        return Err(format!("{typed:?} isn't a Jira site's address"));
    }
    // A bare name is an Atlassian Cloud site; anything with a dot or a port is taken
    // as it is.
    let host = if host.contains(['.', ':']) {
        host
    } else {
        format!("{host}.atlassian.net")
    };
    Ok(format!("https://{host}"))
}

/// Where the sign-in is kept.
#[derive(Clone, Debug)]
pub struct CredentialStore {
    path: PathBuf,
}

impl CredentialStore {
    /// A store at `<home>/jira-credentials.json`.
    pub fn in_home(home: &std::path::Path) -> Self {
        Self {
            path: home.join("jira-credentials.json"),
        }
    }

    /// The saved sign-in, if there is a readable one.
    pub fn load(&self) -> Option<Account> {
        let bytes = std::fs::read(&self.path).ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(account) => Some(account),
            Err(e) => {
                tracing::warn!(error = %e, "ignoring an unreadable Jira sign-in");
                None
            }
        }
    }

    /// Saves the sign-in, readable only by this user.
    pub fn save(&self, account: &Account) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(account).map_err(std::io::Error::other)?;
        crate::platform::write_private(&self.path, &json)
    }

    /// Forgets the sign-in.
    pub fn delete(&self) -> std::io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_normalized_and_must_be_https() {
        for typed in [
            "acme",
            " ACME.atlassian.net ",
            "https://acme.atlassian.net/",
            "https://acme.atlassian.net/jira/software/projects/A/boards/1",
        ] {
            assert_eq!(site_address(typed).unwrap(), "https://acme.atlassian.net");
        }
        assert_eq!(
            site_address("jira.example.com").unwrap(),
            "https://jira.example.com"
        );
        assert!(site_address("http://acme.atlassian.net").is_err());
        assert!(site_address("").is_err());
        assert!(site_address("bad host").is_err());
    }

    #[test]
    fn the_sign_in_is_saved_privately_loaded_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::in_home(dir.path());
        assert_eq!(store.load(), None);
        let account = Account {
            site: "https://acme.atlassian.net".into(),
            email: "a@b.c".into(),
            token: "s3cret".into(),
            display_name: "Ada".into(),
        };
        store.save(&account).unwrap();
        assert_eq!(store.load(), Some(account.clone()));
        assert!(!format!("{account:?}").contains("s3cret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("jira-credentials.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.delete().unwrap();
        store.delete().unwrap();
        assert_eq!(store.load(), None);
    }
}
