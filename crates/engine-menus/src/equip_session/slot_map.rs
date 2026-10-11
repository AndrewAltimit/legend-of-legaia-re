//! Equip slot spaces: browse rows, engine slots and the retail record bytes, with the maps between them.

/// Number of equip slots the engine models - one per record `+0x196` byte.
/// Seven of them are browse rows; the eighth (the Ra-Seru byte, the engine's
/// Hand Guard slot) is carried but never browsed, as in retail.
pub const EQUIP_SLOTS: u8 = 8;

/// Slot rows below Best Equipment in the slot-browse step: retail's seven
/// (`DAT_801E43E8` / `DAT_801E43F4` are seven entries each).
pub const BROWSE_SLOT_ROWS: usize = 7;

/// Rows in the slot-browse cursor space: "Best Equipment" plus one row per
/// browsable slot - retail's own `8` (`li a1,0x8` into the cursor stepper at
/// `0x801D9AFC`, and the shared tail's wrap at 8 after a candidate commit).
pub const SLOT_BROWSE_ROWS: u8 = BROWSE_SLOT_ROWS as u8 + 1;

/// The slot-browse row that runs the Best Equipment applier.
pub const SLOT_BROWSE_BEST_ROW: u8 = 0;

/// Engine [`crate::equipment::EquipSlot`] index each slot-browse row below
/// Best Equipment shows, in row order: retail's seven rows - weapon, helmet,
/// body armour, footwear, Goods x3, the pictogram codes `DAT_801E43F4`.
///
/// The engine's Hand Guard slot (index `3`, the per-character Ra-Seru byte
/// `0x8007B424`) has no row: retail's browse resolves rows through the
/// weapon halfword and `DAT_801E43E8` = `00 01 00 04 05 06 07`, neither of
/// which ever names the Ra-Seru byte, so the Ra-Seru is shown and changed
/// nowhere on the Equip screen.
pub const BROWSE_SLOT_ORDER: [u8; BROWSE_SLOT_ROWS] = [0, 1, 2, 4, 5, 6, 7];

/// The engine slot browse row `row` (`1..`) opens.
pub fn slot_for_browse_row(row: u8) -> u8 {
    BROWSE_SLOT_ORDER[usize::from(row.saturating_sub(1)).min(BROWSE_SLOT_ROWS - 1)]
}

/// The browse row (`1..`) showing engine slot `slot`. The unbrowsed Hand
/// Guard slot answers row `1`.
pub fn browse_row_for_slot(slot: u8) -> u8 {
    BROWSE_SLOT_ORDER
        .iter()
        .position(|&s| s == slot)
        .map_or(1, |i| i as u8 + 1)
}

/// Engine slot -> record `+0x196` byte for the character in roster slot
/// `party_slot`: the weapon on its per-character byte (`DAT_8007B42C`), the
/// Hand Guard row on the other one of the pair (`0x8007B424`, the Ra-Seru
/// byte), body armour on byte `0`, the rest in place. A bijection per
/// character, so a record round-trips through the engine's slot order.
pub fn record_byte_for_engine_slot(slot: u8, party_slot: u8) -> usize {
    let p = usize::from(party_slot).min(2);
    match slot {
        0 => RETAIL_WEAPON_EQUIP_BYTE[p] as usize,
        2 => 0,
        3 => RETAIL_RASERU_EQUIP_BYTE[p] as usize,
        s => usize::from(s),
    }
}

/// A record's `+0x196` equip bytes in the engine's slot order.
pub fn engine_equip_from_record(bytes: [u8; 8], party_slot: u8) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (slot, o) in out.iter_mut().enumerate() {
        *o = bytes[record_byte_for_engine_slot(slot as u8, party_slot)];
    }
    out
}

/// Item id standing for the candidate list's **Remove** row - retail's
/// class-`0x4000` payload-`0` entry, which carries no item at all.
pub const REMOVE_ROW_ID: u8 = 0;

/// Engine equip-slot index each of the four **armament** slots writes, in
/// the candidate array's weapon-first order.
///
/// Retail resolves the same thing through the halfword table
/// `DAT_801E43E8` (and, for the weapon, the per-character offset at
/// `DAT_8007B42C + char*2`); `FUN_801CF760` indexes it by the armament
/// index rather than writing `record.equip[i]` directly, which matters here
/// because the engine inserts a Hand Guard slot at index `3` that retail
/// does not have - so footwear is engine slot `4`, not `3`.
pub const ARMAMENT_ENGINE_SLOTS: [usize; 4] = [0, 1, 2, 4];

/// Per-character weapon equip byte, retail's `DAT_8007B42C` halfwords
/// (`2, 3, 2`): Vahn's and Gala's weapon lives in equip byte `2`, Noa's in
/// byte `3`.
pub const RETAIL_WEAPON_EQUIP_BYTE: [i16; 3] = [2, 3, 2];

/// Per-character **Ra-Seru** equip byte, the halfword table at
/// `0x8007B424` (`3, 2, 3`) - the complement of
/// [`RETAIL_WEAPON_EQUIP_BYTE`], so each character's weapon and Ra-Seru
/// occupy the two bytes the other does not.
///
/// The screens that gate on "does this character have a Ra-Seru equipped"
/// read `record[0x196 + table[char]]`, not a fixed slot: the record screen's
/// spell list does it with a `lui 0x8008` / `addiu -0x4bdc` pair at
/// `0x801DA5D0..0x801DA600` before it reads the learned-spell count.
pub const RETAIL_RASERU_EQUIP_BYTE: [i16; 3] = [3, 2, 3];

/// Engine [`EquipSlot`] index each retail `+0x196` equip byte maps to.
///
/// Retail's array is `[body, head, weapon, weapon, footwear, goods x3]`;
/// the engine's is weapon-first with a Hand Guard row retail has no byte
/// for, so bytes `2` and `3` (the two per-character weapon bytes) both fold
/// onto engine slot `0` and engine slot `3` has no retail byte at all.
pub const RETAIL_EQUIP_BYTE_TO_ENGINE_SLOT: [u8; 8] = [2, 1, 0, 0, 4, 5, 6, 7];

/// Engine [`EquipSlot`] index the engine's Hand Guard row occupies. It has
/// no retail counterpart, so the retail applier cannot resolve a
/// destination for it and the browse row stands.
pub const ENGINE_ONLY_HAND_GUARD_SLOT: u8 = 3;

/// The retail equip-screen slot row (`FUN_801E5A08`'s third argument) an
/// engine [`EquipSlot`] index is confirmed from, or `None` for the engine's
/// own Hand Guard row.
///
/// Retail's rows are `weapon, helmet, armor, boot, goods x3`; rows `0..3`
/// are the armaments whose destination the applier resolves from the item's
/// equip class, and rows `>= 4` are the Goods rows it writes verbatim at
/// `row + 1`.
pub fn retail_slot_row_for_engine_slot(engine_slot: u8) -> Option<i32> {
    match engine_slot {
        0 => Some(0), // weapon
        1 => Some(1), // helmet
        2 => Some(2), // body armour
        4 => Some(3), // footwear
        5..=7 => Some(engine_slot as i32 - 1),
        _ => None, // Hand Guard - engine-only row
    }
}

/// The equipment-record `+7` slot bits for a disc equip category, i.e. the
/// inverse of `legaia_game_tables::equip_stats::EquipBonus::slot`.
pub(super) fn slot_bits_of_category(cat: legaia_game_tables::equip_stats::EquipSlot) -> u8 {
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    match cat {
        Disc::Body => 0x00,
        Disc::Head => 0x20,
        Disc::Weapon => 0x40,
        Disc::Footwear => 0x60,
    }
}
