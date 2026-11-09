//! The attachment store (ADR 0016): the bytes of files attached to board tasks, one
//! file per attachment in `<home>/attachments/<id>`, and each image's thumbnail next to
//! it in `<id>.thumb`. The board records what is attached; this module only keeps
//! bytes, and is told what to delete.
//!
//! Images are decoded here, never in clients: a thumbnail is scaled to fit
//! [`THUMBNAIL_SIZE`] and kept as raw RGBA (an 8-byte header of width and height, then
//! the pixels), so a client can draw it without an image decoder. Decoding is bounded
//! by [`image::Limits`], since the bytes come from a client.

use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use image::{ImageReader, Limits};
use urushi_core::AttachmentId;

/// The longest side of a thumbnail, in pixels.
pub(crate) const THUMBNAIL_SIZE: u32 = 320;
/// The largest image (each side) the daemon will decode.
const MAX_IMAGE_SIDE: u32 = 16_384;
/// The most memory one decode may use.
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

/// A thumbnail: straight RGBA rows, `width × height × 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Thumbnail {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels.
    pub rgba: Vec<u8>,
}

/// What a decoded image gives: its size, and its thumbnail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decoded {
    /// The image's width and height in pixels.
    pub size: (u32, u32),
    /// Its thumbnail.
    pub thumbnail: Thumbnail,
}

/// Why an image couldn't be decoded.
#[derive(Debug, thiserror::Error)]
#[error("not an image that can be shown: {0}")]
pub(crate) struct DecodeError(String);

/// Decodes an image (PNG, JPEG, GIF or WebP) and scales it to a thumbnail.
pub(crate) fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| DecodeError(e.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| DecodeError(e.to_string()))?;
    let size = (image.width(), image.height());
    let small = if size.0 > THUMBNAIL_SIZE || size.1 > THUMBNAIL_SIZE {
        image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE)
    } else {
        image
    };
    let rgba = small.to_rgba8();
    Ok(Decoded {
        size,
        thumbnail: Thumbnail {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        },
    })
}

/// Attached files on disk.
#[derive(Clone, Debug)]
pub(crate) struct AttachmentStore {
    dir: PathBuf,
}

impl AttachmentStore {
    /// A store in `dir` (created on first write).
    pub(crate) fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, id: AttachmentId) -> PathBuf {
        self.dir.join(id.to_string())
    }

    fn thumbnail_path(&self, id: AttachmentId) -> PathBuf {
        self.dir.join(format!("{id}.thumb"))
    }

    /// Stores a file, and its thumbnail if it is an image. Either both are written or
    /// neither is left behind.
    pub(crate) fn write(
        &self,
        id: AttachmentId,
        bytes: &[u8],
        thumbnail: Option<&Thumbnail>,
    ) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let result = write_atomically(&self.path(id), bytes).and_then(|()| match thumbnail {
            Some(t) => write_atomically(&self.thumbnail_path(id), &encode_thumbnail(t)),
            None => Ok(()),
        });
        if result.is_err() {
            self.remove(id);
        }
        result
    }

    /// A stored file's bytes.
    pub(crate) fn read(&self, id: AttachmentId) -> std::io::Result<Vec<u8>> {
        fs::read(self.path(id))
    }

    /// A stored image's thumbnail, made again from the file if its cached copy is
    /// missing or damaged.
    pub(crate) fn thumbnail(&self, id: AttachmentId) -> std::io::Result<Thumbnail> {
        if let Some(t) = fs::read(self.thumbnail_path(id))
            .ok()
            .and_then(|b| decode_thumbnail(&b))
        {
            return Ok(t);
        }
        let decoded = decode(&self.read(id)?).map_err(std::io::Error::other)?;
        let _ = write_atomically(
            &self.thumbnail_path(id),
            &encode_thumbnail(&decoded.thumbnail),
        );
        Ok(decoded.thumbnail)
    }

    /// Deletes a file and its thumbnail (missing ones are fine).
    pub(crate) fn remove(&self, id: AttachmentId) {
        let _ = fs::remove_file(self.path(id));
        let _ = fs::remove_file(self.thumbnail_path(id));
    }

    /// Deletes every file not in `keep`, and temp files left by an interrupted write:
    /// attachments of tasks deleted while the daemon was down, or whose deletion failed.
    pub(crate) fn retain(&self, keep: &HashSet<AttachmentId>) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let id = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.trim_end_matches(".thumb"))
                .and_then(|n| n.parse::<AttachmentId>().ok());
            if id.is_none_or(|id| !keep.contains(&id)) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Replaces the file at `path`: write a temp file next to it, fsync, rename.
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

fn encode_thumbnail(t: &Thumbnail) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + t.rgba.len());
    out.extend_from_slice(&t.width.to_le_bytes());
    out.extend_from_slice(&t.height.to_le_bytes());
    out.extend_from_slice(&t.rgba);
    out
}

fn decode_thumbnail(bytes: &[u8]) -> Option<Thumbnail> {
    let width = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
    let height = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let rgba = bytes.get(8..)?.to_vec();
    let expected = usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
    (width > 0 && height > 0 && rgba.len() == expected).then_some(Thumbnail {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    /// A `width × height` PNG of one color.
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn images_decode_to_thumbnails_that_fit() {
        let d = decode(&png(1000, 500)).unwrap();
        assert_eq!(d.size, (1000, 500));
        assert_eq!((d.thumbnail.width, d.thumbnail.height), (320, 160));
        assert_eq!(d.thumbnail.rgba.len(), 320 * 160 * 4);
        // Small images keep their size.
        let d = decode(&png(10, 20)).unwrap();
        assert_eq!((d.thumbnail.width, d.thumbnail.height), (10, 20));
        assert!(decode(b"\x89PNG\r\n\x1a\nnot really").is_err());
        assert!(decode(b"plain text").is_err());
    }

    #[test]
    fn files_and_thumbnails_are_stored_read_and_swept() {
        let dir = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(dir.path().join("attachments"));
        let (a, b) = (AttachmentId::from_u128(1), AttachmentId::from_u128(2));
        let image = png(400, 400);
        let decoded = decode(&image).unwrap();
        store.write(a, &image, Some(&decoded.thumbnail)).unwrap();
        store.write(b, b"hello", None).unwrap();
        assert_eq!(store.read(a).unwrap(), image);
        assert_eq!(store.thumbnail(a).unwrap(), decoded.thumbnail);
        // A damaged cached thumbnail is made again.
        fs::write(store.thumbnail_path(a), b"junk").unwrap();
        assert_eq!(store.thumbnail(a).unwrap(), decoded.thumbnail);
        assert!(store.thumbnail(b).is_err(), "text has no thumbnail");
        // Sweeping keeps only what's listed, and drops temp files.
        fs::write(dir.path().join("attachments/x.tmp"), "half").unwrap();
        store.retain(&HashSet::from([a]));
        let mut left: Vec<String> = fs::read_dir(dir.path().join("attachments"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [a.to_string(), format!("{a}.thumb")]);
        store.remove(a);
        assert!(store.read(a).is_err());
    }
}
