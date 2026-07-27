//! Run / escape roll (`FUN_801E791C`). Split out of `battle_formulas.rs`.

// ---------------------------------------------------------------------------
// Run / escape roll (FUN_801E791C)
// ---------------------------------------------------------------------------
//
// The routine battle-action state 0x64 calls to decide a retail flee. It
// writes the outcome into `_DAT_8007726C` - the battle-message source pointer
// states 0x64/0x65 test: `ctx + 0x159` ("escaped" text) on success,
// `ctx + 0x189` ("couldn't escape" text) on failure.
//
//   party_score = Σ_party  (SPD*3)>>1 + (maxHP - curHP)>>4     (actor +0x164 / +0x14E / +0x14C)
//   enemy_score = Σ_enemy   SPD      + (maxHP - curHP)>>5
//   roll_p = rand() % party_score;  roll_e = rand() % enemy_score
//   if Escape Boost (ability bit 52):  roll_p += roll_p >> 1
//   if Great Escape (bit 55) or ctx[+0x291] == 2 or forced:  roll_p = roll_e
//   fail  iff  !forced && (roll_p < roll_e || ctx[+0x287] != 0)
//
// Both sides run faster the more hurt they are (missing HP raises the score),
// and the party's SPD is weighted 1.5x against the enemies' 1x. The accessory
// bits are the +0xF8 ability word (passives 0x34 "Escape Boost" / Chicken
// Heart and 0x37 "Great Escape" / Chicken King - the assured-escape bit wins
// the compare exactly but still loses to the no-escape battle flag
// `ctx+0x287`, which is why Chicken King is "assured escape (non-boss)").
// `forced` is the battle flag `_DAT_8007bac0 & 0x100`: it bypasses even the
// no-escape flag and skips the "No. of Escapes" Records counter
// (`_DAT_800846A8`) the normal success path increments.

/// One combatant folded into an escape-roll side score (`FUN_801E791C`).
#[derive(Clone, Copy, Debug, Default)]
pub struct EscapeActor {
    /// Live SPD stat (actor `+0x164`).
    pub speed: u16,
    /// Current HP (actor `+0x14C`).
    pub hp: u16,
    /// Max HP (actor `+0x14E`).
    pub max_hp: u16,
}

/// Party-side flags folded into the escape decision (`FUN_801E791C`).
#[derive(Clone, Copy, Debug, Default)]
pub struct EscapeFlags {
    /// Ability bit 52 (passive `0x34`, Chicken Heart): party roll * 1.5.
    /// Retail ORs the bit over the *living* party members' `+0xF8` words.
    pub escape_boost: bool,
    /// Ability bit 55 (passive `0x37`, Chicken King) - or the `ctx+0x291 == 2`
    /// battle-type byte: the party roll is set equal to the enemy roll, so
    /// the compare can't fail (assured escape) but `no_escape` still blocks.
    pub assured: bool,
    /// `ctx+0x287` - the scripted "can't run from this battle" flag.
    pub no_escape: bool,
    /// `_DAT_8007bac0 & 0x100` - forced flee: succeeds unconditionally
    /// (bypasses even `no_escape`) and skips the flee counter.
    pub forced: bool,
}

impl EscapeFlags {
    /// Bit 20 of the second ability word (`record+0xF8`) = passive index
    /// `0x34` (bit 52 of the 64-bit field) - Escape Boost.
    pub const ESCAPE_BOOST_WORD1: u32 = 0x0010_0000;
    /// Bit 23 of the second ability word = passive index `0x37` (bit 55) -
    /// Great Escape.
    pub const GREAT_ESCAPE_WORD1: u32 = 0x0080_0000;

    /// Fold one living party member's second ability word (`record+0xF8`)
    /// into the flags, the per-slot OR of `FUN_801E791C`'s party loop.
    pub fn fold_ability_word1(&mut self, word1: u32) {
        self.escape_boost |= word1 & Self::ESCAPE_BOOST_WORD1 != 0;
        self.assured |= word1 & Self::GREAT_ESCAPE_WORD1 != 0;
    }

    /// Fold the **latched formation advantage** (`ctx+0x291`) into the flags -
    /// the second, non-accessory source of [`Self::assured`].
    ///
    /// `FUN_801E791C` reads the latched byte directly and compares it against
    /// `2`:
    ///
    /// ```text
    /// 801e7ad8  lbu v1,0x291(v0)      ; v0 = ctx (_DAT_8007bd24)
    /// 801e7adc  li  v0,0x2
    /// 801e7ae0  beq v1,v0,0x801e7af0  ; -> move s2,s3  (roll_p = roll_e)
    /// ```
    ///
    /// So only [`FormationAdvantage::Preemptive`] qualifies - a back attack
    /// (`1`) does **not** penalise the escape roll here, it only cost the party
    /// its first round through the initiative lockout.
    ///
    /// "Assured" overstates it: setting `roll_p = roll_e` only makes the
    /// `roll_p < roll_e` compare unsatisfiable. The `ctx+0x287` no-escape flag
    /// is tested *after* that compare (`801e7b14`) and still fails the escape,
    /// so a pre-emptive strike into a scripted no-flee battle does not get away.
    /// Only the forced-flee arm bypasses `+0x287`.
    ///
    /// REF: FUN_801E791C (the `ctx+0x291 == 2` arm)
    pub fn fold_formation_latch(&mut self, latched: super::FormationAdvantage) {
        self.assured |= latched == super::FormationAdvantage::Preemptive;
    }
}

/// The party side of the escape compare (`FUN_801E791C` first loop): each
/// party slot contributes `(SPD*3)>>1 + (maxHP - curHP)>>4`. Retail iterates
/// every party slot (downed members included - a downed member still
/// contributes its full missing HP).
pub fn escape_party_score(party: &[EscapeActor]) -> u32 {
    party
        .iter()
        .map(|a| ((a.speed as u32 * 3) >> 1) + ((a.max_hp.saturating_sub(a.hp) as u32) >> 4))
        .sum()
}

/// The enemy side of the escape compare (`FUN_801E791C` second loop): each
/// enemy slot contributes `SPD + (maxHP - curHP)>>5`.
pub fn escape_enemy_score(enemies: &[EscapeActor]) -> u32 {
    enemies
        .iter()
        .map(|a| a.speed as u32 + ((a.max_hp.saturating_sub(a.hp) as u32) >> 5))
        .sum()
}

/// The escape decision of `FUN_801E791C`: `true` = the party gets away.
///
/// `rand` is the routine's two 15-bit PsyQ rand draws in call order (first
/// modulo the party score, second modulo the enemy score). Retail traps on a
/// zero score (`break 0x1C00` on the div); the engine saturates both scores
/// at 1 instead - a zero score cannot occur in a live battle (every living
/// actor has nonzero SPD).
///
/// PORT: FUN_801E791C (roll + compare; the success-side staging - actor
/// scatter toward camera, live-HP writeback to the character records with
/// the downed-member 1-HP floor, flee-counter bump - stays with the callers
/// in `battle_action` / engine-core.)
pub fn escape_roll(party_score: u32, enemy_score: u32, flags: EscapeFlags, rand: [u16; 2]) -> bool {
    let mut roll_p = rand[0] as u32 % party_score.max(1);
    let roll_e = rand[1] as u32 % enemy_score.max(1);
    if flags.escape_boost {
        roll_p += roll_p >> 1;
    }
    if flags.assured || flags.forced {
        roll_p = roll_e;
    }
    if flags.forced {
        return true;
    }
    !(roll_p < roll_e || flags.no_escape)
}

// ---------------------------------------------------------------------------
// Monster escape roll (FUN_801EC0DC)
// ---------------------------------------------------------------------------
//
// The enemy-side mirror of the party roll above: "does the monster in this slot
// break off and flee this action?" Transcribed from the DISASSEMBLY in
// `ghidra/scripts/funcs/overlay_battle_action_801ec0dc.txt` (194 instructions,
// battle overlay 0898 at base 0x801CE818) - not from that dump's C.
//
// Three facts pin it as the enemy escape roll rather than, say, the capture
// roll its caller `FUN_801E9FD4` also hosts:
//
//   * it opens on the same `ctx[+0x287]` no-escape gate the party roll's
//     failure arm tests, and returns "no" unconditionally when that is set;
//   * the `*3/2` weighting and the `missingHP >> 5` term are the party roll's
//     own shapes, applied to the other side;
//   * the one ability bit that can force a "no" is `record[+0xF8] & 0x400000`
//     = bit 54 of the 64-bit accessory-passive field = passive index `0x36`,
//     **No Escape** (Chicken Guard), whose in-game text is literally "enemies
//     can't escape". See `docs/formats/accessory-passive-table.md`.
//
// Retail arithmetic, in order:
//
//   monster_sum = SUM over live monster slots (3..3+monster_count):
//                     maxHP + curHP>>1 + ATK          (+0x14E, +0x14C, +0x158)
//   for slot in 0..party_count:                        (slots 0..2)
//       if curHP == 0 { monster_sum <<= 1 }            // a downed member
//       else { party_sum += maxHP>>3 + curHP>>4 + ATK>>3
//              blocked |= record[+0xF8] & 0x400000 }
//   party_avg   = party_sum   / party_count            // retail traps on 0
//   monster_avg = monster_sum / monster_count
//   party_avg  += (target.maxHP - target.curHP) >> 5
//   monster_avg = max(monster_avg, (party_avg * 3) >> 1)
//   spread      = monster_avg - target.INT * 2;  if spread <= 0 { spread = 1 }
//   party_roll   = party_avg   + rand() % (party_avg + target.INT)
//   monster_roll = monster_avg + rand() % spread
//   if monster_roll >= party_roll { return false }
//   if rand() & 7 != 0           { return false }      // flat 1-in-8 gate
//   !blocked
//
// Every sign points the same way, which is the cross-check that the reading is
// the right way round: a *wounded* monster flees more easily (its own missing HP
// is added to the **party** side, the side it has to beat), a *winning* monster
// flees less (each downed party member doubles the monster side), and the
// weighting plus the 1-in-8 gate together make a flee rare.

/// One combatant folded into a monster-escape side score (`FUN_801EC0DC`).
///
/// Distinct from [`EscapeActor`]: the enemy roll weighs HP and **ATK**
/// (`+0x158`), where the party roll weighs SPD (`+0x164`).
#[derive(Clone, Copy, Debug, Default)]
pub struct FleeActor {
    /// Current HP (actor `+0x14C`). Zero = downed; a downed *monster*
    /// contributes nothing, a downed *party member* doubles the monster side.
    pub hp: u16,
    /// Max HP (actor `+0x14E`).
    pub max_hp: u16,
    /// Live ATK stat (actor `+0x158`).
    pub atk: u16,
}

/// Ability bit `0x400000` of the second ability word (`record+0xF8`) = passive
/// index `0x36` (bit 54 of the 64-bit field) - **No Escape** / Chicken Guard.
pub const NO_ESCAPE_WORD1: u32 = 0x0040_0000;

/// The two side scores of `FUN_801EC0DC`, before the averaging step.
///
/// `party` is indexed by battle slot `0..party_count`, `monsters` by
/// `0..monster_count` (retail reads them from pool slots `3..`).
/// `ability_word1` holds each party slot's character-record `+0xF8` word.
///
/// Returns `(monster_sum, party_sum, blocked)`.
pub fn monster_escape_side_scores(
    party: &[FleeActor],
    monsters: &[FleeActor],
    ability_word1: &[u32],
) -> (u32, u32, bool) {
    let mut monster_sum: u32 = 0;
    for m in monsters {
        if m.hp == 0 {
            continue;
        }
        monster_sum = monster_sum
            .wrapping_add(m.max_hp as u32)
            .wrapping_add((m.hp >> 1) as u32)
            .wrapping_add(m.atk as u32);
    }

    let mut party_sum: u32 = 0;
    let mut blocked = false;
    for (i, p) in party.iter().enumerate() {
        if p.hp == 0 {
            // The dead-member arm doubles the *monster* accumulator - retail's
            // `sll s1,s1,0x1` sits inside the party loop, not the monster one.
            monster_sum = monster_sum.wrapping_shl(1);
            continue;
        }
        party_sum = party_sum
            .wrapping_add((p.max_hp >> 3) as u32)
            .wrapping_add((p.hp >> 4) as u32)
            .wrapping_add((p.atk >> 3) as u32);
        if ability_word1.get(i).copied().unwrap_or(0) & NO_ESCAPE_WORD1 != 0 {
            blocked = true;
        }
    }

    (monster_sum, party_sum, blocked)
}

/// The monster-escape decision of `FUN_801EC0DC`: `true` = the monster in
/// `target` breaks off and flees.
///
/// `no_escape_flag` is `ctx[+0x287]`, the same byte [`escape_roll`] takes as
/// [`EscapeFlags::no_escape`]. `target` is the fleeing monster's own actor
/// (retail resolves it as pool slot `param_1`), `target_int` its INT stat
/// (`+0x168`).
///
/// `rand` is called once per retail `func_0x80056798` draw **in call order**,
/// and the third draw only happens when the score compare passes - which is why
/// this takes a closure rather than a fixed array: an array would over-consume
/// the stream on the common failure path.
///
/// Retail traps (`break 0x1C00`) when either side count is zero; this saturates
/// the divisors at 1 instead. A live battle always has both.
///
/// PORT: FUN_801EC0DC
///
/// Wired at the retail call site's mirror: the monster action picker's
/// once-per-pass flee checkpoint (`FUN_801E9FD4`, `jal 0x801ec0dc` at
/// `0x801ea980`) is `engine-core`'s `World::pick_monster_action`, which calls
/// this through `World::monster_flee_roll` and seeds action category
/// `+0x1DE == 5` on success - the monster arm of the Run band (the state-0x68
/// leave-battle states). See `docs/subsystems/battle-formulas.md`
/// ("Monster escape roll - FUN_801EC0DC").
pub fn monster_escape_roll(
    no_escape_flag: u8,
    party: &[FleeActor],
    monsters: &[FleeActor],
    ability_word1: &[u32],
    target: FleeActor,
    target_int: u16,
    mut rand: impl FnMut() -> u32,
) -> bool {
    if no_escape_flag != 0 {
        return false;
    }
    let (monster_sum, party_sum, blocked) =
        monster_escape_side_scores(party, monsters, ability_word1);

    let mut party_avg = (party_sum / (party.len() as u32).max(1)) as i32;
    let mut monster_avg = (monster_sum / (monsters.len() as u32).max(1)) as i32;

    // `subu` then `sra 5`: a negative difference stays negative.
    party_avg += (target.max_hp as i32 - target.hp as i32) >> 5;

    let floor = party_avg.wrapping_mul(3) >> 1;
    if monster_avg < floor {
        monster_avg = floor;
    }

    let mut spread = monster_avg - (target_int as i32) * 2;
    if spread <= 0 {
        spread = 1;
    }

    let party_div = (party_avg + target_int as i32).max(1);
    let party_roll = party_avg + (rand() % party_div as u32) as i32;
    let monster_roll = monster_avg + (rand() % spread as u32) as i32;

    if monster_roll >= party_roll {
        return false;
    }
    if rand() & 7 != 0 {
        return false;
    }
    !blocked
}

#[cfg(test)]
mod monster_escape_tests {
    use super::*;

    fn healthy(hp: u16, max: u16, atk: u16) -> FleeActor {
        FleeActor {
            hp,
            max_hp: max,
            atk,
        }
    }

    #[test]
    fn no_escape_flag_short_circuits_before_any_draw() {
        let mut draws = 0;
        let out = monster_escape_roll(
            1,
            &[healthy(100, 100, 30)],
            &[healthy(80, 100, 40)],
            &[0],
            healthy(80, 100, 40),
            10,
            || {
                draws += 1;
                0
            },
        );
        assert!(!out);
        assert_eq!(draws, 0, "the gate is the first instruction pair");
    }

    #[test]
    fn side_scores_use_the_retail_shifts() {
        let (m, p, blocked) =
            monster_escape_side_scores(&[healthy(64, 128, 32)], &[healthy(200, 400, 90)], &[0]);
        // monster: maxHP + curHP>>1 + ATK
        assert_eq!(m, 400 + 100 + 90);
        // party: maxHP>>3 + curHP>>4 + ATK>>3
        assert_eq!(p, 16 + 4 + 4);
        assert!(!blocked);
    }

    #[test]
    fn dead_monster_contributes_nothing_dead_party_member_doubles() {
        let live = healthy(200, 400, 90);
        let (m_all, _, _) = monster_escape_side_scores(&[healthy(1, 1, 0)], &[live, live], &[0]);
        let (m_one, _, _) =
            monster_escape_side_scores(&[healthy(1, 1, 0)], &[live, healthy(0, 400, 90)], &[0]);
        assert_eq!(m_all, 2 * (400 + 100 + 90));
        assert_eq!(m_one, 400 + 100 + 90);

        // A downed party member doubles whatever the monster loop accumulated.
        let (m_doubled, p, _) = monster_escape_side_scores(
            &[healthy(0, 100, 10), healthy(64, 128, 32)],
            &[live],
            &[0, 0],
        );
        assert_eq!(m_doubled, 2 * (400 + 100 + 90));
        assert_eq!(p, 16 + 4 + 4, "the downed member adds nothing party-side");
    }

    /// A party / monster pair whose only way past the compare is the random
    /// term. The `*3/2` floor puts the monster average at `1.5 * P` where `P` is
    /// the party average, so `party_roll` has to spend more than half its own
    /// modulo range to win: `P + r1%P > 1.5P + r2%(1.5P)`. `r1 = P - 1` with
    /// `r2 = 0` is the widest such gap.
    const PASSING_PARTY: [FleeActor; 1] = [FleeActor {
        hp: 9000,
        max_hp: 9000,
        atk: 9000,
    }];
    const TOKEN_MONSTER: [FleeActor; 1] = [FleeActor {
        hp: 1,
        max_hp: 1,
        atk: 0,
    }];

    /// The party average the pair above produces: `maxHP>>3 + curHP>>4 + ATK>>3`.
    const PASSING_PARTY_AVG: u32 = (9000 >> 3) + (9000 >> 4) + (9000 >> 3);

    #[test]
    fn no_escape_passive_blocks_a_roll_that_otherwise_passes() {
        let roll = |ability: u32| {
            let mut draws = [PASSING_PARTY_AVG - 1, 0, 0].into_iter();
            monster_escape_roll(
                0,
                &PASSING_PARTY,
                &TOKEN_MONSTER,
                &[ability],
                TOKEN_MONSTER[0],
                0,
                || draws.next().unwrap(),
            )
        };
        assert!(roll(0), "unblocked: the rigged compare passes");
        assert!(!roll(NO_ESCAPE_WORD1), "Chicken Guard forces a refusal");
    }

    #[test]
    fn one_in_eight_gate_rejects_every_nonzero_low_three_bits() {
        for third in 0u32..8 {
            let mut draws = [PASSING_PARTY_AVG - 1, 0, third].into_iter();
            let out = monster_escape_roll(
                0,
                &PASSING_PARTY,
                &TOKEN_MONSTER,
                &[0],
                TOKEN_MONSTER[0],
                0,
                || draws.next().unwrap(),
            );
            assert_eq!(out, third & 7 == 0, "third draw {third}");
        }
    }

    #[test]
    fn the_floor_makes_a_zero_random_term_always_lose() {
        // Same pair, but with no help from the first draw the monster side's
        // *3/2 floor wins outright - which is why a monster flee is rare.
        let mut draws = [0u32, 0, 0].into_iter();
        let out = monster_escape_roll(
            0,
            &PASSING_PARTY,
            &TOKEN_MONSTER,
            &[0],
            TOKEN_MONSTER[0],
            0,
            || draws.next().unwrap(),
        );
        assert!(!out);
    }

    #[test]
    fn third_draw_is_skipped_when_the_compare_fails() {
        // A monster side that dwarfs the party side: monster_roll >= party_roll,
        // so retail returns before the `& 7` draw.
        let party = [healthy(8, 8, 0)];
        let monsters = [healthy(9000, 9000, 9000)];
        let mut n = 0;
        let out = monster_escape_roll(0, &party, &monsters, &[0], healthy(8, 8, 0), 0, || {
            n += 1;
            0
        });
        assert!(!out);
        assert_eq!(n, 2, "only the two modulo draws are consumed");
    }

    #[test]
    fn monster_floor_is_at_least_three_halves_of_the_party_average() {
        // party_sum = 8 (maxHP>>3 of 64), monster_sum = 1 -> the floor clamp
        // lifts the monster average to (8*3)>>1 = 12, so the compare fails even
        // though the raw monster score is tiny.
        let party = [healthy(16, 64, 0)];
        let monsters = [healthy(1, 2, 0)];
        let mut n = 0;
        let out = monster_escape_roll(0, &party, &monsters, &[0], healthy(1, 2, 0), 0, || {
            n += 1;
            0
        });
        assert!(
            !out,
            "the *3/2 floor keeps the monster side above the party"
        );
    }
}
