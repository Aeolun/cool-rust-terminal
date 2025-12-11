// ABOUTME: Session state persistence for terminal restoration.
// ABOUTME: Saves pane scrollback and working directories to disk.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Session data for a single pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSession {
    /// Compressed scrollback content (zstd compressed JSON)
    pub scrollback: Vec<u8>,
    /// Working directory at close time
    pub cwd: Option<PathBuf>,
    /// Pane position in layout (for potential future layout restoration)
    pub layout_index: usize,
}

/// Complete session data for the terminal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub version: u32,
    pub panes: Vec<PaneSession>,
}

impl SessionData {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            panes: Vec::new(),
        }
    }

    /// Add a pane's session data
    pub fn add_pane(&mut self, scrollback: Vec<u8>, cwd: Option<PathBuf>, layout_index: usize) {
        self.panes.push(PaneSession {
            scrollback,
            cwd,
            layout_index,
        });
    }

    /// Get the default session file path (~/.local/state/cool-rust-term/session.bin)
    pub fn default_path() -> Option<PathBuf> {
        // Use state_dir on macOS/Linux, fall back to data_local_dir
        dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .map(|p| p.join("cool-rust-term").join("session.bin"))
    }

    /// Save session data to disk
    pub fn save(&self, path: &std::path::Path) -> Result<(), SessionError> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Serialize to JSON then compress with zstd
        let json = serde_json::to_vec(self)?;
        let mut encoder = zstd::Encoder::new(Vec::new(), 3)?;
        encoder.write_all(&json)?;
        let compressed = encoder.finish()?;

        std::fs::write(path, compressed)?;
        Ok(())
    }

    /// Save session to default path
    pub fn save_to_default(&self) -> Result<PathBuf, SessionError> {
        let path = Self::default_path().ok_or(SessionError::NoStatePath)?;
        self.save(&path)?;
        Ok(path)
    }

    /// Load session data from disk
    pub fn load(path: &std::path::Path) -> Result<Self, SessionError> {
        let compressed = std::fs::read(path)?;

        let mut decoder = zstd::Decoder::new(&compressed[..])?;
        let mut json = Vec::new();
        decoder.read_to_end(&mut json)?;

        let session: SessionData = serde_json::from_slice(&json)?;

        // Version check - for future compatibility
        if session.version > Self::CURRENT_VERSION {
            return Err(SessionError::UnsupportedVersion(session.version));
        }

        Ok(session)
    }

    /// Load session from default path, returns None if not found or invalid
    pub fn load_from_default() -> Option<Self> {
        let path = Self::default_path()?;
        Self::load(&path).ok()
    }

    /// Delete the session file
    pub fn clear_default() -> Result<(), SessionError> {
        if let Some(path) = Self::default_path() {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
}

impl Default for SessionData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Could not determine state directory")]
    NoStatePath,

    #[error("Unsupported session version: {0}")]
    UnsupportedVersion(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_roundtrip() {
        let mut session = SessionData::new();
        session.add_pane(vec![1, 2, 3], Some(PathBuf::from("/home/test")), 0);
        session.add_pane(vec![4, 5, 6], None, 1);

        // Save to temp file
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_session.bin");

        session.save(&temp_path).unwrap();

        // Load back
        let loaded = SessionData::load(&temp_path).unwrap();

        assert_eq!(loaded.version, SessionData::CURRENT_VERSION);
        assert_eq!(loaded.panes.len(), 2);
        assert_eq!(loaded.panes[0].scrollback, vec![1, 2, 3]);
        assert_eq!(loaded.panes[0].cwd, Some(PathBuf::from("/home/test")));
        assert_eq!(loaded.panes[1].scrollback, vec![4, 5, 6]);
        assert_eq!(loaded.panes[1].cwd, None);

        // Cleanup
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_default_path() {
        // Should return Some on most systems
        let path = SessionData::default_path();
        // Just verify it doesn't panic and has expected structure
        if let Some(p) = path {
            assert!(p.ends_with("cool-rust-term/session.bin"));
        }
    }
}
