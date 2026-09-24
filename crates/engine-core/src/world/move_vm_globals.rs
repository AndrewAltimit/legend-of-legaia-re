//! Move-VM globals: per-actor bytecode and buffer pools, the shared predicate / counter / slot-table words, scratchpad ramp targets and the per-tick outcomes.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Move-VM globals: per-actor bytecode and buffer pools, the shared predicate / counter / slot-table words, scratchpad ramp targets and the per-tick outcomes.
pub struct MoveVmGlobals {
    /// Per-actor move-VM bytecode buffers. Indexed by actor slot. Empty
    /// vec means "no active move" - the move VM is not ticked for that
    /// actor. Set via [`crate::world::World::set_move_bytecode`].
    pub bytecode: Vec<Vec<u16>>,
    /// MOVE buffer pool root, mirroring retail `_DAT_8007B888`. Populated
    /// per scene from the slot-1 `Asset(0x05) = Move` descriptor (the
    /// MDT-shaped offset-table blob parsed by [`legaia_mdt::MoveBuffer`]).
    /// Consumed by the [`vm::move_buffer::MoveBufferHost`] impl in
    /// `move_buffer_host.rs`. Empty when no scene MOVE table is wired
    /// (the cursor's resolver returns `None` and the per-actor state
    /// stays idle, matching retail when the table pointer is null).
    pub buffer_root: Vec<u8>,
    /// MOVE2 buffer pool root, mirroring retail `_DAT_8007B840`. Used
    /// when the per-actor `cursor_requested` is `>= 0x400`. Empty
    /// across most retail save states; only a small number of scenes
    /// install this. See `docs/formats/mdt.md`.
    pub buffer2_root: Vec<u8>,
    /// Alternate MOVE buffer pool root, mirroring retail `_DAT_8007B75C`.
    /// Selected by [`vm::move_buffer::STATUS_FLAG_ALT_POOL`] in the
    /// per-actor status flag word. Populated by the world-map / battle
    /// overlay paths.
    pub buffer_alt_root: Vec<u8>,
    /// Per-actor [`TickEvent`]s emitted by the last
    /// [`crate::world::World::tick_actor_physics`] pass. Engines that want to react
    /// to audio cues, render submissions, or unlink requests drain
    /// this each frame; the move-buffer cursor kick is dispatched
    /// inline so callers do not need to inspect this list to keep
    /// move-VM playback running.
    pub last_tick_events: Vec<(u8, TickResult)>,
    /// Move-VM global predicate at `_DAT_801F22F4` (set by ext sub-op 0x08,
    /// cleared by 0x09; sub-ops 0x0A / 0x0B branch on it).
    pub predicate: u32,
    /// Move-VM global counter at `_DAT_801F22F6` (cleared by ext sub-op 0x0F,
    /// cycled mod 16 by sub-op 0x10).
    pub counter: u16,
    /// Move-VM 16-slot 8-byte-stride scratch table at `&DAT_801F3498`. Used
    /// by ext sub-ops 0x11 / 0x12 / 0x25 / 0x27 / 0x28 / 0x31 / 0x32 / 0x34
    /// / 0x35 to checkpoint world coords + tween state per actor / animation.
    pub slot_table: [[u8; 8]; 16],
    /// Move-VM axis offset at `_DAT_8007C348` - used by ext sub-ops 0x36 / 0x37
    /// for the `0x8E - axis` threshold predicate. Engines write per-scene.
    pub axis_threshold: i16,
    /// Move-VM scratchpad ramp ratio numerator at `_DAT_1F800393` - used by
    /// ext sub-op 0x23 (anim-bank lerp) as the numerator of a 12.0 fixed-point
    /// ratio against the operand-supplied denominator.
    pub ramp_ratio: u8,
    /// Move-VM `_DAT_8007B9D8` - globally-shared 32-bit slot written by ext
    /// sub-op 0x2F. Engines read this on whatever frame-tick they want.
    pub dat_8007b9d8: i32,
    /// Move-VM 16-slot scratchpad ramp targets at `_DAT_1F80035C` - used by
    /// ext sub-op 0x29 (per-frame ramp / immediate write). Stored as i16
    /// pairs (target, current); engines apply per-frame interpolation.
    pub scratchpad_targets: [i16; 16],
    /// Actor-VM glide targets (op `0x09` `MotionAt` → `start_motion`,
    /// retail `FUN_800358c0`), keyed by actor slot: each entry glides the
    /// actor's `move_state` `(world_x, world_y)` toward the target through
    /// the motion VM, one step per tick (`Self::tick_actor_motions`).
    pub actor_motions: std::collections::BTreeMap<u8, FieldNpcMotion>,
    /// Per-actor move-VM outcomes from the most recent [`crate::world::World::tick_move_vms`]
    /// call. Pairs of `(actor_slot, outcome)`. Engines drain or inspect this
    /// after `World::tick` to react to halts / pending opcodes.
    pub outcomes: Vec<(u8, vm::move_vm::ActorTickOutcome)>,
    /// Move-VM extension sub-op `0x2C` executions since the last draw - the
    /// scanline strip emitter `FUN_801D31B0`'s inputs, captured at the step
    /// and drawn by [`crate::world::World::move_strip_render_step`] on the
    /// host's render pass. Capped at [`MOVE_STRIP_REQUEST_CAP`].
    pub strip_requests: Vec<vm::move_ext_strip::StripRequest>,
}

/// Most strip requests [`MoveVmGlobals::strip_requests`] holds between two
/// draws. A host that never draws (a headless tick loop) would otherwise grow
/// the queue without bound; retail has no queue at all - it emits into the
/// frame's primitive buffer on the spot.
pub const MOVE_STRIP_REQUEST_CAP: usize = 64;

impl MoveVmGlobals {
    pub fn new() -> Self {
        Self {
            bytecode: vec![Vec::new(); MAX_ACTORS],
            buffer_root: Vec::new(),
            buffer2_root: Vec::new(),
            buffer_alt_root: Vec::new(),
            last_tick_events: Vec::new(),
            predicate: 0,
            counter: 0,
            slot_table: [[0u8; 8]; 16],
            axis_threshold: 0,
            ramp_ratio: 0,
            dat_8007b9d8: 0,
            scratchpad_targets: [0; 16],
            actor_motions: std::collections::BTreeMap::new(),
            outcomes: Vec::new(),
            strip_requests: Vec::new(),
        }
    }
}

impl MoveVmGlobals {
    /// Queue one sub-op `0x2C` execution for the next draw, dropping it once
    /// [`MOVE_STRIP_REQUEST_CAP`] requests are already waiting.
    pub fn push_strip_request(&mut self, req: vm::move_ext_strip::StripRequest) {
        if self.strip_requests.len() < MOVE_STRIP_REQUEST_CAP {
            self.strip_requests.push(req);
        }
    }

    /// Drain the queued strip requests - what a host's draw path hands the
    /// shared render step (`legaia_engine_ui::move_strip::move_strip_prims`).
    pub fn take_strip_requests(&mut self) -> Vec<vm::move_ext_strip::StripRequest> {
        std::mem::take(&mut self.strip_requests)
    }
}

impl Default for MoveVmGlobals {
    fn default() -> Self {
        Self::new()
    }
}
