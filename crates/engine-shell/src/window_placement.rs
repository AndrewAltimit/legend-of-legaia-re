//! Where the game window and the launcher reopen: where each last closed,
//! when that is still on a connected monitor
//! ([`legaia_app_theme::placement`], the rule Explorer's window follows).
//!
//! Both positions live in one small file beside the launcher's settings,
//! [`FILE`] in [`launcher::config_dir`](crate::launcher::config_dir), each
//! program writing only its own key. They are not in `launcher.toml`: the
//! launcher stays open (hidden) while the game runs and writes its settings
//! again afterwards, which would put back a game position it read before the
//! game moved its window. A scripted run (a screenshot, a capture child)
//! neither reads nor writes the file, so a capture's window never depends on
//! where a player last left theirs.
//!
//! A fullscreen window keeps the position saved before it went fullscreen,
//! and a desktop that does not report window positions (Wayland) leaves the
//! file alone.

use std::path::PathBuf;

use legaia_app_theme::placement::{Monitor, restore_physical};
use serde::{Deserialize, Serialize};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

/// The file both positions live in.
pub const FILE: &str = "window-placement.toml";

/// Which window a position belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// The play window.
    Game,
    /// The launcher's screen.
    Launcher,
}

/// [`FILE`]'s contents: each window's outer top-left corner in physical
/// pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    game: Option<[i32; 2]>,
    launcher: Option<[i32; 2]>,
    /// The game window's inner size, logical points, when it last closed in
    /// a window (`game_size_scale` is the *Window size* row then: moving the
    /// row asks for that size instead of the dragged one).
    game_size: Option<[u32; 2]>,
    game_size_scale: Option<u8>,
    /// It closed maximized; [`Self::game_size`] is the size before.
    game_maximized: bool,
}

impl Saved {
    fn slot(&mut self, which: Which) -> &mut Option<[i32; 2]> {
        match which {
            Which::Game => &mut self.game,
            Which::Launcher => &mut self.launcher,
        }
    }
}

fn path() -> Option<PathBuf> {
    crate::launcher::config_dir().map(|d| d.join(FILE))
}

/// The file's contents; anything unreadable is no saved position (nothing in
/// it is worth setting aside).
fn load() -> Saved {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

/// The connected monitors, as the event loop reports them.
pub fn monitors(evl: &ActiveEventLoop) -> Vec<Monitor> {
    evl.available_monitors()
        .map(|m| Monitor {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
            scale: m.scale_factor(),
        })
        .collect()
}

/// Where `which` last closed, when that is still on a connected monitor.
pub fn restore(which: Which, evl: &ActiveEventLoop) -> Option<[i32; 2]> {
    restore_physical(*load().slot(which), &monitors(evl))
}

/// The smallest game window a saved size opens at: the PlayStation's own
/// 320x240 picture.
const MIN_GAME_SIZE: [u32; 2] = [320, 240];

/// The game window's last dragged size (logical points) when it still applies:
/// saved under the same *Window size* row, and held to the monitor it reopens
/// on ([`legaia_app_theme::placement::restore_size`]).
pub fn restore_game_size(
    option_scale: u8,
    position: Option<[i32; 2]>,
    evl: &ActiveEventLoop,
) -> Option<[u32; 2]> {
    let s = load();
    if s.game_size_scale != Some(option_scale) {
        return None;
    }
    legaia_app_theme::placement::restore_size(s.game_size, position, &monitors(evl), MIN_GAME_SIZE)
}

/// Whether the game window closed maximized.
pub fn game_was_maximized() -> bool {
    load().game_maximized
}

/// Remember the game window's size for the next start. A fullscreen window
/// leaves the saved one as it is, and a maximized one keeps the size it had
/// before, with the flag set.
pub fn remember_game_size(window: &Window, option_scale: u8) {
    if window.fullscreen().is_some() {
        return;
    }
    let Some(file) = path() else {
        return;
    };
    let mut saved = load();
    let maximized = window.is_maximized();
    saved.game_maximized = maximized;
    if !maximized {
        let logical = window.inner_size().to_logical::<f64>(window.scale_factor());
        saved.game_size = Some([logical.width.round() as u32, logical.height.round() as u32]);
        saved.game_size_scale = Some(option_scale);
    }
    let Ok(text) = toml::to_string(&saved) else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = legaia_app_theme::settings_file::write_atomic(&file, text.as_bytes()) {
        log::warn!("{}: {e}", file.display());
    }
}

/// Remember where `window` is as `which`'s position. A fullscreen window, or
/// a desktop that does not report positions, leaves the saved one as it is.
pub fn remember(which: Which, window: &Window) {
    if window.fullscreen().is_some() {
        return;
    }
    let Ok(at) = window.outer_position() else {
        return;
    };
    let Some(file) = path() else {
        return;
    };
    let mut saved = load();
    let pos = Some([at.x, at.y]);
    if *saved.slot(which) == pos {
        return;
    }
    *saved.slot(which) = pos;
    let Ok(text) = toml::to_string(&saved) else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = legaia_app_theme::settings_file::write_atomic(&file, text.as_bytes()) {
        log::warn!("{}: {e}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_window_keeps_its_own_key() {
        let mut s = Saved::default();
        *s.slot(Which::Game) = Some([10, 20]);
        let text = toml::to_string(&s).unwrap();
        let back: Saved = toml::from_str(&text).unwrap();
        assert_eq!(back.game, Some([10, 20]));
        assert_eq!(back.launcher, None);
        // A file written by a build that knew only one key still reads.
        let one: Saved = toml::from_str("launcher = [5, 6]\n").unwrap();
        assert_eq!(one.launcher, Some([5, 6]));
        assert_eq!(one.game, None);
    }
}
