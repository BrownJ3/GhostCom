# Security Review: Rendezvous MVP

Date: 2026-05-25

This review covers the current GhostCom terminal client and the Fly-ready website/rendezvous service.

## Scope

Reviewed components:

- Terminal client direct chat flow.
- Terminal client rendezvous flow.
- Terminal client relay flow.
- Website `/rv` WebSocket rendezvous endpoint.
- Website `/relay` WebSocket relay endpoint.
- Application frame parsing.
- Session verification flow.
- No-persistence posture.

Not reviewed:

- Release packaging.
- Cross-platform installer behavior.
- OS-level memory, swap, crash dump, and terminal scrollback behavior.
- External cryptographic audit of dependencies.

## Current Security Posture

GhostCom currently uses:

- Ephemeral self-signed TLS identities generated per process.
- Mutual TLS certificate presentation.
- Manual shared session verification code.
- Encrypted application frames after TLS setup.
- Bounded chat and rendezvous message sizes.
- End-to-end Noise encryption for relay sessions.
- RAM-only rendezvous rooms.
- RAM-only relay rooms.
- One-time invite code joins.
- Five-minute invite code expiry.

The rendezvous service is treated as untrusted. It helps one peer learn a temporary direct connection candidate, but it does not participate in message encryption or peer authentication.

## Positive Findings

- Chat messages are not sent to the rendezvous server.
- Relay chat messages are protected by endpoint-to-endpoint Noise transport encryption.
- The rendezvous server does not receive private keys or TLS session secrets.
- The relay server does not receive Noise private keys or session secrets.
- The rendezvous server stores rooms in memory only.
- The relay server stores rooms in memory only.
- Invite codes are generated from the OS cryptographic RNG.
- Invite codes are one-time use.
- Invite codes expire after five minutes.
- Rendezvous setup messages are capped at 512 bytes.
- Relay setup messages and binary frames are capped.
- Rendezvous active rooms are capped at 512.
- Rendezvous active WebSocket connections are capped at 1024.
- Rendezvous WebSocket attempts are capped at 30 per IP per minute.
- Rendezvous invite creation attempts are capped at 10 per IP per five minutes.
- Rendezvous invite join attempts are capped at 60 per IP per minute.
- Chat frames are length bounded.
- Display names reject control characters.
- Both peers must confirm a shared verification code before chat.
- A malicious rendezvous server cannot silently MITM if users compare the session code out-of-band.

## Known Risks

### Direct Reachability

The rendezvous service does not solve NAT traversal by itself. It shares the caller address as observed by the server plus the caller's local listening port. This works when the caller is reachable, but it may fail behind NAT, firewalls, mobile networks, or restrictive Wi-Fi.

Security impact: low.

Availability impact: high.

### Metadata Exposure

The rendezvous server can observe:

- Caller IP address.
- Joiner IP address.
- Invite creation and join timing.
- Whether an invite was joined.

It must not be described as metadata-free. The accurate claim is that it stores no durable rendezvous state by design and never receives chat plaintext.

Security impact: medium for privacy.

### Manual Verification Dependency

MITM protection depends on users comparing the shared session verification code. If users skip or improperly compare the code, a malicious network or rendezvous server could attempt interception.

Security impact: high.

Mitigation: keep the prompt explicit and consider adding a shorter human-friendly comparison phrase later.

### Process-Local Rate Limiting

The rendezvous server now has process-local per-IP rate limits, active room caps, and active connection caps.

Security impact: improved.

Remaining availability risk: medium to high under distributed attacks.

Process-local limits help with brute force and accidental cost spikes from a small number of sources, but they do not replace upstream DDoS protection from the hosting provider, firewall, CDN, or load balancer.

### No Origin Policy Yet

The WebSocket endpoint does not currently validate `Origin`.

Security impact: low for the CLI client, medium if browser-based clients are added later.

Required before browser client support:

- Explicit allowed origins.
- CSRF-style review for browser-initiated WebSockets.

### Candidate Integrity

The rendezvous candidate is not cryptographically signed. This is acceptable because the subsequent TLS session verification code detects peer substitution, but users must verify the code.

Security impact: medium if users skip verification.

### Invite Code Brute Force

Invite codes are 16 alphanumeric characters generated from the OS RNG. The search space is large, but brute-force attempts should still be rate-limited before public deployment.

Security impact: low with rate limits, medium without rate limits.

### Terminal And OS Leakage

GhostCom cannot fully control terminal scrollback, shell history, operating system swap, hibernation, crash dumps, screenshots, or compromised endpoints.

Security impact: environment-dependent.

### Relay Metadata

Relay mode improves reachability but exposes relay metadata to the server and hosting provider:

- IP addresses.
- Session timing.
- Session duration.
- Encrypted byte counts.

The relay cannot decrypt chat contents when users complete and verify the Noise session, but it should not be described as metadata-free.

Security impact: medium for privacy.

## Required Hardening Before Public Production Claims

- Add upstream DDoS protection or documented hosting-level abuse controls.
- Add structured operational metrics that exclude invite codes and IPs where possible.
- Add dependency auditing with `cargo audit`.
- Add dependency policy checks with `cargo deny`.
- Add fuzzing for frame parsing and rendezvous JSON parsing.
- Add cross-platform CI.
- Add release build checksums.
- Add installer checksum verification.
- Add release provenance attestation.
- Add signed checksums or detached binary signatures before production-grade distribution claims.
- Commission external security review.

## Acceptable Current Claims

Accurate:

```text
GhostCom uses a short-lived rendezvous server to exchange direct connection information, then requires end-to-end session verification before chat.
```

Accurate:

```text
The rendezvous server does not receive chat plaintext and does not store chat history.
```

Inaccurate:

```text
The rendezvous server sees no metadata.
```

Inaccurate:

```text
GhostCom is audited or production secure.
```
