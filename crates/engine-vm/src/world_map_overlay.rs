//! World-map dev-menu overlay leaves, ported clean-room from the world-map
//! overlay (`overlay_world_map.bin`, base `0x801C0000`; the bytes are
//! byte-identical to the field/PROT-0897 image at the same VAs, so the dump
//! label is only a resolution hint).
//!
//! This module ports the *simulation / data-model* half of four overlay
//! functions. Every one of the originals is dominated by GPU-packet emission
//! (`FUN_8001AA68` text, `FUN_80034B78` / `FUN_80034E4C` number draws,
//! `FUN_8002C69C` panel fills, `FUN_80036888` MES draws). Those calls are the
//! render seam and are **not** reproduced here - the crate boundary keeps
//! `engine-vm` renderer-free. What is ported is the decoded behaviour each
//! function computes *before* it draws: value clamping, fixed-width decimal
//! formatting, cursor/phase logic and equipment-stat aggregation. The engine
//! host wires the resulting model to a renderer.
//!
//! The four addresses are `FUN_801EAD98` (dev-menu renderer: row model +
//! formatter), `FUN_801ECA08` (panel sizer + list-picker cursor SM),
//! `FUN_801ED710` (battle-records screen data model) and `FUN_801E5B4C`
//! (equipment stat-comparison preview). Each is tagged on the individual items
//! that port it, not on this module: a module-level `PORT:` tag makes the whole
//! file the anchor, and a file is read as live as soon as *any* function in it
//! is reachable - which here would report all four addresses wired on the
//! strength of the one item that is.
//!
//! A fifth leaf of the same overlay, the `0x4C 0xD3` countdown scheduler
//! `FUN_801D2EBC`, lives in [`crate::escape_timer`] - it has an engine caller
//! and so does not belong under this module's wiring status at all.
//!
//! ## Source
//!
//! - `ghidra/scripts/funcs/overlay_world_map_801ead98.txt`
//! - `ghidra/scripts/funcs/801eca08.txt`
//! - `ghidra/scripts/funcs/overlay_world_map_801ed710.txt`
//! - `ghidra/scripts/funcs/overlay_world_map_801e5b4c.txt`
//!
//! ## Wiring status
//!
//! Three of the four addresses are on a host path; the reason differs per
//! address, so each item carries its own note rather than a blanket module
//! disclosure. The per-address summary:
//!
//! - **`FUN_801EAD98` / `FUN_801ECA08`** (row model, formatter, panel sizer,
//!   list-picker cursor + draw gate) are hosted by the engine's dev-menu
//!   screen `legaia_engine_core::dev_menu_host`. That screen carries the row
//!   subset whose backing state the engine owns, and it maps each of its rows
//!   onto retail's own index space ([`DevMenuRow::from_index`]) so the label /
//!   `CLOSED` decision, the row formatter, the panel geometry, the cursor step
//!   and the draw gate are all retail's. The two leaves still without a
//!   consumer are the CAMERA readout (nothing publishes retail's packed
//!   scratchpad camera word) and the 18 rows of the retail list the engine has
//!   no backing state for.
//! - **`FUN_801ED710`** (battle-records data model) is hosted by the native
//!   window's developer-menu Records page, which feeds it the live character
//!   records and the world play clock. Two of its inputs are still absent -
//!   the lifetime battle / escape tallies and the treasure census - so those
//!   fields read zero and the treasure line stays hidden, which is also what
//!   retail draws off a save that never incremented them.
//! - **`FUN_801E5B4C`** (equipment stat-comparison preview) is split. The slot
//!   resolution is live through [`crate::dev_equip_commit::commit_equip`]. The
//!   comparison panel is not: the engine's equip screen is the menu-overlay
//!   flow (`legaia_engine_core::EquipSession`, ported from `FUN_801D9C14` /
//!   `FUN_801D99F0`), which previews by trial-equipping into its own 8-slot
//!   array and re-running its stat aggregator, so nothing calls the shared
//!   5-slot aggregation.

// ---------------------------------------------------------------------------
// FUN_801EAD98 - fixed-width decimal formatter
// ---------------------------------------------------------------------------

/// Zero-padded fixed-width decimal, ported from the digit kernel that
/// `FUN_801EAD98` inlines ~15 times (one copy per numeric menu readout).
///
/// Wired: `legaia_engine_core::dev_menu_host::DevMenuSession::row_value`
/// formats every numeric row through here, reached from
/// `PlayWindowApp::tick_dev_menu`.
/// PORT: FUN_801EAD98 (digit kernel)
///
/// The retail routine seeds a scratch buffer, sets `_DAT_801F2B80 = width`,
/// reduces the magnitude to `width` decimal digits (`value % 10^width`) and
/// emits them most-significant first with leading zeros. A negative value is
/// negated, reduced, and prefixed with `'-'` (the retail code additionally
/// nudges the draw X left by 8 px per sign glyph - that shift is a render
/// concern and lives with the host).
///
/// `width` is clamped to `1..=9` (the retail scratch buffer is 16 bytes and
/// the largest width the callers request is 7).
pub fn format_fixed_decimal(value: i32, width: usize) -> String {
    let width = width.clamp(1, 9);
    let neg = value < 0;
    // Retail negates first, then reduces modulo 10^width.
    let magnitude = (value as i64).unsigned_abs();
    let pow10: u64 = 10u64.pow(width as u32);
    let mut rem = magnitude % pow10;

    // `divisor` starts at 10^(width-1) >= 1 and is only divided *after* it is
    // used, so it is never 0 during a digit computation.
    let mut divisor = pow10 / 10;
    let mut out = String::with_capacity(width + 1);
    if neg {
        out.push('-');
    }
    for _ in 0..width {
        out.push((b'0' + (rem / divisor) as u8) as char);
        rem %= divisor;
        divisor /= 10;
    }
    out
}

// ---------------------------------------------------------------------------
// FUN_801EAD98 - dev-menu row model
// ---------------------------------------------------------------------------

/// The 24 rows of the world-map developer menu, in list order (`local_40`
/// `0..=0x17` in `FUN_801EAD98`). Rows the retail code labels only through a
/// data pointer whose text is not decoded here carry a neutral name.
///
/// Wired: `legaia_engine_core::dev_menu_host::DevMenuRow::retail_row` maps
/// each engine row onto its retail index, and `DevMenuSession::row_label`
/// asks the resulting row kind, once per row per frame, whether it draws its
/// label or `CLOSED`. The chain to a host root is
/// `PlayWindowApp::handle_redraw` -> `tick_dev_menu` ->
/// `build_dev_menu_draws` -> `row_label` -> `row_is_closed` -> `retail_row`.
/// The 18 rows the engine has no backing state for are modelled here and not
/// listed there.
/// PORT: FUN_801EAD98 (row dispatch)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevMenuRow {
    MapChange,       // 0x00 - "MAP CHANGE"  (or CLOSED, gate _DAT_8007B868)
    CardOption,      // 0x01 - "CARD OPTION" (or CLOSED)
    PlayerStatus,    // 0x02
    Camera,          // 0x03 - reads _DAT_1F800384
    Encount,         // 0x04 - reads DAT_8007B5F8
    OtherSettings,   // 0x05
    BgmCall,         // 0x06 - reads _DAT_801F2E90
    Debug,           // 0x07 - reads _DAT_8007B6D0 (signed)
    RecoverHpMp,     // 0x08
    PowerfulPlayers, // 0x09
    GetItem,         // 0x0A - reads _DAT_801F2E8C
    GetAllItems,     // 0x0B
    EquipVahn,       // 0x0C - default sub-dispatch iVar5==0
    EquipNoa,        // 0x0D - default sub-dispatch iVar5==1
    EquipGala,       // 0x0E - default sub-dispatch iVar5==2
    PlayerParam,     // 0x0F
    PlayerChar,      // 0x10 - reads _DAT_8007B8F8
    EventFlag,       // 0x11 - reads _DAT_801F2AA0 (flag grid)
    Coord0,          // 0x12 - reads DAT_1F8003E8
    Coord1,          // 0x13 - reads DAT_1F8003E9
    Coord2,          // 0x14 - reads DAT_1F8003EA
    Coord3,          // 0x15 - reads DAT_1F8003EB
    PlayPos,         // 0x16 - reads _DAT_8007C364+0x14 / +0x18
    ResetErrors,     // 0x17
}

impl DevMenuRow {
    /// Map a list index to its row kind. Indices `>= 0x18` are out of the
    /// retail bounds check (`0x17 < local_40` short-circuits the switch).
    ///
    /// Wired through `legaia_engine_core::dev_menu_host::DevMenuRow::retail_row`,
    /// which the row list's label builder calls every frame - see
    /// [`DevMenuRow`] for the chain to the host root.
    /// PORT: FUN_801EAD98 (`switch(local_40)` + the default 0xC/0xD/0xE arm)
    pub fn from_index(index: u32) -> Option<DevMenuRow> {
        use DevMenuRow::*;
        Some(match index {
            0x00 => MapChange,
            0x01 => CardOption,
            0x02 => PlayerStatus,
            0x03 => Camera,
            0x04 => Encount,
            0x05 => OtherSettings,
            0x06 => BgmCall,
            0x07 => Debug,
            0x08 => RecoverHpMp,
            0x09 => PowerfulPlayers,
            0x0A => GetItem,
            0x0B => GetAllItems,
            // The retail switch has no case for 0xC/0xD/0xE; they fall to the
            // `default:` arm which sub-dispatches on `local_40 - 0xC`.
            0x0C => EquipVahn,
            0x0D => EquipNoa,
            0x0E => EquipGala,
            0x0F => PlayerParam,
            0x10 => PlayerChar,
            0x11 => EventFlag,
            0x12 => Coord0,
            0x13 => Coord1,
            0x14 => Coord2,
            0x15 => Coord3,
            0x16 => PlayPos,
            0x17 => ResetErrors,
            _ => return None,
        })
    }

    /// Whether this row renders as "CLOSED" instead of its label. Only the
    /// `MAP CHANGE` and `CARD OPTION` rows are gated, by `_DAT_8007B868 != 0`.
    ///
    /// The polarity is the disassembly's: case 0 is `lw v0,-0x4798(0x8008)`
    /// then `beq v0,zero,0x801EAE5C`, so the **zero** leg loads the
    /// `MAP CHANGE` string and the fall-through loads `CLOSED`; case 1 repeats
    /// it at `0x801EB310` for `CARD OPTION`.
    ///
    /// Wired: `legaia_engine_core::dev_menu_host::DevMenuSession::row_is_closed`
    /// gates the `MAP CHANGE` row's label through here, and its own caller
    /// `DevMenuSession::row_label` is what the row list draws from every
    /// frame - see [`DevMenuRow`] for the chain to the host root.
    /// PORT: FUN_801EAD98 (cases 0 and 1)
    pub fn is_closed(self, gate_b868: u32) -> bool {
        matches!(self, DevMenuRow::MapChange | DevMenuRow::CardOption) && gate_b868 != 0
    }
}

/// Decode the CAMERA row's two displayed angles from the packed camera word
/// `_DAT_1F800384`. Returns `None` when the word is the sentinel
/// `0x7F7F0000`, in which case retail draws the fixed string `"000 000"`.
///
/// Each angle is the average of a low and a high byte lane:
/// `pitch = ((w & 0xFF) + ((w >> 16) & 0xFF)) / 2`,
/// `yaw   = (((w >> 8) & 0xFF) + ((w >> 24) & 0xFF)) / 2`.
///
/// NOT WIRED: the dev-menu host has no CAMERA row, because nothing in the
/// engine publishes retail's packed scratchpad camera word `_DAT_1F800384` -
/// `WorldMapController` keeps azimuth and zoom as separate scalars, not as
/// the four byte lanes this averages. Wiring needs that word published first.
/// PORT: FUN_801EAD98 (case 3)
pub fn decode_camera_readout(cam_word: u32) -> Option<(i32, i32)> {
    if cam_word == 0x7F7F_0000 {
        return None;
    }
    let pitch = ((cam_word & 0xFF) + ((cam_word >> 16) & 0xFF)) as i32 >> 1;
    let yaw = (((cam_word >> 8) & 0xFF) + ((cam_word >> 24) & 0xFF)) as i32 >> 1;
    Some((pitch, yaw))
}

// ---------------------------------------------------------------------------
// FUN_801ECA08 - panel sizer + list-picker cursor state machine
// ---------------------------------------------------------------------------

/// Sizing for the developer-menu panel descriptor at
/// `0x801F2B98 + col_idx*0x1C`.
///
/// REF: FUN_801ECA08 (panel-sizing prologue) - the computation is tagged on
/// [`panel_geometry`]; this is only its return type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelGeometry {
    /// Panel top Y (`desc[+0x0A]`), bottom-anchored to the 208px viewport.
    pub y: i16,
    /// Panel height in pixels (`desc[+0x0E]`).
    pub height: i16,
}

/// Compute panel geometry from the inclusive row range `row_start..=row_end`.
/// `rows = row_end - row_start + 1`; `height = rows*8`; `y = 0xD0 - rows*8`.
///
/// Wired: `legaia_engine_core::dev_menu_host::DevMenuSession::tick` sizes the
/// row list's panel from its own row span every frame.
/// PORT: FUN_801ECA08
pub fn panel_geometry(row_start: i32, row_end: i32) -> PanelGeometry {
    let rows = row_end - row_start + 1;
    PanelGeometry {
        y: (0xD0 - rows * 8) as i16,
        height: (rows * 8) as i16,
    }
}

/// SFX ids the list-picker fires (`FUN_80035B50` argument).
pub const SFX_CURSOR_MOVE: u32 = 0x21;
pub const SFX_CONFIRM: u32 = 0x37;
pub const SFX_CANCEL: u32 = 0x36;

/// One step of the vertical cursor over `row_start..=row_end`. The wrap is a
/// **swap, not a clamp**: stepping below `row_start` jumps to `row_end` and
/// above `row_end` jumps back to `row_start`. Returns the new cursor row and
/// whether a move SFX (`SFX_CURSOR_MOVE`) should fire.
///
/// `up`/`down` are the D-pad edges (`_DAT_8007BB84 & 0x1000` / `& 0x4000`).
///
/// Named `dev_menu_cursor_step` rather than `cursor_step` on purpose: a free
/// function's name is the whole of its identity to the reachability pass
/// (`docs/tooling/stale-not-wired-triage.md`), and
/// `legaia_engine_core::baka_cabinet::cursor_step` is a live free function of
/// that name, so every call to *that* one used to read as a call to this one.
/// Keep the names distinct.
///
/// Wired: [`legaia_engine_core::dev_menu_host::DevMenuSession::step_row`]
/// steps the dev-menu row list through here, reached from
/// `PlayWindowApp::tick_dev_menu`.
///
/// PORT: FUN_801ECA08 (phase-1 cursor block)
pub fn dev_menu_cursor_step(
    cursor: i32,
    row_start: i32,
    row_end: i32,
    up: bool,
    down: bool,
) -> (i32, bool) {
    let mut c = cursor;
    let mut moved = false;
    if up {
        c -= 1;
        moved = true;
    }
    if down {
        c += 1;
        moved = true;
    }
    // Retail applies both wrap tests unconditionally after the move.
    if c < row_start {
        c = row_end;
    }
    if c > row_end {
        c = row_start;
    }
    (c, moved)
}

/// The list-picker phase (`ctx[+0x54]`), 5-way.
///
/// Wired: `legaia_engine_core::dev_menu_host::DevMenuSession::list_phase`
/// tracks the engine dev menu's page in this phase space, which is what the
/// retail draw gate below is asked against.
/// PORT: FUN_801ECA08 (`switch(ctx[+0x54])`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListPickerPhase {
    /// Seed cursor = row_start, open panel, then falls through into `Active`.
    #[default]
    Open = 0,
    /// Cursor movement + confirm/cancel input.
    Active = 1,
    /// Confirm settle.
    ConfirmSettle = 2,
    /// Cancel unwind (`FUN_801EA9B0`).
    CancelUnwind = 3,
    /// Teardown; restores saved selection, resets phase to 0.
    Teardown = 4,
}

impl ListPickerPhase {
    pub fn from_i16(v: i16) -> Option<ListPickerPhase> {
        Some(match v {
            0 => ListPickerPhase::Open,
            1 => ListPickerPhase::Active,
            2 => ListPickerPhase::ConfirmSettle,
            3 => ListPickerPhase::CancelUnwind,
            4 => ListPickerPhase::Teardown,
            _ => return None,
        })
    }
}

/// Whether the menu list body (`FUN_801EAD98`) is drawn this frame. Retail
/// computes `iVar6 = phase * gate` after the phase switch and draws when the
/// product is `1` or `3`:
///
/// - phase `Active` (1) with `gate = 1` (input allowed) -> draws;
/// - phase `CancelUnwind` (3) with `gate` = the unwind dispatcher's return
///   -> draws;
/// - every other product (0, 2, 4, ...) -> no draw.
///
/// The unwind dispatcher is `FUN_801EA9B0`, and it returns `1`
/// unconditionally - `s1` is loaded with `1` in the delay slot of its bound
/// check and every arm, including the out-of-range one, exits through
/// `move v0,s1` (see [`crate::world_map_panel::dev_menu_action`]). So phase
/// `CancelUnwind` always draws; that gate never suppresses it. The argument
/// stays because the phase-1 leg's gate is the caller's own input-allowed
/// flag, which does vary.
///
/// Wired: `legaia_engine_core::dev_menu_host::DevMenuSession::list_visible`
/// is this gate's output, recomputed every frame.
/// PORT: FUN_801ECA08 (`mult s2,s3` draw gate)
pub fn list_body_draws(phase: i16, gate: i32) -> bool {
    let product = phase as i32 * gate;
    product == 1 || product == 3
}

// ---------------------------------------------------------------------------
// FUN_801ED710 - battle-records screen data model
// ---------------------------------------------------------------------------

/// Per-character stat block read from the records stats record at
/// `0x80084140 + n*0x414`.
///
/// REF: FUN_801ED710 (per-character loops) - the clamping is tagged on
/// [`records_screen`]; this is its per-character input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CharRecordStats {
    /// `+0x6B4`, clamped to 999 for display.
    pub max_hits: u32,
    /// `+0x6B0`, clamped to 9_999_999 for display.
    pub max_damage: u32,
    /// `+0x660`, clamped to 999 for display.
    pub knockouts: u32,
    /// `+0x664`, clamped to 999_999 for display.
    pub monsters_defeated: u32,
    /// `+0x74D` (byte) - number of Hyper Arts learned.
    pub hyper_arts: u8,
    /// `+0x704` (byte) - number of magics learned.
    pub magic: u8,
}

/// The decoded, display-ready records screen. All caps + the play-time
/// decomposition are reproduced from `FUN_801ED710`.
///
/// REF: FUN_801ED710 - the model builder is tagged on [`records_screen`];
/// this is its return type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordsScreen {
    /// No. of Battles (`_DAT_800846A4`), capped 99999.
    pub battles: u32,
    /// No. of Escapes (`_DAT_800846A8`), capped 99999.
    pub escapes: u32,
    /// Play time H:MM:SS, hours capped at 99 (then MM=SS=59).
    pub play_hours: i32,
    pub play_minutes: i32,
    pub play_seconds: i32,
    /// Per-character max-hits, clamped 999.
    pub max_hits: [u32; 3],
    /// Per-character max-damage, clamped 9_999_999.
    pub max_damage: [u32; 3],
    /// Per-character knockouts, clamped 999.
    pub knockouts: [u32; 3],
    /// Per-character monsters-defeated, clamped 999_999.
    pub monsters_defeated: [u32; 3],
    /// Per-character Hyper-Arts count.
    pub hyper_arts: [u8; 3],
    /// Per-character magic count.
    pub magic: [u8; 3],
    /// Treasure percentage `found*100/total` (0 when `total <= 0`).
    pub treasure_percent: i32,
    /// Treasure fractional part `(found*10000/total) - percent*100`.
    pub treasure_fraction: i32,
    /// Whether the treasure line is drawn at all (`0 < total`).
    pub treasure_shown: bool,
}

/// Decompose a `1/60 s` play-time frame counter (`_DAT_800845DC`) into
/// H:MM:SS with the retail 99h clamp.
///
/// Wired: reached from [`records_screen`], whose host is the native window's
/// developer-menu Records page.
/// PORT: FUN_801ED710 (play-time block)
pub fn decompose_play_time(frames: u32) -> (i32, i32, i32) {
    let secs = (frames / 60) as i32;
    let mut seconds = secs % 60;
    let total_minutes = secs / 60;
    let mut hours = total_minutes / 60;
    let mut minutes = total_minutes % 60;
    if hours > 99 {
        hours = 99;
        minutes = 59;
        seconds = 59;
    }
    (hours, minutes, seconds)
}

/// Build the records-screen model from the raw runtime globals.
///
/// Wired: `legaia-engine play-window`'s developer-menu Records page builds
/// this model from the live character records and the world play clock -
/// `PlayWindowApp::build_dev_records_draws` -> `dev_records_model` -> here.
/// PORT: FUN_801ED710
pub fn records_screen(
    battles: u32,
    escapes: u32,
    play_frames: u32,
    chars: &[CharRecordStats; 3],
    treasure_found: i32,
    treasure_total: i32,
) -> RecordsScreen {
    let (play_hours, play_minutes, play_seconds) = decompose_play_time(play_frames);

    let mut out = RecordsScreen {
        battles: battles.min(99_999),
        escapes: escapes.min(99_999),
        play_hours,
        play_minutes,
        play_seconds,
        ..Default::default()
    };
    for (i, c) in chars.iter().enumerate() {
        out.max_hits[i] = c.max_hits.min(999);
        out.max_damage[i] = c.max_damage.min(9_999_999);
        out.knockouts[i] = c.knockouts.min(999);
        out.monsters_defeated[i] = c.monsters_defeated.min(999_999);
        out.hyper_arts[i] = c.hyper_arts;
        out.magic[i] = c.magic;
    }
    if treasure_total > 0 {
        out.treasure_shown = true;
        let percent = treasure_found * 100 / treasure_total;
        out.treasure_percent = percent;
        out.treasure_fraction = treasure_found * 10_000 / treasure_total - percent * 100;
    }
    out
}

// ---------------------------------------------------------------------------
// FUN_801E5B4C - equipment stat-comparison preview
// ---------------------------------------------------------------------------

/// The five stat bonuses an equipment record contributes, read from the
/// stride-8 equipment table `DAT_80074F68` bytes `+0..+4`.
pub type EquipStatBonus = [u8; 5];

/// Direction arrow the preview shows for one stat when a candidate item is
/// pending: candidate vs current.
///
/// REF: FUN_801E5B4C (arrow-glyph selection) - the comparison is tagged on
/// [`stat_deltas`]; this is its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatDelta {
    /// Candidate raises the stat. Retail draws glyph
    /// [`EQUIP_GLYPH_HIGHER`] and then selects ink [`EQUIP_INK_HIGHER`].
    Up,
    /// Candidate lowers it: glyph [`EQUIP_GLYPH_LOWER`], ink
    /// [`EQUIP_INK_LOWER`].
    Down,
    /// No change - neither `slt` fires, so nothing is drawn and the ink is
    /// left where the label put it.
    Same,
}

/// Sum the five equipment stat bonuses across a character's five equip slots.
///
/// `slots[i]` is the equip id stored at `char[+0x75E + i]`. `item_stat_index`
/// resolves an equip id to its stat-table index (item record `+1`, stride-12
/// table `DAT_80074368`). `equip_bonus` resolves that index to the five
/// bonus bytes (stride-8 table `DAT_80074F68`). Slot id `0` contributes
/// nothing but is still looked up in retail; callers pass a resolver that
/// returns zeroes for id `0`.
///
/// The consumer is **not** the pause-menu equip screen. `FUN_801E5B4C` has one
/// `jal` in the corpus, `0x801F1778`, inside `FUN_801F16C0` - the hub panel's
/// stacked per-entry label list, which draws a label and then calls this as the
/// entry's **sub-draw**. The pause-menu preview is a separate flow that
/// trial-equips into its own 8-slot array and re-runs
/// `legaia_engine_core::battle_stats`'s aggregator, so it never reaches here.
///
/// Wired: [`equip_stat_panel`] is the whole sub-draw and runs this twice - once
/// over the live loadout, once over the trial one - and
/// [`crate::baka_hub_actors::entry_list`] calls it where the `jal` sits. The
/// live path is `World::tick_submode_screen` -> `HubPainter::EntryList` ->
/// `entry_list`.
/// PORT: FUN_801E5B4C (aggregation loops)
/// REF: FUN_801F16C0 - the hub entry list that calls this as a sub-draw.
pub fn aggregate_slot_stats(
    slots: &[u8; 5],
    item_stat_index: impl Fn(u8) -> u8,
    equip_bonus: impl Fn(u8) -> EquipStatBonus,
) -> [i32; 5] {
    let mut totals = [0i32; 5];
    for &equip_id in slots {
        let stat_idx = item_stat_index(equip_id);
        let bonus = equip_bonus(stat_idx);
        for (t, b) in totals.iter_mut().zip(bonus.iter()) {
            *t += *b as i32;
        }
    }
    totals
}

/// Can character `char_idx` equip an item whose equip-table record byte `+6`
/// is `mask`? Retail spells out only chars 0/1/2 (Vahn/Noa/Gala) as
/// `mask & (1 << char_idx)`; for any other character none of the guard arms
/// match, so the item is treated as equippable.
///
/// Wired: the candidate arm of [`equip_stat_panel`], which draws the reject
/// line instead of the comparison columns when this returns `false`. The
/// engine's own equip screen is a different screen and gates with
/// `legaia_engine_core::equipment`'s mask check, not with this.
/// PORT: FUN_801E5B4C (equippability guard)
pub fn can_equip(mask: u8, char_idx: u32) -> bool {
    if char_idx < 3 {
        (mask >> char_idx) & 1 != 0
    } else {
        true
    }
}

/// Resolve the destination equip slot for a candidate whose equip-table
/// record byte `+7` is `slot_bits`. `(slot_bits & 0x60) >> 5` selects:
/// `0 -> slot 0`, `1 -> slot 1`, `2 -> per-character weapon slot`
/// (`weapon_slot_table[char_idx]`, from `0x8007B42C`), `3 -> slot 4`.
///
/// Wired twice. [`equip_stat_panel`] resolves the trial-equip destination
/// through here, which is retail's own use; and
/// [`crate::dev_equip_commit::commit_equip`] resolves a real commit through it,
/// with the caller chain
/// `legaia_engine_core::dev_menu_host::DevMenuSession::commit_equip_row` ->
/// `PlayWindowApp::tick_dev_menu` -> `handle_redraw`.
///
/// PORT: FUN_801E5B4C (slot resolution)
pub fn resolve_equip_slot(slot_bits: u8, char_idx: usize, weapon_slot_table: &[i16]) -> usize {
    let sel = ((slot_bits & 0x60) >> 5) as usize;
    match sel {
        2 => weapon_slot_table.get(char_idx).copied().unwrap_or(0) as usize,
        3 => 4,
        other => other, // 0 or 1
    }
}

/// Per-stat direction arrows comparing a candidate loadout's stat totals to
/// the current totals. Retail shows the arrow next to a stat when the
/// candidate total differs (`candidate > current -> Up`, `< -> Down`).
///
/// Wired: [`equip_stat_panel`] runs this over the two aggregations and turns
/// each verdict into the arrow glyph and the ink the candidate column beside it
/// is drawn under.
/// PORT: FUN_801E5B4C (LAB_801E5FB0 comparison block)
pub fn stat_deltas(current: &[i32; 5], candidate: &[i32; 5]) -> [StatDelta; 5] {
    let mut out = [StatDelta::Same; 5];
    for i in 0..5 {
        out[i] = match candidate[i].cmp(&current[i]) {
            std::cmp::Ordering::Greater => StatDelta::Up,
            std::cmp::Ordering::Less => StatDelta::Down,
            std::cmp::Ordering::Equal => StatDelta::Same,
        };
    }
    out
}

// ---------------------------------------------------------------------------
// FUN_801E5B4C - the whole sub-panel
// ---------------------------------------------------------------------------

/// Rodata pointer VAs of the three stat labels, in draw order.
///
/// The pointers are loaded from consecutive words at `0x801F29CC` /
/// `0x801F29D0` / `0x801F29D4`; the strings themselves are Sony bytes and are
/// not reproduced.
pub const EQUIP_LABEL_VAS: [u32; 3] = [0x801F_29CC, 0x801F_29D0, 0x801F_29D4];

/// Rodata pointer VA of the "this character cannot equip that" line
/// (`0x801F29C8`), the reject arm's only draw.
pub const EQUIP_REJECT_VA: u32 = 0x801F_29C8;

/// Which of the five aggregated bonus slots each drawn row reads.
///
/// Only three of the five are drawn: the equipment record's `+1` / `+2` / `+3`
/// bytes, which [`equipment-table.md`] pins as ATK / UDF / LDF. The `+0` (INT)
/// and `+4` (SPD) accumulators are summed and never painted.
///
/// [`equipment-table.md`]: ../../../docs/formats/equipment-table.md
pub const EQUIP_ROW_BONUS_SLOTS: [usize; 3] = [1, 2, 3];

/// Character-record offsets of the base stat each row adds its bonus total to.
///
/// Retail reads `0x80084140 + char*0x414 + 0x6DA/0x6DC/0x6DE`; rebasing by the
/// `0x5C8` block-to-record distance gives `+0x112` / `+0x114` / `+0x116`, which
/// `legaia_save` names `atk` / `udf` / `ldf`.
pub const EQUIP_ROW_RECORD_OFFSETS: [usize; 3] = [0x112, 0x114, 0x116];

/// Text ink (`_DAT_8007B454`) the panel opens on and restores to.
pub const EQUIP_INK_NORMAL: i32 = 7;
/// Ink the reject line draws under.
pub const EQUIP_INK_REJECT: i32 = 9;
/// Ink left for the candidate column when the candidate total is **lower**.
pub const EQUIP_INK_LOWER: i32 = 1;
/// Ink left for the candidate column when the candidate total is **higher**.
pub const EQUIP_INK_HIGHER: i32 = 6;

/// Glyph the arrow emitter draws when the candidate total is lower.
pub const EQUIP_GLYPH_LOWER: i32 = 5;
/// Glyph it draws when the candidate total is higher.
pub const EQUIP_GLYPH_HIGHER: i32 = 4;

/// Column offsets from the caller's pen: label, current value, arrow,
/// candidate value.
pub const EQUIP_COL_LABEL: i16 = 0x08;
/// Current-total column.
pub const EQUIP_COL_CURRENT: i16 = 0x38;
/// Arrow column.
pub const EQUIP_COL_ARROW: i16 = 0x50;
/// Candidate-total column.
pub const EQUIP_COL_CANDIDATE: i16 = 0x58;
/// The reject line's own offsets, which are not a row of the grid.
pub const EQUIP_REJECT_OFFSET: (i16, i16) = (0x0C, 0x08);

/// Row pitch with the op-`0x49` descriptor clear / set (`_DAT_8007B450`).
pub const EQUIP_ROW_PITCH: [i16; 2] = [0x0E, 0x0D];

/// Digits every value column prints (`FUN_80034B78(value, 3, x, y)`).
pub const EQUIP_VALUE_DIGITS: i32 = 3;

/// Menu-mode words (`_DAT_8007BB9C`) that take the candidate from the
/// inventory list at `0x80084140 + 0x1818`, indexed by the shared cursor.
pub const EQUIP_MODE_INVENTORY: [u32; 3] = [0x1000, 0x6000, 0x9000];
/// The mode that uses the shared cursor **as** the candidate id.
pub const EQUIP_MODE_DIRECT: u32 = 0x3000;
/// The mode that compares against an empty loadout when the cursor is `1`.
pub const EQUIP_MODE_BLANK: u32 = 0x4000;

/// The item-property table row `FUN_801E5B4C` reads for a candidate id
/// (`DAT_80074368`, stride `0x0C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemProps {
    /// `+0` - `1` = equipment, `2` = the class that draws the plain block.
    pub kind: u8,
    /// `+1` - index into the stride-8 equipment table.
    pub stat_index: u8,
}

/// The equipment-table row (`DAT_80074F68`, stride `8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EquipProps {
    /// `+0..+4` - the five stat bonuses.
    pub bonuses: EquipStatBonus,
    /// `+6` - the equip-character mask.
    pub char_mask: u8,
    /// `+7` - the slot-type bits.
    pub slot_bits: u8,
}

/// What the caller publishes before the sub-draw plus the record fields it
/// reads back out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EquipPanelInput {
    /// The caller's pen, `param_1[+0x0A]` / `param_1[+0x0C]`.
    pub x: i16,
    /// Pen y.
    pub y: i16,
    /// `_DAT_8007B469`, the character the caller published for this entry.
    pub char_index: usize,
    /// `char[+0x75E..+0x763]` - the five equip slot ids.
    pub slots: [u8; 5],
    /// The three base stats at [`EQUIP_ROW_BONUS_SLOTS`]' record offsets.
    pub base_stats: [i32; 3],
    /// `_DAT_8007BB9C`, the menu mode word.
    pub mode: u32,
    /// `_DAT_8007BB88`, the shared list cursor.
    pub cursor: i32,
    /// The inventory id list at `0x80084140 + 0x1818`, stride `2`, that the
    /// three inventory modes index with the cursor.
    pub inventory: Vec<u8>,
    /// `0x8007B42C` - the per-character weapon slot table.
    pub weapon_slots: Vec<i16>,
    /// `_DAT_8007B450` non-zero, which tightens the row pitch by one pixel.
    pub tight_rows: bool,
}

/// One draw the sub-panel emits, named for the retail emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipPanelDraw {
    /// `FUN_80036888(*label_va, 0, 0, x, y)` under `ink`.
    Label {
        label_va: u32,
        x: i16,
        y: i16,
        ink: i32,
    },
    /// `FUN_80034B78(value, 3, x, y)` under `ink`.
    Value {
        value: i32,
        x: i16,
        y: i16,
        ink: i32,
    },
    /// `FUN_8003C1F8(glyph, x, y)` under `ink`.
    Arrow {
        glyph: i32,
        x: i16,
        y: i16,
        ink: i32,
    },
}

/// Which candidate loadout the mode word and cursor select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipCandidate {
    /// No comparison column: the three inventory modes, `0x3000` and `0x4000`
    /// all fall through to the plain block for the cursor values that do not
    /// name an item.
    None,
    /// Mode `0x4000` with cursor `1`: compare against an all-zero candidate.
    Empty,
    /// An item id to trial-equip.
    Item(u8),
}

/// Decode the candidate the mode word and cursor select.
///
/// PORT: FUN_801E5B4C (`0x801E5C7C..0x801E5D18` mode ladder)
pub fn equip_candidate(mode: u32, cursor: i32, inventory: &[u8]) -> EquipCandidate {
    if EQUIP_MODE_INVENTORY.contains(&mode) {
        let id = usize::try_from(cursor)
            .ok()
            .and_then(|i| inventory.get(i).copied())
            .unwrap_or(0);
        return EquipCandidate::Item(id);
    }
    if mode == EQUIP_MODE_DIRECT {
        return EquipCandidate::Item(cursor as u8);
    }
    if mode == EQUIP_MODE_BLANK && cursor == 1 {
        return EquipCandidate::Empty;
    }
    EquipCandidate::None
}

/// The whole of `FUN_801E5B4C`: the per-entry equipment stat sub-panel the hub
/// entry list draws under each label.
///
/// Three rows, each `label | current | (arrow) | candidate`, over the ATK /
/// UDF / LDF accumulators. The comparison columns only appear when the mode
/// word names a candidate; otherwise retail draws the same three rows without
/// them, which is the arm the hub's own list mode takes.
///
/// The ink is a **store to `_DAT_8007B454` between draws**, not a per-draw
/// argument, and the arrow's store lands *after* its own `jal` - so the ink an
/// arrow selects colours the candidate column beside it, not the arrow. That
/// is reproduced here by carrying the ink forward.
///
/// PORT: FUN_801E5B4C
///
/// Wired: [`crate::baka_hub_actors::entry_list`] calls this where retail's
/// `jal 0x801F1778` sits, once per drawn entry. Its own caller chain is
/// `World::tick_submode_screen` -> `HubPainter::EntryList`.
pub fn equip_stat_panel(
    input: &EquipPanelInput,
    item_props: impl Fn(u8) -> ItemProps,
    equip_props: impl Fn(u8) -> EquipProps,
) -> Vec<EquipPanelDraw> {
    let current = aggregate_slot_stats(
        &input.slots,
        |id| item_props(id).stat_index,
        |idx| equip_props(idx).bonuses,
    );

    let candidate = match equip_candidate(input.mode, input.cursor, &input.inventory) {
        EquipCandidate::None => None,
        EquipCandidate::Empty => Some([0i32; 5]),
        EquipCandidate::Item(id) => {
            let props = item_props(id);
            match props.kind {
                1 => {
                    let equip = equip_props(props.stat_index);
                    if !can_equip(equip.char_mask, input.char_index as u32) {
                        // The reject arm draws one line and returns.
                        let (dx, dy) = EQUIP_REJECT_OFFSET;
                        return vec![EquipPanelDraw::Label {
                            label_va: EQUIP_REJECT_VA,
                            x: input.x + dx,
                            y: input.y + dy,
                            ink: EQUIP_INK_REJECT,
                        }];
                    }
                    // Trial-equip into the resolved slot, aggregate, restore.
                    let slot =
                        resolve_equip_slot(equip.slot_bits, input.char_index, &input.weapon_slots);
                    let mut trial = input.slots;
                    if let Some(cell) = trial.get_mut(slot) {
                        *cell = id;
                    }
                    Some(aggregate_slot_stats(
                        &trial,
                        |i| item_props(i).stat_index,
                        |idx| equip_props(idx).bonuses,
                    ))
                }
                // Kind 2 draws the plain block; every other kind draws nothing
                // at all (`bne a0,v0,0x801E63E0` at `0x801E6280`).
                2 => None,
                _ => return Vec::new(),
            }
        }
    };

    let deltas = candidate.map(|c| stat_deltas(&current, &c));
    let pitch = EQUIP_ROW_PITCH[usize::from(input.tight_rows)];
    let mut out = Vec::new();
    let mut y = input.y;
    for (row, &bonus_slot) in EQUIP_ROW_BONUS_SLOTS.iter().enumerate() {
        // The ink is reasserted per row: retail stores `7` before each label.
        let mut ink = EQUIP_INK_NORMAL;
        out.push(EquipPanelDraw::Label {
            label_va: EQUIP_LABEL_VAS[row],
            x: input.x + EQUIP_COL_LABEL,
            y,
            ink,
        });
        let base = input.base_stats[row];
        out.push(EquipPanelDraw::Value {
            value: base + current[bonus_slot],
            x: input.x + EQUIP_COL_CURRENT,
            y,
            ink,
        });
        if let (Some(cand), Some(d)) = (candidate, deltas) {
            match d[bonus_slot] {
                StatDelta::Down => {
                    out.push(EquipPanelDraw::Arrow {
                        glyph: EQUIP_GLYPH_LOWER,
                        x: input.x + EQUIP_COL_ARROW,
                        y,
                        ink,
                    });
                    ink = EQUIP_INK_LOWER;
                }
                StatDelta::Up => {
                    out.push(EquipPanelDraw::Arrow {
                        glyph: EQUIP_GLYPH_HIGHER,
                        x: input.x + EQUIP_COL_ARROW,
                        y,
                        ink,
                    });
                    ink = EQUIP_INK_HIGHER;
                }
                StatDelta::Same => {}
            }
            out.push(EquipPanelDraw::Value {
                value: base + cand[bonus_slot],
                x: input.x + EQUIP_COL_CANDIDATE,
                y,
                ink,
            });
        }
        // The last row does not advance - the epilogue follows it directly.
        if row + 1 < EQUIP_ROW_BONUS_SLOTS.len() {
            y += pitch;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FUN_801E5B4C sub-panel ------------------------------------------

    fn panel_input() -> EquipPanelInput {
        EquipPanelInput {
            x: 0x20,
            y: 0x30,
            char_index: 1,
            slots: [1, 0, 0, 0, 0],
            base_stats: [10, 20, 30],
            ..EquipPanelInput::default()
        }
    }

    /// Item 1 is an equippable with bonus row 1; item 2 is `kind 2`; item 3 is
    /// an equippable Noa cannot wear; item 9 is a kind the panel ignores.
    fn item(id: u8) -> ItemProps {
        match id {
            1 => ItemProps {
                kind: 1,
                stat_index: 1,
            },
            2 => ItemProps {
                kind: 2,
                stat_index: 0,
            },
            3 => ItemProps {
                kind: 1,
                stat_index: 3,
            },
            9 => ItemProps {
                kind: 7,
                stat_index: 0,
            },
            _ => ItemProps::default(),
        }
    }

    fn equip(idx: u8) -> EquipProps {
        match idx {
            1 => EquipProps {
                bonuses: [0, 5, 0, 0, 0],
                char_mask: 7,
                slot_bits: 0x40,
            },
            3 => EquipProps {
                bonuses: [0, 9, 0, 0, 0],
                char_mask: 1,
                slot_bits: 0x40,
            },
            _ => EquipProps::default(),
        }
    }

    #[test]
    fn the_mode_ladder_picks_the_candidate_source() {
        let inv = [0u8, 4, 8];
        for m in EQUIP_MODE_INVENTORY {
            assert_eq!(equip_candidate(m, 2, &inv), EquipCandidate::Item(8));
        }
        // `0x3000` uses the cursor as the id itself.
        assert_eq!(
            equip_candidate(EQUIP_MODE_DIRECT, 2, &inv),
            EquipCandidate::Item(2)
        );
        // `0x4000` compares against nothing, and only on cursor 1.
        assert_eq!(
            equip_candidate(EQUIP_MODE_BLANK, 1, &inv),
            EquipCandidate::Empty
        );
        assert_eq!(
            equip_candidate(EQUIP_MODE_BLANK, 0, &inv),
            EquipCandidate::None
        );
        // Anything else falls through to the plain block.
        assert_eq!(equip_candidate(0, 1, &inv), EquipCandidate::None);
        assert_eq!(equip_candidate(0x2000, 1, &inv), EquipCandidate::None);
    }

    #[test]
    fn the_plain_block_is_three_label_value_rows() {
        let input = panel_input();
        let out = equip_stat_panel(&input, item, equip);
        assert_eq!(out.len(), 6, "three rows, label + current only");
        // Row 0 sits at the pen; rows step by the wide pitch.
        let want_y = [0x30, 0x30 + 0x0E, 0x30 + 0x1C];
        for (row, &y) in want_y.iter().enumerate() {
            assert_eq!(
                out[row * 2],
                EquipPanelDraw::Label {
                    label_va: EQUIP_LABEL_VAS[row],
                    x: 0x20 + EQUIP_COL_LABEL,
                    y,
                    ink: EQUIP_INK_NORMAL,
                }
            );
            // Slot 0 holds item 1, whose bonus lands on accumulator 1 - so
            // only the ATK row picks it up.
            let bonus = if row == 0 { 5 } else { 0 };
            assert_eq!(
                out[row * 2 + 1],
                EquipPanelDraw::Value {
                    value: input.base_stats[row] + bonus,
                    x: 0x20 + EQUIP_COL_CURRENT,
                    y,
                    ink: EQUIP_INK_NORMAL,
                }
            );
        }
    }

    #[test]
    fn the_board_flag_tightens_the_row_pitch_by_one() {
        let mut input = panel_input();
        input.tight_rows = true;
        let out = equip_stat_panel(&input, item, equip);
        let ys: Vec<i16> = out
            .iter()
            .filter_map(|d| match d {
                EquipPanelDraw::Label { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(ys, vec![0x30, 0x30 + 0x0D, 0x30 + 0x1A]);
    }

    #[test]
    fn a_candidate_adds_the_arrow_and_inks_the_column_beside_it() {
        // Trial-equip item 1 (ATK +5) over an empty weapon slot: ATK rises.
        let mut input = panel_input();
        input.slots = [0; 5];
        input.base_stats = [10, 20, 30];
        input.mode = EQUIP_MODE_DIRECT;
        input.cursor = 1;
        let out = equip_stat_panel(&input, item, equip);

        // Row 0: label, current, arrow, candidate. Rows 1/2: no arrow.
        assert_eq!(
            out[2],
            EquipPanelDraw::Arrow {
                glyph: EQUIP_GLYPH_HIGHER,
                x: 0x20 + EQUIP_COL_ARROW,
                y: 0x30,
                // The arrow itself draws under the label's ink; its own store
                // lands after the `jal`.
                ink: EQUIP_INK_NORMAL,
            }
        );
        assert_eq!(
            out[3],
            EquipPanelDraw::Value {
                value: 10 + 5,
                x: 0x20 + EQUIP_COL_CANDIDATE,
                y: 0x30,
                ink: EQUIP_INK_HIGHER,
            }
        );
        // The unchanged rows still print a candidate column, just no arrow and
        // no ink change.
        assert_eq!(
            out[6],
            EquipPanelDraw::Value {
                value: 20,
                x: 0x20 + EQUIP_COL_CANDIDATE,
                y: 0x30 + 0x0E,
                ink: EQUIP_INK_NORMAL,
            }
        );
        assert!(
            !out.iter().any(|d| matches!(
                d,
                EquipPanelDraw::Arrow { y, .. } if *y != 0x30
            )),
            "only the stat that moved gets an arrow"
        );
    }

    #[test]
    fn losing_a_stat_picks_the_other_glyph_and_ink() {
        let mut input = panel_input();
        // Currently wearing item 1 in the weapon slot; the candidate is item 2,
        // which is `kind 2` - so retail draws the plain block, not a swap.
        input.mode = EQUIP_MODE_DIRECT;
        input.cursor = 2;
        assert_eq!(equip_stat_panel(&input, item, equip).len(), 6);

        // A real downgrade: wearing item 3 (+9), trial item 1 (+5).
        let mut input = panel_input();
        input.char_index = 0;
        input.slots = [0, 0, 3, 0, 0];
        input.weapon_slots = vec![2, 2, 2];
        input.mode = EQUIP_MODE_DIRECT;
        input.cursor = 1;
        let out = equip_stat_panel(&input, item, equip);
        assert_eq!(
            out[2],
            EquipPanelDraw::Arrow {
                glyph: EQUIP_GLYPH_LOWER,
                x: 0x20 + EQUIP_COL_ARROW,
                y: 0x30,
                ink: EQUIP_INK_NORMAL,
            }
        );
        assert!(matches!(
            out[3],
            EquipPanelDraw::Value {
                value: 15,
                ink: EQUIP_INK_LOWER,
                ..
            }
        ));
    }

    #[test]
    fn an_unequippable_candidate_replaces_the_whole_panel_with_one_line() {
        let mut input = panel_input();
        // Item 3's mask is Vahn-only and this entry is Noa.
        input.char_index = 1;
        input.mode = EQUIP_MODE_DIRECT;
        input.cursor = 3;
        let out = equip_stat_panel(&input, item, equip);
        assert_eq!(
            out,
            vec![EquipPanelDraw::Label {
                label_va: EQUIP_REJECT_VA,
                x: 0x20 + EQUIP_REJECT_OFFSET.0,
                y: 0x30 + EQUIP_REJECT_OFFSET.1,
                ink: EQUIP_INK_REJECT,
            }]
        );
    }

    #[test]
    fn a_candidate_of_any_other_kind_draws_nothing_at_all() {
        let mut input = panel_input();
        input.mode = EQUIP_MODE_DIRECT;
        input.cursor = 9;
        assert!(
            equip_stat_panel(&input, item, equip).is_empty(),
            "`bne a0,v0,0x801E63E0` at 0x801E6280 skips the epilogue's draws"
        );
    }

    #[test]
    fn the_empty_candidate_compares_the_loadout_against_nothing() {
        let mut input = panel_input();
        input.mode = EQUIP_MODE_BLANK;
        input.cursor = 1;
        let out = equip_stat_panel(&input, item, equip);
        // ATK is 10 + 5 now and 10 bare, so the candidate column is lower.
        assert!(matches!(
            out[2],
            EquipPanelDraw::Arrow {
                glyph: EQUIP_GLYPH_LOWER,
                ..
            }
        ));
        assert!(matches!(
            out[3],
            EquipPanelDraw::Value {
                value: 10,
                ink: EQUIP_INK_LOWER,
                ..
            }
        ));
    }

    #[test]
    fn fixed_decimal_zero_pads() {
        assert_eq!(format_fixed_decimal(7, 3), "007");
        assert_eq!(format_fixed_decimal(0, 2), "00");
        assert_eq!(format_fixed_decimal(1234, 4), "1234");
    }

    #[test]
    fn fixed_decimal_truncates_to_width() {
        // value % 10^width - the retail reduction.
        assert_eq!(format_fixed_decimal(1234, 3), "234");
        assert_eq!(format_fixed_decimal(99999, 4), "9999");
    }

    #[test]
    fn fixed_decimal_negative_prefixes_minus() {
        assert_eq!(format_fixed_decimal(-5, 3), "-005");
        assert_eq!(format_fixed_decimal(-42, 2), "-42");
    }

    #[test]
    fn dev_menu_row_indexing() {
        assert_eq!(DevMenuRow::from_index(0), Some(DevMenuRow::MapChange));
        assert_eq!(DevMenuRow::from_index(0x0C), Some(DevMenuRow::EquipVahn));
        assert_eq!(DevMenuRow::from_index(0x0E), Some(DevMenuRow::EquipGala));
        assert_eq!(DevMenuRow::from_index(0x17), Some(DevMenuRow::ResetErrors));
        assert_eq!(DevMenuRow::from_index(0x18), None);
    }

    #[test]
    fn only_map_card_rows_close() {
        assert!(DevMenuRow::MapChange.is_closed(1));
        assert!(DevMenuRow::CardOption.is_closed(0x1234));
        assert!(!DevMenuRow::MapChange.is_closed(0));
        assert!(!DevMenuRow::PlayerStatus.is_closed(1));
    }

    #[test]
    fn camera_readout_sentinel_and_average() {
        assert_eq!(decode_camera_readout(0x7F7F_0000), None);
        // low lane bytes 0x40 & 0x60 -> (0x40+0x60)/2 = 0x50
        // high lane bytes 0x10 & 0x30 -> (0x10+0x30)/2 = 0x20
        let w = 0x30_60_10_40u32; // b3=0x30, b2=0x60, b1=0x10, b0=0x40
        assert_eq!(decode_camera_readout(w), Some((0x50, 0x20)));
    }

    #[test]
    fn panel_geometry_bottom_anchors() {
        // 5 rows -> height 40, y = 0xD0 - 40 = 168.
        assert_eq!(panel_geometry(0, 4), PanelGeometry { y: 168, height: 40 });
        // single row.
        assert_eq!(panel_geometry(3, 3), PanelGeometry { y: 0xC8, height: 8 });
    }

    #[test]
    fn cursor_wraps_by_swap() {
        // up from top -> jumps to bottom.
        assert_eq!(dev_menu_cursor_step(0, 0, 4, true, false), (4, true));
        // down from bottom -> jumps to top.
        assert_eq!(dev_menu_cursor_step(4, 0, 4, false, true), (0, true));
        // plain move.
        assert_eq!(dev_menu_cursor_step(2, 0, 4, false, true), (3, true));
        // no input -> no move.
        assert_eq!(dev_menu_cursor_step(2, 0, 4, false, false), (2, false));
    }

    #[test]
    fn list_body_draw_gate() {
        // phase Active(1) with input allowed -> draws.
        assert!(list_body_draws(1, 1));
        // phase CancelUnwind(3) while unwind running -> draws.
        assert!(list_body_draws(3, 1));
        // phase CancelUnwind after unwind done -> no draw.
        assert!(!list_body_draws(3, 0));
        // phase ConfirmSettle -> never draws.
        assert!(!list_body_draws(2, 1));
        // phase Active with input suppressed -> no draw.
        assert!(!list_body_draws(1, 0));
    }

    #[test]
    fn play_time_decomposition_and_clamp() {
        // 90 frames -> 1 second (90/60 = 1).
        assert_eq!(decompose_play_time(90), (0, 0, 1));
        // 1 hour = 60*60*60 frames.
        assert_eq!(decompose_play_time(60 * 60 * 60), (1, 0, 0));
        // over 99h clamps to 99:59:59.
        assert_eq!(decompose_play_time(200 * 60 * 60 * 60), (99, 59, 59));
    }

    #[test]
    fn records_screen_clamps() {
        let chars = [
            CharRecordStats {
                max_hits: 5000,
                max_damage: 50_000_000,
                knockouts: 5000,
                monsters_defeated: 5_000_000,
                hyper_arts: 7,
                magic: 3,
            },
            CharRecordStats::default(),
            CharRecordStats::default(),
        ];
        let r = records_screen(200_000, 42, 60, &chars, 25, 50);
        assert_eq!(r.battles, 99_999); // capped
        assert_eq!(r.escapes, 42); // uncapped
        assert_eq!(r.max_hits[0], 999);
        assert_eq!(r.max_damage[0], 9_999_999);
        assert_eq!(r.knockouts[0], 999);
        assert_eq!(r.monsters_defeated[0], 999_999);
        assert_eq!(r.hyper_arts[0], 7);
        assert!(r.treasure_shown);
        assert_eq!(r.treasure_percent, 50); // 25*100/50
        assert_eq!(r.treasure_fraction, 0);
    }

    #[test]
    fn records_treasure_hidden_when_no_total() {
        let chars = [CharRecordStats::default(); 3];
        let r = records_screen(0, 0, 0, &chars, 0, 0);
        assert!(!r.treasure_shown);
        assert_eq!(r.treasure_percent, 0);
    }

    #[test]
    fn treasure_fraction_nonzero() {
        let chars = [CharRecordStats::default(); 3];
        // 1 of 3 -> 33.33%
        let r = records_screen(0, 0, 0, &chars, 1, 3);
        assert_eq!(r.treasure_percent, 33); // 100/3
        assert_eq!(r.treasure_fraction, 33); // 10000/3=3333, minus 33*100=3300 -> 33
    }

    #[test]
    fn equip_aggregation_sums_bonuses() {
        // slots 1 and 2 equipped, ids 3 and 4 respectively.
        let slots = [1u8, 2, 0, 0, 0];
        let item_stat = |id: u8| match id {
            1 => 10, // stat index 10
            2 => 20,
            _ => 0,
        };
        let bonus = |idx: u8| match idx {
            10 => [1, 2, 3, 4, 5],
            20 => [5, 4, 3, 2, 1],
            _ => [0, 0, 0, 0, 0],
        };
        let totals = aggregate_slot_stats(&slots, item_stat, bonus);
        assert_eq!(totals, [6, 6, 6, 6, 6]);
    }

    #[test]
    fn equippability_mask() {
        // mask bit per character.
        assert!(can_equip(0b001, 0)); // Vahn
        assert!(!can_equip(0b001, 1)); // Noa cannot
        assert!(can_equip(0b010, 1)); // Noa
        assert!(can_equip(0b100, 2)); // Gala
        assert!(can_equip(0, 5)); // out-of-range char treated equippable
    }

    #[test]
    fn slot_bits_resolution() {
        let weapon = [0i16, 1, 2, 3];
        assert_eq!(resolve_equip_slot(0x00, 0, &weapon), 0); // sel 0
        assert_eq!(resolve_equip_slot(0x20, 0, &weapon), 1); // sel 1
        assert_eq!(resolve_equip_slot(0x40, 2, &weapon), 2); // sel 2 -> weapon[2]
        assert_eq!(resolve_equip_slot(0x60, 0, &weapon), 4); // sel 3 -> slot 4
    }

    #[test]
    fn stat_delta_arrows() {
        let current = [10, 20, 30, 0, 0];
        let candidate = [15, 20, 25, 0, 0];
        let d = stat_deltas(&current, &candidate);
        assert_eq!(d[0], StatDelta::Up);
        assert_eq!(d[1], StatDelta::Same);
        assert_eq!(d[2], StatDelta::Down);
    }
}
