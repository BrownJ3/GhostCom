# Protocol Notes

GhostCom uses a secure transport for encryption and authentication. The application protocol only frames already-encrypted messages.

## Design Rules

- Do not put cryptography in the application frame layer.
- Do not include timestamps in the first release.
- Do not include usernames or durable peer identifiers in the first release.
- Reject messages above the configured maximum size.
- Reject invalid UTF-8 chat payloads.
- Treat unknown critical frame types as errors.

## Initial Frame Shape

The exact binary format will be finalized during implementation. The intended structure is:

```text
magic/version
message_type
payload_length
payload
```

## Message Types

```text
invite_proof
hello
chat
group_chat
typing_start
typing_stop
close
```

Relay `invite_proof` frames are encrypted end-to-end and are only valid during relay session setup immediately after the Noise handshake. Each proof is a 32-byte SHA-256 value bound to the client-generated invite secret, the Noise handshake hash, and the sender role. The relay never receives the invite secret because clients send only the room portion of a full `room.secret` invite to the server.

Private relay deployments can require device authorization before invite creation or join. In that mode, each relay setup message includes a client device public key, nonce, and Ed25519 signature over the setup action, optional room code, and nonce. The relay checks the public key against `GHSTCOM_RELAY_ALLOWED_DEVICE_KEYS`, enforces the optional `public_key@expires_at_unix` approval expiry, and verifies the signature before allowing `create`, `join`, `group_create`, or `group_join`.

Group relay invites use the `g:room.secret` form. The relay receives only the room code. Each joiner establishes a separate Noise session with the group host, proves knowledge of the invite secret inside that encrypted session, and then exchanges chat frames through the host. `group_chat` frames carry an ephemeral sender id, sender display name, and message so the host can re-encrypt participant messages to the other joiners without asking the relay to understand chat contents.

Typing frames are encrypted end-to-end like chat frames. They carry no payload,
are never persisted, and are only used for transient terminal presence during an
active session. Emitting typing frames is disabled by default until capability
negotiation is added, because older clients reject unknown frame types.

## Size Limits

Initial limits should be conservative:

```text
max chat payload: 8192 bytes
max frame payload: 16384 bytes
```

These limits protect the terminal UI and reduce memory-exhaustion risk.

## Compatibility

Protocol changes must update this document and include tests for rejecting incompatible or malformed frames.
