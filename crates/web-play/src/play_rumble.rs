//! Controller rumble on the play page: the session's
//! `BootSession::rumble_frame` (the native window's gilrs force-feedback
//! source too), served to the page's Gamepad API vibration call
//! (`legaiaSetRumble` in `site/js/pad-bindings.js`).

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// `[strong, weak]` motor levels in `0.0..=1.0` the frame's last tick
    /// left; `[0, 0]` with no scene loaded.
    pub fn play_rumble_levels(&self) -> Vec<f32> {
        let f = self
            .scene_host
            .session()
            .map(|s| s.rumble_frame())
            .unwrap_or_default();
        vec![f.strong, f.weak]
    }
}

#[cfg(test)]
mod tests {
    /// The page hands the levels to the controller after the frame's ticks.
    #[test]
    fn the_page_drives_the_controller_motors() {
        let frame = include_str!("../../../site/js/play-view-frame.js");
        assert!(frame.contains("play_rumble_levels"));
        let pad = include_str!("../../../site/js/pad-bindings.js");
        assert!(pad.contains("window.legaiaSetRumble") && pad.contains("vibrationActuator"));
    }

    #[test]
    fn no_scene_is_no_rumble() {
        let rt = crate::runtime::LegaiaRuntime::new();
        assert_eq!(rt.play_rumble_levels(), vec![0.0, 0.0]);
    }
}
