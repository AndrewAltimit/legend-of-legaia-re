//! Direction → named-art recognizer.
//!
//! A saved chain (and the retail combo input) is a flat string of directional
//! [`Command`]s. The runtime turns it into a queue of `0x19 <art> …` blocks by
//! recognizing, left to right, which named art each run of directions performs.
//! This module ports that recognition step: given a character's art catalog
//! (each art's directional [`Command`] string, from its
//! [`ArtRecord`](crate::record::ArtRecord) / [`arts_table`](crate::arts_table)
//! entry) plus an input direction string, it returns the ordered sequence of
//! named-art [`ActionConstant`]s the input performs.
//!
//! It is the prerequisite the Super-Art live wiring needs: a Super fires when a
//! recognized art *sequence* ends on a known combination (see
//! [`crate::super_art::SuperMatcher::trigger_by_art_sequence`]). The exact
//! byte-level queue the retail builder emits — including the interleaved
//! connector directions between arts — is unpinned (`ctx[+0x274]`), so this
//! recognizer **abstracts** those connectors: a direction that begins no art is
//! skipped (treated as a connector), rather than aborting recognition. That
//! keeps the recovered art ordering faithful even though the literal queue bytes
//! aren't reproduced.

use crate::queue::{ActionConstant, Command};

/// One catalog entry the recognizer matches against: a named art's action
/// constant and its directional command string.
pub type ArtCommands<'a> = (ActionConstant, &'a [Command]);

/// Recognize the ordered sequence of named arts an input direction string
/// performs.
///
/// Greedy **longest-match**, left to right: at each position the longest art
/// whose command string is a prefix of the remaining input is consumed and its
/// action constant emitted; a position that begins no art is skipped (an
/// abstracted connector direction — see the module docs). Arts with an empty
/// command string are ignored (they can never match and would loop).
///
/// Returns the arts in performed order. An input that performs no recognizable
/// art returns an empty vector.
pub fn recognize_art_sequence(arts: &[ArtCommands<'_>], input: &[Command]) -> Vec<ActionConstant> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        let mut best: Option<(ActionConstant, usize)> = None;
        for &(action, cmds) in arts {
            if cmds.is_empty() || !input[i..].starts_with(cmds) {
                continue;
            }
            if best.is_none_or(|(_, len)| len < cmds.len()) {
                best = Some((action, cmds.len()));
            }
        }
        match best {
            Some((action, len)) => {
                out.push(action);
                i += len;
            }
            // No art starts here: treat the direction as an abstracted
            // inter-art connector and step over it.
            None => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::Command::{Down, Left, Right, Up};

    fn ac(b: u8) -> ActionConstant {
        ActionConstant::from_byte(b).unwrap()
    }

    #[test]
    fn recognizes_a_back_to_back_chain() {
        // art 0x27 = [Up], art 0x1F = [Down]
        let somersault: &[Command] = &[Up];
        let cyclone: &[Command] = &[Down];
        let arts = [(ac(0x27), somersault), (ac(0x1F), cyclone)];
        // Up Down Up -> Somersault Cyclone Somersault
        let seq = recognize_art_sequence(&arts, &[Up, Down, Up]);
        assert_eq!(seq, vec![ac(0x27), ac(0x1F), ac(0x27)]);
    }

    #[test]
    fn longest_match_wins_over_a_prefix_art() {
        // A two-direction art must win over its single-direction prefix art so
        // a multi-direction art isn't shadowed.
        let short_art: &[Command] = &[Left];
        let long_art: &[Command] = &[Left, Right];
        let arts = [(ac(0x1B), short_art), (ac(0x1C), long_art)];
        assert_eq!(
            recognize_art_sequence(&arts, &[Left, Right]),
            vec![ac(0x1C)],
            "the 2-direction art should consume both inputs, not the 1-direction prefix"
        );
    }

    #[test]
    fn skips_connector_directions_between_arts() {
        // Arts are [Up] and [Down]; a stray Left between them is an abstracted
        // connector and must be skipped, not abort recognition.
        let a: &[Command] = &[Up];
        let b: &[Command] = &[Down];
        let arts = [(ac(0x27), a), (ac(0x1F), b)];
        assert_eq!(
            recognize_art_sequence(&arts, &[Up, Left, Down]),
            vec![ac(0x27), ac(0x1F)]
        );
    }

    #[test]
    fn empty_command_arts_are_ignored() {
        let empty: &[Command] = &[];
        let real: &[Command] = &[Up];
        let arts = [(ac(0x1B), empty), (ac(0x27), real)];
        assert_eq!(recognize_art_sequence(&arts, &[Up]), vec![ac(0x27)]);
    }

    #[test]
    fn no_recognizable_art_returns_empty() {
        let a: &[Command] = &[Up, Up];
        let arts = [(ac(0x27), a)];
        assert!(recognize_art_sequence(&arts, &[Left, Right, Down]).is_empty());
    }
}
