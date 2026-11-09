//! Files attached to tasks: what the board records about each (name, type, size, and
//! an image's size in pixels). The bytes live in the daemon's attachment store
//! (ADR 0016); a task only names them. Removing a task removes its attachments' records
//! with it, and the daemon then deletes their files.

use serde::{Deserialize, Serialize};

use super::{Board, BoardError, Task};
use crate::{AttachmentId, TaskId, Timestamp};

/// Largest attachment, in bytes. Kept well under the protocol's frame limit, since an
/// attachment travels in one frame.
pub const MAX_ATTACHMENT_BYTES: usize = 6 * 1024 * 1024;
/// Most attachments one task can have.
pub const MAX_ATTACHMENTS: usize = 20;
/// Longest file name, in characters.
pub const MAX_FILE_NAME: usize = 255;

/// A file attached to a task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Identity (also names the file in the store).
    pub id: AttachmentId,
    /// The file's name, as it was attached (no directories).
    pub name: String,
    /// Its media type, from its contents (e.g. `image/png`).
    pub media_type: String,
    /// Size in bytes.
    pub size: u64,
    /// When it was attached.
    pub created_at: Timestamp,
    /// For an image: its width and height in pixels.
    #[serde(default)]
    pub image: Option<(u32, u32)>,
}

/// A file name to keep: the last path component, trimmed, without control characters.
/// Blank and over-long names are refused.
pub fn file_name(name: &str) -> Result<String, BoardError> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let clean: String = last.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.is_empty() || clean == "." || clean == ".." {
        return Err(BoardError::EmptyFileName);
    }
    if clean.chars().count() > MAX_FILE_NAME {
        return Err(BoardError::TooLong {
            what: "a file name",
            max: MAX_FILE_NAME,
        });
    }
    Ok(clean.to_owned())
}

/// The media type of a file, from its first bytes: the image types the daemon can
/// decode, PDF, plain text (UTF-8 without NULs), or `application/octet-stream`.
pub fn media_type(bytes: &[u8]) -> &'static str {
    const SIGNATURES: [(&[u8], &str); 6] = [
        (b"\x89PNG\r\n\x1a\n", "image/png"),
        (b"\xff\xd8\xff", "image/jpeg"),
        (b"GIF87a", "image/gif"),
        (b"GIF89a", "image/gif"),
        (b"%PDF-", "application/pdf"),
        (b"RIFF", "riff"),
    ];
    for (signature, kind) in SIGNATURES {
        if bytes.starts_with(signature) {
            if kind == "riff" {
                return if bytes.get(8..12) == Some(b"WEBP") {
                    "image/webp"
                } else {
                    "application/octet-stream"
                };
            }
            return kind;
        }
    }
    let head = &bytes[..bytes.len().min(4096)];
    let text = !head.contains(&0)
        && match std::str::from_utf8(head) {
            Ok(_) => true,
            // A multi-byte character cut at the end of the sample is still text.
            Err(e) => e.error_len().is_none(),
        };
    if text {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

/// Whether a media type is one of the images the daemon makes thumbnails of.
pub fn is_image(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

impl Task {
    /// Every attachment of this task and the tasks under it.
    pub fn all_attachments(&self) -> Vec<&Attachment> {
        self.subtree()
            .into_iter()
            .flat_map(|t| &t.attachments)
            .collect()
    }
}

impl Board {
    /// Adds an attachment to a task (at any depth).
    pub fn add_attachment(
        &mut self,
        task: TaskId,
        attachment: Attachment,
    ) -> Result<(), BoardError> {
        let task = self.task_mut(task)?;
        if task.attachments.len() >= MAX_ATTACHMENTS {
            return Err(BoardError::TooManyAttachments);
        }
        task.attachments.push(attachment);
        Ok(())
    }

    /// The attachment with this id, and the task it belongs to.
    pub fn attachment(&self, id: AttachmentId) -> Option<(&Task, &Attachment)> {
        self.all_tasks()
            .find_map(|t| t.attachments.iter().find(|a| a.id == id).map(|a| (t, a)))
    }

    /// Removes an attachment's record and returns it.
    pub fn remove_attachment(&mut self, id: AttachmentId) -> Result<Attachment, BoardError> {
        let task = self
            .attachment(id)
            .map(|(t, _)| t.id)
            .ok_or(BoardError::NoSuchAttachment(id))?;
        let list = &mut self.task_mut(task)?.attachments;
        let at = list
            .iter()
            .position(|a| a.id == id)
            .ok_or(BoardError::NoSuchAttachment(id))?;
        Ok(list.remove(at))
    }

    /// Every attachment on the board, at any depth.
    pub fn all_attachments(&self) -> impl Iterator<Item = &Attachment> {
        self.all_tasks().flat_map(|t| &t.attachments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoardId, ColumnId};

    fn attachment(n: u128, name: &str) -> Attachment {
        Attachment {
            id: AttachmentId::from_u128(n),
            name: name.into(),
            media_type: "text/plain".into(),
            size: 3,
            created_at: Timestamp::from_unix_millis(1),
            image: None,
        }
    }

    #[test]
    fn file_names_keep_only_the_last_component() {
        assert_eq!(file_name("/tmp/a/b.png").unwrap(), "b.png");
        assert_eq!(file_name(r"C:\Users\me\shot 1.jpg").unwrap(), "shot 1.jpg");
        assert_eq!(file_name(" notes\u{7}.txt ").unwrap(), "notes.txt");
        assert_eq!(file_name("dir/"), Err(BoardError::EmptyFileName));
        assert_eq!(file_name(".."), Err(BoardError::EmptyFileName));
        assert!(matches!(
            file_name(&"x".repeat(300)),
            Err(BoardError::TooLong { .. })
        ));
    }

    #[test]
    fn media_types_come_from_the_contents() {
        assert_eq!(media_type(b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(media_type(b"\xff\xd8\xff\xe0"), "image/jpeg");
        assert_eq!(media_type(b"GIF89a.."), "image/gif");
        assert_eq!(media_type(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(media_type(b"RIFF\0\0\0\0WAVE"), "application/octet-stream");
        assert_eq!(media_type(b"%PDF-1.7"), "application/pdf");
        assert_eq!(media_type("héllo".as_bytes()), "text/plain");
        assert_eq!(media_type(b"a\0b"), "application/octet-stream");
        assert!(is_image("image/png") && !is_image("text/plain"));
    }

    #[test]
    fn attachments_are_added_found_bounded_and_removed() {
        let mut b = Board::new(
            BoardId::from_u128(1),
            "b",
            Timestamp::from_unix_millis(0),
            [10, 11, 12].map(ColumnId::from_u128),
        )
        .unwrap();
        let card = Task::new(
            TaskId::from_u128(1),
            "card",
            "",
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        b.add_task(ColumnId::from_u128(10), card).unwrap();
        let step = Task::new(
            TaskId::from_u128(2),
            "step",
            "",
            Timestamp::from_unix_millis(0),
        )
        .unwrap();
        b.add_subtask(TaskId::from_u128(1), step).unwrap();
        b.add_attachment(TaskId::from_u128(2), attachment(5, "a.txt"))
            .unwrap();
        assert_eq!(
            b.attachment(AttachmentId::from_u128(5)).map(|(t, _)| t.id),
            Some(TaskId::from_u128(2))
        );
        assert_eq!(
            b.task(TaskId::from_u128(1))
                .unwrap()
                .all_attachments()
                .len(),
            1
        );
        for n in 1..MAX_ATTACHMENTS {
            b.add_attachment(TaskId::from_u128(2), attachment(100 + n as u128, "x"))
                .unwrap();
        }
        let before = b.clone();
        assert_eq!(
            b.add_attachment(TaskId::from_u128(2), attachment(999, "x")),
            Err(BoardError::TooManyAttachments)
        );
        assert_eq!(b, before);
        assert_eq!(
            b.remove_attachment(AttachmentId::from_u128(5))
                .unwrap()
                .name,
            "a.txt"
        );
        assert_eq!(
            b.remove_attachment(AttachmentId::from_u128(5)),
            Err(BoardError::NoSuchAttachment(AttachmentId::from_u128(5)))
        );
        assert_eq!(b.all_attachments().count(), MAX_ATTACHMENTS - 1);
    }
}
