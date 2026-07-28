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
//! ## What is not ported here
//!
//! The tail from `0x801F0B4C` on - a second budget pass keyed on
//! `actor[+0x170]` with `/10` and `/7` reciprocal divides and an
//! `AP-Used-Down`-style halving - is decoded only far enough to know it is a
//! distinct budget stage, so it is left out rather than guessed at. Everything
//! this module exposes is a kernel whose every instruction is accounted for.
//!
//! # NOT WIRED
//!
//! No engine caller - but **the call site is not the missing piece**, and an
//! earlier note here naming the unported battle flow SM `FUN_801D388C` as the
//! caller was wrong. The retail caller is the action SM itself: `jal
//! 0x801f0450` at `0x801E2AB8` in `overlay_battle_action_801e295c.txt`, the
//! first call of the `ctx[+0x07] == 0x00` arm that begins at `0x801E2AB4` -
//! the arm that latches `ctx[+0x290]` into `+0x291` and stamps state `0x0A` /
//! `0x0B`, which is ported and live as
//! [`battle_action`](crate::battle_action)'s `Begin`. The wire has a home;
//! what it has no arguments for is the pool arm's two disc-side inputs:
//!
//! * the per-(character, weapon) arts-command records at
//!   `DAT_801C9360[slot][cmd]` with their `+0x74` AP costs (see
//!   [`docs/subsystems/arts-command-gauge.md`](../../docs/subsystems/arts-command-gauge.md)),
//!   which reach the disc only as far as the equipped-swing decode and never
//!   into a battle-setup table, and
//! * the four-entry status-guard mask table at `0x801F672C`, which no parser
//!   extracts at all.
//!
//! The auto-fill arm needs less - the ability bit is already
//! `BattleActionHost::character_ability_bits` and the status word is on the
//! actor - but its learned-arts list (`record[+0x185]` count, `+0x186..`
//! bytes) has no host accessor either. Calling the module from `Begin` with
//! empty tables would be a call that does nothing in production, so the
//! honest order is tables first, call site second. `engine-core` drives
//! auto-fighting party members through its own stand-in physical action until
//! then.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_needs_the_bit_set_and_the_status_clear() {
        assert!(gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0));
        assert!(!gate_selects_auto_fill(0, 0));
        assert!(!gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0x0004));
        assert!(!gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0x0400));
        // Bits outside the mask do not veto.
        assert!(gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0x0380));
    }

    #[test]
    fn only_character_two_gets_the_higher_floor() {
        assert_eq!(auto_fill_floor(1), 4);
        assert_eq!(auto_fill_floor(2), 6);
        assert_eq!(auto_fill_floor(3), 4);
        assert_eq!(auto_fill_floor(0), 4);
    }

    #[test]
    fn stop_roll_fires_on_every_seventh_value() {
        assert!(auto_fill_should_stop(0));
        assert!(auto_fill_should_stop(7));
        assert!(auto_fill_should_stop(70));
        for r in 1..7 {
            assert!(!auto_fill_should_stop(r), "r={r}");
        }
    }

    #[test]
    fn auto_fill_bails_on_an_empty_learned_list() {
        let mut n = 0;
        let out = auto_fill_queue(1, &[], || {
            n += 1;
            1
        });
        assert!(out.bytes.is_empty());
        assert_eq!(n, 0, "the count gate precedes every draw");
    }

    #[test]
    fn auto_fill_biases_each_kept_art_and_discards_the_ones_under_the_floor() {
        // learned = [2, 9]: 2 is under the floor of 4, 9 is over it.
        let learned = [2u8, 9];
        // Draws, in call order: (stop=1, pick=0) -> discard;
        // (stop=1, pick=1) -> keep; (stop=7) -> stop.
        let mut script = vec![1, 0, 1, 1, 7].into_iter();
        let out = auto_fill_queue(1, &learned, || script.next().unwrap());
        assert_eq!(out.discarded, 1);
        assert_eq!(out.bytes, vec![9 + ART_ACTION_BIAS]);
    }

    #[test]
    fn auto_fill_stops_at_the_queue_limit() {
        let learned = [9u8];
        // Never stop, always pick index 0.
        let out = auto_fill_queue(1, &learned, || 1);
        assert_eq!(out.bytes.len(), AUTO_FILL_QUEUE_LIMIT);
        assert!(out.bytes.iter().all(|&b| b == 9 + ART_ACTION_BIAS));
    }

    #[test]
    fn floor_six_rejects_arts_a_floor_of_four_would_keep() {
        let learned = [5u8];
        let keep = auto_fill_queue(1, &learned, {
            let mut n = 0;
            move || {
                n += 1;
                if n > 2 { 7 } else { 1 }
            }
        });
        assert_eq!(keep.bytes, vec![5 + ART_ACTION_BIAS]);
        let reject = auto_fill_queue(2, &learned, {
            let mut n = 0;
            move || {
                n += 1;
                if n > 2 { 7 } else { 1 }
            }
        });
        assert!(reject.bytes.is_empty());
        assert_eq!(reject.discarded, 1);
    }

    #[test]
    fn weight_family_comes_off_the_monster_type_byte() {
        assert_eq!(weight_family(2), WeightFamily::TypeTwo);
        assert_eq!(weight_family(3), WeightFamily::TypeThree);
        for t in [0u8, 1, 4, 0xFF] {
            assert_eq!(weight_family(t), WeightFamily::None, "type {t}");
        }
    }

    #[test]
    fn no_family_leaves_every_command_at_the_default_weight() {
        assert_eq!(
            command_weight(&[0x12, 0x1C, 0, 0], WeightFamily::None),
            WEIGHT_DEFAULT
        );
    }

    #[test]
    fn the_weight_ladder_walks_default_then_one_band_then_both() {
        use WeightFamily::TypeTwo;
        // Nothing in either band.
        assert_eq!(command_weight(&[0x20, 0x21, 0, 0], TypeTwo), WEIGHT_DEFAULT);
        // Low band only.
        assert_eq!(command_weight(&[0x11, 0x20, 0, 0], TypeTwo), WEIGHT_LOW);
        // High band only.
        assert_eq!(command_weight(&[0x1B, 0x20, 0, 0], TypeTwo), WEIGHT_HIGH);
        // Low then high, and high then low, both land on 2.
        assert_eq!(command_weight(&[0x11, 0x1B, 0, 0], TypeTwo), WEIGHT_BOTH);
        assert_eq!(command_weight(&[0x1B, 0x11, 0, 0], TypeTwo), WEIGHT_BOTH);
    }

    #[test]
    fn the_other_family_uses_the_other_two_bands() {
        use WeightFamily::TypeThree;
        // `..=0x10` is the low band here, so 0x11 misses it entirely.
        assert_eq!(command_weight(&[0x11, 0, 0, 0], TypeThree), WEIGHT_DEFAULT);
        assert_eq!(command_weight(&[0x10, 0, 0, 0], TypeThree), WEIGHT_LOW);
        assert_eq!(command_weight(&[0x16, 0, 0, 0], TypeThree), WEIGHT_HIGH);
        assert_eq!(command_weight(&[0x1A, 0x05, 0, 0], TypeThree), WEIGHT_BOTH);
        // 0x1B is past the high band for this family.
        assert_eq!(command_weight(&[0x1B, 0, 0, 0], TypeThree), WEIGHT_DEFAULT);
    }

    #[test]
    fn a_zero_byte_ends_the_ladder_scan() {
        use WeightFamily::TypeTwo;
        // The 0x1B after the terminator is never seen.
        assert_eq!(command_weight(&[0x11, 0, 0x1B, 0x1B], TypeTwo), WEIGHT_LOW);
    }

    #[test]
    fn the_ladder_reads_at_most_four_bytes() {
        use WeightFamily::TypeTwo;
        let bytes = [0x20, 0x20, 0x20, 0x20];
        assert_eq!(command_weight(&bytes, TypeTwo), WEIGHT_DEFAULT);
        // A fifth byte in a band cannot be reached.
        assert_eq!(command_weight(&[0x20, 0x20, 0x20, 0x20], TypeTwo), 8);
    }

    fn cmd(id: u8, cost: u8, bytes: [u8; 4], guard: u16) -> ArtsCommand {
        ArtsCommand {
            id,
            cost,
            bytes,
            guard,
        }
    }

    #[test]
    fn pool_repeats_each_command_by_its_weight_and_tracks_the_cheapest() {
        let commands = [
            cmd(0x0C, 20, [0x11, 0, 0, 0], 0),    // low band -> weight 1
            cmd(0x0D, 8, [0x1B, 0, 0, 0], 0),     // high band -> weight 4
            cmd(0x0E, 30, [0x11, 0x1B, 0, 0], 0), // both -> weight 2
        ];
        let pool = build_candidate_pool(&commands, WeightFamily::TypeTwo, 0);
        assert_eq!(pool.min_cost, 8);
        assert_eq!(pool.entries.iter().filter(|&&c| c == 0x0C).count(), 1);
        assert_eq!(pool.entries.iter().filter(|&&c| c == 0x0D).count(), 4);
        assert_eq!(pool.entries.iter().filter(|&&c| c == 0x0E).count(), 2);
        assert!(!pool.overflowed);
    }

    #[test]
    fn a_guard_hit_drops_a_command_but_still_lowers_the_min_cost() {
        let commands = [
            cmd(0x0C, 4, [0x11, 0, 0, 0], 0x0080),
            cmd(0x0D, 20, [0x11, 0, 0, 0], 0),
        ];
        let pool = build_candidate_pool(&commands, WeightFamily::TypeTwo, 0x0080);
        assert!(!pool.entries.contains(&0x0C), "guarded out");
        assert!(pool.entries.contains(&0x0D));
        assert_eq!(
            pool.min_cost, 4,
            "the min-cost scan runs before the guard test"
        );
    }

    #[test]
    fn the_default_weight_can_outgrow_retails_scratch() {
        // Four commands, no family match, weight 8 apiece = 32 pushes into a
        // 16-byte buffer.
        let commands: Vec<ArtsCommand> = (FIRST_COMMAND..LAST_COMMAND_EXCL)
            .map(|id| cmd(id, 10, [0x20, 0, 0, 0], 0))
            .collect();
        let pool = build_candidate_pool(&commands, WeightFamily::None, 0);
        assert!(pool.overflowed);
        assert_eq!(pool.entries.len(), CANDIDATE_SCRATCH);
    }

    #[test]
    fn spend_is_skipped_when_the_gauge_cannot_cover_the_cheapest() {
        let pool = CandidatePool {
            entries: vec![0x0C, 0x0C],
            min_cost: 20,
            overflowed: false,
        };
        let mut n = 0;
        let out = spend_gauge(
            &pool,
            19,
            |_| 20,
            || {
                n += 1;
                0
            },
        );
        assert!(out.queue.is_empty());
        assert_eq!(out.gauge_left, 19);
        assert_eq!(n, 0, "the gate precedes the first draw");
    }

    #[test]
    fn spend_consumes_the_gauge_and_stops_below_the_floor() {
        let pool = CandidatePool {
            entries: vec![0x0C, 0x0D, 0x0E, 0x0F],
            min_cost: 10,
            overflowed: false,
        };
        // Draw slots 0, 1, 2, 3 in order.
        let mut seq = (0..8).cycle();
        let out = spend_gauge(&pool, 25, |_| 10, || seq.next().unwrap());
        assert_eq!(out.queue.len(), 2, "25 AP buys two 10-cost commands");
        assert_eq!(out.gauge_left, 5);
    }

    #[test]
    fn spend_stops_once_every_entry_is_consumed() {
        let pool = CandidatePool {
            entries: vec![0x0C, 0x0D],
            min_cost: 1,
            overflowed: false,
        };
        let mut seq = (0..2).cycle();
        let out = spend_gauge(&pool, 1000, |_| 1, || seq.next().unwrap());
        assert_eq!(out.queue.len(), 2);
        assert_eq!(out.gauge_left, 998);
    }

    #[test]
    fn a_pick_the_gauge_cannot_afford_is_refused_without_spending() {
        let pool = CandidatePool {
            entries: vec![0x0C, 0x0D],
            min_cost: 5,
            overflowed: false,
        };
        // Slot 0 is a 100-cost command the gauge cannot buy; slot 1 costs 5.
        let mut seq = [0i32, 0, 1].into_iter().chain(std::iter::repeat(1));
        let out = spend_gauge(
            &pool,
            9,
            |c| if c == 0x0C { 100 } else { 5 },
            || seq.next().unwrap(),
        );
        assert_eq!(out.queue, vec![0x0D]);
        assert_eq!(out.gauge_left, 4);
    }

    #[test]
    fn target_roll_lands_in_the_monster_slot_band() {
        for draw in 0..12 {
            let slot = roll_target_slot(5, draw);
            assert!((3..8).contains(&slot), "draw {draw} -> slot {slot}");
        }
        assert_eq!(roll_target_slot(1, 999), 3);
    }

    #[test]
    fn liveness_predicate_matches_the_hp_halfword_test() {
        assert!(slot_is_live(&FleeActor {
            hp: 1,
            max_hp: 1,
            atk: 0
        }));
        assert!(!slot_is_live(&FleeActor::default()));
    }
}
