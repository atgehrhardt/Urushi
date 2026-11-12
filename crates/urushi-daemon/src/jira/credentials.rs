//! The daemon's Jira sign-ins: each a site, an Atlassian account's email and an API
//! token (ADR 0021), several at once (ADR 0051), each named by its [`Account::key`].
//!
//! They are kept in `<home>/jira-credentials.json`, readable only by the user (0600, in
//! the 0700 home), and replaced atomically. The file holds `{"accounts": [...]}`; one
//! written before ADR 0051 holds a single account, and is read as a list of one. They
//! are never part of the workspace or the settings, never logged ([`Account`]'s `Debug`
//! hides the token) and never sent to a client: they only learn the site, the email
//! and the display name.

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
    /// Its Atlassian account id, as last read.
    #[serde(default)]
    pub account_id: String,
}

impl Account {
    /// What names this sign-in: its site's host and its email in lower case, e.g.
    /// `acme.atlassian.net/ada@acme.com`. Known before signing in, so signing in again
    /// with the same site and email replaces the sign-in rather than adding one.
    pub fn key(&self) -> String {
        key_of(&self.site, &self.email)
    }
}

/// The key of the sign-in for `site` (an address, as [`site_address`] gives) and
/// `email`.
pub fn key_of(site: &str, email: &str) -> String {
    format!(
        "{}/{}",
        site.trim_start_matches("https://"),
        email.trim().to_lowercase()
    )
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

    /// The saved sign-ins, in the order they were made (none if there is no readable
    /// file).
    pub fn load(&self) -> Vec<Account> {
        let Ok(bytes) = std::fs::read(&self.path) else {
            return Vec::new();
        };
        match serde_json::from_slice(&bytes) {
            Ok(Stored::Many { accounts }) => accounts,
            Ok(Stored::One(account)) => vec![account],
            Err(e) => {
                tracing::warn!(error = %e, "ignoring unreadable Jira sign-ins");
                Vec::new()
            }
        }
    }

    /// Saves the sign-ins, readable only by this user; none forgets them all.
    pub fn save(&self, accounts: &[Account]) -> std::io::Result<()> {
        if accounts.is_empty() {
            return self.delete();
        }
        let stored = StoredRef { accounts };
        let json = serde_json::to_vec_pretty(&stored).map_err(std::io::Error::other)?;
        crate::platform::write_private(&self.path, &json)
    }

    /// Forgets every sign-in.
    fn delete(&self) -> std::io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// The file as written: a list of sign-ins, or one sign-in from before ADR 0051.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Many { accounts: Vec<Account> },
    One(Account),
}

/// What [`CredentialStore::save`] writes.
#[derive(Serialize)]
struct StoredRef<'a> {
    accounts: &'a [Account],
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

    fn account(site: &str, email: &str, token: &str) -> Account {
        Account {
            site: site.into(),
            email: email.into(),
            token: token.into(),
            display_name: "Ada".into(),
            account_id: "acc-ada".into(),
        }
    }

    #[test]
    fn sign_ins_are_saved_privately_loaded_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::in_home(dir.path());
        assert!(store.load().is_empty());
        let accounts = vec![
            account("https://acme.atlassian.net", "a@b.c", "s3cret"),
            account("https://globex.atlassian.net", "a@b.c", "0ther"),
        ];
        store.save(&accounts).unwrap();
        assert_eq!(store.load(), accounts);
        for a in &accounts {
            assert!(!format!("{a:?}").contains(&a.token));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("jira-credentials.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.save(&[]).unwrap();
        store.save(&[]).unwrap();
        assert!(store.load().is_empty());
        assert!(!dir.path().join("jira-credentials.json").exists());
    }

    #[test]
    fn a_sign_in_saved_before_there_could_be_several_is_read_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::in_home(dir.path());
        let old = account("https://acme.atlassian.net", "a@b.c", "s3cret");
        std::fs::write(
            dir.path().join("jira-credentials.json"),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        assert_eq!(store.load(), vec![old]);
        std::fs::write(dir.path().join("jira-credentials.json"), b"{not json").unwrap();
        assert!(store.load().is_empty());
    }

    #[test]
    fn a_sign_in_is_named_by_its_host_and_email() {
        let a = account("https://acme.atlassian.net", " Ada@Acme.COM ", "t");
        assert_eq!(a.key(), "acme.atlassian.net/ada@acme.com");
        assert_eq!(
            key_of("https://acme.atlassian.net", "ada@acme.com"),
            a.key()
        );
    }
}
