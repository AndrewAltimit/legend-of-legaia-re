//! Clean-room **Muscle Dome** match rules engine.
//!
//! The dome is **not a card battle**. It is a ladder of ordinary Legaia
//! battles: three courses of 8 / 8 / 13 rounds, each round one real monster
//! staged into the ordinary battle formation cell
//! ([`parse_course_ladder`]). Each turn the fighter enters a directional
//! command string under an AP budget - the same input the normal battle
//! command screen takes - and the string plays out through the shared
//! battle-action path.
//!
//! It is **not** turn-limited, and the `"Turns Left: N  HP Left: P%"` strip
//! is not its HUD. That strip's draw sites gate on `*(u8*)0x8007BD0C ==
//! 0xB6`, and `0x8007BD0C` is the four-slot monster-id formation cell
//! ([`crate::encounter_record`],
//! [`FORMATION_CELL_ADDR`](crate::capture_observations::battle_init_overlay::FORMATION_CELL_ADDR)),
//! so the gate reads "the first enemy is monster `0xB6`" - Koru, the game's
//! one four-turn timed boss. The dome's own roster tops out at id `0xAA`, so
//! no dome round can satisfy it. See
//! [`docs/subsystems/minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md).
//!
//! **A leg ends on a KO and on nothing else.** The arena does not run a
//! battle loop of its own: `FUN_801D1510` stores the round's monster id into
//! formation slot 0, clears slots 1..3, and sets the global game-mode word
//! `_DAT_8007B83C = 0x14` ([`crate::mode::GameMode::BattleInit`]) - its only
//! game-mode write. From there the round is an ordinary battle, ended by the
//! state-`0x5A` end-of-action gate of `FUN_801E295C` when one side has no
//! standing combatant, and routed back to the arena (mode `0x18`) instead of
//! the field by the exit selector `FUN_80046A20`. The battle turn counter
//! `ctx+0x28a` is bumped in one place and never compared against a bound: its
//! readers are per-turn *scripted enemy behaviour* selectors plus Koru's own
//! countdown. So there is no timeout arm to port - see
//! [`timed_fight_turns_left`].
//!
//! This module is the *rules* layer: the four-direction deal, the
//! budget-gated commit into the fighter's action queue, the turn counter,
//! the HP-Left readout, and the win / lose / Seru-reward
//! bookkeeping - driven by the parsed tables ([`legaia_asset::muscle_dome`])
//! and per-command costs (the equipment sections' swing-record `+0x74`
//! bytes, [`legaia_asset::battle_char_assembly::SwingAnimation::cost`]). The
//! sprite presentation, dome camera and the full battle-action playback are
//! host concerns.
//!
//! What is pinned (see
//! [`docs/subsystems/minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md)):
//!
//! - The deal is four slots; each slot's id comes from the deck table
//!   `DAT_801f4b8c` (the direction-command ids `0xC..=0xF`) and its cost
//!   from the fighter's per-command record (`DAT_801c9360[char][cmd]+0x74`,
//!   the same byte the Arts gauge reads). `FUN_801d388c` case 9.
//! - The turn budget `ctx+0x6dc` seeds from the fighter record `+0x154`;
//!   commit (`case 0xb`) rejects an overspend, appends the direction's
//!   command id to the actor `+0x1df` queue (16 slots, zeroed on the turn's
//!   first commit), debits `ctx+0x6dc` and accrues `ctx+0x6d8`.
//! - The timed-fight strip is drawn by the phase-`0x14` arm of
//!   `FUN_801d0748` (the shared battle round driver), gated on formation
//!   slot 0 `== `[`TIMED_FIGHT_MONSTER_ID`]: `DAT_801f6958 = 4 -
//!   ctx[+0x28a]` (Turns Left, one digit) and `DAT_801f6959 = hp * 100 /
//!   max_hp` of the **enemy** record `DAT_801c937c` (HP Left, three digits).
//!   The format string is on the disc at PROT 0898 file offset `0x0` (VA
//!   `0x801CE818`). Kept here because the port's session still reuses it as
//!   a leg bound - see [`TIMED_FIGHT_TURN_LIMIT`].
//! - `ctx+0x28a` is the battle turn counter the shared battle-action SM
//!   bumps at the end of a turn (case `0xff`, which also parks the match
//!   phase at `0x14`; see [`MuscleDomeSession::resolve_turn`]).
//! - The reward message composes a spell name from the shared spell-name
//!   table at id `ctx+0x269 + 0x80` (the player Seru-magic block).
//!
//! What is a documented host model: the opponent commits through the same
//! selection logic (retail has no dome-specific AI table) - here greedily in
//! deal order while its budget lasts, out of the player's own direction deck
//! rather than a monster action set; and per-command damage resolution goes
//! through a host-supplied function.
//!
//! Chain: retail `FUN_801d0748` (match SM, `ctx+6` phases) → `FUN_801d388c`
//! (deal / commit) → the battle-action path (queued-command playback).

// The leg-end chain the module docs above cite. None of it is ported here -
// the arena's handoff and the battle's own end scans live in the battle
// world, and this module only records that they, not a turn budget, decide
// a leg.
// REF: FUN_801d1510 (arena opponent installer: formation slot 0 + game mode 0x14)
// REF: FUN_801e295c (state 0x5A end-of-action KO scans set the battle-end signal)
// REF: FUN_80046a20 (battle-exit mode selector: mode 0x18 returns to the arena)

use legaia_asset::element_affinity::ElementAffinity;
use legaia_asset::move_power::{self, MoveRecord};
use legaia_engine_vm::battle_formulas::{
    DamageFinish, DefenderResist, SummonRollActor, arts_physical_predamage_lazy,
    damage_finish_lazy, psyq_rand_step, spirit_gauge_fill,
};

/// Deal size (the retail deal loop builds exactly four slots, one per
/// direction).
pub const HAND_SLOTS: usize = legaia_asset::muscle_dome::HAND_SLOTS;

/// Queue capacity: the turn's first commit zeroes `actor+0x1df..+0x1ee`
/// (16 bytes), bounding the per-turn queue.
pub const QUEUE_CAP: usize = 0x10;

/// Monster id the `Turns Left / HP Left` strip's draw sites gate on
/// (`*(u8*)0x8007BD0C == 0xB6`). `0x8007BD0C` is the **formation cell**, not
/// a battle-type byte, so this is a monster id: Koru. It is here, not in a
/// dome-named constant, because the earlier reading called it
/// `DOME_BATTLE_TYPE` and that was wrong.
pub const TIMED_FIGHT_MONSTER_ID: u8 = 0xB6;

/// The numerator of the timed fight's `Turns Left` digit: its HUD prints
/// `4 - ctx[+0x28a]` (`0x801d0f9c..0x801d0fa4`).
///
/// This bounds **Koru's** fight, not a dome leg. [`MuscleDomeSession`] does
/// not read it: a dome round is an ordinary battle and ends on a KO.
pub const TIMED_FIGHT_TURN_LIMIT: u32 = 4;

/// The timed fight's `Turns Left` digit for battle turn counter `turn`
/// (`ctx+0x28a`), floored at zero.
///
/// Free-standing because the strip belongs to the one fight whose formation
/// slot 0 is [`TIMED_FIGHT_MONSTER_ID`]. It is a decode of retail's
/// arithmetic, not a rule any dome session is subject to.
///
/// PORT: FUN_801d0748 phase 0x14 (`DAT_801f6958 = 4 - ctx[+0x28a]`)
///
/// NOT WIRED: deliberately. [`MuscleDomeSession`] does not read it and must
/// not - the strip is Koru's timed fight, gated on formation slot 0 being
/// [`TIMED_FIGHT_MONSTER_ID`], and a dome round is an ordinary battle that
/// ends on a knockout. A caller appears when a host draws that one fight's
/// `Turns Left / HP Left` HUD strip, which needs the formation-cell gate the
/// engine does not carry.
pub fn timed_fight_turns_left(turn: u32) -> u32 {
    TIMED_FIGHT_TURN_LIMIT.saturating_sub(turn)
}

/// The HP-Left readout's scale: `hp * 100 / max_hp`, a plain percentage. The
/// retail expression is the MIPS shift-add chain `((hp<<1 + hp)<<3 + hp)<<2`
/// at `0x801d0f38..0x801d0f4c`.
pub const HP_LEFT_SCALE: i32 = 100;

/// The fighter slot the HUD's HP-Left readout reads: retail takes it off
/// `DAT_801c937c`, actor-table index 3 - the first **enemy** slot (the party
/// occupies 0..=2). Slot 1 is this port's opponent.
pub const HP_LEFT_SLOT: usize = 1;

/// Spell-name id base for the reward (`ctx+0x269 + 0x80`, the player
/// Seru-magic block of the shared spell table).
pub const REWARD_SPELL_ID_BASE: u8 = 0x80;

/// One fighter's battle-actor stat profile, as the retail damage kernel
/// reads it off the actor record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DomeCombatant {
    /// Max HP (`+0x14e`) - the spirit-gauge fill divides by it.
    pub hp_max: u16,
    /// INT working value (`+0x168`) - the damage roll's own stat.
    pub int: u16,
    /// UDF (`+0x15c`) - defender roll term A.
    pub udf: u16,
    /// LDF (`+0x160`) - defender roll term B.
    pub ldf: u16,
    /// Element id (`0..=7`) for the affinity scale.
    pub element: u8,
}

/// One resolved command play of the last turn, for a host's playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomePlay {
    /// Fighter slot that acted.
    pub attacker: usize,
    /// The queued direction-command id (`0xC..=0xF`).
    pub cmd: u8,
    /// The move-power record's power for that command.
    pub power: i32,
    /// Damage the defender took.
    pub damage: i32,
    /// Both fighters' HP straight after the play.
    pub hp_after: [i32; 2],
}

/// The **retail damage kernel** for a dome turn: the PROT 0898 battle tables
/// plus both fighters' stat profiles and the contest's PsyQ `rand()` cursor.
///
/// A queued direction resolves exactly as retail plays a battle action - the
/// move-power record via the `0x801F4E63` id → index map
/// ([`legaia_asset::move_power`]), the arts/physical predamage roll
/// (`FUN_801dd0ac`), the element-affinity scale (`FUN_801dd864`) and the
/// damage finisher (`FUN_801ddb30`), drawing from the `rand()` stream in
/// retail call order (3 draws, +2 when the bonus arm fires, +1 when
/// mitigation floors the hit). The defender's spirit gauge accrues from each
/// hit (`spirit_gauge_fill`).
///
/// Install it on a session with
/// [`MuscleDomeSession::install_damage_model`] and drive it with
/// [`MuscleDomeSession::resolve_turn_retail`]; both the native and browser
/// hosts share this one kernel rather than each inventing a damage rule.
///
/// The model keeps its own HP mirror because the roll reads the defender's
/// *live* `+0x14c`, which drops mid-turn: the session applies the same
/// damage in the same order, so the mirror stays in step with it.
///
/// PORT: FUN_801dd0ac / FUN_801dd864 / FUN_801ddb30 (through
/// [`legaia_engine_vm::battle_formulas`])
#[derive(Debug, Clone)]
pub struct DomeDamageModel {
    move_power: Vec<MoveRecord>,
    move_map: [u8; move_power::MOVE_ID_INDEX_MAP_LEN],
    affinity: Option<ElementAffinity>,
    combatants: [DomeCombatant; 2],
    rng: u32,
    hp: [i32; 2],
    spirit: [u16; 2],
    log: Vec<DomePlay>,
}

impl DomeDamageModel {
    /// Build from already-parsed tables.
    pub fn new(
        move_power: Vec<MoveRecord>,
        move_map: [u8; move_power::MOVE_ID_INDEX_MAP_LEN],
        affinity: Option<ElementAffinity>,
        combatants: [DomeCombatant; 2],
        hp: [i32; 2],
        rng_seed: u32,
    ) -> Self {
        Self {
            move_power,
            move_map,
            affinity,
            combatants,
            rng: rng_seed,
            hp,
            spirit: [0, 0],
            log: Vec::new(),
        }
    }

    /// Parse the tables straight off the **raw** PROT 0898 entry (the
    /// move-power table, its id → index map and the element-affinity matrix
    /// are all pinned at raw-entry file offsets; the entry is stored
    /// uncompressed, so the raw and as-loaded views agree). Returns `None`
    /// when the move-power table does not decode.
    pub fn from_battle_overlay(
        raw: &[u8],
        combatants: [DomeCombatant; 2],
        hp: [i32; 2],
        rng_seed: u32,
    ) -> Option<Self> {
        let table = move_power::parse(raw)?;
        let map = move_power::parse_id_index_map(raw)?;
        let affinity = legaia_asset::element_affinity::parse(raw);
        Some(Self::new(table, map, affinity, combatants, hp, rng_seed))
    }

    /// Both fighters' stat profiles.
    pub fn combatants(&self) -> &[DomeCombatant; 2] {
        &self.combatants
    }

    /// A fighter's spirit gauge (`actor+0x170`, `0..=100`) - the value the
    /// shared battle status plate displays.
    ///
    /// REF: FUN_801d8de8 elems 0x52/0x53 (stage `actor+0x170` into the
    /// plate globals; the plate is shared battle chrome, not dome-specific)
    pub fn spirit(&self, slot: usize) -> u16 {
        self.spirit[slot]
    }

    /// The `rand()` cursor, so a host can persist or reseed the stream.
    pub fn rng_seed(&self) -> u32 {
        self.rng
    }

    /// The last resolved turn's play-by-play.
    pub fn plays(&self) -> &[DomePlay] {
        &self.log
    }

    /// Open a turn: clear the play log and re-sync the HP mirror to the
    /// session's live values.
    pub fn begin_turn(&mut self, hp: [i32; 2]) {
        self.log.clear();
        self.hp = hp;
    }

    /// Resolve one queued command - the function
    /// [`MuscleDomeSession::resolve_turn`] takes as its damage closure.
    pub fn damage(&mut self, attacker: usize, cmd: u8) -> i32 {
        let defender = attacker ^ 1;
        let power = move_power::record_for_move_id(&self.move_power, &self.move_map, cmd)
            .map(|r| r.power())
            .unwrap_or(0);
        let hp = self.hp;
        let combatants = self.combatants;
        let actor = |slot: usize| SummonRollActor {
            hp: hp[slot].clamp(0, u16::MAX as i32) as u16,
            agl: combatants[slot].int,
            stat_a: combatants[slot].udf,
            stat_b: combatants[slot].ldf,
            status: 0,
            guard: 0,
        };
        let affinity_pct = self
            .affinity
            .as_ref()
            .and_then(|a| {
                a.affinity_pct(combatants[attacker].element, combatants[defender].element)
            })
            .unwrap_or(100);
        let damage = {
            let rng = &mut self.rng;
            let rng3 = [
                psyq_rand_step(rng),
                psyq_rand_step(rng),
                psyq_rand_step(rng),
            ];
            let (att_roll, def_roll) = arts_physical_predamage_lazy(
                power,
                &actor(attacker),
                &actor(defender),
                affinity_pct,
                rng3,
                || [psyq_rand_step(rng), psyq_rand_step(rng)],
            );
            let finish = DamageFinish {
                predamage: att_roll.saturating_sub(def_roll),
                attacker_slot: if attacker == 0 { 0 } else { 3 },
                defender_slot: if defender == 0 { 0 } else { 3 },
                attacker_element: combatants[attacker].element,
                defender_resist: DefenderResist::default(),
                defender_guarding: false,
                enemy_defender_halve: false,
                bypass_party_resist: false,
                summon_power_pct: 100,
                floor_rand: 0,
            };
            damage_finish_lazy(&finish, || psyq_rand_step(rng)) as i32
        };
        self.hp[defender] = (self.hp[defender] - damage).max(0);
        self.spirit[defender] = spirit_gauge_fill(
            damage as u32,
            combatants[defender].hp_max,
            self.spirit[defender],
            DefenderResist::default(),
            defender == 0,
        );
        self.log.push(DomePlay {
            attacker,
            cmd,
            power,
            damage,
            hp_after: self.hp,
        });
        damage
    }
}

/// One dealt slot: a direction-command id + its per-fighter AP cost.
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
    /// Directions are being committed under the turn's AP budget.
    Select,
    /// Both queues are built; the turn is ready to play out.
    Resolve,
    /// The turn played out; HP updated, another turn or a decision.
    TurnOver,
    /// The player's fighter won (reward available).
    Won,
    /// The player's fighter lost.
    Lost,
}

/// One fighter's dome state.
#[derive(Debug, Clone)]
struct DomeFighter {
    hand: [MuscleCard; HAND_SLOTS],
    /// Remaining turn budget (`ctx+0x6dc`), reseeded each turn.
    budget: u16,
    /// Points spent this turn (`ctx+0x6d8`).
    spent: u16,
    /// The `+0x1df` action queue: committed command ids this turn.
    queue: Vec<u8>,
    hp: i32,
    max_hp: i32,
    /// The `+0x154` pool the budget reseeds from each turn.
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

    fn reset_turn(&mut self) {
        self.budget = self.budget_pool;
        self.spent = 0;
        self.queue.clear();
    }
}

/// The running Muscle Dome contest. Slot 0 = the player's fighter, slot 1 =
/// the opponent ([`HP_LEFT_SLOT`], the record the HUD's percentage reads).
#[derive(Debug, Clone)]
pub struct MuscleDomeSession {
    f: [DomeFighter; 2],
    phase: MusclePhase,
    /// Turns played out so far - the `ctx+0x28a` battle turn counter. It is
    /// a counter, not a budget: nothing in retail bounds a battle by it.
    turn: u32,
    /// The awarded Seru index (`ctx+0x269`); the reward spell id is
    /// `REWARD_SPELL_ID_BASE + index`.
    reward_seru_index: u8,
    /// Damage applied to each fighter in the last resolution, for the HUD.
    last_turn_damage: [i32; 2],
    /// The installed retail damage kernel, when the host has disc tables to
    /// give it ([`MuscleDomeSession::install_damage_model`]).
    damage: Option<DomeDamageModel>,
    /// The round time meter's `0..=`[`TIME_METER_MAX`] counter, advanced by
    /// [`Self::tick_time_meter`].
    time_meter: u8,
    /// The meter bar sprite's Y offset for the current counter value.
    time_meter_bar_y: i16,
}

impl MuscleDomeSession {
    /// Start a contest: per-fighter deals (deck command ids + that fighter's
    /// costs), turn-budget pools (record `+0x154`), HP, and the Seru index
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
            turn: 0,
            reward_seru_index,
            last_turn_damage: [0, 0],
            damage: None,
            time_meter: 0,
            time_meter_bar_y: time_meter_step(0, 0, false, false).1,
        }
    }

    /// Advance the round **time meter** one frame by the frame delta `dt`.
    ///
    /// The counter climbs while the contest is in its selection phase (retail's
    /// phase tag `'P'`) and drains otherwise, and the bar sprite's Y offset
    /// follows it ([`time_meter_step`]). Retail additionally gates the climb on
    /// a separate ramp flag; nothing in the port lowers that flag mid-selection,
    /// so the session passes it up and the phase is the whole gate here.
    ///
    /// Returns the bar's new Y offset.
    pub fn tick_time_meter(&mut self, dt: u8) -> i16 {
        let in_select = self.phase == MusclePhase::Select;
        let (counter, bar_y) = time_meter_step(self.time_meter, dt, in_select, in_select);
        self.time_meter = counter;
        self.time_meter_bar_y = bar_y;
        bar_y
    }

    /// The time meter's current counter, `0..=`[`TIME_METER_MAX`].
    pub fn time_meter(&self) -> u8 {
        self.time_meter
    }

    /// The time-meter bar sprite's current Y offset (`-0x92` empty, `+0xE`
    /// full).
    pub fn time_meter_bar_y(&self) -> i16 {
        self.time_meter_bar_y
    }

    /// Current phase.
    pub fn phase(&self) -> MusclePhase {
        self.phase
    }

    /// Turns played out so far (0-based) - the `ctx+0x28a` counter. Unbounded:
    /// a leg runs until one fighter drops.
    pub fn turn(&self) -> u32 {
        self.turn
    }

    /// A fighter's current HP.
    pub fn hp(&self, slot: usize) -> i32 {
        self.f[slot].hp
    }

    /// A fighter's dealt directions.
    pub fn hand(&self, slot: usize) -> &[MuscleCard; HAND_SLOTS] {
        &self.f[slot].hand
    }

    /// Remaining turn budget (`ctx+0x6dc`).
    pub fn budget(&self, slot: usize) -> u16 {
        self.f[slot].budget
    }

    /// Points spent this turn (`ctx+0x6d8`).
    pub fn spent(&self, slot: usize) -> u16 {
        self.f[slot].spent
    }

    /// The committed command-id queue (`actor+0x1df`).
    pub fn queue(&self, slot: usize) -> &[u8] {
        &self.f[slot].queue
    }

    /// Damage each side took in the last resolved turn.
    pub fn last_turn_damage(&self) -> [i32; 2] {
        self.last_turn_damage
    }

    /// A fighter's HP as a plain percentage of its maximum:
    /// `hp * 100 / max_hp`.
    pub fn hp_left_percent(&self, slot: usize) -> i32 {
        self.f[slot].hp * HP_LEFT_SCALE / self.f[slot].max_hp
    }

    /// The HUD's **HP Left** readout: the *opponent's* remaining HP as a
    /// percentage ([`HP_LEFT_SLOT`]). This is the quantity the dome scores a
    /// timed-out leg on - not a per-fighter score, and the scale is 100, not
    /// the `0x6C` an earlier reading took off the shift-add chain.
    ///
    /// PORT: FUN_801d0748 phase 0x14 (`DAT_801f6959 =
    /// DAT_801c937c[+0x14c] * 100 / DAT_801c937c[+0x14e]`)
    pub fn hp_left(&self) -> i32 {
        self.hp_left_percent(HP_LEFT_SLOT)
    }

    /// The reward spell id on a win (`REWARD_SPELL_ID_BASE + ctx+0x269`, an
    /// id into the shared spell-name table's player Seru-magic block).
    pub fn reward_spell_id(&self) -> u8 {
        REWARD_SPELL_ID_BASE.wrapping_add(self.reward_seru_index)
    }

    /// The three-part **victory banner** retail composes on a dome win, as
    /// indices - the host resolves the strings.
    ///
    /// `char_id` is the winning fighter's 1-based character id (retail's
    /// `DAT_8007BD10[ctx+0x13]`); the lead-in line is entry `char_id - 1` of
    /// the victory-message pointer table
    /// ([`legaia_asset::muscle_dome::VICTORY_MSG_TABLE_VA`], which holds
    /// exactly the three party fighters' lines), then the reward spell's
    /// name, then a fixed suffix.
    ///
    /// Retail runs this assembly **inline** in `FUN_801D8DE8`'s HUD case
    /// `0x59` (`0x801D9154..0x801D91D0`); `FUN_801DBA90` is a standalone,
    /// instruction-identical twin of that arm which no image references. The
    /// port composes through the decode of the twin because it is the same
    /// rule; the *live* site is the case-`0x59` arm.
    // REF: FUN_801dba90 (the standalone twin this delegates to)
    // REF: FUN_801d8de8 (case 0x59, the live site of the same assembly)
    pub fn reward_banner(
        &self,
        char_id: u8,
    ) -> legaia_engine_vm::battle_cast_dispatch::RewardBanner {
        legaia_engine_vm::battle_cast_dispatch::reward_banner(char_id, self.reward_seru_index)
    }

    /// Whether the leg is over. A KO either way - there is no other way for a
    /// dome leg to end.
    pub fn decided(&self) -> bool {
        matches!(self.phase, MusclePhase::Won | MusclePhase::Lost)
    }

    /// Whether `slot` can commit dealt direction `card_slot` right now:
    /// selection phase, queue space, and the budget covers the cost.
    pub fn can_commit(&self, slot: usize, card_slot: usize) -> bool {
        self.phase == MusclePhase::Select
            && card_slot < HAND_SLOTS
            && self.f[slot].queue.len() < QUEUE_CAP
            && self.f[slot].budget >= self.f[slot].hand[card_slot].cost
    }

    /// Commit one dealt direction: append its command id to the fighter's
    /// action queue, debit the budget, accrue the spent total. Returns
    /// `false` (rejected) on an overspend or outside the selection phase.
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

    /// The opponent's selection: the same commit logic in deal order while
    /// the budget lasts (retail reuses the shared deal/commit paths keyed on
    /// `ctx+0x13`; there is no dome-specific AI table - the in-order greedy
    /// walk is the host model).
    ///
    /// Host model, disclosed: the opponent draws from the *player's* four
    /// direction commands (`0xC..=0xF`), not from a monster action set. A
    /// monster fights the dome through its own PROT 867 action stream, which
    /// this session does not model.
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

    /// Whether `slot`'s selection is exhausted: no dealt direction is
    /// affordable (or the queue is full). Retail ends the input automatically
    /// at this point - the phase byte advances off the input arm without a
    /// confirm press (recomp capture: three 30-cost commits on a 100 budget
    /// move `ctx+6` `0x50 -> 0x5a` on the third press).
    ///
    /// REF: FUN_801d0748 phase 0x50
    pub fn selection_exhausted(&self, slot: usize) -> bool {
        self.phase == MusclePhase::Select && (0..HAND_SLOTS).all(|c| !self.can_commit(slot, c))
    }

    /// Reselect: throw the fighter's committed queue away and restore the
    /// turn budget (the retail confirm menu's "Reselect" arm returns to a
    /// clean input state - queue re-zeroed, budget back at the pool seed).
    ///
    /// REF: FUN_801d0748 phase 0x6e
    pub fn reset_selection(&mut self, slot: usize) {
        if self.phase != MusclePhase::Select && self.phase != MusclePhase::Resolve {
            return;
        }
        self.f[slot].queue.clear();
        self.f[slot].budget += self.f[slot].spent;
        self.f[slot].spent = 0;
        self.phase = MusclePhase::Select;
    }

    /// Play the turn out: each fighter's **entire** queued command string
    /// resolves as one turn - the player's first, then the opponent's -
    /// through `damage(attacker_slot, command_id) -> damage` (the host's
    /// battle-path stand-in), stopping at a KO. Retail plays a battle turn
    /// the same way: one actor's queued `+0x1df` string runs to completion
    /// through the shared battle-action machinery before the next actor
    /// takes its turn. The strings are **not** interleaved command-by-command.
    ///
    /// Bumps the [`turn`](Self::turn) counter (the `ctx+0x28a` analogue) and
    /// settles the phase: a KO decides the leg, and anything else continues at
    /// [`MusclePhase::TurnOver`]. Nothing else can end it - retail's only
    /// battle-end signal (`DAT_8007BD71 = 0xFE`) comes from the `0x5A`
    /// end-of-action KO scans, never from the turn counter.
    ///
    /// PORT: FUN_801d0748 commit phases 0x3c/0x46/0x50 (queue walk into
    /// `actor+0x1dd`/`+0x1de`, effect applied to the opposing record's HP)
    ///
    /// REF: FUN_801e295c case 0xff (`ctx[+0x28a] += 1`, phase back to 0x14 -
    /// the shared battle-action SM owns the turn counter this bumps)
    pub fn resolve_turn(&mut self, mut damage: impl FnMut(usize, u8) -> i32) {
        if self.phase != MusclePhase::Resolve {
            return;
        }
        self.last_turn_damage = [0, 0];
        'play: for attacker in 0..2usize {
            let defender = attacker ^ 1;
            for i in 0..self.f[attacker].queue.len() {
                let cmd = self.f[attacker].queue[i];
                let d = damage(attacker, cmd).max(0);
                self.last_turn_damage[defender] += d;
                self.f[defender].hp = (self.f[defender].hp - d).max(0);
                if self.f[defender].hp == 0 {
                    break 'play;
                }
            }
        }
        self.turn += 1;
        self.phase = match (self.f[0].hp == 0, self.f[1].hp == 0) {
            (true, _) => MusclePhase::Lost,
            (false, true) => MusclePhase::Won,
            (false, false) => MusclePhase::TurnOver,
        };
    }

    /// Install the shared [`DomeDamageModel`] so the turn can resolve through
    /// the **retail** battle formulas instead of a host stand-in.
    pub fn install_damage_model(&mut self, model: DomeDamageModel) {
        self.damage = Some(model);
    }

    /// The installed retail damage kernel, if any.
    pub fn damage_model(&self) -> Option<&DomeDamageModel> {
        self.damage.as_ref()
    }

    /// A fighter's spirit gauge (`actor+0x170`), `0` with no damage model
    /// installed.
    pub fn spirit(&self, slot: usize) -> u16 {
        self.damage.as_ref().map_or(0, |m| m.spirit(slot))
    }

    /// The last resolved turn's play-by-play, empty with no damage model
    /// installed.
    pub fn last_turn_plays(&self) -> &[DomePlay] {
        self.damage.as_ref().map_or(&[], |m| m.plays())
    }

    /// Play the turn out through the installed retail damage kernel - the
    /// path both hosts use. Returns `false` (and does nothing) when no
    /// [`DomeDamageModel`] is installed.
    pub fn resolve_turn_retail(&mut self) -> bool {
        let Some(mut model) = self.damage.take() else {
            return false;
        };
        model.begin_turn([self.f[0].hp, self.f[1].hp]);
        self.resolve_turn(|attacker, cmd| model.damage(attacker, cmd));
        self.damage = Some(model);
        true
    }

    /// Start the next turn after a non-terminal resolution: reseed the
    /// budgets from the pools, clear the queues. No-op once a KO has decided
    /// the leg.
    pub fn next_turn(&mut self) {
        if self.phase != MusclePhase::TurnOver {
            return;
        }
        self.f[0].reset_turn();
        self.f[1].reset_turn();
        self.phase = MusclePhase::Select;
    }
}

/// Fixed item id of the one-shot Master-course first-clear prize (the
/// War God Icon; `FUN_800421D4(0xCD, 1)`).
pub const CONTEST_PRIZE_ITEM_ID: u8 = 0xCD;

/// Story-flag id of the one-shot prize latch (`FUN_8003CE64(0x6CB)` - once
/// set, the prize never re-awards).
pub const CONTEST_PRIZE_FLAG: u16 = 0x6CB;

/// The Master-course fight index the prize gates on (`round >= 0xD`, i.e.
/// the 13th and final fight of the Master course row).
pub const CONTEST_PRIZE_ROUND: u32 = 0xD;

// --- The course ladder: who you actually fight ------------------------------

/// PROT entry holding the arena door/init overlay the ladder lives in.
pub const ARENA_OVERLAY_PROT_INDEX: usize = 977;

/// Load base of that entry as a slot-A overlay.
pub const ARENA_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Overlay VA of the 3-entry course descriptor table.
pub const COURSE_TABLE_VA: u32 = 0x801D_1A08;

/// File offset of the course descriptor table in the raw entry.
pub const COURSE_TABLE_FILE_OFFSET: usize = (COURSE_TABLE_VA - ARENA_OVERLAY_BASE_VA) as usize;

/// Overlay VA of the per-`(course, round)` score table.
pub const SCORE_TABLE_VA: u32 = 0x801D_1860;

/// File offset of the score table in the raw entry.
pub const SCORE_TABLE_FILE_OFFSET: usize = (SCORE_TABLE_VA - ARENA_OVERLAY_BASE_VA) as usize;

/// Row stride of the score table: 16 `i32` cells per course.
pub const SCORE_TABLE_COURSE_STRIDE: usize = 0x40;

/// Courses the arena offers.
pub const COURSE_COUNT: usize = 3;

/// Byte stride of one course descriptor (`{ i32 count; u32 first_round }`).
pub const COURSE_DESCRIPTOR_STRIDE: usize = 8;

/// Byte stride of one round record (`{ u32 name_ptr; u32 monster_id }`).
pub const ROUND_RECORD_STRIDE: usize = 8;

/// Rounds any one course may declare, as a sanity bound on the descriptor.
pub const MAX_ROUNDS_PER_COURSE: usize = 16;

/// One round of a course: the opponent, and where its label lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomeRound {
    /// `+0x00` - overlay VA of the round's label string (the course menu
    /// draws it; the port does not need the text to fight the round).
    pub label_va: u32,
    /// `+0x04` - the opponent's **monster id**, the byte `FUN_801D1510`
    /// stores into formation slot 0 at `0x8007BD0C`. Index it into the
    /// monster archive as `(id - 1) * 0x14000`.
    pub monster_id: u8,
}

/// One course of the ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomeCourse {
    /// Its rounds, in fight order.
    pub rounds: Vec<DomeRound>,
}

/// Decode the arena's course ladder out of a raw PROT 0977 entry.
///
/// The descriptor table at [`COURSE_TABLE_FILE_OFFSET`] holds three
/// `{ i32 round_count; u32 first_round }` records; each `first_round`
/// points at a run of `round_count` `{ u32 label_va; u32 monster_id }`
/// records in the same entry. Retail's `FUN_801D1510` indexes exactly this
/// pair with `(DAT_801D1A90, DAT_801D1A94)` - the same `(course, round)` the
/// score table takes - and writes the round's `monster_id` byte into
/// formation slot 0.
///
/// Returns `None` when the descriptor does not decode as three in-range
/// courses of `1..=`[`MAX_ROUNDS_PER_COURSE`] rounds each, which is what
/// keeps the fixed offsets honest on an entry that is not this one.
///
/// PORT: FUN_801d1510 (the table walk; the formation store is the host's)
pub fn parse_course_ladder(overlay_0977: &[u8]) -> Option<Vec<DomeCourse>> {
    let read_u32 = |at: usize| -> Option<u32> {
        overlay_0977
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let mut out = Vec::with_capacity(COURSE_COUNT);
    for course in 0..COURSE_COUNT {
        let at = COURSE_TABLE_FILE_OFFSET + course * COURSE_DESCRIPTOR_STRIDE;
        let count = read_u32(at)? as usize;
        let first = read_u32(at + 4)?;
        if count == 0 || count > MAX_ROUNDS_PER_COURSE {
            return None;
        }
        let base = first.checked_sub(ARENA_OVERLAY_BASE_VA)? as usize;
        let mut rounds = Vec::with_capacity(count);
        for r in 0..count {
            let rec = base + r * ROUND_RECORD_STRIDE;
            let label_va = read_u32(rec)?;
            let monster_id = read_u32(rec + 4)?;
            // Retail takes the byte, not the word (`lbu ... 4(v0)`).
            if monster_id > 0xFF || monster_id == 0 {
                return None;
            }
            rounds.push(DomeRound {
                label_va,
                monster_id: monster_id as u8,
            });
        }
        out.push(DomeCourse { rounds });
    }
    Some(out)
}

/// The score cell a cleared `(course, round)` adds to the running tally.
///
/// `round` is 1-based, matching retail's `DAT_801D1860 + course * 0x40 +
/// (round - 1) * 4`. Returns `None` outside the table.
pub fn course_score_cell(overlay_0977: &[u8], course: usize, round: u32) -> Option<i32> {
    if course >= COURSE_COUNT || round == 0 || round as usize > MAX_ROUNDS_PER_COURSE {
        return None;
    }
    let at =
        SCORE_TABLE_FILE_OFFSET + course * SCORE_TABLE_COURSE_STRIDE + (round as usize - 1) * 4;
    overlay_0977
        .get(at..at + 4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

/// Outcome of the arena contest settlement kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContestSettlement {
    /// The score tally (`_DAT_80084440`) after settlement.
    pub score: i32,
    /// The continue latch (`DAT_801d1adc`) after settlement.
    pub continuing: bool,
    /// The one-shot prize item is awarded this settlement
    /// (`FUN_800421D4(0xCD, 1)`).
    pub award_prize: bool,
}

/// Arena contest settlement - the score/prize half of the minigame
/// completion routine in the arena roster/init overlay (PROT 0977 at
/// slot-A base `0x801CE818`, file `+0x2748`).
///
/// Retail runs this after a contest leg: it restores the SC block, then
/// settles the running score tally and, exactly once per save, awards the
/// Master-course first-clear prize. The decision order is:
///
/// 1. Not continuing -> the tally is halved (signed `/ 2`); continuing
///    keeps it intact.
/// 2. A finished contest (`contest_over`) zeroes the tally and drops the
///    continue latch.
/// 3. A still-live continue adds the per-`(course, round)` score-table
///    entry (`DAT_801d1860 + course*0x40 + (round-1)*4`) and, when the
///    round counter has reached the Master-course final fight and the
///    one-shot flag `0x6CB` is still clear, awards item `0xCD` (the War
///    God Icon).
///
/// `score_table_entry` is the caller-resolved `DAT_801d1860` cell for
/// `(course, round)`; `prize_already_awarded` is the `0x6CB` flag-bank
/// bit.
///
// NOT WIRED: **one** of its six inputs is the blocker, and it is now the only
// one. Four have arrived since this note was first written:
//
// - `score_table_entry` is [`course_score_cell`], reading the same raw PROT
//   0977 entry the ladder comes off.
// - `round` and the course index are real: [`parse_course_ladder`] decodes
//   the three courses and their 8 / 8 / 13 rounds, and the play window
//   already walks one of them.
// - `prize_already_awarded` is bit `0x6CB` of the system-flag bank the engine
//   does model; [`crate::prize_exchange`] gates its own availability on the
//   same bank.
//
// What is left is `continuing`, and it gates every arm. It is the player's
// *choice* to keep going, latched in retail at `DAT_801D1ADC` behind a
// continue prompt, and no host offers one - a leg simply ends. Passing "the
// player won" for it would halve or keep the tally on a decision retail never
// asked for, which is a rule, not plumbing. The prompt is the work, and the
// arm that sets the latch (inside `FUN_801D0CD4` / `FUN_801D0068`) is not yet
// walked.
/// PORT: FUN_801d0f60
pub fn settle_contest(
    score: i32,
    continuing: bool,
    contest_over: bool,
    round: u32,
    score_table_entry: i32,
    prize_already_awarded: bool,
) -> ContestSettlement {
    // 801d1014..801d1038: halve the tally unless the continue latch is up.
    let mut score = if continuing { score } else { score / 2 };
    let mut continuing = continuing;
    // 801d1044..801d1060: a finished contest zeroes both.
    if contest_over {
        continuing = false;
        score = 0;
    }
    // 801d10d4..801d1144: live continue -> add the score-table cell; the
    // prize is gated on the Master-course final fight + the one-shot flag.
    let mut award_prize = false;
    if continuing {
        score += score_table_entry;
        if round >= CONTEST_PRIZE_ROUND && !prize_already_awarded {
            award_prize = true;
        }
    }
    ContestSettlement {
        score,
        continuing,
        award_prize,
    }
}

/// One animated-sprite glide record (`ctx + 0x11B4 + i*0xC`, up to 0x28
/// handles): a sprite easing from `start` to `target` over `total` frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpriteGlide {
    /// `+0x00` total frame count; `0` = slot inactive.
    pub total: u8,
    /// `+0x01` elapsed frames.
    pub elapsed: u8,
    /// `+0x04`/`+0x06` target screen position.
    pub target: (i16, i16),
    /// `+0x08`/`+0x0A` start screen position.
    pub start: (i16, i16),
}

/// One step's outcome for a glide handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlideStep {
    /// Slot inactive - nothing written.
    Idle,
    /// The step reached the target: the sprite snaps to `target` and the
    /// record deactivates (`total = 0`).
    Arrived { pos: (i16, i16) },
    /// Still in flight: linear interpolation `start + (target - start) *
    /// elapsed / total` (signed division), plus the remaining-frames count
    /// retail folds into its return (`total - elapsed + 1`).
    Moving { pos: (i16, i16), remaining: u32 },
}

impl SpriteGlide {
    /// PORT: FUN_801d9bbc (one handle's step; retail loops all 0x28 handles
    /// per frame with the frame delta from scratchpad `0x1F800393`).
    ///
    /// Arrival test is `dt >= total - elapsed` **before** accumulating;
    /// otherwise `elapsed += dt` first and the eased position uses the new
    /// elapsed count.
    pub fn step(&mut self, dt: u8) -> GlideStep {
        if self.total == 0 {
            return GlideStep::Idle;
        }
        if dt as i32 >= self.total as i32 - self.elapsed as i32 {
            self.total = 0;
            return GlideStep::Arrived { pos: self.target };
        }
        self.elapsed += dt;
        let lerp = |s: i16, t: i16| {
            let d = (t as i32 - s as i32) * self.elapsed as i32 / self.total as i32;
            (s as i32 + d) as i16
        };
        GlideStep::Moving {
            pos: (
                lerp(self.start.0, self.target.0),
                lerp(self.start.1, self.target.1),
            ),
            remaining: (self.total - self.elapsed) as u32 + 1,
        }
    }
}

/// The round time meter's counter ceiling (`0xC` ticks = a full bar).
pub const TIME_METER_MAX: u8 = 0xC;

/// PORT: FUN_801d3444 (core ramp + bar mapping) - the round **time meter**:
/// while the phase tag is `'P'` (0x50, the selection phase) and the ramp
/// flag is up, the 0..=0xC counter climbs by the frame delta (clamped at
/// [`TIME_METER_MAX`]); otherwise it drains by the delta (floored at 0).
/// The bar sprite's Y offset is `counter * 160 / 12 - 0x92` (the
/// `0x2AAAAAAB` reciprocal-multiply divide) - `-0x92` empty, `+0xE` full.
/// Returns `(new_counter, bar_y)`.
///
/// Wired: [`MuscleDomeSession::tick_time_meter`], which the host calls once a
/// frame while a contest is up.
pub fn time_meter_step(counter: u8, dt: u8, in_select_phase: bool, ramp_up: bool) -> (u8, i16) {
    let new = if ramp_up && in_select_phase {
        (counter as u32 + dt as u32).min(TIME_METER_MAX as u32) as u8
    } else {
        counter.saturating_sub(dt)
    };
    let bar_y = (new as i32 * 160 / 12 - 0x92) as i16;
    (new, bar_y)
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
    fn resolution_plays_whole_strings_and_reads_hp_left() {
        let mut s = session();
        s.commit_card(0, 0);
        s.commit_card(0, 1);
        s.ai_commit_all(1);
        s.end_selection();
        assert_eq!(s.phase(), MusclePhase::Resolve);
        s.resolve_turn(|_, _| 50);
        // Player queued 2, opponent 2: both take 100.
        assert_eq!(s.hp(0), 400);
        assert_eq!(s.hp(1), 300);
        assert_eq!(s.last_turn_damage(), [100, 100]);
        assert_eq!(s.phase(), MusclePhase::TurnOver);
        // The readout is a plain percentage (scale 100, not 0x6C), and the
        // HUD's own number is the OPPONENT's.
        assert_eq!(s.hp_left_percent(0), 400 * 100 / 500);
        assert_eq!(s.hp_left_percent(1), 300 * 100 / 400);
        assert_eq!(s.hp_left(), 75, "HUD reads slot 1 = the opponent");
        // The turn counter advanced; nothing is counting down against it.
        assert_eq!(s.turn(), 1);
        // Next turn reseeds budgets + clears queues.
        s.next_turn();
        assert_eq!(s.phase(), MusclePhase::Select);
        assert_eq!(s.budget(0), 100);
        assert!(s.queue(0).is_empty());
    }

    #[test]
    fn a_turn_plays_each_string_whole_not_interleaved() {
        // Player queues two commands, the opponent one. Interleaved play
        // would order them p0, o0, p1; a real turn is p0, p1, o0.
        let mut s = MuscleDomeSession::new(
            hand([1, 1, 1, 1]),
            hand([1, 0xFFFF, 0xFFFF, 0xFFFF]),
            [2, 1],
            [500, 500],
            0,
        );
        s.commit_card(0, 0);
        s.commit_card(0, 3);
        s.ai_commit_all(1);
        assert_eq!(s.queue(0), &[0x0C, 0x0D]);
        assert_eq!(s.queue(1), &[0x0C]);
        s.end_selection();
        let mut order = Vec::new();
        s.resolve_turn(|attacker, cmd| {
            order.push((attacker, cmd));
            1
        });
        assert_eq!(order, vec![(0, 0x0C), (0, 0x0D), (1, 0x0C)]);
    }

    #[test]
    fn dome_leg_runs_past_four_turns_and_ends_only_on_a_ko() {
        let mut s = session();
        // The opponent has 400 HP; 40 a turn needs ten turns to drop it. A
        // four-turn bound would have ended this leg at turn 4 with the
        // opponent still standing on 240 HP.
        for turn in 1..=10 {
            assert_eq!(s.phase(), MusclePhase::Select, "turn {turn} is playable");
            assert!(!s.decided(), "turn {turn}: nobody has dropped yet");
            s.commit_card(0, 0);
            s.end_selection();
            s.resolve_turn(|attacker, _| if attacker == 0 { 40 } else { 0 });
            assert_eq!(s.turn(), turn);
            if turn < 10 {
                assert_eq!(
                    s.phase(),
                    MusclePhase::TurnOver,
                    "turn {turn}: the leg continues"
                );
                s.next_turn();
            }
        }
        // Turn 10 lands the KO - the only thing that ends a leg.
        assert_eq!(s.hp(1), 0);
        assert_eq!(s.phase(), MusclePhase::Won);
        assert!(s.decided());
    }

    #[test]
    fn dome_turns_left_is_korus_hud_not_a_dome_rule() {
        // The strip's arithmetic is still decoded - as a free function keyed
        // on the battle turn counter, reachable only by the fight whose
        // formation slot 0 is the timed-fight monster id.
        assert_eq!(timed_fight_turns_left(0), 4);
        assert_eq!(timed_fight_turns_left(3), 1);
        assert_eq!(timed_fight_turns_left(4), 0);
        assert_eq!(timed_fight_turns_left(99), 0, "floored, not wrapped");
        // And the dome's own ladder can never reach that fight.
        assert_eq!(TIMED_FIGHT_MONSTER_ID, 0xB6);
    }

    #[test]
    fn retail_kernel_is_the_shared_resolution_path() {
        // No model installed: the retail path declines rather than inventing
        // a damage rule of its own.
        let mut bare = session();
        bare.commit_card(0, 0);
        bare.end_selection();
        assert!(!bare.resolve_turn_retail(), "no model, no resolution");
        assert_eq!(bare.phase(), MusclePhase::Resolve, "phase untouched");

        // With a model installed the same call drives the turn, logging each
        // play in whole-string order and advancing the rand cursor.
        let mut s = session();
        s.install_damage_model(DomeDamageModel::new(
            Vec::new(),
            [0u8; move_power::MOVE_ID_INDEX_MAP_LEN],
            None,
            [
                DomeCombatant {
                    hp_max: 500,
                    int: 60,
                    udf: 20,
                    ldf: 20,
                    element: 0,
                },
                DomeCombatant {
                    hp_max: 400,
                    int: 50,
                    udf: 15,
                    ldf: 15,
                    element: 0,
                },
            ],
            [500, 400],
            0x1234_5678,
        ));
        let seed_before = s.damage_model().unwrap().rng_seed();
        s.commit_card(0, 0);
        s.commit_card(0, 3);
        s.ai_commit_all(1);
        s.end_selection();
        assert!(s.resolve_turn_retail());
        let plays = s.last_turn_plays();
        assert_eq!(plays.len(), s.queue(0).len() + s.queue(1).len());
        let order: Vec<usize> = plays.iter().map(|p| p.attacker).collect();
        assert_eq!(
            order,
            vec![0, 0, 1, 1],
            "each string plays whole, player first"
        );
        assert_ne!(
            s.damage_model().unwrap().rng_seed(),
            seed_before,
            "the PsyQ rand cursor advanced"
        );
        // The model's HP mirror tracks the session's own HP.
        assert_eq!(plays.last().unwrap().hp_after, [s.hp(0), s.hp(1)]);
        assert_eq!(s.turn(), 1);
    }

    #[test]
    fn selection_exhausts_when_no_card_is_affordable() {
        let mut s = session();
        assert!(!s.selection_exhausted(0));
        s.commit_card(0, 0); // 30, budget 70
        s.commit_card(0, 0); // 30, budget 40
        assert!(!s.selection_exhausted(0), "a 30-cost card still fits in 40");
        s.commit_card(0, 0); // 30, budget 10
        assert!(
            s.selection_exhausted(0),
            "cheapest card is 30, budget 10: retail ends the input here"
        );
    }

    #[test]
    fn reset_selection_clears_the_queue_and_refunds_the_budget() {
        let mut s = session();
        s.commit_card(0, 0);
        s.commit_card(0, 1);
        assert_eq!(s.budget(0), 28);
        s.end_selection();
        s.reset_selection(0);
        assert_eq!(s.phase(), MusclePhase::Select);
        assert!(s.queue(0).is_empty());
        assert_eq!(s.budget(0), 100);
        assert_eq!(s.spent(0), 0);
    }

    #[test]
    fn ko_decides_the_contest_and_names_the_reward() {
        let mut s = session();
        s.commit_card(0, 0);
        s.end_selection();
        s.resolve_turn(|attacker, _| if attacker == 0 { 1000 } else { 0 });
        assert_eq!(s.phase(), MusclePhase::Won);
        assert!(s.decided());
        assert_eq!(s.reward_spell_id(), 0x83);
        assert_eq!(s.hp_left(), 0, "the opponent has nothing left");
    }

    #[test]
    fn player_ko_loses() {
        let mut s = session();
        s.ai_commit_all(1);
        s.end_selection();
        // The opponent's string still runs whole after the player's, so a
        // KO lands even though the player queued nothing this turn.
        s.resolve_turn(|attacker, _| if attacker == 1 { 1000 } else { 0 });
        assert_eq!(s.phase(), MusclePhase::Lost);
    }

    #[test]
    fn settlement_halves_the_tally_when_not_continuing() {
        // 801d102c..801d1034: signed /2, rounding toward zero.
        let s = settle_contest(101, false, false, 5, 40, false);
        assert_eq!(s.score, 50);
        assert!(!s.continuing);
        assert!(!s.award_prize);
        let s = settle_contest(-101, false, false, 5, 40, false);
        assert_eq!(s.score, -50, "MIPS srl/addu/sra idiom rounds toward zero");
    }

    #[test]
    fn settlement_adds_the_score_table_cell_on_continue() {
        let s = settle_contest(100, true, false, 5, 40, false);
        assert_eq!(s.score, 140);
        assert!(s.continuing);
        assert!(!s.award_prize, "prize gates on the Master-course final");
    }

    #[test]
    fn contest_over_zeroes_score_and_latch() {
        let s = settle_contest(100, true, true, 13, 40, false);
        assert_eq!(s.score, 0);
        assert!(!s.continuing);
        assert!(!s.award_prize, "dropped latch skips the prize branch");
    }

    #[test]
    fn glide_arrives_snaps_and_deactivates() {
        let mut g = SpriteGlide {
            total: 10,
            elapsed: 8,
            target: (100, 50),
            start: (0, 0),
        };
        // dt >= total - elapsed: snap to target, slot deactivates.
        assert_eq!(g.step(2), GlideStep::Arrived { pos: (100, 50) });
        assert_eq!(g.total, 0);
        assert_eq!(g.step(1), GlideStep::Idle);
    }

    #[test]
    fn glide_eases_linearly_with_signed_division() {
        let mut g = SpriteGlide {
            total: 10,
            elapsed: 0,
            target: (-100, 40),
            start: (0, 0),
        };
        assert_eq!(
            g.step(5),
            GlideStep::Moving {
                pos: (-50, 20),
                remaining: 6
            }
        );
        assert_eq!(g.elapsed, 5);
    }

    #[test]
    fn time_meter_ramps_in_select_phase_and_drains_otherwise() {
        // Ramp clamps at 0xC.
        assert_eq!(time_meter_step(0xB, 3, true, true), (0xC, 0xE));
        // Outside the select phase the same flags drain.
        assert_eq!(time_meter_step(5, 2, false, true).0, 3);
        // Drain floors at zero; empty bar sits at -0x92.
        assert_eq!(time_meter_step(1, 3, true, false), (0, -0x92));
    }

    #[test]
    fn prize_awards_once_at_the_master_course_final() {
        let s = settle_contest(100, true, false, 13, 40, false);
        assert!(s.award_prize);
        assert_eq!(s.score, 140);
        // One-shot: the 0x6CB flag suppresses the re-award.
        let s = settle_contest(100, true, false, 13, 40, true);
        assert!(!s.award_prize);
    }
}
