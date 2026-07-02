//! Clean-room **Muscle Dome card-battle** rules engine.
//!
//! A port of the arena logic resident in the battle-action overlay (PROT
//! 0898): the four-slot hand deal, the point-budget card commit into the
//! fighter's action queue, the HP-ratio score readout, and the win/lose /
//! Seru-reward bookkeeping - driven by the parsed hand tables
//! ([`legaia_asset::muscle_dome`]) and per-command costs (the equipment
//! sections' swing-record `+0x74` bytes,
//! [`legaia_asset::battle_char_assembly::SwingAnimation::cost`]). This is
//! the *rules* layer: the card/sprite presentation, dome camera, and the
//! full battle-action playback are host concerns.
//!
//! What is pinned (see
//! [`docs/subsystems/minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md)):
//!
//! - The hand is four cards; each card's id comes from the deck table
//!   `DAT_801f4b8c` (the direction-command ids `0xC..=0xF`) and its cost
//!   from the fighter's per-command record (`DAT_801c9360[char][cmd]+0x74`,
//!   the same byte the Arts gauge reads). `FUN_801d388c` case 9.
//! - The round budget `ctx+0x6dc` seeds from the fighter record `+0x154`;
//!   commit (`case 0xb`) rejects an overspend, appends the card's command id
//!   to the actor `+0x1df` queue (16 slots, zeroed on the round's first
//!   commit), debits `ctx+0x6dc` and accrues `ctx+0x6d8`.
//! - The score readout is `hp * 0x6c / max_hp` (the phase-`0x6e` arm of
//!   `FUN_801d0748`); win/lose phases branch on the fighter HP fields.
//! - The reward message composes a spell name from the shared spell-name
//!   table at id `ctx+0x269 + 0x80` (the player Seru-magic block).
//!
//! What is a documented host model: the opponent commits through the same
//! selection logic (retail has no dome-specific AI table) - here greedily in
//! hand order while its budget lasts; and per-card damage resolution goes
//! through a host-supplied function (retail plays each queued action through
//! the shared battle-action path; whether any dome-specific scaling applies
//! is an open question on the doc).
//!
//! Chain: retail `FUN_801d0748` (match SM, `ctx+6` phases) → `FUN_801d388c`
//! (deal / commit) → the battle-action path (queued-card playback).

/// Hand size (the retail deal loop builds exactly four slots).
pub const HAND_SLOTS: usize = legaia_asset::muscle_dome::HAND_SLOTS;

/// Queue capacity: the round's first commit zeroes `actor+0x1df..+0x1ee`
/// (16 bytes), bounding the per-round queue.
pub const QUEUE_CAP: usize = 0x10;

/// The score readout's scale: `hp * 0x6c / max_hp` (the ratio × 108).
pub const SCORE_SCALE: i32 = 0x6C;

/// Spell-name id base for the reward (`ctx+0x269 + 0x80`, the player
/// Seru-magic block of the shared spell table).
pub const REWARD_SPELL_ID_BASE: u8 = 0x80;

/// One dealt card: a direction-command id + its per-fighter cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuscleCard {
    /// Command id (`0xC..=0xF`, from the deck table `DAT_801f4b8c`).
    pub command_id: u8,
    /// AP cost (the fighter's per-command record `+0x74` byte).
    pub cost: u16,
}

/// Match phase, host view of the retail `ctx+6` loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusclePhase {
    /// Cards are being committed under the round budget.
    Select,
    /// Both queues are built; the round is ready to play out.
    Resolve,
    /// The round played out; HP / score updated, next round or a decision.
    RoundOver,
    /// The player's fighter won (reward available).
    Won,
    /// The player's fighter lost.
    Lost,
}

/// One fighter's dome state.
#[derive(Debug, Clone)]
struct DomeFighter {
    hand: [MuscleCard; HAND_SLOTS],
    /// Remaining round budget (`ctx+0x6dc`), reseeded each round.
    budget: u16,
    /// Points spent this round (`ctx+0x6d8`).
    spent: u16,
    /// The `+0x1df` action queue: committed command ids this round.
    queue: Vec<u8>,
    hp: i32,
    max_hp: i32,
    /// The `+0x154` pool the budget reseeds from each round.
    budget_pool: u16,
}

impl DomeFighter {
    fn new(hand: [MuscleCard; HAND_SLOTS], budget_pool: u16, hp: i32) -> Self {
        Self {
            hand,
            budget: budget_pool,
            spent: 0,
            queue: Vec::new(),
            hp,
            max_hp: hp.max(1),
            budget_pool,
        }
    }

    fn reset_round(&mut self) {
        self.budget = self.budget_pool;
        self.spent = 0;
        self.queue.clear();
    }
}

/// The running Muscle Dome contest. Slot 0 = the player's fighter, slot 1 =
/// the opponent.
#[derive(Debug, Clone)]
pub struct MuscleDomeSession {
    f: [DomeFighter; 2],
    phase: MusclePhase,
    round: u32,
    /// The awarded Seru index (`ctx+0x269`); the reward spell id is
    /// `REWARD_SPELL_ID_BASE + index`.
    reward_seru_index: u8,
    /// Damage applied to each fighter in the last resolution, for the HUD.
    last_round_damage: [i32; 2],
}

impl MuscleDomeSession {
    /// Start a contest: per-fighter hands (deck command ids + that fighter's
    /// costs), round-budget pools (record `+0x154`), HP, and the Seru index
    /// awarded on a win.
    pub fn new(
        player_hand: [MuscleCard; HAND_SLOTS],
        opponent_hand: [MuscleCard; HAND_SLOTS],
        budget_pools: [u16; 2],
        hp: [i32; 2],
        reward_seru_index: u8,
    ) -> Self {
        Self {
            f: [
                DomeFighter::new(player_hand, budget_pools[0], hp[0]),
                DomeFighter::new(opponent_hand, budget_pools[1], hp[1]),
            ],
            phase: MusclePhase::Select,
            round: 0,
            reward_seru_index,
            last_round_damage: [0, 0],
        }
    }

    /// Current phase.
    pub fn phase(&self) -> MusclePhase {
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

    /// A fighter's hand.
    pub fn hand(&self, slot: usize) -> &[MuscleCard; HAND_SLOTS] {
        &self.f[slot].hand
    }

    /// Remaining round budget (`ctx+0x6dc`).
    pub fn budget(&self, slot: usize) -> u16 {
        self.f[slot].budget
    }

    /// Points spent this round (`ctx+0x6d8`).
    pub fn spent(&self, slot: usize) -> u16 {
        self.f[slot].spent
    }

    /// The committed command-id queue (`actor+0x1df`).
    pub fn queue(&self, slot: usize) -> &[u8] {
        &self.f[slot].queue
    }

    /// Damage each side took in the last resolved round.
    pub fn last_round_damage(&self) -> [i32; 2] {
        self.last_round_damage
    }

    /// The score readout: `hp * 0x6c / max_hp` (retail renders this in the
    /// phase-`0x6e` arm).
    ///
    /// PORT: FUN_801d0748 phase 0x6e (`actor[+0x14c]*0x6c/actor[+0x14e]`)
    pub fn score_percent(&self, slot: usize) -> i32 {
        self.f[slot].hp * SCORE_SCALE / self.f[slot].max_hp
    }

    /// The reward spell id on a win (`REWARD_SPELL_ID_BASE + ctx+0x269`, an
    /// id into the shared spell-name table's player Seru-magic block).
    pub fn reward_spell_id(&self) -> u8 {
        REWARD_SPELL_ID_BASE.wrapping_add(self.reward_seru_index)
    }

    /// Whether the contest is decided.
    pub fn decided(&self) -> bool {
        matches!(self.phase, MusclePhase::Won | MusclePhase::Lost)
    }

    /// Whether `slot` can commit hand card `card_slot` right now: selection
    /// phase, queue space, and the budget covers the cost.
    pub fn can_commit(&self, slot: usize, card_slot: usize) -> bool {
        self.phase == MusclePhase::Select
            && card_slot < HAND_SLOTS
            && self.f[slot].queue.len() < QUEUE_CAP
            && self.f[slot].budget >= self.f[slot].hand[card_slot].cost
    }

    /// Commit one hand card: append its command id to the fighter's action
    /// queue, debit the budget, accrue the spent total. Returns `false`
    /// (rejected) on an overspend or outside the selection phase.
    ///
    /// PORT: FUN_801d388c case 0xb (budget gate, `actor+0x1df` append,
    /// `ctx+0x6d8`/`ctx+0x6dc` accounting)
    pub fn commit_card(&mut self, slot: usize, card_slot: usize) -> bool {
        if !self.can_commit(slot, card_slot) {
            return false;
        }
        let card = self.f[slot].hand[card_slot];
        self.f[slot].queue.push(card.command_id);
        self.f[slot].spent += card.cost;
        self.f[slot].budget -= card.cost;
        true
    }

    /// The opponent's selection: the same commit logic in hand order while
    /// the budget lasts (retail reuses the shared deal/commit paths keyed on
    /// `ctx+0x13`; there is no dome-specific AI table - the in-order greedy
    /// walk is the host model).
    pub fn ai_commit_all(&mut self, slot: usize) {
        loop {
            let pick = (0..HAND_SLOTS).find(|&c| self.can_commit(slot, c));
            match pick {
                Some(c) => {
                    self.commit_card(slot, c);
                }
                None => break,
            }
        }
    }

    /// Close the selection phase (the player confirms their queue).
    pub fn end_selection(&mut self) {
        if self.phase == MusclePhase::Select {
            self.phase = MusclePhase::Resolve;
        }
    }

    /// Play the round out: both queues resolve through `damage(attacker_slot,
    /// command_id) -> damage` (the host's battle-path stand-in), alternating
    /// player-first, stopping at a KO. Retail resolves each queued action
    /// through the shared battle-action machinery against the opposing
    /// actor record.
    ///
    /// PORT: FUN_801d0748 commit phases 0x3c/0x46/0x50 (queue walk into
    /// `actor+0x1dd`/`+0x1de`, effect applied to the opposing record's HP)
    pub fn resolve_round(&mut self, mut damage: impl FnMut(usize, u8) -> i32) {
        if self.phase != MusclePhase::Resolve {
            return;
        }
        self.last_round_damage = [0, 0];
        let max_len = self.f[0].queue.len().max(self.f[1].queue.len());
        'play: for i in 0..max_len {
            for attacker in 0..2usize {
                let defender = attacker ^ 1;
                let Some(&cmd) = self.f[attacker].queue.get(i) else {
                    continue;
                };
                let d = damage(attacker, cmd).max(0);
                self.last_round_damage[defender] += d;
                self.f[defender].hp = (self.f[defender].hp - d).max(0);
                if self.f[defender].hp == 0 {
                    break 'play;
                }
            }
        }
        self.phase = match (self.f[0].hp == 0, self.f[1].hp == 0) {
            (true, _) => MusclePhase::Lost,
            (false, true) => MusclePhase::Won,
            (false, false) => MusclePhase::RoundOver,
        };
    }

    /// Start the next round after a non-terminal resolution: reseed the
    /// budgets from the pools, clear the queues.
    pub fn next_round(&mut self) {
        if self.phase != MusclePhase::RoundOver {
            return;
        }
        self.round += 1;
        self.f[0].reset_round();
        self.f[1].reset_round();
        self.phase = MusclePhase::Select;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hand(costs: [u16; 4]) -> [MuscleCard; 4] {
        [
            MuscleCard {
                command_id: 0x0C,
                cost: costs[0],
            },
            MuscleCard {
                command_id: 0x0F,
                cost: costs[1],
            },
            MuscleCard {
                command_id: 0x0E,
                cost: costs[2],
            },
            MuscleCard {
                command_id: 0x0D,
                cost: costs[3],
            },
        ]
    }

    fn session() -> MuscleDomeSession {
        MuscleDomeSession::new(
            hand([0x1E, 0x2A, 0x2A, 0x1E]),
            hand([0x1E, 0x1E, 0x1E, 0x1E]),
            [100, 70],
            [500, 400],
            3,
        )
    }

    #[test]
    fn commit_respects_the_budget() {
        let mut s = session();
        assert!(s.commit_card(0, 0)); // 0x1E = 30, budget 70 left
        assert!(s.commit_card(0, 1)); // 0x2A = 42, budget 28 left
        assert_eq!(s.spent(0), 72);
        assert_eq!(s.budget(0), 28);
        assert!(!s.commit_card(0, 2), "42 > 28 rejected");
        assert!(!s.commit_card(0, 3), "30 > 28 rejected");
    }

    #[test]
    fn queue_carries_command_ids() {
        let mut s = session();
        s.commit_card(0, 0);
        s.commit_card(0, 3);
        assert_eq!(s.queue(0), &[0x0C, 0x0D]);
    }

    #[test]
    fn ai_commits_greedily_under_budget() {
        let mut s = session();
        s.ai_commit_all(1);
        // Pool 70, all cards 30: two commits (60), third rejected.
        assert_eq!(s.queue(1).len(), 2);
        assert_eq!(s.spent(1), 60);
    }

    #[test]
    fn resolution_alternates_and_scores_hp_ratio() {
        let mut s = session();
        s.commit_card(0, 0);
        s.commit_card(0, 1);
        s.ai_commit_all(1);
        s.end_selection();
        assert_eq!(s.phase(), MusclePhase::Resolve);
        s.resolve_round(|_, _| 50);
        // Player queued 2, opponent 2: both take 100.
        assert_eq!(s.hp(0), 400);
        assert_eq!(s.hp(1), 300);
        assert_eq!(s.last_round_damage(), [100, 100]);
        assert_eq!(s.phase(), MusclePhase::RoundOver);
        // Score readout = hp * 0x6c / max.
        assert_eq!(s.score_percent(0), 400 * 0x6C / 500);
        assert_eq!(s.score_percent(1), 300 * 0x6C / 400);
        // Next round reseeds budgets + clears queues.
        s.next_round();
        assert_eq!(s.phase(), MusclePhase::Select);
        assert_eq!(s.budget(0), 100);
        assert!(s.queue(0).is_empty());
    }

    #[test]
    fn ko_decides_the_contest_and_names_the_reward() {
        let mut s = session();
        s.commit_card(0, 0);
        s.end_selection();
        s.resolve_round(|attacker, _| if attacker == 0 { 1000 } else { 0 });
        assert_eq!(s.phase(), MusclePhase::Won);
        assert!(s.decided());
        assert_eq!(s.reward_spell_id(), 0x83);
        assert_eq!(s.score_percent(1), 0);
    }

    #[test]
    fn player_ko_loses() {
        let mut s = session();
        s.ai_commit_all(1);
        s.end_selection();
        s.resolve_round(|attacker, _| if attacker == 1 { 1000 } else { 0 });
        assert_eq!(s.phase(), MusclePhase::Lost);
    }
}
