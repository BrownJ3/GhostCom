# Cross-Platform Requirements

GhostCom must feel like the same tool on macOS, Windows, and Linux. The first release should be usable by people who are comfortable opening a terminal but should not require platform-specific setup beyond obtaining the binary.

## Supported Platforms

Initial support target:

```text
macOS x86_64
macOS aarch64
Windows x86_64
Linux x86_64
```

Linux aarch64 is a desirable later target, but the initial automated release workflow focuses on the common desktop platforms first.

## Distribution Goal

Each release should provide standalone binaries where practical:

```text
ghstprtcl-x86_64-apple-darwin.tar.gz
ghstprtcl-aarch64-apple-darwin.tar.gz
ghstprtcl-x86_64-pc-windows-msvc.zip
ghstprtcl-x86_64-unknown-linux-gnu.tar.gz
```

Package managers can come later. The first goal is a downloadable binary that runs from a terminal.

Install scripts may be served from the GhostCom site, but they must download binaries from GitHub Releases and verify the published SHA-256 checksum before installation. Future releases should add stronger signature verification before claiming production-grade distribution security.

## Terminal Behavior

The default interface should work in common terminals:

- macOS Terminal.
- iTerm2.
- Windows Terminal.
- PowerShell.
- Command Prompt where practical.
- Common Linux terminal emulators.
- SSH sessions.

Avoid assumptions about:

- ANSI feature completeness.
- Terminal dimensions.
- Unicode glyph support.
- Clipboard access.
- Shell-specific behavior.

The first UI should be line-oriented before introducing a full-screen TUI.

## Networking Behavior

The release binary is named `ghstprtcl`. Running it without arguments opens the menu:

```text
ghstprtcl
```

The first release supports direct connections:

```text
peer A: ghstprtcl listen --bind 0.0.0.0:7777
peer B: ghstprtcl connect <host>:7777
```

Users may need to configure firewalls, port forwarding, VPN, Tailscale, WireGuard, or another private network for direct mode. For cross-network use where direct reachability fails, GhostCom supports relay mode:

```text
peer A: ghstprtcl relay-call --relay wss://ghostcom-site.fly.dev/relay
peer B: ghstprtcl relay-join <invite-code> --relay wss://ghostcom-site.fly.dev/relay
```

Relay mode must be described precisely: the relay cannot decrypt chat contents when users verify the Noise code, but it can observe connection metadata.

## Filesystem Behavior

By default, GhostCom should not create:

- Config directories.
- Cache directories.
- Log files.
- History files.
- Contact files.
- Identity files.

Tests should verify that a normal chat session does not create application files.

## Release Checks

Before publishing binaries, run smoke tests on all supported platforms:

- Start listener.
- Connect from another terminal.
- Start relay call through the deployed relay.
- Join relay call through the deployed relay.
- Verify shared session code prompt.
- Send messages both directions.
- Close from each side.
- Confirm no application files were created.
- Download release assets.
- Verify each asset against `SHA256SUMS`.
