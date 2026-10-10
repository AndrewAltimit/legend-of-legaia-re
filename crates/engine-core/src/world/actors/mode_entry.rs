//! The party roster and the mode entries that start from the field: the
//! present-party list, battle entry, world-map entry and the FMV cutscene
//! hand-off and return.
//! Split out of `actors.rs`; no logic change.

use super::*;

impl World {
    /// Resolve a battle/party ordinal (actor slot, HUD row, VRAM texture
    /// band) to the **roster slot** of the character occupying it, per
    /// [`crate::world::PartyState::active_party`]. Identity when no composition is installed
    /// or the ordinal runs past it - the historical slot-`i`-is-character-`i`
    /// behaviour every synthetic test relies on.
    pub fn party_roster_slot(&self, member: usize) -> usize {
        self.party
            .active_party
            .get(member)
            .map(|&s| s as usize)
            .unwrap_or(member)
    }

    /// The field party list - retail's `0x80084598` member ids for
    /// `DAT_80084594` entries - as the field VM's party ops see it.
    ///
    /// [`crate::world::PartyState::party_actor_slots`] carries it once a
    /// save or a party op has installed it. Before that (a New Game, or a
    /// save whose composition is the roster's identity order) the list is
    /// the installed battle composition: `active_party` when set, else the
    /// identity `0..party_count`. A list the party ops emptied stays empty
    /// ([`crate::world::PartyState::field_list_emptied`]).
    pub fn present_party_list(&self) -> Vec<u8> {
        if self.party.party_actor_slots.is_empty() && self.party.field_list_emptied {
            return Vec::new();
        }
        if !self.party.party_actor_slots.is_empty() {
            return self
                .party
                .party_actor_slots
                .iter()
                .flatten()
                .copied()
                .collect();
        }
        if !self.party.active_party.is_empty() {
            return self.party.active_party.clone();
        }
        (0..self.party.party_count.min(4)).collect()
    }

    /// Install `list` as the field party list and the battle composition
    /// together, as retail's party ops write the one list both read.
    /// An empty list clears the field list and leaves the battle
    /// composition as it was (no party of zero is ever fought with).
    pub fn install_present_party_list(&mut self, list: Vec<u8>) {
        self.party.party_actor_slots = list.iter().take(4).map(|&id| Some(id)).collect();
        self.party.field_list_emptied = list.is_empty();
        if !list.is_empty() {
            self.set_active_party(list);
        }
    }

    /// Install a present-party composition: `slots[i]` = roster slot for
    /// battle ordinal `i` (the engine mirror of retail's present-party
    /// list at `0x8007BD10`). The list caps at the 3 on-screen party
    /// positions (the runtime texture-band count). Sets
    /// [`crate::world::PartyState::party_count`] to the resulting length and, for each ordinal
    /// whose mapped roster record exists, reseeds the party actor's HP /
    /// MP / liveness / SPD mirror from it - the same projection
    /// [`Self::load_party`] performs for the identity mapping. Ordinals
    /// past the roster keep their live mirrors (zeroed-roster / synthetic
    /// setups render the character with default equipment, exactly like
    /// the identity default).
    pub fn set_active_party(&mut self, slots: Vec<u8>) {
        let mut active = slots;
        active.truncate(3);
        // Retail's New Game seeds all four live records from the SCUS
        // template (`0x80084708 + n*0x414`, the seed routine's four-iteration
        // loop), so a member who joins later already has a level-1 record.
        // The engine's New Game roster is Vahn alone; a join naming a slot it
        // lacks takes that slot's template row here, or the member would
        // fight with 0 / 0 HP and the battle could never see the party wiped.
        // Every slot up to the highest named one is filled, as retail's are:
        // a roster grown to reach slot 2 must not leave a zeroed slot 1.
        if let Some(tpl) = self.tables.starting_party.clone() {
            let top = active.iter().copied().max().unwrap_or(0);
            let missing: Vec<u8> = (0..=top)
                .filter(|&r| {
                    self.party
                        .roster
                        .members
                        .get(usize::from(r))
                        .is_none_or(|m| m.hp_mp_sp().hp_max == 0)
                })
                .collect();
            if !missing.is_empty() {
                self.seed_party_members(&tpl, &missing);
            }
        }
        for (member, &rslot) in active.iter().enumerate() {
            let Some(rec) = self.party.roster.members.get(rslot as usize) else {
                continue;
            };
            let hms = rec.hp_mp_sp();
            let activate = self.party_mirror_activates(member);
            if let Some(a) = self.actors.get_mut(member) {
                if activate {
                    a.active = true;
                }
                a.battle.hp = hms.hp_cur;
                a.battle.max_hp = hms.hp_max;
                a.battle.mp = hms.mp_cur;
                a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            }
            if let Some(s) = self.battle.speed.get_mut(member) {
                *s = rec.live_stats().spd;
            }
        }
        if !active.is_empty() {
            self.party.party_count = active.len() as u8;
        }
        self.party.active_party = active;
    }

    /// Place the world into [`SceneMode::Battle`] and populate the actor
    /// pointer table with `party_count` party slots followed by
    /// `monster_count` monster slots, mirroring the layout
    /// `FUN_800520F0` produces (slots 0..2 = party, 3..7 = monsters; total
    /// caps at 8). Actors are seated at the retail stage seats
    /// ([`crate::battle_seats`]): the party at negative Z facing the
    /// monsters at positive Z, both rows selected by combatant count
    /// exactly like the setup `FUN_800513F0`.
    ///
    /// This is the engine-core analogue of the retail battle scene
    /// loader's "stamp the actor table from the scene record" pre-pass.
    /// Engines that drive the loader from real scene data (party data +
    /// monster archive) skip this helper and write the slots directly;
    /// it's the convenience path for tests + the asset-viewer's
    /// `battle-scene` subcommand.
    ///
    /// The battle-action state machine is seeded at
    /// [`legaia_engine_vm::battle_action::ActionState::Begin`].
    // PORT: FUN_800513F0 (battle setup: seat stamping from the SCUS tables)
    pub fn enter_battle(&mut self, party_count: u8, monster_count: u8) {
        self.mode = SceneMode::Battle;
        self.battle.entry_serial = self.battle.entry_serial.wrapping_add(1);
        self.battle.monster_flee_attempted = false;
        // The battle scene setup re-seeds object-effect row 0
        // (`0x80055DDC..0x80055DF8`).
        // REF: FUN_80055B6C
        self.object_effect.reseed_for_battle();
        // The magic-level-up queue is a per-battle oracle record, not a host
        // hand-off: the banner the level-up raises is the battle message
        // banner (`raise_magic_level_banner`, screen element `0x65`), which
        // both hosts draw through `battle_hud::battle_banner_message`. No
        // host drains the queue, so it is bounded here - one battle's events
        // at most - instead of growing for the whole session.
        self.seru.magic_level_ups.clear();
        self.party.party_count = party_count.min(3);
        let monster_count = monster_count.min(5);
        let actor_count =
            ((self.party.party_count as usize) + (monster_count as usize)).min(MAX_ACTORS);
        for i in 0..(self.party.party_count as usize).min(actor_count) {
            let s = crate::battle_seats::party_seat(self.party.party_count, i);
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = s.x;
            actor.move_state.world_y = s.y;
            actor.move_state.world_z = s.z;
            actor.battle.liveness = 1;
            // Seated facing: the party faces the monster row (+Z = heading
            // 0 in the FUN_80019B28 convention). Overwritten by the SM's
            // per-action bearing writes once actions run.
            actor.battle.facing_angle = 0;
        }
        for i in (self.party.party_count as usize)..actor_count {
            let s = crate::battle_seats::monster_seat(
                monster_count,
                i - self.party.party_count as usize,
                false,
            );
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = s.x;
            actor.move_state.world_y = s.y;
            actor.move_state.world_z = s.z;
            actor.battle.liveness = 1;
            // Monsters face the party row (-Z = heading 0x800).
            actor.battle.facing_angle = 0x800;
        }
        // Every battle row past this fight's layout starts empty. The target
        // rows, the validator and the round walk all read slots
        // `party_count..party_count + 5` and count one as present by its
        // battle stats, so a slot the last fight (or the field) left carrying
        // stats seated a ghost enemy: a one-member party after a larger
        // layout faced its real monster plus stale rows that never die.
        // Retail's battle loader builds the actor table fresh per fight.
        for actor in self.actors.iter_mut().take(8).skip(actor_count) {
            actor.battle = Default::default();
            actor.battle_monster_id = None;
            actor.battle_element = None;
        }
        // Reset the battle ctx and seed at Begin via the public byte API to
        // avoid pulling battle_action::ActionState into world.rs imports.
        self.battle_ctx = vm::battle_action::BattleActionCtx::new();
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        // Battle init's ambient seed (`0x80051C70..0x80051C84`): the floor,
        // which the per-frame ramp lifts to the settled `0x80` while no cast
        // holds `ctx[+0x243]` - the fight's floor fades in from dark.
        self.battle_ctx.ambient_base = vm::battle_ground_grid::AMBIENT_BASE_FLOOR;
        self.battle.ambient_stored =
            vm::battle_ground_grid::ambient_base_rgb(vm::battle_ground_grid::AMBIENT_BASE_FLOOR);
        // Battle init spawns the backdrop records fresh: `+0x78` starts at 0.
        self.battle.backdrop_cue = 0;
        self.battle.end = None;
        // Effect pool is reused across scenes - reset to a fresh instance
        // (per-battle the head/free-list rebuilds from scratch). This is
        // retail's battle-loader init call (stage `0xE`, `0x80052670`): a
        // fresh pool is exactly the state `FUN_801DE914(0x1000, 0xA00)`
        // leaves, so `Pool::init_head` carries `REPLACED-BY` naming this line.
        // The rest of the battle-effect state goes with it - the mode switch
        // into battle runs the same actor-pool reset (`FUN_8001E1B4`) the
        // exit does.
        // REF: FUN_801DE914
        self.teardown_battle_effects();
        // Sparring fight: resolve the battle-stage id exactly as retail's
        // battle-entry tail does - default 0, and raise it to the tutorial
        // stage only when the disc's one-shot arm flag is set, consuming the
        // flag. `battle_tutorial_pending` is the separate debug force
        // (`World::prime_battle_tutorial`); both are evaluated so a forced
        // fight still consumes an armed flag rather than leaving it to fire
        // again on the next battle.
        self.battle.tutorial = None;
        self.battle.tutorial_boxes.clear();
        self.battle.flow = crate::battle_flow::BattleFlowState::Idle;
        self.battle.round_flow = crate::battle_round::RoundFlow::default();
        self.battle.commit_log_launch = None;
        self.battle.intro_names_frames = 0;
        // The per-fighter Auto flags and parked queues are battle state; the
        // disc inputs beside them are scene state and stay.
        self.battle.auto_combo.flags = [false; 3];
        self.battle.auto_combo.pending = false;
        self.battle.auto_combo.queues = Default::default();
        // `ctx[+0x289]` and the rest of the side-band state start at zero with
        // the rest of the battle context, as do the stage modules' own words.
        self.battle.sideband = Default::default();
        self.battle.sparring_round_pending = false;
        self.battle.arrival = Default::default();
        self.battle.form_transition = Default::default();
        // Battle init registers a fresh backdrop pair; any rebind is gone.
        self.battle.backdrop_rebound = false;
        self.battle.vram_moves.clear();
        self.battle.vram_scrolls.clear();
        self.battle.vram_loads = Default::default();
        self.battle.stage_camera = None;
        self.battle.stage_banner = None;
        // The entity SM's battle-entry tail writes the stage id: `0` in the
        // delay slot, raised to the tutorial stage by `arm_battle_tutorial`
        // when its arm fired (`0x801DA698..0x801DA6B0`). Battle init's
        // per-formation override is `enter_battle_from_formation`'s.
        self.battle.stage_id = 0;
        let armed_by_disc = self.take_battle_tutorial_arm();
        if self.battle.tutorial_pending || armed_by_disc {
            self.arm_battle_tutorial();
        }
    }

    /// Place the world into [`SceneMode::WorldMap`] and install a
    /// [`WorldMapController`] if one isn't already present. After this,
    /// [`World::tick`] drives the controller from the per-frame pad set
    /// via [`World::set_pad`] - scroll, azimuth, zoom, and the top-view
    /// debug toggle all respond to input through the engine tick rather
    /// than a host-side controller.
    ///
    /// Idempotent: re-entering world-map mode keeps the existing
    /// controller (and its accumulated camera state) instead of resetting
    /// it.
    pub fn enter_world_map(&mut self) {
        self.mode = SceneMode::WorldMap;
        if self.world_map.ctrl.is_none() {
            self.world_map.ctrl = Some(WorldMapController::new());
        }
    }

    /// Consume a pending field-VM FMV trigger and flip into the cutscene
    /// mode, mirroring retail's main mode dispatcher reading the
    /// next-game-mode global (`_DAT_8007B83C == 0x1A`, game mode 26) one
    /// frame after the field-VM op `0x4C 0xE2` writes it.
    ///
    /// Only fires from [`SceneMode::Field`] (the only mode that runs the
    /// field VM and so the only one that can set the trigger). The pending
    /// id is always drained; an id whose runtime FMV slot points at a
    /// cut/missing path ([`crate::cutscene::fmv_index_to_str_filename`]
    /// returns `None`) is a no-op transition - the field continues - which
    /// matches the engine's documented "treat a cut slot as a no-op" rule.
    pub(crate) fn maybe_enter_pending_cutscene(&mut self) {
        let Some(fmv_id) = self.cutscene.pending_fmv_trigger.take() else {
            return;
        };
        if self.mode != SceneMode::Field {
            return;
        }
        if crate::cutscene::fmv_index_to_str_filename(fmv_id).is_some() {
            self.cutscene.return_mode = Some(self.mode);
            self.mode = SceneMode::Cutscene;
            self.cutscene.active_fmv = Some(fmv_id);
        }
    }

    /// The FMV index currently playing in [`SceneMode::Cutscene`], or `None`
    /// when no STR FMV is active. Hosts poll this after [`World::tick`] to
    /// learn which `MV*.STR` to open.
    pub fn active_fmv(&self) -> Option<i16> {
        self.cutscene.active_fmv
    }

    /// The retail `MV*.STR` path of the active cutscene FMV, or `None` when
    /// no STR FMV is active. Convenience over
    /// [`crate::cutscene::fmv_index_to_str_filename`].
    pub fn active_fmv_str_filename(&self) -> Option<&'static str> {
        self.cutscene
            .active_fmv
            .and_then(crate::cutscene::fmv_index_to_str_filename)
    }

    /// End the active STR-FMV cutscene and return to the scene mode that was
    /// live when it started (the field, in the normal flow). Retail returns
    /// here when the cutscene/MDEC overlay finishes playback and unloads.
    ///
    /// The field VM resumes from where it paused - its program counter is
    /// already past the FMV op, so the next field tick continues the script.
    /// A no-op when no cutscene is active.
    ///
    /// Retail's master dispatch (`FUN_801CEA3C`) does NOT return to the
    /// trigger scene for mid-game FMVs - it copies a CDNAME label from the
    /// seven-entry list at `0x801CE8AC` into the next-scene name global
    /// `0x80084548` (+ spawn/door word `0x80084540`), e.g. `town01` triggers
    /// fmv 1 and lands in `town0b`. That transfer needs the host's asset
    /// index, so this parks the finished id in [`crate::world::CutsceneState::finished_fmv`] and
    /// [`crate::scene::SceneHost::apply_pending_fmv_handoff`] performs it -
    /// one drain, whichever host polls.
    // REF: FUN_801CEA3C
    pub fn finish_cutscene(&mut self) {
        if self.mode == SceneMode::Cutscene {
            self.mode = self.cutscene.return_mode.take().unwrap_or(SceneMode::Field);
            self.cutscene.finished_fmv = self.cutscene.active_fmv;
            self.cutscene.active_fmv = None;
        }
    }

    /// Drain the id parked by [`World::finish_cutscene`]. `None` when no FMV
    /// has finished since the last drain.
    ///
    /// The world half of the post-play hand-off: a host with no scene loader
    /// (a headless world test, the `sim-trace` emitter) can consume the edge
    /// without one, and the scene host's
    /// [`apply_pending_fmv_handoff`](crate::scene::SceneHost::apply_pending_fmv_handoff)
    /// is the only production caller. `take` semantics are what stop two
    /// hosts - or one host polling twice - from transferring control twice.
    pub fn take_finished_fmv(&mut self) -> Option<i16> {
        self.cutscene.finished_fmv.take()
    }
}
