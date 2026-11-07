# 0003. Versioned protobuf over length-prefixed frames

- **Status:** Accepted
- **Date:** 2026-11-06

## Context
The client/daemon protocol must work across versions (an old app talking to a new daemon), across machines (Tailscale), and possibly with clients written in other languages later.

## Decision
- Messages are defined in protobuf under `proto/urushi/v1/` and generated with `prost` + `protox` (pure Rust, no `protoc` needed to build).
- Framing is a `u32` big-endian length followed by an `Envelope`, with a frame size cap. A `Hello` exchange negotiates the version.
- `buf lint` and `buf breaking` run in CI.

## Alternatives considered
- **serde + bincode/postcard:** no schema, weak rules for evolving messages, and Rust-only.
- **JSON:** verbose for screen updates, and no schema enforcement.
- **gRPC:** pulls in HTTP/2 and a large dependency tree, and its streaming model doesn't fit our ack-based flow control.

## Consequences
- Schema changes are additive only (`change-protocol` skill). Breaking changes require `v2/`.
