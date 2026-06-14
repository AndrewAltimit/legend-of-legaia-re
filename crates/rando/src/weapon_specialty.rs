//! Weapon-specialty randomizer.
//!
//! Reassigns which weapon **class** each character specializes in. In retail,
//! equipping a weapon outside a character's favored class makes that character's
//! **arm** command (action-gauge command `0x0C`) cost more AP, so fewer commands
//! fit in an arts combo. The cost is *not* a runtime favored-class comparison -
//! it is a per-(character, weapon) byte baked into the player battle files,
//! copied verbatim into the runtime gauge at battle load. See
//! [`docs/subsystems/arts-command-gauge.md`].
//!
//! The byte lives inside each weapon's LZS-compressed section, at
//! `decoded_section[+0x04]` (the swing-action record offset) `+ 0x74`. A
//! favored-class weapon carries [`FAVORED_COST`]; an off-class weapon carries a
//! higher cost. This randomizer permutes the three favored families among the
//! three characters and rewrites those bytes so each character's new favored
//! family is single-cost and every other class is off-class.
//!
//! Note: the retail data has a rare far-off-class tier (`0x36`, e.g. Noa's
//! clubs/axes); this pass normalizes every non-favored weapon to the standard
//! off-class [`OFFCLASS_COST`]. The Astral Sword and other non-class weapons
//! (no [`weapon_family`]) are left untouched, preserving their always-wide arm.

use crate::rng::SplitMix64;

/// Arm cost of a favored-class weapon (single-width arm input).
pub const FAVORED_COST: u8 = 0x1E;
/// Arm cost assigned to every off-class weapon.
pub const OFFCLASS_COST: u8 = 0x2A;

/// Offset of the swing-record pointer in a decoded section header.
const SWING_PTR_OFF: usize = 0x04;
/// Offset of the arm cost inside the swing record.
const ARM_COST_IN_SWING: usize = 0x74;

/// A weapon class family - the unit a character specializes in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// Blades, knives, swords, Vahn's fist - Vahn's vanilla specialty.
    Blade,
    /// Claws, ferals, fangs - Noa's vanilla specialty.
    Claw,
    /// Clubs, axes, maces - Gala's vanilla specialty.
    Club,
}

impl Family {
    /// Lower-case label for reports.
    pub fn label(self) -> &'static str {
        match self {
            Family::Blade => "blade",
            Family::Claw => "claw",
            Family::Club => "club",
        }
    }
}

/// Map an equippable item id to its weapon family, or `None` for non-class
/// weapons (the Astral Sword `0xBA`, armor, accessories) which are never
/// reassigned.
pub fn weapon_family(id: u8) -> Option<Family> {
    match id {
        0x1A | 0x1B | 0x22..=0x27 => Some(Family::Blade),
        0x1C..=0x1F | 0x28..=0x2D => Some(Family::Claw),
        0x20 | 0x21 | 0x2E..=0x33 => Some(Family::Club),
        _ => None,
    }
}

/// One player battle file: its PROT entry index, character label, and vanilla
/// favored family.
pub struct Player {
    /// PROT entry index of the player battle file.
    pub entry: usize,
    /// Character name (for reports).
    pub name: &'static str,
    /// The character's favored family in retail.
    pub vanilla: Family,
}

/// The three randomizable player battle files (Vahn `0863`, Noa `0864`, Gala
/// `0865`). Terra's `0866` carries only a handful of records and no arm-cost
/// sections, so it is left out.
pub const PLAYERS: [Player; 3] = [
    Player {
        entry: 863,
        name: "Vahn",
        vanilla: Family::Blade,
    },
    Player {
        entry: 864,
        name: "Noa",
        vanilla: Family::Claw,
    },
    Player {
        entry: 865,
        name: "Gala",
        vanilla: Family::Club,
    },
];

/// Assign each of the three characters a favored family - a seeded permutation
/// of `{Blade, Claw, Club}` (index `i` is the family for `PLAYERS[i]`).
pub fn plan_favored(seed: u64) -> [Family; 3] {
    let mut fams = [Family::Blade, Family::Claw, Family::Club];
    let mut rng = SplitMix64::new(seed);
    rng.shuffle(&mut fams);
    fams
}

/// Locate the arm-cost byte offset within a decoded weapon section, or `None`
/// when the section has no swing record or the offset would fall out of bounds.
pub fn arm_cost_offset(decoded: &[u8]) -> Option<usize> {
    let raw = decoded.get(SWING_PTR_OFF..SWING_PTR_OFF + 4)?;
    let swing = u32::from_le_bytes(raw.try_into().ok()?) as usize;
    if swing == 0 {
        return None;
    }
    let off = swing.checked_add(ARM_COST_IN_SWING)?;
    (off < decoded.len()).then_some(off)
}

/// The cost a weapon of `fam` should carry for a character whose new favored
/// family is `favored`.
pub fn cost_for(fam: Family, favored: Family) -> u8 {
    if fam == favored {
        FAVORED_COST
    } else {
        OFFCLASS_COST
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_cover_the_weapon_id_space() {
        // every weapon id in the three class ranges maps to exactly one family
        assert_eq!(weapon_family(0x1B), Some(Family::Blade)); // Ra-Seru Blade
        assert_eq!(weapon_family(0x24), Some(Family::Blade)); // Short Sword
        assert_eq!(weapon_family(0x28), Some(Family::Claw)); // Nail Glove
        assert_eq!(weapon_family(0x1F), Some(Family::Claw)); // Ra-Seru Fangs
        assert_eq!(weapon_family(0x21), Some(Family::Club)); // Ra-Seru Club
        assert_eq!(weapon_family(0x33), Some(Family::Club)); // Great Axe
        assert_eq!(weapon_family(0xBA), None); // Astral Sword - never reassigned
        assert_eq!(weapon_family(0x38), None); // a head seal
    }

    #[test]
    fn plan_is_a_permutation_and_deterministic() {
        let a = plan_favored(0x1234);
        let b = plan_favored(0x1234);
        assert_eq!(a, b, "same seed -> same plan");
        // every family appears exactly once
        for fam in [Family::Blade, Family::Claw, Family::Club] {
            assert_eq!(a.iter().filter(|&&f| f == fam).count(), 1);
        }
    }

    #[test]
    fn cost_rule() {
        assert_eq!(cost_for(Family::Claw, Family::Claw), FAVORED_COST);
        assert_eq!(cost_for(Family::Blade, Family::Claw), OFFCLASS_COST);
    }
}
