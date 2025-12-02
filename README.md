# Cool Rust Terminal

<img src="assets/icon.png" width="128" align="right" alt="Cool Rust Terminal icon">

A CRT-styled terminal emulator written in Rust, inspired by [cool-retro-term](https://github.com/Swordfish90/cool-retro-term).

![Cool Rust Terminal](https://img.shields.io/badge/status-work%20in%20progress-yellow)
[![Built with Claude Code](https://img.shields.io/badge/Built%20with-Claude%20Code-D97757?logo=claude&logoColor=white)](https://claude.ai/code)

![Screenshot](screenshot.png)

## Features

- **CRT Visual Effects**
  - Barrel distortion (curved screen)
  - Scanlines (aligned to text rows for readability)
  - Phosphor bloom/glow
  - Burn-in persistence effect
  - Static noise and flicker
  - Vignette (edge darkening)
  - Focus glow for active pane

- **Multi-Pane Support**
  - Automatic grid layout (up to 16 panes)
  - Per-pane CRT effects mode
  - Amber separator lines between panes
  - Click to focus, visual focus indicators

- **Terminal Features**
  - Full terminal emulation via alacritty_terminal
  - 10,000 line scrollback buffer
  - Mouse wheel and Shift+PageUp/Down scrolling
  - Text selection with auto-copy to clipboard
  - Full ANSI color support (16, 256, and true color)

- **Customization**
  - Live config UI (Ctrl+,)
  - Color schemes: Amber, Green, White, ANSI
  - 13 bundled fonts (retro IBM + modern options)
  - All effects adjustable via sliders

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+Enter` | Add new pane |
| `Ctrl+,` or `Ctrl+Shift+P` | Toggle config UI |
| `Ctrl+Shift+G` | Toggle debug grid |
| `Ctrl+Shift+C` | Copy selection |
| `Ctrl+Shift+V` | Paste |
| `Shift+PageUp/Down` | Scroll history |
| Mouse wheel | Scroll history |

## Installation

The easiest way to install Cool Rust Terminal is via the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/Aeolun/cool-rust-terminal/main/install.sh | bash
```

This works on **Linux** (x86_64) and **macOS** (Intel & Apple Silicon). On Linux, it installs the binary and creates a desktop launcher. On macOS, it installs the app to `/Applications`.

For Windows, download the latest `.zip` from [Releases](https://github.com/Aeolun/cool-rust-terminal/releases).

## Building from Source

Requires Rust 1.70+ and a GPU with Vulkan/Metal/DX12 support.

```bash
cargo build --release
./target/release/cool-rust-term
```

## Development

After cloning, set up git hooks for automatic formatting and lint checks:

```bash
just setup
```

This runs `cargo fmt` and `cargo clippy` before each commit.

## Architecture

The project is organized as a Cargo workspace with multiple crates:

- `crt-app` - Main application, window management, event handling
- `crt-renderer` - wgpu-based rendering (text, CRT effects, lines)
- `crt-terminal` - Terminal emulation wrapper around alacritty_terminal
- `crt-layout` - Pane layout management
- `crt-core` - Shared types, config, color schemes

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed technical documentation.

## Configuration

Config is stored at `~/.config/cool-rust-term/config.toml` and is auto-saved when modified through the UI.

## Credits

- Inspired by [cool-retro-term](https://github.com/Swordfish90/cool-retro-term) by Filippo Scognamiglio
- Terminal emulation by [alacritty_terminal](https://github.com/alacritty/alacritty)
- GPU rendering via [wgpu](https://wgpu.rs/)

## License

This project is licensed under the **GNU General Public License v3.0** - see the [LICENSE](LICENSE) file for details.
