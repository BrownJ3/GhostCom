# Relay Security Design

The relay exists to make GhostCom work when peers cannot directly reach each other because of NAT, firewalls, or mobile networks.

## Trust Model

The relay is untrusted.

The relay may observe:

- Caller IP address.
- Joiner IP address.
- Invite creation and join timing.
- Session duration.
- Encrypted byte counts.

The relay must not observe:

- Chat plaintext.
- Display names before endpoint encryption.
- Noise private keys.
- Noise session keys.
- Decrypted application frames.

## Encryption Model

Relay sessions use the Noise Protocol Framework through the `snow` crate. Peers perform a Noise handshake through the relay. The relay forwards opaque WebSocket binary messages only.

The server never participates in the Noise handshake. It does not terminate, inspect, or authenticate the encrypted chat session.

Both peers display a shared verification code derived from the Noise handshake hash. Users must compare this code out-of-band before sending messages.

## Server Behavior

The relay server must:

- Keep relay rooms in RAM only.
- Use cryptographically random invite codes.
- Allow each invite code to be joined only once.
- Expire waiting rooms quickly.
- Cap active relay rooms.
- Cap active relay sessions.
- Cap active relay WebSocket connections.
- Cap WebSocket setup message sizes.
- Cap relayed binary frame sizes.
- Close idle paired sessions.
- Drop both peers on malformed relay traffic.
- Avoid logging relay payloads.
- Avoid persistent storage.

## Current Abuse Limits

- 256 active waiting rooms.
- 1024 active relay WebSocket connections.
- 256 active paired relay sessions.
- 60 WebSocket setup attempts per IP per minute.
- 10 invite creation attempts per IP per five minutes.
- 60 invite join attempts per IP per minute.
- 32 KiB maximum binary frame size.
- 64 MiB maximum relayed bytes per direction.
- 15 minute idle timeout for paired relay forwarding.

## Destruction Semantics

On disconnect or error, the relay drops:

- The room entry.
- Both WebSocket tasks.
- In-memory forwarding counters.

The relay intentionally stores no chat history and no relay payloads. However, GhostCom cannot guarantee that cloud provider infrastructure, kernel buffers, VM snapshots, crash dumps, packet captures, or compromised endpoints have no recoverable traces.

Accurate claim:

```text
The relay intentionally stores no messages and cannot decrypt chat contents.
```

Inaccurate claim:

```text
The relay leaves no recoverable trace anywhere.
```
