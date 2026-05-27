//! Per-VM `Host` trait implementations that bridge each clean-room VM into
//! [`World`]. Split out of `world.rs`.

use super::*;

use crate::battle_events::BattleEvent;
use crate::field_events::FieldEvent;
use legaia_engine_vm as vm;
use vm::battle_action::{BattleActionHost, BattleActor, BattleEndCause, Pose};
use vm::effect_vm::{EffectHost, MasterSlot, StateOutcome};
use vm::field::{CameraParam, FieldCtx, FieldHost, Op49State, SceneFadeResult};
use vm::move_vm::{ActorState as MoveActorState, MoveHost};
use vm::{Host as ActorVmHost, Position as ActorVmPosition};

// --- actor VM host ---------------------------------------------------------

pub(super) struct ActorVmHostImpl<'a> {
    pub(super) world: &'a mut World,
}

impl<'a> ActorVmHost for ActorVmHostImpl<'a> {
    fn actor_exists(&self, actor_id: u8) -> bool {
        self.world
            .actors
            .get(actor_id as usize)
            .is_some_and(|a| a.active)
    }
    fn default_position(&self, actor_id: u8) -> ActorVmPosition {
        self.world
            .actors
            .get(actor_id as usize)
            .map(|a| a.default_pos)
            .unwrap_or_default()
    }
    fn spawn(&mut self, actor_id: u8, default_position: ActorVmPosition) {
        let a = &mut self.world.actors[actor_id as usize];
        if !a.active {
            *a = Actor::new();
            a.active = true;
        }
        a.default_pos = default_position;
        a.move_state.world_x = default_position.x;
        a.move_state.world_y = default_position.y;
    }
    fn set_position(&mut self, actor_id: u8, p: ActorVmPosition) {
        let a = &mut self.world.actors[actor_id as usize];
        a.move_state.world_x = p.x;
        a.move_state.world_y = p.y;
    }
    fn start_motion(&mut self, _actor_id: u8, _target: ActorVmPosition) {
        // Engines typically schedule a tween here; the world records nothing
        // by default.
    }
    fn delete_sprite(&mut self, actor_id: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.active = false;
        }
    }
    fn global_update(&mut self) {
        // Tick whatever per-frame sprite-system state advances. The default
        // world has no global sprite ticker, but engines override this.
    }
    fn actor_effect(&mut self, actor_id: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.last_effect = a.last_effect.wrapping_add(1);
        }
    }
    fn set_field_1d(&mut self, actor_id: u8, value: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.field_1d = value;
        }
    }
    fn clear_field_20(&mut self, actor_id: u8) {
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.field_20 = 0;
        }
    }
    fn snap_clear_condition(&self, actor_id: u8) -> bool {
        self.world
            .actors
            .get(actor_id as usize)
            .map(|a| a.snap_clear)
            .unwrap_or(false)
    }
    fn motion_target(&self, actor_id: u8) -> Option<ActorVmPosition> {
        self.world
            .actors
            .get(actor_id as usize)
            .and_then(|a| a.motion_target)
    }
}

// --- move VM host ----------------------------------------------------------

pub(super) struct MoveVmHostImpl<'a> {
    pub(super) world: &'a mut World,
    /// Actor slot currently being stepped. Routes `move_bytecode_*` callbacks
    /// to the right `world.move_bytecode[slot]` buffer and the `*_slot_*`
    /// table reads to per-slot scratch (the shared 16-slot table is global,
    /// not per actor; this is unused there).
    pub(super) current_slot: Option<usize>,
    /// Deferred bytecode writes accumulated during one `step` call. The VM
    /// borrows `world.move_bytecode[slot]` immutably as the bytecode slice;
    /// we can't write back through the same borrow, so the host buffers
    /// writes and `step_move_vm` flushes them after step returns.
    ///
    /// Reads consult this map first so an in-flight write within the same
    /// step (e.g. 0x1B copy loop reading from a freshly-mutated word) sees
    /// the latest value.
    pub(super) deferred_writes: std::collections::BTreeMap<usize, u16>,
}

impl<'a> MoveHost for MoveVmHostImpl<'a> {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let idx = index as usize % self.world.sin_lut.len().max(1);
        let s = self.world.sin_lut.get(idx).copied().unwrap_or(0);
        let c = self.world.cos_lut.get(idx).copied().unwrap_or(0);
        (s, c)
    }
    fn keyframe_curve_multiplier(&self) -> u8 {
        // Default mirrors retail's startup-time write of `DAT_1F80037D`.
        0x10
    }

    // --- ext-VM globals -----------------------------------------------

    fn move_global_predicate_get(&self) -> u32 {
        self.world.move_predicate
    }
    fn move_global_predicate_set(&mut self, value: u32) {
        self.world.move_predicate = value;
    }
    fn move_global_counter_get(&self) -> u16 {
        self.world.move_counter
    }
    fn move_global_counter_set(&mut self, value: u16) {
        self.world.move_counter = value;
    }

    // --- ext-VM 16-slot scratch table ---------------------------------

    fn move_slot_load_u32(&self, slot: u16, dword_off: u8) -> u32 {
        let i = (slot & 0x0F) as usize;
        let off = (dword_off & 0x4) as usize; // 0 or 4
        let bytes = &self.world.move_slot_table[i][off..off + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
    fn move_slot_save_u32(&mut self, slot: u16, dword_off: u8, value: u32) {
        let i = (slot & 0x0F) as usize;
        let off = (dword_off & 0x4) as usize;
        self.world.move_slot_table[i][off..off + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn move_slot_load_u16(&self, slot: u16, byte_off: u8) -> u16 {
        let i = (slot & 0x0F) as usize;
        let off = (byte_off & 0x6) as usize; // even, 0..6
        let bytes = &self.world.move_slot_table[i][off..off + 2];
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
    fn move_slot_save_u16(&mut self, slot: u16, byte_off: u8, value: u16) {
        let i = (slot & 0x0F) as usize;
        let off = (byte_off & 0x6) as usize;
        self.world.move_slot_table[i][off..off + 2].copy_from_slice(&value.to_le_bytes());
    }

    // --- bytecode self-modify (0x04 / 0x1B / 0x1E) --------------------

    fn move_bytecode_read_u16(&self, word_off: usize) -> u16 {
        if let Some(&v) = self.deferred_writes.get(&word_off) {
            return v;
        }
        let Some(slot) = self.current_slot else {
            return 0;
        };
        self.world
            .move_bytecode
            .get(slot)
            .and_then(|bc| bc.get(word_off))
            .copied()
            .unwrap_or(0)
    }
    fn move_bytecode_write_u16(&mut self, word_off: usize, value: u16) {
        self.deferred_writes.insert(word_off, value);
    }

    // --- player / map-origin queries ----------------------------------

    fn move_player_world_xyz(&self) -> [i16; 3] {
        match self.world.player_actor_slot {
            Some(slot) => {
                let s = &self.world.actors[slot as usize].move_state;
                [s.world_x, s.world_y, s.world_z]
            }
            None => [0, 0, 0],
        }
    }
    fn move_fixed_origin_xz(&self) -> (i32, i32) {
        self.world.map_origin_xz
    }
    fn move_axis_threshold(&self) -> i16 {
        self.world.move_axis_threshold
    }
    fn move_dat_1f800393(&self) -> u8 {
        self.world.move_ramp_ratio
    }

    // --- shared system flag bank --------------------------------------

    fn ext_query_flag_bank(&self, flag_index: i16) -> u32 {
        if self.world.system_flag_test(flag_index as u16) {
            1
        } else {
            0
        }
    }
    fn ext_set_flag_bank(&mut self, flag_index: i16) {
        self.world.system_flag_set(flag_index as u16);
    }
    fn ext_clear_flag_bank(&mut self, flag_index: i16) {
        self.world.system_flag_clear(flag_index as u16);
    }

    // --- ext sub-op 0x29 scratchpad ramp ------------------------------

    fn ext_scratchpad_write(&mut self, slot_index: i16, value: i16) {
        let i = (slot_index as u16 & 0x0F) as usize;
        self.world.scratchpad_targets[i] = value;
    }
    fn ext_scratchpad_ramp(&mut self, slot_index: i16, target: i16, _ticks: i16) {
        // Default world has no per-frame ramp scheduler; record the target
        // immediately so reads see the final state. Engines override to
        // model the per-frame interpolation.
        let i = (slot_index as u16 & 0x0F) as usize;
        self.world.scratchpad_targets[i] = target;
    }

    // --- ext sub-op 0x2F global slot ---------------------------------

    fn ext_set_8007b9d8(&mut self, value: i32) {
        self.world.move_dat_8007b9d8 = value;
    }

    // --- ext sub-op 0x3A angle-to-player ------------------------------

    fn ext_compute_angle(&self, state: &MoveActorState) -> u16 {
        // Per the original: `func_0x80019B28(actor.world_z, actor.world_x,
        // player.world_z, player.world_x)`. Engines that don't model a
        // player slot get angle 0 (matching the no-player default).
        let Some(player_slot) = self.world.player_actor_slot else {
            return 0;
        };
        let player = &self.world.actors[player_slot as usize].move_state;
        // Atan2-style angle in PSX 12-bit units (4096 = full circle). The
        // original used a libgte angle helper; we use a portable
        // f32::atan2 then quantise. Direction convention matches the
        // original (Z first arg, X second).
        let dz = (player.world_z as i32 - state.world_z as i32) as f32;
        let dx = (player.world_x as i32 - state.world_x as i32) as f32;
        if dx == 0.0 && dz == 0.0 {
            return 0;
        }
        let theta = dz.atan2(dx);
        let units = (theta / std::f32::consts::TAU * 4096.0).round() as i32;
        (units & 0x0FFF) as u16
    }

    // --- ext sub-op 0x3B party-member position lookup ------------------

    fn ext_party_member_lookup(&self, slot: i16) -> Option<[i16; 3]> {
        let actor_slot = *self.world.party_actor_slots.get(slot as usize)?;
        let actor_slot = actor_slot? as usize;
        let st = &self.world.actors[actor_slot].move_state;
        Some([st.world_x, st.world_y, st.world_z])
    }

    // --- ext sub-op 0x3C fade colour -----------------------------------

    fn ext_fade_color(&mut self, rgb: [u8; 3], ticks: u16) {
        self.world.pending_fade = Some(FadeRequest { rgb, ticks });
    }

    // `ext_dispatch` uses the default trait impl, which routes through
    // `self` - so sub-op handlers see the world-backed callbacks above.
}

// --- effect VM host --------------------------------------------------------

pub(super) struct EffectHostImpl<'a> {
    pub(super) world: &'a mut World,
}

impl<'a> EffectHost for EffectHostImpl<'a> {
    fn next_random(&mut self) -> i32 {
        self.world.next_rng() as i32
    }
    fn advance_state(&mut self, _slot: usize, master: &mut MasterSlot) -> StateOutcome {
        // REF: FUN_801e0088
        // Clean-room lifetime: count elapsed frames in `field_14` (a scratch
        // word the retail walker manages during state advance) and retire the
        // effect after a fixed budget. Without this an effect terminates on
        // its first work tick and never persists long enough to render. The
        // faithful per-state token walk (retail `FUN_801E0088` pass 1) lands
        // with the textured-sprite render path; see
        // `effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES`.
        master.field_14 = master.field_14.saturating_add(1);
        if (master.field_14 as u32) >= vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES {
            StateOutcome::Terminate
        } else {
            StateOutcome::Continue
        }
    }
}

// --- field VM host ---------------------------------------------------------

/// Bridge between the ported world-map entity SM ([`vm::world_map::step`]) and
/// the [`World`]. One is constructed per [`Self::tick_world_map`]; the entity
/// `Vec` is taken out of the world while the SM runs so the bridge can hold a
/// `&mut World`, then put back.
pub(super) struct WorldMapEntityHostImpl<'a> {
    pub(super) world: &'a mut World,
}

impl<'a> vm::world_map::WorldMapEntityHost for WorldMapEntityHostImpl<'a> {
    fn activation_gate_open(&self) -> bool {
        // Retail gates the SM body on `_DAT_8007b868 == 0` (door/portal open).
        // The clean-room world has no closed-portal state yet, so the body
        // always runs when world-map entities are installed; the per-state
        // gates (encounter-enabled, dialog-active) still apply below.
        true
    }
    fn encounter_countdown(&self) -> i8 {
        self.world.world_map_encounter.countdown
    }
    fn set_encounter_countdown(&mut self, v: i8) {
        self.world.world_map_encounter.countdown = v;
    }
    fn encounter_enabled(&self) -> bool {
        self.world.world_map_encounter.enabled
    }
    fn on_encounter(&mut self, entity_idx: usize, _resolver_result: u32) {
        // Latch a formation for resolution into a battle at the end of the
        // world-map tick. Prefer this entity's own encounter-zone formation;
        // fall back to the map-wide shared formation. Pace the next encounter
        // by resetting the shared countdown.
        let formation_id = match self.world.world_map_entity_configs.get(entity_idx) {
            Some(WorldMapEntityConfig::EncounterZone { formation_id }) => *formation_id,
            _ => self.world.world_map_encounter.formation_id,
        };
        self.world.pending_world_map_encounter = Some(formation_id);
        self.world.world_map_encounter.countdown = self.world.world_map_encounter.reset_to;
    }
    fn on_activating(&mut self, _entity_idx: usize) {
        // Pending scene/portal data copy - no engine-side scene buffer yet.
    }
    fn on_scene_transition(&mut self, entity_idx: usize) {
        // A portal entity reached the transition state. When it carries a
        // target map, surface the richer transition event; otherwise fall back
        // to the generic interaction marker.
        match self.world.world_map_entity_configs.get(entity_idx) {
            Some(WorldMapEntityConfig::Portal { target_map }) => {
                let target_map = *target_map;
                self.world
                    .pending_field_events
                    .push(FieldEvent::WorldMapTransition {
                        target_map,
                        slot: entity_idx as u8,
                    });
            }
            _ => {
                self.world
                    .pending_field_events
                    .push(FieldEvent::FieldInteract {
                        interact_id: 0xFF,
                        slot: entity_idx as u8,
                    });
            }
        }
    }
    fn dialog_active(&self) -> bool {
        self.world.current_dialog.is_some()
    }
    fn player_walking(&self) -> bool {
        self.world.world_map_player_walking
    }
    fn on_interact(&mut self, entity_idx: usize) {
        let interact_id = match self.world.world_map_entity_configs.get(entity_idx) {
            Some(WorldMapEntityConfig::Npc { interact_id, .. }) => *interact_id,
            _ => 0,
        };
        self.world
            .pending_field_events
            .push(FieldEvent::FieldInteract {
                interact_id,
                slot: entity_idx as u8,
            });
    }
    fn encounter_counter_is_sentinel(&self) -> bool {
        false
    }
    fn clear_encounter_counter(&mut self) {}
}

/// Bridge between the ported `FUN_801DA51C` SM and a [`World`] **field**
/// carrier (the same SM the overworld bridge drives, but ticked in
/// [`SceneMode::Field`] for MAN-placed scene entities). Constructed per
/// [`World::tick_field_carriers`]; the carrier `Vec` is taken out of the world
/// while the SM runs.
///
/// The discriminating difference from [`WorldMapEntityHostImpl`]: field
/// carriers never fire a *random* encounter (towns run a 0% rate), so
/// `encounter_enabled` is `false` and the carrier only advances when
/// [`World::engage_field_carrier`] moves it to `Activating`. Its state-1 body
/// then `on_activating` -> installs the MAN formation by index, and the
/// fall-through `on_scene_transition` -> latches the battle handoff.
pub(super) struct FieldCarrierHostImpl<'a> {
    pub(super) world: &'a mut World,
}

impl<'a> vm::world_map::WorldMapEntityHost for FieldCarrierHostImpl<'a> {
    fn activation_gate_open(&self) -> bool {
        true
    }
    fn encounter_countdown(&self) -> i8 {
        // The dialogue-accept (`engage_field_carrier`) leaves the carrier at
        // Activating with a zero countdown, so the next tick runs the state-1
        // body to completion. Report 0 so a freshly-engaged carrier transitions
        // immediately rather than draining a stale counter.
        0
    }
    fn set_encounter_countdown(&mut self, _v: i8) {}
    fn encounter_enabled(&self) -> bool {
        // Scripted carriers are not random encounters - the Idle state must
        // never self-fire. Advancement is entirely via `engage_field_carrier`.
        false
    }
    fn on_encounter(&mut self, _entity_idx: usize, _resolver_result: u32) {}
    fn on_activating(&mut self, _entity_idx: usize) {
        // State-1 `entity[+0x94]` formation copy. Retail copies the carrier's
        // formation into the global cell here; the clean-room world latches it
        // in `on_scene_transition` (same state-1 tick) and resolves it from
        // `formation_table` directly at the end of the carrier tick, so no
        // persistent encounter session is created (a re-rolling session would
        // re-fire after the battle returns). No-op.
    }
    fn on_scene_transition(&mut self, entity_idx: usize) {
        // `case 2/3` fall-through battle handoff (`_DAT_8007b83c = 8`): latch
        // the carrier's MAN formation (by index, so the scene's merged monster
        // stats stand) for direct resolution at the end of the tick.
        if let Some(FieldCarrierConfig::ScriptedEncounter { formation_id }) =
            self.world.field_carrier_configs.get(entity_idx).cloned()
        {
            self.world.pending_field_carrier_battle = Some(formation_id);
        }
    }
    fn dialog_active(&self) -> bool {
        self.world.current_dialog.is_some()
    }
    fn player_walking(&self) -> bool {
        // Report "player walking" so the SM's proximity-interact path stays
        // suppressed: the clean-room world has no player-near-NPC model yet, so
        // a field carrier is engaged explicitly via `engage_field_carrier`
        // rather than by the SM's auto-interact gate (which would otherwise
        // re-fire `on_interact` every frame once its cooldown bit latched).
        true
    }
    fn on_interact(&mut self, entity_idx: usize) {
        // Reached only once a future proximity model opens the gate; surfaces
        // the carrier's interaction id for the host.
        let interact_id = match self.world.field_carrier_configs.get(entity_idx) {
            Some(FieldCarrierConfig::Npc { interact_id }) => *interact_id,
            _ => 0,
        };
        self.world
            .pending_field_events
            .push(FieldEvent::FieldInteract {
                interact_id,
                slot: entity_idx as u8,
            });
    }
    fn encounter_counter_is_sentinel(&self) -> bool {
        false
    }
    fn clear_encounter_counter(&mut self) {}
}

pub(super) struct FieldHostImpl<'a> {
    pub(super) world: &'a mut World,
}

impl<'a> FieldHost for FieldHostImpl<'a> {
    fn global_flags(&self) -> u32 {
        self.world.story_flags
    }
    fn set_global_flags(&mut self, value: u32) {
        self.world.story_flags = value;
    }
    fn frame_delta(&self) -> u16 {
        // Default world ticks one logical frame per `tick()`. Engines that
        // run faster-than-frame can override this on a custom host wrapper.
        1
    }
    fn extra_flags(&self) -> u32 {
        self.world.extra_flags
    }

    // Op-0x49 STATE_RESUME, scoped to the `town01` opening cutscene timeline.
    // The pinned name-entry handoff (P2[3] body `0x02c6`, `49 03 00`) suspends
    // the script here; the engine opens the name-entry overlay on the Idle->arm
    // edge and keeps the op Armed (parked) until the player commits, then Done
    // (resume). Outside the timeline (`in_cutscene_timeline == false`) and
    // outside the opening (`prologue_naming_pending == false`) these fall back
    // to the default Idle, so a normal field-VM op-0x49 behaves as before.
    // REF: FUN_801F03F0 (name-entry overlay) / op49_invoke_setup func_0x80020de0
    fn op49_state(&self) -> Op49State {
        if self.world.in_cutscene_timeline && self.world.prologue_naming_armed {
            if self.world.name_entry_active() {
                Op49State::Armed
            } else {
                Op49State::Done
            }
        } else {
            Op49State::Idle
        }
    }
    fn op49_invoke_setup(&mut self) {
        if self.world.in_cutscene_timeline
            && self.world.prologue_naming_pending
            && !self.world.prologue_naming_armed
            && !self.world.name_entry_active()
        {
            // Lead character (party slot 0 = Vahn) is the one named at the
            // opening, matching the retail char-record pointer `_DAT_8007B450`.
            self.world.open_name_entry(0);
            self.world.prologue_naming_armed = true;
        }
    }
    fn screen_mode(&self) -> u32 {
        self.world.screen_mode
    }

    // Shared system flag bank - same fourth-flag-bank at `_DAT_80086D70`
    // that move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D query, plus the
    // 0x5x / 0x6x / 0x7x default-route opcodes.
    fn system_flag_set(&mut self, idx: u16) {
        self.world.system_flag_set(idx);
    }
    fn system_flag_clear(&mut self, idx: u16) {
        self.world.system_flag_clear(idx);
    }
    fn system_flag_test(&self, idx: u16) -> bool {
        self.world.system_flag_test(idx)
    }
    fn scene_transition(&mut self, map_id: u8) {
        // Record the request; SceneHost::tick drains it after the field
        // step returns so the bytecode swap doesn't invalidate the
        // borrow we're stepping through.
        self.world.pending_scene_transition = Some(map_id);
    }

    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8) {
        // Named scene-change (op 0x3F): the destination name is inline, so no
        // map-id resolver is needed. Recorded for SceneHost::tick to drain,
        // the same deferral as the map-id path above (the bytecode swap can't
        // run while we're stepping through it).
        self.world.pending_named_scene_transition = Some((scene.to_string(), entry_x, entry_z));
    }

    fn is_scripted_encounter_armed(&self) -> bool {
        self.world.scripted_encounter_armed
    }

    fn install_scripted_encounter(&mut self, window: &[u8]) {
        // Queue the record window for the field-step driver to install after
        // the VM borrow ends (we can't mutate the encounter session while the
        // field bytecode is still borrowed).
        self.world.pending_scripted_encounter = Some(window.to_vec());
    }

    fn op4c_n_e_sub2_fmv_trigger(&mut self, fmv_id: i16) {
        // Field-VM op `0x4C 0xE2` - retail handler at 0x801E30E4 writes
        // the resolved s16 to `_DAT_8007BA78` (FMV index) and pokes
        // `_DAT_8007B83C = 0x1A` (next game mode = 26 = StrInit). We
        // record the request here so the SceneHost / engine driver can
        // pop it after the field step returns and switch its scene
        // mode without invalidating the field-VM borrow.
        self.world.pending_fmv_trigger = Some(fmv_id);
        self.world
            .pending_field_events
            .push(FieldEvent::FmvTrigger { fmv_id });
    }

    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        // Sub-ops 1 (start field BGM) and 9 (queue) are the cases that
        // pin a "currently playing" id. Other sub-ops are control words
        // (pause / stop / volume / etc.) - we still surface the event so
        // the engine can route them, just without overwriting current_bgm.
        if sub_op == 1 || sub_op == 9 {
            self.world.current_bgm = Some(text_id);
        } else if sub_op == 4 {
            // 4 = stop.
            self.world.current_bgm = None;
        }
        self.world
            .pending_field_events
            .push(FieldEvent::Bgm { text_id, sub_op });
    }

    fn play_sfx(&mut self, sfx_id: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::PlaySfx { sfx_id });
    }

    fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        let inline_vec = inline.to_vec();
        self.world.current_dialog = Some(DialogRequest {
            text_id,
            inline: inline_vec.clone(),
            world_x,
            world_z,
            depth_id,
        });
        self.world
            .pending_field_events
            .push(FieldEvent::OpenDialog {
                text_id,
                inline: inline_vec,
                world_x,
                world_z,
                depth_id,
            });
    }

    /// Field-VM op 0x4C n5 sub-4 — dialog-advance poll.
    ///
    /// The retail dispatcher calls `FUN_801D65D8(0)` (dialog "advance one
    /// frame" query); a non-zero return halts the VM at `pc`, a zero
    /// return advances `pc += 2`. Our world tracks dialog activity via
    /// `current_dialog` (cleared by the engine after the user dismisses
    /// the box). When a dismiss button (Cross / Circle) was just-pressed
    /// this frame, drop the dialog request inline so the VM transitions
    /// without the host having to round-trip another event.
    ///
    /// Returns `true` while a dialog is showing and the user has *not*
    /// dismissed it this frame. Returns `false` when there's no active
    /// dialog or when the dismiss button just fired (clears the request
    /// and unblocks the VM in one step).
    fn op4c_n_5_sub_4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        if self.world.current_dialog.is_none() {
            return false;
        }
        let dismissed = (self.world.input.just_pressed(input::PadButton::Cross)
            || self.world.input.just_pressed(input::PadButton::Circle))
            && !self.world.dialog_input_consumed;
        if dismissed {
            self.world.dialog_input_consumed = true;
            self.world.current_dialog = None;
            self.world
                .pending_field_events
                .push(FieldEvent::DialogDismissed);
            // Accepting a scripted-encounter carrier's prompt engages it: this is
            // the dialogue-accept that advances the carrier SM (`FUN_801DA51C`)
            // to its scene-transition. The battle launches on the next
            // `tick_field_carriers`. (The tutorial fight is forced, so any
            // dismiss is the accept; the undecoded Yes/No box-selection logic
            // would gate this once pinned.)
            if let Some(idx) = self.world.pending_carrier_engage.take() {
                self.world.engage_field_carrier(idx);
            }
            return false;
        }
        true
    }

    /// Field-VM op `0x4C` outer-nibble-7 - rectangular collision-grid wall
    /// paint. Writes the high-nibble wall bits of the per-scene collision
    /// grid (`*(_DAT_1F8003EC) + 0x4000`), the same grid
    /// [`World::step_field_locomotion`] reads. The VM dispatcher has
    /// already turned the op operands into half-open tile ranges; we just
    /// apply the per-byte mutation. See [`World::paint_field_collision`].
    fn op4c_n7_tile_flag_bulk(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        self.world
            .paint_field_collision(sub, x_range, z_range, mask);
    }

    fn add_money(&mut self, delta: i32) {
        let new_total = (self.world.money as i64 + delta as i64).clamp(0, 9_999_999) as i32;
        self.world.money = new_total;
        self.world
            .pending_field_events
            .push(FieldEvent::AddMoney { delta });
    }

    fn set_item_count(&mut self, slot_byte: u8, count: u8) {
        if count == 0 {
            self.world.inventory.remove(&slot_byte);
        } else {
            self.world.inventory.insert(slot_byte, count);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SetItemCount { slot_byte, count });
    }

    fn party_add(&mut self, char_id: u8) -> bool {
        // The retail engine maintains a sorted insertion in
        // `_DAT_80084598..` (cap 4) and writes the leader slot when the
        // party transitions from empty. We mirror that with
        // `party_actor_slots` + `party_leader_slot`.
        let already_present = self
            .world
            .party_actor_slots
            .iter()
            .any(|s| matches!(s, Some(id) if *id == char_id));
        let accepted = if already_present {
            false
        } else if self.world.party_actor_slots.len() < 4 {
            self.world.party_actor_slots.push(Some(char_id));
            // First member also becomes the leader (matches retail's
            // `count == 0` arm).
            if self.world.party_leader_slot.is_none() {
                self.world.party_leader_slot = Some(char_id);
            }
            true
        } else {
            false
        };
        self.world
            .pending_field_events
            .push(FieldEvent::PartyAdd { char_id, accepted });
        accepted
    }

    fn party_remove(&mut self, char_id: u8) {
        self.world
            .party_actor_slots
            .retain(|s| !matches!(s, Some(id) if *id == char_id));
        if matches!(self.world.party_leader_slot, Some(id) if id == char_id) {
            // Promote next member or clear.
            self.world.party_leader_slot = self.world.party_actor_slots.first().copied().flatten();
        }
        self.world
            .pending_field_events
            .push(FieldEvent::PartyRemove { char_id });
    }

    fn field_interact(&mut self, interact_id: u8, slot: u8) {
        // The real field-dialogue path: open the interacted actor's own inline
        // interaction-script MES (retail `actor[+0x90]`, keyed by `slot`) and
        // arm/engage a scripted-encounter carrier on that slot (the dialogue-
        // accept auto-arm). Shared with the interaction probe via
        // [`World::trigger_field_interact`].
        self.world.trigger_field_interact(interact_id, slot);
    }

    fn render_cfg_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::RenderCfgLong { b1, b2, b3, b4 });
    }

    fn render_cfg_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::RenderCfgShort { r, g, b, packed });
    }

    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::SceneRegisterWrite {
                slot_10,
                slot_12,
                slot_14,
            });
    }

    fn counter_update(&mut self, op0: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::CounterUpdate { op0 });
    }

    fn setup_animation(&mut self, _ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        self.world
            .pending_field_events
            .push(FieldEvent::SetupAnimation {
                count,
                base_id,
                frames: frames.to_vec(),
            });
    }

    fn set_party_leader(&mut self, leader_id: u8) {
        self.world.party_leader_slot = Some(leader_id);
        self.world
            .pending_field_events
            .push(FieldEvent::SetPartyLeader { leader_id });
    }

    fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
        self.world.camera_state.params = params.to_vec();
        self.world.camera_state.apply_trigger = apply_trigger;
        self.world.camera_state.mode = mode;
        self.world
            .pending_field_events
            .push(FieldEvent::CameraConfigure {
                params: params.to_vec(),
                apply_trigger,
                mode,
            });
    }

    fn camera_load(&mut self, payload: &[u8]) {
        self.world.camera_state.loaded_payload = payload.to_vec();
        self.world
            .pending_field_events
            .push(FieldEvent::CameraLoad {
                payload: payload.to_vec(),
            });
    }

    fn camera_save(&mut self) {
        // Snapshot what we have currently - engines that model real camera
        // matrices can override this on a custom host wrapper. For now we
        // write a placeholder so save/load round-trip behaves.
        self.world.camera_state.saved = self.world.camera_state.loaded_payload.clone();
        self.world.pending_field_events.push(FieldEvent::CameraSave);
    }

    fn camera_apply(&mut self) {
        self.world
            .pending_field_events
            .push(FieldEvent::CameraApply);
    }

    fn scene_fade(&mut self, op0_word: u16, op1_word: u16) -> SceneFadeResult {
        self.world
            .pending_field_events
            .push(FieldEvent::SceneFade { op0_word, op1_word });
        SceneFadeResult::Done
    }

    fn effect_anim_trigger(&mut self, _ctx: &mut FieldCtx, arg: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::EffectAnimTrigger { arg });
    }

    fn menu_ctrl_sub1(&mut self, op0: u8, payload: &[u8; 5]) {
        self.world.pending_field_events.push(FieldEvent::MenuCtrl {
            op0,
            payload: *payload,
        });
    }

    fn menu_refresh(&mut self) {
        self.world
            .pending_field_events
            .push(FieldEvent::MenuRefresh);
    }

    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        // Player path: also propagate to the active actor slot's
        // move_state so the renderer / collision layer sees the teleport.
        if is_player
            && let Some(slot) = self.world.player_actor_slot
            && let Some(actor) = self.world.actors.get_mut(slot as usize)
        {
            actor.move_state.world_x = world_x as i16;
            actor.move_state.world_z = world_z as i16;
        }
        let _ = ctx;
        self.world.pending_field_events.push(FieldEvent::MoveTo {
            world_x,
            world_z,
            is_player,
        });
    }

    fn exec_move(&mut self, _ctx: &mut FieldCtx, move_id: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::ExecMove { move_id });
    }

    fn op4c_n8_sub_0_actor_allocator(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        // In the spawned opening-cutscene context (target 0xF8) this op is the
        // inline-narration text-draw, not an actor spawn - the separate
        // `CutsceneNarration` presenter owns those pages. Suppress the spawn
        // side-effect while the cutscene timeline steps; the VM still advances
        // the PC past the page bytes on its own.
        if self.world.in_cutscene_timeline {
            return;
        }
        // Walk `count` variable-length records out of `tail` using the
        // retail packet-length rule (FUN_8003CA38, mirrored in
        // `legaia_engine_vm::field_helpers::packet_length`): bytes <= 0x1E
        // terminate a record; bytes whose top nibble is 0xC consume one
        // extra byte. The walker stops when the tail is exhausted - the
        // retail original would over-read into adjacent memory, which the
        // clean-room port refuses by construction.
        let mut records: Vec<Vec<u8>> = Vec::with_capacity(count as usize);
        let mut cursor = 0usize;
        for _ in 0..count {
            if cursor >= tail.len() {
                break;
            }
            let len = vm::field_helpers::packet_length(&tail[cursor..]);
            records.push(tail[cursor..cursor + len].to_vec());
            // Skip the terminator byte itself (the byte <= 0x1E that
            // closed the record); if the walker ran off the end without
            // seeing one, `cursor + len == tail.len()` and the next
            // iteration's bounds check exits the loop.
            cursor += len + 1;
        }
        for record in &records {
            self.world.pending_actor_spawns.push(record.clone());
        }
        self.world
            .pending_field_events
            .push(FieldEvent::ActorAllocate { records });
    }

    fn op4c_n_d_sub8_call_d77f4(&mut self, b1: u8, words: [i16; 3]) {
        // Synchronous actor allocator (see retail `FUN_801D77F4` body
        // dumped at `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt`).
        // The dispatcher packs the four args
        //   `[vdf_idx: u8, tmd_idx: i16, kind: i16, variant: i16]`
        // into the 7 bytes after `[0x4C, 0xD8]`; FUN_801D77F4 then writes
        // `actor[+0x3C] = kind` and `actor[+0x3E] = variant` on the
        // allocated slot, plus `actor[+0x48] = DAT_8007C018[tmd_idx]`
        // (TMD pointer) and `actor[+0x4C] = VDF_body_ptr`. We mirror
        // all four writes here.
        let kind = words[1] as u16;
        let variant = words[2] as u16;
        let tmd_ref = self.world.global_tmd(words[0]).cloned();
        // Mirror retail's `actor[+0x4C] = VDF_body_ptr`: look up the
        // VDF record body bytes and store them on the allocated actor.
        // `None` when no VDF buffer is installed or the index is OOR;
        // engines that drive the host without setting one still get the
        // kind/variant writes (synchronous spawn semantics) plus an
        // empty `record` in the event payload.
        let record_bytes: Vec<u8> = self
            .world
            .vdf_record_bytes(b1)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let start = FIELD_SPAWN_START_SLOT as usize;
        match self
            .world
            .actors
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, a)| !a.active)
            .map(|(i, _)| i)
        {
            Some(slot_idx) => {
                let actor = &mut self.world.actors[slot_idx];
                actor.active = true;
                actor.kind = kind;
                actor.variant = variant;
                actor.tmd_ref = tmd_ref;
                actor.spawn_record = if record_bytes.is_empty() {
                    None
                } else {
                    Some(record_bytes.clone())
                };
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawned {
                        slot: slot_idx as u8,
                        kind,
                        variant,
                        record: record_bytes,
                    });
            }
            None => {
                // Pool-exhausted: mirrors the retail bail-silently branch
                // where FUN_80020DE0 returns 0.
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawnFailed {
                        record: record_bytes,
                    });
            }
        }
    }
}

// --- battle action host ----------------------------------------------------

pub(super) struct BattleHostImpl<'a> {
    pub(super) world: &'a mut World,
}

impl<'a> BattleActionHost for BattleHostImpl<'a> {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.world.actors.get(slot as usize).map(|a| &a.battle)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.world
            .actors
            .get_mut(slot as usize)
            .map(|a| &mut a.battle)
    }
    fn rng(&mut self) -> u32 {
        self.world.next_rng()
    }
    fn previous_action_cleared(&self, _: u8) -> bool {
        self.world.prev_action_cleared
    }
    fn sound_bank_ready(&self, _: u8) -> bool {
        self.world.sound_bank_ready
    }
    fn is_capture_spell(&self, id: u8) -> bool {
        self.world.capture_spells.contains(&id)
    }
    fn spell_mp_cost(&self, id: u8) -> u8 {
        self.world.spell_costs.get(&id).copied().unwrap_or(0)
    }
    fn character_ability_bits(&self, slot: u8) -> u32 {
        let i = slot as usize;
        self.world
            .character_ability_bits
            .get(i)
            .copied()
            .unwrap_or(0)
    }
    fn range_check(&self, attacker: u8, target: u8) -> u16 {
        self.world
            .range_table
            .get(&(attacker, target))
            .copied()
            .unwrap_or(0)
    }
    fn battle_end(&mut self, cause: BattleEndCause) {
        self.world.battle_end = Some(cause);
        self.world
            .pending_battle_events
            .push(BattleEvent::BattleEnd { cause });
    }
    fn party_count(&self) -> u8 {
        self.world.party_count
    }
    fn pose(&mut self, actor_id: u8, pose: Pose) {
        self.world
            .pending_battle_events
            .push(BattleEvent::Pose { actor_id, pose });
    }
    fn ui_element(&mut self, effect_id: u8, mode: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::UiElement { effect_id, mode });
        // mode == 0: spawn/reset. Route directly into the effect pool so
        // the VM's state machine drives the effect lifecycle while engines
        // also receive the event for visual dispatch.
        if mode == 0 {
            self.world.try_spawn_effect(effect_id, [0, 0, 0], 0);
        }
    }
    fn camera_bounds(&mut self) {
        self.world
            .pending_battle_events
            .push(BattleEvent::CameraBounds);
    }
    fn party_setup(&mut self, actor_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::PartySetup { actor_slot });
    }
    fn monster_setup(&mut self, actor_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::MonsterSetup { actor_slot });
        // Faithful `FUN_801E7320`: expand the targeting class the action picker
        // left in `actor.active_target` into a concrete target slot.
        self.world.resolve_monster_target(actor_slot);
    }
    fn recompute_battle_order(&mut self) {
        self.world
            .pending_battle_events
            .push(BattleEvent::RecomputeBattleOrder);
    }
    fn load_capture_archive(&mut self, idx: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::LoadCaptureArchive { idx });
    }
    fn spell_anim_trigger(&mut self, party_slot: u8, spell_id: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::SpellAnimTrigger {
                party_slot,
                spell_id,
            });
    }
    fn spell_anim_sustain(&mut self, actor_id: u8, anim_id: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::SpellAnimSustain { actor_id, anim_id });
    }
    fn apply_damage(&mut self, icon: u8, page: u8, target_slot: u8, party_slot: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::ApplyDamage {
                icon,
                page,
                target_slot,
                party_slot,
            });
    }
    fn apply_art_strike(&mut self, info: legaia_engine_vm::battle_action::ArtStrikeInfo) {
        // Resolve per-slot weapon attack and the defense the art targets.
        let attack = self
            .world
            .battle_attack
            .get(info.actor_slot as usize)
            .copied()
            .unwrap_or(0);
        let defense = self.world.resolve_battle_defense(info.target_slot, &info);
        let outcome = crate::art_strike::apply_art_strike(attack, defense, &info);
        self.world
            .pending_battle_events
            .push(BattleEvent::ApplyArtStrike {
                actor_slot: info.actor_slot,
                target_slot: info.target_slot,
                strike_index: info.strike_index,
                outcome,
            });
    }
    fn screen_shake(&mut self, magnitude: u16) {
        self.world
            .pending_battle_events
            .push(BattleEvent::ScreenShake { magnitude });
    }
    fn ramp_brightness(&mut self, target_pct: u8) {
        self.world
            .pending_battle_events
            .push(BattleEvent::RampBrightness { target_pct });
    }
}
