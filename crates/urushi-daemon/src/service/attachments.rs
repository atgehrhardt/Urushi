//! Files attached to board tasks (ADR 0016): adding one (checked, decoded if it is an
//! image, stored, then recorded on the task), fetching it or its thumbnail, and
//! deleting it. The store's file IO and image decoding run off the async runtime.
//!
//! The file is written before the task records it, so a recorded attachment always
//! has its bytes; if recording fails the file is removed again. When tasks or boards
//! are deleted their files are removed after the change is saved, and anything left
//! over (a crash in between) is swept when the daemon starts.

use std::collections::HashSet;

use urushi_core::board::{self, Attachment};
use urushi_core::{AttachmentId, Board, BoardError, Task, TaskId};

use super::{Service, ServiceError, ServiceResult, blocking, lock, new_uuid, now};
use crate::attachments::{self, Thumbnail};

/// An attachment's bytes, as `GetAttachment` answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentData {
    /// The file itself, with its media type.
    File {
        /// Media type.
        media_type: String,
        /// Contents.
        bytes: Vec<u8>,
    },
    /// An image's thumbnail as raw RGBA.
    Thumbnail {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// `width × height × 4` bytes.
        rgba: Vec<u8>,
    },
}

impl Service {
    /// Attaches a file to a task (at any depth).
    pub async fn add_attachment(
        &self,
        task: TaskId,
        name: &str,
        bytes: Vec<u8>,
    ) -> ServiceResult<Attachment> {
        if bytes.len() > board::MAX_ATTACHMENT_BYTES {
            return Err(ServiceError::Invalid(format!(
                "an attachment can be at most {} MiB",
                board::MAX_ATTACHMENT_BYTES / (1024 * 1024)
            )));
        }
        let name = board::file_name(name)?;
        let media_type = board::media_type(&bytes);
        let id = AttachmentId::from_uuid(new_uuid());
        let store = self.attachments.clone();
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let image = board::is_image(media_type);
        let dimensions = blocking(move || {
            let decoded = if image {
                Some(
                    attachments::decode(&bytes)
                        .map_err(|e| ServiceError::Invalid(e.to_string()))?,
                )
            } else {
                None
            };
            store
                .write(id, &bytes, decoded.as_ref().map(|d| &d.thumbnail))
                .map_err(|e| ServiceError::Internal(format!("saving the attachment: {e}")))?;
            Ok(decoded.map(|d| d.size))
        })
        .await?;
        let attachment = Attachment {
            id,
            name,
            media_type: media_type.to_owned(),
            size,
            created_at: now(),
            image: dimensions,
        };
        let recorded = self.mutate(|state| {
            state
                .board_of_task(task)?
                .add_attachment(task, attachment.clone())?;
            Ok(attachment)
        });
        if recorded.is_err() {
            self.attachments.remove(id);
        }
        recorded
    }

    /// An attachment's file, or its thumbnail if it is an image.
    pub async fn get_attachment(
        &self,
        id: AttachmentId,
        thumbnail: bool,
    ) -> ServiceResult<AttachmentData> {
        let record = {
            let state = lock(&self.state);
            state
                .boards
                .iter()
                .find_map(|b| b.attachment(id).map(|(_, a)| a.clone()))
                .ok_or(BoardError::NoSuchAttachment(id))?
        };
        if thumbnail && record.image.is_none() {
            return Err(ServiceError::Precondition(format!(
                "{} is not an image",
                record.name
            )));
        }
        let store = self.attachments.clone();
        blocking(move || {
            let missing = |e: std::io::Error| {
                ServiceError::Internal(format!("reading attachment {}: {e}", record.name))
            };
            Ok(if thumbnail {
                let Thumbnail {
                    width,
                    height,
                    rgba,
                } = store.thumbnail(id).map_err(missing)?;
                AttachmentData::Thumbnail {
                    width,
                    height,
                    rgba,
                }
            } else {
                AttachmentData::File {
                    media_type: record.media_type,
                    bytes: store.read(id).map_err(missing)?,
                }
            })
        })
        .await
    }

    /// Deletes an attachment and its file.
    pub fn delete_attachment(&self, id: AttachmentId) -> ServiceResult<()> {
        self.mutate(|state| {
            let board = state
                .boards
                .iter_mut()
                .find(|b| b.attachment(id).is_some())
                .ok_or(BoardError::NoSuchAttachment(id))?;
            board.remove_attachment(id)?;
            Ok(())
        })?;
        self.attachments.remove(id);
        Ok(())
    }

    /// Removes the files of attachments that went with deleted tasks or boards.
    pub(super) fn forget_attachments<'a>(&self, gone: impl IntoIterator<Item = &'a Attachment>) {
        for a in gone {
            self.attachments.remove(a.id);
        }
    }

    /// Deletes files that no task names (left by a crash between deleting a task and
    /// its files). Called once at start.
    pub(super) fn sweep_attachments(&self) {
        let keep: HashSet<AttachmentId> = lock(&self.state)
            .boards
            .iter()
            .flat_map(Board::all_attachments)
            .map(|a| a.id)
            .collect();
        self.attachments.retain(&keep);
    }
}

/// Every attachment of a removed task and its subtasks.
pub(super) fn of_task(task: &Task) -> Vec<Attachment> {
    task.all_attachments().into_iter().cloned().collect()
}
