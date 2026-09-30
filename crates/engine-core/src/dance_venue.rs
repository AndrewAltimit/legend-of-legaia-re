//! The dance **venue**: what the dance overlay's own init `FUN_801CEF54`
//! stages around the beat clock, and what its teardown `FUN_801D414C` puts
//! back.
//!
//! Retail's dance *is* a scene: the mode-24 init switch `FUN_80025980` hands
//! arm 6 to `FUN_801CEF54`, which saves the caller's scene-name buffer and
//! PROT block base, loads the venue's field file (the `other7` block at raw
//! TOC index `0x4CC`), seats the camera on the floor and pre-stamps the four
//! dancer face rigs into VRAM. The port keeps the walked-in scene resident
//! underneath (its actors, VM and field state are not torn down), so the
//! save / restore pair becomes two halves here:
//!
//! * [`sync_dance_venue`] - the **globals**: on the first frame the world is
//!   in [`SceneMode::Dance`] it writes the record's block base into the
//!   `_DAT_80084540` mirror, the view window into the camera's visible-tile
//!   window, and publishes the record's camera pose; on the first frame it is
//!   not, it restores the saved block base and window. One call per host
//!   frame, on both play hosts.
//! * [`DanceVenue::build`] - the **venue itself**: the scene the record's
//!   stream id names, its resources, its placed / terrain draw list, the HUD
//!   page and the five entry face stamps, already applied to the venue VRAM.
//!   The native window builds it on the staging edge and draws it in place of
//!   the walked-in scene; the browser pages build it once per disc through
//!   the same call.
//!
//! See `docs/subsystems/minigame-dance.md` § Entering and leaving the hall.

use crate::camera::Camera;
use crate::dance::{DanceMode, DanceSceneEntry, dance_face_rig, dance_scene_entry};
use crate::field_env::{self, EnvDraw};
use crate::scene::{ProtIndex, Scene};
use crate::scene_resources::{BuildOptions, SceneLoadKind, SceneResources};
use crate::world::{SceneMode, World};
use legaia_asset::dance_art::{self, FaceRig};
use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_engine_vm::psx_camera::FieldCameraView;
use legaia_tim::Vram;

/// The world scale retail folds into the camera rotation under game mode 24:
/// the mode initialiser `FUN_8001DCF8` loads `0x6000` into the diagonal of
/// `_DAT_8007BF10` for mode `2` **and** mode `0x18`
/// (`0x8001DF4C..0x8001DF70`), the same 6x the field frames through.
const VENUE_WORLD_SCALE: f32 = crate::camera_view::CUTSCENE_WORLD_SCALE;

fn angle_rad(units: u16) -> f32 {
    f32::from(units as i16) / 4096.0 * std::f32::consts::TAU
}

/// The dance camera the entry stages, as the port's field-frame pose.
///
/// Angles, `H` and the eye-space translation are the record's stores
/// ([`DanceSceneEntry::camera_angles`], [`DanceSceneEntry::gte_h`],
/// [`DanceSceneEntry::camera_pair`] with `+0x0` cleared). The focus is the
/// beat-clock actor's position: its tick writes the negated `+0x14` / `+0x18`
/// into the focus trio every frame, and the Y global keeps the `0` the mode
/// initialiser cleared it to (`0x8001E154`). The eye trio is reduced by the
/// 6x world scale the way every field pose is.
pub fn venue_camera(e: &DanceSceneEntry) -> FieldCameraView {
    let (sx, _, sz) = e.dancer_spawn;
    FieldCameraView {
        focus: [f32::from(sx), 0.0, f32::from(sz)],
        pitch: angle_rad(e.camera_angles.0),
        yaw: angle_rad(e.camera_angles.1),
        roll: angle_rad(e.camera_angles.2),
        h: f32::from(e.gte_h),
        tr_eye: [
            0.0,
            e.camera_pair.0 as f32 / VENUE_WORLD_SCALE,
            e.camera_pair.1 as f32 / VENUE_WORLD_SCALE,
        ],
    }
}

// --- the camera keyframe track (FUN_801CF470, 0x801CF51C..0x801CF7D8) ----

/// Overlay VA of the camera pose records: 8-byte `[i16 x, i16 y, i16 z,
/// i16 pad]` records read in pairs, record `2k` the angle trio and record
/// `2k + 1` the eye-space trio of pose `k` (`addiu t0,v0,0x43a0` at
/// `0x801CF67C`). Pose 0 is the entry's own stores.
pub const DANCE_CAMERA_POSES_VA: u32 = 0x801D_43A0;

/// Overlay VA of the qualifier's key table - `u32` pose indices, one per key
/// (`addiu a0,a0,0x4440` at `0x801CF604`, taken while `DAT_801D514C == 0`).
pub const DANCE_CAMERA_KEYS_QUALIFIER_VA: u32 = 0x801D_4440;

/// Overlay VA of the key table every other mode reads (`addiu a0,a0,0x4488`
/// at `0x801CF60C`).
pub const DANCE_CAMERA_KEYS_VA: u32 = 0x801D_4488;

/// Frames one key segment lasts: the timer `DAT_801D533C` reloads `0x151`
/// when it goes negative (`li v1,0x151` at `0x801CF540`).
pub const DANCE_CAMERA_SEGMENT: i32 = 0x151;

/// The key index wraps back to `1` - never `0` - on reaching this
/// (`slti v0,v0,0xd` at `0x801CF550` and `0x801CF574`), so key 0 plays only
/// on the first pass: the track opens on pose `tbl[0]` and then cycles keys
/// `1..=12`.
pub const DANCE_CAMERA_KEY_WRAP: i32 = 13;

/// Entries of the ease table the entry builds at `DAT_801D583C`
/// (`FUN_801CEF54`, `0x801CEF98..0x801CF054`): a 1024-step half-cosine
/// rise from `0` to `0x1000`, then 32 entries held at `0x1000`.
pub const DANCE_CAMERA_EASE_LEN: usize = 0x420;

/// The ease table `FUN_801CEF54` builds into BSS at `DAT_801D583C` out of
/// the SCUS sine table (`*_DAT_8007B81C`, 4096 steps a turn), stepping it two
/// entries at a time:
///
/// - `ease[i] = (sin[0xC00 + 2i] + 0x1000) / 2` for `i < 0x200`
///   (`lh v0,0x1800(a0)` over a 4-byte stride, `0x801CEFAC..0x801CEFD4`);
/// - `ease[0x200 + i] = sin[2i] / 2 + 0x800` for `i < 0x200`
///   (`0x801CEFF4..0x801CF024`);
/// - `ease[0x400..0x420] = 0x1000` (`0x801CF040..0x801CF050`).
///
/// Both halvings truncate toward zero (`srl 31` / `addu` / `sra 1`).
pub fn dance_camera_ease_table() -> Vec<i16> {
    use legaia_engine_vm::battle_action::motion::sin12;
    let half = |v: i32| ((v + ((v as u32 >> 31) as i32)) >> 1) as i16;
    let mut t = Vec::with_capacity(DANCE_CAMERA_EASE_LEN);
    for i in 0..0x200u16 {
        t.push(half(i32::from(sin12(0xC00 + 2 * i)) + 0x1000));
    }
    for i in 0..0x200u16 {
        t.push(half(i32::from(sin12(2 * i))) + 0x800);
    }
    t.resize(DANCE_CAMERA_EASE_LEN, 0x1000);
    t
}

/// One camera pose: the `_DAT_8007B790` angle trio and the `0x800840B8`
/// eye-space trio the track writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceCameraPose {
    pub angles: [i16; 3],
    pub eye: [i32; 3],
}

/// The dance tick's camera keyframe track: the key tables and pose records
/// out of the overlay image, the entry-built ease table, and the two
/// counters the entry seeds (`sw zero,0x533c` / `sw v0(-1),0x5338` at
/// `0x801CF348..0x801CF350`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanceCameraTrack {
    /// Every pose record the key tables reach, `[x, y, z]`.
    records: Vec<[i16; 3]>,
    /// `[qualifier, other]` key tables, keys `0..13`.
    keys: [[u32; DANCE_CAMERA_KEY_WRAP as usize]; 2],
    ease: Vec<i16>,
    /// `DAT_801D533C`.
    timer: i32,
    /// `DAT_801D5338`.
    key: i32,
}

impl DanceCameraTrack {
    /// Parse the track out of the dance overlay (PROT 0980) in its loaded
    /// form (file offset = VA - `0x801CE818`). `None` when a table or a
    /// record it names falls outside the image.
    pub fn from_overlay(overlay: &[u8]) -> Option<Self> {
        let base = legaia_asset::dance_chart::DANCE_OVERLAY_BASE_VA;
        let word = |va: u32| -> Option<u32> {
            let o = va.checked_sub(base)? as usize;
            Some(u32::from_le_bytes(overlay.get(o..o + 4)?.try_into().ok()?))
        };
        let mut keys = [[0u32; DANCE_CAMERA_KEY_WRAP as usize]; 2];
        for (t, va) in [DANCE_CAMERA_KEYS_QUALIFIER_VA, DANCE_CAMERA_KEYS_VA]
            .into_iter()
            .enumerate()
        {
            for (k, slot) in keys[t].iter_mut().enumerate() {
                *slot = word(va + 4 * k as u32)?;
            }
        }
        let max_pose = keys.iter().flatten().copied().max()?;
        // A pose index past this is not a table the retail track reads.
        if max_pose > 0x100 {
            return None;
        }
        let n = 2 * (max_pose as usize + 1);
        let o = (DANCE_CAMERA_POSES_VA - base) as usize;
        let bytes = overlay.get(o..o + n * 8)?;
        let h = |r: &[u8], i: usize| i16::from_le_bytes([r[i], r[i + 1]]);
        let records = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|r| [h(r, 0), h(r, 2), h(r, 4)])
            .collect();
        Some(Self {
            records,
            keys,
            ease: dance_camera_ease_table(),
            timer: 0,
            key: -1,
        })
    }

    /// The current key (`DAT_801D5338`) and segment timer (`DAT_801D533C`).
    pub fn counters(&self) -> (i32, i32) {
        (self.key, self.timer)
    }

    /// The pose key `key` of `mode`'s table names, un-interpolated - what the
    /// track sits on exactly at the end of the segment that eases into it.
    pub fn key_pose(&self, mode: DanceMode, key: usize) -> Option<DanceCameraPose> {
        let table = &self.keys[usize::from(mode != DanceMode::Qualifier)];
        let (a, e) = self.record_pose(*table.get(key)?)?;
        Some(DanceCameraPose {
            angles: a,
            eye: e.map(i32::from),
        })
    }

    /// Pose `k`'s angle and eye records, as `lh` reads them.
    fn record_pose(&self, k: u32) -> Option<([i16; 3], [i16; 3])> {
        let a = *self.records.get(2 * k as usize)?;
        let e = *self.records.get(2 * k as usize + 1)?;
        Some((a, e))
    }

    /// The pose the counters stand on, without advancing them. Before the
    /// first tick (the entry's `-1` key) that is pose 0 - the entry's stores.
    pub fn pose(&self, mode: DanceMode) -> Option<DanceCameraPose> {
        let table = &self.keys[usize::from(mode != DanceMode::Qualifier)];
        if self.key < 0 {
            let (a, e) = self.record_pose(table[0])?;
            return Some(DanceCameraPose {
                angles: a,
                eye: e.map(i32::from),
            });
        }
        let s2 = self.key as usize;
        let s1 = if self.key + 1 >= DANCE_CAMERA_KEY_WRAP {
            1
        } else {
            s2 + 1
        };
        let (a_ang, a_eye) = self.record_pose(*table.get(s2)?)?;
        let (b_ang, b_eye) = self.record_pose(*table.get(s1)?)?;
        // `((0x151 - timer) << 10) / 0x151` - the multiply by `0x309E0185`
        // and `mfhi >> 6` at `0x801CF638..0x801CF664` is that division.
        let step = ((DANCE_CAMERA_SEGMENT - self.timer) << 10) / DANCE_CAMERA_SEGMENT;
        let w = i32::from(*self.ease.get(step.clamp(0, 0x41F) as usize)?);
        // `a + (b - a) * w / 0x1000`, the product rounded toward zero
        // (`bgez` / `addiu 0xfff` / `sra 0xc`).
        let lerp = |a: i16, b: i16| i32::from(a) + (i32::from(b) - i32::from(a)) * w / 0x1000;
        Some(DanceCameraPose {
            // `lhu` + delta, `sh`: the angle wraps as a halfword.
            angles: std::array::from_fn(|i| lerp(a_ang[i], b_ang[i]) as i16),
            eye: std::array::from_fn(|i| lerp(a_eye[i], b_eye[i])),
        })
    }

    /// One dance tick of the track: the gate, the segment timer, the key
    /// step, then the interpolated pose `FUN_801CF470` writes into
    /// `_DAT_8007B790..94` and `0x800840B8..C0`.
    ///
    /// The gate is the tick's: the dance state `DAT_801D5334` non-zero (every
    /// state the entry leaves - the port's staged dance), the dev counter
    /// `_DAT_8007B6D0` zero (always, off the debug menu), and the mode not
    /// the how-to demo (`li v0,0x2; beq v1,v0` at `0x801CF510`). `None` when
    /// the gate holds the camera - the how-to demo keeps the entry's pose.
    ///
    /// PORT: FUN_801cf470 (the camera keyframe block, `0x801CF51C..0x801CF7D8`)
    pub fn tick(&mut self, mode: DanceMode, frame_delta: u8) -> Option<DanceCameraPose> {
        if mode == DanceMode::HowTo {
            return None;
        }
        self.timer -= i32::from(frame_delta);
        if self.timer < 0 {
            self.timer = DANCE_CAMERA_SEGMENT;
            self.key += 1;
            if self.key >= DANCE_CAMERA_KEY_WRAP {
                self.key = 1;
            }
        }
        self.pose(mode)
    }
}

/// A track pose as the port's field-frame camera: the entry's `H` and the
/// beat-clock actor's focus ([`venue_camera`]), with this pose's angles and
/// its eye trio reduced by the 6x world scale.
pub fn venue_camera_at(e: &DanceSceneEntry, pose: DanceCameraPose) -> FieldCameraView {
    FieldCameraView {
        pitch: angle_rad(pose.angles[0] as u16),
        yaw: angle_rad(pose.angles[1] as u16),
        roll: angle_rad(pose.angles[2] as u16),
        tr_eye: pose.eye.map(|c| c as f32 / VENUE_WORLD_SCALE),
        ..venue_camera(e)
    }
}

/// One of the entry's resolved face stamps: which rig the selector picked
/// and which pose it stamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryFaceStamp {
    /// The call's dancer slot (`a0`).
    pub dancer: u8,
    /// The pose (`a1`).
    pub pose: u8,
    /// The mode global in force for the call.
    pub mode: DanceMode,
    /// The rig [`dance_face_rig`] resolves - the strip the blit reads.
    pub rig: usize,
}

/// Replay the entry's five `FUN_801D03C4` calls through the selector's own
/// gates: a dancer past `3` stamps nothing, a pose equal to the dancer's
/// latched pose stamps nothing, and the three-word latch is cleared to `-1`
/// each time the entry rewrites the mode global. The retail result is rigs
/// `0`, `1`, `2`, `2`, `3` - every rig the floor can show, stamped once
/// before the first frame draws.
pub fn entry_face_stamps(e: &DanceSceneEntry) -> Vec<EntryFaceStamp> {
    let mut latch = [-1i32; 4];
    let mut mode_prev: Option<u32> = None;
    let mut out = Vec::new();
    for (&(dancer, pose), &mode) in e.face_stamps.iter().zip(e.face_stamp_mode.iter()) {
        if mode_prev != Some(mode) {
            latch = [-1; 4];
            mode_prev = Some(mode);
        }
        let d = usize::from(dancer);
        if d >= latch.len() || latch[d] == i32::from(pose) {
            continue;
        }
        latch[d] = i32::from(pose);
        let mode = if mode == 0 {
            DanceMode::Qualifier
        } else {
            DanceMode::Finals
        };
        if let Some(rig) = dance_face_rig(mode, d) {
            out.push(EntryFaceStamp {
                dancer,
                pose,
                mode,
                rig,
            });
        }
    }
    out
}

/// Stamp `pose` of `rig` into `vram`: the selector's two `MoveImage` blits,
/// eye cell then mouth cell, each from `base + (u >> 2, v)` of the pose's
/// frame-table row to the rig's fixed destination. Returns `false` when the
/// frame table is empty.
///
// REF: FUN_801d03c4, FUN_80058490 (the selector's two `MoveImage` blits)
pub fn stamp_face(vram: &mut Vram, rig: &FaceRig, frames: &[[u8; 4]], pose: usize) -> bool {
    let Some(frame) = frames.get(pose.min(frames.len().saturating_sub(1))) else {
        return false;
    };
    vram.move_image(
        rig.base.0 + u16::from(frame[0] >> 2),
        rig.base.1 + u16::from(frame[1]),
        rig.eyes.w_hw,
        rig.eyes.h,
        rig.eyes.dst.0,
        rig.eyes.dst.1,
    );
    vram.move_image(
        rig.base.0 + u16::from(frame[2] >> 2),
        rig.base.1 + u16::from(frame[3]),
        rig.mouth.w_hw,
        rig.mouth.h,
        rig.mouth.dst.0,
        rig.mouth.dst.1,
    );
    true
}

/// The extraction-frame PROT entry of the entry's audio bank - raw
/// [`DanceSceneEntry::stream_ids`]`.1` less the CDNAME frame shift.
pub fn venue_sfx_vab_index(e: &DanceSceneEntry) -> u32 {
    e.stream_ids.1 - legaia_prot::cdname::RAW_TOC_INDEX_OFFSET
}

/// A two-define CDNAME map framing only the venue block, for a host that
/// holds the PROT bytes but not `CDNAME.TXT`: the block starts at the
/// record's raw block base and ends at its audio bank, the first entry past
/// the venue's scene data. Bounding the block there also keeps it off the
/// TOC's zeroed tail.
pub fn venue_cdname_stub(e: &DanceSceneEntry) -> String {
    let name = legaia_asset::dance_cast::DANCE_SCENE_NAME;
    format!(
        "#define {name} {} \n#define {name}_end {} \n",
        e.scene_block_base, e.stream_ids.1
    )
}

/// The venue scene's CDNAME name: the block whose `#define` is exactly the
/// record's raw block base.
pub fn venue_scene_name(index: &ProtIndex, e: &DanceSceneEntry) -> Option<String> {
    index.cdname_map()?.get(&e.stream_ids.0).cloned()
}

/// One of the venue's static draws with its cross-draw coplanar lift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VenueDraw {
    pub draw: EnvDraw,
    pub lift: [f32; 3],
}

/// The loaded venue: the scene [`DanceSceneEntry::stream_ids`] names, with
/// the VRAM the entry leaves resident.
pub struct DanceVenue {
    /// The venue scene (`other7`).
    pub scene: Scene,
    /// Its resources. `resources.vram` also carries the human dancer's field
    /// atlas (the rig-0 face strip), the HUD page and the five entry face
    /// stamps.
    pub resources: SceneResources,
    /// Terrain tiles then placed objects - the order the coplanar ranking
    /// needs - each with its lift.
    pub draws: Vec<VenueDraw>,
    /// The scene's ANM bundle, the rest-pose source for bound props and the
    /// dancers' choreography bank.
    pub anm: Option<PlayerAnmBundle>,
    /// The entry's face stamps, in call order.
    pub face_stamps: Vec<EntryFaceStamp>,
    /// How many of [`Self::face_stamps`] found their frame table and blitted.
    pub faces_stamped: usize,
    /// Whether the HUD page reached [`Self::resources`]' VRAM.
    pub hud_staged: bool,
    /// The record's camera ([`venue_camera`]).
    pub camera: FieldCameraView,
}

impl DanceVenue {
    /// Load the venue the entry record names. `overlay` is the dance overlay
    /// (PROT 0980) in its loaded form - the face frame tables and the HUD
    /// widget table live in its rodata; without it the venue still loads but
    /// stamps no faces and stages no HUD page.
    ///
    /// `index` must carry a CDNAME map naming the record's block base (the
    /// full map, or [`venue_cdname_stub`]).
    pub fn build(index: &ProtIndex, overlay: Option<&[u8]>) -> Option<Self> {
        let e = dance_scene_entry();
        let name = venue_scene_name(index, &e)?;
        let scene = Scene::load(index, &name).ok()?;
        let (mut resources, _) = SceneResources::build_targeted_with_options(
            &scene,
            &[],
            BuildOptions {
                kind: SceneLoadKind::Field,
                // The venue's field file is uploaded whole: the dancer atlases
                // and their CLUT rows must all be resident.
                upload_all_tims: true,
                system_ui: None,
            },
        )
        .ok()?;
        // The human dancer's field atlas (PROT 0874 §2) - resident from the
        // field in retail, and the strip rig 0 stamps into.
        if let Ok(raw) = index.entry_bytes(legaia_asset::field_char_textures::PROT_ENTRY_INDEX)
            && let Ok(pack) = legaia_asset::field_char_textures::parse(&raw)
        {
            pack.upload_to_vram(&mut resources.vram, false);
        }
        let mut hud_staged = false;
        let mut faces_stamped = 0;
        let face_stamps = entry_face_stamps(&e);
        if let Some(overlay) = overlay {
            let mut rects: Vec<crate::dance::DanceHudRect> = Vec::new();
            for (w, _) in crate::dance::dance_widgets_with_abr(overlay) {
                let r = (
                    w.tpage_xy(),
                    (((w.clut & 0x3F) * 16), (w.clut >> 6) & 0x1FF),
                );
                if !rects.contains(&r) {
                    rects.push(r);
                }
            }
            hud_staged = crate::dance::stage_dance_hud_vram(index, &rects, &mut resources.vram) > 0;
            for s in &face_stamps {
                let Some(rig) = dance_art::FACE_RIGS.get(s.rig) else {
                    continue;
                };
                let frames = dance_art::parse_face_frames(overlay, rig).unwrap_or_default();
                if stamp_face(&mut resources.vram, rig, &frames, usize::from(s.pose)) {
                    faces_stamped += 1;
                }
            }
        }
        let draws = venue_draws(index, &scene, &resources);
        let anm = crate::npc_catalog::scene_anm_bundle(&scene);
        Some(Self {
            scene,
            resources,
            draws,
            anm,
            face_stamps,
            faces_stamped,
            hud_staged,
            camera: venue_camera(&e),
        })
    }

    /// Frame-0 rigid transforms of scene-ANM record `anim_id - 1`, for a bound
    /// prop's rest pose - `None` (draw unposed) when the record is missing or
    /// its bone count is not the mesh's object count (retail's count-equality
    /// contract in `FUN_8001B964`).
    pub fn frame0_bone_offsets(
        &self,
        anim_id: u8,
        objects: usize,
    ) -> Option<Vec<([i16; 3], [i16; 3])>> {
        let anm = self.anm.as_ref()?;
        let rec_idx = usize::from(anim_id).checked_sub(1)?;
        let rec = anm.record_lenient(rec_idx).ok()?;
        if rec.bone_count as usize != objects {
            return None;
        }
        Some(
            (0..objects)
                .map(|b| match anm.bone_transform(rec_idx, 0, b) {
                    Some(t) => (
                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                    ),
                    None => ([0; 3], [0; 3]),
                })
                .collect(),
        )
    }
}

/// The venue's static draw list: terrain tiles (records without
/// `FLAG_PLACED`) then placed objects, resolved through the same
/// [`field_env`] calls the field scene uses, each with the cross-draw
/// coplanar lift - ranked terrain first, as every host ranks them.
fn venue_draws(index: &ProtIndex, scene: &Scene, res: &SceneResources) -> Vec<VenueDraw> {
    use legaia_asset::field_objects::FLAG_PLACED;
    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    let binds = scene.field_object_binds(index).ok().flatten();
    let placement_records = scene
        .field_object_placements(index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let terrain_records: Vec<_> = scene
        .field_terrain_tiles(index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.flags & FLAG_PLACED == 0)
        .collect();
    let (placements, _) = field_env::resolve_placed_env_draws(
        &env_tmds,
        &placement_records,
        floor_lut,
        binds.as_ref(),
    );
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &terrain_records, floor_lut);
    let mut ranked: Vec<EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    ranked.extend(terrain.iter().copied());
    ranked.extend(placements.iter().copied());
    let planes = crate::coplanar_draws::draw_plane_summaries(&ranked, res);
    let lifts = crate::coplanar_draws::coplanar_draw_offsets(&ranked, &planes);
    ranked
        .into_iter()
        .map(|draw| VenueDraw {
            draw,
            lift: lifts.get(&draw).copied().unwrap_or([0.0; 3]),
        })
        .collect()
}

/// What the entry staged over the walked-in state, and what the teardown
/// puts back. Lives on [`crate::world::MinigameState::dance_venue`] exactly
/// while the dance runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DanceVenueStage {
    /// The record's camera ([`venue_camera`]); the dance arm of
    /// [`crate::camera_view::resolve_field_camera`] frames with it.
    pub camera: FieldCameraView,
    /// `_DAT_80084540` as the walked-in scene had it (`0x801CF0D8` saves it
    /// to `DAT_801D5180`; `0x801D4184` restores it).
    pub saved_block_base: u32,
    /// The camera's visible-tile window as the walked-in scene had it.
    pub saved_view_window: [i8; 4],
    /// Bumps per staging, so a host holding a built venue can tell a re-entry
    /// from the run it built for.
    pub generation: u32,
}

/// Which edge [`sync_dance_venue`] took this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceVenueEdge {
    /// The entry's globals were staged (the dance began).
    Staged,
    /// The teardown's restores ran (the dance ended).
    Restored,
}

/// Stage or tear down the venue globals to match the world's mode. Call once
/// per host frame after the world tick, with the camera the host frames the
/// field with.
///
/// The staging edge is the entry `FUN_801CEF54`'s globals, straight off
/// [`dance_scene_entry`]: `_DAT_80084540` = [`DanceSceneEntry::scene_block_base`]
/// (the port's mirror is `World::battle.map_id`, the value the scene loader
/// writes on every field entry), the scratchpad window `0x1F8003E8..EB` =
/// [`DanceSceneEntry::view_window`], and the camera pose [`venue_camera`]
/// derives. The restore edge is the half of the teardown `FUN_801D414C` the
/// port holds state for: the block base and the window go back to what the
/// walked-in scene had. The scene-name restore needs no write here - the
/// walked-in scene is never unloaded.
///
/// Every staged frame then advances the run's camera keyframe track
/// ([`DanceCameraTrack::tick`], via [`crate::dance::DanceGame::advance_camera`])
/// and re-frames the staged camera on the pose it wrote
/// ([`venue_camera_at`]). A run with no track, or the how-to demo, keeps the
/// entry's pose.
///
// REF: FUN_801CEF54, FUN_801D414C (the record is
// `crate::dance::dance_scene_entry`, whose `PORT:` tag this call makes live)
pub fn sync_dance_venue(world: &mut World, camera: &mut Camera) -> Option<DanceVenueEdge> {
    let edge = stage_or_restore(world, camera);
    // The dance tick's camera keyframe track runs every staged frame - the
    // staging frame included, which is the tick's first after the entry and
    // lands on pose 0 exactly.
    if world.mode == SceneMode::Dance
        && let (Some(stage), Some(game)) = (
            world.minigames.dance_venue.as_mut(),
            world.minigames.dance.as_mut(),
        )
        && let Some(pose) = game.advance_camera(1)
    {
        stage.camera = venue_camera_at(&dance_scene_entry(), pose);
    }
    edge
}

fn stage_or_restore(world: &mut World, camera: &mut Camera) -> Option<DanceVenueEdge> {
    let in_dance = world.mode == SceneMode::Dance;
    match (in_dance, world.minigames.dance_venue.is_some()) {
        (true, false) => {
            let e = dance_scene_entry();
            let generation = world.minigames.dance_venue_generation.wrapping_add(1);
            world.minigames.dance_venue_generation = generation;
            world.minigames.dance_venue = Some(DanceVenueStage {
                camera: venue_camera(&e),
                saved_block_base: world.battle.map_id,
                saved_view_window: camera.zone.view_window,
                generation,
            });
            world.battle.map_id = u32::from(e.scene_block_base);
            let (x0, z0, x1, z1) = e.view_window;
            camera.zone.view_window = [x0, z0, x1, z1];
            Some(DanceVenueEdge::Staged)
        }
        (false, true) => {
            let stage = world.minigames.dance_venue.take()?;
            let teardown = crate::dance::dance_scene_stage();
            if teardown.restores_scene_block_base {
                world.battle.map_id = stage.saved_block_base;
            }
            camera.zone.view_window = stage.saved_view_window;
            Some(DanceVenueEdge::Restored)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ease_table_is_a_half_cosine_rise_then_a_hold() {
        let t = dance_camera_ease_table();
        assert_eq!(t.len(), DANCE_CAMERA_EASE_LEN);
        assert_eq!((t[0], t[0x200], t[0x3FF]), (0, 0x800, 0xFFF));
        assert!(t[0x400..].iter().all(|&w| w == 0x1000));
        assert!(t.windows(2).all(|w| w[0] <= w[1]), "monotonic");
        for (i, &w) in t[..0x400].iter().enumerate() {
            let model = 2048.0 * (1.0 - (std::f64::consts::PI * i as f64 / 1024.0).cos());
            assert!(
                (f64::from(w) - model).abs() <= 1.5,
                "ease[{i}] = {w} vs {model}"
            );
        }
    }

    #[test]
    fn the_segment_step_is_the_magic_division_retail_runs() {
        // `mult` by 0x309E0185, `mfhi`, `sra 6`, minus the sign word.
        for timer in 0..=DANCE_CAMERA_SEGMENT {
            let x = (DANCE_CAMERA_SEGMENT - timer) << 10;
            let hi = ((i64::from(x) * 0x309E_0185_i64) >> 32) as i32;
            let magic = (hi >> 6) - (x >> 31);
            assert_eq!(magic, x / DANCE_CAMERA_SEGMENT, "timer {timer}");
        }
    }

    /// An overlay-shaped buffer: every key of the qualifier table names pose
    /// `k % 3`, every key of the other table pose `2`; pose `k` has angles
    /// `(100k, -100k, 0)` and eye `(0, 1000k, -1000k)`.
    fn synthetic_overlay() -> Vec<u8> {
        let base = legaia_asset::dance_chart::DANCE_OVERLAY_BASE_VA;
        let mut o = vec![0u8; (DANCE_CAMERA_KEYS_VA - base) as usize + 0x48];
        for k in 0..13u32 {
            let q = (DANCE_CAMERA_KEYS_QUALIFIER_VA - base + 4 * k) as usize;
            o[q..q + 4].copy_from_slice(&(k % 3).to_le_bytes());
            let r = (DANCE_CAMERA_KEYS_VA - base + 4 * k) as usize;
            o[r..r + 4].copy_from_slice(&2u32.to_le_bytes());
        }
        for p in 0..3i16 {
            let at = (DANCE_CAMERA_POSES_VA - base) as usize + 16 * p as usize;
            for (i, c) in [100 * p, -100 * p, 0, 0, 0, 1000 * p, -1000 * p, 0]
                .into_iter()
                .enumerate()
            {
                o[at + 2 * i..at + 2 * i + 2].copy_from_slice(&c.to_le_bytes());
            }
        }
        o
    }

    fn pose(k: i16) -> DanceCameraPose {
        DanceCameraPose {
            angles: [100 * k, -100 * k, 0],
            eye: [0, 1000 * i32::from(k), -1000 * i32::from(k)],
        }
    }

    #[test]
    fn the_track_opens_on_key_zero_then_eases_segment_by_segment() {
        let mut t = DanceCameraTrack::from_overlay(&synthetic_overlay()).expect("parses");
        let q = DanceMode::Qualifier;
        assert_eq!(t.counters(), (-1, 0));
        assert_eq!(t.pose(q), Some(pose(0)), "the entry's pose before a tick");
        // The first tick reloads the timer and lands on key 0 at weight 0.
        assert_eq!(t.tick(q, 1), Some(pose(0)));
        assert_eq!(t.counters(), (0, DANCE_CAMERA_SEGMENT));
        // Half-way through the segment: ease[512] = 0x800, the mean pose.
        let mut last = None;
        for _ in 0..(DANCE_CAMERA_SEGMENT + 1) / 2 {
            last = t.tick(q, 1);
        }
        let mid = last.unwrap();
        let (_, timer) = t.counters();
        let step = ((DANCE_CAMERA_SEGMENT - timer) << 10) / DANCE_CAMERA_SEGMENT;
        let w = i32::from(dance_camera_ease_table()[step as usize]);
        assert_eq!(mid.eye[1], 1000 * w / 0x1000);
        assert_eq!(mid.angles[1], (-100 * w / 0x1000) as i16);
        // The segment's last frame (timer 0) is the next key's pose exactly,
        // and the reload holds it.
        while t.counters().1 > 0 {
            last = t.tick(q, 1);
        }
        assert_eq!(last, Some(pose(1)));
        assert_eq!(t.tick(q, 1), Some(pose(1)));
        assert_eq!(t.counters(), (1, DANCE_CAMERA_SEGMENT));
    }

    #[test]
    fn the_key_wraps_to_one_never_zero() {
        let mut t = DanceCameraTrack::from_overlay(&synthetic_overlay()).unwrap();
        let q = DanceMode::Qualifier;
        let mut keys = Vec::new();
        for _ in 0..14 * (DANCE_CAMERA_SEGMENT + 1) {
            t.tick(q, 1);
            if keys.last() != Some(&t.counters().0) {
                keys.push(t.counters().0);
            }
        }
        assert_eq!(&keys[..14], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 1]);
        // Key 12 eases toward key 1, not key 0 (`li s1,1` at 0x801CF580).
        let mut t = DanceCameraTrack::from_overlay(&synthetic_overlay()).unwrap();
        while t.counters() != (12, 0) {
            t.tick(q, 1);
        }
        assert_eq!(t.pose(q), Some(pose(1)));
    }

    #[test]
    fn the_mode_picks_the_table_and_the_how_to_demo_holds_the_entry_pose() {
        let mut t = DanceCameraTrack::from_overlay(&synthetic_overlay()).unwrap();
        assert_eq!(t.tick(DanceMode::Finals, 1), Some(pose(2)));
        let mut t = DanceCameraTrack::from_overlay(&synthetic_overlay()).unwrap();
        for _ in 0..500 {
            assert_eq!(t.tick(DanceMode::HowTo, 1), None);
        }
        assert_eq!(t.counters(), (-1, 0), "the gate skips the whole block");
        // A frame delta of 2 drains the segment twice as fast.
        let mut t = DanceCameraTrack::from_overlay(&synthetic_overlay()).unwrap();
        t.tick(DanceMode::Qualifier, 2);
        t.tick(DanceMode::Qualifier, 2);
        assert_eq!(t.counters(), (0, DANCE_CAMERA_SEGMENT - 2));
    }

    #[test]
    fn a_track_pose_frames_through_the_entry_focus_and_h() {
        let e = dance_scene_entry();
        let entry = DanceCameraPose {
            angles: [
                e.camera_angles.0 as i16,
                e.camera_angles.1 as i16,
                e.camera_angles.2 as i16,
            ],
            eye: [0, e.camera_pair.0 as i32, e.camera_pair.1 as i32],
        };
        assert_eq!(venue_camera_at(&e, entry), venue_camera(&e));
        let moved = venue_camera_at(
            &e,
            DanceCameraPose {
                eye: [600, 0, 0],
                ..entry
            },
        );
        assert_eq!(moved.tr_eye, [100.0, 0.0, 0.0]);
        assert_eq!(
            (moved.focus, moved.h),
            (venue_camera(&e).focus, venue_camera(&e).h)
        );
    }

    #[test]
    fn the_entry_face_stamps_preload_every_floor_rig() {
        let s = entry_face_stamps(&dance_scene_entry());
        let rigs: Vec<usize> = s.iter().map(|s| s.rig).collect();
        assert_eq!(rigs, [0, 1, 2, 2, 3]);
        assert_eq!(s[0].pose, 1);
        assert!(s[1..].iter().all(|s| s.pose == 0));
        assert!(s[..3].iter().all(|s| s.mode == DanceMode::Finals));
        assert!(s[3..].iter().all(|s| s.mode == DanceMode::Qualifier));
    }

    #[test]
    fn a_latched_pose_stamps_nothing() {
        let mut e = dance_scene_entry();
        // One mode for all five: the repeats of slots 1 and 2 hit the latch.
        e.face_stamp_mode = [0; 5];
        let rigs: Vec<usize> = entry_face_stamps(&e).iter().map(|s| s.rig).collect();
        assert_eq!(rigs, [0, 2, 3]);
    }

    #[test]
    fn the_venue_camera_frames_the_qualifier_floor() {
        let e = dance_scene_entry();
        let v = venue_camera(&e);
        assert_eq!(v.focus, [6144.0, 0.0, 13056.0]);
        assert_eq!(v.h, 512.0);
        // The qualifier cast (`dance_cast`): Noa centre, kinds 2 / 3 flanking,
        // all at `z 0x3480`, standing on the floor at the spawn's height.
        let floor_y = f32::from(e.dancer_spawn.1);
        let vp = v.vp(4.0 / 3.0);
        for x in [0x1740, 0x1800, 0x18C0] {
            for y in [floor_y, floor_y - 250.0] {
                // `vp` is the Y-up render frame; the world is Y-down.
                let p = [x as f32, -y, 0x3480 as f32, 1.0];
                let mut c = [0.0f32; 4];
                for (row, out) in c.iter_mut().enumerate() {
                    *out = (0..4).map(|col| vp[col * 4 + row] * p[col]).sum();
                }
                assert!(c[3] > 0.0, "in front of the lens");
                let (sx, sy) = (c[0] / c[3], c[1] / c[3]);
                assert!(sx.abs() < 1.0 && sy.abs() < 1.0, "on screen: {sx} {sy}");
            }
        }
    }

    #[test]
    fn entering_and_leaving_the_dance_stages_and_restores_the_globals() {
        let mut world = World::new();
        let mut camera = Camera::new();
        world.mode = SceneMode::Field;
        world.battle.map_id = 0x2A;
        let field_window = camera.zone.view_window;
        assert_eq!(sync_dance_venue(&mut world, &mut camera), None);

        world.mode = SceneMode::Dance;
        assert_eq!(
            sync_dance_venue(&mut world, &mut camera),
            Some(DanceVenueEdge::Staged)
        );
        let e = dance_scene_entry();
        assert_eq!(world.battle.map_id, u32::from(e.scene_block_base));
        assert_eq!(camera.zone.view_window, [-8, -10, 8, 10]);
        let stage = world.minigames.dance_venue.expect("staged");
        assert_eq!(stage.camera, venue_camera(&e));
        assert_eq!(stage.generation, 1);
        // Idempotent while the dance runs.
        assert_eq!(sync_dance_venue(&mut world, &mut camera), None);

        world.mode = SceneMode::Field;
        assert_eq!(
            sync_dance_venue(&mut world, &mut camera),
            Some(DanceVenueEdge::Restored)
        );
        assert_eq!(world.battle.map_id, 0x2A);
        assert_eq!(camera.zone.view_window, field_window);
        assert!(world.minigames.dance_venue.is_none());

        // A second run is a new generation.
        world.mode = SceneMode::Dance;
        sync_dance_venue(&mut world, &mut camera);
        assert_eq!(world.minigames.dance_venue.map(|s| s.generation), Some(2));
    }

    #[test]
    fn the_stub_map_and_the_audio_bank_come_off_the_record() {
        let e = dance_scene_entry();
        assert_eq!(
            venue_sfx_vab_index(&e),
            dance_art::DANCE_SFX_VAB_PROT_INDEX as u32
        );
        let map = legaia_prot::cdname::parse_str(&venue_cdname_stub(&e)).unwrap();
        assert_eq!(
            map.get(&e.stream_ids.0).map(String::as_str),
            Some(legaia_asset::dance_cast::DANCE_SCENE_NAME)
        );
    }
}
