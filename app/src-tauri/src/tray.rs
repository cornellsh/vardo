//! The system tray.
//!
//! A password manager is used many times a day for a few seconds at a time.
//! Quitting between uses means re-typing the master password and waiting for
//! Argon2, which is deliberately slow - about a second on this machine and
//! nearer three on older hardware. So closing the window hides it, the way
//! KeePassXC and every other manager in this category behaves.
//!
//! # The part that needs care
//!
//! A vault sitting unlocked behind a hidden window is a vault left open, and
//! the person who left it that way has no window to remind them. That is the
//! whole risk this file introduces, and the mitigation is not subtle: **the
//! tray icon shows the lock state**, in a form you can tell apart at a glance
//! rather than a tint someone has to squint at.
//!
//! Auto-lock keeps running while hidden, because the timer lives in the
//! frontend and the frontend keeps running. Hiding does not lock by itself -
//! that would defeat the point of staying resident - but the menu offers it in
//! one click, and the icon never lies about which state you are in.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Drawn rather than loaded, so the two states cannot drift apart and so the
/// difference between them is a property of this file rather than of two PNGs
/// somebody has to remember to keep in step.
///
/// Locked is the mark in outline. Unlocked fills it and adds a dot, which
/// reads at 22px where a colour change alone does not - tray themes vary and a
/// user may have inverted icons.
fn icon(unlocked: bool) -> Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    // The wagon: an arch with eaves, matching the app mark. Rasterised by hand
    // because pulling a renderer in for a 32-pixel square would be the
    // heaviest dependency in the tree.
    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            let in_body = (12.0..=26.0).contains(&fy) && (6.0..=26.0).contains(&fx);
            let in_roof = {
                let dx = fx - 16.0;
                let dy = fy - 13.0;
                dy <= 0.0 && (dx * dx + dy * dy).sqrt() <= 10.0
            };
            let in_eaves = (11.0..=13.0).contains(&fy) && (4.0..=28.0).contains(&fx);
            let in_door = (13.0..=19.0).contains(&fx) && (19.0..=26.0).contains(&fy);

            let solid = (in_body || in_roof || in_eaves) && !in_door;
            if !solid {
                continue;
            }

            // Outline when locked: keep only pixels near the edge of the shape.
            let draw = if unlocked {
                true
            } else {
                fy <= 13.5
                    || fy >= 24.5
                    || fx <= 7.5
                    || fx >= 24.5
                    || in_eaves
                    || (fx - 16.0).abs().max((fy - 13.0).abs()) >= 8.5
                    || ((12.0..=20.0).contains(&fx) && fy >= 18.0)
            };
            if !draw {
                continue;
            }

            let i = ((y * SIZE + x) * 4) as usize;
            rgba[i] = 0xff;
            rgba[i + 1] = 0xff;
            rgba[i + 2] = 0xff;
            rgba[i + 3] = 0xff;
        }
    }

    Image::new_owned(rgba, SIZE, SIZE)
}

/// What the tray currently claims. Kept so the icon and tooltip are updated
/// together and cannot disagree.
pub fn set_unlocked<R: Runtime>(app: &AppHandle<R>, unlocked: bool) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let _ = tray.set_icon(Some(icon(unlocked)));
    let _ = tray.set_tooltip(Some(if unlocked {
        // Says so plainly. A tooltip reading only "Vardo" would leave the
        // state to the icon alone, and icon themes are not always legible.
        "Vardo - unlocked"
    } else {
        "Vardo - locked"
    }));
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let show = MenuItem::with_id(app, "show", "Show Vardo", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "Lock now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &lock, &sep, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(icon(false))
        .tooltip("Vardo - locked")
        .menu(&menu)
        // False, or a left click opens the menu on some desktops and does
        // nothing on others. The click is handled below so one click always
        // shows the window, which is what people expect of a tray icon.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal(app),
            "lock" => {
                // Locking is the frontend's job: it owns the auto-lock timer
                // and the unsaved-changes rule, and a second path that dropped
                // the session from here could throw away an unsaved edit that
                // the window is in the middle of writing.
                let _ = app.emit_to("main", "vault://lock-requested", ());
                reveal(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button, .. } = event {
                if button == tauri::tray::MouseButton::Left {
                    reveal(tray.app_handle());
                }
            }
        })
        .build(app)
}

/// Shows the window and puts it in front.
///
/// `unminimize` before `show`: a window hidden while minimised comes back
/// minimised on some window managers, which looks exactly like the tray icon
/// not working.
pub(crate) fn reveal<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
