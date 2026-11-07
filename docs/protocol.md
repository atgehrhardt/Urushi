# Wire protocol

> Status: **v1.0 implemented**. The schema lives in [`proto/urushi/v1/`](../proto/urushi/v1/), the codec and conversions in `crates/urushi-proto`.
> Any change must follow the `change-protocol` skill.

## Transport

| Context | Transport | Authorization |
|---|---|---|
| Local, macOS/Linux | Unix domain socket in the runtime dir, mode 0600 | Peer's OS user id must match the daemon's user |
| Local, Windows | Named pipe | DACL limited to the current user |
| Remote | TCP bound **only** to Tailscale IPs | Tailscale `whois` → allowlist of tailnet logins |

The protocol never assumes client and daemon are on the same machine. No file paths are exchanged for content, and there is no shared memory.

## Framing

Each frame is `u32` big-endian length, then a protobuf-encoded `Envelope`. Frames have a maximum size (initially 8 MiB). Anything larger closes the connection.

```
Envelope {
  oneof kind {
    Request      { id, oneof body }    // client → daemon, always answered
    Response     { id, oneof outcome } // daemon → client, matches a Request id
    Event        { oneof body }        // daemon → client, unsolicited
    Notification { oneof body }        // client → daemon, fire-and-forget (input, acks, scroll)
  }
}
```

## Handshake

The client sends `Hello { protocol_major, protocol_minor, client_name, client_version }`.
The daemon replies with `Welcome { protocol_major, protocol_minor, daemon_version, host_name }`, or with an error and closes the connection.
Within one major version, minor versions are additive. A different major version is rejected.

## Message groups (v1)

- **Workspace:** `ListWorkspace{subscribe}`, `AddProject`, `RemoveProject`, `ListBranches`, `CreateThread`, `RenameThread`, `PlanThreadDeletion` → `DeletionPlan{token}`, `DeleteThread{token}`, `CreateTab`, `CloseTab`, `RenameTab`, `SplitPane`, `ClosePane`, `FocusPane`, `MoveDivider`, `ResizePaneEdge`, `Equalize`. The daemon sends `WorkspaceChanged` events to subscribers.
- **Terminal:**
  - `AttachPane{pane, cols, rows}` returns a full `ScreenUpdate`. After that the daemon streams `ScreenUpdate{seq}` events, and the client must send the `AckScreen{seq}` notification before the next one is sent.
  - `DetachPane`, `SetPaneSize`, the `Input{key|text|paste}` and `ScrollPane` notifications, and `ReadScreenText` (plain text, for the CLI and tests).
  - Events: `PaneExited`, `PaneBell`.
- **Settings:** `GetSettings`, `UpdateSettings` (global and per project).
- **Remote:** `TailnetStatus`, `SetTailnetServe`.

## Screen encoding

`ScreenUpdate` carries:

- only the lines that changed, each as styled spans `(text, Style{fg, bg, flags})`. Trailing default blanks are trimmed. With `STYLE_FLAG_WIDE`, every character in the span is two columns wide.
- colours as `Color{indexed | rgb}`; unset means the theme default. The client's theme resolves the palette.
- cursor position and shape
- terminal modes that matter to the client (e.g. mouse reporting)
- the title set by the application, plus scrollback position (`display_offset`, `history_size`)

A full snapshot uses the same shape with every row included.
