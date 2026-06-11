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
//! PORT: FUN_800467E8 (`world_map_camera_relative_bits` — held-pad camera-yaw
//!       remap; the engine reads the world-map camera azimuth directly from
//!       [`WorldMapController`] rather than the retail `gp+0x2D8` quadrant.)

use std::sync::Arc;

use crate::battle_events::{BattleEvent, BattleHitFx, BattleSfxCue};
use crate::field_events::FieldEvent;
use crate::input;
use crate::levelup::{LevelUpBanner, LevelUpResult, LevelUpTracker};
use crate::move_buffer_host;
use crate::tactical_arts::{ArtLearnedBanner, TacticalArtsTracker};
use crate::world_map::WorldMapController;
pub use legaia_anm::{AnimPlayer, PoseFrame};
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
/// player's CURRENT position as `(x + dx, z - dz)` — note the Z delta is
/// SUBTRACTED, exactly as the retail probe reads it. A direction is blocked
/// when ANY of its three probes lands on a wall sub-cell.
///
/// The lookahead is asymmetric on purpose: the `+2`-biased Z mapping and the
/// `ceil-1` X mapping ([`World::field_tile_is_wall`]) make 48 the crossing
/// distance in the positive directions and 47 in the negative ones, so each
/// edge sits one full tile ahead in cell space. (The on-disc rows are
/// 16 bytes; the trailing fourth pair — a half-distance centre point — is
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
/// (STATIC entities — props, `flags & 0x1020000 == 0` — use a wider
/// `0x40 + 0x10` = 80-unit box around their MAN record anchor instead; the
/// engine has no prop-collision list, so that arm is unmodelled.)
const FIELD_NPC_BOX_HALF: i32 = 0x40 - 0x18;

/// Retail interact facing-probe table `DAT_801f2254` (field overlay 0897,
/// file offset `0x23A3C`, `0x40` bytes after [`FIELD_WALL_PROBES`]'s
/// `DAT_801f2214`), decoded from the disc. One `(dx, dz)` displacement per
/// 45° facing sector — a radius-64 compass point — applied to the player's
/// position with the shared `(x + dx, z - dz)` convention, so every entry
/// points 64 units *ahead* of the player along its sector's facing.
/// Indexed by the retail sector `(facing & 0xfff) >> 9` where retail facing
/// `0` looks along Z- (the engine's field heading stores `0` = Z+, a
/// half-turn off — see [`World::field_interact_probe_slot`]).
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
/// — ±72 units, wider than the ±40 the locomotion probe gets with its zero
/// extents ([`FIELD_NPC_BOX_HALF`]).
const FIELD_INTERACT_BOX_HALF: i32 = 0x40 + 0x20 - 0x18;

/// Half-extent of a STATIC entity's (prop's) collision box: retail's
/// static arm always tests `|probe - centre| < 0x40 + 0x10` per axis (the
/// `0x10` is hard-coded, independent of the caller extents that widen the
/// moving-actor box) — ±80 units around the record-derived footprint centre.
const FIELD_PROP_BOX_HALF: i32 = 0x40 + 0x10;

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
/// units (`4096` = full turn) — the
/// [`WorldMapController`](crate::world_map::WorldMapController) azimuth the
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

/// Frame cap for the opening-cutscene timeline. If the spawned context never
/// reaches its closing `GFLAG_SET 26` within this many frames (≈20 s at 60 fps,
/// generous for the opening's camera path), the engine forces it complete and
/// arms the hand-off statically so the prologue can't stall.
const CUTSCENE_TIMELINE_MAX_FRAMES: u32 = 1200;

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
    /// When set, pad locomotion blocks a direction with retail's
    /// **three-probe leading-edge footprint** ([`FIELD_WALL_PROBES`], the
    /// `DAT_801f2214` table `FUN_801cfe4c` walks) instead of a single
    /// candidate-centre test: the player rests ~47 units off a wall plane
    /// exactly like retail, instead of walking up to it. Off by default so
    /// the existing locomotion oracles (and BFS nav drivers, which keep the
    /// centre test regardless) are bit-identical; enable it for
    /// retail-faithful wall standoff. Validated against the wall-press
    /// captures by `engine-shell/tests/field_collision_discriminator.rs`.
    pub leading_edge_wall_probes: bool,
    /// When set, pad locomotion also blocks a direction when any of retail's
    /// **actor-collision probes** ([`FIELD_ACTOR_PROBES`], the `DAT_801f21b4`
    /// sibling table) lands inside a field NPC's body box or a placed prop's
    /// static collision box ([`Self::field_actor_dir_blocked`]) - NPCs and
    /// props become solid, as in retail, where `FUN_801cfe4c`'s actor bits
    /// (`1`/`4`) gate a step exactly like the wall bit (`2`). Off by default
    /// for the same oracle-stability reason as
    /// [`Self::leading_edge_wall_probes`]. The locomotion-path touch
    /// side-effects (event post `FUN_801d5b5c` on the `+0x98`-linked partner
    /// for prop walk-touch) are not modelled; the button-press interact
    /// dispatch is ([`Self::tick_field_interaction_probe`]).
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
    /// turn-order fallback ([`World::next_living_combatant`]); when any actor
    /// carries real SPD the next-actor selector switches to the SPD-seeded
    /// initiative scheme ([`World::next_combatant_by_initiative`], the port of
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

    /// Last-issued battle-end cause (for inspection / engine side-effects).
    pub battle_end: Option<BattleEndCause>,

    /// Active full-screen fade, staged by the battle SM's escape teardown
    /// (retail state `0x66` spawns the `DAT_801C9070` black→white ramp via
    /// the fade-primitive spawner `FUN_80024E80`). Stepped once per
    /// [`World::tick`]; dropped when the ramp completes. Hosts draw an
    /// overlay from [`crate::fade::FadeState::rgb`] while this is `Some`.
    pub screen_fade: Option<crate::fade::FadeState>,

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
    /// entry_z)` — the entry-tile bytes are kept for future destination
    /// spawn-point wiring. `None` between transitions.
    pub pending_named_scene_transition: Option<(String, u8, u8)>,

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

    /// Per-strike battle sound cues surfaced this frame for the host to play
    /// through its SFX bank (the art-record `HitCue` sound cues that
    /// [`World::fold_battle_event`] resolves from an `ApplyArtStrike` outcome —
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

    /// Per-actor inline interaction-script dialogue, keyed by the actor's
    /// MAN partition-1 record index — the `slot` a field-interact op
    /// (`0x3E` with `op0 < 100`) carries. Populated at field-scene entry from
    /// the scene's actor placements. This is the **real** field NPC dialogue
    /// source (the actor's inline MES text at retail `actor[+0x90]`), so
    /// [`crate::world::vm_hosts`]'s `field_interact` opens the interacted
    /// actor's dialogue from here — not from a `0x3F` op (which is the named
    /// scene-change, not dialogue). Empty between field scenes.
    pub field_npc_dialog: std::collections::HashMap<u8, Vec<u8>>,

    /// Prologue-aware companion to [`Self::field_npc_dialog`], keyed by the same
    /// `slot`. Carries each talk NPC's **untruncated** interaction record (full
    /// body + entry PC + first-segment offset) so the opt-in field-VM dialogue
    /// runner ([`Self::use_vm_dialogue`]) can execute the interaction prologue —
    /// the story-flag `SysFlag.Test`/`JmpRel` segment-selection bytecode before
    /// the first `0x1F` — instead of starting at the first segment. The default
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
    /// ([`Self::tick_field_interaction_probe`]) box-tests the player's position
    /// against these to fire a `field_interact` on the action button — the
    /// clean-room analogue of retail's `FUN_801cf9f4` adjacency test.
    ///
    /// The runtime actor frame **is** the MAN placement frame: `FUN_8003A1E4`
    /// spawns each actor at `world = tile*128 + 0x40` (the placement's
    /// [`world_x`](legaia_asset::man_section::ActorPlacement::world_x)), stored
    /// straight into `actor[+0x14/+0x18]` by `FUN_80024C88` with no anchor, and
    /// the player cold-spawn `0xA40` is `tile 20*128 + 0x40` in that same frame.
    /// So these placement positions compare directly against the player's
    /// [`crate::vm::ActorMoveState::world_x`]. (Positions are the *spawn* tile;
    /// the engine does not yet walk field NPCs, so spawn == current.)
    pub field_npc_positions: std::collections::HashMap<u8, (i16, i16)>,

    /// Static prop collision-box centres `(world_x, world_z)`, one per placed
    /// object of the scene's field `.MAP` object grid — the engine's source
    /// for the **static-entity arm** of the actor-collision probe (retail
    /// `FUN_801cf9f4` result bit `4`; box half-extent
    /// [`FIELD_PROP_BOX_HALF`]). Installed at field-scene entry from
    /// [`crate::scene::Scene::field_object_placements`] (each placement's
    /// [`collider_x`](legaia_asset::field_objects::Placement::collider_x) /
    /// `collider_z` = spawn position + the record's collision-footprint
    /// offset, live-verified against the spawned static actors of catalogued
    /// captures). Gated by [`Self::solid_field_npcs`] alongside the
    /// moving-NPC arm.
    pub field_prop_colliders: Vec<(i32, i32)>,

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

    /// Optional battle-action move-power table (PROT 0898, runtime VA
    /// `0x801F4F5C`). When present, the monster special-attack damage path
    /// resolves each move id to its real per-move power scalar and rolls
    /// damage through the faithful arts/physical kernel
    /// ([`crate::world::World::enemy_move_predamage`]); when `None` (disc-free /
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
    /// `0x81..=0x8b`) sets `(spell_id, target world pos)` here — the engine
    /// equivalent of the retail cast band resolving the per-summon overlay
    /// (`FUN_8003EC70(id-0x79)`). The host (which has the PROT index) drains it
    /// via [`World::take_pending_summon_spawn`], loads the summon overlay
    /// (extraction `PROT 903 + (id - 0x81)`), and calls [`World::spawn_summon`]. Kept as a
    /// host-fulfilled request because `World` is index-agnostic (same pattern
    /// as the capture-archive load).
    pub pending_summon_spawn: Option<(u8, [i16; 3])>,

    /// Active battle move-power effect-FX scene-graph, while one is playing. A
    /// move's `0x01..=0x63` on-contact / launch effect-list entries spawn the
    /// `0x801f6324` prototype records (summon-format move-VM parts) through the
    /// same machinery as a summon — [`World::spawn_move_fx`] seeds it,
    /// [`World::tick_move_fx`] advances it, [`World::active_move_fx_part_draws`]
    /// renders it. Separate from [`active_summon`](Self::active_summon) so a
    /// move's FX and a summon don't clobber each other.
    pub active_move_fx: Option<crate::summon::SummonScene>,

    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, set by [`World::spawn_move_fx`] from the move record's
    /// `+0x0b` field and cleared when the scene drains. Surfaced via
    /// [`World::active_move_fx_trail_texpage`] for the render layer's streak
    /// pass — the trail id this carries is what
    /// `legaia_engine_render::afterimage::build_afterimage_quad` (the ported
    /// `FUN_801e1ab0`) turns into the jittered semi-transparent quad.
    pub active_move_fx_trail_texpage: Option<u16>,

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
    /// with the lowest-HP living member. Off by default — the retail behaviour
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
    /// by default — when off, behaviour is identical to before.
    pub use_vm_dialogue: bool,

    /// Opt-in: route the live basic-attack damage through the retail damage
    /// finisher ([`legaia_engine_vm::battle_formulas::damage_finish`], the port
    /// of `FUN_801ddb30`) instead of stopping at the raw roll. The finisher
    /// adds the universal post-stages — elemental resistance, guard / enemy
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

    /// Magic-XP threshold table from `SCUS_942.54` (`0x8007656C`, 8 ascending
    /// u16 steps). Installed at boot via
    /// [`World::install_magic_xp_thresholds`]; while `None` (disc-free) summon
    /// casts still accrue spell XP but never level the spell up.
    pub magic_xp_thresholds: Option<[u16; crate::magic_xp::THRESHOLD_STEPS]>,

    /// Summon-magic level-ups resolved this session: `(party_slot, spell_id,
    /// new_level)` per event, in resolution order. The engine analogue of the
    /// retail level-up banner (the level-up check fires UI element `0x65` —
    /// REF: FUN_801e70bc, ported in `world::battle::accrue_summon_spell_xp`);
    /// hosts drain via [`World::drain_magic_level_ups`].
    pub magic_level_ups: Vec<(u8, u8, u8)>,

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

    /// Per-entity overworld world position `(x, z)`, paired by index with
    /// [`Self::world_map_entities`]. Populated only by
    /// [`Self::install_world_map_entities_at`] (the disc placement seeding);
    /// the config-only installers leave it empty. When present, it drives the
    /// **auto-engage-on-walkover** trigger in [`Self::tick_world_map`]: the
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
    /// of [`Self::tick_world_map`] to flip into [`SceneMode::Battle`]. `None`
    /// between encounters.
    pub pending_world_map_encounter: Option<u16>,

    /// Region-keyed random-encounter state for the overworld (the
    /// `FUN_801D9E1C` port, [`crate::region_encounter`]). When set,
    /// [`Self::tick_world_map`] rolls it once per 128-unit tile the player
    /// crosses, latching [`Self::pending_world_map_encounter`] on a trigger.
    /// `None` on a camera-only world map (no region data routed).
    ///
    /// REF: FUN_801D9E1C
    pub world_map_region_tracker: Option<crate::region_encounter::RegionEncounterTracker>,

    /// Player tile (`world >> 7`) at the previous overworld step check, for
    /// per-tile step detection. `None` until the first world-map tick seeds it.
    pub world_map_last_tile: Option<(i32, i32)>,

    /// Overworld player walk speed in world units per frame (per held d-pad
    /// direction). Default [`Self::WORLD_MAP_PLAYER_SPEED`].
    pub world_map_player_speed: i16,

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

    /// Field-interact `slot` -> [`Self::field_carriers`] index, for the
    /// **scripted-encounter** carriers only. Built by
    /// [`Self::install_field_carriers_from_man`] so a field-interact on the
    /// sparring partner's placement can find its carrier and auto-arm the fight
    /// (the dialogue-accept drives the engage instead of the manual API). Plain
    /// talk NPCs are deliberately absent — interacting with them never launches
    /// a battle.
    pub field_carrier_slots: std::collections::HashMap<u8, usize>,

    /// A scripted-encounter carrier whose dialogue the player opened via a
    /// field-interact and which engages when that dialogue is dismissed (the
    /// accept). Set in [`crate::world::vm_hosts`]'s `field_interact`, consumed
    /// by the dialog-advance dismiss (`op 0x4C n5 sub-4`). `None` when no
    /// scripted carrier's prompt is up.
    pub pending_carrier_engage: Option<usize>,

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
    /// op's inline buffer the placement walker captured); [`World::tick_world_map`]
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
            battle_end: None,
            screen_fade: None,
            roster: legaia_save::Party::zeroed(0),
            pending_scene_transition: None,
            pending_named_scene_transition: None,
            pending_fmv_trigger: None,
            pending_scripted_encounter: None,
            scripted_encounter_armed: false,
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
            field_prop_colliders: Vec::new(),
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
            status_effects: vm::status_effects::StatusEffectTracker::new(),
            ap_gauges: [crate::ap_gauge::ApGauge::default(); 3],
            fury_boost: [None; 3],
            item_catalog: crate::items::ItemCatalog::default(),
            item_effects: None,
            spell_catalog: crate::spells::SpellCatalog::default(),
            art_records: std::collections::HashMap::new(),
            battle_buffs: Vec::new(),
            battle_captures: Vec::new(),
            magic_xp_thresholds: None,
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
            world_map_player_speed: Self::WORLD_MAP_PLAYER_SPEED,
            battle_return_mode: SceneMode::Field,
            field_carriers: Vec::new(),
            field_carrier_configs: Vec::new(),
            pending_field_carrier_battle: None,
            field_carrier_slots: std::collections::HashMap::new(),
            pending_carrier_engage: None,
            party_names: Vec::new(),
            name_entry: None,
            cutscene_narration: None,
            cutscene_timeline: None,
            inline_dialogue: None,
            in_cutscene_timeline: false,
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
        self.pending_named_scene_transition = None;
        self.pending_fmv_trigger = None;
        self.pending_scripted_encounter = None;
        self.scripted_encounter_armed = false;
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

    /// Install the opening-cutscene narration presenter with `pages` (the
    /// inline subtitle pages decoded from the scene MAN's cutscene-timeline
    /// script; see [`crate::man_field_scripts::collect_partition_narration`]).
    /// A presenter with no pages installs nothing - a scene that carries no
    /// inline narration simply never shows one. The host renders the active
    /// page from [`Self::cutscene_narration`]; [`Self::tick`] advances its
    /// per-page timer.
    pub fn open_cutscene_narration(&mut self, pages: Vec<String>) {
        if pages.is_empty() {
            return;
        }
        self.cutscene_narration = Some(crate::cutscene_narration::CutsceneNarration::new(pages));
    }

    /// `true` while the opening-cutscene narration is on screen (not yet
    /// stepped past its last page). Hosts gate the prologue hand-off on this:
    /// the narration plays first, the Rim Elm hand-off follows.
    pub fn cutscene_narration_active(&self) -> bool {
        self.cutscene_narration
            .as_ref()
            .is_some_and(|n| !n.is_complete())
    }

    /// Skip the active narration to its next page (a confirm press). Clears
    /// the presenter once it advances past the last page. Returns `true` while
    /// narration is still on screen, `false` once it completes (so the host
    /// lets the confirm fall through to [`Self::take_prologue_handoff`]).
    pub fn skip_cutscene_narration(&mut self) -> bool {
        let Some(narration) = self.cutscene_narration.as_mut() else {
            return false;
        };
        let still_active = narration.skip_page();
        if !still_active {
            self.cutscene_narration = None;
        }
        still_active
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

    /// Arm the prologue -> Rim Elm hand-off **only when** the scene's MAN
    /// cutscene timeline actually issues the `GFLAG_SET 26` write the retail
    /// hand-off gate waits on.
    ///
    /// This is the data-driven companion to [`Self::arm_prologue_handoff`]:
    /// instead of blindly raising the bit on scene entry, the engine walks
    /// the scene MAN's partition-2 records (the cutscene timelines) for a
    /// `GFLAG_SET` of [`PROLOGUE_HANDOFF_BIT`] via
    /// [`crate::man_field_scripts::walk_partition_gflag_sites`] and arms only
    /// when it is present - so a cutscene scene that never issues that write
    /// can never produce a false hand-off. Returns `true` when it armed.
    ///
    /// The engine doesn't yet tick `opdeene`'s partition-2 cutscene records
    /// frame-by-frame (the camera + actor `MoveTo`s that precede the flag
    /// write), so this confirms the arming op exists in the real disc
    /// bytecode and sets exactly the bit the executed `GFLAG_SET` would.
    /// Pairs with [`Self::take_prologue_handoff`] for the confirm-press gate.
    // REF: FUN_801D1344
    pub fn arm_prologue_handoff_from_man(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> bool {
        let armed = crate::man_field_scripts::walk_partition_gflag_sites(man_file, man, 2)
            .iter()
            .any(|s| s.set && s.bit as u32 == PROLOGUE_HANDOFF_BIT);
        if armed {
            self.arm_prologue_handoff();
        }
        armed
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
        // The opening narration plays first: while its subtitle pages are on
        // screen the confirm press skips pages (see
        // [`Self::skip_cutscene_narration`]) and never reaches this gate, so
        // the hand-off can only fire once the narration has finished.
        if confirm
            && !self.cutscene_narration_active()
            && self.story_flags & PROLOGUE_HANDOFF_FLAG != 0
            && self.active_scene_label == legaia_asset::new_game::OPENING_CUTSCENE_SCENE
        {
            self.story_flags &= !PROLOGUE_HANDOFF_FLAG;
            // Mark the upcoming `town01` entry as the new-game opening so it
            // installs the opening cutscene timeline (which opens name entry at
            // its pinned op-`0x49`); a normal `town01` visit never sets this.
            self.entering_town01_opening = true;
            Some(legaia_asset::new_game::OPENING_SCENE)
        } else {
            None
        }
    }

    /// Load the opening-cutscene timeline record from the scene MAN as a
    /// spawned field-VM context, so its camera path + actor moves play and the
    /// closing `GFLAG_SET 26` fires by execution.
    ///
    /// Finds the partition-2 (cutscene-timeline) record that issues the
    /// [`PROLOGUE_HANDOFF_BIT`] `GFLAG_SET` via
    /// [`crate::man_field_scripts::walk_partition_gflag_sites`], resolves its
    /// named-record span with
    /// [`crate::man_field_scripts::partition_record_span`] (the partition-2
    /// header decode), and slices the record body from its `script_start` so
    /// relative jumps wrap against the record base (retail
    /// `buffer_base = script_start`). The spawned context begins at the
    /// record's first-opcode offset (`pc0`).
    ///
    /// Returns `true` when a timeline was installed. Returns `false` (no
    /// matching record, or span resolution failed) so the caller can fall back
    /// to the static hand-off arm ([`Self::arm_prologue_handoff_from_man`]).
    // REF: FUN_8003BDE0
    // REF: FUN_801D1344
    pub fn load_cutscene_timeline_from_man(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> bool {
        let Some(record_idx) =
            crate::man_field_scripts::walk_partition_gflag_sites(man_file, man, 2)
                .into_iter()
                .find(|s| s.set && s.bit as u32 == PROLOGUE_HANDOFF_BIT)
                .map(|s| s.record)
        else {
            return false;
        };
        if !self.install_cutscene_timeline_record(man_file, man, 2, record_idx, false) {
            return false;
        }
        // opdeene's terminal `GFLAG_SET 26` arms the `town01` hand-off; mark the
        // timeline so its completion / frame-cap safety net does so.
        if let Some(tl) = self.cutscene_timeline.take() {
            self.cutscene_timeline = Some(tl.arming_prologue_handoff());
        }
        true
    }

    /// Partition-2 record index of `town01`'s opening cutscene timeline (the
    /// establishing camera sweep + Vahn's walk-out + the name-entry handoff).
    /// A stable disc invariant; the record carries the name-entry STATE_RESUME
    /// pinned at body offset `0x02c6` (see `town01_opening_timeline_trace.rs`).
    pub const TOWN01_OPENING_TIMELINE_RECORD: usize = 3;

    /// Install `town01`'s opening cutscene timeline (the establishing shot +
    /// Vahn's scripted walk-out + the name-entry handoff) as a spawned field-VM
    /// context, and arm the name-entry handoff so the timeline's pinned op-`0x49`
    /// STATE_RESUME opens the *"Select your name."* overlay (rather than the
    /// host opening it blindly at the scene hand-off).
    ///
    /// Unlike [`Self::load_cutscene_timeline_from_man`] this does NOT arm a
    /// prologue scene hand-off - `town01` is the destination, and the record's
    /// terminal is the name-entry suspend, not a scene change. Returns `true`
    /// when installed.
    // REF: FUN_8003BDE0
    pub fn install_town01_opening_timeline(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> bool {
        if !self.install_cutscene_timeline_record(
            man_file,
            man,
            2,
            Self::TOWN01_OPENING_TIMELINE_RECORD,
            false,
        ) {
            return false;
        }
        self.prologue_naming_pending = true;
        self.prologue_naming_armed = false;
        true
    }

    /// Install a specific partition / record as a spawned cutscene-timeline
    /// context. The general core behind [`Self::load_cutscene_timeline_from_man`]
    /// (which locates `opdeene`'s `GFLAG_SET 26` record first) and the
    /// town-opening op-stream trace harness (which installs `town01`'s opening
    /// timeline record by index).
    ///
    /// Resolves the record's `(script_start, pc0, body_len)` span, slices the
    /// body from `script_start` (so relative jumps wrap against the record
    /// base), NOP-fills the inline narration spans (they are data the separate
    /// [`crate::cutscene_narration::CutsceneNarration`] presenter consumes, not
    /// field-VM opcodes - overwriting each with the 1-byte NOP `0x21` is
    /// offset-preserving so the camera / move / flag ops keep their offsets),
    /// and installs the timeline with `trace` controlling op-stream recording.
    ///
    /// Returns `true` when a timeline was installed; `false` when the span can't
    /// be resolved.
    // REF: FUN_8003BDE0
    pub fn install_cutscene_timeline_record(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        partition: usize,
        record_idx: usize,
        trace: bool,
    ) -> bool {
        let Some((script_start, pc0, body_len)) =
            crate::man_field_scripts::partition_record_span(man_file, man, partition, record_idx)
        else {
            return false;
        };
        let Some(body) = man.get(script_start..script_start + body_len) else {
            return false;
        };
        let mut body = body.to_vec();
        for block in legaia_asset::cutscene_text::parse_narration(&body) {
            let (start, end) = block.byte_span();
            let end = end.min(body.len());
            if start < end {
                for b in &mut body[start..end] {
                    *b = 0x21;
                }
            }
        }
        let mut tl = crate::cutscene_timeline::CutsceneTimeline::new(body, pc0);
        if trace {
            tl = tl.with_trace();
        }
        self.cutscene_timeline = Some(tl);
        true
    }

    /// `true` while the opening-cutscene timeline is still executing (installed
    /// and not yet complete). Diagnostics / tests read this; the hand-off gate
    /// itself keys off the scratchpad flag the timeline sets, not this.
    pub fn cutscene_timeline_active(&self) -> bool {
        self.cutscene_timeline
            .as_ref()
            .is_some_and(|t| !t.is_done())
    }

    /// Step the opening-cutscene timeline one frame.
    ///
    /// Runs the spawned cutscene context ([`crate::cutscene_timeline`]) through
    /// the field VM until it yields, waits, or completes - mirroring retail's
    /// run-until-`YIELD`-per-frame dispatch. Camera Configure (`0x45`) and
    /// actor MoveTo (`0x23`) ops emit the same [`crate::field_events::FieldEvent`]s
    /// the runtime camera folds in; the closing `GFLAG_SET 26` writes the
    /// hand-off bit through the same host path the main field VM uses, so the
    /// `town01` hand-off arms by execution.
    ///
    /// Bounded two ways so real disc bytecode can never hang the tick or stall
    /// the prologue:
    /// - a per-frame step budget caps a non-yielding loop;
    /// - a frame cap forces completion if the timeline never reaches its
    ///   closing op (e.g. it hits an op this port cannot advance past); for the
    ///   `opdeene` prologue ([`crate::cutscene_timeline::CutsceneTimeline::arms_prologue_handoff`])
    ///   the hand-off is then armed statically as a safety net.
    ///
    /// The `town01` opening timeline parks on op-`0x49` STATE_RESUME to open the
    /// name-entry overlay (via the op-49 host hooks); while that overlay is up
    /// the timeline is frozen (no step, no frame-cap progress) so the cutscene
    /// stays suspended exactly as retail's STATE_RESUME does.
    ///
    /// No-op when no timeline is installed or it has already completed.
    // REF: FUN_8003BDE0
    pub fn step_cutscene_timeline(&mut self) {
        let Some(mut tl) = self.cutscene_timeline.take() else {
            return;
        };
        if tl.done {
            self.cutscene_timeline = Some(tl);
            return;
        }
        // Freeze the timeline while the name-entry overlay it spawned is open:
        // its op-`0x49` STATE_RESUME is suspended until the player commits a
        // name, so neither the VM nor the frame cap advances meanwhile.
        if self.name_entry_active() {
            self.cutscene_timeline = Some(tl);
            return;
        }
        tl.frames = tl.frames.saturating_add(1);
        self.in_cutscene_timeline = true;
        {
            let mut host = FieldHostImpl { world: self };
            let mut budget = CUTSCENE_TIMELINE_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = tl.pc;
                let opcode_byte = tl.bytecode.get(pc).copied().unwrap_or(0);
                let result = vm::field::step(&mut host, &mut tl.ctx, &tl.bytecode, pc);
                let (mut next_pc, kind, mut stop) = match result {
                    FieldStepResult::Advance { next_pc } => (
                        next_pc,
                        crate::cutscene_timeline::TraceResult::Advance,
                        false,
                    ),
                    FieldStepResult::Yield { resume_pc } => (
                        resume_pc,
                        crate::cutscene_timeline::TraceResult::Yield,
                        true,
                    ),
                    // WAIT_FRAMES and conditional holds return `Halt` at the
                    // same PC: end the frame and resume there next tick.
                    FieldStepResult::Halt { final_pc } => {
                        (final_pc, crate::cutscene_timeline::TraceResult::Halt, true)
                    }
                    // An op this port can't advance past: stop and let the
                    // safety net below arm the hand-off.
                    FieldStepResult::Pending { pc, .. } => {
                        (pc, crate::cutscene_timeline::TraceResult::Pending, true)
                    }
                    FieldStepResult::Unknown { pc, .. } => {
                        (pc, crate::cutscene_timeline::TraceResult::Unknown, true)
                    }
                };
                // Step past the timeline's conditional-wait parks. Retail Halts
                // at PC on a handshake the engine doesn't model - a flag a
                // spawned sub-context sets (`0x2D`/`0x30` flag-test, `0x4C`
                // nibble-C `script_alloc` / globals-gate) - so advancing by the
                // op's encoded width (these read one operand byte,
                // `header_size + 1`) keeps the timeline flowing toward its
                // camera / move / STATE_RESUME ops. Two parks are kept:
                // `0x4A` WAIT_FRAMES (a real timed wait that plays out over
                // frames via the wait accumulator) and `0x49` STATE_RESUME (the
                // name-entry suspend, driven by the op-49 host hooks).
                let op = opcode_byte & 0x7F;
                if matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && op != 0x4A
                    && op != 0x49
                {
                    let header_size = if opcode_byte & 0x80 != 0 { 2 } else { 1 };
                    next_pc = pc + header_size + 1;
                    stop = false;
                }
                if tl.trace_enabled {
                    tl.trace.push(crate::cutscene_timeline::TraceEntry {
                        pc,
                        opcode_byte,
                        opcode: opcode_byte & 0x7F,
                        next_pc,
                        result: kind,
                    });
                }
                tl.pc = next_pc;
                if matches!(
                    kind,
                    crate::cutscene_timeline::TraceResult::Pending
                        | crate::cutscene_timeline::TraceResult::Unknown
                ) {
                    tl.done = true;
                }
                if stop {
                    break;
                }
            }
        }
        self.in_cutscene_timeline = false;
        // Frame cap: real disc bytecode must never hang the tick.
        if tl.frames >= CUTSCENE_TIMELINE_MAX_FRAMES {
            tl.done = true;
        }
        if tl.arms_prologue_handoff {
            // opdeene prologue: the record ends with `GFLAG_SET 26`. Complete on
            // that bit; if execution can't reach it within the cap, arm the
            // hand-off statically as a safety net so the prologue can't stall.
            if self.story_flags & PROLOGUE_HANDOFF_FLAG != 0 {
                tl.done = true;
            }
            if tl.done && self.story_flags & PROLOGUE_HANDOFF_FLAG == 0 {
                self.arm_prologue_handoff();
            }
            self.cutscene_timeline = Some(tl);
        } else if tl.done {
            // town01 opening timeline finished (or capped): drop it so the view
            // reverts from the cutscene camera to normal field gameplay.
            self.cutscene_timeline = None;
        } else {
            self.cutscene_timeline = Some(tl);
        }
    }

    /// Begin running an inline interaction script through the field VM (the
    /// faithful dialogue path — see [`crate::inline_dialogue`]). `inline` is the
    /// actor's interaction-script bytes (e.g. [`DialogRequest::inline`]), which
    /// begin at the first `0x1F` text segment. Replaces any running script.
    pub fn start_inline_dialogue(&mut self, inline: Vec<u8>) {
        self.inline_dialogue = Some(crate::inline_dialogue::InlineDialogue::from_inline(inline));
    }

    /// Start the inline-script runner on a full interaction record, executing the
    /// prologue from `entry_pc` (the record's `script_pc0`) before the first text
    /// segment at `first_segment`. The prologue's `SysFlag.Test`/`JmpRel` chain
    /// selects which segment the box opens at per story state; if it can't reach a
    /// segment the runner falls back to `first_segment`. See
    /// [`crate::inline_dialogue::InlineDialogue::with_prologue`].
    pub fn start_inline_dialogue_with_prologue(
        &mut self,
        body: Vec<u8>,
        entry_pc: usize,
        first_segment: usize,
    ) {
        self.inline_dialogue = Some(crate::inline_dialogue::InlineDialogue::with_prologue(
            std::sync::Arc::new(body),
            entry_pc,
            first_segment,
        ));
    }

    /// Advance the running inline interaction script one tick. Between text
    /// boxes the field VM executes the control bytecode (prologue story-flag
    /// tests, `SET`/`CLEAR` flag ops, scene changes) through the World host; at
    /// each `0x1F` segment it opens / ticks a dialog box. `confirm` dismisses the
    /// current box, or commits a menu choice — applying that option's relative
    /// jump (`FUN_80038050`) and handing the branch to the VM so its side
    /// effects run before the reply. `up`/`down` move a menu cursor. No-op when
    /// no inline dialogue is running.
    // PORT: FUN_80039B7C
    // REF: FUN_80038050 (the option-jump apply is delegated to OwnedDialogPanel::confirm_menu)
    // REF: FUN_8003CF7C (the inline fast-forward loop below subsumes retail's
    //      run-to-next-text helper: tick the field VM until `byte & 0x7F < 0x20`)
    pub fn step_inline_dialogue(&mut self, confirm: bool, up: bool, down: bool) {
        use crate::inline_dialogue::INLINE_DIALOGUE_STEP_BUDGET;
        let Some(mut id) = self.inline_dialogue.take() else {
            return;
        };
        if id.done {
            self.inline_dialogue = Some(id);
            return;
        }

        // A box is open: tick the typewriter + route input.
        if let Some(panel) = id.panel.as_mut() {
            if panel.menu_active() {
                if up {
                    panel.move_picker_cursor(-1);
                }
                if down {
                    panel.move_picker_cursor(1);
                }
            }
            panel.tick();
            if confirm {
                if panel.menu_active() {
                    // Commit the choice: apply the option's relative jump and
                    // resume the VM at the branch handler (its flag-sets /
                    // scene-change run before the reply box).
                    let choice = panel.picker_cursor();
                    let target = panel.picker().and_then(|pk| pk.jump_target(choice));
                    id.last_choice = Some(choice);
                    match target {
                        Some(t) => id.pc = t,
                        None => id.done = true,
                    }
                    id.panel = None;
                } else if panel.is_waiting_for_input() || panel.is_done() {
                    // Plain box dismissed: resume the VM just past this segment.
                    id.pc = panel.pc;
                    id.panel = None;
                }
            }
            self.inline_dialogue = Some(id);
            return;
        }

        // No box open: step the VM until the next text segment or an end.
        let mut host = FieldHostImpl { world: self };
        let mut budget = INLINE_DIALOGUE_STEP_BUDGET;
        while budget > 0 {
            budget -= 1;
            let b = id.bytecode.get(id.pc).copied().unwrap_or(0);
            // Retail SM transition test: a byte with `& 0x7F < 0x20` is a text
            // lead (`0x1F`) or a terminator (`0x00..0x1E`), not an opcode.
            if b & 0x7F < 0x20 {
                if b == 0x1F {
                    // Reached a text segment. A prologue (if any) selected it, so
                    // retire the fallback and open the box here.
                    id.fallback_segment_pc = None;
                    id.panel = Some(crate::dialog::OwnedDialogPanel::at_segment(
                        std::sync::Arc::clone(&id.bytecode),
                        id.pc,
                    ));
                    break;
                }
                // A non-`0x1F` terminator before any box opened: if a prologue
                // fallback is pending, resume at the first segment (so the box
                // still shows); otherwise the conversation ends.
                if let Some(fb) = id.fallback_segment_pc.take() {
                    id.pc = fb;
                    continue;
                }
                id.done = true;
                break;
            }
            match vm::field::step(&mut host, &mut id.ctx, &id.bytecode, id.pc) {
                FieldStepResult::Advance { next_pc } => id.pc = next_pc,
                FieldStepResult::Yield { resume_pc } => id.pc = resume_pc,
                // A wait/hold, an unhandled op, or an end: stop. (Unlike the
                // cutscene timeline the runner does not force-advance past a
                // Halt — an inline interaction script that can't proceed ends.)
                // While a prologue is still running (no box opened yet), a halt
                // means the prologue can't proceed — fall back to the first
                // segment so the dialogue is never worse than the truncated path.
                FieldStepResult::Halt { .. }
                | FieldStepResult::Pending { .. }
                | FieldStepResult::Unknown { .. } => {
                    if let Some(fb) = id.fallback_segment_pc.take() {
                        id.pc = fb;
                        continue;
                    }
                    id.done = true;
                    break;
                }
            }
        }
        self.inline_dialogue = Some(id);
    }

    /// Live-loop bridge for the inline-script runner: when [`Self::use_vm_dialogue`]
    /// is set, this starts the runner the frame a field dialogue opens (from
    /// [`Self::current_dialog`]'s inline buffer), steps it from the current pad
    /// edges (Cross/Circle = confirm, Up/Down = menu cursor), and tears it down
    /// (clearing `current_dialog`) when the conversation ends. No-op when the
    /// flag is off, so the default simplified path is untouched.
    pub fn drive_inline_dialogue(&mut self) {
        if !self.use_vm_dialogue {
            return;
        }
        // Start the runner the frame a dialogue request appears. When the opened
        // NPC carries a prologue record, run it from the entry PC so the
        // interaction prologue (segment selection) executes; otherwise start at
        // the first segment from the request's inline buffer.
        if self.inline_dialogue.is_none() {
            if let Some(prologue) = self.active_inline_prologue.take() {
                self.inline_dialogue = Some(crate::inline_dialogue::InlineDialogue::with_prologue(
                    std::sync::Arc::new(prologue.body),
                    prologue.entry_pc,
                    prologue.first_segment,
                ));
            } else if let Some(req) = self.current_dialog.as_ref() {
                if !req.inline.is_empty() {
                    self.start_inline_dialogue(req.inline.clone());
                } else {
                    return;
                }
            } else {
                return;
            }
        }
        let confirm = self.input.just_pressed(input::PadButton::Cross)
            || self.input.just_pressed(input::PadButton::Circle);
        let up = self.input.just_pressed(input::PadButton::Up);
        let down = self.input.just_pressed(input::PadButton::Down);
        self.step_inline_dialogue(confirm, up, down);
        if self.inline_dialogue.as_ref().is_some_and(|d| d.is_done()) {
            self.inline_dialogue = None;
            self.current_dialog = None;
            self.pending_field_events
                .push(crate::field_events::FieldEvent::DialogDismissed);
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

    /// Drain the battle sound cues queued this frame (the art-strike `HitCue`
    /// sounds [`Self::fold_battle_event`] resolves). The host plays each through
    /// its `SfxBank::play_one_shot` at the cue's `timing_frames` delay; nothing
    /// here mutates gameplay state. Returns them in resolve order.
    pub fn drain_battle_sfx_cues(&mut self) -> Vec<BattleSfxCue> {
        std::mem::take(&mut self.battle_sfx_cues)
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
                actor_slot,
                target_slot,
                outcome,
                ..
            } => {
                // Surface the strike's sound cues for the host's SFX bank (these
                // were previously dropped). Hit-effect-only cues (kind 0x4C)
                // carry no sound, so only the `is_sound` cues are queued.
                for cue in &outcome.cues {
                    if cue.is_sound() {
                        self.battle_sfx_cues.push(BattleSfxCue {
                            kind: cue.kind,
                            timing_frames: cue.timing_frames,
                            actor_slot: *actor_slot,
                            target_slot: *target_slot,
                        });
                    }
                }
                // A petrified target (Stone) can't be damaged - the strike is
                // fully absorbed (and so doesn't wake a Sleep/Numb either).
                let petrified = self.actor_is_petrified(*target_slot);
                if let Some(target) = self.actors.get_mut(*target_slot as usize) {
                    if let Some(dmg) = outcome.damage
                        && !petrified
                    {
                        target.battle.hp = target.battle.hp.saturating_sub(dmg);
                        // Damage clears Sleep / Numb on the target (matches
                        // retail - the unit wakes when hit).
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
            // Cast band (SM path): the per-actor action SM fires
            // `spell_anim_trigger` at `MagicPreCastWait`. For a player Seru-magic
            // id, request the summon spawn (the host resolves the overlay PROT
            // entry + spawns). Origin = the caster party slot's position when
            // available, else a default forward cast point.
            BattleEvent::BattleEnd {
                cause: BattleEndCause::Escaped,
            } => {
                // Retail escape teardown (battle SM state 0x66): stage the
                // 0x40-frame black→white screen fade the SM spawns through
                // the fade primitive before the battle unloads.
                self.screen_fade = Some(crate::fade::FadeState::load(
                    &crate::fade::escape_fade_template(),
                ));
                // A petrified member returns to normal when the party
                // escapes (retail's run band floors every downed party
                // slot's HP at 1 on a successful escape; the tracker-level
                // Stone clear is the engine's model of that restore).
                self.status_effects.cure_stone_on_escape();
                None
            }
            BattleEvent::SpellAnimTrigger {
                party_slot,
                spell_id,
            } => {
                let origin = self
                    .actors
                    .get(*party_slot as usize)
                    .map(|a| {
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ]
                    })
                    .unwrap_or([0, -300, -645]);
                self.request_summon_spawn(*spell_id, origin);
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
                // A DoT kill is a death: pair HP==0 with liveness=0 like every
                // other damage entry point (fold_spell_outcome / apply_battle_art
                // / apply_basic_attack). Otherwise the corpse stays "alive" for
                // the liveness-keyed wipe checks + target/turn resolvers.
                if actor.battle.hp == 0 {
                    actor.battle.liveness = 0;
                }
            }
        }
    }

    /// Set the item catalog the battle / field menu consults for item
    /// actions. Replaces any prior catalog. Engines populate this at
    /// boot time (typically from the vanilla catalog).
    ///
    /// When a real on-disc item-effect table has been installed via
    /// [`Self::set_item_effects`], its field/battle usability flags are applied
    /// onto the new catalog so the item-menu gating matches retail.
    pub fn set_item_catalog(&mut self, catalog: crate::items::ItemCatalog) {
        let mut catalog = catalog;
        if let Some(table) = &self.item_effects {
            catalog.apply_effect_flags(table);
            catalog.apply_stat_items(table);
            catalog.apply_buff_items(table);
            catalog.apply_action_gauge_items(table);
        }
        self.item_catalog = catalog;
    }

    /// Install the real on-disc item-effect descriptor table. Subsequent
    /// [`Self::set_item_catalog`] calls apply its usability flags; this also
    /// re-applies them to the catalog already installed.
    pub fn set_item_effects(&mut self, table: legaia_asset::item_effect::ItemEffectTable) {
        self.item_catalog.apply_effect_flags(&table);
        self.item_catalog.apply_stat_items(&table);
        self.item_catalog.apply_buff_items(&table);
        self.item_catalog.apply_action_gauge_items(&table);
        self.item_effects = Some(table);
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
        crate::battle_arts::character_for_slot(slot)
    }

    /// Build the Arts submenu rows for `caster` from their saved chains. For
    /// each chain, the longest staged art record whose command string the
    /// chain ends with ([`crate::battle_arts::chain_matches_record`]) supplies
    /// the real power profile; chains with no matching record fall back to a
    /// synthetic profile derived from the directional commands.
    fn build_battle_arts_rows(&self, caster: u8) -> Vec<crate::battle_arts::ArtRow> {
        use crate::battle_arts::{
            ArtRow, chain_matches_record, miracle_for_chain, power_from_record, super_for_chain,
            synthetic_power,
        };
        let character = self.caster_character(caster);
        self.saved_chains
            .iter()
            .filter(|c| c.char_slot == caster)
            .map(|c| {
                // Miracle Arts win over a plain art-record match: a chain whose
                // directional string is the caster's Miracle Art replaces the
                // whole queue with the finisher sequence (the retail order:
                // Miracle replacement runs before any tail Super expansion).
                if let Some(miracle) = miracle_for_chain(character, &c.sequence) {
                    let (power, enemy_effect) = self.miracle_strike_profile(character, miracle);
                    return ArtRow {
                        name: c.name.clone(),
                        power,
                        enemy_effect,
                        miracle: Some(miracle.name),
                        super_art: None,
                    };
                }
                // Super Arts next (after Miracle, matching the retail order):
                // recognize the chain's named-art sequence from the caster's
                // art catalog and tail-match it against the caster's Super art
                // sequences (connectors abstracted — see `super_for_chain`).
                let caster_records = || {
                    self.art_records
                        .iter()
                        .filter(|((ch, _), _)| *ch == character)
                        .map(|(_, rec)| rec)
                };
                if let Some(sa) = super_for_chain(character, &c.sequence, caster_records()) {
                    let (power, enemy_effect) = self.super_strike_profile(character, sa);
                    return ArtRow {
                        name: c.name.clone(),
                        power,
                        enemy_effect,
                        miracle: None,
                        super_art: Some(sa.name),
                    };
                }
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
                            miracle: None,
                            super_art: None,
                        }
                    }
                    None => ArtRow {
                        name: c.name.clone(),
                        power: synthetic_power(&c.sequence),
                        enemy_effect: legaia_art::EnemyEffect::None,
                        miracle: None,
                        super_art: None,
                    },
                }
            })
            .collect()
    }

    /// Resolve a Miracle Art's per-strike power profile from its
    /// finisher-replacement queue. Runs the canonical command resolution
    /// ([`legaia_engine_vm::battle_action::resolve_action_queue`]), which
    /// replaces the directional input with the Miracle's component-art queue,
    /// then turns each art constant in that queue into strikes:
    ///
    /// - if the `(character, art)` record is staged ([`Self::set_art_record`]),
    ///   the art contributes its real damage power bytes + status effect;
    /// - otherwise it contributes one tier-0 (`x12`) synthetic strike, the same
    ///   graceful-degradation profile [`crate::battle_arts::synthetic_power`]
    ///   uses when no disc art data is loaded.
    ///
    /// The first staged component art's status effect is adopted for the whole
    /// finisher. Result is clamped to [`crate::battle_arts::MAX_ART_HITS`] and
    /// floored at one strike.
    fn miracle_strike_profile(
        &self,
        character: legaia_art::Character,
        miracle: &legaia_art::MiracleArt,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        let queue =
            legaia_engine_vm::battle_action::resolve_action_queue(character, miracle.commands, &[]);
        self.art_actions_strike_profile(character, queue.actions().iter().copied())
    }

    /// Resolve a **Super Art**'s per-strike power profile from its
    /// finisher-replacement queue ([`legaia_art::SuperArt::replace`]). The
    /// replacement keeps the leading component arts and ends in the Super
    /// finisher constant(s) (e.g. Tri-Somersault → `… 1A 2B 2B 2B`), so each art
    /// constant in it contributes a strike via the shared resolver
    /// ([`Self::art_actions_strike_profile`]) — real [`ArtRecord`] power where the
    /// `(character, art)` record is staged, else a tier-0 synthetic strike.
    ///
    /// [`ArtRecord`]: legaia_art::ArtRecord
    fn super_strike_profile(
        &self,
        character: legaia_art::Character,
        sa: &legaia_art::SuperArt,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        let actions = sa
            .replace
            .iter()
            .filter_map(|&b| legaia_art::ActionConstant::from_byte(b));
        self.art_actions_strike_profile(character, actions)
    }

    /// Turn a queue of [`ActionConstant`](legaia_art::ActionConstant)s into a
    /// per-strike power profile: each art constant resolves to its staged
    /// [`ArtRecord`](legaia_art::ArtRecord) power bytes + status effect, or one
    /// tier-0 (`x12`) synthetic strike when that art's record isn't loaded (the
    /// graceful-degradation fallback the no-disc-data path uses). The first
    /// staged status effect is adopted for the whole finisher; the result is
    /// clamped to [`crate::battle_arts::MAX_ART_HITS`] and floored at one strike.
    /// Shared by the Miracle and Super finisher resolvers.
    fn art_actions_strike_profile(
        &self,
        character: legaia_art::Character,
        actions: impl Iterator<Item = legaia_art::ActionConstant>,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        use crate::battle_arts::MAX_ART_HITS;
        use legaia_art::power::PowerByte;
        // Synthetic UDF x12 - the tier-0 high strike a component art with no
        // staged record degrades to.
        const SYNTH_UDF_X12: u8 = 0x16;

        let mut power: Vec<PowerByte> = Vec::new();
        let mut enemy_effect = legaia_art::EnemyEffect::None;
        for action in actions {
            if !action.is_art() {
                continue;
            }
            match self.art_records.get(&(character, action)) {
                Some(rec) => {
                    let (mut bytes, effect) = crate::battle_arts::power_from_record(rec);
                    if enemy_effect == legaia_art::EnemyEffect::None {
                        enemy_effect = effect;
                    }
                    power.append(&mut bytes);
                }
                None => power.push(PowerByte::from_byte(SYNTH_UDF_X12)),
            }
            if power.len() >= MAX_ART_HITS as usize {
                break;
            }
        }
        power.truncate(MAX_ART_HITS as usize);
        if power.is_empty() {
            power.push(PowerByte::from_byte(SYNTH_UDF_X12));
        }
        (power, enemy_effect)
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
        // Permanent multi-stat boost (class-6 Water line): resolved from the
        // on-disc effect table (which this path needs anyway), not the pure
        // table-less `apply_effect`.
        if let crate::items::ItemEffect::StatUp = entry.effect {
            return self.apply_stat_up_item(item_id, target_slot);
        }
        // One-battle stat buff (class-7 Elixir): ramps the target's battle-actor
        // stat scalars by ×6/5, resolved from the on-disc table.
        if let crate::items::ItemEffect::BattleBuff = entry.effect {
            return self.apply_buff_item(item_id, target_slot);
        }
        // One-battle action-gauge extension (class-5 Fury Boost): extends the
        // target's AP gauge for the rest of the battle.
        if let crate::items::ItemEffect::ActionGauge = entry.effect {
            return self.apply_fury_boost_item(target_slot);
        }
        let idx = target_slot as usize;
        // BattleActor holds `mp` but not `max_mp`; engines that wire the
        // character record into the actor populate it via a sibling field.
        // For the snapshot we use the character_max_mp accessor (defaults
        // to `mp` itself when not separately tracked, which gives a
        // conservative "MP already capped" reading).
        let status_mask = self
            .status_effects
            .statuses(target_slot)
            .iter()
            .fold(0u8, |m, s| m | crate::items::status_bit(s.kind));
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
                status_mask,
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
            crate::items::ItemOutcome::StatRaised { target, delta } => {
                // Permanent stat-up consumable (Power Tonic, Vital Tonic, ...):
                // raise the persistent roster record and refresh the live
                // derived values so the gain shows immediately and survives a
                // save. These items are field-only.
                self.apply_stat_raise(idx, target, delta);
            }
            _ => {}
        }
        outcome
    }

    /// Apply a one-battle action-gauge extension
    /// ([`crate::items::ItemEffect::ActionGauge`], Fury Boost) to party
    /// `target_slot`. Retail sets the actor `+0x1F9` flag, and the action-SM
    /// gauge-build phase then sizes the gauge as `gauge_stat * 7 / 5 + 8`
    /// (clamped) instead of the base length. The engine models the AP gauge as a
    /// discrete per-turn budget rather than a continuous pixel length, so it
    /// approximates the extension by raising the slot's [`ApGauge::base_ap`] by
    /// the retail `×7/5` ratio (the `+8` pixel term and the gauge-stat source are
    /// not representable here). The boost persists for the battle (`base_ap`
    /// survives [`ApGauge::reset_for_turn`]) and the live gauge gains the delta
    /// immediately; it is reverted at battle end ([`Self::finish_battle`]).
    /// Idempotent within a battle (a second Fury Boost re-sets the already-set
    /// flag, no extra gauge). Returns [`crate::items::ItemOutcome::NoEffect`] for
    /// a non-party slot.
    fn apply_fury_boost_item(&mut self, target_slot: u8) -> crate::items::ItemOutcome {
        let idx = target_slot as usize;
        if idx >= self.ap_gauges.len() {
            return crate::items::ItemOutcome::NoEffect;
        }
        // Already boosted this battle: retail re-sets the same flag, no compound.
        if self.fury_boost[idx].is_none() {
            let gauge = &mut self.ap_gauges[idx];
            let before = gauge.base_ap;
            let after = ((before as u16 * 7) / 5) as u8;
            let delta = after.saturating_sub(before);
            gauge.set_base_ap(after);
            // Extend the live gauge so the longer budget is usable this turn.
            gauge.current_ap = gauge.current_ap.saturating_add(delta);
            self.fury_boost[idx] = Some(delta);
        }
        crate::items::ItemOutcome::ActionGaugeExtended
    }

    /// Apply a one-battle stat buff ([`crate::items::ItemEffect::BattleBuff`],
    /// the class-7 Elixirs) to `target_slot`. The buffed stats are resolved from
    /// the installed on-disc item-effect table; each is ramped ×6/5 for the rest
    /// of the battle through the shared buff path ([`Self::apply_battle_buff`],
    /// the same machinery as buff *spells*) — so it reuses the precise
    /// revert-on-expiry / revert-at-battle-end bookkeeping. `Defense` ramps the
    /// single defence scalar; `Agility` maps to the accuracy/evasion proxy (no
    /// live scalar yet, so it only runs the turn timer, like a buff spell on
    /// Speed). Returns [`crate::items::ItemOutcome::NoEffect`] when no table is
    /// installed or the id isn't a one-battle buff.
    fn apply_buff_item(&mut self, item_id: u8, target_slot: u8) -> crate::items::ItemOutcome {
        use crate::spells::BuffStat;
        use legaia_asset::item_effect::{StatItemEffect, StatTarget};
        // "One battle": a turn count large enough to outlast the encounter; the
        // buff is reverted wholesale at battle end (`finish_battle`).
        const ONE_BATTLE: u8 = u8::MAX;
        // Positive magnitude selects the retail ×6/5 multiplicative ramp in
        // `apply_battle_buff` (the value itself is only a sign hint there).
        const BUFF_SIGN: i16 = 1;

        let resolved = self
            .item_effects
            .as_ref()
            .and_then(|t| t.stat_effect(item_id));
        let Some(StatItemEffect::BuffOneBattle(stats)) = resolved else {
            return crate::items::ItemOutcome::NoEffect;
        };
        let mut count = 0u8;
        for stat in stats {
            let buff_stat = match stat {
                StatTarget::Attack => BuffStat::Attack,
                StatTarget::Defense => BuffStat::Defense,
                StatTarget::Speed => BuffStat::Speed,
                // AGL drives accuracy + evasion (both proxy it); one entry
                // models the AGL buff.
                StatTarget::Agility => BuffStat::Accuracy,
                // The permanent-only stats never appear in a class-7 buff; skip
                // defensively rather than fabricate a battle scalar for them.
                StatTarget::MaxHp | StatTarget::MaxMp | StatTarget::Intelligence => continue,
            };
            self.apply_battle_buff(target_slot, buff_stat, BUFF_SIGN, ONE_BATTLE);
            count = count.saturating_add(1);
        }
        if count == 0 {
            crate::items::ItemOutcome::NoEffect
        } else {
            crate::items::ItemOutcome::Buffed { count }
        }
    }

    /// Apply a permanent multi-stat boost ([`crate::items::ItemEffect::StatUp`],
    /// the class-6 *Water* line) to party slot `target_slot`. The per-stat
    /// changes are resolved from the installed on-disc item-effect table via the
    /// item's `(class, tier)` descriptor (`legaia_asset::item_effect`), then each
    /// is applied through the shared [`Self::apply_stat_raise`] persistent path.
    /// `Defense` raises both defence facets (DEF-up + DEF-down), matching the
    /// retail apply handler. Returns [`crate::items::ItemOutcome::NoEffect`] when
    /// no table is installed or the id isn't a permanent stat-up.
    fn apply_stat_up_item(&mut self, item_id: u8, target_slot: u8) -> crate::items::ItemOutcome {
        use crate::items::StatBoostTarget as T;
        use legaia_asset::item_effect::{StatItemEffect, StatTarget};
        let idx = target_slot as usize;
        // Resolve to an owned value so the immutable table borrow is dropped
        // before the mutable `apply_stat_raise` calls.
        let resolved = self
            .item_effects
            .as_ref()
            .and_then(|t| t.stat_effect(item_id));
        let Some(StatItemEffect::Permanent(changes)) = resolved else {
            return crate::items::ItemOutcome::NoEffect;
        };
        let mut raises: Vec<(T, u16)> = Vec::with_capacity(changes.len() + 1);
        for ch in &changes {
            match ch.stat {
                StatTarget::MaxHp => raises.push((T::HpMax, ch.delta)),
                StatTarget::MaxMp => raises.push((T::MpMax, ch.delta)),
                StatTarget::Attack => raises.push((T::Attack, ch.delta)),
                // The retail handler writes both defence facets for one item.
                StatTarget::Defense => {
                    raises.push((T::Udf, ch.delta));
                    raises.push((T::Ldf, ch.delta));
                }
                StatTarget::Speed => raises.push((T::Speed, ch.delta)),
                StatTarget::Intelligence => raises.push((T::Intelligence, ch.delta)),
                StatTarget::Agility => raises.push((T::Agility, ch.delta)),
            }
        }
        let count = raises.len().min(u8::MAX as usize) as u8;
        for (target, delta) in raises {
            self.apply_stat_raise(idx, target, delta);
        }
        if count == 0 {
            crate::items::ItemOutcome::NoEffect
        } else {
            crate::items::ItemOutcome::StatsRaised { count }
        }
    }

    /// Apply a permanent [`crate::items::ItemOutcome::StatRaised`] to party
    /// slot `idx`: mutate the persistent character record and re-derive the
    /// live battle stats. HP/MP-max raises bump the live actor's caps (and
    /// current values) too; combat-stat raises land in the `+0x110` live-stat
    /// block that [`Self::seed_party_battle_stats`] reads.
    ///
    /// The exact retail cap / refill rules for stat-up consumables are not
    /// byte-pinned (the items are field-only and absent from the captured
    /// battle traces), so the engine uses self-consistent rules: combat stats
    /// cap at the record's per-stat cap constant (fallback `999`), HP/MP max
    /// cap at `9999`, and a max raise refills the gained amount.
    fn apply_stat_raise(&mut self, idx: usize, target: crate::items::StatBoostTarget, delta: u16) {
        use crate::items::StatBoostTarget as T;
        const STAT_CAP_FALLBACK: u16 = 999;
        const HPMP_CAP: u16 = 9999;
        if self.roster.members.get(idx).is_none() {
            return;
        }
        match target {
            T::HpMax => {
                {
                    let rec = &mut self.roster.members[idx];
                    let mut hms = rec.hp_mp_sp();
                    hms.hp_max = hms.hp_max.saturating_add(delta).min(HPMP_CAP);
                    hms.hp_cur = hms.hp_cur.saturating_add(delta).min(hms.hp_max);
                    rec.set_hp_mp_sp(hms);
                    let mut rs = rec.record_stats();
                    rs.hp_max = rs.hp_max.saturating_add(delta).min(HPMP_CAP);
                    rec.set_record_stats(rs);
                }
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.max_hp = a.battle.max_hp.saturating_add(delta).min(HPMP_CAP);
                    a.battle.hp = a.battle.hp.saturating_add(delta).min(a.battle.max_hp);
                }
            }
            T::MpMax => {
                let new_max;
                {
                    let rec = &mut self.roster.members[idx];
                    let mut hms = rec.hp_mp_sp();
                    hms.mp_max = hms.mp_max.saturating_add(delta).min(HPMP_CAP);
                    hms.mp_cur = hms.mp_cur.saturating_add(delta).min(hms.mp_max);
                    rec.set_hp_mp_sp(hms);
                    let mut rs = rec.record_stats();
                    rs.mp_max = rs.mp_max.saturating_add(delta).min(HPMP_CAP);
                    rec.set_record_stats(rs);
                    new_max = hms.mp_max;
                }
                self.set_character_max_mp(idx as u8, new_max);
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.mp = a.battle.mp.saturating_add(delta).min(new_max);
                }
            }
            // Combat stats live in the +0x110 block the stat resolver reads.
            // Accuracy + Evasion both derive from AGL there, so both land on
            // AGL (matching `seed_party_battle_stats`), as does the AGL raise
            // itself; Speed and Intelligence have their own halfwords.
            T::Attack
            | T::Udf
            | T::Ldf
            | T::Accuracy
            | T::Evasion
            | T::Agility
            | T::Speed
            | T::Intelligence => {
                {
                    let rec = &mut self.roster.members[idx];
                    let cap = match rec.record_stats().cap_constant {
                        0 => STAT_CAP_FALLBACK,
                        c => c,
                    };
                    let mut ls = rec.live_stats();
                    let bump = |v: u16| v.saturating_add(delta).min(cap);
                    match target {
                        T::Attack => ls.atk = bump(ls.atk),
                        T::Udf => ls.udf = bump(ls.udf),
                        T::Ldf => ls.ldf = bump(ls.ldf),
                        T::Accuracy | T::Evasion | T::Agility => ls.agl = bump(ls.agl),
                        T::Speed => ls.spd = bump(ls.spd),
                        T::Intelligence => ls.int = bump(ls.int),
                        T::HpMax | T::MpMax => {}
                    }
                    rec.set_live_stats(ls);
                }
                self.seed_party_battle_stats();
            }
        }
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

    /// Distribute `xp_reward` (the summed enemy EXP) to the surviving party
    /// members after a `BattleEndCause::MonsterWipe`. Mirrors the retail split
    /// ([`vm::battle_formulas::victory_exp_per_member`], `FUN_8004E568`):
    ///
    /// - The summed reward is scaled by 3/4 (`v - (v >> 2)`), then **ceiling**-
    ///   divided among the surviving (HP > 0) members — not a floor-divide of
    ///   the raw sum.
    /// - Dead members (HP == 0) receive zero XP and are excluded from the divisor.
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
        // Retail scales the summed EXP by 3/4 then ceiling-divides among the
        // living members (`FUN_8004E568` `8004e568.txt:461`), NOT a plain
        // floor-divide of the raw sum.
        let per_member_xp =
            vm::battle_formulas::victory_exp_per_member(xp_reward, alive.len() as u32);
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

    /// Resolve the victory spoils for `formation` (the reward half of
    /// `FUN_8004E568`): accumulate each dead enemy's gold as `gold >> 1`,
    /// finalize it through the +25% bonus + halve
    /// ([`vm::battle_formulas::victory_gold_finalize`]) and add it to
    /// [`World::money`]; sum the enemy EXP and distribute it (scaled 3/4,
    /// ceiling-split) via [`World::apply_battle_xp`]. Returns the aggregated
    /// [`BattleRewards`] (`gold` is the **credited** amount, not the raw sum) so
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
        // Accumulated `gold >> 1` over dead enemies (the victory-gold accumulator
        // in `FUN_8004E568`); finalized below via the `>> 1` halve + optional
        // +25% bonus. NOT the raw record-gold sum.
        let mut gold_acc: u32 = 0;
        let mut drops: Vec<u8> = Vec::new();
        for slot in &formation.slots {
            let Some(def) = catalog.get(slot.monster_id) else {
                continue;
            };
            xp_total = xp_total.saturating_add(def.exp as u32);
            gold_acc =
                gold_acc.saturating_add(vm::battle_formulas::victory_gold_per_monster(def.gold));
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
        // The +25% gold bonus fires when a living party member carries bit
        // `0x10000` of the SECOND ability word (`FUN_8004E568` tests the u32
        // at record `+0xF8`): overall bit 48 of the `+0xF4` bitfield = byte 6,
        // mask 0x01 = accessory passive index 0x30 ("Gold Boost", the Golden
        // Book - see `docs/formats/accessory-passive-table.md`). "Living" =
        // post-battle battle HP > 0, the same set `apply_battle_xp` divides
        // EXP among.
        let party_count = self.party_count as usize;
        let more_gold = (0..party_count).any(|i| {
            self.actors.get(i).is_some_and(|a| a.battle.hp > 0)
                && self
                    .roster
                    .members
                    .get(i)
                    .is_some_and(|rec| rec.ability_bits()[6] & 0x01 != 0)
        });
        let gold_credited = vm::battle_formulas::victory_gold_finalize(gold_acc, more_gold);

        let level_ups = if xp_total > 0 {
            self.apply_battle_xp(xp_total)
        } else {
            Vec::new()
        };
        let new_money = (self.money as i64).saturating_add(gold_credited as i64);
        self.money = new_money.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        BattleRewards {
            xp: xp_total,
            gold: gold_credited,
            level_ups,
            drops,
        }
    }

    /// Resolve a **steal** attempt against `monster_id` using the per-monster
    /// steal table (the Evil God Icon mechanic). Rolls the monster's steal
    /// chance against the deterministic world RNG; on success the stolen item is
    /// added to [`Self::inventory`] and its id returned. Returns `None` when the
    /// monster has no steal (item `0` / chance `0`) or the roll misses.
    ///
    /// The steal item + chance live in a static `SCUS_942.54` table
    /// (`DAT_80077828`), **not** the monster record, so the caller passes the
    /// parsed [`legaia_asset::steal_table::StealTable`] (the engine reads the
    /// disc-resident data the randomizer edits). This is the steal counterpart
    /// to the drop grant in [`Self::apply_battle_loot`]: a percent roll
    /// (`rand % 100 < chance`) then the same `inventory` add. See
    /// `docs/formats/steal-table.md`.
    pub fn apply_steal(
        &mut self,
        monster_id: u16,
        steal_table: &legaia_asset::steal_table::StealTable,
    ) -> Option<u8> {
        let entry = steal_table.entry(monster_id).filter(|e| e.is_stealable())?;
        let roll = (self.next_rng() % 100) as u8;
        if roll < entry.chance_pct {
            let slot = self.inventory.entry(entry.item_id).or_insert(0);
            *slot = slot.saturating_add(1);
            Some(entry.item_id)
        } else {
            None
        }
    }

    /// Commit a shop **buy** transaction for the session's pending item: if the
    /// player can afford it, deduct the gold and add the item(s) to
    /// [`Self::inventory`], returning `(item_id, qty, gold_delta)` (the delta is
    /// negative). Returns `None` when the buy isn't valid (unaffordable, sell
    /// mode, no pending item — see [`crate::shop::ShopSession::try_buy`]).
    ///
    /// This is the engine's shop-purchase grant kernel, shared by the menu
    /// runtime's `ShopConfirm` commit and exercised directly by the shop /
    /// casino randomizer runtime oracles — the buy counterpart to
    /// [`Self::apply_steal`] / [`Self::apply_battle_loot`]. The item id sold is
    /// whatever the shop's stock holds, which for a town merchant is decoded
    /// straight from the scene's field-VM script (op `0x49`) the randomizer
    /// edits, so a patched shop id flows through here into the bag.
    pub fn buy_from_shop(&mut self, session: &crate::shop::ShopSession) -> Option<(u8, u8, i32)> {
        let (item_id, qty, delta) = session.try_buy(self.money)?;
        self.money = (self.money + delta).clamp(0, 9_999_999);
        let count = self.inventory.entry(item_id).or_insert(0);
        *count = count.saturating_add(qty);
        Some((item_id, qty, delta))
    }

    /// Build a [`crate::shop::ShopSession`] for the `idx`-th gold shop located in
    /// the active scene ([`Self::scene_shops`], decoded from the scene MAN +
    /// priced from the SCUS item table at scene entry). `None` when `idx` is out
    /// of range (no merchant, or the disc / item data was absent at boot, leaving
    /// the list empty).
    ///
    /// This is the bridge from the disc-sourced per-scene stock to the menu
    /// runtime: a host installs the returned session via
    /// [`crate::menu_runtime::MenuRuntime::open_shop`] when the player triggers
    /// the scene's merchant (field-VM op `0x49`).
    pub fn scene_shop_session(&self, idx: usize) -> Option<crate::shop::ShopSession> {
        let shop = self.scene_shops.get(idx)?;
        Some(crate::shop::ShopSession::new(shop.inventory.clone()))
    }

    /// Recognise + open a gold shop from a field-VM op-`0x49` sub-0 instruction
    /// (`instr` = the opcode byte onward: `[0x49][0x00][len][...][count][ids][name]`).
    ///
    /// The same strict, sellable-mask-gated record validation the shop catalog
    /// uses ([`legaia_asset::shop_stock::parse_record`]) rejects every non-shop
    /// op-0x49 sub-0 (inn / save prompts carry MES text, not a priced item
    /// list), so this only fires on a real merchant. Gated on
    /// [`Self::item_shop_data`] being installed - without prices there's no
    /// sellable mask (so a disc-free build can't false-positive) and no shop to
    /// price anyway. On a match it stages a priced [`crate::shop::ShopSession`]
    /// on [`Self::pending_field_shop`] and arms the op-0x49 gate; a no-op if a
    /// shop is already armed (single-open) or the record doesn't validate.
    ///
    /// Returns `true` when a shop was armed.
    pub fn try_arm_field_shop(&mut self, instr: &[u8]) -> bool {
        if self.field_shop_armed {
            return false;
        }
        let Some(data) = self.item_shop_data.as_ref() else {
            return false;
        };
        let mask = data.sellable_mask();
        let Some(rec) = legaia_asset::shop_stock::parse_record(instr, 0, Some(&mask)) else {
            return false;
        };
        let items = rec
            .id_offsets
            .iter()
            .filter_map(|&o| instr.get(o).copied())
            .map(|id| crate::shop::ShopItem {
                item_id: id,
                price: data.price(id) as u32,
            })
            .collect();
        let inv = crate::shop::ShopInventory::new(0, items);
        self.pending_field_shop = Some(crate::shop::ShopSession::new(inv));
        self.field_shop_armed = true;
        self.field_shop_open = true;
        true
    }

    /// Drain the shop the field VM just opened (see [`Self::try_arm_field_shop`])
    /// so the host can drive its buy/sell UI. Returns `None` if no shop is
    /// pending. The op-0x49 gate stays armed until [`Self::finish_field_shop`].
    pub fn take_pending_field_shop(&mut self) -> Option<crate::shop::ShopSession> {
        self.pending_field_shop.take()
    }

    /// Mark the open field shop closed: the op-0x49 tristate flips Armed ->
    /// Done so the field VM resumes past the merchant op on its next step. The
    /// arm itself is cleared by the VM's resume (`op49_clear`).
    pub fn finish_field_shop(&mut self) {
        self.field_shop_open = false;
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
                    clut: e.clut,
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

    /// Snapshot every placed overworld entity for the renderer: one
    /// [`WorldMapEntityMarker`] per installed entity that carries a world
    /// position, paired with its coarse [`WorldMapEntityKind`].
    ///
    /// Returns an empty vector unless the disc-placement seeding
    /// ([`Self::install_world_map_entities_at`]) populated
    /// [`Self::world_map_entity_positions`] - the config-only installers
    /// (which leave positions empty) produce no markers, so a camera-only or
    /// synthetic world map degrades cleanly. The marker `y` is the player
    /// actor's current plane (the placements are 2D), so markers sit on the
    /// player's walking plane rather than at an arbitrary `y = 0`.
    pub fn world_map_entity_markers(&self) -> Vec<WorldMapEntityMarker> {
        if self.world_map_entity_positions.is_empty() {
            return Vec::new();
        }
        let base_y = self
            .player_actor_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| a.move_state.world_y as f32)
            .unwrap_or(0.0);
        self.world_map_entity_positions
            .iter()
            .enumerate()
            .map(|(i, &(x, z))| {
                let kind = match self.world_map_entity_configs.get(i) {
                    Some(WorldMapEntityConfig::EncounterZone { .. }) => {
                        WorldMapEntityKind::EncounterZone
                    }
                    Some(WorldMapEntityConfig::Portal { .. }) => WorldMapEntityKind::Portal,
                    // An NPC config or no config at all (a plain interaction)
                    // both render as the NPC marker.
                    Some(WorldMapEntityConfig::Npc { .. }) | None => WorldMapEntityKind::Npc,
                };
                WorldMapEntityMarker {
                    world_pos: [x as f32, base_y, z as f32],
                    kind,
                }
            })
            .collect()
    }

    /// The player's overworld position for the renderer, or `None` when there
    /// is no active player actor. The world-map draw path shows the player at
    /// this position (the player's own mesh isn't drawn in
    /// [`SceneMode::WorldMap`]), oriented by [`WorldMapPlayerMarker::facing`].
    pub fn world_map_player_marker(&self) -> Option<WorldMapPlayerMarker> {
        let slot = self.player_actor_slot? as usize;
        let a = self.actors.get(slot)?;
        if !a.active {
            return None;
        }
        Some(WorldMapPlayerMarker {
            world_pos: [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ],
            facing: a.move_state.render_26,
        })
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

    /// Spawn a Seru-magic summon scene-graph from a parsed stager overlay (e.g.
    /// extraction PROT 0903, Gimard *Burning Attack*) at `origin` (world units).
    /// `record_bytes` is the overlay's raw bytes (the buffer `overlay` was parsed
    /// from); `model_base` is the pool index a part's `model_sel == 0` resolves to
    /// (the summon's mesh-set base, e.g. [`crate::scene::GIMARD_TAIL_FIRE_MODEL_INDEX`]).
    /// Replaces any in-flight summon. Tick it with [`Self::tick_summon`].
    ///
    /// NOTE this drives the engine's **move-VM scene-graph stand-in**
    /// ([`crate::summon::SummonScene`]), not the faithful player-summon render. A
    /// live trace resolved that retail draws the player summon as an ordinary
    /// battle actor via the per-object TRS-keyframe path `FUN_80048A08` /
    /// `FUN_8004998C` (ported in [`legaia_engine_vm::anim_vm`]); see the
    /// `SummonScene` module reconciliation note.
    // REF: FUN_80048A08 (faithful player-summon render = battle-actor TRS-keyframe draw)
    pub fn spawn_summon(
        &mut self,
        overlay: &legaia_asset::summon_overlay::SummonOverlay,
        record_bytes: &[u8],
        model_base: usize,
        origin: [i16; 3],
    ) {
        self.active_summon = Some(crate::summon::SummonScene::spawn(
            overlay,
            record_bytes,
            model_base,
            origin,
        ));
    }

    /// Advance the active summon one frame through the move VM. No-op when no
    /// summon is playing; drains the scene once every part has finished.
    /// `frame_delta` is the per-part wait-timer drain (anim-speed × frame-rate).
    pub fn tick_summon(&mut self, frame_delta: u16) {
        let Some(mut scene) = self.active_summon.take() else {
            return;
        };
        {
            // Borrow split: the move-VM host borrows the rest of `World` (sin
            // LUT etc.) while the scene's part states live in `scene`, taken out
            // above. `current_slot = None` — summon parts are not World actors,
            // so the slot-routed callbacks are inert for them.
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
            };
            scene.tick(&mut host, frame_delta);
        }
        if !scene.finished() {
            self.active_summon = Some(scene);
        }
    }

    /// Per-part render draws for the active summon's mesh-bearing parts (empty
    /// when no summon is playing). Each draw's `model_index` indexes
    /// [`Self::global_tmd_pool`]. See [`crate::summon::SummonScene::part_draws`]
    /// for the faithful-tick / interpreted-transform boundary.
    pub fn active_summon_part_draws(&self) -> Vec<crate::summon::SummonPartDraw> {
        self.active_summon
            .as_ref()
            .map(|s| s.part_draws())
            .unwrap_or_default()
    }

    /// Spawn a battle move's effect-FX scene-graph at `origin` (world units).
    ///
    /// A move's `0x01..=0x63` on-contact (`+0x12`) / launch (`+0x16`) effect-list
    /// entries each index the `0x801f6324` prototype-pointer table; every such
    /// entry resolves to a summon-format move-VM record (`+0x00 model_sel`,
    /// `+0x02 flags`, `+0x04` bytecode) staged by the shared `FUN_80021B04`
    /// machinery. This parses those records out of the retained battle-action
    /// overlay (PROT 0898) and spawns them as a [`crate::summon::SummonScene`]
    /// with model base [`crate::scene::EFFECT_MODEL_LIBRARY_BASE`] (the engine's
    /// fixed-library analogue of the retail `gp[0x754] = party_count + 2`; see
    /// that constant's docs), so each mesh part resolves to
    /// `global_tmd_pool[model_sel + 3]` — the PROT 0871 effect-model library,
    /// already resident.
    ///
    /// Returns `false` (no scene spawned) when the move-power table / overlay
    /// isn't installed (disc-free battles), the move id has no power record, or
    /// the move carries no spawnable effect entries. Replaces any in-flight
    /// move-FX scene. Tick with [`Self::tick_move_fx`].
    pub fn spawn_move_fx(&mut self, move_id: u8, origin: [i16; 3]) -> bool {
        let Some(overlay) = self.move_power_overlay.clone() else {
            return false;
        };
        let Some(cat) = self.move_power.as_ref() else {
            return false;
        };
        let Some(fx) = cat.fx_for_move_id(move_id) else {
            return false;
        };
        use legaia_asset::move_power::EffectListEntry;

        // VA → file-offset delta for the battle-action overlay (the move-power
        // table's VA vs its file offset). A 0x801f6324 entry's VA minus this
        // lands on the record's file offset.
        let va_to_file = legaia_asset::move_power::MOVE_POWER_TABLE_VA
            - legaia_asset::move_power::MOVE_POWER_TABLE_FILE_OFFSET as u32;

        // Parse ALL prototype records first (full offset set) so each record's
        // move-VM bytecode is bounded by its true packed neighbour, then select
        // the ones this move's Spawn entries reference. Bounding against only
        // this move's subset would over-run each record into the next selected
        // one rather than the next packed one.
        let Some(aux) = cat.aux_tables() else {
            return false;
        };
        let all_offsets: Vec<usize> = aux
            .proto()
            .iter()
            .filter(|&&va| va != 0)
            .map(|&va| va.wrapping_sub(va_to_file) as usize)
            .filter(|&f| f + 4 <= overlay.len())
            .collect();
        let all_parts = legaia_asset::summon_overlay::parse_records_at(&overlay, &all_offsets);

        // The file offsets this move's Spawn entries point at.
        let wanted: std::collections::BTreeSet<usize> = fx
            .contact_effects
            .iter()
            .chain(fx.launch_effects.iter())
            .filter_map(|e| match e.entry {
                EffectListEntry::Spawn(_) => e.proto,
                _ => None,
            })
            .map(|va| va.wrapping_sub(va_to_file) as usize)
            .collect();
        if wanted.is_empty() {
            return false;
        }
        let parts: Vec<legaia_asset::summon_overlay::SummonPart> = all_parts
            .into_iter()
            .filter(|p| wanted.contains(&p.record_off))
            .collect();
        if parts.is_empty() {
            return false;
        }
        self.active_move_fx = Some(crate::summon::SummonScene::spawn_parts(
            &parts,
            &overlay,
            crate::scene::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
        // Surface the move's presentation fields for the render / audio layers:
        // the trail/afterimage texpage (`+0x0b`) and the sound cue (`+0x0d`).
        self.active_move_fx_trail_texpage = Some(fx.trail_texpage);
        if fx.sound_cue_id != 0 {
            self.pending_move_fx_cue = Some(fx.sound_cue_id);
        }

        // The high-bit (`0x80`) effect-list entries route to the 2D efect.dat
        // pool (`FUN_801dfdf0`), not the 0x801f6324 scene-graph: spawn each
        // through the effect pool by its 7-bit id (no-op when efect.dat isn't
        // loaded). (Reached only on the scene-graph success path; a move whose
        // lists hold *only* AltEffect entries returns early above with the
        // empty-Spawn-set guard — an documented edge case, rare for FX moves.)
        let alt_ids: Vec<u8> = fx
            .contact_effects
            .iter()
            .chain(fx.launch_effects.iter())
            .filter_map(|e| match e.entry {
                legaia_asset::move_power::EffectListEntry::AltEffect(id) => Some(id),
                _ => None,
            })
            .collect();
        for id in alt_ids {
            self.try_spawn_effect(id, origin, 0);
        }
        true
    }

    /// Take the pending move-FX sound cue id, if [`Self::spawn_move_fx`] set one
    /// this step. The host routes it through `legaia_engine_audio::classify_cue`
    /// (the `FUN_8004fcc8` dispatch) → the SFX ring / voice trigger. Returns
    /// `None` when no cue is pending.
    pub fn take_pending_move_fx_cue(&mut self) -> Option<u8> {
        self.pending_move_fx_cue.take()
    }

    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, or `None` when none is playing. The render layer applies
    /// it to the move's streak pass.
    pub fn active_move_fx_trail_texpage(&self) -> Option<u16> {
        self.active_move_fx_trail_texpage
    }

    /// Advance the active move-FX scene one frame through the move VM (the
    /// move-FX sibling of [`Self::tick_summon`]). No-op when none is playing;
    /// drains the scene once every part has finished.
    pub fn tick_move_fx(&mut self, frame_delta: u16) {
        let Some(mut scene) = self.active_move_fx.take() else {
            return;
        };
        {
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
            };
            scene.tick(&mut host, frame_delta);
        }
        if !scene.finished() {
            self.active_move_fx = Some(scene);
        } else {
            // Scene drained: drop the trail texpage with it.
            self.active_move_fx_trail_texpage = None;
        }
    }

    /// Per-part render draws for the active move-FX scene's mesh-bearing parts
    /// (empty when none is playing). Each draw's `model_index` indexes
    /// [`Self::global_tmd_pool`] (the PROT 0871 effect-model library).
    pub fn active_move_fx_part_draws(&self) -> Vec<crate::summon::SummonPartDraw> {
        self.active_move_fx
            .as_ref()
            .map(|s| s.part_draws())
            .unwrap_or_default()
    }

    /// Take the pending production summon-spawn request, if a player Seru-magic
    /// cast set one this step. Returns `(spell_id, origin)`; the host maps
    /// `spell_id` to the overlay PROT entry (extraction `903 + (spell_id - 0x81)`), loads
    /// it, and calls [`Self::spawn_summon`]. See [`Self::pending_summon_spawn`].
    pub fn take_pending_summon_spawn(&mut self) -> Option<(u8, [i16; 3])> {
        self.pending_summon_spawn.take()
    }

    /// Request a summon spawn for `spell_id` at `origin` if it is a player
    /// Seru-magic id (`0x81..=0x8b`). Idempotent within a step (last cast wins);
    /// no-op for non-summon ids. The retail cast band's overlay-resolve point.
    pub(crate) fn request_summon_spawn(&mut self, spell_id: u8, origin: [i16; 3]) {
        if (crate::summon::SERU_SUMMON_IDS).contains(&spell_id) {
            self.pending_summon_spawn = Some((spell_id, origin));
        }
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
        // Step the active full-screen fade (escape teardown ramp); drop it
        // once the ramp lands on its target so hosts stop drawing the overlay.
        if let Some(fade) = &mut self.screen_fade
            && !fade.step()
        {
            self.screen_fade = None;
        }
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
        // Advance the opening-cutscene narration. A confirm press (Cross /
        // Circle) skips to the next page early, mirroring the retail text
        // balloon (which advances on confirm or its dwell timer, whichever is
        // first); otherwise the per-page dwell timer auto-advances it so the
        // cutscene plays unattended. Clear it once the last page finishes so
        // the prologue hand-off gate releases. (Edge-triggered: the same press
        // can't run through multiple pages in one frame.)
        let narration_confirm = self.input.just_pressed(input::PadButton::Cross)
            || self.input.just_pressed(input::PadButton::Circle);
        if let Some(narration) = &mut self.cutscene_narration {
            let still_on_screen = if narration_confirm {
                narration.skip_page()
            } else {
                narration.tick(1)
            };
            if !still_on_screen {
                self.cutscene_narration = None;
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
                // Per-tick: one Cross/Circle edge feeds at most one of the
                // script's 0x4C dialog poll or the interaction probe.
                self.dialog_input_consumed = false;
                self.step_cutscene_timeline();
                self.step_field();
                self.tick_tile_board();
                self.step_field_locomotion();
                // Interaction probe (retail FUN_801cf9f4): talk to an adjacent
                // NPC / dismiss its box on the action button. Runs before the
                // carrier tick so a dialogue-accept engage launches the battle
                // the same frame.
                self.tick_field_interaction_probe();
                self.tick_field_carriers();
                // Faithful dialogue path (opt-in): drive a just-opened field
                // dialogue through the field VM so branch handlers execute.
                self.drive_inline_dialogue();
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
                    self.step_cutscene_timeline();
                    self.step_field();
                }
                None
            }
            SceneMode::WorldMap => {
                self.tick_world_map();
                None
            }
            // The pause menu owns the frame (retail CARD mode 0x17): field /
            // battle dispatch is suspended; the hosting session drives the
            // menu state machine and restores the suspended mode on close.
            SceneMode::Menu => None,
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
        // direction is held. These are the Up/Right/Down/Left bits the
        // locomotion step ([`Self::step_world_map_locomotion`]) reads — the
        // face buttons (Triangle/Circle/Cross/Square, 0x1000..0x8000) must not
        // count as walking or a confirm press would suppress the talk-to gate.
        const WORLD_MAP_DPAD: u16 = input::PadButton::Up as u16
            | input::PadButton::Right as u16
            | input::PadButton::Down as u16
            | input::PadButton::Left as u16;
        self.world_map_player_walking = pad & WORLD_MAP_DPAD != 0;

        // Talk-to: open / dismiss an adjacent NPC's dialogue on a confirm
        // press. Runs before locomotion so opening a box suppresses movement +
        // portal auto-engage this frame (both gate off `current_dialog`).
        self.tick_world_map_npc_dialog();

        // Move the player first, then auto-engage any portal the player just
        // walked onto (sets its SM to Transitioning), so the entity SM step
        // below fires the portal's transition the *same* tick.
        self.step_world_map_locomotion();
        self.auto_engage_world_map_portals();

        if !self.world_map_entities.is_empty() {
            // Take the entity list out so the SM's host bridge can borrow the
            // world mutably (mirrors the monster-AI-state borrow window).
            let mut entities = std::mem::take(&mut self.world_map_entities);
            for (idx, ctx) in entities.iter_mut().enumerate() {
                let mut host = WorldMapEntityHostImpl { world: self };
                vm::world_map::step(idx, ctx, &mut host);
            }
            self.world_map_entities = entities;
        }

        // The region-keyed random-encounter roll (the `FUN_801D9E1C` path): on
        // each 128-unit tile crossing, roll the active region. No-op without a
        // region tracker, so a camera-only world map is unchanged.
        self.live_world_map_tick();

        // Resolve a latched overworld encounter into a battle. Runs for both
        // the entity-SM countdown path and the region-roll path.
        if let Some(formation_id) = self.pending_world_map_encounter.take() {
            self.begin_world_map_encounter(formation_id);
        }
    }

    /// Overworld player walk speed in world units per frame (per held d-pad
    /// direction). The field player moves ~8 units/frame
    /// ([`FIELD_BASE_STEP`]); the overworld uses the same baseline.
    pub const WORLD_MAP_PLAYER_SPEED: i16 = 8;

    /// Move the overworld player actor from the held d-pad, bounded by the
    /// scene's walkability grid.
    ///
    /// Held d-pad is remapped through the overworld camera azimuth
    /// ([`world_map_camera_relative_bits`]) so "screen up" walks away from the
    /// follow camera and "screen right" walks screen-right regardless of how
    /// the map is rotated — the same camera-relative remap retail's
    /// `func_0x800467e8` applies, and the counterpart to the field's
    /// [`Self::decode_field_direction`].
    ///
    /// Collision is **not** a separate unknown: the retail world-map-walk
    /// overlay's locomotion is the same `FUN_801d01b0` as the field, colliding
    /// against the same `_DAT_1f8003ec + 0x4000` walkability grid
    /// ([`Self::field_tile_is_wall`]) which [`crate::scene::SceneHost::enter_field_scene`]
    /// already loads from the scene's MAP file. Stepping runs through the shared
    /// [`Self::advance_with_collision`], so walls stop the overworld player
    /// exactly as on the field.
    ///
    /// No-op without a live player actor, while a dialog owns the frame, in the
    /// top-view debug camera, or while the player's movement-disabled flag
    /// (`+0x10 & 0x80000`) is set (encounter queued / cutscene owns the player).
    fn step_world_map_locomotion(&mut self) {
        if self.current_dialog.is_some() {
            return;
        }
        // In the top-view debug camera the d-pad scrolls the camera
        // ([`WorldMapController::tick`]); only walk the player in walk mode.
        if self
            .world_map_ctrl
            .as_ref()
            .is_some_and(|c| c.is_top_view())
        {
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
        // Held d-pad → camera-relative direction bits. `sx`/`sy` are the raw
        // screen deltas (right / forward); the azimuth remap rotates them into
        // world space against the overworld follow camera.
        let pad = self.input.pad();
        let mut sx = 0i32;
        let mut sy = 0i32;
        if pad & input::PadButton::Up.mask() != 0 {
            sy += 1;
        }
        if pad & input::PadButton::Down.mask() != 0 {
            sy -= 1;
        }
        if pad & input::PadButton::Right.mask() != 0 {
            sx += 1;
        }
        if pad & input::PadButton::Left.mask() != 0 {
            sx -= 1;
        }
        let azimuth = self.world_map_ctrl.as_ref().map(|c| c.azimuth).unwrap_or(0);
        let dir_bits = world_map_camera_relative_bits(azimuth, sx, sy);
        if dir_bits == 0 {
            return;
        }
        // Record the heading from the world-space movement direction (the same
        // `render_26` field the field path stores from `decode_field_direction`,
        // a PSX 12-bit angle: 4096 = full turn). The world-map walk uses the
        // camera-relative bits rather than `decode_field_direction`, so it must
        // set the heading itself; the player marker reads it to draw a facing
        // tick. Deterministic: same pad + azimuth -> same heading.
        let dz = (dir_bits & 0x1000 != 0) as i32 - (dir_bits & 0x4000 != 0) as i32;
        let dx = (dir_bits & 0x2000 != 0) as i32 - (dir_bits & 0x8000 != 0) as i32;
        if dx != 0 || dz != 0 {
            let heading = (((dx as f32).atan2(dz as f32) / std::f32::consts::TAU * 4096.0).round()
                as i32)
                .rem_euclid(4096) as i16;
            self.actors[slot].move_state.render_26 = heading;
        }
        let mut speed = self.world_map_player_speed.max(1) as i32;
        // Diagonal normalise: when both axes are moving, x0.75 - mirroring the
        // field controller (`FUN_801d01b0`) and the retail world-map walk
        // overlay (`speed -= speed >> 2`). `advance_with_collision` steps both
        // axes by the same amount, so without this a diagonal travels `speed` on
        // each axis = ~1.41x the cardinal speed.
        if dx != 0 && dz != 0 {
            speed -= speed >> 2;
        }
        self.advance_with_collision(slot, dir_bits, speed);
    }

    /// Per-tile overworld step → region-keyed encounter roll (the world-map
    /// counterpart to [`Self::live_field_tick`]).
    ///
    /// A "step" is the player actor crossing into a new 128-unit tile
    /// (`world >> 7`); each step drives one
    /// [`crate::region_encounter::RegionEncounterTracker::on_step`] against the
    /// player's current position. A trigger latches
    /// [`Self::pending_world_map_encounter`], which [`Self::tick_world_map`]
    /// resolves into a battle. The RNG comes from the world's shared
    /// deterministic source, drawn only on the trigger branch, so replays stay
    /// bit-identical. No-op without a player actor or region tracker.
    fn live_world_map_tick(&mut self) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (wx, wz) = match self.actors.get(slot as usize) {
            Some(a) => (a.move_state.world_x, a.move_state.world_z),
            None => return,
        };
        let tile = ((wx as i32) >> 7, (wz as i32) >> 7);
        let crossed = match self.world_map_last_tile {
            Some(prev) if prev != tile => {
                self.world_map_last_tile = Some(tile);
                true
            }
            None => {
                self.world_map_last_tile = Some(tile);
                false
            }
            _ => false,
        };
        if !crossed {
            return;
        }
        // Roll the active region. Take the tracker out so the RNG closure can
        // borrow `self` (same pattern as the entity-SM borrow window).
        if let Some(mut tracker) = self.world_map_region_tracker.take() {
            let roll = tracker.on_step(wx, wz, || self.next_rng());
            self.world_map_region_tracker = Some(tracker);
            if let Some(roll) = roll {
                self.pending_world_map_encounter = Some(roll.formation_id as u16);
            }
        }
    }

    /// Route the scene's region-keyed encounter table onto the overworld so
    /// [`Self::tick_world_map`] rolls random encounters per region. Resets the
    /// step-tile latch. Pair with [`Self::enter_world_map`] (or call after it).
    pub fn set_world_map_regions(&mut self, table: crate::region_encounter::RegionEncounterTable) {
        self.world_map_region_tracker =
            Some(crate::region_encounter::RegionEncounterTracker::new(table));
        self.world_map_last_tile = None;
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
        self.world_map_entity_positions.clear();
    }

    /// Seed overworld entities with a per-entity config **and** world position.
    /// One Idle state machine per `(config, position)`. The positions enable
    /// the auto-engage-on-walkover trigger in [`Self::tick_world_map`] (the
    /// player stepping onto a `Portal` entity's tile fires it). Replaces any
    /// previously installed set. This is the disc-placement seeding path
    /// ([`crate::scene::SceneHost::enter_world_map_scene`] feeds it the
    /// classified actor placements + their spawn positions).
    pub fn install_world_map_entities_at(
        &mut self,
        entities: Vec<(WorldMapEntityConfig, (i16, i16))>,
    ) {
        self.world_map_entities = (0..entities.len())
            .map(|_| vm::world_map::WorldMapEntityCtx::default())
            .collect();
        self.world_map_entity_positions = entities.iter().map(|(_, pos)| *pos).collect();
        self.world_map_entity_configs = entities.into_iter().map(|(cfg, _)| cfg).collect();
    }

    /// Auto-engage any `Portal` overworld entity the player is standing on.
    ///
    /// The clean-room stand-in for retail's per-entity player-position-in-zone
    /// trigger: a portal whose placement tile (`pos >> 7`) matches the player's
    /// current tile is driven to its transition state, exactly as a host
    /// [`Self::engage_world_map_entity`] call would, so the next SM step fires
    /// the [`crate::field_events::FieldEvent::WorldMapTransition`]. Only `Idle`
    /// portals are engaged, so a portal fires once per visit and the player can
    /// stand on the tile without re-triggering. NPC entities are *not*
    /// auto-engaged (they are talk-to, not walk-onto). No-op without entity
    /// positions, a player actor, or while a dialog owns the frame.
    fn auto_engage_world_map_portals(&mut self) {
        if self.current_dialog.is_some() || self.world_map_entity_positions.is_empty() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (px, pz) = match self.actors.get(slot as usize) {
            Some(a) => (
                (a.move_state.world_x as i32) >> 7,
                (a.move_state.world_z as i32) >> 7,
            ),
            None => return,
        };
        // Collect the portals the player is standing on (still Idle), then
        // engage them — separated so the immutable scan drops before the
        // mutable `engage` borrow.
        let mut to_engage: Vec<usize> = Vec::new();
        for (idx, ctx) in self.world_map_entities.iter().enumerate() {
            if ctx.state != vm::world_map::EntityState::Idle as u16 {
                continue;
            }
            if !matches!(
                self.world_map_entity_configs.get(idx),
                Some(WorldMapEntityConfig::Portal { .. })
            ) {
                continue;
            }
            let Some(&(ex, ez)) = self.world_map_entity_positions.get(idx) else {
                continue;
            };
            if (ex as i32) >> 7 == px && (ez as i32) >> 7 == pz {
                to_engage.push(idx);
            }
        }
        for idx in to_engage {
            self.engage_world_map_entity(idx);
        }
    }

    /// Open / dismiss an overworld NPC's dialogue on a confirm press.
    ///
    /// The talk-to counterpart of [`Self::auto_engage_world_map_portals`]
    /// (portals are walk-onto, NPCs are talk-to). While a box is up, a
    /// confirm/cancel press (`Cross`/`Circle`) dismisses it: the overworld has
    /// no field VM ticking to run the op-`0x4C` dismiss hook the field path
    /// uses ([`vm_hosts`](crate::world)), so the world map owns the dismiss
    /// directly. Otherwise, a confirm press while the player stands within one
    /// tile of an [`WorldMapEntityConfig::Npc`] that carries inline dialog text
    /// (the `Dialog` op the placement walker found) opens that text against the
    /// scene's MES container — sets [`Self::current_dialog`] and emits
    /// [`FieldEvent::OpenDialog`], which the host renders through
    /// [`crate::scene::SceneHost::open_pending_dialog`], the same panel path
    /// the field VM's op `0x3F` feeds.
    ///
    /// No-op while walking (a held direction is a movement frame, not a
    /// talk-to), without entity positions, or without a player actor. An NPC
    /// with an interaction but no inline text is left to the SM's
    /// [`FieldEvent::FieldInteract`] path unchanged.
    fn tick_world_map_npc_dialog(&mut self) {
        // A box is up: a confirm/cancel press dismisses it (and the locomotion
        // + auto-engage steps stay gated off `current_dialog` meanwhile).
        if self.current_dialog.is_some() {
            // The inline-script runner, when active, owns dismissal.
            if self.inline_dialogue.is_none()
                && (self.input.just_pressed(input::PadButton::Cross)
                    || self.input.just_pressed(input::PadButton::Circle))
            {
                self.current_dialog = None;
                self.pending_field_events.push(FieldEvent::DialogDismissed);
            }
            return;
        }
        // Otherwise a confirm press next to a talkable NPC opens its dialogue.
        if self.world_map_player_walking || !self.input.just_pressed(input::PadButton::Cross) {
            return;
        }
        if self.world_map_entity_positions.is_empty() {
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (px, pz) = match self.actors.get(slot as usize) {
            Some(a) => (
                (a.move_state.world_x as i32) >> 7,
                (a.move_state.world_z as i32) >> 7,
            ),
            None => return,
        };
        // First talkable NPC within one tile (Chebyshev) of the player. An NPC
        // is talkable when it carries inline dialog text or a box-config id.
        let mut open: Option<(u16, Vec<u8>)> = None;
        for (idx, cfg) in self.world_map_entity_configs.iter().enumerate() {
            let (text_id, inline) = match cfg {
                WorldMapEntityConfig::Npc {
                    text_id, inline, ..
                } if text_id.is_some() || !inline.is_empty() => {
                    (text_id.unwrap_or(0), inline.clone())
                }
                _ => continue,
            };
            let Some(&(ex, ez)) = self.world_map_entity_positions.get(idx) else {
                continue;
            };
            if ((ex as i32 >> 7) - px).abs() <= 1 && ((ez as i32 >> 7) - pz).abs() <= 1 {
                open = Some((text_id, inline));
                break;
            }
        }
        if let Some((text_id, inline)) = open {
            self.current_dialog = Some(DialogRequest {
                text_id,
                inline: inline.clone(),
                world_x: 0,
                world_z: 0,
                depth_id: 0,
            });
            self.pending_field_events.push(FieldEvent::OpenDialog {
                text_id,
                inline,
                world_x: 0,
                world_z: 0,
                depth_id: 0,
            });
        }
    }

    /// Host signal that the player engaged overworld entity `idx` (walked onto
    /// a portal tile / pressed confirm on it). Drives the entity SM straight to
    /// its scene-transition state so the next [`Self::tick_world_map`] fires the
    /// transition; a [`WorldMapEntityConfig::Portal`] then surfaces a
    /// [`crate::field_events::FieldEvent::WorldMapTransition`] with its target
    /// map. No-op for an out-of-range index.
    ///
    /// Hosts can call this directly; [`Self::auto_engage_world_map_portals`]
    /// also calls it each tick for any `Portal` entity the player has walked
    /// onto (the engine-driven trigger), so an entity installed with a position
    /// via [`Self::install_world_map_entities_at`] fires on walk-over without a
    /// host call.
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
        // The slot map is only meaningful for a MAN-derived install; a
        // hand-built set has no placement slots. Clear it (and any armed engage)
        // so a re-install never leaves a stale slot pointing at the old set.
        self.field_carrier_slots.clear();
        self.pending_carrier_engage = None;
    }

    /// Install the scene's field carriers **derived from its MAN actor-placement
    /// partition** ([`crate::man_field_scripts::derive_field_carriers`]) rather
    /// than from a hand-built list, replacing any previously installed set.
    ///
    /// Returns the carrier-Vec index of the Rim Elm sparring partner (the
    /// [`FieldCarrierConfig::ScriptedEncounter`] carrier) when the scene MAN
    /// contains it (town01), so the caller can [`Self::engage_field_carrier`] it
    /// on the dialogue-accept; `None` for scenes without that placement.
    ///
    /// This is the faithful counterpart to the hand-built
    /// [`Self::install_field_carriers`]: the carrier set, and the sparring
    /// carrier's identity within it, come from the real scene data.
    pub fn install_field_carriers_from_man(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> Option<usize> {
        let derived = crate::man_field_scripts::derive_field_carriers(man_file, man);
        let sparring_idx = derived
            .iter()
            .position(|d| matches!(d.config, FieldCarrierConfig::ScriptedEncounter { .. }));

        // Map each scripted-encounter carrier's placement slot -> its carrier-Vec
        // index, so a field-interact on that placement auto-arms the fight. The
        // carrier index is the position in `derived` (install_field_carriers
        // preserves order). Plain talk NPCs are intentionally excluded: talking
        // to them must never launch a battle.
        let carrier_slots: std::collections::HashMap<u8, usize> = derived
            .iter()
            .enumerate()
            .filter(|(_, d)| matches!(d.config, FieldCarrierConfig::ScriptedEncounter { .. }))
            .filter_map(|(idx, d)| u8::try_from(d.placement_index).ok().map(|slot| (slot, idx)))
            .collect();

        self.install_field_carriers(derived.into_iter().map(|d| d.config).collect());
        // install_field_carriers cleared the slot map; repopulate for this set.
        self.field_carrier_slots = carrier_slots;

        // Capture each actor's inline interaction-script dialogue, keyed by its
        // partition-1 record index (= the `slot` a field-interact op carries),
        // so `field_interact` can open the interacted actor's real dialogue.
        // This is the actor's own inline MES text (retail `actor[+0x90]`), the
        // mechanism `0x3F` was wrongly standing in for.
        self.field_npc_dialog.clear();
        self.field_npc_dialog_prologue.clear();
        self.field_npc_positions.clear();
        for (placement, kind) in crate::man_field_scripts::classify_placements(man_file, man) {
            if let crate::man_field_scripts::PlacementKind::Npc {
                dialog_inline: Some(inline),
                ..
            } = kind
                && let Ok(slot) = u8::try_from(placement.index)
            {
                self.field_npc_dialog.insert(slot, inline);
                // Stash the untruncated record so the opt-in field-VM runner can
                // execute the interaction prologue (segment selection) — purely
                // additive; the default path keeps using `field_npc_dialog`.
                if let Some(prologue) =
                    crate::man_field_scripts::placement_inline_prologue(man_file, man, &placement)
                {
                    self.field_npc_dialog_prologue.insert(slot, prologue);
                }
                // The interaction probe box-tests the player against this spawn
                // position (= runtime actor frame; see `field_npc_positions`).
                self.field_npc_positions
                    .insert(slot, (placement.world_x, placement.world_z));
            }
        }

        sparring_idx
    }

    /// Trigger a field interaction on placement `slot` (retail's field-interact
    /// op `0x3E` with `op0 < 100`, and the interaction-probe dispatch). Opens
    /// the actor's inline dialogue if it has any, arms / engages a scripted-
    /// encounter carrier on that slot (the dialogue-accept auto-arm), and
    /// surfaces a [`FieldEvent::FieldInteract`]. Shared by the field VM host and
    /// [`Self::tick_field_interaction_probe`].
    pub fn trigger_field_interact(&mut self, interact_id: u8, slot: u8) {
        self.last_field_interact = Some((interact_id, slot));
        // Stash this slot's untruncated record (if any) so the opt-in VM-dialogue
        // runner can execute its interaction prologue. Always reassigned (to
        // `None` when absent) so a prior interaction's prologue can't leak.
        self.active_inline_prologue = self.field_npc_dialog_prologue.get(&slot).cloned();
        let opened_dialog = if let Some(inline) = self.field_npc_dialog.get(&slot).cloned() {
            self.open_field_dialog(inline);
            true
        } else {
            false
        };
        // A scripted-encounter carrier on this slot (the sparring partner): with
        // a prompt up, the engage waits for the accept (dialog dismiss); a
        // carrier with no inline text engages immediately on interaction.
        if let Some(&carrier_idx) = self.field_carrier_slots.get(&slot) {
            if opened_dialog {
                self.pending_carrier_engage = Some(carrier_idx);
            } else {
                self.engage_field_carrier(carrier_idx);
            }
        }
        self.pending_field_events
            .push(crate::field_events::FieldEvent::FieldInteract { interact_id, slot });
    }

    /// Open a field dialogue box from an inline interaction-script buffer (the
    /// text is the buffer itself; the retail box geometry isn't pinned, so the
    /// box coords are zero). Sets [`Self::current_dialog`] and surfaces a
    /// [`FieldEvent::OpenDialog`].
    fn open_field_dialog(&mut self, inline: Vec<u8>) {
        self.current_dialog = Some(DialogRequest {
            text_id: 0,
            inline: inline.clone(),
            world_x: 0,
            world_z: 0,
            depth_id: 0,
        });
        self.pending_field_events
            .push(crate::field_events::FieldEvent::OpenDialog {
                text_id: 0,
                inline,
                world_x: 0,
                world_z: 0,
                depth_id: 0,
            });
    }

    /// Clean-room interaction probe — retail `FUN_801cf9f4`, the action-button
    /// adjacency test that talks to a nearby field NPC.
    ///
    /// Mirrors [`Self::tick_world_map_npc_dialog`] for field mode: a single
    /// handler for both opening and dismissing a field dialogue on player input,
    /// so a probe-opened box (talking to an NPC by walking up to it, with no
    /// script `0x4C` poll) still dismisses.
    ///
    /// - **Box up:** a just-pressed Cross / Circle dismisses it (and engages a
    ///   pending scripted-encounter carrier — the dialogue-accept).
    /// - **No box:** a just-pressed Cross runs the retail facing probe
    ///   ([`Self::field_interact_probe_slot`]: the `DAT_801f2254` compass
    ///   point 64 units ahead, ±72 box); a hit opens that NPC's dialogue via
    ///   [`Self::trigger_field_interact`] and turns the player toward it
    ///   ([`Self::face_field_npc`]).
    ///
    /// The [`Self::dialog_input_consumed`] per-tick guard keeps this and the
    /// field VM's `0x4C` dialog poll from both acting on the same button edge.
    /// No-op without a player actor or installed NPC positions.
    ///
    /// PORT: FUN_801cf9f4
    /// REF: FUN_8003A1E4, FUN_80024C88
    fn tick_field_interaction_probe(&mut self) {
        use crate::input::PadButton;
        let confirm = self.input.just_pressed(PadButton::Cross);
        let cancel = self.input.just_pressed(PadButton::Circle);

        if self.current_dialog.is_some() {
            // The inline-script runner, when active, owns box dismissal.
            if self.inline_dialogue.is_none() && (confirm || cancel) && !self.dialog_input_consumed
            {
                self.dialog_input_consumed = true;
                self.current_dialog = None;
                self.pending_field_events
                    .push(crate::field_events::FieldEvent::DialogDismissed);
                if let Some(idx) = self.pending_carrier_engage.take() {
                    self.engage_field_carrier(idx);
                }
            }
            return;
        }

        if self.dialog_input_consumed || !confirm || self.field_npc_positions.is_empty() {
            return;
        }
        // Retail geometry: a single facing-indexed compass probe 64 units
        // ahead, box-tested at ±72 against each NPC
        // ([`Self::field_interact_probe_slot`]). A hit posts the touch event
        // on the matched actor and turns the player toward it — the
        // face-the-NPC step retail applies to moving-class partners
        // (`flags & 0x20010 == 0x20000`), which every talk NPC is
        // (capture-pinned by `rimelm_npc_press_tetsu`).
        if let Some(npc_slot) = self.field_interact_probe_slot() {
            self.dialog_input_consumed = true;
            self.trigger_field_interact(0, npc_slot);
            self.face_field_npc(npc_slot);
        }
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

    /// Advance the per-object battle animation of every actor carrying one,
    /// folding the result into `pose_frame`. The battle render path then
    /// deforms each actor's mesh through `tmd_to_vram_mesh_posed_rot`. Call once
    /// per battle frame (the field [`tick_actors`](Self::tick_actors) drives the
    /// ANM path instead). Unlike `tick_actors` this does not gate on `.active`,
    /// since battle-init actors keep their `tmd_binding` without the field
    /// `.active` flag.
    pub fn tick_battle_animations(&mut self) {
        for actor in &mut self.actors {
            if let Some(player) = &mut actor.battle_animation {
                actor.pose_frame = Some(player.tick());
            }
        }
    }

    /// Bind a battle animation player to actor `slot`, resetting its
    /// `pose_frame`. No-ops for out-of-range slots.
    pub fn set_actor_battle_animation(
        &mut self,
        slot: usize,
        player: crate::battle_anim::MonsterAnimPlayer,
    ) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.battle_animation = Some(player);
            actor.pose_frame = None;
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

    /// Install the per-scene region / zone tables (the `.MAP` `+0x10000`
    /// block + the MAN section-3 camera-region table) and run the initial
    /// per-tile refresh. Pass empty slices for scenes without the data -
    /// the refresh then clears [`Self::extra_flags`] and resets the
    /// attribute block to the default fill, so stale tables never leak
    /// across a transition.
    pub fn load_field_region_tables(&mut self, map_region_block: &[u8], zone_table: &[u8]) {
        self.field_map_region_block = map_region_block.to_vec();
        self.field_zone_table = zone_table.to_vec();
        self.refresh_field_regions();
    }

    /// Per-tile region refresh - drives the [`crate::field_regions`] ports
    /// (`FUN_800180EC` + `FUN_801DBA20`) against the player's current tile.
    ///
    /// Quantises `tile = (world - 0x40) >> 7` (the retail locomotion-cluster
    /// convention for `FUN_801DBA20`'s arguments), rebuilds
    /// [`Self::extra_flags`] (the `_DAT_8007B8F4` region-type mask the
    /// field-VM op `0x42` mode 0 tests), latches the scratch attribute
    /// block, and re-selects the current camera-zone record. Called on
    /// scene entry and on every player tile crossing
    /// ([`Self::live_field_tick`]).
    ///
    /// REF: FUN_800180EC, FUN_801DBA20 (ports in [`crate::field_regions`])
    pub fn refresh_field_regions(&mut self) {
        if self.field_map_region_block.is_empty() && self.field_zone_table.is_empty() {
            // No per-scene tables installed - leave `extra_flags` to the
            // host (e.g. tests that drive op 0x42 directly).
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let (wx, wz) = match self.actors.get(slot as usize) {
            Some(a) => (a.move_state.world_x, a.move_state.world_z),
            None => return,
        };
        let tx = (wx as i32 - 0x40) >> 7;
        let tz = (wz as i32 - 0x40) >> 7;
        let table = crate::field_regions::RegionTable::parse(&self.field_map_region_block);
        let world_map_mode = self.mode == SceneMode::WorldMap;
        let (mask, attrs) =
            crate::field_regions::refresh_region_attributes(table.as_ref(), tx, tz, world_map_mode);
        self.extra_flags = mask;
        self.field_region_attributes = attrs;
        if let Some(result) =
            crate::field_regions::zone_query(&self.field_zone_table, table.as_ref(), &attrs, tx, tz)
        {
            // Retail rewrites `_DAT_8007B8F4` from the zone query's own
            // rebuild too (identical recomputation).
            self.extra_flags = result.region_mask;
            self.field_zone_record = result.record.map(|r| {
                let mut rec = [0u8; crate::field_regions::ZONE_RECORD_STRIDE];
                rec.copy_from_slice(r);
                rec
            });
        } else {
            self.field_zone_record = None;
        }
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
    /// Sample the field floor height at a world `(x, z)`, the port of
    /// `FUN_80019278`'s height branch (`ghidra/scripts/funcs/80019278.txt`).
    ///
    /// The collision grid's **low nibble** is a floor-elevation tier; this
    /// resolves it through the per-scene [`Self::field_floor_height_lut`] and
    /// **bilinearly interpolates** the `2x2` tile block around the position. The
    /// tile is `(x >> 7, z >> 7)` (128-unit tiles); the sub-tile weights are
    /// `x & 0x7F` / `z & 0x7F` (0..=127). When all four corner tiers match, the
    /// LUT value is returned directly (the retail fast path); otherwise the four
    /// corner heights are weighted `top*(0x80-wz) + bottom*wz` (each edge
    /// interpolated by `wx`) and divided by `0x4000` (`>> 14`, with the retail
    /// `+0x3FFF` round-toward-zero on a negative accumulator).
    ///
    /// This is the town-field floor sampler: the retail function's `+0x8000`
    /// attribute gating — the world-map continent `0x1000` on-grid flag side
    /// effect and the `0x800` tile-board special branch (`func_0x801d5630`) — is
    /// **not** reproduced here (the engine doesn't keep that attribute grid).
    /// Returns `0` when the grid / LUT isn't loaded or the tile is out of range.
    ///
    /// PORT: FUN_80019278 (floor-height branch; the `+0x8000` continent /
    /// tile-board branches stay with the field/world-map systems).
    pub fn sample_field_floor_height(&self, world_x: i32, world_z: i32) -> i32 {
        if self.field_collision_grid.len() < FIELD_GRID_LEN {
            return 0;
        }
        let tile_x = world_x >> 7;
        let tile_z = world_z >> 7;
        // The 2x2 block needs (tile_x+1, tile_z+1) in range.
        if tile_x < 0
            || tile_z < 0
            || tile_x as usize + 1 >= FIELD_GRID_STRIDE
            || tile_z as usize + 1 >= FIELD_GRID_STRIDE
        {
            return 0;
        }
        let base = tile_z as usize * FIELD_GRID_STRIDE + tile_x as usize;
        let g = &self.field_collision_grid;
        let lut = &self.field_floor_height_lut;
        // Low nibble = elevation tier; LUT-index it for each of the 4 corners.
        let c00 = (g[base] & 0x0F) as usize;
        let c01 = (g[base + 1] & 0x0F) as usize;
        let c10 = (g[base + FIELD_GRID_STRIDE] & 0x0F) as usize;
        let c11 = (g[base + FIELD_GRID_STRIDE + 1] & 0x0F) as usize;
        if c00 == c01 && c00 == c10 && c00 == c11 {
            return lut[c00] as i32;
        }
        let wx = world_x & 0x7F;
        let wz = world_z & 0x7F;
        let (l00, l01, l10, l11) = (
            lut[c00] as i32,
            lut[c01] as i32,
            lut[c10] as i32,
            lut[c11] as i32,
        );
        let acc =
            (l01 * wx + l00 * (0x80 - wx)) * (0x80 - wz) + l10 * (0x80 - wx) * wz + l11 * wx * wz;
        if acc < 0 {
            (acc + 0x3FFF) >> 14
        } else {
            acc >> 14
        }
    }

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
    /// Single candidate-centre wall test against the `+0x4000` grid, using
    /// retail's exact sub-cell derivation: `zc = (z>>6)+2`,
    /// `xc = ((x+0x3f)>>6)-1`, tile column/row = `sub_cell >> 1` (rows of
    /// `0x80` bytes), wall bit = `byte >> 4 & quadrant_mask` with quadrant
    /// `(zc & 1) * 2 + (xc & 1)`.
    ///
    /// The `+2` Z bias and `ceil-1` X rounding are NOT optional look-ahead:
    /// the wall bits are authored with the bias baked in. This is proven by
    /// the `rimelm_wall_press_down` capture: the live player rests pressed
    /// against a wall at a position whose plain floor-indexed cell is an
    /// all-quads wall byte (the player could never legally stand there under
    /// floor indexing) while the biased read places that wall band one tile
    /// north, exactly where the on-screen wall blocks. The floor sampler
    /// ([`Self::sample_field_floor_height`], `FUN_80019278`) reads the SAME
    /// grid bytes with plain floor indexing — the low (elevation) and high
    /// (wall) nibbles of one byte are addressed under two different
    /// world-to-cell mappings by their two retail consumers. See
    /// `docs/subsystems/field-locomotion.md` ("Collision") and the
    /// disc-gated `engine-shell/tests/field_collision_discriminator.rs`.
    ///
    /// Retail tests **three leading-edge footprint probes** through this
    /// sampler (47-48 units ahead, ±16 lateral; per-direction table
    /// `DAT_801f2214` = [`FIELD_WALL_PROBES`]) — see
    /// [`World::field_dir_blocked`], wired into pad locomotion behind
    /// [`World::leading_edge_wall_probes`]. With the flag off, locomotion
    /// tests one candidate-centre point — a standoff/feel difference, not an
    /// indexing one.
    pub fn field_tile_is_wall(&self, x: i16, z: i16) -> bool {
        if self.field_collision_grid.len() < FIELD_GRID_LEN {
            return false;
        }
        if x < 0 || z < 0 {
            return true; // off the grid origin reads as a wall (clamp inside)
        }
        let zc = ((z as i32) >> 6) + 2;
        let xc = (((x as i32) + 0x3F) >> 6) - 1;
        let col = (xc / 2) & 0x7F;
        let row = (zc - (zc >> 31)) >> 1;
        let idx = (col + row * FIELD_GRID_STRIDE as i32) as usize;
        let Some(&byte) = self.field_collision_grid.get(idx) else {
            return false;
        };
        let quad = ((zc & 1) << 1 | (xc & 1)) as u32;
        (byte >> 4) & (1u8 << quad) != 0
    }

    /// Retail's static-wall direction test: from the CURRENT position
    /// `(x, z)`, probe the three leading-edge points of [`FIELD_WALL_PROBES`]
    /// row `dir` (`0` = Z-, `1` = X-, `2` = Z+, `3` = X+) through
    /// [`Self::field_tile_is_wall`]; the direction is blocked when any probe
    /// lands on a wall sub-cell.
    ///
    /// PORT: FUN_801cfe4c
    /// REF: FUN_801cfc40
    ///
    /// This is the static-wall arm of `FUN_801cfe4c` (result bit `2`): the
    /// probes are taken at the player's pre-step position, so a step commits
    /// while the edge is still clear and the next step from the deeper
    /// position blocks — the player rests 47-48 units off the wall plane,
    /// step-exact (pinned by the `rimelm_wall_press_left`/`_down` captures).
    /// The actor-collision arm (result bits `1`/`4`) is
    /// [`Self::field_actor_dir_blocked`].
    pub fn field_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        FIELD_WALL_PROBES[dir & 3]
            .iter()
            .any(|&(dx, dz)| self.field_tile_is_wall(x.saturating_add(dx), z.saturating_sub(dz)))
    }

    /// Retail's actor-collision direction test: from the CURRENT position
    /// `(x, z)`, take the three probe points of [`FIELD_ACTOR_PROBES`] row
    /// `dir` (same `(x + dx, z - dz)` convention as the wall probes) and
    /// box-test each against every field NPC's position
    /// ([`Self::field_npc_positions`]); the direction is blocked when any
    /// probe lands within [`FIELD_NPC_BOX_HALF`] (40 units) of an NPC on
    /// both axes (strict).
    ///
    /// PORT: FUN_801cfc40
    /// REF: FUN_801cfe4c
    ///
    /// Covers both entity classes of `FUN_801cfc40`:
    ///
    /// - the **moving-actor arm** (result bit `1`) — the class village NPCs
    ///   belong to, capture-pinned by `rimelm_npc_press_tetsu` (the sparring
    ///   partner's `flags+0x10 = 0x08020884` carries the `0x20000` class
    ///   bit, and the mutual `+0x98` collision link is live in-frame). The
    ///   engine's NPCs don't roam, so their live position equals their MAN
    ///   placement anchor; box ±[`FIELD_NPC_BOX_HALF`] (40).
    /// - the **static-entity arm** (result bit `4`) — placed `.MAP` props,
    ///   box ±[`FIELD_PROP_BOX_HALF`] (80) around the record-derived
    ///   footprint centre ([`Self::field_prop_colliders`]).
    ///
    /// Not modelled: the retail touch side-effects on the locomotion path
    /// (mutual `+0x98` partner link; the prop walk-touch auto event post
    /// `FUN_801d5b5c` — the engine has no prop event scripts), NPC motion,
    /// and the `_DAT_8007b6b8 == 0x20` full-table delegation to
    /// `FUN_801cf9f4`. The button-press interact dispatch (facing probe +
    /// event + face-the-NPC) IS modelled —
    /// [`Self::tick_field_interaction_probe`]. Faithful quirk kept: the
    /// probe has no near-side clamp, so a position already deep inside a box
    /// (past the probe reach) reads clear — exactly as retail's forward-only
    /// probe behaves.
    pub fn field_actor_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        if self.field_npc_positions.is_empty() && self.field_prop_colliders.is_empty() {
            return false;
        }
        FIELD_ACTOR_PROBES[dir & 3].iter().any(|&(dx, dz)| {
            let px = x.saturating_add(dx) as i32;
            let pz = z.saturating_sub(dz) as i32;
            self.field_npc_positions.values().any(|&(ax, az)| {
                (px - ax as i32).abs() < FIELD_NPC_BOX_HALF
                    && (pz - az as i32).abs() < FIELD_NPC_BOX_HALF
            }) || self.field_prop_colliders.iter().any(|&(cx, cz)| {
                (px - cx).abs() < FIELD_PROP_BOX_HALF && (pz - cz).abs() < FIELD_PROP_BOX_HALF
            })
        })
    }

    /// Retail's interact probe: from the player's position, take the single
    /// [`FIELD_FACING_PROBES`] compass point 64 units ahead along the
    /// current facing and return the NPC whose ±[`FIELD_INTERACT_BOX_HALF`]
    /// (72-unit) box contains it, if any.
    ///
    /// PORT: FUN_801cf9f4
    /// REF: FUN_801d01b0
    ///
    /// The engine's field heading ([`decode_field_direction`]
    /// (Self::decode_field_direction)) stores `0` = Z+ while the retail
    /// facing byte stores `0` = Z- (a Z+ walk writes `0x800` to `+0x26`), so
    /// the sector index adds the half-turn before quantising. On overlapping
    /// NPC boxes retail keeps the *last* actor-list hit (the `+0x98` link is
    /// overwritten per match); the engine's NPC set is a hash map with no
    /// list order, so it picks the hit nearest the probe point instead
    /// (tie-break: lowest slot) — identical whenever NPCs stand more than
    /// 144 units apart, which every authored placement does.
    fn field_interact_probe_slot(&self) -> Option<u8> {
        let slot = self.player_actor_slot? as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return None;
        }
        let ms = &self.actors[slot].move_state;
        let (x, z) = (ms.world_x, ms.world_z);
        let sector = (((ms.render_26 as i32 + 0x800) & 0xfff) >> 9) as usize;
        let (dx, dz) = FIELD_FACING_PROBES[sector];
        let px = x.saturating_add(dx) as i32;
        let pz = z.saturating_sub(dz) as i32;
        let mut best: Option<(i32, u8)> = None;
        for (&npc_slot, &(ax, az)) in &self.field_npc_positions {
            let (ex, ez) = ((px - ax as i32).abs(), (pz - az as i32).abs());
            if ex < FIELD_INTERACT_BOX_HALF && ez < FIELD_INTERACT_BOX_HALF {
                let d = ex * ex + ez * ez;
                if best.is_none_or(|(bd, bs)| d < bd || (d == bd && npc_slot < bs)) {
                    best = Some((d, npc_slot));
                }
            }
        }
        best.map(|(_, s)| s)
    }

    /// Turn the player toward field NPC `npc_slot` (retail's face-the-NPC
    /// step after a successful interact probe: `func_0x80019b28` computes
    /// the 12-bit angle from the touched actor to the player and stores it
    /// in the player's `+0x26`). The engine computes the same angle with
    /// float `atan2` in its own heading convention (`0` = Z+) rather than
    /// retail's arctan LUT at `0x8006f4c8`, so it is shape-faithful, not
    /// bit-exact — the value only feeds the heading marker and the next
    /// probe's 45° sector quantisation.
    ///
    /// REF: FUN_80019b28
    fn face_field_npc(&mut self, npc_slot: u8) {
        let Some(&(nx, nz)) = self.field_npc_positions.get(&npc_slot) else {
            return;
        };
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() {
            return;
        }
        let ms = &mut self.actors[slot].move_state;
        let (dx, dz) = (
            (nx as i32 - ms.world_x as i32) as f32,
            (nz as i32 - ms.world_z as i32) as f32,
        );
        if dx == 0.0 && dz == 0.0 {
            return;
        }
        ms.render_26 =
            ((dx.atan2(dz) / std::f32::consts::TAU * 4096.0).round() as i32 & 0x0FFF) as i16;
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
        // Lock pad-driven locomotion while an opening-cutscene timeline owns
        // the scene (the establishing camera sweep + name-entry). During the
        // sweep the script drives the lead actor through its own MoveTo ops;
        // the pad must not also walk the player out from under the cinematic
        // camera. Releases the frame the timeline drops (matches retail, where
        // free-roam control returns only after the opening choreography ends).
        if self.cutscene_timeline_active() {
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

        self.advance_with_collision(slot, dir_bits, speed);

        // Terrain follow (gated): after the X/Z step commits, snap the
        // player's Y to the per-scene floor elevation at the new tile. Done
        // here rather than inside the shared `advance_with_collision` so the
        // world-map walk path (which collides through the same routine but
        // derives height from the continent grid) is unaffected. No-op height
        // 0 until a scene supplies a floor LUT.
        if self.follow_terrain_height {
            let (x, z) = {
                let ms = &self.actors[slot].move_state;
                (ms.world_x as i32, ms.world_z as i32)
            };
            let y = self.sample_field_floor_height(x, z);
            self.actors[slot].move_state.world_y = y as i16;
        }
    }

    /// Advance actor `slot` by `speed` world units in the direction encoded by
    /// `dir_bits` (post-remap convention: `0x1000`=Z+, `0x4000`=Z-,
    /// `0x2000`=X+, `0x8000`=X-), stepping [`FIELD_STEP_UNIT`] at a time and
    /// committing only the axes that stay off a wall in
    /// [`World::field_collision_grid`]. X collision uses the just-committed Z
    /// so a diagonal move can't tunnel through a wall corner.
    ///
    /// Shared by [`Self::step_field_locomotion`] and
    /// [`Self::step_world_map_locomotion`]: retail `FUN_801d01b0` is the same
    /// routine in both the field and world-map-walk overlays, and both collide
    /// against the same `_DAT_1f8003ec + 0x4000` walkability grid.
    ///
    /// With [`Self::leading_edge_wall_probes`] set, each axis instead blocks
    /// on retail's three-probe leading-edge footprint taken at the CURRENT
    /// position ([`Self::field_dir_blocked`]) — the retail standoff — and
    /// commits the step whenever the edge is clear. The default candidate-
    /// centre test is kept (off-flag) for the locomotion oracles and the
    /// BFS nav drivers. With [`Self::solid_field_npcs`] set, each axis
    /// additionally blocks when the direction's actor-collision probes land
    /// inside a field NPC's body box ([`Self::field_actor_dir_blocked`]) —
    /// retail gates a step on the actor bits and the wall bit together
    /// (`FUN_801cfe4c` returning any of `1`/`2`/`4` refuses the 2-unit step).
    pub fn advance_with_collision(&mut self, slot: usize, dir_bits: u16, speed: i32) {
        let edge = self.leading_edge_wall_probes;
        let solid_npcs = self.solid_field_npcs;
        let mut remaining = speed;
        while remaining > 0 {
            let ms = &self.actors[slot].move_state;
            let (cx, cz) = (ms.world_x, ms.world_z);
            // Z axis.
            if dir_bits & 0x1000 != 0 {
                let nz = cz.saturating_add(FIELD_STEP_UNIT as i16);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz, 2)
                } else {
                    self.field_tile_is_wall(cx, nz)
                } || (solid_npcs && self.field_actor_dir_blocked(cx, cz, 2));
                if !blocked {
                    self.actors[slot].move_state.world_z = nz;
                }
            } else if dir_bits & 0x4000 != 0 {
                let nz = cz.saturating_sub(FIELD_STEP_UNIT as i16);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz, 0)
                } else {
                    self.field_tile_is_wall(cx, nz)
                } || (solid_npcs && self.field_actor_dir_blocked(cx, cz, 0));
                if !blocked {
                    self.actors[slot].move_state.world_z = nz;
                }
            }
            // X axis (re-read X in case Z committed; X collision uses the
            // committed Z so footprints don't tunnel diagonally).
            let cz2 = self.actors[slot].move_state.world_z;
            if dir_bits & 0x2000 != 0 {
                let nx = cx.saturating_add(FIELD_STEP_UNIT as i16);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz2, 3)
                } else {
                    self.field_tile_is_wall(nx, cz2)
                } || (solid_npcs && self.field_actor_dir_blocked(cx, cz2, 3));
                if !blocked {
                    self.actors[slot].move_state.world_x = nx;
                }
            } else if dir_bits & 0x8000 != 0 {
                let nx = cx.saturating_sub(FIELD_STEP_UNIT as i16);
                let blocked = if edge {
                    self.field_dir_blocked(cx, cz2, 1)
                } else {
                    self.field_tile_is_wall(nx, cz2)
                } || (solid_npcs && self.field_actor_dir_blocked(cx, cz2, 1));
                if !blocked {
                    self.actors[slot].move_state.world_x = nx;
                }
            }
            remaining -= FIELD_STEP_UNIT;
        }
    }

    /// Step the player one navigation frame toward world position `(tx, tz)`,
    /// using the same per-axis field collision as pad locomotion
    /// ([`Self::advance_with_collision`]) but a world-space direction. Returns
    /// `true` once the player is within `tol` units of the target on both axes.
    ///
    /// This is the auto-navigation primitive a driver loops (following a path of
    /// waypoints) to walk the player to a target — e.g. the v0.1 oracle walking
    /// from the cold-boot spawn to the sparring partner before talking to it.
    /// It drives the real locomotion stepping/collision, just without the pad →
    /// camera-relative remap. No-op without an active player actor.
    pub fn nav_step_toward(&mut self, tx: i16, tz: i16, tol: i16) -> bool {
        let Some(slot) = self.player_actor_slot else {
            return false;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return false;
        }
        let (cx, cz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        if (cx - tx).abs() <= tol && (cz - tz).abs() <= tol {
            return true;
        }
        let mut dir = 0u16;
        let (mut wx, mut wz) = (0i32, 0i32);
        if tz > cz {
            dir |= 0x1000; // Z+
            wz = 1;
        } else if tz < cz {
            dir |= 0x4000; // Z-
            wz = -1;
        }
        if tx > cx {
            dir |= 0x2000; // X+
            wx = 1;
        } else if tx < cx {
            dir |= 0x8000; // X-
            wx = -1;
        }
        if dir != 0 {
            // Walking sets the heading, exactly as the pad path does (retail
            // locomotion writes the facing every moved frame) — so a nav walk
            // leaves the player facing its travel direction and the interact
            // probe ([`Self::field_interact_probe_slot`]) sees the same state
            // a pad walk would produce.
            self.actors[slot].move_state.render_26 =
                (((wx as f32).atan2(wz as f32) / std::f32::consts::TAU * 4096.0).round() as i32
                    & 0x0FFF) as i16;
            self.advance_with_collision(slot, dir, FIELD_BASE_STEP);
        }
        false
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
                    // Per-tile region refresh (the `FUN_800180EC` /
                    // `FUN_801DBA20` grain - retail re-runs the region scan
                    // when the player tile changes).
                    self.refresh_field_regions();
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
        // Clear any monster-slot SPD / accuracy / evasion left over from a
        // previous battle so this formation's values are the only ones seen.
        for s in self.battle_speed.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for s in self.battle_accuracy.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for s in self.battle_evasion.iter_mut().skip(party_count as usize) {
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
                if let Some(s) = self.battle_accuracy.get_mut(mslot) {
                    *s = def.accuracy as u16;
                }
                if let Some(s) = self.battle_evasion.get_mut(mslot) {
                    *s = def.evasion as u16;
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

mod battle;
mod encounters;
mod save;
mod vm_hosts;

#[cfg(test)]
mod tests;
