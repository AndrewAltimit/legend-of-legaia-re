//! Cast audio-cue dispatcher - the per-cast sound-cue resolver the battle
//! action SM invokes when a cast starts (`jal 0x801E3E04` in
//! `FUN_801E295C`).
//!
//! PORT: FUN_801F3990
//!
//! Battle overlay (PROT 0898, file `+0x25178`); the port is decoded from a
//! resident battle-overlay capture (`overlay_muscle_dome_801f3990.txt` -
//! the muscle-dome capture is the battle-action overlay in residence, see
//! `docs/subsystems/minigame-muscle-dome.md`). The 0897-labelled dump at
//! this VA prints the over-read `FUN_801F3894` body and is NOT this
//! function (`docs/reference/functions.md` row `801F3990`).
//!
//! Retail inputs: acting slot `ctx[+0x13]`, its char-kind byte
//! `DAT_8007BD10[slot]`, the actor's cast class `actor[+0x1E8]` (seeded at
//! action state `0x3C` from the spell table's class byte), the queue head
//! `actor[+0x1DF]` and the sub-class byte `actor[+0x1E9]`. Output: one
//! `FUN_8004FCC8` SFX id, the `0xFE` item-give special, or nothing.
//!
//! # NOT WIRED
//!
//! The engine's live cast path schedules its cues through
//! `ArtStrikeInfo` / `BattleSfxCue` (see
//! `docs/subsystems/battle-action.md`); nothing resolves through this
//! byte-faithful mapper yet. Ported because the class -> cue-id algebra
//! (including the per-character `char_kind * 0x10 + base` player band and
//! the enemy-side `0x20C..0x20E` band) is the reference a cue-parity
//! oracle needs.

/// Outcome of the cast audio-cue dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastCueOutcome {
    /// No cue for this cast class.
    None,
    /// Play SFX `id` through the retail one-shot player (`FUN_8004FCC8`).
    Sfx(u16),
    /// The `actor[+0x1DF] == 0xFE` special: give item `0xFE` x1
    /// (`FUN_800421D4(0xFE, 1)`), play voice cue `voice_arg`
    /// (`FUN_8003D53C(char_kind + 0x19, 0, 0x5A)`) and stamp
    /// `_DAT_8007BD08` from the frame-speed byte.
    ItemGive {
        /// `char_kind + 0x19`, the first `FUN_8003D53C` argument.
        voice_arg: u8,
    },
}

/// PORT: FUN_801F3990 - resolve the cast-start audio cue.
///
/// Laws (capture-resident disassembly, jump tables reconstructed by the
/// decompiler from the resident image):
/// - enemy-side leg (`char_kind == 4` **or** `slot >= 3`): cast class
///   `0..=2` -> SFX `0x20C`, `3 | 8` -> `0x20D`, `4` -> `0x20E`, all
///   other classes silent;
/// - player leg, queue head `0xFE`: the item-give special (no class cue);
/// - player leg otherwise: class `0 | 1` -> `char_kind*0x10 + 0xF8`,
///   `2` -> `+0xF9`, `3 | 8` -> `+0xFA`, `4` -> `+0xFB`, `5` -> `+0xFC`,
///   `7` -> `+0xFC` only when the sub-class byte `actor[+0x1E9]` is in
///   `1..=4`, classes `6` and `> 8` silent.
pub fn cast_audio_cue(
    slot: u8,
    char_kind: u8,
    cast_class: u8,
    queue_head: u8,
    sub_class: u8,
) -> CastCueOutcome {
    if char_kind == 4 || slot >= 3 {
        return match cast_class {
            0..=2 => CastCueOutcome::Sfx(0x20C),
            3 | 8 => CastCueOutcome::Sfx(0x20D),
            4 => CastCueOutcome::Sfx(0x20E),
            _ => CastCueOutcome::None,
        };
    }
    if queue_head == 0xFE {
        return CastCueOutcome::ItemGive {
            voice_arg: char_kind.wrapping_add(0x19),
        };
    }
    let base = u16::from(char_kind) * 0x10;
    match cast_class {
        0 | 1 => CastCueOutcome::Sfx(base + 0xF8),
        2 => CastCueOutcome::Sfx(base + 0xF9),
        3 | 8 => CastCueOutcome::Sfx(base + 0xFA),
        4 => CastCueOutcome::Sfx(base + 0xFB),
        5 => CastCueOutcome::Sfx(base + 0xFC),
        7 if (1..=4).contains(&sub_class) => CastCueOutcome::Sfx(base + 0xFC),
        _ => CastCueOutcome::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemy_leg_band() {
        for class in 0..=2 {
            assert_eq!(
                cast_audio_cue(3, 0, class, 0, 0),
                CastCueOutcome::Sfx(0x20C)
            );
        }
        assert_eq!(cast_audio_cue(3, 0, 3, 0, 0), CastCueOutcome::Sfx(0x20D));
        assert_eq!(cast_audio_cue(3, 0, 8, 0, 0), CastCueOutcome::Sfx(0x20D));
        assert_eq!(cast_audio_cue(3, 0, 4, 0, 0), CastCueOutcome::Sfx(0x20E));
        assert_eq!(cast_audio_cue(3, 0, 5, 0, 0), CastCueOutcome::None);
        // char-kind 4 routes to the enemy leg even on a player slot.
        assert_eq!(cast_audio_cue(0, 4, 0, 0, 0), CastCueOutcome::Sfx(0x20C));
    }

    #[test]
    fn player_leg_per_character_band() {
        // char_kind 2 (Noa in the 0x8007BD10 kind space): base 0x20.
        assert_eq!(cast_audio_cue(0, 2, 0, 0, 0), CastCueOutcome::Sfx(0x118));
        assert_eq!(cast_audio_cue(0, 2, 1, 0, 0), CastCueOutcome::Sfx(0x118));
        assert_eq!(cast_audio_cue(0, 2, 2, 0, 0), CastCueOutcome::Sfx(0x119));
        assert_eq!(cast_audio_cue(0, 2, 3, 0, 0), CastCueOutcome::Sfx(0x11A));
        assert_eq!(cast_audio_cue(0, 2, 8, 0, 0), CastCueOutcome::Sfx(0x11A));
        assert_eq!(cast_audio_cue(0, 2, 4, 0, 0), CastCueOutcome::Sfx(0x11B));
        assert_eq!(cast_audio_cue(0, 2, 5, 0, 0), CastCueOutcome::Sfx(0x11C));
        assert_eq!(cast_audio_cue(0, 2, 6, 0, 0), CastCueOutcome::None);
    }

    #[test]
    fn class_7_gates_on_sub_class() {
        for sub in 1..=4u8 {
            assert_eq!(cast_audio_cue(0, 1, 7, 0, sub), CastCueOutcome::Sfx(0x10C));
        }
        assert_eq!(cast_audio_cue(0, 1, 7, 0, 0), CastCueOutcome::None);
        assert_eq!(cast_audio_cue(0, 1, 7, 0, 5), CastCueOutcome::None);
    }

    #[test]
    fn item_give_special_precedes_class_dispatch() {
        assert_eq!(
            cast_audio_cue(0, 2, 0, 0xFE, 0),
            CastCueOutcome::ItemGive { voice_arg: 0x1B }
        );
        // But not on the enemy leg.
        assert_eq!(cast_audio_cue(3, 0, 0, 0xFE, 0), CastCueOutcome::Sfx(0x20C));
    }
}
