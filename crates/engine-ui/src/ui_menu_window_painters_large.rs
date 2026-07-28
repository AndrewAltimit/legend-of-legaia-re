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
//! the character record ([`EquipStatBlock::from_character_record`] - a
//! frameless 32-instruction leaf, the menu overlay's first routine), and
//! `FUN_801CF650` sums the equipment bonuses into it. The trial-equip mirror
//! lives one block later at `0x801EF0A0`:
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
//! PORT: FUN_801cf5d0 - the seeder that fills [`EquipStatBlock`]
//! REF: FUN_801cf650 - the equipment-bonus summer over the same block
//!
//! Source: `ghidra/scripts/funcs/overlay_menu_801d1290.txt`,
//! `ghidra/scripts/funcs/overlay_menu_801d4c28.txt`.
//!
//! # Wiring
//!
//! `crate::ui_menu_window_dispatch` resolves both descriptors to these
//! painters, and the screen that opens them is the shop's **equipment-buy
//! recipient flow** (retail sub-screen `0x1C`, `FUN_801DB380`): the buy
//! list's kind dispatch (`engine-core`'s `shop::buy_list_confirm_route`)
//! routes an equipment row into `shop::BuyRecipientSession`, and
//! [`recipient_picker_draws_for`] below paints windows 25 / 41 beside the
//! recipient list. That composition is what both hosts call - the browser
//! play page through `web-viewer::play_shop::recipient_window_draws`, the
//! native window through `window/shop_windows.rs::recipient_window_draws` -
//! so neither can grow a row order, a cursor row or a note string the other
//! lacks. The pause equip flow (`legaia_engine_core::equip_session`) still
//! draws its capture-pinned window set (ids 2 / 21 / 22 / 23) and never
//! opens these - window 25's rect `(14, 40, 144, 52)` overlaps the party
//! window 21 it would have to replace.

use crate::ui_menu_window_painters::{
    ChoiceFlags, EquipTargetRow, PainterRect, PainterSprite, equip_target_list_draws_for,
};
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

/// Character-record byte offsets `FUN_801CF5D0` seeds the block from, in
/// block-word order.
///
/// Retail addresses them off `0x80084140 + slot*0x414`; the record base is
/// `0x80084708`, so the same fields are `record + 0x104 / 0x108 / 0x110 ..`,
/// the `char +0x1NN` column of the table above. Each is read with `lhu`
/// (zero-extended) and stored as a full word.
pub const EQUIP_STAT_RECORD_OFFSETS: [usize; 8] =
    [0x104, 0x108, 0x110, 0x112, 0x114, 0x116, 0x118, 0x11A];

impl EquipStatBlock {
    /// Seed the block from one `0x414`-byte character record.
    ///
    /// The port of the seeder: the eight offsets, their order, and the
    /// zero-extending `u16 -> u32` widening. The record *selection* -
    /// retail's `slot * 0x414` off `0x80084140` - is the caller's slice, so
    /// this takes the record itself rather than a slot index.
    ///
    /// `None` when the slice is too short to hold the last field.
    ///
    /// PORT: FUN_801cf5d0
    pub fn from_character_record(record: &[u8]) -> Option<Self> {
        let field = |off: usize| -> Option<i32> {
            let b = record.get(off..off + 2)?;
            Some(i32::from(u16::from_le_bytes([b[0], b[1]])))
        };
        let w = EQUIP_STAT_RECORD_OFFSETS;
        Some(Self {
            hp: field(w[0])?,
            mp: field(w[1])?,
            agl: field(w[2])?,
            atk: field(w[3])?,
            udf: field(w[4])?,
            ldf: field(w[5])?,
            spd: field(w[6])?,
            int: field(w[7])?,
        })
    }

    /// Word `i` of the block (`0..=7`), matching the retail `0x801EF080 +
    /// i*4` addressing the painters index with. Out-of-range reads `0`.
    ///
    /// Named `stat_word` rather than `word` on purpose: the port catalog
    /// resolves calls by name with no receiver inference, and a method
    /// called `word` collects an edge from every unrelated `word(...)` in
    /// the workspace - which reported this whole module as wired.
    pub fn stat_word(&self, i: usize) -> i32 {
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
                    view.current.stat_word(word),
                    view.candidate.stat_word(word),
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
                    value: view.current.stat_word(word).min(STAT_CLAMP),
                    digits: STAT_DIGITS,
                    ink: INK_DEFAULT,
                });
                push_delta(
                    &mut out,
                    view.current.stat_word(word),
                    view.candidate.stat_word(word),
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
                        value: current.stat_word(word).min(STAT_CLAMP),
                        digits: STAT_DIGITS,
                        ink: INK_DEFAULT,
                    });
                    if let Some(cand) = candidate {
                        push_delta(
                            &mut out,
                            current.stat_word(word),
                            cand.stat_word(word),
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

// ---------------------------------------------------------------------
// The equipment-buy recipient sub-screen (windows 36 / 25 / 41)
// ---------------------------------------------------------------------

/// Window 36's header row - the picker's **bag row** (marker index 0), not a
/// caption: confirming it buys one copy into the inventory. Retail's own
/// string is a menu-overlay rodata literal; the port stages an
/// engine-authored line in the same slot so the translation layer owns the
/// text.
pub const RECIPIENT_HEADING: &str = "Put in bag";

/// Window 41's note for a member already wearing the staged item.
pub const RECIPIENT_NOTE_EQUIPPED: &str = "Equipped";

/// Window 41's note for a member the item's character mask excludes.
pub const RECIPIENT_NOTE_CANNOT_EQUIP: &str = "Cannot equip";

/// Stat-row labels for [`CompareRows::AtkUdfLdf`], in emit order.
pub const COMPARE_LABELS_ATK: [&str; 3] = ["ATK", "UDF", "LDF"];

/// Stat-row labels for [`CompareRows::SpdIntAgl`], in emit order.
pub const COMPARE_LABELS_SPD: [&str; 3] = ["SPD", "INT", "AGL"];

/// The three window rects the recipient sub-screen paints into, each `None`
/// when the disc descriptor for that id resolves to a different renderer (or
/// the window table did not parse at all).
///
/// A host fills this from the menu-overlay window table through
/// `ui_menu_window_dispatch::painter_at`, so an id whose descriptor names a
/// different routine is skipped rather than mis-drawn.
///
/// REF: FUN_801d56fc - window 36's renderer, ported as
/// [`crate::ui_menu_window_painters::equip_target_list_draws_for`]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecipientWindowRects {
    /// Window 36 (`FUN_801D56FC`) - the recipient row list.
    pub target_list: Option<PainterRect>,
    /// Window 25 (`FUN_801D1290`) - the highlighted member's compare panel.
    pub active_compare: Option<PainterRect>,
    /// Window 41 (`FUN_801D4C28`) - the party-wide compare column.
    pub party_compare: Option<PainterRect>,
}

/// One party member as the recipient sub-screen sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipientMemberView<'a> {
    /// Display name from the character record `+0x2A7`.
    pub name: &'a str,
    /// `false` when the staged item's character mask excludes this member.
    pub equippable: bool,
    /// The member already wears the staged item, so window 41 prints the
    /// "Equipped" note instead of a stat block.
    pub already_equipped: bool,
    /// Live derived-stat block (`0x801EF080`).
    pub current: EquipStatBlock,
    /// Trial-equip block (`0x801EF0A0`): `current` plus the staged item's
    /// modifiers minus whatever the item would displace.
    pub candidate: EquipStatBlock,
    /// HP maximum straight off the record (`+0x104`).
    pub hp_max: u16,
    /// MP maximum straight off the record (`+0x108`).
    pub mp_max: u16,
}

/// The whole equipment-buy recipient sub-screen (retail `0x1C`,
/// `FUN_801DB380`) as a model: one row per party member behind a bag row,
/// plus the two stat-compare readouts beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipientPickerView<'a> {
    /// Heading of window 36's list.
    pub heading: &'a str,
    /// Session cursor word (`DAT_801E46C0`): row 0 is the bag, rows `1..`
    /// are party members, and the high bits carry the marker flags.
    pub cursor: u32,
    /// Party members in roster order.
    pub members: &'a [RecipientMemberView<'a>],
    /// Compare-category byte resolved for the staged item (equip record
    /// `+5`). Every retail equipment record carries [`CATEGORY_DEFAULT`], so
    /// a host without the raw table may pass that constant.
    pub staged_category: u8,
}

/// Paint the recipient sub-screen's three windows.
///
/// This is the whole screen, not one window: the shared composition both
/// hosts call, so the browser play page and the native window cannot drift
/// apart in row order, cursor rows, category resolution or note strings. A
/// window whose rect is `None` is skipped.
///
/// The returned sprites are window 36's marker requests
/// ([`PainterSprite`]); the host draws them from its own cursor atlas (or an
/// ASCII stand-in while that page is missing).
///
/// PORT: FUN_801db380 (the sub-screen's draw half; the session state machine
/// is `legaia_engine_core::shop::BuyRecipientSession`)
pub fn recipient_picker_draws_for(
    font: &legaia_font::Font,
    rects: RecipientWindowRects,
    view: &RecipientPickerView<'_>,
) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let mut out = Vec::new();
    let mut sprites = Vec::new();

    // Window 36 - the recipient list. The header row is the bag row
    // (marker index 0); each member row takes marker index i + 1.
    if let Some(rect) = rects.target_list {
        let rows: Vec<(EquipTargetRow, &str)> = view
            .members
            .iter()
            .enumerate()
            .map(|(i, m)| {
                (
                    EquipTargetRow {
                        member_class: i as u8,
                        equippable: m.equippable,
                    },
                    m.name,
                )
            })
            .collect();
        let (text, marks) =
            equip_target_list_draws_for(font, rect, view.heading, &rows, ChoiceFlags(view.cursor));
        out.extend(text);
        sprites.extend(marks);
    }

    // Window 41 - the party-wide compare column.
    if let Some(rect) = rects.party_compare {
        let views: Vec<PartyCompareMemberView<'_>> = view
            .members
            .iter()
            .map(|m| PartyCompareMemberView {
                name: m.name,
                outcome: if m.already_equipped {
                    PartyCompareOutcome::Equipped(RECIPIENT_NOTE_EQUIPPED)
                } else if !m.equippable {
                    PartyCompareOutcome::CannotEquip(RECIPIENT_NOTE_CANNOT_EQUIP)
                } else {
                    PartyCompareOutcome::Stats {
                        current: m.current,
                        candidate: Some(m.candidate),
                        labels: COMPARE_LABELS_ATK,
                    }
                },
            })
            .collect();
        let fields = party_compare_panel_fields(&views, (rect.x, rect.y));
        out.extend(compare_panel_draws_for(font, &fields));
    }

    // Window 25 - the highlighted member's own compare panel. Row 0 of the
    // picker is the bag, so only rows `1..` have a member to compare.
    let row = (view.cursor & 0xFFF) as usize;
    if let (Some(rect), Some(m)) = (
        rects.active_compare,
        row.checked_sub(1).and_then(|i| view.members.get(i)),
    ) {
        // The staged-item detail arm: the picker always has a staged
        // equipment id, retail's `slot_row >= 4` case. `staged_id` is
        // positive there, so the "nothing staged" fallback (which is the
        // only consumer of the equipped item's category) cannot fire.
        let category = active_compare_category(CompareCategoryInputs {
            slot_row: 4,
            staged_id: 1,
            staged_category: view.staged_category,
            equipped_id: 0,
            equipped_category: CATEGORY_DEFAULT,
        });
        let rows = CompareRows::from_category(category);
        let panel = EquipComparePanelView {
            name: m.name,
            current: m.current,
            candidate: m.candidate,
            hp_max: m.hp_max,
            mp_max: m.mp_max,
            rows,
            labels: match rows {
                CompareRows::SpdIntAgl => COMPARE_LABELS_SPD,
                _ => COMPARE_LABELS_ATK,
            },
        };
        let fields = equip_compare_panel_fields(&panel, (rect.x, rect.y));
        out.extend(compare_panel_draws_for(font, &fields));
    }

    (out, sprites)
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

    /// The seeder reads eight `u16`s at fixed record offsets and widens them
    /// zero-extended - a value with the high bit set must not come back
    /// negative, which is the one way an `lh`-vs-`lhu` slip would show.
    #[test]
    fn the_seeder_zero_extends_every_field() {
        let mut rec = vec![0u8; 0x414];
        for (i, off) in EQUIP_STAT_RECORD_OFFSETS.iter().enumerate() {
            let v = 0x8000u16 + i as u16;
            rec[*off..*off + 2].copy_from_slice(&v.to_le_bytes());
        }
        let b = EquipStatBlock::from_character_record(&rec).expect("full record");
        for i in 0..8 {
            assert_eq!(b.stat_word(i), 0x8000 + i as i32, "word {i}");
        }
    }

    #[test]
    fn the_seeder_reads_the_fields_in_block_order() {
        let mut rec = vec![0u8; 0x414];
        // HP 120 at +0x104, ATK 33 at +0x112, INT 9 at +0x11A.
        rec[0x104..0x106].copy_from_slice(&120u16.to_le_bytes());
        rec[0x112..0x114].copy_from_slice(&33u16.to_le_bytes());
        rec[0x11A..0x11C].copy_from_slice(&9u16.to_le_bytes());
        let b = EquipStatBlock::from_character_record(&rec).expect("full record");
        assert_eq!((b.hp, b.atk, b.int), (120, 33, 9));
        assert_eq!(b.mp, 0);
        // A record that stops before the last field resolves to nothing
        // rather than to a partly-seeded block.
        assert!(EquipStatBlock::from_character_record(&rec[..0x11B]).is_none());
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

    // -- the recipient sub-screen (windows 36 / 25 / 41) ----------------

    const W36: PainterRect = PainterRect {
        x: 138,
        y: 98,
        w: 168,
        h: 52,
    };
    const W25: PainterRect = PainterRect {
        x: 14,
        y: 40,
        w: 144,
        h: 52,
    };
    const W41: PainterRect = PainterRect {
        x: 14,
        y: 46,
        w: 108,
        h: 158,
    };

    fn member(name: &str, equippable: bool, already: bool) -> RecipientMemberView<'_> {
        RecipientMemberView {
            name,
            equippable,
            already_equipped: already,
            current: block(10, 20, 30),
            candidate: block(15, 20, 25),
            hp_max: 100,
            mp_max: 40,
        }
    }

    fn all_rects() -> RecipientWindowRects {
        RecipientWindowRects {
            target_list: Some(W36),
            active_compare: Some(W25),
            party_compare: Some(W41),
        }
    }

    /// Cursor row 0 is the bag, so window 25 has nothing to compare and
    /// draws nothing - the one row-indexing slip that would put member 0's
    /// panel under the bag row.
    #[test]
    fn the_bag_row_draws_no_active_compare_panel() {
        let font = legaia_font::Font::placeholder();
        let members = [member("A", true, false), member("B", true, false)];
        let view = RecipientPickerView {
            heading: "h",
            cursor: 0,
            members: &members,
            staged_category: CATEGORY_DEFAULT,
        };
        let bag_only = RecipientWindowRects {
            target_list: None,
            active_compare: Some(W25),
            party_compare: None,
        };
        let (draws, _) = recipient_picker_draws_for(&font, bag_only, &view);
        assert!(draws.is_empty());

        // Row 1 is member 0, and now the panel paints.
        let view = RecipientPickerView { cursor: 1, ..view };
        let (draws, _) = recipient_picker_draws_for(&font, bag_only, &view);
        assert!(!draws.is_empty());
    }

    /// Every member row takes marker index `i + 1`, because window 36's
    /// header row *is* the bag row.
    #[test]
    fn member_rows_take_marker_index_one_and_up() {
        let font = legaia_font::Font::placeholder();
        let members = [member("A", true, false), member("B", false, false)];
        let view = RecipientPickerView {
            heading: "h",
            cursor: 2,
            members: &members,
            staged_category: CATEGORY_DEFAULT,
        };
        let (_, sprites) = recipient_picker_draws_for(&font, all_rects(), &view);
        assert_eq!(sprites.len(), 1);
        assert_eq!(
            sprites[0].y,
            W36.y + 2 * crate::ui_menu_window_painters::PAINTER_ROW_PITCH
        );
    }

    /// Window 41 swaps a member's stat block for a note in two cases, and
    /// "already equipped" wins over "cannot equip".
    #[test]
    fn window_41_notes_replace_the_stat_block() {
        let members = [
            member("A", true, false),
            member("B", false, false),
            member("C", false, true),
        ];
        let view = RecipientPickerView {
            heading: "h",
            cursor: 0,
            members: &members,
            staged_category: CATEGORY_DEFAULT,
        };
        let font = legaia_font::Font::placeholder();
        let rects = RecipientWindowRects {
            target_list: None,
            active_compare: None,
            party_compare: Some(W41),
        };
        // Drawn text is font quads, so assert on the field list the same
        // call builds - rebuilt here through the public party painter.
        let (draws, _) = recipient_picker_draws_for(&font, rects, &view);
        assert!(!draws.is_empty());

        let views = [
            PartyCompareMemberView {
                name: "A",
                outcome: PartyCompareOutcome::Stats {
                    current: block(10, 20, 30),
                    candidate: Some(block(15, 20, 25)),
                    labels: COMPARE_LABELS_ATK,
                },
            },
            PartyCompareMemberView {
                name: "B",
                outcome: PartyCompareOutcome::CannotEquip(RECIPIENT_NOTE_CANNOT_EQUIP),
            },
            PartyCompareMemberView {
                name: "C",
                outcome: PartyCompareOutcome::Equipped(RECIPIENT_NOTE_EQUIPPED),
            },
        ];
        let expect =
            compare_panel_draws_for(&font, &party_compare_panel_fields(&views, (W41.x, W41.y)));
        assert_eq!(draws.len(), expect.len());
    }

    /// Every retail equipment record's `+5` byte is the `0x40` sentinel, so
    /// the recipient panel always shows the ATK / UDF / LDF triple. A host
    /// that cannot resolve the byte may pass [`CATEGORY_DEFAULT`] and get
    /// the identical screen; this pins that equivalence.
    #[test]
    fn the_equipment_category_sentinel_selects_the_atk_triple() {
        let font = legaia_font::Font::placeholder();
        let members = [member("A", true, false)];
        let rects = RecipientWindowRects {
            target_list: None,
            active_compare: Some(W25),
            party_compare: None,
        };
        let mk = |cat| RecipientPickerView {
            heading: "h",
            cursor: 1,
            members: &members,
            staged_category: cat,
        };
        let (a, _) = recipient_picker_draws_for(&font, rects, &mk(CATEGORY_DEFAULT));
        let (b, _) = recipient_picker_draws_for(&font, rects, &mk(0x40));
        assert_eq!(a.len(), b.len());
        assert_eq!(
            CompareRows::from_category(CATEGORY_DEFAULT),
            CompareRows::AtkUdfLdf
        );
    }

    /// A window whose descriptor did not resolve is skipped, not drawn at
    /// the origin - the failure a `PainterRect::default()` fallback would
    /// hide.
    #[test]
    fn an_unresolved_window_is_skipped() {
        let font = legaia_font::Font::placeholder();
        let members = [member("A", true, false)];
        let view = RecipientPickerView {
            heading: "h",
            cursor: 1,
            members: &members,
            staged_category: CATEGORY_DEFAULT,
        };
        let (none, sprites) =
            recipient_picker_draws_for(&font, RecipientWindowRects::default(), &view);
        assert!(none.is_empty());
        assert!(sprites.is_empty());
        let (all, _) = recipient_picker_draws_for(&font, all_rects(), &view);
        assert!(!all.is_empty());
    }
}
