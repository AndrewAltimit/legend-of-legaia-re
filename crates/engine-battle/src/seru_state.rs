//! Seru capture + magic-learning state: the capture log and registry, this battle's captures, shiny rolls and magic level-ups.
//!
//! One subsystem's state, held as a unit on the composite `World`.
//! Fields keep their retail provenance notes.

/// Default chance (percent) that a capturable enemy spawns shiny.
/// Matches the `--shiny-seru` randomizer default and the rare-encounter
/// feel the feature is named for.
pub const DEFAULT_SHINY_CHANCE_PCT: u8 = 2;

/// Seru capture + magic-learning state: the capture log and registry, this battle's captures, shiny rolls and magic level-ups.
pub struct SeruState {
    /// Per-character Seru capture log - drives the post-battle "spell
    /// learned!" banner and the in-menu spell list. Pure data; saved
    /// through [`legaia_save::SaveExtV2::per_char`].
    pub log: crate::seru_learning::SeruCaptureLog,
    /// Master Seru registry (Seru id -> spell taught + capture points).
    /// Engines install via `World::set_seru_registry`; `World::finish_battle`
    /// resolves [`SeruState::battle_captures`] against it into [`SeruState::log`].
    /// Empty by default - captures then bank no points (the monster is still
    /// downed + logged, but nothing is learned).
    pub registry: crate::seru_learning::SeruRegistry,
    /// Capture outcomes produced by the most recently finished battle, one per
    /// captured Seru that the registry accepted. Replaced (not appended) by
    /// every `resolve_captures`, so it holds one battle's outcomes at most.
    /// The "captured / learned" line both hosts draw is
    /// `PartyState::current_capture_banner`, built from the
    /// same resolve; this list is the record behind it, read by
    /// `World::drain_last_capture_outcomes`.
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
    /// [`DEFAULT_SHINY_CHANCE_PCT`].
    pub shiny_chance_pct: u8,
    /// Battle slots flagged shiny this battle (filled by
    /// `World::roll_shiny_enemy` at battle entry, drained at battle end).
    /// A shiny enemy's stats are pre-boosted; capturing it marks the learned
    /// spell shiny.
    pub shiny_enemy_slots: std::collections::HashSet<u8>,
    /// Monster ids captured **as shiny** this battle (subset of
    /// [`SeruState::battle_captures`]; `resolve_captures` marks their spell shiny).
    pub shiny_captures: Vec<u16>,
    /// Summon-magic level-ups resolved this session: `(party_slot, spell_id,
    /// new_level)` per event, in resolution order. The engine analogue of the
    /// retail level-up banner (the level-up check fires UI element `0x65` -
    /// REF: FUN_801e70bc, ported in `world::battle::accrue_summon_spell_xp`).
    /// What the player sees is the battle message banner the same level-up
    /// raises; this list is the per-battle record behind it, cleared at
    /// `World::enter_battle` and read by
    /// `World::drain_magic_level_ups` (tests and oracles).
    pub magic_level_ups: Vec<(u8, u8, u8)>,
}

impl SeruState {
    pub fn new() -> Self {
        Self {
            battle_captures: Vec::new(),
            shiny_chance_pct: DEFAULT_SHINY_CHANCE_PCT,
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
