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
// REF: FUN_801CEF54, FUN_801D414C (the record is
// `crate::dance::dance_scene_entry`, whose `PORT:` tag this call makes live)
pub fn sync_dance_venue(world: &mut World, camera: &mut Camera) -> Option<DanceVenueEdge> {
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
