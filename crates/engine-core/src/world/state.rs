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
    /// Field VM execution context. Live in `SceneMode::Field` and
    /// `SceneMode::Cutscene` (cutscenes are field scenes that suppress
    /// player input via context flags).
    pub field_ctx: FieldCtx,
    /// Field VM bytecode buffer. Engines load this from a scene's PROT
    /// asset bundle when entering a field scene; `field_pc` indexes it.
    pub field_bytecode: Vec<u8>,
    /// Current field-VM PC. Updated by `tick()` based on the StepResult.
    pub field_pc: usize,
    /// Per-actor move-VM bytecode buffers. Indexed by actor slot. Empty
    /// vec means "no active move" - the move VM is not ticked for that
    /// actor. Set via [`World::set_move_bytecode`].
    pub move_bytecode: Vec<Vec<u16>>,
    /// MOVE buffer pool root, mirroring retail `_DAT_8007B888`. Populated
    /// per scene from the slot-1 `Asset(0x05) = Move` descriptor (the
    /// MDT-shaped offset-table blob parsed by [`legaia_mdt::MoveBuffer`]).
    /// Consumed by the [`vm::move_buffer::MoveBufferHost`] impl in
    /// `move_buffer_host.rs`. Empty when no scene MOVE table is wired
    /// (the cursor's resolver returns `None` and the per-actor state
    /// stays idle, matching retail when the table pointer is null).
    pub move_buffer_root: Vec<u8>,
    /// MOVE2 buffer pool root, mirroring retail `_DAT_8007B840`. Used
    /// when the per-actor `cursor_requested` is `>= 0x400`. Empty
    /// across most retail save states; only a small number of scenes
    /// install this. See `docs/formats/mdt.md`.
    pub move2_buffer_root: Vec<u8>,
    /// Alternate MOVE buffer pool root, mirroring retail `_DAT_8007B75C`.
    /// Selected by [`vm::move_buffer::STATUS_FLAG_ALT_POOL`] in the
    /// per-actor status flag word. Populated by the world-map / battle
    /// overlay paths.
    pub move_buffer_alt_root: Vec<u8>,
    /// Per-actor [`TickEvent`]s emitted by the last
    /// [`World::tick_actor_physics`] pass. Engines that want to react
    /// to audio cues, render submissions, or unlink requests drain
    /// this each frame; the move-buffer cursor kick is dispatched
    /// inline so callers do not need to inspect this list to keep
    /// move-VM playback running.
    pub last_tick_events: Vec<(u8, TickResult)>,
    /// Move-VM global predicate at `_DAT_801F22F4` (set by ext sub-op 0x08,
    /// cleared by 0x09; sub-ops 0x0A / 0x0B branch on it).
    pub move_predicate: u32,
    /// Move-VM global counter at `_DAT_801F22F6` (cleared by ext sub-op 0x0F,
    /// cycled mod 16 by sub-op 0x10).
    pub move_counter: u16,
    /// Move-VM 16-slot 8-byte-stride scratch table at `&DAT_801F3498`. Used
    /// by ext sub-ops 0x11 / 0x12 / 0x25 / 0x27 / 0x28 / 0x31 / 0x32 / 0x34
    /// / 0x35 to checkpoint world coords + tween state per actor / animation.
    pub move_slot_table: [[u8; 8]; 16],
    /// Move-VM axis offset at `_DAT_8007C348` - used by ext sub-ops 0x36 / 0x37
    /// for the `0x8E - axis` threshold predicate. Engines write per-scene.
    pub move_axis_threshold: i16,
    /// Move-VM scratchpad ramp ratio numerator at `_DAT_1F800393` - used by
    /// ext sub-op 0x23 (anim-bank lerp) as the numerator of a 12.0 fixed-point
    /// ratio against the operand-supplied denominator.
    pub move_ramp_ratio: u8,
    /// Fixed map origin pair at `(_DAT_80089118, _DAT_80089120)` - used by ext
    /// sub-op 0x24 (world position lerp toward fixed map origin).
    pub map_origin_xz: (i32, i32),
    /// Player actor slot - when `Some(slot)`, ext sub-ops 0x06 / 0x07 / 0x2A
    /// / 0x36 / 0x39 read `actors[slot].move_state.world_{x,y,z}` as the
    /// player position. `None` falls back to the origin (default impl).
    pub player_actor_slot: Option<u8>,
    /// Per-scene field collision / floor grid. Retail equivalent: the
    /// walkability map at `*(_DAT_1F8003EC) + 0x4000` that the locomotion
    /// collision check (`FUN_801cfe4c`) samples. One byte per 128-unit
    /// tile, `0x80`-byte rows, up to `0x80` rows (`0x4000` bytes). The
    /// **high nibble** holds 4 sub-cell wall bits (a `2x2` quadrant grid of
    /// `64x64` cells); the **low nibble** is a floor-elevation tier
    /// (unused by the wall check). Painted incrementally by the field-VM
    /// `0x4C` outer-nibble-7 op as the scene prescript runs; zeroed (all
    /// walkable) at field entry via [`World::reset_field_collision_grid`].
    /// Empty until the first field scene is entered.
    pub field_collision_grid: Vec<u8>,
    /// Per-scene `.MAP` region-table block (the file's `+0x10000..+0x12000`
    /// region - retail `*(_DAT_1F8003EC) + 0x10000`). Scanned per tile by
    /// the [`crate::field_regions`] ports to rebuild [`World::extra_flags`]
    /// (the `_DAT_8007B8F4` mirror) and the scratch attribute box. Empty for
    /// scenes without a field map.
    pub field_map_region_block: Vec<u8>,
    /// Per-scene MAN section-3 zone table (the camera-region records the
    /// boot walk installs at the control block `_DAT_801C6EA4 + 0x4`):
    /// a count byte + 18-byte records, queried per tile by
    /// [`crate::field_regions::zone_query`]. Empty for scenes whose MAN has
    /// no section 3.
    pub field_zone_table: Vec<u8>,
    /// The scratchpad region-attribute block (`0x1F800384..87` +
    /// `0x1F80037C`) latched by the per-tile refresh; read by the zone
    /// query's kind-0 arm.
    pub field_region_attributes: crate::field_regions::RegionAttributes,
    /// The 18-byte zone record the player currently stands in (the camera-
    /// region payload `FUN_801DBC20` consumes), refreshed on tile crossing.
    /// `None` when no zone record matches (retail loads the default camera
    /// parameter set).
    pub field_zone_record: Option<[u8; crate::field_regions::ZONE_RECORD_STRIDE]>,
    /// The 16-entry floor-height LUT the collision grid's low nibble indexes
    /// (retail `DAT_1F80035C`, filled from the MAN header by `FUN_8003AEB0` as
    /// 16 negated `s16` elevation tiers). Resolved per-scene into here from
    /// [`crate::scene::SceneAssets::field_floor_height_lut`]; consumed by
    /// [`World::sample_field_floor_height`] (the port of `FUN_80019278`). All
    /// zero until a field scene supplies it.
    pub field_floor_height_lut: [i16; 16],
    /// The `.MAP` **object-grid** cell words (`+0x8000`, one `u16` per tile,
    /// `0x80 x 0x80`). [`World::sample_field_floor_height`] tests each tile's
    /// [`crate::world::CELL_ELEVATION_OVERRIDE`] (`0x800`) bit to pick the
    /// floor model: bilinear corner-nibble surface, or the flat tile mean plus
    /// the tile's [`Self::field_elevation_overrides`] record (ramps / stairs).
    /// Empty until a field scene supplies it - then every tile reads as a
    /// plain bilinear tile, the pre-override behaviour.
    pub field_object_cells: Vec<u16>,
    /// The scene's kind-2 `.MAP` **elevation-override** records, primary
    /// (`+0x10000`) table followed by the fallback (`+0x12000`) one, so a
    /// linear first-match scan reproduces `FUN_801D5630`'s order. Consumed by
    /// [`World::sample_field_floor_height`] on
    /// [`crate::world::CELL_ELEVATION_OVERRIDE`] tiles.
    pub field_elevation_overrides: Vec<crate::world::ElevationOverride>,
    /// When set, field free-movement snaps the player's `world_y` to the
    /// per-scene terrain elevation each step via
    /// [`World::sample_field_floor_height`] (the port of `FUN_80019278`).
    /// Off by default so the flat-Y locomotion oracles keep their constant
    /// `world_y`; enable it for terrain-following play. Only the pad
    /// locomotion path consults it - world-map walk keeps its own height
    /// model - and it no-ops harmlessly (height `0`) until a scene supplies a
    /// floor LUT + collision grid.
    pub follow_terrain_height: bool,
    /// The player's field idle/walk clip pair (PROT 0874 §1 locomotion
    /// bundle). Installed per scene by the host
    /// ([`World::set_field_player_anim`]); the field tick advances it after
    /// the locomotion step and folds the output into the player actor's
    /// `pose_frame`, so hosts rebuild the posed mesh exactly like the battle
    /// animation path. `None` = static rest pose.
    pub field_player_anim: Option<crate::field_anim::FieldPlayerAnim>,
    /// When set, pad locomotion blocks a direction with retail's
    /// **three-probe leading-edge footprint** (`FIELD_WALL_PROBES`, the
    /// `DAT_801f2214` table `FUN_801cfe4c` walks) instead of a single
    /// candidate-centre test: the player rests ~47 units off a wall plane
    /// exactly like retail, instead of walking up to it. Off by default so
    /// the existing locomotion oracles (and BFS nav drivers, which keep the
    /// centre test regardless) are bit-identical; enable it for
    /// retail-faithful wall standoff. Validated against the wall-press
    /// captures by `engine-shell/tests/field_collision_discriminator.rs`.
    pub leading_edge_wall_probes: bool,
    /// When set, pad locomotion also blocks a direction when any of retail's
    /// **actor-collision probes** (`FIELD_ACTOR_PROBES`, the `DAT_801f21b4`
    /// sibling table) lands inside a field NPC's body box or a placed prop's
    /// static collision box ([`Self::field_actor_dir_blocked`]) - NPCs and
    /// props become solid, as in retail, where `FUN_801cfe4c`'s actor bits
    /// (`1`/`4`) gate a step exactly like the wall bit (`2`). Off by default
    /// for the same oracle-stability reason as
    /// [`Self::leading_edge_wall_probes`]. The locomotion-path touch
    /// dispatch (the `FUN_801d5b5c` auto event post for prop walk-touch) is
    /// modelled separately and independent of this flag -
    /// `Self::check_field_walk_touch`; the button-press interact dispatch
    /// is (`Self::tick_field_interaction_probe`).
    pub solid_field_npcs: bool,
    /// Camera azimuth (PSX 12-bit angle, `4096` = full turn) used to make
    /// d-pad locomotion camera-relative. Retail equivalent: the view
    /// direction `func_0x800467e8` remaps the held pad against. `0` maps
    /// "screen up" to world `+Z` (the default follow camera looking down
    /// `+Z`). Engines that orbit the camera write the current azimuth here
    /// each frame; the locomotion remap quantises it to the nearest 90°.
    pub field_camera_azimuth: u16,
    /// Party-member actor slots - `party_actor_slots[i] = Some(actor_slot)`
    /// resolves move-VM ext sub-op 0x3B (`ext_party_member_lookup`) to the
    /// world-coords of the actor at that slot. Default empty (the lookup
    /// returns `None`, which forces sub-op 0x3B's "skip" path).
    pub party_actor_slots: Vec<Option<u8>>,
    /// Last fade colour requested by move-VM ext sub-op 0x3C - engines
    /// drain this each frame to drive the screen fade. `None` when no
    /// fade is pending.
    pub pending_fade: Option<FadeRequest>,
    /// Move-VM `_DAT_8007B9D8` - globally-shared 32-bit slot written by ext
    /// sub-op 0x2F. Engines read this on whatever frame-tick they want.
    pub move_dat_8007b9d8: i32,
    /// Move-VM 16-slot scratchpad ramp targets at `_DAT_1F80035C` - used by
    /// ext sub-op 0x29 (per-frame ramp / immediate write). Stored as i16
    /// pairs (target, current); engines apply per-frame interpolation.
    pub scratchpad_targets: [i16; 16],
    /// Shared system flag bank at `_DAT_80085758` - bitfield read / written
    /// by:
    /// - field VM high-byte default routes 0x5x / 0x6x / 0x7x
    ///   (`system_flag_set` / `system_flag_clear` / `system_flag_test`)
    /// - move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D
    ///   (`ext_query_flag_bank` / `ext_set_flag_bank` / `ext_clear_flag_bank`)
    ///
    /// Lazily grown on write - the field VM's opcode-encoded idx ranges over
    /// `0..=0x87FF`, so a fixed 256-bit array is too small.
    pub system_flags: Vec<u8>,
    /// Field-VM `extra_flags` register read by op 0x42 mode 0 - the
    /// `_DAT_8007B8F4` **region-type mask**: bit `n` set when the player's
    /// tile sits inside a type-`n` region of the scene `.MAP` region table.
    /// Rebuilt per tile crossing by [`World::refresh_field_regions`] (the
    /// `FUN_800180EC` / `FUN_801DBA20` ports in [`crate::field_regions`])
    /// when the per-scene tables are installed; otherwise host-owned
    /// scene-local state.
    pub extra_flags: u32,
    /// Field-VM `screen_mode` register read by op 0x42 mode 1 - packed mode
    /// bits (bits 4 / 5 / 6 / 7 individually testable; bits 12..15 indexed
    /// against `screen_mode_table`).
    pub screen_mode: u32,
    /// Field-VM scratchpad flag word (`_DAT_1F800394` in retail). Set
    /// by op `0x2E` GFLAG_SET; cleared by op `0x2F` GFLAG_CLR; tested
    /// by op `0x30` GFLAG_TST.
    ///
    /// Independent of [`Self::story_flag_bits`]: retail seeds this from
    /// the game-mode descriptor table on mode init (low 16 bits of
    /// `mode_table[mode_idx].param`) and the SC save/load bulk copy
    /// from RAM `0x80084340` never reaches scratchpad, so the bitmap
    /// and this word are not mirror copies of each other.
    pub story_flags: u32,
    /// Full 512-byte story-flag bitmap mirroring retail RAM
    /// `0x80085600..0x80085800` (SC block offset `0x14C0`). This is the
    /// narrative-progress bitmap the SC block persists, separate from
    /// the per-mode scratchpad word [`Self::story_flags`].
    ///
    /// Empty (`vec![]`) when the engine hasn't been booted from a retail
    /// SC block; populated via [`Self::load_full`] when a retail-shaped
    /// [`legaia_save::SaveFile`] is restored.
    pub story_flag_bits: Vec<u8>,

    /// PRNG state consumed by every VM that calls `host.rng()`. Default uses
    /// a deterministic LCG so tests are reproducible.
    pub rng_state: u32,

    /// Sin LUT used by move-VM op `0x03`. Engines populate from extracted
    /// asset data; default is empty (returns zero).
    pub sin_lut: Vec<i16>,
    /// Cos LUT - same shape as `sin_lut`.
    pub cos_lut: Vec<i16>,

    /// Battle-action helper tables. Engines populate per scene.
    pub spell_costs: std::collections::HashMap<u8, u8>,
    pub capture_spells: std::collections::HashSet<u8>,
    pub character_ability_bits: [u32; 8],
    pub range_table: std::collections::HashMap<(u8, u8), u16>,
    /// Per-slot weapon attack used by [`art_strike::apply_art_strike`] to
    /// compute Tactical-Art damage. Engines populate from the active
    /// character record's weapon power. Default zero - un-populated slots
    /// produce floor-clamped damage (`= 1`).
    pub battle_attack: [u16; 8],
    /// Per-slot magic attack scalar used by [`spells::cast_spell`] for the
    /// caster's `mag` column when resolving a player-driven battle Magic cast.
    /// Engines populate from the active character record; default zero (which
    /// floors damage spells at 1).
    pub battle_magic: [u16; 8],
    /// Per-slot defense facing the strike. The retail engine selects UDF
    /// or LDF based on the strike's `power_target`; this single field is a
    /// minimum-viable substitute that engines wishing to model both can
    /// override via [`World::set_battle_defense_for_target`].
    pub battle_defense: [u16; 8],
    /// Optional UDF / LDF defense override per slot. When set, the
    /// art-strike applier uses the matching half for the strike's target
    /// class instead of [`Self::battle_defense`]. Engines that don't
    /// distinguish UDF / LDF can leave this `None`.
    pub battle_defense_split: [Option<(u16, u16)>; 8],
    /// Per-slot SPD (turn-order initiative seed, retail actor `+0x164`).
    /// Party slots are seeded from each character record's live SPD in
    /// [`World::load_party`]; monster slots from [`crate::monster_catalog::MonsterDef::speed`]
    /// at battle setup. When **every** living actor has SPD `0` (the
    /// disc-free / synthetic case) the battle stays on the round-robin
    /// turn-order fallback (`World::next_living_combatant`); when any actor
    /// carries real SPD the next-actor selector switches to the SPD-seeded
    /// initiative scheme (`World::next_combatant_by_initiative`, the port of
    /// `recompute_battle_order` / `FUN_801daba4`).
    ///
    /// REF: FUN_801DABA4
    pub battle_speed: [u16; 8],

    /// Per-slot accuracy stat (retail actor `+0x168`, the AGL-derived
    /// hit/dodge seed). Used as the **attacker's** term in the selector-9
    /// accuracy roll ([`legaia_engine_vm::battle_formulas::accuracy_roll`]).
    /// Party slots are seeded from each character's resolved `acc` in
    /// [`World::seed_party_battle_stats`]; monster slots from
    /// [`crate::monster_catalog::MonsterDef::accuracy`] at battle setup. When
    /// the attacker's accuracy is `0` (the disc-free / synthetic case) the
    /// strike auto-hits and consumes no RNG, so battles that don't seed these
    /// stats keep their always-land behaviour and bit-identical RNG streams.
    pub battle_accuracy: [u16; 8],
    /// Per-slot evasion stat - the **defender's** term in the selector-9
    /// accuracy roll. Same field as [`Self::battle_accuracy`] in retail
    /// (`+0x168` serves both rolls); kept separate here so equipment that
    /// modifies only accuracy or only evasion is representable. Seeded
    /// alongside accuracy.
    pub battle_evasion: [u16; 8],

    /// Number of physical swings the active monster attacker lands on its
    /// current turn - the enemy multi-action budget. Computed at monster-turn
    /// arm ([`World::arm_monster_strike_budget`]) from the monster's AGL gauge
    /// ([`crate::monster_catalog::MonsterDef::agl`]) + its swing costs
    /// (`action_costs`) via the port of `FUN_801E9FD4`'s budget loop
    /// ([`legaia_engine_vm::battle_action::enemy_action_budget`]), then consumed
    /// by [`World::apply_basic_attack`]. Always `1` for a party attacker (its
    /// multi-hit is the AP/arts system) and for a monster with no AGL / swing
    /// data (the disc-free / synthetic catalog), so unbudgeted battles stay
    /// bit-identical. Defaults to `1`.
    ///
    /// REF: FUN_801E9FD4
    pub monster_strike_budget: u8,

    /// "Previous action cleared" gate - toggled by the engine when an
    /// animation transition completes.
    pub prev_action_cleared: bool,
    /// "Sound bank ready" gate.
    pub sound_bank_ready: bool,

    /// Number of party slots (default 3).
    pub party_count: u8,

    /// Present-party composition: `active_party[i]` = the **roster slot**
    /// (index into [`Self::roster`]) occupying battle ordinal `i`. The
    /// engine mirror of retail's present-party list at `0x8007BD10`
    /// (1-based char ids there; 0-based roster slots here). Battle actor
    /// slot `i`, HUD row `i`, and the runtime VRAM texture band `i`
    /// (`relocate_tsb_cba` row `481 + i`) all key on the ORDINAL; the
    /// character content (player battle file `863 + roster_slot`,
    /// equipment, spell list, XP recipient) keys on the roster slot -
    /// the live-verified retail banding rule (band = ordinal, file =
    /// 862 + char_id). Empty = identity mapping (slot `i` = roster `i`,
    /// the Vahn/Noa/Gala default). Resolve through
    /// [`Self::party_roster_slot`]; install via [`Self::set_active_party`].
    pub active_party: Vec<u8>,

    /// Last-issued battle-end cause (for inspection / engine side-effects).
    pub battle_end: Option<BattleEndCause>,

    /// Active full-screen fade, staged by the battle SM's escape teardown
    /// (retail state `0x66` spawns the `DAT_801C9070` black→white ramp via
    /// the fade-primitive spawner `FUN_80024E80`). Stepped once per
    /// [`World::tick`]; dropped when the ramp completes. Hosts draw an
    /// overlay from [`crate::fade::FadeState::rgb`] while this is `Some`.
    pub screen_fade: Option<crate::fade::FadeState>,

    /// Active field-VM colour-fade overlay (op `0x34` sub-0, the effect-global
    /// colour + intensity setup `FUN_801E1FB0`). The opening prologue's white
    /// flash (`34 05 FF FF FF 00 00`) sets this; hosts draw a full-screen wash
    /// of [`crate::fade::ColorFade::rgb`] at [`crate::fade::ColorFade::coverage`]
    /// while it is `Some`. Stepped once per [`World::tick`]; dropped when the
    /// ramp completes. Distinct from [`Self::screen_fade`] (the battle escape
    /// RGB ramp) - this is the field/cutscene fade path.
    pub color_fade: Option<crate::fade::ColorFade>,

    /// Persistent per-character roster - populated by [`World::load_party`]
    /// and written back by [`World::save_party`]. Each record is the
    /// 0x414-byte struct documented in `docs/subsystems/battle.md`. The
    /// in-battle `BattleActor` slots mirror HP / MP from this; everything
    /// else (spells, equipment, ability bits) flows through this canonical
    /// store.
    pub roster: legaia_save::Party,

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

    /// Pending FMV trigger (field-VM op `0x4C 0xE2`). When `Some(fmv_id)`,
    /// the field VM has signalled that the next-game-mode global should
    /// transition to game mode 26 (StrInit) with the given index. Engines
    /// drain this after [`World::tick`] to actually open the corresponding
    /// `MV*.STR` (use [`crate::cutscene::fmv_index_to_str_filename`] for
    /// the retail mapping). `None` between triggers.
    pub pending_fmv_trigger: Option<i16>,

    /// Pending scripted-encounter install (field-VM bare arm-encounter op
    /// `0x37`/`0x41`). When that op runs and [`Self::scripted_encounter_armed`]
    /// is set, the host records the bounded record window overlaying the opcode
    /// here; the field-step driver drains it after the VM borrow ends and feeds
    /// it to [`Self::install_scripted_encounter`]. `None` between installs.
    ///
    /// Retail writes the install opcode pointer into `actor[+0x94]`
    /// (`0x801DEEDC`) and the 5-state `FUN_801DA51C` SM reads it as a formation
    /// record once it reaches the encounter-confirm state. There is no
    /// dedicated encounter opcode - the consuming entity SM is the
    /// discriminator. `scripted_encounter_armed` is the engine-side stand-in
    /// for "the active entity is an encounter carrier" until the per-scene
    /// carrier identity / SM-confirm trigger is pinned from disc bytecode.
    pub pending_scripted_encounter: Option<Vec<u8>>,

    /// When `true`, the field VM's bare arm-encounter op (`0x37`/`0x41`) is
    /// treated as a scripted-encounter install: the record window overlaying
    /// the opcode is parsed as an [`crate::encounter_record::EncounterRecord`]
    /// and installed via [`Self::install_scripted_encounter`], which then
    /// disarms (fire-once). Default `false` so generic script yields are never
    /// mistaken for encounter arms. See [`Self::arm_scripted_encounter`].
    pub scripted_encounter_armed: bool,

    /// One-shot override: a scripted/forced formation has been installed
    /// ([`Self::install_man_formation`] / [`Self::install_encounter_from_record`])
    /// and the next [`Self::on_field_step`] must fire it regardless of any
    /// per-region random rate. Retail copies the carrier's `entity[+0x94]`
    /// formation into the battle cell independent of the random-roll path
    /// (`FUN_801D9E1C`), so a 0%-random scene (e.g. town01's Rim Elm tutorial)
    /// still starts the scripted fight. Cleared when the step consumes it.
    pub scripted_formation_pending: bool,

    /// The FMV currently playing in [`SceneMode::Cutscene`]. Set when the
    /// world consumes a [`Self::pending_fmv_trigger`] at the top of a
    /// [`World::tick`] and flips into the cutscene mode (mirroring retail's
    /// next-game-mode dispatch to game mode 26 one frame after the field-VM
    /// op writes the global). While `Some`, the field VM is suspended (the
    /// STR overlay owns the frame in retail); the host plays the resolved
    /// `MV*.STR` and calls [`World::finish_cutscene`] when playback ends.
    /// `None` outside an STR-FMV cutscene.
    pub active_fmv: Option<i16>,

    /// Scene mode to restore when the active STR-FMV cutscene finishes
    /// (set on entry, consumed by [`World::finish_cutscene`]). Retail
    /// returns to the field after the cutscene overlay unloads; `None`
    /// outside a cutscene.
    pub cutscene_return_mode: Option<SceneMode>,

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
    /// `actor[+0x90]`; the clean-room port leaves that policy to the
    /// engine that consumes the request.
    pub pending_actor_spawns: Vec<Vec<u8>>,

    /// Battle action state machine side-effects emitted this frame.
    /// Engines drain after [`World::tick`] to dispatch poses, UI elements,
    /// damage, screen-shake, etc. See [`BattleEvent`] for the per-variant
    /// citation.
    ///
    /// [`BattleEvent`]: crate::battle_events::BattleEvent
    pub pending_battle_events: Vec<BattleEvent>,

    /// Presentation-only per-strike HP deltas surfaced for HUD damage
    /// popups. The gameplay-state HP mutation has *already* happened by
    /// the time an entry lands here (the live battle loop folds art-strike
    /// damage and applies the generic physical strike before queuing the
    /// matching FX), so engines must NOT re-apply these to HP - they only
    /// drive the floating-number / status overlay. Drained by the host via
    /// [`World::drain_battle_hit_fx`]; cleared on battle exit.
    pub battle_hit_fx: Vec<BattleHitFx>,

    /// Per-strike battle sound cues surfaced this frame for the host to play
    /// through its SFX bank (the art-record `HitCue` sound cues that
    /// [`World::fold_battle_event`] resolves from an `ApplyArtStrike` outcome -
    /// previously dropped). Cosmetic, like [`Self::battle_hit_fx`]: no gameplay
    /// state depends on them. Drained via [`World::drain_battle_sfx_cues`];
    /// cleared on battle exit.
    pub battle_sfx_cues: Vec<BattleSfxCue>,

    /// Last BGM the field VM started (op 0x35 sub-1 / sub-9). `None` until
    /// a scene starts one. Updated synchronously when the VM emits the
    /// corresponding `Bgm` event.
    pub current_bgm: Option<u16>,

    /// BGM id to swap to when a live-loop encounter begins, restored to the
    /// field track when the battle ends. `None` (the default) leaves music
    /// untouched across the Battle transition - set it via
    /// [`World::set_battle_bgm`] to enable the swap. The swap is routed as an
    /// ordinary `FieldEvent::Bgm` start (sub-op 1), so the host's existing
    /// BGM director resolves the SEQ and cross-fades exactly like a field
    /// op-`0x35` start.
    pub battle_bgm: Option<u16>,

    /// Field track stashed at battle entry so `World::restore_field_bgm`
    /// can resume it after the encounter. Managed by the swap helpers; not
    /// meant to be set directly.
    pub field_bgm_resume: Option<u16>,

    /// `true` while the battle track is playing (set by
    /// `World::swap_to_battle_bgm`, cleared by
    /// `World::restore_field_bgm`). Guards against double-swap / spurious
    /// restore.
    pub battle_bgm_active: bool,

    /// Active dialog request - populated by the field-VM op 0x3F handler,
    /// cleared by the engine after the user dismisses the box. The MES
    /// renderer reads `text_id` + `inline`; the world-coords + depth feed
    /// the box placement.
    pub current_dialog: Option<DialogRequest>,

    /// Active 3-actor talk session (field-VM op `0x43` sub-2; retail talk
    /// controller from `FUN_801D2D38`). Refreshed on every sub-2
    /// instruction; the paired system flag `0xD` is the retail talk-active
    /// lock. See [`ThreeActorTalk`].
    pub three_actor_talk: Option<ThreeActorTalk>,

    /// Last `field_interact` request. Cleared by the engine when handled
    /// (set to `None`).
    pub last_field_interact: Option<(u8, u8)>,

    /// Per-actor inline interaction-script dialogue, keyed by the actor's
    /// MAN partition-1 record index - the `slot` a field-interact op
    /// (`0x3E` with `op0 < 100`) carries. Populated at field-scene entry from
    /// the scene's actor placements. This is the **real** field NPC dialogue
    /// source (the actor's inline MES text at retail `actor[+0x90]`), so
    /// `crate::world::vm_hosts`'s `field_interact` opens the interacted
    /// actor's dialogue from here - not from a `0x3F` op (which is the named
    /// scene-change, not dialogue). Empty between field scenes.
    pub field_npc_dialog: std::collections::HashMap<u8, Vec<u8>>,

    /// Prologue-aware companion to [`Self::field_npc_dialog`], keyed by the same
    /// `slot`. Carries each talk NPC's **untruncated** interaction record (full
    /// body + entry PC + first-segment offset) so the opt-in field-VM dialogue
    /// runner ([`Self::use_vm_dialogue`]) can execute the interaction prologue -
    /// the story-flag `SysFlag.Test`/`JmpRel` segment-selection bytecode before
    /// the first `0x1F` - instead of starting at the first segment. The default
    /// simplified path ignores this and uses `field_npc_dialog` unchanged.
    pub field_npc_dialog_prologue:
        std::collections::HashMap<u8, crate::man_field_scripts::InlineDialogPrologue>,

    /// The interaction-prologue record for the dialogue [`Self::trigger_field_interact`]
    /// most recently opened (taken by [`Self::drive_inline_dialogue`] when it
    /// starts the runner). `None` when the opened NPC has no prologue record.
    pub active_inline_prologue: Option<crate::man_field_scripts::InlineDialogPrologue>,

    /// Per-talkable-NPC spawn world position `(world_x, world_z)`, keyed by the
    /// same `slot` as [`Self::field_npc_dialog`]. Populated at field-scene entry
    /// from the MAN actor placements. The interaction probe
    /// (`Self::tick_field_interaction_probe`) box-tests the player's position
    /// against these to fire a `field_interact` on the action button - the
    /// clean-room analogue of retail's `FUN_801cf9f4` adjacency test.
    ///
    /// The runtime actor frame **is** the MAN placement frame: `FUN_8003A1E4`
    /// spawns each actor at `world = tile*128 + 0x40` (the placement's
    /// [`world_x`](legaia_asset::man_section::ActorPlacement::world_x)), stored
    /// straight into `actor[+0x14/+0x18]` by `FUN_80024C88` with no anchor, and
    /// the player cold-spawn `0xA40` is `tile 20*128 + 0x40` in that same frame.
    /// So these placement positions compare directly against the player's
    /// [`crate::vm::ActorMoveState::world_x`]. Positions are LIVE, not just
    /// the spawn tile: `Self::tick_field_npc_motions` writes walking NPCs'
    /// per-frame positions back here, so collision and interact probes follow
    /// a moving NPC.
    pub field_npc_positions: std::collections::HashMap<u8, (i16, i16)>,

    /// Live per-NPC heading (PSX 12-bit angle, same `render_26` convention as
    /// the player: `0` = travel Z+), keyed by placement slot. Written by
    /// `Self::tick_field_npc_motions` from each walk step's direction, and
    /// retained when the walker stops (an NPC keeps facing the way it last
    /// moved). Absent for NPCs that have never walked - hosts render those
    /// unrotated (the placement record carries no facing byte; scripted
    /// initial facings are the per-actor field-VM channels, not yet
    /// executed).
    pub field_npc_headings: std::collections::HashMap<u8, i16>,

    /// Static prop colliders, one per placed object of the scene's field
    /// `.MAP` object grid - the engine's source for the **actor-collision
    /// arms** of the movement probe (retail `FUN_801CFC40`). Installed at
    /// field-scene entry from
    /// [`crate::scene::Scene::field_object_placements`] (each placement's
    /// [`collider_x`](legaia_asset::field_objects::Placement::collider_x) /
    /// `collider_z` = spawn position + the record's collision-footprint
    /// offset, live-verified against the spawned static actors of catalogued
    /// captures), with each bound placement's class bits decoded from its
    /// bind record's spawn prologue. **Solid by default** - retail's placed
    /// props always enter the collision candidate list (`FUN_801CF754`)
    /// unless their script sets `+0x10 & 3`; a closed door blocks the player
    /// until its touch pass runs `31 00`.
    pub field_prop_colliders: Vec<FieldPropCollider>,
    /// The cold field-entry spawn `(x, z)` the scene host resolved at entry
    /// ([`Self::resolve_cold_field_spawn`]) - a standable, reachable spot in
    /// the scene's largest walkable component. Kept so the helper-context
    /// teardown can re-seat the player here if a partially-executed spawned
    /// record left them inside a wall (see [`Self::step_helper_contexts`]).
    /// `None` outside field scenes.
    pub resolved_cold_spawn: Option<(i16, i16)>,

    /// Per-scene bank of placed-prop animation + interaction runtimes (the
    /// door swings, the searchable cupboards), keyed by the placement's
    /// footprint-anchor tile. Built at field-scene entry
    /// ([`crate::field_env::PropAnimBank::build`]); clips advance every field
    /// tick, and a touched / interacted prop's bind record runs through the
    /// field VM ([`Self::start_prop_interaction`]).
    pub field_prop_bank: crate::field_env::PropAnimBank,

    /// A prop the movement probe touched this tick (the `FUN_801CFC40`
    /// static-arm hit whose result bit `4` the locomotion auto-posts through
    /// `FUN_801D5B5C`): the anchor key of the touched [`Self::field_prop_bank`]
    /// entry. Drained by [`Self::tick_prop_interactions`], which starts the
    /// record's field-VM run.
    pub pending_prop_touch: Option<(u8, u8)>,

    /// Per-NPC autonomous walk routes, keyed by the same placement `slot` as
    /// [`Self::field_npc_dialog`]: the ordered local waypoints the placement's
    /// own pre-text script walks the actor through (its `0x4C 0x51` NPC
    /// move-to-tile ops - [`crate::man_field_scripts::placement_motion_route`]).
    /// Driven through the motion VM by `Self::tick_field_npc_motions` when
    /// [`Self::animate_field_npcs`] is set. `BTreeMap` so the per-tick walk
    /// order is deterministic (the replay oracle requires bit-stable traces).
    pub field_npc_routes: std::collections::BTreeMap<u8, Vec<(i16, i16)>>,

    /// Per-NPC glide speed, keyed by the same placement `slot` as
    /// [`Self::field_npc_routes`]: the per-frame world-unit step
    /// `Self::start_field_npc_motion` writes into a leg's motion-VM
    /// [`legaia_engine_vm::motion_vm::MotionState::speed`], decoded from the
    /// placement's real walk-kernel operands
    /// ([`crate::man_field_scripts::placement_glide_speed`]: the bound MAN
    /// tail-section-1 wander/step ops first, then the record's own field-VM
    /// `0x37`/`0x41`/`0x47` yield ops, then the facing-nibble heuristic as a
    /// last resort). A slot with no decodable motion leg is absent and the
    /// leg falls back to the stand-in
    /// [`crate::world::FIELD_NPC_MOTION_SPEED`]. See
    /// `docs/subsystems/field-locomotion.md`.
    pub field_npc_glide_speeds: std::collections::BTreeMap<u8, u16>,

    /// Per-NPC default-move pair `[move_id, anim_id]`, keyed by placement
    /// `slot`: the motion op-`0x17` writes into the retail per-actor table at
    /// `0x801C6470`, statically harvested from the scene MAN's tail-section-1
    /// streams ([`crate::man_field_scripts::motion_default_move_writes`]).
    /// The table the interaction motion-pause kick (`FUN_8003C9AC`, ported at
    /// [`legaia_engine_vm::motion_pause`]) reloads a moving-class actor's
    /// requested-move pair from.
    pub field_npc_default_moves: std::collections::BTreeMap<u8, [u8; 2]>,

    /// In-flight field-NPC walk legs, keyed by placement `slot`. Stepped once
    /// per field tick through the ported motion VM; each step writes the new
    /// position back into [`Self::field_npc_positions`], so the moving NPC
    /// keeps its ±40-unit collision box and its interact box at the live
    /// position. Script-started legs (interaction-prologue `0x4C 0x51`, actor
    /// VM `start_motion`) run regardless of [`Self::animate_field_npcs`].
    pub field_npc_motions: std::collections::BTreeMap<u8, FieldNpcMotion>,

    /// Drive autonomous NPC patrol routes ([`Self::field_npc_routes`]) through
    /// the motion VM. Off by default (NPCs rest at their placement anchors,
    /// like the locomotion oracles expect); `play-window --live-npcs` enables
    /// it. Script-started motion is NOT gated by this flag.
    pub animate_field_npcs: bool,

    /// Per-placement walk-touch events, keyed by placement `slot`: the
    /// placements whose script fires on body contact (door warps, player
    /// throw-back teleports - [`crate::man_field_scripts::placement_walk_touch_event`]),
    /// with the placement's spawn position as the contact-box centre. The
    /// locomotion's per-step touch dispatch (`Self::check_field_walk_touch`)
    /// posts these without a button press - retail's `FUN_801d5b5c` auto
    /// event post on the static-entity collision arm.
    pub field_walk_touch: std::collections::BTreeMap<u8, ((i16, i16), WalkTouchEvent)>,

    /// For each `.MAP`-object door bind ([`Self::install_trigger_walk_touch`]),
    /// the **flat** MAN record index the object's script is. A door record is a
    /// field-VM script whose opening `SysFlag.Test` chain selects the arm that
    /// runs, so the effect is re-resolved against the live story flags at
    /// contact time ([`crate::man_field_scripts::resolve_walk_touch_event`])
    /// rather than frozen at scene load; the `field_walk_touch` entry keeps the
    /// structural decode as the fallback.
    pub field_walk_touch_records: std::collections::BTreeMap<u8, usize>,

    /// Walk-touch edge latch: the slot whose contact box the player currently
    /// stands in, so a sustained press posts its event once (retail gates the
    /// per-step post on the player's `+0x10 & 0x80000` engaged flag, cleared
    /// by the dialog SM teardown - the engine latches per contact instead).
    pub active_walk_touch: Option<u8>,

    /// The post-remap direction bits of this tick's movement attempt
    /// (`0x1000`/`0x4000`/`0x2000`/`0x8000`; `0` when no direction is held).
    /// The walk-touch dispatch derives its leading probe points from it -
    /// retail's touch fires from the same forward probes that block the
    /// step, so contact must be tested ahead of the player, not at the
    /// player's feet.
    pub last_move_dir_bits: u16,

    /// While [`Self::step_inline_dialogue`] is stepping the field VM over an
    /// NPC's interaction record, this carries that NPC's placement slot so the
    /// `0x4C 0x51` NPC-run host hook can route the walk to the right actor
    /// (the engine's stand-in for retail's per-actor script context pointer).
    pub stepping_inline_npc: Option<u8>,

    /// The placement slot [`Self::trigger_field_interact`] most recently
    /// opened a dialogue for; consumed by [`Self::drive_inline_dialogue`] so
    /// the inline runner knows which NPC its record belongs to.
    pub active_inline_slot: Option<u8>,

    /// Actor-VM glide targets (op `0x09` `MotionAt` → `start_motion`,
    /// retail `FUN_800358c0`), keyed by actor slot: each entry glides the
    /// actor's `move_state` `(world_x, world_y)` toward the target through
    /// the motion VM, one step per tick (`Self::tick_actor_motions`).
    pub actor_motions: std::collections::BTreeMap<u8, FieldNpcMotion>,

    /// Per-tick guard: set when a Cross/Circle press is consumed by a field
    /// dialogue open or dismiss this tick, so the script's `0x4C` dialog poll
    /// and the interaction probe can't both act on the same edge (double
    /// open/dismiss). Reset at the top of each [`SceneMode::Field`] tick.
    pub dialog_input_consumed: bool,

    /// Active party slot for the leader (op 0x4C sub-0 writes here, plus
    /// `party_add` populates it on the first member).
    pub party_leader_slot: Option<u8>,

    /// Running money total (gold). Modified by op 0x3A `add_money`,
    /// clamped to `[0, 9_999_999]` per the original retail formula.
    pub money: i32,

    /// Per-slot inventory counts. Indexed by raw `slot_byte` operand of
    /// op 0x3B (`(slot >> 4) * 0x414 + (slot & 0xF)` in retail). Engines
    /// can re-key this to their own inventory model.
    pub inventory: std::collections::HashMap<u8, u8>,

    /// Last camera state snapshot - filled by `camera_save`, applied by
    /// `camera_apply` / `camera_load`. Engines that draw a camera read
    /// this between frames.
    pub camera_state: CameraState,

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

    /// Per-actor move-VM outcomes from the most recent [`World::tick_move_vms`]
    /// call. Pairs of `(actor_slot, outcome)`. Engines drain or inspect this
    /// after `World::tick` to react to halts / pending opcodes.
    pub move_outcomes: Vec<(u8, vm::move_vm::ActorTickOutcome)>,

    /// Per-character Tactical Arts use-counter tracker. Engines call
    /// [`World::notify_art_used`] from the battle side-effects handler when
    /// a Tactical Arts strike lands; the tracker emits
    /// [`BattleEvent::TacticalArtLearned`] and sets
    /// [`World::current_art_banner`] on first learn.
    pub tactical_arts: TacticalArtsTracker,

    /// Active "art learned" HUD banner. Set by [`World::notify_art_used`]
    /// when a new art crosses the learn threshold; its `frames_remaining`
    /// counter is decremented by [`World::tick`] until it reaches zero.
    /// `None` when no banner is active. Engines render this as a dialog-
    /// font overlay above the battle HUD.
    pub current_art_banner: Option<ArtLearnedBanner>,

    /// Per-party XP accumulator and level state. Engines call
    /// [`World::apply_battle_xp`] after a `BattleEndCause::MonsterWipe` to
    /// distribute XP and check for level-ups.
    pub level_up_tracker: LevelUpTracker,

    /// Active level-up HUD banner. Set by [`World::apply_battle_xp`] for the
    /// last character that leveled up; `frames_remaining` is decremented by
    /// [`World::tick`] until it reaches zero. `None` when no banner is active.
    /// Engines render this as a dialog-font overlay after battle.
    pub current_level_up_banner: Option<LevelUpBanner>,

    /// Active post-battle Seru-capture banner. Set by `World::resolve_captures`
    /// when a capture is accepted; advanced one frame per [`World::tick`] and
    /// cleared when its [`crate::seru_learning::SeruCaptureSession`] reaches
    /// `Done`. Engines render [`crate::seru_learning::SeruCaptureSession::current_banner`]
    /// as a dialog-font overlay after battle, the sibling of
    /// [`Self::current_level_up_banner`].
    pub current_capture_banner: Option<crate::seru_learning::SeruCaptureSession>,

    /// World-map camera and entity state. `Some` when `mode == SceneMode::WorldMap`,
    /// `None` otherwise.
    pub world_map_ctrl: Option<WorldMapController>,

    /// Tile-board grid (puzzle / board minigame mode; movement +
    /// collision). `Some` when a field scene has installed a tile board
    /// (retail field-VM op `0x49`). Drives discrete cell-to-cell player
    /// movement in the `SceneMode::Field` tick. This is *not* general
    /// town locomotion (Legaia towns use free movement). See
    /// [`crate::tile_board`].
    pub tile_board: Option<crate::tile_board::TileBoard>,

    /// While stepping on a tile board, the world `(x, z)` the player
    /// actor is interpolating toward (the destination tile centre).
    /// `None` when the player is idle and ready to accept a new
    /// direction. Mirrors the walk SM's "interpolate to target" state
    /// (`overlay_0897_801ef2b0` case 2).
    pub tile_board_target: Option<(i32, i32)>,

    /// `true` while a field-VM op-0x49 sub-5 board install holds the
    /// script suspended (the engine face of retail's `_DAT_8007b450`
    /// arm for the board consumer). The op reads `Armed` while
    /// [`Self::tile_board`] is installed and `Done` once the board
    /// exits (an event cell landing), then clears this on resume.
    pub tile_board_armed: bool,

    /// The parsed op-49 board header (radius / mode flag / actor
    /// template ids) kept for the render + event consumers while the
    /// board is installed.
    pub tile_board_header: Option<crate::tile_board::TileBoardHeader>,

    /// Per-cell-value tile-actor table (retail `DAT_801f35bc`, 15 entries).
    /// Index = cell value: slot `0` = the player actor, `2..=14` = the
    /// per-value tile actors spawned at board install from
    /// `tile_template_base + (value - 2)`. Each entry is the actor-pool
    /// slot holding that value's instance, or `None` when the value is not
    /// present on the board / the pool was exhausted. Cleared on teardown.
    pub tile_actor_slots: [Option<u8>; crate::tile_board::TILE_ACTOR_TABLE_LEN],

    /// Per-frame tile-board draw list: one entry per drawn cell, naming the
    /// tile actor to draw and the world-centre position to draw it at
    /// (`overlay_0897_801e0f3c`). Refreshed every field tick while a board
    /// is installed (honouring the header `+6` full-vs-windowed mode and
    /// `+5` radius); empty otherwise. The deferred renderer consumes this
    /// to draw each tile actor's mesh at every listed cell.
    pub tile_board_draw_list: Vec<crate::tile_board::TileDraw>,

    /// Screen-effect widget host (the PROT-0900 mask / sprite / panel /
    /// letterbox family), driven by the field-VM op `0x43` sub-ops
    /// `0x10`/`0x11`/`0x13`/`0x14`/`0x15` - the ending-scene widget
    /// path. See [`crate::screen_fx`].
    pub screen_fx: crate::screen_fx::ScreenFxHost,

    /// The current frame's widget draw list, refreshed by the Field /
    /// Cutscene tick while any widget is live ([`Self::tick_screen_fx`]).
    /// Renderers composite these 2D overlays above the scene.
    pub screen_fx_frame: crate::screen_fx::ScreenFxFrame,

    /// Live 4-byte register-ramp records spawned by the field-VM op `0x43`
    /// sub-3..6 (retail `FUN_8003C6A4` actors on the effect list). The
    /// engine holds the parameterization; the per-frame interpolator handler
    /// is untraced, so nothing ticks these yet. See [`crate::register_ramp`].
    pub register_ramps: Vec<crate::register_ramp::RegisterRamp>,

    /// Noa dance (rhythm) minigame state. `Some` while `mode ==
    /// SceneMode::Dance`; the beat clock + hit judge run each tick. See
    /// [`crate::dance::DanceGame`] and [`World::enter_dance`].
    pub dance: Option<crate::dance::DanceGame>,

    /// The scene mode to restore when the dance minigame ends
    /// ([`World::enter_dance`] snapshots the mode it interrupted). Mirrors the
    /// pause-menu suspend/restore contract.
    pub dance_return_mode: SceneMode,

    /// The most recent dance-press judgement, kept for the host HUD (the
    /// score/gauge banner). Reset to `None` on [`World::enter_dance`]; updated
    /// each frame a directional press is judged.
    pub dance_last_judge: Option<crate::dance::Judge>,

    /// Fishing minigame session. `Some` while `mode == SceneMode::Fishing`; the
    /// cast / fight / score loop runs each tick. See
    /// [`crate::fishing::FishingSession`] and [`World::enter_fishing`].
    pub fishing: Option<crate::fishing::FishingSession>,

    /// The scene mode to restore when the fishing minigame ends
    /// ([`World::enter_fishing`] snapshots the interrupted mode).
    pub fishing_return_mode: SceneMode,

    /// Persistent fishing-point pool, mirroring retail's `_DAT_8008444C`
    /// counter: [`World::exit_fishing`] banks the session record's points
    /// here, and the point exchange spends from it
    /// ([`World::fishing_exchange_buy`]). Hosts seed a new session's
    /// [`crate::fishing::FishingRecord`] from this cell.
    pub fishing_points: i32,

    /// Persistent one-time prize bitmask, mirroring retail's `_DAT_8008446C`:
    /// bit `row + venue * 8` latches when a `limit == 1` exchange row is
    /// bought (see [`legaia_asset::fishing_exchange`]).
    pub fishing_prizes_purchased: u32,

    /// Fishing point-exchange (prize shop) session. `Some` while the exchange
    /// list is open on the host's fishing screen; purchases commit through
    /// [`World::fishing_exchange_buy`].
    pub fishing_exchange: Option<crate::fishing::PrizeExchange>,

    /// Slot-machine minigame session. `Some` while
    /// `mode == SceneMode::SlotMachine`; the reel state machine runs each
    /// tick. See [`crate::slot_machine::SlotMachine`] and
    /// [`World::enter_slot_machine`].
    pub slot_machine: Option<crate::slot_machine::SlotMachine>,

    /// The scene mode to restore when the slot-machine minigame ends
    /// ([`World::enter_slot_machine`] snapshots the interrupted mode).
    pub slot_return_mode: SceneMode,

    /// Baka Fighter duel state. `Some` while `mode ==
    /// SceneMode::BakaFighter`; the exchange / round / match state machine
    /// runs each tick. See [`crate::baka_fighter::BakaFight`] and
    /// [`World::enter_baka_fighter`].
    pub baka_fighter: Option<crate::baka_fighter::BakaFight>,

    /// The scene mode to restore when the Baka Fighter match ends
    /// ([`World::enter_baka_fighter`] snapshots the interrupted mode).
    pub baka_return_mode: SceneMode,

    /// Muscle Dome contest state. `Some` while `mode ==
    /// SceneMode::MuscleDome`; the hand-select / commit / resolve loop runs
    /// each tick. See [`crate::muscle_dome::MuscleDomeSession`] and
    /// [`World::enter_muscle_dome`].
    pub muscle_dome: Option<crate::muscle_dome::MuscleDomeSession>,

    /// The scene mode to restore when the Muscle Dome contest ends
    /// ([`World::enter_muscle_dome`] snapshots the interrupted mode).
    pub muscle_return_mode: SceneMode,

    /// The casino coin bank (`_DAT_800845A4`, the GameShark "Infinite
    /// Coins" cell). Read to seed the slot machine's playing balance and
    /// **assigned** its final balance on cash-out (the retail state-100
    /// commit is an assignment, not a delta).
    pub casino_coins: u32,

    /// The mode-24 minigame door-warp's backup of the active scene name
    /// (retail `0x8007BAE8`, written by the OTHER-INIT entry `FUN_80025980`
    /// from `0x80084548`). [`World::minigame_return_warp`] restores it into
    /// [`World::active_scene_label`] on exit. `None` while no warp is armed.
    pub minigame_scene_backup: Option<String>,

    /// The mode-24 session-winnings accumulator (retail `_DAT_80084440`,
    /// zeroed by the field-VM `0x3E` warp arm; the minigame overlays add
    /// their winnings here). [`World::minigame_return_warp`] commits it
    /// into [`World::casino_coins`].
    pub minigame_winnings: u32,

    /// Per-actor status-effect tracker (Toxic / Numb / Venom /
    /// Sleep / Confuse / Curse / Stone / Faint). Populated by
    /// [`World::fold_battle_event`] on `ApplyArtStrike` events whose
    /// `enemy_effect` is non-`None`; ticked per turn by engines that
    /// drive a battle round. See [`legaia_engine_vm::status_effects`].
    pub status_effects: vm::status_effects::StatusEffectTracker,

    /// Per-character AP gauge - drives Tactical-Arts command input.
    /// Index 0..=2 maps to party slots; engines call
    /// [`crate::ap_gauge::ApGauge::reset_for_turn`] at turn start and
    /// [`crate::ap_gauge::ApGauge::charge_spirit`] when the player
    /// presses Spirit during command input.
    pub ap_gauges: [crate::ap_gauge::ApGauge; 3],

    /// Per-party-slot guard stance for the current battle: `true` after the
    /// slot's **Spirit** command until its next turn starts. The retail state
    /// is the actor's pending-action byte `+0x1DE == 4` (Spirit), consumed by
    /// the damage finisher's guard-halve stage
    /// ([`legaia_engine_vm::battle_formulas::DamageFinish::defender_guarding`]).
    pub battle_guarding: [bool; 3],

    /// Per-party-slot Fury Boost state for the current battle: `Some(delta)` is
    /// the AP added to that slot's gauge by the class-5 Fury Boost item (retail
    /// actor `+0x1F9` flag). Reverted wholesale at battle end (`finish_battle`),
    /// the same lifecycle as [`Self::battle_buffs`]; `None` = not boosted.
    pub fury_boost: [Option<u8>; 3],

    /// Item catalog used by item-action resolution. Populated at battle
    /// init from [`crate::items::ItemCatalog::vanilla`] (or a custom
    /// catalog set by [`World::set_item_catalog`]); empty by default so
    /// the field VM doesn't trigger item effects in non-battle scenes.
    pub item_catalog: crate::items::ItemCatalog,

    /// Real on-disc item-effect descriptor table ([`legaia_asset::item_effect`],
    /// `DAT_800752C0`), if the boot source's `SCUS_942.54` was readable. When
    /// present, [`Self::set_item_catalog`] applies its field/battle usability
    /// flags onto the installed catalog so item-menu gating matches retail.
    pub item_effects: Option<legaia_asset::item_effect::ItemEffectTable>,

    /// Spell catalog used by the player-driven battle Magic submenu to resolve
    /// spell ids → names / MP cost / effect. Populated at battle init from
    /// [`crate::spells::SpellCatalog::vanilla`] (or a custom catalog via
    /// [`World::set_spell_catalog`]); empty by default.
    pub spell_catalog: crate::spells::SpellCatalog,

    /// Art-record catalog used by the player-driven battle Arts submenu to
    /// resolve a saved chain → its real per-strike power profile. Keyed by
    /// `(character, art constant)`; populated from disc art data (PROT entry
    /// `0x05C4`) via [`World::set_art_record`] when available. Empty by
    /// default - the Arts submenu then falls back to a synthetic power profile
    /// derived from the chain's directional commands
    /// (see [`crate::battle_arts::synthetic_power`]).
    pub art_records: std::collections::HashMap<
        (legaia_art::Character, legaia_art::ActionConstant),
        legaia_art::ArtRecord,
    >,

    /// Per-actor character max MP. The retail `BattleActor` holds only
    /// the running `mp` value (not the cap); the cap lives on the
    /// character record at `+0x140`. Engines populate this from the
    /// character record at battle init.
    pub character_max_mp: Vec<u16>,

    /// Active encounter session - bracketed transition + grace machine for
    /// step-driven random battles. `Some` when an encounter table is
    /// installed; `None` in scenes where encounters are disabled
    /// (towns / cutscenes / world-map). Engines call
    /// [`World::on_field_step`] from the field-step path (player walks one
    /// tile) to advance the tracker; the resulting [`crate::encounter::EncounterPhase`]
    /// drives the camera-shake / fade / battle-load chain.
    pub encounter: Option<crate::encounter::EncounterSession>,

    /// Per-character v2 save extension data. Mirrors `SaveExtV2` shape;
    /// engines populate from in-memory state at save time and consume on
    /// load. Index 0..=2 = main characters; entries beyond are story
    /// guests. Each entry holds learned-arts mask, learned spells, seru
    /// captures, and per-character active chain quick-slots.
    pub per_char_ext: Vec<(u8, legaia_save::CharSaveExt)>,

    /// Cross-character saved-chain library. Engines populate from a
    /// [`crate::tactical_arts_editor::ChainLibrary`] at save time and
    /// hydrate one back into the editor on load.
    pub saved_chains: Vec<legaia_save::SavedChainRecord>,

    /// Per-character Seru capture log - drives the post-battle "spell
    /// learned!" banner and the in-menu spell list. Pure data; saved
    /// through [`legaia_save::SaveExtV2::per_char`].
    pub seru_log: crate::seru_learning::SeruCaptureLog,

    /// Master Seru registry (Seru id -> spell taught + capture points).
    /// Engines install via [`World::set_seru_registry`]; `World::finish_battle`
    /// resolves [`World::battle_captures`] against it into [`World::seru_log`].
    /// Empty by default - captures then bank no points (the monster is still
    /// downed + logged, but nothing is learned).
    pub seru_registry: crate::seru_learning::SeruRegistry,

    /// Capture outcomes produced by the most recently finished battle, one per
    /// captured Seru that the registry accepted. Hosts drain this with
    /// [`World::drain_last_capture_outcomes`] to drive the "captured / learned"
    /// banner ([`crate::seru_learning::SeruCaptureSession`]).
    pub last_capture_outcomes: Vec<crate::seru_learning::CaptureOutcome>,

    /// Total game time in wall-clock seconds since the world was
    /// instantiated or loaded. Engines tick this independently of
    /// `frame` (which can pause-skip during dialogs / cutscenes).
    /// Persisted in [`legaia_save::SaveExtV2::play_time_seconds`].
    pub play_time_seconds: u32,

    /// Optional formation table - engines install this at boot via
    /// [`World::set_formation_table`] so triggered encounters can resolve
    /// their `formation_id` into concrete monster slot definitions.
    pub formation_table: crate::monster_catalog::FormationTable,

    /// Optional monster catalog - paired with `formation_table`. Engines
    /// look up [`crate::monster_catalog::MonsterDef`] by id when
    /// initialising the [`crate::battle_session::BattleSession`].
    pub monster_catalog: crate::monster_catalog::MonsterCatalog,

    /// Optional battle-action move-power table (PROT 0898, runtime VA
    /// `0x801F4F5C`). When present, the monster special-attack damage path
    /// resolves each move id to its real per-move power scalar and rolls
    /// damage through the faithful arts/physical kernel
    /// (`crate::world::World::enemy_move_predamage`); when `None` (disc-free /
    /// synthetic battles) that path falls back to the MP-scaled placeholder, so
    /// the RNG stream and determinism trace are unchanged. Installed lazily from
    /// the disc by [`crate::scene::SceneHost`].
    pub move_power: Option<crate::move_power::MovePowerCatalog>,

    /// Raw bytes of the battle-action overlay (PROT 0898) the [`move_power`]
    /// catalog was parsed from, retained so the move-FX render path can read the
    /// `0x801f6324` prototype records' move-VM bytecode (the catalog holds only
    /// the parsed tables). Installed alongside [`move_power`] by
    /// [`crate::scene::SceneHost`]; `None` in disc-free battles (move FX simply
    /// don't spawn). `Arc` so cloning `World` stays cheap.
    pub move_power_overlay: Option<Arc<[u8]>>,

    /// Battle **element-affinity** tables ([`legaia_asset::element_affinity`],
    /// matrix `0x801F53E8` + per-character element table `0x801F5480`). When
    /// present, the monster special-attack damage path scales the attacker roll
    /// by `matrix[enemy_element][party_member_element]` (`FUN_801dd864`);
    /// `None` (disc-free / synthetic battles) keeps the neutral 100% multiplier,
    /// so the damage and determinism trace are unchanged. Installed lazily from
    /// the same PROT 0898 overlay as [`Self::move_power`] by
    /// [`crate::scene::SceneHost`].
    pub element_affinity: Option<legaia_asset::element_affinity::ElementAffinity>,

    /// Item-table data the gold-shop path needs from `SCUS_942.54` (per-id buy
    /// price + a "names a real item" mask). Installed once at boot by the host
    /// (e.g. `BootSession`); `None` on disc-free builds, which leaves shop stock
    /// host-supplied and unpriced. See [`crate::shop_catalog`].
    pub item_shop_data: Option<crate::shop_catalog::ShopItemData>,

    /// Gold shops located in the active scene's MAN, priced from
    /// [`Self::item_shop_data`]. Repopulated on each field-scene entry by
    /// [`crate::scene::SceneHost::enter_field_scene`]; empty when the scene has
    /// no merchant or the disc isn't available. The field-menu shop-open path
    /// picks from these instead of a hand-authored stock list.
    pub scene_shops: Vec<crate::shop_catalog::SceneShop>,

    /// A priced shop the field VM has just opened (op `0x49` sub-0 inline shop
    /// record - see [`Self::try_arm_field_shop`]). The host drains it with
    /// [`Self::take_pending_field_shop`] to drive the buy/sell UI, then calls
    /// [`Self::finish_field_shop`] when the player leaves so the field VM
    /// resumes past the op. `None` between shop opens.
    pub pending_field_shop: Option<crate::shop::ShopSession>,

    /// `true` from the frame a field-VM shop op (`0x49` sub-0) is recognised
    /// until the op's resume runs - it gates the op-0x49 tristate so the VM
    /// stays suspended while the shop is up. Distinguishes a shop arm from the
    /// name-entry arm and a plain script yield.
    pub field_shop_armed: bool,

    /// `true` while the opened shop UI is still up; the host clears it via
    /// [`Self::finish_field_shop`] so the op-0x49 tristate flips Armed -> Done.
    pub field_shop_open: bool,

    /// Per-item battle-stat modifier table (weapon / armor / accessory
    /// bonuses). Empty by default; install via [`World::set_equipment_table`]
    /// so [`World::seed_party_battle_stats`] folds equipped gear onto each
    /// party combatant's attack / defense at battle entry.
    pub equipment_table: crate::battle_stats::EquipmentTable,

    /// Accessory ("Goods") passive-effect catalog: item id → passive index +
    /// per-index party-wide scope, decoded from the executable. Empty by
    /// default; install via [`World::set_accessory_passives`].
    /// [`World::refresh_party_ability_bits`] derives each member's ability
    /// bitfield from it, and
    /// [`crate::battle_stats::compute_battle_stats_with_passives`] applies the
    /// percent stat boosts inside [`World::seed_party_battle_stats`].
    pub accessory_passives: crate::accessory_passives::AccessoryPassives,

    /// Party-global 4×u32 ability mask - the engine mirror of retail
    /// `DAT_80074358..0x80074368` (every member's `+0xF4` bitfield OR'd
    /// together each rebuild). Bit-tested via [`World::party_has_ability`]
    /// (the `FUN_800431D0` port); rebuilt by
    /// [`World::refresh_party_ability_bits`].
    pub party_ability_mask: [u32; crate::accessory_passives::ABILITY_WORDS],

    /// Battle-scoped monster-AI state (cooldowns / phase counter / recent-target
    /// ring) read & written by the per-monster-id scripted-cast picker
    /// ([`crate::monster_ai::decide`]). Reset on each battle enter.
    pub monster_ai_state: crate::monster_ai::MonsterAiState,

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
    /// Read by the field-VM `0x4C 0xD8` host hook to populate
    /// [`Actor::tmd_ref`] on synchronous-spawn.
    pub global_tmd_pool: Vec<Option<Arc<GlobalTmd>>>,

    /// Active Seru-magic summon scene-graph, while one is playing. Spawned off
    /// the battle-action cast band (or [`World::spawn_summon`] for the debug
    /// path); ticked each frame through the move VM by [`World::tick_summon`]
    /// and drained when every part finishes. Rendered via
    /// [`World::active_summon_part_draws`].
    pub active_summon: Option<crate::summon::SummonScene>,
    /// Production cast-band request: a player Seru-magic cast (spell id
    /// `0x81..=0x8b`) sets `(spell_id, target world pos)` here - the engine
    /// equivalent of the retail cast band resolving the per-summon overlay
    /// (`FUN_8003EC70(id-0x79)`). The host (which has the PROT index) drains it
    /// via [`World::take_pending_summon_spawn`], loads the summon overlay
    /// (extraction `PROT 903 + (id - 0x81)`), and calls [`World::spawn_summon`]. Kept as a
    /// host-fulfilled request because `World` is index-agnostic (same pattern
    /// as the capture-archive load).
    pub pending_summon_spawn: Option<(u8, [i16; 3])>,

    /// Production battle-FX request for a **non-summon** move: a spell cast or
    /// enemy special whose move-power record carries a spawnable effect list
    /// sets `(move_id, target world pos)` here (see [`World::request_move_fx_spawn`]).
    /// The host drains it via [`World::take_pending_move_fx_spawn`] and calls
    /// [`World::spawn_move_fx`] (which reads the retained PROT 0898 overlay).
    /// The sibling of [`pending_summon_spawn`](Self::pending_summon_spawn) for
    /// the move-FX (rather than summon-creature) render path.
    pub pending_move_fx_spawn: Option<(u8, [i16; 3])>,

    /// Active battle move-power effect-FX scene-graph, while one is playing. A
    /// move's `0x01..=0x63` on-contact / launch effect-list entries spawn the
    /// `0x801f6324` prototype records (summon-format move-VM parts) through the
    /// same machinery as a summon - [`World::spawn_move_fx`] seeds it,
    /// [`World::tick_move_fx`] advances it, [`World::active_move_fx_part_draws`]
    /// renders it. Separate from [`active_summon`](Self::active_summon) so a
    /// move's FX and a summon don't clobber each other.
    pub active_move_fx: Option<crate::summon::SummonScene>,

    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, set by [`World::spawn_move_fx`] from the move record's
    /// `+0x0b` field and cleared when the scene drains. Surfaced via
    /// [`World::active_move_fx_trail_texpage`] for the render layer's streak
    /// pass - the trail id this carries is what
    /// `legaia_engine_render::afterimage::build_afterimage_quad` (the ported
    /// `FUN_801e1ab0`) turns into the jittered semi-transparent quad.
    pub active_move_fx_trail_texpage: Option<u16>,

    /// The current scene's **field move-VM stager table** - the prescript
    /// records (`scene_event_scripts` / `scene_v12_table` offset `0x800`) parsed
    /// as summon-format move-VM stager records, the field-resident sibling of the
    /// per-summon stagers (see `docs/formats/scene-v12-table.md` +
    /// `legaia_asset::scene_event_scripts::move_stager_records`). The field VM's
    /// op `0x34` sub-3 ("Play 3D animation") installs one by id through
    /// `FUN_800252EC` → the part-stager `FUN_80021B04` → the move VM; the engine
    /// mirrors that in [`World::spawn_field_stager`]. Empty until
    /// [`World::install_field_stagers`] runs at scene entry. Distinct from the
    /// field-VM bytecode the scene also runs (`field_bytecode`); these records are
    /// the move-VM side of the same prescript bundle.
    pub field_stagers: Vec<legaia_asset::summon_overlay::SummonPart>,
    /// The prescript bundle bytes the [`field_stagers`](Self::field_stagers)
    /// records index into (needed to seed a part's move buffer when spawning).
    pub field_stager_bytes: Vec<u8>,
    /// Live field move-VM scene-graph effects spawned by op `0x34` sub-3, each a
    /// one-part [`crate::summon::SummonScene`]; ticked by
    /// [`World::tick_field_fx`], drawn via [`World::active_field_fx_part_draws`],
    /// with the non-visual nodes (the `0x4001` sound emitter) surfaced separately
    /// through [`World::active_field_fx_render_nodes`]. A `Vec` because several
    /// can be live at once (the prescript triggers them independently).
    pub active_field_fx: Vec<crate::summon::SummonScene>,

    /// Adaptive frame-step factor `dt` - the retail scratchpad byte
    /// `DAT_1F800393`, the number of *vsyncs per game tick*. The frame-flip
    /// path (`FUN_80016B6C`, see `ghidra/scripts/funcs/80016b6c.txt`) rewrites
    /// it every frame from the measured frame cost (`1`, `2` past `0xF0`, `3`
    /// past `0x1FE`, `4` past `0x2D0`), clamped up to the per-mode floor
    /// `_DAT_8007B9D8`. Live poll baselines: field/town scenes run at `2`
    /// (30 fps) and the overworld kingdom scenes (`mapNN`) at `3` (20 fps) -
    /// the engine pins those per-scene values on entry
    /// ([`crate::scene::SceneHost::enter_field_scene`]) rather than modelling
    /// the load-adaptive writer. Consumed by everything that advances
    /// per-game-tick in vsync units - the scripted CLUT fades
    /// ([`Self::step_clut_fx`]) and the shell's CLUT-cycle cadence.
    ///
    /// REF: FUN_80016B6C
    pub frame_step: u8,
    /// Vsyncs accumulated toward the next retail *game tick* (a game tick
    /// spans [`Self::frame_step`] vsyncs). Advanced by [`World::tick`] on the
    /// sim ticks that map to a retail vsync ([`Self::field_frame_step`]).
    pub clut_vsync_accum: u8,
    /// Retail game ticks elapsed since the host last drained the scripted
    /// CLUT effects ([`World::step_clut_fx`] consumes these). Only
    /// accumulates while [`Self::clut_fx`] is non-empty, and saturates at a
    /// small cap so a host that never drains can't wind up an unbounded
    /// backlog.
    pub clut_pending_game_ticks: u32,
    /// Live scripted CLUT-cell effects (field-VM `0x4C` n6 sub-`0x61`):
    /// pending one-shot cell writes and in-flight cross-fades. Spawned by
    /// [`World::spawn_clut_cell_fx`] (the `op4c_n6_sub_61_emitter` host
    /// hook), stepped + applied against the host's software VRAM by
    /// [`World::step_clut_fx`], cleared on scene entry.
    pub clut_fx: Vec<crate::world::ClutCellFx>,

    /// Pending move-FX sound cue id (`+0x0d`), set by [`World::spawn_move_fx`]
    /// when the move carries a non-zero cue. The host drains it via
    /// [`World::take_pending_move_fx_cue`] and routes it through
    /// `legaia_engine_audio::classify_cue` → the SFX ring / voice trigger
    /// (the retail `FUN_8004fcc8` dispatch). Same host-fulfilled-request shape
    /// as [`pending_summon_spawn`](Self::pending_summon_spawn).
    pub pending_move_fx_cue: Option<u8>,

    // --- live gameplay loop (Field <-> Battle round trip) -----------------
    /// Master opt-in for the in-`tick` Field <-> Battle round trip.
    ///
    /// When `false` (the default) [`World::tick`] keeps its historical
    /// behaviour: the Field branch runs the field VM + locomotion but never
    /// rolls encounters, and the Battle branch runs a single
    /// [`World::step_battle`] without applying damage or re-arming the SM
    /// (engines / tests drive those externally). When `true`, [`World::tick`]
    /// drives the whole loop itself - step-driven encounter roll, automatic
    /// `Field -> Battle` transition resolving a real formation, an in-engine
    /// physical-attack battle resolver, and the `Battle -> Field` return with
    /// loot applied. Hosts that want a playable slice (`legaia-engine
    /// play-window`, the v0.1 playthrough oracle) set this once after boot.
    pub live_gameplay_loop: bool,

    /// Opt-in, NON-FAITHFUL gameplay tweak: when a monster picks a single
    /// living party member to attack, override the (faithful, random) choice
    /// with the lowest-HP living member. Off by default - the retail behaviour
    /// is a uniform random target. The faithful random target is still rolled
    /// in full (identical RNG-call count + stream); only the final single
    /// party slot is replaced, so a replay stays internally deterministic and
    /// all downstream battle RNG is unaffected. All-party / monster-band / self
    /// targets are never touched.
    pub smarter_monster_targeting: bool,

    /// Opt-in: route field NPC dialogue through the inline-script field-VM
    /// runner ([`Self::drive_inline_dialogue`]) instead of the simplified
    /// `current_dialog` / `OwnedDialogPanel` path, so dialogue branch handlers
    /// actually execute (story-flag tests, `SET`/`CLEAR`, scene changes). Off
    /// by default - when off, behaviour is identical to before.
    pub use_vm_dialogue: bool,

    /// Opt-in: route the live basic-attack damage through the retail damage
    /// finisher ([`legaia_engine_vm::battle_formulas::damage_finish`], the port
    /// of `FUN_801ddb30`) instead of stopping at the raw roll. The finisher
    /// adds the universal post-stages - elemental resistance, guard / enemy
    /// halve, the rand-based no-damage floor, and the 9999 cap. Equipment
    /// resistance + guard state aren't modelled on the battle actor yet, so
    /// those inputs default to "no mitigation"; with the gate on the finisher
    /// currently contributes the faithful 9999 cap and the `rand()%9+8` floor
    /// on a zeroed hit. Off by default so the existing flat path (min-floor 1,
    /// `0xFFFF` cap) and its RNG call-count stay the default. The finisher
    /// draws one RNG **only** when the hit zeroes out, matching retail.
    pub use_damage_finish: bool,

    /// Opt-in for a **player-driven** battle inside the live loop. When
    /// `false` (the default) the live loop auto-resolves each party turn with
    /// a physical Attack on the first living monster (the historical spine
    /// behaviour). When `true`, every party turn pauses the action SM and runs
    /// a [`crate::battle_input::BattleCommandSession`] that reads
    /// [`World::input`]: the player selects a command from the battle command
    /// menu and a target before the strike commits. Requires
    /// [`Self::live_gameplay_loop`]; hosts that want a playable battle
    /// (`legaia-engine play-window`) set both after boot. All four commands
    /// are wired: Attack strikes; Arts / Magic / Item open
    /// [`Self::battle_arts_menu`] / [`Self::battle_spell_menu`] /
    /// [`Self::battle_item_menu`].
    pub battle_player_driven: bool,

    /// Active command-selection session for the player-driven battle. `Some`
    /// only while a party member is choosing a command/target (the action SM
    /// is parked meanwhile); `None` when the SM is running or outside battle.
    /// Managed by the live loop; hosts read it to draw the command menu /
    /// target cursor.
    pub battle_command: Option<crate::battle_input::BattleCommandSession>,

    /// Active inventory submenu for the player-driven battle, opened when the
    /// player picks **Item** from the command menu. `Some` while the player
    /// browses items / picks a target (both the action SM and
    /// [`Self::battle_command`] are parked meanwhile); `None` otherwise. The
    /// World owns it - not [`crate::battle_input::BattleCommandSession`] -
    /// because it needs the live inventory + party stats. Hosts read it to
    /// draw the item overlay.
    pub battle_item_menu: Option<crate::inventory_use::InventoryUseSession>,

    /// Active spell submenu for the player-driven battle, opened when the
    /// player picks **Magic** from the command menu. `Some` while the player
    /// browses spells / picks a target (both the action SM and
    /// [`Self::battle_command`] are parked meanwhile); `None` otherwise. The
    /// World owns it because building the spell list needs the caster's
    /// learned spells + live MP. Hosts read it to draw the spell overlay.
    pub battle_spell_menu: Option<crate::battle_magic::BattleSpellSession>,

    /// Active Arts submenu for the player-driven battle, opened when the player
    /// picks **Arts** from the command menu. `Some` while the player browses
    /// saved chains / picks a target (the action SM and [`Self::battle_command`]
    /// are parked meanwhile); `None` otherwise. The World owns it because each
    /// row's power profile is resolved from [`Self::saved_chains`] +
    /// [`Self::art_records`] by `Self::build_battle_arts_rows`. Hosts read it
    /// to draw the arts overlay.
    pub battle_arts_menu: Option<crate::battle_arts::BattleArtsSession>,

    /// Active stat buffs / debuffs applied by battle Magic, one entry per
    /// `(slot, stat)`. Each holds the exact delta written into the per-slot
    /// scalar so expiry can undo it, plus the remaining turn count (decremented
    /// at the start of the buffed actor's turn). Cleared - and their deltas
    /// reverted - by `World::finish_battle`.
    pub battle_buffs: Vec<BattleBuff>,

    /// Monster ids captured this battle by a capture spell (`SpellEffect::Capture`).
    /// The captured monster is downed immediately; the host drains this for
    /// post-battle Seru-learning resolution (the live loop carries no Seru
    /// registry, so the learn step itself lives outside the battle tick).
    pub battle_captures: Vec<u16>,

    /// Chance, in percent, that a capturable enemy spawns as a **shiny**
    /// variant in a given battle: a single rare enemy with +35% stats whose
    /// captured Seru deals +35% damage forever (see the `--shiny-seru`
    /// randomizer feature). `0` disables. Default
    /// [`World::DEFAULT_SHINY_CHANCE_PCT`].
    pub shiny_chance_pct: u8,

    /// Battle slots flagged shiny this battle (filled by
    /// [`World::roll_shiny_enemy`] at battle entry, drained at battle end).
    /// A shiny enemy's stats are pre-boosted; capturing it marks the learned
    /// spell shiny.
    pub shiny_enemy_slots: std::collections::HashSet<u8>,

    /// Monster ids captured **as shiny** this battle (subset of
    /// [`Self::battle_captures`]; `resolve_captures` marks their spell shiny).
    pub shiny_captures: Vec<u16>,

    /// Magic-XP threshold table from `SCUS_942.54` (`0x8007656C`, 8 ascending
    /// u16 steps). Installed at boot via
    /// [`World::install_magic_xp_thresholds`]; while `None` (disc-free) summon
    /// casts still accrue spell XP but never level the spell up.
    pub magic_xp_thresholds: Option<[u16; crate::magic_xp::THRESHOLD_STEPS]>,

    /// Seru-trade config from the patched disc (the randomizer's `--seru-trade`
    /// blob: enabled flag + master seed + offer cap). Installed at boot via
    /// [`World::install_seru_trade_config`]; `None` (or `enabled == false`)
    /// disables vendor seru trading. See [`crate::seru_trade`].
    pub seru_trade_config: Option<legaia_asset::seru_trade::SeruTradeConfig>,

    /// Summon-magic level-ups resolved this session: `(party_slot, spell_id,
    /// new_level)` per event, in resolution order. The engine analogue of the
    /// retail level-up banner (the level-up check fires UI element `0x65` -
    /// REF: FUN_801e70bc, ported in `world::battle::accrue_summon_spell_xp`);
    /// hosts drain via [`World::drain_magic_level_ups`].
    pub magic_level_ups: Vec<(u8, u8, u8)>,

    /// Set when an escape spell (`SpellEffect::Escape`) resolves. The live
    /// battle tick returns to the field on the next pass (no loot, no
    /// game-over). Cleared by `World::finish_battle`.
    pub battle_escaped: bool,

    /// Scripted "can't run from this battle" flag (retail battle ctx
    /// `+0x287`, the input `FUN_801E791C`'s escape roll tests). Set when a
    /// battle enters through the field-VM scripted-battle op
    /// ([`World::trigger_scripted_battle`]) - the boss rows carry a non-zero
    /// first header byte the retail reader ORs `0x80` into a battle-setup
    /// flag for - so a staged boss fight refuses the Run command (fleeing
    /// would leave the stager's marker set and let the post-victory record
    /// spawn unearned). Cleared by [`World::finish_battle`].
    pub battle_no_escape: bool,

    /// Formation currently being fought, captured at the `Field -> Battle`
    /// transition. Drives [`World::apply_battle_loot`] on victory. `None`
    /// outside battle.
    pub active_formation: Option<crate::monster_catalog::FormationDef>,

    /// Boss-stager bindings for the active scene, keyed by partition-1
    /// placement slot: the record an approach (walk-touch) or interact on
    /// that placed actor runs through the field VM. Derived from the scene
    /// MAN's own bytes at entry
    /// ([`World::install_boss_stagers_from_man`]); consumed by
    /// [`World::run_boss_stager_record`] (rikuroa's Caruban stager `P1[3]`:
    /// `52 89` staged-marker SET then `3E FF 11` battle entry - every flag
    /// in the chain lands from the record's own script bytes, nothing is
    /// engine-stamped).
    pub field_boss_stagers: std::collections::HashMap<u8, crate::world::FieldBossStager>,

    /// Aggregated rewards from the most recent victory - surfaced for the
    /// post-battle banner / HUD. `None` until the first battle resolves.
    pub last_battle_rewards: Option<BattleRewards>,

    /// Set when the live loop resolves a battle to
    /// [`BattleEndCause::PartyWipe`]. v0.1 has no game-over screen, so the
    /// loop returns to the field with this flag raised; hosts read it to
    /// surface a defeat state.
    pub game_over: bool,

    /// Field state captured at the `Field -> Battle` transition so the live
    /// loop can restore it on victory. The retail engine re-enters the field
    /// scene from scratch; the clean-room loop snapshots the actor table +
    /// player slot instead. `None` outside battle. Managed by the live loop;
    /// hosts read [`Self::mode`] / [`Self::active_formation`] instead.
    pub field_return: Option<FieldReturnState>,

    /// Player tile `(col, row)` on the previous live-loop field tick. A
    /// change between ticks is one "step" and drives the encounter roll,
    /// mirroring the retail per-step counter rather than a per-frame roll.
    /// `None` until the first field tick records a tile. Managed by the live
    /// loop.
    pub field_last_tile: Option<(i16, i16)>,

    /// Per-entity world-map state machines (the port of `FUN_801DA51C` in
    /// [`vm::world_map`]). One [`vm::world_map::WorldMapEntityCtx`] per
    /// installed overworld entity (encounter zones / town portals / NPCs).
    /// Empty unless [`Self::install_world_map_entities`] seeded them, so
    /// world-map mode without gameplay (camera-only) keeps ticking untouched.
    /// Driven each [`SceneMode::WorldMap`] tick by `Self::tick_world_map`.
    pub world_map_entities: Vec<vm::world_map::WorldMapEntityCtx>,

    /// Per-entity role config, paired by index with [`Self::world_map_entities`].
    /// Empty (or shorter than the entity list) means an entity has no specific
    /// role: its encounters fall back to [`Self::world_map_encounter`]'s shared
    /// formation and it surfaces a plain interaction. Installed together with
    /// the entities via [`Self::install_world_map_entities_with_configs`].
    pub world_map_entity_configs: Vec<WorldMapEntityConfig>,

    /// Per-entity overworld world position `(x, z)`, paired by index with
    /// [`Self::world_map_entities`]. Populated only by
    /// [`Self::install_world_map_entities_at`] (the disc placement seeding);
    /// the config-only installers leave it empty. When present, it drives the
    /// **auto-engage-on-walkover** trigger in `Self::tick_world_map`: the
    /// player stepping onto a `Portal` entity's tile fires its transition with
    /// no host call, the clean-room stand-in for retail's per-entity
    /// player-position-in-zone check.
    pub world_map_entity_positions: Vec<(i16, i16)>,

    /// Shared overworld encounter-rate state - the retail globals the
    /// world-map entity SM reads (`DAT_8007b604` countdown, `DAT_8007b5f8`
    /// enable flag) plus the formation an overworld encounter spawns.
    pub world_map_encounter: WorldMapEncounterState,

    /// Whether the player is moving on the overworld this tick (the entity
    /// SM's `_DAT_8007c364[+0x10] & 0x80000` player-walking gate). Set from
    /// the pad each world-map tick; a stationary player lets the interaction
    /// check fire.
    pub world_map_player_walking: bool,

    /// Overworld encounter pending resolution into a battle: the formation id
    /// an entity SM's encounter handler latched this frame. Drained at the end
    /// of `Self::tick_world_map` to flip into [`SceneMode::Battle`]. `None`
    /// between encounters.
    pub pending_world_map_encounter: Option<u16>,

    /// Region-keyed random-encounter state for the overworld (the
    /// `FUN_801D9E1C` port, [`crate::region_encounter`]). When set,
    /// `Self::tick_world_map` rolls it once per 128-unit tile the player
    /// crosses, latching [`Self::pending_world_map_encounter`] on a trigger.
    /// `None` on a camera-only world map (no region data routed).
    ///
    /// REF: FUN_801D9E1C
    pub world_map_region_tracker: Option<crate::region_encounter::RegionEncounterTracker>,

    /// Player tile (`world >> 7`) at the previous overworld step check, for
    /// per-tile step detection. `None` until the first world-map tick seeds it.
    pub world_map_last_tile: Option<(i32, i32)>,

    /// Region-keyed random-encounter state for the current FIELD scene (the
    /// same [`crate::region_encounter`] `FUN_801D9E1C` port the overworld
    /// uses, [`Self::world_map_region_tracker`]). When set,
    /// [`Self::on_field_step`] rolls against the player's *active region*
    /// (per-region rate increment + formation-range pick) and drives the
    /// trigger through the [`crate::encounter::EncounterSession`]'s
    /// transition / grace SM, instead of the session's mean-rate tracker.
    /// `None` on scenes whose MAN has no encounter-region section (towns,
    /// or any engine that hasn't routed per-region data) - those fall back
    /// to the aggregated mean-rate `EncounterSession`.
    ///
    /// REF: FUN_801D9E1C
    pub field_region_tracker: Option<crate::region_encounter::RegionEncounterTracker>,

    /// Overworld player walk speed in world units per frame (per held d-pad
    /// direction). Default [`Self::WORLD_MAP_PLAYER_SPEED`].
    pub world_map_player_speed: i16,

    /// Scene mode to return to when the current battle finishes. Captured at
    /// the transition into [`SceneMode::Battle`]; `Self::finish_battle`
    /// restores it (an overworld encounter returns to [`SceneMode::WorldMap`],
    /// a field encounter to [`SceneMode::Field`]). Defaults to
    /// [`SceneMode::Field`].
    pub battle_return_mode: SceneMode,

    /// Per-entity **field** state machines - the same `FUN_801DA51C` SM the
    /// overworld uses ([`vm::world_map`]), but ticked in [`SceneMode::Field`]
    /// for the scene's MAN-placed actors. A scripted-encounter carrier (the
    /// Rim Elm Tetsu fight) sits Idle until [`Self::engage_field_carrier`]
    /// (the dialogue-accept) advances it to `Activating`; the next
    /// `Self::tick_field_carriers` then copies its formation and launches the
    /// battle, mirroring retail's state-1 `entity[+0x94]` copy + `case 2/3`
    /// fall-through battle handoff. Empty unless
    /// [`Self::install_field_carriers`] seeded them.
    pub field_carriers: Vec<vm::world_map::WorldMapEntityCtx>,

    /// Per-carrier role config, paired by index with [`Self::field_carriers`].
    pub field_carrier_configs: Vec<FieldCarrierConfig>,

    /// Field carrier battle pending resolution: the MAN `formation_id` a
    /// carrier SM latched on its scene-transition this frame. Drained at the
    /// end of `Self::tick_field_carriers` to flip Field -> Battle. `None`
    /// between transitions.
    pub pending_field_carrier_battle: Option<u16>,

    /// Field-interact `slot` -> [`Self::field_carriers`] index, for the
    /// **scripted-encounter** carriers only. Built by
    /// [`Self::install_field_carriers_from_man`] so a field-interact on the
    /// sparring partner's placement can find its carrier and auto-arm the fight
    /// (the dialogue-accept drives the engage instead of the manual API). Plain
    /// talk NPCs are deliberately absent - interacting with them never launches
    /// a battle.
    pub field_carrier_slots: std::collections::HashMap<u8, usize>,

    /// A scripted-encounter carrier whose dialogue the player opened via a
    /// field-interact and which engages when that dialogue is dismissed (the
    /// accept). Set in `crate::world::vm_hosts`'s `field_interact`, consumed
    /// by the dialog-advance dismiss (`op 0x4C n5 sub-4`). `None` when no
    /// scripted carrier's prompt is up.
    ///
    /// This any-accept path is used for a carrier whose dialogue has **no
    /// picker**. The Rim Elm spar's dialogue *does* (a 4-option menu whose
    /// index-2 entry "I want to practice with you." arms the fight), so it takes
    /// the faithful [`Self::carrier_menu`] path instead - the engage there fires
    /// only on the fight option, matching retail (live-pinned by
    /// `autorun_tetsu_confirm.lua`: a dialog-SM inline picker, cursor at
    /// `*(0x801C6EA4)+0x0C`, confirming index 2 drives `0x03 -> 0x09 -> 0x15`).
    pub pending_carrier_engage: Option<usize>,

    /// The faithful counterpart to [`Self::pending_carrier_engage`]: when the
    /// opened carrier dialogue carries a 4-option picker (the Rim Elm spar menu),
    /// this holds the live menu so the engage fires **only** on the fight option
    /// ("I want to practice with you.", picker index 2 - RE-pinned live by
    /// `autorun_tetsu_confirm.lua`), not on any accept. `None` when the carrier
    /// has no picker (then `pending_carrier_engage` keeps the any-accept path).
    pub carrier_menu: Option<CarrierMenu>,

    /// Per-party-slot display names. Seeded from the starting-party template
    /// at [`Self::seed_starting_party`] and overwritten by the name-entry
    /// overlay ([`Self::open_name_entry`]). Indexed by party slot; a slot with
    /// no entry falls back to the template name at the call site.
    pub party_names: Vec<String>,

    /// Active name-entry overlay session, or `None` when no name is being
    /// entered. Installed by [`Self::open_name_entry`] (the opening `town01`
    /// script's lead-character prompt) and driven by
    /// [`Self::step_name_entry`]; on commit the name lands in
    /// [`Self::party_names`].
    pub name_entry: Option<crate::name_entry::NameEntry>,

    /// Active opening-cutscene narration presenter, or `None` when no cutscene
    /// narration is playing. Installed by [`Self::open_cutscene_narration`]
    /// (the `opdeene` opening prologue) with the inline subtitle pages decoded
    /// from the scene MAN's cutscene-timeline script; its per-page timer is
    /// advanced in [`Self::tick`], and the host renders [`Self::cutscene_narration`]'s
    /// current page. It gates the prologue hand-off: while it is active the
    /// confirm press skips narration pages, and only once it completes does a
    /// confirm reach [`Self::take_prologue_handoff`].
    pub cutscene_narration: Option<crate::cutscene_narration::CutsceneNarration>,

    /// Monotonic counter incremented each time [`Self::open_cutscene_narration`]
    /// installs a crawl block. Lets observers distinguish back-to-back crawl
    /// blocks (a non-blocking crawl opens the next block the same tick the prior
    /// scrolls out) that a rising-edge `active`-watch would merge into one.
    pub cutscene_narration_seq: u32,

    /// Active opening-cutscene timeline executor, or `None` when no cutscene
    /// timeline is running. Installed by
    /// [`Self::load_cutscene_timeline_from_man`] (the `opdeene` opening
    /// prologue) with the partition-2 record that issues `GFLAG_SET 26`;
    /// stepped each frame by [`Self::step_cutscene_timeline`] so the cutscene's
    /// camera path + actor moves play and the hand-off bit fires by execution.
    /// See [`crate::cutscene_timeline::CutsceneTimeline`].
    ///
    /// This is the single **modal** context slot: while it is active the
    /// cutscene camera owns the frame and pad locomotion is locked
    /// ([`Self::cutscene_timeline_active`] gates). Ordinary mid-play spawned
    /// records execute concurrently in [`Self::helper_contexts`] instead and
    /// never seize either.
    pub cutscene_timeline: Option<crate::cutscene_timeline::CutsceneTimeline>,

    /// Concurrent spawned-record contexts: partition-2 records spawned
    /// mid-play (field-VM op-`0x44` outside the opening chain) that execute
    /// as independent field-VM contexts, mirroring retail's per-record spawn
    /// (`FUN_8003BDE0` installs `ctx[+0x90]`/`ctx[+0x9E]` and lets the
    /// per-frame context sweep run it as a sibling). Unlike
    /// [`Self::cutscene_timeline`] these never seize the camera or lock
    /// player locomotion ([`Self::cutscene_timeline_active`] does not cover
    /// them); only cutscene-class records - the opening chain and gated
    /// walk-on beat records - install as the modal timeline. Installed by
    /// [`Self::install_spawned_helper_record`], stepped per frame by
    /// [`Self::step_helper_contexts`], bounded by
    /// [`SPAWNED_CONTEXT_SLOTS`] (retail's context table is a small fixed
    /// actor-slot pool). A completed context is dropped the frame it ends.
    pub helper_contexts: Vec<crate::cutscene_timeline::CutsceneTimeline>,

    /// A running inline interaction script driven through the field VM (the
    /// faithful dialogue path). Opt-in alternative to the simplified
    /// [`Self::current_dialog`] / `OwnedDialogPanel` path: it *executes* the
    /// prologue flag tests, branch flag-sets, and scene changes between text
    /// boxes. See [`crate::inline_dialogue`] and [`Self::step_inline_dialogue`].
    pub inline_dialogue: Option<crate::inline_dialogue::InlineDialogue>,

    /// `true` only while [`Self::step_cutscene_timeline`] is executing the
    /// spawned cutscene context. The field-VM host reads it to suppress the
    /// actor-allocator hook (op `0x4C` n8 sub-0), which in the cutscene context
    /// (target `0xF8`) is the inline-narration text-draw the separate
    /// [`Self::cutscene_narration`] presenter owns - not an actor spawn.
    pub in_cutscene_timeline: bool,

    /// Retail-frame sub-clock accumulator (fixed-point, `RETAIL_FPS` added per
    /// tick, wrapping at `SIM_HZ`). The sim ticks at 100 Hz, but the narration
    /// crawl roller's scroll speed is authored in retail's ~60 fps field frames;
    /// scrolling it once per 100 Hz tick drains the crawl ~1.7x too fast (the
    /// creation crawl finishes ~5 s early, then the caption + between-crawl
    /// camera choreography leave a long dead-air gap). [`Self::tick`] advances
    /// this and derives [`Self::field_frame_step`] so the roller runs at 60 fps.
    /// NB the cutscene *timeline*'s WAIT_FRAMES stay on the 100 Hz sim clock -
    /// their frame-count vs rate errors happen to cancel so block 2 lands at
    /// retail wall-time; pacing them too would push block 2 far too late.
    pub field_frame_accum: u32,

    /// `1` on the ~60 % of sim ticks that map to a retail field frame, else `0`
    /// (derived from [`Self::field_frame_accum`] each [`Self::tick`]). Fed to
    /// the narration roller so the crawl scrolls at retail's 60 fps wall-speed.
    pub field_frame_step: u16,

    /// Per-actor field-VM channels: one spawned context per MAN partition-1
    /// placement record, mirroring the retail per-record spawn
    /// (`FUN_8003A1E4`). Spawned alongside a cutscene timeline
    /// ([`Self::install_cutscene_timeline_record`]) so the timeline's
    /// cross-context pokes (flag writes, animate cues, moves) land on real
    /// per-actor contexts - the opening prologue's vignette mechanism.
    /// Stepped run-until-yield per frame by [`Self::step_field_channels`].
    pub field_channels: Vec<crate::field_channels::FieldChannel>,

    /// The MAN payload the channels' bytecode slices from (each channel's
    /// buffer base is its `record_offset` into this).
    pub field_channels_man: Option<std::sync::Arc<Vec<u8>>>,

    /// Placement index of the channel context currently executing (its own
    /// slice in [`Self::step_field_channels`], or the target of a
    /// cross-context poke from the cutscene timeline), so field-VM host hooks
    /// (animate, move) can attribute the side-effect to that placement's NPC.
    /// `None` outside a channel-targeted step.
    pub executing_channel: Option<u8>,

    /// Animation cues raised by channel scripts (op `0x4B` ANIMATE):
    /// `placement_index -> (count, base_id, keyframe bytes)`. The windowed
    /// host drains these each frame and re-targets the NPC's clip player.
    pub field_npc_anim_cues: std::collections::HashMap<u8, (u8, u8, Vec<u8>)>,

    /// Set when the `town01` opening cutscene timeline is installed via the
    /// new-game prologue hand-off. While set, the timeline's first op-`0x49`
    /// STATE_RESUME (the pinned name-entry handoff at P2[3] body `0x02c6`) opens
    /// the name-entry overlay instead of parking generically. One-shot for the
    /// opening; a normal `town01` visit never sets it. See
    /// [`Self::install_town01_opening_timeline`].
    pub prologue_naming_pending: bool,

    /// Set once the timeline's op-`0x49` has opened the name-entry overlay, so
    /// the op suspends (Armed) until the player commits a name, then resumes
    /// (Done) - and never re-opens it on the record's later STATE_RESUMEs.
    pub prologue_naming_armed: bool,

    /// Set by [`Self::take_prologue_handoff`] when it hands off to `town01`, so
    /// the next `town01` field entry installs the opening cutscene timeline
    /// (establishing shot + Vahn walk-out + name-entry handoff). Cleared when
    /// the entry consumes it, so only the prologue path runs the opening.
    pub entering_town01_opening: bool,

    /// The active opening-cutscene static title card (narration `0x89`
    /// blocks): pages shown simultaneously, centered mid-screen, until a
    /// blank card block clears it (the `map01` fly-in's "twilight of
    /// humanity" card). Rendered by the host; independent of the crawl
    /// roller [`Self::cutscene_narration`].
    pub cutscene_card: Option<Vec<String>>,

    /// The `opdeene` "It was the Seru." caption, decoded to RGBA at scene
    /// entry ([`crate::cutscene_caption::decode_opdeene_caption`]). `Some`
    /// only while `opdeene` is loaded; the host uploads it once as a sprite
    /// atlas and blits it, faded by [`Self::cutscene_caption_alpha`]. Unlike
    /// the crawl / card this is a pre-rendered image, not font text - retail
    /// draws it as a scene textured quad, so the engine blits the scene
    /// texture rather than rendering a string. See [`crate::cutscene_caption`].
    pub cutscene_caption: Option<crate::cutscene_caption::CaptionImage>,

    /// Fade level (0..=1) of [`Self::cutscene_caption`], ramped each
    /// [`Self::tick`]. Target-visible in the gap after the first narration
    /// crawl block scrolls out and before the second opens (retail shows the
    /// caption once, between `opdeene`'s two crawls).
    pub cutscene_caption_alpha: f32,

    /// Frames [`Self::cutscene_caption`] has been fully faded in. Used to bound
    /// the caption to a retail-like ~2 s beat and fade it back out, since the
    /// engine's inter-crawl timeline gap currently runs much longer than
    /// retail's - so the caption reads as a deliberate pause, not a freeze,
    /// even when the second crawl block is still frames away. Reset on scene
    /// entry; never re-shows once the hold elapses (the gap continues hidden).
    pub cutscene_caption_shown_frames: u32,

    /// Pending field-VM op-`0x44` SPAWN_RECORD requests: the GLOBAL record
    /// indices whose partition-2 records should spawn as new contexts.
    /// Recorded by the host hook (the VM borrow precludes resolving the MAN
    /// there); drained FIFO by `SceneHost::tick`, which re-bases each into
    /// partition 2 (`global - N0 - N1`, retail `FUN_8003BDE0`) and installs
    /// the record - as the modal cutscene timeline during the opening chain,
    /// as a concurrent [`Self::helper_contexts`] entry otherwise - when its
    /// C1/C2 story-flag gates pass. A queue (bounded by
    /// [`SPAWNED_CONTEXT_SLOTS`]) so a second spawn issued while another
    /// record executes is not dropped.
    pub pending_record_spawns: Vec<u8>,

    /// `true` while the New-Game opening cutscene chain is playing (from the
    /// `opdeene` entry through its `opstati` / `opurud` / world-map fly-in
    /// legs, until `town01` is entered). While set, a confirm press with the
    /// hand-off bit armed skips the WHOLE remaining opening to `town01` -
    /// retail's `FUN_801D1344` packet is a skip available any time after
    /// `opdeene` arms `GFLAG 26`, not a post-narration gate. Set when the
    /// prologue cutscene scene is entered; cleared by the skip or by the
    /// `town01` opening entry.
    pub opening_chain_active: bool,
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
            field_ctx: FieldCtx::default(),
            field_bytecode: Vec::new(),
            field_pc: 0,
            move_bytecode: vec![Vec::new(); MAX_ACTORS],
            move_buffer_root: Vec::new(),
            move2_buffer_root: Vec::new(),
            move_buffer_alt_root: Vec::new(),
            last_tick_events: Vec::new(),
            move_predicate: 0,
            move_counter: 0,
            move_slot_table: [[0u8; 8]; 16],
            move_axis_threshold: 0,
            move_ramp_ratio: 0,
            map_origin_xz: (0, 0),
            player_actor_slot: None,
            field_collision_grid: Vec::new(),
            field_map_region_block: Vec::new(),
            field_zone_table: Vec::new(),
            field_region_attributes: crate::field_regions::RegionAttributes::DEFAULT_FILL,
            field_zone_record: None,
            field_floor_height_lut: [0i16; 16],
            field_object_cells: Vec::new(),
            field_elevation_overrides: Vec::new(),
            follow_terrain_height: false,
            field_player_anim: None,
            leading_edge_wall_probes: false,
            solid_field_npcs: false,
            field_camera_azimuth: 0,
            party_actor_slots: Vec::new(),
            pending_fade: None,
            move_dat_8007b9d8: 0,
            scratchpad_targets: [0; 16],
            system_flags: Vec::new(),
            extra_flags: 0,
            screen_mode: 0,
            story_flags: 0,
            story_flag_bits: Vec::new(),
            rng_state: 0x1234_5678,
            active_summon: None,
            pending_summon_spawn: None,
            pending_move_fx_spawn: None,
            sin_lut: Vec::new(),
            cos_lut: Vec::new(),
            spell_costs: Default::default(),
            capture_spells: Default::default(),
            character_ability_bits: [0; 8],
            range_table: Default::default(),
            battle_attack: [0; 8],
            battle_magic: [0; 8],
            battle_defense: [0; 8],
            battle_defense_split: [None; 8],
            battle_speed: [0; 8],
            battle_accuracy: [0; 8],
            battle_evasion: [0; 8],
            monster_strike_budget: 1,
            prev_action_cleared: true,
            sound_bank_ready: true,
            party_count: 3,
            active_party: Vec::new(),
            battle_end: None,
            screen_fade: None,
            color_fade: None,
            roster: legaia_save::Party::zeroed(0),
            pending_scene_transition: None,
            pending_named_scene_transition: None,
            pending_fmv_trigger: None,
            pending_scripted_encounter: None,
            scripted_encounter_armed: false,
            scripted_formation_pending: false,
            active_fmv: None,
            cutscene_return_mode: None,
            pending_field_events: Vec::new(),
            pending_actor_spawns: Vec::new(),
            pending_battle_events: Vec::new(),
            battle_hit_fx: Vec::new(),
            battle_sfx_cues: Vec::new(),
            current_bgm: None,
            battle_bgm: None,
            field_bgm_resume: None,
            battle_bgm_active: false,
            current_dialog: None,
            three_actor_talk: None,
            last_field_interact: None,
            field_npc_dialog: std::collections::HashMap::new(),
            field_npc_dialog_prologue: std::collections::HashMap::new(),
            active_inline_prologue: None,
            field_npc_positions: std::collections::HashMap::new(),
            field_npc_headings: std::collections::HashMap::new(),
            field_prop_colliders: Vec::new(),
            resolved_cold_spawn: None,
            field_prop_bank: Default::default(),
            pending_prop_touch: None,
            field_npc_routes: std::collections::BTreeMap::new(),
            field_npc_glide_speeds: std::collections::BTreeMap::new(),
            field_npc_default_moves: std::collections::BTreeMap::new(),
            field_npc_motions: std::collections::BTreeMap::new(),
            animate_field_npcs: false,
            field_walk_touch: std::collections::BTreeMap::new(),
            field_walk_touch_records: std::collections::BTreeMap::new(),
            active_walk_touch: None,
            last_move_dir_bits: 0,
            stepping_inline_npc: None,
            active_inline_slot: None,
            actor_motions: std::collections::BTreeMap::new(),
            dialog_input_consumed: false,
            party_leader_slot: None,
            money: 0,
            inventory: std::collections::HashMap::new(),
            camera_state: CameraState::default(),
            frame: 0,
            input: input::InputState::default(),
            move_outcomes: Vec::new(),
            tactical_arts: TacticalArtsTracker::new(),
            current_art_banner: None,
            level_up_tracker: LevelUpTracker::new(),
            current_level_up_banner: None,
            current_capture_banner: None,
            world_map_ctrl: None,
            tile_board: None,
            tile_board_target: None,
            tile_board_armed: false,
            tile_board_header: None,
            tile_actor_slots: [None; crate::tile_board::TILE_ACTOR_TABLE_LEN],
            tile_board_draw_list: Vec::new(),
            screen_fx: Default::default(),
            screen_fx_frame: Default::default(),
            register_ramps: Vec::new(),
            dance: None,
            dance_return_mode: SceneMode::Field,
            dance_last_judge: None,
            fishing: None,
            fishing_return_mode: SceneMode::Field,
            fishing_points: 0,
            fishing_prizes_purchased: 0,
            fishing_exchange: None,
            slot_machine: None,
            slot_return_mode: SceneMode::Field,
            baka_fighter: None,
            baka_return_mode: SceneMode::Field,
            muscle_dome: None,
            muscle_return_mode: SceneMode::Field,
            casino_coins: 0,
            minigame_scene_backup: None,
            minigame_winnings: 0,
            status_effects: vm::status_effects::StatusEffectTracker::new(),
            ap_gauges: [crate::ap_gauge::ApGauge::default(); 3],
            battle_guarding: [false; 3],
            fury_boost: [None; 3],
            item_catalog: crate::items::ItemCatalog::default(),
            item_effects: None,
            spell_catalog: crate::spells::SpellCatalog::default(),
            art_records: std::collections::HashMap::new(),
            battle_buffs: Vec::new(),
            battle_captures: Vec::new(),
            shiny_chance_pct: Self::DEFAULT_SHINY_CHANCE_PCT,
            shiny_enemy_slots: std::collections::HashSet::new(),
            shiny_captures: Vec::new(),
            magic_xp_thresholds: None,
            seru_trade_config: None,
            magic_level_ups: Vec::new(),
            battle_escaped: false,
            battle_no_escape: false,
            character_max_mp: Vec::new(),
            encounter: None,
            per_char_ext: Vec::new(),
            saved_chains: Vec::new(),
            seru_log: crate::seru_learning::SeruCaptureLog::new(),
            seru_registry: crate::seru_learning::SeruRegistry::new(),
            last_capture_outcomes: Vec::new(),
            play_time_seconds: 0,
            formation_table: crate::monster_catalog::FormationTable::new(),
            monster_catalog: crate::monster_catalog::MonsterCatalog::new(),
            move_power: None,
            move_power_overlay: None,
            active_move_fx: None,
            active_move_fx_trail_texpage: None,
            field_stagers: Vec::new(),
            field_stager_bytes: Vec::new(),
            active_field_fx: Vec::new(),
            // Field/town baseline; scene entry re-pins (`mapNN` -> 3).
            frame_step: 2,
            clut_vsync_accum: 0,
            clut_pending_game_ticks: 0,
            clut_fx: Vec::new(),
            pending_move_fx_cue: None,
            element_affinity: None,
            item_shop_data: None,
            scene_shops: Vec::new(),
            pending_field_shop: None,
            field_shop_armed: false,
            field_shop_open: false,
            equipment_table: crate::battle_stats::EquipmentTable::new(),
            accessory_passives: Default::default(),
            party_ability_mask: [0; crate::accessory_passives::ABILITY_WORDS],
            monster_ai_state: crate::monster_ai::MonsterAiState::new(),
            active_scene_label: String::new(),
            vdf_buffer: None,
            global_tmd_pool: Vec::new(),
            live_gameplay_loop: false,
            smarter_monster_targeting: false,
            use_vm_dialogue: false,
            use_damage_finish: false,
            battle_player_driven: false,
            battle_command: None,
            battle_item_menu: None,
            battle_spell_menu: None,
            battle_arts_menu: None,
            active_formation: None,
            field_boss_stagers: std::collections::HashMap::new(),
            last_battle_rewards: None,
            game_over: false,
            field_return: None,
            field_last_tile: None,
            world_map_entities: Vec::new(),
            world_map_entity_configs: Vec::new(),
            world_map_entity_positions: Vec::new(),
            world_map_encounter: WorldMapEncounterState::default(),
            world_map_player_walking: false,
            pending_world_map_encounter: None,
            world_map_region_tracker: None,
            world_map_last_tile: None,
            field_region_tracker: None,
            world_map_player_speed: Self::WORLD_MAP_PLAYER_SPEED,
            battle_return_mode: SceneMode::Field,
            field_carriers: Vec::new(),
            field_carrier_configs: Vec::new(),
            pending_field_carrier_battle: None,
            field_carrier_slots: std::collections::HashMap::new(),
            pending_carrier_engage: None,
            carrier_menu: None,
            party_names: Vec::new(),
            name_entry: None,
            cutscene_narration: None,
            cutscene_narration_seq: 0,
            cutscene_timeline: None,
            helper_contexts: Vec::new(),
            inline_dialogue: None,
            in_cutscene_timeline: false,
            field_frame_accum: 0,
            field_frame_step: 0,
            field_channels: Vec::new(),
            field_channels_man: None,
            executing_channel: None,
            field_npc_anim_cues: std::collections::HashMap::new(),
            prologue_naming_pending: false,
            prologue_naming_armed: false,
            entering_town01_opening: false,
            cutscene_card: None,
            cutscene_caption: None,
            cutscene_caption_alpha: 0.0,
            cutscene_caption_shown_frames: 0,
            pending_record_spawns: Vec::new(),
            opening_chain_active: false,
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
    // REF: FUN_80025B64
    // REF: FUN_801D6704
    // REF: FUN_801DD35C
    // REF: FUN_80034A6C
    // REF: FUN_800560B4
    // REF: FUN_8004F0E8
    pub fn begin_new_game(&mut self) {
        self.story_flags = 0;
        self.story_flag_bits.clear();
        self.money = NEW_GAME_STARTING_GOLD;
        self.inventory.clear();
        self.pending_scene_transition = None;
        self.pending_named_scene_transition = None;
        self.pending_fmv_trigger = None;
        self.pending_scripted_encounter = None;
        self.scripted_encounter_armed = false;
        self.scripted_formation_pending = false;
        self.encounter = None;
        self.battle_end = None;
        self.game_over = false;
        self.play_time_seconds = 0;
        self.cutscene_timeline = None;
        self.helper_contexts.clear();
        self.cutscene_narration = None;
        self.cutscene_card = None;
        self.prologue_naming_pending = false;
        self.prologue_naming_armed = false;
        self.entering_town01_opening = false;
        self.pending_record_spawns.clear();
        self.opening_chain_active = false;
        self.mode = SceneMode::Field;
    }
}
