//! World-level constants, collision/probe tables, timeline budgets, and
//! small pure helpers extracted verbatim from `world.rs`.

use super::*;

/// Maximum simultaneous actors in the world. Mirrors the battle-side cap of
/// 8 + 32 spare slots for field-side NPCs / cutscene actors.
pub const MAX_ACTORS: usize = 64;

/// Number of stat-bearing battle slots (party + monsters). Indexes
/// [`World::battle_attack`] / [`World::battle_defense`] / [`World::battle_speed`]
/// and bounds the turn-order initiative scan.
pub(crate) const BATTLE_SLOTS: usize = 8;

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
pub(crate) const TILE_BOARD_SPEED: i32 = crate::tile_board::TILE / 8;

/// Bytes per row of the field collision grid (retail `0x80`-byte rows at
/// `*(_DAT_1F8003EC) + 0x4000`).
pub(crate) const FIELD_GRID_STRIDE: usize = 0x80;
/// Total field-collision-grid size: `0x80` rows of `0x80` bytes.
pub(crate) const FIELD_GRID_LEN: usize = FIELD_GRID_STRIDE * 0x80;
/// Base walk step (retail `base_step = 8` in `FUN_801d01b0`). Scaled by the
/// player's `+0x72` speed multiplier and the per-frame delta scalar.
pub(crate) const FIELD_BASE_STEP: i32 = 8;
/// Per-iteration advance of the locomotion step loop (retail commits in
/// 2-unit increments per axis).
pub(crate) const FIELD_STEP_UNIT: i32 = 2;
/// Retail player speed multiplier installed by the scene-entry map-init
/// `FUN_8003aeb0` (`player[+0x72] = 0x1000`, a `12.0` fixed-point `1.0`).
pub(crate) const FIELD_PLAYER_SPEED_MULT: u16 = 0x1000;

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
pub(crate) const FIELD_WALL_PROBES: [[(i16, i16); 3]; 4] = [
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
pub(crate) const FIELD_ACTOR_PROBES: [[(i16, i16); 3]; 4] = [
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
pub(crate) const FIELD_NPC_BOX_HALF: i32 = 0x40 - 0x18;

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
pub(crate) const FIELD_FACING_PROBES: [(i16, i16); 8] = [
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
pub(crate) const FIELD_INTERACT_BOX_HALF: i32 = 0x40 + 0x20 - 0x18;

/// Half-extent of a STATIC entity's (prop's) collision box: retail's
/// static arm always tests `|probe - centre| < 0x40 + 0x10` per axis (the
/// `0x10` is hard-coded, independent of the caller extents that widen the
/// moving-actor box) - ±80 units around the record-derived footprint centre.
pub(crate) const FIELD_PROP_BOX_HALF: i32 = 0x40 + 0x10;

/// Per-frame world-unit budget for one field-NPC motion-VM step.
///
/// Retail derivation (static, `FUN_8003774C` in `overlay_0897`, cases 0x37 /
/// 0x41 / 0x47 - the NPC glide/pursue path): the per-frame glide magnitude is
/// the global per-frame delta `_DAT_1f800393` scaled by a base step encoded
/// **in the motion op's own operand** (`4 << (operand bits)`; the 0x47 case
/// steps `0x80 / (4 << (op[2] & 7))` units per inner iteration, iterated
/// `_DAT_1f800393` times). The actor `+0x72` speed multiplier does **not**
/// participate: `FUN_8003774C` never reads `+0x72`. The `+0x72` multiplier
/// path (`speed = ((base_step * actor[+0x72]) >> 12) * _DAT_1f800393`) is
/// exclusive to the player free-movement controller `FUN_801d01b0`
/// (`overlay_0897_801d0684`) - the sole `>> 0xc` speed-scale in the corpus.
/// The task-B6 "+0x72 NPC glide path" premise is falsified by the dumps.
///
/// The engine drives NPC legs through a synthetic single-op `0x47` program
/// (see [`FIELD_NPC_MOTION_PROGRAM`]) whose operand bytes are zero, so the
/// motion VM cannot decode the base step from the program itself; instead the
/// engine derives each placement's glide speed from the placement's **real
/// walk-kernel operands** off the disc
/// ([`crate::man_field_scripts::placement_glide_speed`]: the MAN
/// tail-section-1 wander/step ops first, then the record's own field-VM
/// `0x37`/`0x41`/`0x47` yield ops) and writes it into the leg's
/// [`vm::motion_vm::MotionState::speed`]. This constant is the **fallback**
/// used when a placement carries no walk-kernel op at all (and for the
/// actor-VM sprite glide, which has no MAN motion operand): base step 8 =
/// `field_npc_glide_speed(2)`, so a placement with the default base-step
/// selector paces exactly as this stand-in did.
pub(crate) const FIELD_NPC_MOTION_SPEED: u16 = 8;

/// The shared retail walk-step formula: `numerator >> (2 + bits)` world units
/// per frame, floored at 1 so a leg always makes progress (the engine choice;
/// retail's integer division reaches 0 for `bits >= 6`, which no authored
/// stream uses). Both walk kernels step on this ladder:
///
/// - `FUN_8003774C` (field-VM yield ops, interpreted in place from the
///   pointer parked at actor `+0x94`): per-frame magnitude
///   `numerator * dt / (4 << bits)` - `numerator = 0x80` for ops
///   `0x37`/`0x47`, **`0x40` for op `0x41`** (the `li a1,0x40` / `li a1,0x80`
///   split at `0x80037908`), `dt = _DAT_1f800393` taken at 1 (one engine tick
///   = one retail update; the frame-step scalar is modelled by tick cadence,
///   not by scaling the step).
/// - `FUN_80038158` (MAN tail-section-1 motion streams): the directional
///   steps `0x03`/`0x19`/`0x20`, the pad-echo step `0x06`, and the AABB
///   wander `0x18` all move `0x80 >> (2 + bits)` per frame.
// PORT: FUN_8003774C (ops 0x37/0x41/0x47 step magnitude)
// REF: FUN_80038158 (ops 0x03/0x06/0x18/0x19/0x20 step magnitude)
pub(crate) fn field_npc_walk_step_speed(numerator: u16, bits: u8) -> u16 {
    (u32::from(numerator) >> (2 + u32::from(bits & 0xF))).max(1) as u16
}

/// Derive a field-NPC per-frame glide speed from a base-step selector - the
/// retail encoding of the walk-kernel ops' own operands (`b2 & 7` for 0x47,
/// `(op0>>5 & 4)|(op1>>6)` for 0x37/0x41, `b1 & 0xF` for the tail-section-1
/// steps 0x03/0x19/0x20, the scattered AABB-byte high bits for 0x06/0x18).
/// The `0x80`-numerator arm of [`field_npc_walk_step_speed`].
///
/// Retail `FUN_8003774C` (ops 0x37/0x47) glides at
/// `_DAT_1f800393 × 0x80 / (4 << bits)` units per frame, i.e. per-frame
/// magnitude `0x80 >> (2 + bits)` with the per-frame delta scalar
/// `_DAT_1f800393 = 1` (its cold-field value). The `+0x72` player speed
/// multiplier does NOT participate - `FUN_8003774C` never reads it - so this is
/// purely operand-derived, unlike the player's `FUN_801d01b0` walk step:
///
/// | `bits` | `4 << bits` | glide speed (`0x80 >> (2+bits)`) |
/// |--------|-------------|----------------------------------|
/// | 0 | 4 | 32 |
/// | 1 | 8 | 16 |
/// | 2 | 16 | 8 (= [`FIELD_NPC_MOTION_SPEED`]) |
/// | 3 | 32 | 4 |
/// | 4 | 64 | 2 |
/// | 5..=7 | 128..512 | 1 (floored) |
///
/// Clamped to a minimum of 1 so a leg always makes progress (retail never
/// stalls a glide at 0).
pub(crate) fn field_npc_glide_speed(base_step_bits: u8) -> u16 {
    field_npc_walk_step_speed(0x80, base_step_bits & 0x7)
}

/// Motion-VM bytecode for one field-NPC walk leg: a single `0x47`
/// `MoveTowardTarget` op (the pursue/glide opcode of `FUN_8003774C`). The
/// engine resets the cursor per leg, so the one-op program is the whole
/// script.
pub(crate) const FIELD_NPC_MOTION_PROGRAM: [u8; 1] =
    [vm::motion_vm::MotionOp::MoveTowardTarget as u8];

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

/// One NPC's **ambient facing** channel - the idle turn-in-place behaviour of
/// the second motion VM (`FUN_80038158` ops `0x04` / `0x0D`), driven by the
/// [`legaia_engine_vm::ambient_motion::AmbientMotion`] port.
///
/// The stream arrives as MAN tail-section 1: one motion record per bound
/// actor, each carrying a table of gated *variants*
/// ([`legaia_asset::man_motion::stream_variants`]). Retail's interpreter
/// preamble re-selects the live variant every tick - the first variant whose
/// `DAT_80085758` system flag is set, else the `0xFFFF` default - so the
/// selection is re-run per tick here too rather than frozen at scene load,
/// and a swap reseeds the VM cursor the way the retail preamble does.
#[derive(Debug, Clone)]
pub struct FieldNpcAmbient {
    /// `(selector, bytecode)` per variant, in header-table order. The
    /// selector is the raw `MotionVariant::selector`: `0xFFFF` = default,
    /// else `selector & 0xFFF` is the gating system-flag id.
    pub variants: Vec<(u16, Vec<u8>)>,
    /// Index into [`Self::variants`] the VM is currently running, or `None`
    /// before the first tick has selected one.
    pub live: Option<usize>,
    /// The ambient facing interpreter (PC, cursor, raw `+0x26` heading, and
    /// the actor's own slice of the `0x801C66A0` ramp pool).
    pub vm: vm::ambient_motion::AmbientMotion,
    /// Any variant of this stream carries a walk op (`0x03`/`0x19`/`0x20`
    /// directional step or `0x18` AABB wander), so this placement's motion
    /// is the ambient VM's business.
    ///
    /// Retail dispatches the two per-actor motion VMs off *different* actor
    /// flag bits (`+0x10 & 0x80` for this one, `& 0x400` for the pursue VM
    /// at `FUN_8003774C`), so no actor is ever walked by both. The engine
    /// keeps that exclusivity by standing the autonomous patrol substitute
    /// down for the slots this flag marks - scripted legs (interaction
    /// prologue, cutscene timeline) still win, exactly as they outrank a
    /// patrol.
    pub walks: bool,
}

impl FieldNpcAmbient {
    /// Retail's per-tick variant re-selection: the first flag-gated variant
    /// whose system flag is set, else the `0xFFFF` default. Returns an index
    /// into [`Self::variants`].
    ///
    /// REF: FUN_80038158 (interpreter preamble), FUN_8003A9D4 (binding)
    pub fn select_variant(&self, flag: impl Fn(u16) -> bool) -> Option<usize> {
        self.variants
            .iter()
            .position(|(sel, _)| {
                *sel != legaia_asset::man_motion::SELECTOR_DEFAULT && flag(*sel & 0x0FFF)
            })
            .or_else(|| {
                self.variants
                    .iter()
                    .position(|(sel, _)| *sel == legaia_asset::man_motion::SELECTOR_DEFAULT)
            })
    }
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

/// The off-map "hide box" world X/Z (`16320` = `0x3FC0`) a field script parks
/// an actor at to remove it from the scene.
///
/// Two uses share it: a MAN placement whose spawn is already this box is a
/// conditional actor retail hides until a script places it (skipped at NPC
/// build time), and the `town01` opening cutscene `MoveTo`s the townsfolk here
/// to clear the establishing shot, restoring them (via
/// [`crate::world::World::field_npc_positions`] fallback to the MAN spawn) when
/// the opening timeline completes. Both the build-time skip
/// (`legaia_engine_shell` NPC upload) and the completion restore key off this
/// value.
pub const FIELD_OFFMAP_HIDE_XZ: i16 = 16320;

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
pub(crate) const CUTSCENE_TIMELINE_STEP_BUDGET: u32 = 256;

/// Frame cap for a cutscene timeline that must return control (the `town01`
/// opening). If the spawned context never reaches its terminal within this
/// many frames (≈20 s at 60 fps), the engine forces it complete.
pub(crate) const CUTSCENE_TIMELINE_MAX_FRAMES: u32 = 1200;

/// Frame cap for the `opdeene` prologue timeline. The record arms the
/// hand-off bit near its TOP (`GFLAG_SET 26` at body `+0x17`) and then plays
/// the whole vignette choreography - camera beats, actor-channel pokes,
/// waits - for the narration's duration, so it must NOT complete on the bit;
/// it runs until the record reaches a terminal state, the player confirms the
/// hand-off (the scene change drops it), or this generous cap (≈60 s).
pub(crate) const PROLOGUE_TIMELINE_MAX_FRAMES: u32 = 3600;

/// Per-frame, per-channel field-VM step budget for the spawned per-actor
/// channels ([`World::step_field_channels`]). Retail slices end at a yield /
/// park / `0x21` NOP, normally within a handful of ops; the budget bounds a
/// malformed non-yielding stretch.
pub(crate) const FIELD_CHANNEL_STEP_BUDGET: u32 = 128;

/// Bound on the concurrent spawned-record contexts
/// ([`World::helper_contexts`]) and the pending op-`0x44` spawn queue
/// ([`World::pending_record_spawns`]). Retail's context table is a small
/// fixed actor-slot pool (`FUN_8003BDE0` allocates from the actor list), so
/// the container is bounded rather than open-ended; a scene never
/// legitimately runs this many helper records at once.
pub const SPAWNED_CONTEXT_SLOTS: usize = 8;

/// Frame budget for a cutscene-timeline channel-completion PARK (the
/// cross-context `CFLAG_TST` handshake, `B3 <id> <bit>`; see
/// [`World::step_cutscene_timeline`] and
/// [`crate::cutscene_timeline::ChannelWait`]).
///
/// Retail's halt-acquire / state-resume protocol parks the timeline at the
/// flag-test until the poked channel raises the completion bit - normally a
/// handful of frames while the channel's own script plays its beat and sets the
/// flag (`0x31 CFLAG_SET`). This bound caps that wait so a channel our port
/// cannot advance to its flag-set can't stall the timeline: on expiry the
/// timeline falls back to the by-width step-past (the pre-handshake behaviour),
/// keeping the prologue flowing. Sized to a couple of beat-lengths, well under
/// the [`CUTSCENE_TIMELINE_MAX_FRAMES`] / [`PROLOGUE_TIMELINE_MAX_FRAMES`] caps
/// so many parks in one record still complete comfortably.
pub(crate) const CHANNEL_WAIT_PARK_TIMEOUT: u32 = 30;

/// Park bound for a cross-context **walk-to-tile yield**
/// (`C7 <id> <tx> <tz> <mode>`, [`crate::cutscene_timeline::TimelineWalk`]).
/// A walk park is a real playout - the longest authored legs cross a dozen
/// tiles at the slowest operand speed (1 unit/tick ≈ 1500 ticks) - so this is
/// sized as a safety net, not a beat length; on expiry the walker snaps to
/// the op target so the choreography stays coherent.
pub(crate) const WALK_PARK_TIMEOUT: u32 = 2400;

/// Move `cur` toward `target` by at most `max_delta`, snapping exactly
/// onto `target` when within range. Used by the tile-board interpolator.
pub(crate) fn step_toward(cur: i32, target: i32, max_delta: i32) -> i32 {
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
pub(crate) fn tile_step_from_input(
    input: &input::InputState,
) -> Option<crate::tile_board::TileStep> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_npc_glide_speed_derives_the_retail_base_step_ladder() {
        // `0x80 >> (2 + (bits & 7))`, floored at 1. bits==2 reproduces the
        // FIELD_NPC_MOTION_SPEED stand-in exactly, so the default path is
        // behaviourally unchanged.
        assert_eq!(field_npc_glide_speed(0), 32);
        assert_eq!(field_npc_glide_speed(1), 16);
        assert_eq!(field_npc_glide_speed(2), 8);
        assert_eq!(field_npc_glide_speed(2), FIELD_NPC_MOTION_SPEED);
        assert_eq!(field_npc_glide_speed(3), 4);
        assert_eq!(field_npc_glide_speed(4), 2);
        // 5..=7 floor at 1 (retail never stalls a glide at 0).
        assert_eq!(field_npc_glide_speed(5), 1);
        assert_eq!(field_npc_glide_speed(6), 1);
        assert_eq!(field_npc_glide_speed(7), 1);
        // Only the low 3 bits select the base step (matches `op[2] & 7`).
        assert_eq!(field_npc_glide_speed(0xF8), field_npc_glide_speed(0));
        assert_eq!(field_npc_glide_speed(0x82), field_npc_glide_speed(2));
    }
}
