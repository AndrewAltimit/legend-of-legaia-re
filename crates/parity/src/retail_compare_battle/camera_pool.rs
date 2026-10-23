//! The displayed frame's camera, checked against the ordering tables.
//!
//! The battle floor is a procedural grid (`func_0x801d02c0`,
//! `docs/subsystems/battle-stage-camera.md`): every `POLY_GT4` it emits
//! (CLUT `0x77C0`, tpage `0x000D`) has its corners on a fixed lattice of
//! `0x100`-unit sub-cells, projected through the battle camera with the 4x
//! world scale. So a pose can be scored against an ordering table by how far
//! each drawn corner sits from the nearest projected lattice point. The table
//! being built fits the RAM's live pose; the older one fits the pose one pass
//! back, which names the step that pass walked the camera
//! ([`pass_steps`] reads it off the frame history; the grid confirms it).

use super::*;
use legaia_engine_camera::projection::{BATTLE_WORLD_SCALE, BattleCamPose, battle_vp};
use legaia_ram_image::extract::PSX_RAM_KSEG0;
use legaia_ram_image::prim_pool::{self, Prim};

/// The floor grid's texture address (`77c0c0c0 000dc0df ..`, the emitter's
/// literal UV words).
const GROUND_CLUT: u16 = 0x77C0;
const GROUND_TPAGE: u16 = 0x000D;
/// The 28 x 28-cell grid's corner lattice: `x0 = -0x1C00`, `z0 = -0x1E00`,
/// sub-cell `0x100`, 57 corners a side (`battle_backdrop::grid_origin`).
const LATTICE_X0: f32 = -7168.0;
const LATTICE_Z0: f32 = -7680.0;
const LATTICE_STEP: f32 = 256.0;
const LATTICE_SIDE: usize = 57;
/// Fewest floor quads a table needs to score a pose.
const MIN_QUADS: usize = 64;
/// Mean corner error (pixels) under which the newer table counts as drawn at
/// the RAM's pose.
const LIVE_FIT_PX: f32 = 1.0;

/// One floor `POLY_GT4`'s screen corners.
type FloorQuad = [(i16, i16); 4];

/// Every ordering table's floor quads, by table.
fn ground_tables(ram: &[u8]) -> Vec<Vec<FloorQuad>> {
    ground_tables_at(ram).into_iter().map(|(_, q)| q).collect()
}

/// [`ground_tables`] keyed by each table's bucket-array start.
fn ground_tables_at(ram: &[u8]) -> Vec<(u32, Vec<FloorQuad>)> {
    prim_pool::find_ot_arrays(ram, PSX_RAM_KSEG0, 64)
        .iter()
        .filter_map(|ot| {
            let head = ot.head.checked_sub(PSX_RAM_KSEG0)? as usize;
            let quads: Vec<_> = prim_pool::chain_walk(ram, PSX_RAM_KSEG0, head)
                .into_iter()
                .filter_map(|c| match c.prim {
                    Prim::PolyGt4 {
                        verts, clut, tpage, ..
                    } if clut == GROUND_CLUT && tpage == GROUND_TPAGE => Some(verts),
                    _ => None,
                })
                .collect();
            (quads.len() >= MIN_QUADS).then_some((ot.start, quads))
        })
        .collect()
}

/// The newest ordering table by the battle floor, for [`display_phase`]: the
/// one whose floor fits the RAM's live camera pose while the other's does
/// not. `None` without two floor tables, or when the camera did not move
/// across the pass (both fit).
pub(super) fn newest_table_by_ground(ram: &[u8], ctx: u32) -> Option<u32> {
    let tables = ground_tables_at(ram);
    if tables.len() < 2 {
        return None;
    }
    let (live, _) = retail_cam_tween(ram, ctx);
    let fits: Vec<u32> = tables
        .iter()
        .filter(|(_, q)| ground_error(&live, q) <= LIVE_FIT_PX)
        .map(|(start, _)| *start)
        .collect();
    (fits.len() == 1).then(|| fits[0])
}

/// The floor lattice projected under `pose`, screen pixels over 320 x 240.
fn project_lattice(pose: &BattleCamPose) -> Vec<(f32, f32)> {
    let m = battle_vp(pose, BATTLE_WORLD_SCALE, 4.0 / 3.0);
    let mut out = Vec::with_capacity(LATTICE_SIDE * LATTICE_SIDE);
    for i in 0..LATTICE_SIDE {
        for j in 0..LATTICE_SIDE {
            let v = [
                (LATTICE_X0 + i as f32 * LATTICE_STEP) * BATTLE_WORLD_SCALE,
                0.0,
                (LATTICE_Z0 + j as f32 * LATTICE_STEP) * BATTLE_WORLD_SCALE,
                1.0,
            ];
            let c: [f32; 4] = std::array::from_fn(|r| (0..4).map(|k| m[k * 4 + r] * v[k]).sum());
            if c[3] > 1.0 {
                out.push(((c[0] / c[3] + 1.0) * 160.0, (1.0 - c[1] / c[3]) * 120.0));
            }
        }
    }
    out
}

/// Mean distance from each on-screen corner of `quads` to the nearest
/// lattice point projected under `pose`.
fn ground_error(pose: &BattleCamPose, quads: &[[(i16, i16); 4]]) -> f32 {
    let pts = project_lattice(pose);
    let (mut sum, mut n) = (0.0f32, 0u32);
    for &(x, y) in quads.iter().flatten() {
        if !(0..320).contains(&x) || !(0..240).contains(&y) {
            continue;
        }
        let d = pts
            .iter()
            .map(|&(px, py)| (px - f32::from(x)).powi(2) + (py - f32::from(y)).powi(2))
            .fold(f32::MAX, f32::min);
        sum += d.sqrt();
        n += 1;
    }
    if n == 0 { f32::MAX } else { sum / n as f32 }
}

/// The step the pass the RAM holds walked the camera, as the older ordering
/// table's floor shows it: of the steps `1..=4`, the one whose one-pass
/// walk-back the older table fits best. `None` without two floor tables, when
/// no table fits the live pose (the RAM's camera is not the one the newer
/// table drew), or when the camera did not move on that pass (every
/// candidate is the live pose).
pub(super) fn older_table_step(ram: &[u8], ctx: u32) -> Option<u8> {
    let tables = ground_tables(ram);
    if tables.len() < 2 {
        return None;
    }
    let (live, _) = retail_cam_tween(ram, ctx);
    let live_err: Vec<f32> = tables.iter().map(|t| ground_error(&live, t)).collect();
    let newer = (0..tables.len()).min_by(|&a, &b| live_err[a].total_cmp(&live_err[b]))?;
    if live_err[newer] > LIVE_FIT_PX {
        return None;
    }
    let older = (0..tables.len()).find(|&t| t != newer)?;
    let poses: Vec<(u8, BattleCamPose)> = (1..=4u8)
        .map(|s| {
            (
                s,
                displayed_cam_live_steps(ram, ctx, &[s], false, [None; 9]),
            )
        })
        .collect();
    if poses.iter().all(|(_, p)| *p == live) {
        return None;
    }
    poses
        .iter()
        .map(|(s, p)| (*s, ground_error(p, &tables[older])))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
}
