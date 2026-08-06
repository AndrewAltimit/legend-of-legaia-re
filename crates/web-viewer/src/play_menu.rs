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
//! The **root command list is the shared retail picker**
//! ([`FieldMenuSession`]), not a page-local cursor: [`Self::play_menu_open`]
//! seeds it with the world's money + play time, samples the two row gates
//! retail reads at every draw into a [`FieldMenuGate`] (the op-`0x49` entry
//! context and the scene MAN's save-allow bit) and switches the world into
//! [`SceneMode::Menu`], which is the same construction
//! `BootSession::open_field_menu` performs for the native window. So the row
//! ink and the confirm routing come from one `root_menu_confirm_route` call
//! per row on both hosts, and a row cannot draw white here and buzz there -
//! which it did, letting a player Save in a scene whose own data forbids it.
//!
//! The page drives it exactly like the field: hand it edge-triggered pad words,
//! then blit the two draw lists (sprites off the chrome atlas, texts off the
//! font atlas) over the frozen scene. Every row - the top-level command list
//! plus the Items / Magic / Equip / Status / Options / Load / Save
//! sub-screens - runs the real [`FieldMenuSubsession`] the native
//! `play-window` builds, and renders through the identical
//! `legaia-engine-ui` draw builders; the site is just a different framebuffer
//! over the same menu.
//!
//! ## What this module still decides
//!
//! Not the composition. Which windows a screen frames, which painter draws its
//! title tab, where the modals sit in the sprite order and the final 320x240
//! stage scale all live in
//! [`legaia_engine_ui::pause_menu`](legaia_engine_ui::pause_menu), shared with
//! the native window - this file used to carry a private copy, and two
//! divergences had already grown in it (the tab painter and the Use-confirm
//! frame order). What is left here is the **projection**: turning a live
//! `engine-core` session into the plain view structs the composition takes,
//! which needs `engine-core` types the wgpu-free UI crate deliberately does
//! not depend on.
//!
//! ## Load / Save
//!
//! Load and Save drive the retail save-select screen ([`SaveSelectSession`],
//! `docs/subsystems/save-screen.md`) against the page's **memory-card rack**
//! ([`crate::cards`]) in its two-stage card-slots mode:
//!
//! 1. **Browsing** - the `SLOT 1` / `SLOT 2` pills are the console's two
//!    memory-card ports. A pill is selectable when the page has inserted a
//!    card image there.
//! 2. **NowChecking** - the "Now checking. Do not remove MEMORY CARD" dialog
//!    slides in while the card is read.
//! 3. **SlotPreview** - the card's fifteen blocks as retail's 5x3 portrait
//!    grid, with the focused block's info panel sliding up underneath.
//! 4. Confirming loads that block into the live world, or (Save) raises the
//!    overwrite prompt and then writes the session into the card image.
//!
//! None of that flow lives here. The grid cursor, the once-per-port card
//! read, the rule that a Load may not confirm an empty block and the mapping
//! from a finished session back to `(port, cell)` are
//! [`legaia_engine_core::save_screen::SaveScreenFlow`] - one kernel both this
//! page and the native `play-window` drive, so the two cannot disagree about
//! what the screen addresses. This host supplies only the **bytes**: the
//! fifteen block snapshots behind a port, and the load / write against the
//! card image.

use super::*;
use crate::runtime::LegaiaRuntime;
use legaia_engine_core::equip_session::EquipSession;
use legaia_engine_core::field_menu::{
    FieldMenuGate, FieldMenuInput, FieldMenuOutcome, FieldMenuPhase, FieldMenuRow, FieldMenuSession,
};
use legaia_engine_core::field_menu_dispatch::{
    self, ArtsEditorPhaseTag, FieldMenuSubsession, apply_arts_outcome, apply_equip_outcome,
    apply_pause_items_outcome, apply_spell_outcome, status_snapshots,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::inventory_use::{InventoryUseSession, InventoryUseState};
use legaia_engine_core::options::OptionsSession;
use legaia_engine_core::save_menu_atlas::{SaveMenuAtlas, build_atlas};
use legaia_engine_core::save_screen::{SaveCommitKind, SaveScreenFlow};
use legaia_engine_core::save_select::{
    SaveRack, SaveSelectMode, SaveSelectSession, SelectPhase, SlotInfoMode,
};
use legaia_engine_core::spell_menu::{SpellMenuPhase, SpellMenuSession};
use legaia_engine_core::status_screen::StatusScreenSession;
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::{
    self as ui, FieldMenuPartyView, FieldMenuRowView, SaveMenuAtlasRects, SlotGridCell,
    SlotInfoView, SpriteDraw, StatusPanelView, StatusSatelliteView, StatusStatRow, TextDraw,
};

/// Stage origin + integer scale that upscales the 320x240 boot-UI stage to
/// fill the play surface, centred. Shared with the native window rather than
/// mirrored: `legaia_engine_ui::pause_menu::stage_transform`.
pub(crate) use legaia_engine_ui::pause_menu::stage_transform;
use legaia_engine_ui::pause_menu::{
    GenericContent, ItemsScreenView, MagicScreenView, MenuRects, OptionsScreenView, PauseMenuCtx,
    PauseScreen, SpecialConfirmView, StatusScreenView, TopLevelView, equip_screen_compose,
    pause_screen_draws, spell_level_notice_draws,
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

    /// The chrome atlas' band rects, when the gold chrome resolved from the
    /// disc (shared with the dialog reading box, [`crate::play_dialog`]).
    pub(crate) fn chrome_rects(&self) -> Option<&SaveMenuAtlasRects> {
        self.chrome.as_ref().map(|(_, r)| r)
    }

    /// The battle HUD's nine status-element + eight element badge cells,
    /// projected out of the baked atlas.
    ///
    /// Which cells actually carry art is a property of **this bake** (three
    /// of the status badges need the row-511 CLUT extension the slice above
    /// is rooted at), so it travels with the atlas rather than being
    /// re-derived from the layout - the same rule the native window follows.
    pub(crate) fn battle_badges(
        &self,
    ) -> Option<legaia_engine_ui::battle_hud_chrome::BattleBadgeRects> {
        self.chrome.as_ref().map(
            |(a, _)| legaia_engine_ui::battle_hud_chrome::BattleBadgeRects {
                status: a.band_status_badges(),
                element: a.band_element_badges(),
            },
        )
    }

    /// The disc-parsed menu-overlay window-descriptor table, when it parsed.
    ///
    /// [`Self::window_rect`] falls back to pinned rects for the screens whose
    /// layout is capture-verified, but the retail **painter** family
    /// ([`crate::play_shop`]) dispatches on each descriptor's `renderer_va`,
    /// which no fallback can invent - those windows draw only when the real
    /// table is present, exactly as they do in the native window.
    pub(crate) fn window_table(&self) -> Option<&legaia_asset::menu_windows::MenuWindowTable> {
        self.windows.as_ref()
    }

    /// Descriptor-rect resolver over the parsed table, with the pinned
    /// fallback + the frame-box maths shared with the native window
    /// ([`legaia_engine_ui::pause_menu::MenuRects`]).
    fn rects(&self) -> MenuRects<'_> {
        MenuRects::new(self.windows.as_ref())
    }

    fn window_rect(&self, id: usize) -> (i32, i32, i32, i32) {
        self.rects().rect(id)
    }

    /// The chrome atlas' band rects, when the gold chrome resolved.
    fn chrome_rects_opt(&self) -> Option<&SaveMenuAtlasRects> {
        self.chrome.as_ref().map(|(_, r)| r)
    }

    /// The shared pause-menu composition context for this frame.
    fn menu_ctx(&self, origin: (i32, i32), scale: u32) -> PauseMenuCtx<'_> {
        PauseMenuCtx {
            font: &self.font,
            rects: self.rects(),
            chrome: self.chrome_rects_opt(),
            origin,
            scale,
        }
    }
}

/// Active pause-menu state: the shared root-list state machine plus the open
/// sub-screen, if any.
pub struct PlayMenu {
    /// The retail root command picker, shared with the native `play-window`
    /// ([`FieldMenuSession`]). The page used to hold a bare `u8` cursor and
    /// route confirms itself, which meant the two gate inputs retail reads at
    /// every draw - the op-`0x49` entry context and the scene's save-allow
    /// bit - had no reader in the browser at all: every row drew white and
    /// every row opened, so a player could Save in a scene whose MAN header
    /// forbids it. The session owns the ink, the confirm routing and the
    /// suspend/resume handshake; this module supplies only the sub-screens.
    session: FieldMenuSession,
    /// [`SceneMode`] the world ran when the menu opened, restored on close -
    /// the browser twin of `BootSession::field_menu_resume`. The menu holds
    /// the world in [`SceneMode::Menu`] while it is up, which is what
    /// suspends field dispatch (retail `game_mode 0x17`).
    resume_mode: SceneMode,
    sub: Option<PlaySub>,
    /// The save screen's shared driver: block-grid cursor + the card read
    /// behind it. Lives in `engine-core` so this page and the native window
    /// step the same cursor and gate the same confirms.
    save_flow: SaveScreenFlow,
    /// CDNAME label of the scene an in-canvas card Load landed in, waiting for
    /// the page to pick it up ([`LegaiaRuntime::play_menu_take_load_scene`]).
    /// Retail resumes the save in the scene it was written in; the page owns
    /// scene entry, so the menu parks the label here.
    pending_load_scene: Option<String>,
}

/// The open sub-screen. Every row runs the real [`FieldMenuSubsession`] the
/// native `play-window` builds, and renders through the exact same
/// `legaia-engine-ui` draw builders.
enum PlaySub {
    // Boxed: the sub-session enum is large, and this is a per-menu allocation.
    Session(Box<FieldMenuSubsession>),
}

impl PlayMenu {
    fn new(session: FieldMenuSession, resume_mode: SceneMode) -> Self {
        PlayMenu {
            session,
            resume_mode,
            sub: None,
            save_flow: SaveScreenFlow::new(),
            pending_load_scene: None,
        }
    }
}

/// `(edge & button)` test on a PSX-encoded pad-edge word.
fn pressed(edge: u16, b: PadButton) -> bool {
    edge & b.mask() != 0
}

/// Map the engine-core window-14 target-panel model onto the engine-ui view
/// rows. Mirrors the native shell's `target_panel_members`.
fn target_panel_members(
    model: &legaia_engine_core::pause_screens::TargetPanelModel,
) -> Vec<ui::TargetPanelMember<'_>> {
    model
        .members
        .iter()
        .map(|m| ui::TargetPanelMember {
            name: m.name.as_str(),
            level: m.level,
            hp: m.hp,
            mp: m.mp,
            hp_max: m.hp_max,
            mp_max: m.mp_max,
            base_hp_max: m.base_hp_max,
            base_mp_max: m.base_mp_max,
            stat_eff: m.stat_eff,
            stat_base: m.stat_base,
        })
        .collect()
}

/// Hand-cursor decode of the target panel: all-party picks put the hand on
/// every row (retail cursor bit `0x2000`), otherwise it sits on one row.
fn target_panel_cursor(
    model: &legaia_engine_core::pause_screens::TargetPanelModel,
) -> ui::TargetPanelCursor {
    if model.all_targets {
        ui::TargetPanelCursor::All { pressed: false }
    } else {
        ui::TargetPanelCursor::Single {
            row: model.cursor_row,
            pressed: false,
        }
    }
}

/// Slide-in y-offset (delta from parked y) of the save screen's bottom info
/// panel. Mirrors the native shell's `info_panel_slide_offset`: retail's
/// `FUN_801E08D8` ramps the panel from off-screen-below (394) up to parked
/// (138) as its own timer runs, so 0 = fully landed.
fn info_panel_slide_offset(session: &SaveSelectSession) -> i32 {
    use legaia_engine_core::save_select::{
        INFO_PANEL_OFFSCREEN_Y, INFO_PANEL_PARKED_Y, interpolate_anim,
    };
    let (_, y) = interpolate_anim(
        (0, INFO_PANEL_OFFSCREEN_Y),
        (0, INFO_PANEL_PARKED_Y),
        session.info_panel_slide_anim_t(),
    );
    y - INFO_PANEL_PARKED_Y
}

/// Borrow the shared `engine-core` Equip screen model into the shared
/// `engine-ui` compose input.
///
/// The only per-host line left on this screen: the phase tag has to cross
/// from `engine-core`'s enum to `engine-ui`'s, and `engine-ui` deliberately
/// does not depend on `engine-core`. Everything either side of it - the
/// projection and the composition - is shared. The native window's twin is
/// `window/menu_draws.rs::equip_compose_input`.
fn equip_compose_input(
    m: &legaia_engine_core::pause_screens::EquipScreenModel,
    text_cursor: bool,
) -> ui::pause_menu::EquipComposeInput<'_> {
    use legaia_engine_core::pause_screens::EquipScreenPhase as Tag;
    ui::pause_menu::EquipComposeInput {
        party_names: &m.party_names,
        slot_labels: &m.slot_labels,
        slot_items: &m.slot_items,
        candidate_names: &m.candidate_names,
        candidate_counts: &m.candidate_counts,
        stat_compare: &m.stat_compare,
        phase: match m.phase {
            Tag::SlotPicker => ui::EquipDrawPhase::SlotPicker,
            Tag::ItemPicker => ui::EquipDrawPhase::ItemPicker,
            Tag::Confirm => ui::EquipDrawPhase::Confirm,
        },
        cursor: m.cursor,
        active_slot: m.active_slot,
        confirm_label: m.confirm_label.as_deref(),
        char_slot: m.char_slot as usize,
        slot_cursor: m.slot_cursor,
        pictogram_rows: m.pictogram_rows,
        text_cursor,
    }
}

/// Serialize one draw quad to JSON. `TextDraw` and `SpriteDraw` are the same
/// shape (`dst` / `src` rect + RGBA tint); the page samples the font atlas for
/// quads in the `texts` list and the chrome atlas for the `sprites` list.
pub(crate) fn quad_json(d: &TextDraw) -> serde_json::Value {
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
        // The real retail proportional dialog font decoded from the disc at
        // `load_disc` (byte-identical to what the native pause menu draws); the
        // built-in placeholder only stands in on a PROT.DAT-only load where the
        // font TIM / SCUS width table weren't available.
        let font = self
            .menu_font
            .clone()
            .unwrap_or_else(legaia_font::Font::placeholder);
        // Chrome atlas + window table off the loaded PROT, best-effort: a
        // PROT.DAT-only load may lack the overlay slices, in which case the
        // menu still renders its glyphs (no gold frame).
        let (chrome, windows) = match self.scene_host.as_ref() {
            Some(host) => {
                let idx = &host.index;
                let panel = {
                    // Rooted ONE TIM EARLIER than the system-UI sheet, at the
                    // row-511 CLUT extension - the same base the native window
                    // uses. That TIM carries no pixels, only sub-palettes
                    // 16..18, and three of the nine status-element badges
                    // (Stone / Rage / Faint) decode with nothing else. A slice
                    // rooted at the sheet puts the extension *behind* its start
                    // where `build_atlas` cannot reach it, and those three
                    // cells silently bake blank.
                    let base =
                        legaia_engine_core::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET as u64;
                    let end = (legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                        + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE)
                        as u64;
                    idx.prot_dat_raw_bytes(base, (end - base) as usize)
                };
                let pill =
                    idx.entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32);
                // The battle HUD's 8x12 numeral cells, off the menu-glyph
                // TIM's sub-palette 13. Baked into the same atlas so the
                // HUD's sprite list stays one texture on this host too.
                let glyph_tim = idx
                    .prot_dat_raw_bytes(
                        legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
                        legaia_asset::menu_glyph_atlas::TIM_SIZE,
                    )
                    .ok();
                let chrome = match (panel, pill) {
                    (Ok(panel_bytes), Ok(pill_bytes)) => {
                        match build_atlas(&panel_bytes, &pill_bytes, glyph_tim.as_deref()) {
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
    ///
    /// Byte-for-byte the construction `BootSession::open_field_menu` does:
    /// a [`FieldMenuSession`] seeded with the world's money + play time, the
    /// two row gates sampled into a [`FieldMenuGate`], and the world switched
    /// into [`SceneMode::Menu`] so field dispatch suspends while the menu owns
    /// the frame. Both gate inputs are scene-scoped and the menu suspends the
    /// field, so sampling once at open is equivalent to retail's per-frame
    /// re-read.
    pub fn play_menu_open(&mut self) {
        if !self.ensure_menu_assets() {
            return;
        }
        if self.play_menu.is_some() {
            return;
        }
        // The whole menu-open precondition, asked of the engine rather than
        // spelled out here: the engaged bit (a talking player's Start opens
        // nothing) and the scene mode (the field *and* the overworld, since
        // retail runs one locomotion controller across both). This is the
        // same predicate `BootSession::tick` and the native window route
        // through - three local copies of the mode test is how the overworld
        // lost the pause menu.
        // REF: FUN_801D01B0
        if self
            .scene_host
            .as_ref()
            .is_some_and(|h| !h.world.field_menu_open_allowed())
        {
            return;
        }
        let mut session = FieldMenuSession::new();
        let resume_mode = match self.scene_host.as_mut() {
            Some(host) => {
                let world = &mut host.world;
                session.money = world.money.max(0) as u32;
                session.play_time_seconds = world.play_time_seconds;
                session.set_gate(FieldMenuGate {
                    entry_context_kind: world.menu_entry_context_kind(),
                    save_allowed: world.scene_save_allowed,
                });
                // Same entry decode the native window runs: a locked context
                // opens on the notice panel, not on the root picker.
                session.open_entry_screen();
                let resume = world.mode;
                world.mode = SceneMode::Menu;
                resume
            }
            None => SceneMode::Field,
        };
        self.play_menu = Some(PlayMenu::new(session, resume_mode));
    }

    /// Open the pause menu directly on one row's sub-screen, named the way
    /// [`FieldMenuRow::label`] names it (`"Load"`, `"Options"`, …).
    ///
    /// The boot title's Continue and Options rows land here: retail's title
    /// screen routes them to the same save-select and options screens the
    /// pause menu reaches, and the browser has no second copy of either. The
    /// row is opened through the session's own confirm routing, so a row the
    /// gate blocks stays blocked here too. Returns `false` when the row name
    /// is unknown or the row buzzes.
    pub fn play_menu_open_row(&mut self, row: &str) -> bool {
        let Some(row) = FieldMenuRow::ALL.iter().copied().find(|r| r.label() == row) else {
            return false;
        };
        self.play_menu_open();
        let Some(menu) = self.play_menu.as_ref() else {
            return false;
        };
        if !menu.session.row_is_available(row) {
            self.play_menu_close();
            return false;
        }
        // Drive the shared picker to the row rather than assigning its
        // cursor: `tick` is what puts the session in `Suspended`, which is
        // what `resume` needs to hand control back on close.
        let steps = usize::from(row.index());
        for _ in 0..steps {
            self.play_menu_input(PadButton::Down.mask());
        }
        self.play_menu_input(PadButton::Cross.mask());
        self.play_menu
            .as_ref()
            .is_some_and(|m| matches!(m.sub, Some(PlaySub::Session(_))))
    }

    /// Close the menu (and any open sub-screen), restoring the scene mode the
    /// world ran when it opened - the browser twin of
    /// `BootSession::close_field_menu`.
    pub fn play_menu_close(&mut self) {
        let Some(menu) = self.play_menu.take() else {
            return;
        };
        if let Some(host) = self.scene_host.as_mut() {
            host.world.mode = menu.resume_mode;
        }
    }

    /// Whether a Start edge would open the pause menu right now:
    /// [`World::field_menu_open_allowed`] over the wire, so the page can grey
    /// its own affordance without re-deriving the rule.
    ///
    /// The page used to answer this itself by comparing
    /// [`Self::scene_mode`] against the string `"Field"`, with a comment
    /// asserting Start was inert "on the world map". Retail says otherwise -
    /// the overworld runs the same locomotion controller the field does - and
    /// a page-side copy of an engine rule is drift the drift gates cannot
    /// see, because no Rust symbol is missing.
    pub fn play_menu_can_open(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.field_menu_open_allowed())
    }

    pub fn play_menu_is_open(&self) -> bool {
        self.play_menu.is_some()
    }

    /// Whether the current scene permits a menu Save
    /// ([`World::scene_save_allowed`](legaia_engine_core::world::World::scene_save_allowed),
    /// seeded at scene load from the MAN header bit retail copies into
    /// `_DAT_8007B6A8`). The page shows the Save-here hint from this, and the
    /// menu's own Save row inks and buzzes from the same value through
    /// [`FieldMenuGate`].
    pub fn play_scene_save_allowed(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.scene_save_allowed)
    }

    /// Take the CDNAME scene label an in-canvas card **Load** landed in, if
    /// one is waiting; `""` otherwise. The page polls this after driving the
    /// menu and, when it is a scene it can walk, enters it - retail resumes a
    /// save in the scene it was written in. Consuming clears it.
    pub fn play_menu_take_load_scene(&mut self) -> String {
        self.play_menu
            .as_mut()
            .and_then(|m| m.pending_load_scene.take())
            .unwrap_or_default()
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
        // Window 7 (spell level-up notice) owns the pad while armed: retail's
        // cast sub-screens stall on the confirm | cancel masks after the
        // widget-VM `[open window 7]` script (`0x801E4D50` / `0x801E4D78`),
        // and nothing else on the pad moves. Same pre-empt as the native
        // window's field-menu arm.
        if self.menu.dismiss_spell_level_notice(
            pressed(edge, PadButton::Cross),
            pressed(edge, PadButton::Circle),
            pressed(edge, PadButton::Triangle),
        ) {
            return;
        }
        // Sub-screen active: route to its session, then check for exit.
        let has_sub = self
            .play_menu
            .as_ref()
            .map(|m| m.sub.is_some())
            .unwrap_or(false);
        if has_sub {
            // Answer the flow's card read off `&self` before the `&mut`
            // borrow below: reading a port lifts fifteen SC blocks, which is
            // the one thing the kernel cannot do for itself.
            self.service_card_read();

            let mut session_done = false;
            let mut edge = edge;
            if let Some(m) = self.play_menu.as_mut()
                && let Some(PlaySub::Session(session)) = m.sub.as_mut()
            {
                // Step the grid cursor and gate an empty-block Load BEFORE
                // the session ticks, so a confirm on the same edge commits
                // the cell the player is looking at.
                if let FieldMenuSubsession::Save(s) = session.as_ref() {
                    edge = m.save_flow.before_tick(s, edge);
                }
                // Engine extension: Triangle on the Status screen swaps it
                // for the Tactical Arts chain editor (retail's seven rows
                // carry no Arts row). The edge is consumed, so the same
                // press does not also drive the screen it replaced.
                let opened_arts = match self.scene_host.as_ref() {
                    Some(host) => field_menu_dispatch::try_open_arts_editor(
                        session.as_mut(),
                        edge,
                        &host.world,
                    ),
                    None => false,
                };
                if !opened_arts {
                    session.tick_pad_edge(edge);
                }
                session_done = session.is_done();
            }
            if session_done {
                // Fold the finished session's result into the live world
                // (equip swap / item use / spell cast / card load-save)
                // exactly as the native shell does, then drop back to the
                // top-level list on the row that opened it.
                let flow = self
                    .play_menu
                    .as_ref()
                    .map(|m| m.save_flow.clone())
                    .unwrap_or_default();
                let sub = self.play_menu.as_mut().and_then(|m| m.sub.take());
                if let Some(PlaySub::Session(session)) = sub {
                    let session = *session;
                    match session {
                        // Load / Save reach the card rack, which needs the
                        // whole runtime - so it is applied outside the
                        // scene-host borrow the other rows take.
                        FieldMenuSubsession::Save(s) => self.apply_card_outcome(&flow, &s),
                        // Options: value edits commit inside the session's own
                        // popup (retail writes the config word at popup
                        // confirm and never reverts), so the closing state is
                        // the player's. Lift it onto the runtime - the same
                        // `self.options_state = session.state().clone()` the
                        // native window does - or the next open rebuilds from
                        // defaults and the screen forgets every change.
                        FieldMenuSubsession::Config(o) => {
                            self.options_state = o.state().clone();
                            self.persist_and_apply_options();
                        }
                        other => {
                            if let Some(host) = self.scene_host.as_mut() {
                                let world = &mut host.world;
                                match other {
                                    FieldMenuSubsession::Equip { session, char_slot } => {
                                        apply_equip_outcome(&session, char_slot, world);
                                    }
                                    // The full Items applier, not the inner
                                    // flow's: it also carries the special
                                    // Use routes' menu-exit handoff (Door of
                                    // Light's escape, Door of Wind's staged
                                    // world-map warp). The bag decrements
                                    // ride `s.inner` either way.
                                    FieldMenuSubsession::Items(s) => {
                                        let _ = apply_pause_items_outcome(&s, world);
                                    }
                                    FieldMenuSubsession::Spells(s) => {
                                        // A leveled menu cast returns the
                                        // window-7 pair; the shared runtime
                                        // holds the beat and the pre-empt at
                                        // the top of this method holds the
                                        // pad - same shape as the native
                                        // window.
                                        if let Some(notice) = apply_spell_outcome(&s, world) {
                                            self.menu.arm_spell_level_notice(notice);
                                        }
                                    }
                                    // Persist the edited chain back into the
                                    // world's saved chains so the next
                                    // battle's Arts rows reflect it - the
                                    // same chain_library <-> store_chain_library
                                    // bridge the native window uses.
                                    FieldMenuSubsession::Arts(editor) => {
                                        let mut library = world.chain_library();
                                        if apply_arts_outcome(editor, &mut library).is_ok() {
                                            world.store_chain_library(&library);
                                        }
                                    }
                                    // Status carries no world-mutating outcome
                                    // on close (Options is lifted above, on
                                    // the runtime rather than the world).
                                    _ => {}
                                }
                            }
                        }
                    }
                    // Hand control back to the shared picker, which parks the
                    // cursor on the row that opened the sub-screen - the same
                    // `menu.resume(false)` the native shell calls.
                    if let Some(m) = self.play_menu.as_mut() {
                        let _ = m.session.resume(false);
                    }
                }
            }
            return;
        }

        // Top-level command list: the shared retail picker, not a local
        // cursor. `tick` inks and routes off the same `root_menu_confirm_route`
        // the row renderer draws from, so a row cannot draw white and then
        // open something the gate forbids.
        let input = FieldMenuInput {
            up: pressed(edge, PadButton::Up),
            down: pressed(edge, PadButton::Down),
            // The kind-0x0D ready check is a horizontal two-row choice, so
            // the picker needs left / right as well as up / down.
            left: pressed(edge, PadButton::Left),
            right: pressed(edge, PadButton::Right),
            cross: pressed(edge, PadButton::Cross),
            circle: pressed(edge, PadButton::Circle),
            start: pressed(edge, PadButton::Start),
        };
        let suspended_row = match self.play_menu.as_mut() {
            Some(m) => {
                let _ = m.session.tick(input);
                match m.session.phase() {
                    FieldMenuPhase::Suspended { row } => Some(row),
                    _ => None,
                }
            }
            None => None,
        };
        if let Some(row) = suspended_row {
            // Load / Save browse the console's two memory-card ports, so the
            // rack is `CardPorts` - which is also what puts the session in
            // the matching two-stage flow; no host flips that flag by hand
            // (`SaveSelectSession::for_rack`). Every other
            // row builds the real retail sub-session from the disc catalogs
            // installed on the host world at `load_disc` (spell / equipment /
            // item), matching the native shell's `FieldMenuSubsession::build`.
            let rack = SaveRack::CardPorts(self.card_slot_snapshots());
            let sub = self.scene_host.as_ref().map(|host| {
                let world = &host.world;
                let chain = world.chain_library();
                PlaySub::Session(Box::new(FieldMenuSubsession::build(
                    row,
                    world,
                    &self.options_state,
                    &rack,
                    &chain,
                    &world.spell_catalog,
                    &world.equipment_table,
                )))
            });
            if let Some(sub) = sub
                && let Some(m) = self.play_menu.as_mut()
            {
                m.sub = Some(sub);
                m.save_flow.reset();
            }
        }
        // Circle on the root list (or a sub-session that asked to close the
        // menu entirely) finishes the session; restore the suspended scene
        // mode and drop the menu, exactly as `close_field_menu` does.
        let outcome = self.play_menu.as_ref().and_then(|m| m.session.outcome());
        if let Some(FieldMenuOutcome::Closed | FieldMenuOutcome::Confirmed(_)) = outcome {
            self.play_menu_close();
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

        // The kind-0x0D entry pair replaces the root list rather than
        // overlaying it: both sub-screens open with `05 00` (close every
        // window) before opening their own, so the command rows are not on
        // screen while either is up. Browser twin of the native window's
        // `context_locked_screen_draws`.
        let context_screen = self.build_context_locked(assets, menu, &mut texts, origin, scale);
        match &menu.sub {
            _ if context_screen => {}
            None => self.build_top_level(assets, menu, &mut sprites, &mut texts, origin, scale),
            Some(PlaySub::Session(sub)) => match sub.as_ref() {
                FieldMenuSubsession::Save(s) => {
                    self.build_save_select(assets, s, menu, &mut sprites, &mut texts, origin, scale)
                }
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
                FieldMenuSubsession::Arts(s) => {
                    self.build_arts_editor(assets, s, &mut sprites, &mut texts, origin, scale)
                }
            },
        }

        // Window 7 - the spell level-up notice - overlays whichever menu
        // screen is current while the shared runtime holds the beat. Both
        // hosts route it through the one shared composition, so neither can
        // pick a different painter or a different stand-in cursor.
        if let Some(notice) = self.menu.spell_level_notice() {
            texts.extend(spell_level_notice_draws(
                &assets.menu_ctx(origin, scale),
                &notice.line,
            ));
        }

        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

/// Test-only probes for the disc-gated pause-menu oracles
/// (`tests/menu_parity.rs`). Native-only so the wasm export surface the page
/// consumes stays exactly the player-facing API.
#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// The runtime's live [`legaia_engine_core::options::OptionsState`] as
    /// JSON - what the next Options open seeds itself from.
    pub fn debug_options_json(&self) -> String {
        serde_json::to_string(&self.options_state).unwrap_or_default()
    }

    /// The **open** Options sub-session's own state as JSON, or `None` when
    /// that screen is not the one holding the pad. Comparing it against
    /// [`Self::debug_options_json`] is what proves the seed is the runtime's
    /// state rather than a fresh default.
    pub fn debug_open_options_json(&self) -> Option<String> {
        let m = self.play_menu.as_ref()?;
        let PlaySub::Session(session) = m.sub.as_ref()?;
        match session.as_ref() {
            FieldMenuSubsession::Config(o) => serde_json::to_string(o.state()).ok(),
            _ => None,
        }
    }
}

impl LegaiaRuntime {
    /// Top-level command list + money/time box + party info panel, with gold
    /// window chrome + the cursor / icon sprites. Mirrors the native window's
    /// `BootUiState::FieldMenu { sub: None }` path.
    /// Windows 6 and 5 - the pair the pause menu draws when the op-`0x49`
    /// entry context's kind byte is `0x0D`. Browser twin of the native
    /// window's `context_locked_screen_draws`, off the same session phases,
    /// the same disc-parsed rects and the same
    /// [`legaia_engine_core::pause_screens::ContextLockedLabels`] - so
    /// neither host can invent a label the other does not have.
    ///
    /// Returns `true` when one of the two drew, which is what suppresses
    /// the root list for the frame.
    fn build_context_locked(
        &self,
        assets: &PlayMenuAssets,
        menu: &PlayMenu,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) -> bool {
        let Some(world) = self.menu_world() else {
            return false;
        };
        let ctx = assets.menu_ctx(origin, scale);
        let labels = &world.menu_context_labels;
        let out = if menu.session.notice_is_up() {
            let lines: Vec<&str> = labels.notice_lines.iter().map(String::as_str).collect();
            pause_screen_draws(&ctx, PauseScreen::ContextNotice { lines: &lines })
        } else if let Some(cursor_row) = menu.session.ready_confirm_cursor() {
            pause_screen_draws(
                &ctx,
                PauseScreen::ContextReady {
                    headings: [
                        labels.ready_headings[0].as_str(),
                        labels.ready_headings[1].as_str(),
                    ],
                    choices: [labels.choices[0].as_str(), labels.choices[1].as_str()],
                    cursor: u32::from(cursor_row),
                },
            )
        } else {
            return false;
        };
        if out.is_empty() {
            return false;
        }
        texts.extend(out.texts);
        true
    }

    /// Top-level command list + money/time box + party info panel.
    ///
    /// Money, play time, cursor and per-row ink all come off the shared
    /// session's view - the same projection the native window renders. The
    /// page used to substitute `world.frame / 60` for the clock and a
    /// literal `true` for every row's ink; the first made the H:MM:SS box a
    /// per-page-load frame counter (and wrote that number into any save
    /// taken from the browser), and the second drew a blocked row white.
    fn build_top_level(
        &self,
        assets: &PlayMenuAssets,
        menu: &PlayMenu,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let Some(world) = self.menu_world() else {
            return;
        };
        let view = menu.session.view();
        let rows: Vec<FieldMenuRowView<'_>> = view
            .rows
            .iter()
            .map(|r| FieldMenuRowView {
                label: r.label,
                enabled: r.enabled,
            })
            .collect();
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
        let party_ap: Vec<u16> = snaps.iter().map(|s| s.ap as u16).collect();
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::TopLevel(TopLevelView {
                rows: &rows,
                cursor: view.cursor,
                money: view.money,
                play_time_seconds: view.play_time_seconds,
                party: &party,
                party_ap: &party_ap,
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Status sub-screen: the main panel + the three satellite windows + the
    /// Status tab, with the LV/HP/MP + AP-gauge + element icon sprites.
    fn build_status(
        &self,
        assets: &PlayMenuAssets,
        s: &StatusScreenSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
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
        let panel = StatusPanelView {
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
        let names: Vec<&str> = s.snapshots().iter().map(|m| m.name.as_str()).collect();
        let satellite = StatusSatelliteView {
            party_names: &names,
            cursor: s.cursor() as usize,
            name: &snap.name,
            level: snap.level,
        };
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::Status(StatusScreenView {
                panel: &panel,
                satellite: &satellite,
                ap: snap.ap as u16,
                atr_char: snap.slot as usize,
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Options sub-screen: the settings rows + value popup + the hand cursor,
    /// with the options window frame + tab.
    fn build_config(
        &self,
        assets: &PlayMenuAssets,
        s: &OptionsSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
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
        let popup = s.popup().map(|p| ui::OptionsPopupDraw {
            rect: self.options_popup_rect(assets, &p),
            choices: p.choices,
            cursor: p.cursor,
        });
        let row_y_off: i32 = rows
            .iter()
            .take(s.cursor() as usize)
            .map(|r| r.advance)
            .sum();
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::Options(OptionsScreenView {
                rows: &row_views,
                cursor: s.cursor(),
                popup,
                row_y_off,
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Tactical Arts chain editor, inside the generic sub-window frame.
    ///
    /// The engine extension reached by Triangle on the Status screen (see
    /// `field_menu_dispatch::try_open_arts_editor`). The live editor state
    /// is projected by the shared `arts_editor_view`, so the character
    /// name, the pretty-printed sequences and the "+ New" room check are
    /// the same code the native window runs - only the borrow into
    /// `ArtsEditorDrawArgs` is per host.
    fn build_arts_editor(
        &self,
        assets: &PlayMenuAssets,
        editor: &legaia_engine_core::tactical_arts_editor::ChainEditor,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let Some(world) = self.menu_world() else {
            return;
        };
        let view = field_menu_dispatch::arts_editor_view(editor, world);
        let saved_rows: Vec<ui::ArtsChainRow<'_>> = view
            .saved
            .iter()
            .map(|(name, pretty)| ui::ArtsChainRow {
                name,
                pretty_sequence: pretty,
            })
            .collect();
        let args = ui::ArtsEditorDrawArgs {
            character_name: &view.character_name,
            phase: match view.phase {
                ArtsEditorPhaseTag::Browsing => ui::ArtsEditorPhase::Browsing,
                ArtsEditorPhaseTag::Editing => ui::ArtsEditorPhase::Editing,
                ArtsEditorPhaseTag::Naming => ui::ArtsEditorPhase::Naming,
            },
            saved: &saved_rows,
            browse_cursor: view.browse_cursor,
            editing_pretty: &view.editing_pretty,
            editing_len: view.editing_len,
            min_len: view.min_len,
            max_len: view.max_len,
            naming_name: &view.naming_name,
            can_add_new: view.can_add_new,
        };
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::Generic(GenericContent::Arts(args)),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Answer the card read [`SaveScreenFlow`] is waiting on, if any.
    ///
    /// The kernel decides *when* a port must be read (once per port, never
    /// per frame); this host is what turns a port number into fifteen block
    /// snapshots, because only it holds the card image.
    fn service_card_read(&mut self) {
        let Some(m) = self.play_menu.as_ref() else {
            return;
        };
        let Some(PlaySub::Session(session)) = m.sub.as_ref() else {
            return;
        };
        let FieldMenuSubsession::Save(s) = session.as_ref() else {
            return;
        };
        let Some(port) = m.save_flow.pending_read(s) else {
            return;
        };
        let blocks = self.card_block_snapshots(port as usize);
        if let Some(m) = self.play_menu.as_mut() {
            m.save_flow.install_blocks(port, blocks);
        }
    }

    /// Commit a finished Load / Save session against the memory-card rack.
    ///
    /// [`SaveScreenFlow::commit`] resolves the port off the session's outcome
    /// and the cell off the grid; this host maps cell `i` to card block
    /// `i + 1` (block 0 is the card directory) and moves the bytes. A failure
    /// (card ejected mid-flow, unreadable block) is logged and drops the
    /// player back to the menu rather than throwing - the world is left
    /// untouched.
    fn apply_card_outcome(&mut self, flow: &SaveScreenFlow, session: &SaveSelectSession) {
        let Some(commit) = flow.commit(session) else {
            return;
        };
        let block = commit.cell + 1;
        match commit.kind {
            SaveCommitKind::Load => {
                match self.load_session_from_card(commit.port as usize, block) {
                    Ok(scene) => {
                        // Retail resumes a save in the scene it was written in.
                        // The page owns scene entry, so park the label for it.
                        if let Some(m) = self.play_menu.as_mut() {
                            m.pending_load_scene = Some(scene);
                        }
                    }
                    Err(e) => crate::console_log(&format!("play menu: card load failed: {e}")),
                }
            }
            SaveCommitKind::Save => {
                if let Err(e) = self.write_session_into_card(commit.port as usize, block) {
                    crate::console_log(&format!("play menu: card save failed: {e}"));
                }
            }
        }
    }

    /// Load / Save sub-screen: the real retail save-select chrome, driven off
    /// the memory-card rack. Mirrors the native window's
    /// `save_select_chrome_sprite_draws` + its `boot_ui_draws` text half,
    /// with the pill row bound to the rack's card ports and the preview grid
    /// to the selected card's blocks.
    #[allow(clippy::too_many_arguments)]
    fn build_save_select(
        &self,
        assets: &PlayMenuAssets,
        s: &SaveSelectSession,
        menu: &PlayMenu,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let font = &assets.font;
        let title = match s.mode() {
            SaveSelectMode::Load => "Load",
            SaveSelectMode::Save => "Save",
        };
        let phase = s.phase();
        let card = s.current_slot();

        // --- text: the panel title ---
        // The confirm prompt is deliberately NOT handed to
        // `save_select_draws_for`: its inline Yes/No is the flat model's
        // layout, which lands on top of this screen's info panel. Retail
        // raises the prompt as its own centred messagebox (FUN_801E1C1C
        // mode 3) - emitted at the end of this function.
        let rows: Vec<ui::SaveSelectRow<'_>> = s
            .slots()
            .iter()
            .map(|slot| ui::SaveSelectRow {
                label: &slot.label,
                present: slot.present,
                party_lv: slot.party_lv,
                play_time_seconds: slot.play_time_seconds,
                money: slot.money,
                location: &slot.location,
            })
            .collect();
        let mut d = ui::save_select_draws_for(
            font,
            title,
            &rows,
            card as usize,
            None,
            origin,
            scale,
            // The chrome atlas supplies the pointing-finger cursor sprite;
            // fall back to the ASCII cursor glyph only without it.
            assets.chrome.is_none(),
        );

        // --- sprites: pills + phase overlays (need the chrome atlas) ---
        let Some((_, rects)) = assets.chrome.as_ref() else {
            texts.extend(d);
            return;
        };

        // Retail draws every pill while browsing, but shows only the picked
        // one - relocated up under the Load panel - once a card is committed,
        // sliding it there over 16 frames (FUN_801E1C1C mode 2).
        let (pills, pill_anchor): (Vec<u8>, (i32, i32)) = match phase {
            SelectPhase::NowChecking { slot, .. }
            | SelectPhase::SlotPreview { slot }
            | SelectPhase::ConfirmOverwrite { slot, .. }
            | SelectPhase::ConfirmDelete { slot, .. } => {
                // Slide start = the pill's Browsing position (retail
                // mode-2 start (160, 96) minus the inlined -0x18
                // x-shift = the Browsing pill quad).
                let pos = s.interpolate(
                    ui::SAVE_SELECT_SLOT1_POS,
                    ui::SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE,
                );
                (vec![slot], pos)
            }
            _ => (
                (0..s.slots().len().min(2) as u8).collect(),
                ui::SAVE_SELECT_SLOT1_POS,
            ),
        };
        sprites.extend(ui::save_select_chrome_draws_for(
            rects,
            &pills,
            pill_anchor,
            origin,
            scale,
        ));
        // The pill cursor is suppressed once a card is committed: the dialog
        // covers the pill row and the grid emits its own cursor.
        if matches!(phase, SelectPhase::Browsing { .. }) && !s.slots().is_empty() {
            sprites.push(ui::save_select_cursor_draw_for(
                rects,
                (card as usize).min(1),
                origin,
                scale,
            ));
        }

        match phase {
            SelectPhase::NowChecking { .. } => {
                // Panel + text slide in together from the right, matching
                // retail mode-0's (416, 112) -> (160, 112).
                let pos_x = legaia_engine_core::save_select::interpolate_anim(
                    (ui::NOW_CHECKING_SLIDE_START_X, 0),
                    (ui::NOW_CHECKING_SLIDE_TARGET_X, 0),
                    s.slide_anim_t(),
                )
                .0;
                let slide = (pos_x - ui::NOW_CHECKING_SLIDE_TARGET_X, 0);
                sprites.extend(ui::now_checking_panel_draws_for(
                    rects, origin, scale, slide,
                ));
                d.extend(ui::now_checking_text_draws_for(font, origin, scale, slide));
            }
            SelectPhase::SlotPreview { .. }
            | SelectPhase::ConfirmOverwrite { .. }
            | SelectPhase::ConfirmDelete { .. } => {
                // The picked card's fifteen blocks as retail's 5x3 grid, plus
                // the focused block's info panel sliding up underneath. The
                // blocks come off the card read's cache - see
                // `refresh_card_read_cache`; an unread card draws an empty
                // grid rather than re-parsing here every frame.
                let (blocks, cell) = menu.save_flow.preview(s);
                let cells: Vec<SlotGridCell> = blocks
                    .iter()
                    .map(|b| SlotGridCell {
                        present: b.present,
                        portrait_char_id: b.present.then_some(b.leader_char_id),
                    })
                    .collect();
                sprites.extend(ui::slot_preview_grid_draws_for(
                    rects, &cells, cell, origin, scale,
                ));
                let focused = blocks.get(cell as usize).filter(|b| b.present);
                let play_time = focused.map(|b| b.play_time_string()).unwrap_or_default();
                let view = focused.map(|b| SlotInfoView {
                    slot_no: b.slot.saturating_add(1),
                    location: &b.location,
                    play_time: &play_time,
                    leader_name: &b.leader_name,
                    leader_level: b.party_lv,
                    leader_hp: b.leader_hp,
                    leader_mp: b.leader_mp,
                    leader_char_id: b.leader_char_id,
                });
                let y_off = info_panel_slide_offset(s);
                sprites.extend(ui::slot_info_panel_draws_for(
                    rects,
                    view.as_ref(),
                    y_off,
                    origin,
                    scale,
                ));
                d.extend(ui::slot_info_panel_text_draws_for(
                    font,
                    view.as_ref(),
                    y_off,
                    origin,
                    scale,
                    // This branch only runs with the chrome atlas
                    // resident, which draws the label sprites.
                    true,
                ));
                // No preview means the block holds nothing loadable; retail
                // fills the panel with a caption saying which kind of
                // nothing rather than leaving it blank.
                if view.is_none()
                    && let Some(b) = blocks.get(cell as usize)
                    && let Some(caption) = SlotInfoMode::for_slot(b).caption(s.mode())
                {
                    d.extend(ui::slot_info_caption_draws_for(
                        font, caption, y_off, origin, scale,
                    ));
                }
            }
            _ => {}
        }

        // The confirm prompt rides on top of everything, sliding up from
        // below the stage (retail mode 3, (160, 344) -> (160, 88)).
        let confirm: Option<(&str, u8)> = match phase {
            SelectPhase::ConfirmOverwrite { cursor, .. } => Some(("Do you wish to save?", cursor)),
            SelectPhase::ConfirmDelete { cursor, .. } => Some(("Delete this save?", cursor)),
            _ => None,
        };
        if let Some((prompt, cursor)) = confirm {
            let y = legaia_engine_core::save_select::interpolate_anim(
                (0, ui::CONFIRM_DIALOG_SLIDE_START_Y),
                (0, ui::CONFIRM_DIALOG_SLIDE_TARGET_Y),
                s.info_panel_slide_anim_t(),
            )
            .1;
            sprites.extend(ui::confirm_dialog_panel_draws_for(rects, y, origin, scale));
            d.extend(ui::confirm_dialog_text_draws_for(
                font, prompt, cursor, y, origin, scale,
            ));
        }
        texts.extend(d);
    }

    /// Items sub-screen: the retail four-window layout (command 13 / list
    /// 15 / info 17 + the "Items" tab) fed from the engine-core session
    /// model. During target-select retail replaces the item list with
    /// window 14 - the party target panel (`FUN_801D0520`); the generic
    /// overlay only stands in when there is no world (and so no roster)
    /// behind the session.
    fn build_items(
        &self,
        assets: &PlayMenuAssets,
        s: &legaia_engine_core::pause_screens::PauseItemsSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let ctx = assets.menu_ctx(origin, scale);
        let model = legaia_engine_core::pause_screens::items_screen_model(s);
        if model.target_select {
            if let Some(panel) = self
                .menu_world()
                .and_then(|w| legaia_engine_core::pause_screens::target_panel_view_model(s, w))
                .filter(|m| !m.members.is_empty())
            {
                let members = target_panel_members(&panel);
                let view = ui::TargetPanelView {
                    members: &members,
                    mode: ui::TargetPanelMode::from_preview_word(panel.mode),
                    cursor: target_panel_cursor(&panel),
                    label_icons: assets.chrome.is_some(),
                    text_cursor: assets.chrome.is_none(),
                };
                let out = pause_screen_draws(&ctx, PauseScreen::ItemsTarget(&view));
                sprites.extend(out.sprites);
                texts.extend(out.texts);
                return;
            }
            let out = pause_screen_draws(
                &ctx,
                PauseScreen::Generic(GenericContent::Prebuilt(
                    self.items_session_draws(assets, &s.inner),
                )),
            );
            sprites.extend(out.sprites);
            texts.extend(out.texts);
            return;
        }
        let rows: Vec<ui::PauseItemsRow<'_>> = model
            .page_rows
            .iter()
            .map(|(name, count)| ui::PauseItemsRow {
                name,
                count: *count,
            })
            .collect();
        let info = model.info.as_ref().map(|i| ui::PauseItemInfo {
            name: &i.name,
            count: i.count,
            desc: &i.desc,
            passive: i.passive.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
        });
        let view = ui::PauseItemsView {
            rows: &rows,
            page: model.page,
            pages: model.pages,
            phase: if model.focus_list {
                ui::PauseItemsPhase::List
            } else {
                ui::PauseItemsPhase::Command
            },
            command_cursor: model.command_cursor,
            list_cursor: model.list_cursor_on_page,
            bag_empty: model.bag_empty,
            info,
            text_cursor: assets.chrome.is_none(),
        };
        let throw = model
            .throw_confirm
            .as_ref()
            .map(|c| ui::PauseThrowConfirmView {
                name: &c.name,
                count: c.count,
                cursor: c.cursor,
                text_cursor: assets.chrome.is_none(),
            });
        // Retail's own Use-route prompt strings live in the menu overlay's
        // unrecovered data segment, so the port stages the item name and its
        // own question in the retail line slots - the geometry, which is what
        // the renderer is, is exact.
        let special_one_line = model
            .special_confirm
            .as_ref()
            .map(|sc| format!("Use {}?", sc.item_name));
        let special_lines: Vec<&str> = match (model.special_confirm.as_ref(), &special_one_line) {
            (Some(sc), Some(one)) => {
                if matches!(
                    sc.route,
                    legaia_engine_core::pause_screens::UseRoute::Incense
                ) {
                    vec![sc.item_name.as_str(), "Use it?"]
                } else {
                    vec![one.as_str()]
                }
            }
            _ => Vec::new(),
        };
        let point_card = model.info.as_ref().filter(|i| i.is_point_card).map(|_| {
            self.menu_world()
                .map(|w| w.point_card.max(0) as u32)
                .unwrap_or(0)
        });
        let out = pause_screen_draws(
            &ctx,
            PauseScreen::Items(ItemsScreenView {
                view: &view,
                point_card,
                throw_confirm: throw.as_ref(),
                special_confirm: model.special_confirm.as_ref().map(|sc| SpecialConfirmView {
                    lines: &special_lines,
                    cursor: sc.cursor,
                }),
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Magic sub-screen: the retail four-window layout (list 18 / caster
    /// 19 / info 20 + the "Magic" tab) fed from the engine-core session
    /// model. During target-select the generic overlay stands in (its
    /// retail window layout is unpinned).
    fn build_spells(
        &self,
        assets: &PlayMenuAssets,
        s: &SpellMenuSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let ctx = assets.menu_ctx(origin, scale);
        let model = legaia_engine_core::pause_screens::magic_screen_model(
            s,
            self.menu_world().and_then(|w| w.menu_text.as_ref()),
        );
        if !model.target_select {
            let casters: Vec<ui::PauseMagicCaster<'_>> = model
                .casters
                .iter()
                .map(|(name, level, mp, mp_max)| ui::PauseMagicCaster {
                    name,
                    level: *level as u16,
                    mp: *mp,
                    mp_max: *mp_max,
                })
                .collect();
            let rows: Vec<ui::PauseMagicRow<'_>> = model
                .page_rows
                .iter()
                .map(|(name, ra_seru)| ui::PauseMagicRow {
                    name,
                    ra_seru: *ra_seru,
                })
                .collect();
            let info = model.info.as_ref().map(|i| ui::PauseMagicInfo {
                name: &i.name,
                level: i.level,
                desc: &i.desc,
                mp_cost: i.mp_cost,
            });
            let view = ui::PauseMagicView {
                casters: &casters,
                rows: &rows,
                page: model.page,
                pages: model.pages,
                phase: if model.focus_list {
                    ui::PauseMagicPhase::List
                } else {
                    ui::PauseMagicPhase::Caster
                },
                caster_cursor: model.caster_cursor,
                list_cursor: model.list_cursor_on_page,
                info,
                label_icons: assets.chrome.is_some(),
                text_cursor: assets.chrome.is_none(),
            };
            let out = pause_screen_draws(
                &ctx,
                PauseScreen::Magic(MagicScreenView {
                    view: &view,
                    casters: model.casters.len(),
                }),
            );
            sprites.extend(out.sprites);
            texts.extend(out.texts);
            return;
        }
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
        let out = pause_screen_draws(&ctx, PauseScreen::Generic(GenericContent::SpellMenu(args)));
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Equip sub-screen: the retail multi-window layout (party / item-list /
    /// main window + the Equip tab) + the slot pictogram column and hand
    /// cursors.
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
        let ctx = assets.menu_ctx(origin, scale);
        let names = self
            .menu_world()
            .map(field_menu_dispatch::roster_names)
            .unwrap_or_default();
        let m = legaia_engine_core::pause_screens::equip_screen_model(session, char_slot, &names);
        let out = equip_screen_compose(&ctx, &equip_compose_input(&m, assets.chrome.is_none()));
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Build the inventory item-use overlay text draws. Ported verbatim from
    /// the native shell's `items_session_draws` so the site emits the identical
    /// draw list. Crate-visible: the battle overlay draws the in-battle Item
    /// submenu through the same projection ([`crate::play_battle`]).
    pub(crate) fn items_session_draws(
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
        battle: Some(legaia_engine_ui::BattleChromeRects {
            panel_bg: a.band_battle_panel_bg(),
            plate_cap_l: a.band_battle_plate_cap_l(),
            plate_body: a.band_battle_plate_body(),
            plate_cap_r: a.band_battle_plate_cap_r(),
            separator: a.band_battle_separator(),
            digits: a.band_hud_digits(),
        }),
    }
}
