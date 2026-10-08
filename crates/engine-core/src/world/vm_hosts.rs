//! Per-VM `Host` trait implementations that bridge each port VM into
//! [`World`]. Split out of `world.rs`.

use super::*;

use crate::battle_events::BattleEvent;
use crate::field_events::FieldEvent;
use legaia_engine_vm as vm;
use vm::battle_action::{BattleActionHost, BattleActor, BattleEndCause, Pose};
use vm::effect_vm::EffectHost;
use vm::field::{CameraParam, FieldCtx, FieldHost, Op49State, SceneFadeResult};
use vm::move_vm::{ActorState as MoveActorState, MoveHost};
use vm::{Host as ActorVmHost, Position as ActorVmPosition};

mod battle_host;
mod field_host;

pub(super) use battle_host::*;
pub(super) use field_host::*;

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
    fn slide_to(&mut self, actor_id: u8, target: ActorVmPosition) {
        // Retail `FUN_800357fc`: copy the live position into the motion
        // source, write the target and raise the motion word; the per-frame
        // walker animates it. The world installs a motion-VM leg gliding the
        // actor's sprite position toward the target
        // ([`World::start_actor_motion`], stepped by `tick_actor_motions`),
        // and - when the actor id is also an installed field-NPC placement
        // slot - walks that NPC in the field frame (y → z).
        // PORT: FUN_800357fc
        self.world.start_actor_motion(actor_id, target);
        if self.world.npcs.positions.contains_key(&actor_id) {
            self.world
                .start_field_npc_motion(actor_id, target.x, target.y);
        }
    }
    fn snap_to(&mut self, actor_id: u8, p: ActorVmPosition) {
        // Retail `FUN_800358c0`: position, source and target written alike,
        // motion word cleared.
        // PORT: FUN_800358c0
        let a = &mut self.world.actors[actor_id as usize];
        a.move_state.world_x = p.x;
        a.move_state.world_y = p.y;
        a.field_20 = 0;
    }
    fn begin_close(&mut self, actor_id: u8) {
        // `FUN_80035978` starts a close animation this host has no frames
        // for, so the close completes at once.
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.active = false;
        }
    }
    fn close_all(&mut self) {
        // `FUN_80035A4C`: every window on the list begins its close. This
        // demo host's windows are the field actors, which it does not own
        // as a window list, so it closes none.
    }
    fn destroy(&mut self, actor_id: u8) {
        // `FUN_800319A8`: free and unlink at once.
        if let Some(a) = self.world.actors.get_mut(actor_id as usize) {
            a.active = false;
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
    /// When set, this host is ticking an **ambient field-fx part** whose
    /// bytecode is a window of the shared prescript stager bundle
    /// (`world.props.stager_bytes`) starting at this u16-word offset.
    /// Routes `move_bytecode_read_u16` to the shared bundle (retail's
    /// `_DAT_8007B8D0`-resident copy, which the self-modifying ext ops
    /// 0x04/0x1B/0x1E patch in place) and arms `spawn_child` collection.
    pub(super) field_record_words: Option<usize>,
    /// Child spawns collected from op `0x25` while ticking an ambient part
    /// (`FUN_80021B04(actor+0x14, ..., _DAT_8007B8D0 + offsets[v1], ...)`):
    /// the prescript record id + the spawning part's world position and
    /// its rotation banks `+0x24 / +0x26 / +0x28` (`actor+0x24`, the
    /// stager's second argument).
    pub(super) child_spawns: Vec<(i16, [i16; 3], [i16; 3])>,
}

impl<'a> MoveHost for MoveVmHostImpl<'a> {
    /// Op `0x17` - the battle-overlay escape `FUN_801F30C4(actor, mode)`,
    /// queued with what the burst reads off the parent (its `+0x14` position,
    /// `+0x24` rotation trio and `+0x72` scale) for
    /// [`World::flush_battle_bursts`]. Battle-only, as the overlay is.
    fn ext_17(&mut self, state: &mut vm::move_vm::ActorState, arg: i16) {
        if self.world.mode != SceneMode::Battle {
            return;
        }
        self.world
            .casting
            .pending_bursts
            .push(crate::world::PendingBurst {
                mode: arg as u16 as u32,
                pos: [state.world_x, state.world_y, state.world_z],
                rot: [state.render_24, state.render_26, state.render_28],
                scale: state.field_72,
            });
    }

    /// Op `0x1D` - `sh op[1], DAT_8007B6DE`: a store straight into SFX ring
    /// slot 3 with no cursor pair and no countdown (`0x80023680..0x8002368C`
    /// in `FUN_80023070`). The drainer plays it on its next pass. This is how
    /// the field's ambient effect scripts sound: `kor5`'s looping cue `0x204`
    /// every 63 vsyncs, the lightning director's thunder `0x20B` - a
    /// census of a `kor5` memory-card state sees the store at `0x80023688`
    /// (`ra 0x80023AE8`) two vsyncs before each drained key-on.
    fn global_write_1d(&mut self, value: u16) {
        self.world
            .audio
            .sfx_ring_ops
            .push(crate::world::SfxRingOp::WriteSlot(3, value as i16));
    }

    /// Ext `0x17` / `0x18` / `0x1A` / `0x19`: the object-effect table
    /// (`0x80083FF8`) a raised `+0x42` draws under.
    fn ext_world_struct_init(&mut self, index: i16, values: [i16; 5]) {
        self.world.object_effect.write(index, values);
    }
    fn ext_world_struct_write(&mut self, index: i16, values: [i16; 5]) {
        self.world.object_effect.write(index, values);
    }
    fn ext_world_struct_add(&mut self, index: i16, deltas: [i16; 5]) {
        self.world.object_effect.add(index, deltas);
    }

    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let idx = index as usize % self.world.sin_lut.len().max(1);
        let s = self.world.sin_lut.get(idx).copied().unwrap_or(0);
        let c = self.world.cos_lut.get(idx).copied().unwrap_or(0);
        (s, c)
    }
    fn keyframe_curve_multiplier(&self) -> u8 {
        // `DAT_1F80037D`, the game-speed rate byte op `0x0A` scales its
        // lane velocities by. SCUS plants `8` at boot (`addiu v0,zero,8` /
        // `sb v0,0x37d(at)`, `0x80055FB4` / `0x80055FBC`); only the Baka
        // Fighter and DEBUG MODE overlays write another value
        // (`docs/subsystems/move-vm.md`). Mid-Spirit captures confirm it on
        // the aura lane: authored `0x66`, gaining `0x66` a frame.
        crate::summon::RETAIL_CHANNEL_DELTA as u8
    }
    fn ext_rand16(&mut self) -> u16 {
        // Retail ext 0x05/0x30 call the BIOS `A(2Fh) rand` thunk
        // `FUN_80056798` (`jal 0x80056798` at 0x801D3714 / 0x801D45F8 in
        // `overlay_0897_801d362c.txt`), so the draw is the shaped world
        // stream, `0..=0x7FFF` - 0x30 tests its low bit, and the raw LCG
        // state's low bit strictly alternates.
        self.world.next_rand() as u16
    }

    // --- ext-VM globals -----------------------------------------------

    fn move_global_predicate_get(&self) -> u32 {
        self.world.move_vm.predicate
    }
    fn move_global_predicate_set(&mut self, value: u32) {
        self.world.move_vm.predicate = value;
    }
    fn move_global_counter_get(&self) -> u16 {
        self.world.move_vm.counter
    }
    fn move_global_counter_set(&mut self, value: u16) {
        self.world.move_vm.counter = value;
    }

    // --- ext-VM 16-slot scratch table ---------------------------------

    fn move_slot_load_u32(&self, slot: u16, dword_off: u8) -> u32 {
        let i = (slot & 0x0F) as usize;
        let off = (dword_off & 0x4) as usize; // 0 or 4
        let bytes = &self.world.move_vm.slot_table[i][off..off + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
    fn move_slot_save_u32(&mut self, slot: u16, dword_off: u8, value: u32) {
        let i = (slot & 0x0F) as usize;
        let off = (dword_off & 0x4) as usize;
        self.world.move_vm.slot_table[i][off..off + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn move_slot_load_u16(&self, slot: u16, byte_off: u8) -> u16 {
        let i = (slot & 0x0F) as usize;
        let off = (byte_off & 0x6) as usize; // even, 0..6
        let bytes = &self.world.move_vm.slot_table[i][off..off + 2];
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
    fn move_slot_save_u16(&mut self, slot: u16, byte_off: u8, value: u16) {
        let i = (slot & 0x0F) as usize;
        let off = (byte_off & 0x6) as usize;
        self.world.move_vm.slot_table[i][off..off + 2].copy_from_slice(&value.to_le_bytes());
    }

    // --- bytecode self-modify (0x04 / 0x1B / 0x1E) --------------------

    fn move_bytecode_read_u16(&self, word_off: usize) -> u16 {
        if let Some(&v) = self.deferred_writes.get(&word_off) {
            return v;
        }
        // Ambient field-fx parts read the shared prescript bundle in place
        // (retail `_DAT_8007B8D0`): the word offset is PC-space relative to
        // the part's record base.
        if let Some(base) = self.field_record_words {
            let byte = (base + word_off) * 2;
            return self
                .world
                .props
                .stager_bytes
                .get(byte..byte + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .unwrap_or(0);
        }
        let Some(slot) = self.current_slot else {
            return 0;
        };
        self.world
            .move_vm
            .bytecode
            .get(slot)
            .and_then(|bc| bc.get(word_off))
            .copied()
            .unwrap_or(0)
    }
    fn move_bytecode_write_u16(&mut self, word_off: usize, value: u16) {
        self.deferred_writes.insert(word_off, value);
    }

    // --- op 0x25 child spawn ------------------------------------------

    fn spawn_child(&mut self, state: &mut MoveActorState, slot: i16) {
        // Only the ambient field-fx path spawns children in the engine (the
        // summon stand-in keeps its scenes single-record). Retail seats the
        // child at the parent's world position and rotation banks
        // (`FUN_80021B04(actor+0x14, actor+0x24, _DAT_8007B8D0 + offsets[v1],
        // 0x1000)`).
        if self.field_record_words.is_some() {
            self.child_spawns.push((
                slot,
                [state.world_x, state.world_y, state.world_z],
                [state.render_24, state.render_26, state.render_28],
            ));
        }
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
        self.world.terrain.map_origin_xz
    }
    fn move_axis_threshold(&self) -> i16 {
        self.world
            .move_vm
            .pool_top_override
            .unwrap_or_else(|| self.world.actor_pool_top())
    }
    fn move_dat_1f800393(&self) -> u8 {
        self.world.move_vm.ramp_ratio
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
        self.world.move_vm.scratchpad_targets[i] = value;
    }
    fn ext_scratchpad_ramp(&mut self, slot_index: i16, target: i16, _ticks: i16) {
        // Default world has no per-frame ramp scheduler; record the target
        // immediately so reads see the final state. Engines override to
        // model the per-frame interpolation.
        let i = (slot_index as u16 & 0x0F) as usize;
        self.world.move_vm.scratchpad_targets[i] = target;
    }

    // --- ext sub-op 0x2C scanline strip emitter -----------------------

    fn ext_func801d31b0(&mut self, state: &mut MoveActorState, _operand: &[u16]) {
        // `FUN_801D31B0` draws on the spot; the port captures what it reads
        // and draws it on the host's render pass (`move_strip_prims`).
        self.world
            .move_vm
            .push_strip_request(vm::move_ext_strip::StripRequest::from_actor(state));
    }

    // --- ext sub-op 0x2F global slot ---------------------------------

    fn ext_set_8007b9d8(&mut self, value: i32) {
        self.world.move_vm.dat_8007b9d8 = value;
        // The word is the frame-step **floor** (`FUN_80016B6C` reads it at
        // `0x80017178` and stores its low byte into `DAT_1F800393` when the
        // measured cadence is below it, `lbu -0x4628` at `0x80017190`). A
        // stager that raises it - opdeene's prescript record 16 writes `3` as
        // its first op - slows the game tick from the next frame on. The
        // engine runs the deterministic arm of that resolver (adaptive cadence
        // off), whose result is the floor itself, so the cadence follows.
        let floor = (value as u8).max(1);
        self.world.clock.frame_step_floor = floor;
        self.world.clock.frame_step = floor;
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
        let actor_slot = *self.world.party.party_actor_slots.get(slot as usize)?;
        let actor_slot = actor_slot? as usize;
        let st = &self.world.actors[actor_slot].move_state;
        Some([st.world_x, st.world_y, st.world_z])
    }

    // --- ext sub-op 0x3C fade colour -----------------------------------

    fn ext_fade_color(&mut self, rgb: [u8; 3], ticks: u16) {
        self.world.presentation.pending_fade = Some(FadeRequest { rgb, ticks });
    }

    // `ext_dispatch` uses the default trait impl, which routes through
    // `self` - so sub-op handlers see the world-backed callbacks above.
}

// --- effect VM host --------------------------------------------------------

pub(super) struct EffectHostImpl<'a> {
    pub(super) world: &'a mut World,
}

impl<'a> EffectHost for EffectHostImpl<'a> {
    // The faithful walker (`Pool::tick_retail`) derives the whole effect
    // lifecycle from the catalog's spawn records + animation frames; the
    // host only supplies the RNG (mirror bits + spawn-offset rewrites).
    fn next_random(&mut self) -> i32 {
        // The walker ports battle-overlay `FUN_801DFDF0` / `FUN_801E0088`,
        // whose draws are `jal 0x80056798` (`0x801DFF64` / `0x801DFFCC`,
        // `0x801E01CC`): a shaped, never-negative `rand()`.
        self.world.next_rand() as i32
    }

    /// `FUN_801DFDF0`'s two special ids (`0x801DFE38..0x801DFE58`): `4` and
    /// `0x13` first seat a move-VM trigger actor, then spawn the effect as
    /// every other id does. The overlay is battle-resident, so the side call
    /// only exists in battle.
    fn is_summon_effect(&self, effect_id: u8) -> bool {
        self.world.mode == SceneMode::Battle
            && vm::battle_burst::trigger_for_effect(effect_id).is_some()
    }

    /// `FUN_80050ED4(world_pos, &{0, angle, 0}, trigger, 0x1000)`, queued for
    /// [`World::flush_battle_bursts`].
    fn handle_summon(&mut self, effect_id: u8, world_pos: [i16; 3], angle: u16) {
        if let Some(trigger) = vm::battle_burst::trigger_for_effect(effect_id) {
            self.world
                .casting
                .pending_burst_triggers
                .push((trigger, world_pos, angle));
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
        // The port's world has no closed-portal state yet, so the body
        // always runs when world-map entities are installed; the per-state
        // gates (encounter-enabled, dialog-active) still apply below.
        true
    }
    fn encounter_countdown(&self) -> i8 {
        self.world.world_map.encounter.countdown
    }
    fn set_encounter_countdown(&mut self, v: i8) {
        self.world.world_map.encounter.countdown = v;
    }
    fn encounter_enabled(&self) -> bool {
        self.world.world_map.encounter.enabled
    }
    fn on_encounter(&mut self, entity_idx: usize, _resolver_result: u32) {
        // Latch a formation for resolution into a battle at the end of the
        // world-map tick. Prefer this entity's own encounter-zone formation;
        // fall back to the map-wide shared formation. Pace the next encounter
        // by resetting the shared countdown.
        let formation_id = match self.world.world_map.entity_configs.get(entity_idx) {
            Some(WorldMapEntityConfig::EncounterZone { formation_id }) => *formation_id,
            _ => self.world.world_map.encounter.formation_id,
        };
        self.world.world_map.pending_encounter = Some(formation_id);
        self.world.world_map.encounter.countdown = self.world.world_map.encounter.reset_to;
    }
    fn on_activating(&mut self, _entity_idx: usize) {
        // Pending scene/portal data copy - no engine-side scene buffer yet.
    }
    fn on_scene_transition(&mut self, entity_idx: usize) {
        // A portal entity reached the transition state. Which of the two
        // portal shapes it is decides where the number goes - and they are
        // *different id spaces*, which is the whole reason this arm is split.
        match self.world.world_map.entity_configs.get(entity_idx) {
            // A **minigame door** on the overworld (the `map02` / `map03`
            // fishing signboards): its payload is the op-`0x3E` `op0 - 100`
            // mode-24 sub-id, so it arms the door warp exactly as the field
            // VM's own arm and the walk-touch arm do. Routing it as a map id
            // instead resolved a code-overlay selector through a CDNAME
            // ordinal and warped the player to an unrelated scene.
            // REF: FUN_801DE840 case 0x3e at 0x801E078C
            Some(WorldMapEntityConfig::MinigameDoor { sub_id }) => {
                let sub_id = *sub_id;
                self.world.arm_minigame_warp();
                self.world.minigames.pending_warp = Some(sub_id);
            }
            // An overworld town/dungeon entrance (the `0x3F`-bridge portal) -
            // the only producer of `WorldMapTransition`. The event carries the
            // `0x3F` destination index and the `slot`; the host's transition
            // drain reads the real CDNAME destination from
            // `world_map_entity_configs[slot]`.
            Some(WorldMapEntityConfig::OverworldPortal { index, .. }) => {
                let index = *index;
                self.world
                    .pending_field_events
                    .push(FieldEvent::WorldMapTransition {
                        dest_index: index as u16,
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
        self.world.dialogue_owns_input()
    }
    fn player_walking(&self) -> bool {
        self.world.world_map.player_walking
    }
    fn on_interact(&mut self, entity_idx: usize) {
        let interact_id = match self.world.world_map.entity_configs.get(entity_idx) {
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
        // formation into the global cell here; the port's world latches it
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
            self.world.carriers.configs.get(entity_idx).cloned()
        {
            self.world.carriers.pending_battle = Some(formation_id);
        }
    }
    fn dialog_active(&self) -> bool {
        self.world.dialogue_owns_input()
    }
    fn player_walking(&self) -> bool {
        // Report "player walking" so the SM's proximity-interact path stays
        // suppressed: the port's world has no player-near-NPC model yet, so
        // a field carrier is engaged explicitly via `engage_field_carrier`
        // rather than by the SM's auto-interact gate (which would otherwise
        // re-fire `on_interact` every frame once its cooldown bit latched).
        true
    }
    fn on_interact(&mut self, entity_idx: usize) {
        // Reached only once a future proximity model opens the gate; surfaces
        // the carrier's interaction id for the host.
        let interact_id = match self.world.carriers.configs.get(entity_idx) {
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
