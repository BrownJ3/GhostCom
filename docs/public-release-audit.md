# Public Release Exposure Audit

Date: 2026-05-25

This audit records what would become visible if the repository or release assets
are made public.

## No Sensitive Secrets Found

The working tree and Git history were scanned for common secret patterns:

- API keys and tokens.
- Passwords and client secrets.
- Fly, GitHub, AWS, database, webhook, and OpenAI-style credentials.
- Private key, certificate, and `.env` material.

No damaging credentials, private keys, chat secrets, session secrets, or
deployment tokens were found.

## Expected Public Identifiers

The following identifiers are present and should be considered public if the
repository is opened:

- GitHub repository path: `BrownJ3/GhostCom`.
- Fly app hostname: `ghostcom-site.fly.dev`.
- Default relay endpoint: `wss://ghostcom-site.fly.dev/relay`.
- Default rendezvous endpoint: `wss://ghostcom-site.fly.dev/rv`.
- Commit author metadata has been rewritten to
  `GhostCom Maintainers <noreply@ghostcom.local>`.

These are not application secrets, but they do identify the project, deployment
target, and commit author.

## Operational Exposure

Public code reveals implementation details such as invite-code length, rate
limit settings, route names, protocol framing, and release packaging. These must
not be treated as secrets. Security must come from cryptographic design, peer
verification, rate limiting, and signed distribution.

## Before Making Public

- Keep local Git identity configured to non-personal project metadata before
  making future commits.
- Keep installer scripts in GitHub, not on the Fly runtime service.
- Sign release checksums before broad public distribution.
- Keep real deployment secrets in Fly/GitHub secret stores only, never in this
  repository.
