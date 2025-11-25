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
│   Automatic grid layout. Arranges N panes in near-square grid.  │
│   Adapts to window aspect ratio (landscape vs portrait).        │
│                                                                  │
│   Example: 5 panes in landscape (1/2/2 columns)                 │
│   ┌───────┬───────┬───────┐                                     │
│   │       │   2   │   4   │                                     │
│   │   1   ├───────┼───────┤                                     │
│   │       │   3   │   5   │                                     │
│   └───────┴───────┴───────┘                                     │
│                                                                  │
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

### 1. Automatic Grid Layout

```rust
struct LayoutTree {
    panes: Vec<PaneId>,
    focused: PaneId,
    next_id: u64,
}
```

The layout automatically arranges panes in a near-square grid:
- Number of columns = `ceil(sqrt(n))`
- Extra panes fill the last columns
- Landscape windows: columns side-by-side
- Portrait windows: rows stacked

Operations:
- `add_pane()` - adds a new pane, gets focus
- `close(pane_id)` - removes pane, grid reflows
- `pane_rects(width, height)` - calculates layout rectangles

### 2. Per-Pane vs Shared Rendering

**Current: Shared CRT effect across all panes**

All panes render to a single offscreen texture, then CRT effects are applied
to the whole thing. Amber separator lines drawn between panes.

**Future option: Per-pane CRT effects**

Each pane would get its own render texture and CRT effect chain, making each
pane look like its own "monitor". Would require per-pane offscreen textures
and a final compositor pass.

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

3. **Pane borders**: Amber separator lines using box drawing characters

4. **Effect presets**: Start with Amber only

5. **Scrollback**: Scrollbar + mouse wheel

## Implementation Progress

### Completed (Phase 1 + Phase 2 partial)

- [x] Cargo workspace with 5 crates (crt-core, crt-terminal, crt-layout, crt-renderer, crt-app)
- [x] winit window + event loop
- [x] wgpu device/surface initialization
- [x] Glyph atlas using fontdue (IBM VGA font, 18px)
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
- [x] Automatic grid layout system (adapts to window aspect ratio)
- [x] Multiple panes with independent terminals
- [x] Shift+Ctrl+Enter to add new pane
- [x] Shell exit closes pane, last pane closes app
- [x] Amber separator lines between panes
- [x] Pane content padding (8px)

### Known Issues

- Only ASCII rendering supported (non-ASCII → '?')
- Text selection doesn't show visual highlight (no background rendering yet)
- CRT effect intensities are hardcoded (need config system)
- No way to change focus between panes (click-to-focus not implemented)

### Next Steps

**Phase 2 - Remaining:**
- Burn-in effect (ping-pong framebuffers)
- Click to change pane focus
- Per-pane CRT effects (optional mode)

**Phase 3 - Polish:**
- Config file loading (TOML)
- Font selection
- Effect intensity controls
- Scrollback with scrollbar
- ANSI color support (interpret terminal color escape codes)

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
