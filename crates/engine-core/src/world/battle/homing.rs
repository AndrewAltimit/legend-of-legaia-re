//! The move projectile's **homing slots** - the per-slot flight of
//! `FUN_801E09F8`'s effect-child driver
//! ([`legaia_engine_effects::action_effect_script::HomingSlots`]), seated on the world.
//!
//! The effect script's terminator seeds them beside the move-FX streak;
//! the battle frame flies them. What reads them is the cast camera: case `7`
//! of `FUN_801DC0A0` hands on to the projectile shot `8` once the census
//! counts a child, and case `8` frames slot `0`
//! (`crate::battle_cam_inputs::spell_cam_inputs`).

use super::*;
use legaia_engine_effects::action_effect_script::TargetBand;

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
        let seats = self.battle_seat_map();
        let seat_of = |retail: u8| -> Option<usize> {
            match (band.first, band.last) {
                (3, 6) | (0, 2) => seats.engine(retail).map(usize::from),
                _ => Some(usize::from(retail)),
            }
        };
        let mut seats = [None; legaia_engine_effects::action_effect_script::HOMING_SLOTS];
        let mut children = [0u8; legaia_engine_effects::action_effect_script::HOMING_SLOTS];
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
        // A new flight's slots own no handle until they spawn one.
        for scene in &mut self.casting.active_action_fx {
            scene.homing_slot = None;
        }
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
        // nothing ([`legaia_engine_battle_loop::cast_fx_state::CastFxState::homing_holds_lists`]).
        let fold_took = std::mem::take(&mut self.casting.homing_takes_lists);
        let fold_pending = !fold_took && self.cast_fold_pending(caster);
        self.casting.homing.emits = fold_took || fold_pending;
        self.casting.homing_holds_lists = fold_pending.then_some(caster as u8);
    }

    /// The terminator's own spawns, the run of bytes from record `+0x0E` up to
    /// the first zero: a bit-7 byte goes to the 2D pool `FUN_801DFDF0` at slot
    /// `k`'s position and heading, any other to the `0x801F6324` prototype at
    /// slot `0`'s position and rotation trio, and its actor becomes slot `k`'s
    /// handle. The run does not wait on the flight and has no fold to defer
    /// to. Aluru Cannon's `2C 29` is the charge at the muzzle, up before the
    /// slot's phase `1` is out. `0x801F6418`'s CLUT stage is the table arm's.
    // PORT: FUN_801DEA50 (`0x801DF3E0..0x801DF508`, the `+0x0E` launch run)
    pub(in crate::world) fn spawn_launch_run(&mut self, caster: usize, record: Option<usize>) {
        use legaia_engine_effects::action_effect_script as fx;
        let Some(raw) = record.and_then(|idx| {
            let cat = self.tables.move_power.as_ref()?;
            cat.record_at_index(idx).map(|rec| rec.raw)
        }) else {
            return;
        };
        let slots = self.casting.homing.slots;
        for (k, &byte) in raw[0x0E..].iter().take_while(|&&b| b != 0).enumerate() {
            let direct = byte & fx::EFFECT_DIRECT_BIT != 0;
            let s = slots[if direct { k.min(slots.len() - 1) } else { 0 }];
            self.battle.effect_spawns.push(
                legaia_engine_battle::battle_events::BattleEffectSpawn {
                    actor_slot: caster as u8,
                    effect: byte & !fx::EFFECT_DIRECT_BIT,
                    direct,
                    at: (s.pos[0], s.pos[1], s.pos[2]),
                    facing: s.heading,
                    homing_slot: Some(k.min(slots.len() - 1) as u8),
                },
            );
        }
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
        a.battle.action_category
            == legaia_engine_battle_vm::battle_action::ActionCategory::Magic.as_byte()
            && !legaia_engine_effects::summon::PLAYER_SUMMON_IDS.contains(&move_id)
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
        use legaia_engine_effects::action_effect_script as fx;
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

    /// `(ctx[+0x24E..=+0x251], ctx[+0x252..=+0x255])` as the cast census
    /// reads them: each slot's phase byte and child byte. All zero while the
    /// flight has no record to fly on, since [`Self::tick_homing_slots`]
    /// would never free such a slot.
    ///
    /// REF: FUN_801E09F8 (census head, `0x801E0B6C..0x801E0BF0`)
    pub(in crate::world) fn homing_child_slots(&self) -> ([u8; 4], [u8; 4]) {
        let h = &self.casting.homing;
        let flies = h.record.is_some_and(|idx| {
            self.tables
                .move_power
                .as_ref()
                .is_some_and(|cat| cat.record_at_index(idx).is_some())
        });
        if !flies {
            return ([0; 4], [0; 4]);
        }
        (h.slots.map(|s| s.phase), h.slots.map(|s| s.child))
    }

    /// One battle frame of the homing slots, after the streak's own counter
    /// walk (slot `0`'s phase-`1` word).
    pub(in crate::world) fn tick_homing_slots(&mut self) {
        let counter = self.casting.move_fx_streak.counter_word;
        self.tick_homing_slots_with(None, counter);
    }

    /// [`Self::tick_homing_slots`] as one battle pass of `step` vsyncs
    /// (`HomingSlots::step_pass`), with slot `0`'s phase-`1` word as the
    /// pass found it.
    pub(in crate::world) fn tick_homing_slots_pass(&mut self, step: u8, streak_before: u16) {
        self.tick_homing_slots_with(Some(step), streak_before);
    }

    fn tick_homing_slots_with(&mut self, pass_step: Option<u8>, counter: u16) {
        // The landed bolts are this pass's calls, or none.
        self.battle.flight_ribbons = None;
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
        let spawns = match pass_step {
            Some(step) => homing.step_pass(&raw, counter, step, target),
            None => homing.step(&raw, counter, target),
        };
        self.casting.homing = homing;
        // A tracking record drags each slot's handle onto the slot's fresh
        // position between spawns (`0x801E10CC..0x801E1108`).
        for (slot, pos) in homing.follows.iter().enumerate() {
            let Some(p) = pos else {
                continue;
            };
            let p = p.map(|v| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
            for scene in &mut self.casting.active_action_fx {
                if scene.homing_slot == Some(slot as u8) {
                    scene.drag_handle(p);
                }
            }
        }
        // Every phase-`3` pass stores the record's impact selector and its
        // tint on the slot's victim (`0x801E1560..0x801E15EC`), so the tint
        // holds from the landing to the hit; the cast fold's own store at
        // the hit (`World::apply_enemy_move_status`) is the same triple.
        for child in homing.landed.iter().filter(|&&child| child != 0) {
            self.arm_impact_tint(usize::from(child - 1), raw[0x0A]);
        }
        for child in homing.shocks.iter().filter(|&&child| child != 0) {
            if let Some(a) = self.actors.get_mut(usize::from(child - 1)) {
                a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(0x20);
            }
        }
        // And each such pass with the counter still running draws one streak
        // ribbon per `0xFF` byte of the `+0x16` list at the slot's position,
        // in the record's trail (`0x801E17EC..0x801E1834`). A slot in flight
        // draws one per `0xFF` byte of the `+0x12` list at its head first
        // (`0x801E1284..0x801E12D8`), so a record whose flight list is bolts
        // trails them all the way in; retail's slot loop runs both for one
        // slot before the next.
        let count = |list: &[u8]| list.iter().filter(|&&b| b == 0xFF).count();
        let (trails, bolts) = (count(&raw[0x12..0x16]), count(&raw[0x16..0x1A]));
        let mut ring = legaia_engine_cast::cast_module_camera::CastRibbonRing {
            points: [[0; 3]; 16],
            trail_id: raw[0x0B],
            count: 0,
        };
        let clamp =
            |p: [i32; 3]| p.map(|v| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
        for i in 0..homing.slots.len() {
            let trail = homing.trails[i].map(|p| (clamp(p), trails));
            let shock = (homing.shocks[i] != 0).then(|| (clamp(homing.slots[i].pos), bolts));
            for (p, n) in trail.into_iter().chain(shock) {
                for _ in 0..n {
                    if let Some(slot) = ring.points.get_mut(usize::from(ring.count)) {
                        *slot = p;
                        ring.count += 1;
                    }
                }
            }
        }
        if ring.count > 0 {
            self.battle.flight_ribbons = Some(ring);
        }
        // A landed slot's hit opens on the record's impact cue
        // (`lbu a0,0xd(v0)` / `jal 0x8004FCC8` at `0x801E184C..0x801E1854`).
        // Id `0` rings `-1`, which the drainer never plays.
        for _ in homing.hits.iter().filter(|&&child| child != 0) {
            if raw[0x0D] != 0 {
                self.dispatch_battle_cue(u16::from(raw[0x0D]));
            }
        }
        // A landed slot's hit (`0x801E1844..0x801E1A68`): the cast that rides
        // this flight folds onto that slot's victim now.
        let caster = self.battle_ctx.active_actor;
        if self
            .casting
            .pending_cast
            .as_ref()
            .is_some_and(|pc| pc.on_flight && pc.caster == caster)
        {
            for child in homing.hits {
                if let Some(victim) = child.checked_sub(1) {
                    self.fold_pending_cast_target(victim);
                }
            }
        }
        if !homing.emits {
            return;
        }
        // Each list byte goes where the effect script's own records go: bit
        // 7 to the 2D pool `FUN_801DFDF0` at the slot's heading, the rest to
        // the `0x801F6324` prototype scene with its CLUT stage
        // (`0x801E1178..0x801E1238`).
        // From the draw, what a list spawns is a node linked behind the
        // pass's tick walk: the fade actor and the effect children first
        // tick, and first show, on the next pass
        // (`World::release_draw_spawns`).
        let from_draw = self.battle.in_draw_cast_tick;
        for s in spawns {
            // The landing list's `0x64` is a screen flash, not a spawn
            // (`FUN_80024E80(0x801C9070, 2)` at `0x801E1530`); retail's fade
            // actors run side by side, so it takes a seat of its own.
            if s.flash {
                if from_draw {
                    self.battle.draw_flashes = self.battle.draw_flashes.saturating_add(1);
                } else {
                    self.push_homing_flash();
                }
                continue;
            }
            let list = if from_draw {
                &mut self.battle.draw_spawns
            } else {
                &mut self.battle.effect_spawns
            };
            list.push(legaia_engine_battle::battle_events::BattleEffectSpawn {
                actor_slot: caster,
                effect: s.effect & !legaia_engine_effects::action_effect_script::EFFECT_DIRECT_BIT,
                direct: s.effect & legaia_engine_effects::action_effect_script::EFFECT_DIRECT_BIT
                    != 0,
                at: (s.at[0], s.at[1], s.at[2]),
                facing: s.heading,
                homing_slot: Some(s.slot),
            });
        }
    }

    /// The landing flash's fade actor.
    fn push_homing_flash(&mut self) {
        let (mut t, id) = legaia_engine_effects::action_effect_script::homing_flash_template();
        t.mode[2] = id;
        self.presentation
            .module_fades
            .push(legaia_engine_system::fade::FadeState::load(&t));
    }

    /// Hand on what the last pass's draw spawned (`BattleState::draw_spawns`):
    /// the head of the pass that first ticks those nodes.
    pub(in crate::world) fn release_draw_spawns(&mut self) {
        for _ in 0..std::mem::take(&mut self.battle.draw_flashes) {
            self.push_homing_flash();
        }
        let spawns = std::mem::take(&mut self.battle.draw_spawns);
        self.battle.effect_spawns.extend(spawns);
    }
}

#[cfg(test)]
mod tests {
    use crate::world::World;
    use legaia_engine_mode::mode::SceneMode;
    use legaia_engine_system::sound_state::SfxRingOp;
    use legaia_game_tables::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
    };

    /// Aluru Cannon's retail recording rings `0x49` on the pass its slot lands
    /// (`FUN_8004FCC8` from `0x801E1854`, `record[+0x0D]`).
    fn world_with_a_landing_flight(cue: u8) -> World {
        let mut buf = vec![0u8; MOVE_POWER_TABLE_FILE_OFFSET + 44 * MOVE_POWER_RECORD_STRIDE + 64];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 1;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 0x0D] = cue;
        let mut world = World {
            mode: SceneMode::Battle,
            ..World::default()
        };
        world.tables.move_power = Some(
            legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&buf)
                .expect("synthetic catalog parses"),
        );
        world.actors[0].active = true;
        world.casting.homing.seed(
            Some(1),
            (0, 0, 0),
            &[Some([0, 0, 0]), None, None, None],
            |i| u8::from(i == 0),
        );
        world
    }

    fn ticks(world: &mut World) -> Vec<SfxRingOp> {
        for _ in 0..8 {
            world.tick_homing_slots();
        }
        std::mem::take(&mut world.audio.sfx_ring_ops)
    }

    #[test]
    fn a_landed_slot_rings_the_record_cue_once() {
        let mut world = world_with_a_landing_flight(0x49);
        assert_eq!(
            ticks(&mut world),
            vec![SfxRingOp::PushUnlessSustained {
                id: 0x49,
                key: 0x49 + 0x19C
            }]
        );
    }

    #[test]
    fn the_terminator_spawns_its_launch_run_up_to_the_zero() {
        let mut buf = vec![0u8; MOVE_POWER_TABLE_FILE_OFFSET + 44 * MOVE_POWER_RECORD_STRIDE + 64];
        let rec = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[rec] = 1;
        // Aluru Cannon's run, then a list byte past the zero the walk never reads.
        buf[rec + 0x0E] = 0x2C;
        buf[rec + 0x0F] = 0x29;
        buf[rec + 0x12] = 0x2A;
        let mut world = World {
            mode: SceneMode::Battle,
            ..World::default()
        };
        world.tables.move_power = Some(
            legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&buf)
                .expect("synthetic catalog parses"),
        );
        world
            .casting
            .homing
            .seed(Some(1), (5, -6, 7), &[None; 4], |_| 0);
        world.spawn_launch_run(4, Some(1));
        let spawns = world.drain_battle_effect_spawns();
        assert_eq!(
            spawns
                .iter()
                .map(|s| (s.effect, s.direct, s.at, s.homing_slot))
                .collect::<Vec<_>>(),
            vec![
                (0x2C, false, (5, -6, 7), Some(0)),
                (0x29, false, (5, -6, 7), Some(1)),
            ]
        );
    }

    /// A fade actor or an effect child the cast tick links from the draw
    /// first ticks on the next pass: the draw's cast tick holds its spawns,
    /// and the next pass's head hands them on.
    #[test]
    fn the_draws_cast_tick_holds_its_spawns_for_the_next_pass() {
        let landing = |from_draw: bool| {
            let mut world = world_with_a_landing_flight(0x49);
            let cat = world.tables.move_power.as_ref().expect("catalog");
            let mut raw = cat.record_at_index(1).expect("record").raw;
            raw[0x16] = 0x64;
            raw[0x17] = 0x2A;
            let mut buf =
                vec![0u8; MOVE_POWER_TABLE_FILE_OFFSET + 44 * MOVE_POWER_RECORD_STRIDE + 64];
            buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
            let rec = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE;
            buf[rec..rec + raw.len()].copy_from_slice(&raw);
            world.tables.move_power = Some(
                legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&buf)
                    .expect("synthetic catalog parses"),
            );
            world.battle.in_draw_cast_tick = from_draw;
            world.casting.homing.emits = true;
            for _ in 0..8 {
                world.tick_homing_slots();
            }
            world
        };
        let direct = landing(false);
        let fades = direct.presentation.module_fades.len();
        let spawns = direct.battle.effect_spawns.len();
        assert!(fades > 0 && spawns > 0, "{fades} {spawns}");

        let mut held = landing(true);
        assert!(held.presentation.module_fades.is_empty());
        assert!(held.battle.effect_spawns.is_empty());
        held.release_draw_spawns();
        assert_eq!(held.presentation.module_fades.len(), fades);
        assert_eq!(held.battle.effect_spawns.len(), spawns);
        assert!(held.battle.draw_spawns.is_empty());
    }

    #[test]
    fn a_record_with_no_cue_rings_nothing() {
        let mut world = world_with_a_landing_flight(0);
        assert_eq!(ticks(&mut world), vec![]);
    }
}
