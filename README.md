# GhostCom

GhostCom is a terminal-first, peer-to-peer chat application for macOS, Windows, and Linux. Its core promise is narrow and deliberate:

> If two people have terminals and can reach each other over a network, they can open an encrypted chat session with no chat history, no account system, and no intentional metadata storage.

GhostCom is designed as an ephemeral secure communication tool, not a general messaging platform. Messages live in RAM for the active process lifetime and are destroyed when the session closes.

## Product Goals

- Work quickly from a terminal on macOS, Windows, and Linux.
- Support direct peer-to-peer 1-to-1 chat as the first production target.
- Encrypt all traffic between peers.
- Authenticate peers using explicit verification.
- Store no chat logs, transcripts, contact lists, peer history, telemetry, or diagnostics containing sensitive data.
- Keep the first version small enough to audit.

## Non-Goals

- No accounts.
- No cloud sync.
- No offline messages.
- No server-side message relay in the first release.
- No group chat in the first release.
- No file transfer in the first release.
- No persistent identity by default.
- No custom cryptographic protocol.

## Security Model

GhostCom should use proven cryptographic libraries and protocols rather than inventing its own. The first production-oriented MVP should use mutually authenticated TLS 1.3 through `rustls`, with short-lived session state and manual peer verification.

See [SECURITY.md](SECURITY.md), [docs/threat-model.md](docs/threat-model.md), [docs/architecture.md](docs/architecture.md), and [docs/security-checklist.md](docs/security-checklist.md) before making protocol or storage changes.

## Quick Start

Build the development binary:

```text
cargo build
```

Start a listener in one terminal:

```text
cargo run -- listen --bind 0.0.0.0:7777
```

Connect from another terminal or machine:

```text
cargo run -- connect <host>:7777
```

Or use the rendezvous flow with a deployed GhostCom site:

```text
cargo run -- call --rendezvous wss://your-site.fly.dev/rv
cargo run -- join <invite-code> --rendezvous wss://your-site.fly.dev/rv
```

For the most reliable cross-network path, use relay mode:

```text
cargo run -- relay-call --relay wss://ghostcom-site.fly.dev/relay
cargo run -- relay-join <invite-code> --relay wss://ghostcom-site.fly.dev/relay
```

Both sides will see the same session verification code. Compare that code out-of-band and type `YES` on both sides to begin chatting.

After verification, each side can choose a display name for the session. Press Enter to use the generated name. Display names are ephemeral and are not saved.

Inside a chat session:

```text
/quit
```

closes the session.

## CLI Shape

```text
ghostcom call --rendezvous wss://your-site.fly.dev/rv
ghostcom join <invite-code> --rendezvous wss://your-site.fly.dev/rv
ghostcom relay-call --relay wss://ghostcom-site.fly.dev/relay
ghostcom relay-join <invite-code> --relay wss://ghostcom-site.fly.dev/relay
ghostcom listen --bind 0.0.0.0:7777
ghostcom connect <host>:7777
```

During connection setup, both peers see a shared verification code. Users must compare that value out-of-band before trusting the session.

## Cross-Platform Target

GhostCom should build and run on:

- macOS
- Windows
- Linux

The terminal experience should avoid platform-specific assumptions where possible. Any platform-specific behavior must be documented and tested.

See [docs/cross-platform.md](docs/cross-platform.md) for platform and release expectations.

## Development Status

This repository has an initial encrypted 1-to-1 terminal chat MVP:

- Direct TCP listener/client.
- Ephemeral in-memory TLS identity per run.
- Mutual TLS certificate presentation.
- Manual shared session verification code confirmation.
- Ephemeral display names, chosen or generated per session.
- Optional WebSocket rendezvous for invite-code based direct connection setup.
- Optional WebSocket relay using end-to-end Noise encryption.
- Bounded message frames.
- No application logs, config file, contact book, or message persistence.

This is not audited and should not yet be treated as production-secure. The next step is hardening, packaging, cross-platform CI, and independent security review as described in [docs/roadmap.md](docs/roadmap.md).
