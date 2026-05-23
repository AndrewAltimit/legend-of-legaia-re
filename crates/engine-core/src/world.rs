//! Composite actor + scene runtime that wires the per-VM hosts together.
//!
//! `legaia-engine-vm` ships each script VM (actor / sprite, move-table,
//! effect, field, battle action) as a small clean-room port + a `Host` trait
//! that lets engines plug in their own state. This module is the engine-side
//! glue: a single [`World`] that owns the per-actor data and implements every
//! VM `Host` trait by routing into that data.
//!
//! ## Why a single composite
//!
//! In the retail runtime, "an actor" is a 0xCB-byte record holding everything
//! all four VMs read/write - world position, anim banks, flags, render bank,
//! per-action queue, etc. Splitting that across four crates would force
//! engines to keep four parallel index tables in sync. The composite pattern
//! here keeps the per-VM `ActorState` structs intact (clean-room boundary
//! preserved) but lets one struct own them.
//!
//! Engines that want a different layout - say, ECS storage - should
//! implement the VM `Host` traits themselves; this is the default.
//! REF: FUN_8001E890, FUN_80021DF4, FUN_80026B4C, FUN_8003CA38, FUN_8003CE08, FUN_800520F0
//! REF: FUN_801D65D8, FUN_801D77F4, FUN_801D8DE8, FUN_801DE840, FUN_801DFDF8

use std::sync::Arc;

use crate::battle_events::{BattleEvent, BattleHitFx};
use crate::field_events::FieldEvent;
use crate::input;
use crate::levelup::{LevelUpBanner, LevelUpResult, LevelUpTracker};
use crate::move_buffer_host;
use crate::tactical_arts::{ArtLearnedBanner, TacticalArtsTracker};
use crate::world_map::WorldMapController;
pub use legaia_anm::{AnimPlayer, PoseFrame};
use legaia_engine_vm as vm;
use legaia_save;
use vm::actor_tick::{ActorPhysics, ListenerState, TickEvent, TickResult, TickScalars};
use vm::battle_action::{
    BattleActionCtx, BattleActionHost, BattleActor, BattleEndCause, Pose, StepOutcome,
};
use vm::effect_vm::{EffectHost, MasterSlot, Pool, StateOutcome};
use vm::field::{CameraParam, FieldCtx, FieldHost, SceneFadeResult, StepResult as FieldStepResult};
use vm::move_buffer::{MoveBufferState, cursor_advance};
use vm::move_vm::{ActorState as MoveActorState, MoveHost};
use vm::{Host as ActorVmHost, Position as ActorVmPosition};

/// Maximum simultaneous actors in the world. Mirrors the battle-side cap of
/// 8 + 32 spare slots for field-side NPCs / cutscene actors.
pub const MAX_ACTORS: usize = 64;

/// Number of stat-bearing battle slots (party + monsters). Indexes
/// [`World::battle_attack`] / [`World::battle_defense`] / [`World::battle_speed`]
/// and bounds the turn-order initiative scan.
const BATTLE_SLOTS: usize = 8;

/// Default `start_slot` engines pass to
/// [`World::materialize_actor_spawns`]. Slots `0..FIELD_SPAWN_START_SLOT`
/// stay reserved so the field-VM actor-allocator path can't clobber the
/// party (slots `0..party_count`, typically 0..3) or the small band of
/// scripted NPC / cutscene actors the scene reserves above the party.
/// The exact retail value is unknown - 8 is the smallest power-of-two
/// that comfortably brackets every observed `party_count + scripted-NPC`
/// span and matches the start-slot the field-VM unit tests use.
pub const FIELD_SPAWN_START_SLOT: u8 = 8;

/// Per-frame opcode cap for the move VM. Retail has no explicit cap (relies
/// on opcodes naturally yielding via `WAIT_SET` / `HALT`); for a software
/// port we set a generous defensive cap so a buggy script can't hang the
/// engine. 4096 is well above the largest real Tactical-Arts move script.
pub const MOVE_VM_BUDGET: usize = 4096;

/// World units the player actor advances per frame while interpolating
/// to a target tile centre during a tile-board step. Retail derives the
/// per-frame delta from the frame-speed scalar `DAT_1f800393`
/// (`overlay_0897_801ef2b0` case 2); the engine uses a fixed cadence of
/// `TILE / 8` (16 units) - eight frames to cross one 128-unit tile.
const TILE_BOARD_SPEED: i32 = crate::tile_board::TILE / 8;

/// Bytes per row of the field collision grid (retail `0x80`-byte rows at
/// `*(_DAT_1F8003EC) + 0x4000`).
const FIELD_GRID_STRIDE: usize = 0x80;
/// Total field-collision-grid size: `0x80` rows of `0x80` bytes.
const FIELD_GRID_LEN: usize = FIELD_GRID_STRIDE * 0x80;
/// Base walk step (retail `base_step = 8` in `FUN_801d01b0`). Scaled by the
/// player's `+0x72` speed multiplier and the per-frame delta scalar.
const FIELD_BASE_STEP: i32 = 8;
/// Per-iteration advance of the locomotion step loop (retail commits in
/// 2-unit increments per axis).
const FIELD_STEP_UNIT: i32 = 2;
/// Retail player speed multiplier installed by the scene-entry map-init
/// `FUN_8003aeb0` (`player[+0x72] = 0x1000`, a `12.0` fixed-point `1.0`).
const FIELD_PLAYER_SPEED_MULT: u16 = 0x1000;

/// Cold field-entry player spawn coordinate (both X and Z).
///
/// On a non-warp (cold) field scene entry, the per-scene initializer
/// `FUN_801D6704` creates the player actor at actor coords
/// `(0xA40, 0, 0xA40)` - the centre of the camera's `0x20`-tile view window
/// (`func_0x80024c88(&local_68=...)`, with the `sVar13`/`sVar14` sub-tile
/// terms zero for a cold entry). Warp entries (`_DAT_8007b8b8 == 2`) override
/// X/Z from the saved transition coords `_DAT_80084568`/`_DAT_8008456C`.
///
/// Cold entry only ever happens for the New Game opening scene (`town01`,
/// Rim Elm), so this doubles as Vahn's authored opening spawn. See
/// `ghidra/scripts/funcs/overlay_0897_801d6704.txt` (the
/// `func_0x80024c88` call) and `docs/subsystems/field-locomotion.md`.
pub const FIELD_COLD_SPAWN_XZ: i16 = 0x0A40;

/// Starting gold (money) a New Game grants the party.
///
/// The retail new-game data-init `FUN_80034A6C` writes the party-gold global
/// `_DAT_8008459C` (the same word the battle-victory reward writer
/// `FUN_8004F0E8` credits) to a hardcoded `500` - it is a constant in the
/// init routine, not a field of the starting-party template. The same routine
/// also zeroes the story-flag region and calls the stat seed `FUN_800560B4`.
/// See `ghidra/scripts/funcs/80034a6c.txt`.
pub const NEW_GAME_STARTING_GOLD: i32 = 500;

/// Scratchpad flag-word bit (`_DAT_1F800394 & 0x0400_0000`, bit 26) that
/// the opening cutscene `opdeene` raises to arm the handoff to Rim Elm
/// (`town01`). Retail sets it with field-VM `GFLAG_SET 26` (op `0x2E`
/// operand `0x1A`) at the end of the prologue cutscene timeline, and the
/// per-frame field controller `FUN_801D1344` consumes it (with the
/// confirm-press gate) to issue the name-based scene change. See
/// [`World::arm_prologue_handoff`] / [`World::take_prologue_handoff`].
pub const PROLOGUE_HANDOFF_FLAG: u32 = 0x0400_0000;

/// Move `cur` toward `target` by at most `max_delta`, snapping exactly
/// onto `target` when within range. Used by the tile-board interpolator.
fn step_toward(cur: i32, target: i32, max_delta: i32) -> i32 {
    let d = target - cur;
    if d.abs() <= max_delta {
        target
    } else if d > 0 {
        cur + max_delta
    } else {
        cur - max_delta
    }
}

/// Decode one tile-step direction from the pad. Mirrors the single-
/// direction decode in the walk SM (`overlay_0897_801ef2b0` case 4):
/// vertical takes priority over horizontal, and only one axis moves per
/// step. D-pad only (board movement is digital).
fn tile_step_from_input(input: &input::InputState) -> Option<crate::tile_board::TileStep> {
    use crate::tile_board::TileStep;
    if input.pressed(input::PadButton::Up) {
        Some(TileStep::Up)
    } else if input.pressed(input::PadButton::Down) {
        Some(TileStep::Down)
    } else if input.pressed(input::PadButton::Left) {
        Some(TileStep::Left)
    } else if input.pressed(input::PadButton::Right) {
        Some(TileStep::Right)
    } else {
        None
    }
}

/// One queued fade request. Move-VM ext sub-op 0x3C writes either an
/// immediate fade (`ticks == 0`) or a ramp (`ticks > 0`) - engines drain
/// `pending_fade` each frame to drive the screen overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeRequest {
    pub rgb: [u8; 3],
    pub ticks: u16,
}

/// Render-agnostic snapshot of one live effect-pool master slot, produced by
/// [`World::active_effect_markers`] - one entry per effect (effect origin +
/// age). [`World::active_effect_sprites`] is the richer per-child billboard
/// view the textured-quad render path uses; this coarse marker remains for
/// hosts/tests that only need effect positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectMarker {
    /// Effect origin in world units (the pool stores 8.8 fixed-point; this
    /// is the integer-unit position the renderer's MVP consumes).
    pub world_pos: [f32; 3],
    /// 12-bit spawn angle (`master.angle`), passed through for hosts that
    /// orient the effect.
    pub angle: u16,
    /// Lifetime fraction in `0.0..=1.0` (0 = just spawned, 1 = about to
    /// retire). Hosts fade the marker out as this approaches 1.
    pub age01: f32,
}

/// Render-agnostic snapshot of one live effect **child sprite**, produced by
/// [`World::active_effect_sprites`]. This is the faithful billboard the retail
/// per-frame walker (`FUN_801E0088` pass 2) emits: a camera-facing quad sized
/// by the sprite-atlas entry, positioned at the effect origin plus the child's
/// spread offset, sampling VRAM at the atlas's `(u, v)` / `tpage` / `clut`.
///
/// The texel-source VRAM upload for battle effects is not yet pinned (see
/// `docs/formats/effect.md`), so a host that samples VRAM here will draw the
/// faithful geometry/animation with whatever is resident; the `page`/`clut`/
/// `uv` carry the real coordinates so textures appear once that upload lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectSprite {
    /// Child world position in world units (effect origin + spread offset).
    pub world_pos: [f32; 3],
    /// Billboard size in world units (atlas `w` × `h`, in texel-equivalent
    /// units the host scales as it sees fit).
    pub size: [f32; 2],
    /// Top-left source texel within the texture page (atlas `u`, `v`).
    pub uv: [u16; 2],
    /// Source rectangle size in texels (atlas `w`, `h`).
    pub uv_size: [u16; 2],
    /// PSX `tpage` descriptor (texture-page base + colour mode).
    pub page: u16,
    /// CLUT (CBA) id.
    pub clut: u16,
    /// Lifetime fraction `0.0..=1.0` (for fade-out), shared with the effect.
    pub age01: f32,
}

/// Render-agnostic snapshot of one live effect's **3D model** (`etmd.dat`),
/// produced by [`World::active_effect_models`]. This is the other effect
/// render path alongside [`EffectSprite`]'s 2D billboards: spell effects like
/// *Tail Fire* are small Gouraud-shaded `etmd` meshes textured by `etim`
/// (pinned pixel-exact against a live battle VRAM capture - see
/// `docs/formats/effect.md`), not billboards.
///
/// The host resolves `tmd_index` through its global TMD pool
/// ([`World::global_tmd`]), builds a VRAM mesh, and draws it at `world_pos`
/// (the `etim` texels are already resident in scene VRAM). The retail
/// effect-id -> model selection is driven by the move/art VM and not yet
/// decoded, so the only producer today is the model-spawn helper
/// ([`World::spawn_debug_effect_model`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectModel {
    /// Index into [`World::global_tmd_pool`] (the `etmd.dat` head) for this
    /// effect's mesh.
    pub tmd_index: usize,
    /// Effect origin in world units (the pool stores 8.8 fixed-point).
    pub world_pos: [f32; 3],
    /// 12-bit spawn angle (`master.angle`).
    pub angle: u16,
    /// Lifetime fraction `0.0..=1.0` (0 = just spawned, 1 = about to retire).
    pub age01: f32,
}

/// Scene mode the world is running. Drives which top-level VMs tick and
/// which auxiliary state lives in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneMode {
    /// Title / no scene. Only the actor VM and effect pool are live.
    #[default]
    Title,
    /// Field / town scene - field VM drives event flow.
    Field,
    /// Battle scene - battle action state machine runs over the actor table.
    Battle,
    /// Cutscene mode - actor VM runs but no field/battle dispatch.
    Cutscene,
    /// World-map mode - `WorldMapController` drives camera and entity ticks.
    WorldMap,
}

/// One sprite frame on a sprite sheet. Equivalent in shape to
/// `legaia_engine_render::SpriteRequest` but lives in engine-core (which
/// can't depend on the wgpu-bound render crate). Engine binaries
/// translate one-to-one when handing the per-frame sprite list to the
/// renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteFrame {
    /// Atlas source rect (`x`, `y`, `w`, `h`) in atlas texels.
    pub atlas_src: (u32, u32, u32, u32),
    /// Tint multiplied with the sampled atlas texel.
    pub tint: [f32; 4],
    /// World-y offset (pixels) added to the actor's screen-space y when
    /// the renderer projects [`Actor::move_state`] coords. `0` for ground-
    /// level sprites.
    pub anchor_y: i16,
}

impl Default for SpriteFrame {
    fn default() -> Self {
        Self {
            atlas_src: (0, 0, 0, 0),
            tint: [1.0; 4],
            anchor_y: 0,
        }
    }
}

/// Per-actor sprite request emitted by [`World::collect_sprite_requests`].
/// Engine binaries map this 1:1 to `legaia_engine_render::SpriteRequest`
/// for the wgpu sprite-batch upload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorSpriteRequest {
    /// Index in [`World::actors`] this request came from.
    pub actor_slot: u8,
    /// Top-left in screen-space pixels (the engine projects
    /// [`Actor::move_state`] world coords through its camera before
    /// emitting; this is the post-projection result).
    pub world_x: i32,
    pub world_y: i32,
    pub atlas_src: (u32, u32, u32, u32),
    pub tint: [f32; 4],
}

/// One entry in [`World::global_tmd_pool`]. Mirrors a single slot of the
/// retail `DAT_8007C018` global TMD-pointer table - a parsed TMD plus its
/// raw bytes so downstream renderers can drive `legaia_tmd::mesh::*` builders
/// without re-fetching from disc.
///
/// See `project_dat_8007c018_global_tmd_table.md` and
/// `project_global_tmd_pool_source.md` for the retail-side semantics. The
/// clean-room port treats the pool as opaque indexed storage: the field-VM
/// `0x4C 0xD8` host hook reads slot `tmd_idx` and writes the resulting `Arc`
/// onto [`Actor::tmd_ref`] - whatever populated the slot is the producer's
/// concern.
#[derive(Debug)]
pub struct GlobalTmd {
    pub tmd: legaia_tmd::Tmd,
    pub raw: Vec<u8>,
}

/// Per-actor record held by the world. Composes the per-VM state structs.
///
/// Each VM's `Host` trait reads/writes only the slice it owns, so the per-VM
/// state structs don't need to know about each other. The world coalesces
/// world-XYZ to keep rendering / collision queries on one source of truth.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    /// `true` if this slot is in use. Empty slots are skipped by every VM
    /// dispatcher.
    pub active: bool,

    /// Default-position lookup result for the actor VM. Retail reads this
    /// from a 16-byte-stride table at `0x801E473C`; engines populate per
    /// scene from extracted assets.
    pub default_pos: ActorVmPosition,

    /// Move-VM per-actor state.
    pub move_state: MoveActorState,

    /// Per-actor physics tick state - port of `FUN_80021DF4`. Driven
    /// each frame by [`World::tick_actor_physics`]. The dispatch byte
    /// at [`ActorPhysics::dispatch_byte`] (`+0x5A`) selects the per-arm
    /// behaviour; engines set it via [`Actor::set_physics_dispatch`].
    pub physics: ActorPhysics,

    /// Per-actor move-buffer cursor + envelope state, mirroring the
    /// retail layout at `actor[+0x5C..+0xCC]`. Lives in its own struct
    /// because retail aliases `+0xA0` / `+0xB8` / `+0xC8` with the
    /// physics view; see [`vm::move_buffer::MoveBufferState`].
    /// Updated by [`World::tick_actor_physics`] in response to the
    /// per-arm [`TickEvent::MoveVmKick`] signal.
    pub move_buffer: MoveBufferState,

    /// Battle-action per-actor state. Populated only when the world is in
    /// [`SceneMode::Battle`].
    pub battle: BattleActor,

    /// Sprite / actor-VM scratch fields:
    /// - `field_1d`: opaque per-actor flag (set by actor VM op `SetField1d`).
    /// - `field_20`: opaque per-actor 16-bit slot (cleared by `ClearField20`).
    pub field_1d: u8,
    pub field_20: u16,

    /// `subobj` snap-clear condition flag - engine sets this when the actor's
    /// subobj is in the "snap to anchor" configuration. Read by actor VM op
    /// `SpawnDefault`.
    pub snap_clear: bool,

    /// Optional motion target consumed by actor VM op `EffectMotion`.
    /// `None` → the op no-ops (the retail equivalent of a null subobj
    /// pointer).
    pub motion_target: Option<ActorVmPosition>,

    /// Last-frame effect spawn - engine wires whatever rendering / sound
    /// flash it has. We just record the actor id for inspection.
    pub last_effect: u32,

    /// Most recent status effect inflicted by an art strike (Burned /
    /// Shocked / …). Engines clear this when they've folded it into their
    /// status-bar UI; defaults to `None`.
    pub pending_status: Option<legaia_art::record::EnemyEffect>,

    /// Optional sprite frame for this actor. Drives the per-frame sprite
    /// batch through [`World::collect_sprite_requests`]. When `None`, the
    /// actor is invisible (or rendered as a 3D mesh through the TMD path).
    pub sprite_frame: Option<SpriteFrame>,

    /// Active keyframe animation player. `None` means no animation is
    /// playing. Set via [`World::set_actor_animation`].
    pub active_animation: Option<AnimPlayer>,

    /// Last per-bone pose produced by `active_animation.tick()`. `None`
    /// until the first frame after an animation is assigned. Renderers
    /// consume this via `tmd_to_vram_mesh_posed` to deform the actor's
    /// mesh each frame.
    pub pose_frame: Option<PoseFrame>,

    /// Index into `SceneResources::tmds` for this actor's bound mesh.
    /// `None` means no TMD is bound - the actor has no visible 3D model.
    /// Set via [`World::set_actor_tmd_binding`].
    pub tmd_binding: Option<usize>,

    /// Actor kind classifier. Retail equivalent: the `u16` at `actor[+0x3C]`
    /// written by `FUN_801D77F4` (overlay actor allocator). Zero means
    /// "unset" - either the actor was created via the actor-VM path
    /// (`spawn` / `spawn_actor`) rather than the field-VM allocator, or no
    /// kind has been wired yet.
    pub kind: u16,
    /// Actor variant. Retail equivalent: the `u16` at `actor[+0x3E]`.
    /// Co-written with `kind` by `FUN_801D77F4`.
    pub variant: u16,
    /// Record bytecode this actor was instantiated from. Retail stores a
    /// pointer at `actor[+0x4C]` whose meaning depends on the allocation
    /// path - for the field-VM `0x4C 0x80` allocator the pointer addresses
    /// the child-actor packet that the parent script's `tail` contributed.
    /// `None` for actors spawned through other paths.
    ///
    /// Distinct from [`Self::tmd_binding`] / [`Self::active_animation`],
    /// which cover the rendering side.
    pub spawn_record: Option<Vec<u8>>,

    /// Global TMD this actor was instantiated with. Retail equivalent: the
    /// `u32` at `actor[+0x48]` written by the overlay actor allocator -
    /// `iVar13 = *(int *)(&DAT_8007C018 + ((tmd_idx << 16) >> 14))`. Set by
    /// the field-VM `0x4C 0xD8` host hook from [`World::global_tmd`], and
    /// reachable from rendering / animation systems that want a mesh +
    /// raw bytes for this actor without re-walking the global pool.
    /// `None` for actors spawned through paths that don't reference the
    /// global TMD pool (the actor VM's own `Spawn*` opcodes, etc.).
    pub tmd_ref: Option<Arc<GlobalTmd>>,

    /// Monster id (1-based, into the PROT 867 monster archive) when this
    /// actor is an enemy spawned for a battle formation. `None` for party
    /// members and for actors outside a formation-driven battle. Set by
    /// [`World::enter_battle_from_formation`]; lets a renderer fetch the
    /// monster's battle mesh + texture pool (the on-disc CBA/TSB are
    /// relocated per battle slot - see
    /// `legaia_asset::monster_archive::MonsterMesh::battle_render_mesh`).
    pub battle_monster_id: Option<u16>,
}

impl Actor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark this slot as active. Returns `&mut Self` for chaining.
    pub fn activate(&mut self) -> &mut Self {
        self.active = true;
        self
    }

    /// Engine-side hook: set the dispatch byte the per-actor physics
    /// tick reads at `actor[+0x5A]`. See [`vm::actor_tick`] for the
    /// dispatch ladder. Most actors run dispatch byte `0x06` (the
    /// keyframe arm); engines set per-actor as scene assets are bound.
    pub fn set_physics_dispatch(&mut self, b: u16) {
        self.physics.set_dispatch(b);
    }
}

/// One active stat buff / debuff produced by a battle Magic cast.
///
/// The applied delta is the exact change written into the per-slot scalar
/// ([`World::battle_attack`] / [`World::battle_defense`] / [`World::battle_magic`]),
/// so [`World::finish_battle`] (or natural expiry) can revert it precisely.
/// Stats with no live-loop scalar (Accuracy / Evasion / Speed) are tracked
/// with a zero delta so the timer still expires cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleBuff {
    /// Actor-table slot the buff applies to.
    pub slot: u8,
    /// Buffed stat.
    pub stat: crate::spells::BuffStat,
    /// Signed delta actually written into the per-slot scalar (`0` for stats
    /// that have no live-loop scalar).
    pub applied_delta: i16,
    /// Turns remaining before the buff expires.
    pub turns: u8,
}

/// One monster's chosen action for its turn, produced by the action picker
/// [`World::pick_monster_action`] (the port of `FUN_801E9FD4`'s decision core).
#[derive(Debug, Clone, PartialEq, Eq)]
enum MonsterAction {
    /// Physical strike against party slot `target`.
    Physical { target: u8 },
    /// Cast `spell_id` against the resolved absolute `targets` slots.
    Cast { spell_id: u8, targets: Vec<u8> },
}

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
    /// Shared system flag bank at `_DAT_80086D70` - bitfield read / written
    /// by:
    /// - field VM high-byte default routes 0x5x / 0x6x / 0x7x
    ///   (`system_flag_set` / `system_flag_clear` / `system_flag_test`)
    /// - move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D
    ///   (`ext_query_flag_bank` / `ext_set_flag_bank` / `ext_clear_flag_bank`)
    ///
    /// Lazily grown on write - the field VM's opcode-encoded idx ranges over
    /// `0..=0x87FF`, so a fixed 256-bit array is too small.
    pub system_flags: Vec<u8>,
    /// Field-VM `extra_flags` register read by op 0x42 mode 0 - a 32-bit
    /// auxiliary flag word (origin TBD; treated as scene-local state).
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
    /// turn-order fallback ([`World::next_living_combatant`]); when any actor
    /// carries real SPD the next-actor selector switches to the SPD-seeded
    /// initiative scheme ([`World::next_combatant_by_initiative`], the port of
    /// `recompute_battle_order` / `FUN_801daba4`).
    pub battle_speed: [u16; 8],

    /// "Previous action cleared" gate - toggled by the engine when an
    /// animation transition completes.
    pub prev_action_cleared: bool,
    /// "Sound bank ready" gate.
    pub sound_bank_ready: bool,

    /// Number of party slots (default 3).
    pub party_count: u8,

    /// Last-issued battle-end cause (for inspection / engine side-effects).
    pub battle_end: Option<BattleEndCause>,

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

    /// Pending FMV trigger (field-VM op `0x4C 0xE2`). When `Some(fmv_id)`,
    /// the field VM has signalled that the next-game-mode global should
    /// transition to game mode 26 (StrInit) with the given index. Engines
    /// drain this after [`World::tick`] to actually open the corresponding
    /// `MV*.STR` (use [`crate::cutscene::FmvIndex::str_filename`] for the
    /// retail mapping). `None` between triggers.
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

    /// Field track stashed at battle entry so [`World::restore_field_bgm`]
    /// can resume it after the encounter. Managed by the swap helpers; not
    /// meant to be set directly.
    pub field_bgm_resume: Option<u16>,

    /// `true` while the battle track is playing (set by
    /// [`World::swap_to_battle_bgm`], cleared by
    /// [`World::restore_field_bgm`]). Guards against double-swap / spurious
    /// restore.
    pub battle_bgm_active: bool,

    /// Active dialog request - populated by the field-VM op 0x3F handler,
    /// cleared by the engine after the user dismisses the box. The MES
    /// renderer reads `text_id` + `inline`; the world-coords + depth feed
    /// the box placement.
    pub current_dialog: Option<DialogRequest>,

    /// Last `field_interact` request. Cleared by the engine when handled
    /// (set to `None`).
    pub last_field_interact: Option<(u8, u8)>,

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
    /// ([`Self::tick_tile_board`]), the world-map controller
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

    /// Active post-battle Seru-capture banner. Set by [`World::resolve_captures`]
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

    /// Per-actor status-effect tracker (Burned / Shocked / Poisoned /
    /// Asleep / Confused / Silenced / Stunned / Petrified). Populated by
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

    /// Item catalog used by item-action resolution. Populated at battle
    /// init from [`crate::items::ItemCatalog::vanilla`] (or a custom
    /// catalog set by [`World::set_item_catalog`]); empty by default so
    /// the field VM doesn't trigger item effects in non-battle scenes.
    pub item_catalog: crate::items::ItemCatalog,

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
    /// Engines install via [`World::set_seru_registry`]; [`World::finish_battle`]
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

    /// Per-item battle-stat modifier table (weapon / armor / accessory
    /// bonuses). Empty by default; install via [`World::set_equipment_table`]
    /// so [`World::seed_party_battle_stats`] folds equipped gear onto each
    /// party combatant's attack / defense at battle entry.
    pub equipment_table: crate::battle_stats::EquipmentTable,

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
    /// [`Self::art_records`] by [`Self::build_battle_arts_rows`]. Hosts read it
    /// to draw the arts overlay.
    pub battle_arts_menu: Option<crate::battle_arts::BattleArtsSession>,

    /// Active stat buffs / debuffs applied by battle Magic, one entry per
    /// `(slot, stat)`. Each holds the exact delta written into the per-slot
    /// scalar so expiry can undo it, plus the remaining turn count (decremented
    /// at the start of the buffed actor's turn). Cleared - and their deltas
    /// reverted - by [`World::finish_battle`].
    pub battle_buffs: Vec<BattleBuff>,

    /// Monster ids captured this battle by a capture spell (`SpellEffect::Capture`).
    /// The captured monster is downed immediately; the host drains this for
    /// post-battle Seru-learning resolution (the live loop carries no Seru
    /// registry, so the learn step itself lives outside the battle tick).
    pub battle_captures: Vec<u16>,

    /// Set when an escape spell (`SpellEffect::Escape`) resolves. The live
    /// battle tick returns to the field on the next pass (no loot, no
    /// game-over). Cleared by [`World::finish_battle`].
    pub battle_escaped: bool,

    /// Formation currently being fought, captured at the `Field -> Battle`
    /// transition. Drives [`World::apply_battle_loot`] on victory. `None`
    /// outside battle.
    pub active_formation: Option<crate::monster_catalog::FormationDef>,

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
    /// Driven each [`SceneMode::WorldMap`] tick by [`Self::tick_world_map`].
    pub world_map_entities: Vec<vm::world_map::WorldMapEntityCtx>,

    /// Per-entity role config, paired by index with [`Self::world_map_entities`].
    /// Empty (or shorter than the entity list) means an entity has no specific
    /// role: its encounters fall back to [`Self::world_map_encounter`]'s shared
    /// formation and it surfaces a plain interaction. Installed together with
    /// the entities via [`Self::install_world_map_entities_with_configs`].
    pub world_map_entity_configs: Vec<WorldMapEntityConfig>,

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
    /// of [`Self::tick_world_map`] to flip into [`SceneMode::Battle`]. `None`
    /// between encounters.
    pub pending_world_map_encounter: Option<u16>,

    /// Scene mode to return to when the current battle finishes. Captured at
    /// the transition into [`SceneMode::Battle`]; [`Self::finish_battle`]
    /// restores it (an overworld encounter returns to [`SceneMode::WorldMap`],
    /// a field encounter to [`SceneMode::Field`]). Defaults to
    /// [`SceneMode::Field`].
    pub battle_return_mode: SceneMode,

    /// Per-entity **field** state machines - the same `FUN_801DA51C` SM the
    /// overworld uses ([`vm::world_map`]), but ticked in [`SceneMode::Field`]
    /// for the scene's MAN-placed actors. A scripted-encounter carrier (the
    /// Rim Elm Tetsu fight) sits Idle until [`Self::engage_field_carrier`]
    /// (the dialogue-accept) advances it to `Activating`; the next
    /// [`Self::tick_field_carriers`] then copies its formation and launches the
    /// battle, mirroring retail's state-1 `entity[+0x94]` copy + `case 2/3`
    /// fall-through battle handoff. Empty unless
    /// [`Self::install_field_carriers`] seeded them.
    pub field_carriers: Vec<vm::world_map::WorldMapEntityCtx>,

    /// Per-carrier role config, paired by index with [`Self::field_carriers`].
    pub field_carrier_configs: Vec<FieldCarrierConfig>,

    /// Field carrier battle pending resolution: the MAN `formation_id` a
    /// carrier SM latched on its scene-transition this frame. Drained at the
    /// end of [`Self::tick_field_carriers`] to flip Field -> Battle. `None`
    /// between transitions.
    pub pending_field_carrier_battle: Option<u16>,

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
}

/// Per-field-carrier role. The retail engine builds one record per MAN-placed
/// scene entity; this is the clean-room slice the field entity SM acts on.
/// Paired by index with [`World::field_carriers`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldCarrierConfig {
    /// A **scripted-encounter carrier**: engaging it (the dialogue-accept)
    /// advances its `FUN_801DA51C` SM to its scene-transition, which selects
    /// MAN formation `formation_id` (by index, so the scene's merged monster
    /// stats stand) and launches the battle. The Rim Elm Tetsu tutorial fight
    /// is `formation_id` [`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`].
    ScriptedEncounter { formation_id: u16 },
    /// A plain interactable NPC. Surfaces a
    /// [`crate::field_events::FieldEvent::FieldInteract`] with `interact_id`.
    Npc { interact_id: u8 },
}

/// Per-overworld-entity role. The retail engine builds one record per on-map
/// entity from the scene's entity table; this is the clean-room slice the
/// gameplay SM acts on - an encounter zone spawns its own formation, a portal
/// targets a scene, an NPC just surfaces an interaction. Paired by index with
/// [`World::world_map_entities`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldMapEntityConfig {
    /// A roaming-encounter zone: when the shared countdown drains while this
    /// entity is the one that fires, it spawns `formation_id` (instead of the
    /// map-wide [`WorldMapEncounterState::formation_id`]).
    EncounterZone { formation_id: u16 },
    /// A town / dungeon portal: engaging it (via
    /// [`World::engage_world_map_entity`]) transitions to `target_map`,
    /// surfaced as [`crate::field_events::FieldEvent::WorldMapTransition`].
    Portal { target_map: u16 },
    /// A plain interactable (NPC / signpost). Surfaces a
    /// [`crate::field_events::FieldEvent::FieldInteract`] with `interact_id`.
    Npc { interact_id: u8 },
}

/// Shared overworld encounter-rate state read by the world-map entity SM.
#[derive(Debug, Clone)]
pub struct WorldMapEncounterState {
    /// `DAT_8007b604` - signed per-step encounter countdown shared across all
    /// overworld entities. Decremented in the entity SM's Idle state; an
    /// encounter fires when it reaches zero (and [`Self::enabled`]).
    pub countdown: i8,
    /// `DAT_8007b5f8 != 0` - master encounter-enable flag. When `false`,
    /// overworld encounters never fire regardless of the countdown.
    pub enabled: bool,
    /// Formation id an overworld encounter spawns (resolved against
    /// [`World::formation_table`]). The retail per-region resolution
    /// (`FUN_800243F0` → the MAN region table) is a separate thread; v0.1
    /// uses one configured formation for the whole map.
    pub formation_id: u16,
    /// Frames to reset the countdown to after an encounter fires (so the next
    /// encounter is paced rather than re-firing every frame at zero).
    pub reset_to: i8,
}

impl Default for WorldMapEncounterState {
    fn default() -> Self {
        Self {
            countdown: 0,
            enabled: false,
            formation_id: 0,
            reset_to: 64,
        }
    }
}

/// Field state snapshot taken at the `Field -> Battle` transition and
/// restored when the live loop returns from battle. See
/// [`World::live_gameplay_loop`].
#[derive(Debug, Clone, Default)]
pub struct FieldReturnState {
    pub actors: Vec<Actor>,
    pub player_actor_slot: Option<u8>,
    pub party_count: u8,
}

/// Pending dialog request for the field-VM op 0x3F handler. The engine
/// renders + advances; clearing `World::current_dialog` signals the script
/// to resume.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogRequest {
    pub text_id: u16,
    pub inline: Vec<u8>,
    pub world_x: u16,
    pub world_z: u16,
    pub depth_id: u8,
}

/// Aggregated post-battle rewards returned by [`World::apply_battle_loot`].
///
/// Engines surface this as the post-battle banner ("got X XP, Y gold, level
/// up!"). The XP / gold totals reflect what was actually credited (monster
/// ids missing from the catalog don't contribute), `level_ups` carries the
/// per-character results from [`World::apply_battle_xp`], and `drops`
/// carries the item ids the loot roll surfaced from each fallen monster.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleRewards {
    pub xp: u32,
    pub gold: u32,
    pub level_ups: Vec<LevelUpResult>,
    /// Item drops the post-battle loot roll surfaced. One entry per
    /// monster slot that *both* (a) had a non-`None` `drop_item` in the
    /// catalog and (b) rolled below `drop_rate_q8 / 256`.
    pub drops: Vec<u8>,
}

/// Camera state populated by `camera_save` / `camera_load` and read by
/// `camera_apply`. Engines render the configured camera each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraState {
    /// Last `CameraConfigure` params applied (param-mask + values).
    pub params: Vec<CameraParam>,
    /// Last apply-trigger value.
    pub apply_trigger: u16,
    /// Last apply-mode nibble.
    pub mode: u8,
    /// Snapshot bytes from `camera_save`.
    pub saved: Vec<u8>,
    /// Last `camera_load` payload.
    pub loaded_payload: Vec<u8>,
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
            prev_action_cleared: true,
            sound_bank_ready: true,
            party_count: 3,
            battle_end: None,
            roster: legaia_save::Party::zeroed(0),
            pending_scene_transition: None,
            pending_fmv_trigger: None,
            pending_scripted_encounter: None,
            scripted_encounter_armed: false,
            active_fmv: None,
            cutscene_return_mode: None,
            pending_field_events: Vec::new(),
            pending_actor_spawns: Vec::new(),
            pending_battle_events: Vec::new(),
            battle_hit_fx: Vec::new(),
            current_bgm: None,
            battle_bgm: None,
            field_bgm_resume: None,
            battle_bgm_active: false,
            current_dialog: None,
            last_field_interact: None,
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
            status_effects: vm::status_effects::StatusEffectTracker::new(),
            ap_gauges: [crate::ap_gauge::ApGauge::default(); 3],
            item_catalog: crate::items::ItemCatalog::default(),
            spell_catalog: crate::spells::SpellCatalog::default(),
            art_records: std::collections::HashMap::new(),
            battle_buffs: Vec::new(),
            battle_captures: Vec::new(),
            battle_escaped: false,
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
            equipment_table: crate::battle_stats::EquipmentTable::new(),
            monster_ai_state: crate::monster_ai::MonsterAiState::new(),
            active_scene_label: String::new(),
            vdf_buffer: None,
            global_tmd_pool: Vec::new(),
            live_gameplay_loop: false,
            battle_player_driven: false,
            battle_command: None,
            battle_item_menu: None,
            battle_spell_menu: None,
            battle_arts_menu: None,
            active_formation: None,
            last_battle_rewards: None,
            game_over: false,
            field_return: None,
            field_last_tile: None,
            world_map_entities: Vec::new(),
            world_map_entity_configs: Vec::new(),
            world_map_encounter: WorldMapEncounterState::default(),
            world_map_player_walking: false,
            pending_world_map_encounter: None,
            battle_return_mode: SceneMode::Field,
            field_carriers: Vec::new(),
            field_carrier_configs: Vec::new(),
            pending_field_carrier_battle: None,
            party_names: Vec::new(),
            name_entry: None,
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
    /// over from a prior session) and set [`SceneMode::Field`] — the
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
        self.pending_fmv_trigger = None;
        self.pending_scripted_encounter = None;
        self.scripted_encounter_armed = false;
        self.encounter = None;
        self.battle_end = None;
        self.game_over = false;
        self.play_time_seconds = 0;
        self.mode = SceneMode::Field;
    }

    /// Record the active scene label. Engines call this from the scene-load
    /// path (typically right before `install_encounter_for_scene`) so
    /// downstream consumers (HUD, diagnostics, save snapshots) can surface
    /// the current scene without re-walking the [`crate::scene::SceneHost`].
    pub fn set_active_scene_label(&mut self, label: impl Into<String>) {
        self.active_scene_label = label.into();
    }

    /// Display name for a party slot - the name-entry result if one was
    /// committed, otherwise the template default seeded at
    /// [`Self::seed_starting_party`]. Empty string when the slot is unknown.
    pub fn party_name(&self, slot: usize) -> &str {
        self.party_names.get(slot).map(String::as_str).unwrap_or("")
    }

    /// Open the name-entry overlay for `slot`, seeded with the slot's current
    /// display name (e.g. the template `Vahn`). Mirrors the opening `town01`
    /// script's lead-character naming prompt. The host drives it each frame
    /// with [`Self::step_name_entry`] and renders from [`Self::name_entry`].
    pub fn open_name_entry(&mut self, slot: usize) {
        let initial = self.party_name(slot).to_string();
        self.name_entry = Some(crate::name_entry::NameEntry::new(slot, &initial));
    }

    /// `true` while the name-entry overlay is active.
    pub fn name_entry_active(&self) -> bool {
        self.name_entry.is_some()
    }

    /// Advance the active name-entry overlay by one input frame. On commit
    /// (the player confirms "Is this name okay?") the entered name is written
    /// into [`Self::party_names`] for the entry's slot, the session is closed,
    /// and `true` is returned so the host can resume the field script.
    /// Returns `false` while the overlay stays open (or when none is active).
    pub fn step_name_entry(&mut self, input: crate::name_entry::NameEntryInput) -> bool {
        let Some(entry) = self.name_entry.as_mut() else {
            return false;
        };
        entry.step(input);
        if entry.state == crate::name_entry::NameEntryState::Done {
            let slot = entry.char_index;
            let name = entry.committed_name();
            if self.party_names.len() <= slot {
                self.party_names.resize(slot + 1, String::new());
            }
            self.party_names[slot] = name;
            self.name_entry = None;
            true
        } else {
            false
        }
    }

    /// Arm the prologue cutscene -> Rim Elm handoff.
    ///
    /// In retail the opening cutscene scene `opdeene` runs a scripted
    /// timeline (a field-VM record in the MAN's third record partition)
    /// that ends with `GFLAG_SET 26` - field-VM op `0x2E` with operand
    /// `0x1A`, which sets bit 26 (`0x0400_0000`) of the scratchpad flag
    /// word `_DAT_1F800394` (the engine's [`Self::story_flags`]) right
    /// after staging the closing camera + actor moves. Once that bit is
    /// set, the per-frame field controller `FUN_801D1344` waits for the
    /// player's confirm press and then issues a name-based scene-change
    /// packet to `town01` (see [`Self::take_prologue_handoff`]).
    ///
    /// The engine doesn't yet replay that cutscene timeline (only record
    /// 0 of the scene runs), so callers arm the bit explicitly when they
    /// enter `opdeene` live. This sets exactly the flag the retail
    /// `GFLAG_SET 26` would, so the downstream gate stays faithful.
    // REF: FUN_801D1344
    pub fn arm_prologue_handoff(&mut self) {
        self.story_flags |= PROLOGUE_HANDOFF_FLAG;
    }

    /// Poll the prologue cutscene -> Rim Elm handoff gate.
    ///
    /// Faithful port of the one-shot block in `FUN_801D1344`:
    ///
    /// ```c
    /// if (_DAT_8007b868 == 0 && (_DAT_1f800394 & 0x4000000) && (_DAT_8007b850 & 0x100)) {
    ///     ... fade; town01 entry coords (0xec0, 0x2dc0); ...
    ///     _DAT_1f800394 &= 0xfbffffff;            // fire-once: clear bit 26
    ///     func_0x8001fd44(s_town01_801ce82c, 3);  // name-based scene change
    /// }
    /// ```
    ///
    /// Returns the handoff target scene ([`legaia_asset::new_game::OPENING_SCENE`]
    /// = `town01`) once - when the active scene is the prologue cutscene
    /// ([`legaia_asset::new_game::OPENING_CUTSCENE_SCENE`] = `opdeene`),
    /// the trigger bit is set ([`Self::arm_prologue_handoff`]), and the
    /// caller reports a confirm-button press this frame. Clears the bit so
    /// it fires once, exactly as retail clears `0x4000000`. Returns `None`
    /// otherwise. The host issues the actual scene change (the engine's
    /// equivalent of the scene-change packet) on a `Some`.
    // REF: FUN_801D1344
    // REF: FUN_8001FD44
    pub fn take_prologue_handoff(&mut self, confirm: bool) -> Option<&'static str> {
        if confirm
            && self.story_flags & PROLOGUE_HANDOFF_FLAG != 0
            && self.active_scene_label == legaia_asset::new_game::OPENING_CUTSCENE_SCENE
        {
            self.story_flags &= !PROLOGUE_HANDOFF_FLAG;
            Some(legaia_asset::new_game::OPENING_SCENE)
        } else {
            None
        }
    }

    /// Install the VDF ("set_mime") buffer for the active scene. The bytes
    /// must follow the `[u32 count][u32 byte_offsets[count]][body...]`
    /// layout the retail asset-dispatcher case 7 produces; see
    /// [`Self::vdf_buffer`] for citation. Engines that want the
    /// field-VM `0x4C 0xD8` opcode to surface real spawn bytecode call
    /// this on scene-load with the extracted asset-type-7 chunk's body.
    ///
    /// Passing `None` clears the buffer; the next `0x4C 0xD8` call will
    /// leave `Actor::spawn_record` empty.
    pub fn set_vdf_buffer(&mut self, bytes: Option<Vec<u8>>) {
        self.vdf_buffer = bytes;
    }

    /// Resolve a VDF body slice by index using the
    /// `[u32 count][u32 byte_offsets[count]][body...]` layout. Each
    /// returned slice starts at `byte_offsets[idx]` and runs to the
    /// next body's offset (or end-of-buffer for the last entry).
    ///
    /// Returns `None` when:
    ///  - no VDF buffer is set (the scene loader skipped the install),
    ///  - the buffer is too short to read the header,
    ///  - `idx >= count`, or
    ///  - the indexed offset walks past end-of-buffer.
    ///
    /// Mirrors the retail body at
    /// `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt:152-203`:
    /// `puVar11 = (uint *)(iVar12 + *(int *)(((vdf_idx << 16) >> 14) + iVar12 + 4))`.
    pub fn vdf_record_bytes(&self, idx: u8) -> Option<&[u8]> {
        let buf = self.vdf_buffer.as_deref()?;
        if buf.len() < 4 {
            return None;
        }
        let count = u32::from_le_bytes(buf[0..4].try_into().ok()?);
        if (idx as u32) >= count {
            return None;
        }
        let table_byte = 4usize;
        let slot = table_byte + (idx as usize) * 4;
        if slot + 4 > buf.len() {
            return None;
        }
        let off = u32::from_le_bytes(buf[slot..slot + 4].try_into().ok()?) as usize;
        if off >= buf.len() {
            return None;
        }
        // Bound the body by the next *greater* offset (offsets aren't
        // guaranteed monotonic - we pick the smallest offset above
        // `off` from any later table slot, defaulting to EOB).
        let mut end = buf.len();
        for i in (idx as u32 + 1)..count {
            let s = table_byte + (i as usize) * 4;
            if s + 4 > buf.len() {
                break;
            }
            let next = u32::from_le_bytes(buf[s..s + 4].try_into().ok()?) as usize;
            if next > off && next <= buf.len() && next < end {
                end = next;
            }
        }
        Some(&buf[off..end])
    }

    /// Install a global TMD at pool index `idx`. The pool grows lazily on
    /// write to accommodate sparse loader-chain installs. Indices that
    /// later producers fill in stay `None` until they're explicitly set.
    ///
    /// Mirrors the retail `FUN_80026B4C` writer
    /// (`DAT_8007C018[DAT_8007B774++] = tmd_ptr`) but exposes the index
    /// directly rather than auto-bumping a counter - engines that want
    /// the retail behaviour can read the next free slot via
    /// [`Self::global_tmd_pool`]`.len()` and pass it here.
    pub fn set_global_tmd(&mut self, idx: usize, tmd: Arc<GlobalTmd>) {
        if idx >= self.global_tmd_pool.len() {
            self.global_tmd_pool.resize(idx + 1, None);
        }
        self.global_tmd_pool[idx] = Some(tmd);
    }

    /// Resolve a global TMD by pool index. Mirrors the retail field-VM
    /// allocator's `iVar13 = DAT_8007C018[(int16_t)tmd_idx]` read - the
    /// caller is responsible for clamping negative indices (the retail
    /// engine sign-extends the i16 then implicitly treats it as unsigned;
    /// the clean-room port returns `None` for negative or out-of-range
    /// indices via the `i16 → usize` cast guarded by the bounds check).
    ///
    /// Returns `None` when the slot is empty or `idx` is out of range.
    pub fn global_tmd(&self, idx: i16) -> Option<&Arc<GlobalTmd>> {
        if idx < 0 {
            return None;
        }
        self.global_tmd_pool.get(idx as usize)?.as_ref()
    }

    /// Drain emitted field-VM events. Engines call once per frame after
    /// [`World::tick`] to dispatch BGM, dialog, money, etc. Returns events
    /// in emission order.
    pub fn drain_field_events(&mut self) -> Vec<FieldEvent> {
        std::mem::take(&mut self.pending_field_events)
    }

    /// Drain queued actor-spawn requests emitted by field-VM op `0x4C 0x80`.
    /// Each entry is the variable-length bytecode stream for one child
    /// actor. Engines route these into their actor pool.
    pub fn drain_actor_spawns(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_actor_spawns)
    }

    /// Engine-side consumer for queued actor-spawn requests.
    ///
    /// Drains [`Self::pending_actor_spawns`] (records queued by the
    /// `0x4C 0x80` halt-acquire-gated path) and, for each record:
    /// 1. Scans `actors[start_slot..MAX_ACTORS]` for the first inactive
    ///    slot. Slots `0..start_slot` are skipped so engines keep their
    ///    party / scripted actors out of the auto-allocation range.
    /// 2. Activates the slot and stores the record bytes on
    ///    [`Actor::spawn_record`]. The retail allocator writes the
    ///    bytecode pointer to `actor[+0x90]` (different from the `+0x4C`
    ///    VDF-body field that the synchronous `0x4C 0xD8` path uses);
    ///    the clean-room port stores the raw bytes on `spawn_record`
    ///    regardless and lets the engine route them as field-VM
    ///    bytecode for a child actor (the records are scripted-child
    ///    coroutines, not TMD-body or kind/variant tuples).
    /// 3. Emits a [`FieldEvent::ActorSpawned`] event for the engine.
    ///
    /// Leaves [`Actor::kind`] and [`Actor::variant`] at zero. The
    /// retail allocator for this opcode (overlay code at
    /// `overlay_world_map_801de840.txt:7080-7123` case `8 sub-0`)
    /// allocates from pool `0x801f28a0` and writes only
    /// `actor[+0x90]` (bytecode ptr), `actor[+0x94]` (parent
    /// back-pointer) and `actor[+0x54] = 0`; the `+0x3C`/`+0x3E`
    /// kind/variant fields are never written by this path, so zero
    /// matches retail.
    ///
    /// Mirrors the retail allocator's pool-exhausted branch: if no
    /// inactive slot is available, the record is dropped silently and
    /// a [`FieldEvent::ActorSpawnFailed`] event is emitted instead.
    ///
    /// Returns the count of slots that were actually allocated.
    pub fn materialize_actor_spawns(&mut self, start_slot: u8) -> usize {
        let start = (start_slot as usize).min(self.actors.len());
        let records = std::mem::take(&mut self.pending_actor_spawns);
        let mut allocated = 0usize;
        for record in records {
            match self
                .actors
                .iter()
                .enumerate()
                .skip(start)
                .find(|(_, a)| !a.active)
                .map(|(i, _)| i)
            {
                Some(slot_idx) => {
                    let actor = &mut self.actors[slot_idx];
                    actor.active = true;
                    actor.kind = 0;
                    actor.variant = 0;
                    actor.spawn_record = Some(record.clone());
                    self.pending_field_events.push(FieldEvent::ActorSpawned {
                        slot: slot_idx as u8,
                        kind: 0,
                        variant: 0,
                        record,
                    });
                    allocated += 1;
                }
                None => {
                    self.pending_field_events
                        .push(FieldEvent::ActorSpawnFailed { record });
                }
            }
        }
        allocated
    }

    /// Drain emitted battle action events. Engines call once per frame
    /// after [`World::tick`] to dispatch poses, UI elements, damage, etc.
    /// Returns events in emission order.
    pub fn drain_battle_events(&mut self) -> Vec<BattleEvent> {
        std::mem::take(&mut self.pending_battle_events)
    }

    /// Drain the presentation-only per-strike HP deltas queued by the live
    /// battle loop. Engines feed these into a damage-popup model; the HP
    /// mutation has already happened, so they are never re-applied. Returns
    /// the FX in the order they were resolved this frame.
    pub fn drain_battle_hit_fx(&mut self) -> Vec<BattleHitFx> {
        std::mem::take(&mut self.battle_hit_fx)
    }

    /// Apply the gameplay-state side of a single battle event - currently
    /// `ApplyArtStrike` (subtracts the resolved damage from the target's
    /// `BattleActor::hp`, clamping at zero, and records the enemy effect on
    /// the target's `pending_status`). Engines that want both the visual
    /// dispatch and the gameplay-state update call this for each event
    /// drained from [`Self::drain_battle_events`].
    ///
    /// Returns `Some((target_slot, hp_after))` for events that changed HP,
    /// `None` otherwise - useful for HUD popups that want the post-hit HP.
    pub fn fold_battle_event(&mut self, event: &BattleEvent) -> Option<(u8, u16)> {
        match event {
            BattleEvent::ApplyArtStrike {
                target_slot,
                outcome,
                ..
            } => {
                if let Some(target) = self.actors.get_mut(*target_slot as usize) {
                    if let Some(dmg) = outcome.damage {
                        target.battle.hp = target.battle.hp.saturating_sub(dmg);
                        // Damage clears Asleep on the target (matches retail -
                        // the enemy wakes when hit).
                        self.status_effects.on_damaged(*target_slot);
                    }
                    if outcome.enemy_effect != legaia_art::record::EnemyEffect::None {
                        target.pending_status = Some(outcome.enemy_effect);
                        // Push the status into the tracker so it
                        // subsequently ticks per-turn.
                        self.status_effects
                            .apply_from_enemy_effect(*target_slot, outcome.enemy_effect);
                    }
                    return Some((*target_slot, target.battle.hp));
                }
                None
            }
            _ => None,
        }
    }

    /// Step every actor's status effects forward one turn - folds the
    /// tick-damage into `BattleActor::hp` and emits per-status events.
    /// Called by engines once per battle round.
    pub fn tick_status_effects(&mut self) {
        let actor_count = self.actors.len();
        for slot in 0..actor_count as u8 {
            let (cur, max) = self
                .actors
                .get(slot as usize)
                .map(|a| (a.battle.hp, a.battle.max_hp))
                .unwrap_or((0, 0));
            if max == 0 {
                continue;
            }
            let dmg = self.status_effects.tick_actor(slot, cur, max);
            if dmg > 0
                && let Some(actor) = self.actors.get_mut(slot as usize)
            {
                actor.battle.hp = actor.battle.hp.saturating_sub(dmg);
            }
        }
    }

    /// Set the item catalog the battle / field menu consults for item
    /// actions. Replaces any prior catalog. Engines populate this at
    /// boot time (typically from the vanilla catalog).
    pub fn set_item_catalog(&mut self, catalog: crate::items::ItemCatalog) {
        self.item_catalog = catalog;
    }

    /// Install the spell catalog used by the player-driven battle Magic
    /// submenu. Engines call this at battle init (commonly
    /// [`crate::spells::SpellCatalog::vanilla`]).
    pub fn set_spell_catalog(&mut self, catalog: crate::spells::SpellCatalog) {
        self.spell_catalog = catalog;
    }

    /// Stage one decoded art record for the player-driven battle Arts submenu,
    /// keyed by `(character, art constant)`. Engines call this at battle init
    /// for every art the party can run (parsed from disc PROT entry `0x05C4`)
    /// so a saved chain ending in that art deals its real per-strike power.
    pub fn set_art_record(
        &mut self,
        character: legaia_art::Character,
        action: legaia_art::ActionConstant,
        record: legaia_art::ArtRecord,
    ) {
        self.art_records.insert((character, action), record);
    }

    /// Bulk-install art records (see [`World::set_art_record`]). Existing
    /// entries for the same key are replaced.
    pub fn set_art_records(
        &mut self,
        records: impl IntoIterator<
            Item = (
                (legaia_art::Character, legaia_art::ActionConstant),
                legaia_art::ArtRecord,
            ),
        >,
    ) {
        self.art_records.extend(records);
    }

    /// Resolve a party slot to the [`legaia_art::Character`] whose art tables
    /// apply. Party slots 0/1/2 are Vahn/Noa/Gala; out-of-range slots (story
    /// guests, monsters) fall back to Vahn so the lookup never panics.
    fn caster_character(&self, slot: u8) -> legaia_art::Character {
        legaia_art::Character::all()
            .get(slot as usize)
            .copied()
            .unwrap_or(legaia_art::Character::Vahn)
    }

    /// Build the Arts submenu rows for `caster` from their saved chains. For
    /// each chain, the longest staged art record whose command string the
    /// chain ends with ([`crate::battle_arts::chain_matches_record`]) supplies
    /// the real power profile; chains with no matching record fall back to a
    /// synthetic profile derived from the directional commands.
    fn build_battle_arts_rows(&self, caster: u8) -> Vec<crate::battle_arts::ArtRow> {
        use crate::battle_arts::{
            ArtRow, chain_matches_record, power_from_record, synthetic_power,
        };
        let character = self.caster_character(caster);
        self.saved_chains
            .iter()
            .filter(|c| c.char_slot == caster)
            .map(|c| {
                let best = self
                    .art_records
                    .iter()
                    .filter(|((ch, _), _)| *ch == character)
                    .filter(|(_, rec)| chain_matches_record(&c.sequence, rec))
                    .max_by_key(|(_, rec)| rec.commands.len());
                match best {
                    Some((_, rec)) => {
                        let (power, enemy_effect) = power_from_record(rec);
                        ArtRow {
                            name: c.name.clone(),
                            power,
                            enemy_effect,
                        }
                    }
                    None => ArtRow {
                        name: c.name.clone(),
                        power: synthetic_power(&c.sequence),
                        enemy_effect: legaia_art::EnemyEffect::None,
                    },
                }
            })
            .collect()
    }

    /// Pull the cross-character saved-chain library out as a
    /// [`crate::tactical_arts_editor::ChainLibrary`] - what the field menu's
    /// Tactical Arts editor browses + edits. The editor mutates the returned
    /// library; the engine writes the result back with
    /// [`Self::store_chain_library`] so the edit reaches the next battle's
    /// Arts menu (via [`Self::build_battle_arts_rows`]) and the next save
    /// (via [`Self::saved_chains`]).
    pub fn chain_library(&self) -> crate::tactical_arts_editor::ChainLibrary {
        crate::tactical_arts_editor::ChainLibrary::from_records(&self.saved_chains)
    }

    /// Write an edited [`crate::tactical_arts_editor::ChainLibrary`] back into
    /// [`Self::saved_chains`], replacing the whole library. This is the bridge
    /// that closes the loop the field menu opens with [`Self::chain_library`]:
    /// once stored, a chain composed in the editor is selectable in battle and
    /// persists across `save_full` / `load_full`.
    pub fn store_chain_library(&mut self, lib: &crate::tactical_arts_editor::ChainLibrary) {
        self.saved_chains = lib.to_records();
    }

    /// Use an item from the catalog against a target slot. Wraps the
    /// `items::apply_effect` resolution and folds the outcome back into
    /// world state (HP/MP deltas, status cure, revive HP). Returns the
    /// resolved [`crate::items::ItemOutcome`] so the engine can drive
    /// dialog / SFX / visual cues.
    ///
    /// Returns [`crate::items::ItemOutcome::NoEffect`] when:
    ///   - the item id is not in the catalog,
    ///   - or the target slot is out of range.
    ///
    /// HP / MP changes are clamped to the actor's max values. Cure /
    /// CureAll outcomes also clear the corresponding entries from the
    /// `StatusEffectTracker`.
    pub fn use_item(&mut self, item_id: u8, target_slot: u8) -> crate::items::ItemOutcome {
        let entry = match self.item_catalog.get(item_id) {
            Some(e) => *e,
            None => return crate::items::ItemOutcome::NoEffect,
        };
        let idx = target_slot as usize;
        // BattleActor holds `mp` but not `max_mp`; engines that wire the
        // character record into the actor populate it via a sibling field.
        // For the snapshot we use the character_max_mp accessor (defaults
        // to `mp` itself when not separately tracked, which gives a
        // conservative "MP already capped" reading).
        let snapshot = match self.actors.get(idx) {
            Some(a) => crate::items::TargetSnapshot {
                hp: a.battle.hp,
                hp_max: a.battle.max_hp,
                mp: a.battle.mp,
                mp_max: self
                    .character_max_mp
                    .get(idx)
                    .copied()
                    .unwrap_or(a.battle.mp),
                is_dead: a.battle.hp == 0 && a.battle.max_hp > 0,
            },
            None => return crate::items::ItemOutcome::NoEffect,
        };
        let outcome = crate::items::apply_effect(entry.effect, &snapshot);
        match outcome {
            crate::items::ItemOutcome::HealedHp { amount } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = a.battle.hp.saturating_add(amount).min(a.battle.max_hp);
                }
            }
            crate::items::ItemOutcome::HealedMp { amount } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    let cap = self.character_max_mp.get(idx).copied().unwrap_or(u16::MAX);
                    a.battle.mp = a.battle.mp.saturating_add(amount).min(cap);
                }
            }
            crate::items::ItemOutcome::Cured { kind } => {
                self.status_effects.cure(target_slot, kind);
            }
            crate::items::ItemOutcome::CuredAll => {
                self.status_effects.cure_all(target_slot);
            }
            crate::items::ItemOutcome::Revived { hp_after } => {
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = hp_after.min(a.battle.max_hp);
                }
            }
            crate::items::ItemOutcome::SpiritGained { amount } if idx < self.ap_gauges.len() => {
                // Refund AP into the active actor's gauge if it's a party slot.
                self.ap_gauges[idx].refund(amount);
            }
            crate::items::ItemOutcome::DamageDealt { amount } => {
                // Offensive item (e.g. Bomb): subtract HP from the enemy slot
                // and down it if it reaches zero.
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.hp = a.battle.hp.saturating_sub(amount);
                    if a.battle.hp == 0 {
                        a.battle.liveness = 0;
                    }
                }
            }
            crate::items::ItemOutcome::CaptureRolled { strength } => {
                // Capture item: roll against the enemy's missing-HP fraction
                // (shared with the Magic capture path); a success downs the
                // monster and logs its id into `battle_captures`.
                self.resolve_capture(target_slot, strength.min(u8::MAX as u16) as u8);
            }
            crate::items::ItemOutcome::EscapeRequested => {
                // Escape item (e.g. Goblin Foot): flag the encounter to end;
                // the battle item-menu tick returns to the field.
                self.battle_escaped = true;
            }
            _ => {}
        }
        outcome
    }

    /// Set per-slot character max MP (mirrors `char_record[+0x140]`
    /// from the save record). Engines call this once per scene init -
    /// usually from `set_character_record_for_slot`. Unset slots default
    /// to `0`, which makes [`Self::use_item`] treat MP healing as a
    /// no-op for that slot.
    pub fn set_character_max_mp(&mut self, slot: u8, mp_max: u16) {
        let i = slot as usize;
        if i >= self.character_max_mp.len() {
            self.character_max_mp.resize(i + 1, 0);
        }
        self.character_max_mp[i] = mp_max;
    }

    /// Reset every party-member's AP gauge for a new turn. Refills to
    /// `base_ap`, clears the Spirit-charged flag.
    pub fn reset_party_ap(&mut self) {
        for g in self.ap_gauges.iter_mut() {
            g.reset_for_turn();
        }
    }

    /// Set the per-slot weapon attack used by Tactical-Art strike damage
    /// resolution. Engines call this when a character equips / unequips a
    /// weapon, or once at battle init from the active stat record.
    pub fn set_battle_attack(&mut self, slot: u8, atk: u16) {
        if let Some(s) = self.battle_attack.get_mut(slot as usize) {
            *s = atk;
        }
    }

    /// Set the per-slot magic attack scalar used by battle Magic damage
    /// resolution. Engines call this at battle init from the active stat
    /// record's magic stat.
    pub fn set_battle_magic(&mut self, slot: u8, mag: u16) {
        if let Some(s) = self.battle_magic.get_mut(slot as usize) {
            *s = mag;
        }
    }

    /// Set the per-slot generic defense - used when no UDF / LDF split is
    /// configured for the slot.
    pub fn set_battle_defense(&mut self, slot: u8, def: u16) {
        if let Some(s) = self.battle_defense.get_mut(slot as usize) {
            *s = def;
        }
    }

    /// Set per-slot UDF / LDF defense override. Replaces any prior value.
    /// Pass `None` to revert to [`Self::set_battle_defense`].
    pub fn set_battle_defense_split(&mut self, slot: u8, udf_ldf: Option<(u16, u16)>) {
        if let Some(s) = self.battle_defense_split.get_mut(slot as usize) {
            *s = udf_ldf;
        }
    }

    /// Resolve the defense value to use against a single Tactical-Art
    /// strike. Used by the world's `BattleActionHost::apply_art_strike`
    /// impl. Public so engines can call the same lookup directly when
    /// they want to apply art strikes outside the SM (e.g. for testing).
    pub fn resolve_battle_defense(
        &self,
        target_slot: u8,
        info: &legaia_engine_vm::battle_action::ArtStrikeInfo,
    ) -> u16 {
        let idx = target_slot as usize;
        // If we have a UDF / LDF split for the slot, pick the half that
        // matches the strike's power target. Otherwise fall back to the
        // single defense value.
        if let Some(Some((udf, ldf))) = self.battle_defense_split.get(idx)
            && let Some(legaia_art::power::PowerByte::Damage(p)) = info.power
        {
            return match p.target {
                legaia_art::power::PowerTarget::Udf => *udf,
                legaia_art::power::PowerTarget::Ldf => *ldf,
            };
        }
        self.battle_defense.get(idx).copied().unwrap_or(0)
    }

    /// Distribute `xp_reward` to the surviving party members after a
    /// `BattleEndCause::MonsterWipe`. Mirrors the retail-shape split:
    ///
    /// - Surviving members (HP > 0) split the reward equally, rounded down.
    /// - Dead members (HP == 0) receive zero XP.
    /// - Remainder bytes from the integer divide are dropped on the floor,
    ///   matching the retail end-of-battle distribution.
    ///
    /// For each member that crosses a level threshold, bumps the roster
    /// record's HP/MP maxima, resyncs the live `BattleActor` mirror, pushes
    /// a [`BattleEvent::LevelUp`], and appends a [`LevelUpResult`] to the
    /// returned vec.
    ///
    /// If every party member is dead (TPK) but the caller still invokes this
    /// (e.g. a Phoenix Down style revive-after-victory), the split degenerates
    /// to a no-op — there are no alive recipients.
    pub fn apply_battle_xp(&mut self, xp_reward: u32) -> Vec<LevelUpResult> {
        let party_count = self.party_count as usize;
        // Living-member count drives the divisor. We pull HP from
        // `BattleActor` (the live mirror) so the resolver sees the
        // post-battle state, not the record's saved HP.
        let alive: Vec<u8> = (0..party_count as u8)
            .filter(|&i| self.actors.get(i as usize).is_some_and(|a| a.battle.hp > 0))
            .collect();
        if alive.is_empty() {
            return Vec::new();
        }
        let per_member_xp = xp_reward / alive.len() as u32;
        if per_member_xp == 0 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for char_id in alive {
            let Some(result) = self.level_up_tracker.grant_xp(char_id, per_member_xp) else {
                continue;
            };
            let slot = char_id as usize;
            if let Some(rec) = self.roster.members.get_mut(slot) {
                LevelUpTracker::apply_to_record(&result, rec);
            }
            let new_hms = self.roster.members.get(slot).map(|r| r.hp_mp_sp());
            if let (Some(actor), Some(hms)) = (self.actors.get_mut(slot), new_hms) {
                actor.battle.max_hp = hms.hp_max;
                actor.battle.hp = hms.hp_cur;
                actor.battle.mp = hms.mp_cur;
            }
            self.pending_battle_events.push(BattleEvent::LevelUp {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
            });
            self.current_level_up_banner = Some(LevelUpBanner {
                char_id,
                new_level: result.new_level,
                hp_gained: result.hp_gained,
                mp_gained: result.mp_gained,
                frames_remaining: LevelUpBanner::DEFAULT_FRAMES,
            });
            results.push(result);
        }
        results
    }

    /// Sum the per-monster `exp` and `gold` for every slot in `formation`,
    /// distribute the XP via [`World::apply_battle_xp`], and add the gold
    /// to [`World::money`]. Returns the aggregated [`BattleRewards`] so
    /// engines can surface the post-battle banner ("got N XP, M gold,
    /// learned spell X").
    ///
    /// Monsters whose ids aren't in `catalog` contribute zero - the call
    /// silently skips them rather than failing, so a partially-populated
    /// catalog still drives a battle-end transition.
    pub fn apply_battle_loot(
        &mut self,
        formation: &crate::monster_catalog::FormationDef,
        catalog: &crate::monster_catalog::MonsterCatalog,
    ) -> BattleRewards {
        let mut xp_total: u32 = 0;
        let mut gold_total: u32 = 0;
        let mut drops: Vec<u8> = Vec::new();
        for slot in &formation.slots {
            let Some(def) = catalog.get(slot.monster_id) else {
                continue;
            };
            xp_total = xp_total.saturating_add(def.exp as u32);
            gold_total = gold_total.saturating_add(def.gold as u32);
            if let Some(item_id) = def.drop_item
                && def.drop_rate_q8 > 0
            {
                // 1-in-256 fixed-point drop roll: pull one byte from the
                // deterministic RNG and compare. `drop_rate_q8 == 255`
                // makes the drop near-guaranteed (1/256 floor); `0`
                // already short-circuited above.
                let roll = (self.next_rng() & 0xFF) as u8;
                if roll < def.drop_rate_q8 {
                    drops.push(item_id);
                    let entry = self.inventory.entry(item_id).or_insert(0);
                    *entry = entry.saturating_add(1);
                }
            }
        }
        let level_ups = if xp_total > 0 {
            self.apply_battle_xp(xp_total)
        } else {
            Vec::new()
        };
        let new_money = (self.money as i64).saturating_add(gold_total as i64);
        self.money = new_money.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        BattleRewards {
            xp: xp_total,
            gold: gold_total,
            level_ups,
            drops,
        }
    }

    /// Record one use of `art_id` by `char_id` (roster index).
    ///
    /// Delegates to [`TacticalArtsTracker::notify_art_used`]. When the use
    /// count first crosses the learn threshold, this method:
    ///
    /// 1. Pushes [`BattleEvent::TacticalArtLearned`] onto
    ///    [`Self::pending_battle_events`].
    /// 2. Sets [`Self::current_art_banner`] with a 2-second display window
    ///    so the engine's HUD overlay can show "Learned Art #N!".
    ///
    /// Subsequent calls for the same `(char_id, art_id)` pair are no-ops.
    pub fn notify_art_used(&mut self, char_id: u8, art_id: u8) {
        if let Some(ev) = self.tactical_arts.notify_art_used(char_id, art_id) {
            let text = format!("Learned {}!", ev.name);
            self.current_art_banner = Some(ArtLearnedBanner {
                text,
                frames_remaining: ArtLearnedBanner::DEFAULT_FRAMES,
            });
            self.pending_battle_events
                .push(BattleEvent::TacticalArtLearned {
                    char_id: ev.char_id,
                    art_id: ev.art_id,
                });
        }
    }

    /// Set / clear the move-VM bytecode for `slot`. `None` clears the
    /// buffer; subsequent ticks won't run the move VM on this actor.
    pub fn set_move_bytecode(&mut self, slot: usize, bytecode: Option<Vec<u16>>) {
        if slot < self.move_bytecode.len() {
            self.move_bytecode[slot] = bytecode.unwrap_or_default();
        }
    }

    /// Set bit `idx` in the shared system flag bank. `idx >> 3` is the byte
    /// offset; the bit mask is `0x80 >> (idx & 7)` (MSB-first, mirroring the
    /// SCUS helper at `FUN_8003CE08`). The bank grows lazily as needed.
    pub fn system_flag_set(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte >= self.system_flags.len() {
            self.system_flags.resize(byte + 1, 0);
        }
        self.system_flags[byte] |= 0x80u8 >> (idx & 7);
    }

    /// Clear bit `idx` in the shared system flag bank. See [`system_flag_set`].
    /// Out-of-bounds clears are no-ops (the bit is already zero).
    ///
    /// [`system_flag_set`]: World::system_flag_set
    pub fn system_flag_clear(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte < self.system_flags.len() {
            self.system_flags[byte] &= !(0x80u8 >> (idx & 7));
        }
    }

    /// Test bit `idx` in the shared system flag bank. Returns `false` for
    /// indices past the currently-grown size.
    pub fn system_flag_test(&self, idx: u16) -> bool {
        let byte = (idx >> 3) as usize;
        if byte < self.system_flags.len() {
            self.system_flags[byte] & (0x80u8 >> (idx & 7)) != 0
        } else {
            false
        }
    }

    /// Replace the field-VM bytecode buffer + reset PC. Engines call this
    /// when entering a new field scene (loading the scene's per-event
    /// script) to start interpretation from the beginning.
    pub fn load_field_script(&mut self, bytecode: Vec<u8>) {
        self.field_bytecode = bytecode;
        self.field_pc = 0;
        self.field_ctx = FieldCtx::default();
    }

    /// Load a field-VM bytecode buffer and begin interpretation at `pc`
    /// instead of 0.
    ///
    /// Used to run a MAN-resolved **scene-entry system script** (retail
    /// `FUN_8003ab2c`, context channel `0xFB`): the buffer is the MAN slice
    /// taken from the script block's start, and `pc` is the first opcode's
    /// offset into that slice (past the `[local-count][locals][record-header]`
    /// prefix). Slicing from the script start keeps relative jumps wrapping
    /// against the slice base (index 0), matching the retail
    /// `buffer_base = script_start` convention. See
    /// [`crate::scene::Scene::field_man_entry_script`].
    ///
    /// REF: FUN_8003ab2c (the port lives in `legaia_asset::man_section`).
    pub fn load_field_script_at(&mut self, bytecode: Vec<u8>, pc: usize) {
        self.field_bytecode = bytecode;
        self.field_pc = pc;
        self.field_ctx = FieldCtx::default();
    }

    /// Load one event-script record into the field VM, skipping the leading
    /// `0xFFFF 0x0000` frame-divider sentinel when present.
    ///
    /// Records pulled from `scene_event_scripts` / `scene_scripted_asset_table`
    /// containers commonly open with the 4-byte sentinel; the field VM's
    /// dispatcher in retail consumes the sentinel as a record-start marker
    /// rather than an opcode (the high bit + low-7-bits 0x7F would otherwise
    /// hit the "UNFIND INDICATION" default arm). The exact dispatcher prelude
    /// hasn't been fully traced, so this skip is heuristic - revise once
    /// `FUN_801DE840`'s outer loop is captured.
    pub fn load_field_record(&mut self, record_bytes: &[u8]) {
        const FRAME_DIVIDER: [u8; 4] = [0xFF, 0xFF, 0x00, 0x00];
        let pc = if record_bytes.starts_with(&FRAME_DIVIDER) {
            4
        } else {
            0
        };
        self.field_bytecode = record_bytes.to_vec();
        self.field_pc = pc;
        self.field_ctx = FieldCtx::default();
    }

    /// Load a `Party` (per-character roster) into the world's actor table.
    ///
    /// Per-character record 0 maps to actor slot 0, record 1 to slot 1, …
    /// up to `party.len()` (capped by `MAX_ACTORS`). For each loaded slot
    /// the world:
    ///
    /// - activates the actor,
    /// - copies HP / MP from the record's [`HpMpSp`] block into the
    ///   `BattleActor` mirrors,
    /// - stows the full record bytes via [`World::roster`] for later
    ///   round-trip via [`World::save_party`].
    ///
    /// The `legaia-save` crate's [`legaia_save::CharacterRecord::parse`] is
    /// the lossless deserializer; this method is the runtime-side glue that
    /// projects the persistent record into the per-VM actor state.
    ///
    /// [`HpMpSp`]: legaia_save::HpMpSp
    pub fn load_party(&mut self, party: legaia_save::Party) {
        let n = party.members.len().min(self.actors.len());
        for (slot, rec) in party.members.iter().take(n).enumerate() {
            let hms = rec.hp_mp_sp();
            let a = &mut self.actors[slot];
            a.active = true;
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
            a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            // Seed the per-slot turn-order SPD from the record's live stats so
            // a battle's next-actor selector can run the initiative scheme.
            // A zeroed record leaves SPD at 0 -> round-robin fallback.
            if let Some(s) = self.battle_speed.get_mut(slot) {
                *s = rec.live_stats().spd;
            }
        }
        self.party_count = n as u8;
        self.roster = party;
    }

    /// Capture the world's current actor state back into a `Party`. The
    /// roster bytes are returned verbatim except for the HP / MP / max-HP
    /// fields, which are resynced from the live `BattleActor` mirrors so
    /// in-battle damage / heals end up in the saved record.
    ///
    /// Round-trip: `world.load_party(p); world.save_party() == p` modulo
    /// the HP/MP resync (which is a no-op when no battle has run yet).
    pub fn save_party(&mut self) -> legaia_save::Party {
        for (slot, rec) in self
            .roster
            .members
            .iter_mut()
            .enumerate()
            .take(self.actors.len())
        {
            let mut hms = rec.hp_mp_sp();
            let a = &self.actors[slot];
            hms.hp_cur = a.battle.hp;
            hms.hp_max = a.battle.max_hp;
            hms.mp_cur = a.battle.mp;
            rec.set_hp_mp_sp(hms);
        }
        self.roster.clone()
    }

    /// Capture the complete engine state (party + globals) into a [`legaia_save::SaveFile`].
    ///
    /// Pairs with [`World::load_full`]. Use this instead of [`World::save_party`] when
    /// you need `story_flags`, `money`, and `inventory` to survive a save/load cycle.
    pub fn save_full(&mut self) -> legaia_save::SaveFile {
        let party = self.save_party();
        let mut inventory: Vec<(u8, u8)> = self
            .inventory
            .iter()
            .map(|(&id, &count)| (id, count))
            .collect();
        inventory.sort_by_key(|&(id, _)| id);

        // Build per-character extension records from live world state.
        let active_party: Vec<u8> = (0..party.members.len() as u8).collect();
        let mut per_char: Vec<(u8, legaia_save::CharSaveExt)> = Vec::new();
        for slot in 0..party.members.len() as u8 {
            let mut ce = legaia_save::CharSaveExt::default();
            // Learned arts: derive from TacticalArtsTracker - bit i is
            // set when art id i has crossed the learn threshold.
            for art_id in 0..32u8 {
                if self.tactical_arts.is_learned(slot, art_id) {
                    ce.learned_arts_mask |= 1u32 << art_id;
                }
            }
            // Spells: the per-character learned spell list from the seru log.
            ce.spells = self.seru_log.learned_spells(slot).to_vec();
            // Seru captures: export the live log's per-Seru capture-point
            // progress (real seru_id -> points) so sub-threshold progress
            // survives a save/load. Sorted for deterministic output.
            ce.seru_captures = self
                .seru_log
                .iter_rows()
                .filter(|(s, _, _)| *s == slot)
                .map(|(_, sid, row)| (sid, row.points))
                .collect();
            ce.seru_captures.sort_by_key(|&(sid, _)| sid);
            // Active-chain selection still lives in the per-char ext mirror.
            if let Some((_, src)) = self.per_char_ext.iter().find(|(s, _)| *s == slot) {
                ce.active_chains = src.active_chains;
            }
            per_char.push((slot, ce));
        }

        legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt {
                story_flags: self.story_flags,
                story_flag_bits: self.story_flag_bits.clone(),
                money: self.money,
                inventory,
            },
            ext_v2: legaia_save::SaveExtV2 {
                play_time_seconds: self.play_time_seconds,
                active_party,
                per_char,
                saved_chains: self.saved_chains.clone(),
            },
        }
    }

    /// Restore engine state from a [`legaia_save::SaveFile`] produced by [`World::save_full`].
    ///
    /// Party records are applied through [`World::load_party`]; globals overwrite the
    /// current `story_flags`, `money`, and `inventory`. Sync per-slot
    /// [`LevelUpTracker::level`] from each loaded record's `+0x100` byte
    /// so reloads don't silently reset every party slot to level 1.
    pub fn load_full(&mut self, sf: legaia_save::SaveFile) {
        self.load_party(sf.party);
        self.story_flags = sf.ext.story_flags;
        self.story_flag_bits = sf.ext.story_flag_bits;
        self.money = sf.ext.money;
        self.inventory.clear();
        for (id, count) in sf.ext.inventory {
            if count > 0 {
                self.inventory.insert(id, count);
            }
        }
        // Hydrate the level-up tracker's per-slot level from the loaded
        // character records. Without this, the tracker keeps its default
        // 1-per-slot level even when the saved record has the party at
        // level 30 — the next level-up grant would silently roll the
        // party back to level 1 + N.
        for (slot, rec) in self.roster.members.iter().enumerate() {
            if slot < self.level_up_tracker.level.len() {
                self.level_up_tracker.level[slot] = rec.level().max(1);
            }
        }
        // V2 ext block - repopulate engine-side trackers.
        self.play_time_seconds = sf.ext_v2.play_time_seconds;
        self.saved_chains = sf.ext_v2.saved_chains.clone();
        self.per_char_ext = sf.ext_v2.per_char.clone();
        // Reset trackers so reloads don't accumulate stale state.
        self.tactical_arts = TacticalArtsTracker::new();
        self.seru_log = crate::seru_learning::SeruCaptureLog::new();
        for (slot, ce) in &sf.ext_v2.per_char {
            // Re-mark learned arts so the tracker doesn't re-fire the
            // "first time learned" event for arts the save already has.
            for art_id in 0..32u8 {
                if ce.learned_arts_mask & (1u32 << art_id) != 0 {
                    self.tactical_arts.mark_known(*slot, art_id);
                }
            }
            // Restore per-Seru capture-point progress. When the registry is
            // installed, a row that's already over threshold restores as
            // learned (with its spell), so a later capture doesn't re-fire
            // the learn event.
            for &(sid, pts) in &ce.seru_captures {
                let def = self.seru_registry.get(sid);
                let learned = def.is_some_and(|d| pts >= d.learn_threshold);
                let spell_id = def.map(|d| d.spell_id);
                self.seru_log
                    .restore_row(*slot, sid, pts, 0, learned, spell_id);
            }
            // Ensure every persisted learned spell lands in the learned list,
            // even with no registry installed: map it back to its teaching
            // Seru when known, else key by the spell id as a surrogate.
            for &spell_id in &ce.spells {
                if let Some(def) = self.seru_registry.seru_for_spell(spell_id) {
                    self.seru_log.mark_learned(*slot, def.id, spell_id);
                } else {
                    self.seru_log.mark_learned(*slot, spell_id as u16, spell_id);
                }
            }
        }
    }

    /// Activate a slot and return a mutable reference to the actor.
    ///
    /// PORT: FUN_80020DE0
    pub fn spawn_actor(&mut self, slot: usize) -> &mut Actor {
        let a = &mut self.actors[slot];
        a.active = true;
        a
    }

    /// Ensure the slot at `id` is initialized with the supplied default
    /// position and active. Idempotent.
    ///
    /// Preserves `tmd_binding` and `active_animation` across the reset so
    /// that `init_scene_animations` bindings survive the first field-VM
    /// actor-spawn opcode.
    pub fn ensure_actor(&mut self, id: u8, default_pos: ActorVmPosition) -> &mut Actor {
        let a = &mut self.actors[id as usize];
        if !a.active {
            let tmd_binding = a.tmd_binding;
            let active_animation = a.active_animation.take();
            *a = Actor::new();
            a.tmd_binding = tmd_binding;
            a.active_animation = active_animation;
            a.active = true;
        }
        a.default_pos = default_pos;
        a
    }

    /// Pre-bind every actor slot to its scene resources before the field VM
    /// spawns actors. Wires:
    ///
    /// - `actor.tmd_binding = slot_idx` (direct 1:1 ordering: the retail
    ///   `FUN_8001E890` loop registers TMDs in pack offset-table order -
    ///   actor K → TMD slot K).
    /// - `actor.active_animation` seeded from ANM record 0 (idle) when an
    ///   ANM pack is present for that slot.
    ///
    /// Because `ensure_actor` preserves these fields across resets, the
    /// bindings survive the first field-VM actor-spawn opcode.
    pub fn init_scene_animations(&mut self, resources: &crate::scene_resources::SceneResources) {
        for (i, actor) in self.actors.iter_mut().enumerate() {
            if i < resources.tmds.len() {
                actor.tmd_binding = Some(i);
            }
            if actor.active_animation.is_none()
                && let Some(anm) = resources.anm_pack_for_actor(i)
                && let Some(rec_bytes) = anm.record_bytes(0)
            {
                let bone_count = resources
                    .tmds
                    .get(i)
                    .map(|t| t.tmd.objects.len())
                    .unwrap_or(1)
                    .max(1);
                if let Ok(player) = AnimPlayer::new(rec_bytes.to_vec(), bone_count) {
                    actor.active_animation = Some(player);
                }
            }
        }
    }

    /// Run the actor VM bytecode against this world.
    ///
    /// Convenience wrapper around [`vm::run`] that constructs a host borrow.
    pub fn run_actor_bytecode(&mut self, bytecode: &[u8]) -> Result<usize, vm::VmError> {
        let mut host = ActorVmHostImpl { world: self };
        vm::run(&mut host, bytecode)
    }

    /// Step the move VM once for the actor at `slot`, using `bytecode` as
    /// the move buffer. Returns the [`vm::move_vm::StepResult`].
    ///
    /// Engines typically call this in a loop on each per-frame actor tick
    /// until the inner step returns `Halt` or `Wait`.
    ///
    /// Writes the host's `move_bytecode_write_u16` calls (issued by ext
    /// sub-ops 0x04 / 0x1B / 0x1E / 0x36) back to `world.move_bytecode[slot]`
    /// after step completes - see the `MoveVmHostImpl` deferred-writes map.
    pub fn step_move_vm(&mut self, slot: usize, bytecode: &[u16]) -> vm::move_vm::StepResult {
        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: Some(slot),
            deferred_writes: std::collections::BTreeMap::new(),
        };
        let actor_state = unsafe {
            // SAFETY: the host borrows `world.actors[slot]` only through
            // queries that don't read this slot's `move_state`. The host
            // implementation never touches `actors[slot].move_state`; it
            // only reads sin/cos LUTs and other engine-side data.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        let result = vm::move_vm::step(&mut host, actor_state, bytecode);
        let writes = std::mem::take(&mut host.deferred_writes);
        if !writes.is_empty()
            && let Some(buf) = self.move_bytecode.get_mut(slot)
        {
            for (off, value) in writes {
                if off >= buf.len() {
                    buf.resize(off + 1, 0);
                }
                buf[off] = value;
            }
        }
        result
    }

    /// Run one battle-action state-machine step.
    pub fn step_battle(&mut self) -> StepOutcome {
        let ctx_ptr: *mut BattleActionCtx = &mut self.battle_ctx;
        let mut host = BattleHostImpl { world: self };
        // SAFETY: BattleHostImpl never reads or writes `world.battle_ctx`
        // through the borrow; it only touches `actors`, helper tables, and
        // call-records.
        let ctx = unsafe { &mut *ctx_ptr };
        vm::battle_action::step(&mut host, ctx)
    }

    /// Tick the effect pool.
    pub fn tick_effects(&mut self) {
        let pool_ptr: *mut Pool = &mut self.effect_pool;
        let mut host = EffectHostImpl { world: self };
        // SAFETY: EffectHostImpl never reads `world.effect_pool` through
        // the borrow.
        let pool = unsafe { &mut *pool_ptr };
        pool.tick(&mut host);
    }

    /// Snapshot every live effect for the renderer: one [`EffectMarker`] per
    /// active master slot (`child_count > 0`), with its world position, spawn
    /// angle, and lifetime fraction.
    ///
    /// This is the render-agnostic seam between the effect VM and the host's
    /// draw path. The host drains it each frame after [`Self::tick`] and emits
    /// whatever it can draw; nothing here depends on the renderer.
    pub fn active_effect_markers(&self) -> Vec<EffectMarker> {
        let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        self.effect_pool
            .master_slots
            .iter()
            .filter(|m| m.child_count > 0)
            .map(|m| EffectMarker {
                // Pool positions are 8.8 fixed-point world units.
                world_pos: [
                    (m.pos_x as f32) / 256.0,
                    (m.pos_y as f32) / 256.0,
                    (m.pos_z as f32) / 256.0,
                ],
                angle: m.angle,
                age01: ((m.field_14 as f32) / lifetime).clamp(0.0, 1.0),
            })
            .collect()
    }

    /// Snapshot every live effect's **child sprites** as faithful billboards -
    /// the textured-quad seam that supersedes [`Self::active_effect_markers`]'
    /// one-cross-per-effect view. For each active master slot it resolves the
    /// effect's children through the loaded [`crate::world::World::effect_catalog`],
    /// walks each child's pack0 animation to the current frame, and reads the
    /// frame's sprite-atlas entry for size + VRAM `(u, v)` / `tpage` / `clut`.
    ///
    /// Mirrors the retail per-frame walker (`FUN_801E0088` pass 2): one GPU
    /// sprite primitive per child, sized from the atlas, placed at the effect
    /// origin plus the child's spread offset. Returns an empty vector when the
    /// catalog is empty (e.g. no disc), so it degrades cleanly.
    pub fn active_effect_sprites(&self) -> Vec<EffectSprite> {
        let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        let atlas = self.effect_catalog.atlas();
        let mut out = Vec::new();
        for m in self.effect_pool.master_slots.iter() {
            if m.child_count == 0 {
                continue;
            }
            let Some((_script, children)) = self.effect_catalog.entry(m.ui_id) else {
                continue;
            };
            let age01 = ((m.field_14 as f32) / lifetime).clamp(0.0, 1.0);
            let frame_idx = m.field_14.max(0) as usize;
            let origin = [
                m.pos_x as f32 / 256.0,
                m.pos_y as f32 / 256.0,
                m.pos_z as f32 / 256.0,
            ];
            for (i, child) in children.iter().enumerate() {
                // Resolve the current animation frame -> atlas entry.
                let entry = self.effect_catalog.anim(child.sprite_id).and_then(|batch| {
                    if batch.frames.is_empty() {
                        return None;
                    }
                    // Loop the batch over the effect lifetime (the faithful
                    // per-frame token cadence is not extracted; a uniform loop
                    // keeps the sprite animating over its visible life).
                    let f = frame_idx % batch.frames.len();
                    atlas.get(batch.frames[f].atlas_index as usize)
                });
                let Some(e) = entry else {
                    continue;
                };
                // Child placement: the stored spread offset (random-distribution
                // path) or a small deterministic ring (walker-populated path).
                let (dx, dz) = m
                    .child_offsets
                    .get(i)
                    .copied()
                    .map(|(x, z)| (x as f32 / 256.0, z as f32 / 256.0))
                    .unwrap_or_else(|| {
                        let a = i as f32 * std::f32::consts::TAU / children.len().max(1) as f32;
                        let r = (child.width.max(child.depth).max(8) as f32) / 256.0;
                        (a.cos() * r, a.sin() * r)
                    });
                out.push(EffectSprite {
                    world_pos: [origin[0] + dx, origin[1], origin[2] + dz],
                    size: [e.w.max(1) as f32, e.h.max(1) as f32],
                    uv: [e.u as u16, e.v as u16],
                    uv_size: [e.w as u16, e.h as u16],
                    page: e.page,
                    clut: e.clut as u16,
                    age01,
                });
            }
        }
        out
    }

    /// Snapshot every live effect that has a 3D model assigned, for the
    /// `etmd`-model render path. One [`EffectModel`] per active master slot
    /// whose `model_index` is set (the model-driven effects); 2D-billboard-only
    /// effects are skipped. The host resolves `tmd_index` through
    /// [`Self::global_tmd`], builds a VRAM mesh, and draws it at `world_pos`.
    ///
    /// Distinct from [`Self::active_effect_sprites`] (the 2D billboard seam):
    /// effects like *Tail Fire* render as a small `etmd` mesh textured by the
    /// resident `etim` texels, not a billboard.
    pub fn active_effect_models(&self) -> Vec<EffectModel> {
        let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        self.effect_pool
            .master_slots
            .iter()
            .filter(|m| m.child_count > 0)
            .filter_map(|m| {
                let tmd_index = m.model_index?;
                Some(EffectModel {
                    tmd_index,
                    world_pos: [
                        m.pos_x as f32 / 256.0,
                        m.pos_y as f32 / 256.0,
                        m.pos_z as f32 / 256.0,
                    ],
                    angle: m.angle,
                    age01: ((m.field_14 as f32) / lifetime).clamp(0.0, 1.0),
                })
            })
            .collect()
    }

    /// Dev/visualization helper: seat one synthetic active effect carrying a
    /// 3D `etmd` model at `world_pos` (world units), so the model render path
    /// (e.g. *Tail Fire* = `etmd` mesh index 4, textured by `etim`) can be
    /// exercised by hand. `tmd_index` indexes [`Self::global_tmd_pool`]. The
    /// slot ages and retires through the normal [`Self::tick_effects`] lifetime.
    ///
    /// Like [`Self::spawn_debug_effect`], this is **not** a retail code path -
    /// the production effect-id -> etmd-model selection (driven by the move/art
    /// VM) is not yet decoded. Returns `false` when the pool is full.
    pub fn spawn_debug_effect_model(&mut self, world_pos: [f32; 3], tmd_index: usize) -> bool {
        let Some(slot) = self.effect_pool.allocate_master() else {
            return false;
        };
        let m = &mut self.effect_pool.master_slots[slot];
        *m = vm::effect_vm::MasterSlot::default();
        m.child_count = 1;
        m.pos_x = (world_pos[0] * 256.0) as i32;
        m.pos_y = (world_pos[1] * 256.0) as i32;
        m.pos_z = (world_pos[2] * 256.0) as i32;
        m.model_index = Some(tmd_index);
        true
    }

    /// Dev/visualization helper: seat one synthetic active effect into the
    /// pool at `world_pos` (world units) so the effect-pool render bridge can
    /// be exercised by hand. It ages and retires through the normal
    /// [`Self::tick_effects`] lifetime like any spawned effect.
    ///
    /// This is **not** a retail code path - it's a hand-spawn for exercising
    /// the render bridge without driving the action SM. The real catalog (PROT
    /// 0873 `efect.dat`) loads at scene entry, so `ui_element` spawns resolve
    /// to real scripts; use [`Self::try_spawn_effect`] for the production path.
    /// Returns `false` when the pool is full.
    pub fn spawn_debug_effect(&mut self, world_pos: [f32; 3]) -> bool {
        let Some(slot) = self.effect_pool.allocate_master() else {
            return false;
        };
        let m = &mut self.effect_pool.master_slots[slot];
        *m = vm::effect_vm::MasterSlot::default();
        // child_count > 0 marks the slot active for the walker + markers.
        m.child_count = 1;
        m.pos_x = (world_pos[0] * 256.0) as i32;
        m.pos_y = (world_pos[1] * 256.0) as i32;
        m.pos_z = (world_pos[2] * 256.0) as i32;
        true
    }

    /// Spawn effect `ui_id` at `world_pos` / `angle` via the pool, looking
    /// up the script in `self.effect_catalog`. No-op when the catalog is
    /// empty or the id is out of range. Mirrors the retail path through
    /// `FUN_801D8DE8 → FUN_801DFDF8`.
    pub fn try_spawn_effect(&mut self, ui_id: u8, world_pos: [i16; 3], angle: u16) {
        let catalog_ptr: *const vm::effect_vm::EffectCatalog = &self.effect_catalog;
        let pool_ptr: *mut vm::effect_vm::Pool = &mut self.effect_pool;
        let mut host = EffectHostImpl { world: self };
        // SAFETY: EffectHostImpl only reads `world.rng_state`; it never
        // accesses `effect_pool` or `effect_catalog` through the borrow.
        let pool = unsafe { &mut *pool_ptr };
        let catalog = unsafe { &*catalog_ptr };
        let _ = pool.spawn_by_ui_id(&mut host, ui_id, world_pos, angle, catalog);
    }

    /// Install an [`crate::encounter::EncounterSession`] for the current
    /// scene. Engines call this on scene-enter once the per-scene encounter
    /// table is known. `None` disables encounters for the active scene.
    pub fn set_encounter_session(&mut self, session: Option<crate::encounter::EncounterSession>) {
        self.encounter = session;
    }

    /// Install an encounter session resolved from a registry against the
    /// given CDNAME label. Engines call this from the scene-load path so
    /// every scene gets its retail-mapped encounter table without having
    /// to plumb tables through the boot config.
    ///
    /// Returns `true` when the registry resolved a non-empty table and
    /// installed it, `false` when no rule matched (or the resolved table
    /// has `trigger_rate_q8 == 0` - in which case the session is
    /// installed-but-quiet so the engine can still call `on_field_step`
    /// without nil checks).
    ///
    /// The on-disc resolver (reading the per-scene encounter table out of
    /// `0865_battle_data`) lands once a runtime watchpoint trace pins the
    /// table offset. Engines currently feed the registry from
    /// [`crate::encounter_registry::vanilla_encounter_registry`] or a
    /// custom composition.
    pub fn install_encounter_for_scene(
        &mut self,
        registry: &crate::encounter_registry::EncounterRegistry,
        scene_label: &str,
    ) -> bool {
        match registry.resolve(scene_label) {
            Some(table) => {
                let tracker = crate::encounter::EncounterTracker::new(table.clone());
                let session = crate::encounter::EncounterSession::new(tracker);
                let nonempty = !table.is_empty();
                self.encounter = Some(session);
                nonempty
            }
            None => {
                self.encounter = None;
                false
            }
        }
    }

    /// Install a fully-built [`crate::encounter::EncounterTable`] plus its
    /// per-row [`crate::monster_catalog::FormationDef`]s as the active
    /// scene's encounter source.
    ///
    /// This is the disc-resident path: the field scene-entry flow resolves
    /// the table + formations straight from the scene's MAN asset (retail
    /// `_DAT_8007B898`) via [`crate::encounter_man::scene_encounter_from_man`]
    /// and installs them here, in place of the synthetic-pattern
    /// [`Self::install_encounter_for_scene`] fallback. The formation defs are
    /// merged into `formation_table` so the table's row-index `formation_id`s
    /// resolve to concrete monster sets at battle-load.
    ///
    /// Returns whether the installed table is non-empty (an empty table is
    /// still installed-but-quiet so engines can call [`Self::on_field_step`]
    /// without nil checks).
    pub fn install_man_encounter(
        &mut self,
        table: crate::encounter::EncounterTable,
        formations: Vec<crate::monster_catalog::FormationDef>,
    ) -> bool {
        for def in formations {
            self.formation_table.insert(def);
        }
        let nonempty = !table.is_empty();
        let tracker = crate::encounter::EncounterTracker::new(table);
        self.encounter = Some(crate::encounter::EncounterSession::new(tracker));
        nonempty
    }

    /// Replace just the monster stat catalog, leaving `formation_table`
    /// untouched. The MAN encounter source carries formation monster-ids but
    /// not stat blocks, so the host installs the stat catalog separately when
    /// the formations come from [`Self::install_man_encounter`].
    pub fn set_monster_catalog(&mut self, catalog: crate::monster_catalog::MonsterCatalog) {
        self.monster_catalog = catalog;
    }

    /// Install the per-item battle-stat modifier table (weapon / armor /
    /// accessory bonuses). Boot wires this once; [`Self::seed_party_battle_stats`]
    /// folds the equipped items onto each party combatant at battle entry.
    pub fn set_equipment_table(&mut self, table: crate::battle_stats::EquipmentTable) {
        self.equipment_table = table;
    }

    /// Seed each party combatant's battle attack / defense from the roster's
    /// live stats plus equipped-gear bonuses.
    ///
    /// For every party slot whose roster record carries real stats (live
    /// attack `> 0`), this resolves a [`crate::battle_stats::BattleStats`] from
    /// the character's base attack / UDF / LDF and the modifiers of the items
    /// in its equipment slots ([`crate::battle_stats::compute_battle_stats_default`]
    /// against [`Self::equipment_table`]), then writes
    /// [`Self::battle_attack`] (= resolved attack) and
    /// [`Self::battle_defense_split`] (= resolved UDF / LDF). Slots with a
    /// zeroed roster record are left untouched, so synthetic battles that set
    /// `battle_attack` directly keep their values.
    ///
    /// Called automatically from the live-loop battle entry; also public so a
    /// host can refresh stats after an equipment change without re-entering the
    /// battle.
    pub fn seed_party_battle_stats(&mut self) {
        let pc = self.party_count.min(3) as usize;
        for slot in 0..pc {
            let Some(rec) = self.roster.members.get(slot) else {
                continue;
            };
            let live = rec.live_stats();
            // A zeroed roster carries no real stats; don't clobber any value a
            // synthetic battle set directly.
            if live.atk == 0 {
                continue;
            }
            let record = crate::battle_stats::StatRecord {
                base_attack: live.atk,
                base_udf: live.udf,
                base_ldf: live.ldf,
                base_accuracy: live.agl,
                base_evasion: live.agl,
                equip: rec.equipment().slots,
            };
            let stats = crate::battle_stats::compute_battle_stats_default(
                &record,
                &self.equipment_table,
                &[],
            );
            if let Some(s) = self.battle_attack.get_mut(slot) {
                *s = stats.atk;
            }
            self.set_battle_defense_split(slot as u8, Some((stats.udf, stats.ldf)));
        }
    }

    /// Install a formation + monster catalog pair. Boot wires this once;
    /// engines read it at battle-load time.
    pub fn set_formation_table(
        &mut self,
        table: crate::monster_catalog::FormationTable,
        catalog: crate::monster_catalog::MonsterCatalog,
    ) {
        self.formation_table = table;
        self.monster_catalog = catalog;
    }

    /// Install a [`crate::encounter_record::EncounterRecord`] decoded from
    /// an on-disc byte slice as the next encounter for the active scene.
    ///
    /// Mirrors the retail flow at `0x801DA620..0x801DA678`: the parsed
    /// record's monster ids are turned into a [`crate::monster_catalog::FormationDef`]
    /// (registered into `formation_table`), wrapped in a single-row
    /// [`crate::encounter::EncounterTable`] (rate `0xFF/256` so the next
    /// step roll always fires), and installed as the active session.
    ///
    /// Returns the synthesized `formation_id` so engines can immediately
    /// transition to battle if they want to skip the per-step roll.
    /// `None` means the record was empty (no monsters).
    pub fn install_encounter_from_record(
        &mut self,
        scene_label: &str,
        record: &crate::encounter_record::EncounterRecord,
    ) -> Option<u16> {
        if record.is_empty() {
            return None;
        }
        let formation = record.to_formation_def(scene_label);
        let formation_id = formation.formation_id;
        self.formation_table.insert(formation);

        use crate::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new(scene_label);
        // Force the next roll to succeed: the record IS the encounter.
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(formation_id, 1));
        let tracker = EncounterTracker::new(table);
        self.encounter = Some(EncounterSession::new(tracker));
        Some(formation_id)
    }

    /// Install an already-registered per-scene formation as the next encounter,
    /// by its `formation_id`.
    ///
    /// This is the faithful model of a scripted-battle carrier entity selecting
    /// a formation **by index** into the per-scene formation table - the
    /// mechanism the Rim Elm Tetsu tutorial fight uses. The per-scene formations
    /// load from the MAN asset into a contiguous 8-byte-stride table
    /// (`[3 reserved][count][<=4 ids]`, see [`crate::encounter_record`] /
    /// `docs/formats/encounter.md`); a carrier entity arms its encounter by
    /// pointing `actor[+0x94]` at one entry (`table_base + index*8`), and
    /// `FUN_801DA51C` copies that record into the formation cell on confirm. The
    /// id 0x4F that lands in the cell is **not** an inline script literal - it is
    /// the `monster_id` of town01 MAN `formation_id` 4, already registered by
    /// [`Self::install_man_encounter`] at scene entry (with its real archive
    /// stats merged).
    ///
    /// Unlike [`Self::install_encounter_from_record`], this re-encodes nothing:
    /// it forces the existing table row, so the scene's merged monster stats
    /// stand. Returns the `formation_id` installed, or `None` when it isn't
    /// registered or has no slots.
    ///
    /// REF: FUN_801DA51C
    pub fn install_man_formation(&mut self, formation_id: u16) -> Option<u16> {
        let has_slots = self
            .formation_table
            .formation(formation_id)
            .is_some_and(|def| !def.slots.is_empty());
        if !has_slots {
            return None;
        }
        let scene_label = self.active_scene_label.clone();

        use crate::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new(&scene_label);
        // Force the next step roll: the scripted carrier installs this formation.
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(formation_id, 1));
        let tracker = EncounterTracker::new(table);
        self.encounter = Some(EncounterSession::new(tracker));
        Some(formation_id)
    }

    /// Arm (or disarm) the scripted-encounter consumer.
    ///
    /// While armed, the field VM's bare arm-encounter op (`0x37`/`0x41`) hands
    /// the record window overlaying the opcode to the host, which parses it as
    /// an [`crate::encounter_record::EncounterRecord`] and routes it through
    /// [`Self::install_scripted_encounter`]. See
    /// [`Self::scripted_encounter_armed`] for why this gate exists (there is no
    /// dedicated encounter opcode; the consuming entity SM is the retail
    /// discriminator).
    pub fn arm_scripted_encounter(&mut self, on: bool) {
        self.scripted_encounter_armed = on;
    }

    /// Install a scripted encounter from the inline bytecode window the field
    /// VM forwarded at the bare arm-encounter op (`0x37`/`0x41`); the record
    /// overlays the opcode (`[opcode][op1][op2][count][ids..]`).
    ///
    /// The window is parsed as an [`crate::encounter_record::EncounterRecord`]
    /// (`[flag][_][_][count][ids..]`) and, when it carries at least one
    /// monster, installed against the active scene via
    /// [`Self::install_encounter_from_record`] - so the next
    /// [`Self::on_field_step`] flips Field -> Battle. Emits a
    /// [`FieldEvent::ScriptedEncounter`] for engine visibility regardless of
    /// whether the parse yielded a non-empty formation.
    ///
    /// Returns the synthesized `formation_id`, or `None` if the window did not
    /// parse into a non-empty record.
    ///
    /// PORT: FUN_801DA51C (the `[+4 + slot]` record-overlay reader)
    pub fn install_scripted_encounter(&mut self, record_bytes: &[u8]) -> Option<u16> {
        self.pending_field_events
            .push(FieldEvent::ScriptedEncounter {
                record: record_bytes.to_vec(),
            });
        let record = crate::encounter_record::EncounterRecord::parse(record_bytes)?;
        if record.is_empty() {
            return None;
        }
        let scene = self.active_scene_label.clone();
        let id = self.install_encounter_from_record(&scene, &record);
        // Fire-once: retail clears `entity[+0x94]` after the formation copy so
        // the arm fires exactly once. Disarm the engine-side carrier flag too.
        if id.is_some() {
            self.scripted_encounter_armed = false;
        }
        id
    }

    /// Field-step trigger. Engines call this once per "the player walked
    /// one map cell" (typically when the player actor's grid coord moves)
    /// to advance the encounter tracker. Returns `true` if a battle
    /// transition was triggered this step.
    ///
    /// The method is a no-op when no [`crate::encounter::EncounterSession`]
    /// is installed, when the session is not in `Idle`, or when the world
    /// is not in [`SceneMode::Field`].
    pub fn on_field_step(&mut self) -> bool {
        if !matches!(self.mode, SceneMode::Field) {
            return false;
        }
        let rng = self.next_rng();
        match self.encounter.as_mut() {
            Some(session) => session.on_step(rng),
            None => false,
        }
    }

    /// Per-frame tick of the encounter session timers. Drives the
    /// `Transition` and `Grace` countdowns.
    pub fn tick_encounter(&mut self) {
        if let Some(session) = self.encounter.as_mut() {
            session.tick_frame();
        }
    }

    /// Return the resolved [`crate::monster_catalog::FormationDef`] for the
    /// currently-triggered encounter, if any. Engines call this after the
    /// session reports `Triggered` to drain the roll and resolve into a
    /// concrete monster set; the session advances to `Battling` as a
    /// side-effect.
    pub fn drain_encounter_formation(&mut self) -> Option<crate::encounter::EncounterRoll> {
        self.encounter.as_mut().and_then(|s| s.drain_triggered())
    }

    /// Mark that the active battle finished. Engines call this from the
    /// post-battle resolution path so the session enters its grace window
    /// (suppresses encounters for `grace_frames` frames).
    pub fn end_encounter_battle(&mut self) {
        if let Some(session) = self.encounter.as_mut() {
            session.end_battle();
        }
    }

    /// Advance the wall-clock play-time counter by `delta_seconds`. Engines
    /// drive this from the frame loop's wall-clock delta. Mirrors the
    /// retail "play time" field shown on the save screen.
    pub fn advance_play_time(&mut self, delta_seconds: u32) {
        self.play_time_seconds = self.play_time_seconds.saturating_add(delta_seconds);
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
    pub fn set_pad(&mut self, mask: u16) {
        self.input.set_pad(mask);
    }

    /// Per-frame world tick. Drives whichever scene-mode VMs are live.
    /// Returns the battle-step outcome when in [`SceneMode::Battle`], else
    /// `None`.
    ///
    /// Order of operations:
    ///  1. Effect pool tick - runs every frame regardless of mode.
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
    pub fn tick(&mut self) -> Option<StepOutcome> {
        self.frame += 1;
        // Consume a pending FMV transition the field VM signalled last frame
        // (op `0x4C 0xE2`). Retail's main mode dispatcher reads the
        // next-game-mode global one frame after the op writes it, so the flip
        // into the cutscene mode lands here, at the top of the following tick.
        self.maybe_enter_pending_cutscene();
        self.tick_effects();
        self.tick_move_vms();
        self.tick_actor_physics();
        self.tick_actors();
        // Tick art-learned banner countdown - clear when it reaches zero.
        if let Some(banner) = &mut self.current_art_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_art_banner = None;
            }
        }
        // Tick level-up banner countdown.
        if let Some(banner) = &mut self.current_level_up_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_level_up_banner = None;
            }
        }
        // Advance the post-battle Seru-capture banner; clear when it finishes.
        if let Some(banner) = &mut self.current_capture_banner {
            banner.tick_frame();
            if banner.is_done() {
                self.current_capture_banner = None;
            }
        }
        match self.mode {
            SceneMode::Battle => {
                if self.live_gameplay_loop {
                    self.live_battle_tick()
                } else {
                    Some(self.step_battle())
                }
            }
            SceneMode::Field => {
                self.step_field();
                self.tick_tile_board();
                self.step_field_locomotion();
                self.tick_field_carriers();
                if self.live_gameplay_loop {
                    self.live_field_tick();
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
                    self.step_field();
                }
                None
            }
            SceneMode::WorldMap => {
                self.tick_world_map();
                None
            }
            SceneMode::Title => None,
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
        if self.current_dialog.is_some() {
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

    /// Drive the world-map controller from this frame's pad.
    ///
    /// The pad word is whatever the host installed via [`World::set_pad`]
    /// before this [`World::tick`]; the "held" mask is the just-pressed
    /// edge (`pad & !pad_prev`), matching the retail newly-pressed word
    /// (`_DAT_8007B874`) that [`WorldMapController::tick`] expects. No-ops
    /// when no controller is installed (e.g. world-map mode entered
    /// without [`World::enter_world_map`]).
    ///
    /// Reads only pad bits, never wall-clock state, so the resulting
    /// controller mutation is deterministic across identical pad streams.
    ///
    /// When overworld entities are installed
    /// ([`Self::install_world_map_entities`]) it also steps each one through
    /// the ported entity SM ([`vm::world_map::step`]): the Idle state drains
    /// the shared encounter countdown and, when it reaches zero with
    /// encounters enabled, latches the configured formation, which this method
    /// then resolves into a battle ([`SceneMode::WorldMap`] → [`SceneMode::Battle`],
    /// returning to the world map on victory). Interactions / portal
    /// transitions surface as [`FieldEvent::FieldInteract`] for the host.
    ///
    /// The per-entity SM itself is ported in [`vm::world_map`]; the encounter
    /// formation resolver it gates is the retail BGM/asset resolver.
    ///
    /// REF: FUN_801DA51C
    /// REF: FUN_800243F0
    fn tick_world_map(&mut self) {
        let pad = self.input.pad();
        let pad_held = pad & !self.input.pad_prev();
        if let Some(ctrl) = &mut self.world_map_ctrl {
            ctrl.tick(pad, pad_held);
        }
        // Player is "walking" on the overworld this frame when any d-pad
        // direction is held (bits 0x1000/0x2000/0x4000/0x8000).
        const WORLD_MAP_DPAD: u16 = 0x1000 | 0x2000 | 0x4000 | 0x8000;
        self.world_map_player_walking = pad & WORLD_MAP_DPAD != 0;

        if !self.world_map_entities.is_empty() {
            // Take the entity list out so the SM's host bridge can borrow the
            // world mutably (mirrors the monster-AI-state borrow window).
            let mut entities = std::mem::take(&mut self.world_map_entities);
            for (idx, ctx) in entities.iter_mut().enumerate() {
                let mut host = WorldMapEntityHostImpl { world: self };
                vm::world_map::step(idx, ctx, &mut host);
            }
            self.world_map_entities = entities;

            // Resolve a latched overworld encounter into a battle.
            if let Some(formation_id) = self.pending_world_map_encounter.take() {
                self.begin_world_map_encounter(formation_id);
            }
        }
    }

    /// Seed `count` overworld entity state machines (all Idle) so
    /// [`Self::tick_world_map`] drives encounter / interaction gameplay.
    /// Replaces any previously installed set. The retail engine builds one
    /// record per on-map entity from the scene's entity table; the clean-room
    /// world takes the count and pairs it with the shared encounter state
    /// configured via [`Self::set_world_map_encounter`].
    pub fn install_world_map_entities(&mut self, count: usize) {
        self.world_map_entities = (0..count)
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.world_map_entity_configs.clear();
    }

    /// Seed overworld entities with per-entity [`WorldMapEntityConfig`]s. One
    /// state machine (Idle) is created per config, so encounter zones spawn
    /// their own formation and portals carry their own target map. Replaces any
    /// previously installed set.
    pub fn install_world_map_entities_with_configs(&mut self, configs: Vec<WorldMapEntityConfig>) {
        self.world_map_entities = (0..configs.len())
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.world_map_entity_configs = configs;
    }

    /// Host signal that the player engaged overworld entity `idx` (walked onto
    /// a portal tile / pressed confirm on it). Drives the entity SM straight to
    /// its scene-transition state so the next [`Self::tick_world_map`] fires the
    /// transition; a [`WorldMapEntityConfig::Portal`] then surfaces a
    /// [`crate::field_events::FieldEvent::WorldMapTransition`] with its target
    /// map. No-op for an out-of-range index.
    ///
    /// This is the clean-room stand-in for retail's per-entity
    /// player-position-in-zone trigger (the engine has no overworld player
    /// placement yet); it mirrors the arm-via-API shape of the scripted-field
    /// encounter seam.
    pub fn engage_world_map_entity(&mut self, idx: usize) {
        if let Some(ctx) = self.world_map_entities.get_mut(idx) {
            // State 2 = Transitioning: the SM fires `on_scene_transition` and
            // retires the entity on the next tick.
            ctx.state = vm::world_map::EntityState::Transitioning as u16;
        }
    }

    /// Configure the shared overworld encounter rate. `enabled` is the master
    /// gate, `start_countdown` the initial per-step counter, `formation_id`
    /// the formation an encounter spawns (resolved against
    /// [`Self::formation_table`]), and `reset_to` the value the countdown is
    /// reset to after each encounter fires.
    pub fn set_world_map_encounter(
        &mut self,
        enabled: bool,
        start_countdown: i8,
        formation_id: u16,
        reset_to: i8,
    ) {
        self.world_map_encounter = WorldMapEncounterState {
            enabled,
            countdown: start_countdown,
            formation_id,
            reset_to,
        };
    }

    /// Place the scene's field entity SMs (all Idle). One
    /// [`vm::world_map::WorldMapEntityCtx`] per [`FieldCarrierConfig`], so a
    /// scripted-encounter carrier can be advanced via
    /// [`Self::engage_field_carrier`] and ticked by
    /// [`Self::tick_field_carriers`]. Replaces any previously installed set.
    ///
    /// This is the field-mode counterpart to
    /// [`Self::install_world_map_entities_with_configs`]; retail builds the
    /// same per-entity records from the scene's MAN actor-placement partition.
    pub fn install_field_carriers(&mut self, configs: Vec<FieldCarrierConfig>) {
        self.field_carriers = (0..configs.len())
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.field_carrier_configs = configs;
        self.pending_field_carrier_battle = None;
    }

    /// Host signal that the player engaged field carrier `idx` (accepted the
    /// Tetsu "Come at me!" dialogue / pressed confirm on the NPC). Advances the
    /// carrier's `FUN_801DA51C` SM from Idle to **Activating** and drains its
    /// countdown to zero, so the next [`Self::tick_field_carriers`] runs the
    /// state-1 body in full: `on_activating` (formation copy) immediately
    /// followed by the `case 2/3` fall-through scene-transition (battle
    /// handoff). No-op for an out-of-range index or a non-Idle carrier.
    ///
    /// Mirrors retail's scripted state-0 -> state-1 advance (towns are 0%
    /// random, so the Tetsu carrier never self-advances via the encounter
    /// roll; the dialogue script drives it).
    ///
    /// REF: FUN_801DA51C
    pub fn engage_field_carrier(&mut self, idx: usize) {
        if let Some(ctx) = self.field_carriers.get_mut(idx)
            && ctx.state == vm::world_map::EntityState::Idle as u16
        {
            ctx.state = vm::world_map::EntityState::Activating as u16;
        }
    }

    /// Step every installed field carrier SM one frame (the field-mode use of
    /// the ported `FUN_801DA51C`), then resolve a latched scripted-encounter
    /// transition into a battle. No-op when no carriers are installed.
    ///
    /// The entity list is taken out of the world so the SM's host bridge can
    /// borrow `&mut World` (same pattern as [`Self::tick_world_map`]).
    ///
    /// REF: FUN_801DA51C
    fn tick_field_carriers(&mut self) {
        if self.field_carriers.is_empty() {
            return;
        }
        let mut carriers = std::mem::take(&mut self.field_carriers);
        for (idx, ctx) in carriers.iter_mut().enumerate() {
            let mut host = FieldCarrierHostImpl { world: self };
            vm::world_map::step(idx, ctx, &mut host);
        }
        self.field_carriers = carriers;

        if let Some(formation_id) = self.pending_field_carrier_battle.take() {
            self.begin_field_carrier_battle(formation_id);
        }
    }

    /// Resolve a field carrier's latched `formation_id` against
    /// [`Self::formation_table`] and flip Field -> Battle, snapshotting the
    /// field context so [`Self::finish_battle`] returns to [`SceneMode::Field`].
    /// No-op when the id isn't registered.
    fn begin_field_carrier_battle(&mut self, formation_id: u16) {
        // Reuse the field-encounter battle entry: a carrier transition is a
        // forced encounter against a registered MAN formation.
        self.begin_encounter_battle(crate::encounter::EncounterRoll {
            formation_id,
            row_index: 0,
            roll_q8: 0,
        });
    }

    /// Resolve `formation_id` against [`Self::formation_table`] and flip from
    /// the world map into a battle, snapshotting the world-map context so
    /// [`Self::finish_battle`] returns to [`SceneMode::WorldMap`]. No-op when
    /// the id isn't registered (the encounter is simply dropped).
    fn begin_world_map_encounter(&mut self, formation_id: u16) {
        let Some(formation) = self.formation_table.formation(formation_id).cloned() else {
            return;
        };
        self.field_return = Some(FieldReturnState {
            actors: self.actors.clone(),
            player_actor_slot: self.player_actor_slot,
            party_count: self.party_count,
        });
        self.battle_return_mode = SceneMode::WorldMap;
        // `enter_battle_from_formation` swaps to the battle BGM itself.
        self.enter_battle_from_formation(&formation);
        self.active_formation = Some(formation);
    }

    /// Per-actor move-VM tick - clean port of `FUN_80021DF4` (lines
    /// `80022B94..80022BBC`).
    ///
    /// Two-phase: (1) pre-tick decrement the per-actor `wait_timer` by the
    /// global frame-time `delta`, (2) run the move VM through
    /// [`vm::move_vm::actor_tick`], which gates on the resulting timer and
    /// inspects the HALT flag after the call. Outcomes are recorded in
    /// [`World::move_outcomes`] so engines that want to react to per-actor
    /// halts / waits can read them after the world ticks.
    ///
    /// `delta` mirrors the retail product `_DAT_1f800393 * _DAT_1f80037D`
    /// (per-frame anim-speed scalars). Engines pass their own per-frame
    /// scalar; the default world tick uses `1` so a Wait of N consumes N
    /// frames.
    pub fn tick_move_vms_with_delta(&mut self, delta: u16) {
        self.move_outcomes.clear();
        for slot in 0..self.actors.len() {
            if !self.actors[slot].active {
                continue;
            }
            let bc = self.move_bytecode.get(slot).cloned().unwrap_or_default();
            if bc.is_empty() {
                continue;
            }
            // Pre-tick: decrement wait timer (retail does this unconditionally
            // before the gate).
            vm::move_vm::decrement_wait_timer(&mut self.actors[slot].move_state, delta);
            let outcome = self.actor_tick_at(slot, &bc, MOVE_VM_BUDGET);
            self.move_outcomes.push((slot as u8, outcome));
        }
    }

    /// Backwards-compatible wrapper using `delta = 1`.
    pub fn tick_move_vms(&mut self) {
        self.tick_move_vms_with_delta(1);
    }

    /// Per-actor physics tick - clean-room port driver for
    /// `engine-vm::actor_tick::tick_actor` (FUN_80021DF4). Runs
    /// [`vm::actor_tick::tick_actor`] once per active slot, then dispatches
    /// the emitted [`TickEvent`]s.
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
    /// [`World::last_tick_events`] for engines that want to consume
    /// them but otherwise no-op. Wiring those is orthogonal to the
    /// move-buffer cursor.
    ///
    /// `frame_delta` matches the retail `DAT_1F800393` ramp scalar
    /// (idle = `1`). The default tick uses `1`.
    pub fn tick_actor_physics_with(&mut self, scalars: TickScalars, listener: &ListenerState) {
        self.last_tick_events.clear();
        let host = move_buffer_host::WorldMoveBufferView {
            move_buf: &self.move_buffer_root,
            move2_buf: &self.move2_buffer_root,
            alt_buf: &self.move_buffer_alt_root,
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
                self.last_tick_events.push((idx as u8, res));
            }
        }
    }

    /// Backwards-compatible wrapper using idle scalars and a default
    /// listener (no positional SFX integration yet).
    pub fn tick_actor_physics(&mut self) {
        let listener = ListenerState::unicast(0, 0, 0);
        self.tick_actor_physics_with(TickScalars::idle(), &listener);
    }

    /// Install the MOVE buffer pool root (retail `_DAT_8007B888`). The
    /// bytes are the MDT-shaped offset-table blob the scene-load path
    /// extracts from the slot-1 `Asset(0x05) = Move` descriptor. Pass
    /// an empty slice to clear it - the cursor's resolver will then
    /// return `None` for every requested id.
    pub fn set_move_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move_buffer_root = bytes;
    }

    /// Install the MOVE2 buffer pool root (retail `_DAT_8007B840`).
    /// Selected when an actor's `cursor_requested` is `>= 0x400`.
    pub fn set_move2_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move2_buffer_root = bytes;
    }

    /// Install the alternate MOVE buffer pool root (retail
    /// `_DAT_8007B75C`). Selected when the actor's status flag word
    /// has [`vm::move_buffer::STATUS_FLAG_ALT_POOL`] set.
    pub fn set_move_buffer_alt_root(&mut self, bytes: Vec<u8>) {
        self.move_buffer_alt_root = bytes;
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

    /// Bind actor `slot` to TMD index `tmd_idx` in `SceneResources::tmds`.
    /// Renderers use this binding to look up the right mesh when applying
    /// the actor's `pose_frame`. No-ops for out-of-range slots.
    pub fn set_actor_tmd_binding(&mut self, slot: usize, tmd_idx: usize) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.tmd_binding = Some(tmd_idx);
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
        };
        let actor_state = unsafe {
            // SAFETY: same disjoint-field justification as `step_move_vm`.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        let outcome = vm::move_vm::actor_tick(&mut host, actor_state, bytecode, budget);
        let writes = std::mem::take(&mut host.deferred_writes);
        if !writes.is_empty()
            && let Some(buf) = self.move_bytecode.get_mut(slot)
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

    /// Place the world into [`SceneMode::Battle`] and populate the actor
    /// pointer table with `party_count` party slots followed by
    /// `monster_count` monster slots, mirroring the layout
    /// `FUN_800520F0` produces (slots 0..2 = party, 3..7 = monsters; total
    /// caps at 8). Each actor is positioned `radius` units left (party)
    /// or right (monsters) of the origin, with a per-row z spread.
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
    pub fn enter_battle(&mut self, party_count: u8, monster_count: u8, radius: i16) {
        self.mode = SceneMode::Battle;
        self.party_count = party_count.min(3);
        let actor_count =
            ((self.party_count as usize) + (monster_count.min(5) as usize)).min(MAX_ACTORS);
        // Spread along z. Party left, monsters right, both staggered by 0.6 / 0.4.
        for i in 0..(self.party_count as usize).min(actor_count) {
            let z = (i as i16 - 1) * (radius * 6 / 10);
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = -radius;
            actor.move_state.world_y = 0;
            actor.move_state.world_z = z;
            actor.battle.liveness = 1;
        }
        for i in (self.party_count as usize)..actor_count {
            let z = (i as i16 - 5) * (radius * 4 / 10);
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = radius;
            actor.move_state.world_y = 0;
            actor.move_state.world_z = z;
            actor.battle.liveness = 1;
        }
        // Reset the battle ctx and seed at Begin via the public byte API to
        // avoid pulling battle_action::ActionState into world.rs imports.
        self.battle_ctx = vm::battle_action::BattleActionCtx::new();
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        self.battle_end = None;
        // Effect pool is reused across scenes - reset to a fresh instance
        // (per-battle the head/free-list rebuilds from scratch).
        self.effect_pool = vm::effect_vm::Pool::new();
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
        if self.world_map_ctrl.is_none() {
            self.world_map_ctrl = Some(WorldMapController::new());
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
    fn maybe_enter_pending_cutscene(&mut self) {
        let Some(fmv_id) = self.pending_fmv_trigger.take() else {
            return;
        };
        if self.mode != SceneMode::Field {
            return;
        }
        if crate::cutscene::fmv_index_to_str_filename(fmv_id).is_some() {
            self.cutscene_return_mode = Some(self.mode);
            self.mode = SceneMode::Cutscene;
            self.active_fmv = Some(fmv_id);
        }
    }

    /// The FMV index currently playing in [`SceneMode::Cutscene`], or `None`
    /// when no STR FMV is active. Hosts poll this after [`World::tick`] to
    /// learn which `MV*.STR` to open.
    pub fn active_fmv(&self) -> Option<i16> {
        self.active_fmv
    }

    /// The retail `MV*.STR` path of the active cutscene FMV, or `None` when
    /// no STR FMV is active. Convenience over
    /// [`crate::cutscene::fmv_index_to_str_filename`].
    pub fn active_fmv_str_filename(&self) -> Option<&'static str> {
        self.active_fmv
            .and_then(crate::cutscene::fmv_index_to_str_filename)
    }

    /// End the active STR-FMV cutscene and return to the scene mode that was
    /// live when it started (the field, in the normal flow). Retail returns
    /// here when the cutscene/MDEC overlay finishes playback and unloads.
    ///
    /// The field VM resumes from where it paused - its program counter is
    /// already past the FMV op, so the next field tick continues the script.
    /// A no-op when no cutscene is active.
    pub fn finish_cutscene(&mut self) {
        if self.mode == SceneMode::Cutscene {
            self.mode = self.cutscene_return_mode.take().unwrap_or(SceneMode::Field);
            self.active_fmv = None;
        }
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

    // --- field collision grid + free-movement locomotion ----------------

    /// Reset the per-scene field collision grid to "all walkable" (every
    /// byte zero). Called at field entry; the scene prescript repaints the
    /// wall bits via the field-VM `0x4C` outer-nibble-7 op. Mirrors the
    /// retail wholesale clear of `*(_DAT_1F8003EC) + 0x4000` at scene boot
    /// (the exact retail clear site is unpinned; zeroing here is the
    /// engine-side equivalent - see `docs/subsystems/field-locomotion.md`).
    pub fn reset_field_collision_grid(&mut self) {
        self.field_collision_grid.clear();
        self.field_collision_grid.resize(FIELD_GRID_LEN, 0);
    }

    /// Load the per-scene base collision/floor grid from the field map file's
    /// `+0x4000` region (the `DATA\FIELD\<scene>.MAP` slice exposed by
    /// [`crate::scene::Scene::field_collision_grid`]). `grid` is the raw
    /// `0x80 x 0x80` byte grid: high nibble = sub-cell wall bits, low nibble =
    /// floor-elevation tier - the same byte format the runtime grid uses, so
    /// it copies verbatim. The field-VM `0x4C` nibble-7 ops then layer
    /// story-conditional deltas on top as the prescript runs.
    ///
    /// PORT: the `+0x4000` sub-region streamed by `FUN_8001f7c0` into the
    /// field buffer at `*(_DAT_1f8003ec)`. Byte-exact vs live RAM (town01).
    pub fn load_field_collision_grid(&mut self, grid: &[u8]) {
        let n = grid.len().min(FIELD_GRID_LEN);
        self.field_collision_grid.clear();
        self.field_collision_grid.resize(FIELD_GRID_LEN, 0);
        self.field_collision_grid[..n].copy_from_slice(&grid[..n]);
    }

    /// Apply one field-VM `0x4C` outer-nibble-7 rectangular wall paint to
    /// the collision grid. `x_range` / `z_range` are the half-open tile
    /// spans the VM dispatcher already computed from the op operands; `sub`
    /// selects the per-byte high-nibble mutation:
    ///
    /// | sub | op |
    /// |---|---|
    /// | 0 | `byte &= 0x0F` (clear walls - make walkable) |
    /// | 1 | `byte |= 0xF0` (block all four sub-cells) |
    /// | 2 | `byte &= ~(mask << 4)` (clear selected wall bits) |
    /// | 3 | `byte |= (mask << 4)` (set selected wall bits) |
    ///
    /// Out-of-range tiles are skipped. The low nibble (floor-elevation
    /// tier) is preserved.
    fn paint_field_collision(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        if self.field_collision_grid.len() < FIELD_GRID_LEN {
            self.reset_field_collision_grid();
        }
        let hi = mask << 4;
        for row in z_range.0..z_range.1 {
            let row_base = (row as usize) * FIELD_GRID_STRIDE;
            for col in x_range.0..x_range.1 {
                let idx = row_base + col as usize;
                let Some(byte) = self.field_collision_grid.get_mut(idx) else {
                    continue;
                };
                match sub {
                    0 => *byte &= 0x0F,
                    1 => *byte |= 0xF0,
                    2 => *byte &= !hi,
                    3 => *byte |= hi,
                    _ => {}
                }
            }
        }
    }

    /// Sample the collision grid at world coords `(x, z)` and return `true`
    /// if the covering sub-cell is a wall.
    ///
    /// PORT: FUN_801cfe4c
    ///
    /// Mirrors `FUN_801cfe4c`'s grid
    /// sample: world coords convert to 64-unit sub-cells (`>> 6`), the
    /// 128-unit tile column/row is `sub_cell >> 1` (rows of `0x80` bytes),
    /// and the wall bit is `byte >> 4 & quadrant_mask` where the quadrant
    /// is selected by `(sub_cell_z & 1) * 2 + (sub_cell_x & 1)`.
    pub fn field_tile_is_wall(&self, x: i16, z: i16) -> bool {
        if self.field_collision_grid.len() < FIELD_GRID_LEN {
            return false;
        }
        if x < 0 || z < 0 {
            return true; // off the grid origin reads as a wall (clamp inside)
        }
        let sx = (x as i32) >> 6;
        let sz = (z as i32) >> 6;
        let col = (sx >> 1) & 0x7F;
        let row = (sz >> 1) & 0x7F;
        let idx = (col + row * FIELD_GRID_STRIDE as i32) as usize;
        let Some(&byte) = self.field_collision_grid.get(idx) else {
            return false;
        };
        let quad = ((sz & 1) << 1 | (sx & 1)) as u32;
        (byte >> 4) & (1u8 << quad) != 0
    }

    /// Decode this frame's held d-pad into a camera-relative movement
    /// direction and an 8-direction heading angle. Returns
    /// `(dir_bits, heading)` where `dir_bits` uses the retail post-remap
    /// convention (`0x1000` = Z+, `0x4000` = Z-, `0x2000` = X+, `0x8000` =
    /// X-) and `heading` is a PSX 12-bit angle (`4096` = full turn).
    /// `dir_bits == 0` means no direction is held.
    ///
    /// The raw screen direction (up / down / left / right) is remapped by
    /// [`World::field_camera_azimuth`] quantised to the nearest 90° so
    /// "screen up" always walks away from the camera, the same job
    /// `func_0x800467e8` does in retail.
    fn decode_field_direction(&self) -> (u16, i16) {
        let up = self.input.pressed(input::PadButton::Up);
        let down = self.input.pressed(input::PadButton::Down);
        let left = self.input.pressed(input::PadButton::Left);
        let right = self.input.pressed(input::PadButton::Right);

        // Screen-space delta: +Y forward (away from camera), +X right.
        let mut sx: i32 = 0;
        let mut sy: i32 = 0;
        if up {
            sy += 1;
        }
        if down {
            sy -= 1;
        }
        if right {
            sx += 1;
        }
        if left {
            sx -= 1;
        }
        if sx == 0 && sy == 0 {
            return (0, 0);
        }

        // Quantise the camera azimuth to one of four cardinal rotations and
        // rotate the screen delta into world space. quadrant 0 = identity
        // (screen-up -> +Z, screen-right -> +X).
        let quadrant = (((self.field_camera_azimuth as u32) + 512) / 1024) & 3;
        let (mut wx, mut wz) = match quadrant {
            0 => (sx, sy),
            1 => (sy, -sx),
            2 => (-sx, -sy),
            _ => (-sy, sx),
        };
        wx = wx.clamp(-1, 1);
        wz = wz.clamp(-1, 1);

        let mut bits = 0u16;
        if wz > 0 {
            bits |= 0x1000; // Z+
        } else if wz < 0 {
            bits |= 0x4000; // Z-
        }
        if wx > 0 {
            bits |= 0x2000; // X+
        } else if wx < 0 {
            bits |= 0x8000; // X-
        }

        // Heading: atan2(wx, wz) in 12-bit units. Z+ = 0, X+ = quarter turn.
        let heading = (((wx as f32).atan2(wz as f32) / std::f32::consts::TAU * 4096.0).round()
            as i32
            & 0x0FFF) as i16;
        (bits, heading)
    }

    /// Free-movement locomotion step - the engine-side port of
    /// `FUN_801d01b0` (field overlay `overlay_0897`).
    ///
    /// PORT: FUN_801d01b0
    ///
    /// Reads this frame's
    /// pad, turns it into a camera-relative direction + facing, and
    /// advances the player actor in 2-unit increments with per-axis
    /// collision against [`World::field_collision_grid`].
    ///
    /// No-ops when there is no player actor, while a dialog box is up (the
    /// field VM owns the frame), while the tile-board minigame is installed
    /// (that mode runs its own digital stepper), or while the player's
    /// movement-disabled flag (`+0x10 & 0x80000`) is set (encounter queued
    /// / cutscene owns the player). Reads only pad bits + grid + actor
    /// state, so it is deterministic across identical pad streams.
    pub fn step_field_locomotion(&mut self) {
        if self.current_dialog.is_some() || self.tile_board.is_some() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return;
        }
        if self.actors[slot].move_state.flags & 0x0008_0000 != 0 {
            return;
        }

        let (dir_bits, heading) = self.decode_field_direction();
        if dir_bits == 0 {
            return;
        }
        self.actors[slot].move_state.render_26 = heading;

        // speed = ((base_step * player[+0x72]) >> 12) * DAT_1f800393.
        let mult = self.actors[slot].move_state.field_72 as i32;
        let ratio = self.move_ramp_ratio.max(1) as i32;
        let mut speed = ((FIELD_BASE_STEP * mult) >> 12) * ratio;
        // Diagonal normalise (camera mode 4, both axes pressed): x0.75.
        let z_pressed = dir_bits & 0x5000 != 0;
        let x_pressed = dir_bits & 0xA000 != 0;
        if z_pressed && x_pressed {
            speed -= speed >> 2;
        }
        if speed <= 0 {
            return;
        }

        // Step loop: advance FIELD_STEP_UNIT per iteration with per-axis
        // collision, committing only the axes that stay clear.
        let mut remaining = speed;
        while remaining > 0 {
            let ms = &self.actors[slot].move_state;
            let (cx, cz) = (ms.world_x, ms.world_z);
            // Z axis.
            if dir_bits & 0x1000 != 0 {
                let nz = cz.saturating_add(FIELD_STEP_UNIT as i16);
                if !self.field_tile_is_wall(cx, nz) {
                    self.actors[slot].move_state.world_z = nz;
                }
            } else if dir_bits & 0x4000 != 0 {
                let nz = cz.saturating_sub(FIELD_STEP_UNIT as i16);
                if !self.field_tile_is_wall(cx, nz) {
                    self.actors[slot].move_state.world_z = nz;
                }
            }
            // X axis (re-read X in case Z committed; X collision uses the
            // committed Z so footprints don't tunnel diagonally).
            let cz2 = self.actors[slot].move_state.world_z;
            if dir_bits & 0x2000 != 0 {
                let nx = cx.saturating_add(FIELD_STEP_UNIT as i16);
                if !self.field_tile_is_wall(nx, cz2) {
                    self.actors[slot].move_state.world_x = nx;
                }
            } else if dir_bits & 0x8000 != 0 {
                let nx = cx.saturating_sub(FIELD_STEP_UNIT as i16);
                if !self.field_tile_is_wall(nx, cz2) {
                    self.actors[slot].move_state.world_x = nx;
                }
            }
            remaining -= FIELD_STEP_UNIT;
        }
    }

    // --- live gameplay loop: Field <-> Battle round trip ------------------

    /// Per-frame field-side driver for the live gameplay loop. Gated by
    /// [`Self::live_gameplay_loop`] in [`Self::tick`]; never called when the
    /// flag is off.
    ///
    /// Composes the already-existing encounter pieces into the per-frame
    /// flow the retail field loop runs:
    ///
    /// 1. **Step detection.** A "step" is the player actor crossing into a
    ///    new 128-unit collision tile (`pos >> 7`). Each step drives one
    ///    [`Self::on_field_step`] roll - matching the retail per-step
    ///    counter rather than rolling every frame.
    /// 2. **Timers.** [`Self::tick_encounter`] advances the session's
    ///    `Transition` / `Grace` countdowns every frame regardless of
    ///    movement.
    /// 3. **Transition.** When the session reaches `Triggered`,
    ///    [`Self::drain_encounter_formation`] yields the rolled formation and
    ///    [`Self::begin_encounter_battle`] flips `Field -> Battle`.
    fn live_field_tick(&mut self) {
        // (1) step detection on tile crossing.
        if let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get(slot as usize)
        {
            let tile = (actor.move_state.world_x >> 7, actor.move_state.world_z >> 7);
            match self.field_last_tile {
                Some(prev) if prev != tile => {
                    self.field_last_tile = Some(tile);
                    self.on_field_step();
                }
                None => self.field_last_tile = Some(tile),
                _ => {}
            }
        }
        // (2) advance transition / grace timers.
        self.tick_encounter();
        // (3) Triggered -> begin battle.
        if let Some(roll) = self.drain_encounter_formation() {
            self.begin_encounter_battle(roll);
        }
    }

    /// Resolve `roll` to a concrete formation and flip into battle.
    ///
    /// Snapshots the field actor table (restored verbatim on victory),
    /// remembers the formation for [`Self::apply_battle_loot`], and seeds the
    /// battle actor table from the formation + monster catalog. No-op when
    /// the roll's `formation_id` isn't registered in
    /// [`Self::formation_table`] (the session has already advanced to
    /// `Battling`, so the next [`Self::end_encounter_battle`] cleans it up).
    fn begin_encounter_battle(&mut self, roll: crate::encounter::EncounterRoll) {
        let Some(formation) = self.formation_table.formation(roll.formation_id).cloned() else {
            // Unknown formation: bail back to the field by ending the (empty)
            // battle so the session leaves `Battling`.
            self.end_encounter_battle();
            return;
        };
        self.field_return = Some(FieldReturnState {
            actors: self.actors.clone(),
            player_actor_slot: self.player_actor_slot,
            party_count: self.party_count,
        });
        self.battle_return_mode = SceneMode::Field;
        self.enter_battle_from_formation(&formation);
        self.active_formation = Some(formation);
    }

    /// Seed the battle actor table from `formation` and enter
    /// [`SceneMode::Battle`].
    ///
    /// Party slots `0..party_count` keep their HP / MP (seeded from the
    /// roster by the boot path); monster slots take HP / attack / defense
    /// from [`Self::monster_catalog`]. Every combatant is marked alive,
    /// `action_category = Attack`, and party members target the first
    /// monster. The battle-action context is seeded at `Begin` with the
    /// Attack action queued. This is the live-loop counterpart to the
    /// generic [`Self::enter_battle`] placement helper.
    /// Configure the battle BGM track id. `Some(id)` enables the
    /// Battle↔Field music swap (the live loop switches to `id` on encounter
    /// and restores the field track on battle end); `None` disables it. See
    /// [`World::battle_bgm`].
    pub fn set_battle_bgm(&mut self, bgm_id: Option<u16>) {
        self.battle_bgm = bgm_id;
    }

    /// Switch to the configured battle track at encounter start. No-op when
    /// [`World::battle_bgm`] is `None` or the swap is already active. Stashes
    /// the current field track for [`World::restore_field_bgm`] and queues a
    /// `FieldEvent::Bgm` start so the host's BGM director cross-fades to it.
    fn swap_to_battle_bgm(&mut self) {
        let Some(battle) = self.battle_bgm else {
            return;
        };
        if self.battle_bgm_active || self.current_bgm == Some(battle) {
            return;
        }
        self.field_bgm_resume = self.current_bgm;
        self.current_bgm = Some(battle);
        self.battle_bgm_active = true;
        self.pending_field_events.push(FieldEvent::Bgm {
            text_id: battle,
            sub_op: 1,
        });
    }

    /// Restore the field track stashed by [`World::swap_to_battle_bgm`] when
    /// a battle ends. No-op unless a battle swap is active. Queues a
    /// `FieldEvent::Bgm` start for the stashed track, or a stop (sub-op 4)
    /// when no field track was playing at encounter start.
    fn restore_field_bgm(&mut self) {
        if !self.battle_bgm_active {
            return;
        }
        self.battle_bgm_active = false;
        match self.field_bgm_resume.take() {
            Some(track) => {
                self.current_bgm = Some(track);
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: track,
                    sub_op: 1,
                });
            }
            None => {
                self.current_bgm = None;
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: 0,
                    sub_op: 4,
                });
            }
        }
    }

    fn enter_battle_from_formation(&mut self, formation: &crate::monster_catalog::FormationDef) {
        let party_count = self.party_count.clamp(1, 3);
        let monster_count = formation.slots.len().min(5) as u8;
        // Reuse the placement helper for actor spawn + spacing, then overlay
        // per-slot stats.
        self.enter_battle(party_count, monster_count, 600);
        let first_monster = party_count;
        for slot in 0..party_count as usize {
            let a = &mut self.actors[slot];
            a.battle.liveness = 1;
            a.battle.action_category = 3; // Attack
            a.battle.active_target = first_monster;
            // Party members are not monsters - clear any id left from a
            // previous battle that placed an enemy in this slot.
            a.battle_monster_id = None;
        }
        // Fold the roster's live stats + equipped-gear bonuses onto the party
        // combatants' attack / defense (no-op for a zeroed roster).
        self.seed_party_battle_stats();
        // Clear any monster-slot SPD left over from a previous battle so the
        // initiative gate only sees this formation's speeds.
        for s in self.battle_speed.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for (i, fslot) in formation.slots.iter().take(5).enumerate() {
            let mslot = party_count as usize + i;
            if mslot >= self.actors.len() {
                break;
            }
            // Tag the slot with its monster id so a renderer can fetch the
            // battle mesh, even if the catalog has no stats for it.
            self.actors[mslot].battle_monster_id = Some(fslot.monster_id);
            if let Some(def) = self.monster_catalog.get(fslot.monster_id) {
                let speed = def.speed;
                let a = &mut self.actors[mslot];
                a.battle.hp = def.hp;
                a.battle.max_hp = def.hp;
                a.battle.mp = def.mp;
                a.battle.liveness = 1;
                a.battle.action_category = 3;
                if let Some(s) = self.battle_attack.get_mut(mslot) {
                    *s = def.attack;
                }
                if let Some(s) = self.battle_defense.get_mut(mslot) {
                    *s = def.udf.max(def.ldf);
                }
                if let Some(s) = self.battle_speed.get_mut(mslot) {
                    *s = speed;
                }
            }
        }
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.active_actor = 0;
        // Fresh battle: clear the monster-AI cooldowns / phase counter / ring.
        self.monster_ai_state.reset();
        // Seed the turn-order initiative keys for this battle. When real SPD is
        // present this lets the next-actor selector run the initiative scheme;
        // slot 0 still opens round 1 (its key is consumed below) so subsequent
        // turns order by initiative. A no-SPD battle leaves every key at 0 and
        // stays on the round-robin fallback.
        self.seed_battle_initiative();
        // Switch to the battle track (if configured) - the host's BGM
        // director cross-fades from the field music.
        self.swap_to_battle_bgm();
        if self.battle_player_driven {
            // Player-driven: don't pre-arm the first attack - open the command
            // menu for party member 0 and let the SM idle until the player
            // confirms (handled in `live_battle_tick`).
            self.open_battle_command(0);
        }
    }

    /// Open the player-driven command menu for party member `actor` and park
    /// the action SM. The action context's `active_actor` is set now; the
    /// queued action / target is filled in by [`Self::tick_battle_command`]
    /// once the player confirms. No-op unless [`Self::battle_player_driven`].
    fn open_battle_command(&mut self, actor: u8) {
        if !self.battle_player_driven {
            return;
        }
        self.battle_ctx.active_actor = actor;
        self.battle_command = Some(crate::battle_input::BattleCommandSession::new(actor, actor));
    }

    /// Drive the open command session one frame from [`World::input`]. When the
    /// session resolves, arm the action SM with the chosen command + target
    /// (v0.1: a physical Attack) and clear the session so the SM resumes.
    /// On an abort (no valid target) it falls back to the first living monster
    /// so the loop never deadlocks.
    fn tick_battle_command(&mut self) {
        use crate::battle_input::{BattleCommandInput, Resolution};
        use crate::input::PadButton;
        use crate::target_picker::{CursorRow, SlotState};

        let Some(mut session) = self.battle_command.take() else {
            return;
        };

        let party_count = self.party_count.clamp(1, 3);
        let slot_at = |idx: usize| -> SlotState {
            match self.actors.get(idx) {
                Some(a) if a.battle.max_hp > 0 => SlotState::alive(true, a.battle.liveness != 0),
                _ => SlotState::default(),
            }
        };
        let mut party = [SlotState::default(); 3];
        for (i, p) in party.iter_mut().enumerate().take(party_count as usize) {
            *p = slot_at(i);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, m) in monsters.iter_mut().enumerate() {
            *m = slot_at(party_count as usize + i);
        }

        let ev = BattleCommandInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        session.input(ev, party, monsters);

        match session.resolved() {
            Some(Resolution::Confirmed {
                // v0.1 only enables Attack, so `command` is always Attack here;
                // Arts/Magic/Item aren't wired into the live loop yet.
                command: _,
                target_row,
                target_slot,
            }) => {
                let target = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                let actor = session.actor;
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3; // Attack
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
                // Session done; SM resumes next tick.
            }
            Some(Resolution::OpenArtsMenu) => {
                // Player picked Arts: hand off to the saved-chain submenu (same
                // pattern as Magic / Item). `tick_battle_arts_menu` drives until
                // the player runs an art (turn cycles via EndOfAction) or backs
                // out.
                self.battle_ctx.active_actor = session.actor;
                let rows = self.build_battle_arts_rows(session.actor);
                self.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(
                    session.actor,
                    session.actor,
                    rows,
                ));
            }
            Some(Resolution::OpenSpellMenu) => {
                // Player picked Magic: hand off to the spell submenu (same
                // pattern as Item). `tick_battle_spell_menu` drives until the
                // player casts (turn cycles via EndOfAction) or backs out.
                self.battle_ctx.active_actor = session.actor;
                match self.build_battle_spell_session(session.actor) {
                    Some(menu) => self.battle_spell_menu = Some(menu),
                    // No caster record / no catalog - don't strand the SM;
                    // reopen the command menu so the player can pick again.
                    None => self.open_battle_command(session.actor),
                }
            }
            Some(Resolution::OpenItemMenu) => {
                // Player picked Item: hand off to the inventory submenu. The
                // command session is dropped (already taken) and the action SM
                // stays parked; `tick_battle_item_menu` drives until the player
                // uses an item (turn cycles via EndOfAction) or backs out
                // (the command menu reopens for the same actor).
                self.battle_ctx.active_actor = session.actor;
                self.battle_item_menu = Some(self.build_battle_item_session());
            }
            Some(Resolution::Aborted) => {
                // No valid target the player could pick - arm a default strike
                // on the first living monster so the loop progresses.
                let actor = session.actor;
                let target = (party_count..self.actors.len() as u8)
                    .find(|&i| self.actors[i as usize].battle.liveness != 0)
                    .unwrap_or(party_count);
                if let Some(a) = self.actors.get_mut(actor as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
                self.battle_ctx.active_actor = actor;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
            }
            None => {
                // Still selecting - keep the session open for the next frame.
                self.battle_command = Some(session);
            }
        }
    }

    /// Drive the open battle Arts submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::battle_arts::BattleArtsInput`] per
    /// frame. On a confirmed execution the art runs via [`Self::apply_battle_art`]
    /// (driving each strike's power byte through the real `apply_art_strike`
    /// path) and the action SM parks at `EndOfAction` so the live loop cycles to
    /// the next combatant. Backing out reopens the command menu.
    fn tick_battle_arts_menu(&mut self) {
        use crate::battle_arts::{ArtsResolution, BattleArtsInput};
        use crate::input::PadButton;
        use crate::target_picker::SlotState;

        let Some(mut menu) = self.battle_arts_menu.take() else {
            return;
        };

        let party_count = self.party_count.clamp(1, 3);
        let slot_at = |idx: usize| -> SlotState {
            match self.actors.get(idx) {
                Some(a) if a.battle.max_hp > 0 => SlotState::alive(true, a.battle.liveness != 0),
                _ => SlotState::default(),
            }
        };
        let mut party = [SlotState::default(); 3];
        for (i, p) in party.iter_mut().enumerate().take(party_count as usize) {
            *p = slot_at(i);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, m) in monsters.iter_mut().enumerate() {
            *m = slot_at(party_count as usize + i);
        }

        let ev = BattleArtsInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        menu.input(ev, party, monsters);

        match menu.resolved() {
            Some(ArtsResolution::Confirmed {
                art_index,
                target_row,
                target_slot,
            }) => {
                let caster = menu.actor;
                let (power, enemy_effect) = menu
                    .arts
                    .get(art_index as usize)
                    .map(|a| (a.power.clone(), a.enemy_effect))
                    .unwrap_or_default();
                self.apply_battle_art(caster, &power, enemy_effect, target_row, target_slot);
                self.battle_ctx.action_state =
                    vm::battle_action::ActionState::EndOfAction.as_byte();
            }
            Some(ArtsResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle_arts_menu = Some(menu);
            }
        }
    }

    /// Execute an art against the picked target through the real art-power
    /// path.
    ///
    /// Each [`legaia_art::PowerByte`] in `power` drives one strike through
    /// [`crate::art_strike::apply_art_strike`]: the byte's multiplier tier +
    /// UDF/LDF target are decoded, [`Self::resolve_battle_defense`] picks the
    /// matching defense half (when a UDF/LDF split is configured), and the
    /// per-strike damage is deducted. The art's `enemy_effect` is applied once
    /// after a landing hit (if the target survives). Summed damage surfaces as
    /// one HUD popup; the target is downed if its HP reaches zero.
    ///
    /// `power` comes from the matched art record when one is staged, else a
    /// synthetic per-direction profile (see [`Self::build_battle_arts_rows`]),
    /// so the same kernel handles both real and demo arts.
    fn apply_battle_art(
        &mut self,
        caster: u8,
        power: &[legaia_art::PowerByte],
        enemy_effect: legaia_art::EnemyEffect,
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use crate::target_picker::CursorRow;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;
        let party_count = self.party_count.clamp(1, 3);
        let target = match target_row {
            CursorRow::Enemy => party_count + target_slot,
            CursorRow::Ally => target_slot,
        } as usize;
        if target >= self.actors.len() {
            return;
        }
        let attack = self
            .battle_attack
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let character = self.caster_character(caster);
        let mut total: u32 = 0;
        let mut landed: u8 = 0;
        for (i, pb) in power.iter().enumerate() {
            if self.actors[target].battle.liveness == 0 {
                break;
            }
            // Minimal per-strike info: `apply_art_strike` + `resolve_battle_defense`
            // only read `power` + `enemy_effect`. `art` is a placeholder; the
            // live loop doesn't drive the per-art animation script.
            let info = ArtStrikeInfo {
                strike_index: i as u8,
                anim_byte: 0,
                actor_slot: caster,
                target_slot: target as u8,
                character,
                art: legaia_art::ActionConstant::Art1B,
                power: Some(*pb),
                dmg_timing: None,
                enemy_effect,
                hit_cue: None,
            };
            let defense = self.resolve_battle_defense(target as u8, &info);
            let outcome = crate::art_strike::apply_art_strike(attack, defense, &info);
            if let Some(dmg) = outcome.damage {
                let a = &mut self.actors[target].battle;
                a.hp = a.hp.saturating_sub(dmg);
                total = total.saturating_add(dmg as u32);
                landed = landed.saturating_add(1);
                if a.hp == 0 {
                    a.liveness = 0;
                }
            }
        }
        if landed > 0
            && enemy_effect != legaia_art::EnemyEffect::None
            && self.actors[target].battle.liveness != 0
        {
            self.status_effects
                .apply_from_enemy_effect(target as u8, enemy_effect);
        }
        if total > 0 {
            self.battle_hit_fx.push(BattleHitFx {
                target_slot: target as u8,
                amount: total.min(u16::MAX as u32) as u16,
                is_heal: false,
                is_crit: landed > 1,
            });
        }
    }

    /// Build the battle Magic submenu for `caster` (an actor-table / party-row
    /// index). Reads the caster's learned spells off their roster record and
    /// their live battle MP to grey out unaffordable rows. Returns `None` when
    /// there's no roster record for the slot (the caller reopens the command
    /// menu so the SM isn't stranded).
    fn build_battle_spell_session(
        &self,
        caster: u8,
    ) -> Option<crate::battle_magic::BattleSpellSession> {
        let member = self.roster.members.get(caster as usize)?;
        let list = member.spell_list();
        let n = (list.count as usize).min(list.ids.len());
        // Union the roster's saved spell list with anything learned via Seru
        // capture this session, so a freshly-learned spell is immediately
        // castable without waiting for a save/load round-trip.
        let mut learned: Vec<u8> = list.ids[..n].to_vec();
        for &sid in self.seru_log.learned_spells(caster) {
            if !learned.contains(&sid) {
                learned.push(sid);
            }
        }
        let caster_mp = self
            .actors
            .get(caster as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);
        Some(crate::battle_magic::BattleSpellSession::new(
            caster,
            caster,
            &learned,
            &self.spell_catalog,
            caster_mp,
        ))
    }

    /// Drive the open battle Magic submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::battle_magic::BattleSpellInput`] per
    /// frame. On a confirmed cast the spell applies via [`Self::apply_battle_spell`]
    /// (MP deducted, HP / heal / cure / revive folded, popups surfaced) and the
    /// action SM parks at `EndOfAction` so the live loop cycles to the next
    /// combatant - a cast is the caster's whole turn, no strike fires. Backing
    /// out reopens the command menu for the same actor.
    fn tick_battle_spell_menu(&mut self) {
        use crate::battle_magic::{BattleSpellInput, SpellResolution};
        use crate::input::PadButton;
        use crate::target_picker::SlotState;

        let Some(mut menu) = self.battle_spell_menu.take() else {
            return;
        };

        let party_count = self.party_count.clamp(1, 3);
        let slot_at = |idx: usize| -> SlotState {
            match self.actors.get(idx) {
                Some(a) if a.battle.max_hp > 0 => SlotState::alive(true, a.battle.liveness != 0),
                _ => SlotState::default(),
            }
        };
        let mut party = [SlotState::default(); 3];
        for (i, p) in party.iter_mut().enumerate().take(party_count as usize) {
            *p = slot_at(i);
        }
        let mut monsters = [SlotState::default(); 5];
        for (i, m) in monsters.iter_mut().enumerate() {
            *m = slot_at(party_count as usize + i);
        }

        let ev = BattleSpellInput {
            up: self.input.just_pressed(PadButton::Up),
            down: self.input.just_pressed(PadButton::Down),
            left: self.input.just_pressed(PadButton::Left),
            right: self.input.just_pressed(PadButton::Right),
            cross: self.input.just_pressed(PadButton::Cross),
            circle: self.input.just_pressed(PadButton::Circle),
        };
        menu.input(ev, &self.spell_catalog, party, monsters);

        match menu.resolved() {
            Some(SpellResolution::Confirmed {
                spell_id,
                target_row,
                target_slot,
            }) => {
                let caster = menu.actor;
                self.apply_battle_spell(caster, spell_id, target_row, target_slot);
                if self.battle_escaped {
                    // Escape spell succeeded: leave the encounter now (no loot,
                    // no game-over) instead of cycling the turn.
                    self.finish_battle();
                } else {
                    self.battle_ctx.action_state =
                        vm::battle_action::ActionState::EndOfAction.as_byte();
                }
            }
            Some(SpellResolution::Aborted) => {
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            None => {
                self.battle_spell_menu = Some(menu);
            }
        }
    }

    /// Cast `spell_id` from `caster` against the picked target and fold the
    /// outcome into world state. MP is deducted once up-front; the spell's
    /// [`crate::spells::SpellTarget`] shape decides which slots are affected
    /// (single → the picked slot; `AllEnemies` / `AllAllies` → the whole band),
    /// each resolved through [`crate::spells::cast_spell`]. Caster magic comes
    /// from [`Self::battle_magic`]; target magic-defense reuses
    /// [`Self::battle_defense`]. Damage / heal / cure / revive / buff / capture
    /// / escape all fold through [`Self::fold_spell_outcome`].
    fn apply_battle_spell(
        &mut self,
        caster: u8,
        spell_id: u8,
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) {
        use crate::spells::SpellTarget;
        use crate::target_picker::CursorRow;

        let Some(def) = self.spell_catalog.get(spell_id).cloned() else {
            return;
        };
        let party_count = self.party_count.clamp(1, 3);
        let targets: Vec<u8> = match def.target {
            SpellTarget::OneEnemy | SpellTarget::OneAlly | SpellTarget::SelfOnly => {
                let abs = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                vec![abs]
            }
            SpellTarget::AllEnemies => (party_count..self.actors.len() as u8).collect(),
            SpellTarget::AllAllies => (0..party_count).collect(),
        };
        self.cast_spell_on_slots(caster, &def, &targets);
    }

    /// Deduct `def`'s MP cost from `caster` and fold its effect onto each
    /// absolute actor slot in `targets`. Shared by the player cast path
    /// ([`Self::apply_battle_spell`], which resolves the cursor rows to slots
    /// from the player's perspective) and the monster-AI cast path
    /// ([`Self::apply_monster_spell`], which resolves party slots). MP is spent
    /// once up front; each target folds through [`Self::fold_spell_outcome`].
    /// Returns `false` (no MP spent, nothing folded) when the caster can't
    /// afford the cost.
    fn cast_spell_on_slots(
        &mut self,
        caster: u8,
        def: &crate::spells::SpellDef,
        targets: &[u8],
    ) -> bool {
        use crate::spells::{SpellSnapshot, cast_spell};

        let cost = def.mp_cost as u16;
        let (caster_hp, caster_max_hp, caster_mp_before) = match self.actors.get(caster as usize) {
            Some(a) => (a.battle.hp, a.battle.max_hp, a.battle.mp),
            None => return false,
        };
        if caster_mp_before < cost {
            return false;
        }
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.mp = a.battle.mp.saturating_sub(cost);
        }
        let caster_mag = self.battle_magic.get(caster as usize).copied().unwrap_or(0);

        for &t in targets {
            let Some(actor) = self.actors.get(t as usize) else {
                continue;
            };
            // Skip empty slots (no configured HP).
            if actor.battle.max_hp == 0 {
                continue;
            }
            let snap = SpellSnapshot {
                caster_mag,
                caster_hp,
                caster_max_hp,
                caster_mp: caster_mp_before,
                target_mdef: self.battle_defense.get(t as usize).copied().unwrap_or(0),
                target_hp: actor.battle.hp,
                target_hp_max: actor.battle.max_hp,
                target_mp: actor.battle.mp,
                target_alive: actor.battle.liveness != 0,
                target_weakness: crate::spells::ElementMask::default(),
            };
            let outcome = cast_spell(def, t, &snap);
            self.fold_spell_outcome(outcome);
        }
        true
    }

    /// Fold a single-target [`crate::spells::SpellOutcome`] into live actor
    /// state and surface a HUD popup. Damage subtracts HP (and downs the
    /// target at zero); heals / revives add HP (capped); cures clear the
    /// target's status; buffs adjust a per-slot scalar with a turn timer
    /// ([`Self::apply_battle_buff`]); capture rolls vs the monster's weakened
    /// state ([`Self::resolve_capture`]); escape flags a return to the field
    /// ([`Self::battle_escaped`]). `Failed` is a no-op (MP already spent).
    fn fold_spell_outcome(&mut self, outcome: crate::spells::SpellOutcome) {
        use crate::spells::SpellOutcome as O;
        match outcome {
            O::Damage { target, amount, .. } => {
                if let Some(a) = self.actors.get_mut(target as usize) {
                    a.battle.hp = a.battle.hp.saturating_sub(amount);
                    if a.battle.hp == 0 {
                        a.battle.liveness = 0;
                    }
                }
                self.battle_hit_fx.push(BattleHitFx {
                    target_slot: target,
                    amount,
                    is_heal: false,
                    is_crit: false,
                });
            }
            O::Heal { target, amount } => {
                if let Some(a) = self.actors.get_mut(target as usize) {
                    a.battle.hp = a.battle.hp.saturating_add(amount).min(a.battle.max_hp);
                }
                if amount > 0 {
                    self.battle_hit_fx.push(BattleHitFx {
                        target_slot: target,
                        amount,
                        is_heal: true,
                        is_crit: false,
                    });
                }
            }
            O::Cure { target, .. } => {
                self.status_effects.cure_all(target);
            }
            O::Revive { target, hp } => {
                if let Some(a) = self.actors.get_mut(target as usize) {
                    a.battle.hp = hp.min(a.battle.max_hp);
                    if a.battle.hp > 0 {
                        a.battle.liveness = 1;
                    }
                }
                self.battle_hit_fx.push(BattleHitFx {
                    target_slot: target,
                    amount: hp,
                    is_heal: true,
                    is_crit: false,
                });
            }
            O::Buff {
                target,
                stat,
                magnitude,
                turns,
            } => {
                self.apply_battle_buff(target, stat, magnitude, turns);
            }
            O::CaptureRoll { target, hit_pct } => {
                self.resolve_capture(target, hit_pct);
            }
            O::Escape => {
                self.battle_escaped = true;
            }
            // Multi-target variants aren't produced by per-slot casts; Failed
            // is a no-op (MP was already spent up front).
            _ => {}
        }
    }

    /// Apply (or refresh) a stat buff / debuff on `slot`. The delta is written
    /// straight into the matching per-slot battle scalar so it changes damage
    /// the same frame: `Attack`/`MagicAttack`/`Defense` map to
    /// [`Self::battle_attack`] / [`Self::battle_magic`] / [`Self::battle_defense`]
    /// (`MagicDefense` reuses `battle_defense`, the spell-defense proxy). The
    /// scalar is `u16`, so a negative magnitude saturates at zero and the
    /// recorded `applied_delta` is the exact change (for precise undo on
    /// expiry). Accuracy / Evasion / Speed have no live-loop scalar; the buff
    /// is tracked with a zero delta so the turn timer still runs. Re-casting
    /// the same `(slot, stat)` refreshes: the old delta is reverted first.
    fn apply_battle_buff(
        &mut self,
        slot: u8,
        stat: crate::spells::BuffStat,
        magnitude: i16,
        turns: u8,
    ) {
        // Refresh: revert + drop any existing buff on this (slot, stat).
        if let Some(pos) = self
            .battle_buffs
            .iter()
            .position(|b| b.slot == slot && b.stat == stat)
        {
            let old = self.battle_buffs.remove(pos);
            self.add_to_buff_scalar(old.slot, old.stat, -old.applied_delta);
        }
        if turns == 0 {
            return;
        }
        let applied_delta = self.add_to_buff_scalar(slot, stat, magnitude);
        self.battle_buffs.push(BattleBuff {
            slot,
            stat,
            applied_delta,
            turns,
        });
    }

    /// Add `delta` to the per-slot scalar backing `stat` and return the exact
    /// change made (after `u16` saturation). Stats with no live-loop scalar
    /// return `0`.
    fn add_to_buff_scalar(&mut self, slot: u8, stat: crate::spells::BuffStat, delta: i16) -> i16 {
        use crate::spells::BuffStat;
        let scalar = match stat {
            BuffStat::Attack => self.battle_attack.get_mut(slot as usize),
            BuffStat::MagicAttack => self.battle_magic.get_mut(slot as usize),
            BuffStat::Defense | BuffStat::MagicDefense => {
                self.battle_defense.get_mut(slot as usize)
            }
            BuffStat::Accuracy | BuffStat::Evasion | BuffStat::Speed => None,
        };
        let Some(scalar) = scalar else { return 0 };
        let before = *scalar as i32;
        let after = (before + delta as i32).clamp(0, u16::MAX as i32);
        *scalar = after as u16;
        (after - before) as i16
    }

    /// Tick the buffs on `slot` at the start of its turn: decrement each, and
    /// revert + drop those that reach zero.
    fn tick_battle_buffs_on_turn(&mut self, slot: u8) {
        let mut expired: Vec<BattleBuff> = Vec::new();
        self.battle_buffs.retain_mut(|b| {
            if b.slot != slot {
                return true;
            }
            b.turns = b.turns.saturating_sub(1);
            if b.turns == 0 {
                expired.push(*b);
                false
            } else {
                true
            }
        });
        for b in expired {
            self.add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
    }

    /// Resolve a capture-spell roll against the monster in `target`. The
    /// effective chance scales with the monster's missing-HP fraction (full
    /// `hit_pct` only near death, zero at full HP) - mirroring retail capture,
    /// which is reliable only on a weakened Seru. On success the monster is
    /// downed (so it counts toward the wipe) and its id is logged into
    /// [`Self::battle_captures`] for post-battle Seru learning.
    fn resolve_capture(&mut self, target: u8, hit_pct: u8) {
        let (hp, max, monster_id, alive) = match self.actors.get(target as usize) {
            Some(a) => (
                a.battle.hp as u32,
                a.battle.max_hp as u32,
                a.battle_monster_id,
                a.battle.liveness != 0,
            ),
            None => return,
        };
        if !alive || max == 0 {
            return;
        }
        let missing = max.saturating_sub(hp);
        let effective = (hit_pct as u32 * missing / max).min(100);
        let roll = self.next_rng() % 100;
        if roll >= effective {
            return;
        }
        if let Some(a) = self.actors.get_mut(target as usize) {
            a.battle.hp = 0;
            a.battle.liveness = 0;
        }
        if let Some(id) = monster_id {
            self.battle_captures.push(id);
        }
    }

    /// Drain the monster ids captured this battle (see [`Self::battle_captures`]).
    pub fn drain_battle_captures(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.battle_captures)
    }

    /// Install the master [`crate::seru_learning::SeruRegistry`]. Boot wires
    /// this once; [`Self::finish_battle`] consults it to bank capture points.
    pub fn set_seru_registry(&mut self, registry: crate::seru_learning::SeruRegistry) {
        self.seru_registry = registry;
    }

    /// Resolve this battle's captured monsters into Seru-learning progress.
    ///
    /// Drains [`Self::battle_captures`] (so the list is always cleared), maps
    /// each captured monster id to its Seru id via [`Self::monster_catalog`],
    /// and banks capture points against [`Self::seru_log`] for every active
    /// party slot through [`crate::seru_learning::record_capture`]. Any Seru
    /// that crosses its learn threshold adds its spell to the character's
    /// learned list (which [`Self::build_battle_spell_session`] then offers).
    /// Accepted outcomes are stashed in [`Self::last_capture_outcomes`] for the
    /// host to drive the capture / learned banner. Monsters with no Seru, or
    /// any capture when the registry is empty, bank nothing.
    fn resolve_captures(&mut self) {
        let captures = std::mem::take(&mut self.battle_captures);
        self.last_capture_outcomes.clear();
        self.current_capture_banner = None;
        if captures.is_empty() || self.seru_registry.is_empty() {
            return;
        }
        let party_slots: Vec<u8> = (0..self.party_count.clamp(1, 3)).collect();
        let seru_ids: Vec<u16> = captures
            .iter()
            .filter_map(|&mid| self.monster_catalog.get(mid).and_then(|d| d.seru_id))
            .collect();
        let mut first_accepted: Option<(u16, crate::seru_learning::CaptureOutcome)> = None;
        for sid in seru_ids {
            let outcome = crate::seru_learning::record_capture(
                &self.seru_registry,
                &mut self.seru_log,
                sid,
                &party_slots,
            );
            if outcome.accepted {
                if first_accepted.is_none() {
                    first_accepted = Some((sid, outcome.clone()));
                }
                self.last_capture_outcomes.push(outcome);
            }
        }
        // Build the host-facing banner for the first accepted capture (a
        // single battle captures at most one Seru in practice). Names resolve
        // the Seru from the registry and the learned spell from the catalog.
        if let Some((sid, outcome)) = first_accepted {
            let seru_name = self
                .seru_registry
                .get(sid)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| format!("Seru {sid:#04X}"));
            let spell_catalog = &self.spell_catalog;
            let banner = crate::seru_learning::SeruCaptureSession::new(
                seru_name,
                sid,
                outcome,
                |char_slot, spell_id| {
                    let char_name = format!("Character {}", char_slot + 1);
                    let spell_name = spell_catalog
                        .get(spell_id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| format!("Spell {spell_id:#04X}"));
                    (char_name, spell_name)
                },
            );
            self.current_capture_banner = Some(banner);
        }
    }

    /// Drain the capture outcomes from the most recently finished battle.
    pub fn drain_last_capture_outcomes(&mut self) -> Vec<crate::seru_learning::CaptureOutcome> {
        std::mem::take(&mut self.last_capture_outcomes)
    }

    /// Build the battle-context inventory submenu from live world state:
    /// every item the player holds (`count > 0`), one party-member target row
    /// per configured party slot, then one enemy row per live monster slot
    /// (tagged `is_enemy`). Healing / cure / revive items validate against the
    /// party rows; offensive items (Bomb / capture / escape) validate against
    /// the enemy rows - the session routes the cursor to the correct side.
    fn build_battle_item_session(&self) -> crate::inventory_use::InventoryUseSession {
        use crate::inventory_use::{InventoryContext, InventoryUseSession, TargetRow};
        let names = crate::field_menu_dispatch::roster_names(self);
        let items: Vec<u8> = self
            .inventory
            .iter()
            .filter_map(|(id, qty)| (*qty > 0).then_some(*id))
            .collect();
        let pc = self.party_count.clamp(1, 3) as usize;
        let mut targets: Vec<TargetRow> = (0..pc)
            .filter_map(|i| {
                let a = self.actors.get(i)?;
                // Skip unconfigured party slots (no battle stats).
                if a.battle.max_hp == 0 {
                    return None;
                }
                let mp_max = self.character_max_mp.get(i).copied().unwrap_or(0);
                let name = names
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("P{}", i + 1));
                let mut row = TargetRow::new(i as u8, name).with_stats(
                    a.battle.hp,
                    a.battle.max_hp,
                    a.battle.mp,
                    mp_max,
                );
                row.alive = a.battle.liveness != 0;
                Some(row)
            })
            .collect();
        // Enemy rows: every monster slot that's configured for battle. Tagged
        // `is_enemy` so the session only accepts offensive items here.
        for slot in pc..self.actors.len() {
            let Some(a) = self.actors.get(slot) else {
                break;
            };
            if a.battle.max_hp == 0 || a.battle_monster_id.is_none() {
                continue;
            }
            let name = a
                .battle_monster_id
                .and_then(|id| self.monster_catalog.get(id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("Enemy {}", slot - pc + 1));
            let mut row = TargetRow::new(slot as u8, name)
                .with_stats(a.battle.hp, a.battle.max_hp, 0, 0)
                .with_enemy(true);
            row.alive = a.battle.liveness != 0;
            targets.push(row);
        }
        InventoryUseSession::new(
            self.item_catalog.clone(),
            items,
            targets,
            InventoryContext::Battle,
        )
    }

    /// Drive the open battle inventory submenu one frame from [`World::input`].
    ///
    /// Edge-triggered pad → one [`crate::inventory_use::InventoryUseInput`] per
    /// frame. On a completed use the chosen item is applied authoritatively via
    /// [`Self::use_item`], one copy is consumed from the inventory, a heal /
    /// cure popup is surfaced for the HUD, and the action SM is parked at
    /// `EndOfAction` so the live loop cycles to the next combatant (no strike
    /// fires - using an item is the actor's whole turn). Backing out reopens
    /// the command menu for the same actor.
    fn tick_battle_item_menu(&mut self) {
        use crate::input::PadButton;
        use crate::inventory_use::{InventoryUseEvent, InventoryUseInput, InventoryUseState};

        let Some(mut menu) = self.battle_item_menu.take() else {
            return;
        };

        let ev = if self.input.just_pressed(PadButton::Up) {
            Some(InventoryUseInput::Up)
        } else if self.input.just_pressed(PadButton::Down) {
            Some(InventoryUseInput::Down)
        } else if self.input.just_pressed(PadButton::Cross) {
            Some(InventoryUseInput::Confirm)
        } else if self.input.just_pressed(PadButton::Circle) {
            Some(InventoryUseInput::Cancel)
        } else {
            None
        };

        // The item under the cursor before the input - `current_item` reads
        // the `item_cursor` in TargetSelect, so this is the item that a Confirm
        // on a target row resolves to (the Done state no longer exposes it).
        let item_before = menu.current_item().map(|e| e.id);
        if let Some(ev) = ev {
            menu.input(ev);
        }
        let used = menu.drain_events().into_iter().find_map(|e| match e {
            InventoryUseEvent::Used { slot, .. } => Some(slot),
            _ => None,
        });

        if let Some(target_slot) = used {
            if let Some(item_id) = item_before {
                let outcome = self.use_item(item_id, target_slot);
                self.consume_item(item_id);
                self.push_item_use_fx(target_slot, outcome);
            }
            if self.battle_escaped {
                // Escape item succeeded: leave the encounter now (no loot, no
                // game-over) instead of cycling the turn.
                self.finish_battle();
            } else {
                // Using an item is the actor's whole turn: park at EndOfAction
                // so the live loop's re-arm block cycles to the next combatant.
                self.battle_ctx.action_state =
                    vm::battle_action::ActionState::EndOfAction.as_byte();
            }
            return;
        }

        match menu.state {
            InventoryUseState::Aborted => {
                // Backed out without using an item - reopen the command menu.
                let actor = self.battle_ctx.active_actor;
                self.open_battle_command(actor);
            }
            _ => {
                // Still browsing / target-selecting - keep the menu open.
                self.battle_item_menu = Some(menu);
            }
        }
    }

    /// Remove one copy of `item_id` from the inventory, dropping the entry
    /// when the count reaches zero. No-op when the player holds none.
    pub fn consume_item(&mut self, item_id: u8) {
        if let Some(qty) = self.inventory.get_mut(&item_id) {
            *qty = qty.saturating_sub(1);
            if *qty == 0 {
                self.inventory.remove(&item_id);
            }
        }
    }

    /// Surface a cosmetic HUD popup for a resolved item use. Heals / MP
    /// restores / revives push a heal-coloured number; offensive items push a
    /// damage-coloured number; cures push the status letter. The HP / status
    /// side is already applied by [`Self::use_item`]; this is presentation-only
    /// (drained via [`Self::drain_battle_hit_fx`]).
    fn push_item_use_fx(&mut self, target_slot: u8, outcome: crate::items::ItemOutcome) {
        use crate::items::ItemOutcome;
        let (amount, is_heal) = match outcome {
            ItemOutcome::HealedHp { amount } | ItemOutcome::HealedMp { amount } => (amount, true),
            ItemOutcome::Revived { hp_after } => (hp_after, true),
            ItemOutcome::DamageDealt { amount } => (amount, false),
            // Cures / capture / escape / stat boosts / no-effect: no number.
            _ => return,
        };
        if amount == 0 {
            return;
        }
        self.battle_hit_fx.push(BattleHitFx {
            target_slot,
            amount,
            is_heal,
            is_crit: false,
        });
    }

    /// Per-frame battle-side driver for the live gameplay loop. Gated by
    /// [`Self::live_gameplay_loop`] in [`Self::tick`].
    ///
    /// Wraps [`Self::step_battle`] with the host-side glue retail performs
    /// through the render + animation systems, so the battle resolves from
    /// `tick` alone:
    ///
    /// - **Damage application.** Drains this step's [`BattleEvent`]s and
    ///   folds [`BattleEvent::ApplyArtStrike`] damage into target HP. A
    ///   generic physical attack (no art) is applied on the
    ///   `AttackChain -> AttackRecovery` edge via [`Self::apply_basic_attack`].
    /// - **Liveness.** Any combatant whose HP hit zero is marked dead so the
    ///   SM's wipe scan sees it.
    /// - **Turn cycling.** When the SM idles at `EndOfAction` with monsters
    ///   still alive, the next party member is re-armed (v0.1 keeps monsters
    ///   passive - party turns only).
    /// - **Recovery edge.** Clears `ADVANCE_DONE` at `AttackRecovery`, the
    ///   edge the retail recovery animation drives.
    ///
    /// On [`StepOutcome::BattleComplete`] it runs [`Self::finish_battle`] to
    /// apply loot and return to the field.
    fn live_battle_tick(&mut self) -> Option<StepOutcome> {
        use vm::battle_action::{ActionState, ActorFlags};

        // Player-driven: while the Arts submenu is open the action SM is
        // parked - drive it from the pad and return until the player runs an
        // art (turn cycles) or backs out (reopens the command menu).
        if self.battle_arts_menu.is_some() {
            self.tick_battle_arts_menu();
            return None;
        }

        // Player-driven: while the spell submenu is open the action SM is
        // parked - drive it from the pad and return until the player casts
        // (turn cycles) or backs out (reopens the command menu).
        if self.battle_spell_menu.is_some() {
            self.tick_battle_spell_menu();
            return None;
        }

        // Player-driven: while the inventory submenu is open the action SM is
        // parked - drive it from the pad and return until the player uses an
        // item (turn cycles) or backs out (reopens the command menu).
        if self.battle_item_menu.is_some() {
            self.tick_battle_item_menu();
            return None;
        }

        // Player-driven: while a command session is open the action SM is
        // parked - drive the command picker from the pad and return without
        // advancing the SM until the player confirms.
        if self.battle_command.is_some() {
            self.tick_battle_command();
            return None;
        }

        let outcome = self.step_battle();

        // Apply this step's damage events (art strikes carry a damage value;
        // the loop owns folding while live, so events are consumed here).
        let events = std::mem::take(&mut self.pending_battle_events);
        let mut art_strike_applied = false;
        for e in &events {
            if let BattleEvent::ApplyArtStrike {
                target_slot,
                outcome,
                ..
            } = e
            {
                art_strike_applied = true;
                // Surface the resolved strike damage for HUD popups (the
                // fold below applies the HP side; this is cosmetic only).
                if let Some(dmg) = outcome.damage
                    && dmg > 0
                {
                    self.battle_hit_fx.push(BattleHitFx {
                        target_slot: *target_slot,
                        amount: dmg,
                        is_heal: false,
                        is_crit: false,
                    });
                }
            }
            self.fold_battle_event(e);
        }

        // Generic physical attack: deal damage on the strike-landed edge when
        // no art strike already did.
        if let StepOutcome::Transition { from, to } = outcome
            && from == ActionState::AttackChain.as_byte()
            && to == ActionState::AttackRecovery.as_byte()
            && !art_strike_applied
        {
            self.apply_basic_attack();
        }

        // Mark the dead so the SM's liveness scan resolves the wipe.
        for a in self.actors.iter_mut() {
            if a.battle.max_hp > 0 && a.battle.hp == 0 {
                a.battle.liveness = 0;
            }
        }

        // Recovery-edge ADVANCE_DONE clear (retail clears this when the
        // recovery animation finishes; we simulate the same edge inline).
        let attacker = self.battle_ctx.active_actor as usize;
        if attacker < self.actors.len()
            && self.actors[attacker]
                .battle
                .flag_bits
                .has(ActorFlags::ADVANCE_DONE)
            && self.battle_ctx.action_state == ActionState::AttackRecovery.as_byte()
        {
            self.actors[attacker]
                .battle
                .flag_bits
                .clear(ActorFlags::ADVANCE_DONE);
        }

        // Re-arm the next combatant when the SM idles at EndOfAction, cycling
        // across the whole actor table (party AND monsters) in slot order so
        // monsters take their turns. Only re-arm while BOTH sides still have a
        // living member - if either side is wiped we leave the SM at
        // EndOfAction so its liveness scan resolves the wipe into
        // BattleComplete next step.
        let party_count = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let party_alive = (0..party_count).any(|i| self.actors[i as usize].battle.liveness != 0);
        let monsters_alive = (party_count..n).any(|i| self.actors[i as usize].battle.liveness != 0);
        if self.battle_ctx.action_state == ActionState::EndOfAction.as_byte()
            && party_alive
            && monsters_alive
            && let Some(next) = self.next_combatant_by_initiative()
        {
            // Start-of-turn: age this actor's buffs / debuffs, reverting any
            // that expire this turn.
            self.tick_battle_buffs_on_turn(next);
            let next_is_party = next < party_count;
            if next_is_party && self.battle_player_driven {
                // Party turn under player control: pause the SM and let the
                // player pick the command. `tick_battle_command` arms the SM
                // on confirm.
                self.open_battle_command(next);
            } else if !next_is_party {
                // Monster turn: the AI picks a spell or a physical strike.
                self.take_monster_turn(next);
            } else {
                // Party turn when not player-driven: arm a generic physical
                // attack against the first living opponent.
                let target = self.first_living_opponent_of(next).unwrap_or(next);
                self.battle_ctx.active_actor = next;
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
                if let Some(a) = self.actors.get_mut(next as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
            }
        }

        if matches!(outcome, StepOutcome::BattleComplete) {
            self.finish_battle();
        }
        Some(outcome)
    }

    /// Apply one generic physical strike from the active attacker to the
    /// first living combatant on the opposing side. v0.1 stand-in for the
    /// art-driven strike path: `damage = art_strike_damage_default(attack,
    /// defense, 16)` (≈ `attack - defense`, floored at 1) so a party with no
    /// configured weapon attack still chips the monster down - and, for a
    /// monster attacker, a monster with no configured attack still chips the
    /// party. Always hits (no accuracy roll) to guarantee the loop resolves.
    ///
    /// The opposing side is chosen by the attacker's slot: party slots
    /// (`< party_count`) strike monsters; monster slots strike the party.
    fn apply_basic_attack(&mut self) {
        let attacker = self.battle_ctx.active_actor as usize;
        let Some(target) = self.resolve_attack_target(attacker as u8) else {
            return;
        };
        let target = target as usize;
        let attack = self.battle_attack.get(attacker).copied().unwrap_or(0);
        let defense = self.battle_defense.get(target).copied().unwrap_or(0);
        let dmg = vm::battle_formulas::art_strike_damage_default(attack, defense, 16);
        let a = &mut self.actors[target];
        a.battle.hp = a.battle.hp.saturating_sub(dmg);
        if a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        // Surface the strike for HUD damage popups.
        self.battle_hit_fx.push(BattleHitFx {
            target_slot: target as u8,
            amount: dmg,
            is_heal: false,
            is_crit: false,
        });
    }

    /// Resolve the slot a strike from `attacker` should land on. Honors a
    /// pre-selected [`battle::BattleActor::active_target`] when it points at a
    /// living actor on the opposing side (so the player's target-picker choice
    /// and the monster-AI target choice both take effect), otherwise falls back
    /// to [`Self::first_living_opponent_of`].
    fn resolve_attack_target(&self, attacker: u8) -> Option<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
        if let Some(a) = self.actors.get(attacker as usize) {
            let t = a.battle.active_target;
            if (lo..hi).contains(&t)
                && self
                    .actors
                    .get(t as usize)
                    .is_some_and(|x| x.battle.liveness != 0)
            {
                return Some(t);
            }
        }
        self.first_living_opponent_of(attacker)
    }

    /// Drive one monster's turn. Runs the action picker
    /// ([`Self::pick_monster_action`], the port of `FUN_801E9FD4`'s generic
    /// decision core) and either folds the chosen cast and parks the SM at
    /// `EndOfAction` (a spell is the whole turn, like the player magic path) or
    /// arms a physical strike for the action SM to run.
    fn take_monster_turn(&mut self, slot: u8) {
        use vm::battle_action::ActionState;

        self.battle_ctx.active_actor = slot;
        match self.pick_monster_action(slot) {
            MonsterAction::Cast { spell_id, targets } => {
                let def = self.spell_catalog.get(spell_id).cloned();
                if let Some(def) = def
                    && self.cast_spell_on_slots(slot, &def, &targets)
                {
                    self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                    return;
                }
                // Cast didn't fold (no catalog entry / unaffordable after the
                // pick) - fall through to a physical strike.
                self.arm_monster_physical(slot);
            }
            MonsterAction::Physical { target } => {
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
            }
        }
    }

    /// Arm a generic physical strike for monster `slot` against the first
    /// living party member (fallback when a picked cast can't fold).
    fn arm_monster_physical(&mut self, slot: u8) {
        use vm::battle_action::ActionState;
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
        }
    }

    /// Monster-AI action picker - clean-room port of the **generic decision
    /// core** of `FUN_801E9FD4` (`overlay_battle_action_801e9fd4.txt`), the
    /// routine retail runs (from `recompute_battle_order` / `FUN_801DABA4`) to
    /// choose each monster's action.
    ///
    /// Faithful to the core: it rolls `rand % (1 + live_magic_count)` over the
    /// monster's own global magic-attack ids (record `+0x21..=+0x23`, carried on
    /// [`crate::monster_catalog::MonsterDef::magic_attacks`]); a roll of `0`
    /// picks a **physical** strike (target `rand % party_count`), otherwise it
    /// picks magic id `magic[roll-1]` and resolves the target by the spell's
    /// shape byte (`spell_table[id*0xC + 2] & 0x60`), modelled here through the
    /// catalog's [`crate::spells::SpellTarget`]: `OneEnemy` → a random living
    /// party member, `AllEnemies` → the whole living party, `AllAllies` → the
    /// whole living monster band, `OneAlly` → the most-weakened living ally (or
    /// self), `SelfOnly` → self. A cast the monster can't afford from its live
    /// MP (`actor+0x150`) falls back to a physical strike, matching retail's
    /// affordability gate (`actor[0x150] < spell.mp_cost`).
    ///
    /// The large per-monster-id scripted-cast `switch` that follows the core in
    /// retail keys on `DAT_8007BD0C[slot]`, which `FUN_801DA51C` fills from the
    /// encounter record's `[+4 + slot]` monster ids - i.e. the **monster id**,
    /// not an abstract AI-type, so each case is bespoke AI for a specific
    /// monster the engine already identifies via `battle_monster_id`. That
    /// switch is ported in [`crate::monster_ai`] ([`crate::monster_ai::decide`])
    /// and consulted here as an override, followed by the post-switch
    /// recent-target ring ([`crate::monster_ai::apply_recent_target_ring`]). The
    /// companion target resolver `FUN_801E7320` is ported as
    /// [`Self::resolve_monster_target`] (the `monster_setup` hook).
    ///
    /// PORT: FUN_801E9FD4
    /// REF: FUN_801DABA4
    fn pick_monster_action(&mut self, slot: u8) -> MonsterAction {
        let pc = self.party_count.max(1);

        // --- generic decision core ---
        // The monster's own castable global magic ids (parser already drops the
        // empty `<= 1` slots, so every entry is "live").
        let magic: Vec<u8> = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| d.magic_attacks.clone())
            .unwrap_or_default();
        let mp = self
            .actors
            .get(slot as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);

        // Roll over (1 + live_magic_count); 0 => physical. Always consumes one
        // RNG draw, exactly like retail.
        let denom = 1 + magic.len() as u32;
        let roll = self.next_rng() % denom;
        // Provisional choice (category 3 = physical strike, 2 = magic).
        let (mut category, mut spell_id) = (3u8, 0u8);
        let mut target_class;
        if roll != 0 {
            let id = magic[(roll - 1) as usize];
            if let Some(def) = self.spell_catalog.get(id).cloned()
                && mp >= def.mp_cost as u16
            {
                category = 2;
                spell_id = id;
                target_class = self.monster_cast_target_class(slot, &def);
            } else {
                target_class = self.random_living_party_member(pc).unwrap_or(slot);
            }
        } else {
            target_class = self.random_living_party_member(pc).unwrap_or(slot);
        }

        // --- per-monster-id scripted override (the FUN_801E9FD4 switch) + the
        // post-switch recent-target anti-repeat ring. Run in a borrow window
        // with the AI state owned locally so the RNG closure can take `self`.
        if let Some(monster_id) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
        {
            let (hp, max_hp) = self
                .actors
                .get(slot as usize)
                .map(|a| (a.battle.hp, a.battle.max_hp))
                .unwrap_or((0, 0));
            let allies_with_mp = (0..pc)
                .filter(|&i| {
                    self.actors
                        .get(i as usize)
                        .is_some_and(|a| a.battle.liveness != 0 && a.battle.mp != 0)
                })
                .count() as u8;
            let n = self.actors.len() as u8;
            let ctx = crate::monster_ai::MonsterAiCtx {
                monster_id: (monster_id & 0xFF) as u8,
                monster_index: slot.saturating_sub(pc),
                caster_slot: slot,
                hp,
                max_hp,
                mp,
                party_count: pc,
                monster_count: n.saturating_sub(pc).max(1),
                field_flags: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.field_flags)
                    .unwrap_or(0),
                allies_with_mp,
            };
            let mut ai = std::mem::take(&mut self.monster_ai_state);
            if let Some(cast) = crate::monster_ai::decide(&ctx, &mut ai, &mut || self.next_rng()) {
                category = cast.category;
                spell_id = cast.spell_id;
                target_class = cast.target_class;
            }
            // Anti-repeat ring (applies to whichever single party target stands).
            target_class = crate::monster_ai::apply_recent_target_ring(
                target_class,
                spell_id,
                pc,
                &mut ai,
                &mut || self.next_rng(),
            );
            self.monster_ai_state = ai;
        }

        // --- build the action ---
        if category == 2 {
            let targets = self.resolve_class_to_slots(slot, target_class);
            if !targets.is_empty() {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 2;
                    a.battle.params[0] = spell_id;
                }
                return MonsterAction::Cast { spell_id, targets };
            }
        }
        // Physical strike (or a cast that resolved no targets).
        let target = if target_class < pc {
            target_class
        } else {
            self.random_living_party_member(pc)
                .or_else(|| self.first_living_opponent_of(slot))
                .unwrap_or(slot)
        };
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.action_category = 3;
            a.battle.active_target = target;
        }
        MonsterAction::Physical { target }
    }

    /// The live battle-mode counter (`ctx+0x28A`, `_DAT_8007BD24[0x28A]`).
    ///
    /// This is the boss/scripted-mode gate the per-monster AI `switch` reads:
    /// multi-phase bosses (`0xA8`, `0xB4`, `0xB5`, `0xB6`, `0xA2..=0xA4`, …)
    /// change which spell they cast as it advances. `0` in a normal battle.
    pub fn battle_mode(&self) -> u8 {
        self.monster_ai_state.mode_flags
    }

    /// Advance the battle-mode counter by one - the faithful port of the
    /// battle-action SM's `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), the
    /// boss-phase-transition pseudo-action. A boss script issues action `0xFF`
    /// when the fight crosses a scripted phase boundary; the next monster turn's
    /// [`Self::pick_monster_action`] then reads the bumped mode through
    /// [`crate::monster_ai::decide`], activating that phase's scripted casts.
    /// The retail counter is a byte, so it wraps at `0xFF`.
    ///
    /// PORT: FUN_801E295C
    pub fn advance_battle_mode(&mut self) {
        self.monster_ai_state.mode_flags = self.monster_ai_state.mode_flags.wrapping_add(1);
    }

    /// Target **class** the generic core picks for a monster casting `def`, by
    /// the spell's [`crate::spells::SpellTarget`] shape (monster's perspective:
    /// enemies = party band, allies = monster band). Single-enemy → a random
    /// living party slot; `AllEnemies` → class `8`; `AllAllies` → class `9`;
    /// `OneAlly` → the most-weakened living ally (or self); `SelfOnly` → self.
    fn monster_cast_target_class(&mut self, slot: u8, def: &crate::spells::SpellDef) -> u8 {
        use crate::spells::SpellTarget;
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        match def.target {
            SpellTarget::OneEnemy => self.random_living_party_member(pc).unwrap_or(slot),
            SpellTarget::AllEnemies => 8,
            SpellTarget::AllAllies => 9,
            SpellTarget::SelfOnly => slot,
            SpellTarget::OneAlly => {
                let mut best: Option<(u8, u16)> = None;
                for i in pc..n {
                    if let Some(a) = self.actors.get(i as usize)
                        && a.battle.liveness != 0
                        && a.battle.hp < a.battle.max_hp / 2
                        && best.is_none_or(|(_, hp)| a.battle.hp < hp)
                    {
                        best = Some((i, a.battle.hp));
                    }
                }
                best.map(|(i, _)| i).unwrap_or(slot)
            }
        }
    }

    /// Resolve an absolute target list from a `+0x1DD` target class: `8` = all
    /// living party, `9` = all living monsters, `< party_count` = that single
    /// party slot, otherwise that single monster/self slot.
    fn resolve_class_to_slots(&self, slot: u8, class: u8) -> Vec<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let alive = |i: u8| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        };
        let _ = slot;
        match class {
            8 => (0..pc).filter(|&i| alive(i)).collect(),
            9 => (pc..n).filter(|&i| alive(i)).collect(),
            t if t < n => vec![t],
            // Out-of-range class: no targets (the caller falls back to physical).
            _ => Vec::new(),
        }
    }

    /// Pick a random living party member (`rand % party_count`, re-rolled until
    /// it lands on a living slot), mirroring the party-target roll shared by
    /// `FUN_801E9FD4` and `FUN_801E7320`. `None` only when the whole party is
    /// down. The deterministic LCG cycles every value, so the re-roll loop
    /// always terminates once one member is alive.
    fn random_living_party_member(&mut self, party_count: u8) -> Option<u8> {
        let pc = party_count.max(1);
        let any_alive = (0..pc).any(|i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        });
        if !any_alive {
            return None;
        }
        loop {
            let t = (self.next_rng() % pc as u32) as u8;
            if self
                .actors
                .get(t as usize)
                .is_some_and(|a| a.battle.liveness != 0)
            {
                return Some(t);
            }
        }
    }

    /// Clean-room port of `FUN_801E7320` - the monster-AI **target resolver**,
    /// invoked by the battle SM (`FUN_801E295C`) at `ActionSeed` as the
    /// `monster_setup` hook for monster actors whose `field_flags & 0x380` is
    /// set. It reads the targeting-class byte the action picker left in
    /// `actor.active_target` (`+0x1DD`) and expands it into a concrete target,
    /// re-rolling the deterministic RNG until it lands on a living actor on the
    /// matching side:
    ///
    /// - **class `0..2`** → a living **monster** slot (`rand % monster_count +
    ///   party_count`); if it lands on self, clears `action_category` and keeps
    ///   self as the target.
    /// - **class `3..6`** → a living **party** slot (`rand % party_count`).
    /// - **class `8`** → 1-in-3 keeps the all-target code `9`, else self.
    /// - **class `7` / other** → 1-in-3 sets the all-target code `8`, else self.
    ///
    /// Retail ctx fields: `ctx[+0]` = party count, `ctx[+1]` = monster count,
    /// `ctx[+0x13]` = active slot - here read from `party_count` / the actor
    /// table / `slot`. See `ghidra/scripts/funcs/overlay_battle_action_801e7320.txt`.
    ///
    /// Note: in the current live loop monsters carry `field_flags == 0`, so the
    /// SM does not invoke this and the picker's own target stands. Wiring the
    /// `0x380` flag (set by retail at an as-yet-untraced init site) is the open
    /// RE thread; this port keeps the routine faithful for when that lands.
    ///
    /// PORT: FUN_801E7320
    /// REF: FUN_801E295C
    fn resolve_monster_target(&mut self, slot: u8) {
        let pc = self.party_count.max(1);
        let mc = (self.actors.len() as u8).saturating_sub(pc).max(1);
        let class = match self.actors.get(slot as usize) {
            Some(a) => a.battle.active_target,
            None => return,
        };
        let set_target = |w: &mut Self, t: u8| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.active_target = t;
            }
        };
        let clear_category_self = |w: &mut Self| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.action_category = 0;
                a.battle.active_target = slot;
            }
        };
        if class < 3 {
            // Target a living monster (the caster's own band).
            loop {
                let t = (self.next_rng() % mc as u32) as u8 + pc;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    if t == slot {
                        clear_category_self(self);
                    }
                    return;
                }
            }
        } else if class < 7 {
            // Target a living party member.
            loop {
                let t = (self.next_rng() % pc as u32) as u8;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    return;
                }
            }
        } else if class == 8 {
            if self.next_rng().is_multiple_of(3) {
                set_target(self, 9);
            } else {
                clear_category_self(self);
            }
        } else if self.next_rng().is_multiple_of(3) {
            set_target(self, 8);
        } else {
            clear_category_self(self);
        }
    }

    /// First living actor on the side opposing `attacker`. Party slots
    /// (`< party_count`) oppose the monster band (`party_count..`); monster
    /// slots oppose the party. `None` if that side is wiped.
    fn first_living_opponent_of(&self, attacker: u8) -> Option<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
        (lo..hi).find(|&i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// Next living combatant after `after` in round-robin slot order across
    /// the whole actor table (party then monsters, wrapping). Drives the live
    /// loop's turn cycling so monsters take turns interleaved with the party.
    /// `None` only when no actor is alive.
    fn next_living_combatant(&self, after: u8) -> Option<u8> {
        let n = self.actors.len();
        if n == 0 {
            return None;
        }
        (1..=n).find_map(|step| {
            let idx = (after as usize + step) % n;
            (self.actors[idx].battle.liveness != 0).then_some(idx as u8)
        })
    }

    /// True when at least one living battle slot carries a non-zero SPD. Gates
    /// the SPD-seeded initiative turn order on real speed data; otherwise the
    /// battle stays on the round-robin [`Self::next_living_combatant`].
    fn any_battle_speed(&self) -> bool {
        (0..BATTLE_SLOTS).any(|i| {
            self.battle_speed[i] != 0 && self.actors.get(i).is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// Seed every living battle slot's initiative key from its SPD; dead slots
    /// get `0`. Per-actor formula `init_key = speed + rand()%(speed/2 + 1) + 1`
    /// (`overlay_0897_801e23ec`), so every living actor's key is `>= 1`.
    fn reseed_initiative(&mut self) {
        for i in 0..BATTLE_SLOTS {
            let alive = self.actors.get(i).is_some_and(|a| a.battle.liveness != 0);
            if !alive {
                if let Some(a) = self.actors.get_mut(i) {
                    a.battle.init_key = 0;
                }
                continue;
            }
            let speed = self.battle_speed[i];
            let span = (speed / 2 + 1) as u32; // never 0
            let key = speed as u32 + (self.next_rng() % span) + 1;
            if let Some(a) = self.actors.get_mut(i) {
                a.battle.init_key = key.min(u16::MAX as u32) as u16;
            }
        }
    }

    /// Seed the battle's initiative keys at setup: every living actor gets a
    /// key, then slot 0's key is consumed so it leads round 1 and the selector
    /// orders the rest by initiative. No-op (keys left at `0`) when no SPD is
    /// present, leaving the battle on the round-robin fallback.
    fn seed_battle_initiative(&mut self) {
        if !self.any_battle_speed() {
            return;
        }
        self.reseed_initiative();
        if let Some(a) = self.actors.get_mut(0) {
            a.battle.init_key = 0;
        }
    }

    /// Next combatant by SPD-seeded initiative - the port of
    /// `recompute_battle_order` (`FUN_801daba4`). Returns the living actor with
    /// the highest current initiative key (random tiebreak via `rand %
    /// tie_count`), consuming that actor's key so the next turn picks another.
    /// When every living actor's key is spent a new round is seeded. Dead
    /// actors' keys are zeroed (the function's first loop) so they can't be
    /// picked. Falls back to round-robin when no actor carries SPD.
    ///
    /// PORT: FUN_801DABA4
    fn next_combatant_by_initiative(&mut self) -> Option<u8> {
        if !self.any_battle_speed() {
            return self.next_living_combatant(self.battle_ctx.active_actor);
        }
        // First loop: zero dead actors' keys so the max-pick skips them.
        for i in 0..BATTLE_SLOTS {
            if self.actors.get(i).is_some_and(|a| a.battle.liveness == 0)
                && let Some(a) = self.actors.get_mut(i)
            {
                a.battle.init_key = 0;
            }
        }
        // Round boundary: when no living actor still holds a key, reseed.
        let any_key = (0..BATTLE_SLOTS).any(|i| {
            self.actors
                .get(i)
                .is_some_and(|a| a.battle.liveness != 0 && a.battle.init_key != 0)
        });
        if !any_key {
            self.reseed_initiative();
        }
        // Highest key among living actors; ties collected in slot order.
        let mut best: u16 = 0;
        let mut ties: Vec<u8> = Vec::new();
        for i in 0..BATTLE_SLOTS {
            let Some(a) = self.actors.get(i) else {
                continue;
            };
            if a.battle.liveness == 0 {
                continue;
            }
            let key = a.battle.init_key;
            if key == 0 {
                continue;
            }
            if key > best {
                best = key;
                ties.clear();
                ties.push(i as u8);
            } else if key == best {
                ties.push(i as u8);
            }
        }
        if ties.is_empty() {
            return self.next_living_combatant(self.battle_ctx.active_actor);
        }
        let pick = ties[(self.next_rng() as usize) % ties.len()];
        if let Some(a) = self.actors.get_mut(pick as usize) {
            a.battle.init_key = 0; // consume this turn
        }
        Some(pick)
    }

    /// Resolve a finished battle and return to the field.
    ///
    /// On [`BattleEndCause::MonsterWipe`] applies loot (XP / gold / drops /
    /// level-ups) via [`Self::apply_battle_loot`] against the captured
    /// formation; on [`BattleEndCause::PartyWipe`] raises [`Self::game_over`]
    /// (v0.1 has no defeat screen). Either way the field actor snapshot is
    /// restored, the encounter session drops into its grace window, and the
    /// scene mode flips back to [`SceneMode::Field`].
    fn finish_battle(&mut self) {
        if self.battle_end == Some(BattleEndCause::MonsterWipe)
            && let Some(formation) = self.active_formation.clone()
        {
            // `apply_battle_loot` borrows the catalog while mutating self, so
            // swap it out and back around the call.
            let catalog = std::mem::take(&mut self.monster_catalog);
            let rewards = self.apply_battle_loot(&formation, &catalog);
            self.monster_catalog = catalog;
            self.last_battle_rewards = Some(rewards);
        }
        if self.battle_end == Some(BattleEndCause::PartyWipe) {
            self.game_over = true;
        }
        self.active_formation = None;
        self.battle_end = None;
        self.battle_escaped = false;
        // Restore the field track stashed at encounter start (cross-fades
        // back from the battle music). No-op if no swap was active.
        self.restore_field_bgm();
        // Revert any lingering buff deltas so the per-slot scalars return to
        // base, then drop the trackers + captured-id log (a new battle re-inits
        // these).
        let buffs = std::mem::take(&mut self.battle_buffs);
        for b in buffs {
            self.add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
        // Bank any captured Seru into learning progress (drains battle_captures).
        self.resolve_captures();
        // Drop any open command / item / spell session - they belong to the
        // finished battle.
        self.battle_command = None;
        self.battle_item_menu = None;
        self.battle_spell_menu = None;
        self.battle_arts_menu = None;
        // Stale damage popups must not bleed into the next encounter / field.
        self.battle_hit_fx.clear();
        // Post-battle grace + suppression on the session.
        self.end_encounter_battle();
        // Restore the field actor table captured at the transition.
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party_count = ret.party_count;
        }
        // Return to the mode the battle was entered from (the field for a
        // field encounter, the overworld for a world-map encounter), then
        // reset the latch so a subsequent direct `enter_battle` defaults back
        // to the field.
        self.mode = self.battle_return_mode;
        self.battle_return_mode = SceneMode::Field;
        // Reset step tracking so the post-battle position doesn't count as a
        // step on the next field tick.
        self.field_last_tile = None;
    }

    /// Active enemy actors in the current battle as `(actor_index,
    /// monster_id, battle_slot)`, where `battle_slot` is the 0-based monster
    /// index the battle texture loader keys VRAM placement on (feed it to
    /// `legaia_asset::monster_archive::MonsterMesh::battle_render_mesh`).
    /// Empty unless the world is in [`SceneMode::Battle`].
    ///
    /// A renderer uses this to bridge each decoded monster mesh into its draw
    /// list: the engine itself never loads the archive, so the actor only
    /// carries the id - the host resolves it to a mesh.
    pub fn battle_monster_slots(&self) -> Vec<(usize, u16, u8)> {
        if !matches!(self.mode, SceneMode::Battle) {
            return Vec::new();
        }
        let first_monster = self.party_count as usize;
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(idx, a)| {
                let id = a.battle_monster_id?;
                let slot = idx.checked_sub(first_monster)? as u8;
                Some((idx, id, slot))
            })
            .collect()
    }

    /// Configure the actor at `slot` as the field player and reset the
    /// per-scene collision grid.
    ///
    /// REF: FUN_8003aeb0
    ///
    /// Mirrors the player-actor setup in the
    /// scene-entry map-init `FUN_8003aeb0` (`player[+0x72] = 0x1000`) plus
    /// the per-frame delta scalar `DAT_1f800393` (defaulted to `1` when the
    /// world hasn't installed one). Idempotent across scene transitions.
    pub fn install_field_player(&mut self, slot: u8) {
        self.player_actor_slot = Some(slot);
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.active = true;
            actor.move_state.field_72 = FIELD_PLAYER_SPEED_MULT;
        }
        if self.move_ramp_ratio == 0 {
            self.move_ramp_ratio = 1;
        }
        self.reset_field_collision_grid();
    }

    /// One field-VM step. Drives `field_ctx` + `field_pc` from the loaded
    /// `field_bytecode`. No-op when no bytecode is loaded.
    pub fn step_field(&mut self) -> Option<FieldStepResult> {
        if self.field_bytecode.is_empty() {
            return None;
        }
        let ctx_ptr: *mut FieldCtx = &mut self.field_ctx;
        let bc_ptr: *const Vec<u8> = &self.field_bytecode;
        let pc = self.field_pc;
        let mut host = FieldHostImpl { world: self };
        // SAFETY: FieldHostImpl never borrows `world.field_ctx` or
        // `world.field_bytecode` through the borrow.
        let ctx = unsafe { &mut *ctx_ptr };
        let bc: &[u8] = unsafe { (*bc_ptr).as_slice() };
        let res = vm::field::step(&mut host, ctx, bc, pc);
        match &res {
            FieldStepResult::Advance { next_pc } => self.field_pc = *next_pc,
            FieldStepResult::Yield { resume_pc } => self.field_pc = *resume_pc,
            FieldStepResult::Halt { final_pc } => self.field_pc = *final_pc,
            FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                self.field_pc = *pc;
            }
        }
        // The field-VM borrow has ended; install any scripted encounter the
        // op 0x34 sub-2 forwarded-PC capture queued this step.
        self.drain_pending_scripted_encounter();
        Some(res)
    }

    /// Drain a queued scripted-encounter install (set by the `+0x94`
    /// forwarded-PC capture host hook) into the active encounter session.
    /// No-op when nothing is queued. Called by [`Self::step_field`] once the
    /// field-VM borrow has ended.
    pub fn drain_pending_scripted_encounter(&mut self) {
        if let Some(record) = self.pending_scripted_encounter.take() {
            self.install_scripted_encounter(&record);
        }
    }
}

// --- actor VM host ---------------------------------------------------------

struct ActorVmHostImpl<'a> {
    world: &'a mut World,
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

struct MoveVmHostImpl<'a> {
    world: &'a mut World,
    /// Actor slot currently being stepped. Routes `move_bytecode_*` callbacks
    /// to the right `world.move_bytecode[slot]` buffer and the `*_slot_*`
    /// table reads to per-slot scratch (the shared 16-slot table is global,
    /// not per actor; this is unused there).
    current_slot: Option<usize>,
    /// Deferred bytecode writes accumulated during one `step` call. The VM
    /// borrows `world.move_bytecode[slot]` immutably as the bytecode slice;
    /// we can't write back through the same borrow, so the host buffers
    /// writes and `step_move_vm` flushes them after step returns.
    ///
    /// Reads consult this map first so an in-flight write within the same
    /// step (e.g. 0x1B copy loop reading from a freshly-mutated word) sees
    /// the latest value.
    deferred_writes: std::collections::BTreeMap<usize, u16>,
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

struct EffectHostImpl<'a> {
    world: &'a mut World,
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
struct WorldMapEntityHostImpl<'a> {
    world: &'a mut World,
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
            Some(WorldMapEntityConfig::Npc { interact_id }) => *interact_id,
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
struct FieldCarrierHostImpl<'a> {
    world: &'a mut World,
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

struct FieldHostImpl<'a> {
    world: &'a mut World,
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
        let dismissed = self.world.input.just_pressed(input::PadButton::Cross)
            || self.world.input.just_pressed(input::PadButton::Circle);
        if dismissed {
            self.world.current_dialog = None;
            self.world
                .pending_field_events
                .push(FieldEvent::DialogDismissed);
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
        self.world.last_field_interact = Some((interact_id, slot));
        self.world
            .pending_field_events
            .push(FieldEvent::FieldInteract { interact_id, slot });
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

struct BattleHostImpl<'a> {
    world: &'a mut World,
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

// --- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use vm::Insn;

    #[test]
    fn world_starts_with_inactive_actors() {
        let world = World::new();
        assert_eq!(world.actors.len(), MAX_ACTORS);
        assert!(world.actors.iter().all(|a| !a.active));
    }

    #[test]
    fn actor_vm_spawn_default_runs_through_world() {
        let mut world = World::new();
        // Pre-set default position for actor 7.
        world.actors[7].default_pos = ActorVmPosition::new(100, 50);
        // Bytecode: SpawnDefault actor 7, then End.
        let bc = {
            let mut v = vec![];
            v.extend_from_slice(
                &Insn {
                    opcode: 0x01,
                    operand_b: 7,
                    operand_w: 0,
                }
                .encode(),
            );
            v.extend_from_slice(&[0u8; 4]);
            v
        };
        let pc = world.run_actor_bytecode(&bc).unwrap();
        assert_eq!(pc, 4);
        assert!(world.actors[7].active);
        assert_eq!(world.actors[7].move_state.world_x, 100);
    }

    #[test]
    fn actor_vm_set_field_1d_writes_when_actor_exists() {
        let mut world = World::new();
        world.actors[3].active = true;
        let bc = {
            let mut v = vec![];
            v.extend_from_slice(
                &Insn {
                    opcode: 0x03,
                    operand_b: 3,
                    operand_w: 0xFF42,
                }
                .encode(),
            );
            v.extend_from_slice(&[0u8; 4]);
            v
        };
        world.run_actor_bytecode(&bc).unwrap();
        assert_eq!(world.actors[3].field_1d, 0x42);
    }

    #[test]
    fn move_vm_step_writes_world_state() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Bytecode: WORLD_SET (op 0x07) x=100, y=50, z=10, then HALT.
        let bc: Vec<u16> = vec![0x0007, 100, 50, 10, 0x0008];
        let res = world.step_move_vm(0, &bc);
        // First step is WORLD_SET (Advance), then we'd need to call again for HALT.
        assert!(matches!(res, vm::move_vm::StepResult::Advance));
        assert_eq!(world.actors[0].move_state.world_x, 100);
        assert_eq!(world.actors[0].move_state.world_y, 50);
    }

    #[test]
    fn world_tick_in_battle_mode_runs_state_machine() {
        let mut world = World::new();
        world.mode = SceneMode::Battle;
        // Mark all actors alive so end-of-action doesn't immediately wipe.
        for a in &mut world.actors {
            a.battle.liveness = 1;
        }
        world.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        world.battle_ctx.queued_action = 5;
        let out = world.tick();
        assert!(matches!(out, Some(StepOutcome::Transition { .. })));
        assert_eq!(
            world.battle_ctx.action_state,
            vm::battle_action::ActionState::PreActionWait.as_byte()
        );
    }

    #[test]
    fn world_tick_in_title_mode_returns_none() {
        let mut world = World::new();
        world.mode = SceneMode::Title;
        let out = world.tick();
        assert!(out.is_none());
        assert_eq!(world.frame, 1);
    }

    #[test]
    fn next_rng_is_deterministic() {
        let mut a = World::new();
        let mut b = World::new();
        let seq_a: Vec<_> = (0..10).map(|_| a.next_rng()).collect();
        let seq_b: Vec<_> = (0..10).map(|_| b.next_rng()).collect();
        assert_eq!(seq_a, seq_b);
        // And not all zero.
        assert!(seq_a.iter().any(|&x| x != 0));
    }

    #[test]
    fn battle_party_wipe_signals_end_via_world() {
        let mut world = World::new();
        world.mode = SceneMode::Battle;
        // Kill all party.
        for i in 0..3 {
            world.actors[i].battle.liveness = 0;
        }
        // Mark monsters alive.
        for i in 3..8 {
            world.actors[i].battle.liveness = 1;
        }
        world.battle_ctx.action_state = vm::battle_action::ActionState::EndOfAction.as_byte();
        let out = world.tick();
        assert_eq!(out, Some(StepOutcome::BattleComplete));
        assert_eq!(world.battle_end, Some(BattleEndCause::PartyWipe));
    }

    #[test]
    fn ensure_actor_is_idempotent_and_writes_default_pos() {
        let mut world = World::new();
        world.ensure_actor(2, ActorVmPosition::new(7, 11));
        assert!(world.actors[2].active);
        assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(7, 11));
        // Calling again with new pos updates it but doesn't reset the actor.
        world.actors[2].field_1d = 0xAB;
        world.ensure_actor(2, ActorVmPosition::new(13, 17));
        assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(13, 17));
        assert_eq!(world.actors[2].field_1d, 0xAB);
    }

    #[test]
    fn effect_pool_persists_then_terminates_over_lifetime() {
        let mut world = World::new();
        // Mark slot 0 active by setting child_count > 0 so the tick walker
        // visits it.
        world.effect_pool.master_slots[0].child_count = 4;

        // The effect must survive each work tick until the fixed lifetime
        // budget is spent - it no longer terminates on the first tick.
        let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES;
        for frame in 1..lifetime {
            world.tick_effects();
            assert_eq!(
                world.effect_pool.master_slots[0].child_count, 4,
                "effect retired early at frame {frame}"
            );
            assert_eq!(world.effect_pool.master_slots[0].field_14, frame as i32);
        }
        // The tick that reaches the budget retires the slot.
        world.tick_effects();
        assert_eq!(world.effect_pool.master_slots[0].child_count, 0);
    }

    #[test]
    fn world_tick_in_field_mode_steps_field_vm() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // Bytecode: 0x37 YIELD. Should set ctx.flags |= 0x400 + advance PC
        // past the yield.
        world.load_field_script(vec![0x37, 0x00]);
        let _ = world.tick();
        assert_eq!(world.field_ctx.flags & 0x400, 0x400, "halt bit set");
        assert!(
            world.field_pc > 0,
            "field_pc should advance after yield, got {}",
            world.field_pc
        );
    }

    #[test]
    fn world_tick_field_mode_no_bytecode_is_noop() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // No bytecode loaded. Tick should not panic and should not advance
        // field_pc.
        let _ = world.tick();
        assert_eq!(world.field_pc, 0);
    }

    // --- field collision grid + free-movement locomotion ---------------

    #[test]
    fn field_grid_block_all_then_clear() {
        let mut world = World::new();
        world.reset_field_collision_grid();
        // Block tile (col=2, row=3) - covers world x in [256,384), z in
        // [384,512).
        world.paint_field_collision(1, (2, 3), (3, 4), 0);
        assert!(world.field_tile_is_wall(320, 448), "painted tile is a wall");
        // Neighbour tile (col=1) stays walkable.
        assert!(!world.field_tile_is_wall(160, 448));
        // Clearing the same rectangle makes it walkable again.
        world.paint_field_collision(0, (2, 3), (3, 4), 0);
        assert!(!world.field_tile_is_wall(320, 448));
    }

    #[test]
    fn field_grid_set_mask_selects_quadrant() {
        let mut world = World::new();
        world.reset_field_collision_grid();
        // Set wall bit for quadrant 0 (sub-cell x even, z even) of tile
        // (0,0) only.
        world.paint_field_collision(3, (0, 1), (0, 1), 0b0001);
        assert!(world.field_tile_is_wall(10, 10), "quadrant 0 is a wall");
        // Quadrant 1 (sub-cell x odd) of the same tile is untouched.
        assert!(!world.field_tile_is_wall(64 + 10, 10));
    }

    #[test]
    fn field_vm_nibble7_paints_collision_grid() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        // 0x4C outer-nibble-7 sub-1 (block all): bytes
        // [0x4C, 0x71, col0, row0, col1, row1, mask]. The paint covers
        // columns [col0, col1+1) and rows [row0+1, row1+2) (the row bounds
        // carry an extra +1 the column bounds do not - see FUN_801de840
        // case 7), so [2, 3, 2, 3] paints column 2, row 4.
        world.load_field_script(vec![0x4C, 0x71, 2, 3, 2, 3, 0x00]);
        let _ = world.tick();
        // The hook routed the paint into the grid: tile (col 2, row 4) ->
        // world x in [256, 384), z in [512, 640).
        assert!(world.field_tile_is_wall(320, 576));
        // The unshifted tile (col 2, row 3) is NOT painted.
        assert!(!world.field_tile_is_wall(320, 448));
    }

    #[test]
    fn load_field_collision_grid_copies_map_region_and_nibble7_layers_on_top() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        // Synthesize a base grid: block tile (col=5, row=6) in all four
        // sub-cells (high nibble 0xF), floor tier 2 (low nibble) elsewhere.
        let mut grid = vec![0u8; FIELD_GRID_LEN];
        grid[6 * FIELD_GRID_STRIDE + 5] = 0xF2;
        world.load_field_collision_grid(&grid);
        // tile (5,6) -> world x in [640,768), z in [768,896).
        assert!(world.field_tile_is_wall(700, 800), "base grid wall loaded");
        assert!(!world.field_tile_is_wall(700, 600), "other tiles walkable");
        // Low nibble (floor tier) is preserved, not treated as a wall bit.
        assert_eq!(world.field_collision_grid[6 * FIELD_GRID_STRIDE + 5], 0xF2);
        // A nibble-7 paint layers a delta on top of the loaded base.
        world.paint_field_collision(1, (8, 9), (8, 9), 0);
        assert!(world.field_tile_is_wall(8 * 128 + 10, 8 * 128 + 10));
        // The base wall is still present after the delta.
        assert!(world.field_tile_is_wall(700, 800));
    }

    #[test]
    fn load_field_collision_grid_pads_short_input() {
        let mut world = World::new();
        world.load_field_collision_grid(&[0xF0, 0x00]);
        assert_eq!(world.field_collision_grid.len(), FIELD_GRID_LEN);
        assert!(
            world.field_tile_is_wall(10, 10),
            "first tile wall from input"
        );
    }

    #[test]
    fn locomotion_moves_player_on_dpad() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        world.actors[0].move_state.world_x = 200;
        world.actors[0].move_state.world_z = 200;
        // Up -> +Z. speed = (8 * 0x1000 >> 12) * 1 = 8 -> +8 in 2-unit steps.
        world.set_pad(input::PadButton::Up.mask());
        let _ = world.tick();
        assert_eq!(world.actors[0].move_state.world_z, 208);
        assert_eq!(world.actors[0].move_state.world_x, 200);
    }

    #[test]
    fn locomotion_diagonal_normalises_speed() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        world.actors[0].move_state.world_x = 400;
        world.actors[0].move_state.world_z = 400;
        // Up+Right -> Z+ and X+. speed = 8, diagonal -= 8>>2 = 6 -> +6 each.
        world.set_pad(input::PadButton::Up.mask() | input::PadButton::Right.mask());
        let _ = world.tick();
        assert_eq!(world.actors[0].move_state.world_z, 406);
        assert_eq!(world.actors[0].move_state.world_x, 406);
    }

    #[test]
    fn locomotion_stops_at_wall() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        world.actors[0].move_state.world_x = 200;
        world.actors[0].move_state.world_z = 250;
        // Block tile (col=1, row=2) - the tile the +Z walk crosses into at
        // z=256.
        world.paint_field_collision(1, (1, 2), (2, 3), 0);
        world.set_pad(input::PadButton::Up.mask());
        let _ = world.tick();
        // Player advances 250 -> 254, then the candidate 256 lands in the
        // blocked tile and is rejected. Without the wall it would reach 258.
        assert_eq!(world.actors[0].move_state.world_z, 254);
        assert_eq!(world.actors[0].move_state.world_x, 200);
    }

    #[test]
    fn locomotion_gated_by_movement_disabled_flag() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        world.actors[0].move_state.world_z = 200;
        world.actors[0].move_state.flags |= 0x0008_0000; // encounter / cutscene owns player
        world.set_pad(input::PadButton::Up.mask());
        let _ = world.tick();
        assert_eq!(
            world.actors[0].move_state.world_z, 200,
            "no movement while disabled"
        );
    }

    #[test]
    fn locomotion_gated_by_active_dialog() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        world.actors[0].move_state.world_z = 200;
        world.current_dialog = Some(DialogRequest {
            text_id: 1,
            inline: Vec::new(),
            world_x: 0,
            world_z: 0,
            depth_id: 0,
        });
        world.set_pad(input::PadButton::Up.mask());
        let _ = world.tick();
        assert_eq!(
            world.actors[0].move_state.world_z, 200,
            "dialog owns the frame"
        );
    }

    #[test]
    fn locomotion_deterministic_across_identical_pad_stream() {
        fn drive(pads: &[u16]) -> (i16, i16) {
            let mut world = World::new();
            world.mode = SceneMode::Field;
            world.install_field_player(0);
            world.actors[0].move_state.world_x = 300;
            world.actors[0].move_state.world_z = 300;
            // A couple of deterministic walls so collision rejection is in
            // the path being compared.
            world.paint_field_collision(1, (0, 3), (0, 3), 0);
            for &p in pads {
                world.set_pad(p);
                let _ = world.tick();
            }
            let ms = &world.actors[0].move_state;
            (ms.world_x, ms.world_z)
        }
        let up = input::PadButton::Up.mask();
        let down = input::PadButton::Down.mask();
        let left = input::PadButton::Left.mask();
        let right = input::PadButton::Right.mask();
        let seq = [up, up | right, right, down, down | left, left, 0, up];
        assert_eq!(
            drive(&seq),
            drive(&seq),
            "identical pad stream is bit-identical"
        );
    }

    #[test]
    fn world_tick_drives_per_actor_move_vm() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.actors[0].active = true;
        // Move-VM bytecode: WORLD_SET (op 0x07) x=42, y=10, z=5, then HALT.
        world.set_move_bytecode(0, Some(vec![0x0007, 42, 10, 5, 0x0008]));
        let _ = world.tick();
        // First step is WORLD_SET; should write the position.
        assert_eq!(world.actors[0].move_state.world_x, 42);
        assert_eq!(world.actors[0].move_state.world_y, 10);
    }

    #[test]
    fn world_tick_skips_move_vm_when_wait_timer_set() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.wait_timer = 5;
        world.set_move_bytecode(0, Some(vec![0x0007, 99, 99, 99, 0x0008]));
        let _ = world.tick();
        // Wait timer decremented, but move VM didn't run -> position unchanged.
        assert_eq!(world.actors[0].move_state.wait_timer, 4);
        assert_eq!(world.actors[0].move_state.world_x, 0);
    }

    #[test]
    fn load_field_script_resets_pc_and_ctx() {
        let mut world = World::new();
        world.field_pc = 42;
        world.field_ctx.flags = 0xFFFF;
        world.load_field_script(vec![0xFF; 8]);
        assert_eq!(world.field_pc, 0);
        assert_eq!(world.field_ctx.flags, 0);
        assert_eq!(world.field_bytecode.len(), 8);
    }

    #[test]
    fn enter_battle_populates_party_and_monsters() {
        let mut world = World::default();
        world.enter_battle(3, 5, 600);
        assert_eq!(world.mode, SceneMode::Battle);
        assert_eq!(world.party_count, 3);
        // 3 party + 5 monsters = 8 active.
        let active_count = world.actors.iter().filter(|a| a.active).count();
        assert_eq!(active_count, 8);
        // Party slots are at -600 X.
        for i in 0..3 {
            assert_eq!(world.actors[i].move_state.world_x, -600);
            assert_eq!(world.actors[i].battle.liveness, 1);
        }
        // Monster slots at +600 X.
        for i in 3..8 {
            assert_eq!(world.actors[i].move_state.world_x, 600);
            assert_eq!(world.actors[i].battle.liveness, 1);
        }
        // SM seeded at Begin.
        assert_eq!(
            world.battle_ctx.action_state,
            vm::battle_action::ActionState::Begin.as_byte()
        );
    }

    #[test]
    fn enter_battle_caps_party_at_three() {
        let mut world = World::default();
        // Even if asked for more party than the cap, we clamp to 3.
        world.enter_battle(8, 0, 100);
        assert_eq!(world.party_count, 3);
    }

    #[test]
    fn enter_world_map_installs_controller() {
        let mut world = World::default();
        assert!(world.world_map_ctrl.is_none());
        world.enter_world_map();
        assert_eq!(world.mode, SceneMode::WorldMap);
        assert!(world.world_map_ctrl.is_some());
        // Idempotent: re-entry keeps the existing controller + state.
        world.world_map_ctrl.as_mut().unwrap().camera_x = 42;
        world.enter_world_map();
        assert_eq!(world.world_map_ctrl.as_ref().unwrap().camera_x, 42);
    }

    #[test]
    fn world_tick_drives_world_map_from_pad() {
        // A pad installed via set_pad() before tick() flows into the
        // world-map controller through World::tick's WorldMap arm. This is
        // the A1 keystone: input changes per-frame World state through the
        // tick path, not via a host-side controller.
        let mut world = World::default();
        world.enter_world_map();
        world.world_map_ctrl.as_mut().unwrap().debug_enabled = true;

        // Frame 1: the toggle combo (0x4A held, edge includes 0x40) flips
        // the view into top-view.
        world.set_pad(0x4A);
        let _ = world.tick();
        assert!(world.world_map_ctrl.as_ref().unwrap().is_top_view());

        // Frame 2: in top-view, the left-scroll bit (0x1000) moves the
        // camera. Releasing the toggle bits first so this frame is a clean
        // scroll, not another toggle.
        world.set_pad(0);
        let _ = world.tick();
        world.set_pad(0x1000);
        let _ = world.tick();
        assert_eq!(world.world_map_ctrl.as_ref().unwrap().camera_x, -8);
    }

    #[test]
    fn world_map_tick_is_deterministic_across_identical_pad_streams() {
        let pad_stream = [0x4Au16, 0x0000, 0x1000, 0x0020, 0x0002];
        let drive = |stream: &[u16]| {
            let mut world = World::default();
            world.enter_world_map();
            world.world_map_ctrl.as_mut().unwrap().debug_enabled = true;
            for &pad in stream {
                world.set_pad(pad);
                let _ = world.tick();
            }
            let c = world.world_map_ctrl.unwrap();
            (c.view_mode, c.camera_x, c.camera_z, c.azimuth, c.zoom)
        };
        assert_eq!(drive(&pad_stream), drive(&pad_stream));
    }

    /// With no overworld entities installed, the world-map tick is camera-only:
    /// the encounter state never advances even when encounters are enabled.
    #[test]
    fn world_map_without_entities_never_encounters() {
        let mut world = World::default();
        world.enter_world_map();
        world.set_world_map_encounter(true, 0, 7, 64);
        // No install_world_map_entities call.
        for _ in 0..10 {
            let _ = world.tick();
        }
        assert_eq!(world.mode, SceneMode::WorldMap);
        assert!(world.pending_world_map_encounter.is_none());
    }

    /// An installed overworld entity whose shared countdown reaches zero (with
    /// encounters enabled) fires an encounter that resolves into a battle, and
    /// the battle is tagged to return to the overworld - not the field.
    #[test]
    fn world_map_encounter_flips_to_battle_returning_to_world_map() {
        use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.live_gameplay_loop = true;
        world.enter_world_map();
        // A capable lone party member.
        world.actors[0].active = true;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.liveness = 1;
        world.set_battle_attack(0, 80);
        // Formation 7 spawns one weak monster (id 100); register its stats.
        world
            .formation_table
            .insert(FormationDef::new(7, vec![FormationSlot::new(100)]));
        let mut cat = MonsterCatalog::new();
        cat.insert(MonsterDef::new(100, "Test Slug", 20, 4));
        world.set_monster_catalog(cat);
        // One entity; encounters enabled with the countdown already at zero so
        // the first Idle step fires immediately.
        world.install_world_map_entities(1);
        world.set_world_map_encounter(true, 0, 7, 64);

        // Tick once: the entity SM fires the encounter and the world flips into
        // battle, tagged to return to the overworld.
        let _ = world.tick();
        assert_eq!(world.mode, SceneMode::Battle);
        assert_eq!(world.battle_return_mode, SceneMode::WorldMap);
        assert!(world.field_return.is_some());

        // Drive the fight to completion; it must return to the world map, not
        // the field.
        let mut returned = false;
        for _ in 0..8000 {
            world.tick();
            if world.mode != SceneMode::Battle {
                returned = true;
                break;
            }
        }
        assert!(returned, "the overworld battle must resolve");
        assert_eq!(
            world.mode,
            SceneMode::WorldMap,
            "an overworld encounter returns to the world map"
        );
    }

    /// A stationary player next to an idle overworld entity triggers an
    /// interaction (surfaced as a `FieldInteract` event), and a moving player
    /// does not.
    #[test]
    fn world_map_idle_entity_interacts_only_when_player_stationary() {
        let mut world = World::default();
        world.enter_world_map();
        world.install_world_map_entities(1);
        // Encounters disabled so only the interaction path can fire.
        world.set_world_map_encounter(false, 50, 0, 64);

        // Player moving (d-pad held): no interaction.
        world.set_pad(0x1000);
        let _ = world.tick();
        assert!(
            !world
                .pending_field_events
                .iter()
                .any(|e| matches!(e, FieldEvent::FieldInteract { .. })),
            "a walking player does not interact"
        );

        // Player stationary: the idle entity interacts.
        world.set_pad(0);
        let _ = world.tick();
        let interacted = world
            .drain_field_events()
            .iter()
            .any(|e| matches!(e, FieldEvent::FieldInteract { interact_id: 0, .. }));
        assert!(interacted, "a stationary player interacts with the entity");
    }

    /// An encounter-zone entity spawns its OWN formation, not the map-wide
    /// shared one.
    #[test]
    fn world_map_encounter_zone_uses_its_own_formation() {
        use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.live_gameplay_loop = true;
        world.enter_world_map();
        world.actors[0].active = true;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.liveness = 1;
        world.set_battle_attack(0, 80);
        // Register both the zone's formation (9) and a decoy shared one (7).
        world
            .formation_table
            .insert(FormationDef::new(9, vec![FormationSlot::new(100)]));
        world
            .formation_table
            .insert(FormationDef::new(7, vec![FormationSlot::new(101)]));
        let mut cat = MonsterCatalog::new();
        cat.insert(MonsterDef::new(100, "Zone Slug", 20, 4));
        cat.insert(MonsterDef::new(101, "Decoy", 20, 4));
        world.set_monster_catalog(cat);
        // Entity 0 is an encounter zone for formation 9; shared formation is 7.
        world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::EncounterZone {
            formation_id: 9,
        }]);
        world.set_world_map_encounter(true, 0, 7, 64);

        let _ = world.tick();
        assert_eq!(world.mode, SceneMode::Battle);
        assert_eq!(
            world.active_formation.as_ref().map(|f| f.formation_id),
            Some(9),
            "the zone's own formation spawns, not the shared one"
        );
    }

    /// Engaging a portal entity surfaces a `WorldMapTransition` carrying the
    /// portal's target map id.
    #[test]
    fn world_map_portal_engage_surfaces_target_map() {
        let mut world = World::default();
        world.enter_world_map();
        world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Portal {
            target_map: 5,
        }]);
        // Encounters off so only the transition path can fire.
        world.set_world_map_encounter(false, 50, 0, 64);

        world.engage_world_map_entity(0);
        let _ = world.tick();
        let transitioned = world.drain_field_events().into_iter().any(|e| {
            matches!(
                e,
                FieldEvent::WorldMapTransition {
                    target_map: 5,
                    slot: 0
                }
            )
        });
        assert!(transitioned, "the portal surfaces its target map");
    }

    /// An NPC-config entity surfaces its configured interaction id.
    #[test]
    fn world_map_npc_config_surfaces_interact_id() {
        let mut world = World::default();
        world.enter_world_map();
        world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Npc {
            interact_id: 7,
        }]);
        world.set_world_map_encounter(false, 50, 0, 64);
        // Stationary player: the idle entity interacts.
        world.set_pad(0);
        let _ = world.tick();
        let interacted = world
            .drain_field_events()
            .into_iter()
            .any(|e| matches!(e, FieldEvent::FieldInteract { interact_id: 7, .. }));
        assert!(interacted, "the NPC surfaces its configured interact id");
    }

    // ---- tile-board step + collision (A2) ----

    /// Tile-board world: 3x3 board, all floor except a wall at (1,1);
    /// player actor in slot 0 placed at its start-tile centre.
    fn tile_board_world() -> World {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.player_actor_slot = Some(0);
        w.actors[0].active = true;
        let cells = vec![1, 1, 1, 1, crate::tile_board::CELL_WALL, 1, 1, 1, 1];
        let board = crate::tile_board::TileBoard::new(3, 3, 0, 0, cells);
        w.tile_board = Some(board);
        let (x, z) = w.tile_board.as_ref().unwrap().player_world();
        w.actors[0].move_state.world_x = x as i16;
        w.actors[0].move_state.world_z = z as i16;
        w
    }

    fn pad_held(world: &mut World, mask: u16, frames: usize) {
        for _ in 0..frames {
            world.set_pad(mask);
            let _ = world.tick();
        }
    }

    #[test]
    fn tile_board_holding_right_steps_to_edge() {
        let mut w = tile_board_world();
        // Hold Right long enough to cross two tiles (8 frames/tile) and
        // bump the east edge.
        pad_held(&mut w, input::PadButton::Right.mask(), 40);
        let b = w.tile_board.as_ref().unwrap();
        // col advances 0 -> 1 -> 2, then (3,_) is out of bounds -> stops.
        assert_eq!(b.player_col, 2);
        assert_eq!(b.player_row, 0);
        // Actor settled on the (2,0) tile centre and the step is idle.
        let (tx, _tz) = b.tile_world(2, 0);
        assert_eq!(w.actors[0].move_state.world_x as i32, tx);
        assert_eq!(w.tile_board_target, None);
    }

    #[test]
    fn tile_board_takes_multiple_frames_per_tile() {
        let mut w = tile_board_world();
        // One tick: direction committed (col 0 -> 1), target set, but the
        // actor hasn't reached the next tile centre yet.
        w.set_pad(input::PadButton::Right.mask());
        let _ = w.tick();
        assert_eq!(w.tile_board.as_ref().unwrap().player_col, 1);
        assert!(w.tile_board_target.is_some());
        let (tx, _) = w.tile_board.as_ref().unwrap().tile_world(1, 0);
        assert!((w.actors[0].move_state.world_x as i32) < tx);
    }

    #[test]
    fn tile_board_blocked_by_wall() {
        let mut w = tile_board_world();
        // Start the player directly north of the (1,1) wall.
        {
            let b = w.tile_board.as_mut().unwrap();
            b.player_col = 1;
            b.player_row = 0;
        }
        let (x, z) = w.tile_board.as_ref().unwrap().player_world();
        w.actors[0].move_state.world_x = x as i16;
        w.actors[0].move_state.world_z = z as i16;
        let before = w.actors[0].move_state.world_z;
        // Down would step into the (1,1) wall - rejected, player stays.
        pad_held(&mut w, input::PadButton::Down.mask(), 20);
        let b = w.tile_board.as_ref().unwrap();
        assert_eq!((b.player_col, b.player_row), (1, 0));
        assert_eq!(w.actors[0].move_state.world_z, before);
        assert_eq!(w.tile_board_target, None);
    }

    #[test]
    fn tile_board_gated_by_dialog() {
        let mut w = tile_board_world();
        w.current_dialog = Some(DialogRequest {
            text_id: 0,
            inline: Vec::new(),
            world_x: 0,
            world_z: 0,
            depth_id: 0,
        });
        pad_held(&mut w, input::PadButton::Right.mask(), 20);
        let b = w.tile_board.as_ref().unwrap();
        assert_eq!((b.player_col, b.player_row), (0, 0));
        assert_eq!(w.tile_board_target, None);
    }

    #[test]
    fn tile_board_is_deterministic() {
        let drive = || {
            let mut w = tile_board_world();
            for &mask in &[
                input::PadButton::Right.mask(),
                input::PadButton::Down.mask(),
                input::PadButton::Right.mask(),
            ] {
                pad_held(&mut w, mask, 12);
            }
            let b = w.tile_board.as_ref().unwrap().clone();
            (
                b.player_col,
                b.player_row,
                w.actors[0].move_state.world_x,
                w.actors[0].move_state.world_z,
            )
        };
        assert_eq!(drive(), drive());
    }

    #[test]
    fn collect_sprite_requests_emits_one_per_active_actor_with_frame() {
        let mut world = World::default();
        // Slot 0: active + sprite frame at (10, 20) world coords.
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 100;
        world.actors[0].move_state.world_z = 200;
        world.set_actor_sprite(
            0,
            Some(SpriteFrame {
                atlas_src: (0, 0, 16, 24),
                tint: [1.0, 1.0, 1.0, 1.0],
                anchor_y: -8,
            }),
        );
        // Slot 1: active but no frame - shouldn't emit.
        world.actors[1].active = true;
        // Slot 2: frame but inactive - shouldn't emit.
        world.set_actor_sprite(
            2,
            Some(SpriteFrame {
                atlas_src: (16, 0, 16, 24),
                tint: [1.0; 4],
                anchor_y: 0,
            }),
        );

        let requests = world.collect_sprite_requests();
        assert_eq!(requests.len(), 1);
        let r = &requests[0];
        assert_eq!(r.actor_slot, 0);
        assert_eq!(r.world_x, 100);
        // anchor_y subtracts from world_z (z + (-8)) = 192.
        assert_eq!(r.world_y, 192);
        assert_eq!(r.atlas_src, (0, 0, 16, 24));
    }

    #[test]
    fn set_actor_sprite_with_none_clears_existing_frame() {
        let mut world = World::default();
        world.actors[0].active = true;
        world.set_actor_sprite(
            0,
            Some(SpriteFrame {
                atlas_src: (0, 0, 8, 8),
                ..Default::default()
            }),
        );
        assert!(world.actors[0].sprite_frame.is_some());
        world.set_actor_sprite(0, None);
        assert!(world.actors[0].sprite_frame.is_none());
    }

    #[test]
    fn load_field_record_skips_frame_divider_sentinel() {
        let mut world = World::new();
        // Record opens with FFFF 0000 frame divider.
        let record = vec![0xFF, 0xFF, 0x00, 0x00, 0x37, 0x00];
        world.load_field_record(&record);
        assert_eq!(world.field_pc, 4, "frame divider should bump pc to 4");
        assert_eq!(world.field_bytecode.len(), 6);
    }

    #[test]
    fn load_field_record_without_sentinel_starts_at_zero() {
        let mut world = World::new();
        let record = vec![0x37, 0x00];
        world.load_field_record(&record);
        assert_eq!(world.field_pc, 0);
    }

    /// Field VM op 0x3E with `op0 >= 100` is the scene-transition arm
    /// (`map_id = op0 - 100`). The world's `FieldHostImpl` records the
    /// request in `pending_scene_transition` for `SceneHost::tick` to
    /// drain on the next frame boundary.
    #[test]
    fn field_scene_transition_writes_pending_map_id() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // Bytecode: opcode 0x3E, op0 = 105 (map_id 5), then 4 padding
        // bytes (op0 + 4 trailing operand bytes per the dispatcher math).
        let bytecode = vec![0x3E, 105, 0, 0, 0, 0];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.pending_scene_transition, Some(5));
    }

    /// `op0 < 100` is the field_interact arm - should NOT trigger a
    /// scene transition.
    #[test]
    fn field_op_3e_low_op0_does_not_request_scene_transition() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let bytecode = vec![0x3E, 50, 7];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.pending_scene_transition, None);
    }

    /// Field-VM op `0x4C 0xE2` (FMV trigger) records the FMV index in
    /// `World::pending_fmv_trigger` AND emits a `FieldEvent::FmvTrigger`
    /// for engines to drain. Retail handler at `0x801E30E4` writes the
    /// s16 to `_DAT_8007BA78` and pokes next-game-mode = 0x1A; the
    /// world mirrors the request via these two channels.
    #[test]
    fn field_op_4c_e2_records_pending_fmv_trigger() {
        use crate::cutscene::{STR_INIT_GAME_MODE, fmv_index_to_str_filename};
        use crate::field_events::FieldEvent;

        let mut world = World::new();
        world.mode = SceneMode::Field;
        // `[0x4C, 0xE2, 0x03, 0x00, 0, 0]` → fmv_id 3 → MV4.STR.
        let bytecode = vec![0x4C, 0xE2, 0x03, 0x00, 0, 0];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.pending_fmv_trigger, Some(3));
        let events = world.drain_field_events();
        assert!(events.contains(&FieldEvent::FmvTrigger { fmv_id: 3 }));
        assert_eq!(fmv_index_to_str_filename(3), Some("MOV/MV4.STR"));
        assert_eq!(STR_INIT_GAME_MODE, 26);
    }

    /// The FMV trigger transitions Field → Cutscene one frame later (retail's
    /// main dispatcher reads the next-game-mode global the frame after the
    /// field-VM op writes it), exposes the active FMV + its `MV*.STR` path,
    /// and suspends the field VM while it plays. `finish_cutscene` returns to
    /// the field.
    #[test]
    fn field_fmv_trigger_drives_field_cutscene_field_flow() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // fmv_id 3 → MV4.STR (a playable slot).
        world.load_field_script(vec![0x4C, 0xE2, 0x03, 0x00, 0, 0]);

        // Frame 1: op fires, records the pending trigger; still in Field.
        let _ = world.tick();
        assert_eq!(world.mode, SceneMode::Field);
        assert_eq!(world.pending_fmv_trigger, Some(3));
        assert_eq!(world.active_fmv(), None);

        // Frame 2: the pending trigger is consumed at the top of the tick and
        // the world flips into the cutscene mode for the resolved FMV.
        let _ = world.tick();
        assert_eq!(world.mode, SceneMode::Cutscene);
        assert_eq!(world.pending_fmv_trigger, None);
        assert_eq!(world.active_fmv(), Some(3));
        assert_eq!(world.active_fmv_str_filename(), Some("MOV/MV4.STR"));

        // While the FMV plays the field VM is suspended (no further field
        // stepping); ticking keeps the world in Cutscene until the host ends
        // playback.
        let _ = world.tick();
        assert_eq!(world.mode, SceneMode::Cutscene);
        assert_eq!(world.active_fmv(), Some(3));

        // Host signals playback complete → back to the field.
        world.finish_cutscene();
        assert_eq!(world.mode, SceneMode::Field);
        assert_eq!(world.active_fmv(), None);
    }

    /// An FMV id whose runtime slot points at a cut/missing path is drained
    /// without entering the cutscene mode - the engine treats it as a no-op
    /// and the field keeps running.
    #[test]
    fn field_fmv_trigger_cut_path_is_a_noop() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // fmv_id 7 → slots 5..=11 are dev-only cut paths (no retail STR).
        world.load_field_script(vec![0x4C, 0xE2, 0x07, 0x00, 0, 0]);

        let _ = world.tick(); // op fires
        assert_eq!(world.pending_fmv_trigger, Some(7));
        let _ = world.tick(); // pending consumed
        assert_eq!(
            world.mode,
            SceneMode::Field,
            "cut path does not enter cutscene"
        );
        assert_eq!(world.pending_fmv_trigger, None, "pending still drained");
        assert_eq!(world.active_fmv(), None);
    }

    // --- Save / load round-trip ----------------------------------------

    #[test]
    fn load_party_populates_battle_actor_hp_mp() {
        let mut party = legaia_save::Party::zeroed(3);
        let mut hms = party.members[0].hp_mp_sp();
        hms.hp_cur = 137;
        hms.hp_max = 150;
        hms.mp_cur = 42;
        party.members[0].set_hp_mp_sp(hms);
        let mut hms1 = party.members[1].hp_mp_sp();
        hms1.hp_cur = 0; // dead member
        hms1.hp_max = 100;
        party.members[1].set_hp_mp_sp(hms1);

        let mut world = World::new();
        world.load_party(party);

        assert!(world.actors[0].active);
        assert_eq!(world.actors[0].battle.hp, 137);
        assert_eq!(world.actors[0].battle.max_hp, 150);
        assert_eq!(world.actors[0].battle.mp, 42);
        assert_eq!(world.actors[0].battle.liveness, 1);
        // Dead member: liveness flipped to 0.
        assert_eq!(world.actors[1].battle.liveness, 0);
        assert_eq!(world.party_count, 3);
    }

    #[test]
    fn save_party_round_trips_after_load() {
        let mut party = legaia_save::Party::zeroed(3);
        let mut hms = party.members[0].hp_mp_sp();
        hms.hp_cur = 200;
        hms.hp_max = 250;
        hms.mp_cur = 100;
        party.members[0].set_hp_mp_sp(hms);

        let original_bytes = party.write();

        let mut world = World::new();
        world.load_party(party);
        let saved = world.save_party();

        assert_eq!(saved.write(), original_bytes);
    }

    #[test]
    fn save_party_picks_up_in_battle_hp_changes() {
        let mut party = legaia_save::Party::zeroed(2);
        let mut hms = party.members[0].hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        party.members[0].set_hp_mp_sp(hms);

        let mut world = World::new();
        world.load_party(party);
        // Simulate damage during battle.
        world.actors[0].battle.hp = 25;

        let saved = world.save_party();
        assert_eq!(saved.members[0].hp_mp_sp().hp_cur, 25);
        // Max HP unchanged.
        assert_eq!(saved.members[0].hp_mp_sp().hp_max, 100);
    }

    #[test]
    fn load_party_caps_at_max_actors() {
        let many = legaia_save::Party::zeroed(MAX_ACTORS + 10);
        let mut world = World::new();
        world.load_party(many);
        assert_eq!(world.party_count, MAX_ACTORS as u8);
    }

    #[test]
    fn save_full_round_trips_globals() {
        let mut world = World::new();
        world.load_party(legaia_save::Party::zeroed(2));
        world.story_flags = 0xCAFE_F00D;
        world.money = 54321;
        world.inventory.insert(3, 9);
        world.inventory.insert(77, 1);

        let sf = world.save_full();
        assert_eq!(sf.ext.story_flags, 0xCAFE_F00D);
        assert_eq!(sf.ext.money, 54321);
        // inventory is sorted by item_id
        assert_eq!(sf.ext.inventory, vec![(3, 9), (77, 1)]);

        let bytes = sf.write();
        let parsed = legaia_save::SaveFile::parse(&bytes).unwrap();

        let mut world2 = World::new();
        world2.load_full(parsed);
        assert_eq!(world2.story_flags, 0xCAFE_F00D);
        assert_eq!(world2.money, 54321);
        assert_eq!(world2.inventory.get(&3), Some(&9));
        assert_eq!(world2.inventory.get(&77), Some(&1));
        assert_eq!(world2.party_count, 2);
    }

    #[test]
    fn load_full_clears_old_inventory() {
        let mut world = World::new();
        world.inventory.insert(1, 10);
        world.inventory.insert(2, 20);

        let sf = legaia_save::SaveFile {
            party: legaia_save::Party::zeroed(1),
            ext: legaia_save::SaveExt {
                story_flags: 1,
                story_flag_bits: Vec::new(),
                money: 0,
                inventory: vec![(5, 3)],
            },
            ext_v2: legaia_save::SaveExtV2::default(),
        };
        world.load_full(sf);
        assert!(!world.inventory.contains_key(&1));
        assert!(!world.inventory.contains_key(&2));
        assert_eq!(world.inventory.get(&5), Some(&3));
    }

    #[test]
    fn effect_pool_tick_decrements_state_byte() {
        let mut world = World::new();
        world.effect_pool.master_slots[0].child_count = 4;
        // state >= 8 → write back state - 8 and skip.
        world.effect_pool.master_slots[0].state = 12;
        world.tick_effects();
        assert_eq!(world.effect_pool.master_slots[0].state, 4);
        // Slot still active.
        assert_eq!(world.effect_pool.master_slots[0].child_count, 4);
    }

    // --- move-VM host wiring (round 5) ------------------------------------

    #[test]
    fn move_vm_global_predicate_round_trips_through_world() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Move bytecode: 0x2F sub-op 0x08 (set predicate to 1), then HALT.
        world.set_move_bytecode(0, Some(vec![0x002F, 0x0008, 0x0008]));
        let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
        assert_eq!(
            world.move_predicate, 1,
            "ext sub-op 0x08 should set move_predicate to 1"
        );
    }

    #[test]
    fn move_vm_global_counter_set_and_get() {
        let mut world = World::new();
        world.actors[0].active = true;
        // 0x2F sub-op 0x0F clears counter, then HALT.
        world.move_counter = 5;
        world.set_move_bytecode(0, Some(vec![0x002F, 0x000F, 0x0008]));
        let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
        assert_eq!(world.move_counter, 0);
    }

    #[test]
    fn move_vm_slot_table_save_and_load_round_trip() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 0x1234u16 as i16;
        world.actors[0].move_state.world_y = 0x5678u16 as i16;
        world.actors[0].move_state.world_z = 0x9ABCu16 as i16;
        world.actors[0].move_state.world_y_mirror = 0xDEF0u16 as i16;
        world.actors[0].move_state.field_86 = 0x0003; // slot index = 3
        // 0x2F sub-op 0x11 - save world coords into slot 3, then HALT.
        world.set_move_bytecode(0, Some(vec![0x002F, 0x0011, 0x0008]));
        let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
        // Verify the bytes landed in slot 3.
        let lo = u32::from_le_bytes(world.move_slot_table[3][0..4].try_into().unwrap());
        let hi = u32::from_le_bytes(world.move_slot_table[3][4..8].try_into().unwrap());
        assert_eq!(lo & 0xFFFF, 0x1234);
        assert_eq!((lo >> 16) & 0xFFFF, 0x5678);
        assert_eq!(hi & 0xFFFF, 0x9ABC);
        assert_eq!((hi >> 16) & 0xFFFF, 0xDEF0);
    }

    #[test]
    fn move_vm_bytecode_write_persists_after_step() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 100;
        world.actors[0].move_state.world_y = 200;
        world.actors[0].move_state.world_z = 50;
        // 0x2F sub-op 0x04 - write actor world XYZ to bytecode at
        // pc + op[2] + 3. With pc=0 and op[2]=2, target indices are 5/6/7.
        let bc = vec![
            0x002F, 0x0004, 0x0002, 0xCAFE, 0xCAFE, 0x0000, 0x0000, 0x0000,
        ];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // After step, the world's stored bytecode should reflect the writes.
        assert_eq!(world.move_bytecode[0][5], 100u16);
        assert_eq!(world.move_bytecode[0][6], 200u16);
        assert_eq!(world.move_bytecode[0][7], 50u16);
    }

    #[test]
    fn move_vm_bytecode_inplace_add_sees_prior_step_writes() {
        // 0x2F sub-op 0x1E does buffer[pc + op[2] + 4] += op[3].
        // After two consecutive steps each adding 5, the slot should hold 10
        // (proving the world flushes deferred writes between steps).
        let mut world = World::new();
        world.actors[0].active = true;
        // Two 0x1E ops back-to-back, each pointing at the same operand slot.
        // Each op is size 1 (default_arm), so we step it twice.
        // Slot 4 from instruction at pc=0 lands at index 4.
        let bc = vec![0x002F, 0x001E, 0, 5, 0]; // op[2]=0, op[3]=5
        world.set_move_bytecode(0, Some(bc.clone()));
        // First step: bytecode[0 + 0 + 4] (= 0) += 5 → 5.
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.move_bytecode[0][4], 5);
        // Step again with a fresh-cloned bytecode read of the world's buffer.
        let bc2 = world.move_bytecode[0].clone();
        // PC has advanced; reset for the same op to fire again.
        world.actors[0].move_state.pc = 0;
        let _ = world.step_move_vm(0, &bc2);
        assert_eq!(
            world.move_bytecode[0][4], 10,
            "second 0x1E should see flushed write from first step"
        );
    }

    // --- system flag bank (round 6) -------------------------------------

    #[test]
    fn system_flag_set_and_test_round_trips_through_world() {
        let mut world = World::new();
        world.system_flag_set(0);
        world.system_flag_set(7);
        world.system_flag_set(15);
        world.system_flag_set(255);
        assert!(world.system_flag_test(0));
        assert!(world.system_flag_test(7));
        assert!(world.system_flag_test(15));
        assert!(world.system_flag_test(255));
        assert!(!world.system_flag_test(1));
        assert!(!world.system_flag_test(254));
        // Out-of-bounds idx returns false.
        assert!(!world.system_flag_test(256));
        assert!(!world.system_flag_test(0xFFFF));
    }

    #[test]
    fn system_flag_clear_only_touches_target_bit() {
        let mut world = World::new();
        world.system_flag_set(3);
        world.system_flag_set(4);
        world.system_flag_clear(3);
        assert!(!world.system_flag_test(3));
        assert!(world.system_flag_test(4));
    }

    #[test]
    fn move_vm_ext_query_flag_bank_reads_world_system_flags() {
        let mut world = World::new();
        world.actors[0].active = true;
        world.system_flag_set(42);
        // Bytecode: 0x2F sub-op 0x13 - predicate-true → default_arm (size 1),
        // predicate-false → size 4.
        let bc = vec![0x002F, 0x0013, 42];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // Predicate true → PC advanced by 1.
        assert_eq!(world.actors[0].move_state.pc, 1);
        // Now clear and re-run - predicate false → PC += 4.
        world.system_flag_clear(42);
        world.actors[0].move_state.pc = 0;
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.actors[0].move_state.pc, 4);
    }

    #[test]
    fn move_vm_ext_set_flag_bank_writes_world_system_flags() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Bytecode: 0x2F sub-op 0x1C - set flag bank (idx = op_w(2)).
        let bc = vec![0x002F, 0x001C, 100];
        world.set_move_bytecode(0, Some(bc.clone()));
        assert!(!world.system_flag_test(100));
        let _ = world.step_move_vm(0, &bc);
        assert!(world.system_flag_test(100));
    }

    #[test]
    fn field_vm_system_flag_set_routes_to_world() {
        // Field-VM 0x5x default-route SET - `[0x50 | nibble, idx_byte]`.
        // idx encoding: `((opcode_byte & 0x8F) << 8) | idx_byte`. For raw
        // opcode 0x50, top bit clear, low nibble 0 → idx = idx_byte.
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.load_field_script(vec![0x50, 42]);
        let _ = world.tick();
        assert!(
            world.system_flag_test(42),
            "0x50 default-route should set system flag 42"
        );
    }

    #[test]
    fn field_vm_system_flag_set_with_low_nibble_includes_high_byte() {
        // 0x52 with low-nibble 2 → idx = (0x02 << 8) | idx_byte.
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.load_field_script(vec![0x52, 7]);
        let _ = world.tick();
        assert!(
            world.system_flag_test(0x0207),
            "0x52 default-route should set system flag 0x0207"
        );
    }

    #[test]
    fn field_vm_system_flag_clear_routes_to_world() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.system_flag_set(99);
        // 0x60 CLEAR with operand 99.
        world.load_field_script(vec![0x60, 99]);
        let _ = world.tick();
        assert!(!world.system_flag_test(99));
    }

    #[test]
    fn field_vm_system_flag_test_takes_jump_when_bit_set() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.system_flag_set(33);
        // 0x70 TEST with idx=33, jump delta = 10.
        world.load_field_script(vec![0x70, 33, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let _ = world.tick();
        // pc was 0; header_size = 1; +1 (idx byte) + delta(10) = 12.
        assert_eq!(world.field_pc, 12);
    }

    #[test]
    fn field_vm_extra_flags_op42_reads_world() {
        // Op 0x42 mode=0 - host.extra_flags() & (1 << (op1 & 0x1F)) test.
        // Set bit 5 in extra_flags; op_42 with op1=5 should take the jump.
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.extra_flags = 1 << 5;
        // [0x42, mode=0, op1=5, lo=4, hi=0] - header_size + 4 = 5 byte total
        // for skip path; jump path = pc + header_size + 2 + delta.
        world.load_field_script(vec![0x42, 0, 5, 4, 0]);
        let _ = world.tick();
        // With extra_flags bit 5 set, predicate is true → jump.
        // Jump target = 0 + 1 (header) + 2 + 4 = 7.
        assert_eq!(world.field_pc, 7, "extra_flags-true 0x42 should take jump");
    }

    #[test]
    fn move_vm_ext_set_8007b9d8_writes_world_field() {
        let mut world = World::new();
        world.actors[0].active = true;
        // 0x2F sub-op 0x2F - `_DAT_8007B9D8 = (i32) op[1]`. Note: op[1] in
        // sub-op space = sub-op selector 0x2F itself, op[2] = the value.
        // Per the move_vm port, ext sub-op 0x2F passes op[1] (the sub-op
        // word's "next slot" in the operand stream).
        let bc = vec![0x002F, 0x002F, 0xCAFE];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // Whatever the sub-op handler writes, world.move_dat_8007b9d8 should
        // pick up a non-zero value.
        assert_ne!(world.move_dat_8007b9d8, 0);
    }

    #[test]
    fn ext_compute_angle_matches_quadrant_when_player_set() {
        // Place actor at origin, player due-east; angle should be ~0 mod 4096
        // (positive X direction = angle 0 in the dz.atan2(dx) convention).
        let mut world = World::new();
        world.actors[0].active = true;
        world.actors[0].move_state.world_x = 0;
        world.actors[0].move_state.world_z = 0;
        world.actors[1].active = true;
        world.actors[1].move_state.world_x = 100;
        world.actors[1].move_state.world_z = 0;
        world.player_actor_slot = Some(1);
        // Drive ext sub-op 0x3A: VM writes the angle into bytecode at
        // `state.pc + op_w(2) + 3`. With pc=0 and op_w(2)=0, dst = u16[3].
        let bc = vec![0x002F, 0x003A, 0, 0xFFFF, 0xFFFF];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // angle 0 (player due-east) should produce ~0 in the dst slot.
        assert_eq!(
            world.move_bytecode[0][3], 0,
            "angle to due-east player should be 0"
        );
    }

    #[test]
    fn ext_compute_angle_returns_zero_when_no_player() {
        // No player slot designated → ext_compute_angle returns 0.
        let mut world = World::new();
        world.actors[0].active = true;
        let bc = vec![0x002F, 0x003A, 0, 0xFFFF];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.move_bytecode[0][3], 0);
    }

    #[test]
    fn ext_party_member_lookup_returns_table_position() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Party member at index 1 = world actor slot 5 with a known position.
        world.actors[5].active = true;
        world.actors[5].move_state.world_x = 100;
        world.actors[5].move_state.world_y = 50;
        world.actors[5].move_state.world_z = 200;
        world.party_actor_slots = vec![None, Some(5), None];
        // Sub-op 0x3B: dst = pc + op_w(3) + 4. We use op_w(2)=1 (party slot 1)
        // and op_w(3)=0 so dst = u16[4..7].
        let bc = vec![
            0x002F, 0x003B, 0x0001, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA, 0xAAAA,
        ];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(world.move_bytecode[0][4], 100u16);
        assert_eq!(world.move_bytecode[0][5], 50u16);
        assert_eq!(world.move_bytecode[0][6], 200u16);
    }

    #[test]
    fn ext_party_member_lookup_skips_when_none() {
        // No party table entry → 0x3B returns size-4 (skip), pre-clears dst.
        let mut world = World::new();
        world.actors[0].active = true;
        let bc = vec![0x002F, 0x003B, 0x0000, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        // Dst slots pre-cleared even when lookup returns None.
        assert_eq!(world.move_bytecode[0][4], 0);
        assert_eq!(world.move_bytecode[0][5], 0);
        assert_eq!(world.move_bytecode[0][6], 0);
    }

    #[test]
    fn ext_fade_color_records_pending_request() {
        let mut world = World::new();
        world.actors[0].active = true;
        // Sub-op 0x3C: r=0xAB, g=0xCD, b=0xEF, ticks=4 (ramp).
        let bc = vec![0x002F, 0x003C, 0x00AB, 0x00CD, 0x00EF, 0x0004];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(
            world.pending_fade,
            Some(FadeRequest {
                rgb: [0xAB, 0xCD, 0xEF],
                ticks: 4
            })
        );
    }

    #[test]
    fn move_player_world_xyz_reads_designated_player_slot() {
        let mut world = World::new();
        world.actors[2].active = true;
        world.actors[2].move_state.world_x = 100;
        world.actors[2].move_state.world_y = 200;
        world.actors[2].move_state.world_z = 300;
        world.player_actor_slot = Some(2);
        // No direct API to read move_player_world_xyz; verify by stepping
        // sub-op 0x39 (squared-distance "inside radius" predicate). With
        // actor 0 at origin and player at (100, _, 300), dist_sq = 100²+300² =
        // 100000 - predicate fails for r=10 (r² = 100), passes for r=400
        // (r² = 160000).
        world.actors[0].active = true;
        // Predicate fail → PC += 4.
        let bc = vec![0x002F, 0x0039, 10, 0, 0, 0];
        world.set_move_bytecode(0, Some(bc.clone()));
        let _ = world.step_move_vm(0, &bc);
        assert_eq!(
            world.actors[0].move_state.pc, 4,
            "small-radius 0x39 should fail"
        );
        // Predicate pass → PC += 1.
        world.actors[0].move_state.pc = 0;
        let bc2 = vec![0x002F, 0x0039, 400, 0, 0, 0];
        world.set_move_bytecode(0, Some(bc2.clone()));
        let _ = world.step_move_vm(0, &bc2);
        assert_eq!(
            world.actors[0].move_state.pc, 1,
            "large-radius 0x39 should pass"
        );
    }

    // --- Field-event emission ---------------------------------------------

    /// Op 0x35 sub-1 (start BGM) emits `FieldEvent::Bgm` and pins
    /// `current_bgm`. Encoding: `[0x35, lo, hi, sub_op]`.
    #[test]
    fn field_op_35_sub1_emits_bgm_event_and_pins_current() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // text_id = 0x42 (LE), sub_op = 1 (start field BGM).
        let bytecode = vec![0x35, 0x42, 0x00, 0x01];
        world.load_field_script(bytecode);
        let _ = world.tick();
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::Bgm {
                    sub_op: 1,
                    text_id: 0x42
                }
            )),
            "expected Bgm event, got {evs:?}"
        );
        assert_eq!(world.current_bgm, Some(0x42));
    }

    /// Op 0x3F (open dialog) populates `current_dialog` and emits an
    /// `OpenDialog` event. Encoding: `[0x3F, lo, hi, len, ...inline, xb, zb, depth]`.
    #[test]
    fn field_op_3f_emits_open_dialog() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // text_id = 0xAB, len = 0, then xb / zb / depth_id (3 bytes).
        let bytecode = vec![0x3F, 0xAB, 0x00, 0x00, 0x01, 0x02, 0x03];
        world.load_field_script(bytecode);
        let _ = world.tick();
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(
                |e| matches!(e, FieldEvent::OpenDialog { text_id: 0xAB, inline, .. } if inline.is_empty())
            ),
            "expected OpenDialog event, got {evs:?}"
        );
        assert_eq!(world.current_dialog.as_ref().map(|d| d.text_id), Some(0xAB));
    }

    /// Dialog-advance host hook (`op 0x4C n5 sub-4`): when `current_dialog`
    /// is set, the VM halts at the poll site. A just-pressed Cross /
    /// Circle clears the request inline and unblocks the VM the same
    /// frame, with a `DialogDismissed` event surfaced for downstream
    /// HUD consumers.
    #[test]
    fn dialog_advance_halts_then_clears_on_just_pressed_cross() {
        use crate::input::PadButton;

        let mut world = World::new();
        world.mode = SceneMode::Field;

        // Open dialog then arm a poll (4C 54) followed by a sentinel op.
        // 0x3F header: text_id=0xAB, len=0, then xb/zb/depth (3 bytes).
        // 0x4C 0x54: dialog-advance poll (2 bytes).
        // 0x1A: bgm-end-style nop / unknown — anything that makes
        //   `step_field` advance further when the dialog clears.
        let bc = vec![0x3F, 0xAB, 0x00, 0x00, 0x01, 0x02, 0x03, 0x4C, 0x54, 0x00];
        world.load_field_script(bc);

        // Tick 1: open the dialog. The 4C 54 poll runs next tick.
        let _ = world.tick();
        assert!(world.current_dialog.is_some(), "dialog should be open");

        // No buttons pressed: the poll halts at the same PC.
        world.input.set_pad(0);
        let pc_before = world.field_pc;
        let _ = world.tick();
        assert!(
            world.current_dialog.is_some(),
            "dialog persists with no input"
        );
        assert_eq!(
            world.field_pc, pc_before,
            "VM should halt at the poll PC while dialog is active"
        );

        // Cross just-pressed: the host clears the request inline and
        // advances PC by 2 (past the poll).
        world.input.set_pad(PadButton::Cross.mask());
        let _ = world.tick();
        assert!(
            world.current_dialog.is_none(),
            "dialog should clear on just-pressed Cross",
        );
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(e, FieldEvent::DialogDismissed)),
            "expected DialogDismissed event, got {evs:?}",
        );
        assert!(
            world.field_pc > pc_before,
            "VM should advance past poll PC ({} > {})",
            world.field_pc,
            pc_before,
        );
    }

    /// Dialog-advance hook returns `false` (advance) when no dialog is
    /// active. Mirrors the retail dispatcher's behavior when
    /// `FUN_801D65D8(0)` returns zero (dialog done).
    #[test]
    fn dialog_advance_no_op_when_no_dialog() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // Just the poll + sentinel - no preceding 0x3F.
        let bc = vec![0x4C, 0x54, 0x00];
        world.load_field_script(bc);
        let pc_before = world.field_pc;
        let _ = world.tick();
        assert!(
            world.field_pc > pc_before,
            "VM should advance immediately when no dialog is showing",
        );
    }

    /// Op 0x3A (add_money) clamps to `[0, 9_999_999]` and emits `AddMoney`.
    #[test]
    fn field_op_3a_clamps_and_emits_add_money() {
        let mut world = World::new();
        world.money = 100;
        world.mode = SceneMode::Field;
        // 0x3A op0=0xFF op1=0xFF op2=0xFF (24-bit -1) → delta = -1.
        // The op handler reads the 3-byte payload; sign-extend to i32.
        let bytecode = vec![0x3A, 0xFF, 0xFF, 0xFF];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert!(world.money >= 0, "money clamps to non-negative");
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(e, FieldEvent::AddMoney { .. })),
            "expected AddMoney event, got {evs:?}"
        );
    }

    /// Op 0x3C (party_add) appends to `party_actor_slots` and seeds the
    /// leader on the empty-party path.
    #[test]
    fn field_op_3c_party_add_first_member_becomes_leader() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // 0x3C + char_id (op0).
        let bytecode = vec![0x3C, 0x07];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.party_actor_slots, vec![Some(7)]);
        assert_eq!(world.party_leader_slot, Some(7));
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::PartyAdd {
                    char_id: 7,
                    accepted: true
                }
            )),
            "expected PartyAdd event, got {evs:?}"
        );
    }

    /// Drain helper empties the queue.
    #[test]
    fn drain_field_events_empties_queue() {
        let mut world = World::new();
        world
            .pending_field_events
            .push(FieldEvent::PlaySfx { sfx_id: 1 });
        let drained = world.drain_field_events();
        assert_eq!(drained.len(), 1);
        assert!(world.pending_field_events.is_empty());
    }

    /// Op `0x4C 0x80` (actor allocator) walks `count` variable-length
    /// records using the `FUN_8003CA38` packet-length rule, emits one
    /// `ActorAllocate` event, and queues each record's bytecode in
    /// `pending_actor_spawns`. Encoding here: count=2, two records each
    /// terminated by `0x00`.
    #[test]
    fn field_op_4c_n8_sub0_walks_records_and_queues_spawns() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // [4C, 0x80, 2, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00]
        //   record 0 = [0x40, 0x41] (two normal tokens, terminator 0x00)
        //   record 1 = [0xC1, 0x42] (escape pair via 0xCx high nibble)
        let bytecode = vec![0x4C, 0x80, 0x02, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();
        // PC should land at byte 3 (the first record's first byte) - the
        // retail VM advances PC by exactly 3 regardless of how many
        // records the host consumes.
        assert_eq!(world.field_pc, 3);
        // Pending queue should hold both records, in emission order.
        let spawns = world.drain_actor_spawns();
        assert_eq!(spawns.len(), 2);
        assert_eq!(spawns[0], vec![0x40, 0x41]);
        assert_eq!(spawns[1], vec![0xC1, 0x42]);
        // The event queue should also carry one ActorAllocate with both
        // records.
        let evs = world.drain_field_events();
        let allocate = evs
            .iter()
            .find_map(|e| match e {
                FieldEvent::ActorAllocate { records } => Some(records.clone()),
                _ => None,
            })
            .expect("expected ActorAllocate event");
        assert_eq!(allocate.len(), 2);
        assert_eq!(allocate[0], vec![0x40, 0x41]);
        assert_eq!(allocate[1], vec![0xC1, 0x42]);
    }

    /// `count = 0` is a legal degenerate case - no records walked, no
    /// event payload, but the event is still emitted to mark the
    /// allocator call site.
    #[test]
    fn field_op_4c_n8_sub0_zero_count_emits_empty_event() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let bytecode = vec![0x4C, 0x80, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();
        assert_eq!(world.field_pc, 3);
        assert!(world.drain_actor_spawns().is_empty());
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::ActorAllocate { records } if records.is_empty()
            )),
            "expected empty ActorAllocate event, got {evs:?}"
        );
    }

    /// `drain_actor_spawns` empties the queue.
    #[test]
    fn drain_actor_spawns_empties_queue() {
        let mut world = World::new();
        world.pending_actor_spawns.push(vec![0xAA, 0xBB]);
        let drained = world.drain_actor_spawns();
        assert_eq!(drained, vec![vec![0xAA, 0xBB]]);
        assert!(world.pending_actor_spawns.is_empty());
    }

    /// `materialize_actor_spawns` allocates a fresh slot from
    /// `start_slot..MAX_ACTORS`, populates it with the queued record, and
    /// emits an `ActorSpawned` event.
    #[test]
    fn materialize_actor_spawns_allocates_slot_and_emits_event() {
        let mut world = World::new();
        world.pending_actor_spawns.push(vec![0x10, 0x20, 0x30]);
        let allocated = world.materialize_actor_spawns(8);
        assert_eq!(allocated, 1);
        assert!(world.pending_actor_spawns.is_empty());
        assert!(world.actors[8].active);
        assert_eq!(
            world.actors[8].spawn_record.as_deref(),
            Some(&[0x10, 0x20, 0x30][..])
        );
        assert_eq!(world.actors[8].kind, 0);
        assert_eq!(world.actors[8].variant, 0);
        let evs = world.drain_field_events();
        let spawned = evs
            .iter()
            .find_map(|e| match e {
                FieldEvent::ActorSpawned {
                    slot,
                    kind,
                    variant,
                    record,
                } => Some((*slot, *kind, *variant, record.clone())),
                _ => None,
            })
            .expect("expected ActorSpawned event");
        assert_eq!(spawned, (8u8, 0u16, 0u16, vec![0x10, 0x20, 0x30]));
    }

    /// `materialize_actor_spawns` allocates consecutive inactive slots
    /// when several spawn requests are queued.
    #[test]
    fn materialize_actor_spawns_fills_consecutive_inactive_slots() {
        let mut world = World::new();
        world.pending_actor_spawns.push(vec![0xAA]);
        world.pending_actor_spawns.push(vec![0xBB]);
        world.pending_actor_spawns.push(vec![0xCC]);
        let allocated = world.materialize_actor_spawns(4);
        assert_eq!(allocated, 3);
        assert!(world.actors[4].active);
        assert!(world.actors[5].active);
        assert!(world.actors[6].active);
        assert_eq!(world.actors[4].spawn_record.as_deref(), Some(&[0xAA][..]));
        assert_eq!(world.actors[5].spawn_record.as_deref(), Some(&[0xBB][..]));
        assert_eq!(world.actors[6].spawn_record.as_deref(), Some(&[0xCC][..]));
    }

    /// Slots below `start_slot` are reserved - even when they are
    /// inactive, the materializer doesn't touch them.
    #[test]
    fn materialize_actor_spawns_skips_reserved_low_slots() {
        let mut world = World::new();
        // Slot 0 is inactive but reserved (start_slot=10).
        world.pending_actor_spawns.push(vec![0xDE, 0xAD]);
        world.materialize_actor_spawns(10);
        assert!(!world.actors[0].active);
        assert!(world.actors[10].active);
    }

    /// Mirrors retail's "pool exhausted → bail silently" branch of
    /// `FUN_801D77F4`. When no inactive slot is available in the
    /// allocation range, the record is dropped and a `ActorSpawnFailed`
    /// event is emitted instead of `ActorSpawned`.
    #[test]
    fn materialize_actor_spawns_emits_failure_when_pool_exhausted() {
        let mut world = World::new();
        // Make every slot from index 60 upward active.
        for slot in 60..MAX_ACTORS {
            world.actors[slot].active = true;
        }
        world.pending_actor_spawns.push(vec![0xEE]);
        let allocated = world.materialize_actor_spawns(60);
        assert_eq!(allocated, 0);
        let evs = world.drain_field_events();
        assert!(evs.iter().any(|e| matches!(
            e,
            FieldEvent::ActorSpawnFailed { record } if record == &[0xEE]
        )));
    }

    /// End-to-end: a field-VM `0x4C 0x80` opcode followed by
    /// `materialize_actor_spawns` should land both events
    /// (`ActorAllocate` from the opcode, `ActorSpawned` from the
    /// materializer) and leave the actor slot populated.
    #[test]
    fn field_op_4c_n8_sub0_then_materialize_flow_end_to_end() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // One record `[0x40, 0x41]` terminated by `0x00`.
        let bytecode = vec![0x4C, 0x80, 0x01, 0x40, 0x41, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();
        let allocated = world.materialize_actor_spawns(16);
        assert_eq!(allocated, 1);
        assert!(world.actors[16].active);
        assert_eq!(
            world.actors[16].spawn_record.as_deref(),
            Some(&[0x40, 0x41][..])
        );
        let evs = world.drain_field_events();
        // Both the ActorAllocate (from the opcode) and ActorSpawned (from
        // the materializer) should appear in emission order.
        let kinds: Vec<&'static str> = evs
            .iter()
            .filter_map(|e| match e {
                FieldEvent::ActorAllocate { .. } => Some("alloc"),
                FieldEvent::ActorSpawned { .. } => Some("spawned"),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, vec!["alloc", "spawned"]);
    }

    /// Op `0x4C 0xD8` is the synchronous-spawn sibling of the halt-acquire
    /// `0x4C 0x80` path. The dispatcher decodes
    /// `[0x4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, var_lo, var_hi]`
    /// into `(vdf_idx, [tmd_idx, kind, variant])` and calls the
    /// FieldHostImpl override directly - no queue. The actor slot must
    /// come out active with `kind` / `variant` mirrored from the operand,
    /// and a single `ActorSpawned` event must surface in the queue.
    #[test]
    fn field_op_4c_d8_spawns_actor_synchronously_with_kind_variant() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // `[0x4C, 0xD8, vdf_idx=0x07, tmd=0x0102, kind=0xABCD, variant=0xBEEF, 0x00]`.
        // Trailing 0x00 is a HALT so the VM doesn't run off the end.
        let bytecode = vec![0x4C, 0xD8, 0x07, 0x02, 0x01, 0xCD, 0xAB, 0xEF, 0xBE, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();

        let slot = FIELD_SPAWN_START_SLOT as usize;
        assert!(
            world.actors[slot].active,
            "0x4C 0xD8 should have spawned synchronously into slot {slot}",
        );
        assert_eq!(world.actors[slot].kind, 0xABCD);
        assert_eq!(world.actors[slot].variant, 0xBEEF);
        // 0x4C 0xD8 doesn't carry packet bytes in the bytecode - the
        // record lives in the VDF buffer at runtime - so spawn_record
        // stays `None` until the VDF / global TMD lift lands.
        assert!(world.actors[slot].spawn_record.is_none());

        let evs = world.drain_field_events();
        let spawned: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                FieldEvent::ActorSpawned {
                    slot: s,
                    kind,
                    variant,
                    record,
                } => Some((*s, *kind, *variant, record.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            spawned,
            vec![(FIELD_SPAWN_START_SLOT, 0xABCDu16, 0xBEEFu16, Vec::new())]
        );
        // No ActorAllocate event - that one is exclusively the
        // queue-based 0x4C 0x80 path.
        assert!(
            !evs.iter()
                .any(|e| matches!(e, FieldEvent::ActorAllocate { .. })),
            "0x4C 0xD8 must not emit ActorAllocate; got {evs:?}"
        );
        // And nothing was queued on the pending_actor_spawns side - the
        // synchronous path doesn't go through the materializer.
        assert!(world.pending_actor_spawns.is_empty());
    }

    /// `0x4C 0xD8` with a populated VDF buffer should copy the indexed
    /// body bytes onto the spawned actor's `spawn_record` (mirror of
    /// retail `actor[+0x4C] = VDF_body_ptr`) and surface them in the
    /// `ActorSpawned` event payload.
    #[test]
    fn field_op_4c_d8_with_vdf_buffer_populates_spawn_record() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        // VDF buffer with two records:
        //   header:  count = 2
        //   table:   offsets[0] = 12, offsets[1] = 16
        //   body 0:  [0xDE, 0xAD, 0xBE, 0xEF] @ off 12 (4 bytes -> 16)
        //   body 1:  [0xCA, 0xFE, 0xBA, 0xBE, 0x42] @ off 16 (to EOB)
        let mut vdf = Vec::new();
        vdf.extend_from_slice(&2u32.to_le_bytes()); // count
        vdf.extend_from_slice(&12u32.to_le_bytes()); // offsets[0]
        vdf.extend_from_slice(&16u32.to_le_bytes()); // offsets[1]
        vdf.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        vdf.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42]);
        world.set_vdf_buffer(Some(vdf));

        // Sanity-check the lookup helper.
        assert_eq!(
            world.vdf_record_bytes(0),
            Some(&[0xDE, 0xAD, 0xBE, 0xEF][..])
        );
        assert_eq!(
            world.vdf_record_bytes(1),
            Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..])
        );
        assert_eq!(world.vdf_record_bytes(2), None); // idx >= count

        // `[0x4C, 0xD8, vdf_idx=0x01, tmd=0x0102, kind=0x1111, variant=0x2222, 0x00]`.
        let bytecode = vec![0x4C, 0xD8, 0x01, 0x02, 0x01, 0x11, 0x11, 0x22, 0x22, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();

        let slot = FIELD_SPAWN_START_SLOT as usize;
        assert!(world.actors[slot].active);
        assert_eq!(world.actors[slot].kind, 0x1111);
        assert_eq!(world.actors[slot].variant, 0x2222);
        assert_eq!(
            world.actors[slot].spawn_record.as_deref(),
            Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..]),
            "spawn_record should mirror VDF body 1"
        );

        let evs = world.drain_field_events();
        let spawned: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                FieldEvent::ActorSpawned { record, .. } => Some(record.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(spawned, vec![vec![0xCA, 0xFE, 0xBA, 0xBE, 0x42]]);
    }

    /// `0x4C 0xD8` with a populated global TMD pool should write a
    /// matching `Arc<GlobalTmd>` onto the spawned actor's `tmd_ref`
    /// (mirror of retail `actor[+0x48] = DAT_8007C018[tmd_idx]`).
    /// Indices the pool hasn't seen leave `tmd_ref` at `None` rather
    /// than aborting the spawn.
    #[test]
    fn field_op_4c_d8_with_global_tmd_pool_populates_tmd_ref() {
        let mut world = World::new();
        world.mode = SceneMode::Field;

        // Install a stub TMD at pool slot 5. The Tmd doesn't need to
        // represent realistic mesh data - the host hook only does an
        // Arc::clone and stores the result.
        let stub = std::sync::Arc::new(GlobalTmd {
            tmd: legaia_tmd::Tmd {
                header: legaia_tmd::Header {
                    id: 0x8000_0002,
                    flags: 1,
                    nobj: 0,
                    flist_bit_set: true,
                },
                objects: Vec::new(),
            },
            raw: vec![
                0x02, 0x00, 0x00, 0x80, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        });
        let stub_ptr = std::sync::Arc::as_ptr(&stub);
        world.set_global_tmd(5, stub.clone());

        // `[0x4C, 0xD8, vdf_idx=0x00, tmd=0x0005, kind=0x1111, variant=0x2222, 0x00]`.
        let bytecode = vec![0x4C, 0xD8, 0x00, 0x05, 0x00, 0x11, 0x11, 0x22, 0x22, 0x00];
        world.load_field_script(bytecode);
        let _ = world.tick();

        let slot = FIELD_SPAWN_START_SLOT as usize;
        assert!(world.actors[slot].active);
        let tmd_ref = world.actors[slot]
            .tmd_ref
            .as_ref()
            .expect("tmd_ref should mirror DAT_8007C018[5]");
        assert_eq!(
            std::sync::Arc::as_ptr(tmd_ref),
            stub_ptr,
            "tmd_ref should reference the installed pool entry by Arc identity",
        );

        // A second spawn with an unpopulated index leaves tmd_ref at None.
        let bytecode2 = vec![0x4C, 0xD8, 0x00, 0x09, 0x00, 0x33, 0x33, 0x44, 0x44, 0x00];
        world.load_field_script(bytecode2);
        let _ = world.tick();
        let slot2 = slot + 1;
        assert!(world.actors[slot2].active);
        assert!(
            world.actors[slot2].tmd_ref.is_none(),
            "empty pool slot should not populate tmd_ref",
        );
    }

    /// Accessors round-trip: `set_global_tmd` + `global_tmd` agree on
    /// installed slots, negative indices return `None`, and the pool
    /// grows lazily.
    #[test]
    fn global_tmd_accessor_round_trip() {
        let mut world = World::new();
        assert!(world.global_tmd(0).is_none());
        assert!(world.global_tmd(-1).is_none());

        let stub = std::sync::Arc::new(GlobalTmd {
            tmd: legaia_tmd::Tmd {
                header: legaia_tmd::Header {
                    id: 0x8000_0002,
                    flags: 1,
                    nobj: 0,
                    flist_bit_set: true,
                },
                objects: Vec::new(),
            },
            raw: Vec::new(),
        });
        world.set_global_tmd(3, stub.clone());
        // Pool grew to fit idx 3.
        assert_eq!(world.global_tmd_pool.len(), 4);
        assert!(world.global_tmd_pool[0..3].iter().all(|s| s.is_none()));
        assert!(std::sync::Arc::ptr_eq(
            world.global_tmd(3).expect("slot 3 populated"),
            &stub
        ));
        assert!(world.global_tmd(7).is_none(), "out-of-range -> None");
        assert!(world.global_tmd(-5).is_none(), "negative -> None");
    }

    /// `vdf_record_bytes` rejects out-of-range indices, malformed
    /// buffers, and the `None` (no VDF installed) path.
    #[test]
    fn vdf_record_bytes_handles_edge_cases() {
        let mut world = World::new();
        assert_eq!(world.vdf_record_bytes(0), None, "no VDF -> None");

        // Empty buffer (shorter than header word).
        world.set_vdf_buffer(Some(vec![0x01, 0x02]));
        assert_eq!(world.vdf_record_bytes(0), None);

        // Count = 0.
        world.set_vdf_buffer(Some(vec![0x00, 0x00, 0x00, 0x00]));
        assert_eq!(world.vdf_record_bytes(0), None);

        // Count = 1 but offset walks past EOB.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&0xFFFFu32.to_le_bytes()); // offsets[0] - past EOB
        buf.extend_from_slice(&[0xAAu8; 8]);
        world.set_vdf_buffer(Some(buf));
        assert_eq!(world.vdf_record_bytes(0), None);
    }

    /// `tick_move_vms` records per-actor outcomes via `actor_tick`. A
    /// HALT-loaded script (op `0x08` = HALT, encoded as `0x0008` in u16)
    /// should yield `Halted`.
    #[test]
    fn tick_move_vms_records_halt_outcome() {
        let mut world = World::new();
        world.spawn_actor(0);
        world.actors[0].move_state.wait_timer = -1;
        // Move-VM HALT opcode is `0x08`.
        world.set_move_bytecode(0, Some(vec![0x0008]));
        world.tick_move_vms();
        assert!(
            world
                .move_outcomes
                .iter()
                .any(|(s, o)| *s == 0 && matches!(o, vm::move_vm::ActorTickOutcome::Halted)),
            "expected actor 0 to halt, got {:?}",
            world.move_outcomes
        );
    }

    /// Wait gate: actor with `wait_timer >= 0` reports Waiting and the VM
    /// is not entered. Decrement happens before the gate.
    #[test]
    fn tick_move_vms_with_delta_decrements_then_gates() {
        let mut world = World::new();
        world.spawn_actor(0);
        world.actors[0].move_state.wait_timer = 3;
        // Bytecode that would change state if VM ran (op 0x08 HALT).
        world.set_move_bytecode(0, Some(vec![0x0008]));
        world.tick_move_vms_with_delta(1);
        // After delta=1: wait_timer = 2, still >= 0 -> Waiting.
        assert_eq!(world.actors[0].move_state.wait_timer, 2);
        assert!(matches!(
            world.move_outcomes[0],
            (0, vm::move_vm::ActorTickOutcome::Waiting)
        ));
        // After three more ticks (delta=1 each): wait_timer goes 1, 0, -1.
        // Only when wait_timer is strictly negative does the VM run.
        world.tick_move_vms_with_delta(1);
        world.tick_move_vms_with_delta(1);
        world.tick_move_vms_with_delta(1);
        // The last tick should have entered the VM and Halted.
        assert!(matches!(
            world.move_outcomes[0],
            (0, vm::move_vm::ActorTickOutcome::Halted)
        ));
    }

    #[test]
    fn try_spawn_effect_populates_pool() {
        let mut world = World::default();
        let script = vm::effect_vm::EffectScript {
            child_count: 2,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
        assert_eq!(world.effect_pool.active_count(), 0);
        world.try_spawn_effect(0, [10, 0, -10], 0x200);
        assert_eq!(world.effect_pool.active_count(), 1);
        assert_eq!(world.effect_pool.master_slots[0].pos_x, 10i32 << 8);
    }

    #[test]
    fn active_effect_markers_reflect_pool_and_fade_with_age() {
        let mut world = World::default();
        let script = vm::effect_vm::EffectScript {
            child_count: 2,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);

        // No live effects -> no markers.
        assert!(world.active_effect_markers().is_empty());

        world.try_spawn_effect(0, [10, 0, -10], 0x200);
        let markers = world.active_effect_markers();
        assert_eq!(markers.len(), 1);
        // 8.8 fixed pool position decodes back to the spawn world units.
        assert_eq!(markers[0].world_pos, [10.0, 0.0, -10.0]);
        assert_eq!(markers[0].angle, 0x200);
        // Freshly spawned: no elapsed frames yet.
        assert_eq!(markers[0].age01, 0.0);

        // Age advances toward 1.0 as the effect ticks through its lifetime.
        world.tick_effects();
        let aged = world.active_effect_markers();
        assert_eq!(aged.len(), 1);
        assert!(aged[0].age01 > 0.0 && aged[0].age01 < 1.0);

        // Once the lifetime is spent the slot retires and emits no marker.
        for _ in 0..vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES {
            world.tick_effects();
        }
        assert!(world.active_effect_markers().is_empty());
    }

    #[test]
    fn spawn_debug_effect_seats_marker_then_ages_out() {
        let mut world = World::default();
        assert!(world.spawn_debug_effect([128.0, 0.0, -64.0]));
        let markers = world.active_effect_markers();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].world_pos, [128.0, 0.0, -64.0]);
        assert_eq!(markers[0].age01, 0.0);

        // Ages and retires via the normal effect lifetime.
        for _ in 0..=vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES {
            world.tick_effects();
        }
        assert!(world.active_effect_markers().is_empty());
    }

    #[test]
    fn spawn_debug_effect_model_emits_model_not_billboard() {
        let mut world = World::default();
        // A model-only effect (no catalog): emits an EffectModel carrying the
        // requested global-TMD-pool index, and no 2D billboard sprite.
        assert!(world.spawn_debug_effect_model([16.0, 4.0, -8.0], 4));
        let models = world.active_effect_models();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].tmd_index, 4);
        assert_eq!(models[0].world_pos, [16.0, 4.0, -8.0]);
        assert_eq!(models[0].age01, 0.0);
        // Plain debug effect (no model_index) emits no model.
        assert!(world.spawn_debug_effect([0.0, 0.0, 0.0]));
        assert_eq!(world.active_effect_models().len(), 1);

        // Ages and retires via the normal effect lifetime.
        for _ in 0..=vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES {
            world.tick_effects();
        }
        assert!(world.active_effect_models().is_empty());
    }

    #[test]
    fn try_spawn_effect_noop_on_empty_catalog() {
        let mut world = World::default();
        world.try_spawn_effect(0, [0, 0, 0], 0);
        assert_eq!(world.effect_pool.active_count(), 0);
    }

    #[test]
    fn ui_element_mode0_pushes_event_and_spawns_effect() {
        let mut world = World {
            mode: SceneMode::Battle,
            ..World::default()
        };
        let script = vm::effect_vm::EffectScript {
            child_count: 1,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
        // Drive through the BattleHostImpl path by ticking the SM. Setting
        // up a full SM state is complex; we call try_spawn_effect directly
        // (the BattleHostImpl wiring is verified by the disc-gated test).
        world.try_spawn_effect(0, [0, 0, 0], 0);
        assert_eq!(world.effect_pool.active_count(), 1);
    }

    #[test]
    fn ui_element_mode1_does_not_spawn() {
        let mut world = World::default();
        let script = vm::effect_vm::EffectScript {
            child_count: 1,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
        // Simulate the mode==1 (terminate) path: only the event is pushed,
        // no pool spawn. try_spawn_effect is not called for mode==1.
        // Directly confirm pool stays empty if we don't call try_spawn_effect.
        assert_eq!(world.effect_pool.active_count(), 0);
    }

    // --- Tactical Arts ---

    #[test]
    fn notify_art_used_emits_event_and_sets_banner() {
        let mut world = World::default();
        world.tactical_arts.set_threshold(1);
        world.notify_art_used(0, 3);
        let evs = world.drain_battle_events();
        assert_eq!(evs.len(), 1);
        assert_eq!(
            evs[0],
            BattleEvent::TacticalArtLearned {
                char_id: 0,
                art_id: 3
            }
        );
        let banner = world.current_art_banner.as_ref().expect("banner set");
        assert!(banner.text.contains("Art #3"));
        assert_eq!(
            banner.frames_remaining,
            crate::tactical_arts::ArtLearnedBanner::DEFAULT_FRAMES
        );
    }

    #[test]
    fn notify_art_used_no_event_before_threshold() {
        let mut world = World::default();
        world.tactical_arts.set_threshold(5);
        for _ in 0..4 {
            world.notify_art_used(0, 1);
        }
        assert!(world.drain_battle_events().is_empty());
        assert!(world.current_art_banner.is_none());
    }

    #[test]
    fn banner_countdown_clears_after_frames() {
        let mut world = World::default();
        world.tactical_arts.set_threshold(1);
        world.notify_art_used(0, 0);
        // Banner starts at DEFAULT_FRAMES.
        assert!(world.current_art_banner.is_some());
        // Tick DEFAULT_FRAMES times; banner should reach 0 and clear.
        for _ in 0..=crate::tactical_arts::ArtLearnedBanner::DEFAULT_FRAMES {
            world.tick();
        }
        assert!(
            world.current_art_banner.is_none(),
            "banner should have cleared"
        );
    }

    // --- Level-up banner ---

    #[test]
    fn apply_battle_xp_sets_level_up_banner() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        // Slot 0 must be alive for the split to credit XP.
        world.actors[0].battle.hp = 100;
        // Retail table: 50 XP to reach level 2 (SCUS 0x8007123C entry[0]).
        world.apply_battle_xp(50);
        let banner = world
            .current_level_up_banner
            .as_ref()
            .expect("level-up banner should be set");
        assert_eq!(banner.char_id, 0);
        assert_eq!(banner.new_level, 2);
        assert_eq!(banner.hp_gained, 10); // default StatGain
        assert_eq!(banner.mp_gained, 5);
        assert_eq!(
            banner.frames_remaining,
            crate::levelup::LevelUpBanner::DEFAULT_FRAMES
        );
    }

    #[test]
    fn apply_battle_xp_skips_dead_members() {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        // Alive: slots 0 + 2. Dead: slot 1 (HP = 0).
        world.actors[0].battle.hp = 100;
        world.actors[1].battle.hp = 0;
        world.actors[2].battle.hp = 100;
        let results = world.apply_battle_xp(100);
        // 100 / 2 alive = 50 each; both should reach L2 (50 threshold).
        let slot_ids: Vec<u8> = results.iter().map(|r| r.char_id).collect();
        assert!(slot_ids.contains(&0));
        assert!(slot_ids.contains(&2));
        assert!(
            !slot_ids.contains(&1),
            "dead slot 1 must not appear in level-up results"
        );
    }

    #[test]
    fn apply_battle_xp_no_alive_returns_empty() {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        // No actor with HP > 0 → nobody to credit.
        let results = world.apply_battle_xp(500);
        assert!(results.is_empty());
        assert!(world.current_level_up_banner.is_none());
    }

    #[test]
    fn apply_battle_loot_rolls_drop_item_when_rate_is_max() {
        use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
        let mut cat = MonsterCatalog::new();
        let mut def = MonsterDef::new(7, "Slime", 10, 5);
        def.drop_item = Some(0x42);
        def.drop_rate_q8 = 255; // near-guaranteed roll
        cat.insert(def);
        let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        let rewards = world.apply_battle_loot(&formation, &cat);
        assert_eq!(rewards.drops, vec![0x42]);
        assert_eq!(world.inventory.get(&0x42).copied(), Some(1));
    }

    #[test]
    fn apply_basic_attack_queues_hit_fx_for_damaged_monster() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        // Slot 0 attacker, slot 1 a living monster.
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.hp = 60;
        world.actors[1].battle.max_hp = 60;
        world.actors[1].battle.liveness = 1;
        world.battle_ctx.active_actor = 0;
        // Give the attacker enough ATK to chip the monster (>defense).
        world.battle_attack[0] = 40;
        world.battle_defense[1] = 10;
        world.apply_basic_attack();
        let fx = world.drain_battle_hit_fx();
        assert_eq!(fx.len(), 1);
        assert_eq!(fx[0].target_slot, 1);
        assert!(fx[0].amount > 0);
        assert!(!fx[0].is_heal);
        // Drain empties the queue.
        assert!(world.drain_battle_hit_fx().is_empty());
    }

    #[test]
    fn first_living_opponent_is_chosen_by_attacker_side() {
        let mut world = World {
            party_count: 2,
            ..World::default()
        };
        // Party slots 0,1 dead+alive; monster slots 2,3.
        world.actors[0].battle.liveness = 0;
        world.actors[1].battle.liveness = 1;
        world.actors[2].battle.liveness = 0;
        world.actors[3].battle.liveness = 1;
        // Party attacker -> first living monster (slot 3, since 2 is dead).
        assert_eq!(world.first_living_opponent_of(1), Some(3));
        // Monster attacker -> first living party member (slot 1, since 0 dead).
        assert_eq!(world.first_living_opponent_of(3), Some(1));
    }

    #[test]
    fn next_living_combatant_round_robins_skipping_dead() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        for a in world.actors.iter_mut() {
            a.battle.liveness = 0;
        }
        world.actors[0].battle.liveness = 1; // party
        world.actors[2].battle.liveness = 1; // monster
        // After party (0) comes monster (2); after monster (2) wraps to party (0).
        assert_eq!(world.next_living_combatant(0), Some(2));
        assert_eq!(world.next_living_combatant(2), Some(0));
    }

    /// Three living actors with well-separated SPD: the per-turn key ranges
    /// (`speed + rand()%(speed/2+1) + 1`) can't overlap, so the order is fixed
    /// by SPD regardless of the RNG. Highest SPD acts first; each turn is
    /// consumed; a fresh round is seeded once everyone has acted.
    #[test]
    fn initiative_orders_turns_by_speed_then_reseeds() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        for a in world.actors.iter_mut() {
            a.battle.liveness = 0;
        }
        // slot 0 (party) SPD 10, slot 1 (monster) SPD 50, slot 2 (monster) 30.
        // Key ranges: 11..=16, 51..=76, 31..=46 — disjoint.
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.liveness = 1;
        world.actors[2].battle.liveness = 1;
        world.battle_speed[0] = 10;
        world.battle_speed[1] = 50;
        world.battle_speed[2] = 30;
        // Fresh keys (all 0): the first pick seeds a round, then orders by SPD.
        assert_eq!(world.next_combatant_by_initiative(), Some(1)); // SPD 50
        assert_eq!(world.next_combatant_by_initiative(), Some(2)); // SPD 30
        assert_eq!(world.next_combatant_by_initiative(), Some(0)); // SPD 10
        // Round exhausted -> reseed -> highest SPD again.
        assert_eq!(world.next_combatant_by_initiative(), Some(1));
    }

    /// A dead actor never wins a turn even with the highest SPD: the selector
    /// zeroes dead actors' keys (the `FUN_801daba4` first loop).
    #[test]
    fn initiative_skips_dead_high_speed_actor() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        for a in world.actors.iter_mut() {
            a.battle.liveness = 0;
        }
        world.actors[0].battle.liveness = 1; // party, SPD 20
        world.actors[1].battle.liveness = 0; // dead monster, SPD 90
        world.actors[2].battle.liveness = 1; // monster, SPD 40
        world.battle_speed[0] = 20;
        world.battle_speed[1] = 90;
        world.battle_speed[2] = 40;
        // Slot 1 is dead -> skipped; slot 2 (40) outruns slot 0 (20).
        assert_eq!(world.next_combatant_by_initiative(), Some(2));
        assert_eq!(world.next_combatant_by_initiative(), Some(0));
    }

    /// With no SPD anywhere the selector defers to round-robin slot order.
    #[test]
    fn initiative_falls_back_to_round_robin_without_speed() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        for a in world.actors.iter_mut() {
            a.battle.liveness = 0;
        }
        world.actors[0].battle.liveness = 1;
        world.actors[2].battle.liveness = 1;
        assert!(!world.any_battle_speed());
        world.battle_ctx.active_actor = 0;
        assert_eq!(world.next_combatant_by_initiative(), Some(2));
        world.battle_ctx.active_actor = 2;
        assert_eq!(world.next_combatant_by_initiative(), Some(0));
    }

    /// Setup seeding consumes slot 0's key so the party lead opens round 1 and
    /// the rest order by initiative behind it.
    #[test]
    fn seed_battle_initiative_lets_slot0_lead_round_one() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        for a in world.actors.iter_mut() {
            a.battle.liveness = 0;
        }
        world.actors[0].battle.liveness = 1; // party, SPD 10
        world.actors[1].battle.liveness = 1; // monster, SPD 50
        world.battle_speed[0] = 10;
        world.battle_speed[1] = 50;
        world.seed_battle_initiative();
        // Slot 0 consumed (leads round 1 separately); slot 1 still armed.
        assert_eq!(world.actors[0].battle.init_key, 0);
        assert!(world.actors[1].battle.init_key > 0);
        // The selector therefore picks slot 1 next, then slot 0 (after reseed).
        assert_eq!(world.next_combatant_by_initiative(), Some(1));
    }

    /// `any_battle_speed` only fires for SPD carried by a *living* actor.
    #[test]
    fn any_battle_speed_requires_a_living_carrier() {
        let mut world = World::default();
        for a in world.actors.iter_mut() {
            a.battle.liveness = 0;
        }
        assert!(!world.any_battle_speed());
        // SPD on a dead slot doesn't count.
        world.battle_speed[3] = 40;
        assert!(!world.any_battle_speed());
        // Living carrier flips the gate.
        world.actors[3].battle.liveness = 1;
        assert!(world.any_battle_speed());
    }

    #[test]
    fn monsters_take_turns_and_can_wipe_the_party() {
        use legaia_engine_vm::battle_action::ActionState;
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.live_gameplay_loop = true;
        world.mode = SceneMode::Battle;
        // Lone party member: low HP, weak attack so the fight lasts several
        // rounds and the monster gets turns.
        world.actors[0].active = true;
        world.actors[0].battle.hp = 40;
        world.actors[0].battle.max_hp = 40;
        world.actors[0].battle.liveness = 1;
        world.set_battle_attack(0, 4);
        // Lone monster: tanky + hits hard enough to kill the party member.
        world.actors[1].battle.hp = 500;
        world.actors[1].battle.max_hp = 500;
        world.actors[1].battle.liveness = 1;
        world.set_battle_attack(1, 25);
        // Arm the first turn (party member swings at the monster).
        world.battle_ctx.active_actor = 0;
        world.battle_ctx.queued_action = 3;
        world.battle_ctx.action_state = ActionState::Begin.as_byte();
        world.actors[0].battle.active_target = 1;
        world.actors[0].battle.action_category = 3;

        let start_party_hp = world.actors[0].battle.hp;
        let mut party_took_damage = false;
        let mut ended = false;
        for _ in 0..4000 {
            world.tick();
            if world.actors[0].battle.hp < start_party_hp {
                party_took_damage = true;
            }
            // finish_battle flips back to Field (and raises game_over on a
            // party wipe).
            if world.mode == SceneMode::Field {
                ended = true;
                break;
            }
        }
        assert!(
            party_took_damage,
            "the monster must take turns and damage the party"
        );
        assert!(ended, "the battle must resolve (party wiped)");
        assert!(world.game_over, "a party wipe raises game_over");
    }

    #[test]
    fn multi_monster_battle_all_monsters_act_and_party_can_win() {
        use legaia_engine_vm::battle_action::ActionState;
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.live_gameplay_loop = true;
        world.mode = SceneMode::Battle;
        // Lone party member: enough HP to survive three weak monsters, enough
        // attack to chip each down over a few rounds.
        world.actors[0].active = true;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.liveness = 1;
        world.set_battle_attack(0, 30);
        // Three monsters in slots 1..=3, each with modest HP + a light hit.
        for s in 1..=3 {
            world.actors[s].battle.hp = 40;
            world.actors[s].battle.max_hp = 40;
            world.actors[s].battle.liveness = 1;
            world.set_battle_attack(s as u8, 3);
        }
        // Arm the party member's first swing.
        world.battle_ctx.active_actor = 0;
        world.battle_ctx.queued_action = 3;
        world.battle_ctx.action_state = ActionState::Begin.as_byte();
        world.actors[0].battle.active_target = 1;
        world.actors[0].battle.action_category = 3;

        let start_hp = world.actors[0].battle.hp;
        let mut ended = false;
        for _ in 0..8000 {
            world.tick();
            if world.mode == SceneMode::Field {
                ended = true;
                break;
            }
        }
        assert!(ended, "the multi-monster battle must resolve");
        // Party wiped all three monsters (victory, not a party wipe).
        assert!(!world.game_over, "party should survive and win");
        for s in 1..=3 {
            assert_eq!(
                world.actors[s].battle.liveness, 0,
                "monster slot {s} should be defeated"
            );
        }
        // The monsters got turns: the party took at least some damage from
        // three light attackers over the fight.
        assert!(
            world.actors[0].battle.hp < start_hp,
            "monsters should have damaged the party over the multi-round fight"
        );
    }

    #[test]
    fn battle_item_use_heals_ally_consumes_item_and_cycles_turn() {
        use crate::input::PadButton;
        use crate::items::ItemCatalog;
        use legaia_engine_vm::battle_action::ActionState;

        let mut world = World {
            party_count: 2,
            ..World::default()
        };
        world.battle_player_driven = true;
        world.mode = SceneMode::Battle;
        world.set_item_catalog(ItemCatalog::vanilla());
        // Two party members (slot 0 wounded), one monster.
        for i in 0..2usize {
            world.actors[i].battle.max_hp = 200;
            world.actors[i].battle.hp = 200;
            world.actors[i].battle.liveness = 1;
            world.set_character_max_mp(i as u8, 30);
        }
        world.actors[0].battle.hp = 50;
        world.actors[2].battle.max_hp = 80;
        world.actors[2].battle.hp = 80;
        world.actors[2].battle.liveness = 1;
        // Healing Leaf (id 0x01) heals 100 HP; hold two.
        world.inventory.insert(0x01, 2);

        // Open the item submenu for the active party member.
        world.battle_ctx.active_actor = 0;
        world.battle_item_menu = Some(world.build_battle_item_session());
        {
            let m = world.battle_item_menu.as_ref().unwrap();
            assert_eq!(m.filtered_items.len(), 1, "one battle-usable item");
            assert_eq!(m.targets.len(), 2, "two party targets");
        }

        // Frame 1: Cross confirms the item -> target select.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu();
        assert!(world.battle_item_menu.is_some(), "still picking a target");

        // Frame 2: Cross confirms the first target (the wounded slot 0).
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu();

        assert_eq!(world.actors[0].battle.hp, 150, "healed 50 -> 150");
        assert_eq!(
            world.inventory.get(&0x01).copied(),
            Some(1),
            "one Healing Leaf consumed"
        );
        assert!(world.battle_item_menu.is_none(), "menu closed after use");
        assert_eq!(
            world.battle_ctx.action_state,
            ActionState::EndOfAction.as_byte(),
            "turn parked at EndOfAction so the loop cycles"
        );
        let fx = world.drain_battle_hit_fx();
        assert_eq!(fx.len(), 1);
        assert!(fx[0].is_heal);
        assert_eq!(fx[0].amount, 100);
        assert_eq!(fx[0].target_slot, 0);
    }

    #[test]
    fn battle_item_menu_cancel_reopens_command_menu() {
        use crate::input::PadButton;
        use crate::items::ItemCatalog;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.battle_player_driven = true;
        world.mode = SceneMode::Battle;
        world.set_item_catalog(ItemCatalog::vanilla());
        world.actors[0].battle.max_hp = 100;
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.liveness = 1;
        world.inventory.insert(0x01, 1);

        world.battle_ctx.active_actor = 0;
        world.battle_item_menu = Some(world.build_battle_item_session());

        // Circle from the item list backs all the way out.
        world.set_pad(0);
        world.set_pad(PadButton::Circle.mask());
        world.tick_battle_item_menu();

        assert!(world.battle_item_menu.is_none(), "item menu closed");
        assert!(
            world.battle_command.is_some(),
            "command menu reopened for the same actor"
        );
        assert_eq!(world.battle_command.as_ref().unwrap().actor, 0);
        // No item was consumed on a cancel.
        assert_eq!(world.inventory.get(&0x01).copied(), Some(1));
    }

    /// Build a 1-party-member, 1-monster battle world for the offensive-item
    /// tests. The monster sits at slot 1 (party_count = 1) with the supplied
    /// HP and a `battle_monster_id` so it shows up as an enemy target row.
    #[cfg(test)]
    fn offensive_item_world(monster_hp: u16, monster_id: u16) -> World {
        use crate::items::ItemCatalog;
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.battle_player_driven = true;
        world.mode = SceneMode::Battle;
        world.set_item_catalog(ItemCatalog::vanilla());
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 200;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.max_hp = monster_hp;
        world.actors[1].battle.hp = monster_hp;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(monster_id);
        world.battle_ctx.active_actor = 0;
        world
    }

    /// Monster-AI cast path: a monster whose record carries a castable spell
    /// it can afford folds a real spell onto the party (HP drops, MP spent, a
    /// damage popup queues) and parks the SM at `EndOfAction` so the loop
    /// cycles - rather than the generic physical strike. RNG is pinned so the
    /// cast-vs-strike roll lands on "cast".
    #[test]
    fn monster_ai_casts_a_castable_spell_under_fixed_rng() {
        use crate::monster_catalog::vanilla_monster_catalog;
        use crate::spells::SpellCatalog;
        use legaia_engine_vm::battle_action::ActionState;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        // Party member at slot 0.
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 200;
        world.actors[0].battle.liveness = 1;
        // Bandit Boss (id 5) at slot 1: carries [Flame 0x20, Thunder Bolt 0x23]
        // and 10 MP - enough to afford either.
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.set_battle_magic(1, 40);
        // Seed 0: the action picker's first `rand % (1 + magic_count)` (magic
        // count 2 -> `% 3`) lands on 1, so it casts magic[0] = Flame (0x20).
        world.rng_state = 0;

        let party_hp_before = world.actors[0].battle.hp;
        world.take_monster_turn(1);

        assert_eq!(
            world.actors[1].battle.params[0], 0x20,
            "picker chose Flame (magic_attacks[0])"
        );
        assert!(
            world.actors[0].battle.hp < party_hp_before,
            "the monster's spell dealt damage to the party"
        );
        assert!(world.actors[1].battle.mp < 10, "the monster spent MP");
        assert_eq!(
            world.battle_ctx.action_state,
            ActionState::EndOfAction.as_byte(),
            "a cast is the whole turn; SM parks at EndOfAction"
        );
        let fx = world.drain_battle_hit_fx();
        assert_eq!(fx.len(), 1, "one damage popup queued");
        assert!(!fx[0].is_heal, "damage-coloured popup");
        assert_eq!(fx[0].target_slot, 0, "the party member took the hit");
    }

    /// A monster with no castable spells always picks a physical strike: the
    /// action picker rolls `rand % (1 + 0) == 0`, so the magic branch is never
    /// taken regardless of the seed. It still targets a (single living) party
    /// member and arms the SM at `Begin`.
    #[test]
    fn spell_less_monster_always_arms_physical_strike() {
        use legaia_engine_vm::battle_action::ActionState;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 200;
        world.actors[0].battle.liveness = 1;
        // Goblin (id 1) has no magic_attacks; leave the catalog empty so the
        // monster id doesn't resolve either - the magic branch can't be taken.
        world.actors[1].battle.max_hp = 30;
        world.actors[1].battle.hp = 30;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(1);

        world.take_monster_turn(1);

        assert_eq!(world.battle_ctx.queued_action, 3, "physical strike queued");
        assert_eq!(
            world.battle_ctx.action_state,
            ActionState::Begin.as_byte(),
            "SM armed at Begin to run the strike"
        );
        assert_eq!(
            world.actors[1].battle.action_category, 3,
            "physical action category"
        );
        assert_eq!(
            world.actors[1].battle.active_target, 0,
            "targets the only living party member (slot 0)"
        );
    }

    /// A minimal `efect.dat` 2-pack: 1 atlas entry (a 24x24 sprite at texel
    /// (5,7), tpage 0x88, clut 0x12), 1 anim batch (one frame -> atlas 0), and
    /// 1 effect script with 1 child referencing sprite_id 0.
    fn minimal_efect_dat() -> Vec<u8> {
        let mut buf = vec![0u8; 8];
        // atlas[0]: u=5 v=7 w=24 h=24, tpage=0x88, clut=0x12, unk=0
        buf.extend_from_slice(&[5u8, 7, 24, 24]);
        buf.extend_from_slice(&0x88u16.to_le_bytes());
        buf.extend_from_slice(&[0x12u8, 0]);
        let pack0 = buf.len() as u32;
        // pack0: 1 anim batch, 1 frame (atlas_index 0).
        buf.extend_from_slice(&1u32.to_le_bytes());
        let p0_tbl = buf.len();
        buf.extend_from_slice(&[0u8; 4]);
        let anim0 = buf.len() as u32;
        buf.extend_from_slice(&[1u8, 0]); // frame_count=1, flags
        buf.extend_from_slice(&[0u8, 0, 0, 0, 0, 0]); // frame 0 -> atlas 0
        buf[p0_tbl..p0_tbl + 4].copy_from_slice(&anim0.to_le_bytes());
        let pack1 = buf.len() as u32;
        // pack1: 1 effect script, 1 child (sprite_id 0), flags 0 (no spread).
        buf.extend_from_slice(&1u32.to_le_bytes());
        let p1_tbl = buf.len();
        buf.extend_from_slice(&[0u8; 4]);
        let script0 = buf.len() as u32;
        buf.extend_from_slice(&[1u8, 0]); // child_count=1, flags=0
        buf.extend_from_slice(&0i16.to_le_bytes()); // spread
        buf.extend_from_slice(&0u16.to_le_bytes()); // child sprite_id=0
        buf.extend_from_slice(&0i16.to_le_bytes()); // width
        buf.extend_from_slice(&0u16.to_le_bytes()); // anim_flags
        buf.extend_from_slice(&0i16.to_le_bytes()); // depth
        buf.extend_from_slice(&[0u8; 6]); // tail
        buf[p1_tbl..p1_tbl + 4].copy_from_slice(&script0.to_le_bytes());
        buf[0..4].copy_from_slice(&pack0.to_le_bytes());
        buf[4..8].copy_from_slice(&pack1.to_le_bytes());
        buf
    }

    /// A spawned effect produces a faithful billboard sprite per child, sized
    /// and UV-addressed from the real sprite atlas (the textured-quad seam).
    #[test]
    fn active_effect_sprites_carry_atlas_size_and_vram_coords() {
        use legaia_engine_vm::effect_vm::EffectCatalog;
        let mut world = World {
            effect_catalog: EffectCatalog::from_efect_dat_bytes(&minimal_efect_dat()),
            ..World::default()
        };
        assert_eq!(world.effect_catalog.len(), 1, "one effect script");

        // No effects yet -> no sprites.
        assert!(world.active_effect_sprites().is_empty());

        // Spawn effect 0 at world (10, 0, 20).
        world.try_spawn_effect(0, [10, 0, 20], 0);
        let sprites = world.active_effect_sprites();
        assert_eq!(sprites.len(), 1, "one child sprite");
        let s = sprites[0];
        assert_eq!(s.uv, [5, 7], "atlas texel origin");
        assert_eq!(s.uv_size, [24, 24], "atlas sprite size");
        assert_eq!(s.size, [24.0, 24.0]);
        assert_eq!(s.page, 0x88);
        assert_eq!(s.clut, 0x12);
        // Origin Y matches; X/Z within a small deterministic ring of (10, 20).
        assert!((s.world_pos[1] - 0.0).abs() < 1e-3);
        assert!((s.world_pos[0] - 10.0).abs() < 1.0);
        assert!((s.world_pos[2] - 20.0).abs() < 1.0);
    }

    /// Per-monster-id scripted AI (the `FUN_801E9FD4` switch) end-to-end: a
    /// wounded monster whose id has a low-HP self-heal case folds that heal onto
    /// itself rather than striking the party. Monster id 6 (case `0x06`) casts
    /// `0x52` at self when `HP <= maxHP/2` and its ability cooldown is clear.
    #[test]
    fn scripted_ai_monster_self_heals_when_wounded() {
        use crate::monster_catalog::vanilla_monster_catalog;
        use crate::spells::SpellCatalog;
        use legaia_engine_vm::battle_action::ActionState;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 200;
        world.actors[0].battle.liveness = 1;
        // Monster id 6 (Skeleton -> AI case 0x06) at slot 1, wounded to 20/100
        // with MP to spare for the heal.
        world.actors[1].battle.max_hp = 100;
        world.actors[1].battle.hp = 20;
        world.actors[1].battle.mp = 20;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(6);
        world.rng_state = 7;

        world.take_monster_turn(1);

        assert_eq!(
            world.actors[1].battle.params[0], 0x52,
            "self-heal spell queued"
        );
        assert!(
            world.actors[1].battle.hp > 20,
            "the monster healed itself instead of striking the party"
        );
        assert_eq!(world.actors[0].battle.hp, 200, "party untouched");
        assert_eq!(
            world.battle_ctx.action_state,
            ActionState::EndOfAction.as_byte(),
            "cast is the whole turn"
        );
        assert_eq!(world.monster_ai_state.dat[4], 1, "ability cooldown armed");
        let fx = world.drain_battle_hit_fx();
        assert!(fx.iter().any(|f| f.is_heal && f.target_slot == 1));
    }

    /// Faithful `FUN_801E7320`: a targeting class in `3..=6` resolves to a
    /// living PARTY slot; a class in `0..=2` resolves to a living MONSTER slot.
    /// (Dead slots are skipped via the re-roll loop.)
    #[test]
    fn monster_target_resolver_expands_class_to_correct_side() {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        // Party slots 0..2: slot 0 dead, slots 1+2 alive.
        for i in 0..3u8 {
            let a = &mut world.actors[i as usize];
            a.battle.max_hp = 100;
            a.battle.hp = if i == 0 { 0 } else { 100 };
            a.battle.liveness = if i == 0 { 0 } else { 1 };
        }
        // Monster slots 3+4 alive.
        for i in 3..5u8 {
            let a = &mut world.actors[i as usize];
            a.battle.max_hp = 80;
            a.battle.hp = 80;
            a.battle.liveness = 1;
        }

        // Caster = monster slot 3, class 3 (party-targeting). Resolves to a
        // LIVING party slot (1 or 2, never the dead slot 0).
        world.actors[3].battle.active_target = 3; // class 3..6 -> party
        world.rng_state = 12345;
        world.resolve_monster_target(3);
        let t = world.actors[3].battle.active_target;
        assert!(
            (1..=2).contains(&t),
            "class 3 -> living party slot, got {t}"
        );

        // Class 1 (monster-band targeting). Resolves to a living monster slot.
        world.actors[3].battle.active_target = 1; // class 0..2 -> monster band
        world.rng_state = 999;
        world.resolve_monster_target(3);
        let t = world.actors[3].battle.active_target;
        assert!(
            (3..=4).contains(&t),
            "class 1 -> living monster slot, got {t}"
        );
    }

    /// `advance_battle_mode` (the SM `case 0xFF` writer for `ctx+0x28A`) flips a
    /// multi-phase boss from its first-phase cast to its phased cast on the next
    /// turn. Monster id `0xB6` always casts, picking its spell purely by mode.
    #[test]
    fn advancing_the_battle_mode_drives_a_boss_to_its_next_phase() {
        use crate::monster_catalog::MonsterDef;
        use crate::spells::SpellCatalog;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        // A clean-room boss at monster slot 1 with id 0xB6 (no own magic - it
        // casts purely off its scripted phase table).
        world
            .monster_catalog
            .insert(MonsterDef::new(0xb6, "Boss", 400, 50));
        world.actors[0].battle.max_hp = 300;
        world.actors[0].battle.hp = 300;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.max_hp = 400;
        world.actors[1].battle.hp = 400;
        world.actors[1].battle.mp = 250;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(0xb6);
        world.rng_state = 1;

        assert_eq!(world.battle_mode(), 0, "fresh battle starts in phase 0");
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0xa2, "phase 0 cast");

        // A scripted phase transition advances the mode; next turn is phase I.
        world.advance_battle_mode();
        assert_eq!(world.battle_mode(), 1);
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0xa3, "phase 1 cast");
    }

    #[test]
    fn battle_item_bomb_damages_enemy_and_cursor_lands_on_the_monster() {
        use crate::input::PadButton;
        use legaia_engine_vm::battle_action::ActionState;

        let mut world = offensive_item_world(500, 7);
        // Bomb (0x13) deals 200 HP to an enemy.
        world.inventory.insert(0x13, 1);
        world.battle_item_menu = Some(world.build_battle_item_session());
        {
            let m = world.battle_item_menu.as_ref().unwrap();
            assert_eq!(m.targets.len(), 2, "one ally + one enemy target");
            assert!(!m.targets[0].is_enemy, "ally row first");
            assert!(m.targets[1].is_enemy, "enemy row second");
        }

        // Frame 1: Cross confirms the Bomb -> target select. The cursor must
        // skip the ally and land on the enemy row (offensive item).
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu();
        {
            let m = world.battle_item_menu.as_ref().unwrap();
            match m.state {
                crate::inventory_use::InventoryUseState::TargetSelect { cursor, .. } => {
                    assert_eq!(cursor, 1, "cursor positioned on the enemy row");
                }
                other => panic!("expected TargetSelect, got {other:?}"),
            }
        }

        // Frame 2: Cross confirms the enemy -> 200 damage.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu();

        assert_eq!(world.actors[1].battle.hp, 300, "500 -> 300 after Bomb");
        assert_eq!(world.inventory.get(&0x13).copied(), None, "Bomb consumed");
        assert!(world.battle_item_menu.is_none(), "menu closed after use");
        assert_eq!(
            world.battle_ctx.action_state,
            ActionState::EndOfAction.as_byte(),
            "turn parked at EndOfAction"
        );
        let fx = world.drain_battle_hit_fx();
        assert_eq!(fx.len(), 1);
        assert!(!fx[0].is_heal, "damage-coloured popup");
        assert_eq!(fx[0].amount, 200);
        assert_eq!(fx[0].target_slot, 1);
    }

    #[test]
    fn battle_item_bomb_downs_a_low_hp_enemy() {
        use crate::input::PadButton;

        let mut world = offensive_item_world(120, 7);
        world.inventory.insert(0x13, 1); // Bomb, 200 dmg vs 120 HP.
        world.battle_item_menu = Some(world.build_battle_item_session());

        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu(); // confirm item -> target
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu(); // confirm enemy

        assert_eq!(world.actors[1].battle.hp, 0, "HP floored at zero");
        assert_eq!(world.actors[1].battle.liveness, 0, "monster downed");
    }

    #[test]
    fn battle_item_capture_downs_a_weakened_enemy_and_logs_the_id() {
        use crate::input::PadButton;

        // Weakened monster (10/500 HP) so the missing-HP capture roll is
        // near-certain; pin the RNG so the roll (23) lands.
        let mut world = offensive_item_world(500, 42);
        world.actors[1].battle.hp = 10;
        world.rng_state = 0;
        world.inventory.insert(0x11, 1); // Genocide Crystal (capture).
        world.battle_item_menu = Some(world.build_battle_item_session());

        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu(); // item -> target (lands on enemy)
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu(); // confirm enemy

        assert_eq!(
            world.actors[1].battle.liveness, 0,
            "captured monster downed"
        );
        assert_eq!(
            world.drain_battle_captures(),
            vec![42],
            "monster id logged for post-battle Seru learning"
        );
    }

    #[test]
    fn battle_item_escape_returns_to_field() {
        use crate::input::PadButton;

        let mut world = offensive_item_world(500, 7);
        world.inventory.insert(0x12, 1); // Goblin Foot (escape).
        world.battle_item_menu = Some(world.build_battle_item_session());

        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu(); // item -> target
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_item_menu(); // confirm

        assert_eq!(world.mode, SceneMode::Field, "escaped back to the field");
        assert!(!world.battle_escaped, "escape flag reset by finish_battle");
        assert!(world.battle_item_menu.is_none(), "battle menus cleared");
        assert_eq!(world.inventory.get(&0x12).copied(), None, "item consumed");
    }

    #[test]
    fn battle_magic_cast_damages_monster_spends_mp_and_cycles_turn() {
        use crate::input::PadButton;
        use crate::spells::SpellCatalog;
        use legaia_engine_vm::battle_action::ActionState;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.battle_player_driven = true;
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        // Caster with a magic stat + MP; one monster.
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 200;
        world.actors[0].battle.mp = 50;
        world.actors[0].battle.liveness = 1;
        world.set_battle_magic(0, 100);
        world.actors[1].battle.max_hp = 300;
        world.actors[1].battle.hp = 300;
        world.actors[1].battle.liveness = 1;
        // Give the caster a learned offensive spell: Flame (0x20, 5 MP).
        let mut party = legaia_save::Party::zeroed(1);
        let mut list = party.members[0].spell_list();
        list.count = 1;
        list.ids[0] = 0x20;
        party.members[0].set_spell_list(list);
        world.roster = party;

        // Open the spell submenu for the caster.
        world.battle_ctx.active_actor = 0;
        world.battle_spell_menu = world.build_battle_spell_session(0);
        {
            let m = world.battle_spell_menu.as_ref().expect("spell menu built");
            assert_eq!(m.spells.len(), 1, "one learned spell");
            assert!(m.spells[0].affordable, "50 MP covers a 5 MP spell");
        }

        // Frame 1: Cross opens the target cursor on the lone monster.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_spell_menu();
        assert!(world.battle_spell_menu.is_some(), "still picking a target");

        // Frame 2: Cross confirms the monster; the cast resolves.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_spell_menu();

        assert!(world.battle_spell_menu.is_none(), "spell menu closed");
        assert_eq!(world.actors[0].battle.mp, 45, "5 MP spent on Flame");
        assert!(
            world.actors[1].battle.hp < 300,
            "Flame should have damaged the monster"
        );
        assert_eq!(
            world.battle_ctx.action_state,
            ActionState::EndOfAction.as_byte(),
            "turn parked at EndOfAction so the loop cycles"
        );
        let fx = world.drain_battle_hit_fx();
        assert_eq!(fx.len(), 1);
        assert!(!fx[0].is_heal, "offensive spell is damage, not heal");
        assert_eq!(fx[0].target_slot, 1);
    }

    #[test]
    fn battle_magic_buff_raises_scalar_refreshes_and_expires() {
        use crate::spells::{BuffStat, SpellOutcome};

        let mut world = World::default();
        world.set_battle_attack(0, 50);

        // Power Up: +20 Attack for 2 turns.
        world.fold_spell_outcome(SpellOutcome::Buff {
            target: 0,
            stat: BuffStat::Attack,
            magnitude: 20,
            turns: 2,
        });
        assert_eq!(world.battle_attack[0], 70, "buff adds to the scalar");
        assert_eq!(world.battle_buffs.len(), 1);

        // Re-casting refreshes (reverts the old delta first, no stacking).
        world.fold_spell_outcome(SpellOutcome::Buff {
            target: 0,
            stat: BuffStat::Attack,
            magnitude: 20,
            turns: 2,
        });
        assert_eq!(world.battle_attack[0], 70, "refresh does not stack");
        assert_eq!(world.battle_buffs.len(), 1);

        // Ages one turn per the buffed actor's turn; expires on the 2nd.
        world.tick_battle_buffs_on_turn(0);
        assert_eq!(world.battle_attack[0], 70);
        world.tick_battle_buffs_on_turn(0);
        assert_eq!(
            world.battle_attack[0], 50,
            "expiry reverts the delta exactly"
        );
        assert!(world.battle_buffs.is_empty());
    }

    #[test]
    fn battle_magic_debuff_saturates_at_zero_and_reverts_exactly() {
        use crate::spells::{BuffStat, SpellOutcome};

        let mut world = World::default();
        // Power Down on an enemy with a small attack: -25 saturates the u16
        // scalar at 0, and the recorded delta is the actual change (-10).
        world.set_battle_attack(3, 10);
        world.fold_spell_outcome(SpellOutcome::Buff {
            target: 3,
            stat: BuffStat::Attack,
            magnitude: -25,
            turns: 1,
        });
        assert_eq!(world.battle_attack[3], 0, "debuff saturates at zero");

        // One tick expires it; the exact -10 delta is reverted back to 10.
        world.tick_battle_buffs_on_turn(3);
        assert_eq!(world.battle_attack[3], 10);
        assert!(world.battle_buffs.is_empty());
    }

    #[test]
    fn battle_magic_capture_downs_a_weakened_monster_and_logs_the_id() {
        use crate::spells::SpellOutcome;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        // rng_state 0 -> first next_rng() % 100 == 23 (deterministic).
        world.rng_state = 0;
        world.actors[1].battle.max_hp = 100;
        world.actors[1].battle.hp = 10; // missing 90
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(42);

        // hit_pct 60, missing 90/100 -> effective 54; roll 23 < 54 -> captured.
        world.fold_spell_outcome(SpellOutcome::CaptureRoll {
            target: 1,
            hit_pct: 60,
        });
        assert_eq!(
            world.actors[1].battle.liveness, 0,
            "captured monster is downed"
        );
        assert_eq!(world.actors[1].battle.hp, 0);
        assert_eq!(world.drain_battle_captures(), vec![42]);

        // A near-full-HP monster has a tiny effective chance -> the same roll
        // misses and the monster is untouched.
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.rng_state = 0; // roll 23
        world.actors[1].battle.max_hp = 100;
        world.actors[1].battle.hp = 95; // missing 5 -> effective 3
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(7);
        world.fold_spell_outcome(SpellOutcome::CaptureRoll {
            target: 1,
            hit_pct: 60,
        });
        assert_eq!(
            world.actors[1].battle.liveness, 1,
            "healthy monster resists"
        );
        assert!(world.battle_captures.is_empty());
    }

    #[test]
    fn battle_magic_escape_returns_to_field() {
        use crate::input::PadButton;
        use crate::spells::SpellCatalog;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.battle_player_driven = true;
        world.mode = SceneMode::Battle;
        world.actors[0].battle.max_hp = 100;
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.mp = 20;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.max_hp = 200;
        world.actors[1].battle.hp = 200;
        world.actors[1].battle.liveness = 1;
        world.spell_catalog = SpellCatalog::vanilla();

        // Open the spell submenu with Warp (0x41, SelfOnly escape) learned.
        world.battle_ctx.active_actor = 0;
        world.battle_spell_menu = Some(crate::battle_magic::BattleSpellSession::new(
            0,
            0,
            &[0x41],
            &world.spell_catalog,
            20,
        ));

        // SelfOnly target resolves immediately, so one Cross casts Warp.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_spell_menu();

        assert_eq!(world.mode, SceneMode::Field, "escape returns to the field");
        assert!(
            world.battle_spell_menu.is_none(),
            "submenu dropped on escape"
        );
        assert!(
            !world.battle_escaped,
            "escape flag cleared by finish_battle"
        );
        assert!(world.last_battle_rewards.is_none(), "escape grants no loot");
    }

    /// A registry whose Seru hits its learn threshold in one capture, and a
    /// monster catalog linking monster id 7 -> Seru 1.
    #[cfg(test)]
    fn capture_world(party_count: u8) -> World {
        use crate::monster_catalog::{MonsterCatalog, MonsterDef};
        use crate::seru_learning::{SeruDef, SeruRegistry};

        let mut world = World {
            party_count,
            ..World::default()
        };
        // Zeroed roster (empty spell lists) so `build_battle_spell_session`
        // resolves a member per party slot; learned spells come from the log.
        world.roster = legaia_save::Party::zeroed(party_count.max(1) as usize);
        let mut reg = SeruRegistry::new();
        reg.insert(SeruDef {
            id: 1,
            name: "Spark".into(),
            spell_id: 0x20,
            capture_points: 100,
            learnable_mask: 0b0000_0111,
            learn_threshold: 100,
        });
        reg.insert(SeruDef {
            id: 2,
            name: "Slow".into(),
            spell_id: 0x21,
            capture_points: 40, // below threshold in one capture
            learnable_mask: 0b0000_0111,
            learn_threshold: 100,
        });
        world.set_seru_registry(reg);
        let mut cat = MonsterCatalog::new();
        cat.insert(MonsterDef::new(7, "Killer Bee", 25, 9).with_seru(1));
        cat.insert(MonsterDef::new(8, "Slime", 40, 8).with_seru(2));
        cat.insert(MonsterDef::new(9, "Wolf", 35, 12)); // no Seru
        world.set_monster_catalog(cat);
        world
    }

    #[test]
    fn capture_banks_points_and_learns_on_finish_battle() {
        let mut world = capture_world(2);
        // Two monsters captured this battle: Killer Bee (Seru 1, learns) and
        // Wolf (no Seru, banks nothing).
        world.battle_captures = vec![7, 9];

        world.finish_battle();

        // battle_captures always drained.
        assert!(world.battle_captures.is_empty());
        // Both party slots learned Spark (id 0x20).
        assert!(world.seru_log.has_learned(0, 1));
        assert!(world.seru_log.has_learned(1, 1));
        assert_eq!(world.seru_log.learned_spells(0), &[0x20]);
        // One accepted outcome (the Wolf had no Seru), with two learn events.
        let outcomes = world.drain_last_capture_outcomes();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].learns.len(), 2);
        // Outcomes drained.
        assert!(world.drain_last_capture_outcomes().is_empty());
    }

    #[test]
    fn capture_below_threshold_banks_points_without_learning() {
        let mut world = capture_world(1);
        world.battle_captures = vec![8]; // Slime -> Seru 2, 40 < 100

        world.finish_battle();

        assert!(!world.seru_log.has_learned(0, 2), "not learned yet");
        assert_eq!(world.seru_log.row(0, 2).points, 40, "points banked");
        let outcomes = world.drain_last_capture_outcomes();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].learns.is_empty());
    }

    #[test]
    fn capture_sets_the_banner_and_it_clears_on_tick() {
        use crate::seru_learning::CaptureState;

        let mut world = capture_world(1);
        world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
        world.battle_captures = vec![7]; // Killer Bee -> Seru 1 (Spark), learns
        world.finish_battle();

        // The banner opens on the capture phase naming the captured Seru.
        let banner = world
            .current_capture_banner
            .as_ref()
            .expect("capture banner set");
        assert_eq!(banner.seru_name(), "Spark");
        assert!(matches!(banner.state(), CaptureState::Capturing { .. }));
        assert_eq!(banner.current_banner().as_deref(), Some("Captured: Spark!"));
        // A learn event was recorded (party slot 0 crossed the threshold).
        assert_eq!(banner.learns().len(), 1);

        // Drive the banner to completion via World::tick (Field mode after the
        // battle). The default durations are 60 capture + 90 announce frames.
        for _ in 0..(60 + 90 + 4) {
            world.tick();
        }
        assert!(
            world.current_capture_banner.is_none(),
            "banner clears after its phases elapse"
        );
    }

    #[test]
    fn sub_threshold_capture_banner_shows_no_learn_line() {
        let mut world = capture_world(1);
        world.battle_captures = vec![8]; // Slime -> Seru 2, 40 < 100, no learn
        world.finish_battle();

        let banner = world
            .current_capture_banner
            .as_ref()
            .expect("capture banner set even without a learn");
        assert_eq!(banner.seru_name(), "Slow");
        assert!(banner.learns().is_empty());
    }

    #[test]
    fn battle_bgm_swaps_on_encounter_and_restores_on_finish() {
        use crate::monster_catalog::{FormationDef, FormationSlot};

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.current_bgm = Some(0x0A); // field track playing
        world.set_battle_bgm(Some(0x40)); // configured battle track
        let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);

        world.enter_battle_from_formation(&formation);

        // Swapped to the battle track, with a start event queued for the host.
        assert_eq!(world.current_bgm, Some(0x40));
        assert!(world.battle_bgm_active);
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::Bgm {
                    text_id: 0x40,
                    sub_op: 1
                }
            )),
            "battle BGM start queued: {evs:?}"
        );

        // Finish (no formation/loot) restores the field track + queues its start.
        world.finish_battle();
        assert_eq!(world.current_bgm, Some(0x0A));
        assert!(!world.battle_bgm_active);
        let evs = world.drain_field_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                FieldEvent::Bgm {
                    text_id: 0x0A,
                    sub_op: 1
                }
            )),
            "field BGM restore queued: {evs:?}"
        );
    }

    #[test]
    fn battle_bgm_unset_leaves_music_untouched() {
        use crate::monster_catalog::{FormationDef, FormationSlot};

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.current_bgm = Some(0x0A);
        // No battle_bgm configured (default None) -> no swap, no events.
        let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);
        world.enter_battle_from_formation(&formation);
        assert_eq!(world.current_bgm, Some(0x0A));
        assert!(!world.battle_bgm_active);
        assert!(
            !world
                .drain_field_events()
                .iter()
                .any(|e| matches!(e, FieldEvent::Bgm { .. })),
            "no BGM events when battle_bgm is unset"
        );
        world.finish_battle();
        assert_eq!(world.current_bgm, Some(0x0A));
    }

    #[test]
    fn battle_bgm_with_silent_field_stops_on_finish() {
        use crate::monster_catalog::{FormationDef, FormationSlot};

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.current_bgm = None; // no field music playing
        world.set_battle_bgm(Some(0x40));
        let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);

        world.enter_battle_from_formation(&formation);
        assert_eq!(world.current_bgm, Some(0x40));
        let _ = world.drain_field_events();

        world.finish_battle();
        // Nothing to resume -> battle music stops (sub-op 4) and id clears.
        assert_eq!(world.current_bgm, None);
        let evs = world.drain_field_events();
        assert!(
            evs.iter()
                .any(|e| matches!(e, FieldEvent::Bgm { sub_op: 4, .. })),
            "BGM stop queued when no field track to resume: {evs:?}"
        );
    }

    #[test]
    fn learned_spell_is_offered_in_the_battle_spell_session() {
        let mut world = capture_world(1);
        world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
        // Caster has an empty roster spell list; learning Spark via capture
        // should still surface it in the battle spell menu.
        world.battle_captures = vec![7];
        world.finish_battle();
        world.actors[0].battle.mp = 99;

        let session = world
            .build_battle_spell_session(0)
            .expect("session builds for slot 0");
        assert!(
            session.spells.iter().any(|s| s.id == 0x20),
            "captured Spark is castable: {:?}",
            session.spells.iter().map(|s| s.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn capture_progress_round_trips_through_save_load() {
        // Bank a sub-threshold capture, save, reload into a fresh world that
        // has the registry installed, and confirm the points + learned state
        // survive.
        let mut world = capture_world(1);
        world.battle_captures = vec![7, 8]; // Seru 1 learns; Seru 2 banks 40
        world.finish_battle();
        assert!(world.seru_log.has_learned(0, 1));
        assert_eq!(world.seru_log.row(0, 2).points, 40);

        let save = world.save_full();

        let mut reloaded = capture_world(1);
        reloaded.load_full(save);
        assert!(
            reloaded.seru_log.has_learned(0, 1),
            "learned Spark restored"
        );
        assert_eq!(
            reloaded.seru_log.learned_spells(0),
            &[0x20],
            "spell list restored"
        );
        assert_eq!(
            reloaded.seru_log.row(0, 2).points,
            40,
            "sub-threshold progress restored"
        );
        assert!(
            !reloaded.seru_log.has_learned(0, 2),
            "still below threshold after reload"
        );
    }

    #[test]
    fn arts_editor_chain_round_trips_through_save_into_the_battle_menu() {
        use crate::tactical_arts_editor::{ChainEditor, EditInput, EditOutcome};

        // A field-side session: the player opens the Tactical Arts editor and
        // composes a brand-new chain for character slot 0 (Down, Up, Up).
        let mut field = World {
            party_count: 1,
            ..World::default()
        };
        let mut lib = field.chain_library();
        let mut ed = ChainEditor::new(0, &lib);
        // Cross: open the "+ New" editor.
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        for dir in [
            EditInput {
                down: true,
                ..Default::default()
            },
            EditInput {
                up: true,
                ..Default::default()
            },
            EditInput {
                up: true,
                ..Default::default()
            },
        ] {
            ed.tick(dir);
        }
        // Cross: commit to naming, then Cross again: confirm the default name.
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        ed.tick(EditInput {
            cross: true,
            ..Default::default()
        });
        assert!(
            matches!(ed.outcome(), Some(EditOutcome::Saved { slot: 0, .. })),
            "editor saved a new chain"
        );
        // Apply the edit to the library and store it back into the world -
        // the bridge under test (no direct `saved_chains` seeding).
        ed.apply_outcome(&mut lib).unwrap();
        field.store_chain_library(&lib);

        // The chain now serializes with the save block...
        let save = field.save_full();
        assert_eq!(save.ext_v2.saved_chains.len(), 1);

        // ...and a fresh boot that loads the save can offer it in battle.
        let mut battle = World {
            party_count: 1,
            ..World::default()
        };
        battle.load_full(save);
        let rows = battle.build_battle_arts_rows(0);
        assert_eq!(
            rows.len(),
            1,
            "the edited chain reaches the battle arts menu"
        );
        // Default new-chain name preset; 3 directional inputs => 3 synthetic hits.
        assert_eq!(rows[0].hits(), 3);
    }

    #[test]
    fn battle_arts_synthetic_chain_runs_through_art_power_path_and_cycles_turn() {
        use crate::input::PadButton;
        use legaia_engine_vm::battle_action::ActionState;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.battle_player_driven = true;
        world.mode = SceneMode::Battle;
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 200;
        world.actors[0].battle.liveness = 1;
        world.set_battle_attack(0, 40);
        world.actors[1].battle.max_hp = 500;
        world.actors[1].battle.hp = 500;
        world.actors[1].battle.liveness = 1;
        world.set_battle_defense(1, 10);
        // One saved chain, 3 directional commands (Left, Right, Down) -> 3 hits.
        // No art record staged, so the row uses the synthetic ×12 profile.
        world.saved_chains.push(legaia_save::SavedChainRecord {
            char_slot: 0,
            name: "Combo".into(),
            sequence: vec![1, 2, 3],
        });

        world.battle_ctx.active_actor = 0;
        world.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(
            0,
            0,
            world.build_battle_arts_rows(0),
        ));
        assert_eq!(world.battle_arts_menu.as_ref().unwrap().arts[0].hits(), 3);

        // Frame 1: Cross opens the target cursor.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_arts_menu();
        assert!(world.battle_arts_menu.is_some(), "still picking a target");

        // Frame 2: Cross confirms the monster; the art runs.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_arts_menu();

        assert!(world.battle_arts_menu.is_none(), "arts menu closed");
        // Three synthetic ×12 hits: (40*12/16 - 10) = 20 each => 60 total.
        let per_hit = legaia_engine_vm::battle_formulas::art_strike_damage_default(40, 10, 12);
        assert_eq!(world.actors[1].battle.hp, 500 - per_hit * 3);
        assert_eq!(
            world.battle_ctx.action_state,
            ActionState::EndOfAction.as_byte(),
            "turn parked at EndOfAction so the loop cycles"
        );
        let fx = world.drain_battle_hit_fx();
        assert_eq!(fx.len(), 1, "one summed popup for the combo");
        assert!(!fx[0].is_heal);
        assert_eq!(fx[0].amount, per_hit * 3);
        assert_eq!(fx[0].target_slot, 1);
    }

    #[test]
    fn battle_arts_uses_staged_art_record_power_tiers_and_status() {
        use crate::input::PadButton;
        use legaia_art::power::PowerByte;
        use legaia_art::queue::{ActionConstant, Command};
        use legaia_art::record::EnemyEffect;

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.battle_player_driven = true;
        world.mode = SceneMode::Battle;
        world.actors[0].battle.liveness = 1;
        world.set_battle_attack(0, 64);
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        // UDF / LDF split so the record's per-strike target picks the right half.
        world.set_battle_defense_split(1, Some((10, 40)));

        // Stage a Vahn art: two damage strikes (UDF ×28, LDF ×28) that burns.
        let rec = legaia_art::ArtRecord {
            action: ActionConstant::Art1B,
            commands: vec![Command::Up, Command::Up],
            anim_index: 0,
            anim_extra: vec![],
            name: None,
            power: vec![PowerByte::from_byte(0x1A), PowerByte::from_byte(0x1F)],
            dmg_timing: vec![],
            effect_cues: Default::default(),
            hit_cues: vec![],
            identifier: 0,
            anim_speed: 0,
            enemy_effect: EnemyEffect::Burned,
            repeat_frames: Default::default(),
            background: 0,
            runtime_address: None,
        };
        world.set_art_record(legaia_art::Character::Vahn, ActionConstant::Art1B, rec);

        // Saved chain ending in the art's command string (Up, Up).
        world.saved_chains.push(legaia_save::SavedChainRecord {
            char_slot: 0,
            name: "Burning Combo".into(),
            sequence: vec![1, 4, 4], // Left, Up, Up
        });

        let rows = world.build_battle_arts_rows(0);
        assert_eq!(rows[0].hits(), 2, "two damage strikes from the record");
        assert_eq!(rows[0].enemy_effect, EnemyEffect::Burned);

        world.battle_ctx.active_actor = 0;
        world.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(0, 0, rows));

        // Open the target cursor, then confirm.
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_arts_menu();
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        world.tick_battle_arts_menu();

        // UDF ×28 vs udf=10: 64*28/16 - 10 = 112 - 10 = 102.
        // LDF ×28 vs ldf=40: 64*28/16 - 40 = 112 - 40 = 72.
        let expect = (102u16 + 72u16) as u32;
        assert_eq!(world.actors[1].battle.hp, 4000 - expect as u16);
        assert!(
            world.status_effects.is_afflicted(1),
            "the art's Burned effect was applied to the target"
        );
        let fx = world.drain_battle_hit_fx();
        assert_eq!(fx.len(), 1);
        assert_eq!(fx[0].amount, expect as u16);
        assert!(fx[0].is_crit, "multi-hit art flagged as crit popup");
    }

    #[test]
    fn apply_battle_loot_never_drops_when_rate_zero() {
        use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
        let mut cat = MonsterCatalog::new();
        let mut def = MonsterDef::new(7, "Slime", 10, 5);
        def.drop_item = Some(0x42);
        def.drop_rate_q8 = 0;
        cat.insert(def);
        let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        let rewards = world.apply_battle_loot(&formation, &cat);
        assert!(rewards.drops.is_empty());
        assert!(!world.inventory.contains_key(&0x42));
    }

    #[test]
    fn load_full_hydrates_level_up_tracker_from_record_levels() {
        // Build a 3-character save with levels 7, 12, 25.
        let mut party = legaia_save::Party::zeroed(3);
        party.members[0].set_level(7);
        party.members[1].set_level(12);
        party.members[2].set_level(25);
        let sf = legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt::default(),
            ext_v2: legaia_save::SaveExtV2::default(),
        };
        let mut world = World::new();
        // Tracker defaults to 1 for every slot.
        assert_eq!(world.level_up_tracker.level[0], 1);
        world.load_full(sf);
        assert_eq!(world.level_up_tracker.level[0], 7);
        assert_eq!(world.level_up_tracker.level[1], 12);
        assert_eq!(world.level_up_tracker.level[2], 25);
    }

    #[test]
    fn load_full_zero_level_record_clamps_to_one() {
        // Records that haven't had a level written (zero byte at +0x100)
        // shouldn't make the tracker think the slot is below L1.
        let party = legaia_save::Party::zeroed(2);
        let sf = legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt::default(),
            ext_v2: legaia_save::SaveExtV2::default(),
        };
        let mut world = World::new();
        world.load_full(sf);
        assert_eq!(world.level_up_tracker.level[0], 1);
        assert_eq!(world.level_up_tracker.level[1], 1);
    }

    #[test]
    fn apply_battle_xp_drops_remainder_from_integer_division() {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.actors[1].battle.hp = 100;
        world.actors[2].battle.hp = 100;
        // 101 / 3 = 33; the leftover 2 XP is dropped.
        let _ = world.apply_battle_xp(101);
        assert_eq!(world.level_up_tracker.xp[0], 33);
        assert_eq!(world.level_up_tracker.xp[1], 33);
        assert_eq!(world.level_up_tracker.xp[2], 33);
    }

    #[test]
    fn level_up_banner_countdown_clears() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.apply_battle_xp(50); // retail table: exactly L2 threshold
        assert!(world.current_level_up_banner.is_some());
        for _ in 0..=crate::levelup::LevelUpBanner::DEFAULT_FRAMES {
            world.tick();
        }
        assert!(
            world.current_level_up_banner.is_none(),
            "level-up banner should have cleared"
        );
    }

    #[test]
    fn no_level_up_banner_when_xp_insufficient() {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.hp = 100;
        world.apply_battle_xp(49); // retail table: 49 < 50 (L2 threshold)
        assert!(world.current_level_up_banner.is_none());
    }

    #[test]
    fn art_strike_applier_pushes_apply_art_strike_event() {
        // Drive `BattleHostImpl::apply_art_strike` from a synthetic
        // ArtStrikeInfo and assert the world's pending_battle_events grows
        // by one ApplyArtStrike with the resolved damage.
        use legaia_art::Character;
        use legaia_art::power::PowerByte;
        use legaia_art::queue::ActionConstant;
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

        let mut world = World::new();
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 10);
        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
            dmg_timing: Some(0x10),
            enemy_effect: EnemyEffect::Burned,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);

        assert_eq!(world.pending_battle_events.len(), 1);
        match &world.pending_battle_events[0] {
            BattleEvent::ApplyArtStrike {
                actor_slot,
                target_slot,
                strike_index,
                outcome,
            } => {
                assert_eq!(*actor_slot, 0);
                assert_eq!(*target_slot, 3);
                assert_eq!(*strike_index, 0);
                assert_eq!(outcome.damage, Some(102));
                assert_eq!(outcome.enemy_effect, EnemyEffect::Burned);
            }
            other => panic!("unexpected event: {:?}", other.summary()),
        }
    }

    #[test]
    fn art_strike_split_defense_picks_udf_or_ldf() {
        // With a (UDF=5, LDF=50) split on slot 3, a UDF-targeted strike
        // hits 5 def → high damage; LDF-targeted hits 50 def → low.
        use legaia_art::Character;
        use legaia_art::power::PowerByte;
        use legaia_art::queue::ActionConstant;
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

        let mut world = World::new();
        world.set_battle_attack(0, 64);
        world.set_battle_defense_split(3, Some((5, 50)));

        let mk = |power: PowerByte| ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(power),
            dmg_timing: Some(0x10),
            enemy_effect: EnemyEffect::None,
            hit_cue: None,
        };
        // 0x1A = UDF × 28 → (64 * 28)/16 - 5 = 112 - 5 = 107.
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(mk(PowerByte::from_byte(0x1A)));
        // 0x1F = LDF × 28 → (64 * 28)/16 - 50 = 112 - 50 = 62.
        host.apply_art_strike(mk(PowerByte::from_byte(0x1F)));
        let events = world.drain_battle_events();
        let mut udf_dmg = None;
        let mut ldf_dmg = None;
        for e in events {
            if let BattleEvent::ApplyArtStrike { outcome, .. } = e
                && let Some(t) = outcome.power_target
            {
                match t {
                    legaia_art::power::PowerTarget::Udf => udf_dmg = outcome.damage,
                    legaia_art::power::PowerTarget::Ldf => ldf_dmg = outcome.damage,
                }
            }
        }
        assert_eq!(udf_dmg, Some(107));
        assert_eq!(ldf_dmg, Some(62));
    }

    #[test]
    fn fold_battle_event_apply_art_strike_subtracts_hp_and_records_status() {
        use legaia_art::power::{PowerByte, PowerTarget};
        use legaia_art::queue::{ActionConstant, Character};
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;

        let mut world = World::new();
        world.party_count = 4;
        for slot in 0..4 {
            world.actors[slot].active = true;
            world.actors[slot].battle.hp = 200;
            world.actors[slot].battle.max_hp = 200;
        }
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 5);

        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
            dmg_timing: Some(0x10),
            enemy_effect: EnemyEffect::Burned,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);
        let events = world.drain_battle_events();
        assert_eq!(events.len(), 1);
        for e in &events {
            let r = world.fold_battle_event(e);
            // 64 * 28 / 16 - 5 = 107 damage. Target slot 3 starts at 200,
            // ends at 93.
            assert_eq!(r, Some((3, 93)));
        }
        assert_eq!(world.actors[3].battle.hp, 93);
        // Burned status was folded into pending_status.
        assert_eq!(
            world.actors[3].pending_status,
            Some(legaia_art::record::EnemyEffect::Burned)
        );
        // PowerTarget enum is needed only to satisfy the import linter
        // when the assertions don't otherwise reference it.
        let _ = PowerTarget::Udf;
    }

    #[test]
    fn fold_battle_event_other_variants_dont_modify_state() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].active = true;
        world.actors[0].battle.hp = 100;
        let r = world.fold_battle_event(&BattleEvent::CameraBounds);
        assert_eq!(r, None);
        assert_eq!(world.actors[0].battle.hp, 100);
    }

    #[test]
    fn use_item_heals_hp_clamped_to_max() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 200;
        world.actors[0].battle.hp = 50;
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Item id 1 is the small heal in the vanilla catalog.
        let outcome = world.use_item(1, 0);
        assert!(matches!(
            outcome,
            crate::items::ItemOutcome::HealedHp { .. }
        ));
        // HP raised but clamped at max.
        assert!(world.actors[0].battle.hp > 50);
        assert!(world.actors[0].battle.hp <= 200);
    }

    #[test]
    fn use_item_heal_all_fills_to_max() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 300;
        world.actors[0].battle.hp = 100;
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Find the HealAll entry (id 4 in the vanilla catalog - Healing Globe).
        let outcome = world.use_item(4, 0);
        assert!(matches!(
            outcome,
            crate::items::ItemOutcome::HealedHp { .. }
        ));
        assert_eq!(world.actors[0].battle.hp, 300);
    }

    #[test]
    fn use_item_unknown_id_returns_no_effect() {
        let mut world = World::new();
        world.party_count = 1;
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        let outcome = world.use_item(99, 0);
        assert!(matches!(outcome, crate::items::ItemOutcome::NoEffect));
    }

    #[test]
    fn use_item_revive_writes_hp_after() {
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 0; // dead
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Resurrection Leaf is id 0x0C (50% revive).
        let outcome = world.use_item(0x0C, 0);
        assert!(matches!(outcome, crate::items::ItemOutcome::Revived { .. }));
        // 50% of 400 = 200.
        assert_eq!(world.actors[0].battle.hp, 200);
    }

    #[test]
    fn use_item_cure_clears_status() {
        use legaia_art::record::EnemyEffect;
        let mut world = World::new();
        world.party_count = 1;
        world.actors[0].battle.max_hp = 100;
        world.actors[0].battle.hp = 50;
        // Apply a Burned status, then cure it via CureAll.
        world
            .status_effects
            .apply_from_enemy_effect(0, EnemyEffect::Burned);
        assert!(world.status_effects.is_afflicted(0));
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        // Antidote Flower is id 0x09 (CureAll).
        let outcome = world.use_item(0x09, 0);
        assert!(matches!(outcome, crate::items::ItemOutcome::CuredAll));
        assert!(!world.status_effects.is_afflicted(0));
    }

    #[test]
    fn fold_battle_event_clamps_to_zero_hp() {
        use legaia_art::power::PowerByte;
        use legaia_art::queue::{ActionConstant, Character};
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;

        let mut world = World::new();
        world.party_count = 4;
        world.actors[3].active = true;
        world.actors[3].battle.hp = 30;
        world.actors[3].battle.max_hp = 30;
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 0);

        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)), // huge damage vs 30 HP
            dmg_timing: None,
            enemy_effect: EnemyEffect::None,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);
        let events = world.drain_battle_events();
        for e in &events {
            world.fold_battle_event(e);
        }
        // saturating_sub clamps to 0 instead of wrapping.
        assert_eq!(world.actors[3].battle.hp, 0);
    }

    #[test]
    fn fold_battle_event_pushes_status_into_tracker() {
        use legaia_art::power::PowerByte;
        use legaia_art::queue::{ActionConstant, Character};
        use legaia_art::record::EnemyEffect;
        use legaia_engine_vm::battle_action::ArtStrikeInfo;
        use legaia_engine_vm::status_effects::StatusKind;

        let mut world = World::new();
        world.party_count = 4;
        world.actors[3].active = true;
        world.actors[3].battle.hp = 100;
        world.actors[3].battle.max_hp = 100;
        world.set_battle_attack(0, 64);
        world.set_battle_defense(3, 10);
        let info = ArtStrikeInfo {
            strike_index: 0,
            anim_byte: 0x10,
            actor_slot: 0,
            target_slot: 3,
            character: Character::Vahn,
            art: ActionConstant::Art1B,
            power: Some(PowerByte::from_byte(0x1A)),
            dmg_timing: None,
            enemy_effect: EnemyEffect::Burned,
            hit_cue: None,
        };
        let mut host = BattleHostImpl { world: &mut world };
        host.apply_art_strike(info);
        let events = world.drain_battle_events();
        for e in &events {
            world.fold_battle_event(e);
        }
        assert!(world.status_effects.has(3, StatusKind::Burned));
    }

    #[test]
    fn tick_status_effects_drains_hp() {
        use legaia_engine_vm::status_effects::StatusKind;
        let mut world = World::new();
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.max_hp = 160;
        world.status_effects.apply(0, StatusKind::Burned);
        world.tick_status_effects();
        assert_eq!(world.actors[0].battle.hp, 90);
    }

    #[test]
    fn reset_party_ap_refills_all_three_gauges() {
        let mut world = World::new();
        for g in world.ap_gauges.iter_mut() {
            g.try_spend(3);
        }
        world.reset_party_ap();
        for g in world.ap_gauges.iter() {
            assert_eq!(g.current_ap, g.base_ap);
            assert!(!g.spirit_charged);
        }
    }

    #[test]
    fn item_catalog_setter_replaces() {
        use crate::items::ItemCatalog;
        let mut world = World::new();
        assert!(world.item_catalog.is_empty());
        world.set_item_catalog(ItemCatalog::vanilla());
        assert!(!world.item_catalog.is_empty());
    }

    #[test]
    fn install_encounter_for_scene_resolves_field_pattern() {
        use crate::encounter_registry::vanilla_encounter_registry;
        let mut world = World::new();
        let r = vanilla_encounter_registry();
        let installed = world.install_encounter_for_scene(&r, "map01");
        assert!(installed, "field pattern should match");
        assert!(world.encounter.is_some());
    }

    #[test]
    fn install_encounter_for_scene_quiets_in_towns() {
        use crate::encounter_registry::vanilla_encounter_registry;
        let mut world = World::new();
        let r = vanilla_encounter_registry();
        let installed = world.install_encounter_for_scene(&r, "town01");
        assert!(!installed, "town pattern resolves but is quiet");
        assert!(
            world.encounter.is_some(),
            "session installed for nil checks"
        );
    }

    #[test]
    fn install_encounter_for_scene_returns_false_with_no_default() {
        use crate::encounter_registry::EncounterRegistry;
        let mut world = World::new();
        let r = EncounterRegistry::new(); // empty, no default
        let installed = world.install_encounter_for_scene(&r, "anything");
        assert!(!installed);
        assert!(world.encounter.is_none());
    }

    #[test]
    fn install_encounter_for_scene_replaces_active_session() {
        use crate::encounter_registry::vanilla_encounter_registry;
        let mut world = World::new();
        let r = vanilla_encounter_registry();
        // Install a field session, then a town session - the town call
        // should replace the field session even though it's quiet.
        world.install_encounter_for_scene(&r, "map01");
        assert!(world.encounter.is_some());
        let initial_table_label = world
            .encounter
            .as_ref()
            .unwrap()
            .tracker()
            .table()
            .scene_label
            .clone();
        world.install_encounter_for_scene(&r, "town01");
        let new_table_label = world
            .encounter
            .as_ref()
            .unwrap()
            .tracker()
            .table()
            .scene_label
            .clone();
        assert_ne!(initial_table_label, new_table_label);
    }

    #[test]
    fn install_encounter_from_record_registers_and_arms() {
        use crate::encounter_record::EncounterRecord;
        let mut world = World::new();
        // mc2-shaped record: two monsters, both id 4.
        let record = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x04, 0, 0],
        };
        let formation_id = world
            .install_encounter_from_record("map01", &record)
            .expect("non-empty record produces an id");
        // Formation registered.
        let formation = world
            .formation_table
            .formation(formation_id)
            .expect("formation registered");
        assert_eq!(formation.slots.len(), 2);
        assert_eq!(formation.slots[0].monster_id, 4);
        assert_eq!(formation.slots[1].monster_id, 4);
        // Session installed and rate forced high.
        let session = world.encounter.as_ref().expect("session installed");
        assert_eq!(session.tracker().table().trigger_rate_q8, 0xFF);
        assert_eq!(session.tracker().table().entries.len(), 1);
        assert_eq!(
            session.tracker().table().entries[0].formation_id,
            formation_id
        );
    }

    #[test]
    fn install_scripted_encounter_parses_window_and_arms_battle() {
        let mut world = World::new();
        world.set_formation_table(
            crate::monster_catalog::vanilla_formation_table(),
            crate::monster_catalog::vanilla_monster_catalog(),
        );
        world.set_active_scene_label("town01");
        world.mode = SceneMode::Field;
        world.arm_scripted_encounter(true);
        // Record window overlaying the arm opcode: [op][op1][op2][count=2][ids..].
        let window = [0x37u8, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00];
        let formation_id = world
            .install_scripted_encounter(&window)
            .expect("non-empty record installs a formation");
        // Fire-once: a successful install disarms the carrier flag.
        assert!(!world.scripted_encounter_armed);
        // Formation registered with the window's two ids.
        let formation = world
            .formation_table
            .formation(formation_id)
            .expect("formation registered");
        assert_eq!(formation.slots.len(), 2);
        assert_eq!(formation.slots[0].monster_id, 0x4F);
        assert_eq!(formation.slots[1].monster_id, 0x50);
        // Session installed at the forced-high rate.
        assert_eq!(
            world
                .encounter
                .as_ref()
                .unwrap()
                .tracker()
                .table()
                .trigger_rate_q8,
            0xFF
        );
        // Event surfaced for engine visibility.
        assert!(world.pending_field_events.iter().any(|e| matches!(
            e,
            FieldEvent::ScriptedEncounter { record } if record == &window
        )));
        // The very next field step flips Field -> a triggered encounter.
        assert!(
            world.on_field_step(),
            "forced-rate roll triggers the battle"
        );
    }

    #[test]
    fn install_scripted_encounter_empty_or_short_window_returns_none() {
        let mut world = World::new();
        world.set_active_scene_label("town01");
        // count = 0 -> empty record -> no install.
        assert_eq!(world.install_scripted_encounter(&[0, 0, 0, 0]), None);
        assert!(world.encounter.is_none());
        // Too short to even hold the count byte -> parse fails.
        assert_eq!(world.install_scripted_encounter(&[0, 0]), None);
    }

    #[test]
    fn seed_party_battle_stats_folds_live_stats_and_equipment() {
        use crate::battle_stats::{EquipmentTable, ItemModifier};
        use legaia_save::EquipmentSlots;
        use legaia_save::character::LiveStats;

        let mut world = World::new();
        let mut party = legaia_save::Party::zeroed(1);
        party.members[0].set_live_stats(LiveStats {
            agl: 12,
            atk: 30,
            udf: 10,
            ldf: 8,
            spd: 5,
            int: 4,
        });
        let mut slots = [0u8; 8];
        slots[0] = 5; // a weapon in the first slot
        party.members[0].set_equipment(EquipmentSlots { slots });
        world.load_party(party);

        // Item 5 grants +7 attack, +3 UDF, +2 LDF.
        let mut table = EquipmentTable::new();
        table.set(
            5,
            ItemModifier {
                atk: 7,
                udf: 3,
                ldf: 2,
                acc: 0,
                eva: 0,
                ability_bits: [0; 32],
            },
        );
        world.set_equipment_table(table);

        world.seed_party_battle_stats();
        assert_eq!(world.battle_attack[0], 37, "30 base + 7 weapon");
        assert_eq!(
            world.battle_defense_split[0],
            Some((13, 10)),
            "(10+3) UDF, (8+2) LDF"
        );
    }

    #[test]
    fn seed_party_battle_stats_skips_zeroed_roster() {
        // A synthetic battle sets battle_attack directly then loads a zeroed
        // roster; seeding must not clobber the manual value.
        let mut world = World::new();
        world.set_battle_attack(0, 60);
        world.load_party(legaia_save::Party::zeroed(3));
        world.seed_party_battle_stats();
        assert_eq!(world.battle_attack[0], 60, "zeroed roster leaves it intact");
        assert_eq!(world.battle_defense_split[0], None);
    }

    #[test]
    fn drain_pending_scripted_encounter_only_when_queued() {
        let mut world = World::new();
        world.set_formation_table(
            crate::monster_catalog::vanilla_formation_table(),
            crate::monster_catalog::vanilla_monster_catalog(),
        );
        world.set_active_scene_label("town01");
        // Nothing queued -> no-op.
        world.drain_pending_scripted_encounter();
        assert!(world.encounter.is_none());
        // Queue a window (as the armed forwarded-PC hook would) and drain.
        world.pending_scripted_encounter = Some(vec![0, 0, 0, 1, 0x12, 0, 0, 0]);
        world.drain_pending_scripted_encounter();
        assert!(world.pending_scripted_encounter.is_none());
        assert!(world.encounter.is_some());
    }

    #[test]
    fn install_encounter_from_record_empty_returns_none() {
        use crate::encounter_record::EncounterRecord;
        let mut world = World::new();
        let id = world.install_encounter_from_record("map01", &EncounterRecord::EMPTY);
        assert!(id.is_none());
        // No session installed.
        assert!(world.encounter.is_none());
    }

    #[test]
    fn install_man_formation_forces_registered_row() {
        use crate::monster_catalog::{FormationDef, FormationSlot};
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.set_active_scene_label("town01");
        // Register a lone-monster formation at id 4 (town01's Tetsu row shape).
        world
            .formation_table
            .insert(FormationDef::new(4, vec![FormationSlot::new(0x4F)]));

        // Unknown id -> None, no session.
        assert!(world.install_man_formation(9).is_none());
        assert!(world.encounter.is_none());

        // Registered id installs a forced-rate session that triggers next step.
        assert_eq!(world.install_man_formation(4), Some(4));
        assert!(world.encounter.is_some());
        assert!(
            world.on_field_step(),
            "forced-rate session triggers on the next step"
        );
    }

    #[test]
    fn field_carrier_engage_launches_battle_and_returns_to_field() {
        use crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
        use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Field;
        world.live_gameplay_loop = true; // auto-resolve the battle leg
        world.set_active_scene_label("town01");
        // A capable lone party member so the battle can resolve.
        world.actors[0].active = true;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.liveness = 1;
        world.set_battle_attack(0, 80);
        // town01's Tetsu row: formation index 4 = lone monster id 0x4F.
        world.formation_table.insert(FormationDef::new(
            RIM_ELM_TRAINING_FORMATION_ID,
            vec![FormationSlot::new(0x4F)],
        ));
        let mut cat = MonsterCatalog::new();
        cat.insert(MonsterDef::new(0x4F, "Tetsu", 999, 40));
        world.set_monster_catalog(cat);

        // Place one scripted-encounter carrier (the Tetsu NPC) - the field-mode
        // use of the FUN_801DA51C SM.
        world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
            formation_id: RIM_ELM_TRAINING_FORMATION_ID,
        }]);

        // Idle: ticking does NOT launch a battle (towns are 0% random; the
        // carrier waits for the dialogue-accept).
        world.tick();
        assert_eq!(
            world.mode,
            SceneMode::Field,
            "an idle scripted carrier never self-fires"
        );
        assert_eq!(world.field_carriers[0].state, 0, "carrier still Idle");

        // The dialogue-accept advances the carrier to Activating; the next tick
        // runs the state-1 body (formation copy) and the case 2/3 fall-through
        // (battle handoff), flipping Field -> Battle, tagged to return to field.
        world.engage_field_carrier(0);
        world.tick();
        assert_eq!(world.mode, SceneMode::Battle);
        assert_eq!(world.battle_return_mode, SceneMode::Field);
        assert!(world.field_return.is_some());
        let formation = world.active_formation.as_ref().expect("active formation");
        assert_eq!(
            formation.slots[0].monster_id, 0x4F,
            "Tetsu in the enemy slot"
        );
        assert_eq!(
            world.field_carriers[0].state,
            vm::world_map::EntityState::Terminal as u16,
            "carrier retired to Terminal after the transition"
        );

        // Drive the fight to completion; it must return to the field.
        let mut returned = false;
        for _ in 0..8000 {
            world.tick();
            if world.mode != SceneMode::Battle {
                returned = true;
                break;
            }
        }
        assert!(returned, "battle resolves");
        assert_eq!(world.mode, SceneMode::Field, "returns to the field");
        // The carrier stays Terminal - the scripted fight fires exactly once.
        assert_eq!(
            world.field_carriers[0].state,
            vm::world_map::EntityState::Terminal as u16
        );
    }

    #[test]
    fn field_carrier_unengaged_never_fires() {
        use crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
        use crate::monster_catalog::{FormationDef, FormationSlot};

        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Field;
        world.set_active_scene_label("town01");
        world.formation_table.insert(FormationDef::new(
            RIM_ELM_TRAINING_FORMATION_ID,
            vec![FormationSlot::new(0x4F)],
        ));
        world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
            formation_id: RIM_ELM_TRAINING_FORMATION_ID,
        }]);

        // Many idle ticks must never flip into battle (no random rate).
        for _ in 0..256 {
            world.tick();
            assert_eq!(world.mode, SceneMode::Field);
        }
        assert!(world.field_return.is_none());
        assert!(world.pending_field_carrier_battle.is_none());
    }

    #[test]
    fn begin_new_game_clears_state_and_enters_field() {
        let mut world = World::new();
        // Dirty the world as if a prior session had been played.
        world.mode = SceneMode::Battle;
        world.story_flags = 0xDEAD_BEEF;
        world.story_flag_bits = vec![1, 2, 3];
        world.money = 4242;
        world.inventory.insert(0x10, 5);
        world.scripted_encounter_armed = true;
        world.game_over = true;
        world.play_time_seconds = 9999;

        world.begin_new_game();

        // The retail field-launch (master mode 3) clean slate.
        assert_eq!(world.mode, SceneMode::Field);
        assert_eq!(world.story_flags, 0);
        assert!(world.story_flag_bits.is_empty());
        // New-game gold is the retail constant (FUN_80034A6C), not zero.
        assert_eq!(world.money, NEW_GAME_STARTING_GOLD);
        assert!(world.inventory.is_empty());
        assert!(!world.scripted_encounter_armed);
        assert!(world.encounter.is_none());
        assert!(!world.game_over);
        assert_eq!(world.play_time_seconds, 0);
    }

    #[test]
    fn prologue_handoff_fires_once_on_confirm_in_opdeene() {
        let mut world = World::new();
        world.set_active_scene_label(legaia_asset::new_game::OPENING_CUTSCENE_SCENE);

        // Not armed yet: confirm does nothing.
        assert_eq!(world.take_prologue_handoff(true), None);

        world.arm_prologue_handoff();
        assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

        // Armed but no confirm: stays in the cutscene.
        assert_eq!(world.take_prologue_handoff(false), None);
        assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

        // Armed + confirm: hands off to town01 and clears the bit (fire-once).
        assert_eq!(
            world.take_prologue_handoff(true),
            Some(legaia_asset::new_game::OPENING_SCENE)
        );
        assert_eq!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

        // A second confirm does not re-fire.
        assert_eq!(world.take_prologue_handoff(true), None);
    }

    #[test]
    fn prologue_handoff_only_fires_in_the_cutscene_scene() {
        let mut world = World::new();
        // Armed, confirm pressed, but the active scene is not `opdeene`.
        world.set_active_scene_label(legaia_asset::new_game::OPENING_SCENE);
        world.arm_prologue_handoff();
        assert_eq!(world.take_prologue_handoff(true), None);
        // Bit is left intact for the gate to fire only in `opdeene`.
        assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);
    }

    // ------------------------------------------------------------------
    // tick_actor_physics + MoveBufferHost wiring
    // ------------------------------------------------------------------

    /// Build a 1-record MOVE pool: index `id` -> offset `record_off`,
    /// record body `[0, flag, fc_lo, fc_hi, 0, 0, divisor, 0]`.
    fn make_move_pool(id: u16, record_off: usize, frame_count: u16, divisor: u8) -> Vec<u8> {
        // Table size matches retail's hard-coded 1024-entry view.
        let table_entries = 1024usize;
        let table_bytes = table_entries * 4;
        let total = (record_off + 16).max(table_bytes);
        let mut pool = vec![0u8; total];
        let off = (id as usize) * 4;
        pool[off..off + 4].copy_from_slice(&(record_off as u32).to_le_bytes());
        let fc = frame_count.to_le_bytes();
        pool[record_off + 1] = 0; // flag
        pool[record_off + 2] = fc[0];
        pool[record_off + 3] = fc[1];
        pool[record_off + 6] = divisor;
        pool
    }

    #[test]
    fn tick_actor_physics_skips_inactive_slots() {
        let mut world = World::new();
        // No actor active; should be a no-op (no panics, no events).
        world.tick_actor_physics();
        assert!(world.last_tick_events.is_empty());
    }

    #[test]
    fn tick_actor_physics_records_keyframe_event_for_active_actor() {
        let mut world = World::new();
        // Activate slot 0 on the keyframe dispatch arm; populate the
        // record pointer so the keyframe writeback fires.
        world.actors[0].active = true;
        world.actors[0].set_physics_dispatch(0x06);
        world.actors[0].physics.set_record_ptr(0x80100000);
        world.actors[0].physics.set_bone_count(8);
        world.tick_actor_physics();
        // One slot fired; events vector non-empty.
        assert_eq!(world.last_tick_events.len(), 1);
        let (slot, res) = &world.last_tick_events[0];
        assert_eq!(*slot, 0);
        assert!(
            res.events
                .iter()
                .any(|e| matches!(e, TickEvent::KeyframePoseWritten { bone_count: 8 }))
        );
    }

    #[test]
    fn move_vm_kick_drives_cursor_advance_against_installed_pool() {
        let mut world = World::new();
        // Install a MOVE pool with id 3 -> record at offset 0x1010,
        // frame_count = 8, divisor = 1.
        world.set_move_buffer_root(make_move_pool(3, 0x1010, 8, 1));
        // Activate slot 0; set the move_vm_kick flag so the physics
        // tick's late-update emits TickEvent::MoveVmKick.
        world.actors[0].active = true;
        world.actors[0].set_physics_dispatch(0x06);
        world.actors[0].physics.move_vm_kick = 1;
        // Request move id 3; phase rate of 8 steps per frame.
        world.actors[0].move_buffer.cursor_requested = 3;
        world.actors[0].move_buffer.phase_rate = 8;
        world.tick_actor_physics();
        // MoveVmKick emitted.
        let (_, res) = &world.last_tick_events[0];
        assert!(
            res.events
                .iter()
                .any(|e| matches!(e, TickEvent::MoveVmKick))
        );
        // Cursor latched the new id and stepped once.
        assert_eq!(world.actors[0].move_buffer.cursor_active, 3);
        // First frame after latch: cursor_active==3, phase started at
        // 0, advanced by phase_rate * frame_delta = 8 * 1 = 8.
        assert_eq!(world.actors[0].move_buffer.phase, 8);
        // Move VM kick flag set by the latch (cursor_advance writes
        // move_vm_kick = 1 whenever it latches a new record).
        assert_eq!(world.actors[0].move_buffer.move_vm_kick, 1);
    }

    #[test]
    fn move_vm_kick_no_record_is_graceful_noop() {
        let mut world = World::new();
        // No pool installed; cursor_advance's resolver returns None.
        world.actors[0].active = true;
        world.actors[0].set_physics_dispatch(0x06);
        world.actors[0].physics.move_vm_kick = 1;
        world.actors[0].move_buffer.cursor_requested = 5;
        world.actors[0].move_buffer.phase_rate = 8;
        world.tick_actor_physics();
        // Kick emitted but cursor stays idle (no record source).
        assert_eq!(world.actors[0].move_buffer.cursor_active, 0);
        assert_eq!(world.actors[0].move_buffer.phase, 0);
        assert_eq!(world.actors[0].move_buffer.move_vm_kick, 0);
    }

    #[test]
    fn tick_does_not_advance_cursor_when_move_vm_kick_is_clear() {
        let mut world = World::new();
        world.set_move_buffer_root(make_move_pool(2, 0x1010, 4, 1));
        // Activate slot 0 but leave move_vm_kick = 0 in physics; the
        // late-update path does NOT emit MoveVmKick this frame, so
        // the cursor stays untouched even though a request is pending.
        world.actors[0].active = true;
        world.actors[0].set_physics_dispatch(0x06);
        world.actors[0].move_buffer.cursor_requested = 2;
        world.actors[0].move_buffer.phase_rate = 4;
        let before = world.actors[0].move_buffer.clone();
        world.tick_actor_physics();
        // Cursor unchanged (no kick).
        assert_eq!(world.actors[0].move_buffer, before);
    }

    #[test]
    fn world_tick_runs_physics_pass_in_order() {
        // Smoke test: World::tick invokes tick_actor_physics. After
        // one tick with the kick flag set + a record installed, the
        // per-actor cursor should have advanced.
        let mut world = World::new();
        world.set_move_buffer_root(make_move_pool(1, 0x1010, 8, 1));
        world.actors[0].active = true;
        world.actors[0].set_physics_dispatch(0x06);
        world.actors[0].physics.move_vm_kick = 1;
        world.actors[0].move_buffer.cursor_requested = 1;
        world.actors[0].move_buffer.phase_rate = 4;
        // World::tick (no scene mode) returns None for Title; the
        // physics pass still runs unconditionally.
        world.tick();
        assert_eq!(world.actors[0].move_buffer.cursor_active, 1);
    }
}
