//! Disc-gated: the monster flee roll (`FUN_801EC0DC`) never lets the Rim Elm
//! sparring partner run, and still lets a weak random-encounter monster run.
//!
//! The sparring fight is town01 formation row 4, whose `record[+0]` header
//! byte is `0`: the scripted-fight flag `ctx[+0x287]` is clear (retail's
//! battle states over that fight read `ctx+0x287 = 0`, `DAT_8007BD60 = 0x01`,
//! `_DAT_8007BAC0 = 0`), so neither of the roll's two gates refuses it. What
//! keeps Tetsu on the field in retail is the roll's own arithmetic: the
//! monster side is **averaged over the seated monster count** `ctx[+1]`
//! (`lbu v0,0x1(v1)` / `div s1,v0` at `0x801EC278..0x801EC2A8`), and a lone
//! 999-HP monster's average dwarfs anything a starting party can roll.
//!
//! The defect this pins: the engine averaged over every slot of its actor
//! table above the party (the whole preallocated pool), so a lone monster's
//! side score was divided by the table size and a wounded Tetsu fled.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use super::*;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

fn prot_entry(dir: &std::path::Path, index: usize) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries.get(index).cloned().expect("PROT entry");
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read entry");
    bytes
}

/// PROT entry the monster archive lives in.
const MONSTER_ARCHIVE_PROT: usize = 867;

/// Tetsu's training record - the one monster of town01 formation row 4.
const TETSU_TRAINING: u16 = 0x4F;

/// A lone-monster formation seated through the real battle entry, header
/// byte `0` (the class town01 row 4 belongs to), against a one-member party
/// with a starting character's stats.
fn seat_lone(rec: &legaia_asset::monster_archive::MonsterRecord) -> World {
    let mut w = World::default();
    w.party.party_count = 1;
    w.tables
        .monster_catalog
        .insert(crate::monster_catalog::monster_def_from_record(rec));
    let formation = crate::monster_catalog::FormationDef::new(
        4,
        vec![crate::monster_catalog::FormationSlot::new(rec.id)],
    );
    w.enter_battle_from_formation(&formation);
    // A level-1 Vahn: healthy, ~80 HP, ~20 ATK.
    w.actors[0].battle.max_hp = 80;
    w.actors[0].battle.hp = 80;
    w.actors[0].battle.liveness = 1;
    w.battle.attack[0] = 20;
    w
}

/// Count the seeds (of `seeds`) on which the monster in slot 1 rolls a flee
/// at `hp`.
/// Each roll re-arms the once-per-pass checkpoint and reseeds the stream, so
/// every seed is one fresh picker pass.
fn flee_count(w: &mut World, hp: u16, seeds: u32) -> u32 {
    (0..seeds)
        .filter(|&seed| {
            w.actors[1].battle.hp = hp;
            w.battle.monster_flee_attempted = false;
            w.rng_state = seed.wrapping_mul(2_654_435_761);
            matches!(w.pick_monster_action(1), MonsterAction::Flee)
        })
        .count() as u32
}

#[test]
fn the_sparring_partner_never_flees() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let archive = prot_entry(&dir, MONSTER_ARCHIVE_PROT);
    let rec = legaia_asset::monster_archive::record(&archive, TETSU_TRAINING)
        .expect("archive parses")
        .expect("Tetsu's training record");
    let mut w = seat_lone(&rec);
    assert!(
        !w.battle.scripted_fight,
        "town01 row 4 carries header byte 0, as retail's ctx+0x287 = 0 reads"
    );
    let max = w.actors[1].battle.max_hp;
    assert!(max > 0, "Tetsu seated with HP");
    // Every wound level, many seeds each.
    let mut total = 0;
    for hp in [max, max / 2, max / 4, max / 10, 1] {
        total += flee_count(&mut w, hp, 2_000);
    }
    assert_eq!(total, 0, "Tetsu (max HP {max}) must never flee");
    eprintln!("[ran] Tetsu max HP {max}: 0 flees over 10000 rolls");
}

/// The contrast that keeps the test above non-vacuous: the roll is live in a
/// random encounter. A weak archive monster, wounded, facing a much stronger
/// party, flees on roughly one roll in eight - the flat `rand() & 7` gate.
#[test]
fn a_weak_wounded_random_monster_can_still_flee() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let archive = prot_entry(&dir, MONSTER_ARCHIVE_PROT);
    let slots = legaia_asset::monster_archive::slot_count(&archive) as u16;
    // The weakest record by HP + ATK that seats at all.
    let rec = (1..=slots)
        .filter_map(|id| {
            legaia_asset::monster_archive::record(&archive, id)
                .ok()
                .flatten()
        })
        .filter(|r| r.hp > 0)
        .min_by_key(|r| u32::from(r.hp) + u32::from(r.stats[1]))
        .expect("some seatable record");
    let mut w = seat_lone(&rec);
    // A strong party member: the monster side's `*3/2` floor is what it has
    // to stay under, so the party has to dominate.
    w.actors[0].battle.max_hp = 9999;
    w.actors[0].battle.hp = 9999;
    w.battle.attack[0] = 999;
    let fled = flee_count(&mut w, 1, 2_000);
    assert!(
        fled > 0,
        "monster {:#x} (HP {}) at 1 HP never fled - the roll is dead",
        rec.id,
        rec.hp
    );
    eprintln!(
        "[ran] monster {:#x} (HP {}): {fled}/2000 flees at 1 HP",
        rec.id, rec.hp
    );
}
