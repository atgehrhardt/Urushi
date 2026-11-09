//! Attachments: core [`Attachment`] ⇄ wire `v1::Attachment` (since 1.8). An image's
//! size travels as width and height, 0 for other files.

use urushi_core::board::Attachment;

use super::super::{ConvertError, millis_from_proto, millis_to_proto, parse_id};
use crate::v1;

/// Core attachment → wire (also the answer to `AddAttachment`).
pub fn attachment_to_proto(a: &Attachment) -> v1::Attachment {
    let (width, height) = a.image.unwrap_or((0, 0));
    v1::Attachment {
        id: a.id.to_string(),
        name: a.name.clone(),
        media_type: a.media_type.clone(),
        size: a.size,
        created_at_ms: millis_to_proto(a.created_at),
        width,
        height,
    }
}

pub(super) fn attachment_from_proto(a: &v1::Attachment) -> Result<Attachment, ConvertError> {
    Ok(Attachment {
        id: parse_id(&a.id, "board.task.attachment.id")?,
        name: a.name.clone(),
        media_type: a.media_type.clone(),
        size: a.size,
        created_at: millis_from_proto(a.created_at_ms),
        image: (a.width > 0 && a.height > 0).then_some((a.width, a.height)),
    })
}
