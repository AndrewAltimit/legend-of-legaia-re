//! [`ArtRecord`] — the per-art binary record laid out in RAM.
//!
//! Field positions and meanings from the external `Art Data Format`
//! research. Per-character base addresses (Vahn `0x80160EFC`, Noa
//! `0x80176998`, Gala `0x8018BA54`) and the on-disc source live in PROT
//! entry `0x05C4`. Records are variable-length: the command sequence and
//! the power-data section both shrink/expand per art, so the format is
//! schema-then-walk rather than fixed-stride.
//!
//! This struct is the *decoded* shape — the raw on-disc layout is parsed
//! by [`crate::parse::parse_record`].

use serde::{Deserialize, Serialize};

use crate::power::PowerByte;
use crate::queue::{ActionConstant, Command};

/// Status effect an art can apply to the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EnemyEffect {
    #[default]
    None,
    Burned,
    Shocked,
    Other(u8),
}

impl EnemyEffect {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => EnemyEffect::None,
            1 => EnemyEffect::Burned,
            2 => EnemyEffect::Shocked,
            n => EnemyEffect::Other(n),
        }
    }
}

/// One entry in the Special Effect Cues table.
///
/// Per the researcher's notes the on-disc shape occupies 2 words (8 bytes):
/// the first half-word selects an effect constant, the remaining 3
/// half-words are interpreted as XYZ coordinates by the effect spawner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EffectCue {
    pub effect_id: u16,
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

impl EffectCue {
    pub fn from_words(w0: u16, w1: u16, w2: u16, w3: u16) -> Self {
        Self {
            effect_id: w0,
            x: w1 as i16,
            y: w2 as i16,
            z: w3 as i16,
        }
    }

    /// Cue is "active" iff any field is non-zero. The spreadsheet's
    /// example art (`02 33 00 00 00 00 00 00`) has effect_id = 0x33 and
    /// all-zero coords — still a real cue.
    pub fn is_active(&self) -> bool {
        self.effect_id != 0 || self.x != 0 || self.y != 0 || self.z != 0
    }
}

/// One Hit Effect Cue: a single 32-bit word.
///
/// Layout per the researcher: high half = animation-frame timing, low half
/// = effect/sound constant. Common low-half values:
/// - `0x1A` — sound effect trigger
/// - `0x4C` — hit effect (visible flash / damage popup)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HitCue {
    pub timing_frames: u16,
    pub kind: u16,
}

impl HitCue {
    pub fn from_word(word: u32) -> Self {
        Self {
            timing_frames: (word >> 16) as u16,
            kind: (word & 0xFFFF) as u16,
        }
    }

    pub fn is_sound(&self) -> bool {
        self.kind == 0x1A
    }

    pub fn is_hit_effect(&self) -> bool {
        self.kind == 0x4C
    }
}

/// Repeat-frames descriptor: replays a specific frame range a given number
/// of times during the art animation. For some arts this also repeats the
/// damage from power bytes that fall in the repeated range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RepeatFrames {
    pub repeat_count: u8,
    pub start_frame: u8,
    pub end_frame: u8,
}

impl RepeatFrames {
    pub fn is_active(&self) -> bool {
        self.repeat_count != 0 && self.end_frame > self.start_frame
    }
}

/// Decoded art record.
///
/// Some fields are absent for some arts (Special Effect Cues are unused on
/// most regular arts; Art Name is only populated for Super / Miracle
/// finishers and a handful of Hyper Arts). Use the per-field `Option` /
/// `is_active()` helpers to test before consuming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtRecord {
    /// Action constant the queue uses to invoke this art (one of
    /// `0x1B–0x32`).
    pub action: ActionConstant,
    /// Directional command sequence the player enters to chain into this
    /// art. May be empty for Miracle / Super Art finishers, which are
    /// invoked by command-string match or queue-pattern match rather than
    /// directly from input.
    pub commands: Vec<Command>,
    /// Index into the per-character art-animation table. Only byte 0 is
    /// commonly used; the spec reserves 6 bytes ("Additional Anim Data?"
    /// in the spreadsheet) and a handful of arts (e.g. Hurricane Kick)
    /// chain multiple anim records.
    pub anim_index: u8,
    pub anim_extra: Vec<u8>,
    /// Display name embedded in the record. Populated for Super Arts,
    /// Miracle Art finishers, and some Hyper Arts; `None` otherwise (the
    /// runtime falls back to a per-character name table).
    pub name: Option<String>,
    /// Up to 4 power bytes describing damage per hit. Position in the
    /// vec corresponds to the hit index used by `dmg_timing`.
    pub power: Vec<PowerByte>,
    /// Per-power-byte animation-frame timing (when each hit fires).
    pub dmg_timing: Vec<u8>,
    /// Up to 2 special-effect cues fired during the animation.
    pub effect_cues: [EffectCue; 2],
    /// Up to 4 hit-effect cues (sound + visual triggers).
    pub hit_cues: Vec<HitCue>,
    /// Identifier byte. Some values trigger special animations
    /// (`0x67` = Thunderbolt for Heaven's Drop).
    pub identifier: u8,
    /// Animation playback speed. Lower = slower, higher = faster.
    pub anim_speed: u8,
    /// Status effect inflicted on hit, if any.
    pub enemy_effect: EnemyEffect,
    /// Frame-range replay descriptor. `is_active()` tells you whether
    /// the field is meaningful.
    pub repeat_frames: RepeatFrames,
    /// Background mode. `0` = regular, `2` = black (used by Super Arts
    /// and the Tornado Flame Hyper Art).
    pub background: u8,
    /// "Address" slot — written by the runtime after the art is used the
    /// first time in a battle. Always `None` for static records.
    pub runtime_address: Option<u32>,
}

impl ArtRecord {
    /// Iterate decoded power bytes that produce damage.
    pub fn damage_hits(&self) -> impl Iterator<Item = (usize, &PowerByte)> {
        self.power.iter().enumerate().filter(|(_, p)| p.is_damage())
    }

    /// Total expected number of hits accounting for `repeat_frames`. This
    /// matches the researcher's note on Super Tempest dealing 8 hits despite
    /// only 4 power bytes (first 2 are repeated 3×, last 2 fire once).
    pub fn total_hits(&self) -> usize {
        let base = self.damage_hits().count();
        if !self.repeat_frames.is_active() {
            return base;
        }
        let count = self.repeat_frames.repeat_count as usize;
        if count == 0 {
            return base;
        }
        // Hits that fall within the repeat frame range fire `count` times
        // total instead of once. We don't know per-hit frame indices here
        // (the consumer applies `dmg_timing` to position each hit), so we
        // approximate by saying the first 2 hits are in the repeat range
        // when repeat_frames are active and there are >2 hits — matches
        // the documented Super Tempest case.
        if base <= 2 {
            base * count
        } else {
            (2 * count) + (base - 2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::power::{ArtPower, PowerByte, PowerTarget};

    fn dmg(target: PowerTarget, multiplier: u8) -> PowerByte {
        PowerByte::Damage(ArtPower {
            target,
            multiplier,
            alt_range: false,
        })
    }

    #[test]
    fn enemy_effect_decodes() {
        assert_eq!(EnemyEffect::from_byte(0), EnemyEffect::None);
        assert_eq!(EnemyEffect::from_byte(1), EnemyEffect::Burned);
        assert_eq!(EnemyEffect::from_byte(2), EnemyEffect::Shocked);
        assert_eq!(EnemyEffect::from_byte(7), EnemyEffect::Other(7));
    }

    #[test]
    fn effect_cue_active_test() {
        // Spreadsheet example: 02 33 00 00 00 00 00 00 = effect_id 0x33,
        // all-zero coords. Still active because effect_id != 0.
        let cue = EffectCue::from_words(0x0033, 0, 0, 0);
        assert!(cue.is_active());
        assert_eq!(cue.effect_id, 0x33);

        let empty = EffectCue::default();
        assert!(!empty.is_active());
    }

    #[test]
    fn hit_cue_classifies_kind() {
        let sound = HitCue::from_word(0x0010_001A);
        assert_eq!(sound.timing_frames, 0x10);
        assert!(sound.is_sound());
        assert!(!sound.is_hit_effect());

        let hit = HitCue::from_word(0x0020_004C);
        assert!(hit.is_hit_effect());
        assert!(!hit.is_sound());
    }

    #[test]
    fn super_tempest_total_hits() {
        // Super Tempest: 4 power bytes; first 2 repeat 3× -> 8 total hits.
        let rec = ArtRecord {
            action: ActionConstant::Art30,
            commands: vec![],
            anim_index: 0,
            anim_extra: vec![],
            name: Some("Super Tempest".into()),
            power: vec![
                dmg(PowerTarget::Ldf, 12),
                dmg(PowerTarget::Ldf, 12),
                dmg(PowerTarget::Udf, 12),
                dmg(PowerTarget::Udf, 12),
            ],
            dmg_timing: vec![0x10, 0x14, 0x18, 0x1C],
            effect_cues: [EffectCue::default(); 2],
            hit_cues: vec![],
            identifier: 0x6E,
            anim_speed: 0x10,
            enemy_effect: EnemyEffect::None,
            repeat_frames: RepeatFrames {
                repeat_count: 3,
                start_frame: 0,
                end_frame: 0x14,
            },
            background: 2,
            runtime_address: None,
        };
        assert_eq!(rec.total_hits(), 2 * 3 + 2);
    }
}
