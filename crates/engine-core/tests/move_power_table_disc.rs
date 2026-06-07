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

#[test]
fn move_fx_descriptor_decodes_from_the_real_overlay() {
    let Some(entry) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT/0898 missing");
        return;
    };
    use legaia_engine_core::move_power::MovePowerCatalog;

    let cat = MovePowerCatalog::from_overlay_0898(&entry).expect("move-power table parses");

    // A real boot reaches the auxiliary effect tables + impact-config table that
    // live further into the same overlay than the power table.
    assert!(
        cat.aux_tables().is_some(),
        "the real overlay reaches the 0x801F6324 / 0x801F6418 aux tables"
    );
    assert!(
        cat.impact_table().is_some(),
        "the real overlay reaches the 0x801f53d4 impact-config table"
    );

    // The full behavioural descriptor resolves for Tail Fire (move id 0x27): the
    // trail texpage is always a 0x77xx word, and every resolved on-contact /
    // launch spawn entry that names an aux index resolves its prototype + SFX.
    let fx = cat
        .fx_for_move_id(0x27)
        .expect("Tail Fire (0x27) has a power record");
    assert_eq!(fx.move_id, 0x27);
    assert_eq!(fx.record_index, 0x12);
    assert_eq!(
        fx.trail_texpage & 0xFF00,
        0x7700,
        "trail texpage is a 0x77xx GP0 word"
    );

    // Across the whole named-attack band, every Spawn entry the lists carry
    // resolves through the aux tables (no out-of-table index escapes), and at
    // least one move actually carries an effect list (the band isn't inert).
    use legaia_asset::move_power::{EffectListEntry, IMPACT_EFFECT_TABLE_LEN};
    let mut moves_with_effects = 0usize;
    for move_id in 0x25u8..=0x74 {
        let Some(fx) = cat.fx_for_move_id(move_id) else {
            continue;
        };
        // An in-range impact selector resolves a config word.
        if fx.impact_effect != 0 && (fx.impact_effect as usize) <= IMPACT_EFFECT_TABLE_LEN {
            assert!(
                fx.impact_config.is_some(),
                "move {move_id:#04x} impact selector {} resolves a config word",
                fx.impact_effect
            );
        }
        let mut had = false;
        for eff in fx.contact_effects.iter().chain(fx.launch_effects.iter()) {
            had = true;
            if let EffectListEntry::Spawn(_) = eff.entry {
                assert!(
                    eff.proto.is_some() && eff.sfx.is_some(),
                    "move {move_id:#04x} spawn entry resolves a prototype + SFX"
                );
            }
        }
        if had {
            moves_with_effects += 1;
        }
    }
    assert!(
        moves_with_effects >= 1,
        "at least one named attack carries a resolved effect list ({moves_with_effects})"
    );
}
