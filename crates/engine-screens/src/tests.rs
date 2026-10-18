use super::*;
use legaia_engine_menus::menu::MenuState;
use legaia_engine_menus::shop_catalog::ShopItemData;
use legaia_game_tables::menu_windows::{MenuWindowDescriptor, MenuWindowTable};

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
    let panel = legaia_engine_menus::shop::shop_sell_detail_panel((100, 60), 5, 200, None);
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

/// A merchant's shop opened the way both hosts open one: armed on the
/// world, drained by `poll_field_overlay` (the opening tick's step).
fn merchant_shop() -> (World, MenuRuntime) {
    use legaia_engine_menus::shop::{ShopInventory, ShopItem, ShopSession};
    let mut world = World::new();
    world.shops.shop_armed = true;
    world.shops.shop_open = true;
    world.shops.pending_shop = Some(ShopSession::new(ShopInventory::new(
        0,
        vec![ShopItem {
            item_id: 0x40,
            price: 10,
        }],
    )));
    let mut menu = MenuRuntime::new(std::env::temp_dir());
    assert!(menu.poll_field_overlay(&mut world).is_some());
    (world, menu)
}

/// The shop's windows slide in **once**. The first frame after the field's
/// fade used to find no slide stepped yet, so the screen drew every window
/// at home, and the next frame started the slide from off screen - the open
/// played twice.
#[test]
fn shop_open_slides_in_once() {
    use legaia_engine_menus::shop::SHOP_SLIDE_FRAMES;
    let (mut world, mut menu) = merchant_shop();
    let font = legaia_font::Font::placeholder();
    let chrome = chrome_rects();
    let mut seen: Vec<u8> = Vec::new();
    for _ in 0..60 {
        menu.step_field_session(&mut world, 0);
        let i = inputs(&world, &menu, &font, None, Some(&chrome), &[]);
        let f = shop_overlay_frame(&i, 960, 720);
        assert!(f.sprites.len() < 4096 && f.stage_texts.len() < 4096);
        if menu.shop_fade_level().is_some() {
            assert!(f.is_empty(), "a window drew under the field fade");
            continue;
        }
        let slides = menu.shop_slides();
        assert!(
            !slides.is_empty(),
            "a shop frame past the fade with no slide state"
        );
        let plate = slides.iter().find(|(id, _)| *id == 33).map(|(_, p)| *p);
        seen.push(plate.expect("vendor plate on the root screen"));
    }
    let expected: Vec<u8> = (1..=SHOP_SLIDE_FRAMES).collect();
    assert_eq!(&seen[..expected.len()], &expected[..]);
    assert!(
        seen[expected.len()..]
            .iter()
            .all(|p| *p == SHOP_SLIDE_FRAMES)
    );
}

/// Leaving the shop slides its windows back out and prints nothing: retail's
/// exit sub-screen (`FUN_801DD12C`) runs the close-all script and fades,
/// with no closing line. The port used to drop the windows at once and
/// print `[SHOP - DONE]` and then `[CLOSING]` over the black screen.
#[test]
fn shop_close_slides_out_with_no_closing_text() {
    use legaia_engine_menus::shop::SHOP_SLIDE_FRAMES;
    let (mut world, mut menu) = merchant_shop();
    let font = legaia_font::Font::placeholder();
    let chrome = chrome_rects();
    for _ in 0..40 {
        menu.step_field_session(&mut world, 0);
    }
    assert_eq!(menu.ctx_state(), MenuState::ShopMenu.as_byte());
    let triangle = legaia_engine_system::input::PadButton::Triangle.mask();
    menu.step_field_session(&mut world, triangle);
    assert_eq!(menu.ctx_state(), MenuState::ShopExit.as_byte());
    let mut out: Vec<u8> = Vec::new();
    let mut ticks = 0;
    while menu.is_open() {
        let i = inputs(&world, &menu, &font, None, Some(&chrome), &[]);
        let f = shop_overlay_frame(&i, 960, 720);
        assert!(
            fallback_panel_draws(&i, 960, 720).panel.is_empty(),
            "a closing line on state {:#x}",
            menu.ctx_state()
        );
        let slides = menu.shop_slides();
        if slides.is_empty() {
            assert!(f.is_empty(), "draws with every window gone");
        } else {
            out.push(slides.iter().map(|(_, p)| *p).max().unwrap_or(0));
        }
        menu.step_field_session(&mut world, 0);
        ticks += 1;
        assert!(ticks < 200, "the shop never closed");
    }
    // The press frame still shows the root at home (retail reaches the exit
    // sub-screen on the frame after the picker hands it the pad), then
    // every window steps back out to its park edge.
    let expected: Vec<u8> = (1..=SHOP_SLIDE_FRAMES).rev().collect();
    assert_eq!(out, expected, "the windows slide out in step");
    assert!(menu.shop_session.is_none());
    assert!(!world.shops.shop_open, "the merchant op resumes");
}
