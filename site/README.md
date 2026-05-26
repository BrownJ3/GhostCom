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

## Notes

- The service reads `PORT`, which Fly provides.
- The page is embedded in the Rust binary.
- Installer scripts are hosted in the GitHub repository, not on the Fly service.
- `/install.sh`, `/install.ps1`, and other unknown paths return `404 Not Found`.
- The rendezvous service uses in-memory rooms only.
- Invite codes expire after five minutes.
- Invite codes are one-time use.
- WebSocket setup messages are capped at 512 bytes.
- Active rendezvous rooms are capped at 512.
- Active rendezvous WebSocket connections are capped at 1024.
- Per-IP WebSocket upgrade attempts are capped at 30 per minute.
- Per-IP invite creation attempts are capped at 10 per five minutes.
- Per-IP invite join attempts are capped at 60 per minute.
- The server forwards direct connection candidates only; it does not relay chat messages.
- The relay endpoint forwards opaque Noise-encrypted binary frames only.
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

Relay mode is for peers that cannot directly reach each other. The relay is still untrusted; clients perform a Noise handshake through it and verify a shared session code before chat.

## Install Script Surface

The install script endpoints are intentionally plain files and are not linked from the public page. They download release assets from GitHub Releases and verify `SHA256SUMS` before installing `ghstprtcl`.

If releases are private, these scripts will not work for unauthenticated users. For easy installs by anyone, publish public release assets or serve authenticated downloads through a separate design.
