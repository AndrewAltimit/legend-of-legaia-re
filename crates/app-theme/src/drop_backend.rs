//! Which windowing backend a desktop program opens on so a dropped file
//! reaches it.
//!
//! winit 0.30 delivers `DroppedFile` on X11, Windows and macOS but has no
//! drag-and-drop on Wayland at all, so a file dropped on a native Wayland
//! window is silently ignored. The launcher and Legaia Explorer both
//! advertise a drop, and neither needs anything Wayland-specific, so on a
//! Wayland session that also offers XWayland (`DISPLAY` set) each opens as
//! an X11 client instead: the compositor bridges a drag from a Wayland file
//! manager into XWayland, and the drop arrives. Each program has its own
//! opt-out variable that keeps it native (no drop; the file dialog and the
//! typed path still work): [`LAUNCHER_KEEP_WAYLAND`] and
//! [`EXPLORER_KEEP_WAYLAND`].

use std::ffi::OsStr;

/// Keeps the game launcher on native Wayland.
pub const LAUNCHER_KEEP_WAYLAND: &str = "LEGAIA_LAUNCHER_WAYLAND";

/// Keeps Legaia Explorer on native Wayland.
pub const EXPLORER_KEEP_WAYLAND: &str = "LEGAIA_EXPLORER_WAYLAND";

/// Whether to open through XWayland: a Wayland session (`wayland`) that
/// also offers X11 (`x11`), with the program's opt-out unset.
pub fn prefer_x11_for_drop(wayland: bool, x11: bool, keep_wayland: bool) -> bool {
    wayland && x11 && !keep_wayland
}

/// [`prefer_x11_for_drop`] read off this process's environment:
/// `WAYLAND_DISPLAY` and `DISPLAY` non-empty, and `keep_wayland_var` (one of
/// the opt-outs above) unset.
pub fn prefer_x11_for_drop_from_env(keep_wayland_var: &str) -> bool {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| v != OsStr::new(""));
    prefer_x11_for_drop(
        set("WAYLAND_DISPLAY"),
        set("DISPLAY"),
        std::env::var_os(keep_wayland_var).is_some(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xwayland_only_on_a_wayland_session_with_x11() {
        assert!(prefer_x11_for_drop(true, true, false));
        assert!(!prefer_x11_for_drop(true, true, true), "opt-out honoured");
        assert!(!prefer_x11_for_drop(true, false, false), "no XWayland");
        assert!(
            !prefer_x11_for_drop(false, true, false),
            "plain X11 already drops"
        );
    }
}
