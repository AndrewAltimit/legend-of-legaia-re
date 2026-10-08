use super::*;
use crate::inventory_use::{InventoryContext, TargetRow};
use crate::items::ItemCatalog;
use crate::spell_menu::{CasterSlot, SpellMenuInput};
use crate::spells::SpellCatalog;

fn items_session(ids_counts: &[(u8, u8)]) -> PauseItemsSession {
    let items: Vec<u8> = ids_counts.iter().map(|(id, _)| *id).collect();
    let rows: Vec<PauseItemRow> = ids_counts
        .iter()
        .enumerate()
        .map(|(i, (id, count))| PauseItemRow {
            id: *id,
            // Test rows come from a dense list, so the row ordinal IS the
            // slot; a holed bag is exercised in `bag_row_payload_is_a_slot`.
            slot: i as u8,
            name: format!("Item {id:02X}"),
            count: *count,
            desc: format!("Desc {id:02X}"),
            passive: None,
        })
        .collect();
    let targets = vec![TargetRow::new(0, "Vahn").with_stats(50, 100, 10, 30)];
    let inner = InventoryUseSession::new(
        ItemCatalog::vanilla(),
        items,
        targets,
        InventoryContext::Field,
    );
    PauseItemsSession::new(inner, rows)
}

fn edge(b: PadButton) -> u16 {
    b.mask()
}

/// The screen opens in command focus; Cross on "Use" moves the hand
/// into the list; Circle in the list returns to the command window;
/// Circle there closes.
#[test]
fn items_focus_walk_command_list_command_close() {
    let mut s = items_session(&[(0x77, 3)]);
    assert_eq!(s.focus, PauseItemsFocus::Command);
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::List);
    s.input_pad_edge(edge(PadButton::Circle));
    assert_eq!(s.focus, PauseItemsFocus::Command);
    assert!(!s.is_done());
    s.input_pad_edge(edge(PadButton::Circle));
    assert!(s.is_done());
}

/// Throw Out draws its own row order (`FUN_80030628` content id `0x22`),
/// and the trip back restores whatever order the screen was in - not the
/// order it was built in. The distinction is the Arrange command: a player
/// who arranges, opens Throw Out and backs out must still see the arranged
/// list.
#[test]
fn throw_out_swaps_the_row_order_and_the_back_out_restores_what_it_found() {
    let mut s = items_session(&[(0x11, 1), (0x22, 1), (0x33, 1)]);
    // A Throw Out build that reverses the rows, so the swap is visible.
    s = s.with_throw_out_row_order(vec![2, 1, 0]);
    let opened: Vec<u8> = s.rows.iter().map(|r| r.slot).collect();
    assert_eq!(opened, vec![0, 1, 2]);

    // Arrange (command row 2) reorders in place; capture what it left.
    s.command_cursor = 2;
    s.input_pad_edge(edge(PadButton::Cross));
    let arranged: Vec<u8> = s.rows.iter().map(|r| r.slot).collect();

    // Throw Out (command row 1) swaps to the builder's order...
    s.command_cursor = 1;
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
    assert_eq!(
        s.rows.iter().map(|r| r.slot).collect::<Vec<_>>(),
        vec![2, 1, 0],
        "the Throw Out list draws content id 0x22's order"
    );
    // ...and backing out restores the order it found, Arrange included.
    s.input_pad_edge(edge(PadButton::Circle));
    assert_eq!(s.focus, PauseItemsFocus::Command);
    assert_eq!(
        s.rows.iter().map(|r| r.slot).collect::<Vec<_>>(),
        arranged,
        "the back-out must not undo an Arrange"
    );
}

/// An empty bag keeps the hand on the command window ("Use" refuses).
#[test]
fn items_empty_bag_refuses_list_entry() {
    let mut s = items_session(&[]);
    assert!(s.bag_empty());
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::Command);
}

/// The page total is the occupied-row count's page count, never the
/// bag capacity: one held item reads `PAGE 1/ 1` (no page-turn arrow),
/// and an empty bag reports `0`, which suppresses the header the way
/// retail's zero-count branch does.
#[test]
fn items_page_total_counts_rows_not_capacity() {
    let one = items_screen_model(&items_session(&[(0x77, 47)]));
    assert_eq!((one.page, one.pages), (1, 1));
    let twelve: Vec<(u8, u8)> = (1..=12).map(|i| (i, 1)).collect();
    assert_eq!(items_screen_model(&items_session(&twelve)).pages, 1);
    let thirteen: Vec<(u8, u8)> = (1..=13).map(|i| (i, 1)).collect();
    assert_eq!(items_screen_model(&items_session(&thirteen)).pages, 2);
    assert_eq!(items_screen_model(&items_session(&[])).pages, 0);
}

/// Left/Right flip 12-row pages over the bag; the model slices the
/// visible page and reports `ceil(rows / 12)` as the total.
#[test]
fn items_page_flip_and_model_slice() {
    let rows: Vec<(u8, u8)> = (1..=30).map(|i| (i, 1)).collect();
    let mut s = items_session(&rows);
    s.input_pad_edge(edge(PadButton::Cross)); // into the list
    let m = items_screen_model(&s);
    assert_eq!(m.page, 1);
    assert_eq!(m.pages, 3);
    assert_eq!(m.page_rows.len(), LIST_PAGE_ROWS);
    assert!(m.focus_list);

    s.input_pad_edge(edge(PadButton::Right));
    let m = items_screen_model(&s);
    assert_eq!(m.page, 2);
    assert_eq!(m.list_cursor_on_page, 0);
    // Page 3 holds the remaining 6 rows.
    s.input_pad_edge(edge(PadButton::Right));
    let m = items_screen_model(&s);
    assert_eq!(m.page, 3);
    assert_eq!(m.page_rows.len(), 6);
    // Clamped at the last row; Left returns.
    s.input_pad_edge(edge(PadButton::Left));
    let m = items_screen_model(&s);
    assert_eq!(m.page, 2);
}

/// Throw Out walk (FUN_801D8734): command row 1 enters the discard
/// list; Cross opens the confirm seeded on "No"; confirming "No"
/// returns to the list; confirming "Yes" discards the whole stack,
/// records it on the inner session and returns to the list.
#[test]
fn items_throw_out_confirm_defaults_no_and_discards_stack() {
    let mut s = items_session(&[(0x77, 3), (0x78, 2)]);
    s.input_pad_edge(edge(PadButton::Down)); // -> Throw Out
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::ThrowOutConfirm);
    assert_eq!(s.confirm_cursor, 1, "retail seeds the confirm on No");
    // Confirm "No": nothing discarded, back to the list.
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
    assert_eq!(s.rows.len(), 2);
    // Re-open, toggle to "Yes", confirm: stack 0x77 goes.
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Up));
    assert_eq!(s.confirm_cursor, 0);
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
    assert_eq!(s.rows.len(), 1);
    assert_eq!(s.rows[0].id, 0x78);
    assert_eq!(s.inner.thrown_items, vec![0x77]);
    assert_eq!(s.inner.items, vec![0x78]);
}

/// The throw-out view model stages the confirm window content, and
/// the confirm phases keep the list focus (grey rows).
#[test]
fn items_throw_confirm_model_content() {
    let mut s = items_session(&[(0x77, 12)]);
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Cross));
    let m = items_screen_model(&s);
    assert!(m.focus_list);
    assert!(m.throw_confirm.is_none());
    s.input_pad_edge(edge(PadButton::Cross));
    let m = items_screen_model(&s);
    let confirm = m.throw_confirm.expect("confirm open");
    assert_eq!(confirm.name, "Item 77");
    assert_eq!(confirm.count, 12);
    assert_eq!(confirm.cursor, 1);
    assert!(m.focus_list);
}

/// Discarding the last remaining stack drops the hand back onto the
/// command window (the retail bag rescan finds nothing and returns
/// to submenu 5); discarding the last *row* steps the hand back.
#[test]
fn items_throw_out_empties_bag_back_to_command() {
    let mut s = items_session(&[(0x77, 1), (0x78, 1)]);
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Cross));
    // Hand on the last row.
    s.input_pad_edge(edge(PadButton::Down));
    assert_eq!(s.list_cursor(), 1);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Up)); // Yes
    s.input_pad_edge(edge(PadButton::Cross));
    // Last-row fix-up: the hand stepped back onto the remaining row.
    assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
    assert_eq!(s.list_cursor(), 0);
    // Discard the final stack: back to the command window.
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Up));
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::Command);
    assert!(s.bag_empty());
    assert_eq!(s.inner.thrown_items, vec![0x78, 0x77]);
    assert!(!s.is_done(), "the screen stays open on the command window");
}

/// Circle backs out of the confirm and out of the throw-out list
/// without discarding.
#[test]
fn items_throw_out_circle_backs_out() {
    let mut s = items_session(&[(0x77, 3)]);
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Circle));
    assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
    s.input_pad_edge(edge(PadButton::Circle));
    assert_eq!(s.focus, PauseItemsFocus::Command);
    assert!(s.inner.thrown_items.is_empty());
    assert_eq!(s.rows.len(), 1);
}

/// Arrange (FUN_801D64A8): rows re-sort by the rank table and the
/// list scroll resets; the inner id list stays parallel.
#[test]
fn items_arrange_sorts_rows_by_rank_table() {
    use crate::menu_arrange::ArrangeRankTable;
    let mut s = items_session(&[(0x10, 1), (0x20, 2), (0x30, 3)]);
    // Rank order reverses the id order: 0x30 first, 0x10 last.
    let mut order = [0u8; 0x100];
    order[0] = 0x30;
    order[1] = 0x20;
    order[2] = 0x10;
    s = s.with_arrange_rank(Some(ArrangeRankTable::from_display_order(&order)));
    // Park the hand mid-list first (via Use focus), then back out and
    // Arrange: the cursor resets to the top.
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Circle));
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Down)); // -> Arrange
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::Command);
    let ids: Vec<u8> = s.rows.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![0x30, 0x20, 0x10]);
    assert_eq!(s.inner.items, ids);
    assert_eq!(s.list_cursor(), 0, "retail zeroes the list scroll");
}

/// An empty bag buzzes every command row (the FUN_801D7C00 bag scan
/// gates the dispatch, not just "Use").
#[test]
fn items_empty_bag_refuses_throw_and_arrange() {
    let mut s = items_session(&[]);
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::Command);
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::Command);
}

/// The info model carries the hovered row's real count + description.
#[test]
fn items_info_follows_hovered_row() {
    let mut s = items_session(&[(0x77, 9), (0x78, 2)]);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Down));
    let m = items_screen_model(&s);
    let info = m.info.expect("hovered row staged");
    assert_eq!(info.name, "Item 78");
    assert_eq!(info.count, 2);
    assert_eq!(info.desc, "Desc 78");
}

fn magic_session() -> SpellMenuSession {
    let party = vec![
        CasterSlot {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            mp: 30,
            hp_max: 100,
            mp_max: 120,
            level: 7,
            spells: vec![0x81, 0x9c],
            spell_levels: vec![2, 1],
            ability_bits: 0,
            ra_seru_missing: false,
        },
        CasterSlot {
            slot: 1,
            name: "Noa".into(),
            hp: 50,
            mp: 40,
            hp_max: 90,
            mp_max: 80,
            level: 6,
            spells: vec![0x83],
            spell_levels: vec![3],
            ability_bits: 0,
            ra_seru_missing: false,
        },
    ];
    let targets = vec![crate::spell_menu::TargetRow {
        slot: 0,
        name: "Vahn".into(),
        hp: 60,
        hp_max: 100,
    }];
    SpellMenuSession::new(party, targets, SpellCatalog::vanilla())
}

/// Caster focus: mp/mp_max plumb through; the hovered caster's list
/// previews white (focus_list = false) with no staged info.
#[test]
fn magic_model_caster_focus_carries_mp_max() {
    let s = magic_session();
    let m = magic_screen_model(&s, None);
    assert!(!m.focus_list);
    assert_eq!(m.casters.len(), 2);
    assert_eq!(m.casters[0], ("Vahn".to_string(), 7, 30, 120));
    assert_eq!(m.casters[1].3, 80);
    assert!(m.info.is_none());
    assert_eq!(m.page_rows.len(), 2);
    assert_eq!((m.page, m.pages), (1, 1));
}

/// A caster with no spells has a zero-row list, which retail's list
/// kernel draws with no PAGE header at all: the page total is `0`.
#[test]
fn magic_model_empty_list_has_no_page_total() {
    let party = vec![CasterSlot {
        slot: 0,
        name: "Gala".into(),
        hp: 60,
        mp: 30,
        hp_max: 100,
        mp_max: 120,
        level: 7,
        ..Default::default()
    }];
    let s = SpellMenuSession::new(party, Vec::new(), SpellCatalog::vanilla());
    let m = magic_screen_model(&s, None);
    assert_eq!(m.pages, 0);
    assert!(m.page_rows.is_empty());
}

/// List focus: rows grey (focus_list), the hovered spell stages into
/// the info window with its learned level; Ra-Seru ids flag the wider
/// icon.
#[test]
fn magic_model_list_focus_stages_info() {
    let mut s = magic_session();
    let _ = s.tick(SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), SpellMenuPhase::SpellSelect { .. }));
    let m = magic_screen_model(&s, None);
    assert!(m.focus_list);
    let info = m.info.expect("hovered spell staged");
    assert_eq!(info.level, 2);
    assert!(!info.ra_seru);
    // Row 1 (0x9c = Horn) is in the Ra-Seru block.
    assert!(m.page_rows[1].1);
    let _ = s.tick(SpellMenuInput {
        down: true,
        ..Default::default()
    });
    let m = magic_screen_model(&s, None);
    let info = m.info.expect("hovered spell staged");
    assert!(info.ra_seru);
    assert_eq!(info.level, 1);
}

/// Description + name fall back through the MenuTextTables when the
/// catalog has no entry.
#[test]
fn magic_model_desc_resolves_through_text_tables() {
    let mut s = magic_session();
    let _ = s.tick(SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    let mut entries = vec![legaia_asset::spell_names::SpellEntry::default(); 0x82];
    entries[0x81].desc = Some("Crazy Driver\nAttack enemies.".to_string());
    let text = MenuTextTables {
        spell_names: Some(legaia_asset::spell_names::SpellNameTable::from_entries(
            entries,
        )),
        ..Default::default()
    };
    let m = magic_screen_model(&s, Some(&text));
    let info = m.info.expect("hovered spell staged");
    assert_eq!(info.desc, "Crazy Driver\nAttack enemies.");
}

/// PIN: the Magic screen's displayed MP cost is discounted through the
/// per-caster MP-cost kernel (`FUN_80035394`). A caster with the half-MP
/// ability bit (`0x20`) shows half cost; the quarter bit (`0x10`) shows a
/// quarter shaved off; both set = half wins; no bits = full cost.
fn staged_mp_cost(ability_bits: u32) -> u16 {
    let mut catalog = SpellCatalog::new();
    catalog.insert(crate::spells::SpellDef {
        id: 0x81,
        name: "Costly".into(),
        mp_cost: 40,
        ..Default::default()
    });
    let party = vec![CasterSlot {
        slot: 0,
        name: "Vahn".into(),
        hp: 60,
        mp: 120,
        hp_max: 100,
        mp_max: 120,
        level: 7,
        spells: vec![0x81],
        spell_levels: vec![1],
        ability_bits,
        ra_seru_missing: false,
    }];
    let targets = vec![crate::spell_menu::TargetRow {
        slot: 0,
        name: "Vahn".into(),
        hp: 60,
        hp_max: 100,
    }];
    let mut s = SpellMenuSession::new(party, targets, catalog);
    // Enter the spell list so the hovered row stages into the info window.
    let _ = s.tick(SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    assert!(matches!(s.phase(), SpellMenuPhase::SpellSelect { .. }));
    magic_screen_model(&s, None)
        .info
        .expect("hovered spell staged")
        .mp_cost
}

/// The kind-4 list kernel's pad decode (FUN_80032A44): Up/Down wrap
/// within the visible page, Left/Right are the only scroll.
#[test]
fn list_kernel_navigate_page_local_wrap() {
    let n = 30; // pages: 0..12, 12..24, 24..30
    let up = edge(PadButton::Up);
    let down = edge(PadButton::Down);
    let left = edge(PadButton::Left);
    let right = edge(PadButton::Right);
    // Up above the page top steps back one row.
    assert_eq!(list_kernel_navigate(13, n, up), 12);
    // Up at a page top wraps to that page's last row.
    assert_eq!(list_kernel_navigate(12, n, up), 23);
    // ...clamped to the row count on the last partial page.
    assert_eq!(list_kernel_navigate(24, n, up), 29);
    // Down steps forward; past the page bottom wraps to the page top.
    assert_eq!(list_kernel_navigate(10, n, down), 11);
    assert_eq!(list_kernel_navigate(11, n, down), 0);
    // Down past the last row wraps to the last page's top.
    assert_eq!(list_kernel_navigate(29, n, down), 24);
    // Left only pages while scrolled; Right only while rows remain.
    assert_eq!(list_kernel_navigate(5, n, left), 5);
    assert_eq!(list_kernel_navigate(17, n, left), 5);
    assert_eq!(list_kernel_navigate(5, n, right), 17);
    assert_eq!(list_kernel_navigate(26, n, right), 26);
    // Right clamps the selection to the last row.
    assert_eq!(list_kernel_navigate(23, n, right), 29);
    // Empty list is inert.
    assert_eq!(list_kernel_navigate(0, 0, down), 0);
}

/// FUN_801D7E50 phase-2 dispatch: classes 0x80..0x82 take the
/// dedicated routes, flag bit 0x20 picks the all-party apply.
#[test]
fn use_route_dispatch_matches_retail() {
    assert_eq!(use_route_for_effect(0x80, 0x82), UseRoute::DoorOfLight);
    assert_eq!(use_route_for_effect(0x81, 0x82), UseRoute::DoorOfWind);
    assert_eq!(use_route_for_effect(0x82, 0x82), UseRoute::Incense);
    assert_eq!(use_route_for_effect(0x00, 0xA2), UseRoute::ApplyAll);
    assert_eq!(use_route_for_effect(0x00, 0x82), UseRoute::ApplySingle);
    assert_eq!(use_route_for_effect(0x06, 0x86), UseRoute::ApplySingle);
}

/// FUN_801D6A54: only kind-2 items with effect class 6 preview;
/// args 0/5 share the HP/MP panel, 1..=4 map onto modes 2..=5.
#[test]
fn target_panel_mode_matches_retail_map() {
    assert_eq!(target_panel_mode(2, 6, 0), 1); // Life Water
    assert_eq!(target_panel_mode(2, 6, 5), 1); // Magic Water
    assert_eq!(target_panel_mode(2, 6, 1), 2); // Power Water
    assert_eq!(target_panel_mode(2, 6, 2), 3); // Guardian Water
    assert_eq!(target_panel_mode(2, 6, 3), 4); // Swift Water
    assert_eq!(target_panel_mode(2, 6, 4), 5); // Wisdom Water
    assert_eq!(target_panel_mode(2, 6, 6), 0);
    assert_eq!(target_panel_mode(2, 0, 0), 0); // healing item
    assert_eq!(target_panel_mode(0, 6, 0), 0); // wrong kind byte
}

/// **All three** special routes map here. The earlier reading dropped
/// Door of Wind because its screen is a destination *list* rather than
/// a Yes/No window - but `FUN_801D7E50`'s phase-2 dispatch
/// (`801d7f80..801d7fd8`) branches all three effect classes out of the
/// target-panel flow at the same place, and it is
/// [`SpecialUseSession::new`] that picks the screen shape. Filtering
/// `0x81` out here is what made submenu `0xC` unreachable: with no
/// route, a Door of Wind confirm fell through to the ordinary use flow,
/// where the item is not even in the catalog, and the press did nothing
/// at all.
#[test]
fn all_three_special_routes_map_to_a_route() {
    assert_eq!(
        special_use_route_for_item(DOOR_OF_LIGHT_ITEM_ID),
        Some(UseRoute::DoorOfLight)
    );
    assert_eq!(
        special_use_route_for_item(INCENSE_ITEM_ID),
        Some(UseRoute::Incense)
    );
    assert_eq!(
        special_use_route_for_item(DOOR_OF_WIND_ITEM_ID),
        Some(UseRoute::DoorOfWind)
    );
    assert_eq!(special_use_route_for_item(0x01), None);
    // The screen shape still splits two ways: only the Yes/No routes
    // open in `Confirm`.
    for (id, phase) in [
        (DOOR_OF_LIGHT_ITEM_ID, SpecialUsePhase::Confirm),
        (INCENSE_ITEM_ID, SpecialUsePhase::Confirm),
        (DOOR_OF_WIND_ITEM_ID, SpecialUsePhase::PickDestination),
    ] {
        let route = special_use_route_for_item(id).expect("route");
        assert_eq!(SpecialUseSession::new(route, vec![]).phase, phase);
    }
}

/// Confirming a Door of Light in the Use list opens the route's own
/// confirm window instead of the target panel, and the confirm seeds
/// to **Yes** - the opposite default from the Throw Out confirm.
#[test]
fn use_list_confirm_on_door_of_light_opens_the_special_confirm() {
    let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 1)]);
    s.input_pad_edge(edge(PadButton::Cross)); // Use -> list
    assert_eq!(s.focus, PauseItemsFocus::List);
    s.input_pad_edge(edge(PadButton::Cross)); // confirm the row
    assert_eq!(s.focus, PauseItemsFocus::SpecialRoute);
    assert!(!s.target_select(), "the target panel must not open");
    let sp = s.special_use().expect("route session");
    assert_eq!(sp.route, UseRoute::DoorOfLight);
    assert_eq!(sp.cursor, 0, "seeded on Yes");
    let model = items_screen_model(&s);
    let sc = model.special_confirm.expect("confirm model");
    assert_eq!(sc.route, UseRoute::DoorOfLight);
    assert_eq!(sc.cursor, 0);
}

/// Yes on the Door of Light closes the whole menu (retail hands the
/// field exit code 4); Yes on an Incense applies in place and drops
/// back to the Use list, as does a cancel.
#[test]
fn special_confirm_outcomes_route_back_the_way_retail_does() {
    let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 1)]);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross)); // Yes
    assert!(s.is_done());
    assert_eq!(
        s.special_use().and_then(|sp| sp.exit_code()),
        Some(MENU_EXIT_CODE_FIELD_ESCAPE)
    );
    assert_eq!(
        s.special_use().and_then(|sp| sp.consumed_item_id()),
        Some(DOOR_OF_LIGHT_ITEM_ID)
    );

    let mut s = items_session(&[(INCENSE_ITEM_ID, 1)]);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross)); // Yes
    assert!(!s.is_done(), "Incense stays on the Items screen");
    assert_eq!(s.focus, PauseItemsFocus::List);
    assert_eq!(
        s.take_special_use().and_then(|sp| sp.consumed_item_id()),
        Some(INCENSE_ITEM_ID)
    );

    let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 1)]);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Circle)); // cancel
    assert!(!s.is_done());
    assert_eq!(s.focus, PauseItemsFocus::List);
    assert_eq!(s.special_use().and_then(|sp| sp.consumed_item_id()), None);
}

/// A Door of **Wind** confirm opens the destination list, not a Yes/No
/// window - and the Items screen model projects the landmarks through
/// the shared list channel with no confirm prompt attached.
#[test]
fn door_of_wind_opens_the_destination_list_not_a_confirm() {
    let towns = vec![
        crate::pause_screens::WarpDestination {
            record_index: 0,
            name: "Rim Elm".into(),
            scene_id: 0x0055,
            menu_x: 0x60,
            menu_y: 0x19,
        },
        crate::pause_screens::WarpDestination {
            record_index: 4,
            name: "Drake Castle".into(),
            scene_id: 0x0162,
            menu_x: 0x36,
            menu_y: 0x3E,
        },
    ];
    let mut s = items_session(&[(DOOR_OF_WIND_ITEM_ID, 1)]).with_warp_destinations(towns);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::SpecialRoute);
    let sp = s.special_use().expect("route session");
    assert_eq!(sp.route, UseRoute::DoorOfWind);
    assert_eq!(sp.phase, SpecialUsePhase::PickDestination);
    assert_eq!(sp.landmarks, vec!["Rim Elm", "Drake Castle"]);
    let m = items_screen_model(&s);
    assert!(
        m.special_confirm.is_none(),
        "no Yes/No prompt on this route"
    );
    assert!(m.info.is_none(), "the info window stays closed");
    assert_eq!(
        m.page_rows
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec!["Rim Elm", "Drake Castle"]
    );
    assert_eq!(m.list_cursor_on_page, 0);
    assert_eq!((m.page, m.pages), (1, 1));

    // Picking the second row stages that record's triple and hands the
    // menu the world-map warp exit code, consuming exactly one 0x89.
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Cross));
    assert!(s.is_done());
    assert_eq!(s.exit_code(), Some(MENU_EXIT_CODE_WORLD_MAP_WARP));
    assert_eq!(
        s.staged_warp(),
        Some(crate::pause_screens::StagedWarp {
            scene_id: 0x0162,
            menu_x: 0x36,
            menu_y: 0x3E,
        })
    );
    assert_eq!(s.inner.consumed_items, vec![DOOR_OF_WIND_ITEM_ID]);
}

/// The one-shot commit guard: a host that keeps ticking a finished
/// screen before it notices `is_done` must not consume a second copy.
#[test]
fn a_committed_special_route_consumes_exactly_once() {
    let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 3)]);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross)); // Yes
    for _ in 0..5 {
        s.input_pad_edge(edge(PadButton::Cross));
    }
    assert_eq!(s.inner.consumed_items, vec![DOOR_OF_LIGHT_ITEM_ID]);
}

/// Door of Light (FUN_801D8A58): Yes/No confirm seeded on Yes;
/// confirming Yes consumes 0x88 and exits with code 4; "No" and
/// Circle cancel without consuming.
#[test]
fn special_use_door_of_light_confirm() {
    let mut s = SpecialUseSession::new(UseRoute::DoorOfLight, vec![]);
    assert_eq!(s.phase, SpecialUsePhase::Confirm);
    assert_eq!(s.cursor, 0, "retail seeds the confirm on Yes");
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(
        s.phase,
        SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape)
    );
    assert_eq!(s.consumed_item_id(), Some(DOOR_OF_LIGHT_ITEM_ID));
    assert_eq!(s.exit_code(), Some(MENU_EXIT_CODE_FIELD_ESCAPE));

    let mut s = SpecialUseSession::new(UseRoute::DoorOfLight, vec![]);
    s.input_pad_edge(edge(PadButton::Down)); // -> No
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.phase, SpecialUsePhase::Done(SpecialUseOutcome::Cancelled));
    assert_eq!(s.consumed_item_id(), None);
    assert_eq!(s.exit_code(), None);
}

/// Incense (FUN_801D8D94): Yes consumes 0x8A and applies the
/// encounter suppression without exiting the menu.
#[test]
fn special_use_incense_confirm() {
    let mut s = SpecialUseSession::new(UseRoute::Incense, vec![]);
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(
        s.phase,
        SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress)
    );
    assert_eq!(s.consumed_item_id(), Some(INCENSE_ITEM_ID));
    assert_eq!(s.exit_code(), None, "Incense drops back to the Use list");
}

/// One Incense is one confirm: the route takes the copy off the row at
/// commit (`FUN_80042310(0x8A, 1)` at `0x801D8E68`), so the last copy's
/// row leaves the list and a second Cross cannot re-open its confirm.
#[test]
fn incense_last_copy_cannot_be_confirmed_twice() {
    let mut s = items_session(&[(0x01, 1), (INCENSE_ITEM_ID, 1)]);
    s.input_pad_edge(edge(PadButton::Cross)); // Use
    s.input_pad_edge(edge(PadButton::Down)); // Incense row
    s.input_pad_edge(edge(PadButton::Cross)); // open confirm
    s.input_pad_edge(edge(PadButton::Cross)); // Yes
    assert_eq!(s.incense_uses(), 1);
    assert!(s.rows.iter().all(|r| r.id != INCENSE_ITEM_ID));
    assert_eq!(s.inner.items, vec![0x01]);
    // Cursor clamps onto the remaining row; a further Cross on it is an
    // ordinary use, never another Incense.
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.incense_uses(), 1);
    assert_eq!(s.inner.consumed_items, vec![INCENSE_ITEM_ID]);
}

/// Two copies: the first confirm leaves the row at one, and the gate
/// follows the window the confirms built up inside the screen.
#[test]
fn incense_row_counts_down_and_gate_tracks_the_window() {
    let mut s = items_session(&[(INCENSE_ITEM_ID, 5)]).with_incense_window(0x80);
    s.input_pad_edge(edge(PadButton::Cross)); // Use
    s.input_pad_edge(edge(PadButton::Cross)); // open confirm
    s.input_pad_edge(edge(PadButton::Cross)); // Yes: window 0xC0
    assert_eq!(s.rows[0].count, 4);
    s.input_pad_edge(edge(PadButton::Cross));
    s.input_pad_edge(edge(PadButton::Cross)); // Yes: window 0x100
    assert_eq!(s.incense_uses(), 2);
    assert_eq!(s.rows[0].count, 3);
    // 0x100 >= 0xE0: the row is greyed, its Cross is a buzz.
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(s.focus, PauseItemsFocus::List);
    assert_eq!(s.incense_uses(), 2);
}

/// Door of Wind (FUN_801D8B90): the destination list opens directly;
/// a pick consumes 0x89 and exits with the world-map warp code;
/// Circle cancels back to the Use list.
#[test]
fn special_use_door_of_wind_pick() {
    let towns = vec!["Rim Elm".to_string(), "Drake Castle".to_string()];
    let mut s = SpecialUseSession::new(UseRoute::DoorOfWind, towns.clone());
    assert_eq!(s.phase, SpecialUsePhase::PickDestination);
    s.input_pad_edge(edge(PadButton::Down));
    s.input_pad_edge(edge(PadButton::Cross));
    assert_eq!(
        s.phase,
        SpecialUsePhase::Done(SpecialUseOutcome::Warp { landmark: 1 })
    );
    assert_eq!(s.consumed_item_id(), Some(DOOR_OF_WIND_ITEM_ID));
    assert_eq!(s.exit_code(), Some(MENU_EXIT_CODE_WORLD_MAP_WARP));

    let mut s = SpecialUseSession::new(UseRoute::DoorOfWind, towns);
    s.input_pad_edge(edge(PadButton::Circle));
    assert_eq!(s.phase, SpecialUsePhase::Done(SpecialUseOutcome::Cancelled));
}

/// The target-panel model assembles from the inner flow's target
/// rows while (and only while) the use flow is in target select.
#[test]
fn target_panel_model_from_target_select() {
    let mut s = items_session(&[(0x77, 3)]);
    assert!(target_panel_model(&s, 0).is_none());
    s.input_pad_edge(edge(PadButton::Cross)); // -> list
    s.input_pad_edge(edge(PadButton::Cross)); // confirm -> target select
    assert!(s.target_select());
    let m = target_panel_model(&s, 1).expect("target select stages the panel");
    assert_eq!(m.mode, 1);
    assert_eq!(m.members.len(), 1);
    assert_eq!(m.members[0].name, "Vahn");
    assert_eq!(m.members[0].hp, 50);
    assert_eq!(m.members[0].hp_max, 100);
    assert!(!m.all_targets);
}

/// The host entry point resolves the staged bag id and, without a disc
/// item-effect table, falls back to the plain (mode 0) panel while
/// still filling the per-member record fields from the live roster.
#[test]
fn target_panel_view_model_fills_record_fields() {
    let mut s = items_session(&[(0x77, 3)]);
    let mut world = crate::world::World::new();
    assert!(target_panel_view_model(&s, &world).is_none());
    s.input_pad_edge(edge(PadButton::Cross)); // -> list
    s.input_pad_edge(edge(PadButton::Cross)); // confirm -> target select
    assert_eq!(staged_use_item_id(&s), Some(0x77));

    // Slot 0 of the roster is the target row's record.
    let mut rec = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
    let mut base = rec.record_stats();
    base.hp_max = 111;
    base.mp_max = 22;
    base.atk = 33;
    base.udf = 34;
    base.ldf = 35;
    base.spd = 36;
    base.int = 37;
    rec.set_record_stats(base);
    let mut live = rec.live_stats();
    live.atk = 43;
    live.udf = 44;
    live.ldf = 45;
    live.spd = 46;
    live.int = 47;
    rec.set_live_stats(live);
    world.party.roster.members = vec![rec];

    let m = target_panel_view_model(&s, &world).expect("target select stages the panel");
    // No disc effect table on this world - the plain panel.
    assert_eq!(m.mode, 0);
    assert_eq!(m.members.len(), 1);
    assert_eq!(m.members[0].base_hp_max, 111);
    assert_eq!(m.members[0].base_mp_max, 22);
    assert_eq!(m.members[0].stat_eff, [43, 44, 45, 46, 47]);
    assert_eq!(m.members[0].stat_base, [33, 34, 35, 36, 37]);
}

#[test]
fn magic_model_displays_per_caster_discounted_mp_cost() {
    // No ability bits: full base cost.
    assert_eq!(staged_mp_cost(0x00), 40);
    // Half-MP bit (0x20): cost - (cost >> 1) = 20.
    assert_eq!(staged_mp_cost(0x20), 20);
    // Quarter bit (0x10): cost - (cost >> 2) = 30 (shaves 25%, not "to a quarter").
    assert_eq!(staged_mp_cost(0x10), 30);
    // Both bits set: Half (0x20) wins the priority - 20, not 30.
    assert_eq!(staged_mp_cost(0x30), 20);
}

#[test]
fn notify_window_operands_and_pens() {
    let n = notify_window_operands((12, 30), 2, 5);
    assert_eq!(n.c1_operand, 2);
    // base + selector * 0x40, truncated to a byte by the retail `sb`.
    assert_eq!(n.c5_operand, 5 + 2 * 0x40);
    assert_eq!(n.text_pen, (12, 30));
    assert_eq!(n.cursor_pen, (12 + 0xE6, 30 + 0xD));
    // selector 4 * 0x40 = 0x100 wraps to 0 in the byte store.
    assert_eq!(notify_window_operands((0, 0), 4, 7).c5_operand, 7);
}

#[test]
fn root_menu_routes_the_five_unconditional_rows() {
    for (row, want) in [(0u16, 0x05u8), (1, 0x0E), (2, 0x12), (3, 0x15), (4, 0x17)] {
        assert_eq!(
            root_menu_confirm_route(row, None, false),
            RootMenuRoute::Sub(want)
        );
    }
    assert_eq!(root_menu_confirm_route(7, None, true), RootMenuRoute::None);
}

#[test]
fn root_menu_load_row_is_gated_on_the_context_kind_not_its_presence() {
    // No context at all: Load is available.
    assert_eq!(
        root_menu_confirm_route(5, None, false),
        RootMenuRoute::Sub(0x18)
    );
    // A context of some other kind: still available.
    assert_eq!(
        root_menu_confirm_route(5, Some(0x07), false),
        RootMenuRoute::Sub(0x18)
    );
    // The locked kind: buzz.
    assert_eq!(
        root_menu_confirm_route(5, Some(ROOT_MENU_CONTEXT_LOCKED), false),
        RootMenuRoute::Buzz
    );
}

/// Row 5 is `@Load` and row 6 is `@Save`, not the other way round, and
/// the two gates therefore attach to the rows the labels name. The
/// evidence is the menu overlay's own string pool: `FUN_801CFD68` hands
/// the string primitive `0x801CEA00` for row 5 and `0x801CEA08` for row
/// 6, and those cells hold `@Load` and `@Save`. Pinning it here keeps a
/// future edit from re-swapping the two gates back.
#[test]
fn the_gated_rows_are_load_then_save_in_that_order() {
    // The scene forbids saving; nothing is parked. Load is offered and
    // Save buzzes - the shape a no-save scene has to produce.
    assert_eq!(
        root_menu_confirm_route(5, None, false),
        RootMenuRoute::Sub(0x18)
    );
    assert_eq!(root_menu_confirm_route(6, None, false), RootMenuRoute::Buzz);
    // A parked script context flips it: Save is fine, Load is refused,
    // and leaving asks first.
    assert_eq!(
        root_menu_confirm_route(5, Some(ROOT_MENU_CONTEXT_LOCKED), true),
        RootMenuRoute::Buzz
    );
    assert_eq!(
        root_menu_confirm_route(6, Some(ROOT_MENU_CONTEXT_LOCKED), true),
        RootMenuRoute::Sub(0x19)
    );
    assert_eq!(root_menu_cancel_route(Some(ROOT_MENU_CONTEXT_LOCKED)), 3);
}

#[test]
fn root_menu_save_row_needs_the_scene_save_allow_flag() {
    assert_eq!(root_menu_confirm_route(6, None, false), RootMenuRoute::Buzz);
    assert_eq!(
        root_menu_confirm_route(6, None, true),
        RootMenuRoute::Sub(0x19)
    );
}

#[test]
fn root_menu_cancel_asks_first_under_the_locked_context() {
    assert_eq!(root_menu_cancel_route(None), 0);
    assert_eq!(root_menu_cancel_route(Some(0x01)), 0);
    assert_eq!(root_menu_cancel_route(Some(ROOT_MENU_CONTEXT_LOCKED)), 3);
}

/// The menu block stops at the fifth equip byte. An item sitting in slot
/// `5` is inside `StatRecord::equip` and inside the equipment table, so
/// only the slot bound can keep it out of the block - which is the one
/// way a walk widened to the battle aggregator's eight would show.
#[test]
fn menu_stat_block_sums_five_equip_slots_not_eight() {
    use crate::battle_stats::{EquipmentTable, ItemModifier, StatRecord};
    let mut table = EquipmentTable::new();
    table.set(
        0x11,
        ItemModifier {
            atk: 7,
            ..Default::default()
        },
    );
    let mut record = StatRecord {
        base_attack: 10,
        base_accuracy: 33,
        ..Default::default()
    };

    record.equip[4] = 0x11;
    let inside = menu_stat_block(&record, &table, 100, 20);
    record.equip[4] = 0;
    record.equip[5] = 0x11;
    let outside = menu_stat_block(&record, &table, 100, 20);

    // Words: HP, MP, AGL, ATK, UDF, LDF, SPD, INT.
    assert_eq!(inside[0], 100);
    assert_eq!(inside[1], 20);
    assert_eq!(inside[2], 33, "AGL is the record word, never equipment-fed");
    assert_eq!(inside[3], 17, "slot 4 is inside the walk");
    assert_eq!(outside[3], 10, "slot 5 is past it");
}

/// Window 25's `slot_row` is retail's **browse row**, not the engine's
/// `EquipSlot` index.
///
/// A driven capture of the retail screen on each of its seven rows
/// (`captures/w1a-0919/equip_panel_astral`, one save state + frame per
/// row) settles the split: rows 1..4 - weapon, helmet, body, footwear -
/// all print the ATK / UDF / LDF triple whatever is hovered, because
/// `slti v0, s0, 4` at `0x801D137C` skips the category lookup for them;
/// only the three Goods rows resolve one, and the first of them draws
/// MAX HP / MAX MP for an HP-boost accessory. The engine's Hand Guard
/// slot is not a browse row at all.
/// Window 22's stat block is the Best Equipment row's preview only
/// (`FUN_801D21C0`'s second pass is gated on sub-screen `0x13`, row 0):
/// the candidate step's compare is window 25, so the block is empty there.
#[test]
fn the_main_window_compare_block_belongs_to_the_best_equipment_row() {
    use crate::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
    use crate::equip_session::{EquipInput, EquipSession};
    let mut inv = crate::world::ItemBag::new();
    inv.insert(1, 1);
    let mut session = EquipSession::new(
        StatRecord::default(),
        inv,
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
    );
    let names = vec!["Vahn".to_string()];
    let best = equip_screen_model(&session, 0, &names, &[], None, None);
    let labels: Vec<&str> = best.stat_compare.iter().map(|r| r.0).collect();
    assert_eq!(labels, ["ATK", "UDF", "LDF"]);
    assert_eq!(best.pictogram_rows, 7);
    assert_eq!(best.slot_items.len(), 7, "retail's seven browse rows");

    session.input(EquipInput {
        down: true,
        ..Default::default()
    });
    let row1 = equip_screen_model(&session, 0, &names, &[], None, None);
    assert!(row1.stat_compare.is_empty() && row1.best_changes.is_empty());
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    let picker = equip_screen_model(&session, 0, &names, &[], None, None);
    assert!(picker.stat_compare.is_empty());
}

#[test]
fn compare_slot_row_is_the_retail_browse_row_not_the_engine_slot() {
    use crate::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
    use crate::equip_session::{EquipInput, EquipSession, EquipState};

    // Engine slot -> the row retail's browse column would be on. Rows
    // `>= 4` are the three Goods rows, the only ones that resolve a
    // compare category. Engine slot 3 (Hand Guard, the Ra-Seru byte) is
    // not a browse row, as in retail, so no picker opens on it.
    const WANT: [(u8, i32); 7] = [
        (0, 0), // weapon
        (1, 1), // helmet
        (2, 2), // body
        (4, 3), // footwear - retail row 3, below the `slti 4` guard
        (5, 4), // Goods 1
        (6, 5), // Goods 2
        (7, 6), // Goods 3
    ];

    let record = legaia_save::CharacterRecord::zeroed();
    let names = vec!["Vahn".to_string()];
    for (engine_slot, want_row) in WANT {
        let mut inv = crate::world::ItemBag::new();
        // One owned candidate whose legacy `id >> 5` slot is this row,
        // so the picker has something to stage.
        let id = (engine_slot << 5) | 1;
        inv.insert(id, 1);
        let mut session = EquipSession::new(
            StatRecord::default(),
            inv,
            EquipmentTable::new(),
            StatusModifiers::default(),
            Vec::new(),
        );
        for _ in 0..crate::equip_session::browse_row_for_slot(engine_slot) {
            session.input(EquipInput {
                down: true,
                ..Default::default()
            });
        }
        session.input(EquipInput {
            cross: true,
            ..Default::default()
        });
        assert!(
            matches!(session.state(), EquipState::ItemPicker { slot, .. } if slot == engine_slot),
            "engine slot {engine_slot} did not open its picker"
        );
        let ctx = EquipCompareCtx {
            record: &record,
            equip_info: None,
            item_effects: None,
        };
        let model = equip_screen_model(&session, 0, &names, &[], None, Some(ctx));
        let compare = model.compare.expect("candidate step publishes window 25");
        assert_eq!(
            compare.slot_row, want_row,
            "engine slot {engine_slot} should report retail browse row {want_row}"
        );
    }
}

/// The class byte picks the table, and both tables are indexed by the
/// item record's `+1` byte. Without a table at all the lookup answers the
/// sentinel rather than guessing.
#[test]
fn compare_category_falls_back_to_the_sentinel_without_tables() {
    let record = legaia_save::CharacterRecord::zeroed();
    let ctx = EquipCompareCtx {
        record: &record,
        equip_info: None,
        item_effects: None,
    };
    assert_eq!(
        compare_category_for_item(0x11, &ctx),
        COMPARE_CATEGORY_DEFAULT
    );
}

/// The four kind arms of `FUN_801DC6B4`'s entry decode, plus the
/// fall-through: any other kind and a null context keep the root picker.
#[test]
fn the_entry_decode_routes_the_four_context_kinds() {
    assert_eq!(menu_entry_subscreen(Some(0)), 0x1A);
    assert_eq!(menu_entry_subscreen(Some(1)), 0x19);
    assert_eq!(menu_entry_subscreen(Some(7)), 0x20);
    assert_eq!(
        menu_entry_subscreen(Some(ROOT_MENU_CONTEXT_LOCKED)),
        CONTEXT_LOCKED_ENTRY_SUBSCREEN
    );
    for kind in [None, Some(2), Some(5), Some(0x0B), Some(0x0C), Some(0xFF)] {
        assert_eq!(menu_entry_subscreen(kind), ROOT_PICKER_SUBSCREEN);
    }
    // The save-point arm opens the same card driver the root picker's
    // Save row routes to.
    assert_eq!(CONTEXT_SAVE_ENTRY_SUBSCREEN, ROOT_MENU_ROUTES[6]);
}
