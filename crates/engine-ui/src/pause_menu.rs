//! Pause-menu **composition**: which windows a screen opens, which painter
//! draws each one, and in what order the frames, the content and the modals
//! land on top of each other.
//!
//! Every other module in this crate builds one panel. This one is the layer
//! above: it is the code that used to live twice - once in the native
//! `play-window`'s `window/menu_draws.rs` + `window/title_save_draws.rs`, once
//! in the browser play page's `web-viewer/src/play_menu.rs` - with each host
//! deciding for itself which window set a screen frames, whether a title tab
//! goes through the descriptor painter or the pinned fallback, and where a
//! modal sits in the sprite order. Two hosts assembling the same kind of list
//! is the drift shape `docs/tooling/host-drift.md` tier 7 names, and a screen
//! that is only assembled inside a binary's private module can never be
//! entered by a library test either.
//!
//! The split this module draws:
//!
//! * the **host** resolves its own assets - the disc-parsed window table, the
//!   chrome atlas band rects, the font - and projects the live `engine-core`
//!   session into the plain view structs the panel builders take;
//! * this module owns the **composition** - [`MenuRects`] (descriptor rect ->
//!   pen / frame rect, with the pinned fallback), [`stage_transform`], the
//!   per-screen window sets, the tab painter choice, the sprite ordering and
//!   the final stage scale.
//!
//! `engine-core` is deliberately *not* a dependency: `engine-render` is
//! documented as a leaf presentation crate that does not depend on the
//! simulation (`docs/subsystems/engine.md`), and it re-exports this crate
//! wholesale, so a dependency here would be one there. The cost is that each
//! host still writes the session -> view projection; the benefit is that the
//! composition below is reachable from a plain library test with no disc, no
//! GPU and no host.

use crate::ui_menu_window_painters::{
    ChoiceFlags, char_prompt_draws_for, label_list_draws_for, title_tab_draws_for,
    two_line_choice_panel_draws_for,
};
use crate::{
    ArtsEditorDrawArgs, EquipScreenView, FieldMenuPartyView, FieldMenuRowView,
    InventoryUseDrawArgs, OptionsPopupDraw, OptionsRowView, PauseItemsView, PauseMagicView,
    PauseThrowConfirmView, SaveMenuAtlasRects, SpellMenuDrawArgs, SpriteDraw, StatusPanelView,
    StatusSatelliteView, TargetPanelView, TextDraw,
};
use crate::{MenuWindowPainter, painter_at, painter_rect};
use legaia_asset::menu_windows::{MenuWindowTable, window_ids};

/// Menu-overlay descriptor id of the **spell level-up notice** window
/// (renderer `FUN_801DCCB4`, the [`MenuWindowPainter::CharPrompt`] painter).
pub const WIN_MAGIC_LEVEL_NOTICE: usize = 7;
/// Descriptor id of the kind-`0x0D` **notice panel** (`FUN_801D6360`, the
/// [`MenuWindowPainter::LabelList`] painter), opened by menu sub-screen `4`'s
/// script `0x801E4BE0`.
pub const WIN_CONTEXT_NOTICE: usize = 6;
/// Descriptor id of the kind-`0x0D` **ready check** (`FUN_801D61B0`, the
/// [`MenuWindowPainter::TwoLineChoicePanel`] painter), opened by sub-screen
/// `3`'s script `0x801E4BD4`.
pub const WIN_CONTEXT_READY: usize = 5;

/// Content rect used for the sub-screens whose retail window sets are not
/// capture-pinned (the Tactical-Arts editor, the spell target-select
/// stand-in, the generic inventory overlay) - a near-fullscreen window on the
/// 320x240 stage.
pub const MENU_SUBWINDOW_CONTENT: (i32, i32, i32, i32) = (18, 18, 284, 200);

/// Windows framed while the Items screen's Use flow picks a target: the
/// screen tab plus descriptor 14, the party target panel (`FUN_801D0520`).
pub const TARGET_SELECT_WINDOWS: [usize; 2] = [window_ids::TAB_ITEMS, 14];

/// Pinned content rects mirroring the disc descriptor table:
/// `(descriptor id, (x, y, w, h))`.
///
/// Each descriptor rect is the window's *content* origin/extent (the
/// `a0+0xa..+0x10` rect the retail content renderers receive); the
/// caller-drawn 9-slice frame extends 8 px past it on every side
/// ([`MenuRects::frame_rect`]).
///
/// How retail-true these numbers are, said exactly: the disc-gated
/// `legaia-asset` test `menu_windows_real` pins the parsed disc table against
/// its own literal list of the same rects (RAM-matched to six catalogued
/// menu-open captures). It does **not** read this constant, so the two are
/// verified separately rather than against each other - a third copy of the
/// geometry, and the reason to change one is a reason to check the other.
///
/// The four modal ids at the end (9 / 10 / 12 / 14) are the reason this table
/// is the single fallback rather than a per-call-site `if pen == (0, 0)`
/// guard. Both hosts carried that guard, and it never fired: an id absent
/// from the table falls through to [`MENU_SUBWINDOW_CONTENT`], whose origin is
/// `(18, 18)` and not `(0, 0)`, so a disc-less run drew the throw-out prompt,
/// the two Use confirms and the party target panel at the near-fullscreen
/// origin instead of at their own pinned rects.
#[rustfmt::skip]
pub const MENU_WINDOW_FALLBACK: [(usize, (i32, i32, i32, i32)); 27] = [
    (window_ids::TAB_ITEMS, (16, 12, 60, 12)),
    (window_ids::TAB_MAGIC, (16, 12, 60, 12)),
    (window_ids::ITEMS_COMMAND, (32, 44, 80, 38)),
    (window_ids::ITEMS_LIST, (174, 22, 132, 182)),
    (window_ids::ITEMS_INFO, (14, 108, 144, 40)),
    (window_ids::MAGIC_LIST, (174, 22, 132, 182)),
    (window_ids::MAGIC_CASTER, (14, 40, 144, 96)),
    (window_ids::MAGIC_INFO, (14, 152, 144, 52)),
    (window_ids::TAB_EQUIP, (16, 12, 60, 12)),
    (window_ids::TAB_STATUS, (12, 12, 60, 12)),
    (window_ids::TAB_OPTIONS, (16, 12, 60, 12)),
    (window_ids::EQUIP_PARTY, (14, 42, 80, 38)),
    (window_ids::EQUIP_MAIN, (14, 96, 292, 108)),
    (window_ids::EQUIP_LIST, (174, 22, 132, 182)),
    (window_ids::STATUS_PARTY_LIST, (14, 38, 60, 38)),
    (window_ids::STATUS_CONDITION, (14, 92, 60, 10)),
    (window_ids::STATUS_MAIN, (90, 16, 218, 188)),
    (window_ids::STATUS_SUMMARY, (14, 134, 60, 70)),
    (window_ids::OPTIONS_MAIN, (24, 40, 256, 148)),
    // The options value popup's descriptor x/w (y/h are stamped per open -
    // see the host's `options_popup_rect`).
    (window_ids::OPTIONS_POPUP, (170, 132, 128, 36)),
    (window_ids::TOP_MONEY_TIME, (24, 178, 104, 24)),
    (window_ids::TOP_COMMAND_LIST, (24, 24, 104, 94)),
    (window_ids::TOP_INFO_PANEL, (144, 24, 152, 180)),
    (9, crate::ITEMS_THROW_CONFIRM_RECT),
    (10, crate::ITEMS_USE_CONFIRM_1LINE_RECT),
    (12, crate::ITEMS_USE_CONFIRM_2LINE_RECT),
    (14, crate::TARGET_PANEL_RECT),
];

/// Descriptor-rect resolver: the disc-parsed menu-overlay window table when
/// the host has one, else the pinned mirror in [`MENU_WINDOW_FALLBACK`].
#[derive(Clone, Copy, Default)]
pub struct MenuRects<'a> {
    table: Option<&'a MenuWindowTable>,
}

impl<'a> MenuRects<'a> {
    /// Wrap a host's parsed table (or its absence).
    pub fn new(table: Option<&'a MenuWindowTable>) -> Self {
        MenuRects { table }
    }

    /// The parsed table, when the host had one. The retail **painter** family
    /// dispatches on each descriptor's `renderer_va`, which no fallback can
    /// invent, so painter-gated windows draw only while this is `Some`.
    pub fn table(&self) -> Option<&'a MenuWindowTable> {
        self.table
    }

    /// Content rect for a menu window id.
    pub fn rect(&self, id: usize) -> (i32, i32, i32, i32) {
        if let Some(d) = self.table.and_then(|t| t.window(id)) {
            return d.rect();
        }
        MENU_WINDOW_FALLBACK
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| *r)
            .unwrap_or(MENU_SUBWINDOW_CONTENT)
    }

    /// Content-origin pen for a menu window id.
    pub fn pen(&self, id: usize) -> (i32, i32) {
        let (x, y, _, _) = self.rect(id);
        (x, y)
    }

    /// Frame rect (the 9-slice chrome box): the retail border art extends
    /// 8 px past the content rect on every side.
    pub fn frame_rect(&self, id: usize) -> (i32, i32, i32, i32) {
        let (x, y, w, h) = self.rect(id);
        (x - 8, y - 8, w + 16, h + 16)
    }
}

/// Stage origin + integer scale mapping the 320x240 boot-UI stage onto a
/// host surface, centred.
///
/// Every retail-pinned menu position is expressed in 320x240 framebuffer
/// pixels, so this is the one transform that takes them to screen coords -
/// and using the same stage for the title art, the save chrome and the pause
/// menu is what keeps their relative positions correct at any resolution.
pub fn stage_transform(surface_w: u32, surface_h: u32) -> ((i32, i32), u32) {
    let scale = (surface_w.max(1) / crate::BOOT_UI_STAGE_W)
        .min(surface_h.max(1) / crate::BOOT_UI_STAGE_H)
        .clamp(1, 4);
    let sw = crate::BOOT_UI_STAGE_W * scale;
    let sh = crate::BOOT_UI_STAGE_H * scale;
    let x0 = (surface_w as i32 - sw as i32) / 2;
    let y0 = (surface_h as i32 - sh as i32) / 2;
    ((x0, y0), scale)
}

/// One frame's worth of pause-menu output: glyph quads off the font atlas,
/// sprite quads off the chrome atlas. Both are already in surface pixels.
#[derive(Debug, Clone, Default)]
pub struct PauseMenuDraws {
    pub texts: Vec<TextDraw>,
    pub sprites: Vec<SpriteDraw>,
}

impl PauseMenuDraws {
    /// Append another screen's output (used when a modal overlays a screen).
    pub fn extend(&mut self, other: PauseMenuDraws) {
        self.texts.extend(other.texts);
        self.sprites.extend(other.sprites);
    }

    /// True when neither list has anything in it.
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty() && self.sprites.is_empty()
    }
}

/// Everything the composition needs that is a property of the *host* rather
/// than of the screen: the font, the resolved window rects, the chrome atlas
/// band rects (absent on a disc-less run) and the stage transform.
pub struct PauseMenuCtx<'a> {
    pub font: &'a legaia_font::Font,
    pub rects: MenuRects<'a>,
    pub chrome: Option<&'a SaveMenuAtlasRects>,
    pub origin: (i32, i32),
    pub scale: u32,
}

impl PauseMenuCtx<'_> {
    /// Whether the sprite pass runs. Panels that have both a sprite label and
    /// an ASCII stand-in key on this: with the atlas resident the stand-in is
    /// suppressed, without it the stand-in carries the layout.
    pub fn chrome_present(&self) -> bool {
        self.chrome.is_some()
    }

    /// ASCII stand-in for a painter's corner cursor sprite.
    fn hand(&self, x: i32, y: i32) -> Vec<TextDraw> {
        crate::text_draws_for(&self.font.layout_ascii(">"), (x, y), crate::MENU_TEXT_GOLD)
    }

    /// A sub-screen's title-tab label.
    ///
    /// The painter is picked by asking the descriptor which renderer it names
    /// rather than by trusting the id: `painter_at` resolves the disc table's
    /// `renderer_va` and hands back the descriptor whose rect the painter
    /// hangs off. A table whose renderer moved (a modded disc), or no disc
    /// table at all, falls back to the pinned rect.
    ///
    /// Living here is the point: the browser page used to call
    /// [`crate::tab_label_draws`] unconditionally while the native window went
    /// through the painter, so a modded disc moved the tab on one host only.
    ///
    /// REF: FUN_801DCAD8 - the Status tab's content renderer.
    fn tab_title(&self, tab_id: usize, label: &str) -> Vec<TextDraw> {
        if let Some((d, _)) = self
            .rects
            .table()
            .and_then(|t| painter_at(t, tab_id, MenuWindowPainter::TitleTab))
        {
            return title_tab_draws_for(self.font, painter_rect(d), label);
        }
        crate::tab_label_draws(self.font, label, self.rects.pen(tab_id))
    }

    /// The 9-slice frames (or carved tab plaques) for a screen's window set,
    /// in retail draw order - a later window's opaque interior occludes an
    /// earlier one.
    ///
    /// The title-tab windows (descriptor ids `0..=4`) wear the carved plaque
    /// instead of the gold 9-slice + filigree frame; retail draws no window
    /// chrome for them beyond the plaque sprites.
    fn window_chrome(&self, ids: &[usize]) -> Vec<SpriteDraw> {
        let Some(rects) = self.chrome else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for &id in ids {
            if id <= window_ids::TAB_OPTIONS {
                let (_, _, w, _) = self.rects.rect(id);
                out.extend(crate::tab_banner_draws(
                    rects,
                    self.rects.pen(id),
                    w,
                    self.origin,
                    self.scale,
                ));
                continue;
            }
            out.extend(crate::menu_window_chrome_draws_for(
                rects,
                self.rects.frame_rect(id),
                self.origin,
                self.scale,
            ));
        }
        out
    }

    /// The single near-fullscreen frame the unpinned screens sit in.
    fn generic_frame(&self) -> Vec<SpriteDraw> {
        let Some(rects) = self.chrome else {
            return Vec::new();
        };
        let (x, y, w, h) = MENU_SUBWINDOW_CONTENT;
        crate::menu_window_chrome_draws_for(
            rects,
            (x - 8, y - 8, w + 16, h + 16),
            self.origin,
            self.scale,
        )
    }

    /// One 9-slice frame around an arbitrary content rect.
    fn frame_around(&self, content: (i32, i32, i32, i32)) -> Vec<SpriteDraw> {
        let Some(rects) = self.chrome else {
            return Vec::new();
        };
        crate::menu_window_chrome_draws_for(
            rects,
            (content.0 - 8, content.1 - 8, content.2 + 16, content.3 + 16),
            self.origin,
            self.scale,
        )
    }
}

/// The Items screen's Use-route confirm (submenu `0xB` Door of Light ->
/// window 10 / `FUN_801D1DAC`, submenu `0xD` Incense -> window 12 /
/// `FUN_801D1F10`). A different window and renderer from the Throw Out
/// confirm, and the cursor seeds to Yes rather than No.
pub struct SpecialConfirmView<'a> {
    /// Prompt lines as the retail renderer stages them: one line for the
    /// window-10 form, two for window 12's indented variant.
    pub lines: &'a [&'a str],
    pub cursor: u8,
}

/// Content of a screen this crate frames but does not lay out itself - the
/// three surfaces whose retail window sets are not capture-pinned.
pub enum GenericContent<'a> {
    /// The Tactical Arts chain editor (an engine extension, no retail row).
    Arts(ArtsEditorDrawArgs<'a>),
    /// The spell menu's target-select stand-in.
    SpellMenu(SpellMenuDrawArgs<'a>),
    /// The generic inventory item-use overlay.
    Inventory(InventoryUseDrawArgs<'a>),
    /// Content the host laid out itself, in unscaled stage pixels.
    ///
    /// One caller: the Items screen's target-select stand-in, whose
    /// projection walks `InventoryUseSession`'s bag directly and so cannot
    /// live in a crate that does not see `engine-core`. The frame and the
    /// stage scale are still this module's; only the glyph layout is not.
    Prebuilt(Vec<TextDraw>),
}

/// Top-level pause menu: the command list, the money / play-time box and the
/// party info panel.
pub struct TopLevelView<'a> {
    pub rows: &'a [FieldMenuRowView<'a>],
    pub cursor: u8,
    pub money: u32,
    pub play_time_seconds: u32,
    pub party: &'a [FieldMenuPartyView<'a>],
    /// Per-member AP, for the info panel's gauges (sprite pass only).
    pub party_ap: &'a [u16],
}

/// Status sub-screen: the main panel plus its three satellite windows.
pub struct StatusScreenView<'a> {
    pub panel: &'a StatusPanelView<'a>,
    pub satellite: &'a StatusSatelliteView<'a>,
    /// Highlighted member's AP, for the main window's gauge sprites.
    pub ap: u16,
    /// Highlighted member's roster character id, for the summary ATR icon.
    pub atr_char: usize,
}

/// Options sub-screen: the settings rows, the per-open value popup and the
/// pointing hand.
pub struct OptionsScreenView<'a> {
    pub rows: &'a [OptionsRowView<'a>],
    pub cursor: u8,
    pub popup: Option<OptionsPopupDraw<'a>>,
    /// Sum of the row advances above the cursor - where the hand sits.
    pub row_y_off: i32,
}

/// Items sub-screen in its browsing form (the target-select beat is
/// [`PauseScreen::ItemsTarget`]).
pub struct ItemsScreenView<'a> {
    pub view: &'a PauseItemsView<'a>,
    /// Live Point Card bank when the staged item is the `0xFE` Point Card
    /// (retail branches the info panel on the id - `FUN_801D0F1C` at
    /// `0x801d0fd0`).
    pub point_card: Option<u32>,
    pub throw_confirm: Option<&'a PauseThrowConfirmView<'a>>,
    pub special_confirm: Option<SpecialConfirmView<'a>>,
}

/// Magic sub-screen in its browsing form.
pub struct MagicScreenView<'a> {
    pub view: &'a PauseMagicView<'a>,
    /// Caster block count, for the hand-cursor sprite pass.
    pub casters: usize,
}

/// Equip sub-screen.
pub struct EquipComposeView<'a> {
    pub view: &'a EquipScreenView<'a>,
    /// Roster slot of the character being edited (party-window hand).
    pub char_slot: usize,
    /// Slot-picker cursor row, or `None` past the slot picker.
    pub slot_cursor: Option<u16>,
    /// Pictogram rows to draw. Retail draws 7; the engine's 8th slot row
    /// stays navigable but icon-less.
    pub pictogram_rows: usize,
}

/// One pause-menu screen, as the composition sees it.
pub enum PauseScreen<'a> {
    TopLevel(TopLevelView<'a>),
    Status(StatusScreenView<'a>),
    Options(OptionsScreenView<'a>),
    Items(ItemsScreenView<'a>),
    /// Window 14, the party target panel that replaces the item list while
    /// the Use flow picks a target.
    ItemsTarget(&'a TargetPanelView<'a>),
    Magic(MagicScreenView<'a>),
    Equip(EquipComposeView<'a>),
    /// A screen framed by the single near-fullscreen window.
    Generic(GenericContent<'a>),
    /// Kind-`0x0D` entry notice (window 6).
    ContextNotice {
        lines: &'a [&'a str],
    },
    /// Kind-`0x0D` ready check (window 5).
    ContextReady {
        headings: [&'a str; 2],
        choices: [&'a str; 2],
        /// The shared cursor word `FUN_801D688C` maintains: low 12 bits are
        /// the selected row, the `0x1000` bit inverts the marker.
        cursor: u32,
    },
}

/// Compose one pause-menu screen into its final draw lists.
///
/// Texts are laid out in 320x240 stage pixels by the panel builders and
/// scaled here, once, at the end; sprite builders take the stage transform
/// directly. That asymmetry is the builders' existing contract, and folding
/// the scale into this one place is what stops a host from scaling twice (or
/// not at all) on a screen the other host got right.
pub fn pause_screen_draws(ctx: &PauseMenuCtx, screen: PauseScreen<'_>) -> PauseMenuDraws {
    let mut texts: Vec<TextDraw> = Vec::new();
    let mut sprites: Vec<SpriteDraw> = Vec::new();
    match screen {
        PauseScreen::TopLevel(v) => {
            texts.extend(crate::field_menu_draws_for(
                ctx.font,
                v.rows,
                v.cursor,
                v.money,
                v.play_time_seconds,
                ctx.rects.pen(window_ids::TOP_COMMAND_LIST),
                ctx.rects.pen(window_ids::TOP_MONEY_TIME),
            ));
            texts.extend(crate::field_menu_info_draws_for(
                ctx.font,
                v.party,
                ctx.rects.pen(window_ids::TOP_INFO_PANEL),
            ));
            sprites.extend(ctx.window_chrome(&legaia_asset::menu_windows::TOP_LEVEL_WINDOWS));
            if let Some(rects) = ctx.chrome {
                sprites.extend(crate::field_menu_icon_sprites_for(
                    rects,
                    v.cursor,
                    v.party_ap,
                    ctx.rects.pen(window_ids::TOP_COMMAND_LIST),
                    ctx.rects.pen(window_ids::TOP_MONEY_TIME),
                    ctx.rects.pen(window_ids::TOP_INFO_PANEL),
                    ctx.origin,
                    ctx.scale,
                ));
            }
        }
        PauseScreen::Status(v) => {
            texts.extend(crate::status_screen_draws_for(
                ctx.font,
                v.panel,
                None,
                ctx.rects.pen(window_ids::STATUS_MAIN),
                ctx.chrome_present(),
            ));
            texts.extend(crate::status_satellite_draws_for(
                ctx.font,
                v.satellite,
                ctx.rects.pen(window_ids::STATUS_PARTY_LIST),
                ctx.rects.pen(window_ids::STATUS_CONDITION),
                ctx.rects.pen(window_ids::STATUS_SUMMARY),
                ctx.chrome_present(),
            ));
            texts.extend(ctx.tab_title(window_ids::TAB_STATUS, "Status"));
            sprites.extend(ctx.window_chrome(&legaia_asset::menu_windows::STATUS_SCREEN_WINDOWS));
            if let Some(rects) = ctx.chrome {
                sprites.extend(crate::status_icon_sprites_for(
                    rects,
                    ctx.rects.pen(window_ids::STATUS_MAIN),
                    v.ap,
                    ctx.origin,
                    ctx.scale,
                ));
                sprites.extend(crate::status_satellite_icon_sprites_for(
                    rects,
                    v.satellite.cursor,
                    v.atr_char,
                    ctx.rects.pen(window_ids::STATUS_PARTY_LIST),
                    ctx.rects.pen(window_ids::STATUS_CONDITION),
                    ctx.rects.pen(window_ids::STATUS_SUMMARY),
                    ctx.origin,
                    ctx.scale,
                ));
            }
        }
        PauseScreen::Options(v) => {
            texts.extend(crate::options_draws_for(
                ctx.font,
                v.rows,
                v.cursor,
                v.popup.as_ref(),
                ctx.rects.pen(window_ids::OPTIONS_MAIN),
            ));
            texts.extend(ctx.tab_title(window_ids::TAB_OPTIONS, "Options"));
            sprites.extend(ctx.window_chrome(&legaia_asset::menu_windows::OPTIONS_SCREEN_WINDOWS));
            if let Some(rects) = ctx.chrome {
                if let Some(p) = v.popup.as_ref() {
                    let (x, y, w, h) = p.rect;
                    sprites.extend(crate::menu_window_chrome_draws_for(
                        rects,
                        (x - 6, y - 2, w + 12, h + 12),
                        ctx.origin,
                        ctx.scale,
                    ));
                }
                sprites.push(crate::options_hand_cursor_sprite(
                    rects,
                    ctx.rects.pen(window_ids::OPTIONS_MAIN),
                    v.row_y_off,
                    ctx.origin,
                    ctx.scale,
                ));
            }
        }
        PauseScreen::Items(v) => {
            let info_pen = ctx.rects.pen(window_ids::ITEMS_INFO);
            texts.extend(crate::items_screen_draws_for(
                ctx.font,
                v.view,
                ctx.rects.pen(window_ids::ITEMS_COMMAND),
                ctx.rects.pen(window_ids::ITEMS_LIST),
                info_pen,
            ));
            if let Some(points) = v.point_card {
                texts.extend(crate::item_points_panel_draws(ctx.font, info_pen, points));
            }
            texts.extend(ctx.tab_title(window_ids::TAB_ITEMS, "Items"));
            if let Some(confirm) = v.throw_confirm {
                texts.extend(crate::items_throw_confirm_draws_for(
                    ctx.font,
                    confirm,
                    ctx.rects.pen(9),
                ));
            }
            let special_pen = v.special_confirm.as_ref().map(|sc| {
                let (win_id, _) = crate::use_confirm_window(sc.lines.len());
                (win_id, ctx.rects.pen(win_id))
            });
            if let (Some(sc), Some((_, pen))) = (v.special_confirm.as_ref(), special_pen) {
                texts.extend(crate::confirm_prompt_draws(
                    ctx.font,
                    sc.lines,
                    &["Yes", "No"],
                    pen,
                ));
                if !ctx.chrome_present() {
                    let (hx, hy) = crate::confirm_prompt_hand_pos(pen, sc.lines.len(), sc.cursor);
                    texts.extend(ctx.hand(hx, hy));
                }
            }
            // Frames first, then the screen's own content sprites, then the
            // modals on top - the order retail draws them in, and the one
            // the two hosts disagreed about (the browser emitted the Use
            // confirm's frame *before* the window set, so the item list's
            // frame painted over it).
            sprites.extend(ctx.window_chrome(&legaia_asset::menu_windows::ITEMS_SCREEN_WINDOWS));
            sprites.extend(ctx.frame_around(crate::ITEMS_INFO_EXTRA_BOX_RECT));
            if let Some(rects) = ctx.chrome {
                sprites.extend(crate::items_screen_sprites_for(
                    rects,
                    v.view.phase,
                    v.view.command_cursor,
                    v.view.list_cursor,
                    v.view.page,
                    v.view.pages,
                    ctx.rects.pen(window_ids::ITEMS_COMMAND),
                    ctx.rects.pen(window_ids::ITEMS_LIST),
                    ctx.origin,
                    ctx.scale,
                ));
                if let Some(confirm) = v.throw_confirm {
                    sprites.extend(crate::items_throw_confirm_sprites_for(
                        rects,
                        confirm.cursor,
                        ctx.rects.pen(9),
                        ctx.origin,
                        ctx.scale,
                    ));
                }
                if let (Some(sc), Some((win_id, pen))) = (v.special_confirm.as_ref(), special_pen) {
                    sprites.extend(ctx.frame_around(ctx.rects.rect(win_id)));
                    let hand = crate::confirm_prompt_hand_pos(pen, sc.lines.len(), sc.cursor);
                    // Retail's per-record quad drawer `FUN_801E3FF0` at the
                    // neutral `0x80` modulation.
                    sprites.push(crate::save_ui_record_quad(
                        rects.cursor,
                        (0x80, 0x80, 0x80),
                        hand,
                        ctx.origin,
                        ctx.scale,
                    ));
                }
            }
        }
        PauseScreen::ItemsTarget(view) => {
            let pen = ctx.rects.pen(14);
            texts.extend(crate::target_panel_draws_for(ctx.font, view, pen));
            texts.extend(ctx.tab_title(window_ids::TAB_ITEMS, "Items"));
            sprites.extend(ctx.window_chrome(&TARGET_SELECT_WINDOWS));
            if let Some(rects) = ctx.chrome {
                sprites.extend(crate::target_panel_sprites_for(
                    rects, view, pen, ctx.origin, ctx.scale,
                ));
            }
        }
        PauseScreen::Magic(v) => {
            texts.extend(crate::magic_screen_draws_for(
                ctx.font,
                v.view,
                ctx.rects.pen(window_ids::MAGIC_CASTER),
                ctx.rects.pen(window_ids::MAGIC_LIST),
                ctx.rects.pen(window_ids::MAGIC_INFO),
            ));
            texts.extend(ctx.tab_title(window_ids::TAB_MAGIC, "Magic"));
            sprites.extend(ctx.window_chrome(&legaia_asset::menu_windows::MAGIC_SCREEN_WINDOWS));
            if let Some(rects) = ctx.chrome {
                sprites.extend(crate::magic_screen_sprites_for(
                    rects,
                    v.casters,
                    v.view.phase,
                    v.view.caster_cursor,
                    v.view.list_cursor,
                    v.view.page,
                    v.view.pages,
                    ctx.rects.pen(window_ids::MAGIC_CASTER),
                    ctx.rects.pen(window_ids::MAGIC_LIST),
                    ctx.origin,
                    ctx.scale,
                ));
            }
        }
        PauseScreen::Equip(v) => {
            texts.extend(crate::equip_screen_draws_for(
                ctx.font,
                v.view,
                ctx.rects.pen(window_ids::EQUIP_PARTY),
                ctx.rects.pen(window_ids::EQUIP_LIST),
                ctx.rects.pen(window_ids::EQUIP_MAIN),
            ));
            texts.extend(ctx.tab_title(window_ids::TAB_EQUIP, "Equip"));
            sprites.extend(ctx.window_chrome(&legaia_asset::menu_windows::EQUIP_SCREEN_WINDOWS));
            if let Some(rects) = ctx.chrome {
                sprites.extend(crate::equip_screen_sprites_for(
                    rects,
                    v.pictogram_rows,
                    ctx.rects.pen(window_ids::EQUIP_MAIN),
                    ctx.rects.pen(window_ids::EQUIP_PARTY),
                    v.char_slot,
                    v.slot_cursor,
                    ctx.origin,
                    ctx.scale,
                ));
            }
        }
        PauseScreen::Generic(content) => {
            sprites.extend(ctx.generic_frame());
            texts.extend(match content {
                GenericContent::Arts(args) => {
                    crate::tactical_arts_editor_draws_for(ctx.font, args, (16, 32))
                }
                GenericContent::SpellMenu(args) => {
                    crate::spell_menu_draws_for(ctx.font, args, (32, 32))
                }
                GenericContent::Inventory(args) => {
                    crate::inventory_use_draws_for(ctx.font, args, (16, 32))
                }
                GenericContent::Prebuilt(draws) => draws,
            });
        }
        PauseScreen::ContextNotice { lines } => {
            let Some((d, _)) = ctx
                .rects
                .table()
                .and_then(|t| painter_at(t, WIN_CONTEXT_NOTICE, MenuWindowPainter::LabelList))
            else {
                return PauseMenuDraws::default();
            };
            let (out, cursor) = label_list_draws_for(ctx.font, painter_rect(d), lines);
            texts.extend(out);
            texts.extend(ctx.hand(cursor.x, cursor.y));
        }
        PauseScreen::ContextReady {
            headings,
            choices,
            cursor,
        } => {
            let Some((d, _)) = ctx.rects.table().and_then(|t| {
                painter_at(t, WIN_CONTEXT_READY, MenuWindowPainter::TwoLineChoicePanel)
            }) else {
                return PauseMenuDraws::default();
            };
            let (out, marks) = two_line_choice_panel_draws_for(
                ctx.font,
                painter_rect(d),
                headings,
                choices,
                ChoiceFlags(cursor),
            );
            texts.extend(out);
            for s in marks {
                texts.extend(ctx.hand(s.x, s.y));
            }
        }
    }
    crate::scale_stage_text_draws(&mut texts, ctx.origin, ctx.scale);
    PauseMenuDraws { texts, sprites }
}

/// Owned-string flavour of the Equip screen's inputs, as
/// `engine-core::pause_screens::equip_screen_model` hands them over.
///
/// The borrow from owned model to `EquipScreenView` was written out twice -
/// once per host - so it lives here beside the composition instead. This
/// crate cannot name the model type (it does not depend on `engine-core`),
/// but it can take its fields as plain slices, which is all the borrow ever
/// needed.
pub struct EquipComposeInput<'a> {
    pub party_names: &'a [String],
    pub slot_labels: &'a [String],
    pub slot_items: &'a [String],
    pub candidate_names: &'a [String],
    pub candidate_counts: &'a [u8],
    /// `(label, current, preview)` rows of the main window's compare block.
    pub stat_compare: &'a [(&'static str, u16, u16)],
    pub phase: crate::EquipDrawPhase,
    pub cursor: u16,
    pub active_slot: u8,
    pub confirm_label: Option<&'a str>,
    pub char_slot: usize,
    pub slot_cursor: Option<u16>,
    pub pictogram_rows: usize,
    /// Emit ASCII `>` cursors (no chrome atlas to draw the hand sprite).
    pub text_cursor: bool,
}

/// Borrow an [`EquipComposeInput`] into the equip screen's view and compose
/// it - the whole Equip sub-screen, texts and sprites.
pub fn equip_screen_compose(ctx: &PauseMenuCtx, input: &EquipComposeInput<'_>) -> PauseMenuDraws {
    let party_names: Vec<&str> = input.party_names.iter().map(String::as_str).collect();
    let slots: Vec<crate::EquipSlotRow<'_>> = input
        .slot_labels
        .iter()
        .zip(input.slot_items.iter())
        .map(|(label, current_name)| crate::EquipSlotRow {
            label,
            current_name,
        })
        .collect();
    let candidates: Vec<crate::EquipCandidateRow<'_>> = input
        .candidate_names
        .iter()
        .zip(input.candidate_counts.iter())
        .map(|(name, count)| crate::EquipCandidateRow {
            name,
            count: *count,
        })
        .collect();
    let stat_compare: Vec<crate::EquipStatRow<'_>> = input
        .stat_compare
        .iter()
        .map(|(label, current, preview)| crate::EquipStatRow {
            label,
            current: *current,
            preview: *preview,
        })
        .collect();
    let view = EquipScreenView {
        party_names: &party_names,
        party_cursor: input.char_slot,
        slots: &slots,
        candidates: &candidates,
        stat_compare: &stat_compare,
        phase: input.phase,
        cursor: input.cursor,
        active_slot: input.active_slot,
        confirm_label: input.confirm_label,
        text_cursor: input.text_cursor,
    };
    pause_screen_draws(
        ctx,
        PauseScreen::Equip(EquipComposeView {
            view: &view,
            char_slot: input.char_slot,
            slot_cursor: input.slot_cursor,
            pictogram_rows: input.pictogram_rows,
        }),
    )
}

/// Window 7 - the spell level-up notice, drawn over whichever menu screen is
/// current while the shared menu runtime holds the beat a leveled menu cast
/// armed.
///
/// Content-only, off the disc-parsed id-7 rect: like the shop's descriptor
/// windows it draws only when the real table is present, because the painter
/// is dispatched on the descriptor's renderer.
pub fn spell_level_notice_draws(ctx: &PauseMenuCtx, line: &str) -> Vec<TextDraw> {
    let Some((d, _)) = ctx
        .rects
        .table()
        .and_then(|t| painter_at(t, WIN_MAGIC_LEVEL_NOTICE, MenuWindowPainter::CharPrompt))
    else {
        return Vec::new();
    };
    let (mut out, cursor) = char_prompt_draws_for(ctx.font, painter_rect(d), line);
    out.extend(ctx.hand(cursor.x, cursor.y));
    crate::scale_stage_text_draws(&mut out, ctx.origin, ctx.scale);
    out
}
