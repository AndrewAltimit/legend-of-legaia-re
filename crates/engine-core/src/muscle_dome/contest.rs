//! The contest: the ladder run that sits above a single leg, and its settlement.
//! Split out of `muscle_dome.rs`.

use super::*;

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
/// REPLACED-BY: [`DomeContest`], which holds `course` and `round` as typed
/// fields and advances them with [`cursor_next_leg`]. Retail re-derives the
/// pair from the packed word every frame because that word is its only
/// storage; the port replaced the storage, so the repack has no job left and
/// no host is owed one - wiring it would mean re-introducing retail's packed
/// cursor for its own sake. It stays as the inverse of [`cursor_course`] /
/// [`cursor_round`] and as the proof that the high bytes survive a contest.
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

/// The [`legaia_save::EquipmentSlots`] indices the arena strips when a
/// contest opens on a course above Beginner - body armour (record `+0x196`),
/// head gear (`+0x197`), the weapon byte (`+0x198`) and leg gear (`+0x19A`).
///
/// Index `3` is the Seru-lock byte `+0x199` and indices `5..=7` are the three
/// accessory bytes `+0x19B..+0x19D`; retail writes **none** of those four, so
/// a stripped fighter keeps its accessories and its summon access. The four
/// stores are `sb zero` at `0x801D0F24` / `0x801D0F28` / `0x801D0F2C` and the
/// `jal`'s delay slot `0x801D0F34`.
///
/// PORT: FUN_801d0ed8 (the gear-strip arm)
pub const CONTEST_STRIPPED_EQUIP_SLOTS: [usize; 4] = [0, 1, 2, 4];

/// What opening a contest does to the fighter's record.
///
/// Retail's arena entry decodes the course into `DAT_801D1A90` in the delay
/// slot of its `jal 0x801D0ED8` (`0x801CEBF0`), and the callee's first test
/// is `bnez` on that byte - so "no equipment" is a **course** rule, not a
/// round rule, and the Beginner course (`0`) keeps its gear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContestStartRestore {
    /// Zero the four [`CONTEST_STRIPPED_EQUIP_SLOTS`] bytes before the
    /// refill. Set for every course but Beginner.
    pub strip_gear: bool,
}

/// Apply the contest-start restore to one character record.
///
/// The body is three `(max, cur)` halfword pairs copied max-to-cur on the
/// live game-state window at `0x80084140 + 0x6CC / 0x6D0 / 0x6D4`, which is
/// the **lead** party record's `+0x104` / `+0x108` / `+0x10C` (`0x80084708 -
/// 0x80084140 = 0x5C8`): HP, MP and SP all come back full. There is no
/// per-character stride in the instruction stream, so the arena restores
/// party slot 0 and nobody else.
///
/// Retail runs the per-character stat aggregator `FUN_80042558` **between**
/// the gear strip and the refill, so the maxima the refill copies are the
/// ones recomputed under the stripped equipment. The port's equivalent
/// recompute is the caller's; this function copies whatever maxima the
/// record holds when it is called, which is why the strip happens here too
/// rather than in the host.
///
/// PORT: FUN_801d0ed8
pub fn apply_contest_start_restore(
    record: &mut legaia_save::CharacterRecord,
    restore: ContestStartRestore,
) {
    if restore.strip_gear {
        let mut eq = record.equipment();
        for &slot in CONTEST_STRIPPED_EQUIP_SLOTS.iter() {
            eq.slots[slot] = 0;
        }
        record.set_equipment(eq);
    }
    let mut hms = record.hp_mp_sp();
    hms.hp_cur = hms.hp_max;
    hms.mp_cur = hms.mp_max;
    hms.sp_cur = hms.sp_max;
    record.set_hp_mp_sp(hms);
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
    pub(super) word: u32,
    pub(super) lengths: [u32; COURSE_COUNT],
    pub(super) score: [ScoreRow; COURSE_COUNT],
    pub(super) tally: i32,
    pub(super) latch: bool,
    pub(super) gave_up: bool,
    pub(super) state: ContestState,
    pub(super) rows: LegScoreRows,
    pub(super) hp_restore: i32,
    /// The one-shot contest-start restore, pending until a host consumes it
    /// ([`DomeContest::take_start_restore`]). Retail runs it in the arena
    /// entry's **first-entry** arm only - the `_DAT_8007BAC0 == 0` side of
    /// the `bnez` at `0x801CEB58` - so a re-entered arena (every later leg)
    /// never reaches the `jal 0x801D0ED8` at `0x801CEBF0`.
    pub(super) start_restore: Option<ContestStartRestore>,
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
        let word = contest_entry_word(flags);
        Self {
            word,
            lengths,
            score,
            tally: 0,
            latch: false,
            gave_up: false,
            state: ContestState::Fight,
            rows: LegScoreRows::default(),
            hp_restore: 0,
            start_restore: Some(ContestStartRestore {
                strip_gear: cursor_course(word).min(COURSE_COUNT - 1) != 0,
            }),
        }
    }

    /// Consume the one-shot contest-start restore, if it has not run yet.
    ///
    /// Retail's arena entry decodes `(course, round)` into `DAT_801D1A90` /
    /// `DAT_801D1A94` and calls `FUN_801D0ED8` in the same breath, but only
    /// on the first entry - the re-entry arm at `0x801CEC00` jumps past it.
    /// So this returns `Some` exactly once per contest, and the caller
    /// applies it with [`apply_contest_start_restore`].
    ///
    /// PORT: FUN_801cea6c (`0x801CEBEC..0x801CEBF4`)
    pub fn take_start_restore(&mut self) -> Option<ContestStartRestore> {
        self.start_restore.take()
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

    /// Arm the between-leg tally screen's roll-up, with the coin tally as it
    /// stood *before* the leg's score cell landed in it.
    ///
    /// The hub settles a leg in one step ([`Self::advance`]) because the
    /// totals do not depend on the roll; what the roll decides is what the
    /// screen shows while it is up. So the ramp is handed out armed and the
    /// host drives it a frame at a time, ending on exactly the values the
    /// settled contest already holds.
    ///
    /// PORT: FUN_801cf074 (the arming half; the per-frame step is
    /// `crate::other_game_overlay::ScoreTallyRamp::tick`)
    pub fn tally_roll(&self) -> (crate::other_game_overlay::ScoreTallyRamp, i32) {
        (
            crate::other_game_overlay::ScoreTallyRamp::arm(self.rows),
            self.tally - self.rows.score_cell,
        )
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
    pub(super) fn cell(&self, course: usize, round: u32) -> i32 {
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
