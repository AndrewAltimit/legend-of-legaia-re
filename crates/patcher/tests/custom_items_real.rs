//! Disc oracle for the **custom items** injection (Nature's Elixir / Seru
//! Tear / Delilas Tear): every fingerprint the plan validates - the three
//! empty-name item records, the spare descriptor slots, both class
//! jump-table default words, the seven cave heads, the two battle-overlay
//! hook sites, and the arena-settle grant site - is checked against the real
//! US build, then the whole injection round-trips onto a scratch copy.
//! Requires `LEGAIA_DISC_BIN`; skips (and passes) without it.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply::inject_custom_items;
use legaia_patcher::custom_items::{
    ARENA_OVERLAY_PROT_INDEX, BATTLE_OVERLAY_PROT_INDEX, CustomItemsInjection,
    DELILAS_TEAR_ITEM_ID, ELIXIR_ITEM_ID, SERU_TEAR_ITEM_ID,
};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn plan_validates_against_the_real_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let battle = patcher
        .read_entry(BATTLE_OVERLAY_PROT_INDEX)
        .expect("read battle overlay");
    let overlay = patcher
        .read_entry(ARENA_OVERLAY_PROT_INDEX)
        .expect("read arena overlay");
    let plan =
        CustomItemsInjection::plan(&scus, &battle, &overlay).expect("plan against real build");
    // 3 item records + 3 descriptors + 2 jump-table writes + 6 caves.
    assert_eq!(plan.scus.len(), 14, "SCUS write count");
    assert_eq!(plan.battle.len(), 2, "battle hook count");
    assert_eq!(plan.overlay.len(), 1, "grant hook count");
}

#[test]
fn injection_round_trips_and_is_idempotent() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    assert!(
        inject_custom_items(&mut patcher).expect("inject custom items"),
        "first application must report a change"
    );
    // The patched image parses: the three items now carry their names.
    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS in patched image");
    let names = legaia_asset::item_names::ItemNameTable::from_scus(&scus).expect("parse names");
    assert_eq!(names.name(ELIXIR_ITEM_ID), Some("Nature's Elixir"));
    assert_eq!(names.name(SERU_TEAR_ITEM_ID), Some("Seru Tear"));
    assert_eq!(names.name(DELILAS_TEAR_ITEM_ID), Some("Delilas Tear"));
    // Second application is a no-op.
    assert!(
        !inject_custom_items(&mut patcher).expect("re-inject"),
        "second application must be idempotent"
    );
}

#[test]
fn free_cast_flag_starts_clear_on_the_patched_image() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    inject_custom_items(&mut patcher).expect("inject custom items");
    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS in patched image");
    let off = file_offset_for_va(&scus, legaia_patcher::custom_items::FREECAST_FLAG_VA)
        .expect("resolve flag VA");
    assert_eq!(
        &scus[off..off + 4],
        &[0, 0, 0, 0],
        "the free-cast flag cell must ship clear"
    );
}
