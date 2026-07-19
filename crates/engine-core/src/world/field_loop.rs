//! Live field<->battle loop, encounter/formation battle entry, battle BGM swap, field player install/seating, and step_field.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    // --- live gameplay loop: Field <-> Battle round trip ------------------

    /// Per-frame field-side driver for the live gameplay loop. Gated by
    /// [`Self::live_gameplay_loop`] in [`Self::tick`]; never called when the
    /// flag is off.
    ///
    /// Composes the already-existing encounter pieces into the per-frame
    /// flow the retail field loop runs:
    ///
    /// 1. **Step detection.** A "step" is the player actor crossing into a
    ///    new 128-unit collision tile (`pos >> 7`). Each step drives one
    ///    [`Self::on_field_step`] roll - matching the retail per-step
    ///    counter rather than rolling every frame.
    /// 2. **Timers.** [`Self::tick_encounter`] advances the session's
    ///    `Transition` / `Grace` countdowns every frame regardless of
    ///    movement.
    /// 3. **Transition.** When the session reaches `Triggered`,
    ///    [`Self::drain_encounter_formation`] yields the rolled formation and
    ///    [`Self::begin_encounter_battle`] flips `Field -> Battle`.
    pub(crate) fn live_field_tick(&mut self) {
        // (1) step detection on tile crossing.
        if let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get(slot as usize)
        {
            let tile = (actor.move_state.world_x >> 7, actor.move_state.world_z >> 7);
            match self.field_last_tile {
                Some(prev) if prev != tile => {
                    self.field_last_tile = Some(tile);
                    // Per-tile region refresh (the `FUN_800180EC` /
                    // `FUN_801DBA20` grain - retail re-runs the region scan
                    // when the player tile changes).
                    self.refresh_field_regions();
                    self.on_field_step();
                }
                None => self.field_last_tile = Some(tile),
                _ => {}
            }
        }
        // (2) advance transition / grace timers.
        self.tick_encounter();
        // (3) Triggered -> begin battle.
        if let Some(roll) = self.drain_encounter_formation() {
            self.begin_encounter_battle(roll);
        }
    }

    /// Resolve `roll` to a concrete formation and flip into battle.
    ///
    /// Snapshots the field actor table (restored verbatim on victory),
    /// remembers the formation for [`Self::apply_battle_loot`], and seeds the
    /// battle actor table from the formation + monster catalog. No-op when
    /// the roll's `formation_id` isn't registered in
    /// [`Self::formation_table`] (the session has already advanced to
    /// `Battling`, so the next [`Self::end_encounter_battle`] cleans it up).
    pub(crate) fn begin_encounter_battle(&mut self, roll: crate::encounter::EncounterRoll) {
        let Some(formation) = self.formation_table.formation(roll.formation_id).cloned() else {
            // Unknown formation: bail back to the field by ending the (empty)
            // battle so the session leaves `Battling`.
            self.end_encounter_battle();
            return;
        };
        self.field_return = Some(FieldReturnState {
            actors: self.actors.clone(),
            player_actor_slot: self.player_actor_slot,
            party_count: self.party_count,
        });
        self.battle_return_mode = SceneMode::Field;
        // No engine-side battle staging: a scripted boss fight's transient
        // staged marker (rikuroa's `0x289`) is SET by the stager record's own
        // script bytes (`P1[3]`'s `52 89`, executed through
        // [`Self::run_boss_stager_record`]) immediately before its `3E FF`
        // battle-entry op reaches this path.
        self.enter_battle_from_formation(&formation);
        self.active_formation = Some(formation);
    }

    /// Seed the battle actor table from `formation` and enter
    /// [`SceneMode::Battle`].
    ///
    /// Party slots `0..party_count` keep their HP / MP (seeded from the
    /// roster by the boot path); monster slots take HP / attack / defense
    /// from [`Self::monster_catalog`]. Every combatant is marked alive,
    /// `action_category = Attack`, and party members target the first
    /// monster. The battle-action context is seeded at `Begin` with the
    /// Attack action queued. This is the live-loop counterpart to the
    /// generic [`Self::enter_battle`] placement helper.
    /// Configure the battle BGM track id. `Some(id)` enables the
    /// Battle↔Field music swap (the live loop switches to `id` on encounter
    /// and restores the field track on battle end); `None` disables it. See
    /// [`World::battle_bgm`].
    pub fn set_battle_bgm(&mut self, bgm_id: Option<u16>) {
        self.battle_bgm = bgm_id;
    }

    /// Switch to the configured battle track at encounter start. No-op when
    /// [`World::battle_bgm`] is `None` or the swap is already active. Stashes
    /// the current field track for [`World::restore_field_bgm`] and queues a
    /// `FieldEvent::Bgm` start so the host's BGM director cross-fades to it.
    fn swap_to_battle_bgm(&mut self) {
        let Some(battle) = self.battle_bgm else {
            return;
        };
        if self.battle_bgm_active || self.current_bgm == Some(battle) {
            return;
        }
        self.field_bgm_resume = self.current_bgm;
        self.current_bgm = Some(battle);
        self.battle_bgm_active = true;
        self.pending_field_events.push(FieldEvent::Bgm {
            text_id: battle,
            sub_op: 1,
        });
    }

    /// Restore the field track stashed by [`World::swap_to_battle_bgm`] when
    /// a battle ends. No-op unless a battle swap is active. Queues a
    /// `FieldEvent::Bgm` start for the stashed track, or a stop (sub-op 4)
    /// when no field track was playing at encounter start.
    pub(crate) fn restore_field_bgm(&mut self) {
        if !self.battle_bgm_active {
            return;
        }
        self.battle_bgm_active = false;
        match self.field_bgm_resume.take() {
            Some(track) => {
                self.current_bgm = Some(track);
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: track,
                    sub_op: 1,
                });
            }
            None => {
                self.current_bgm = None;
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: 0,
                    sub_op: 4,
                });
            }
        }
    }

    pub(crate) fn enter_battle_from_formation(
        &mut self,
        formation: &crate::monster_catalog::FormationDef,
    ) {
        let party_count = self.party_count.clamp(1, 3);
        let monster_count = formation.slots.len().min(5) as u8;
        // Drop any field dialogue left open across the transition. The
        // engage conversation already played in the field; a leftover
        // inline-script runner would otherwise re-walk the NPC's whole
        // segment bank over the battle (and nothing in battle mode owns
        // its input). Retail's in-battle tutorial boxes are a separate
        // stage-overlay (extraction 967) channel, not the field box.
        self.inline_dialogue = None;
        self.current_dialog = None;
        self.carrier_menu = None;
        self.pending_carrier_engage = None;
        // Reuse the placement helper for actor spawn + seating, then overlay
        // per-slot stats.
        self.enter_battle(party_count, monster_count);
        let first_monster = party_count;
        for slot in 0..party_count as usize {
            let a = &mut self.actors[slot];
            a.battle.liveness = 1;
            a.battle.action_category = 3; // Attack
            a.battle.active_target = first_monster;
            // Party members are not monsters - clear any id left from a
            // previous battle that placed an enemy in this slot.
            a.battle_monster_id = None;
        }
        // Fold the roster's live stats + equipped-gear bonuses onto the party
        // combatants' attack / defense (no-op for a zeroed roster).
        self.seed_party_battle_stats();
        // Clear any monster-slot SPD / accuracy / evasion left over from a
        // previous battle so this formation's values are the only ones seen.
        for s in self.battle_speed.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for s in self.battle_accuracy.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for s in self.battle_evasion.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for (i, fslot) in formation.slots.iter().take(5).enumerate() {
            let mslot = party_count as usize + i;
            if mslot >= self.actors.len() {
                break;
            }
            // Tag the slot with its monster id so a renderer can fetch the
            // battle mesh, even if the catalog has no stats for it.
            self.actors[mslot].battle_monster_id = Some(fslot.monster_id);
            if let Some(def) = self.monster_catalog.get(fslot.monster_id) {
                let speed = def.speed;
                let a = &mut self.actors[mslot];
                a.battle.hp = def.hp;
                a.battle.max_hp = def.hp;
                a.battle.mp = def.mp;
                a.battle.liveness = 1;
                a.battle.action_category = 3;
                if let Some(s) = self.battle_attack.get_mut(mslot) {
                    *s = def.attack;
                }
                if let Some(s) = self.battle_defense.get_mut(mslot) {
                    *s = def.udf.max(def.ldf);
                }
                if let Some(s) = self.battle_speed.get_mut(mslot) {
                    *s = speed;
                }
                if let Some(s) = self.battle_accuracy.get_mut(mslot) {
                    *s = def.accuracy as u16;
                }
                if let Some(s) = self.battle_evasion.get_mut(mslot) {
                    *s = def.evasion as u16;
                }
            }
        }
        // Roll for a rare shiny capturable enemy now that every monster slot
        // carries its stats + id (so capturability + the +35% boost see final
        // values). Clears last battle's flags first.
        self.shiny_enemy_slots.clear();
        self.shiny_captures.clear();
        self.roll_shiny_enemy(first_monster);

        self.battle_ctx.queued_action = 3;
        self.battle_ctx.active_actor = 0;
        // Fresh battle: clear the monster-AI cooldowns / phase counter / ring.
        self.monster_ai_state.reset();
        // Seed the turn-order initiative keys for this battle. When real SPD is
        // present this lets the next-actor selector run the initiative scheme;
        // slot 0 still opens round 1 (its key is consumed below) so subsequent
        // turns order by initiative. A no-SPD battle leaves every key at 0 and
        // stays on the round-robin fallback.
        self.seed_battle_initiative();
        // Switch to the battle track (if configured) - the host's BGM
        // director cross-fades from the field music.
        self.swap_to_battle_bgm();
        if self.battle_player_driven {
            // Player-driven: don't pre-arm the first attack - open the command
            // menu for party member 0 and let the SM idle until the player
            // confirms (handled in `live_battle_tick`).
            self.open_battle_command(0);
        }
    }

    /// Configure the actor at `slot` as the field player and reset the
    /// per-scene collision grid.
    ///
    /// REF: FUN_8003aeb0
    ///
    /// Mirrors the player-actor setup in the
    /// scene-entry map-init `FUN_8003aeb0` (`player[+0x72] = 0x1000`) plus
    /// the per-frame delta scalar `DAT_1f800393` (defaulted to `1` when the
    /// world hasn't installed one). Idempotent across scene transitions.
    pub fn install_field_player(&mut self, slot: u8) {
        self.player_actor_slot = Some(slot);
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.active = true;
            actor.move_state.field_72 = FIELD_PLAYER_SPEED_MULT;
        }
        if self.move_ramp_ratio == 0 {
            self.move_ramp_ratio = 1;
        }
        self.reset_field_collision_grid();
    }

    /// Seat the player actor at a field tile centre (`world = tile*128 +
    /// 0x40` - the same tile->world mapping the MAN placement spawns and the
    /// op-`0x3E` region check use, whose inverse is `(world - 0x40) >> 7`).
    ///
    /// This is the warp-arrival placement: a door transition (field-VM op
    /// `0x3F`) carries the destination entry tile in its trailing bytes, and
    /// [`crate::scene::SceneHost::tick`] calls this after the destination
    /// scene loads so the player stands at the door it arrived through
    /// instead of the cold-boot spawn. The floor height is sampled so the
    /// player lands on the destination's terrain tier rather than `y = 0`.
    pub fn seat_player_at_tile(&mut self, tile_x: u8, tile_z: u8) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        // Retail entry-byte decode (`FUN_801DE840` case 0x3F): the low 7
        // bits are the tile, the high bit selects the far half of the tile
        // (`(b & 0x7F) * 0x80 + 0x40`, `+0x80` when bit 7 is set).
        let half =
            |b: u8| -> i16 { i16::from(b & 0x7F) * 128 + if b & 0x80 != 0 { 0x80 } else { 0x40 } };
        let (wx, wz) = (half(tile_x), half(tile_z));
        let wy = self.sample_field_floor_height(wx as i32, wz as i32) as i16;
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.move_state.world_x = wx;
            actor.move_state.world_y = wy;
            actor.move_state.world_z = wz;
        }
    }

    /// Face the player along a warp-arrival compass sector - the op-`0x3F`
    /// trailing `dir` byte. Retail resolves `dir & 7` through the 8-entry
    /// i16 table at SCUS `0x80073F04` (`[0, 0x200, 0x400, .. 0xE00]` - the
    /// eight 45-degree compass points of the 12-bit angle space) into the
    /// arrival-facing global `_DAT_80073EFC`; the engine stores the same
    /// angle on the player's heading (`move_state.render_26`, `0` = +Z).
    ///
    /// REF: FUN_801DE840 (case 0x3F facing write, table 0x80073F04)
    pub fn face_player_sector(&mut self, dir: u8) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.move_state.render_26 = i16::from(dir & 7) * 0x200;
        }
    }

    /// One field-VM step. Drives `field_ctx` + `field_pc` from the loaded
    /// `field_bytecode`. No-op when no bytecode is loaded.
    pub fn step_field(&mut self) -> Option<FieldStepResult> {
        if self.field_bytecode.is_empty() {
            return None;
        }
        let ctx_ptr: *mut FieldCtx = &mut self.field_ctx;
        let bc_ptr: *const Vec<u8> = &self.field_bytecode;
        let pc = self.field_pc;
        let mut host = FieldHostImpl { world: self };
        // SAFETY: FieldHostImpl never borrows `world.field_ctx` or
        // `world.field_bytecode` through the borrow.
        let ctx = unsafe { &mut *ctx_ptr };
        let bc: &[u8] = unsafe { (*bc_ptr).as_slice() };
        let res = vm::field::step(&mut host, ctx, bc, pc);
        match &res {
            FieldStepResult::Advance { next_pc } => self.field_pc = *next_pc,
            FieldStepResult::Yield { resume_pc } => self.field_pc = *resume_pc,
            FieldStepResult::Halt { final_pc } => self.field_pc = *final_pc,
            FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                self.field_pc = *pc;
            }
        }
        // The field-VM borrow has ended; install any scripted encounter the
        // op 0x34 sub-2 forwarded-PC capture queued this step.
        self.drain_pending_scripted_encounter();
        Some(res)
    }

    /// Run the just-loaded scene-entry system script (ctx `0xFB`) up to its
    /// first yield / wait / halt, bounded.
    ///
    /// Retail's per-frame context loop runs every live context until it
    /// yields, so the entry script's whole prologue - flag routing, tile
    /// walls, BGM, and the `0x52F` arrival-fade arm (`4C 12 00 00 00 00 00`
    /// instant black + `4C 12 80 80 80 44 00` ramp to neutral) - executes
    /// within the scene-load frame, BEFORE the first rendered frame. The
    /// engine's per-tick driver steps the field VM one op per tick, which
    /// would smear those load-frame ops over seconds (and flash the scene
    /// full-bright before the fade op is reached); this pre-run restores the
    /// load-frame semantics. The budget bounds a mis-decoded stream; a
    /// script parked on a wait/yield resumes on the normal per-tick step.
    ///
    /// REF: FUN_8003AB2C (system-script frame slice)
    pub fn pre_run_entry_script(&mut self) {
        const ENTRY_SCRIPT_STEP_BUDGET: usize = 2048;
        // Seat the system ctx on the player's spawn position: the entry
        // script's player-targeted position tests (`CD F8` bbox - retail
        // resolves target `0xF8` to the live player object) read the ctx
        // position, and the ctx-0xFB system context has none of its own. The
        // opdeene fade arm sits behind exactly such a bbox gate.
        if let Some(slot) = self.player_actor_slot
            && let Some(a) = self.actors.get(slot as usize)
        {
            self.field_ctx.world_x = a.move_state.world_x as u16;
            self.field_ctx.world_z = a.move_state.world_z as u16;
        }
        for _ in 0..ENTRY_SCRIPT_STEP_BUDGET {
            match self.step_field() {
                // Continue through `Yield` as well as `Advance`: several
                // dispatcher arms exit via retail's `addiu s8, s8, N` PC-delta
                // idiom (e.g. the nibble-7 tile-wall ops), which the VM models
                // as a yield even though retail continues the same frame - the
                // opdeene fade arm sits past three of them. Real frame parks
                // stop the pre-run: `WaitFrames` mid-wait reports `Halt` at
                // PC, and unimplemented / text ops report Pending / Unknown.
                Some(FieldStepResult::Advance { .. } | FieldStepResult::Yield { .. }) => continue,
                _ => break,
            }
        }
    }

    /// Drain a queued scripted-encounter install (set by the `+0x94`
    /// forwarded-PC capture host hook) into the active encounter session.
    /// No-op when nothing is queued. Called by [`Self::step_field`] once the
    /// field-VM borrow has ended.
    pub fn drain_pending_scripted_encounter(&mut self) {
        if let Some(record) = self.pending_scripted_encounter.take() {
            self.install_scripted_encounter(&record);
        }
    }
}
