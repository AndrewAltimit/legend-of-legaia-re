//! Engine-side menu runtime - wires
//! [`legaia_engine_menus::menu::MenuCtx`] / [`legaia_engine_menus::menu::step`] to
//! a [`legaia_engine_core::world::World`] and to disk-backed save / load slots.
//!
//! [`MenuRuntime`] owns the menu ctx, a save-slot directory, and a small
//! flag block driven by [`step`] callbacks.
//! Engines call [`MenuRuntime::tick`] each frame with a [`MenuInput`]; the
//! runtime advances the state machine, captures save bytes when the menu
//! commits at `SavePickSlot`, writes them to a file, and on `LoadSlot`
//! commit reads a file back into the world.
//!
//! Rendering is engine-side (see `asset-viewer` or any custom shell) - the
//! runtime exposes a [`MenuRuntime::current_label`] string per state so the
//! HUD overlay has something to render even before the per-screen layouts
//! land.

use std::path::{Path, PathBuf};

use crate::bag_rows::WorldBagRows;
use anyhow::{Context, Result};
use legaia_engine_menus::menu::{MenuCtx, MenuHost, open, step};
use legaia_engine_menus::menu::{MenuInput, MenuState};
use legaia_save::{EquipmentSlots, Party, SpellList};

use legaia_engine_core::world::World;
use legaia_engine_menus::equipment::DiscEquipInfo;
use legaia_engine_menus::inn::InnSession;
use legaia_engine_menus::shop::{
    BuyListRoute, BuyRecipientEvent, BuyRecipientSession, ShopSession,
};

mod host;
mod notices;
mod save_slots;
mod sessions;
mod shop_flow;
mod tick;
mod views;

use legaia_engine_menus::shop_cues::*;
pub use save_slots::*;

/// File extension the runtime uses for save slots. PSX memory-card `.mcr`
/// support is layered on top of [`legaia_save::card`]; this runtime uses a
/// flat `<dir>/slot_NN.bin` shape for development convenience.
pub const SAVE_EXT: &str = "bin";

/// One menu-driven tick outcome - engines log / observe / react.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTickEvent {
    /// Menu ticked normally - no slot operation requested this frame.
    Stepped,
    /// Save committed to slot `slot` at `path`. Engines flash a UI banner.
    Saved {
        slot: u8,
        path: PathBuf,
    },
    /// Load committed for slot `slot` from `path`. World state was
    /// replaced with the loaded party.
    Loaded {
        slot: u8,
        path: PathBuf,
    },
    /// Save / load operation requested but the file was missing or
    /// invalid. Engines surface the error to the player; the menu
    /// transitions back to the picker.
    SaveError {
        slot: u8,
        message: String,
    },
    LoadError {
        slot: u8,
        message: String,
    },
}

/// Per-frame menu runtime. Lives alongside the world; engines tick it
/// after [`legaia_engine_core::world::World::tick`] when in
/// [`legaia_engine_mode::mode::SceneMode::Menu`] (or whatever in-menu mode the engine
/// uses).
/// Ticks the field takes to fade to black when a shop opens - see
/// [`MenuRuntime::shop_fade_level`].
pub const SHOP_FADE_FRAMES: u32 = 14;

pub struct MenuRuntime {
    pub ctx: MenuCtx,
    /// Save-slot directory. Created lazily on first save.
    pub save_dir: PathBuf,
    /// Number of save slots the picker offers (default 3 - one per save
    /// file in the `slot_NN.bin` shape).
    pub slot_count: u8,
    /// Index into `World::party.roster.members` for the active character
    /// sub-screen (StatusEquipment / StatusMagic / StatusTacticalArts).
    /// Updated by `commit(StatusCharacter, slot)`.
    pub selected_char: usize,
    /// Active shop session. Set via [`MenuRuntime::open_shop`] before
    /// entering `ShopBuy`; cleared on `ShopExit` commit.
    pub shop_session: Option<ShopSession>,
    /// Active inn session. Set via [`MenuRuntime::open_inn`] before
    /// entering `InnConfirm`; cleared after the player confirms or cancels.
    pub inn_session: Option<InnSession>,
    /// Active seru-trade session, opened when the player picks **Trade** in the
    /// `ShopMenu` top picker (the randomizer's `--seru-trade` feature). Holds the
    /// vendor's offers for the current two-hour window; cleared on shop exit.
    pub trade_session: Option<legaia_engine_battle::seru_trade::SeruTradeSession>,
    /// Offer index selected at `ShopTrade`, applied at `ShopTradeConfirm` (the
    /// `ShopTradeConfirm` cursor is the yes/no slot, not the offer).
    trade_pending_offer: usize,
    /// Disc-pinned per-item equip restrictions (character mask + slot
    /// category), installed by hosts via [`MenuRuntime::install_equip_info`].
    /// Feeds the retail buy-list kind dispatch: without it an equipment row
    /// cannot open the recipient picker.
    pub equip_info: Option<DiscEquipInfo>,
    /// Opt into the retail-shaped **equipment buy** flow: a confirmed
    /// buy-list row whose item kind is `1` (equipment) opens the
    /// buy-recipient picker (retail sub-screen `0x1C`, `FUN_801DB380`)
    /// instead of the quantity picker. Off by default - a host that enables
    /// it must also draw the picker (window 36 + the stat-compare windows
    /// 25 / 41); the browser play page does.
    pub retail_equipment_buy: bool,
    /// The live buy-recipient picker. While `Some`, [`MenuRuntime::tick`]
    /// drives it instead of the menu VM (the menu ctx stays parked on
    /// `ShopBuy`, exactly as retail parks list mode 1 under sub-screen
    /// `0x1C`).
    pub recipient_session: Option<BuyRecipientSession>,
    /// The live **quantity screen** (retail's two sibling steppers,
    /// `FUN_801DB7F4` buying / `FUN_801DBD94` selling). While `Some`,
    /// [`MenuRuntime::tick`] drives it instead of the menu VM: one number
    /// moves in place under the pad, the confirm commits the transaction
    /// with no Yes/No screen after it, and the list it came from stays
    /// parked behind the window. Both hosts read
    /// [`MenuRuntime::quantity_view`] for the window they draw.
    pub quantity_session: Option<legaia_engine_menus::shop::QuantityPicker>,
    /// The live casino **prize-exchange** session (menu-overlay sub-screen
    /// `0x20`, field-VM op-`0x49` sub-op 7). While `Some`,
    /// [`MenuRuntime::tick`] drives it instead of the menu VM - the session
    /// owns its own browse / Yes-No phases (`FUN_801DC1CC`'s 4-state SM), so
    /// it never enters the [`MenuState`] graph. Cleared when the browse
    /// cancel exits; the tick then calls `World::finish_prize_exchange` so
    /// the suspended field script resumes past the counter op.
    pub prize_session: Option<legaia_engine_minigames::prize_exchange::PrizeExchangeSession>,
    /// Cursor to restore after a stay-on-the-list route (a refused buy and
    /// the recipient-picker hand both keep the hand on the confirmed row;
    /// the VM's route reset would drop it to row 0).
    stay_cursor: Option<u8>,
    /// The live **Point Card toast**: `Some(credit)` while retail's window
    /// `0x1F` is up, holding the points this purchase earned.
    ///
    /// Retail's buy commit (`FUN_801DB7F4` case 2 / `FUN_801DB380`) credits
    /// the Point Card, hands the widget VM the one-command script
    /// `0x801E4EDC` / `0x801E4EA8` - both are literally `[open window 0x1F]`
    /// followed by the terminator - and then stalls in a phase that returns
    /// to the buy list only on a confirm / cancel press. The beat exists
    /// **only** while the party holds the Point Card: case 3 short-circuits
    /// straight back to sub-screen `0x1B` when `FUN_80042F4C(0xFE)` is zero.
    ///
    /// A host paints window 31 (`engine-ui`'s `amount_prompt_draws_for`)
    /// while this is `Some`.
    point_card_toast: Option<i32>,
    /// The shop windows' open / close slides
    /// ([`legaia_engine_menus::shop::ShopSlides`]), stepped once per tick toward the
    /// current screen's window set. Hosts draw from
    /// [`Self::shop_slides`].
    shop_slides: legaia_engine_menus::shop::ShopSlides,
    /// Free-running tick count, the engine's stand-in for retail's frame
    /// word `0x80084570` that the list kernel blink-gates its page
    /// triangles on ([`Self::ui_frame`]).
    ui_frames: u32,
    /// Ticks since the current shop / prize counter opened, saturating -
    /// the clock of the field's fade to black ([`Self::shop_fade_level`]).
    shop_age: u32,
    /// The list row the hand was on when a quantity stepper opened over it.
    /// The kind-4 list keeps its selection behind the stepper, so the hand
    /// comes back to this row (clamped to the rebuilt list) when it closes.
    quantity_list_row: u8,
    /// The UI cue this tick's shop step raised, if any, for the host to key
    /// ([`Self::take_ui_cue`]). Retail's shop screens key their own blips:
    /// the kind-4 list kernel `FUN_80032A44` (cursor `0x21` on a move,
    /// confirm `0x20` / dim-row buzz `0x23`, cancel `0x37`) and the
    /// quantity / recipient screens' own `FUN_80035B50` / `FUN_80035BD0`
    /// calls ([`shop_ui_cue`] has the per-screen table).
    ui_cue: Option<u8>,
    /// The live **spell level-up notice**: `Some` while retail's window 7 is
    /// up, holding the `(caster, spell index)` pair `FUN_80035C00` set plus
    /// the assembled prompt line.
    ///
    /// Retail's magic-cast sub-screens (`FUN_801D9280` / `FUN_801D9594`)
    /// seed `_DAT_8007BB70` / `_DAT_8007BB78` to `0xFF`, run the effect
    /// apply, and hand the widget VM the one-command script `0x801E4D50` /
    /// `0x801E4D78` (`[open window 7]` + terminator) only when the sentinel
    /// changed - then stall for a confirm / cancel press. The engine arms
    /// this from [`crate::field_menu_dispatch::apply_spell_outcome`]'s
    /// return; a host paints window 7 (`engine-ui`'s
    /// `char_prompt_draws_for`) while this is `Some` and holds the pad
    /// until a press clears it.
    spell_level_notice: Option<legaia_engine_battle::magic_xp::SpellLevelNotice>,
    /// The live **art-learned notice**: `Some` while retail's window 8 is
    /// up. The Items use sub-screen opens it (script `0x801E4C60`, `[open
    /// window 8]`) when the applier changed `_DAT_8007BB78` - the Hyper-Art
    /// book arm's `FUN_80035C00(slot, art)` - and stalls for a confirm /
    /// cancel press before closing it (`0x801E4C68`). Hosts arm it from
    /// [`legaia_engine_core::world::MenuState::pending_art_notice`] and paint window 8
    /// (`engine-ui`'s `art_learned_notice_draws`) while this is `Some`.
    art_learned_notice: Option<legaia_engine_menus::pause_screens::ArtLearnedNotice>,
    /// Pending operation flagged by the host hooks; consumed inside
    /// [`MenuRuntime::tick`].
    pending: Option<PendingOp>,
    /// Last `ctx.state` the window-widget choreography reacted to. The
    /// shop states drive the disc-resolved widget scripts
    /// ([`legaia_engine_menus::menu_widget`]) through
    /// [`World::run_shop_widget_open`] /
    /// [`World::run_shop_widget_sell_away`]; this cell edge-detects the
    /// state changes (retail's `FUN_801DAFD4` runs the scripts on the
    /// same transitions).
    ///
    /// [`World::run_shop_widget_open`]: World::run_shop_widget_open
    /// [`World::run_shop_widget_sell_away`]: World::run_shop_widget_sell_away
    widget_state_seen: u8,
}

/// The live quantity screen's content, as a host needs it to lay the window
/// out: which stepper is up, the staged item, the stepped number, its bound
/// and the unit price the running total comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantityView {
    /// `true` for the buy stepper (window 35), `false` for the sell one
    /// (window 37).
    pub buying: bool,
    pub item_id: u8,
    /// The stepped number (`DAT_801E46B4`).
    pub quantity: u8,
    /// The bound (`DAT_801E46B8`) the value row's second number prints.
    pub max: u8,
    /// Unit price the running total multiplies (buy) or halves (sell).
    pub price: u16,
}

/// Fold a [`MenuInput`] back into the edge-triggered PSX pad word the
/// retail sub-screen ports decode.
///
/// The menu VM takes per-button booleans while the quantity steppers take
/// the pad word their disassembly tests, so the two models meet here rather
/// than in either host.
fn menu_input_pad_word(input: MenuInput) -> u16 {
    use legaia_engine_system::input::PadButton;
    let mut w = 0u16;
    for (on, b) in [
        (input.cross, PadButton::Cross),
        (input.circle, PadButton::Circle),
        (input.triangle, PadButton::Triangle),
        (input.square, PadButton::Square),
        (input.up, PadButton::Up),
        (input.down, PadButton::Down),
        (input.left, PadButton::Left),
        (input.right, PadButton::Right),
    ] {
        if on {
            w |= b.mask();
        }
    }
    w
}

/// The [`MenuInput`] a host hands [`MenuRuntime::tick`] for one tick, from
/// the pad word's **edges** (`held & !previous_held`) - the one decode both
/// play hosts use, so neither can feed the runtime a held word.
///
/// Nothing inside the runtime filters repeats: every screen it drives moves
/// a cursor or commits on the input it is handed. The native window used to
/// build its input from the held word, so one key press lasting a few ticks
/// moved the shop cursor several rows or walked straight through the buy
/// confirm, while the browser play page (which sends one edge per press)
/// stepped once. The inverse of [`menu_input_pad_word`].
pub fn menu_input_from_pad_edges(edge: u16) -> MenuInput {
    use legaia_engine_system::input::PadButton;
    let on = |b: PadButton| edge & b.mask() != 0;
    MenuInput {
        cross: on(PadButton::Cross),
        circle: on(PadButton::Circle),
        triangle: on(PadButton::Triangle),
        square: on(PadButton::Square),
        up: on(PadButton::Up),
        down: on(PadButton::Down),
        left: on(PadButton::Left),
        right: on(PadButton::Right),
    }
}

#[derive(Debug, Clone)]
enum PendingOp {
    Save { slot: u8 },
    Load { slot: u8 },
}

impl MenuRuntime {
    pub fn new(save_dir: impl Into<PathBuf>) -> Self {
        Self {
            ctx: MenuCtx::default(),
            save_dir: save_dir.into(),
            slot_count: 3,
            selected_char: 0,
            shop_session: None,
            inn_session: None,
            trade_session: None,
            trade_pending_offer: 0,
            equip_info: None,
            retail_equipment_buy: false,
            recipient_session: None,
            quantity_session: None,
            prize_session: None,
            stay_cursor: None,
            point_card_toast: None,
            shop_slides: legaia_engine_menus::shop::ShopSlides::default(),
            ui_frames: 0,
            shop_age: 0,
            quantity_list_row: 0,
            ui_cue: None,
            spell_level_notice: None,
            art_learned_notice: None,
            pending: None,
            widget_state_seen: 0,
        }
    }
}

struct MenuRuntimeHost<'a> {
    /// Set when a confirm landed on a row the list refuses (a buy the purse
    /// cannot cover): the list kernel's dim-row buzz instead of a confirm.
    refused: &'a mut bool,
    world: &'a mut World,
    slot_count: u8,
    pending: &'a mut Option<PendingOp>,
    selected_char: &'a mut usize,
    shop_session: &'a mut Option<ShopSession>,
    inn_session: &'a mut Option<InnSession>,
    trade_session: &'a mut Option<legaia_engine_battle::seru_trade::SeruTradeSession>,
    trade_pending_offer: &'a mut usize,
    equip_info: &'a Option<DiscEquipInfo>,
    retail_equipment_buy: bool,
    recipient_session: &'a mut Option<BuyRecipientSession>,
    stay_cursor: &'a mut Option<u8>,
    point_card_toast: &'a mut Option<i32>,
}

#[cfg(test)]
mod tests;
