//! The three-actor talk: the leader-switch request, the per-frame tick and its
//! teardown.

use super::*;

impl World {
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
    /// The SM kernel is [`legaia_engine_field::leader_swap::LeaderSwap`]
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
    ///   parked at the `0x3F80` sentinel, the fade-back-in spawns, and the
    ///   talk's own record is queued as an op-`0x44` spawn (`FUN_8003BDE0`).
    /// - **3** - release the fade object (engine: [`crate::world::ScreenFxState::fade`]
    ///   steps itself; nothing to do).
    /// - **4** - hold the fade-in, then clear the camera-busy latch and
    ///   return to state 0 (the poll).
    /// - **5** - despawn = [`Self::end_three_actor_talk`]. The engine folds
    ///   retail's one-frame 0 -> 5 despawn delay into the same tick.
    ///
    /// Not modelled: `FUN_801DE190` (party-display rebuild - the engine
    /// derives the display from `party_actor_slots`) and the grid re-anchor
    /// pair `FUN_801DE3E0`/`FUN_801DB8EC`/`FUN_801DAA50` (the engine camera
    /// follows the player actor).
    pub(crate) fn tick_three_actor_talk(&mut self) {
        use legaia_engine_field::leader_swap::{
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
        let leader = self.field_leader().min(2);
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
                    self.presentation.fade = Some(legaia_engine_system::fade::FadeState::load(
                        &legaia_engine_system::fade::FadeTemplate {
                            kind: 2,
                            duration: legaia_engine_field::leader_swap::LEADER_SWAP_FADE_FRAMES
                                as i16,
                            start_rgb: [0, 0, 0],
                            end_rgb: [0xFF, 0xFF, 0xFF],
                            mode: [0, -1, 0],
                        },
                    ));
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
                    self.presentation.fade = Some(legaia_engine_system::fade::FadeState::load(
                        &legaia_engine_system::fade::FadeTemplate {
                            kind: 2,
                            duration: legaia_engine_field::leader_swap::LEADER_SWAP_FADE_FRAMES
                                as i16,
                            start_rgb: [0xFF, 0xFF, 0xFF],
                            end_rgb: [0, 0, 0],
                            mode: [0, 0, 0],
                        },
                    ));
                    // `801d2c58..801d2c78`: `FUN_8003BDE0(x, z, +0x72, 1)`,
                    // the talk's own record spawn. `+0x72` is the
                    // instruction's record byte less the MAN header's first
                    // two partition counts (`FUN_801D2D38`, `0x801D2DA8..
                    // 0x801D2DD0`), and the spawner's index mode adds those
                    // counts back (`0x8003BE74..0x8003BEBC`), so the record
                    // spawned is the byte as written: a global index, the
                    // op-`0x44` space. `nilboa`'s Three Tunnels name `0x5F`,
                    // P2[17], which re-dresses the map for the incoming
                    // leader's ravine - the boulders stand only in the Fire
                    // Ravine. The spawner checks the record's gates itself.
                    // REF: FUN_8003BDE0
                    if self.field_vm.pending_record_spawns.len()
                        < crate::world::SPAWNED_CONTEXT_SLOTS
                    {
                        self.field_vm.pending_record_spawns.push(talk.duration);
                    }
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
        legaia_engine_field::field_channels::resolve_target(view, id)
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
}
