//! Disc oracle for the menu-window `renderer_va` -> painter dispatch.
//!
//! `engine-ui::ui_menu_window_dispatch` maps a descriptor's content-renderer VA
//! to the painter that draws it - the port of the indirect call the retail
//! window walker makes (`FUN_80031D00`, `jalr` on the live window's `+0x28`,
//! copied there from the descriptor's `+0xC`). Its unit tests build synthetic
//! descriptors, so they prove the mapping is *self*-consistent and nothing
//! more. This test takes the **disc's own table** - PROT 0899 in its as-loaded
//! form, parsed by `legaia_asset::menu_windows` - and asserts the dispatch
//! resolves exactly the windows it claims to, at the ids the retail table puts
//! them at.
//!
//! That is the half a synthetic table cannot check: a painter mapped to a VA
//! no descriptor names would look fine in a unit test and be dead in the game.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` + `extracted/PROT.DAT`.

use legaia_asset::menu_windows::{
    self, MENU_OVERLAY_BASE_VA, MENU_OVERLAY_PROT_INDEX, MENU_WINDOW_COUNT, MenuWindowTable,
};
use legaia_asset::static_overlay;
use legaia_engine_render::{CounterSource, MenuWindowPainter, menu_window_painters, painter_at};
use legaia_prot::archive::Archive;
use std::path::PathBuf;

fn extracted_file(name: &str) -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted", "../extracted", "../../extracted"] {
        let f = PathBuf::from(dir).join(name);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn menu_window_table() -> Option<MenuWindowTable> {
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
    let overlay = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    Some(menu_windows::parse(&overlay).expect("window table parses"))
}

/// Every `(id, painter)` the dispatch must find in the retail table, in id
/// order. Ids absent from this list are renderer-less list containers or
/// renderers painted by their own builder (e.g. 28's status panel).
fn expected() -> Vec<(usize, MenuWindowPainter)> {
    use MenuWindowPainter as P;
    let gold = P::Counter {
        pictogram: legaia_engine_render::COUNTER_PICTOGRAM_GOLD,
        source: CounterSource::PartyGold,
    };
    let coins = P::Counter {
        pictogram: legaia_engine_render::COUNTER_PICTOGRAM_COINS,
        source: CounterSource::CasinoCoins,
    };
    vec![
        // The five pause tabs and window 43 are the same routine.
        (0, P::TitleTab),
        (1, P::TitleTab),
        (2, P::TitleTab),
        (3, P::TitleTab),
        (4, P::TitleTab),
        (5, P::TwoLineChoicePanel),
        (6, P::LabelList),
        (7, P::CharPrompt),
        (24, P::CountPanel),
        (25, P::ActiveStatCompare),
        (31, P::AmountPrompt),
        (32, gold),
        (33, P::RecordTitleTab),
        (34, P::ItemDescription),
        (36, P::EquipTargetList),
        (37, P::SellQuantity),
        (41, P::PartyStatCompare),
        (43, P::TitleTab),
        (45, coins),
        (46, P::ChoicePanel),
    ]
}

#[test]
fn the_dispatch_resolves_the_disc_tables_own_renderers() {
    let Some(table) = menu_window_table() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    assert_eq!(table.windows.len(), MENU_WINDOW_COUNT);
    assert_eq!(
        menu_window_painters(&table),
        expected(),
        "dispatch coverage of the disc window table"
    );
}

/// The ids the native shop pass looks up must still name the painters it
/// hands content to - `painter_at` is what refuses a renderer that moved.
#[test]
fn the_shop_window_ids_resolve_to_the_painters_the_host_expects() {
    let Some(table) = menu_window_table() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let gold = MenuWindowPainter::Counter {
        pictogram: legaia_engine_render::COUNTER_PICTOGRAM_GOLD,
        source: CounterSource::PartyGold,
    };
    // 33 vendor plate / 32 purse / 34 item info / 37 sell quantity - the
    // windows the shop's open script slides in (docs/subsystems/shop.md) that
    // this crate paints.
    assert!(painter_at(&table, 33, MenuWindowPainter::RecordTitleTab).is_some());
    assert!(painter_at(&table, 32, gold).is_some());
    assert!(painter_at(&table, 34, MenuWindowPainter::ItemDescription).is_some());
    assert!(painter_at(&table, 37, MenuWindowPainter::SellQuantity).is_some());
    // And the wrong expectation is refused rather than mis-drawn.
    assert!(painter_at(&table, 34, MenuWindowPainter::SellQuantity).is_none());
}

/// Window 35, the buy-quantity readout, has **no** painter variant: its
/// content renderer `FUN_801D5510` is ported as a pens-returning kernel
/// (`engine-core::shop::shop_buy_quantity_panel`), so `painter_at` cannot
/// resolve it and both hosts guard the id with the descriptor's own
/// `renderer_va` instead.
///
/// That guard is only as good as the constant it compares against, and a
/// unit test on either host would just restate its own literal. This asserts
/// the pairing the hosts depend on against the **disc's** table: id 35 exists,
/// its renderer is that VA, its rect is drawable, and no painter claims it.
#[test]
fn window_35_is_the_pens_only_buy_quantity_renderer_both_hosts_guard_on() {
    let Some(table) = menu_window_table() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    /// The `renderer_va` both hosts filter window 35 by.
    const RENDERER_BUY_QUANTITY: u32 = 0x801D_5510;
    let d = table.window(35).expect("window 35 in the disc table");
    assert_eq!(
        d.renderer_va, RENDERER_BUY_QUANTITY,
        "window 35's content renderer moved - both hosts' buy-quantity block \
         filters on this VA and would silently stop drawing"
    );
    let r = legaia_engine_render::painter_rect(d);
    assert!(r.w > 0 && r.h > 0, "window 35 has a drawable extent");
    assert!(
        legaia_engine_render::painter_for(d).is_none(),
        "window 35 must stay outside the painter dispatch: its port returns \
         pens, not a draw list, so a painter mapping would draw it twice"
    );
}

/// Each painted window's rect is the descriptor's, so a host never needs a
/// pinned copy: the dispatch hands back the descriptor and the rect comes off
/// it. Guards against a painter being fed a rect from the wrong record.
#[test]
fn painter_rects_come_off_the_descriptor_they_resolved_from() {
    let Some(table) = menu_window_table() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    for (id, painter) in menu_window_painters(&table) {
        let (d, p) = painter_at(&table, id, painter).expect("id resolves to its own painter");
        assert_eq!(p, painter);
        let r = legaia_engine_render::painter_rect(d);
        assert_eq!((r.x, r.y, r.w, r.h), d.rect(), "window {id} rect");
        assert!(r.w > 0 && r.h > 0, "window {id} has a drawable extent");
    }
}
