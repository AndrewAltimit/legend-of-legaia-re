//! The two large menu-overlay window painters: the equip screen's
//! **stat-compare** panels.
//!
//! Both are content renderers named by the menu-overlay window-descriptor
//! table (52 records at VA `0x801E4738` / PROT 0899 file `0x15F20`, parser
//! `legaia_asset::menu_windows`), so the geometry below hangs off the
//! window's live content origin and nothing else:
//!
//! | Window | Rect | Renderer |
//! |---|---|---|
//! | 25 | `(14, 40, 144, 52)` | `FUN_801D1290` - active character, one stat pair or triple |
//! | 41 | `(14, 46, 108, 158)` | `FUN_801D4C28` - the same compare for every party member |
//!
//! Panels are content-only draws: the 9-slice frame is caller-drawn.
//!
//! ## The stat block both read
//!
//! `FUN_801CF5D0(char_idx)` seeds an eight-word block at `0x801EF080` from
//! the character record, and `FUN_801CF650` sums the equipment bonuses into
//! it. The trial-equip mirror lives one block later at `0x801EF0A0`:
//!
//! ```text
//! +0x00 <- record +0x6CC (char +0x104)  HP max
//! +0x04 <- record +0x6D0 (char +0x108)  MP max
//! +0x08 <- record +0x6D8 (char +0x110)  AGL
//! +0x0C <- record +0x6DA (char +0x112)  ATK
//! +0x10 <- record +0x6DC (char +0x114)  UDF
//! +0x14 <- record +0x6DE (char +0x116)  LDF
//! +0x18 <- record +0x6E0 (char +0x118)  SPD
//! +0x1C <- record +0x6E2 (char +0x11A)  INT
//! ```
//!
//! [`EquipStatBlock`] is that block; a panel row prints the `0x801EF080`
//! value, then - only when the two differ - a rise/fall arrow plus the
//! `0x801EF0A0` value.
//!
//! ## Which stats a row shows
//!
//! `FUN_801D1290` picks the row set from a single **category byte**, and the
//! byte comes from two different tables depending on the item's class
//! (`0x801D1388..0x801D1474`): equipment (`item[+0] == 1`) contributes the
//! equip record's `+5` byte, anything else the item-effect record's `+3`.
//! See [`CompareRows::from_category`] for the three-way split and
//! [`active_compare_category`] for the fallback chain.
//!
//! `FUN_801D4C28` has no such switch - it always shows the ATK / UDF / LDF
//! triple, because it is the party-wide "what would this do for everyone"
//! column rather than the per-slot detail panel.
//!
//! PORT: FUN_801d1290 - window 25, active-character stat compare
//! PORT: FUN_801d4c28 - window 41, per-party-member stat compare
//! REF: FUN_801cf5d0 - the seeder that fills [`EquipStatBlock`]
//! REF: FUN_801cf650 - the equipment-bonus summer over the same block
//!
//! Source: `ghidra/scripts/funcs/overlay_menu_801d1290.txt`,
//! `ghidra/scripts/funcs/overlay_menu_801d4c28.txt`.
//!
//! # NOT WIRED
//!
//! Neither host opens these windows. The engine's equip flow
//! (`legaia_engine_core::equip_session`) previews a candidate through
//! `compute_battle_stats` and renders it with the engine's own equipment
//! screen; it has neither the retail eight-word block nor the window-25 /
//! window-41 containers to draw into, and `EquipSession` exposes no
//! party-wide preview at all (it is single-character by construction).
//! Wiring needs the equip screen rebuilt on the retail window set first.

use crate::{TextDraw, text_draws_for};

/// The eight-word derived-stat block at `0x801EF080` (and its trial-equip
/// mirror at `0x801EF0A0`), in the order `FUN_801CF5D0` writes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EquipStatBlock {
    /// Word 0 - HP maximum.
    pub hp: i32,
    /// Word 1 - MP maximum.
    pub mp: i32,
    /// Word 2 - agility. No equipment byte adds to it.
    pub agl: i32,
    /// Word 3 - attack.
    pub atk: i32,
    /// Word 4 - defense-up.
    pub udf: i32,
    /// Word 5 - defense-down.
    pub ldf: i32,
    /// Word 6 - speed.
    pub spd: i32,
    /// Word 7 - intelligence.
    pub int: i32,
}

impl EquipStatBlock {
    /// Word `i` of the block (`0..=7`), matching the retail `0x801EF080 +
    /// i*4` addressing the painters index with. Out-of-range reads `0`.
    pub fn word(&self, i: usize) -> i32 {
        match i {
            0 => self.hp,
            1 => self.mp,
            2 => self.agl,
            3 => self.atk,
            4 => self.udf,
            5 => self.ldf,
            6 => self.spd,
            7 => self.int,
            _ => 0,
        }
    }
}

/// Which stat rows a compare panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareRows {
    /// Two rows: HP then MP (block words 0 and 1). Category `< 6`.
    HpMp,
    /// Three rows: SPD, INT, AGL (block words 6, 7, 2). Category `10..=12`.
    SpdIntAgl,
    /// Three rows: ATK, UDF, LDF (block words 3, 4, 5). Every other
    /// category - including the `0x40` no-passive sentinel every retail
    /// equipment row carries, which is why an ordinary weapon or armour
    /// swap shows this triple.
    AtkUdfLdf,
}

impl CompareRows {
    /// The three-way split at `0x801D1474` / `0x801D1640`: `sltiu cat, 6`
    /// first, then `sltiu (cat - 10), 3`. Both compares are unsigned, so
    /// the second is a wrapping subtract.
    ///
    /// PORT: FUN_801d1290 (`0x801D1470..0x801D1648`)
    pub fn from_category(category: u8) -> Self {
        if category < 6 {
            CompareRows::HpMp
        } else if category.wrapping_sub(10) < 3 {
            CompareRows::SpdIntAgl
        } else {
            CompareRows::AtkUdfLdf
        }
    }

    /// Block word indices this row set prints, in emit order.
    pub fn word_indices(self) -> &'static [usize] {
        match self {
            CompareRows::HpMp => &[0, 1],
            CompareRows::SpdIntAgl => &[6, 7, 2],
            CompareRows::AtkUdfLdf => &[3, 4, 5],
        }
    }
}

/// Category byte used when no item resolves one - the value `FUN_801D1290`
/// pre-loads before its lookup chain, and the value every retail equipment
/// record's `+5` byte happens to hold.
pub const CATEGORY_DEFAULT: u8 = 0x40;

/// Inputs to [`active_compare_category`], each named for the retail global
/// it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompareCategoryInputs {
    /// `(DAT_801E46C0 & 0xFFF) - 1`: the equip screen's slot-browse row
    /// minus its row 0 ("Best Equipment"), so `0..=6` are the slot rows.
    pub slot_row: i32,
    /// `DAT_801E46B0`: the staged (hovered) item id. `-1` = nothing staged.
    pub staged_id: i32,
    /// Category byte resolved for [`Self::staged_id`].
    pub staged_category: u8,
    /// The id already sitting in the row's equip slot; `0` = empty.
    pub equipped_id: u8,
    /// Category byte resolved for [`Self::equipped_id`].
    pub equipped_category: u8,
}

/// Resolve the category byte the panel keys its row set on.
///
/// Two guards from the disassembly are easy to lose:
///
/// * The staged item's category is only consulted on **slot rows `>= 4`**
///   (`slti s0, 4` at `0x801D137C` branches *past* the lookup). Weapon and
///   armour rows therefore always fall back to [`CATEGORY_DEFAULT`], which
///   is why they always show the ATK / UDF / LDF triple regardless of what
///   is hovered.
/// * The "nothing staged" fallback (`DAT_801E46B0 == -1`) is **not** gated
///   on the slot row, so an empty hover still picks its rows off whatever
///   is currently equipped in the slot.
///
/// PORT: FUN_801d1290 (`0x801D137C..0x801D1474`)
pub fn active_compare_category(inp: CompareCategoryInputs) -> u8 {
    let mut category = CATEGORY_DEFAULT;
    if inp.slot_row >= 4 && inp.staged_id > 0 {
        category = inp.staged_category;
    }
    if inp.staged_id == -1 && inp.equipped_id != 0 {
        category = inp.equipped_category;
    }
    category
}

// --- Ink staging ids (`_DAT_8007B454`) -------------------------------------

/// Default ink: names, labels and every printed value.
pub const INK_DEFAULT: u8 = 7;
/// Ink staged for a rising stat (the candidate is higher).
pub const INK_RISE: u8 = 6;
/// Ink staged for a falling stat (the candidate is not higher).
pub const INK_FALL: u8 = 1;
/// Ink staged for the "already equipped" note in window 41.
pub const INK_EQUIPPED: u8 = 4;
/// Ink staged for the "cannot equip" note in window 41.
pub const INK_CANNOT_EQUIP: u8 = 9;

/// Separator-glyph id (`FUN_8003C1F8`) for a rising stat.
pub const ARROW_RISE: u8 = 4;
/// Separator-glyph id for a falling stat.
pub const ARROW_FALL: u8 = 5;

/// UI-icon-atlas ids (`FUN_8002C488`) the HP row draws.
pub const ICON_HP: (u8, u8) = (0x64, 0x3F);
/// UI-icon-atlas ids the MP row draws.
pub const ICON_MP: (u8, u8) = (0x65, 0x40);

// --- Layout ---------------------------------------------------------------

/// Vertical pitch between compare rows in both painters.
const ROW_PITCH: i32 = 0x0D;
/// First row's y offset from the window content origin (window 25).
const ROW0_DY: i32 = 0x10;
/// Icon rows sit two scanlines below their row baseline.
const ICON_DY: i32 = 2;

// Window 25 columns, relative to the content origin.
const W25_ICON_X: i32 = 0x10;
const W25_ICON2_X: i32 = 0x24;
const W25_PAIR_VALUE_X: i32 = 0x34;
const W25_PAIR_ARROW_X: i32 = 0x58;
const W25_PAIR_DELTA_X: i32 = 0x64;
const W25_LABEL_X: i32 = 0x10;
const W25_VALUE_X: i32 = 0x38;
const W25_ARROW_X: i32 = 0x54;
const W25_DELTA_X: i32 = 0x60;

// Window 41 columns + per-member pitch.
const W41_MEMBER_PITCH: i32 = 0x37;
const W41_NOTE_DX: i32 = 0x0C;
const W41_NOTE_DY: i32 = 0x14;
const W41_ROW0_DY: i32 = 0x0D;
const W41_LABEL_X: i32 = 0x04;
const W41_VALUE_X: i32 = 0x2C;
const W41_ARROW_X: i32 = 0x48;
const W41_DELTA_X: i32 = 0x54;

/// Digit width of an HP / MP field (`FUN_80034B78(v, 4, ..)`), clamped at
/// `9999`.
const PAIR_DIGITS: u8 = 4;
const PAIR_CLAMP: i32 = 9999;
/// Digit width of a stat field (`FUN_80034B78(v, 3, ..)`), clamped at `999`.
const STAT_DIGITS: u8 = 3;
const STAT_CLAMP: i32 = 999;

/// One draw a compare panel emits, in retail emit order.
#[derive(Debug, Clone, PartialEq)]
pub enum ComparePanelField {
    /// A string draw (`FUN_80036888`): the character name, a stat label, or
    /// one of window 41's status notes. The ink tells them apart.
    Text {
        x: i32,
        y: i32,
        text: String,
        ink: u8,
    },
    /// A UI-icon-atlas sprite (`FUN_8002C488`). Not a font glyph - the host
    /// owns the atlas, exactly as the status page's LV / HP / MP icons.
    Icon { x: i32, y: i32, id: u8 },
    /// A blank-padded decimal field (`FUN_80034B78`).
    Number {
        x: i32,
        y: i32,
        value: i32,
        digits: u8,
        ink: u8,
    },
    /// A rise / fall arrow glyph (`FUN_8003C1F8`).
    Arrow { x: i32, y: i32, glyph: u8, ink: u8 },
}

/// Window 25's view: the active character's compare panel.
#[derive(Debug, Clone, Copy)]
pub struct EquipComparePanelView<'a> {
    /// Display name from the character record `+0x2A7`.
    pub name: &'a str,
    /// The live block (`0x801EF080`).
    pub current: EquipStatBlock,
    /// The trial-equip block (`0x801EF0A0`).
    pub candidate: EquipStatBlock,
    /// HP maximum straight off the record (`+0x104`). The HP row prints
    /// **this**, not `current.hp` - only the delta column reads the block.
    pub hp_max: u16,
    /// MP maximum straight off the record (`+0x108`), same asymmetry.
    pub mp_max: u16,
    /// Row set, from [`CompareRows::from_category`].
    pub rows: CompareRows,
    /// The three stat-row labels in emit order. Unused by
    /// [`CompareRows::HpMp`], which draws icons instead.
    pub labels: [&'a str; 3],
}

/// Build window 25's field list at content origin `pen`.
///
/// PORT: FUN_801d1290
pub fn equip_compare_panel_fields(
    view: &EquipComparePanelView<'_>,
    pen: (i32, i32),
) -> Vec<ComparePanelField> {
    let (x, y) = pen;
    let mut out = Vec::new();
    out.push(ComparePanelField::Text {
        x,
        y,
        text: view.name.to_string(),
        ink: INK_DEFAULT,
    });

    match view.rows {
        CompareRows::HpMp => {
            let pairs = [
                (ICON_HP, view.hp_max, 0usize),
                (ICON_MP, view.mp_max, 1usize),
            ];
            for (row, ((icon_a, icon_b), max, word)) in pairs.into_iter().enumerate() {
                let ry = y + ROW0_DY + row as i32 * ROW_PITCH;
                out.push(ComparePanelField::Icon {
                    x: x + W25_ICON_X,
                    y: ry + ICON_DY,
                    id: icon_a,
                });
                out.push(ComparePanelField::Icon {
                    x: x + W25_ICON2_X,
                    y: ry + ICON_DY,
                    id: icon_b,
                });
                out.push(ComparePanelField::Number {
                    x: x + W25_PAIR_VALUE_X,
                    y: ry,
                    value: i32::from(max),
                    digits: PAIR_DIGITS,
                    ink: INK_DEFAULT,
                });
                push_delta(
                    &mut out,
                    view.current.word(word),
                    view.candidate.word(word),
                    (x + W25_PAIR_ARROW_X, x + W25_PAIR_DELTA_X, ry),
                    PAIR_DIGITS,
                    PAIR_CLAMP,
                );
            }
        }
        rows => {
            for (row, &word) in rows.word_indices().iter().enumerate() {
                let ry = y + ROW0_DY + row as i32 * ROW_PITCH;
                out.push(ComparePanelField::Text {
                    x: x + W25_LABEL_X,
                    y: ry,
                    text: view.labels[row].to_string(),
                    ink: INK_DEFAULT,
                });
                out.push(ComparePanelField::Number {
                    x: x + W25_VALUE_X,
                    y: ry,
                    value: view.current.word(word).min(STAT_CLAMP),
                    digits: STAT_DIGITS,
                    ink: INK_DEFAULT,
                });
                push_delta(
                    &mut out,
                    view.current.word(word),
                    view.candidate.word(word),
                    (x + W25_ARROW_X, x + W25_DELTA_X, ry),
                    STAT_DIGITS,
                    STAT_CLAMP,
                );
            }
        }
    }
    out
}

/// The arrow + candidate-value pair both painters emit when a stat moves.
/// Retail stages ink `6` and glyph `4` when the candidate is strictly
/// higher, ink `1` and glyph `5` otherwise, then restores ink `7` for the
/// number. Nothing is emitted when the two values are equal.
fn push_delta(
    out: &mut Vec<ComparePanelField>,
    current: i32,
    candidate: i32,
    cols: (i32, i32, i32),
    digits: u8,
    clamp: i32,
) {
    let (arrow_x, delta_x, y) = cols;
    if candidate == current {
        return;
    }
    let (glyph, ink) = if current < candidate {
        (ARROW_RISE, INK_RISE)
    } else {
        (ARROW_FALL, INK_FALL)
    };
    out.push(ComparePanelField::Arrow {
        x: arrow_x,
        y,
        glyph,
        ink,
    });
    out.push(ComparePanelField::Number {
        x: delta_x,
        y,
        value: candidate.min(clamp),
        digits,
        ink: INK_DEFAULT,
    });
}

/// What window 41 has to say about one party member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartyCompareOutcome<'a> {
    /// The staged item is already in one of the member's eight equip bytes
    /// (`record +0x196..+0x19D`). Retail prints one note and stops.
    Equipped(&'a str),
    /// The equip record's `+6` character mask rejects this member.
    CannotEquip(&'a str),
    /// The ATK / UDF / LDF triple. `candidate` is `None` for a non-equipment
    /// staged id, where retail prints the current values with no arrows.
    Stats {
        current: EquipStatBlock,
        candidate: Option<EquipStatBlock>,
        labels: [&'a str; 3],
    },
}

/// One party member's row block in window 41.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartyCompareMemberView<'a> {
    /// Display name from the character record `+0x2A7`.
    pub name: &'a str,
    /// What the panel shows under the name.
    pub outcome: PartyCompareOutcome<'a>,
}

/// Build window 41's field list at content origin `pen`. Members are drawn
/// in party-roster order (`0x80084598`), `0x37` scanlines apart.
///
/// PORT: FUN_801d4c28
pub fn party_compare_panel_fields(
    members: &[PartyCompareMemberView<'_>],
    pen: (i32, i32),
) -> Vec<ComparePanelField> {
    let (x, y) = pen;
    let mut out = Vec::new();
    for (i, member) in members.iter().enumerate() {
        let my = y + i as i32 * W41_MEMBER_PITCH;
        out.push(ComparePanelField::Text {
            x,
            y: my,
            text: member.name.to_string(),
            ink: INK_DEFAULT,
        });
        match member.outcome {
            PartyCompareOutcome::Equipped(text) => out.push(ComparePanelField::Text {
                x: x + W41_NOTE_DX,
                y: my + W41_NOTE_DY,
                text: text.to_string(),
                ink: INK_EQUIPPED,
            }),
            PartyCompareOutcome::CannotEquip(text) => out.push(ComparePanelField::Text {
                x: x + W41_NOTE_DX,
                y: my + W41_NOTE_DY,
                text: text.to_string(),
                ink: INK_CANNOT_EQUIP,
            }),
            PartyCompareOutcome::Stats {
                current,
                candidate,
                labels,
            } => {
                for (row, &word) in CompareRows::AtkUdfLdf.word_indices().iter().enumerate() {
                    let ry = my + W41_ROW0_DY + row as i32 * ROW_PITCH;
                    out.push(ComparePanelField::Text {
                        x: x + W41_LABEL_X,
                        y: ry,
                        text: labels[row].to_string(),
                        ink: INK_DEFAULT,
                    });
                    out.push(ComparePanelField::Number {
                        x: x + W41_VALUE_X,
                        y: ry,
                        value: current.word(word).min(STAT_CLAMP),
                        digits: STAT_DIGITS,
                        ink: INK_DEFAULT,
                    });
                    if let Some(cand) = candidate {
                        push_delta(
                            &mut out,
                            current.word(word),
                            cand.word(word),
                            (x + W41_ARROW_X, x + W41_DELTA_X, ry),
                            STAT_DIGITS,
                            STAT_CLAMP,
                        );
                    }
                }
            }
        }
    }
    out
}

/// Resolve a compare-panel ink-staging id to an RGBA tint. `7` / `6` / `1`
/// are the same string-CLUT rows the records screen pins; `4` and `9` are
/// the two note colours and fall back to white and orange respectively.
pub fn compare_panel_ink(staging: u8) -> [f32; 4] {
    match staging {
        INK_DEFAULT => crate::MENU_TEXT_WHITE,
        INK_RISE => crate::MENU_TEXT_GOLD,
        INK_FALL => crate::MENU_TEXT_TEAL,
        INK_CANNOT_EQUIP => crate::MENU_TEXT_ORANGE,
        _ => crate::MENU_TEXT_WHITE,
    }
}

/// Fixed decimal-cell pitch of `FUN_80034B78` (8 px per digit).
const NUM_CELL_W: i32 = 8;

/// Render a field list to [`TextDraw`]s. [`ComparePanelField::Icon`] is
/// dropped - it is a UI-icon-atlas sprite, not a font glyph, and the host
/// owns that atlas. [`ComparePanelField::Arrow`] renders as an ASCII
/// stand-in until the separator-glyph page is uploaded.
///
/// PORT: FUN_801d1290 / FUN_801d4c28 (the text half of both)
pub fn compare_panel_draws_for(
    font: &legaia_font::Font,
    fields: &[ComparePanelField],
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for field in fields {
        match field {
            ComparePanelField::Text { x, y, text, ink } => {
                out.extend(text_draws_for(
                    &font.layout_ascii(text),
                    (*x, *y),
                    compare_panel_ink(*ink),
                ));
            }
            ComparePanelField::Icon { .. } => {}
            ComparePanelField::Number {
                x,
                y,
                value,
                digits,
                ink,
            } => {
                let s = value.max(&0).to_string();
                let len = s.len() as i32;
                let color = compare_panel_ink(*ink);
                for (i, ch) in s.chars().enumerate() {
                    let cell = (i32::from(*digits) - len + i as i32).max(0);
                    out.extend(text_draws_for(
                        &font.layout_ascii(&ch.to_string()),
                        (*x + cell * NUM_CELL_W, *y),
                        color,
                    ));
                }
            }
            ComparePanelField::Arrow { x, y, glyph, ink } => {
                let ch = if *glyph == ARROW_RISE { '+' } else { '-' };
                out.extend(text_draws_for(
                    &font.layout_ascii(&ch.to_string()),
                    (*x, *y),
                    compare_panel_ink(*ink),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(atk: i32, udf: i32, ldf: i32) -> EquipStatBlock {
        EquipStatBlock {
            atk,
            udf,
            ldf,
            ..Default::default()
        }
    }

    #[test]
    fn category_split_matches_the_two_unsigned_compares() {
        for c in 0..6u8 {
            assert_eq!(CompareRows::from_category(c), CompareRows::HpMp);
        }
        for c in [6u8, 7, 8, 9, 13, 0x40, 0xFF] {
            assert_eq!(CompareRows::from_category(c), CompareRows::AtkUdfLdf);
        }
        for c in 10..=12u8 {
            assert_eq!(CompareRows::from_category(c), CompareRows::SpdIntAgl);
        }
    }

    #[test]
    fn word_indices_match_the_retail_block_offsets() {
        assert_eq!(CompareRows::HpMp.word_indices(), &[0, 1]);
        assert_eq!(CompareRows::SpdIntAgl.word_indices(), &[6, 7, 2]);
        assert_eq!(CompareRows::AtkUdfLdf.word_indices(), &[3, 4, 5]);
    }

    #[test]
    fn staged_category_only_applies_on_slot_rows_four_and_up() {
        let base = CompareCategoryInputs {
            slot_row: 3,
            staged_id: 0x20,
            staged_category: 11,
            equipped_id: 0,
            equipped_category: 0,
        };
        // Row 3 is below the guard: the default sentinel survives.
        assert_eq!(active_compare_category(base), CATEGORY_DEFAULT);
        assert_eq!(
            active_compare_category(CompareCategoryInputs {
                slot_row: 4,
                ..base
            }),
            11
        );
    }

    #[test]
    fn empty_hover_falls_back_to_the_equipped_item_on_any_row() {
        let inp = CompareCategoryInputs {
            slot_row: 0,
            staged_id: -1,
            staged_category: 11,
            equipped_id: 0x33,
            equipped_category: 3,
        };
        assert_eq!(active_compare_category(inp), 3);
        // An empty slot has nothing to fall back to.
        assert_eq!(
            active_compare_category(CompareCategoryInputs {
                equipped_id: 0,
                ..inp
            }),
            CATEGORY_DEFAULT
        );
    }

    #[test]
    fn window25_triple_lays_out_three_rows_thirteen_apart() {
        let view = EquipComparePanelView {
            name: "VAHN",
            current: block(50, 30, 25),
            candidate: block(50, 30, 25),
            hp_max: 0,
            mp_max: 0,
            rows: CompareRows::AtkUdfLdf,
            labels: ["ATK", "UDF", "LDF"],
        };
        let f = equip_compare_panel_fields(&view, (14, 40));
        // Name + 3 * (label, value); nothing moved, so no arrows.
        assert_eq!(f.len(), 7);
        assert!(matches!(
            &f[0],
            ComparePanelField::Text { x: 14, y: 40, .. }
        ));
        let ys: Vec<i32> = f
            .iter()
            .filter_map(|d| match d {
                ComparePanelField::Text { y, x: 30, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(ys, vec![40 + 0x10, 40 + 0x1D, 40 + 0x2A]);
    }

    #[test]
    fn window25_emits_a_rise_arrow_and_the_candidate_value() {
        let view = EquipComparePanelView {
            name: "VAHN",
            current: block(50, 30, 25),
            candidate: block(58, 30, 20),
            hp_max: 0,
            mp_max: 0,
            rows: CompareRows::AtkUdfLdf,
            labels: ["ATK", "UDF", "LDF"],
        };
        let f = equip_compare_panel_fields(&view, (0, 0));
        let arrows: Vec<_> = f
            .iter()
            .filter_map(|d| match d {
                ComparePanelField::Arrow { glyph, ink, y, .. } => Some((*glyph, *ink, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(
            arrows,
            vec![
                (ARROW_RISE, INK_RISE, 0x10),
                // UDF is unchanged - no arrow row at 0x1D.
                (ARROW_FALL, INK_FALL, 0x2A),
            ]
        );
        // The delta column carries the candidate value, not the current one.
        let deltas: Vec<i32> = f
            .iter()
            .filter_map(|d| match d {
                ComparePanelField::Number { x, value, .. } if *x == W25_DELTA_X => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec![58, 20]);
    }

    #[test]
    fn window25_hp_pair_prints_the_record_maxima_and_clamps_the_delta() {
        let view = EquipComparePanelView {
            name: "NOA",
            current: EquipStatBlock {
                hp: 120,
                mp: 40,
                ..Default::default()
            },
            candidate: EquipStatBlock {
                hp: 20000,
                mp: 40,
                ..Default::default()
            },
            hp_max: 120,
            mp_max: 40,
            rows: CompareRows::HpMp,
            labels: ["", "", ""],
        };
        let f = equip_compare_panel_fields(&view, (0, 0));
        let icons: Vec<u8> = f
            .iter()
            .filter_map(|d| match d {
                ComparePanelField::Icon { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(icons, vec![ICON_HP.0, ICON_HP.1, ICON_MP.0, ICON_MP.1]);
        let delta: Vec<i32> = f
            .iter()
            .filter_map(|d| match d {
                ComparePanelField::Number { x, value, .. } if *x == W25_PAIR_DELTA_X => {
                    Some(*value)
                }
                _ => None,
            })
            .collect();
        assert_eq!(delta, vec![PAIR_CLAMP]);
    }

    #[test]
    fn window41_notes_replace_the_stat_rows_entirely() {
        let members = [
            PartyCompareMemberView {
                name: "VAHN",
                outcome: PartyCompareOutcome::Equipped("Equipped"),
            },
            PartyCompareMemberView {
                name: "NOA",
                outcome: PartyCompareOutcome::CannotEquip("Cannot Equip"),
            },
        ];
        let f = party_compare_panel_fields(&members, (14, 46));
        assert_eq!(f.len(), 4);
        match &f[1] {
            ComparePanelField::Text { x, y, ink, .. } => {
                assert_eq!((*x, *y, *ink), (14 + 0x0C, 46 + 0x14, INK_EQUIPPED));
            }
            other => panic!("expected the note, got {other:?}"),
        }
        match &f[3] {
            ComparePanelField::Text { x, y, ink, .. } => {
                assert_eq!(
                    (*x, *y, *ink),
                    (14 + 0x0C, 46 + 0x37 + 0x14, INK_CANNOT_EQUIP)
                );
            }
            other => panic!("expected the second note, got {other:?}"),
        }
    }

    #[test]
    fn window41_without_a_candidate_draws_no_arrows() {
        let members = [PartyCompareMemberView {
            name: "GALA",
            outcome: PartyCompareOutcome::Stats {
                current: block(70, 40, 35),
                candidate: None,
                labels: ["ATK", "UDF", "LDF"],
            },
        }];
        let f = party_compare_panel_fields(&members, (0, 0));
        assert!(
            !f.iter()
                .any(|d| matches!(d, ComparePanelField::Arrow { .. }))
        );
        // Name + 3 * (label, value).
        assert_eq!(f.len(), 7);
    }

    #[test]
    fn window41_members_are_a_fixed_pitch_apart() {
        let stats = PartyCompareOutcome::Stats {
            current: block(1, 2, 3),
            candidate: None,
            labels: ["A", "B", "C"],
        };
        let members = [
            PartyCompareMemberView {
                name: "A",
                outcome: stats,
            },
            PartyCompareMemberView {
                name: "B",
                outcome: stats,
            },
            PartyCompareMemberView {
                name: "C",
                outcome: stats,
            },
        ];
        let f = party_compare_panel_fields(&members, (14, 46));
        let names: Vec<i32> = f
            .iter()
            .filter_map(|d| match d {
                ComparePanelField::Text { x: 14, y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec![46, 46 + 0x37, 46 + 0x6E]);
    }
}
