use super::*;
use legaia_asset::menu_windows::{MenuWindowDescriptor, MenuWindowTable};
use legaia_engine_core::menu_runtime::MenuState;
use legaia_engine_core::shop_catalog::ShopItemData;

fn table_with(id: usize, renderer_va: u32) -> MenuWindowTable {
    let blank = MenuWindowDescriptor {
        content_id: 0,
        park_edge: 0,
        kind: 3,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        renderer_va: 0,
    };
    let mut windows = vec![blank; 52];
    windows[id] = MenuWindowDescriptor {
        x: 100,
        y: 60,
        w: 0x90,
        h: 0x70,
        renderer_va,
        ..blank
    };
    MenuWindowTable { windows }
}

fn inputs<'a>(
    world: &'a World,
    menu: &'a MenuRuntime,
    font: &'a legaia_font::Font,
    table: Option<&'a MenuWindowTable>,
    chrome: Option<&'a SaveMenuAtlasRects>,
    coin_counter: &'a [SubmodeLine],
) -> ScreenInputs<'a> {
    ScreenInputs {
        world,
        menu,
        font,
        table,
        chrome,
        seru_names: None,
        coin_counter,
    }
}

/// Chrome rects with non-zero 9-slice cells (the panel tiler divides by the
/// edge sizes).
fn chrome_rects() -> SaveMenuAtlasRects {
    let cell = (0, 0, 8, 8);
    SaveMenuAtlasRects {
        panel_tl: cell,
        panel_tr: cell,
        panel_bl: cell,
        panel_br: cell,
        panel_top: cell,
        panel_bot: cell,
        panel_left: cell,
        panel_right: cell,
        panel_interior: cell,
        cursor: cell,
        ..Default::default()
    }
}

fn has_ink(draws: &[TextDraw], ink: [f32; 4]) -> bool {
    draws.iter().any(|d| d.color == ink)
}

#[test]
fn sell_detail_draws_nothing_without_a_staged_item() {
    let world = World::new();
    let menu = MenuRuntime::new(std::env::temp_dir());
    let font = legaia_font::Font::placeholder();
    let table = table_with(WIN_SELL_DETAIL, RENDERER_SELL_DETAIL);
    let i = inputs(&world, &menu, &font, Some(&table), None, &[]);
    let (texts, pic) = sell_detail_window_draws(&i, &table, None);
    assert!(texts.is_empty(), "retail leaves only the shade box");
    assert!(pic.is_none());
}

#[test]
fn sell_detail_prices_a_sellable_item() {
    let mut world = World::new();
    let mut prices = [0u16; 256];
    prices[5] = 200;
    world.shops.item_shop_data = Some(ShopItemData::from_prices(prices));
    let menu = MenuRuntime::new(std::env::temp_dir());
    let font = legaia_font::Font::placeholder();
    let table = table_with(WIN_SELL_DETAIL, RENDERER_SELL_DETAIL);
    let i = inputs(&world, &menu, &font, Some(&table), None, &[]);
    let (texts, pic) = sell_detail_window_draws(&i, &table, Some(5));
    let panel = legaia_engine_core::shop::shop_sell_detail_panel((100, 60), 5, 200, None);
    let row = panel.sell.expect("a priced item has a price row");
    let pic = pic.expect("the currency pictogram rides the price row");
    assert_eq!(
        (pic.x, pic.y),
        (i32::from(row.icon_pen.0), i32::from(row.icon_pen.1))
    );
    assert_eq!(pic.id, legaia_engine_ui::COUNTER_PICTOGRAM_GOLD);
    assert!(has_ink(&texts, legaia_engine_ui::MENU_TEXT_GOLD), "name");
    assert!(
        has_ink(&texts, legaia_engine_ui::MENU_TEXT_TEAL),
        "price label"
    );
    assert!(
        !has_ink(&texts, legaia_engine_ui::MENU_TEXT_ORANGE),
        "no cannot-sell line"
    );
}

#[test]
fn sell_detail_marks_an_unpriced_item_unsellable() {
    let world = World::new();
    let menu = MenuRuntime::new(std::env::temp_dir());
    let font = legaia_font::Font::placeholder();
    let table = table_with(WIN_SELL_DETAIL, RENDERER_SELL_DETAIL);
    let i = inputs(&world, &menu, &font, Some(&table), None, &[]);
    let (texts, pic) = sell_detail_window_draws(&i, &table, Some(5));
    assert!(pic.is_none(), "no price row, no pictogram");
    assert!(
        has_ink(&texts, legaia_engine_ui::MENU_TEXT_ORANGE),
        "cannot-sell line"
    );
    assert!(
        !has_ink(&texts, legaia_engine_ui::MENU_TEXT_TEAL),
        "no price label"
    );
}

#[test]
fn sell_detail_skips_a_window_naming_another_renderer() {
    let world = World::new();
    let menu = MenuRuntime::new(std::env::temp_dir());
    let font = legaia_font::Font::placeholder();
    let table = table_with(WIN_SELL_DETAIL, RENDERER_BUY_QUANTITY);
    let i = inputs(&world, &menu, &font, Some(&table), None, &[]);
    let (texts, pic) = sell_detail_window_draws(&i, &table, Some(5));
    assert!(texts.is_empty() && pic.is_none());
}

#[test]
fn panel_frame_is_sized_from_the_panel_alone() {
    let mut world = World::new();
    world.party.money = 120;
    let mut menu = MenuRuntime::new(std::env::temp_dir());
    menu.open_inn(30);
    menu.ctx.state = MenuState::InnConfirm.as_byte();
    let font = legaia_font::Font::placeholder();
    let chrome = chrome_rects();
    // A coin-counter line far above the panel: it joins the stage texts but
    // must not stretch the panel's frame.
    let coin = [SubmodeLine {
        text: b"COINS".to_vec(),
        x: 40,
        y: 12,
        pen: 0,
    }];
    let i = inputs(&world, &menu, &font, None, Some(&chrome), &coin);
    let (w, h) = (960, 720);
    let frame = shop_overlay_frame(&i, w, h);

    let panel = fallback_panel_draws(&i, w, h).panel;
    assert!(!panel.is_empty(), "the inn prompt draws");
    let rect = panel_frame_rect(&panel).expect("a panel has a frame");
    let (origin, scale) = legaia_engine_ui::pause_menu::stage_transform(w, h);
    let expected = legaia_engine_ui::menu_window_chrome_draws_for(&chrome, rect, origin, scale);
    let got: Vec<_> = frame.sprites.iter().map(|s| s.dst).collect();
    let want: Vec<_> = expected.iter().map(|s| s.dst).collect();
    assert_eq!(got, want, "the frame is the panel's");
    // Sizing off everything in the group would have counted the coin row.
    assert_ne!(panel_frame_rect(&frame.stage_texts), Some(rect));
}

#[test]
fn no_panel_no_frame() {
    assert_eq!(panel_frame_rect(&[]), None);
    let world = World::new();
    let menu = MenuRuntime::new(std::env::temp_dir());
    let font = legaia_font::Font::placeholder();
    let chrome = chrome_rects();
    let i = inputs(&world, &menu, &font, None, Some(&chrome), &[]);
    assert!(shop_overlay_frame(&i, 960, 720).is_empty());
}

#[test]
fn inn_sleep_caption_uses_the_menu_ink() {
    let world = World::new();
    let mut menu = MenuRuntime::new(std::env::temp_dir());
    menu.open_inn(30);
    menu.ctx.state = MenuState::InnSleep.as_byte();
    let font = legaia_font::Font::placeholder();
    let i = inputs(&world, &menu, &font, None, None, &[]);
    let panel = fallback_panel_draws(&i, 960, 720).panel;
    assert!(!panel.is_empty());
    assert!(
        panel
            .iter()
            .all(|d| d.color == legaia_engine_ui::MENU_TEXT_WHITE)
    );
}
