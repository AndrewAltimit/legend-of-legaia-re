//! Disc-gated: the accessory ("Goods") passive-effect catalog built from the
//! real `SCUS_942.54` drives the engine path end to end - equipping a known
//! accessory populates the character's ability bits (so an MP-saver halves the
//! live cast cost), applies the percent stat boosts, and propagates the
//! party-wide passives through the global mask. Skips without
//! `LEGAIA_DISC_BIN`.
//!
//! Ground-truth ids (pinned in `crates/asset/tests/accessory_passive_real.rs`
//! against the executable, cross-validated vs the curated gamedata):
//! Life Ring `0xC0` = passive 0x00 (max HP +10%), Spirit Talisman `0xC5` =
//! passive 0x05 (MP Used Down 2, the Half-cost bit `0x20`), Power Ring `0xC6`
//! = passive 0x06 (ATK +20%), Golden Book `0xF0` = passive 0x30 (Gold Boost,
//! party-wide).

use legaia_engine_core::Vfs;
use legaia_engine_core::accessory_passives::AccessoryPassives;
use legaia_engine_core::world::World;
use std::path::PathBuf;

fn passives_from_disc() -> Option<AccessoryPassives> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        return None;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let table = legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus(&scus)
        .expect("parse accessory-passive table");
    Some(AccessoryPassives::from_disc(&table))
}

#[test]
fn disc_accessory_passives_drive_bits_boosts_and_party_wide_mask() {
    let Some(passives) = passives_from_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };

    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    world.set_accessory_passives(passives);

    let mut party = legaia_save::Party::zeroed(3);
    let stats = legaia_save::character::RecordStats {
        hp_max: 100,
        mp_max: 30,
        cap_constant: 100,
        agl: 40,
        atk: 100,
        udf: 50,
        ldf: 60,
        spd: 35,
        int: 20,
    };
    let live = legaia_save::character::LiveStats {
        agl: 40,
        atk: 100,
        udf: 50,
        ldf: 60,
        spd: 35,
        int: 20,
    };
    for rec in party.members.iter_mut() {
        rec.set_record_stats(stats);
        rec.set_live_stats(live);
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        hms.mp_cur = 30;
        rec.set_hp_mp_sp(hms);
    }
    // Member 0 wears the Spirit Talisman (MP Used Down 2).
    let mut eq = party.members[0].equipment();
    eq.slots[7] = 0xC5;
    party.members[0].set_equipment(eq);
    // Member 1 wears the Life Ring (max HP +10%) and the Golden Book
    // (party-wide Gold Boost) in a ring slot - retail resolves passives from
    // all 8 equipment bytes regardless of slot.
    let mut eq = party.members[1].equipment();
    eq.slots[7] = 0xC0;
    eq.slots[5] = 0xF0;
    party.members[1].set_equipment(eq);
    // Member 2 wears the Power Ring (ATK +20%).
    let mut eq = party.members[2].equipment();
    eq.slots[7] = 0xC6;
    party.members[2].set_equipment(eq);
    world.load_party(party);

    world.seed_party_battle_stats();

    // Spirit Talisman: the Half-cost ability bit (0x20) reaches the exact
    // mask + helper the engine's three cast paths consume
    // (`World::cast_spell_on_slots` / `BattleSpellSession::new` / the
    // battle-action VM host all route through `MpCostModifier`).
    use legaia_engine_vm::battle_formulas::{MpCostModifier, mp_cost_after_ability_bits};
    let bits = world.character_ability_bits[0];
    assert_eq!(bits & 0x20, 0x20, "Spirit Talisman sets the Half-cost bit");
    let modifier = MpCostModifier::from_ability_flags(bits);
    assert_eq!(modifier, MpCostModifier::Half);
    assert_eq!(
        mp_cost_after_ability_bits(8, modifier),
        4,
        "an 8-MP cast charges 4 with the Spirit Talisman equipped"
    );

    // Life Ring: max HP +10% of the base (100 -> 110) on the live actor.
    assert_eq!(
        world.actors[1].battle.max_hp, 110,
        "Life Ring boosts max HP by 10% of the base"
    );

    // Power Ring: ATK +20% of the base (100 + 100/5 = 120).
    assert_eq!(
        world.battle_attack[2], 120,
        "Power Ring boosts attack by 20% of the base"
    );
    // The wearer-only boost does not leak onto other members.
    assert_eq!(world.battle_attack[0], 100);

    // Golden Book: party-wide scope - the Gold Boost bit (index 0x30) lands
    // in the global mask, and the wearer's record carries the byte the
    // battle-end gold consumer reads (`+0xF8` word bit 16 = byte 6 bit 0).
    assert!(
        world.party_has_ability(0x30),
        "Golden Book sets the party-wide Gold Boost bit"
    );
    assert_eq!(
        world.roster.members[1].ability_bits()[6] & 0x01,
        0x01,
        "the wearer's record carries the Gold Boost byte"
    );
    // A passive nobody wears stays clear.
    assert!(!world.party_has_ability(0x10), "no Evil God Icon equipped");
}
