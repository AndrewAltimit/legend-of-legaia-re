//! Disc-gated: field-VM op `0x3E` with `op0 < 100` is the **scripted-battle
//! install**, on the real shipped sites whose `op0` is not `0xFF`.
//!
//! Retail's case-`0x3E` arm (`FUN_801DE840`, PROT 0897) tests `op0` twice and
//! nowhere else: `beq v1,0xFF` at `0x801E06FC` and `sltiu v0,v1,0x64` in its
//! delay slot, `beqz` at `0x801E0704` sending `op0 >= 100` to the minigame
//! door-warp at `0x801E078C`. Both `0xFF` and every `op0 < 100` fall into the
//! same body - `FUN_801D9E1C(player, 0)`, then (system entity present, dev
//! word `_DAT_8007B868` clear) `sys[+0x8A] = 1`,
//! `sys[+0x94] = *(ctrl+0x20) + op1 * ctrl[+0x5D] + 1`, the step-counter
//! reroll and `FUN_8003CE08(0xE)`. `op0` is never read again, so `3E 00 02`
//! installs formation row 2 exactly as `3E FF 02` would.
//!
//! The disc carries ten clean non-`0xFF` sites (`asset field-op-census --only
//! 3E`): `town0b` `3E 00 02` x5, `stone` `3E 00 03` x4, `jagaroom` `3E 01 00`
//! x1. This steps one site per scene off the scene's own MAN bytes through the
//! production field VM and asserts the fight that retail starts: no dialogue
//! box, and the field-to-battle intro landing in `SceneMode::Battle` against
//! the MAN formation row the op names.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// One census site: `(scene, partition, record index, offset in the record,
/// op0, formation row)`.
const SITES: &[(&str, usize, usize, usize, u8, u8)] = &[
    ("town0b", 1, 6, 0x006D, 0x00, 0x02),
    ("stone", 0, 2, 0x0856, 0x00, 0x03),
    ("jagaroom", 2, 9, 0x0E96, 0x01, 0x00),
];

#[test]
fn low_op0_sites_install_their_formation_row_and_enter_battle() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    for &(scene, partition, index, off, op0, row) in SITES {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.world.toggles.live_gameplay_loop = true;
        host.enter_field_scene(scene, 0)
            .unwrap_or_else(|e| panic!("enter {scene}: {e:#}"));
        let man = host
            .scene
            .as_ref()
            .unwrap()
            .field_man_payload(&host.index)
            .expect("MAN payload read")
            .unwrap_or_else(|| panic!("{scene} resolves its bundle MAN"));
        let man_file = legaia_asset::man_section::parse(&man).expect("MAN parses");
        let (start, _pc0, len) = legaia_engine_core::man_field_scripts::partition_record_span(
            &man_file, &man, partition, index,
        )
        .unwrap_or_else(|| panic!("{scene} P{partition}[{index}] span resolves"));
        let body = &man[start..start + len];
        let op = &body[off..off + 3];
        assert_eq!(
            op,
            &[0x3E, op0, row],
            "{scene} P{partition}[{index}] @ {off:#06x} carries the census op"
        );

        // The row is a real MAN formation with monsters, registered at entry.
        let record =
            legaia_engine_core::encounter_man::formation_record_for_row(&man, usize::from(row))
                .unwrap_or_else(|| panic!("{scene} formation row {row} decodes"));
        assert!(record.count > 0, "{scene} row {row} carries monsters");
        let first_monster = u16::from(record.monster_ids[0]);
        assert!(
            host.world
                .tables
                .formation_table
                .formation(u16::from(row))
                .is_some_and(|d| !d.slots.is_empty()),
            "{scene} registers row {row} at scene entry"
        );

        // Step the disc's own three op bytes through the production field VM.
        host.world.dialog.current = None;
        host.world.load_field_script(op.to_vec());
        host.world.input.set_pad(0);
        let mut entered = false;
        for _ in 0..400 {
            host.tick().expect("tick");
            assert!(
                host.world.dialog.current.is_none(),
                "{scene}: `3E {op0:02X} {row:02X}` must open no dialogue"
            );
            if host.world.mode == SceneMode::Battle {
                entered = true;
                break;
            }
        }
        assert!(
            entered,
            "{scene}: `3E {op0:02X} {row:02X}` enters the battle (mode {:?})",
            host.world.mode
        );
        let monster_slot = host.world.party.party_count.clamp(1, 3) as usize;
        assert_eq!(
            host.world.actors[monster_slot].battle_monster_id,
            Some(first_monster),
            "{scene}: the first enemy is formation row {row}'s first monster"
        );
        eprintln!(
            "[ok] {scene} P{partition}[{index}] `3E {op0:02X} {row:02X}` -> row {row} \
             (count {}, first monster {first_monster:#04x}) -> Battle",
            record.count
        );
    }
}
