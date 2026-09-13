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
    /// (unused by the wall check). Loaded at field entry from the per-scene
    /// `DATA\FIELD\<scene>.MAP` `+0x4000` region - that disc blob is the
    /// **base** grid, and the live retail grid byte-matches it with zero
    /// diffs. The field-VM `0x4C` outer-nibble-7 op then applies
    /// story-conditional wall *deltas* on top as the scene prescript runs;
    /// it does not author the grid from scratch.
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
    /// Which object-grid cell bit this scene records its **standable floor**
    /// with - [`legaia_asset::field_objects::CELL_WALK_VISIBLE`] (`0x1000`) for
    /// the scenes that set it, [`legaia_asset::field_objects::CELL_VISIBLE`]
    /// (`0x2000`) for the ones that never do.
    ///
    /// Both bits are draw gates on the same `u16`; `0x1000` is the walk view's
    /// and `0x2000` the overhead one's. Most scenes set `0x1000` on every tile
    /// the party may stand on, but eighteen of the disc's field scenes (the
    /// Sol / Karisto `kor*` band, the Drake-castle `jouin*` interiors,
    /// `tunnela`, `jagaroom`, `noaru`, `juui2`, `dream`, `edkorout`, `edlast`)
    /// carry object grids with `0x2000` cells and **not one** `0x1000` cell.
    /// Gating the standable test on `0x1000` alone therefore reads those scenes
    /// as having no floor at all, which makes
    /// [`World::resolve_cold_field_spawn`] inert there: every component is
    /// empty, so the retail seat is returned unresolved and `kor5` seats the
    /// player inside a wall.
    ///
    /// Retail never consults this grid to decide where the player may stand -
    /// standing is the collision grid's wall bits
    /// ([`World::field_tile_is_wall`]) - so the bit is only ever the port's
    /// extra "is this inside the authored area" filter, and the filter has to
    /// use whichever bit the scene actually authored.
    ///
    /// Set by [`World::load_field_object_cells`]; `CELL_WALK_VISIBLE` before
    /// any scene supplies a grid.
    pub field_floor_cell_bit: u16,
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
    /// Field NPC state: positions, headings, routes, motions, ambient anims, dialog bindings and the solid / animate toggles.
    pub npcs: FieldNpcState,
    /// Accumulated walked amount the field walk-regen tick drains (retail
    /// `_DAT_801F2274`). [`World::step_field_locomotion`] bumps it on every
    /// retail frame whose locomotion step actually committed;
    /// [`World::tick_field_walk_regen`] consumes
    /// [`crate::walk_regen::WALK_REGEN_STEP_COST`] per regen tick. The drain
    /// is retail-pinned, the fill unit is the engine's - see
    /// [`World::tick_field_walk_regen`].
    pub walk_regen_steps: i32,
    /// The walk-regen tick's secondary countdown (retail `_DAT_8007B600`),
    /// which arms a dialog-window callback on its zero edge. Nothing in the
    /// engine arms it, so it stays `0` and the edge never fires - the
    /// window descriptor it schedules (`_DAT_8007B450`) has no engine
    /// analogue.
    pub walk_regen_window: i32,
    /// Camera azimuth (PSX 12-bit angle, `4096` = full turn) used to make
    /// d-pad locomotion camera-relative. Retail equivalent: the view
    /// direction `func_0x800467e8` remaps the held pad against. `0` maps
    /// "screen up" to world `+Z` (the default follow camera looking down
    /// `+Z`). Engines that orbit the camera write the current azimuth here
    /// each frame; the locomotion remap quantises it to the nearest 90°.
    pub field_camera_azimuth: u16,
    /// Opt-in precise-movement mode for pad locomotion. When set,
    /// [`World::step_field_locomotion`] decodes the held direction
    /// **continuously** instead of through retail's 4/8-way quantisation:
    /// the camera azimuth is applied at full angular resolution (not
    /// snapped to the nearest 90°), key diagonals walk true 45° vectors at
    /// full speed, and an analog stick ([`crate::input::InputState::lstick`])
    /// passes its angle through untouched. Off by default - the default
    /// path stays bit-identical to the retail-faithful quantised remap
    /// (replays / oracles are unaffected unless a host opts in).
    pub precise_movement: bool,
    /// Field Move option: `false` = Walk, `true` = Run (retail config word
    /// `0x800846CC`, the pause menu's "Field Move" row - see
    /// [`crate::options::FieldMoveOpt`]). Hosts mirror their
    /// [`crate::options::OptionsState`] onto this the way they mirror
    /// [`Self::precise_movement`].
    ///
    /// This is the *default*, not the state: the run button INVERTS it, so
    /// with Run selected the button walks. See
    /// [`World::field_run_active`].
    pub field_move_run_default: bool,
    /// `true` while the field run button is held this frame. Derived from the
    /// pad word inside [`World::set_pad`], so no host wires it separately.
    ///
    /// Retail reads it as `pad_held & mask`, where the held-pad word is
    /// `_DAT_8007B850` and the mask is the config word `0x800846DC` - `0x48`
    /// = **Cross | R1** in the packed pad layout, seeded once by the new-game
    /// data-init `FUN_80034A6C` at `0x80034AB8` and written by nothing else
    /// on the disc, so retail's run button is not configurable. The port's
    /// mask is [`Self::field_run_button_mask`], and it defaults to those two
    /// buttons. The XOR structure around the flag, in
    /// [`World::field_run_active`], is pinned as well.
    pub field_run_button_held: bool,
    /// Which pad buttons count as "the run button" for
    /// [`Self::field_run_button_held`].
    ///
    /// Defaults to
    /// [`FIELD_RUN_BUTTON_MASK_DEFAULT`](crate::world::config::FIELD_RUN_BUTTON_MASK_DEFAULT)
    /// = retail's `Cross | R1` plus **Square**, the port's historical
    /// binding, kept as an alternate. Assign
    /// [`FIELD_RUN_BUTTON_MASK_RETAIL`](crate::world::config::FIELD_RUN_BUTTON_MASK_RETAIL)
    /// for the retail button set exactly. Which *key* produces each of those
    /// buttons is the host's binding table
    /// (`legaia-engine config set --binding`), not this mask.
    pub field_run_button_mask: u16,
    /// Forced-slow gate: retail's `_DAT_8007B6A8` arm of the base-step
    /// selector. Non-zero there selects base step
    /// [`crate::world::config::FIELD_BASE_STEP_FORCED_SLOW`] and **skips the
    /// run check entirely** - a forced walk cannot be run out of.
    ///
    /// NOT WIRED: no host drives this yet. `_DAT_8007B6A8` is the same word
    /// the action-button gate and the per-scene save-allow test read
    /// (`docs/subsystems/field-locomotion.md`), and the port has no
    /// equivalent of its writer, so the constant and the arm are ported and
    /// the flag stays `false`.
    pub field_forced_slow: bool,
    /// Scene-entry VDF pulse **enhancement** gate
    /// ([`World::install_entry_vdf_pulse`]). On by default; clearing it
    /// keeps every never-retail-armed morph pack (jou's flesh ground)
    /// static at plain entry, exactly as retail draws it. Retail-armed
    /// scenes are unaffected either way - the installer stands aside for
    /// them regardless.
    pub entry_pulse_enabled: bool,
    /// Sub-step remainder carried between precise-movement frames, in world
    /// units per axis (|carry| < one collision step). Lets shallow movement
    /// angles accumulate distance across frames instead of rounding to
    /// zero. Only touched while [`Self::precise_movement`] is active with a
    /// direction held; reset when input releases.
    pub precise_move_carry: (f32, f32),
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

    /// Battle-action helper tables.
    ///
    /// There is deliberately **no** spell-cost or capture-spell table here.
    /// `BattleHostImpl` answers both questions from the models already loaded
    /// at boot - [`World::spell_catalog`] for the price, and the disc spell
    /// table's class byte (`World::spell_table_class`, off
    /// [`World::menu_text`]) for the capture route - so the battle-action
    /// state machine and the live cast path cannot price or classify the same
    /// spell differently. A pair of `HashMap`s here that nothing filled is
    /// exactly how they used to.
    /// There is no melee-range table here either, for the same reason and by
    /// the same evidence: retail computes reach (`FUN_8004E2F0`) from the
    /// attacker's per-character base, both actors' size classes and their live
    /// positions. [`World::battle_range_metric`] answers from those models.
    pub character_ability_bits: [u32; 8],
    /// Live battle session state: per-seat stat arrays, command / submenu sessions, flow + round state, tutorial, intro transition, escape timer, buffs, hit / effect queues and the end-of-battle latches.
    pub battle: BattleState,

    /// Screen-shake amplitude - retail `_DAT_8007B630`.
    ///
    /// Written by exactly one thing in retail: the field-VM opcode
    /// `0x4C` outer-nibble `8` sub-`4` (`[4C, 0x84, amplitude]`), ported at
    /// [`legaia_engine_vm::field::FieldHost::op4c_n8_sub4_set_b630`]. It is
    /// the only input to the LCG camera jitter `FUN_801D9D30`
    /// ([`legaia_engine_vm::battle_camera::apply_shake`]): `0` is the resting
    /// state and `1..=0x15` widens the jitter window.
    ///
    /// REF: FUN_801D9D30
    pub camera_shake_amplitude: u8,

    /// Audio-side state: BGM selection and resume, the SFX cue / delay slots, sound-bank handshakes and the battle SFX / XA / shout cue queues.
    pub audio: AudioState,

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

    /// Disc-parsed static tables installed at boot / scene load (items, spells, arts, monsters, formations, move power, equipment, thresholds, CDNAME map).
    pub tables: DiscTables,

    /// Active full-screen fade, staged by the battle SM's escape teardown
    /// (retail state `0x66` spawns the `DAT_801C9070` black→white ramp via
    /// the fade-primitive spawner `FUN_80024E80`). Stepped once per
    /// [`World::tick`]; dropped when the ramp completes. Hosts draw an
    /// overlay from [`crate::fade::FadeState::rgb`] while this is `Some`.
    pub screen_fade: Option<crate::fade::FadeState>,

    /// Effect-layer global colour (op `0x34` sub-0, `FUN_801E1FB0`; neutral
    /// operand `0xFF`, stored normalized). The opening timeline ramps it in
    /// the crawl gaps (`34 05 00 00 00 D2 00` = to black over 210 frames,
    /// `34 01 FF FF FF 00 00` = instant neutral). Stepped once per
    /// [`World::tick`]; dropped once it lands on the neutral identity.
    /// **Not a screen fade**: the retail cold-boot capture holds the lit
    /// villager tableau across the span where the timeline's black ramp
    /// would blank a full-screen fade, so this value feeds the effect layer
    /// (the creation-glow planes; consumer still an open thread) and stays
    /// out of [`World::scene_screen_tint`]. Scene-local: reset on scene
    /// entry. Distinct from [`Self::screen_fade`] (the battle escape ramp).
    pub effect_tint: Option<crate::fade::SceneTintRamp>,

    /// Global multiply screen tint (op `0x4C 0x12` → `DAT_8007BCB8/B9/BA`,
    /// neutral operand `0x80`, stored normalized; ramp via `FUN_8003C5F0`).
    /// The scene-entry fade-in from black - every field scene `P1[0]`'s
    /// `0x52F` arrival arm: `4C 12 00 00 00 00 00` (instant black) then
    /// `4C 12 80 80 80 44 00` (ramp to neutral over 68 frames) - lives here.
    /// Persists across scene changes (retail's cross-scene fade continuity:
    /// a departure fade-to-black carries into the next scene's fade-in).
    /// Stepped once per [`World::tick`]; dropped once neutral.
    pub screen_tint: Option<crate::fade::SceneTintRamp>,

    /// `_DAT_8007B868` - the dev/retail branch discriminator. **Retail is
    /// zero**; a non-zero value is the dev build's "not an ordinary field
    /// frame" state. Read by [`World::man_load_actor_reset`] through
    /// [`crate::field_submode::scene_actor_initial_state`], which forces the
    /// spawned scene actor's state word to `1` when it is set. Mirrors of the
    /// same word live on [`crate::cd_dma`] and [`crate::overlay_loader`]; this
    /// is the field-side copy.
    pub field_mode_flags: u32,

    /// The op-`0x49` submode context block (`0x801F2734..`), in the order
    /// [`crate::field_submode::SUBMODE_CONTEXT_SEEDS`] lists its ten words -
    /// that const carries the offsets. Reseeded on every MAN load by
    /// [`World::man_load_actor_reset`]; word `0` is the submode state
    /// ([`crate::field_submode::SUBMODE_STATE_OPEN`] once opened).
    pub submode_context: [u32; 10],

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

    /// Host-latched "switch character" request for the active three-actor
    /// talk - the engine input standing in for retail's pad-derived word
    /// `_DAT_8007B874` bit `0x80` (the request route of `FUN_801D27E0`'s
    /// state-0 arm gate). Hosts latch it from their pad handler via
    /// [`World::request_talk_leader_switch`]; the controller poll
    /// ([`World::tick_three_actor_talk`]) consumes it on its next state-0
    /// frame and drops it when no talk is live.
    pub talk_switch_requested: bool,

    /// Last `field_interact` request. Cleared by the engine when handled
    /// (set to `None`).
    pub last_field_interact: Option<(u8, u8)>,

    /// The interaction-prologue record for the dialogue [`Self::trigger_field_interact`]
    /// most recently opened (taken by [`Self::drive_inline_dialogue`] when it
    /// starts the runner). `None` when the opened NPC has no prologue record.
    pub active_inline_prologue: Option<crate::man_field_scripts::InlineDialogPrologue>,

    /// Last frame's field position for every actor the motion detector
    /// tracks - the player (from its [`crate::vm::ActorMoveState`]) and every
    /// entry of [`Self::field_npc_positions`]. Rewritten each field tick by
    /// [`Self::detect_field_actor_motion`], which is the only reader.
    ///
    /// Cleared on scene entry alongside [`Self::field_npc_positions`]: a
    /// stale entry across a scene change would read the warp itself as one
    /// enormous step and start every actor walking on the landing frame.
    ///
    /// Public only because `World` is built with functional-update syntax in
    /// integration tests, which requires every field to be visible; treat it
    /// as internal to the detector.
    pub field_motion_prev: std::collections::HashMap<u8, (i16, i16)>,

    /// Placement slots whose field position CHANGED during the frame just
    /// ticked - the source-agnostic "this actor is moving" signal.
    ///
    /// Recomputed every field tick by [`Self::detect_field_actor_motion`] by
    /// diffing live positions against [`Self::field_motion_prev`], so it is
    /// true for a walk driven by the pad, by a nav step, by a motion-VM
    /// patrol leg, by a cutscene `MoveTo`, or by anything else that commits a
    /// position - the animation layer does not have to know which. That is
    /// the point: selecting the walk clip off the *mover* rather than off the
    /// *motion* is what made script-driven actors glide.
    ///
    /// The player's own bit is folded straight into
    /// [`crate::field_anim::FieldPlayerAnim::moved_this_frame`] rather than
    /// left here for a host to read.
    pub field_actor_moving: std::collections::HashSet<u8>,

    /// `true` only while [`Self::pre_run_field_channel_prologues`] is
    /// executing the scene-entry spawn-prologue slices. The field-VM host
    /// reads it to give the prologue's `4C 51` NPC-run ops their load-time
    /// semantics (an initial SEAT written through the channel ctx) without
    /// touching the live free-roam / cutscene behaviour of the same op.
    pub field_entry_prerun: bool,

    /// Scene control block `+0x4A` (`_DAT_801C6EA4 + 0x4A`) - the camera
    /// vertical offset the current scene asks for, in the player actor's
    /// `+0x16` footing units. Written by the field VM's op `0x4C`
    /// outer-nibble-4 sub-9 (all three arms) and read once a frame by
    /// [`Self::tick_camera_offset_ease`].
    pub camera_scene_offset: i16,
    /// `_DAT_8007BCAC` - the smoothed camera vertical offset
    /// [`crate::camera_ease::ease_camera_offset`] walks toward
    /// `camera_scene_offset - player_footing`. Seeded to `0x3C` by retail's
    /// per-scene initialiser `FUN_801D6704`, which is why the engine seeds it
    /// there too. The op `0x4C` n4 sub-9 **delta** arm snaps it instead of
    /// letting the easing arrive.
    pub camera_offset_ease: i32,
    /// Previous tick's player `(world_y, world_z)`. Stands in for the
    /// `+0x1E`/`+0x20` slots retail's settle test compares `+0x16`/`+0x18`
    /// against; the question the comparison asks is whether the actor has
    /// stopped moving in Y and Z, and this answers it without asserting what
    /// retail keeps in those two slots. `None` until the first tick.
    pub camera_ease_prev_yz: Option<(i16, i16)>,

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

    /// The last **committed** locomotion sub-step direction, one entry per
    /// axis at retail's fixed magnitude 8 - the engine's stand-in for the
    /// retail global pair `0x8007BDE0` (X) / `0x8007BDE4` (Z).
    ///
    /// Retail writes these inside the locomotion controller `FUN_801d01b0`
    /// (`0x801d0550` clears the pair on a no-input frame; `0x801d07bc` and
    /// its per-axis siblings record `+-8` alongside each committed 2-unit
    /// sub-step). The magnitude is a fixed probe scale, **not** the frame
    /// speed: the ledge-hop probe multiplies it by 4 and samples at 2x and
    /// 3x that, i.e. 64 and 96 world units ahead - one and one-and-a-half
    /// collision sub-cells.
    ///
    /// A wall-blocked axis records `0`, so a hop is only ever attempted
    /// along an axis the walk actually moved on.
    pub field_step_delta: (i16, i16),

    /// Run the retail vertical settle (`FUN_801d1ba0`'s rate-clamped glide
    /// toward the floor) instead of leaving the actor's Y alone.
    ///
    /// Default **off**, and deliberately separate from
    /// [`Self::follow_terrain_height`]: that flag *snaps* Y to the sampled
    /// floor in one frame, and the engine's flat-Y default (Y untouched when
    /// the snap is off) is an invariant the locomotion oracles pin. Retail
    /// does neither - it glides at `delta_scalar * 12` units per frame, so a
    /// tall drop takes several frames. Enabling this replaces "untouched"
    /// with the retail glide; it does not override the snap, which stays
    /// authoritative when set.
    ///
    /// The ledge-hop trigger is **not** gated on this - a hop is posted off
    /// the step delta whether or not the settle runs.
    pub field_vertical_settle: bool,

    /// The ledge hop [`Self::try_field_ledge_hop`] posted this frame, if any
    /// (retail hands the same triple to `FUN_801d2404`). `None` on every
    /// frame that did not start a hop.
    pub field_ledge_hop: Option<FieldLedgeHop>,

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

    /// Active level-up HUD banner. Set by [`World::apply_battle_xp`];
    /// `frames_remaining` is decremented by [`World::tick`] until it reaches
    /// zero, at which point the next entry of
    /// [`Self::pending_level_up_banners`] takes the slot. `None` when no
    /// banner is active. Engines render this as a dialog-font overlay after
    /// battle.
    pub current_level_up_banner: Option<LevelUpBanner>,

    /// Level-up banners waiting for the slot above, in the order they were
    /// earned.
    ///
    /// One fight can level several party members, and the banner is a single
    /// slot. Writing it per member inside the distribution loop meant each
    /// leveller overwrote the previous one in the same frame and the player
    /// saw exactly one banner - the last - for a battle that levelled three.
    /// Queueing is what makes "three members levelled" legible as three
    /// banners.
    pub pending_level_up_banners: std::collections::VecDeque<LevelUpBanner>,

    /// Active post-battle Seru-capture banner. Set by `World::resolve_captures`
    /// when a capture is accepted; advanced one frame per [`World::tick`] and
    /// cleared when its [`crate::seru_learning::SeruCaptureSession`] reaches
    /// `Done`. Engines render [`crate::seru_learning::SeruCaptureSession::current_banner`]
    /// as a dialog-font overlay after battle, the sibling of
    /// [`Self::current_level_up_banner`].
    pub current_capture_banner: Option<crate::seru_learning::SeruCaptureSession>,

    /// Overworld state: the world-map controller, its entity state machines and the encounter / region trackers.
    pub world_map: WorldMapState,

    /// Tile-board grid-mode state (the op-0x49 puzzle board, not town locomotion).
    pub board: TileBoardState,

    /// Screen-effect widget host (the PROT-0900 mask / sprite / panel /
    /// letterbox family), driven by the field-VM op `0x43` sub-ops
    /// `0x10`/`0x11`/`0x13`/`0x14`/`0x15` - the ending-scene widget
    /// path. See [`crate::screen_fx`].
    pub screen_fx: crate::screen_fx::ScreenFxHost,

    /// The current frame's widget draw list, refreshed by the Field /
    /// Cutscene tick while any widget is live ([`Self::tick_screen_fx`]).
    /// Renderers composite these 2D overlays above the scene.
    pub screen_fx_frame: crate::screen_fx::ScreenFxFrame,

    /// Live camera-register zone-ramp records spawned by the field-VM op
    /// `0x43` sub-3..6 (retail `FUN_8003C6A4` actors on the effect list).
    /// [`World::tick_register_ramps`] runs each one's `FUN_80037018` handler
    /// against the player's position every field frame. See
    /// [`crate::register_ramp`].
    pub register_ramps: Vec<crate::register_ramp::RegisterRamp>,

    /// The four field camera-configuration registers
    /// (`0x8007B60C`/`B610`/`B614`/`B618`) the ramps above write. Seeded to
    /// [`crate::camera::CAMERA_ZONE_DEFAULTS`] on scene entry; consumed by
    /// [`crate::camera::Camera::tick`].
    pub camera_registers: crate::register_ramp::CameraRegisterFile,

    /// The live cinematic bar emitter (field-VM op `0x43` sub-`0xC`, retail
    /// template `0x801F2858` / tick `FUN_801DD784`). One at a time, because
    /// its spawner is the one op that allocates it and its envelope retires
    /// itself; [`World::tick_field_timer_actors`] steps it and
    /// [`Self::cinematic_bar`] is what the two hosts draw from.
    pub cinematic_bars: Option<legaia_engine_vm::field_actor_timers::ShutterBars>,

    /// This frame's bar height in scanlines, republished every tick so a
    /// renderer reads a value rather than re-stepping the envelope.
    pub cinematic_bar: i16,

    /// Live eased-move records (field-VM op `0x43` sub-9 with a non-zero
    /// tick count, retail template `0x801F2840` / tick `FUN_801DD4C4`), each
    /// paired with the actor whose position triple it writes.
    pub eased_moves: Vec<crate::world::FieldEasedMove>,

    /// The player actor's `+0x8E` **inverted-Y latch** - retail's third
    /// height arm, and the one thing an eased move over the player publishes
    /// besides the position triple.
    ///
    /// `FUN_801DD4C4` stores `-Y` here on every frame of a move whose target
    /// carries [`EASE_TARGET_INVERT_Y`](legaia_engine_vm::field_actor_timers::EASE_TARGET_INVERT_Y)
    /// (`0x801DD6A4..0x801DD6B8`: `lw v0,0x10(a2)` / `lui v1,0x2000` / `and` /
    /// `subu v0,zero,a1` / `sh v0,0x8e(a2)`). The consumer is the field-actor
    /// driver `FUN_8003BC08`, whose height arm tests the same flag **first**
    /// (`0x8003BC4C..0x8003BC64`) and writes `-(+0x8E)` into the actor's
    /// `+0x16` in place of the ground height its other two arms would sample -
    /// so the latch is not decoration, it is what stops the floor controllers
    /// dragging an airborne scripted move back down to the terrain.
    ///
    /// The engine's two height controllers are exactly those other two arms
    /// ([`World::field_vertical_settle`] is the glide, and
    /// [`World::follow_terrain_height`] the snap), so both read this and step
    /// aside while it is armed. `None` is the no-mirror case, which is every
    /// ordinary frame.
    ///
    /// Player-only, and that is a real limit rather than a simplification:
    /// retail's `+0x8E` is per-actor, but a scene NPC in this engine is a
    /// placement slot with an `(x, z)` pair and a scene-build Y from
    /// [`legaia_asset::field_objects::Placement::world_y`] - it has no
    /// per-frame height controller for a mirror to override.
    pub field_eased_mirror_y: Option<i16>,

    /// Live floor-height-ladder oscillators (field-VM op `0x4C` nibble-9
    /// sub-`0..2`, retail template `0x801F27EC` / tick `FUN_801DA930`). Each
    /// drives one rung of [`Self::field_floor_height_lut`].
    pub floor_tier_bobs: Vec<legaia_engine_vm::field_actor_timers::FloorTierBob>,

    /// Minigame sessions (dance, fishing, slot machine, Baka Fighter, Muscle Dome) plus the casino coin / point-card wallet.
    pub minigames: MinigameState,

    /// The op-`0x49` sub-screen the submode driver actor is running, if any.
    /// See [`crate::field_submode_screen`]; ticked by
    /// [`World::tick_handler_actors`].
    pub submode_screen: crate::field_submode_screen::SubmodeScreen,

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

    /// Seru capture + magic-learning state: the capture log and registry, this battle's captures, shiny rolls and magic level-ups.
    pub seru: SeruState,

    /// Total game time in wall-clock seconds since the world was
    /// instantiated or loaded. Engines tick this independently of
    /// `frame` (which can pause-skip during dialogs / cutscenes).
    /// Persisted in [`legaia_save::SaveExtV2::play_time_seconds`].
    pub play_time_seconds: u32,

    /// Per-scene **save permission** - retail's `_DAT_8007B6A8`.
    ///
    /// Seeded at scene load from the scene MAN header's `[0x01] & 1`
    /// ([`legaia_asset::man_section::ManHeader::low_flag`]) by
    /// [`World::install_scene_save_permission`]; a scene with no MAN, and a
    /// world that has not loaded one, reads `false` - the same state retail's
    /// own init leaves the byte in. Read by the pause menu, where a cleared
    /// flag greys the Save row and buzzes its confirm
    /// ([`crate::pause_screens::root_menu_confirm_route`]).
    pub scene_save_allowed: bool,

    /// Town shop + prize-exchange session state (the gold shop and the casino / fishing prize counter).
    pub shops: ShopState,

    /// Pause-menu runtime state: disc-parsed text / widget tables and the pending warp / escape requests.
    pub menu: MenuState,

    /// Party-global 4×u32 ability mask - the engine mirror of retail
    /// `DAT_80074358..0x80074368` (every member's `+0xF4` bitfield OR'd
    /// together each rebuild). Bit-tested via [`World::party_has_ability`]
    /// (the `FUN_800431D0` port); rebuilt by
    /// [`World::refresh_party_ability_bits`].
    pub party_ability_mask: [u32; crate::accessory_passives::ABILITY_WORDS],

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

    /// Summon / cast-module / move-FX scene-graph state: the active summon scene, the cast stager and phase bytes, and the move-effect spawns and trails.
    pub casting: CastFxState,

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
    /// Field ambient animation state: the CLUT-walk / CLUT-cell cyclers, VDF pulse, script VRAM moves and their vsync accumulators.
    pub ambient: AmbientFxState,
    /// Photosensitivity guard over the ambient CLUT-cell cyclers (see
    /// [`crate::options::OptionsState::reduce_flashing`]). When `true`
    /// (the default - a host that never plumbs options stays safe),
    /// [`World::step_ambient_fx`] slew-limits the **applied** luminance
    /// channels (`v_add`, `white`) toward each cell's simulated target
    /// instead of jumping, so full-swing per-tick strobes (koin3's dance
    /// floor) become sub-hazard-rate pulses. Hue / saturation sweeps pass
    /// through untouched. The move-VM state itself always advances
    /// retail-exact - this only shapes the VRAM presentation.
    pub reduce_flashing: bool,

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
    /// Retail `DAT_8007B9D8` - the per-mode **floor** under
    /// [`Self::frame_step`], installed by the mode/scene loader and never by
    /// the frame driver. `FUN_80016B6C` applies it as a minimum (`slt` plus a
    /// store taken only when the adaptive value is *below* it), so it raises
    /// the cadence and never caps it. Kept separate from `frame_step` because
    /// folding the two lets a single slow frame ratchet the floor upward
    /// permanently.
    ///
    /// REF: FUN_80016B6C, FUN_801D6704
    pub frame_step_floor: u8,
    /// The scene control block `_DAT_801C6EA4` (`0x64` bytes), re-allocated
    /// and reset on every scene load.
    ///
    /// REF: FUN_8003A024
    pub scene_control_block: crate::scus_leaf_kernels::SceneControlBlockReset,

    /// Set to request that the next per-frame mode handler skip its frame.
    ///
    /// Retail's frame-begin pass `FUN_8001698C` returns `1` when `gp+0x3D8`
    /// is set and neither `_DAT_8007B938` nor `gp+0x55C` carries bit `0x800`;
    /// its caller (the per-frame mode handler, [`crate::mode::per_frame_stage`])
    /// then abandons the frame after a pad poll and a `VSync(0)` - no
    /// mid-frame driver, no frame-end pass. Consumed (and cleared) by
    /// [`crate::mode::ModeDriver::tick`] via [`World::take_frame_begin_skip`].
    ///
    /// Defaults to `false`; a host that never sets it gets the pre-existing
    /// tick-every-frame behaviour - and that is also what **retail** does.
    /// The flag has no retail producer that a shipped disc can reach: a
    /// five-form sweep plus the `gp`-relative sweep over `SCUS_942.54` and
    /// every based overlay image finds exactly three sites touching
    /// `gp+0x3D8`, and two are clears - the mode-change edge's
    /// (`0x800161E8`, which [`crate::mode::ModeSeat`] performs) and a reset
    /// path's (`0x8001E100`). The one **setter** is
    /// `_DAT_8007B6F0 = ~_DAT_8007B6F0` at `0x80018850`, the R1+Start pause
    /// toggle in `FUN_8001822C`'s dev-hotkey tail, and that whole tail sits
    /// behind `_DAT_8007B98C != 0` (`beq` at `0x800185FC`), which is zero on
    /// retail. So this is a *debug pause* channel, and the port's own debug
    /// surface - not a missing engine wire - is what would set it.
    ///
    /// REF: FUN_8001698C
    /// REF: FUN_8001822C - the dev-hotkey tail that owns the only setter.
    pub frame_begin_skip: bool,
    /// Retail's frame-time history behind the adaptive cadence
    /// (`DAT_80084098[16]` + `0x1F800392`). Only advanced when a host calls
    /// [`World::resolve_frame_step`]; a host with no frame-time telemetry
    /// leaves it untouched and keeps the deterministic floor.
    pub frame_step_telemetry: vm::actor_tick::FrameStepTelemetry,
    /// Vsyncs accumulated toward the next **actor** game tick. Same clock as
    /// [`Self::clut_vsync_accum`] and the same law - retail resolves one
    /// `DAT_1F800393` per frame and runs the actor pool once per game tick,
    /// so the per-actor physics / anim / motion passes fire once every
    /// [`Self::frame_step`] vsyncs rather than once per rendered frame. The
    /// tick that fires carries [`Self::frame_step`] into the dispatcher's
    /// scalars ([`legaia_engine_vm::actor_tick::TickScalars::for_cadence`]),
    /// which is what keeps wall-clock durations identical while the pose
    /// sample rate drops.
    ///
    /// REF: FUN_80016B6C (cadence resolver), FUN_801D6704 (field floor = 2)
    pub actor_vsync_accum: u8,

    // --- live gameplay loop (Field <-> Battle round trip) -----------------
    /// Master opt-in for the **field side** of the in-`tick` Field <-> Battle
    /// round trip: the step-driven random-encounter roll.
    ///
    /// When `false` the Field branch of [`World::tick`] runs the field VM +
    /// locomotion but never rolls an encounter. When `true` it also drives
    /// [`World::live_field_tick`] - per-step roll, transition countdown, and
    /// the automatic `Field -> Battle` flip resolving a real formation.
    ///
    /// The **battle side is not gated by this flag.** Once the world is in
    /// [`SceneMode::Battle`] - however it got there: this roll, a field
    /// carrier's scripted `3E FF` fight, a world-map region encounter, or a
    /// direct [`World::enter_battle`] - [`World::tick`] always drives
    /// [`World::live_battle_tick`], because a battle that cannot resolve is a
    /// soft-lock. Retail has no "loop enabled" concept either
    /// (`FUN_801E295C`). Hosts that want a driven-battle-only slice can
    /// simply leave this flag off and enter battle themselves.
    ///
    // REF: FUN_801E295C (the retail action SM, which has no such gate)
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

    /// Route the live basic-attack damage through the retail damage
    /// finisher ([`legaia_engine_vm::battle_formulas::damage_finish`], the port
    /// of `FUN_801ddb30`) instead of stopping at the raw roll. The finisher
    /// adds the universal post-stages - the party defender's equipment
    /// elemental-resistance ladder (live, off the character's ability words
    /// via [`World::defender_resist`]), the rand-based no-damage floor on a
    /// hit mitigation zeroed, and the 9999 cap. The guard halve is
    /// deliberately not taken here: the melee kernel already charges the
    /// Spirit stance as its guard-roll triple. **On by default** - retail
    /// always runs the finisher after the melee roll; `false` keeps the flat
    /// pre-finisher path (min-floor 1, `0xFFFF` cap) for comparison. The
    /// finisher draws one RNG **only** when the hit zeroes out, matching
    /// retail.
    pub use_damage_finish: bool,

    /// The system flags the active scene's own field-VM records SET on
    /// their way into a `3E FF <row>` scripted battle entry, each paired
    /// with that row ([`crate::man_field_scripts::BattleEntryArm`]), read
    /// off the MAN when the scene's carriers are installed. The disc-side
    /// evidence a direct `--battle <row>` entry consults to replay the arm
    /// the row's own record raises ([`World::replay_scripted_battle_arm`]).
    pub scene_battle_entry_arms: Vec<crate::man_field_scripts::BattleEntryArm>,

    /// Battle "Select Attack" option - retail config word `0x800846C4`,
    /// the pause menu's row ([`crate::options::SelectAttackOpt`]): whether
    /// the ring's Attack arm shows the `Auto | Command` prompt (`0x78`), goes
    /// straight to the target cursor (`0x5A`) or straight to the directional
    /// arts entry (`0x50`) - `FUN_801D0748`'s `0x28` Left arm at
    /// `0x801D15E0..0x801D1650`. Hosts mirror their `OptionsState` onto this
    /// the way they mirror [`Self::field_move_run_default`].
    pub battle_select_attack: crate::options::SelectAttackOpt,

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
    /// hosts read [`Self::mode`] / [`Self::active_formation`] instead.
    pub field_return: Option<FieldReturnState>,

    /// Player tile `(col, row)` on the previous live-loop field tick. A
    /// change between ticks is one "step" and drives the encounter roll,
    /// mirroring the retail per-step counter rather than a per-frame roll.
    /// `None` until the first field tick records a tile. Managed by the live
    /// loop.
    pub field_last_tile: Option<(i16, i16)>,

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

    /// Field-scene carrier entities: the per-entity FUN_801DA51C state machines ticked in field scenes and their battle / engage handoffs.
    pub carriers: FieldCarrierState,

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

    /// Monotonic count of sim ticks that ran, advanced once per
    /// [`Self::tick`]. It is the world's cheapest "a frame actually ran"
    /// witness - the mode driver's frame-begin-skip test probes it to tell an
    /// abandoned frame from a live one.
    ///
    /// Historically this was a fixed-point phase accumulator bridging a
    /// claimed 100 Hz sim to retail's 60 Hz display frame. No host ever ticked
    /// at 100 Hz, so the phase only ever *withheld* retail frames; with the
    /// 1:1 denomination (see [`Self::tick`]) there is no phase left to carry.
    pub field_frame_accum: u32,

    /// Monotonic count of retail display frames elapsed. Consumers that have
    /// to advance something in retail-frame time (the renderer's cutscene
    /// camera glide, whose `apply_trigger` is a duration in display frames)
    /// diff this rather than counting sim ticks.
    ///
    /// Under the 1:1 denomination this equals [`Self::frame`]; it stays a
    /// separate counter because it names a *unit* (retail display frames) that
    /// the sim-tick counter does not promise.
    pub field_frames: u64,

    /// `1` on every sim tick that maps to a retail display frame - which, under
    /// the 1:1 denomination [`Self::tick`] documents, is every sim tick.
    ///
    /// Consumers gate on it to say "this is retail-frame paced": the narration
    /// roller (whose scroll speed is pinned as 1 px per 6 frames at 60 Hz), the
    /// effect pool, the escape timer, the CLUT / ambient game-tick banks, the
    /// timed sound release, and the field-NPC motion legs. It is a *unit*
    /// marker, not a throttle - a host that re-introduced oversampling would
    /// make it selective again without any of those call sites changing.
    pub field_frame_step: u16,

    /// Set by [`Self::seed_free_roam_story_baseline`] for scene-picker /
    /// `--scene` entries: the world was staged for a free-roam visit with no
    /// story behind it, so the BGM host arm drops entry-window pauses (their
    /// authored repair - the opening records' sub-9 restarts - never runs
    /// here). The new-game / opening chain leaves this `false`.
    pub free_roam_staging: bool,
    /// [`Self::field_frames`] at the most recent free-roam staging / scene
    /// entry - the base of the entry window the pause-drop measures against.
    pub free_roam_entry_frame: u64,

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

    /// `true` while [`Self::run_spawned_record_slice`] is stepping a spawned
    /// partition-2 record context (the modal cutscene timeline or a
    /// concurrent helper context). Host hooks use it to distinguish a
    /// spawned record's cross-context channel poke (seat the target exactly -
    /// the retail run settles on the op target) from the live channel
    /// stepper's own-script op (glide).
    pub in_spawned_record_slice: bool,

    /// The scene's `.MAP` object script binds
    /// (`(flat_record_index, contact_centre)`, retail `FUN_8003A55C`),
    /// stored at scene entry so a cutscene-timeline install that has to
    /// respawn the channel set can re-append the object-bind channels
    /// ([`crate::field_channels::spawn_object_channels`]).
    pub object_channel_binds: Vec<(usize, (i16, i16))>,

    /// Scripted player-move cues raised by cutscene-timeline `A2 F8
    /// <move_id>` ExecMove pokes, in emission order. The windowed host
    /// drains these each frame and queues the named clip as a one-shot on
    /// [`Self::field_player_anim`] - the clip is scene-ANM-bundle record
    /// `move_id - 1`, the same `id - 1` record space the op-`0x4B` NPC cues
    /// use (live-pinned: the `town01` post-naming ExecMove 48/49 land the
    /// retail player anim pointer on scene records 47/48).
    pub field_player_move_cues: Vec<u8>,

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
            cutscene: CutsceneState::new(),
            field_object_cells: Vec::new(),
            field_floor_cell_bit: legaia_asset::field_objects::CELL_WALK_VISIBLE,
            field_elevation_overrides: Vec::new(),
            follow_terrain_height: false,
            field_player_anim: None,
            leading_edge_wall_probes: false,
            npcs: FieldNpcState::new(),
            walk_regen_steps: 0,
            walk_regen_window: 0,
            field_camera_azimuth: 0,
            precise_movement: false,
            field_move_run_default: false,
            field_run_button_held: false,
            field_run_button_mask: crate::world::config::FIELD_RUN_BUTTON_MASK_DEFAULT,
            field_forced_slow: false,
            entry_pulse_enabled: true,
            precise_move_carry: (0.0, 0.0),
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
            casting: CastFxState::new(),
            sin_lut: Vec::new(),
            cos_lut: Vec::new(),
            character_ability_bits: [0; 8],
            battle: BattleState::new(),
            camera_shake_amplitude: 0,
            audio: AudioState::new(),
            party_count: 3,
            active_party: Vec::new(),
            screen_fade: None,
            effect_tint: None,
            screen_tint: None,
            field_mode_flags: 0,
            submode_context: [0; 10],
            roster: legaia_save::Party::zeroed(0),
            pending_scene_transition: None,
            pending_named_scene_transition: None,
            encounters: EncounterState::new(),
            pending_field_events: Vec::new(),
            pending_actor_spawns: Vec::new(),
            pending_battle_events: Vec::new(),
            current_dialog: None,
            three_actor_talk: None,
            talk_switch_requested: false,
            last_field_interact: None,
            active_inline_prologue: None,
            field_motion_prev: std::collections::HashMap::new(),
            field_actor_moving: std::collections::HashSet::new(),
            field_entry_prerun: false,
            camera_scene_offset: 0,
            camera_offset_ease: crate::camera_ease::CAMERA_OFFSET_EASE_SEED,
            camera_ease_prev_yz: None,
            field_prop_colliders: Vec::new(),
            resolved_cold_spawn: None,
            field_prop_bank: Default::default(),
            pending_prop_touch: None,
            field_walk_touch: std::collections::BTreeMap::new(),
            field_walk_touch_records: std::collections::BTreeMap::new(),
            active_walk_touch: None,
            last_move_dir_bits: 0,
            field_step_delta: (0, 0),
            field_vertical_settle: false,
            field_ledge_hop: None,
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
            pending_level_up_banners: std::collections::VecDeque::new(),
            current_capture_banner: None,
            world_map: WorldMapState::new(),
            board: TileBoardState::new(),
            screen_fx: Default::default(),
            screen_fx_frame: Default::default(),
            register_ramps: Vec::new(),
            cinematic_bars: None,
            cinematic_bar: 0,
            eased_moves: Vec::new(),
            field_eased_mirror_y: None,
            floor_tier_bobs: Vec::new(),
            camera_registers: Default::default(),
            minigames: MinigameState::new(),
            submode_screen: crate::field_submode_screen::SubmodeScreen::default(),
            tables: DiscTables::new(),
            seru: SeruState::new(),
            per_char_ext: Vec::new(),
            saved_chains: Vec::new(),
            play_time_seconds: 0,
            scene_save_allowed: false,
            field_stagers: Vec::new(),
            field_stager_bytes: Vec::new(),
            active_field_fx: Vec::new(),
            ambient: AmbientFxState::new(),
            reduce_flashing: true,
            // Field/town baseline; scene entry re-pins (`mapNN` -> 3).
            frame_step: 2,
            frame_step_floor: 2,
            scene_control_block: crate::scus_leaf_kernels::SCENE_CONTROL_BLOCK_RESET,
            frame_begin_skip: false,
            frame_step_telemetry: vm::actor_tick::FrameStepTelemetry::new(),
            actor_vsync_accum: 0,
            shops: ShopState::new(),
            menu: MenuState::new(),
            party_ability_mask: [0; crate::accessory_passives::ABILITY_WORDS],
            active_scene_label: String::new(),
            vdf_buffer: None,
            global_tmd_pool: Vec::new(),
            live_gameplay_loop: false,
            smarter_monster_targeting: false,
            use_vm_dialogue: false,
            use_damage_finish: true,
            scene_battle_entry_arms: Vec::new(),
            battle_select_attack: crate::options::SelectAttackOpt::default(),
            field_boss_stagers: std::collections::HashMap::new(),
            game_over: false,
            game_over_hold: false,
            field_return: None,
            field_last_tile: None,
            field_region_tracker: None,
            carriers: FieldCarrierState::new(),
            party_names: Vec::new(),
            name_entry: None,
            helper_contexts: Vec::new(),
            inline_dialogue: None,
            // Every sim tick is a retail display frame under the 1:1
            // denomination, so there is no phase to prime: a world that ticks
            // exactly once advances the roller and the retail-frame-paced
            // record contexts by exactly one frame.
            field_frame_accum: 0,
            field_frame_step: 0,
            field_frames: 0,
            free_roam_staging: false,
            free_roam_entry_frame: 0,
            field_channels: Vec::new(),
            field_channels_man: None,
            executing_channel: None,
            in_spawned_record_slice: false,
            object_channel_binds: Vec::new(),
            field_player_move_cues: Vec::new(),
            pending_record_spawns: Vec::new(),
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
    ///   picker visit never runs. [`Self::free_roam_staging`] lets the BGM
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
        self.free_roam_staging = true;
        self.free_roam_entry_frame = self.field_frames;
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
        self.story_flags = 0;
        self.story_flag_bits.clear();
        // A NEW GAME is the opening chain, not a free-roam picker visit: the
        // authored entry pauses / pre-event scenery are the point. Any flags
        // an earlier picker staging seeded reset with the bank.
        self.free_roam_staging = false;
        self.system_flags.clear();
        self.money = NEW_GAME_STARTING_GOLD;
        self.minigames.point_card = 0;
        self.inventory.clear();
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
        if let Some(t) = self.field_region_tracker.as_mut() {
            t.reset();
        }
        if let Some(t) = self.world_map.region_tracker.as_mut() {
            t.reset();
        }
        self.battle.end = None;
        self.game_over = false;
        self.game_over_hold = false;
        self.play_time_seconds = 0;
        self.cutscene.timeline = None;
        self.helper_contexts.clear();
        self.cutscene.narration = None;
        self.cutscene.card = None;
        self.cutscene.text_balloon = None;
        // Camera-register zone ramps are scene content: retail's MAN loader
        // retire sweep (`FUN_8003AEB0` at `0x8003B414`) is keyed on the ramp
        // actor's own handler VA, and the zone-miss defaults are reinstalled
        // by `FUN_801DBE9C`. Both happen on scene entry.
        self.register_ramps.clear();
        self.camera_registers = Default::default();
        // The three frame-delta timer templates are scene content too: the
        // MAN loader's retire sweep drops every pool actor, and a bar
        // envelope or a floor-rung bob left running across a scene change
        // would keep writing into the new scene's ladder.
        self.cinematic_bars = None;
        self.cinematic_bar = 0;
        self.eased_moves.clear();
        self.floor_tier_bobs.clear();
        self.cutscene.prologue_naming_pending = false;
        self.cutscene.prologue_naming_armed = false;
        self.cutscene.entering_town01_opening = false;
        self.pending_record_spawns.clear();
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
        self.inventory
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
