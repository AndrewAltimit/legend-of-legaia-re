//! Disc-gated: tile-board tile-actor visual draw wiring.
//!
//! Two layers:
//! 1. A census over every scene MAN's partition scripts for the field-VM
//!    op `0x49` sub-op `5` tile-board install (the inline 13-byte header
//!    `_DAT_8007b450` points at). The pinned result is NEGATIVE: no retail
//!    scene MAN carries a board install - the tile board is a dev/unused
//!    minigame mode reachable only through the op itself (which is why the
//!    play-window exposes the `LEGAIA_TILE_BOARD_DEMO=1` synthetic trigger).
//! 2. A boot of a real field scene through [`BootSession`], installing a
//!    board from the same 14-byte op window the field VM would deliver,
//!    then asserting the shell draw assembly
//!    ([`legaia_engine_shell::tile_board_draws`]) yields a per-cell draw at
//!    every drawable cell, with every tile actor's template mesh resolved
//!    from the resident global TMD pool (the set the play-window redraw
//!    pass uploads + draws).
//!
//! Skips when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
use legaia_engine_core::man_field_scripts::partition_record_span;
use legaia_engine_core::tile_board::{self, TileBoardHeader};
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_engine_shell::tile_board_draws;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Decode a MAN from a scene-entry file: the bare `scene_asset_table` at 0,
/// the scripted-table wrapper, or the v12-embedded 0x800-aligned fallback
/// (the same resolution order as `legaia_engine_core::scene_bundle`).
fn load_man_from_scene(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut candidates: Vec<usize> = Vec::new();
    if legaia_asset::scene_asset_table::detect(bytes).is_some() {
        candidates.push(0);
    }
    if let Some(info) = legaia_asset::scene_scripted_asset_table::detect(bytes) {
        candidates.push(info.asset_table_offset);
    }
    let mut off = 0x800;
    while off + 0x40 <= bytes.len() {
        if legaia_asset::scene_asset_table::detect(&bytes[off..]).is_some() {
            candidates.push(off);
        }
        off += 0x800;
    }
    for table_offset in candidates {
        let Some(table) = legaia_asset::scene_asset_table::detect(&bytes[table_offset..]) else {
            continue;
        };
        let Some(man) = table
            .descriptors
            .iter()
            .take(table.count)
            .find(|d| d.type_byte == 0x03)
            .copied()
        else {
            continue;
        };
        let start = table_offset + man.data_offset as usize;
        if man.size == 0 || man.data_offset == 0 || start >= bytes.len() {
            continue;
        }
        let Ok((decoded, _)) = legaia_lzs::decompress_tracked(&bytes[start..], man.size as usize)
        else {
            continue;
        };
        if decoded.len() == man.size as usize {
            return Some(decoded);
        }
    }
    None
}

/// Scan every partition record of one MAN as a field-VM script for
/// op-`0x49` sub-`5` installs whose operand window parses as a
/// [`TileBoardHeader`]. Returns `(partition, record, header)` per hit, plus
/// a count of sub-5 sites whose header failed the parse gate.
fn board_headers_in_man(man: &[u8]) -> (Vec<(usize, usize, TileBoardHeader)>, usize) {
    let Ok(mf) = legaia_asset::man_section::parse(man) else {
        return (Vec::new(), 0);
    };
    let mut out = Vec::new();
    let mut unparsed = 0usize;
    for (partition, &count) in mf.header.partition_counts.iter().enumerate() {
        for index in 0..count.max(0) as usize {
            let Some((script_start, pc0, body_len)) =
                partition_record_span(&mf, man, partition, index)
            else {
                continue;
            };
            let body = &man[script_start..script_start + body_len];
            for insn in LinearWalker::new(body, pc0).flatten() {
                let InsnInfo::StateResume { sub_op: 5, .. } = insn.info else {
                    continue;
                };
                // Operand window from the sub-op byte (the retail
                // `_DAT_8007b450` target): opcode header, then sub-op + 12
                // header bytes.
                let hs = if insn.extended.is_some() { 2 } else { 1 };
                match body.get(insn.pc + hs..).and_then(TileBoardHeader::parse) {
                    Some(h) => out.push((partition, index, h)),
                    None => unparsed += 1,
                }
            }
        }
    }
    (out, unparsed)
}

/// Census: which retail scene MANs install a tile board. Pinned result:
/// NONE - every partition record of every scene MAN decodes without a
/// single op-0x49 sub-5 site (a raw byte-pair sweep over the decompressed
/// MANs agrees), so the retail scripts never enter the board mode. This is
/// the disc-side justification for the synthetic demo/install triggers.
#[test]
fn tile_board_scene_census_pins_no_retail_install() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_dir().map(|d| d.join("PROT")) else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut found: BTreeMap<String, Vec<(usize, usize, TileBoardHeader)>> = BTreeMap::new();
    let mut scenes_with_man = 0usize;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .expect("read extracted/PROT")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "BIN"))
        .collect();
    entries.sort();
    for path in entries {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        scenes_with_man += 1;
        let (headers, unparsed) = board_headers_in_man(&man);
        assert_eq!(
            unparsed, 0,
            "{path:?}: op-0x49 sub-5 site with a malformed inline header"
        );
        if headers.is_empty() {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        for (p, r, h) in &headers {
            eprintln!(
                "{name}: p{p} r{r} board {}x{} origin ({},{})",
                h.width, h.height, h.origin_x, h.origin_z
            );
        }
        found.insert(name, headers);
    }
    eprintln!(
        "scanned {scenes_with_man} scene MANs; {} carry tile-board installs",
        found.len()
    );
    assert!(scenes_with_man > 50, "MAN extraction regressed");
    assert!(
        found.is_empty(),
        "a retail scene MAN now decodes an op-0x49 sub-5 tile-board install \
         ({found:?}); the census premise changed - update the demo-trigger \
         docs and wire the scene's natural install"
    );
}

/// The 14-byte op window the field VM hands `op49_menu_request` for a
/// sub-5 install: `[0x49, 0x05, header +1..+0xC]`. `tile_template_base = 3`
/// points the tile templates at the resident global-pool head (the
/// effect-model library seeded at field entry), so every drawable cell
/// value resolves a real mesh.
fn demo_instr(origin_x: u8, origin_z: u8) -> [u8; 14] {
    [0x49, 0x05, origin_x, origin_z, 7, 7, 5, 0, 0, 0, 0, 0, 0, 3]
}

#[test]
fn tile_board_draw_assembly_covers_every_drawable_cell() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");
    let world = &mut session.host.world;
    assert!(
        world.try_install_tile_board(&demo_instr(17, 11)),
        "board install rejected"
    );
    // One field tick rebuilds the per-frame draw list.
    session.tick().expect("field tick");
    let world = &session.host.world;
    let board = world.tile_board.as_ref().expect("board installed");

    // Every drawable cell on the (full-draw, mode 0) board appears in the
    // draw list, and the shell assembly mirrors it 1:1 with the cell-centre
    // world positions.
    let drawable_cells = board
        .cells
        .iter()
        .filter(|&&c| tile_board::is_drawable_cell(c))
        .count();
    assert!(
        drawable_cells > 0,
        "procedural fill produced no drawable cells"
    );
    assert_eq!(world.tile_board_draw_list.len(), drawable_cells);
    let draws = tile_board_draws::tile_board_actor_draws(world);
    assert_eq!(
        draws.len(),
        drawable_cells,
        "assembly drops draw-list cells"
    );
    for (d, td) in world.tile_board_draw_list.iter().zip(&draws) {
        assert_eq!(d.slot, td.slot);
        assert_eq!(d.cell_value, td.cell_value);
        assert_eq!(td.world[0], d.world_x as f32);
        assert_eq!(td.world[2], d.world_z as f32);
        // Tile centres: (origin + idx) * 0x80 + 0x40.
        assert_eq!((d.world_x - 0x40) & 0x7F, 0, "off-centre tile X");
        assert_eq!((d.world_z - 0x40) & 0x7F, 0, "off-centre tile Z");
    }

    // Every present cell value's tile actor spawned with a resolved
    // template mesh (`tmd_ref`) from the global pool - the exact set the
    // play-window redraw queues for GPU upload. The upload queue lists each
    // slot exactly once.
    let need = tile_board_draws::tile_actor_slots_needing_mesh(world);
    let distinct_slots: std::collections::BTreeSet<u8> =
        world.tile_board_draw_list.iter().map(|d| d.slot).collect();
    assert_eq!(
        need.len(),
        distinct_slots.len(),
        "a spawned tile actor is missing its template mesh (pool gap?)"
    );
    for slot in &need {
        let a = &world.actors[*slot as usize];
        assert!(a.active && a.tmd_ref.is_some());
        assert!(
            tile_board_draws::is_tile_actor_slot(world, *slot as usize),
            "tile slot {slot} not board-owned"
        );
    }

    // The player actor (tile table slot 0) is NOT board-owned - the normal
    // field path keeps drawing it.
    let pslot = world.player_actor_slot.expect("player seated") as usize;
    assert!(!tile_board_draws::is_tile_actor_slot(world, pslot));
}

#[test]
fn tile_board_unresolved_templates_degrade_gracefully() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");
    let world = &mut session.host.world;
    // Point the tile templates far past the resident pool: every spawn
    // still allocates a slot (empty tmd_ref), nothing panics, and the
    // upload queue stays empty - the draws exist but the renderer skips
    // them (no mesh ever uploads for an unresolved template).
    let mut instr = demo_instr(17, 11);
    instr[13] = 200;
    assert!(world.try_install_tile_board(&instr));
    session.tick().expect("field tick");
    let world = &session.host.world;
    assert!(!world.tile_board_draw_list.is_empty());
    assert!(
        tile_board_draws::tile_actor_slots_needing_mesh(world).is_empty(),
        "unresolved templates must not enter the upload queue"
    );
    // The per-cell assembly still enumerates the cells (the actors are
    // live); the bin-side drained-slot gate is what keeps them off-screen.
    assert_eq!(
        tile_board_draws::tile_board_actor_draws(world).len(),
        world.tile_board_draw_list.len()
    );
}
