# Wire protocol

> Status: **design**. The schema will be added in `proto/urushi/v1/` during milestone M3.
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
    Request  { id, oneof body }   // client → daemon
    Response { id, oneof result } // daemon → client, matches a Request id
    Event    { oneof body }       // daemon → client, unsolicited
  }
}
```

## Handshake

The client sends `Hello { protocol_major, protocol_minor, client_name, client_version }`.
The daemon replies with `Welcome { protocol_major, protocol_minor, daemon_version, host_id }`, or with an error and closes the connection.
Within one major version, minor versions are additive. A different major version is rejected.

## Message groups (v1)

- **Workspace:** `ListWorkspace`, `AddProject`, `RemoveProject`, `CreateThread`, `RenameThread`, `PlanThreadDeletion` → `DeletionPlan{token}`, `DeleteThread{token}`, `CreateTab`, `CloseTab`, `SplitPane`, `ClosePane`, `MoveDivider`, `FocusPane`, `Equalize`. The daemon sends `WorkspaceChanged` events to subscribers.
- **Terminal:**
  - `AttachPane{pane, cols, rows}` returns a full `ScreenSnapshot`. After that the daemon streams `ScreenUpdate{seq}` events, and the client must `Ack{seq}` each one before the next is sent.
  - `DetachPane`, `ResizePane`, `Input{key|text|paste|mouse}`, `Scrollback{range}`.
- **Settings:** `GetSettings`, `UpdateSettings` (global and per project).
- **Remote:** `TailnetStatus`, `SetTailnetServe`.

## Screen encoding

`ScreenUpdate` carries:

- only the rows that changed, each as styled spans `(text, style_id)`
- any new entries for the style table
- cursor position and shape
- terminal modes that matter to the client (e.g. mouse reporting)
- title and working directory

A full snapshot uses the same shape with every row included.
