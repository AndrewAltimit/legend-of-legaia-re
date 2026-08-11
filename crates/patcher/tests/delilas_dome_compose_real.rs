//! Composition oracle for the **Delilas Challenge** dome injection: it shares
//! the verified-dead SCUS rodata gap with the other code-injection features,
//! so this pins which combinations coexist and which collide.
//!
//! - Delilas composes with the bonus-equipment drop, flee-EXP, and enemy-ally
//!   caves (disjoint gap regions).
//! - Delilas and shiny-Seru BOTH claim `0x8007AE00`, so they cannot coexist -
//!   whichever lands first makes the other's plan refuse. This is exactly the
//!   collision the CLI/web drivers resolve by precedence (the challenge wins),
//!   and the regression behind "the Delilas option vanished under a preset".
//!
//! Requires `LEGAIA_DISC_BIN`; skips (and passes) without it.

use legaia_patcher::apply;
use legaia_patcher::delilas_challenge::DelilasSites;
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn koin1_applied(image: &[u8]) -> bool {
    let patcher = DiscPatcher::open(image.to_vec()).expect("open");
    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).expect("read entry");
        if let Some(sites) = DelilasSites::locate(&entry, idx) {
            return sites.already_applied;
        }
    }
    false
}

#[test]
fn delilas_composes_with_the_other_gap_features_but_not_shiny() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Coexists with the non-shiny gap caves (bonus drop 0x8007AB80, enemy-ally
    // 0x8007ACA0, flee-EXP 0x8007AD00 - all below the dome cave at 0x8007AE00),
    // in either order relative to the challenge.
    let mut p = DiscPatcher::open(disc.clone()).expect("open");
    apply::inject_equipment_bonus_drop(&mut p, 10).expect("bonus drop");
    apply::inject_flee_exp(&mut p, 25).expect("flee exp");
    apply::inject_enemy_ally(&mut p, 20).expect("enemy ally");
    let rep = apply::apply_delilas_challenge(&mut p, true).expect("delilas after gap features");
    assert!(rep.changed && rep.dome_injected);
    assert!(
        koin1_applied(&p.into_image()),
        "delilas menu must land alongside bonus/flee/enemy-ally"
    );

    // Delilas and shiny-Seru collide at 0x8007AE00: whichever is applied first
    // makes the other refuse (they are NOT allowed to silently overwrite).
    let mut a = DiscPatcher::open(disc.clone()).expect("open");
    apply::inject_shiny_seru(&mut a, 20).expect("shiny first");
    assert!(
        apply::apply_delilas_challenge(&mut a, true).is_err(),
        "delilas must refuse when shiny already occupies the arena cave"
    );

    let mut b = DiscPatcher::open(disc).expect("open");
    apply::apply_delilas_challenge(&mut b, true).expect("delilas first");
    assert!(
        apply::inject_shiny_seru(&mut b, 20).is_err(),
        "shiny must refuse when the dome cave already occupies the arena"
    );
    // Delilas alone still landed in the delilas-first arm.
    assert!(koin1_applied(&b.into_image()));
}
