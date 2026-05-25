# Cross-Platform Requirements

GhostCom must feel like the same tool on macOS, Windows, and Linux. The first release should be usable by people who are comfortable opening a terminal but should not require platform-specific setup beyond obtaining the binary.

## Supported Platforms

Initial support target:

```text
macOS x86_64
macOS aarch64
Windows x86_64
Linux x86_64
Linux aarch64
```

## Distribution Goal

Each release should provide standalone binaries where practical:

```text
ghostcom-macos-x86_64
ghostcom-macos-aarch64
ghostcom-windows-x86_64.exe
ghostcom-linux-x86_64
ghostcom-linux-aarch64
```

Package managers can come later. The first goal is a downloadable binary that runs from a terminal.

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

The first release supports direct connections:

```text
peer A: ghostcom listen --bind 0.0.0.0:7777
peer B: ghostcom connect <host>:7777
```

Users may need to configure firewalls, port forwarding, VPN, Tailscale, WireGuard, or another private network. GhostCom should explain connection failures clearly but should not add relay infrastructure in the first release.

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
- Verify shared session code prompt.
- Send messages both directions.
- Close from each side.
- Confirm no application files were created.
