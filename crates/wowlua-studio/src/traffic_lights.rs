//! macOS's window buttons centred in Studio's 40 pt title bar (decisions
//! S013, S022). AppKit lays them out for its own 28 pt title bar; this
//! makes the title bar's container as tall as Studio's bar and puts the
//! buttons on its centre line, again whenever AppKit moved them back
//! (resizing does). The only `unsafe` in Studio: reaching AppKit's views.
#![allow(unsafe_code)]

use eframe::wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use objc2_app_kit::{NSView, NSWindowButton, NSWindowStyleMask};
use objc2_foundation::NSPoint;

/// Studio's title bar (`chrome::TITLE_H`), in points.
const BAR: f64 = 40.0;
/// The buttons' left margin: the same as above and below them.
const LEFT: f64 = 13.0;
/// From one button's left edge to the next (AppKit's own spacing).
const STEP: f64 = 20.0;

const BUTTONS: [NSWindowButton; 3] = [
    NSWindowButton::CloseButton,
    NSWindowButton::MiniaturizeButton,
    NSWindowButton::ZoomButton,
];

/// Places the buttons if AppKit has moved them; cheap when it has not.
pub fn place(frame: &eframe::Frame) {
    let Ok(handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // SAFETY: the handle is the content view of Studio's window, alive for
    // as long as the window and used here on the main thread, where eframe
    // runs the app on macOS.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    let Some(window) = view.window() else {
        return;
    };
    if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return;
    }
    let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) else {
        return;
    };
    let target_y = (BAR - close.frame().size.height) / 2.0;
    let origin = close.frame().origin;
    if (origin.x - LEFT).abs() < 0.5 && (origin.y - target_y).abs() < 0.5 {
        return;
    }
    // The buttons sit in the title bar view, inside the title bar's
    // container view: the container gets Studio's height.
    // SAFETY: both views are retained by the returned `Retained`s and
    // belong to the window, which is alive.
    let container = unsafe { close.superview().and_then(|bar| bar.superview()) };
    if let Some(container) = container {
        let window_height = window.frame().size.height;
        let mut rect = container.frame();
        rect.size.height = BAR;
        rect.origin.y = window_height - BAR;
        container.setFrame(rect);
    }
    for (i, kind) in BUTTONS.into_iter().enumerate() {
        if let Some(button) = window.standardWindowButton(kind) {
            button.setFrameOrigin(NSPoint::new(LEFT + i as f64 * STEP, target_y));
        }
    }
}
