//! Single-instance lock — refuse to start a second copy of the GUI.
//!
//! The lock is platform-aware courtesy of `single-instance`:
//!   - **Windows**: named mutex under `Local\\GADARAH-GUI`.
//!   - **Linux**: abstract Unix socket name `gadarah-gui`.
//!   - **macOS**: same abstract-socket trick (works in /tmp).
//!
//! On hit, we surface a desktop notification ("GADARAH is already running")
//! so the user knows why their click did nothing, then exit. Failing
//! "open" — if the OS API is unavailable for any reason — we let the app
//! start anyway rather than block launch.

use single_instance::SingleInstance;

const APP_KEY: &str = "GADARAH-GUI-2.x";

/// Held for the lifetime of the running GUI. Drop releases the OS handle.
pub struct InstanceLock {
    _handle: SingleInstance,
}

/// Outcome of the startup check.
pub enum InstanceCheck {
    /// We're the only instance. Hold the returned lock for the life of the
    /// process; dropping it releases the OS handle.
    First(InstanceLock),
    /// Another GADARAH GUI is already running. Caller should exit.
    AlreadyRunning,
    /// The OS API failed (no permission, no abstract namespace, etc.).
    /// Caller should proceed without enforcing the lock.
    Unavailable(String),
}

/// Probe the lock. Call from `main()` before `eframe::run_native`.
pub fn check() -> InstanceCheck {
    match SingleInstance::new(APP_KEY) {
        Ok(handle) => {
            if handle.is_single() {
                InstanceCheck::First(InstanceLock { _handle: handle })
            } else {
                InstanceCheck::AlreadyRunning
            }
        }
        Err(e) => InstanceCheck::Unavailable(e.to_string()),
    }
}

/// Fire a brief OS toast telling the user another GADARAH is already
/// running. Best-effort; failures are silent because we're about to exit
/// anyway and there's no UI to surface them on.
pub fn notify_already_running() {
    let mut n = notify_rust::Notification::new();
    n.summary("GADARAH is already running")
        .body("Use the running window or quit it before starting another instance.")
        .appname("GADARAH");
    let _ = n.show();
}
