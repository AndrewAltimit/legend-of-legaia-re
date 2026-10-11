//! The AI-side Arts command assembler.
//!
//! PORT: FUN_801F0450
//!
//! The counterpart to the player queue-builder `FUN_801EED1C`: for each party
//! slot the game is driving itself, this fills the actor's `+0x1DF..+0x1F2`
//! action stream that the attack band's strike loop then walks. It is the
//! natural producer of the multi-strike streams observed on an Evil-Medallion
//! wearer - see
//! [`docs/subsystems/battle-action.md`](../../docs/subsystems/battle-action.md).
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801f0450.txt` (928 instructions).
//!
//! ## Two arms, selected by the same pair of gates
//!
//! | `record[+0xF8] & 0x2000` | `actor[+0x16E] & 0x404` | Arm |
//! |---|---|---|
//! | set | clear | [`auto_fill_queue`] - a blind weighted draw from the character's learned-arts list |
//! | otherwise | | [`build_candidate_pool`] + [`spend_gauge`] - the AP-budgeted pool draw |
//!
//! The auto-fill arm also seeds the action itself: category `+0x1DE = 3`
//! (Attack) and a target rolled uniformly over the live monster slots, pushed
//! through the dead-target redirect
//! [`redirect_dead_target`](crate::battle_action::redirect_dead_target)
//! (`FUN_801DB124`).
//!
//! ## The art insertion tail
//!
//! The tail from `0x801F0B4C` on is [`insert_arts`]: a walk over the
//! character's art-animation bank (the `0xD0`-stride records at
//! `*(DAT_801C9360[slot] + 0x58) + 4`) that splices learned arts' arrow
//! strings over the spend loop's direction queue, paying for each out of a
//! **local copy** of the Spirit gauge `actor[+0x170]`. Its divisors are
//! `% 5` (the opening record roll), `% 7` (the Spirit gate), `% 100` (the
//! accept chance), `% 3` (the re-seed) and `% 2` (the step) - an earlier note
//! here read a `/10`; the `0x66666667` reciprocal is followed by `sra 1`, so
//! it is `/5`. The halving under record `+0xF8 & 0x800` applies to the
//! per-input Spirit cost, not to the AP gauge.
//!
//! The `0x801F696C` special-trigger store at `0x801F0518` is **not** in the
//! tail: it heads the auto-fill arm (`li v0,1` / `sw v0,0x696c(v1)` right
//! after the `+0x16E & 0x404` veto), so it is raised for every slot that arm
//! takes - see `auto_fill_party_queues` in `battle_action::dispatch`.
//!
//! # Wiring: both arms are live
//!
//! The retail caller is the action SM itself: `jal 0x801f0450` at
//! `0x801E2AB8` in `overlay_battle_action_801e295c.txt`, the **first**
//! instruction of the `ctx[+0x07] == 0x00` arm that begins at `0x801E2AB4` -
//! the arm that latches `ctx[+0x290]` into `+0x291` and stamps state `0x0A` /
//! `0x0B`, ported and live as [`battle_action`](crate::battle_action)'s
//! `Begin`. `battle_action::dispatch`'s `auto_fill_party_queues` is the port
//! of the per-slot loop, and it drives [`auto_fill_queue`] +
//! [`auto_fill_floor`] + [`roll_target_slot`] from there.
//!
//! Correcting one thing an earlier note here got wrong: the gate word is
//! **not** `BattleActionHost::character_ability_bits`. That accessor is the
//! character record's `+0xF4` word (the MP-cost discount bits the cast band
//! reads at `0x801E3D04`); the auto-fill gate reads `+0xF8`
//! (`0x801F04D4`, `lw v0,0x6c0(v1)` off the `0x80084140` base) - the *upper*
//! word of the same 64-bit accessory-passive bitfield, reached through
//! [`BattleActionHost::character_ability_bits_high`](crate::battle_action::BattleActionHost::character_ability_bits_high).
//! The learned-arts list is
//! [`BattleActionHost::learned_arts`](crate::battle_action::BattleActionHost::learned_arts)
//! (record `+0x185` count, `+0x186..` ids) and the per-character floor keys on
//! `DAT_8007BD10[slot]`, the roster character id, not the battle slot.
//!
//! **The pool arm is the player's Auto attack.** Its caller-side gate is the
//! per-fighter Auto flag `ctx[+0x266 + slot]` (`0x801F0704`), which the
//! command SM writes on the attack-mode prompt (`1` on `Auto` at
//! `0x801D17D0` or under the `Automatic` option at `0x801D164C`, `0` on
//! `Command`, `0` every frame the ring is up), and the routine runs every
//! round: the Begin confirm moves the flow to `0xFE`, whose arm writes
//! `ctx[7] = 0` (`0x801D3224`), and state `0x00` opens with this `jal`. So an
//! Auto pick does not replay the saved command string - that string is what
//! the review screen shows - it runs this arm, which rebuilds the queue.
//! `engine-core`'s `World::run_auto_attack_pool_arms` drives
//! [`build_candidate_pool`] + [`spend_gauge`] + [`insert_arts`] from
//! `begin_round_execution`, with the per-(character, weapon) command costs
//! and leading entry bytes and the four status-guard masks at `0x801F672C`
//! read off the disc at scene entry.

use crate::battle_formulas::FleeActor;

/// Character-record ability bit that selects the auto-fill arm
/// (`record[+0xF8] & 0x2000`).
pub const AUTO_FILL_ABILITY_BIT: u32 = 0x2000;

/// Actor status bits that veto the auto-fill arm (`actor[+0x16E] & 0x404`).
pub const AUTO_FILL_STATUS_VETO: u16 = 0x0404;

/// First action-queue byte offset in the actor record (`+0x1DF`).
pub const QUEUE_BASE: usize = 0x1DF;

/// Queue entries the auto-fill arm will write before it stops (`s4 < 0xF`).
pub const AUTO_FILL_QUEUE_LIMIT: usize = 0xF;

/// Constant added to a learned-art index to make an action-queue byte
/// (`addiu v0, v1, 0x1b`).
pub const ART_ACTION_BIAS: u8 = 0x1B;

/// First arts command id the pool arm scans (`s2 = 0xC`).
pub const FIRST_COMMAND: u8 = 0x0C;

/// One past the last arts command id the pool arm scans (`s2 < 0x10`).
pub const LAST_COMMAND_EXCL: u8 = 0x10;

/// Candidate-scratch bytes the pool arm clears before filling
/// (`sp+0x10`, `0x10` entries).
pub const CANDIDATE_SCRATCH: usize = 0x10;

/// Does the `record[+0xF8]` / `actor[+0x16E]` pair select the auto-fill arm?
pub const fn gate_selects_auto_fill(ability_word1: u32, status: u16) -> bool {
    ability_word1 & AUTO_FILL_ABILITY_BIT != 0 && status & AUTO_FILL_STATUS_VETO == 0
}

/// The auto-fill arm's per-character queue-byte floor.
///
/// A learned-art index strictly below the floor is discarded (the slot is
/// zeroed and re-rolled); at or above it, the byte written is
/// `index + ART_ACTION_BIAS`. Retail sets `4` in a branch delay slot and
/// overrides it to `6` only for character id `2`.
pub const fn auto_fill_floor(char_id: u8) -> u8 {
    if char_id == 2 { 6 } else { 4 }
}

/// The auto-fill arm's stop roll: retail computes `rand() / 7` with the
/// `0x92492493` reciprocal and stops when `rand()` is an exact multiple of 7.
pub const fn auto_fill_should_stop(draw: i32) -> bool {
    draw % 7 == 0
}

/// One auto-fill outcome: the bytes written into `actor[+0x1DF..]` in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoFillQueue {
    /// Queue bytes, in the order retail stored them.
    pub bytes: Vec<u8>,
    /// Rolls that landed under the floor and were discarded - retail writes a
    /// zero into the slot and re-rolls it without advancing the write cursor.
    pub discarded: usize,
}

/// The auto-fill arm (`0x801F05C0..0x801F06D4`).
///
/// `learned` is the character record's learned-arts list
/// (`record[+0x186 + i]`, count at `record[+0x185]`); `rand` supplies the
/// draws in retail's call order - a stop roll first, then an index roll, per
/// iteration.
///
/// Retail bails immediately when the list is empty (`record[+0x185] == 0`).
pub fn auto_fill_queue(
    char_id: u8,
    learned: &[u8],
    mut rand: impl FnMut() -> i32,
) -> AutoFillQueue {
    let mut out = AutoFillQueue::default();
    if learned.is_empty() {
        return out;
    }
    let floor = auto_fill_floor(char_id);
    loop {
        if auto_fill_should_stop(rand()) {
            return out;
        }
        let pick = (rand().rem_euclid(learned.len() as i32)) as usize;
        let art = learned[pick];
        if art < floor {
            // Retail zeroes the slot and does *not* advance the write cursor,
            // so the same slot is rolled again next iteration.
            out.discarded += 1;
        } else {
            out.bytes.push(art.wrapping_add(ART_ACTION_BIAS));
            if out.bytes.len() >= AUTO_FILL_QUEUE_LIMIT {
                return out;
            }
        }
    }
}

/// The two byte-range families the pool arm's weight ladder recognises, chosen
/// by the target monster's type byte (`monster_record[+0x1E]`).
///
/// Retail builds a two-bit mask: bit `0` when the type is `2`, bit `1` when it
/// is `3`, nothing otherwise - so the two families are mutually exclusive in
/// practice and a type outside `{2, 3}` leaves every command at the default
/// weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeightFamily {
    /// Type `3`: low band `..=0x10`, high band `0x16..=0x1A`.
    TypeThree,
    /// Type `2`: low band `0x11..=0x15`, high band `0x1B..=0x1F`.
    TypeTwo,
    /// Any other type - no ranges match, so the weight stays at the default.
    None,
}

/// Retail's `s4` mask construction from the monster type byte.
pub const fn weight_family(monster_type: u8) -> WeightFamily {
    match monster_type {
        3 => WeightFamily::TypeThree,
        2 => WeightFamily::TypeTwo,
        _ => WeightFamily::None,
    }
}

/// Default weight a command carries before either band matches (`s1 = 8`).
pub const WEIGHT_DEFAULT: u8 = 8;

/// Weight after only the low band matched.
pub const WEIGHT_LOW: u8 = 1;

/// Weight after only the high band matched.
pub const WEIGHT_HIGH: u8 = 4;

/// Weight after both bands matched.
pub const WEIGHT_BOTH: u8 = 2;

/// The pool arm's per-command weight ladder (`0x801F081C..0x801F0988`).
///
/// Walks up to four bytes of the command record, stopping at the first zero.
/// Each byte is tested against the family's low band and then its high band,
/// and each test advances the weight one rung:
///
/// | before | low band hits | high band hits |
/// |---|---|---|
/// | `8` (default) | `1` | `4` |
/// | `1` | - | `2` |
/// | `4` | `2` | - |
/// | `2` | - | - |
///
/// Both tests read the byte fresh, and the second test compares against the
/// weight as it stood *before* the first test's write - which is why a single
/// byte inside both bands could not exist and a byte in one band only ever
/// moves one rung.
pub fn command_weight(command_bytes: &[u8], family: WeightFamily) -> u8 {
    /// One band membership test.
    type Band = fn(u8) -> bool;
    let (low, high): (Band, Band) = match family {
        WeightFamily::TypeThree => (|b| b < 0x11, |b| (0x16..=0x1A).contains(&b)),
        WeightFamily::TypeTwo => (
            |b| (0x11..=0x15).contains(&b),
            |b| (0x1B..=0x1F).contains(&b),
        ),
        WeightFamily::None => return WEIGHT_DEFAULT,
    };
    let mut weight = WEIGHT_DEFAULT;
    for &b in command_bytes.iter().take(4) {
        if b == 0 {
            break;
        }
        if low(b) {
            let before = weight;
            if weight == WEIGHT_DEFAULT {
                weight = WEIGHT_LOW;
            }
            if before == WEIGHT_HIGH {
                weight = WEIGHT_BOTH;
            }
        }
        if high(b) {
            let before = weight;
            if weight == WEIGHT_DEFAULT {
                weight = WEIGHT_HIGH;
            }
            if before == WEIGHT_LOW {
                weight = WEIGHT_BOTH;
            }
        }
    }
    weight
}

/// One arts command as the pool arm sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArtsCommand {
    /// Command id (`0x0C..=0x0F`), and the byte pushed into the queue.
    pub id: u8,
    /// AP cost, the record's `+0x74` byte.
    pub cost: u8,
    /// Up to four leading bytes of the command record - the weight ladder's
    /// input.
    pub bytes: [u8; 4],
    /// The command's status-guard mask, `*(i16*)(0x801F672C + (id-0xC)*2)`.
    /// A command whose mask intersects the actor's `+0x16E` word is dropped.
    pub guard: u16,
}

/// A built candidate pool: the repeated command ids plus the cheapest cost seen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CandidatePool {
    /// Command ids, each repeated as many times as its weight - retail's
    /// `sp+0x10` scratch.
    pub entries: Vec<u8>,
    /// `s8` - the minimum `+0x74` cost across **every** scanned command,
    /// including ones the guard later dropped. Seeded at `0xFF`.
    pub min_cost: u8,
    /// Set when the pool outgrew retail's `0x10`-byte scratch. Reachable in
    /// principle (four commands at the default weight of 8 is 32 pushes); the
    /// port records it rather than writing past the buffer.
    pub overflowed: bool,
}

/// Build the weighted candidate pool (`0x801F07C4..0x801F0A0C`).
///
/// `status` is the acting actor's `+0x16E` word. The `min_cost` scan runs
/// before the guard test, so a guarded-out command still lowers the floor the
/// spend loop compares against - that is in the instruction order, not an
/// approximation.
pub fn build_candidate_pool(
    commands: &[ArtsCommand],
    family: WeightFamily,
    status: u16,
) -> CandidatePool {
    let mut pool = CandidatePool {
        min_cost: 0xFF,
        ..Default::default()
    };
    for c in commands {
        if c.cost < pool.min_cost {
            pool.min_cost = c.cost;
        }
        let mut weight = command_weight(&c.bytes, family);
        if status & c.guard != 0 {
            weight = 0;
        }
        for _ in 0..weight {
            if pool.entries.len() >= CANDIDATE_SCRATCH {
                pool.overflowed = true;
                break;
            }
            pool.entries.push(c.id);
        }
    }
    pool
}

/// What the spend loop wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpendResult {
    /// Queue bytes in order; retail also writes a trailing zero terminator
    /// after each push, which this leaves implicit.
    pub queue: Vec<u8>,
    /// AP gauge left after the spend.
    pub gauge_left: i16,
}

/// The pool arm's spend loop (`0x801F0A10..0x801F0B48`).
///
/// Draws uniformly from `pool.entries` (retail: `rand() % count`), refuses a
/// pick the gauge cannot cover, consumes a taken entry by zeroing its scratch
/// slot, subtracts the cost, and loops while the gauge still covers
/// `pool.min_cost`. A zero entry - either a consumed slot or a slot the clear
/// left untouched - is skipped without consuming gauge, which is what makes the
/// loop terminate on a thinning pool.
///
/// The whole loop is skipped when the gauge starts below `pool.min_cost`, and
/// it also stops once it has taken as many entries as the pool held.
pub fn spend_gauge(
    pool: &CandidatePool,
    gauge: i16,
    costs: impl Fn(u8) -> u8,
    mut rand: impl FnMut() -> i32,
) -> SpendResult {
    let mut out = SpendResult {
        gauge_left: gauge,
        ..Default::default()
    };
    if gauge < pool.min_cost as i16 {
        return out;
    }
    let mut scratch = pool.entries.clone();
    if scratch.is_empty() {
        return out;
    }
    let mut taken = 0usize;
    loop {
        if taken == scratch.len() {
            return out;
        }
        let slot = (rand().rem_euclid(scratch.len() as i32)) as usize;
        let cmd = scratch[slot];
        if cmd != 0 && out.gauge_left >= costs(cmd) as i16 {
            out.queue.push(cmd);
            scratch[slot] = 0;
            out.gauge_left -= costs(cmd) as i16;
            taken += 1;
        }
        if out.gauge_left < pool.min_cost as i16 {
            return out;
        }
    }
}

/// Roll the auto-fill arm's target: a uniform draw over the live monster slots,
/// biased to the pool's monster base (`rand() % ctx[1] + 3`).
///
/// Retail immediately pushes the result through `FUN_801DB124`, so a dead slot
/// is redirected rather than kept; the redirect itself lives in
/// [`crate::battle_action::redirect_dead_target`].
pub fn roll_target_slot(monster_count: u8, draw: i32) -> u8 {
    let n = monster_count.max(1) as i32;
    (draw.rem_euclid(n) + 3) as u8
}

/// A convenience view for a caller that only has the party pool as
/// [`FleeActor`]s: is this slot's actor still standing?
///
/// Present so the two battle-formula pools share one liveness predicate rather
/// than each open-coding `+0x14C != 0`.
pub const fn slot_is_live(a: &FleeActor) -> bool {
    a.hp != 0
}

// --- the Spirit-budgeted art insertion tail (0x801F0B4C..0x801F1274) ---------

/// Offset of the art-bank record's combo string (`+0x00`, arrows `1..=4`,
/// zero-terminated) - the bytes the tail both counts and splices.
pub const TAIL_COMBO_LEN: usize = 0x0A;

/// Offset of the zero-terminated byte run the reject arm measures
/// (`lbu v0,0xb(v1)` at `0x801F1200`).
pub const TAIL_SKIP_RUN: usize = 0x0B;

/// First art-bank record index the tail considers: the opening roll is
/// `rand() % 5 + 0xB` (`0x801F0BC4`) and every re-seed is `rand() % 3 + 0xB`
/// (`0x801F11D8`). Bank index `0xB` is learned-art id `0` - the same `0xB`
/// the auto-fill arm's `ART_ACTION_BIAS - 0x10` encodes.
pub const TAIL_FIRST_ART: u8 = 0x0B;

/// The Spirit threshold's base: an insertion is only attempted while
/// `rand() % 7 + 0x12 < budget` (`0x801F0C58..0x801F0C5C`).
pub const TAIL_SPIRIT_THRESHOLD_BASE: i32 = 0x12;

/// Per-input Spirit cost: `0xB` on the first pass, `0xA` after it, `6` from the
/// fifth pass on (`0x801F0CAC..0x801F0CC8`), halved under record `+0xF8`
/// bit `0x800` (`0x801F0D00..0x801F0D0C`).
pub const TAIL_COST_FIRST: u8 = 0x0B;

pub const TAIL_COST_LATER: u8 = 0x0A;

pub const TAIL_COST_LATE: u8 = 6;

/// Record `+0xF8` bit that halves the per-input cost.
pub const TAIL_COST_HALVING_BIT: u32 = 0x800;

/// Accept chance, percent: `0x32` below the character's low-tier bound,
/// `0x4B` at or above it (`0x801F0E4C..0x801F0E78`).
pub const TAIL_CHANCE_LOW_TIER: i32 = 0x32;

pub const TAIL_CHANCE_HIGH_TIER: i32 = 0x4B;

/// Marker stand-in: with the slot's Miracle marker `ctx[+0x25F + slot]`
/// clear, the first four passes overwrite the first arrow's need with `100`
/// (`sb v0,0x10(sp)` at `0x801F0E38`) so no insertion can be afforded.
pub const TAIL_MARKERLESS_NEED: u8 = 0x64;

/// Engine safety bound on the random refill's re-rolls. Retail's loop always
/// terminates (the art's own need leaves one spare direction over the slots
/// it must refill); the bound only stops a malformed input from spinning.
const TAIL_REFILL_GUARD: usize = 0x1_0000;

/// What the tail reads besides the queue.
#[derive(Clone, Copy, Debug)]
pub struct ArtsTailInput<'a> {
    /// Roster character id `DAT_8007BD10[slot]` (`2` = Noa gets the
    /// `0xD`/`0xE` skip and the higher low-tier bound).
    pub char_id: u8,
    /// The acting actor's Spirit gauge `+0x170` (`lhu`, then compared as a
    /// signed halfword). The tail spends a **local copy** - nothing is
    /// stored back.
    pub spirit: u16,
    /// Character record `+0xF8` (the upper accessory-passive word).
    pub ability_high: u32,
    /// Character record's learned-arts list (`+0x186..`, count `+0x185`).
    pub learned: &'a [u8],
    /// The slot's Miracle marker `ctx[+0x25F + slot] != 0`.
    pub miracle_marker: bool,
    /// The art-animation bank (`*(DAT_801C9360[slot] + 0x58)`): the low byte
    /// of its `u32` count is the loop bound, records follow at `+4` at the
    /// `0xD0` stride. Each element is one whole record.
    pub records: &'a [[u8; 0xD0]],
    /// The bank's count byte (`lbu v0,0x0(v0)` at `0x801F0B84`). Kept apart
    /// from `records.len()` so a caller can pass the disc's count verbatim;
    /// a record index past `records` reads as all-zero bytes.
    pub art_count: u8,
}

/// One art the tail spliced in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TailInsertion {
    /// Bank record index (learned-art id + `0xB`).
    pub art: u8,
    /// Queue position its first arrow landed at.
    pub at: usize,
    /// Arrows spliced (`s2`: `1` + the non-zero run from combo byte `1`).
    pub len: usize,
    /// Spirit charged against the local budget (`len * per-input cost`).
    pub cost: i32,
    /// The combo was one longer than the free region: retail's head index
    /// went to `-1`, so every free slot took `combo[i + 1]` and the combo's
    /// byte `0` fell off. [`Self::at`] reads `0` then.
    pub clipped: bool,
}

/// What the tail did to the queue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtsTailResult {
    /// Insertions in the order they were made.
    pub inserted: Vec<TailInsertion>,
    /// The local Spirit budget left (never written back).
    pub budget_left: i32,
    /// `rand()` draws consumed.
    pub draws: usize,
}

/// The auto-combo's **art insertion tail** (`FUN_801F0450`
/// `0x801F0B4C..0x801F1274`), run after the pool arm's spend loop has written
/// `queue` - a run of direction swings `0x0C..=0x0F` into `actor[+0x1DF..]`.
///
/// It walks the character's art-animation bank upward from a random record in
/// `0xB..=0xF`, and each pass either splices one art's arrow string over the
/// tail of the still-free region of the queue or skips ahead:
///
/// 1. Noa (`char_id == 2`) steps over records `0xD` / `0xE` (`+2`).
/// 2. **Spirit gate**: stop unless `rand() % 7 + 0x12 < budget` and at least two
///    free queue slots remain.
/// 3. A record whose combo byte `1` is zero (fewer than two arrows) is skipped.
/// 4. Need vs have: the combo's arrows from byte `1` on (byte `0` is spliced
///    but not counted, `li s2,0x1` at `0x801F0DA0`) against the free region's
///    direction census; without the Miracle marker the first four passes
///    demand `100` of arrow `1`.
/// 5. Cost `len * per_input` must fit the budget; a `rand() % 100` roll under
///    `50` / `75` (low / high tier); the art must be **learned**
///    (`index - 0xB` in the list); not the art just placed; not below the tier
///    floor a placed low-tier art raises (`0x11` for Noa, `0xF` otherwise).
/// 6. Accept: the census drops by the need, the budget by the cost, and the
///    free region is rebuilt - its head refilled with `rand() % 4` directions
///    drawn from what is left, its last `len` slots the combo as `arrow + 0xB`.
///    The free region shrinks by `len`, and the next record is
///    `rand() % 3 + 0xB`.
/// 7. Reject: skip ahead by **twice** the length of the record's zero-terminated
///    run at `+0x0B` - the loop's `a0` is loaded once and its delay-slot
///    increment runs on both edges, so every byte of the run counts two.
///
/// Every pass then advances by `rand() % 2 + 1` and the walk ends once the
/// index reaches the bank count. `queue.len()` is the spend loop's count (the
/// free region's starting size); bytes past it are not touched.
///
/// PORT: FUN_801F0450 (`0x801F0B4C..0x801F1274`, the art insertion tail)
///
/// Wired: `engine-core`'s `World::run_auto_attack_pool_arms`, run from
/// `begin_round_execution` for every Attack a player committed off an Auto
/// pick - the command flow both hosts drive.
pub fn insert_arts(
    queue: &mut [u8],
    input: &ArtsTailInput,
    mut rand: impl FnMut() -> i32,
) -> ArtsTailResult {
    let mut draws = 0usize;
    let mut draw = || {
        draws += 1;
        rand()
    };
    static ZERO: [u8; 0xD0] = [0; 0xD0];
    let records = input.records;
    let rec = |i: u8| records.get(usize::from(i)).unwrap_or(&ZERO);
    let mut inserted = Vec::new();
    // `s7`: a local copy of the Spirit gauge, compared as a signed halfword.
    let mut budget: i32 = i32::from(input.spirit as i16);
    let budget16 = |b: i32| i32::from(b as i16);
    // `s8`: the free region's size, compared as a signed halfword. It can
    // go to `-1` (see the rebuild below), which ends the walk at the next
    // Spirit gate.
    let mut free: isize = queue.len().min(0xFF) as isize;
    let mut pass: u8 = 0; // `s5`, compared as a byte
    let mut floor: u8 = 0; // `sp+0x38`
    let mut last: u8 = 0xFF; // `s3`
    // `s4` is only ever compared and indexed through `andi 0xff`, so byte
    // wrapping arithmetic is exact.
    let mut art: u8 = (draw() % 5 + i32::from(TAIL_FIRST_ART)) as u8;
    let noa = input.char_id == 2;
    while art < input.art_count {
        'pass: {
            if noa && art.wrapping_sub(0xD) < 2 {
                art = art.wrapping_add(2);
            }
            // The Spirit gate. Its stop path parks the index at the count
            // (`lbu s4,0x40(sp)` at `0x801F0C78`) and still takes the common
            // advance below, draw included.
            let threshold = draw() % 7 + TAIL_SPIRIT_THRESHOLD_BASE;
            if threshold >= budget16(budget) || free < 2 {
                art = input.art_count;
                break 'pass;
            }
            let r = rec(art);
            if r[1] == 0 {
                break 'pass;
            }
            let mut per_input = if pass == 0 {
                TAIL_COST_FIRST
            } else {
                TAIL_COST_LATER
            };
            if pass >= 4 {
                per_input = TAIL_COST_LATE;
            }
            if input.ability_high & TAIL_COST_HALVING_BIT != 0 {
                per_input >>= 1;
            }
            // Retail's 8-byte scratch `sp+0x10..0x17`: lanes 0..3 = the
            // combo's need (arrow - 1), lanes 4..7 = the free region's census
            // (byte - 8). Both indices are unchecked byte arithmetic; an
            // index outside the scratch lands elsewhere in retail's frame and
            // is dropped here.
            let mut sp = [0u8; 8];
            for &b in &queue[..free.max(0) as usize] {
                if let Some(v) = sp.get_mut(usize::from(b.wrapping_sub(8))) {
                    *v = v.wrapping_add(1);
                }
            }
            let mut len = 1usize; // `s2`
            while len < TAIL_COMBO_LEN && r[len] != 0 {
                if let Some(v) = sp.get_mut(usize::from(r[len].wrapping_sub(1))) {
                    *v = v.wrapping_add(1);
                }
                len += 1;
            }
            if !input.miracle_marker && pass < 4 {
                sp[0] = TAIL_MARKERLESS_NEED;
            }
            let low_bound = if noa { 0x11 } else { 0x0F };
            let mut chance = if art < low_bound {
                TAIL_CHANCE_LOW_TIER
            } else {
                TAIL_CHANCE_HIGH_TIER
            };
            // `0x801F0E7C..0x801F0E90`: a zero index (reachable only through
            // byte wrap) replaces the chance with `budget == 100`.
            if art == 0 {
                chance = i32::from(budget16(budget) == 100);
            }
            let cost = (len as i32) * i32::from(per_input);
            let reject = 'gate: {
                if (0..4).any(|k| sp[4 + k] < sp[k]) {
                    break 'gate true;
                }
                if budget16(budget) < cost {
                    break 'gate true;
                }
                if draw() % 100 >= chance {
                    break 'gate true;
                }
                if !input.learned.contains(&art.wrapping_sub(TAIL_FIRST_ART)) {
                    break 'gate true;
                }
                last == art || art < floor
            };
            if reject {
                // `0x801F11DC..0x801F123C`: `a0` holds the run's first byte
                // for the whole loop and the delay-slot `addiu s1,s1,1` runs
                // on both edges, so each byte of the run counts twice.
                let run = r[TAIL_SKIP_RUN..].iter().take_while(|&&b| b != 0).count();
                art = art.wrapping_add((2 * run) as u8);
                break 'pass;
            }
            for k in 0..4 {
                sp[4 + k] = sp[4 + k].wrapping_sub(sp[k]);
            }
            budget -= cost;
            // Rebuild the free region: `head` slots refilled from the census,
            // the rest the combo as `arrow + 0xB`. `head` is `-1` when the
            // combo is one longer than the free region; retail then writes
            // `rec[i + 1]` into every slot (the combo's byte 0 falls off).
            let head = free - len as isize;
            let mut guard = 0usize;
            let mut i: isize = 0;
            while i < free {
                let at = i as usize;
                if i < head {
                    let d = draw() % 4;
                    queue[at] = d as u8;
                    let lane = usize::from((d as u8).wrapping_add(4));
                    match sp.get_mut(lane) {
                        Some(v) if *v != 0 => {
                            *v -= 1;
                            queue[at] = (d as u8).wrapping_add(0x0C);
                        }
                        _ => {
                            guard += 1;
                            if guard > TAIL_REFILL_GUARD {
                                break;
                            }
                            continue;
                        }
                    }
                } else {
                    queue[at] = r[(i - head) as usize].wrapping_add(0x0B);
                }
                i += 1;
            }
            inserted.push(TailInsertion {
                art,
                at: head.max(0) as usize,
                len,
                cost,
                clipped: head < 0,
            });
            free = head;
            last = art;
            if art < low_bound {
                floor = low_bound;
            }
            art = (draw() % 3 + i32::from(TAIL_FIRST_ART)) as u8;
        }
        pass = pass.wrapping_add(1);
        art = art.wrapping_add((draw() % 2 + 1) as u8);
    }
    ArtsTailResult {
        inserted,
        budget_left: budget,
        draws,
    }
}

#[cfg(test)]
mod tests;
