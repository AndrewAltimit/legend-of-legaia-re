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
//!
//! **A dome leg pays nothing.** What a *contest* pays is casino coins, and
//! the arithmetic is [`DomeContest`] - the ladder layer above a leg. The
//! caption table `0x801F4DFC` that the session's [`reward_banner`] decodes is
//! the shared cast-caption composer's per-character label table, resident in
//! every battle-family overlay and reached whenever anyone casts; reading it
//! as a dome payout is what put an invented Seru capture on a dome win.
//!
//! [`reward_banner`]: MuscleDomeSession::reward_banner
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

// The flag-bank primitives the contest settlement calls. The port returns the
// flag *decisions* to its caller (`ContestSettlement`) instead of writing a
// bank itself, so the addresses are references rather than ports.
// REF: FUN_8003ce08 (set a system flag - the 0x50A / 0x35 / 0x130+course arms)
// REF: FUN_8003ce34 (clear a system flag - both are cleared before settling)
// REF: FUN_8003ce64 (read a system flag - the course, length and prize gates)
// REF: FUN_800421d4 (give item - the one-shot War God Icon award)

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

impl MusclePhase {
    /// A **turn** inside an open leg just resolved - and nothing else.
    ///
    /// Retail's turn boundary is entirely inside the battle: the
    /// battle-action SM writes `ctx[6] = 0x14` (the round driver's turn-top
    /// arm) and bumps the turn counter `ctx+0x28a`, then the driver re-enters
    /// its own command cluster (`ctx+6 = 0x28`). The arena's hub state machine
    /// is not running at all - the game is in battle mode - so **no hub screen
    /// is raised between turns**. A host that puts one there is inventing a
    /// beat retail does not have.
    ///
    /// REF: FUN_801e295c (`0x801E67E8..0x801E6810`)
    pub fn ends_turn(self) -> bool {
        matches!(self, Self::TurnOver)
    }

    /// The **leg** is over - the fight ended on a KO, which is the only thing
    /// that ends one.
    ///
    /// This is the boundary the arena hub sees: the `0x5A` end-of-action scan
    /// of `FUN_801E295C` raises the battle-end signal `DAT_8007BD71 = 0xFE`
    /// (party wipe at `0x801E65D8`, cause `5`; monster wipe at `0x801E6674`,
    /// cause `0`), the exit selector routes back to arena mode `0x18`, and only
    /// then does the hub decide between another leg and settlement.
    ///
    /// REF: FUN_801e295c (`0x801E65D8`, `0x801E6674`)
    pub fn ends_leg(self) -> bool {
        matches!(self, Self::Won | Self::Lost)
    }
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
    /// The caption's Seru index (`ctx+0x269`); the captioned spell id is
    /// `REWARD_SPELL_ID_BASE + index`. Display only - see
    /// [`Self::reward_spell_id`].
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
    /// Start one leg: per-fighter deals (deck command ids + that fighter's
    /// costs), turn-budget pools (record `+0x154`), HP, and the Seru index
    /// the victory caption names (display only - a leg pays nothing).
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

    /// The spell id the victory caption names (`REWARD_SPELL_ID_BASE +
    /// ctx+0x269`, an id into the shared spell-name table's player Seru-magic
    /// block).
    ///
    /// This is a **caption** input, not a payout: nothing in the arena
    /// overlay grants the named Seru, and a contest's real reward is coins
    /// ([`DomeContest::settle`]). Hosts display it; they must not credit it.
    pub fn reward_spell_id(&self) -> u8 {
        REWARD_SPELL_ID_BASE.wrapping_add(self.reward_seru_index)
    }

    /// The three-part **cast caption** as indices - the host resolves the
    /// strings.
    ///
    /// The table this indexes (`0x801F4DFC`) is the shared battle-family
    /// per-character label table, byte-identical across the battle-action,
    /// magic-capture, magic-level-up and dome overlays. It captions a cast;
    /// it does not describe a dome prize. Keep it for display and take the
    /// contest's payout from [`DomeContest::settle`].
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

    /// Resolve the turn the way **both** hosts must: through the retail
    /// kernel when disc tables are installed, and otherwise by closing the
    /// turn with zero damage so the leg still advances.
    ///
    /// The fallback is a rule, not plumbing: without it a host that never
    /// installed a [`DomeDamageModel`] leaves the session parked in
    /// [`MusclePhase::Resolve`] with nothing able to move it, which is a hang
    /// rather than a degraded contest. It lives here so neither host can have
    /// it and the other not.
    ///
    /// Returns whether the retail kernel drove the turn.
    pub fn resolve_turn_or_zero(&mut self) -> bool {
        if self.resolve_turn_retail() {
            return true;
        }
        self.resolve_turn(|_, _| 0);
        false
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

/// The score cell table's rounds-per-course capacity, as a decoded row.
pub type ScoreRow = [i32; MAX_ROUNDS_PER_COURSE];

/// Decode the arena's per-`(course, round)` score table out of a raw PROT
/// 0977 entry - the same `DAT_801D1860` rows [`course_score_cell`] indexes,
/// lifted whole so a running contest carries its own copy.
///
/// Returns `None` when the table does not lie inside the entry.
pub fn parse_score_table(overlay_0977: &[u8]) -> Option<[ScoreRow; COURSE_COUNT]> {
    let mut out = [[0i32; MAX_ROUNDS_PER_COURSE]; COURSE_COUNT];
    for (course, row) in out.iter_mut().enumerate() {
        for (r, cell) in row.iter_mut().enumerate() {
            let at = SCORE_TABLE_FILE_OFFSET + course * SCORE_TABLE_COURSE_STRIDE + r * 4;
            let b = overlay_0977.get(at..at + 4)?;
            *cell = i32::from_le_bytes(b.try_into().unwrap());
        }
    }
    Some(out)
}

// --- The contest: the ladder run that sits above a single leg ---------------

/// Bit of the party-standing byte `DAT_8007BD60` the arena reads to decide a
/// leg was survived (`0x801CEDD8` / `0x801CEE1C`: `lbu` then `andi 0x80`).
///
/// It is neither of the two arms the subsystem doc used to guess at. The
/// battle's own state-`0x5A` party-wipe scan clears it, and the shared
/// minigame-exit routine `FUN_80026018` re-raises it (`ori 0x80`) on the way
/// back out - so on arena re-entry the bit reads "the party is still
/// standing".
pub const PARTY_STANDING_BIT: u8 = 0x80;

/// Leg-outcome code (`_DAT_80084448`) meaning the fighter **ran**. The battle
/// SM writes it from its flee arm (`0x801D3288`), and it is the one code the
/// arena treats as giving the contest up.
pub const LEG_OUTCOME_RAN: u32 = 4;

/// The leg-outcome scoring table `DAT_801D1A5C`, indexed by
/// `min(outcome, 3)`.
pub const LEG_OUTCOME_TABLE: [i32; 4] = [8, 12, 4, 2];

/// Cap on the turns-taken scoring lane (`slti a2, 9` then `a2 = 8`).
pub const TURNS_LANE_CAP: u32 = 8;

/// Divisor every `× max_hp` scoring lane is scaled by (retail's `0x51EB851F`
/// reciprocal multiply).
pub const SCORE_LANE_DIVISOR: i32 = 100;

/// Story flag whose *absence* stops the Master course at round 8
/// (`0x801CED44`).
pub const MASTER_GATE_FLAG_8: u16 = 0x378;

/// Story flag whose absence stops the Master course at round 11
/// (`0x801CED6C`).
pub const MASTER_GATE_FLAG_11: u16 = 0x382;

/// Story flag whose absence stops the Master course at round 12
/// (`0x801CED94`).
pub const MASTER_GATE_FLAG_12: u16 = 0x471;

/// Course index whose length the three gates above clamp. The clamp block is
/// entered only on `course == 2` (`0x801CED28`: `bne v1, 2`), so the Beginner
/// and Expert courses always run their declared 8 rounds.
pub const MASTER_COURSE: usize = 2;

/// The `(round threshold, story flag)` pairs the Master course's length is
/// clamped by, in retail's own order - a later pair that fires overwrites an
/// earlier one, which is why the order is part of the rule.
pub const MASTER_LENGTH_GATES: [(u32, u16); 3] = [
    (8, MASTER_GATE_FLAG_8),
    (11, MASTER_GATE_FLAG_11),
    (12, MASTER_GATE_FLAG_12),
];

/// Story flags that pick which course the arena opens on, with the sub-id
/// word each seeds (`0x801CEB88` / `0x801CEBA8` / `0x801CEBBC`). Retail tests
/// all three in order and lets the last one that is set win.
pub const COURSE_UNLOCK_FLAGS: [(u16, u32); COURSE_COUNT] =
    [(0x536, 0x101), (0x537, 0x111), (0x538, 0x321)];

/// The sub-id word a contest opens on with none of [`COURSE_UNLOCK_FLAGS`]
/// set: course 0, round 0.
pub const CONTEST_ENTRY_WORD_DEFAULT: u32 = 1;

/// Story flag retail sets on a settled contest the player is still running
/// (`FUN_8003CE08(0x50A)`), cleared at the top of every settlement.
pub const CONTEST_CONTINUE_FLAG: u16 = 0x50A;

/// Story flag retail sets when the contest ended because the fighter ran
/// (`FUN_8003CE08(0x35)`), cleared at the top of every settlement.
pub const CONTEST_GAVE_UP_FLAG: u16 = 0x35;

/// Base of the three "ran from this course's first fight" flags
/// (`0x130 + course`), set only when the give-up landed on round 1. Curated
/// lore knows the same three as the Muscle Paradise / Chicken King trigger.
pub const COURSE_RAN_FIRST_FLAG_BASE: u16 = 0x130;

/// The round a give-up has to land on for [`COURSE_RAN_FIRST_FLAG_BASE`] to
/// be set (`beq a0, 1` at `0x801D1070`).
pub const COURSE_RAN_FIRST_ROUND: u32 = 1;

/// Ceiling the casino coin bank saturates at when a contest pays out.
pub const COIN_BANK_MAX: i32 = legaia_engine_vm::baka_hub_actors::COIN_BANK_MAX;

/// Decode the course index out of the mode-24 sub-id word `_DAT_8007BAC0`:
/// `((word - 1) & 0xFF) >> 4`.
///
/// PORT: FUN_801cea6c (`0x801CEBD4`, and again at `0x801CEC30`)
pub fn cursor_course(word: u32) -> usize {
    ((word.wrapping_sub(1) & 0xFF) >> 4) as usize
}

/// Decode the round index out of the same word: `(word - 1) & 0xF`.
///
/// PORT: FUN_801cea6c (`0x801CEC18`)
pub fn cursor_round(word: u32) -> u32 {
    word.wrapping_sub(1) & 0xF
}

/// Advance the word one leg. Retail's arena init does this - and only this -
/// when it is re-entered with the word already non-zero, which is what makes
/// "finished a leg" and "advanced the ladder" the same event.
///
/// PORT: FUN_801cea6c (`0x801CEC00`)
pub fn cursor_next_leg(word: u32) -> u32 {
    word.wrapping_add(1)
}

/// Re-pack `(course, round)` into the word's low byte, leaving every higher
/// byte alone. The hub does this at the end of all but its settle state, so
/// the word's high bytes survive a whole contest untouched - which is why the
/// unlock seeds can carry `0x100` / `0x300` in them and still decode to
/// course 0 / 2.
///
/// PORT: FUN_801cf870 (`0x801D00B8..0x801D00E4`)
/// NOT WIRED: retail re-derives `(course, round)` from the word every frame
/// because the word is its only storage. [`DomeContest`] holds both as fields
/// and advances with [`cursor_next_leg`], so nothing in the port needs the
/// repack. It stays as the inverse of [`cursor_course`] / [`cursor_round`],
/// and as the proof that the high bytes survive a contest - wiring it would
/// mean re-introducing retail's packed storage for its own sake.
pub fn cursor_repack(word: u32, course: usize, round: u32) -> u32 {
    (word & !0xFF).wrapping_add(1) + ((course as u32) << 4) + round
}

/// The story-flag reads a contest needs, sampled by the host once.
///
/// Sampling rather than calling back keeps the rules kernel free of a flag
/// bank and lets both hosts - one of which is a `wasm_bindgen` boundary -
/// hand the same shape in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContestFlags {
    /// [`COURSE_UNLOCK_FLAGS`] in order (`0x536`, `0x537`, `0x538`).
    pub course_unlock: [bool; COURSE_COUNT],
    /// [`MASTER_LENGTH_GATES`] in order (`0x378`, `0x382`, `0x471`).
    pub master_gates: [bool; 3],
    /// The one-shot prize latch [`CONTEST_PRIZE_FLAG`] (`0x6CB`).
    pub prize_awarded: bool,
}

/// The sub-id word a fresh contest opens on, given the unlock flags. Retail
/// seeds `1` and then lets each set flag overwrite it in turn, so the highest
/// unlocked course wins.
///
/// PORT: FUN_801cea6c (`0x801CEB88..0x801CEBC8`)
pub fn contest_entry_word(flags: &ContestFlags) -> u32 {
    let mut word = CONTEST_ENTRY_WORD_DEFAULT;
    for (i, &(_, seed)) in COURSE_UNLOCK_FLAGS.iter().enumerate() {
        if flags.course_unlock[i] {
            word = seed;
        }
    }
    word
}

/// How many rounds `course` runs before it is exhausted.
///
/// `declared` is the course descriptor's own count ([`parse_course_ladder`]).
/// Only [`MASTER_COURSE`] is clamped, and each gate is *considered* only once
/// the run has actually reached its threshold - so the answer depends on the
/// round as well as on the flags. Retail applies the three gates in order and
/// lets a later one overwrite an earlier one, which can raise the cap again;
/// that is reproduced rather than tidied.
///
/// PORT: FUN_801cea6c (`0x801CED28..0x801CEDA4`)
pub fn course_length(course: usize, declared: u32, round: u32, flags: &ContestFlags) -> u32 {
    if course != MASTER_COURSE {
        return declared;
    }
    let mut cap = declared;
    for (i, &(threshold, _)) in MASTER_LENGTH_GATES.iter().enumerate() {
        if round >= threshold && !flags.master_gates[i] {
            cap = threshold;
        }
    }
    cap
}

/// What the battle handed back about the leg just fought.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegReport {
    /// `DAT_8007BD60 & `[`PARTY_STANDING_BIT`] - the party is still standing.
    pub survived: bool,
    /// `_DAT_80084448` - the battle's outcome code;
    /// [`LEG_OUTCOME_RAN`] gives the contest up.
    pub outcome: u32,
    /// `_DAT_80084444` - turns the leg took.
    pub turns_taken: u32,
}

/// The four count-up rows the between-leg screen rolls.
///
/// The first three are HP recovery, not score: they drain into the same
/// accumulator `DAT_801D1AC8` that the restore state adds to the fighter's
/// HP. Only [`Self::score_cell`] drains into the coin tally. That is what the
/// six-row tally screen holds, and it is why the scoring and the healing are
/// one mechanism rather than two.
///
/// PORT: FUN_801d1184 (the four lane values)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LegScoreRows {
    /// `round * 2 * max_hp / 100` (`DAT_801D1ACC`).
    pub round_lane: i32,
    /// `min(turns_taken, 8) * max_hp / 100` (`DAT_801D1AD0`).
    pub turns_lane: i32,
    /// `LEG_OUTCOME_TABLE[min(outcome, 3)] * max_hp / 100` (`DAT_801D1AD4`).
    pub outcome_lane: i32,
    /// The `(course, round)` score cell (`DAT_801D1AAC`) - the only row that
    /// is money.
    pub score_cell: i32,
}

impl LegScoreRows {
    /// The HP the restore state hands back: the three recovery lanes summed,
    /// which is exactly what the tally screen accumulates into
    /// `DAT_801D1AC8`.
    ///
    /// PORT: FUN_801cf074 (`0x801CF0DC` / `0x801CF150` / `0x801CF1C8`)
    pub fn hp_restore(&self) -> i32 {
        self.round_lane + self.turns_lane + self.outcome_lane
    }
}

/// Compute a finished leg's four rows. `round` is the **post-advance** round
/// index, the same one retail reads out of `DAT_801D1A94` after the arena has
/// bumped the sub-id word.
///
/// PORT: FUN_801d1184
pub fn leg_score_rows(
    round: u32,
    turns_taken: u32,
    outcome: u32,
    hp_max: u16,
    score_cell: i32,
) -> LegScoreRows {
    let hp = hp_max as i32;
    let scale = |n: i32| n * hp / SCORE_LANE_DIVISOR;
    LegScoreRows {
        round_lane: scale(round as i32 * 2),
        turns_lane: scale(turns_taken.min(TURNS_LANE_CAP) as i32),
        outcome_lane: scale(LEG_OUTCOME_TABLE[(outcome.min(3)) as usize]),
        score_cell,
    }
}

/// Apply a between-leg HP restore: `hp_cur = min(hp_max, hp_cur + amount)`.
///
/// Retail stores the sum through a halfword before comparing it, so the add
/// wraps at 16 bits and only then clamps; that is reproduced exactly rather
/// than simplified to a saturating add.
///
/// PORT: FUN_801cf870 state 0x0C (`0x801CFE7C..0x801CFEA8`)
pub fn restore_hp(hp_cur: u16, hp_max: u16, amount: i32) -> u16 {
    let sum = hp_cur.wrapping_add(amount as u16);
    if sum > hp_max { hp_max } else { sum }
}

/// Credit a settled tally into the casino coin bank, saturating at
/// [`COIN_BANK_MAX`].
///
/// The credit lives in the **shared** minigame-exit routine, not in anything
/// dome-specific: `coins += tally`, then a single `slt` against `0x0098967F`
/// clamps it. The lower clamp at zero is the port's, because the engine's
/// bank is unsigned where retail's is a signed word.
///
/// PORT: FUN_80026018 (`0x80026058..0x80026078`)
pub fn credit_casino_coins(coins: u32, tally: i32) -> u32 {
    (coins as i32).saturating_add(tally).clamp(0, COIN_BANK_MAX) as u32
}

/// Where the contest hub is between legs. The values are retail's own hub
/// state ids (`DAT_801D1A78`, dispatched through the 51-entry jump table at
/// `0x801CE990`); the states the jump table routes to its default arm are
/// presentation and have no rule to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ContestState {
    /// `0x14` - the next leg is staged and fightable.
    Fight = 0x14,
    /// `0x0A` - the finished leg's rows are computed and ramping in.
    LegScore = 0x0A,
    /// `0x0B` - the rows are draining into the accumulators.
    Tally = 0x0B,
    /// `0x0C` - the accumulated recovery is being added to the fighter's HP.
    Restore = 0x0C,
    /// `0x32` - the contest is finished and settles.
    Settle = 0x32,
    /// The port's own terminal: [`DomeContest::settle`] has run.
    Settled = 0xFF,
}

/// Whether **this leg boundary** raises the arena's between-legs INTERVAL +
/// score-tally screen.
///
/// Call it at a leg boundary ([`MusclePhase::ends_leg`]) with the contest
/// state left after the leg was reported. It is the one place the cadence is
/// decided, so no host can grow its own: the native window and the browser
/// dome page both read this.
///
/// Retail's hub routes a finished leg through the 51-entry jump table at
/// `0x801CE990` on `DAT_801D1A78`, and only one of the four outcomes reaches
/// the tally screen `0x0A`:
///
/// | Leg | Hub state | Screen |
/// |---|---|---|
/// | survived, course not exhausted | `0x0A` | INTERVAL + tally |
/// | survived, course exhausted | `0x32` | settlement |
/// | not survived | `0x32` | settlement |
/// | ran | `0x32` | settlement |
///
/// Both hosts drain `0x0A`..`0x0C` inside their leg report, so the state they
/// can still observe afterwards is [`ContestState::Fight`] (the ladder staged
/// another leg - the tally screen ran) or a settling / absent contest (it did
/// not). A **turn** boundary never reaches here at all, which is the point:
/// the arena hub does not run during a leg.
///
/// PORT: FUN_801cf870 (hub dispatch `0x801CF8E4`, jump table `0x801CE990`)
pub fn leg_boundary_raises_interval(after_report: Option<ContestState>) -> bool {
    matches!(after_report, Some(ContestState::Fight))
}

/// A running Muscle Dome **contest** - the ladder above a single leg.
///
/// A leg is an ordinary battle that ends on a KO ([`MuscleDomeSession`]).
/// Everything a leg does *not* decide lives here: which `(course, round)` is
/// staged, whether the run continues, what a cleared leg is worth, how much
/// HP comes back between legs, and what the settled run pays.
///
/// Retail keeps all of that in the arena roster/init overlay (PROT 0977) as a
/// second state machine above the battle's: `FUN_801CEA6C` re-enters it after
/// every leg and `FUN_801CF870` runs its hub. Both hosts drive this one
/// model, so neither can quietly grow a ladder rule of its own.
///
/// PORT: FUN_801cea6c (contest re-entry) / FUN_801cf870 (hub)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomeContest {
    word: u32,
    lengths: [u32; COURSE_COUNT],
    score: [ScoreRow; COURSE_COUNT],
    tally: i32,
    latch: bool,
    gave_up: bool,
    state: ContestState,
    rows: LegScoreRows,
    hp_restore: i32,
}

impl DomeContest {
    /// Open a contest on the course the unlock flags pick, with the course
    /// lengths and score rows the disc declares.
    ///
    /// The tally starts at zero, mirroring the arena init's own
    /// `_DAT_80084440 = 0`.
    pub fn enter(
        flags: &ContestFlags,
        lengths: [u32; COURSE_COUNT],
        score: [ScoreRow; COURSE_COUNT],
    ) -> Self {
        Self {
            word: contest_entry_word(flags),
            lengths,
            score,
            tally: 0,
            latch: false,
            gave_up: false,
            state: ContestState::Fight,
            rows: LegScoreRows::default(),
            hp_restore: 0,
        }
    }

    /// Open a contest straight off a raw PROT 0977 entry, taking both the
    /// course lengths and the score rows from the disc. Returns `None` when
    /// the entry does not decode as the arena overlay.
    pub fn from_overlay(overlay_0977: &[u8], flags: &ContestFlags) -> Option<Self> {
        let ladder = parse_course_ladder(overlay_0977)?;
        let score = parse_score_table(overlay_0977)?;
        let mut lengths = [0u32; COURSE_COUNT];
        for (i, slot) in lengths.iter_mut().enumerate() {
            *slot = ladder.get(i)?.rounds.len() as u32;
        }
        Some(Self::enter(flags, lengths, score))
    }

    /// The packed sub-id word (`_DAT_8007BAC0`), high bytes included.
    pub fn word(&self) -> u32 {
        self.word
    }

    /// The staged course.
    ///
    /// Clamped to the last course, which retail does not do: its decode can
    /// name course `0..=15` and it simply indexes the three-record descriptor
    /// table with whatever comes out. No reachable word produces one, so the
    /// clamp only turns an impossible state into a defined one.
    pub fn course(&self) -> usize {
        cursor_course(self.word).min(COURSE_COUNT - 1)
    }

    /// The staged round, `0` on the contest's first leg.
    pub fn round(&self) -> u32 {
        cursor_round(self.word)
    }

    /// The running coin tally (`_DAT_80084440`).
    pub fn tally(&self) -> i32 {
        self.tally
    }

    /// The continue latch (`DAT_801D1ADC`): the run cleared its whole course
    /// and is still standing.
    pub fn continue_latch(&self) -> bool {
        self.latch
    }

    /// The contest ended because the fighter ran (`DAT_801D1A74`).
    pub fn gave_up(&self) -> bool {
        self.gave_up
    }

    /// Where the hub is.
    pub fn state(&self) -> ContestState {
        self.state
    }

    /// Whether the contest is finished - the hub has reached settlement.
    pub fn over(&self) -> bool {
        matches!(self.state, ContestState::Settle | ContestState::Settled)
    }

    /// The finished leg's four rows, for the tally screen.
    pub fn rows(&self) -> LegScoreRows {
        self.rows
    }

    /// The HP the restore state has accumulated (`DAT_801D1AC8`).
    pub fn pending_hp_restore(&self) -> i32 {
        self.hp_restore
    }

    /// How long the staged course runs under the current flags.
    pub fn staged_course_length(&self, flags: &ContestFlags) -> u32 {
        course_length(
            self.course(),
            self.lengths[self.course()],
            self.round(),
            flags,
        )
    }

    /// The score cell a cleared `(course, round)` is worth.
    fn cell(&self, course: usize, round: u32) -> i32 {
        if round == 0 {
            return 0;
        }
        self.score
            .get(course)
            .and_then(|row| row.get(round as usize - 1))
            .copied()
            .unwrap_or(0)
    }

    /// Report a finished leg. This is the arena's own re-entry: the sub-id
    /// word advances one leg, the new `(course, round)` decodes out of it, and
    /// the hub picks between carrying on and settling.
    ///
    /// `hp_max` is the fighter's maximum HP, which every recovery lane scales
    /// by.
    ///
    /// PORT: FUN_801cea6c (`0x801CEC00`, `0x801CEDB8..0x801CEE8C`)
    pub fn finish_leg(&mut self, report: LegReport, hp_max: u16, flags: &ContestFlags) {
        // 0x801CEC00: a re-entered arena advances the ladder by one.
        self.word = cursor_next_leg(self.word);
        // 0x801CECE0: every arena entry drops the continue latch first.
        self.latch = false;
        let course = self.course();
        let round = self.round();
        self.rows = leg_score_rows(
            round,
            report.turns_taken,
            report.outcome,
            hp_max,
            self.cell(course, round),
        );
        let exhausted = round >= course_length(course, self.lengths[course], round, flags);
        // 0x801CEE44: the run/give-up code overrides whatever the arms above
        // decided, and is the one path that voids the tally outright.
        if report.outcome == LEG_OUTCOME_RAN {
            self.gave_up = true;
            self.state = ContestState::Settle;
        } else if exhausted {
            // 0x801CEDD8: only a survived, exhausted course raises the latch.
            self.latch = report.survived;
            self.state = ContestState::Settle;
        } else if report.survived {
            self.state = ContestState::LegScore;
        } else {
            self.state = ContestState::Settle;
        }
    }

    /// Step the between-leg hub one state: rows in, rows drained, HP restored,
    /// next leg staged. A host that has no tally screen can call it three
    /// times in a row; one that does can call it as each screen finishes.
    ///
    /// Returns the state it moved to. No-op once the hub is settling.
    ///
    /// PORT: FUN_801cf870 states 0x0A / 0x0B / 0x0C
    pub fn advance(&mut self) -> ContestState {
        self.state = match self.state {
            ContestState::LegScore => ContestState::Tally,
            ContestState::Tally => {
                // The three recovery lanes accumulate; the score cell is the
                // only row that reaches the coin tally.
                self.hp_restore += self.rows.hp_restore();
                self.tally += self.rows.score_cell;
                ContestState::Restore
            }
            ContestState::Restore => ContestState::Fight,
            other => other,
        };
        self.state
    }

    /// Take the accumulated between-leg HP restore and apply it to a fighter,
    /// clearing the accumulator. Returns the fighter's new current HP.
    ///
    /// PORT: FUN_801cf870 state 0x0C (`0x801CFE7C..0x801CFEA8`)
    pub fn take_hp_restore(&mut self, hp_cur: u16, hp_max: u16) -> u16 {
        let amount = std::mem::take(&mut self.hp_restore);
        restore_hp(hp_cur, hp_max, amount)
    }

    /// Settle the contest: halve or keep the tally, add the final leg's cell,
    /// and decide the one-shot prize. Idempotent - a second call is a no-op
    /// that returns the same settled tally.
    ///
    /// The caller pays the returned [`ContestSettlement::score`] into the coin
    /// bank with [`credit_casino_coins`] and applies the flags it names.
    ///
    /// PORT: FUN_801d0f60
    pub fn settle(&mut self, flags: &ContestFlags) -> ContestSettlement {
        if self.state == ContestState::Settled {
            return ContestSettlement {
                score: self.tally,
                continuing: self.latch,
                award_prize: false,
                set_continue_flag: false,
                set_gave_up_flag: false,
                set_ran_first_flag: None,
            };
        }
        let course = self.course();
        let round = self.round();
        let out = settle_contest(
            self.tally,
            self.latch,
            self.gave_up,
            course,
            round,
            self.cell(course, round),
            flags.prize_awarded,
        );
        self.tally = out.score;
        self.latch = out.continuing;
        self.state = ContestState::Settled;
        out
    }
}

/// Outcome of the arena contest settlement kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContestSettlement {
    /// The score tally (`_DAT_80084440`) after settlement - the amount
    /// [`credit_casino_coins`] pays into the coin bank.
    pub score: i32,
    /// The continue latch (`DAT_801d1adc`) after settlement.
    pub continuing: bool,
    /// The one-shot prize item is awarded this settlement
    /// (`FUN_800421D4(0xCD, 1)`).
    pub award_prize: bool,
    /// Set [`CONTEST_CONTINUE_FLAG`] (`FUN_8003CE08(0x50A)`).
    pub set_continue_flag: bool,
    /// Set [`CONTEST_GAVE_UP_FLAG`] (`FUN_8003CE08(0x35)`).
    pub set_gave_up_flag: bool,
    /// Set this `0x130 + course` flag - the fighter ran from the course's
    /// first fight. Curated lore knows the same three as the Muscle Paradise
    /// "run from the first battle in all three difficulties" trigger, which
    /// is what pins them.
    pub set_ran_first_flag: Option<u16>,
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
/// 2. A given-up contest (`gave_up`) zeroes the tally and drops the
///    continue latch.
/// 3. A still-live continue adds the per-`(course, round)` score-table
///    entry (`DAT_801d1860 + course*0x40 + (round-1)*4`) and, when the
///    round counter has reached the Master-course final fight and the
///    one-shot flag `0x6CB` is still clear, awards item `0xCD` (the War
///    God Icon).
///
/// `continuing` is the latch [`DomeContest::finish_leg`] raises: it is not a
/// prompt the player answers but a **derived** fact - the course was run to
/// its end and the party is still standing (`DAT_801D1ADC` has exactly three
/// writers, and the only one that raises it sits behind those two tests).
/// `gave_up` is `DAT_801D1A74`, raised only when the leg's outcome code was
/// [`LEG_OUTCOME_RAN`]. `score_table_entry` is the caller-resolved
/// `DAT_801d1860` cell for `(course, round)`; `prize_already_awarded` is the
/// `0x6CB` flag-bank bit.
///
/// Wired: [`DomeContest::settle`], which both hosts reach when a contest
/// ends.
///
/// PORT: FUN_801d0f60
pub fn settle_contest(
    score: i32,
    continuing: bool,
    gave_up: bool,
    course: usize,
    round: u32,
    score_table_entry: i32,
    prize_already_awarded: bool,
) -> ContestSettlement {
    // 801d1014..801d1038: halve the tally unless the continue latch is up.
    // The live latch is also what sets the `0x50A` flag.
    let set_continue_flag = continuing;
    let mut score = if continuing { score } else { score / 2 };
    let mut continuing = continuing;
    // 801d1044..801d1060: a given-up contest zeroes both.
    let mut set_ran_first_flag = None;
    if gave_up {
        continuing = false;
        score = 0;
        // 801d1064..801d10c8: running from a course's *first* fight latches
        // that course's own flag.
        if round == COURSE_RAN_FIRST_ROUND && course < COURSE_COUNT {
            set_ran_first_flag = Some(COURSE_RAN_FIRST_FLAG_BASE + course as u16);
        }
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
        set_continue_flag,
        set_gave_up_flag: gave_up,
        set_ran_first_flag,
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
        let s = settle_contest(101, false, false, 0, 5, 40, false);
        assert_eq!(s.score, 50);
        assert!(!s.continuing);
        assert!(!s.award_prize);
        let s = settle_contest(-101, false, false, 0, 5, 40, false);
        assert_eq!(s.score, -50, "MIPS srl/addu/sra idiom rounds toward zero");
    }

    #[test]
    fn settlement_adds_the_score_table_cell_on_continue() {
        let s = settle_contest(100, true, false, 0, 5, 40, false);
        assert_eq!(s.score, 140);
        assert!(s.continuing);
        assert!(!s.award_prize, "prize gates on the Master-course final");
    }

    #[test]
    fn contest_over_zeroes_score_and_latch() {
        let s = settle_contest(100, true, true, 2, 13, 40, false);
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

    // --- the contest layer ------------------------------------------------

    /// Synthetic score rows shaped like the ladder's (8 / 8 / 13 populated
    /// cells) but not its values - the disc's own numbers are read off the
    /// disc by `tests/muscle_contest_real.rs`, which is where they belong.
    fn score_rows() -> [ScoreRow; COURSE_COUNT] {
        let mut s = [[0i32; MAX_ROUNDS_PER_COURSE]; COURSE_COUNT];
        let lens = [8usize, 8, 13];
        for (c, row) in s.iter_mut().enumerate() {
            for (r, cell) in row.iter_mut().enumerate().take(lens[c]) {
                *cell = (c as i32 + 1) * 10 + r as i32;
            }
        }
        s
    }

    fn all_gates() -> ContestFlags {
        ContestFlags {
            course_unlock: [false; 3],
            master_gates: [true; 3],
            prize_awarded: false,
        }
    }

    fn contest(flags: &ContestFlags) -> DomeContest {
        DomeContest::enter(flags, [8, 8, 13], score_rows())
    }

    fn cleared(turns: u32) -> LegReport {
        LegReport {
            survived: true,
            outcome: 0,
            turns_taken: turns,
        }
    }

    #[test]
    fn the_sub_id_word_carries_course_and_round_in_its_low_byte() {
        // The three unlock seeds, decoded.
        assert_eq!((cursor_course(0x001), cursor_round(0x001)), (0, 0));
        assert_eq!((cursor_course(0x101), cursor_round(0x101)), (0, 0));
        assert_eq!((cursor_course(0x111), cursor_round(0x111)), (1, 0));
        assert_eq!((cursor_course(0x321), cursor_round(0x321)), (2, 0));
        // A leg advance is +1, and the round walks with it.
        let mut w = 0x321;
        for round in 1..=5 {
            w = cursor_next_leg(w);
            assert_eq!((cursor_course(w), cursor_round(w)), (2, round));
        }
        // The repack rewrites only the low byte - the high bytes are what
        // let 0x321 mean "course 2" and survive a whole contest.
        assert_eq!(cursor_repack(0x321, 2, 5), 0x326);
        assert_eq!(cursor_repack(0x326, 2, 5) & !0xFF, 0x300);
    }

    #[test]
    fn the_entry_word_takes_the_highest_unlocked_course() {
        let mut f = ContestFlags::default();
        assert_eq!(contest_entry_word(&f), CONTEST_ENTRY_WORD_DEFAULT);
        f.course_unlock = [true, false, false];
        assert_eq!(cursor_course(contest_entry_word(&f)), 0);
        f.course_unlock = [true, true, false];
        assert_eq!(cursor_course(contest_entry_word(&f)), 1);
        // Retail tests all three in order and lets the last set one win.
        f.course_unlock = [true, true, true];
        assert_eq!(cursor_course(contest_entry_word(&f)), 2);
        f.course_unlock = [false, false, true];
        assert_eq!(cursor_course(contest_entry_word(&f)), 2);
    }

    #[test]
    fn only_the_master_course_is_story_gated() {
        let mut f = all_gates();
        // Beginner / Expert never clamp, whatever the flags say.
        f.master_gates = [false; 3];
        assert_eq!(course_length(0, 8, 7, &f), 8);
        assert_eq!(course_length(1, 8, 7, &f), 8);
        // Master clamps, but only once the run has reached the threshold -
        // the gate below the round you are on is not consulted.
        assert_eq!(course_length(2, 13, 7, &f), 13, "round 7 is under gate 8");
        assert_eq!(course_length(2, 13, 8, &f), 8);
        f.master_gates = [true, false, false];
        assert_eq!(course_length(2, 13, 8, &f), 13);
        assert_eq!(course_length(2, 13, 11, &f), 11);
        f.master_gates = [true, true, false];
        assert_eq!(course_length(2, 13, 11, &f), 13);
        assert_eq!(course_length(2, 13, 12, &f), 12);
        f.master_gates = [true; 3];
        assert_eq!(course_length(2, 13, 12, &f), 13, "all gates open: full 13");
    }

    #[test]
    fn the_four_lanes_scale_by_max_hp_except_the_money_one() {
        // 500 HP: each lane is `n * 500 / 100` = `n * 5`.
        let r = leg_score_rows(3, 5, 1, 500, 40);
        assert_eq!(r.round_lane, 3 * 2 * 5);
        assert_eq!(r.turns_lane, 5 * 5);
        assert_eq!(r.outcome_lane, LEG_OUTCOME_TABLE[1] * 5);
        assert_eq!(r.score_cell, 40, "the score cell is not scaled");
        // The turns lane caps at 8.
        assert_eq!(leg_score_rows(0, 99, 0, 500, 0).turns_lane, 8 * 5);
        // The outcome index saturates at 3.
        assert_eq!(
            leg_score_rows(0, 0, 9, 500, 0).outcome_lane,
            LEG_OUTCOME_TABLE[3] * 5
        );
        // The three recovery lanes are what the restore state hands back;
        // the money row is not part of it.
        assert_eq!(r.hp_restore(), r.round_lane + r.turns_lane + r.outcome_lane);
    }

    #[test]
    fn a_cleared_leg_advances_the_ladder_scores_and_heals() {
        let f = all_gates();
        let mut c = contest(&f);
        assert_eq!((c.course(), c.round()), (0, 0));
        assert_eq!(c.state(), ContestState::Fight);

        c.finish_leg(cleared(4), 500, &f);
        // The ladder advanced and the cleared leg's cell is row 0 cell 0.
        assert_eq!((c.course(), c.round()), (0, 1));
        assert_eq!(c.state(), ContestState::LegScore);
        let cell0 = score_rows()[0][0];
        assert_eq!(c.rows().score_cell, cell0);
        assert_eq!(c.tally(), 0, "nothing banks until the tally drains");

        assert_eq!(c.advance(), ContestState::Tally);
        assert_eq!(c.advance(), ContestState::Restore);
        assert_eq!(c.tally(), cell0);
        assert!(c.pending_hp_restore() > 0);
        // The restore is capped by max HP.
        assert_eq!(c.take_hp_restore(499, 500), 500);
        assert_eq!(c.pending_hp_restore(), 0);
        assert_eq!(c.advance(), ContestState::Fight);
        assert!(!c.over());
    }

    #[test]
    fn a_cleared_course_pays_the_whole_row_and_a_lost_one_pays_half() {
        let f = all_gates();
        // Run the Beginner course to its end.
        let mut c = contest(&f);
        for _ in 0..8 {
            c.finish_leg(cleared(3), 400, &f);
            if c.over() {
                break;
            }
            c.advance();
            c.advance();
            c.advance();
        }
        assert_eq!(c.round(), 8, "eight legs cleared");
        assert!(c.over());
        assert!(c.continue_latch(), "course run out, party standing");
        let out = c.settle(&f);
        // Cells 1..=7 banked through the tally screen, cell 8 added at
        // settlement: the whole row, which is what the curated table calls
        // the course's reward.
        assert_eq!(out.score, score_rows()[0][..8].iter().sum::<i32>());
        assert!(out.set_continue_flag);
        assert!(!out.set_gave_up_flag);

        // The same run, lost on the last leg: no latch, tally halved.
        let mut c = contest(&f);
        for leg in 0..8 {
            let survived = leg < 7;
            c.finish_leg(
                LegReport {
                    survived,
                    outcome: 0,
                    turns_taken: 3,
                },
                400,
                &f,
            );
            if c.over() {
                break;
            }
            c.advance();
            c.advance();
            c.advance();
        }
        assert!(!c.continue_latch());
        let banked: i32 = score_rows()[0][..7].iter().sum();
        assert_eq!(c.settle(&f).score, banked / 2);
    }

    #[test]
    fn running_voids_the_tally_and_latches_the_courses_own_flag() {
        let f = all_gates();
        let mut c = contest(&f);
        // Bank a leg first, so there is something to void.
        c.finish_leg(cleared(3), 400, &f);
        c.advance();
        c.advance();
        c.advance();
        assert_eq!(c.tally(), score_rows()[0][0]);
        // Now run from the second fight.
        c.finish_leg(
            LegReport {
                survived: true,
                outcome: LEG_OUTCOME_RAN,
                turns_taken: 1,
            },
            400,
            &f,
        );
        assert!(c.gave_up());
        assert!(c.over());
        let out = c.settle(&f);
        assert_eq!(out.score, 0, "a give-up pays nothing");
        assert!(out.set_gave_up_flag);
        // Round 2, not 1: the Muscle Paradise flag only latches on the
        // course's first fight.
        assert_eq!(out.set_ran_first_flag, None);

        // Running from the very first fight does latch it.
        let mut c = contest(&f);
        c.finish_leg(
            LegReport {
                survived: true,
                outcome: LEG_OUTCOME_RAN,
                turns_taken: 1,
            },
            400,
            &f,
        );
        assert_eq!(
            c.settle(&f).set_ran_first_flag,
            Some(COURSE_RAN_FIRST_FLAG_BASE)
        );
    }

    #[test]
    fn a_story_gated_master_course_ends_early_at_its_cap() {
        // No gate flags: the Master course stops at round 8 rather than 13,
        // so the run settles eight legs in with the latch up.
        let f = ContestFlags {
            course_unlock: [false, false, true],
            master_gates: [false; 3],
            prize_awarded: false,
        };
        let mut c = contest(&f);
        assert_eq!(c.course(), 2);
        for _ in 0..13 {
            c.finish_leg(cleared(2), 600, &f);
            if c.over() {
                break;
            }
            c.advance();
            c.advance();
            c.advance();
        }
        assert_eq!(c.round(), 8, "clamped to 8 by the missing 0x378 flag");
        assert!(c.continue_latch());
        let out = c.settle(&f);
        assert!(!out.award_prize, "the prize needs the full 13-round run");
        assert_eq!(out.score, score_rows()[2][..8].iter().sum::<i32>());
    }

    #[test]
    fn the_full_master_run_pays_the_row_sum_and_the_one_shot_prize() {
        let f = ContestFlags {
            course_unlock: [false, false, true],
            master_gates: [true; 3],
            prize_awarded: false,
        };
        let mut c = contest(&f);
        for _ in 0..13 {
            c.finish_leg(cleared(2), 600, &f);
            if c.over() {
                break;
            }
            c.advance();
            c.advance();
            c.advance();
        }
        assert_eq!(c.round(), 13);
        let out = c.settle(&f);
        assert!(out.award_prize);
        let row_sum: i32 = score_rows()[2][..13].iter().sum();
        assert_eq!(out.score, row_sum, "the whole row, every cell once");
        // Settlement is idempotent: the second call cannot pay twice.
        let again = c.settle(&f);
        assert_eq!(again.score, row_sum);
        assert!(!again.award_prize);
    }

    #[test]
    fn the_coin_credit_saturates_at_the_bank_ceiling() {
        assert_eq!(credit_casino_coins(0, 818), 818);
        assert_eq!(credit_casino_coins(100, 13830), 13930);
        assert_eq!(
            credit_casino_coins(COIN_BANK_MAX as u32, 1),
            COIN_BANK_MAX as u32
        );
        assert_eq!(credit_casino_coins(9_999_990, 100), COIN_BANK_MAX as u32);
        // The port's own lower clamp: retail's bank is a signed word, the
        // engine's is unsigned.
        assert_eq!(credit_casino_coins(10, -100), 0);
    }

    #[test]
    fn the_zero_damage_fallback_closes_the_turn_instead_of_hanging() {
        let mut s = session();
        s.commit_card(0, 0);
        s.end_selection();
        assert_eq!(s.phase(), MusclePhase::Resolve);
        // No model installed: the shared path still moves the turn on, which
        // is the difference between a degraded contest and a hang.
        assert!(!s.resolve_turn_or_zero(), "the retail kernel did not drive");
        assert_eq!(s.phase(), MusclePhase::TurnOver);
        assert_eq!(s.turn(), 1);
        assert_eq!(s.last_turn_damage(), [0, 0]);
    }

    #[test]
    fn prize_awards_once_at_the_master_course_final() {
        let s = settle_contest(100, true, false, 2, 13, 40, false);
        assert!(s.award_prize);
        assert_eq!(s.score, 140);
        // One-shot: the 0x6CB flag suppresses the re-award.
        let s = settle_contest(100, true, false, 2, 13, 40, true);
        assert!(!s.award_prize);
    }
}
