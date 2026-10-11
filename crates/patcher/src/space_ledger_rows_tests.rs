//! The menu-overlay mods write inside their space-ledger rows; checked
//! here, beside the mods, since the ledger sits in a crate below them.

use legaia_disc_patch::space_ledger::{Image, MENU_OVERLAY, Owner, REGIONS};

/// The constants the menu-overlay mods write through sit inside their
/// ledger rows, so the ledger and the modules cannot drift apart.
#[test]
fn menu_overlay_mod_constants_are_ledger_rows() {
    use legaia_arts_patch::super_art_menu::{
        MENU_DESC_END_VA, MENU_DESC_VA, MENU_RUN_END_VA, MENU_RUN_VA,
    };
    use legaia_code_hooks::seru_overlay::{RUN_C_VA, TRADE_HANDLER_END};
    let within = |s: u32, e: u32| {
        REGIONS.iter().any(|r| {
            r.image == Image::Prot(MENU_OVERLAY)
                && matches!(r.owner, Owner::Mods(_))
                && r.start_va <= s
                && e <= r.end_va
        })
    };
    assert!(within(RUN_C_VA, TRADE_HANDLER_END));
    assert!(within(MENU_RUN_VA, MENU_RUN_END_VA));
    assert!(within(MENU_DESC_VA, MENU_DESC_END_VA));
}
