//! The composite [`World`] struct definition and its core `impl` blocks
//! (`Default` + constructor/dispatch) extracted verbatim from `world.rs`.

use super::*;

/// Singleton world / scene held by an engine integration.
///
/// Holds the actor table, the active battle-action ctx (when the scene mode
/// is [`SceneMode::Battle`]), the shared effect-VM pool, and the rotation
/// LUTs / RNG state used by the move-VM ports.
///
/// The `Host` trait impls live on a thin `WorldHost<'_>` borrow to keep
/// borrow-checker complexity manageable - see [`World::with_host`].
pub struct World {
    pub mode: SceneMode,
    pub actors: Vec<Actor>,
    pub battle_ctx: BattleActionCtx,
    pub effect_pool: Pool,
    /// Script catalog for the effect VM. Populated at battle-enter time
    /// from PROT 873 (`efect.dat`) pack1 data via
    /// [`legaia_engine_vm::effect_vm::EffectCatalog::from_pack1_bytes`].
    /// An empty catalog is safe - `BattleHostImpl::ui_element` spawns
    /// nothing until a real catalog is wired. Set via
    /// [`crate::scene::SceneHost::set_effect_catalog`].
    pub effect_catalog: vm::effect_vm::EffectCatalog,
    /// Dev-spawned synthetic effects ([`World::spawn_debug_effect`] /
    /// [`World::spawn_debug_effect_model`]) - engine-side visualization
    /// aids kept outside the retail effect pool, aged by
    /// [`World::tick_effects`] over a fixed budget.
    pub debug_effects: Vec<DebugEffect>,
    /// Field VM execution context. Live in `SceneMode::Field` and
    /// `SceneMode::Cutscene` (cutscenes are field scenes that suppress
    /// player input via context flags).
    pub field_ctx: FieldCtx,
    /// Field VM bytecode buffer. Engines load this from a scene's PROT
    /// asset bundle when entering a field scene; `field_pc` indexes it.
    pub field_bytecode: Vec<u8>,
    /// Current field-VM PC. Updated by `tick()` based on the StepResult.
    pub field_pc: usize,
    /// Move-VM globals: per-actor bytecode and buffer pools, the shared predicate / counter / slot-table words, scratchpad ramp targets and the per-tick outcomes.
    pub move_vm: MoveVmGlobals,
    /// Per-scene field terrain: walkability grid, map-region block, zone table, floor-height LUT, object cells, elevation overrides and the region / tile trackers.
    pub terrain: FieldTerrain,
    /// Player actor slot - when `Some(slot)`, ext sub-ops 0x06 / 0x07 / 0x2A
    /// / 0x36 / 0x39 read `actors[slot].move_state.world_{x,y,z}` as the
    /// player position. `None` falls back to the origin (default impl).
    pub player_actor_slot: Option<u8>,
    /// Player field-locomotion state: run / slow / precise-movement gates, step deltas, ledge hop, vertical settle, wall probes and the per-tick movement cues.
    pub locomotion: FieldLocomotion,
    /// Field NPC state: positions, headings, routes, motions, ambient anims, dialog bindings and the solid / animate toggles.
    pub npcs: FieldNpcState,
    /// Engine behaviour toggles: the live gameplay loop, VM-driven dialogue, damage finish, monster targeting, select-attack option, flashing reduction and the entry pulse gate.
    pub toggles: WorldToggles,
    /// Party + save-game state: roster, active party and leader, money, inventory, ability masks, tactical arts, level-up tracking, banners, per-character save extensions and name entry.
    pub party: PartyState,
    /// Full-screen presentation state: fades, tints, the screen-effect widget host and the cinematic bars.
    pub presentation: ScreenFxState,
    /// Story / system flag words: the retail flag arrays and the story-flag bit image the scripts test and set.
    pub flags: StoryFlagState,
    /// The field fog-particle pool (`_DAT_8007B7E0`) the ambient emitter
    /// spawns into and the render pass draws from - see
    /// [`crate::fog_particles`].
    pub fog: crate::fog_particles::FogPool,
    /// Field script actors: op `0x43` scripted arcs and the NPC height
    /// channel they write, and op `0x34` sub-1 attached lights.
    pub script_actors: FieldScriptActorState,

    /// PRNG state consumed by every VM that calls `host.rng()`. Default uses
    /// a deterministic LCG so tests are reproducible.
    pub rng_state: u32,

    /// The sine view of retail's one SCUS trig table, the table
    /// `_DAT_8007B81C` points at (`0x80070A2C`, installed at boot by
    /// `FUN_80026BE0`): `0x1000` entries of `trunc(sin(i * 2pi / 4096) *
    /// 4096)`. Read by move-VM op `0x03` (the X term) and the world-map
    /// horizon emitter `FUN_801D7EA0`. Filled by [`World::new`] from
    /// [`crate::action_effect_script::retail_rotation_lut`], byte-exact
    /// with the disc table.
    pub sin_lut: Vec<i16>,
    /// The cosine view, `_DAT_8007B7F8` = the same table `+0x400` entries
    /// on. Read by move-VM op `0x03` (the Z term).
    pub cos_lut: Vec<i16>,

    /// Live battle session state: per-seat stat arrays, command / submenu sessions, flow + round state, tutorial, intro transition, escape timer, buffs, hit / effect queues and the end-of-battle latches.
    pub battle: BattleState,

    /// Field camera rig: the saved camera snapshot, scene offset + ease, shake amplitude and the zone-ramp register file.
    pub camera: CameraRig,

    /// Audio-side state: BGM selection and resume, the SFX cue / delay slots, sound-bank handshakes and the battle SFX / XA / shout cue queues.
    pub audio: AudioState,

    /// Disc-parsed static tables installed at boot / scene load (items, spells, arts, monsters, formations, move power, equipment, thresholds, CDNAME map).
    pub tables: DiscTables,

    /// Field-VM execution state beyond the main context: per-record channels, helper contexts, spawn requests, the op-0x49 submode block / screen, eased moves and the entry / free-roam latches.
    pub field_vm: FieldVmState,

    /// Pending field-VM scene transition (`scene_transition(map_id)` was
    /// called this frame). Drained by [`crate::scene::SceneHost::tick`]:
    /// when `Some(map_id)`, the host resolves the map id to a scene name,
    /// loads it, and reinitialises the field VM. `None` between transitions.
    pub pending_scene_transition: Option<u8>,

    /// Pending **named** scene transition (field-VM op `0x3F`, the named
    /// scene-change). When `Some`, the op carried the destination scene name
    /// inline (no map-id resolver needed); [`crate::scene::SceneHost::tick`]
    /// drains it and loads that scene directly. Fields: `(scene, entry_x,
    /// entry_z)` - the entry-tile bytes are kept for future destination
    /// spawn-point wiring. `None` between transitions.
    pub pending_named_scene_transition: Option<(String, u8, u8, u8)>,

    /// Cutscene presentation state: narration, timeline, caption / card / balloon overlays, FMV handoff and the opening-chain latches.
    pub cutscene: CutsceneState,

    /// Random / scripted encounter state: the per-scene encounter session, the scripted-encounter arm and the roll gates.
    pub encounters: EncounterState,

    /// Field-VM side-effects emitted this frame. Engines drain after
    /// [`World::tick`] to dispatch BGM, dialog, money, party, camera, etc.
    /// Mirror of the `FieldHost` callbacks - see [`FieldEvent`] for the
    /// per-variant citation.
    ///
    /// [`FieldEvent`]: crate::field_events::FieldEvent
    pub pending_field_events: Vec<FieldEvent>,

    /// Pending actor-spawn requests emitted by field-VM op `0x4C 0x80`
    /// (the actor allocator). Each entry is one child-actor's bytecode
    /// stream, split out of the parent script's `tail` via the retail
    /// `FUN_8003CA38` packet-length walker. Engines drain this through
    /// [`Self::drain_actor_spawns`] after [`Self::tick`] and route each
    /// record into their own actor pool - the retail engine mallocs a
    /// per-actor vertex pool and stores the record pointer at
    /// `actor[+0x90]`; the port leaves that policy to the
    /// engine that consumes the request.
    pub pending_actor_spawns: Vec<Vec<u8>>,

    /// Battle action state machine side-effects emitted this frame.
    /// Engines drain after [`World::tick`] to dispatch poses, UI elements,
    /// damage, screen-shake, etc. See [`BattleEvent`] for the per-variant
    /// citation.
    ///
    /// [`BattleEvent`]: crate::battle_events::BattleEvent
    pub pending_battle_events: Vec<BattleEvent>,

    /// Field dialogue state: the simplified dialog panel, the inline field-VM dialogue runner and the interact / talk latches.
    pub dialog: DialogState,

    /// Field props + triggers: prop colliders and bank, walk-touch records, the scene's move-VM stager tables, boss stagers, live field effects and the resolved cold spawn.
    pub props: FieldPropState,

    /// Frame counter incremented every [`World::tick`].
    pub frame: u64,

    /// Per-frame pad / stick input snapshot. Hosts call
    /// [`World::set_pad`] (or write directly via `input.set_pad`) before
    /// each [`World::tick`]; subsystems that consume input read it from
    /// here. Default-constructed [`InputState`] = no buttons held.
    ///
    /// Consumed in the world-tick path by: field free-movement locomotion
    /// ([`Self::step_field_locomotion`]), the tile-board walk SM
    /// (`Self::tick_tile_board`), the world-map controller
    /// ([`Self::enter_world_map`]), and the field-VM dialog-advance poll.
    /// Hosts that drive a scripted timeline (`legaia-engine replay`, the
    /// v0.1 playthrough oracle) thread recorded `j-replay-v1` pad events
    /// here via [`Self::set_pad`] before each tick. Menu navigation still
    /// runs through the host-side `play-window` loop.
    pub input: input::InputState,

    /// Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
    pub world_map: WorldMapState,

    /// Tile-board grid-mode state (the op-0x49 puzzle board, not town locomotion).
    pub board: TileBoardState,

    /// Minigame sessions (dance, fishing, slot machine, Baka Fighter, Muscle Dome) plus the casino coin / point-card wallet.
    pub minigames: MinigameState,

    /// Seru capture + magic-learning state: the capture log and registry, this battle's captures, shiny rolls and magic level-ups.
    pub seru: SeruState,

    /// Frame clock: the adaptive frame-step factor and its telemetry, the vsync accumulators, the sim-tick / display-frame counters and play time.
    pub clock: FrameClock,

    /// Town shop + prize-exchange session state (the gold shop and the casino / fishing prize counter).
    pub shops: ShopState,

    /// Pause-menu runtime state: disc-parsed text / widget tables and the pending warp / escape requests.
    pub menu: MenuState,

    /// CDNAME label of the active scene, if any. Set by
    /// [`World::set_active_scene_label`] on scene-load and consumed by
    /// engine-side helpers ([`World::install_encounter_for_scene`] reads
    /// this when it's called with the empty string, the encounter HUD
    /// surfaces it for diagnostics, etc.). Empty when no scene is loaded.
    pub active_scene_label: String,

    /// VDF ("set_mime", asset type `0x07`) buffer for the active scene.
    /// Layout `[u32 count][u32 byte_offsets[count]][body...]` mirrors the
    /// retail `DAT_8007B7DC` buffer the asset-dispatcher case 7 builds
    /// (see `project_vdf_buffer_and_parallel_table.md` for byte-level
    /// detail). The buffer holds the spawnable actor templates the field
    /// VM's `0x4C 0xD8` opcode resolves via [`World::vdf_record_bytes`].
    ///
    /// `None` when no scene is loaded or the scene carries no VDF chunk
    /// (most utility / cutscene scenes don't). Populated by
    /// [`crate::scene::SceneHost::enter_field_scene`] from the first
    /// asset-type-7 chunk found in the scene's streaming entries.
    pub vdf_buffer: Option<Vec<u8>>,

    /// Global TMD-pointer pool indexed by `tmd_idx`. Mirrors retail
    /// `DAT_8007C018` (the 143-entry homogeneous TMD pointer table in
    /// steady-state - see `project_dat_8007c018_global_tmd_table.md`).
    /// `None` at indices the active loader chain hasn't populated; the
    /// vector grows on demand through [`Self::set_global_tmd`].
    ///
    /// Seeded by [`crate::scene::SceneHost::enter_field_scene`] with the
    /// 5 character-mesh TMDs from PROT 0874 section 0 (byte-equality
    /// verified in `project_global_tmd_pool_source.md`). Producers of the
    /// other 138 kingdom-derived entries are not yet pinned; those slots
    /// stay `None` until the full chain lands.
    ///
    /// Read by the tile-board install's allocator. The field-VM `0x4C 0xD8`
    /// hook resolves through [`Self::field_scene_bank`] instead: its operand
    /// is a scene-bank index ([`Self::field_pool_tmd`]).
    pub global_tmd_pool: Vec<Option<Arc<GlobalTmd>>>,

    /// The current field scene's **model bank**: pool slots
    /// `crate::model_bank::SCENE_BANK_BASE..` of retail `DAT_8007C018`, in
    /// registration order (index `i` = pool slot `5 + i`). Installed by
    /// [`crate::scene::SceneHost::enter_field_scene`] from its
    /// [`crate::model_bank::SceneModelBank`]; read by
    /// [`Self::field_pool_tmd`]. Distinct from [`Self::global_tmd_pool`],
    /// which the engine seeds with the battle effect-model library - the two
    /// are the same retail table at different times, and a field operand
    /// must not resolve against the battle half.
    pub field_scene_bank: Vec<Option<Arc<GlobalTmd>>>,

    /// Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
    pub casting: CastFxState,

    /// Field ambient animation state: the CLUT-walk / CLUT-cell cyclers, VDF pulse, script VRAM moves and their vsync accumulators.
    pub ambient: AmbientFxState,

    /// The scene control block `_DAT_801C6EA4` (`0x64` bytes), re-allocated
    /// and reset on every scene load.
    ///
    /// REF: FUN_8003A024
    pub scene_control_block: crate::scus_leaf_kernels::SceneControlBlockReset,

    /// The system flags the active scene's own field-VM records SET on
    /// their way into a `3E FF <row>` scripted battle entry, each paired
    /// with that row ([`crate::man_field_scripts::BattleEntryArm`]), read
    /// off the MAN when the scene's carriers are installed. The disc-side
    /// evidence a direct `--battle <row>` entry consults to replay the arm
    /// the row's own record raises ([`World::replay_scripted_battle_arm`]).
    pub scene_battle_entry_arms: Vec<crate::man_field_scripts::BattleEntryArm>,

    /// Set when a battle resolves to [`BattleEndCause::PartyWipe`]. Hosts
    /// read it to raise their defeat state (native
    /// `BootUiState::GameOver`, the browser's game-over overlay) and clear it
    /// when the player picks an outcome.
    pub game_over: bool,

    /// Set by [`World::finish_battle`]'s party-wipe arm: the field restore
    /// (actor table, scene mode, step tracking) is deferred so hosts hold the
    /// final battle frame through the game-over hand-off, mirroring retail's
    /// frozen frame while mode 22 CARD INIT streams the menu overlay. Cleared
    /// by [`World::resolve_game_over_hold`], which performs the deferred
    /// restore - hosts call it when their `GameOverSession` resolves.
    pub game_over_hold: bool,

    /// Field state captured at the `Field -> Battle` transition so the live
    /// loop can restore it on victory. The retail engine re-enters the field
    /// scene from scratch; the port's loop snapshots the actor table +
    /// player slot instead. `None` outside battle. Managed by the live loop;
    /// hosts read [`Self::mode`] / [`crate::world::BattleState::active_formation`] instead.
    pub field_return: Option<FieldReturnState>,

    /// Field-scene carrier entities: the per-entity FUN_801DA51C state machines ticked in field scenes and their battle / engage handoffs.
    pub carriers: FieldCarrierState,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// Build a fresh world with `MAX_ACTORS` empty slots.
    pub fn new() -> Self {
        Self {
            mode: SceneMode::default(),
            actors: (0..MAX_ACTORS).map(|_| Actor::new()).collect(),
            battle_ctx: BattleActionCtx::new(),
            effect_pool: Pool::new(),
            effect_catalog: vm::effect_vm::EffectCatalog::default(),
            debug_effects: Vec::new(),
            field_ctx: FieldCtx::default(),
            field_bytecode: Vec::new(),
            field_pc: 0,
            move_vm: MoveVmGlobals::new(),
            terrain: FieldTerrain::new(),
            player_actor_slot: None,
            cutscene: CutsceneState::new(),
            locomotion: FieldLocomotion::new(),
            npcs: FieldNpcState::new(),
            toggles: WorldToggles::new(),
            party: PartyState::new(),
            presentation: ScreenFxState::new(),
            flags: StoryFlagState::new(),
            fog: crate::fog_particles::FogPool::new(),
            script_actors: FieldScriptActorState::default(),
            rng_state: 0x1234_5678,
            casting: CastFxState::new(),
            sin_lut: crate::action_effect_script::retail_rotation_lut()
                .sin_table()
                .to_vec(),
            cos_lut: crate::action_effect_script::retail_rotation_lut()
                .cos_table()
                .to_vec(),
            battle: BattleState::new(),
            camera: CameraRig::new(),
            audio: AudioState::new(),
            field_vm: FieldVmState::new(),
            pending_scene_transition: None,
            pending_named_scene_transition: None,
            encounters: EncounterState::new(),
            pending_field_events: Vec::new(),
            pending_actor_spawns: Vec::new(),
            pending_battle_events: Vec::new(),
            dialog: DialogState::new(),
            props: FieldPropState::new(),
            frame: 0,
            input: input::InputState::default(),
            world_map: WorldMapState::new(),
            board: TileBoardState::new(),
            minigames: MinigameState::new(),
            tables: DiscTables::new(),
            seru: SeruState::new(),
            clock: FrameClock::new(),
            ambient: AmbientFxState::new(),
            scene_control_block: crate::scus_leaf_kernels::SCENE_CONTROL_BLOCK_RESET,
            shops: ShopState::new(),
            menu: MenuState::new(),
            active_scene_label: String::new(),
            vdf_buffer: None,
            global_tmd_pool: Vec::new(),
            field_scene_bank: Vec::new(),
            scene_battle_entry_arms: Vec::new(),
            game_over: false,
            game_over_hold: false,
            field_return: None,
            carriers: FieldCarrierState::new(),
        }
    }

    /// Establish a fresh-game slate and enter the field per-frame mode.
    ///
    /// This is the engine's analog of the retail title-screen NEW GAME
    /// transition. In retail, confirming NEW GAME writes the master
    /// game-mode word `_DAT_8007B83C = 2` (field INIT, `FUN_80025B64`),
    /// whose per-scene initializer `FUN_801D6704` loads the map and then
    /// hands off to mode 3 (field per-frame) by writing
    /// `_DAT_8007B83C = 3`. See `docs/subsystems/boot.md` ("New Game boot
    /// chain") and `crates/engine-vm/src/title_overlay.rs`
    /// (`MASTER_GAME_MODE_FIELD_LAUNCH` / `MASTER_GAME_MODE_FIELD_RUN`).
    ///
    /// Here that collapses to: clear the unambiguous new-game-owned state
    /// (story flags, money, inventory, and any pending transitions left
    /// over from a prior session) and set [`SceneMode::Field`] - the
    /// engine's mapping of master mode 3. Distinct from the Continue path,
    /// which instead hydrates the world from a save slot.
    ///
    /// Gold and the story-flag clear mirror the retail new-game data-init
    /// `FUN_80034A6C`: it zeroes the story-flag region and writes party gold
    /// (`_DAT_8008459C`) to a hardcoded [`NEW_GAME_STARTING_GOLD`] = 500. The
    /// starting party stats come from [`World::seed_starting_party`] (the
    /// `FUN_800560B4` template expansion `FUN_80034A6C` calls), which a caller
    /// with the disc's `SCUS_942.54` invokes right after this to drop Vahn into
    /// slot 0. Retail's front-end (`FUN_801DD35C`) goes title-menu -> fade ->
    /// `init_game` -> master-mode 2 (field) directly, with no narration or
    /// name-entry sub-mode; `init_game` sets the opening scene to `opdeene` (the
    /// prologue cutscene), which hands off to `town01` (Rim Elm). The opening
    /// narration and the name-entry screen ("Select your name." character grid)
    /// are downstream field/event/menu-overlay steps after the field launches,
    /// not modeled here yet; this seed just copies the template's default name
    /// (`Vahn`).
    /// Stage the world for a **free-roam picker entry** - the scene-picker /
    /// `--scene` paths that drop into a scene with no story behind them.
    /// Retail has no such entry: every visit arrives with the story state a
    /// playthrough accumulated, and scene scripts assume it. Two consequences
    /// a cold picker entry gets wrong, both repaired here:
    ///
    /// - **Entry-script BGM pauses park forever.** town01's entry script
    ///   starts the town theme then pauses it while flag `0x225` is clear
    ///   (the opening's silent dawn; `P1[0]` `+0x5D..+0x91`); retail's
    ///   opening records repair it with their own sub-9 starts, which a
    ///   picker visit never runs. [`crate::world::FieldVmState::free_roam_staging`] lets the BGM
    ///   host arm drop a pause issued inside the entry window (see
    ///   `op35_bgm` in `vm_hosts`).
    /// - **Story-twin scenes present the wrong world event.** `town0c` is
    ///   post-Mist Rim Elm: flag `0x147` seats the blown-gate rock debris
    ///   and parks the intact wall/doorway pieces (the same records exist in
    ///   `town01` - the flag decides, not the scene). Curated per-scene
    ///   seeds below stage each twin at its canonical visit.
    ///
    /// # The south gate takes TWO flags, not one
    ///
    /// Rim Elm's south gate is cut open by the gate object's bind script
    /// (`town01`/`town0c` `P0[20]`, bound to the object at tile `(23, 43)` by
    /// the `.MAP` gate-0 kind-1 trigger and run by the scene-init bind
    /// prologue `FUN_8003A55C`). It makes three unconditional `4C 70` clears
    /// and then branches on **`0x147` (327) and `0x141` (321) together**; only
    /// the both-set arm runs `4C 70 18 2D 19 2E` (cols 24..25, rows 46..47),
    /// which cuts collision-grid row 47 and opens the doorway. Cold = sealed;
    /// `0x147` alone = still blocked, just further north; both = open at cols
    /// 24/25 with col 26 correctly still walled.
    ///
    /// So `town0c` seeds both. Seeding `0x147` on its own produced a staged
    /// world no playthrough can reach - the rubble of a blown gate in front of
    /// a doorway you cannot walk through.
    ///
    /// `town01` deliberately seeds **neither**. `0x147` is the same flag that
    /// seats the rock debris, and pre-Mist Rim Elm's authored appearance is
    /// rocks hidden / doorway intact / gate shut - which is exactly what the
    /// cold-entry oracles pin (`field_object_visibility_disc.rs` asserts
    /// `P0[18..21]` are story-hidden while `0x147` is clear, and
    /// `free_roam_staging_disc.rs` asserts picking `town01` after `town0c`
    /// re-hides them). A "usable" pre-Mist south gate would have to come from
    /// somewhere other than this flag pair.
    ///
    /// Managed flags reset first, so picking scenes in any order never leaks
    /// one scene's staging into the next. The new-game / opening chain must
    /// NOT call this - the opening's authored silence and pre-event
    /// presentation are the point there.
    pub fn seed_free_roam_story_baseline(&mut self, scene: &str) {
        self.field_vm.free_roam_staging = true;
        self.field_vm.free_roam_entry_frame = self.clock.display_frames;
        // Managed story-event flags (reset-then-seed).
        self.system_flag_clear(0x147);
        self.system_flag_clear(0x141);
        if scene == "town0c" {
            // Juggernaut blew the south gate: rocks out, intact door parked,
            // and - with the second half of the gate script's `and` - the
            // doorway itself actually cut open.
            self.system_flag_set(0x147);
            self.system_flag_set(0x141);
        }
    }

    // REF: FUN_80025B64
    // REF: FUN_801D6704
    // REF: FUN_801DD35C
    // REF: FUN_80034A6C
    // REF: FUN_800560B4
    // REF: FUN_8004F0E8
    pub fn begin_new_game(&mut self) {
        self.flags.story_flags = 0;
        self.flags.story_flag_bits.clear();
        // A NEW GAME is the opening chain, not a free-roam picker visit: the
        // authored entry pauses / pre-event scenery are the point. Any flags
        // an earlier picker staging seeded reset with the bank.
        self.field_vm.free_roam_staging = false;
        self.flags.system_flags.clear();
        self.party.money = NEW_GAME_STARTING_GOLD;
        self.minigames.point_card = 0;
        self.party.inventory.clear();
        self.pending_scene_transition = None;
        self.pending_named_scene_transition = None;
        self.cutscene.pending_fmv_trigger = None;
        self.encounters.pending_scripted = None;
        self.encounters.scripted_armed = false;
        self.encounters.scripted_formation_pending = false;
        // Reset the encounter session rather than dropping it. Dropping it
        // left the per-region trackers installed with their sink gone: a
        // region roll is destructive (it draws RNG, latches the pick and
        // re-seeds the counter *before* returning), so every roll after a New
        // Game was a fight that happened and was discarded, and
        // `scene_can_roll_encounters` still answered `true` off the stale
        // cache. The runtime self-heals now (`World::on_field_step` installs a
        // bracket), but a live session must survive the reset for the scene's
        // own table to keep driving it.
        if let Some(session) = self.encounters.session.as_mut() {
            session.reset();
        }
        if let Some(t) = self.terrain.region_tracker.as_mut() {
            t.reset();
        }
        if let Some(t) = self.world_map.region_tracker.as_mut() {
            t.reset();
        }
        self.battle.end = None;
        self.game_over = false;
        self.game_over_hold = false;
        self.clock.play_time_seconds = 0;
        self.cutscene.timeline = None;
        self.field_vm.helper_contexts.clear();
        self.cutscene.narration = None;
        self.cutscene.card = None;
        self.cutscene.text_balloon = None;
        // Camera-register zone ramps are scene content: retail's MAN loader
        // retire sweep (`FUN_8003AEB0` at `0x8003B414`) is keyed on the ramp
        // actor's own handler VA, and the zone-miss defaults are reinstalled
        // by `FUN_801DBE9C`. Both happen on scene entry.
        self.camera.register_ramps.clear();
        self.camera.registers = Default::default();
        // The three frame-delta timer templates are scene content too: the
        // MAN loader's retire sweep drops every pool actor, and a bar
        // envelope or a floor-rung bob left running across a scene change
        // would keep writing into the new scene's ladder.
        self.presentation.cinematic_bars = None;
        self.presentation.cinematic_bar = 0;
        self.field_vm.eased_moves.clear();
        self.terrain.floor_tier_bobs.clear();
        self.cutscene.prologue_naming_pending = false;
        self.cutscene.prologue_naming_armed = false;
        self.cutscene.entering_town01_opening = false;
        self.field_vm.pending_record_spawns.clear();
        self.cutscene.opening_chain_active = false;
        // Arm the arrival side of the scene-transition fade handshake
        // (`0x52F`/`0x530`/`0x531`, the shared `P1[0]` idiom): with `0x52F`
        // set, the destination entry script's arrival arm fires `4C 12 00 00
        // 00 00 00` (instant black) + `4C 12 80 80 80 44 00` (ramp to neutral
        // over 68 frames) - the fade-in from black the retail cold-boot
        // capture shows opening the prologue. In retail the flag is staged by
        // the departing side / New-Game boot before the field launches; the
        // engine's New Game is that boot.
        self.system_flag_set(0x52F);
        self.mode = SceneMode::Field;
    }

    /// Does the party hold the **Point Card** (item
    /// [`crate::shop::POINT_CARD_ITEM_ID`])?
    ///
    /// Retail asks the bag-slot scan `FUN_80042F4C(0xFE)` and tests the
    /// returned count's low halfword (`sll 16` / `beq`), so a slot present
    /// with count `0` reads as not held - which the `> 0` here matches. The
    /// gate guards both the accrual and the toast beat: the buy commit
    /// (`FUN_801DB7F4` case 3) returns straight to the buy list when it is
    /// closed.
    ///
    /// REF: FUN_80042F4C
    pub fn point_card_held(&self) -> bool {
        self.party
            .inventory
            .get(&crate::shop::POINT_CARD_ITEM_ID)
            .is_some_and(|c| *c > 0)
    }

    /// Credit one buy commit's Point Card accrual and return what was
    /// added, or `None` when the party does not hold the card.
    ///
    /// `(price / 20) * qty`, clamped into [`crate::shop::POINT_CARD_CAP`] -
    /// the arithmetic the Point Card's own on-disc description states as
    /// "5% of the price". Retail runs this **before** the gold debit; the
    /// engine's caller does the same.
    ///
    /// PORT: FUN_801DB7F4 (the case-2 accrual arm at
    /// `0x801dbac8..0x801dbb4c`: the `0xCCCCCCCD` reciprocal divide, the
    /// `mult` by the quantity, and the `0x0098967F` clamp)
    ///
    /// Wired: `crate::menu_runtime`'s `ShopConfirm` buy commit and the
    /// buy-recipient picker's two purchase arms both call it, and each then
    /// arms the window-31 toast when it returns `Some`.
    pub fn credit_point_card(&mut self, price: u16, qty: i32) -> Option<i32> {
        if !self.point_card_held() {
            return None;
        }
        let credit = crate::shop::point_card_credit(price, qty);
        self.minigames.point_card =
            crate::shop::apply_point_card(self.minigames.point_card, credit);
        Some(credit)
    }
}
