//! Per-character art name tables and Learned Art Constant mapping.
//!
//! Every art constant `0x1B–0x32` resolves to a different art name per
//! character. The runtime stores these names elsewhere (a per-character
//! string table referenced by the action constant); this crate carries the
//! authoritative mapping for use by tests, the asset viewer, and the engine
//! battle event log.
//!
//! The Learned Art Constant table maps a character's "art slot index" (the
//! ordinal a character has learned an art into) to the action constant that
//! invokes it. RAM addresses for the runtime tables:
//!
//! - Vahn: `0x8008488D`
//! - Noa:  `0x80084CA1`
//! - Gala: `0x8008506C`
//!
//! Source: external researcher's `Learned Art Constant` and `Action
//! Constant` spreadsheets.

use crate::queue::{ActionConstant, Character};

/// Look up a character-specific art name for an action constant.
///
/// Returns `None` for non-art constants (`0x00–0x1A`) and for character/art
/// combinations that don't have a learnable art (e.g. Gala has no `0x31`
/// or `0x32`).
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

/// Resolve a character's "learned art slot" (0..N) to the action constant
/// the runtime queues.
///
/// Slot 0 is always the character's Miracle Art starter (`0x1B` = Vahn's
/// Craze / Noa's Ark / Biron Rage). Slots 1..N follow the on-disc table
/// order — see the per-character `*_LEARNED_ART_SLOTS` constants for the
/// exact ordering.
pub fn learned_art_action(character: Character, slot: u8) -> Option<ActionConstant> {
    let table = match character {
        Character::Vahn => VAHN_LEARNED_ART_SLOTS,
        Character::Noa => NOA_LEARNED_ART_SLOTS,
        Character::Gala => GALA_LEARNED_ART_SLOTS,
    };
    let raw = *table.get(slot as usize)?;
    ActionConstant::from_byte(raw)
}

/// Number of learnable art slots for a character.
pub fn learned_art_slot_count(character: Character) -> usize {
    match character {
        Character::Vahn => VAHN_LEARNED_ART_SLOTS.len(),
        Character::Noa => NOA_LEARNED_ART_SLOTS.len(),
        Character::Gala => GALA_LEARNED_ART_SLOTS.len(),
    }
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
// Learned Art Slots — slot index → action constant byte.
//
// Source: per-row tables in the researcher's "Art Data" spreadsheet. Slot 0
// is always 0x1B (Miracle Art starter); subsequent slots follow on-disc
// order. Records that the researcher annotated as "starter" or "doesn't
// seem to do anything" between the Hyper and finisher rows are skipped
// since they aren't independently usable.
// ---------------------------------------------------------------------------

const VAHN_LEARNED_ART_SLOTS: &[u8] = &[
    0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A,
    0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x30,
];

const NOA_LEARNED_ART_SLOTS: &[u8] = &[
    0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A,
    0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32,
];

const GALA_LEARNED_ART_SLOTS: &[u8] = &[
    0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A,
    0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
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
    fn learned_art_slot_counts_match_character() {
        // Vahn: 22 slots (0x1B..=0x30). Noa: 24 (0x1B..=0x32). Gala: 21 (0x1B..=0x2F).
        assert_eq!(learned_art_slot_count(Character::Vahn), 22);
        assert_eq!(learned_art_slot_count(Character::Noa), 24);
        assert_eq!(learned_art_slot_count(Character::Gala), 21);
    }
}
