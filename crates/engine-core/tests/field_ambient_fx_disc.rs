//! Disc-gated: jou's ambient effect tree (the "pulsating flesh" scene -
//! Rim Elm fused with the Juggernaut) runs end-to-end through the engine's
//! ambient move-VM path.
//!
//! Pins, all against the real disc bytes:
//!  - the MAN partition-1 effect-script scan finds exactly one ambient
//!    install (arg 0 → prescript record 1);
//!  - spawning it fans the op-`0x25` tree out (the lightning director, the
//!    fifteen CLUT-row cyclers whose self-modifying spawn stepping tiles
//!    row 502 in 16-halfword cells, the row-504 lightning palette, the
//!    ambient SFX loop);
//!  - the mode-3 CLUT-cell integrator emits palette writes, and raising the
//!    lightning flag (system flag `0x364`, the director's signal) makes the
//!    cyclers' brightness adds jump - the on-screen lightning pulse;
//!  - `step_ambient_fx` against a software VRAM actually rewrites texels.
//!
//! Skip-pass when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::ambient_effect_installs;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

#[test]
fn jou_ambient_tree_spawns_and_cycles_clut_cells_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let scene = Scene::load(&index, "jou").expect("load jou");

    // The prescript stager bundle + the MAN.
    let scripts = scene.find_event_scripts().expect("jou event scripts");
    let stager_bytes = scripts.bytes.to_vec();
    let man_bytes = scene
        .field_man_payload(&index)
        .expect("man payload")
        .expect("jou has a MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("parse MAN");

    // One ambient install: the P1 effect script's `34 30 00`.
    let installs = ambient_effect_installs(&man_file, &man_bytes);
    assert_eq!(installs, vec![0], "jou ambient install census");

    let mut world = World::default();
    world.frame_step = 2; // town cadence
    world.install_field_stagers(&stager_bytes);
    assert!(
        world.field_stagers.len() >= 47,
        "jou prescript records ({})",
        world.field_stagers.len()
    );

    // Spawn the ambient tree (arg 0 → record 1, the FUN_800252EC id law).
    assert!(world.spawn_ambient_record(installs[0] as usize + 1, [0, 0, 0]));
    // Installer + director + 15 cyclers + lightning palette + rec23 beam +
    // the SFX loop = 20 parts.
    assert_eq!(world.ambient_fx.len(), 20, "jou ambient fan-out");

    // The fifteen row-502 cyclers tile the CLUT row: the self-modifying
    // ext-0x1E spawn stepping gives each instance its own 16-halfword cell.
    // (Engine snapshot semantics land one 0x10 step behind retail - cells
    // 0x00..0xE0 instead of 0x10..0xF0; see `world/ambient.rs`.)
    let mut ticks = 0;
    while ticks < 64 {
        world.tick_ambient_fx();
        ticks += 1;
        if world.active_ambient_cell_fx().len() >= 17 {
            break;
        }
    }
    let fx = world.active_ambient_cell_fx();
    let row_502: Vec<u16> = {
        let mut xs: Vec<u16> = fx
            .iter()
            .filter(|f| f.rect.1 == 0x1F6)
            .map(|f| f.rect.0)
            .collect();
        xs.sort_unstable();
        xs.dedup();
        xs
    };
    assert!(
        row_502.len() >= 15,
        "row-502 cycler cells: {row_502:x?} (fx count {})",
        fx.len()
    );
    let stride_ok = row_502.windows(2).all(|w| w[1] - w[0] == 0x10);
    assert!(stride_ok, "cells tile at 16-halfword stride: {row_502:x?}");
    assert!(
        fx.iter().any(|f| f.rect.1 == 0x1F8),
        "row-504 lightning palette cell present"
    );

    // Lightning: the director's signal is system flag 0x364. Raising it
    // makes the cyclers jump their S/V adds (the palette flash).
    let idle_v: i32 = fx.iter().map(|f| i32::from(f.v_add).abs()).sum();
    world.system_flag_set(0x364);
    for _ in 0..4 {
        world.tick_ambient_fx();
    }
    let flash = world.active_ambient_cell_fx();
    let flash_v: i32 = flash.iter().map(|f| i32::from(f.v_add).abs()).sum();
    assert!(
        flash_v != idle_v,
        "lightning flag moves the brightness adds (idle {idle_v}, flash {flash_v})"
    );

    // The VRAM step writes texels: seed the captured rows with a non-zero
    // ramp so the HSV rewrite has something to act on.
    let mut vram = legaia_tim::Vram::new();
    for (x, y) in [(0u16, 0x1F6u16), (0x70, 0x1F8)] {
        let row: Vec<u8> = (0..256u16)
            .flat_map(|i| (0x0421u16.wrapping_mul(i % 31 + 1)).to_le_bytes())
            .collect();
        vram.write_block(x, y, 256 - x, 1, &row[..usize::from(256 - x) * 2]);
    }
    world.ambient_pending_game_ticks = 2;
    let wrote = world.step_ambient_fx(&mut vram);
    assert!(wrote, "ambient CLUT-cell step rewrites VRAM texels");
}
