# GhostCom Site

This is a separate deployable Rust web service for the public GhostCom site and rendezvous endpoint. The public-facing page is intentionally minimal: a black screen with green connection text.

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

Rendezvous endpoint:

```text
ws://127.0.0.1:8080/rv
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

Clients that use this private relay/rendezvous service must set the same value
before running `ghstprtcl`:

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
- `GHSTCOM_RELAY_ACCESS_TOKEN` is optional. When set, `/relay` and `/rv` require matching client setup messages before creating or joining rooms.
- `GHSTCOM_RELAY_ENABLED=false` disables `/relay`.
- `GHSTCOM_RENDEZVOUS_ENABLED=false` disables `/rv`.
- The page is embedded in the Rust binary.
- Installer scripts are hosted in the GitHub repository, not on the Fly service.
- `/install.sh`, `/install.ps1`, and other unknown paths return `404 Not Found`.
- The rendezvous service uses in-memory rooms only.
- Invite codes expire after five minutes.
- Invite codes are one-time use.
- WebSocket setup messages are capped at 512 bytes.
- Active rendezvous rooms are capped at 64.
- Active rendezvous WebSocket connections are capped at 128.
- Per-IP WebSocket upgrade attempts are capped at 20 per minute.
- Per-IP invite creation attempts are capped at 6 per five minutes.
- Per-IP invite join attempts are capped at 30 per minute.
- Global rendezvous setup attempts are capped in-process.
- The server forwards direct connection candidates only; it does not relay chat messages.
- The relay endpoint forwards opaque Noise-encrypted binary frames only.
- Active relay waiting rooms are capped at 64.
- Active relay WebSocket connections are capped at 128.
- Active relay sessions are capped at 32.
- Relayed bytes are capped at 8 MiB per direction per session.
- Paired relay sessions have a 15 minute idle timeout and 60 minute hard lifetime.
- No database or persistent storage is used.

## Client Rendezvous Usage

After deployment, use the Fly URL from the client:

```text
cargo run -- call --rendezvous wss://your-app.fly.dev/rv
cargo run -- join <invite-code> --rendezvous wss://your-app.fly.dev/rv
```

The rendezvous server is not trusted for message security. The chat still requires the end-to-end session verification code before messages can be exchanged.

Relay mode:

```text
cargo run -- relay-call --relay wss://your-app.fly.dev/relay
cargo run -- relay-join <invite-code> --relay wss://your-app.fly.dev/relay
```

Relay mode is for peers that cannot directly reach each other. The relay is still untrusted; clients perform a Noise handshake through it and authenticate the client-generated secret from the full invite before chat. Legacy room-only invites fall back to manual shared session code verification.

## Install Script Surface

The install script endpoints are intentionally plain files and are not linked from the public page. They download release assets from GitHub Releases and verify `SHA256SUMS` before installing `ghstprtcl`.

If releases are private, these scripts will not work for unauthenticated users. For easy installs by anyone, publish public release assets or serve authenticated downloads through a separate design.
