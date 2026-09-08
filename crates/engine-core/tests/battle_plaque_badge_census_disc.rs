//! The battle name-plaque's **element badge** against the disc's own monster
//! archive.
//!
//! The badge is not a computation: the archive name begins with a caret plus
//! a letter and the plaque draws badge `letter - 'A'` out of the eight-record
//! strip `0x8B..=0x92`, drawing nothing at all when the name carries no
//! escape (`docs/subsystems/battle.md`, the element-badge section). The port
//! used to derive the badge from the record's `+0x1D` element byte instead,
//! which is wrong twice over: it badged every monster, and it indexed the
//! strip in element order where the caret orders it `A..H`.
//!
//! This test pins both halves off the user's own disc:
//!
//! * the **census** - only a minority of the populated records carry an
//!   escape, and the rest must resolve to `None`;
//! * the **bijection** - every carrying record's caret letter agrees with its
//!   element byte through [`ELEMENT_TO_PLAQUE_BADGE`], with zero exceptions,
//!   which is what makes the two orders substitutable *as a check* while
//!   never being substitutable as a selector;
//! * the **propagation** - `MonsterDef` carries the record's answer verbatim,
//!   so both hosts (native `play-window` HUD and the browser play page, which
//!   share `battle_hud::battle_plaque_element_badge`) draw the same badge.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::monster_archive;
use legaia_engine_core::battle_hud::{BATTLE_PLAQUE_BADGE_COUNT, ELEMENT_TO_PLAQUE_BADGE};
use legaia_engine_core::monster_catalog::monster_def_from_record;
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn plaque_badge_is_name_markup_not_the_element_byte() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    let all = monster_archive::records(&entry).expect("archive walk");
    assert!(
        all.len() > 150,
        "expected the populated roster, got {}",
        all.len()
    );

    let carrying: Vec<_> = all.iter().filter(|r| r.plaque_badge.is_some()).collect();

    // A **minority** carry a badge. The earlier selector badged all of them,
    // which is the defect this asserts against: if `carrying.len()` ever
    // reaches `all.len()`, the selector has gone back to deriving the badge.
    assert!(
        !carrying.is_empty() && carrying.len() * 2 < all.len(),
        "badge-carrying records {} of {} - expected a minority",
        carrying.len(),
        all.len()
    );

    // Every carried badge addresses the eight-record strip.
    for r in &carrying {
        let b = r.plaque_badge.expect("filtered");
        assert!(
            usize::from(b) < BATTLE_PLAQUE_BADGE_COUNT,
            "id {} badge {b} out of the {BATTLE_PLAQUE_BADGE_COUNT}-record strip",
            r.id
        );
    }

    // The caret letter is a bijection onto `+0x1D` with **zero** exceptions,
    // through the `element -> caret index` permutation. That the two orders
    // differ is the second half of the defect: an element-ordered strip index
    // disagrees with the caret for six of the eight elements.
    let mut per_badge = [0usize; BATTLE_PLAQUE_BADGE_COUNT];
    for r in &carrying {
        let badge = r.plaque_badge.expect("filtered");
        let element = usize::from(r.element);
        assert!(
            element < BATTLE_PLAQUE_BADGE_COUNT,
            "id {} element {element} out of range while carrying a badge",
            r.id
        );
        assert_eq!(
            ELEMENT_TO_PLAQUE_BADGE[element], badge,
            "id {} ({}): element {element} maps to badge {} but the name says {badge}",
            r.id, r.name, ELEMENT_TO_PLAQUE_BADGE[element]
        );
        per_badge[usize::from(badge)] += 1;
    }
    // The permutation is not the identity - exactly the reason the old
    // selector drew the wrong art for most elements.
    assert_ne!(
        ELEMENT_TO_PLAQUE_BADGE,
        [0, 1, 2, 3, 4, 5, 6, 7],
        "the caret order must not collapse onto the element order"
    );
    // Every strip record is selected by at least one monster, so no badge in
    // the strip is unreachable art.
    for (b, n) in per_badge.iter().enumerate() {
        assert!(*n > 0, "badge {b} is selected by no record");
    }

    // Names keep the escape *stripped* - the badge is the only thing the
    // caret contributes to the plaque.
    for r in &all {
        assert!(
            !r.name.starts_with('^'),
            "id {} name kept its escape: {:?}",
            r.id,
            r.name
        );
    }

    // The catalog carries the record's answer verbatim, which is what the
    // shared selector `battle_hud::battle_plaque_element_badge` reads on both
    // hosts.
    for r in &all {
        let def = monster_def_from_record(r);
        assert_eq!(
            def.plaque_badge, r.plaque_badge,
            "id {} badge lost in monster_def_from_record",
            r.id
        );
    }
}
