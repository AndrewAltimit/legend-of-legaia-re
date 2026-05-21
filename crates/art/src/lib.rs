//! `legaia-art` - Tactical Arts data system.
//!
//! ## What this crate models
//!
//! The "Arts" system is Legaia's command-driven attack mechanic: the player
//! enters a sequence of directional inputs during their turn and the runtime
//! resolves it against a per-character table of *arts*, then expands certain
//! sequences into *Super Arts* (find/replace pattern matching) or *Miracle
//! Arts* (full action-queue replacement).
//!
//! Three layers compose at runtime:
//!
//! 1. **Action queue** - a flat sequence of [`ActionConstant`] values built
//!    from player input + queued character actions for the current turn.
//! 2. **Art records** - per-character binary records describing damage,
//!    animation, hit timing, and effects for each art. See [`ArtRecord`].
//! 3. **Trigger tables** - [`MiracleArt`] (command-string → replacement) and
//!    [`SuperArt`] (find/replace on the action queue) lookups that mutate the
//!    action queue before damage resolution.
//!
//! ## Source of truth
//!
//! The data tables in this crate (action constants 0x00–0x32, per-character
//! art names, Miracle/Super Art trigger sequences) come from external
//! reverse-engineering work that captured the relevant tables from RAM
//! addresses `0x80160EFC` (Vahn), `0x80176998` (Noa), `0x8018BA54` (Gala)
//! and PROT entry `0x05C4`. See [`docs/formats/art-data.md`](../../docs/formats/art-data.md)
//! for the binary record layout, multiplier encoding, and full table
//! citations.
//!
//! The constants in [`tables`] are *retail* - no in-engine guesses. If a
//! field has not been pinned down for a given character, that slot is `None`
//! rather than fabricated.

#![allow(clippy::too_many_arguments)]

pub mod arts_table;
pub mod miracle;
pub mod parse;
pub mod power;
pub mod queue;
pub mod record;
pub mod super_art;
pub mod tables;

pub use miracle::{MIRACLE_ARTS, MiracleArt, MiracleMatcher};
pub use parse::{ParseError, ParsedArtRecord, parse_record};
pub use power::{ArtPower, PowerByte, PowerTarget};
pub use queue::{ActionConstant, ActionQueue, Character, Command};
pub use record::{ArtRecord, EffectCue, EnemyEffect, HitCue, RepeatFrames};
pub use super_art::{SUPER_ARTS, SuperArt, SuperMatcher};
pub use tables::{
    art_anim_max_slot, art_anim_name, art_name, is_art, learned_art_action, learned_art_count,
    learned_art_max_slot,
};

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn vahns_craze_constant_is_1b() {
        let action = ActionConstant::from_byte(0x1B).unwrap();
        assert_eq!(action.as_byte(), 0x1B);
        // Vahn slot 0 in Learned Art Constant table is 0x1B (Vahn's Craze).
        assert_eq!(learned_art_action(Character::Vahn, 0), Some(action));
        assert_eq!(art_name(Character::Vahn, action), Some("Vahn's Craze"));
        assert_eq!(art_name(Character::Noa, action), Some("Noa's Ark"));
        assert_eq!(art_name(Character::Gala, action), Some("Biron Rage"));
    }

    #[test]
    fn miracle_art_replaces_action_queue() {
        // Vahn's Craze command is RDLULURDL (per the researcher's spreadsheet).
        let cmds = [
            Command::Right,
            Command::Down,
            Command::Left,
            Command::Up,
            Command::Left,
            Command::Up,
            Command::Right,
            Command::Down,
            Command::Left,
        ];
        let matcher = MiracleMatcher::with_default_table();
        let mut q = ActionQueue::new();
        let triggered = matcher.try_trigger(Character::Vahn, &cmds, &mut q);
        assert!(triggered, "Vahn's Craze command should trigger Miracle Art");
        // After trigger the action queue is non-empty and ends with the
        // Miracle Art finisher (Tornado Flame Miracle, action 0x2A).
        assert!(!q.is_empty());
        let last = q.actions().last().copied().unwrap();
        assert_eq!(last, ActionConstant::from_byte(0x2A).unwrap());
    }

    #[test]
    fn super_art_finds_pattern_in_queue() {
        // Vahn 0x2B Tri-Somersault: Find = [0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27]
        // Build a queue ending with that pattern; the matcher should
        // append the Replace tail.
        let mut q = ActionQueue::new();
        for b in [0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27] {
            q.push(ActionConstant::from_byte(b).unwrap());
        }
        let matcher = SuperMatcher::with_default_table();
        let trig = matcher.try_trigger_at_tail(Character::Vahn, &mut q);
        assert!(trig.is_some(), "Tri-Somersault should trigger");
        // After trigger queue ends with 0x2B 0x2B 0x2B (Tri-Somersault hits 3x).
        let acts = q.actions();
        assert_eq!(acts[acts.len() - 3].as_byte(), 0x2B);
        assert_eq!(acts[acts.len() - 2].as_byte(), 0x2B);
        assert_eq!(acts[acts.len() - 1].as_byte(), 0x2B);
    }
}
