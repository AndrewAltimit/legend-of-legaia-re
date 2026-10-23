//! The Items screen session: command-window / list focus model over the item-use flow.

use super::*;

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
    /// This slot's [`crate::menu_list_rows::ROW_DISABLED`] /
    /// [`crate::menu_list_rows::ROW_ALT_INK`] bits in the **Use** list build
    /// (content id 3, `crate::menu_list_rows::build_use_list_rows`). `0`
    /// (white) when the host had no row tables.
    pub use_bits: u16,
    /// The same bits in the **Throw Out** list build (content id `0x22`,
    /// `crate::menu_list_rows::build_throw_out_rows`).
    pub throw_bits: u16,
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
/// `legaia_engine_menu_runtime::field_menu_dispatch::apply_inventory_outcome` with
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
    /// (`legaia_engine_menu_runtime::field_menu_dispatch::warp_destinations`). Empty when the
    /// executable was not reachable at boot - the route then opens an
    /// empty list rather than an invented one.
    warp_destinations: Vec<WarpDestination>,
    /// Destination a committed Door of Wind pick staged, in retail's
    /// `0x80084624`/`28`/`2C` shape. Drained by
    /// `legaia_engine_menu_runtime::field_menu_dispatch::apply_pause_items_outcome` onto
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
    /// `legaia_engine_menu_runtime::field_menu_dispatch::apply_pause_items_outcome` replays onto
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
    /// `legaia_engine_battle_vm::battle_action::item_count_gate`)
    pub fn with_incense_window(mut self, window: i32) -> Self {
        self.incense_window = window;
        self.incense_blocked = !legaia_engine_battle_vm::battle_action::item_count_gate(window);
        self
    }

    /// Incense confirms this screen committed (see [`Self::with_incense_window`]).
    pub fn incense_uses(&self) -> u8 {
        self.incense_uses
    }

    /// Whether the Use list's Incense row is greyed right now (see
    /// [`Self::with_incense_window`]): the build-time dim bit, kept current
    /// across the confirms this screen commits.
    pub fn incense_blocked(&self) -> bool {
        self.incense_blocked
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
                        self.incense_window = legaia_engine_system::incense_window::top_up_cooldown(
                            self.incense_window,
                        );
                        self.incense_blocked =
                            !legaia_engine_battle_vm::battle_action::item_count_gate(
                                self.incense_window,
                            );
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
