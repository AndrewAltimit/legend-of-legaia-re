//! The battle HUD's per-phase rule against the disc's own sub-draw script
//! table.
//!
//! `engine-core::battle_hud`'s predicates (`battle_panels_visible`,
//! `battle_readout_bar_slot`, `battle_begin_tab_visible`,
//! `battle_ring_ap_plate_value`, ...) encode which retained text actors
//! retail's menu SM leaves on screen after each `ctx[+0x06]` transition.
//! Retail does not compute that: `FUN_801D388C(step)` runs record `step` of
//! `PTR_DAT_801F4D34` (battle overlay 0898 rodata), a `(placement record,
//! mode)` list, after a hard reset of the handle list. So the rule is disc
//! data, and this test decodes it off the user's disc and asserts the
//! predicates' constants against it - per step, per record, per mode.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::battle_hud::{
    SUBDRAW_STEP_COUNT, SubdrawStep, placement_record as rec, subdraw_step,
    subdraw_steps as step_of,
};
use legaia_engine_core::scene::ProtIndex;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// The loaded battle overlay image and its base VA.
fn battle_overlay() -> Option<(Vec<u8>, u32)> {
    let root = extracted_root()?;
    let index = ProtIndex::open_extracted(&root).expect("open PROT index");
    let record = legaia_asset::static_overlay::overlay_map()
        .by_label("battle_action")
        .expect("battle_action overlay record");
    let bytes = index
        .entry_bytes(record.prot_index)
        .expect("battle overlay entry bytes");
    let loaded = legaia_asset::static_overlay::as_loaded(&bytes, record).expect("load overlay");
    Some((loaded, record.base_va))
}

fn steps() -> Option<Vec<SubdrawStep>> {
    let (image, base) = battle_overlay()?;
    let out: Vec<SubdrawStep> = (0..SUBDRAW_STEP_COUNT)
        .map(|i| subdraw_step(&image, base, i).unwrap_or_else(|| panic!("step {i:#x} decodes")))
        .collect();
    Some(out)
}

/// Non-vacuity: the table has fifty steps, every one decodes, and the
/// shapes the RAM walk saw are the shapes the disc carries.
#[test]
fn every_step_decodes_and_the_table_is_not_empty() {
    let Some(steps) = steps() else { return };
    assert_eq!(steps.len(), SUBDRAW_STEP_COUNT);
    let populated = steps.iter().filter(|s| !s.pairs.is_empty()).count();
    assert!(populated >= 40, "only {populated} populated steps");
    // Step 0x0B is the one empty record.
    assert!(steps[0x0B].pairs.is_empty());
    assert_eq!(steps[0x0B].anim, 0);
    // Every record id is a placement-table index (103 initialised records)
    // and every mode is a two-bit value.
    for (i, s) in steps.iter().enumerate() {
        for &(r, m) in &s.pairs {
            assert!(r < 103, "step {i:#x} names record {r}");
            assert!(m <= 3, "step {i:#x} record {r} mode {m}");
        }
    }
}

/// The round prompt (step 0): both chips unfold and every panel record is
/// raised; no bar, no plaque, no AP plate.
#[test]
fn round_prompt_raises_the_panels_and_nothing_else() {
    let Some(steps) = steps() else { return };
    let s = &steps[step_of::ROUND_PROMPT];
    assert_eq!(s.anim, 1, "the step rebuilds the handle list");
    for p in rec::PANEL {
        assert!(s.shows(p), "panel record {p:#x} is raised");
    }
    assert!(s.shows(0x00) && s.shows(0x03), "Begin / Run unfold");
    for absent in [
        rec::BAR,
        rec::PLAQUE_BEHIND_TAB,
        rec::PLAQUE,
        rec::AP_PLATE,
        rec::BEGIN_TAB,
    ] {
        assert_eq!(
            s.mode_of(absent),
            None,
            "record {absent:#x} is not in step 0"
        );
    }
}

/// The ring (step 1): the panels park, the bar, the tab, the plaque and the
/// AP plate come up, and the magic chip is one of the four arms.
#[test]
fn the_ring_swaps_panels_for_bar_tab_plaque_and_ap_plate() {
    let Some(steps) = steps() else { return };
    let s = &steps[step_of::RING];
    assert_eq!(s.anim, 1);
    for p in rec::PANEL {
        assert_eq!(s.mode_of(p), Some(1), "panel {p:#x} parks (mode 1)");
    }
    assert!(s.shows(rec::BAR), "the bar rises");
    assert!(s.shows(rec::BEGIN_TAB), "Begin becomes the tab");
    assert!(
        s.shows(rec::PLAQUE_BEHIND_TAB),
        "the plaque drops in behind it"
    );
    assert!(s.shows(rec::AP_PLATE), "the AP plate slides in");
    assert!(s.shows(rec::MAGIC_CHIP), "the element chip unfolds");
    assert_eq!(
        s.mode_of(rec::PLAQUE),
        None,
        "the action plaque is not the ring's"
    );
}

/// Browsing the item / magic windows brings the panels back and parks the
/// bar (steps 5 and 7); their target steps do the reverse (`0x18` /
/// `0x1B`). The AP plate leaves with the ring.
#[test]
fn windows_show_panels_and_their_target_steps_show_the_bar() {
    let Some(steps) = steps() else { return };
    for (name, i) in [
        ("item window", step_of::ITEM_WINDOW),
        ("magic window", step_of::MAGIC_WINDOW),
    ] {
        let s = &steps[i];
        for p in rec::PANEL {
            assert!(s.shows(p), "{name}: panel {p:#x} is back up");
        }
        assert_eq!(s.mode_of(rec::BAR), Some(1), "{name}: the bar parks");
        assert_eq!(
            s.mode_of(rec::AP_PLATE),
            Some(1),
            "{name}: the plate leaves"
        );
        assert_eq!(
            s.mode_of(rec::PLAQUE_BEHIND_TAB),
            Some(3),
            "{name}: the plaque snaps behind the tab"
        );
    }
    let item_open = &steps[step_of::ITEM_TARGET_OPEN];
    for p in rec::PANEL {
        assert_eq!(
            item_open.mode_of(p),
            Some(1),
            "item target: panel {p:#x} parks"
        );
    }
    let item_cursor = &steps[step_of::ITEM_TARGET_CURSOR];
    assert!(
        item_cursor.shows(rec::BAR),
        "item target cursor: the bar is up"
    );
    let magic_target = &steps[step_of::MAGIC_TARGET];
    for p in rec::PANEL {
        assert_eq!(
            magic_target.mode_of(p),
            Some(1),
            "magic target: panel {p:#x} parks"
        );
    }
    assert!(magic_target.shows(rec::BAR), "magic target: the bar is up");
}

/// The attack-mode prompt and the target cursor park the bar and drop the
/// plate but keep the tab + plaque; the arts-entry screen parks the bar,
/// raises the AP bar in its seat and keeps the plate.
#[test]
fn attack_mode_target_cursor_and_arts_input_park_the_bar() {
    let Some(steps) = steps() else { return };
    for (name, i) in [
        ("attack mode", step_of::ATTACK_MODE),
        ("target cursor", step_of::TARGET_CURSOR),
    ] {
        let s = &steps[i];
        assert_eq!(s.mode_of(rec::BAR), Some(1), "{name}: the bar parks");
        assert_eq!(
            s.mode_of(rec::AP_PLATE),
            Some(1),
            "{name}: the plate leaves"
        );
        assert_eq!(s.mode_of(rec::BEGIN_TAB), Some(3), "{name}: the tab stays");
        assert_eq!(
            s.mode_of(rec::PLAQUE_BEHIND_TAB),
            Some(3),
            "{name}: plaque stays"
        );
        for p in rec::PANEL {
            assert_eq!(s.mode_of(p), None, "{name}: no panel");
        }
    }
    let arts = &steps[step_of::ARTS_INPUT];
    assert_eq!(arts.mode_of(rec::BAR), Some(1), "arts input: the bar parks");
    assert!(
        arts.shows(rec::AP_BAR),
        "arts input: the AP bar takes the seat"
    );
    assert_eq!(
        arts.mode_of(rec::AP_PLATE),
        Some(3),
        "arts input: the plate snaps"
    );
}

/// The all-committed prompt (`0x6E`, step `0x23`) keeps the tab and drops
/// the plaque, the bar, the panels and the plate - which is why the port's
/// `CommandSurface::Other` draws no party surface and no plaque.
#[test]
fn all_committed_prompt_carries_only_the_tab() {
    let Some(steps) = steps() else { return };
    let s = &steps[step_of::ALL_COMMITTED];
    assert_eq!(s.mode_of(rec::BEGIN_TAB), Some(3));
    for absent in [rec::BAR, rec::PLAQUE_BEHIND_TAB, rec::PLAQUE] {
        assert_eq!(s.mode_of(absent), None, "record {absent:#x} absent");
    }
    for p in rec::PANEL {
        assert_eq!(s.mode_of(p), None);
    }
    assert_eq!(s.mode_of(rec::AP_PLATE), Some(1));
}

/// No menu step raises the action-only records: the action plaque, the
/// move name, the combo cluster and the target plaque belong to the action
/// SM's seed arms alone.
#[test]
fn no_menu_step_opens_an_action_only_record() {
    let Some(steps) = steps() else { return };
    for (i, s) in steps.iter().enumerate() {
        for r in [rec::PLAQUE, rec::MOVE_NAME, rec::COMBO, rec::TARGET_PLAQUE] {
            assert!(!s.shows(r), "menu step {i:#x} raises action record {r:#x}");
        }
    }
}
