//! Decode the real spell-name table out of `extracted/SCUS_942.54` and check it
//! against the monster archive's magic-attack ids. Skips and passes when the
//! executable / archive aren't on disk (the disc-gated skip pattern).

use legaia_asset::monster_archive::records;
use legaia_asset::spell_names::SpellNameTable;
use std::path::PathBuf;

fn workspace() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(PathBuf::from)
}

#[test]
fn names_enemy_magic_or_skips() {
    let Some(ws) = workspace() else { return };
    let scus_path = ws.join("extracted").join("SCUS_942.54");
    let archive_path = ws
        .join("extracted")
        .join("PROT")
        .join("0867_battle_data.BIN");
    if !scus_path.is_file() || !archive_path.is_file() {
        eprintln!("extracted SCUS / archive not present - skipping");
        return;
    }

    let scus = std::fs::read(&scus_path).expect("read SCUS");
    let table = SpellNameTable::from_scus(&scus).expect("parse spell table");
    assert_eq!(table.len(), 256);

    // Pinned named monster attacks (the band starting at 0x25).
    assert_eq!(table.name(0x25), Some("Fire Breath"));
    assert_eq!(table.name(0x27), Some("Tail Fire"));
    assert_eq!(table.mp(0x27), Some(16));

    // Gimard (archive id 10) is the save-state ground truth: its magic-attack
    // array carries the global id 0x27, which the table names "Tail Fire".
    let archive = std::fs::read(&archive_path).expect("read archive");
    let recs = records(&archive).expect("decode archive");
    let gimard = recs.iter().find(|r| r.id == 10).expect("gimard id 10");
    assert!(
        gimard.magic_attacks.contains(&0x27),
        "Gimard magic_attacks = {:02x?}",
        gimard.magic_attacks
    );
    assert_eq!(
        gimard
            .magic_attacks
            .iter()
            .find(|&&id| id == 0x27)
            .and_then(|&id| table.name(id)),
        Some("Tail Fire")
    );

    // Every monster's magic-attack ids resolve to a real name (no raw-id
    // fallbacks across the corpus), proving the +0x21 array holds global ids.
    let mut named = 0usize;
    for r in &recs {
        for &id in &r.magic_attacks {
            assert!(
                table.name(id).is_some(),
                "monster {:?} magic id 0x{id:02x} has no name",
                r.name
            );
            named += 1;
        }
    }
    assert!(
        named > 80,
        "expected many named enemy magic ids, got {named}"
    );

    // Info-window descriptions: the stats `+4` byte indexes the 0x80075DB0
    // pointer table (FUN_801D2E74). Every player Seru-magic spell
    // (0x81..=0x95) carries one; the retail shape is "<title>|<effect
    // line>" - two '\n'-separated lines after decode. Structural checks
    // only (the strings are Sony text and stay uncommitted).
    for id in 0x81..=0x95u8 {
        let desc = table
            .desc(id)
            .unwrap_or_else(|| panic!("seru spell 0x{id:02x} has no description"));
        let lines: Vec<&str> = desc.split('\n').collect();
        assert!(
            lines.len() == 2 && lines.iter().all(|l| !l.is_empty()),
            "0x{id:02x} desc shape: {desc:?}"
        );
    }
    // Internal enemy-attack tiers (0x00..=0x24) carry no description index.
    assert_eq!(table.desc(0x01), None);
}

/// The capture-class module map: every spell record with class byte `'c'`
/// (`stats +0`) names its cast module by the `+1` sub-id, and the module
/// lives at extraction PROT `935 + sub_id`. Pins the statically derived
/// per-entry identity of the enemy/capture module band `0935..=0966` against
/// the capture-verified legs (`docs/formats/spell-table.md`,
/// `docs/subsystems/battle-action.md`).
#[test]
fn capture_class_module_map_or_skips() {
    let Some(ws) = workspace() else { return };
    let scus_path = ws.join("extracted").join("SCUS_942.54");
    if !scus_path.is_file() {
        eprintln!("extracted SCUS not present - skipping");
        return;
    }
    let scus = std::fs::read(&scus_path).expect("read SCUS");
    let table = SpellNameTable::from_scus(&scus).expect("parse spell table");
    let recs = legaia_asset::spell_names::capture_class_records(&scus).expect("capture records");

    // Capture-pinned / playtest-pinned legs of the band (spell id -> entry).
    let pins: &[(u8, u32, &str)] = &[
        (0x37, 944, "Guilty Cross"),
        (0x55, 946, "Call Wave"),
        (0x5C, 952, "Bloody Horns"),
        (0x5D, 953, "Terio Punch"),
        (0x5E, 953, "Bull Charge"),
        (0x79, 958, "Blazing Slash"),
        (0x7A, 959, "Megaton Press"),
        (0x7B, 960, "Plasma Strike"),
        (0xA5, 962, "Ultra Charge"),
        (0xAC, 940, "Mystic Shield"),
        (0xAD, 966, "Evil Seru Magic"),
        (0xB4, 961, "Final Crisis"),
        (0xB6, 965, "Doomsday"),
        (0xB7, 938, "Mystic Circle"),
        (0x76, 957, "Death Game"),
    ];
    for &(id, entry, name) in pins {
        let k = recs
            .iter()
            .find(|&&(sid, _)| sid == id)
            .map(|&(_, k)| k)
            .unwrap_or_else(|| panic!("spell 0x{id:02x} {name} is not capture-class"));
        assert_eq!(
            legaia_asset::spell_names::capture_module_prot(k),
            entry,
            "spell 0x{id:02x} {name} module entry"
        );
        assert_eq!(table.name(id), Some(name), "spell 0x{id:02x} name");
    }

    // Coverage: the sub-id space maps onto the whole band 0935..=0966 with no
    // gap - every entry in the band is somebody's cast module.
    let entries: std::collections::BTreeSet<u32> = recs
        .iter()
        .map(|&(_, k)| legaia_asset::spell_names::capture_module_prot(k))
        .collect();
    assert_eq!(
        entries.iter().copied().collect::<Vec<_>>(),
        (935..=966).collect::<Vec<_>>(),
        "capture-class sub-ids cover exactly extraction 0935..=0966"
    );
}
