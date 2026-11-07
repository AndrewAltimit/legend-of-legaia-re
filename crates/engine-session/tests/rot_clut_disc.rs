//! Disc-gated: the status CLUT pass's **Rot arm** (`FUN_8004CE2C`,
//! `0x8004D7A8..0x8004D98C`) through the chain both play hosts tick.
//!
//! `World::install_menu_text` reads the per-character palette windows at
//! `0x80078630` out of the real `SCUS_942.54`, `BattleHud::sync_status` folds
//! the seat's rotted limb into its latch, and `step_battle_vram_edits` - the
//! battle VRAM step of the native window and the browser play page - seats
//! the windows off the party and tints that limb's span of CLUT row
//! `481 + slot` blue, leaving the rest of the row alone.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use legaia_engine_battle::battle_hud_state::BattleHud;
use legaia_engine_battle::battle_status_clut::{
    PARTY_CLUT_ENTRIES, PARTY_CLUT_ROW_BASE, ROT_CLUT_LIMB_BITS, RotClutWindows,
};
use legaia_engine_battle_vm::bgr555_grey::bgr555_to_rot_blue;
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_engine_scene::vfs::Vfs;
use legaia_engine_session::battle_vram::step_battle_vram_edits;
use std::path::PathBuf;

fn scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        return None;
    }
    legaia_engine_scene::vfs::DiscVfs::open(&path)
        .ok()?
        .read("SCUS_942.54")
        .ok()
}

#[test]
fn a_rotted_limb_tints_its_palette_window_through_the_host_step() {
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let table = RotClutWindows::from_scus(&scus).expect("window table decodes");
    // Every hero row is three non-empty spans inside the 240-entry row, on
    // 16-entry CLUT boundaries.
    for id in 1..=3u8 {
        let row = table.row(id).unwrap();
        for limb in 0..3 {
            let (lo, hi) = (row[limb * 2], row[limb * 2 + 1]);
            assert!(lo < hi, "id {id} limb {limb}: [{lo:#x}, {hi:#x})");
            assert!(usize::from(hi) <= PARTY_CLUT_ENTRIES);
            assert_eq!((lo % 16, hi % 16), (0, 0), "id {id} limb {limb}");
        }
    }

    let mut world = World::new();
    world.install_menu_text(&scus);
    world.enter_battle(3, 1);
    let windows = world.battle_rot_clut_windows();
    assert_eq!(windows[0], table.row(1), "seat 0 holds roster id 1");

    // Seed a colourful palette on the seat's row (the battle loader's write).
    let mut vram = psx_tim::Vram::new();
    let row = PARTY_CLUT_ROW_BASE;
    let words: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
        .map(|i| 0x8000 | ((i % 31 + 1) as u16) | ((((i / 3) % 31 + 1) as u16) << 10))
        .collect();
    let bytes: Vec<u8> = words.iter().flat_map(|c| c.to_le_bytes()).collect();
    vram.write_clut_row(0, row, &bytes);

    let mut hud = BattleHud::new();
    hud.sync_status(0, &world.battle.status_effects);
    assert!(!step_battle_vram_edits(
        &mut world,
        &mut hud.status_clut,
        Some(&mut vram)
    ));

    // Rot on limb 1 - the applier's state.
    world.battle.status_effects.apply(0, StatusKind::Rot);
    world.battle.status_effects.set_rot_limb(0, 1);
    assert_eq!(
        world.battle.status_effects.display_flags(0) & 0x38,
        ROT_CLUT_LIMB_BITS[1]
    );
    hud.sync_status(0, &world.battle.status_effects);
    assert!(step_battle_vram_edits(
        &mut world,
        &mut hud.status_clut,
        Some(&mut vram)
    ));

    let w = windows[0].unwrap();
    let (lo, hi) = (usize::from(w[2]), usize::from(w[3]));
    for (x, &orig) in words.iter().enumerate() {
        let got = vram.pixel(x, row as usize);
        if (lo..hi).contains(&x) {
            assert_eq!(got, bgr555_to_rot_blue(orig), "entry {x:#x} in the window");
        } else {
            assert_eq!(got, orig, "entry {x:#x} outside the window");
        }
    }
    // Once per affliction.
    hud.sync_status(0, &world.battle.status_effects);
    assert!(!step_battle_vram_edits(
        &mut world,
        &mut hud.status_clut,
        Some(&mut vram)
    ));
}
