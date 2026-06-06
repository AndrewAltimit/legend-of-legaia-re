//! Super Arts - find/replace pattern matching on the action queue.
//!
//! Once the player has finished entering commands, the runtime walks the
//! action queue and looks for known *Find* patterns. When a Find pattern
//! matches the **tail** of the queue and all participating arts have non-NEW
//! status with their AP cost paid, the pattern is replaced by a *Replace*
//! pattern that ends with the Super Art's finisher action constant.
//!
//! Notes per the researcher:
//! - The last art of the Find string must be the last action in the queue.
//! - All arts in the Find string must be non-NEW arts (AP cost is paid by
//!   them; the Super Art itself does not consume AP).
//! - The Replace string typically appends one or more copies of the
//!   finisher action constant for multi-hit Super Arts (e.g. Tri-Somersault
//!   appends `0x2B` three times).
//!
//! Source: ZetaPhoenix's public "Legaia Arts Data" Google Sheets
//! spreadsheet (Super Arts tab). See the README's Acknowledgments
//! section for the link.

use crate::queue::{ActionConstant, ActionQueue, Character};

/// Static Super Art trigger entry.
#[derive(Debug, Clone, Copy)]
pub struct SuperArt {
    pub character: Character,
    pub name: &'static str,
    pub finisher: u8,
    pub find: &'static [u8],
    pub replace: &'static [u8],
}

impl SuperArt {
    /// The sequence of **named-art** action constants in this Super's [`find`]
    /// pattern, with the `0x19` starters and the interleaved connector
    /// directions stripped out — e.g. Tri-Somersault's
    /// `find = [19 27 0F 19 1F 0E 19 27]` yields `[0x27, 0x1F, 0x27]`.
    ///
    /// The connector direction after each art (the `0F` / `0E` here) is
    /// **combo-specific** — the same art appears with different connectors
    /// across Supers (Vahn's `0x27` is followed by `0F` in Tri-Somersault but
    /// `0E` in Power Slash) — so it cannot be reconstructed from each art's own
    /// command string. The exact runtime queue-builder that emits those
    /// connectors (`ctx[+0x274]`) is unpinned. This art-only projection is the
    /// part of the pattern that *is* pinned, and is what the live Arts submenu
    /// matches a recognized art chain against (see
    /// [`SuperMatcher::trigger_by_art_sequence`]).
    ///
    /// [`find`]: SuperArt::find
    pub fn art_sequence(&self) -> Vec<u8> {
        self.find
            .iter()
            .copied()
            .filter(|&b| ActionConstant::from_byte(b).is_some_and(|a| a.is_art()))
            .collect()
    }
}

/// Test whether `tail` ends with `find`.
fn tail_matches(actions: &[ActionConstant], find: &[u8]) -> bool {
    if actions.len() < find.len() {
        return false;
    }
    let start = actions.len() - find.len();
    actions[start..]
        .iter()
        .zip(find.iter())
        .all(|(a, b)| a.as_byte() == *b)
}

/// Result of a Super Art match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuperHit {
    pub character: Character,
    pub finisher: u8,
    pub matched_len: usize,
    pub appended_len: usize,
}

pub struct SuperMatcher {
    table: &'static [SuperArt],
}

impl SuperMatcher {
    pub fn with_default_table() -> Self {
        Self { table: SUPER_ARTS }
    }

    pub fn with_table(table: &'static [SuperArt]) -> Self {
        Self { table }
    }

    /// Search for a Super Art whose `find` pattern matches the tail of the
    /// queue. On match, drops the matched bytes and appends the `replace`
    /// bytes; returns the [`SuperHit`].
    ///
    /// We pick the **longest** matching Find, so disjoint partial matches
    /// don't shadow longer ones. The retail runtime uses table-order
    /// scanning, but Find strings within a character don't overlap (each
    /// chains a different combination of arts), so length-based ranking is
    /// equivalent in practice and is more defensible against future entries.
    pub fn try_trigger_at_tail(
        &self,
        character: Character,
        queue: &mut ActionQueue,
    ) -> Option<SuperHit> {
        let actions = queue.actions();
        let mut best: Option<SuperArt> = None;
        for entry in self.table {
            if entry.character != character {
                continue;
            }
            if tail_matches(actions, entry.find)
                && best.is_none_or(|b| b.find.len() < entry.find.len())
            {
                best = Some(*entry);
            }
        }
        let entry = best?;
        let matched_len = entry.find.len();
        let new_len = actions.len() - matched_len;
        queue.truncate(new_len);
        // Append the replace bytes.
        for &b in entry.replace {
            // Safe to unwrap - Super Art replacement bytes are always
            // valid action constants.
            queue.push(ActionConstant::from_byte(b).expect("super art replace byte"));
        }
        Some(SuperHit {
            character,
            finisher: entry.finisher,
            matched_len,
            appended_len: entry.replace.len(),
        })
    }

    /// Find the Super Art whose **art-only** sequence ([`SuperArt::art_sequence`])
    /// matches the tail of a *recognized* art chain — i.e. the player chained
    /// these named arts in this order, ending on the Super's last art.
    ///
    /// This is the connector-abstracted sibling of [`Self::try_trigger_at_tail`]:
    /// where that one matches the byte-exact queue (starters + connector
    /// directions + arts), this matches only the pinned named-art ordering, so
    /// the live Arts submenu can fire a Super from a recognized chain without
    /// the unpinned `ctx[+0x274]` connector bytes (see [`SuperArt::art_sequence`]).
    /// Longest match wins, matching [`Self::try_trigger_at_tail`]'s ranking.
    pub fn trigger_by_art_sequence(
        &self,
        character: Character,
        recognized: &[ActionConstant],
    ) -> Option<&'static SuperArt> {
        let arts: Vec<u8> = recognized.iter().map(|a| a.as_byte()).collect();
        let mut best: Option<(&'static SuperArt, usize)> = None;
        for entry in self.table {
            if entry.character != character {
                continue;
            }
            let seq = entry.art_sequence();
            if seq.len() < 2 || !arts.ends_with(&seq) {
                continue;
            }
            if best.is_none_or(|(_, len)| len < seq.len()) {
                best = Some((entry, seq.len()));
            }
        }
        best.map(|(entry, _)| entry)
    }

    /// Run repeated Super Art expansions until a fixpoint is reached, in
    /// case a Super Art's replacement creates a new tail-match condition.
    pub fn expand_to_fixpoint(&self, character: Character, queue: &mut ActionQueue) -> usize {
        let mut hits = 0usize;
        // Cap iterations defensively; Super Art tables never have more
        // than a handful per character.
        for _ in 0..16 {
            if self.try_trigger_at_tail(character, queue).is_none() {
                break;
            }
            hits += 1;
        }
        hits
    }
}

// ---------------------------------------------------------------------------
// Per-character Super Art tables (matches the researcher's spreadsheet).
// ---------------------------------------------------------------------------

/// Combined Super Art table. 5 entries per character × 3 characters = 15.
pub const SUPER_ARTS: &[SuperArt] = &[
    // ---- Vahn ----
    SuperArt {
        character: Character::Vahn,
        name: "Tri-Somersault",
        finisher: 0x2B,
        find: &[0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27],
        replace: &[0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x1A, 0x2B, 0x2B, 0x2B],
    },
    SuperArt {
        character: Character::Vahn,
        name: "Maximum Blow",
        finisher: 0x2C,
        find: &[0x19, 0x28, 0x0E, 0x19, 0x26, 0x0C, 0x19, 0x25],
        replace: &[0x19, 0x28, 0x0E, 0x19, 0x26, 0x0C, 0x1A, 0x2C],
    },
    SuperArt {
        character: Character::Vahn,
        name: "Fire Tackle",
        finisher: 0x2D,
        find: &[0x19, 0x29, 0x0C, 0x19, 0x25, 0x0D, 0x19, 0x28],
        replace: &[0x19, 0x29, 0x0C, 0x19, 0x25, 0x0D, 0x1A, 0x2D],
    },
    SuperArt {
        character: Character::Vahn,
        name: "Power Slash",
        finisher: 0x2E,
        find: &[0x19, 0x28, 0x0E, 0x19, 0x27, 0x0E, 0x19, 0x26],
        replace: &[0x19, 0x28, 0x0E, 0x19, 0x27, 0x0E, 0x1A, 0x2E],
    },
    SuperArt {
        character: Character::Vahn,
        name: "Rolling Combo",
        finisher: 0x2F,
        find: &[0x19, 0x22, 0x0C, 0x19, 0x25, 0x0F, 0x0F, 0x19, 0x21],
        replace: &[0x19, 0x22, 0x0C, 0x19, 0x25, 0x0F, 0x0F, 0x1A, 0x2F, 0x30],
    },
    // ---- Noa ----
    SuperArt {
        character: Character::Noa,
        name: "Triple Lizard",
        finisher: 0x2E,
        find: &[0x19, 0x25, 0x0F, 0x19, 0x24, 0x0E, 0x19, 0x2B],
        replace: &[0x19, 0x25, 0x0F, 0x19, 0x24, 0x0E, 0x1A, 0x2E, 0x2E, 0x2E],
    },
    SuperArt {
        character: Character::Noa,
        name: "Super Javelin",
        finisher: 0x2F,
        find: &[0x19, 0x22, 0x0E, 0x19, 0x29],
        replace: &[0x19, 0x22, 0x0E, 0x1A, 0x2F],
    },
    SuperArt {
        character: Character::Noa,
        name: "Super Tempest",
        finisher: 0x30,
        find: &[0x19, 0x26, 0x0D, 0x0C, 0x0F, 0x0F, 0x19, 0x21],
        replace: &[0x19, 0x26, 0x0D, 0x0C, 0x0F, 0x0F, 0x1A, 0x30],
    },
    SuperArt {
        character: Character::Noa,
        name: "Love You",
        finisher: 0x31,
        find: &[0x19, 0x27, 0x0E, 0x19, 0x2B, 0x0E, 0x0C, 0x19, 0x23],
        replace: &[0x19, 0x27, 0x0E, 0x19, 0x2B, 0x0E, 0x0C, 0x1A, 0x31],
    },
    SuperArt {
        character: Character::Noa,
        name: "Dragon Fangs",
        finisher: 0x32,
        find: &[0x19, 0x2B, 0x0F, 0x19, 0x24, 0x0E, 0x19, 0x2A],
        replace: &[0x19, 0x2B, 0x0F, 0x19, 0x24, 0x0E, 0x1A, 0x32],
    },
    // ---- Gala ----
    SuperArt {
        character: Character::Gala,
        name: "Back Punch x3",
        finisher: 0x2B,
        find: &[0x19, 0x27, 0x0F, 0x19, 0x29, 0x0D, 0x19, 0x26],
        replace: &[0x19, 0x27, 0x0F, 0x19, 0x29, 0x0D, 0x1A, 0x2B, 0x2B, 0x2B],
    },
    SuperArt {
        character: Character::Gala,
        name: "Super Ironhead",
        finisher: 0x2C,
        find: &[0x19, 0x29, 0x0F, 0x19, 0x24, 0x0E, 0x19, 0x27],
        replace: &[0x19, 0x29, 0x0F, 0x19, 0x24, 0x0E, 0x1A, 0x2C],
    },
    SuperArt {
        character: Character::Gala,
        name: "Rushing Crush",
        finisher: 0x2D,
        find: &[0x19, 0x28, 0x0F, 0x19, 0x29, 0x0F, 0x19, 0x24],
        replace: &[0x19, 0x28, 0x0F, 0x19, 0x29, 0x0F, 0x1A, 0x2D],
    },
    SuperArt {
        character: Character::Gala,
        name: "Heaven's Drop",
        finisher: 0x2E,
        find: &[0x19, 0x29, 0x0F, 0x19, 0x24, 0x0C, 0x0E, 0x19, 0x22],
        replace: &[0x19, 0x29, 0x0F, 0x19, 0x24, 0x0C, 0x0E, 0x1A, 0x2E],
    },
    SuperArt {
        character: Character::Gala,
        name: "Neo Static Raising",
        finisher: 0x2F,
        find: &[0x19, 0x26, 0x0F, 0x19, 0x25, 0x0C, 0x0D, 0x0F, 0x19, 0x21],
        replace: &[0x19, 0x26, 0x0F, 0x19, 0x25, 0x0C, 0x0D, 0x0F, 0x1A, 0x2F],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_from(bytes: &[u8]) -> ActionQueue {
        let mut q = ActionQueue::new();
        for &b in bytes {
            q.push(ActionConstant::from_byte(b).unwrap());
        }
        q
    }

    fn ac(b: u8) -> ActionConstant {
        ActionConstant::from_byte(b).unwrap()
    }

    #[test]
    fn art_sequence_strips_starters_and_connectors() {
        // Tri-Somersault find = [19 27 0F 19 1F 0E 19 27] -> arts [27 1F 27].
        let tri = SUPER_ARTS
            .iter()
            .find(|s| s.name == "Tri-Somersault")
            .unwrap();
        assert_eq!(tri.art_sequence(), vec![0x27, 0x1F, 0x27]);
    }

    #[test]
    fn trigger_by_art_sequence_matches_the_pinned_ordering() {
        let m = SuperMatcher::with_default_table();
        // Exact art sequence triggers Tri-Somersault, connectors abstracted.
        let recognized = [ac(0x27), ac(0x1F), ac(0x27)];
        let hit = m
            .trigger_by_art_sequence(Character::Vahn, &recognized)
            .expect("art sequence triggers a Super");
        assert_eq!(hit.name, "Tri-Somersault");
        assert_eq!(hit.finisher, 0x2B);

        // A leading art is allowed (tail match): [21 27 1F 27] still triggers.
        assert!(
            m.trigger_by_art_sequence(Character::Vahn, &[ac(0x21), ac(0x27), ac(0x1F), ac(0x27)])
                .is_some()
        );

        // A single art never triggers a Super.
        assert!(
            m.trigger_by_art_sequence(Character::Vahn, &[ac(0x27)])
                .is_none()
        );

        // The same art sequence on a different character does not match Vahn's.
        assert!(
            m.trigger_by_art_sequence(Character::Gala, &recognized)
                .is_none(),
        );
    }

    #[test]
    fn vahn_tri_somersault_triggers() {
        let mut q = queue_from(&[0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27]);
        let matcher = SuperMatcher::with_default_table();
        let hit = matcher
            .try_trigger_at_tail(Character::Vahn, &mut q)
            .unwrap();
        assert_eq!(hit.finisher, 0x2B);
        let bytes: Vec<u8> = q.actions().iter().map(|a| a.as_byte()).collect();
        assert_eq!(
            bytes,
            vec![0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x1A, 0x2B, 0x2B, 0x2B]
        );
    }

    #[test]
    fn noa_super_javelin_short_pattern() {
        let mut q = queue_from(&[0x19, 0x22, 0x0E, 0x19, 0x29]);
        let matcher = SuperMatcher::with_default_table();
        let hit = matcher.try_trigger_at_tail(Character::Noa, &mut q).unwrap();
        assert_eq!(hit.finisher, 0x2F);
        let bytes: Vec<u8> = q.actions().iter().map(|a| a.as_byte()).collect();
        assert_eq!(bytes, vec![0x19, 0x22, 0x0E, 0x1A, 0x2F]);
    }

    #[test]
    fn gala_neo_static_raising_longest_pattern() {
        // Longest Gala find (10 bytes).
        let mut q = queue_from(&[0x19, 0x26, 0x0F, 0x19, 0x25, 0x0C, 0x0D, 0x0F, 0x19, 0x21]);
        let matcher = SuperMatcher::with_default_table();
        let hit = matcher
            .try_trigger_at_tail(Character::Gala, &mut q)
            .unwrap();
        assert_eq!(hit.finisher, 0x2F);
        assert_eq!(hit.matched_len, 10);
    }

    #[test]
    fn no_match_when_pattern_not_at_tail() {
        // Pattern present but followed by an unrelated action - must NOT trigger.
        let mut q = queue_from(&[
            0x19, 0x22, 0x0E, 0x19, 0x29, // Super Javelin pattern
            0x03, // … but Attack appended after, so not at tail
        ]);
        let matcher = SuperMatcher::with_default_table();
        let hit = matcher.try_trigger_at_tail(Character::Noa, &mut q);
        assert!(hit.is_none());
    }

    #[test]
    fn no_match_when_character_mismatched() {
        // Vahn's Tri-Somersault pattern, but matched against Noa.
        let mut q = queue_from(&[0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27]);
        let matcher = SuperMatcher::with_default_table();
        assert!(
            matcher
                .try_trigger_at_tail(Character::Noa, &mut q)
                .is_none()
        );
    }

    #[test]
    fn empty_queue_is_safe() {
        let mut q = ActionQueue::new();
        let matcher = SuperMatcher::with_default_table();
        assert!(
            matcher
                .try_trigger_at_tail(Character::Vahn, &mut q)
                .is_none()
        );
    }

    #[test]
    fn fixpoint_runs_at_least_once_then_stops() {
        let mut q = queue_from(&[0x19, 0x22, 0x0E, 0x19, 0x29]);
        let matcher = SuperMatcher::with_default_table();
        let hits = matcher.expand_to_fixpoint(Character::Noa, &mut q);
        assert_eq!(hits, 1);
        // Running again on the post-trigger queue should be a no-op.
        assert_eq!(matcher.expand_to_fixpoint(Character::Noa, &mut q), 0);
    }

    #[test]
    fn super_table_size_15() {
        // 5 each for Vahn / Noa / Gala = 15.
        assert_eq!(SUPER_ARTS.len(), 15);
        let counts = (0..3)
            .map(|i| {
                let c = match i {
                    0 => Character::Vahn,
                    1 => Character::Noa,
                    _ => Character::Gala,
                };
                SUPER_ARTS.iter().filter(|s| s.character == c).count()
            })
            .collect::<Vec<_>>();
        assert_eq!(counts, vec![5, 5, 5]);
    }
}
