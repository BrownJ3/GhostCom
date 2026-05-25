# Security Checklist

This checklist must be reviewed before any release that claims to be secure or production-worthy.

## Cryptography

- Uses a proven protocol implementation.
- Does not implement custom encryption, key exchange, or authentication.
- Uses authenticated encryption through the transport.
- Provides forward secrecy.
- Authenticates both peers.
- Displays verification material for out-of-band comparison.
- Rejects unverified peers before chat begins.

## Storage

- Does not write messages to disk.
- Does not write peer addresses to disk.
- Does not write fingerprints to disk.
- Does not write connection timestamps to disk.
- Does not create logs by default.
- Does not create a config file by default.
- Does not create a cache directory by default.

## Logging And Errors

- Logs are off by default.
- Logs never include message contents.
- Logs never include private keys or session secrets.
- Errors are actionable but do not dump sensitive protocol state.
- Panic paths do not intentionally print decrypted frames.

## Protocol Robustness

- Incoming frames have strict size limits.
- Invalid UTF-8 is rejected for chat messages.
- Unknown critical frame types are rejected.
- Malformed frames cannot panic the process.
- Peer disconnects are handled cleanly.
- Ctrl+C triggers orderly shutdown.

## Rendezvous

- Rendezvous rooms are kept in RAM only.
- Invite codes are generated from an OS cryptographic RNG.
- Invite codes are short-lived and one-time use.
- Rendezvous setup messages have strict size limits.
- Rendezvous does not receive chat messages.
- Rendezvous does not receive session private keys.
- Rendezvous does not replace end-to-end peer verification.
- Public deployment has per-IP rate limits for WebSocket upgrades, invite creation, and invite joins.
- Public deployment has active room and connection caps.
- Public deployment has upstream network-level DDoS protection or a plan for abusive traffic beyond process-local limits.
- Operational metrics exclude invite codes and chat data.

## Relay

- Relay rooms are kept in RAM only.
- Relay invite codes are generated from an OS cryptographic RNG.
- Relay invite codes are short-lived and one-time use.
- Relay setup messages have strict size limits.
- Relay binary frames have strict size limits.
- Relay forwards opaque encrypted bytes only.
- Relay does not parse chat frames.
- Relay does not receive Noise private keys or session keys.
- Relay does not replace end-to-end session verification.

## Rust Safety

- No `unsafe` code, or every `unsafe` block has focused review.
- `cargo fmt` passes.
- `cargo clippy` passes with project-selected deny rules.
- `cargo test` passes.
- Dependency audit passes.
- Dependency license/policy review passes.

## Testing

- Unit tests cover frame parsing.
- Integration tests cover local peer chat.
- Integration tests cover no-persistence behavior.
- Fuzz tests cover frame parsing.
- Cross-platform CI covers macOS, Windows, and Linux.

## Release

- Release build is reproducible enough for project needs.
- Binaries are checksummed.
- Release notes describe security-relevant changes.
- Known limitations are documented.
- The project does not claim external audit unless one has happened.
