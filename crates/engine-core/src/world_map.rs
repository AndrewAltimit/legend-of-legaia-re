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

/// One-shot gate for the world-map POLY_FT4 batch emitter (`FUN_801D7EA0`;
/// 0897 field-overlay sibling `FUN_801C9688`).
///
/// Retail keeps this in the persistent `0x801F0000+` region so it survives
/// overlay swaps:
///
/// | Field | Retail global | Notes |
/// |---|---|---|
/// | `armed` | `_DAT_801F351C` | Set to `1` by the arm; the emitter self-clears it after one emission. |
/// | `scale` | `_DAT_801F3520` | Render scale / range (the emitter uses it as `local_3c` and `local_3c / 5`). |
/// | `angle_step` | `_DAT_801F3524` | Angle step per frame tick. |
/// | `ot_layer` | `_DAT_801F3528` | OT layer / draw priority. |
///
/// Armed by the 40-byte setter `FUN_801D8258`, whose caller (`FUN_801D1344`,
/// 0897 relocation copy `FUN_801C2B2C`) sources the three params from the
/// trigger globals `_DAT_8007BCD4/_D8/_DC`. The setter's first argument is
/// dead - retail stores only `a1..a3`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmitterGate {
    pub armed: bool,
    pub scale: u32,
    pub angle_step: u32,
    pub ot_layer: u32,
}

impl EmitterGate {
    /// Arm the gate for one emission, staging the emitter's inputs. A re-arm
    /// before the emitter consumes the gate overwrites the staged params
    /// (retail plain stores, no accumulate).
    // PORT: FUN_801D8258
    // REF: FUN_801D1344 (param-prep wrapper; forwards _DAT_8007BCD4/_D8/_DC)
    // REF: FUN_801C2B2C (the wrapper's 0897 field-overlay relocation copy)
    pub fn arm(&mut self, scale: u32, angle_step: u32, ot_layer: u32) {
        self.armed = true;
        self.scale = scale;
        self.angle_step = angle_step;
        self.ot_layer = ot_layer;
    }

    /// Consumer side: if armed, self-clear the gate and yield the staged
    /// `(scale, angle_step, ot_layer)` params for one emission (the
    /// `_DAT_801F351C != 0 -> _DAT_801F351C = 0` head of `FUN_801D7EA0` /
    /// `FUN_801C9688`). `None` when not armed.
    // REF: FUN_801D7EA0, FUN_801C9688 (the two gate-clearing emitters)
    pub fn take(&mut self) -> Option<(u32, u32, u32)> {
        if !self.armed {
            return None;
        }
        self.armed = false;
        Some((self.scale, self.angle_step, self.ot_layer))
    }
}

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
    /// One-shot POLY_FT4 batch-emitter gate. Retail hosts it in persistent
    /// RAM shared with the 0897 field overlay (see [`EmitterGate`]); the
    /// engine parks it on the controller, where the world-map render state
    /// lives.
    pub emitter_gate: EmitterGate,
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

    #[test]
    fn emitter_gate_arms_and_self_clears_once() {
        let mut gate = EmitterGate::default();
        assert_eq!(gate.take(), None, "unarmed gate yields nothing");
        gate.arm(0x500, 0x10, 4);
        assert!(gate.armed);
        // The emitter consumes the gate exactly once (retail self-clear).
        assert_eq!(gate.take(), Some((0x500, 0x10, 4)));
        assert!(!gate.armed);
        assert_eq!(gate.take(), None, "one-shot: second take is empty");
        // The staged params stay readable after the clear (retail leaves
        // _DAT_801F3520..28 in place; only the flag resets).
        assert_eq!(gate.scale, 0x500);
    }

    #[test]
    fn emitter_gate_rearm_overwrites_staged_params() {
        let mut gate = EmitterGate::default();
        gate.arm(1, 2, 3);
        gate.arm(7, 8, 9);
        assert_eq!(gate.take(), Some((7, 8, 9)), "plain stores, last arm wins");
    }
}
