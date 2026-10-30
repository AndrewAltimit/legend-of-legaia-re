//! Disc-gated: a pack weapon worn by Vahn reaches a real battle. The fight
//! seeds the weapon slot's per-swing attack from the pack item (where the
//! same fight with the retail Chaos Breaker seeds the disc table's 72), the
//! party's swing costs and battle mesh read the item's borrowed look, and
//! the retail weapon the pack item displaced waits in the bag. Skips and
//! passes without `LEGAIA_DISC_BIN`.

use std::sync::Arc;

use legaia_engine_core::live_loop::LiveLoopOpts;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::vfs::Vfs;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::items::ItemRef;
use legaia_expansion_format::{ExpansionPack, PackFiles};
use legaia_save::EquipmentSlots;
use legaia_save::character::LiveStats;

const CHAOS_BREAKER: u8 = 0x27;
const CARUBAN: u16 = 0x49;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"disctest\"\nname = \"Disc Test\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "items.toml".into(),
        br#"
[items.rebirth_blade]
name = "Rebirth Blade"
kind = "weapon"
icon = { kind = "item", id = 0x27 }
characters = ["vahn"]
attack = 112
"#
        .to_vec(),
    );
    files.insert(
        "battle.toml".into(),
        format!(
            "[formations.f]\nmonsters = [{{ kind = \"monster\", id = {CARUBAN} }}]\n[bosses.b]\nformation = \"f\"\nonce = false\n"
        )
        .into_bytes(),
    );
    Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

fn enter_fight(host: &mut SceneHost) {
    assert!(host.world.start_expansion_boss("b"));
    for _ in 0..2_000 {
        if host.world.mode == SceneMode::Battle {
            break;
        }
        host.world.tick();
    }
    assert_eq!(host.world.mode, SceneMode::Battle, "the fight starts");
}

#[test]
fn a_worn_pack_weapon_changes_the_attack_a_real_battle_seeds() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open the disc");
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world
        .arm_live_loop("town01", &LiveLoopOpts::playable());
    host.world.toggles.live_gameplay_loop = true;
    host.world.battle.player_driven = false;

    let mut party = legaia_save::Party::zeroed(1);
    party.members[0].set_live_stats(LiveStats {
        agl: 40,
        atk: 300,
        udf: 200,
        ldf: 200,
        spd: 60,
        int: 50,
    });
    let mut slots = [0u8; 8];
    slots[2] = CHAOS_BREAKER;
    party.members[0].set_equipment(EquipmentSlots { slots });
    host.world.load_party(party);
    // The disc's equipment table, as the play session installs it at boot.
    let scus = legaia_engine_scene::vfs::DiscVfs::open(std::path::Path::new(&disc))
        .expect("open the disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let stats = legaia_game_tables::equip_stats::EquipStatTable::from_scus(&scus)
        .expect("the equipment table parses");
    host.world.set_equipment_table(
        legaia_engine_menus::equipment::equip_modifier_table_from_disc(&stats),
    );
    let retail_atk = host
        .world
        .tables
        .equipment_table
        .get(CHAOS_BREAKER)
        .expect("the disc's equipment table")
        .atk;
    assert_eq!(retail_atk, 72, "the static table's Chaos Breaker");

    host.world
        .activate_expansion(pack())
        .expect("pack activates");
    host.merge_expansion_monsters();

    // Baseline: the retail weapon.
    enter_fight(&mut host);
    assert_eq!(host.world.battle.equip_atk[0][2], 72);
    let base_line = host.world.battle.attack[0];
    host.world.abandon_live_battle();
    for _ in 0..60 {
        host.world.tick();
    }

    assert!(
        host.world
            .grant_item_ref(&ItemRef::Pack("rebirth_blade".into()), 1)
    );
    assert!(host.world.equip_pack_item("rebirth_blade", 0, 2));
    assert_eq!(
        host.world.party.inventory.get(&CHAOS_BREAKER).copied(),
        Some(1)
    );
    assert_eq!(host.world.member_look_equipment(0)[2], CHAOS_BREAKER);

    enter_fight(&mut host);
    assert_eq!(
        host.world.battle.equip_atk[0][2], 112,
        "the pack weapon's ATK is the slot's swing fold"
    );
    assert_eq!(host.world.battle.attack[0], base_line, "base ATK unchanged");
    assert_eq!(
        legaia_engine_battle_vm::battle_formulas::arms_weapon_atk_fold(
            0x11,
            &host.world.battle.equip_atk[0]
        ),
        Some(56),
        "an art folds half the weapon"
    );
    host.world.abandon_live_battle();
    host.world.deactivate_expansion();
}
