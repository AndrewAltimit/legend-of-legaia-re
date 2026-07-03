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
//!
//! PORT: FUN_800467E8 (`world_map_camera_relative_bits` - held-pad camera-yaw
//!       remap; the engine reads the world-map camera azimuth directly from
//!       [`WorldMapController`] rather than the retail `gp+0x2D8` quadrant.)

use std::sync::Arc;

use crate::battle_events::{BattleEvent, BattleHitFx, BattleSfxCue};
use crate::field_events::FieldEvent;
use crate::input;
use crate::levelup::{LevelUpBanner, LevelUpResult, LevelUpTracker};
use crate::man_field_scripts::WalkTouchEvent;
use crate::move_buffer_host;
use crate::tactical_arts::{ArtLearnedBanner, TacticalArtsTracker};
use crate::world_map::WorldMapController;
pub use legaia_anm::{AnimPlayer, PoseFrame};
use legaia_asset::monster_archive::MonsterAnimation;
use legaia_engine_vm as vm;
use legaia_save;
use vm::Position as ActorVmPosition;
use vm::actor_tick::{ActorPhysics, ListenerState, TickEvent, TickResult, TickScalars};
use vm::battle_action::{BattleActionCtx, BattleActor, BattleEndCause, StepOutcome};
use vm::effect_vm::Pool;
use vm::field::{CameraParam, FieldCtx, StepResult as FieldStepResult};
use vm::move_buffer::{MoveBufferState, cursor_advance};
use vm::move_vm::ActorState as MoveActorState;

use vm_hosts::{
    ActorVmHostImpl, BattleHostImpl, EffectHostImpl, FieldCarrierHostImpl, FieldHostImpl,
    MoveVmHostImpl, WorldMapEntityHostImpl,
};

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

/// Retail static-wall leading-edge probe table `DAT_801f2214` (field overlay
/// 0897, file offset `0x239FC`), decoded from the disc. Indexed by travel
/// direction (`0` = Z-, `1` = X-, `2` = Z+, `3` = X+; the `FUN_801cfe4c`
/// `param_3` convention), three `(dx, dz)` pairs each, applied to the
/// player's CURRENT position as `(x + dx, z - dz)` - note the Z delta is
/// SUBTRACTED, exactly as the retail probe reads it. A direction is blocked
/// when ANY of its three probes lands on a wall sub-cell.
///
/// The lookahead is asymmetric on purpose: the `+2`-biased Z mapping and the
/// `ceil-1` X mapping ([`World::field_tile_is_wall`]) make 48 the crossing
/// distance in the positive directions and 47 in the negative ones, so each
/// edge sits one full tile ahead in cell space. (The on-disc rows are
/// 16 bytes; the trailing fourth pair - a half-distance centre point - is
/// never read by the wall probe and is omitted here.)
const FIELD_WALL_PROBES: [[(i16, i16); 3]; 4] = [
    [(-16, 48), (0, 48), (16, 48)], // dir 0: Z- (edge at z-48, ±16 in X)
    [(-47, -16), (-47, 0), (-47, 16)], // dir 1: X- (edge at x-47, ±16 in Z)
    [(-16, -47), (0, -47), (16, -47)], // dir 2: Z+ (edge at z+47, ±16 in X)
    [(48, -16), (48, 0), (48, 16)], // dir 3: X+ (edge at x+48, ±16 in Z)
];

/// Retail actor-collision probe table `DAT_801f21b4` (field overlay 0897,
/// file offset `0x2399C`, the sibling 0x60 bytes before
/// [`FIELD_WALL_PROBES`]'s `DAT_801f2214`), decoded from the disc. Same
/// shape and `(x + dx, z - dz)` application as the wall table, but the
/// probes feed the per-actor box test `FUN_801cfc40` instead of the
/// walkability grid: a wider sweep (64/63 ahead, ±32 lateral vs the wall
/// probes' 48/47, ±16) because actors block with a body box rather than a
/// sub-cell edge. The trailing fourth on-disc pair (a half-distance centre
/// point, `(0,32)`-class) is never read by `FUN_801cfe4c` and is omitted.
const FIELD_ACTOR_PROBES: [[(i16, i16); 3]; 4] = [
    [(-32, 64), (0, 64), (32, 64)], // dir 0: Z- (probes at z-64, ±32 in X)
    [(-63, -32), (-63, 0), (-63, 32)], // dir 1: X- (probes at x-63, ±32 in Z)
    [(-32, -63), (0, -63), (32, -63)], // dir 2: Z+ (probes at z+63, ±32 in X)
    [(64, -32), (64, 0), (64, 32)], // dir 3: X+ (probes at x+64, ±32 in Z)
];

/// Half-extent of the box a field NPC blocks with, around its live position:
/// retail `FUN_801cfc40` classes village NPCs as **moving actors** (`flags &
/// 0x1020000 != 0`) and tests `|probe - pos| < 0x40 + (ex - 0x18)` per axis
/// with the locomotion's caller extents `ex = 0`, i.e. ±40 units.
/// Capture-pinned by `rimelm_npc_press_tetsu`: the sparring partner's flags
/// (`0x08020884`) carry the `0x20000` class bit, putting him on this arm
/// (result bit `1`), with the mutual `+0x98` collision link live in-frame.
/// (STATIC entities - props, `flags & 0x1020000 == 0` - use a wider
/// `0x40 + 0x10` = 80-unit box around their MAN record anchor instead; see
/// [`FIELD_PROP_BOX_HALF`] / [`World::field_prop_colliders`].)
const FIELD_NPC_BOX_HALF: i32 = 0x40 - 0x18;

/// Retail interact facing-probe table `DAT_801f2254` (field overlay 0897,
/// file offset `0x23A3C`, `0x40` bytes after [`FIELD_WALL_PROBES`]'s
/// `DAT_801f2214`), decoded from the disc. One `(dx, dz)` displacement per
/// 45° facing sector - a radius-64 compass point - applied to the player's
/// position with the shared `(x + dx, z - dz)` convention, so every entry
/// points 64 units *ahead* of the player along its sector's facing.
/// Indexed by the retail sector `(facing & 0xfff) >> 9` where retail facing
/// `0` looks along Z- (the engine's field heading stores `0` = Z+, a
/// half-turn off - see [`World::field_interact_probe_slot`]).
///
/// Retail reads this table in `FUN_801d01b0`'s touch dispatch: when the
/// configured interact button is just-pressed (`_DAT_8007b874 &
/// _DAT_800846d0`) it probes this single point through `FUN_801cf9f4` with
/// extents `0x20` and treats a result bit `1` (moving-class actor hit) as
/// "talk to that actor".
const FIELD_FACING_PROBES: [(i16, i16); 8] = [
    (0, 64),    // sector 0: facing Z- -> probe (x, z-64)
    (-64, 64),  // sector 1: Z- / X- diagonal
    (-64, 0),   // sector 2: facing X- -> probe (x-64, z)
    (-64, -64), // sector 3: X- / Z+ diagonal
    (0, -64),   // sector 4: facing Z+ -> probe (x, z+64)
    (64, -64),  // sector 5: Z+ / X+ diagonal
    (64, 0),    // sector 6: facing X+ -> probe (x+64, z)
    (64, 64),   // sector 7: X+ / Z- diagonal
];

/// Half-extent of the box a field NPC answers the *interact* probe with:
/// the touch dispatch passes extents `0x20` into `FUN_801cf9f4`, whose
/// moving-actor arm tests `|probe - pos| < 0x40 + (extent - 0x18)` per axis
/// - ±72 units, wider than the ±40 the locomotion probe gets with its zero
///   extents ([`FIELD_NPC_BOX_HALF`]).
const FIELD_INTERACT_BOX_HALF: i32 = 0x40 + 0x20 - 0x18;

/// Half-extent of a STATIC entity's (prop's) collision box: retail's
/// static arm always tests `|probe - centre| < 0x40 + 0x10` per axis (the
/// `0x10` is hard-coded, independent of the caller extents that widen the
/// moving-actor box) - ±80 units around the record-derived footprint centre.
const FIELD_PROP_BOX_HALF: i32 = 0x40 + 0x10;

/// Per-frame world-unit budget for one field-NPC motion-VM step. The exact
/// retail per-NPC glide speed (the `+0x72` multiplier path the NPC run
/// dispatch feeds) is not capture-pinned; the engine uses the player's
/// walking step magnitude (base step 8 × the default `0x1000` multiplier),
/// which paces an NPC like a walking player.
const FIELD_NPC_MOTION_SPEED: u16 = 8;

/// Motion-VM bytecode for one field-NPC walk leg: a single `0x47`
/// `MoveTowardTarget` op (the pursue/glide opcode of `FUN_8003774C`). The
/// engine resets the cursor per leg, so the one-op program is the whole
/// script.
const FIELD_NPC_MOTION_PROGRAM: [u8; 1] = [vm::motion_vm::MotionOp::MoveTowardTarget as u8];

/// One in-flight field-NPC walk leg, stepped through the ported motion VM
/// ([`legaia_engine_vm::motion_vm::step`], the `FUN_8003774C` port) by
/// `World::tick_field_npc_motions`. The live position lives in
/// [`World::field_npc_positions`] (so collision / interact probes follow the
/// walking NPC automatically); this carries the VM cursor + target.
#[derive(Debug, Clone)]
pub struct FieldNpcMotion {
    /// Motion-VM state (cursor, per-frame speed, accumulated budget). The
    /// `world_x` / `world_z` fields mirror the NPC's live position.
    pub state: vm::motion_vm::MotionState,
    /// World-space walk target of the current leg.
    pub target: (i16, i16),
    /// For an autonomous route leg: the index into
    /// [`World::field_npc_routes`] this leg walks toward (the next leg starts
    /// at `cursor + 1`, wrapping - a patrol loop). `None` for a
    /// script-started leg (interaction-prologue `0x4C 0x51` or actor-VM
    /// `start_motion`), which ends where it lands.
    pub route_cursor: Option<usize>,
}

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

/// Remap a screen-space d-pad delta into overworld direction bits using the
/// world-map camera azimuth, so "screen up" always walks away from the camera
/// and "screen right" walks screen-right regardless of how the map is framed.
///
/// Mirrors retail `func_0x800467e8`, which remaps the held pad through the same
/// camera yaw the renderer frames the overworld with. `azimuth` is PSX angle
/// units (`4096` = full turn) - the
/// [`WorldMapController`] azimuth the
/// renderer's `world_map_camera_mvp` orbits the eye by:
/// `eye = center + (d·cosθ, -0.7d, d·sinθ)`, `θ = azimuth / 4096 · τ`.
///
/// The world→screen axes are taken **from the real camera matrix, not from a
/// hand-derived "away from camera" guess**: under the renderer's Y-down
/// (eye at `-Y`, `+Y` up-vector) convention the on-screen vertical axis is
/// inverted relative to the eye→centre direction, so the verified mapping is
/// screen-up → world `(cosθ, sinθ)` and screen-right → world `(sinθ, -cosθ)`.
/// The `world_map_camera_relative_*` tests in `crates/engine-shell` project the
/// chosen world direction back through `world_map_camera_mvp` and assert it
/// moves the right way on screen for every azimuth, so this stays in lock-step
/// with the camera.
///
/// `sx` is the screen-right delta (`+1` = Right pressed), `sy` the
/// screen-up delta (`+1` = Up pressed). Returns the post-remap convention
/// bits (`0x1000` = Z+, `0x4000` = Z-, `0x2000` = X+, `0x8000` = X-), quantised
/// to 8 directions (a world axis is taken when its component is within ~22.5°
/// of that axis); `0` when nothing is held.
pub fn world_map_camera_relative_bits(azimuth: i32, sx: i32, sy: i32) -> u16 {
    if sx == 0 && sy == 0 {
        return 0;
    }
    let theta = (azimuth as f32) / 4096.0 * std::f32::consts::TAU;
    let (sin, cos) = theta.sin_cos();
    // screen-up    -> world (-cosθ, -sinθ)   (verified against world_map_camera_mvp)
    // screen-right -> world ( sinθ, -cosθ)
    // The camera looks down on the (Y-up) flipped terrain from positive Y, so
    // the on-screen vertical axis runs opposite the eye->centre forward dir;
    // hence the screen-up -> world mapping carries the negative sign.
    let wx = (sx as f32) * sin - (sy as f32) * cos;
    let wz = -(sx as f32) * cos - (sy as f32) * sin;
    // sin(22.5°): within this band of an axis the press is treated as cardinal;
    // beyond it (a rotated framing) both bits set and the player walks diagonally.
    const T: f32 = 0.382_683_43;
    let mut bits = 0u16;
    if wz > T {
        bits |= 0x1000; // Z+
    } else if wz < -T {
        bits |= 0x4000; // Z-
    }
    if wx > T {
        bits |= 0x2000; // X+
    } else if wx < -T {
        bits |= 0x8000; // X-
    }
    bits
}

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
pub const PROLOGUE_HANDOFF_FLAG: u32 = 1 << PROLOGUE_HANDOFF_BIT;

/// Scratchpad flag-bit index (`26`) of [`PROLOGUE_HANDOFF_FLAG`]. This is
/// the operand of the prologue cutscene's `GFLAG_SET` op (`0x2E 0x1A`); the
/// data-driven arm [`World::arm_prologue_handoff_from_man`] matches a MAN
/// `GFLAG_SET` against this bit.
pub const PROLOGUE_HANDOFF_BIT: u32 = 26;

/// Per-frame field-VM step budget for the opening-cutscene timeline
/// ([`World::step_cutscene_timeline`]). Bounds a non-yielding stretch of real
/// disc bytecode so it can't hang the tick; the timeline normally yields or
/// waits well within this.
const CUTSCENE_TIMELINE_STEP_BUDGET: u32 = 256;

/// Frame cap for a cutscene timeline that must return control (the `town01`
/// opening). If the spawned context never reaches its terminal within this
/// many frames (≈20 s at 60 fps), the engine forces it complete.
const CUTSCENE_TIMELINE_MAX_FRAMES: u32 = 1200;

/// Frame cap for the `opdeene` prologue timeline. The record arms the
/// hand-off bit near its TOP (`GFLAG_SET 26` at body `+0x17`) and then plays
/// the whole vignette choreography - camera beats, actor-channel pokes,
/// waits - for the narration's duration, so it must NOT complete on the bit;
/// it runs until the record reaches a terminal state, the player confirms the
/// hand-off (the scene change drops it), or this generous cap (≈60 s).
const PROLOGUE_TIMELINE_MAX_FRAMES: u32 = 3600;

/// Per-frame, per-channel field-VM step budget for the spawned per-actor
/// channels ([`World::step_field_channels`]). Retail slices end at a yield /
/// park / `0x21` NOP, normally within a handful of ops; the budget bounds a
/// malformed non-yielding stretch.
const FIELD_CHANNEL_STEP_BUDGET: u32 = 128;

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
///
/// REF: FUN_801E0088
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

/// Coarse role of a placed overworld entity, carried on
/// [`WorldMapEntityMarker`] so a host can colour-code its render marker
/// without inspecting the full [`WorldMapEntityConfig`] (which carries
/// per-kind payload the renderer doesn't need).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMapEntityKind {
    /// A roaming-encounter zone ([`WorldMapEntityConfig::EncounterZone`]).
    EncounterZone,
    /// A town / dungeon portal ([`WorldMapEntityConfig::Portal`]).
    Portal,
    /// A plain interactable NPC / signpost ([`WorldMapEntityConfig::Npc`]),
    /// also the fallback for an entity with no config row.
    Npc,
}

/// Render-agnostic snapshot of one placed overworld entity, produced by
/// [`World::world_map_entity_markers`]. The disc-sourced placement seeding
/// ([`crate::scene::SceneHost::enter_world_map_scene`]) installs each entity
/// with a world position and a [`WorldMapEntityConfig`]; this pairs the
/// position with its coarse [`WorldMapEntityKind`] so a host can draw a marker
/// at each on-map portal / NPC / encounter zone.
///
/// This is the seam for the still-open per-entity mesh-resolution thread: the
/// retail engine binds each placement to its own actor model, which is not yet
/// decoded, so the host draws a kind-coded marker at the position rather than
/// the entity's real mesh. The position shares the player's coordinate frame
/// (both come from the scene MAN), so a marker reads correctly relative to the
/// player even while the kingdom terrain mesh renders at its own pack-local
/// coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldMapEntityMarker {
    /// Entity world position in world units. `x` / `z` are the MAN placement
    /// coordinates; `y` is the player actor's current plane (the entities are
    /// 2D placements, so they sit on the player's walking plane).
    pub world_pos: [f32; 3],
    /// Coarse role, for colour-coding the marker.
    pub kind: WorldMapEntityKind,
}

/// Render-agnostic snapshot of the player's overworld position, produced by
/// [`World::world_map_player_marker`]. In `SceneMode::WorldMap` the kingdom
/// terrain mesh renders at its pack-local coordinates and the player actor's
/// own mesh is not drawn, so a host draws a marker at this position to show the
/// player on the map. `facing` is the player's heading (`render_26`), so the
/// host can draw a direction tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldMapPlayerMarker {
    /// Player world position in world units (the player actor's
    /// `move_state.world_x / y / z`).
    pub world_pos: [f32; 3],
    /// Player heading as a PSX 12-bit angle (`4096` = full turn), measured the
    /// same way the field path stores it (`render_26`). `0` faces `+Z`.
    pub facing: i16,
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
    /// Noa dance (rhythm) minigame - the beat clock + hit judge own the frame
    /// ([`crate::dance::DanceGame`]); field / battle dispatch is suspended. The
    /// hosting session preserves the suspended scene state and restores its
    /// mode on exit, like the pause menu. Retail `game_mode 0x18` (`OTHER MODE`)
    /// overlay pair for the dance overlay (PROT 0980).
    Dance,
    /// Fishing minigame - the cast / fight / score loop owns the frame
    /// ([`crate::fishing::FishingSession`]); field / battle dispatch is
    /// suspended and the interrupted mode restored on exit. Retail
    /// `game_mode 0x18` (`OTHER MODE`) overlay pair for the fishing overlay
    /// (PROT 0972).
    Fishing,
    /// Casino slot-machine minigame - the reel state machine owns the frame
    /// ([`crate::slot_machine::SlotMachine`]); field / battle dispatch is
    /// suspended and the interrupted mode restored on exit. Retail
    /// `game_mode 0x18` (`OTHER MODE`, the mode-24 minigame door-warp) for
    /// the slot overlay (PROT 0975).
    SlotMachine,
    /// Baka Fighter duel minigame - the exchange / round / match state
    /// machine owns the frame ([`crate::baka_fighter::BakaFight`]); field /
    /// battle dispatch is suspended and the interrupted mode restored on
    /// exit. Retail `game_mode 0x18` (`OTHER MODE`, the mode-24 minigame
    /// door-warp) for the Baka Fighter overlay (PROT 0976).
    BakaFighter,
    /// Muscle Dome card-battle contest - the hand-select / commit / resolve
    /// loop owns the frame ([`crate::muscle_dome::MuscleDomeSession`]); field
    /// / battle dispatch is suspended and the interrupted mode restored on
    /// exit. Retail: the arena runs *inside* the battle overlay (PROT 0898)
    /// on the `_DAT_8007bd24` context, entered through the mode-24 sub-id-5
    /// door (PROT 0977).
    MuscleDome,
    /// In-field pause menu - the retail CARD mode pair (`game_mode 0x17`,
    /// `CARD MODE`, which hosts both the memory-card UI and the pause menu;
    /// every menu-open capture holds `_DAT_8007B83C = 0x17`). Field/battle
    /// dispatch is suspended while the menu owns the frame; only the actor
    /// VM and effect pool run, like `Title`. The hosting session preserves
    /// the suspended scene state and restores its mode on close.
    Menu,
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

    /// Most recent status effect inflicted by an art strike (Toxic /
    /// Numb / …). Engines clear this when they've folded it into their
    /// status-bar UI; defaults to `None`.
    pub pending_status: Option<legaia_art::record::EnemyEffect>,

    /// Optional sprite frame for this actor. Drives the per-frame sprite
    /// batch through [`World::collect_sprite_requests`]. When `None`, the
    /// actor is invisible (or rendered as a 3D mesh through the TMD path).
    pub sprite_frame: Option<SpriteFrame>,

    /// Active keyframe animation player. `None` means no animation is
    /// playing. Set via [`World::set_actor_animation`].
    pub active_animation: Option<AnimPlayer>,

    /// Last per-bone pose produced by `active_animation.tick()` (field) or
    /// `battle_animation.tick()` (battle). `None` until the first frame after
    /// an animation is assigned. Field renderers consume this via
    /// `tmd_to_vram_mesh_posed` (translation only); battle renderers use
    /// `tmd_to_vram_mesh_posed_rot` (full per-object rigid transform).
    pub pose_frame: Option<PoseFrame>,

    /// Battle per-object rigid-transform animation player. Set at battle init
    /// for monster (and player-summon) actors from their archive idle clip; the
    /// battle tick advances it into `pose_frame`. Mutually exclusive with
    /// `active_animation` (field ANM) on a given actor.
    pub battle_animation: Option<crate::battle_anim::MonsterAnimPlayer>,

    /// Per-slot battle action clips for this actor (the player files'
    /// `record[0]` action streams, expanded so channel `i` drives TMD object
    /// `i`), indexed by action-stream slot; index 0 = the idle loop. Set by
    /// the host at battle init (see `World::set_actor_battle_action_clips`);
    /// consulted by the battle-action SM's `pose()` host hook to switch
    /// `battle_animation` between idle and action clips. `None` keeps the
    /// idle-only behavior.
    pub battle_action_clips: Option<std::sync::Arc<Vec<Option<MonsterAnimation>>>>,

    /// Pose id the current `battle_animation` was selected for, in the pose-id
    /// space the battle-action SM emits (`6` idle / `7` ready / `8` recover /
    /// `9` defeat). Lets a repeated request for the same pose keep the playing
    /// clip instead of restarting it. (Retail mechanism note: the SM's
    /// `FUN_801D5854(actor, 6..9)` calls are camera/presentation programs - the
    /// anim ids retail stages into `actor+0x1DA` are entry indices, with idle
    /// = id `0`; ids `7`/`8`/`9` are real entries staged by the SM and the
    /// anim commit `FUN_8004AD80`. The engine folds both spaces into this one
    /// hook; pose `6` plays entry 0's idle loop, which matches the frames
    /// retail shows.)
    pub battle_pose: Option<u8>,

    /// Hit-reaction action tag currently playing on `battle_animation`
    /// (the retail `actor+0x1EF..+0x1F3` key space: `2`/`3` light flinch,
    /// `4` knockdown, `5` get-up, `0x0B` block). Set by
    /// [`World::queue_battle_reaction`]; cleared when the reaction chain
    /// finishes and idle resumes. Takes precedence over `battle_pose` while
    /// active.
    pub battle_reaction: Option<u8>,

    /// Per-character **art-animation bank** clips (the player file's
    /// record[0] `+0x58` bank, each record's keyframe stream resolved
    /// through its `readef.DAT` `"ME"` archive and expanded so channel `i`
    /// drives TMD object `i`), indexed by bank record. The staged-anim
    /// commit ([`World::commit_staged_battle_anims`]) materializes record
    /// `q - 0x10` from here for a staged id `q >= 0x10`, exactly like
    /// retail `FUN_8004AD80`. `None` (monsters, or hosts that didn't decode
    /// the bank) treats staged ids as plain action-table entry indices.
    pub battle_art_bank: Option<std::sync::Arc<Vec<Option<MonsterAnimation>>>>,

    /// Committed anim id (post `FUN_8004AD80` rewrite) of the **staged
    /// one-shot clip** currently owning `battle_animation` - a weapon swing
    /// (`0xC..0xF`) or a materialized art record (dynamic slot `0x10`/
    /// `0x11`). While `Some`, the SM's per-frame `pose()` requests don't
    /// steal the player (same precedence rule as `battle_reaction`).
    /// Cleared by [`World::tick_battle_animations`] when the clip finishes:
    /// that's the engine's anim-end signal - `ADVANCE_DONE` is cleared so
    /// the attack chain reads its next strike byte, and the id pair
    /// converges back to idle `0`.
    pub battle_staged_anim: Option<u8>,

    /// Battle monster texture slot (`0..=4`). The monster TMD's on-disc CBA/TSB
    /// are nominal defaults the battle loader relocates per slot
    /// (`FUN_80055468` → `legaia_asset::monster_archive::relocate_cba/relocate_tsb`).
    /// Renderers that rebuild this actor's mesh from the raw TMD each frame (the
    /// posed-animation path) must re-apply that relocation, since a fresh build
    /// carries the nominal addresses again. `None` for non-monster actors.
    pub battle_tex_slot: Option<u8>,

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
    /// `World::enter_battle_from_formation`; lets a renderer fetch the
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
/// so `World::finish_battle` (or natural expiry) can revert it precisely.
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
///
/// REF: FUN_801E9FD4
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

    /// Static prop collision-box centres `(world_x, world_z)`, one per placed
    /// object of the scene's field `.MAP` object grid - the engine's source
    /// for the **static-entity arm** of the actor-collision probe (retail
    /// `FUN_801cf9f4` result bit `4`; box half-extent
    /// `FIELD_PROP_BOX_HALF`). Installed at field-scene entry from
    /// [`crate::scene::Scene::field_object_placements`] (each placement's
    /// [`collider_x`](legaia_asset::field_objects::Placement::collider_x) /
    /// `collider_z` = spawn position + the record's collision-footprint
    /// offset, live-verified against the spawned static actors of catalogued
    /// captures). Gated by [`Self::solid_field_npcs`] alongside the
    /// moving-NPC arm.
    pub field_prop_colliders: Vec<(i32, i32)>,

    /// Per-NPC autonomous walk routes, keyed by the same placement `slot` as
    /// [`Self::field_npc_dialog`]: the ordered local waypoints the placement's
    /// own pre-text script walks the actor through (its `0x4C 0x51` NPC
    /// move-to-tile ops - [`crate::man_field_scripts::placement_motion_route`]).
    /// Driven through the motion VM by `Self::tick_field_npc_motions` when
    /// [`Self::animate_field_npcs`] is set. `BTreeMap` so the per-tick walk
    /// order is deterministic (the replay oracle requires bit-stable traces).
    pub field_npc_routes: std::collections::BTreeMap<u8, Vec<(i16, i16)>>,

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

    /// Walk-touch edge latch: the slot whose contact box the player currently
    /// stands in, so a sustained press posts its event once (retail gates the
    /// per-step post on the player's `+0x10 & 0x80000` engaged flag, cleared
    /// by the dialog SM teardown - the engine latches per contact instead).
    pub active_walk_touch: Option<u8>,

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

    /// Screen-effect widget host (the PROT-0900 mask / sprite / panel /
    /// letterbox family), driven by the field-VM op `0x43` sub-ops
    /// `0x10`/`0x11`/`0x13`/`0x14`/`0x15` - the ending-scene widget
    /// path. See [`crate::screen_fx`].
    pub screen_fx: crate::screen_fx::ScreenFxHost,

    /// The current frame's widget draw list, refreshed by the Field /
    /// Cutscene tick while any widget is live ([`Self::tick_screen_fx`]).
    /// Renderers composite these 2D overlays above the scene.
    pub screen_fx_frame: crate::screen_fx::ScreenFxFrame,

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

    /// Active opening-cutscene timeline executor, or `None` when no cutscene
    /// timeline is running. Installed by
    /// [`Self::load_cutscene_timeline_from_man`] (the `opdeene` opening
    /// prologue) with the partition-2 record that issues `GFLAG_SET 26`;
    /// stepped each frame by [`Self::step_cutscene_timeline`] so the cutscene's
    /// camera path + actor moves play and the hand-off bit fires by execution.
    /// See [`crate::cutscene_timeline::CutsceneTimeline`].
    pub cutscene_timeline: Option<crate::cutscene_timeline::CutsceneTimeline>,

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
}

/// A live scripted-encounter carrier menu: the 4-option picker its dialogue
/// presents (the Rim Elm spar's "hear about Biron / secrets of fighting /
/// **practice with you** / nothing"). Navigated with Up/Down; confirming the
/// `fight_option` index engages the carrier (-> Battle), any other option just
/// closes (its talk branch). Mirrors the retail inline-picker cursor
/// `*(0x801C6EA4)+0x0C`; the engine can't run the option's field-VM branch, so it
/// gates the engage on `fight_option` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarrierMenu {
    /// Index into [`World::field_carriers`] this menu engages.
    pub carrier_idx: usize,
    /// Number of options in the picker (4 for the spar).
    pub n: usize,
    /// The picker option whose confirm engages the carrier (the fight choice).
    pub fight_option: usize,
    /// Highlighted option `0..n`.
    pub cursor: usize,
}

/// If `dialogue` presents the spar's 4-option picker, return `(n_options,
/// fight_option_index)`. The fight option is the choice whose label is the
/// "practice" request ("I want to practice with you." - RE-pinned index 2 for
/// the Rim Elm spar). `None` when the dialogue carries no such picker, so a
/// carrier without a menu keeps the any-accept engage path.
fn spar_menu_of(dialogue: &[u8]) -> Option<(usize, usize)> {
    for p in legaia_mes::scan_pickers(dialogue) {
        if p.n != 4 {
            continue;
        }
        if let Some(f) = p.options.iter().position(|o| {
            o.label
                .windows(8)
                .any(|w| w.eq_ignore_ascii_case(b"practice"))
        }) {
            return Some((p.n, f));
        }
    }
    None
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
    ///
    /// `inline` is the entity's own placement-script dialog text (the `0x3F`
    /// op's inline buffer the placement walker captured); `World::tick_world_map`
    /// opens it when the player presses confirm next to the entity. `text_id`
    /// is the box-config id carried alongside it (not an MES index). Both are
    /// empty/`None` when the placement has an interaction but no inline dialog.
    Npc {
        interact_id: u8,
        text_id: Option<u16>,
        inline: Vec<u8>,
    },
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
            field_map_region_block: Vec::new(),
            field_zone_table: Vec::new(),
            field_region_attributes: crate::field_regions::RegionAttributes::DEFAULT_FILL,
            field_zone_record: None,
            field_floor_height_lut: [0i16; 16],
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
            last_field_interact: None,
            field_npc_dialog: std::collections::HashMap::new(),
            field_npc_dialog_prologue: std::collections::HashMap::new(),
            active_inline_prologue: None,
            field_npc_positions: std::collections::HashMap::new(),
            field_npc_headings: std::collections::HashMap::new(),
            field_prop_colliders: Vec::new(),
            field_npc_routes: std::collections::BTreeMap::new(),
            field_npc_motions: std::collections::BTreeMap::new(),
            animate_field_npcs: false,
            field_walk_touch: std::collections::BTreeMap::new(),
            active_walk_touch: None,
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
            screen_fx: Default::default(),
            screen_fx_frame: Default::default(),
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
            cutscene_timeline: None,
            inline_dialogue: None,
            in_cutscene_timeline: false,
            field_channels: Vec::new(),
            field_channels_man: None,
            executing_channel: None,
            field_npc_anim_cues: std::collections::HashMap::new(),
            prologue_naming_pending: false,
            prologue_naming_armed: false,
            entering_town01_opening: false,
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
        self.prologue_naming_pending = false;
        self.prologue_naming_armed = false;
        self.entering_town01_opening = false;
        self.mode = SceneMode::Field;
    }
}

mod actors;
mod assets_events;
mod battle;
mod effects;
mod encounters;
mod field_carriers;
mod field_loop;
mod field_movement;
mod frame_tick;
mod items_arts;
mod narration;
mod save;
mod vm_hosts;
mod worldmap;

#[cfg(test)]
mod tests;
