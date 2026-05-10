//! Miracle Arts - full action-queue replacement triggered by a command match.
//!
//! Every character has one Miracle Art:
//!
//! | Character | Art           | RAM (`801F` segment) | PROT (`05C4` area) |
//! |-----------|---------------|-----------------------|---------------------|
//! | Vahn      | Vahn's Craze  | `0x64F4`              | `0x0CDC`            |
//! | Noa       | Noa's Ark     | `0x6504`              | `0x0CEC`            |
//! | Gala      | Biron Rage    | `0x6514`              | `0x0CFC`            |
//!
//! ## Trigger semantics
//!
//! 1. The player enters a command sequence.
//! 2. The runtime compares the sequence against each character's Miracle
//!    Art [`MiracleArt::commands`] table.
//! 3. On match, the entire action queue is replaced with the Miracle Art's
//!    [`MiracleArt::replacement`] action constants.
//! 4. The first 4 bytes of the replacement are stored on disc with the MSB
//!    of the high nibble set (`0x8C` / `0x8D` / `0x8E` / `0x8F`); the
//!    runtime zeros that bit when copying into the queue. This crate
//!    follows the runtime convention - [`MiracleArt::replacement`] is the
//!    *post-mask* action constants ready to write into the queue.
//!
//! Source: external researcher's `Miracle Arts Data` spreadsheet.

use crate::queue::{ActionConstant, ActionQueue, Character, Command};

/// Single Miracle Art definition.
pub struct MiracleArt {
    pub character: Character,
    pub name: &'static str,
    /// Command sequence the player must enter to trigger this art.
    pub commands: &'static [Command],
    /// Replacement action constants written into the queue on trigger.
    /// Stored already MSB-cleared (matches what the runtime writes).
    pub replacement: &'static [ActionConstant],
}

/// Static table of all Miracle Arts.
pub const MIRACLE_ARTS: &[MiracleArt] = &[
    MiracleArt {
        character: Character::Vahn,
        name: "Vahn's Craze",
        commands: &[
            Command::Right,
            Command::Down,
            Command::Left,
            Command::Up,
            Command::Left,
            Command::Up,
            Command::Right,
            Command::Down,
            Command::Left,
        ],
        // Researcher tail: 8C 8D 8E 8F 1A 22 28 23 27 20 2A
        // Post-MSB-mask: 0C 0D 0E 0F 1A 22 28 23 27 20 2A
        // Decoded: Left, Right, Down, Up, SpecialStarter, Spin Combo,
        // Charging Scorch, Pyro Pummel, Somersault, Hurricane,
        // Tornado Flame (Miracle).
        replacement: &[
            ActionConstant::Left,
            ActionConstant::Right,
            ActionConstant::Down,
            ActionConstant::Up,
            ActionConstant::SpecialStarter,
            ActionConstant::Art22,
            ActionConstant::Art28,
            ActionConstant::Art23,
            ActionConstant::Art27,
            ActionConstant::Art20,
            ActionConstant::Art2A,
        ],
    },
    MiracleArt {
        character: Character::Noa,
        name: "Noa's Ark",
        commands: &[
            Command::Left,
            Command::Up,
            Command::Right,
            Command::Down,
            Command::Up,
            Command::Left,
            Command::Up,
            Command::Down,
            Command::Right,
        ],
        // Researcher tail: 8D 8C 8F 8E 1A 2A 26 27 2B 24 2C 2D
        // Post-mask: 0D 0C 0F 0E 1A 2A 26 27 2B 24 2C 2D
        // Order in the spreadsheet: Right, Left, Up, Down, Special Starter,
        // Acrobatic Blitz, Dolphin Attack, Mirage Lancer, Lizard Tail,
        // Swan Driver, Jurassic Blow 1, Jurassic Blow 2.
        replacement: &[
            ActionConstant::Right,
            ActionConstant::Left,
            ActionConstant::Up,
            ActionConstant::Down,
            ActionConstant::SpecialStarter,
            ActionConstant::Art2A,
            ActionConstant::Art26,
            ActionConstant::Art27,
            ActionConstant::Art2B,
            ActionConstant::Art24,
            ActionConstant::Art2C,
            ActionConstant::Art2D,
        ],
    },
    MiracleArt {
        character: Character::Gala,
        name: "Biron Rage",
        commands: &[
            Command::Right,
            Command::Right,
            Command::Down,
            Command::Up,
            Command::Down,
            Command::Up,
            Command::Down,
            Command::Left,
            Command::Left,
        ],
        // Researcher tail: 8C 8D 8E 8F 1A 26 23 27 20 28 2A
        // Post-mask: 0C 0D 0E 0F 1A 26 23 27 20 28 2A
        // Order: Left, Right, Down, Up, Special Starter, Back Punch,
        // Side Kick, Ironhead, Electro Thrash, Battering Ram,
        // Thunder Punch (Miracle).
        replacement: &[
            ActionConstant::Left,
            ActionConstant::Right,
            ActionConstant::Down,
            ActionConstant::Up,
            ActionConstant::SpecialStarter,
            ActionConstant::Art26,
            ActionConstant::Art23,
            ActionConstant::Art27,
            ActionConstant::Art20,
            ActionConstant::Art28,
            ActionConstant::Art2A,
        ],
    },
];

/// Strip the on-disc MSB-set quirk from a raw replacement byte. The
/// runtime ANDs with `0x7F`; this helper exists for tooling that ingests
/// raw RAM dumps before constructing a [`MiracleArt`].
pub fn unmask_replacement_byte(raw: u8) -> u8 {
    raw & 0x7F
}

/// Match-and-trigger driver for Miracle Arts.
///
/// Wraps an immutable table; in tests you can swap the table with
/// [`MiracleMatcher::with_table`] for fixture-driven assertions.
pub struct MiracleMatcher {
    table: &'static [MiracleArt],
}

impl MiracleMatcher {
    pub fn with_default_table() -> Self {
        Self {
            table: MIRACLE_ARTS,
        }
    }

    pub fn with_table(table: &'static [MiracleArt]) -> Self {
        Self { table }
    }

    /// Look up the Miracle Art for a character whose `commands` match the
    /// player input exactly.
    pub fn find(&self, character: Character, input: &[Command]) -> Option<&'static MiracleArt> {
        self.table
            .iter()
            .find(|art| art.character == character && art.commands == input)
    }

    /// Try to trigger a Miracle Art. On match the queue is *cleared* and
    /// replaced with the art's replacement string. Returns `true` on
    /// trigger.
    pub fn try_trigger(
        &self,
        character: Character,
        input: &[Command],
        queue: &mut ActionQueue,
    ) -> bool {
        let Some(art) = self.find(character, input) else {
            return false;
        };
        queue.replace_all(art.replacement.iter().copied());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miracle_table_one_per_character() {
        assert_eq!(MIRACLE_ARTS.len(), 3);
        assert!(
            MIRACLE_ARTS
                .iter()
                .any(|m| m.character == Character::Vahn && m.name == "Vahn's Craze")
        );
        assert!(
            MIRACLE_ARTS
                .iter()
                .any(|m| m.character == Character::Noa && m.name == "Noa's Ark")
        );
        assert!(
            MIRACLE_ARTS
                .iter()
                .any(|m| m.character == Character::Gala && m.name == "Biron Rage")
        );
    }

    #[test]
    fn unmask_replacement_byte_clears_msb() {
        assert_eq!(unmask_replacement_byte(0x8C), 0x0C);
        assert_eq!(unmask_replacement_byte(0x8F), 0x0F);
        // Already-masked bytes are untouched.
        assert_eq!(unmask_replacement_byte(0x1A), 0x1A);
        assert_eq!(unmask_replacement_byte(0x2A), 0x2A);
    }

    #[test]
    fn miracle_trigger_clears_existing_queue() {
        let mut q = ActionQueue::new();
        // Start with junk in the queue.
        q.push(ActionConstant::Attack);
        q.push(ActionConstant::Item);

        let m = MiracleMatcher::with_default_table();
        let triggered = m.try_trigger(
            Character::Gala,
            &[
                Command::Right,
                Command::Right,
                Command::Down,
                Command::Up,
                Command::Down,
                Command::Up,
                Command::Down,
                Command::Left,
                Command::Left,
            ],
            &mut q,
        );
        assert!(triggered);
        // Queue no longer contains the junk Attack/Item.
        assert_eq!(q.actions().first().copied(), Some(ActionConstant::Left));
        // Last action is the Miracle finisher (Thunder Punch Miracle = 0x2A).
        assert_eq!(q.actions().last().copied().unwrap().as_byte(), 0x2A);
    }

    #[test]
    fn miracle_no_match_leaves_queue_alone() {
        let mut q = ActionQueue::new();
        q.push(ActionConstant::Attack);
        let m = MiracleMatcher::with_default_table();
        let triggered = m.try_trigger(
            Character::Vahn,
            &[Command::Up, Command::Up, Command::Up],
            &mut q,
        );
        assert!(!triggered);
        assert_eq!(q.actions(), &[ActionConstant::Attack]);
    }

    #[test]
    fn miracle_replacement_starts_with_unmasked_directionals() {
        // All three replacements start with 4 directional bytes (0x0C/0x0D/0x0E/0x0F)
        // followed by SpecialStarter (0x1A).
        for art in MIRACLE_ARTS {
            assert!(art.replacement.len() >= 5);
            for (i, action) in art.replacement[..4].iter().enumerate() {
                assert!(
                    action.is_directional(),
                    "{}: replacement[{i}] = {action:?} should be directional",
                    art.name
                );
            }
            assert_eq!(art.replacement[4], ActionConstant::SpecialStarter);
        }
    }
}
