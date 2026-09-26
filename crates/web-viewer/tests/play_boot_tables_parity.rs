//! Disc-gated: the browser play page boots the same static-SCUS progression
//! tables the native window does.
//!
//! The native boot installed the XP curve + Noa/Gala correction divisors, the
//! stat-growth curves, the victory-pose table, the XA cue durations, the
//! magic-XP thresholds and the accessory passives one read at a time; the
//! page's `load_disc` installed none of them, and every consumer's disc-free
//! fallback hid it (placeholder growth, summons that never level, no accessory
//! passives, silent melee grunts, a victory pose that skipped its `rand()`).
//! Both hosts now call `World::install_retail_progression_tables`; this drives
//! the page's own `load_disc` and asserts every table landed, and pairs the
//! result against the engine install over the same executable.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

#[test]
fn the_play_page_installs_the_native_boot_progression_tables() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable (disc-gated)");
        return;
    };
    let scus = legaia_web_viewer::disc::extract_scus(&bytes).expect("SCUS on the disc");

    // The native side's install, over the same executable: what the page has
    // to match. It must itself be non-vacuous.
    let mut native = legaia_engine_core::world::World::default();
    let got = native.install_retail_progression_tables(&scus);
    assert!(got.all(), "engine install decoded every table: {got:?}");

    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    let v: serde_json::Value =
        serde_json::from_str(&rt.debug_progression_tables_json()).expect("probe json");
    eprintln!("[ok] page progression tables: {v}");
    for key in [
        "xp_corrections",
        "growth",
        "victory_pose",
        "xa_cue_durations",
        "magic_xp",
    ] {
        assert_eq!(
            v[key], true,
            "page world lacks the native boot's `{key}` table"
        );
    }
    assert_eq!(
        v["accessory_passives"].as_u64(),
        Some(native.tables.accessory_passives.len() as u64),
        "page accessory-passive catalog differs from the native install"
    );
    assert!(
        !native.tables.accessory_passives.is_empty(),
        "the native accessory catalog is itself empty - the pairing is vacuous"
    );
}
