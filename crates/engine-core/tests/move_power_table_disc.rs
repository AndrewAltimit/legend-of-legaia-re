//! Disc-gated: the engine-side move-power catalog loads from the real
//! battle-action overlay (PROT 0898) and resolves battle move ids to their
//! per-move power scalar through the `0x801F4E63` id → index map.
//!
//! The asset crate has its own byte-level tests for the parser; this one pins
//! the *engine wrapper* ([`legaia_engine_core::move_power::MovePowerCatalog`])
//! end to end against the entry bytes a real boot streams, and joins the
//! move-id space to a known named monster special-attack (Tail Fire, move id
//! `0x27`, the enemy-Gimard move — power record index `0x12`). Skips without
//! `LEGAIA_DISC_BIN` (CLAUDE.md convention).
use std::path::PathBuf;

fn overlay_0898() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for d in ["extracted/PROT", "../../extracted/PROT"] {
        let p = PathBuf::from(d).join("0898_xxx_dat.BIN");
        if let Ok(b) = std::fs::read(&p) {
            return Some(b);
        }
    }
    None
}

#[test]
fn move_power_catalog_resolves_named_monster_attacks() {
    let Some(entry) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT/0898 missing");
        return;
    };
    use legaia_engine_core::move_power::MovePowerCatalog;

    let cat = MovePowerCatalog::from_overlay_0898(&entry)
        .expect("move-power table parses from the real PROT 0898 overlay");
    assert!(cat.len() >= 44, "full table parsed ({} records)", cat.len());

    // Tail Fire (move id 0x27) — a named monster special-attack — resolves to
    // record index 0x12 with a real, positive power (the arts-roll modulus
    // base). This is the move whose power the summon-reconciliation thread
    // pinned (enemy Tail Fire = move 0x27).
    let rec = cat
        .record_for_move_id(0x27)
        .expect("move id 0x27 (Tail Fire) has a power record");
    assert_eq!(rec.index, 0x12, "Tail Fire maps to power record index 0x12");
    let power = cat.power_for_move_id(0x27).expect("power resolves");
    assert_eq!(power, rec.power());
    assert!(power > 0, "Tail Fire carries a positive power ({power})");

    // The whole named-attack band (move ids 0x25..=0x74 -> record indices
    // 0x10..=0x2b) resolves through the map to a populated record.
    let mut named = 0usize;
    for move_id in 0x25u8..=0x74 {
        if let Some(r) = cat.record_for_move_id(move_id) {
            assert!(!r.is_empty(), "move {move_id:#04x} -> a populated record");
            named += 1;
        }
    }
    assert!(
        named >= 8,
        "several named monster attacks resolve to power records ({named})"
    );

    // An id outside the mapped move space resolves to nothing.
    assert_eq!(cat.power_for_move_id(0x00), None);
}
