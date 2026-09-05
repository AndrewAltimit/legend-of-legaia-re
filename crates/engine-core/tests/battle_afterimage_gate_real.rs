//! Disc-gated oracle for the arts after-image **ring-id gate** on monster
//! seats (`legaia_engine_core::battle_afterimage::monster_ring_id`).
//!
//! Retail's anim tick (`FUN_80047430`, `0x80048044..0x80048060`) stamps a
//! monster's history-ring id from the committed record: `record[+0x77] +
//! 0x10`, or `0x11` when `record[+0x87] == 1`; the ghost walk
//! (`FUN_80049348`, `sltiu 0x11` at `0x80049460`) draws only ids `>= 0x11`.
//! So whether a monster ever ghosts is a property of the two record bytes,
//! and this test reads them off every action entry in the monster archive
//! (PROT 0867).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing
//! (CI without disc data).

use legaia_asset::monster_archive;
use legaia_engine_core::battle_afterimage as ai;
use std::path::PathBuf;

fn prot_file(name: &str) -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join(name);
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

/// `(action_tag, +0x77, +0x87, +0x7A)` for every entry of one monster's
/// action table, in table order (the same `0x4A` count / `0x4C` offset walk
/// `monster_archive::animations` runs).
fn entry_bytes(entry: &[u8], id: u16) -> Option<Vec<(u8, u8, u8, u8)>> {
    let block = monster_archive::decode_block(entry, id).ok()??;
    let count = *block.get(0x4a)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let off = legaia_bytes::u32_le(&block, 0x4c + i * 4)? as usize;
        let tag = *block.get(off)?;
        let b77 = block.get(off + 0x77).copied().unwrap_or(0);
        let b87 = block.get(off + 0x87).copied().unwrap_or(0);
        let b7a = block.get(off + 0x7a).copied().unwrap_or(0);
        out.push((tag, b77, b87, b7a));
    }
    Some(out)
}

#[test]
fn monster_ring_id_gate_matches_the_disc_records() {
    let Some(entry) = prot_file("0867_battle_data.BIN") else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut monsters = 0usize;
    let mut entries = 0usize;
    let mut idle_eligible = 0usize;
    let mut non_idle_eligible = 0usize;
    let mut flag87 = 0usize;
    let mut b77_hist = std::collections::BTreeMap::<u8, usize>::new();
    let mut b7a_hist = std::collections::BTreeMap::<u8, usize>::new();
    for id in 1..=194u16 {
        let Some(rows) = entry_bytes(&entry, id) else {
            continue;
        };
        if rows.is_empty() {
            continue;
        }
        monsters += 1;
        for (tag, b77, b87, b7a) in rows {
            entries += 1;
            *b77_hist.entry(b77).or_default() += 1;
            *b7a_hist.entry(b7a).or_default() += 1;
            if b87 == 1 {
                flag87 += 1;
            }
            let ring = ai::monster_ring_id(b77, b87);
            let eligible = ai::ghost_eligible(ring);
            if tag == 0 {
                if eligible {
                    idle_eligible += 1;
                    eprintln!(
                        "monster {id}: idle entry ghost-eligible (+0x77={b77:#x} +0x87={b87})"
                    );
                }
            } else if eligible {
                non_idle_eligible += 1;
            }
        }
    }
    eprintln!(
        "[census] {monsters} monsters, {entries} entries, +0x87==1: {flag87}, \
         idle eligible: {idle_eligible}, non-idle eligible: {non_idle_eligible}, \
         +0x77 histogram: {b77_hist:?}, +0x7A (impact class) histogram: {b7a_hist:?}"
    );
    assert!(
        monsters > 100,
        "archive walk found only {monsters} monsters"
    );
    // The melee routine's `sltiu v0,v0,0x6` bound on the record's `+0x7A`
    // (`FUN_801EC3E4` 0x801EE3E0): the disc never carries a class the
    // 5-entry `0x801F53D4` table cannot serve, so the "read past the
    // table" arm is unreachable from retail data.
    assert!(
        b7a_hist
            .keys()
            .all(|&c| c < legaia_engine_core::move_power::IMPACT_CLASS_LIMIT),
        "a monster entry carries an impact class past the table: {b7a_hist:?}"
    );
    // The observable retail behaviour the gate exists for: an idle monster
    // never ghosts.
    assert_eq!(
        idle_eligible, 0,
        "an idle monster entry passed the retail ring-id gate"
    );
}

/// The two tutorial encounters the coordinator's captures cover (Gobu
/// Gobu id 4, Tetsu id 79): their idle entries are ineligible, pinned
/// individually so a regression names the monster.
#[test]
fn tetsu_and_gobu_gobu_idle_entries_never_ghost() {
    let Some(entry) = prot_file("0867_battle_data.BIN") else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    for (id, name) in [(4u16, "Gobu Gobu"), (79, "Tetsu")] {
        let rows = entry_bytes(&entry, id).unwrap_or_else(|| panic!("{name} decodes"));
        let (tag, b77, b87, _) = rows[0];
        assert_eq!(tag, 0, "{name}: table entry 0 is the idle loop");
        assert!(
            !ai::ghost_eligible(ai::monster_ring_id(b77, b87)),
            "{name}: idle entry +0x77={b77:#x} +0x87={b87} must not ghost"
        );
        eprintln!("{name}: {rows:?}");
    }
}
