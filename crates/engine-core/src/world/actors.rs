//! Move-VM/actor-physics ticking, battle animation staging/commit/reactions, poses, party roster, battle/world-map entry, cutscene finish, and sprite requests.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;
mod battle_anim;
mod battle_fx;
mod mode_entry;

/// The committed anim id (`+0x1D9`) the effect stepper's code substitution
/// keys on - the dynamic art slot the anim commit remaps an art clip onto.
const ACTION_FX_ART_SLOT: u8 = 0x11;
/// The table-form code an art's code-`0` record becomes (`li s1,0x9` at
/// `0x801DF094`).
const ACTION_FX_ART_CODE: u8 = 9;

impl World {
    /// The actor allocator's free-stack top `_DAT_8007C348` as the move VM's
    /// ext sub-ops `0x36` / `0x37` read it: `0x8E` less the live actors.
    ///
    /// Retail allocates every field actor - placements, script actors, effect
    /// parts - from one 143-entry pool, and the library states hold its top
    /// between 14 and 135 (7 to 128 live). The port has no single pool, so the
    /// count is the populations that stand for one: the active world actors,
    /// the ambient effect parts and the script arcs / attached lights. What the
    /// predicates need is the scale, not the exact figure - every shipped
    /// `0x37` is a headroom guard at `0x80..=0x87` live, every `0x36` a
    /// spin-wait until the count drops below `0x76..=0x78` - and before this
    /// the port answered "pool full" for every scene, so all of them halted.
    pub fn actor_pool_top(&self) -> i16 {
        let live = self.actors.iter().filter(|a| a.active).count()
            + self.ambient.fx.len()
            + self.script_actors.arcs.len()
            + self.script_actors.lights.len();
        (0x8E - live.min(0x8E) as i32) as i16
    }

    /// Per-actor move-VM tick - clean port of `FUN_80021DF4` (lines
    /// `80022B94..80022BBC`).
    ///
    /// Two-phase: (1) pre-tick decrement the per-actor `wait_timer` by the
    /// global frame-time `delta`, (2) run the move VM through
    /// [`vm::move_vm::actor_tick`], which gates on the resulting timer and
    /// inspects the HALT flag after the call. Outcomes are recorded in
    /// [`crate::world::MoveVmGlobals::outcomes`] so engines that want to react to per-actor
    /// halts / waits can read them after the world ticks.
    ///
    /// `delta` mirrors the retail product `_DAT_1f800393 * _DAT_1f80037D`
    /// (per-frame anim-speed scalars). Engines pass their own per-frame
    /// scalar; the default world tick uses `1` so a Wait of N consumes N
    /// frames.
    pub fn tick_move_vms_with_delta(&mut self, delta: u16) {
        self.move_vm.outcomes.clear();
        for slot in 0..self.actors.len() {
            if !self.actors[slot].active {
                continue;
            }
            let bc = self.move_vm.bytecode.get(slot).cloned().unwrap_or_default();
            if bc.is_empty() {
                continue;
            }
            // Pre-tick: decrement wait timer (retail does this unconditionally
            // before the gate).
            vm::move_vm::decrement_wait_timer(&mut self.actors[slot].move_state, delta);
            let outcome = self.actor_tick_at(slot, &bc, MOVE_VM_BUDGET);
            self.move_vm.outcomes.push((slot as u8, outcome));
        }
    }

    /// Backwards-compatible wrapper using `delta = 1`.
    pub fn tick_move_vms(&mut self) {
        self.tick_move_vms_with_delta(1);
    }

    /// Per-actor physics tick - port driver for
    /// `engine-vm::actor_tick::tick_actor` (FUN_80021DF4). Runs
    /// [`vm::actor_tick::tick_actor`] once per active slot, then dispatches
    /// the emitted [`TickEvent`]s.
    ///
    /// This loop is the engine's form of the retail actor-list iterator
    /// `FUN_8002519C` - the walker `FUN_80016444` runs over each of the
    /// five `_DAT_8007C34C..0x36C` list heads. Per node it first snapshots
    /// the previous position (`+0x14` -> `+0x1C`, `+0x18` -> `+0x20`,
    /// `0x800251D4..0x800251F0`); a live node (`+0x10 & 8` clear) then gets
    /// `jalr node[+0x0C]` (`0x800252B4`) - for a standard actor that is
    /// `FUN_80021DF4`, reached through the same pointer as every other
    /// handler. A retired node (`& 8`) instead gets a one-time teardown
    /// guarded by bit `0x02000000` (not `0x200`): `FUN_80024DFC`,
    /// `FUN_800204A4`, and - when its handler is `FUN_80021DF4` - the frees of
    /// `+0xA8` / `+0x4C` and `FUN_800250D4` (`0x800251F4..0x80025294`). An
    /// earlier version of this doc described an inline physics tick with a
    /// `0x200` "ticked this frame" dedupe, which is not these bytes. The
    /// engine keeps one pool with an `active` flag instead of five lists, and
    /// the special-fn nodes are the dedicated ticks `World::tick` sequences
    /// around this loop. Neither the previous-position snapshot nor the
    /// retire teardown is modelled here. Node layout
    /// and observed tick fns: `docs/subsystems/world-map.md`
    /// ("per-frame render-pass iterator").
    ///
    /// At the moment the only event the engine reacts to is
    /// [`TickEvent::MoveVmKick`], which drives
    /// [`vm::move_buffer::cursor_advance`] against the actor's
    /// [`MoveBufferState`]. The cursor's record source is the per-scene
    /// MOVE pool installed via [`World::set_move_buffer_root`] (mirrors
    /// retail `_DAT_8007B888` / `_DAT_8007B840` / `_DAT_8007B75C`).
    ///
    /// The other event variants (audio cues, render submissions,
    /// unlink requests, keyframe pose writeback) are recorded in
    /// [`crate::world::MoveVmGlobals::last_tick_events`] for engines that want to consume
    /// them but otherwise no-op. Wiring those is orthogonal to the
    /// move-buffer cursor.
    ///
    /// `frame_delta` matches the retail `DAT_1F800393` ramp scalar
    /// (idle = `1`). The default tick uses `1`.
    // PORT: FUN_8002519c (list-walk tick dispatch; pool-not-lists divergence
    //                     documented above)
    pub fn tick_actor_physics_with(&mut self, scalars: TickScalars, listener: &ListenerState) {
        self.move_vm.last_tick_events.clear();
        let host = move_buffer_host::WorldMoveBufferView {
            move_buf: &self.move_vm.buffer_root,
            move2_buf: &self.move_vm.buffer2_root,
            alt_buf: &self.move_vm.buffer_alt_root,
        };
        for (idx, actor) in self.actors.iter_mut().enumerate() {
            if !actor.active {
                continue;
            }
            let res = vm::actor_tick::tick_actor(&mut actor.physics, scalars, listener);
            if !res.events.is_empty() {
                // Drive the move-buffer cursor on any MoveVmKick event.
                let kicked = res
                    .events
                    .iter()
                    .any(|e| matches!(e, TickEvent::MoveVmKick));
                if kicked {
                    cursor_advance(&mut actor.move_buffer, &host, scalars.frame_delta);
                }
                self.move_vm.last_tick_events.push((idx as u8, res));
            }
        }
    }

    /// Default-listener wrapper (no positional SFX integration yet) carrying
    /// the **live cadence** into the dispatcher scalars.
    ///
    /// `frame_delta` is retail's `DAT_1F800393` - the vsyncs one game tick
    /// spans - not a constant `1`. [`World::tick`] fires this once every
    /// [`crate::world::FrameClock::frame_step`] vsyncs, so the two together conserve
    /// vsyncs-per-second: the dispatcher integrates the same total delta over
    /// the same wall-clock span, just in fewer, larger steps. That is exactly
    /// retail's own trade, and it is why duration-based parity (the camera
    /// mover, every `t = min(t + dt, d)` accumulator) is untouched by it.
    ///
    /// REF: FUN_80016B6C
    pub fn tick_actor_physics(&mut self) {
        let listener = ListenerState::unicast(0, 0, 0);
        let cadence = vm::actor_tick::FrameCadence::from_raw(self.clock.frame_step);
        self.tick_actor_physics_with(TickScalars::for_cadence(cadence, 1), &listener);
    }

    /// Install the MOVE buffer pool root (retail `_DAT_8007B888`). The
    /// bytes are the MDT-shaped offset-table blob the scene-load path
    /// extracts from the slot-1 `Asset(0x05) = Move` descriptor. Pass
    /// an empty slice to clear it - the cursor's resolver will then
    /// return `None` for every requested id.
    pub fn set_move_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move_vm.buffer_root = bytes;
    }

    /// Install the MOVE2 buffer pool root (retail `_DAT_8007B840`).
    /// Selected when an actor's `cursor_requested` is `>= 0x400`.
    pub fn set_move2_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move_vm.buffer2_root = bytes;
    }

    /// Install the alternate MOVE buffer pool root (retail
    /// `_DAT_8007B75C`). Selected when the actor's status flag word
    /// has [`vm::move_buffer::STATUS_FLAG_ALT_POOL`] set.
    pub fn set_move_buffer_alt_root(&mut self, bytes: Vec<u8>) {
        self.move_vm.buffer_alt_root = bytes;
    }

    /// Advance all active actor animations one frame. Mirrors the
    /// keyframe-table block in `FUN_80021DF4` (`0x80022ec4..0x80023040`)
    /// that walks `actor[+0x4C]` (anim pointer) when `actor[+0x22]`
    /// (factor) is non-zero. Called by [`World::tick`] after the move-VM
    /// pass.
    pub fn tick_actors(&mut self) {
        for actor in &mut self.actors {
            if !actor.active {
                continue;
            }
            if let Some(player) = &mut actor.active_animation {
                actor.pose_frame = Some(player.tick());
            }
        }
    }

    /// Bind an animation player to actor `slot`. Replaces any existing
    /// player and resets the playhead. No-ops for out-of-range slots.
    pub fn set_actor_animation(&mut self, slot: usize, player: AnimPlayer) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.active_animation = Some(player);
            actor.pose_frame = None;
        }
    }

    /// Whether a host's per-actor mesh pass draws actor `slot`: it must carry
    /// a TMD binding **and** be active (spawned). [`Self::init_scene_animations`]
    /// pre-binds every slot `K` to scene TMD `K` so a later spawn finds its
    /// mesh, which leaves the never-spawned slots bound, inactive and parked
    /// at the origin. Retail has no such actors - `FUN_8001E890` registers the
    /// scene TMDs in the pointer table without allocating any, and the render
    /// dispatcher walks only the allocated list - so drawing them paints every
    /// scene-pack mesh at world `(0, 0, 0)`.
    ///
    /// `synthetic_battle_camera` is the one exception the native window keeps:
    /// a battle with no stage dome frames every bound body round the origin.
    // REF: FUN_8001E890 (scene TMD registration - no actor allocation)
    // REF: FUN_8001D140 (the render dispatcher's allocated-list walk)
    pub fn actor_slot_drawn(&self, slot: usize, synthetic_battle_camera: bool) -> bool {
        self.actors
            .get(slot)
            .is_some_and(|a| a.tmd_binding.is_some() && (a.active || synthetic_battle_camera))
    }

    /// Bind actor `slot` to TMD index `tmd_idx` in `SceneResources::tmds`.
    /// Renderers use this binding to look up the right mesh when applying
    /// the actor's `pose_frame`. No-ops for out-of-range slots.
    pub fn set_actor_tmd_binding(&mut self, slot: usize, tmd_idx: usize) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.tmd_binding = Some(tmd_idx);
        }
    }

    /// Install the field player's idle/walk clip pair (built by the host from
    /// the PROT 0874 §1 locomotion bundle -
    /// [`crate::field_anim::FieldPlayerAnim`]). The field tick advances it
    /// after the locomotion step; `None` (the default) leaves the player on
    /// the static rest pose.
    pub fn set_field_player_anim(&mut self, anim: Option<crate::field_anim::FieldPlayerAnim>) {
        self.locomotion.player_anim = anim;
    }

    /// Frame count to size a **cross-context** clip cursor with when the scene
    /// ANM bundle cannot name the poked clip
    /// ([`crate::field_env::PropAnimBank::bind_actor_clip`]): the live
    /// locomotion clip's length if a host has installed a player clip player,
    /// else [`crate::field_env::PLAYER_CLIP_STANDIN_FRAMES`]. It sets how long
    /// the script's end-latch spin waits, so it only has to be finite and of
    /// the right order - the frames the player actually *sees* are the host
    /// clip player's, which is a different object.
    pub(crate) fn player_clip_frames_hint(&self) -> u16 {
        self.locomotion
            .player_anim
            .as_ref()
            .map(|a| a.active_frame_count() as u16)
            .filter(|n| *n > 0)
            .unwrap_or(crate::field_env::PLAYER_CLIP_STANDIN_FRAMES)
    }

    /// The size the player's figure draws at, as a factor of its mesh:
    /// the player's `+0x72` over `0x1000`.
    ///
    /// The animated renderer `FUN_8001B964` scales the actor matrix by
    /// `+0x72` whenever it is not `0x1000` (`0x8001BA6C..0x8001BAA4`, the
    /// three stores into the scale vector `0x1F800348` and `ScaleMatrix`), so
    /// the word the pad step reads as its speed multiplier is also the
    /// figure's size. Each kingdom's entry script sets it to `0xC00`, which is
    /// why the overworld player stands at three quarters of its town height.
    /// Both play hosts fold this into the player's draw.
    ///
    /// A `0` word is retail's "do not draw": `FUN_8001B964` tests it first
    /// (`lhu v0,0x72(s0)` / `beq v0,zero,0x8001BE20` at `0x8001B998..0x8001B9A0`)
    /// and returns without emitting a packet. Cutscenes hide the player that
    /// way while a stand-in walks (`CC F8 40 00 00 00 00`, see
    /// [`Self::player_hidden`]), so the scale is `0.0` and both hosts skip the
    /// draw. `1.0` with no player actor.
    ///
    /// REF: FUN_8001B964
    pub fn player_render_scale(&self) -> f32 {
        f32::from(self.player_scale_word()) / 4096.0
    }

    /// `true` while the player's `+0x72` is `0` - retail's animated renderer
    /// skips the actor outright (`FUN_8001B964` at `0x8001B9A0`). Both play
    /// hosts drop the player's draw (textured and colour halves) on it.
    ///
    /// REF: FUN_8001B964
    pub fn player_hidden(&self) -> bool {
        self.player_scale_word() == 0
    }

    /// Un-hide the player a script context left hidden because the **port**
    /// ended it before it reached its own restore.
    ///
    /// Cutscenes and talks hide the player with `CC F8 40 00 00 ..` and show
    /// it again with `CC F8 40 00 10 ..` (op `4C` nibble-4 sub-0 aimed at the
    /// player anchor `0xF8`; see [`Self::player_render_scale`]). Retail's
    /// runner (`FUN_80039B7C`) only lets go of a context on the context's own
    /// terminal bytes, so a record that hides the player always reaches the
    /// restore it carries. The port's runners also end a context on their
    /// anti-hang nets - a frame cap, a park timeout, a loop with no box, an
    /// op they cannot advance - and a context cut that way left `+0x72` at
    /// `0`: the player undrawn **and** unable to move, because the same word
    /// is the pad step's speed multiplier, until the next scene entry
    /// re-seated it.
    ///
    /// The rescue is keyed on the record's own bytes, so it never invents a
    /// restore: it fires only while the player is hidden, no ramp is in
    /// flight, and the record holds a non-zero `CC F8 40` write at or after
    /// the PC the run stopped on (`end_pc`) that the run never executed
    /// (`visited`). The player takes that op's target value - the value the
    /// script would have written. A record with no pending restore leaves the
    /// player hidden, as retail would. Returns whether it restored.
    ///
    /// REF: FUN_80039B7C, FUN_8001B964
    pub(crate) fn restore_owed_player_scale(
        &mut self,
        bytecode: &[u8],
        end_pc: usize,
        visited: &[bool],
    ) -> bool {
        if !self.player_hidden() || self.locomotion.player_scale_ramps.active() != 0 {
            return false;
        }
        let owed = (end_pc..bytecode.len().saturating_sub(4)).find_map(|p| {
            if visited.get(p).copied().unwrap_or(false)
                || !crate::world::vm_hosts::is_player_scale_op(bytecode, p)
            {
                return None;
            }
            let v = u16::from_le_bytes([bytecode[p + 3], bytecode[p + 4]]);
            (v != 0).then_some(v)
        });
        let Some(value) = owed else {
            return false;
        };
        let Some(a) = self
            .player_actor_slot
            .and_then(|s| self.actors.get_mut(s as usize))
        else {
            return false;
        };
        log::info!(
            "script context ended before its player restore; +0x72 0 -> {value:#06x} \
             (end pc {end_pc:#06x})"
        );
        a.move_state.field_72 = value;
        true
    }

    /// The player's live `+0x72`, `0x1000` with no player actor.
    fn player_scale_word(&self) -> u16 {
        self.player_actor_slot
            .and_then(|s| self.actors.get(s as usize))
            .map_or(0x1000, |a| a.move_state.field_72)
    }

    /// One frame of the player's `+0x72` ramps - the slots
    /// [`crate::world::FieldLocomotion::player_scale_ramps`] holds, lerped by
    /// retail's ramp ticker (`end + (start - end) * remaining / total`,
    /// `remaining` down by the frame step) and stored as a halfword (kind 2).
    /// A cutscene's `CC F8 40 00 10 64 00` grows the player back from hidden
    /// over 100 frames this way.
    ///
    /// REF: FUN_80036D80
    /// One frame of the `4C 48` heading tweens: each lands the scheduler's
    /// retail-space value in the placement's heading (engine space,
    /// `+ 0x800`), raw, as the scheduler's `sh` does.
    pub(crate) fn tick_npc_heading_ramps(&mut self) {
        if self.npcs.heading_ramps.active() == 0 {
            return;
        }
        let speed = self.move_vm.ramp_ratio.max(1);
        for w in self.npcs.heading_ramps.tick(speed) {
            if let Ok(slot) = u8::try_from(w.owner) {
                self.npcs
                    .headings
                    .insert(slot, (w.value as i16).wrapping_add(0x800));
            }
        }
    }

    pub(crate) fn tick_player_scale_ramp(&mut self) {
        if self.locomotion.player_scale_ramps.active() == 0 {
            return;
        }
        let speed = self.move_vm.ramp_ratio.max(1);
        let writes = self.locomotion.player_scale_ramps.tick(speed);
        let Some(slot) = self.player_actor_slot.map(usize::from) else {
            return;
        };
        if let Some(a) = self.actors.get_mut(slot) {
            for w in writes {
                a.move_state.field_72 = w.value as u16;
            }
        }
    }

    /// Is the player running this frame?
    ///
    /// Retail (`FUN_801d01b0` at `0x801D0358..0x801D03A0`) computes it as the
    /// **exclusive or** of two things:
    ///
    /// - the run button - held pad `_DAT_8007B850` AND the mask config word
    ///   `[0x800846DC]`, which is `0x48` = Cross | R1;
    /// - the Field Move option word `[0x800846CC]` (= `0x80084140 + 0x58c`,
    ///   the pause menu's Walk / Run row).
    ///
    /// The XOR is what the paired branches encode: from the button-held side
    /// (`bnez` at `0x801D0370` → `0x801D0390`) a set option jumps PAST the
    /// `$s4 = 0xc` store, and from the button-clear side it falls INTO it. So
    /// the option picks the default and the button inverts it - hold to run
    /// when Walk is selected, hold to walk when Run is.
    pub fn field_run_active(&self) -> bool {
        self.locomotion.run_button_held != self.locomotion.run_default
    }

    /// The frame's base step - retail's `$s4` before the `+0x72` multiply at
    /// `0x801D056C`.
    ///
    /// Ported from the selector at `0x801D0334..0x801D03E0`, in the order
    /// retail tests it: forced-slow wins outright (its arm `j`s past
    /// everything else), otherwise run vs walk. The debug-turbo arm
    /// ([`crate::world::config::FIELD_BASE_STEP_DEBUG_TURBO`]) is recorded but
    /// never taken - see its doc comment for the three gates.
    ///
    /// The forced-slow arm's byte `_DAT_8007B6A8` is the per-scene MAN flag
    /// [`crate::world::PartyState::scene_save_allowed`] (`FUN_8003AEB0` copies
    /// `MAN[1] & 1` into it), set on exactly the three kingdom world maps - so
    /// the overworld player always takes the slow step and never runs, which
    /// is what the retail overworld states measure (`10` units per `dt = 3`
    /// frame: `(5 * 0xC00) >> 12 = 3`, times 3, rounded up by the 2-unit
    /// stepper).
    ///
    /// PORT: FUN_801d01b0 (base-step selector)
    pub fn field_base_step(&self) -> i32 {
        if self.locomotion.forced_slow || self.party.scene_save_allowed {
            return crate::world::config::FIELD_BASE_STEP_FORCED_SLOW;
        }
        if self.field_run_active() {
            return crate::world::config::FIELD_BASE_STEP_RUN;
        }
        crate::world::config::FIELD_BASE_STEP
    }

    /// Recompute [`crate::world::FieldLocomotion::actor_moving`] by diffing every tracked
    /// actor's live field position against last frame's, and fold the
    /// player's own bit into its locomotion animation.
    ///
    /// This is the source-agnostic half of the locomotion animation. The pad
    /// and nav-walk paths raise
    /// [`crate::field_anim::FieldPlayerAnim::moved_this_frame`] directly
    /// (they know they moved before they commit, and a *wall-blocked* pad
    /// step still walks in place the way retail does, which a position diff
    /// cannot see). Everything else that moves an actor - a motion-VM patrol
    /// leg, a cutscene `MoveTo`, a channel-driven walk-on - commits a
    /// position and nothing more, so without this pass those actors slide
    /// along in their idle pose.
    ///
    /// Called once per field tick, immediately before
    /// [`Self::tick_field_player_anim`], so it sees every position any earlier
    /// step in the frame committed.
    ///
    /// A slot appearing for the first time (an actor seated mid-scene by a
    /// timeline, or the frame after a scene load) seeds the snapshot and is
    /// NOT reported as moving - its arrival is a placement, not a step.
    pub(crate) fn detect_field_actor_motion(&mut self) {
        self.locomotion.actor_moving.clear();
        let player_slot = self.player_actor_slot;
        // The player reads from its move_state (the locomotion commits
        // there); NPCs read from the live placement-position map.
        let player_pos = player_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        let mut seen: std::collections::HashSet<u8> =
            std::collections::HashSet::with_capacity(self.npcs.positions.len() + 1);
        let mut moved_player = false;
        if let (Some(slot), Some(pos)) = (player_slot, player_pos) {
            seen.insert(slot);
            match self.locomotion.motion_prev.insert(slot, pos) {
                Some(prev) if prev != pos => {
                    moved_player = true;
                    self.locomotion.actor_moving.insert(slot);
                }
                _ => {}
            }
        }
        for (&slot, &pos) in &self.npcs.positions {
            if Some(slot) == player_slot {
                // The player is tracked off its move_state above; a stale
                // mirror of it here must not double-report.
                continue;
            }
            seen.insert(slot);
            match self.locomotion.motion_prev.insert(slot, pos) {
                Some(prev) if prev != pos => {
                    self.locomotion.actor_moving.insert(slot);
                }
                _ => {}
            }
        }
        // Drop slots that are no longer tracked (scene actors torn down), so
        // a later scene reusing the slot number starts from a fresh seed
        // instead of diffing against a dead actor's last position.
        self.locomotion
            .motion_prev
            .retain(|slot, _| seen.contains(slot));
        if moved_player && let Some(anim) = &mut self.locomotion.player_anim {
            anim.moved_this_frame = true;
        }
    }

    /// One field-frame advance of the player's locomotion animation: pick
    /// idle vs walk off the movement flag the locomotion step just set, emit
    /// the active clip's frame into the player actor's `pose_frame`. Called
    /// by [`World::tick`]'s field branch right after
    /// [`World::step_field_locomotion`].
    pub(crate) fn tick_field_player_anim(&mut self) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let Some(anim) = &mut self.locomotion.player_anim else {
            return;
        };
        let pose = anim.tick();
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.pose_frame = Some(pose);
        }
    }

    /// Run [`vm::move_vm::actor_tick`] for `slot` against the given `bytecode`
    /// with the supplied opcode `budget`. Returns the typed outcome -
    /// engines route `Halted` to their halt-handler, `EndOfBuffer` to "clear
    /// the move", `Pending` to a debug log.
    pub fn actor_tick_at(
        &mut self,
        slot: usize,
        bytecode: &[u16],
        budget: usize,
    ) -> vm::move_vm::ActorTickOutcome {
        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: Some(slot),
            deferred_writes: std::collections::BTreeMap::new(),
            field_record_words: None,
            child_spawns: Vec::new(),
        };
        let actor_state = unsafe {
            // SAFETY: same disjoint-field justification as `step_move_vm`.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        let outcome = vm::move_vm::actor_tick(&mut host, actor_state, bytecode, budget);
        let writes = std::mem::take(&mut host.deferred_writes);
        if !writes.is_empty()
            && let Some(buf) = self.move_vm.bytecode.get_mut(slot)
        {
            for (off, value) in writes {
                if off >= buf.len() {
                    buf.resize(off + 1, 0);
                }
                buf[off] = value;
            }
        }
        outcome
    }

    /// Build the per-frame sprite list for the renderer. One
    /// [`ActorSpriteRequest`] per active actor with a [`SpriteFrame`] set;
    /// the screen-space coordinates are derived from the actor's
    /// `move_state.world_x` / `move_state.world_z` (PSX field coords) by
    /// flattening to a top-down `(x, z)` view and adding the sprite's
    /// `anchor_y`. Engines that have a real camera projection pre-process
    /// the move_state coords before populating [`Actor::sprite_frame`] (or
    /// override this helper).
    ///
    /// Mirrors the retail `FUN_80021DF4` per-frame actor tick's "draw
    /// sprite at world position" pre-pass - the actual GPU upload happens
    /// in `legaia_engine_render` against the supplied atlas.
    pub fn collect_sprite_requests(&self) -> Vec<ActorSpriteRequest> {
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(slot, a)| {
                if !a.active {
                    return None;
                }
                let frame = a.sprite_frame?;
                let world_x = a.move_state.world_x as i32;
                let world_y = a.move_state.world_z as i32 + frame.anchor_y as i32;
                Some(ActorSpriteRequest {
                    actor_slot: slot as u8,
                    world_x,
                    world_y,
                    atlas_src: frame.atlas_src,
                    tint: frame.tint,
                })
            })
            .collect()
    }

    /// Set the sprite frame for the actor at `slot`. Idempotent - passing
    /// `None` removes the frame so the actor stops rendering as a sprite.
    pub fn set_actor_sprite(&mut self, slot: u8, frame: Option<SpriteFrame>) {
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.sprite_frame = frame;
        }
    }

    /// Seat the **actor clone** field-VM op `0x4C` sub-1 sub-op `0x14` asks
    /// for: a fading, tinted copy of `src_id`'s transform on a pool slot
    /// whose handler is the clip-fraction fade.
    ///
    /// PORT: FUN_801D835C (the helper; the plan kernel is
    /// [`crate::field_actor_clone::clone_plan`])
    /// REF: FUN_8003C83C (the id resolve), FUN_80020DE0 (the allocation),
    /// REF: FUN_801D820C (the tick [`Self::tick_handler_actors`] then runs)
    ///
    /// `src_id` is resolved in the cross-context target space: `0xF8` is the
    /// player anchor, any other id is matched against the scene's script
    /// channels and read at its **live** position
    /// (`World::npcs.positions`, which the walk legs update) rather than at
    /// its MAN spawn point. An unresolvable id seats nothing, which is
    /// retail's `beqz s5` skip; so does an exhausted pool.
    ///
    /// Returns the seated slot.
    pub fn spawn_actor_clone(&mut self, src_id: u8, modulation: u32, rate: i16) -> Option<usize> {
        let src = self.clone_source(src_id)?;
        let plan = crate::field_actor_clone::clone_plan(src, modulation, rate);
        let start = FIELD_SPAWN_START_SLOT as usize;
        let slot_idx = self
            .actors
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, a)| !a.active)
            .map(|(i, _)| i)?;
        // Retail's `FUN_80020DE0` hands back a zeroed node stamped from the
        // descriptor, so the clone starts from a default record rather than
        // from whatever the slot last held.
        let mut actor = Actor {
            active: true,
            handler: crate::actor_handler::ActorHandler::ClipFade,
            state_54: crate::field_actor_clone::CLONE_DESCRIPTOR_INITIAL_STATE,
            ..Actor::default()
        };
        actor.physics.world_x = plan.pos.0;
        actor.physics.world_y = plan.pos.1;
        actor.physics.world_z = plan.pos.2;
        actor.physics.motion_x = plan.rot.0;
        actor.physics.motion_y = plan.rot.1;
        actor.physics.motion_z = plan.rot.2;
        actor.physics.timer = plan.rate;
        actor.physics.focal_envelope = plan.fraction;
        actor.move_state.world_x = plan.pos.0;
        actor.move_state.world_y = plan.pos.1;
        actor.move_state.world_z = plan.pos.2;
        actor.move_state.render_24 = plan.rot.0;
        actor.move_state.render_26 = plan.rot.1;
        actor.move_state.render_28 = plan.rot.2;
        actor.modulation_rgb = Some(crate::field_actor_clone::modulation_rgb(plan.modulation));
        self.actors[slot_idx] = actor;
        Some(slot_idx)
    }

    /// Seat the **reflection controller** the field VM's `4C 86` installs:
    /// a pool actor that mirrors one addressable actor's pose onto another
    /// across an axis-aligned plane, while the mirrored actor stands inside
    /// a tile rect.
    ///
    /// PORT: FUN_801E573C (the plan kernel is
    /// [`legaia_engine_vm::field_actor_reflect::spawn_controller`])
    /// REF: FUN_8003C83C (the arm's id resolve), FUN_80020DE0 (the allocation),
    /// REF: FUN_801E5154 (the tick [`Self::tick_handler_actors`] then runs)
    ///
    /// `ctx_is_player` is the executing context's `+0x10 & 0x01000000`, the
    /// same bit the eased-move arm reads; [`crate::world::FieldVmState::executing_channel`]
    /// resolves retail's `a0` - the script's own actor, which becomes the
    /// image - and the bit only stands in when no channel is executing. The
    /// `source_id` byte resolves in the cross-context target space (`0xF8` is
    /// the player), and an id that names nothing seats nothing, which is the
    /// arm's own `beqz s7` skip.
    ///
    /// Returns the seated slot.
    pub fn spawn_reflection_controller(
        &mut self,
        ctx_is_player: bool,
        source_id: u8,
        words: [i16; 6],
    ) -> Option<usize> {
        use crate::world::EasedMoveTarget;
        // Retail's `a0` is the executing context STRUCT, and a talk record's
        // context is the record's own actor even when its `+0x10` player bit
        // is up (the bit says who raised the record, not whose pose the
        // context carries). Every shipped `4C 86` sits in such a record and
        // names `0xF8`, so reading the bit as "the context is the player"
        // pairs the player with itself and the tick teleports them onto the
        // mirror line - the `other1` / `ropeway2` cold-spawn regression. The
        // record's placement is the image; the player bit only decides the
        // seat when there is no executing channel at all.
        let destination = match (self.field_vm.executing_channel, ctx_is_player) {
            (Some(placement), _) => EasedMoveTarget::Placement(placement),
            (None, true) => EasedMoveTarget::Player,
            (None, false) => return None,
        };
        let source = if source_id == 0xF8 {
            EasedMoveTarget::Player
        } else {
            let view = self.channel_view();
            let ci = crate::field_channels::resolve_target(view, source_id)?;
            EasedMoveTarget::Placement(view[ci].placement_index as u8)
        };
        if source == destination {
            // A pair whose two ends are one actor would write that actor's
            // own mirrored pose back onto it every frame; no retail record
            // forms one, and seating it can only strand the player.
            return None;
        }
        let slot = self.spawn_handler_actor(crate::actor_handler::ActorHandler::Reflection)?;
        let controller = legaia_engine_vm::field_actor_reflect::spawn_controller(words);
        let a = &mut self.actors[slot];
        a.state_54 = legaia_engine_vm::field_actor_reflect::REFLECT_INITIAL_STATE;
        a.reflection = Some(crate::world::ReflectionLink {
            destination,
            source,
            controller,
        });
        Some(slot)
    }

    /// The scene's channel set as a cross-context resolve should see it: the
    /// live vector, or - while a stepping pass has moved it out to execute one
    /// of its channels - the copy that pass took
    /// ([`crate::world::FieldVmState::stepping_view`]).
    ///
    /// Retail's resolver (`FUN_8003C83C`) walks the whole actor list whatever
    /// is executing, and a `4C 86` / `4C 14` / talk op that names another
    /// actor resolves it from inside a running script by construction. Read
    /// against the live vector alone, every such id resolved to nothing during
    /// a step: `conc2` seated one of its three entry-time mirrors (the one
    /// naming the player, which bypasses the walk) where a retail capture of
    /// the same entry seats all three.
    // REF: FUN_8003C83C
    pub fn channel_view(&self) -> &[crate::field_channels::FieldChannel] {
        if self.field_vm.channels.is_empty() {
            &self.field_vm.stepping_view
        } else {
            &self.field_vm.channels
        }
    }

    /// The source-actor fields the clone helper reads, resolved from a
    /// cross-context target byte.
    // REF: FUN_8003C83C
    fn clone_source(&self, src_id: u8) -> Option<crate::field_actor_clone::CloneSource> {
        use crate::field_actor_clone::CloneSource;
        if src_id == 0xF8 {
            let slot = self.player_actor_slot? as usize;
            let a = self.actors.get(slot)?;
            return Some(CloneSource {
                pos: (
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ),
                rot: (
                    a.move_state.render_24,
                    a.move_state.render_26,
                    a.move_state.render_28,
                ),
                ..CloneSource::default()
            });
        }
        let view = self.channel_view();
        let ci = crate::field_channels::resolve_target(view, src_id)?;
        let ch = &view[ci];
        let placement = ch.placement_index as u8;
        let (x, z) = self
            .npcs
            .positions
            .get(&placement)
            .copied()
            .unwrap_or((ch.ctx.world_x as i16, ch.ctx.world_z as i16));
        Some(CloneSource {
            pos: (x, ch.ctx.world_y as i16, z),
            rot: (ch.ctx.field_24, ch.ctx.field_26 as i16, ch.ctx.field_28),
            ..CloneSource::default()
        })
    }

    /// Allocate a field actor in the auto-spawn slot range
    /// ([`FIELD_SPAWN_START_SLOT`]..), resolving its mesh from the global
    /// TMD pool (`tmd_idx`) and its spawn record from the VDF buffer
    /// (`vdf_idx`), and stamping the `kind`/`variant` classifier. Returns
    /// the allocated slot index, or `None` when the pool is exhausted.
    ///
    /// Shared by the field-VM synchronous actor allocator (the `0x4C 0xD8`
    /// path, retail `FUN_801D77F4`) and the tile-board install
    /// ([`World::try_install_tile_board`]); both resolve a template id
    /// through the same global-TMD + VDF-buffer path. Slots below
    /// [`FIELD_SPAWN_START_SLOT`] are skipped so party / scripted actors
    /// stay out of the auto-allocation range. Unresolved indices leave the
    /// actor with an empty `tmd_ref` / `spawn_record` (the synchronous
    /// spawn still succeeds), matching the retail bail-through.
    pub(crate) fn spawn_field_actor(
        &mut self,
        tmd_idx: i16,
        vdf_idx: u8,
        kind: u16,
        variant: u16,
    ) -> Option<usize> {
        let tmd_ref = self.global_tmd(tmd_idx).cloned();
        let record_bytes: Vec<u8> = self
            .vdf_record_bytes(vdf_idx)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let start = FIELD_SPAWN_START_SLOT as usize;
        let slot_idx = self
            .actors
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, a)| !a.active)
            .map(|(i, _)| i)?;
        let actor = &mut self.actors[slot_idx];
        // A retired occupant's handler, reflection link and move state must
        // not leak into the new actor (retail's allocator rewrites them).
        actor.init_allocated();
        actor.kind = kind;
        actor.variant = variant;
        actor.tmd_ref = tmd_ref;
        actor.spawn_record = if record_bytes.is_empty() {
            None
        } else {
            Some(record_bytes)
        };
        Some(slot_idx)
    }

    /// Step the **Arts announcement banner** one battle frame.
    ///
    /// PORT: FUN_801E2524 (driver; the kernel is
    /// [`legaia_engine_vm::battle_action::step_flash_ramp`])
    ///
    /// Retail calls the ramp unconditionally from the battle draw tick
    /// (`FUN_800480D8`, `jal 0x801E2524` at `0x80048140`) and lets the stage
    /// byte gate it. The engine splits simulation from presentation, so the
    /// *step* runs here - inside the battle frame tick, where every host
    /// reaches it - and the quads come off the resulting state at draw time
    /// ([`Self::battle_arts_banner_quads`]). Returns `true` when the frame
    /// drew something, which is what a test can assert without a renderer.
    pub fn tick_arts_banner(&mut self, frame_delta: u8) -> bool {
        use legaia_engine_vm::battle_action as fr;
        let frame = fr::step_flash_ramp(
            self.battle_ctx.arts_banner_stage,
            self.battle_ctx.arts_banner_level,
            frame_delta,
        );
        if let Some(stage) = frame.stage_out {
            self.battle_ctx.arts_banner_stage = stage;
        }
        if let Some(level) = frame.level_out {
            self.battle_ctx.arts_banner_level = level;
        }
        !frame.layers.is_empty()
    }

    /// This frame's banner quads, in retail emit order - the shared read both
    /// hosts build their screen primitives from
    /// (`legaia_engine_ui::battle_numerals::arts_banner_prims`).
    ///
    /// Retail re-reads `ctx[+0x28C]` per layer *inside* the emitter, so a
    /// layer emitted later in the frame still sees the pre-walk value; the
    /// step above writes the walked value back, so this read has to recompute
    /// the layer set from the pre-walk clock rather than reuse the stepped
    /// one. Empty outside battle and on an idle banner.
    pub fn battle_arts_banner_quads(&self) -> Vec<legaia_engine_vm::battle_action::FlashQuad> {
        use legaia_engine_vm::battle_action as fr;
        if self.mode != SceneMode::Battle {
            return Vec::new();
        }
        let level = self.battle_ctx.arts_banner_level;
        // `frame_delta = 0`: the layer set for this clock, with no walk.
        let frame = fr::step_flash_ramp(self.battle_ctx.arts_banner_stage, level, 0);
        frame
            .layers
            .iter()
            .filter_map(|l| fr::flash_quads(l, level))
            .flatten()
            .collect()
    }

    /// The field VM's `0x4C 0xD8` allocator, whole: [`Self::spawn_field_actor`]
    /// plus the tail the port used to stop short of - the `actor+0x90`
    /// rest-pose snapshot, the `+0x3C`/`+0x3E` envelope rates and the
    /// `+0x0C` handler identity that makes the slot a morph actor at all.
    ///
    /// PORT: FUN_801D77F4
    ///
    /// The two immediates the instruction carries are the envelope's rise
    /// and fall rates in that order (`sh s5,0x3c` / `sh s6,0x3e` at
    /// `0x801D79A4`), and the weight, direction and render-mode halfwords
    /// are zeroed, so a fresh morph actor starts at the rest pose and rises.
    /// A spawn that resolves no morph block or no mesh seats no morph state
    /// and behaves exactly like the plain allocator - retail's own
    /// bail-through, where the snapshot allocation is a zero-byte request
    /// and the copy loop never runs.
    ///
    /// The tile-board install keeps calling [`Self::spawn_field_actor`]: its
    /// actors are not this opcode's, and the template id it passes as a VDF
    /// index would otherwise seat a morph block that retail never installs.
    pub(crate) fn spawn_morph_weight_actor(
        &mut self,
        tmd_idx: i16,
        vdf_idx: u8,
        up_rate: u16,
        down_rate: u16,
    ) -> Option<usize> {
        let slot_idx = self.spawn_field_actor(tmd_idx, vdf_idx, up_rate, down_rate)?;
        // The operand is a scene-bank index, not a raw pool slot: the arm
        // adds `*(u16*)0x8007B6F8` before the call (see
        // [`Self::field_pool_tmd`]). Resolving it against the pool the
        // engine seeds with the battle effect library bound jagaroom's and
        // garmel's morph actors to effect models and left balden's unbound.
        self.actors[slot_idx].tmd_ref = self.field_pool_tmd(tmd_idx).cloned();
        let block = match self.actors[slot_idx].spawn_record.clone() {
            Some(b) if b.len() >= 4 => b,
            _ => return Some(slot_idx),
        };
        let Some(gtmd) = self.actors[slot_idx].tmd_ref.as_ref().map(Arc::clone) else {
            return Some(slot_idx);
        };
        let groups = Self::tmd_group_vertex_bytes(&gtmd.tmd);
        let refs: Vec<&[u8]> = groups.iter().map(Vec::as_slice).collect();
        let rest_pose = crate::morph_weight_apply::rest_pose_snapshot(&block, &refs);
        let actor = &mut self.actors[slot_idx];
        actor.handler = crate::actor_handler::ActorHandler::MorphWeights;
        actor.morph_weights = Some(crate::morph_weight_apply::MorphWeightActor {
            block,
            rest_pose,
            envelope: crate::morph_weight_apply::MorphWeightEnvelope {
                weight: 0,
                up_rate: up_rate as i16,
                down_rate: down_rate as i16,
                descending: false,
            },
        });
        Some(slot_idx)
    }

    /// Every TMD object's vertex block as the 8-byte GTE vertices retail's
    /// object table points at (`[i16 x][i16 y][i16 z][i16 pad]`).
    fn tmd_group_vertex_bytes(tmd: &legaia_tmd::Tmd) -> Vec<Vec<u8>> {
        tmd.objects
            .iter()
            .map(|o| {
                let mut out = Vec::with_capacity(o.vertices.len() * 8);
                for v in &o.vertices {
                    out.extend_from_slice(&v.x.to_le_bytes());
                    out.extend_from_slice(&v.y.to_le_bytes());
                    out.extend_from_slice(&v.z.to_le_bytes());
                    out.extend_from_slice(&v._pad.to_le_bytes());
                }
                out
            })
            .collect()
    }

    /// Actor slots carrying live morph-weight state, with each one's current
    /// blend weight - the host-side change detector: re-pose and re-upload a
    /// slot whose weight has moved since the last upload.
    pub fn morph_weight_actor_weights(&self) -> Vec<(u8, i16)> {
        self.actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.active)
            .filter_map(|(i, a)| {
                let m = a.morph_weights.as_ref()?;
                Some((u8::try_from(i).ok()?, m.envelope.weight))
            })
            .collect()
    }

    /// The **one** engine-side morph kernel both hosts draw through: actor
    /// `slot`'s mesh with its rest pose restored and its morph deltas
    /// re-blended at the live `+0x6E` weight, returned as a posed TMD
    /// alongside the raw bytes a VRAM-mesh build needs and the weight it was
    /// posed at.
    ///
    /// This is [`crate::morph_weight_apply::apply_morph_weights`] run over
    /// the engine's own vertex representation, so neither host owns any part
    /// of the blend. `None` when the slot carries no morph state, no mesh,
    /// or a block that names nothing the mesh has.
    pub fn morph_weight_posed_tmd(
        &self,
        slot: usize,
    ) -> Option<(legaia_tmd::Tmd, Arc<Vec<u8>>, i16)> {
        let actor = self.actors.get(slot)?;
        if !actor.active {
            return None;
        }
        let m = actor.morph_weights.as_ref()?;
        let gtmd = actor.tmd_ref.as_ref()?;
        let mut groups = Self::tmd_group_vertex_bytes(&gtmd.tmd);
        let weight = m.envelope.weight;
        if crate::morph_weight_apply::apply_morph_weights(
            &m.block,
            &mut groups,
            &m.rest_pose,
            weight,
        ) == 0
        {
            return None;
        }
        let mut posed = gtmd.tmd.clone();
        for (obj, bytes) in posed.objects.iter_mut().zip(groups.iter()) {
            for (i, v) in obj.vertices.iter_mut().enumerate() {
                let o = i * 8;
                if o + 6 > bytes.len() {
                    break;
                }
                v.x = i16::from_le_bytes([bytes[o], bytes[o + 1]]);
                v.y = i16::from_le_bytes([bytes[o + 2], bytes[o + 3]]);
                v.z = i16::from_le_bytes([bytes[o + 4], bytes[o + 5]]);
            }
        }
        Some((posed, Arc::new(gtmd.raw.clone()), weight))
    }
}
