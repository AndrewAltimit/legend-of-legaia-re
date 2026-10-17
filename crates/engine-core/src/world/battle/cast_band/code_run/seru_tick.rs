//! The player Seru tick for modules 0909..0913.

use super::*;

impl World {
    // --- player Seru 0909..0913 ---
    /// Dispatch one of the five player-Seru tick bodies for this frame
    /// ([`legaia_engine_cast::seru_magic_ticks`], PROT 0909..0913, action ids
    /// `0x87..=0x8B`).
    ///
    /// The damage and heal inputs are deliberately neutral. The engine folds
    /// a cast's HP outcome exactly once, at
    /// [`Self::cast_spell_on_slots_prepaid`], so what runs here is each
    /// module's phase machine, its `ctx+0x278` discipline and its staging /
    /// render writes; the damage step itself stays a tested kernel rather
    /// than becoming a second application. That is the same posture every
    /// other non-sweep tick body in the band takes, and it is why PROT 0911's
    /// heal amount is passed as `0`.
    ///
    /// The magnitude is not lost by that: `seru_tick_heal_amount` computes
    /// `battle_formulas::orb_heal_amount` of the caster's per-spell magic
    /// level (the character record's `+0x161` byte, found by scanning the
    /// learned-id list at `+0x13D`) and overrides the spell catalog's
    /// placeholder inside the fold, so the amount a live Orb restores is
    /// retail's `(level << 6) + 0x1C0` clamped to the seat's missing HP.
    /// Passing it here as well would restore twice, once per owner - which is
    /// exactly what PROT 0905 used to do.
    pub(in crate::world::battle::cast_band) fn run_seru_b_tick(
        &mut self,
        entry: u32,
        ctx: &mut legaia_engine_cast::cast_module_ticks::CastModuleCtx,
        seats: &mut [legaia_engine_cast::cast_module_ticks::CastActorState],
        caster_slot: u8,
        summon_slot: u8,
        victim_slot: u8,
    ) -> (
        legaia_engine_cast::cast_module_ticks::CastTickStep,
        Vec<legaia_engine_cast::cast_module_ticks::AoeHit>,
    ) {
        use legaia_engine_cast::cast_module_ticks::{AoeHit, SweepHit};
        use legaia_engine_cast::seru_magic_ticks as seru;

        // `seats` is the retail-shaped row (`seat_map`); the kernels take
        // their seats in it, the engine's own reads stay engine-indexed.
        let map = self.cast_seat_map();
        let (r_caster, r_summon, r_victim) = (
            map.retail(caster_slot),
            map.retail(summon_slot),
            map.retail(victim_slot),
        );
        fn lift(hits: &[SweepHit]) -> Vec<AoeHit> {
            hits.iter()
                .map(|h| AoeHit {
                    seat: h.seat,
                    applied: h.applied as i32,
                })
                .collect()
        }
        match entry {
            909 => {
                let mut settle = self.casting.module_settle_countdown;
                let (step, sweep) =
                    seru::viguro_tick(ctx, seats, r_caster, r_summon, &mut settle, |_| 0);
                self.casting.module_settle_countdown = settle;
                (step, lift(&sweep.hits))
            }
            // PROT 0910 paces its strike on two timers - arm 7's wind-up and
            // arm 9's staggered slashes - built from the frame step
            // (`0x1F800393`) and the speed scalar (`0x1F80037D`), so the slash
            // reactions land on retail's cadence. Each of the four landings
            // runs `swordie_slash_step` with a neutral wrapper return: the
            // clamp, the hit counter and the per-slash reaction clip are
            // live, the HP outcome stays the fold's.
            //
            // Retail runs the body once a battle pass, which spans the frame
            // step's vsyncs, and drains by `rate * speed` there; the engine
            // runs it once a vsync, so its `rate` is `1` (passing the frame
            // step ran both timers at twice retail's speed). `speed` is the
            // scalar itself - the battle seating's normal rate `8` - not `1`:
            // the thresholds scale with it, and arm 7 stores it as the
            // summon's animation rate `+0x21D`, which at `1` played the
            // strike at an eighth of normal speed.
            910 => {
                let clock = seru::SwordieClock {
                    rate: 1,
                    speed: legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL,
                };
                let mut slashes = self.casting.module_swordie;
                let phase_in = ctx.phase;
                let (step, hits) =
                    seru::swordie_tick(ctx, seats, r_summon, r_victim, &mut slashes, clock, |_| 0);
                self.swordie_blades(
                    phase_in,
                    ctx.phase,
                    &mut slashes,
                    clock.speed,
                    summon_slot,
                    victim_slot,
                );
                self.casting.module_swordie = slashes;
                (step, lift(&hits))
            }
            911 => {
                let maxes: Vec<u16> = (0..map.retail_len())
                    .map(|r| {
                        map.engine(r as u8)
                            .and_then(|e| self.actors.get(usize::from(e)))
                            .map_or(0, |a| a.battle.max_hp)
                    })
                    .collect();
                // Spell id for a `seru_magic_ticks` entry: the player
                // Seru-magic block is linear, `entry = 903 + (id - 0x81)`.
                let spell_id = (entry - 903 + 0x81) as u8;
                let cleanse =
                    self.cure_selector(caster_slot, spell_id, seru::ORB_CLEANSE_MIN_LEVEL);
                let mut settle = self.casting.module_settle_countdown;
                // The arms move the creature's own seat (`+0x34..+0x38`):
                // arm 3 lifts it, arm 4 drops it to the ground, arm 5 steps
                // it forward and up.
                let mut pos = self
                    .actors
                    .get(usize::from(summon_slot))
                    .filter(|a| a.active)
                    .map(|a| seru::OrbSeatPos {
                        x: a.move_state.world_x,
                        y: a.move_state.world_y,
                        z: a.move_state.world_z,
                        facing: a.battle.facing_angle & 0xFFF,
                    });
                let mut scratch = pos.unwrap_or_default();
                let phase_in = ctx.phase;
                let (step, _healed) = seru::orb_tick(
                    ctx,
                    seats,
                    r_summon,
                    0,
                    cleanse,
                    &mut settle,
                    &mut scratch,
                    |s| maxes.get(s as usize).copied().unwrap_or(0),
                );
                self.casting.module_settle_countdown = settle;
                // Arm 5's pass steps and lifts the creature in the pass that
                // cuts to it, and the cut lands on the next tick here.
                if pos.is_some_and(|b| b != scratch)
                    && self.orb_defer_seat(phase_in, ctx.phase, &scratch)
                {
                    return (step, Vec::new());
                }
                if let Some(before) = pos.as_mut()
                    && *before != scratch
                    && let Some(a) = self.actors.get_mut(usize::from(summon_slot))
                {
                    a.move_state.world_x = scratch.x;
                    a.move_state.world_y = scratch.y;
                    a.move_state.world_z = scratch.z;
                }
                (step, Vec::new())
            }
            912 => {
                let mut settle = self.casting.module_settle_countdown;
                // Arm 0x0B waits on the creature's clip cursor (its anim
                // node's `+0x68`).
                let cursor = self
                    .actors
                    .get(usize::from(summon_slot))
                    .filter(|a| a.active)
                    .and_then(|a| a.battle_animation.as_ref())
                    .map(|p| p.cursor_sixteenths());
                let phase_in = ctx.phase;
                let (step, hits) =
                    seru::freed_tick(ctx, seats, r_summon, &mut settle, cursor, |_| 0);
                self.casting.module_settle_countdown = settle;
                self.freed_body_effects(phase_in, ctx.phase, caster_slot);
                (step, lift(&hits))
            }
            _ => {
                let mut settle = self.casting.module_settle_countdown;
                let phase_in = ctx.phase;
                let (step, hit) = seru::nova_tick(ctx, seats, r_summon, r_victim, 0, &mut settle);
                self.casting.module_settle_countdown = settle;
                self.nova_victim_motion(phase_in, ctx.phase, settle, victim_slot);
                let hits: Vec<SweepHit> = hit.into_iter().collect();
                (step, lift(&hits))
            }
        }
    }
}
