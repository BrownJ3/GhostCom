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
- Cap WebSocket setup message sizes.
- Cap relayed binary frame sizes.
- Drop both peers on malformed relay traffic.
- Avoid logging relay payloads.
- Avoid persistent storage.

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

