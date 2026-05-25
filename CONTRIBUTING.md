# Contributing

GhostCom is security-sensitive software. Small, reviewable changes are strongly preferred.

## Before Changing Behavior

Read:

- [SECURITY.md](SECURITY.md)
- [docs/threat-model.md](docs/threat-model.md)
- [docs/architecture.md](docs/architecture.md)
- [docs/protocol.md](docs/protocol.md)

## Development Rules

- Do not add message persistence.
- Do not add telemetry.
- Do not log message contents.
- Do not add custom cryptography.
- Do not add `unsafe` without a focused security review.
- Keep dependencies minimal.
- Keep cross-platform behavior in mind.

## Expected Checks

Before a release, the project should pass:

```text
cargo fmt
cargo clippy
cargo test
cargo audit
cargo deny
```

The project may not have all checks wired up during early scaffolding, but new implementation work should move toward this baseline.

