//! Move-driven battle effects: move-FX and action-table spawns, battle burst
//! flush, the per-frame tick and the part-draw / morphed-mesh projections.

use super::*;

impl World {
    /// Spawn a battle move's effect-FX scene-graph at `origin` (world units).
    ///
    /// A move's `0x01..=0x63` on-contact (`+0x12`) / launch (`+0x16`) effect-list
    /// entries each index the `0x801f6324` prototype-pointer table; every such
    /// entry resolves to a summon-format move-VM record (`+0x00 model_sel`,
    /// `+0x02 flags`, `+0x04` bytecode) staged by the shared `FUN_80021B04`
    /// machinery. This parses those records out of the retained battle-action
    /// overlay (PROT 0898) and spawns them as a [`legaia_engine_effects::summon::SummonScene`]
    /// with model base `crate::world::EFFECT_MODEL_LIBRARY_BASE` (the engine's
    /// fixed-library analogue of the retail `gp[0x754] = party_count + 2`; see
    /// that constant's docs), so each mesh part resolves to
    /// `global_tmd_pool[model_sel + 3]` - the PROT 0871 effect-model library,
    /// already resident.
    ///
    /// Returns `false` (nothing spawned) when the move-power table isn't
    /// installed (disc-free battles), the move id has no power record, or the
    /// move carries no spawnable effect entries. Replaces any in-flight
    /// move-FX scene. Tick with [`Self::tick_move_fx`].
    pub fn spawn_move_fx(&mut self, move_id: u8, origin: [i16; 3]) -> bool {
        let Some(cat) = self.tables.move_power.as_ref() else {
            return false;
        };
        let Some(fx) = cat.fx_for_move_id(move_id) else {
            return false;
        };
        use legaia_game_tables::move_power::{self, BATTLE_OVERLAY_BASE, EffectListEntry};

        // The high-bit (`0x80`) effect-list entries route to the 2D efect.dat
        // pool (`FUN_801dfdf0`), not the 0x801f6324 scene-graph. They spawn
        // whether or not a 3D scene stages below: a move whose lists hold
        // *only* AltEffect entries has no Spawn prototype but still fires its
        // 2D effects.
        let alt_ids: Vec<u8> = fx
            .contact_effects
            .iter()
            .chain(fx.launch_effects.iter())
            .filter_map(|e| match e.entry {
                EffectListEntry::AltEffect(id) => Some(id),
                _ => None,
            })
            .collect();

        // The file offsets this move's Spawn entries point at (proto VA → file).
        let wanted: std::collections::BTreeSet<usize> = fx
            .contact_effects
            .iter()
            .chain(fx.launch_effects.iter())
            .filter_map(|e| match e.entry {
                EffectListEntry::Spawn(_) => e.proto,
                _ => None,
            })
            .filter_map(|va| va.checked_sub(BATTLE_OVERLAY_BASE).map(|o| o as usize))
            .collect();
        let trail_texpage = fx.trail_texpage;
        let sound_cue_id = fx.sound_cue_id;

        // A cast whose terminator is still ahead leaves both lists to the
        // homing flight it seeds (`World::seed_homing_slots`): only the
        // move's sound cue surfaces here (the trail texpage is scene-scoped).
        if self.casting.homing_takes_lists {
            if sound_cue_id != 0 {
                self.casting.pending_move_fx_cue = Some(sound_cue_id);
            }
            return true;
        }

        let mut staged_scene = false;
        if !wanted.is_empty() {
            // The 3D scene-graph path needs the retained battle-action overlay
            // (PROT 0898) for the prototype records; the 2D pool path does not.
            //
            // Parse ALL prototype records first (full offset set) so each
            // record's move-VM bytecode is bounded by its true packed
            // neighbour, then select the ones this move's Spawn entries
            // reference. Bounding against only this move's subset would
            // over-run each record into the next selected one rather than the
            // next packed one.
            if let Some(overlay) = self.tables.move_power_overlay.clone()
                && let Some(all_parts) = move_power::parse_effect_proto_records(&overlay)
            {
                let parts: Vec<legaia_overlay_images::summon_overlay::SummonPart> = all_parts
                    .into_iter()
                    .filter(|p| wanted.contains(&p.record_off))
                    .collect();
                if !parts.is_empty() {
                    self.casting.active_move_fx =
                        Some(legaia_engine_effects::summon::SummonScene::spawn_parts(
                            &parts,
                            &overlay,
                            crate::world::EFFECT_MODEL_LIBRARY_BASE,
                            origin,
                        ));
                    // Surface the move's presentation fields for the
                    // render / audio layers: the trail/afterimage texpage
                    // (`+0x0b`) and the sound cue (`+0x0d`). The texpage is
                    // scene-scoped (dropped when the scene drains), so it
                    // only surfaces when a scene actually stages.
                    self.casting.move_fx_trail_texpage = Some(trail_texpage);
                    if sound_cue_id != 0 {
                        self.casting.pending_move_fx_cue = Some(sound_cue_id);
                    }
                    staged_scene = true;
                }
            }
        }

        let spawned_alt = !alt_ids.is_empty();
        for id in alt_ids {
            self.try_spawn_effect(id, origin, 0);
        }
        staged_scene || spawned_alt
    }

    /// Live table-form effect-script scenes [`Self::spawn_action_table_effect`]
    /// keeps at once. Retail allocates from a `0x60`-slot pool shared with
    /// every other effect actor; a battle action's script names at most eight
    /// records, so a small cap keeps the honest shape without the pool.
    const ACTION_FX_CAP: usize = 8;

    /// Spawn one **table-form effect-script record**: effect id `0x00..=0x31`
    /// indexes the `0x801F6324` prototype table for a summon-format move-VM
    /// part record, staged as its own small scene at `origin` - the engine
    /// analogue of the effect-script walk's `FUN_80050ED4` arm
    /// (`FUN_801DEA50`, `0x801df168..0x801df194`: `a2 = 0x801F6324[id*4]`).
    /// The per-effect **CLUT source x** the same arm reads (`0x801F6418[id]`,
    /// `0x801df0ec..0x801df134`) is not staged here but at the queue's drain:
    /// `World::drain_battle_effect_spawns` pushes it onto
    /// [`crate::world::BattleState::clut_stages`] under the retail gate (plain code below
    /// `0x32`, non-zero map byte), so callers routing an effect-script spawn
    /// through this get the palette swap without a second copy. It is a 16x1
    /// `MoveImage` onto `(224, 476)`, not a sound cue - see
    /// [`legaia_engine_battle::battle_effect_clut`].
    ///
    /// This form seats its parts at unit scale with zeroed angles - the
    /// cue-group arm's spawn. The effect-script walk's own spawn passes the
    /// arm's rotation and scale: [`Self::spawn_action_table_effect_posed`].
    ///
    /// No-op (returns `false`) without an installed move-power catalog /
    /// overlay (disc-free battles), for an id outside the prototype table,
    /// or at the concurrency cap. Ticked by [`Self::tick_move_fx`]; drawn
    /// through [`Self::active_move_fx_part_draws`].
    // PORT: FUN_801DEA50 (the table-form spawn arm; the pool allocator it
    // calls, FUN_80050ED4, is modeled by the scene list + cap).
    pub fn spawn_action_table_effect(&mut self, effect_id: u8, origin: [i16; 3]) -> bool {
        self.spawn_action_table_effect_posed(
            effect_id,
            origin,
            [0; 3],
            legaia_engine_effects::summon::SPAWN_RENDER_SCALE,
        )
    }

    /// The effect-script walk's table-form spawn with the arm's own pose
    /// (`FUN_801DEA50`, `0x801DF098..0x801DF194`): the rotation trio is
    /// `(0, actor[+0x46] + 0x800, 0)` - the spawning actor's facing turned
    /// half a circle (`lhu v0,0x46(v0)` / `addiu v0,v0,0x800` /
    /// `sh v0,0x1a(sp)`, the two other angles zeroed) - and the scale is
    /// the code's base ([`action_fx_base_scale`]) times the actor's
    /// mesh-header `+0x72` over `0x1000`. The seater `FUN_80021B04` copies
    /// the trio into the render banks `+0x24..+0x28`, `rot[1] & 0xFFF` into
    /// the heading `+0x96` and the scale into `+0x72`. Codes `4..=6` then
    /// seat two more records on the same pose, the prototypes `0x801F5E28`
    /// and `0x801F5E6C` (`0x801DF1E8..0x801DF22C`).
    ///
    /// Without the turn every cast-clip ray burst (`nighto_summon_mid_cast`,
    /// `swordie_summon_mid_cast`, `freed_summon_mid_cast`) spun from yaw `0`
    /// rather than from the caster's back-facing: the nighto capture's
    /// node reads `+0x26 = 1376`, 26 frames after a seat at `840 + 0x800`
    /// spinning `-56` a frame.
    ///
    /// The code `0` -> `9` substitution when the active actor's `+0x1D9`
    /// reads `0x11` (`0x801DF054..0x801DF094`) happens upstream, where the
    /// walk queues the spawn (`World::step_actor_effect_script`), so
    /// `effect_id` arrives already substituted.
    pub fn spawn_action_table_effect_posed(
        &mut self,
        effect_id: u8,
        origin: [i16; 3],
        rot: [i16; 3],
        scale: u16,
    ) -> bool {
        if self.casting.active_action_fx.len() >= Self::ACTION_FX_CAP {
            return false;
        }
        let Some(off) = self
            .tables
            .move_power
            .as_ref()
            .and_then(|cat| cat.aux_tables())
            .and_then(|aux| aux.proto_record_offset(effect_id))
        else {
            return false;
        };
        let Some(overlay) = self.tables.move_power_overlay.clone() else {
            return false;
        };
        use legaia_game_tables::move_power;
        let Some(all_parts) = move_power::parse_effect_proto_records(&overlay) else {
            return false;
        };
        let mut offs: Vec<usize> = all_parts.iter().map(|p| p.record_off).collect();
        let mut parts: Vec<legaia_overlay_images::summon_overlay::SummonPart> = all_parts
            .into_iter()
            .filter(|p| p.record_off == off)
            .collect();
        if parts.is_empty() {
            return false;
        }
        if (4..=6).contains(&effect_id) {
            let twins: Vec<usize> = ACTION_FX_TWIN_RECORDS
                .iter()
                .filter_map(|va| va.checked_sub(move_power::BATTLE_OVERLAY_BASE))
                .map(|o| o as usize)
                .collect();
            offs.extend(&twins);
            parts.extend(
                legaia_overlay_images::summon_overlay::parse_records_at(&overlay, &offs)
                    .into_iter()
                    .filter(|p| twins.contains(&p.record_off)),
            );
        }
        parts.retain(|p| p.record_off < overlay.len());
        let mut scene = legaia_engine_effects::summon::SummonScene::spawn_parts(
            &parts,
            &overlay,
            crate::world::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        );
        // Each part keeps its record's address in the resident battle
        // overlay - the `+0x48` word a retail node carries - so a capture
        // can find the replay's part of the record a state's node runs
        // ([`Self::install_action_fx_part_snapshot`]).
        for (part, p) in scene.parts.iter_mut().zip(&parts) {
            part.tag = Some(move_power::BATTLE_OVERLAY_BASE + p.record_off as u32);
        }
        scene.pose_parts(rot, scale);
        self.casting.active_action_fx.push(scene);
        true
    }

    /// Seat the live effect-script table parts of a captured state over the
    /// replay's parts of the same records
    /// ([`legaia_engine_effects::summon::CastPartSeed`], tagged with the
    /// record's battle-overlay address): the capture harness's twin of
    /// [`legaia_engine_effects::summon::SummonScene::install_part_snapshot`]
    /// for the table-form scenes, which are one scene per spawn rather than
    /// one scene per cast. Seeds go onto the live parts of their tag in
    /// scene order, one each; a seed with no part left is skipped and a
    /// part with no seed is left alone - the snapshot re-poses parts the
    /// replay spawned, it neither invents nor retires one.
    pub fn install_action_fx_part_snapshot(
        &mut self,
        seeds: &[legaia_engine_effects::summon::CastPartSeed],
    ) {
        let mut used: std::collections::BTreeMap<u32, usize> = Default::default();
        for seed in seeds {
            let n = used.entry(seed.tag).or_insert(0);
            let Some(part) = self
                .casting
                .active_action_fx
                .iter_mut()
                .flat_map(|s| s.parts.iter_mut())
                .filter(|p| !p.finished && p.tag == Some(seed.tag))
                .nth(*n)
            else {
                continue;
            };
            *n += 1;
            for &(off, value) in &seed.pokes {
                // The depth-cue level `+0x78` is the snapshot's alone: no
                // stager arm writes it into a part it seats, so it is not
                // one of `poke`'s offsets.
                if off == 0x78 {
                    part.state.field_78 = value;
                } else {
                    part.poke(off, value);
                }
            }
        }
    }

    /// Seat one battle-overlay record by its address, as `FUN_80050ED4`
    /// does for a literal record pointer: the record's parts at `origin`,
    /// turned by `rot` and scaled by `scale` (`+0x72`), as their own scene
    /// in [`crate::world::CastFxState::active_action_fx`]. Returns whether it
    /// seated.
    pub fn spawn_battle_record_posed(
        &mut self,
        record_va: u32,
        origin: [i16; 3],
        rot: [i16; 3],
        scale: u16,
    ) -> bool {
        use legaia_game_tables::move_power;
        if self.casting.active_action_fx.len() >= Self::ACTION_FX_CAP {
            return false;
        }
        let Some(overlay) = self.tables.move_power_overlay.clone() else {
            return false;
        };
        let Some(off) = record_va
            .checked_sub(move_power::BATTLE_OVERLAY_BASE)
            .map(|o| o as usize)
        else {
            return false;
        };
        let mut offs: Vec<usize> = move_power::parse_effect_proto_records(&overlay)
            .unwrap_or_default()
            .iter()
            .map(|p| p.record_off)
            .collect();
        offs.push(off);
        let parts: Vec<_> =
            legaia_overlay_images::summon_overlay::parse_records_at(&overlay, &offs)
                .into_iter()
                .filter(|p| p.record_off == off)
                .collect();
        if parts.is_empty() {
            return false;
        }
        let mut scene = legaia_engine_effects::summon::SummonScene::spawn_parts(
            &parts,
            &overlay,
            crate::world::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        );
        // Tagged with the record's address like a table-form part, so a
        // capture's snapshot finds it: Tail Fire's floor ring (record
        // `0x801F5EB0`, seated here on the struck body's pair) went untagged
        // and kept the replay's own clock under every seed.
        for (part, p) in scene.parts.iter_mut().zip(&parts) {
            part.tag = Some(move_power::BATTLE_OVERLAY_BASE + p.record_off as u32);
        }
        scene.pose_parts(rot, scale);
        self.casting.active_action_fx.push(scene);
        true
    }

    /// Seat what the effect spawner and move-VM op `0x17` queued this step
    /// ([`crate::world::CastFxState::pending_burst_triggers`] /
    /// [`crate::world::CastFxState::pending_bursts`]) into the effect-script
    /// scene list.
    ///
    /// A trigger is the one-part program `FUN_801DFDF0` seats ahead of effect
    /// ids `4` / `0x13` (`WAIT_SET 0 / 0x17 mode / WAIT_SET 0 / HALT`); its
    /// `0x17` is the burst. A burst runs `FUN_801F30C4`'s twelve spawn blocks
    /// ([`vm::battle_burst::run_burst`]) and seats each child on the arm's
    /// stager record at the parent's position, with the parent's rotation
    /// trio and the block's yaw as `rot[1]`, then writes the block's scale
    /// `+0x72`, spread `+0x3E` and tail `+0x98` over it - the sprite-arm fire
    /// puffs `gimard_burning_attack` holds eight of (record `0x801F5DA4`,
    /// flags `0x380`, scales `0x800` / `0x1000`). No-op without the battle
    /// overlay image (disc-free battles).
    pub(crate) fn flush_battle_bursts(&mut self) {
        let triggers = std::mem::take(&mut self.casting.pending_burst_triggers);
        let bursts = std::mem::take(&mut self.casting.pending_bursts);
        if triggers.is_empty() && bursts.is_empty() {
            return;
        }
        let Some(overlay) = self.tables.move_power_overlay.clone() else {
            return;
        };
        use legaia_game_tables::move_power::{self, BATTLE_OVERLAY_BASE};
        let mut offs: Vec<usize> = move_power::parse_effect_proto_records(&overlay)
            .unwrap_or_default()
            .iter()
            .map(|p| p.record_off)
            .collect();
        let record_at = |offs: &mut Vec<usize>, va: u32| {
            let off = va.checked_sub(BATTLE_OVERLAY_BASE)? as usize;
            offs.push(off);
            legaia_overlay_images::summon_overlay::parse_records_at(&overlay, offs)
                .into_iter()
                .find(|p| p.record_off == off)
        };
        for (va, pos, angle) in triggers {
            let Some(part) = record_at(&mut offs, va) else {
                continue;
            };
            if self.casting.active_action_fx.len() >= Self::ACTION_FX_CAP {
                continue;
            }
            let mut scene = legaia_engine_effects::summon::SummonScene::spawn_parts(
                &[],
                &overlay,
                crate::world::EFFECT_MODEL_LIBRARY_BASE,
                pos,
            );
            scene.push_parts(
                std::slice::from_ref(&part),
                &overlay,
                pos,
                [0, angle as i16, 0],
            );
            self.casting.active_action_fx.push(scene);
        }
        struct Host<'w> {
            world: &'w mut World,
            yaw: i16,
            scale: u16,
            out: Vec<vm::battle_burst::SpawnRequest>,
        }
        impl vm::battle_burst::BurstHost for Host<'_> {
            fn rand(&mut self) -> i32 {
                self.world.next_rand() as i32
            }
            fn sin(&self, index: u32) -> i16 {
                legaia_engine_system::trig::trig12(index as u16).0
            }
            fn cos(&self, index: u32) -> i16 {
                legaia_engine_system::trig::trig12(index as u16).1
            }
            fn parent_yaw(&self) -> i16 {
                self.yaw
            }
            fn parent_scale(&self) -> u16 {
                self.scale
            }
            fn spawn(
                &mut self,
                _record_addr: u32,
                request: vm::battle_burst::SpawnRequest,
            ) -> Option<u32> {
                self.out.push(request);
                Some(self.out.len() as u32)
            }
        }
        for b in bursts {
            let Some(mode) = vm::battle_burst::BurstMode::from_arg(b.mode) else {
                continue;
            };
            let Some(part) = record_at(&mut offs, mode.record_addr()) else {
                continue;
            };
            let mut host = Host {
                world: self,
                yaw: b.rot[1],
                scale: b.scale,
                out: Vec::new(),
            };
            vm::battle_burst::run_burst(&mut host, b.mode);
            let requests = host.out;
            let mut scene = legaia_engine_effects::summon::SummonScene::spawn_parts(
                &[],
                &overlay,
                crate::world::EFFECT_MODEL_LIBRARY_BASE,
                b.pos,
            );
            for req in requests {
                scene.push_parts(
                    std::slice::from_ref(&part),
                    &overlay,
                    b.pos,
                    [b.rot[0], req.yaw, b.rot[2]],
                );
                if let Some(child) = scene.parts.last_mut() {
                    // The record's battle-overlay address, as a retail
                    // child's `+0x48` reads, so a capture's live children
                    // find theirs ([`Self::install_action_fx_part_snapshot`]).
                    child.tag = Some(mode.record_addr());
                    child.state.field_72 = req.scale as u16;
                    child.state.anim_3e = req.spread;
                    child.state.tween_scale_y = req.tail;
                }
            }
            if self.casting.active_action_fx.len() < Self::ACTION_FX_CAP {
                self.casting.active_action_fx.push(scene);
            }
        }
    }

    /// The seater's own VM run for every battle FX part seated since the
    /// last tick ([`legaia_engine_effects::summon::SummonScene::seat_run`]), over the
    /// summon, move-FX and effect-script scenes. Run after each batch of
    /// seats so no part reaches a draw before its program has.
    pub fn seat_run_battle_fx(&mut self) {
        let mut scenes: Vec<legaia_engine_effects::summon::SummonScene> = Vec::new();
        scenes.extend(self.casting.active_summon.take());
        let summon_n = scenes.len();
        scenes.extend(self.casting.active_move_fx.take());
        let move_n = scenes.len();
        scenes.append(&mut self.casting.active_action_fx);
        let channel_delta = self.effect_channel_delta();
        let step = self.move_vm.ramp_ratio.max(1);
        for scene in &mut scenes {
            scene.channel_delta = channel_delta;
            let seated = {
                let mut host = MoveVmHostImpl {
                    world: self,
                    current_slot: None,
                    deferred_writes: std::collections::BTreeMap::new(),
                    field_record_words: None,
                    child_spawns: Vec::new(),
                };
                scene.seat_run(&mut host)
            };
            // The seat frame's part tick carries the envelope tail
            // `FUN_800204F8` (`jal` at `0x80022EF4`) like any other.
            for i in seated {
                let part = &mut scene.parts[i];
                if !part.finished
                    && part.state.flags & vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE != 0
                {
                    vm::vdf_morph::envelope_tick_actor(&mut part.state, step);
                }
            }
        }
        let mut it = scenes.into_iter();
        if summon_n == 1 {
            self.casting.active_summon = it.next();
        }
        if move_n > summon_n {
            self.casting.active_move_fx = it.next();
        }
        // Any scene a seat run spawned lands behind the ones it ran.
        let spawned = std::mem::take(&mut self.casting.active_action_fx);
        self.casting.active_action_fx = it.chain(spawned).collect();
    }

    /// Take the pending move-FX sound cue id, if [`Self::spawn_move_fx`] set one
    /// this step. The host routes it through `legaia_engine_audio::classify_cue`
    /// (the `FUN_8004fcc8` dispatch) → the SFX ring / voice trigger. Returns
    /// `None` when no cue is pending.
    pub fn take_pending_move_fx_cue(&mut self) -> Option<u8> {
        self.casting.pending_move_fx_cue.take()
    }

    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, or `None` when none is playing. The render layer applies
    /// it to the move's streak pass.
    pub fn active_move_fx_trail_texpage(&self) -> Option<u16> {
        self.casting.move_fx_trail_texpage
    }

    /// Advance the active move-FX scene one frame through the move VM (the
    /// move-FX sibling of [`Self::tick_summon`]). No-op when none is playing;
    /// drains the scene once every part has finished.
    pub fn tick_move_fx(&mut self, frame_delta: u16) {
        // What the effect spawner queued since the last tick seats first, so
        // its trigger runs this tick as retail's does on the frame after the
        // seat; what this tick's op `0x17`s queue seats at the end.
        self.flush_battle_bursts();
        let channel_delta = self.effect_channel_delta();
        // Effect-script table-form scenes: take the list, tick each, keep the
        // unfinished. Their wait timers drain at retail's own rate - the part
        // tick subtracts `DAT_1F800393 * DAT_1F80037D` from `+0x54` per frame
        // (`FUN_80021DF4`, `0x80021E40..0x80021E4C`), i.e. the channel delta,
        // so a `WAIT_SET v` (`+0x54 = v << 3`) holds `v` frames. Under the
        // scene-graph step `0x400` every wait under 128 frames expired in one
        // tick: the Spirit charge's aura prototypes (`0x07` / `0x08`, a
        // `0x4F`-frame hold each) lived three ticks and never showed.
        let mut action_fx = std::mem::take(&mut self.casting.active_action_fx);
        for scene in &mut action_fx {
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
                field_record_words: None,
                child_spawns: Vec::new(),
            };
            scene.channel_delta = channel_delta;
            scene.tick(&mut host, channel_delta);
            self.queue_battle_part_scrolls(&mut scene.parts);
            // The morph-lane ramp envelope `FUN_80020740`, which the part
            // tick runs on a part whose `+0x10` word carries the op-`0x0A`
            // bit `0x1000` - the same gate the field ambient tree takes. It
            // is what moves the lane weights [`Self::morphed_part_tmd`]
            // blends at.
            // The envelope steps by `DAT_1F800393` alone (`+0xA0 +=
            // +0xB8 * DAT_1F800393`, `FUN_80020740`) - the frame byte, whose
            // stand-in is the move-VM ramp ratio the channel delta is built
            // from, not the field cadence `frame_step`. Mid-Spirit captures
            // pin it: the aura lane gains `0x66` per frame while the wait
            // drains `8`, so the cone takes `0x28` frames to open.
            //
            // The envelope tail `FUN_800204F8` belongs to the per-frame part
            // tick (`jal` at `0x80022EF4`, after the VM call); the spawn's
            // own first VM run has no tail. So the spawn tick - the engine's
            // first tick of a table-form scene, which runs that first VM
            // step - grows nothing: the captures read `0x66 * N` at wait
            // `0x278 - 8 * N`.
            let step = self.move_vm.ramp_ratio.max(1);
            for part in &mut scene.parts {
                if !part.finished
                    && part.state.flags & vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE != 0
                {
                    vm::vdf_morph::envelope_tick_actor(&mut part.state, step);
                }
            }
        }
        action_fx.retain(|s| !s.finished());
        self.casting.active_action_fx = action_fx;
        self.flush_battle_bursts();
        self.tick_mystic_shield_fx(channel_delta);

        let Some(mut scene) = self.casting.active_move_fx.take() else {
            return;
        };
        {
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
                field_record_words: None,
                child_spawns: Vec::new(),
            };
            scene.channel_delta = channel_delta;
            scene.tick(&mut host, frame_delta);
        }
        self.queue_battle_part_scrolls(&mut scene.parts);
        if !scene.finished() {
            self.casting.active_move_fx = Some(scene);
        } else {
            // Scene drained: drop the trail texpage with it.
            self.casting.move_fx_trail_texpage = None;
        }
    }

    /// Per-part render draws for the active move-FX scene's mesh-bearing parts
    /// plus the live effect-script table-form scenes
    /// ([`crate::world::CastFxState::active_action_fx`]) - one render seam so hosts that already
    /// draw move FX draw the effect-script spawns too. Empty when nothing is
    /// playing. Each draw's `model_index` indexes [`Self::global_tmd_pool`]
    /// (the PROT 0871 effect-model library).
    pub fn active_move_fx_part_draws(&self) -> Vec<legaia_engine_effects::summon::SummonPartDraw> {
        let mut out = self
            .casting
            .active_move_fx
            .as_ref()
            .map(|s| s.part_draws())
            .unwrap_or_default();
        for scene in &self.casting.active_action_fx {
            out.extend(scene.part_draws());
        }
        out
    }

    /// The mesh a part draw renders: its pool TMD with the part's live VDF
    /// morph lanes staged onto every group they name (`FUN_8001C604` +
    /// `FUN_8005B038`, [`legaia_scene_formats::vdf_morph::sum_group_deltas`]), or `None` when
    /// the part carries no armed lane or no lane resolves (the host draws
    /// the rest mesh from [`Self::global_tmd`]). In battle the lanes index
    /// the battle pack `vdf.dat` ([`crate::world::DiscTables::battle_vdf`]);
    /// elsewhere the scene's VDF.
    ///
    /// This is what gives the Spirit charge its aura: its two effect-script
    /// prototypes are small rest meshes (pool `14` / `15`) that op `0x0A`
    /// grows through `vdf.dat` entry 12 into the cone around the actor.
    /// Both hosts resolve every move-FX part draw through it.
    pub fn morphed_part_tmd(
        &self,
        draw: &legaia_engine_effects::summon::SummonPartDraw,
    ) -> Option<legaia_engine_scene::model_bank::GlobalTmd> {
        let lanes = draw.morph.lanes();
        if lanes.is_empty() {
            return None;
        }
        let rest = self.global_tmd(draw.model_index as i16)?;
        let resolve = |idx: u8| -> Option<&[u8]> {
            if self.mode == legaia_engine_mode::mode::SceneMode::Battle {
                legaia_engine_scene::scene_bundle::vdf_entry(
                    self.tables.battle_vdf.as_deref()?,
                    idx,
                )
            } else {
                self.vdf_record_bytes(idx)
            }
        };
        let slots: Vec<(&[u8], i16)> = lanes
            .iter()
            .filter_map(|&(idx, w)| Some((resolve(idx)?, w as i16)))
            .collect();
        if slots.is_empty() {
            return None;
        }
        let mut tmd = rest.tmd.clone();
        for (group, obj) in tmd.objects.iter_mut().enumerate() {
            let deltas = legaia_scene_formats::vdf_morph::sum_group_deltas(
                obj.vertices.len(),
                group as u32,
                &slots,
            );
            for (v, d) in obj.vertices.iter_mut().zip(deltas) {
                v.x = v.x.wrapping_add(d[0]);
                v.y = v.y.wrapping_add(d[1]);
                v.z = v.z.wrapping_add(d[2]);
            }
        }
        Some(legaia_engine_scene::model_bank::GlobalTmd {
            tmd,
            raw: rest.raw.clone(),
        })
    }

    /// The local-space mesh a summon / move-FX part draw renders: its
    /// morphed or rest mesh ([`Self::morphed_part_tmd`], else the pool
    /// TMD) with the part's render scale and colour word applied
    /// ([`legaia_engine_effects::summon::SummonPartDraw::apply_render_state`]) - the model
    /// draw `0x8001B160` hands every part through. `None` when the part's
    /// mesh is not in the pool. Both hosts draw every part whose
    /// [`legaia_engine_effects::summon::SummonPartDraw::draws_rest_mesh`] is false through
    /// this kernel: it is what fades the Spirit aura in and out.
    pub fn part_draw_vram_mesh(
        &self,
        draw: &legaia_engine_effects::summon::SummonPartDraw,
    ) -> Option<legaia_tmd::mesh::VramMesh> {
        let morphed = self.morphed_part_tmd(draw);
        let g = match morphed.as_ref() {
            Some(g) => g,
            None => self.global_tmd(draw.model_index as i16)?.as_ref(),
        };
        let mut mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&g.tmd, &g.raw);
        draw.apply_render_state(&mut mesh);
        Some(mesh)
    }

    /// Drain a pending non-summon move-FX spawn request (the host calls
    /// [`Self::spawn_move_fx`] with it). See [`crate::world::CastFxState::pending_move_fx_spawn`].
    pub fn take_pending_move_fx_spawn(&mut self) -> Option<(u8, [i16; 3])> {
        self.casting.pending_move_fx_spawn.take()
    }

    /// Request a move-FX spawn for the non-summon move `move_id` at `origin`,
    /// but only when the move-power table is installed and the move's record
    /// carries a spawnable effect entry ([`legaia_engine_battle::move_power::MovePowerCatalog::move_has_spawn_fx`]).
    /// No-op otherwise (plain physical hits, disc-free battles with no table).
    /// Idempotent within a step (last cast wins). The move-FX sibling of
    /// [`Self::request_summon_spawn`]; Seru-summon ids go through that instead.
    pub(crate) fn request_move_fx_spawn(&mut self, move_id: u8, origin: [i16; 3]) {
        if self
            .tables
            .move_power
            .as_ref()
            .is_some_and(|cat| cat.move_has_spawn_fx(move_id))
        {
            self.casting.pending_move_fx_spawn = Some((move_id, origin));
        }
    }
}

/// The two records the effect-script walk's table arm seats beside codes
/// `4..=6` (`addiu a2,a2,0x5e28` / `0x5e6c` ahead of the two
/// `jal 0x80050ED4` at `0x801DF210` / `0x801DF228`).
const ACTION_FX_TWIN_RECORDS: [u32; 2] = [0x801F_5E28, 0x801F_5E6C];

/// The table arm's base spawn scale for an effect code (`li s7,0x1000` at
/// `0x801DEC74`; `0xC00` for code `4` at `0x801DF0C4`, `0x2000` for code `6`
/// at `0x801DF0D0`), before the actor's mesh-header `+0x72` scales it.
pub(crate) fn action_fx_base_scale(effect_id: u8) -> u16 {
    match effect_id {
        4 => 0xC00,
        6 => 0x2000,
        _ => 0x1000,
    }
}
