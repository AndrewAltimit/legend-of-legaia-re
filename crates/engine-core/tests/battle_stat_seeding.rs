//! Regressions for the two monster stat-seeding defects that made every
//! engine-driven battle easier than retail's.
//!
//! Both fail against the pre-fix engine:
//!
//! 1. **The battle-load stat boost was not applied.** `monster_def_from_record`
//!    read the raw record accessors. Retail's record→actor copy `FUN_80054CB0`
//!    *boosts* four of the six combat stats while copying, so the raw record
//!    systematically understates the enemy the player actually fights.
//! 2. **Monster slots collapsed defence** to `max(udf, ldf)`, which left the
//!    melee kernel's UDF/LDF parity branch dead for the whole monster band.
//!
//! The disc-gated test at the bottom measures (1) against the real monster
//! archive; everything else is disc-free and skips nothing.

use std::path::PathBuf;

use legaia_asset::monster_archive::MonsterRecord;
use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
    monster_def_from_record,
};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// The Gaza (Sim-Seru) record, id 166 - the fight whose live international
/// retail capture pins the gate-set boost profile. Raw record stats, in record
/// order: `[AGL 128, ATK 288, UDF 222, LDF 200, INT 220, SPD 146]`.
fn gaza_record() -> MonsterRecord {
    MonsterRecord {
        id: 166,
        name: "Gaza".into(),
        hp: 15000,
        mp: 1200,
        stats: [128, 288, 222, 200, 220, 146],
        element: 6,
        size_class: 26,
        gold: 30000,
        exp: 42000,
        drop_item: 0,
        drop_chance_pct: 0,
        seru_id: 0,
        catch_rate_pct: 0,
        magic_count: 0,
        spells: vec![],
        magic_attacks: vec![],
    }
}

/// The catalog entry carries the *boosted* profile, stat for stat.
///
/// `FUN_80054CB0` copies the record's six stats into the actor and then, on the
/// gate-set profile, re-reads each record value and adds a shifted copy of it
/// to the stored actor value: `ATK += ATK>>2` (×5/4), `UDF += UDF` and
/// `LDF += LDF` (×2), `INT += INT>>3` (×9/8). AGL and SPD are never touched by
/// the boost block at all, and HP / MP are copied before it.
///
/// Before the fix every one of these read the raw record value.
#[test]
fn a_catalog_entry_carries_the_battle_load_boost() {
    let rec = gaza_record();
    let def = monster_def_from_record(&rec);

    // Boosted four.
    assert_eq!(def.attack, 360, "ATK ×5/4: 288 + (288>>2)");
    assert_eq!(def.udf, 444, "UDF ×2: 222 * 2");
    assert_eq!(def.ldf, 400, "LDF ×2: 200 * 2");
    assert_eq!(def.intel, 247, "INT ×9/8: 220 + (220>>3)");

    // Pass-through two, plus HP/MP.
    assert_eq!(def.agl, 128, "AGL is copied unchanged");
    assert_eq!(def.speed, 146, "SPD is copied unchanged");
    assert_eq!(def.hp, 15000);
    assert_eq!(def.mp, 1200);

    // Every boosted stat must differ from the raw record - otherwise this test
    // would still pass against a catalog that silently reverted to the raw
    // accessors for a record whose boost happens to be a no-op.
    assert_ne!(def.attack, rec.attack());
    assert_ne!(def.udf, rec.defense_high());
    assert_ne!(def.ldf, rec.defense_low());
    assert_ne!(def.intel, rec.intelligence());

    // The whole tuple, in one shot, against the archive's own kernel.
    let bs = rec.battle_stats();
    assert_eq!(
        [def.agl, def.attack, def.udf, def.ldf, def.intel, def.speed],
        bs
    );
}

/// The accuracy / evasion bytes are clamped from the
/// **boosted** INT, because the actor field the interrupt roll reads
/// (`+0x168`) is the post-boost one - the boost block's last store writes
/// `+0x168`/`+0x16A` itself.
#[test]
fn accuracy_and_evasion_clamp_the_boosted_int() {
    let rec = gaza_record();
    let def = monster_def_from_record(&rec);
    // 247 boosted, 220 raw - both fit in a byte, so the clamp cannot hide the
    // difference.
    assert_eq!(def.accuracy, 247);
    assert_eq!(def.evasion, 247);
}

/// A world with `party` party slots seated in battle against `formation`.
fn battle_world(monsters: &[MonsterDef], slots: &[FormationSlot]) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.max_hp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 40);
    }
    let mut catalog = MonsterCatalog::new();
    for m in monsters {
        catalog.insert(m.clone());
    }
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, slots.to_vec()));
    w.set_formation_table(table, catalog);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(1), "formation row 1 registers");
    // The scripted entry runs the field-to-battle intro transition (132
    // display frames) before the mode flips.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    w
}

/// A monster slot carries both defence facets, so the melee
/// kernel's parity pick resolves to two different numbers.
///
/// `FUN_801EC3E4` reads UDF (`+0x15C`) when `(command - 0x0C) % 10 < 5` and LDF
/// (`+0x160`) otherwise. Before the fix battle entry wrote only the single
/// `battle_defense` scalar (`max(udf, ldf)`), so `physical_defense_of` fell
/// through to it for every monster and both halves answered the same value -
/// the branch existed but could never be taken on the enemy side.
#[test]
fn a_monster_slot_carries_both_defence_facets() {
    let mut lopsided = MonsterDef::new(1, "Lopsided", 400, 30);
    lopsided.udf = 90;
    lopsided.ldf = 10;
    let w = battle_world(&[lopsided], &[FormationSlot::new(1)]);
    let mslot = w.party_count; // first monster

    assert_eq!(
        w.battle_defense_split[mslot as usize],
        Some((90, 10)),
        "battle entry must seed the monster band's (UDF, LDF) pair"
    );

    // The parity pick must now resolve to genuinely different numbers. `0x0C`
    // is the arm command (a UDF-target swing); `0x11` lands in the LDF half.
    let udf_side = w.physical_defense_of(mslot, 0x0C);
    let ldf_side = w.physical_defense_of(mslot, 0x11);
    assert_eq!(udf_side, 90);
    assert_eq!(ldf_side, 10);
    assert_ne!(
        udf_side, ldf_side,
        "the kernel's UDF/LDF branch is inert for enemies again"
    );
}

/// A monster slot inherited from a previous battle's
/// party member does not keep that member's split when the new formation's
/// monster id misses the catalog.
#[test]
fn battle_entry_clears_a_stale_monster_defence_split() {
    let mut w = battle_world(
        &[MonsterDef::new(1, "Mob", 100, 10)],
        &[FormationSlot::new(1)],
    );
    // Stamp a bogus split on an unoccupied monster slot, then re-enter battle
    // with a formation whose id is absent from the catalog.
    w.set_battle_defense_split(4, Some((999, 999)));
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(2, vec![FormationSlot::new(77)])); // id 77 not in catalog
    let catalog = w.monster_catalog.clone();
    w.set_formation_table(table, catalog);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(2));
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    assert_eq!(
        w.battle_defense_split[4], None,
        "a monster slot must not defend with the split a previous occupant left"
    );
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

/// Disc-gated: the boost against the real monster archive.
///
/// For every decodable record on the disc, the catalog entry must equal the
/// boosted profile and - wherever the raw stat is non-zero - must exceed the
/// raw record stat. The second half is what makes this non-vacuous: a catalog
/// that reverted to the raw accessors would still satisfy the first half for
/// records whose stats are all zero.
#[test]
fn real_archive_records_seed_the_boosted_profile() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut archive =
        legaia_prot::archive::Archive::open(&extracted.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[867].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 867");

    let mut checked = 0usize;
    let mut boosted_atk = 0usize;
    let mut boosted_def = 0usize;
    for id in 1..=255u16 {
        let Ok(Some(rec)) = legaia_asset::monster_archive::record(&bytes, id) else {
            continue;
        };
        let def = monster_def_from_record(&rec);
        let bs = rec.battle_stats();
        checked += 1;
        assert_eq!(
            [def.agl, def.attack, def.udf, def.ldf, def.intel, def.speed],
            bs,
            "monster {id} ({}) does not carry the boosted profile",
            rec.name
        );
        // Strict inequality wherever the boost has room to act. `ATK>>2` is
        // zero below 4, and `INT>>3` below 8, so only test above those.
        if rec.attack() >= 4 {
            assert!(
                def.attack > rec.attack(),
                "monster {id} ({}) ATK {} not boosted above the raw record {}",
                rec.name,
                def.attack,
                rec.attack()
            );
            boosted_atk += 1;
        }
        if rec.defense_high() > 0 {
            assert_eq!(def.udf, rec.defense_high() * 2);
            assert!(def.udf > rec.defense_high());
            boosted_def += 1;
        }
        // AGL / SPD must NOT move.
        assert_eq!(def.agl, rec.agility());
        assert_eq!(def.speed, rec.speed());
    }

    assert!(
        checked >= 100,
        "expected 100+ decodable monster records, got {checked}"
    );
    assert!(
        boosted_atk >= 100 && boosted_def >= 100,
        "the strict-inequality half barely ran (atk {boosted_atk}, def \
         {boosted_def}) - this test would pass against the raw accessors"
    );
}
