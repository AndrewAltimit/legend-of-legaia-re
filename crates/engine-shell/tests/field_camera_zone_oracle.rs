//! Retail oracle for the **zone-driven field follow camera**
//! (`engine-core::camera_zone` + `Camera::zone`): for every walkable state in
//! the save library, seat the engine on the state's scene and player tile
//! and grade the port against what retail's own RAM holds.
//!
//! A field state's RAM carries the whole retail pipeline's inputs and
//! outputs at once, so the port is measured in three tiers that fail
//! independently:
//!
//! 1. **Zone selection** - the engine's tile query (`FUN_801DE3E0` /
//!    `FUN_801DBA20` + the loader `FUN_801DBC20`) must produce the camera
//!    parameter block retail holds at `0x8007B607..0x8007B627`, and the
//!    walk-region attribute box the composer spans must match scratchpad
//!    `0x1F800384..87`. A miss here is a wrong record, a wrong query, or a
//!    script-loaded record the tile query cannot see.
//! 2. **Compose** - `compose` over retail's OWN block, box and player must
//!    reproduce the staging descriptor retail last composed at `0x801F3580`
//!    (pitch `+0x02`, yaw `+0x06`, eye `+0x0E/+0x12/+0x16`, `H` `+0x26`) -
//!    the `FUN_801DAB90` arithmetic in isolation.
//! 3. **Live pose** - the engine's snapped `(H, pitch, yaw)` against the
//!    live `_DAT_8007B6F4` / `_DAT_8007B790/92`. Two kinds of state
//!    legitimately differ and are classified, never hidden: a **scripted**
//!    one, where an op-`0x45` shot owns the live camera (the live trio equals
//!    the op-`0x45` staging struct at `0x801C6EA8`, and the follow
//!    descriptor is stale - composed at an earlier player position), and a
//!    **mid-glide** one, where retail's ease stopped short because the
//!    player stopped moving (the wall-press captures push into a wall, so
//!    the position never changes and the ease never runs).
//!
//! The two LUT reproductions the composer leans on (`atan_q11`, `sqrt0_lut`)
//! are pinned entry-for-entry against `SCUS_942.54` in the same file.
//!
//! Skips (passes) unless `scripts/scenarios.toml`, `saves/library` and
//! `extracted/` are all present. Structural assertions only - no Sony bytes
//! are printed or asserted.

use std::path::{Path, PathBuf};

use legaia_engine_core::camera::Camera;
use legaia_engine_core::camera_zone::{
    CameraZoneConfig, ComposeInputs, atan_q11, compose, sqrt0_lut,
};
use legaia_engine_core::scene::SceneHost;
use legaia_mednafen::game_anchors;
use legaia_mednafen::{SaveState, ScenarioManifest};

const RAM_BASE: u32 = 0x8000_0000;
/// GTE `H` (`_DAT_8007B6F4`).
const GTE_H: u32 = 0x8007_B6F4;
/// Camera rotation trio `(pitch, yaw, roll)`.
const CAM_ROT: u32 = 0x8007_B790;
/// The camera parameter block (`0x8007B600 + 0x28`).
const PARAM_BLOCK: u32 = 0x8007_B600;
/// The follow composer's staging descriptor (field overlay 0897).
const STAGING: u32 = 0x801F_3580;
/// The scene MAN's low header bit (`DAT_8007B6A8`).
const HALF_EYE_FLAG: u32 = 0x8007_B6A8;
/// The op-`0x45` Configure staging struct (`_DAT_801C6EA8`): a scripted shot
/// writes the live globals from it.
const SCRIPT_CAM: u32 = 0x801C_6EA8;
/// Scratchpad offsets of the walk-region attribute box.
const SCRATCH_BOX: usize = 0x384;
/// The SCUS arctangent table (2049 halfwords) and the `SquareRoot0` mantissa
/// table (192 halfwords).
const ATAN_TABLE_VA: u32 = 0x8006_F4C8;
const SQRT_TABLE_VA: u32 = 0x8007_8E84;

fn library() -> Option<(ScenarioManifest, PathBuf)> {
    let manifest = ["scripts/scenarios.toml", "../../scripts/scenarios.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())?;
    let lib = ["saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())?;
    Some((ScenarioManifest::from_path(&manifest).ok()?, lib))
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn off(va: u32) -> usize {
    (va - RAM_BASE) as usize & 0x1F_FFFF
}

fn rs16(ram: &[u8], va: u32) -> i16 {
    let o = off(va);
    i16::from_le_bytes([ram[o], ram[o + 1]])
}

/// PS-X EXE VA -> file offset (header is 0x800 bytes, text base at +0x18).
fn exe_off(scus: &[u8], va: u32) -> Option<usize> {
    if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
        return None;
    }
    let base = u32::from_le_bytes([scus[0x18], scus[0x19], scus[0x1A], scus[0x1B]]);
    va.checked_sub(base).map(|d| d as usize + 0x800)
}

/// The retail arctangent and square-root tables are the trigonometric
/// reproductions `camera_zone` computes, entry for entry.
#[test]
fn the_composer_luts_reproduce_the_scus_tables() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Ok(scus) = std::fs::read(extracted.join("SCUS_942.54")) else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let rd = |va: u32| -> i16 {
        let o = exe_off(&scus, va).expect("table inside the image");
        i16::from_le_bytes([scus[o], scus[o + 1]])
    };
    let atan_bad = (0..=2048i32)
        .filter(|&i| i32::from(rd(ATAN_TABLE_VA + 2 * i as u32)) != atan_q11(i))
        .count();
    let sqrt_bad = (0..0xC0usize)
        .filter(|&i| i32::from(rd(SQRT_TABLE_VA + 2 * i as u32)) != sqrt0_lut(i))
        .count();
    assert_eq!(atan_bad, 0, "atan table entries that differ");
    assert_eq!(sqrt_bad, 0, "sqrt mantissa entries that differ");
    eprintln!("[ok] atan 2049/2049 and sqrt 192/192 table entries reproduced");
}

/// One walkable state's retail-side reading.
struct RetailSide {
    label: String,
    scene: String,
    player: [i32; 3],
    block: CameraZoneConfig,
    scratch_box: [u8; 4],
    staging: (i16, i16, [i16; 3], i16),
    /// The player `(X, footing, Z)` the staging descriptor was composed
    /// at (its focus fields, X / Z negated back).
    staging_player: [i32; 3],
    live: (i16, i16, i16),
    /// The op-`0x45` struct's `(pitch, yaw, H)`.
    script_cam: (i16, i16, i16),
    half_eye: bool,
}

fn walkable_states(manifest: &ScenarioManifest, lib: &Path) -> Vec<RetailSide> {
    let mut out = Vec::new();
    for sc in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(sc, lib) else {
            continue;
        };
        if !path.exists() || path.extension().is_some_and(|e| e != "mcr") {
            continue;
        }
        let Ok(st) = SaveState::from_path(&path) else {
            continue;
        };
        let (Ok(ram), Ok(scratch)) = (st.main_ram(), st.scratch_ram()) else {
            continue;
        };
        if game_anchors::game_mode(ram) != 0x03 {
            continue;
        }
        let scene = game_anchors::scene_name(ram);
        if scene.starts_with("map") {
            continue;
        }
        let Some(p) = game_anchors::player_ptr(ram) else {
            continue;
        };
        let Some(block) =
            CameraZoneConfig::from_retail_block(&ram[off(PARAM_BLOCK)..off(PARAM_BLOCK) + 0x28])
        else {
            continue;
        };
        out.push(RetailSide {
            label: sc.label.clone(),
            scene,
            player: [
                i32::from(rs16(ram, p + 0x14)),
                i32::from(rs16(ram, p + 0x16)),
                i32::from(rs16(ram, p + 0x18)),
            ],
            block,
            scratch_box: [
                scratch[SCRATCH_BOX],
                scratch[SCRATCH_BOX + 1],
                scratch[SCRATCH_BOX + 2],
                scratch[SCRATCH_BOX + 3],
            ],
            staging: (
                rs16(ram, STAGING + 0x02),
                rs16(ram, STAGING + 0x06),
                [
                    rs16(ram, STAGING + 0x0E),
                    rs16(ram, STAGING + 0x12),
                    rs16(ram, STAGING + 0x16),
                ],
                rs16(ram, STAGING + 0x26),
            ),
            staging_player: [
                -i32::from(rs16(ram, STAGING + 0x1A)),
                i32::from(rs16(ram, STAGING + 0x1E)),
                -i32::from(rs16(ram, STAGING + 0x22)),
            ],
            live: (rs16(ram, CAM_ROT), rs16(ram, CAM_ROT + 2), rs16(ram, GTE_H)),
            script_cam: (
                rs16(ram, SCRIPT_CAM + 0x02),
                rs16(ram, SCRIPT_CAM + 0x06),
                rs16(ram, SCRIPT_CAM + 0x26),
            ),
            half_eye: ram[off(HALF_EYE_FLAG)] != 0,
        });
    }
    out
}

/// The block fields the composer reads for this mode - anchors matter only
/// to modes 3 and 5, sweep bytes only to the sweep modes.
fn block_key(c: &CameraZoneConfig) -> Vec<i32> {
    let mut k = vec![i32::from(c.mode), i32::from(c.b60b), c.depth, c.h];
    match c.mode >> 4 {
        3 => k.extend([i32::from(c.b60a), c.anchor_x, c.anchor_h, c.anchor_z]),
        5 => k.extend([c.pitch, c.yaw, c.anchor_x, c.anchor_h, c.anchor_z]),
        _ => k.extend([
            i32::from(c.b608),
            i32::from(c.b609),
            i32::from(c.b60a),
            c.pitch,
            c.yaw,
        ]),
    }
    k
}

#[test]
fn every_walkable_state_frames_through_the_zone_camera() {
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing");
        return;
    };
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let states = walkable_states(&manifest, &lib);
    if states.is_empty() {
        eprintln!("[skip] no walkable field state in the library");
        return;
    }

    let mut n = 0usize;
    let mut zone_ok = 0usize;
    let mut box_ok = 0usize;
    let mut compose_ok = 0usize;
    let mut live_ok = 0usize;
    let mut mid_glide = 0usize;
    let mut scripted = 0usize;
    let mut stale_staging = 0usize;
    let mut foreign_staging = 0usize;
    let mut current_staging = 0usize;
    let mut current_compose_ok = 0usize;
    let mut settled_free_roam = 0usize;
    let mut settled_free_roam_ok = 0usize;
    let mut unseated = 0usize;
    let mut unexplained = 0usize;
    let mut misses: Vec<String> = Vec::new();

    for s in &states {
        n += 1;
        let tag = format!("{} ({})", s.scene, s.label);

        // ---- engine side: seat on the scene at the state's player tile.
        let mut host = match SceneHost::open_extracted(&extracted) {
            Ok(h) => h,
            Err(e) => {
                unseated += 1;
                misses.push(format!("{tag}: no host ({e})"));
                continue;
            }
        };
        host.world.begin_new_game();
        if let Err(e) = host.enter_field_scene(&s.scene, 0) {
            unseated += 1;
            misses.push(format!("{tag}: could not enter ({e})"));
            continue;
        }
        let Some(slot) = host.world.player_actor_slot else {
            unseated += 1;
            misses.push(format!("{tag}: no player actor"));
            continue;
        };
        {
            let a = &mut host.world.actors[slot as usize];
            a.move_state.world_x = s.player[0] as i16;
            a.move_state.world_y = s.player[1] as i16;
            a.move_state.world_z = s.player[2] as i16;
        }
        // Drop anything the entry script staged over the camera so the
        // follow camera owns the frame the way a settled free-roam state does.
        host.world.cutscene.timeline = None;
        host.world.camera.state.params.clear();
        let mut cam = Camera::default();
        cam.reset_globals_for_scene_entry();
        cam.tick(&host.world);
        assert!(cam.zone.active, "{tag}: the zone camera did not engage");

        // ---- tier 1: zone selection.
        let z_ok = block_key(&cam.zone.config) == block_key(&s.block);
        let b_ok = cam.zone.attrs.box_bytes == s.scratch_box;
        zone_ok += usize::from(z_ok);
        box_ok += usize::from(b_ok);

        // ---- tier 2: compose over retail's own inputs. The staging
        // descriptor is whatever the follow composer LAST wrote; when its
        // own focus names a different player position than the state's,
        // retail has not composed since (a scripted move, or the ease lock),
        // so the comparison is made at the position it was composed for.
        let stale = s.staging_player[0] != s.player[0] || s.staging_player[2] != s.player[2];
        stale_staging += usize::from(stale);
        // Every mode copies the block's `H` into the descriptor verbatim, so
        // a staging `H` that is not the resident block's proves the
        // descriptor was composed from a block since replaced (the opening's
        // scripted mode-3 record) - nothing in RAM can reproduce it.
        let foreign = s.staging.3 != s.block.h as i16;
        foreign_staging += usize::from(foreign);
        let at = if stale { s.staging_player } else { s.player };
        let floor = host.world.sample_field_floor_height(at[0], at[2]);
        let inputs = ComposeInputs {
            player: at,
            floor_y: floor,
            attr_box: s.scratch_box,
            live_pitch: i32::from(s.live.0),
            live_yaw: i32::from(s.live.1),
            half_eye_y: s.half_eye,
        };
        let t = compose(&s.block, &inputs).target;
        // The footing IS the floor sample of the tile the player stands on,
        // so a compose that matches with the footing but not with the
        // engine's sampler isolates the sampler.
        let t_footing = compose(
            &s.block,
            &ComposeInputs {
                floor_y: at[1],
                ..inputs
            },
        )
        .target;
        let want = s.staging;
        let c_ok = (t.pitch, t.yaw, t.h) == (want.0, want.1, want.3) && t.eye == want.2;
        let c_footing_ok =
            (t_footing.pitch, t_footing.yaw, t_footing.h) == (want.0, want.1, want.3);
        compose_ok += usize::from(c_ok);
        if !stale && !foreign {
            current_staging += 1;
            current_compose_ok += usize::from(c_ok);
        }
        if !z_ok {
            // Which section-3 record, if any, decodes to the block retail
            // holds (structural only - index and kind).
            let table = &host.world.terrain.zone_table;
            let count = table.first().copied().unwrap_or(0) as usize;
            let mut hits = Vec::new();
            for i in 0..count {
                let o = 1 + i * legaia_engine_core::field_regions::ZONE_RECORD_STRIDE;
                let Some(r) =
                    table.get(o..o + legaia_engine_core::field_regions::ZONE_RECORD_STRIDE)
                else {
                    break;
                };
                let rec: [u8; 18] = r.try_into().unwrap();
                let mut c = cam.zone.config;
                c.load_record(&rec);
                if block_key(&c) == block_key(&s.block) {
                    hits.push(format!("#{i} kind {}", rec[0]));
                }
            }
            misses.push(format!(
                "{tag}: zone table has {count} records; retail's block decodes from {hits:?} \
                 (engine loaded {:?})",
                cam.zone.loaded_record.map(|r| r[0])
            ));
        }

        // ---- tier 3: the live pose.
        let g = &cam.globals.0;
        let engine = (g[0] as i16, g[1] as i16, g[9] as i16);
        let l_ok = engine == s.live;
        live_ok += usize::from(l_ok);
        let staged_matches_live = (want.0, want.1, want.3) == s.live;
        // A scripted shot: the live trio is the op-`0x45` struct's, allowing
        // one axis still gliding under the script's own mover.
        let script_axes = usize::from(s.live.0 == s.script_cam.0)
            + usize::from(s.live.1 == s.script_cam.1)
            + usize::from(s.live.2 == s.script_cam.2);
        let script = !l_ok && script_axes >= 2 && !staged_matches_live;
        let glide = !l_ok && !script && c_ok && !staged_matches_live;
        scripted += usize::from(script);
        mid_glide += usize::from(glide);
        unexplained += usize::from(!(l_ok || script || glide || !z_ok));
        // A settled free-roam state: retail's own live trio IS its composed
        // target and no script owns the camera. Here the port must be exact.
        if staged_matches_live && !script && !stale {
            settled_free_roam += 1;
            settled_free_roam_ok += usize::from(l_ok);
        }

        if !(z_ok && b_ok && c_ok && l_ok) {
            let class = if script {
                " [scripted shot]"
            } else if glide {
                " [mid-glide]"
            } else {
                ""
            };
            misses.push(format!(
                "{tag}: mode {:#x} zone={} box={} compose={} (footing-only {}) live={}{}{}  \
                 engine(pitch,yaw,H)={:?} retail live={:?} staging={:?} script={:?} \
                 engine block {:?} retail block {:?}",
                s.block.mode,
                z_ok,
                b_ok,
                c_ok,
                c_footing_ok,
                l_ok,
                class,
                match (stale, foreign) {
                    (_, true) => " [staging from a replaced block]",
                    (true, false) => " [stale staging]",
                    _ => "",
                },
                engine,
                s.live,
                (want.0, want.1, want.3),
                s.script_cam,
                block_key(&cam.zone.config),
                block_key(&s.block),
            ));
        }
    }

    for m in &misses {
        eprintln!("[miss] {m}");
    }
    eprintln!(
        "[ok] {n} walkable field states: zone block {zone_ok}/{n}, attribute box {box_ok}/{n}, \
         compose (retail block+box -> retail staging) {compose_ok}/{n} overall and \
         {current_compose_ok}/{current_staging} where the staging is current ({stale_staging} \
         stale, {foreign_staging} from a replaced block), live (H,pitch,yaw) exact {live_ok}/{n}, \
         scripted shot {scripted}/{n}, mid-glide {mid_glide}/{n}, settled free-roam exact \
         {settled_free_roam_ok}/{settled_free_roam}, unseated {unseated}/{n}"
    );
    assert_eq!(unseated, 0, "every walkable state's scene must seat");
    assert_eq!(
        box_ok, n,
        "the attribute box must match retail's scratchpad on every state"
    );
    // The compose arithmetic is the port's own claim: wherever retail's
    // descriptor was composed from the resident block at the state's own
    // position, the port must reproduce it byte for byte.
    assert_eq!(
        current_compose_ok, current_staging,
        "compose must reproduce retail's staging descriptor on every current staging"
    );
    assert!(
        current_staging > 0,
        "at least one current staging must exist"
    );
    assert_eq!(
        settled_free_roam_ok, settled_free_roam,
        "on every settled free-roam state the port's (H, pitch, yaw) must equal retail's"
    );
    assert_eq!(
        unexplained, 0,
        "every live miss must be a scripted shot, a mid-glide or a zone-selection miss"
    );
    assert!(
        settled_free_roam > 0,
        "at least one settled free-roam state must exist"
    );
}
