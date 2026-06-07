//! Decode the real equipment stat-bonus table out of `extracted/SCUS_942.54`
//! if present. Skips and passes when the executable isn't on disk - same gating
//! pattern as the other disc-dependent tests so CI doesn't need Sony bytes.
//!
//! The attack / defense bytes and equip masks are validated against the curated
//! gamedata values (data/gamedata/{weapons,armor}.toml).

use legaia_asset::equip_stats::{EquipSlot, EquipStatTable};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_equip_stat_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = EquipStatTable::from_scus(&bytes).expect("parse equip-stat table");

    // The equipment ids reach bonus rows well past 100 (Ra-Seru Thongs = 104).
    assert!(table.record_count() > 100, "bonus row count");

    // Weapons: +1 is the attack bonus, matching weapons.toml. Slot = Weapon.
    let weapon = |id: u8, attack: u8, mask: u8| {
        let b = table.bonus(id).unwrap_or_else(|| panic!("weapon {id:#x}"));
        assert_eq!(b.attack(), attack, "weapon {id:#x} attack");
        assert_eq!(b.slot(), EquipSlot::Weapon, "weapon {id:#x} slot");
        assert_eq!(b.def_up(), 0, "weapon {id:#x} no def-up");
        assert_eq!(b.equip_mask(), mask, "weapon {id:#x} equip mask");
    };
    weapon(0x22, 6, 7); // Survival Knife: attack 6, equippable by anyone (mask 7).
    weapon(0x25, 40, 1); // Force Blade: attack 40, Vahn only.
    weapon(0x27, 72, 1); // Chaos Breaker: attack 72, Vahn only.

    // Body armor: +2/+3 are def-up/def-down, matching armor.toml udf/ldf.
    let armor = |id: u8, udf: u8, ldf: u8, mask: u8| {
        let b = table.bonus(id).unwrap_or_else(|| panic!("armor {id:#x}"));
        assert_eq!(b.def_up(), udf, "armor {id:#x} udf");
        assert_eq!(b.def_down(), ldf, "armor {id:#x} ldf");
        assert_eq!(b.attack(), 0, "armor {id:#x} no attack");
        assert_eq!(b.slot(), EquipSlot::Body, "armor {id:#x} slot");
        assert_eq!(b.equip_mask(), mask, "armor {id:#x} equip mask");
    };
    armor(0x43, 8, 7, 1); // Hunter Clothes: udf 8 / ldf 7, Vahn.
    armor(0x45, 24, 24, 1); // Warrior Armor: 24 / 24, Vahn.
    armor(0x47, 45, 45, 1); // Master Armor: 45 / 45, Vahn.

    // Head accessory (War God Band, Gala): slot = Head, mask = Gala (4).
    // Head gear carries the INT bonus (+0), never the SPD bonus (+4).
    let band = table.bonus(0x41).expect("War God Band");
    assert_eq!(band.slot(), EquipSlot::Head);
    assert_eq!(band.equip_mask(), 4, "War God Band equips Gala");
    assert_eq!(band.def_up(), 47, "War God Band def-up");
    assert_eq!(band.spd_up(), 0, "head gear sets no SPD bonus");

    // Footwear (Warrior Boots, Vahn): slot = Footwear; the +4 byte is the SPD
    // bonus (pinned via FUN_801CF5D0: equip +4 -> char record +0x118 = SPD).
    let boots = table.bonus(0x59).expect("Warrior Boots");
    assert_eq!(boots.slot(), EquipSlot::Footwear);
    assert_eq!(boots.spd_up(), 4, "Warrior Boots SPD bonus");
    assert_eq!(boots.int_up(), 0, "footwear sets no INT bonus");

    // Corpus invariant matching the slot->stat mapping: the INT bonus (+0)
    // appears only on head gear, the SPD bonus (+4) only on footwear. (Body and
    // weapon records leave both at 0.)
    for id in 0u8..=255 {
        let Some(b) = table.bonus(id) else { continue };
        if b.int_up() != 0 {
            assert_eq!(b.slot(), EquipSlot::Head, "INT bonus on non-head {id:#x}");
        }
        if b.spd_up() != 0 {
            assert_eq!(
                b.slot(),
                EquipSlot::Footwear,
                "SPD bonus on non-footwear {id:#x}"
            );
        }
    }

    // Ra-Seru gear sets the +7 bit-0 flag.
    assert!(
        table.bonus(0x1B).expect("Ra-Seru Blade").is_ra_seru(),
        "Ra-Seru Blade is a Ra-Seru equip"
    );
    assert!(
        !table.bonus(0x22).unwrap().is_ra_seru(),
        "Survival Knife is not"
    );

    // A consumable id (Healing Leaf 0x77) is not equipment -> no bonus record.
    assert!(!table.is_equipment(0x77));
    assert!(table.bonus(0x77).is_none());
}
