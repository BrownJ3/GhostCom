# Threat Model

This document keeps GhostCom honest about what it does and does not protect.

## Assets

GhostCom protects:

- Chat message contents in transit.
- Session keys.
- Peer authentication material.
- In-process message state.

GhostCom avoids creating:

- Chat logs.
- Message history.
- Contact lists.
- Connection history.
- Telemetry.

## In-Scope Attackers

GhostCom should defend against:

- Passive network observers.
- Active network attackers attempting interception or tampering.
- A malicious or compromised rendezvous server attempting peer substitution.
- Replay of malformed or stale frames within a session.
- Malformed input intended to crash the app or exhaust memory.
- Accidental local persistence by the application.

## Out-of-Scope Attackers

GhostCom does not claim to defeat:

- A compromised local machine.
- A malicious or compromised peer.
- Screen recording, screenshots, or copied terminal text.
- Operating system swap, hibernation, or crash dumps.
- Traffic analysis of IP addresses, timing, and packet sizes.
- Malware with access to process memory.
- Denial of service against the rendezvous server.

## Trust Assumptions

Users must verify the shared session code out-of-band before relying on confidentiality against active network attackers.

The rendezvous server is untrusted. It can observe connection metadata and can attempt to interfere with setup, but it must not be able to read messages or silently complete a MITM attack when users verify the shared session code.

The selected cryptographic libraries are trusted to implement their protocols correctly. GhostCom should minimize its own cryptographic decision-making.

## Privacy Claims

Accurate claim:

```text
GhostCom does not intentionally persist messages or connection metadata.
```

Inaccurate claim:

```text
GhostCom leaves no trace anywhere on the system or network.
```

The second claim is not realistic and must not be used.

## Rendezvous Metadata

The rendezvous service may observe:

- Caller IP address.
- Joiner IP address.
- Invite creation time.
- Invite join time.
- Whether an invite was joined.

The rendezvous service must not intentionally persist this information in durable application storage.
