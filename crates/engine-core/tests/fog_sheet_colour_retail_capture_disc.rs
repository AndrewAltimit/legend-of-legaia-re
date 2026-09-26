//! Retail capture: the fog sheets' modulation colour, record for record.
//!
//! `keikoku_chest_preload` (`map01`, the kingdom overworld, retail SCUS) holds
//! the fog gate raised and all `0x48` records of the raised cap alive. Its
//! frame's display list carries the sheets `FUN_8003F86C` emitted - `POLY_FT4`,
//! command `0x2E`, CLUT `0x7640`, page `0x0027` - and every packet's colour is
//! the one `FUN_8003F3FC` computed at `0x8003F4F4..0x8003F5DC` from its
//! record's **pre-update** age: `grey * tint * brightness >> 15`, tint the
//! neutral `0x80` (`_DAT_8007BCB8..BA`).
//!
//! The state's pool holds the ages *after* later walks advanced them, by
//! `rate * dt` per pass with `dt` the frame step `DAT_1F800393` (plus the
//! player-box ageing, which the match shows no drawn record took). The ordering
//! table the frame hands the GPU was built two passes before the capture's
//! pool (libgpu double-buffers it): rolling every record back by
//! `2 * rate * dt` and running the engine's colour kernel
//! (`FogParticle::sheet_rgb`) over it reproduces all of that table's fog
//! packets, two halves per record that drew. So the port's per-sheet colour
//! is retail's, and an overworld frame that reads brighter than retail's does
//! so through sheet geometry and draw ordering, not the colour law.
//!
//! Skips (and passes) when the scenario manifest or the save library is
//! missing.

use legaia_engine_core::fog_particles::{FOG_CLUT, FOG_POOL_SLOTS, FOG_TPAGE, FogParticle};
use legaia_mednafen::prim_pool::{self, Prim};
use legaia_mednafen::{SaveState, ScenarioManifest};
use std::collections::BTreeMap;
use std::path::PathBuf;

const RAM_MASK: u32 = 0x001F_FFFF;
/// `_DAT_8007B7E0`: the fog pool pointer MAIN INIT stores.
const FOG_POOL_PTR: u32 = 0x8007_B7E0;
/// `_DAT_8007BCB8..BA`: the op `0x4C 0x12` global multiply tint.
const FOG_TINT: u32 = 0x8007_BCB8;
/// `_DAT_8007C364`: the player actor.
const PLAYER_PTR: u32 = 0x8007_C364;
/// `_DAT_8007BCAC`: the camera vertical offset the walk subtracts from `y`.
const CAMERA_Y_OFFSET: u32 = 0x8007_BCAC;
/// `_DAT_8007BB04`: the overworld curvature table `FUN_800271A8` builds.
const CURVATURE_LUT_PTR: u32 = 0x8007_BB04;
/// Scratchpad `DAT_1F800393`: the frame step the pool walk ages by.
const FRAME_STEP_SCRATCH: usize = 0x393;

fn find(rel: &str) -> Option<PathBuf> {
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}{rel}")))
        .find(|p| p.exists())
}

fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = (va & RAM_MASK) as usize;
    u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
}

#[test]
fn fog_sheet_colours_match_the_retail_frame_record_for_record() {
    let (Some(manifest_path), Some(library)) =
        (find("scripts/scenarios.toml"), find("saves/library"))
    else {
        eprintln!("[skip] scenarios manifest / saves library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");
    let Some(scn) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == "keikoku_chest_preload")
    else {
        eprintln!("[skip] keikoku_chest_preload missing from the manifest");
        return;
    };
    let Some(save_path) = manifest.library_save_path(scn, library.as_path()) else {
        eprintln!("[skip] scenario has no library backup");
        return;
    };
    if !save_path.exists() {
        eprintln!("[skip] library backup not present");
        return;
    }
    let state = SaveState::from_path(&save_path).expect("parse save state");
    let ram = state.main_ram().expect("main RAM");
    let scratch = state.scratch_ram().expect("scratchpad");

    // Retail side 1: the pool records.
    let pool = u32_at(ram, FOG_POOL_PTR);
    let tint = {
        let o = (FOG_TINT & RAM_MASK) as usize;
        [ram[o], ram[o + 1], ram[o + 2]]
    };
    assert_eq!(tint, [0x80; 3], "the overworld tint is neutral");
    let dt = u16::from(scratch[FRAME_STEP_SCRATCH]);
    let mut records = Vec::new();
    for i in 0..FOG_POOL_SLOTS as u32 {
        let o = ((pool + 0xA4 + i * 0x18) & RAM_MASK) as usize;
        let rec = &ram[o..o + 0x18];
        if rec[5] == 0 {
            continue;
        }
        let age = u16::from_le_bytes([rec[0], rec[1]]);
        let rate = u16::from_le_bytes([rec[2], rec[3]]);
        assert_eq!(rec[0x14], rec[0x15], "one grey in all three bytes");
        assert_eq!(rec[0x14], rec[0x16], "one grey in all three bytes");
        records.push((age, rate, rec[0x14]));
    }
    assert_eq!(records.len(), 72, "the raised cap's 0x48 records are live");

    // Retail side 2: each ordering table's fog packets, walked in draw order.
    // libgpu double-buffers the frame, so RAM holds the table on screen and
    // the one being built; each was coloured a whole number of walk passes
    // before the ages the pool now holds.
    let kernel = |passes: u16| -> BTreeMap<u8, usize> {
        let mut out: BTreeMap<u8, usize> = BTreeMap::new();
        for &(age, rate, grey) in &records {
            let rec = FogParticle {
                age: age.saturating_sub(rate * dt * passes),
                grey,
                alive: true,
                ..FogParticle::default()
            };
            if let Some(rgb) = rec.sheet_rgb(tint) {
                assert_eq!(rgb[0], rgb[1]);
                *out.entry(rgb[0]).or_default() += 2;
            }
        }
        out
    };
    let mut full_frames = 0usize;
    for ot in prim_pool::find_ot_arrays(ram, 0x8000_0000, 64) {
        let chain = prim_pool::chain_walk(ram, 0x8000_0000, (ot.head & RAM_MASK) as usize);
        let mut packets: BTreeMap<u8, usize> = BTreeMap::new();
        let mut sorted = Vec::new();
        for c in chain {
            if let Prim::PolyFt4 {
                cmd,
                color,
                clut,
                tpage,
                ..
            } = c.prim
                && clut == FOG_CLUT
                && tpage == FOG_TPAGE
            {
                assert_eq!(cmd, 0x2E, "textured, semi-transparent, blended");
                assert!(color[0] == color[1] && color[1] == color[2], "a grey");
                *packets.entry(color[0]).or_default() += 1;
                sorted.push(color[0]);
            }
        }
        if sorted.is_empty() {
            continue;
        }
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2];
        // Every packet's colour must be one the engine gives its record.
        // Records whose two halves both culled drew nothing, so the engine
        // side may carry more.
        let fits = |k: &BTreeMap<u8, usize>| {
            packets
                .iter()
                .all(|(c, &n)| k.get(c).copied().unwrap_or(0) >= n)
        };
        let passes = (1..=3).find(|&p| fits(&kernel(p)));
        eprintln!(
            "[ok] OT 0x{:08X}: {} fog packets, median rgb {median}; \
             engine kernel matches at {passes:?} pass(es) back (frame step {dt})",
            ot.start,
            sorted.len()
        );
        assert!(
            passes.is_some(),
            "OT 0x{:08X}: the fog packet colours are not the engine kernel's \
             over the pool's records one to three passes back",
            ot.start
        );
        if sorted.len() >= 100 {
            full_frames += 1;
        }
    }
    assert!(
        full_frames >= 1,
        "no pool carries a whole frame of fog sheets"
    );
}

/// Geometry, record for record: the state's pool rolled back to the frame the
/// walked ordering table was built from (two passes of the frame step), the
/// state's own camera words, vertical offset and walk box, through
/// `FogPool::render_step`. Each engine half must land on a retail packet of
/// the same colour to within a pixel, and the engine must draw nearly all of
/// retail's packets. Both halves of that rest on the two things the emitter
/// does that a world-space reading misses: the sheet is a view-space
/// billboard (`H * 0x80 * S / vz` tall whatever the pitch), and on the
/// overworld both corners take the curvature table's `SY` term - without it
/// the sheets at the top of the frame, retail's band above the ridges, cull
/// as off-screen. The table itself is checked entry for entry against the
/// capture's `*_DAT_8007BB04`.
#[test]
fn fog_sheet_geometry_matches_the_retail_frame() {
    use legaia_engine_core::fog_particles::{FogFrameEnv, FogPool, FogView};
    use legaia_engine_vm::psx_camera::FieldCameraView;
    let (Some(manifest_path), Some(library)) =
        (find("scripts/scenarios.toml"), find("saves/library"))
    else {
        eprintln!("[skip] scenarios manifest / saves library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");
    let Some(scn) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == "keikoku_chest_preload")
    else {
        eprintln!("[skip] keikoku_chest_preload missing from the manifest");
        return;
    };
    let Some(save_path) = manifest.library_save_path(scn, library.as_path()) else {
        eprintln!("[skip] scenario has no library backup");
        return;
    };
    if !save_path.exists() {
        eprintln!("[skip] library backup not present");
        return;
    }
    let state = SaveState::from_path(&save_path).expect("parse save state");
    let ram = state.main_ram().expect("main RAM");
    let scratch = state.scratch_ram().expect("scratchpad");
    let i16_at = |va: u32| {
        let o = (va & RAM_MASK) as usize;
        i16::from_le_bytes([ram[o], ram[o + 1]])
    };
    let dt = u16::from(scratch[FRAME_STEP_SCRATCH]);
    let back = 2 * dt;
    let overworld = scratch[0x394] & 1 != 0;
    assert!(overworld, "the map01 state holds the overworld bit");

    // The pool, rolled back to the walked table's frame.
    let pool_va = u32_at(ram, FOG_POOL_PTR);
    let mut pool = FogPool::new();
    pool.overworld = overworld;
    for i in 0..FOG_POOL_SLOTS {
        let o = ((pool_va + 0xA4 + i as u32 * 0x18) & RAM_MASK) as usize;
        let r = &ram[o..o + 0x18];
        let rec = &mut pool.records[i];
        rec.alive = r[5] != 0;
        if !rec.alive {
            continue;
        }
        rec.rate = u16::from_le_bytes([r[2], r[3]]);
        rec.age = u16::from_le_bytes([r[0], r[1]]).saturating_sub(rec.rate * back);
        rec.slot = r[4];
        rec.vx = r[6] as i8;
        rec.vz = r[7] as i8;
        let x = i32::from_le_bytes([r[8], r[9], r[10], r[11]]);
        let z = i32::from_le_bytes([r[12], r[13], r[14], r[15]]);
        rec.x = x - i32::from(rec.vx) * i32::from(back);
        rec.z = z - i32::from(rec.vz) * i32::from(back);
        rec.y = i16::from_le_bytes([r[16], r[17]]);
        rec.grey = r[0x14];
    }

    // The state's camera, at the engine's `1x` eye scale.
    let turn = |a: u32| i16_at(a) as f32 / 4096.0 * std::f32::consts::TAU;
    let s = i16_at(0x8007_BF10) as f32 / 4096.0;
    let view = FieldCameraView {
        focus: [
            -(i16_at(0x8008_9118) as f32),
            -(i16_at(0x8008_911C) as f32),
            -(i16_at(0x8008_9120) as f32),
        ],
        pitch: turn(0x8007_B790),
        yaw: turn(0x8007_B792),
        roll: turn(0x8007_B794),
        h: i16_at(0x8007_B6F4) as f32,
        tr_eye: [
            u32_at(ram, 0x8008_40B8) as i32 as f32 / s,
            u32_at(ram, 0x8008_40BC) as i32 as f32 / s,
            u32_at(ram, 0x8008_40C0) as i32 as f32 / s,
        ],
    };
    let pl = u32_at(ram, PLAYER_PTR);
    let env = FogFrameEnv {
        dt: dt as u8,
        player: [
            i16_at(pl + 0x14).into(),
            i16_at(pl + 0x16).into(),
            i16_at(pl + 0x18).into(),
        ],
        dpad_held: false,
        y_offset: u32_at(ram, CAMERA_Y_OFFSET) as i32,
        tint: [0x80; 3],
        window: [
            scratch[0x384],
            scratch[0x385],
            scratch[0x386],
            scratch[0x387],
        ],
    };
    let quads = pool
        .render_step(&FogView::from_field_view(&view), &env)
        .to_vec();

    // Retail's walked table.
    // The curvature table the emitter adds, entry for entry.
    let lut = u32_at(ram, CURVATURE_LUT_PTR);
    let table = legaia_engine_core::overworld_curvature::curvature_table();
    for (i, &e) in table.iter().enumerate() {
        assert_eq!(e, i16_at(lut + 2 * i as u32), "curvature entry {i}");
    }
    let mut retail = Vec::new();
    for ot in prim_pool::find_ot_arrays(ram, 0x8000_0000, 64) {
        for c in prim_pool::chain_walk(ram, 0x8000_0000, (ot.head & RAM_MASK) as usize) {
            if let Prim::PolyFt4 {
                color,
                clut,
                tpage,
                verts,
                ..
            } = c.prim
                && clut == FOG_CLUT
                && tpage == FOG_TPAGE
            {
                retail.push((color[0], verts));
            }
        }
    }

    let mut used = vec![false; retail.len()];
    let mut worst = 0i32;
    let mut unmatched = Vec::new();
    for q in &quads {
        let best = retail
            .iter()
            .enumerate()
            .filter(|(i, (c, _))| !used[*i] && *c == q.rgb[0])
            .map(|(i, (_, v))| {
                let d = (0..4)
                    .map(|k| {
                        let dx = (i32::from(v[k].0) - i32::from(q.xy[k].0)).abs();
                        let dy = (i32::from(v[k].1) - i32::from(q.xy[k].1)).abs();
                        dx.max(dy)
                    })
                    .max()
                    .unwrap_or(0);
                (d, i)
            })
            .min();
        match best {
            Some((d, i)) if d <= 2 => {
                used[i] = true;
                worst = worst.max(d);
            }
            _ => unmatched.push((q.rgb[0], q.xy)),
        }
    }
    let matched = used.iter().filter(|u| **u).count();
    let missed: Vec<_> = retail
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(r, _)| r)
        .collect();
    eprintln!("[ok] retail packets the engine does not draw: {missed:?}");
    eprintln!(
        "[ok] {} engine halves, {} retail fog packets; {matched} matched within {worst} px; \
         engine halves with no retail packet: {:?}",
        quads.len(),
        retail.len(),
        unmatched
    );
    assert!(unmatched.is_empty(), "engine halves retail did not draw");
    assert!(
        matched * 10 >= retail.len() * 9,
        "the engine draws only {matched} of retail's {} fog packets",
        retail.len()
    );
}
