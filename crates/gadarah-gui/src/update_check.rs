//! Background update check.
//!
//! On GUI startup, spawn a thread that hits the GitHub Releases API for the
//! latest tag, compares it to `CARGO_PKG_VERSION`, and pushes an Info-level
//! alert into `SharedState.alerts` if a newer version is available. The
//! banner widget surfaces it with an "Update Now" button; clicking that
//! button spawns the local `gadarah-wizard.exe --update` (preferred) or
//! falls back to opening the GitHub release page.
//!
//! Cached at `$CONFIG/gadarah/update_check.json` so we don't hammer the
//! GitHub API on every launch — checks at most once every 6 hours.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::state::{Alert, AlertSeverity, AppState};

/// Public release endpoint. No auth required — anonymous quota is 60/hr,
/// far above what one user opening the GUI can produce.
const RELEASES_URL: &str =
    "https://api.github.com/repos/magusgriffin/GADARAH/releases/latest";

/// Don't re-query GitHub more often than this.
const CHECK_INTERVAL_SECS: i64 = 6 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReleaseCache {
    last_checked: i64,
    latest_tag: Option<String>,
    release_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

fn cache_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("gadarah").join("update_check.json"))
}

fn load_cache() -> Option<ReleaseCache> {
    let path = cache_path()?;
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_cache(cache: &ReleaseCache) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(cache) {
        let _ = std::fs::write(&path, bytes);
    }
}

/// Compare two `vX.Y.Z` (or `X.Y.Z`) strings. Returns `true` iff `latest`
/// is strictly greater than `current`. Anything that doesn't parse falls
/// back to `false` so a malformed tag never spams the user with a
/// downgrade alert.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let trimmed = s.trim_start_matches('v').trim();
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() < 3 {
            return None;
        }
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        // Strip any pre-release suffix like "1-rc1".
        let patch_str = parts[2].split('-').next()?;
        let patch = patch_str.parse().ok()?;
        Some((major, minor, patch))
    };
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Spawn the background check. Idempotent — safe to call once per app
/// launch. Caller hands us a clone of `AppState` so we can push the alert
/// from the worker thread without blocking the UI.
pub fn spawn(state: AppState) {
    std::thread::Builder::new()
        .name("gadarah-gui-update-check".into())
        .spawn(move || run_check(state))
        .ok();
}

fn run_check(state: AppState) {
    let now = chrono::Utc::now().timestamp();
    let current = env!("CARGO_PKG_VERSION");

    // Honour the cache cooldown so we don't burn GitHub quota on every launch.
    let cached = load_cache();
    let should_query = match &cached {
        Some(c) => now - c.last_checked >= CHECK_INTERVAL_SECS,
        None => true,
    };

    let release = if should_query {
        fetch_latest_release()
    } else {
        // Cached tag is stale-but-fresh-enough — synthesise a release
        // record from it so we still surface the banner if an update was
        // already known.
        cached.as_ref().and_then(|c| {
            Some(GhRelease {
                tag_name: c.latest_tag.clone()?,
                html_url: c.release_url.clone()?,
                draft: false,
                prerelease: false,
            })
        })
    };

    let Some(release) = release else {
        return;
    };
    if release.draft || release.prerelease {
        return;
    }

    if should_query {
        save_cache(&ReleaseCache {
            last_checked: now,
            latest_tag: Some(release.tag_name.clone()),
            release_url: Some(release.html_url.clone()),
        });
    }

    if !is_newer(&release.tag_name, current) {
        return;
    }

    // Push the alert. Banner picks it up next frame.
    let alert = Alert {
        timestamp: now,
        severity: AlertSeverity::Info,
        title: format!("Update available — {}", release.tag_name),
        body: format!(
            "A newer GADARAH ({}) is available. You're on {}. Click Update Now to refresh.",
            release.tag_name, current,
        ),
        dismissed: false,
        action_url: Some(release.html_url),
        action_update_wizard: true,
        oracle_advice: None,
        suppress_os_notification: false,
    };
    if let Ok(mut g) = state.lock() {
        g.push_alert(alert);
    }
}

fn fetch_latest_release() -> Option<GhRelease> {
    let agent = format!("gadarah-gui/{}", env!("CARGO_PKG_VERSION"));
    let client = reqwest::blocking::Client::builder()
        .user_agent(agent)
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "update_check: GitHub returned non-2xx");
        return None;
    }
    resp.json::<GhRelease>().ok()
}

/// Resolve the wizard binary path next to the running GUI executable. When
/// the user installed via the wizard, `gadarah-wizard.exe` lives in the
/// same directory. Returns `None` for dev/cargo-run scenarios.
pub fn local_wizard_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        "gadarah-wizard.exe"
    } else {
        "gadarah-wizard"
    };
    let candidate = dir.join(name);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Spawn the local wizard in update mode. Falls back to opening the
/// release URL in the browser if the wizard isn't co-located. Returns the
/// fallback URL on success-via-browser so the caller can log it.
pub fn launch_update_wizard(release_url: Option<&str>) -> Result<(), String> {
    if let Some(wizard) = local_wizard_path() {
        return std::process::Command::new(&wizard)
            .arg("--update")
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("spawn wizard {}: {e}", wizard.display()));
    }
    if let Some(url) = release_url {
        return open_action_url(url);
    }
    Err("no local wizard and no release URL".into())
}

/// Open a URL in the user's default browser. Used by the alert banner when
/// the user clicks "Open" on an alert that has an `action_url` but no
/// wizard affordance.
pub fn open_action_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let cmd = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let cmd = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = std::process::Command::new("xdg-open").arg(url).spawn();
    cmd.map(|_| ()).map_err(|e| format!("open {url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_semver_segments() {
        assert!(is_newer("v2.1.5", "2.1.4"));
        assert!(is_newer("2.2.0", "v2.1.99"));
        assert!(!is_newer("v2.1.4", "2.1.4"));
        assert!(!is_newer("v2.1.3", "2.1.4"));
    }

    #[test]
    fn newer_handles_prerelease_suffix() {
        // The prerelease suffix is dropped in parsing, so v2.1.5-rc1 == v2.1.5.
        assert!(!is_newer("v2.1.5-rc1", "v2.1.5"));
        assert!(is_newer("v2.1.5", "v2.1.4-final"));
    }

    #[test]
    fn newer_returns_false_on_garbage() {
        assert!(!is_newer("not-a-version", "2.1.4"));
        assert!(!is_newer("v2.1.5", "garbage"));
    }
}
