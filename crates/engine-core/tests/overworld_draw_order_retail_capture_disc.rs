//! Retail capture: the overworld draw order between the continent and the
//! fog sheets, bucket for bucket.
//!
//! `keikoku_chest_preload` (`map01`, the kingdom overworld, retail SCUS) holds
//! a whole frame's ordering table in RAM. Walking it bucket by bucket gives
//! each packet's bucket, and so the order retail draws the continent's cells
//! (`FUN_801F89B8`, PROT 0901) and the fog halves (`FUN_8003F86C`) in:
//!
//! 1. **The keys.** Every fog half the port's render step reproduces sits at
//!    `(SZ - 0x10) >> 5` and every continent cell the port's heightfield
//!    reproduces at `(max corner SZ >> 5) + 14`, both relative to the one base
//!    pointer `*0x1F8003F4` - so the two are on one scale
//!    (`legaia_engine_core::overworld_draw_order`).
//! 2. **Ties.** Where a bucket holds both, the sheet comes first in the chain,
//!    so the cell covers it.
//! 3. **Coverage.** Rasterising the walked table in chain order gives the
//!    share of the fog's light a later continent cell covers. The port's
//!    flat per-bucket depth policy over the port's own geometry must land on
//!    that share; the per-pixel policy it replaces does not.
//!
//! Skips (and passes) when the scenario manifest or the save library is
//! missing.

use legaia_engine_core::fog_particles::{
    FOG_CLUT, FOG_POOL_SLOTS, FOG_TPAGE, FogFrameEnv, FogPool, FogQuad, FogView,
};
use legaia_engine_core::overworld_curvature::curvature_at;
use legaia_engine_core::overworld_draw_order as order;
use legaia_engine_vm::psx_camera::{FieldCameraView, mat4_mul, mat4_scale, mat4_translation};
use legaia_mednafen::prim_pool::{self, Prim};
use legaia_mednafen::{SaveState, ScenarioManifest};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

const RAM_MASK: u32 = 0x001F_FFFF;
const FOG_POOL_PTR: u32 = 0x8007_B7E0;
const PLAYER_PTR: u32 = 0x8007_C364;
const CAMERA_Y_OFFSET: u32 = 0x8007_BCAC;
const FRAME_STEP_SCRATCH: usize = 0x393;
/// Scratchpad `0x1F8003F4`: the ordering-table base pointer both emitters
/// index (`lw t4,0xe0(t6)` / `lw t3,0xe0(t9)` off `0x1F800314`).
const OT_BASE_SCRATCH: usize = 0x3F4;
/// Scratchpad `0x1F8003EC`: the field-env block (the streamed `.MAP`).
const FIELD_ENV_SCRATCH: usize = 0x3EC;
/// Scratchpad `0x1F80035C`: the 16-entry floor-height ladder.
const FLOOR_LADDER_SCRATCH: usize = 0x35C;
const FIELD_MAP_BYTES: usize = 0x12000;
const W: usize = 320;
const H: usize = 240;

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

/// Every packet of one ordering table in chain order, with its bucket (the
/// index of the last table word the walk passed).
fn walk_with_buckets(ram: &[u8], ot: &prim_pool::OtArray) -> HashMap<usize, (usize, u32)> {
    let mut out = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let mut cursor = ot.head & RAM_MASK;
    let (lo, hi) = (ot.start & RAM_MASK, ot.end & RAM_MASK);
    let mut bucket = 0u32;
    let mut order = 0usize;
    while seen.insert(cursor) && (cursor as usize) + 4 <= ram.len() {
        let tag = u32_at(ram, cursor);
        if (lo..hi).contains(&cursor) {
            bucket = (cursor - lo) / 4;
        } else {
            out.insert(cursor as usize, (order, bucket));
            order += 1;
        }
        let next = tag & 0x00FF_FFFF;
        if next == 0x00FF_FFFF {
            break;
        }
        cursor = next & RAM_MASK;
    }
    out
}

/// Pixel-centre coverage of one triangle, with the barycentric weights.
fn raster_tri(v: [(f32, f32); 3], mut f: impl FnMut(usize, [f32; 3])) {
    let (x0, y0) = v[0];
    let (x1, y1) = v[1];
    let (x2, y2) = v[2];
    let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
    if area.abs() < 1e-6 {
        return;
    }
    let minx = x0.min(x1).min(x2).floor().max(0.0) as usize;
    let maxx = (x0.max(x1).max(x2).ceil().min(W as f32) as usize).min(W);
    let miny = y0.min(y1).min(y2).floor().max(0.0) as usize;
    let maxy = (y0.max(y1).max(y2).ceil().min(H as f32) as usize).min(H);
    for py in miny..maxy {
        for px in minx..maxx {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            let w0 = ((x1 - x) * (y2 - y) - (x2 - x) * (y1 - y)) / area;
            let w1 = ((x2 - x) * (y0 - y) - (x0 - x) * (y2 - y)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                f(py * W + px, [w0, w1, w2]);
            }
        }
    }
}

/// A `POLY_FT4`'s two triangles in PSX vertex order `(0,1,2)` + `(1,3,2)`.
fn raster_quad(v: [(f32, f32); 4], mut f: impl FnMut(usize, [f32; 4])) {
    raster_tri([v[0], v[1], v[2]], |p, b| f(p, [b[0], b[1], b[2], 0.0]));
    raster_tri([v[1], v[3], v[2]], |p, b| f(p, [0.0, b[0], b[2], b[1]]));
}

fn same_quad(a: &[(i32, i32); 4], b: &[(i16, i16); 4], tol: i32) -> bool {
    let mut used = [false; 4];
    a.iter().all(|&(x, y)| {
        let hit = (0..4).find(|&k| {
            !used[k] && (i32::from(b[k].0) - x).abs() <= tol && (i32::from(b[k].1) - y).abs() <= tol
        });
        if let Some(k) = hit {
            used[k] = true;
        }
        hit.is_some()
    })
}

/// Summed corner distance between a cell and a packet, each packet corner
/// paired with its nearest cell corner.
fn quad_error(a: &[(i32, i32); 4], b: &[(i16, i16); 4]) -> i32 {
    b.iter()
        .map(|&(x, y)| {
            a.iter()
                .map(|&(ax, ay)| (ax - i32::from(x)).abs() + (ay - i32::from(y)).abs())
                .min()
                .unwrap_or(0)
        })
        .sum()
}

#[test]
fn overworld_fog_and_continent_draw_in_retail_bucket_order() {
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
    let scratch_u32 = |o: usize| u32::from_le_bytes(scratch[o..o + 4].try_into().unwrap());
    let dt = u16::from(scratch[FRAME_STEP_SCRATCH]);
    let back = 2 * dt;
    assert!(
        scratch[0x394] & 1 != 0,
        "the map01 state holds the overworld bit"
    );

    // ---- The port's side: fog pool rolled back to the walked table's frame.
    let pool_va = u32_at(ram, FOG_POOL_PTR);
    let mut pool = FogPool::new();
    pool.overworld = true;
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
    // The hosts draw the overworld through the walk frame, which composes
    // the 6x world scale about the player into its matrix
    // (`camera_view::world_map_walk_vp`); the fog projects through the `1x`
    // field view of the same pose and scales its depths onto that frame
    // (`World::field_fx_view`).
    let scale = legaia_engine_core::camera_view::WORLD_MAP_WORLD_SCALE;
    let walk_view = FieldCameraView {
        focus: [0.0; 3],
        tr_eye: view.tr_eye.map(|c| c * scale),
        ..view
    };
    let mesh = mat4_mul(
        &walk_view.vp(4.0 / 3.0),
        &mat4_mul(
            &mat4_scale(scale),
            // Y-up render frame: the raw Y-down focus flips its Y.
            &mat4_translation([-view.focus[0], view.focus[1], -view.focus[2]]),
        ),
    );
    let fv = FogView::from_field_view(&view).with_depth_scale(scale);
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
    let quads: Vec<FogQuad> = pool.render_step(&fv, &env).to_vec();

    // The continent as the port builds it, from the state's own `.MAP` and
    // floor ladder.
    let map_off = (scratch_u32(FIELD_ENV_SCRATCH) & RAM_MASK) as usize;
    let map = &ram[map_off..map_off + FIELD_MAP_BYTES];
    let lut: [i16; 16] = std::array::from_fn(|i| {
        let o = FLOOR_LADDER_SCRATCH + 2 * i;
        i16::from_le_bytes([scratch[o], scratch[o + 1]])
    });
    let hf = legaia_asset::field_objects::build_walk_heightfield(map, &lut);
    assert!(hf.quad_count() > 1000, "map01's continent resolves");
    let mesh_clip = |p: [f32; 3]| -> [f32; 4] {
        // Y-up render frame: raw retail points flip Y.
        let v = [p[0], -p[1], p[2], 1.0];
        std::array::from_fn(|r| (0..4).map(|c| mesh[4 * c + r] * v[c]).sum())
    };
    struct Cell {
        xy: [(i32, i32); 4],
        xy_f: [(f32, f32); 4],
        ndc: [f32; 4],
        key: u32,
        /// The depth-cued packet colour the port draws the cell with
        /// (`overworld_ground_cue`, keyed on corner `(x1, z0)`).
        cue: u8,
    }
    let mut cells = Vec::new();
    for c in hf.positions.as_chunks::<4>().0 {
        let mut xy = [(0i32, 0i32); 4];
        let mut xy_f = [(0f32, 0f32); 4];
        let mut ndc = [0f32; 4];
        let mut sz = [0u32; 4];
        let mut ok = true;
        for k in 0..4 {
            // The heightfield stores `-ladder[n]`; retail's raw Y is the
            // ladder value itself (`lh s1,0x48(s1)` at `0x801F8A7C`).
            let raw = [c[k][0], -c[k][1], c[k][2]];
            let clip = mesh_clip(raw);
            if clip[3] <= 1.0 {
                ok = false;
                break;
            }
            let sz_k = clip[3].round().clamp(0.0, 65535.0);
            let bend = curvature_at(sz_k as i32) as f32;
            let sx = (clip[0] / clip[3] + 1.0) * 0.5 * W as f32;
            let sy = (1.0 - clip[1] / clip[3]) * 0.5 * H as f32 + bend;
            xy_f[k] = (sx, sy);
            xy[k] = (sx.round() as i32, sy.round() as i32);
            ndc[k] = clip[2] / clip[3];
            sz[k] = sz_k as u32;
        }
        if ok {
            cells.push(Cell {
                xy,
                xy_f,
                ndc,
                key: order::ground_ot_index(sz),
                cue: legaia_engine_core::overworld_ground_cue::ground_cue_color(
                    legaia_asset::field_objects::GROUND_PRIM_COLOR,
                    sz[1],
                )[0],
            });
        }
    }

    // ---- Retail's side: the frame's full table, bucket for bucket.
    let ot = prim_pool::find_ot_arrays(ram, 0x8000_0000, 64)
        .into_iter()
        .max_by_key(|ot| {
            prim_pool::chain_walk(ram, 0x8000_0000, (ot.head & RAM_MASK) as usize)
                .iter()
                .filter(|c| {
                    matches!(c.prim, Prim::PolyFt4 { clut, tpage, .. }
                        if clut == FOG_CLUT && tpage == FOG_TPAGE)
                })
                .count()
        })
        .expect("an ordering table");
    let buckets = walk_with_buckets(ram, &ot);
    let chain = prim_pool::chain_walk(ram, 0x8000_0000, (ot.head & RAM_MASK) as usize);

    // Match the port's fog halves and continent cells onto retail packets.
    let mut fog_delta: BTreeMap<i64, usize> = BTreeMap::new();
    let mut ground_delta: BTreeMap<i64, usize> = BTreeMap::new();
    let mut fog_used = vec![false; quads.len()];
    let mut cell_used = vec![false; cells.len()];
    // Chain order + colour of every retail fog packet; chain order of every
    // matched continent packet and of every opaque polygon.
    let mut retail_fog = Vec::new();
    let mut retail_ground = Vec::new();
    let mut retail_opaque = Vec::new();
    let mut ground_candidates = 0usize;
    // Retail packet colour minus the port's cue, per matched cell.
    let mut cue_delta: BTreeMap<i32, usize> = BTreeMap::new();
    let mut tie_buckets: BTreeMap<u32, (Option<usize>, Option<usize>)> = BTreeMap::new();
    for c in &chain {
        let Some(&(ord, bucket)) = buckets.get(&c.offset) else {
            continue;
        };
        match c.prim {
            Prim::PolyFt4 {
                clut,
                tpage,
                color,
                verts,
                ..
            } if clut == FOG_CLUT && tpage == FOG_TPAGE => {
                retail_fog.push((ord, color[0], verts));
                let hit = quads.iter().enumerate().find(|(i, q)| {
                    !fog_used[*i]
                        && q.rgb[0] == color[0]
                        && same_quad(&q.xy.map(|(x, y)| (i32::from(x), i32::from(y))), &verts, 2)
                });
                if let Some((i, q)) = hit {
                    fog_used[i] = true;
                    *fog_delta
                        .entry(i64::from(bucket) - i64::from(q.ot_index))
                        .or_default() += 1;
                    let e = tie_buckets.entry(bucket).or_default();
                    e.0 = Some(e.0.map_or(ord, |o: usize| o.max(ord)));
                }
            }
            Prim::PolyFt4 {
                cmd, verts, color, ..
            } if cmd & 2 == 0 => {
                ground_candidates += 1;
                retail_opaque.push((ord, verts.to_vec()));
                // The nearest cell by summed corner error: far cells near the
                // horizon are a few pixels across, so a tolerance alone can
                // hand a packet its neighbour.
                let hit = cells
                    .iter()
                    .enumerate()
                    .filter(|(i, cl)| !cell_used[*i] && same_quad(&cl.xy, &verts, 2))
                    .min_by_key(|(_, cl)| quad_error(&cl.xy, &verts));
                if let Some((i, cl)) = hit {
                    cell_used[i] = true;
                    retail_ground.push((ord, verts));
                    *cue_delta
                        .entry(i32::from(color[0]) - i32::from(cl.cue))
                        .or_default() += 1;
                    *ground_delta
                        .entry(i64::from(bucket) - i64::from(cl.key))
                        .or_default() += 1;
                    let e = tie_buckets.entry(bucket).or_default();
                    e.1 = Some(e.1.map_or(ord, |o: usize| o.min(ord)));
                }
            }
            ref p => {
                let semi = (p.cmd() >> 1) & 1 != 0;
                let v = p.verts();
                if !semi && v.len() >= 3 && !matches!(p.kind(), k if k.starts_with("SPRT")) {
                    retail_opaque.push((ord, v));
                }
            }
        }
    }

    // 1. The keys: one offset for every fog half and every continent cell -
    //    the base pointer's position in its table.
    let base = scratch_u32(OT_BASE_SCRATCH);
    let base_index = prim_pool::find_ot_arrays(ram, 0x8000_0000, 64)
        .iter()
        .find(|o| (o.start..o.end).contains(&base))
        .map(|o| i64::from((base - o.start) / 4));
    eprintln!(
        "[ok] OT 0x{:08X}: base pointer 0x{base:08X} = bucket {base_index:?} of its table",
        ot.start
    );
    eprintln!(
        "[ok] fog halves matched {} of {} retail fog packets; bucket - port key: {fog_delta:?}",
        fog_used.iter().filter(|u| **u).count(),
        retail_fog.len()
    );
    eprintln!(
        "[ok] continent cells matched {} of {ground_candidates} opaque POLY_FT4 packets; \
         bucket - port key: {ground_delta:?}",
        retail_ground.len()
    );
    let offset = *fog_delta.iter().max_by_key(|(_, n)| **n).unwrap().0;
    assert_eq!(
        Some(offset),
        base_index,
        "the fog key is relative to *0x1F8003F4"
    );
    assert_eq!(fog_delta.len(), 1, "every fog half at (SZ - 0x10) >> 5");
    let exact = ground_delta.get(&offset).copied().unwrap_or(0);
    // The port's `SZ` is a rounded float against the GTE's integer divide, so
    // a corner on a bucket edge can land one bucket over.
    let within_one: usize = (offset - 1..=offset + 1)
        .filter_map(|d| ground_delta.get(&d))
        .sum();
    assert!(
        retail_ground.len() >= 200,
        "the continent's cells reproduce"
    );
    assert!(
        exact * 100 >= retail_ground.len() * 85 && within_one * 100 >= retail_ground.len() * 95,
        "continent cells at (max SZ >> 5) + 14: {exact} exact, {within_one} within one, of {}",
        retail_ground.len()
    );

    // 1b. The depth cue: every matched cell's packet colour is `DPCS` of the
    //     neutral base toward the far colour `0x1000`, keyed on the depth of
    //     corner `(x1, z0)` (`FUN_801F89B8`, `legaia_engine_core::
    //     overworld_ground_cue`). The residue is the few cells whose bucket
    //     key also misses (a far horizon cell matched to its neighbour).
    let cue_exact = cue_delta.get(&0).copied().unwrap_or(0);
    let cue_near: usize = (-3..=3).filter_map(|d| cue_delta.get(&d)).sum();
    let cued = cells
        .iter()
        .zip(&cell_used)
        .filter(|(c, u)| **u && c.cue > 0x80)
        .count();
    eprintln!(
        "[ok] ground cue: retail colour - port cue {cue_delta:?} over {} cells ({cued} cued past 0x80)",
        retail_ground.len()
    );
    assert!(cued >= 50, "the frame reaches past SZ 0x5000");
    assert!(
        cue_exact * 100 >= retail_ground.len() * 95 && cue_near * 100 >= retail_ground.len() * 99,
        "ground packet colour is the cue: {cue_exact} exact, {cue_near} within three, of {}",
        retail_ground.len()
    );

    // 2. Ties: in a bucket holding both, every sheet precedes every cell.
    let shared: Vec<_> = tie_buckets
        .values()
        .filter_map(|(f, g)| Some(((*f)?, (*g)?)))
        .collect();
    eprintln!("[ok] buckets holding a sheet and a cell: {}", shared.len());
    assert!(
        shared
            .iter()
            .all(|(last_fog, first_cell)| last_fog < first_cell),
        "a cell covers a sheet in its own bucket"
    );

    // 3. Coverage: the share of the fog's light a later cell covers.
    let mut last_ground = vec![None::<usize>; W * H];
    for (ord, v) in &retail_ground {
        raster_quad(v.map(|(x, y)| (x as f32, y as f32)), |p, _| {
            last_ground[p] = Some(last_ground[p].map_or(*ord, |o: usize| o.max(*ord)));
        });
    }
    let mut last_opaque = vec![None::<usize>; W * H];
    for (ord, v) in &retail_opaque {
        let f: Vec<(f32, f32)> = v.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut mark = |p: usize| {
            last_opaque[p] = Some(last_opaque[p].map_or(*ord, |o: usize| o.max(*ord)));
        };
        if f.len() == 4 {
            raster_quad([f[0], f[1], f[2], f[3]], |p, _| mark(p));
        } else {
            raster_tri([f[0], f[1], f[2]], |p, _| mark(p));
        }
    }
    let (mut total, mut hid_ground, mut hid_opaque) = (0f64, 0f64, 0f64);
    for (ord, grey, v) in &retail_fog {
        raster_quad(v.map(|(x, y)| (x as f32, y as f32)), |p, _| {
            let w = f64::from(*grey);
            total += w;
            if last_ground[p].is_some_and(|o| o > *ord) {
                hid_ground += w;
            }
            if last_opaque[p].is_some_and(|o| o > *ord) {
                hid_opaque += w;
            }
        });
    }
    let retail_share = hid_ground / total;

    // The port's depth buffer over its own continent, three ways: the
    // per-pixel depth it drew with, the flat bucket depth that replaces it,
    // and the ordering table's own rule on the port's keys.
    let mut z_pixel = vec![f32::INFINITY; W * H];
    let mut z_flat = vec![f32::INFINITY; W * H];
    let mut k_min = vec![u32::MAX; W * H];
    for cl in &cells {
        let flat = order::ndc_at_w(&mesh, order::bucket_sz(cl.key));
        raster_quad(cl.xy_f, |p, b| {
            let z = (0..4).map(|k| b[k] * cl.ndc[k]).sum::<f32>();
            z_pixel[p] = z_pixel[p].min(z);
            z_flat[p] = z_flat[p].min(flat);
            k_min[p] = k_min[p].min(cl.key);
        });
    }
    let unscaled = FogView::from_field_view(&view);
    let (mut port_total, mut hid_old, mut hid_pixel, mut hid_flat, mut hid_rule) =
        (0f64, 0f64, 0f64, 0f64, 0f64);
    for q in &quads {
        let depth = q.depth.expect("an overworld sheet carries its depth");
        // What the sheet carried before: the particle's per-pixel depth at
        // the field view's `w`, unscaled onto the walk frame.
        let old = unscaled
            .ndc_at_sz(q.sz)
            .expect("a drawn sheet is in front of the eye");
        let scaled = order::ndc_at_w(&mesh, q.sz);
        raster_quad(q.xy.map(|(x, y)| (x as f32, y as f32)), |p, _| {
            let w = f64::from(q.rgb[0]);
            port_total += w;
            if old >= z_pixel[p] {
                hid_old += w;
            }
            if scaled >= z_pixel[p] {
                hid_pixel += w;
            }
            if depth >= z_flat[p] {
                hid_flat += w;
            }
            if k_min[p] <= q.ot_index {
                hid_rule += w;
            }
        });
    }
    eprintln!(
        "[ok] retail: continent cells cover {:.1}% of the fog's light ({hid_ground:.0} of \
         {total:.0} grey-pixels); every opaque polygon {:.1}%",
        100.0 * retail_share,
        100.0 * hid_opaque / total
    );
    eprintln!(
        "[ok] port, same frame ({port_total:.0} grey-pixels): per-pixel depth at the unscaled \
         field-view w {:.1}%, per-pixel at the walk frame's w {:.1}%, flat bucket depth {:.1}%, \
         bucket rule {:.1}%",
        100.0 * hid_old / port_total,
        100.0 * hid_pixel / port_total,
        100.0 * hid_flat / port_total,
        100.0 * hid_rule / port_total
    );
    // The flat depth buffer is the ordering table's rule, pixel for pixel.
    assert!(
        (hid_flat - hid_rule).abs() <= 0.002 * port_total,
        "flat depth disagrees with the bucket rule"
    );
    // And it lands on retail's covered share to within half a point; both
    // per-pixel readings miss it by more (the unscaled one hides nothing,
    // the scaled one over-hides the slopes).
    let flat_share = hid_flat / port_total;
    let miss = (flat_share - retail_share).abs();
    assert!(
        miss < 0.005,
        "port covers {flat_share:.4} of the fog, retail {retail_share:.4}"
    );
    for (name, hid) in [("unscaled", hid_old), ("per-pixel", hid_pixel)] {
        assert!(
            (hid / port_total - retail_share).abs() > miss,
            "the {name} policy was already as close"
        );
    }
}
