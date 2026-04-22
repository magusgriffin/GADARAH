# GADARAH Windows Installer

This directory contains the WiX v4 source for the GADARAH Windows MSI.

## Building the MSI

Requirements:
- Windows 10/11 build host
- Rust toolchain with `x86_64-pc-windows-msvc` target
- `cargo install cargo-wix`
- WiX v4 toolset (installed automatically by `cargo wix` on first run)

Build:

```powershell
cargo build --release -p gadarah-gui
cargo build --release -p gadarah-cli
cargo wix -p gadarah-gui --nocapture
```

The MSI lands in `target\wix\GADARAH-0.1.0-x86_64.msi`.

## Features

The installer has a Feature Tree so users pick exactly what they want:

| Feature | Default | What it installs |
|---|---|---|
| GADARAH GUI | ✓ required | `gadarah-gui.exe`, Start Menu shortcut |
| CLI Daemon | ✓ on | `gadarah-cli.exe` |
| Ollama + DeepSeek R1 1.5B | ✗ off | Runs `install_ollama.ps1` post-install |

The Ollama bootstrap feature is opt-in because it downloads ~1.1 GB from the
public internet. Users who prefer a different model (7B, custom GGUF) or
a remote endpoint (Kimi K2 via Moonshot, OpenAI, etc.) can skip this step
entirely and configure the Oracle from the app.

## SmartScreen / Unsigned MSI

This MSI is **unsigned** in the v1 build. On first run users will see:

> Windows protected your PC
> Microsoft Defender SmartScreen prevented an unrecognized app from starting.

Workaround: click *More info* → *Run anyway*.

To remove the warning, an OV or EV code-signing certificate is required
(~$200–600/year). The WiX source supports signing via the standard
`LightTool` post-build hook when a cert is configured in the build
environment.

For now, the MSI's SHA-256 is published alongside the release artifact so
users can verify the download matches the published binary.

## Autostart

The installer does **not** configure autostart. Users who want the GUI to
launch on login should add a Start Menu → Startup folder shortcut manually;
the daemon should be configured via Task Scheduler with explicit credentials.

## License

GADARAH is dual-licensed under MIT OR Apache-2.0. See the `LICENSE-MIT` and
`LICENSE-APACHE` files in the installed tree.
