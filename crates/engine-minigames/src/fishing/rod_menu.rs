//! Rod / lure selection, the help panel and the fishing menu.
//! Split out of `fishing.rs`.

// --- rod / lure selection ----------------------------------------------------

/// The line-record base offset shared by the hook check (`FUN_801d4004`:
/// `record < gate + 300`) and the catch-HUD length readout (`FUN_801d1580`:
/// `record - 300`, clamped at zero). The HUD-side copy of the same literal
/// is `legaia_engine_ui::ui_fishing::RECORD_STRIKE_BASE`.
pub const RECORD_STRIKE_BASE: i32 = 300;

/// The inventory item id whose count the persistent HUD shows for the
/// selected rod index (`FUN_801d13f0`: `_DAT_80084450 + 0x9d` - the lure
/// consumable paired with the rod).
pub fn lure_item_id(rod_index: u32) -> u32 {
    0x9d + rod_index
}

/// How many rod / lure kinds the selector cycles through
/// (items `0x9d..=0x9f`, i.e. [`lure_item_id`] over `0..ROD_KINDS`).
pub const ROD_KINDS: u32 = 3;

/// The rod-ownership gate the driver runs before letting a cast start: `false`
/// parks it in the "no rod" state, `true` lets it into the main loop.
///
/// Retail sums the inventory counts of all three lure items and bails when the
/// total is zero; otherwise it *advances the persistent rod index*
/// (`_DAT_80084450`, wrapping at [`ROD_KINDS`]) until it lands on a kind the
/// player actually holds. So the gate is not read-only - selling the selected
/// lure silently re-points the selection at the next owned one, which is why
/// the HUD's rod label can change without the player touching the menu.
///
/// `count_of` supplies the live inventory count for an item id. The sum
/// guarantees termination in retail; the port bounds the scan at
/// [`ROD_KINDS`] anyway so a caller with an out-of-range index cannot hang it.
// PORT: FUN_801d712c (lure-ownership gate + persistent lure-index re-point)
// PARTLY WIRED: `World::enter_fishing_session` runs it over the live bag at
// every session entry (door warp and both play hosts' launchers) and writes
// the corrected lure index back. Its other retail role - the rod/lure
// selection screen's cursor handler, which is what lets the player *change*
// the selection - has no host UI, so that path is still unreached.
pub fn select_owned_rod(rod_index: &mut u32, mut count_of: impl FnMut(u32) -> i32) -> bool {
    let owned: i32 = (0..ROD_KINDS).map(|k| count_of(lure_item_id(k))).sum();
    if owned == 0 {
        return false;
    }
    for _ in 0..ROD_KINDS {
        if count_of(lure_item_id(*rod_index)) != 0 {
            return true;
        }
        *rod_index += 1;
        if *rod_index >= ROD_KINDS {
            *rod_index = 0;
        }
    }
    // Unreachable while `owned != 0` and the index is in range; a stale
    // out-of-range index lands here instead of spinning.
    false
}

/// Item id of the **rod** whose ownership the overlay bring-up probes for.
///
/// The rod family (`0xA0..=0xA2`) is not the lure family ([`lure_item_id`],
/// `0x9D..=0x9F`), and the two are tracked by two different persistent words:
/// the lure index is `_DAT_80084450` and the rod stat is `_DAT_80084454`, the
/// value every tension divisor in this module scales by. Reading one gate as
/// the other silently ties the reel feel to the lure the player is holding.
pub fn rod_item_id(rod_index: u32) -> u32 {
    0xa0 + rod_index
}

/// How many probes the bring-up's rod scan makes before giving up.
///
/// Two full cycles of [`ROD_KINDS`]. The extra lap cannot find anything the
/// first one missed - it is retail being belt-and-braces about a persistent
/// index that could start out of range.
pub const ENTRY_ROD_PROBES: u32 = 6;

/// The rod index the fishing overlay's bring-up leaves in `_DAT_80084454`.
///
/// `saved` is the persistent index the player last used; `count_of` reports
/// the live bag count of an item id. The scan keeps `saved` when its rod is
/// held, otherwise steps forward (wrapping at [`ROD_KINDS`]) up to
/// [`ENTRY_ROD_PROBES`] times, and lands on `0` when the player holds no rod
/// at all - so a rodless player fishes with the **first** rod's stat, not with
/// whatever stale index the save carried.
///
/// The wrap and the give-up write are two separate stores in retail, and the
/// try counter advances on every miss including the one that wraps, which is
/// what makes six probes cover two laps rather than six fresh rods.
///
/// PORT: FUN_801CF070 (`0x801cf35c..0x801cf39c`)
///
/// WIRED: `legaia_engine_core::world::World::resolve_fishing_entry_rod` runs it over the
/// party's live bag and writes the result back to the persistent rod cell,
/// which `World::enter_fishing_session` seeds the session's rod from. Every
/// entry reaches it through `SceneHost::enter_fishing_from_overlay` - the
/// mode-24 door warp and both play hosts' debug launchers alike.
pub fn entry_rod_index(saved: u32, mut count_of: impl FnMut(u32) -> i32) -> u32 {
    let mut rod = saved;
    for _ in 0..ENTRY_ROD_PROBES {
        if count_of(rod_item_id(rod)) != 0 {
            return rod;
        }
        rod += 1;
        if rod >= ROD_KINDS {
            rod = 0;
        }
    }
    0
}

/// The 16-entry floor-height LUT the fishing bring-up writes to the
/// scratchpad at `0x1F80035C`, one halfword per floor tier.
///
/// Retail stores `-0x20 * n` descending from `0x1F80037A`, so tier `n` sits
/// `0x20` world units below tier `n - 1`. It is the same LUT the per-cell
/// terrain emitters index by a map cell's low nibble, in the sign convention
/// [`legaia_asset::field_objects::Placement::world_y`] uses (`world_y =
/// -lut[nibble]`), which is why the entries here are positive.
///
/// PORT: FUN_801CF070 (`0x801cf24c..0x801cf268`)
///
/// REPLACED-BY: `legaia_asset::field_objects::Placement::world_y` and
/// `legaia_asset::field_objects::build_walk_heightfield`, which take a
/// scene's floor LUT as a parameter and read it out of the scene's own MAN
/// (`Scene::field_floor_height_lut`). The port has no scratchpad for a scene
/// to publish a LUT into, so the constant is the retail value for the fishing
/// venue rather than a slot anything writes.
pub const FISHING_FLOOR_LUT: [i16; 16] = [
    0x000, 0x020, 0x040, 0x060, 0x080, 0x0a0, 0x0c0, 0x0e0, 0x100, 0x120, 0x140, 0x160, 0x180,
    0x1a0, 0x1c0, 0x1e0,
];

/// Points the bring-up adds to the persistent counter `_DAT_8008444C` when
/// the dev print flag `_DAT_8007B9B0` is set.
///
/// The same `999999` the persistent HUD caps its point row at, added in one
/// store before anything else in the overlay runs - a developer shortcut past
/// the prize counter, not a reachable game rule. Retail ships with the flag
/// clear.
///
/// PORT: FUN_801CF070 (`0x801cf0a8..0x801cf0d0`)
///
/// REPLACED-BY: nothing is owed a port. It is a debug-flag branch, and the
/// engine's equivalent of granting points is editing the save; the constant
/// is here because a reader of `0x8008444C` who sees `999999` in a capture
/// should know a dev build can put it there in one frame.
pub const DEV_ENTRY_POINT_BONUS: i32 = 999_999;

/// One text line of the fishing help panel: which overlay string-table
/// row to draw, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpPanelLine {
    /// Index into the active page's string-pointer table
    /// (page 0 table at overlay VA `0x801D8130`, page 1 at `0x801D8168`).
    pub string_index: u8,
    /// Screen X (the panel's `x` argument, passed through per line).
    pub x: i16,
    /// Screen Y (`y + 13 * index` - the 13 px line pitch).
    pub y: i16,
}

/// Renderer-agnostic layout of the fishing **help panel** - the
/// two-page line-list screen the fishing overlay draws at `0x801D72A0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpPanelLayout {
    pub lines: Vec<HelpPanelLine>,
    /// Footer line position (retail constants `x = 0xE0`, `y = 0xCA`;
    /// the footer string differs per page: overlay VA `0x801CF048` /
    /// `0x801CF050`).
    pub footer: (i16, i16),
    /// The widget-frame emit that closes the draw
    /// (`FUN_8002C69C(x, y, 0x119, 0xC3)`).
    pub frame: (i16, i16, i16, i16),
}

/// PORT: overlay_fishing_801d72a0
///
/// Fishing help-panel layout - the static-extract resolution of the VA
/// `0x801D72A0` open case (see `docs/subsystems/minigame-fishing.md`).
/// The fishing overlay's own bytes at that VA (PROT 0972 file `0x8A88`,
/// base `0x801CE818`) are a clean `(x, y, page)` panel renderer:
///
/// - page 0: 14 lines from the string-pointer table at `0x801D8130`;
/// - page != 0: 15 lines from the sibling table at `0x801D8168`;
/// - both: 13 px line pitch, a per-page footer at `(0xE0, 0xCA)`, a
///   widget-frame emit `FUN_8002C69C(x, y, 0x119, 0xC3)`, and the
///   field-subsystem mode byte `DAT_80073F20 = 0x10` stored on entry.
///
/// The line **strings** are overlay bytes (Sony text) and are not
/// modeled; hosts resolve `string_index` against the user's disc.
///
/// Wired: row 1 of the venue hub menu opens it. The hub opens from the idle
/// shore on Triangle / Select (state `0x0C`'s `& 0x110` test), which is the
/// entry every host lacked - they enter the pond directly, as retail does,
/// and then had no key for the menu. [`crate::fishing_hub::FishingHub::lines`]
/// calls this for [`crate::fishing_hub::HubScreen::Help`] and resolves each
/// `string_index` against the tables [`crate::fishing_hub::FishingHubText`]
/// reads off the disc; the native window's HUD and the browser play page
/// draw it through `World::fishing_hub_lines`, the minigames page through
/// `fishing_hub_json`.
pub fn help_panel_layout(x: i16, y: i16, second_page: bool) -> HelpPanelLayout {
    let count = if second_page { 15 } else { 14 };
    let lines = (0..count)
        .map(|i| HelpPanelLine {
            string_index: i,
            x,
            y: y + 13 * i as i16,
        })
        .collect();
    HelpPanelLayout {
        lines,
        footer: (0xE0, 0xCA),
        frame: (x, y, 0x119, 0xC3),
    }
}

/// Outcome of one [`FishingMenu::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingMenuTick {
    /// SFX request this frame (`sh id, 0x8007B6D8`): `0x37` cancel,
    /// `0x21` cursor move, `0x20` confirm. `None` when no pad edge hit.
    pub sfx: Option<u16>,
    /// New fishing-SM state (`0x801D926C`) when a transition fired:
    /// cancel -> `0x0A`; confirm row 0..4 -> `0x0A` / `0x65` / `0x6E` /
    /// `0x78` / `0xC8`.
    pub next_state: Option<u32>,
    /// Rows 2 / 3 copy the persistent **lure index** `_DAT_80084450` into
    /// `0x801D90DC` on confirm (`0x801D0680..0x801D0690`) - the tackle
    /// screen's cursor, so that screen opens on the equipped lure. The name is
    /// historical: the word copied is not the points bank (`_DAT_8008444C`).
    pub snapshot_points: bool,
    /// Row 4 (leave) clears the scene-load flag `_DAT_8007BC20` and sets
    /// the overlay exit latch `0x801D90CC = 1`.
    pub leave_venue: bool,
}

/// PORT: overlay_fishing_801d0474
///
/// Fishing **main-menu picker** - static extract from PROT 0972 (file
/// `0x1C5C`, base `0x801CE818`). One call per frame:
///
/// - `interactive` (retail `a0 != 0`) gates both the pad handling and
///   the cursor icon; a zero call draws the row text only.
/// - Pad edges (pressed global `0x801D90D8`): `& 0x21` cancel (state
///   `0x0A`, SFX `0x37`); `& 0x1000` up / `& 0x4000` down move the
///   cursor (`0x801D912C`) with SFX `0x21`.
/// - The cursor clamps by **snapping**: `< 0` -> 4, `>= 5` -> 0 (with
///   the ±1 steps that is a 5-row wrap).
/// - Draw: 5 row strings at `x = 0x6C`, `y = 0x58 + 0x10 * row`; the
///   cursor icon (`FUN_8002C488`) at `(0x5B, 0x58 + 0x10 * cursor)`;
///   panel frame via `FUN_801D74B0(0xA0, 0x50, 0x68, 0x50)`.
/// - Confirm (`& 0x44`, SFX `0x20`): jump table over the cursor row ->
///   next SM state (see [`FishingMenuTick::next_state`]); rows 2/3 also
///   seed the tackle screen's cursor with the lure index, row 4 arms the venue
///   exit.
///
/// Wired through [`crate::fishing_hub`]: the pond's idle shore opens the hub on
/// Triangle / Select (retail state `0x0C`), and the hub runs this kernel for
/// state `0x64` on every fishing host - `World::tick_fishing_hub` for the
/// native window and the browser play page, `PondSession::hub_step` directly
/// on the minigames page. Entering the pond directly (every host's launcher
/// and the venue door) was never the gap: retail enters it directly too, and
/// the menu is a key press away from the shore.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishingMenu {
    /// Cursor row (`0x801D912C`).
    pub cursor: i32,
}

/// Row text x / first-row y / row pitch, from the draw calls.
pub const FISHING_MENU_ROW_X: i16 = 0x6C;
pub const FISHING_MENU_ROW_Y0: i16 = 0x58;
pub const FISHING_MENU_ROW_PITCH: i16 = 0x10;
/// Confirm-row -> next-state map (jump table at overlay VA `0x801CEF58`).
pub const FISHING_MENU_ROW_STATES: [u32; 5] = [0x0A, 0x65, 0x6E, 0x78, 0xC8];

impl FishingMenu {
    pub fn tick(&mut self, pad_pressed: u16, interactive: bool) -> FishingMenuTick {
        let mut out = FishingMenuTick {
            sfx: None,
            next_state: None,
            snapshot_points: false,
            leave_venue: false,
        };
        if interactive {
            if pad_pressed & 0x21 != 0 {
                out.next_state = Some(0x0A);
                out.sfx = Some(0x37);
            }
            if pad_pressed & 0x1000 != 0 {
                out.sfx = Some(0x21);
                self.cursor -= 1;
            }
            if pad_pressed & 0x4000 != 0 {
                out.sfx = Some(0x21);
                self.cursor += 1;
            }
        }
        // Snap clamp (retail: bgez / slti 5 pair - not a modulo).
        if self.cursor < 0 {
            self.cursor = 4;
        }
        if self.cursor >= 5 {
            self.cursor = 0;
        }
        if interactive && pad_pressed & 0x44 != 0 {
            out.sfx = Some(0x20);
            let row = self.cursor as usize;
            if row < 5 {
                out.next_state = Some(FISHING_MENU_ROW_STATES[row]);
                out.snapshot_points = row == 2 || row == 3;
                out.leave_venue = row == 4;
            }
        }
        out
    }

    /// The cursor icon position for this frame (interactive draws only).
    pub fn cursor_pos(&self) -> (i16, i16) {
        (
            0x5B,
            FISHING_MENU_ROW_Y0 + FISHING_MENU_ROW_PITCH * self.cursor as i16,
        )
    }
}

/// Outcome of one [`RodLureSelect::tick`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RodLureSelectTick {
    /// SFX request this frame (`sh id, _DAT_8007B6D8`): `0x20` confirm /
    /// equip, `0x22` cannot equip (selected lure not owned), `0x37` cancel,
    /// `0x21` cursor move. `None` when no pad edge acted.
    pub sfx: Option<u16>,
    /// A lure was equipped: the new persistent lure/label index
    /// (`_DAT_80084450 = cursor`, `cursor < 3`). Its consumable is
    /// [`lure_item_id`]`(index)`.
    pub equip_lure: Option<u32>,
    /// A rod was equipped: the new persistent rod-upgrade stat
    /// (`_DAT_80084454 = slot`, the value that scales the tension change).
    pub equip_rod: Option<i32>,
    /// Cancel / confirm-out: the caller jumps the fishing SM to `100`
    /// (`DAT_801D926C = 100`).
    pub leave: bool,
}

/// PORT: FUN_801d0f5c (rod / lure select screen - the input + equip half)
///
/// The fishing overlay's rod/lure select screen (`overlay_fishing_801d0f5c.txt`,
/// PROT 0972). The retail body is input, equip, **and** the row render; this
/// port models the input/equip kernel only (the row list + owned-count highlight
/// is a host draw concern, like the [`FishingMenu`] split).
///
/// Per frame it first counts the **owned rods** among item ids `0xA0..=0xA2`
/// (`owned_rods`), which bounds the cursor. When `interactive` (retail
/// `param_1 != 0`):
///
/// - **accept** (`pad_edge & 0x44` = Cross `0x40` / L1 `0x04`): a lure row
///   (`cursor < 3`) equips its lure - if the lure item `0x9D + cursor` is owned
///   it writes the persistent lure index `_DAT_80084450 = cursor` (SFX `0x20`),
///   otherwise it refuses with SFX `0x22`. A rod row (`cursor >= 3`) walks the
///   three rod slots and equips the `(cursor - 3)`-th **owned** one, writing the
///   persistent rod stat `_DAT_80084454 = slot` (SFX `0x20`) - so unowned slots
///   are skipped, and the visible rod rows are exactly the owned rods.
/// - **cancel** (`pad_edge & 0x21` = Circle `0x20` / L2 `0x01`): SFX `0x37`,
///   [`leave`](RodLureSelectTick::leave).
/// - **move** (`pad_move & 0x1000` up / `& 0x4000` down): step the cursor -/+1,
///   SFX `0x21`. `pad_move` is the retail `DAT_801D90D8` mask, distinct from the
///   `pad_edge` (`_DAT_8007B874`) accept/cancel mask.
///
/// The cursor **snap-wrap** runs every frame regardless of `interactive`
/// (retail): a cursor past `owned_rods + 2` snaps to `0`, a negative cursor snaps
/// to `owned_rods + 2` - the `3` lure rows plus the owned-rod rows.
///
/// Wired as the hub's row 2 ([`crate::fishing_hub::HubScreen::Tackle`], retail
/// state `0x6E`): the two world hosts answer `count_of` off the live bag
/// (`World::tick_fishing_hub`), and an equip lands in the session's persistent
/// lure / rod, which `World::exit_fishing` banks. The minigames page models no
/// tackle inventory - rod and lure are `fishing_pond_start` arguments and its
/// HUD already shows a fixed lure count - so its `count_of` reports every
/// tackle item as held, which lets that page's player equip any of the six.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RodLureSelect {
    /// Cursor row (`DAT_801D90DC`); `0..3` = the three lure rows, `3..` = the
    /// owned rods.
    pub cursor: i32,
}

impl RodLureSelect {
    pub fn tick(
        &mut self,
        pad_edge: u32,
        pad_move: u32,
        interactive: bool,
        mut count_of: impl FnMut(u32) -> i32,
    ) -> RodLureSelectTick {
        let owned_rods: i32 = (0..ROD_KINDS).filter(|k| count_of(0xa0 + k) != 0).count() as i32;
        let mut out = RodLureSelectTick::default();
        if interactive {
            if pad_edge & 0x44 != 0 {
                if self.cursor < 3 {
                    // Lure row: equip only if the paired lure item is owned.
                    if count_of(0x9d + self.cursor as u32) != 0 {
                        out.sfx = Some(0x20);
                        out.equip_lure = Some(self.cursor as u32);
                    } else {
                        out.sfx = Some(0x22);
                    }
                } else {
                    // Rod row: the (cursor - 3)-th owned rod among slots 0..3.
                    let mut remaining = self.cursor - 3;
                    let mut slot = 0i32;
                    while slot < 3 {
                        if count_of(0xa0 + slot as u32) != 0 {
                            if remaining < 1 {
                                out.sfx = Some(0x20);
                                out.equip_rod = Some(slot);
                                break;
                            }
                            remaining -= 1;
                        }
                        slot += 1;
                    }
                }
            }
            if pad_edge & 0x21 != 0 {
                out.sfx = Some(0x37);
                out.leave = true;
            }
            if pad_move & 0x1000 != 0 {
                out.sfx = Some(0x21);
                self.cursor -= 1;
            }
            if pad_move & 0x4000 != 0 {
                out.sfx = Some(0x21);
                self.cursor += 1;
            }
        }
        // Snap-wrap (retail: the bgez / slt pair, run every frame).
        if owned_rods + 2 < self.cursor {
            self.cursor = 0;
        }
        if self.cursor < 0 {
            self.cursor = owned_rods + 2;
        }
        out
    }
}
