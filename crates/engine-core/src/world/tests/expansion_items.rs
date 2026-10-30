//! Pack items on a disc-free world (`world::expansion_items`): grants and
//! counts by item reference, pack gear folding into the battle seed and
//! the ability bits, the borrowed look, the per-fight aliases behind a pack
//! drop and the Item command, the item screen's buy and equip through
//! `World::tick`, the progress surviving deactivation, and no pack leaving
//! every hook the identity.

use std::sync::Arc;

use super::*;
use legaia_engine_battle::battle_stats::{EquipmentTable, ItemModifier};
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::pad::PadButton;
use legaia_expansion_format::items::ItemRef;
use legaia_expansion_format::{ExpansionPack, PackFiles};
use legaia_save::EquipmentSlots;
use legaia_save::character::LiveStats;

const ITEMS_TOML: &str = r#"
[items.rebirth_blade]
name = "Rebirth Blade"
kind = "weapon"
icon = { kind = "item", id = 0x27 }
price = 1000
characters = ["vahn"]
attack = 108
set = "rebirth"

[items.rebirth_mail]
name = "Rebirth Mail"
kind = "armor"
defence = 20
set = "rebirth"

[items.tonic]
name = "Tonic"
kind = "consumable"
price = 50
effect = { kind = "heal_hp", amount = 300 }

[sets.rebirth]
name = "Rebirth"
bonuses = [{ pieces = 2, attack = 6, passives = [0x06] }]

[[shops]]
id = "armory"
name = "Armory"
stock = ["rebirth_blade", "tonic", 0x77]
"#;

const BATTLE_TOML: &str = r#"
[monsters.keeper]
base = { kind = "monster", id = 9 }
name = "Keeper"
drop = { item = "rebirth_blade", chance_pct = 100 }

[formations.den]
monsters = ["keeper"]

[bosses.keeper]
formation = "den"
reward_items = [{ item = "tonic", count = 3 }]
"#;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("items.toml".into(), ITEMS_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE_TOML.as_bytes().to_vec());
    Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

/// Vahn with real stats wearing a retail weapon (`0x27`, +72 ATK) in byte 2.
fn world() -> World {
    let mut w = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    w.mode = SceneMode::Field;
    let mut party = legaia_save::Party::zeroed(1);
    party.members[0].set_live_stats(LiveStats {
        agl: 12,
        atk: 90,
        udf: 40,
        ldf: 40,
        spd: 20,
        int: 10,
    });
    let mut slots = [0u8; 8];
    slots[2] = 0x27;
    party.members[0].set_equipment(EquipmentSlots { slots });
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_max = 900;
    hms.hp_cur = 100;
    party.members[0].set_hp_mp_sp(hms);
    w.load_party(party);
    let mut table = EquipmentTable::new();
    table.set(
        0x27,
        ItemModifier {
            atk: 72,
            udf: 0,
            ldf: 0,
            spd: 0,
            int: 0,
            ability_bits: [0; 32],
        },
    );
    w.set_equipment_table(table);
    w.actors[0].active = true;
    w.actors[0].battle.hp = 400;
    w.actors[0].battle.max_hp = 400;
    w.actors[0].battle.liveness = 1;
    let mut cat = MonsterCatalog::new();
    let mut wolf = MonsterDef::new(9, "Wolf", 35, 12);
    wolf.exp = 20;
    wolf.gold = 40;
    cat.insert(wolf);
    w.set_monster_catalog(cat);
    w.tables
        .formation_table
        .insert(FormationDef::new(1, vec![FormationSlot::new(9)]));
    w.rng_state = 0x0BAD_5EED;
    w
}

fn press(w: &mut World, b: PadButton) {
    w.input.set_pad(b.mask());
    w.tick();
    w.input.set_pad(0);
    w.tick();
}

#[test]
fn with_no_pack_every_item_hook_is_the_identity() {
    let mut w = world();
    assert_eq!(
        w.member_look_equipment(0),
        w.party.roster.members[0].equipment().slots
    );
    assert!(w.pack_worn_bonus(0).is_none());
    assert!(w.grant_item_ref(&ItemRef::Retail(0x77), 2));
    assert_eq!(w.party.inventory.get(&0x77).copied(), Some(2));
    assert!(!w.grant_item_ref(&ItemRef::Pack("rebirth_blade".into()), 1));
    assert!(w.grant_item_id(0xFD, 1), "an unlent id is a retail id");
    assert_eq!(w.party.inventory.get(&0xFD).copied(), Some(1));
    assert!(!w.open_item_shop(""));
    w.seed_party_battle_stats();
    assert_eq!(w.battle.equip_atk[0], [0, 0, 72, 0, 0]);
}

#[test]
fn pack_gear_displaces_retail_gear_and_folds_into_the_battle_seed() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    w.seed_party_battle_stats();
    let (retail_atk, retail_def) = (w.battle.attack[0], w.battle.defense_split[0]);
    assert!(w.grant_item_ref(&ItemRef::Pack("rebirth_blade".into()), 1));
    assert!(
        !w.equip_pack_item("rebirth_blade", 0, 0),
        "a weapon is byte 2"
    );
    assert!(w.equip_pack_item("rebirth_blade", 0, 2));
    // The retail weapon went to the bag and the byte is empty.
    assert_eq!(w.party.roster.members[0].equipment().slots[2], 0);
    assert_eq!(w.party.inventory.get(&0x27).copied(), Some(1));
    // The look is the borrowed Chaos Breaker; the stats are the pack's.
    assert_eq!(w.member_look_equipment(0)[2], 0x27);
    w.seed_party_battle_stats();
    assert_eq!(
        w.battle.equip_atk[0][2], 108,
        "the swing folds the pack ATK"
    );
    assert_eq!(w.battle.attack[0], retail_atk, "base ATK is unchanged");
    assert_eq!(w.battle.defense_split[0], retail_def);
    // A second set piece adds the two-piece tier.
    w.grant_item_ref(&ItemRef::Pack("rebirth_mail".into()), 1);
    assert!(w.equip_pack_item("rebirth_mail", 0, 0));
    w.seed_party_battle_stats();
    assert_eq!(w.battle.attack[0], retail_atk + 6, "the set's +6");
    assert_eq!(w.battle.defense_split[0], Some((60, 60)));
    let b = w.pack_worn_bonus(0).unwrap();
    assert_ne!(b.ability_words[0] & (1 << 6), 0, "the tier's ATK Boost");
    // The compare panel reads the same fold.
    let now = w.pack_stat_line(0, None);
    let off = w.pack_stat_line(0, Some((2, None)));
    assert_eq!(now.atk - off.atk, 108 + 6, "the blade and its tier");
    // A retail item equipped over it sends the pack item home.
    let mut eq = w.party.roster.members[0].equipment();
    eq.slots[2] = 0x27;
    w.party.roster.members[0].set_equipment(eq);
    w.reconcile_pack_gear();
    assert_eq!(w.pack_item_progress().count("rebirth_blade"), 1);
    assert_eq!(w.pack_item_progress().worn_at(0, 2), None);
}

#[test]
fn the_item_screen_buys_and_equips_through_the_tick() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    w.party.money = 1500;
    assert!(w.open_item_shop("armory"));
    assert!(w.item_screen_open());
    w.tick();
    let m = w.item_screen_view().unwrap();
    assert_eq!(m.title, "Armory");
    let names: Vec<&str> = m.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names[..2], ["Rebirth Blade", "Tonic"]);
    press(&mut w, PadButton::Cross);
    press(&mut w, PadButton::Cross);
    assert_eq!(w.pack_item_progress().count("rebirth_blade"), 1);
    assert_eq!(w.party.money, 500);
    press(&mut w, PadButton::Circle);
    assert!(!w.item_screen_open());
    assert_eq!(w.mode, SceneMode::Field);
    // The bag's Gear tab: wear it on Vahn, with the compare up first.
    assert!(w.open_item_bag(true));
    w.tick();
    press(&mut w, PadButton::Cross);
    let cmp = w.item_screen_view().unwrap().compare.expect("compare");
    assert_eq!(cmp.rows[0].0, "ATK");
    assert_eq!(cmp.rows[0].2 - cmp.rows[0].1, 108 - 72);
    press(&mut w, PadButton::Cross);
    assert_eq!(w.pack_item_progress().worn_at(0, 2), Some("rebirth_blade"));
}

#[test]
fn a_pack_drop_and_a_boss_reward_land_in_the_pack_bag() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert!(w.start_expansion_boss("keeper"));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    let alias = w
        .expansion
        .items
        .aliases
        .id_of("rebirth_blade")
        .expect("lent");
    assert_eq!(
        w.tables.monster_catalog.get(9).unwrap().drop_item,
        Some(alias)
    );
    for _ in 0..8000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    let r = w.battle.last_rewards.clone().expect("the party wins");
    assert_eq!(r.drops, [alias], "the drop rides its alias");
    assert_eq!(w.pack_item_progress().count("rebirth_blade"), 1);
    assert_eq!(w.pack_item_progress().count("tonic"), 3, "the boss reward");
    assert!(
        w.party.inventory.get(&alias).is_none(),
        "never the retail bag"
    );
    // The results window can still name it.
    assert_eq!(
        w.tables.item_catalog.get(alias).map(|e| e.name),
        Some("Rebirth Blade")
    );
}

#[test]
fn a_pack_consumable_is_a_row_of_the_battle_item_command() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    w.grant_item_ref(&ItemRef::Pack("tonic".into()), 2);
    let formation = FormationDef::new(1, vec![FormationSlot::new(9)]);
    w.expansion_prepare_battle(&formation);
    let alias = w.expansion.items.aliases.id_of("tonic").expect("lent");
    let session = w.build_battle_item_session();
    assert!(session.items.contains(&alias));
    w.mode = SceneMode::Battle;
    w.actors[0].battle.hp = 50;
    let out = w.apply_battle_item(alias, 0);
    assert_eq!(
        out,
        legaia_engine_menus::items::ItemOutcome::HealedHp { amount: 300 }
    );
    w.consume_item(alias);
    assert_eq!(w.pack_item_progress().count("tonic"), 1);
}

#[test]
fn a_field_use_runs_the_retail_handler_and_spends_one() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    w.grant_item_ref(&ItemRef::Pack("tonic".into()), 1);
    let before = w.party.roster.members[0].hp_mp_sp().hp_cur;
    let out = w.use_pack_item("tonic", 0);
    assert_eq!(
        out,
        Some(legaia_engine_menus::items::ItemOutcome::HealedHp { amount: 300 })
    );
    assert_eq!(w.party.roster.members[0].hp_mp_sp().hp_cur, before + 300);
    assert_eq!(w.pack_item_progress().count("tonic"), 0);
    assert!(
        legaia_engine_expansion::items::ALIAS_POOL
            .iter()
            .all(|&id| w.tables.item_catalog.get(id).is_none()),
        "the scratch alias went back"
    );
}

#[test]
fn pack_items_survive_deactivation_as_dormant_progress() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    w.grant_item_ref(&ItemRef::Pack("rebirth_blade".into()), 1);
    w.equip_pack_item("rebirth_blade", 0, 2);
    w.deactivate_expansion();
    assert!(w.pack_worn_bonus(0).is_none(), "no pack, no bonus");
    w.seed_party_battle_stats();
    assert_eq!(w.battle.equip_atk[0][2], 0, "the slot is empty without it");
    w.activate_expansion(pack()).unwrap();
    assert_eq!(w.pack_item_progress().worn_at(0, 2), Some("rebirth_blade"));
    let block = w.expansion.quests.progress.encode();
    let back = legaia_engine_expansion::quest_save::ExpansionProgress::decode(&block).unwrap();
    assert_eq!(back.items.worn_at(0, 2), Some("rebirth_blade"));
}
