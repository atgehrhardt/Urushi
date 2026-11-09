# 0016. Keep attachments in the daemon, send their bytes over the protocol, and decode images there

- **Status:** Accepted
- **Date:** 2026-11-09

## Context
Users want to attach images and other files to board tasks and see images on cards.
Boards live in the daemon (ADR 0002), and clients may be on other machines, including
phones over Tailscale (ADR 0005), so a client's file path means nothing to the daemon or
to other clients. The workspace document (ADR 0008) is rewritten whole on every change,
so it can't hold file contents. Clients draw with an in-house renderer that takes raw
RGBA bitmaps and has no image decoder.

## Decision
- The **daemon stores attachments**: one file per attachment in
  `<home>/attachments/<id>`. A task records only the metadata (name, media type, size,
  an image's pixel size).
- **Bytes travel in protocol messages**: `AddAttachment` carries the file and
  `GetAttachment` returns it, each in one frame. An attachment is at most **6 MiB**, which
  keeps a frame under the 8 MiB limit with room to spare, and a task has at most 20.
- **The daemon decodes images** (PNG, JPEG, GIF, WebP, with the `image` crate under
  decode limits) when they are attached, refuses ones it can't decode, and keeps a
  **thumbnail as raw RGBA** that fits 320 pixels (`<id>.thumb`). Clients ask for
  thumbnails and draw them as they are.
- The media type comes from the file's contents, not its name or the client.
- Files go when their attachment, task or board is deleted, after the change is saved;
  files nothing names are swept when the daemon starts.

## Alternatives considered
- **Chunked streaming** (start/append/finish requests). Allows bigger files, but needs
  upload state, cleanup of abandoned uploads, and flow control on both sides. Not worth
  it for screenshots and notes; it can be added later as new messages.
- **Raising the frame limit.** Every connection would accept huge frames, including
  remote ones, for the sake of one rare request.
- **Decoding in clients.** Every client (desktop, iOS, Android) would carry an image
  decoder, and phones would decode full-size photos to draw a 30-pixel thumbnail.
- **Storing paths instead of copies.** Breaks for remote clients and when the file
  moves.
- **Files in the JSON document (base64).** Every board change would rewrite every file.

## Consequences
- Attaching works the same from any client, and thumbnails cost clients nothing but
  drawing.
- The daemon has a new directory to manage and one more dependency (`image`, daemon
  only).
- Files over 6 MiB can't be attached.
- Opening an attachment downloads it to the client's temp directory and hands it to the
  system's app (desktop only for now).
