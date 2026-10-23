//! Field-menu sub-session dispatcher.
//!
//! Hooks the seven [`legaia_engine_menus::field_menu::FieldMenuRow`] selections to their
//! respective renderer-agnostic sub-sessions:
//!
//! | Row     | Sub-session                                           |
//! |---------|-------------------------------------------------------|
//! | Items   | [`legaia_engine_menus::inventory_use::InventoryUseSession`]         |
//! | Equip   | [`legaia_engine_menus::equip_session::EquipSession`]                |
//! | Spells  | [`legaia_engine_menus::spell_menu::SpellMenuSession`]               |
//! | Arts    | [`legaia_engine_battle::tactical_arts_editor::ChainEditor`]          |
//! | Status  | [`legaia_engine_menus::status_screen::StatusScreenSession`]         |
//! | Save    | [`legaia_engine_menus::save_select::SaveSelectSession`] (Save mode) |
//! | Config  | [`legaia_engine_menus::options::OptionsSession`]                    |
//!
//! Pure plumbing - the dispatcher builds the right sub-session from
//! [`World`] state, routes per-frame pad input into it, and exposes
//! `is_done` for the engine to call [`legaia_engine_menus::field_menu::FieldMenuSession::resume`].
//! Side-effects (writing equipment back to a record, casting a spell on
//! the active party, persisting a save) are intentionally left to the
//! engine - see [`apply_equip_outcome`] / [`apply_inventory_outcome`] /
//! [`apply_spell_outcome`] / [`apply_arts_outcome`] for the typed helpers.

use crate::bag_rows::WorldBagRows;
use legaia_engine_battle::battle_stats::{EquipmentTable, StatusModifiers};
use legaia_engine_battle::magic_xp::SpellLevelNotice;
use legaia_engine_battle::spells::SpellCatalog;
use legaia_engine_battle::tactical_arts_editor::{ChainEditor, ChainLibrary, EditInput};
use legaia_engine_core::world::World;
use legaia_engine_menus::equip_session::{EquipInput, EquipOutcome, EquipSession};
use legaia_engine_menus::field_menu::FieldMenuRow;
use legaia_engine_menus::inventory_use::{
    InventoryContext, InventoryUseSession, TargetRow as InvTargetRow,
};
use legaia_engine_menus::list_order::{LIST_ORDER_STEP_MAGIC, ListOrderRow, ListOrderSession};
use legaia_engine_menus::options::{OptionsInput, OptionsSession, OptionsState};
use legaia_engine_menus::pause_screens::ItemDisplayText;
use legaia_engine_menus::pause_screens::stat_record_from_character;
use legaia_engine_menus::pause_screens::{PauseItemRow, PauseItemsSession};
use legaia_engine_menus::save_select::{SaveRack, SaveSelectMode, SaveSelectSession, SelectInput};
use legaia_engine_menus::spell_menu::{
    CasterSlot as SpellCasterSlot, SpellMenuInput, SpellMenuOutcome, SpellMenuSession,
    TargetRow as SpellTargetRow,
};
use legaia_engine_menus::status_screen::{
    ElementRankView, EquipSlotView, StatusInput, StatusScreenSession, StatusSnapshot,
};
use legaia_engine_system::input::{Mapping, PadButton};

mod arts_editor;
mod outcomes;
mod session_builders;
mod status;
#[cfg(test)]
mod tests;
mod tick;

pub use arts_editor::*;
pub use outcomes::*;
pub use session_builders::*;
pub use status::*;
pub use tick::*;

/// One of the seven sub-sessions that can be active beneath a suspended
/// [`legaia_engine_menus::field_menu::FieldMenuSession`].
pub enum FieldMenuSubsession {
    /// The retail Items screen (command window + 12-row list pages +
    /// info window) layered over the item-use flow. The inner
    /// [`InventoryUseSession`] stays the outcome carrier -
    /// [`apply_inventory_outcome`] takes `&session.inner`.
    Items(PauseItemsSession),
    /// Equip session paired with the slot of the character whose record is
    /// being edited so the caller can write the result back to the right
    /// roster member.
    ///
    /// `picking` is retail's character picker `0x12` (`FUN_801D98F0`), the
    /// Equip row's first step: the pad walks the present party and the slot
    /// rows show the hovered member's equipment; a confirm hands the pad to
    /// the slot browse (`0x13`), whose cancel comes back here.
    Equip {
        session: EquipSession,
        char_slot: u8,
        picking: bool,
    },
    Spells(SpellMenuSession),
    /// The per-character list page with the spell list's **reorder**.
    ///
    /// Not a pause-menu row of its own. It is the list half of menu-overlay
    /// sub-screen `0x15` (`FUN_801DA2A0`), and `0x15` is what the root
    /// picker's **row 3 - Status** routes to: `0x801D6C4C` is the only site
    /// in PROT 0899 that writes `0x15` into the submenu word `DAT_801E46A4`,
    /// and it sits in the root picker `FUN_801D6B20`'s row-3 arm. So the
    /// screen this page hangs off is the Status screen, and a confirm there
    /// is retail's own entry to it.
    ListOrder(ListOrderSession),
    Arts(ChainEditor),
    Status(StatusScreenSession),
    Save(SaveSelectSession),
    Config(OptionsSession),
}

impl FieldMenuSubsession {
    /// Construct the sub-session matching `row`. Engines that want to
    /// override one of the construction inputs (e.g. supply a custom
    /// equipment table for Equip, or a saved-chain library for Arts)
    /// should build that variant directly.
    /// `save_rack` is the model behind the Load / Save rows - and its kind
    /// is what puts the session in retail's two-stage card flow, so a host
    /// never decides that separately. See [`SaveRack`].
    pub fn build(
        row: FieldMenuRow,
        world: &World,
        options: &OptionsState,
        save_rack: &SaveRack,
        chain_library: &ChainLibrary,
        spell_catalog: &SpellCatalog,
        equipment_table: &EquipmentTable,
    ) -> Self {
        // `chain_library` remains a build input so engines can construct
        // the Arts editor variant directly (no retail pause-menu row
        // opens it - retail's list is Items / Magic / Equip / Status /
        // Options / Load / Save).
        let _ = chain_library;
        match row {
            FieldMenuRow::Items => Self::Items(build_pause_items_session(world)),
            FieldMenuRow::Equip => {
                // The picker opens on the leader - the head of the present
                // party list retail's picker walks.
                let first = active_leader_slot(world);
                let mut session = build_equip_session(world, first, equipment_table);
                session.set_slot_cursor_hidden(true);
                Self::Equip {
                    session,
                    char_slot: first,
                    picking: true,
                }
            }
            FieldMenuRow::Magic => Self::Spells(build_spell_session(world, spell_catalog)),
            FieldMenuRow::Status => Self::Status(
                StatusScreenSession::new(status_snapshots(world))
                    .with_spell_rows(status_spell_rows(world, spell_catalog)),
            ),
            FieldMenuRow::Load => {
                Self::Save(SaveSelectSession::for_rack(SaveSelectMode::Load, save_rack))
            }
            FieldMenuRow::Save => {
                Self::Save(SaveSelectSession::for_rack(SaveSelectMode::Save, save_rack))
            }
            FieldMenuRow::Options => Self::Config(OptionsSession::new(options.clone())),
        }
    }

    /// Return the [`FieldMenuRow`] this subsession was built for.
    pub fn row(&self) -> FieldMenuRow {
        match self {
            Self::Items(_) => FieldMenuRow::Items,
            Self::Equip { .. } => FieldMenuRow::Equip,
            Self::Spells(_) => FieldMenuRow::Magic,
            // The page hangs off the Status screen (retail's sub-screen
            // `0x15`), so a resume drops the hand back on that row.
            Self::ListOrder(_) => FieldMenuRow::Status,
            // The Arts chain editor is an engine extension with no
            // retail pause-menu row; park the resume cursor on Status
            // (the retail surface that lists a character's arts).
            Self::Arts(_) => FieldMenuRow::Status,
            Self::Status(_) => FieldMenuRow::Status,
            Self::Save(s) => match s.mode() {
                SaveSelectMode::Load => FieldMenuRow::Load,
                _ => FieldMenuRow::Save,
            },
            Self::Config(_) => FieldMenuRow::Options,
        }
    }

    /// Arm the Options sub-session's engine-only **Key Config** screen (Select) against
    /// the host's live keyboard binding table. No-op on every other variant.
    ///
    /// A post-construction hook rather than a [`Self::build`] argument: the
    /// binding table is a host concern (the native binary's
    /// `legaia-input.toml`, the page's `localStorage` entry) and every
    /// headless caller of `build` - replay drivers, oracles, the disc-gated
    /// menu tests - has no table to offer and should keep the retail ten
    /// rows.
    pub fn arm_key_rebind(&mut self, mapping: Mapping) {
        if let Self::Config(s) = self {
            s.arm_key_rebind(mapping);
        }
    }

    /// The Options sub-session's live binding table once a rebind committed,
    /// paired with "it changed since you last asked". `None` on every other
    /// variant, and on an Options session whose Key Config screen was never
    /// armed.
    ///
    /// The host's persist cue: native writes `legaia-input.toml`, the page
    /// writes the binding key it restores from on load.
    pub fn take_rebound_mapping(&mut self) -> Option<Mapping> {
        let Self::Config(s) = self else {
            return None;
        };
        if !s.take_bindings_dirty() {
            return None;
        }
        s.mapping().cloned()
    }

    /// Drive one frame using a PSX-encoded edge-triggered "newly pressed"
    /// pad bitmask. Each variant's tick method receives the matching
    /// per-button input bundle.
    pub fn tick_pad_edge(&mut self, pressed: u16) {
        self.tick_pad_edge_with_key(pressed, None);
    }

    /// [`Self::tick_pad_edge`] carrying the host's most-recent keyboard key
    /// name, in the engine's own key vocabulary
    /// ([`legaia_engine_system::input::KEY_NAME_DOM_CODES`]). Consumed only by the Options
    /// sub-session's key-rebind screen while it awaits a key; every other
    /// variant ignores it.
    pub fn tick_pad_edge_with_key(&mut self, pressed: u16, key_pressed: Option<&str>) {
        match self {
            Self::Items(s) => s.input_pad_edge(pressed),
            Self::Equip {
                session, picking, ..
            } if *picking => {
                // Retail's character picker `0x12` (`FUN_801D98F0`): confirm
                // hands the pad to the slot browse `0x13`, cancel leaves the
                // Equip screen. Moving between members needs the world to
                // rebuild the session, so [`tick_open_subsession`] takes the
                // Up / Down edges before this arm.
                if pressed & PadButton::Cross.mask() != 0 {
                    *picking = false;
                    session.set_slot_cursor_hidden(false);
                    session.reopen_slot_browse();
                } else if pressed & PadButton::Circle.mask() != 0 {
                    session.cancel();
                }
            }
            Self::Equip {
                session, picking, ..
            } => {
                session.input(EquipInput {
                    up: pressed & PadButton::Up.mask() != 0,
                    down: pressed & PadButton::Down.mask() != 0,
                    left: pressed & PadButton::Left.mask() != 0,
                    right: pressed & PadButton::Right.mask() != 0,
                    cross: pressed & PadButton::Cross.mask() != 0,
                    circle: pressed & PadButton::Circle.mask() != 0,
                    triangle: pressed & PadButton::Triangle.mask() != 0,
                });
                // The slot browse's cancel returns to the character picker
                // (`0x13` cancel -> `0x12`), not to the root list.
                if session.outcome() == Some(EquipOutcome::Cancelled) {
                    *picking = true;
                    session.set_slot_cursor_hidden(true);
                    session.reopen_slot_browse();
                }
            }
            Self::Spells(s) => {
                let _ = s.tick(SpellMenuInput::from_pad_edge(pressed));
            }
            Self::ListOrder(s) => {
                let _ = s.tick(pressed);
            }
            Self::Arts(s) => {
                let square = pressed & PadButton::Square.mask() != 0;
                let _ = s.tick(EditInput {
                    up: pressed & PadButton::Up.mask() != 0,
                    down: pressed & PadButton::Down.mask() != 0,
                    left: pressed & PadButton::Left.mask() != 0,
                    right: pressed & PadButton::Right.mask() != 0,
                    cross: pressed & PadButton::Cross.mask() != 0,
                    circle: pressed & PadButton::Circle.mask() != 0,
                    triangle: pressed & PadButton::Triangle.mask() != 0,
                    square,
                    // Square doubles as "cycle name" while in the naming
                    // phase; the editor's tick path ignores name_next
                    // outside that phase.
                    name_next: square,
                });
            }
            Self::Status(s) => {
                // Confirm on the shown character opens the reorder page -
                // retail's own route, the confirm arm of sub-screen `0x15`'s
                // character picker (`0x801DA3C8`). An empty list takes the
                // reject arm (`ListOrderSession::open` answers `None`) and
                // the press does nothing, exactly as retail buzzes and stays.
                if pressed & PadButton::Cross.mask() != 0
                    && let Some(order) = open_status_list_order(s)
                {
                    *self = Self::ListOrder(order);
                    return;
                }
                let _ = s.tick(StatusInput::from_pad_edge(pressed));
            }
            Self::Save(s) => {
                let _ = s.tick(SelectInput {
                    up: pressed & PadButton::Up.mask() != 0,
                    down: pressed & PadButton::Down.mask() != 0,
                    left: pressed & PadButton::Left.mask() != 0,
                    right: pressed & PadButton::Right.mask() != 0,
                    cross: pressed & PadButton::Cross.mask() != 0,
                    circle: pressed & PadButton::Circle.mask() != 0,
                    triangle: pressed & PadButton::Triangle.mask() != 0,
                });
            }
            Self::Config(s) => {
                let _ = s.tick_with_key(OptionsInput::from_pad_edge(pressed), key_pressed);
            }
        }
    }

    /// `true` once the inner sub-session has reached its terminal state.
    /// The shell should then call
    /// [`legaia_engine_menus::field_menu::FieldMenuSession::resume`] to drop control
    /// back into the field menu.
    pub fn is_done(&self) -> bool {
        match self {
            Self::Items(s) => s.is_done(),
            Self::Equip { session, .. } => session.is_done(),
            Self::Spells(s) => s.is_done(),
            Self::ListOrder(s) => s.is_done(),
            Self::Arts(s) => s.is_done(),
            Self::Status(s) => s.is_done(),
            Self::Save(s) => s.is_done(),
            Self::Config(s) => s.is_done(),
        }
    }
}
