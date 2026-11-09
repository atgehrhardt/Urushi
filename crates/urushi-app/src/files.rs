//! Local files for attachments, used by the runner (never by `update`, which does no
//! IO): reading a chosen or dropped file to send to the daemon, and saving a fetched
//! attachment to a temporary file so the system's app can open it.

use std::path::{Path, PathBuf};

use urushi_core::board::{MAX_ATTACHMENT_BYTES, file_name};

/// A file's name and contents, if it can be attached; otherwise why not, for the user.
pub(crate) fn read_attachment(path: &str) -> Result<(String, Vec<u8>), String> {
    let name = file_name(path).map_err(|e| e.to_string())?;
    let meta = std::fs::metadata(path).map_err(|e| format!("Can't read {name}: {e}"))?;
    if meta.is_dir() {
        return Err(format!("{name} is a folder; attach files one by one."));
    }
    if meta.len() > MAX_ATTACHMENT_BYTES as u64 {
        return Err(format!(
            "{name} is too big to attach (the limit is {} MiB).",
            MAX_ATTACHMENT_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Can't read {name}: {e}"))?;
    Ok((name, bytes))
}

/// Writes an attachment to `<temp>/urushi-attachments/<id>/<name>` and returns the
/// path. Files of the same name stay apart by their attachment's id.
pub(crate) fn save_for_opening(id: &str, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    save_in(&std::env::temp_dir(), id, name, bytes)
}

fn save_in(temp: &Path, id: &str, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    // Both come from the daemon; keep them to one path component each anyway.
    let id = file_name(id).map_err(std::io::Error::other)?;
    let name = file_name(name).map_err(std::io::Error::other)?;
    let dir = temp.join("urushi-attachments").join(id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    #[test]
    fn files_are_read_for_attaching_unless_too_big_or_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "hi").unwrap();
        let (name, bytes) = read_attachment(file.to_str().unwrap()).unwrap();
        assert_eq!(
            (name.as_str(), bytes.as_slice()),
            ("notes.txt", b"hi".as_slice())
        );
        let big = dir.path().join("big.bin");
        std::fs::write(&big, vec![0; MAX_ATTACHMENT_BYTES + 1]).unwrap();
        assert!(
            read_attachment(big.to_str().unwrap())
                .unwrap_err()
                .contains("too big")
        );
        let sub = dir.path().join("folder");
        std::fs::create_dir(&sub).unwrap();
        assert!(read_attachment(sub.to_str().unwrap()).is_err());
        assert!(read_attachment(dir.path().join("missing").to_str().unwrap()).is_err());
    }

    #[test]
    fn attachments_open_from_their_own_folder_with_a_safe_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_in(dir.path(), "abc", "../../evil.txt", b"x").unwrap();
        assert_eq!(path, dir.path().join("urushi-attachments/abc/evil.txt"));
        assert_eq!(std::fs::read(path).unwrap(), b"x");
    }
}
