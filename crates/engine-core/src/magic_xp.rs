//! Summon-magic spell XP: the per-spell experience that levels Seru magic.
//!
//! Casting Seru magic trains the spell itself. The character record keeps a
//! per-spell-slot u32 XP array at `+0x8` (parallel to the spell-id list at
//! `+0x13D` and the level bytes at `+0x161`); the battle damage finisher
//! accrues into it (`FUN_801ddb30` tail, ported as
//! [`legaia_engine_vm::battle_formulas::summon_spell_xp_gain`]) and the
//! post-summon check `FUN_801e70bc` levels the spell up against the static
//! `SCUS_942.54` threshold table at `0x8007656C` (ported as
//! [`legaia_engine_vm::battle_formulas::summon_magic_levels_up`]).
//!
//! This module owns the data plumbing both kernels need:
//!
//! - [`thresholds_from_scus`] decodes the threshold table off the user's
//!   `SCUS_942.54` (no Sony bytes committed - same pattern as
//!   [`crate::shop_catalog::ShopItemData::from_scus`]);
//! - the record accessors read/write the `+0x8` XP array through
//!   [`legaia_save::CharacterRecord::raw`], so the accrued XP round-trips
//!   through saves for free.
//!
//! The live wiring is `World::cast_spell_on_slots` (accrual + level-up after
//! a party summon cast) - the leveled `+0x161` byte is exactly what the next
//! cast's magic-power stage reads (`caster_magic_power_byte`), closing the
//! cast → XP → level → stronger-cast loop.
//!
//! REF: FUN_801ddb30, FUN_801e70bc

use legaia_save::CharacterRecord;

/// RAM address of the magic-XP threshold table in `SCUS_942.54`
/// (`overlay_battle_action_801e70bc.txt`: `*(ushort *)((level - 1) * 2 +
/// -0x7ff89a94)` = `0x8007656C + (level - 1) * 2`).
pub const THRESHOLDS_VA: u32 = 0x8007_656C;

/// Number of threshold steps: levels 1..=8 each have an entry; level 9 is
/// the cap (`FUN_801e70bc` guards `level < 9` before the increment).
pub const THRESHOLD_STEPS: usize = 8;

/// Character-record offset of the per-spell-slot u32 XP array
/// (`overlay_battle_action_801ddb30.txt:1059`: `slot * 4 + record -
/// 0x7ff7b8f0` = live `0x80084710` = record base `0x80084708` + `0x8`).
pub const SPELL_XP_OFFSET: usize = 0x8;

/// Retail search bound over the spell-id list: both `FUN_801ddb30` and
/// `FUN_801e70bc` scan at most `0x20` entries of the `+0x13D` array (the
/// same bound the magic-power stage uses).
pub const SPELL_SEARCH_BOUND: usize = 0x20;

/// Decode the magic-XP threshold table from `SCUS_942.54` bytes.
///
/// Reads [`THRESHOLD_STEPS`] little-endian u16s at [`THRESHOLDS_VA`] through
/// the PS-X EXE header's `t_addr` (file offset `va - t_addr + 0x800`).
/// Returns `None` when the bytes aren't a PS-X EXE, the address falls outside
/// the text segment, or the table fails its integrity shape (strictly
/// ascending, all non-zero - the retail table is `17, 50, 92, … 536`).
pub fn thresholds_from_scus(scus: &[u8]) -> Option<[u16; THRESHOLD_STEPS]> {
    if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
        return None;
    }
    let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
    let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
    if THRESHOLDS_VA < t_addr
        || THRESHOLDS_VA + (THRESHOLD_STEPS * 2) as u32 > t_addr.checked_add(t_size)?
    {
        return None;
    }
    let off = (THRESHOLDS_VA - t_addr) as usize + 0x800;
    let bytes = scus.get(off..off + THRESHOLD_STEPS * 2)?;
    let mut out = [0u16; THRESHOLD_STEPS];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
    }
    // Integrity: a level-up curve is strictly ascending and starts non-zero.
    if out[0] == 0 || out.windows(2).any(|w| w[0] >= w[1]) {
        return None;
    }
    Some(out)
}

/// Index of `spell_id` in the record's spell-id list (`+0x13D`), scanning at
/// most [`SPELL_SEARCH_BOUND`] entries - the shared lookup both retail
/// functions open with. `None` when the spell isn't in the list (retail would
/// fall through with slot `0x20` and touch bytes past the arrays; the engine
/// skips instead).
pub fn spell_slot(record: &CharacterRecord, spell_id: u8) -> Option<usize> {
    let list = record.spell_list();
    list.ids
        .iter()
        .take(SPELL_SEARCH_BOUND)
        .position(|&id| id == spell_id)
}

/// Accrued XP for the spell at `slot` - the u32 at record `+0x8 + slot * 4`.
pub fn spell_xp(record: &CharacterRecord, slot: usize) -> u32 {
    let off = SPELL_XP_OFFSET + slot * 4;
    legaia_bytes::u32_le(&record.raw, off).unwrap_or(0)
}

/// Add `gain` (saturating) to the spell-XP slot. No-op when the offset is out
/// of range (slot >= [`SPELL_SEARCH_BOUND`] never happens via [`spell_slot`]).
pub fn add_spell_xp(record: &mut CharacterRecord, slot: usize, gain: u32) {
    let off = SPELL_XP_OFFSET + slot * 4;
    if record.raw.len() < off + 4 {
        return;
    }
    let cur = spell_xp(record, slot);
    record.raw[off..off + 4].copy_from_slice(&cur.saturating_add(gain).to_le_bytes());
}

/// PORT: FUN_801E92DC - learn a new Seru spell by **prepending** it to the
/// character's spell list. Retail shifts the three parallel arrays up by one
/// slot in a single descending loop (`id[i+1] = id[i]`, `level[i+1] =
/// level[i]`, `xp[i+1] = xp[i]` for `i = count-1..=0`), then writes slot 0's
/// id, `level = 1`, `xp = 0`, and increments the count. The newest Seru
/// therefore always lists first. (The randomizer's shiny-Seru feature
/// patches exactly this routine's level write + shift - see
/// `crates/patcher/src/shiny_seru/`.)
///
/// ## The stored id is the FULL spell id, not its low byte
///
/// Retail's parameter is the id **minus** `0x80` and the store re-adds it:
/// `801e93a8 addiu v1,t3,0x80` / `801e93ac sb v1,0x705(v0)`, where `t3` is
/// the incoming argument. So the `+0x13D` array holds the full `0x8x` id -
/// which is also what the level-up check compares it against: `801e7124 lbu
/// v1,0x1df(a0)` (the actor's action id) against `801e7128 lbu v0,0x705(v0)`
/// (the array entry), and `801e7150 addiu v1,v0,-0x86` puts that action id in
/// the `0x86..` band. This port therefore takes the id the array stores and
/// writes it unchanged; the `- 0x80` an earlier reading applied stored
/// retail's *argument* instead of its *result*, which would have broken the
/// [`spell_slot`] lookup that scans the same array by full id.
///
/// The shift is bounded to the record's array capacity
/// ([`legaia_save::MAX_SPELLS`]); retail's unbounded `do`/`while` never
/// exceeds it in practice (the id space `0x81..=0x8B` caps a legitimate list
/// at 11 entries).
///
/// Driven from `World::resolve_captures`: every accepted capture whose
/// points crossed a Seru's learn threshold commits the learn into the
/// character record here, beside the `SeruCaptureLog::learned_spells` read
/// model the menus and the battle spell session list from.
pub fn learn_spell_prepend(record: &mut CharacterRecord, spell_id: u8) {
    let mut list = record.spell_list();
    let count = (list.count as usize).min(list.ids.len() - 1);
    // Shift ids/levels (descending, like retail's single loop) ...
    for i in (0..count).rev() {
        list.ids[i + 1] = list.ids[i];
        list.levels[i + 1] = list.levels[i];
    }
    list.ids[0] = spell_id;
    list.levels[0] = 1;
    list.count = list.count.wrapping_add(1);
    record.set_spell_list(list);
    // ... and the parallel u32 XP array at +0x8.
    for i in (0..count).rev() {
        let xp = spell_xp(record, i);
        let off = SPELL_XP_OFFSET + (i + 1) * 4;
        record.raw[off..off + 4].copy_from_slice(&xp.to_le_bytes());
    }
    record.raw[SPELL_XP_OFFSET..SPELL_XP_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
}

/// The banner suffix retail appends after the spell name (battle-action
/// overlay rodata at `0x801F6840 + 4` - the `+4` skips a 4-byte glyph-prefix
/// chain, leaving the possessive suffix).
pub const MAGIC_LEVEL_INCREASED_SUFFIX: &str = "'s magic level increased.";

/// Compose the "magic level increased" banner shown when a Seru spell levels
/// up: the cast spell's name (`DAT_800754D0[actor[+0x1DF]]`, the shared
/// spell-name table) followed by [`MAGIC_LEVEL_INCREASED_SUFFIX`]. Retail
/// builds it into the shared context message buffer `_DAT_8007BD24 + 0x1F9`
/// (string copy `FUN_8003CA78` + append `FUN_8003CAC4`).
///
/// PORT: FUN_801F452C
///
/// Driven from `World::accrue_summon_spell_xp`: the same edge that pushes
/// `World::magic_level_ups` composes this line and stages it on the world's
/// banner channel, resolving the spell name through `World::spell_catalog`.
/// Retail raises it as its own UI element (`0x65`); the engine has one
/// banner slot, so the two share it.
pub fn magic_level_increased_message(spell_name: &str) -> String {
    format!("{spell_name}{MAGIC_LEVEL_INCREASED_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal synthetic PS-X EXE: header + zeroed text with a threshold
    /// table planted at the retail VA. No Sony bytes.
    fn synthetic_scus(table: &[u16]) -> Vec<u8> {
        let t_addr: u32 = 0x8001_0000;
        let t_size: u32 = 0x7_0000;
        let mut scus = vec![0u8; 0x800 + t_size as usize];
        scus[0..8].copy_from_slice(b"PS-X EXE");
        scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        scus[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        let off = (THRESHOLDS_VA - t_addr) as usize + 0x800;
        for (i, v) in table.iter().enumerate() {
            scus[off + i * 2..off + i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        scus
    }

    #[test]
    fn thresholds_decode_from_synthetic_exe() {
        let table = [17u16, 50, 92, 144, 208, 288, 392, 536];
        let scus = synthetic_scus(&table);
        assert_eq!(thresholds_from_scus(&scus), Some(table));
    }

    #[test]
    fn thresholds_reject_non_ascending_or_zero() {
        // Zero first entry.
        let scus = synthetic_scus(&[0, 50, 92, 144, 208, 288, 392, 536]);
        assert_eq!(thresholds_from_scus(&scus), None);
        // Non-ascending.
        let scus = synthetic_scus(&[17, 50, 50, 144, 208, 288, 392, 536]);
        assert_eq!(thresholds_from_scus(&scus), None);
        // Not a PS-X EXE.
        assert_eq!(thresholds_from_scus(&[0u8; 0x1000]), None);
    }

    #[test]
    fn spell_xp_round_trips_through_record_raw() {
        let mut rec = CharacterRecord::zeroed();
        let mut list = rec.spell_list();
        list.count = 2;
        list.ids[0] = 0x81;
        list.ids[1] = 0x87;
        list.levels[0] = 1;
        list.levels[1] = 3;
        rec.set_spell_list(list);

        let slot = spell_slot(&rec, 0x87).expect("spell in list");
        assert_eq!(slot, 1);
        assert_eq!(spell_xp(&rec, slot), 0);
        add_spell_xp(&mut rec, slot, 12);
        add_spell_xp(&mut rec, slot, 5);
        assert_eq!(spell_xp(&rec, slot), 17);
        // The XP lands in the +0x8 u32 array (live 0x80084710 + slot*4).
        assert_eq!(rec.raw[SPELL_XP_OFFSET + 4], 17);
        // Unknown spell: no slot.
        assert_eq!(spell_slot(&rec, 0xA0), None);
    }

    #[test]
    fn spell_slot_respects_retail_search_bound() {
        let mut rec = CharacterRecord::zeroed();
        let mut list = rec.spell_list();
        // MAX_SPELLS (36) > the retail 0x20 bound: a spell parked in slot
        // 0x21 is invisible to the XP path, exactly like retail.
        list.count = 36;
        list.ids[0x21] = 0x8B;
        rec.set_spell_list(list);
        assert_eq!(spell_slot(&rec, 0x8B), None);
    }

    #[test]
    fn learn_prepend_shifts_ids_levels_and_xp() {
        let mut rec = CharacterRecord::zeroed();
        let mut list = rec.spell_list();
        list.count = 2;
        list.ids[0] = 0x83; // level 4
        list.ids[1] = 0x81; // level 2
        list.levels[0] = 4;
        list.levels[1] = 2;
        rec.set_spell_list(list);
        add_spell_xp(&mut rec, 0, 77);
        add_spell_xp(&mut rec, 1, 33);

        learn_spell_prepend(&mut rec, 0x8B);
        let l = rec.spell_list();
        assert_eq!(l.count, 3);
        // The array holds the FULL id (retail stores `arg + 0x80`), which is
        // the same id space `spell_slot` scans by.
        assert_eq!(&l.ids[..3], &[0x8B, 0x83, 0x81]);
        assert_eq!(spell_slot(&rec, 0x8B), Some(0));
        assert_eq!(&l.levels[..3], &[1, 4, 2]);
        assert_eq!(spell_xp(&rec, 0), 0, "new spell starts at zero XP");
        assert_eq!(spell_xp(&rec, 1), 77, "old slot-0 XP moved with it");
        assert_eq!(spell_xp(&rec, 2), 33);
    }

    #[test]
    fn level_up_banner_appends_the_retail_suffix() {
        assert_eq!(
            magic_level_increased_message("Gimard"),
            "Gimard's magic level increased."
        );
    }
}
