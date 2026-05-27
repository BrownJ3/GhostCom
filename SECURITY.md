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
- Escape terminal control characters before printing peer-controlled text.
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
- release checksum generation
- release provenance attestation where supported

Security-sensitive dependencies should be few, actively maintained, and pinned through `Cargo.lock` for releases.

## Distribution Security

Prebuilt binaries are part of the security boundary. A simple install flow must not bypass verification.

Release assets should be published through GitHub Releases with:

- Per-asset SHA-256 checksums.
- Detached signatures for the checksum manifest.
- GitHub artifact provenance attestations where supported.

Installer scripts must verify the checksum signature before trusting `SHA256SUMS`, then verify the selected archive checksum before copying binaries into a user path. The release signing private key must live only in GitHub Actions secrets or an equivalent secret manager; the public verification key is safe to publish in this repository.

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
- Global rate limits.
- Global active room limits.
- Global active connection limits.
- Optional server-side access token for private deployments.
- Emergency enable/disable switches for public WebSocket services.
- Operational metrics that avoid sensitive values.

Current rendezvous abuse limits:

- 64 active rendezvous rooms.
- 128 active rendezvous WebSocket connections.
- 20 WebSocket upgrade attempts per IP per minute.
- 6 invite creation attempts per IP per five minutes.
- 30 invite join attempts per IP per minute.
- 180 global WebSocket upgrade attempts per minute.
- 40 global invite creation attempts per five minutes.
- 180 global invite join attempts per minute.

On Fly.io, per-IP limits use the proxy-provided `Fly-Client-IP` header before
falling back to the socket peer address. The service intentionally ignores
`X-Forwarded-For` because it is commonly user-supplied unless the deployment
has explicit trusted-proxy enforcement.

The rendezvous server can still observe IP addresses and timing. GhostCom must not claim that rendezvous is metadata-free.

Current relay abuse limits:

- 64 active waiting relay rooms.
- 128 active relay WebSocket connections.
- 32 active paired relay sessions.
- 30 WebSocket setup attempts per IP per minute.
- 6 invite creation attempts per IP per five minutes.
- 30 invite join attempts per IP per minute.
- 240 global WebSocket setup attempts per minute.
- 40 global invite creation attempts per five minutes.
- 180 global invite join attempts per minute.
- 32 KiB maximum relayed binary frame size.
- 8 MiB maximum relayed bytes per direction.
- 15 minute idle timeout for paired relay forwarding.
- 60 minute maximum paired relay session lifetime.

Private or cost-sensitive deployments should set `GHSTCOM_RELAY_ACCESS_TOKEN`
as a deployment secret and distribute the same value to authorized clients
through a separate trusted channel. The value must not be committed to the
repository. Operators can set `GHSTCOM_RELAY_ENABLED=false` or
`GHSTCOM_RENDEZVOUS_ENABLED=false` to disable the costly WebSocket services
without changing the public landing page.

## Reporting Vulnerabilities

This project does not yet have a public vulnerability intake process. Before public release, add a private security contact and disclosure process here.
