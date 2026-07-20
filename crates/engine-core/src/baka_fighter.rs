//! Clean-room **Baka Fighter duel minigame** rules engine.
//!
//! A faithful port of the fight logic in the Baka Fighter overlay (PROT 0976):
//! the rock-paper-scissors exchange resolver, the HP-tiered damage kernel, the
//! comeback-critical roll, the scripted-pattern CPU move picker, and the
//! best-of-3 round/match bookkeeping - driven by the already-parsed roster +
//! action tables ([`legaia_asset::baka_opponents`]). This is the *rules*
//! layer: it consumes chosen attack types (pad presses on the player side)
//! and produces resolved exchanges, damage, round wins and the gold prize,
//! exactly as the retail overlay does. The side-view sprite presentation
//! (billboard actors, banners, HUD) is a host concern and is not covered here.
//!
//! Every formula and constant is the reading from
//! [`docs/subsystems/minigame-baka-fighter.md`](../../../docs/subsystems/minigame-baka-fighter.md),
//! re-derived from the overlay dumps cited on each item. Two aspects are host
//! simplifications, called out inline: exchange pacing (retail sequences
//! per-action keyframes through the sprite system; this port clears the
//! exchange immediately after resolution and paces re-entry with the retail
//! cooldown decay), and the special's charge (retail lands the round-winning
//! special only on its final keyframe; this port exposes the charge as the
//! time the special has been held before the exchange settles).
//!
//! Chain: retail `FUN_801d3468` (match resolution SM) → `FUN_801d3a14`
//! (exchange win-condition) → `FUN_801d3b18` (damage) → `FUN_801d6660`
//! (comeback-crit roll); CPU picks via `FUN_801d487c`.

use legaia_asset::baka_opponents::{BakaActionSet, BakaOpponent, ROUND_WIN_TARGET};

use crate::levelup::BiosRand;

/// Starting HP each round (`FUN_801d1744` round seed: `DAT_801dbfc4 = 0xc80`).
pub const HP_START: i32 = 0xC80;

/// HP at/above this uses stat tier `[0]` (the `0x8c1` threshold).
pub const HP_TIER_HIGH: i32 = 0x8C1;

/// HP at/above this (but below [`HP_TIER_HIGH`]) uses tier `[1]` (`0x3c1`).
pub const HP_TIER_MID: i32 = 0x3C1;

/// The comeback-crit roll only fires while `0 < HP < 0x280` (`FUN_801d6660`).
pub const CRIT_HP_BAND: i32 = 0x280;

/// Per-consecutive-hit damage bonus step (`(combo - 1) * 0x40`).
pub const COMBO_DAMAGE_STEP: i32 = 0x40;

/// Post-exchange cooldown seed (`FUN_801d3468` writes 200).
pub const COOLDOWN_RESET: i32 = 200;

/// Cooldown decay per frame step (`cooldown -= frame_step * 0x10`).
pub const COOLDOWN_DECAY: i32 = 0x10;

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
    fn atk_tier(&self, hp: i32) -> i32 {
        Self::tier(&self.atk_tiers, hp)
    }

    /// The HP-keyed DEF tier (same thresholds).
    fn def_tier(&self, hp: i32) -> i32 {
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
struct FighterState {
    /// Current round HP (`&DAT_801dbfc4[slot]`).
    hp: i32,
    /// Round wins (`&DAT_801dbff0[slot]`); 2 takes the match.
    round_wins: u32,
    /// Consecutive hits *taken* (`&DAT_801dbfec[slot]`) - feeds the escalating
    /// combo damage bonus and resets when this fighter wins an exchange.
    combo: i32,
    /// Total hits taken this match (`&DAT_801dbff4[slot]`).
    hits_taken: u32,
    /// Chosen attack this exchange (`&DAT_801dbfe0[slot]`, `None` = type 0).
    chosen: Option<BakaAttack>,
    /// "Already committed this exchange" flag (`&DAT_801dbfe8[slot]`).
    committed: bool,
    /// Pending comeback critical (`&DAT_801dc05c[slot]`).
    crit_pending: bool,
    /// Attack-rate cooldown (`DAT_801dbea0` / `DAT_801dbea4`).
    cooldown: i32,
    /// CPU scripted-pattern cursor (`&DAT_801dc044[slot]`, counts DOWN).
    ai_cursor: usize,
    /// Frames the current special has charged (host view of the retail
    /// keyframe gate - see [`BakaFight::choose`]).
    special_charge: u32,
}

impl FighterState {
    fn new() -> Self {
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
            special_charge: 0,
        }
    }

    fn reset_round(&mut self) {
        self.hp = HP_START;
        self.combo = 0;
        self.chosen = None;
        self.committed = false;
        self.crit_pending = false;
        self.cooldown = COOLDOWN_RESET;
        self.special_charge = 0;
    }
}

/// Match phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPhase {
    /// Exchanges run; fighters choose and resolve.
    Fighting,
    /// A round just ended (winner indexed); the next tick starts the next round.
    RoundOver(usize),
    /// The match is decided.
    MatchOver(usize),
}

/// Frames a special must charge before it lands as a full (round-winning) hit.
/// Host view of the retail final-keyframe gate: the special's keyframe count
/// (action record `+0x1c`, 1..=5 corpus-wide) times the sub-keyframe frame
/// scale; a special resolved earlier still wins the exchange but not the round.
pub const SPECIAL_CHARGE_FRAMES_PER_KEYFRAME: u32 = 4;

/// The SFX cue the duel fires when an exchange's damage lands.
///
/// Retail queues sound by writing a cue id straight into the 4-entry ring at
/// `_DAT_8007B6D8`, which the drainer `FUN_80016B6C` resolves against the
/// static descriptor table (`&DAT_8006F198 + id*8`, see
/// `docs/formats/sfx-table.md`). The damage kernel `FUN_801D3B18` writes `9`.
///
/// It is the only cue the *fight* fires: a sweep of the whole duel overlay
/// finds exactly four ring writes - this one plus the menu / tally blips
/// ([`BAKA_CUE_CONFIRM`] / [`BAKA_CUE_CURSOR`] / [`BAKA_CUE_CANCEL`]), which
/// belong to the surrounding UI, not to [`BakaFight`]. Round-start banners,
/// KOs, draws and victory poses are **silent** in retail.
pub const BAKA_CUE_HIT: u8 = 0x09;
/// Menu confirm blip (duel menu SM). Not fired by [`BakaFight`] - the host's
/// UI owns it.
pub const BAKA_CUE_CONFIRM: u8 = 0x20;
/// Menu cursor-move blip, also the score-tally tick (`FUN_801D239C`).
pub const BAKA_CUE_CURSOR: u8 = 0x21;
/// Menu cancel blip.
pub const BAKA_CUE_CANCEL: u8 = 0x37;

/// The running Baka Fighter duel.
#[derive(Debug, Clone)]
pub struct BakaFight {
    cfg: [FighterConfig; 2],
    f: [FighterState; 2],
    /// SFX cue ids queued this tick, in fire order - the host's view of the
    /// retail cue-ring writes. Drained by [`BakaFight::take_cues`].
    cues: Vec<u8>,
    /// Which slots the CPU picker drives (slot 1 in retail; both for demos).
    ai_controlled: [bool; 2],
    /// Special full-charge gate per slot, in frames (from the action set's
    /// special keyframe count).
    special_full_frames: [u32; 2],
    /// Round index (`DAT_801dbf20`).
    round: u32,
    /// Per-exchange settle timer (`DAT_801dbf54`). No seeder exists in the
    /// dumped corpus - it only ever decays - so it starts (and stays) 0
    /// unless a host installs a pace.
    settle_timer: i32,
    phase: MatchPhase,
    rng: BiosRand,
    last_exchange: Option<ExchangeReport>,
}

impl BakaFight {
    /// Start a best-of-3 match: `player_cfg` in slot 0 (pad-driven),
    /// `opponent_cfg` in slot 1 (CPU picker). `special_keyframes` are the two
    /// fighters' special keyframe counts (action record `+0x1c`).
    pub fn new(
        player_cfg: FighterConfig,
        opponent_cfg: FighterConfig,
        special_keyframes: [i32; 2],
        seed: u32,
    ) -> Self {
        Self {
            cfg: [player_cfg, opponent_cfg],
            f: [FighterState::new(), FighterState::new()],
            cues: Vec::new(),
            ai_controlled: [false, true],
            special_full_frames: [
                special_keyframes[0].max(0) as u32 * SPECIAL_CHARGE_FRAMES_PER_KEYFRAME,
                special_keyframes[1].max(0) as u32 * SPECIAL_CHARGE_FRAMES_PER_KEYFRAME,
            ],
            round: 0,
            settle_timer: 0,
            phase: MatchPhase::Fighting,
            rng: BiosRand::new(seed),
            last_exchange: None,
        }
    }

    /// Build both fighters straight from the parsed overlay tables. `None`
    /// when either roster id is out of range.
    pub fn from_tables(
        opponents: &[BakaOpponent],
        actions: &[BakaActionSet],
        player_roster: usize,
        opponent_roster: usize,
        seed: u32,
    ) -> Option<Self> {
        let p =
            FighterConfig::from_tables(opponents.get(player_roster)?, actions.get(player_roster)?);
        let o = FighterConfig::from_tables(
            opponents.get(opponent_roster)?,
            actions.get(opponent_roster)?,
        );
        let kf = [
            actions[player_roster].keyframes[legaia_asset::baka_opponents::ACTION_SPECIAL],
            actions[opponent_roster].keyframes[legaia_asset::baka_opponents::ACTION_SPECIAL],
        ];
        Some(Self::new(p, o, kf, seed))
    }

    /// Current match phase.
    pub fn phase(&self) -> MatchPhase {
        self.phase
    }

    /// Round index (0-based).
    pub fn round(&self) -> u32 {
        self.round
    }

    /// A fighter's current HP.
    pub fn hp(&self, slot: usize) -> i32 {
        self.f[slot].hp
    }

    /// A fighter's round-win count.
    pub fn round_wins(&self, slot: usize) -> u32 {
        self.f[slot].round_wins
    }

    /// A fighter's consecutive-hits-taken combo counter.
    pub fn combo(&self, slot: usize) -> i32 {
        self.f[slot].combo
    }

    /// A fighter's chosen attack this exchange, if any.
    pub fn chosen(&self, slot: usize) -> Option<BakaAttack> {
        self.f[slot].chosen
    }

    /// Whether a fighter can commit an attack right now (fighting phase, no
    /// choice pending, cooldown elapsed).
    pub fn can_choose(&self, slot: usize) -> bool {
        self.phase == MatchPhase::Fighting
            && self.f[slot].chosen.is_none()
            && !self.f[slot].committed
            && self.f[slot].cooldown <= 0
    }

    /// The last resolved exchange, for the host HUD.
    /// Drain the SFX cue ids the fight queued since the last call, in fire
    /// order (retail's cue-ring writes; see [`BAKA_CUE_HIT`]). Hosts route
    /// each through their SFX bank - the site's arts/minigame pages resolve
    /// them against the disc's class-2 sound bank.
    pub fn take_cues(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.cues)
    }

    /// Cue ids queued but not yet drained.
    pub fn pending_cues(&self) -> &[u8] {
        &self.cues
    }

    pub fn last_exchange(&self) -> Option<ExchangeReport> {
        self.last_exchange
    }

    /// Gold prize for beating the slot-1 opponent (roster record `+0x20`).
    pub fn gold_reward(&self) -> u32 {
        self.cfg[1].gold_reward
    }

    /// The match winner, once decided.
    pub fn winner(&self) -> Option<usize> {
        match self.phase {
            MatchPhase::MatchOver(w) => Some(w),
            _ => None,
        }
    }

    /// `true` once the match is decided.
    pub fn match_over(&self) -> bool {
        matches!(self.phase, MatchPhase::MatchOver(_))
    }

    /// Commit an attack for `slot` this exchange. Returns `false` (ignored)
    /// while the fighter can't act - see [`Self::can_choose`].
    pub fn choose(&mut self, slot: usize, attack: BakaAttack) -> bool {
        if !self.can_choose(slot) {
            return false;
        }
        self.f[slot].chosen = Some(attack);
        self.f[slot].special_charge = 0;
        true
    }

    /// The CPU move pick: random attack or a backward step of the scripted
    /// pattern, exactly as the retail picker rolls it.
    ///
    /// PORT: FUN_801d487c (opponent AI move picker)
    fn ai_pick(&mut self, slot: usize) -> BakaAttack {
        let roll = self.rng.next_u15() as i32;
        let r6 = roll % 6;
        let pattern_len = self.cfg[slot].ai_pattern.len();
        if r6 < 3 {
            if self.f[slot].ai_cursor == 0 {
                return BakaAttack::from_type_id((r6 % 3) as u8 + 1).unwrap();
            }
        } else if self.f[slot].ai_cursor == 0 {
            // Seed the cursor to the pattern length; the pattern then plays
            // back-to-front to exhaustion.
            if pattern_len > 0 && self.cfg[slot].ai_pattern[0] != 0 {
                self.f[slot].ai_cursor = pattern_len;
            }
            if self.f[slot].ai_cursor == 0 {
                return BakaAttack::from_type_id((r6 % 3) as u8 + 1).unwrap();
            }
        }
        self.f[slot].ai_cursor -= 1;
        let sym = self.cfg[slot].ai_pattern[self.f[slot].ai_cursor];
        BakaAttack::from_type_id((sym - 1) % 3 + 1).unwrap()
    }

    /// Resolve the current exchange.
    ///
    /// PORT: FUN_801d3a14 (exchange win-condition: settle timer, special
    /// priority, committed gates, and the 2>1 / 3>2 / 1>3 beats relation)
    fn resolve(&mut self, frame_step: i32) -> ExchangeOutcome {
        // Settle timer: while it hasn't elapsed the exchange stays open.
        if self.settle_timer - frame_step >= 0 {
            self.settle_timer -= frame_step;
            return ExchangeOutcome::Undecided;
        }
        self.settle_timer = 0;
        let p1 = self.f[0].chosen.map(BakaAttack::type_id).unwrap_or(0);
        let p2 = self.f[1].chosen.map(BakaAttack::type_id).unwrap_or(0);
        // The special is an unbeatable win (fighter 0 checked first). Host
        // pacing: a held special resolves once fully charged (the retail
        // final-keyframe hit = the round win) or the moment the opponent
        // commits an attack (guard-break: an ordinary exchange win).
        if p1 == 4 && (self.f[0].special_charge >= self.special_full_frames[0] || p2 != 0) {
            return ExchangeOutcome::FighterWins(0);
        }
        if p2 == 4 && (self.f[1].special_charge >= self.special_full_frames[1] || p1 != 0) {
            return ExchangeOutcome::FighterWins(1);
        }
        if p1 == 0 && p2 == 0 {
            return ExchangeOutcome::Undecided;
        }
        if self.f[0].committed || self.f[1].committed {
            return ExchangeOutcome::Undecided;
        }
        if p1 == p2 {
            return ExchangeOutcome::Draw;
        }
        // Beats relation from the dump: 2 beats 1, 3 beats 2, 1 beats 3.
        match (p1, p2) {
            (1, 2) | (2, 3) | (3, 1) => ExchangeOutcome::FighterWins(1),
            (2, 1) | (3, 2) | (1, 3) => ExchangeOutcome::FighterWins(0),
            // One side idle (type 0): an attack never lands on a non-attacker.
            _ => ExchangeOutcome::Undecided,
        }
    }

    /// Apply exchange damage to `loser`. Returns `(damage, critical,
    /// special_round_win)`.
    ///
    /// PORT: FUN_801d3b18 (damage application: HP-tiered ATK/DEF, combo bonus,
    /// crit override, special full-hit round win)
    fn apply_damage(&mut self, loser: usize) -> (i32, bool, bool) {
        let winner = loser ^ 1;
        // The retail ring write (`_DAT_8007b6d8 = 9`) sits at the top of
        // FUN_801D3B18, before the damage arithmetic - so a double-KO draw
        // (which applies damage twice) queues the cue twice, as it does here.
        self.cues.push(BAKA_CUE_HIT);
        self.f[loser].hits_taken += 1;
        let winner_type = self.f[winner].chosen.map(BakaAttack::type_id).unwrap_or(0);

        // Special full-hit: only a fully-charged special scores the immediate
        // round win (retail: landed on the action's final sub-keyframe).
        let mut special_round_win = false;
        if winner_type == 4 && self.f[winner].special_charge >= self.special_full_frames[winner] {
            self.f[winner].round_wins += 1;
            self.f[loser].committed = true;
            special_round_win = true;
        }

        let def_tier = self.cfg[loser].def_tier(self.f[loser].hp);
        let atk_tier = self.cfg[winner].atk_tier(self.f[winner].hp);
        let power = self.cfg[winner]
            .attack_power
            .get(winner_type as usize)
            .copied()
            .unwrap_or(0);
        let mod_ = self.cfg[loser].damage_mod;
        let combo = self.f[loser].combo;
        let hit = power + power * atk_tier / 100;
        let guard = mod_ + mod_ * def_tier / 100;
        let mut dmg = (hit * (200 - guard) * 0x20) / 100 + (combo - 1) * COMBO_DAMAGE_STEP;
        let critical = self.f[winner].crit_pending;
        if critical {
            dmg = power << 7;
        }
        if self.f[loser].hp > 0 {
            self.f[loser].hp -= dmg;
        }
        if self.f[loser].hp < 1 {
            self.f[loser].hp = 0;
        }
        self.f[loser].combo += 1;
        (dmg, critical, special_round_win)
    }

    /// Roll the comeback critical for a fighter that just took a hit: fires
    /// while `0 < HP < 0x280` on `rand() % 100 < crit_chance`.
    ///
    /// PORT: FUN_801d6660 (critical / lucky-hit roll)
    fn roll_comeback_crit(&mut self, slot: usize) {
        let hp = self.f[slot].hp;
        if hp > 0 && hp < CRIT_HP_BAND {
            let roll = self.rng.next_u15() as i32 % 100;
            if roll < self.cfg[slot].crit_chance {
                self.f[slot].crit_pending = true;
            }
        }
    }

    /// Clear the exchange state on both fighters (host pacing simplification:
    /// retail sequences the recovery through the per-action keyframes).
    fn end_exchange(&mut self) {
        for s in 0..2 {
            self.f[s].chosen = None;
            self.f[s].committed = false;
            self.f[s].special_charge = 0;
        }
    }

    /// End the current round with `winner` (KO path credits here; a landed
    /// full special already credited inside the damage kernel).
    fn end_round(&mut self, winner: usize, already_credited: bool) {
        if !already_credited {
            self.f[winner].round_wins += 1;
        }
        if self.f[winner].round_wins >= ROUND_WIN_TARGET {
            self.phase = MatchPhase::MatchOver(winner);
        } else {
            self.phase = MatchPhase::RoundOver(winner);
        }
    }

    /// Advance the duel one frame: decay cooldowns, let the CPU pick, charge
    /// a held special, resolve the exchange, and book damage / rounds / the
    /// match, mirroring the retail resolution arm.
    ///
    /// PORT: FUN_801d3468 (round / match resolution state machine)
    pub fn tick(&mut self, frame_step: i32) {
        match self.phase {
            MatchPhase::MatchOver(_) => return,
            MatchPhase::RoundOver(_) => {
                // Next round starts on the following tick (the retail banner
                // sequence sits here).
                self.round += 1;
                self.f[0].reset_round();
                self.f[1].reset_round();
                self.phase = MatchPhase::Fighting;
                return;
            }
            MatchPhase::Fighting => {}
        }

        // Cooldown decay (`cooldown -= frame_step * 0x10`, floored at 0).
        for s in 0..2 {
            if self.f[s].cooldown > 0 {
                self.f[s].cooldown -= frame_step * COOLDOWN_DECAY;
            }
            if self.f[s].cooldown < 0 {
                self.f[s].cooldown = 0;
            }
        }

        // CPU commits once its cooldown elapses.
        for s in 0..2 {
            if self.ai_controlled[s] && self.can_choose(s) {
                let pick = self.ai_pick(s);
                self.f[s].chosen = Some(pick);
            }
        }

        // A held special charges toward its full (round-winning) hit.
        for s in 0..2 {
            if self.f[s].chosen == Some(BakaAttack::Special) {
                self.f[s].special_charge += frame_step.max(0) as u32;
            }
        }

        match self.resolve(frame_step) {
            ExchangeOutcome::Undecided => {}
            ExchangeOutcome::FighterWins(w) => {
                let l = w ^ 1;
                // Phase gate: the winner must actually be mid-attack.
                if self.f[w].chosen.is_none() {
                    return;
                }
                let (damage, critical, special_round_win) = self.apply_damage(l);
                // Winner's own hit streak clears; crit flags reset; the loser
                // rolls the comeback crit (retail: FUN_801d6660(loser)).
                self.f[w].combo = 0;
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.roll_comeback_crit(l);
                // Cooldowns: fighter-0 win leaves slot 0 free (retail writes
                // 0/200); a fighter-1 win slows both (200/200).
                if w == 0 {
                    self.f[0].cooldown = 0;
                    self.f[1].cooldown = COOLDOWN_RESET;
                } else {
                    self.f[0].cooldown = COOLDOWN_RESET;
                    self.f[1].cooldown = COOLDOWN_RESET;
                }
                self.end_exchange();
                self.last_exchange = Some(ExchangeReport {
                    winner: w,
                    draw: false,
                    damage,
                    critical,
                    special_round_win,
                });
                if special_round_win {
                    self.end_round(w, true);
                } else if self.f[l].hp == 0 {
                    self.end_round(w, false);
                }
            }
            ExchangeOutcome::Draw => {
                // Both take damage, both streaks reset, both roll comebacks.
                let (d0, c0, _) = self.apply_damage(0);
                let (d1, c1, _) = self.apply_damage(1);
                self.f[0].combo = 0;
                self.f[1].combo = 0;
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.roll_comeback_crit(0);
                self.roll_comeback_crit(1);
                self.f[0].cooldown = COOLDOWN_RESET;
                self.f[1].cooldown = COOLDOWN_RESET;
                self.end_exchange();
                self.last_exchange = Some(ExchangeReport {
                    winner: 1,
                    draw: true,
                    damage: d0.max(d1),
                    critical: c0 || c1,
                    special_round_win: false,
                });
                match (self.f[0].hp == 0, self.f[1].hp == 0) {
                    // Double KO replays the round: no round win is credited.
                    (true, true) => self.phase = MatchPhase::RoundOver(0),
                    (true, false) => self.end_round(1, false),
                    (false, true) => self.end_round(0, false),
                    (false, false) => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------- ladder run

/// Phase of a cabinet [`LadderRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    /// A match against the current rung's opponent is in progress.
    Fighting,
    /// The match was won and the rung's prize joined the pot: the retail
    /// end-of-match menu is up (the "NEXT GAME / PAY OUT" cells on the PROT
    /// 1203 tally sheet, drawn by `FUN_801d239c`'s tally screen).
    Choice,
    /// The player took "PAY OUT" mid-run: the pot is banked, the run is over.
    PaidOut,
    /// A match was lost: the accumulated pot is forfeited, the run is over.
    GameOver,
    /// Every rung cleared: the full pot pays out (the "VICTORY! / ALL STAGE
    /// CLEAR!" sheet).
    AllClear,
}

/// The cabinet's ladder run with the between-match **cash-out** choice.
///
/// Retail grain: after every match win the tally screen offers "NEXT GAME"
/// or "PAY OUT" (both are widget cells in the PROT 1203 art pack, on the
/// same sheet as "GET COIN" + its digit strip - see
/// `docs/subsystems/minigame-baka-fighter.md`). Fighting on keeps the
/// accumulated prize pot at risk; paying out banks it and ends the run.
/// Two rules are host readings of the risk (stated, not overlay-pinned):
/// a mid-run loss forfeits the whole pot, and clearing the final rung pays
/// the pot out automatically. The rung prizes are the roster records' own
/// gold column, so a full 14-rung clear from rung 0 pays the full-clear
/// total (460 on the retail disc).
#[derive(Debug, Clone)]
pub struct LadderRun {
    /// `(roster_id, prize_gold)` per rung, in cabinet serve order.
    ladder: Vec<(usize, u32)>,
    rung: usize,
    pot: u32,
    banked: u32,
    forfeited: u32,
    phase: RunPhase,
}

impl LadderRun {
    /// Start a run at `start_rung` of `ladder` (`(roster_id, prize)` pairs in
    /// serve order). `None` when the ladder is empty or the rung is out of
    /// range.
    pub fn new(ladder: Vec<(usize, u32)>, start_rung: usize) -> Option<Self> {
        if ladder.is_empty() || start_rung >= ladder.len() {
            return None;
        }
        Some(Self {
            ladder,
            rung: start_rung,
            pot: 0,
            banked: 0,
            forfeited: 0,
            phase: RunPhase::Fighting,
        })
    }

    pub fn phase(&self) -> RunPhase {
        self.phase
    }

    /// Current rung index (0-based into the serve order).
    pub fn rung(&self) -> usize {
        self.rung
    }

    /// Total rungs in the ladder.
    pub fn len(&self) -> usize {
        self.ladder.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ladder.is_empty()
    }

    /// Prize pot currently at risk.
    pub fn pot(&self) -> u32 {
        self.pot
    }

    /// Coins committed by a pay-out / all-clear (0 while running or after a
    /// forfeit).
    pub fn banked(&self) -> u32 {
        self.banked
    }

    /// Coins lost to a mid-run defeat.
    pub fn forfeited(&self) -> u32 {
        self.forfeited
    }

    /// The rung being fought (or offered next): `(roster_id, prize)`.
    pub fn current(&self) -> Option<(usize, u32)> {
        self.ladder.get(self.rung).copied()
    }

    /// A match win: the rung's prize joins the pot. Moves to [`RunPhase::Choice`]
    /// (or pays out immediately on the final rung → [`RunPhase::AllClear`]).
    /// Returns the prize added, or `None` when not fighting.
    pub fn match_won(&mut self) -> Option<u32> {
        if self.phase != RunPhase::Fighting {
            return None;
        }
        let (_, prize) = self.current()?;
        self.pot += prize;
        if self.rung + 1 == self.ladder.len() {
            self.banked = self.pot;
            self.phase = RunPhase::AllClear;
        } else {
            self.phase = RunPhase::Choice;
        }
        Some(prize)
    }

    /// A match loss: the pot is forfeited. Returns the coins lost, or `None`
    /// when not fighting.
    pub fn match_lost(&mut self) -> Option<u32> {
        if self.phase != RunPhase::Fighting {
            return None;
        }
        self.forfeited = self.pot;
        self.pot = 0;
        self.phase = RunPhase::GameOver;
        Some(self.forfeited)
    }

    /// Take "NEXT GAME": risk the pot on the next rung. Returns the next
    /// rung's roster id, or `None` when no choice is pending.
    pub fn fight_on(&mut self) -> Option<usize> {
        if self.phase != RunPhase::Choice {
            return None;
        }
        self.rung += 1;
        self.phase = RunPhase::Fighting;
        self.current().map(|(roster, _)| roster)
    }

    /// Take "PAY OUT": bank the pot and end the run. Returns the coins
    /// banked, or `None` when no choice is pending.
    pub fn pay_out(&mut self) -> Option<u32> {
        if self.phase != RunPhase::Choice {
            return None;
        }
        self.banked = self.pot;
        self.phase = RunPhase::PaidOut;
        Some(self.banked)
    }
}

// --- End-of-match score tally ----------------------------------------------

/// Remainder above which the tally drains at [`TALLY_DIVISOR_FAST`] per step.
pub const TALLY_FAST_THRESHOLD: i32 = 5;
/// Remainder below which the tally drains one unit per step.
pub const TALLY_SLOW_THRESHOLD: i32 = 3;
/// Divisor applied to a large remainder (`> TALLY_FAST_THRESHOLD`).
pub const TALLY_DIVISOR_FAST: i32 = 5;
/// Divisor applied to a mid-sized remainder.
pub const TALLY_DIVISOR_MID: i32 = 2;

/// How much the end-of-match tally moves out of a counter this frame, given
/// the amount still to drain.
///
/// The tally screen animates four score counters emptying into the running
/// total and the player's gold. The step is proportional, not linear, so a big
/// remainder empties fast and the last few units tick over one at a time:
/// `> 5` drains a fifth per frame, `3..=5` a half, and `< 3` exactly one - so
/// the counter always reaches zero rather than asymptotically approaching it.
///
/// `skip` is the tally's fast-forward flag (`DAT_801dbf00`): when set the whole
/// remainder moves in one step, which is what makes holding the button snap
/// the tally to its end state.
// PORT: FUN_801d6710 (tally drain step; the doc's "digit drawer" reading is
// wrong - this function draws nothing, it is the per-frame drain rate)
pub fn tally_drain_step(remaining: i32, skip: bool) -> i32 {
    if skip {
        return remaining;
    }
    if remaining > TALLY_FAST_THRESHOLD {
        return remaining / TALLY_DIVISOR_FAST;
    }
    if remaining < TALLY_SLOW_THRESHOLD {
        return 1;
    }
    remaining / TALLY_DIVISOR_MID
}

/// Run one counter of the tally to empty, returning the per-frame steps it
/// takes. Each step is [`tally_drain_step`] of what is left; the sum is the
/// original `amount`.
///
/// Retail drains a negative counter by the same rule, which would run away
/// from zero - no call site produces one, and the port treats it as empty.
pub fn tally_drain_sequence(amount: i32) -> Vec<i32> {
    let mut left = amount.max(0);
    let mut steps = Vec::new();
    while left > 0 {
        let step = tally_drain_step(left, false).clamp(1, left);
        steps.push(step);
        left -= step;
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tally_drain_accelerates_then_ticks_out_one_at_a_time() {
        // Large remainder: a fifth per frame.
        assert_eq!(tally_drain_step(100, false), 20);
        assert_eq!(tally_drain_step(6, false), 1);
        // The 3..=5 band halves.
        assert_eq!(tally_drain_step(5, false), 2);
        assert_eq!(tally_drain_step(4, false), 2);
        assert_eq!(tally_drain_step(3, false), 1);
        // Below 3 the step is exactly one, which is what lands it on zero.
        assert_eq!(tally_drain_step(2, false), 1);
        assert_eq!(tally_drain_step(1, false), 1);
    }

    #[test]
    fn tally_fast_forward_moves_the_whole_remainder() {
        assert_eq!(tally_drain_step(1234, true), 1234);
        assert_eq!(tally_drain_sequence(1234).iter().sum::<i32>(), 1234);
    }

    #[test]
    fn tally_sequence_always_terminates_at_exactly_the_total() {
        for amount in [0, 1, 2, 3, 5, 6, 30, 460, 9999] {
            let steps = tally_drain_sequence(amount);
            assert_eq!(
                steps.iter().sum::<i32>(),
                amount,
                "tally of {amount} drains to exactly zero"
            );
            assert!(steps.iter().all(|&s| s > 0), "no zero-length step stalls");
        }
        assert!(tally_drain_sequence(0).is_empty());
        assert!(
            tally_drain_sequence(-5).is_empty(),
            "negative treated as empty"
        );
    }

    fn cfg(roster_id: usize, power: i32) -> FighterConfig {
        FighterConfig {
            roster_id,
            damage_mod: 100,
            def_tiers: [0, 0, 0],
            crit_chance: 0,
            atk_tiers: [0, 0, 0],
            attack_power: [0, power, power, power, 0],
            gold_reward: 30,
            ai_pattern: vec![1, 2, 3],
        }
    }

    fn fight() -> BakaFight {
        let mut f = BakaFight::new(cfg(0, 10), cfg(1, 10), [2, 2], 1);
        f.ai_controlled = [false, false]; // deterministic: drive both by hand
        f
    }

    #[test]
    fn a_decided_exchange_queues_the_hit_cue_and_a_draw_queues_none() {
        let mut f = fight();
        // Undecided: nobody has chosen, so no damage and no cue.
        f.tick(1);
        assert!(f.take_cues().is_empty(), "no exchange, no cue");

        // 2 beats 1 -> slot 1 wins, damage lands on slot 0, cue 9 fires once.
        f.choose(0, BakaAttack::A);
        f.choose(1, BakaAttack::B);
        f.tick(1);
        assert_eq!(f.take_cues(), vec![BAKA_CUE_HIT]);
        // Drained.
        assert!(f.take_cues().is_empty());

        // A draw (same type both sides) resolves without applying damage.
        f.choose(0, BakaAttack::A);
        f.choose(1, BakaAttack::A);
        f.tick(1);
        assert!(
            f.take_cues().is_empty(),
            "a drawn exchange applies no damage, so fires no hit cue"
        );
    }

    #[test]
    fn beats_relation_matches_the_dump() {
        // 2 beats 1, 3 beats 2, 1 beats 3.
        for (a, b, w) in [
            (BakaAttack::A, BakaAttack::B, 1usize),
            (BakaAttack::B, BakaAttack::C, 1),
            (BakaAttack::C, BakaAttack::A, 1),
            (BakaAttack::B, BakaAttack::A, 0),
            (BakaAttack::C, BakaAttack::B, 0),
            (BakaAttack::A, BakaAttack::C, 0),
        ] {
            let mut f = fight();
            assert!(f.choose(0, a));
            assert!(f.choose(1, b));
            f.tick(1);
            let r = f.last_exchange().expect("resolved");
            assert_eq!(r.winner, w, "{a:?} vs {b:?}");
            assert!(!r.draw);
            // Loser took damage; base formula: hit=10, guard=100 →
            // 10*100*0x20/100 = 320, first hit combo bonus (0-1)*0x40 = -64.
            assert_eq!(r.damage, 320 - 64);
            assert_eq!(f.hp(w ^ 1), HP_START - 256);
        }
    }

    #[test]
    fn same_type_is_a_draw_damaging_both() {
        let mut f = fight();
        assert!(f.choose(0, BakaAttack::B));
        assert!(f.choose(1, BakaAttack::B));
        f.tick(1);
        let r = f.last_exchange().expect("resolved");
        assert!(r.draw);
        assert_eq!(f.hp(0), HP_START - 256);
        assert_eq!(f.hp(1), HP_START - 256);
    }

    #[test]
    fn special_beats_everything_with_fighter0_priority() {
        let mut f = fight();
        assert!(f.choose(0, BakaAttack::Special));
        assert!(f.choose(1, BakaAttack::Special));
        f.tick(1);
        let r = f.last_exchange().expect("resolved");
        assert_eq!(r.winner, 0);
        // Special power is 0: the hit itself is the combo term only.
        assert_eq!(f.hp(0), HP_START);
    }

    #[test]
    fn charged_special_wins_the_round_outright() {
        let mut f = fight();
        assert!(f.choose(0, BakaAttack::Special));
        // Charge to full (2 keyframes * 4 frames); the exchange resolves the
        // tick the charge completes.
        let mut ticks = 0;
        while f.last_exchange().is_none() {
            f.tick(1);
            ticks += 1;
            assert!(ticks <= 8, "charged special resolves at full charge");
        }
        let r = f.last_exchange().expect("resolved");
        assert!(r.special_round_win);
        assert_eq!(f.round_wins(0), 1);
        assert!(matches!(f.phase(), MatchPhase::RoundOver(0)));
    }

    #[test]
    fn attack_on_idle_opponent_never_lands() {
        let mut f = fight();
        assert!(f.choose(0, BakaAttack::A));
        for _ in 0..10 {
            f.tick(1);
        }
        assert!(f.last_exchange().is_none());
        assert_eq!(f.hp(1), HP_START);
    }

    #[test]
    fn combo_bonus_escalates_on_consecutive_hits() {
        let mut f = fight();
        // Hit fighter 1 twice; second hit carries combo=1 → bonus 0.
        assert!(f.choose(0, BakaAttack::B));
        assert!(f.choose(1, BakaAttack::A));
        f.tick(1);
        let first = f.last_exchange().unwrap().damage;
        // Cooldown: fighter-0 win leaves slot 0 free, slot 1 at 200 (decays
        // 16/frame → ~13 frames).
        for _ in 0..13 {
            f.tick(1);
        }
        assert!(f.choose(0, BakaAttack::B));
        assert!(f.choose(1, BakaAttack::A));
        f.tick(1);
        let second = f.last_exchange().unwrap().damage;
        assert_eq!(second, first + COMBO_DAMAGE_STEP);
    }

    #[test]
    fn ko_ends_the_round_and_two_rounds_take_the_match() {
        let mut f = fight();
        let mut rounds = 0;
        let mut guard = 0;
        while !f.match_over() {
            guard += 1;
            assert!(guard < 10_000, "match terminates");
            match f.phase() {
                MatchPhase::Fighting => {
                    f.choose(0, BakaAttack::B);
                    f.choose(1, BakaAttack::A);
                }
                MatchPhase::RoundOver(w) => {
                    assert_eq!(w, 0);
                    rounds += 1;
                }
                MatchPhase::MatchOver(_) => {}
            }
            f.tick(1);
        }
        assert_eq!(f.winner(), Some(0));
        assert_eq!(f.round_wins(0), ROUND_WIN_TARGET);
        assert_eq!(rounds, 1, "second round win ends the match directly");
        assert_eq!(f.gold_reward(), 30);
    }

    #[test]
    fn hp_tier_keying_shifts_the_multipliers() {
        let mut player = cfg(0, 10);
        player.atk_tiers = [0, 50, 100]; // stronger as HP drops
        let mut f = BakaFight::new(player, cfg(1, 10), [2, 2], 1);
        f.ai_controlled = [false, false];
        // Drop fighter 0 into the low band by rigging HP directly.
        f.f[0].hp = HP_TIER_MID - 1;
        f.choose(0, BakaAttack::B);
        f.choose(1, BakaAttack::A);
        f.tick(1);
        // hit = 10 + 10*100/100 = 20 → 20*100*0x20/100 = 640, combo -64.
        assert_eq!(f.last_exchange().unwrap().damage, 640 - 64);
    }

    #[test]
    fn comeback_crit_replaces_damage_with_power_shift() {
        let mut player = cfg(0, 10);
        player.crit_chance = 100; // always
        let mut f = BakaFight::new(player, cfg(1, 10), [2, 2], 1);
        f.ai_controlled = [false, false];
        // Put fighter 0 in the crit HP band and let it take a hit → rolls.
        f.f[0].hp = CRIT_HP_BAND - 1;
        f.choose(0, BakaAttack::A);
        f.choose(1, BakaAttack::B);
        f.tick(1);
        assert!(f.f[0].crit_pending, "comeback crit armed");
        // Fighter 0's next winning hit crits: dmg = power << 7 = 1280.
        for _ in 0..13 {
            f.tick(1);
        }
        f.choose(0, BakaAttack::B);
        f.choose(1, BakaAttack::A);
        f.tick(1);
        let r = f.last_exchange().unwrap();
        assert!(r.critical);
        assert_eq!(r.damage, 10 << 7);
    }

    #[test]
    fn ai_pattern_plays_backward_after_seeding() {
        let mut opp = cfg(1, 10);
        opp.ai_pattern = vec![1, 2, 3];
        let mut f = BakaFight::new(cfg(0, 10), opp, [2, 2], 7);
        // Force the seeded-pattern branch by draining picks: over many picks
        // the backward walk must appear (3 → 2 → 1 as types C, B, A).
        let mut seen_backward = false;
        for _ in 0..64 {
            f.f[1].ai_cursor = 0;
            // Find a pick that seeds (roll % 6 >= 3): after seeding, cursor
            // is len-1 and the pick is the LAST symbol (3 → C).
            let pick = f.ai_pick(1);
            if f.f[1].ai_cursor == 2 {
                assert_eq!(pick, BakaAttack::C, "seeded pick = last symbol");
                assert_eq!(f.ai_pick(1), BakaAttack::B);
                assert_eq!(f.ai_pick(1), BakaAttack::A);
                assert_eq!(f.f[1].ai_cursor, 0);
                seen_backward = true;
                break;
            }
        }
        assert!(seen_backward, "the scripted pattern branch fired");
    }

    // ---------------------------------------------------------- ladder run

    fn run_ladder() -> Vec<(usize, u32)> {
        // Strictly-increasing prizes like the retail first lap.
        vec![(5, 10), (6, 20), (7, 30), (8, 40)]
    }

    #[test]
    fn ladder_pot_accumulates_and_pays_out() {
        let mut r = LadderRun::new(run_ladder(), 0).unwrap();
        assert_eq!(r.current(), Some((5, 10)));
        assert_eq!(r.match_won(), Some(10));
        assert_eq!(r.phase(), RunPhase::Choice);
        assert_eq!(r.pot(), 10);
        assert_eq!(r.fight_on(), Some(6));
        assert_eq!(r.match_won(), Some(20));
        assert_eq!(r.pot(), 30);
        // Cash out mid-run banks the pot and ends the run.
        assert_eq!(r.pay_out(), Some(30));
        assert_eq!(r.phase(), RunPhase::PaidOut);
        assert_eq!(r.banked(), 30);
        // No further transitions.
        assert_eq!(r.fight_on(), None);
        assert_eq!(r.match_won(), None);
    }

    #[test]
    fn ladder_loss_forfeits_the_pot() {
        let mut r = LadderRun::new(run_ladder(), 0).unwrap();
        r.match_won();
        r.fight_on();
        r.match_won();
        r.fight_on();
        assert_eq!(r.pot(), 30);
        assert_eq!(r.match_lost(), Some(30));
        assert_eq!(r.phase(), RunPhase::GameOver);
        assert_eq!(r.pot(), 0);
        assert_eq!(r.banked(), 0);
        assert_eq!(r.forfeited(), 30);
    }

    #[test]
    fn ladder_full_clear_pays_the_whole_pot() {
        let mut r = LadderRun::new(run_ladder(), 0).unwrap();
        for _ in 0..3 {
            r.match_won();
            r.fight_on();
        }
        // Final rung: the win pays out automatically (no choice pending).
        assert_eq!(r.match_won(), Some(40));
        assert_eq!(r.phase(), RunPhase::AllClear);
        assert_eq!(r.banked(), 100);
        assert_eq!(r.pay_out(), None);
    }

    #[test]
    fn ladder_start_rung_and_bounds() {
        assert!(LadderRun::new(vec![], 0).is_none());
        assert!(LadderRun::new(run_ladder(), 4).is_none());
        let mut r = LadderRun::new(run_ladder(), 3).unwrap();
        assert_eq!(r.current(), Some((8, 40)));
        // Dropping in at the last rung: one win = all clear, pot = that prize.
        assert_eq!(r.match_won(), Some(40));
        assert_eq!(r.phase(), RunPhase::AllClear);
        assert_eq!(r.banked(), 40);
    }
}
