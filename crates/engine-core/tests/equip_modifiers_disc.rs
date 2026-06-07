//! Disc-gated: the disc-accurate equipment modifier table (built from the real
//! `SCUS_942.54` stat-bonus table) keys by real item ids and carries the
//! byte-exact attack / defense bonuses - unlike the fabricated-id vanilla
//! catalog. Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use legaia_engine_core::equipment::equip_modifier_table_from_disc;
use std::path::PathBuf;

#[test]
fn disc_modifier_table_uses_real_ids_and_stats() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let stats = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus).expect("equip table");
    let table = equip_modifier_table_from_disc(&stats);

    // Chaos Breaker (id 0x27) is a weapon: attack 72, no defense.
    let cb = table.get(0x27).expect("Chaos Breaker modifier");
    assert_eq!(cb.atk, 72);
    assert_eq!(cb.udf, 0);
    // Master Armor (id 0x47): def-up/def-down 45/45, no attack.
    let armor = table.get(0x47).expect("Master Armor modifier");
    assert_eq!(armor.atk, 0);
    assert_eq!((armor.udf, armor.ldf), (45, 45));

    // The fabricated vanilla catalog keys 0x20 as "Bronze Sword" (atk 10), but
    // the real id 0x20 is the Mace (attack 38) - proving the disc table is the
    // real id space, not the fabricated one.
    let mace = table.get(0x20).expect("real id 0x20 = Mace");
    assert_eq!(
        mace.atk, 38,
        "real 0x20 is the Mace (atk 38), not Bronze Sword"
    );

    // A consumable id carries no equipment modifier.
    assert!(table.get(0x77).is_none(), "Healing Leaf is not equipment");
}
