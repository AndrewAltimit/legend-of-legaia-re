//! Disc-gated: applying the real on-disc item-effect descriptor table
//! ([`legaia_asset::item_effect`], `DAT_800752C0`) over the vanilla item
//! catalog corrects each consumable's field/battle usability gates to match
//! retail. Skips without `LEGAIA_DISC_BIN`.
//!
//! Notably this flips the cure/revive items to battle-only: the curated catalog
//! had Antidote/Medicine/Phoenix marked field-usable, but their on-disc flag
//! byte is `0x84` (battle bit only) - status/death only matter in battle.

use legaia_engine_core::Vfs;
use legaia_engine_core::items::ItemCatalog;
use std::path::PathBuf;

#[test]
fn effect_flags_correct_item_usability_gates() {
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
    let effects =
        legaia_asset::item_effect::ItemEffectTable::from_scus(&scus).expect("effect table parses");

    let mut catalog = ItemCatalog::vanilla();
    // Baseline: the curated catalog marks the cure/revive items field-usable.
    assert!(
        catalog.get(0x80).unwrap().usable_in_field,
        "Phoenix curated field"
    );
    catalog.apply_effect_flags(&effects);

    let gate = |id: u8| {
        let e = catalog
            .get(id)
            .unwrap_or_else(|| panic!("catalog id {id:#x}"));
        (e.usable_in_field, e.usable_in_battle)
    };
    // Healers: usable in both menus (flag 0x86).
    assert_eq!(gate(0x77), (true, true), "Healing Leaf field+battle");
    assert_eq!(gate(0x7C), (true, true), "Magic Leaf field+battle");
    // Cure / revive: battle-only (flag 0x84) - the disc correction.
    assert_eq!(gate(0x7E), (false, true), "Antidote battle-only");
    assert_eq!(gate(0x7F), (false, true), "Medicine battle-only");
    assert_eq!(gate(0x80), (false, true), "Phoenix battle-only");
    // Field utility: field-only (flag 0x02, no battle bit).
    assert_eq!(gate(0x88), (true, false), "Door of Light field-only");
}
