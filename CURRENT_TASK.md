# Session Restoration Feature

## Goal
Restore terminal sessions on startup, including scrollback buffer and working directory, so users can continue where they left off.

## What Can Be Restored
- **Scrollback buffer**: Visual history of what was on screen
- **Working directory**: The cwd of each shell at close time
- **Pane layout**: Already implemented (pane count saved/restored)

## What Cannot Be Restored
- Running processes (vim, ssh, htop, etc.) - they're terminated on close
- Shell state (unexported variables, aliases loaded at runtime)
- Command history position

## Requirements

### 1. Scrollback Serialization
- On close: Extract scrollback from each pane's `Grid`
- Compress before storage (zstd or similar) - max 10k lines × 16 panes = 160k lines worst case
- Store in a session file (separate from config, e.g., `~/.local/state/cool-rust-term/session.bin`)
- On restore: Decompress and replay into new terminal grid

### 2. Working Directory Query
Need platform-specific code to get cwd from child process:

**Linux:**
```rust
fn get_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{}/cwd", pid)).ok()
}
```

**macOS:**
```rust
// Use libproc crate or libc::proc_pidinfo
fn get_cwd(pid: u32) -> Option<PathBuf> {
    // proc_pidinfo with PROC_PIDVNODEPATHINFO
}
```

**Windows:**
- Complex: requires NtQueryInformationProcess or WMI
- Consider: skip cwd restore on Windows, or use simpler heuristic

### 3. Session File Format
```rust
struct SessionData {
    version: u32,
    panes: Vec<PaneSession>,
}

struct PaneSession {
    /// Compressed scrollback content (lines + attributes)
    scrollback: Vec<u8>,
    /// Working directory at close time
    cwd: Option<PathBuf>,
    /// Pane position in layout (for future layout restoration)
    layout_index: usize,
}
```

### 4. Terminal Changes Needed
- Expose child PID from `Terminal` wrapper
- Add method to serialize grid content
- Add method to restore grid from serialized content
- Add `working_directory` parameter to `Terminal::new()`

### 5. User Experience
- On close: Save session automatically (if enabled in config)
- On startup: Check for session file
  - If found and valid: restore panes with scrollback, start shells in saved cwds
  - If invalid/missing: start fresh (current behavior)
- Add config option: `behavior.restore_session: bool` (default: true)
- Consider: command to manually save/clear session

## Implementation Order
1. Add PID tracking to Terminal
2. Implement cwd query (Linux: `/proc/{pid}/cwd`, macOS: libproc)
3. Implement scrollback serialization/compression (zstd)
4. Session file format and save on close
5. Session restore on startup (skip entirely on Windows)
6. Config option `behavior.restore_session` and polish

## Decisions Made
- **Pane layout**: Pane count determines layout (no manual splitting), so just restore count
- **Session expiry**: None. Keep forever. Users can clean `~/.local/state/cool-rust-term/` themselves if needed
- **Windows**: Skip session restore entirely (no cwd, no scrollback) - would be confusing to have partial restore
