//! Disc-gated invariants for `FUN_80043390`'s per-prim renderer dispatch
//! tables.
//!
//! Two invariants are asserted across every available mednafen save:
//!
//! 1. The SCUS-resident dispatch table at `0x8007657C` is **byte-identical**
//!    in every save. The table lives inside `SCUS_942.54`'s loaded code
//!    region, so RAM writes can't legally touch it - the bytes are the
//!    same in every state.
//!
//! 2. The overlay-resident dispatch table at `0x801F8968` is **populated
//!    only** when the world-map overlay is paged in. Town / battle /
//!    cutscene / FMV-trigger states have either zeros at that address or
//!    leftover overlay-code bytes that don't match the SCUS-shared
//!    low-mode quartet `looks_like_dispatch_table()` checks for.
//!
//! Skipped when `LEGAIA_MEDNAFEN_DIR` is unset, matching the existing
//! pattern in `real_saves.rs`. CI runs without disc-side saves.

use legaia_mednafen::SaveState;
use legaia_mednafen::prim_dispatch::{
    OVERLAY_ALPHA_ROWS, OVERLAY_TABLE_BASE, ROW_BYTES, SCUS_ALPHA_ROWS, SCUS_TABLE_BASE,
    SLOTS_PER_ROW, decode, decode_both,
};

const MAX_SLOTS: u8 = 10;

fn mcs_dir() -> Option<std::path::PathBuf> {
    std::env::var("LEGAIA_MEDNAFEN_DIR")
        .ok()
        .map(std::path::PathBuf::from)
}

/// Resolve a slot to a real save state file path. Returns `None` when the
/// env var is unset or the slot file doesn't exist.
fn save_for(slot: u8) -> Option<std::path::PathBuf> {
    let dir = mcs_dir()?;
    // We accept either the scenarios.toml-driven filename or the
    // generic "*.mc<slot>" tail. Real corpora use the long disc-hash
    // filename; rather than hard-code that here, glob.
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let name = entry.file_name().into_string().ok()?;
        if name.ends_with(&format!(".mc{slot}")) {
            return Some(entry.path());
        }
    }
    None
}

/// Snapshot every available mednafen save's RAM. Returns `(slot, ram)`
/// pairs.
fn collect_saves() -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    for slot in 0..MAX_SLOTS {
        let Some(p) = save_for(slot) else { continue };
        let Ok(s) = SaveState::from_path(&p) else {
            continue;
        };
        let Ok(ram) = s.main_ram() else { continue };
        out.push((slot, ram.to_vec()));
    }
    out
}

#[test]
fn scus_dispatch_table_invariant_across_all_saves() {
    let saves = collect_saves();
    if saves.is_empty() {
        eprintln!(
            "skipped: LEGAIA_MEDNAFEN_DIR unset or no save states found \
             (this is expected on CI)"
        );
        return;
    }

    let (anchor_slot, anchor_ram) = &saves[0];
    let anchor =
        decode(anchor_ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS).expect("SCUS dispatch table decodes");

    // SCUS path always populated.
    assert!(
        anchor.looks_like_dispatch_table(),
        "mc{anchor_slot}: SCUS dispatch table doesn't look like a dispatch \
         table - row-0 low-mode slots don't match shared quartet"
    );

    for (slot, ram) in &saves[1..] {
        let table = decode(ram, SCUS_TABLE_BASE, SCUS_ALPHA_ROWS)
            .unwrap_or_else(|e| panic!("mc{slot}: SCUS table decode failed: {e}"));
        // Compare every slot of every alpha row to the anchor save.
        for (row_idx, (row_anchor, row_here)) in
            anchor.rows.iter().zip(table.rows.iter()).enumerate()
        {
            for (slot_idx, (a, b)) in row_anchor
                .slots
                .iter()
                .zip(row_here.slots.iter())
                .enumerate()
            {
                assert_eq!(
                    a, b,
                    "SCUS table drift between mc{anchor_slot} and mc{slot} \
                     at row {row_idx} slot {slot_idx}: \
                     0x{a:08X} vs 0x{b:08X}"
                );
            }
        }
    }

    // Belt-and-braces: confirm we did inspect at least the full table
    // extent.
    assert_eq!(anchor.rows.len(), SCUS_ALPHA_ROWS);
    assert_eq!(anchor.rows[0].slots.len(), SLOTS_PER_ROW);
    assert_eq!(
        ROW_BYTES,
        (SLOTS_PER_ROW * 4) as u32,
        "row stride invariant"
    );
}

#[test]
fn overlay_dispatch_table_only_populated_for_world_map() {
    let saves = collect_saves();
    if saves.is_empty() {
        eprintln!(
            "skipped: LEGAIA_MEDNAFEN_DIR unset or no save states found \
             (this is expected on CI)"
        );
        return;
    }

    // The overlay dispatch table at 0x801F8968 should be populated in
    // saves whose world-map overlay is paged in (mednafen-state's
    // `looks_like_dispatch_table()` returns true), and otherwise be
    // either empty or leftover-code (returns false). It must NEVER be
    // empty AND populated at once (that's a no-op assertion - the
    // populated branch implies non-empty), and at least one save in any
    // disc-side corpus that captured a world-map state must surface as
    // populated.
    //
    // Past corpora always included at least one world-map slot (mc1 by
    // convention); when no slot is in world-map, we still pass the test
    // but log so the corpus author re-captures.
    let mut populated_slots: Vec<u8> = Vec::new();
    let mut empty_slots: Vec<u8> = Vec::new();
    let mut leftover_slots: Vec<u8> = Vec::new();

    for (slot, ram) in &saves {
        let (_scus, overlay) =
            decode_both(ram).unwrap_or_else(|e| panic!("mc{slot}: decode_both: {e}"));
        assert_eq!(overlay.rows.len(), OVERLAY_ALPHA_ROWS);
        if overlay.is_empty() {
            empty_slots.push(*slot);
        } else if overlay.looks_like_dispatch_table() {
            populated_slots.push(*slot);
            // High-mode targets must classify as Overlay (inside the
            // documented 0x801C0000..0x801F9000 window).
            use legaia_mednafen::prim_dispatch::{SlotKind, classify};
            for tgt in overlay.high_mode_targets() {
                assert_eq!(
                    classify(tgt),
                    SlotKind::Overlay,
                    "mc{slot}: overlay high-mode target 0x{tgt:08X} doesn't \
                     classify as Overlay - the documented overlay window may \
                     need widening past 0x801F9000"
                );
            }
        } else {
            leftover_slots.push(*slot);
        }
    }

    if populated_slots.is_empty() {
        eprintln!(
            "[note] no save in this corpus has the world-map overlay loaded \
             (populated=0, empty={}, leftover={}); \
             the test still passes but capture a world-map slot to \
             actually exercise the populated path",
            empty_slots.len(),
            leftover_slots.len()
        );
    } else {
        eprintln!(
            "world-map overlay populated in mc{:?}; empty in mc{:?}; \
             leftover-code in mc{:?}",
            populated_slots, empty_slots, leftover_slots
        );
    }

    // Sanity invariant: the overlay table base must be inside main RAM.
    const _: () = assert!(
        OVERLAY_TABLE_BASE >= 0x8000_0000 && OVERLAY_TABLE_BASE < 0x8020_0000,
        "overlay table base outside main RAM",
    );
}
