//! Retail pause-menu **Items** / **Magic** screen sessions + view models.
//!
//! The draw builders live in `legaia-engine-ui`
//! (`ui_menu/pause_lists.rs`: `items_screen_draws_for` /
//! `magic_screen_draws_for`); this module is the renderer-agnostic data
//! side both hosts (play-window + the web play page) feed them from:
//!
//! - [`MenuTextTables`] - the disc-derived text: item names + info-window
//!   descriptions (`PTR_DAT_8007436C`, `docs/formats/item-table.md`),
//!   spell names / MP / descriptions (`DAT_800754C8` + the `0x80075DB0`
//!   description pointer table, `docs/formats/spell-table.md`) and the
//!   accessory passive name/description table (`0x8007625C`,
//!   `docs/formats/accessory-passive-table.md`).
//! - [`PauseItemsSession`] - the retail Items screen's focus model
//!   (command window -> list) layered over the item-use flow
//!   ([`crate::inventory_use::InventoryUseSession`]), with real bag
//!   counts and 12-row list paging.
//! - [`items_screen_model`] / [`magic_screen_model`] - owned view models
//!   the hosts map 1:1 onto the engine-ui `PauseItemsView` /
//!   `PauseMagicView` structs.
//!
//! Retail provenance for the layouts + phase words is in
//! `docs/subsystems/field-menu.md` (`FUN_801D0D18` command window,
//! `FUN_801DCB60`/`FUN_801D0F1C` item info, `FUN_801D2C98` caster window,
//! `FUN_801D2E74` spell info).

use crate::input::PadButton;
use crate::inventory_use::{InventoryUseInput, InventoryUseSession, InventoryUseState};
use crate::spell_menu::{SpellMenuPhase, SpellMenuSession};
use legaia_engine_vm::battle_formulas::{MpCostModifier, mp_cost_after_ability_bits};

/// Rows per list page (both retail list windows show 12 rows filling the
/// 182-px content height at the 0xE pitch).
pub const LIST_PAGE_ROWS: usize = 12;

/// Ra-Seru summon spell-id block (`Palma`..`Ozma`, the egg-derived
/// summons): these rows lead with the wider winged element icon in the
/// spell list. See `docs/formats/spell-table.md`.
pub const RA_SERU_SPELL_IDS: std::ops::RangeInclusive<u8> = 0x9A..=0xA0;

/// Disc-derived pause-menu text tables (best-effort per table; every
/// lookup has a caller-side fallback so a PROT.DAT-only load still
/// renders ids).
#[derive(Debug, Clone, Default)]
pub struct MenuTextTables {
    /// Item names + info-window descriptions.
    pub item_names: Option<legaia_asset::item_names::ItemNameTable>,
    /// Spell names / MP / info-window descriptions.
    pub spell_names: Option<legaia_asset::spell_names::SpellNameTable>,
    /// Accessory ("Goods") passive name/description records - the green +
    /// white lines of the item info window's extra widget box.
    pub passives: Option<legaia_asset::accessory_passive::AccessoryPassiveTable>,
    /// The arts-name table (`DAT_80075EC4`, [`legaia_art::arts_table`]) - what
    /// the `0xC5` markup token resolves against, keyed on `[character, art]`.
    pub arts: Option<Vec<legaia_art::arts_table::ArtTableEntry>>,
    /// The post-battle level-up window's seven lines, raw MES bytes, index
    /// `mask - 1` (screen elements `0x45..=0x4B`,
    /// [`legaia_asset::screen_elements::RECORD_LEVEL_UP_BASE`]).
    pub level_up_lines: Option<Vec<Vec<u8>>>,
    /// The report window's drop line template, raw MES bytes
    /// ([`legaia_asset::screen_elements::drop_line_template`]).
    pub drop_line: Option<Vec<u8>>,
}

impl MenuTextTables {
    /// Parse all three tables out of a `SCUS_942.54` image (each
    /// best-effort).
    pub fn from_scus(scus: &[u8]) -> Self {
        Self {
            item_names: legaia_asset::item_names::ItemNameTable::from_scus(scus),
            spell_names: legaia_asset::spell_names::SpellNameTable::from_scus(scus),
            passives: legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus(scus),
            arts: legaia_art::arts_table::parse_from_scus(scus),
            level_up_lines: (1..=7)
                .map(|mask| {
                    legaia_asset::screen_elements::payload_string(
                        scus,
                        legaia_asset::screen_elements::RECORD_LEVEL_UP_BASE + mask,
                    )
                })
                .collect(),
            drop_line: legaia_asset::screen_elements::drop_line_template(scus),
        }
    }

    /// The arts name the `0xC5` token resolves for `[character, art]` - the
    /// table record whose row is the character and whose column is the art.
    pub fn art_name(&self, character: u8, art: u8) -> Option<&str> {
        self.arts
            .as_ref()?
            .iter()
            .find(|e| e.character as u8 == character && e.index == art)
            .map(|e| e.name.as_str())
    }

    /// Display name for item `id`, or `None`.
    pub fn item_name(&self, id: u8) -> Option<&str> {
        self.item_names.as_ref()?.name(id)
    }

    /// Info-window description for item `id`, or `None`.
    pub fn item_desc(&self, id: u8) -> Option<&str> {
        self.item_names.as_ref()?.desc(id)
    }

    /// Display name for spell `id`, or `None`.
    pub fn spell_name(&self, id: u8) -> Option<&str> {
        self.spell_names.as_ref()?.name(id)
    }

    /// Info-window description for spell `id`, or `None`.
    pub fn spell_desc(&self, id: u8) -> Option<&str> {
        self.spell_names.as_ref()?.desc(id)
    }

    /// The accessory passive lines for item `id`: `(green name line,
    /// white description)` from the `0x8007625C` record's `+4` / `+8`
    /// strings. The description is whole, its `'|'` breaks kept: every
    /// window that prints it (`FUN_801D0F1C`'s extra box, the shop's info and
    /// detail windows) goes through the line-breaking printer
    /// (`FUN_80036888`), which puts the rest one row pitch down. This used to
    /// keep the first line only, on the reading that the box shows one line.
    pub fn item_passive_text(&self, id: u8) -> Option<(String, String)> {
        let (_, record) = self.passives.as_ref()?.passive(id)?;
        let name = record.name.clone()?;
        Some((name, record.description.clone().unwrap_or_default()))
    }
}

/// One bag row of the Items screen, resolved at session build.
#[derive(Debug, Clone, Default)]
pub struct PauseItemRow {
    pub id: u8,
    /// Physical **bag slot** this row's payload names (retail
    /// `_DAT_8007BB88`). `0` when the host built the session without a
    /// slot-indexed bag - see [`crate::inventory_use::InventoryUseSession::bag_slots`].
    pub slot: u8,
    pub name: String,
    /// Real bag count (the world inventory count, not the session's
    /// one-entry-per-id item list length).
    pub count: u8,
    /// Info-window description (empty when the disc text is unavailable).
    pub desc: String,
    /// Accessory passive lines for the extra widget box.
    pub passive: Option<(String, String)>,
}

/// Focus of the Items screen (the retail submenu word `DAT_801E46A4`:
/// `5` = command window, `6` = the Use list, `7` = the Throw Out list;
/// the Throw Out confirm is submenu 7's phase 3, `FUN_801D8734`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseItemsFocus {
    /// Hand on the Use / Throw Out / Arrange command window.
    Command,
    /// Hand inside the item list (the Use flow, submenu 6).
    List,
    /// Hand inside the item list picking a stack to discard (submenu 7,
    /// `FUN_801D8734` phases 0..2).
    ThrowOutList,
    /// The Yes / No throw-out confirm window (descriptor id 9, renderer
    /// `FUN_801D1B20`; `FUN_801D8734` phase 3).
    ThrowOutConfirm,
    /// One of the three special Use routes has the screen - submenu `0xB`
    /// (Door of Light, Yes/No window 10, renderer `FUN_801D1DAC`), `0xC`
    /// (Door of Wind, destination list window 11) or `0xD` (Incense,
    /// Yes/No window 12, renderer `FUN_801D1F10`). Distinct from
    /// [`Self::ThrowOutConfirm`]: different windows, different renderers,
    /// and the two Yes/No routes seed the cursor to **Yes** rather than No.
    /// The live state is [`PauseItemsSession::special_use`], whose
    /// [`SpecialUsePhase`] says which of the two screen shapes is open.
    SpecialRoute,
}

/// The retail Items screen session: the command-window/list focus model
/// layered over the item-use flow. The inner
/// [`InventoryUseSession`] stays the behaviour driver (admissibility
/// filter, target select, outcome) - hosts keep applying its outcome via
/// `legaia_engine_core::field_menu_dispatch::apply_inventory_outcome` with
/// [`Self::inner`].
pub struct PauseItemsSession {
    /// The item-use flow. Its `items` list is id-sorted, one entry per
    /// distinct bag id, parallel to [`Self::rows`]. NB its browsing
    /// cursor walks `filtered_items` (usable-in-context rows only);
    /// retail's list hand walks **every** bag row, so the screen keeps
    /// its own flat [`Self::list_cursor`] and only maps into the inner
    /// flow on a confirm.
    pub inner: InventoryUseSession,
    /// Resolved per-row display data (parallel to `inner.items`).
    pub rows: Vec<PauseItemRow>,
    pub focus: PauseItemsFocus,
    /// Command-window row (0 = Use, 1 = Throw Out, 2 = Arrange).
    pub command_cursor: u8,
    /// Throw-out confirm row (0 = Yes, 1 = No). Retail seeds the confirm
    /// cursor word `DAT_801E46D0` to `1` on open - "No" is the default.
    pub confirm_cursor: u8,
    /// The live special Use route, while one is open. Boxed to keep the
    /// session (and the `FieldMenuSubsession` enum carrying it) small.
    special_use: Option<Box<SpecialUseSession>>,
    /// Guards the one-shot commit of the live route's terminal outcome.
    /// The route's session stays readable after it finishes (hosts and
    /// tests read its outcome), so without this a host that ticks the
    /// screen again before noticing [`Self::is_done`] would consume a
    /// second copy of the item.
    special_committed: bool,
    /// The Door of Wind destination rows, resolved at session build from
    /// the disc placement table + the live discovery flags
    /// (`legaia_engine_core::field_menu_dispatch::warp_destinations`). Empty when the
    /// executable was not reachable at boot - the route then opens an
    /// empty list rather than an invented one.
    warp_destinations: Vec<WarpDestination>,
    /// Destination a committed Door of Wind pick staged, in retail's
    /// `0x80084624`/`28`/`2C` shape. Drained by
    /// `legaia_engine_core::field_menu_dispatch::apply_pause_items_outcome` onto
    /// `legaia_engine_core::world::MenuState::pending_warp`.
    staged_warp: Option<StagedWarp>,
    /// Menu exit code the finished screen hands the outer menu SM
    /// (`_DAT_8007B43C`): [`MENU_EXIT_CODE_FIELD_ESCAPE`] or
    /// [`MENU_EXIT_CODE_WORLD_MAP_WARP`]. `None` on every ordinary close.
    exit_code: Option<u32>,
    /// Arrange sort ranks (id -> rank). `None` falls back to the id-order
    /// identity ([`crate::menu_arrange::ArrangeRankTable::id_order`]).
    /// Boxed to keep the session (and the `FieldMenuSubsession` enum
    /// carrying it) small.
    arrange_rank: Option<Box<crate::menu_arrange::ArrangeRankTable>>,
    /// Bag slots in whatever order the rows were in when the command window
    /// dispatched Throw Out - restored on the way back. Captured live rather
    /// than taken from the build, because Arrange reorders the rows in place
    /// and restoring a build-time order would silently undo it.
    restore_slots: Vec<u8>,
    /// Bag slots in the **Throw Out** list's order (content id `0x22`), which
    /// is a different build of the same bag: an equipment piece sorts to the
    /// tail whether or not its record refuses discard, and the key-item /
    /// no-discard gates only change the ink. Empty on a disc-free load.
    throw_out_slots: Vec<u8>,
    /// Flat hand position over [`Self::rows`] (all bag rows).
    cursor: usize,
    /// Set when the player backs out of the command window (Circle /
    /// Triangle) - the screen is finished without an item use.
    closed: bool,
    /// Incense confirms committed while the screen was up. Each one is a
    /// `FUN_800402F4` class-`0x82` apply - one `FUN_80046870` top-up of the
    /// Incense window - which
    /// `legaia_engine_core::field_menu_dispatch::apply_pause_items_outcome` replays onto
    /// the world when the screen finishes.
    incense_uses: u8,
    /// The Use list's Incense row is greyed: the Incense window already
    /// holds `0xE0` or more walk ticks. See [`Self::with_incense_window`].
    incense_blocked: bool,
    /// The Incense window as this screen sees it: the live word at open,
    /// topped up by every confirm the screen commits, so the gate greys the
    /// row on the use that reaches `0xE0` rather than only on the next open.
    incense_window: i32,
}

impl PauseItemsSession {
    pub fn new(inner: InventoryUseSession, rows: Vec<PauseItemRow>) -> Self {
        Self {
            inner,
            rows,
            focus: PauseItemsFocus::Command,
            command_cursor: 0,
            confirm_cursor: 1,
            special_use: None,
            special_committed: false,
            warp_destinations: Vec::new(),
            staged_warp: None,
            exit_code: None,
            arrange_rank: None,
            restore_slots: Vec::new(),
            throw_out_slots: Vec::new(),
            cursor: 0,
            closed: false,
            incense_uses: 0,
            incense_blocked: false,
            incense_window: 0,
        }
    }

    /// Seed the Incense row's gate from the live Incense window
    /// (`_DAT_8007B600`, the engine's
    /// `legaia_engine_core::world::FieldLocomotion::walk_regen_window`).
    ///
    /// Retail greys the row in the Use-list build: content id 3 asks
    /// `FUN_8003043C` whether the item would do anything, which runs the
    /// action validator `FUN_8003FB10` on the item-effect record's class byte,
    /// and Incense's class `0x82` arm is the three-instruction leaf
    /// `FUN_80046898` - `_DAT_8007B600 < 0xE0` (`slti v0,v0,0xe0` at
    /// `0x800468A0`). A greyed row's confirm is a buzz in the kind-4 list
    /// kernel, so a window at or past `0xE0` ticks refuses another Incense
    /// before its Yes/No window ever opens.
    ///
    /// REF: FUN_80046898 (the gate; ported as
    /// `legaia_engine_vm::battle_action::item_count_gate`)
    pub fn with_incense_window(mut self, window: i32) -> Self {
        self.incense_window = window;
        self.incense_blocked = !legaia_engine_vm::battle_action::item_count_gate(window);
        self
    }

    /// Incense confirms this screen committed (see [`Self::with_incense_window`]).
    pub fn incense_uses(&self) -> u8 {
        self.incense_uses
    }

    /// Attach the **Throw Out** row order (`FUN_80030628` content id `0x22`)
    /// as a bag-slot sequence. The screen is built in the Use order (content
    /// id 3) and swaps to this one when the command window dispatches row 1,
    /// swapping back on the way out.
    pub fn with_throw_out_row_order(mut self, throw_out_slots: Vec<u8>) -> Self {
        self.throw_out_slots = throw_out_slots;
        self
    }

    /// Attach the Door of Wind destination rows (the visible placement
    /// records; see [`WarpDestination`]).
    pub fn with_warp_destinations(mut self, destinations: Vec<WarpDestination>) -> Self {
        self.warp_destinations = destinations;
        self
    }

    /// The Door of Wind destination rows this screen would offer.
    pub fn warp_destinations(&self) -> &[WarpDestination] {
        &self.warp_destinations
    }

    /// The menu exit code a finished special route handed the outer menu SM
    /// (`_DAT_8007B43C`), if any. `4` = the dungeon escape, `5` = the
    /// world-map warp.
    pub fn exit_code(&self) -> Option<u32> {
        self.exit_code
    }

    /// The destination a committed Door of Wind pick staged.
    pub fn staged_warp(&self) -> Option<StagedWarp> {
        self.staged_warp
    }

    /// Attach the disc-parsed Arrange rank table
    /// ([`crate::menu_arrange::parse_arrange_rank_table`]).
    pub fn with_arrange_rank(
        mut self,
        rank: Option<crate::menu_arrange::ArrangeRankTable>,
    ) -> Self {
        self.arrange_rank = rank.map(Box::new);
        self
    }

    /// The retail command-window grey-out: the bag scan found no held
    /// item.
    pub fn bag_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Flat hand position over the full bag list (every row, not just
    /// the context-usable ones - the hand can rest on a non-usable row;
    /// confirming it buzzes, matching retail).
    pub fn list_cursor(&self) -> usize {
        self.cursor
    }

    /// 1-based current page of the list.
    pub fn page(&self) -> u16 {
        (self.list_cursor() / LIST_PAGE_ROWS) as u16 + 1
    }

    /// Total page count: `ceil(rows / 12)`, `0` for an empty list.
    ///
    /// Retail's kind-4 list kernel `FUN_80032A44` recovers the total by
    /// stepping `visible`-sized pages until the accumulator reaches the row
    /// count (`0x80032e44..0x80032e78`), and draws no PAGE header at all
    /// for a zero count (`beq a0,zero` at `0x80032e18`). The row count is
    /// the **occupied** slot count: the Use-list builder `FUN_80030628`
    /// skips an empty slot (`beq s0,zero,0x80030a0c` at `0x8003089c`). So
    /// one held item reads `PAGE 1/ 1` with no page-turn arrow - the bag's
    /// capacity never enters the figure.
    pub fn pages(&self) -> u16 {
        self.rows.len().div_ceil(LIST_PAGE_ROWS) as u16
    }

    /// `true` while the item-use flow is in its target-select phase (the
    /// host overlays the target picker).
    pub fn target_select(&self) -> bool {
        matches!(self.inner.state, InventoryUseState::TargetSelect { .. })
    }

    /// Session finished (backed out of the command window, or the inner
    /// use flow reached `Done`).
    pub fn is_done(&self) -> bool {
        self.closed || self.inner.is_done()
    }

    /// Drive one frame from an edge-triggered PSX pad word.
    ///
    /// - **Command focus** (retail submenu 5, `FUN_801D7C00`): Up/Down
    ///   cycle the three rows; the bag scan gates every confirm (empty =
    ///   buzz no-op). Cross on "Use" enters the list (submenu 6), on
    ///   "Throw Out" enters the discard list (submenu 7), on "Arrange"
    ///   runs the bag sort (`FUN_801D64A8`) and resets the list scroll.
    ///   Circle/Triangle close the screen.
    /// - **List focus** (Use): Up/Down move the hand with the retail
    ///   kernel's page-local wrap, Left/Right flip 12-row pages (the
    ///   only scroll - [`list_kernel_navigate`]), Cross confirms into
    ///   the use flow, Circle returns to the command window.
    /// - **Throw Out list** (`FUN_801D8734` phase 2): same navigation;
    ///   Cross opens the Yes/No confirm seeded on "No"; Circle returns
    ///   to the command window.
    /// - **Throw Out confirm** (phase 3): Up/Down toggle Yes/No; Cross
    ///   on Yes discards the whole stack (the retail delete zeroes both
    ///   bag-slot bytes) and returns to the list - or to the command
    ///   window when the bag empties; Cross on No / Circle back out.
    /// - **Target select**: everything forwards to the inner flow.
    //
    // PORT: FUN_801D7C00 (items command SM: submenu routing + Arrange phase)
    // PORT: FUN_801D8734 (throw-out list + confirm SM)
    // PORT: FUN_801D8308 (single-target apply SM, phases 0..2: preview-mode
    //   staging via target_panel_mode, party-row navigate, confirm
    //   revalidation buzz (retail FUN_8003FB10 -> InvalidConfirm), one
    //   apply. The post-apply repeat-stay (retail phase 7 returns the hand
    //   to the party rows while stock and applicability hold), the notify
    //   window (script 0x801E4C60) and the 20-frame exhaustion timer
    //   collapse into the session's single-apply Done.)
    // PORT: FUN_801D7FF8 (the sibling ALL-party apply SM - retail submenu
    //   9, the `flags & 0x20` arm of use_route_for_effect: same preview
    //   staging via FUN_801D6A54, but its picker runs with count 0
    //   (`FUN_801D688C(&DAT_801E46C4, 0, 0)` at 0x801d80a4 - confirm /
    //   cancel only, no target rows), cancel drops to the Use list
    //   (submenu 6), confirm cues SFX 0x25 and applies to every member
    //   through the same FUN_800402F4 + FUN_80042558 chain with one bag
    //   decrement (FUN_80043048) and the FUN_8003043C applicability
    //   re-probe. The session's ApplyAll arm is this flow.)
    pub fn input_pad_edge(&mut self, pressed: u16) {
        let up = pressed & PadButton::Up.mask() != 0;
        let down = pressed & PadButton::Down.mask() != 0;
        let cross = pressed & PadButton::Cross.mask() != 0;
        let circle = pressed & PadButton::Circle.mask() != 0;
        let triangle = pressed & PadButton::Triangle.mask() != 0;

        if self.target_select() {
            if let Some(ev) = simple_inventory_input(pressed) {
                self.inner.input(ev);
            }
            return;
        }
        match self.focus {
            PauseItemsFocus::Command => {
                if circle || triangle {
                    self.closed = true;
                    return;
                }
                if up {
                    self.command_cursor = (self.command_cursor + 2) % 3;
                }
                if down {
                    self.command_cursor = (self.command_cursor + 1) % 3;
                }
                // Retail scans the bag before dispatching any command row
                // and buzzes (SFX 0x23) on an empty bag.
                if cross && !self.bag_empty() {
                    match self.command_cursor {
                        0 => self.focus = PauseItemsFocus::List,
                        // Retail opens a *different* list window here (content
                        // id `0x22`, window 16) with its own build, not the
                        // Use list re-pointed: a key item dims in place, an
                        // equipment piece sorts to the tail, and the
                        // effect-flag-`0x8` group goes last. The order the
                        // rows are in right now is what the back-out restores,
                        // so an Arrange the player just ran survives the trip.
                        1 => {
                            self.restore_slots = self.rows.iter().map(|r| r.slot).collect();
                            self.reorder_rows_by_slot(&self.throw_out_slots.clone());
                            self.focus = PauseItemsFocus::ThrowOutList;
                        }
                        _ => self.arrange(),
                    }
                }
            }
            PauseItemsFocus::List => {
                if circle {
                    self.focus = PauseItemsFocus::Command;
                    return;
                }
                self.list_navigate(pressed);
                if cross {
                    // Retail's Use dispatch routes on the hovered item's
                    // effect class before it ever opens the target panel:
                    // classes `0x80` / `0x82` branch into submenus 0xB /
                    // 0xD, which raise their own confirm window instead
                    // (`use_route_for_effect`). The bag ids of those two
                    // routes are fixed, so the branch keys on the id -
                    // the class lookup is the general form and needs the
                    // item-effect record the row does not carry.
                    if let Some(route) = self
                        .rows
                        .get(self.cursor)
                        .and_then(|r| special_use_route_for_item(r.id))
                    {
                        // A greyed Incense row buzzes in the list kernel
                        // (`with_incense_window`) - no confirm window opens.
                        if route == UseRoute::Incense && self.incense_blocked {
                            return;
                        }
                        // Only the Door of Wind route reads the landmark
                        // list; the two Yes/No routes open with an empty
                        // one, exactly as `SpecialUseSession::new` expects.
                        let landmarks = if route == UseRoute::DoorOfWind {
                            self.warp_destinations
                                .iter()
                                .map(|d| d.name.clone())
                                .collect()
                        } else {
                            Vec::new()
                        };
                        self.special_use = Some(Box::new(SpecialUseSession::new(route, landmarks)));
                        self.special_committed = false;
                        self.focus = PauseItemsFocus::SpecialRoute;
                        return;
                    }
                    // Map the hand row into the inner flow's filtered
                    // cursor space; a non-usable row has no mapping and
                    // the confirm is a buzz no-op (retail).
                    if let Some(fpos) = self
                        .inner
                        .filtered_items
                        .iter()
                        .position(|&ix| ix == self.cursor)
                    {
                        if let InventoryUseState::Browsing { cursor } = &mut self.inner.state {
                            *cursor = fpos;
                        }
                        self.inner.input(InventoryUseInput::Confirm);
                    }
                }
            }
            PauseItemsFocus::ThrowOutList => {
                if circle {
                    // Retail: list result 3 -> restore the id-15 list
                    // window and return to submenu 5. That window carries the
                    // other build, so the order the screen arrived in goes
                    // back with it.
                    self.reorder_rows_by_slot(&self.restore_slots.clone());
                    self.focus = PauseItemsFocus::Command;
                    return;
                }
                self.list_navigate(pressed);
                if cross && self.cursor < self.rows.len() {
                    // Confirm window opens seeded on "No"
                    // (`DAT_801E46D0 = 1`).
                    self.confirm_cursor = 1;
                    self.focus = PauseItemsFocus::ThrowOutConfirm;
                }
            }
            PauseItemsFocus::ThrowOutConfirm => {
                if circle {
                    self.focus = PauseItemsFocus::ThrowOutList;
                    return;
                }
                // FUN_801D688C over 2 rows with wrap.
                if up || down {
                    self.confirm_cursor ^= 1;
                }
                if cross {
                    if self.confirm_cursor == 0 {
                        self.throw_out_selected();
                    } else {
                        self.focus = PauseItemsFocus::ThrowOutList;
                    }
                }
            }
            PauseItemsFocus::SpecialRoute => {
                let Some(sp) = self.special_use.as_mut() else {
                    self.focus = PauseItemsFocus::List;
                    return;
                };
                sp.input_pad_edge(pressed);
                let SpecialUsePhase::Done(outcome) = sp.phase.clone() else {
                    return;
                };
                if self.special_committed {
                    return;
                }
                self.special_committed = true;
                // Every committing route hands `FUN_80042310(id, 1)` before
                // it leaves - the one-copy bag decrement. `consumed_items`
                // is what carries it to the world applier.
                let consumed = sp.consumed_item_id();
                if let Some(id) = consumed {
                    self.inner.consumed_items.push(id);
                }
                match outcome {
                    // Door of Light hands the field the escape exit code
                    // and closes the whole menu; Door of Wind stages its
                    // destination and closes with the warp code; Incense
                    // applies in place and drops back to the Use list, and
                    // a cancel does the same without consuming.
                    SpecialUseOutcome::FieldEscape => {
                        self.exit_code = Some(MENU_EXIT_CODE_FIELD_ESCAPE);
                        self.closed = true;
                    }
                    SpecialUseOutcome::Warp { landmark } => {
                        // `landmark` is the visible row; the placement
                        // record behind it carries the staged triple.
                        self.staged_warp =
                            self.warp_destinations.get(landmark).map(|d| StagedWarp {
                                scene_id: d.scene_id,
                                menu_x: d.menu_x,
                                menu_y: d.menu_y,
                            });
                        self.exit_code = Some(MENU_EXIT_CODE_WORLD_MAP_WARP);
                        self.closed = true;
                    }
                    SpecialUseOutcome::EncounterSuppress => {
                        self.incense_uses = self.incense_uses.saturating_add(1);
                        // The applier's `+0x40` lands on the live window at
                        // once, and the Use list the route returns to greys
                        // the row off that word (`FUN_80046898`).
                        self.incense_window =
                            legaia_engine_vm::battle_helpers::top_up_cooldown(self.incense_window);
                        self.incense_blocked =
                            !legaia_engine_vm::battle_action::item_count_gate(self.incense_window);
                        if let Some(id) = consumed {
                            self.take_one_copy(id);
                        }
                        self.focus = PauseItemsFocus::List;
                    }
                    SpecialUseOutcome::Cancelled => {
                        self.focus = PauseItemsFocus::List;
                    }
                }
            }
        }
    }

    /// The live special Use route, while its confirm window is open.
    /// The host reads the finished session's
    /// [`SpecialUseSession::consumed_item_id`] /
    /// [`SpecialUseSession::exit_code`] to apply the outcome.
    pub fn special_use(&self) -> Option<&SpecialUseSession> {
        self.special_use.as_deref()
    }

    /// Drop a finished special route once the host has applied it.
    pub fn take_special_use(&mut self) -> Option<SpecialUseSession> {
        self.special_use.take().map(|b| *b)
    }

    /// Shared list navigation - the retail kind-4 list kernel's pad
    /// decode (see [`list_kernel_navigate`]).
    fn list_navigate(&mut self, pressed: u16) {
        self.cursor = list_kernel_navigate(self.cursor, self.rows.len(), pressed);
    }

    /// The Arrange command: sort the bag rows by the rank table and
    /// reset the list scroll (retail zeroes `_DAT_8007BB90` /
    /// `_DAT_8007BB98` before re-opening the list window).
    ///
    /// The engine's bag rows carry no holes (one row per held id), so
    /// the kernel's empty-slot sink never engages here; the visible
    /// effect is the rank reorder.
    // REF: FUN_801D64A8 (kernel lives in crate::menu_arrange)
    fn arrange(&mut self) {
        let rank = self
            .arrange_rank
            .as_deref()
            .cloned()
            .unwrap_or_else(crate::menu_arrange::ArrangeRankTable::id_order);
        // Sort rows and the inner parallel id list together via the
        // shared kernel over (id, count) pairs.
        let mut pairs: Vec<(u8, u8)> = self.rows.iter().map(|r| (r.id, r.count.max(1))).collect();
        crate::menu_arrange::arrange_bag_slots(&mut pairs, &rank);
        let mut reordered = Vec::with_capacity(self.rows.len());
        let mut remaining: Vec<PauseItemRow> = std::mem::take(&mut self.rows);
        for (id, _) in pairs {
            if let Some(at) = remaining.iter().position(|r| r.id == id) {
                reordered.push(remaining.remove(at));
            }
        }
        reordered.extend(remaining);
        self.rows = reordered;
        self.inner.items = self.rows.iter().map(|r| r.id).collect();
        self.inner.refresh_filter();
        self.cursor = 0;
    }

    /// Permute the visible rows into the order `slots` names, keeping the
    /// inner session's parallel id list in lockstep (the same pairing
    /// [`Self::arrange`] maintains). Slots the current row set does not hold
    /// are skipped, and any row the order does not name keeps its relative
    /// position at the tail - so a stale order degrades to "no reorder"
    /// rather than to a lost row.
    fn reorder_rows_by_slot(&mut self, slots: &[u8]) {
        if slots.is_empty() {
            return;
        }
        let mut remaining: Vec<PauseItemRow> = std::mem::take(&mut self.rows);
        let mut reordered = Vec::with_capacity(remaining.len());
        for &slot in slots {
            if let Some(at) = remaining.iter().position(|r| r.slot == slot) {
                reordered.push(remaining.remove(at));
            }
        }
        reordered.extend(remaining);
        self.rows = reordered;
        self.inner.items = self.rows.iter().map(|r| r.id).collect();
        self.inner.refresh_filter();
        self.cursor = 0;
    }

    /// The throw-out delete: discard the selected row's whole stack
    /// (retail zeroes both bytes of the bag slot pair), step the hand
    /// back when it sat on the last row, and drop back to the command
    /// window when the bag scan comes up empty.
    /// One copy of `id` leaves the bag the moment a special route commits:
    /// retail's Incense Yes calls `FUN_80042310(0x8A, 1)` at `0x801D8E68`,
    /// before the applier and before the route drops back to the Use list -
    /// so the row the list returns to shows one fewer, and the last copy's
    /// row is gone. Without this the screen kept offering an Incense the bag
    /// no longer held, and one copy could be confirmed up to the window cap.
    // REF: FUN_80042310 (the one-copy bag decrement the route calls)
    fn take_one_copy(&mut self, id: u8) {
        let at = if self.rows.get(self.cursor).is_some_and(|r| r.id == id) {
            Some(self.cursor)
        } else {
            self.rows.iter().position(|r| r.id == id)
        };
        let Some(at) = at else {
            return;
        };
        let row = &mut self.rows[at];
        row.count = row.count.saturating_sub(1);
        if row.count == 0 {
            self.rows.remove(at);
            self.inner.remove_item_at(at);
            self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        }
    }

    fn throw_out_selected(&mut self) {
        if self.cursor >= self.rows.len() {
            self.focus = PauseItemsFocus::ThrowOutList;
            return;
        }
        let row = self.rows.remove(self.cursor);
        self.inner.thrown_items.push(row.id);
        // Retail's confirm zeroes `bag[cursor*2]` - the slot the row's payload
        // named, not the first slot holding that id.
        self.inner.thrown_slots.push(row.slot);
        self.inner.remove_item_at(self.cursor);
        // Retail scroll fix-up: deleting the last list entry steps the
        // selection (and scroll) back one row.
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        self.focus = if self.rows.is_empty() {
            PauseItemsFocus::Command
        } else {
            PauseItemsFocus::ThrowOutList
        };
    }
}

/// The retail list-window pad decode - the SCUS kind-4 list kernel
/// `FUN_80032A44`'s navigation phase, in flat-cursor form (the kernel
/// keeps `scroll top` (`node+0x0`) and `selected` (`node+0x6`)
/// separately; page starts stay `LIST_PAGE_ROWS`-aligned under these
/// moves, so `top = cursor - cursor % ROWS` is an invariant):
///
/// - **Up** (held `0x1000`, `80032ae8..80032c74`): selection `-1` while
///   above the page top; at the page top it wraps to the page's last
///   row (`80032b28`: `sel = top + visible - 1`, clamped to the row
///   count at `80032c5c..80032c6c`).
/// - **Down** (`0x4000`, `80032b44..80032b84`): selection `+1`; stepping
///   past the page bottom (`sel+1 == top+visible`, `80032b68`) or past
///   the last row (`sel+1 == count`, `80032b78` fallthrough) wraps back
///   to the page top (`80032b80` restores `node+0x0`).
/// - **Left** (`0x8000`, `80032b90..80032c0c`): page up - only while
///   `top > 0`; both top and selection step back one page.
/// - **Right** (`0x2000`, `80032c1c..80032c50`): page down - only while
///   `top + visible < count`; selection clamps to the last row.
///
/// Up/Down never scroll - the only scrolling is the Left/Right page
/// flip, which is why the retail lists read as fixed 12-row pages.
///
/// PORT: FUN_80032A44 (kind-4 list kernel - navigation phase)
pub fn list_kernel_navigate(cursor: usize, n: usize, pressed: u16) -> usize {
    list_kernel_navigate_rows(cursor, n, pressed, LIST_PAGE_ROWS)
}

/// [`list_kernel_navigate`] for a list window whose page holds `rows` rows -
/// the kernel's `visible = (content_h - 4) / 0xE` for that window (12 for
/// the pause lists, 7 for the shop's buy list, 11 for its sell list).
///
/// PORT: FUN_80032A44 (kind-4 list kernel - navigation phase)
pub fn list_kernel_navigate_rows(cursor: usize, n: usize, pressed: u16, rows: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let rows = rows.max(1);
    let mut c = cursor.min(n - 1);
    let top = c - c % rows;
    if pressed & PadButton::Up.mask() != 0 {
        c = if c > top {
            c - 1
        } else {
            (top + rows).min(n) - 1
        };
    }
    if pressed & PadButton::Down.mask() != 0 {
        let top = c - c % rows;
        c = if (c + 1).is_multiple_of(rows) || c + 1 == n {
            top
        } else {
            c + 1
        };
    }
    if pressed & PadButton::Left.mask() != 0 {
        let top = c - c % rows;
        if top > 0 {
            c -= rows;
        }
    }
    if pressed & PadButton::Right.mask() != 0 {
        let top = c - c % rows;
        if top + rows < n {
            c = (c + rows).min(n - 1);
        }
    }
    c
}

fn simple_inventory_input(pressed: u16) -> Option<InventoryUseInput> {
    if pressed & PadButton::Up.mask() != 0 {
        Some(InventoryUseInput::Up)
    } else if pressed & PadButton::Down.mask() != 0 {
        Some(InventoryUseInput::Down)
    } else if pressed & PadButton::Cross.mask() != 0 {
        Some(InventoryUseInput::Confirm)
    } else if pressed & PadButton::Circle.mask() != 0 {
        Some(InventoryUseInput::Cancel)
    } else {
        None
    }
}

/// Owned view model of the Items screen - maps 1:1 onto the engine-ui
/// `PauseItemsView`.
#[derive(Debug, Clone, Default)]
pub struct ItemsScreenModel {
    /// The current page's visible rows: `(name, count)`.
    pub page_rows: Vec<(String, u16)>,
    pub page: u16,
    pub pages: u16,
    /// `true` = hand inside the list (rows drop to the grey staging-0
    /// ink); `false` = command-window focus (rows white).
    pub focus_list: bool,
    pub command_cursor: u8,
    /// List row on the current page.
    pub list_cursor_on_page: u8,
    pub bag_empty: bool,
    /// Info-window content for the staged (hovered) item.
    pub info: Option<ItemsInfoModel>,
    /// `true` while the use flow is picking a target - hosts overlay the
    /// target picker.
    pub target_select: bool,
    /// The Throw Out confirm window content (descriptor id 9, renderer
    /// `FUN_801D1B20`) - `Some` while the Yes/No prompt is open. Hosts
    /// draw it with `engine-ui::items_throw_confirm_draws_for` over the
    /// command window (the retail confirm slides the command window out
    /// and window 9 in).
    pub throw_confirm: Option<ThrowConfirmModel>,
    /// The special Use route's own confirm window content - `Some` while
    /// submenu `0xB` (Door of Light) or `0xD` (Incense) has its Yes/No
    /// prompt open. A different window and renderer from `throw_confirm`;
    /// hosts draw it with `engine-ui::confirm_prompt_draws`.
    pub special_confirm: Option<SpecialConfirmModel>,
}

/// Special Use-route confirm window content - the shape both
/// `FUN_801D1DAC` (window 10, Door of Light) and `FUN_801D1F10`
/// (window 12, Incense) render.
#[derive(Debug, Clone)]
pub struct SpecialConfirmModel {
    /// Which route raised the window - it picks the descriptor rect and
    /// the one-line vs three-line renderer.
    pub route: UseRoute,
    /// Name of the item being used, staged as the prompt's first line.
    pub item_name: String,
    /// 0 = Yes, 1 = No. Retail seeds these two windows to **Yes**,
    /// unlike the Throw Out confirm.
    pub cursor: u8,
}

/// Throw Out confirm window content (`FUN_801D1B20`).
#[derive(Debug, Clone, Default)]
pub struct ThrowConfirmModel {
    /// Name of the stack about to be discarded.
    pub name: String,
    /// Its bag count (the whole stack is discarded).
    pub count: u16,
    /// 0 = Yes, 1 = No (retail defaults to No).
    pub cursor: u8,
}

/// Item info window content (`FUN_801DCB60` / `FUN_801D0F1C`).
#[derive(Debug, Clone, Default)]
pub struct ItemsInfoModel {
    pub name: String,
    pub count: u16,
    pub desc: String,
    pub passive: Option<(String, String)>,
    /// The staged row is the **Point Card** (`0xFE`), which retail's shared
    /// info panel branches on before anything else: `FUN_801D0F1C` compares
    /// the staged id against `0xFE` at `0x801d0fc0` and, on a match, draws
    /// its "Points Left" label + the `_DAT_800845B4` bank and **jumps past**
    /// the whole passive / scope-pictogram block.
    ///
    /// The bank itself is not here because this model is built from the
    /// session alone; a host reads `legaia_engine_core::world::MinigameState::point_card` and
    /// calls `engine-ui`'s `item_points_panel_draws`. The passive lines stay
    /// `None` on this row without needing a suppression: the Point Card's
    /// effect descriptor carries the `0x41` no-passive sentinel.
    pub is_point_card: bool,
}

/// The Items screen's model while the Door of Wind destination list
/// (retail submenu `0xC`, window 11) has the screen.
///
/// The rows are the unlocked landmarks and the hand is the list kernel's,
/// so the page maths is the shared one. Two deliberate residuals, both
/// stated rather than papered over:
///
/// - the row **count column draws `0`**, because the shared list row model
///   carries a `count` and the port has no window-11 renderer of its own
///   to drop it; retail's destination rows have no count column;
/// - the info window stays closed (`info: None`), which is retail - the
///   staged-id gate `DAT_801E46B0` is not restaged by this submenu.
fn destination_list_model(s: &PauseItemsSession, sp: &SpecialUseSession) -> ItemsScreenModel {
    let n = sp.landmarks.len();
    let cursor = sp.cursor.min(n.saturating_sub(1));
    let start = (cursor / LIST_PAGE_ROWS) * LIST_PAGE_ROWS;
    ItemsScreenModel {
        page_rows: sp
            .landmarks
            .iter()
            .skip(start)
            .take(LIST_PAGE_ROWS)
            .map(|name| (name.clone(), 0))
            .collect(),
        page: (start / LIST_PAGE_ROWS) as u16 + 1,
        pages: n.div_ceil(LIST_PAGE_ROWS).max(1) as u16,
        focus_list: true,
        command_cursor: s.command_cursor,
        list_cursor_on_page: (cursor - start) as u8,
        bag_empty: false,
        info: None,
        target_select: false,
        throw_confirm: None,
        special_confirm: None,
    }
}

/// Assemble the Items screen view model from a live session.
pub fn items_screen_model(s: &PauseItemsSession) -> ItemsScreenModel {
    // The Door of Wind destination list is a list window like the Use list
    // (retail window 11, driven by the same kind-4 kernel), so it projects
    // through the same rows / page / cursor channel rather than needing a
    // second one - which is what lets both hosts draw it with the list
    // renderer they already call.
    if let Some(sp) = s.special_use()
        && sp.phase == SpecialUsePhase::PickDestination
    {
        return destination_list_model(s, sp);
    }
    let cursor = s.list_cursor();
    let page0 = cursor / LIST_PAGE_ROWS;
    let start = page0 * LIST_PAGE_ROWS;
    let page_rows = s
        .rows
        .iter()
        .skip(start)
        .take(LIST_PAGE_ROWS)
        .map(|r| (r.name.clone(), r.count as u16))
        .collect();
    // Retail gates the info window on the staged id `DAT_801E46B0`: the
    // command SM's init phase zeroes it, the Use / Throw Out list phases
    // restage it from the hovered slot every frame.
    let info = if s.focus == PauseItemsFocus::Command {
        None
    } else {
        s.rows.get(cursor).map(|r| ItemsInfoModel {
            name: r.name.clone(),
            count: r.count as u16,
            desc: r.desc.clone(),
            passive: r.passive.clone(),
            is_point_card: r.id == crate::shop::POINT_CARD_ITEM_ID,
        })
    };
    let throw_confirm = if s.focus == PauseItemsFocus::ThrowOutConfirm {
        s.rows.get(cursor).map(|r| ThrowConfirmModel {
            name: r.name.clone(),
            count: r.count as u16,
            cursor: s.confirm_cursor,
        })
    } else {
        None
    };
    let special_confirm = s.special_use().and_then(|sp| {
        matches!(sp.phase, SpecialUsePhase::Confirm).then(|| SpecialConfirmModel {
            route: sp.route,
            item_name: s
                .rows
                .get(cursor)
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            cursor: sp.cursor as u8,
        })
    });
    ItemsScreenModel {
        page_rows,
        page: s.page(),
        pages: s.pages(),
        // The hand sits inside the list for the Use list and both Throw
        // Out phases (rows drop to the grey staging-0 ink in all three).
        focus_list: matches!(
            s.focus,
            PauseItemsFocus::List
                | PauseItemsFocus::ThrowOutList
                | PauseItemsFocus::ThrowOutConfirm
        ),
        command_cursor: s.command_cursor,
        list_cursor_on_page: (cursor - start) as u8,
        bag_empty: s.bag_empty(),
        info,
        target_select: s.target_select(),
        throw_confirm,
        special_confirm,
    }
}

/// Owned view model of the Magic screen - maps 1:1 onto the engine-ui
/// `PauseMagicView`.
#[derive(Debug, Clone, Default)]
pub struct MagicScreenModel {
    /// Caster blocks: `(name, level, mp, mp_max)`.
    pub casters: Vec<(String, u8, u16, u16)>,
    /// The current page's visible spell rows: `(name, ra_seru)`.
    pub page_rows: Vec<(String, bool)>,
    pub page: u16,
    pub pages: u16,
    /// `true` = hand inside the spell list; `false` = caster-window focus.
    pub focus_list: bool,
    pub caster_cursor: u8,
    pub list_cursor_on_page: u8,
    pub info: Option<MagicInfoModel>,
    /// `true` while the cast flow is picking a target.
    pub target_select: bool,
}

/// Spell info window content (`FUN_801D2E74`).
#[derive(Debug, Clone, Default)]
pub struct MagicInfoModel {
    pub name: String,
    /// Learned spell level (record `+0x161` list).
    pub level: u8,
    /// Description (line breaks are `'\n'`).
    pub desc: String,
    pub mp_cost: u16,
    pub ra_seru: bool,
}

/// Assemble the Magic screen view model from a live [`SpellMenuSession`].
///
/// Phase map: `CharSelect` = caster focus (the hovered caster's list
/// shows white), `SpellSelect` = list focus (rows grey, hovered spell
/// staged into the info window), `TargetSelect` = the host overlays the
/// target picker, `GroupConfirm` = the no-pick group flow, which draws the
/// list exactly as `SpellSelect` does and no picker at all. `text` fills
/// descriptions; names fall back
/// catalog -> spell-name table -> `Spell XX`.
pub fn magic_screen_model(s: &SpellMenuSession, text: Option<&MenuTextTables>) -> MagicScreenModel {
    let casters: Vec<(String, u8, u16, u16)> = s
        .party()
        .iter()
        .map(|c| (c.name.clone(), c.level.max(1), c.mp, c.mp_max.max(c.mp)))
        .collect();

    let (caster_idx, focus_list, list_cursor, target_select) = match s.phase() {
        SpellMenuPhase::CharSelect { cursor } => (*cursor as usize, false, 0usize, false),
        SpellMenuPhase::SpellSelect { caster, cursor } => {
            (*caster as usize, true, *cursor as usize, false)
        }
        SpellMenuPhase::TargetSelect { caster, cursor, .. } => {
            (*caster as usize, true, *cursor as usize, true)
        }
        // Retail's group sub-screen `0x10` draws no target rows: the spell
        // list keeps the hand and the info window keeps the staged spell.
        SpellMenuPhase::GroupConfirm { caster, cursor, .. } => {
            (*caster as usize, true, *cursor as usize, false)
        }
        SpellMenuPhase::Done(_) => (0, false, 0, false),
    };

    let spell_name = |id: u8| -> String {
        s.catalog()
            .get(id)
            .map(|d| d.name.clone())
            .or_else(|| text.and_then(|t| t.spell_name(id)).map(str::to_string))
            .unwrap_or_else(|| format!("Spell {id:02X}"))
    };

    let spells: Vec<u8> = s
        .party()
        .get(caster_idx)
        .map(|c| c.spells.clone())
        .unwrap_or_default();
    // `0` for a caster with no spells: the list kernel draws no PAGE header
    // at a zero row count (`FUN_80032A44`, `beq a0,zero` at `0x80032e18`).
    let pages = spells.len().div_ceil(LIST_PAGE_ROWS) as u16;
    // In caster focus the hovered caster's list previews from page 1; the
    // list cursor only exists in list focus.
    let cursor = if focus_list { list_cursor } else { 0 };
    let page0 = if spells.is_empty() {
        0
    } else {
        (cursor / LIST_PAGE_ROWS).min(spells.len().div_ceil(LIST_PAGE_ROWS) - 1)
    };
    let start = page0 * LIST_PAGE_ROWS;
    let page_rows: Vec<(String, bool)> = spells
        .iter()
        .skip(start)
        .take(LIST_PAGE_ROWS)
        .map(|id| (spell_name(*id), RA_SERU_SPELL_IDS.contains(id)))
        .collect();

    // Info: the staged spell (hovered list row) - only while the hand is
    // in the list (retail gates on the staged id `DAT_801E46B0`).
    let info = if focus_list {
        spells.get(cursor).map(|id| {
            let level = s
                .party()
                .get(caster_idx)
                .map(|c| c.spell_level(cursor))
                .unwrap_or(1);
            let desc = text
                .and_then(|t| t.spell_desc(*id))
                .unwrap_or_default()
                .to_string();
            let base_cost = s
                .catalog()
                .get(*id)
                .map(|d| d.mp_cost as u16)
                .or_else(|| {
                    text.and_then(|t| t.spell_names.as_ref())
                        .and_then(|t| t.mp(*id))
                        .map(u16::from)
                })
                .unwrap_or(0);
            // Route the displayed cost through the per-caster MP-cost kernel
            // (`FUN_80035394`) so the Magic screen shows the discounted cost
            // an MP-saver ability actually charges, matching the battle path
            // (`BattleSpellSession::new` / `World::cast_spell_on_slots`).
            let ability_bits = s
                .party()
                .get(caster_idx)
                .map(|c| c.ability_bits)
                .unwrap_or(0);
            let mp_cost = mp_cost_after_ability_bits(
                base_cost,
                MpCostModifier::from_ability_flags(ability_bits),
            );
            MagicInfoModel {
                name: spell_name(*id),
                level,
                desc,
                mp_cost,
                ra_seru: RA_SERU_SPELL_IDS.contains(id),
            }
        })
    } else {
        None
    };

    MagicScreenModel {
        casters,
        page_rows,
        page: page0 as u16 + 1,
        pages,
        focus_list,
        caster_cursor: caster_idx as u8,
        list_cursor_on_page: (cursor - start) as u8,
        info,
        target_select,
    }
}

/// The window-14 target-panel preview mode for a picked item - the
/// retail preview word `DAT_801E46CC` derivation: only an item whose
/// record kind byte (`0x80074368 + id*0xC + 0`) is `2` **and** whose
/// item-effect class (`0x800752C0 + eff*4 + 0`) is `6` (the
/// permanent-stat Waters) previews; the effect arg (`+1`) maps `0 -> 1`
/// (Life Water), `1 -> 2` (Power Water / ATK), `2 -> 3` (Guardian
/// Water / UDF+LDF), `3 -> 4` (Swift Water / SPD), `4 -> 5` (Wisdom
/// Water / INT), `5 -> 1` (Magic Water shares the HP/MP panel).
/// Everything else is mode `0` - the plain `cur/max` panel.
///
/// PORT: FUN_801D6A54 (target-panel preview-mode derivation)
///
/// Driven from [`target_panel_view_model_of`], the host entry point for the
/// window-14 panel: the staged bag id resolves the item record's kind
/// byte and effect descriptor through the world's disc-parsed
/// [`legaia_asset::item_effect::ItemEffectTable`].
pub fn target_panel_mode(item_kind: u8, effect_class: u8, effect_arg: u8) -> u32 {
    if item_kind != 2 || effect_class != 6 {
        return 0;
    }
    match effect_arg {
        0 | 5 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 5,
        _ => 0,
    }
}

/// Fixed bag ids the three special Use routes consume (`FUN_80042310` /
/// `FUN_80043048` calls with literal ids in the submenu handlers).
pub const DOOR_OF_LIGHT_ITEM_ID: u8 = 0x88;
pub const DOOR_OF_WIND_ITEM_ID: u8 = 0x89;
pub const INCENSE_ITEM_ID: u8 = 0x8A;

/// Menu exit codes the special routes hand to the outer menu state
/// machine (`_DAT_8007B43C`, with the `DAT_801E46A0 = 0xF2` fade): `4` =
/// the Door of Light dungeon-escape handoff, `5` = the Door of Wind
/// world-map warp.
pub const MENU_EXIT_CODE_FIELD_ESCAPE: u32 = 4;
pub const MENU_EXIT_CODE_WORLD_MAP_WARP: u32 = 5;

/// The item-effect **class byte** (`0x800752C0 + eff*4 + 0`) of the three
/// special Use items. These three ids are the only ones retail's dispatch
/// (`FUN_801D7E50`) sends to a dedicated submenu, and each one's class is
/// fixed disc data, so the engine can key the class off the id while
/// [`PauseItemRow`] carries no effect record of its own.
fn special_use_effect_class(item_id: u8) -> Option<u8> {
    match item_id {
        DOOR_OF_LIGHT_ITEM_ID => Some(0x80),
        DOOR_OF_WIND_ITEM_ID => Some(0x81),
        INCENSE_ITEM_ID => Some(0x82),
        _ => None,
    }
}

/// Which of the three special Use routes - if any - a bag id opens.
///
/// All three route out of the ordinary target-panel flow at the same place
/// (`FUN_801D7E50` phase 2), and they differ only in the screen they raise:
/// Door of Light raises the Yes/No window 10 (`FUN_801D1DAC`), Incense the
/// Yes/No window 12 (`FUN_801D1F10`), and Door of Wind the destination
/// **list** window 11, driven by the kind-4 list kernel rather than a
/// picker. [`SpecialUseSession::new`] is what turns the route into the
/// right opening phase, so all three belong here.
///
/// The route itself comes from [`use_route_for_effect`] - the ported
/// dispatch is the decision point, and this wrapper only supplies the
/// class byte.
pub fn special_use_route_for_item(item_id: u8) -> Option<UseRoute> {
    // The all-party flag byte is irrelevant on this path: the three class
    // bytes below are all matched before `use_route_for_effect` looks at
    // it, which is why 0 is a faithful stand-in for the record's `+2`.
    match use_route_for_effect(special_use_effect_class(item_id)?, 0) {
        route @ (UseRoute::DoorOfLight | UseRoute::DoorOfWind | UseRoute::Incense) => Some(route),
        // The two generic apply routes are not special-route screens.
        _ => None,
    }
}

/// One row of the Door of Wind destination list - a **visible** placement
/// record from the quick-travel table (`DAT_80073A98`, 6-byte stride;
/// parser [`legaia_asset::worldmap_menu`]).
///
/// The visible set is built by the same walk the world-map landmark menu
/// runs (`FUN_80030628` case `0x19`, `0x80031870..0x800318dc`): each record
/// is skipped when its `name_idx` repeats the last **accepted** row's, then
/// gated on the system flag at `record[1] + 0x20` (`FUN_8003CE64`); an
/// accepted row is pushed as the string id `0x8000 | record_index`, which
/// `FUN_8002FF8C` resolves back through `names[placement[index].name_idx]`.
/// [`Self::record_index`] is that record ordinal - retail's
/// `_DAT_8007BB88`, which `FUN_801D8B90` phase 3 scales by 6 straight back
/// into the same table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarpDestination {
    /// Ordinal of the placement record in `DAT_80073A98` (**not** the
    /// visible row ordinal - locked landmarks leave gaps).
    pub record_index: u32,
    /// Landmark name from `DAT_80073B18`.
    pub name: String,
    /// Destination scene id, record `+2`. Retail stages it into the
    /// world-state word `0x80084628`.
    pub scene_id: u16,
    /// World-map marker x, record `+4` -> `0x80084624`.
    pub menu_x: u8,
    /// World-map marker y, record `+5` -> `0x8008462C`.
    pub menu_y: u8,
}

/// The destination a committed Door of Wind use staged, in the shape
/// `FUN_801D8B90` phase 3 writes it (`0x80084624` / `0x80084628` /
/// `0x8008462C`) before handing the outer menu SM exit code
/// [`MENU_EXIT_CODE_WORLD_MAP_WARP`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagedWarp {
    pub scene_id: u16,
    pub menu_x: u8,
    pub menu_y: u8,
}

/// Which submenu a confirmed Use-list pick routes to - the
/// `FUN_801D7E50` phase-2 dispatch on the picked item's effect class
/// (`801d7f80..801d7fd8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseRoute {
    /// Effect flag bit `0x20` set (all-party): submenu 9
    /// (`FUN_801D7FF8`) - the target panel opens in all-row hand mode
    /// with no row navigation.
    ApplyAll,
    /// Default route: submenu 0xA (`FUN_801D8308`) - single-target pick
    /// over the party rows.
    ApplySingle,
    /// Effect class `0x80` (Door of Light): submenu 0xB
    /// (`FUN_801D8A58`).
    DoorOfLight,
    /// Effect class `0x81` (Door of Wind): submenu 0xC
    /// (`FUN_801D8B90`).
    DoorOfWind,
    /// Effect class `0x82` (Incense): submenu 0xD (`FUN_801D8D94`).
    Incense,
}

/// Route a confirmed Use pick by its item-effect record: class byte
/// `0x80`/`0x81`/`0x82` take the dedicated flows; anything else goes to
/// the all-party apply when the flag byte (`+2`) has bit `0x20`, else
/// the single-target apply.
///
/// Driven from [`special_confirm_route_for_item`], which every Use-list
/// confirm runs. The `ApplyAll` / `ApplySingle` split it also decides is
/// not consulted there: the engine's target shape comes from the inner
/// [`InventoryUseSession`], which reads its own catalog rather than the
/// item-effect flag byte.
///
/// PORT: FUN_801D7E50 (Use-list phase-2 effect-class dispatch)
pub fn use_route_for_effect(effect_class: u8, effect_flags: u8) -> UseRoute {
    match effect_class {
        0x80 => UseRoute::DoorOfLight,
        0x81 => UseRoute::DoorOfWind,
        0x82 => UseRoute::Incense,
        _ if effect_flags & 0x20 != 0 => UseRoute::ApplyAll,
        _ => UseRoute::ApplySingle,
    }
}

/// Terminal result of a special Use route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialUseOutcome {
    /// Backed out - retail returns to the Use list (submenu 6) without
    /// consuming anything.
    Cancelled,
    /// Door of Light confirmed: one `0x88` consumed; the menu closes
    /// with exit code [`MENU_EXIT_CODE_FIELD_ESCAPE`] (the field-side
    /// dungeon-escape handoff).
    FieldEscape,
    /// Door of Wind destination picked: one `0x89` consumed; the menu
    /// closes with exit code [`MENU_EXIT_CODE_WORLD_MAP_WARP`].
    /// `landmark` indexes the quick-travel placement table (retail
    /// `0x80073A98`, 6-byte records - `legaia_asset::worldmap_menu`);
    /// retail stages record `+2`/`+4`/`+5` into the world-state words
    /// `0x80084628`/`0x80084624`/`0x8008462C` before the handoff.
    Warp { landmark: usize },
    /// Incense confirmed: one `0x8A` consumed and the class-`0x82`
    /// encounter-suppression effect applied through the SCUS item-effect
    /// applier (`FUN_800402F4`); the flow drops back to the Use list.
    EncounterSuppress,
}

/// Phase of a [`SpecialUseSession`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialUsePhase {
    /// Yes/No confirm (Door of Light / Incense). Unlike the Throw Out
    /// confirm, retail seeds the cursor to **0 - "Yes"**
    /// (`801d8ab4` / `801d8df0` zero `DAT_801E46D0`).
    Confirm,
    /// Door of Wind destination list (window 11, driven by the kind-4
    /// list kernel; the hand hides while the kernel idles).
    PickDestination,
    Done(SpecialUseOutcome),
}

/// State machine for the three special Use routes (submenus
/// 0xB / 0xC / 0xD). The session is pure routing - the host applies the
/// outcome (consume the fixed item id, close the menu with the exit
/// code, or apply the encounter suppression).
///
/// The three `PORT:` tags sit on the **arms** rather than here, and that
/// placement is load-bearing: a tag on the type resolves, for the runtime
/// reach join, to the type's first function - `new` - which every route
/// constructs. All three addresses would then read *entered* the moment any
/// one route ran, rather than each answering for its own arm.
pub struct SpecialUseSession {
    pub route: UseRoute,
    /// Destination names for the Door of Wind list (unlocked landmarks,
    /// in placement-table order).
    pub landmarks: Vec<String>,
    /// Confirm row (0 = Yes) or destination row.
    pub cursor: usize,
    pub phase: SpecialUsePhase,
}

impl SpecialUseSession {
    /// Start the route's flow. `DoorOfWind` opens the destination list;
    /// `DoorOfLight` / `Incense` open the Yes/No confirm seeded on Yes.
    /// (`ApplyAll` / `ApplySingle` are not special routes - they keep
    /// the target-panel flow and construct no session here.)
    pub fn new(route: UseRoute, landmarks: Vec<String>) -> Self {
        let phase = match route {
            UseRoute::DoorOfWind => SpecialUsePhase::PickDestination,
            _ => SpecialUsePhase::Confirm,
        };
        Self {
            route,
            landmarks,
            cursor: 0,
            phase,
        }
    }

    /// The fixed bag id the finished route consumed, if any.
    pub fn consumed_item_id(&self) -> Option<u8> {
        match &self.phase {
            SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape) => Some(DOOR_OF_LIGHT_ITEM_ID),
            SpecialUsePhase::Done(SpecialUseOutcome::Warp { .. }) => Some(DOOR_OF_WIND_ITEM_ID),
            SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress) => Some(INCENSE_ITEM_ID),
            _ => None,
        }
    }

    /// The menu exit code the finished route hands to the outer menu SM
    /// (`_DAT_8007B43C`), if the route exits the menu.
    pub fn exit_code(&self) -> Option<u32> {
        match &self.phase {
            SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape) => {
                Some(MENU_EXIT_CODE_FIELD_ESCAPE)
            }
            SpecialUsePhase::Done(SpecialUseOutcome::Warp { .. }) => {
                Some(MENU_EXIT_CODE_WORLD_MAP_WARP)
            }
            _ => None,
        }
    }

    /// Drive one frame from an edge-triggered PSX pad word.
    pub fn input_pad_edge(&mut self, pressed: u16) {
        match self.phase {
            SpecialUsePhase::Confirm => self.confirm_input(pressed),
            SpecialUsePhase::PickDestination => self.pick_destination_input(pressed),
            SpecialUsePhase::Done(_) => {}
        }
    }

    /// The Yes/No confirm window shared by the two confirm routes. One body,
    /// because retail's two routines differ only in which window descriptor
    /// they raise and which outcome the Yes row commits: Door of Light hands
    /// the field the escape exit code, Incense applies the class-`0x82`
    /// encounter suppression in place.
    ///
    /// PORT: FUN_801D8A58 (Door of Light confirm + exit-code 4 handoff)
    /// PORT: FUN_801D8D94 (Incense confirm + class-0x82 apply)
    fn confirm_input(&mut self, pressed: u16) {
        let up = pressed & PadButton::Up.mask() != 0;
        let down = pressed & PadButton::Down.mask() != 0;
        let cross = pressed & PadButton::Cross.mask() != 0;
        let circle = pressed & PadButton::Circle.mask() != 0;
        if circle {
            self.phase = SpecialUsePhase::Done(SpecialUseOutcome::Cancelled);
            return;
        }
        // FUN_801D688C over 2 rows with wrap.
        if up || down {
            self.cursor ^= 1;
        }
        if cross {
            self.phase = if self.cursor == 0 {
                match self.route {
                    UseRoute::Incense => {
                        SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress)
                    }
                    _ => SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape),
                }
            } else {
                // "No" confirms back to the Use list.
                SpecialUsePhase::Done(SpecialUseOutcome::Cancelled)
            };
        }
    }

    /// The Door of Wind destination list (window 11, driven by the kind-4 list
    /// kernel rather than a Yes/No picker) - retail submenu `0xC`, phases
    /// 2..3 of `FUN_801D8B90`.
    ///
    /// Reached from the Use list: [`special_use_route_for_item`] routes bag id
    /// `0x89` here, `legaia_engine_core::field_menu_dispatch::build_pause_items_session`
    /// fills the rows from the disc placement table, and
    /// [`items_screen_model`] projects them through the shared list channel so
    /// both hosts draw the screen with the list renderer they already call.
    ///
    /// A pick warps: retail's phase 4 writes `_DAT_8007B43C = 5` and the
    /// outer menu SM acts on it; the port stages the destination on
    /// `legaia_engine_core::world::MenuState::pending_warp` and the world tick's
    /// menu-warp drain (`World::drain_staged_menu_warp`) resolves the staged
    /// scene word - a raw CDNAME TOC index - into the named scene
    /// transition the scene host consumes, seating the party at the
    /// record's tile. The bag decrement, the exit code and the staged
    /// triple are all committed by this screen.
    ///
    /// PORT: FUN_801D8B90 (Door of Wind destination list + exit-code 5 warp)
    fn pick_destination_input(&mut self, pressed: u16) {
        let cross = pressed & PadButton::Cross.mask() != 0;
        let circle = pressed & PadButton::Circle.mask() != 0;
        if circle {
            // Retail restores the saved Use-list scroll
            // (`DAT_801EF070/74`) on the way back.
            self.phase = SpecialUsePhase::Done(SpecialUseOutcome::Cancelled);
            return;
        }
        self.cursor = list_kernel_navigate(self.cursor, self.landmarks.len(), pressed);
        if cross && self.cursor < self.landmarks.len() {
            self.phase = SpecialUsePhase::Done(SpecialUseOutcome::Warp {
                landmark: self.cursor,
            });
        }
    }
}

/// One roster row of the window-14 target panel view model.
#[derive(Debug, Clone, Default)]
pub struct TargetPanelMemberModel {
    pub name: String,
    /// Record `+0x130`. The inner use-flow's target rows carry no level;
    /// hosts with party records overwrite this (0 draws as a blank-ish
    /// `0` otherwise).
    pub level: u8,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    /// Record-side base maxima (`+0x11C` / `+0x11E`) - the teal paren
    /// values of the mode-1 (Life / Magic Water) preview rows. Zero
    /// unless the builder had the character record.
    pub base_hp_max: u16,
    pub base_mp_max: u16,
    /// Effective stats in the retail panel's label order (ATK, UDF, LDF,
    /// SPD, INT) - the left value of the modes-2..5 stat rows.
    pub stat_eff: [u16; 5],
    /// Record-side base stats (`+0x124..+0x12C`), same order - the teal
    /// paren value of the same rows.
    pub stat_base: [u16; 5],
}

/// Owned view model of the window-14 party target panel - maps onto the
/// engine-ui `TargetPanelView` (renderer `FUN_801D0520`).
#[derive(Debug, Clone, Default)]
pub struct TargetPanelModel {
    pub members: Vec<TargetPanelMemberModel>,
    /// The preview word `DAT_801E46CC` value (0..=5, see
    /// [`target_panel_mode`]).
    pub mode: u32,
    pub cursor_row: u8,
    /// All-party pick (retail cursor bit `0x2000` - hand on every row).
    pub all_targets: bool,
}

/// Assemble the target-panel view model while the Items screen's use
/// flow is in target select. `mode` is the retail preview word for the
/// staged item ([`target_panel_mode`]; pass 0 without disc effect
/// tables - the plain `cur/max` panel).
pub fn target_panel_model(s: &PauseItemsSession, mode: u32) -> Option<TargetPanelModel> {
    let InventoryUseState::TargetSelect { cursor, .. } = &s.inner.state else {
        return None;
    };
    let members = s
        .inner
        .targets
        .iter()
        .map(|t| TargetPanelMemberModel {
            name: t.name.clone(),
            level: 0,
            hp: t.hp,
            hp_max: t.hp_max,
            mp: t.mp,
            mp_max: t.mp_max,
            ..Default::default()
        })
        .collect();
    Some(TargetPanelModel {
        members,
        mode,
        cursor_row: *cursor as u8,
        all_targets: false,
    })
}

/// The bag id the Items screen's use flow currently has staged - the row
/// the target select was entered from (`item_cursor` -> `filtered_items`
/// -> `items`). `None` outside target select.
pub fn staged_use_item_id(s: &PauseItemsSession) -> Option<u8> {
    let InventoryUseState::TargetSelect { item_cursor, .. } = &s.inner.state else {
        return None;
    };
    let idx = s.inner.filtered_items.get(*item_cursor).copied()?;
    s.inner.items.get(idx).copied()
}

/// One item id's display text, as every screen that shows an item needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDisplayText {
    /// Display name. Falls back to the curated catalog, then to a raw id,
    /// so a screen never shows nothing.
    pub name: String,
    /// Info-panel description. Empty when the disc text is unavailable.
    pub desc: String,
    /// An accessory's two passive lines (`(name, description)`).
    pub passive: Option<(String, String)>,
}

/// The stat aggregator's input record for one character: live base stats
/// off the record, accuracy and evasion from AGL, and the record's equip
/// bytes.
pub fn stat_record_from_character(
    c: &legaia_save::CharacterRecord,
) -> crate::battle_stats::StatRecord {
    let eq_bytes = c.equipment().slots;
    let live = c.live_stats();
    crate::battle_stats::StatRecord {
        base_attack: live.atk,
        base_udf: live.udf,
        base_ldf: live.ldf,
        // Accuracy / evasion derive from AGL (not equipment-fed).
        base_accuracy: live.agl,
        base_evasion: live.agl,
        base_spd: live.spd,
        base_int: live.int,
        equip: eq_bytes,
    }
}

/// Host entry point for the window-14 target panel: derive the retail
/// preview word for the staged item off the world's disc item-effect
/// table, build the view model, then fill the per-member record-side
/// fields (base maxima + base stats) the water previews draw from the
/// live party records.
///
/// This is the call the pause-menu host makes while the Items screen is
/// in target select; [`target_panel_mode`] and [`target_panel_model`]
/// are its two halves.
///
/// `item_effects` is the disc item-effect table (`World::tables.item_effects`)
/// and `members` the live roster (`World::party.roster.members`): the whole
/// slice of the world the panel reads. engine-core's
/// `pause_screens::target_panel_view_model` passes them off a `World`.
pub fn target_panel_view_model_of(
    s: &PauseItemsSession,
    item_effects: Option<&legaia_asset::item_effect::ItemEffectTable>,
    members: &[legaia_save::CharacterRecord],
) -> Option<TargetPanelModel> {
    let mode = staged_use_item_id(s)
        .and_then(|id| {
            let table = item_effects?;
            let eff = table.effect(id)?;
            Some(target_panel_mode(table.kind(id), eff.class, eff.tier))
        })
        .unwrap_or(0);
    let mut model = target_panel_model(s, mode)?;
    for (m, row) in model.members.iter_mut().zip(s.inner.targets.iter()) {
        // Party rows index the roster by slot; monster rows (battle-side
        // targets) have no character record and keep the zeroed fields.
        let Some(rec) = members.get(row.slot as usize) else {
            continue;
        };
        if row.is_enemy {
            continue;
        }
        let live = rec.live_stats();
        let base = rec.record_stats();
        m.level = match rec.magic_rank() {
            l @ 1..=99 => l,
            _ => legaia_save::level_for_cumulative_xp(rec.cumulative_xp()),
        };
        m.base_hp_max = base.hp_max;
        m.base_mp_max = base.mp_max;
        m.stat_eff = [live.atk, live.udf, live.ldf, live.spd, live.int];
        m.stat_base = [base.atk, base.udf, base.ldf, base.spd, base.int];
    }
    Some(model)
}

/// The staged notify-window message after its two markup operands are
/// patched, plus the window's two pens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyWindow {
    /// Operand byte written after the first `0xC1` markup token.
    pub c1_operand: u8,
    /// Operand byte written after the first `0xC5` markup token.
    pub c5_operand: u8,
    /// Message pen (ink `7`) at the window content origin.
    pub text_pen: (i16, i16),
    /// Hand-sprite pen at `(WX + 0xE6, WY + 0xD)`; kind and mode are both `1`.
    pub cursor_pen: (i16, i16),
}

/// Patch the two markup operands of the **notify window** (menu-overlay
/// window `8`, the panel an item-use result opens) and resolve its pens.
///
/// The message is not formatted at draw time: the window renderer takes the
/// template resident at `DAT_801E4700` in the menu overlay's own data
/// segment, finds the first `0xC1` and the first `0xC5` markup token in it
/// (`FUN_8003CBF8`, the same `0xC0`-class lead-byte scan the dialog
/// strcpy/strcat use) and overwrites **the byte following each token** in
/// place. So the template's operand slots are placeholders the renderer
/// refills every frame, not values baked when the message was staged.
///
/// The arithmetic is what the disassembly pins: the `0xC1` operand is the
/// low byte of `selector` (`_DAT_8007BB70`) and the `0xC5` operand is
/// `base + selector * 0x40` (`_DAT_8007BB78` plus the **halfword** at
/// `_DAT_8007BB70` scaled by `0x40`), both truncated to a byte by the `sb`.
///
/// What the two globals hold is pinned by their writers. The Items use
/// sub-screen seeds both to `0xFF` before the applier runs
/// (`0x801D850C` / `0x801D8510`) and opens this window only when `BB78`
/// changed (`0x801D8548..0x801D8564`, script `0x801E4C60` = `01 08`, open
/// window 8). The only writer that changes it on that path is the
/// applier's Hyper-Art-book arm, `jal 0x80035C00` at `0x8004208C` with
/// `a0 = class - 0xB` (the roster slot) and `a1` = the art id it inserted:
/// `FUN_80035C00` is two stores, `sh a0,0x858(gp)` / `sh a1,0x860(gp)`. So
/// `selector` is the learning character and `base` the art id, the `0xC1`
/// operand names the character and the `0xC5` operand `slot * 0x40 + art`
/// is exactly the arts-name token's `[character, art]` key. The window is
/// the "learned a new art" notice.
///
/// PORT: FUN_801dcd58 (menu-overlay notify-window content renderer)
/// REF: FUN_8003cbf8 (the markup-token scan whose offset the operand write
/// is relative to)
///
/// WIRED: [`patch_notify_template`] runs this over the disc template, and
/// `field_menu_dispatch::apply_inventory_outcome` composes the notice from
/// it whenever a pause-menu item use teaches an art; both play hosts park the
/// notice on `MenuRuntime` and paint window 8 while it is up.
pub fn notify_window_operands(window: (i16, i16), selector: i16, base: u8) -> NotifyWindow {
    let (wx, wy) = window;
    NotifyWindow {
        c1_operand: selector as u8,
        c5_operand: base.wrapping_add((selector.wrapping_mul(0x40)) as u8),
        text_pen: (wx, wy),
        cursor_pen: (wx + 0xE6, wy + 0xD),
    }
}

/// VA of the notify window's message template - a MES-markup string in
/// the menu overlay's data segment (PROT 0899), not a runtime buffer:
/// its only reference is `FUN_801DCD58`'s own `lui`/`addiu` pair
/// (`0x801DCD68` / `0x801DCD6C`).
pub const NOTIFY_TEMPLATE_VA: u32 = 0x801E_4700;

/// The notify-window template out of a PROT 0899 image: the bytes at
/// [`NOTIFY_TEMPLATE_VA`] up to the terminator, `0xC0..=0xCF` tokens kept
/// whole so a `0x00` operand does not end the string. `None` when the VA is
/// outside the image or the string is empty.
///
/// No text is committed: the VA is the coordinate and this reads the bytes
/// from the image the user supplied.
pub fn notify_template_from_menu_overlay(overlay: &[u8]) -> Option<Vec<u8>> {
    let off = NOTIFY_TEMPLATE_VA.checked_sub(MENU_OVERLAY_BASE_VA)? as usize;
    let rest = overlay.get(off..)?;
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(&b) = rest.get(i) {
        if b & 0xF0 == 0xC0 {
            out.extend_from_slice(rest.get(i..i + 2)?);
            i += 2;
            continue;
        }
        if b < 0x1F {
            break;
        }
        out.push(b);
        i += 1;
    }
    (!out.is_empty()).then_some(out)
}

/// Refill the template's two operand slots the way `FUN_801DCD58` does:
/// the byte after the first `0xC1` and the byte after the first `0xC5`
/// take [`notify_window_operands`]' two results.
pub fn patch_notify_template(template: &mut [u8], selector: i16, base: u8) {
    let ops = notify_window_operands((0, 0), selector, base);
    for (token, operand) in [(0xC1u8, ops.c1_operand), (0xC5u8, ops.c5_operand)] {
        let mut i = 0usize;
        while i + 1 < template.len() {
            let b = template[i];
            if b == token {
                template[i + 1] = operand;
                break;
            }
            i += if b & 0xF0 == 0xC0 { 2 } else { 1 };
        }
    }
}

/// Expand a patched notify template into display lines: `0xC1 x` splices
/// the party name `name(x)`, `0xC5 x` the arts name `art(x >> 6, x & 0x3F)`,
/// the colour escape `0xCF` and every other two-byte token draw nothing, and
/// `0x7C` breaks the line (`docs/formats/dialog-font.md`). A token the
/// resolver cannot answer splices nothing.
pub fn expand_notify_lines(
    patched: &[u8],
    name: impl Fn(u8) -> Option<String>,
    art: impl Fn(u8, u8) -> Option<String>,
) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut i = 0usize;
    while let Some(&b) = patched.get(i) {
        if b & 0xF0 == 0xC0 {
            let arg = patched.get(i + 1).copied().unwrap_or(0);
            let spliced = match b {
                0xC1 => name(arg),
                0xC5 => art(arg >> 6, arg & 0x3F),
                _ => None,
            };
            if let Some(text) = spliced {
                lines.last_mut().expect("never empty").push_str(&text);
            }
            i += 2;
            continue;
        }
        if b == 0x7C {
            lines.push(String::new());
        } else if (0x20..0x7F).contains(&b) {
            lines.last_mut().expect("never empty").push(b as char);
        }
        i += 1;
    }
    lines
}

/// The window-8 notification beat: a pause-menu item use taught `art_id` to
/// roster slot `character` (retail's `FUN_80035C00(slot, art)` pair), with
/// the disc template already patched and expanded. Held by
/// `legaia_engine_core::menu_runtime::MenuRuntime` until a confirm / cancel press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtLearnedNotice {
    /// Roster slot that learned the art (retail `_DAT_8007BB70`).
    pub character: u8,
    /// The art id inserted (retail `_DAT_8007BB78`).
    pub art_id: u8,
    /// The message, one entry per `0x7C`-separated line.
    pub lines: Vec<String>,
}

/// Number of rows the menu-overlay root command picker offers.
pub const ROOT_MENU_ROWS: u16 = 7;

/// The entry-context kind byte (`*_DAT_8007B450`) that both gates the root
/// menu's **Load** row and redirects its cancel into the Yes/No confirm.
pub const ROOT_MENU_CONTEXT_LOCKED: u8 = 0x0D;

/// The per-scene save-allow flag `_DAT_8007B6A8` gating the **Save** row.
///
/// Scene load seeds it from the MAN header's `[0x01] & 1`
/// ([`legaia_asset::man_section::ManHeader::low_flag`]); a cleared flag is
/// what makes a scene a no-save scene. It is the same byte the
/// "Save Anywhere" cheat forces - see
/// [`docs/reference/memory-map.md`](../../../docs/reference/memory-map.md).
pub const ROOT_MENU_SAVE_ALLOW_FLAG: u32 = 0x8007_B6A8;

/// Sub-screen each root-menu row hands off to, in the retail draw order
/// **Items / Magic / Equip / Status / Options / Load / Save**. Rows `5`
/// (Load, `0x18`) and `6` (Save, `0x19`) are the two conditional ones -
/// see [`root_menu_confirm_route`].
///
/// The row labels are read off the menu overlay's own string pool - the
/// seven pointers `FUN_801CFD68` hands the string primitive are `@Items`,
/// `@Magic`, `@Equip`, `@Status`, `@Options`, `@Load`, `@Save` at
/// `0x801CE9D0`, `..9D8`, `..9E0`, `..9E8`, `..9F4`, `0x801CEA00`,
/// `..EA08` - so `0x18` is the **load** card driver and `0x19` the save
/// one, which is the direction retail's own op selector confirms
/// (`0x18` -> `FUN_801DD35C(1, 2)` skips the card-file erase, `0x19` ->
/// `(1, 1)` performs it).
///
/// REF: FUN_801dd35c (the card-driver body whose op selector fixes the
/// direction of the two gated rows)
pub const ROOT_MENU_ROUTES: [u8; ROOT_MENU_ROWS as usize] =
    [0x05, 0x0E, 0x12, 0x15, 0x17, 0x18, 0x19];

/// What confirming a root-menu row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootMenuRoute {
    /// Hand off to this sub-screen id (`DAT_801E46A4`).
    Sub(u8),
    /// Row is unavailable - retail plays the reject cue `0x23` and stays.
    Buzz,
    /// Row index outside `0..7`: nothing happens.
    None,
}

/// Confirm routing for the menu-overlay **root command picker**
/// (sub-screen `0x01`).
///
/// The picker runs `FUN_801D688C(&DAT_801E46BC, 7, 1)` - seven rows - and
/// routes the confirmed row through [`ROOT_MENU_ROUTES`]. Two rows are
/// conditional and buzz instead of advancing:
///
/// * **Load** (row `5`) is blocked when an entry context is installed at
///   `_DAT_8007B450` **and** its kind byte is
///   [`ROOT_MENU_CONTEXT_LOCKED`]. A null context pointer allows the row -
///   the test is on the kind, not on the pointer's presence. The same
///   context makes cancel ask first ([`root_menu_cancel_route`]), which is
///   coherent: a parked field script must not be replaced by a loaded game
///   nor abandoned without a confirm.
/// * **Save** (row `6`) is blocked when the per-scene save-allow byte
///   [`ROOT_MENU_SAVE_ALLOW_FLAG`] is zero.
///
/// Both gates are re-read by the list renderer `FUN_801CFD68`, which greys
/// the same two rows to ink `0` from the same two globals - so the confirm
/// arm never buzzes a row that drew white.
///
/// Every accepted row first clears the shared list globals
/// `_DAT_8007BB98` / `_DAT_8007BB90` / `_DAT_8007BB88`, and the Magic row
/// additionally stages `DAT_801E46C8 = DAT_801E46C4 & 0xFFF`; both are host
/// state the caller mirrors.
///
/// PORT: FUN_801d6b20 (menu-overlay sub-screen `0x01`, phase-1 confirm arm
/// `0x801D6BCC..0x801D6CF4`)
/// REF: FUN_801d688c (the cursor navigator this screen drives; ported as
/// `crate::menu_input`)
/// REF: FUN_801cfd68 (the row renderer whose grey arms read the same two
/// globals in the same order)
///
/// Live on the pause-menu path. [`crate::field_menu::FieldMenuSession`] calls
/// this once per row to ink the list ([`crate::field_menu::FieldMenuSession::row_is_available`],
/// which the renderer greys on) and once more on Cross to decide advance vs
/// buzz - the same double read, in the same order, that keeps retail's row
/// renderer and confirm arm agreeing. The `Sub(id)` payload is consumed rather
/// than dropped: the session resolves the confirmed row back **through** the
/// id ([`crate::field_menu::FieldMenuRow::from_retail_subscreen`]), so
/// [`ROOT_MENU_ROUTES`] decides which sub-session the shell pushes.
///
/// Both gate inputs come from the world at menu-open
/// (`BootSession::open_field_menu`): `save_allowed` from
/// `legaia_engine_core::world::PartyState::scene_save_allowed`, which scene load seeds from
/// [`legaia_asset::man_section::ManHeader::low_flag`], and
/// `entry_context_kind` from
/// `legaia_engine_core::world::World::menu_entry_context_kind`. The save gate is the one
/// that bites on real data - the MAN bit is set on the three kingdom world
/// maps and clear on every field scene, so Save greys everywhere but the
/// overworld. The Load gate is plumbed and live but cannot yet reach its
/// blocking value: the port tags each op-`0x49` park with its owning context
/// instead of keeping retail's single pointer, and no path records the armed
/// sub-op, so the kind resolves to `0`, `5` or `None` - all allow branches.
pub fn root_menu_confirm_route(
    row: u16,
    entry_context_kind: Option<u8>,
    save_allowed: bool,
) -> RootMenuRoute {
    match row {
        5 => {
            if entry_context_kind == Some(ROOT_MENU_CONTEXT_LOCKED) {
                RootMenuRoute::Buzz
            } else {
                RootMenuRoute::Sub(ROOT_MENU_ROUTES[5])
            }
        }
        6 => {
            if save_allowed {
                RootMenuRoute::Sub(ROOT_MENU_ROUTES[6])
            } else {
                RootMenuRoute::Buzz
            }
        }
        r if r < ROOT_MENU_ROWS => RootMenuRoute::Sub(ROOT_MENU_ROUTES[r as usize]),
        _ => RootMenuRoute::None,
    }
}

/// Sub-screen a cancel out of the root command picker lands on: `0` (the
/// terminal exit screen) normally, and `3` - the Yes/No confirm - when the
/// installed entry context's kind byte is [`ROOT_MENU_CONTEXT_LOCKED`]. So
/// the same context that hides the Load row is the one that makes leaving
/// the menu ask first.
///
/// PORT: FUN_801d6b20 (cancel arm `0x801D6CF8..0x801D6D18`)
///
/// WIRED: [`crate::field_menu::FieldMenuSession`]'s Browsing cancel arm
/// calls this with the gate's entry-context kind and, on the locked route
/// (`3`, [`CONTEXT_LOCKED_CANCEL_SUBSCREEN`]), opens its `ReadyConfirm`
/// phase - the Yes/No leave confirm this note used to name as the missing
/// prerequisite - instead of closing. The plain route (`0`) closes the
/// menu, retail's terminal exit screen.
pub fn root_menu_cancel_route(entry_context_kind: Option<u8>) -> u8 {
    if entry_context_kind == Some(ROOT_MENU_CONTEXT_LOCKED) {
        3
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// The kind-`0x0D` entry screens: sub-screens 4 and 3
// ---------------------------------------------------------------------------

/// Sub-screen the save/menu driver **opens on** for entry-context kind
/// [`ROOT_MENU_CONTEXT_LOCKED`] - the notice panel that draws window 6.
///
/// `FUN_801DC6B4`'s entry decode writes the sub-screen id four ways, one per
/// kind, and `4` is exactly one of them:
///
/// ```text
/// 801dc8d0  lbu  v1,0x0(a0)          ; the kind byte
/// 801dc8d4  li   v0,0xd
/// 801dc8d8  bne  v1,v0,0x801dc8ec
/// 801dc8e0  li   v0,0x4
/// 801dc8e4  sw   v0,0x46a4(a1)       ; DAT_801E46A4 = 4
/// ```
///
/// Nothing else in the overlay writes `4` there, and nothing else writes
/// `0x20` (the prize exchange) either - a sweep of every
/// `sw rt,0x46a4(rs)` in PROT 0899 finds 66 writers and exactly one for
/// each of those two ids, both inside this decode. So these screens hang
/// off the entry-context kind and off nothing else.
///
/// PORT: FUN_801DC6B4 (`0x801dc8d0..0x801dc8e4`)
///
/// Live: [`menu_entry_subscreen`] answers this id for kind `0x0D`, and
/// [`crate::field_menu::FieldMenuSession::open_entry_screen`] - which both
/// play hosts call at menu-open - opens the notice panel exactly when the
/// decode lands here.
pub const CONTEXT_LOCKED_ENTRY_SUBSCREEN: u8 = 4;

/// Sub-screen the menu driver opens on when no entry context is installed:
/// the root command picker (`sw s1,0x46a4(a1)` with `s1 = 1` at `0x801DC86C`).
pub const ROOT_PICKER_SUBSCREEN: u8 = 1;
/// Entry-context kind `0` (an inline shop, op-`0x49` sub-op `0`) opens `0x1A`.
pub const CONTEXT_SHOP_ENTRY_SUBSCREEN: u8 = 0x1A;
/// Entry-context kind `1` (a field save point) opens `0x19`, the save card
/// driver - the same id the root picker's Save row routes to.
pub const CONTEXT_SAVE_ENTRY_SUBSCREEN: u8 = 0x19;
/// Entry-context kind `7` opens `0x20`, the casino prize exchange.
pub const CONTEXT_PRIZE_ENTRY_SUBSCREEN: u8 = 0x20;

/// The menu driver's **entry decode**: which sub-screen a menu opens on,
/// from the entry-context kind byte `*_DAT_8007B450`.
///
/// `FUN_801DC6B4` state 0 (`0x801DC85C..0x801DC8E4`) stores the root picker
/// (`1`) first, then overwrites it once per matching kind with four
/// independent `lbu (a0)` compares - `0` -> `0x1A`, `1` -> `0x19`, `7` ->
/// `0x20`, `0x0D` -> `4` - so any other kind, and a null context pointer,
/// keeps the picker.
///
/// One arm is outside this function's domain: a context pointer equal to
/// the literal `1` (not a record) opens sub-screen `2` and clears the
/// pointer (`0x801DC868..0x801DC878`). The port's context is a kind byte, not
/// a pointer, and nothing in it produces that sentinel.
///
/// Only the `0x0D` arm has a consumer that routes on the decoded id -
/// [`crate::field_menu::FieldMenuSession::open_entry_screen`]. The shop, save
/// point and prize exchange reach their screens through dedicated host paths
/// (the shop session, the save flow, the prize-exchange session) that open
/// them directly rather than through the pause menu.
///
/// PORT: FUN_801DC6B4 (`0x801DC85C..0x801DC8E4`, the entry decode)
pub fn menu_entry_subscreen(entry_context_kind: Option<u8>) -> u8 {
    match entry_context_kind {
        Some(0) => CONTEXT_SHOP_ENTRY_SUBSCREEN,
        Some(1) => CONTEXT_SAVE_ENTRY_SUBSCREEN,
        Some(7) => CONTEXT_PRIZE_ENTRY_SUBSCREEN,
        Some(ROOT_MENU_CONTEXT_LOCKED) => CONTEXT_LOCKED_ENTRY_SUBSCREEN,
        _ => ROOT_PICKER_SUBSCREEN,
    }
}

/// Sub-screen the root picker's **cancel** hands to under the same kind -
/// the ready check that draws window 5. See [`root_menu_cancel_route`].
pub const CONTEXT_LOCKED_CANCEL_SUBSCREEN: u8 = 3;

/// Load base of the menu overlay's string pool - the image
/// [`menu_overlay_string`] slices.
pub const MENU_OVERLAY_BASE_VA: u32 = legaia_asset::menu_windows::MENU_OVERLAY_BASE_VA;

/// The six label VAs `FUN_801D6360` loads into the string primitive, in
/// draw order (`lui a0,0x801d` + `addiu a0,a0,-0x1358` and its five
/// siblings at `0x801d636c..0x801d6448`).
///
/// Coordinates only - the text is read from the caller's own image, the
/// same rule `legaia_asset::battle_ui_strings` follows. The sixth entry is
/// a one-byte control string rather than a line, which is why the panel
/// reads as five lines plus the advance hand.
pub const NOTICE_PANEL_LABEL_VAS: [u32; 6] = [
    0x801C_ECA8,
    0x801C_ECD4,
    0x801C_ECFC,
    0x801C_ED20,
    0x801C_ED38,
    0x801C_ED58,
];

/// The two heading VAs `FUN_801D61B0` loads above its choice group.
pub const READY_CONFIRM_HEADING_VAS: [u32; 2] = [0x801C_EC78, 0x801C_EC94];

/// The one heading VA `FUN_801D603C` loads above its choice group (window
/// 46, the prize-exchange redeem confirm).
pub const CHOICE_PANEL_HEADING_VA: u32 = 0x801C_EAC8;

/// The shared Yes / No choice labels both choice painters load.
pub const CHOICE_YES_VA: u32 = 0x801C_EA84;
/// See [`CHOICE_YES_VA`].
pub const CHOICE_NO_VA: u32 = 0x801C_EA8C;

/// Read one NUL-terminated menu-overlay string at `va` out of a PROT 0899
/// image, dropping the leading `@` the string primitive uses as its
/// lead-in marker.
///
/// Stops at the first byte outside printable ASCII as well as at the NUL,
/// so an entry that is really a one-byte control code comes back empty
/// rather than as mojibake. `None` means the VA is outside the image.
///
/// No text is committed anywhere in this crate: the VAs above are the
/// coordinates and this reads the bytes from the image the user supplied.
pub fn menu_overlay_string(overlay: &[u8], va: u32) -> Option<String> {
    let off = va.checked_sub(MENU_OVERLAY_BASE_VA)? as usize;
    let rest = overlay.get(off..)?;
    let body = rest.strip_prefix(b"@").unwrap_or(rest);
    let end = body
        .iter()
        .position(|&b| !(0x20..0x7F).contains(&b))
        .unwrap_or(body.len());
    Some(String::from_utf8_lossy(&body[..end]).into_owned())
}

/// Every label the kind-`0x0D` pair needs, read off one PROT 0899 image.
///
/// A host installs this on its session at menu-open; a session without it
/// draws the panels with no text rather than with invented text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextLockedLabels {
    /// The notice panel's lines, empty entries dropped (window 6).
    pub notice_lines: Vec<String>,
    /// The ready check's two heading lines (window 5).
    pub ready_headings: [String; 2],
    /// Yes / No, shared by both choice painters.
    pub choices: [String; 2],
}

impl ContextLockedLabels {
    /// Read every label out of a PROT 0899 image.
    pub fn from_menu_overlay(overlay: &[u8]) -> Self {
        let s = |va: u32| menu_overlay_string(overlay, va).unwrap_or_default();
        Self {
            notice_lines: NOTICE_PANEL_LABEL_VAS
                .iter()
                .map(|&va| s(va))
                .filter(|l| !l.is_empty())
                .collect(),
            ready_headings: [
                s(READY_CONFIRM_HEADING_VAS[0]),
                s(READY_CONFIRM_HEADING_VAS[1]),
            ],
            choices: [s(CHOICE_YES_VA), s(CHOICE_NO_VA)],
        }
    }

    /// `true` once a host has installed real disc text.
    pub fn is_installed(&self) -> bool {
        !self.notice_lines.is_empty() || !self.ready_headings[0].is_empty()
    }
}

/// Phase tag of the Equip screen, mirroring
/// [`crate::equip_session::EquipState`] as a flat word the hosts map onto
/// `engine-ui`'s `EquipDrawPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipScreenPhase {
    SlotPicker,
    ItemPicker,
    Confirm,
}

/// Owned view model of the Equip screen - the sibling of
/// [`items_screen_model`] / [`magic_screen_model`] for the third
/// descriptor-window screen.
///
/// It exists for the same reason those do: the projection is real work
/// (eight slot labels, the candidate list for the active slot with its bag
/// counts, and a full `compute_battle_stats` pass with the hovered item
/// installed), and it was written out twice - once in the native window's
/// `equip_session_draws`, once in the browser's. Two copies of a stat
/// preview is two chances to preview a different number.
pub struct EquipScreenModel {
    /// Party-window rows.
    pub party_names: Vec<String>,
    /// Slot labels in engine slot order (retail identifies slots by the
    /// pictogram column; the label is an engine hint).
    pub slot_labels: Vec<String>,
    /// Per-slot equipped-item display names; empty string for an empty slot.
    pub slot_items: Vec<String>,
    /// Candidate item names for the active slot. Empty in `SlotPicker`.
    pub candidate_names: Vec<String>,
    /// Bag count per candidate, parallel to [`Self::candidate_names`].
    pub candidate_counts: Vec<u8>,
    /// The three retail compare rows (`FUN_801D21C0`'s stat block) as
    /// `(label, current, preview)`: the live menu block against the block
    /// with the Best Equipment picks installed. Retail draws them only while
    /// the slot browse's hand is on the Best Equipment row (sub-screen `0x13`,
    /// row 0); empty everywhere else - the candidate step's compare is
    /// window 25 ([`Self::compare`]), not this block.
    pub stat_compare: Vec<(&'static str, u16, u16)>,
    /// The Best Equipment row's per-armament change list: `(armament row
    /// 0..=3, candidate name)` for each armament whose pick
    /// (`DAT_801EF0C0[i]`) differs from what the slot holds. Retail draws the
    /// change arrow, the armament pictogram and the name on that armament's
    /// slot row. Same gate as [`Self::stat_compare`].
    pub best_changes: Vec<(u8, String)>,
    pub phase: EquipScreenPhase,
    /// Cursor row inside the active phase column.
    pub cursor: u16,
    /// Active slot index in `ItemPicker` / `Confirm`.
    pub active_slot: u8,
    /// Pending-swap label above the Yes/No prompt.
    pub confirm_label: Option<String>,
    /// Roster slot of the character being equipped.
    pub char_slot: u8,
    /// The party-window row that character sits on (its place in the
    /// present party, which is what [`Self::party_names`] lists) - the row
    /// the hand cursor marks.
    pub party_row: u8,
    /// Slot-picker cursor row, or `None` past the slot picker - what the
    /// sprite pass puts the second hand on.
    pub slot_cursor: Option<u16>,
    /// Pictogram rows the sprite pass draws - retail's seven browse rows.
    pub pictogram_rows: usize,
    /// Window 24's item-info panel content for the hovered candidate, or
    /// `None` outside the candidate step.
    ///
    /// Retail's Equip screen opens **five** windows, not four: sub-screen
    /// `0x12` picks the character and `0x13` browses the slot rows over the
    /// capture-pinned set `2 / 21 / 22 / 23`, and `0x14` - the candidate
    /// list - adds windows `24` and `25` on top through open script
    /// `0x801E4DC8` (`docs/subsystems/field-menu.md`). Window 24's renderer
    /// `FUN_801DCC20` calls the **shared item-info panel** `FUN_801D0F1C`,
    /// the same one window 17 draws on the Items screen - which is why the
    /// two descriptors carry byte-identical rects `(14, 108, 144, 40)`.
    ///
    /// That makes this panel an *addition* to the port's screen rather than
    /// a different layout for it - the reading that had window 24 waived as
    /// needing "the whole screen moved onto the descriptor-table layout".
    pub info: Option<EquipItemInfoModel>,
    /// Window 25's stat-compare panel, the candidate step's **other**
    /// addition (open script `0x801E4DC8` names 24 and 25 together).
    /// `None` outside the candidate step, or when the host supplied no
    /// [`EquipCompareCtx`].
    pub compare: Option<EquipCompareModel>,
}

/// Window 24's item-info content - the hovered candidate's own row of the
/// shared panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipItemInfoModel {
    pub name: String,
    /// Bag count, echoed beside the name exactly as window 17 echoes it.
    pub count: u16,
    pub desc: String,
    /// An accessory's two passive lines.
    pub passive: Option<(String, String)>,
}

/// Window 25's model - the Equip screen's own stat-compare panel.
///
/// The fields are the retail panel's inputs, one per global the renderer
/// reads: the two eight-word blocks, the record's HP / MP maxima (the HP and
/// MP rows print the record halfword, not the block word) and the four
/// values the compare **category** is resolved from. `engine-ui` holds the
/// resolver and the painter, so this type carries no row set - only what the
/// resolver asks for.
///
/// REF: FUN_801d1290 - the renderer this feeds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipCompareModel {
    /// Display name drawn at the content origin (record `+0x2A7`).
    pub name: String,
    /// The live block (`0x801EF080`).
    pub current: [i32; 8],
    /// The trial-equip block (`0x801EF0A0`) - the same eight words with the
    /// hovered candidate installed in the active slot.
    pub candidate: [i32; 8],
    /// HP maximum off the record (`+0x104`).
    pub hp_max: u16,
    /// MP maximum off the record (`+0x108`).
    pub mp_max: u16,
    /// Browse row past the screen's row 0, retail's
    /// `(DAT_801E46C0 & 0xFFF) - 1`.
    pub slot_row: i32,
    /// Staged (hovered) item id, retail's `DAT_801E46B0`; `-1` = nothing
    /// staged.
    pub staged_id: i32,
    /// Category byte resolved for [`Self::staged_id`].
    pub staged_category: u8,
    /// Id already sitting in the browsed slot's equip byte; `0` = empty.
    pub equipped_id: u8,
    /// Category byte resolved for [`Self::equipped_id`].
    pub equipped_category: u8,
}

/// The record + disc tables [`equip_screen_model`] needs before it can
/// publish an [`EquipCompareModel`].
///
/// A host that has none of it passes `None` and the screen keeps its
/// window-22 compare block alone - the panel is an addition to the screen,
/// not a replacement for any part of it.
#[derive(Clone, Copy)]
pub struct EquipCompareCtx<'a> {
    /// The character record behind the session. Supplies the HP / MP maxima,
    /// which [`crate::battle_stats::StatRecord`] does not carry.
    pub record: &'a legaia_save::CharacterRecord,
    /// Disc equipment stat-bonus table view, for the class-`1` arm of the
    /// category lookup (`0x80074F68 + row*8 + 5`).
    pub equip_info: Option<&'a crate::equipment::DiscEquipInfo>,
    /// Disc item-effect table, for the item record's class byte and the
    /// non-equipment arm of the category lookup (`0x800752C0 + row*4 + 3`).
    pub item_effects: Option<&'a legaia_asset::item_effect::ItemEffectTable>,
}

/// Equip bytes the **menu** stat aggregator walks.
///
/// `FUN_801CF650`'s loop counter is bounded by `slti a2, 5` at `0x801CF744`,
/// so the block behind windows 22 / 25 / 41 sums the first five equip bytes
/// only - not all eight. The battle-side aggregator `FUN_80042558`, which
/// [`crate::battle_stats::compute_battle_stats`] implements, walks the whole
/// array; that is why [`menu_stat_block`] zeroes the tail rather than
/// handing the shared aggregator the record as it stands.
pub const MENU_BLOCK_EQUIP_SLOTS: usize = 5;

/// Category byte used when no lookup resolves one - `FUN_801D1290`'s
/// pre-loaded `li a1, 0x40` and the no-passive sentinel of the equipment
/// bonus rows.
pub const COMPARE_CATEGORY_DEFAULT: u8 = 0x40;

/// Resolve one item id's compare-category byte, retail's
/// `0x801D1388..0x801D13F8` lookup chain.
///
/// The item property record's class byte (`0x80074368 + id*0xC + 0`) picks
/// the table: class `1` reads the equipment bonus row's `+5`, anything else
/// reads the item-effect descriptor's `+3`. Both arms index by the **same**
/// `+1` byte of the item record, which is what
/// [`legaia_asset::item_effect::ItemEffectTable::subtype`] returns.
///
/// REF: FUN_801d1290 - the renderer this lookup belongs to; the guards that
/// decide *whether* it is consulted are `engine-ui`'s
/// `active_compare_category`.
pub fn compare_category_for_item(id: u8, ctx: &EquipCompareCtx<'_>) -> u8 {
    let Some(effects) = ctx.item_effects else {
        return COMPARE_CATEGORY_DEFAULT;
    };
    let row = effects.subtype(id);
    if effects.kind(id) == 1 {
        match ctx.equip_info {
            Some(info) => info.row_passive_index(row),
            None => COMPARE_CATEGORY_DEFAULT,
        }
    } else {
        effects
            .descriptor(row)
            .map(|e| e.marker)
            .unwrap_or(COMPARE_CATEGORY_DEFAULT)
    }
}

/// Build the eight-word menu stat block for one record + equipment set.
///
/// Word order is the retail block's: HP max, MP max, AGL, ATK, UDF, LDF,
/// SPD, INT. Words 0 / 1 / 2 come straight off the record (`FUN_801CF5D0`
/// seeds all eight; only these three are never added to); words 3..=7 are
/// the seed plus the equipment bonuses of the first
/// [`MENU_BLOCK_EQUIP_SLOTS`] slots, which is what `FUN_801CF650` sums.
///
/// REF: FUN_801cf5d0 - the seeder.
/// REF: FUN_801cf650 - the equipment-bonus summer.
pub fn menu_stat_block(
    record: &crate::battle_stats::StatRecord,
    table: &crate::battle_stats::EquipmentTable,
    hp_max: u16,
    mp_max: u16,
) -> [i32; 8] {
    let mut walked = *record;
    for slot in MENU_BLOCK_EQUIP_SLOTS..walked.equip.len() {
        walked.equip[slot] = 0;
    }
    let neutral = crate::battle_stats::StatusModifiers::default();
    let s = crate::battle_stats::compute_battle_stats(&walked, table, &[], &neutral);
    [
        i32::from(hp_max),
        i32::from(mp_max),
        // AGL is the record's `+0x110` word; equipment never feeds it, so the
        // seeded value stands. `StatRecord` carries it as the accuracy line.
        i32::from(record.base_accuracy),
        i32::from(s.atk),
        i32::from(s.udf),
        i32::from(s.ldf),
        i32::from(s.spd),
        i32::from(s.int),
    ]
}

/// Project a live [`crate::equip_session::EquipSession`] into
/// [`EquipScreenModel`].
///
/// `party_names` is the world's roster snapshot, which the session does not
/// carry. The stat preview uses the neutral status set: this is the field
/// menu, and the session recomputes with live status modifiers on commit.
///
/// `text` resolves an item id's display name / description / passive lines -
/// `legaia_engine_core::field_menu_dispatch::item_display_text` against the live world.
/// Passing `None` leaves every item spelled as its raw id, which is what a
/// disc-free test wants and what the screen showed on both hosts for as long
/// as the resolver lived inside the Items screen's own session builder.
/// `compare` is the record + disc tables window 25's panel needs; passing
/// `None` leaves [`EquipScreenModel::compare`] empty and the screen draws
/// exactly what it drew before the panel existed.
pub fn equip_screen_model(
    session: &crate::equip_session::EquipSession,
    char_slot: u8,
    party_names: &[String],
    present: &[u8],
    text: Option<&dyn Fn(u8) -> ItemDisplayText>,
    compare: Option<EquipCompareCtx<'_>>,
) -> EquipScreenModel {
    use crate::equip_session::EquipState;
    use crate::equipment::EquipSlot;

    let record = session.record();
    let name_of = |id: u8| match text {
        Some(f) => f(id).name,
        None => format!("Item {id:02X}"),
    };
    // Rows in retail's browse order (weapon, helmet, body, footwear, Goods
    // x3); the engine's Hand Guard slot (the Ra-Seru byte) has no row.
    let order = crate::equip_session::BROWSE_SLOT_ORDER;
    let slot_labels: Vec<String> = order
        .iter()
        .map(|&i| {
            EquipSlot::from_index(i)
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| format!("Slot {i}"))
        })
        .collect();
    let slot_items: Vec<String> = order
        .iter()
        .map(|&i| match record.equip.get(usize::from(i)).copied() {
            None | Some(0) => String::new(),
            Some(id) => name_of(id),
        })
        .collect();

    let (phase, cursor, active_slot, confirm_label) = match session.state() {
        EquipState::SlotPicker { cursor } => {
            (EquipScreenPhase::SlotPicker, cursor as u16, cursor, None)
        }
        EquipState::ItemPicker { slot, cursor } => {
            (EquipScreenPhase::ItemPicker, cursor, slot, None)
        }
        EquipState::Confirm {
            slot,
            item_id,
            cursor,
        } => (
            EquipScreenPhase::Confirm,
            cursor as u16,
            slot,
            Some(format!("Equip {}?", name_of(item_id))),
        ),
        EquipState::Done(_) => (EquipScreenPhase::SlotPicker, 0, 0, None),
    };

    // Candidates + stat compare only matter past the slot picker.
    let (candidate_names, candidate_counts, considered_id): (Vec<String>, Vec<u8>, Option<u8>) =
        if phase == EquipScreenPhase::SlotPicker {
            (Vec::new(), Vec::new(), None)
        } else {
            let items = session.items_for_slot(active_slot);
            let names: Vec<String> = items.iter().map(|it| name_of(it.id)).collect();
            let counts: Vec<u8> = items
                .iter()
                .map(|it| session.inventory().get(&it.id).copied().unwrap_or(0))
                .collect();
            // The item the compare block previews: the hovered row in the
            // picker, the pending item in the confirm phase.
            let considered = match session.state() {
                EquipState::Confirm { item_id, .. } => Some(item_id),
                _ => items.get(cursor as usize).map(|it| it.id),
            };
            (names, counts, considered)
        };

    // Window 22's second pass (`FUN_801D21C0`, `0x801D23BC..0x801D27F8`):
    // gated on the settled slot-browse sub-screen (`DAT_801E46A4 ==
    // DAT_801E46A8 == 0x13`) and on browse row 0. It walks the four
    // armaments against the Best Equipment picks and then prints the
    // ATK / UDF / LDF words of the live block (`0x801EF08C..94`) with the
    // picks' block (`0x801EF0AC..B4`) beside any that differ.
    let on_best_row = matches!(
        session.state(),
        EquipState::SlotPicker { cursor } if cursor == crate::equip_session::SLOT_BROWSE_BEST_ROW
    ) && !session.slot_cursor_hidden();
    let (stat_compare, best_changes) = if on_best_row {
        let picks = session.best_equipment_now();
        let mut trial = *record;
        let mut changes = Vec::new();
        for (row, (&slot, &pick)) in crate::equip_session::ARMAMENT_ENGINE_SLOTS
            .iter()
            .zip(picks.iter())
            .enumerate()
        {
            if record.equip.get(slot).copied() != Some(pick) {
                changes.push((row as u8, name_of(pick)));
                trial.equip[slot] = pick;
            }
        }
        let cur = menu_stat_block(record, session.equipment(), 0, 0);
        let new = menu_stat_block(&trial, session.equipment(), 0, 0);
        let w = |b: &[i32; 8], i: usize| b[i].clamp(0, i32::from(u16::MAX)) as u16;
        (
            vec![
                ("ATK", w(&cur, 3), w(&new, 3)),
                ("UDF", w(&cur, 4), w(&new, 4)),
                ("LDF", w(&cur, 5), w(&new, 5)),
            ],
            changes,
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // Window 24's panel: the hovered candidate's own info row, resolved
    // through the same text tables the Items screen's window 17 uses. Retail
    // gates the panel on the staged id, so the Remove row (`id == 0`) leaves
    // it empty rather than describing nothing.
    let info = considered_id.filter(|&id| id != 0).map(|id| {
        let t = text.map(|f| f(id)).unwrap_or_default();
        EquipItemInfoModel {
            name: if t.name.is_empty() {
                format!("Item {id:02X}")
            } else {
                t.name
            },
            count: u16::from(session.inventory().get(&id).copied().unwrap_or(0)),
            desc: t.desc,
            passive: t.passive,
        }
    });

    // Window 25's panel. Retail's renderer bails outright when nothing is
    // staged (`beq v0, zero` on `DAT_801E46B0` at `0x801D12BC`), so the
    // panel appears with the candidate list and goes with it - the same
    // life-cycle as window 24 beside it.
    let compare = compare.as_ref().and_then(|ctx| {
        let staged = considered_id?;
        let hp_mp = ctx.record.hp_mp_sp();
        let equipped_id = record.equip.get(active_slot as usize).copied().unwrap_or(0);
        let mut trial = *record;
        if let Some(slot) = trial.equip.get_mut(active_slot as usize) {
            *slot = staged;
        }
        Some(EquipCompareModel {
            name: party_names
                .get(char_slot as usize)
                .cloned()
                .unwrap_or_default(),
            current: menu_stat_block(record, session.equipment(), hp_mp.hp_max, hp_mp.mp_max),
            candidate: menu_stat_block(&trial, session.equipment(), hp_mp.hp_max, hp_mp.mp_max),
            hp_max: hp_mp.hp_max,
            mp_max: hp_mp.mp_max,
            // Retail's browse ROW, not the engine's `EquipSlot` index. The
            // two part company at index 3: retail's rows resolve through the
            // two-table map the armament writer uses - row 0 takes the
            // per-character weapon halfword off `0x8007B42C` (`2, 3, 2`,
            // indexed by the roster slot), rows 1 and up take `0x801E43E8` =
            // `00 01 00 04 05 06 07` - so the browse order is weapon,
            // helmet, body, footwear, Goods x3, while `EquipSlot` inserts a
            // `HandGuard` the disc has no byte for. Feeding the slot index
            // straight through made footwear row `4`, and the `slti v0, s0,
            // 4` guard at `0x801D137C` then resolved a compare category
            // there - a retail capture of that row shows the ATK / UDF / LDF
            // triple, the `CATEGORY_DEFAULT` fallback, on every candidate.
            // The engine-only Hand Guard row has no retail row at all, so it
            // reports `-1`: below the guard, and not a row number retail
            // would ever pass.
            slot_row: crate::equip_session::retail_slot_row_for_engine_slot(active_slot)
                .unwrap_or(-1),
            staged_id: i32::from(staged),
            staged_category: compare_category_for_item(staged, ctx),
            equipped_id,
            equipped_category: compare_category_for_item(equipped_id, ctx),
        })
    });

    EquipScreenModel {
        info,
        compare,
        // The party window lists the present party (retail's
        // `DAT_80084594`-long member list), not every roster record; an empty
        // `present` keeps the roster order.
        party_names: if present.is_empty() {
            party_names.to_vec()
        } else {
            present
                .iter()
                .filter_map(|&s| party_names.get(usize::from(s)).cloned())
                .collect()
        },
        party_row: present
            .iter()
            .position(|&s| s == char_slot)
            .map_or(char_slot, |r| r as u8),
        slot_labels,
        slot_items,
        candidate_names,
        candidate_counts,
        stat_compare,
        best_changes,
        phase,
        cursor,
        // The row the active slot draws on (its place in the browse order).
        active_slot: crate::equip_session::browse_row_for_slot(active_slot) - 1,
        confirm_label,
        char_slot,
        slot_cursor: match session.state() {
            EquipState::SlotPicker { .. } if session.slot_cursor_hidden() => None,
            EquipState::SlotPicker { cursor } => Some(cursor as u16),
            _ => None,
        },
        pictogram_rows: crate::equip_session::BROWSE_SLOT_ROWS,
    }
}

#[cfg(test)]
mod tests;
