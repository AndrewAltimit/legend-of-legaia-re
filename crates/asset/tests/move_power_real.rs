//! Disc-gated: the battle-action per-move power table (`0x801F4F5C`) parses out
//! of the real PROT 0898 (battle-action overlay) entry at the pinned offset.
//!
//! Pins, on real disc bytes, that the table `FUN_801dd0ac` reads for the
//! arts/physical attacker roll lives in the battle-action overlay image and
//! decodes to the power values observed in-RAM (byte-matched against two battle
//! save states). Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is
//! absent (the workspace disc-gated convention).

use std::path::PathBuf;

use legaia_asset::move_power::{self, BATTLE_ACTION_OVERLAY_PROT_INDEX};
use legaia_prot::archive::Archive;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

#[test]
fn move_power_table_parses_with_pinned_powers() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");

    let table = move_power::parse(&bytes).expect("move-power table parses at the pinned offset");
    assert_eq!(table.len(), move_power::MOVE_POWER_TABLE_LEN);

    // Move id 0 is the unused all-zero slot.
    assert!(table[0].is_empty(), "move id 0 should be the unused slot");

    // Pinned powers (`+0` field >> 2), byte-matched against the in-RAM table in
    // two battle save states. The three "special" lead records carry large
    // values (187 / 625 / 1500); the mid-table arts records are small.
    let expect: &[(usize, i32)] = &[(1, 187), (2, 625), (3, 1500), (9, 250), (16, 15), (43, 249)];
    for &(idx, pow) in expect {
        assert_eq!(table[idx].power(), pow, "power for move id {idx}");
        assert!(!table[idx].is_empty(), "move id {idx} should be populated");
    }

    // Every populated record's raw power is consistent with its decoded power.
    for r in &table {
        assert_eq!(r.power(), (r.power_raw as i32) >> 2);
    }

    // Residual record fields, pinned against the real PROT 0898 bytes (each
    // field code-traced to a battle-action SM reader; see move_power.rs).
    // Record 3 (move id 0x29): a homing physical strike with two effect lists.
    let r3 = &table[3];
    assert_eq!(r3.strike_y_offset(), 250); // +0x02
    assert_eq!(r3.phase_duration(), 480); // +0x06
    assert_eq!(r3.homing_speed(), 0x20); // +0x08
    assert!(r3.effect_tracks_strike()); // +0x09
    assert_eq!(r3.impact_effect(), 1); // +0x0a
    assert_eq!(r3.trail_texture_page(), 0); // +0x0b
    assert_eq!(r3.annotation_tag(), Some('C')); // +0x0c designer tag (unread at runtime)
    assert_eq!(r3.sound_cue_id(), 0x4d); // +0x0d
    assert_eq!(r3.contact_effects(), vec![0x27, 0x8e, 0x8d]); // +0x12 (0x00-terminated)
    assert_eq!(r3.launch_effects(), vec![0x28, 0x64, 0x9d]); // +0x16
    // Record 12 (move id 0x2f): a multi-target move -- list_mode 0xFF and the
    // effect lists are the 0xFF skip sentinel (no spawns).
    let r12 = &table[12];
    assert_eq!(r12.list_mode(), 0xff); // +0x0e all-arms broadcast
    assert_eq!(r12.annotation_tag(), Some('G')); // +0x0c
    assert!(r12.launch_effects().is_empty()); // +0x16 = ff ff ff ff
    // The 'C'/'E'/'G' designer tag only appears on the unnamed internal-tier
    // records (ids 1..15); the named monster-attack records (16+) carry 0.
    assert_eq!(table[16].annotation_tag(), None);
    for r in table.iter().skip(16) {
        assert_eq!(
            r.annotation_tag(),
            None,
            "named-attack record {} should carry no designer tag",
            r.index
        );
    }

    // The id -> power-index map (0x80 bytes before the table) resolves battle
    // move ids to records (`power_table[map[move_id]]`). Pinned move ids ->
    // index, byte-matched against the in-RAM map across two battle save states.
    let map = move_power::parse_id_index_map(&bytes).expect("id->index map parses");
    let expect_map: &[(u8, u8)] = &[
        (0x04, 1),
        (0x05, 2),
        (0x06, 3),
        (0x07, 4),
        (0x19, 9),
        (0x25, 16),
        (0x46, 16), // a second id sharing record 16
        (0x74, 40),
    ];
    for &(move_id, idx) in expect_map {
        assert_eq!(
            move_power::index_for_move_id(&map, move_id),
            Some(idx),
            "move id {move_id:#04x} -> power index"
        );
    }
    // End-to-end: move id 0x04 -> record 1 -> power 187; 0x06 -> record 3 -> 1500.
    assert_eq!(
        move_power::record_for_move_id(&table, &map, 0x04).map(|r| r.power()),
        Some(187)
    );
    assert_eq!(
        move_power::record_for_move_id(&table, &map, 0x06).map(|r| r.power()),
        Some(1500)
    );
    // Unmapped ids resolve to no record.
    assert_eq!(move_power::index_for_move_id(&map, 0x00), None);
    assert_eq!(move_power::index_for_move_id(&map, 0x10), None);

    // Cross-reference: the move id is the same id space as the SCUS spell-name
    // table (both indexed by actor[+0x1df]). So every mapped move id >= 0x25 is a
    // named monster attack and every mapped id < 0x24 is an unnamed internal
    // enemy-attack tier. Pin that structural split (no Sony name strings embedded
    // here -- only the named/unnamed boundary). Skips if the executable is
    // absent.
    let Some(scus) = read_scus() else {
        eprintln!("[skip] SCUS_942.54 absent - skipping spell-name cross-reference");
        return;
    };
    let spells = legaia_asset::spell_names::SpellNameTable::from_scus(&scus)
        .expect("parse SCUS spell-name table");
    let mut named_hi = 0usize;
    let mut unnamed_lo = 0usize;
    for move_id in 0u8..=0x7f {
        if move_power::index_for_move_id(&map, move_id).is_none() {
            continue;
        }
        let named = spells.name(move_id).is_some_and(|n| !n.is_empty());
        if move_id >= 0x25 {
            assert!(
                named,
                "mapped move id {move_id:#04x} (>=0x25) should be a named monster attack"
            );
            named_hi += 1;
        } else {
            assert!(
                !named,
                "mapped move id {move_id:#04x} (<0x24) should be an unnamed internal tier"
            );
            unnamed_lo += 1;
        }
    }
    // Sanity: both groups are non-trivial (the named monster attacks dominate).
    assert!(
        named_hi >= 25,
        "named monster-attack records (got {named_hi})"
    );
    assert!(
        unnamed_lo >= 10,
        "unnamed internal-tier records (got {unnamed_lo})"
    );
}

/// The move-power table is **special-attack-only**: its id -> index map covers
/// the internal enemy-attack tiers + named monster attacks, but the basic-attack
/// / Tactical-Art move-id range `0x08..=0x18` is entirely unmapped. Pinned from a
/// live battle capture (Vahn's queued Somersault art carries move id `0x0F`, a
/// lone Gobu Gobu's basic attack `0x09`) cross-checked against the disc map: both
/// resolve to no record, so neither a party member's art nor an enemy basic
/// attack draws its damage from `FUN_801dd0ac` / this table - that path is
/// reserved for the special attacks, and a party member's art takes its power
/// from the art-record power byte instead (see `docs/formats/art-data.md`).
#[test]
fn move_power_map_is_special_attack_only() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");
    let map = move_power::parse_id_index_map(&bytes).expect("id->index map parses");

    // The captured live move ids (Vahn's Somersault art 0x0F, enemy basic 0x09)
    // are unmapped - so the move-power damage kernel is not their damage source.
    assert_eq!(
        move_power::index_for_move_id(&map, 0x0F),
        None,
        "Somersault 0x0F"
    );
    assert_eq!(
        move_power::index_for_move_id(&map, 0x09),
        None,
        "enemy basic 0x09"
    );
    // The basic-attack / art move-id bands are unmapped (the mapped `0x12..=0x15`
    // interleaved here are the internal enemy-attack tiers, not basic/art ids).
    for mid in (0x08u8..=0x11).chain(0x16u8..=0x18) {
        assert_eq!(
            move_power::index_for_move_id(&map, mid),
            None,
            "basic/art id {mid:#04x} should not map into the move-power table"
        );
    }
    // But the special-attack ids the table IS for resolve (internal tiers +
    // named monster attacks) - a representative sample.
    for mid in [0x04u8, 0x07, 0x12, 0x19, 0x25, 0x27, 0x74] {
        assert!(
            move_power::index_for_move_id(&map, mid).is_some(),
            "special-attack id {mid:#04x} should map into the move-power table"
        );
    }
}

/// The auxiliary effect tables (`0x801F6324` prototypes + `0x801F6418` SFX) the
/// records' `+0x12` / `+0x16` effect-id lists index parse out of the same real
/// PROT 0898 entry at their pinned offsets, and the spawn indices that appear in
/// the real records resolve to in-range entries.
#[test]
fn effect_aux_tables_parse_from_disc() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");

    let table = move_power::parse(&bytes).expect("move-power table parses");
    let aux = move_power::EffectAuxTables::parse(&bytes).expect("effect aux tables parse");
    assert_eq!(aux.proto().len(), move_power::EFFECT_AUX_TABLE_LEN);
    assert_eq!(aux.sfx().len(), move_power::EFFECT_AUX_TABLE_LEN);

    // Record 3 (move id 0x29) carries a contact list [0x27, 0x8e, 0x8d] and a
    // launch list [0x28, 0x64, 0x9d]. Classify each byte exactly as the runtime
    // dispatch does, and pin the table lookups for the two real Spawn entries.
    use move_power::EffectListEntry::*;
    assert_eq!(move_power::EffectListEntry::classify(0x27), Spawn(0x27));
    assert_eq!(move_power::EffectListEntry::classify(0x28), Spawn(0x28));
    assert_eq!(move_power::EffectListEntry::classify(0x64), FixedFlash);
    assert_eq!(move_power::EffectListEntry::classify(0x8e), AltEffect(0x0e));
    assert_eq!(move_power::EffectListEntry::classify(0x8d), AltEffect(0x0d));
    assert_eq!(move_power::EffectListEntry::classify(0x9d), AltEffect(0x1d));

    // The two Spawn indices resolve to concrete table values (byte-matched
    // against the real PROT 0898 bytes). The prototype entries are overlay VAs:
    // `0x801F6324`'s `u32`s point at ~0x20-byte effect-prototype structs in the
    // same overlay (here at `0x801F5BBC` / `0x801F5BDC`, exactly 0x20 apart), and
    // both effects share SFX cue `0xD0`.
    assert_eq!(aux.effect_proto(0x27), Some(0x801F_5BBC));
    assert_eq!(aux.effect_proto(0x28), Some(0x801F_5BDC));
    assert_eq!(aux.effect_sfx(0x27), Some(0xD0));
    assert_eq!(aux.effect_sfx(0x28), Some(0xD0));
    // The prototype pointers all land in the battle-action overlay's VA window.
    for (i, &p) in aux.proto().iter().enumerate() {
        if p != 0 {
            assert!(
                (0x801C_0000..0x8020_0000).contains(&p),
                "proto[{i}] = {p:#010x} is not an overlay VA"
            );
        }
    }

    // Every Spawn entry across every record's two effect lists resolves to an
    // in-range table index (the records never reference a spawn index past the
    // 61-entry tables).
    let mut spawn_seen = 0usize;
    for r in &table {
        for &e in r.contact_effects().iter().chain(r.launch_effects().iter()) {
            if let Spawn(idx) = move_power::EffectListEntry::classify(e) {
                assert!(
                    aux.effect_proto(idx).is_some() && aux.effect_sfx(idx).is_some(),
                    "record {} spawn index {idx:#04x} out of the aux-table range",
                    r.index
                );
                spawn_seen += 1;
            }
        }
    }
    assert!(spawn_seen >= 1, "no Spawn entries across the records");
}

/// The impact-effect pointer table (`0x801F53D4`, the record's `+0x0a` selector)
/// parses out of the real PROT 0898 entry: 5 overlay-VA pointers immediately
/// before the element-affinity matrix.
#[test]
fn impact_effect_table_parses_from_disc() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");

    let table = move_power::parse_impact_effect_table(&bytes).expect("impact-effect table parses");
    // Five packed u32 config words written to the strike actor's +0x04 (these are
    // NOT pointers — they carry the `0x3FF`-masked packed lanes of the impact
    // config). Byte-matched against the real PROT 0898 bytes.
    assert_eq!(
        table,
        [
            0x0000_03FF,
            0x3FF4_0100,
            0x3FF1_0040,
            0x3FF0_4040,
            0x3FFF_FFFF
        ]
    );
    // The table sits in the 0x14-byte gap immediately before the element-affinity
    // matrix (the next pinned datum), confirming its 5-entry extent.
    assert_eq!(
        move_power::IMPACT_EFFECT_TABLE_FILE_OFFSET + move_power::IMPACT_EFFECT_TABLE_LEN * 4,
        legaia_asset::element_affinity::AFFINITY_MATRIX_FILE_OFFSET,
    );
}

/// Read `SCUS_942.54` from `extracted/` if present.
fn read_scus() -> Option<Vec<u8>> {
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base).join("SCUS_942.54");
        if let Ok(b) = std::fs::read(&p) {
            return Some(b);
        }
    }
    None
}
