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

/// Default bag capacity backing the Items list's page count. The retail
/// header reads `PAGE 1 / 6` on the catalogued capture - six 12-row pages
/// = 72 bag slots (the `0x80085958 + i*2` slot array scanned over
/// `_DAT_8007B5EA.._DAT_8007B5EC`).
pub const DEFAULT_BAG_PAGES: u16 = 6;

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
}

impl MenuTextTables {
    /// Parse all three tables out of a `SCUS_942.54` image (each
    /// best-effort).
    pub fn from_scus(scus: &[u8]) -> Self {
        Self {
            item_names: legaia_asset::item_names::ItemNameTable::from_scus(scus),
            spell_names: legaia_asset::spell_names::SpellNameTable::from_scus(scus),
            passives: legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus(scus),
        }
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
    /// white description line)` - what `FUN_801D0F1C` draws in the extra
    /// widget box from the `0x8007625C` record's `+4` / `+8` strings.
    pub fn item_passive_lines(&self, id: u8) -> Option<(String, String)> {
        let (_, record) = self.passives.as_ref()?.passive(id)?;
        let name = record.name.clone()?;
        // The white line is the description's first line (the retail `|`
        // break maps below the box; the box shows one line per row).
        let desc = record
            .description
            .clone()
            .map(|d| d.split('|').next().unwrap_or_default().trim().to_string())
            .unwrap_or_default();
        Some((name, desc))
    }
}

/// One bag row of the Items screen, resolved at session build.
#[derive(Debug, Clone, Default)]
pub struct PauseItemRow {
    pub id: u8,
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
/// [`crate::field_menu_dispatch::apply_inventory_outcome`] with
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
    /// ([`crate::field_menu_dispatch::warp_destinations`]). Empty when the
    /// executable was not reachable at boot - the route then opens an
    /// empty list rather than an invented one.
    warp_destinations: Vec<WarpDestination>,
    /// Destination a committed Door of Wind pick staged, in retail's
    /// `0x80084624`/`28`/`2C` shape. Drained by
    /// [`crate::field_menu_dispatch::apply_pause_items_outcome`] onto
    /// [`crate::world::World::pending_menu_warp`].
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
    /// Flat hand position over [`Self::rows`] (all bag rows).
    cursor: usize,
    /// Set when the player backs out of the command window (Circle /
    /// Triangle) - the screen is finished without an item use.
    closed: bool,
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
            cursor: 0,
            closed: false,
        }
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

    /// Total page count: the fixed bag capacity's page count (the retail
    /// header shows the bag's page total, not the held-item count).
    pub fn pages(&self) -> u16 {
        DEFAULT_BAG_PAGES.max(self.rows.len().div_ceil(LIST_PAGE_ROWS).max(1) as u16)
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
                        1 => self.focus = PauseItemsFocus::ThrowOutList,
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
                    // window and return to submenu 5.
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
                if let Some(id) = sp.consumed_item_id() {
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
                    SpecialUseOutcome::EncounterSuppress | SpecialUseOutcome::Cancelled => {
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

    /// The throw-out delete: discard the selected row's whole stack
    /// (retail zeroes both bytes of the bag slot pair), step the hand
    /// back when it sat on the last row, and drop back to the command
    /// window when the bag scan comes up empty.
    fn throw_out_selected(&mut self) {
        if self.cursor >= self.rows.len() {
            self.focus = PauseItemsFocus::ThrowOutList;
            return;
        }
        let row = self.rows.remove(self.cursor);
        self.inner.thrown_items.push(row.id);
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
    if n == 0 {
        return 0;
    }
    let mut c = cursor.min(n - 1);
    let rows = LIST_PAGE_ROWS;
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
    /// session alone; a host reads [`crate::world::World::point_card`] and
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
/// target picker. `text` fills descriptions; names fall back
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
    let pages = spells.len().div_ceil(LIST_PAGE_ROWS).max(1) as u16;
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
/// Driven from [`target_panel_view_model`], the host entry point for the
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
    /// `0x89` here, [`crate::field_menu_dispatch::build_pause_items_session`]
    /// fills the rows from the disc placement table, and
    /// [`items_screen_model`] projects them through the shared list channel so
    /// both hosts draw the screen with the list renderer they already call.
    ///
    /// A pick warps: retail's phase 4 writes `_DAT_8007B43C = 5` and the
    /// outer menu SM acts on it; the port stages the destination on
    /// [`crate::world::World::pending_menu_warp`] and the world tick's
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

/// Host entry point for the window-14 target panel: derive the retail
/// preview word for the staged item off the world's disc item-effect
/// table, build the view model, then fill the per-member record-side
/// fields (base maxima + base stats) the water previews draw from the
/// live party records.
///
/// This is the call the pause-menu host makes while the Items screen is
/// in target select; [`target_panel_mode`] and [`target_panel_model`]
/// are its two halves.
pub fn target_panel_view_model(
    s: &PauseItemsSession,
    world: &crate::world::World,
) -> Option<TargetPanelModel> {
    let mode = staged_use_item_id(s)
        .and_then(|id| {
            let table = world.item_effects.as_ref()?;
            let eff = table.effect(id)?;
            Some(target_panel_mode(table.kind(id), eff.class, eff.tier))
        })
        .unwrap_or(0);
    let mut model = target_panel_model(s, mode)?;
    for (m, row) in model.members.iter_mut().zip(s.inner.targets.iter()) {
        // Party rows index the roster by slot; monster rows (battle-side
        // targets) have no character record and keep the zeroed fields.
        let Some(rec) = world.roster.members.get(row.slot as usize) else {
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
/// already-staged template at `DAT_801E4700`, finds the first `0xC1` and the
/// first `0xC5` markup token in it (`FUN_8003CBF8`, the same `0xC0`-class
/// lead-byte scan the dialog strcpy/strcat use) and overwrites **the byte
/// following each token** in place. So the template's operand slots are
/// placeholders the renderer refills every frame, not values baked when the
/// message was staged.
///
/// The arithmetic is what the disassembly pins: the `0xC1` operand is the
/// low byte of `selector` (`_DAT_8007BB70`) and the `0xC5` operand is
/// `base + selector * 0x40` (`_DAT_8007BB78` plus the **halfword** at
/// `_DAT_8007BB70` scaled by `0x40`), both truncated to a byte by the `sb`.
/// What the two globals index is not pinned.
///
/// PORT: FUN_801dcd58 (menu-overlay notify-window content renderer)
/// REF: FUN_8003cbf8 (the markup-token scan whose offset the operand write
/// is relative to)
///
/// NOT WIRED: nothing stages a message template for this to patch. The
/// engine reports an item-use result as a typed event the host renders
/// with its own text, so there is no `DAT_801E4700` buffer holding
/// `0xC1` / `0xC5` markup tokens, and no host raises menu-overlay window
/// `8` at all. Wiring it needs the staged-template notify window to exist
/// first - and the two globals the operands index (`_DAT_8007BB70` /
/// `_DAT_8007BB78`) are themselves still unpinned.
pub fn notify_window_operands(window: (i16, i16), selector: i16, base: u8) -> NotifyWindow {
    let (wx, wy) = window;
    NotifyWindow {
        c1_operand: selector as u8,
        c5_operand: base.wrapping_add((selector.wrapping_mul(0x40)) as u8),
        text_pen: (wx, wy),
        cursor_pen: (wx + 0xE6, wy + 0xD),
    }
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
/// [`crate::world::World::scene_save_allowed`], which scene load seeds from
/// [`legaia_asset::man_section::ManHeader::low_flag`], and
/// `entry_context_kind` from
/// [`crate::world::World::menu_entry_context_kind`]. The save gate is the one
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
/// NOT WIRED: same missing entry context as [`root_menu_confirm_route`],
/// and a second missing piece of its own.
/// [`crate::field_menu::FieldMenuSession`] closes on Circle with
/// `FieldMenuOutcome::Closed`; it has neither a sub-screen id space for
/// the `0` / `3` return values to name nor a leave-confirm screen for the
/// locked-context arm to select, so the prerequisite is a second pause-menu
/// screen (the Yes/No leave confirm), not just the context byte.
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
pub const CONTEXT_LOCKED_ENTRY_SUBSCREEN: u8 = 4;

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
    /// `(label, current, preview)`. Empty when nothing is previewed.
    pub stat_compare: Vec<(&'static str, u16, u16)>,
    pub phase: EquipScreenPhase,
    /// Cursor row inside the active phase column.
    pub cursor: u16,
    /// Active slot index in `ItemPicker` / `Confirm`.
    pub active_slot: u8,
    /// Pending-swap label above the Yes/No prompt.
    pub confirm_label: Option<String>,
    /// Roster slot of the character being equipped.
    pub char_slot: u8,
    /// Slot-picker cursor row, or `None` past the slot picker - what the
    /// sprite pass puts the second hand on.
    pub slot_cursor: Option<u16>,
    /// Pictogram rows the sprite pass draws. Retail draws exactly 7; the
    /// engine's 8th slot row stays navigable but icon-less so the column
    /// matches the retail capture.
    pub pictogram_rows: usize,
}

/// Project a live [`crate::equip_session::EquipSession`] into
/// [`EquipScreenModel`].
///
/// `party_names` is the world's roster snapshot, which the session does not
/// carry. The stat preview uses the neutral status set: this is the field
/// menu, and the session recomputes with live status modifiers on commit.
pub fn equip_screen_model(
    session: &crate::equip_session::EquipSession,
    char_slot: u8,
    party_names: &[String],
) -> EquipScreenModel {
    use crate::equip_session::EquipState;
    use crate::equipment::EquipSlot;

    let record = session.record();
    let slot_labels: Vec<String> = (0..8u8)
        .map(|i| {
            EquipSlot::from_index(i)
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| format!("Slot {i}"))
        })
        .collect();
    let slot_items: Vec<String> = record
        .equip
        .iter()
        .map(|&id| {
            if id == 0 {
                String::new()
            } else {
                format!("Item {id:02X}")
            }
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
            Some(format!("Equip Item {item_id:02X}?")),
        ),
        EquipState::Done(_) => (EquipScreenPhase::SlotPicker, 0, 0, None),
    };

    // Candidates + stat compare only matter past the slot picker.
    let (candidate_names, candidate_counts, considered_id): (Vec<String>, Vec<u8>, Option<u8>) =
        if phase == EquipScreenPhase::SlotPicker {
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
            // The item the compare block previews: the hovered row in the
            // picker, the pending item in the confirm phase.
            let considered = match session.state() {
                EquipState::Confirm { item_id, .. } => Some(item_id),
                _ => items.get(cursor as usize).map(|it| it.id),
            };
            (names, counts, considered)
        };

    let stat_compare: Vec<(&'static str, u16, u16)> = match considered_id {
        Some(id) => {
            let neutral = crate::battle_stats::StatusModifiers::default();
            let cur = crate::battle_stats::compute_battle_stats(
                record,
                session.equipment(),
                &[],
                &neutral,
            );
            let mut copy = *record;
            copy.equip[active_slot as usize] = id;
            let new = crate::battle_stats::compute_battle_stats(
                &copy,
                session.equipment(),
                &[],
                &neutral,
            );
            vec![
                ("ATK", cur.atk, new.atk),
                ("UDF", cur.udf, new.udf),
                ("LDF", cur.ldf, new.ldf),
            ]
        }
        None => Vec::new(),
    };

    EquipScreenModel {
        party_names: party_names.to_vec(),
        slot_labels,
        slot_items,
        candidate_names,
        candidate_counts,
        stat_compare,
        phase,
        cursor,
        active_slot,
        confirm_label,
        char_slot,
        slot_cursor: match session.state() {
            EquipState::SlotPicker { cursor } => Some(cursor as u16),
            _ => None,
        },
        pictogram_rows: record.equip.len().min(7),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory_use::{InventoryContext, TargetRow};
    use crate::items::ItemCatalog;
    use crate::spell_menu::{CasterSlot, SpellMenuInput};
    use crate::spells::SpellCatalog;

    fn items_session(ids_counts: &[(u8, u8)]) -> PauseItemsSession {
        let items: Vec<u8> = ids_counts.iter().map(|(id, _)| *id).collect();
        let rows: Vec<PauseItemRow> = ids_counts
            .iter()
            .map(|(id, count)| PauseItemRow {
                id: *id,
                name: format!("Item {id:02X}"),
                count: *count,
                desc: format!("Desc {id:02X}"),
                passive: None,
            })
            .collect();
        let targets = vec![TargetRow::new(0, "Vahn").with_stats(50, 100, 10, 30)];
        let inner = InventoryUseSession::new(
            ItemCatalog::vanilla(),
            items,
            targets,
            InventoryContext::Field,
        );
        PauseItemsSession::new(inner, rows)
    }

    fn edge(b: PadButton) -> u16 {
        b.mask()
    }

    /// The screen opens in command focus; Cross on "Use" moves the hand
    /// into the list; Circle in the list returns to the command window;
    /// Circle there closes.
    #[test]
    fn items_focus_walk_command_list_command_close() {
        let mut s = items_session(&[(0x77, 3)]);
        assert_eq!(s.focus, PauseItemsFocus::Command);
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::List);
        s.input_pad_edge(edge(PadButton::Circle));
        assert_eq!(s.focus, PauseItemsFocus::Command);
        assert!(!s.is_done());
        s.input_pad_edge(edge(PadButton::Circle));
        assert!(s.is_done());
    }

    /// An empty bag keeps the hand on the command window ("Use" refuses).
    #[test]
    fn items_empty_bag_refuses_list_entry() {
        let mut s = items_session(&[]);
        assert!(s.bag_empty());
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::Command);
    }

    /// Left/Right flip 12-row pages over the bag; the model slices the
    /// visible page and reports the retail 6-page bag total.
    #[test]
    fn items_page_flip_and_model_slice() {
        let rows: Vec<(u8, u8)> = (1..=30).map(|i| (i, 1)).collect();
        let mut s = items_session(&rows);
        s.input_pad_edge(edge(PadButton::Cross)); // into the list
        let m = items_screen_model(&s);
        assert_eq!(m.page, 1);
        assert_eq!(m.pages, DEFAULT_BAG_PAGES);
        assert_eq!(m.page_rows.len(), LIST_PAGE_ROWS);
        assert!(m.focus_list);

        s.input_pad_edge(edge(PadButton::Right));
        let m = items_screen_model(&s);
        assert_eq!(m.page, 2);
        assert_eq!(m.list_cursor_on_page, 0);
        // Page 3 holds the remaining 6 rows.
        s.input_pad_edge(edge(PadButton::Right));
        let m = items_screen_model(&s);
        assert_eq!(m.page, 3);
        assert_eq!(m.page_rows.len(), 6);
        // Clamped at the last row; Left returns.
        s.input_pad_edge(edge(PadButton::Left));
        let m = items_screen_model(&s);
        assert_eq!(m.page, 2);
    }

    /// Throw Out walk (FUN_801D8734): command row 1 enters the discard
    /// list; Cross opens the confirm seeded on "No"; confirming "No"
    /// returns to the list; confirming "Yes" discards the whole stack,
    /// records it on the inner session and returns to the list.
    #[test]
    fn items_throw_out_confirm_defaults_no_and_discards_stack() {
        let mut s = items_session(&[(0x77, 3), (0x78, 2)]);
        s.input_pad_edge(edge(PadButton::Down)); // -> Throw Out
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::ThrowOutConfirm);
        assert_eq!(s.confirm_cursor, 1, "retail seeds the confirm on No");
        // Confirm "No": nothing discarded, back to the list.
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
        assert_eq!(s.rows.len(), 2);
        // Re-open, toggle to "Yes", confirm: stack 0x77 goes.
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Up));
        assert_eq!(s.confirm_cursor, 0);
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].id, 0x78);
        assert_eq!(s.inner.thrown_items, vec![0x77]);
        assert_eq!(s.inner.items, vec![0x78]);
    }

    /// The throw-out view model stages the confirm window content, and
    /// the confirm phases keep the list focus (grey rows).
    #[test]
    fn items_throw_confirm_model_content() {
        let mut s = items_session(&[(0x77, 12)]);
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Cross));
        let m = items_screen_model(&s);
        assert!(m.focus_list);
        assert!(m.throw_confirm.is_none());
        s.input_pad_edge(edge(PadButton::Cross));
        let m = items_screen_model(&s);
        let confirm = m.throw_confirm.expect("confirm open");
        assert_eq!(confirm.name, "Item 77");
        assert_eq!(confirm.count, 12);
        assert_eq!(confirm.cursor, 1);
        assert!(m.focus_list);
    }

    /// Discarding the last remaining stack drops the hand back onto the
    /// command window (the retail bag rescan finds nothing and returns
    /// to submenu 5); discarding the last *row* steps the hand back.
    #[test]
    fn items_throw_out_empties_bag_back_to_command() {
        let mut s = items_session(&[(0x77, 1), (0x78, 1)]);
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Cross));
        // Hand on the last row.
        s.input_pad_edge(edge(PadButton::Down));
        assert_eq!(s.list_cursor(), 1);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Up)); // Yes
        s.input_pad_edge(edge(PadButton::Cross));
        // Last-row fix-up: the hand stepped back onto the remaining row.
        assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
        assert_eq!(s.list_cursor(), 0);
        // Discard the final stack: back to the command window.
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Up));
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::Command);
        assert!(s.bag_empty());
        assert_eq!(s.inner.thrown_items, vec![0x78, 0x77]);
        assert!(!s.is_done(), "the screen stays open on the command window");
    }

    /// Circle backs out of the confirm and out of the throw-out list
    /// without discarding.
    #[test]
    fn items_throw_out_circle_backs_out() {
        let mut s = items_session(&[(0x77, 3)]);
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Circle));
        assert_eq!(s.focus, PauseItemsFocus::ThrowOutList);
        s.input_pad_edge(edge(PadButton::Circle));
        assert_eq!(s.focus, PauseItemsFocus::Command);
        assert!(s.inner.thrown_items.is_empty());
        assert_eq!(s.rows.len(), 1);
    }

    /// Arrange (FUN_801D64A8): rows re-sort by the rank table and the
    /// list scroll resets; the inner id list stays parallel.
    #[test]
    fn items_arrange_sorts_rows_by_rank_table() {
        use crate::menu_arrange::ArrangeRankTable;
        let mut s = items_session(&[(0x10, 1), (0x20, 2), (0x30, 3)]);
        // Rank order reverses the id order: 0x30 first, 0x10 last.
        let mut order = [0u8; 0x100];
        order[0] = 0x30;
        order[1] = 0x20;
        order[2] = 0x10;
        s = s.with_arrange_rank(Some(ArrangeRankTable::from_display_order(&order)));
        // Park the hand mid-list first (via Use focus), then back out and
        // Arrange: the cursor resets to the top.
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Circle));
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Down)); // -> Arrange
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::Command);
        let ids: Vec<u8> = s.rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![0x30, 0x20, 0x10]);
        assert_eq!(s.inner.items, ids);
        assert_eq!(s.list_cursor(), 0, "retail zeroes the list scroll");
    }

    /// An empty bag buzzes every command row (the FUN_801D7C00 bag scan
    /// gates the dispatch, not just "Use").
    #[test]
    fn items_empty_bag_refuses_throw_and_arrange() {
        let mut s = items_session(&[]);
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::Command);
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::Command);
    }

    /// The info model carries the hovered row's real count + description.
    #[test]
    fn items_info_follows_hovered_row() {
        let mut s = items_session(&[(0x77, 9), (0x78, 2)]);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Down));
        let m = items_screen_model(&s);
        let info = m.info.expect("hovered row staged");
        assert_eq!(info.name, "Item 78");
        assert_eq!(info.count, 2);
        assert_eq!(info.desc, "Desc 78");
    }

    fn magic_session() -> SpellMenuSession {
        let party = vec![
            CasterSlot {
                slot: 0,
                name: "Vahn".into(),
                hp: 60,
                mp: 30,
                hp_max: 100,
                mp_max: 120,
                level: 7,
                spells: vec![0x81, 0x9c],
                spell_levels: vec![2, 1],
                ability_bits: 0,
                ra_seru_missing: false,
            },
            CasterSlot {
                slot: 1,
                name: "Noa".into(),
                hp: 50,
                mp: 40,
                hp_max: 90,
                mp_max: 80,
                level: 6,
                spells: vec![0x83],
                spell_levels: vec![3],
                ability_bits: 0,
                ra_seru_missing: false,
            },
        ];
        let targets = vec![crate::spell_menu::TargetRow {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            hp_max: 100,
        }];
        SpellMenuSession::new(party, targets, SpellCatalog::vanilla())
    }

    /// Caster focus: mp/mp_max plumb through; the hovered caster's list
    /// previews white (focus_list = false) with no staged info.
    #[test]
    fn magic_model_caster_focus_carries_mp_max() {
        let s = magic_session();
        let m = magic_screen_model(&s, None);
        assert!(!m.focus_list);
        assert_eq!(m.casters.len(), 2);
        assert_eq!(m.casters[0], ("Vahn".to_string(), 7, 30, 120));
        assert_eq!(m.casters[1].3, 80);
        assert!(m.info.is_none());
        assert_eq!(m.page_rows.len(), 2);
    }

    /// List focus: rows grey (focus_list), the hovered spell stages into
    /// the info window with its learned level; Ra-Seru ids flag the wider
    /// icon.
    #[test]
    fn magic_model_list_focus_stages_info() {
        let mut s = magic_session();
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SpellMenuPhase::SpellSelect { .. }));
        let m = magic_screen_model(&s, None);
        assert!(m.focus_list);
        let info = m.info.expect("hovered spell staged");
        assert_eq!(info.level, 2);
        assert!(!info.ra_seru);
        // Row 1 (0x9c = Horn) is in the Ra-Seru block.
        assert!(m.page_rows[1].1);
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let m = magic_screen_model(&s, None);
        let info = m.info.expect("hovered spell staged");
        assert!(info.ra_seru);
        assert_eq!(info.level, 1);
    }

    /// Description + name fall back through the MenuTextTables when the
    /// catalog has no entry.
    #[test]
    fn magic_model_desc_resolves_through_text_tables() {
        let mut s = magic_session();
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        let mut entries = vec![legaia_asset::spell_names::SpellEntry::default(); 0x82];
        entries[0x81].desc = Some("Crazy Driver\nAttack enemies.".to_string());
        let text = MenuTextTables {
            spell_names: Some(legaia_asset::spell_names::SpellNameTable::from_entries(
                entries,
            )),
            ..Default::default()
        };
        let m = magic_screen_model(&s, Some(&text));
        let info = m.info.expect("hovered spell staged");
        assert_eq!(info.desc, "Crazy Driver\nAttack enemies.");
    }

    /// PIN: the Magic screen's displayed MP cost is discounted through the
    /// per-caster MP-cost kernel (`FUN_80035394`). A caster with the half-MP
    /// ability bit (`0x20`) shows half cost; the quarter bit (`0x10`) shows a
    /// quarter shaved off; both set = half wins; no bits = full cost.
    fn staged_mp_cost(ability_bits: u32) -> u16 {
        let mut catalog = SpellCatalog::new();
        catalog.insert(crate::spells::SpellDef {
            id: 0x81,
            name: "Costly".into(),
            mp_cost: 40,
            ..Default::default()
        });
        let party = vec![CasterSlot {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            mp: 120,
            hp_max: 100,
            mp_max: 120,
            level: 7,
            spells: vec![0x81],
            spell_levels: vec![1],
            ability_bits,
            ra_seru_missing: false,
        }];
        let targets = vec![crate::spell_menu::TargetRow {
            slot: 0,
            name: "Vahn".into(),
            hp: 60,
            hp_max: 100,
        }];
        let mut s = SpellMenuSession::new(party, targets, catalog);
        // Enter the spell list so the hovered row stages into the info window.
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SpellMenuPhase::SpellSelect { .. }));
        magic_screen_model(&s, None)
            .info
            .expect("hovered spell staged")
            .mp_cost
    }

    /// The kind-4 list kernel's pad decode (FUN_80032A44): Up/Down wrap
    /// within the visible page, Left/Right are the only scroll.
    #[test]
    fn list_kernel_navigate_page_local_wrap() {
        let n = 30; // pages: 0..12, 12..24, 24..30
        let up = edge(PadButton::Up);
        let down = edge(PadButton::Down);
        let left = edge(PadButton::Left);
        let right = edge(PadButton::Right);
        // Up above the page top steps back one row.
        assert_eq!(list_kernel_navigate(13, n, up), 12);
        // Up at a page top wraps to that page's last row.
        assert_eq!(list_kernel_navigate(12, n, up), 23);
        // ...clamped to the row count on the last partial page.
        assert_eq!(list_kernel_navigate(24, n, up), 29);
        // Down steps forward; past the page bottom wraps to the page top.
        assert_eq!(list_kernel_navigate(10, n, down), 11);
        assert_eq!(list_kernel_navigate(11, n, down), 0);
        // Down past the last row wraps to the last page's top.
        assert_eq!(list_kernel_navigate(29, n, down), 24);
        // Left only pages while scrolled; Right only while rows remain.
        assert_eq!(list_kernel_navigate(5, n, left), 5);
        assert_eq!(list_kernel_navigate(17, n, left), 5);
        assert_eq!(list_kernel_navigate(5, n, right), 17);
        assert_eq!(list_kernel_navigate(26, n, right), 26);
        // Right clamps the selection to the last row.
        assert_eq!(list_kernel_navigate(23, n, right), 29);
        // Empty list is inert.
        assert_eq!(list_kernel_navigate(0, 0, down), 0);
    }

    /// FUN_801D7E50 phase-2 dispatch: classes 0x80..0x82 take the
    /// dedicated routes, flag bit 0x20 picks the all-party apply.
    #[test]
    fn use_route_dispatch_matches_retail() {
        assert_eq!(use_route_for_effect(0x80, 0x82), UseRoute::DoorOfLight);
        assert_eq!(use_route_for_effect(0x81, 0x82), UseRoute::DoorOfWind);
        assert_eq!(use_route_for_effect(0x82, 0x82), UseRoute::Incense);
        assert_eq!(use_route_for_effect(0x00, 0xA2), UseRoute::ApplyAll);
        assert_eq!(use_route_for_effect(0x00, 0x82), UseRoute::ApplySingle);
        assert_eq!(use_route_for_effect(0x06, 0x86), UseRoute::ApplySingle);
    }

    /// FUN_801D6A54: only kind-2 items with effect class 6 preview;
    /// args 0/5 share the HP/MP panel, 1..=4 map onto modes 2..=5.
    #[test]
    fn target_panel_mode_matches_retail_map() {
        assert_eq!(target_panel_mode(2, 6, 0), 1); // Life Water
        assert_eq!(target_panel_mode(2, 6, 5), 1); // Magic Water
        assert_eq!(target_panel_mode(2, 6, 1), 2); // Power Water
        assert_eq!(target_panel_mode(2, 6, 2), 3); // Guardian Water
        assert_eq!(target_panel_mode(2, 6, 3), 4); // Swift Water
        assert_eq!(target_panel_mode(2, 6, 4), 5); // Wisdom Water
        assert_eq!(target_panel_mode(2, 6, 6), 0);
        assert_eq!(target_panel_mode(2, 0, 0), 0); // healing item
        assert_eq!(target_panel_mode(0, 6, 0), 0); // wrong kind byte
    }

    /// **All three** special routes map here. The earlier reading dropped
    /// Door of Wind because its screen is a destination *list* rather than
    /// a Yes/No window - but `FUN_801D7E50`'s phase-2 dispatch
    /// (`801d7f80..801d7fd8`) branches all three effect classes out of the
    /// target-panel flow at the same place, and it is
    /// [`SpecialUseSession::new`] that picks the screen shape. Filtering
    /// `0x81` out here is what made submenu `0xC` unreachable: with no
    /// route, a Door of Wind confirm fell through to the ordinary use flow,
    /// where the item is not even in the catalog, and the press did nothing
    /// at all.
    #[test]
    fn all_three_special_routes_map_to_a_route() {
        assert_eq!(
            special_use_route_for_item(DOOR_OF_LIGHT_ITEM_ID),
            Some(UseRoute::DoorOfLight)
        );
        assert_eq!(
            special_use_route_for_item(INCENSE_ITEM_ID),
            Some(UseRoute::Incense)
        );
        assert_eq!(
            special_use_route_for_item(DOOR_OF_WIND_ITEM_ID),
            Some(UseRoute::DoorOfWind)
        );
        assert_eq!(special_use_route_for_item(0x01), None);
        // The screen shape still splits two ways: only the Yes/No routes
        // open in `Confirm`.
        for (id, phase) in [
            (DOOR_OF_LIGHT_ITEM_ID, SpecialUsePhase::Confirm),
            (INCENSE_ITEM_ID, SpecialUsePhase::Confirm),
            (DOOR_OF_WIND_ITEM_ID, SpecialUsePhase::PickDestination),
        ] {
            let route = special_use_route_for_item(id).expect("route");
            assert_eq!(SpecialUseSession::new(route, vec![]).phase, phase);
        }
    }

    /// Confirming a Door of Light in the Use list opens the route's own
    /// confirm window instead of the target panel, and the confirm seeds
    /// to **Yes** - the opposite default from the Throw Out confirm.
    #[test]
    fn use_list_confirm_on_door_of_light_opens_the_special_confirm() {
        let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 1)]);
        s.input_pad_edge(edge(PadButton::Cross)); // Use -> list
        assert_eq!(s.focus, PauseItemsFocus::List);
        s.input_pad_edge(edge(PadButton::Cross)); // confirm the row
        assert_eq!(s.focus, PauseItemsFocus::SpecialRoute);
        assert!(!s.target_select(), "the target panel must not open");
        let sp = s.special_use().expect("route session");
        assert_eq!(sp.route, UseRoute::DoorOfLight);
        assert_eq!(sp.cursor, 0, "seeded on Yes");
        let model = items_screen_model(&s);
        let sc = model.special_confirm.expect("confirm model");
        assert_eq!(sc.route, UseRoute::DoorOfLight);
        assert_eq!(sc.cursor, 0);
    }

    /// Yes on the Door of Light closes the whole menu (retail hands the
    /// field exit code 4); Yes on an Incense applies in place and drops
    /// back to the Use list, as does a cancel.
    #[test]
    fn special_confirm_outcomes_route_back_the_way_retail_does() {
        let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 1)]);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross)); // Yes
        assert!(s.is_done());
        assert_eq!(
            s.special_use().and_then(|sp| sp.exit_code()),
            Some(MENU_EXIT_CODE_FIELD_ESCAPE)
        );
        assert_eq!(
            s.special_use().and_then(|sp| sp.consumed_item_id()),
            Some(DOOR_OF_LIGHT_ITEM_ID)
        );

        let mut s = items_session(&[(INCENSE_ITEM_ID, 1)]);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross)); // Yes
        assert!(!s.is_done(), "Incense stays on the Items screen");
        assert_eq!(s.focus, PauseItemsFocus::List);
        assert_eq!(
            s.take_special_use().and_then(|sp| sp.consumed_item_id()),
            Some(INCENSE_ITEM_ID)
        );

        let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 1)]);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Circle)); // cancel
        assert!(!s.is_done());
        assert_eq!(s.focus, PauseItemsFocus::List);
        assert_eq!(s.special_use().and_then(|sp| sp.consumed_item_id()), None);
    }

    /// A Door of **Wind** confirm opens the destination list, not a Yes/No
    /// window - and the Items screen model projects the landmarks through
    /// the shared list channel with no confirm prompt attached.
    #[test]
    fn door_of_wind_opens_the_destination_list_not_a_confirm() {
        let towns = vec![
            crate::pause_screens::WarpDestination {
                record_index: 0,
                name: "Rim Elm".into(),
                scene_id: 0x0055,
                menu_x: 0x60,
                menu_y: 0x19,
            },
            crate::pause_screens::WarpDestination {
                record_index: 4,
                name: "Drake Castle".into(),
                scene_id: 0x0162,
                menu_x: 0x36,
                menu_y: 0x3E,
            },
        ];
        let mut s = items_session(&[(DOOR_OF_WIND_ITEM_ID, 1)]).with_warp_destinations(towns);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.focus, PauseItemsFocus::SpecialRoute);
        let sp = s.special_use().expect("route session");
        assert_eq!(sp.route, UseRoute::DoorOfWind);
        assert_eq!(sp.phase, SpecialUsePhase::PickDestination);
        assert_eq!(sp.landmarks, vec!["Rim Elm", "Drake Castle"]);
        let m = items_screen_model(&s);
        assert!(
            m.special_confirm.is_none(),
            "no Yes/No prompt on this route"
        );
        assert!(m.info.is_none(), "the info window stays closed");
        assert_eq!(
            m.page_rows
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>(),
            vec!["Rim Elm", "Drake Castle"]
        );
        assert_eq!(m.list_cursor_on_page, 0);
        assert_eq!((m.page, m.pages), (1, 1));

        // Picking the second row stages that record's triple and hands the
        // menu the world-map warp exit code, consuming exactly one 0x89.
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Cross));
        assert!(s.is_done());
        assert_eq!(s.exit_code(), Some(MENU_EXIT_CODE_WORLD_MAP_WARP));
        assert_eq!(
            s.staged_warp(),
            Some(crate::pause_screens::StagedWarp {
                scene_id: 0x0162,
                menu_x: 0x36,
                menu_y: 0x3E,
            })
        );
        assert_eq!(s.inner.consumed_items, vec![DOOR_OF_WIND_ITEM_ID]);
    }

    /// The one-shot commit guard: a host that keeps ticking a finished
    /// screen before it notices `is_done` must not consume a second copy.
    #[test]
    fn a_committed_special_route_consumes_exactly_once() {
        let mut s = items_session(&[(DOOR_OF_LIGHT_ITEM_ID, 3)]);
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross));
        s.input_pad_edge(edge(PadButton::Cross)); // Yes
        for _ in 0..5 {
            s.input_pad_edge(edge(PadButton::Cross));
        }
        assert_eq!(s.inner.consumed_items, vec![DOOR_OF_LIGHT_ITEM_ID]);
    }

    /// Door of Light (FUN_801D8A58): Yes/No confirm seeded on Yes;
    /// confirming Yes consumes 0x88 and exits with code 4; "No" and
    /// Circle cancel without consuming.
    #[test]
    fn special_use_door_of_light_confirm() {
        let mut s = SpecialUseSession::new(UseRoute::DoorOfLight, vec![]);
        assert_eq!(s.phase, SpecialUsePhase::Confirm);
        assert_eq!(s.cursor, 0, "retail seeds the confirm on Yes");
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(
            s.phase,
            SpecialUsePhase::Done(SpecialUseOutcome::FieldEscape)
        );
        assert_eq!(s.consumed_item_id(), Some(DOOR_OF_LIGHT_ITEM_ID));
        assert_eq!(s.exit_code(), Some(MENU_EXIT_CODE_FIELD_ESCAPE));

        let mut s = SpecialUseSession::new(UseRoute::DoorOfLight, vec![]);
        s.input_pad_edge(edge(PadButton::Down)); // -> No
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(s.phase, SpecialUsePhase::Done(SpecialUseOutcome::Cancelled));
        assert_eq!(s.consumed_item_id(), None);
        assert_eq!(s.exit_code(), None);
    }

    /// Incense (FUN_801D8D94): Yes consumes 0x8A and applies the
    /// encounter suppression without exiting the menu.
    #[test]
    fn special_use_incense_confirm() {
        let mut s = SpecialUseSession::new(UseRoute::Incense, vec![]);
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(
            s.phase,
            SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress)
        );
        assert_eq!(s.consumed_item_id(), Some(INCENSE_ITEM_ID));
        assert_eq!(s.exit_code(), None, "Incense drops back to the Use list");
    }

    /// Door of Wind (FUN_801D8B90): the destination list opens directly;
    /// a pick consumes 0x89 and exits with the world-map warp code;
    /// Circle cancels back to the Use list.
    #[test]
    fn special_use_door_of_wind_pick() {
        let towns = vec!["Rim Elm".to_string(), "Drake Castle".to_string()];
        let mut s = SpecialUseSession::new(UseRoute::DoorOfWind, towns.clone());
        assert_eq!(s.phase, SpecialUsePhase::PickDestination);
        s.input_pad_edge(edge(PadButton::Down));
        s.input_pad_edge(edge(PadButton::Cross));
        assert_eq!(
            s.phase,
            SpecialUsePhase::Done(SpecialUseOutcome::Warp { landmark: 1 })
        );
        assert_eq!(s.consumed_item_id(), Some(DOOR_OF_WIND_ITEM_ID));
        assert_eq!(s.exit_code(), Some(MENU_EXIT_CODE_WORLD_MAP_WARP));

        let mut s = SpecialUseSession::new(UseRoute::DoorOfWind, towns);
        s.input_pad_edge(edge(PadButton::Circle));
        assert_eq!(s.phase, SpecialUsePhase::Done(SpecialUseOutcome::Cancelled));
    }

    /// The target-panel model assembles from the inner flow's target
    /// rows while (and only while) the use flow is in target select.
    #[test]
    fn target_panel_model_from_target_select() {
        let mut s = items_session(&[(0x77, 3)]);
        assert!(target_panel_model(&s, 0).is_none());
        s.input_pad_edge(edge(PadButton::Cross)); // -> list
        s.input_pad_edge(edge(PadButton::Cross)); // confirm -> target select
        assert!(s.target_select());
        let m = target_panel_model(&s, 1).expect("target select stages the panel");
        assert_eq!(m.mode, 1);
        assert_eq!(m.members.len(), 1);
        assert_eq!(m.members[0].name, "Vahn");
        assert_eq!(m.members[0].hp, 50);
        assert_eq!(m.members[0].hp_max, 100);
        assert!(!m.all_targets);
    }

    /// The host entry point resolves the staged bag id and, without a disc
    /// item-effect table, falls back to the plain (mode 0) panel while
    /// still filling the per-member record fields from the live roster.
    #[test]
    fn target_panel_view_model_fills_record_fields() {
        let mut s = items_session(&[(0x77, 3)]);
        let mut world = crate::world::World::new();
        assert!(target_panel_view_model(&s, &world).is_none());
        s.input_pad_edge(edge(PadButton::Cross)); // -> list
        s.input_pad_edge(edge(PadButton::Cross)); // confirm -> target select
        assert_eq!(staged_use_item_id(&s), Some(0x77));

        // Slot 0 of the roster is the target row's record.
        let mut rec = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
        let mut base = rec.record_stats();
        base.hp_max = 111;
        base.mp_max = 22;
        base.atk = 33;
        base.udf = 34;
        base.ldf = 35;
        base.spd = 36;
        base.int = 37;
        rec.set_record_stats(base);
        let mut live = rec.live_stats();
        live.atk = 43;
        live.udf = 44;
        live.ldf = 45;
        live.spd = 46;
        live.int = 47;
        rec.set_live_stats(live);
        world.roster.members = vec![rec];

        let m = target_panel_view_model(&s, &world).expect("target select stages the panel");
        // No disc effect table on this world - the plain panel.
        assert_eq!(m.mode, 0);
        assert_eq!(m.members.len(), 1);
        assert_eq!(m.members[0].base_hp_max, 111);
        assert_eq!(m.members[0].base_mp_max, 22);
        assert_eq!(m.members[0].stat_eff, [43, 44, 45, 46, 47]);
        assert_eq!(m.members[0].stat_base, [33, 34, 35, 36, 37]);
    }

    #[test]
    fn magic_model_displays_per_caster_discounted_mp_cost() {
        // No ability bits: full base cost.
        assert_eq!(staged_mp_cost(0x00), 40);
        // Half-MP bit (0x20): cost - (cost >> 1) = 20.
        assert_eq!(staged_mp_cost(0x20), 20);
        // Quarter bit (0x10): cost - (cost >> 2) = 30 (shaves 25%, not "to a quarter").
        assert_eq!(staged_mp_cost(0x10), 30);
        // Both bits set: Half (0x20) wins the priority - 20, not 30.
        assert_eq!(staged_mp_cost(0x30), 20);
    }

    #[test]
    fn notify_window_operands_and_pens() {
        let n = notify_window_operands((12, 30), 2, 5);
        assert_eq!(n.c1_operand, 2);
        // base + selector * 0x40, truncated to a byte by the retail `sb`.
        assert_eq!(n.c5_operand, 5 + 2 * 0x40);
        assert_eq!(n.text_pen, (12, 30));
        assert_eq!(n.cursor_pen, (12 + 0xE6, 30 + 0xD));
        // selector 4 * 0x40 = 0x100 wraps to 0 in the byte store.
        assert_eq!(notify_window_operands((0, 0), 4, 7).c5_operand, 7);
    }

    #[test]
    fn root_menu_routes_the_five_unconditional_rows() {
        for (row, want) in [(0u16, 0x05u8), (1, 0x0E), (2, 0x12), (3, 0x15), (4, 0x17)] {
            assert_eq!(
                root_menu_confirm_route(row, None, false),
                RootMenuRoute::Sub(want)
            );
        }
        assert_eq!(root_menu_confirm_route(7, None, true), RootMenuRoute::None);
    }

    #[test]
    fn root_menu_load_row_is_gated_on_the_context_kind_not_its_presence() {
        // No context at all: Load is available.
        assert_eq!(
            root_menu_confirm_route(5, None, false),
            RootMenuRoute::Sub(0x18)
        );
        // A context of some other kind: still available.
        assert_eq!(
            root_menu_confirm_route(5, Some(0x07), false),
            RootMenuRoute::Sub(0x18)
        );
        // The locked kind: buzz.
        assert_eq!(
            root_menu_confirm_route(5, Some(ROOT_MENU_CONTEXT_LOCKED), false),
            RootMenuRoute::Buzz
        );
    }

    /// Row 5 is `@Load` and row 6 is `@Save`, not the other way round, and
    /// the two gates therefore attach to the rows the labels name. The
    /// evidence is the menu overlay's own string pool: `FUN_801CFD68` hands
    /// the string primitive `0x801CEA00` for row 5 and `0x801CEA08` for row
    /// 6, and those cells hold `@Load` and `@Save`. Pinning it here keeps a
    /// future edit from re-swapping the two gates back.
    #[test]
    fn the_gated_rows_are_load_then_save_in_that_order() {
        // The scene forbids saving; nothing is parked. Load is offered and
        // Save buzzes - the shape a no-save scene has to produce.
        assert_eq!(
            root_menu_confirm_route(5, None, false),
            RootMenuRoute::Sub(0x18)
        );
        assert_eq!(root_menu_confirm_route(6, None, false), RootMenuRoute::Buzz);
        // A parked script context flips it: Save is fine, Load is refused,
        // and leaving asks first.
        assert_eq!(
            root_menu_confirm_route(5, Some(ROOT_MENU_CONTEXT_LOCKED), true),
            RootMenuRoute::Buzz
        );
        assert_eq!(
            root_menu_confirm_route(6, Some(ROOT_MENU_CONTEXT_LOCKED), true),
            RootMenuRoute::Sub(0x19)
        );
        assert_eq!(root_menu_cancel_route(Some(ROOT_MENU_CONTEXT_LOCKED)), 3);
    }

    #[test]
    fn root_menu_save_row_needs_the_scene_save_allow_flag() {
        assert_eq!(root_menu_confirm_route(6, None, false), RootMenuRoute::Buzz);
        assert_eq!(
            root_menu_confirm_route(6, None, true),
            RootMenuRoute::Sub(0x19)
        );
    }

    #[test]
    fn root_menu_cancel_asks_first_under_the_locked_context() {
        assert_eq!(root_menu_cancel_route(None), 0);
        assert_eq!(root_menu_cancel_route(Some(0x01)), 0);
        assert_eq!(root_menu_cancel_route(Some(ROOT_MENU_CONTEXT_LOCKED)), 3);
    }
}
