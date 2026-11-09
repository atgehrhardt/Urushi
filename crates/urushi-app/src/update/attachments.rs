//! Attachments in the app: choosing or dropping files onto the task editor (the runner
//! reads them; `update` only sends them), opening one with the system's app, deleting
//! one, and fetching the thumbnails of images that come into view (ADR 0016).

use std::sync::Arc;

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_ui::ImageData;

use super::request;
use crate::board::{MAX_THUMBNAILS, wanted_thumbnails};
use crate::msg::{BoardMsg, Effect, Followup};
use crate::state::{AppState, Dialog};

impl AppState {
    /// Handles the attachment messages (see `card_action`).
    pub(super) fn attachment_action(&mut self, msg: BoardMsg, fx: &mut Vec<Effect>) {
        match msg {
            BoardMsg::AddFiles(task) => fx.push(Effect::PickFiles { task }),
            BoardMsg::Attach { task, name, bytes } => {
                let body = Body::AddAttachment(v1::AddAttachmentRequest {
                    task_id: task,
                    name,
                    data: bytes,
                });
                fx.push(request(body, Followup::None));
            }
            BoardMsg::AttachFailed(reason) => self.error = Some(reason),
            BoardMsg::OpenAttachment(id) => {
                if let Some((_, a)) = self.workspace.attachment(&id) {
                    let followup = Followup::OpenAttachment {
                        id: id.clone(),
                        name: a.name.clone(),
                    };
                    let body = Body::GetAttachment(v1::GetAttachmentRequest {
                        attachment_id: id,
                        thumbnail: false,
                    });
                    fx.push(request(body, followup));
                }
            }
            BoardMsg::DeleteAttachment(attachment_id) => {
                let body = Body::DeleteAttachment(v1::DeleteAttachmentRequest { attachment_id });
                fx.push(request(body, Followup::None));
            }
            _ => {}
        }
    }

    /// A file dropped while the editor of an existing task is open is attached to it.
    /// Returns whether it was.
    pub(super) fn drop_onto_task(
        &mut self,
        path: &str,
        is_dir: bool,
        fx: &mut Vec<Effect>,
    ) -> bool {
        let Some(Dialog::Task {
            task: Some(task), ..
        }) = &self.dialog
        else {
            return false;
        };
        if !is_dir && self.daemon_supports_board_extras() {
            fx.push(Effect::ReadFiles {
                task: task.clone(),
                paths: vec![path.to_owned()],
            });
        }
        true
    }

    /// Asks for the thumbnails now on screen that haven't been asked for, and forgets
    /// ones out of view once there are too many.
    pub(super) fn want_thumbnails(&mut self, fx: &mut Vec<Effect>) {
        if !self.board.visible() || !self.daemon_supports_board_extras() {
            return;
        }
        let editing = match &self.dialog {
            Some(Dialog::Task { task, .. }) => task.as_deref(),
            _ => None,
        };
        let wanted = wanted_thumbnails(&self.workspace, self.board.board(&self.workspace), editing);
        let thumbnails = &mut self.board.thumbnails;
        for id in &wanted {
            if thumbnails.requested.insert(id.clone()) {
                let body = Body::GetAttachment(v1::GetAttachmentRequest {
                    attachment_id: id.clone(),
                    thumbnail: true,
                });
                fx.push(request(body, Followup::Thumbnail(id.clone())));
            }
        }
        if thumbnails.images.len() > MAX_THUMBNAILS {
            thumbnails.images.retain(|id, _| wanted.contains(id));
            let kept = &thumbnails.images;
            thumbnails
                .requested
                .retain(|id| kept.contains_key(id) || wanted.contains(id));
        }
    }

    /// Handles the answers to attachment requests: a thumbnail to keep, or a file to
    /// open. Anything else is handed back.
    pub(super) fn attachment_answered(
        &mut self,
        followup: Followup,
        outcome: Outcome,
        fx: &mut Vec<Effect>,
    ) -> Option<(Followup, Outcome)> {
        match (followup, outcome) {
            (Followup::Thumbnail(id), Outcome::AttachmentData(data)) => {
                if let Some(image) =
                    ImageData::new(image_id(&id), data.width, data.height, data.data)
                {
                    self.board.thumbnails.images.insert(id, Arc::new(image));
                }
                None
            }
            (Followup::OpenAttachment { id, name }, Outcome::AttachmentData(data)) => {
                fx.push(Effect::OpenFile {
                    id,
                    name,
                    bytes: data.data,
                });
                None
            }
            other => Some(other),
        }
    }
}

/// A stable bitmap id for an attachment (the renderer caches bitmaps by id). The high
/// bit keeps it clear of the app's own small ids (the logo is 1).
fn image_id(attachment: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in attachment.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash | (1 << 63)
}
