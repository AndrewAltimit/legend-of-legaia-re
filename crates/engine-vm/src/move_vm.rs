//! Move-table opcode VM, ported clean-room from `FUN_80023070` (main VM in
//! `SCUS_942.54`) and `FUN_801D362C` (extension VM in the town overlay).
//!
//! See `docs/subsystems/move-vm.md` for the byte-level reference. The VM drives
//! per-actor animation, motion, and combat moves (Tactical Arts) — distinct
//! from the actor / sprite VM in [`super`] and the field / event VM in
//! [`super::field`]. It is invoked every frame from the actor tick
//! (`FUN_80021DF4`) on a per-actor "move buffer" that the field VM's `EXEC_MOVE`
//! opcode (`0x22`) staged via `FUN_800204F8`.
//!
//! ## Bytecode layout
//!
//! Operand stream is **u16-aligned**. PC is also tracked in u16 units (matching
//! how the original stores it as a signed 16-bit value in `actor[+0x70]`):
//!
//! ```text
//!   *(actor + 0x48 + actor[+0x70] * 2) = u16 opcode
//!   *(actor + 0x48 + (actor[+0x70] + 1) * 2) = u16 operand_0
//!   *(actor + 0x48 + (actor[+0x70] + 2) * 2) = u16 operand_1
//!   ...
//! ```
//!
//! Each handler advances PC by an opcode-specific number of u16 words.
//! Out-of-range opcodes (`>= 0x47`) silently terminate the loop, matching the
//! `sltiu v0, v1, 0x47` bound check in the original dispatcher.
//!
//! ## Two-layer dispatch
//!
//! - The main VM has 71 opcodes (`0x00..=0x46`).
//! - Opcode `0x2F` escapes to a per-overlay extension dispatcher
//!   (`FUN_801D362C` in the town overlay), with 61 sub-opcodes
//!   (`0x00..=0x3C`). The sub-opcode is the u16 at `op[1]`.
//!
//! Both are wired through the [`MoveHost`] trait — extension sub-handlers that
//! don't fit a clean Rust idiom are hooked through `host.ext_*` callbacks.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live in this crate. The Ghidra
//! decompilation at `ghidra/scripts/funcs/80023070.txt` and
//! `ghidra/scripts/funcs/overlay_0897_801d362c.txt` are the *spec*, not source.
//! The [`MoveHost`] trait abstracts every call the original made into the
//! engine layer — implementations live in `crates/engine-core` (or wherever
//! the actor pool is modeled).
//!
//! Tests use hand-authored synthetic bytecode (no Sony bytes).

#![allow(clippy::too_many_arguments)]

/// Per-actor move-VM state. One instance per actor that's running a move.
///
/// Field naming uses the byte-offset convention from `docs/subsystems/move-vm.md`
/// to keep the link to the decompilation explicit. Engines free to back this
/// with whatever data structure makes sense — the VM mutates this struct
/// directly and dispatches side effects through [`MoveHost`].
#[derive(Debug, Clone, Default)]
pub struct ActorState {
    /// `+0x10` — actor flag word. Bit `0x8` set by op 0x08 HALT;
    /// bits `0x2` (op 0x3A/0x3B), `0x1000` (op 0x0A KEYFRAME_LOAD),
    /// `0x10000` and `0x40000000` (composite control word) toggled by various.
    pub flags: u32,
    /// `+0x14` — world X.
    pub world_x: i16,
    /// `+0x16` — world Y.
    pub world_y: i16,
    /// `+0x18` — world Z.
    pub world_z: i16,
    /// `+0x22` — Y-rotation accumulator (cleared by op 0x3D).
    pub y_rot: i16,
    /// `+0x24` — render bank 0.
    pub render_24: i16,
    /// `+0x26` — render bank 1 (op 0x06 / 0x39 / 0x05).
    pub render_26: i16,
    /// `+0x28` — render bank 2.
    pub render_28: i16,
    /// `+0x2A` — Y mirror (kept in sync with [`world_y`] for collision).
    pub world_y_mirror: i16,
    /// `+0x3C` — animation bank slot 0 (op 0x00, written `v << 3`).
    pub anim_3c: i16,
    /// `+0x3E` — animation bank slot 1.
    pub anim_3e: i16,
    /// `+0x40` — animation bank slot 2.
    pub anim_40: i16,
    /// `+0x42` — generic per-actor scalar (op 0x10).
    pub field_42: u16,
    /// `+0x50` — midpoint blend / sub-state byte. Set by ext op 0x0C and
    /// incremented by ext op 0x0D; consumed as the 4th argument to the
    /// `FUN_801E45BC` midpoint helper from ext ops 0x0E / 0x12.
    pub field_50: u16,
    /// `+0x52` — control word written by op 0x15 (the `0x400` bit additionally
    /// clears `flags & 0x80`).
    pub field_52: u16,
    /// `+0x54` — wait/timer accumulator (op 0x09 sets `v << 3`; ticked down
    /// elsewhere). Setting non-zero ends the per-frame interpreter loop.
    pub wait_timer: i16,
    /// `+0x56` — move-table sub-state (cleared by ops 0x13 / 0x42).
    pub move_substate: i16,
    /// `+0x5A` — move sub-mode marker (set to 2/4/6/7 by various ops).
    pub move_submode: i16,
    /// `+0x5C` — anim-loop counter (cleared by op 0x3C SCRATCH_WRITE).
    pub field_5c: i16,
    /// `+0x62` — local flag bank (16 bits). AND/OR by 0x31/0x32.
    pub local_flags: u16,
    /// `+0x68` — generic slot.
    pub field_68: i16,
    /// `+0x6A` — generic slot (op 0x44, written `v << 3`).
    pub field_6a: i16,
    /// `+0x6C` — keyframe count (op 0x0A, byte slot).
    pub keyframe_count: u8,
    /// `+0x6D` — face / body rotation index (cleared by ext op 0x02; written
    /// by op 0x21).
    pub face_rotation: u8,
    /// `+0x70` — **the move-VM PC, in u16 units**.
    pub pc: i16,
    /// `+0x72` — generic slot (op 0x0E).
    pub field_72: u16,
    /// `+0x74` — composite control word (op 0x0C builds it; op 0x33 clears
    /// bit `0x40000000`).
    pub field_74: u32,
    /// `+0x78` — generic slot (op 0x0C).
    pub field_78: u16,
    /// `+0x7A` — generic slot (op 0x12).
    pub field_7a: u16,
    /// `+0x80` — animation bank 2 slot 0 (op 0x04, `v << 3`).
    pub anim_80: i16,
    /// `+0x82` — animation bank 2 slot 1.
    pub anim_82: i16,
    /// `+0x84` — animation bank 2 slot 2.
    pub anim_84: i16,
    /// `+0x86` — composite slot bits (set by ext op 0x10/0x11/0x15/0x16).
    pub field_86: u16,
    /// `+0x88` — saved PC for op 0x18/0x19 jump-back loop.
    pub field_88: u16,
    /// `+0x8A` — saved PC for op 0x1A/0x1B jump-back loop.
    pub field_8a: u16,
    /// `+0x8C` — counter for op 0x18/0x19.
    pub field_8c: u16,
    /// `+0x8E` — counter for op 0x1A/0x1B.
    pub field_8e: u16,
    /// `+0x90` — tween source 0 (op 0x37 absolute, 0x35/0x2D add).
    pub tween_src_x: i16,
    /// `+0x92` — tween source 1.
    pub tween_src_y: i16,
    /// `+0x94` — tween source 2.
    pub tween_src_z: i16,
    /// `+0x96` — tween scale 0 (op 0x2E `v << 3`, 0x29 absolute).
    pub tween_scale_x: i16,
    /// `+0x98` — tween scale 1.
    pub tween_scale_y: i16,
    /// `+0x9A` — tween scale 2.
    pub tween_scale_z: i16,
    /// `+0x9C` — keyframe gate / index (op 0x2C sets to 1; op 0x30 clears).
    pub field_9c: i32,
    /// `+0x9E` — keyframe descriptor word.
    pub field_9e: u16,
    /// `+0xA0..0xA6` — keyframe buffer descriptor (op 0x2C).
    pub keyframe_desc: [u16; 4],
    /// `+0xA8` — heap or inline keyframe pointer / value (op 0x2C / 0x26).
    pub field_a8: i32,
    /// `+0xAC..0xCA` — per-frame anim slot block. Modeled as a flat array
    /// addressable by byte-offset for opcodes that touch deep into it.
    /// Indices are in u16 units relative to `+0xAC`.
    pub anim_block: [u16; 16],
    /// `+0xCA` — duration slot (op 0x1C, `v << 3`).
    pub field_ca: u16,
}

impl ActorState {
    /// Build a fresh actor with PC at zero. Equivalent to a freshly-staged
    /// move buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read a u16 slot in `anim_block` by byte-offset relative to `+0xAC`.
    pub fn anim_block_u16(&self, byte_off: usize) -> u16 {
        self.anim_block.get(byte_off / 2).copied().unwrap_or(0)
    }

    /// Write a u16 slot in `anim_block` by byte-offset relative to `+0xAC`.
    pub fn anim_block_u16_set(&mut self, byte_off: usize, value: u16) {
        if let Some(slot) = self.anim_block.get_mut(byte_off / 2) {
            *slot = value;
        }
    }
}

/// Outcome of a single VM step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult {
    /// The handler ran. PC has been advanced. Caller can step again on the
    /// same frame budget.
    Advance,
    /// Loop break: `0x08` HALT cleared the loop flag, and the dispatcher set
    /// `actor.flags |= 0x8`.
    Halt,
    /// Loop break: `0x09` WAIT_SET seeded the wait timer; further opcodes are
    /// deferred to the next frame.
    Wait,
    /// Out-of-range opcode (`>= 0x47`) — the original dispatcher's bound check
    /// fails and the loop epilogue exits.
    EndOfBuffer { opcode: u16 },
    /// The handler is recognized but not yet implemented in this port.
    Pending { opcode: u16 },
}

/// Symbolic names for the main VM opcodes, mirroring `docs/subsystems/move-vm.md`.
///
/// Reserved or unknown opcodes return `None` from [`MoveOpcode::from_u16`];
/// the runtime treats them as fall-through (loop break) via the `>= 0x47`
/// bound check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MoveOpcode {
    AnimBankSet = 0x00,
    WorldAdd = 0x01,
    BankSet98 = 0x02,
    WorldRotateAdd = 0x03,
    AnimBank2 = 0x04,
    RenderBankAdd = 0x05,
    Write26 = 0x06,
    WorldSet = 0x07,
    Halt = 0x08,
    WaitSet = 0x09,
    KeyframeLoad = 0x0A,
    DefaultBreak = 0x0B,
    Composite0c = 0x0C,
    Write90 = 0x0D,
    Write72 = 0x0E,
    Write92 = 0x0F,
    Write42 = 0x10,
    Write94 = 0x11,
    Write7a = 0x12,
    SubMode13 = 0x13,
    AnimC0 = 0x14,
    Ctrl52 = 0x15,
    Stub16 = 0x16,
    Ext17 = 0x17,
    SaveLoop18 = 0x18,
    LoopJump19 = 0x19,
    SaveLoop1a = 0x1A,
    LoopJump1b = 0x1B,
    WriteCa = 0x1C,
    GlobalWrite1d = 0x1D,
    AnimBlock1e = 0x1E,
    AnimBlock1f = 0x1F,
    Ext20 = 0x20,
    FaceRot21 = 0x21,
    EpilogueShortcut = 0x22,
    AnimBlock23 = 0x23,
    AnimBlockAdd24 = 0x24,
    SpawnSub25 = 0x25,
    AnimBlock26 = 0x26,
    AnimBlock27 = 0x27,
    Add9a = 0x28,
    Write96 = 0x29,
    Write9aShifted = 0x2A,
    TweenAbsTriple = 0x2B,
    KeyBufferAlloc = 0x2C,
    WorldIncVariant = 0x2D,
    TweenScaleSet = 0x2E,
    OverlayExt = 0x2F,
    KeyBufferFree = 0x30,
    LFlagAnd = 0x31,
    LFlagOr = 0x32,
    ClearBit40000000 = 0x33,
    TweenSetup = 0x34,
    WorldIncVariant2 = 0x35,
    TweenDurationSet = 0x36,
    WorldSetVariant2 = 0x37,
    B2Add = 0x38,
    RenderBankSet = 0x39,
    Flag2Set = 0x3A,
    Flag2Clear = 0x3B,
    ScratchWrite = 0x3C,
    AnimInterpolate = 0x3D,
    Write22 = 0x3E,
    WriteD0 = 0x3F,
    Func58490 = 0x40,
    WriteB2 = 0x41,
    AnimBlock42 = 0x42,
    Flag2000 = 0x43,
    Triplet44 = 0x44,
    AnimBlock45 = 0x45,
    TweenInit46 = 0x46,
}

impl MoveOpcode {
    /// Decode the opcode word. Returns `None` for `>= 0x47` (treated as
    /// end-of-buffer by the runtime) or for the small gap below `0x0B` that
    /// is handled implicitly.
    pub fn from_u16(v: u16) -> Option<Self> {
        if v > 0x46 {
            return None;
        }
        // SAFETY: the discriminants above are exactly `0x00..=0x46`, and `v`
        // is bound-checked. transmute is the standard idiom but a match
        // is clearer; we use match.
        Some(match v {
            0x00 => Self::AnimBankSet,
            0x01 => Self::WorldAdd,
            0x02 => Self::BankSet98,
            0x03 => Self::WorldRotateAdd,
            0x04 => Self::AnimBank2,
            0x05 => Self::RenderBankAdd,
            0x06 => Self::Write26,
            0x07 => Self::WorldSet,
            0x08 => Self::Halt,
            0x09 => Self::WaitSet,
            0x0A => Self::KeyframeLoad,
            0x0B => Self::DefaultBreak,
            0x0C => Self::Composite0c,
            0x0D => Self::Write90,
            0x0E => Self::Write72,
            0x0F => Self::Write92,
            0x10 => Self::Write42,
            0x11 => Self::Write94,
            0x12 => Self::Write7a,
            0x13 => Self::SubMode13,
            0x14 => Self::AnimC0,
            0x15 => Self::Ctrl52,
            0x16 => Self::Stub16,
            0x17 => Self::Ext17,
            0x18 => Self::SaveLoop18,
            0x19 => Self::LoopJump19,
            0x1A => Self::SaveLoop1a,
            0x1B => Self::LoopJump1b,
            0x1C => Self::WriteCa,
            0x1D => Self::GlobalWrite1d,
            0x1E => Self::AnimBlock1e,
            0x1F => Self::AnimBlock1f,
            0x20 => Self::Ext20,
            0x21 => Self::FaceRot21,
            0x22 => Self::EpilogueShortcut,
            0x23 => Self::AnimBlock23,
            0x24 => Self::AnimBlockAdd24,
            0x25 => Self::SpawnSub25,
            0x26 => Self::AnimBlock26,
            0x27 => Self::AnimBlock27,
            0x28 => Self::Add9a,
            0x29 => Self::Write96,
            0x2A => Self::Write9aShifted,
            0x2B => Self::TweenAbsTriple,
            0x2C => Self::KeyBufferAlloc,
            0x2D => Self::WorldIncVariant,
            0x2E => Self::TweenScaleSet,
            0x2F => Self::OverlayExt,
            0x30 => Self::KeyBufferFree,
            0x31 => Self::LFlagAnd,
            0x32 => Self::LFlagOr,
            0x33 => Self::ClearBit40000000,
            0x34 => Self::TweenSetup,
            0x35 => Self::WorldIncVariant2,
            0x36 => Self::TweenDurationSet,
            0x37 => Self::WorldSetVariant2,
            0x38 => Self::B2Add,
            0x39 => Self::RenderBankSet,
            0x3A => Self::Flag2Set,
            0x3B => Self::Flag2Clear,
            0x3C => Self::ScratchWrite,
            0x3D => Self::AnimInterpolate,
            0x3E => Self::Write22,
            0x3F => Self::WriteD0,
            0x40 => Self::Func58490,
            0x41 => Self::WriteB2,
            0x42 => Self::AnimBlock42,
            0x43 => Self::Flag2000,
            0x44 => Self::Triplet44,
            0x45 => Self::AnimBlock45,
            0x46 => Self::TweenInit46,
            _ => return None,
        })
    }
}

/// Result of an extension-VM call.
///
/// The original encodes its return as `iVar16 >> 16` in the fall-through path,
/// so the high 16 bits carry a control code (0x2 = world dirty, 0x3 = state-jump,
/// 0x4 = predicate-true skip, 0x5/0x7/0x8/0xB/0xD = various) and the low
/// 16 bits, when present, are the size in u16 units. Engines decide how to
/// interpret the control codes; the VM only needs the size.
///
/// Extension handlers return a [`MoveExtResult`]; the VM uses `size_u16` to
/// advance PC, identical to the main VM's `param_3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveExtResult {
    /// Number of u16 words this handler consumed (added to PC).
    pub size_u16: i16,
}

impl MoveExtResult {
    /// Default-arm-equivalent: `param_3 = 1` (advance by one word, the
    /// sub-opcode itself).
    pub const fn default_arm() -> Self {
        Self { size_u16: 1 }
    }

    pub const fn with_size(size_u16: i16) -> Self {
        Self { size_u16 }
    }
}

/// Engine-side callbacks for the move VM.
///
/// All methods have default impls so a minimal host (only animation-state
/// reads) compiles. Each method documents which SCUS function it stands in for.
pub trait MoveHost {
    /// Op 0x03 sin/cos table lookup. The original reads a table at
    /// `_DAT_8007B81C` (sin) and `_DAT_8007B7F8` (cos), each 0x1000-entry
    /// signed-16-bit, indexed by `actor[+0x96] & 0xFFF`. Hosts return the
    /// `(sin, cos)` pair for the given index.
    fn rotation_lut(&self, _index: u16) -> (i16, i16) {
        (0, 0)
    }

    /// Op 0x16 STUB. The original's `FUN_80024C80` is a pure `jr ra` — a
    /// no-op the VM only calls to mirror dispatch. Default impl is a no-op.
    fn stub_16(&mut self, _state: &mut ActorState, _arg: i16) {}

    /// Op 0x17 — `func_0x801f30c4(actor, op[1])`. Overlay-resident effect
    /// trigger (the actual handler isn't yet decompiled). Default no-op.
    fn ext_17(&mut self, _state: &mut ActorState, _arg: i16) {}

    /// Op 0x1D — write `DAT_8007B6DE = op[1]`. A single-byte global slot.
    /// Hosts model their own global state; default impl ignores.
    fn global_write_1d(&mut self, _value: u16) {}

    /// Op 0x20 — `(*gp[0x714])(actor, op[1], op[2])`. Indirect call through
    /// an SDK-style vtable. The function is not yet identified. Default no-op.
    fn ext_20(&mut self, _state: &mut ActorState, _arg0: i16, _arg1: i16) {}

    /// Op 0x21 — `DAT_8007BE60 + face_id*12` write. The original maintains a
    /// `12 * N`-byte table indexed by face id. The VM passes the decoded
    /// face_id + the four u16s + final i32. Default no-op.
    fn face_rotation_setup(&mut self, _face_id: u8, _params: [u16; 4], _target: i32) {}

    /// Op 0x25 — `FUN_80021B04`. Spawns a child actor at the slot given by
    /// `op[1]`. Default no-op (engine-side actor pool).
    fn spawn_child(&mut self, _state: &mut ActorState, _slot: i16) {}

    /// Op 0x2C — heap allocation for the keyframe buffer.
    /// `bytes` is `w * h * 2` from the encoded params. Original calls
    /// `FUN_80017888(0, bytes)` to allocate. Returns the heap pointer (or
    /// `0` to indicate "use the inline buffer").
    fn keyframe_alloc(&mut self, _bytes: i32) -> i32 {
        0
    }

    /// Op 0x2C — initialize the keyframe descriptor at `&actor + 0xA0`.
    /// Original calls `FUN_8005842C(descriptor_ptr, buffer_ptr)`. Default
    /// no-op.
    fn keyframe_init(&mut self, _state: &mut ActorState, _buffer_ptr: i32) {}

    /// Op 0x30 — release the keyframe buffer if heap-allocated. Default
    /// no-op (FUN_800583C8 in the original).
    fn keyframe_free(&mut self, _state: &mut ActorState, _buffer_ptr: i32) {}

    /// Op 0x40 — `FUN_80058490(local_30_block, op[5], op[6])`. Where
    /// `local_30_block` is a 4-word stack array populated from `op[1..5]`.
    /// Default no-op.
    fn func58490(&mut self, _block: [u16; 4], _arg0: i16, _arg1: i16) {}

    /// Multiplier byte read by op 0x0A KEYFRAME_LOAD — `DAT_1F80037D`.
    /// Default 0x100 (a sane multiplier; the original is initialized at boot).
    fn keyframe_curve_multiplier(&self) -> u8 {
        0x10
    }

    /// Extension VM entry. The VM has already decoded the sub-opcode word.
    /// Hosts may stub the entire extension VM by returning
    /// `MoveExtResult::default_arm()` or implement per-sub-op behaviour.
    fn ext_dispatch(
        &mut self,
        state: &mut ActorState,
        sub_opcode: u16,
        operand: &[u16],
    ) -> MoveExtResult {
        // Default impl walks the well-understood arms and falls through to
        // `default_arm()` for the rest.
        ext_default_dispatch(self, state, sub_opcode, operand)
    }

    /// Extension sub-op 0x01 — `func_0x8001a068(fmt, x, y, z)` — debug print
    /// of world coords. Default no-op.
    fn ext_debug_world(&mut self, _x: i16, _y: i16, _z: i16) {}

    /// Extension sub-op 0x05 / 0x30 — `func_0x80056798()` — opaque, returns
    /// the size to advance by. Default returns `default_arm()`.
    fn ext_func56798(&mut self, _state: &mut ActorState) -> MoveExtResult {
        MoveExtResult::default_arm()
    }

    /// Extension sub-op 0x0E — `FUN_801E45BC(out_xy, sub_a, sub_b, mode)` —
    /// computes a midpoint-style position; the VM passes the three i16
    /// triples plus the mode word read from `actor[+0x50]`. Default no-op
    /// (the VM still writes the result fields if the host returns them).
    fn ext_midpoint_set(
        &mut self,
        _state: &mut ActorState,
        _a: [i16; 3],
        _b: [i16; 3],
        _offsets: [i16; 3],
        _mode: u16,
    ) {
    }

    /// Extension sub-op 0x13 / 0x14 — `func_0x8003CE64(flag_index)` — query
    /// the fourth-flag-bank bitfield (`DAT_80086D70`). Returns 0 if clear,
    /// non-zero otherwise. The VM uses the result to decide between the
    /// fall-through and predicate-skip arms.
    fn ext_query_flag_bank(&self, _flag_index: i16) -> u32 {
        0
    }

    /// Extension sub-op 0x1C — `func_0x8003CE08(flag_index)` — set a bit in
    /// the fourth-flag-bank.
    fn ext_set_flag_bank(&mut self, _flag_index: i16) {}

    /// Extension sub-op 0x1D — `func_0x8003CE34(flag_index)` — clear a bit
    /// in the fourth-flag-bank.
    fn ext_clear_flag_bank(&mut self, _flag_index: i16) {}

    /// Extension sub-op 0x29 — `func_0x8003C5F0(0, &DAT_1F80035C[idx], 2,
    /// current, -value, ticks)` — schedules a per-frame ramp of a scratchpad
    /// slot. Default no-op.
    fn ext_scratchpad_ramp(&mut self, _slot_index: i16, _target: i16, _ticks: i16) {}

    /// Extension sub-op 0x29 immediate path — direct write to scratchpad
    /// slot. The VM only fires this when `ticks == 0`.
    fn ext_scratchpad_write(&mut self, _slot_index: i16, _value: i16) {}

    /// Extension sub-op 0x2C — `FUN_801D31B0(actor, op)` — overlay-resident
    /// helper. Default no-op.
    fn ext_func801d31b0(&mut self, _state: &mut ActorState, _operand: &[u16]) {}

    /// Extension sub-op 0x2E — `func_0x80059010(...)` and friends — emits
    /// a packet onto the GP0 OT (the PSX render-list ring). Default no-op.
    fn ext_emit_ot_packet(&mut self, _operand: &[u16]) {}

    /// Extension sub-op 0x2F — `_DAT_8007B9D8 = (i32) op[1]`. A globally-
    /// shared 32-bit slot. Default no-op.
    fn ext_set_8007b9d8(&mut self, _value: i32) {}

    /// Extension sub-op 0x3A — `func_0x80019B28(z1, x1, z2, x2)` — angle
    /// computation between the actor's world coords and the player's. The
    /// VM stores the result at `op[op[1] + 3]`. Default returns 0.
    fn ext_compute_angle(&self, _state: &ActorState) -> u16 {
        0
    }

    /// Extension sub-op 0x3B — read `_DAT_8007B898 + 0x22` triple via
    /// `func_0x8003D064`. Returns the resolved actor index. Default returns
    /// `None` (no party member).
    fn ext_party_member_lookup(&self, _slot: i16) -> Option<[i16; 3]> {
        None
    }

    /// Extension sub-op 0x3C — fade colour write. When `ticks == 0`, writes
    /// the three colour bytes immediately to scratchpad globals; non-zero
    /// schedules a ramp. The VM passes both branches through this hook so
    /// hosts can model whichever is convenient. PC += 6.
    fn ext_fade_color(&mut self, _rgb: [u8; 3], _ticks: u16) {}

    /// Extension sub-op 0x18/0x19/0x1A — three world-derived writes to a
    /// shared 0x14-byte struct at `iVar16 - 0x7FF7C008` (offset-resolved by
    /// `*op[1] * 0x14`). Hosts model the table; the VM passes index + the
    /// 5 u16 values written. Default no-op.
    fn ext_world_struct_write(&mut self, _index: i16, _values: [i16; 5]) {}

    /// Extension sub-op 0x17 — initial / configuration write to the same
    /// struct as `ext_world_struct_write`. 5 u16 values from operand+6..16.
    fn ext_world_struct_init(&mut self, _index: i16, _values: [i16; 5]) {}

    /// Read 4 bytes from the move-VM 16-slot scratch table at
    /// `&DAT_801F3498`. Each slot is 8 bytes wide; `dword_off` is `0` or
    /// `4`. Used by ext sub-ops 0x26 / 0x32 / 0x35 (load) and 0x12 / 0x28
    /// (read-modify-write). Default impl returns 0 — hosts that care
    /// about cross-actor save/restore override this.
    fn move_slot_load_u32(&self, _slot: u16, _dword_off: u8) -> u32 {
        0
    }

    /// Write 4 bytes to the move-VM 16-slot scratch table. See
    /// [`Self::move_slot_load_u32`]. Used by ext sub-ops 0x25 / 0x31 (and
    /// the partial-slot pair below). `dword_off` is `0` or `4`.
    fn move_slot_save_u32(&mut self, _slot: u16, _dword_off: u8, _value: u32) {}

    /// Read 2 bytes from the move-VM 16-slot scratch table. `byte_off` is
    /// `0`..`6` (even). Used by ext sub-op 0x35 (single-u16 reload).
    fn move_slot_load_u16(&self, _slot: u16, _byte_off: u8) -> u16 {
        0
    }

    /// Write 2 bytes to the move-VM 16-slot scratch table. `byte_off` is
    /// `0`..`6` (even). Used by ext sub-ops 0x27 (3 × u16 from `+0x90`) and
    /// 0x34 (1 × u16 from `+0x72`).
    fn move_slot_save_u16(&mut self, _slot: u16, _byte_off: u8, _value: u16) {}

    /// Read the move-VM global predicate at `&DAT_801F22F4` (set by ext
    /// sub-op 0x08, cleared by 0x09). Sub-ops 0x0A / 0x0B branch on it.
    /// Default returns 0 — equivalent to "predicate is false / never set"
    /// (sub-op 0x0A skips, sub-op 0x0B falls through).
    fn move_global_predicate_get(&self) -> u32 {
        0
    }

    /// Write the move-VM global predicate. Used by ext sub-ops 0x08 / 0x09.
    fn move_global_predicate_set(&mut self, _value: u32) {}

    /// Read the move-VM global counter at `&DAT_801F22F6`. Cleared by ext
    /// sub-op 0x0F; cycled mod 16 by sub-op 0x10 (which also writes the
    /// captured low byte into `actor[+0x86]`).
    fn move_global_counter_get(&self) -> u16 {
        0
    }

    /// Write the move-VM global counter. Used by ext sub-ops 0x0F / 0x10.
    fn move_global_counter_set(&mut self, _value: u16) {}

    /// Player position read from `_DAT_8007C364 + 0x14..+0x1A` — a 3 × i16
    /// triple. Used by ext sub-op 0x2A (world position lerp toward player)
    /// and the bbox-vs-player tests at 0x06 / 0x07 / 0x36 / 0x39. Default
    /// returns the origin (engine-vm test hosts override).
    fn move_player_world_xyz(&self) -> [i16; 3] {
        [0, 0, 0]
    }

    /// Map fixed-origin pair `(_DAT_80089118, _DAT_80089120)` — the (x, z)
    /// origin used by ext sub-op 0x24 (world position lerp toward fixed
    /// map origin). Default returns `(0, 0)`.
    fn move_fixed_origin_xz(&self) -> (i32, i32) {
        (0, 0)
    }

    /// Read `_DAT_1F800393` — a u8 scratchpad slot used by ext sub-op 0x23
    /// as the numerator of a 12.0 fixed-point ramp ratio against the
    /// operand-supplied denominator. Default returns 0 (the lerp becomes
    /// a no-op).
    fn move_dat_1f800393(&self) -> u8 {
        0
    }

    /// Read `_DAT_8007C348` — the axis offset used by ext sub-ops 0x36 / 0x37
    /// for the `0x8E - axis` threshold predicate. Default returns 0 (so the
    /// threshold collapses to `op[2] < 0x8E` / `op[2] > 0x8E`).
    fn move_axis_threshold(&self) -> i16 {
        0
    }

    /// Read a u16 from the actor's move-bytecode buffer at the given absolute
    /// word offset (`actor[+0x48][word_off]`). Used by ext sub-ops 0x1B (copy
    /// loop) and 0x1E (read-modify-write). Default returns 0 — hosts that
    /// model the move buffer (e.g. engine-core's `World`) override.
    fn move_bytecode_read_u16(&self, _word_off: usize) -> u16 {
        0
    }

    /// Write a u16 to the actor's move-bytecode buffer. Used by ext sub-ops
    /// 0x04 (write actor world to operand slot), 0x1B (copy loop), and 0x1E
    /// (read-modify-write). Default no-op.
    fn move_bytecode_write_u16(&mut self, _word_off: usize, _value: u16) {}
}

/// Clean-room RGB→HSV port of `FUN_8001a78c`. Inputs are 0..255; outputs are
/// `(H ∈ 0..0x167, S ∈ 0..255, V ∈ 0..255)`. Used by ext sub-ops 0x1F / 0x20
/// to rotate a packed RGB color in HSV space.
///
/// The original SCUS implementation uses signed-integer division with
/// fixed-point scaling by `0x100` and the `0x60 / 0x100 = 60/256` segment
/// multiplier — the result space is effectively degrees in 0..360 (= 0x168).
fn rgb_to_hsv(r: i32, g: i32, b: i32) -> (i32, i32, i32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let diff = max - min;
    let v = max;
    if max == 0 {
        return (0, 0, 0);
    }
    let s = (diff * 0x100) / max;
    if s == 0 {
        return (0, 0, v);
    }
    // Hue computation in segment-based form.
    let mut h = if r == max {
        ((g - b) * 0x100) / diff
    } else if g == max {
        ((b - r) * 0x100) / diff + 0x200
    } else {
        ((r - g) * 0x100) / diff + 0x400
    };
    h = (h * 0x3C) >> 8;
    if h < 0 {
        h += 0x168;
    }
    (h, s, v)
}

/// Clean-room HSV→RGB port of `FUN_8001a8dc`. `H ∈ 0..0x167`, `S, V ∈ 0..256`.
/// Returns `(R, G, B)` each in 0..255 (caller may clamp further; FUN_8001a6c8
/// caps at 0xF8). Used by ext sub-ops 0x1F / 0x20.
fn hsv_to_rgb(h: i32, s: i32, v: i32) -> (i32, i32, i32) {
    let s = s.clamp(0, 0x100);
    let v = v.clamp(0, 0x100);
    let mut h_scaled = (h.rem_euclid(0x168)) * 0x100;
    if h_scaled < 0 {
        h_scaled += 0x16800;
    }
    let f = ((h_scaled / 0x3C) & 0xFF) as u32 as i32;
    let segment = (h_scaled / 0x3C) >> 8;
    let p = (v * (0x100 - s)) >> 8;
    let q = (v * (0x100 - ((s * f) >> 8))) >> 8;
    let t = (v * (0x100 - ((s * (0x100 - f)) >> 8))) >> 8;
    match segment {
        0 | 6 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        5 => (v, p, q),
        _ => (0, 0, 0),
    }
}

fn ext_default_dispatch<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    sub_opcode: u16,
    operand: &[u16],
) -> MoveExtResult {
    // The original C reads `*(short *)(param_2 + N)` where param_2 points
    // at the opcode word itself. So `param_2 + 4` is u16-index 2 (the third
    // word: opcode + sub_opcode + first param). `op_w(N)` reads u16-index N
    // of `operand`, which is `bytecode[pc..]`.
    let op_w = |i: usize| -> u16 { operand.get(i).copied().unwrap_or(0) };

    match sub_opcode {
        // 0x00 — falls into default arm (size 1).
        0x00 => MoveExtResult::default_arm(),

        // 0x01 — `func_0x8001a068("EFC %d %d %d", x, y, z)` debug print.
        // Original sets `iVar16 = 0x20000` then breaks (size = 2).
        0x01 => {
            host.ext_debug_world(state.world_x, state.world_y, state.world_z);
            MoveExtResult::with_size(2)
        }

        // 0x02 — clear face_rotation. Default-arm size.
        0x02 => {
            state.face_rotation = 0;
            MoveExtResult::default_arm()
        }

        // 0x03 — clear flags bit 0x1000.
        0x03 => {
            state.flags &= !0x1000;
            MoveExtResult::default_arm()
        }

        // 0x04 — write actor world XYZ into the operand slot at u16-index
        // `op[2] + 3`. The "self-modifying" pattern stores a copy of the
        // current world coords into the move-bytecode itself; the absolute
        // word offset is `state.pc + op[2] + 3`. 3 consecutive writes for
        // x/y/z. Default-arm size.
        0x04 => {
            let base = state.pc as usize + op_w(2) as usize + 3;
            host.move_bytecode_write_u16(base, state.world_x as u16);
            host.move_bytecode_write_u16(base + 1, state.world_y as u16);
            host.move_bytecode_write_u16(base + 2, state.world_z as u16);
            MoveExtResult::default_arm()
        }

        // 0x05 — opaque func56798().
        0x05 => host.ext_func56798(state),

        // 0x06 / 0x07 — bbox-vs-player test. The original canonicalises the
        // box in-place by swapping `op_w(2)/(4)` if `op_w(4) < op_w(2)`, then
        // tests whether the player position falls inside `[(xa-0x40)/0x80,
        // (xb+0x40)/0x80]` × `[(za-0x40)/0x80, (zb+0x40)/0x80]`. 0x06 means
        // "default-arm if inside, size 7 if outside"; 0x07 means the opposite.
        // The size-7 branch skips a 7-u16 follow-up payload. We can't mutate
        // the operand stream from here (it's a read-only slice), so the port
        // forwards the predicate to the host: `move_player_world_xyz` returns
        // the player position. If the host reports the origin (the default
        // impl), the test always reads "outside the box" — so we model 0x06
        // as a skip (size 7) and 0x07 as a continue (default-arm). Hosts
        // that surface real player coords get the right behavior.
        0x06 | 0x07 => {
            let player = host.move_player_world_xyz();
            // op_w(2)..op_w(5) are the four corner u16s. We do NOT swap in
            // place because the operand is a read-only slice.
            let mut xa = op_w(2) as i16 as i32;
            let mut xb = op_w(4) as i16 as i32;
            let mut za = op_w(3) as i16 as i32;
            let mut zb = op_w(5) as i16 as i32;
            if xb < xa {
                std::mem::swap(&mut xa, &mut xb);
            }
            if zb < za {
                std::mem::swap(&mut za, &mut zb);
            }
            // Original scales by 0x80 with a 0x40 half-cell margin.
            let outside = (player[0] as i32) < xa * 0x80 + 0x40
                || (player[2] as i32) < za * 0x80 + 0x40
                || xb * 0x80 + 0x40 < player[0] as i32
                || zb * 0x80 + 0x40 < player[2] as i32;
            // 0x06: outside is the skip path (size 7); 0x07: inside is.
            let skip = if sub_opcode == 0x06 {
                outside
            } else {
                !outside
            };
            if skip {
                MoveExtResult::with_size(7)
            } else {
                MoveExtResult::default_arm()
            }
        }

        // 0x08 — `DAT_801F22F4 = 1` (set move-VM global predicate).
        0x08 => {
            host.move_global_predicate_set(1);
            MoveExtResult::default_arm()
        }

        // 0x09 — `DAT_801F22F4 = 0` (clear).
        0x09 => {
            host.move_global_predicate_set(0);
            MoveExtResult::default_arm()
        }

        // 0x0A — predicate gate: if `DAT_801F22F4 != 0` falls into default
        // (size 1, advance and continue); else returns size 3 (skip a 3-u16
        // payload). Per the dump's `iVar16 = 3; if (DAT_801f22f4 != 0)
        // goto LAB_801d38c8; ...; default:` shape — `LAB_801d38c8` is the
        // default-arm label, and the fall-through hits `iVar16 << 0x10 break`
        // with `iVar16 = 3`.
        0x0A => {
            if host.move_global_predicate_get() != 0 {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(3)
            }
        }

        // 0x0B — opposite predicate (skip when set).
        0x0B => {
            if host.move_global_predicate_get() == 0 {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(3)
            }
        }

        // 0x0C — `actor[+0x50] = op_w(2)` (set midpoint blend / sub-state).
        0x0C => {
            state.field_50 = op_w(2);
            MoveExtResult::default_arm()
        }

        // 0x0D — `actor[+0x50] += op_w(2)` (additive variant).
        0x0D => {
            state.field_50 = state.field_50.wrapping_add(op_w(2));
            MoveExtResult::default_arm()
        }

        // 0x0E — midpoint position calc + write to actor world.
        // Reads param_2 + 4/6/8 (a) + 10/12/14 (off) + 16/18/20 (b). The
        // midpoint helper consumes `actor[+0x50]` (the blend amount set by
        // ext ops 0x0C/0x0D). Original returns `iVar16 = 0xb0000` → size 11
        // (opcode + sub-op + 9 operand u16s).
        0x0E => {
            let a = [op_w(2) as i16, op_w(3) as i16, op_w(4) as i16];
            let off = [op_w(5) as i16, op_w(6) as i16, op_w(7) as i16];
            let b = [op_w(8) as i16, op_w(9) as i16, op_w(10) as i16];
            let mode = state.field_50;
            host.ext_midpoint_set(state, a, b, off, mode);
            // Pre-stage the world coords the way the original does (the
            // helper may overwrite them depending on `mode`, but a host that
            // doesn't model the helper still gets the average write-through).
            state.world_x = off[0].wrapping_add(((a[0] as i32 + b[0] as i32) >> 1) as i16);
            state.world_y = off[1].wrapping_add(((a[1] as i32 + b[1] as i32) >> 1) as i16);
            state.world_z = off[2].wrapping_add(((a[2] as i32 + b[2] as i32) >> 1) as i16);
            MoveExtResult::with_size(11)
        }

        // 0x0F — `DAT_801F22F6 = 0` (clear move-VM global counter).
        0x0F => {
            host.move_global_counter_set(0);
            MoveExtResult::default_arm()
        }

        // 0x10 — wrap `DAT_801F22F6` mod 16, capture low byte into
        // `actor.field_86` (preserving the high byte), then increment the
        // counter. Per the original:
        //   if (0xf < c) c = 0;
        //   captured = c & 0xff;
        //   c += 1;
        //   actor.field_86 = (actor.field_86 & 0xff00) | captured;
        0x10 => {
            let mut counter = host.move_global_counter_get();
            if counter > 0xF {
                counter = 0;
            }
            let captured = counter & 0xFF;
            host.move_global_counter_set(counter.wrapping_add(1));
            state.field_86 = (state.field_86 & 0xFF00) | captured;
            MoveExtResult::default_arm()
        }

        // 0x11 — save `actor[+0x14..+0x1C]` (world coords + Y mirror) into
        // the scratch slot indexed by `actor.field_86 & 0xFF`. Same shape
        // as sub-op 0x25 but the slot index comes from the cycle counter
        // updated by sub-ops 0x0F / 0x10 instead of an operand u16.
        0x11 => {
            let slot = state.field_86 & 0xFF;
            let lo = (state.world_x as u16 as u32) | ((state.world_y as u16 as u32) << 16);
            let hi = (state.world_z as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            MoveExtResult::default_arm()
        }

        // 0x12 — slot-indexed midpoint variant of 0x0E. Reads
        // `actor[+0x86] & 0xFF` as a slot index, loads `slot.x/y/z` from the
        // 16-slot scratch table at `&DAT_801F3498`, then computes
        //   actor.world.{x,y,z} = op[2/3/4] + (slot.{x,y,z} + op[5/6/7]) / 2
        // before passing through the same midpoint helper as 0x0E. The
        // operand layout is `(op[2..4]=offset, op[5..7]=b)` — `a` comes from
        // the slot, not from the bytecode. Original returns `iVar16 =
        // 0x80000` → size 8 (opcode + sub-op + 6 operand u16s).
        0x12 => {
            let slot = state.field_86 & 0xFF;
            let slot_lo = host.move_slot_load_u32(slot, 0);
            let slot_hi = host.move_slot_load_u32(slot, 4);
            let a = [
                (slot_lo & 0xFFFF) as i16,
                ((slot_lo >> 16) & 0xFFFF) as i16,
                (slot_hi & 0xFFFF) as i16,
            ];
            let b = [op_w(5) as i16, op_w(6) as i16, op_w(7) as i16];
            let off = [op_w(2) as i16, op_w(3) as i16, op_w(4) as i16];
            let mode = state.field_50;
            host.ext_midpoint_set(state, a, b, off, mode);
            state.world_x = off[0].wrapping_add(((a[0] as i32 + b[0] as i32) >> 1) as i16);
            state.world_y = off[1].wrapping_add(((a[1] as i32 + b[1] as i32) >> 1) as i16);
            state.world_z = off[2].wrapping_add(((a[2] as i32 + b[2] as i32) >> 1) as i16);
            MoveExtResult::with_size(8)
        }

        // 0x13 / 0x14 — flag-bank predicate tests against the fourth flag
        // bank `DAT_80086D70` (via `func_0x8003CE64`). op_w(2) = flag index.
        // Both jump to the shared `LAB_801D4830` epilogue: returns 1 when
        // `predicate != 0`, else 4. 0x13 falls through unconditionally
        // (`goto LAB_801D4830`); 0x14 inverts (returns 4 when predicate is
        // set, default-arm when clear).
        0x13 => {
            if host.ext_query_flag_bank(op_w(2) as i16) != 0 {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(4)
            }
        }
        0x14 => {
            if host.ext_query_flag_bank(op_w(2) as i16) != 0 {
                MoveExtResult::with_size(4)
            } else {
                MoveExtResult::default_arm()
            }
        }

        // 0x15 / 0x16 — set a flag bit on actor.flags (mask 0x800000 / 0x200000).
        0x15 => {
            state.flags |= 0x800000;
            MoveExtResult::default_arm()
        }
        0x16 => {
            state.flags |= 0x200000;
            MoveExtResult::default_arm()
        }

        // 0x17 — world-struct init. op_w(2) = idx (param_2+4); op_w(3..7) = 5 vals.
        0x17 => {
            let idx = op_w(2) as i16;
            let vals = [
                op_w(3) as i16,
                op_w(4) as i16,
                op_w(5) as i16,
                op_w(6) as i16,
                op_w(7) as i16,
            ];
            host.ext_world_struct_init(idx, vals);
            MoveExtResult::default_arm()
        }
        // 0x18/0x19/0x1A — world-struct write variants. Same op_w(2) = idx.
        0x18..=0x1A => {
            let idx = op_w(2) as i16;
            let vals = [
                op_w(3) as i16,
                op_w(4) as i16,
                op_w(5) as i16,
                op_w(6) as i16,
                op_w(7) as i16,
            ];
            host.ext_world_struct_write(idx, vals);
            MoveExtResult::default_arm()
        }

        // 0x1B — in-bytecode copy loop. For `i in 0..op[4]`:
        //   buffer[state.pc + op[3] + i + 5] = buffer[state.pc + op[2] + i + 5]
        // The base offset of 5 (= u16-index 5) targets the operand region
        // *past* the count word — the bytecode following this instruction
        // is treated as an inline scratch buffer indexed by op[2]/op[3].
        // Falls into default-arm regardless of the count.
        0x1B => {
            let count = op_w(4) as i16;
            if count > 0 {
                let src_base = state.pc as usize + op_w(2) as usize + 5;
                let dst_base = state.pc as usize + op_w(3) as usize + 5;
                for i in 0..count as usize {
                    let v = host.move_bytecode_read_u16(src_base + i);
                    host.move_bytecode_write_u16(dst_base + i, v);
                }
            }
            MoveExtResult::default_arm()
        }

        // 0x1C / 0x1D — set / clear flag bank. op_w(2) = flag index.
        0x1C => {
            host.ext_set_flag_bank(op_w(2) as i16);
            MoveExtResult::with_size(3)
        }
        0x1D => {
            host.ext_clear_flag_bank(op_w(2) as i16);
            MoveExtResult::with_size(3)
        }

        // 0x1E — in-place add: `buffer[state.pc + op[2] + 4] += op[3]`.
        // Read-modify-write of a single u16 inside the move bytecode.
        // Wrapping i16 add per the original `*(short *)(...) + *(short *)(...)`.
        0x1E => {
            let off = state.pc as usize + op_w(2) as usize + 4;
            let cur = host.move_bytecode_read_u16(off) as i16;
            let new = cur.wrapping_add(op_w(3) as i16);
            host.move_bytecode_write_u16(off, new as u16);
            MoveExtResult::default_arm()
        }

        // 0x1F / 0x20 — HSV-space color ramp on `actor[+0xa0]` (sub-op 0x1F)
        // or `actor[+0xa4]` (sub-op 0x20). The packed u32 holds an RGB triple:
        // R = byte 0, G = byte 1, B = byte 2 (bit 24-31 reserved).
        //
        // Per FUN_801D362C (overlay_0897_801d362c, case 0x1f/0x20):
        //   1. Decompose puVar14[0..3] into R/G/B (each 0..255).
        //   2. RGB→HSV via FUN_8001a78c → (H ∈ 0..0x167, S ∈ 0..255, V ∈ 0..255).
        //   3. H += op[2]; wrap into [0, 0x167]. S += op[3], clamp 0..255.
        //      V += op[4], clamp 0..255.
        //   4. HSV→RGB via FUN_8001a6c8 (which clamps each output to 0..0xF8).
        //   5. Re-pack `puVar14[0] = R | G<<8 | B<<16`.
        //
        // Returns default_arm() (size 1) — the bytecode operand stream is
        // re-interpreted as outer opcode 0x1F / 0x20 on the next dispatch
        // (intentional self-modifying layout — see also sub-ops 0x04/0x1B/0x1E).
        0x1F | 0x20 => {
            let kd_offset = if sub_opcode == 0x1F { 0 } else { 2 };
            let lo = state.keyframe_desc[kd_offset];
            let hi = state.keyframe_desc[kd_offset + 1];
            let r = (lo & 0xFF) as i32;
            let g = ((lo >> 8) & 0xFF) as i32;
            let b = (hi & 0xFF) as i32;
            let (mut h, mut s, mut v) = rgb_to_hsv(r, g, b);
            h += op_w(2) as i16 as i32;
            while h < 0 {
                h += 0x168;
            }
            while h > 0x167 {
                h -= 0x168;
            }
            s = (s + op_w(3) as i16 as i32).clamp(0, 0xFF);
            v = (v + op_w(4) as i16 as i32).clamp(0, 0xFF);
            let (nr, ng, nb) = hsv_to_rgb(h, s, v);
            // FUN_8001a6c8 clamps each channel to 0..0xF8.
            let nr = nr.min(0xF8) as u16;
            let ng = ng.min(0xF8) as u16;
            let nb = nb.min(0xF8) as u16;
            state.keyframe_desc[kd_offset] = nr | (ng << 8);
            state.keyframe_desc[kd_offset + 1] = (hi & 0xFF00) | nb;
            MoveExtResult::default_arm()
        }

        // 0x21 — `actor.anim_3c..40 += op_w(2..4)`.
        0x21 => {
            state.anim_3c = state.anim_3c.wrapping_add(op_w(2) as i16);
            state.anim_3e = state.anim_3e.wrapping_add(op_w(3) as i16);
            state.anim_40 = state.anim_40.wrapping_add(op_w(4) as i16);
            MoveExtResult::default_arm()
        }

        // 0x22 — `actor.world += op_w(2..4)`.
        0x22 => {
            state.world_x = state.world_x.wrapping_add(op_w(2) as i16);
            state.world_y = state.world_y.wrapping_add(op_w(3) as i16);
            state.world_z = state.world_z.wrapping_add(op_w(4) as i16);
            MoveExtResult::default_arm()
        }

        // 0x23 — animation lerp toward target world coords using the
        // scratchpad ramp counter at `_DAT_1F800393`. Per the dump:
        //   t = (DAT_1F800393 << 12) / op[5];                  // 12.0 ratio
        //   anim_3c -= (anim_3c * t) >> 12;                     // remove old
        //   anim_3e -= (anim_3e * t) >> 12;
        //   anim_40 -= (anim_40 * t) >> 12;
        //   anim_3c += ((op[2] - actor.world_x) * t) >> 12;     // toward target
        //   anim_3e += ((op[3] - actor.world_y) * t) >> 12;
        //   anim_40 += ((op[4] - actor.world_z) * t) >> 12;
        // The original `trap(0x1c00)` / `trap(0x1800)` divide-by-zero traps are
        // skipped at the source-line level by the MIPS divide-trap pattern;
        // we simply guard `denom == 0` and skip the update. This is faithful
        // to the in-game behavior because the trap signal would terminate
        // execution rather than continue with a bogus ratio.
        0x23 => {
            let denom = op_w(5) as i16 as i32;
            if denom != 0 {
                let dat = host.move_dat_1f800393() as u32 as i32;
                let t = (dat << 12) / denom;
                let s = |v: i16| -> i16 { v.wrapping_sub(((v as i32 * t) >> 12) as i16) };
                state.anim_3c = s(state.anim_3c);
                state.anim_3e = s(state.anim_3e);
                state.anim_40 = s(state.anim_40);
                let dx = ((op_w(2) as i16 as i32 - state.world_x as i32) * t) >> 12;
                let dy = ((op_w(3) as i16 as i32 - state.world_y as i32) * t) >> 12;
                let dz = ((op_w(4) as i16 as i32 - state.world_z as i32) * t) >> 12;
                state.anim_3c = state.anim_3c.wrapping_add(dx as i16);
                state.anim_3e = state.anim_3e.wrapping_add(dy as i16);
                state.anim_40 = state.anim_40.wrapping_add(dz as i16);
            }
            MoveExtResult::default_arm()
        }

        // 0x24 / 0x2A — fixed-point lerp on actor world coords. Both share
        // the per-axis form `actor[axis] = op[axis] + ((target - op[axis]) *
        // op[axis_t]) >> 12`. The Y axis always lerps toward player.world_y
        // (with operand `op_w(3)` as base, `op_w(6)` as t). The X axis and
        // Z axis differ by sub-op:
        //   0x24 — uses `(_DAT_80089118, _DAT_80089120)` map origin: target
        //          = `-(op + origin)` (i.e. fixed map-relative anchor).
        //   0x2A — uses `(player.world_x, player.world_z)`.
        //
        // Operand layout: op_w(2,3,4) = base x/y/z; op_w(5)=t_x, op_w(6)=t_y,
        // op_w(7)=t_z (each scaled by `>> 12`).
        0x24 | 0x2A => {
            let player = host.move_player_world_xyz();
            let (origin_x, origin_z) = host.move_fixed_origin_xz();
            let base_x = op_w(2) as i16 as i32;
            let base_y = op_w(3) as i16 as i32;
            let base_z = op_w(4) as i16 as i32;
            let t_x = op_w(5) as i16 as i32;
            let t_y = op_w(6) as i16 as i32;
            let t_z = op_w(7) as i16 as i32;

            let (target_x, target_z) = if sub_opcode == 0x24 {
                // Fixed-origin path: the dump's `-op - origin` is the
                // signed displacement from `-(op + origin)`.
                (-(base_x + origin_x), -(base_z + origin_z))
            } else {
                (player[0] as i32, player[2] as i32)
            };

            // X axis.
            state.world_x = (base_x + (((target_x - base_x).wrapping_mul(t_x)) >> 12)) as i16;
            // Y axis (always vs. player).
            state.world_y =
                (base_y + (((player[1] as i32 - base_y).wrapping_mul(t_y)) >> 12)) as i16;
            // Z axis.
            state.world_z = (base_z + (((target_z - base_z).wrapping_mul(t_z)) >> 12)) as i16;
            MoveExtResult::default_arm()
        }

        // 0x25 — save `actor[+0x14..+0x1C]` (world coords + Y mirror) into
        // the 16-slot scratch table at `&DAT_801F3498`. Each slot is 8 bytes:
        // `slot[0..4] = (world_x:u16, world_y:u16)`,
        // `slot[4..8] = (world_z:u16, world_y_mirror:u16)`.
        0x25 => {
            let slot = op_w(2);
            let lo = (state.world_x as u16 as u32) | ((state.world_y as u16 as u32) << 16);
            let hi = (state.world_z as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            MoveExtResult::default_arm()
        }

        // 0x26 — load 8 bytes from the scratch slot back into
        // `actor[+0x14..+0x1C]`.
        0x26 => {
            let slot = op_w(2);
            let lo = host.move_slot_load_u32(slot, 0);
            let hi = host.move_slot_load_u32(slot, 4);
            state.world_x = (lo & 0xFFFF) as i16;
            state.world_y = ((lo >> 16) & 0xFFFF) as i16;
            state.world_z = (hi & 0xFFFF) as i16;
            state.world_y_mirror = ((hi >> 16) & 0xFFFF) as i16;
            MoveExtResult::default_arm()
        }

        // 0x27 — save the three tween-source u16s `actor[+0x90..+0x96]` into
        // the slot's first 6 bytes (slot[0/2/4] = tween_src_x/y/z).
        0x27 => {
            let slot = op_w(2);
            host.move_slot_save_u16(slot, 0, state.tween_src_x as u16);
            host.move_slot_save_u16(slot, 2, state.tween_src_y as u16);
            host.move_slot_save_u16(slot, 4, state.tween_src_z as u16);
            MoveExtResult::default_arm()
        }

        // 0x28 — load 3 × u16 from the slot, scale `+0x92/+0x94` by
        // `op_w(3)/op_w(4)` (with `>> 12` fixed-point shift), and clamp the
        // scaled outputs to `[-0xFF, 0xFF]`. Returns size 5 when the third
        // result needs the upper-bound branch (mirroring the original
        // `iVar16 = 5` shortcut), default-arm size otherwise.
        //
        // The upper-z bound check has to stay as a separate `if` (rather
        // than a `clamp` call) because the dump's "did we clamp z to the
        // upper bound" flag is what selects the return size.
        #[allow(clippy::manual_clamp)]
        0x28 => {
            let slot = op_w(2);
            let scale_y = op_w(3) as i16 as i32;
            let scale_z = op_w(4) as i16 as i32;
            state.tween_src_x = host.move_slot_load_u16(slot, 0) as i16;
            let raw_y = host.move_slot_load_u16(slot, 2) as i16 as i32;
            let raw_z = host.move_slot_load_u16(slot, 4) as i16 as i32;
            let mut y_scaled = ((raw_y * scale_y) >> 12) as i16;
            let mut z_scaled = ((raw_z * scale_z) >> 12) as i16;
            if y_scaled < -0xFF {
                y_scaled = -0xFF;
            }
            if y_scaled > 0xFF {
                y_scaled = 0xFF;
            }
            if z_scaled < -0xFF {
                z_scaled = -0xFF;
            }
            let upper_bound_branch = z_scaled > 0xFF;
            if upper_bound_branch {
                z_scaled = 0xFF;
            }
            state.tween_src_y = y_scaled;
            state.tween_src_z = z_scaled;
            if upper_bound_branch {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(5)
            }
        }

        // 0x31 — save `actor[+0x24..+0x2C]` (the three render banks +
        // `+0x2A` Y mirror) into the slot.
        0x31 => {
            let slot = op_w(2);
            let lo = (state.render_24 as u16 as u32) | ((state.render_26 as u16 as u32) << 16);
            let hi = (state.render_28 as u16 as u32) | ((state.world_y_mirror as u16 as u32) << 16);
            host.move_slot_save_u32(slot, 0, lo);
            host.move_slot_save_u32(slot, 4, hi);
            MoveExtResult::default_arm()
        }

        // 0x32 — load 8 bytes from the slot back into the render-bank
        // section at `+0x24..+0x2C`.
        0x32 => {
            let slot = op_w(2);
            let lo = host.move_slot_load_u32(slot, 0);
            let hi = host.move_slot_load_u32(slot, 4);
            state.render_24 = (lo & 0xFFFF) as i16;
            state.render_26 = ((lo >> 16) & 0xFFFF) as i16;
            state.render_28 = (hi & 0xFFFF) as i16;
            state.world_y_mirror = ((hi >> 16) & 0xFFFF) as i16;
            MoveExtResult::default_arm()
        }

        // 0x34 — save `actor[+0x72]` (`field_72`) into slot[0..2].
        0x34 => {
            let slot = op_w(2);
            host.move_slot_save_u16(slot, 0, state.field_72);
            MoveExtResult::default_arm()
        }

        // 0x35 — load slot[0..2] into `actor[+0x72]`.
        0x35 => {
            let slot = op_w(2);
            state.field_72 = host.move_slot_load_u16(slot, 0);
            MoveExtResult::default_arm()
        }

        // 0x29 — scratchpad ramp or immediate write. op_w(2)=slot,
        // op_w(3)=target, op_w(4)=ticks.
        0x29 => {
            let slot = op_w(2) as i16;
            let target = op_w(3) as i16;
            let ticks = op_w(4) as i16;
            if ticks != 0 {
                host.ext_scratchpad_ramp(slot, -target, ticks);
            } else {
                host.ext_scratchpad_write(slot, -target);
            }
            MoveExtResult::default_arm()
        }

        // 0x2B — `actor[+0xB4..+0xBC] = op_w(2..6)`. Writes 4 u16 anim-block
        // slots (`anim_block_u16` at byte-off 8/10/12/14 = `+0xB4/B6/B8/BA`).
        // Per the dump, the dispatcher's default-arm closes the case after
        // the writes; engines treating the sub-op as a 5-u16 instruction
        // can override `ext_dispatch` if they need accurate PC math.
        0x2B => {
            state.anim_block_u16_set(8, op_w(2));
            state.anim_block_u16_set(10, op_w(3));
            state.anim_block_u16_set(12, op_w(4));
            state.anim_block_u16_set(14, op_w(5));
            MoveExtResult::default_arm()
        }

        // 0x2C — overlay sub-routine.
        0x2C => {
            host.ext_func801d31b0(state, operand);
            MoveExtResult::default_arm()
        }

        // 0x2D — additive variant of 0x2B.
        // `actor[+0xB4..+0xBC] += op_w(2..6)`. Wrapping add per the
        // `*(short *)` semantics in the original.
        0x2D => {
            for (slot, idx) in [(8, 2), (10, 3), (12, 4), (14, 5)] {
                let cur = state.anim_block_u16(slot);
                let add = op_w(idx);
                state.anim_block_u16_set(slot, cur.wrapping_add(add));
            }
            MoveExtResult::default_arm()
        }

        // 0x2E — emit OT packet.
        0x2E => {
            host.ext_emit_ot_packet(operand);
            MoveExtResult::with_size(2)
        }

        // 0x2F — write `_DAT_8007B9D8`. op_w(2) = the i16 value.
        0x2F => {
            host.ext_set_8007b9d8(op_w(2) as i16 as i32);
            MoveExtResult::default_arm()
        }

        // 0x30 — opaque func56798 (same as 0x05).
        0x30 => host.ext_func56798(state),

        // 0x33 — `actor[+0xC0..+0xC8] += op_w(2..6)` (4 i16 anim-block slots
        // at byte-off 20/22/24/26 = `+0xC0/C2/C4/C6`). Wrapping add per the
        // `*(short *) +` semantics in the original.
        0x33 => {
            for (slot, idx) in [(20, 2), (22, 3), (24, 4), (26, 5)] {
                let cur = state.anim_block_u16(slot);
                let add = op_w(idx);
                state.anim_block_u16_set(slot, cur.wrapping_add(add));
            }
            MoveExtResult::default_arm()
        }

        // 0x36 / 0x37 — axis threshold against `0x8E - DAT_8007C348`.
        // 0x38 / 0x39 — squared-distance to the player.
        // All four predicates funnel through `LAB_801D4830`: returns 1 when
        // the predicate is true, else 4 (skip a 3-u16 follow-up payload).
        //
        //  - 0x36: op[2] < (0x8E - axis)            ; "outside lower bound"
        //  - 0x37: (0x8E - axis) < op[2]            ; "above upper bound"
        //  - 0x38: op[2]^2 < ((dx*dx) + (dz*dz))    ; "outside radius"
        //  - 0x39: ((dx*dx) + (dz*dz)) < op[2]^2    ; "inside radius"
        //
        // dx/dz are `actor.world - player.world`. The default `MoveHost`
        // returns the origin for the player, so engines that don't model
        // the player position get "actor at the origin offset" — close
        // enough for static unit tests; real hosts override.
        0x36..=0x39 => {
            let v = op_w(2) as i16 as i32;
            let predicate = match sub_opcode {
                0x36 => v < 0x8E - (host.move_axis_threshold() as i32),
                0x37 => 0x8E - (host.move_axis_threshold() as i32) < v,
                _ => {
                    let player = host.move_player_world_xyz();
                    let dx = state.world_x as i32 - player[0] as i32;
                    let dz = state.world_z as i32 - player[2] as i32;
                    let dist_sq = dx * dx + dz * dz;
                    let r_sq = v * v;
                    if sub_opcode == 0x38 {
                        r_sq < dist_sq
                    } else {
                        dist_sq < r_sq
                    }
                }
            };
            if predicate {
                MoveExtResult::default_arm()
            } else {
                MoveExtResult::with_size(4)
            }
        }

        // 0x3A — angle to player → operand slot at param_2 + op_w(2)*2 + 6.
        0x3A => {
            let _ = host.ext_compute_angle(state);
            MoveExtResult::default_arm()
        }

        // 0x3B — party-member position lookup → operand slot. op_w(3) = slot.
        0x3B => {
            let slot = op_w(3) as i16;
            let _ = host.ext_party_member_lookup(slot);
            MoveExtResult::default_arm()
        }

        // 0x3C — fade colour. op_w(2,3,4) = r/g/b (low bytes), op_w(5)=ticks.
        // The original reads `*(undefined1 *)(param_2 + 4)` etc — that's the
        // low byte of u16-index 2. op_w returns u16, so we cast to u8.
        0x3C => {
            let r = op_w(2) as u8;
            let g = op_w(3) as u8;
            let b = op_w(4) as u8;
            let ticks = op_w(5);
            host.ext_fade_color([r, g, b], ticks);
            MoveExtResult::with_size(6)
        }

        // Anything `>= 0x3D` is reserved / unknown — treat as default arm
        // since the original switch had no entries past 0x3C and would land
        // in `default:` (size 1 + iVar16 << 16, treated as fall-through here).
        _ => MoveExtResult::default_arm(),
    }
}

/// Decode and execute one instruction.
///
/// `bytecode` is the move buffer as u16 words (the original stores it as
/// `int* actor[+0x48]` and indexes with `actor[+0x70] * 2`); `state.pc` is
/// the current u16-word offset.
///
/// Returns a [`StepResult`] describing the outcome. The dispatcher loop is
/// the caller's responsibility — the move VM's outer loop in the original
/// was just "step until break".
pub fn step<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    bytecode: &[u16],
) -> StepResult {
    let pc_start = state.pc as usize;
    let Some(opcode) = bytecode.get(pc_start).copied() else {
        return StepResult::EndOfBuffer { opcode: 0 };
    };
    if opcode > 0x46 {
        return StepResult::EndOfBuffer { opcode };
    }

    // Operand reader — pure function over the bytecode slice, doesn't borrow
    // `state`. Out-of-range reads return 0 (matching the original's reliance
    // on the move buffer being correctly sized for each opcode).
    let read = |i: usize| -> u16 { bytecode.get(pc_start + i).copied().unwrap_or(0) };

    // Handlers return the size in u16 units (a.k.a. `param_3`). Halt and
    // Wait are signalled by setting `outcome` to non-Advance.
    let mut outcome = StepResult::Advance;
    let mut size: i16;

    match opcode {
        // 0x00 — ANIM_BANK_SET. size 4.
        // actor[+0x3C..+0x40] = op[1..3] << 3.
        0x00 => {
            state.anim_3c = (read(1) as i16).wrapping_shl(3);
            state.anim_3e = (read(2) as i16).wrapping_shl(3);
            state.anim_40 = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x01 — WORLD_ADD. size 4.
        0x01 => {
            let v1 = read(1) as i16;
            let v2 = read(2) as i16;
            let v3 = read(3) as i16;
            state.world_x = state.world_x.wrapping_add(v1);
            state.world_y = state.world_y.wrapping_add(v2);
            state.world_y_mirror = state.world_y_mirror.wrapping_add(v2);
            state.world_z = state.world_z.wrapping_add(v3);
            size = 4;
        }
        // 0x02 — BANK_SET_98. size 2. actor[+0x98] = op[1] << 3.
        0x02 => {
            state.tween_scale_y = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x03 — WORLD_ROTATE_ADD. size 2. Sin/cos rotated add into world XZ.
        0x03 => {
            let v1 = read(1) as i16;
            let (sin_v, cos_v) = host.rotation_lut(state.tween_scale_x as u16 & 0xFFF);
            // x += (sin * v1) >> 12
            let dx = ((sin_v as i32) * (v1 as i32)) >> 12;
            // z += (cos * v1) >> 12
            let dz = ((cos_v as i32) * (v1 as i32)) >> 12;
            state.world_x = state.world_x.wrapping_add(dx as i16);
            state.world_z = state.world_z.wrapping_add(dz as i16);
            size = 2;
        }
        // 0x04 — ANIM_BANK_2. size 4.
        0x04 => {
            state.anim_80 = (read(1) as i16).wrapping_shl(3);
            state.anim_82 = (read(2) as i16).wrapping_shl(3);
            state.anim_84 = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x05 — RENDER_BANK_ADD. size 4.
        0x05 => {
            state.render_24 = state.render_24.wrapping_add(read(1) as i16);
            state.render_26 = state.render_26.wrapping_add(read(2) as i16);
            state.render_28 = state.render_28.wrapping_add(read(3) as i16);
            size = 4;
        }
        // 0x06 — WRITE_26. size 2.
        0x06 => {
            state.render_26 = read(1) as i16;
            size = 2;
        }
        // 0x07 — WORLD_SET. size 4.
        0x07 => {
            state.world_x = read(1) as i16;
            state.world_y = read(2) as i16;
            state.world_y_mirror = read(2) as i16;
            state.world_z = read(3) as i16;
            size = 4;
        }
        // 0x08 — HALT. size 0, ends loop.
        0x08 => {
            state.flags |= 0x8;
            outcome = StepResult::Halt;
            size = 0;
        }
        // 0x09 — WAIT_SET. size 2, ends loop.
        0x09 => {
            state.wait_timer = (read(1) as i16).wrapping_shl(3);
            outcome = StepResult::Wait;
            size = 2;
        }
        // 0x0A — KEYFRAME_LOAD. variable size = 3 + count*3.
        0x0A => {
            state.flags |= 0x1000;
            let header_op2 = read(2);
            state.keyframe_count = header_op2 as u8;
            let count = header_op2 as i16;
            let curve_mul = host.keyframe_curve_multiplier() as i32;
            size = 3;
            for i in 0..count.max(0) {
                let base = (3 + 3 * i) as usize;
                // Note: byte writes to `+0xB0+i` are abstracted into anim_block.
                let descriptor_byte = read(base) as u8;
                state.anim_block_u16_set(0x04 + (i as usize) * 2, descriptor_byte as u16);

                let raw_b8 = read(base + 1) as i16 as i32;
                let scaled_b8 = (raw_b8 * curve_mul) >> 3;
                state.anim_block_u16_set(0x0C + (i as usize) * 2, scaled_b8 as u16);

                let raw_c8 = read(base + 2) as i16 as i32;
                let scaled_c8 = (raw_c8 * curve_mul) >> 3;
                state.anim_block_u16_set(0x1C + (i as usize) * 2, scaled_c8 as u16);

                size += 3;
            }
            // op[1] == 0 path also clears anim_block[0] + a 4-byte slot.
            if read(1) == 0 {
                state.anim_block_u16_set(0x00, 0);
            }
            state.local_flags = 0;
        }
        // 0x0B — DefaultBreak: drops out of switch with size 0; the original
        // skipped the size set, and the epilogue still ran (no PC advance).
        0x0B => {
            // No change to PC; runtime continues the loop unless a prior
            // handler set bVar3 = false. We keep size = 0, advance, and rely
            // on the caller to detect a no-progress loop if it cares.
            size = 0;
        }
        // 0x0C — composite control word build. size 6.
        0x0C => {
            let v1 = read(1) as i16 as i32;
            let v2 = read(2) as i16 as i32;
            let v3 = read(3) as i16 as i32;
            let v4 = read(4) as i16 as i32;
            state.field_74 = ((v1 << 24) as u32 | 0x4000_0000)
                .wrapping_add((v2) as u32)
                .wrapping_add((v3 as u32) << 8)
                .wrapping_add((v4 as u32) << 16);
            state.field_78 = read(5);
            size = 6;
        }
        // 0x0D — write tween_src_x = op[1] << 3. size 2.
        0x0D => {
            state.tween_src_x = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x0E — write field_72 = op[1]. size 2.
        0x0E => {
            state.field_72 = read(1);
            size = 2;
        }
        // 0x0F — write tween_src_y = op[1] << 3.
        0x0F => {
            state.tween_src_y = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x10 — write field_42.
        0x10 => {
            state.field_42 = read(1);
            size = 2;
        }
        // 0x11 — write tween_src_z = op[1] << 3.
        0x11 => {
            state.tween_src_z = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x12 — write field_7a.
        0x12 => {
            state.field_7a = read(1);
            size = 2;
        }
        // 0x13 — sub-mode init (16 ops mostly write the descriptor area).
        0x13 => {
            state.move_submode = 2;
            state.move_substate = 4;
            state.flags &= 0xFFFF_FFFD; // clear bit 2
            // The remaining 14 u16 writes target a chunk of fields we model
            // as `anim_block`. Map them generically.
            for i in 1..=15u16 {
                state.anim_block_u16_set((i as usize) * 2, read(i as usize));
            }
            size = 0x10;
        }
        // 0x14 — write four `anim_block` slots `<< 3`. size 5.
        0x14 => {
            for i in 0..4 {
                let v = (read(1 + i) as i16).wrapping_shl(3);
                state.anim_block_u16_set(0x14 + i * 2, v as u16);
            }
            size = 5;
        }
        // 0x15 — write field_52, with the 0x400 bit additionally clearing
        // flags & 0x80.
        0x15 => {
            let v = read(1);
            state.field_52 = v;
            if (v & 0x400) != 0 {
                state.flags &= 0xFFFF_FF7F;
            }
            size = 2;
        }
        // 0x16 — STUB. size 2. Calls FUN_80024C80 which is just `jr ra`.
        0x16 => {
            host.stub_16(state, read(1) as i16);
            size = 2;
        }
        // 0x17 — overlay-resident extension. size 2.
        0x17 => {
            host.ext_17(state, read(1) as i16);
            size = 2;
        }
        // 0x18 — save current PC into field_88, then field_8c = op[1].
        0x18 => {
            state.field_88 = state.pc as u16;
            state.field_8c = read(1);
            size = 2;
        }
        // 0x19 — counter-decrement loop (for 0x18 setup).
        0x19 => {
            let next = state.field_8c.wrapping_sub(1);
            if (state.field_8c & 0x4000) == 0 {
                state.field_8c = next;
                size = 1;
                if next > 40000 {
                    // Branch out: continue iterating, jump back.
                    state.pc = state.field_88 as i16;
                    return StepResult::Advance;
                }
            } else {
                state.pc = state.field_88 as i16;
                return StepResult::Advance;
            }
        }
        // 0x1A / 0x1B — second loop pair.
        0x1A => {
            state.field_8a = state.pc as u16;
            state.field_8e = read(1);
            size = 2;
        }
        0x1B => {
            let next = state.field_8e.wrapping_sub(1);
            if (state.field_8e & 0x4000) == 0 {
                state.field_8e = next;
                size = 1;
                if next > 40000 {
                    state.pc = state.field_8a as i16;
                    return StepResult::Advance;
                }
            } else {
                state.pc = state.field_8a as i16;
                return StepResult::Advance;
            }
        }
        // 0x1C — write field_ca = op[1] << 3.
        0x1C => {
            state.field_ca = (read(1) as i16).wrapping_shl(3) as u16;
            size = 2;
        }
        // 0x1D — global write.
        0x1D => {
            host.global_write_1d(read(1));
            size = 2;
        }
        // 0x1E — write 7 anim_block slots. size 8.
        0x1E => {
            state.move_submode = 4;
            for (n, off) in [
                (1, 0x18u16),
                (2, 0x20),
                (3, 0x22),
                (4, 0x24),
                (5, 0x26),
                (6, 0x28),
                (7, 0x2A),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 8;
        }
        // 0x1F — write 7 anim_block slots with merged descriptor. size 8.
        0x1F => {
            // Merges current low byte of `+0x9E` into op[1].
            let merged = (state.field_9e & 0xFF) | read(1);
            state.field_9e = merged;
            for (n, off) in [
                (2, 0x04u16),
                (3, 0x06),
                (4, 0xFC),
                (5, 0xFE),
                (6, 0x00),
                (7, 0x02),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 8;
        }
        // 0x20 — vtable thunk. size 3.
        0x20 => {
            host.ext_20(state, read(1) as i16, read(2) as i16);
            size = 3;
        }
        // 0x21 — face rotation setup. size 7.
        0x21 => {
            let face_id = read(1) as u8;
            state.face_rotation = face_id;
            let params = [read(2), read(3), read(4), read(5)];
            let target = read(6) as i16 as i32;
            host.face_rotation_setup(face_id, params, target);
            size = 7;
        }
        // 0x22 — epilogue shortcut (size 1, like the default break path).
        0x22 => {
            size = 1;
        }
        // 0x23 — table write. size 0xD.
        0x23 => {
            state.move_submode = 2;
            state.move_substate = 4;
            state.flags &= 0xFFFF_FFFD;
            state.field_9e = read(1) | 0x4000;
            // The original writes a 24-bit packed value at +0xA0 from op[2..4],
            // then 9 u16 writes; we approximate as anim_block writes.
            for (n, off) in [
                (5, 0x18u16),
                (6, 0x1A),
                (7, 0x04),
                (8, 0x06),
                (9, 0xFC),
                (10, 0xFE),
                (11, 0x00),
                (12, 0x02),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 0xD;
        }
        // 0x24 — anim_block additive. size 3.
        0x24 => {
            let v1 = read(1) as i16;
            let v2 = read(2) as i16;
            // Shifts add v1 into +0xA8, +0xAC; v2 into +0xAA, +0xAE.
            for (off, val) in [(0xFC, v1), (0xFE, v2), (0x00, v1), (0x02, v2)] {
                let cur = state.anim_block_u16(off) as i16;
                state.anim_block_u16_set(off, cur.wrapping_add(val) as u16);
            }
            size = 3;
        }
        // 0x25 — child-actor spawn dispatch. size 2.
        0x25 => {
            host.spawn_child(state, read(1) as i16);
            size = 2;
        }
        // 0x26 — write 4 anim_block slots. size 5.
        0x26 => {
            for (n, off) in [(1, 0xFCu16), (2, 0xFE), (3, 0x00), (4, 0x02)] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 5;
        }
        // 0x27 — write 2 anim_block slots. size 3.
        0x27 => {
            state.anim_block_u16_set(0x04, read(1));
            state.anim_block_u16_set(0x06, read(2));
            size = 3;
        }
        // 0x28 — additive scale Z. size 2.
        0x28 => {
            state.tween_scale_z = state.tween_scale_z.wrapping_add(read(1) as i16);
            size = 2;
        }
        // 0x29 — write tween_scale_x = op[1] (no shift). size 2.
        0x29 => {
            state.tween_scale_x = read(1) as i16;
            size = 2;
        }
        // 0x2A — write tween_scale_z = op[1] << 3. size 2.
        0x2A => {
            state.tween_scale_z = (read(1) as i16).wrapping_shl(3);
            size = 2;
        }
        // 0x2B — TWEEN_ABS_TRIPLE. size 4.
        0x2B => {
            state.tween_src_x = read(1) as i16;
            state.tween_src_y = read(2) as i16;
            state.tween_src_z = read(3) as i16;
            size = 4;
        }
        // 0x2C — KEY_BUFFER_ALLOC. size 5.
        0x2C => {
            for (n, slot) in [(1usize, 0usize), (2, 1), (3, 2), (4, 3)] {
                state.keyframe_desc[slot] = read(n);
            }
            let w = state.keyframe_desc[2] as i16;
            let h = state.keyframe_desc[3] as i16;
            let buffer_ptr = if w >= 0x11 {
                let bytes = (w as i32) * (h as i32) * 2;
                let ptr = host.keyframe_alloc(bytes);
                state.field_a8 = ptr;
                ptr
            } else {
                0
            };
            host.keyframe_init(state, buffer_ptr);
            state.field_9c = 1;
            size = 5;
        }
        // 0x2D — WORLD_INC_VARIANT. size 4.
        0x2D => {
            state.tween_src_x = state.tween_src_x.wrapping_add(read(1) as i16);
            state.tween_src_y = state.tween_src_y.wrapping_add(read(2) as i16);
            state.tween_src_z = state.tween_src_z.wrapping_add(read(3) as i16);
            size = 4;
        }
        // 0x2E — TWEEN_SCALE_SET. size 4.
        0x2E => {
            state.tween_scale_x = (read(1) as i16).wrapping_shl(3);
            state.tween_scale_y = (read(2) as i16).wrapping_shl(3);
            state.tween_scale_z = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x2F — overlay extension dispatch.
        0x2F => {
            let sub = read(1);
            // Build an operand window; the extension VM walks `param_2 = op`,
            // i.e. the opcode word itself is at offset 0 and the sub-op at +1.
            let start = state.pc as usize;
            let window = if start < bytecode.len() {
                &bytecode[start..]
            } else {
                &[]
            };
            let result = host.ext_dispatch(state, sub, window);
            size = result.size_u16;
        }
        // 0x30 — KEY_BUFFER_FREE. ends the loop epilogue but advances by 1.
        0x30 => {
            host.keyframe_free(state, state.field_a8);
            state.field_9c = 0;
            // Original goto-jumps to caseD_22 (size 1, then bVar3 stays true).
            size = 1;
        }
        // 0x31 — LFLAG_AND. size 2.
        0x31 => {
            state.local_flags &= read(1);
            size = 2;
        }
        // 0x32 — LFLAG_OR. size 2.
        0x32 => {
            state.local_flags |= read(1);
            size = 2;
        }
        // 0x33 — clear bit 0x40000000 in field_74. size 1.
        0x33 => {
            state.field_74 &= !0x4000_0000u32;
            size = 1;
        }
        // 0x34 — TWEEN_SETUP. size 9.
        0x34 => {
            state.anim_block_u16_set(0x00, read(1)); // +0xAC
            state.anim_block_u16_set(0x04, read(2)); // +0xB0
            state.tween_src_x = read(3) as i16;
            state.tween_src_y = read(4) as i16;
            state.field_9c = read(5) as i16 as i32;
            state.field_a8 = read(6) as i16 as i32;
            state.anim_block_u16_set(0xF8, read(7));
            // Original writes `(int) op[8]` at +0xA8 — we update field_a8 too
            // since the slot overlaps; for the test surface we treat it as
            // the 32-bit value.
            size = 9;
        }
        // 0x35 — WORLD_INC_VARIANT2. size 3.
        0x35 => {
            state.tween_src_x = state.tween_src_x.wrapping_add(read(1) as i16);
            state.tween_src_y = state.tween_src_y.wrapping_add(read(2) as i16);
            size = 3;
        }
        // 0x36 — TWEEN_DURATION_SET. size 3.
        0x36 => {
            state.tween_scale_y = (read(1) as i16).wrapping_shl(3);
            state.tween_scale_z = (read(2) as i16).wrapping_shl(3);
            state.anim_block_u16_set(0x0C, 0); // +0xB8 = 0
            state.anim_block_u16_set(0x0E, 0);
            size = 3;
        }
        // 0x37 — WORLD_SET_VARIANT2. size 3.
        0x37 => {
            state.tween_src_x = read(1) as i16;
            state.tween_src_y = read(2) as i16;
            size = 3;
        }
        // 0x38 — B2_ADD. size 2.
        0x38 => {
            let cur = state.anim_block_u16(0x06) as i16;
            state.anim_block_u16_set(0x06, cur.wrapping_add(read(1) as i16) as u16);
            size = 2;
        }
        // 0x39 — RENDER_BANK_SET (absolute). size 4.
        0x39 => {
            state.render_24 = read(1) as i16;
            state.render_26 = read(2) as i16;
            state.render_28 = read(3) as i16;
            size = 4;
        }
        // 0x3A — flags |= 2. size 1.
        0x3A => {
            state.flags |= 2;
            size = 1;
        }
        // 0x3B — flags &= ~2. size 1.
        0x3B => {
            state.flags &= !2u32;
            size = 1;
        }
        // 0x3C — SCRATCH_WRITE. size 2 (the original branches out via the
        // alloc-on-first-use path; we keep the simple shape and leave the
        // alloc-call to the host via keyframe_alloc).
        0x3C => {
            state.move_submode = 6;
            state.y_rot = 0;
            state.field_68 = 0;
            state.field_5c = 0;
            // NB: full opcode body iterates `count` slots of 6 u16s each;
            // we surface the count and let hosts that need the inner loop
            // read it from anim_block themselves. The safe forward-progress
            // size is 2 + count * 6 worst-case; the runtime does this in a
            // double-loop body. For port correctness we mirror the original
            // PC advance, walking the operand stream.
            let count = read(1) as i16 as i32;
            // The original allocates a buffer if `actor[+0x4C] == 0`; we
            // fold into keyframe_alloc with `bytes = count << 5 | 8`.
            if state.field_a8 == 0 && count > 0 {
                state.field_a8 = host.keyframe_alloc(count.wrapping_shl(5) | 8);
            }
            state.anim_block_u16_set(0x18, state.pc as u16); // mirror +0xCC = pc
            size = 2 + count.max(0) as i16 * 6;
        }
        // 0x3D — anim interpolate. size 3 (variable sub-loops in original).
        0x3D => {
            state.y_rot = 0;
            state.anim_block_u16_set(0x1A, state.pc as u16); // +0xCE = pc
            size = 3 + (read(2) as i16).max(0) * 6;
        }
        // 0x3E — write +0x22. size 2.
        0x3E => {
            state.y_rot = read(1) as i16;
            size = 2;
        }
        // 0x3F — write anim_block +0xD0 slot. size 2.
        0x3F => {
            state.anim_block_u16_set(0x1C, read(1));
            size = 2;
        }
        // 0x40 — call FUN_80058490. size 7.
        0x40 => {
            let block = [read(1), read(2), read(3), read(4)];
            host.func58490(block, read(5) as i16, read(6) as i16);
            size = 7;
        }
        // 0x41 — write anim_block +0xB2 slot. size 2.
        0x41 => {
            state.anim_block_u16_set(0x06, read(1));
            size = 2;
        }
        // 0x42 — anim_block init variant. size 0xF.
        0x42 => {
            state.move_substate = 4;
            state.move_submode = 2;
            state.flags &= 0xFFFF_FFFD;
            state.field_9e = read(1) | 0x2000;
            for (n, off) in [
                (2, 0xF8u16),
                (3, 0xC4),
                (4, 0x08),
                (5, 0x0A),
                (6, 0x0C),
                (7, 0x0E),
                (8, 0xFC),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 0xF;
        }
        // 0x43 — `actor[+0x86] |= 0x2000`. size 1.
        0x43 => {
            state.field_86 |= 0x2000;
            size = 1;
        }
        // 0x44 — triplet write. size 4.
        0x44 => {
            state.field_9e = read(1);
            state.field_68 = read(2) as i16;
            state.field_6a = (read(3) as i16).wrapping_shl(3);
            size = 4;
        }
        // 0x45 — anim_block + sub_mode = 7. size 8.
        0x45 => {
            state.move_submode = 7;
            for (n, off) in [
                (1, 0x14u16),
                (2, 0x18),
                (3, 0x1A),
                (4, 0x20),
                (5, 0x22),
                (6, 0x1C),
                (7, 0x1E),
            ] {
                state.anim_block_u16_set(off as usize, read(n));
            }
            size = 8;
        }
        // 0x46 — TWEEN_INIT. size 4.
        0x46 => {
            state.tween_src_z = state.tween_src_x;
            state.tween_scale_x = state.tween_src_y;
            state.tween_scale_y = (read(1) as i16).wrapping_sub(state.tween_src_x);
            state.tween_scale_z = (read(2) as i16).wrapping_sub(state.tween_src_y);
            // anim_block +0xB8 = 1, +0xC0 = (i32) op[3].
            state.anim_block_u16_set(0x0C, 1);
            state.anim_block_u16_set(0x14, read(3));
            size = 4;
        }
        _ => {
            // Bound check above caught >= 0x47; remaining opcodes in 0x00..=0x46
            // are exhaustively listed. Anything reaching here is a logic bug.
            return StepResult::EndOfBuffer { opcode };
        }
    }

    state.pc = state.pc.wrapping_add(size);
    outcome
}

/// Run the VM in a tick loop until it breaks (`Halt`, `Wait`, `EndOfBuffer`,
/// `Pending`, or budget exhaustion). Mirrors the per-frame entry-point in
/// `FUN_80021DF4 → FUN_80023070`.
///
/// `budget` caps the number of opcodes per frame so a buggy script can't hang
/// the engine. The original has no explicit cap (relies on opcodes naturally
/// breaking), but a cap is the only safe thing for a software port.
pub fn run_until_break<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    bytecode: &[u16],
    budget: usize,
) -> StepResult {
    for _ in 0..budget {
        match step(host, state, bytecode) {
            StepResult::Advance => continue,
            other => return other,
        }
    }
    StepResult::Pending { opcode: 0xFFFF }
}

/// Outcome of one [`actor_tick`] call. Mirrors the move-VM-relevant control
/// flow at `FUN_80021DF4 + 0x800..0x83C`: gate on `wait_timer`, run the VM,
/// inspect the `HALT` flag bit on return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorTickOutcome {
    /// `wait_timer >= 0` — the VM was not entered this frame. The retail
    /// `bgez` at `0x80022B9C` skips the move-VM call. Engines run their
    /// post-tick work normally; the actor is just animating in place.
    Waiting,
    /// VM ran and exited via `0x08` HALT (`actor.flags & 0x8` set). Retail
    /// branches to its halt-handler at `0x80023040` after seeing this bit.
    Halted,
    /// VM ran and exited via `0x09` WAIT_SET — the wait timer has been
    /// re-seeded; further opcodes deferred to next frame.
    WaitSeeded,
    /// VM ran past the bytecode buffer (out-of-range opcode `>= 0x47`).
    /// Retail's bound check (`sltiu v0, v1, 0x47`) terminates the dispatch
    /// loop. Engines normally treat this as "script finished".
    EndOfBuffer { opcode: u16 },
    /// VM exhausted its per-frame opcode budget without breaking. Retail
    /// has no explicit cap; this is a defensive port-only outcome.
    BudgetExhausted,
    /// VM hit an opcode the port hasn't implemented yet. Retail would
    /// dispatch normally; engines decide whether to log + skip or panic.
    Pending { opcode: u16 },
}

/// Per-frame actor advance, ported from the move-VM-relevant slice of
/// `FUN_80021DF4` (lines `80022B94..80022BBC` in the dump).
///
/// Retail behaviour:
///
/// 1. Pre-tick (caller's responsibility — see [`decrement_wait_timer`]):
///    `actor[+0x54] -= delta` (delta is the product of two scratchpad
///    speed scalars).
/// 2. **Move-VM gate**: `if (wait_timer >= 0) skip; else run VM`. The retail
///    bgez at 0x80022B9C is the canonical gate.
/// 3. After the VM call: `if (actor.flags & 0x8) goto halt-handler`.
///
/// Engines compose their per-frame work around this — pre-move integration,
/// post-move animation/render — and gate the move-VM step through this
/// function so the wait-timer + HALT semantics stay faithful to retail.
///
/// This is a thin port: the heavy per-frame work (animation interpolation,
/// position integration, GPU primitive emission) is host-side. The function
/// returns a typed outcome the engine can pattern-match on.
pub fn actor_tick<H: MoveHost + ?Sized>(
    host: &mut H,
    state: &mut ActorState,
    bytecode: &[u16],
    budget: usize,
) -> ActorTickOutcome {
    // Move-VM gate. Retail: `bgez wait_timer, skip-vm`. We branch on the
    // signed value: only step when timer is strictly negative.
    if state.wait_timer >= 0 {
        return ActorTickOutcome::Waiting;
    }

    let result = run_until_break(host, state, bytecode, budget);

    // Post-call flag check. Retail: `andi v0, flags, 0x8; bne v0, zero,
    // halt-handler`. The HALT opcode already sets `flags |= 0x8` inside
    // `step` (see op 0x08). We mirror retail's branch by reading the bit
    // back here so callers don't need to.
    if state.flags & 0x8 != 0 {
        return ActorTickOutcome::Halted;
    }

    match result {
        StepResult::Wait => ActorTickOutcome::WaitSeeded,
        StepResult::EndOfBuffer { opcode } => ActorTickOutcome::EndOfBuffer { opcode },
        StepResult::Pending { opcode: 0xFFFF } => ActorTickOutcome::BudgetExhausted,
        StepResult::Pending { opcode } => ActorTickOutcome::Pending { opcode },
        // Halt was already handled above via the flag check (defensive).
        StepResult::Halt => ActorTickOutcome::Halted,
        // Advance shouldn't escape run_until_break, but match exhaustively.
        StepResult::Advance => ActorTickOutcome::BudgetExhausted,
    }
}

/// Pre-tick wait-timer decrement, ported from the head of `FUN_80021DF4`
/// (line `param_1 + 0x54 -= ...`).
///
/// Retail does:
///
/// ```text
///   actor[+0x54] -= (ushort)DAT_1F800393 * (ushort)DAT_1F80037D;
/// ```
///
/// where the two factors are scratchpad speed scalars (per-actor anim speed
/// × global frame-rate compensation). Engines compute their own `delta` —
/// however they expose those scalars — and pass it here.
///
/// The cast back to `i16` matches retail's `*(ushort *)(param_1 + 0x54)
/// = ...` write-back; the wraparound is intentional and the move-VM gate
/// in [`actor_tick`] interprets the result as `i16`.
pub fn decrement_wait_timer(state: &mut ActorState, delta: u16) {
    // Retail uses unsigned subtraction with `ushort` truncation. The
    // wrapping i16 sub gives the same bytewise result.
    state.wait_timer = state.wait_timer.wrapping_sub(delta as i16);
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestHost {
        rotation_table: std::collections::HashMap<u16, (i16, i16)>,
        stub16_calls: Vec<i16>,
        ext17_calls: Vec<i16>,
        global_writes_1d: Vec<u16>,
        ext20_calls: Vec<(i16, i16)>,
        face_rot_calls: Vec<(u8, [u16; 4], i32)>,
        spawn_calls: Vec<i16>,
        keyframe_allocs: Vec<i32>,
        keyframe_inits: u32,
        keyframe_frees: u32,
        func58490_calls: Vec<([u16; 4], i16, i16)>,
        ext_debug_world_calls: Vec<(i16, i16, i16)>,
        ext_world_struct_writes: Vec<(i16, [i16; 5])>,
        ext_world_struct_inits: Vec<(i16, [i16; 5])>,
        ext_set_flag_bank_calls: Vec<i16>,
        ext_clear_flag_bank_calls: Vec<i16>,
        ext_scratchpad_ramp_calls: Vec<(i16, i16, i16)>,
        ext_scratchpad_write_calls: Vec<(i16, i16)>,
        ext_emit_ot_calls: u32,
        ext_set_8007b9d8_calls: Vec<i32>,
        ext_fade_calls: Vec<([u8; 3], u16)>,
        /// Models the 16-slot 8-byte-stride scratch table at `&DAT_801F3498`.
        slot_table: [[u8; 8]; 16],
        /// Models `_DAT_801F22F4` (move-VM global predicate).
        global_predicate: u32,
        /// Models `_DAT_801F22F6` (move-VM global counter, mod 16).
        global_counter: u16,
        /// Models the player's world coords at `_DAT_8007C364 + 0x14..+0x1A`.
        player_xyz: [i16; 3],
        /// Models the fixed-origin pair at `_DAT_80089118 / _DAT_80089120`.
        fixed_origin_xz: (i32, i32),
        /// Models `_DAT_1F800393` (scratchpad ramp ratio numerator).
        dat_1f800393: u8,
        /// Models `_DAT_8007C348` (axis threshold offset).
        axis_threshold: i16,
        /// Constant value returned by `ext_query_flag_bank` (lets predicate
        /// tests select the expected branch).
        ext_query_flag_bank_returns: u32,
        /// Mirrors the actor's move bytecode buffer for sub-ops 0x04 / 0x1B
        /// / 0x1E. Tests that exercise these ops pre-seed the buffer to
        /// match the program they pass to `step`, then assert on the
        /// post-step contents.
        bytecode_buffer: Vec<u16>,
    }

    impl MoveHost for TestHost {
        fn rotation_lut(&self, index: u16) -> (i16, i16) {
            self.rotation_table.get(&index).copied().unwrap_or((0, 0))
        }
        fn stub_16(&mut self, _state: &mut ActorState, arg: i16) {
            self.stub16_calls.push(arg);
        }
        fn ext_17(&mut self, _state: &mut ActorState, arg: i16) {
            self.ext17_calls.push(arg);
        }
        fn global_write_1d(&mut self, value: u16) {
            self.global_writes_1d.push(value);
        }
        fn ext_20(&mut self, _state: &mut ActorState, a: i16, b: i16) {
            self.ext20_calls.push((a, b));
        }
        fn face_rotation_setup(&mut self, face_id: u8, params: [u16; 4], target: i32) {
            self.face_rot_calls.push((face_id, params, target));
        }
        fn spawn_child(&mut self, _state: &mut ActorState, slot: i16) {
            self.spawn_calls.push(slot);
        }
        fn keyframe_alloc(&mut self, bytes: i32) -> i32 {
            self.keyframe_allocs.push(bytes);
            // Return a non-zero pseudo-pointer so tests can verify it was
            // captured into ActorState.field_a8.
            0xC0DE_0000u32 as i32
        }
        fn keyframe_init(&mut self, _state: &mut ActorState, _buf: i32) {
            self.keyframe_inits += 1;
        }
        fn keyframe_free(&mut self, _state: &mut ActorState, _buf: i32) {
            self.keyframe_frees += 1;
        }
        fn func58490(&mut self, block: [u16; 4], a: i16, b: i16) {
            self.func58490_calls.push((block, a, b));
        }
        fn ext_debug_world(&mut self, x: i16, y: i16, z: i16) {
            self.ext_debug_world_calls.push((x, y, z));
        }
        fn ext_set_flag_bank(&mut self, idx: i16) {
            self.ext_set_flag_bank_calls.push(idx);
        }
        fn ext_clear_flag_bank(&mut self, idx: i16) {
            self.ext_clear_flag_bank_calls.push(idx);
        }
        fn ext_scratchpad_ramp(&mut self, slot: i16, target: i16, ticks: i16) {
            self.ext_scratchpad_ramp_calls.push((slot, target, ticks));
        }
        fn ext_scratchpad_write(&mut self, slot: i16, value: i16) {
            self.ext_scratchpad_write_calls.push((slot, value));
        }
        fn ext_emit_ot_packet(&mut self, _: &[u16]) {
            self.ext_emit_ot_calls += 1;
        }
        fn ext_set_8007b9d8(&mut self, value: i32) {
            self.ext_set_8007b9d8_calls.push(value);
        }
        fn ext_fade_color(&mut self, rgb: [u8; 3], ticks: u16) {
            self.ext_fade_calls.push((rgb, ticks));
        }
        fn ext_world_struct_write(&mut self, idx: i16, vals: [i16; 5]) {
            self.ext_world_struct_writes.push((idx, vals));
        }
        fn ext_world_struct_init(&mut self, idx: i16, vals: [i16; 5]) {
            self.ext_world_struct_inits.push((idx, vals));
        }
        fn move_slot_save_u32(&mut self, slot: u16, dword_off: u8, value: u32) {
            let slot_idx = (slot as usize) & 0xF;
            let off = dword_off as usize;
            self.slot_table[slot_idx][off..off + 4].copy_from_slice(&value.to_le_bytes());
        }
        fn move_slot_load_u32(&self, slot: u16, dword_off: u8) -> u32 {
            let slot_idx = (slot as usize) & 0xF;
            let off = dword_off as usize;
            u32::from_le_bytes(self.slot_table[slot_idx][off..off + 4].try_into().unwrap())
        }
        fn move_slot_save_u16(&mut self, slot: u16, byte_off: u8, value: u16) {
            let slot_idx = (slot as usize) & 0xF;
            let off = byte_off as usize;
            self.slot_table[slot_idx][off..off + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn move_slot_load_u16(&self, slot: u16, byte_off: u8) -> u16 {
            let slot_idx = (slot as usize) & 0xF;
            let off = byte_off as usize;
            u16::from_le_bytes(self.slot_table[slot_idx][off..off + 2].try_into().unwrap())
        }
        fn move_global_predicate_get(&self) -> u32 {
            self.global_predicate
        }
        fn move_global_predicate_set(&mut self, value: u32) {
            self.global_predicate = value;
        }
        fn move_global_counter_get(&self) -> u16 {
            self.global_counter
        }
        fn move_global_counter_set(&mut self, value: u16) {
            self.global_counter = value;
        }
        fn move_player_world_xyz(&self) -> [i16; 3] {
            self.player_xyz
        }
        fn move_fixed_origin_xz(&self) -> (i32, i32) {
            self.fixed_origin_xz
        }
        fn move_dat_1f800393(&self) -> u8 {
            self.dat_1f800393
        }
        fn move_axis_threshold(&self) -> i16 {
            self.axis_threshold
        }
        fn ext_query_flag_bank(&self, _idx: i16) -> u32 {
            self.ext_query_flag_bank_returns
        }
        fn move_bytecode_read_u16(&self, word_off: usize) -> u16 {
            self.bytecode_buffer.get(word_off).copied().unwrap_or(0)
        }
        fn move_bytecode_write_u16(&mut self, word_off: usize, value: u16) {
            if word_off >= self.bytecode_buffer.len() {
                self.bytecode_buffer.resize(word_off + 1, 0);
            }
            self.bytecode_buffer[word_off] = value;
        }
    }

    fn program(words: &[u16]) -> Vec<u16> {
        // Append a guard word so the VM never reads past end during the
        // tested handler; the test asserts PC after one step.
        let mut v = words.to_vec();
        v.extend_from_slice(&[
            0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF,
        ]);
        v
    }

    #[test]
    fn opcode_decode_round_trip_in_bound() {
        for v in 0u16..=0x46 {
            let op = MoveOpcode::from_u16(v).expect("must decode");
            assert_eq!(op as u16, v);
        }
        assert!(MoveOpcode::from_u16(0x47).is_none());
        assert!(MoveOpcode::from_u16(0xFFFF).is_none());
    }

    #[test]
    fn op00_anim_bank_set_writes_shifted_triple_and_advances_pc_by_4() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x00, 1, 2, 3]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(state.anim_3c, 1 << 3);
        assert_eq!(state.anim_3e, 2 << 3);
        assert_eq!(state.anim_40, 3 << 3);
        assert_eq!(state.pc, 4);
    }

    #[test]
    fn op01_world_add_advances_y_mirror_too() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.world_x = 100;
        state.world_y = 50;
        state.world_y_mirror = 50;
        state.world_z = 25;
        let bc = program(&[0x01, 5, 10, 15]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(state.world_x, 105);
        assert_eq!(state.world_y, 60);
        assert_eq!(state.world_y_mirror, 60);
        assert_eq!(state.world_z, 40);
        assert_eq!(state.pc, 4);
    }

    #[test]
    fn op07_world_set_overrides_y_mirror() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.world_y = 999;
        state.world_y_mirror = 999;
        let bc = program(&[0x07, 11, 22, 33]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(state.world_x, 11);
        assert_eq!(state.world_y, 22);
        assert_eq!(state.world_y_mirror, 22);
        assert_eq!(state.world_z, 33);
    }

    #[test]
    fn op08_halt_sets_flag_8_and_breaks_loop() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x08]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Halt);
        assert_eq!(state.flags & 0x8, 0x8);
        // Halt does not advance PC (size=0).
        assert_eq!(state.pc, 0);
    }

    #[test]
    fn op09_wait_set_seeds_timer_and_yields() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x09, 5]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Wait);
        assert_eq!(state.wait_timer, 5 << 3);
        assert_eq!(state.pc, 2);
    }

    #[test]
    fn op16_stub_calls_host_and_advances() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x16, 0x42]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(host.stub16_calls, vec![0x42]);
        assert_eq!(state.pc, 2);
    }

    #[test]
    fn op21_face_rotation_writes_index_and_calls_host() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x21, 7, 100, 200, 300, 400, 0x8000]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(state.face_rotation, 7);
        assert_eq!(host.face_rot_calls.len(), 1);
        let (face_id, params, target) = &host.face_rot_calls[0];
        assert_eq!(*face_id, 7);
        assert_eq!(*params, [100, 200, 300, 400]);
        // 0x8000 sign-extended to i32 is -32768.
        assert_eq!(*target, -32768);
        assert_eq!(state.pc, 7);
    }

    #[test]
    fn op2c_alloc_path_calls_host_when_w_ge_0x11() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // [op, count_x, count_y, w, h] — w=0x11, h=4, bytes = 0x11*4*2 = 136.
        let bc = program(&[0x2C, 1, 2, 0x11, 4]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(host.keyframe_allocs, vec![136]);
        assert_eq!(host.keyframe_inits, 1);
        assert_eq!(state.field_a8, 0xC0DE_0000u32 as i32);
        assert_eq!(state.field_9c, 1);
        assert_eq!(state.pc, 5);
    }

    #[test]
    fn op2c_inline_path_skips_alloc_when_w_lt_0x11() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x2C, 1, 2, 4, 4]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert!(host.keyframe_allocs.is_empty());
        assert_eq!(host.keyframe_inits, 1);
        assert_eq!(state.field_9c, 1);
    }

    #[test]
    fn op30_key_buffer_free_clears_field_9c() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.field_9c = 1;
        state.field_a8 = 0xDEAD_BEEFu32 as i32;
        let bc = program(&[0x30]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(host.keyframe_frees, 1);
        assert_eq!(state.field_9c, 0);
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op31_lflag_and_with_op32_lflag_or_round_trip() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.local_flags = 0xFFFF;
        // AND with 0x000F → low nibble only.
        let bc = program(&[0x31, 0x000F]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.local_flags, 0x000F);
        // OR with 0xFF00 → restore high byte.
        state.pc = 0;
        let bc2 = program(&[0x32, 0xFF00]);
        step(&mut host, &mut state, &bc2);
        assert_eq!(state.local_flags, 0xFF0F);
    }

    #[test]
    fn op33_clear_bit_40000000_only_touches_that_bit() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.field_74 = 0xFFFF_FFFF;
        let bc = program(&[0x33]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.field_74, 0xBFFF_FFFF);
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op3a_and_op3b_toggle_flag_2() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x3A]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.flags & 2, 2);
        state.pc = 0;
        let bc2 = program(&[0x3B]);
        step(&mut host, &mut state, &bc2);
        assert_eq!(state.flags & 2, 0);
    }

    #[test]
    fn op2f_dispatches_extension_vm() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // op[0] = 0x2F, op[1] = sub 0x01 (debug print).
        state.world_x = 1;
        state.world_y = 2;
        state.world_z = 3;
        let bc = program(&[0x2F, 0x01]);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(r, StepResult::Advance);
        assert_eq!(host.ext_debug_world_calls, vec![(1, 2, 3)]);
        // Sub-0x01 returns size 2.
        assert_eq!(state.pc, 2);
    }

    #[test]
    fn op2f_subop_02_clears_face_rotation() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.face_rotation = 7;
        let bc = program(&[0x2F, 0x02]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.face_rotation, 0);
        // default-arm size is 1.
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op2f_subop_29_ramp_path_calls_host_with_negated_target() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // [0x2F, 0x29, slot=3, target=10, ticks=5]
        let bc = program(&[0x2F, 0x29, 3, 10, 5]);
        step(&mut host, &mut state, &bc);
        assert_eq!(host.ext_scratchpad_ramp_calls, vec![(3, -10, 5)]);
        assert!(host.ext_scratchpad_write_calls.is_empty());
    }

    #[test]
    fn op2f_subop_29_immediate_path_when_ticks_zero() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x29, 3, 10, 0]);
        step(&mut host, &mut state, &bc);
        assert!(host.ext_scratchpad_ramp_calls.is_empty());
        assert_eq!(host.ext_scratchpad_write_calls, vec![(3, -10)]);
    }

    #[test]
    fn op2f_subop_3c_immediate_fade_color() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x3C, 0xFF, 0x80, 0x40, 0]);
        step(&mut host, &mut state, &bc);
        assert_eq!(host.ext_fade_calls, vec![(([0xFF, 0x80, 0x40]), 0)]);
        assert_eq!(state.pc, 6);
    }

    #[test]
    fn out_of_range_opcode_is_end_of_buffer() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x47]);
        let r = step(&mut host, &mut state, &bc);
        assert!(matches!(r, StepResult::EndOfBuffer { opcode: 0x47 }));
    }

    #[test]
    fn run_until_break_steps_through_until_halt() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // Program: WORLD_SET (4), WRITE_26 (2), HALT (0). Total 3 ticks.
        let bc = program(&[
            0x07, 1, 2, 3, // WORLD_SET
            0x06, 99,   // WRITE_26
            0x08, // HALT
        ]);
        let r = run_until_break(&mut host, &mut state, &bc, 16);
        assert_eq!(r, StepResult::Halt);
        assert_eq!(state.world_x, 1);
        assert_eq!(state.render_26, 99);
        assert_eq!(state.flags & 0x8, 0x8);
        assert_eq!(state.pc, 6); // 4 + 2 + 0 (halt doesn't advance).
    }

    #[test]
    fn op03_world_rotate_add_uses_host_lut() {
        let mut host = TestHost::default();
        // Pretend rotation index 0 has sin = 0x1000, cos = 0x0800.
        host.rotation_table.insert(0, (0x1000, 0x0800));
        let mut state = ActorState::new();
        state.tween_scale_x = 0; // index = 0
        let bc = program(&[0x03, 100]);
        step(&mut host, &mut state, &bc);
        // dx = (0x1000 * 100) >> 12 = 100.
        assert_eq!(state.world_x, 100);
        // dz = (0x0800 * 100) >> 12 = 50.
        assert_eq!(state.world_z, 50);
        assert_eq!(state.pc, 2);
    }

    #[test]
    fn op2f_subop_1c_sets_flag_bank_via_host() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x1C, 42]);
        step(&mut host, &mut state, &bc);
        assert_eq!(host.ext_set_flag_bank_calls, vec![42]);
        // Sub-0x1C returns size 3.
        assert_eq!(state.pc, 3);
    }

    #[test]
    fn op2f_subop_15_sets_flag_bit_800000() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x15]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.flags & 0x800000, 0x800000);
    }

    // ---- 0x2B / 0x2D / 0x33 anim-block writes ----

    #[test]
    fn op2f_subop_2b_writes_anim_block_b4_through_ba() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // [0x2F, 0x2B, w0, w1, w2, w3] — writes to +0xB4/B6/B8/BA.
        let bc = program(&[0x2F, 0x2B, 0x1111, 0x2222, 0x3333, 0x4444]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.anim_block_u16(8), 0x1111);
        assert_eq!(state.anim_block_u16(10), 0x2222);
        assert_eq!(state.anim_block_u16(12), 0x3333);
        assert_eq!(state.anim_block_u16(14), 0x4444);
    }

    #[test]
    fn op2f_subop_2d_adds_to_anim_block_b4_through_ba() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.anim_block_u16_set(8, 100);
        state.anim_block_u16_set(10, 200);
        state.anim_block_u16_set(12, 300);
        state.anim_block_u16_set(14, 400);
        let bc = program(&[0x2F, 0x2D, 5, 6, 7, 8]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.anim_block_u16(8), 105);
        assert_eq!(state.anim_block_u16(10), 206);
        assert_eq!(state.anim_block_u16(12), 307);
        assert_eq!(state.anim_block_u16(14), 408);
    }

    #[test]
    fn op2f_subop_2d_wrapping_add_when_overflowed() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.anim_block_u16_set(8, 0xFFFF);
        let bc = program(&[0x2F, 0x2D, 1, 0, 0, 0]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.anim_block_u16(8), 0x0000);
    }

    #[test]
    fn op2f_subop_33_adds_to_anim_block_c0_through_c6() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.anim_block_u16_set(20, 1000);
        state.anim_block_u16_set(22, 2000);
        state.anim_block_u16_set(24, 3000);
        state.anim_block_u16_set(26, 4000);
        let bc = program(&[0x2F, 0x33, 11, 22, 33, 44]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.anim_block_u16(20), 1011);
        assert_eq!(state.anim_block_u16(22), 2022);
        assert_eq!(state.anim_block_u16(24), 3033);
        assert_eq!(state.anim_block_u16(26), 4044);
    }

    #[test]
    fn op2f_subop_0c_writes_field_50() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x0C, 0xABCD]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.field_50, 0xABCD);
    }

    #[test]
    fn op2f_subop_0d_adds_to_field_50_with_wrap() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.field_50 = 0xFFFE;
        let bc = program(&[0x2F, 0x0D, 5]);
        step(&mut host, &mut state, &bc);
        // 0xFFFE + 5 wraps to 0x0003.
        assert_eq!(state.field_50, 0x0003);
    }

    #[test]
    fn op2f_subop_25_then_26_round_trips_world_coords() {
        // Save world coords to slot 3, perturb the actor, then load back.
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.world_x = 100;
        state.world_y = 200;
        state.world_z = 300;
        state.world_y_mirror = 400;
        let save = program(&[0x2F, 0x25, 3]);
        step(&mut host, &mut state, &save);
        // Perturb.
        state.world_x = -1;
        state.world_y = -1;
        state.world_z = -1;
        state.world_y_mirror = -1;
        // Reset PC for the second program.
        state.pc = 0;
        let load = program(&[0x2F, 0x26, 3]);
        step(&mut host, &mut state, &load);
        assert_eq!(state.world_x, 100);
        assert_eq!(state.world_y, 200);
        assert_eq!(state.world_z, 300);
        assert_eq!(state.world_y_mirror, 400);
    }

    #[test]
    fn op2f_subop_27_saves_tween_src_triple_into_first_six_bytes_of_slot() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.tween_src_x = 0x1111;
        state.tween_src_y = 0x2222;
        state.tween_src_z = 0x3333;
        let bc = program(&[0x2F, 0x27, 5]);
        step(&mut host, &mut state, &bc);
        // slot[5] bytes 0..2/2..4/4..6 should match the three i16 values.
        assert_eq!(host.move_slot_load_u16(5, 0), 0x1111);
        assert_eq!(host.move_slot_load_u16(5, 2), 0x2222);
        assert_eq!(host.move_slot_load_u16(5, 4), 0x3333);
    }

    #[test]
    fn op2f_subop_28_loads_scales_and_clamps() {
        // Pre-load slot 7 with three known u16 values, then run sub-op 0x28
        // with scale operands chosen so that the y-axis result clamps to
        // -0xFF and the z-axis result clamps to +0xFF.
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        host.move_slot_save_u16(7, 0, 1000); // becomes tween_src_x as-is
        host.move_slot_save_u16(7, 2, -2000i16 as u16); // raw_y = -2000
        host.move_slot_save_u16(7, 4, 2000); // raw_z = 2000
        // op_w(2)=slot=7, op_w(3)=scale_y, op_w(4)=scale_z. Use 0x4000
        // (=2.0 in 12.4 fixed) so raw * scale >> 12 hits the clamp band.
        let bc = program(&[0x2F, 0x28, 7, 0x4000, 0x4000]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.tween_src_x, 1000);
        // -2000 * 0x4000 >> 12 = -8000 → clamps to -0xFF.
        assert_eq!(state.tween_src_y, -0xFF);
        // 2000 * 0x4000 >> 12 = 8000 → clamps to +0xFF.
        assert_eq!(state.tween_src_z, 0xFF);
    }

    #[test]
    fn op2f_subop_31_then_32_round_trips_render_banks() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.render_24 = -100;
        state.render_26 = -200;
        state.render_28 = -300;
        state.world_y_mirror = -400;
        step(&mut host, &mut state, &program(&[0x2F, 0x31, 9]));
        state.render_24 = 0;
        state.render_26 = 0;
        state.render_28 = 0;
        state.world_y_mirror = 0;
        state.pc = 0;
        step(&mut host, &mut state, &program(&[0x2F, 0x32, 9]));
        assert_eq!(state.render_24, -100);
        assert_eq!(state.render_26, -200);
        assert_eq!(state.render_28, -300);
        assert_eq!(state.world_y_mirror, -400);
    }

    #[test]
    fn op2f_subop_34_then_35_round_trips_field_72() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.field_72 = 0xCAFE;
        step(&mut host, &mut state, &program(&[0x2F, 0x34, 12]));
        state.field_72 = 0;
        state.pc = 0;
        step(&mut host, &mut state, &program(&[0x2F, 0x35, 12]));
        assert_eq!(state.field_72, 0xCAFE);
    }

    #[test]
    fn op2f_subop_08_sets_global_predicate_and_subop_09_clears_it() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        step(&mut host, &mut state, &program(&[0x2F, 0x08]));
        assert_eq!(host.global_predicate, 1);
        state.pc = 0;
        step(&mut host, &mut state, &program(&[0x2F, 0x09]));
        assert_eq!(host.global_predicate, 0);
    }

    #[test]
    fn op2f_subop_0a_falls_through_when_predicate_set() {
        let mut host = TestHost::default();
        host.global_predicate = 1;
        let mut state = ActorState::new();
        // The convention in this VM: dispatcher's `default_arm()` returns
        // `size_u16 = 1` so the main step advances PC by 1 (matching the
        // PSX dispatcher's `iVar16 = 1; default: iVar16 << 0x10; return
        // iVar16 >> 0x10` shape).
        let bc = program(&[0x2F, 0x0A]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op2f_subop_0a_skips_when_predicate_clear() {
        let mut host = TestHost::default();
        host.global_predicate = 0;
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x0A]);
        step(&mut host, &mut state, &bc);
        // Skip path = `with_size(3)` → PC += 3.
        assert_eq!(state.pc, 3);
    }

    #[test]
    fn op2f_subop_0b_skips_when_predicate_set() {
        let mut host = TestHost::default();
        host.global_predicate = 1;
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x0B]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 3);
    }

    #[test]
    fn op2f_subop_0b_falls_through_when_predicate_clear() {
        let mut host = TestHost::default();
        host.global_predicate = 0;
        let mut state = ActorState::new();
        step(&mut host, &mut state, &program(&[0x2F, 0x0B]));
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op2f_subop_0f_clears_global_counter() {
        let mut host = TestHost::default();
        host.global_counter = 7;
        let mut state = ActorState::new();
        step(&mut host, &mut state, &program(&[0x2F, 0x0F]));
        assert_eq!(host.global_counter, 0);
    }

    #[test]
    fn op2f_subop_10_cycles_counter_and_writes_low_byte_to_field_86() {
        let mut host = TestHost::default();
        host.global_counter = 5;
        let mut state = ActorState::new();
        // Pre-set the high byte of field_86 to verify it's preserved.
        state.field_86 = 0xAA00;
        step(&mut host, &mut state, &program(&[0x2F, 0x10]));
        // Captured value (5) goes to low byte of field_86; counter increments.
        assert_eq!(state.field_86, 0xAA05);
        assert_eq!(host.global_counter, 6);
    }

    #[test]
    fn op2f_subop_10_wraps_counter_at_16() {
        let mut host = TestHost::default();
        host.global_counter = 16; // > 15 → wraps to 0 first
        let mut state = ActorState::new();
        step(&mut host, &mut state, &program(&[0x2F, 0x10]));
        // Counter wrapped to 0, captured 0, then incremented to 1.
        assert_eq!(state.field_86 & 0xFF, 0);
        assert_eq!(host.global_counter, 1);
    }

    #[test]
    fn op2f_subop_2a_lerps_world_toward_player_position() {
        // op_w(2..4) = base x/y/z; op_w(5..7) = per-axis t (`>> 12` shift).
        // With t=0x1000 (= 1.0 in 12.4 fixed) the result lands exactly on
        // the player position.
        let mut host = TestHost::default();
        host.player_xyz = [1000, 2000, 3000];
        let mut state = ActorState::new();
        let bc = program(&[
            0x2F, 0x2A, // sub-op
            500, 800, 1500, // base
            0x1000, 0x1000, 0x1000, // t = 1.0
        ]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.world_x, 1000);
        assert_eq!(state.world_y, 2000);
        assert_eq!(state.world_z, 3000);
    }

    #[test]
    fn op2f_subop_2a_at_t_zero_keeps_base() {
        let mut host = TestHost::default();
        host.player_xyz = [9999, 9999, 9999];
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x2A, 500, 800, 1500, 0, 0, 0]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.world_x, 500);
        assert_eq!(state.world_y, 800);
        assert_eq!(state.world_z, 1500);
    }

    #[test]
    fn op2f_subop_24_uses_fixed_origin_for_x_and_z() {
        // Sub-0x24 X: target = -(base + origin); Y still toward player.
        let mut host = TestHost::default();
        host.fixed_origin_xz = (200, 300);
        host.player_xyz = [0, 5000, 0]; // Y target
        let mut state = ActorState::new();
        // base = (100, 1000, 50), t = (0x1000, 0x1000, 0x1000)
        let bc = program(&[0x2F, 0x24, 100, 1000, 50, 0x1000, 0x1000, 0x1000]);
        step(&mut host, &mut state, &bc);
        // X target = -(100 + 200) = -300. Lerp at t=1 → -300.
        assert_eq!(state.world_x, -300);
        // Y target = player.world_y (5000). Lerp at t=1 → 5000.
        assert_eq!(state.world_y, 5000);
        // Z target = -(50 + 300) = -350.
        assert_eq!(state.world_z, -350);
    }

    #[test]
    fn op2f_subop_11_saves_world_to_slot_indexed_by_field_86_low_byte() {
        // The pair 0x10 + 0x11 round-trips: cycle counter writes low byte
        // of field_86, then 0x11 saves world to that slot. Verify with a
        // pre-set field_86 value to keep the test free of cycle-counter
        // sequencing.
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.field_86 = 0xAA0B; // low byte = 11 → slot index 11 (& 0xF = 11)
        state.world_x = -50;
        state.world_y = -100;
        state.world_z = -150;
        state.world_y_mirror = -200;
        step(&mut host, &mut state, &program(&[0x2F, 0x11]));
        assert_eq!(host.move_slot_load_u16(11, 0) as i16, -50);
        assert_eq!(host.move_slot_load_u16(11, 2) as i16, -100);
        assert_eq!(host.move_slot_load_u16(11, 4) as i16, -150);
        assert_eq!(host.move_slot_load_u16(11, 6) as i16, -200);
    }

    #[test]
    fn op2f_subop_10_then_subop_11_produces_a_running_capture() {
        // Cycle the counter twice (each captures the pre-increment value
        // into field_86 low byte) and verify the slot writes hit the right
        // indices. Counter starts at 0:
        //   step 0x10: captures 0 → field_86 lo = 0; counter becomes 1.
        //   step 0x11: saves world to slot 0.
        //   step 0x10: captures 1 → field_86 lo = 1; counter becomes 2.
        //   step 0x11: saves world to slot 1.
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.world_x = 100;
        for expected_slot in 0..3 {
            state.world_x = 100 + expected_slot as i16;
            state.pc = 0;
            step(&mut host, &mut state, &program(&[0x2F, 0x10]));
            state.pc = 0;
            step(&mut host, &mut state, &program(&[0x2F, 0x11]));
            assert_eq!(
                host.move_slot_load_u16(expected_slot as u16, 0) as i16,
                100 + expected_slot as i16
            );
        }
        assert_eq!(host.global_counter, 3);
    }

    #[test]
    fn op2f_subop_06_skips_when_player_outside_box() {
        // Box corners (xa=10, za=20, xb=20, zb=30) scaled by 0x80 + 0x40 =
        // x in [1344, 2624], z in [2624, 3904]. Player at (0, 0, 0) is
        // outside → 0x06 takes the size-7 skip.
        let mut host = TestHost::default();
        host.player_xyz = [0, 0, 0];
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x06, 10, 20, 20, 30]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 7);
    }

    #[test]
    fn op2f_subop_06_continues_when_player_inside_box() {
        let mut host = TestHost::default();
        host.player_xyz = [2000, 0, 3000]; // inside the [1344..2624] × [2624..3904] band
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x06, 10, 20, 20, 30]);
        step(&mut host, &mut state, &bc);
        // default-arm = size_u16 = 1 → PC += 1.
        assert_eq!(state.pc, 1);
    }

    // ---- actor_tick / decrement_wait_timer wiring ----

    #[test]
    fn decrement_wait_timer_subtracts_delta() {
        let mut state = ActorState::new();
        state.wait_timer = 10;
        decrement_wait_timer(&mut state, 4);
        assert_eq!(state.wait_timer, 6);
    }

    #[test]
    fn decrement_wait_timer_wraps_to_negative() {
        let mut state = ActorState::new();
        state.wait_timer = 1;
        decrement_wait_timer(&mut state, 3);
        // 1 - 3 = -2 (wrapping i16).
        assert_eq!(state.wait_timer, -2);
    }

    #[test]
    fn actor_tick_skips_vm_when_timer_nonneg() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.wait_timer = 0;

        let bc = program(&[0x06, 99, 0x08]); // WRITE_26, HALT
        let r = actor_tick(&mut host, &mut state, &bc, 16);
        assert_eq!(r, ActorTickOutcome::Waiting);
        // VM was not entered — render_26 unchanged, no HALT flag.
        assert_eq!(state.render_26, 0);
        assert_eq!(state.flags & 0x8, 0);
        assert_eq!(state.pc, 0);
    }

    #[test]
    fn actor_tick_runs_vm_when_timer_negative() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.wait_timer = -1;

        let bc = program(&[0x06, 99, 0x08]); // WRITE_26, HALT
        let r = actor_tick(&mut host, &mut state, &bc, 16);
        assert_eq!(r, ActorTickOutcome::Halted);
        assert_eq!(state.render_26, 99);
        assert_eq!(state.flags & 0x8, 0x8);
    }

    #[test]
    fn actor_tick_reports_wait_seeded() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.wait_timer = -1;

        // op 0x09 WAIT_SET sets wait_timer = arg << 3 and breaks.
        let bc = program(&[0x09, 5]);
        let r = actor_tick(&mut host, &mut state, &bc, 16);
        assert_eq!(r, ActorTickOutcome::WaitSeeded);
        assert_eq!(state.wait_timer, 40); // 5 << 3
        // No HALT flag.
        assert_eq!(state.flags & 0x8, 0);
    }

    #[test]
    fn actor_tick_reports_end_of_buffer_on_oor_opcode() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.wait_timer = -1;

        let bc = program(&[0x47]); // out of range
        let r = actor_tick(&mut host, &mut state, &bc, 16);
        assert!(matches!(r, ActorTickOutcome::EndOfBuffer { opcode: 0x47 }));
    }

    #[test]
    fn actor_tick_reports_budget_exhausted() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.wait_timer = -1;

        // Infinite loop: SAVE_LOOP18 + LOOP_JUMP19 (no break opcode).
        // Easier: just use a long sequence of WRITE_26 ops with no HALT.
        let mut words = vec![];
        for _ in 0..32 {
            words.push(0x06);
            words.push(0x00);
        }
        let bc = program(&words);
        let r = actor_tick(&mut host, &mut state, &bc, 4);
        assert_eq!(r, ActorTickOutcome::BudgetExhausted);
    }

    /// Composed bytecode walks several explicit-size opcodes — WORLD_SET,
    /// FACE_ROTATION, ext sub 0x1C (set-flag-bank, size 3), then HALT.
    /// `run_until_break` should exit at HALT with all per-op state changes
    /// applied. Avoids the default-arm sub-ops whose `size_u16 = 1`
    /// semantics leave PC pointing at the sub-op byte (which would then be
    /// re-interpreted as a new opcode). Mirrors session-20's integration
    /// style for the field VM but exercises the move-VM dispatch table.
    #[test]
    fn run_until_break_walks_explicit_size_opcodes_then_halts() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();

        // Bytecode:
        // op 0x07 1 2 3                       — WORLD_SET (size 4)
        // op 0x21 7 100 200 300 400 0x8000   — face rotation (size 7)
        // op 0x2F sub 0x1C 42                 — ext set_flag_bank (size 3)
        // op 0x08                              — HALT (size 0)
        let bc = program(&[
            0x07, 1, 2, 3, // WORLD_SET
            0x21, 7, 100, 200, 300, 400, 0x8000, // FACE_ROT
            0x2F, 0x1C, 42,   // ext set_flag_bank(42), size 3
            0x08, // HALT
        ]);

        let r = run_until_break(&mut host, &mut state, &bc, 64);
        assert_eq!(r, StepResult::Halt);

        // World coords from WORLD_SET.
        assert_eq!(state.world_x, 1);
        assert_eq!(state.world_y, 2);
        assert_eq!(state.world_z, 3);
        // Face rotation index recorded.
        assert_eq!(state.face_rotation, 7);
        assert_eq!(host.face_rot_calls.len(), 1);
        // Ext set_flag_bank captured the index.
        assert_eq!(host.ext_set_flag_bank_calls, vec![42]);
        // HALT bit set.
        assert_eq!(state.flags & 0x8, 0x8);
        // PC stops at the HALT word (size 0). 4 + 7 + 3 = 14.
        assert_eq!(state.pc, 14);
    }

    /// `actor_tick` composed with `decrement_wait_timer` for a multi-frame
    /// scenario where the script seeds a new wait inside the VM and the
    /// next frame must decrement that wait before re-entering. Mirrors a
    /// retail per-frame loop where one script `WAIT_SET` keeps the actor
    /// idle for a known number of frames.
    #[test]
    fn actor_tick_wait_set_then_decrements_to_resume() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.wait_timer = -1; // VM eligible

        // WAIT_SET 4 (sets timer = 32 = 4<<3) then HALT.
        let bc = program(&[0x09, 4, 0x08]);

        // Frame 1: VM runs, hits WAIT_SET, breaks with WaitSeeded.
        let r1 = actor_tick(&mut host, &mut state, &bc, 16);
        assert_eq!(r1, ActorTickOutcome::WaitSeeded);
        assert_eq!(state.wait_timer, 32);

        // Frames 2..N: pre-tick decrements; until timer is back negative,
        // VM is gated. Use delta=8 so it takes 5 frames (32 → 24 → 16 → 8 → 0 → -8).
        for expected in [24, 16, 8, 0] {
            decrement_wait_timer(&mut state, 8);
            assert_eq!(state.wait_timer, expected);
            assert_eq!(
                actor_tick(&mut host, &mut state, &bc, 16),
                ActorTickOutcome::Waiting
            );
        }

        // One more pre-tick → timer = -8 (negative). VM runs, but we're
        // sitting at the HALT instruction now (PC was advanced past the
        // 2-word WAIT_SET on the seed step).
        decrement_wait_timer(&mut state, 8);
        assert_eq!(state.wait_timer, -8);
        assert_eq!(
            actor_tick(&mut host, &mut state, &bc, 16),
            ActorTickOutcome::Halted
        );
    }

    #[test]
    fn actor_tick_pretick_then_tick_models_retail_frame() {
        // Compose decrement_wait_timer + actor_tick to model a full frame.
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.wait_timer = 8; // initially "still waiting"

        let bc = program(&[0x06, 42, 0x08]);

        // Frame 1: pre-tick takes timer 8 → 5 (delta = 3). Still nonneg, skip.
        decrement_wait_timer(&mut state, 3);
        assert_eq!(
            actor_tick(&mut host, &mut state, &bc, 16),
            ActorTickOutcome::Waiting
        );

        // Frame 2: 5 → 2 (still nonneg).
        decrement_wait_timer(&mut state, 3);
        assert_eq!(
            actor_tick(&mut host, &mut state, &bc, 16),
            ActorTickOutcome::Waiting
        );

        // Frame 3: 2 → -1 (now negative). VM runs, hits HALT.
        decrement_wait_timer(&mut state, 3);
        assert_eq!(
            actor_tick(&mut host, &mut state, &bc, 16),
            ActorTickOutcome::Halted
        );
        assert_eq!(state.render_26, 42);
    }

    #[test]
    fn op2f_subop_0e_advances_pc_by_eleven_and_writes_world_average() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // a = (10, 20, 30), off = (1, 2, 3), b = (40, 60, 90)
        // mid = ((10+40)/2 + 1, (20+60)/2 + 2, (30+90)/2 + 3) = (26, 42, 63)
        let bc = program(&[0x2F, 0x0E, 10, 20, 30, 1, 2, 3, 40, 60, 90]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.world_x, 26);
        assert_eq!(state.world_y, 42);
        assert_eq!(state.world_z, 63);
        assert_eq!(
            state.pc, 11,
            "0x0E must advance past the entire 11-word instruction"
        );
    }

    #[test]
    fn op2f_subop_12_uses_slot_indexed_by_field_86_low_byte() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.field_86 = 0x4205; // slot index = 5
        // Pre-populate slot 5 with a = (50, 60, 70).
        host.move_slot_save_u32(5, 0, (50u16 as u32) | ((60u16 as u32) << 16));
        host.move_slot_save_u32(5, 4, (70u16 as u32) | ((0u16 as u32) << 16));
        // off = (1, 2, 3), b = (50, 60, 70)
        // mid_x = ((50 + 50)/2) + 1 = 51
        // mid_y = ((60 + 60)/2) + 2 = 62
        // mid_z = ((70 + 70)/2) + 3 = 73
        let bc = program(&[0x2F, 0x12, 1, 2, 3, 50, 60, 70]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.world_x, 51);
        assert_eq!(state.world_y, 62);
        assert_eq!(state.world_z, 73);
        assert_eq!(state.pc, 8, "0x12 must advance past the 8-word instruction");
    }

    #[test]
    fn op2f_subop_13_falls_through_when_flag_set() {
        let mut host = TestHost::default();
        host.ext_query_flag_bank_returns = 1;
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x13, 7]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 1, "predicate-true → default-arm size 1");
    }

    #[test]
    fn op2f_subop_13_skips_when_flag_clear() {
        let mut host = TestHost::default();
        host.ext_query_flag_bank_returns = 0;
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x13, 7]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 4, "predicate-false → skip past 3-u16 follow-up");
    }

    #[test]
    fn op2f_subop_14_inverts_predicate() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // 0x14 with predicate set → SKIP (size 4).
        host.ext_query_flag_bank_returns = 1;
        let bc = program(&[0x2F, 0x14, 7]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 4);
        // 0x14 with predicate clear → fall through (size 1).
        let mut host2 = TestHost::default();
        host2.ext_query_flag_bank_returns = 0;
        let mut state2 = ActorState::new();
        step(&mut host2, &mut state2, &bc);
        assert_eq!(state2.pc, 1);
    }

    #[test]
    fn op2f_subop_36_axis_threshold_below() {
        // 0x36 predicate: op[2] < (0x8E - axis). axis=0, op[2]=0x40 → 0x40 < 0x8E true.
        let mut host = TestHost::default();
        host.axis_threshold = 0;
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x36, 0x40]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 1, "predicate true → default-arm");
    }

    #[test]
    fn op2f_subop_36_axis_threshold_above_skips() {
        // op[2]=0xFF, axis=0: 0xFF < 0x8E is false → skip 4.
        let mut host = TestHost::default();
        host.axis_threshold = 0;
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x36, 0xFF]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 4);
    }

    #[test]
    fn op2f_subop_37_is_inverse_of_36() {
        // 0x37: (0x8E - axis) < op[2]. With axis=0, op[2]=0xFF → 0x8E < 0xFF true.
        let mut host = TestHost::default();
        host.axis_threshold = 0;
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x37, 0xFF]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 1);
        // axis=0, op[2]=0x40 → 0x8E < 0x40 false → skip.
        let mut state2 = ActorState::new();
        let bc2 = program(&[0x2F, 0x37, 0x40]);
        step(&mut host, &mut state2, &bc2);
        assert_eq!(state2.pc, 4);
    }

    #[test]
    fn op2f_subop_38_predicate_outside_radius() {
        let mut host = TestHost::default();
        host.player_xyz = [0, 0, 0];
        let mut state = ActorState::new();
        // Actor at (10, 0, 0), player at origin → dist² = 100. r=8 → r²=64.
        // 0x38: r² < dist² → 64 < 100 true → default-arm.
        state.world_x = 10;
        let bc = program(&[0x2F, 0x38, 8]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op2f_subop_39_predicate_inside_radius() {
        let mut host = TestHost::default();
        host.player_xyz = [0, 0, 0];
        let mut state = ActorState::new();
        // Actor at (3, 0, 4), player at origin → dist² = 25. r=10 → r²=100.
        // 0x39: dist² < r² → 25 < 100 true → default-arm.
        state.world_x = 3;
        state.world_z = 4;
        let bc = program(&[0x2F, 0x39, 10]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 1);
        // Move actor to (100, 0, 0): dist² = 10000, r²=100 → false → skip.
        let mut state2 = ActorState::new();
        state2.world_x = 100;
        let bc2 = program(&[0x2F, 0x39, 10]);
        step(&mut host, &mut state2, &bc2);
        assert_eq!(state2.pc, 4);
    }

    #[test]
    fn op2f_subop_23_anim_lerp_zero_denom_is_noop() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.anim_3c = 100;
        state.anim_3e = 200;
        state.anim_40 = 300;
        // op[5] = 0 → divide-trap path; we skip the update.
        let bc = program(&[0x2F, 0x23, 0, 0, 0, 0]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.anim_3c, 100);
        assert_eq!(state.anim_3e, 200);
        assert_eq!(state.anim_40, 300);
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op2f_subop_23_anim_lerp_full_ratio_writes_target_offset() {
        let mut host = TestHost::default();
        host.dat_1f800393 = 1;
        let mut state = ActorState::new();
        // With dat=1, denom=1: t = (1 << 12) / 1 = 4096.
        // anim_3c = 0; first pass: 0 - (0 * 4096 >> 12) = 0
        //          ; second pass: 0 + ((100 - 0) * 4096 >> 12) = 100
        let bc = program(&[0x2F, 0x23, 100, 200, 300, 1]);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.anim_3c, 100);
        assert_eq!(state.anim_3e, 200);
        assert_eq!(state.anim_40, 300);
    }

    #[test]
    fn op2f_subop_04_writes_actor_world_into_bytecode_buffer() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.world_x = 10;
        state.world_y = 20;
        state.world_z = 30;
        // op[2] = 5 → writes at state.pc(0) + 5 + 3 = word indices 8, 9, 10.
        let bc = program(&[0x2F, 0x04, 5]);
        host.bytecode_buffer = bc.clone();
        step(&mut host, &mut state, &bc);
        assert_eq!(host.bytecode_buffer[8], 10);
        assert_eq!(host.bytecode_buffer[9], 20);
        assert_eq!(host.bytecode_buffer[10], 30);
        assert_eq!(state.pc, 1, "default-arm");
    }

    #[test]
    fn op2f_subop_1e_in_place_add_to_bytecode() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // op[2] = 3, op[3] = 5 → buffer[state.pc(0) + 3 + 4 = 7] += 5.
        let bc = program(&[0x2F, 0x1E, 3, 5]);
        host.bytecode_buffer = bc.clone();
        host.bytecode_buffer[7] = 100;
        step(&mut host, &mut state, &bc);
        assert_eq!(host.bytecode_buffer[7], 105);
    }

    #[test]
    fn op2f_subop_1b_copy_loop_within_bytecode() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // op[2] = 0 (src), op[3] = 4 (dst), op[4] = 3 (count).
        // src base = state.pc(0) + 0 + 5 = 5. dst base = 0 + 4 + 5 = 9.
        // Copies buffer[5..8] → buffer[9..12].
        let bc = program(&[0x2F, 0x1B, 0, 4, 3]);
        host.bytecode_buffer = bc.clone();
        host.bytecode_buffer[5] = 0xAAAA;
        host.bytecode_buffer[6] = 0xBBBB;
        host.bytecode_buffer[7] = 0xCCCC;
        // Pre-fill destination with sentinels so we can detect writes.
        host.bytecode_buffer[9] = 0;
        host.bytecode_buffer[10] = 0;
        host.bytecode_buffer[11] = 0;
        step(&mut host, &mut state, &bc);
        assert_eq!(host.bytecode_buffer[9], 0xAAAA);
        assert_eq!(host.bytecode_buffer[10], 0xBBBB);
        assert_eq!(host.bytecode_buffer[11], 0xCCCC);
    }

    #[test]
    fn op2f_subop_1b_zero_count_is_noop() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = program(&[0x2F, 0x1B, 0, 4, 0]);
        host.bytecode_buffer = bc.clone();
        let before = host.bytecode_buffer.clone();
        step(&mut host, &mut state, &bc);
        assert_eq!(host.bytecode_buffer, before);
    }

    // --- HSV helpers + ext sub-op 0x1F / 0x20 -----------------------------

    #[test]
    fn rgb_to_hsv_pure_red_round_trip() {
        let (h, s, v) = rgb_to_hsv(0xFF, 0, 0);
        assert_eq!(h, 0, "pure red has H = 0");
        assert!(s > 0xF0, "pure red is fully saturated, got {s:#x}");
        assert_eq!(v, 0xFF, "pure red has V = 0xFF");
    }

    #[test]
    fn rgb_to_hsv_pure_green_lands_in_segment_2() {
        let (h, _s, _v) = rgb_to_hsv(0, 0xFF, 0);
        // Green = 120 deg = 0x78 in this encoding.
        assert_eq!(h, 0x78);
    }

    #[test]
    fn rgb_to_hsv_pure_blue_lands_in_segment_4() {
        let (h, _s, _v) = rgb_to_hsv(0, 0, 0xFF);
        // Blue = 240 deg = 0xF0.
        assert_eq!(h, 0xF0);
    }

    #[test]
    fn rgb_to_hsv_zero_returns_zero() {
        assert_eq!(rgb_to_hsv(0, 0, 0), (0, 0, 0));
    }

    #[test]
    fn hsv_to_rgb_segment_dispatch_matches_each_arm() {
        // V = 0xFF, S = 0xFF — picks the segment based on H.
        // Segment 0 (H=0): (V, t, p) — pure red.
        let (r, g, b) = hsv_to_rgb(0, 0xFF, 0xFF);
        assert!(r >= 0xF0 && g <= 1 && b <= 1, "segment 0 ≈ pure red");
        // Segment 2 (H=0x78=120 deg): green.
        let (r, g, b) = hsv_to_rgb(0x78, 0xFF, 0xFF);
        assert!(r <= 1 && g >= 0xF0 && b <= 1, "segment 2 ≈ pure green");
        // Segment 4 (H=0xF0=240 deg): blue.
        let (r, g, b) = hsv_to_rgb(0xF0, 0xFF, 0xFF);
        assert!(r <= 1 && g <= 1 && b >= 0xF0, "segment 4 ≈ pure blue");
    }

    #[test]
    fn hsv_to_rgb_zero_saturation_returns_grey() {
        let (r, g, b) = hsv_to_rgb(0x55, 0, 0x80);
        assert_eq!((r, g, b), (0x80, 0x80, 0x80));
    }

    #[test]
    fn op2f_subop_1f_rotates_hue_on_keyframe_desc_lo() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // Pre-set actor[+0xa0..+0xa3] = packed pure-red RGB.
        state.keyframe_desc[0] = 0x00FF; // R=0xFF, G=0
        state.keyframe_desc[1] = 0x0000; // B=0
        // Sub-op 0x1F: delta H = 0x78 (= 120 deg), delta S = 0, delta V = 0.
        // Should rotate red → green.
        let bc = program(&[0x2F, 0x1F, 0x78, 0, 0]);
        step(&mut host, &mut state, &bc);
        let r = state.keyframe_desc[0] & 0xFF;
        let g = (state.keyframe_desc[0] >> 8) & 0xFF;
        let b = state.keyframe_desc[1] & 0xFF;
        assert!(
            g > r,
            "hue-rotated by 120 deg should make G dominant ({r:#x},{g:#x},{b:#x})"
        );
        assert!(
            g > b,
            "hue-rotated by 120 deg should make G dominate B ({r:#x},{g:#x},{b:#x})"
        );
        // FUN_8001a6c8 caps at 0xF8.
        assert!(g <= 0xF8);
        // PC advances by 1 (default_arm).
        assert_eq!(state.pc, 1);
    }

    #[test]
    fn op2f_subop_20_targets_keyframe_desc_hi_pair() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        // Pre-set actor[+0xa4..+0xa7] = packed pure-blue.
        state.keyframe_desc[2] = 0x0000;
        state.keyframe_desc[3] = 0x00FF; // B=0xFF
        // Sub-op 0x20: delta H = 0x78 (= 120 deg). Blue → red.
        let bc = program(&[0x2F, 0x20, 0x78, 0, 0]);
        step(&mut host, &mut state, &bc);
        let r = state.keyframe_desc[2] & 0xFF;
        let g = (state.keyframe_desc[2] >> 8) & 0xFF;
        let b = state.keyframe_desc[3] & 0xFF;
        assert!(r > g);
        assert!(r > b);
        // 0x1F slot must be untouched.
        assert_eq!(state.keyframe_desc[0], 0);
        assert_eq!(state.keyframe_desc[1], 0);
    }

    #[test]
    fn op2f_subop_1f_value_decrement_dims_color() {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        state.keyframe_desc[0] = 0x80FF; // R=0xFF, G=0x80
        state.keyframe_desc[1] = 0x0040; // B=0x40
        let v_before = (state.keyframe_desc[0] & 0xFF).max((state.keyframe_desc[0] >> 8) & 0xFF);
        // Delta H = 0, delta S = 0, delta V = -0x40 (use signed).
        let bc = program(&[0x2F, 0x1F, 0, 0, (-0x40i16) as u16]);
        step(&mut host, &mut state, &bc);
        let r_after = state.keyframe_desc[0] & 0xFF;
        let g_after = (state.keyframe_desc[0] >> 8) & 0xFF;
        let v_after = r_after.max(g_after);
        assert!(
            v_after < v_before,
            "lowering V should reduce the dominant channel ({v_before:#x} -> {v_after:#x})"
        );
    }
}
