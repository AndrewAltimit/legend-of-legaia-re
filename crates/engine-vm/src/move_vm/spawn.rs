//! Move-VM spawn entry point (`FUN_80021B04`): allocate an actor and kick it running a move buffer.

// ---------------------------------------------------------------------------
// Move-VM spawn entry point: FUN_80021B04.
//
// One-shot "allocate an actor that will run a move buffer" helper. Composes
// the actor allocator (`ActorAllocatorHost::spawn_at_position` +
// `rebuild_object_table`) with an init-word-keyed per-actor state setup and
// a single move-VM kick. Callers are engine-side spawn sites (script-VM op 3
// 3D-anim play, world-map spawn paths, ad-hoc move spawns).
// ---------------------------------------------------------------------------

use crate::actor_alloc::{ActorAllocatorHost, ActorHandle, SpawnPosition};

/// Pool selectors retail passes to `FUN_80020DE0` from `FUN_80021B04`.
///
/// Forwarded verbatim into [`ActorAllocatorHost::spawn_at_position`] for
/// engines that key by pool; single-pool engines ignore them. The retail
/// values are `(&DAT_8007062C, _DAT_8007C350)`; we encode them as the raw
/// `0x8007_xxxx` addresses so the call site reads identically against the
/// `funcs/80021b04.txt` dump.
pub const MOVE_SPAWN_POOL_A: u32 = 0x8007_062C;
pub const MOVE_SPAWN_POOL_B: u32 = 0x8007_C350;

/// Classification of the move buffer's leading word at `*move_buffer`.
///
/// Selects the per-actor sub-state init pattern in [`spawn_move_actor`].
/// Decoded by [`SpawnSubmode::classify`].
///
/// Retail equivalent: the four-way branch in `FUN_80021B04` at
/// `0x80021bd0..0x80021d3c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnSubmode {
    /// Init word has the high bit set (negative as `i16`).
    ///
    /// Retail writes: clear `+0x56` and `+0x5A`, set bit `0x2` in flags,
    /// skip the OBJECT-table rebuild, skip the type-keyed slot clear.
    Negative,
    /// Init word == `0x4000`.
    ///
    /// Retail writes: `+0x5A = 3`, `+0x56 = 0`, set bit `0x2` in flags,
    /// clear keyframe slots `+0x9C` / `+0x9E` / `+0xA8`.
    Keyframe,
    /// Init word == `0x4001`.
    ///
    /// Retail writes: `+0x5A = 5`, `+0x56 = 0`, set bit `0x2` in flags,
    /// clear tween slots `+0x98` / `+0x9A` / `+0xB0..+0xB8`; `+0xB0` then
    /// receives `0xFFFF`.
    Tween,
    /// Init word is non-negative and not in `{0x4000, 0x4001}`.
    ///
    /// Retail writes: `+0x5A = 1`, set bit `0x08000000` in flags, run
    /// the OBJECT-table rebuild, clear render scratch
    /// (`+0x80..+0x84`, `+0x90..+0x9A`, `+0xC0..+0xCA`), then write
    /// `+0x96 = rot[1] & 0xFFF`.
    Default,
}

impl SpawnSubmode {
    /// Classify the move buffer's init word (`*move_buffer`) per the
    /// retail four-way branch.
    pub fn classify(init_word: u16) -> Self {
        let signed = init_word as i16;
        if signed < 0 {
            Self::Negative
        } else if init_word == 0x4000 {
            Self::Keyframe
        } else if init_word == 0x4001 {
            Self::Tween
        } else {
            Self::Default
        }
    }
}

/// Parameter bundle for [`spawn_move_actor`].
///
/// Mirrors the four args to `FUN_80021B04(pos, rot, move_buffer, param_4)`:
///
/// - `pos` ← `param_1` (3 u16s read as `(x, y, z)`).
/// - `rot` ← `param_2` (3 u16s; written to `+0x24..+0x28` and, in the
///   `Default` arm, masked into `+0x96`).
/// - `init_word` ← `*param_3` (the move buffer's leading u16; classified
///   into [`SpawnSubmode`]).
/// - `seq_word` ← `param_4` (written to `+0x72`).
///
/// The move-buffer *pointer* itself (`param_3`) is not carried here - the
/// clean-room engine binds the buffer via [`crate::move_buffer::MoveBufferHost`]
/// rather than stashing a raw pointer at `actor[+0x48]`. Engines that need a
/// per-actor reference back to the buffer track it through their own
/// channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveSpawnRequest {
    pub pos: SpawnPosition,
    pub rot: [u16; 3],
    pub init_word: u16,
    pub seq_word: u16,
}

/// Host hooks unique to the move-VM spawn entry point.
///
/// Extends [`ActorAllocatorHost`] (which already supplies `spawn_at_position`
/// and `rebuild_object_table`) with the three FUN_80021B04 stages the
/// allocator alone can't express:
///
/// 1. [`apply_move_spawn_state`] - per-arm field writes that depend on the
///    classified [`SpawnSubmode`].
/// 2. [`kick_move_vm`] - the final `FUN_80023070(actor)` call.
/// 3. [`mirror_world_y`] - the trailing `actor[+0x2A] = actor[+0x16]` copy.
///
/// [`apply_move_spawn_state`]: MoveSpawnHost::apply_move_spawn_state
/// [`kick_move_vm`]: MoveSpawnHost::kick_move_vm
/// [`mirror_world_y`]: MoveSpawnHost::mirror_world_y
pub trait MoveSpawnHost: ActorAllocatorHost {
    /// Apply the per-submode actor-state writes documented on
    /// [`SpawnSubmode`]. Engines write into their own actor representation;
    /// the trait only carries the classification and the spawn-time inputs.
    fn apply_move_spawn_state(
        &mut self,
        actor: ActorHandle,
        submode: SpawnSubmode,
        req: &MoveSpawnRequest,
    );

    /// Run one move-VM tick on the freshly-spawned actor. Retail equivalent
    /// is the unconditional `jal FUN_80023070` at the tail of FUN_80021B04.
    fn kick_move_vm(&mut self, actor: ActorHandle);

    /// Copy `actor[+0x16]` (world_y) into `actor[+0x2A]` (world_y_mirror).
    /// Retail equivalent is the final two instructions of FUN_80021B04.
    fn mirror_world_y(&mut self, actor: ActorHandle);
}

/// Allocate an actor and launch it on a move buffer.
///
/// PORT: FUN_80021B04
///
/// NOT WIRED: the host side is ready - `impl MoveSpawnHost for World` lives in
/// `legaia_engine_core::actor_alloc_host` - but nothing in the engine spawns a
/// move-VM actor. Both live actor paths construct through the world's own
/// pool: the field path spawns from MAN placements and the battle path from
/// the formation, and neither routes a `MoveSpawnRequest` through the retail
/// allocator. The prerequisite is a spawn site that *starts from a move
/// buffer* - retail's are the script-VM 3D-anim play arm and the world-map
/// spawn paths, neither of which is ported - so only tests drive this entry
/// point today.
///
/// One-shot composition that mirrors the SCUS body at
/// `ghidra/scripts/funcs/80021b04.txt`:
///
/// 1. Classify `req.init_word` (see [`SpawnSubmode`]).
/// 2. Allocate the actor through [`ActorAllocatorHost::spawn_at_position`]
///    (which folds in the pos copy + GTE transform). Returns `None` on
///    allocator failure - matches the retail `iVar3 == 0` branch.
/// 3. For non-[`SpawnSubmode::Negative`] arms, run
///    [`ActorAllocatorHost::rebuild_object_table`] so the per-actor
///    OBJECT-table pointer (`actor[+0x44]`) is populated from
///    `DAT_8007C018[actor[+0x64]]`.
/// 4. Apply the per-submode state writes via
///    [`MoveSpawnHost::apply_move_spawn_state`].
/// 5. Kick the move VM once via [`MoveSpawnHost::kick_move_vm`].
/// 6. Mirror `world_y → world_y_mirror` via
///    [`MoveSpawnHost::mirror_world_y`].
///
/// The retail body additionally writes `DAT_80070630` (a global scratch
/// slot) on entry. The slot has no SCUS reader outside FUN_80021B04 itself
/// in the captured corpus; the clean-room port drops it. See
/// `find_addr_materializer_dat_80070630.py` (TODO if a reader surfaces) for
/// the readback search.
pub fn spawn_move_actor<H: MoveSpawnHost + ?Sized>(
    host: &mut H,
    req: MoveSpawnRequest,
) -> Option<ActorHandle> {
    let submode = SpawnSubmode::classify(req.init_word);
    let actor = host.spawn_at_position(req.pos, MOVE_SPAWN_POOL_A, MOVE_SPAWN_POOL_B)?;
    if submode != SpawnSubmode::Negative {
        host.rebuild_object_table(actor);
    }
    host.apply_move_spawn_state(actor, submode, &req);
    host.kick_move_vm(actor);
    host.mirror_world_y(actor);
    Some(actor)
}
