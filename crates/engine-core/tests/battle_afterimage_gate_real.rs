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
    // `(monster id, action tag, class)` for every entry with a non-zero
    // selector, so a regression names the record.
    let mut class_carriers = Vec::<(u16, u8, u8)>::new();
    let mut class6_carriers = Vec::<(u16, u8, u8)>::new();
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
            if b7a != 0 {
                class_carriers.push((id, tag, b7a));
            }
            if b7a == 6 {
                class6_carriers.push((id, tag, b7a));
            }
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
         +0x77 histogram: {b77_hist:?}, +0x7A (status / impact selector) histogram: {b7a_hist:?}"
    );
    eprintln!("[census] non-zero +0x7A carriers (id, tag, class): {class_carriers:?}");
    assert!(
        monsters > 100,
        "archive walk found only {monsters} monsters"
    );
    // The action record's `+0x7A` is the hit routine's **status / impact
    // selector**, not a tint index: `FUN_801EC3E4` stamps the impact-tint
    // triple only for `0 < class < 6` (`sltiu v0,v0,0x6` at `0x801EE3E0`)
    // and then routes every class to its own status arm - `3` / `4` roll
    // `+0x16E |= 1` / `|= 2` one in eight, `5` rolls a rot-limb bit on a
    // party target, and `6` (`0x801EE690`) rolls `+0x16E |= 0x1000` one in
    // four with **no** tint. So the disc legitimately carries `6` (it is
    // what the `sltiu` guard is for), and nothing past `6` has an arm. The
    // port's `IMPACT_CLASS_LIMIT` gate is that same `sltiu`: class 6 must
    // exist on the disc for the gate to be non-vacuous, and no class may
    // exceed the last routed value.
    const LAST_ROUTED_CLASS: u8 = 6;
    assert_eq!(
        legaia_engine_core::move_power::IMPACT_CLASS_LIMIT,
        LAST_ROUTED_CLASS,
        "the tint gate is `< 6`: class 6 is the status-only arm"
    );
    assert!(
        b7a_hist.keys().all(|&c| c <= LAST_ROUTED_CLASS),
        "a monster entry carries a selector with no retail arm: {b7a_hist:?}"
    );
    assert!(
        b7a_hist.contains_key(&LAST_ROUTED_CLASS),
        "no disc entry carries class 6 - the `sltiu 0x6` tint gate would be vacuous: {b7a_hist:?}"
    );
    assert!(
        !class6_carriers.is_empty() && class6_carriers.iter().all(|&(_, tag, _)| tag != 0),
        "class 6 rides attack entries, never the idle loop: {class6_carriers:?}"
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

/// The same `+0x7A` byte on the **player** side: every basic-action entry
/// of the four player battle files (`data\battle\PLAYER1..4`, extraction
/// 863..866) and every art record of their record-0 banks. The melee /
/// arts routine reads the acting record's byte whichever side is acting,
/// so this is the selector space a party swing can stamp on a monster.
#[test]
fn player_file_records_carry_only_routed_selectors() {
    std::env::var_os("LEGAIA_DISC_BIN").expect("gated above by prot_file; keep the same skip");
    let mut files = Vec::new();
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("086")
                    && (863..=866).contains(&name[..4].parse::<u32>().unwrap_or(0))
                {
                    files.push((name, e.path()));
                }
            }
            break;
        }
    }
    if files.is_empty() {
        eprintln!("[skip] extracted/PROT/086[3-6]_* missing");
        return;
    }
    files.sort();
    let mut seen = std::collections::BTreeMap::<u8, usize>::new();
    for (name, path) in &files {
        let bytes = std::fs::read(path).expect("player file reads");
        let anims = legaia_asset::battle_char_assembly::battle_animations(&bytes)
            .unwrap_or_else(|e| panic!("{name}: basic animations: {e}"));
        let basic: Vec<(u8, u8, u8, u8)> = anims
            .iter()
            .map(|a| (a.action_id, a.attach_key, a.solo_flag, a.impact_class))
            .collect();
        let record0 = legaia_asset::battle_char_assembly::decode_record0(&bytes)
            .unwrap_or_else(|e| panic!("{name}: record0: {e}"));
        let arts = legaia_asset::battle_char_assembly::art_animation_bank(&record0)
            .unwrap_or_else(|e| panic!("{name}: art bank: {e}"));
        let art_rows: Vec<(usize, u8, u8)> = arts
            .iter()
            .map(|r| (r.index, r.anim_id, r.impact_class))
            .collect();
        for &(_, _, _, c) in &basic {
            *seen.entry(c).or_default() += 1;
        }
        for &(_, _, c) in &art_rows {
            *seen.entry(c).or_default() += 1;
        }
        eprintln!("[census] {name}: basic (tag, +0x77, +0x87, +0x7A) = {basic:?}");
        eprintln!(
            "[census] {name}: art records (index, anim_id, +0x7A) with a non-zero selector = {:?} of {}",
            art_rows.iter().filter(|r| r.2 != 0).collect::<Vec<_>>(),
            art_rows.len()
        );
    }
    eprintln!("[census] player-side +0x7A histogram: {seen:?}");
    assert!(
        seen.keys().all(|&c| c <= 6),
        "a player record carries a selector with no retail arm: {seen:?}"
    );
}
