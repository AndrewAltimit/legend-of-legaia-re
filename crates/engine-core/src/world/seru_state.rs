//! Seru capture + magic-learning state: the capture log and registry, this battle's captures, shiny rolls and magic level-ups.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Seru capture + magic-learning state: the capture log and registry, this battle's captures, shiny rolls and magic level-ups.
pub struct SeruState {
    /// Per-character Seru capture log - drives the post-battle "spell
    /// learned!" banner and the in-menu spell list. Pure data; saved
    /// through [`legaia_save::SaveExtV2::per_char`].
    pub log: crate::seru_learning::SeruCaptureLog,
    /// Master Seru registry (Seru id -> spell taught + capture points).
    /// Engines install via [`World::set_seru_registry`]; `World::finish_battle`
    /// resolves [`crate::world::SeruState::battle_captures`] against it into [`crate::world::SeruState::log`].
    /// Empty by default - captures then bank no points (the monster is still
    /// downed + logged, but nothing is learned).
    pub registry: crate::seru_learning::SeruRegistry,
    /// Capture outcomes produced by the most recently finished battle, one per
    /// captured Seru that the registry accepted. Hosts drain this with
    /// [`World::drain_last_capture_outcomes`] to drive the "captured / learned"
    /// banner ([`crate::seru_learning::SeruCaptureSession`]).
    pub last_capture_outcomes: Vec<crate::seru_learning::CaptureOutcome>,
    /// Monster ids captured this battle by a capture spell (`SpellEffect::Capture`).
    /// The captured monster is downed immediately; the host drains this for
    /// post-battle Seru-learning resolution (the live loop carries no Seru
    /// registry, so the learn step itself lives outside the battle tick).
    pub battle_captures: Vec<u16>,
    /// Chance, in percent, that a capturable enemy spawns as a **shiny**
    /// variant in a given battle: a single rare enemy with +35% stats whose
    /// captured Seru deals +35% damage forever (see the `--shiny-seru`
    /// randomizer feature). `0` disables. Default
    /// [`World::DEFAULT_SHINY_CHANCE_PCT`].
    pub shiny_chance_pct: u8,
    /// Battle slots flagged shiny this battle (filled by
    /// [`World::roll_shiny_enemy`] at battle entry, drained at battle end).
    /// A shiny enemy's stats are pre-boosted; capturing it marks the learned
    /// spell shiny.
    pub shiny_enemy_slots: std::collections::HashSet<u8>,
    /// Monster ids captured **as shiny** this battle (subset of
    /// [`crate::world::SeruState::battle_captures`]; `resolve_captures` marks their spell shiny).
    pub shiny_captures: Vec<u16>,
    /// Summon-magic level-ups resolved this session: `(party_slot, spell_id,
    /// new_level)` per event, in resolution order. The engine analogue of the
    /// retail level-up banner (the level-up check fires UI element `0x65` -
    /// REF: FUN_801e70bc, ported in `world::battle::accrue_summon_spell_xp`);
    /// hosts drain via [`World::drain_magic_level_ups`].
    pub magic_level_ups: Vec<(u8, u8, u8)>,
}

impl SeruState {
    pub fn new() -> Self {
        Self {
            battle_captures: Vec::new(),
            shiny_chance_pct: World::DEFAULT_SHINY_CHANCE_PCT,
            shiny_enemy_slots: std::collections::HashSet::new(),
            shiny_captures: Vec::new(),
            magic_level_ups: Vec::new(),
            log: crate::seru_learning::SeruCaptureLog::new(),
            registry: crate::seru_learning::SeruRegistry::new(),
            last_capture_outcomes: Vec::new(),
        }
    }
}

impl Default for SeruState {
    fn default() -> Self {
        Self::new()
    }
}
