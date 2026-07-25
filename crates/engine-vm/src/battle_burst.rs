//! The two-mode battle effect burst - move-VM opcode `0x17`.
//!
//! PORT: FUN_801F30C4
//! REF: FUN_80023070, FUN_80021B04, FUN_80050ED4
//!
//! `(actor, mode)`. The first argument is an **actor**, not a free-standing
//! record: the entry is the battle-side escape opcode of the move VM, so its
//! caller is `FUN_80023070` case `0x17` and `mode` is that instruction's single
//! operand. It is the exact sibling of the field escape `0x2F`
//! ([`crate::move_vm_overlay_ext`]) - `0x17` only does anything while the
//! battle overlay `0898` is resident.
//!
//! What it does: four iterations round the compass, three spawn blocks each, so
//! **twelve child actors** per call. Each child is seated on one of two static
//! move-VM stager records in `0898`'s tail, at the parent's world position, with
//! the parent's rotation triple copied through and its **Y component jittered**.
//! `mode` picks the arm; anything but `0` or `1` falls straight to the epilogue.
//!
//! Read from a disassembly of the mapped `0898` image at base `0x801CE818`.
//! `disasm-overlay-fn.py` historically could not be used here - it stopped at
//! the first unconditional `j` and reported 18 instructions for this entry - so
//! the span `0x801F30C4..0x801F398C` was disassembled with raw capstone. It
//! really does end where `func_0x801F3990` begins: `0x801F3988` is the `jr ra`
//! and `0x801F3990` a clean `addiu sp, sp, -0x20` prologue. 563 instructions.
//!
//! ## The entry is one loop written twice
//!
//! The three-way fork on the second argument reaches two loop bodies of 260
//! instructions each. Diffed instruction by instruction they are **identical
//! except for twelve**, three of which are only the loop-latch shape (arm `0`
//! exits on `beqz` and jumps back, arm `1` falls through on `bnez`). The nine
//! real differences are three constants repeated once per spawn block:
//!
//! | | arm `0` ([`BurstMode::Wide`]) | arm `1` ([`BurstMode::Narrow`]) |
//! |---|---|---|
//! | stager record | `0x801F5DA4` | `0x801F5D0C` |
//! | cosine divisors | `/48`, `/72`, `/96` | `/96`, `/144`, `/192` |
//! | tail offsets | `+0x70`, `+0xA8`, `+0x38` | `+0x30`, `+0x48`, `+0x18` |
//!
//! Two exact relations fall out, and [`arm_invariants_hold`] checks them rather
//! than leaving them as prose: every narrow cosine divisor is **twice** its wide
//! counterpart (the divide is the same magic multiply with one extra `sra`), and
//! every narrow tail offset is exactly **3/7** of its wide counterpart. So
//! `mode` selects the same burst at a smaller radius, not a different effect.
//!
//! ## Where each block's three values land
//!
//! Naming them by their destination rather than by shape, because the
//! destinations are what makes the routine legible:
//!
//! * **`yaw`** - folded into the *second halfword* of the eight bytes copied out
//!   of the parent's `+0x24..+0x2B`, i.e. `rot[1]` of the triple handed to
//!   `FUN_80021B04` as `param_2`. The seater masks that to 12 bits into the
//!   child's `+0x96` ([`crate::move_vm::ActorState::tween_scale_x`]), which move-VM
//!   op `0x03` uses as the rotation-LUT index - so this value **is** the child's
//!   heading, modulo the 4096-step circle.
//! * **`spread`** - stored at the child's `+0x3E`
//!   ([`crate::move_vm::ActorState::anim_3e`]), the middle of the `+0x3C..+0x40`
//!   triple.
//! * **`tail`** - stored at the child's `+0x98`
//!   ([`crate::move_vm::ActorState::tween_scale_y`]), the middle of the
//!   `+0x96..+0x9A` triple whose first element is the yaw above. The store is
//!   `sh v0, 0x18(s0)` after an `addiu s0, s0, 0x80`, i.e. the same `actor+0x80`
//!   base the seater's generic arm zeroes `+0x98` through - and the burst writes
//!   it *after* the spawn returns, so it survives that clear.
//!
//! ## Angles
//!
//! Block `0` indexes the trig LUTs at `iteration * 1024` - the four cardinals -
//! while blocks `1` and `2` share `(iteration * 1024 + 512) & 0xFFF`, the four
//! diagonals. Only the diagonal arm masks (`andi 0xfff` at `0x801F32A8`); the
//! cardinal arm is a bare `sll $s1, $s2, 0xb`, and over the loop's four
//! iterations the two agree, which is why [`lut_index`] models them separately
//! instead of masking both.
//!
//! Block `2` reuses block `1`'s index register rather than recomputing it.
//!
//! ## The scale argument is not uniform across the three blocks
//!
//! Blocks `0` and `1` load the parent's `+0x72` with **`lhu`** and pass
//! `>> 1` (a logical shift, in the `jal`'s delay slot). Block `2` loads it with
//! **`lh`** and passes it **unshifted** - the delay slot there carries
//! `move $a1, $s5` instead. Both arms agree on this, so it is the block that
//! decides, not the mode: see [`SpawnBlock::scale_halved`] and
//! [`SpawnBlock::child_scale`]. The value becomes the child's own `+0x72`
//! (`sh s4, 0x72(s0)` in `FUN_80021B04`), so blocks `0` and `1` spawn at half
//! the parent's scale and block `2` at full.
//!
//! ## Reciprocal divides
//!
//! Fourteen distinct (magic, shift, divisor) triples across the two arms, and
//! **every one is checked against plain truncating division** over a dense band
//! plus the 32-bit signed boundaries - a reciprocal that is nearly the divide it
//! looks like is the classic way this goes silently wrong, and it has gone wrong
//! here before. The shift is the part that gets dropped: `0x2AAAAAAB` is `/6`
//! read bare, `/48` with its `>> 3`, `/96` with `>> 4` and `/192` with `>> 5` -
//! all four appear in this one function. `0x88888889` is the **signed
//! magic-with-add** form (`mfhi`, `addu` the original, then `sra 3`) and needs
//! signed arithmetic to reproduce; it is `/15`. All of them are used as
//! `x - (x / d) * d`, i.e. a modulo, except the three cosine divides.
//!
//! [`reciprocal_mod`] and [`signed_shift_div`] carry the verification as tests
//! rather than as a claim in prose.
//!
//! # NOT WIRED
//!
//! Nothing in the engine spawns a move-VM actor, which is what this entry does
//! twelve of. The specific missing input is the same one
//! [`crate::move_vm::spawn::spawn_move_actor`] names: a spawn site that *starts
//! from a move buffer*. [`BurstHost::spawn`] is the seam, and a host that wants
//! to serve it needs two things:
//!
//! * an actor pool that can seat a move buffer - `impl MoveSpawnHost for World`
//!   exists in `legaia_engine_core::actor_alloc_host`, but no live path routes a
//!   `MoveSpawnRequest` through it; and
//! * the arm's stager record, which [`BurstRecord::parse`] slices out of a
//!   supplied `0898` image. It is disc data - the parser reads it at load time
//!   and **none of its bytes are reproduced here**.
//!
//! `MoveHost::ext_17` is the call-in seam on the other side; it is likewise a
//! default no-op until a host owns a pool.
//!
//! ## The spawn call itself is not a boundary
//!
//! Retail's spawn is `FUN_80050ED4(actor + 0x14, rot_scratch, record,
//! scale)`, which is decoded (`see ghidra/scripts/funcs/80050ed4.txt`): it scans
//! the 0x60-slot pointer pool at `DAT_801C90F0` for the first null entry, calls
//! `FUN_80021B04` with the same four arguments (sign-extending the low halfword
//! of the fourth on the way), stores the returned actor pointer into that slot,
//! and returns it - or returns `0` when all 96 slots are taken. It is carried in
//! the port catalog's ignore list as subsumed glue, because the pool scan is the
//! engine's own pool and the behaviour is
//! [`crate::move_vm::spawn::spawn_move_actor`]. A full pool means **no child is
//! spawned at all** and the burst's two post-spawn stores would fault on a null
//! pointer in retail; [`BurstHost::spawn`] returns an `Option` so a port can
//! model the exhausted pool without inventing that fault.

use crate::move_vm::{ActorState, MoveHost, StepResult, step};

/// Iterations each arm runs (`slti $v0, $s2, 4`).
pub const ITERATIONS: u32 = 4;
/// Spawn blocks per iteration - three `FUN_80050ED4` calls.
pub const SPAWNS_PER_ITERATION: usize = 3;
/// Draws per iteration (`FUN_80056798` x3 per block).
pub const DRAWS_PER_ITERATION: usize = 9;
/// Offset the burst adds to the spawned actor pointer before its second store
/// (`addiu $s0, $s0, 0x80`). The store is then `sh v0, 0x18(s0)`, so the field
/// is `actor + 0x98`.
pub const RECORD_STRIDE: i32 = 0x80;
/// Bytes copied from the parent's `+0x24` into the stack scratch by the
/// `lwl`/`lwr` pair at the top of every block - the rotation triple plus one
/// trailing halfword.
pub const SCRATCH_BYTES: usize = 8;
/// Offset of the halfword inside that scratch the yaw term lands on
/// (`sh $a2, 0x12($sp)` against a scratch at `sp+0x10`) - `rot[1]`.
pub const SCRATCH_YAW_OFFSET: usize = 2;
/// Full turn of the trig LUTs the burst indexes (`andi 0xfff` + halfword
/// entries). The child's heading is this modulus.
pub const LUT_TURN: u32 = 0x1000;

/// Which parameter set the second argument selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstMode {
    /// `mode == 0`.
    Wide,
    /// `mode == 1`.
    Narrow,
}

impl BurstMode {
    /// Resolve the second argument. Retail falls straight to the epilogue for
    /// anything else, so the burst is a no-op.
    pub const fn from_arg(mode: u32) -> Option<Self> {
        match mode {
            0 => Some(BurstMode::Wide),
            1 => Some(BurstMode::Narrow),
            _ => None,
        }
    }

    /// The stager-record address this arm hands the spawner (`lui 0x801f` +
    /// `addiu`, three times per loop body). The record's bytes are disc data;
    /// [`BurstRecord::parse`] reads them, this only carries the address.
    pub const fn record_addr(self) -> u32 {
        match self {
            BurstMode::Wide => 0x801F_5DA4,
            BurstMode::Narrow => 0x801F_5D0C,
        }
    }

    /// The address of the 18-byte move program whose only substantive opcode is
    /// the `0x17` that fires *this* arm.
    ///
    /// Each arm's stager record is preceded in `0898`'s tail by a nine-word
    /// trigger of the shape `WAIT_SET 0 / 0x17 <mode> / WAIT_SET 0 / HALT`, and
    /// the trigger's operand matches the arm whose record follows it. That pair
    /// is how the burst is reached: something seats the trigger as an ordinary
    /// move buffer, the VM's case `0x17` calls this entry, and the twelve
    /// children run the record two bytes past the trigger's end.
    ///
    /// These two addresses are cited elsewhere in the repo as "binary animation
    /// tables passed to the particle spawner `FUN_80050ED4`". They are not
    /// tables and they do not reach the spawner directly - they are move
    /// programs, and it is the `0x17` inside them that reaches it.
    pub const fn trigger_addr(self) -> u32 {
        match self {
            BurstMode::Wide => 0x801F_5D90,
            BurstMode::Narrow => 0x801F_5CF8,
        }
    }

    /// The three spawn blocks, in the order the loop runs them.
    pub const fn blocks(self) -> [SpawnBlock; SPAWNS_PER_ITERATION] {
        match self {
            BurstMode::Wide => WIDE_BLOCKS,
            BurstMode::Narrow => NARROW_BLOCKS,
        }
    }
}

/// One spawn block's constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnBlock {
    /// `true` when the block indexes the LUTs on the diagonals
    /// (`(iter * 1024 + 512) & 0xFFF`) rather than the cardinals
    /// (`iter * 1024`, unmasked).
    pub diagonal: bool,
    /// Right shift applied to the sine term as a **signed** divide (the
    /// `bgez` / `addiu (1 << n) - 1` rounding pair precedes it).
    pub sine_shift: u32,
    /// Modulus of the yaw jitter (`rand % this`).
    pub yaw_mod: i32,
    /// Bias added after the yaw jitter.
    pub yaw_bias: i32,
    /// Divisor applied to the cosine term.
    pub cosine_div: i32,
    /// Modulus of the first post-spawn store's jitter.
    pub spread_mod: i32,
    /// Bias added after that jitter. The store lands at `child + 0x3E`.
    pub spread_bias: i32,
    /// Modulus of the second post-spawn store's jitter.
    pub tail_mod: i32,
    /// Bias added after that jitter. The store lands at `child + 0x98`.
    pub tail_bias: i32,
    /// `true` for the two blocks that load the parent's `+0x72` with `lhu` and
    /// pass `>> 1`; `false` for block `2`, which loads it with `lh` and passes
    /// it whole. See [`SpawnBlock::child_scale`].
    pub scale_halved: bool,
}

impl SpawnBlock {
    /// The fourth argument this block hands the spawner, from the parent's raw
    /// `+0x72` halfword.
    ///
    /// The two forms are genuinely different for a parent whose `+0x72` has its
    /// top bit set: `lhu >> 1` is a zero-extended halve, `lh` is a
    /// sign-extension with no halve.
    pub const fn child_scale(self, parent_scale: u16) -> i32 {
        if self.scale_halved {
            (parent_scale >> 1) as i32
        } else {
            parent_scale as i16 as i32
        }
    }
}

/// Arm `0`'s three blocks.
pub const WIDE_BLOCKS: [SpawnBlock; SPAWNS_PER_ITERATION] = [
    SpawnBlock {
        diagonal: false,
        sine_shift: 3,
        yaw_mod: 257,
        yaw_bias: -0x80,
        cosine_div: 48,
        spread_mod: 21,
        spread_bias: -0x0A,
        tail_mod: 33,
        tail_bias: 0x70,
        scale_halved: true,
    },
    SpawnBlock {
        diagonal: true,
        sine_shift: 4,
        yaw_mod: 129,
        yaw_bias: -0x40,
        cosine_div: 72,
        spread_mod: 15,
        spread_bias: -0x07,
        tail_mod: 49,
        tail_bias: 0xA8,
        scale_halved: true,
    },
    SpawnBlock {
        diagonal: true,
        sine_shift: 3,
        yaw_mod: 257,
        yaw_bias: -0x80,
        cosine_div: 96,
        spread_mod: 11,
        spread_bias: -0x05,
        tail_mod: 17,
        tail_bias: 0x38,
        scale_halved: false,
    },
];

/// Arm `1`'s three blocks - identical but for the cosine divisors and the tail
/// biases.
pub const NARROW_BLOCKS: [SpawnBlock; SPAWNS_PER_ITERATION] = [
    SpawnBlock {
        cosine_div: 96,
        tail_bias: 0x30,
        ..WIDE_BLOCKS[0]
    },
    SpawnBlock {
        cosine_div: 144,
        tail_bias: 0x48,
        ..WIDE_BLOCKS[1]
    },
    SpawnBlock {
        cosine_div: 192,
        tail_bias: 0x18,
        ..WIDE_BLOCKS[2]
    },
];

/// The trig-LUT element index a block uses on a given iteration.
///
/// The two arms are not symmetric in retail: the diagonal arm masks to
/// [`LUT_TURN`], the cardinal arm does not. Over `0..`[`ITERATIONS`] the mask is
/// invisible, which is exactly why it is modelled rather than assumed.
pub const fn lut_index(iteration: u32, diagonal: bool) -> u32 {
    if diagonal {
        ((iteration << 10) + 0x200) & (LUT_TURN - 1)
    } else {
        iteration << 10
    }
}

/// Retail's signed divide-by-power-of-two: bias a negative numerator by
/// `(1 << shift) - 1` before the arithmetic shift so it truncates toward zero.
pub const fn signed_shift_div(value: i32, shift: u32) -> i32 {
    let biased = if value < 0 {
        value + ((1 << shift) - 1)
    } else {
        value
    };
    biased >> shift
}

/// The modulo the reciprocal chains compute. Retail forms `x - (x / d) * d`
/// with a magic multiply; every constant is verified against plain division in
/// this module's tests, so the port uses the division directly.
pub const fn reciprocal_mod(draw: i32, divisor: i32) -> i32 {
    if divisor == 0 {
        return 0;
    }
    draw - (draw / divisor) * divisor
}

/// What a block hands the spawner, plus the two values it writes afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    /// The scratch halfword `rot[1]` after the yaw term is folded in. The
    /// seater masks it to [`LUT_TURN`] into the child's `+0x96`.
    pub yaw: i16,
    /// The child's `+0x72` - see [`SpawnBlock::child_scale`].
    pub scale: i32,
    /// Value stored at the child's `+0x3E`.
    pub spread: i16,
    /// Value stored at the child's `+0x98`.
    pub tail: i16,
}

impl SpawnRequest {
    /// The heading the child actually ends up with, i.e. the seater's
    /// `param_2[1] & 0xFFF` write into `+0x96`.
    pub const fn child_heading(self) -> u16 {
        (self.yaw as u16) & (LUT_TURN as u16 - 1)
    }
}

/// Everything the burst reads that is not one of its own constants.
pub trait BurstHost {
    /// `func_0x80056798` - the SCUS RNG.
    fn rand(&mut self) -> i32;
    /// `sin[index]`, via the pointer at `_DAT_8007B81C`.
    fn sin(&self, index: u32) -> i16;
    /// `cos[index]`, via the pointer at `_DAT_8007B7F8`.
    fn cos(&self, index: u32) -> i16;
    /// The scratch halfword the block starts from - `rot[1]`, the second of the
    /// three halfwords copied out of the parent's `+0x24`. Re-read at the top of
    /// every block, so a host that mutates the parent between blocks is
    /// modelling retail.
    fn parent_yaw(&self) -> i16;
    /// The parent's raw `+0x72` halfword, before the per-block `lhu >> 1` /
    /// `lh` fork in [`SpawnBlock::child_scale`].
    fn parent_scale(&self) -> u16;
    /// Seat one child on `record_addr` and return a handle to it, or `None`
    /// when the pool is exhausted (retail's `FUN_80050ED4` returns `0`).
    ///
    /// Retail is `FUN_80050ED4` → `FUN_80021B04`; the port of the latter is
    /// [`crate::move_vm::spawn::spawn_move_actor`]. The `request` carries the
    /// two values the burst writes into the child *after* the call, so a host
    /// can apply them itself if its actor handle is not otherwise reachable.
    fn spawn(&mut self, record_addr: u32, request: SpawnRequest) -> Option<u32>;
}

/// Run one spawn block and return what it produced.
///
/// Draw order is retail's: the yaw draw before the spawn call, then the spread
/// draw, then the tail draw. All three are consumed unconditionally - a pool
/// failure does not skip the two trailing draws, because retail's `mfhi`
/// chains sit between the `jal`s with no branch on the returned pointer.
pub fn run_block(
    host: &mut impl BurstHost,
    block: SpawnBlock,
    iteration: u32,
    record_addr: u32,
) -> SpawnRequest {
    let idx = lut_index(iteration, block.diagonal);

    let draw = host.rand();
    let sine = signed_shift_div(host.sin(idx) as i32, block.sine_shift);
    let yaw = (host.parent_yaw() as i32)
        .wrapping_add(sine)
        .wrapping_add(reciprocal_mod(draw, block.yaw_mod))
        .wrapping_add(block.yaw_bias) as i16;

    let scale = block.child_scale(host.parent_scale());

    let draw = host.rand();
    let spread = ((host.cos(idx) as i32) / block.cosine_div)
        .wrapping_add(reciprocal_mod(draw, block.spread_mod))
        .wrapping_add(block.spread_bias) as i16;

    let draw = host.rand();
    let tail = reciprocal_mod(draw, block.tail_mod).wrapping_add(block.tail_bias) as i16;

    let req = SpawnRequest {
        yaw,
        scale,
        spread,
        tail,
    };
    host.spawn(record_addr, req);
    req
}

/// The whole burst (`FUN_801F30C4`).
///
/// Returns the requests in the order they were issued, or an empty vector when
/// `mode` selects neither arm.
pub fn run_burst(host: &mut impl BurstHost, mode: u32) -> Vec<SpawnRequest> {
    let Some(mode) = BurstMode::from_arg(mode) else {
        return Vec::new();
    };
    let blocks = mode.blocks();
    let record_addr = mode.record_addr();
    let mut out = Vec::with_capacity(ITERATIONS as usize * SPAWNS_PER_ITERATION);
    for iteration in 0..ITERATIONS {
        for block in blocks {
            out.push(run_block(host, block, iteration, record_addr));
        }
    }
    out
}

/// The two exact relations between the arms, as a runtime check rather than a
/// claim in prose: every narrow cosine divisor is twice the wide one, and every
/// narrow tail offset is exactly three sevenths of the wide one.
pub fn arm_invariants_hold() -> bool {
    WIDE_BLOCKS.iter().zip(NARROW_BLOCKS.iter()).all(|(w, n)| {
        n.cosine_div == w.cosine_div * 2
            && w.tail_bias * 3 % 7 == 0
            && n.tail_bias == w.tail_bias * 3 / 7
    })
}

// ---------------------------------------------------------------------------
// The stager records
// ---------------------------------------------------------------------------

/// One of the two move-VM stager records the burst seats its children on.
///
/// Layout is the shared move-buffer record format - `[i16 model_sel][u16 flags]
/// [move-VM bytecode]`, terminated by op `0x08` HALT - documented under
/// [`docs/subsystems/move-vm.md`](../../../docs/subsystems/move-vm.md), the same
/// shape `legaia_asset::summon_overlay` and `legaia_asset::scene_event_scripts`
/// parse for the summon and per-scene stager tables.
///
/// The bytes come from a supplied `0898` image and are never committed: this
/// struct owns a decoded copy at runtime, and every assertion about the records
/// in this module's tests is a *structural* one (extent, terminator, which
/// opcodes appear, which single word differs between the arms).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BurstRecord {
    /// `+0x00` - the seater's four-way selector. Both arms are the
    /// transform-node value.
    pub model_sel: i16,
    /// `+0x02`.
    pub flags: u16,
    /// `+0x04..` - the move-VM program in u16 units, including its terminating
    /// HALT word. This is what a host binds as the child's move buffer.
    pub program: Vec<u16>,
}

/// The two header words that precede a record's bytecode.
pub const RECORD_HEADER_WORDS: usize = 2;

/// Ceiling on the words a record walk will consume before giving up. Retail has
/// no such bound; the walk needs one because a mis-based image decodes to
/// arbitrary opcodes.
const RECORD_WALK_LIMIT: usize = 4096;

/// A [`MoveHost`] that answers nothing, for walking a program's extent.
///
/// Every `MoveHost` method has a default, so the empty impl is the whole
/// no-op host. Extent is a property of the opcode sizes alone - no handler in
/// the size path consults the host - so walking with this is exact.
struct NullMoveHost;
impl MoveHost for NullMoveHost {}

impl BurstRecord {
    /// Slice an arm's stager record out of a mapped overlay image.
    ///
    /// `image` is the raw `0898` bytes and `base_va` the address they are mapped
    /// at (`0x801CE818` for the verified static extraction). Returns `None` when
    /// the record's address is outside the image, when the header does not fit,
    /// or when the program does not terminate inside
    /// [`RECORD_WALK_LIMIT`] words - all three of which a wrong base produces.
    pub fn parse(image: &[u8], base_va: u32, mode: BurstMode) -> Option<Self> {
        Self::parse_at(image, base_va, mode.record_addr())
    }

    /// [`BurstRecord::parse`] against an arbitrary address - the trigger records
    /// at [`BurstMode::trigger_addr`] are the same format.
    pub fn parse_at(image: &[u8], base_va: u32, record_va: u32) -> Option<Self> {
        let off = record_va.checked_sub(base_va)? as usize;
        let head = image.get(off..)?;
        if head.len() < RECORD_HEADER_WORDS * 2 {
            return None;
        }
        let word = |i: usize| -> Option<u16> {
            let b = head.get(i * 2..i * 2 + 2)?;
            Some(u16::from_le_bytes([b[0], b[1]]))
        };
        let model_sel = word(0)? as i16;
        let flags = word(1)?;

        // Walk the bytecode with the ported dispatcher so the opcode sizes are
        // not restated here. `step` advances `state.pc` itself.
        let avail = (head.len() / 2).saturating_sub(RECORD_HEADER_WORDS);
        let mut words = Vec::with_capacity(avail.min(RECORD_WALK_LIMIT));
        for i in 0..avail.min(RECORD_WALK_LIMIT) {
            words.push(word(RECORD_HEADER_WORDS + i)?);
        }
        let mut state = ActorState::default();
        let mut host = NullMoveHost;
        for _ in 0..RECORD_WALK_LIMIT {
            match step(&mut host, &mut state, &words) {
                StepResult::Advance | StepResult::Wait => {}
                StepResult::Halt => {
                    // HALT has size 0, so the PC still points at it.
                    let end = state.pc as usize + 1;
                    words.truncate(end);
                    return Some(Self {
                        model_sel,
                        flags,
                        program: words,
                    });
                }
                StepResult::EndOfBuffer { .. } | StepResult::Pending { .. } => return None,
            }
        }
        None
    }

    /// Total size of the record on disc, header included, in bytes.
    pub fn byte_len(&self) -> usize {
        (RECORD_HEADER_WORDS + self.program.len()) * 2
    }

    /// The opcode at each instruction boundary, in program order. Structural -
    /// it names which handlers the record uses without reproducing operands.
    pub fn opcode_sequence(&self) -> Vec<u16> {
        let mut state = ActorState::default();
        let mut host = NullMoveHost;
        let mut ops = Vec::new();
        for _ in 0..RECORD_WALK_LIMIT {
            let pc = state.pc as usize;
            let Some(op) = self.program.get(pc).copied() else {
                break;
            };
            ops.push(op);
            match step(&mut host, &mut state, &self.program) {
                StepResult::Advance | StepResult::Wait => {}
                _ => break,
            }
        }
        ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Host {
        draws: Vec<i32>,
        cursor: usize,
        yaw: i16,
        scale: u16,
        pool_free: usize,
        spawns: Vec<(u32, SpawnRequest)>,
    }

    impl Host {
        fn new(draws: Vec<i32>) -> Self {
            Self {
                draws,
                cursor: 0,
                yaw: 0,
                scale: 0x1000,
                pool_free: usize::MAX,
                spawns: Vec::new(),
            }
        }
    }

    impl BurstHost for Host {
        fn rand(&mut self) -> i32 {
            let v = self.draws.get(self.cursor).copied().unwrap_or(0);
            self.cursor += 1;
            v
        }
        fn sin(&self, index: u32) -> i16 {
            let rad = (index & 0xFFF) as f64 * std::f64::consts::TAU / 4096.0;
            (rad.sin() * 4096.0).round() as i16
        }
        fn cos(&self, index: u32) -> i16 {
            let rad = (index & 0xFFF) as f64 * std::f64::consts::TAU / 4096.0;
            (rad.cos() * 4096.0).round() as i16
        }
        fn parent_yaw(&self) -> i16 {
            self.yaw
        }
        fn parent_scale(&self) -> u16 {
            self.scale
        }
        fn spawn(&mut self, record_addr: u32, request: SpawnRequest) -> Option<u32> {
            if self.pool_free == 0 {
                return None;
            }
            self.pool_free = self.pool_free.saturating_sub(1);
            self.spawns.push((record_addr, request));
            Some(0x8010_0000)
        }
    }

    // -- the mode fork ------------------------------------------------------

    #[test]
    fn only_zero_and_one_select_an_arm() {
        assert_eq!(BurstMode::from_arg(0), Some(BurstMode::Wide));
        assert_eq!(BurstMode::from_arg(1), Some(BurstMode::Narrow));
        for m in [2u32, 3, 0xFF, 0xFFFF_FFFF] {
            assert_eq!(BurstMode::from_arg(m), None, "mode {m}");
        }
    }

    #[test]
    fn an_unselected_mode_spawns_nothing_and_draws_nothing() {
        let mut h = Host::new(vec![1; 64]);
        assert!(run_burst(&mut h, 7).is_empty());
        assert_eq!(h.cursor, 0, "the fork precedes every draw");
        assert!(h.spawns.is_empty());
    }

    // -- the arm constants --------------------------------------------------

    #[test]
    fn the_two_arms_differ_only_in_the_documented_constants() {
        for (w, n) in WIDE_BLOCKS.iter().zip(NARROW_BLOCKS.iter()) {
            assert_eq!(w.diagonal, n.diagonal);
            assert_eq!(w.sine_shift, n.sine_shift);
            assert_eq!(w.yaw_mod, n.yaw_mod);
            assert_eq!(w.yaw_bias, n.yaw_bias);
            assert_eq!(w.spread_mod, n.spread_mod);
            assert_eq!(w.spread_bias, n.spread_bias);
            assert_eq!(w.tail_mod, n.tail_mod);
            assert_eq!(
                w.scale_halved, n.scale_halved,
                "the lhu>>1 / lh fork is per block, not per arm"
            );
            assert_ne!(w.cosine_div, n.cosine_div);
            assert_ne!(w.tail_bias, n.tail_bias);
        }
    }

    #[test]
    fn narrow_is_wide_at_half_the_cosine_and_three_sevenths_the_offset() {
        assert!(arm_invariants_hold());
        // Spelled out, so a future edit to the tables trips this too.
        assert_eq!(WIDE_BLOCKS.map(|b| b.cosine_div), [48, 72, 96]);
        assert_eq!(NARROW_BLOCKS.map(|b| b.cosine_div), [96, 144, 192]);
        assert_eq!(WIDE_BLOCKS.map(|b| b.tail_bias), [0x70, 0xA8, 0x38]);
        assert_eq!(NARROW_BLOCKS.map(|b| b.tail_bias), [0x30, 0x48, 0x18]);
    }

    #[test]
    fn the_two_arms_use_different_records() {
        assert_ne!(
            BurstMode::Wide.record_addr(),
            BurstMode::Narrow.record_addr()
        );
        assert_eq!(BurstMode::Wide.record_addr(), 0x801F_5DA4);
        assert_eq!(BurstMode::Narrow.record_addr(), 0x801F_5D0C);
    }

    #[test]
    fn each_trigger_sits_immediately_before_the_record_it_fires() {
        // The trigger is nine words (two header + WAIT/EXT/WAIT/HALT), and the
        // record starts one word past its end - the tail is packed with a
        // single alignment word between records.
        for mode in [BurstMode::Wide, BurstMode::Narrow] {
            let gap = mode.record_addr() - mode.trigger_addr();
            assert_eq!(gap, 0x14, "{mode:?}: trigger is 18 bytes + 2 of padding");
        }
    }

    // -- angles -------------------------------------------------------------

    #[test]
    fn block_zero_walks_the_cardinals_and_the_rest_the_diagonals() {
        assert_eq!(
            (0..ITERATIONS)
                .map(|i| lut_index(i, false))
                .collect::<Vec<_>>(),
            vec![0, 1024, 2048, 3072]
        );
        assert_eq!(
            (0..ITERATIONS)
                .map(|i| lut_index(i, true))
                .collect::<Vec<_>>(),
            vec![512, 1536, 2560, 3584]
        );
        assert!(!WIDE_BLOCKS[0].diagonal);
        assert!(WIDE_BLOCKS[1].diagonal && WIDE_BLOCKS[2].diagonal);
    }

    #[test]
    fn the_two_arms_are_a_quarter_turn_apart_and_evenly_spaced() {
        // The property the constants encode: four spawn directions per block,
        // one full turn between them, and the diagonal arm offset by an eighth.
        let card: Vec<u32> = (0..ITERATIONS).map(|i| lut_index(i, false)).collect();
        let diag: Vec<u32> = (0..ITERATIONS).map(|i| lut_index(i, true)).collect();
        for w in card.windows(2) {
            assert_eq!(w[1] - w[0], LUT_TURN / ITERATIONS);
        }
        for w in diag.windows(2) {
            assert_eq!(w[1] - w[0], LUT_TURN / ITERATIONS);
        }
        for (c, d) in card.iter().zip(diag.iter()) {
            assert_eq!(d - c, LUT_TURN / 8);
        }
    }

    #[test]
    fn only_the_diagonal_arm_masks() {
        // Retail's cardinal arm is a bare `sll $s1, $s2, 0xb`. Inside the loop
        // bound the two agree; outside it they must not, or the mask has been
        // added to an arm that has none.
        for i in 0..ITERATIONS {
            assert!(lut_index(i, false) < LUT_TURN);
        }
        assert_eq!(lut_index(4, false), 4096, "cardinal arm does not wrap");
        assert_eq!(lut_index(4, true), 512, "diagonal arm does");
    }

    // -- arithmetic kernels -------------------------------------------------

    #[test]
    fn signed_shift_div_truncates_toward_zero() {
        assert_eq!(signed_shift_div(16, 3), 2);
        assert_eq!(signed_shift_div(15, 3), 1);
        assert_eq!(signed_shift_div(-16, 3), -2);
        // The bias is what makes this differ from a bare `sra`.
        assert_eq!(signed_shift_div(-15, 3), -1);
        assert_eq!(-15i32 >> 3, -2, "a bare sra would round away from zero");
        for v in -5000i32..5000 {
            for sh in [3u32, 4] {
                assert_eq!(signed_shift_div(v, sh), v / (1 << sh), "v={v} sh={sh}");
            }
        }
    }

    /// The magic-multiply reciprocals, reproduced exactly as retail computes
    /// them and checked against plain truncating division.
    ///
    /// `q = (hi(x * magic) >> shift) - (x >> 31)`, or with the operand added
    /// back before the shift for the `0x88888889` magic-with-add form. Every
    /// (magic, shift, divisor) triple in the two arms is here; the shift is the
    /// part that has been misread before, so the table carries it explicitly.
    #[test]
    fn every_magic_multiply_is_the_divide_it_is_taken_for() {
        fn hi(a: i32, b: i32) -> i32 {
            (((a as i64) * (b as i64)) >> 32) as i32
        }
        fn q_plain(x: i32, magic: i32, shift: u32) -> i32 {
            (hi(x, magic) >> shift) - (x >> 31)
        }
        fn q_add(x: i32, magic: i32, shift: u32) -> i32 {
            (hi(x, magic).wrapping_add(x) >> shift) - (x >> 31)
        }

        #[allow(clippy::type_complexity)]
        let cases: &[(&str, i32, u32, i32, fn(i32, i32, u32) -> i32)] = &[
            ("blk0 yaw", 0x7F80_7F81u32 as i32, 7, 257, q_plain),
            ("blk0 cos wide", 0x2AAA_AAAB, 3, 48, q_plain),
            ("blk0 cos narrow", 0x2AAA_AAAB, 4, 96, q_plain),
            ("blk0 spread", 0x30C3_0C31, 2, 21, q_plain),
            ("blk0 tail", 0x3E0F_83E1, 3, 33, q_plain),
            ("blk1 yaw", 0x0FE0_3F81, 3, 129, q_plain),
            ("blk1 cos wide", 0x38E3_8E39, 4, 72, q_plain),
            ("blk1 cos narrow", 0x38E3_8E39, 5, 144, q_plain),
            ("blk1 spread", 0x8888_8889u32 as i32, 3, 15, q_add),
            ("blk1 tail", 0x5397_829D, 4, 49, q_plain),
            ("blk2 cos wide", 0x2AAA_AAAB, 4, 96, q_plain),
            ("blk2 cos narrow", 0x2AAA_AAAB, 5, 192, q_plain),
            ("blk2 spread", 0x2E8B_A2E9, 1, 11, q_plain),
            ("blk2 tail", 0x7878_7879, 3, 17, q_plain),
        ];

        let boundaries = [
            i32::MIN,
            i32::MIN + 1,
            -1_073_741_824,
            -65_537,
            -65_536,
            -32_769,
            -32_768,
            32_767,
            32_768,
            65_535,
            65_536,
            1_073_741_823,
            i32::MAX - 1,
            i32::MAX,
        ];
        for &(label, magic, shift, d, form) in cases {
            for x in (-40_000i32..40_000).chain(boundaries) {
                assert_eq!(form(x, magic, shift), x / d, "{label}: /{d} at x={x}");
            }
            // And the modulo the port actually uses agrees with retail's
            // `x - q*d` over the same band.
            for x in -40_000i32..40_000 {
                assert_eq!(
                    reciprocal_mod(x, d),
                    x - form(x, magic, shift) * d,
                    "{label}: modulo at x={x}"
                );
            }
        }
    }

    #[test]
    fn reciprocal_mod_matches_plain_modulo() {
        for d in [257i32, 129, 21, 15, 11, 33, 49, 17] {
            for v in 0i32..2000 {
                assert_eq!(reciprocal_mod(v, d), v % d, "v={v} d={d}");
            }
        }
        assert_eq!(reciprocal_mod(5, 0), 0, "no divide-by-zero trap");
    }

    // -- the loop -----------------------------------------------------------

    #[test]
    fn a_burst_issues_twelve_spawns_and_thirty_six_draws() {
        let mut h = Host::new(vec![0; 128]);
        let out = run_burst(&mut h, 0);
        assert_eq!(out.len(), (ITERATIONS as usize) * SPAWNS_PER_ITERATION);
        assert_eq!(out.len(), 12);
        assert_eq!(h.cursor, 12 * 3);
        assert_eq!(h.cursor, ITERATIONS as usize * DRAWS_PER_ITERATION);
        assert_eq!(h.spawns.len(), 12);
    }

    #[test]
    fn an_exhausted_pool_still_consumes_the_whole_rng_stream() {
        // Retail's two post-spawn `mfhi` chains sit between the `jal`s with no
        // branch on the returned pointer, so a null return does not shorten the
        // draw stream. A host that mirrors the RNG must see all 36.
        let mut h = Host::new(vec![3; 128]);
        h.pool_free = 5;
        let out = run_burst(&mut h, 0);
        assert_eq!(out.len(), 12, "every block still produces a request");
        assert_eq!(h.spawns.len(), 5, "only five fit in the pool");
        assert_eq!(h.cursor, ITERATIONS as usize * DRAWS_PER_ITERATION);
    }

    #[test]
    fn every_spawn_carries_the_arms_record() {
        let mut h = Host::new(vec![0; 128]);
        run_burst(&mut h, 1);
        assert!(
            h.spawns
                .iter()
                .all(|(t, _)| *t == BurstMode::Narrow.record_addr())
        );
    }

    // -- per-block values ---------------------------------------------------

    #[test]
    fn the_tail_value_is_bounded_by_its_modulus_and_bias() {
        for mode in [BurstMode::Wide, BurstMode::Narrow] {
            for b in mode.blocks() {
                for draw in 0i32..200 {
                    let t = reciprocal_mod(draw, b.tail_mod) + b.tail_bias;
                    assert!(t >= b.tail_bias);
                    assert!(t < b.tail_bias + b.tail_mod);
                }
            }
        }
    }

    #[test]
    fn narrow_tails_land_strictly_below_wide_tails() {
        // The 3/7 scaling means every narrow band sits under its wide twin.
        for (w, n) in WIDE_BLOCKS.iter().zip(NARROW_BLOCKS.iter()) {
            assert!(n.tail_bias + n.tail_mod <= w.tail_bias + w.tail_mod);
            assert!(n.tail_bias < w.tail_bias);
        }
    }

    #[test]
    fn the_yaw_folds_the_sine_the_jitter_and_the_bias() {
        let mut h = Host::new(vec![0; 8]);
        h.yaw = 1000;
        let b = WIDE_BLOCKS[0];
        // Iteration 0 -> LUT index 0 -> sin 0, cos 4096.
        let req = run_block(&mut h, b, 0, 0);
        // yaw = 1000 + 0 + (0 % 257) + (-0x80)
        assert_eq!(req.yaw, 1000 - 0x80);
        // spread = 4096/48 + (0 % 21) - 0x0A
        assert_eq!(req.spread, (4096 / 48) - 0x0A);
        // tail = (0 % 33) + 0x70
        assert_eq!(req.tail, 0x70);
    }

    #[test]
    fn the_child_heading_is_the_yaw_modulo_one_turn() {
        // The seater masks `rot[1]` to 12 bits, so a yaw that overflows the
        // circle still lands inside it - including from the negative side,
        // which a signed remainder would get wrong.
        let mut h = Host::new(vec![0; 8]);
        for start in [-4000i16, -1, 0, 1, 4095, 4096, 20000] {
            h.yaw = start;
            h.cursor = 0;
            let req = run_block(&mut h, WIDE_BLOCKS[0], 0, 0);
            assert!(
                req.child_heading() < LUT_TURN as u16,
                "start={start} heading={}",
                req.child_heading()
            );
        }
    }

    #[test]
    fn blocks_zero_and_one_halve_the_parent_scale_and_block_two_does_not() {
        // Retail's fork is `lhu >> 1` vs `lh`, which only diverge for a parent
        // whose +0x72 has the top bit set - so probe there, not just at 0x1000.
        for parent in [0u16, 1, 0x1000, 0x7FFF, 0x8000, 0xFFFF] {
            assert_eq!(
                WIDE_BLOCKS[0].child_scale(parent),
                (parent >> 1) as i32,
                "parent={parent:#06X}"
            );
            assert_eq!(WIDE_BLOCKS[1].child_scale(parent), (parent >> 1) as i32);
            assert_eq!(
                WIDE_BLOCKS[2].child_scale(parent),
                parent as i16 as i32,
                "block 2 sign-extends and does not halve"
            );
        }
        // The two forms really are distinguishable, or the test is vacuous.
        assert_ne!(
            WIDE_BLOCKS[0].child_scale(0x8000),
            WIDE_BLOCKS[2].child_scale(0x8000)
        );
    }

    #[test]
    fn a_burst_carries_two_distinct_child_scales() {
        let mut h = Host::new(vec![0; 128]);
        h.scale = 0x1000;
        let out = run_burst(&mut h, 0);
        for (i, req) in out.iter().enumerate() {
            let want = if i % SPAWNS_PER_ITERATION == 2 {
                0x1000
            } else {
                0x0800
            };
            assert_eq!(req.scale, want, "block {} of iteration {}", i % 3, i / 3);
        }
    }

    #[test]
    fn the_same_draws_place_narrow_spawns_tighter_than_wide() {
        let draws = vec![7i32; 128];
        let mut hw = Host::new(draws.clone());
        let mut hn = Host::new(draws);
        let wide = run_burst(&mut hw, 0);
        let narrow = run_burst(&mut hn, 1);
        assert_eq!(wide.len(), narrow.len());
        // Yaw and scale are arm-independent; the spread and tail are not.
        for (w, n) in wide.iter().zip(narrow.iter()) {
            assert_eq!(w.yaw, n.yaw);
            assert_eq!(w.scale, n.scale);
            assert!(n.tail < w.tail);
        }
    }

    #[test]
    fn draw_order_is_yaw_then_spread_then_tail() {
        // Distinct draws make the ordering observable.
        let mut h = Host::new(vec![100, 200, 300]);
        h.yaw = 0;
        let b = WIDE_BLOCKS[0];
        let req = run_block(&mut h, b, 0, 0);
        assert_eq!(
            req.yaw,
            (reciprocal_mod(100, b.yaw_mod) + b.yaw_bias) as i16
        );
        assert_eq!(
            req.spread,
            (4096 / b.cosine_div + reciprocal_mod(200, b.spread_mod) + b.spread_bias) as i16
        );
        assert_eq!(
            req.tail,
            (reciprocal_mod(300, b.tail_mod) + b.tail_bias) as i16
        );
        // And the three draws really were distinct, so the order is observable.
        assert_ne!(req.yaw, req.spread);
        assert_ne!(req.spread, req.tail);
    }

    // -- the record parser --------------------------------------------------

    /// Build a synthetic image around a hand-assembled record so the parser is
    /// exercised without disc data. Real-image coverage is the disc-gated
    /// `battle_burst_records` test.
    fn image_with(base_va: u32, record_va: u32, words: &[u16]) -> Vec<u8> {
        let off = (record_va - base_va) as usize;
        let mut img = vec![0xAAu8; off + words.len() * 2 + 64];
        for (i, w) in words.iter().enumerate() {
            img[off + i * 2..off + i * 2 + 2].copy_from_slice(&w.to_le_bytes());
        }
        img
    }

    #[test]
    fn a_record_walk_stops_at_halt_and_keeps_the_halt_word() {
        let base = 0x801C_E818;
        let va = BurstMode::Wide.record_addr();
        // [model_sel=-1][flags=0] 0x39 a b c | 0x06 v | 0x08
        let words = [0xFFFF, 0x0000, 0x39, 1, 2, 3, 0x06, 9, 0x08, 0x1234, 0x5678];
        let img = image_with(base, va, &words);
        let rec = BurstRecord::parse(&img, base, BurstMode::Wide).expect("parses");
        assert_eq!(rec.model_sel, -1);
        assert_eq!(rec.flags, 0);
        assert_eq!(rec.opcode_sequence(), vec![0x39, 0x06, 0x08]);
        assert_eq!(
            rec.program.last().copied(),
            Some(0x08),
            "the terminator is part of the program"
        );
        assert_eq!(rec.byte_len(), (2 + 7) * 2);
        assert!(
            !rec.program.contains(&0x1234),
            "nothing past HALT is captured"
        );
    }

    #[test]
    fn a_record_that_never_halts_is_rejected() {
        let base = 0x801C_E818;
        let va = BurstMode::Wide.record_addr();
        // 0x47 is past the dispatcher's bound check - retail ends the loop
        // there, but it is not a terminator, so the walk must refuse it.
        let words = [0xFFFF, 0x0000, 0x47, 0x47, 0x47];
        let img = image_with(base, va, &words);
        assert!(BurstRecord::parse(&img, base, BurstMode::Wide).is_none());
    }

    #[test]
    fn an_address_outside_the_image_is_rejected() {
        let base = 0x801C_E818;
        let img = vec![0u8; 16];
        assert!(BurstRecord::parse(&img, base, BurstMode::Wide).is_none());
        // A record address below the base underflows rather than wrapping.
        assert!(BurstRecord::parse_at(&img, base, base - 4).is_none());
    }

    #[test]
    fn a_trigger_record_is_the_same_format_as_a_stager_record() {
        // Structural: the trigger shape the two arms share is
        // [model_sel][flags] WAIT_SET 0 / 0x17 mode / WAIT_SET 0 / HALT.
        let base = 0x801C_E818;
        let va = BurstMode::Wide.trigger_addr();
        let words = [0xFFFF, 0x0000, 0x09, 0, 0x17, 0, 0x09, 0, 0x08];
        let img = image_with(base, va, &words);
        let rec = BurstRecord::parse_at(&img, base, va).expect("parses");
        assert_eq!(rec.opcode_sequence(), vec![0x09, 0x17, 0x09, 0x08]);
        assert_eq!(rec.byte_len(), 18, "nine words including the header");
        assert_eq!(
            rec.byte_len() + 2,
            (BurstMode::Wide.record_addr() - va) as usize,
            "the stager record starts one word past the trigger's end"
        );
    }
}
