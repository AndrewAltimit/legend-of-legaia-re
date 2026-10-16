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

/// PROT 0910's slash arm: the timer gates that start the four slashes and
/// the first landings run here (`0x801F78D8..0x801F7938`), so the cast's
/// outcome is owed once the phase has passed it.
pub const SWORDIE_SLASH_ARM: u8 = 9;

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

/// The PROT entry of Swordie's module (`summon_swordie`).
pub const SWORDIE_ENTRY: u32 = 910;

/// PROT 0910's opaque split-panel builder, `FUN_801F7C18`: two `POLY_G4`
/// from the vertex tables at `0x801F8D6C` / `0x801F8D7C`. Arms `0..=4` call
/// it on every pass, held or not.
pub const SWORDIE_SPLIT_ROUTINE: u32 = 0x801F_7C18;

/// PROT 0910's slash-streak builder, `FUN_801F7DD0`, called only by arm `3`
/// (`jal 0x801F7DD0` at `0x801F6F3C`), ahead of the split.
pub const SWORDIE_STREAK_ROUTINE: u32 = 0x801F_7DD0;

/// The arm that ramps the streak down the split line (`0x801F6E70`).
pub const SWORDIE_STREAK_ARM: u8 = 3;

/// The arm that slides the two panels apart (`0x801F6F64`).
pub const SWORDIE_SPLIT_ARM: u8 = 4;

/// Arm `3` holds while the split counter is below this (`sltiu v0,v0,0x40`
/// at `0x801F6F10`), then clamps it to `0x3F` and advances.
pub const SWORDIE_STREAK_END: u32 = 0x40;

/// Arm `4` holds while the split counter is at most this (`sltiu v0,v0,0x81`
/// at `0x801F7040`).
pub const SWORDIE_SPLIT_END: u32 = 0x80;

/// Arm `5` holds while the split counter is below this (`sltiu v0,v0,0x169`
/// at `0x801F7128`).
pub const SWORDIE_ARM5_END: u32 = 0x169;

/// The two split panels as arm `0` seeds them (`0x801F6C20..0x801F6C70`):
/// left trapezoid `(0,0) (0x80,0) (0,0xE0) (0xC0,0xE0)`, right trapezoid
/// `(0x80,0) (0x140,0) (0xC0,0xE0) (0x140,0xE0)` - the whole 320x224 frame
/// cut along the line `(0x80,0)..(0xC0,0xE0)`. Each is the table's eight
/// halfwords in packet order, `x0 y0 x1 y1 x2 y2 x3 y3`.
pub const SWORDIE_PANELS_AT_REST: [[i16; 8]; 2] = [
    [0, 0, 0x80, 0, 0, 0xE0, 0xC0, 0xE0],
    [0x80, 0, 0x140, 0, 0xC0, 0xE0, 0x140, 0xE0],
];

/// The arm-`4` panel tables for split counter `c` (`0x801F6F90..0x801F7034`):
/// the left panel slides down-left (`x - 4c` / `x - 2c`, `y + 8c` .. `y +
/// 2c`), the right one down-right, each corner at its own rate, so the cut
/// opens as the two halves fall away off screen. Halfword stores, so every
/// value wraps to `i16`.
pub fn swordie_split_panels(c: u32) -> [[i16; 8]; 2] {
    let c = c as i32;
    let h = |v: i32| v as i16;
    [
        [
            h(-4 * c),
            h(8 * c),
            h(0x80 - 4 * c),
            h(4 * c),
            h(-2 * c),
            h(4 * c + 0xE0),
            h(0xC0 - 2 * c),
            h(2 * c + 0xE0),
        ],
        [
            h(6 * c + 0x80),
            h(3 * c),
            h(4 * c + 0x140),
            h(4 * c),
            h(2 * c + 0xC0),
            // `lhu` of the counter word plus `0xE0`.
            h((c & 0xFFFF) + 0xE0),
            h(4 * c + 0x140),
            h(2 * c + 0xE0),
        ],
    ]
}

/// What PROT 0910's packet builders drew on one pass of the tick: both are
/// screen-space and read nothing but the module words they are handed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SwordieDraw {
    /// `FUN_801F7C18` ran over these panel tables.
    pub split: Option<[[i16; 8]; 2]>,
    /// `FUN_801F7DD0` ran with this split counter.
    pub streak: Option<u32>,
    /// The slashes whose `FUN_801F81DC` call drew the blade
    /// ([`SWORDIE_BLADE_RECORD`]) on this pass: still travelling after the
    /// call's progress step.
    pub blades: [bool; SWORDIE_SLASHES as usize],
}

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
    /// `0x801F8D8C` - the split counter arms `3`, `4` and `5` ramp by
    /// `(rate * speed) / 4` a pass; arm `0` zeroes it and arms `3` and `4`
    /// zero it on their way out.
    pub split: u32,
    /// `0x801F8D6C` / `0x801F8D7C` - the two split panels' vertex tables
    /// (arm `0` seeds [`SWORDIE_PANELS_AT_REST`], arm `4` rewrites them from
    /// the counter, [`swordie_split_panels`]).
    pub panels: [[i16; 8]; 2],
    /// What the last pass drew ([`SwordieDraw`]).
    pub draw: SwordieDraw,
    /// `0x801F8D90` / `0x801F8D92` - the plane pair the slashes fly from,
    /// stored by arm 8 ([`swordie_blade_origin`]). The tick body has no seat
    /// positions; the host writes it on arm 8's pass.
    pub origin: (i16, i16),
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

    /// The split counter's per-pass step, `(rate * speed) / 4` (`mult`,
    /// `bgez` / `addiu 3` / `sra 2`; the product is never negative).
    fn split_step(self) -> u32 {
        self.step() >> 2
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
/// * `0` - `ctx[+0x278] = 0`, hit counter `0x801F8DAC = 0`, split counter
///   `0x801F8D8C = 0`, the split panels seeded ([`SWORDIE_PANELS_AT_REST`]);
/// * `2` - summon `+0x1DA = 1`, `+0x1DC = 1`, `+0x21C = 0`, `+0x225 = 0`,
///   shatter `+0x1BA = 0`, hit and split counters `= 0`;
/// * `3` - summon `+0x21C = 0` and shatter `+0x1BA = 0` every pass; the split counter grows by
///   `(rate * speed) / 4` and the arm **holds** below `0x40`
///   ([`SWORDIE_STREAK_END`]); the crossing pass clamps it to `0x3F`, stores
///   summon `+0x1DA = 0`, clears its loop counter `+0x21B` / `+0x176` and
///   advances, and the counter is zeroed after the
///   pass's draws;
/// * `4` - the counter grows the same way, the panel tables are rewritten
///   from it ([`swordie_split_panels`]) and the arm holds while it is at most
///   `0x80` ([`SWORDIE_SPLIT_END`]), then zeroes it and advances;
/// * `5` - the counter grows again and the arm holds below `0x169`
///   ([`SWORDIE_ARM5_END`]); then summon `+0x1DA = 2` (a plain store),
///   `+0x1DC += 1`, `+0x21C = 7`;
/// * `6` - `ctx[+0x6D8] = speed << 8` (`0x801F76A8`);
/// * `7` - drain `ctx[+0x6D8]` by `rate * speed` and **hold** while it is not
///   negative (`bgez` at `0x801F76E0`); then summon `+0x21B = 0`,
///   `+0x176 = 0` and `+0x21D = speed`
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
/// cadence of four vsyncs per tick. Arms `3`, `4` and `5` take 8, 17 and 46
/// passes at a step of `32`, the measured `8`, `17` and `46`. Arm `1`'s
/// stream request and arm `2`'s poll pass at once (the engine's stream is
/// resident).
///
/// The packet builders are the module's own screen-space draws:
/// [`SwordieSlashes::draw`] records which ran on the pass and over what -
/// the split panels on every pass of arms `0..=4`, the streak on every pass
/// of arm `3` - and `legaia_render_kernels::cast_swordie` builds them.
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
/// PORT: FUN_801F69EC (PROT 0910 tick; phase chain, staging, the arm 3..5 split counter and the arm 6..0xA timers, packet-builder calls; camera arms unported)
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
    slashes.draw = SwordieDraw::default();
    let step = run_latched(ctx, |c| match c.phase {
        0 => {
            c.ctx_278 = 0;
            slashes.panels = SWORDIE_PANELS_AT_REST;
            slashes.split = 0;
            slashes.hits = 0;
            slashes.draw.split = Some(slashes.panels);
            CastArmStep::Advance
        }
        // Arm 1's stream request and arm 2's poll pass at once here; both
        // draw the panels first (`jal 0x801F7C18` at `0x801F6C90` /
        // `0x801F6CD4`).
        1 => {
            slashes.draw.split = Some(slashes.panels);
            CastArmStep::Advance
        }
        2 => {
            slashes.draw.split = Some(slashes.panels);
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.staged_anim = SWORDIE_ARM2_CLIP;
                s.restage = SWORDIE_ARM2_CLIP;
                s.render_flag = 0;
                // The creature appears whole: `sh zero,0x1ba(s2)` at
                // `0x801F6E28`.
                s.shatter = 0;
            }
            slashes.split = 0;
            slashes.hits = 0;
            CastArmStep::Advance
        }
        // The streak runs down the cut while the counter climbs to `0x40`:
        // the crossing pass clamps it to `0x3F`, clears the clip and
        // advances, and the streak and the panels draw on every pass, the
        // crossing one included, before the counter is zeroed for arm 4.
        SWORDIE_STREAK_ARM => {
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.render_flag = 0;
                // `sh zero,0x1ba(s2)` at `0x801F6E8C`.
                s.shatter = 0;
            }
            slashes.split = slashes.split.wrapping_add(clock.split_step());
            let crossed = slashes.split >= SWORDIE_STREAK_END;
            if crossed {
                slashes.split = SWORDIE_STREAK_END - 1;
                // `sb zero,0x1da` / `sb zero,0x21b` / `sh zero,0x176` at
                // `0x801F6F20..0x801F6F28`: back to idle, and the park of
                // the clip arm 2 staged released so its cursor runs on to
                // the natural end that commits the idle.
                if let Some(s) = seats.get_mut(summon_seat as usize) {
                    s.staged_anim = 0;
                    s.loop_count = 0;
                    s.loop_timer = 0;
                }
            }
            slashes.draw.streak = Some(slashes.split);
            slashes.draw.split = Some(slashes.panels);
            if crossed {
                slashes.split = 0;
                CastArmStep::Advance
            } else {
                CastArmStep::Hold
            }
        }
        SWORDIE_SPLIT_ARM => {
            slashes.split = slashes.split.wrapping_add(clock.split_step());
            slashes.panels = swordie_split_panels(slashes.split);
            slashes.draw.split = Some(slashes.panels);
            if slashes.split <= SWORDIE_SPLIT_END {
                return CastArmStep::Hold;
            }
            slashes.split = 0;
            CastArmStep::Advance
        }
        5 => {
            slashes.split = slashes.split.wrapping_add(clock.split_step());
            if slashes.split < SWORDIE_ARM5_END {
                return CastArmStep::Hold;
            }
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
            // `sb zero,0x21b` / `sh zero,0x176` at `0x801F76F0..0x801F76F4`
            // release the strike clip's park, then `+0x21D = speed`.
            if let Some(s) = seats.get_mut(summon_seat as usize) {
                s.loop_count = 0;
                s.loop_timer = 0;
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
/// PORT: FUN_801F81DC (PROT 0910; the whole routine: progress accumulator, hit counter, and the blade-draw flag whose seat is `swordie_blade_pose`, and the landing through `swordie_slash`)
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
        slashes.draw.blades[slash as usize] = true;
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

/// The record PROT 0910's slash routine seats on every call that leaves its
/// slash travelling (`addiu a2,a2,-0x7384` / `jal 0x80021B04` at
/// `0x801F8834..0x801F883C`, scale `0x1000`): one blade image a pass, so a
/// slash leaves a trail of them along its curve.
pub const SWORDIE_BLADE_RECORD: u32 = 0x801F_8C7C;

/// The height the blades fly at before slashes `2` / `3` lift them
/// (`li t4,-0xc8` / `sh t4,0x12(sp)` at `0x801F8350`).
pub const SWORDIE_BLADE_Y: i16 = -0xC8;

/// The plane pair arm 8 stores at `0x801F8D90` for the slashes to fly from:
/// the creature's pair moved a twelfth of a unit along its heading `f`
/// (`* 0x2AAAAAAB`, `sra 1`, less the sign: `trunc(sin f / 12)` /
/// `trunc(cos f / 12)` at `0x801F77F4..0x801F789C`).
pub fn swordie_blade_origin(x: i16, z: i16, facing: u16) -> (i16, i16) {
    let (sin, cos) = legaia_engine_system::trig::trig12(facing & 0xFFF);
    (x.wrapping_add(sin / 12), z.wrapping_add(cos / 12))
}

/// `(sin(a) * b) >> shift`, rounded toward zero (`bgez` / `addiu 2^shift -
/// 1` / `sra shift`).
fn mul_shift(a: i16, b: i16, shift: u32) -> i32 {
    let p = i32::from(a) * i32::from(b);
    let p = if p < 0 { p + (1 << shift) - 1 } else { p };
    p >> shift
}

/// Where `FUN_801F81DC` seats slash `slash`'s blade for its progress word
/// `progress` (after the call's step) and the speed scalar `speed`, flying
/// from `origin` past the victim's plane pair `victim`: `(pos, angles)`.
///
/// ```text
/// h  = FUN_80019B28(victim, origin) + 0x800          ; 0x801F82B0..0x801F82BC
/// b  = origin + ((sin h, cos h) * progress / speed) >> 7   ; mult, divu, srl 7
/// u  = (progress << 5) / speed                       ; the arc's half-turn
/// h2 = h - 0x400
/// 0: b - (sin h2, cos h2) * sin u >> 15, yaw h2 - cos u / 8
/// 1: b + (sin h2, cos h2) * sin u >> 15, yaw h2 + cos u / 8
/// 2: b - (sin h2, cos h2) * sin u >> 16, yaw h2 - cos u / 16, y -0xC8 - sin u / 16
/// 3: b + (sin h2, cos h2) * sin u >> 16, yaw h2 + cos u / 16, y -0xC8 - sin u / 16
/// ```
///
/// The base's `divu` takes the signed product as unsigned and `srl 7`
/// shifts it logically; the halfword store keeps the low sixteen bits, which
/// for a power-of-two speed is the arithmetic quotient. The two outer
/// slashes bow out half a unit either side of the line, the inner two a
/// quarter and rise as they go.
///
/// REF: FUN_801F81DC (PROT 0910; `0x801F829C..0x801F8840`, the blade's seat, under [`swordie_slash_step`]'s tag)
pub fn swordie_blade_pose(
    slash: u8,
    progress: u32,
    speed: u8,
    origin: (i16, i16),
    victim: (i16, i16),
) -> ([i16; 3], [i16; 3]) {
    use crate::cast_module_camera::{ModuleSeat, heading};
    use legaia_engine_system::trig::trig12;
    let speed = u32::from(speed.max(1));
    let seat = |(x, z): (i16, i16)| ModuleSeat {
        x,
        y: 0,
        z,
        facing: 0,
    };
    let h = heading(seat(victim), seat(origin)).wrapping_add(0x800) & 0xFFF;
    let (sin_h, cos_h) = trig12(h);
    let reach = |t: i16| {
        let lo = (i32::from(t).wrapping_mul(progress as i32)) as u32;
        ((lo / speed) >> 7) as u16
    };
    let bx = (origin.0 as u16).wrapping_add(reach(sin_h)) as i16;
    let bz = (origin.1 as u16).wrapping_add(reach(cos_h)) as i16;
    let u = ((progress << 5) / speed) as u16 & 0xFFF;
    let (sin_u, cos_u) = trig12(u);
    let h2 = h.wrapping_sub(0x400) & 0xFFF;
    let (sin_h2, cos_h2) = trig12(h2);
    let (shift, sign, lift, turn) = match slash {
        0 => (15, -1, 0, -i32::from(cos_u) / 8),
        1 => (15, 1, 0, i32::from(cos_u) / 8),
        2 => (16, -1, i32::from(sin_u) / 16, -i32::from(cos_u) / 16),
        _ => (16, 1, i32::from(sin_u) / 16, i32::from(cos_u) / 16),
    };
    let x = i32::from(bx) + sign * mul_shift(sin_h2, sin_u, shift);
    let z = i32::from(bz) + sign * mul_shift(cos_h2, sin_u, shift);
    let y = i32::from(SWORDIE_BLADE_Y) - lift;
    let yaw = i32::from(h2) + turn;
    ([x as i16, y as i16, z as i16], [0, yaw as i16, 0])
}
