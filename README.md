# GhostCom

GhostCom is a terminal-first encrypted chat application for macOS, Windows, and Linux. Its core promise is narrow and deliberate:

> If two people have terminals and can reach each other over a network, they can open an encrypted chat session with no chat history, no account system, and no intentional metadata storage.

GhostCom is designed as an ephemeral secure communication tool, not a general messaging platform. Messages live in RAM for the active process lifetime and are destroyed when the session closes. The default cross-network path uses an untrusted relay; chat contents and display names are still encrypted end-to-end between peers.

## Product Goals

- Work quickly from a terminal on macOS, Windows, and Linux.
- Support simple 1-to-1 encrypted terminal chat.
- Support invite-only encrypted group chat through a trusted group host.
- Work across typical home networks through an untrusted relay.
- Keep direct peer-to-peer TCP mode available for LANs, VPNs, and reachable hosts.
- Encrypt all traffic between peers.
- Authenticate peers using explicit verification.
- Store no chat logs, transcripts, contact lists, peer history, telemetry, or diagnostics containing sensitive data.
- Keep the first version small enough to audit.

## Non-Goals

- No accounts.
- No cloud sync.
- No offline messages.
- No file transfer in the first release.
- No persistent identity by default.
- No custom cryptographic protocol.
- No claim that the relay hides network metadata such as IP address, timing, or traffic volume.

## Security Model

GhostCom should use proven cryptographic libraries and protocols rather than inventing its own. Direct connections use mutually authenticated TLS 1.3 through `rustls`. Relay connections use an end-to-end Noise session through `snow`; the relay forwards opaque binary frames and never receives chat plaintext or session keys.

See [SECURITY.md](SECURITY.md), [docs/threat-model.md](docs/threat-model.md), [docs/architecture.md](docs/architecture.md), and [docs/security-checklist.md](docs/security-checklist.md) before making protocol or storage changes.

## Quick Start

Install the latest alpha from GitHub Releases.

macOS Apple Silicon or Linux x64:

```text
curl -fsSL https://raw.githubusercontent.com/BrownJ3/GhostCom/master/install/install.sh | sh
```

Windows PowerShell 7 or newer:

```text
irm https://raw.githubusercontent.com/BrownJ3/GhostCom/master/install/install.ps1 | iex
```

Start GhostCom:

```text
ghstprtcl
```

On macOS or Linux, the installer places `ghstprtcl` in `~/.local/bin` and adds that directory to your shell profile if needed. Open a new terminal after installation, then `ghstprtcl` should work from any folder.

The menu defaults to relay mode, which is the easiest way to chat across different networks.

Start a relay chat:

```text
ghstprtcl relay-call
```

Share the invite code with the other person, then they run:

```text
ghstprtcl relay-join <invite-code>
```

For higher-risk use, run `ghstprtcl relay-join` without the invite on the
command line and paste the code at the prompt. That avoids writing the invite
into shell history.

Start an invite-only group chat:

```text
ghstprtcl relay-group
```

Share the group invite code with trusted participants. They join with the same join command:

```text
ghstprtcl relay-join <group-invite-code>
```

Group chat uses a conservative host-mediated design. Each participant establishes an independent end-to-end Noise session with the group host, and the host re-encrypts messages to the rest of the group. The relay still forwards only opaque encrypted frames, but the group host is a trusted participant and necessarily sees group messages. Group invites expire after 5 minutes, and the host must approve each participant with `/allow <id>` before they receive group traffic.

Group hosts can type `/who` to list admitted and pending participants, or `/close-invite` to stop new joins while keeping the current group connected.

If the group host leaves or closes the terminal session, the group chat ends for
all participants.

Private relays can require approved device keys instead of a shared relay password. Each client automatically creates a local Ed25519 device key at `~/.ghostcom/device.key` the first time it uses relay mode. If a private relay is configured with `GHSTCOM_RELAY_ALLOWED_DEVICE_KEYS`, unapproved clients receive a device fingerprint and suggested allowlist entry to send to the relay operator. The operator adds approved entries to `GHSTCOM_RELAY_ALLOWED_DEVICE_KEYS` and redeploys/restarts the relay.

Example relay allowlist:

```text
GHSTCOM_RELAY_ALLOWED_DEVICE_KEYS=publicKeyOne@1790640000,publicKeyTwo@1790640000
```

The number after `@` is the approval expiration time as a Unix timestamp. The relay suggests a 30-day approval entry when a new device asks for access. Bare public keys without `@expires_at` are accepted for compatibility but should be avoided for high-risk deployments.

The older `GHSTCOM_RELAY_ACCESS_TOKEN` still works as an optional deployment-wide gate, but expiring device approvals are the better fit for revoking one compromised device without rotating everyone.

Relay invite codes include a client-generated secret that is never sent to the relay. After the Noise handshake, both clients prove knowledge of that secret inside the encrypted channel and then start the chat. This makes entering the invite code the normal consent step.

Legacy room-only relay codes still fall back to showing a session verification code. In that case, compare the code out-of-band and type `YES` on both sides before chatting.

After invite authentication, each side can choose a display name for the session. Press Enter to use the generated name. Display names are ephemeral and are not saved.

Inside a chat session:

```text
/quit
```

closes the session.

## Development

Build the development binary:

```text
cargo build
```

Run the user-facing menu:

```text
cargo run
```

The release binary is named `ghstprtcl`. Running it with no arguments starts a small menu for starting or joining a relay chat.

For direct LAN/VPN testing, start a listener in one terminal:

```text
cargo run -- listen --bind 0.0.0.0:7777
```

Connect from another terminal or machine:

```text
cargo run -- connect <host>:7777
```

For the default cross-network relay flow during development:

```text
cargo run -- relay-call
cargo run -- relay-join <invite-code>
```

## CLI Shape

```text
ghstprtcl
ghstprtcl relay-call --relay wss://ghostcom-site.fly.dev/relay
ghstprtcl relay-group --relay wss://ghostcom-site.fly.dev/relay
ghstprtcl relay-join <invite-code> --relay wss://ghostcom-site.fly.dev/relay
ghstprtcl listen --bind 0.0.0.0:7777
ghstprtcl connect <host>:7777
```

Direct connection setup shows both peers a shared verification code. Users must compare that value out-of-band before trusting the direct session.

Relay setup uses a full invite in the form `room.secret`. The relay receives only the `room` value. The client-generated `secret` remains local to the two peers and is used after the Noise handshake to authenticate the session inside the encrypted channel. If a relay join uses an older room-only invite, GhostCom falls back to manual verification.

## Cross-Platform Target

GhostCom should build and run on:

- macOS
- Windows
- Linux

The terminal experience should avoid platform-specific assumptions where possible. Any platform-specific behavior must be documented and tested.

See [docs/cross-platform.md](docs/cross-platform.md) for platform and release expectations.

## Installer Plan

Release builds are published through GitHub Releases when a `v*` tag is pushed. The workflow builds standalone `ghstprtcl` binaries for Apple Silicon macOS, Windows x64, and Linux x64, publishes `SHA256SUMS`, signs it as `SHA256SUMS.sig`, and attempts GitHub artifact attestations where supported.

The alpha installers currently default to `v0.1.0-alpha.14` because prereleases are not always exposed through GitHub's `latest` release URL. To override the version later, set `GHSTPRTCL_VERSION`.

The scripts download release assets from GitHub, verify the detached signature on `SHA256SUMS`, and then verify the selected archive checksum before installing. The macOS/Linux installer requires `openssl` and adds the install directory to the user's shell profile when needed. The Windows installer requires PowerShell 7 or newer for signature verification and adds the install directory to the user's PATH. The Fly service does not host installer scripts; it is reserved for the relay runtime and minimal public site. Intel macOS is not included in the current alpha binary set.

## Development Status

This repository has an initial encrypted 1-to-1 terminal chat MVP:

- Direct TCP listener/client.
- Ephemeral in-memory TLS identity per run.
- Mutual TLS certificate presentation.
- Manual shared session verification code confirmation.
- Ephemeral display names, chosen or generated per session.
- End-to-end encrypted transient typing indicators are implemented but disabled by default until capability negotiation is added.
- Optional WebSocket relay using end-to-end Noise encryption.
- Invite-only relay group chat using per-participant Noise sessions through a trusted host.
- Advanced opt-in WebSocket rendezvous for private direct-connection experiments.
- Bounded message frames.
- No application logs, config file, contact book, or message persistence.

This is not audited and should not yet be treated as production-secure. The next step is hardening, packaging, cross-platform CI, and independent security review as described in [docs/roadmap.md](docs/roadmap.md).
