//! The menu overlay's developer **character-parameter editor** - the input
//! and stat-clamp halves of `FUN_801D6E18`.
//!
//! The editor is not part of the retail save flow (see
//! `docs/subsystems/save-screen.md`); it is a debug page that walks a
//! twelve-row cursor over one character's stat fields and steps the hovered
//! one. What makes it worth porting is the pass it runs at the **end of
//! every tick, unconditionally**: a two-stage clamp over all four character
//! records that is the game's own statement of its stat caps.
//!
//! ## What is ported here
//!
//! * `0x801D6EC0..0x801D7240` - the row cursor, the step scaling, and the
//!   twelve-way row -> record-field jump table ([`step_row`], [`edit_step`],
//!   [`editor_row`], [`DebugEditor::tick`]).
//! * `0x801D72F4..0x801D7518` - the clamp pass ([`clamp_record_stats`]).
//!
//! Not ported: the bespoke debug **renderer** that fills the rest of the
//! function (`0x801D7524` onward - roughly two thirds of its 890
//! instructions), and the phase router `FUN_801DA2A0` that drives it.
//! Both are screen-side and have no engine surface to draw into.
//!
//! ## The clamp pass
//!
//! Every tick, for each of the four records at `0x80084140 + n*0x414`:
//!
//! 1. a sanity stage - the level byte outside `1..=0xC7` resets to `1`, and
//!    each of the nine record-side stat halfwords outside `1..=0x4E1F`
//!    resets to `1`;
//! 2. a ceiling stage - each field is capped at its own maximum.
//!
//! The ceilings line up exactly with `legaia_save`'s
//! `RecordStats` window, and the `+0x120` cap constant's ceiling of `100`
//! independently confirms the "always 100 in captured saves" reading:
//!
//! | Char offset | Field | Ceiling |
//! |---|---|---|
//! | `+0x11C` | max HP | `9999` |
//! | `+0x11E` | max MP | `999` |
//! | `+0x120` | cap constant | `100` |
//! | `+0x122` | AGL | `999` |
//! | `+0x124` | ATK | `999` |
//! | `+0x126` | UDF | `999` |
//! | `+0x128` | LDF | `999` |
//! | `+0x12A` | SPD | `999` |
//! | `+0x12C` | INT | `999` |
//! | `+0x130` | level | `99` |
//!
//! Ported from `FUN_801D6E18` (`0x801D6EC0..0x801D7240` cursor + field edit,
//! `0x801D72F4..0x801D7518` clamp pass; the renderer half is not ported).
//! The address tag sits on each implementing function below rather than on
//! this module block - a module-level tag would claim every symbol in the
//! file and report the whole set live off one name collision.
//!
//! Source: `ghidra/scripts/funcs/overlay_save_ui_801d6e18.txt`.
//!
//! Wired: [`crate::dev_menu_host::DevMenuSession`]'s `PLAYER_PARAM` page
//! drives [`DebugEditor::tick`], and its per-frame tail runs
//! [`clamp_record_stats`] over the whole party exactly as retail does.

/// Character-record offsets the editor touches, relative to the record
/// base (`0x80084708 + n*0x414`). The retail code addresses them off
/// `0x80084140 + n*0x414`, i.e. `0x5C8` lower.
pub mod offsets {
    /// Experience word (`i32`), the only 32-bit field the editor steps.
    pub const XP: usize = 0x000;
    /// Maximum HP.
    pub const HP_MAX: usize = 0x11C;
    /// Maximum MP.
    pub const MP_MAX: usize = 0x11E;
    /// Per-stat cap constant.
    pub const CAP_CONSTANT: usize = 0x120;
    /// Agility.
    pub const AGL: usize = 0x122;
    /// Attack.
    pub const ATK: usize = 0x124;
    /// Up-defence.
    pub const UDF: usize = 0x126;
    /// Down-defence.
    pub const LDF: usize = 0x128;
    /// Speed.
    pub const SPD: usize = 0x12A;
    /// Intelligence.
    pub const INT: usize = 0x12C;
    /// Displayed level byte.
    pub const LEVEL: usize = 0x130;
    /// First byte of the `0x11`-byte span the confirm row zeroes.
    pub const CONFIRM_CLEAR_START: usize = 0x185;
    /// Length of that span (`+0x185` plus a `0x10`-byte loop).
    pub const CONFIRM_CLEAR_LEN: usize = 0x11;
}

/// Rows the cursor `_DAT_8007BB88` walks, in jump-table order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorRow {
    /// Row 0 - steps the character cursor `DAT_801E46C4` over `0..=3`,
    /// wrapping. Uses the step's *direction* only, never its magnitude.
    Character,
    /// Rows 1-9 and 11 - add the step to the record field at this offset.
    Field {
        offset: usize,
        width: FieldWidth,
        /// Row 11 scales the step by `0x10` before the add.
        scale: i32,
    },
    /// Row 10 - no field; the confirm button on this row clears the
    /// `+0x185` span instead.
    Confirm,
}

/// Width of an editable record field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldWidth {
    U8,
    U16,
    I32,
}

/// Number of editor rows (`sltiu row, 0xC` guards the jump table).
pub const EDITOR_ROWS: u32 = 0x0C;

/// The row the confirm button acts on.
pub const CONFIRM_ROW: u32 = 10;

/// Resolve a row to what it edits. Rows past [`EDITOR_ROWS`] edit nothing.
///
/// PORT: FUN_801d6e18 (jump table at `0x801CEDC8`, 12 entries)
pub fn editor_row(row: u32) -> Option<EditorRow> {
    use offsets as o;
    Some(match row {
        0 => EditorRow::Character,
        1 => EditorRow::Field {
            offset: o::LEVEL,
            width: FieldWidth::U8,
            scale: 1,
        },
        2 => field16(o::HP_MAX),
        3 => field16(o::MP_MAX),
        4 => field16(o::ATK),
        5 => field16(o::UDF),
        6 => field16(o::LDF),
        7 => field16(o::SPD),
        8 => field16(o::INT),
        9 => field16(o::AGL),
        10 => EditorRow::Confirm,
        11 => EditorRow::Field {
            offset: o::XP,
            width: FieldWidth::I32,
            scale: 0x10,
        },
        _ => return None,
    })
}

const fn field16(offset: usize) -> EditorRow {
    EditorRow::Field {
        offset,
        width: FieldWidth::U16,
        scale: 1,
    }
}

// --- Pad bits --------------------------------------------------------------

/// `_DAT_8007BB84 & 0x1000` - move the row cursor up (wraps `0 -> 0xB`).
pub const PAD_ROW_UP: u32 = 0x1000;
/// `_DAT_8007BB84 & 0x4000` - move the row cursor down (wraps `0xB -> 0`).
pub const PAD_ROW_DOWN: u32 = 0x4000;
/// `_DAT_8007BB84 & 0xA000` - either edit direction is held.
pub const PAD_EDIT: u32 = 0xA000;
/// The negative half of [`PAD_EDIT`].
pub const PAD_EDIT_NEGATIVE: u32 = 0x8000;
/// `_DAT_8007B850 & 8` - multiply the step magnitude by 8.
pub const MOD_X8: u32 = 0x8;
/// `_DAT_8007B850 & 2` - shift the magnitude left by 3 (so `&8` **and**
/// `&2` together give `64`, and `&2` alone gives `8`).
pub const MOD_SHIFT3: u32 = 0x2;

/// SFX cued by the confirm row.
pub const CONFIRM_SFX: u8 = 0x25;

/// The signed step one frame of held input produces.
///
/// The magnitude starts at `1`, becomes `8` when [`MOD_X8`] is held, and is
/// then shifted left by three when [`MOD_SHIFT3`] is - so the three
/// reachable magnitudes are `1`, `8` and `64`, and `MOD_SHIFT3` alone also
/// yields `8`. [`PAD_EDIT_NEGATIVE`] negates both the magnitude and the
/// separate `direction` the character row uses.
///
/// PORT: FUN_801d6e18 (`0x801D6F34..0x801D6F70`)
pub fn edit_step(pad: u32, modifiers: u32) -> (i32, i32) {
    let mut magnitude: i32 = 1;
    let mut direction: i32 = 1;
    if modifiers & MOD_X8 != 0 {
        magnitude = 8;
    }
    if modifiers & MOD_SHIFT3 != 0 {
        magnitude <<= 3;
    }
    if pad & PAD_EDIT_NEGATIVE != 0 {
        direction = -1;
        magnitude = -magnitude;
    }
    (magnitude, direction)
}

/// Step the row cursor for one frame of pad input.
///
/// PORT: FUN_801d6e18 (`0x801D6EC0..0x801D6F20`)
pub fn step_row(row: u32, pad: u32) -> u32 {
    let mut row = row;
    if pad & PAD_ROW_UP != 0 {
        row = if row == 0 { EDITOR_ROWS - 1 } else { row - 1 };
    }
    if pad & PAD_ROW_DOWN != 0 {
        row = if row == EDITOR_ROWS - 1 { 0 } else { row + 1 };
    }
    row
}

/// Highest character index the row-0 cursor wraps at.
pub const MAX_CHARACTER_INDEX: u32 = 3;

/// Step the character cursor with wraparound, the row-0 edit.
///
/// PORT: FUN_801d6e18 (`0x801D6FA4..0x801D6FE4`)
pub fn step_character(character: u32, direction: i32) -> u32 {
    if direction > 0 {
        if character == MAX_CHARACTER_INDEX {
            0
        } else {
            character + 1
        }
    } else if character == 0 {
        MAX_CHARACTER_INDEX
    } else {
        character - 1
    }
}

/// Apply one row's edit to a character record.
///
/// The adds are **wrapping and unclamped** - retail lets a byte roll over
/// and relies on [`clamp_record_stats`] at the end of the tick to bring it
/// back. Returns `false` when the row edits no field.
///
/// PORT: FUN_801d6e18 (`0x801D6FE8..0x801D7238`)
pub fn apply_field_edit(record: &mut [u8], row: u32, step: i32) -> bool {
    let Some(EditorRow::Field {
        offset,
        width,
        scale,
    }) = editor_row(row)
    else {
        return false;
    };
    let delta = step.wrapping_mul(scale);
    match width {
        FieldWidth::U8 => {
            let Some(cell) = record.get_mut(offset) else {
                return false;
            };
            *cell = cell.wrapping_add(delta as u8);
        }
        FieldWidth::U16 => {
            if record.len() < offset + 2 {
                return false;
            }
            let v = u16::from_le_bytes([record[offset], record[offset + 1]]);
            record[offset..offset + 2].copy_from_slice(&v.wrapping_add(delta as u16).to_le_bytes());
        }
        FieldWidth::I32 => {
            if record.len() < offset + 4 {
                return false;
            }
            let v = i32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
            record[offset..offset + 4].copy_from_slice(&v.wrapping_add(delta).to_le_bytes());
        }
    }
    true
}

/// The confirm-row action: zero the `0x11`-byte span at `+0x185`, the run
/// immediately before the eight equipment bytes at `+0x196`.
///
/// PORT: FUN_801d6e18 (`0x801D7278..0x801D72DC`)
pub fn apply_confirm_clear(record: &mut [u8]) -> bool {
    let end = offsets::CONFIRM_CLEAR_START + offsets::CONFIRM_CLEAR_LEN;
    if record.len() < end {
        return false;
    }
    record[offsets::CONFIRM_CLEAR_START..end].fill(0);
    true
}

// --- Clamp pass ------------------------------------------------------------

/// Lower bound the sanity stage enforces on every field it touches.
pub const STAT_SANITY_MIN: u16 = 1;
/// Upper bound of the sanity stage for the nine record-side halfwords.
pub const STAT_SANITY_MAX: u16 = 0x4E1F;
/// Upper bound of the sanity stage for the level byte.
pub const LEVEL_SANITY_MAX: u8 = 0xC7;
/// Value a failed sanity check writes back.
pub const STAT_SANITY_RESET: u16 = 1;

/// Per-field ceilings the second stage enforces, as
/// `(char offset, ceiling)`. The order is retail's.
pub const STAT_CEILINGS: [(usize, u16); 9] = [
    (offsets::HP_MAX, 9999),
    (offsets::MP_MAX, 999),
    (offsets::CAP_CONSTANT, 100),
    (offsets::ATK, 999),
    (offsets::UDF, 999),
    (offsets::LDF, 999),
    (offsets::INT, 999),
    (offsets::AGL, 999),
    (offsets::SPD, 999),
];

/// Ceiling on the displayed level byte.
pub const LEVEL_CEILING: u8 = 99;

/// The nine record-side halfwords the sanity stage checks, in retail order.
const SANITY_FIELDS: [usize; 9] = [
    offsets::HP_MAX,
    offsets::MP_MAX,
    offsets::CAP_CONSTANT,
    offsets::ATK,
    offsets::UDF,
    offsets::LDF,
    offsets::INT,
    offsets::AGL,
    offsets::SPD,
];

fn read_u16(record: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*record.get(at)?, *record.get(at + 1)?]))
}

fn write_u16(record: &mut [u8], at: usize, v: u16) {
    if record.len() >= at + 2 {
        record[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
}

/// Run both clamp stages over one character record.
///
/// Retail runs this over all four records every tick regardless of which
/// one the cursor is on, then re-runs the stat aggregator `FUN_80042558`.
///
/// PORT: FUN_801d6e18 (`0x801D72F4..0x801D7518`)
pub fn clamp_record_stats(record: &mut [u8]) {
    // Stage 1 - sanity. The level test is `(v - 1) & 0xFF` against
    // `0xC7`, so `0` (and anything above `0xC7`) resets rather than
    // saturating.
    if let Some(level) = record.get_mut(offsets::LEVEL)
        && level.wrapping_sub(1) >= LEVEL_SANITY_MAX
    {
        *level = STAT_SANITY_RESET as u8;
    }
    for at in SANITY_FIELDS {
        if let Some(v) = read_u16(record, at)
            && v.wrapping_sub(1) >= STAT_SANITY_MAX
        {
            write_u16(record, at, STAT_SANITY_RESET);
        }
    }

    // Stage 2 - per-field ceilings.
    if let Some(level) = record.get_mut(offsets::LEVEL)
        && *level > LEVEL_CEILING
    {
        *level = LEVEL_CEILING;
    }
    for (at, ceiling) in STAT_CEILINGS {
        if let Some(v) = read_u16(record, at)
            && v > ceiling
        {
            write_u16(record, at, ceiling);
        }
    }
}

/// The editor's own cursor state, so a host can drive it without owning
/// the retail globals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DebugEditor {
    /// `_DAT_8007BB88` - the row cursor.
    pub row: u32,
    /// `DAT_801E46C4` - the character being edited.
    pub character: u32,
}

impl DebugEditor {
    /// One frame: move the row cursor, then apply the held edit to the
    /// hovered field (or to the character cursor on row 0). `records` is
    /// the four live character records.
    ///
    /// PORT: FUN_801d6e18 (`0x801D6EC0..0x801D7240`)
    pub fn tick(&mut self, pad: u32, modifiers: u32, records: &mut [&mut [u8]]) {
        self.row = step_row(self.row, pad);
        if pad & PAD_EDIT == 0 {
            return;
        }
        let (step, direction) = edit_step(pad, modifiers);
        match editor_row(self.row) {
            Some(EditorRow::Character) => {
                self.character = step_character(self.character, direction);
            }
            Some(EditorRow::Field { .. }) => {
                if let Some(record) = records.get_mut(self.character as usize) {
                    apply_field_edit(record, self.row, step);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Vec<u8> {
        vec![0u8; 0x414]
    }

    #[test]
    fn the_row_table_matches_the_display_order() {
        assert_eq!(editor_row(0), Some(EditorRow::Character));
        assert_eq!(editor_row(10), Some(EditorRow::Confirm));
        assert_eq!(editor_row(12), None);
        // Rows 1..=9 are LV, HP, MP, ATK, UDF, LDF, SPD, INT, AGL.
        let want = [
            offsets::LEVEL,
            offsets::HP_MAX,
            offsets::MP_MAX,
            offsets::ATK,
            offsets::UDF,
            offsets::LDF,
            offsets::SPD,
            offsets::INT,
            offsets::AGL,
        ];
        for (i, offset) in want.into_iter().enumerate() {
            match editor_row(i as u32 + 1) {
                Some(EditorRow::Field { offset: got, .. }) => assert_eq!(got, offset, "row {i}"),
                other => panic!("row {i}: {other:?}"),
            }
        }
        // The cap constant at +0x120 is deliberately not editable - the
        // table skips it even though the clamp pass still guards it.
        for row in 0..EDITOR_ROWS {
            if let Some(EditorRow::Field { offset, .. }) = editor_row(row) {
                assert_ne!(offset, offsets::CAP_CONSTANT);
            }
        }
    }

    #[test]
    fn step_magnitudes_are_one_eight_and_sixty_four() {
        assert_eq!(edit_step(0x2000, 0).0, 1);
        assert_eq!(edit_step(0x2000, MOD_X8).0, 8);
        assert_eq!(edit_step(0x2000, MOD_SHIFT3).0, 8);
        assert_eq!(edit_step(0x2000, MOD_X8 | MOD_SHIFT3).0, 64);
    }

    #[test]
    fn the_negative_bit_flips_both_the_magnitude_and_the_direction() {
        let (step, dir) = edit_step(PAD_EDIT_NEGATIVE, MOD_X8);
        assert_eq!(step, -8);
        assert_eq!(dir, -1);
        let (step, dir) = edit_step(0x2000, MOD_X8);
        assert_eq!((step, dir), (8, 1));
    }

    #[test]
    fn the_row_cursor_wraps_at_both_ends() {
        assert_eq!(step_row(0, PAD_ROW_UP), EDITOR_ROWS - 1);
        assert_eq!(step_row(EDITOR_ROWS - 1, PAD_ROW_DOWN), 0);
        assert_eq!(step_row(5, PAD_ROW_UP), 4);
        assert_eq!(step_row(5, PAD_ROW_DOWN), 6);
        // Both bits held run both steps in order, landing back where it was.
        assert_eq!(step_row(5, PAD_ROW_UP | PAD_ROW_DOWN), 5);
    }

    #[test]
    fn the_character_cursor_wraps_over_four_slots() {
        assert_eq!(step_character(3, 1), 0);
        assert_eq!(step_character(0, -1), 3);
        assert_eq!(step_character(1, 1), 2);
    }

    #[test]
    fn the_xp_row_scales_its_step_by_sixteen() {
        let mut r = record();
        assert!(apply_field_edit(&mut r, 11, 3));
        assert_eq!(i32::from_le_bytes(r[0..4].try_into().unwrap()), 48);
        assert!(apply_field_edit(&mut r, 11, -1));
        assert_eq!(i32::from_le_bytes(r[0..4].try_into().unwrap()), 32);
    }

    #[test]
    fn field_edits_wrap_rather_than_saturate() {
        let mut r = record();
        r[offsets::LEVEL] = 0;
        assert!(apply_field_edit(&mut r, 1, -1));
        assert_eq!(r[offsets::LEVEL], 0xFF);
        // The clamp pass is what brings it back.
        clamp_record_stats(&mut r);
        assert_eq!(r[offsets::LEVEL], 1);
    }

    #[test]
    fn confirm_clears_the_span_just_before_the_equipment_bytes() {
        let mut r = record();
        for b in r.iter_mut() {
            *b = 0xAA;
        }
        assert!(apply_confirm_clear(&mut r));
        assert!(r[0x185..0x196].iter().all(|&b| b == 0));
        assert_eq!(r[0x184], 0xAA);
        assert_eq!(r[0x196], 0xAA, "the equipment bytes are untouched");
    }

    #[test]
    fn the_sanity_stage_resets_zero_and_absurd_values_to_one() {
        let mut r = record();
        write_u16(&mut r, offsets::HP_MAX, 0);
        write_u16(&mut r, offsets::ATK, 0x4E20);
        write_u16(&mut r, offsets::AGL, 0x4E1F);
        clamp_record_stats(&mut r);
        assert_eq!(read_u16(&r, offsets::HP_MAX), Some(1));
        assert_eq!(read_u16(&r, offsets::ATK), Some(1));
        // 0x4E1F is the last in-range value; the ceiling stage takes it
        // down to 999 rather than the sanity stage down to 1.
        assert_eq!(read_u16(&r, offsets::AGL), Some(999));
    }

    #[test]
    fn the_ceilings_are_the_games_own_stat_caps() {
        let mut r = record();
        for (at, _) in STAT_CEILINGS {
            write_u16(&mut r, at, 0x4E1F);
        }
        r[offsets::LEVEL] = 0xC7;
        clamp_record_stats(&mut r);
        assert_eq!(read_u16(&r, offsets::HP_MAX), Some(9999));
        assert_eq!(read_u16(&r, offsets::MP_MAX), Some(999));
        assert_eq!(read_u16(&r, offsets::CAP_CONSTANT), Some(100));
        assert_eq!(read_u16(&r, offsets::INT), Some(999));
        assert_eq!(r[offsets::LEVEL], LEVEL_CEILING);
    }

    #[test]
    fn a_legal_record_survives_the_clamp_untouched() {
        let mut r = record();
        r[offsets::LEVEL] = 42;
        write_u16(&mut r, offsets::HP_MAX, 512);
        write_u16(&mut r, offsets::MP_MAX, 88);
        write_u16(&mut r, offsets::CAP_CONSTANT, 100);
        for at in [
            offsets::AGL,
            offsets::ATK,
            offsets::UDF,
            offsets::LDF,
            offsets::SPD,
            offsets::INT,
        ] {
            write_u16(&mut r, at, 60);
        }
        let before = r.clone();
        clamp_record_stats(&mut r);
        assert_eq!(r, before);
    }

    #[test]
    fn the_editor_tick_routes_row_zero_to_the_character_cursor() {
        let mut a = record();
        let mut b = record();
        let mut recs: Vec<&mut [u8]> = vec![&mut a, &mut b];
        let mut ed = DebugEditor::default();
        // Row 0, right: the character cursor moves and no record changes.
        ed.tick(0x2000, 0, &mut recs);
        assert_eq!(ed.character, 1);
        assert!(recs[0].iter().all(|&x| x == 0));
        assert!(recs[1].iter().all(|&x| x == 0));

        // Down to row 1 (level) and step it on the now-selected character.
        ed.tick(PAD_ROW_DOWN | 0x2000, MOD_X8, &mut recs);
        assert_eq!(ed.row, 1);
        assert_eq!(recs[1][offsets::LEVEL], 8);
        assert_eq!(recs[0][offsets::LEVEL], 0);
    }

    #[test]
    fn an_idle_frame_edits_nothing() {
        let mut a = record();
        let mut recs: Vec<&mut [u8]> = vec![&mut a];
        let mut ed = DebugEditor::default();
        ed.tick(0, MOD_X8, &mut recs);
        assert_eq!(ed, DebugEditor::default());
        assert!(recs[0].iter().all(|&x| x == 0));
    }
}
