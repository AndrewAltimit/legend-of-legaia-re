//! Per-round battle bookkeeping: the initiative-key seeder (`FUN_801DA780`)
//! and the agility / action-gauge reset (`FUN_801D88CC`). Split out of
//! `battle_formulas.rs`.
//!
//! These are the two passes that run at a round boundary, and the order is
//! **reset first**: the battle-flow SM calls `FUN_801D88CC` at `801d0ed0` and
//! `FUN_801DA780` at `801d0ed8`. So each actor's spendable AGL is restored and
//! its stale action stream cleared, and only then does the seeder assign every
//! slot the key the turn-order selector (`FUN_801DABA4`, ported as
//! `World::next_combatant_by_initiative`) sorts on.
//!
//! The caller-side sweep is ported as `engine-core`'s
//! `BattleRound::boundary`, which the live battle loop runs at its
//! round boundary.

// ---------------------------------------------------------------------------
// Initiative key seeding (FUN_801DA780)
// ---------------------------------------------------------------------------
//
// Retail address caution: `battle-formulas.md` long attributed the base roll to
// `overlay_0897_801e23ec`. That is an aliased VA - PROT 0897's extraction
// over-reads into 0898 and Ghidra maps it at `0x801C0000` instead of the true
// `0x801CE818`, so every `0x801Fxxxx`/`0x801Exxxx` function it surfaces is a
// different battle-overlay routine. The battle-resident seeder is
// `FUN_801DA780`, the direct caller of `FUN_801DABA4`, and it carries four
// terms the aliased reading missed: the wounded-HP bonus, the Slow halving,
// the ability-bit fast/slow arms, and the one-side lockout.

/// One combatant's inputs to the initiative-key seed (`FUN_801DA780`).
#[derive(Clone, Copy, Debug, Default)]
pub struct InitiativeActor {
    /// Live SPD stat (actor `+0x164`).
    pub speed: u16,
    /// Current HP (actor `+0x14C`).
    pub hp: u16,
    /// Max HP (actor `+0x14E`).
    pub max_hp: u16,
    /// Party slot (`0..=2`) rather than a monster slot (`3..=6`). The wounded
    /// bonus is far more generous for the party - see [`seed_initiative`].
    pub is_party: bool,
    /// Status word `actor+0x16E == 0x1000` (Slow): halves the finished key.
    pub slowed: bool,
    /// The character record's ability bitfield at `+0xF4`. Only meaningful for
    /// party actors; monsters pass `0`. See [`InitiativeAbility`].
    pub ability_bits: u32,
}

/// The three `+0xF4` ability bits `FUN_801DA780` tests. Two of them force the
/// key rather than adjust it, and each arm is gated on the *other* class being
/// absent - so a character carrying both a fast and a slow bit keeps the plain
/// rolled key.
pub struct InitiativeAbility;

impl InitiativeAbility {
    /// `0x8000` - one of the two "always act last" bits.
    pub const SLOW_A: u32 = 0x0000_8000;
    /// `0x40000` - the other "always act last" bit.
    pub const SLOW_B: u32 = 0x0004_0000;
    /// `0x20000` - "always act first": adds `0x1000`, far above any rolled key.
    pub const FAST: u32 = 0x0002_0000;
    /// Either slow bit.
    pub const SLOW_MASK: u32 = Self::SLOW_A | Self::SLOW_B;
}

/// The key a forced-slow actor is pinned to - below every rolled key (which is
/// always `>= 1` and normally `>= speed`), so the actor acts last.
pub const INITIATIVE_SLOW_KEY: u16 = 1;
/// The bonus a forced-fast actor adds, above the reachable rolled range.
pub const INITIATIVE_FAST_BONUS: u16 = 0x1000;

/// Seed one actor's initiative key (actor `+0x16C`).
///
/// ```text
/// key  = speed + roll + 1                     roll = rand() % (speed/2 + 1)
/// key += wounded_bonus(actor)
/// key  = key >> 1                             if Slow (+0x16E == 0x1000)
/// key  = 1                                    if slow-bit && !fast-bit
/// key += 0x1000                               if fast-bit && !slow-bit
/// ```
///
/// The **wounded bonus** rewards being hurt, and the party's schedule is three
/// bands deep against the monsters' flat one:
///
/// | side | condition | bonus |
/// |---|---|---|
/// | party | `hp < max_hp/4` | `(max_hp - hp) >> 4` |
/// | party | `hp < max_hp/2` | `(max_hp - hp) >> 5` |
/// | party | otherwise | `(max_hp - hp) >> 6` |
/// | monster | always | `(max_hp - hp) >> 10` |
///
/// A near-dead party member therefore gets a real turn-order edge (a `>> 4` on
/// three-quarters of a large HP pool is worth more than the SPD roll itself),
/// while a wounded monster's `>> 10` is almost always zero. Callers supply
/// `roll` so the RNG draw order stays the caller's business - retail draws it
/// from `func_0x80056798` once per slot, in slot order.
///
/// PORT: FUN_801da780 (per-actor scoring; the slot sweep, the `ctx+0x290`
/// side lockout - [`apply_side_lockout`] - and the scripted boss orders stay
/// with the caller)
pub fn seed_initiative(actor: &InitiativeActor, roll: u16) -> u16 {
    let mut key = actor.speed.wrapping_add(roll).wrapping_add(1);
    key = key.wrapping_add(wounded_bonus(actor));
    if actor.slowed {
        key >>= 1;
    }
    let slow = actor.ability_bits & InitiativeAbility::SLOW_MASK != 0;
    let fast = actor.ability_bits & InitiativeAbility::FAST != 0;
    if slow && !fast {
        INITIATIVE_SLOW_KEY
    } else if fast && !slow {
        key.wrapping_add(INITIATIVE_FAST_BONUS)
    } else {
        key
    }
}

/// The missing-HP term of [`seed_initiative`]. Saturating, so a corrupt
/// `hp > max_hp` yields `0` rather than wrapping into a huge bonus.
pub fn wounded_bonus(actor: &InitiativeActor) -> u16 {
    let deficit = u32::from(actor.max_hp.saturating_sub(actor.hp));
    let shift = if actor.is_party {
        if u32::from(actor.hp) < u32::from(actor.max_hp >> 2) {
            4
        } else if u32::from(actor.hp) < u32::from(actor.max_hp >> 1) {
            5
        } else {
            6
        }
    } else {
        10
    };
    (deficit >> shift).min(u32::from(u16::MAX)) as u16
}

/// The modulus of the seeder's RNG draw: `rand() % (speed/2 + 1)`. Never zero,
/// so a zero-SPD actor still rolls cleanly (the draw just contributes nothing).
pub fn initiative_roll_modulus(speed: u16) -> u32 {
    u32::from(speed / 2) + 1
}

/// The formation advantage `ctx+0x290` records - which side got the drop on
/// the other, and therefore which side loses its whole first round.
///
/// The formation roll ([`roll_formation_advantage`], `FUN_80051D84`) writes
/// this byte at battle setup; `FUN_801E295C` state `0x00` latches it into
/// `ctx+0x291` and clears the original. Two consumers read it, and they read
/// *different* copies:
///
/// - the initiative seeder ([`apply_side_lockout`]) reads `+0x290` and zeroes
///   the disadvantaged side's keys, so that side sits out round one;
/// - the escape roll reads the latched `+0x291` and, on
///   [`FormationAdvantage::Preemptive`], sets the party roll equal to the enemy
///   roll so the `roll_p < roll_e` compare cannot fail.
///
/// The shorthand for that second arm is "escape assured", and it overstates
/// what the code does. `FUN_801E791C` applies it at `801e7af0` and only *then*
/// tests the scripted no-escape flag `ctx+0x287` at `801e7b14`, so a pre-emptive
/// strike into a no-flee battle is still caught. Only the forced-flee arm
/// (`_DAT_8007bac0 & 0x100`, which sets the routine's mode word to `2`) bypasses
/// `+0x287`.
///
/// Because the escape roll reads the *latched* copy, the latch in state `0x00`
/// is load-bearing: dropping it (clearing `+0x290` without copying it first)
/// silently disables pre-emptive-strike escapes for the whole battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormationAdvantage {
    /// `ctx+0x290 == 0` - neither side surprised the other.
    None,
    /// `ctx+0x290 == 1` - **back attack**: the monsters got the drop. Party
    /// slots `0..=2` are keyed `0` and turned to face `0x800` (180°).
    BackAttack,
    /// `ctx+0x290 == 2` - **pre-emptive strike**: the party got the drop.
    /// Monster slots `3..=6` are keyed `0` and turned to face `0`.
    Preemptive,
}

/// Alias kept for the initiative-side reading of the same byte.
pub type SideLockout = FormationAdvantage;

impl FormationAdvantage {
    /// Decode the raw `ctx+0x290` / `ctx+0x291` byte. Values other than `1`/`2`
    /// are [`FormationAdvantage::None`].
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => FormationAdvantage::BackAttack,
            2 => FormationAdvantage::Preemptive,
            _ => FormationAdvantage::None,
        }
    }

    /// Raw byte for this advantage.
    pub fn to_byte(self) -> u8 {
        match self {
            FormationAdvantage::None => 0,
            FormationAdvantage::BackAttack => 1,
            FormationAdvantage::Preemptive => 2,
        }
    }

    /// True when the slot's initiative key must be zeroed. Party slots are
    /// `0..=2`, monster slots `3..=6`.
    pub fn zeroes_slot(self, slot: u8) -> bool {
        match self {
            FormationAdvantage::None => false,
            FormationAdvantage::BackAttack => slot < 3,
            FormationAdvantage::Preemptive => (3..=6).contains(&slot),
        }
    }

    /// The facing (`actor+0x46`) the disadvantaged side is turned to. A back
    /// attack spins the **party** around to `0x800` (180° in the 4096-step
    /// angle space - they were caught looking the wrong way); a pre-emptive
    /// strike faces the **monsters** to `0`.
    pub fn loser_facing(self) -> Option<u16> {
        match self {
            FormationAdvantage::None => None,
            FormationAdvantage::BackAttack => Some(0x800),
            FormationAdvantage::Preemptive => Some(0),
        }
    }
}

/// The two `+0xF8` ability bits that bias the formation roll, and the RNG
/// moduli they swap in.
pub struct FormationAbility;

impl FormationAbility {
    /// `+0xF8` bit 18 - improves the party's odds of a pre-emptive strike:
    /// multiplies the party score by 3/2 **and** drops the pre-emptive RNG
    /// modulus from 16 to 2 (a 1-in-2 gate instead of 1-in-16).
    pub const PREEMPTIVE: u32 = 0x0004_0000;
    /// `+0xF8` bit 19 - guards against back attacks: halves the enemy score
    /// **and** raises the back-attack RNG modulus from 16 to 64.
    pub const GUARD_BACK: u32 = 0x0008_0000;
}

/// Default RNG modulus for both formation gates (a 1-in-16 chance).
pub const FORMATION_MOD_DEFAULT: u32 = 16;
/// Pre-emptive modulus with [`FormationAbility::PREEMPTIVE`] equipped.
pub const FORMATION_MOD_PREEMPTIVE: u32 = 2;
/// Back-attack modulus with [`FormationAbility::GUARD_BACK`] equipped.
pub const FORMATION_MOD_GUARD_BACK: u32 = 64;

/// Inputs to [`roll_formation_advantage`] that don't come from the stat sums.
#[derive(Clone, Copy, Debug, Default)]
pub struct FormationInputs {
    /// OR of the living party members' `+0xF8` ability words. See
    /// [`FormationAbility`].
    pub ability_bits: u32,
    /// The formation's first monster id (`DAT_8007BD0C`).
    pub monster_id: u8,
    /// The current map id (`_DAT_80084540`). Only `0x0C` and `0x15` matter -
    /// they are the two maps carrying scripted ambushes.
    pub map_id: u16,
}

/// Roll the battle's formation advantage (`FUN_80051D84`).
///
/// Both sides' **average** SPD is compared, each blurred by a random spread, and
/// the winner still has to pass a rarity gate:
///
/// ```text
/// p = mean(party SPD);  e = mean(enemy SPD)
/// a = p + rand() % (2*|p - e|)          // party score
/// b = e + rand() % (2*|a - e|)          // enemy score, spread about the *party* score
/// a += a >> 1        if PREEMPTIVE bit  // 3/2
/// b -= b >> 1        if GUARD_BACK bit  // 1/2
/// back attack   if a < b && rand() % mod_back == 0
/// pre-emptive   if b < a && rand() % mod_pre  == 0
/// ```
///
/// Note `b`'s spread is taken about `|a - e|`, i.e. the *already-rolled* party
/// score - not about `|p - e|`. The two draws are therefore correlated, and a
/// party that rolls high widens the enemy's spread as well.
///
/// Two scripted overrides force a back attack regardless of the scores: the
/// monster ids `0x3D..=0x3F` on maps `0x0C` / `0x15`, and the monster id `0xA7`
/// anywhere. The map-gated arm additionally sets `_DAT_8007BAC0 |= 0x200`,
/// which stays with the caller.
///
/// Retail skips the whole roll when the battle carries the scripted no-escape
/// flag (`ctx+0x287`) or `DAT_8007B64A` is set; callers gate on that before
/// calling. Draws are taken in retail order, so `rand` must be the same
/// generator the rest of the battle shares.
///
/// PORT: FUN_80051d84
pub fn roll_formation_advantage(
    party_spd: &[u16],
    enemy_spd: &[u16],
    inputs: &FormationInputs,
    rand: &mut dyn FnMut() -> u32,
) -> FormationAdvantage {
    // Scripted back attacks bypass the score comparison entirely.
    let scripted_ambush = (matches!(inputs.map_id, 0x0C | 0x15)
        && (0x3D..=0x3F).contains(&inputs.monster_id))
        || inputs.monster_id == 0xA7;

    if party_spd.is_empty() || enemy_spd.is_empty() {
        return if scripted_ambush {
            FormationAdvantage::BackAttack
        } else {
            FormationAdvantage::None
        };
    }

    let mean = |v: &[u16]| -> u32 { v.iter().map(|&s| u32::from(s)).sum::<u32>() / v.len() as u32 };
    let p = mean(party_spd);
    let e = mean(enemy_spd);

    // Zero-spread guard. This is a **port-side** decision, not a retail one:
    // the two spread divides at `80051f04` / `80051f28` are bare `div v0,v1`
    // with no divisor test and no `break`, unlike the escape roll's divides
    // (`801e7a90` / `801e7aac`), which the compiler *did* guard with
    // `bne <divisor>,zero` + `break 0x1c00`. A zero divisor on the R3000 does
    // not trap - it is architecturally UNPREDICTABLE, and on the hardware
    // leaves `HI` = the dividend, so retail would fold the raw `rand()` draw
    // in as the spread. Rust's `%` would panic instead, so the port returns a
    // deterministic `0`: a zero spread means the two sides are dead level and
    // contributing the full draw would be noise, not fidelity.
    let spread = |lo: u32, hi: u32, rand: &mut dyn FnMut() -> u32| -> u32 {
        let span = lo.abs_diff(hi) * 2;
        if span == 0 { 0 } else { rand() % span }
    };

    let mut a = p + spread(p, e, rand);
    let mut b = e + spread(a, e, rand);

    let pre_bit = inputs.ability_bits & FormationAbility::PREEMPTIVE != 0;
    let back_bit = inputs.ability_bits & FormationAbility::GUARD_BACK != 0;
    if pre_bit {
        a += a >> 1;
    }
    if back_bit {
        b -= b >> 1;
    }
    let mod_pre = if pre_bit {
        FORMATION_MOD_PREEMPTIVE
    } else {
        FORMATION_MOD_DEFAULT
    };
    let mod_back = if back_bit {
        FORMATION_MOD_GUARD_BACK
    } else {
        FORMATION_MOD_DEFAULT
    };

    if (a < b && rand().is_multiple_of(mod_back)) || scripted_ambush {
        FormationAdvantage::BackAttack
    } else if b < a && rand().is_multiple_of(mod_pre) {
        FormationAdvantage::Preemptive
    } else {
        FormationAdvantage::None
    }
}

/// Apply the `ctx+0x290` side lockout over a seeded 7-slot key table, in place.
///
/// NOT WIRED - **and not wirable as written**, which is a statement about the
/// engine's seating rather than about a missing caller. The round-boundary
/// pass this belongs to *does* exist (`engine-core`'s `BattleRound::boundary`
/// plus the reseed that follows it); what cannot be reused is the slot split.
/// This function hardcodes retail's **fixed** boundary (party `0..=2`,
/// monsters `3..=6`), because retail reserves three party slots whatever the
/// party size. `engine-core` compacts instead - `enter_battle` seats the first
/// monster at `party_count`, so slot 1 can be a monster - and
/// `World::reseed_initiative` therefore applies the same rule against its own
/// `party_count` boundary. Calling this instead would lock out the wrong side
/// for any party smaller than three. Kept as the retail-layout reference the
/// engine's adapter is checked against.
///
/// PORT: FUN_801da780 (the lockout sweep that runs after the per-slot scoring)
pub fn apply_side_lockout(keys: &mut [u16; 7], lockout: SideLockout) {
    for (slot, key) in keys.iter_mut().enumerate() {
        if lockout.zeroes_slot(slot as u8) {
            *key = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Per-round agility reset (FUN_801D88CC)
// ---------------------------------------------------------------------------

/// Cap on a spirit-charged actor's restored AGL (`0x120`) - the same ceiling
/// the spirit-damage formula uses.
pub const SPIRIT_AGL_CAP: u16 = 0x120;

/// Per-round AGL / action-gauge restore (`FUN_801D88CC`, loop A).
///
/// Each round every actor's spendable AGL (`+0x154`) is restored from its base
/// (`+0x156`) - but the arm that fires depends on the actor's action state:
///
/// - **spirit-charged** (`+0x1DE == 4`, or the `+0x1F9` charge byte non-zero):
///   restore to `(base * 7) / 5 + 8`, capped at [`SPIRIT_AGL_CAP`] - a ~40%
///   over-restore, the same shape as the spirit-damage formula.
/// - **plain reset** (`+0x1DE == 3`, or any monster slot `>= 3`): restore to
///   `base`.
/// - **otherwise**: *no reset at all* - a party actor in action state `< 3`
///   carries its remaining AGL into the next round rather than refilling.
///
/// That last arm is easy to miss and is the reason a party member who has been
/// spending AGL mid-combo does not silently refill.
///
/// The caller side - the slot sweep, the `+0x1DF..+0x1EE` action-stream clear
/// and loop B's dead-target re-pick (see [`needs_retarget`]) - is
/// `engine-core`'s `BattleRound::boundary`, which the live battle loop runs
/// when a round ends. The gauge it writes is the battle actor's `+0x154`, and
/// the enemy swing-budget loop spends it.
///
/// The ctx header writes stay with retail's own caller; the engine's boundary
/// hook sits before its active-actor cursor advances, so it has nothing to
/// reset there.
///
/// PORT: FUN_801d88cc (loop A's AGL arm)
pub fn round_reset_agility(cur: u16, base: u16, spirit_charged: bool, plain_reset: bool) -> u16 {
    if spirit_charged {
        let boosted = (u32::from(base) * 7) / 5 + 8;
        boosted.min(u32::from(SPIRIT_AGL_CAP)) as u16
    } else if plain_reset {
        base
    } else {
        cur
    }
}

/// Loop B's re-target predicate (`FUN_801D88CC`): a party actor re-picks its
/// target when the stored slot byte (`+0x1DD`) is out of the `0..=6` slot range
/// or the actor it names is dead (`+0x14C == 0`).
///
/// This predicate is only the "is the stored target still usable" half; the
/// re-pick is `FUN_801DB8B4`. That routine is **not** an RNG-backed picker -
/// an earlier reading here said it was, and the disassembly falsifies it. All
/// 16 of its instructions are a linear scan:
///
/// ```text
/// 801db8b4  li    v1,0x3            ; start at the first monster slot
/// 801db8bc  addiu v0,v0,-0x6c90     ; &DAT_801C9370
/// 801db8c0  addiu a0,v0,0xc         ; ... + 3 pointers
/// 801db8cc  lhu   v0,0x14c(v0)      ; candidate's liveness
/// 801db8d4  bne   v0,zero,0x801db8ec; first living slot wins
/// 801db8e0  sltiu v0,v1,0x7         ; while slot < 7
/// 801db8f0  _move v0,v1             ; returns 7 when the band is wiped
/// ```
///
/// So a party actor whose target died re-points at the **lowest** living
/// monster slot deterministically, and the routine draws nothing from the RNG.
///
/// Wired through `engine-core`'s `BattleRound::boundary` (loop B), which walks
/// the party band only - retail's loop bound is `s1+0xc`, three pointers.
pub fn needs_retarget(target_slot: u8, target_hp: u16) -> bool {
    target_slot > 6 || target_hp == 0
}

/// The `+0x1DF..+0x1EE` window loop A zeroes for every actor each round - the
/// per-action parameter stream (16 bytes). `+0x1DF` is the action id the
/// move-power table is indexed by, so this clear is what makes a stale action
/// unreadable after the round ends.
pub const ACTION_STREAM_RANGE: std::ops::Range<usize> = 0x1DF..0x1EF;

// ---------------------------------------------------------------------------
// Per-round status-0x400 RNG waker (FUN_801F45A4)
// ---------------------------------------------------------------------------

/// The status halfword bit the per-round waker services (battle actor
/// `+0x16E` bit `0x400` - the latent guard-disabling status; no retail
/// applier sets it, see `docs/subsystems/battle.md`).
pub const STATUS_BIT_0X400: u16 = 0x400;

/// Per-round RNG waker for status bit `0x400`, one actor slot's step.
///
/// Retail loops the 7 battle-actor slots (`&DAT_801C9370`); for each live
/// actor (`+0x14C != 0`) whose `+0x16E` status halfword carries bit `0x400`
/// it rolls the shared RNG (`FUN_80056798`) and, when `rng & 7 == 0`, clears
/// exactly that bit (`andi 0xFBFF` at `0x801F4610`, `sh` at `0x801F4614`).
/// The RNG is consumed **only** for live afflicted actors - callers must not
/// pre-roll for empty slots or the stream desyncs.
///
/// Returns the new status halfword, or `None` when nothing changes (dead /
/// empty slot, bit clear, or the 1-in-8 roll misses).
///
/// PORT: FUN_801F45A4
pub fn status_0x400_wakes(status: u16, alive: bool, mut roll: impl FnMut() -> u16) -> Option<u16> {
    if !alive || status & STATUS_BIT_0X400 == 0 {
        return None;
    }
    if roll() & 7 == 0 {
        Some(status & !STATUS_BIT_0X400)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Target framing (FUN_801F0348)
// ---------------------------------------------------------------------------

/// Battle camera height / distance derived from a monster's size class
/// (monster record `+0x1F`), written to `ctx+0x6D0`.
///
/// `clamp(size_class << 7, 0x0C00, 0x1400)`: the default `0x0C00` is also the
/// floor, so only monsters with a size class above `0x18` pull the camera back
/// at all, and everything from `0x28` up saturates at `0x1400`.
///
/// Retail resolves the size byte from the acting actor's target slot
/// (`+0x1DD`), then *overwrites* it with the acting actor's own size when the
/// actor is itself a monster - so a monster's attack frames on the attacker's
/// bulk, not the target's. That second store really does clobber the first; it
/// is not a decompiler artifact.
///
/// This is only the arithmetic; [`camera_height_for_frame`] is the whole
/// routine including the slot gating.
///
/// PORT: FUN_801f0348 (the `<< 7` + clamp arithmetic)
pub fn camera_height_from_size_class(size_class: u8) -> i16 {
    (i16::from(size_class) << 7).clamp(CAMERA_HEIGHT_MIN, CAMERA_HEIGHT_MAX)
}

/// Floor for `ctx+0x6D0`, and also the value the routine seeds it with before
/// any lookup runs.
pub const CAMERA_HEIGHT_MIN: i16 = 0x0C00;
/// Ceiling for `ctx+0x6D0`.
pub const CAMERA_HEIGHT_MAX: i16 = 0x1400;

/// The whole of `FUN_801F0348`: resolve the battle camera's height/distance
/// (`ctx+0x6D0`) from the acting actor's slot and its target slot.
///
/// ```text
/// 801f0358  sh   v0,0x6d0(a1)        ; seed 0x0C00 unconditionally
/// 801f0374  lbu  v1,0x1dd(a0)        ; target slot
/// 801f037c  sltiu v0,v1,0x8
/// 801f0380  beq  v0,zero,0x801f0404  ; target >= 8 -> straight to the clamp
/// 801f0384  _sltiu v0,v1,0x3
/// 801f0388  bne  v0,zero,0x801f03bc  ; target < 3 (party) -> skip target arm
/// ...       ctx+0x6D0 = monster[target-3].size << 7
/// 801f03cc  sltiu v0,v0,0x3
/// 801f03d0  bne  v0,zero,0x801f0404  ; attacker < 3 (party) -> clamp
/// ...       ctx+0x6D0 = monster[attacker-3].size << 7   (clobbers the above)
/// ```
///
/// The `< 8` test is the routine's **outer gate**, and it is wider than it
/// looks: its branch target is the clamp, so a target slot of `8` or above
/// skips the attacker-side lookup as well. A monster attacking with a
/// nonsense target byte therefore frames at the bare `0x0C00` default rather
/// than on its own bulk - the one path where the attacker's size is ignored.
/// Slot bytes are only ever `0..=6` in a live battle, so the gate is a
/// robustness guard against a stale or uninitialised `+0x1DD`; it is modelled
/// here because "unmodelled" and "unreachable" are different claims and only
/// the disassembly settles which one this is.
///
/// `size_of(slot)` returns the monster record's `+0x1F` size byte
/// ([`legaia_asset::monster_archive::MonsterRecord::size_class`]) for a monster
/// slot; it is never called for a party slot.
///
/// `monster_slot_base` is the first slot of the monster band - the value the
/// disassembly hardcodes as `3` ([`RETAIL_MONSTER_SLOT_BASE`], because retail
/// reserves three party slots whatever the party size). `engine-core` compacts
/// its seating instead and seats the first monster at `party_count`, so it
/// passes its own boundary; this is the same split
/// [`apply_side_lockout`] documents from the other side. Both readings agree
/// whenever the party is three, which is every retail battle beyond the
/// prologue.
///
/// Called at [`ActionSeed`](crate::battle_action::ActionState), which is where
/// `FUN_801E295C` calls it (`801e2d2c`, unconditionally, just ahead of the
/// gated `jal 0x801efe44` that the engine's `BattleActionHost::camera_bounds`
/// hook stands in for).
///
/// PORT: FUN_801f0348
pub fn camera_height_for_frame(
    attacker_slot: u8,
    target_slot: u8,
    monster_slot_base: u8,
    size_of: impl Fn(u8) -> u8,
) -> i16 {
    let mut h = CAMERA_HEIGHT_MIN;
    // Outer gate: an out-of-range target byte skips both lookups entirely.
    if target_slot < 8 {
        if target_slot >= monster_slot_base {
            h = i16::from(size_of(target_slot)) << 7;
        }
        if attacker_slot >= monster_slot_base {
            // Retail re-reads the acting slot and overwrites the target-derived
            // value - a monster frames on its own bulk.
            h = i16::from(size_of(attacker_slot)) << 7;
        }
    }
    h.clamp(CAMERA_HEIGHT_MIN, CAMERA_HEIGHT_MAX)
}

/// The monster-band base slot the disassembly hardcodes (`sltiu v0,v1,0x3` at
/// `801f0384` / `801f03cc`). Retail reserves party slots `0..=2` unconditionally.
pub const RETAIL_MONSTER_SLOT_BASE: u8 = 3;
