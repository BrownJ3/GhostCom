# Security Policy

GhostCom is intended to become a production-worthy secure chat tool. Until the project has completed implementation, testing, dependency review, fuzzing, and independent security review, it must not be represented as audited or safe for high-risk use.

## Core Security Commitments

GhostCom must:

- Encrypt every network message.
- Authenticate peers before accepting chat messages.
- Provide forward secrecy through the selected secure transport.
- Avoid custom cryptographic protocol design.
- Avoid intentional persistence of messages or metadata.
- Keep logs disabled by default.
- Ensure logs never contain message text, peer secrets, private keys, or verification material.
- Bound all incoming frame sizes.
- Treat malformed input as hostile.
- Prefer safe Rust.
- Require review for any `unsafe` code.

## No-Persistence Rule

GhostCom must not intentionally write the following to disk:

- Chat messages.
- Message timestamps.
- Peer addresses.
- Peer fingerprints.
- Contact lists.
- Connection history.
- Usernames or aliases.
- Diagnostic logs containing session data.
- Crash reports containing sensitive process memory.

The default mode should create no application data directory and no config file.

## Important Limits

GhostCom can control its own behavior, but it cannot fully control the surrounding system. Users should understand that:

- Terminal scrollback may retain visible messages.
- Operating systems may swap process memory to disk.
- Shells may record commands used to launch the program.
- Remote peers can copy, photograph, or screen record messages.
- Network observers may still see IP addresses and connection timing.

These limits do not weaken the no-persistence rule inside GhostCom, but they matter for honest security claims.

## Identity Policy

The first release should use ephemeral identity by default. Long-term identity keys are allowed only if a future design explicitly introduces them and documents the privacy tradeoff.

If persistent identity is added later:

- It must be opt-in.
- Private keys must be stored with platform-appropriate protections.
- The stored metadata must be disclosed in user-facing documentation.
- The no-persistence claim must be revised to distinguish messages from identity material.

## Dependency Policy

Before release, CI should include:

- `cargo fmt`
- `cargo clippy`
- `cargo test`
- `cargo audit`
- `cargo deny`

Security-sensitive dependencies should be few, actively maintained, and pinned through `Cargo.lock` for releases.

## Rendezvous Security

The rendezvous server is not trusted for confidentiality or authenticity. It may help peers exchange temporary connection information, but endpoint-to-endpoint TLS verification remains mandatory.

The rendezvous server must:

- Keep rooms in RAM only.
- Use cryptographically random invite codes.
- Expire invite codes quickly.
- Allow each invite code to be joined only once.
- Cap setup message sizes.
- Avoid receiving chat plaintext.
- Avoid receiving private keys or session secrets.
- Avoid durable storage of invite state.
- Per-IP rate limits.
- Global active room limits.
- Global active connection limits.
- Operational metrics that avoid sensitive values.

Current rendezvous abuse limits:

- 512 active rendezvous rooms.
- 1024 active rendezvous WebSocket connections.
- 30 WebSocket upgrade attempts per IP per minute.
- 10 invite creation attempts per IP per five minutes.
- 60 invite join attempts per IP per minute.

The rendezvous server can still observe IP addresses and timing. GhostCom must not claim that rendezvous is metadata-free.

## Reporting Vulnerabilities

This project does not yet have a public vulnerability intake process. Before public release, add a private security contact and disclosure process here.
