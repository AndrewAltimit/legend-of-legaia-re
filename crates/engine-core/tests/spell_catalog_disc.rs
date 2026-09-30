//! Disc-gated: the Seru-magic catalog sourced from the user's `SCUS_942.54`
//! (`seru_magic_catalog_from_scus`) reads each spell's MP cost (`+3`) and target
//! shape (`+2`, decoded via the `0x02` ally / `0x20` all bits) straight from the
//! spell table. On the retail disc this must reproduce the pinned `SERU_MAGIC`
//! record byte-for-byte - which is what locks the target-byte decode. Skips
//! without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use std::path::PathBuf;

#[test]
fn disc_spell_catalog_matches_pinned_mp_and_target() {
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

    let disc = legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(&scus)
        .expect("spell table parses");

    // Every pinned Seru spell's MP + target + name read back identically from
    // the retail disc (the pinned constants were decoded from this same table).
    for s in legaia_engine_core::retail_magic::SERU_MAGIC {
        let def = disc
            .get(s.id)
            .unwrap_or_else(|| panic!("disc catalog missing {:#04x}", s.id));
        assert_eq!(def.mp_cost, s.mp, "{} MP from disc", s.name);
        assert_eq!(def.target, s.target, "{} target shape from disc", s.name);
        assert_eq!(def.name, s.name, "{:#04x} name from disc", s.id);
    }

    // Spot-check the four decoded shapes are actually present in the block
    // (so the test is non-vacuous about the target decode).
    let target_of = |id: u8| disc.get(id).map(|d| d.target);
    use legaia_engine_core::spells::SpellTarget;
    assert_eq!(
        target_of(0x81),
        Some(SpellTarget::OneEnemy),
        "Gimard one enemy"
    );
    assert_eq!(
        target_of(0x84),
        Some(SpellTarget::AllEnemies),
        "Gizam all enemies"
    );
    assert_eq!(target_of(0x83), Some(SpellTarget::OneAlly), "Vera one ally");
    assert_eq!(
        target_of(0x89),
        Some(SpellTarget::AllAllies),
        "Orb all allies"
    );
}

/// The whole player Seru block reaches the catalog, not only the pinned
/// eleven: every id the action-id cast band maps onto a cast module
/// (`0x81..=0xA0`) that the disc table names resolves, so a captured Seru past
/// Nova can be listed by the battle Magic submenu and cast.
#[test]
fn disc_spell_catalog_covers_the_whole_player_seru_block() {
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
    let table = legaia_asset::spell_names::SpellNameTable::from_scus(&scus).expect("table");
    let disc = legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(&scus)
        .expect("spell table parses");
    let mut named = 0;
    for id in 0x81..=legaia_engine_core::retail_magic::PLAYER_SERU_LAST_ID {
        let Some(e) = table.entry(id) else { continue };
        let Some(name) = e.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
            continue;
        };
        named += 1;
        let def = disc
            .get(id)
            .unwrap_or_else(|| panic!("catalog missing {id:#04x} ({name})"));
        assert_eq!(def.name, name, "{id:#04x} name");
        assert_eq!(def.mp_cost, e.mp, "{id:#04x} MP");
    }
    eprintln!("[ran] {named} named player Seru ids");
    assert!(
        named > 11,
        "the table names more than the pinned block ({named})"
    );
}
