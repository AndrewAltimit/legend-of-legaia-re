//! World-map dev-menu overlay leaves, ported clean-room from the world-map
//! overlay (`overlay_world_map.bin`, base `0x801C0000`; the bytes are
//! byte-identical to the field/PROT-0897 image at the same VAs, so the dump
//! label is only a resolution hint).
//!
//! This module ports the *simulation / data-model* half of five overlay
//! functions. Every one of the originals is dominated by GPU-packet emission
//! (`FUN_8001AA68` text, `FUN_80034B78` / `FUN_80034E4C` number draws,
//! `FUN_8002C69C` panel fills, `FUN_80036888` MES draws). Those calls are the
//! render seam and are **not** reproduced here - the crate boundary keeps
//! `engine-vm` renderer-free. What is ported is the decoded behaviour each
//! function computes *before* it draws: value clamping, fixed-width decimal
//! formatting, cursor/phase logic, timer decomposition, and equipment-stat
//! aggregation. The engine host wires the resulting model to a renderer.
//!
//! PORT: FUN_801EAD98 - world-map dev-menu renderer (row model + formatter)
//! PORT: FUN_801ECA08 - dev-menu panel sizer + list-picker cursor SM
//! PORT: FUN_801ED710 - battle-records screen data model
//! PORT: FUN_801D2EBC - escape-timer countdown scheduler + HUD decomposition
//! PORT: FUN_801E5B4C - equipment stat-comparison preview
//!
//! ## Source
//!
//! - `ghidra/scripts/funcs/overlay_world_map_801ead98.txt`
//! - `ghidra/scripts/funcs/801eca08.txt`
//! - `ghidra/scripts/funcs/overlay_world_map_801ed710.txt`
//! - `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d2ebc.txt`
//! - `ghidra/scripts/funcs/overlay_world_map_801e5b4c.txt`
//!
//! ## NOT WIRED
//!
//! No engine host calls anything in this module, and the reason differs per
//! address. Each entry names what has to exist first - none of them is more
//! plumbing here.
//!
//! - **`FUN_801EAD98` / `FUN_801ECA08`** (row model, formatter, panel sizer,
//!   list-picker cursor + draw gate). The engine has no world-map developer
//!   menu. Retail's is an actor whose handler owns a phase byte (`ctx[+0x54]`)
//!   and a cursor row (`ctx[+0x9E]`), reached from the world-map controller
//!   behind a debug gate; the engine's world-map mode carries neither field
//!   and opens no menu on it. The render half is equally unconsumed -
//!   `legaia_engine_ui::dev_menu_list_draws_for` has no caller either - so
//!   wiring means standing up the debug screen, not connecting two halves.
//! - **`FUN_801ED710`** (battle-records data model). The engine keeps none of
//!   the lifetime counters this screen reads: battles fought, escapes, and the
//!   per-character max-hits / max-damage / knockouts / monsters-defeated /
//!   Hyper-Arts / magic tallies. `World` carries a play-time clock and nothing
//!   else on the list, and the pause menu has no records page to host the
//!   result. Wiring needs those counters on the persistent record first.
//! - **`FUN_801D2EBC`** (escape-timer scheduler). The timer is armed by field
//!   VM `0x4C 0xD3` (`SCHEDULE_TIMED_FLAGS`), which the port decodes and hands
//!   to `FieldHost::op4c_n_d_sub3_party_setup` - a default no-op body that no
//!   engine host overrides, so the duration / threshold / flag-word triple is
//!   discarded at the installer. Nothing arms a timer, so nothing can drain
//!   one. Wiring is three coupled pieces: timer state on the world, that host
//!   override, and a per-frame drain against the clock delta.
//! - **`FUN_801E5B4C`** (equipment stat-comparison preview). The engine's
//!   equip screen is the menu-overlay flow (`legaia_engine_core::EquipSession`,
//!   ported from `FUN_801D9C14` / `FUN_801D99F0`), which previews by
//!   trial-equipping into its own 8-slot array and re-running its stat
//!   aggregator. Nothing produces the 5-slot `char[+0x75E..]` equip window or
//!   the per-character weapon-slot table (`0x8007B42C`) that this shared
//!   comparison panel indexes, so the aggregation has no input.

// ---------------------------------------------------------------------------
// FUN_801EAD98 - fixed-width decimal formatter
// ---------------------------------------------------------------------------

/// Zero-padded fixed-width decimal, ported from the digit kernel that
/// `FUN_801EAD98` inlines ~15 times (one copy per numeric menu readout).
///
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
/// PORT: FUN_801EAD98 (row dispatch)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevMenuRow {
    MapChange,       // 0x00 - "MAP_CHANGE"  (or CLOSED, gate _DAT_8007B868)
    CardOption,      // 0x01 - "CARD_OPTION" (or CLOSED)
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
    /// `MAP_CHANGE` and `CARD_OPTION` rows are gated, by `_DAT_8007B868 != 0`.
    ///
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
/// PORT: FUN_801ECA08 (panel-sizing prologue)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelGeometry {
    /// Panel top Y (`desc[+0x0A]`), bottom-anchored to the 208px viewport.
    pub y: i16,
    /// Panel height in pixels (`desc[+0x0E]`).
    pub height: i16,
}

/// Compute panel geometry from the inclusive row range `row_start..=row_end`.
/// `rows = row_end - row_start + 1`; `height = rows*8`; `y = 0xD0 - rows*8`.
///
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
/// PORT: FUN_801ECA08 (phase-1 cursor block)
pub fn cursor_step(cursor: i32, row_start: i32, row_end: i32, up: bool, down: bool) -> (i32, bool) {
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
/// PORT: FUN_801ECA08 (`switch(ctx[+0x54])`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListPickerPhase {
    /// Seed cursor = row_start, open panel, then falls through into `Active`.
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
/// - phase `CancelUnwind` (3) with `gate = FUN_801EA9B0()` (1 while the
///   unwind is still running) -> draws;
/// - every other product (0, 2, 4, ...) -> no draw.
///
/// PORT: FUN_801ECA08 (`mult s2,s3` draw gate)
pub fn list_body_draws(phase: i16, gate: i32) -> bool {
    let product = phase as i32 * gate;
    product == 1 || product == 3
}

// ---------------------------------------------------------------------------
// FUN_801ED710 - battle-records screen data model
// ---------------------------------------------------------------------------

/// Per-character stat block read from the records stats record at
/// `0x80088140 + n*0x414`.
///
/// PORT: FUN_801ED710 (per-character loops)
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
/// PORT: FUN_801ED710
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
// FUN_801D2EBC - escape-timer countdown scheduler + HUD decomposition
// ---------------------------------------------------------------------------

/// Ink colour the escape-timer HUD selects from the remaining count
/// (`_DAT_8007B454`): white while there is time, then a warning colour, then
/// a critical colour below the last minute-and-a-half.
///
/// PORT: FUN_801D2EBC (`_DAT_8007B454` selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerInk {
    /// Remaining == 0: neutral / white (`2`).
    Neutral = 2,
    /// `0 < remaining <= 0x707`: warning (`6`).
    Warning = 6,
    /// `remaining > 0x707`: cool/safe (`7`).
    Safe = 7,
}

/// The retail ink logic: `2` at zero, `6` while non-zero and `<= 0x707`,
/// `7` above `0x707`.
///
/// PORT: FUN_801D2EBC
pub fn timer_ink(remaining: i32) -> TimerInk {
    if remaining == 0 {
        TimerInk::Neutral
    } else if remaining > 0x707 {
        TimerInk::Safe
    } else {
        TimerInk::Warning
    }
}

/// Story-flag ids the scheduler fires as the counter drops. Both are the low
/// 12 bits of a packed word (`_DAT_800845C0`): the low half is a warning flag
/// fired once the counter falls below `_DAT_800845BC`, the high half an
/// expiry flag fired when it reaches zero.
///
/// PORT: FUN_801D2EBC (`func_0x8003CE08` calls)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimerFlagEvents {
    /// Fire `flags[warning_flag & 0xFFF]` (counter dropped below threshold).
    pub warning_flag: Option<u16>,
    /// Fire `flags[expiry_flag & 0xFFF]` and disarm the timer (counter hit 0).
    pub expiry_flag: Option<u16>,
}

/// Live state of the escape-timer scheduler.
///
/// PORT: FUN_801D2EBC
#[derive(Debug, Clone, Copy, Default)]
pub struct EscapeTimer {
    /// Remaining countdown (`_DAT_800845A0`).
    pub remaining: i32,
    /// Below-threshold trigger point (`_DAT_800845BC`).
    pub warn_threshold: i32,
    /// Whether the timer is still armed (`_DAT_800845B8 != 0`).
    pub armed: bool,
}

impl EscapeTimer {
    /// Advance the countdown by the frame-clock delta and report which story
    /// flags the tick fires. `clock_delta = new_clock - prev_clock`
    /// (`_DAT_80084570 - old _DAT_80073ED4`). `flag_word` is `_DAT_800845C0`:
    /// low half = warning flag, high half = expiry flag.
    ///
    /// When `busy` is set (the retail short-circuit for any of the three
    /// pause conditions) the counter is left untouched and no flags fire -
    /// the caller still refreshes the clock latch.
    ///
    /// PORT: FUN_801D2EBC (scheduler head)
    pub fn tick(&mut self, clock_delta: i32, flag_word: u32, busy: bool) -> TimerFlagEvents {
        let mut events = TimerFlagEvents::default();
        if busy {
            return events;
        }
        self.remaining -= clock_delta;
        if self.remaining < 1 {
            self.armed = false;
            events.expiry_flag = Some(((flag_word >> 16) & 0xFFF) as u16);
        }
        if self.remaining < self.warn_threshold {
            events.warning_flag = Some((flag_word & 0xFFF) as u16);
        }
        events
    }

    /// Decompose the remaining count into the MM:SS.ff fields the HUD draws.
    /// `frames = remaining % 60`, `seconds = (remaining/60) % 60`,
    /// `minutes = (remaining/60) / 60`.
    ///
    /// PORT: FUN_801D2EBC (`% 0x3C` decomposition + `(frames*100)/0x3C`)
    pub fn hud_fields(&self) -> (i32, i32, i32) {
        let frames = self.remaining % 60;
        let seconds = (self.remaining / 60) % 60;
        let minutes = (self.remaining / 60) / 60;
        // The hundredths cell is `(frames * 100) / 60`.
        let hundredths = frames * 100 / 60;
        (minutes, seconds, hundredths)
    }
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
/// PORT: FUN_801E5B4C (arrow-glyph selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatDelta {
    /// Candidate raises the stat (retail glyph 5, ink 1).
    Up,
    /// Candidate lowers the stat (retail glyph 4, ink 6).
    Down,
    /// No change.
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
/// PORT: FUN_801E5B4C (aggregation loops)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(cursor_step(0, 0, 4, true, false), (4, true));
        // down from bottom -> jumps to top.
        assert_eq!(cursor_step(4, 0, 4, false, true), (0, true));
        // plain move.
        assert_eq!(cursor_step(2, 0, 4, false, true), (3, true));
        // no input -> no move.
        assert_eq!(cursor_step(2, 0, 4, false, false), (2, false));
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
    fn escape_timer_ink_thresholds() {
        assert_eq!(timer_ink(0), TimerInk::Neutral);
        assert_eq!(timer_ink(0x708), TimerInk::Safe);
        assert_eq!(timer_ink(0x707), TimerInk::Warning);
        assert_eq!(timer_ink(1), TimerInk::Warning);
    }

    #[test]
    fn escape_timer_fires_flags_on_expiry() {
        let mut t = EscapeTimer {
            remaining: 5,
            warn_threshold: 100,
            armed: true,
        };
        // flag word: low half 0x0C7 (warning), high half 0x123 (expiry).
        let ev = t.tick(10, 0x0123_00C7, false);
        assert_eq!(t.remaining, -5);
        assert!(!t.armed); // disarmed on expiry
        assert_eq!(ev.expiry_flag, Some(0x123));
        assert_eq!(ev.warning_flag, Some(0x0C7)); // also below threshold
    }

    #[test]
    fn escape_timer_warning_only() {
        let mut t = EscapeTimer {
            remaining: 200,
            warn_threshold: 100,
            armed: true,
        };
        // drop to 150 -> above 0, below... no, 150 > 100 -> no warning yet.
        let ev = t.tick(50, 0x0123_00C7, false);
        assert_eq!(t.remaining, 150);
        assert!(t.armed);
        assert_eq!(ev.expiry_flag, None);
        assert_eq!(ev.warning_flag, None);
        // drop below threshold.
        let ev = t.tick(60, 0x0123_00C7, false);
        assert_eq!(t.remaining, 90);
        assert_eq!(ev.warning_flag, Some(0x0C7));
        assert_eq!(ev.expiry_flag, None);
    }

    #[test]
    fn escape_timer_busy_freezes() {
        let mut t = EscapeTimer {
            remaining: 5,
            warn_threshold: 100,
            armed: true,
        };
        let ev = t.tick(10, 0x0123_00C7, true);
        assert_eq!(t.remaining, 5); // untouched
        assert!(t.armed);
        assert_eq!(ev, TimerFlagEvents::default());
    }

    #[test]
    fn escape_timer_hud_fields() {
        let t = EscapeTimer {
            remaining: 60 * 90 + 30, // 1m30s + 30 frames
            warn_threshold: 0,
            armed: true,
        };
        let (m, s, hundredths) = t.hud_fields();
        assert_eq!(m, 1);
        assert_eq!(s, 30);
        assert_eq!(hundredths, 30 * 100 / 60); // 50
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
