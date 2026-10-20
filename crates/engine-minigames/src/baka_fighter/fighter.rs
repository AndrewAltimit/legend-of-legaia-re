//! Attacks, exchange reports, fighter configuration and the live per-fighter state.

use super::*;
use legaia_minigame_formats::baka_opponents::{BakaActionSet, BakaOpponent};

/// One of the duel's attack commitments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BakaAttack {
    /// Attack type 1 (beaten by 2, beats 3).
    A,
    /// Attack type 2 (beats 1, beaten by 3).
    B,
    /// Attack type 3 (beats 2, beaten by 1).
    C,
    /// Type 4 - the special / guard-break: an immediate exchange win for
    /// whoever throws it (fighter 0 has priority when both do).
    Special,
}

impl BakaAttack {
    /// The retail attack-type id (`DAT_801dbfe0` value space).
    pub fn type_id(self) -> u8 {
        match self {
            BakaAttack::A => 1,
            BakaAttack::B => 2,
            BakaAttack::C => 3,
            BakaAttack::Special => 4,
        }
    }

    /// Map a retail type id (`1..=4`) back to the attack.
    pub fn from_type_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(BakaAttack::A),
            2 => Some(BakaAttack::B),
            3 => Some(BakaAttack::C),
            4 => Some(BakaAttack::Special),
            _ => None,
        }
    }
}

/// Result of one exchange-resolution pass (`FUN_801d3a14` return space:
/// `-1` undecided / `0` fighter-0 wins / `1` fighter-1 wins / `3` draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeOutcome {
    /// No resolution this frame (nobody committed, or the settle timer runs).
    Undecided,
    /// The indexed fighter wins the exchange (its opponent takes the damage).
    FighterWins(usize),
    /// Both chose the same type - both take damage, both reset.
    Draw,
}

/// What one resolved exchange did - surfaced for the host HUD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeReport {
    /// Winning fighter slot (the draw arm reports fighter 1, matching the
    /// retail SM's final crit-roll operand).
    pub winner: usize,
    /// `true` when the exchange was a same-type draw (both damaged).
    pub draw: bool,
    /// Damage applied to the (last) loser.
    pub damage: i32,
    /// The winning hit consumed a pending comeback critical.
    pub critical: bool,
    /// A fully-charged special landed - an immediate round win.
    pub special_round_win: bool,
}

/// Per-fighter static configuration, lifted from the parsed roster + action
/// tables. Build via [`FighterConfig::from_tables`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FighterConfig {
    /// Roster id (0..17) this fighter plays as.
    pub roster_id: usize,
    /// Record `+0x24` - the base defense value (`mod + mod*def/100`).
    pub damage_mod: i32,
    /// Record `+0x28..` - DEF tier % at HP high / mid / low.
    pub def_tiers: [i32; 3],
    /// Record `+0x34` - comeback-critical chance %.
    pub crit_chance: i32,
    /// Record `+0x38..` - ATK tier % at HP high / mid / low.
    pub atk_tiers: [i32; 3],
    /// Action-record `+0x18` powers, indexed by attack type id (1..=4).
    pub attack_power: [i32; 5],
    /// Record `+0x20` - gold paid out when this fighter is beaten.
    pub gold_reward: u32,
    /// Record `+0x4c` - the scripted CPU attack loop (empty = random only).
    pub ai_pattern: Vec<u8>,
}

impl FighterConfig {
    /// Lift a roster entry + its action set into a fight configuration.
    pub fn from_tables(opponent: &BakaOpponent, actions: &BakaActionSet) -> Self {
        let mut attack_power = [0i32; 5];
        for t in 1..=4u8 {
            attack_power[t as usize] = actions.attack_power(t).unwrap_or(0);
        }
        Self {
            roster_id: opponent.index,
            damage_mod: opponent.damage_mod,
            def_tiers: opponent.def_tiers,
            crit_chance: opponent.crit_chance,
            atk_tiers: opponent.atk_tiers,
            attack_power,
            gold_reward: opponent.gold_reward,
            ai_pattern: opponent.ai_pattern.clone(),
        }
    }

    /// The HP-keyed ATK tier (`>= 0x8c1` → `[0]`, `>= 0x3c1` → `[1]`, else `[2]`).
    pub(super) fn atk_tier(&self, hp: i32) -> i32 {
        Self::tier(&self.atk_tiers, hp)
    }

    /// The HP-keyed DEF tier (same thresholds).
    pub(super) fn def_tier(&self, hp: i32) -> i32 {
        Self::tier(&self.def_tiers, hp)
    }

    fn tier(tiers: &[i32; 3], hp: i32) -> i32 {
        if hp >= HP_TIER_HIGH {
            tiers[0]
        } else if hp >= HP_TIER_MID {
            tiers[1]
        } else {
            tiers[2]
        }
    }
}

/// Per-fighter mutable duel state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FighterState {
    /// Current round HP (`&DAT_801dbfc4[slot]`).
    pub(super) hp: i32,
    /// Round wins (`&DAT_801dbff0[slot]`); 2 takes the match.
    pub(super) round_wins: u32,
    /// Consecutive hits *taken* (`&DAT_801dbfec[slot]`) - feeds the escalating
    /// combo damage bonus and resets when this fighter wins an exchange.
    pub(super) combo: i32,
    /// Total hits taken this match (`&DAT_801dbff4[slot]`).
    pub(super) hits_taken: u32,
    /// Chosen attack this exchange (`&DAT_801dbfe0[slot]`, `None` = type 0).
    pub(super) chosen: Option<BakaAttack>,
    /// "Already committed this exchange" flag (`&DAT_801dbfe8[slot]`).
    pub(super) committed: bool,
    /// Pending comeback critical (`&DAT_801dc05c[slot]`).
    pub(super) crit_pending: bool,
    /// Attack-rate cooldown (`DAT_801dbea0` / `DAT_801dbea4`).
    pub(super) cooldown: i32,
    /// CPU scripted-pattern cursor (`&DAT_801dc044[slot]`, counts DOWN).
    pub(super) ai_cursor: usize,
    /// The combat tick's frame cursor over the chosen attack's action record.
    pub(super) clock: StrikeClock,
    /// Frames the chosen attack type has been held (`&DAT_801dc048[slot]`,
    /// block `+0x8C`): the combat tick adds the frame step while the type is
    /// `1..=3` and zeroes it otherwise, and a fresh commit zeroes it after
    /// the add. The duel HUD's lit attack icon reads it as its flash.
    pub(super) hold: i32,
    /// A commit landed since the last hold step: the step zeroes the hold
    /// instead of advancing it, the order retail's combat tick runs in.
    pub(super) hold_reset: bool,
}

impl FighterState {
    pub(super) fn new() -> Self {
        Self {
            hp: HP_START,
            round_wins: 0,
            combo: 0,
            hits_taken: 0,
            chosen: None,
            committed: false,
            crit_pending: false,
            cooldown: 0,
            ai_cursor: 0,
            clock: StrikeClock::default(),
            hold: 0,
            hold_reset: false,
        }
    }

    /// One combat-tick step of the held-type timer.
    ///
    /// REF: FUN_801d3f44 (`0x801D4448..0x801D446C` the add / zero,
    /// `0x801D44CC` the commit's zero)
    pub(super) fn step_hold(&mut self, frame_step: i32) {
        let held_type = self.chosen.map(BakaAttack::type_id);
        if std::mem::take(&mut self.hold_reset) {
            self.hold = 0;
        } else if matches!(held_type, Some(1..=3)) {
            self.hold += frame_step;
        } else {
            self.hold = 0;
        }
    }

    pub(super) fn reset_round(&mut self) {
        self.hp = HP_START;
        self.combo = 0;
        self.chosen = None;
        self.committed = false;
        self.crit_pending = false;
        self.cooldown = COOLDOWN_RESET;
        self.clock = StrikeClock::default();
        self.hold = 0;
        self.hold_reset = false;
    }
}
