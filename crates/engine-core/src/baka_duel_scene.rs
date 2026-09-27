//! The Baka Fighter duel as a **3D surface**: the two fighters' meshes posed
//! by the duel's own clip clocks, the special's afterimage ghosts, the arena
//! walls and floor, and the arena camera - one kernel every host draws.
//!
//! Three layers, each from the overlay's own code (PROT 0976, base
//! `0x801CE818`; `see ghidra/scripts/funcs/overlay_baka_fighter_801cf388.txt`
//! for the cabinet and `overlay_baka_fighter_801d3f44.txt` for the combat
//! tick):
//!
//! * **[`DuelCamera`]** - the arena camera. The round setup (cabinet state
//!   `0x32`, `0x801CFF34..0x801CFF7C`) snaps the ten camera globals to
//!   [`ROUND_SETUP_CAMERA`]; state `0x35` (`0x801D0324..0x801D0454`) spins the
//!   yaw by `dt << 6` a frame while raising the eye trio's Y by `dt << 2` and
//!   Z by `dt << 5`, and once the yaw passes `0x1000` it zeroes the yaw and
//!   hands the eye to a camera-relative glide ([`SWEEP_GLIDE`]) that settles
//!   it at `(_, 0x898, 0x3520)`. That settled pose is the camera the duel
//!   (state `0x64`) fights under - nothing in the duel state writes a camera
//!   global. A special commit arms a second glide from the combat tick
//!   (`0x801D4644..0x801D4740`): the player's row of the table at
//!   [`SPECIAL_CAMERA_TABLE_VA`], or [`OPPONENT_SPECIAL_GLIDE`] for the
//!   opponent. Both glides are the SCUS camera-relative glide family
//!   (`FUN_80021248` normalizes, `FUN_8002149C` walks), run here through
//!   [`legaia_engine_vm::camera_rel_actor`] and [`crate::camera_rel_glide`].
//!   The world is drawn through the field view build `FUN_800172C0` with the
//!   base matrix at `0x6000` (6x, captured in the parked `minigame_baka_fighter`
//!   state along with `H = 512`), so [`DuelCamera::view`] divides the eye trio
//!   by [`DUEL_WORLD_SCALE`] - the rule `renderer.md` derives.
//! * **[`FighterMotion`]** - each fighter's display clip: the action record
//!   the actor's `+0x5C` names and the cursor `+0x68` the clip selector
//!   `FUN_800204F8` advances. [`crate::baka_fighter::BakaFight`] owns one per
//!   seat and steps it every tick.
//! * **[`BakaDuelScene`]** - the combined vertex buffers (fighters, two ghost
//!   copies per fighter, the four arena walls, the floor grid) and the
//!   per-frame pose into world space. [`BakaDuelSurface`] is the per-host
//!   cache that loads the assets once, rebuilds the buffers when a rung seats
//!   a new opponent, and poses them each frame.
//!
//! The impact pair's effect parts ([`crate::baka_impact_fx`]) draw into
//! reserved ranges of the same buffers: the flip-book flash as sprite-arm
//! quads and the two prop flashes as copies of PROT 1203 stage TMDs `1` / `2`.
//! Their UVs and colours change frame to frame, which
//! [`BakaDuelScene::attr_generation`] announces to a host that uploads
//! attributes only on change. What stays outside: the actor drop shadow
//! `FUN_801D6BB8`.

use std::ops::Range;
use std::sync::Arc;

use crate::baka_fighter::{BakaFight, ClipHeader};
use crate::camera::RetailCamGlobals;
use crate::camera_rel_glide::CameraRelGlide;
use legaia_asset::baka_opponents as bo;
use legaia_engine_vm::camera_rel_actor::normalize_camera_relative_params;
use legaia_engine_vm::psx_camera::{FieldCameraView, camera_rotation};

// ---------------------------------------------------------------- camera

/// The duel's base-matrix scale: `_DAT_8007BF10` reads `0x6000` on the
/// diagonal in the parked Baka Fighter state (GTE `0x1000` = 1.0).
pub const DUEL_WORLD_SCALE: f32 = 6.0;

/// The camera globals the round setup (cabinet state `0x32`) stores:
/// pitch `0`, yaw `0x2F8`, roll `0` (`0x801CFF34..0x801CFF48`), focus zero
/// (`0x801CFF4C..0x801CFF64`), eye trio `(0xC8, 0x708, 0x1FE0)`
/// (`0x801CFF68..0x801CFF7C`). `H` is the duel init's `0x200`
/// (`FUN_801CF00C`, `sh v0,-0x490c(v1)` at `0x801CF080`).
pub const ROUND_SETUP_CAMERA: RetailCamGlobals =
    RetailCamGlobals([0, 0x2F8, 0, 0xC8, 0x708, 0x1FE0, 0, 0, 0, 0x200]);

/// State `0x35` holds the spin while the sign-extended yaw is below this
/// (`slti a0,a0,0x1001` at `0x801D03A0`).
pub const SWEEP_END_YAW: i32 = 0x1001;

/// The glide state `0x35` arms once the spin ends (`0x801D03D0..0x801D0454`):
/// the angles and the eye X parked (step `0`), eye Y to `0x898` at `0x1E`,
/// eye Z to `0x3520` at `0xC8`, focus and `H` parked. `(step, target)` pairs
/// in the camera-relative record order ([`crate::camera_rel_glide`]).
pub const SWEEP_GLIDE: [i16; 20] = [
    0, 0, 0, 0, 0, 0, // angles
    0, 0, 0x1E, 0x898, 0xC8, 0x3520, // eye trio
    0, 0, 0, 0, 0, 0, // focus
    0, 0, // H
];

/// The camera the duel settles on: [`ROUND_SETUP_CAMERA`] after the spin and
/// [`SWEEP_GLIDE`]. The only axes the sequence leaves changed are yaw `0`
/// and the eye trio's Y / Z.
pub const DUEL_CAMERA: RetailCamGlobals =
    RetailCamGlobals([0, 0, 0, 0xC8, 0x898, 0x3520, 0, 0, 0, 0x200]);

/// The result close-up the tally's end snaps to: yaw `0x3D4`, eye
/// `(0, 0x8FC, 0x1900)`, pitch `0` (`0x801D0A38..0x801D0A70`) - or `0x64`
/// on the secret opponent's variant (`0x801D0FBC..0x801D0FF0`). Focus and
/// `H` are left as the duel had them.
pub const RESULT_CAMERA: [i32; 6] = [0, 0x3D4, 0, 0, 0x8FC, 0x1900];
/// The secret variant's pitch.
pub const RESULT_SECRET_PITCH: i32 = 0x64;
/// The fighter the result close-up steps forward: the player actor's `+0x5A`
/// is compared with `1` (`0x801D0A74..0x801D0A90`) - the party's second
/// fighter, Noa.
pub const RESULT_STEP_FIGHTER: usize = 1;
/// How far that step goes, on the actor's `+0x18` (Z).
pub const RESULT_STEP_Z: i32 = 0x3C;

/// The opponent's special-commit glide (`0x801D46CC..0x801D4704`): pitch to
/// `-0x14` at `2`, yaw to `0xA8C` at `0x14`, eye to `(-0x3C, 0x80C, 0x2120)`
/// at `(3, 0x22, 0x41)`.
pub const OPPONENT_SPECIAL_GLIDE: [i16; 20] = [
    2, -0x14, 0x14, 0xA8C, 0, 0, //
    3, -0x3C, 0x22, 0x80C, 0x41, 0x2120, //
    0, 0, 0, 0, 0, 0, //
    0, 0,
];

/// Runtime VA of the player's special-commit camera table: `0x20`-byte rows
/// indexed by the fighter actor's `+0x5A` (`sll v1,s8,0x5` at `0x801D4654`),
/// each four 8-byte rows - angle targets, angle steps, eye targets, eye
/// steps (`0x801D464C..0x801D46C4`).
pub const SPECIAL_CAMERA_TABLE_VA: u32 = 0x801D_7DC8;

/// Rows of that table the disc populates - one per party fighter; the fourth
/// row onward is not a camera record.
pub const SPECIAL_CAMERA_ROWS: usize = 3;

/// Read the player's special-commit glides out of the as-loaded overlay
/// (PROT 0976 at [`bo::BAKA_OVERLAY_BASE_VA`]).
pub fn parse_special_cameras(overlay: &[u8]) -> Vec<[i16; 20]> {
    let Some(base) = SPECIAL_CAMERA_TABLE_VA
        .checked_sub(bo::BAKA_OVERLAY_BASE_VA)
        .map(|o| o as usize)
    else {
        return Vec::new();
    };
    let half = |off: usize| -> Option<i16> {
        overlay
            .get(off..off + 2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
    };
    (0..SPECIAL_CAMERA_ROWS)
        .map_while(|row| {
            let r = base + row * 0x20;
            let h = |i: usize| half(r + i * 2);
            let mut rec = [0i16; 20];
            for axis in 0..3 {
                // angles: step at +8, target at +0
                rec[axis * 2] = h(4 + axis)?;
                rec[axis * 2 + 1] = h(axis)?;
                // eye trio: step at +0x18, target at +0x10
                rec[6 + axis * 2] = h(12 + axis)?;
                rec[6 + axis * 2 + 1] = h(8 + axis)?;
            }
            Some(rec)
        })
        .collect()
}

/// The arena camera: the ten retail globals plus the one camera-relative
/// glide the duel can have live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuelCamera {
    globals: RetailCamGlobals,
    glide: Option<CameraRelGlide>,
    /// Cabinet state `0x35`'s spin is running.
    sweeping: bool,
}

impl Default for DuelCamera {
    fn default() -> Self {
        let mut c = Self {
            globals: ROUND_SETUP_CAMERA,
            glide: None,
            sweeping: false,
        };
        c.round_setup();
        c
    }
}

impl DuelCamera {
    /// The round setup's snap, which also starts the round-start spin.
    ///
    /// REF: FUN_801CF388 (state `0x32`, `0x801CFF34..0x801CFF7C`)
    pub fn round_setup(&mut self) {
        self.globals = ROUND_SETUP_CAMERA;
        self.glide = None;
        self.sweeping = true;
    }

    /// The tally's end: snap to [`RESULT_CAMERA`] and drop any glide.
    ///
    /// REF: FUN_801CF388 (states `0x66` / `0x6D`)
    pub fn result_close_up(&mut self, secret: bool) {
        let g = &mut self.globals.0;
        g[..6].copy_from_slice(&RESULT_CAMERA);
        if secret {
            g[0] = RESULT_SECRET_PITCH;
        }
        self.glide = None;
        self.sweeping = false;
    }

    /// Hand the camera to a camera-relative glide record (`(step, target)`
    /// pairs), normalized against the live globals exactly as `FUN_80021248`
    /// does. A new glide supersedes a live one (the spawner flags the
    /// previous family actor for retirement).
    pub fn arm_glide(&mut self, record: &[i16; 20]) {
        let n = normalize_camera_relative_params(record, &self.globals.camera_snapshot());
        self.glide = Some(CameraRelGlide::from_normalized(&n));
    }

    /// One frame: the spin while it runs, then the live glide.
    ///
    /// REF: FUN_801CF388 (state `0x35`, `0x801D0324..0x801D0454`)
    pub fn tick(&mut self, frame_step: i32) {
        let dt = frame_step.max(0);
        if self.sweeping {
            let g = &mut self.globals.0;
            g[1] = i32::from((g[1] as i16).wrapping_add((dt << 6) as i16));
            g[4] = g[4].wrapping_add(dt << 2);
            g[5] = g[5].wrapping_add(dt << 5);
            if g[1] >= SWEEP_END_YAW {
                g[1] = 0;
                self.sweeping = false;
                self.arm_glide(&SWEEP_GLIDE);
            }
        }
        if let Some(glide) = self.glide.as_mut() {
            let done = glide.tick(&mut self.globals, dt.min(255) as u8).finished;
            if done {
                self.glide = None;
            }
        }
    }

    /// The live ten globals.
    pub fn globals(&self) -> RetailCamGlobals {
        self.globals
    }

    /// Whether a spin or glide is still moving the camera.
    pub fn moving(&self) -> bool {
        self.sweeping || self.glide.is_some()
    }

    /// The pose a host projects through, at the 1x world scale the duel
    /// vertices are in (the eye trio divided by [`DUEL_WORLD_SCALE`]).
    pub fn view(&self) -> FieldCameraView {
        let g = self.globals;
        let rad = |a: i32| (a as i16) as f32 / 4096.0 * std::f32::consts::TAU;
        let f = g.focus_world();
        let tr = g.tr_eye();
        FieldCameraView {
            focus: [f[0] as f32, f[1] as f32, f[2] as f32],
            pitch: rad(g.0[0]),
            yaw: rad(g.0[1]),
            roll: rad(g.0[2]),
            h: g.h() as f32,
            tr_eye: [
                tr[0] as f32 / DUEL_WORLD_SCALE,
                tr[1] as f32 / DUEL_WORLD_SCALE,
                tr[2] as f32 / DUEL_WORLD_SCALE,
            ],
        }
    }

    /// The **retail** eye-space depth of a world point (the `MAC3` the GTE
    /// leaves, 6x scale): what the wall draw's cull compares.
    pub fn eye_depth(&self, p: [f32; 3]) -> f32 {
        self.view().eye_space(p)[2] * DUEL_WORLD_SCALE
    }

    /// The column-major view-projection a host multiplies a raw (Y-down)
    /// world vertex by: [`FieldCameraView::vp`] times the Y negation it
    /// expects of a model matrix. Both hosts upload exactly this.
    pub fn vp_raw(&self, aspect: f32) -> [f32; 16] {
        legaia_engine_vm::psx_camera::mat4_mul(
            &self.view().vp(aspect),
            &legaia_engine_vm::psx_camera::WORLD_FLIP,
        )
    }
}

// ---------------------------------------------------------------- motion

/// Action record of the hit reaction the damage kernel plays on the struck
/// side (`base + 6`, `0x801D3C60..0x801D3C70`).
pub const MOTION_HIT: usize = 5;
/// The knockdown: `base + 6 + 2`, played instead of the hit when the landed
/// keyframe is the special's last (`0x801D3C00..0x801D3C18`).
pub const MOTION_KNOCKDOWN: usize = 7;
/// The win flourish the result state plays (`base + 9`, `0x801D100C`).
pub const MOTION_WIN: usize = bo::ACTION_WIN;

/// One fighter's display clip - the actor's clip id `+0x5C`, cursor `+0x68`
/// and the hold / clip-end bits of `+0x62` the clip selector reads and
/// writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FighterMotion {
    /// Action record (`+0x5C` minus the fighter's clip base, minus one).
    pub record: usize,
    /// `+0x68`, 1/16-frame fixed point.
    pub cursor: i32,
    /// `+0x62 & 8` - hold the last frame instead of wrapping.
    pub hold: bool,
    /// `+0x62 & 0x100` - the selector reached the clip end on its last run.
    pub ended: bool,
    /// Block `+0x2C` - the knockdown latch that keeps the idle reset off.
    pub down: bool,
    /// The win flourish is showing; no idle reset.
    pub pinned: bool,
    /// `+0x5E` - the clip the selector last bound.
    bound: Option<usize>,
}

impl FighterMotion {
    /// Start `record` from its first frame (retail zeroes `+0x68` at every
    /// clip store: the commit at `0x801D44D8`, the damage kernel at
    /// `0x801D3CA0`).
    pub fn play(&mut self, record: usize, hold: bool) {
        self.record = record;
        self.cursor = 0;
        self.hold = hold;
        self.ended = false;
        self.bound = Some(record);
    }

    /// Back to the looping idle (the round setup's `base + 1` store).
    pub fn idle(&mut self) {
        *self = Self::default();
    }

    /// One combat tick: the idle reset, then the clip selector's advance.
    ///
    /// REF: FUN_801D3F44 (`0x801D411C..0x801D415C` the idle reset,
    /// `0x801D4744..0x801D47E8` the step), FUN_800204F8 (the selector)
    pub fn step(&mut self, speed: i32, divisor: i32, clip: Option<ClipHeader>, frame_step: i32) {
        if self.ended && !self.down && !self.pinned && self.record != 0 {
            self.record = 0;
            self.hold = false;
        }
        if self.bound != Some(self.record) {
            self.cursor = 0;
            self.bound = Some(self.record);
        }
        let raw = speed.wrapping_mul(divisor);
        let step = (if raw < 0 { raw + 7 } else { raw }) >> 3;
        let step = clip.map_or(step, |c| c.selector_step(step));
        self.ended = false;
        self.cursor = self.cursor.wrapping_add(step.wrapping_mul(frame_step));
        if let Some(c) = clip {
            let end = i32::from(c.frames) * 16 - 1;
            if self.cursor >= end {
                self.cursor = if self.hold { end } else { 0 };
                self.ended = true;
            }
        }
    }

    /// The whole clip frame the renderer poses (`cursor >> 4`).
    pub fn frame(&self) -> usize {
        (self.cursor.max(0) >> 4) as usize
    }
}

// ---------------------------------------------------------------- assets

/// One roster fighter's mesh and clip bank.
#[derive(Debug, Clone)]
pub struct DuelFighterAsset {
    pub tmd: legaia_tmd::Tmd,
    pub raw: Vec<u8>,
    pub bank: legaia_asset::player_anm::PlayerAnmBundle,
    /// Bank record of action `0` (party fighters share one bank, nine
    /// records each; a ladder fighter's own bank starts at `0`).
    pub first_record: usize,
}

/// Everything the duel draws, decoded off the disc once per visit.
#[derive(Debug, Clone, Default)]
pub struct BakaDuelAssets {
    /// Indexed by roster id ([`bo::OPPONENT_COUNT`] entries).
    pub fighters: Vec<Option<DuelFighterAsset>>,
    /// Stage-pack TMD `0` (PROT 1203 descriptor 1) - the wall piece the
    /// epilogue places four times.
    pub wall: Option<(legaia_tmd::Tmd, Vec<u8>)>,
    /// PROT 1203 descriptor 0's TIM pack (HUD sheets, the floor cells).
    pub art: Vec<legaia_tim::Tim>,
    /// PROT 1205's party atlases.
    pub party_atlases: Vec<legaia_tim::Tim>,
    /// Each ladder fighter's own atlas, by roster id.
    pub fighter_tims: Vec<Option<legaia_tim::Tim>>,
    /// Stage-pack TMD `3` - the round-start cameo's ring girl
    /// ([`crate::baka_fighter_chrome::CAMEO_SCENE_MODEL`]).
    pub cameo: Option<(legaia_tmd::Tmd, Vec<u8>)>,
    /// The PROT 1203 clip bank the cameo's clips `0x1C` / `0x1D` resolve
    /// into (display id `k` = record `k - 1`).
    pub cameo_bank: Option<legaia_asset::player_anm::PlayerAnmBundle>,
    /// The overlay's two-record blit table (`&DAT_801DBE84`), the wink.
    pub blit_rects: Vec<bo::BakaBlitRect>,
    /// The stage pack's TMDs `1` and `2` - the impact pair's mesh parts
    /// ([`crate::baka_impact_fx`] template B), by `model - 1`.
    pub impact_models: [Option<(legaia_tmd::Tmd, Vec<u8>)>; 2],
}

impl BakaDuelAssets {
    /// Decode the duel's assets through `read_prot` (extraction PROT index
    /// -> raw entry bytes): the party pack (1204 + 1205), the art / stage /
    /// party-bank container (1203) and the fourteen ladder packs
    /// (1206..=1219). A piece that does not decode is left out.
    pub fn load(read_prot: impl Fn(usize) -> Option<Vec<u8>>) -> Self {
        let mut out = Self {
            fighters: vec![None; bo::OPPONENT_COUNT],
            fighter_tims: vec![None; bo::OPPONENT_COUNT],
            ..Self::default()
        };
        let art_entry = read_prot(bo::BAKA_HUD_ART_PROT_INDEX);
        if let Some(e) = art_entry.as_deref() {
            out.art = legaia_asset::minigame_art::parse_art_pack(e).unwrap_or_default();
            out.wall = stage_tmd(e, 0);
            out.cameo = stage_tmd(
                e,
                usize::from(crate::baka_fighter_chrome::CAMEO_SCENE_MODEL),
            );
            out.impact_models = [stage_tmd(e, 1), stage_tmd(e, 2)];
        }
        let party_bank = art_entry.as_deref().and_then(|e| {
            legaia_asset::player_anm::find_in_entry(e, 4)
                .into_iter()
                .next()
        });
        out.cameo_bank = party_bank.clone();
        if let Some(rec) = legaia_asset::static_overlay::overlay_map()
            .by_prot_index(bo::BAKA_OVERLAY_PROT_INDEX as u32)
            && let Some(raw) = read_prot(bo::BAKA_OVERLAY_PROT_INDEX)
            && let Ok(img) = legaia_asset::static_overlay::as_loaded(&raw, rec)
        {
            out.blit_rects = bo::parse_blit_rects(&img).unwrap_or_default();
        }
        let party_slots = read_prot(legaia_asset::battle_char_pack::PROT_ENTRY_INDEX as usize)
            .and_then(|raw| legaia_asset::battle_char_pack::parse_slots(&raw).ok());
        if let Some(raw) =
            read_prot(legaia_asset::battle_char_pack::ATLAS_PROT_ENTRY_INDEX as usize)
            && let Ok(atlases) = legaia_asset::battle_char_pack::parse_atlases(&raw)
        {
            out.party_atlases = atlases
                .iter()
                .filter_map(|a| legaia_tim::parse(&a.tim_bytes).ok())
                .collect();
        }
        for roster in 0..bo::OPPONENT_COUNT {
            if roster < bo::FIGHTER_PACK_FIRST_ROSTER_ID {
                let (Some(slots), Some(bank)) = (party_slots.as_ref(), party_bank.as_ref()) else {
                    continue;
                };
                let Some(slot) = slots.get(roster) else {
                    continue;
                };
                if let Ok(tmd) = legaia_tmd::parse(&slot.tmd_bytes) {
                    out.fighters[roster] = Some(DuelFighterAsset {
                        tmd,
                        raw: slot.tmd_bytes.clone(),
                        bank: bank.clone(),
                        first_record: roster * bo::ACTIONS_PER_FIGHTER,
                    });
                }
            } else {
                let Some(pack) = bo::fighter_pack_prot_index(roster)
                    .and_then(&read_prot)
                    .and_then(|e| bo::parse_fighter_pack(&e))
                else {
                    continue;
                };
                out.fighter_tims[roster] = legaia_tim::parse(&pack.tim_bytes).ok();
                if let (Ok(tmd), Ok(bank)) = (
                    legaia_tmd::parse(&pack.tmd_bytes),
                    legaia_asset::player_anm::parse(&pack.anim_bytes),
                ) {
                    out.fighters[roster] = Some(DuelFighterAsset {
                        tmd,
                        raw: pack.tmd_bytes,
                        bank,
                        first_record: 0,
                    });
                }
            }
        }
        out
    }

    /// The duel's VRAM with `opponent` seated: the art pages, the party
    /// atlases, then the opponent's own atlas last (the ladder packs past
    /// the first two share one page, loaded one at a time).
    pub fn vram(&self, opponent: usize) -> legaia_tim::Vram {
        let mut vram = legaia_tim::Vram::new();
        for tim in self.art.iter().chain(self.party_atlases.iter()) {
            vram.upload_tim(tim);
        }
        if let Some(Some(tim)) = self.fighter_tims.get(opponent) {
            vram.upload_tim(tim);
        }
        vram
    }

    /// Apply the cameo's blit for table row `index`
    /// ([`crate::baka_fighter_chrome::sprite_blit`]): a `MoveImage` of the
    /// stored eye cell into the live one. `false` when the table did not
    /// decode or has no such row.
    pub fn apply_wink(&self, vram: &mut legaia_tim::Vram, index: usize) -> bool {
        let Some(rect) = self.blit_rects.get(index) else {
            return false;
        };
        let raw = [
            ((rect.src_x - crate::baka_fighter_chrome::BLIT_SRC_X_BASE as u16) << 2) as u8,
            (rect.src_y - crate::baka_fighter_chrome::BLIT_SRC_Y_BASE as u16) as u8,
        ];
        let Some(b) = crate::baka_fighter_chrome::sprite_blit(0, raw) else {
            return false;
        };
        vram.move_image(
            b.src_x as u16,
            b.src_y as u16,
            b.size.0 as u16,
            b.size.1 as u16,
            b.dst_x as u16,
            b.dst_y as u16,
        );
        true
    }
}

/// One TMD out of the PROT 1203 stage pack (descriptor type `0x02`).
fn stage_tmd(entry: &[u8], index: usize) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
    use legaia_asset::{DecodeMode, decode, pack, parse_player_lzs};
    let container = parse_player_lzs(entry, 4).ok()?;
    let desc = container.descriptors.iter().find(|d| d.type_byte == 0x02)?;
    let body = decode(entry, desc, DecodeMode::Lzs).ok()?;
    let bodies = pack::extract_pack(&body).ok()?;
    let raw = bodies.get(index)?.to_vec();
    let tmd = legaia_tmd::parse(&raw).ok()?;
    Some((tmd, raw))
}

// ---------------------------------------------------------------- arena

/// A stage model's placement: origin and yaw.
pub type StagePlacement = ([i16; 3], i16);

/// Where the epilogue places stage model `0`: four copies, the model origin
/// and the yaw (`FUN_801D6D60` calls at `0x801D2108` / `0x801D2130` /
/// `0x801D215C` / `0x801D2184`).
pub const WALL_PLACEMENTS: [StagePlacement; 4] = [
    ([0, 0x64, 0x640], 0),
    ([0x640, 0x64, 0], 0x400),
    ([-0x640, 0x64, 0], -0x400),
    ([0, 0x64, -0x640], 0x800),
];

/// `FUN_801D6D60` draws a wall only when its origin's eye depth is past
/// this (`slti v0,v0,0x2711` at `0x801D6D98`) - the camera-side wall drops
/// out.
pub const WALL_CULL_DEPTH: f32 = 10001.0;

// ---------------------------------------------------------------- scene

/// The ghost passes' colour keep: the afterimage blends toward the black
/// colour word by depth-cue level `0x800` then `0xC00`.
const GHOST_KEEP: [f32; 2] = [0.5, 0.25];

/// World units a ghost pass sits behind its thrower per pass. Retail sorts
/// the ghosts `0x40` deeper in the ordering table; a depth-tested renderer
/// gets the same by setting each a little further from the eye.
const GHOST_SETBACK: f32 = 6.0;

/// The duel's combined vertex buffers and their per-frame pose.
///
/// Positions are raw retail world coordinates (Y down) at 1x; a host
/// multiplies them by [`DuelCamera::vp_raw`]. Every other attribute is
/// static for the scene's life.
#[derive(Debug, Clone)]
pub struct BakaDuelScene {
    roster: [usize; 2],
    /// Posed positions, rewritten by [`Self::pose`].
    pub positions: Vec<[f32; 3]>,
    base: Vec<[f32; 3]>,
    object_ids: Vec<u32>,
    pub uvs: Vec<[u8; 2]>,
    pub cba_tsb: Vec<[u16; 2]>,
    /// Texture modulation colour (the prim's packet colour; `0x80` neutral).
    pub colors: Vec<[u8; 3]>,
    /// `[r, g, b, flag]` per vertex: flag `255` textured (modulate), `0`
    /// untextured (fill) - `crate::packet_color::hybrid`'s layout.
    pub flat_rgba: Vec<u8>,
    /// Every triangle.
    pub indices: Vec<u32>,
    /// The textured triangles only.
    pub textured_indices: Vec<u32>,
    /// The untextured triangles only.
    pub untextured_indices: Vec<u32>,
    /// The build-time `colors` / `flat_rgba` / `cba_tsb`, the depth cue's
    /// and colour word's source for the impact mesh instances.
    base_colors: Vec<[u8; 3]>,
    base_flat: Vec<u8>,
    base_cba_tsb: Vec<[u16; 2]>,
    fighter: [Range<usize>; 2],
    ghost: [[Range<usize>; 2]; 2],
    cameo: Range<usize>,
    walls: Vec<(Range<usize>, StagePlacement)>,
    /// Sprite-arm quad instances, four vertices each.
    impact_sprites: Vec<Range<usize>>,
    /// Mesh-part instances: `(model, range)`.
    impact_meshes: Vec<(usize, Range<usize>)>,
    /// Bumped whenever a pose rewrote `uvs` / `cba_tsb` / `colors` /
    /// `flat_rgba` (the impact parts animate them).
    attr_generation: u32,
}

/// Sprite-arm quad instances the scene reserves. A decided exchange seats
/// one flash, a draw two, and each lives about thirty ticks.
pub const IMPACT_SPRITE_SLOTS: usize = 4;
/// Mesh-part instances reserved per impact model.
pub const IMPACT_MESH_SLOTS: usize = 2;

impl BakaDuelScene {
    /// Build the buffers for `player` vs `opponent` (roster ids).
    pub fn build(assets: &BakaDuelAssets, player: usize, opponent: usize) -> Option<Self> {
        let p = assets.fighters.get(player)?.as_ref()?;
        let o = assets.fighters.get(opponent)?.as_ref()?;
        let mut s = Self {
            roster: [player, opponent],
            positions: Vec::new(),
            base: Vec::new(),
            object_ids: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            colors: Vec::new(),
            flat_rgba: Vec::new(),
            indices: Vec::new(),
            textured_indices: Vec::new(),
            untextured_indices: Vec::new(),
            base_colors: Vec::new(),
            base_flat: Vec::new(),
            base_cba_tsb: Vec::new(),
            fighter: [0..0, 0..0],
            ghost: [[0..0, 0..0], [0..0, 0..0]],
            cameo: 0..0,
            walls: Vec::new(),
            impact_sprites: Vec::new(),
            impact_meshes: Vec::new(),
            attr_generation: 0,
        };
        s.fighter[0] = s.push_tmd(&p.tmd, &p.raw, 1.0);
        s.fighter[1] = s.push_tmd(&o.tmd, &o.raw, 1.0);
        for (k, keep) in GHOST_KEEP.iter().enumerate() {
            s.ghost[0][k] = s.push_tmd(&p.tmd, &p.raw, *keep);
            s.ghost[1][k] = s.push_tmd(&o.tmd, &o.raw, *keep);
        }
        if let Some((tmd, raw)) = assets.wall.as_ref() {
            for placement in WALL_PLACEMENTS {
                let r = s.push_tmd(tmd, raw, 1.0);
                s.walls.push((r, placement));
            }
        }
        if let Some((tmd, raw)) = assets.cameo.as_ref() {
            s.cameo = s.push_tmd(tmd, raw, 1.0);
        }
        s.push_floor();
        for _ in 0..IMPACT_SPRITE_SLOTS {
            let r = s.push_quad();
            s.impact_sprites.push(r);
        }
        for (k, m) in assets.impact_models.iter().enumerate() {
            let Some((tmd, raw)) = m.as_ref() else {
                continue;
            };
            for _ in 0..IMPACT_MESH_SLOTS {
                let r = s.push_tmd(tmd, raw, 1.0);
                s.impact_meshes.push((k + 1, r));
            }
        }
        s.base_colors = s.colors.clone();
        s.base_flat = s.flat_rgba.clone();
        s.base_cba_tsb = s.cba_tsb.clone();
        s.positions = s.base.clone();
        Some(s)
    }

    /// The `(player, opponent)` roster pair the buffers hold.
    pub fn roster(&self) -> [usize; 2] {
        self.roster
    }

    /// Moves whenever a pose rewrote the per-vertex attributes (`uvs`,
    /// `cba_tsb`, `colors`, `flat_rgba`) - the impact parts' flip-book cells
    /// and fades. A host that re-uploads those only on a
    /// [`BakaDuelSurface::generation`] change re-reads them on this one too.
    pub fn attr_generation(&self) -> u32 {
        self.attr_generation
    }

    /// One textured quad's worth of placeholder vertices (two triangles),
    /// for an impact sprite instance; [`Self::pose_impact`] fills it.
    fn push_quad(&mut self) -> Range<usize> {
        let start = self.base.len();
        for _ in 0..4 {
            self.base.push([0.0; 3]);
            self.object_ids.push(u32::MAX);
            self.uvs.push([0, 0]);
            self.cba_tsb.push([0, 0]);
            self.colors.push([0x80; 3]);
            self.flat_rgba.extend_from_slice(&[0x80, 0x80, 0x80, 255]);
        }
        let b = start as u32;
        for t in [[b, b + 1, b + 2], [b + 1, b + 3, b + 2]] {
            self.indices.extend_from_slice(&t);
            self.textured_indices.extend_from_slice(&t);
        }
        start..self.base.len()
    }

    /// Place this frame's impact parts into their reserved ranges and drop
    /// the unused ones; bumps [`Self::attr_generation`] when an attribute
    /// changed.
    ///
    /// Each draw sits at the part's position turned by its yaw bank and
    /// scaled by `+0x72 / 0x1000` - the kind-4 / mesh arm of the render
    /// dispatcher (`FUN_8001ADA4` `0x8001B240..0x8001B2C4`). The X and Z
    /// banks are not applied: no duel template writes them. Every prim takes
    /// the part's colour word the way `FUN_80043390` applies it - the ABE
    /// bit forced on, the ABR mode ORed into the tpage, and the packet colour
    /// depth-cued toward the word's far colour
    /// ([`crate::baka_impact_fx::ColourWord::cue`]).
    fn pose_impact(&mut self, fight: &BakaFight) {
        use crate::baka_impact_fx::ImpactDraw;
        let draws = fight.impact_fx().draws();
        let mut sprite_used = 0;
        let mut mesh_used = vec![false; self.impact_meshes.len()];
        let mut changed = false;
        for d in draws {
            match d {
                ImpactDraw::Sprite {
                    quad,
                    pos,
                    rot,
                    scale,
                    colour,
                } => {
                    let Some(range) = self.impact_sprites.get(sprite_used).cloned() else {
                        continue;
                    };
                    sprite_used += 1;
                    let (sy, cy) = angle(i32::from(rot[1])).sin_cos();
                    let k = f32::from(scale) / 4096.0;
                    let rgb = colour.cue(quad.rgb);
                    let tsb = impact_tsb(quad.tpage, colour);
                    for (j, i) in range.enumerate() {
                        let v = quad.verts[j].map(|c| f32::from(c) * k);
                        self.positions[i] = [
                            v[0] * cy + v[2] * sy + f32::from(pos[0]),
                            v[1] + f32::from(pos[1]),
                            -v[0] * sy + v[2] * cy + f32::from(pos[2]),
                        ];
                        changed |= set(&mut self.uvs[i], quad.uvs[j]);
                        changed |= set(&mut self.cba_tsb[i], [quad.clut, tsb]);
                        changed |= set(&mut self.colors[i], rgb);
                        let flat = [rgb[0], rgb[1], rgb[2], 255];
                        let dst: &mut [u8; 4] = (&mut self.flat_rgba[i * 4..i * 4 + 4])
                            .try_into()
                            .expect("four bytes");
                        changed |= set(dst, flat);
                    }
                }
                ImpactDraw::Mesh {
                    model,
                    pos,
                    rot,
                    scale,
                    colour,
                } => {
                    let Some(slot) = self
                        .impact_meshes
                        .iter()
                        .enumerate()
                        .position(|(n, (m, _))| *m == model && !mesh_used[n])
                    else {
                        continue;
                    };
                    mesh_used[slot] = true;
                    let range = self.impact_meshes[slot].1.clone();
                    let (sy, cy) = angle(i32::from(rot[1])).sin_cos();
                    let k = f32::from(scale) / 4096.0;
                    for i in range {
                        let v = self.base[i].map(|c| c * k);
                        self.positions[i] = [
                            v[0] * cy + v[2] * sy + f32::from(pos[0]),
                            v[1] + f32::from(pos[1]),
                            -v[0] * sy + v[2] * cy + f32::from(pos[2]),
                        ];
                        let [cba, tsb] = self.base_cba_tsb[i];
                        changed |= set(&mut self.cba_tsb[i], [cba, impact_tsb(tsb, colour)]);
                        changed |= set(&mut self.colors[i], colour.cue(self.base_colors[i]));
                        let b = &self.base_flat[i * 4..i * 4 + 4];
                        let c = colour.cue([b[0], b[1], b[2]]);
                        let dst: &mut [u8; 4] = (&mut self.flat_rgba[i * 4..i * 4 + 4])
                            .try_into()
                            .expect("four bytes");
                        changed |= set(dst, [c[0], c[1], c[2], b[3]]);
                    }
                }
            }
        }
        for n in sprite_used..self.impact_sprites.len() {
            let r = self.impact_sprites[n].clone();
            self.collapse(r);
        }
        for (n, used) in mesh_used.iter().enumerate() {
            if !used {
                let r = self.impact_meshes[n].1.clone();
                self.collapse(r);
            }
        }
        if changed {
            self.attr_generation = self.attr_generation.wrapping_add(1);
        }
    }

    /// Append one TMD's hybrid mesh with its packet colours scaled by `keep`.
    fn push_tmd(&mut self, tmd: &legaia_tmd::Tmd, raw: &[u8], keep: f32) -> Range<usize> {
        let (mesh, oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(tmd, raw);
        let flat = crate::packet_color::hybrid(&mesh, &shading);
        let start = self.base.len();
        let scale = |c: u8| (f32::from(c) * keep).round().clamp(0.0, 255.0) as u8;
        self.base.extend_from_slice(&mesh.positions);
        self.object_ids.extend_from_slice(&oids);
        self.uvs.extend_from_slice(&mesh.uvs);
        self.cba_tsb.extend_from_slice(&mesh.cba_tsb);
        self.colors.extend(
            mesh.colors
                .iter()
                .map(|c| [scale(c[0]), scale(c[1]), scale(c[2])]),
        );
        for px in flat.as_chunks::<4>().0 {
            self.flat_rgba
                .extend_from_slice(&[scale(px[0]), scale(px[1]), scale(px[2]), px[3]]);
        }
        for tri in mesh.indices.as_chunks::<3>().0 {
            let t = [
                tri[0] + start as u32,
                tri[1] + start as u32,
                tri[2] + start as u32,
            ];
            self.indices.extend_from_slice(&t);
            let textured = shading.textured.get(tri[0] as usize).copied().unwrap_or(1) != 0;
            if textured {
                self.textured_indices.extend_from_slice(&t);
            } else {
                self.untextured_indices.extend_from_slice(&t);
            }
        }
        start..self.base.len()
    }

    /// The floor grid `FUN_801CEB84` emits - the battle ground grid's own
    /// routine relocated into the duel overlay, so the port is the same
    /// builder ([`legaia_asset::battle_backdrop::build_ground_grid_sized`])
    /// over [`crate::mode_entry_init::duel_overlay_init`]'s window tiles (the
    /// `0x1F8003F8` / `0x1F8003FA` pair the emitter loops over).
    fn push_floor(&mut self) {
        let (w, h) = crate::mode_entry_init::duel_overlay_init().window_tiles;
        let grid =
            legaia_asset::battle_backdrop::build_ground_grid_sized(i32::from(w), i32::from(h));
        let start = self.base.len() as u32;
        self.base.extend_from_slice(&grid.positions);
        self.object_ids
            .extend(std::iter::repeat_n(u32::MAX, grid.positions.len()));
        self.uvs.extend_from_slice(&grid.uvs);
        self.cba_tsb.extend_from_slice(&grid.cba_tsb);
        self.colors.extend_from_slice(&grid.colors);
        for c in &grid.colors {
            self.flat_rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        for &i in &grid.indices {
            self.indices.push(start + i);
            self.textured_indices.push(start + i);
        }
    }

    /// Pose every dynamic range for this frame of `fight`.
    pub fn pose(&mut self, assets: &BakaDuelAssets, fight: &BakaFight) {
        let camera = fight.duel_camera();
        let view = camera.view();
        // Eye-forward in world space, for the ghost setback.
        let r = camera_rotation(view.pitch, view.yaw, view.roll);
        let fwd = [r[2], r[6], r[10]];
        for slot in 0..2 {
            let Some(asset) = assets
                .fighters
                .get(self.roster[slot])
                .and_then(|a| a.as_ref())
            else {
                continue;
            };
            let m = fight.motion(slot);
            let origin = fight.fighter_position(slot);
            let yaw = fight.fighter_yaw(slot);
            let range = self.fighter[slot].clone();
            let rec = asset.first_record + m.record;
            self.pose_range(&asset.tmd, &asset.bank, rec, m.frame(), range, origin, yaw);
            let mut drawn = [false; 2];
            for (owner, frame) in fight.afterimages() {
                if *owner != slot {
                    continue;
                }
                for pass in frame.passes.iter().filter(|p| p.drawn) {
                    let k = (pass.bit & 1) as usize;
                    let back = GHOST_SETBACK * (k + 1) as f32;
                    let o = [
                        origin[0] + fwd[0] * back,
                        origin[1] + fwd[1] * back,
                        origin[2] + fwd[2] * back,
                    ];
                    let f = (i32::from(pass.cursor).max(0) >> 4) as usize;
                    let range = self.ghost[slot][k].clone();
                    let rec = asset.first_record + bo::ACTION_SPECIAL;
                    self.pose_range(&asset.tmd, &asset.bank, rec, f, range, o, yaw);
                    drawn[k] = true;
                }
            }
            for (k, d) in drawn.iter().enumerate() {
                if !d {
                    let range = self.ghost[slot][k].clone();
                    self.collapse(range);
                }
            }
        }
        self.pose_cameo(assets, fight, &view, &r);
        for i in 0..self.walls.len() {
            let (range, (pos, yaw)) = self.walls[i].clone();
            let (base, out) = (&self.base[range.clone()], &mut self.positions[range]);
            place_stage_model(camera, pos, yaw, base, out);
        }
        self.pose_impact(fight);
    }

    /// Pose the round-start cameo, or drop it when none is on stage. The
    /// actor is camera-relative (`+0x52 & 0x400`): `FUN_8001CF50` loads the
    /// base matrix alone (`0x8001D018`) instead of the camera rotation, so
    /// its eye position is `6 * (Ry(yaw) . pose(v) + pos)`, which this maps
    /// back into the world frame the scene's view-projection takes.
    fn pose_cameo(
        &mut self,
        assets: &BakaDuelAssets,
        fight: &BakaFight,
        view: &FieldCameraView,
        r: &[f32; 16],
    ) {
        let range = self.cameo.clone();
        let (Some(actor), Some((tmd, _)), Some(bank)) = (
            fight.cameo(),
            assets.cameo.as_ref(),
            assets.cameo_bank.as_ref(),
        ) else {
            self.collapse(range);
            return;
        };
        let Some(pose) = actor.pose() else {
            self.collapse(range);
            return;
        };
        let record = (pose.clip.max(1) - 1) as usize;
        let frames = bank
            .record(record)
            .map(|h| usize::from(h.frame_count))
            .unwrap_or(1)
            .max(1);
        let raw = (actor.cursor.max(0) >> 4) as usize;
        let frame = if pose.hold_last_frame {
            raw.min(frames - 1)
        } else {
            raw % frames
        };
        let origin = [f32::from(pose.x), f32::from(pose.y), f32::from(pose.z)];
        let yaw = i32::from(pose.yaw);
        self.pose_range(tmd, bank, record, frame, range.clone(), origin, yaw);
        // eye/6 -> world: p = focus + R^T (q - tr/6).
        for p in &mut self.positions[range] {
            let d = [
                p[0] - view.tr_eye[0],
                p[1] - view.tr_eye[1],
                p[2] - view.tr_eye[2],
            ];
            let mut w = view.focus;
            for (i, wi) in w.iter_mut().enumerate() {
                for (j, dj) in d.iter().enumerate() {
                    *wi += r[i * 4 + j] * dj;
                }
            }
            *p = w;
        }
    }

    fn collapse(&mut self, range: Range<usize>) {
        for p in &mut self.positions[range] {
            *p = [0.0; 3];
        }
    }

    /// Pose one fighter range: per object `Rz.Ry.Rx . v + T` from the bank
    /// record at `frame`, then the actor's yaw about Y and its position.
    #[allow(clippy::too_many_arguments)]
    fn pose_range(
        &mut self,
        tmd: &legaia_tmd::Tmd,
        bank: &legaia_asset::player_anm::PlayerAnmBundle,
        record: usize,
        frame: usize,
        range: Range<usize>,
        origin: [f32; 3],
        yaw: i32,
    ) {
        let frames = bank
            .record(record)
            .map(|r| usize::from(r.frame_count))
            .unwrap_or(0);
        let frame = frame.min(frames.saturating_sub(1));
        let parts = tmd.objects.len();
        let xf: Vec<Option<[f32; 9]>> = (0..parts)
            .map(|p| {
                let t = bank.bone_transform(record, frame, p)?;
                let (sx, cx) = angle(t.r_x).sin_cos();
                let (sy, cy) = angle(t.r_y).sin_cos();
                let (sz, cz) = angle(t.r_z).sin_cos();
                Some([
                    cx,
                    sx,
                    cy,
                    sy,
                    cz,
                    sz,
                    t.t_x as f32,
                    t.t_y as f32,
                    t.t_z as f32,
                ])
            })
            .collect();
        let (wsy, wcy) = angle(yaw).sin_cos();
        for i in range {
            let [mut x, mut y, mut z] = self.base[i];
            if let Some(Some(t)) = xf.get(self.object_ids[i] as usize) {
                let [cx, sx, cy, sy, cz, sz, tx, ty, tz] = *t;
                let ny = y * cx - z * sx;
                let nz = y * sx + z * cx;
                y = ny;
                z = nz;
                let nx = x * cy + z * sy;
                let nz = -x * sy + z * cy;
                x = nx;
                z = nz;
                let nx = x * cz - y * sz;
                let ny = x * sz + y * cz;
                x = nx + tx;
                y = ny + ty;
                z += tz;
            }
            self.positions[i] = [
                x * wcy + z * wsy + origin[0],
                y + origin[1],
                -x * wsy + z * wcy + origin[2],
            ];
        }
    }
}

/// Place one stage model for this frame, or drop it: the model at `pos`
/// turned `yaw` about Y, drawn only while the camera sees its origin past
/// [`WALL_CULL_DEPTH`]. A dropped model's vertices collapse to a point.
/// Returns whether it drew.
///
/// PORT: FUN_801D6D60 - `FUN_8003D344` moves the position through the
/// camera (`0x801D6D88`), the depth test rejects below `0x2711`
/// (`0x801D6D98`), `FUN_8004629C` (`RotMatrixY`) turns the model by the
/// rotation argument's `Y` (`0x801D6DB0`), and `FUN_80043390` draws scene
/// model `DAT_8007C018[_DAT_8007B6F8]` (`0x801D6DE0`) - the model the
/// epilogue's four calls place as the arena walls ([`WALL_PLACEMENTS`]).
pub fn place_stage_model(
    camera: &DuelCamera,
    pos: [i16; 3],
    yaw: i16,
    model: &[[f32; 3]],
    out: &mut [[f32; 3]],
) -> bool {
    let origin = [f32::from(pos[0]), f32::from(pos[1]), f32::from(pos[2])];
    if camera.eye_depth(origin) < WALL_CULL_DEPTH {
        out.iter_mut().for_each(|p| *p = [0.0; 3]);
        return false;
    }
    let (sy, cy) = angle(i32::from(yaw)).sin_cos();
    for (o, v) in out.iter_mut().zip(model) {
        *o = [
            v[0] * cy + v[2] * sy + origin[0],
            v[1] + origin[1],
            -v[0] * sy + v[2] * cy + origin[2],
        ];
    }
    true
}

/// Store `v` into `dst`, reporting whether it changed.
fn set<T: PartialEq + Copy>(dst: &mut T, v: T) -> bool {
    let changed = *dst != v;
    *dst = v;
    changed
}

/// A prim's TSB word under an impact part's colour word: the ABE bit forced
/// on when the word's bit 31 is set (`FUN_80043390` ORs it into every
/// packet's code) and the word's ABR mode ORed into the tpage's bits 5..6.
fn impact_tsb(tsb: u16, colour: crate::baka_impact_fx::ColourWord) -> u16 {
    let t = tsb | (u16::from(colour.abr) << 5);
    if colour.semi {
        legaia_tmd::mesh::pack_tsb_semi(t, true)
    } else {
        t
    }
}

/// PSX 12-bit angle to radians.
fn angle(a: i32) -> f32 {
    a as f32 / 4096.0 * std::f32::consts::TAU
}

// ---------------------------------------------------------------- host cache

/// The per-host cache every duel host drives once a frame: the assets
/// (decoded on first use), the buffers for the seated pair, and a generation
/// that moves whenever the static buffers or the VRAM change.
#[derive(Debug, Default)]
pub struct BakaDuelSurface {
    assets: Option<Arc<BakaDuelAssets>>,
    scene: Option<BakaDuelScene>,
    generation: u32,
    /// The blit row the cameo's cell last showed; a change is a VRAM edit
    /// the host must re-upload (a new generation).
    wink: Option<usize>,
}

impl BakaDuelSurface {
    /// One frame. `None` - and the cached buffers dropped - when no duel is
    /// live. The assets load once through `read_prot`; the buffers rebuild
    /// when the seated roster pair changes (a rung install), which bumps
    /// [`Self::generation`]; the pose runs every call.
    pub fn frame(
        &mut self,
        read_prot: impl Fn(usize) -> Option<Vec<u8>>,
        fight: Option<&BakaFight>,
    ) -> Option<&BakaDuelScene> {
        let Some(fight) = fight else {
            if self.scene.take().is_some() {
                self.generation = self.generation.wrapping_add(1);
            }
            return None;
        };
        let assets = self
            .assets
            .get_or_insert_with(|| Arc::new(BakaDuelAssets::load(read_prot)))
            .clone();
        let want = [fight.player_roster(), fight.opponent_roster()];
        if self.scene.as_ref().map(|s| s.roster()) != Some(want) {
            self.scene = BakaDuelScene::build(&assets, want[0], want[1]);
            self.generation = self.generation.wrapping_add(1);
        }
        let wink = fight.cameo().and_then(|c| c.pose()).map(|p| p.blit_index);
        if wink.is_some() && wink != self.wink {
            self.wink = wink;
            self.generation = self.generation.wrapping_add(1);
        }
        let scene = self.scene.as_mut()?;
        scene.pose(&assets, fight);
        Some(scene)
    }

    /// Bumped whenever the static buffers or the VRAM a host holds are stale.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The live scene, as the last [`Self::frame`] posed it.
    pub fn scene(&self) -> Option<&BakaDuelScene> {
        self.scene.as_ref()
    }

    /// The duel VRAM for the seated opponent.
    pub fn vram(&self) -> Option<legaia_tim::Vram> {
        let s = self.scene.as_ref()?;
        let assets = self.assets.as_ref()?;
        let mut vram = assets.vram(s.roster[1]);
        if let Some(i) = self.wink {
            assets.apply_wink(&mut vram, i);
        }
        Some(vram)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_round_spin_settles_on_the_duel_camera() {
        let mut c = DuelCamera::default();
        assert_eq!(c.globals(), ROUND_SETUP_CAMERA);
        for _ in 0..400 {
            c.tick(1);
        }
        assert!(!c.moving(), "spin and glide both finish");
        assert_eq!(c.globals(), DUEL_CAMERA);
    }

    #[test]
    fn the_spin_stops_past_a_full_turn() {
        let mut c = DuelCamera::default();
        let mut frames = 0;
        while c.sweeping {
            c.tick(1);
            frames += 1;
        }
        // (0x1001 - 0x2F8) / 0x40 rounded up.
        assert_eq!(frames, (SWEEP_END_YAW - 0x2F8 + 0x3F) / 0x40);
        assert_eq!(c.globals().0[1], 0);
    }

    #[test]
    fn an_opponent_special_glides_to_its_fixed_pose() {
        let mut c = DuelCamera::default();
        for _ in 0..400 {
            c.tick(1);
        }
        c.arm_glide(&OPPONENT_SPECIAL_GLIDE);
        for _ in 0..600 {
            c.tick(1);
        }
        let g = c.globals().0;
        assert_eq!((g[0] as i16, g[1] as i16), (-0x14, 0xA8C));
        assert_eq!([g[3], g[4], g[5]], [-0x3C, 0x80C, 0x2120]);
    }

    #[test]
    fn motion_plays_an_attack_out_then_idles() {
        let clip = ClipHeader::from_record_words(0, 4, 1);
        let mut m = FighterMotion::default();
        m.play(1, false);
        let mut ticks = 0;
        while m.record == 1 {
            m.step(16, 8, Some(clip), 1);
            ticks += 1;
            assert!(ticks < 100);
        }
        // 4 frames at a whole frame per tick: the selector reports the end on
        // the 4th step, the idle reset lands on the 5th.
        assert_eq!(ticks, 5);
        assert_eq!(m.record, 0);
    }

    #[test]
    fn a_held_knockdown_stays_down() {
        let clip = ClipHeader::from_record_words(0, 3, 1);
        let mut m = FighterMotion::default();
        m.play(MOTION_KNOCKDOWN, true);
        m.down = true;
        for _ in 0..20 {
            m.step(16, 8, Some(clip), 1);
        }
        assert_eq!(m.record, MOTION_KNOCKDOWN);
        assert_eq!(m.frame(), 2, "held on the last frame");
    }

    #[test]
    fn special_camera_rows_read_targets_then_steps() {
        let mut img = vec![0u8; 0xA000];
        let base = (SPECIAL_CAMERA_TABLE_VA - bo::BAKA_OVERLAY_BASE_VA) as usize;
        let row: [i16; 16] = [-2, 300, 5, 0, 1, 7, 3, 0, -40, 900, 4000, 0, 2, 11, 21, 0];
        for (i, v) in row.iter().enumerate() {
            img[base + i * 2..base + i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        let rows = parse_special_cameras(&img);
        assert_eq!(rows.len(), SPECIAL_CAMERA_ROWS);
        assert_eq!(
            &rows[0][..12],
            &[1, -2, 7, 300, 3, 5, 2, -40, 11, 900, 21, 4000]
        );
    }
}
