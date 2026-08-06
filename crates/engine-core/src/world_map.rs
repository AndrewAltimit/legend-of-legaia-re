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

use crate::world_map_panel_host::packed_pad;
use legaia_engine_vm::world_map_dim::{ScreenDimPass, emit_screen_dim};
use legaia_engine_vm::world_map_horizon::{HorizonBatch, emit_horizon};

/// Top-view camera control - live when `view_mode != 0` **and** the L1
/// modifier is held.
///
/// Every mask below is a **packed** pad bit - the layout `FUN_801E76D4`
/// reads out of `_DAT_8007B850` (held) and `_DAT_8007B874` (newly pressed),
/// which `FUN_8001822C` builds by swapping the raw BIOS word's byte halves.
/// It is *not* [`crate::input::PadButton`]'s layout, and the two disagree on
/// every bit that matters here: raw `0x1000` is Triangle, packed `0x1000` is
/// D-pad Up. [`WorldMapController::tick`] converts on the way in
/// ([`packed_pad`]), so the constants stay the retail literals.
const CAM_X_DEC: u16 = 0x1000; // packed Up
const CAM_X_INC: u16 = 0x4000; // packed Down
const CAM_Z_DEC: u16 = 0x2000; // packed Right
const CAM_Z_INC: u16 = 0x8000; // packed Left
const AZ_INC: u16 = 0x0020; // packed Circle → clockwise
const AZ_DEC: u16 = 0x0080; // packed Square → counter-clockwise
const ZOOM_DEC: u16 = 0x0008; // packed R1 - zoom out (height -)
const ZOOM_INC: u16 = 0x0002; // packed R2 - zoom in  (height +)

/// Held-pad modifier that opens the whole camera block
/// (`andi v0,v1,0x4; beqz` at `0x801E7830`): packed `0x4` = L1.
const CAM_MODIFIER: u16 = 0x0004;

/// Packed shoulder bank `L2|R2|L1|R1`. The anim-flag toggles only run when
/// **none** of it is held (`andi v0,v0,0xf; bnez` at `0x801E77D8`), which is
/// what keeps them from firing while the camera modifier is down.
const SHOULDER_MASK: u16 = 0x000F;

/// Packed edge bit that toggles anim-A (`xori v0,v0,0x1` at `0x801E7804`):
/// Circle.
const ANIM_A_TOGGLE: u16 = 0x0020;
/// Packed edge bit that toggles anim-B (`xori v0,v0,0x2` at `0x801E7820`):
/// Square.
const ANIM_B_TOGGLE: u16 = 0x0080;

/// Top-view toggle: the **whole** held word must equal this (`li v0,0x4a;
/// bne v1,v0` at `0x801E7710`) - packed `Cross | R1 | R2`. Retail compares
/// the words for equality, not a mask, so any extra button held cancels it.
const TOGGLE_HELD_WORD: u16 = 0x4A;
/// ... and the newly-pressed word must equal exactly this (`li v0,0x40;
/// bne v1,v0` at `0x801E7724`): packed Cross alone.
const TOGGLE_EDGE_WORD: u16 = 0x40;

/// Packed edge bit that arms the map-display fade-up (`andi v0,v0,0x4` at
/// `0x801D0214` inside the locomotion controller `FUN_801D01B0`): L1.
const MAP_DISPLAY_ARM: u16 = 0x0004;

/// SFX cue the map-display arm raises (`jal FUN_80035B50` with `a0 = 0x20`
/// at `0x801D0220`).
pub const MAP_DISPLAY_SFX: u8 = 0x20;

/// How many undrained cues [`WorldMapController::pending_sfx`] keeps. Retail
/// hands each cue straight to the sound driver and queues nothing; the port
/// stages them for a host that has not claimed them yet, so the queue is
/// capped rather than allowed to grow for the length of a session.
const PENDING_SFX_CAP: usize = 8;

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
/// scene globals `_DAT_8007BCD0` / `_DAT_8007BCD4` / `_DAT_8007BCD8` - read
/// as `a1`/`a2`/`a3` at `0x801D1444`/`0x801D1464`/`0x801D146C`, with `a0`
/// cleared in the delay slot. The setter's first argument is dead - retail
/// stores only `a1..a3`.
///
/// (An earlier reading of this doc named `_DAT_8007BCD4/_D8/_DC`; the three
/// `lw` offsets in `overlay_world_map_801d1344.txt` are `-0x4330`, `-0x432c`
/// and `-0x4328` off `lui 0x8008`, i.e. `0x8007BCD0..0x8007BCD8`.)
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
    // REF: FUN_801D1344 (param-prep wrapper; forwards _DAT_8007BCD0/_D4/_D8)
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
#[derive(Debug, Clone)]
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
    /// The map-display fade-up ramp (`FUN_800196A4`'s `0x8007BAF4`) plus the
    /// kingdom-index mirror it re-derives. Armed by the L1 edge on the
    /// overworld, ticked by [`Self::tick`], self-cleared on completion.
    pub entry_fade: WorldMapEntryFade,
    /// Scene PROT base (`_DAT_80084540`) the fade tick re-derives its kingdom
    /// index from. `0` (no kingdom) until a host installs the real base.
    pub scene_base: u16,
    /// Retail's `0x8007B6A8` byte: whether the map-display fade-up is armable
    /// at all. Its own writer is not pinned, so the port defaults it on - the
    /// controller only exists while a kingdom overworld is the live scene,
    /// which is the state retail's byte selects for.
    pub map_display_enabled: bool,
    /// This tick's full-screen grey fade quad (retail `FUN_80024EE4(1, 2,
    /// grey * 0x010101)`), or `None` on a frame with no live ramp.
    ///
    /// NOT WIRED: no renderer draws it. The engine computes the ramp and
    /// stages the quad; the visible half of the transition is still missing
    /// on all three hosts.
    pub entry_fade_draw: Option<FadeQuad>,
    /// Raised for exactly one tick when the fade-up reaches `0x100` - retail's
    /// `_DAT_8007B83C = 0xC` (MAPDSIP INIT) master-mode store.
    ///
    /// NOT WIRED: no host consumes this yet. The engine has no mode-12
    /// map-display screen to hand off to, so the flag is a report, and the
    /// ramp self-clears rather than parking at `0xFF` the way retail's global
    /// does until the overlay swap clears it.
    pub map_display_requested: bool,
    /// SFX cues the controller raised this tick ([`MAP_DISPLAY_SFX`] is the
    /// only producer). Drained by whoever owns the cue bank.
    pub pending_sfx: Vec<u8>,
    /// The three scene globals `FUN_801D1344` forwards into
    /// [`EmitterGate::arm`]: `(_DAT_8007BCD0, _DAT_8007BCD4, _DAT_8007BCD8)`.
    ///
    /// NOT WIRED: nothing sets these yet, so the gate-arm block below is a
    /// complete chain over a zero source and the horizon emitter never fires.
    /// The source is now identified and is *disc* data, not a missing port:
    /// the field VM writes all three from a script operand - the arms at
    /// `0x801E1638` (`sw v0,-0x4330(v1)`), `0x801E1688` (`-0x432c`) and
    /// `0x801E16C8` (`-0x4328`) in `overlay_world_map_801de840.txt`, each with
    /// a sibling ramp arm through the shared `0x801E205C` epilogue. Routing
    /// those register ids through `engine_core::register_ramp` is what fills
    /// this in; until then a caller-supplied value would be invented input.
    pub horizon_params: (u32, u32, u32),
    /// The world-map band's panel windows and panel actors.
    ///
    /// Retail's world-map controller reaches this band through debug branches
    /// that install a `ctx[+0x54]` phase machine over the shared window
    /// system; the engine parks the whole screen here, where the rest of the
    /// world-map render state already lives, and `World::tick_world_map`
    /// drives it. See [`crate::world_map_panel_host`].
    pub panels: crate::world_map_panel_host::PanelActorHost,
}

impl Default for WorldMapController {
    fn default() -> Self {
        Self {
            view_mode: 0,
            anim_flags: 0,
            camera_x: 0,
            camera_z: 0,
            azimuth: 0,
            zoom: 0,
            debug_enabled: false,
            emitter_gate: EmitterGate::default(),
            horizon_angle: 0,
            horizon_alt_band: false,
            horizon: None,
            screen_dim: None,
            entry_fade: WorldMapEntryFade::default(),
            entry_fade_draw: None,
            scene_base: 0,
            // Retail's `0x8007B6A8` gate - see the field docs above.
            map_display_enabled: true,
            map_display_requested: false,
            pending_sfx: Vec::new(),
            horizon_params: (0, 0, 0),
            panels: crate::world_map_panel_host::PanelActorHost::default(),
        }
    }
}

impl WorldMapController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tick one frame. `pad_current` is the full 16-bit **raw**
    /// ([`crate::input::PadButton`]) pad word for this frame; `pad_held` is
    /// the bits that are newly pressed (not held from the previous frame -
    /// the retail `_DAT_8007B874` word).
    ///
    /// Both are converted to the retail **packed** layout on the way in.
    /// Retail's world-map controller reads `_DAT_8007B850` / `_DAT_8007B874`,
    /// which `FUN_8001822C` builds with the byte halves swapped, and every
    /// literal in `FUN_801E76D4` is a packed bit. Feeding the raw word to
    /// those literals cross-wires the whole band - it puts the camera scroll
    /// on the four face buttons and makes the top-view toggle
    /// `Down + Start + L3` instead of `R1 + R2 + Cross` - which is why no
    /// host ever reached the top-view path. Same trap, same fix as
    /// [`crate::world_map_panel_host`]'s.
    ///
    /// Returns `true` if the view mode was toggled this frame.
    pub fn tick(&mut self, pad_current: u16, pad_held: u16) -> bool {
        let held = packed_pad(pad_current);
        let edge = packed_pad(pad_held);
        self.map_display_requested = false;
        self.entry_fade_draw = None;

        // `FUN_801D1344` head (`0x801D1344..0x801D1388`): while the
        // map-display fade-up ramp is live it owns the whole world-map
        // frame - retail sets the actor's movement-disabled flag, runs
        // `FUN_800196A4` and returns. Nothing else in the controller runs.
        if self.entry_fade.ramp != 0 {
            self.run_entry_fade();
            return false;
        }

        // The map-display arm, from the locomotion controller `FUN_801D01B0`
        // (`0x801D01F8..0x801D024C`): with the availability byte set, an L1
        // edge cues SFX 0x20 and seeds the ramp with 1.
        //
        // The port's panel chords (see `World::tick_world_map_panels`) also
        // bind L1, and those are the port's own binding rather than retail's,
        // so the developer band shadows this arm while it is enabled.
        if self.map_display_enabled && !self.debug_enabled && edge & MAP_DISPLAY_ARM != 0 {
            // Bounded: no host drains this yet (see the field doc), and an
            // unbounded queue on a per-frame path is a leak, not a cue bank.
            if self.pending_sfx.len() >= PENDING_SFX_CAP {
                self.pending_sfx.remove(0);
            }
            self.pending_sfx.push(MAP_DISPLAY_SFX);
            self.entry_fade.ramp = 1;
        }

        // `FUN_801D1344`'s gate-arm block (`0x801D1440..0x801D1474`).
        self.arm_horizon_from_params();

        let mut toggled = false;

        // Top-view debug toggle, gated on `_DAT_8007B98C` (`0x801E7700`).
        // Retail compares the two pad *words* for equality, so a stray held
        // button cancels the chord.
        if self.debug_enabled && held == TOGGLE_HELD_WORD && edge == TOGGLE_EDGE_WORD {
            self.view_mode ^= 1;
            toggled = true;
        }

        // `0x801E779C`: walk mode skips the entire top-view block.
        if self.view_mode == 0 {
            return toggled;
        }

        // `0x801E77D0..0x801E7824`: the anim-flag toggles, live only while no
        // shoulder button is held. Retail runs them *after* the screen-dim
        // call, so a freshly toggled bit dims from the next frame; the engine
        // runs the dim from `World::tick_world_map` after this call, so the
        // port dims one frame earlier. Nothing else observes the difference.
        if held & SHOULDER_MASK == 0 {
            if edge & ANIM_A_TOGGLE != 0 {
                self.anim_flags ^= 1;
            }
            if edge & ANIM_B_TOGGLE != 0 {
                self.anim_flags ^= 2;
            }
        }

        // `0x801E7830`: the camera block needs the L1 modifier held. Without
        // it the top-view d-pad belongs to the anim toggles above, which is
        // why the two banks share bits without colliding.
        if held & CAM_MODIFIER != 0 {
            if held & CAM_X_DEC != 0 {
                self.camera_x -= 8;
            }
            if held & CAM_X_INC != 0 {
                self.camera_x += 8;
            }
            if held & CAM_Z_DEC != 0 {
                self.camera_z -= 8;
            }
            if held & CAM_Z_INC != 0 {
                self.camera_z += 8;
            }
            if held & AZ_INC != 0 {
                self.azimuth += 0x14;
            }
            if held & AZ_DEC != 0 {
                self.azimuth -= 0x14;
            }
            if held & ZOOM_DEC != 0 {
                self.zoom -= 4;
            }
            if held & ZOOM_INC != 0 {
                self.zoom += 4;
            }
        }

        toggled
    }

    /// One frame of the map-display fade-up (`FUN_800196A4`, called from
    /// `FUN_801D1344`'s head).
    ///
    /// The cadence is one vsync per tick: [`Self::tick`] carries no
    /// frame-delta parameter, and `DAT_1F800393` is `1` at a held 60 Hz.
    ///
    /// Retail leaves the ramp parked at `0xFF` and lets the mode-12 overlay
    /// swap clear it (`FUN_801D6704`'s `sw zero`). The engine has no such
    /// swap, so the completion self-clears here: the report fires exactly
    /// once per arm instead of every frame forever.
    // REF: FUN_801D1344 (the caller's `0x8007BAF4 != 0` gate)
    fn run_entry_fade(&mut self) {
        let out = self.entry_fade.tick(self.scene_base, 1);
        self.entry_fade_draw = out.quad;
        if out.enter_map_display {
            self.map_display_requested = true;
            self.entry_fade.ramp = 0;
        }
    }

    /// `FUN_801D1344`'s emitter-gate arm (`0x801D1440..0x801D1474`): arm the
    /// one-shot horizon gate from the three scene globals, but only when at
    /// least one of the first two is non-zero (`bne a1,zero` / `beq v0,zero`).
    ///
    /// Runs every world-map frame, exactly as retail does, so a scene that
    /// sets the globals emits every frame rather than once.
    // REF: FUN_801D1344
    fn arm_horizon_from_params(&mut self) {
        let (scale, angle_step, ot_layer) = self.horizon_params;
        if scale != 0 || angle_step != 0 {
            self.emitter_gate.arm(scale, angle_step, ot_layer);
        }
    }

    /// Drain the SFX cues the controller raised since the last call.
    pub fn drain_sfx(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_sfx)
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

    /// The raw ([`crate::input::PadButton`]) word that arrives at the
    /// controller as `packed`. `tick` takes raw words, every retail literal
    /// is packed, and the conversion is the byte swap.
    fn raw(packed: u16) -> u16 {
        packed.swap_bytes()
    }

    /// Retail's top-view chord in raw bits: R1 + R2 held with Cross the new
    /// press.
    fn toggle_chord() -> (u16, u16) {
        (raw(TOGGLE_HELD_WORD), raw(TOGGLE_EDGE_WORD))
    }

    #[test]
    fn debug_toggle_flips_view_mode() {
        let mut ctrl = WorldMapController {
            debug_enabled: true,
            ..Default::default()
        };
        let (held, edge) = toggle_chord();
        // The chord is R1 | R2 | Cross held with Cross newly pressed - not
        // the raw bits 0x4A (Down | Start | L3) the packed literal reads as
        // when it is applied to an unconverted word.
        assert_eq!(held, 0x4A00);
        assert_eq!(edge, 0x4000);
        let toggled = ctrl.tick(held, edge);
        assert!(toggled);
        assert_eq!(ctrl.view_mode, 1);
        // Second trigger flips back.
        ctrl.tick(held, edge);
        assert_eq!(ctrl.view_mode, 0);
    }

    /// Retail compares the whole pad words, so a stray held button cancels
    /// the chord (`bne v1,v0` at `0x801E7714`, not an `andi`).
    #[test]
    fn toggle_is_an_exact_word_match() {
        let mut ctrl = WorldMapController {
            debug_enabled: true,
            ..Default::default()
        };
        let (held, edge) = toggle_chord();
        assert!(!ctrl.tick(held | crate::input::PadButton::Up.mask(), edge));
        assert_eq!(ctrl.view_mode, 0);
        assert!(!ctrl.tick(held, edge | crate::input::PadButton::Circle.mask()));
        assert_eq!(ctrl.view_mode, 0);
    }

    #[test]
    fn toggle_disabled_when_debug_off() {
        let mut ctrl = WorldMapController {
            debug_enabled: false,
            ..Default::default()
        };
        let (held, edge) = toggle_chord();
        let toggled = ctrl.tick(held, edge);
        assert!(!toggled);
        assert_eq!(ctrl.view_mode, 0);
    }

    #[test]
    fn camera_controls_only_in_top_view() {
        let mut ctrl = WorldMapController {
            view_mode: 0,
            ..Default::default()
        };
        ctrl.tick(raw(CAM_MODIFIER | CAM_X_DEC | CAM_Z_INC), 0);
        assert_eq!(ctrl.camera_x, 0);
        assert_eq!(ctrl.camera_z, 0);
    }

    /// The camera bank is behind the L1 modifier (`andi v0,v1,0x4` at
    /// `0x801E7830`). Without it a top-view d-pad frame moves nothing.
    #[test]
    fn camera_needs_the_l1_modifier() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        ctrl.tick(raw(CAM_X_DEC | CAM_Z_INC), 0);
        assert_eq!((ctrl.camera_x, ctrl.camera_z), (0, 0));
    }

    #[test]
    fn camera_x_z_scroll() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        ctrl.tick(raw(CAM_MODIFIER | CAM_X_DEC | CAM_Z_INC), 0);
        assert_eq!(ctrl.camera_x, -8);
        assert_eq!(ctrl.camera_z, 8);
    }

    #[test]
    fn azimuth_and_zoom() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        ctrl.tick(raw(CAM_MODIFIER | AZ_INC | ZOOM_INC), 0);
        assert_eq!(ctrl.azimuth, 0x14);
        assert_eq!(ctrl.zoom, 4);

        ctrl.tick(raw(CAM_MODIFIER | AZ_DEC | ZOOM_DEC), 0);
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
            ctrl.tick(raw(CAM_MODIFIER | CAM_X_INC), 0);
        }
        assert_eq!(ctrl.camera_x, 40);
    }

    /// The anim-flag toggles (`0x801E77F0` / `0x801E780C`) are what gate the
    /// top-view screen-dim pass, and they only run while no shoulder button
    /// is held (`andi v0,v0,0xf; bnez` at `0x801E77D8`).
    #[test]
    fn anim_flags_toggle_on_circle_and_square_edges() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        ctrl.tick(raw(ANIM_A_TOGGLE), raw(ANIM_A_TOGGLE));
        assert_eq!(ctrl.anim_flags, 1);
        ctrl.tick(raw(ANIM_B_TOGGLE), raw(ANIM_B_TOGGLE));
        assert_eq!(ctrl.anim_flags, 3);
        // Toggling, not setting.
        ctrl.tick(raw(ANIM_A_TOGGLE), raw(ANIM_A_TOGGLE));
        assert_eq!(ctrl.anim_flags, 2);
    }

    #[test]
    fn anim_toggles_are_blocked_while_a_shoulder_is_held() {
        let mut ctrl = WorldMapController {
            view_mode: 1,
            ..Default::default()
        };
        ctrl.tick(raw(CAM_MODIFIER | ANIM_A_TOGGLE), raw(ANIM_A_TOGGLE));
        assert_eq!(ctrl.anim_flags, 0, "L1 held must shadow the anim toggle");
    }

    #[test]
    fn anim_toggles_are_top_view_only() {
        let mut ctrl = WorldMapController::default();
        ctrl.tick(raw(ANIM_A_TOGGLE), raw(ANIM_A_TOGGLE));
        assert_eq!(ctrl.anim_flags, 0);
    }

    /// The dim pass is what the anim bit gates, so the pad edge has to reach
    /// it end to end: toggle into top view, press Circle, and the very next
    /// `run_screen_dim` must produce retail's packet.
    #[test]
    fn pad_alone_reaches_the_top_view_screen_dim() {
        let mut ctrl = WorldMapController {
            debug_enabled: true,
            ..Default::default()
        };
        let (held, edge) = toggle_chord();
        ctrl.tick(held, edge);
        assert!(ctrl.is_top_view());
        assert!(!ctrl.run_screen_dim(), "anim bit still clear");

        ctrl.tick(raw(ANIM_A_TOGGLE), raw(ANIM_A_TOGGLE));
        assert!(ctrl.run_screen_dim(), "anim bit 0 set must dim");
        let dim = ctrl.screen_dim.expect("dim pass");
        assert_eq!(dim.quad.cmd, 0x2A);
        assert_eq!(dim.quad.color, (0, 0, 0));
        assert!(!dim.mode_before.dither && dim.mode_after.dither);
    }

    // -- the map-display fade-up arm (FUN_801D01B0 -> FUN_801D1344) ------

    #[test]
    fn l1_edge_arms_the_map_display_fade_and_cues_its_sfx() {
        let mut ctrl = WorldMapController::default();
        assert_eq!(ctrl.entry_fade.ramp, 0);
        ctrl.tick(raw(MAP_DISPLAY_ARM), raw(MAP_DISPLAY_ARM));
        assert_eq!(ctrl.entry_fade.ramp, 1, "the arm seeds the ramp with 1");
        assert_eq!(ctrl.drain_sfx(), vec![MAP_DISPLAY_SFX]);
    }

    #[test]
    fn the_arm_is_gated_on_the_availability_byte() {
        let mut ctrl = WorldMapController {
            map_display_enabled: false,
            ..Default::default()
        };
        ctrl.tick(raw(MAP_DISPLAY_ARM), raw(MAP_DISPLAY_ARM));
        assert_eq!(ctrl.entry_fade.ramp, 0);
        assert!(ctrl.drain_sfx().is_empty());
    }

    /// While the ramp is live the controller frame *is* the fade: retail's
    /// head returns before the toggle and camera blocks ever run.
    #[test]
    fn a_live_ramp_owns_the_whole_controller_frame() {
        let mut ctrl = WorldMapController {
            debug_enabled: true,
            view_mode: 1,
            ..Default::default()
        };
        ctrl.entry_fade.ramp = 1;
        ctrl.tick(raw(CAM_MODIFIER | CAM_X_INC), 0);
        assert_eq!(ctrl.camera_x, 0, "camera must not step under the fade");
        assert_eq!(ctrl.entry_fade.ramp, 33, "but the ramp advanced");
    }

    /// The ramp runs to the mode-12 hand-off, reports once, and clears - so
    /// a second arm is a second transition rather than a stuck report.
    #[test]
    fn the_fade_runs_to_a_single_map_display_report() {
        let mut ctrl = WorldMapController {
            scene_base: 0xF4,
            ..Default::default()
        };
        ctrl.tick(raw(MAP_DISPLAY_ARM), raw(MAP_DISPLAY_ARM));
        let mut greys = Vec::new();
        let mut reports = 0;
        for _ in 0..32 {
            ctrl.tick(0, 0);
            if let Some(q) = ctrl.entry_fade_draw {
                greys.push(q.grey);
            }
            if ctrl.map_display_requested {
                reports += 1;
            }
        }
        assert_eq!(reports, 1, "exactly one mode-12 hand-off per arm");
        assert_eq!(ctrl.entry_fade.ramp, 0, "the ramp self-clears");
        assert_eq!(ctrl.entry_fade.kingdom_index, Some(1), "Sebucus");
        assert!(
            greys.windows(2).all(|w| w[0] < w[1]),
            "the fade-up ramp must be monotone: {greys:?}"
        );
        assert_eq!(greys.last(), Some(&0xFF));
    }

    /// The horizon gate is armed from the three scene globals every frame,
    /// and only when at least one of the first two is non-zero.
    #[test]
    fn horizon_gate_arms_from_the_scene_globals_only_when_set() {
        let mut ctrl = WorldMapController::default();
        ctrl.tick(0, 0);
        assert!(!ctrl.emitter_gate.armed, "zero params arm nothing");

        ctrl.horizon_params = (0, 0, 9);
        ctrl.tick(0, 0);
        assert!(
            !ctrl.emitter_gate.armed,
            "the OT layer alone does not open the gate"
        );

        ctrl.horizon_params = (0x500, 0x10, 4);
        ctrl.tick(0, 0);
        assert_eq!(ctrl.emitter_gate.take(), Some((0x500, 0x10, 4)));
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
