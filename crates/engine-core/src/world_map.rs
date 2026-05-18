//! World-map controller - camera state and top-view debug toggle.
//!
//! PORT: FUN_801E76D4
//!
//! Mirrors the globals and input logic documented from `FUN_801E76D4`
//! (overlay_world_map.bin). One instance lives on [`crate::world::World`]
//! when `SceneMode::WorldMap` is active.
//!
//! ## Camera state
//!
//! | Field | Retail global | Notes |
//! |---|---|---|
//! | `view_mode` | `DAT_801F2B94` | `0` = normal walk, `1` = top-view debug |
//! | `anim_flags` | `DAT_801F2B95` | bit 0 = anim-A enable, bit 1 = anim-B |
//! | `camera_x` | `_DAT_80089120` | top-view X scroll; ±8 per D-pad frame |
//! | `camera_z` | `_DAT_80089118` | top-view Z scroll; ±8 per D-pad frame |
//! | `azimuth` | `_DAT_8007B794` | top-view rotation; ±0x14 per frame |
//! | `zoom` | `_DAT_8007B6F4` | top-view height; ±4 per frame |
//!
//! ## Top-view debug toggle
//!
//! Fires when `debug_enabled` is `true`, `pad_current & 0x4A == 0x4A`, and
//! `pad_held & 0x40 != 0`. Flips `view_mode` between 0 and 1.
//!
//! ## Source
//!
//! `ghidra/scripts/funcs/801e76d4.txt` (decompiled from `overlay_world_map.bin`).

/// Top-view camera control - live when `view_mode != 0`.
///
/// Pad bit masks mirror `FUN_801E76D4`'s literal constants.
const CAM_X_DEC: u16 = 0x1000; // left
const CAM_X_INC: u16 = 0x4000; // right
const CAM_Z_DEC: u16 = 0x2000; // up
const CAM_Z_INC: u16 = 0x8000; // down
const AZ_INC: u16 = 0x0020; // L1 → clockwise
const AZ_DEC: u16 = 0x0080; // R1 → counter-clockwise
const ZOOM_DEC: u16 = 0x0008; // zoom out (height -)
const ZOOM_INC: u16 = 0x0002; // zoom in  (height +)

/// Toggle combo: both buttons held simultaneously.
const TOGGLE_MASK: u16 = 0x4A;
/// Additional held guard for the toggle.
const TOGGLE_HELD: u16 = 0x40;

/// World-map controller state. Attach to [`crate::world::World`] when the
/// scene mode is `SceneMode::WorldMap`.
#[derive(Debug, Clone, Default)]
pub struct WorldMapController {
    /// View mode: `0` = normal walk, `1` = top-view debug (`DAT_801F2B94`).
    pub view_mode: u8,
    /// Top-view animation enable bits: bit 0 = anim-A, bit 1 = anim-B
    /// (`DAT_801F2B95`).
    pub anim_flags: u8,
    /// Top-view camera X scroll (`_DAT_80089120`).
    pub camera_x: i32,
    /// Top-view camera Z scroll (`_DAT_80089118`).
    pub camera_z: i32,
    /// Top-view camera azimuth (`_DAT_8007B794`).
    pub azimuth: i32,
    /// Top-view zoom / height (`_DAT_8007B6F4`).
    pub zoom: i32,
    /// When `true` the debug toggle combo (`_DAT_8007B98C != 0`) is enabled.
    pub debug_enabled: bool,
}

impl WorldMapController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tick one frame. `pad_current` is the full 16-bit pad word for this
    /// frame; `pad_held` is the bits that are newly pressed (not held from
    /// the previous frame - equivalent to the retail `_DAT_8007B874` mask).
    ///
    /// Returns `true` if the view mode was toggled this frame.
    pub fn tick(&mut self, pad_current: u16, pad_held: u16) -> bool {
        let mut toggled = false;

        // Top-view debug toggle (only when debug_enabled).
        if self.debug_enabled
            && (pad_current & TOGGLE_MASK == TOGGLE_MASK)
            && (pad_held & TOGGLE_HELD != 0)
        {
            self.view_mode ^= 1;
            toggled = true;
        }

        // Top-view camera controls - only active when not in normal-walk mode.
        if self.view_mode != 0 {
            if pad_current & CAM_X_DEC != 0 {
                self.camera_x -= 8;
            }
            if pad_current & CAM_X_INC != 0 {
                self.camera_x += 8;
            }
            if pad_current & CAM_Z_DEC != 0 {
                self.camera_z -= 8;
            }
            if pad_current & CAM_Z_INC != 0 {
                self.camera_z += 8;
            }
            if pad_current & AZ_INC != 0 {
                self.azimuth += 0x14;
            }
            if pad_current & AZ_DEC != 0 {
                self.azimuth -= 0x14;
            }
            if pad_current & ZOOM_DEC != 0 {
                self.zoom -= 4;
            }
            if pad_current & ZOOM_INC != 0 {
                self.zoom += 4;
            }
        }

        toggled
    }

    /// Returns `true` if the top-view debug overlay is active.
    pub fn is_top_view(&self) -> bool {
        self.view_mode != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_walk_mode() {
        let ctrl = WorldMapController::new();
        assert_eq!(ctrl.view_mode, 0);
        assert!(!ctrl.is_top_view());
    }

    #[test]
    fn debug_toggle_flips_view_mode() {
        let mut ctrl = WorldMapController {
            debug_enabled: true,
            ..Default::default()
        };
        let toggled = ctrl.tick(TOGGLE_MASK, TOGGLE_HELD);
        assert!(toggled);
        assert_eq!(ctrl.view_mode, 1);
        // Second trigger flips back.
        ctrl.tick(TOGGLE_MASK, TOGGLE_HELD);
        assert_eq!(ctrl.view_mode, 0);
    }

    #[test]
    fn toggle_disabled_when_debug_off() {
        let mut ctrl = WorldMapController {
            debug_enabled: false,
            ..Default::default()
        };
        let toggled = ctrl.tick(TOGGLE_MASK, TOGGLE_HELD);
        assert!(!toggled);
        assert_eq!(ctrl.view_mode, 0);
    }

    #[test]
    fn camera_controls_only_in_top_view() {
        let mut ctrl = WorldMapController {
            view_mode: 0,
            ..Default::default()
        };
        ctrl.tick(CAM_X_DEC | CAM_Z_INC, 0);
        assert_eq!(ctrl.camera_x, 0);
        assert_eq!(ctrl.camera_z, 0);
    }

    #[test]
    fn camera_x_z_scroll() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        ctrl.tick(CAM_X_DEC | CAM_Z_INC, 0);
        assert_eq!(ctrl.camera_x, -8);
        assert_eq!(ctrl.camera_z, 8);
    }

    #[test]
    fn azimuth_and_zoom() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        ctrl.tick(AZ_INC | ZOOM_INC, 0);
        assert_eq!(ctrl.azimuth, 0x14);
        assert_eq!(ctrl.zoom, 4);

        ctrl.tick(AZ_DEC | ZOOM_DEC, 0);
        assert_eq!(ctrl.azimuth, 0);
        assert_eq!(ctrl.zoom, 0);
    }

    #[test]
    fn multiple_frames_accumulate() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        for _ in 0..5 {
            ctrl.tick(CAM_X_INC, 0);
        }
        assert_eq!(ctrl.camera_x, 40);
    }
}
