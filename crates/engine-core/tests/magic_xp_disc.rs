//! Disc-gated: the summon-magic XP threshold table sourced from the user's
//! `SCUS_942.54` (`magic_xp::thresholds_from_scus`, the static u16 table at
//! `0x8007656C` read by `FUN_801e70bc`). On the retail disc the table must
//! decode (8 strictly-ascending non-zero steps) and install into the World
//! so a summon cast can level its spell. Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use std::path::PathBuf;

#[test]
fn disc_magic_xp_thresholds_decode_and_install() {
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

    let table = legaia_engine_core::magic_xp::thresholds_from_scus(&scus)
        .expect("threshold table decodes from the retail SCUS");

    // Shape invariants of the retail curve (the loader enforces ascending +
    // non-zero; pin the endpoints' relation rather than the literal bytes).
    assert!(table[0] > 0, "level-1 threshold is non-zero");
    assert!(
        table.windows(2).all(|w| w[0] < w[1]),
        "strictly ascending level curve"
    );
    // The level-1 step is small enough that a handful of summon kills
    // (12 XP each) can clear it - the curve starts shallow.
    assert!(
        table[0] <= 12 * 4,
        "level-1 threshold within a few summon kills, got {}",
        table[0]
    );

    // Installs into the World (the boot-side wiring point).
    let mut world = legaia_engine_core::world::World::default();
    assert!(world.install_magic_xp_thresholds(&scus));
    assert_eq!(world.magic_xp_thresholds, Some(table));
}
