//! PROT 0910 Swordie (`0x88`): four-slash tick and its slash callee.

use super::*;

/// PROT 0910 (`summon_swordie`) tick body - `0x801CF4EC` row `7`
/// (`0x801F1FAC`).
pub const SWORDIE_TICK: u32 = 0x801F_69EC;

/// PROT 0910's per-slash applier, reached by three `jal` sites inside
/// [`SWORDIE_TICK`] (`0x801F78E8`, `0x801F7928`, `0x801F7A08`). It is the only
/// routine in this band that writes HP from outside a tick body, which is why
/// a sweep bounded by the tick's own extent reports PROT 0910 as writing no
/// HP at all.
pub const SWORDIE_SLASH: u32 = 0x801F_81DC;

// ---------------------------------------------------------------------------
// PROT 0910 - Swordie
// ---------------------------------------------------------------------------

/// PROT 0910's settle arm.
pub const SWORDIE_SETTLE_PHASE: u8 = 0x0A;

/// The clip arm `2` stages on the summon seat, with `+0x1DC` **set** to the
/// same value (`sb s8,0x1da(s2)` / `sb s8,0x1dc(s2)` at `0x801F6DB0`, `s8 = 1`).
pub const SWORDIE_ARM2_CLIP: u8 = 1;

/// The clip arm `5` stages, this time with a `+0x1DC` **bump**
/// (`0x801F714C`).
pub const SWORDIE_ARM5_CLIP: u8 = 2;

/// The summon seat's render flag at the end of arm `5` (`li v0,0x7` at
/// `0x801F762C`, stored at `0x801F7634`).
pub const SWORDIE_ARM5_RENDER_FLAG: u8 = 7;

/// How many slashes [`swordie_slash`] runs per cast - the `sltiu s0,0x4`
/// bound on the three `FUN_801F81DC` loops (`0x801F77E8`, `0x801F7934`,
/// `0x801F7A14`).
pub const SWORDIE_SLASHES: u8 = 4;

/// The slash index whose reaction is the knockdown (`li v0,3; bne s7,v0` at
/// `0x801F8930`): only the last one knocks the victim down.
pub const SWORDIE_KNOCKDOWN_SLASH: u8 = 3;

/// The hit counter value at which the clamp becomes kill-capable (`li v0,0x4;
/// beq v1,v0` at `0x801F88CC`, against the module counter at `0x801F8DAC`
/// that the same block just incremented).
pub const SWORDIE_LETHAL_HIT: u32 = 4;

/// Right shift PROT 0910 applies to the damage wrapper's return before it
/// clamps and stores it (`srl s1,s1,2` at `0x801F8898`) - so a Swordie slash
/// deals a quarter of what `FUN_801DD0AC` handed back. See
/// [`swordie_slash`].
pub const SWORDIE_APPLIED_SHIFT: u32 = 2;

/// PROT 0910's module words and the context timer its arms `6..=0x0A` pace
/// on - the state the four slashes live in.
///
/// Retail keeps the first three in the module's own image (`0x801F8D9C`,
/// `0x801F8DAC`, `0x801F8DB0`, all zero in the image as loaded) and the fourth
/// in the battle context (`ctx[+0x6D8]`, a signed halfword the Done band's
/// tail timer also uses). The engine resets it when a cast is armed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SwordieSlashes {
    /// `0x801F8D9C + slash * 4` - each slash's progress. Arm `8` zeroes all
    /// four; [`swordie_slash_step`] grows one by `rate * speed` a call and
    /// lands the hit on the call that carries it to `speed << 6`.
    pub progress: [u32; SWORDIE_SLASHES as usize],
    /// `0x801F8DAC` - hits landed this cast, zeroed by arms `0` and `2`
    /// (`0x801F6C84`, `0x801F6E54`). The fourth landing is the lethal one.
    pub hits: u32,
    /// `0x801F8DB0` - the settle countdown: arm `9` arms it with `speed << 7`
    /// (`0x801F79EC`), arm `0x0A` drains it by `rate * speed` once the last
    /// slash has landed.
    pub countdown: i32,
    /// `ctx[+0x6D8]` - arm `6` loads `speed << 8`, arm `7` drains it until it
    /// goes negative, arm `8` zeroes it and arm `9` grows it by `rate * speed`.
    pub timer: i16,
}

/// The two scratchpad scalars PROT 0910's timers are built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwordieClock {
    /// `0x1F800393` - vsyncs per game tick (`lbu 0x7f(0x1F800314)`).
    pub rate: u8,
    /// `0x1F80037D` - the game-speed scalar (`lbu 0x69(0x1F800314)`); every
    /// threshold scales by it, so the dwell in ticks depends on `rate` only.
    pub speed: u8,
}

impl SwordieClock {
    fn step(self) -> u32 {
        u32::from(self.rate) * u32::from(self.speed)
    }
}

/// PROT 0910 (Swordie, action id `0x88`) tick body.
///
/// A `beq`/`slti` chain at `0x801F6A8C..0x801F6B30` over phases `0..=0x0A`
/// and `0xFF` - one of the three gapless runs in this band (with PROT 0912
/// and 0913; PROT 0909 and 0911 both have holes). `s2 = actor_table[7]` is
/// the summon seat and `s3 = actor_table[caster[+0x1DD]]` the victim, resolved
/// in the prologue behind `sltiu v1,0x8`; a target byte of `8` or more leaves
/// `s3` uninitialised, which is retail's own hole, not the port's.
///
/// Simulation writes, by arm:
///
/// * `0` - `ctx[+0x278] = 0`, hit counter `0x801F8DAC = 0`;
/// * `2` - summon `+0x1DA = 1`, `+0x1DC = 1`, `+0x21C = 0`, `+0x225 = 0`,
///   hit counter `= 0`;
/// * `3` - summon `+0x1DA = 0` when its ramp crosses `0x40`, plus `+0x21C = 0`;
/// * `5` - summon `+0x1DA = 2` (a plain store), `+0x1DC += 1`, `+0x21C = 7`;
/// * `6` - `ctx[+0x6D8] = speed << 8` (`0x801F76A8`);
/// * `7` - drain `ctx[+0x6D8]` by `rate * speed` and **hold** while it is not
///   negative (`bgez` at `0x801F76E0`); then summon `+0x21D = speed`
///   (`0x801F7718`), i.e. the animation rate is set to the game-speed scalar
///   for the strike;
/// * `8` - `ctx[+0x6D8] = 0`, victim `+0x21C = 0`, all four slash progress
///   words zeroed (`0x801F77E0..0x801F77F0`);
/// * `9` - grow `ctx[+0x6D8]` by `rate * speed`; per slash `i`, run
///   [`swordie_slash_step`] if it has started, or once the timer passes
///   `(i + 2) * speed * 16` (`0x801F78D8..0x801F7938`) - so the slashes start
///   staggered and overlap; hold until the timer passes `96 * speed`
///   (`0x801F7960`), then arm the settle countdown with `speed << 7`;
/// * `0x0A` - run all four steps unconditionally (`0x801F7A04..0x801F7A18`);
///   once the last slash has landed, drain the countdown by `rate * speed`;
///   when it is spent, hold until the victim is idle, then write phase `0xFF`;
/// * `0xFF` - return zero.
///
/// With `rate = 4` those gates give arm `7` 65 ticks, arm `9` 25 and each
/// slash 16 - the dwell `docs/subsystems/cast-module.md` measured for this
/// body (arm `7` 65, arm `9` 25), which pins `rate` at retail's battle
/// cadence of four vsyncs per tick. The arms before `6` are not gated here.
///
/// **The tick body itself writes no HP and calls no damage wrapper.** That is
/// what `docs/subsystems/cast-module.md`'s "`0` wrappers, `0` HP" cell
/// measures, and it is true only of the tick's own extent: the cast's damage
/// is [`swordie_slash`], a separate framed routine the tick calls four times.
/// `roll` is that routine's wrapper call (`FUN_801DD0AC(0x12, 7, target)`),
/// drawn only on the call that lands a hit; the returned hits are
/// `(victim_seat, applied)` per landing.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69EC (PROT 0910 tick; phase chain, staging and the arm 6..0xA timers, packet/camera arms unported)
pub fn swordie_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    summon_seat: u8,
    victim_seat: u8,
    slashes: &mut SwordieSlashes,
    clock: SwordieClock,
    mut roll: impl FnMut(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            slashes.hits = 0;
            CastArmStep::Advance
        }
        2 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = SWORDIE_ARM2_CLIP;
                s.restage = SWORDIE_ARM2_CLIP;
                s.render_flag = 0;
            }
            slashes.hits = 0;
            CastArmStep::Advance
        }
        3 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0;
                s.staged_anim = 0;
            }
            CastArmStep::Advance
        }
        5 => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = SWORDIE_ARM5_CLIP;
                s.restage = s.restage.wrapping_add(1);
                s.render_flag = SWORDIE_ARM5_RENDER_FLAG;
            }
            CastArmStep::Advance
        }
        6 => {
            slashes.timer = (u32::from(clock.speed) << 8) as i16;
            CastArmStep::Advance
        }
        7 => {
            slashes.timer = (slashes.timer as u16).wrapping_sub(clock.step() as u16) as i16;
            if slashes.timer >= 0 {
                return CastArmStep::Hold;
            }
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.anim_rate = clock.speed;
            }
            CastArmStep::Advance
        }
        8 => {
            slashes.timer = 0;
            slashes.progress = [0; SWORDIE_SLASHES as usize];
            if let Some(s) = seats.get_mut(victim_seat as usize) {
                s.render_flag = 0;
            }
            CastArmStep::Advance
        }
        9 => {
            slashes.timer = (slashes.timer as u16).wrapping_add(clock.step() as u16) as i16;
            for slash in 0..SWORDIE_SLASHES {
                let started = slashes.progress[slash as usize] != 0;
                // `sltu` against the timer loaded with `lh`: a negative timer
                // reads as a huge unsigned value and opens every gate.
                let gate = (u32::from(slash) + 2) * u32::from(clock.speed) * 16;
                if (started || gate < i32::from(slashes.timer) as u32)
                    && let Some(v) = seats.get_mut(victim_seat as usize)
                    && let Some(applied) =
                        swordie_slash_step(slashes, slash, v, clock, || roll(slash))
                {
                    hits.push(SweepHit {
                        seat: victim_seat,
                        applied,
                    });
                }
            }
            if i32::from(slashes.timer) <= 96 * i32::from(clock.speed) {
                return CastArmStep::Hold;
            }
            slashes.countdown = i32::from(clock.speed) << 7;
            CastArmStep::Advance
        }
        SWORDIE_SETTLE_PHASE => {
            for slash in 0..SWORDIE_SLASHES {
                if let Some(v) = seats.get_mut(victim_seat as usize)
                    && let Some(applied) =
                        swordie_slash_step(slashes, slash, v, clock, || roll(slash))
                {
                    hits.push(SweepHit {
                        seat: victim_seat,
                        applied,
                    });
                }
            }
            let last = slashes.progress[SWORDIE_SLASHES as usize - 1];
            let landed = last >= u32::from(clock.speed) << 6;
            if landed && slashes.countdown > 0 {
                slashes.countdown -= clock.step() as i32;
            }
            if slashes.countdown > 0 {
                return CastArmStep::Hold;
            }
            let settled = seats
                .get(victim_seat as usize)
                .map(|v| v.hp == 0 || v.playing_anim == 0)
                .unwrap_or(true);
            if settled {
                c.phase = SERU_B_DONE_PHASE;
            }
            CastArmStep::Hold
        }
        SERU_B_DONE_PHASE => CastArmStep::Finish,
        1 | 4 => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, hits)
}

/// One call of PROT 0910's `FUN_801F81DC` for `slash`: the progress half of
/// the routine, then [`swordie_slash`] on the call that completes it.
///
/// ```text
/// limit = speed << 6
/// if progress[slash] >= limit { return }          ; 0x801F8264, a finished slash is inert
/// progress[slash] += rate * speed                 ; 0x801F8280
/// if progress[slash] < limit { draw the blade; return }   ; 0x801F8294, FUN_80021B04
/// hits += 1                                       ; 0x801F885C
/// ... land the hit ([`swordie_slash`])
/// ```
///
/// A slash therefore takes `64 / rate` calls from its start to its hit, and
/// the tick keeps calling it every frame after that without effect. The
/// wrapper `roll` is drawn only on the landing call - retail's
/// `jal 0x801DD0AC` sits past the early returns - so a caller's RNG cursor
/// advances once per hit, not once per frame.
///
/// Returns the damage applied on a landing call, `None` otherwise.
///
/// PORT: FUN_801F81DC (PROT 0910; the whole routine: progress accumulator, hit counter, and the landing through `swordie_slash`)
pub fn swordie_slash_step(
    slashes: &mut SwordieSlashes,
    slash: u8,
    victim: &mut CastActorState,
    clock: SwordieClock,
    roll: impl FnOnce() -> i32,
) -> Option<u32> {
    let limit = u32::from(clock.speed) << 6;
    let progress = slashes.progress.get_mut(slash as usize)?;
    if *progress >= limit {
        return None;
    }
    *progress = progress.wrapping_add(clock.step());
    if *progress < limit {
        return None;
    }
    slashes.hits = slashes.hits.wrapping_add(1);
    Some(swordie_slash(victim, slash, slashes.hits, roll()))
}

/// PROT 0910's per-slash applier - three `jal` sites inside [`SWORDIE_TICK`],
/// `slash` being the loop index `0..4` retail passes in `a0`.
///
/// ```text
/// counter = ++module[0x801F8DAC]
/// s1      = FUN_801DD0AC(0x12, 7, caster[+0x1DD])
/// 0x8007BD14 -= s1 - (s1 >> 2)                       ; the running total
/// s1      = s1 >> 2                                  ; 0x801F8898, LOGICAL
/// cap = counter == 4 ? victim[+0x14C] : victim[+0x14C] - 1
/// if cap < s1 { s1 = cap }                           ; UNSIGNED, 0x801F88F0
/// victim[+0x10]  += s1                               ; 0x801F8900
/// victim[+0x14C] -= s1                               ; 0x801F890C
/// ```
///
/// The `srl s1, s1, 2` at `0x801F8898` sits between the two operands of the
/// running-total update and rewrites the **same register** the clamp and both
/// stores then use, so a slash applies a *quarter* of the wrapper's return.
/// It is also what the popup shows: the `jal 0x801F44A0` at `0x801F88C0`
/// takes `(s1 << 16) >> 16` after the shift. Capture: one cast returned
/// `427 / 427 / 380 / 384` from the single site `0x801F887C` and retail took
/// `106 / 106 / 95 / 96` HP.
///
/// The cap switch is the finding this routine carries: the first three
/// slashes clamp to `HP - 1` and **cannot kill**, the fourth clamps to `HP`
/// and can. Kill-capability in this band is therefore per **hit**, not per
/// module - `crate::cast_module_ticks`' entry-keyed
/// `crate::cast_module_ticks::damage_shape_for` cannot express it and does
/// not claim to.
///
/// The reaction is staged with the OR restage form, and the knockdown clip
/// only on `slash == 3`; on every other slash the victim takes the alternate
/// reaction even when the gate `+0x1F2` is set.
///
/// `wrapper_return` is the wrapper's raw return, **before** the shift: the
/// shift is this routine's, not its caller's.
///
/// Returns the damage applied.
///
/// This is the landing half only; [`swordie_slash_step`] is the routine
/// the tick calls, and it passes the incremented counter in.
///
/// REF: FUN_801F69EC, FUN_801F81DC
pub fn swordie_slash(
    victim: &mut CastActorState,
    slash: u8,
    hit_counter: u32,
    wrapper_return: i32,
) -> u32 {
    // `srl` is logical, so a negative return becomes a large positive one
    // rather than staying negative - which matters because the clamp below
    // is unsigned too.
    let scaled = ((wrapper_return as u32) >> SWORDIE_APPLIED_SHIFT) as i32;
    let applied = if hit_counter == SWORDIE_LETHAL_HIT {
        apply_hit_floor_zero(victim, scaled)
    } else {
        apply_hit_unsigned_floor_one(victim, scaled)
    };
    if slash == SWORDIE_KNOCKDOWN_SLASH {
        stage_reaction_or(victim);
    } else {
        // The `bne s7,3` leg skips the knockdown pick entirely and always
        // takes the alternate clip.
        victim.staged_anim = if victim.reaction_alt != 0 {
            victim.reaction_alt
        } else {
            victim.reaction_alt2
        };
        victim.restage |= 0x04;
        victim.restage |= 0x01;
    }
    applied
}
