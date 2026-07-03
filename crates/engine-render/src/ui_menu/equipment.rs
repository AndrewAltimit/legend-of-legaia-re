use crate::*;

/// One slot row in the equipment screen.
pub struct EquipSlotRow<'a> {
    pub label: &'a str,
    /// Currently-equipped item display name. "(empty)" for an unfilled
    /// slot.
    pub current_name: &'a str,
}

/// One candidate row in the per-slot item picker.
pub struct EquipCandidateRow<'a> {
    pub name: &'a str,
    pub count: u8,
    /// Stat preview delta vs. the current equipped item: positive deltas
    /// are tinted green, negatives red. Engines compute these by running
    /// `compute_battle_stats` once with the candidate id installed.
    pub atk_delta: i16,
    pub udf_delta: i16,
}

/// Phase tag for [`equipment_session_draws_for`]. Mirrors
/// `engine-core::equip_session::EquipState` without naming the enum so
/// the renderer doesn't pull engine-core in as a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipDrawPhase {
    /// Cursor on the slot grid.
    SlotPicker,
    /// Cursor on the candidate-item list for the active slot.
    ItemPicker,
    /// Yes/No confirmation prompt (`cursor` == 0 for Yes, 1 for No).
    Confirm,
}

/// Bundle for [`equipment_session_draws_for`].
pub struct EquipDrawArgs<'a> {
    /// Display name of the character being equipped.
    pub character_name: &'a str,
    pub slots: &'a [EquipSlotRow<'a>],
    /// Candidate items for the active slot. Empty in `SlotPicker` phase.
    pub candidates: &'a [EquipCandidateRow<'a>],
    pub phase: EquipDrawPhase,
    /// Cursor row inside the active phase column.
    pub cursor: u16,
    /// Active slot index (0..=7) when in `ItemPicker` / `Confirm`.
    pub active_slot: u8,
    /// Optional pending swap label rendered above the Yes/No prompt
    /// ("Equip Iron Sword?"). Only consumed in `Confirm`.
    pub confirm_label: Option<&'a str>,
}

/// Build [`TextDraw`]s for the equipment overlay shared by the field
/// menu's Equip row and the shop's "buy then equip" flow.
///
/// Layout (anchored at `pen`):
/// ```text
/// EQUIP - Vahn
/// > Weapon       Iron Sword
///   Helmet       Leather Cap
///   Body Armor   (empty)
///   ...
///                                      | Iron Sword     ATK +10
///                                      | Wood Sword     ATK +5
///                                      | (empty)
///   Equip Iron Sword?  Yes  No
/// ```
pub fn equipment_session_draws_for(
    font: &legaia_font::Font,
    args: EquipDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    let head = format!("EQUIP - {}", args.character_name);
    out.extend(text_draws_for(&font.layout_ascii(&head), pen, gold));

    // Slot column.
    for (i, slot) in args.slots.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let cursor_here = args.phase == EquipDrawPhase::SlotPicker && i as u16 == args.cursor;
        let row_active = args.phase != EquipDrawPhase::SlotPicker && args.active_slot as usize == i;
        let color = if cursor_here || row_active {
            gold
        } else {
            white
        };
        if cursor_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(slot.label),
            (pen.0 + 14, y),
            color,
        ));
        let item_color = if slot.current_name == "(empty)" {
            dim
        } else {
            color
        };
        out.extend(text_draws_for(
            &font.layout_ascii(slot.current_name),
            (pen.0 + 110, y),
            item_color,
        ));
    }

    // Candidate column.
    if args.phase != EquipDrawPhase::SlotPicker {
        let col_x = pen.0 + 250;
        let head = if let Some(slot) = args.slots.get(args.active_slot as usize) {
            format!("> {}", slot.label)
        } else {
            "Slot".to_string()
        };
        out.extend(text_draws_for(
            &font.layout_ascii(&head),
            (col_x, pen.1),
            gold,
        ));

        if args.candidates.is_empty() {
            out.extend(text_draws_for(
                &font.layout_ascii("(no items)"),
                (col_x, pen.1 + LINE_H),
                dim,
            ));
        }
        for (i, c) in args.candidates.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = args.phase == EquipDrawPhase::ItemPicker && i as u16 == args.cursor;
            let color = if selected_here { gold } else { white };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = format!("{:<14} x{:>2}", c.name, c.count);
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
            let mut delta_x = col_x + 14 + 130;
            if c.atk_delta != 0 {
                let s = format!("ATK {:+}", c.atk_delta);
                let dc = if c.atk_delta > 0 { green } else { red };
                out.extend(text_draws_for(&font.layout_ascii(&s), (delta_x, y), dc));
                delta_x += 56;
            }
            if c.udf_delta != 0 {
                let s = format!("UDF {:+}", c.udf_delta);
                let dc = if c.udf_delta > 0 { green } else { red };
                out.extend(text_draws_for(&font.layout_ascii(&s), (delta_x, y), dc));
            }
        }
    }

    // Confirm prompt at the bottom.
    if args.phase == EquipDrawPhase::Confirm {
        let prompt_y = pen.1 + LINE_H + args.slots.len() as i32 * LINE_H + LINE_H;
        if let Some(label) = args.confirm_label {
            out.extend(text_draws_for(
                &font.layout_ascii(label),
                (pen.0, prompt_y),
                white,
            ));
        }
        for (i, opt) in ["Yes", "No"].iter().enumerate() {
            let x = pen.0 + 110 + i as i32 * 50;
            let selected = i as u16 == args.cursor;
            let color = if selected { gold } else { white };
            if selected {
                out.extend(text_draws_for(
                    &font.layout_ascii(">"),
                    (x - 10, prompt_y + LINE_H),
                    color,
                ));
            }
            out.extend(text_draws_for(
                &font.layout_ascii(opt),
                (x, prompt_y + LINE_H),
                color,
            ));
        }
    }
    out
}
