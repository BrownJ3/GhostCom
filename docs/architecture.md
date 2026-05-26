# Architecture

GhostCom is a small secure terminal application. The architecture should stay boring, explicit, and easy to audit.

## First Production Target

The first production-worthy target is:

```text
1-to-1 direct peer chat
terminal UI
mutually authenticated encrypted transport
ephemeral session state
no intentional persistence
```

## Proposed Rust Modules

```text
src/
  main.rs
  cli.rs
  rendezvous.rs
  app.rs
  transport/
    mod.rs
    tls.rs
  protocol/
    mod.rs
    frame.rs
    message.rs
  terminal/
    mod.rs
    line_ui.rs
  security/
    mod.rs
    fingerprint.rs
    memory.rs
```

## Transport

The initial transport should use TLS 1.3 through `rustls` and `tokio-rustls`.

Required properties:

- TLS 1.3 preferred.
- Mutual peer authentication.
- No plaintext fallback.
- No anonymous unauthenticated mode in production builds.
- Public verification material displayed to both peers.

Future versions may evaluate `libp2p` with Noise if NAT traversal, peer discovery, or richer peer-to-peer networking becomes necessary.

## Rendezvous

The optional rendezvous service runs with the website server at `/rv`.

Required properties:

- WebSocket setup only.
- In-memory rooms only.
- Short-lived invite codes.
- One-time joins.
- Strict setup message size limits.
- No message relay in the current implementation.
- No chat plaintext, chat keys, contact list, or durable identity storage.

The rendezvous server is treated as untrusted. It helps clients exchange a temporary direct connection candidate, then the existing mutually authenticated encrypted chat flow begins.

## Relay

The optional relay service runs with the website server at `/relay`.

Required properties:

- WebSocket setup followed by opaque binary forwarding.
- In-memory relay rooms only.
- Short-lived relay invite codes.
- Client-generated invite secrets that are never sent to the relay.
- One-time joins.
- Strict setup and binary frame size limits.
- No chat frame parsing on the server.
- No chat plaintext, Noise private key, Noise session key, contact list, or durable identity storage.

The relay server is treated as untrusted. Clients perform a Noise handshake through the relay, then authenticate the full invite's client-generated secret inside the encrypted channel before exchanging chat frames. Legacy room-only invites fall back to a shared verification code derived from the Noise handshake hash.

## Protocol

Application messages should be framed above the encrypted transport.

Frame requirements:

- Explicit protocol version.
- Explicit message type.
- Bounded payload length.
- UTF-8 validation for chat messages.
- Rejection of unknown critical message types.
- No timestamps in message frames for the first release.

Initial message types:

```text
hello
chat
close
```

## Runtime State

Runtime state may include:

- Current session keys managed by the transport library.
- Current shared session verification code.
- In-memory message buffer for terminal redraw.
- Local input buffer.
- Connection state.

Runtime state must not be serialized.

## Terminal UI

The first terminal UI may be line-based. A richer TUI can come later only if it does not compromise cross-platform behavior.

Required behavior:

- Do not write transcripts.
- Do not save drafts.
- Do not shell out with message content.
- Clear in-process buffers on shutdown where practical.
- Handle Ctrl+C and peer disconnect cleanly.

## Error Handling

Errors should be useful but not revealing.

Allowed:

```text
connection failed
peer verification failed
invalid frame
message too large
```

Avoid:

```text
raw peer certificate dumps
private key paths
message contents
full decrypted frames
```
