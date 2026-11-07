//! The walk-on dispatcher `FUN_801D1EC4` (PROT 0897), idle half: the tile
//! compare, the object-cell filter, the kind-1 walk-on record and the kind-0
//! warp, over every field scene's real `.MAP` with the trigger-presence stamp
//! `FUN_80017BEC` and the map init's fallback-window stamp
//! (`FUN_8003AEB0`, `0x8003AFA8..0x8003B018`) run first. Held against
//! `walk_on_crossing` and `walk_on_repoll`, and the two stamps against
//! `refresh_object_grid_marks` and `stamp_fallback_walk_on_cells`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::field_regions::{
    MAP_TRIGGER_FALLBACK_OFFSET, parse_intra_scene_teleports, parse_tile_triggers,
    refresh_object_grid_marks, stamp_fallback_walk_on_cells,
};
use legaia_engine_field::field_warp_tile::{
    MOVEMENT_LOCK, WalkOnTables, walk_on_crossing, walk_on_repoll,
};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const DISPATCH: u32 = 0x801D_1EC4;
const STAMP: u32 = 0x8001_7BEC;
const INIT_STAMP: u32 = 0x8003_AFA8;
const INIT_STAMP_END: u32 = 0x8003_B01C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8013_0000;
const WARP_TIMER: u32 = 0x8007_B6B0;
const LAST_X: u32 = 0x8007_BDC8;
const LAST_Z: u32 = 0x8007_BDCC;
const DEST_X: u32 = 0x8007_BDD0;
const DEST_Z: u32 = 0x8007_BDD4;
const CLIP_GATE: u32 = 0x8007_B6A8;
const SCRATCH_FLAGS: u32 = 0x1F80_0394;
const REPOLL: u32 = 0x0008_0000;
const FALLBACK_LEN: usize = 0x2000;

fn index() -> ProtIndex {
    use legaia_engine_scene::vfs::Vfs;
    let vfs = legaia_engine_scene::vfs::DiscVfs::open(std::env::var_os("LEGAIA_DISC_BIN").unwrap())
        .expect("disc");
    let prot = vfs.read("prot.dat").expect("PROT.DAT");
    let names = vfs
        .read("cdname.txt")
        .or_else(|_| vfs.read("data/cdname.txt"))
        .expect("CDNAME.TXT");
    ProtIndex::from_bytes(prot, Some(std::str::from_utf8(&names).expect("text"))).expect("index")
}

/// The `.MAP` image retail holds, `0x12000` bytes and the fallback window
/// behind it, built from the same bytes the port's two table readers use.
/// `None` when the port's kind-0 and kind-1 readers would take their
/// fallbacks from different places, which no single RAM image can hold.
fn map_image(index: &ProtIndex, scene: &Scene) -> Option<Vec<u8>> {
    let bytes = scene.field_map_bytes(index).ok()??;
    if bytes.len() < MAP_TRIGGER_FALLBACK_OFFSET {
        return None;
    }
    let tail = &bytes[MAP_TRIGGER_FALLBACK_OFFSET..];
    let sibling = index
        .entry_bytes_extended(scene.field_map_index(index)? + 1)
        .unwrap_or_default();
    let (own1, own0) = (
        !parse_tile_triggers(tail).is_empty(),
        !parse_intra_scene_teleports(tail).is_empty(),
    );
    let window: &[u8] = match (own1, own0) {
        (true, true) => tail,
        (false, false) => &sibling,
        // Retail reads one window; when the port's two readers would split, the
        // window that holds anything decides and the other list must be empty
        // off the sibling too.
        (true, false) if parse_intra_scene_teleports(&sibling).is_empty() => tail,
        (false, true) if parse_tile_triggers(&sibling).is_empty() => tail,
        _ => return None,
    };
    let mut image = bytes[..MAP_TRIGGER_FALLBACK_OFFSET].to_vec();
    image.extend_from_slice(&window[..window.len().min(FALLBACK_LEN)]);
    image.resize(MAP_TRIGGER_FALLBACK_OFFSET + FALLBACK_LEN, 0);
    Some(image)
}

/// `FUN_8003BDE0`'s `(x, z, record, gate)`.
type Dispatch = (u32, u32, u32, u32);

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    dispatch: Option<Dispatch>,
    /// The kind-0 destination the warp was armed with.
    warp: Option<(u8, u8)>,
    /// The crossing tile left in `(_DAT_8007BDC8, _DAT_8007BDCC)`.
    last: (u32, u32),
}

#[test]
fn the_walk_on_dispatch_runs_what_retail_runs() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let index = index();
    let calls: Rc<RefCell<Vec<Dispatch>>> = Rc::default();
    let log = calls.clone();
    cpu.hook(0x8003_BDE0, move |r, _| {
        log.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2], r.r[A3]));
    });
    // The two warp fades.
    cpu.stub(0x801D_58F0, 0);
    cpu.stub(INIT_STAMP_END, 0);
    let mut rng = Rng(0x5741_4C4B);
    let (mut scenes, mut crossings, mut spawns, mut warps, mut filtered) = (0, 0, 0, 0, 0);
    let mut fallback_stamped = 0;
    let mut failures = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        if scene.field_map_index(&index).is_none() {
            continue;
        }
        let Some(image) = map_image(&index, &scene) else {
            continue;
        };
        scenes += 1;
        cpu.mem.load(MAP, &image);
        cpu.mem.wr32(MAP_PTR, MAP);
        assert_eq!(cpu.call(STAMP, &[]), Stop::Returned, "{name}");
        // The map init's own stamp over the fallback window, run on its own:
        // it starts at `0x8003AFA8` and ends at the `jal` that follows it.
        assert_eq!(cpu.call(INIT_STAMP, &[]), Stop::Returned, "{name}");
        let block = &image[legaia_engine_field::field_regions::MAP_REGION_BLOCK_OFFSET..];
        let fallback = &image[MAP_TRIGGER_FALLBACK_OFFSET..];
        let (k1p, k1f) = (parse_tile_triggers(block), parse_tile_triggers(fallback));
        let mut port_map = image[..MAP_TRIGGER_FALLBACK_OFFSET].to_vec();
        refresh_object_grid_marks(&mut port_map);
        let before = port_map.clone();
        stamp_fallback_walk_on_cells(&mut port_map, &k1f);
        fallback_stamped += (0x8000..0x10000)
            .step_by(2)
            .filter(|&o| before[o..o + 2] != port_map[o..o + 2])
            .count();
        let retail_map = cpu.mem.read(MAP, MAP_TRIGGER_FALLBACK_OFFSET);
        assert!(
            retail_map == port_map,
            "{name}: the stamp differs {:?}",
            crate::common::diff_offsets(&retail_map, &port_map)
        );

        let (k0p, k0f) = (
            parse_intra_scene_teleports(block),
            parse_intra_scene_teleports(fallback),
        );
        let tables = WalkOnTables {
            kind1: (&k1p, &k1f),
            kind0: (&k0p, &k0f),
        };
        let mut tiles: Vec<(u8, u8)> = k1p
            .iter()
            .chain(&k1f)
            .map(|t| (t.tile_x, t.tile_z))
            .chain(k0p.iter().chain(&k0f).map(|t| (t.tile_x, t.tile_z)))
            .filter(|&(x, z)| x < 0x80 && z < 0x80)
            .collect();
        for _ in 0..32 {
            tiles.push((rng.below(0x80) as u8, rng.below(0x80) as u8));
        }
        tiles.sort_unstable();
        tiles.dedup();
        for &(tx, tz) in &tiles {
            let cell = u16::from_le_bytes([
                port_map[0x8000 + usize::from(tx) * 2 + usize::from(tz) * 0x100],
                port_map[0x8000 + usize::from(tx) * 2 + usize::from(tz) * 0x100 + 1],
            ]);
            for case in 0..4u32 {
                let locked = case & 1 != 0;
                let same_tile = case & 2 != 0;
                let repoll = rng.below(2) == 0;
                let (x, z) = (
                    i32::from(tx) * 128 + rng.below(128) as i32,
                    i32::from(tz) * 128 + rng.below(128) as i32,
                );
                cpu.mem.wr16(ACTOR + 0x14, x as u16);
                cpu.mem.wr16(ACTOR + 0x18, z as u16);
                cpu.mem
                    .wr32(ACTOR + 0x10, if locked { MOVEMENT_LOCK } else { 0 });
                cpu.mem.wr32(WARP_TIMER, 0);
                cpu.mem.wr8(CLIP_GATE, 0);
                cpu.mem.wr32(SCRATCH_FLAGS, if repoll { REPOLL } else { 0 });
                let last = if same_tile {
                    (u32::from(tx), u32::from(tz))
                } else {
                    (0xFFFF, 0xFFFF)
                };
                cpu.mem.wr32(LAST_X, last.0);
                cpu.mem.wr32(LAST_Z, last.1);
                cpu.mem.wr32(DEST_X, 0);
                cpu.mem.wr32(DEST_Z, 0);
                calls.borrow_mut().clear();
                assert_eq!(cpu.call(DISPATCH, &[ACTOR]), Stop::Returned, "{name}");
                let retail = Outcome {
                    dispatch: calls.borrow().first().copied(),
                    warp: (cpu.mem.rd32(WARP_TIMER) == 0x26)
                        .then(|| (cpu.mem.rd32(DEST_X) as u8, cpu.mem.rd32(DEST_Z) as u8)),
                    last: (cpu.mem.rd32(LAST_X), cpu.mem.rd32(LAST_Z)),
                };

                let tile = (tx, tz);
                let port = if same_tile {
                    Outcome {
                        dispatch: repoll
                            .then(|| walk_on_repoll(locked, tile, &tables))
                            .flatten()
                            .map(|t| {
                                (
                                    u32::from(tx),
                                    u32::from(tz),
                                    u32::from(t.record),
                                    u32::from(t.gate),
                                )
                            }),
                        warp: None,
                        last,
                    }
                } else {
                    let c = walk_on_crossing(cell, locked, tile, &tables);
                    Outcome {
                        dispatch: c.kind1.map(|t| {
                            (
                                u32::from(tx),
                                u32::from(tz),
                                u32::from(t.record),
                                u32::from(t.gate),
                            )
                        }),
                        warp: c.warp,
                        last: (u32::from(tx), u32::from(tz)),
                    }
                };
                crossings += 1;
                spawns += usize::from(retail.dispatch.is_some_and(|d| d.3 != 0));
                warps += usize::from(retail.warp.is_some());
                let any_table = tables
                    .kind1
                    .0
                    .iter()
                    .chain(tables.kind1.1)
                    .any(|t| (t.tile_x, t.tile_z) == tile)
                    || tables
                        .kind0
                        .0
                        .iter()
                        .chain(tables.kind0.1)
                        .any(|t| (t.tile_x, t.tile_z) == tile);
                filtered += usize::from(!same_tile && !locked && any_table && cell & 0x600 == 0);
                if retail != port && failures.len() < 24 {
                    failures.push(format!(
                        "{name} tile {tile:?} cell {cell:#06x} locked {locked} same {same_tile} \
                         repoll {repoll}: retail {retail:?} port {port:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "{scenes} scenes, {crossings} crossings, {spawns} spawns, {warps} warps, \
         {filtered} table tiles the cell filter stops, {fallback_stamped} cells only the \
         fallback stamp marks"
    );
    assert!(
        scenes > 100 && spawns > 100 && warps > 50,
        "{scenes} {spawns} {warps}"
    );
}
