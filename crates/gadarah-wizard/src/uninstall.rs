//! Uninstall driver — mirrors the install worker-thread shape so the UI
//! renders identically, only the steps flip around:
//!   1. Kill any running gadarah-gui.exe / gadarah.exe.
//!   2. Delete Start Menu + Desktop shortcuts.
//!   3. Delete the HKCU Uninstall registry entry.
//!   4. Delete every file under the install dir EXCEPT the running wizard
//!      (Windows won't let us delete an executing binary). A cmd.exe shim
//!      finishes the job after the wizard exits.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::install::{expand_install_path, InstallEvent};

#[cfg(windows)]
const UNINSTALL_KEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\GADARAH";

pub struct UninstallState {
    rx: Option<Receiver<InstallEvent>>,
    pub started_at: Option<Instant>,
    pub finished: bool,
    pub error: Option<String>,
    pub progress: f32,
    pub current_step: String,
    pub log: Vec<String>,
    pub target: Option<PathBuf>,
    /// User must tick this before Start enables — stops accidental uninstalls.
    pub confirmed: bool,
    /// Preserve `.env.*` and `config/` after uninstall? Default true.
    pub keep_user_data: bool,
}

impl Default for UninstallState {
    fn default() -> Self {
        Self {
            rx: None,
            started_at: None,
            finished: false,
            error: None,
            progress: 0.0,
            current_step: "Not started".to_string(),
            log: Vec::new(),
            target: None,
            confirmed: false,
            keep_user_data: true,
        }
    }
}

impl UninstallState {
    pub fn start(&mut self, target: PathBuf) {
        if self.started_at.is_some() {
            return;
        }
        self.target = Some(target.clone());
        let (tx, rx) = channel();
        let keep = self.keep_user_data;
        if let Err(err) = thread::Builder::new()
            .name("gadarah-wizard-uninstall".into())
            .spawn(move || run_uninstall(tx, target, keep))
        {
            self.error = Some(format!("failed to spawn uninstaller thread: {err}"));
            self.finished = true;
            return;
        }
        self.rx = Some(rx);
        self.started_at = Some(Instant::now());
        self.current_step = "Starting uninstaller".to_string();
    }

    pub fn tick(&mut self) {
        let Some(rx) = self.rx.as_ref() else {
            return;
        };
        if self.finished {
            return;
        }
        loop {
            match rx.try_recv() {
                Ok(InstallEvent::Step { label, progress }) => {
                    self.current_step = label.clone();
                    self.progress = progress;
                    self.log.push(format!("[step] {label}"));
                }
                Ok(InstallEvent::Log(msg)) => self.log.push(msg),
                Ok(InstallEvent::Completed) => {
                    self.finished = true;
                    self.progress = 1.0;
                    self.current_step = "Uninstallation complete".to_string();
                    self.log.push("[done] uninstallation complete".to_string());
                }
                Ok(InstallEvent::Failed(e)) => {
                    self.finished = true;
                    self.error = Some(e.clone());
                    self.log.push(format!("[error] {e}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if !self.finished {
                        self.finished = true;
                        if self.progress < 0.999 {
                            self.error = Some(
                                "uninstaller thread exited unexpectedly".into(),
                            );
                        }
                    }
                    break;
                }
            }
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        let started = self.started_at?;
        if self.progress <= 0.02 {
            return None;
        }
        let elapsed = started.elapsed().as_secs_f32();
        let remaining = (elapsed / self.progress) - elapsed;
        Some(Duration::from_secs_f32(remaining.max(0.0)))
    }
}

fn run_uninstall(tx: Sender<InstallEvent>, target: PathBuf, keep_user_data: bool) {
    macro_rules! step {
        ($label:expr, $p:expr) => {
            if tx
                .send(InstallEvent::Step {
                    label: $label.to_string(),
                    progress: $p,
                })
                .is_err()
            {
                return;
            }
        };
    }
    macro_rules! log {
        ($msg:expr) => {{
            let _ = tx.send(InstallEvent::Log($msg.to_string()));
        }};
    }

    step!("Stopping running processes", 0.05);
    kill_running_processes(&tx);

    step!("Removing Start Menu entry", 0.25);
    match remove_start_menu_entry() {
        Ok(()) => log!("Start Menu cleared"),
        Err(e) => log!(format!("Start Menu: {e}")),
    }
    match remove_desktop_shortcut() {
        Ok(()) => log!("Desktop shortcut removed"),
        Err(e) => log!(format!("Desktop: {e}")),
    }

    step!("Removing registry entry", 0.45);
    match remove_uninstall_registry() {
        Ok(()) => log!("uninstall registry entry removed"),
        Err(e) => log!(format!("Registry: {e}")),
    }

    step!("Deleting install directory", 0.65);
    match delete_install_dir(&target, keep_user_data, &tx) {
        Ok(n) => log!(format!("{n} file(s) deleted")),
        Err(e) => {
            let _ = tx.send(InstallEvent::Failed(format!("delete install dir: {e}")));
            return;
        }
    }

    step!("Scheduling self-delete", 0.90);
    schedule_self_delete(&target, &tx);

    step!("Finalising", 1.0);
    let _ = tx.send(InstallEvent::Completed);
}

fn kill_running_processes(tx: &Sender<InstallEvent>) {
    #[cfg(windows)]
    {
        for name in ["gadarah-gui.exe", "gadarah.exe"] {
            match std::process::Command::new("taskkill")
                .args(["/F", "/T", "/IM", name])
                .output()
            {
                Ok(out) if out.status.success() => {
                    let _ = tx.send(InstallEvent::Log(format!("taskkill {name}: ok")));
                }
                Ok(out) => {
                    let _ = tx.send(InstallEvent::Log(format!(
                        "taskkill {name}: exit {} (not running is fine)",
                        out.status
                    )));
                }
                Err(e) => {
                    let _ = tx.send(InstallEvent::Log(format!("taskkill {name}: {e}")));
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = tx.send(InstallEvent::Log(
            "skip process-kill on non-Windows host".into(),
        ));
    }
}

fn remove_start_menu_entry() -> Result<(), String> {
    let Some(sm) = start_menu_programs_dir() else {
        return Err("Start Menu dir unavailable".into());
    };
    let folder = sm.join("GADARAH");
    if folder.is_dir() {
        std::fs::remove_dir_all(&folder).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn remove_desktop_shortcut() -> Result<(), String> {
    let Some(desktop) = dirs::desktop_dir() else {
        return Err("desktop dir unavailable".into());
    };
    for name in ["GADARAH.lnk", "GADARAH.desktop"] {
        let p = desktop.join(name);
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn remove_uninstall_registry() -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match hkcu.delete_subkey_all(UNINSTALL_KEY) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("delete subkey: {e}")),
    }
}

#[cfg(not(windows))]
fn remove_uninstall_registry() -> Result<(), String> {
    Ok(())
}

fn start_menu_programs_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        dirs::data_dir().map(|d| d.join("Microsoft/Windows/Start Menu/Programs"))
    }
    #[cfg(not(windows))]
    {
        dirs::data_dir().map(|d| d.join("gadarah-wizard-dev/start-menu"))
    }
}

/// Delete everything under `target` except:
/// - the running wizard (Windows won't let us)
/// - `.env*` and `config/` entries when `keep_user_data` is true
///
/// Returns the number of files removed.
fn delete_install_dir(
    target: &Path,
    keep_user_data: bool,
    tx: &Sender<InstallEvent>,
) -> std::io::Result<usize> {
    if !target.is_dir() {
        return Ok(0);
    }
    let self_exe = std::env::current_exe().ok();
    let mut count = 0usize;
    for entry in std::fs::read_dir(target)? {
        let entry = entry?;
        let path = entry.path();

        // Never delete ourselves.
        if self_exe.as_ref().map(|p| p == &path).unwrap_or(false) {
            let _ = tx.send(InstallEvent::Log(format!(
                "keep {} (running wizard; scheduled delete)",
                path.display()
            )));
            continue;
        }

        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if keep_user_data {
            if name.starts_with(".env") || name == "config" {
                let _ = tx.send(InstallEvent::Log(format!(
                    "keep {} (user data)",
                    path.display()
                )));
                continue;
            }
        }

        let res = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match res {
            Ok(()) => count += 1,
            Err(e) => {
                let _ = tx.send(InstallEvent::Log(format!(
                    "delete {}: {e}",
                    path.display()
                )));
            }
        }
    }
    Ok(count)
}

/// Schedule a deferred self-delete so the wizard can remove itself after
/// it exits. Uses a detached cmd.exe that sleeps, deletes, and dies. On
/// non-Windows this is a no-op (dev builds aren't installed into a
/// protected location).
fn schedule_self_delete(target: &Path, tx: &Sender<InstallEvent>) {
    #[cfg(windows)]
    {
        use std::process::{Command, Stdio};
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(InstallEvent::Log(format!("self-exe path: {e}")));
                return;
            }
        };
        // `timeout /t 3` waits ~3 s (plenty for the wizard to exit); `del
        // /f /q` nukes the exe; `rd /s /q` removes the (now empty or
        // mostly-empty) install dir if `keep_user_data` pruning left it
        // otherwise empty.
        let cmd = format!(
            "timeout /t 3 /nobreak >nul & del /f /q \"{}\" & rd /s /q \"{}\" 2>nul",
            exe.display(),
            target.display(),
        );
        let spawn = Command::new("cmd")
            .args(["/C", &cmd])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawn {
            Ok(_) => {
                let _ = tx.send(InstallEvent::Log(
                    "self-delete shim spawned; wizard will remove itself after exit".into(),
                ));
            }
            Err(e) => {
                let _ = tx.send(InstallEvent::Log(format!("schedule self-delete: {e}")));
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = tx.send(InstallEvent::Log(format!(
            "dev build: skipping self-delete ({}).",
            target.display()
        )));
    }
}

/// Detect the install dir of an existing GADARAH install. Order:
/// 1. `--install-dir <path>` argv flag (escape hatch for tests).
/// 2. HKCU UninstallKey → InstallLocation.
/// 3. Default `%LOCALAPPDATA%\\GADARAH`.
pub fn detect_install_dir() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--install-dir" {
            if let Some(p) = args.get(i + 1) {
                return Some(PathBuf::from(p));
            }
        }
    }
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(UNINSTALL_KEY, KEY_READ)
        {
            if let Ok(loc) = key.get_value::<String, _>("InstallLocation") {
                return Some(PathBuf::from(loc));
            }
        }
    }
    Some(expand_install_path("%LOCALAPPDATA%\\GADARAH"))
}
