//! Per-frame time/RNG/pad, the top-level tick dispatcher, and the minigame ticks (tile board, dance, fishing, slot machine, baka fighter, muscle dome).
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.
//!
//! The frame-time sampler behind the adaptive cadence
//! ([`World::resolve_frame_step`]) - retail's `VSync(1)` reading, returned by
//! the dev profiler HUD.
//! REF: FUN_800173BC

use super::*;

/// The scene a travel art's destination word names, or `None` for a miss.
///
/// A word inside the TOC's header rows (`< RAW_TOC_INDEX_OFFSET`) names a
/// head define - `init_data 0`, `gameover_data 1` - not a scene. Retail's
/// resolve scan would match it, but there is no field to load there, and the
/// scene host's entry fails half-way through its reset. It is the word a
/// region record with an all-zero return triple stores (station3's), so a
/// Door of Light used there drops like any other unresolved word.
fn travel_scene(names: &legaia_prot::cdname::IndexMap, word: u32) -> Option<String> {
    if word < legaia_prot::cdname::RAW_TOC_INDEX_OFFSET {
        return None;
    }
    names.get(&word).cloned()
}

/// The longest gap between two play-clock readings that still counts as
/// play ([`World::tick_play_clock`]). The hosts read once a tick, so anything
/// near this is a stall.
pub const PLAY_CLOCK_STALL_SECS: f64 = 5.0;

impl World {
    /// The colour the frame is cleared to this tick - the one both hosts hand
    /// their clear (`battle_stage_clear::scene_clear`'s field input).
    ///
    /// The field's draw-environment clear (`presentation.clear_rgb`, op
    /// `4C 13`) outside four minigame modes. Under the slot machine, Baka
    /// Fighter, the Muscle Dome and the dance the frame clears to **black**:
    /// every retail state of those four holds `r0 / g0 / b0 = 0` at
    /// `0x8007BF5D..5F` and a `0x0000` display background around its HUD
    /// (`minigame_slot_machine`, `minigame_baka_fighter`,
    /// `minigame_muscle_dome`, `minigame_dance_noa`), entered from a town
    /// whose own states carry a non-zero clear. The port suspends the field
    /// instead of reloading over it, so without this the town's colour
    /// showed behind the cabinet and the attract card. Fishing keeps the
    /// field's word: its venue (`other1`) writes its own sky colour, which
    /// the retail fishing state carries.
    pub fn frame_clear_rgb(&self) -> [u8; 3] {
        match self.mode {
            SceneMode::SlotMachine
            | SceneMode::BakaFighter
            | SceneMode::MuscleDome
            | SceneMode::Dance => [0; 3],
            _ => self.presentation.clear_rgb,
        }
    }

    /// Advance the wall-clock play-time counter by `delta_seconds`. Engines
    /// drive this from the frame loop's wall-clock delta. Mirrors the
    /// retail "play time" field shown on the save screen.
    pub fn advance_play_time(&mut self, delta_seconds: u32) {
        self.clock.play_time_seconds = self.clock.play_time_seconds.saturating_add(delta_seconds);
    }

    /// Advance the play clock off a host wall-clock reading `now_secs` (any
    /// epoch - only deltas are used). Whole seconds only, and by delta
    /// against a high-water mark rather than absolutely, so a loaded save
    /// keeps its accumulated total.
    ///
    /// The one play-clock law both play hosts call. Each used to keep its own
    /// origin and high-water mark, and they drifted: the native window
    /// measured from window creation, so the title screen's time landed in
    /// the first delta after New Game; the browser page reset its origin on
    /// New Game but not its high-water mark, so after a second New Game play
    /// time stood still until the wall clock passed the old mark. The origin
    /// is set by the first call and dropped by [`Self::begin_new_game`].
    ///
    /// A reading more than [`PLAY_CLOCK_STALL_SECS`] after the previous one is
    /// a stalled host, not play: a paused or backgrounded browser tab, a
    /// minimised window, a suspended machine. Retail's counter is frames the
    /// game ran, so the gap is taken out of the origin and counts nothing;
    /// both hosts read every tick, and before this a page left paused for an
    /// hour came back an hour older on its save screen.
    pub fn tick_play_clock(&mut self, now_secs: f64) {
        if let (Some(origin), Some(last)) = (
            self.clock.play_clock_origin.as_mut(),
            self.clock.play_clock_last,
        ) {
            let gap = now_secs - last;
            if gap > PLAY_CLOCK_STALL_SECS {
                *origin += gap;
            }
        }
        self.clock.play_clock_last = Some(now_secs);
        let origin = *self.clock.play_clock_origin.get_or_insert(now_secs);
        let now = (now_secs - origin).max(0.0) as u32;
        if now > self.clock.play_clock_high_water {
            let delta = now - self.clock.play_clock_high_water;
            self.clock.play_clock_high_water = now;
            self.advance_play_time(delta);
        }
    }

    /// Commit a host font measurement of the live `4C E1` balloon's line, so
    /// the record carries the centred `x` retail computes at spawn
    /// (`X = (0x140 - width) >> 1`).
    ///
    /// Retail measures inside `FUN_8003C764` because its font metrics are in
    /// the same address space; the engine's atlas is host-side, so the
    /// measurement arrives from the draw layer instead
    /// (`legaia_engine_ui::text_balloon_text_width`). Idempotent - the width
    /// is committed once per balloon and a later call is ignored, which is
    /// what keeps a host that measures every frame from re-centring a
    /// balloon mid-life.
    ///
    /// Returns the pen to draw at, or `None` when no balloon is live.
    ///
    /// REF: FUN_8003C764 (`0x8003C7C0..0x8003C7DC`, the measure + centre)
    pub fn commit_text_balloon_width(&mut self, text_width_px: i16) -> Option<(i32, i32)> {
        let balloon = self.cutscene.text_balloon.as_mut()?;
        if balloon.x.is_none() {
            balloon.center_with_width(text_width_px);
        }
        balloon.pen()
    }

    /// The live `4C E1` balloon's raw page bytes while it is past its startup
    /// tick and still running - i.e. exactly the frames retail's handler
    /// reaches `FUN_80036888`. `None` otherwise.
    ///
    /// The startup band (`timer < 1`) draws nothing in retail, so a host that
    /// keys on `text_balloon.is_some()` shows the balloon one frame early.
    pub fn text_balloon_drawing(&self) -> Option<&[u8]> {
        let b = self.cutscene.text_balloon.as_ref()?;
        (!b.killed && b.timer >= 1 && b.timer < b.total).then_some(b.text.as_slice())
    }

    /// Run every live camera-register zone ramp (field-VM op `0x43`
    /// sub-3..6) for one frame - the port of `FUN_80037018`'s per-actor tick,
    /// driven off the player's world position.
    ///
    /// Retail runs one of these actors per spawned ramp off the effect-actor
    /// list; the engine holds the records on [`crate::world::CameraRig::register_ramps`] and
    /// steps them here. What a tick writes lands in
    /// [`crate::world::CameraRig::registers`], the four field camera-configuration
    /// registers `0x8007B60C`/`B610`/`B614`/`B618`.
    ///
    /// Two retail gates come first, both inside
    /// [`legaia_engine_vm::ambient_motion::zone_ramp_tick`]: the
    /// player-engaged flag (`_DAT_8007C364[+0x10] & 0x80000`, host-substituted
    /// by "a dialog engagement is live", the same substitution the `4C E1`
    /// balloon uses) and the scratch system lock (`_DAT_1F800394 & 0x400`,
    /// which has no engine counterpart and is passed clear).
    ///
    /// Nothing counts down in a zone ramp, so a record never completes - it
    /// tracks the player and runs backwards when he walks back. Retail clears
    /// them on the MAN loader's retire sweep, which is
    /// [`World::reset_for_scene_entry`] here. Two arms do drop a record:
    /// an out-of-range destination width (retail sets the actor's own
    /// `+0x10 |= 8` yield bit instead of writing) and a degenerate `z_lo ==
    /// z_hi` window (retail divides by the zero span and executes the MIPS
    /// `break 0x1C00` trap - the port drops the record rather than reproducing
    /// a CPU exception; no on-disc ramp is authored that way).
    ///
    /// REF: FUN_80037018
    pub fn tick_register_ramps(&mut self) {
        if self.camera.register_ramps.is_empty() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let Some(actor) = self.actors.get(slot as usize) else {
            return;
        };
        let (px, pz) = (actor.move_state.world_x, actor.move_state.world_z);
        let engaged = self.dialogue_owns_input();
        let mut writes: Vec<(crate::register_ramp::RampSlot, i32)> = Vec::new();
        let mut retire: Vec<usize> = Vec::new();
        for (i, ramp) in self.camera.register_ramps.iter().enumerate() {
            match ramp.tick(px, pz, engaged) {
                legaia_engine_vm::ambient_motion::ZoneRampTick::Write { value, .. } => {
                    writes.push((ramp.slot, value));
                }
                legaia_engine_vm::ambient_motion::ZoneRampTick::Retire
                | legaia_engine_vm::ambient_motion::ZoneRampTick::DivideByZero => retire.push(i),
                legaia_engine_vm::ambient_motion::ZoneRampTick::Idle => {}
            }
        }
        for (slot, value) in writes {
            self.camera.registers.set(slot, value);
        }
        for i in retire.into_iter().rev() {
            self.camera.register_ramps.remove(i);
        }
    }

    /// Run every live **frame-delta timer actor** for one frame: the
    /// cinematic bar envelope, the eased moves, and the floor-height-ladder
    /// oscillators.
    ///
    /// These are the three plain templates in the field overlay's own table
    /// (`0x801F2858` / `0x801F2840` / `0x801F27EC`), and they are one method
    /// because retail runs all three off the same actor list on the same
    /// frame delta - the engine holds each family on its own `World` field
    /// instead of a shared pool, but the cadence has to stay identical or a
    /// wipe and a move spawned by the same script beat would drift apart.
    ///
    /// `frame_delta` is retail's `DAT_1F800393`, the same scalar the physics
    /// and colour-tween passes carry.
    ///
    /// REF: FUN_801DD784, FUN_801DD4C4, FUN_801DA930 (the kernels live in
    /// [`legaia_engine_vm::field_actor_timers`])
    /// The player actor's `+0x16` footing height, or `0` when there is no
    /// live player actor. The subtrahend of the camera-offset easing's target.
    pub fn camera_ease_player_footing(&self) -> i16 {
        self.player_actor_slot
            .map(usize::from)
            .filter(|&slot| slot < self.actors.len() && self.actors[slot].active)
            .map_or(0, |slot| self.actors[slot].move_state.world_y)
    }

    /// One frame of the camera vertical-offset easing.
    ///
    /// REF: FUN_801DA390 - this is the per-frame call site retail runs off the
    /// field frame pump; the ported kernel is
    /// [`crate::camera_ease::ease_camera_offset`], which carries the tag.
    ///
    /// Supplies the kernel's six actor-side inputs from the engine's own
    /// player actor. Two of them stand in for retail slots the engine does not
    /// carry: `+0x1E` / `+0x20` become the previous tick's `(world_y,
    /// world_z)`, so the settle test still answers "has the actor stopped
    /// moving in Y and Z". The pad word is [`crate::world::StoryFlagState::story_flags`] - the same
    /// `_DAT_1F800394` scratchpad word retail reads the input lock and the
    /// fast-arm bit out of - and `_DAT_8007B850` is passed as `0`, which
    /// leaves the fast arm disengaged (see the comment at the call).
    ///
    /// The result is observable engine state, not a camera input:
    /// [`crate::camera`] still drives the rendered view from its float
    /// controller, and choosing between them is a fidelity-mode decision.
    pub fn tick_camera_offset_ease(&mut self) {
        let (y, z) = match self
            .player_actor_slot
            .map(usize::from)
            .filter(|&slot| slot < self.actors.len() && self.actors[slot].active)
        {
            Some(slot) => {
                let ms = &self.actors[slot].move_state;
                (ms.world_y, ms.world_z)
            }
            None => {
                self.camera.ease_prev_yz = None;
                return;
            }
        };
        let (prev_y, prev_z) = self.camera.ease_prev_yz.unwrap_or((y, z));
        self.camera.offset_ease =
            crate::camera_ease::ease_camera_offset(crate::camera_ease::CameraEaseInput {
                pad: self.flags.story_flags,
                scene_target: self.camera.scene_offset as u16,
                player_footing: y as u16,
                footing_settled: prev_y,
                z,
                z_target: prev_z,
                // `_DAT_8007B850` has no engine mirror, so the fast arm
                // never engages. It is a shortcut, not a behaviour: with it
                // clear the adaptive arm reaches the same destination in the
                // same 12-units-a-frame cap, only without pinning the step
                // while the gap is small.
                fast_flags: 0,
                current: self.camera.offset_ease,
            });
        self.camera.ease_prev_yz = Some((y, z));
    }

    pub fn tick_field_timer_actors(&mut self, frame_delta: u8) {
        // The bar envelope. It retires itself at phase 3, and the published
        // height is what both hosts' screen-prim pass reads.
        if let Some(bars) = self.presentation.cinematic_bars.as_mut() {
            self.presentation.cinematic_bar = bars.step(frame_delta);
            if bars.retired {
                self.presentation.cinematic_bars = None;
                self.presentation.cinematic_bar = 0;
            }
        } else {
            self.presentation.cinematic_bar = 0;
        }

        // The eased moves. Retail writes the target's `+0x14/+0x16/+0x18`
        // straight through the back-link; the engine writes the same triple
        // wherever that target lives.
        if !self.field_vm.eased_moves.is_empty() {
            let mut records = std::mem::take(&mut self.field_vm.eased_moves);
            for rec in records.iter_mut() {
                let frame = rec.ease.step(frame_delta, rec.target_flags);
                self.apply_eased_move(rec.target, &frame);
            }
            records.retain(|r| !r.ease.retired);
            // A spawn that landed during this pass appended to the (empty)
            // live list, so splice rather than overwrite.
            records.append(&mut self.field_vm.eased_moves);
            self.field_vm.eased_moves = records;
            // Drop the `+0x8E` latch once the last mirrored player move has
            // retired. Retail's byte is sticky and its reader gates on the
            // actor flag `+0x10 & 0x20000000` instead, which a script clears;
            // the engine has no writer for that bit on a pool actor, so a
            // sticky latch here would pin the player's Y for the rest of the
            // scene - a softlock class, not a fidelity gain. The armed window
            // is otherwise identical: every frame of the move, and no other.
            if !self
                .field_vm
                .eased_moves
                .iter()
                .any(|r| matches!(r.target, crate::world::EasedMoveTarget::Player))
            {
                self.locomotion.eased_mirror_y = None;
            }
        }

        // The floor-height ladder. Each record owns one rung; a rung index
        // past the LUT is retail writing off the end of a 16-entry array,
        // which the port declines to do.
        if !self.terrain.floor_tier_bobs.is_empty() {
            let mut bobs = std::mem::take(&mut self.terrain.floor_tier_bobs);
            crate::world::step_floor_ladder(
                &mut bobs,
                &mut self.terrain.floor_height_lut,
                frame_delta,
            );
            bobs.append(&mut self.terrain.floor_tier_bobs);
            self.terrain.floor_tier_bobs = bobs;
        }
    }

    /// Write one eased-move frame through its `+0x90` back-link.
    ///
    /// The engine's two addressable targets are the player's pool slot and a
    /// scene NPC placement, which is exactly the pair the neighbouring
    /// `move_to` host resolves - see [`crate::world::EasedMoveTarget`].
    ///
    /// The `+0x8E` **inverted-Y mirror** is published too, into
    /// [`crate::world::FieldLocomotion::eased_mirror_y`]. It used to be dropped
    /// here for want of a consumer; the consumer is retail's own, and it was
    /// mis-read rather than missing. `FUN_8003BC08`'s height arm tests the
    /// same `0x20000000` flag before either of its ground-height arms
    /// (`0x8003BC4C..0x8003BC64`) and writes `-(+0x8E)` into the actor's
    /// `+0x16`, which is the Y **position** the eased move itself writes - so
    /// the mirror is a hold: it re-asserts the scripted Y against the
    /// per-frame floor follow. The engine's two height controllers
    /// (`World::locomotion.vertical_settle` and `World::locomotion.follow_terrain_height`)
    /// are the ports of that routine's other two arms and both stand down
    /// while the latch is armed.
    ///
    /// Only the player half is published. A scene NPC placement has no
    /// per-frame height controller in this engine (its Y is baked at scene
    /// build by [`legaia_asset::field_objects::Placement::world_y`]), so a
    /// mirror on one would be a write with no reader - the same reason the
    /// whole field was withheld before, now true of one target instead of
    /// both.
    fn apply_eased_move(
        &mut self,
        target: crate::world::EasedMoveTarget,
        frame: &legaia_engine_vm::field_actor_timers::EasedMoveFrame,
    ) {
        match target {
            crate::world::EasedMoveTarget::Player => {
                // Published whether or not a seat resolves: the latch is a
                // property of the move, and retail's store goes through the
                // back-link ahead of anything that reads the seat.
                self.locomotion.eased_mirror_y = frame.mirror_y;
                let Some(slot) = self.player_actor_slot else {
                    return;
                };
                let Some(actor) = self.actors.get_mut(slot as usize) else {
                    return;
                };
                if let Some(x) = frame.axis[0] {
                    actor.move_state.world_x = x;
                    actor.physics.world_x = x;
                }
                if let Some(y) = frame.axis[1] {
                    actor.physics.world_y = y;
                }
                if let Some(z) = frame.axis[2] {
                    actor.move_state.world_z = z;
                    actor.physics.world_z = z;
                }
                if let Some(x) = frame.axis[0] {
                    self.field_ctx.world_x = x as u16;
                }
                if let Some(z) = frame.axis[2] {
                    self.field_ctx.world_z = z as u16;
                }
            }
            crate::world::EasedMoveTarget::Placement(slot) => {
                let cur = self.npcs.positions.get(&slot).copied();
                let (mut x, mut z) = cur.unwrap_or((0, 0));
                if let Some(nx) = frame.axis[0] {
                    x = nx;
                }
                if let Some(nz) = frame.axis[2] {
                    z = nz;
                }
                self.npcs.positions.insert(slot, (x, z));
            }
        }
    }

    /// Latch a mid-talk "switch character" request for the active
    /// three-actor talk. Engine input standing in for retail's pad-derived
    /// word `_DAT_8007B874` bit `0x80` - the request route of the talk
    /// controller's state-0 arm gate (`FUN_801D27E0` `801d2998..801d29a8`).
    /// No-op outside a talk (the latch is dropped by the poll).
    ///
    /// A **second** request route, not the production one. The player's route
    /// is the pad edge retail itself reads, sampled inside
    /// [`Self::tick_three_actor_talk`]; this latch exists for a caller with no
    /// pad word to press - a scripted timeline, a replay fixture, a test.
    // REF: FUN_801D27E0 (state-0 arm gate, request-byte route)
    pub fn request_talk_leader_switch(&mut self) {
        self.dialog.talk_switch_requested = true;
    }

    /// Per-frame step of the three-actor-talk controller SM
    /// (`FUN_801D27E0`, six states at controller `+0x54`).
    ///
    /// PORT: FUN_801D27E0
    ///
    /// The SM kernel is [`crate::cutscene_script_elements::LeaderSwap`]
    /// (the byte-level port of the dispatcher, gate, and search); this tick
    /// is its host. Per state:
    ///
    /// - **0** - caches the three participants' poses into the session's
    ///   saved table (retail rewrites `0x800845E4` every state-0 frame,
    ///   `801d2838..801d28a0` - the table a mid-talk re-arm restores from),
    ///   polls the talk lock (system flag `0xD`, `jal 0x8003ce64 a0=0xD` at
    ///   `801d28c8`; clear routes to state 5 = despawn), then runs the
    ///   switch arm gate over the presence flags `script_id + 0..=2`. The
    ///   request source is the **pad**, read here: retail's
    ///   `801d2998..801d29a8` is `lw _DAT_8007B874; andi 0x80`, the
    ///   newly-pressed word AND packed bit `0x80` - Square, the same bit the
    ///   fishing reel decoder pins. Reading it inside the world is what makes
    ///   the switch reachable everywhere at once: every host drives
    ///   [`Self::set_pad`], so none of them needs a binding of its own and
    ///   there is no second key table to drift from the engine's. The retail
    ///   suppressor pair `_DAT_8007B6B4` / `_DAT_8007B6B0` is
    ///   host-substituted by "a dialogue owns the pad", so a switch never
    ///   arms under an open text box.
    ///
    ///   The port latches its field **run** modifier off the same pad word
    ///   ([`crate::world::FieldLocomotion::run_button_held`], mask
    ///   [`crate::world::FieldLocomotion::run_button_mask`]), so inside an armed talk one press
    ///   does both. Retail behaves the same way - its run mask word
    ///   `0x800846DC` is `0x48` = Cross | R1, and Cross is also the talk
    ///   button - so this is the retail overlap, not a port divergence.
    /// - **1** - hold [`LEADER_SWAP_FADE_FRAMES`] behind the fade-to-white
    ///   ([`crate::world::ScreenFxState::fade`] carries the retail template: kind 2, `0x20`
    ///   frames, black -> white, `801d29c8..801d2a00`).
    /// - **2** - the swap (`801d2a54..801d2c7c`): the outgoing leader's
    ///   participant NPC takes the player's pose (retail: camera `+0x14..`
    ///   onto the outgoing actor), the next participant whose presence flag
    ///   reads clear becomes leader (wrap-scan from `leader+1`), flags
    ///   `0x10..=0x12` are re-pointed at the new leader, the player takes
    ///   the incoming participant's pose (+ the negated map origin
    ///   `_DAT_80089118/20` = [`crate::world::FieldTerrain::map_origin_xz`]), the incoming NPC is
    ///   parked at the `0x3F80` sentinel, and the fade-back-in spawns.
    /// - **3** - release the fade object (engine: [`crate::world::ScreenFxState::fade`]
    ///   steps itself; nothing to do).
    /// - **4** - hold the fade-in, then clear the camera-busy latch and
    ///   return to state 0 (the poll).
    /// - **5** - despawn = [`Self::end_three_actor_talk`]. The engine folds
    ///   retail's one-frame 0 -> 5 despawn delay into the same tick.
    ///
    /// Not modelled: `FUN_801DE190` (party-display rebuild - the engine
    /// derives the display from `party_actor_slots`), the grid re-anchor
    /// pair `FUN_801DE3E0`/`FUN_801DB8EC`/`FUN_801DAA50` (the engine camera
    /// follows the player actor), and the `FUN_8003BDE0` spawn-condition
    /// re-check at the new tile.
    pub(crate) fn tick_three_actor_talk(&mut self) {
        use crate::cutscene_script_elements::{
            LEADER_ACTOR_POSE_SENTINEL, LEADER_SWAP_REQUEST_BIT, LeaderSwapEffect, LeaderSwapWorld,
        };
        let Some(mut talk) = self.dialog.three_actor_talk else {
            // No live talk: a stale switch request must not outlive the
            // session that could consume it.
            self.dialog.talk_switch_requested = false;
            return;
        };
        let request = if talk.swap.phase == 0 {
            // Retail's own source (`_DAT_8007B874 & 0x80` = Square, newly
            // pressed) plus the scripted latch. `take` runs first and
            // unconditionally, so a latch set outside phase 0 is not left to
            // fire into a later swap.
            let latched = core::mem::take(&mut self.dialog.talk_switch_requested);
            latched || self.input.just_pressed(input::PadButton::Square)
        } else {
            false
        };
        let leader = self.party.party_leader_slot.unwrap_or(0).min(2);
        let swap_world = LeaderSwapWorld {
            leader,
            // Host substitution for the retail suppressor pair
            // `_DAT_8007B6B4` / `_DAT_8007B6B0` (un-pinned globals): no
            // switch arms while a dialogue owns the pad.
            suppress_a: i32::from(self.dialogue_owns_input()),
            suppress_b: 0,
            // The engine models no second controller that could hold the
            // camera-busy bit at state 0, and no `_DAT_1F800394` pad-word
            // mirror: both alternate arm routes read clear, leaving the
            // request-byte route.
            camera_flags: 0,
            pad: 0,
            request_byte: if request { LEADER_SWAP_REQUEST_BIT } else { 0 },
        };
        let step = self.clock.frame_step.max(1);
        // Same MSB-first bank layout as `Self::system_flag_test` (the SCUS
        // helper `FUN_8003CE64` the controller calls).
        let flags = &self.flags.system_flags;
        let tick = talk.swap.step(&swap_world, step, |idx| {
            let byte = (idx >> 3) as usize;
            flags
                .get(byte)
                .is_some_and(|b| b & (0x80u8 >> (idx & 7)) != 0)
        });
        if talk.swap.phase == 5 {
            // State-0 poll saw the talk lock down: despawn (retail runs the
            // state-5 body one frame later; the engine folds it).
            self.dialog.three_actor_talk = Some(talk);
            self.end_three_actor_talk();
            return;
        }
        let ids = talk.actor_ids;
        for effect in &tick.effects {
            match *effect {
                LeaderSwapEffect::CachePartyPoses => {
                    // Retail rewrites the 0x800845E4 table from the three
                    // controller actors every state-0 frame; a participant
                    // the engine has no live NPC record for keeps its
                    // arm-time capture.
                    for (i, &id) in ids.iter().enumerate() {
                        let slot = self.talk_participant_slot(id);
                        if let Some(&pos) = self.npcs.positions.get(&slot) {
                            let heading = self.npcs.heading(slot);
                            talk.saved[i] = Some((pos, heading));
                        }
                    }
                }
                LeaderSwapEffect::SpawnFadeOut => {
                    // `801d29c8..801d2a00`: kind 2, 0x20 frames, black ->
                    // white, no start delay, a `-1` hold - the landed white
                    // persists until the state-2 fade-in replaces it.
                    self.presentation.fade =
                        Some(crate::fade::FadeState::load(&crate::fade::FadeTemplate {
                            kind: 2,
                            duration: crate::cutscene_script_elements::LEADER_SWAP_FADE_FRAMES
                                as i16,
                            start_rgb: [0, 0, 0],
                            end_rgb: [0xFF, 0xFF, 0xFF],
                            mode: [0, -1, 0],
                        }));
                }
                LeaderSwapEffect::StoreOutgoingPose { slot } => {
                    // The outgoing leader's participant actor takes the
                    // player's pose (retail: camera `+0x14/16/18/26` onto
                    // the outgoing actor, `801d2a54..801d2ab8`).
                    if let Some((px, pz)) = self.player_field_position() {
                        let heading = self
                            .player_actor_slot
                            .and_then(|s| self.actors.get(s as usize))
                            .map(|a| a.move_state.render_26)
                            .unwrap_or(0);
                        let npc = self.talk_participant_slot(ids[usize::from(slot.min(2))]);
                        self.npcs.positions.insert(npc, (px, pz));
                        self.npcs.headings.insert(npc, heading);
                    }
                }
                LeaderSwapEffect::CommitLeader { slot } => {
                    // `801d2ae8..801d2b1c`: leader byte + collapsed id list
                    // re-point at the incoming slot; flags 0x10..=0x12
                    // cleared, `0x10 + slot` set.
                    self.party.party_leader_slot = Some(slot);
                    self.party.party_actor_slots = vec![Some(slot)];
                    self.system_flag_clear(0x10);
                    self.system_flag_clear(0x11);
                    self.system_flag_clear(0x12);
                    self.system_flag_set(0x10 + u16::from(slot));
                }
                LeaderSwapEffect::RefreshParty => {
                    // Retail `FUN_801DE190` rebuilds the party display; the
                    // engine's display derives from `party_actor_slots`.
                }
                LeaderSwapEffect::RecentreCamera { slot } => {
                    // The player takes the incoming participant's pose
                    // (`801d2b3c..801d2c04`), and the map origin follows
                    // (`_DAT_80089118/20` = negated pose).
                    let npc = self.talk_participant_slot(ids[usize::from(slot.min(2))]);
                    if let Some(&(nx, nz)) = self.npcs.positions.get(&npc) {
                        let heading = self.npcs.heading(npc);
                        let ny = self.sample_field_floor_height(i32::from(nx), i32::from(nz));
                        if let Some(a) = self
                            .player_actor_slot
                            .and_then(|s| self.actors.get_mut(s as usize))
                        {
                            a.move_state.world_x = nx;
                            a.move_state.world_y = ny as i16;
                            a.move_state.world_z = nz;
                            a.move_state.render_26 = heading;
                        }
                        self.terrain.map_origin_xz = (-i32::from(nx), -i32::from(nz));
                        // `FUN_80017EC8(x >> 7, z >> 7, 0, 0)` at `0x801D2BC0`:
                        // the camera re-centre on the new leader's tile, which
                        // re-plans the windowed static-object list.
                        // REF: FUN_80017EC8
                        self.recentre_field_window(i32::from(nx) >> 7, i32::from(nz) >> 7);
                    }
                }
                LeaderSwapEffect::ClearIncomingPose { slot } => {
                    // `801d2c0c..801d2c20`: the incoming leader's actor is
                    // parked at the 0x3F80 sentinel (the player object now
                    // represents them).
                    let npc = self.talk_participant_slot(ids[usize::from(slot.min(2))]);
                    self.npcs.positions.insert(
                        npc,
                        (LEADER_ACTOR_POSE_SENTINEL, LEADER_ACTOR_POSE_SENTINEL),
                    );
                }
                LeaderSwapEffect::SpawnFadeIn => {
                    // `801d2c24..801d2c54`: kind 2, 0x20 frames, white ->
                    // black.
                    self.presentation.fade =
                        Some(crate::fade::FadeState::load(&crate::fade::FadeTemplate {
                            kind: 2,
                            duration: crate::cutscene_script_elements::LEADER_SWAP_FADE_FRAMES
                                as i16,
                            start_rgb: [0xFF, 0xFF, 0xFF],
                            end_rgb: [0, 0, 0],
                            mode: [0, 0, 0],
                        }));
                }
                LeaderSwapEffect::ReleaseFadeObject | LeaderSwapEffect::ClearCameraBusy => {
                    // The engine fade steps + drops itself
                    // ([`crate::world::ScreenFxState::fade`]); no modelled camera flag word
                    // to clear.
                }
                LeaderSwapEffect::RetireController => {
                    self.dialog.three_actor_talk = Some(talk);
                    self.end_three_actor_talk();
                    return;
                }
            }
        }
        self.dialog.three_actor_talk = Some(talk);
    }

    /// Resolve a talk-instruction participant id to the engine's field-NPC
    /// placement slot - the same actor-list walk the op-`0x43` sub-2 arm
    /// performs (retail `FUN_8003C83C`); an unmatched id passes through raw.
    // REF: FUN_8003C83C (id resolve)
    fn talk_participant_slot(&self, id: u8) -> u8 {
        let view = self.channel_view();
        crate::field_channels::resolve_target(view, id)
            .map(|ci| view[ci].placement_index as u8)
            .unwrap_or(id)
    }

    /// End the active three-actor talk: drop the session, clear the talk
    /// lock (system flag `0xD`, idempotent when the script already cleared
    /// it - that clear is what [`Self::tick_three_actor_talk`] fires on),
    /// and restore the story party count + leader from the pre-collapse
    /// snapshot the op-`0x43` arm captured.
    ///
    /// Retail rebuilds the post-talk party through the scene script's own
    /// party ops (the field VM's `0x80084594/98` writers); the controller
    /// itself only despawns (`FUN_801D27E0` state 5). The engine restores
    /// the arm-time snapshot at the same trigger so the collapse is never a
    /// one-way door when a script ends the talk without explicit re-adds -
    /// script party ops that do follow still apply on top (`party_add`
    /// dedupes members already present, `party_remove` prunes), so a script
    /// that rebuilds the same trio converges to the identical end state.
    ///
    /// No-op without an active session.
    // REF: FUN_801D27E0 (state-5 despawn), FUN_801D2D38 (the collapse this undoes)
    pub fn end_three_actor_talk(&mut self) {
        let Some(talk) = self.dialog.three_actor_talk.take() else {
            return;
        };
        self.system_flag_clear(0xD);
        if talk.saved_party_len > 0 {
            self.party.party_actor_slots =
                talk.saved_party[..talk.saved_party_len as usize].to_vec();
            self.party.party_leader_slot = talk
                .saved_leader
                .or_else(|| self.party.party_actor_slots.first().copied().flatten());
            // A party op inside the talk re-installed the battle
            // composition from the collapsed list; the restored list is the
            // one the next battle reads.
            let list = self.present_party_list();
            self.install_present_party_list(list);
        }
    }

    /// Drain a menu-staged Door use into the pause-menu session's travel-art
    /// hand-off ([`Self::begin_pause_session_exit`]), which runs the art and
    /// then issues the named scene transition the scene host consumes
    /// ([`Self::pending_named_scene_transition`]).
    ///
    /// **Door of Wind** ([`crate::world::MenuState::pending_warp`]): the staged triple is
    /// retail's `0x80084628` scene word + `0x80084624`/`0x8008462C` tile
    /// pair (`FUN_801D8B90` phase 3, from quick-travel placement record
    /// bytes `+2/+4/+5`). The scene word is the destination scene's raw
    /// CDNAME TOC index ([`crate::world::DiscTables::scene_toc_names`]), the
    /// same key the art's resolve looks up in the resident define table; the
    /// tile pair seats the party at `(tile << 7) + 0x40`. Exit code `5`, so
    /// the session hands on to Rula (`FUN_801EE328`).
    ///
    /// **Door of Light** ([`crate::world::MenuState::pending_escape`]): exit
    /// code `4` (`FUN_801D8A58`), so the session hands on to Riremito
    /// (`FUN_801EE094`). The three words then hold what the last long-layout
    /// region record the player stood in stored (`region[+9..+0xB]`,
    /// [`crate::region_encounter::WorldMapReturn`]): a retail capture of a
    /// Door of Light in cave01 reads `0x55 @ (37, 109)` there and seats the
    /// party at `(37 << 7) + 0x40, (109 << 7) + 0x40` on map01. Only when no
    /// region has stored a triple yet (an engine-only state: a scene with no
    /// region table, or a use before the first step) does the drain fall back
    /// to the world-map panel host's last recorded map.
    ///
    /// A target that does not resolve logs retail's `UNFIND MAP NUMBER %d`
    /// diagnostic and drops the use before anything is installed - see
    /// [`crate::world::pause_session`] for why that differs from retail.
    ///
    /// REF: FUN_801D8B90 (stage), FUN_801D8A58 (escape exit code),
    /// FUN_801ED308 (the session), FUN_801EE094 / FUN_801EE328 (the arts)
    pub fn drain_staged_menu_warp(&mut self) {
        use crate::pause_screens::{MENU_EXIT_CODE_FIELD_ESCAPE, MENU_EXIT_CODE_WORLD_MAP_WARP};
        use crate::world::pause_session::PauseTravelTarget;
        if let Some(warp) = self.menu.pending_warp.take() {
            match travel_scene(&self.tables.scene_toc_names, u32::from(warp.scene_id)) {
                Some(name) => {
                    let target = PauseTravelTarget {
                        scene: name,
                        tile_x: warp.menu_x,
                        tile_z: warp.menu_y,
                    };
                    self.begin_pause_session_exit(MENU_EXIT_CODE_WORLD_MAP_WARP, target);
                }
                None => {
                    // Retail's miss arm prints and parks (phase 0x63).
                    log::warn!("menu warp: UNFIND MAP NUMBER {}", warp.scene_id);
                }
            }
        }
        if self.menu.pending_escape {
            self.menu.pending_escape = false;
            // The menu's installer refreshes the region setup before the
            // menu opens (`FUN_801F1278` calls `FUN_801D9E1C(player, 0)` at
            // `0x801F12F8`, with `+0x8E` / `+0x8F` preset to `0xFF`); the
            // player cannot move between that open and this drain, so the
            // refresh here reads the same tile.
            self.apply_region_battle_setup_at_player();
            // Retail's resolve reads the triple the last long-layout region
            // record stored (`FUN_801D9E1C`); the capture has cave01 leave
            // `0x55 @ (37, 109)` - the tile outside the cave mouth, not the
            // tile the party entered from - and the art seats the party there.
            if let Some(ret) = self
                .encounters
                .region_setup
                .and_then(|s| s.world_map_return)
            {
                match travel_scene(&self.tables.scene_toc_names, u32::from(ret.map_word)) {
                    Some(name) => {
                        let target = PauseTravelTarget {
                            scene: name,
                            tile_x: ret.tile_x,
                            tile_z: ret.tile_z,
                        };
                        self.begin_pause_session_exit(MENU_EXIT_CODE_FIELD_ESCAPE, target);
                    }
                    None => {
                        log::warn!("menu escape: UNFIND MAP NUMBER {}", ret.map_word);
                    }
                }
                return;
            }
            let visited = self
                .world_map
                .ctrl
                .as_ref()
                .and_then(|c| c.panels.visited.last().copied());
            match visited {
                Some(v) => {
                    let name = match v.map_id {
                        0 => "map01",
                        1 => "map02",
                        2 => "map03",
                        _ => {
                            log::warn!("menu escape: UNFIND MAP NUMBER {}", v.map_id);
                            return;
                        }
                    };
                    let target = PauseTravelTarget {
                        scene: name.to_string(),
                        tile_x: v.tile_x.clamp(0, 0xFF) as u8,
                        tile_z: v.tile_z.clamp(0, 0xFF) as u8,
                    };
                    self.begin_pause_session_exit(MENU_EXIT_CODE_FIELD_ESCAPE, target);
                }
                None => {
                    log::warn!("menu escape: no visited world-map record to return to");
                }
            }
        }
    }

    /// Install the CDNAME `#define` map (raw TOC index → block name) the
    /// menu-warp drain resolves a quick-travel `scene_id` against. The
    /// scene host wires this once at construction from the same parsed
    /// `CDNAME.TXT` its own scene loads use.
    pub fn install_scene_toc_names(&mut self, map: legaia_prot::cdname::IndexMap) {
        self.tables.scene_toc_names = map;
    }

    /// Arm the timed sound-source auto-release for `deadline` vsyncs
    /// (`gp+0x814`). [`Self::tick`] counts it down by the frame step.
    ///
    /// Retail's arm half writes five `gp` cells, not three: on top of the
    /// timer's armed flag (`gp+0x808`) / deadline (`gp+0x814`) / elapsed
    /// (`gp+0x81C`) it latches the caller's tag (`gp+0x810`) and the live
    /// **audio level** `_DAT_8007B910` (`lw a1,-0x46f0(a1)` at `0x800267B0`
    /// into `gp+0x80C`), then tail-calls the libsnd volume shim
    /// `FUN_80062004(*(i16*)0x80070536, (level << 15) >> 16, deadline | 1)`
    /// (`0x800267E4`). Those two extra cells land in [`crate::world::AudioState::sound_arm`] so a
    /// host driving the shim has the exact arguments; the engine has no live
    /// field-mode volume ramp of its own, so the latched level is the value
    /// retail's MAN loader rests `_DAT_8007B910` on at every scene load - the
    /// configured level `_DAT_8008457C` ([`crate::world::AudioState::levels`]),
    /// `0xD7` from a cold boot and a loaded save's own word after a load.
    ///
    /// PORT: FUN_800267A8
    /// REF: FUN_800267FC, FUN_80062004
    pub fn arm_sound_release(&mut self, deadline_vsyncs: i32) {
        self.audio.sound_release.arm(deadline_vsyncs);
        self.audio.pending_sound_release = false;
        self.audio.sound_arm = Some(crate::scus_leaf_kernels::TimedSoundArm::arm(
            0,
            deadline_vsyncs.max(0) as u32,
            self.audio.levels.configured_level,
        ));
    }

    /// Drain the "the sound-release deadline expired" event.
    pub fn take_pending_sound_release(&mut self) -> bool {
        std::mem::take(&mut self.audio.pending_sound_release)
    }

    /// Run the one-shot sound setup `FUN_8002689C`. Returns `true` only on
    /// the first call - retail's `gp+0x804` latch gates every later one out,
    /// which is why the mode-INIT chain can call it freely.
    ///
    /// Nothing is detached: behind the latch retail makes two volume calls,
    /// the cmd-6 SPU command `FUN_80065440(0x32, 0x32)` and the master
    /// volume `SsSetMVol` `FUN_80062AA0(0x7F, 0x7F)`. This port models the
    /// latch only; the two volume writes are left to the audio host's own
    /// defaults. (The function keeps the name an earlier reading gave it.)
    ///
    /// PORT: FUN_8002689c
    pub fn detach_sound(&mut self) -> bool {
        self.audio.sound_detach.detach()
    }

    /// Consume the frame-begin skip request, returning whether this frame
    /// should be abandoned. Models `FUN_8001698C`'s non-zero return; see
    /// [`crate::world::FrameClock::frame_begin_skip`].
    ///
    /// PORT: FUN_8001698c (the frame-skip return; the ring-aging half of the
    /// same function is `legaia_engine_audio::sfx_ring::SfxCueRing::age`)
    pub fn take_frame_begin_skip(&mut self) -> bool {
        std::mem::take(&mut self.clock.frame_begin_skip)
    }

    /// Arm the scripted countdown the field VM installs with `0x4C 0xD3`
    /// (`SCHEDULE_TIMED_FLAGS`).
    ///
    /// The three operands are the ones the installer writes into its four
    /// globals (`0x801E2BDC..0x801E2C30`): `ab` is the packed flag word
    /// `_DAT_800845C0` (high half = expiry flag, low half = below-threshold
    /// flag), `cd` is the duration - stored into **both** `_DAT_800845B8`
    /// (the armed word) and `_DAT_800845A0` (the live counter) - and `ef` is
    /// the below-threshold trigger point `_DAT_800845BC`. A zero duration
    /// leaves the timer disarmed, which is what retail's `_DAT_800845B8 != 0`
    /// arm test resolves to.
    ///
    /// Retail also snapshots the play clock into `_DAT_80073ED4` here so the
    /// first drain sees a zero delta; the engine's drain takes its delta from
    /// the retail-frame sub-clock instead, so there is no latch to seed.
    ///
    /// REF: FUN_801DE840 case 0xD sub 3 (the installer)
    pub fn schedule_timed_flags(&mut self, ab: u32, cd: u32, ef: u32) {
        self.battle.escape_timer_flag_word = ab;
        self.battle.escape_timer = vm::escape_timer::EscapeTimer {
            remaining: cd as i32,
            warn_threshold: ef as i32,
            armed: cd != 0,
        };
        self.battle.escape_timer_hud = None;
        // A zero duration marks a live HUD actor for teardown; otherwise a
        // live one is kept (with its phase) and a missing one is spawned on
        // the next tick (the spawner `0x801D596C` checks for the actor by
        // handler before allocating).
        if cd == 0 {
            self.battle.escape_timer_actor = None;
        }
    }

    /// Whether this frame is one of the ones retail's timed-flag scheduler
    /// sits out. Retail short-circuits on three conditions at
    /// `0x801D2EBC..0x801D2F30`, and a busy frame refreshes the clock latch
    /// without touching the counter:
    ///
    /// 1. `*(_DAT_8007C364 + 0x10) & 0x80000` - the **player actor's engaged
    ///    bit**, the one `FUN_801D5B5C` sets on a touch/talk and the one that
    ///    also suppresses locomotion input at the head of `FUN_801D01B0`. The
    ///    engine's analogue is an open modal dialog.
    /// 2. `_DAT_8007B6B4 != 0` - a **dialogue-pacing countdown**: the typing
    ///    driver `FUN_801D1344` drains it by the frame step and clamps it at
    ///    zero (`0x801D161C..0x801D1630`), so a non-zero value means a text
    ///    beat is still running. Folded into the same modal-dialog test.
    /// 3. `_DAT_8007B6B0 > 0` - the **kind-0 warp timer** of the walk-on
    ///    dispatcher `FUN_801D1EC4`, which counts the `0x26` frames between a
    ///    door-tile crossing and the landing
    ///    ([`crate::world::World::field_warp_in_flight`]). The mode test
    ///    covers the frames where the field is not what is being driven at
    ///    all (a menu, a battle, a minigame).
    fn escape_timer_busy(&self) -> bool {
        self.dialogue_owns_input()
            || self.field_warp_in_flight()
            || !matches!(self.mode, SceneMode::Field | SceneMode::Cutscene)
    }

    /// Drain the scripted countdown one retail frame and fire whichever
    /// system flags the tick reaches, then refresh the HUD readout.
    ///
    /// Retail's `FUN_801D2EBC` is one function that does all three: it
    /// subtracts the play-clock delta from `_DAT_800845A0`, calls
    /// `func_0x8003CE08(flag & 0xFFF)` for the expiry flag at zero (disarming
    /// the timer) and for the below-threshold flag under `_DAT_800845BC`,
    /// then decomposes the remaining count into MM:SS.ff and picks the
    /// readout ink from it. The decomposition and ink are therefore products
    /// of the tick, not of a renderer - [`crate::world::BattleState::escape_timer_hud`] caches
    /// this frame's.
    ///
    /// The delta is one retail frame per call (the caller gates on
    /// [`crate::world::FrameClock::display_frame_step`]); retail reads it as
    /// `_DAT_80084570 - _DAT_80073ED4`, a clock that also advances one step
    /// per display frame.
    ///
    /// REF: FUN_801D2EBC (scheduler + HUD decomposition; the ports are
    /// `legaia_engine_vm::escape_timer::EscapeTimer` and `timer_ink`)
    fn tick_escape_timer(&mut self) {
        // The countdown runs for exactly as long as its HUD actor lives: the
        // arming op spawns it, and after expiry it holds a zeroed readout
        // for `EXPIRED_HOLD` frames before killing itself.
        if self.battle.escape_timer_actor.is_none() {
            if !self.battle.escape_timer.armed {
                self.battle.escape_timer_hud = None;
                return;
            }
            self.battle.escape_timer_actor = Some(vm::escape_timer::EscapeTimerHud::default());
        }
        let busy = self.escape_timer_busy();
        let flag_word = self.battle.escape_timer_flag_word;
        let events = self.battle.escape_timer.tick(1, flag_word, busy);
        if let Some(flag) = events.expiry_flag {
            self.system_flag_set(flag);
        }
        if let Some(flag) = events.warning_flag {
            self.system_flag_set(flag);
        }
        if busy {
            // Retail's busy frame returns before the phase machine and the
            // draw; the port keeps the last readout up.
            return;
        }
        let Some(mut actor) = self.battle.escape_timer_actor else {
            return;
        };
        let frame = actor.step(&mut self.battle.escape_timer, 1);
        if frame.teardown {
            self.battle.escape_timer_actor = None;
            self.battle.escape_timer_hud = None;
            return;
        }
        self.battle.escape_timer_actor = Some(actor);
        let (minutes, seconds, hundredths) = frame.digits;
        self.battle.escape_timer_hud = Some((minutes, seconds, hundredths, frame.ink));
    }

    /// Resolve this frame's cadence the way `FUN_80016B6C` does and install
    /// it into [`crate::world::FrameClock::frame_step`].
    ///
    /// PORT: FUN_80016b6c (the `0x80017044 .. 0x800171D8` cadence block; the
    /// telemetry state machine lives in
    /// [`legaia_engine_vm::actor_tick::FrameStepTelemetry`]).
    ///
    /// `elapsed_hblanks` is the frame time retail samples with `VSync(1)`
    /// through `FUN_800173BC`. The floor is [`crate::world::FrameClock::frame_step_floor`]
    /// (`DAT_8007B9D8`), installed per scene, and the resolver can only raise
    /// the cadence above it - never below.
    ///
    /// **Hosts that want determinism should not call this.** Retail gates the
    /// whole adaptive path on a boot config word (`gp+0x4CE == 0x10`); with
    /// `frameskip_enabled = false` this returns the floor unchanged, which is
    /// exactly what the replay / trace oracles need. Wall-clock-paced hosts
    /// pass their measured frame time and `true`.
    ///
    /// REPLACED-BY: the fixed-rate simulation plus the scene-installed floor
    /// [`crate::world::FrameClock::frame_step_floor`], which is what this
    /// resolves to on every frame that keeps up with vsync.
    ///
    /// The adaptive term is the running maximum of the last sixteen
    /// `VSync(1)` hblank samples against thresholds just under one, two and
    /// three NTSC fields, so it only rises above `1` on a frame the renderer
    /// missed. The engine's simulation never misses one - its tick is
    /// scheduled, not measured - so the resolver's answer is the floor the
    /// scene loader already installed, and nothing samples `FUN_800173BC`'s
    /// hblank count to ask it otherwise. The replay / trace oracles rely on
    /// that: a wall-clock-fed cadence would make them non-deterministic.
    pub fn resolve_frame_step(&mut self, elapsed_hblanks: i32, frameskip_enabled: bool) -> u8 {
        let cadence = self.clock.frame_step_telemetry.resolve(
            elapsed_hblanks,
            frameskip_enabled,
            self.clock.frame_step_floor,
        );
        self.clock.frame_step = cadence.vsyncs_per_tick();
        self.clock.frame_step
    }

    /// The `VSync(n)` argument retail would pass this frame - **last** frame's
    /// cadence, with `< 2` passed as `0` (`0x8001719C`, read before the new
    /// value is written back at `0x800171D8`).
    ///
    /// REF: FUN_80016B6C
    pub fn frame_step_vsync_wait(&self) -> u8 {
        self.clock.frame_step_telemetry.vsync_wait()
    }

    /// Increment the deterministic LCG and return the new **raw** state.
    ///
    /// Not a retail draw: a port of a `jal 0x80056798` site goes through
    /// [`Self::next_rand`]. The raw word's low bits have short periods.
    pub fn next_rng(&mut self) -> u32 {
        // Numerical Recipes LCG. Cheap, deterministic.
        self.rng_state = legaia_engine_vm::battle_formulas::world_lcg_step(self.rng_state);
        self.rng_state
    }

    /// One retail **`rand()`** draw: the next [`Self::next_rng`] state shaped
    /// the way BIOS `A(2Fh)` (`FUN_80056798`) shapes its own, `0..=0x7FFF`
    /// ([`legaia_engine_vm::battle_formulas::bios_rand_shape`]).
    ///
    /// A port that stands in for a `jal 0x80056798` should draw through this
    /// rather than the raw state: retail callers test the result's low bits
    /// and divide it as a 15-bit value. Every battle-side consumer draws
    /// here (see `docs/subsystems/battle-formulas.md`, RNG primitive). Retail's `rand()` has one
    /// kernel seed shared by every caller in SCUS and every overlay (the
    /// executable carries no `srand` thunk), which is why this is one world
    /// stream rather than a generator per subsystem.
    pub fn next_rand(&mut self) -> u32 {
        legaia_engine_vm::battle_formulas::bios_rand_shape(self.next_rng())
    }

    /// Replace the per-frame pad bitmask snapshot. Equivalent to
    /// `self.input.set_pad(mask)` but available without importing
    /// [`input::InputState`] at the call site. Hosts that drive the
    /// world from a scripted timeline (`legaia-engine replay`, the
    /// v0.1 playthrough oracle) call this before each [`Self::tick`].
    /// Also latches [`crate::world::FieldLocomotion::run_button_held`] off the same word, so the
    /// run modifier reaches every host that feeds a pad - native window,
    /// browser play page, replay driver - without any of them wiring it
    /// separately. Deriving it here rather than per-host is deliberate: a
    /// per-host derivation is exactly the shape the UI-drift gate exists to
    /// catch, and this way there is nothing to keep in sync.
    ///
    /// The buttons are [`crate::world::FieldLocomotion::run_button_mask`], which **defaults to
    /// retail's** `Cross | R1` (the config word `0x800846DC` = `0x48`, seeded
    /// by `FUN_80034A6C` and read at `0x801D0364`), plus Square as an
    /// alternate. Square alone was the port's binding for a while and is not
    /// retail's - it is the debug-turbo bit `0x80` on the same selector - so
    /// the default now leads with the retail pair. Rebinding is a *key*
    /// question, not a button one: `legaia-engine config set --binding
    /// W=R1` moves which key produces R1.
    ///
    /// REF: FUN_80034A6C
    pub fn set_pad(&mut self, mask: u16) {
        self.input.set_pad(mask);
        self.locomotion.run_button_held = mask & self.locomotion.run_button_mask != 0;
    }

    /// Per-frame world tick. Drives whichever scene-mode VMs are live.
    /// Returns the battle-step outcome when in [`SceneMode::Battle`], else
    /// `None`.
    ///
    /// Order of operations:
    ///  1. Effect pool tick - the faithful retail walker, run on the
    ///     ~60 Hz retail-frame sub-clock regardless of mode.
    ///  2. Per-actor move-VM tick - only for actors with bytecode loaded.
    ///  3. Per-actor physics tick (`FUN_80021DF4`) - drains timer,
    ///     advances motion, kicks the move-buffer cursor on
    ///     [`TickEvent::MoveVmKick`]. Runs over every active actor.
    ///  4. Per-actor keyframe / anim-player tick.
    ///  5. Mode-specific VM:
    ///     - `Battle`     → battle-action state machine step.
    ///     - `Field`      → field-VM step (or no-op if no bytecode loaded).
    ///     - `Cutscene`   → field-VM step (cutscenes use the same script VM).
    ///     - `Title`      → no further VM.
    ///
    /// This is the engine's counterpart of the retail master frame driver
    /// `FUN_80016444`: retail runs five `FUN_8002519C` **tick passes** over
    /// the actor-list heads `_DAT_8007C34C..0x36C` (pass 3/4 swapped by the
    /// `_DAT_1F800394 & 0x10` mirror bit), then five `FUN_8001D140` **render
    /// passes** (the scratchpad-SP trampoline into `FUN_8001ADA4`), with the
    /// display flip (`FUN_8001D058` → `FUN_80026CE4`) before the render
    /// passes in STR mode (0x15) and after them otherwise, plus dev error
    /// prints and the dev mode-transition writer `FUN_800179C0`. The engine
    /// splits that frame: the tick passes are the per-actor loops below,
    /// the render passes live in the host renderer (wgpu), the flip is the
    /// swapchain present, and the dev prints are not ported. Divergence:
    /// engine actors live in one pool with an `active` flag, not five
    /// linked lists - pass ORDER is preserved by the sequencing below.
    // PORT: FUN_80016444 (frame-pass sequencing; render/flip halves are the
    //                     host renderer's, dev prints not ported)
    pub fn tick(&mut self) -> Option<StepOutcome> {
        // A pinned encounter entry ([`EncounterState::rng_hold`]).
        if let Some(seed) = self.encounters.rng_hold {
            if self.mode == SceneMode::Battle {
                self.encounters.rng_hold = None;
            } else {
                self.rng_state = seed;
            }
        }
        // The move-VM strip set on screen is one tick's
        // (`MoveVmGlobals::strip_frame`).
        self.move_vm.begin_strip_tick();
        let outcome = self.tick_modes();
        // The battle camera observes the frame this tick produced, in every
        // mode (outside battle it drops its state) - once, here, for every
        // host (`crate::battle_cam_inputs`).
        self.tick_battle_camera();
        // The near-camera ghost pass reads the pose this tick settled
        // (`FUN_80046A20` calls `FUN_8004DC68` after its camera update).
        self.tick_battle_camera_ghost();
        // The volumetric ground-fog enhancement steps on the tick this frame
        // settled, after every actor moved (`crate::fog_volume`).
        self.tick_fog_volume();
        outcome
    }

    fn tick_modes(&mut self) -> Option<StepOutcome> {
        self.frame += 1;
        // Does retail run the master frame driver on a frame in this mode?
        //
        // Every per-frame mode handler except one calls `FUN_80016444`, and
        // that call is what advances the actor pool at all - the five
        // `FUN_8002519C` tick passes over `_DAT_8007C34C..0x36C`. The exception
        // is mode 23 CARD, the mode the in-field pause menu runs under
        // (`_DAT_8007B83C = 0x17` in every menu-open capture): its handler
        // `FUN_80025F74` calls `FUN_80017978` instead, and that body is 18
        // instructions with three `jal`s, none of them `0x80016444`
        // (`0x80017978..0x800179BC`). So while the menu owns the frame retail
        // advances **no** actor, effect, move VM or animation; the CARD actor's
        // own `+0x0C` handler is the entire frame.
        //
        // Resolved through the ported mode table rather than spelled out as a
        // `SceneMode::Menu` literal here, so the rule and its provenance cannot
        // drift apart: [`crate::mode::runs_master_frame_driver`] reads it off
        // the same `per_frame_stage` rows the mode-table oracles use.
        //
        // What still runs under the menu is retail's too, and is *outside* the
        // master driver: the CARD handler's frame-begin pass `FUN_8001698C`
        // (which is where the timed sound-source auto-release `FUN_800267FC`
        // lives) and its frame-end pass `FUN_80016B6C` (the cadence resolver
        // and the SFX cue ring) both run unconditionally - `FUN_80017978`
        // returns `move v0,zero`, so the handler's abort branch never fires.
        //
        // **Which consumers this gate actually moves**, stated plainly because
        // it is easy to over-claim. Neither shipped host reaches it with the
        // pause menu open: the native window `continue`s past `session.tick()`
        // while its boot-UI owns the frame, and the browser page's sim loop is
        // gated `if (advance && !menuOpen && ...)`. Both therefore freeze
        // *more* than retail does - they stop the frame-begin / frame-end
        // passes too, which retail keeps running under CARD. This gate is what
        // makes the correct split available: a host can now tick the world
        // under the menu and get retail's behaviour instead of choosing between
        // "everything runs" and "nothing runs". Today it is exercised by the
        // headless `World::tick` consumers (the replay / determinism / mode
        // trace drivers, and any future owner of `ModeDriver`).
        // REF: FUN_80025f74, FUN_80017978, FUN_80016444
        let runs_master_driver = crate::mode::GameMode::for_scene_mode(self.mode)
            .map(crate::mode::runs_master_frame_driver)
            .unwrap_or(true);
        // Bridge the vsync-rate pad to the game-tick-rate actor pool for the
        // op-0x49 submode screens: the hosts publish a pad word every tick and
        // the dispatcher runs every `frame_step` ticks, so without this the
        // half of the edges that land on a skipped pass are lost outright.
        // See `SubmodeScreen::pad_edge_latch`.
        self.latch_submode_pad_edge();
        // Age the post-battle spoils panel (armed by `finish_battle`) and the
        // no-encounters-here hint (armed by `arm_live_loop`).
        self.battle.spoils_frames = self.battle.spoils_frames.saturating_sub(1);
        self.encounters.scene_hint_frames = self.encounters.scene_hint_frames.saturating_sub(1);
        // ------------------------------------------------------------------
        // The simulation clock's denomination.
        //
        // **One `World::tick` is exactly one retail display frame (vsync).**
        // Both hosts drive it that way through one fixed-timestep kernel
        // (`crate::frame_step::SimStepper`, `TICK_SECS = 1/60`, backlog
        // capped at four ticks). So `SIM_HZ == RETAIL_FPS`, the retail-frame sub-clock is
        // an identity - `field_frame_step` is `1` on every tick and
        // `field_frames == frame` - and the gates below are statements of
        // which consumers are retail-frame paced rather than rate changes.
        //
        // Retail's frame has TWO clocks and `DAT_1F800393` relates them.
        // `FUN_80016B6C` resolves that byte once per frame
        // (`0x80017044..0x800171D8`: sample the frame time with `VSync(1)`
        // through `FUN_800173BC`, pick 1..4, raise it to the per-mode floor
        // `DAT_8007B9D8`, then `VSync(n)`-wait), so one pass of the master
        // driver `FUN_80016444` spans `DAT_1F800393` vsyncs - the **game
        // tick**. Every duration and every velocity inside that pass is
        // denominated in vsyncs and scaled by the byte, which makes the
        // wall-clock rates cadence-invariant:
        //
        //  * player locomotion `FUN_801D01B0` - called **once per game tick**
        //    from the field frame pump `FUN_801D1344` (`jal 0x801D01B0` at
        //    `0x801D16F4`, with no cadence gate of its own), and its travel
        //    budget for the call is
        //    `((base_step * player[+0x72]) >> 12) * DAT_1F800393`
        //    (`0x801D0564..0x801D05C4`: `mult s4,v0`; `sra s4,t1,0xc`;
        //    `lbu v1,0x7f(a1)` with `a1 = 0x1F800314`; `mult s4,v1`).
        //  * field-NPC motion `FUN_8003774C` - the same shape
        //    (`0x80037868 lbu s2,0x393(s2)`, then `mult ...,s2` into every
        //    glide leg).
        //  * the frame pump's own countdowns - the dialogue-pacing timer
        //    `_DAT_8007B6B4` (`0x801D1618..0x801D1630`) and the field-control
        //    byte `+0x62` (`0x801D1670..0x801D1690`) each subtract
        //    `DAT_1F800393`, not `1`.
        //
        // So retail's player advances `base_step` units per **vsync** whatever
        // the cadence does: at the field floor of 2 it runs the controller
        // 30x a second for `2 * base_step` each. The engine takes the
        // finer-grained half of that identity - the controller once per vsync
        // with the scalar at 1 - which lands on the same `base_step * 60`
        // units per second and merely emits twice as many intermediate poses.
        // Neither half is a place to insert a rate.
        //
        // The `SIM_HZ = 100` this replaces was a premise no host ever met. It
        // withheld 2 of every 5 retail frames from the *gated* consumers (the
        // narration crawl, the cutscene timeline, the effect pool, the escape
        // timer, NPC motion, the CLUT / ambient game-tick banks, the timed
        // sound release) - 36 Hz against retail's 60 - while the *ungated*
        // ones (locomotion, the per-actor field channels, the prop and
        // tile-board layers) ran at the correct 60. The two errors cancelled
        // in the one place anyone measured: `opening_chain_wall_time.rs`
        // divided observed ticks by 100 while the timeline emitted 0.6 retail
        // frames per tick, so the *seconds* came out right and the *unit*
        // stayed wrong.
        //
        // REF: FUN_80016B6C (cadence resolver), FUN_80016444 (master driver),
        //      FUN_801D1344 (field frame pump), FUN_801D01B0, FUN_8003774C
        const RETAIL_FPS: u32 = 60;
        const SIM_HZ: u32 = RETAIL_FPS;
        // A host that ever wants to oversample has to re-derive every
        // consumer below, not just relax this constant.
        const _: () = assert!(
            SIM_HZ == RETAIL_FPS,
            "one sim tick is one retail display frame"
        );
        self.clock.display_frame_step = 1;
        self.clock.display_frames += 1;
        // Kept advancing as the cheap "a world frame ran" witness (the mode
        // driver's frame-begin-skip test probes it); the fixed-point phase it
        // used to carry is gone with the 1:1 denomination.
        self.clock.sim_ticks = self.clock.sim_ticks.wrapping_add(1);
        // Retail game-tick clock for the scripted CLUT-cell effects: one game
        // tick spans `frame_step` vsyncs (the adaptive `DAT_1F800393` factor
        // written by `FUN_80016B6C`; see [`crate::world::FrameClock::frame_step`]). Count the sim
        // ticks that map to a retail vsync and bank a game tick every
        // `frame_step` of them; [`Self::step_clut_fx`] drains the bank
        // against the host's VRAM. Only accumulates while effects are live
        // (capped so an undrained host can't wind up a backlog).
        if self.clock.display_frame_step == 1
            && !(self.ambient.clut_fx.is_empty() && self.ambient.clut_blend_fx.is_empty())
        {
            self.ambient.clut_vsync_accum += 1;
            if self.ambient.clut_vsync_accum >= self.clock.frame_step.max(1) {
                self.ambient.clut_vsync_accum = 0;
                self.ambient.clut_pending_game_ticks =
                    (self.ambient.clut_pending_game_ticks + 1).min(600);
            }
        }
        // Same game-tick law for the ambient move-VM effect parts (jou's
        // CLUT-cell cyclers / lightning director); drained by the host's
        // `step_ambient_fx` against its VRAM.
        if self.clock.display_frame_step == 1 && !self.ambient.fx.is_empty() {
            self.ambient.vsync_accum += 1;
            if self.ambient.vsync_accum >= self.clock.frame_step.max(1) {
                self.ambient.vsync_accum = 0;
                self.ambient.pending_game_ticks = (self.ambient.pending_game_ticks + 1).min(600);
            }
        }
        // The modelled CD drive under a field XA one-shot: one vsync of its
        // read span elapses per world tick (`World::push_field_xa_cue`).
        self.tick_field_xa_busy();
        // Retail's frame-begin driver services the timed sound-source
        // auto-release before anything else in the frame (`FUN_800267FC`,
        // called at `0x800169FC`). Its accumulator advances by the frame step,
        // so drive it on the sim ticks that map to a retail vsync.
        if self.clock.display_frame_step == 1 {
            let step = self.clock.frame_step.max(1);
            // The teardown gates (`record[+8]` active, `_DAT_8007B868`) live
            // in the libsnd voice binding the engine replaces, so the engine
            // arm is "release when it fires" unconditionally.
            if let crate::sound_state::SoundReleaseTick::Fired { .. } =
                self.audio.sound_release.tick(step, true, false)
            {
                self.audio.pending_sound_release = true;
                // What the expiry does is the field-BGM detach, inline: the
                // released record is the BGM slot `0x8007052C`, and the arm
                // at `0x80026828..0x8002686C` is `FUN_800266E0`'s body - pan
                // reset `FUN_8002657C(0, slot)`, `FUN_80064370(slot[+0xA])`,
                // `DAT_8007B708 = 0` - behind the same `_DAT_8007B868` gate.
                // `FUN_800266E0` is BGM sub-op 2's primitive, so the expiry
                // reaches both hosts' BGM routing as that pause.
                //
                // Except inside a sub-op 9 -> 0xA swap: there the slot still
                // holds the *outgoing* track (the poller `FUN_800243F0`
                // stalls its install on the commit), and that is what the
                // release stops - a cutscene's `9 · 5 · 0xA` fades the old
                // score out under the new one's load. The port started the
                // incoming track at sub-op 9, so pausing here would silence
                // the new score for good (the commit then releases the paused
                // source).
                if !self.audio.start_pending_commit {
                    self.pending_field_events
                        .push(crate::field_events::FieldEvent::Bgm {
                            text_id: 0,
                            sub_op: 2,
                        });
                }
            }
        }
        // Step the active full-screen fade. A template with a hold countdown
        // is dropped once the ramp lands (hosts stop drawing the overlay); one
        // whose hold word is `-1` - the battle-end / escape template - keeps
        // its end colour up until the battle teardown clears it
        // (`FadeState::holds_at_end`, `finish_battle`).
        if let Some(fade) = &mut self.presentation.fade
            && !fade.step()
            && !fade.holds_at_end()
        {
            self.presentation.fade = None;
        }
        self.presentation
            .module_fades
            .retain_mut(|f| f.step() || f.holds_at_end());
        // Step the scripted global multiply tint (op `0x4C 0x12`). A ramp
        // that lands on a non-neutral target HOLDS there (a screen faded to
        // black stays black until a new op replaces it); one that lands on
        // the neutral identity is dropped so the render path returns to
        // untouched. The op-`0x34` sub-0 screen effect is **not** a second
        // channel here - it is a pool colour tween emitting
        // `FUN_80024EE4` pushes, stepped by `tick_handler_actors`.
        if let Some(t) = self.presentation.tint.as_mut() {
            t.step();
            if t.is_identity() {
                self.presentation.tint = None;
            }
        }
        // Step an op-`4C 13` clear-colour ramp (its `FUN_8003C5F0` slot jobs).
        if let Some(r) = self.presentation.clear_ramp.as_mut() {
            r.elapsed = r.elapsed.saturating_add(1);
            self.presentation.clear_rgb = r.value();
            if r.elapsed >= r.total {
                self.presentation.clear_ramp = None;
            }
        }
        // Consume a pending FMV transition the field VM signalled last frame
        // (op `0x4C 0xE2`). Retail's main mode dispatcher reads the
        // next-game-mode global one frame after the op writes it, so the flip
        // into the cutscene mode lands here, at the top of the following tick.
        self.maybe_enter_pending_cutscene();
        // Effect-pool walker on the retail-frame sub-clock: retail runs the
        // per-frame walker once per rendered frame with its vsync catch-up
        // factor - one sweep per vsync. Under the 1:1 denomination above that
        // is one sweep per sim tick; the gate names the clock the walker's
        // wait counters are denominated in rather than thinning them.
        // REF: FUN_801E0088
        if self.clock.display_frame_step == 1 && runs_master_driver {
            self.tick_effects();
        }
        if runs_master_driver {
            self.tick_move_vms();
        }
        // Actor pool on the retail **game-tick** clock. Retail resolves one
        // `DAT_1F800393` per frame (`FUN_80016B6C`) and runs the per-actor
        // dispatcher once per game tick, so with the field floor of 2
        // (installed by the scene loader `FUN_801D6704`) the pool advances
        // every second vsync - and the tick that fires carries `frame_step`
        // into the dispatcher's scalars instead of `1`.
        //
        // The pairing is the whole point, and neither half is correct alone:
        // every retail duration accumulates `DAT_1F800393` rather than `1`
        // (`t = min(t + dt, d)`), which makes durations **cadence-invariant** -
        // a 600-frame move arrives after 600 vsyncs at any cadence. Gating
        // without the scalars would halve wall-clock speed; scaling without
        // the gate would double it. Together they leave every duration where
        // it was and only drop the *sample rate*: retail emits a pose every
        // `frame_step` vsyncs, so the engine draws proportionally fewer
        // intermediate poses over the same wall-clock span.
        //
        // REF: FUN_80016B6C (cadence resolver), FUN_801D6704 (field floor)
        if self.clock.display_frame_step == 1 && runs_master_driver {
            self.clock.actor_vsync_accum += 1;
        }
        let cadence = self.clock.frame_step.max(1);
        let actor_tick_fired = self.clock.actor_vsync_accum >= cadence && runs_master_driver;
        self.clock.game_tick_fired = actor_tick_fired;
        if actor_tick_fired {
            self.clock.actor_vsync_accum = 0;
            self.tick_actor_physics();
            // The `jalr node[+0x0C]` arm of the same walk: run the ported
            // per-frame handler kernels (the colour tween, the field VM
            // clone's clip-fraction fade, the `4C 86` reflection pairs) and
            // drop the actors that raised the kill bit - this pass's own
            // expiries plus any marked by the scene-transition sweep or a
            // `FUN_8003CF40` retire since the last tick.
            // REF: FUN_8002519C
            self.tick_handler_actors(cadence);
            self.tick_actors();
            // Actor-VM glides (op 0x09 `MotionAt` -> `start_motion`): one
            // motion-VM pursue step per game tick toward the recorded target.
            self.tick_actor_motions();
        }
        // Drain the scripted countdown the field VM armed with `0x4C 0xD3`.
        // Retail's scheduler runs once per display frame off the play clock,
        // so drive it on the same retail-frame sub-clock the other 60 Hz
        // consumers use.
        if self.clock.display_frame_step == 1 {
            self.tick_escape_timer();
        }
        // Tick art-learned banner countdown - clear when it reaches zero.
        if let Some(banner) = &mut self.party.current_art_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.party.current_art_banner = None;
            }
        }
        // Tick level-up banner countdown; when it expires the next member who
        // levelled in the same fight takes the slot (see
        // `World::party.pending_level_up_banners`).
        if let Some(banner) = &mut self.party.current_level_up_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.party.current_level_up_banner =
                    self.party.pending_level_up_banners.pop_front();
            }
        }
        // Advance the post-battle Seru-capture banner; clear when it finishes.
        if let Some(banner) = &mut self.party.current_capture_banner {
            banner.tick_frame();
            if banner.is_done() {
                self.party.current_capture_banner = None;
            }
        }
        // Advance the opening-cutscene narration roller. The crawl is
        // timer-driven only (retail `FUN_80037174` has no per-line confirm
        // skip; the player skips the WHOLE opening through the hand-off
        // packet instead - see `take_prologue_handoff`). Clear it once every
        // page has scrolled off so the suspended cutscene timeline resumes.
        // The roller counts vsyncs and runs one handler pass per
        // `frame_step`-vsync retail frame it was opened at (`cutscene_narration`).
        if let Some(narration) = &mut self.cutscene.narration
            && !narration.tick(self.clock.display_frame_step as u32)
        {
            self.cutscene.narration = None;
        }
        // Fade the "It was the Seru." caption image (opdeene's baked-TIM
        // caption, `Self::cutscene_caption`). It is target-visible in the
        // FIRST gap after a narration crawl block has shown (a block opened,
        // `seq >= 1`, and has since scrolled out - narration inactive), and
        // fades out on the next block or scene end (the image is cleared on
        // scene entry). At the retail-video-pinned crawl rate the blocks run
        // back-to-back, so the first real gap lands after the LAST crawl -
        // the caption fades in over the held villager tableau, which is
        // where the retail capture shows it. The smooth alpha ramp stands in
        // for the TIM's two-CLUT fade steps.
        //
        // The timeline's post-crawl hold can run long, so `in_gap` alone
        // would freeze the caption on screen. Bound it to a retail-like ~2 s
        // beat: once it has been fully shown for `CAPTION_HOLD_FRAMES`, fade
        // it back out and keep it hidden (the counter never resets within
        // the scene, so the caption shows exactly once).
        if self.cutscene.caption.is_some() {
            const CAPTION_FADE_STEP: f32 = 0.06;
            const CAPTION_HOLD_FRAMES: u32 = 180;
            let in_gap = self.cutscene.narration_seq >= 1 && !self.cutscene_narration_active();
            if in_gap && self.cutscene.caption_alpha >= 1.0 {
                self.cutscene.caption_shown_frames =
                    self.cutscene.caption_shown_frames.saturating_add(1);
            }
            let hold_elapsed = self.cutscene.caption_shown_frames >= CAPTION_HOLD_FRAMES;
            let target = if in_gap && !hold_elapsed { 1.0 } else { 0.0 };
            if self.cutscene.caption_alpha < target {
                self.cutscene.caption_alpha =
                    (self.cutscene.caption_alpha + CAPTION_FADE_STEP).min(target);
            } else if self.cutscene.caption_alpha > target {
                self.cutscene.caption_alpha =
                    (self.cutscene.caption_alpha - CAPTION_FADE_STEP).max(target);
            }
        }
        // Tick the live `4C E1` text balloon (FUN_801DA7F0 handler; see
        // `crate::text_balloon`). The player-engaged flag (`_DAT_8007C364
        // +0x10 & 0x80000`) is host-substituted by "a dialog engagement is
        // live"; the cadence is the 60 fps sub-clock step, matching the
        // narration roller above.
        let balloon_engaged = self.dialogue_owns_input();
        if runs_master_driver && let Some(balloon) = self.cutscene.text_balloon.as_mut() {
            let engaged = balloon_engaged;
            let cadence = self.clock.display_frame_step as i16;
            if balloon.tick(engaged, cadence) == crate::text_balloon::BalloonTick::Killed {
                self.cutscene.text_balloon = None;
            }
        }
        // Run every live camera-register zone ramp (op `0x43` sub-3..6). Same
        // position in the frame as the balloon above and for the same reason:
        // both are `+0x0C` handlers on retail's one effect-actor list, and
        // this is the one tick path all three hosts reach. Which is also why
        // both sit behind `runs_master_driver`: an actor-list handler is
        // reached only through `FUN_8002519C`, and the CARD-mode frame does not
        // walk the lists at all.
        if runs_master_driver {
            self.tick_register_ramps();
        }
        // The camera vertical-offset easing `FUN_801DA390`: one call a frame,
        // walking `_DAT_8007BCAC` toward `scene_ctrl[+0x4A] - player[+0x16]`.
        // Same gate as the ramps above - retail runs it off the field frame
        // pump, which the CARD-mode frame does not reach.
        if runs_master_driver {
            self.tick_camera_offset_ease();
        }
        // The three frame-delta timer templates (`0x801F2858` bars,
        // `0x801F2840` eased moves, `0x801F27EC` floor-ladder rungs) ride the
        // same gate for the same reason: all three are `+0x0C` handlers on
        // that one effect-actor list.
        if runs_master_driver {
            let delta = self.clock.display_frame_step.min(u16::from(u8::MAX)) as u8;
            self.tick_field_timer_actors(delta);
            // The element channel rides the same gate for the same reason -
            // the ambient emitter is a `+0x0C` handler on that one
            // effect-actor list, and its own template
            // (`0x801F271C`) sits in the very table the three above come from.
            // Self-gates to a no-op on an empty channel.
            let mut rng = crate::world::WorldRng::new(self.rng_state);
            self.tick_cutscene_elements(delta, || rng.step());
            self.rng_state = rng.state();
            // The fog pool's render step (`FUN_8003F348`, run from the field
            // render pass) sees `DAT_1F800393` as its `dt`; the hosts call
            // it from their draw path, so the ticks accumulate until then.
            self.fog.pending_dt = self.fog.pending_dt.saturating_add(u32::from(delta));
        }
        // The three-actor-talk controller's per-frame flag poll: when the
        // scene script drops the talk lock (system flag 0xD), the controller
        // despawns and the story party un-collapses. Same all-hosts tick
        // position as the ramps above.
        if runs_master_driver {
            self.tick_three_actor_talk();
        }
        // Menu-staged Door uses (Door of Wind warp / Door of Light
        // escape): hand the staged record to the pause-menu session, whose
        // travel art ends in the named scene transition the scene host
        // already drains.
        self.drain_staged_menu_warp();
        // The pause-menu session's post-menu half: the ramp-down to the
        // travel-art hand-off, then the art itself, on every host.
        self.tick_pause_session();
        // A minigame the player can enter must be one the player can leave.
        self.poll_minigame_escape();
        // Age the minigame effect-part pool. Here rather than in a host's own
        // frame step: a pool a host owns ages only on that host, and the
        // fishing splash spent its whole life native-only for exactly that
        // reason (see [`crate::minigame_fx`]). Unconditional, like the ramps
        // above - a pool with no live part costs a length test.
        self.minigames.fx.tick(1);
        match self.mode {
            SceneMode::Battle => {
                // The frame driver's discarded `rand()` draw, once a battle
                // frame (`FUN_80046A20` `0x80046D2C`).
                self.tick_battle_pass_draw();
                // Battle animation advance. This is SIMULATION, not
                // presentation: its staged-clip end edge retires `ADVANCE_DONE`
                // and converges the anim id pair - the pacing gate whose
                // failure parks the attack chain at `AttackChain` (`0x1E`)
                // forever. It ran only from the native window's redraw, so the
                // browser and headless hosts never advanced it at all; it must
                // sit here, where every host reaches it. Retail advances the
                // anim system in the frame driver's tick passes (`FUN_8002519C`)
                // ahead of the render passes, which is this position.
                //
                // Ahead of the dialogue gates below, and outside
                // `live_battle_tick`, deliberately: that function early-returns
                // while a command session or a submenu is open, and a command
                // session stays open for as long as the player deliberates.
                // Driving the anims from inside it would freeze every actor's
                // idle loop for that whole window and un-freeze it on confirm.
                // REF: FUN_8002519C
                self.tick_battle_animations();
                // The Arts announcement banner's slide clock. Retail steps it
                // from the battle DRAW tick (`FUN_800480D8`); the engine steps
                // it here so every host advances it, and reads the quads back
                // at draw time (`World::battle_arts_banner_quads`).
                // REF: FUN_800480D8
                self.tick_arts_banner(cadence);
                // In-battle dialogue box (the tutorial text the engage script
                // opened across the transition): the box owns the frame -
                // retail parks the battle under it (the camera holds the
                // dialogue close-up while the text is up) and a confirm /
                // cancel press advances / dismisses it. Drive whichever
                // dialogue channel is live: the inline-script runner carried
                // across the Field -> Battle transition (only Field ticks it
                // otherwise, so it would stick mid-line forever), or the
                // simplified `current_dialog` box on the field / overworld
                // dismiss idiom (`op4c_n_5_sub_4_dialog_advance` /
                // `tick_world_map_npc_dialog`).
                if self.dialog.inline.is_some() {
                    self.drive_inline_dialogue();
                    None
                } else if self.dialog.current.is_some() {
                    if self.input.just_pressed(input::PadButton::Cross)
                        || self.input.just_pressed(input::PadButton::Circle)
                    {
                        self.dialog.current = None;
                        self.pending_field_events
                            .push(crate::field_events::FieldEvent::DialogDismissed);
                    }
                    None
                } else {
                    // A battle that was ENTERED must be DRIVEN. Retail's
                    // action SM (`FUN_801E295C`) has no "loop enabled"
                    // concept - once the battle scene is up it always runs
                    // the full per-frame driver until a wipe resolves it.
                    // This arm used to be gated on
                    // [`crate::world::WorldToggles::live_gameplay_loop`], falling back to a bare
                    // [`Self::step_battle`] that applies no damage, arms no
                    // turn and never calls [`Self::finish_battle`] - while
                    // battle *entry* (a field carrier's `3E FF` scripted
                    // fight, a world-map region encounter) was never gated
                    // at all. The result was an unresolvable battle: the
                    // ungated entry paths could strand a default session in
                    // `SceneMode::Battle` forever. The Field arm's random
                    // encounter *roll* stays opt-in below; driving a battle
                    // the engine is already in does not.
                    // REF: FUN_801E295C (the retail action SM this drives)
                    self.live_battle_tick()
                }
            }
            SceneMode::Field => {
                // The Field arm as a whole is the engine's counterpart of
                // the retail player master frame handler - the field
                // overlay's per-frame driver that wraps everything below:
                // engaged-flag gating, locomotion (FUN_801D01B0), the
                // vertical settle (FUN_801D1BA0), touch/walk-on dispatch
                // (FUN_801DE234/801DE3E0 through FUN_801D1EC4), the camera
                // update (FUN_801DB510/801DAA50) and the intro-skip packet
                // (pad 0x100 while `_DAT_1F800394 & 0x4000000` ->
                // `FUN_8001FD44("town01", 3)`, ported as
                // `World::take_prologue_handoff`). Leg-for-leg mapping in
                // the comments below; the retail body is
                // `overlay_cutscene_dialogue_801d1344.txt`.
                // PORT: FUN_801d1344 (frame-pump orchestration; legs are
                //                     individually ported + cited below)
                //
                // Per-tick: one Cross/Circle edge feeds at most one of the
                // script's 0x4C dialog poll or the interaction probe.
                self.dialog.input_consumed = false;
                // A committed battle suspends every field script context
                // until the fight returns (`Self::field_scripts_held_for_battle`).
                let scripts_held = self.field_scripts_held_for_battle();
                // Retail-frame paced (see `step_spawned_record_contexts`).
                if !scripts_held {
                    self.step_spawned_record_contexts();
                }
                // No placement channel steps here: retail runs a placement's
                // own script only while a touch holds it engaged
                // (`World::pre_run_field_channel_prologues` has the
                // `+0x10 & 0x100` writers), which the engine plays as the
                // interaction timeline.
                let scripts_held = scripts_held || self.field_scripts_held_for_battle();
                // The scene system script (ctx `0xFB`) gets a whole retail
                // frame slice, not one instruction: see
                // [`Self::step_field_frame_slice`] for the three stop
                // conditions and what one-op-per-tick cost.
                if !scripts_held {
                    self.step_field_frame_slice();
                }
                // A placement the system script engaged (`B1 <id> 08`) runs
                // its interaction once the frame is free.
                self.drain_placement_engagements();
                // Field script actors the VM just spawned or is running: the
                // op-0x43 scripted arcs (arc helper `FUN_801D5C08` + release
                // watcher `FUN_801D5D60`) and the op-0x34 sub-1 attached
                // lights' tear-down + keyframe script (`FUN_801E4470` /
                // `FUN_801E3E00`). Pool actors, one visit per frame.
                // REF: FUN_801d5d60, FUN_801e3e00
                self.tick_field_script_arcs();
                self.tick_field_attached_lights();
                // Field-NPC walk legs (cutscene walk-to-tile pokes,
                // actor-VM glides) - one motion-VM step per RETAIL
                // frame, writing back into `field_npc_positions` so collision /
                // interact probes follow the live NPC. The step decode takes
                // `dt = _DAT_1f800393` at 1, so one call credits one retail
                // display frame of glide (`field_npc_walk_step_speed`) and the
                // call rate has to be the retail 60 Hz frame clock. Retail
                // reaches the same wall speed from the other side: it visits
                // `FUN_8003774C` once per game tick and multiplies each leg by
                // `DAT_1F800393` (`0x80037868 lbu s2,0x393(s2)`).
                // REF: FUN_8003774C
                if self.clock.display_frame_step == 1 {
                    self.tick_field_npc_motions();
                }
                // Ambient facing channels (`FUN_80038158` ops 0x04 / 0x0D):
                // the idle turn-in-place a standing town NPC runs between
                // walk legs. Part of the actor pool, so it advances on the
                // actor game tick, not per rendered frame - which is what
                // keeps op 0x0D in lockstep with its ramp scheduler.
                // REF: FUN_80038158, FUN_80036D80
                if actor_tick_fired {
                    self.tick_field_npc_ambient();
                    // The same driver's height arm (`FUN_8003BC08`), after
                    // the tick moved anyone: the glide-class NPCs' Y.
                    self.tick_field_npc_heights();
                }
                self.tick_tile_board();
                // Rebuild the tile-actor draw list from the current board +
                // player cell (retail's per-frame board render pass).
                self.refresh_tile_board_draw_list();
                self.step_field_locomotion();
                // Walk-regen: drain the accumulator the step above just fed
                // and apply the three accessory-gated restore bumps. Its
                // Incense zero edge raises the wear-off notice, which runs
                // here too.
                self.tick_field_walk_regen();
                self.tick_incense_notice();
                // Vertical settle + ledge-hop trigger. Retail runs this as a
                // separate per-frame controller after the walk commits, so
                // it reads the step-delta pair the walk just wrote.
                // PORT: FUN_801d1ba0
                if let Some(pslot) = self.player_actor_slot {
                    self.step_field_vertical(pslot as usize);
                }
                // The system channel's tick follows the player's: its
                // per-tick store of the idle clip base lands after the settle
                // has read the base.
                self.tick_field_system_channel_clip_reset();
                // Motion detection: diff every tracked actor's position
                // against last frame's. Runs after EVERY mover in the frame
                // (timeline, channels, field VM, NPC motion legs, locomotion)
                // so the walk clip is selected by whether an actor moved, not
                // by which subsystem moved it - the script paths commit a
                // position and raise no flag of their own.
                self.tick_player_scale_ramp();
                self.tick_npc_heading_ramps();
                self.npcs.looks.tick(self.move_vm.ramp_ratio.max(1));
                self.detect_field_actor_motion();
                // Locomotion animation: idle vs walk off the movement flag
                // the step above just set, folded into the player's
                // `pose_frame` for the host's posed-mesh rebuild.
                self.tick_field_player_anim();
                // Placed-prop layer: advance the prop clips, step an
                // in-flight prop record run (a door swing / cupboard search
                // through the field VM), and start a run for a movement
                // touch the locomotion just posted (the retail bit-4
                // auto-post of FUN_801D5B5C).
                self.tick_prop_interactions();
                // Interaction probe (retail FUN_801cf9f4): talk to an adjacent
                // NPC / dismiss its box on the action button. Runs before the
                // carrier tick so a dialogue-accept engage launches the battle
                // the same frame.
                self.tick_field_interaction_probe();
                self.tick_field_carriers();
                // Faithful dialogue path (opt-in): drive a just-opened field
                // dialogue through the field VM so branch handlers execute.
                self.drive_inline_dialogue();
                // Interaction teardown: put an addressed NPC's authored facing
                // back once no dialogue channel owns the frame any more
                // (retail's `+0x5A` -> `+0x26` restore on the dialog SM's exit
                // path). Placed after the runner start above, so the frame the
                // talk begins already counts as engaged and the save survives.
                // REF: FUN_80039B7C
                // A placement's own context resumed as the interaction
                // timeline (a boss stager) is the same engaged SM: its
                // restore waits for the timeline's `0x21`.
                if !self.dialogue_owns_input()
                    && self.dialog.active_inline_prologue.is_none()
                    && !self.interaction_timeline_holds_talk_facing()
                {
                    self.release_talk_facing();
                }
                // Screen-effect widgets (mask / sprite / panel / letterbox,
                // the ending-scene op-0x43 family) tick after the script step
                // that may have spawned them this frame.
                self.tick_screen_fx();
                if self.toggles.live_gameplay_loop {
                    self.live_field_tick();
                } else {
                    // `--no-live-loop` gates the encounter *roll* only: a
                    // battle something else armed (a scripted carrier's
                    // transition) is still clocked and drained, so the
                    // intro plays and the fight opens.
                    self.tick_encounter();
                    if let Some(roll) = self.drain_encounter_formation() {
                        self.begin_encounter_battle(roll);
                    }
                }
                None
            }
            SceneMode::Cutscene => {
                // An in-engine choreography cutscene (no STR FMV) is just a
                // field scene that suppresses field/battle dispatch, so the
                // field VM keeps stepping. While an STR FMV is playing
                // ([`active_fmv`] set), the field VM is suspended - retail
                // hands the frame to the cutscene/MDEC overlay - and the host
                // drives playback, calling [`finish_cutscene`] when it ends.
                if self.cutscene.active_fmv.is_none() {
                    self.step_spawned_record_contexts();
                    self.step_field_frame_slice();
                    self.tick_field_script_arcs();
                    self.tick_field_attached_lights();
                    self.tick_screen_fx();
                }
                None
            }
            SceneMode::WorldMap => {
                // The opening chain's `map01` fly-in leg runs its cutscene
                // record over the world map (Mist title card + crawl + the
                // terminal SceneChange into Rim Elm), and a free-roam overworld
                // walk-on **beat** record (a Drake mist-wall force-walk band, a
                // gate-1 non-portal partition-2 record spawned by
                // `SceneHost::dispatch_walk_on_trigger` in WorldMap mode) is the
                // same single-context cutscene timeline. Step whichever is
                // installed; `step_world_map_locomotion` stands the overworld
                // player down while it plays (the force-walk lock).
                // Overworld helper spawns (an op-0x44 issued by a world-map
                // record) execute concurrently, same as the field arm - both
                // are retail-frame paced.
                self.step_spawned_record_contexts();
                // The scene system script (ctx `0xFB`, MAN `P1[0]`). The
                // overworld is a mode-3 field-run scene and retail runs its
                // entry script like any field's: `map01`'s sets the visible
                // tile window (`46 24 EE F4 12 20`, `(-18, -12, 18, 32)` -
                // the window every library `map01` state holds) and raises
                // the ambient-particle gate (`4C 30`) that puts fog over the
                // continent. Same frame slice as the field arm.
                self.step_field_frame_slice();
                // Cross-context walk legs a world-map record starts
                // (`C7 <id> ..`) run on the same per-actor walk kernel as the
                // field's (`FUN_8003774C`, from the actor driver every mode-3
                // scene runs). Without the step an overworld beat that walks
                // a placement and then waits on its halt bit (`B3 <id> 0A` -
                // urudre2's hand-off onto `map01`) parks for good.
                // REF: FUN_8003774C
                if self.clock.display_frame_step == 1 {
                    self.tick_field_npc_motions();
                }
                // The per-actor anim tick: the overworld's MAN actors play
                // the kingdom bundle's slot-4 clips through `FUN_800204F8`
                // exactly as a town's do (a live `map01` actor list holds
                // four clip-bound actors resolving into that bank).
                self.tick_actor_anims();
                // Clock a committed overworld encounter's field-to-battle
                // transition (the intro overlay rides this phase) and open
                // the fight when it elapses - the world-map twin of the
                // field drain in `live_field_tick`.
                self.tick_encounter();
                if let Some(roll) = self.drain_encounter_formation() {
                    self.enter_world_map_battle(roll);
                }
                self.tick_world_map();
                None
            }
            SceneMode::Dance => {
                self.tick_dance();
                None
            }
            SceneMode::Fishing => {
                self.tick_fishing();
                None
            }
            SceneMode::SlotMachine => {
                self.tick_slot_machine();
                None
            }
            SceneMode::BakaFighter => {
                self.tick_baka_fighter();
                None
            }
            SceneMode::MuscleDome => {
                self.tick_muscle_dome();
                None
            }
            // The pause menu owns the frame (retail CARD mode 0x17): field /
            // battle dispatch is suspended; the hosting session drives the
            // menu state machine and restores the suspended mode on close.
            SceneMode::Menu => None,
            SceneMode::Title => None,
        }
    }

    /// Field walk-regen driver: project the present party onto the
    /// [`crate::walk_regen`] kernel, run one tick against
    /// [`crate::world::FieldLocomotion::walk_regen_steps`], and write the bumped gauges back into the
    /// roster records.
    ///
    /// REF: FUN_801D0B90
    ///
    /// The kernel ([`crate::walk_regen::tick_walk_regen`]) is the retail
    /// body: it only runs while the accumulator exceeds
    /// [`crate::walk_regen::WALK_REGEN_STEP_COST`] (`0x20`), subtracts that
    /// cost, and bumps HP / MP / AP by `8` / `2` / `1` for each member whose
    /// ability-bitfield word 1 carries the walk-passive bit (`0x38` Life
    /// Source, `0x39` Magic Source, `0x3A` Mettle Source), each clamped at
    /// the record's effective maximum. A party with none of those accessories
    /// equipped therefore sees no state change at all, which is why wiring
    /// this moves no existing oracle.
    ///
    /// The **fill** side is retail's as well, and it is inside the locomotion
    /// controller: `FUN_801D01B0`'s tail at `0x801D0910..0x801D0928` adds
    /// `DAT_1F800393` to `_DAT_801F2274` behind the step-delta-non-zero test at
    /// `0x801D08F4..0x801D090C` - one unit per vsync whose step committed.
    /// [`Self::step_field_locomotion`] adds [`crate::world::FrameClock::display_frame_step`] once per
    /// sim tick, which is the same rate under the 1:1 denomination.
    ///
    /// The kernel's return value is the edge where the Incense window
    /// [`crate::world::FieldLocomotion::walk_regen_window`] (`_DAT_8007B600`,
    /// armed by the pause Items Incense confirm) runs out. Retail then
    /// installs the entry-context record `0x801F2278` (kind byte `0x0B`) and
    /// spawns the submode driver (`0x801D0CEC..0x801D0D24`), which maps kind
    /// `0x0B` to `FUN_801F1E48` - the one-line wear-off notice. That edge
    /// raises [`Self::raise_incense_notice`].
    ///
    /// The same tick runs on the **overworld**: a kingdom map is a mode-3
    /// field-run scene with the field overlay resident, and the frame driver
    /// `FUN_801D1344` calls this routine (`jal` at `0x801D16EC`) right before
    /// the locomotion controller. The WorldMap arm therefore calls it as
    /// well ([`Self::tick_world_map`]).
    ///
    /// Member order is the present party (retail walks the member-id table
    /// at `0x80084598`), resolved through [`Self::party_roster_slot`].
    pub(crate) fn tick_field_walk_regen(&mut self) {
        use crate::walk_regen::{WalkGauge, WalkRegenMember};
        if self.locomotion.walk_regen_steps <= crate::walk_regen::WALK_REGEN_STEP_COST {
            return;
        }
        let count = (self.party.party_count.min(3) as usize).min(self.party.roster.members.len());
        let slots: Vec<usize> = (0..count).map(|i| self.party_roster_slot(i)).collect();
        let mut members: Vec<WalkRegenMember> = Vec::with_capacity(slots.len());
        for &rslot in &slots {
            let Some(rec) = self.party.roster.members.get(rslot) else {
                continue;
            };
            let hms = rec.hp_mp_sp();
            // Word 1 of the `+0xF4` ability bitfield - the word the three
            // walk-passive bits (`0x38..=0x3A`) land in.
            let bits = rec.ability_bits();
            let ability_hi = u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]);
            members.push(WalkRegenMember {
                ability_hi,
                hp: WalkGauge {
                    value: hms.hp_cur,
                    cap: hms.hp_max,
                },
                mp: WalkGauge {
                    value: hms.mp_cur,
                    cap: hms.mp_max,
                },
                ap: WalkGauge {
                    value: hms.sp_cur,
                    cap: hms.sp_max,
                },
            });
        }
        let mut counter = self.locomotion.walk_regen_steps;
        let mut window = self.locomotion.walk_regen_window;
        let armed = crate::walk_regen::tick_walk_regen(&mut counter, &mut members, &mut window);
        self.locomotion.walk_regen_steps = counter;
        self.locomotion.walk_regen_window = window;
        if armed {
            self.raise_incense_notice();
        }
        for (&rslot, m) in slots.iter().zip(members.iter()) {
            let Some(rec) = self.party.roster.members.get_mut(rslot) else {
                continue;
            };
            let mut hms = rec.hp_mp_sp();
            if hms.hp_cur == m.hp.value && hms.mp_cur == m.mp.value && hms.sp_cur == m.ap.value {
                continue;
            }
            hms.hp_cur = m.hp.value;
            hms.mp_cur = m.mp.value;
            hms.sp_cur = m.ap.value;
            rec.set_hp_mp_sp(hms);
        }
    }
}

mod tile_board_tick;

mod minigame_sessions;
