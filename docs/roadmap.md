# Roadmap

This roadmap prioritizes a small, secure, cross-platform command-line chat before convenience features.

## Phase 0: Design Rails

- Add project README.
- Add security policy.
- Add threat model.
- Add architecture notes.
- Add implementation roadmap.
- Add cross-platform release expectations.
- Add security checklist.

## Phase 1: Minimal Encrypted Chat

- Create Rust CLI project.
- Add `listen` and `connect` commands.
- Implement async TCP connection handling with `tokio`.
- Add TLS 1.3 with mutual authentication using `rustls`.
- Display shared session verification code.
- Require explicit user confirmation before chat starts.
- Send and receive bounded chat frames.
- Keep chat state in memory only.
- Handle Ctrl+C and peer disconnect.

## Phase 2: Hardening

- Add no-persistence integration test.
- Add malformed frame tests.
- Add frame size limit tests.
- Add connection failure tests.
- Add shutdown cleanup tests.
- Add hosting-level DDoS protection guidance.
- Add configurable rendezvous rate limits.
- Add configurable relay rate limits.
- Add dependency audit tooling.
- Add fuzzing for frame parsing.
- Add fuzzing for rendezvous JSON parsing.
- Add CI across macOS, Windows, and Linux.
- Add release binary smoke tests.

## Phase 3: Usability Without Persistence

- Improve terminal input behavior.
- Add clearer connection instructions.
- Add optional QR or copyable verification code display if it does not add persistence.
- Add packaging for common install paths.
- Add checksum-verified install scripts.
- Add signed release verification.

## Phase 4: Security Review Readiness

- Freeze protocol surface.
- Document all dependencies.
- Generate SBOM.
- Review all uses of randomness and key material.
- Review all logging and error paths.
- Commission external review before production security claims.

## Deferred Features

These are intentionally deferred because they expand the threat model:

- Persistent identities.
- Contact book.
- NAT traversal.
- Group chat.
- File transfer.
- Message history.
- Rich terminal UI.
