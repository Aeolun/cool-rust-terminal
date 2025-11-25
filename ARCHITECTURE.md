# cool-rust-term Architecture

## Overview

A GPU-accelerated terminal emulator with CRT visual effects and tiling/split support.

## Core Components

```
┌─────────────────────────────────────────────────────────────────┐
│                         Application                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   Config    │  │   Input     │  │      Window/Event       │  │
│  │   System    │  │   Router    │  │        (winit)          │  │
│  └─────────────┘  └──────┬──────┘  └─────────────────────────┘  │
└──────────────────────────┼──────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────────┐
│                       Layout Manager                             │
│                                                                  │
│   Binary tree of splits. Each leaf = one terminal pane.         │
│                                                                  │
│        ┌─────────┐                                              │
│        │  Root   │                                              │
│        │ (Hsplit)│                                              │
│        └────┬────┘                                              │
│        ┌────┴────┐                                              │
│   ┌────▼───┐ ┌───▼────┐                                         │
│   │ Pane 1 │ │ Vsplit │                                         │
│   └────────┘ └───┬────┘                                         │
│              ┌───┴───┐                                          │
│         ┌────▼──┐ ┌──▼────┐                                     │
│         │Pane 2 │ │Pane 3 │                                     │
│         └───────┘ └───────┘                                     │
└─────────────────────────────────────────────────────────────────┘
                           │
                           │ Each pane owns:
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Terminal Pane                               │
│  ┌──────────────────┐  ┌──────────────────────────────────────┐ │
│  │  PTY + Process   │  │     alacritty_terminal::Term         │ │
│  │                  │◄─┤     (escape parsing, grid state)     │ │
│  └──────────────────┘  └──────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                           │
                           │ Grid of cells (char + style)
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Renderer                                  │
│                                                                  │
│  ┌─────────────────┐      ┌─────────────────────────────────┐   │
│  │   Glyph Atlas   │      │      Per-Pane Render Target     │   │
│  │  (font raster)  │─────►│  1. Render text to texture      │   │
│  └─────────────────┘      │  2. Apply CRT shader chain      │   │
│                           │  3. Output to pane region       │   │
│                           └─────────────────────────────────┘   │
│                                                                  │
│  CRT Shader Chain (per pane):                                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │  Text    │─►│  Static  │─►│ Dynamic  │─►│ Burn-in  │─► Out  │
│  │ Texture  │  │ Effects  │  │ Effects  │  │ Feedback │        │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘        │
│                                                                  │
│  Static: curvature, bloom, RGB shift, colorization             │
│  Dynamic: scanlines, noise, flicker, h-sync jitter             │
│  Burn-in: recursive framebuffer for phosphor decay             │
└─────────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Final Compositor                             │
│                                                                  │
│  Composites all pane outputs into final framebuffer.            │
│  Draws pane borders/separators if configured.                   │
└─────────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. Layout Tree Structure

```rust
enum LayoutNode {
    Split {
        direction: Direction,  // Horizontal | Vertical
        ratio: f32,            // 0.0 - 1.0, position of split
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
    Pane(PaneId),
}
```

Operations:
- `split_pane(pane_id, direction)` - splits a pane, new pane gets focus
- `close_pane(pane_id)` - removes pane, sibling takes its space
- `resize_split(pane_id, delta)` - adjust the split ratio
- `navigate(direction)` - move focus to adjacent pane

### 2. Per-Pane vs Shared Rendering

**Decision: Per-pane CRT effects**

Each pane gets its own:
- Render texture
- CRT effect chain
- Burn-in buffer

Why: Looks cooler. Each pane is its own "monitor". Also simpler - no need to
coordinate effect state across panes.

### 3. Burn-in Implementation

The burn-in effect requires feeding the previous frame back into the shader.

```
Frame N:
  1. Read burn-in texture from frame N-1
  2. Blend current text with decayed burn-in
  3. Write result to burn-in texture for frame N+1
  4. Output to screen
```

Needs ping-pong buffers (two textures, alternate read/write each frame).

### 4. Text Rendering Pipeline

Using a glyph atlas approach (standard for GPU text):

1. Rasterize glyphs on-demand using a font library (fontdue, ab_glyph)
2. Pack into atlas texture
3. Render quads with atlas UV coordinates
4. Cell background colors rendered as solid quads behind text

### 5. Input Handling

```
Keyboard → focused pane's PTY
Mouse → hit-test layout tree → target pane
  - Click: focus + forward to terminal if needed
  - Drag on separator: resize split
```

## Crate Structure

```
cool-rust-term/
├── Cargo.toml              # Workspace
├── crates/
│   ├── crt-core/           # Shared types, config
│   ├── crt-terminal/       # PTY + alacritty_terminal wrapper
│   ├── crt-layout/         # Layout tree, split management
│   ├── crt-renderer/       # wgpu rendering, shaders, glyph atlas
│   └── crt-app/            # Main binary, window, input, glue
└── shaders/
    ├── text.wgsl           # Text/glyph rendering
    ├── crt_static.wgsl     # Curvature, bloom, color
    ├── crt_dynamic.wgsl    # Noise, flicker, scanlines
    └── crt_burnin.wgsl     # Phosphor burn-in
```

## Decisions

1. **Config format**: TOML

2. **Font handling**: Use original cool-retro-term bitmap-style TTF fonts.
   Available: IBM 3278, Terminus, Apple II, IBM PC BIOS, Commodore PET,
   Fixedsys, Hermit, C64, Inconsolata, ProFont, Atari 400/800, IBM VGA, ProggyTiny

3. **Pane borders**: Plain gray separator lines

4. **Effect presets**: Start with Amber only

5. **Scrollback**: Scrollbar + mouse wheel

## Implementation Progress

### Completed (Phase 1)

- [x] Cargo workspace with 5 crates (crt-core, crt-terminal, crt-layout, crt-renderer, crt-app)
- [x] winit window + event loop
- [x] wgpu device/surface initialization
- [x] Glyph atlas using fontdue (IBM VGA font, 24px)
- [x] Text rendering shader (text.wgsl)
- [x] Terminal emulation via alacritty_terminal 0.25
- [x] PTY creation and I/O
- [x] Grid rendering from terminal state
- [x] Cursor display (amber block)
- [x] Keyboard input with modifier keys (Ctrl, Alt)
- [x] App closes when shell exits
- [x] Non-ASCII character handling (collapsed to single '?')
- [x] Text selection with mouse (auto-copy to clipboard on release)
- [x] CRT post-processing shader with two-pass rendering:
  - Off-screen texture for terminal content
  - Barrel distortion (curvature: 0.03)
  - Animated scanlines with subtle drift
  - Bloom effect
  - Flicker animation
  - Static noise
  - Vignette darkening at edges

### Known Issues

- Only ASCII rendering supported (non-ASCII → '?')
- Text selection doesn't show visual highlight (no background rendering yet)
- CRT effect intensities are hardcoded (need config system)

### Next Steps

**Phase 1 complete!**

**Phase 2 - Core features:**
3. Burn-in effect (ping-pong framebuffers)
4. Full CRT shader chain (noise, flicker, RGB shift)
5. Split/layout system integration
6. Multiple panes with independent terminals

**Phase 3 - Polish:**
7. Config file loading (TOML)
8. Font selection
9. Effect intensity controls
10. Scrollback with scrollbar
11. ANSI color support (interpret terminal color escape codes)

## Technical Notes

### Terminal Grid Reading

The terminal grid is read via `alacritty_terminal::Grid`. Key points:
- `Line(0)` is top of visible area
- Wide chars have `WIDE_CHAR_SPACER` flag on second cell
- Cursor position from `grid.cursor.point`

### Character Filtering

Non-ASCII characters and wide char spacers are collapsed - each non-ASCII run
becomes a single '?', and wide char spacers are skipped entirely:
```rust
if is_wide_spacer {
    continue;  // Skip spacer cells entirely
} else if is_non_ascii {
    if in_run { continue; }  // Skip consecutive non-ASCII
    c = '?';
    in_run = true;
} else {
    in_run = false;
}
```

### CRT Effect Pipeline (planned)

```
Terminal Grid → Text Texture → CRT Shader → Screen
                                   ↓
                            - Barrel distortion
                            - Scanlines
                            - Bloom/glow
                            - Color tint
```
