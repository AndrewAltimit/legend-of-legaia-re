//! Shop / item windows - 34, 36, 37: descriptions, the equip target list and the buy / sell quantity panels.

use super::*;

// ---------------------------------------------------------------------
// Shop / item windows - 34, 36, 37
// ---------------------------------------------------------------------

/// Where a window-34 description line comes from.
///
/// `FUN_801D4A80` branches on the item record's kind byte (`+0` of
/// `0x80074368 + id*12`). Kind `2` - the accessory ("Goods") class - does
/// **not** use the item's own description word. It reads the item-effect
/// record `0x800752C0 + effect*4`, takes the passive index at `+3`, and
/// only if that index is below `0x40` does it draw the passive's
/// description from `0x8007625C + index*12 + 8`. An accessory whose
/// passive index is `0x40` or above draws no description at all - the
/// bound is checked twice, unsigned then signed, and both arms bail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptionSource {
    /// The item record's own description word (`+8`).
    Item,
    /// Accessory passive `index`, whose description lives in the
    /// name/description table at `0x8007625C`.
    AccessoryPassive(u8),
    /// Kind `2` with an out-of-range passive index: nothing is drawn.
    None,
}

/// Item kind byte that routes a description through the passive table.
pub const ITEM_KIND_ACCESSORY: u8 = 2;
/// Passive indices at or above this draw no description.
pub const ACCESSORY_PASSIVE_LIMIT: u8 = 0x40;

/// Resolve which description a window-34 draw would use.
///
/// REF: FUN_801D4A80 (the kind-2 branch and its double bound check)
pub fn description_source(item_kind: u8, passive_index: u8) -> DescriptionSource {
    if item_kind != ITEM_KIND_ACCESSORY {
        return DescriptionSource::Item;
    }
    if passive_index >= ACCESSORY_PASSIVE_LIMIT {
        return DescriptionSource::None;
    }
    DescriptionSource::AccessoryPassive(passive_index)
}

/// Window 34: item name, owned count, and the description line below.
///
/// `FUN_801D4A80` draws nothing at all when the selected **item id**
/// `DAT_801E46B0` is not positive - that word is the id the routine
/// multiplies by 12 to reach `0x80074368`, not a list row. Otherwise the
/// name goes at the content
/// origin, then the two-digit owned count at `WX + 0x94` - **both in the
/// accent pen**, which retail stages once before the name and restores only
/// after the count - and the description one row pitch down at `WX + 8` back
/// in the default pen, routed through [`description_source`]. The owned
/// count comes back from `FUN_80042EE0`; the sentinel `0x100` means "not
/// held" and draws `0`.
///
/// PORT: FUN_801D4A80
pub fn item_description_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    selected: bool,
    name: &str,
    owned: u8,
    description: &str,
) -> Vec<TextDraw> {
    if !selected {
        return Vec::new();
    }
    let mut out = text_draws_for(
        &font.layout_ascii(name),
        (rect.x, rect.y),
        PAINTER_INK_ACCENT,
    );
    out.extend(digits_draws(
        font,
        owned as u64,
        rect.x + 0x94,
        rect.y,
        2,
        PAINTER_INK_ACCENT,
    ));
    if !description.is_empty() {
        out.extend(broken_text_draws_for(
            font,
            description,
            (rect.x + 8, rect.y + PAINTER_ROW_PITCH),
            MENU_TEXT_WHITE,
        ));
    }
    out
}

/// A string through the menu text printer's `'|'` line break: each segment
/// drawn [`PAINTER_ROW_PITCH`] below the last, back at `pen.0`.
///
/// Retail's printer `FUN_80036888` resets x to the line start and adds `0xE`
/// to y on a `0x7C` byte (`0x80036924..0x80036938`), so the description words
/// the shop's info and detail windows hand it (through `FUN_800337B0` /
/// `FUN_8003CD00`) print on two rows. The port used to keep the first segment
/// only, on both hosts.
///
/// REF: FUN_80036888
pub fn broken_text_draws_for(
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
    ink: [f32; 4],
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for (i, line) in text.split(['|', '\n']).enumerate() {
        out.extend(text_draws_for(
            &font.layout_ascii(line),
            (pen.0, pen.1 + PAINTER_ROW_PITCH * i as i32),
            ink,
        ));
    }
    out
}

/// Sentinel `FUN_80042EE0` returns when the selected item is not held.
pub const OWNED_COUNT_ABSENT: i32 = 0x100;

/// Owned count the window-34 draw uses for a lookup result.
///
/// REF: FUN_801D4A80 (`li v0,0x100; beq a0,v0 -> clear a0`)
pub fn owned_count_or_zero(lookup: i32, count: u8) -> u8 {
    if lookup == OWNED_COUNT_ABSENT {
        0
    } else {
        count
    }
}

/// One row of the window-36 equip-target list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipTargetRow {
    /// Party-member class byte (`0x80084598 + i`).
    pub member_class: u8,
    /// `false` when the selected equipment's character mask excludes this
    /// member. Retail still draws the row, in the dim ink `0`.
    pub equippable: bool,
}

/// Character-mask bit for a party-member class.
///
/// `FUN_801D56FC` does not compute `1 << class`; it indexes a four-byte
/// overlay table at `0x801E43F0`. On the retail disc that table reads
/// `01 02 04 00`, so it agrees with the shift for classes `0..=2` and
/// gives class `3` a mask of **zero** - the fourth party member matches no
/// equipment through this window.
///
/// REF: FUN_801D56FC
pub fn equip_class_mask(member_class: u8) -> u8 {
    match member_class {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 0,
    }
}

/// Whether the equipment whose character mask is `equip_mask`
/// (`legaia_game_tables::equip_stats::EquipBonus::equip_mask`) can be worn by a
/// member of `member_class`.
///
/// REF: FUN_801D56FC (`and v0,s6,v0; bne v0,zero` - a zero result stages
/// the dim ink `0` rather than skipping the row)
pub fn equip_row_enabled(equip_mask: u8, member_class: u8) -> bool {
    equip_mask & equip_class_mask(member_class) != 0
}

/// Window 36: a header row plus one row per party member, each carrying
/// the shared choice marker.
///
/// `FUN_801D56FC` puts the header at `WX + 0x18` and its marker at
/// `WX + 4`; every member row repeats that pair one row pitch further
/// down, with row `i` taking marker index `i + 1`. The marker chain is the
/// same [`ChoiceFlags`] branch the options windows use, driven here by
/// `DAT_801E46C0` rather than `DAT_801E46D0`.
///
/// PORT: FUN_801D56FC
///
/// Wired on both hosts through the shared composition
/// [`crate::recipient_picker_draws_for`]: the shop's equipment-buy recipient
/// flow (`engine-core`'s `menu_runtime` + `shop::BuyRecipientSession`) opens
/// this window over the parked buy list, and each host resolves the rect off
/// the disc window table and hands the model to that builder.
pub fn equip_target_list_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    header: &str,
    rows: &[(EquipTargetRow, &str)],
    flags: ChoiceFlags,
) -> (Vec<TextDraw>, Vec<PainterSprite>) {
    let label_x = rect.x + 0x18;
    let marker_x = rect.x + 4;
    let mut text = text_draws_for(
        &font.layout_ascii(header),
        (label_x, rect.y),
        MENU_TEXT_WHITE,
    );
    let mut sprites = Vec::new();
    if let Some(variant) = flags.marker_variant(0) {
        sprites.push(PainterSprite {
            sprite: 0,
            variant,
            x: marker_x,
            y: rect.y,
        });
    }
    for (i, (row, label)) in rows.iter().enumerate() {
        let y = rect.y + (i as i32 + 1) * PAINTER_ROW_PITCH;
        // A member the item's mask excludes keeps its row, in ink 0: the
        // renderer stages `7`, then stores zero into the ink global when
        // `mask & 0x801E43F0[char]` is clear (`sw zero,-0x4bac(s4)` at
        // `0x801D5838`) - the dim pen, as the Retock capture shows.
        let ink = if row.equippable {
            MENU_TEXT_WHITE
        } else {
            crate::MENU_TEXT_GREY
        };
        text.extend(text_draws_for(&font.layout_ascii(label), (label_x, y), ink));
        if let Some(variant) = flags.marker_variant(i as u32 + 1) {
            sprites.push(PainterSprite {
                sprite: 0,
                variant,
                x: marker_x,
                y,
            });
        }
    }
    (text, sprites)
}

/// Digit-field width the sell panel sizes its total to.
///
/// `FUN_801D5944` starts at 4 and widens on three unsigned comparisons
/// against the *unit* price: `>= 100` and `>= 1000` each add one, and
/// `>= 10000` **assigns** 5 before those two add to it. So the ladder is
/// 4 / 5 / 6 / 7 rather than a digit count - a four-digit price still
/// reserves six cells.
///
/// REF: FUN_801D5944
pub fn sell_total_digits(unit_price: u32) -> i32 {
    let mut n = 4;
    if unit_price >= 10_000 {
        n = 5;
    }
    if unit_price >= 1_000 {
        n += 1;
    }
    if unit_price >= 100 {
        n += 1;
    }
    n
}

/// The gold total window 37 writes: quantity times the unit price,
/// halved.
///
/// The arithmetic is `mult` then `sra ..,1` - an arithmetic shift, so an
/// odd product truncates toward negative infinity. Products are
/// non-negative here, which makes it plain integer halving: the retail
/// sell price is half the list price.
///
/// REF: FUN_801D5944
pub fn sell_total(quantity: u32, unit_price: u32) -> u32 {
    quantity.saturating_mul(unit_price) / 2
}

/// Window 35's four label strings (`FUN_801D5510`), retail's own: the held
/// line's head (`0x801CEBC8`) and tail (`0x801CEBD0`, at `WX + 0x30`), the
/// line printed instead when the bag scan returns the `0x100` nothing-held
/// sentinel (`0x801CEBD4`), and the prompt (`0x801CEBDC`). A host reads them
/// off the menu overlay (`engine-menus::shop::ShopLabels`).
#[derive(Debug, Clone, Copy, Default)]
pub struct BuyQuantityLabels<'a> {
    pub have_head: &'a str,
    pub have_tail: &'a str,
    pub none_held: &'a str,
    pub prompt: &'a str,
}

/// Window 35: the buy quantity panel, sibling of
/// [`sell_quantity_draws_for`].
///
/// Three lines off the window content origin: the held line at `(WX, WY)`
/// (the count at `WX + 0x20`, its label at `WX + 0x30`; a `None`-held bag
/// scan prints one string at the origin instead), the prompt at
/// `WY + 0xE`, and the value row at `WY + 0x22` carrying the stepped
/// quantity at `WX + 0x18`, a separator glyph at `WX + 0x28` and the
/// quantity **bound** at `WX + 0x30`.
///
/// The second number on that row is the bound, not the unit price: the
/// row's second number call loads `DAT_801E46B8`, which the picker's phase
/// 0 fills with `min(gold / price, 99, 99 - held)`. The running total is
/// the one place the price appears, right-packed into the same
/// price-magnitude digit ladder the sell panel uses
/// ([`sell_total_digits`]) - except that this window's pens are fixed and
/// the sell window's move left as the field widens.
///
/// PORT: FUN_801D5510
pub fn buy_quantity_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    labels: BuyQuantityLabels<'_>,
    held: Option<u32>,
    quantity: u32,
    max: u32,
    unit_price: u32,
) -> (
    Vec<TextDraw>,
    Option<PainterPictogram>,
    Option<PainterSprite>,
) {
    let row = rect.y + 0x22;
    let mut out = Vec::new();
    match held {
        Some(count) => {
            out.extend(text_draws_for(
                &font.layout_ascii(labels.have_head),
                (rect.x, rect.y),
                MENU_TEXT_WHITE,
            ));
            out.extend(digits_draws(
                font,
                u64::from(count),
                rect.x + 0x20,
                rect.y,
                2,
                MENU_TEXT_WHITE,
            ));
            out.extend(text_draws_for(
                &font.layout_ascii(labels.have_tail),
                (rect.x + 0x30, rect.y),
                MENU_TEXT_WHITE,
            ));
        }
        None => out.extend(text_draws_for(
            &font.layout_ascii(labels.none_held),
            (rect.x, rect.y),
            MENU_TEXT_WHITE,
        )),
    }
    out.extend(text_draws_for(
        &font.layout_ascii(labels.prompt),
        (rect.x, rect.y + PAINTER_ROW_PITCH),
        MENU_TEXT_WHITE,
    ));
    out.extend(digits_draws(
        font,
        u64::from(quantity),
        rect.x + 0x18,
        row,
        2,
        MENU_TEXT_WHITE,
    ));
    out.extend(separator_glyph_draws(
        font,
        SEPARATOR_GLYPH_SLASH,
        (rect.x + 0x28, row),
        MENU_TEXT_WHITE,
    ));
    out.extend(digits_draws(
        font,
        u64::from(max),
        rect.x + 0x30,
        row,
        2,
        MENU_TEXT_WHITE,
    ));
    out.extend(digits_draws(
        font,
        u64::from(quantity) * u64::from(unit_price),
        rect.x + 0x62,
        row,
        sell_total_digits(unit_price),
        MENU_TEXT_WHITE,
    ));
    (
        out,
        Some(PainterPictogram {
            id: COUNTER_PICTOGRAM_PRIMARY,
            x: rect.x + 0x58,
            y: rect.y + 0x24,
        }),
        Some(PainterSprite {
            sprite: 0,
            variant: 1,
            x: rect.x + 4,
            y: row,
        }),
    )
}

/// Window 37: the sell quantity panel.
///
/// `FUN_801D5944` draws nothing when the selected item id
/// `DAT_801E46B0` is not positive.
/// Otherwise: a heading at the content origin, then one row `0x14` below
/// carrying the quantity at `WX + 0x10`, a separator glyph at `WX + 0x20`
/// and the held count at `WX + 0x28`. The gold pictogram and the total
/// are then right-packed against `WX + 0x88` / `WX + 0x94` by the digit
/// ladder ([`sell_total_digits`]), so a bigger total pushes both left.
///
/// The unit price retail reads is the **item record's own** `+2` word
/// (`0x80074368 + id*12 + 2`), not the merchant's stock entry, so a bag item
/// the shop does not sell still prices correctly.
///
/// PORT: FUN_801D5944
pub fn sell_quantity_draws_for(
    font: &legaia_font::Font,
    rect: PainterRect,
    selected: bool,
    heading: &str,
    quantity: u32,
    held: u32,
    unit_price: u32,
) -> (
    Vec<TextDraw>,
    Option<PainterPictogram>,
    Option<PainterSprite>,
) {
    if !selected {
        return (Vec::new(), None, None);
    }
    let row = rect.y + 0x14;
    let digits = sell_total_digits(unit_price);
    let pack = rect.x - 8;
    let mut out = text_draws_for(
        &font.layout_ascii(heading),
        (rect.x, rect.y),
        MENU_TEXT_WHITE,
    );
    out.extend(digits_draws(
        font,
        quantity as u64,
        rect.x + 0x10,
        row,
        2,
        MENU_TEXT_WHITE,
    ));
    out.extend(separator_glyph_draws(
        font,
        SEPARATOR_GLYPH_SLASH,
        (rect.x + 0x20, row),
        MENU_TEXT_WHITE,
    ));
    out.extend(digits_draws(
        font,
        held as u64,
        rect.x + 0x28,
        row,
        2,
        MENU_TEXT_WHITE,
    ));
    out.extend(digits_draws(
        font,
        sell_total(quantity, unit_price) as u64,
        pack + 0x94 - digits * NUM_CELL_W,
        row,
        digits,
        MENU_TEXT_WHITE,
    ));
    (
        out,
        Some(PainterPictogram {
            id: COUNTER_PICTOGRAM_PRIMARY,
            x: pack + 0x88 - digits * NUM_CELL_W,
            y: row + 2,
        }),
        Some(PainterSprite {
            sprite: 0,
            variant: 1,
            x: rect.x - 4,
            y: row,
        }),
    )
}
