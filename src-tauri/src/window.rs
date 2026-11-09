//! The window's place and size: put back at start, and kept in the settings when they change.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow, Window,
};

use crate::settings::{Saved, WindowState};

/// How long the window must stay still before its place and size are kept. While it is being maximized it moves and changes
/// size a moment before it says it is maximized, so reading it at once would keep the wrong size.
const SETTLE: Duration = Duration::from_millis(500);

/// The smallest the window can be with 13 px menus, the same as in tauri.conf.json.
const MIN_WIDTH: f64 = 640.0;
const MIN_HEIGHT: f64 = 400.0;

/// The menu text size the window's styles are written for: at it, nothing is zoomed.
const MENU_FONT_SIZE_DRAWN: f64 = 13.0;

/// Zooms the window so its menus, tabs, buttons and dialogs have text of
/// `menu_font_size` pixels; the UI makes the views' text smaller by as much,
/// so their text size stays as set. The smallest the window can be grows with
/// the zoom, so the layout keeps the room it needs, as far as the screen allows.
pub fn set_menu_font_size(window: &WebviewWindow, menu_font_size: u8) {
    let zoom = f64::from(menu_font_size) / MENU_FONT_SIZE_DRAWN;
    let _ = window.set_zoom(zoom);

    let mut width = MIN_WIDTH * zoom;
    let mut height = MIN_HEIGHT * zoom;

    if let Ok(Some(screen)) = window.current_monitor() {
        let area: LogicalSize<f64> = screen.work_area().size.to_logical(screen.scale_factor());
        width = width.min(area.width);
        height = height.min(area.height);
    }

    let _ = window.set_min_size(Some(LogicalSize::new(width, height)));
}

/// When the window last moved or changed size, while that is not kept yet.
#[derive(Default)]
pub struct Unsaved {
    changed: Mutex<Option<Instant>>,
    /// Set once the window has been put in place at start.
    restored: AtomicBool,
}

/// Whether a place is still on a screen, with enough of the window there to take hold of.
fn reachable(window: &WebviewWindow, state: &WindowState) -> bool {
    window.available_monitors().is_ok_and(|monitors| {
        monitors.iter().any(|monitor| {
            let area = monitor.work_area();
            let left = area.position.x;
            let top = area.position.y;
            let right = left + i32::try_from(area.size.width).unwrap_or(i32::MAX);
            let bottom = top + i32::try_from(area.size.height).unwrap_or(i32::MAX);

            state.x + 100 > left
                && state.x < right - 100
                && state.y >= top - 8
                && state.y < bottom - 50
        })
    })
}

/// Puts the window where it was, if that is still on a screen, then shows it. The window starts hidden, so it never jumps.
pub fn restore(window: &WebviewWindow, saved: Option<WindowState>) {
    if let Some(state) = saved {
        let _ = window.set_size(PhysicalSize::new(state.width, state.height));

        if reachable(window, &state) {
            let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
        } else {
            // Its screen is gone: the middle of the main screen instead.
            let _ = window.center();
        }

        if state.maximized {
            let _ = window.maximize();
        }
    }

    let _ = window.show();

    window
        .state::<Unsaved>()
        .restored
        .store(true, Ordering::Relaxed);
}

/// Notes that the window moved or changed size. It is kept once it settles.
pub fn changed<R: Runtime>(window: &Window<R>) {
    let unsaved = window.state::<Unsaved>();

    // The moves of putting it in place at start are not the user's.
    if unsaved.restored.load(Ordering::Relaxed) {
        *unsaved
            .changed
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    }
}

/// Keeps the window's place and size once it has been still for a moment, checking four times a second.
pub fn keep_saving(app: AppHandle) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(250));

            let settled = {
                let unsaved = app.state::<Unsaved>();
                let mut changed = unsaved
                    .changed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);

                match *changed {
                    Some(at) if at.elapsed() >= SETTLE => changed.take().is_some(),
                    _ => false,
                }
            };

            if settled && let Some(window) = app.get_webview_window("main") {
                keep(&window.as_ref().window());
            }
        }
    });
}

/// Keeps the window's place and size now, as when it closes.
pub fn keep<R: Runtime>(window: &Window<R>) {
    // A minimized window reports a place far off every screen.
    if window.is_minimized().unwrap_or(false) {
        return;
    }

    let maximized = window.is_maximized().unwrap_or(false);
    let now = place_and_size(window);
    let file = window.state::<Saved>();

    let _ = file.change(|settings| {
        settings.window = match (maximized, settings.window) {
            // Maximized: keep the place and size it goes back to.
            (true, Some(before)) => Some(WindowState {
                maximized: true,
                ..before
            }),
            (true, None) => now.map(|now| WindowState {
                maximized: true,
                ..now
            }),
            (false, _) => now.or(settings.window),
        };
    });
}

fn place_and_size<R: Runtime>(window: &Window<R>) -> Option<WindowState> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;

    Some(WindowState {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: false,
    })
}
