//! Disc-gated: the battle-action move-power effect-prototype pointer table
//! (`0x801f6324`, PROT 0898) points at records in the **summon part-record
//! format**, decoded by [`legaia_asset::summon_overlay::parse_records_at`].
//!
//! This validates, on real disc bytes, the finding that the move-FX "effect
//! prototype" entries are not a fixed `~0x20`-byte struct but variable-length
//! move-VM scene-graph records identical to the player-summon parts (same
//! stager `FUN_80021B04`, same move VM). Pins the worked examples from the RE
//! trace (ids `0x21`/`0x25`/`0x27`/`0x28`) byte-for-byte. Skips and passes when
//! `LEGAIA_DISC_BIN` / `extracted/` is absent (the disc-gated convention).

use std::path::PathBuf;

use legaia_asset::move_power::{self, BATTLE_ACTION_OVERLAY_PROT_INDEX, BATTLE_OVERLAY_BASE};
use legaia_asset::summon_overlay::SummonPartKind;
use legaia_prot::archive::Archive;

fn overlay_0898() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if !prot.is_file() {
            continue;
        }
        let mut archive = Archive::open(&prot).ok()?;
        let entry = archive
            .entries
            .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
            .cloned()?;
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).ok()?;
        return Some(bytes);
    }
    None
}

#[test]
fn move_fx_prototype_entries_are_summon_format_records() {
    // The overlay's load base is the pinned 0x801CE818.
    assert_eq!(BATTLE_OVERLAY_BASE, 0x801C_E818);

    let Some(bytes) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing");
        return;
    };

    let aux = move_power::EffectAuxTables::parse(&bytes)
        .expect("aux tables parse from the real PROT 0898 overlay");
    let proto = aux.proto();
    assert_eq!(proto.len(), 61, "61-entry prototype-pointer table");

    // The worked examples from the RE trace: spawn-id -> (proto VA, file off,
    // expected model_sel). The library `proto_record_offset` resolves the VA.
    let examples: [(usize, u32, usize, i16); 4] = [
        (0x21, 0x801F_5B28, 0x27310, 24),
        (0x25, 0x801F_5B64, 0x2734C, 2),
        (0x27, 0x801F_5BBC, 0x273A4, 2),
        (0x28, 0x801F_5BDC, 0x273C4, 2),
    ];
    for (id, va, file_off, _) in examples {
        assert_eq!(proto[id], va, "proto[{id:#04x}] VA");
        assert_eq!(
            aux.proto_record_offset(id as u8),
            Some(file_off),
            "proto[{id:#04x}] file offset"
        );
    }

    // The enemy Gimard "Fire Tail" record, cross-referencing the live-capture
    // finding (`crates/mednafen/tests/firetail_movefx_liveness.rs`): proto
    // entries 0 / 0x30 / 0x31 all point at VA 0x801F5484, the record a live
    // Fire-Tail part-actor carries at +0x48 (model_sel 5).
    for id in [0usize, 0x30, 0x31] {
        assert_eq!(proto[id], 0x801F_5484, "Fire Tail proto[{id}] VA");
    }

    // Decode every prototype record straight through the library helper.
    let parts = move_power::parse_effect_proto_records(&bytes)
        .expect("effect-proto records decode from the real overlay");
    assert!(
        parts.len() >= 8,
        "a healthy share of prototype records decode ({})",
        parts.len()
    );

    // Each record is a summon-format part: a transform node, a small library
    // mesh index, or a node-mode sentinel — never garbage. Library indices stay
    // small (they index the 30-entry effect-model window).
    for p in &parts {
        match p.kind() {
            SummonPartKind::LibraryMesh => assert!(
                (0..0x100).contains(&p.model_sel),
                "library mesh sel in range at {:#x}: {}",
                p.record_off,
                p.model_sel
            ),
            SummonPartKind::TransformNode | SummonPartKind::Sentinel => {}
        }
        // The packed records live in a tight data region (the RE trace placed
        // them in file 0x26C6C..0x27AA8).
        assert!(
            (0x26C6C..0x27C00).contains(&p.record_off),
            "record at {:#x} sits in the prototype data region",
            p.record_off
        );
    }

    // The worked-example records decode to their pinned model_sel (plus the
    // Fire Tail record at 0x26C6C, model_sel 5 — what the live part-actor seats).
    let fire_tail_off = (0x801F_5484u32 - BATTLE_OVERLAY_BASE) as usize;
    let with_fire_tail =
        examples
            .iter()
            .copied()
            .chain([(0usize, 0x801F_5484, fire_tail_off, 5i16)]);
    for (id, _, file_off, model_sel) in with_fire_tail {
        let part = parts
            .iter()
            .find(|p| p.record_off == file_off)
            .unwrap_or_else(|| panic!("record for id {id:#04x} at {file_off:#x} parsed"));
        assert_eq!(part.model_sel, model_sel, "id {id:#04x} record model_sel");
        assert!(
            !part.bytecode.is_empty(),
            "id {id:#04x} carries move-VM bytecode"
        );
    }

    // At least one record is a real library-mesh part (model_sel >= 0) -- the
    // band isn't all transform nodes / sentinels.
    assert!(
        parts
            .iter()
            .any(|p| matches!(p.kind(), SummonPartKind::LibraryMesh)),
        "at least one prototype record selects a library mesh"
    );
}
