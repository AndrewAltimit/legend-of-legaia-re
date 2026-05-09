//! Per-character art tables: art-name lookup, Learned Art Constant indexing,
//! and Art Anim Data slot ↔ name resolution.
//!
//! Three distinct per-character tables co-exist:
//!
//! - **Action Constant** (`0x00..=0x32`). The byte the action queue stores
//!   when the runtime decides what each actor is doing this turn. Bytes
//!   `0x1B..=0x32` are art constants whose *name* depends on the character
//!   (so `0x1B` = "Vahn's Craze" / "Noa's Ark" / "Biron Rage"). Use
//!   [`art_name`].
//!
//! - **Learned Art Constant** (`0x00..=0x10`). The slot index used by the
//!   runtime when deciding which arts a character has *unlocked*. Stored
//!   per character at:
//!
//!   | Character | RAM             |
//!   |-----------|-----------------|
//!   | Vahn      | `0x8008488D`    |
//!   | Noa       | `0x80084CA1`    |
//!   | Gala      | `0x8008506C`    |
//!
//!   This index has **holes** for some characters (Noa skips slots 2 and 3
//!   because she doesn't have Fire Blow / Tornado Flame in those positions),
//!   so it is **not** a 1:1 offset into the Action Constant range. Use
//!   [`learned_art_action`].
//!
//! - **Art Anim Data** (`0..=0x12`). Selects the per-character art animation
//!   record. The on-disc Art Record's `anim_index` field at +16 indexes this
//!   table. Slot `0` is always Spirit, slot `3` is always Art Starter. Use
//!   [`art_anim_name`].
//!
//! Source: external researcher's `Action Constant`, `Learned Art Constant`,
//! and `Art Anim Data` spreadsheets.

use crate::queue::{ActionConstant, Character};

/// Look up a character-specific art name for an action constant.
///
/// Returns `None` for non-art constants (`0x00..=0x1A`) and for character /
/// art combinations that don't have a learnable art (e.g. Gala has no
/// `0x30..=0x32`).
pub fn art_name(character: Character, action: ActionConstant) -> Option<&'static str> {
    if !action.is_art() {
        return None;
    }
    let idx = (action.as_byte() - 0x1B) as usize;
    let table = match character {
        Character::Vahn => &VAHN_ART_NAMES,
        Character::Noa => &NOA_ART_NAMES,
        Character::Gala => &GALA_ART_NAMES,
    };
    table.get(idx).copied().flatten()
}

/// Test if an action constant resolves to an art for the given character.
pub fn is_art(character: Character, action: ActionConstant) -> bool {
    art_name(character, action).is_some()
}

/// Resolve a character's Learned Art Constant slot (`0..=0x10`) to the
/// action constant the runtime queues, respecting holes (Noa skips slots
/// `0x02` and `0x03`; Vahn / Gala stop at slot `0x0E`).
///
/// Returns `None` for slots the character doesn't have a learnable art at,
/// even if the slot is below the maximum learned index for some other
/// character. Use [`learned_art_max_slot`] to bound iteration.
pub fn learned_art_action(character: Character, slot: u8) -> Option<ActionConstant> {
    let table: &[u8] = match character {
        Character::Vahn => &VAHN_LEARNED_ART_SLOTS,
        Character::Noa => &NOA_LEARNED_ART_SLOTS,
        Character::Gala => &GALA_LEARNED_ART_SLOTS,
    };
    let raw = *table.get(slot as usize)?;
    if raw == 0 {
        return None;
    }
    ActionConstant::from_byte(raw)
}

/// Highest defined Learned Art Constant slot for a character, inclusive.
/// Iterate `0..=learned_art_max_slot(c)` and skip `None` returns from
/// [`learned_art_action`] to walk a character's learned arts in slot order.
pub fn learned_art_max_slot(character: Character) -> u8 {
    let table: &[u8] = match character {
        Character::Vahn => &VAHN_LEARNED_ART_SLOTS,
        Character::Noa => &NOA_LEARNED_ART_SLOTS,
        Character::Gala => &GALA_LEARNED_ART_SLOTS,
    };
    let mut last = 0u8;
    for (i, &b) in table.iter().enumerate() {
        if b != 0 {
            last = i as u8;
        }
    }
    last
}

/// Total number of arts a character has learnable through the Learned Art
/// Constant table — i.e. the count of non-empty slots in `0..=max_slot`.
pub fn learned_art_count(character: Character) -> usize {
    let table: &[u8] = match character {
        Character::Vahn => &VAHN_LEARNED_ART_SLOTS,
        Character::Noa => &NOA_LEARNED_ART_SLOTS,
        Character::Gala => &GALA_LEARNED_ART_SLOTS,
    };
    table.iter().filter(|&&b| b != 0).count()
}

/// Look up the per-character art animation record name for an `anim_index`
/// (the byte at offset +16 of an Art Record). Slot `0` is Spirit on every
/// character, slot `3` is Art Starter. Returns `None` for unused slots
/// (e.g. Gala has no slot `0xC`).
pub fn art_anim_name(character: Character, anim_index: u8) -> Option<&'static str> {
    let table: &[Option<&'static str>] = match character {
        Character::Vahn => &VAHN_ART_ANIM_NAMES,
        Character::Noa => &NOA_ART_ANIM_NAMES,
        Character::Gala => &GALA_ART_ANIM_NAMES,
    };
    table.get(anim_index as usize).copied().flatten()
}

/// Highest defined Art Anim Data slot for a character. Iterate
/// `0..=art_anim_max_slot(c)` to walk all anim records.
pub fn art_anim_max_slot(character: Character) -> u8 {
    let table: &[Option<&'static str>] = match character {
        Character::Vahn => &VAHN_ART_ANIM_NAMES,
        Character::Noa => &NOA_ART_ANIM_NAMES,
        Character::Gala => &GALA_ART_ANIM_NAMES,
    };
    let mut last = 0u8;
    for (i, slot) in table.iter().enumerate() {
        if slot.is_some() {
            last = i as u8;
        }
    }
    last
}

// ---------------------------------------------------------------------------
// Per-character art name tables (action 0x1B..=0x32 → name).
// ---------------------------------------------------------------------------

const VAHN_ART_NAMES: [Option<&'static str>; 24] = [
    /* 0x1B */ Some("Vahn's Craze"),
    /* 0x1C */ Some("Burning Flare"),
    /* 0x1D */ Some("Fire Blow"),
    /* 0x1E */ Some("Tornado Flame (Hyper)"),
    /* 0x1F */ Some("Cyclone"),
    /* 0x20 */ Some("Hurricane"),
    /* 0x21 */ Some("PK Combo"),
    /* 0x22 */ Some("Spin Combo"),
    /* 0x23 */ Some("Pyro Pummel"),
    /* 0x24 */ Some("Cross Kick"),
    /* 0x25 */ Some("Power Punch"),
    /* 0x26 */ Some("Slash Kick"),
    /* 0x27 */ Some("Somersault"),
    /* 0x28 */ Some("Charging Scorch"),
    /* 0x29 */ Some("Hyper Elbow"),
    /* 0x2A */ Some("Tornado Flame (Miracle)"),
    /* 0x2B */ Some("Tri-Somersault"),
    /* 0x2C */ Some("Maximum Blow"),
    /* 0x2D */ Some("Fire Tackle"),
    /* 0x2E */ Some("Power Slash"),
    /* 0x2F */ Some("Rolling Combo 1"),
    /* 0x30 */ Some("Rolling Combo 2"),
    /* 0x31 */ None,
    /* 0x32 */ None,
];

const NOA_ART_NAMES: [Option<&'static str>; 24] = [
    /* 0x1B */ Some("Noa's Ark"),
    /* 0x1C */ Some("Hurricane Kick 1"),
    /* 0x1D */ Some("Hurricane Kick 2"),
    /* 0x1E */ Some("Hurricane Kick 3"),
    /* 0x1F */ Some("Vulture Blade"),
    /* 0x20 */ Some("Frost Breath"),
    /* 0x21 */ Some("Tempest Break"),
    /* 0x22 */ Some("Rushing Gale"),
    /* 0x23 */ Some("Tough Love"),
    /* 0x24 */ Some("Swan Driver"),
    /* 0x25 */ Some("Bird Step"),
    /* 0x26 */ Some("Dolphin Attack"),
    /* 0x27 */ Some("Mirage Lancer"),
    /* 0x28 */ Some("Blizzard Bash"),
    /* 0x29 */ Some("Sonic Javelin"),
    /* 0x2A */ Some("Acrobatic Blitz"),
    /* 0x2B */ Some("Lizard Tail"),
    /* 0x2C */ Some("Jurassic Blow 1"),
    /* 0x2D */ Some("Jurassic Blow 2"),
    /* 0x2E */ Some("Triple Lizard"),
    /* 0x2F */ Some("Super Javelin"),
    /* 0x30 */ Some("Super Tempest"),
    /* 0x31 */ Some("Love You"),
    /* 0x32 */ Some("Dragon Fangs"),
];

const GALA_ART_NAMES: [Option<&'static str>; 24] = [
    /* 0x1B */ Some("Biron Rage"),
    /* 0x1C */ Some("Explosive Fist"),
    /* 0x1D */ Some("Lightning Storm"),
    /* 0x1E */ Some("Thunder Punch (Hyper)"),
    /* 0x1F */ Some("Bull Horns"),
    /* 0x20 */ Some("Electro Thrash"),
    /* 0x21 */ Some("Neo Raising"),
    /* 0x22 */ Some("Black Rain"),
    /* 0x23 */ Some("Side Kick"),
    /* 0x24 */ Some("Head-Splitter"),
    /* 0x25 */ Some("Guillotine"),
    /* 0x26 */ Some("Back Punch"),
    /* 0x27 */ Some("Ironhead"),
    /* 0x28 */ Some("Battering Ram"),
    /* 0x29 */ Some("Flying Knee Attack"),
    /* 0x2A */ Some("Thunder Punch (Miracle)"),
    /* 0x2B */ Some("Back Punch x3"),
    /* 0x2C */ Some("Super Ironhead"),
    /* 0x2D */ Some("Rushing Crush"),
    /* 0x2E */ Some("Heaven's Drop"),
    /* 0x2F */ Some("Neo Static Raising"),
    /* 0x30 */ None,
    /* 0x31 */ None,
    /* 0x32 */ None,
];

// ---------------------------------------------------------------------------
// Learned Art Constant — slot index → action constant byte. `0x00` marks a
// hole (slot exists for some characters but not this one).
//
// Source: per-row tables in the researcher's "Learned Art Constant"
// spreadsheet. Slot 0 is always 0x1B (Miracle Art starter). Vahn and Gala
// have no holes; Noa skips slots 2 and 3 (her Hurricane Kick covers all
// three on-disc levels through a single learned slot).
// ---------------------------------------------------------------------------

const VAHN_LEARNED_ART_SLOTS: [u8; 0x0F] = [
    /* 0x00 */ 0x1B, /* Vahn's Craze */
    /* 0x01 */ 0x1C, /* Burning Flare */
    /* 0x02 */ 0x1D, /* Fire Blow */
    /* 0x03 */ 0x1E, /* Tornado Flame (Hyper) */
    /* 0x04 */ 0x1F, /* Cyclone */
    /* 0x05 */ 0x20, /* Hurricane */
    /* 0x06 */ 0x21, /* PK Combo */
    /* 0x07 */ 0x22, /* Spin Combo */
    /* 0x08 */ 0x23, /* Pyro Pummel */
    /* 0x09 */ 0x24, /* Cross Kick */
    /* 0x0A */ 0x25, /* Power Punch */
    /* 0x0B */ 0x26, /* Slash Kick */
    /* 0x0C */ 0x27, /* Somersault */
    /* 0x0D */ 0x28, /* Charging Scorch */
    /* 0x0E */ 0x29, /* Hyper Elbow */
];

const NOA_LEARNED_ART_SLOTS: [u8; 0x11] = [
    /* 0x00 */ 0x1B, /* Noa's Ark */
    /* 0x01 */ 0x1C, /* Hurricane Kick (3 levels share this slot) */
    /* 0x02 */ 0x00, /* hole — Noa has no Fire Blow */
    /* 0x03 */ 0x00, /* hole — Noa has no Tornado Flame */
    /* 0x04 */ 0x1F, /* Vulture Blade */
    /* 0x05 */ 0x20, /* Frost Breath */
    /* 0x06 */ 0x21, /* Tempest Break */
    /* 0x07 */ 0x22, /* Rushing Gale */
    /* 0x08 */ 0x23, /* Tough Love */
    /* 0x09 */ 0x24, /* Swan Driver */
    /* 0x0A */ 0x25, /* Bird Step */
    /* 0x0B */ 0x26, /* Dolphin Attack */
    /* 0x0C */ 0x27, /* Mirage Lancer */
    /* 0x0D */ 0x28, /* Blizzard Bash */
    /* 0x0E */ 0x29, /* Sonic Javelin */
    /* 0x0F */ 0x2A, /* Acrobatic Blitz */
    /* 0x10 */ 0x2B, /* Lizard Tail */
];

const GALA_LEARNED_ART_SLOTS: [u8; 0x0F] = [
    /* 0x00 */ 0x1B, /* Biron Rage */
    /* 0x01 */ 0x1C, /* Explosive Fist */
    /* 0x02 */ 0x1D, /* Lightning Storm */
    /* 0x03 */ 0x1E, /* Thunder Punch (Hyper) */
    /* 0x04 */ 0x1F, /* Bull Horns */
    /* 0x05 */ 0x20, /* Electro Thrash */
    /* 0x06 */ 0x21, /* Neo Raising */
    /* 0x07 */ 0x22, /* Black Rain */
    /* 0x08 */ 0x23, /* Side Kick */
    /* 0x09 */ 0x24, /* Head-Splitter */
    /* 0x0A */ 0x25, /* Guillotine */
    /* 0x0B */ 0x26, /* Back Punch */
    /* 0x0C */ 0x27, /* Ironhead */
    /* 0x0D */ 0x28, /* Battering Ram */
    /* 0x0E */ 0x29, /* Flying Knee Attack */
];

// ---------------------------------------------------------------------------
// Art Anim Data — anim_index byte (Art Record +16) → animation slot name.
// Slot 0 is always Spirit, slot 3 is always Art Starter on every character.
// Holes are real (e.g. Vahn has no slot 0xC, Gala has no slot 9).
//
// Source: external researcher's "Art Anim Data" spreadsheet.
// ---------------------------------------------------------------------------

const VAHN_ART_ANIM_NAMES: [Option<&'static str>; 0x12] = [
    /* 0x00 */ Some("Spirit"),
    /* 0x01 */ Some("Power Punch"),
    /* 0x02 */ Some("Slash Kick"),
    /* 0x03 */ Some("Art Starter"),
    /* 0x04 */ Some("Tornado Flame"),
    /* 0x05 */ Some("Hurricane"),
    /* 0x06 */ Some("Charging Scorch"),
    /* 0x07 */ Some("PK Combo"),
    /* 0x08 */ Some("Fire Blow"),
    /* 0x09 */ Some("Somersault"),
    /* 0x0A */ Some("Cyclone"),
    /* 0x0B */ Some("Hyper Elbow"),
    /* 0x0C */ None,
    /* 0x0D */ Some("Burning Flare"),
    /* 0x0E */ Some("Spin Combo"),
    /* 0x0F */ Some("Pyro Pummel"),
    /* 0x10 */ Some("Cross Kick"),
    /* 0x11 */ Some("Acrobatic Blitz"),
];

const NOA_ART_ANIM_NAMES: [Option<&'static str>; 0x12] = [
    /* 0x00 */ Some("Spirit"),
    /* 0x01 */ Some("Tempest Break"),
    /* 0x02 */ Some("Tough Love"),
    /* 0x03 */ Some("Art Starter"),
    /* 0x04 */ Some("Hurricane Kick 1"),
    /* 0x05 */ Some("Hurricane Kick 2"),
    /* 0x06 */ Some("Rushing Gale"),
    /* 0x07 */ Some("Swan Driver"),
    /* 0x08 */ Some("Frost Breath"),
    /* 0x09 */ Some("Lizard Tail"),
    /* 0x0A */ Some("Jurassic Blow 2"),
    /* 0x0B */ Some("Bird Step"),
    /* 0x0C */ Some("Dolphin Attack"),
    /* 0x0D */ Some("Vulture Blade"),
    /* 0x0E */ Some("Mirage Lancer"),
    /* 0x0F */ Some("Blizzard Bash"),
    /* 0x10 */ Some("Sonic Javelin"),
    /* 0x11 */ Some("Electro Thrash"),
];

const GALA_ART_ANIM_NAMES: [Option<&'static str>; 0x13] = [
    /* 0x00 */ Some("Spirit"),
    /* 0x01 */ Some("Bull Horns"),
    /* 0x02 */ Some("Head-Splitter"),
    /* 0x03 */ Some("Art Starter"),
    /* 0x04 */ Some("Lightning Storm"),
    /* 0x05 */ Some("Back Punch"),
    /* 0x06 */ Some("Ironhead"),
    /* 0x07 */ Some("Battering Ram"),
    /* 0x08 */ Some("Flying Knee Attack"),
    /* 0x09 */ None,
    /* 0x0A */ Some("Thunder Punch"),
    /* 0x0B */ Some("Guillotine"),
    /* 0x0C */ Some("Explosive Fist"),
    /* 0x0D */ Some("Black Rain"),
    /* 0x0E */ None,
    /* 0x0F */ None,
    /* 0x10 */ Some("Side Kick"),
    /* 0x11 */ None,
    /* 0x12 */ Some("Neo Raising"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_zero_is_miracle_starter_for_each_character() {
        assert_eq!(
            learned_art_action(Character::Vahn, 0),
            Some(ActionConstant::Art1B)
        );
        assert_eq!(
            learned_art_action(Character::Noa, 0),
            Some(ActionConstant::Art1B)
        );
        assert_eq!(
            learned_art_action(Character::Gala, 0),
            Some(ActionConstant::Art1B)
        );
    }

    #[test]
    fn art_names_resolve_per_character() {
        let craze = ActionConstant::from_byte(0x1B).unwrap();
        assert_eq!(art_name(Character::Vahn, craze), Some("Vahn's Craze"));
        assert_eq!(art_name(Character::Noa, craze), Some("Noa's Ark"));
        assert_eq!(art_name(Character::Gala, craze), Some("Biron Rage"));

        let cyclone = ActionConstant::from_byte(0x1F).unwrap();
        assert_eq!(art_name(Character::Vahn, cyclone), Some("Cyclone"));
        assert_eq!(art_name(Character::Noa, cyclone), Some("Vulture Blade"));
        assert_eq!(art_name(Character::Gala, cyclone), Some("Bull Horns"));
    }

    #[test]
    fn non_art_constants_have_no_name() {
        assert_eq!(art_name(Character::Vahn, ActionConstant::Attack), None);
        assert_eq!(art_name(Character::Noa, ActionConstant::Spirit), None);
        assert_eq!(
            art_name(Character::Gala, ActionConstant::RegularStarter),
            None
        );
    }

    #[test]
    fn gala_lacks_late_arts() {
        // Gala has no 0x30/0x31/0x32 (only 21 arts, ending at 0x2F Neo
        // Static Raising).
        assert_eq!(
            art_name(Character::Gala, ActionConstant::from_byte(0x30).unwrap()),
            None
        );
        assert_eq!(
            art_name(Character::Gala, ActionConstant::from_byte(0x31).unwrap()),
            None
        );

        // Noa is the only character with 0x32 Dragon Fangs.
        assert_eq!(
            art_name(Character::Noa, ActionConstant::from_byte(0x32).unwrap()),
            Some("Dragon Fangs")
        );
        assert_eq!(
            art_name(Character::Vahn, ActionConstant::from_byte(0x31).unwrap()),
            None
        );
    }

    #[test]
    fn learned_art_counts_match_research_data() {
        // Source: Learned Art Constant spreadsheet.
        // Vahn: 15 slots (0x00..=0x0E, no holes).
        // Noa: 17 (0x00..=0x10, holes at 0x02/0x03).
        // Gala: 15 (0x00..=0x0E, no holes).
        assert_eq!(learned_art_count(Character::Vahn), 15);
        assert_eq!(learned_art_count(Character::Noa), 15);
        assert_eq!(learned_art_count(Character::Gala), 15);

        assert_eq!(learned_art_max_slot(Character::Vahn), 0x0E);
        assert_eq!(learned_art_max_slot(Character::Noa), 0x10);
        assert_eq!(learned_art_max_slot(Character::Gala), 0x0E);
    }

    #[test]
    fn noa_learned_art_table_skips_slots_2_and_3() {
        // Per the spreadsheet: Noa's slots 2 and 3 are empty (no Fire Blow,
        // no Tornado Flame); slot 4 is Vulture Blade (action 0x1F).
        assert_eq!(
            learned_art_action(Character::Noa, 1),
            Some(ActionConstant::Art1C)
        );
        assert_eq!(learned_art_action(Character::Noa, 2), None);
        assert_eq!(learned_art_action(Character::Noa, 3), None);
        assert_eq!(
            learned_art_action(Character::Noa, 4),
            Some(ActionConstant::Art1F)
        );
    }

    #[test]
    fn noa_learned_art_extends_to_acrobatic_blitz_and_lizard_tail() {
        // Noa's table goes 2 slots past Vahn / Gala — Acrobatic Blitz at
        // slot 0x0F, Lizard Tail at slot 0x10.
        assert_eq!(
            learned_art_action(Character::Noa, 0x0F),
            Some(ActionConstant::Art2A)
        );
        assert_eq!(
            learned_art_action(Character::Noa, 0x10),
            Some(ActionConstant::Art2B)
        );

        // Vahn / Gala stop at slot 0x0E.
        assert_eq!(learned_art_action(Character::Vahn, 0x0F), None);
        assert_eq!(learned_art_action(Character::Gala, 0x0F), None);
    }

    #[test]
    fn art_anim_slot_zero_is_spirit() {
        assert_eq!(art_anim_name(Character::Vahn, 0), Some("Spirit"));
        assert_eq!(art_anim_name(Character::Noa, 0), Some("Spirit"));
        assert_eq!(art_anim_name(Character::Gala, 0), Some("Spirit"));
    }

    #[test]
    fn art_anim_slot_three_is_starter() {
        assert_eq!(art_anim_name(Character::Vahn, 3), Some("Art Starter"));
        assert_eq!(art_anim_name(Character::Noa, 3), Some("Art Starter"));
        assert_eq!(art_anim_name(Character::Gala, 3), Some("Art Starter"));
    }

    #[test]
    fn art_anim_holes_are_none() {
        // Vahn has no 0x0C; Gala has no 0x09 / 0x0E / 0x0F / 0x11.
        assert_eq!(art_anim_name(Character::Vahn, 0x0C), None);
        assert_eq!(art_anim_name(Character::Gala, 0x09), None);
        assert_eq!(art_anim_name(Character::Gala, 0x0E), None);
        assert_eq!(art_anim_name(Character::Gala, 0x11), None);
    }

    #[test]
    fn art_anim_max_slot_per_character() {
        // Vahn: 0x11 (Acrobatic Blitz). Noa: 0x11 (Electro Thrash).
        // Gala: 0x12 (Neo Raising).
        assert_eq!(art_anim_max_slot(Character::Vahn), 0x11);
        assert_eq!(art_anim_max_slot(Character::Noa), 0x11);
        assert_eq!(art_anim_max_slot(Character::Gala), 0x12);
    }
}
