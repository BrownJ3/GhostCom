# GhostCom Site

This is a separate deployable Rust web service for the public GhostCom site and relay endpoint. The public-facing page is intentionally minimal: a black screen with green connection text.

## Run Locally

```text
cargo run
```

Then open:

```text
http://127.0.0.1:8080
```

Health check:

```text
http://127.0.0.1:8080/health
```

Relay endpoint:

```text
ws://127.0.0.1:8080/relay
```

## Deploy To Fly.io

Install and authenticate the Fly CLI, then from this `site` directory:

```text
fly launch
```

When Fly asks whether to use the existing configuration, accept it. You may need to choose a globally unique app name because Fly app names are shared across all users.

After launch:

```text
fly deploy
```

For private or cost-sensitive deployments, set an access token as a Fly secret:

```text
fly secrets set GHSTCOM_RELAY_ACCESS_TOKEN="replace-with-a-long-random-value"
```

Clients that use this private relay service must set the same value before
running `ghstprtcl`:

```text
export GHSTCOM_RELAY_ACCESS_TOKEN="replace-with-a-long-random-value"
```

Emergency switches can disable the costly WebSocket services while leaving the
landing page and health check online:

```text
fly secrets set GHSTCOM_RELAY_ENABLED=false
fly secrets set GHSTCOM_RENDEZVOUS_ENABLED=false
```

## Notes

- The service reads `PORT`, which Fly provides.
- `GHSTCOM_RELAY_ACCESS_TOKEN` is optional. When set, `/relay` and opt-in `/rv` require matching client setup messages before creating or joining rooms.
- `GHSTCOM_RELAY_ENABLED=false` disables `/relay`.
- `GHSTCOM_RENDEZVOUS_ENABLED=true` enables `/rv`; it is disabled by default because direct inbound peer connections are unreliable across NATs and firewalls.
- The page is embedded in the Rust binary.
- Installer scripts are hosted in the GitHub repository, not on the Fly service.
- `/install.sh`, `/install.ps1`, and other unknown paths return `404 Not Found`.
- Invite codes expire after five minutes.
- Invite codes are one-time use.
- WebSocket setup messages are capped at 512 bytes.
- The relay endpoint forwards opaque Noise-encrypted binary frames only.
- Active relay waiting rooms are capped at 64.
- Active relay WebSocket connections are capped at 128.
- Active relay sessions are capped at 32.
- Relayed bytes are capped at 8 MiB per direction per session.
- Paired relay sessions have a 15 minute idle timeout and 60 minute hard lifetime.
- No database or persistent storage is used.

## Client Relay Usage

```text
cargo run -- relay-call --relay wss://your-app.fly.dev/relay
cargo run -- relay-join <invite-code> --relay wss://your-app.fly.dev/relay
```

Relay mode is for peers that cannot directly reach each other. The relay is still untrusted; clients perform a Noise handshake through it and authenticate the client-generated secret from the full invite before chat. Legacy room-only invites fall back to manual shared session code verification.

The old `/rv` direct rendezvous endpoint is intentionally opt-in. It can be
enabled for private experiments with `GHSTCOM_RENDEZVOUS_ENABLED=true`, but it is
not the hosted product path because many computers cannot accept direct inbound
connections across home NAT, mobile networks, work Wi-Fi, and firewalls.

## Install Script Surface

The install script endpoints are intentionally plain files and are not linked from the public page. They download release assets from GitHub Releases and verify `SHA256SUMS` before installing `ghstprtcl`.

If releases are private, these scripts will not work for unauthenticated users. For easy installs by anyone, publish public release assets or serve authenticated downloads through a separate design.
