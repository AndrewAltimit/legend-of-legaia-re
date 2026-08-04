//! Library-level oracle over the **shared pause-menu composition**.
//!
//! Before the composition was hoisted into this crate, the only code that
//! assembled a pause-menu draw list lived in a binary's private module
//! (`engine-shell`'s `window/menu_draws.rs`) and in the browser play page.
//! A `tests/` target cannot import a binary's modules, so no library test
//! could enter a single pause-menu draw builder no matter how long a pad
//! ladder ran - the gap was structural, not a matter of replay reach.
//!
//! This test is what that hoist buys: every pause screen composed here, in
//! process, with no disc, no GPU and no host. It runs each screen twice -
//! with the chrome atlas absent and present - because the two passes are
//! different code (the sprite half only exists in the second, and several
//! panels swap an ASCII stand-in for a sprite between them).
//!
//! What it asserts is *reachability plus non-vacuity*: each screen produces
//! draws, the painter-gated screens produce them only with a descriptor
//! table, and the two behaviours the hosts used to disagree about - the
//! title-tab painter and the modal sprite order - come out the same way
//! whichever host asks. It is not a pixel oracle; the per-panel geometry is
//! pinned by the builders' own unit tests, and the disc table it falls back
//! from by `legaia-asset`'s disc-gated `menu_windows_real` - which asserts the
//! same rects against its own literal list rather than against
//! [`MENU_WINDOW_FALLBACK`], so the two are separate pins of one set of
//! numbers.

use legaia_asset::menu_windows::{MenuWindowDescriptor, MenuWindowTable, window_ids};
use legaia_engine_ui::pause_menu::{
    EquipComposeInput, GenericContent, ItemsScreenView, MENU_WINDOW_FALLBACK, MagicScreenView,
    MenuRects, OptionsScreenView, PauseMenuCtx, PauseMenuDraws, PauseScreen, SpecialConfirmView,
    StatusScreenView, TopLevelView, equip_screen_compose, pause_screen_draws,
    spell_level_notice_draws, stage_transform,
};
use legaia_engine_ui::ui_menu_window_dispatch as dispatch;
use legaia_engine_ui::{
    ArtsChainRow, ArtsEditorDrawArgs, ArtsEditorPhase, EquipDrawPhase, FieldMenuPartyView,
    FieldMenuRowView, InventoryItemRow, InventoryUseDrawArgs, OptionsPopupDraw, OptionsRowView,
    PauseItemInfo, PauseItemsPhase, PauseItemsRow, PauseItemsView, PauseMagicCaster,
    PauseMagicPhase, PauseMagicRow, PauseMagicView, PauseThrowConfirmView, SaveMenuAtlasRects,
    SpellMenuDrawArgs, SpellRowView, StatusPanelView, StatusSatelliteView, StatusStatRow,
    TargetPanelCursor, TargetPanelMember, TargetPanelMode, TargetPanelView,
};

/// A 960x720 window is what `play-window` opens; the browser play page runs
/// the same stage maths off its canvas size.
const SURFACE: (u32, u32) = (960, 720);

/// A descriptor table carrying exactly the windows this test drives, at the
/// pinned fallback rects and with each window's **retail renderer VA** - the
/// word the painter dispatch keys on. Synthesising it rather than parsing a
/// disc is the point: the dispatch is a property of the VA, so a table built
/// from the documented VAs exercises the same resolution path a real disc
/// takes, with no Sony bytes anywhere near the test.
fn synthetic_table() -> MenuWindowTable {
    let mut windows: Vec<MenuWindowDescriptor> = (0..52)
        .map(|_| MenuWindowDescriptor {
            content_id: 0,
            park_edge: 0,
            kind: 3,
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            renderer_va: 0,
        })
        .collect();
    for (id, (x, y, w, h)) in MENU_WINDOW_FALLBACK {
        if let Some(d) = windows.get_mut(id) {
            d.x = x as i16;
            d.y = y as i16;
            d.w = w as i16;
            d.h = h as i16;
        }
    }
    let renderers: [(usize, u32); 8] = [
        (window_ids::TAB_ITEMS, dispatch::RENDERER_TAB_ITEMS),
        (window_ids::TAB_MAGIC, dispatch::RENDERER_TAB_MAGIC),
        (window_ids::TAB_EQUIP, dispatch::RENDERER_TAB_EQUIP),
        (window_ids::TAB_STATUS, dispatch::RENDERER_TAB_STATUS),
        (window_ids::TAB_OPTIONS, dispatch::RENDERER_TAB_OPTIONS),
        (7, dispatch::RENDERER_CHAR_PROMPT),
        (6, dispatch::RENDERER_LABEL_LIST),
        (5, dispatch::RENDERER_TWO_LINE_CHOICE_PANEL),
    ];
    for (id, va) in renderers {
        if let Some(d) = windows.get_mut(id) {
            d.renderer_va = va;
        }
    }
    MenuWindowTable { windows }
}

/// The chrome atlas' band rects. The real one is baked from the disc's
/// system-UI TIMs; every band here is a distinct non-empty source rect, which
/// is all the sprite emitters read.
fn chrome_rects() -> SaveMenuAtlasRects {
    let mut n = 0u32;
    let mut band = || {
        n += 1;
        (n * 8, 0, 8, 8)
    };
    SaveMenuAtlasRects {
        panel_tl: band(),
        panel_tr: band(),
        panel_bl: band(),
        panel_br: band(),
        panel_top: band(),
        panel_bot: band(),
        panel_left: band(),
        panel_right: band(),
        slot1: band(),
        slot2: band(),
        cursor: band(),
        panel_interior: band(),
        panel_filigree: band(),
        label_lv: band(),
        label_hp: band(),
        label_mp: band(),
        icon_money: band(),
        label_time: band(),
        label_coin: band(),
        gauge_cap: band(),
        gauge_trough: band(),
        gauge_box: band(),
        gauge_tip: band(),
        gauge_digits: band(),
        gauge_100: band(),
        gauge_fill: band(),
        dialog_fill: band(),
        icon_weapon: band(),
        icon_helmet: band(),
        icon_armor: band(),
        icon_boot: band(),
        icon_goods: band(),
        pager_left: band(),
        pager_right: band(),
        tab_cap_l: band(),
        tab_body: band(),
        tab_cap_r: band(),
        atr_icons: [band(), band(), band()],
        load_empty_frame: None,
        load_portrait_by_char: [Some(band()), Some(band()), Some(band())],
        battle: None,
    }
}

/// Host-side inputs for one composition pass.
struct Fixture {
    font: legaia_font::Font,
    table: MenuWindowTable,
    chrome: Option<SaveMenuAtlasRects>,
}

impl Fixture {
    fn new(chrome: bool) -> Self {
        Fixture {
            font: legaia_font::Font::placeholder(),
            table: synthetic_table(),
            chrome: chrome.then(chrome_rects),
        }
    }

    fn ctx(&self) -> PauseMenuCtx<'_> {
        let (origin, scale) = stage_transform(SURFACE.0, SURFACE.1);
        PauseMenuCtx {
            font: &self.font,
            rects: MenuRects::new(Some(&self.table)),
            chrome: self.chrome.as_ref(),
            origin,
            scale,
        }
    }

    /// The same context with **no** descriptor table - a PROT.DAT-only load,
    /// or a disc whose menu overlay failed to parse.
    fn ctx_tableless(&self) -> PauseMenuCtx<'_> {
        let (origin, scale) = stage_transform(SURFACE.0, SURFACE.1);
        PauseMenuCtx {
            font: &self.font,
            rects: MenuRects::new(None),
            chrome: self.chrome.as_ref(),
            origin,
            scale,
        }
    }
}

// --- per-screen drivers -----------------------------------------------------
//
// Each takes the context and composes one screen, exactly as a host would.

fn compose_top_level(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let rows = [
        FieldMenuRowView {
            label: "Items",
            enabled: true,
        },
        FieldMenuRowView {
            label: "Magic",
            enabled: true,
        },
        // The greyed row: retail walks it and refuses at the confirm.
        FieldMenuRowView {
            label: "Save",
            enabled: false,
        },
    ];
    let party = [FieldMenuPartyView {
        name: "Vahn",
        level: 5,
        hp: 60,
        hp_max: 80,
        mp: 10,
        mp_max: 12,
        ap: 40,
    }];
    pause_screen_draws(
        ctx,
        PauseScreen::TopLevel(TopLevelView {
            rows: &rows,
            cursor: 1,
            money: 1234,
            play_time_seconds: 3671,
            party: &party,
            party_ap: &[40],
        }),
    )
}

fn compose_status(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let stat_rows = [
        StatusStatRow {
            label: "ATK",
            value: 30,
            growth: 2,
        },
        StatusStatRow {
            label: "UDF",
            value: 18,
            growth: 1,
        },
    ];
    let equip_rows = [("Weapon", "Sword"), ("Armor", "Vest")];
    let panel = StatusPanelView {
        name: "Vahn",
        level: 5,
        xp: 400,
        xp_to_next: 120,
        hp: 60,
        hp_max: 80,
        mp: 10,
        mp_max: 12,
        ap: 40,
        ap_max: 100,
        stat_rows: &stat_rows,
        equip_rows: &equip_rows,
    };
    let names = ["Vahn", "Noa"];
    let satellite = StatusSatelliteView {
        party_names: &names,
        cursor: 0,
        name: "Vahn",
        level: 5,
    };
    pause_screen_draws(
        ctx,
        PauseScreen::Status(StatusScreenView {
            panel: &panel,
            satellite: &satellite,
            ap: 40,
            atr_char: 0,
        }),
    )
}

fn compose_options(ctx: &PauseMenuCtx<'_>, popup: bool) -> PauseMenuDraws {
    let rows = [
        OptionsRowView {
            label: "Sound",
            value: Some("Stereo"),
            teal: false,
            advance: 14,
        },
        OptionsRowView {
            label: "Message",
            value: Some("Normal"),
            teal: true,
            advance: 14,
        },
    ];
    let choices = ["Stereo", "Monaural"];
    pause_screen_draws(
        ctx,
        PauseScreen::Options(OptionsScreenView {
            rows: &rows,
            cursor: 1,
            popup: popup.then_some(OptionsPopupDraw {
                rect: (170, 132, 128, 36),
                choices: &choices,
                cursor: 0,
            }),
            row_y_off: 14,
        }),
    )
}

fn compose_items(ctx: &PauseMenuCtx<'_>, modal: ItemsModal) -> PauseMenuDraws {
    let rows = [
        PauseItemsRow {
            name: "Healing Leaf",
            count: 3,
        },
        PauseItemsRow {
            name: "Point Card",
            count: 1,
        },
    ];
    let view = PauseItemsView {
        rows: &rows,
        page: 1,
        pages: 6,
        phase: PauseItemsPhase::List,
        command_cursor: 0,
        list_cursor: 1,
        bag_empty: false,
        info: Some(PauseItemInfo {
            name: "Point Card",
            count: 1,
            desc: "Casino points.",
            passive: None,
        }),
        text_cursor: ctx.chrome.is_none(),
    };
    let throw = PauseThrowConfirmView {
        name: "Healing Leaf",
        count: 3,
        cursor: 1,
        text_cursor: ctx.chrome.is_none(),
    };
    let two_line = ["Incense", "Use it?"];
    let one_line = ["Use Door of Light?"];
    pause_screen_draws(
        ctx,
        PauseScreen::Items(ItemsScreenView {
            view: &view,
            point_card: Some(4200),
            throw_confirm: matches!(modal, ItemsModal::Throw).then_some(&throw),
            special_confirm: match modal {
                ItemsModal::SpecialOneLine => Some(SpecialConfirmView {
                    lines: &one_line,
                    cursor: 0,
                }),
                ItemsModal::SpecialTwoLine => Some(SpecialConfirmView {
                    lines: &two_line,
                    cursor: 1,
                }),
                _ => None,
            },
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ItemsModal {
    None,
    Throw,
    SpecialOneLine,
    SpecialTwoLine,
}

fn compose_items_target(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let members = [TargetPanelMember {
        name: "Vahn",
        level: 5,
        hp: 60,
        mp: 10,
        hp_max: 80,
        mp_max: 12,
        base_hp_max: 80,
        base_mp_max: 12,
        stat_eff: [30, 18, 16, 12, 9],
        stat_base: [30, 18, 16, 12, 9],
    }];
    let view = TargetPanelView {
        members: &members,
        mode: TargetPanelMode::from_preview_word(0),
        cursor: TargetPanelCursor::Single {
            row: 0,
            pressed: false,
        },
        label_icons: ctx.chrome.is_some(),
        text_cursor: ctx.chrome.is_none(),
    };
    pause_screen_draws(ctx, PauseScreen::ItemsTarget(&view))
}

fn compose_magic(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let casters = [PauseMagicCaster {
        name: "Vahn",
        level: 5,
        mp: 10,
        mp_max: 12,
    }];
    let rows = [PauseMagicRow {
        name: "Meta",
        ra_seru: true,
    }];
    let view = PauseMagicView {
        casters: &casters,
        rows: &rows,
        page: 1,
        pages: 2,
        phase: PauseMagicPhase::List,
        caster_cursor: 0,
        list_cursor: 0,
        info: None,
        label_icons: ctx.chrome.is_some(),
        text_cursor: ctx.chrome.is_none(),
    };
    pause_screen_draws(
        ctx,
        PauseScreen::Magic(MagicScreenView {
            view: &view,
            casters: 1,
        }),
    )
}

fn compose_equip(ctx: &PauseMenuCtx<'_>, phase: EquipDrawPhase) -> PauseMenuDraws {
    let party_names = ["Vahn".to_string(), "Noa".to_string()];
    let slot_labels: Vec<String> = ["Weapon", "Head", "Armor", "Boots", "Goods"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let slot_items: Vec<String> = vec![
        "Sword".into(),
        String::new(),
        "Vest".into(),
        "".into(),
        "".into(),
    ];
    let candidate_names: Vec<String> = vec!["Item 21".into(), "Item 22".into()];
    let stat_compare = [("ATK", 30u16, 34u16), ("UDF", 18, 18), ("LDF", 16, 15)];
    equip_screen_compose(
        ctx,
        &EquipComposeInput {
            party_names: &party_names,
            slot_labels: &slot_labels,
            slot_items: &slot_items,
            candidate_names: &candidate_names,
            candidate_counts: &[2, 1],
            stat_compare: &stat_compare,
            phase,
            cursor: 1,
            active_slot: 0,
            confirm_label: matches!(phase, EquipDrawPhase::Confirm).then_some("Equip Item 21?"),
            char_slot: 0,
            slot_cursor: matches!(phase, EquipDrawPhase::SlotPicker).then_some(1),
            pictogram_rows: 5,
            text_cursor: ctx.chrome.is_none(),
        },
    )
}

fn compose_arts(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let saved = [ArtsChainRow {
        name: "Double Punch",
        pretty_sequence: "^ ^",
    }];
    pause_screen_draws(
        ctx,
        PauseScreen::Generic(GenericContent::Arts(ArtsEditorDrawArgs {
            character_name: "Vahn",
            phase: ArtsEditorPhase::Browsing,
            saved: &saved,
            browse_cursor: 0,
            editing_pretty: "",
            editing_len: 0,
            min_len: 2,
            max_len: 9,
            naming_name: "",
            can_add_new: true,
        })),
    )
}

fn compose_spell_target(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let names = ["Vahn"];
    let spells = [SpellRowView {
        name: "Meta",
        mp_cost: 4,
        admissible: true,
    }];
    pause_screen_draws(
        ctx,
        PauseScreen::Generic(GenericContent::SpellMenu(SpellMenuDrawArgs {
            party_names: &names,
            party_hp: &[(60, 80)],
            party_mp: &[(10, 12)],
            selected_caster: Some(0),
            spells: &spells,
            selected_spell: None,
            targets: &[],
            selected_target: None,
            cursor: 0,
            phase: 1,
        })),
    )
}

fn compose_inventory_standin(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let items = [InventoryItemRow {
        name: "Healing Leaf",
        count: 3,
        admissible: true,
    }];
    let content = legaia_engine_ui::inventory_use_draws_for(
        ctx.font,
        InventoryUseDrawArgs {
            items: &items,
            targets: &[],
            in_battle: false,
            cursor: 0,
            phase: 0,
            selected_item_name: Some("Healing Leaf"),
        },
        (16, 32),
    );
    pause_screen_draws(ctx, PauseScreen::Generic(GenericContent::Prebuilt(content)))
}

fn compose_context_notice(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    let lines = ["Ready?", "Party is set."];
    pause_screen_draws(ctx, PauseScreen::ContextNotice { lines: &lines })
}

fn compose_context_ready(ctx: &PauseMenuCtx<'_>) -> PauseMenuDraws {
    pause_screen_draws(
        ctx,
        PauseScreen::ContextReady {
            headings: ["Begin the", "battle?"],
            choices: ["Yes", "No"],
            cursor: 1,
        },
    )
}

// --- the oracle -------------------------------------------------------------

/// Every pause screen composes, on both chrome passes.
///
/// The `sprites` half is the one that only exists with the atlas resident,
/// so the two passes are asserted differently: without chrome every screen
/// must still produce text and must produce **no** sprites (the frames come
/// off the atlas), and with chrome every screen that frames a window must
/// produce sprites too.
#[test]
fn every_pause_screen_composes_on_both_chrome_passes() {
    for chrome in [false, true] {
        let fx = Fixture::new(chrome);
        let ctx = fx.ctx();
        let framed: Vec<(&str, PauseMenuDraws)> = vec![
            ("top level", compose_top_level(&ctx)),
            ("status", compose_status(&ctx)),
            ("options", compose_options(&ctx, false)),
            ("options + popup", compose_options(&ctx, true)),
            ("items", compose_items(&ctx, ItemsModal::None)),
            ("items + throw", compose_items(&ctx, ItemsModal::Throw)),
            (
                "items + use confirm (1 line)",
                compose_items(&ctx, ItemsModal::SpecialOneLine),
            ),
            (
                "items + use confirm (2 line)",
                compose_items(&ctx, ItemsModal::SpecialTwoLine),
            ),
            ("items target panel", compose_items_target(&ctx)),
            ("magic", compose_magic(&ctx)),
            (
                "equip: slot picker",
                compose_equip(&ctx, EquipDrawPhase::SlotPicker),
            ),
            (
                "equip: item picker",
                compose_equip(&ctx, EquipDrawPhase::ItemPicker),
            ),
            (
                "equip: confirm",
                compose_equip(&ctx, EquipDrawPhase::Confirm),
            ),
            ("arts editor", compose_arts(&ctx)),
            ("spell target select", compose_spell_target(&ctx)),
            ("inventory stand-in", compose_inventory_standin(&ctx)),
        ];
        for (name, d) in &framed {
            assert!(
                !d.texts.is_empty(),
                "{name}: composed no glyphs (chrome = {chrome})"
            );
            assert_eq!(
                d.sprites.is_empty(),
                !chrome,
                "{name}: the sprite half must exist exactly when the chrome atlas does"
            );
        }
        // The two kind-0x0D screens are content-only - retail's open script
        // closes every window before opening theirs, so they frame nothing.
        for (name, d) in [
            ("context notice", compose_context_notice(&ctx)),
            ("context ready", compose_context_ready(&ctx)),
        ] {
            assert!(!d.texts.is_empty(), "{name}: composed no glyphs");
            assert!(d.sprites.is_empty(), "{name}: draws no window chrome");
        }
        assert!(
            !spell_level_notice_draws(&ctx, "Meta reached Lv2!").is_empty(),
            "window 7 notice composed nothing (chrome = {chrome})"
        );
    }
}

/// The painter-gated windows draw **only** with a real descriptor table.
///
/// The painter is resolved from the descriptor's `renderer_va`, which no
/// pinned fallback can invent, so these three windows are silent on a
/// PROT.DAT-only load. That is the behaviour both hosts documented; before
/// the hoist each enforced it in its own copy of the code.
#[test]
fn painter_gated_windows_need_the_descriptor_table() {
    let fx = Fixture::new(true);
    let bare = fx.ctx_tableless();
    assert!(compose_context_notice(&bare).texts.is_empty());
    assert!(compose_context_ready(&bare).texts.is_empty());
    assert!(spell_level_notice_draws(&bare, "Meta reached Lv2!").is_empty());
    // With the table they all draw - otherwise the assertions above would
    // pass against a composition that is simply broken.
    let full = fx.ctx();
    assert!(!compose_context_notice(&full).texts.is_empty());
    assert!(!compose_context_ready(&full).texts.is_empty());
    assert!(!spell_level_notice_draws(&full, "Meta reached Lv2!").is_empty());
}

/// A screen that is not painter-gated still composes without a table - it
/// falls back to the pinned rects rather than going blank.
#[test]
fn the_pinned_fallback_carries_a_screen_with_no_descriptor_table() {
    let fx = Fixture::new(true);
    let bare = fx.ctx_tableless();
    for (name, d) in [
        ("status", compose_status(&bare)),
        ("items", compose_items(&bare, ItemsModal::None)),
        ("magic", compose_magic(&bare)),
    ] {
        assert!(!d.texts.is_empty(), "{name}: went blank without a table");
        assert!(!d.sprites.is_empty(), "{name}: framed nothing");
    }
}

/// The four modal ids resolve to their own pinned rects, not to the
/// near-fullscreen stand-in.
///
/// Both hosts used to guard this with `if pen == (0, 0) { fallback }`, and
/// the guard never fired: an id missing from the fallback table resolves to
/// `MENU_SUBWINDOW_CONTENT`, whose origin is `(18, 18)`. So a disc-less run
/// drew the throw-out prompt, both Use confirms and the party target panel
/// at the wrong origin on **both** hosts, identically and invisibly.
#[test]
fn the_modal_windows_have_their_own_pinned_rects() {
    let rects = MenuRects::new(None);
    assert_eq!(rects.rect(9), legaia_engine_ui::ITEMS_THROW_CONFIRM_RECT);
    assert_eq!(
        rects.rect(10),
        legaia_engine_ui::ITEMS_USE_CONFIRM_1LINE_RECT
    );
    assert_eq!(
        rects.rect(12),
        legaia_engine_ui::ITEMS_USE_CONFIRM_2LINE_RECT
    );
    assert_eq!(rects.rect(14), legaia_engine_ui::TARGET_PANEL_RECT);
    assert_ne!(
        rects.rect(14),
        legaia_engine_ui::pause_menu::MENU_SUBWINDOW_CONTENT,
        "the target panel must not land on the generic stand-in rect"
    );
}

/// A sub-screen's title tab goes through the descriptor **painter** when the
/// table names one, and through the pinned label otherwise.
///
/// This is the drift the hoist closed: the native window resolved the tab
/// through `painter_at` while the browser page called `tab_label_draws`
/// unconditionally, so a table whose tab renderer moved (a modded disc) put
/// the label in two different places depending on which host was running.
/// Both now ask the same question.
#[test]
fn the_title_tab_follows_the_descriptor_painter() {
    let fx = Fixture::new(false);
    // Move the Status tab's content rect while leaving its renderer in
    // place: the painter hangs off the descriptor's rect, so the tab has to
    // move with it.
    let mut moved = synthetic_table();
    moved.windows[window_ids::TAB_STATUS].x = 120;
    moved.windows[window_ids::TAB_STATUS].y = 90;
    let (origin, scale) = stage_transform(SURFACE.0, SURFACE.1);
    let ctx_moved = PauseMenuCtx {
        font: &fx.font,
        rects: MenuRects::new(Some(&moved)),
        chrome: None,
        origin,
        scale,
    };
    let base = compose_status(&fx.ctx());
    let shifted = compose_status(&ctx_moved);
    assert_eq!(
        base.texts.len(),
        shifted.texts.len(),
        "moving one window must not change how many glyphs the screen draws"
    );
    assert_ne!(
        base.texts.last().map(|d| d.dst),
        shifted.texts.last().map(|d| d.dst),
        "the tab label must track its descriptor rect"
    );

    // With the renderer cleared the descriptor is no longer a title tab, so
    // the composition falls back to the pinned label pen.
    let mut renderer_less = synthetic_table();
    renderer_less.windows[window_ids::TAB_STATUS].renderer_va = 0;
    renderer_less.windows[window_ids::TAB_STATUS].x = 120;
    renderer_less.windows[window_ids::TAB_STATUS].y = 90;
    let ctx_bare = PauseMenuCtx {
        font: &fx.font,
        rects: MenuRects::new(Some(&renderer_less)),
        chrome: None,
        origin,
        scale,
    };
    let fallback = compose_status(&ctx_bare);
    assert!(!fallback.texts.is_empty(), "the tab still draws its label");
}

/// The stage transform is the one both hosts use, and it centres the
/// 320x240 stage at an integer scale.
#[test]
fn the_stage_transform_centres_at_an_integer_scale() {
    let (origin, scale) = stage_transform(960, 720);
    assert_eq!(scale, 3);
    assert_eq!(origin, (0, 0));
    // A surface smaller than the stage clamps to 1x rather than to 0.
    let (_, small) = stage_transform(100, 100);
    assert_eq!(small, 1);
    // And the clamp caps at 4x, which is what keeps a 4K window from
    // scaling the menu off its own edges.
    let (_, big) = stage_transform(4096, 4096);
    assert_eq!(big, 4);
}

/// Every draw the composition emits lands inside the surface it was given.
///
/// A cheap whole-screen invariant: a builder that reads the wrong pen, or a
/// scale applied twice, walks off the surface long before it looks wrong in
/// any other assertion.
#[test]
fn composed_draws_stay_on_the_surface() {
    let fx = Fixture::new(true);
    let ctx = fx.ctx();
    let screens = [
        compose_top_level(&ctx),
        compose_status(&ctx),
        compose_options(&ctx, true),
        compose_items(&ctx, ItemsModal::Throw),
        compose_items_target(&ctx),
        compose_magic(&ctx),
        compose_equip(&ctx, EquipDrawPhase::ItemPicker),
    ];
    for d in &screens {
        for q in d.texts.iter().chain(d.sprites.iter()) {
            assert!(
                q.dst.0 >= -64
                    && q.dst.1 >= -64
                    && q.dst.0 < SURFACE.0 as i32 + 64
                    && q.dst.1 < SURFACE.1 as i32 + 64,
                "draw at {:?} is off the {SURFACE:?} surface",
                q.dst
            );
        }
    }
}
