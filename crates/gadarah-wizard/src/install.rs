//! Real installation driver.
//!
//! A worker thread performs the install and streams progress events back
//! to the UI over an `mpsc` channel. Each step is a real operation:
//! create directory → extract the embedded payload zip → write Start
//! Menu / Desktop shortcuts → write uninstall registry. On non-Windows
//! the shortcut and registry steps become explicit no-ops with a log
//! note, so the code path still exercises on Linux during development.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::tabs::components::ComponentSelection;

/// App payload embedded at compile time. Populated by `build.rs` from the
/// path given in `GADARAH_WIZARD_PAYLOAD`; empty-zip sentinel otherwise.
const PAYLOAD_ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zip"));

/// Name of the Start Menu folder we create under the user's Programs tree.
const START_MENU_FOLDER: &str = "GADARAH";
/// ARP registry key under HKCU.
#[cfg(windows)]
const UNINSTALL_KEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\GADARAH";
/// ARP display name written into the registry.
#[cfg(windows)]
const UNINSTALL_DISPLAY_NAME: &str = "GADARAH";

#[derive(Debug, Clone)]
pub enum InstallEvent {
    /// Forward progress. `progress` is in [0.0, 1.0].
    Step { label: String, progress: f32 },
    /// A free-form line appended to the log panel.
    Log(String),
    /// Terminal success event.
    Completed,
    /// Terminal failure event. UI surfaces the message in red.
    Failed(String),
}

pub struct InstallState {
    rx: Option<Receiver<InstallEvent>>,
    pub started_at: Option<Instant>,
    pub finished: bool,
    pub error: Option<String>,
    /// [0.0, 1.0]
    pub progress: f32,
    pub current_step: String,
    pub log: Vec<String>,
    /// Target install path, captured from components at start().
    pub target: Option<PathBuf>,
}

impl Default for InstallState {
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
        }
    }
}

impl InstallState {
    /// Spawn the worker thread. Subsequent calls are no-ops.
    pub fn start(&mut self, components: &ComponentSelection) {
        if self.started_at.is_some() {
            return;
        }
        let target = expand_install_path(&components.install_path);
        self.target = Some(target.clone());
        self.log.push(format!(
            "[wizard] installing to {}",
            target.display()
        ));
        let (tx, rx) = channel();
        let components = components.clone();
        let target_for_thread = target.clone();
        if let Err(err) = thread::Builder::new()
            .name("gadarah-wizard-install".into())
            .spawn(move || run_install(tx, components, target_for_thread))
        {
            self.error = Some(format!("failed to spawn installer thread: {err}"));
            self.finished = true;
            return;
        }
        self.rx = Some(rx);
        self.started_at = Some(Instant::now());
        self.current_step = "Starting installer".to_string();
    }

    /// Drain any queued events from the worker. Safe to call every frame.
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
                    self.current_step = "Installation complete".to_string();
                    self.log.push("[done] installation complete".to_string());
                }
                Ok(InstallEvent::Failed(e)) => {
                    self.finished = true;
                    self.error = Some(e.clone());
                    self.log.push(format!("[error] {e}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Worker thread finished without a Completed/Failed event.
                    // Treat as success only if we actually saw 100% progress.
                    if !self.finished {
                        if self.progress >= 0.999 {
                            self.finished = true;
                            self.current_step = "Installation complete".to_string();
                        } else {
                            self.finished = true;
                            self.error =
                                Some("installer thread exited unexpectedly".to_string());
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

fn run_install(tx: Sender<InstallEvent>, components: ComponentSelection, target: PathBuf) {
    macro_rules! step {
        ($tx:expr, $label:expr, $p:expr) => {
            if $tx
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
        ($tx:expr, $msg:expr) => {{
            let _ = $tx.send(InstallEvent::Log($msg.to_string()));
        }};
    }

    step!(tx, "Preparing destination", 0.05);
    if let Err(e) = std::fs::create_dir_all(&target) {
        let _ = tx.send(InstallEvent::Failed(format!(
            "create_dir_all {}: {e}",
            target.display()
        )));
        return;
    }

    step!(tx, "Extracting app payload", 0.20);
    match extract_payload(&target) {
        Ok(0) => {
            log!(tx, "payload is empty (dev build) — no files extracted");
        }
        Ok(n) => {
            log!(tx, format!("extracted {n} file(s) into install directory"));
        }
        Err(e) => {
            let _ = tx.send(InstallEvent::Failed(format!("extract: {e}")));
            return;
        }
    }

    step!(tx, "Writing Start Menu shortcut", 0.55);
    match write_start_menu_shortcut(&target) {
        Ok(path) => log!(tx, format!("start menu entry: {}", path.display())),
        Err(e) => log!(tx, format!("shortcut skipped: {e}")),
    }

    if components.create_desktop_shortcut {
        step!(tx, "Writing Desktop shortcut", 0.70);
        match write_desktop_shortcut(&target) {
            Ok(path) => log!(tx, format!("desktop shortcut: {}", path.display())),
            Err(e) => log!(tx, format!("desktop shortcut skipped: {e}")),
        }
    }

    step!(tx, "Recording uninstall metadata", 0.85);
    match write_uninstall_metadata(&target) {
        Ok(()) => log!(tx, "uninstall metadata written"),
        Err(e) => log!(tx, format!("uninstall metadata skipped: {e}")),
    }

    if components.install_ollama {
        step!(tx, "Installing Ollama + DeepSeek R1 1.5B", 0.92);
        match run_ollama_installer(&target, &tx) {
            Ok(()) => log!(tx, "Ollama install step finished"),
            Err(e) => log!(tx, format!("Ollama install failed: {e}")),
        }
    }

    step!(tx, "Finalising", 1.00);
    let _ = tx.send(InstallEvent::Completed);
}

/// Resolve Windows-style env placeholders (%LOCALAPPDATA%, %APPDATA%) and
/// `~` into real paths. Falls back to returning the input unchanged if no
/// placeholder matches.
pub fn expand_install_path(raw: &str) -> PathBuf {
    let mut path = raw.to_string();
    if let Some(home) = dirs::home_dir() {
        path = path.replace('~', &home.display().to_string());
    }
    if let Some(local) = dirs::data_local_dir() {
        path = path.replace("%LOCALAPPDATA%", &local.display().to_string());
    }
    if let Some(app) = dirs::data_dir() {
        path = path.replace("%APPDATA%", &app.display().to_string());
    }
    PathBuf::from(path)
}

fn extract_payload(target: &Path) -> std::io::Result<usize> {
    if PAYLOAD_ZIP.len() <= 22 {
        // Empty-zip sentinel — nothing to extract.
        return Ok(0);
    }
    let reader = Cursor::new(PAYLOAD_ZIP);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut n = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let Some(relative) = entry.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        let out = target.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::create(&out)?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::io::Write::write_all(&mut f, &buf)?;
        n += 1;
    }
    Ok(n)
}

fn write_start_menu_shortcut(target: &Path) -> Result<PathBuf, String> {
    let Some(start_menu) = start_menu_programs_dir() else {
        return Err("Start Menu location unavailable on this platform".into());
    };
    let folder = start_menu.join(START_MENU_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let lnk_path = folder.join(shortcut_file_name());
    let exe = target.join(gui_executable_name());
    write_lnk(&exe, &lnk_path, target)?;
    Ok(lnk_path)
}

fn write_desktop_shortcut(target: &Path) -> Result<PathBuf, String> {
    let desktop = dirs::desktop_dir().ok_or_else(|| "desktop dir unavailable".to_string())?;
    let lnk_path = desktop.join(shortcut_file_name());
    let exe = target.join(gui_executable_name());
    write_lnk(&exe, &lnk_path, target)?;
    Ok(lnk_path)
}

#[cfg(windows)]
fn write_lnk(target_exe: &Path, shortcut_path: &Path, working_dir: &Path) -> Result<(), String> {
    let mut sl = mslnk::ShellLink::new(target_exe).map_err(|e| format!("mslnk::new: {e}"))?;
    sl.set_working_dir(Some(working_dir.display().to_string()));
    sl.set_name(Some("GADARAH".into()));
    sl.create_lnk(shortcut_path)
        .map_err(|e| format!("create_lnk: {e}"))
}

#[cfg(not(windows))]
fn write_lnk(target_exe: &Path, shortcut_path: &Path, _working_dir: &Path) -> Result<(), String> {
    // Non-Windows fallback: write a POSIX-style `.desktop` entry (or plain
    // text file under a dev-only tree) that records the launcher metadata.
    // Real shortcut creation only makes sense on Windows; this exists so the
    // install flow can be exercised end-to-end on Linux during development.
    let contents = format!(
        "[Desktop Entry]\nType=Application\nName=GADARAH\nExec={}\n",
        target_exe.display(),
    );
    std::fs::write(shortcut_path, contents).map_err(|e| e.to_string())
}

fn shortcut_file_name() -> &'static str {
    if cfg!(windows) {
        "GADARAH.lnk"
    } else {
        "GADARAH.desktop"
    }
}

fn start_menu_programs_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        // %APPDATA%\Microsoft\Windows\Start Menu\Programs — the per-user path
        // that doesn't need admin rights.
        dirs::data_dir().map(|d| d.join("Microsoft/Windows/Start Menu/Programs"))
    }
    #[cfg(not(windows))]
    {
        // Mirror the structure under the user's data dir so dev builds can
        // still exercise the write path.
        dirs::data_dir().map(|d| d.join("gadarah-wizard-dev/start-menu"))
    }
}

fn gui_executable_name() -> &'static str {
    if cfg!(windows) {
        "gadarah-gui.exe"
    } else {
        "gadarah-gui"
    }
}

#[cfg(windows)]
fn write_uninstall_metadata(target: &Path) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey_with_flags(UNINSTALL_KEY, KEY_WRITE)
        .map_err(|e| format!("open uninstall key: {e}"))?;
    let install_dir = target.display().to_string();
    let uninstaller = target.join("gadarah-uninstall.exe").display().to_string();
    key.set_value("DisplayName", &UNINSTALL_DISPLAY_NAME.to_string())
        .map_err(|e| format!("write DisplayName: {e}"))?;
    key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION").to_string())
        .map_err(|e| format!("write DisplayVersion: {e}"))?;
    key.set_value("Publisher", &"GADARAH".to_string())
        .map_err(|e| format!("write Publisher: {e}"))?;
    key.set_value("InstallLocation", &install_dir)
        .map_err(|e| format!("write InstallLocation: {e}"))?;
    key.set_value("UninstallString", &uninstaller)
        .map_err(|e| format!("write UninstallString: {e}"))?;
    key.set_value("NoModify", &1u32)
        .map_err(|e| format!("write NoModify: {e}"))?;
    key.set_value("NoRepair", &1u32)
        .map_err(|e| format!("write NoRepair: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn write_uninstall_metadata(target: &Path) -> Result<(), String> {
    // Non-Windows dev builds: drop a marker file so the code path is
    // exercised and the log shows what would be written.
    let marker = target.join("uninstall.meta");
    let content = format!(
        "DisplayName=GADARAH\nInstallLocation={}\nVersion={}\n",
        target.display(),
        env!("CARGO_PKG_VERSION"),
    );
    std::fs::write(&marker, content).map_err(|e| e.to_string())?;
    Ok(())
}

/// On Windows, spawn `powershell -ExecutionPolicy Bypass -File install_ollama.ps1`
/// from the install directory (where the payload has just been extracted). The
/// script downloads OllamaSetup.exe, runs the silent installer, waits for the
/// local API at 127.0.0.1:11434 to respond, and pulls `deepseek-r1:1.5b`. We
/// stream stdout lines as InstallEvent::Log so the wizard's log panel shows
/// real progress from the installer.
///
/// On non-Windows hosts the script is a PowerShell file and there's no
/// runtime to execute it — log the situation and bail cleanly.
fn run_ollama_installer(target: &Path, tx: &Sender<InstallEvent>) -> Result<(), String> {
    let script = target.join("install_ollama.ps1");
    if !script.is_file() {
        return Err(format!(
            "install_ollama.ps1 not found at {} — payload missing the script",
            script.display()
        ));
    }
    #[cfg(windows)]
    {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let mut child = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script.display().to_string(),
            ])
            .current_dir(target)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn powershell: {e}"))?;
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = tx.send(InstallEvent::Log(format!("[ollama] {line}")));
                }
            });
        }
        if let Some(err_pipe) = child.stderr.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err_pipe).lines().map_while(Result::ok) {
                    let _ = tx.send(InstallEvent::Log(format!("[ollama!] {line}")));
                }
            });
        }
        let status = child
            .wait()
            .map_err(|e| format!("wait powershell: {e}"))?;
        if !status.success() {
            return Err(format!(
                "install_ollama.ps1 exited with status {status}"
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = tx;
        let _ = script;
        let _ = target;
        let _ = tx.send(InstallEvent::Log(
            "install_ollama.ps1 skipped: only Windows hosts can run the installer".into(),
        ));
        Ok(())
    }
}
