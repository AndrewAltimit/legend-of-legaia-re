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

use legaia_engine_vm::world_map_dim::{ScreenDimPass, emit_screen_dim};
use legaia_engine_vm::world_map_horizon::{HorizonBatch, emit_horizon};

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
    /// Persisted horizon sweep angle (`_DAT_801F3518`). Advanced once per
    /// armed emission by `frame_step * angle_step`.
    pub horizon_angle: u32,
    /// Alternate horizon source-band select (`_DAT_8007B74C != 0`), which
    /// shifts every band's VRAM blit source row.
    pub horizon_alt_band: bool,
    /// Bands produced by the most recent armed emission, for a renderer to
    /// consume. `None` until the gate first fires.
    pub horizon: Option<HorizonBatch>,
    /// This frame's top-view screen-dim pass, or `None` on frames where the
    /// retail gate (`view_mode != 0 && anim_flags & 1`) does not fire. Set by
    /// [`Self::run_screen_dim`], which the world-map tick calls once per
    /// frame; a renderer draws it behind the top-view debug panels.
    pub screen_dim: Option<ScreenDimPass>,
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

    /// Consume an armed [`EmitterGate`] and run the horizon emitter.
    ///
    /// This is the retail call pair: the gate check + self-clear that opens
    /// `FUN_801D7EA0` / `FUN_801C9688`, followed by the emitter body ported
    /// in [`legaia_engine_vm::world_map_horizon::emit_horizon`]. Returns
    /// `true` when a batch was emitted (i.e. the gate was armed).
    ///
    /// `frame_step` is the adaptive per-frame tick byte `DAT_1F800393`;
    /// `trig` samples the `0x1000`-entry table behind `_DAT_8007B81C`.
    pub fn run_horizon_emitter(&mut self, frame_step: u8, trig: &dyn Fn(u16) -> i16) -> bool {
        let Some((scale, angle_step, ot_layer)) = self.emitter_gate.take() else {
            self.horizon = None;
            return false;
        };
        let batch = emit_horizon(
            scale as i32,
            self.horizon_angle,
            angle_step,
            frame_step,
            ot_layer,
            self.horizon_alt_band,
            trig,
        );
        self.horizon_angle = batch.angle_after;
        self.horizon = Some(batch);
        true
    }

    /// Run the retail top-view screen-dim gate for this frame.
    ///
    /// This is the branch pair at `0x801E7794..0x801E77B8` inside the
    /// controller `FUN_801E76D4`: the whole top-view block is skipped when
    /// `DAT_801F2B94` (`view_mode`) is zero, and within it the dim call is
    /// skipped unless bit 0 of `DAT_801F2B95` (`anim_flags`) is set. When
    /// both hold, retail calls `FUN_801E75DC`, whose packets are built by
    /// [`legaia_engine_vm::world_map_dim::emit_screen_dim`].
    ///
    /// Stores the result in [`Self::screen_dim`] and returns `true` when the
    /// pass fired. Non-firing frames clear the field, so a renderer reading
    /// it never draws a stale dim over a frame retail left undimmed.
    ///
    /// REF: FUN_801E76D4
    pub fn run_screen_dim(&mut self) -> bool {
        if self.view_mode == 0 || self.anim_flags & 1 == 0 {
            self.screen_dim = None;
            return false;
        }
        self.screen_dim = Some(emit_screen_dim());
        true
    }
}

// =========================================================================
// World-map-entry fade-up - FUN_800196A4
// =========================================================================

/// Kingdom index for a scene PROT base index (`_DAT_80084540`), as the
/// fade-up tick derives it into `gp+0x658`: the three kingdom overworld
/// bundles `0x55` (Drake) / `0xF4` (Sebucus) / `0x187` (Karisto) map to
/// `0..=2`; anything else is `None` (retail `-1`).
pub fn kingdom_index_for_scene_base(scene_base: u16) -> Option<u8> {
    match scene_base {
        0x55 => Some(0),
        0xF4 => Some(1),
        0x187 => Some(2),
        _ => None,
    }
}

/// Per-frame draw command the fade-up tick emits: a full-screen grey quad
/// (retail `FUN_80024EE4(1, 2, grey * 0x010101)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeQuad {
    /// Grey level 0..=0xFF (the draw value clamps even when the stored
    /// ramp value overshoots).
    pub grey: u8,
}

/// PORT: FUN_800196A4
///
/// World-map-entry **fade-up tick**. Retail runs this per frame on the
/// way into the world-map display mode:
///
/// 1. Re-derives the kingdom index global `gp+0x658` from the scene
///    PROT base `_DAT_80084540` ([`kingdom_index_for_scene_base`]).
/// 2. When the fade ramp global (`0x8007BAF4`) is non-zero, advances it
///    by `cadence << 5` (the frame-delta byte `DAT_1F800393` times 32),
///    stores the **un-clamped** value back, and emits the fade quad with
///    the grey level clamped to `0xFF`.
/// 3. When the stored ramp value reaches `0x100`, parks it at `0xFF` and
///    stores master mode `_DAT_8007B83C = 0xC` (12 = MAPDSIP INIT, the
///    world-map display overlay swap - see `docs/subsystems/boot.md`).
///
/// A zero ramp value is idle: no quad, no mode switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorldMapEntryFade {
    /// The ramp global (`0x8007BAF4`). `0` = idle; callers arm the fade
    /// by setting it non-zero (retail's arming site writes a small seed).
    pub ramp: i32,
    /// Kingdom index mirror of `gp+0x658` (`None` = retail `-1`).
    pub kingdom_index: Option<u8>,
}

/// One tick's outputs: the fade quad to draw (if the ramp is live) and
/// whether the mode-12 switch fired this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldMapEntryFadeTick {
    pub quad: Option<FadeQuad>,
    pub enter_map_display: bool,
}

impl WorldMapEntryFade {
    /// One frame of `FUN_800196A4`. `scene_base` mirrors
    /// `_DAT_80084540`; `cadence` is the frame-delta byte.
    pub fn tick(&mut self, scene_base: u16, cadence: u8) -> WorldMapEntryFadeTick {
        self.kingdom_index = kingdom_index_for_scene_base(scene_base);
        let mut quad = None;
        if self.ramp != 0 {
            self.ramp += (cadence as i32) << 5;
            let grey = if self.ramp < 0x100 {
                self.ramp as u8
            } else {
                0xFF
            };
            quad = Some(FadeQuad { grey });
        }
        let mut enter = false;
        if self.ramp >= 0x100 {
            self.ramp = 0xFF;
            enter = true;
        }
        WorldMapEntryFadeTick {
            quad,
            enter_map_display: enter,
        }
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

    /// Flat trig table - keeps the band algebra to its scale-only terms.
    fn flat(_: u16) -> i16 {
        0
    }

    #[test]
    fn horizon_emitter_only_runs_when_the_gate_is_armed() {
        let mut ctrl = WorldMapController::new();
        assert!(
            !ctrl.run_horizon_emitter(1, &flat),
            "unarmed gate emits nothing"
        );
        assert!(ctrl.horizon.is_none());

        ctrl.emitter_gate.arm(0x500, 0x10, 4);
        assert!(ctrl.run_horizon_emitter(1, &flat));
        let batch = ctrl.horizon.as_ref().expect("armed gate emits a batch");
        assert_eq!(batch.bands.len(), 224);
        assert_eq!(batch.ot_layer, 4, "the staged OT layer carries through");

        // One-shot: the next frame is unarmed again and drops the batch.
        assert!(!ctrl.run_horizon_emitter(1, &flat));
        assert!(ctrl.horizon.is_none());
    }

    #[test]
    fn horizon_angle_persists_across_emissions() {
        let mut ctrl = WorldMapController::new();
        // Three armed frames at step 0x20 with frame_step 2 advance the
        // persisted angle by 0x40 each time - and nothing else.
        for i in 1..=3u32 {
            ctrl.emitter_gate.arm(0x100, 0x20, 0);
            assert!(ctrl.run_horizon_emitter(2, &flat));
            assert_eq!(ctrl.horizon_angle, i * 0x40);
        }
    }

    #[test]
    fn horizon_alt_band_shifts_the_blit_source_rows() {
        let mut ctrl = WorldMapController {
            horizon_alt_band: true,
            ..Default::default()
        };
        ctrl.emitter_gate.arm(0x100, 0, 0);
        ctrl.run_horizon_emitter(0, &flat);
        let batch = ctrl.horizon.as_ref().unwrap();
        // First band's source row is the raw counter (4) plus the offset.
        assert_eq!(batch.bands[0].blit.src_y, 4 + 0xF0);
    }

    #[test]
    fn emitter_gate_rearm_overwrites_staged_params() {
        let mut gate = EmitterGate::default();
        gate.arm(1, 2, 3);
        gate.arm(7, 8, 9);
        assert_eq!(gate.take(), Some((7, 8, 9)), "plain stores, last arm wins");
    }

    // -- WorldMapEntryFade (FUN_800196A4) ------------------------------

    #[test]
    fn kingdom_index_covers_the_three_overworld_bases() {
        assert_eq!(kingdom_index_for_scene_base(0x55), Some(0));
        assert_eq!(kingdom_index_for_scene_base(0xF4), Some(1));
        assert_eq!(kingdom_index_for_scene_base(0x187), Some(2));
        assert_eq!(kingdom_index_for_scene_base(0x56), None);
        assert_eq!(kingdom_index_for_scene_base(0), None);
    }

    #[test]
    fn fade_idle_emits_nothing() {
        let mut f = WorldMapEntryFade::default();
        let t = f.tick(0x55, 1);
        assert_eq!(t.quad, None);
        assert!(!t.enter_map_display);
        assert_eq!(f.kingdom_index, Some(0));
        assert_eq!(f.ramp, 0, "idle ramp untouched");
    }

    #[test]
    fn fade_ramps_by_32_per_cadence_unit_and_fires_mode_switch() {
        let mut f = WorldMapEntryFade {
            ramp: 1,
            ..Default::default()
        };
        // Tick 1: 1 + 32 = 33; below 0x100 -> grey 33, no switch.
        let t = f.tick(0xF4, 1);
        assert_eq!(t.quad, Some(FadeQuad { grey: 33 }));
        assert!(!t.enter_map_display);
        assert_eq!(f.ramp, 33);
        // Keep ticking until the switch fires.
        let mut switched_at = None;
        for i in 0..16 {
            let t = f.tick(0xF4, 1);
            if t.enter_map_display {
                switched_at = Some(i);
                // The draw value clamps to 0xFF while the stored value
                // overshoots then parks at 0xFF.
                assert_eq!(t.quad, Some(FadeQuad { grey: 0xFF }));
                assert_eq!(f.ramp, 0xFF);
                break;
            }
        }
        // 33 + 32*k >= 0x100 first at k = 7 (index 6).
        assert_eq!(switched_at, Some(6));
    }

    #[test]
    fn fade_parked_ramp_keeps_firing_until_cleared() {
        // Retail leaves the global at 0xFF after the switch; every
        // subsequent tick overshoots and fires again until the mode
        // change clears it.
        let mut f = WorldMapEntryFade {
            ramp: 0xFF,
            ..Default::default()
        };
        let t = f.tick(0x187, 1);
        assert!(t.enter_map_display);
        assert_eq!(f.ramp, 0xFF);
        let t = f.tick(0x187, 1);
        assert!(t.enter_map_display);
    }

    #[test]
    fn fade_cadence_two_doubles_the_step() {
        let mut f = WorldMapEntryFade {
            ramp: 1,
            ..Default::default()
        };
        f.tick(0x55, 2);
        assert_eq!(f.ramp, 1 + 64);
    }
}
