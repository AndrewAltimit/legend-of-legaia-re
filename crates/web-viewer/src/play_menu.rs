//! Browser **pause menu**: the real retail field-menu, rendered from the same
//! `legaia-engine-ui` draw builders the native `play-window` uses.
//!
//! The play page's original pause menu was a lightweight DOM overlay. This
//! module replaces it with the byte-pinned retail chrome: the gold 9-slice
//! window frames + navy filigree come from the disc's menu-UI atlas (PROT 0899
//! plus the PROT.DAT system-UI sheet, assembled by
//! [`legaia_engine_core::save_menu_atlas::build_atlas`]), the glyphs from the
//! proportional dialog font, and every rectangle from the shipped
//! `*_draws_for` functions in `legaia-engine-ui`. The window geometry is the
//! disc-parsed descriptor table ([`legaia_asset::menu_windows`]), with the
//! same pinned fallback the native window keeps.
//!
//! The page drives it exactly like the field: hand it edge-triggered pad words,
//! then blit the two draw lists (sprites off the chrome atlas, texts off the
//! font atlas) over the frozen scene. The top-level command list plus the
//! Items / Magic / Equip / Status / Options sub-screens all run the real
//! [`FieldMenuSubsession`] the native `play-window` builds, and render through
//! the identical `legaia-engine-ui` draw builders - the site is just a
//! different framebuffer over the same menu. Load / Save keep the generic
//! framed window: the play page routes disc-backed saving through its own DOM
//! save-loader, so the in-canvas save-select screen is intentionally not wired.

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_core::equip_session::{EquipSession, EquipState};
use legaia_engine_core::field_menu::FieldMenuRow;
use legaia_engine_core::field_menu_dispatch::{
    self, FieldMenuSubsession, apply_equip_outcome, apply_inventory_outcome, apply_spell_outcome,
    status_snapshots,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::inventory_use::{InventoryUseSession, InventoryUseState};
use legaia_engine_core::options::{OptionsSession, OptionsState};
use legaia_engine_core::save_menu_atlas::{SaveMenuAtlas, build_atlas};
use legaia_engine_core::spell_menu::{SpellMenuPhase, SpellMenuSession};
use legaia_engine_core::status_screen::StatusScreenSession;
use legaia_engine_ui::{
    self as ui, FieldMenuPartyView, FieldMenuRowView, SaveMenuAtlasRects, SpriteDraw,
    StatusPanelView, StatusSatelliteView, StatusStatRow, TextDraw,
};

/// Boot-UI stage the retail menu lays glyphs out in (320x240), upscaled +
/// centred onto the play surface exactly like the native window.
const STAGE_W: u32 = ui::BOOT_UI_STAGE_W;
const STAGE_H: u32 = ui::BOOT_UI_STAGE_H;

/// Near-fullscreen content rect for the screens the native shell frames with a
/// single window rather than a capture-pinned window set: Items / Magic (the
/// generic frame behind `inventory_use_draws_for` / `spell_menu_draws_for`) and
/// the Load / Save placeholder. Mirrors the native window's
/// `MENU_SUBWINDOW_CONTENT`.
const SUBWINDOW_CONTENT: (i32, i32, i32, i32) = (18, 18, 284, 200);

/// Pinned content rects mirroring the disc descriptor table, used when the
/// parsed table is unavailable - byte-identical to the native window's
/// `MENU_WINDOW_FALLBACK`.
#[rustfmt::skip]
const WINDOW_FALLBACK: [(usize, (i32, i32, i32, i32)); 15] = {
    use legaia_asset::menu_windows::window_ids as w;
    [
        (w::TAB_EQUIP, (16, 12, 60, 12)),
        (w::TAB_STATUS, (12, 12, 60, 12)),
        (w::TAB_OPTIONS, (16, 12, 60, 12)),
        (w::EQUIP_PARTY, (14, 42, 80, 38)),
        (w::EQUIP_MAIN, (14, 96, 292, 108)),
        (w::EQUIP_LIST, (174, 22, 132, 182)),
        (w::STATUS_PARTY_LIST, (14, 38, 60, 38)),
        (w::STATUS_CONDITION, (14, 92, 60, 10)),
        (w::STATUS_MAIN, (90, 16, 218, 188)),
        (w::STATUS_SUMMARY, (14, 134, 60, 70)),
        (w::OPTIONS_MAIN, (24, 40, 256, 148)),
        (w::OPTIONS_POPUP, (170, 132, 128, 36)),
        (w::TOP_MONEY_TIME, (24, 178, 104, 24)),
        (w::TOP_COMMAND_LIST, (24, 24, 104, 94)),
        (w::TOP_INFO_PANEL, (144, 24, 152, 180)),
    ]
};

/// The disc-sourced menu chrome (assembled atlas + its band rects) plus the
/// disc-parsed window-descriptor table. Built once, lazily, the first time the
/// menu opens (needs the loaded PROT for the atlas).
pub struct PlayMenuAssets {
    font: legaia_font::Font,
    chrome: Option<(SaveMenuAtlas, SaveMenuAtlasRects)>,
    windows: Option<legaia_asset::menu_windows::MenuWindowTable>,
}

impl PlayMenuAssets {
    /// Shared dialog-font atlas (reused by the boot title screen's text
    /// fallback).
    pub(crate) fn font_ref(&self) -> &legaia_font::Font {
        &self.font
    }

    fn window_rect(&self, id: usize) -> (i32, i32, i32, i32) {
        if let Some(d) = self.windows.as_ref().and_then(|t| t.window(id)) {
            return d.rect();
        }
        WINDOW_FALLBACK
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| *r)
            .unwrap_or(SUBWINDOW_CONTENT)
    }

    fn pen(&self, id: usize) -> (i32, i32) {
        let (x, y, _, _) = self.window_rect(id);
        (x, y)
    }

    /// Frame rect (9-slice chrome box): 8 px past the content rect on each side.
    fn frame_rect(&self, id: usize) -> (i32, i32, i32, i32) {
        let (x, y, w, h) = self.window_rect(id);
        (x - 8, y - 8, w + 16, h + 16)
    }
}

/// Active pause-menu state: cursor over the top-level command list plus the
/// open sub-screen, if any.
pub struct PlayMenu {
    cursor: u8,
    sub: Option<PlaySub>,
}

/// The open sub-screen. Items / Magic / Equip / Status / Options run the real
/// [`FieldMenuSubsession`] the native `play-window` builds - and render through
/// the exact same `legaia-engine-ui` draw builders. `Placeholder` is the
/// generic framed window kept for Load / Save, whose disc-backed save-slot grid
/// is served by the play page's own DOM save-loader rather than the in-canvas
/// save-select screen.
enum PlaySub {
    // Boxed: the sub-session enum is far larger than the `Placeholder` row.
    Session(Box<FieldMenuSubsession>),
    Placeholder(FieldMenuRow),
}

impl PlayMenu {
    fn new() -> Self {
        PlayMenu {
            cursor: 0,
            sub: None,
        }
    }
}

/// Stage origin + integer scale that upscales the 320x240 boot-UI stage to fill
/// the play surface, centred - identical math to the native window's
/// `save_select_stage`.
fn stage_transform(surface_w: u32, surface_h: u32) -> ((i32, i32), u32) {
    let scale = (surface_w / STAGE_W).min(surface_h / STAGE_H).clamp(1, 4);
    let sw = STAGE_W * scale;
    let sh = STAGE_H * scale;
    let x0 = (surface_w as i32 - sw as i32) / 2;
    let y0 = (surface_h as i32 - sh as i32) / 2;
    ((x0, y0), scale)
}

/// `(edge & button)` test on a PSX-encoded pad-edge word.
fn pressed(edge: u16, b: PadButton) -> bool {
    edge & b.mask() != 0
}

/// Serialize one draw quad to JSON. `TextDraw` and `SpriteDraw` are the same
/// shape (`dst` / `src` rect + RGBA tint); the page samples the font atlas for
/// quads in the `texts` list and the chrome atlas for the `sprites` list.
fn quad_json(d: &TextDraw) -> serde_json::Value {
    serde_json::json!({
        "dst": [d.dst.0, d.dst.1, d.dst.2, d.dst.3],
        "src": [d.src.0, d.src.1, d.src.2, d.src.3],
        "color": [d.color[0], d.color[1], d.color[2], d.color[3]],
    })
}

impl LegaiaRuntime {
    /// Build the menu assets on demand (font is always available; chrome +
    /// window table need the loaded PROT). Returns `false` when there is no
    /// disc loaded yet. Crate-visible so the boot title screen can share the
    /// font atlas.
    pub(crate) fn ensure_menu_assets(&mut self) -> bool {
        if self.menu_assets.is_some() {
            return true;
        }
        let font = legaia_font::Font::placeholder();
        // Chrome atlas + window table off the loaded PROT, best-effort: a
        // PROT.DAT-only load may lack the overlay slices, in which case the
        // menu still renders its glyphs (no gold frame).
        let (chrome, windows) = match self.scene_host.as_ref() {
            Some(host) => {
                let idx = &host.index;
                let panel = {
                    let base = legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET as u64;
                    let end = (legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                        + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE)
                        as u64;
                    idx.prot_dat_raw_bytes(base, (end - base) as usize)
                };
                let pill =
                    idx.entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32);
                let chrome = match (panel, pill) {
                    (Ok(panel_bytes), Ok(pill_bytes)) => {
                        match build_atlas(&panel_bytes, &pill_bytes) {
                            Ok(a) => {
                                let rects = save_menu_rects(&a);
                                Some((a, rects))
                            }
                            Err(e) => {
                                crate::console_log(&format!("play menu: chrome atlas failed: {e}"));
                                None
                            }
                        }
                    }
                    _ => None,
                };
                let windows = idx
                    .entry_bytes_extended(
                        legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX as u32,
                    )
                    .ok()
                    .and_then(|b| legaia_asset::menu_windows::parse(&b).ok());
                (chrome, windows)
            }
            None => (None, None),
        };
        self.menu_assets = Some(PlayMenuAssets {
            font,
            chrome,
            windows,
        });
        true
    }

    fn menu_world(&self) -> Option<&legaia_engine_core::world::World> {
        self.scene_host.as_ref().map(|h| &h.world)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Open the retail pause menu. No-op with no disc loaded. The field is
    /// frozen by the page while [`Self::play_menu_is_open`] is true.
    pub fn play_menu_open(&mut self) {
        if !self.ensure_menu_assets() {
            return;
        }
        if self.play_menu.is_none() {
            self.play_menu = Some(PlayMenu::new());
        }
    }

    /// Close the menu (and any open sub-screen).
    pub fn play_menu_close(&mut self) {
        self.play_menu = None;
    }

    pub fn play_menu_is_open(&self) -> bool {
        self.play_menu.is_some()
    }

    /// `true` once the gold chrome atlas resolved from the disc; `false` means
    /// the menu renders glyphs only (PROT.DAT-only load).
    pub fn play_menu_has_chrome(&self) -> bool {
        self.menu_assets
            .as_ref()
            .map(|a| a.chrome.is_some())
            .unwrap_or(false)
    }

    /// The whitewashed font atlas (RGBA8) the text draws sample. Stable across
    /// the session; the page uploads it once.
    pub fn play_menu_font_rgba(&self) -> Vec<u8> {
        self.menu_assets
            .as_ref()
            .map(|a| a.font.atlas_rgba().to_vec())
            .unwrap_or_default()
    }

    /// `[width, height]` of the font atlas.
    pub fn play_menu_font_dims(&self) -> Vec<u32> {
        self.menu_assets
            .as_ref()
            .map(|a| {
                let (w, h) = a.font.atlas_dimensions();
                vec![w, h]
            })
            .unwrap_or_else(|| vec![0, 0])
    }

    /// The assembled menu-chrome atlas (RGBA8) the sprite draws sample. Empty
    /// when no chrome resolved.
    pub fn play_menu_chrome_rgba(&self) -> Vec<u8> {
        self.menu_assets
            .as_ref()
            .and_then(|a| a.chrome.as_ref())
            .map(|(atlas, _)| atlas.rgba.clone())
            .unwrap_or_default()
    }

    /// `[width, height]` of the chrome atlas; `[0, 0]` when none.
    pub fn play_menu_chrome_dims(&self) -> Vec<u32> {
        self.menu_assets
            .as_ref()
            .and_then(|a| a.chrome.as_ref())
            .map(|(atlas, _)| vec![atlas.width, atlas.height])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Drive the menu one frame from an edge-triggered PSX pad word (same bit
    /// layout as [`Self::set_pad`]). Navigation:
    /// - top-level: Up/Down move the cursor, Cross opens the row, Circle closes.
    /// - a sub-screen: routes the edges to its session; Circle (or the session
    ///   finishing) drops back to the top-level list.
    pub fn play_menu_input(&mut self, edge: u16) {
        if self.play_menu.is_none() {
            return;
        }
        // Sub-screen active: route to its session, then check for exit.
        let has_sub = self
            .play_menu
            .as_ref()
            .map(|m| m.sub.is_some())
            .unwrap_or(false);
        if has_sub {
            // Placeholder rows (Load / Save) close on Circle; the real
            // sub-sessions tick their edge and self-report `is_done`.
            let mut close_placeholder: Option<FieldMenuRow> = None;
            let mut session_done = false;
            if let Some(sub) = self.play_menu.as_mut().and_then(|m| m.sub.as_mut()) {
                match sub {
                    PlaySub::Session(session) => {
                        session.tick_pad_edge(edge);
                        session_done = session.is_done();
                    }
                    PlaySub::Placeholder(row) => {
                        if pressed(edge, PadButton::Circle) {
                            close_placeholder = Some(*row);
                        }
                    }
                }
            }
            if let Some(row) = close_placeholder
                && let Some(m) = self.play_menu.as_mut()
            {
                m.sub = None;
                m.cursor = row.index();
            } else if session_done {
                // Fold the finished session's result into the live world
                // (equip swap / item use / spell cast) exactly as the native
                // shell does, then drop back to the top-level list on the row
                // that opened it.
                let sub = self.play_menu.as_mut().and_then(|m| m.sub.take());
                if let Some(PlaySub::Session(session)) = sub {
                    let session = *session;
                    let back = session.row();
                    if let Some(host) = self.scene_host.as_mut() {
                        let world = &mut host.world;
                        match session {
                            FieldMenuSubsession::Equip { session, char_slot } => {
                                apply_equip_outcome(&session, char_slot, world);
                            }
                            FieldMenuSubsession::Items(s) => apply_inventory_outcome(&s, world),
                            FieldMenuSubsession::Spells(s) => apply_spell_outcome(&s, world),
                            // Status / Options / Save / Arts carry no
                            // world-mutating outcome on close.
                            _ => {}
                        }
                    }
                    if let Some(m) = self.play_menu.as_mut() {
                        m.cursor = back.index();
                    }
                }
            }
            return;
        }

        // Top-level command list.
        let n = FieldMenuRow::ALL.len() as u8;
        if pressed(edge, PadButton::Up)
            && let Some(m) = self.play_menu.as_mut()
        {
            m.cursor = (m.cursor + n - 1) % n;
        }
        if pressed(edge, PadButton::Down)
            && let Some(m) = self.play_menu.as_mut()
        {
            m.cursor = (m.cursor + 1) % n;
        }
        if pressed(edge, PadButton::Circle) {
            self.play_menu = None;
            return;
        }
        if pressed(edge, PadButton::Cross) {
            let cursor = self.play_menu.as_ref().map(|m| m.cursor).unwrap_or(0);
            let row = FieldMenuRow::from_index(cursor).unwrap_or(FieldMenuRow::Items);
            let sub = match row {
                // Load / Save keep the generic frame (the play page's own
                // save-loader owns the disc-backed slot grid).
                FieldMenuRow::Load | FieldMenuRow::Save => Some(PlaySub::Placeholder(row)),
                // Every other row builds the real retail sub-session from the
                // disc catalogs installed on the host world at `load_disc`
                // (spell / equipment / item), matching the native shell's
                // `FieldMenuSubsession::build`.
                other => self.scene_host.as_ref().map(|host| {
                    let world = &host.world;
                    let chain = world.chain_library();
                    PlaySub::Session(Box::new(FieldMenuSubsession::build(
                        other,
                        world,
                        &OptionsState::default(),
                        &[],
                        &chain,
                        &world.spell_catalog,
                        &world.equipment_table,
                    )))
                }),
            };
            if let Some(sub) = sub
                && let Some(m) = self.play_menu.as_mut()
            {
                m.sub = Some(sub);
            }
        }
    }

    /// Build the two draw lists for the current menu state, in surface pixels.
    /// Shape:
    /// ```text
    /// { "open": true,
    ///   "sprites": [ { "dst":[x,y,w,h], "src":[x,y,w,h], "color":[r,g,b,a] } ],
    ///   "texts":   [ ... ] }
    /// ```
    /// `sprites` sample the chrome atlas, `texts` the font atlas. `open` is
    /// `false` (and the lists empty) when no menu is up.
    pub fn play_menu_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        let (Some(menu), Some(assets)) = (self.play_menu.as_ref(), self.menu_assets.as_ref())
        else {
            return r#"{"open":false,"sprites":[],"texts":[]}"#.to_string();
        };
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let mut sprites: Vec<SpriteDraw> = Vec::new();
        let mut texts: Vec<TextDraw> = Vec::new();

        match &menu.sub {
            None => self.build_top_level(assets, menu, &mut sprites, &mut texts, origin, scale),
            Some(PlaySub::Session(sub)) => match sub.as_ref() {
                FieldMenuSubsession::Status(s) => {
                    self.build_status(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Config(s) => {
                    self.build_config(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Items(s) => {
                    self.build_items(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Spells(s) => {
                    self.build_spells(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Equip { session, char_slot } => self.build_equip(
                    assets,
                    session,
                    *char_slot,
                    &mut sprites,
                    &mut texts,
                    origin,
                    scale,
                ),
                // Save / Arts are never built from a pause-menu row on the
                // site (Load / Save use `Placeholder`; Arts has no retail
                // row), but keep an exhaustive fallback.
                FieldMenuSubsession::Save(_) | FieldMenuSubsession::Arts(_) => self
                    .build_placeholder(assets, sub.row(), &mut sprites, &mut texts, origin, scale),
            },
            Some(PlaySub::Placeholder(row)) => {
                self.build_placeholder(assets, *row, &mut sprites, &mut texts, origin, scale)
            }
        }

        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

impl LegaiaRuntime {
    /// Top-level command list + money/time box + party info panel, with gold
    /// window chrome + the cursor / icon sprites. Mirrors the native window's
    /// `BootUiState::FieldMenu { sub: None }` path.
    fn build_top_level(
        &self,
        assets: &PlayMenuAssets,
        menu: &PlayMenu,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        use legaia_asset::menu_windows::window_ids;
        let Some(world) = self.menu_world() else {
            return;
        };
        let font = &assets.font;
        let money = world.money.max(0) as u32;
        // The play page tracks no wall-clock play timer; surface the engine
        // frame count as a seconds proxy so the H:MM:SS box reads live.
        let play_time = (world.frame / 60) as u32;

        let rows: Vec<FieldMenuRowView<'_>> = FieldMenuRow::ALL
            .iter()
            .map(|r| FieldMenuRowView {
                label: r.label(),
                enabled: true,
            })
            .collect();
        let mut d = ui::field_menu_draws_for(
            font,
            &rows,
            menu.cursor,
            money,
            play_time,
            assets.pen(window_ids::TOP_COMMAND_LIST),
            assets.pen(window_ids::TOP_MONEY_TIME),
        );
        let snaps = status_snapshots(world);
        let party: Vec<FieldMenuPartyView<'_>> = snaps
            .iter()
            .map(|s| FieldMenuPartyView {
                name: &s.name,
                level: s.level,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap: s.ap as u16,
            })
            .collect();
        d.extend(ui::field_menu_info_draws_for(
            font,
            &party,
            assets.pen(window_ids::TOP_INFO_PANEL),
        ));
        ui::scale_stage_text_draws(&mut d, origin, scale);
        texts.extend(d);

        if let Some((_, rects)) = assets.chrome.as_ref() {
            for &id in &legaia_asset::menu_windows::TOP_LEVEL_WINDOWS {
                sprites.extend(ui::menu_window_chrome_draws_for(
                    rects,
                    assets.frame_rect(id),
                    origin,
                    scale,
                ));
            }
            let party_ap: Vec<u16> = snaps.iter().map(|s| s.ap as u16).collect();
            sprites.extend(ui::field_menu_icon_sprites_for(
                rects,
                menu.cursor,
                &party_ap,
                assets.pen(window_ids::TOP_COMMAND_LIST),
                assets.pen(window_ids::TOP_MONEY_TIME),
                assets.pen(window_ids::TOP_INFO_PANEL),
                origin,
                scale,
            ));
        }
    }

    /// Status sub-screen: the main panel + the three satellite windows + the
    /// Status tab, with the LV/HP/MP + AP-gauge + element icon sprites.
    /// Mirrors the native window's `FieldMenuSubsession::Status` path.
    fn build_status(
        &self,
        assets: &PlayMenuAssets,
        s: &StatusScreenSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        use legaia_asset::menu_windows::window_ids;
        let font = &assets.font;
        let has_chrome = assets.chrome.is_some();
        let Some(snap) = s.current() else {
            return;
        };
        let stat_rows: Vec<StatusStatRow<'_>> = snap
            .stats
            .iter()
            .zip(snap.stat_labels.iter())
            .map(|((live, growth), l)| StatusStatRow {
                label: l,
                value: *live as u32,
                growth: *growth as u32,
            })
            .collect();
        let equip_rows: Vec<(&str, &str)> = snap
            .equip
            .iter()
            .map(|e| (e.label, e.item_name.as_str()))
            .collect();
        let view = StatusPanelView {
            name: &snap.name,
            level: snap.level,
            xp: snap.xp,
            xp_to_next: snap.xp_to_next,
            hp: snap.hp,
            hp_max: snap.hp_max,
            mp: snap.mp,
            mp_max: snap.mp_max,
            ap: snap.ap,
            ap_max: snap.ap_max,
            stat_rows: &stat_rows,
            equip_rows: &equip_rows,
        };
        let mut d = ui::status_screen_draws_for(
            font,
            &view,
            None,
            assets.pen(window_ids::STATUS_MAIN),
            has_chrome,
        );
        let names: Vec<&str> = s.snapshots().iter().map(|m| m.name.as_str()).collect();
        let sat = StatusSatelliteView {
            party_names: &names,
            cursor: s.cursor() as usize,
            name: &snap.name,
            level: snap.level,
        };
        d.extend(ui::status_satellite_draws_for(
            font,
            &sat,
            assets.pen(window_ids::STATUS_PARTY_LIST),
            assets.pen(window_ids::STATUS_CONDITION),
            assets.pen(window_ids::STATUS_SUMMARY),
            has_chrome,
        ));
        d.extend(ui::text_draws_for(
            &font.layout_ascii("Status"),
            assets.pen(window_ids::TAB_STATUS),
            ui::MENU_TEXT_WHITE,
        ));
        ui::scale_stage_text_draws(&mut d, origin, scale);
        texts.extend(d);

        if let Some((_, rects)) = assets.chrome.as_ref() {
            for &id in &legaia_asset::menu_windows::STATUS_SCREEN_WINDOWS {
                if id <= window_ids::TAB_OPTIONS {
                    let (_, _, w, _) = assets.window_rect(id);
                    sprites.extend(ui::tab_banner_draws(
                        rects,
                        assets.pen(id),
                        w,
                        origin,
                        scale,
                    ));
                } else {
                    sprites.extend(ui::menu_window_chrome_draws_for(
                        rects,
                        assets.frame_rect(id),
                        origin,
                        scale,
                    ));
                }
            }
            let ap = snap.ap as u16;
            sprites.extend(ui::status_icon_sprites_for(
                rects,
                assets.pen(window_ids::STATUS_MAIN),
                ap,
                origin,
                scale,
            ));
            let atr_char = snap.slot as usize;
            sprites.extend(ui::status_satellite_icon_sprites_for(
                rects,
                s.cursor() as usize,
                atr_char,
                assets.pen(window_ids::STATUS_PARTY_LIST),
                assets.pen(window_ids::STATUS_CONDITION),
                assets.pen(window_ids::STATUS_SUMMARY),
                origin,
                scale,
            ));
        }
    }

    /// Options sub-screen: the settings rows + value popup + the hand cursor,
    /// with the options window frame + tab. Mirrors the native window's
    /// `FieldMenuSubsession::Config` path.
    fn build_config(
        &self,
        assets: &PlayMenuAssets,
        s: &OptionsSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        use legaia_asset::menu_windows::window_ids;
        let font = &assets.font;
        let rows = s.state().rows();
        let row_views: Vec<ui::OptionsRowView<'_>> = rows
            .iter()
            .map(|r| ui::OptionsRowView {
                label: r.label,
                value: r.value,
                teal: r.teal,
                advance: r.advance,
            })
            .collect();
        let popup_rect = s.popup().map(|p| self.options_popup_rect(assets, &p));
        let popup = s
            .popup()
            .zip(popup_rect)
            .map(|(p, rect)| ui::OptionsPopupDraw {
                rect,
                choices: p.choices,
                cursor: p.cursor,
            });
        let mut d = ui::options_draws_for(
            font,
            &row_views,
            s.cursor(),
            popup.as_ref(),
            assets.pen(window_ids::OPTIONS_MAIN),
        );
        d.extend(ui::text_draws_for(
            &font.layout_ascii("Options"),
            assets.pen(window_ids::TAB_OPTIONS),
            ui::MENU_TEXT_WHITE,
        ));
        ui::scale_stage_text_draws(&mut d, origin, scale);
        texts.extend(d);

        if let Some((_, rects)) = assets.chrome.as_ref() {
            for &id in &legaia_asset::menu_windows::OPTIONS_SCREEN_WINDOWS {
                if id <= window_ids::TAB_OPTIONS {
                    let (_, _, w, _) = assets.window_rect(id);
                    sprites.extend(ui::tab_banner_draws(
                        rects,
                        assets.pen(id),
                        w,
                        origin,
                        scale,
                    ));
                } else {
                    sprites.extend(ui::menu_window_chrome_draws_for(
                        rects,
                        assets.frame_rect(id),
                        origin,
                        scale,
                    ));
                }
            }
            if let Some(p) = s.popup() {
                let (x, y, w, h) = self.options_popup_rect(assets, &p);
                sprites.extend(ui::menu_window_chrome_draws_for(
                    rects,
                    (x - 6, y - 2, w + 12, h + 12),
                    origin,
                    scale,
                ));
            }
            let row_y_off: i32 = rows
                .iter()
                .take(s.cursor() as usize)
                .map(|r| r.advance)
                .sum();
            sprites.push(ui::options_hand_cursor_sprite(
                rects,
                assets.pen(window_ids::OPTIONS_MAIN),
                row_y_off,
                origin,
                scale,
            ));
        }
    }

    /// Generic near-fullscreen frame + a centred label. Used for Load / Save
    /// (whose disc-backed slot grid is served by the page's DOM save-loader) -
    /// byte-identical to the native window's treatment of an unpinned screen.
    fn build_placeholder(
        &self,
        assets: &PlayMenuAssets,
        row: FieldMenuRow,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        if let Some((_, rects)) = assets.chrome.as_ref() {
            let (x, y, w, h) = SUBWINDOW_CONTENT;
            sprites.extend(ui::menu_window_chrome_draws_for(
                rects,
                (x - 8, y - 8, w + 16, h + 16),
                origin,
                scale,
            ));
        }
        let mut d = ui::text_draws_for(
            &assets.font.layout_ascii(row.label()),
            (140, 108),
            ui::MENU_TEXT_WHITE,
        );
        ui::scale_stage_text_draws(&mut d, origin, scale);
        texts.extend(d);
    }

    /// Items sub-screen: the inventory-use overlay + the generic frame chrome.
    /// Mirrors the native window's `FieldMenuSubsession::Items` path
    /// (`items_session_draws` -> `inventory_use_draws_for`).
    fn build_items(
        &self,
        assets: &PlayMenuAssets,
        s: &InventoryUseSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let mut d = self.items_session_draws(assets, s);
        ui::scale_stage_text_draws(&mut d, origin, scale);
        texts.extend(d);
        if let Some((_, rects)) = assets.chrome.as_ref() {
            let (x, y, w, h) = SUBWINDOW_CONTENT;
            sprites.extend(ui::menu_window_chrome_draws_for(
                rects,
                (x - 8, y - 8, w + 16, h + 16),
                origin,
                scale,
            ));
        }
    }

    /// Magic sub-screen: the caster/spell/target list + the generic frame.
    /// Mirrors the native window's `FieldMenuSubsession::Spells` path
    /// (`spell_menu_draws_for`).
    fn build_spells(
        &self,
        assets: &PlayMenuAssets,
        s: &SpellMenuSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let font = &assets.font;
        let names: Vec<&str> = s.party().iter().map(|c| c.name.as_str()).collect();
        let hp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.hp, c.hp)).collect();
        let mp: Vec<(u16, u16)> = s.party().iter().map(|c| (c.mp, c.mp)).collect();
        let spell_rows = s.current_spell_rows();
        let spell_views: Vec<ui::SpellRowView<'_>> = spell_rows
            .iter()
            .map(|sr| ui::SpellRowView {
                name: sr.name.as_str(),
                mp_cost: sr.mp_cost,
                admissible: sr.admissible,
            })
            .collect();
        let target_views: Vec<ui::SpellTargetView<'_>> = s
            .targets()
            .iter()
            .map(|t| ui::SpellTargetView {
                name: t.name.as_str(),
                hp: t.hp,
                hp_max: t.hp_max,
                alive: t.alive(),
            })
            .collect();
        let (selected_caster, selected_spell, phase, cursor) = match s.phase() {
            SpellMenuPhase::CharSelect { cursor } => (None, None, 0u8, *cursor),
            SpellMenuPhase::SpellSelect { caster, cursor } => (Some(*caster), None, 1u8, *cursor),
            SpellMenuPhase::TargetSelect {
                caster,
                spell_id,
                cursor,
            } => (Some(*caster), Some(*spell_id), 2u8, *cursor),
            SpellMenuPhase::Done(_) => return,
        };
        let args = ui::SpellMenuDrawArgs {
            party_names: &names,
            party_hp: &hp,
            party_mp: &mp,
            selected_caster,
            spells: &spell_views,
            selected_spell,
            targets: &target_views,
            selected_target: None,
            cursor,
            phase,
        };
        let mut d = ui::spell_menu_draws_for(font, args, (32, 32));
        ui::scale_stage_text_draws(&mut d, origin, scale);
        texts.extend(d);
        if let Some((_, rects)) = assets.chrome.as_ref() {
            let (x, y, w, h) = SUBWINDOW_CONTENT;
            sprites.extend(ui::menu_window_chrome_draws_for(
                rects,
                (x - 8, y - 8, w + 16, h + 16),
                origin,
                scale,
            ));
        }
    }

    /// Equip sub-screen: the retail multi-window layout (party / item-list /
    /// main window + the Equip tab) + the slot pictogram column and hand
    /// cursors. Mirrors the native window's `FieldMenuSubsession::Equip` path
    /// (`equip_session_draws` -> `equip_screen_draws_for` +
    /// `equip_screen_sprites_for`).
    #[allow(clippy::too_many_arguments)]
    fn build_equip(
        &self,
        assets: &PlayMenuAssets,
        session: &EquipSession,
        char_slot: u8,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        use legaia_asset::menu_windows::window_ids;
        let mut d = self.equip_session_draws(assets, session, char_slot);
        ui::scale_stage_text_draws(&mut d, origin, scale);
        texts.extend(d);

        let Some((_, rects)) = assets.chrome.as_ref() else {
            return;
        };
        for &id in &legaia_asset::menu_windows::EQUIP_SCREEN_WINDOWS {
            if id <= window_ids::TAB_OPTIONS {
                let (_, _, w, _) = assets.window_rect(id);
                sprites.extend(ui::tab_banner_draws(
                    rects,
                    assets.pen(id),
                    w,
                    origin,
                    scale,
                ));
            } else {
                sprites.extend(ui::menu_window_chrome_draws_for(
                    rects,
                    assets.frame_rect(id),
                    origin,
                    scale,
                ));
            }
        }
        let slot_cursor = match session.state() {
            EquipState::SlotPicker { cursor } => Some(cursor as u16),
            _ => None,
        };
        // Retail draws 7 pictogram rows (the 8th slot stays navigable but
        // icon-less), matching `field_menu_chrome_sprite_draws`.
        let n_rows = session.record().equip.len().min(7);
        sprites.extend(ui::equip_screen_sprites_for(
            rects,
            n_rows,
            assets.pen(window_ids::EQUIP_MAIN),
            assets.pen(window_ids::EQUIP_PARTY),
            char_slot as usize,
            slot_cursor,
            origin,
            scale,
        ));
    }

    /// Build the inventory item-use overlay text draws. Ported verbatim from
    /// the native shell's `items_session_draws` so the site emits the identical
    /// draw list.
    fn items_session_draws(
        &self,
        assets: &PlayMenuAssets,
        s: &InventoryUseSession,
    ) -> Vec<TextDraw> {
        let font = &assets.font;
        let filter_set: std::collections::HashSet<usize> =
            s.filtered_items.iter().copied().collect();
        let mut counts: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
        for id in &s.items {
            *counts.entry(*id).or_insert(0) =
                counts.get(id).copied().unwrap_or(0).saturating_add(1);
        }
        let mut seen: std::collections::HashSet<u8> = std::collections::HashSet::new();
        let mut row_data: Vec<(String, u8, bool)> = Vec::new();
        for (i, id) in s.items.iter().enumerate() {
            if !seen.insert(*id) {
                continue;
            }
            let entry = s.catalog.get(*id);
            let name = entry
                .map(|e| e.name.to_string())
                .unwrap_or_else(|| format!("Item {id:02X}"));
            let count = counts.get(id).copied().unwrap_or(1);
            let admissible = filter_set.contains(&i);
            row_data.push((name, count, admissible));
        }
        let item_rows: Vec<ui::InventoryItemRow<'_>> = row_data
            .iter()
            .map(|(n, c, a)| ui::InventoryItemRow {
                name: n,
                count: *c,
                admissible: *a,
            })
            .collect();
        let target_rows: Vec<ui::InventoryTargetRow<'_>> = s
            .targets
            .iter()
            .map(|t| ui::InventoryTargetRow {
                name: &t.name,
                hp: t.hp,
                hp_max: t.hp_max,
                mp: t.mp,
                mp_max: t.mp_max,
                alive: t.alive,
            })
            .collect();
        let (phase, cursor) = match s.state {
            InventoryUseState::Browsing { cursor } => (0u8, cursor as u8),
            InventoryUseState::TargetSelect { cursor, .. } => (1u8, cursor as u8),
            _ => (0u8, 0),
        };
        let selected_item_name = s.current_item().map(|e| e.name);
        let in_battle = matches!(
            s.context,
            legaia_engine_core::inventory_use::InventoryContext::Battle
        );
        let args = ui::InventoryUseDrawArgs {
            items: &item_rows,
            targets: &target_rows,
            in_battle,
            cursor,
            phase,
            selected_item_name,
        };
        ui::inventory_use_draws_for(font, args, (16, 32))
    }

    /// Build the equip-screen text draws. Ported from the native shell's
    /// `equip_session_draws`; resolves the same three retail stat-compare rows
    /// off `compute_battle_stats`.
    fn equip_session_draws(
        &self,
        assets: &PlayMenuAssets,
        session: &EquipSession,
        char_slot: u8,
    ) -> Vec<TextDraw> {
        use legaia_asset::menu_windows::window_ids;
        use legaia_engine_core::equipment::EquipSlot;
        let font = &assets.font;

        let names = self
            .menu_world()
            .map(field_menu_dispatch::roster_names)
            .unwrap_or_default();
        let party_names: Vec<&str> = names.iter().map(String::as_str).collect();

        let record = session.record();
        let mut slot_label_buf: Vec<String> = Vec::with_capacity(8);
        for i in 0..8u8 {
            let label = EquipSlot::from_index(i)
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| format!("Slot {i}"));
            slot_label_buf.push(label);
        }
        let mut slot_item_buf: Vec<String> = Vec::with_capacity(8);
        for &id in record.equip.iter() {
            slot_item_buf.push(if id == 0 {
                String::new()
            } else {
                format!("Item {id:02X}")
            });
        }
        let slot_rows: Vec<ui::EquipSlotRow<'_>> = (0..8usize)
            .map(|i| ui::EquipSlotRow {
                label: &slot_label_buf[i],
                current_name: &slot_item_buf[i],
            })
            .collect();

        let (phase, cursor, active_slot, confirm_label_owned) = match session.state() {
            EquipState::SlotPicker { cursor } => {
                (ui::EquipDrawPhase::SlotPicker, cursor as u16, cursor, None)
            }
            EquipState::ItemPicker { slot, cursor } => {
                (ui::EquipDrawPhase::ItemPicker, cursor, slot, None)
            }
            EquipState::Confirm {
                slot,
                item_id,
                cursor,
            } => {
                let label = format!("Equip Item {item_id:02X}?");
                (
                    ui::EquipDrawPhase::Confirm,
                    cursor as u16,
                    slot,
                    Some(label),
                )
            }
            EquipState::Done(_) => (ui::EquipDrawPhase::SlotPicker, 0, 0, None),
        };

        let (candidate_names, candidate_counts, considered_id): (Vec<String>, Vec<u8>, Option<u8>) =
            if phase == ui::EquipDrawPhase::SlotPicker {
                (Vec::new(), Vec::new(), None)
            } else {
                let items = session.items_for_slot(active_slot);
                let names: Vec<String> = items
                    .iter()
                    .map(|it| format!("Item {:02X}", it.id))
                    .collect();
                let counts: Vec<u8> = items
                    .iter()
                    .map(|it| session.inventory().get(&it.id).copied().unwrap_or(0))
                    .collect();
                let considered = match session.state() {
                    EquipState::Confirm { item_id, .. } => Some(item_id),
                    _ => items.get(cursor as usize).map(|it| it.id),
                };
                (names, counts, considered)
            };
        let candidate_rows: Vec<ui::EquipCandidateRow<'_>> = candidate_names
            .iter()
            .zip(candidate_counts.iter())
            .map(|(name, count)| ui::EquipCandidateRow {
                name,
                count: *count,
            })
            .collect();

        let stat_compare: Vec<ui::EquipStatRow<'_>> = match considered_id {
            Some(id) => {
                let neutral = legaia_engine_core::battle_stats::StatusModifiers::default();
                let cur = legaia_engine_core::battle_stats::compute_battle_stats(
                    record,
                    session.equipment(),
                    &[],
                    &neutral,
                );
                let mut copy = *record;
                copy.equip[active_slot as usize] = id;
                let new = legaia_engine_core::battle_stats::compute_battle_stats(
                    &copy,
                    session.equipment(),
                    &[],
                    &neutral,
                );
                vec![
                    ui::EquipStatRow {
                        label: "ATK",
                        current: cur.atk,
                        preview: new.atk,
                    },
                    ui::EquipStatRow {
                        label: "UDF",
                        current: cur.udf,
                        preview: new.udf,
                    },
                    ui::EquipStatRow {
                        label: "LDF",
                        current: cur.ldf,
                        preview: new.ldf,
                    },
                ]
            }
            None => Vec::new(),
        };

        let view = ui::EquipScreenView {
            party_names: &party_names,
            party_cursor: char_slot as usize,
            slots: &slot_rows,
            candidates: &candidate_rows,
            stat_compare: &stat_compare,
            phase,
            cursor,
            active_slot,
            confirm_label: confirm_label_owned.as_deref(),
            // Hand-cursor sprites come from the chrome atlas when resident.
            text_cursor: assets.chrome.is_none(),
        };
        let mut d = ui::equip_screen_draws_for(
            font,
            &view,
            assets.pen(window_ids::EQUIP_PARTY),
            assets.pen(window_ids::EQUIP_LIST),
            assets.pen(window_ids::EQUIP_MAIN),
        );
        d.extend(ui::text_draws_for(
            &font.layout_ascii("Equip"),
            assets.pen(window_ids::TAB_EQUIP),
            ui::MENU_TEXT_WHITE,
        ));
        d
    }

    /// The options value popup's per-open content rect (its y/h are stamped
    /// from the hovered row) - same helper the native window uses.
    fn options_popup_rect(
        &self,
        assets: &PlayMenuAssets,
        popup: &legaia_engine_core::options::OptionsPopup,
    ) -> (i32, i32, i32, i32) {
        use legaia_asset::menu_windows::window_ids;
        let (px, _, pw, _) = assets.window_rect(window_ids::OPTIONS_POPUP);
        let (_, sy, _, _) = assets.window_rect(window_ids::OPTIONS_MAIN);
        legaia_engine_core::options::options_popup_content_rect(
            sy,
            px,
            pw,
            popup.row,
            popup.choices.len(),
        )
    }
}

/// Assemble the `SaveMenuAtlasRects` band table from a built [`SaveMenuAtlas`] -
/// the same field-by-field mapping the native window does at atlas upload.
fn save_menu_rects(a: &SaveMenuAtlas) -> SaveMenuAtlasRects {
    SaveMenuAtlasRects {
        panel_tl: a.band_panel_tl(),
        panel_tr: a.band_panel_tr(),
        panel_bl: a.band_panel_bl(),
        panel_br: a.band_panel_br(),
        panel_top: a.band_panel_top(),
        panel_bot: a.band_panel_bot(),
        panel_left: a.band_panel_left(),
        panel_right: a.band_panel_right(),
        slot1: a.band_slot1(),
        slot2: a.band_slot2(),
        cursor: a.band_cursor(),
        panel_interior: a.band_panel_interior(),
        panel_filigree: a.band_panel_filigree(),
        label_lv: a.band_label_lv(),
        label_hp: a.band_label_hp(),
        label_mp: a.band_label_mp(),
        icon_money: a.band_icon_money(),
        label_time: a.band_label_time(),
        label_coin: a.band_label_coin(),
        gauge_cap: a.band_gauge_cap(),
        gauge_trough: a.band_gauge_trough(),
        gauge_box: a.band_gauge_box(),
        gauge_tip: a.band_gauge_tip(),
        gauge_digits: a.band_gauge_digits(),
        gauge_100: a.band_gauge_100(),
        gauge_fill: a.band_gauge_fill(),
        dialog_fill: a.band_dialog_fill(),
        icon_weapon: a.band_icon_weapon(),
        icon_helmet: a.band_icon_helmet(),
        icon_armor: a.band_icon_armor(),
        icon_boot: a.band_icon_boot(),
        icon_goods: a.band_icon_goods(),
        pager_left: a.band_pager_left(),
        pager_right: a.band_pager_right(),
        tab_cap_l: a.band_tab_cap_l(),
        tab_body: a.band_tab_body(),
        tab_cap_r: a.band_tab_cap_r(),
        atr_icons: a.band_atr_icons(),
        load_empty_frame: Some(a.band_load_empty_frame()),
        load_portrait_by_char: [
            a.band_load_portrait(0),
            a.band_load_portrait(1),
            a.band_load_portrait(2),
        ],
    }
}
