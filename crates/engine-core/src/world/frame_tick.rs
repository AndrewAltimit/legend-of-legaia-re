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

impl World {
    /// Advance the wall-clock play-time counter by `delta_seconds`. Engines
    /// drive this from the frame loop's wall-clock delta. Mirrors the
    /// retail "play time" field shown on the save screen.
    pub fn advance_play_time(&mut self, delta_seconds: u32) {
        self.play_time_seconds = self.play_time_seconds.saturating_add(delta_seconds);
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
        let balloon = self.text_balloon.as_mut()?;
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
        let b = self.text_balloon.as_ref()?;
        (!b.killed && b.timer >= 1 && b.timer < b.total).then_some(b.text.as_slice())
    }

    /// Run every live camera-register zone ramp (field-VM op `0x43`
    /// sub-3..6) for one frame - the port of `FUN_80037018`'s per-actor tick,
    /// driven off the player's world position.
    ///
    /// Retail runs one of these actors per spawned ramp off the effect-actor
    /// list; the engine holds the records on [`World::register_ramps`] and
    /// steps them here. What a tick writes lands in
    /// [`World::camera_registers`], the four field camera-configuration
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
        if self.register_ramps.is_empty() {
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
        for (i, ramp) in self.register_ramps.iter().enumerate() {
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
            self.camera_registers.set(slot, value);
        }
        for i in retire.into_iter().rev() {
            self.register_ramps.remove(i);
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
        self.talk_switch_requested = true;
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
    ///   The port also latches its field **run** modifier off Square
    ///   ([`Self::field_run_button_held`]), so inside an armed talk one press
    ///   does both. That is the run modifier intruding, not the swap: the
    ///   swap bit is disassembly-pinned and the run mask word `0x800846DC`
    ///   is explicitly not.
    /// - **1** - hold [`LEADER_SWAP_FADE_FRAMES`] behind the fade-to-white
    ///   ([`Self::screen_fade`] carries the retail template: kind 2, `0x20`
    ///   frames, black -> white, `801d29c8..801d2a00`).
    /// - **2** - the swap (`801d2a54..801d2c7c`): the outgoing leader's
    ///   participant NPC takes the player's pose (retail: camera `+0x14..`
    ///   onto the outgoing actor), the next participant whose presence flag
    ///   reads clear becomes leader (wrap-scan from `leader+1`), flags
    ///   `0x10..=0x12` are re-pointed at the new leader, the player takes
    ///   the incoming participant's pose (+ the negated map origin
    ///   `_DAT_80089118/20` = [`Self::map_origin_xz`]), the incoming NPC is
    ///   parked at the `0x3F80` sentinel, and the fade-back-in spawns.
    /// - **3** - release the fade object (engine: [`Self::screen_fade`]
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
        let Some(mut talk) = self.three_actor_talk else {
            // No live talk: a stale switch request must not outlive the
            // session that could consume it.
            self.talk_switch_requested = false;
            return;
        };
        let request = if talk.swap.phase == 0 {
            // Retail's own source (`_DAT_8007B874 & 0x80` = Square, newly
            // pressed) plus the scripted latch. `take` runs first and
            // unconditionally, so a latch set outside phase 0 is not left to
            // fire into a later swap.
            let latched = core::mem::take(&mut self.talk_switch_requested);
            latched || self.input.just_pressed(input::PadButton::Square)
        } else {
            false
        };
        let leader = self.party_leader_slot.unwrap_or(0).min(2);
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
        let step = self.frame_step.max(1);
        // Same MSB-first bank layout as `Self::system_flag_test` (the SCUS
        // helper `FUN_8003CE64` the controller calls).
        let flags = &self.system_flags;
        let tick = talk.swap.step(&swap_world, step, |idx| {
            let byte = (idx >> 3) as usize;
            flags
                .get(byte)
                .is_some_and(|b| b & (0x80u8 >> (idx & 7)) != 0)
        });
        if talk.swap.phase == 5 {
            // State-0 poll saw the talk lock down: despawn (retail runs the
            // state-5 body one frame later; the engine folds it).
            self.three_actor_talk = Some(talk);
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
                        if let Some(&pos) = self.field_npc_positions.get(&slot) {
                            let heading = self.field_npc_headings.get(&slot).copied().unwrap_or(0);
                            talk.saved[i] = Some((pos, heading));
                        }
                    }
                }
                LeaderSwapEffect::SpawnFadeOut => {
                    // `801d29c8..801d2a00`: kind 2, 0x20 frames, black ->
                    // white, no start delay / no hold.
                    self.screen_fade =
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
                        self.field_npc_positions.insert(npc, (px, pz));
                        self.field_npc_headings.insert(npc, heading);
                    }
                }
                LeaderSwapEffect::CommitLeader { slot } => {
                    // `801d2ae8..801d2b1c`: leader byte + collapsed id list
                    // re-point at the incoming slot; flags 0x10..=0x12
                    // cleared, `0x10 + slot` set.
                    self.party_leader_slot = Some(slot);
                    self.party_actor_slots = vec![Some(slot)];
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
                    if let Some(&(nx, nz)) = self.field_npc_positions.get(&npc) {
                        let heading = self.field_npc_headings.get(&npc).copied().unwrap_or(0);
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
                        self.map_origin_xz = (-i32::from(nx), -i32::from(nz));
                    }
                }
                LeaderSwapEffect::ClearIncomingPose { slot } => {
                    // `801d2c0c..801d2c20`: the incoming leader's actor is
                    // parked at the 0x3F80 sentinel (the player object now
                    // represents them).
                    let npc = self.talk_participant_slot(ids[usize::from(slot.min(2))]);
                    self.field_npc_positions.insert(
                        npc,
                        (LEADER_ACTOR_POSE_SENTINEL, LEADER_ACTOR_POSE_SENTINEL),
                    );
                }
                LeaderSwapEffect::SpawnFadeIn => {
                    // `801d2c24..801d2c54`: kind 2, 0x20 frames, white ->
                    // black.
                    self.screen_fade =
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
                    // ([`Self::screen_fade`]); no modelled camera flag word
                    // to clear.
                }
                LeaderSwapEffect::RetireController => {
                    self.three_actor_talk = Some(talk);
                    self.end_three_actor_talk();
                    return;
                }
            }
        }
        self.three_actor_talk = Some(talk);
    }

    /// Resolve a talk-instruction participant id to the engine's field-NPC
    /// placement slot - the same actor-list walk the op-`0x43` sub-2 arm
    /// performs (retail `FUN_8003C83C`); an unmatched id passes through raw.
    // REF: FUN_8003C83C (id resolve)
    fn talk_participant_slot(&self, id: u8) -> u8 {
        crate::field_channels::resolve_target(&self.field_channels, id)
            .map(|ci| self.field_channels[ci].placement_index as u8)
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
        let Some(talk) = self.three_actor_talk.take() else {
            return;
        };
        self.system_flag_clear(0xD);
        if talk.saved_party_len > 0 {
            self.party_actor_slots = talk.saved_party[..talk.saved_party_len as usize].to_vec();
            self.party_leader_slot = talk
                .saved_leader
                .or_else(|| self.party_actor_slots.first().copied().flatten());
        }
    }

    /// Drain a menu-staged transition into the named scene transition the
    /// scene host consumes ([`Self::pending_named_scene_transition`]).
    ///
    /// **Door of Wind** ([`Self::pending_menu_warp`]): the staged triple is
    /// retail's `0x80084628` scene word + `0x80084624`/`0x8008462C` tile
    /// pair (`FUN_801D8B90` phase 3, from quick-travel placement record
    /// bytes `+2/+4/+5`). The scene word is the destination scene's raw
    /// CDNAME TOC index ([`Self::scene_toc_names`]); the tile pair seats
    /// the party at `(tile << 7) + 0x40`, the same conversion the world-map
    /// arrival kernel applies (`FUN_801EE328`: `0x80073EF4/EF8` stores).
    /// The named-transition drain performs exactly that seat
    /// (`seat_player_at_tile`), entering the kingdom overworld for the
    /// three `mapNN` bases and the field scene for the `son` / `korout`
    /// records. An unresolvable scene word logs retail's
    /// `UNFIND MAP NUMBER %d` diagnostic (the `FUN_801EE328` phase-`0x63`
    /// park) and drops the warp.
    ///
    /// **Door of Light** ([`Self::pending_menu_escape`]): retail hands the
    /// outer menu SM exit code 4 (`FUN_801D8A58`) - the dungeon-escape
    /// handoff, whose overlay-side consumer is not yet pinned. The engine
    /// routes it onto the last visited-map record (the return point the
    /// travel arts warp to): back to that kingdom overworld at the stored
    /// tile. With no visited record yet (the party has never stood on a
    /// kingdom map) the escape is dropped with a diagnostic.
    ///
    /// REF: FUN_801D8B90 (stage), FUN_801D8A58 (escape exit code),
    /// FUN_801EE328 (arrival tile math + UNFIND diagnostic)
    pub fn drain_staged_menu_warp(&mut self) {
        if let Some(warp) = self.pending_menu_warp.take() {
            match self.scene_toc_names.get(&u32::from(warp.scene_id)) {
                Some(name) => {
                    self.pending_named_scene_transition =
                        Some((name.clone(), warp.menu_x, warp.menu_y, 0));
                }
                None => {
                    // Retail's miss arm prints and parks (phase 0x63).
                    log::warn!("menu warp: UNFIND MAP NUMBER {}", warp.scene_id);
                }
            }
        }
        if self.pending_menu_escape {
            self.pending_menu_escape = false;
            let visited = self
                .world_map_ctrl
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
                    self.pending_named_scene_transition = Some((
                        name.to_string(),
                        v.tile_x.clamp(0, 0xFF) as u8,
                        v.tile_z.clamp(0, 0xFF) as u8,
                        0,
                    ));
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
        self.scene_toc_names = map;
    }

    /// Arm the timed sound-source auto-release for `deadline` vsyncs
    /// (`gp+0x814`). [`Self::tick`] counts it down by the frame step.
    ///
    /// Retail's arm half writes five `gp` cells, not three: on top of the
    /// timer's armed flag / deadline / elapsed it latches the live brightness
    /// word `_DAT_8007B910` and the caller's tag, then tail-calls the libsnd
    /// volume shim with `(level >> 1, deadline | 1)`. Those two extra cells
    /// land in [`Self::sound_arm`] so a host driving the shim has the exact
    /// arguments; the engine has no live brightness ramp of its own, so the
    /// latched level is the cold-reset value retail boots `_DAT_8007B910` to.
    ///
    /// PORT: FUN_800267A8
    /// REF: FUN_800267FC, FUN_80062004
    pub fn arm_sound_release(&mut self, deadline_vsyncs: i32) {
        self.sound_release.arm(deadline_vsyncs);
        self.pending_sound_release = false;
        self.sound_arm = Some(crate::scus_leaf_kernels::TimedSoundArm::arm(
            0,
            deadline_vsyncs.max(0) as u32,
            crate::new_game::GAME_STATE_COLD_RESET.screen_brightness,
        ));
    }

    /// Drain the "the sound-release deadline expired" event.
    pub fn take_pending_sound_release(&mut self) -> bool {
        std::mem::take(&mut self.pending_sound_release)
    }

    /// Run the one-shot sound detach (`FUN_8002689C`). Returns `true` only on
    /// the first call - retail's `gp+0x804` latch gates every later one out,
    /// which is why the mode-INIT chain can call it freely.
    ///
    /// PORT: FUN_8002689c
    pub fn detach_sound(&mut self) -> bool {
        self.sound_detach.detach()
    }

    /// Consume the frame-begin skip request, returning whether this frame
    /// should be abandoned. Models `FUN_8001698C`'s non-zero return; see
    /// [`Self::frame_begin_skip`].
    ///
    /// PORT: FUN_8001698c (the frame-skip return; the ring-aging half of the
    /// same function is `legaia_engine_audio::sfx_ring::SfxCueRing::age`)
    pub fn take_frame_begin_skip(&mut self) -> bool {
        std::mem::take(&mut self.frame_begin_skip)
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
        self.escape_timer_flag_word = ab;
        self.escape_timer = vm::escape_timer::EscapeTimer {
            remaining: cd as i32,
            warn_threshold: ef as i32,
            armed: cd != 0,
        };
        self.escape_timer_hud = None;
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
    /// 3. `_DAT_8007B6B0 > 0` - the **reposition / warp timer** `FUN_801C36AC`
    ///    counts down while it walks a warped actor to its destination tile.
    ///    The engine warps instantly, so it has no in-flight window; the mode
    ///    test below is the nearest stand-in, covering the frames where the
    ///    field is not what is being driven at all (a menu, a battle, a
    ///    minigame).
    fn escape_timer_busy(&self) -> bool {
        self.dialogue_owns_input() || !matches!(self.mode, SceneMode::Field | SceneMode::Cutscene)
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
    /// of the tick, not of a renderer - [`World::escape_timer_hud`] caches
    /// this frame's.
    ///
    /// The delta is one retail frame per call (the caller gates on
    /// [`World::field_frame_step`]); retail reads it as
    /// `_DAT_80084570 - _DAT_80073ED4`, a clock that also advances one step
    /// per display frame.
    ///
    /// REF: FUN_801D2EBC (scheduler + HUD decomposition; the ports are
    /// `legaia_engine_vm::escape_timer::EscapeTimer` and `timer_ink`)
    fn tick_escape_timer(&mut self) {
        if !self.escape_timer.armed {
            self.escape_timer_hud = None;
            return;
        }
        let busy = self.escape_timer_busy();
        let flag_word = self.escape_timer_flag_word;
        let events = self.escape_timer.tick(1, flag_word, busy);
        if let Some(flag) = events.expiry_flag {
            self.system_flag_set(flag);
        }
        if let Some(flag) = events.warning_flag {
            self.system_flag_set(flag);
        }
        let (minutes, seconds, hundredths) = self.escape_timer.hud_fields();
        let ink = vm::escape_timer::timer_ink(self.escape_timer.remaining);
        self.escape_timer_hud = Some((minutes, seconds, hundredths, ink));
    }

    /// Resolve this frame's cadence the way `FUN_80016B6C` does and install
    /// it into [`Self::frame_step`].
    ///
    /// PORT: FUN_80016b6c (the `0x80017044 .. 0x800171D8` cadence block; the
    /// telemetry state machine lives in
    /// [`legaia_engine_vm::actor_tick::FrameStepTelemetry`]).
    ///
    /// `elapsed_hblanks` is the frame time retail samples with `VSync(1)`
    /// through `FUN_800173BC`. The floor is [`Self::frame_step_floor`]
    /// (`DAT_8007B9D8`), installed per scene, and the resolver can only raise
    /// the cadence above it - never below.
    ///
    /// **Hosts that want determinism should not call this.** Retail gates the
    /// whole adaptive path on a boot config word (`gp+0x4CE == 0x10`); with
    /// `frameskip_enabled = false` this returns the floor unchanged, which is
    /// exactly what the replay / trace oracles need. Wall-clock-paced hosts
    /// pass their measured frame time and `true`.
    ///
    /// NOT WIRED: no host calls it, and the paragraph above is why. The
    /// engine ticks at a fixed sim rate, nothing samples retail's `VSync(1)`
    /// hblank count (`FUN_800173BC`) to supply `elapsed_hblanks`, and the
    /// replay / trace oracles require the floor to stay where the scene
    /// loader installed it. Wiring it needs a host frame-time sampler **and**
    /// a decision that the adaptive cadence is on (retail gates the whole
    /// path behind the boot config word `gp+0x4CE == 0x10`); until then
    /// [`Self::frame_step`] is scene-driven and this has nothing to resolve.
    pub fn resolve_frame_step(&mut self, elapsed_hblanks: i32, frameskip_enabled: bool) -> u8 {
        let cadence = self.frame_step_telemetry.resolve(
            elapsed_hblanks,
            frameskip_enabled,
            self.frame_step_floor,
        );
        self.frame_step = cadence.vsyncs_per_tick();
        self.frame_step
    }

    /// The `VSync(n)` argument retail would pass this frame - **last** frame's
    /// cadence, with `< 2` passed as `0` (`0x8001719C`, read before the new
    /// value is written back at `0x800171D8`).
    ///
    /// REF: FUN_80016B6C
    pub fn frame_step_vsync_wait(&self) -> u8 {
        self.frame_step_telemetry.vsync_wait()
    }

    /// Increment the deterministic LCG and return the new value.
    pub fn next_rng(&mut self) -> u32 {
        // Numerical Recipes LCG. Cheap, deterministic.
        self.rng_state = self
            .rng_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.rng_state
    }

    /// Replace the per-frame pad bitmask snapshot. Equivalent to
    /// `self.input.set_pad(mask)` but available without importing
    /// [`input::InputState`] at the call site. Hosts that drive the
    /// world from a scripted timeline (`legaia-engine replay`, the
    /// v0.1 playthrough oracle) call this before each [`Self::tick`].
    /// Also latches [`Self::field_run_button_held`] off the same word, so the
    /// run modifier reaches every host that feeds a pad - native window,
    /// browser play page, replay driver - without any of them wiring it
    /// separately. Deriving it here rather than per-host is deliberate: a
    /// per-host derivation is exactly the shape the UI-drift gate exists to
    /// catch, and this way there is nothing to keep in sync.
    ///
    /// The button is **Square**. Retail's held-pad speed modifier - the
    /// debug-turbo arm of the same base-step selector - reads packed bit
    /// `0x80`, which the fishing controller pins as Square, so it is the
    /// retail-adjacent choice; the run mask config word `0x800846DC` itself
    /// is unpinned (see [`Self::field_run_button_held`]).
    pub fn set_pad(&mut self, mask: u16) {
        self.input.set_pad(mask);
        self.field_run_button_held = mask & input::PadButton::Square.mask() != 0;
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
        self.frame += 1;
        // Bridge the vsync-rate pad to the game-tick-rate actor pool for the
        // op-0x49 submode screens: the hosts publish a pad word every tick and
        // the dispatcher runs every `frame_step` ticks, so without this the
        // half of the edges that land on a skipped pass are lost outright.
        // See `SubmodeScreen::pad_edge_latch`.
        self.latch_submode_pad_edge();
        // Age the post-battle spoils panel (armed by `finish_battle`) and the
        // no-encounters-here hint (armed by `arm_live_loop`).
        self.battle_spoils_frames = self.battle_spoils_frames.saturating_sub(1);
        self.scene_encounter_hint_frames = self.scene_encounter_hint_frames.saturating_sub(1);
        // ------------------------------------------------------------------
        // The simulation clock's denomination.
        //
        // **One `World::tick` is exactly one retail display frame (vsync).**
        // Both hosts already drive it that way and must keep doing so: the
        // native window's fixed-timestep accumulator (`EngineWindow::
        // drain_ticks`, `TICK_DT = 1.0/60.0`, backlog capped at 4 ticks) and
        // the browser play page's (`site/js/play-app.js`, `TICK_DT = 1000/60`,
        // same cap). So `SIM_HZ == RETAIL_FPS`, the retail-frame sub-clock is
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
        self.field_frame_step = 1;
        self.field_frames += 1;
        // Kept advancing as the cheap "a world frame ran" witness (the mode
        // driver's frame-begin-skip test probes it); the fixed-point phase it
        // used to carry is gone with the 1:1 denomination.
        self.field_frame_accum = self.field_frame_accum.wrapping_add(1);
        // Retail game-tick clock for the scripted CLUT-cell effects: one game
        // tick spans `frame_step` vsyncs (the adaptive `DAT_1F800393` factor
        // written by `FUN_80016B6C`; see [`Self::frame_step`]). Count the sim
        // ticks that map to a retail vsync and bank a game tick every
        // `frame_step` of them; [`Self::step_clut_fx`] drains the bank
        // against the host's VRAM. Only accumulates while effects are live
        // (capped so an undrained host can't wind up a backlog).
        if self.field_frame_step == 1 && !self.clut_fx.is_empty() {
            self.clut_vsync_accum += 1;
            if self.clut_vsync_accum >= self.frame_step.max(1) {
                self.clut_vsync_accum = 0;
                self.clut_pending_game_ticks = (self.clut_pending_game_ticks + 1).min(600);
            }
        }
        // Same game-tick law for the ambient move-VM effect parts (jou's
        // CLUT-cell cyclers / lightning director); drained by the host's
        // `step_ambient_fx` against its VRAM.
        if self.field_frame_step == 1 && !self.ambient_fx.is_empty() {
            self.ambient_vsync_accum += 1;
            if self.ambient_vsync_accum >= self.frame_step.max(1) {
                self.ambient_vsync_accum = 0;
                self.ambient_pending_game_ticks = (self.ambient_pending_game_ticks + 1).min(600);
            }
        }
        // Retail's frame-begin driver services the timed sound-source
        // auto-release before anything else in the frame (`FUN_800267FC`,
        // called at `0x800169FC`). Its accumulator advances by the frame step,
        // so drive it on the sim ticks that map to a retail vsync.
        if self.field_frame_step == 1 {
            let step = self.frame_step.max(1);
            // The teardown gates (`record[+8]` active, `_DAT_8007B868`) live
            // in the libsnd voice binding the engine replaces, so the engine
            // arm is "release when it fires" unconditionally.
            if let crate::sound_state::SoundReleaseTick::Fired { .. } =
                self.sound_release.tick(step, true, false)
            {
                self.pending_sound_release = true;
            }
        }
        // Step the active full-screen fade (escape teardown ramp); drop it
        // once the ramp lands on its target so hosts stop drawing the overlay.
        if let Some(fade) = &mut self.screen_fade
            && !fade.step()
        {
            self.screen_fade = None;
        }
        // Step the two scripted scene-tint channels (op 0x34 sub-0 effect
        // tint + op 0x4C 0x12 global screen tint). A ramp that lands on a
        // non-neutral target HOLDS there (a screen faded to black stays
        // black until a new op replaces it); one that lands on the neutral
        // identity is dropped so the render path returns to untouched.
        for tint in [&mut self.effect_tint, &mut self.screen_tint] {
            if let Some(t) = tint {
                t.step();
                if t.is_identity() {
                    *tint = None;
                }
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
        if self.field_frame_step == 1 {
            self.tick_effects();
        }
        self.tick_move_vms();
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
        if self.field_frame_step == 1 {
            self.actor_vsync_accum += 1;
        }
        let cadence = self.frame_step.max(1);
        let actor_tick_fired = self.actor_vsync_accum >= cadence;
        if actor_tick_fired {
            self.actor_vsync_accum = 0;
            self.tick_actor_physics();
            // The `jalr node[+0x0C]` arm of the same walk: run the ported
            // per-frame handler kernels (today the colour tween) and drop the
            // actors that raised the kill bit - both this pass's tweens and
            // any marked by the scene-transition sweep or a `FUN_8003CF40`
            // retire since the last tick.
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
        if self.field_frame_step == 1 {
            self.tick_escape_timer();
        }
        // Tick art-learned banner countdown - clear when it reaches zero.
        if let Some(banner) = &mut self.current_art_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_art_banner = None;
            }
        }
        // Tick level-up banner countdown; when it expires the next member who
        // levelled in the same fight takes the slot (see
        // `World::pending_level_up_banners`).
        if let Some(banner) = &mut self.current_level_up_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_level_up_banner = self.pending_level_up_banners.pop_front();
            }
        }
        // Advance the post-battle Seru-capture banner; clear when it finishes.
        if let Some(banner) = &mut self.current_capture_banner {
            banner.tick_frame();
            if banner.is_done() {
                self.current_capture_banner = None;
            }
        }
        // Advance the opening-cutscene narration roller. The crawl is
        // timer-driven only (retail `FUN_80037174` has no per-line confirm
        // skip; the player skips the WHOLE opening through the hand-off
        // packet instead - see `take_prologue_handoff`). Clear it once every
        // page has scrolled off so the suspended cutscene timeline resumes.
        // Scroll the roller on the retail-frame sub-clock. Its speed is pinned
        // from a realtime retail video as 1 px per 6 frames at 60 Hz
        // (`cutscene_narration::DEFAULT_FRAMES_PER_PIXEL`), so the sub-clock
        // has to deliver a full 60 retail frames a second - which it does only
        // under the 1:1 denomination (at the old 100 Hz premise it delivered
        // 36, and the crawl ran at 0.6x its own pinned figure).
        if let Some(narration) = &mut self.cutscene_narration
            && !narration.tick(self.field_frame_step as u32)
        {
            self.cutscene_narration = None;
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
        if self.cutscene_caption.is_some() {
            const CAPTION_FADE_STEP: f32 = 0.06;
            const CAPTION_HOLD_FRAMES: u32 = 180;
            let in_gap = self.cutscene_narration_seq >= 1 && !self.cutscene_narration_active();
            if in_gap && self.cutscene_caption_alpha >= 1.0 {
                self.cutscene_caption_shown_frames =
                    self.cutscene_caption_shown_frames.saturating_add(1);
            }
            let hold_elapsed = self.cutscene_caption_shown_frames >= CAPTION_HOLD_FRAMES;
            let target = if in_gap && !hold_elapsed { 1.0 } else { 0.0 };
            if self.cutscene_caption_alpha < target {
                self.cutscene_caption_alpha =
                    (self.cutscene_caption_alpha + CAPTION_FADE_STEP).min(target);
            } else if self.cutscene_caption_alpha > target {
                self.cutscene_caption_alpha =
                    (self.cutscene_caption_alpha - CAPTION_FADE_STEP).max(target);
            }
        }
        // Tick the live `4C E1` text balloon (FUN_801DA7F0 handler; see
        // `crate::text_balloon`). The player-engaged flag (`_DAT_8007C364
        // +0x10 & 0x80000`) is host-substituted by "a dialog engagement is
        // live"; the cadence is the 60 fps sub-clock step, matching the
        // narration roller above.
        let balloon_engaged = self.dialogue_owns_input();
        if let Some(balloon) = self.text_balloon.as_mut() {
            let engaged = balloon_engaged;
            let cadence = self.field_frame_step as i16;
            if balloon.tick(engaged, cadence) == crate::text_balloon::BalloonTick::Killed {
                self.text_balloon = None;
            }
        }
        // Run every live camera-register zone ramp (op `0x43` sub-3..6). Same
        // position in the frame as the balloon above and for the same reason:
        // both are `+0x0C` handlers on retail's one effect-actor list, and
        // this is the one tick path all three hosts reach.
        self.tick_register_ramps();
        // The three-actor-talk controller's per-frame flag poll: when the
        // scene script drops the talk lock (system flag 0xD), the controller
        // despawns and the story party un-collapses. Same all-hosts tick
        // position as the ramps above.
        self.tick_three_actor_talk();
        // Menu-staged transitions (Door of Wind warp / Door of Light
        // escape): convert the staged record into the named scene
        // transition the scene host already drains.
        self.drain_staged_menu_warp();
        // A minigame the player can enter must be one the player can leave.
        self.poll_minigame_escape();
        match self.mode {
            SceneMode::Battle => {
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
                if self.inline_dialogue.is_some() {
                    self.drive_inline_dialogue();
                    None
                } else if self.current_dialog.is_some() {
                    if self.input.just_pressed(input::PadButton::Cross)
                        || self.input.just_pressed(input::PadButton::Circle)
                    {
                        self.current_dialog = None;
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
                    // [`Self::live_gameplay_loop`], falling back to a bare
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
                self.dialog_input_consumed = false;
                // Retail-frame paced (see `step_spawned_record_contexts`).
                self.step_spawned_record_contexts();
                // Per-actor script channels (spawned with a cutscene
                // timeline): each vignette actor's own placement script runs
                // its frame slice - animate cues, scripted moves, flag
                // handshakes with the timeline.
                //
                // Ungated deliberately, and that is the retail denomination,
                // not an omission. A placement script is actor-pool work
                // (`FUN_8002519C` -> the actor's `+0x0C` handler), so retail
                // runs it once per game tick and credits `DAT_1F800393` frames
                // of progress per visit - the same cadence-invariant identity
                // the locomotion note above spells out. The engine takes the
                // fine-grained half: one visit per vsync crediting one frame.
                // Since a sim tick IS a vsync, calling it every tick is
                // retail-frame paced already; a `field_frame_step` gate would
                // be a tautology here, not a correction.
                self.step_field_channels();
                // The scene system script (ctx `0xFB`) gets a whole retail
                // frame slice, not one instruction: see
                // [`Self::step_field_frame_slice`] for the three stop
                // conditions and what one-op-per-tick cost.
                self.step_field_frame_slice();
                // Field-NPC walk legs (autonomous patrol routes + scripted
                // interaction-prologue runs) - one motion-VM step per RETAIL
                // frame, writing back into `field_npc_positions` so collision /
                // interact probes follow the live NPC. The step decode takes
                // `dt = _DAT_1f800393` at 1, so one call credits one retail
                // display frame of glide (`field_npc_walk_step_speed`) and the
                // call rate has to be the retail 60 Hz frame clock. Retail
                // reaches the same wall speed from the other side: it visits
                // `FUN_8003774C` once per game tick and multiplies each leg by
                // `DAT_1F800393` (`0x80037868 lbu s2,0x393(s2)`).
                // REF: FUN_8003774C
                if self.field_frame_step == 1 {
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
                }
                self.tick_tile_board();
                // Rebuild the tile-actor draw list from the current board +
                // player cell (retail's per-frame board render pass).
                self.refresh_tile_board_draw_list();
                self.step_field_locomotion();
                // Walk-regen: drain the accumulator the step above just fed
                // and apply the three accessory-gated restore bumps.
                self.tick_field_walk_regen();
                // Vertical settle + ledge-hop trigger. Retail runs this as a
                // separate per-frame controller after the walk commits, so
                // it reads the step-delta pair the walk just wrote.
                // PORT: FUN_801d1ba0
                if let Some(pslot) = self.player_actor_slot {
                    self.step_field_vertical(pslot as usize);
                }
                // Motion detection: diff every tracked actor's position
                // against last frame's. Runs after EVERY mover in the frame
                // (timeline, channels, field VM, NPC motion legs, locomotion)
                // so the walk clip is selected by whether an actor moved, not
                // by which subsystem moved it - the script paths commit a
                // position and raise no flag of their own.
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
                if !self.dialogue_owns_input() && self.active_inline_prologue.is_none() {
                    self.release_talk_facing();
                }
                // Screen-effect widgets (mask / sprite / panel / letterbox,
                // the ending-scene op-0x43 family) tick after the script step
                // that may have spawned them this frame.
                self.tick_screen_fx();
                if self.live_gameplay_loop {
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
                if self.active_fmv.is_none() {
                    self.step_spawned_record_contexts();
                    self.step_field_channels();
                    self.step_field_frame_slice();
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
    /// [`Self::walk_regen_steps`], and write the bumped gauges back into the
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
    /// [`Self::step_field_locomotion`] adds [`Self::field_frame_step`] once per
    /// sim tick, which is the same rate under the 1:1 denomination.
    ///
    /// One honest gap remains:
    ///
    /// - The kernel's return value is the edge where retail arms a
    ///   dialog-window callback off [`Self::walk_regen_window`]
    ///   (`_DAT_8007B600`). The engine has no such window slot and nothing
    ///   arms the countdown, so the edge cannot fire and the result is
    ///   dropped here.
    ///
    /// Member order is the present party (retail walks the member-id table
    /// at `0x80084598`), resolved through [`Self::party_roster_slot`].
    fn tick_field_walk_regen(&mut self) {
        use crate::walk_regen::{WalkGauge, WalkRegenMember};
        if self.walk_regen_steps <= crate::walk_regen::WALK_REGEN_STEP_COST {
            return;
        }
        let count = (self.party_count.min(3) as usize).min(self.roster.members.len());
        let slots: Vec<usize> = (0..count).map(|i| self.party_roster_slot(i)).collect();
        let mut members: Vec<WalkRegenMember> = Vec::with_capacity(slots.len());
        for &rslot in &slots {
            let Some(rec) = self.roster.members.get(rslot) else {
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
        let mut counter = self.walk_regen_steps;
        let mut window = self.walk_regen_window;
        // The dialog-window arm edge (see the note above) has no consumer.
        let _armed = crate::walk_regen::tick_walk_regen(&mut counter, &mut members, &mut window);
        self.walk_regen_steps = counter;
        self.walk_regen_window = window;
        for (&rslot, m) in slots.iter().zip(members.iter()) {
            let Some(rec) = self.roster.members.get_mut(rslot) else {
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

    /// Tile-board player step: read one d-pad direction from
    /// [`World.input`](Self::input), gate it against the board's
    /// collision cells, and interpolate the player actor toward the
    /// destination tile centre. Drives the puzzle / board minigame mode,
    /// not general town locomotion.
    ///
    /// PORT: the walk state machine in `overlay_0897_801ef2b0`. The
    /// player is either *idle* (`tile_board_target == None`, accepting a
    /// new direction) or *interpolating* toward a committed target tile
    /// (case 2). A direction is only consumed while idle, so holding the
    /// d-pad steps tile-by-tile - matching retail, where the SM re-reads
    /// the pad only after the previous step's interpolation completes.
    ///
    /// No-ops without a player actor slot or an installed
    /// [`tile_board`](crate::tile_board), and while a dialog box is up
    /// (the field VM owns the frame). Reads only pad bits + board state,
    /// so it is deterministic across identical pad streams.
    fn tick_tile_board(&mut self) {
        if self.dialogue_owns_input() {
            return;
        }
        let Some(player_slot) = self.player_actor_slot else {
            return;
        };
        let slot = player_slot as usize;
        if self.tile_board.is_none() || slot >= self.actors.len() {
            return;
        }

        // Interpolating toward a committed target tile.
        if let Some((tx, tz)) = self.tile_board_target {
            let ms = &mut self.actors[slot].move_state;
            let nx = step_toward(ms.world_x as i32, tx, TILE_BOARD_SPEED);
            let nz = step_toward(ms.world_z as i32, tz, TILE_BOARD_SPEED);
            ms.world_x = nx as i16;
            ms.world_z = nz as i16;
            if nx == tx && nz == tz {
                self.tile_board_target = None;
                self.tile_board_arrival();
            }
            return;
        }

        // Idle: decode one direction and try to step.
        let Some(dir) = tile_step_from_input(&self.input) else {
            return;
        };
        if let Some((tx, tz)) = self.tile_board.as_mut().and_then(|b| b.try_step(dir)) {
            self.tile_board_target = Some((tx, tz));
        }
    }

    /// Advance the screen-effect widgets one frame and refresh
    /// [`Self::screen_fx_frame`]. Runs in the Field / Cutscene tick after
    /// the script step (so a sub-op spawned this frame draws this frame,
    /// matching retail's actor-pool order). The engine ticks the widget
    /// clocks by 1 per world tick (retail's per-frame byte
    /// `DAT_1F800393`); the sprite scripts' flag waits probe the shared
    /// system flag bank ([`Self::system_flag_test`], `FUN_8003CE64`).
    fn tick_screen_fx(&mut self) {
        if !self.screen_fx.is_active() {
            if !self.screen_fx_frame.is_empty() {
                self.screen_fx_frame = Default::default();
            }
            return;
        }
        let mut fx = std::mem::take(&mut self.screen_fx);
        self.screen_fx_frame = fx.tick(1, |idx| self.system_flag_test(idx));
        self.screen_fx = fx;
    }

    /// Walk-SM arrival pass (`overlay_0897_801ef2b0` case 3), run when the
    /// player's interpolation reaches the committed tile centre:
    ///
    /// - an **event / transition cell** (`8..=0xA`) leaves the board mode -
    ///   the board uninstalls, and the suspended op-0x49 script reads `Done`
    ///   and resumes (retail reads the header `+7`/`+9` flag operands here;
    ///   the engine surfaces the exit through the op-49 tristate);
    /// - an **animated cell** (`0xB..=0xE`) cycles its value one step,
    ///   wrapping `0xE -> 0xB` (the arrival sub-state's decay pass).
    ///
    /// PORT: overlay_0897_801ef2b0 (arrival sub-states)
    fn tile_board_arrival(&mut self) {
        use crate::tile_board::{
            CELL_ANIM_FIRST, CELL_ANIM_LAST, CELL_EVENT_FIRST, CELL_EVENT_LAST,
        };
        let Some(board) = self.tile_board.as_mut() else {
            return;
        };
        let (col, row) = (board.player_col as i32, board.player_row as i32);
        let Some(cell) = board.cell(col, row) else {
            return;
        };
        if (CELL_EVENT_FIRST..=CELL_EVENT_LAST).contains(&cell) {
            // Event / transition tile: exit the board. `tile_board_armed`
            // stays set so the op-49 tristate reads Done and the field
            // script resumes past the install op. Despawn the tile actors
            // so they don't leak into the next scene.
            self.tile_board = None;
            self.tile_board_header = None;
            self.despawn_tile_actors();
        } else if (CELL_ANIM_FIRST..=CELL_ANIM_LAST).contains(&cell) {
            let next = if cell == CELL_ANIM_LAST {
                CELL_ANIM_FIRST
            } else {
                cell + 1
            };
            let idx = row as usize * board.width as usize + col as usize;
            if let Some(c) = board.cells.get_mut(idx) {
                *c = next;
            }
        }
    }

    /// Install a tile board from a field-VM op-0x49 **sub-op 5** instruction
    /// (`instr` = the bytes from the opcode onward, as handed to
    /// `FieldHost::op49_menu_request`). Parses the 13-byte inline header
    /// (`instr[1..]`, the window retail points `_DAT_8007b450` at), fills the
    /// cells with the retail procedural fill (`overlay_0897_801e0b1c`, seeded
    /// from the world RNG the way retail seeds from BIOS `rand`), seats the
    /// player actor at the board's start-cell centre, and holds the script
    /// suspended (`tile_board_armed`) until the board exits.
    ///
    /// Returns `false` (leaving the op merely suspended, matching the other
    /// op-49 consumers) when a board is already up or the header is
    /// malformed.
    ///
    /// PORT: overlay_0897_801e0b1c (board alloc + fill; cells only - the
    /// per-cell tile-actor spawns are a renderer concern)
    /// REF: overlay_0897_801de840 (op 0x49 arm, `_DAT_8007b450 = pbVar47`)
    pub fn try_install_tile_board(&mut self, instr: &[u8]) -> bool {
        if self.tile_board_armed || self.tile_board.is_some() {
            return false;
        }
        let Some(window) = instr.get(1..) else {
            return false;
        };
        let Some(header) = crate::tile_board::TileBoardHeader::parse(window) else {
            return false;
        };
        let cells = crate::tile_board::procedural_fill(header.width, header.height, || {
            self.next_rng() & 0x7FFF
        });
        let board = crate::tile_board::TileBoard::from_header(&header, cells);

        // Spawn one tile actor per distinct drawn cell value present on the
        // board (retail `DAT_801f35bc[value]`, slots `2..=14`): resolve the
        // template `tile_template_base + (value - 2)` through the same
        // global-TMD + VDF-buffer path the `0x4C 0xD8` field allocator uses
        // (`spawn_field_actor`). The renderer repositions + draws these each
        // frame; unresolved templates still allocate a slot (empty mesh).
        let mut present = [false; crate::tile_board::TILE_ACTOR_TABLE_LEN];
        for &c in &board.cells {
            if crate::tile_board::is_drawable_cell(c) {
                present[c as usize] = true;
            }
        }
        let mut tile_slots = [None; crate::tile_board::TILE_ACTOR_TABLE_LEN];
        for value in crate::tile_board::CELL_DRAW_FIRST..=crate::tile_board::CELL_DRAW_LAST {
            if !present[value as usize] {
                continue;
            }
            let tpl = crate::tile_board::tile_template_for(header.tile_template_base, value);
            if let Some(slot) = self.spawn_field_actor(tpl as i16, tpl, value as u16, 0) {
                tile_slots[value as usize] = Some(slot as u8);
            }
        }
        // Table slot 0 = the player actor (retail spawns it from header
        // `+0xb`). The engine reuses the existing player actor: seat it at
        // the start cell's tile centre so the first step interpolates from
        // the board frame, and bind its mesh from `player_template` when the
        // global TMD pool carries it (else keep the field mesh).
        if let Some(slot) = self.player_actor_slot {
            tile_slots[0] = Some(slot);
            let (x, z) = board.player_world();
            let player_tmd = self.global_tmd(header.player_template as i16).cloned();
            if let Some(a) = self.actors.get_mut(slot as usize) {
                a.move_state.world_x = x as i16;
                a.move_state.world_z = z as i16;
                if let Some(tmd) = player_tmd {
                    a.tmd_ref = Some(tmd);
                }
            }
        }

        self.tile_actor_slots = tile_slots;
        self.tile_board_target = None;
        self.tile_board = Some(board);
        self.tile_board_header = Some(header);
        self.tile_board_armed = true;
        true
    }

    /// Despawn the tile-board tile actors (the `2..=14` entries of the
    /// tile-actor table) and clear the table + draw list. The player actor
    /// (table slot 0) outlives the board and is left in place. Called on
    /// board teardown so tile actors don't leak into the next scene.
    ///
    /// PORT: the walk-SM board-exit teardown (`overlay_0897_801ef2b0`
    /// case 8 -> board free).
    fn despawn_tile_actors(&mut self) {
        for value in crate::tile_board::CELL_DRAW_FIRST..=crate::tile_board::CELL_DRAW_LAST {
            if let Some(slot) = self.tile_actor_slots[value as usize]
                && let Some(a) = self.actors.get_mut(slot as usize)
            {
                *a = Actor::new();
            }
        }
        self.tile_actor_slots = [None; crate::tile_board::TILE_ACTOR_TABLE_LEN];
        self.tile_board_draw_list.clear();
    }

    /// Rebuild the per-frame tile-board draw list (retail
    /// `overlay_0897_801e0f3c`): for every drawable cell in the active draw
    /// set (full board or the windowed radius around the player, per header
    /// `+6`/`+5`), select the cell value's tile actor from the tile-actor
    /// table and record it at the cell's world centre, then reposition that
    /// actor there (retail moves the selected actor before drawing). When a
    /// value repeats across cells the shared actor ends at the last drawn
    /// cell; the draw list still carries the full per-cell set the deferred
    /// renderer needs. Clears the list when no board is installed. The
    /// player actor is drawn by the normal field path, so it is not seated
    /// here (that would fight the step interpolation).
    fn refresh_tile_board_draw_list(&mut self) {
        let Some(header) = self.tile_board_header else {
            self.tile_board_draw_list.clear();
            return;
        };
        let Some(board) = self.tile_board.as_ref() else {
            self.tile_board_draw_list.clear();
            return;
        };
        let mut list = Vec::new();
        for (col, row) in board.draw_cells(header.mode_flag, header.radius) {
            let Some(cell) = board.cell(col, row) else {
                continue;
            };
            if !crate::tile_board::is_drawable_cell(cell) {
                continue;
            }
            let Some(slot) = self.tile_actor_slots[cell as usize] else {
                continue;
            };
            let (world_x, world_z) = board.tile_world(col, row);
            list.push(crate::tile_board::TileDraw {
                col: col as u8,
                row: row as u8,
                cell_value: cell,
                slot,
                world_x,
                world_z,
            });
        }
        for d in &list {
            if let Some(a) = self.actors.get_mut(d.slot as usize) {
                a.move_state.world_x = d.world_x as i16;
                a.move_state.world_z = d.world_z as i16;
            }
        }
        self.tile_board_draw_list = list;
    }

    /// Enter the Noa dance (rhythm) minigame on `game`, suspending the current
    /// scene mode. The suspended mode is restored by [`World::exit_dance`] (and
    /// automatically once the song ends). Mirrors the pause-menu suspend/restore
    /// contract: the interrupted field/battle state stays intact underneath.
    ///
    /// Applies the dance stager's pad-latch clear
    /// ([`crate::dance::dance_scene_stage`]): retail zeroes `_DAT_8007B880` on
    /// the frame the hall is staged, so the confirm press that starts the
    /// minigame is not also read as its first judged note.
    pub fn enter_dance(&mut self, game: crate::dance::DanceGame) {
        // Don't stack a suspend: if the dance is already running, just swap the
        // game so a re-entry keeps the true return mode.
        if self.mode != SceneMode::Dance {
            self.dance_return_mode = self.mode;
        }
        self.dance = Some(game);
        self.dance_last_judge = None;
        self.mode = SceneMode::Dance;
        if crate::dance::dance_scene_stage().clear_pad_latch {
            self.input.clear_edges();
        }
    }

    /// Clear the dance minigame and return the final [`DanceGame`] so the host
    /// can read the score / pass result. Restores the interrupted mode if it is
    /// still `Dance` (a mid-song abort); when the song already auto-ended
    /// [`tick_dance`](Self::tick_dance) has restored the mode but left the game
    /// installed for one frame so the host can read it - this take clears it.
    pub fn exit_dance(&mut self) -> Option<crate::dance::DanceGame> {
        if self.mode == SceneMode::Dance {
            self.mode = self.dance_return_mode;
        }
        self.dance_last_judge = None;
        // The stager runs on teardown as well as on entry, so the press that
        // leaves the hall does not carry into the restored field mode.
        if crate::dance::dance_scene_stage().clear_pad_latch {
            self.input.clear_edges();
        }
        self.dance.take()
    }

    /// Advance the dance minigame one frame: step the beat clock, judge this
    /// frame's directional presses, and end the run when the song finishes.
    ///
    /// The judged buttons are the retail ones: `FUN_801d1af4` reads the
    /// newly-pressed word `_DAT_8007B874` and tests the three face bits
    /// `0x80` / `0x20` / `0x10` = Square / Circle / Triangle, which is exactly
    /// [`crate::dance::DanceDir::pad_bit`]. This frame's pad edges are packed
    /// into that layout and the direction is picked by matching `pad_bit`, so
    /// the bit-to-direction binding lives in the ported kernel. Edge-triggered
    /// (`just_pressed`) so a held button scores at most one note per press.
    ///
    /// PORT: the dance overlay's per-frame driver (`FUN_801cf470` beat clock ->
    /// `FUN_801d1960` hit judge), one advance + one judged press pass per frame.
    fn tick_dance(&mut self) {
        let Some(game) = self.dance.as_mut() else {
            // Mode is Dance but no game installed - drop back to a sane mode.
            self.mode = self.dance_return_mode;
            return;
        };
        game.advance(1);
        // Judge at most one directional press this frame (retail tests all
        // three bits in one pass, but a rhythm player presses one at a time);
        // the scan order is retail's - Triangle, then Square, then Circle.
        //
        // The engine's `PadButton` word and the packed word `FUN_8001822C`
        // builds hold the same 16 buttons with the two bytes swapped (see
        // `crate::retail_pad`: face/shoulder cluster low, dpad/system high),
        // so one rotate turns this frame's edges into the mask the retail
        // judge reads.
        use crate::dance::DanceDir;
        let pressed = (self.input.pad() & !self.input.pad_prev()).rotate_right(8);
        let dir = [DanceDir::C, DanceDir::A, DanceDir::B]
            .into_iter()
            .find(|d| pressed & d.pad_bit() != 0);
        if let Some(dir) = dir {
            self.dance_last_judge = Some(game.judge_press(dir));
        }
        if game.song_over() {
            // Song finished: restore the interrupted mode, leaving `dance`
            // in place so the host can read the final score before clearing.
            self.mode = self.dance_return_mode;
        }
    }

    /// Enter the fishing minigame on `session`, suspending the current scene
    /// mode (restored by [`World::exit_fishing`]). Like the dance / pause-menu
    /// suspend contract, the interrupted field state stays intact underneath.
    pub fn enter_fishing(&mut self, session: crate::fishing::FishingSession) {
        if self.mode != SceneMode::Fishing {
            self.fishing_return_mode = self.mode;
        }
        self.fishing = Some(session);
        self.mode = SceneMode::Fishing;
    }

    /// Leave the fishing minigame and restore the interrupted mode, returning
    /// the session so the host can read the final [`FishingRecord`]. The
    /// record's point total is banked into the persistent
    /// [`World::fishing_points`] pool (retail credits `_DAT_8008444C`
    /// directly; hosts seed the next session's record from the pool). No-op
    /// when fishing isn't active.
    ///
    /// [`FishingRecord`]: crate::fishing::FishingRecord
    pub fn exit_fishing(&mut self) -> Option<crate::fishing::FishingSession> {
        if self.mode == SceneMode::Fishing {
            self.mode = self.fishing_return_mode;
        }
        let session = self.fishing.take();
        self.fishing_exchange = None;
        if let Some(s) = &session {
            self.fishing_points = s.record().points;
        }
        session
    }

    /// Open the fishing point-exchange (prize shop) list on `exchange`.
    /// The host renders [`World::fishing_exchange`] and commits buys through
    /// [`World::fishing_exchange_buy`].
    pub fn open_fishing_exchange(&mut self, mut exchange: crate::fishing::PrizeExchange) {
        // Row 0 hides until strictly affordable - floor the cursor to the
        // first visible row for the current point pool.
        exchange.cursor = exchange
            .cursor
            .max(exchange.first_visible(self.fishing_points));
        self.fishing_exchange = Some(exchange);
    }

    /// Close the point-exchange list.
    pub fn close_fishing_exchange(&mut self) {
        self.fishing_exchange = None;
    }

    /// Commit a point-exchange purchase of `qty` units of `row`
    /// (`FUN_801d06c8`'s Yes arm): validates through
    /// [`crate::fishing::PrizeExchange::buy`] against the persistent pool /
    /// purchased mask / live inventory count, then deducts
    /// [`World::fishing_points`], latches the one-time bit, and grants the
    /// item into [`World::inventory`]. While a fishing session is live its
    /// record is synced to the reduced pool so the on-screen point total
    /// matches. `None` when no exchange is open or the buy doesn't validate.
    pub fn fishing_exchange_buy(
        &mut self,
        row: usize,
        qty: u32,
    ) -> Option<crate::fishing::PrizePurchase> {
        let ex = self.fishing_exchange.as_ref()?;
        let item_id = ex.rows.get(row)?.item_id;
        let owned = *self.inventory.get(&item_id).unwrap_or(&0) as u32;
        let purchase = ex.buy(
            row,
            qty,
            self.fishing_points,
            owned,
            self.fishing_prizes_purchased,
        )?;
        self.fishing_points -= purchase.cost as i32;
        if let Some(bit) = purchase.latched_bit {
            self.fishing_prizes_purchased |= 1 << bit;
        }
        let count = self.inventory.entry(purchase.item_id).or_insert(0);
        *count = count.saturating_add(purchase.qty.min(255) as u8);
        if let Some(s) = &mut self.fishing {
            s.set_points(self.fishing_points);
        }
        Some(purchase)
    }

    /// Advance the fishing minigame one frame, reading this frame's pad:
    ///
    /// - **Casting**: the power meter oscillates; a confirm press
    ///   ([`Cross`](input::PadButton::Cross)) locks the cast and hooks a fish.
    /// - **Fighting**: holding a reel button raises tension - [`Cross`] is reel
    ///   A (the `rod*9 + 0x23` divisor), [`Square`] reel B (`rod*6 + 0x19`);
    ///   neither held bleeds tension off. The line snaps at max tension. The
    ///   two buttons are the retail packed-pad bits `0x40` / `0x80`, decoded
    ///   through the ported reel decoder [`ReelInput::from_pad_mask`]
    ///   (`FUN_801d7450`) rather than by a host `if` chain, so holding both
    ///   resolves to reel A exactly as retail does.
    /// - **Done**: a confirm press recasts.
    ///
    /// [`Cross`]: input::PadButton::Cross
    /// [`Square`]: input::PadButton::Square
    /// [`ReelInput::from_pad_mask`]: crate::fishing::ReelInput::from_pad_mask
    ///
    /// PORT: the fishing overlay's per-frame driver (`FUN_801cf3bc` mode SM ->
    /// `FUN_801d4004` tension). The casting-meter step is not byte-pinned (the
    /// retail meter sweeps visibly fast); `FISHING_CAST_STEP` is the host rate.
    fn tick_fishing(&mut self) {
        use crate::fishing::{FishingPhase, ReelInput};
        /// Per-frame casting-meter step (see the method note - not byte-pinned).
        const FISHING_CAST_STEP: i32 = 0x80;
        let Some(phase) = self.fishing.as_ref().map(|s| s.phase()) else {
            // Mode is Fishing but no session installed - drop back to a sane mode.
            self.mode = self.fishing_return_mode;
            return;
        };
        match phase {
            FishingPhase::Casting => {
                if let Some(s) = self.fishing.as_mut() {
                    s.advance_cast(FISHING_CAST_STEP);
                }
                if self.input.just_pressed(input::PadButton::Cross)
                    && let Some(s) = self.fishing.as_mut()
                {
                    s.lock_cast();
                }
            }
            FishingPhase::Fighting => {
                // Rebuild the two reel bits of the retail held word
                // `_DAT_8007b850` from this frame's pad and let the ported
                // decoder classify them.
                let mut held = 0u32;
                if self.input.pressed(input::PadButton::Cross) {
                    held |= crate::fishing::REEL_A_PAD_BIT;
                }
                if self.input.pressed(input::PadButton::Square) {
                    held |= crate::fishing::REEL_B_PAD_BIT;
                }
                let input = ReelInput::from_pad_mask(held);
                if let Some(s) = self.fishing.as_mut() {
                    s.reel(input, 1);
                }
            }
            FishingPhase::Done => {
                if self.input.just_pressed(input::PadButton::Cross)
                    && let Some(s) = self.fishing.as_mut()
                {
                    s.recast();
                }
            }
        }
    }

    /// Enter the casino slot-machine minigame on `machine`, suspending the
    /// current scene mode (restored by [`World::exit_slot_machine`]). Like
    /// the dance / fishing / pause-menu suspend contract, the interrupted
    /// field state stays intact underneath.
    pub fn enter_slot_machine(&mut self, machine: crate::slot_machine::SlotMachine) {
        if self.mode != SceneMode::SlotMachine {
            self.slot_return_mode = self.mode;
        }
        self.slot_machine = Some(machine);
        self.mode = SceneMode::SlotMachine;
    }

    /// Leave the slot machine and restore the interrupted mode, committing
    /// the session's final balance into the casino coin bank
    /// ([`World::casino_coins`] - the retail state-100 assignment
    /// `_DAT_800845A4 = DAT_801d4114`). Returns the session so the host can
    /// read the final state. No-op when the machine isn't active.
    pub fn exit_slot_machine(&mut self) -> Option<crate::slot_machine::SlotMachine> {
        if self.mode == SceneMode::SlotMachine {
            self.mode = self.slot_return_mode;
        }
        let mut machine = self.slot_machine.take();
        if let Some(m) = machine.as_mut() {
            self.casino_coins = m.cash_out().max(0) as u32;
        }
        machine
    }

    /// The five mode-24 minigame scene modes, in `sub_id` order.
    pub const MINIGAME_MODES: [SceneMode; 5] = [
        SceneMode::Fishing,
        SceneMode::SlotMachine,
        SceneMode::BakaFighter,
        SceneMode::MuscleDome,
        SceneMode::Dance,
    ];

    /// Whether the world is inside one of the five mode-24 minigames.
    pub fn in_minigame(&self) -> bool {
        Self::MINIGAME_MODES.contains(&self.mode)
    }

    /// **Engine affordance, not a retail port: Start leaves any minigame.**
    ///
    /// Retail's five minigames each quit through their own overlay's SM - the
    /// slot cabinet's exit menu row, the duel's decided-match confirm, the
    /// arena's give-up arm - and every one of those is a *different* control
    /// in a *different* overlay. The port has none of them wired to a control
    /// a player can find: the native window exposes developer hotkeys
    /// (`O` / `B` / `M`), and the browser play page does not draw four of the
    /// five modes at all, so entering one there leaves a frozen field with the
    /// BGM still running and no input that does anything.
    ///
    /// That is the invariant [`crate::scene::SceneHost::drain_minigame_warp`]
    /// already states for its *failure* arms - "a script that armed a warp must
    /// never be left in a mode with no exit" - and it has to hold for the
    /// successful ones too, on every host, or a reachable minigame is a
    /// softlock. Each game's own `exit_*` runs, so the bookkeeping (cash-out,
    /// leg report, point bank) is the same as the deliberate exit; a door-warp
    /// entry additionally closes its round trip through
    /// [`Self::minigame_return_warp`].
    fn poll_minigame_escape(&mut self) {
        if !self.in_minigame() || !self.input.just_pressed(input::PadButton::Start) {
            return;
        }
        match self.mode {
            SceneMode::Fishing => {
                self.exit_fishing();
            }
            SceneMode::SlotMachine => {
                self.exit_slot_machine();
            }
            SceneMode::BakaFighter => {
                self.exit_baka_fighter();
            }
            SceneMode::MuscleDome => {
                self.exit_muscle_dome();
            }
            SceneMode::Dance => {
                self.exit_dance();
            }
            _ => unreachable!("guarded by in_minigame"),
        }
        // Close the mode-24 round trip when the entry came through the door
        // warp (`exit_baka_fighter` already does its own).
        if self.minigame_scene_backup.is_some() {
            self.minigame_return_warp();
        }
    }

    /// Arm the mode-24 minigame door-warp: back up the active scene name and
    /// zero the session-winnings accumulator, so [`Self::minigame_return_warp`]
    /// can round-trip back to the departure scene.
    ///
    /// Mirrors the two retail halves of the entry: the field-VM `0x3E` warp
    /// arm zeroes the winnings accumulator `_DAT_80084440`, and the mode-24
    /// OTHER-INIT entry `FUN_80025980` copies the active scene name
    /// `0x80084548` into the backup at `0x8007BAE8` before the minigame
    /// overlay clobbers the field.
    // REF: FUN_80025980 (scene-name backup half), FUN_801DE840 case 0x3E
    //      (winnings-accumulator zero half)
    pub fn arm_minigame_warp(&mut self) {
        self.minigame_scene_backup = Some(self.active_scene_label.clone());
        self.minigame_winnings = 0;
    }

    /// Mode-24 minigame exit / return-warp: restore the backed-up scene name
    /// into [`Self::active_scene_label`], commit the session winnings into
    /// the casino coin bank (`casino_coins += minigame_winnings`, saturating
    /// at the retail `9_999_999` cap), and drop back to [`SceneMode::Field`]
    /// (retail latches `_DAT_8007B83C = 2`, mode 2 MAIN INIT, whose
    /// per-scene initializer reloads the restored scene; the engine keeps
    /// the field state resident underneath its minigame sessions, so
    /// restoring the label + mode completes the same round trip without a
    /// reload).
    ///
    /// Distinct from the slot overlay's cash-out ([`Self::exit_slot_machine`],
    /// an *assignment* into the bank): this commit is a delta-add of the
    /// accumulator (`_DAT_800845A4 += _DAT_80084440`).
    ///
    /// The winnings commit runs even when no warp is armed (retail's add is
    /// unconditional); only the name restore needs the backup.
    // PORT: FUN_80026018
    pub fn minigame_return_warp(&mut self) {
        self.casino_coins = self
            .casino_coins
            .saturating_add(self.minigame_winnings)
            .min(9_999_999);
        if let Some(name) = self.minigame_scene_backup.take() {
            self.active_scene_label = name;
        }
        self.mode = SceneMode::Field;
    }

    /// Advance the slot machine one frame, reading this frame's pad:
    ///
    /// - **Idle**: a [`Cross`](input::PadButton::Cross) press charges the
    ///   flat bet (3 coins, 1 in feature modes) and spins - all three
    ///   paylines always play.
    /// - **Spinning**: the spin-up timer runs down on its own.
    /// - **Stopping**: a [`Cross`] press stops the leftmost live reel (host
    ///   simplification of the retail three stop buttons, pad bits
    ///   `0x80`/`0x40`/`0x20` → reels 0/1/2).
    /// - **Payout**: a [`Cross`] press collects the win into the balance.
    ///
    /// [`Cross`]: input::PadButton::Cross
    ///
    /// PORT: the slot overlay's per-frame driver (`FUN_801cf0d8` reel SM;
    /// the confirmed kernels live in [`crate::slot_machine`]).
    fn tick_slot_machine(&mut self) {
        use crate::slot_machine::SlotPhase;
        let Some(phase) = self.slot_machine.as_ref().map(|m| m.phase()) else {
            // Mode is SlotMachine but no session installed - drop back.
            self.mode = self.slot_return_mode;
            return;
        };
        let confirm = self.input.just_pressed(input::PadButton::Cross);
        let Some(m) = self.slot_machine.as_mut() else {
            return;
        };
        m.tick();
        match phase {
            SlotPhase::Idle => {
                if confirm {
                    m.spin();
                }
            }
            SlotPhase::Spinning => {}
            SlotPhase::Stopping => {
                if confirm {
                    m.stop_next_reel();
                }
            }
            SlotPhase::Payout => {
                if confirm {
                    m.collect();
                }
            }
            SlotPhase::CashedOut => {
                // Committed: restore the interrupted mode (the host reads the
                // session out via [`World::exit_slot_machine`]).
                self.mode = self.slot_return_mode;
            }
        }
    }

    /// Enter the Baka Fighter duel on `fight`, suspending the current scene
    /// mode (restored by [`World::exit_baka_fighter`]). Like the dance /
    /// fishing / slot / pause-menu suspend contract, the interrupted field
    /// state stays intact underneath.
    pub fn enter_baka_fighter(&mut self, fight: crate::baka_fighter::BakaFight) {
        if self.mode != SceneMode::BakaFighter {
            self.baka_return_mode = self.mode;
        }
        // Retail reaches the duel through the mode-24 door warp: the field-VM
        // `0x3E` arm zeroes the winnings accumulator `_DAT_80084440` and the
        // mode-24 OTHER-INIT `FUN_80025980` backs up the active scene name.
        // Only a field entry goes through that warp; an engine-only entry from
        // another mode keeps the plain suspend/restore contract.
        if self.baka_return_mode == SceneMode::Field {
            self.arm_minigame_warp();
        }
        self.baka_fighter = Some(fight);
        self.mode = SceneMode::BakaFighter;
    }

    /// Leave the Baka Fighter duel through the mode-24 return warp
    /// ([`Self::minigame_return_warp`], retail `FUN_80026018`): the winnings
    /// accumulator is banked into [`Self::casino_coins`], the backed-up scene
    /// name is restored and the mode drops back to the field.
    ///
    /// On a decided match with a player win, whatever prize the end-of-match
    /// tally has not yet drained is added to the accumulator first - retail's
    /// tally (`FUN_801D239C` at `0x801D28A8..0x801D28BC`) drains
    /// `DAT_801DBEE8` into `_DAT_80084440` a step at a time while the result
    /// screen is up, so leaving early has to bank the remainder for the total
    /// paid to match the prize either way.
    ///
    /// Returns the fight so the host can read the final state. No-op when no
    /// duel is active.
    pub fn exit_baka_fighter(&mut self) -> Option<crate::baka_fighter::BakaFight> {
        let fight = self.baka_fighter.take();
        if let Some(f) = fight.as_ref()
            && f.winner() == Some(0)
        {
            let owed = f.tally_gold_remaining().max(0) as u32;
            self.minigame_winnings = self.minigame_winnings.saturating_add(owed);
        }
        let return_mode = self.baka_return_mode;
        if self.mode == SceneMode::BakaFighter {
            // The warp's own mode write is retail's mode-2 (field) latch. An
            // engine-only entry from another mode restores that mode instead,
            // keeping the suspend contract the other minigames use.
            self.minigame_return_warp();
            if return_mode != SceneMode::Field {
                self.mode = return_mode;
            }
        }
        fight
    }

    /// Advance the Baka Fighter duel one frame, reading this frame's pad:
    ///
    /// - [`Left`](input::PadButton::Left) / [`Right`](input::PadButton::Right)
    ///   / [`Up`](input::PadButton::Up) commit attack types 1 / 2 / 3 for the
    ///   player slot (retail folds the face/shoulder mask bits
    ///   `0x80`/`0x20`/`0x40` into the same three types);
    ///   [`Down`](input::PadButton::Down) commits the special (type 4).
    /// - The CPU slot picks through the ported `FUN_801d487c` roll inside
    ///   [`crate::baka_fighter::BakaFight::tick`].
    /// - When the match is decided, a [`Cross`](input::PadButton::Cross)
    ///   press leaves the duel (via [`World::exit_baka_fighter`], crediting
    ///   the gold prize on a player win).
    ///
    /// PORT: the Baka Fighter per-frame drive (`FUN_801d3f44` player input →
    /// type commit; `FUN_801d3468` resolution SM via `BakaFight::tick`).
    fn tick_baka_fighter(&mut self) {
        use crate::baka_fighter::BakaAttack;
        let Some(fight) = self.baka_fighter.as_ref() else {
            // Mode is BakaFighter but no fight installed - drop back.
            self.mode = self.baka_return_mode;
            return;
        };
        if fight.match_over() {
            // The result screen: run the score tally, banking each drained
            // step into the mode-24 winnings accumulator exactly as retail's
            // `FUN_801D239C` adds it into `_DAT_80084440` - the coin prize,
            // not party gold (`0x8008459C`). The exit warp
            // ([`Self::minigame_return_warp`]) then pays the accumulator into
            // the casino coin bank. Any face button latches its fast-forward.
            let face = [
                input::PadButton::Triangle,
                input::PadButton::Circle,
                input::PadButton::Cross,
                input::PadButton::Square,
            ]
            .iter()
            .any(|&b| self.input.just_pressed(b));
            if let Some(f) = self.baka_fighter.as_mut() {
                f.tick_with_input(1, face);
                let paid = f.take_tally_gold();
                if paid > 0 {
                    self.minigame_winnings = self.minigame_winnings.saturating_add(paid as u32);
                }
            }
            if self.input.just_pressed(input::PadButton::Cross) {
                self.exit_baka_fighter();
            }
            return;
        }
        let attack = if self.input.just_pressed(input::PadButton::Left) {
            Some(BakaAttack::A)
        } else if self.input.just_pressed(input::PadButton::Right) {
            Some(BakaAttack::B)
        } else if self.input.just_pressed(input::PadButton::Up) {
            Some(BakaAttack::C)
        } else if self.input.just_pressed(input::PadButton::Down) {
            Some(BakaAttack::Special)
        } else {
            None
        };
        if let Some(fight) = self.baka_fighter.as_mut() {
            if let Some(attack) = attack {
                fight.choose(0, attack);
            }
            fight.tick(1);
        }
    }

    /// Enter the Muscle Dome contest on `session`, suspending the current
    /// scene mode (restored by [`World::exit_muscle_dome`]). Same suspend
    /// contract as the other minigames / the pause menu.
    pub fn enter_muscle_dome(&mut self, session: crate::muscle_dome::MuscleDomeSession) {
        if self.mode != SceneMode::MuscleDome {
            self.muscle_return_mode = self.mode;
        }
        self.muscle_dome = Some(session);
        self.mode = SceneMode::MuscleDome;
    }

    /// Leave the Muscle Dome and restore the interrupted mode.
    ///
    /// **A leg pays nothing.** The finished leg is reported to the open
    /// contest ([`World::report_muscle_leg`]), which is what decides whether
    /// the ladder carries on and what the run is eventually worth; a contest
    /// that has reached its end settles here and pays into the coin bank
    /// ([`World::settle_muscle_contest`]).
    ///
    /// It used to credit a Seru capture on a won leg, keyed off the victory
    /// caption's spell id. That was a misattribution: the caption table the
    /// id indexes is the shared battle-family cast-caption table, reached by
    /// any cast in any battle overlay, and the arena grants no Seru at all.
    ///
    /// Returns the session so the host can read the final state.
    pub fn exit_muscle_dome(&mut self) -> Option<crate::muscle_dome::MuscleDomeSession> {
        if self.mode == SceneMode::MuscleDome {
            self.mode = self.muscle_return_mode;
        }
        self.muscle_dome.take()
    }

    /// Report the finished leg to the open contest and step the between-leg
    /// hub, applying the HP the recovery lanes hand back to the lead fighter.
    ///
    /// This is the arena's own re-entry: the ladder advances one leg, the new
    /// `(course, round)` decodes out of the sub-id word, and the hub decides
    /// between staging another leg and settling. Returns the contest state it
    /// landed in, or `None` when no contest is open.
    ///
    /// PORT: FUN_801cea6c (contest re-entry) / FUN_801cf870 states 0x0A..0x0C
    pub fn report_muscle_leg(
        &mut self,
        report: crate::muscle_dome::LegReport,
    ) -> Option<crate::muscle_dome::ContestState> {
        use crate::muscle_dome::ContestState;
        let flags = self.muscle_contest_flags();
        let hp_max = self
            .roster
            .members
            .first()
            .map(|r| r.hp_mp_sp().hp_max)
            .filter(|&hp| hp > 0)
            .unwrap_or(500);
        let contest = self.muscle_contest.as_mut()?;
        contest.finish_leg(report, hp_max, &flags);
        // The three recovery lanes drain, then the restore state hands the
        // total back to the fighter - a dome contest costs no permanent HP.
        while matches!(
            contest.state(),
            ContestState::LegScore | ContestState::Tally | ContestState::Restore
        ) {
            let restoring = contest.state() == ContestState::Restore;
            contest.advance();
            if restoring {
                // The retail store is the game-state window's `+0x6CC` /
                // `+0x6CE` pair, which is the lead party record's own
                // `+0x104` / `+0x106` HP pair (`0x80084708 - 0x80084140 =
                // 0x5C8`).
                let mut hms = match self.roster.members.first() {
                    Some(r) => r.hp_mp_sp(),
                    None => break,
                };
                hms.hp_cur = self
                    .muscle_contest
                    .as_mut()?
                    .take_hp_restore(hms.hp_cur, hp_max);
                if let Some(rec) = self.roster.members.first_mut() {
                    rec.set_hp_mp_sp(hms);
                }
                return Some(self.muscle_contest.as_ref()?.state());
            }
        }
        Some(contest.state())
    }

    /// The story-flag reads the contest rules need, sampled off the system
    /// flag bank.
    pub fn muscle_contest_flags(&self) -> crate::muscle_dome::ContestFlags {
        use crate::muscle_dome as md;
        let mut flags = md::ContestFlags::default();
        for (i, &(id, _)) in md::COURSE_UNLOCK_FLAGS.iter().enumerate() {
            flags.course_unlock[i] = self.system_flag_test(id);
        }
        for (i, &(_, id)) in md::MASTER_LENGTH_GATES.iter().enumerate() {
            flags.master_gates[i] = self.system_flag_test(id);
        }
        flags.prize_awarded = self.system_flag_test(md::CONTEST_PRIZE_FLAG);
        flags
    }

    /// Settle the open contest: pay the tally into the casino coin bank,
    /// apply the flags the settlement names, and hand over the one-shot
    /// Master-course prize when it is due.
    ///
    /// The tally is the contest's whole reward. Returns the settlement, or
    /// `None` when no contest is open or it has not reached its end.
    ///
    /// PORT: FUN_801d0f60 / FUN_80026018 (the coin credit)
    pub fn settle_muscle_contest(&mut self) -> Option<crate::muscle_dome::ContestSettlement> {
        use crate::muscle_dome as md;
        let flags = self.muscle_contest_flags();
        let contest = self.muscle_contest.as_mut()?;
        if !contest.over() {
            return None;
        }
        let out = contest.settle(&flags);
        self.muscle_contest = None;
        self.muscle_settlement = Some(out);
        self.casino_coins = md::credit_casino_coins(self.casino_coins, out.score);
        if out.set_continue_flag {
            self.system_flag_set(md::CONTEST_CONTINUE_FLAG);
        } else {
            self.system_flag_clear(md::CONTEST_CONTINUE_FLAG);
        }
        if out.set_gave_up_flag {
            self.system_flag_set(md::CONTEST_GAVE_UP_FLAG);
        } else {
            self.system_flag_clear(md::CONTEST_GAVE_UP_FLAG);
        }
        if let Some(id) = out.set_ran_first_flag {
            self.system_flag_set(id);
        }
        if out.award_prize {
            self.system_flag_set(md::CONTEST_PRIZE_FLAG);
            let slot = self.inventory.entry(md::CONTEST_PRIZE_ITEM_ID).or_insert(0);
            *slot = slot.saturating_add(1).min(legaia_save::STACK_CAP);
        }
        Some(out)
    }

    /// Advance the Muscle Dome one frame, reading this frame's pad:
    ///
    /// - **Select**: [`Left`](input::PadButton::Left) /
    ///   [`Right`](input::PadButton::Right) / [`Up`](input::PadButton::Up) /
    ///   [`Down`](input::PadButton::Down) commit the four dealt directions
    ///   (the retail direction bits, in the `ctx+0x1114..+0x1120` slot
    ///   order); [`Cross`](input::PadButton::Cross) confirms the queue. The
    ///   opponent commits through the shared selection logic when the player
    ///   confirms.
    /// - **Resolve**: each side's whole queued string plays out through the
    ///   session's installed [`DomeDamageModel`] - the *shared* retail damage
    ///   kernel (move-power record → predamage roll → element affinity →
    ///   finisher, on the contest's PsyQ `rand()` stream), the same one the
    ///   browser host resolves with. A session with no model installed
    ///   resolves to no damage rather than to invented constants.
    /// - **TurnOver / decided**: the next turn is taken automatically (retail
    ///   confirms nothing at a turn boundary), and [`Cross`] leaves a
    ///   finished leg (via [`World::exit_muscle_dome`], crediting the reward
    ///   Seru capture on a win). A leg finishes on a KO and on nothing else:
    ///   turns are counted, never budgeted. Retail agrees - the arena hands
    ///   the round to an ordinary battle (`FUN_801D1510` sets game mode
    ///   `0x14`), and the only battle-end signal comes from the `0x5A`
    ///   end-of-action KO scans.
    ///
    /// [`DomeDamageModel`]: crate::muscle_dome::DomeDamageModel
    ///
    /// `FUN_801D0748` is the **battle overlay's** round / flow SM (context
    /// pointer `_DAT_8007BD24`, phase byte `ctx+6`), not a dome-specific
    /// controller: its 2781 instructions form none of the dome's own tables
    /// (`0x801F4B8C` and friends appear nowhere in it). It is reached here
    /// because a dome leg *is* an ordinary battle, so the reuse is the retail
    /// chain rather than a shape match.
    ///
    /// PORT: FUN_801d0748 (that round driver's phase loop: pick / commit /
    /// resolve), with the presentation left to the host.
    fn tick_muscle_dome(&mut self) {
        use crate::muscle_dome::MusclePhase;
        let Some(phase) = self.muscle_dome.as_ref().map(|s| s.phase()) else {
            self.mode = self.muscle_return_mode;
            return;
        };
        let confirm = self.input.just_pressed(input::PadButton::Cross);
        match phase {
            MusclePhase::Select => {
                let card = if self.input.just_pressed(input::PadButton::Left) {
                    Some(0)
                } else if self.input.just_pressed(input::PadButton::Right) {
                    Some(1)
                } else if self.input.just_pressed(input::PadButton::Up) {
                    Some(2)
                } else if self.input.just_pressed(input::PadButton::Down) {
                    Some(3)
                } else {
                    None
                };
                if let Some(s) = self.muscle_dome.as_mut() {
                    if let Some(card) = card {
                        s.commit_card(0, card);
                    }
                    if confirm {
                        s.ai_commit_all(1);
                        s.end_selection();
                    }
                }
            }
            MusclePhase::Resolve => {
                if let Some(s) = self.muscle_dome.as_mut() {
                    // With no disc tables staged this closes the turn without
                    // damage rather than substituting invented numbers - and
                    // rather than parking the leg in `Resolve` forever.
                    s.resolve_turn_or_zero();
                }
            }
            MusclePhase::TurnOver => {
                // Retail's turn boundary is automatic, not confirmed: the
                // battle-action SM writes `ctx[6] = 0x14` at `0x801E67F0` and
                // the round driver re-enters its own command cluster with no
                // press. The arena hub - the only thing that draws an
                // INTERVAL screen - runs in arena mode `0x18` and is not
                // executing during a leg, so a confirm gate here was a silent
                // one-press stall with nothing on screen to explain it.
                // REF: FUN_801e295c (turn-top arm)
                if let Some(s) = self.muscle_dome.as_mut() {
                    s.next_turn();
                }
            }
            MusclePhase::Won | MusclePhase::Lost => {
                if confirm {
                    let report = crate::muscle_dome::LegReport {
                        survived: phase == MusclePhase::Won,
                        outcome: 0,
                        turns_taken: self.muscle_dome.as_ref().map_or(0, |s| s.turn()),
                    };
                    self.exit_muscle_dome();
                    self.report_muscle_leg(report);
                    // A contest that has run out settles on the spot: the
                    // payout is the contest's, not the leg's.
                    self.settle_muscle_contest();
                }
            }
        }
    }
}
