//! Disc-gated reproducibility for the menu-overlay window-descriptor table.
//!
//! Re-extract the menu overlay (PROT 0899) from the user's `PROT.DAT`,
//! parse the 52-entry window table out of it, and pin the structural
//! invariants + the geometry of the windows byte-matched against the
//! catalogued menu-open save-state captures (status / equipment / options).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/` files are absent.

use std::path::PathBuf;

use legaia_asset::menu_windows::{
    self, EQUIP_SCREEN_WINDOWS, MENU_OVERLAY_BASE_VA, MENU_OVERLAY_PROT_INDEX, MENU_WINDOW_COUNT,
    OPTIONS_SCREEN_WINDOWS, STATUS_SCREEN_WINDOWS, TOP_LEVEL_WINDOWS, window_ids,
};
use legaia_asset::static_overlay;
use legaia_prot::archive::Archive;

fn extracted_file(name: &str) -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted", "../../extracted"] {
        let f = PathBuf::from(dir).join(name);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn menu_overlay() -> Option<Vec<u8>> {
    let prot = extracted_file("PROT.DAT")?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(MENU_OVERLAY_PROT_INDEX as u32)
        .expect("menu overlay in static map");
    assert_eq!(rec.base_va, MENU_OVERLAY_BASE_VA, "menu overlay base");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn window_table_reproduces_the_captured_geometry() {
    let Some(overlay) = menu_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let table = menu_windows::parse(&overlay).expect("window table parses");
    assert_eq!(table.windows.len(), MENU_WINDOW_COUNT);

    // The rects RAM-matched against the six catalogued menu-open captures
    // (`menu_status_field` / `menu_equipment_field` / `menu_options_field`
    // + their town siblings): each screen's live window list carries these
    // descriptor ids at struct +0x8 with these home rects.
    let pin = |id: usize, rect: (i32, i32, i32, i32), renderer: u32| {
        let d = table.window(id).unwrap_or_else(|| panic!("window {id}"));
        assert_eq!(d.rect(), rect, "window {id} rect");
        assert_eq!(d.renderer_va, renderer, "window {id} renderer");
    };

    // Status screen (live ids 3 / 26 / 27 / 28 / 30). Window 28 is the
    // per-character panel: its renderer is the Ghidra-traced FUN_801D33D8
    // and its rect is the WX/WY origin every field-menu.md offset hangs off.
    pin(window_ids::TAB_STATUS, (12, 12, 60, 12), 0x801D_CAD8);
    pin(window_ids::STATUS_PARTY_LIST, (14, 38, 60, 38), 0x801D_2094);
    pin(window_ids::STATUS_CONDITION, (14, 92, 60, 10), 0x801D_30A4);
    pin(window_ids::STATUS_MAIN, (90, 16, 218, 188), 0x801D_33D8);
    pin(window_ids::STATUS_SUMMARY, (14, 134, 60, 70), 0x801D_31EC);

    // Equipment screen (live ids 2 / 21 / 22 / 23).
    pin(window_ids::TAB_EQUIP, (16, 12, 60, 12), 0x801D_CA94);
    pin(window_ids::EQUIP_PARTY, (14, 42, 80, 38), 0x801D_2094);
    pin(window_ids::EQUIP_MAIN, (14, 96, 292, 108), 0x801D_21C0);
    pin(window_ids::EQUIP_LIST, (174, 22, 132, 182), 0);

    // Options screen (live ids 4 / 48).
    pin(window_ids::TAB_OPTIONS, (16, 12, 60, 12), 0x801D_CB1C);
    pin(window_ids::OPTIONS_MAIN, (24, 40, 256, 148), 0x801D_CEF0);

    // Top-level pause menu (ids parked offscreen in every sub-screen
    // capture; the money-box y is 178 on disc, nudged to 180 live).
    pin(window_ids::TOP_COMMAND_LIST, (24, 24, 104, 94), 0x801C_FD68);
    pin(window_ids::TOP_MONEY_TIME, (24, 178, 104, 24), 0x801D_0148);
    pin(window_ids::TOP_INFO_PANEL, (144, 24, 152, 180), 0x801D_030C);

    // Structural invariants across the whole table.
    for (id, d) in table.windows.iter().enumerate() {
        assert!(
            d.renderer_va == 0 || (0x801C_E818..0x8020_0000).contains(&d.renderer_va),
            "window {id} renderer in overlay range"
        );
        assert!(
            matches!(d.kind, 0 | 2 | 3 | 4),
            "window {id} kind {} in the observed class set",
            d.kind
        );
    }
    // Title tabs 0..=3 are kind 2 (the options tab, id 4, is kind 3
    // despite the identical tab geometry); every screen-set id resolves.
    for id in [window_ids::TAB_EQUIP, window_ids::TAB_STATUS] {
        assert_eq!(table.window(id).unwrap().kind, 2, "tab {id} kind");
    }
    for set in [
        &STATUS_SCREEN_WINDOWS[..],
        &EQUIP_SCREEN_WINDOWS[..],
        &OPTIONS_SCREEN_WINDOWS[..],
        &TOP_LEVEL_WINDOWS[..],
    ] {
        for &id in set {
            assert!(table.window(id).is_some(), "screen-set id {id} resolves");
        }
    }
}
