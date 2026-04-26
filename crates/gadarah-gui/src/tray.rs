//! System tray icon — Phase 2 C.
//!
//! Renders a small forge-gold icon in the OS notification area with a
//! right-click menu (Show / Hide / Quit). Window-close on the main GUI
//! becomes a hide-to-tray event when the tray is active, so the GUI no
//! longer feels like a foreground-only tool.
//!
//! ## Platform scope
//!
//! Windows-only for now. Linux requires a gtk main loop on a dedicated
//! thread and tray support across Wayland compositors is patchy enough
//! that shipping it would create more bug reports than UX wins. macOS
//! could be added later via the same `tray-icon` crate; demand-driven.
//!
//! Linux users get the single-instance lock (Phase 2 D) without tray —
//! the rest of the app's integration features still apply.

#![cfg(windows)]

use std::sync::mpsc::{channel, Receiver, Sender};

use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

/// Events the tray emits up to the main app.
#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    /// "Show GADARAH" clicked or icon double-clicked.
    Show,
    /// "Hide" clicked.
    Hide,
    /// "Quit" clicked — main app should exit cleanly.
    Quit,
}

/// Held by the main app. Drop releases the OS handle.
pub struct TrayHandle {
    _tray: TrayIcon,
    rx: Receiver<TrayEvent>,
    show_id: tray_icon::menu::MenuId,
    hide_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
}

impl TrayHandle {
    /// Drain pending events. Caller polls once per frame.
    pub fn drain(&self) -> Vec<TrayEvent> {
        // Tray + menu events are routed through the global receivers
        // that tray-icon installs at process scope. We re-route them
        // through our own channel via the closures registered in `spawn`,
        // so all the consumer needs to do is drain that channel.
        let mut out = Vec::new();
        while let Ok(evt) = self.rx.try_recv() {
            out.push(evt);
        }
        // Also drain any pending menu events that weren't routed through
        // our handler (defensive — should be empty in steady state).
        while let Ok(evt) = MenuEvent::receiver().try_recv() {
            if evt.id == self.show_id {
                out.push(TrayEvent::Show);
            } else if evt.id == self.hide_id {
                out.push(TrayEvent::Hide);
            } else if evt.id == self.quit_id {
                out.push(TrayEvent::Quit);
            }
        }
        // Tray-icon click events (single click → show).
        while let Ok(evt) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::DoubleClick { .. } = evt {
                out.push(TrayEvent::Show);
            }
        }
        out
    }
}

/// Build the tray icon. Returns `None` if the OS rejects the request
/// (e.g. no notification area available). Caller proceeds without tray
/// integration in that case.
pub fn spawn() -> Option<TrayHandle> {
    let icon = build_icon().ok()?;
    let menu = Menu::new();
    let show_item = MenuItem::new("Show GADARAH", true, None);
    let hide_item = MenuItem::new("Hide", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    menu.append_items(&[
        &show_item,
        &hide_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])
    .ok()?;
    let show_id = show_item.id().clone();
    let hide_id = hide_item.id().clone();
    let quit_id = quit_item.id().clone();

    let tray = TrayIconBuilder::new()
        .with_id("gadarah-gui")
        .with_tooltip("GADARAH")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .build()
        .ok()?;

    let (tx, rx) = channel::<TrayEvent>();
    install_event_handlers(
        tx,
        show_id.clone(),
        hide_id.clone(),
        quit_id.clone(),
    );

    Some(TrayHandle {
        _tray: tray,
        rx,
        show_id,
        hide_id,
        quit_id,
    })
}

fn install_event_handlers(
    tx: Sender<TrayEvent>,
    show_id: tray_icon::menu::MenuId,
    hide_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
) {
    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |evt: MenuEvent| {
        let mapped = if evt.id == show_id {
            TrayEvent::Show
        } else if evt.id == hide_id {
            TrayEvent::Hide
        } else if evt.id == quit_id {
            TrayEvent::Quit
        } else {
            return;
        };
        let _ = menu_tx.send(mapped);
    }));

    let tray_tx = tx;
    TrayIconEvent::set_event_handler(Some(move |evt: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { .. } = evt {
            let _ = tray_tx.send(TrayEvent::Show);
        }
    }));
}

/// Build a small RGBA icon procedurally so we don't need to ship a PNG
/// asset. 32×32 forge-gold square with a subtle dark border — readable
/// on both light and dark Windows themes.
fn build_icon() -> Result<Icon, tray_icon::BadIcon> {
    const SIZE: u32 = 32;
    const FORGE_GOLD: [u8; 4] = [212, 168, 71, 255];
    const FORGE_OBSIDIAN: [u8; 4] = [30, 31, 46, 255];
    const FORGE_CRIMSON: [u8; 4] = [139, 26, 26, 255];

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let idx = ((y * SIZE + x) * 4) as usize;
            let on_border = x == 0 || y == 0 || x == SIZE - 1 || y == SIZE - 1;
            // Draw a stylised "G" diagonal slash so it's distinguishable
            // from generic gold squares — top-left and bottom-right
            // anchors stand out.
            let on_anchor = (x < 4 && y < 4) || (x > SIZE - 5 && y > SIZE - 5);
            let chosen = if on_border {
                FORGE_OBSIDIAN
            } else if on_anchor {
                FORGE_CRIMSON
            } else {
                FORGE_GOLD
            };
            rgba[idx..idx + 4].copy_from_slice(&chosen);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE)
}
