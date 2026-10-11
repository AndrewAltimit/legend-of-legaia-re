//! The window painters: entry lists, option panels, gated labels and the acquisition caption.

use super::*;

// ---------------------------------------------------------------------------
// panel draws
// ---------------------------------------------------------------------------

/// Rodata VAs of the literal strings the panel draws load.
pub const STR_ROW_HEADER: u32 = 0x801C_F09C;

pub const STR_THREE_LINE: [u32; 3] = [0x801C_F108, 0x801C_F10C, 0x801C_F110];

pub const STR_TWO_OPTION: [u32; 2] = [0x801C_F138, 0x801C_F140];

pub const STR_COUNT_GATED: [u32; 2] = [0x801C_F14C, 0x801C_F170];

pub const STR_TWO_LINE: [u32; 2] = [0x801C_F190, 0x801C_F198];

pub const STR_SINGLE: u32 = 0x801C_F1A4;

pub const STR_CAPTION: u32 = 0x801C_EA30;

/// PORT: FUN_801f16c0 - the stacked per-entry label list.
///
/// Walks the `DAT_80084594` entries, publishing each entry's code byte to
/// `DAT_8007B469` first because the per-entry sub-draw `FUN_801E5B4C` reads
/// it. Codes at or above `3` draw nothing at all, but still cost a loop step.
/// Each drawn entry prints its label at the running `y`, advances `y` by
/// `0x0D` for the sub-draw and by a further `0x2A` afterwards; the actor's own
/// `+0x0C` is restored at the end, as is the previous entry code.
///
/// The sub-draw is [`equip_stat_panel`], and the `jal 0x801F1778` that reaches
/// it is that routine's only reference in the corpus - so the equipment
/// stat panel exists for this list and nothing else. Its rows are spliced in
/// as [`HubDraw::EntrySubPanel`] at the point the `jal` sits, between the
/// `0x0D` and the `0x2A` pen advances.
pub fn entry_list(actor: &mut HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let saved_y = actor.y;
    for i in 0..env.entry_count as usize {
        let code = env.entry_codes.get(i).copied().unwrap_or(0);
        out.act(HubAction::SetEntryCode(code));
        if code < 3 {
            out.draw(HubDraw::Text {
                text: HubString::EntryLabel(code),
                x: actor.x,
                y: actor.y,
                palette: PALETTE_PANEL,
            });
            actor.y = actor.y.wrapping_add(0x0D);
            for d in entry_sub_panel(actor, env, code) {
                out.draw(HubDraw::EntrySubPanel(d));
            }
            actor.y = actor.y.wrapping_add(0x2A);
        }
    }
    actor.y = saved_y;
    out
}

/// Build the `FUN_801E5B4C` call [`entry_list`] makes for one entry.
///
/// Retail passes only the actor; everything else the sub-draw needs it reads
/// out of globals - the entry code the caller just published to
/// `DAT_8007B469`, the save block, the two static tables and the menu mode
/// word. This assembles the same inputs out of [`HubEnv::equip`].
fn entry_sub_panel(actor: &HubActor, env: &HubEnv, code: u8) -> Vec<EquipPanelDraw> {
    let eq = &env.equip;
    let record = eq
        .records
        .iter()
        .find(|r| r.code == code)
        .copied()
        .unwrap_or_default();
    let input = EquipPanelInput {
        x: actor.x,
        y: actor.y,
        char_index: usize::from(code),
        slots: record.slots,
        base_stats: record.base_stats,
        mode: eq.mode,
        cursor: env.cursor_row,
        inventory: eq.inventory.clone(),
        weapon_slots: eq.weapon_slots.clone(),
        // `_DAT_8007B450` is the same op-`0x49` descriptor cell `HubEnv`
        // already carries as a truth value.
        tight_rows: env.board_flag != 0,
    };
    equip_stat_panel(
        &input,
        |id| {
            eq.item_props
                .get(usize::from(id))
                .copied()
                .unwrap_or_default()
        },
        |idx| {
            eq.equip_props
                .get(usize::from(idx))
                .copied()
                .unwrap_or_default()
        },
    )
}

/// PORT: FUN_801f17d8 - the header string plus one sprite cell per grid column.
///
/// The header prints at the actor origin; the cell row starts `0x10` in on
/// both axes and steps `0x20` per column, its cell id being the grid actor's
/// `+0x54 + i` byte biased by `0x37`. The row length is `_DAT_8007BB88`, not
/// the entry count.
pub fn column_row(actor: &HubActor, env: &HubEnv, grid: &HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    out.draw(HubDraw::HeaderText {
        text: HubString::Literal(STR_ROW_HEADER),
        x: actor.x,
        y: actor.y,
    });
    let mut x = actor.x.wrapping_add(0x10);
    let y = actor.y.wrapping_add(0x10);
    for i in 0..env.cursor_row.max(0) as usize {
        let cell = grid.columns.get(i).copied().unwrap_or(0) as i32 + 0x37;
        out.draw(HubDraw::Cell { x, y, cell });
        x = x.wrapping_add(0x20);
    }
    out
}

/// PORT: FUN_801f1890 - the three-line panel with its own cursor row.
///
/// The first line uses [`PALETTE_PANEL`], the lower two [`PALETTE_DIM`]; the
/// cursor sits at `x + 0x38` and steps `0x0E` per `_DAT_8007BB98` row -
/// retail spells that pitch as `(n * 8 - n) << 1`.
pub fn three_line_panel(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    out.draw(HubDraw::ShortText {
        text: HubString::Literal(STR_THREE_LINE[0]),
        x: x.wrapping_add(0x24),
        y,
        palette: PALETTE_PANEL,
    });
    out.draw(HubDraw::ShortText {
        text: HubString::Literal(STR_THREE_LINE[1]),
        x: x.wrapping_add(0x4C),
        y: y.wrapping_add(0x10),
        palette: PALETTE_DIM,
    });
    out.draw(HubDraw::ShortText {
        text: HubString::Literal(STR_THREE_LINE[2]),
        x: x.wrapping_add(0x4C),
        y: y.wrapping_add(0x1E),
        palette: PALETTE_DIM,
    });
    out.draw(HubDraw::Sprite {
        a: 0,
        b: 1,
        x: x.wrapping_add(0x38),
        y: y.wrapping_add(0x10)
            .wrapping_add((env.cursor_row_alt * 0x0E) as i16),
    });
    out
}

/// PORT: FUN_801f1950 - the two-option panel.
///
/// Each option draws its cursor *before* its label and only when
/// `_DAT_8007BB88` selects that row, so the two tests read the global twice
/// with the labels between them.
pub fn two_option_panel(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    if env.cursor_row == 0 {
        out.draw(HubDraw::Sprite { a: 0, b: 1, x, y });
    }
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_OPTION[0]),
        x: x.wrapping_add(0x14),
        y,
        palette: PALETTE_PANEL,
    });
    if env.cursor_row == 1 {
        out.draw(HubDraw::Sprite {
            a: 0,
            b: 1,
            x,
            y: y.wrapping_add(0x0E),
        });
    }
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_OPTION[1]),
        x: x.wrapping_add(0x14),
        y: y.wrapping_add(0x0E),
        palette: PALETTE_PANEL,
    });
    out
}

/// The right-edge cursor both single-label panels place: `x + width - 0x10`,
/// `y - 2`, sprite `(1, 1)`.
fn edge_cursor(actor: &HubActor) -> HubDraw {
    HubDraw::Sprite {
        a: 1,
        b: 1,
        x: actor.x.wrapping_add(actor.width).wrapping_sub(0x10),
        y: actor.y.wrapping_sub(2),
    }
}

/// PORT: FUN_801f1a1c - the count-gated single label.
///
/// Picks the alternate string when the entry count `DAT_80084594` is below
/// `2` (an unsigned byte test), draws it `0x0C` in from the panel origin, and
/// finishes with the shared right-edge cursor.
pub fn count_gated_label(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let text = if env.entry_count < 2 {
        STR_COUNT_GATED[1]
    } else {
        STR_COUNT_GATED[0]
    };
    out.draw(HubDraw::Text {
        text: HubString::Literal(text),
        x: actor.x.wrapping_add(0x0C),
        y: actor.y,
        palette: PALETTE_PANEL,
    });
    out.draw(edge_cursor(actor));
    out
}

/// PORT: FUN_801f1b64 - the single label plus the right-edge cursor.
///
/// [`count_gated_label`] without the count test.
pub fn single_label(actor: &HubActor) -> HubFrame {
    let mut out = HubFrame::default();
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_SINGLE),
        x: actor.x.wrapping_add(0x0C),
        y: actor.y,
        palette: PALETTE_PANEL,
    });
    out.draw(edge_cursor(actor));
    out
}

/// PORT: FUN_801f1ab0 - the two-line panel with the screen-effect push.
///
/// Both lines start `0x0C` in and are `0x10` apart; the cursor sits `8` left
/// of the origin and steps `0x10` per `_DAT_8007BB88` row. The trailing
/// `FUN_80024EE4(3, 0, 0)` is the only screen-effect push in the family.
pub fn two_line_panel(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_LINE[0]),
        x: x.wrapping_add(0x0C),
        y,
        palette: PALETTE_PANEL,
    });
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_LINE[1]),
        x: x.wrapping_add(0x0C),
        y: y.wrapping_add(0x10),
        palette: PALETTE_PANEL,
    });
    out.draw(HubDraw::Sprite {
        a: 0,
        b: 1,
        x: x.wrapping_sub(8),
        y: y.wrapping_add((env.cursor_row * 0x10) as i16),
    });
    out.draw(HubDraw::Effect(3));
    out
}

// Owner: this is not a Baka hub routine and `0x801F90DC` is not its address.
// Every dump filed at `0x801F90DC` is `dump-extent-attribution.csv` class
// `misbased` - no image holds those bytes at that VA - and they are the menu
// overlay's (PROT 0899) shared item-info panel `FUN_801D0F1C` printed
// `0x281C0` high: its first 39 instructions match `overlay_menu_801d0f1c.txt`
// operand for operand. The whole 221-instruction body is in that dump, so
// the "unrecovered past the number draw" note no longer holds.
/// PORT: FUN_801d0f1c
/// REPLACED-BY: `legaia_engine_ui::menu::screens::pause_lists` - the live port of
/// the same routine (its info window and the id-`0xFE` Point Card arm), which
/// both play hosts' pause screens draw. No host is owed this second copy.
///
/// `DAT_801E46B0` is an **item id**, and the two strings come from the static
/// `SCUS_942.54` item table (`0x8007436C + id * 0x0C`, see
/// [`docs/formats/item-table.md`]) - the name at the record's word `0` and the
/// detail line at word `1`. The special id [`CAPTION_POINT_CARD_ID`] is the
/// **Point Card**, not money: it adds a fixed caption and prints the
/// eight-digit total from `_DAT_800845B4`, which is the Point Card counter
/// (`docs/reference/memory-map.md`) and not a purse. Party gold is
/// `_DAT_8008459C` and the casino coin bank `_DAT_800845A4`, both named
/// separately in this module's globals list - and `0xFE` is the id the shop's
/// buy commit tests with `FUN_80042F4C(0xFE)` before crediting that same
/// counter (`engine-menus::shop::POINT_CARD_ITEM_ID`).
///
/// REF: FUN_80042F4C - the inventory-has query that gate uses; ported on the
/// engine side as the shop session's Point Card check, not here.
///
/// The Ghidra dump of this body stops after the Point Card arm with no epilogue
/// (`Control flow encountered bad instruction data`), so anything past the
/// number draw is unrecovered; what is here is the whole disassembled extent.
pub fn acquisition_caption(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    let id = env.caption_item;
    out.draw(HubDraw::Text {
        text: HubString::ItemName(id),
        x,
        y,
        palette: PALETTE_CAPTION,
    });
    out.draw(HubDraw::Detail {
        text: HubString::ItemDetail(id),
        id,
        x,
        y: y.wrapping_add(0x10),
        // palette 7 is selected before this call
    });
    if id == CAPTION_POINT_CARD_ID {
        out.draw(HubDraw::Text {
            text: HubString::Literal(STR_CAPTION),
            x: x.wrapping_add(0x18),
            y: y.wrapping_add(0x41),
            palette: PALETTE_PANEL,
        });
        out.draw(HubDraw::Number {
            value: env.caption_amount,
            digits: 8,
            x: x.wrapping_add(0x38),
            y: y.wrapping_add(0x4E),
        });
    }
    out
}
