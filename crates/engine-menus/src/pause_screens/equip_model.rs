//! The Equip screen view model: slot rows, item info and the stat-compare panel.

use super::*;

/// Phase tag of the Equip screen, mirroring
/// [`crate::equip_session::EquipState`] as a flat word the hosts map onto
/// `engine-ui`'s `EquipDrawPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipScreenPhase {
    SlotPicker,
    ItemPicker,
    Confirm,
}

/// Owned view model of the Equip screen - the sibling of
/// [`items_screen_model`] / [`magic_screen_model`] for the third
/// descriptor-window screen.
///
/// It exists for the same reason those do: the projection is real work
/// (eight slot labels, the candidate list for the active slot with its bag
/// counts, and a full `compute_battle_stats` pass with the hovered item
/// installed), and it was written out twice - once in the native window's
/// `equip_session_draws`, once in the browser's. Two copies of a stat
/// preview is two chances to preview a different number.
pub struct EquipScreenModel {
    /// Party-window rows.
    pub party_names: Vec<String>,
    /// Slot labels in engine slot order (retail identifies slots by the
    /// pictogram column; the label is an engine hint).
    pub slot_labels: Vec<String>,
    /// Per-slot equipped-item display names; empty string for an empty slot.
    pub slot_items: Vec<String>,
    /// Candidate item names for the active slot. Empty in `SlotPicker`.
    pub candidate_names: Vec<String>,
    /// Bag count per candidate, parallel to [`Self::candidate_names`].
    pub candidate_counts: Vec<u8>,
    /// The three retail compare rows (`FUN_801D21C0`'s stat block) as
    /// `(label, current, preview)`: the live menu block against the block
    /// with the Best Equipment picks installed. Retail draws them only while
    /// the slot browse's hand is on the Best Equipment row (sub-screen `0x13`,
    /// row 0); empty everywhere else - the candidate step's compare is
    /// window 25 ([`Self::compare`]), not this block.
    pub stat_compare: Vec<(&'static str, u16, u16)>,
    /// The Best Equipment row's per-armament change list: `(armament row
    /// 0..=3, candidate name)` for each armament whose pick
    /// (`DAT_801EF0C0[i]`) differs from what the slot holds. Retail draws the
    /// change arrow, the armament pictogram and the name on that armament's
    /// slot row. Same gate as [`Self::stat_compare`].
    pub best_changes: Vec<(u8, String)>,
    pub phase: EquipScreenPhase,
    /// Cursor row inside the active phase column.
    pub cursor: u16,
    /// Active slot index in `ItemPicker` / `Confirm`.
    pub active_slot: u8,
    /// Pending-swap label above the Yes/No prompt.
    pub confirm_label: Option<String>,
    /// Roster slot of the character being equipped.
    pub char_slot: u8,
    /// The party-window row that character sits on (its place in the
    /// present party, which is what [`Self::party_names`] lists) - the row
    /// the hand cursor marks.
    pub party_row: u8,
    /// Slot-picker cursor row, or `None` past the slot picker - what the
    /// sprite pass puts the second hand on.
    pub slot_cursor: Option<u16>,
    /// Pictogram rows the sprite pass draws - retail's seven browse rows.
    pub pictogram_rows: usize,
    /// Window 24's item-info panel content for the hovered candidate, or
    /// `None` outside the candidate step.
    ///
    /// Retail's Equip screen opens **five** windows, not four: sub-screen
    /// `0x12` picks the character and `0x13` browses the slot rows over the
    /// capture-pinned set `2 / 21 / 22 / 23`, and `0x14` - the candidate
    /// list - adds windows `24` and `25` on top through open script
    /// `0x801E4DC8` (`docs/subsystems/field-menu.md`). Window 24's renderer
    /// `FUN_801DCC20` calls the **shared item-info panel** `FUN_801D0F1C`,
    /// the same one window 17 draws on the Items screen - which is why the
    /// two descriptors carry byte-identical rects `(14, 108, 144, 40)`.
    ///
    /// That makes this panel an *addition* to the port's screen rather than
    /// a different layout for it - the reading that had window 24 waived as
    /// needing "the whole screen moved onto the descriptor-table layout".
    pub info: Option<EquipItemInfoModel>,
    /// Window 25's stat-compare panel, the candidate step's **other**
    /// addition (open script `0x801E4DC8` names 24 and 25 together).
    /// `None` outside the candidate step, or when the host supplied no
    /// [`EquipCompareCtx`].
    pub compare: Option<EquipCompareModel>,
}

/// Window 24's item-info content - the hovered candidate's own row of the
/// shared panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipItemInfoModel {
    pub name: String,
    /// Bag count, echoed beside the name exactly as window 17 echoes it.
    pub count: u16,
    pub desc: String,
    /// An accessory's two passive lines.
    pub passive: Option<(String, String)>,
}

/// Window 25's model - the Equip screen's own stat-compare panel.
///
/// The fields are the retail panel's inputs, one per global the renderer
/// reads: the two eight-word blocks, the record's HP / MP maxima (the HP and
/// MP rows print the record halfword, not the block word) and the four
/// values the compare **category** is resolved from. `engine-ui` holds the
/// resolver and the painter, so this type carries no row set - only what the
/// resolver asks for.
///
/// REF: FUN_801d1290 - the renderer this feeds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EquipCompareModel {
    /// Display name drawn at the content origin (record `+0x2A7`).
    pub name: String,
    /// The live block (`0x801EF080`).
    pub current: [i32; 8],
    /// The trial-equip block (`0x801EF0A0`) - the same eight words with the
    /// hovered candidate installed in the active slot.
    pub candidate: [i32; 8],
    /// HP maximum off the record (`+0x104`).
    pub hp_max: u16,
    /// MP maximum off the record (`+0x108`).
    pub mp_max: u16,
    /// Browse row past the screen's row 0, retail's
    /// `(DAT_801E46C0 & 0xFFF) - 1`.
    pub slot_row: i32,
    /// Staged (hovered) item id, retail's `DAT_801E46B0`; `-1` = nothing
    /// staged.
    pub staged_id: i32,
    /// Category byte resolved for [`Self::staged_id`].
    pub staged_category: u8,
    /// Id already sitting in the browsed slot's equip byte; `0` = empty.
    pub equipped_id: u8,
    /// Category byte resolved for [`Self::equipped_id`].
    pub equipped_category: u8,
}

/// The record + disc tables [`equip_screen_model`] needs before it can
/// publish an [`EquipCompareModel`].
///
/// A host that has none of it passes `None` and the screen keeps its
/// window-22 compare block alone - the panel is an addition to the screen,
/// not a replacement for any part of it.
#[derive(Clone, Copy)]
pub struct EquipCompareCtx<'a> {
    /// The character record behind the session. Supplies the HP / MP maxima,
    /// which [`legaia_engine_battle::battle_stats::StatRecord`] does not carry.
    pub record: &'a legaia_save::CharacterRecord,
    /// Disc equipment stat-bonus table view, for the class-`1` arm of the
    /// category lookup (`0x80074F68 + row*8 + 5`).
    pub equip_info: Option<&'a crate::equipment::DiscEquipInfo>,
    /// Disc item-effect table, for the item record's class byte and the
    /// non-equipment arm of the category lookup (`0x800752C0 + row*4 + 3`).
    pub item_effects: Option<&'a legaia_game_tables::item_effect::ItemEffectTable>,
}

/// Equip bytes the **menu** stat aggregator walks.
///
/// `FUN_801CF650`'s loop counter is bounded by `slti a2, 5` at `0x801CF744`,
/// so the block behind windows 22 / 25 / 41 sums the first five equip bytes
/// only - not all eight. The battle-side aggregator `FUN_80042558`, which
/// [`legaia_engine_battle::battle_stats::compute_battle_stats`] implements, walks the whole
/// array; that is why [`menu_stat_block`] zeroes the tail rather than
/// handing the shared aggregator the record as it stands.
pub const MENU_BLOCK_EQUIP_SLOTS: usize = 5;

/// Category byte used when no lookup resolves one - `FUN_801D1290`'s
/// pre-loaded `li a1, 0x40` and the no-passive sentinel of the equipment
/// bonus rows.
pub const COMPARE_CATEGORY_DEFAULT: u8 = 0x40;

/// Resolve one item id's compare-category byte, retail's
/// `0x801D1388..0x801D13F8` lookup chain.
///
/// The item property record's class byte (`0x80074368 + id*0xC + 0`) picks
/// the table: class `1` reads the equipment bonus row's `+5`, anything else
/// reads the item-effect descriptor's `+3`. Both arms index by the **same**
/// `+1` byte of the item record, which is what
/// [`legaia_game_tables::item_effect::ItemEffectTable::subtype`] returns.
///
/// REF: FUN_801d1290 - the renderer this lookup belongs to; the guards that
/// decide *whether* it is consulted are `engine-ui`'s
/// `active_compare_category`.
pub fn compare_category_for_item(id: u8, ctx: &EquipCompareCtx<'_>) -> u8 {
    let Some(effects) = ctx.item_effects else {
        return COMPARE_CATEGORY_DEFAULT;
    };
    let row = effects.subtype(id);
    if effects.kind(id) == 1 {
        match ctx.equip_info {
            Some(info) => info.row_passive_index(row),
            None => COMPARE_CATEGORY_DEFAULT,
        }
    } else {
        effects
            .descriptor(row)
            .map(|e| e.marker)
            .unwrap_or(COMPARE_CATEGORY_DEFAULT)
    }
}

/// Build the eight-word menu stat block for one record + equipment set.
///
/// Word order is the retail block's: HP max, MP max, AGL, ATK, UDF, LDF,
/// SPD, INT. Words 0 / 1 / 2 come straight off the record (`FUN_801CF5D0`
/// seeds all eight; only these three are never added to); words 3..=7 are
/// the seed plus the equipment bonuses of the first
/// [`MENU_BLOCK_EQUIP_SLOTS`] slots, which is what `FUN_801CF650` sums.
///
/// REF: FUN_801cf5d0 - the seeder.
/// REF: FUN_801cf650 - the equipment-bonus summer.
pub fn menu_stat_block(
    record: &legaia_engine_battle::battle_stats::StatRecord,
    table: &legaia_engine_battle::battle_stats::EquipmentTable,
    hp_max: u16,
    mp_max: u16,
) -> [i32; 8] {
    let mut walked = *record;
    for slot in MENU_BLOCK_EQUIP_SLOTS..walked.equip.len() {
        walked.equip[slot] = 0;
    }
    let neutral = legaia_engine_battle::battle_stats::StatusModifiers::default();
    let s = legaia_engine_battle::battle_stats::compute_battle_stats(&walked, table, &[], &neutral);
    [
        i32::from(hp_max),
        i32::from(mp_max),
        // AGL is the record's `+0x110` word; equipment never feeds it, so the
        // seeded value stands. `StatRecord` carries it as the accuracy line.
        i32::from(record.base_accuracy),
        i32::from(s.atk),
        i32::from(s.udf),
        i32::from(s.ldf),
        i32::from(s.spd),
        i32::from(s.int),
    ]
}

/// Project a live [`crate::equip_session::EquipSession`] into
/// [`EquipScreenModel`].
///
/// `party_names` is the world's roster snapshot, which the session does not
/// carry. The stat preview uses the neutral status set: this is the field
/// menu, and the session recomputes with live status modifiers on commit.
///
/// `text` resolves an item id's display name / description / passive lines -
/// `legaia_engine_menu_runtime::field_menu_dispatch::item_display_text` against the live world.
/// Passing `None` leaves every item spelled as its raw id, which is what a
/// disc-free test wants and what the screen showed on both hosts for as long
/// as the resolver lived inside the Items screen's own session builder.
/// `compare` is the record + disc tables window 25's panel needs; passing
/// `None` leaves [`EquipScreenModel::compare`] empty and the screen draws
/// exactly what it drew before the panel existed.
pub fn equip_screen_model(
    session: &crate::equip_session::EquipSession,
    char_slot: u8,
    party_names: &[String],
    present: &[u8],
    text: Option<&dyn Fn(u8) -> ItemDisplayText>,
    compare: Option<EquipCompareCtx<'_>>,
) -> EquipScreenModel {
    use crate::equip_session::EquipState;
    use crate::equipment::EquipSlot;

    let record = session.record();
    let name_of = |id: u8| match text {
        Some(f) => f(id).name,
        None => format!("Item {id:02X}"),
    };
    // Rows in retail's browse order (weapon, helmet, body, footwear, Goods
    // x3); the engine's Hand Guard slot (the Ra-Seru byte) has no row.
    let order = crate::equip_session::BROWSE_SLOT_ORDER;
    let slot_labels: Vec<String> = order
        .iter()
        .map(|&i| {
            EquipSlot::from_index(i)
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| format!("Slot {i}"))
        })
        .collect();
    let slot_items: Vec<String> = order
        .iter()
        .map(|&i| match record.equip.get(usize::from(i)).copied() {
            None | Some(0) => String::new(),
            Some(id) => name_of(id),
        })
        .collect();

    let (phase, cursor, active_slot, confirm_label) = match session.state() {
        EquipState::SlotPicker { cursor } => {
            (EquipScreenPhase::SlotPicker, cursor as u16, cursor, None)
        }
        EquipState::ItemPicker { slot, cursor } => {
            (EquipScreenPhase::ItemPicker, cursor, slot, None)
        }
        EquipState::Confirm {
            slot,
            item_id,
            cursor,
        } => (
            EquipScreenPhase::Confirm,
            cursor as u16,
            slot,
            Some(format!("Equip {}?", name_of(item_id))),
        ),
        EquipState::Done(_) => (EquipScreenPhase::SlotPicker, 0, 0, None),
    };

    // Candidates + stat compare only matter past the slot picker.
    let (candidate_names, candidate_counts, considered_id): (Vec<String>, Vec<u8>, Option<u8>) =
        if phase == EquipScreenPhase::SlotPicker {
            (Vec::new(), Vec::new(), None)
        } else {
            let items = session.items_for_slot(active_slot);
            let names: Vec<String> = items.iter().map(|it| name_of(it.id)).collect();
            let counts: Vec<u8> = items
                .iter()
                .map(|it| session.inventory().get(&it.id).copied().unwrap_or(0))
                .collect();
            // The item the compare block previews: the hovered row in the
            // picker, the pending item in the confirm phase.
            let considered = match session.state() {
                EquipState::Confirm { item_id, .. } => Some(item_id),
                _ => items.get(cursor as usize).map(|it| it.id),
            };
            (names, counts, considered)
        };

    // Window 22's second pass (`FUN_801D21C0`, `0x801D23BC..0x801D27F8`):
    // gated on the settled slot-browse sub-screen (`DAT_801E46A4 ==
    // DAT_801E46A8 == 0x13`) and on browse row 0. It walks the four
    // armaments against the Best Equipment picks and then prints the
    // ATK / UDF / LDF words of the live block (`0x801EF08C..94`) with the
    // picks' block (`0x801EF0AC..B4`) beside any that differ.
    let on_best_row = matches!(
        session.state(),
        EquipState::SlotPicker { cursor } if cursor == crate::equip_session::SLOT_BROWSE_BEST_ROW
    ) && !session.slot_cursor_hidden();
    let (stat_compare, best_changes) = if on_best_row {
        let picks = session.best_equipment_now();
        let mut trial = *record;
        let mut changes = Vec::new();
        for (row, (&slot, &pick)) in crate::equip_session::ARMAMENT_ENGINE_SLOTS
            .iter()
            .zip(picks.iter())
            .enumerate()
        {
            if record.equip.get(slot).copied() != Some(pick) {
                changes.push((row as u8, name_of(pick)));
                trial.equip[slot] = pick;
            }
        }
        let cur = menu_stat_block(record, session.equipment(), 0, 0);
        let new = menu_stat_block(&trial, session.equipment(), 0, 0);
        let w = |b: &[i32; 8], i: usize| b[i].clamp(0, i32::from(u16::MAX)) as u16;
        (
            vec![
                ("ATK", w(&cur, 3), w(&new, 3)),
                ("UDF", w(&cur, 4), w(&new, 4)),
                ("LDF", w(&cur, 5), w(&new, 5)),
            ],
            changes,
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // Window 24's panel: the hovered candidate's own info row, resolved
    // through the same text tables the Items screen's window 17 uses. Retail
    // gates the panel on the staged id, so the Remove row (`id == 0`) leaves
    // it empty rather than describing nothing.
    let info = considered_id.filter(|&id| id != 0).map(|id| {
        let t = text.map(|f| f(id)).unwrap_or_default();
        EquipItemInfoModel {
            name: if t.name.is_empty() {
                format!("Item {id:02X}")
            } else {
                t.name
            },
            count: u16::from(session.inventory().get(&id).copied().unwrap_or(0)),
            desc: t.desc,
            passive: t.passive,
        }
    });

    // Window 25's panel. Retail's renderer bails outright when nothing is
    // staged (`beq v0, zero` on `DAT_801E46B0` at `0x801D12BC`), so the
    // panel appears with the candidate list and goes with it - the same
    // life-cycle as window 24 beside it.
    let compare = compare.as_ref().and_then(|ctx| {
        let staged = considered_id?;
        let hp_mp = ctx.record.hp_mp_sp();
        let equipped_id = record.equip.get(active_slot as usize).copied().unwrap_or(0);
        let mut trial = *record;
        if let Some(slot) = trial.equip.get_mut(active_slot as usize) {
            *slot = staged;
        }
        Some(EquipCompareModel {
            name: party_names
                .get(char_slot as usize)
                .cloned()
                .unwrap_or_default(),
            current: menu_stat_block(record, session.equipment(), hp_mp.hp_max, hp_mp.mp_max),
            candidate: menu_stat_block(&trial, session.equipment(), hp_mp.hp_max, hp_mp.mp_max),
            hp_max: hp_mp.hp_max,
            mp_max: hp_mp.mp_max,
            // Retail's browse ROW, not the engine's `EquipSlot` index. The
            // two part company at index 3: retail's rows resolve through the
            // two-table map the armament writer uses - row 0 takes the
            // per-character weapon halfword off `0x8007B42C` (`2, 3, 2`,
            // indexed by the roster slot), rows 1 and up take `0x801E43E8` =
            // `00 01 00 04 05 06 07` - so the browse order is weapon,
            // helmet, body, footwear, Goods x3, while `EquipSlot` inserts a
            // `HandGuard` the disc has no byte for. Feeding the slot index
            // straight through made footwear row `4`, and the `slti v0, s0,
            // 4` guard at `0x801D137C` then resolved a compare category
            // there - a retail capture of that row shows the ATK / UDF / LDF
            // triple, the `CATEGORY_DEFAULT` fallback, on every candidate.
            // The engine-only Hand Guard row has no retail row at all, so it
            // reports `-1`: below the guard, and not a row number retail
            // would ever pass.
            slot_row: crate::equip_session::retail_slot_row_for_engine_slot(active_slot)
                .unwrap_or(-1),
            staged_id: i32::from(staged),
            staged_category: compare_category_for_item(staged, ctx),
            equipped_id,
            equipped_category: compare_category_for_item(equipped_id, ctx),
        })
    });

    EquipScreenModel {
        info,
        compare,
        // The party window lists the present party (retail's
        // `DAT_80084594`-long member list), not every roster record; an empty
        // `present` keeps the roster order.
        party_names: if present.is_empty() {
            party_names.to_vec()
        } else {
            present
                .iter()
                .filter_map(|&s| party_names.get(usize::from(s)).cloned())
                .collect()
        },
        party_row: present
            .iter()
            .position(|&s| s == char_slot)
            .map_or(char_slot, |r| r as u8),
        slot_labels,
        slot_items,
        candidate_names,
        candidate_counts,
        stat_compare,
        best_changes,
        phase,
        cursor,
        // The row the active slot draws on (its place in the browse order).
        active_slot: crate::equip_session::browse_row_for_slot(active_slot) - 1,
        confirm_label,
        char_slot,
        slot_cursor: match session.state() {
            EquipState::SlotPicker { .. } if session.slot_cursor_hidden() => None,
            EquipState::SlotPicker { cursor } => Some(cursor as u16),
            _ => None,
        },
        pictogram_rows: crate::equip_session::BROWSE_SLOT_ROWS,
    }
}
