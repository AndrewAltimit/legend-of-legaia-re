use crate::*;

/// One row in the inventory item-use list. Plain-data view so the
/// renderer doesn't depend on `engine-core::inventory_use`.
pub struct InventoryItemRow<'a> {
    pub name: &'a str,
    pub count: u8,
    /// `true` when the item passes the active context's filter
    /// (battle/field). Failing items still appear, dimmed.
    pub admissible: bool,
}

/// One row in the inventory item-use target picker.
pub struct InventoryTargetRow<'a> {
    pub name: &'a str,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub alive: bool,
}

/// Bundle of arguments for [`inventory_use_draws_for`]. Bundled so the
/// function takes one payload struct rather than ten positional args.
pub struct InventoryUseDrawArgs<'a> {
    pub items: &'a [InventoryItemRow<'a>],
    pub targets: &'a [InventoryTargetRow<'a>],
    /// `true` for battle context (target column shows monsters too);
    /// `false` for field (party only). Drives the title.
    pub in_battle: bool,
    /// Cursor row inside the active phase column.
    pub cursor: u8,
    /// `0` = item column, `1` = target column.
    pub phase: u8,
    /// Selected item id when in target phase. `None` while browsing.
    pub selected_item_name: Option<&'a str>,
}

/// Build [`TextDraw`]s for the inventory item-use overlay shared by the
/// field menu's Items row and the battle command-menu's Items option.
///
/// Layout (anchored at `pen`):
/// ```text
/// ITEMS
/// > Healing Leaf            x 04         | Vahn        HP 250/300
///   Magic Leaf              x 02         | Noa         HP 180/220
///   Antidote Leaf           x 01         | Gala        HP  90/280
///   ...                                  |
/// ```
///
/// The right-hand target column is only drawn when `phase == 1` (target
/// select). Failing items (filtered out by the active context) render
/// dimmed but stay visible so the player understands why their item
/// disappeared.
pub fn inventory_use_draws_for(
    font: &legaia_font::Font,
    args: InventoryUseDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    let title = if args.in_battle { "ITEMS [B]" } else { "ITEMS" };
    out.extend(text_draws_for(&font.layout_ascii(title), pen, gold));

    if args.items.is_empty() {
        let l = font.layout_ascii("(no usable items)");
        out.extend(text_draws_for(&l, (pen.0, pen.1 + LINE_H), dim));
        return out;
    }

    // Item column.
    for (i, item) in args.items.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected_here = args.phase == 0 && i as u8 == args.cursor;
        let color = if !item.admissible {
            dim
        } else if selected_here {
            gold
        } else {
            white
        };
        if selected_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        let line = format!("{:<20} x{:>3}", item.name, item.count);
        out.extend(text_draws_for(
            &font.layout_ascii(&line),
            (pen.0 + 14, y),
            color,
        ));
    }

    // Target column when picking a target.
    if args.phase == 1 {
        let col_x = pen.0 + 240;
        if let Some(name) = args.selected_item_name {
            let head = format!("Use: {name}");
            out.extend(text_draws_for(
                &font.layout_ascii(&head),
                (col_x, pen.1),
                gold,
            ));
        }
        for (i, t) in args.targets.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = i as u8 == args.cursor;
            let color = if !t.alive {
                red
            } else if selected_here {
                gold
            } else {
                white
            };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = if t.mp_max > 0 {
                format!(
                    "{:<8} HP {:>3}/{:<3} MP {:>3}/{:<3}",
                    t.name, t.hp, t.hp_max, t.mp, t.mp_max
                )
            } else {
                format!("{:<8} HP {:>3}/{:<3}", t.name, t.hp, t.hp_max)
            };
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
        }
        if args.targets.is_empty() {
            let l = font.layout_ascii("(no targets)");
            out.extend(text_draws_for(&l, (col_x, pen.1 + LINE_H), dim));
        }
    }
    out
}
