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
hello
chat
typing_start
typing_stop
close
```

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
