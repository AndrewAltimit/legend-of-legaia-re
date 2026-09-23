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
/// Retail is **not** a four-way branch. `FUN_80021B04` tests the init word
/// twice, on two independent predicates:
///
/// 1. `bltz` on the sign (`0x80021bd8`) picks the OBJECT-table rebuild arm.
/// 2. `sltiu (word - 0x4000), 2` (`0x80021cc0`) picks the keyframe / tween
///    arms, and its **else** branch is the render-scratch clear at
///    `0x80021d3c` that also seeds `+0x96` from `rot[1]`.
///
/// A negative init word is negative *and* outside `{0x4000, 0x4001}`, so it
/// takes arm 1's negative side **and** arm 2's else side - it does not skip
/// the render-scratch clear. [`SpawnSubmode::clears_render_scratch`] is the
/// second predicate; the four variants here are the joint classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnSubmode {
    /// Init word has the high bit set (negative as `i16`).
    ///
    /// Retail writes: clear `+0x56` and `+0x5A`, set bit `0x2` in flags, skip
    /// the OBJECT-table rebuild - and, because the second predicate is on the
    /// value and not the sign, **also** run the render-scratch clear
    /// ([`SpawnSubmode::clears_render_scratch`]) with its `+0x96 = rot[1] &
    /// 0xFFF` write. This is the dominant kind: it is what every
    /// transform/pivot stager record uses.
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
    /// retail branches.
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

    /// Retail's **second** predicate, `!(init_word - 0x4000 <u 2)`: whether the
    /// spawn runs the render-scratch clear at `0x80021d3c` (`+0xC0..+0xCA`,
    /// `+0x80..+0x84`, `+0x90..+0x9A` zeroed, then `+0x96 = rot[1] & 0xFFF`).
    ///
    /// True for [`Negative`](Self::Negative) as well as
    /// [`Default`](Self::Default) - the two arms that are *not* one of the two
    /// render-mode-node values. The sign of the init word does not enter this
    /// test, which is the part a four-way reading loses.
    pub fn clears_render_scratch(self) -> bool {
        !matches!(self, Self::Keyframe | Self::Tween)
    }

    /// Retail's **first** predicate, `bgez` on the init word: whether the
    /// spawn runs the OBJECT-table rebuild (`actor[+0x44]` from
    /// `DAT_8007C018[actor[+0x64]]`) and takes `+0x5A = 1` /
    /// `flags |= 0x08000000` before any later arm overwrites them.
    pub fn rebuilds_object_table(self) -> bool {
        !matches!(self, Self::Negative)
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
/// engine binds the buffer via [`crate::move_buffer::MoveBufferHost`]
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
/// REPLACED-BY: `legaia_engine_core::world::ambient` - `spawn_ambient_record_at`
/// and the `push_ambient_part` / `tick_ambient_part` pair under it, which carry
/// this same routine and are live on both hosts.
///
/// The earlier reading of this row looked for a missing *caller*. There is no
/// missing caller: retail reaches `FUN_80021B04` from one place, the field
/// VM's op-`0x34` sub-3 arm through the stager `FUN_800252EC`, and the engine
/// runs that route through the ambient pool - with the op-`0x25` fan-out, the
/// CLUT-cell integrator and the VDF morph envelope the trait shape here does
/// not carry. Routing the arm through this entry point instead would replace
/// a fuller port with a thinner one.
///
/// What survives here is the **trait-shaped** statement of the same body,
/// useful as the documented one-shot composition and as the seat any future
/// host that really does start from a move buffer would use. No host is owed
/// it.
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
/// Before allocating, the retail body writes `DAT_80070630`
/// (`0x80021B50..0x80021B68`): [`template_model_index`] of the init word.
/// That halfword is not a readerless scratch slot - it is `+4` of the spawn
/// descriptor `0x8007062C` the allocator is then handed, and
/// `FUN_80020DE0` copies descriptor `+4` into the new actor's `+0x64` and
/// `+0x60` (`0x80020E68..0x80020E7C`). The OBJECT-table rebuild then reads
/// `+0x64` back (`lh v0, 0x64(s0)` at `0x80021BF8`) to index
/// `DAT_8007C018`. So the model the rebuild resolves is the init word plus
/// the scene's model base, which is what
/// [`ActorAllocatorHost::rebuild_object_table`] has to be told.
/// The model index `FUN_80021B04` stages into its spawn descriptor
/// (`DAT_80070630`, descriptor `0x8007062C + 4`) before allocating: the init
/// word plus the scene model base `gp[0x754]` (a `lhu`, added and stored as a
/// halfword) when the init word is a model reference, and `0` for the three
/// non-model classes - negative, `0x4000` and `0x4001` (`bltz` / two `beq`
/// at `0x80021B34..0x80021B48`). `FUN_80020DE0` seats it at actor `+0x64` /
/// `+0x60`.
///
/// REF: FUN_80020DE0
pub fn template_model_index(init_word: u16, scene_model_base: u16) -> u16 {
    let signed = init_word as i16;
    if signed < 0 || init_word == 0x4000 || init_word == 0x4001 {
        0
    } else {
        init_word.wrapping_add(scene_model_base)
    }
}

pub fn spawn_move_actor<H: MoveSpawnHost + ?Sized>(
    host: &mut H,
    req: MoveSpawnRequest,
) -> Option<ActorHandle> {
    let submode = SpawnSubmode::classify(req.init_word);
    let actor = host.spawn_at_position(req.pos, MOVE_SPAWN_POOL_A, MOVE_SPAWN_POOL_B)?;
    if submode.rebuilds_object_table() {
        host.rebuild_object_table(actor);
    }
    host.apply_move_spawn_state(actor, submode, &req);
    host.kick_move_vm(actor);
    host.mirror_world_y(actor);
    Some(actor)
}

// ---------------------------------------------------------------------------
// Part-actor pool teardown: FUN_80050E74.
//
// REF: FUN_80050ed4 - the allocator that seats a part into this pool.
// REF: FUN_800480d8 - the battle-scene teardown loop over the same table,
//                     ported as legaia_engine_render::battle_actor_tick.
// ---------------------------------------------------------------------------

/// Slots in the part-actor pool `DAT_801C90F0` (`slti a1, 0x80`).
///
/// The same table `FUN_80050ED4` allocates into: it hands back the first null
/// slot, stores the actor `FUN_80021B04` produced, and the summon / special-
/// attack stagers then own that seat for the length of the attack.
pub const PART_POOL_SLOTS: usize = 0x80;

/// The flag bit the pool flush raises on every actor it retires - the same
/// `+0x10` bit `0x8` move-VM op `0x08` `HALT` sets, and the same bit the
/// battle-scene teardown pass (`FUN_800480D8`) then tests before releasing a
/// seat it did not raise itself.
pub const PART_ACTOR_HALT_FLAG: u32 = 0x8;

/// Retire one seated part-actor: the three writes the flush makes per slot.
///
/// Clearing the wait timer and the `0x18`/`0x19` loop counter matters as much
/// as the flag does. `move_vm::actor_tick` skips an actor whose timer has not
/// expired, so a part parked in a long `WAIT_SET` would outlive the flush;
/// and an actor sitting inside an open loop-back would re-enter it. Zeroing
/// both means the actor's very next tick reaches the halt bit.
///
/// PORT: FUN_80050e74 (`0x80050E90..0x80050EB8`)
///
/// REPLACED-BY: `World::casting.active_summon`
/// (`legaia_engine_core::summon::SummonScene::parts`), the engine list that
/// holds the population `DAT_801C90F0` seats - the 89 `jal` sites to the flush
/// are all in the summon / special-attack stager overlays (PROT 0911..0969).
/// That scene is replaced or dropped whole when a cast ends, so every part is
/// retired at once with no seat left to walk and no actor left needing the
/// halt bit that tells the battle teardown pass to collect it.
pub fn halt_part_actor(actor: &mut crate::move_vm::ActorState) {
    actor.wait_timer = 0;
    actor.field_8c = 0;
    actor.flags |= PART_ACTOR_HALT_FLAG;
}

/// Empty the part-actor pool: [`halt_part_actor`] every seated slot, then
/// null it. Returns how many seats were retired.
///
/// Retail is unconditional - every non-null slot, with no test on the actor.
/// That is what separates it from the battle-scene teardown loop inside
/// `FUN_800480D8`, which walks the same table and releases only the seats
/// whose actor **already** carries [`PART_ACTOR_HALT_FLAG`]. The two are the
/// raise and the collect of one protocol, not two copies of it.
///
/// The 89 `jal` sites are all in the summon / stager overlays, PROT 0911..0969
/// (the `summon.dat` / `readef.DAT` per-special-attack band), which is what
/// fixes the meaning: a special attack flushes the parts it spawned when its
/// sequence ends.
///
/// PORT: FUN_80050e74
///
/// REPLACED-BY: `World::casting.active_summon`, dropped whole at the end of a
/// cast - the same replacement as [`halt_part_actor`], one level up.
pub fn flush_part_actor_pool(slots: &mut [Option<&mut crate::move_vm::ActorState>]) -> usize {
    let mut retired = 0;
    for slot in slots.iter_mut().take(PART_POOL_SLOTS) {
        if let Some(actor) = slot.take() {
            halt_part_actor(actor);
            retired += 1;
        }
    }
    retired
}
