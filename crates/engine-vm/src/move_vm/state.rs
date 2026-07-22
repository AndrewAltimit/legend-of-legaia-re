//! Core move-VM value types: per-actor state, opcode enum, step/ext results.

/// u16 slots covered by [`ActorState::anim_block`], counted from `+0xAC`.
/// Covers every byte offset the ported opcodes address (the deepest is
/// `0xF8`, op `0x38`).
pub const ANIM_BLOCK_SLOTS: usize = 128;

/// The `+0xAC..` u16 window as a newtype, purely so [`ActorState`] can keep
/// deriving `Default` past the 32-element array limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimBlock(pub [u16; ANIM_BLOCK_SLOTS]);

impl Default for AnimBlock {
    fn default() -> Self {
        Self([0; ANIM_BLOCK_SLOTS])
    }
}

impl core::ops::Deref for AnimBlock {
    type Target = [u16; ANIM_BLOCK_SLOTS];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for AnimBlock {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Per-actor move-VM state. One instance per actor that's running a move.
///
/// Field naming uses the byte-offset convention from `docs/subsystems/move-vm.md`
/// to keep the link to the decompilation explicit. Engines free to back this
/// with whatever data structure makes sense - the VM mutates this struct
/// directly and dispatches side effects through [`MoveHost`].
#[derive(Debug, Clone, Default)]
pub struct ActorState {
    /// `+0x10` - actor flag word. Bit `0x8` set by op 0x08 HALT;
    /// bits `0x2` (op 0x3A/0x3B), `0x1000` (op 0x0A KEYFRAME_LOAD),
    /// `0x10000` and `0x40000000` (composite control word) toggled by various.
    pub flags: u32,
    /// `+0x14` - world X.
    pub world_x: i16,
    /// `+0x16` - world Y.
    pub world_y: i16,
    /// `+0x18` - world Z.
    pub world_z: i16,
    /// `+0x22` - Y-rotation accumulator (cleared by op 0x3D).
    pub y_rot: i16,
    /// `+0x24` - render bank 0.
    pub render_24: i16,
    /// `+0x26` - render bank 1 (op 0x06 / 0x39 / 0x05).
    pub render_26: i16,
    /// `+0x28` - render bank 2.
    pub render_28: i16,
    /// `+0x2A` - Y mirror (kept in sync with [`world_y`] for collision).
    pub world_y_mirror: i16,
    /// `+0x3C` - animation bank slot 0 (op 0x00, written `v << 3`).
    pub anim_3c: i16,
    /// `+0x3E` - animation bank slot 1.
    pub anim_3e: i16,
    /// `+0x40` - animation bank slot 2.
    pub anim_40: i16,
    /// `+0x42` - generic per-actor scalar (op 0x10).
    pub field_42: u16,
    /// `+0x50` - midpoint blend / sub-state byte. Set by ext op 0x0C and
    /// incremented by ext op 0x0D; consumed as the 4th argument to the
    /// `FUN_801E45BC` midpoint helper from ext ops 0x0E / 0x12.
    pub field_50: u16,
    /// `+0x52` - control word written by op 0x15 (the `0x400` bit additionally
    /// clears `flags & 0x80`).
    pub field_52: u16,
    /// `+0x54` - wait/timer accumulator (op 0x09 sets `v << 3`; ticked down
    /// elsewhere). Setting non-zero ends the per-frame interpreter loop.
    pub wait_timer: i16,
    /// `+0x56` - move-table sub-state (cleared by ops 0x13 / 0x42).
    pub move_substate: i16,
    /// `+0x5A` - move sub-mode marker (set to 2/4/6/7 by various ops).
    pub move_submode: i16,
    /// `+0x5C` - anim-loop counter (cleared by op 0x3C SCRATCH_WRITE).
    pub field_5c: i16,
    /// `+0x62` - local flag bank (16 bits). AND/OR by 0x31/0x32.
    pub local_flags: u16,
    /// `+0x68` - generic slot.
    pub field_68: i16,
    /// `+0x6A` - generic slot (op 0x44, written `v << 3`).
    pub field_6a: i16,
    /// `+0x6C` - keyframe count (op 0x0A, byte slot).
    pub keyframe_count: u8,
    /// `+0x6D` - face / body rotation index (cleared by ext op 0x02; written
    /// by op 0x21).
    pub face_rotation: u8,
    /// `+0x70` - **the move-VM PC, in u16 units**.
    pub pc: i16,
    /// `+0x72` - generic slot (op 0x0E).
    pub field_72: u16,
    /// `+0x74` - composite control word (op 0x0C builds it; op 0x33 clears
    /// bit `0x40000000`).
    pub field_74: u32,
    /// `+0x78` - generic slot (op 0x0C).
    pub field_78: u16,
    /// `+0x7A` - generic slot (op 0x12).
    pub field_7a: u16,
    /// `+0x7C` - the per-lane morph completion bitfield the ramp
    /// envelope owns (`legaia_engine_vm::move_buffer::MoveBufferState::done_mask`).
    /// The move VM only ever **clears** it, from op `0x0A`'s reset arm.
    pub field_7c: u32,
    /// `+0x80` - animation bank 2 slot 0 (op 0x04, `v << 3`).
    pub anim_80: i16,
    /// `+0x82` - animation bank 2 slot 1.
    pub anim_82: i16,
    /// `+0x84` - animation bank 2 slot 2.
    pub anim_84: i16,
    /// `+0x86` - composite slot bits (set by ext op 0x10/0x11/0x15/0x16).
    pub field_86: u16,
    /// `+0x88` - saved PC for op 0x18/0x19 jump-back loop.
    pub field_88: u16,
    /// `+0x8A` - saved PC for op 0x1A/0x1B jump-back loop.
    pub field_8a: u16,
    /// `+0x8C` - counter for op 0x18/0x19.
    pub field_8c: u16,
    /// `+0x8E` - counter for op 0x1A/0x1B.
    pub field_8e: u16,
    /// `+0x90` - tween source 0 (op 0x37 absolute, 0x35/0x2D add).
    pub tween_src_x: i16,
    /// `+0x92` - tween source 1.
    pub tween_src_y: i16,
    /// `+0x94` - tween source 2.
    pub tween_src_z: i16,
    /// `+0x96` - tween scale 0 (op 0x2E `v << 3`, 0x29 absolute).
    pub tween_scale_x: i16,
    /// `+0x98` - tween scale 1.
    pub tween_scale_y: i16,
    /// `+0x9A` - tween scale 2.
    pub tween_scale_z: i16,
    /// `+0x9C` - keyframe gate / index (op 0x2C sets to 1; op 0x30 clears).
    pub field_9c: i32,
    /// `+0x9E` - keyframe descriptor word.
    pub field_9e: u16,
    /// `+0xA0..0xA6` - keyframe buffer descriptor (op 0x2C).
    pub keyframe_desc: [u16; 4],
    /// `+0xA8` - heap or inline keyframe pointer / value (op 0x2C / 0x26).
    pub field_a8: i32,
    /// `+0xAC..` - per-frame anim slot block. Modeled as a flat array
    /// addressable by byte-offset for opcodes that touch deep into it.
    /// Indices are in u16 units relative to `+0xAC`.
    ///
    /// Sized to the **highest byte offset any ported opcode writes**
    /// (`0xF8`, from op `0x38`) rather than to the `+0xCA` slot names
    /// alone: a short array silently swallowed the per-slot writes of
    /// the variable-length opcodes (op `0x0A` walks `0x0C + 2*i` and
    /// `0x1C + 2*i`), which reads as "the opcode ran" while the data
    /// went nowhere.
    pub anim_block: AnimBlock,
    /// `+0xCA` - duration slot (op 0x1C, `v << 3`).
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

    /// Read a **byte** in `anim_block` by byte-offset relative to `+0xAC`.
    /// The window is stored as u16 slots, so an even offset is the low half
    /// of its slot and an odd offset the high half (little-endian, as the
    /// PSX record is).
    pub fn anim_block_u8(&self, byte_off: usize) -> u8 {
        let word = self.anim_block_u16(byte_off);
        if byte_off.is_multiple_of(2) {
            (word & 0xFF) as u8
        } else {
            (word >> 8) as u8
        }
    }

    /// Write a **byte** in `anim_block` by byte-offset relative to `+0xAC`,
    /// leaving the other half of the slot alone. Byte-stride arrays in the
    /// retail record (op `0x0A`'s `+0xB0 + lane` morph indices) need this -
    /// writing them through the u16 setter makes consecutive lanes
    /// overwrite each other.
    pub fn anim_block_u8_set(&mut self, byte_off: usize, value: u8) {
        let word = self.anim_block_u16(byte_off);
        let merged = if byte_off.is_multiple_of(2) {
            (word & 0xFF00) | u16::from(value)
        } else {
            (word & 0x00FF) | (u16::from(value) << 8)
        };
        self.anim_block_u16_set(byte_off, merged);
    }

    /// Zero the morph-weight halfword at `actor + 0xA0 + lane*2` - op
    /// `0x0A`'s `sh zero, 0xa0(a1)` with `a1 = actor + lane*2`.
    ///
    /// That address range is the ramp envelope's lane array
    /// (`move_buffer::MoveBufferState::lanes`), and in the retail record it
    /// **overlaps** the op-`0x2C` keyframe buffer descriptor at `+0xA0..+0xA8`
    /// and the pointer slot at `+0xA8`. This port keeps that overlap rather
    /// than giving the weights a private array, because the overlap is the
    /// retail layout, not a modelling shortcut.
    pub fn zero_keyframe_weight(&mut self, lane: usize) {
        match 0xA0 + lane * 2 {
            off @ 0xA0..=0xA6 => self.keyframe_desc[(off - 0xA0) / 2] = 0,
            0xA8 => self.field_a8 = ((self.field_a8 as u32) & 0xFFFF_0000) as i32,
            0xAA => self.field_a8 = ((self.field_a8 as u32) & 0x0000_FFFF) as i32,
            off => self.anim_block_u16_set(off - 0xAC, 0),
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
    /// Out-of-range opcode (`>= 0x47`) - the original dispatcher's bound check
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
    MoveImage = 0x40,
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
            0x40 => Self::MoveImage,
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
