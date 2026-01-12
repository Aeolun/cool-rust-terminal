//! GitHub release update checker
//!
//! Checks for new releases on GitHub and reports update availability.

use serde::Deserialize;
use std::time::Duration;

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/Aeolun/cool-rust-terminal/releases/latest";
const RELEASES_URL: &str = "https://github.com/Aeolun/cool-rust-terminal/releases";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Information about available updates
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// The latest version available on GitHub (e.g., "0.2.7")
    pub latest_version: String,
    /// The currently running version
    pub current_version: String,
    /// Whether an update is available
    pub update_available: bool,
    /// URL to the releases page
    pub release_url: String,
}

/// GitHub API response for latest release
#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

/// Check GitHub for the latest release and compare with current version.
///
/// Returns `Some(UpdateInfo)` on success, `None` on any error (network, parse, etc.)
/// This function is designed to fail silently - update checking should never
/// interrupt the user experience.
pub fn check_for_updates() -> Option<UpdateInfo> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    // Make HTTP request with timeout
    let response = ureq::get(GITHUB_API_URL)
        .set("User-Agent", "cool-rust-terminal")
        .set("Accept", "application/vnd.github.v3+json")
        .timeout(REQUEST_TIMEOUT)
        .call()
        .ok()?;

    // Parse JSON response
    let release: GitHubRelease = response.into_json().ok()?;

    // Strip 'v' prefix if present (e.g., "v0.2.7" -> "0.2.7")
    let latest_version = release.tag_name.trim_start_matches('v').to_string();

    // Compare versions
    let update_available = is_newer_version(&latest_version, &current_version);

    Some(UpdateInfo {
        latest_version,
        current_version,
        update_available,
        release_url: RELEASES_URL.to_string(),
    })
}

/// Compare two semver-style version strings.
/// Returns true if `latest` is newer than `current`.
fn is_newer_version(latest: &str, current: &str) -> bool {
    let parse_version = |s: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() >= 3 {
            Some((
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            ))
        } else if parts.len() == 2 {
            Some((parts[0].parse().ok()?, parts[1].parse().ok()?, 0))
        } else {
            None
        }
    };

    match (parse_version(latest), parse_version(current)) {
        (Some((lmaj, lmin, lpatch)), Some((cmaj, cmin, cpatch))) => {
            (lmaj, lmin, lpatch) > (cmaj, cmin, cpatch)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("0.2.7", "0.2.6"));
        assert!(is_newer_version("0.3.0", "0.2.9"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("0.2.6", "0.2.6"));
        assert!(!is_newer_version("0.2.5", "0.2.6"));
        assert!(!is_newer_version("0.1.0", "0.2.0"));
    }
}
