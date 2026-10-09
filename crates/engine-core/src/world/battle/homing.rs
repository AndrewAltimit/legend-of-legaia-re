//! The move projectile's **homing slots** - the per-slot flight of
//! `FUN_801E09F8`'s effect-child driver
//! ([`crate::action_effect_script::HomingSlots`]), seated on the world.
//!
//! The effect script's terminator seeds them beside the move-FX streak;
//! the battle frame flies them. What reads them is the cast camera: case `7`
//! of `FUN_801DC0A0` hands on to the projectile shot `8` once the census
//! counts a child, and case `8` frames slot `0`
//! (`crate::battle_cam_inputs::spell_cam_inputs`).

use super::*;
use crate::action_effect_script::TargetBand;

impl World {
    /// The terminator's seed (`FUN_801DEA50`, `0x801DF298..0x801DF3DC`):
    /// every slot at the launch point, and slot `i` of the band aimed at the
    /// band's `i`-th seat while that seat is alive and is not the caster.
    ///
    /// The band's two whole-side codes are in retail numbering (party
    /// `0..=2`, monsters `3..=6`); a single-seat band is the caster's own
    /// `+0x1DD`, which the engine keeps as an engine actor index.
    pub(in crate::world) fn seed_homing_slots(
        &mut self,
        caster: usize,
        band: TargetBand,
        record: Option<usize>,
        launch: (i32, i32, i32),
    ) {
        let pc = usize::from(self.party.party_count);
        let seat_of = |retail: u8| -> Option<usize> {
            match (band.first, band.last) {
                (3, 6) => Some(pc + usize::from(retail - 3)),
                (0, 2) => (usize::from(retail) < pc).then_some(usize::from(retail)),
                _ => Some(usize::from(retail)),
            }
        };
        let mut seats = [None; crate::action_effect_script::HOMING_SLOTS];
        let mut children = [0u8; crate::action_effect_script::HOMING_SLOTS];
        for (i, retail) in (band.first..=band.last).enumerate() {
            let Some(e) = seat_of(retail) else {
                continue;
            };
            let Some(a) = self.actors.get(e) else {
                continue;
            };
            if i >= seats.len() || !a.active || a.battle.hp == 0 || e == caster {
                continue;
            }
            seats[i] = Some([
                i32::from(a.move_state.world_x),
                i32::from(a.move_state.world_y),
                i32::from(a.move_state.world_z),
            ]);
            children[i] = e as u8 + 1;
        }
        self.casting
            .homing
            .seed(record, launch, &seats, |i| children[i]);
        // A cast's fold would stage the move's two lists at the target in one
        // go (`World::request_move_fx_spawn`); it leaves them to this flight
        // when the caster's script still had this terminator ahead
        // (`CastFxState::homing_takes_lists`). An action with no fold (a
        // physical strike) keeps the effects its own paths draw, and its
        // flight only moves the camera's point.
        //
        // The flight also takes them when the fold is still to come: retail
        // has no fold - `FUN_801E09F8` spawns the lists from this flight
        // whatever the order - so a cast whose terminator runs first emits
        // here, and the fold, finding the flight holds the lists, stages
        // nothing ([`crate::world::CastFxState::homing_holds_lists`]).
        let fold_took = std::mem::take(&mut self.casting.homing_takes_lists);
        let fold_pending = !fold_took && self.cast_fold_pending(caster);
        self.casting.homing.emits = fold_took || fold_pending;
        self.casting.homing_holds_lists = fold_pending.then_some(caster as u8);
    }

    /// Whether actor `slot`'s committed action is a cast whose fold
    /// (`World::cast_spell_on_slots`) will stage a move-FX scene that has not
    /// run yet: a Magic-category action (`+0x1DE == 2`) on a move that is
    /// not a player summon and carries spawnable effect lists.
    fn cast_fold_pending(&self, slot: usize) -> bool {
        let Some(a) = self.actors.get(slot) else {
            return false;
        };
        let move_id = a.battle.params.first().copied().unwrap_or(0);
        a.battle.action_category == vm::battle_action::ActionCategory::Magic.as_byte()
            && !crate::summon::PLAYER_SUMMON_IDS.contains(&move_id)
            && self
                .tables
                .move_power
                .as_ref()
                .is_some_and(|cat| cat.move_has_spawn_fx(move_id))
    }

    /// Whether actor `slot`'s committed effect script has a terminator
    /// record at or past its cursor - the record whose step seeds the homing
    /// slots ([`Self::seed_homing_slots`]).
    pub(in crate::world) fn effect_script_terminates(&self, slot: usize) -> bool {
        use crate::action_effect_script as fx;
        let Some(a) = self.actors.get(slot) else {
            return false;
        };
        let Some(script) = a.battle_effect_script.as_ref() else {
            return false;
        };
        (a.battle_effect_cursor..fx::MAX_CURSOR)
            .map_while(|c| fx::EffectRecord::at(script, c))
            .any(|r| r.is_terminator())
    }

    /// One battle frame of the homing slots, after the streak's own counter
    /// walk (slot `0`'s phase-`1` word).
    pub(in crate::world) fn tick_homing_slots(&mut self) {
        let Some(idx) = self.casting.homing.record else {
            return;
        };
        let Some(raw) = self
            .tables
            .move_power
            .as_ref()
            .and_then(|cat| cat.record_at_index(idx))
            .map(|rec| rec.raw)
        else {
            return;
        };
        let counter = self.casting.move_fx_streak.counter_word;
        let actors = &self.actors;
        let target = |child: u8| -> Option<[i32; 3]> {
            let a = actors.get(usize::from(child).checked_sub(1)?)?;
            a.active.then(|| {
                [
                    i32::from(a.move_state.world_x),
                    i32::from(a.move_state.world_y),
                    i32::from(a.move_state.world_z),
                ]
            })
        };
        let mut homing = self.casting.homing;
        let spawns = homing.step(&raw, counter, target);
        self.casting.homing = homing;
        if !homing.emits {
            return;
        }
        // Each list byte goes where the effect script's own records go: bit
        // 7 to the 2D pool `FUN_801DFDF0` at the slot's heading, the rest to
        // the `0x801F6324` prototype scene with its CLUT stage
        // (`0x801E1178..0x801E1238`).
        let caster = self.battle_ctx.active_actor;
        for s in spawns {
            self.battle
                .effect_spawns
                .push(crate::battle_events::BattleEffectSpawn {
                    actor_slot: caster,
                    effect: s.effect & !crate::action_effect_script::EFFECT_DIRECT_BIT,
                    direct: s.effect & crate::action_effect_script::EFFECT_DIRECT_BIT != 0,
                    at: (s.at[0], s.at[1], s.at[2]),
                    facing: s.heading,
                });
        }
    }
}
