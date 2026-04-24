//! First-run detection — a marker file under the user config dir flags that
//! the welcome overlay has been shown at least once. Same pattern as
//! `OracleConfig::config_path`. If the config dir is unavailable or write
//! fails, we treat every launch as first-run; the overlay dismisses fine,
//! we just don't remember it. Logged at WARN so the user can diagnose.

use std::path::PathBuf;

/// `$CONFIG/gadarah/welcome_seen.flag`. `None` on platforms where
/// `dirs::config_dir()` returns nothing (should be impossible on Win/macOS/Linux).
pub fn flag_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("gadarah").join("welcome_seen.flag"))
}

pub fn is_first_run() -> bool {
    match flag_path() {
        Some(p) => !p.is_file(),
        None => true,
    }
}

/// Write the marker file. Silent no-op on any io error; the caller just sees
/// a repeat of the overlay next launch.
pub fn mark_seen() {
    let Some(path) = flag_path() else {
        tracing::warn!("first_run: config dir unavailable, welcome will repeat");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(error = %e, "first_run: create_dir_all failed");
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, b"seen\n") {
        tracing::warn!(path = %path.display(), error = %e, "first_run: write failed");
    }
}
