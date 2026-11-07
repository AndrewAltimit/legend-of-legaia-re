//! Move-VM program disassembler.
//!
//! Decodes the u16 bytecode the move VM (`FUN_80023070`, 71 opcodes, jump
//! table `0x80010778`) runs out of a part's move buffer, including the
//! `0x2F` `OVERLAY_EXT` sub-opcodes of the field overlay's extension
//! dispatcher (`FUN_801D362C`, 61 sub-opcodes, jump table `0x801CE868`).
//! Side-effect free: it computes each instruction's width, a mnemonic, its
//! operands and the actor fields it writes, and never runs anything. The
//! executing VM is `legaia_engine_vm::move_vm`; the sibling for the field VM's
//! byte stream is `legaia_scene_formats::field_disasm`.
//!
//! A move program lives in a part **record**: `[i16 model_sel][u16 reserved]`
//! then bytecode from halfword 2 (the seater `FUN_80021B04` starts the PC at
//! `2`). Records come from slot-B cast modules and summon stagers (PROT
//! 0903..0966, [`crate::slot_b_module`]), the battle overlay's tail and each
//! scene's prescript stager table.
//!
//! ## Widths
//!
//! Every width is the `param_3` the dispatcher's arm leaves in `$a2` before
//! the shared epilogue at `0x80024150` (`pc += a2`), read off the
//! disassembly: the decompiled C drops most of them into branch delay slots.
//! Four widths depend on operands - `0x0A` (`3 + 3 * op[2]`), `0x3C`
//! (`2 + 6 * op[1]`), `0x3D` (`3 + 6 * op[2]`) and `0x2F` (the sub-op's own
//! width, `li s2, N` in `FUN_801D362C`'s arms, `1` for an out-of-range
//! sub-op). `0x08` `HALT` advances `0` and ends the pass; `0x0B` has no arm of
//! its own (its jump-table word is the epilogue) and advances by whatever
//! `$a2` the previous instruction left, so a linear walk gives it the
//! previous instruction's width.
//!
//! Control flow is reported, not followed: `0x18` / `0x1A` open a counted
//! loop, `0x19` / `0x1B` close it (encoded width `1`; the taken side lands
//! two halfwords past the opening instruction), and ten `0x2F` sub-ops are
//! conditional branches whose taken side adds a signed displacement to the
//! fall-through width. See `docs/subsystems/move-vm.md` and
//! `docs/subsystems/move-vm-overlay-ext.md`.

use std::fmt::Write as _;

/// One past the highest main opcode (`sltiu v0, v1, 0x47` at `0x800230C8`).
pub const MOVE_OP_COUNT: u16 = 0x47;
/// One past the highest `0x2F` sub-opcode (`sltiu v1, 0x3D` at `0x801D3654`).
pub const MOVE_EXT_COUNT: u16 = 0x3D;

/// `HALT`.
pub const OP_HALT: u16 = 0x08;
/// `WAIT_SET`.
pub const OP_WAIT: u16 = 0x09;
/// `KEYFRAME_LOAD`.
pub const OP_KEYFRAME_LOAD: u16 = 0x0A;
/// The opcode with no arm of its own.
pub const OP_CARRY: u16 = 0x0B;
/// `OVERLAY_EXT`.
pub const OP_OVERLAY_EXT: u16 = 0x2F;
/// `KEYFRAME_SEAT`.
pub const OP_KEYFRAME_SEAT: u16 = 0x3C;
/// `KEYFRAME_RETARGET`.
pub const OP_KEYFRAME_RETARGET: u16 = 0x3D;
/// The loop-counter bit that makes `0x19` / `0x1B` branch forever.
pub const LOOP_FOREVER: u16 = 0x4000;

/// Width in halfwords of each main opcode whose width is a constant. `0`
/// marks the operand-dependent and special opcodes (`0x08`, `0x0A`, `0x0B`,
/// `0x2F`, `0x3C`, `0x3D`), which [`insn_width`] resolves.
pub const MOVE_OP_HALFWORDS: [u8; MOVE_OP_COUNT as usize] = [
    4, 4, 2, 2, 4, 4, 2, 4, // 0x00
    0, 2, 0, 0, 6, 2, 2, 2, // 0x08
    2, 2, 2, 16, 5, 2, 2, 2, // 0x10
    2, 1, 2, 1, 2, 2, 8, 8, // 0x18
    3, 7, 1, 13, 3, 2, 5, 3, // 0x20
    2, 2, 2, 4, 5, 4, 4, 0, // 0x28
    1, 2, 2, 1, 9, 3, 3, 3, // 0x30
    2, 4, 1, 1, 0, 0, 2, 2, // 0x38
    7, 2, 15, 1, 4, 8, 4, // 0x40
];

/// Fall-through width in halfwords of each `0x2F` sub-opcode, the
/// `li s2, N` each arm of `FUN_801D362C` leaves before the epilogue at
/// `0x801D4A3C`. The width counts the `0x2F` word and the sub-op word.
pub const MOVE_EXT_HALFWORDS: [u8; MOVE_EXT_COUNT as usize] = [
    16, 2, 2, 2, 3, 5, 7, 7, // 0x00
    2, 2, 3, 3, 3, 3, 11, 2, // 0x08
    2, 2, 8, 4, 4, 2, 2, 8, // 0x10
    5, 8, 8, 5, 3, 3, 4, 5, // 0x18
    5, 5, 5, 6, 8, 3, 3, 3, // 0x20
    5, 5, 8, 6, 7, 6, 13, 3, // 0x28
    5, 3, 3, 6, 3, 3, 4, 4, // 0x30
    4, 4, 3, 4, 6, // 0x38
];

/// Mnemonic of each main opcode.
pub const MOVE_OP_NAMES: [&str; MOVE_OP_COUNT as usize] = [
    "VEL_SET",             // 0x00 +0x3C..+0x40 = v << 3
    "WORLD_ADD",           // 0x01
    "SPEED_SET",           // 0x02 +0x98 = v << 3
    "WORLD_ROTATE_ADD",    // 0x03
    "SPIN_SET",            // 0x04 +0x80..+0x84 = v << 3
    "ROT_ADD",             // 0x05
    "ROT_Y_SET",           // 0x06
    "WORLD_SET",           // 0x07
    "HALT",                // 0x08
    "WAIT",                // 0x09
    "KEYFRAME_LOAD",       // 0x0A
    "CARRY",               // 0x0B
    "CTRL_74",             // 0x0C
    "LEVEL_RATE",          // 0x0D +0x90 = v << 3
    "SCALE_SET",           // 0x0E +0x72
    "SCALE_RATE",          // 0x0F +0x92 = v << 3
    "WRITE_42",            // 0x10
    "RATE_7A",             // 0x11 +0x94 = v << 3
    "SET_7A",              // 0x12
    "DK4_DEFAULT",         // 0x13
    "CHANNEL_RATES",       // 0x14 +0xC0..+0xC6 = v << 3
    "CTRL_52",             // 0x15
    "STUB_16",             // 0x16
    "BATTLE_EXT",          // 0x17
    "LOOP_A",              // 0x18
    "LOOP_BACK_A",         // 0x19
    "LOOP_B",              // 0x1A
    "LOOP_BACK_B",         // 0x1B
    "LENGTH_RATE",         // 0x1C +0xCA = v << 3
    "SFX_SLOT3",           // 0x1D
    "MODE4_SETUP",         // 0x1E
    "MORPH_INSTALL",       // 0x1F
    "SLOT_B_HOOK",         // 0x20
    "SHATTER_ROW",         // 0x21
    "NOP",                 // 0x22
    "DK4_SPRITE",          // 0x23
    "SPRITE_ADD",          // 0x24
    "SPAWN_CHILD",         // 0x25
    "SPRITE_SET",          // 0x26
    "SET_B0_B2",           // 0x27
    "TURN_ADD",            // 0x28 +0x9A += v
    "HEADING_SET",         // 0x29 +0x96 = v
    "TURN_SET",            // 0x2A +0x9A = v << 3
    "RATES_SET",           // 0x2B +0x90..+0x94 = v
    "KEY_BUFFER_ALLOC",    // 0x2C
    "RATES_ADD",           // 0x2D
    "HEADING_MOTION",      // 0x2E +0x96/+0x98/+0x9A = v << 3
    "OVERLAY_EXT",         // 0x2F
    "KEY_BUFFER_FREE",     // 0x30
    "LFLAG_AND",           // 0x31
    "LFLAG_OR",            // 0x32
    "CLEAR_BIT_40000000",  // 0x33
    "SOUND_EMITTER_SETUP", // 0x34
    "RATES_ADD2",          // 0x35
    "SPEED_TURN_SET",      // 0x36
    "RATES_SET2",          // 0x37
    "B2_ADD",              // 0x38
    "ROT_SET",             // 0x39
    "FLAG_2_SET",          // 0x3A
    "FLAG_2_CLEAR",        // 0x3B
    "KEYFRAME_SEAT",       // 0x3C
    "KEYFRAME_RETARGET",   // 0x3D
    "BLEND_CURSOR",        // 0x3E
    "BLEND_RATE",          // 0x3F
    "MOVE_IMAGE",          // 0x40
    "SET_B2",              // 0x41
    "DK4_RIBBON",          // 0x42
    "FIELD_CULL",          // 0x43
    "SET_9E_68_6A",        // 0x44
    "MODE7_SETUP",         // 0x45
    "TWEEN_SEED",          // 0x46
];

/// Mnemonic of each `0x2F` sub-opcode.
pub const MOVE_EXT_NAMES: [&str; MOVE_EXT_COUNT as usize] = [
    "X_NOP16",           // 0x00
    "X_PRINT_POS",       // 0x01
    "X_FACE_ROT_CLEAR",  // 0x02
    "X_FLAG_1000_CLEAR", // 0x03
    "X_POKE_POS",        // 0x04
    "X_POKE_RAND",       // 0x05
    "X_IF_OUT_BOX",      // 0x06
    "X_IF_IN_BOX",       // 0x07
    "X_PRED_SET",        // 0x08
    "X_PRED_CLEAR",      // 0x09
    "X_IF_PRED",         // 0x0A
    "X_IF_NOT_PRED",     // 0x0B
    "X_CURVE_T_SET",     // 0x0C
    "X_CURVE_T_ADD",     // 0x0D
    "X_BEZIER",          // 0x0E
    "X_CYCLE_CLEAR",     // 0x0F
    "X_CYCLE_LATCH",     // 0x10
    "X_SLOT_SAVE_CYC",   // 0x11
    "X_BEZIER_SLOT",     // 0x12
    "X_IF_FLAG",         // 0x13
    "X_IF_NOT_FLAG",     // 0x14
    "X_FLAG_800000",     // 0x15
    "X_FLAG_200000",     // 0x16
    "X_OBJFX_WRITE",     // 0x17
    "X_OBJFX_RESET",     // 0x18
    "X_OBJFX_ADD",       // 0x19
    "X_OBJFX_YAW",       // 0x1A
    "X_POKE_COPY",       // 0x1B
    "X_FLAG_SET",        // 0x1C
    "X_FLAG_CLEAR",      // 0x1D
    "X_POKE_ADD",        // 0x1E
    "X_HSV_A0",          // 0x1F
    "X_HSV_A4",          // 0x20
    "X_VEL_ADD",         // 0x21
    "X_WORLD_ADD",       // 0x22
    "X_VEL_TOWARD",      // 0x23
    "X_LERP_CAMERA",     // 0x24
    "X_SLOT_SAVE",       // 0x25
    "X_SLOT_LOAD",       // 0x26
    "X_RATES_SAVE",      // 0x27
    "X_RATES_LOAD",      // 0x28
    "X_SCRATCH_RAMP",    // 0x29
    "X_LERP_PLAYER",     // 0x2A
    "X_STRIP_SET",       // 0x2B
    "X_STRIP_EMIT",      // 0x2C
    "X_STRIP_ADD",       // 0x2D
    "X_GP0_PACKET",      // 0x2E
    "X_WRITE_B9D8",      // 0x2F
    "X_POKE_RAND_PICK",  // 0x30
    "X_ROT_SAVE",        // 0x31
    "X_ROT_LOAD",        // 0x32
    "X_CHANNEL_ADD",     // 0x33
    "X_SCALE_SAVE",      // 0x34
    "X_SCALE_LOAD",      // 0x35
    "X_IF_COUNT_A",      // 0x36
    "X_IF_COUNT_B",      // 0x37
    "X_IF_FAR",          // 0x38
    "X_IF_NEAR",         // 0x39
    "X_POKE_ANGLE",      // 0x3A
    "X_POKE_PARTY_POS",  // 0x3B
    "X_FADE",            // 0x3C
];

/// What an instruction does to the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Falls through to the next instruction.
    Next,
    /// `0x08`: ends the pass, the PC stays on it.
    Halt,
    /// `0x09`: ends the pass; the part resumes after `units` of wait
    /// (`+0x54 = v << 3`, drained `DAT_1F800393 * DAT_1F80037D` a frame).
    Wait { units: i32 },
    /// `0x18` / `0x1A`: latch this PC and a repeat count into loop `reg`
    /// (`b'A'` / `b'B'`).
    LoopOpen { reg: u8, count: u16 },
    /// `0x19` / `0x1B`: decrement loop `reg` and branch back to two
    /// halfwords past its opening instruction until it underflows (forever
    /// when the count carries [`LOOP_FOREVER`]).
    LoopBack { reg: u8 },
    /// A `0x2F` conditional branch: `target` is the taken side's byte
    /// offset in the buffer (`None` when it would fall below the buffer).
    Branch { target: Option<usize> },
    /// `0x0B`: advances by the previous instruction's width.
    Carry,
    /// An opcode `>= 0x47` (or a halfword past the buffer): the dispatcher's
    /// bound check exits the loop without advancing.
    End,
}

/// One decoded instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveInsn {
    /// Byte offset of the opcode word in the walked buffer.
    pub offset: usize,
    /// The opcode word.
    pub op: u16,
    /// The `0x2F` sub-opcode.
    pub sub: Option<u16>,
    /// Width in halfwords (the encoded, fall-through width). `0` for `HALT`
    /// and [`Flow::End`].
    pub width: usize,
    /// Every halfword of the instruction, the opcode word first.
    pub words: Vec<u16>,
    /// Control effect.
    pub flow: Flow,
}

impl MoveInsn {
    /// Operand `i` (`op[i]`), sign-extended; `0` past the instruction.
    pub fn arg(&self, i: usize) -> i32 {
        i32::from(self.words.get(i).copied().unwrap_or(0) as i16)
    }

    /// Operand `i` unsigned.
    pub fn uarg(&self, i: usize) -> u16 {
        self.words.get(i).copied().unwrap_or(0)
    }

    /// The mnemonic: the main op's, or the sub-op's for `0x2F`.
    pub fn mnemonic(&self) -> &'static str {
        match self.sub {
            Some(s) if s < MOVE_EXT_COUNT => MOVE_EXT_NAMES[s as usize],
            Some(_) => "X_OUT_OF_RANGE",
            None if self.op < MOVE_OP_COUNT => MOVE_OP_NAMES[self.op as usize],
            None => "END",
        }
    }

    /// Byte offset one past the instruction.
    pub fn end(&self) -> usize {
        self.offset + self.width * 2
    }
}

fn rd(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]))
}

/// Width in halfwords of the instruction whose words `w(i)` reads, or `None`
/// for a width the instruction alone does not fix (`0x0B`). `HALT` and an
/// out-of-range opcode return `Some(0)`.
pub fn insn_width(w: impl Fn(usize) -> u16) -> Option<usize> {
    let op = w(0);
    if op >= MOVE_OP_COUNT {
        return Some(0);
    }
    Some(match op {
        OP_HALT => 0,
        OP_CARRY => return None,
        OP_KEYFRAME_LOAD => 3 + 3 * (w(2) as i16).max(0) as usize,
        OP_KEYFRAME_SEAT => 2 + 6 * (w(1) as i16).max(0) as usize,
        OP_KEYFRAME_RETARGET => 3 + 6 * (w(2) as i16).max(0) as usize,
        OP_OVERLAY_EXT => ext_width(w(1)),
        _ => usize::from(MOVE_OP_HALFWORDS[op as usize]),
    })
}

/// Fall-through width of a `0x2F` instruction with sub-opcode `sub`: the
/// arm's `li s2, N`, or `1` for an out-of-range sub-op (the `li s2, 0x1` in
/// the bounds check's delay slot at `0x801D365C`).
pub fn ext_width(sub: u16) -> usize {
    MOVE_EXT_HALFWORDS
        .get(sub as usize)
        .map_or(1, |&n| usize::from(n))
}

/// For a `0x2F` conditional-branch sub-op: `(fall-through width, operand
/// index of the displacement)`. The taken side returns `width + delta`
/// (`lhu v0,..(s3); addiu s2,v0,..` at `0x801D4838`), sign-extended by the
/// epilogue.
pub fn ext_branch(sub: u16) -> Option<(usize, usize)> {
    match sub {
        0x06 | 0x07 => Some((7, 6)),
        0x0A | 0x0B => Some((3, 2)),
        0x13 | 0x14 | 0x36..=0x39 => Some((4, 3)),
        _ => None,
    }
}

/// Decode the instruction at byte `offset`. `carry` is the width a `0x0B`
/// there advances by (the previous instruction's); `None` decodes `0x0B` with
/// width `0`.
pub fn decode_at(bytes: &[u8], offset: usize, carry: Option<usize>) -> MoveInsn {
    let w = |i: usize| rd(bytes, offset + i * 2).unwrap_or(0);
    let Some(op) = rd(bytes, offset) else {
        return MoveInsn {
            offset,
            op: 0xFFFF,
            sub: None,
            width: 0,
            words: Vec::new(),
            flow: Flow::End,
        };
    };
    let width = insn_width(w).unwrap_or(carry.unwrap_or(0));
    let words: Vec<u16> = (0..width.max(1))
        .map_while(|i| rd(bytes, offset + i * 2))
        .collect();
    let sub = (op == OP_OVERLAY_EXT).then(|| w(1));
    let flow = match op {
        o if o >= MOVE_OP_COUNT => Flow::End,
        OP_HALT => Flow::Halt,
        OP_WAIT => Flow::Wait {
            units: i32::from(w(1) as i16) << 3,
        },
        OP_CARRY => Flow::Carry,
        0x18 => Flow::LoopOpen {
            reg: b'A',
            count: w(1),
        },
        0x1A => Flow::LoopOpen {
            reg: b'B',
            count: w(1),
        },
        0x19 => Flow::LoopBack { reg: b'A' },
        0x1B => Flow::LoopBack { reg: b'B' },
        OP_OVERLAY_EXT => match ext_branch(w(1)) {
            Some((base, di)) => {
                let delta = isize::from(w(di) as i16);
                let target = (offset as isize + (base as isize + delta) * 2).max(-1);
                Flow::Branch {
                    target: usize::try_from(target).ok(),
                }
            }
            None => Flow::Next,
        },
        _ => Flow::Next,
    };
    MoveInsn {
        offset,
        op,
        sub,
        width,
        words,
        flow,
    }
}

/// Walk options for [`disassemble`].
#[derive(Debug, Clone, Copy)]
pub struct WalkOptions {
    /// Keep walking past a `HALT` (to list code a branch reaches, or the
    /// next record in a band). The walk otherwise stops at the first `HALT`
    /// no pending forward branch jumps over.
    pub through_halt: bool,
    /// Stop after this many instructions.
    pub max_insns: usize,
    /// Stop at this byte offset (exclusive).
    pub end: Option<usize>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            through_halt: false,
            max_insns: 4096,
            end: None,
        }
    }
}

/// Linear walk from byte `start`, one [`MoveInsn`] per instruction. Stops at
/// a [`Flow::End`], an advance of `0`, a `HALT` (unless a forward `0x2F`
/// branch above it lands past it, or [`WalkOptions::through_halt`]), the
/// buffer end, or the option bounds.
pub fn disassemble(bytes: &[u8], start: usize, opts: WalkOptions) -> Vec<MoveInsn> {
    let mut out = Vec::new();
    let mut pc = start;
    let mut carry = None;
    let mut pending: Vec<usize> = Vec::new();
    let limit = opts.end.unwrap_or(bytes.len()).min(bytes.len());
    while out.len() < opts.max_insns && pc + 2 <= limit {
        let insn = decode_at(bytes, pc, carry);
        if let Flow::Branch { target: Some(t) } = insn.flow
            && t > pc
        {
            pending.push(t);
        }
        let flow = insn.flow;
        let width = insn.width;
        out.push(insn);
        match flow {
            Flow::End => break,
            Flow::Halt => {
                if opts.through_halt {
                    pc += 2;
                    carry = None;
                    continue;
                }
                match pending.iter().copied().filter(|&t| t > pc).min() {
                    Some(t) => {
                        pc = t;
                        carry = None;
                        continue;
                    }
                    None => break,
                }
            }
            _ => {}
        }
        if width == 0 {
            break;
        }
        carry = Some(width);
        pc += width * 2;
    }
    out
}

/// A record's header and program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRecord {
    /// Byte offset of the record in the buffer.
    pub offset: usize,
    /// `[i16 model_sel]`: `-1` transform node, `0x4000` / `0x4001` render
    /// node, `>= 0` library mesh.
    pub model_sel: i16,
    /// The reserved halfword.
    pub reserved: u16,
    /// The program from halfword 2.
    pub program: Vec<MoveInsn>,
}

impl MoveRecord {
    /// Byte offset one past the last decoded instruction (past the `HALT`'s
    /// word for a halted program).
    pub fn end(&self) -> usize {
        self.program.last().map_or(self.offset + 4, |i| {
            if i.flow == Flow::Halt {
                i.offset + 2
            } else {
                i.end()
            }
        })
    }
}

/// Decode the record at byte `offset`: header, then the program walked from
/// halfword 2.
pub fn disassemble_record(bytes: &[u8], offset: usize, opts: WalkOptions) -> Option<MoveRecord> {
    let model_sel = rd(bytes, offset)? as i16;
    let reserved = rd(bytes, offset + 2)?;
    Some(MoveRecord {
        offset,
        model_sel,
        reserved,
        program: disassemble(bytes, offset + 4, opts),
    })
}

/// How a byte offset is printed: raw, or as a VA at `base + offset`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Addressing {
    /// The buffer's load base; `None` prints buffer offsets.
    pub base: Option<u32>,
}

impl Addressing {
    /// The printed address of byte `offset`.
    pub fn at(&self, offset: usize) -> String {
        match self.base {
            Some(b) => format!("{:08X}", b.wrapping_add(offset as u32)),
            None => format!("{offset:06X}"),
        }
    }
}

/// Field writes of a straight store opcode: `(actor offset, operand, shift,
/// add)`. `shift = 3` is the `v << 3` the rate / timer arms store.
fn simple_writes(op: u16) -> &'static [(u16, usize, u8, bool)] {
    match op {
        0x00 => &[
            (0x3C, 1, 3, false),
            (0x3E, 2, 3, false),
            (0x40, 3, 3, false),
        ],
        0x01 => &[(0x14, 1, 0, true), (0x16, 2, 0, true), (0x18, 3, 0, true)],
        0x02 => &[(0x98, 1, 3, false)],
        0x04 => &[
            (0x80, 1, 3, false),
            (0x82, 2, 3, false),
            (0x84, 3, 3, false),
        ],
        0x05 => &[(0x24, 1, 0, true), (0x26, 2, 0, true), (0x28, 3, 0, true)],
        0x06 => &[(0x26, 1, 0, false)],
        0x07 => &[
            (0x14, 1, 0, false),
            (0x16, 2, 0, false),
            (0x18, 3, 0, false),
        ],
        0x0D => &[(0x90, 1, 3, false)],
        0x0E => &[(0x72, 1, 0, false)],
        0x0F => &[(0x92, 1, 3, false)],
        0x10 => &[(0x42, 1, 0, false)],
        0x11 => &[(0x94, 1, 3, false)],
        0x12 => &[(0x7A, 1, 0, false)],
        0x14 => &[
            (0xC0, 1, 3, false),
            (0xC2, 2, 3, false),
            (0xC4, 3, 3, false),
            (0xC6, 4, 3, false),
        ],
        0x15 => &[(0x52, 1, 0, false)],
        0x1C => &[(0xCA, 1, 3, false)],
        0x24 => &[
            (0xA8, 1, 0, true),
            (0xAA, 2, 0, true),
            (0xAC, 1, 0, true),
            (0xAE, 2, 0, true),
        ],
        0x26 => &[
            (0xA8, 1, 0, false),
            (0xAA, 2, 0, false),
            (0xAC, 3, 0, false),
            (0xAE, 4, 0, false),
        ],
        0x27 => &[(0xB0, 1, 0, false), (0xB2, 2, 0, false)],
        0x28 => &[(0x9A, 1, 0, true)],
        0x29 => &[(0x96, 1, 0, false)],
        0x2A => &[(0x9A, 1, 3, false)],
        0x2B => &[
            (0x90, 1, 0, false),
            (0x92, 2, 0, false),
            (0x94, 3, 0, false),
        ],
        0x2D => &[(0x90, 1, 0, true), (0x92, 2, 0, true), (0x94, 3, 0, true)],
        0x2E => &[
            (0x96, 1, 3, false),
            (0x98, 2, 3, false),
            (0x9A, 3, 3, false),
        ],
        0x35 => &[(0x90, 1, 0, true), (0x92, 2, 0, true)],
        0x36 => &[(0x98, 1, 3, false), (0x9A, 2, 3, false)],
        0x37 => &[(0x90, 1, 0, false), (0x92, 2, 0, false)],
        0x38 => &[(0xB2, 1, 0, true)],
        0x39 => &[
            (0x24, 1, 0, false),
            (0x26, 2, 0, false),
            (0x28, 3, 0, false),
        ],
        0x3E => &[(0x22, 1, 0, false)],
        0x3F => &[(0xD0, 1, 0, false)],
        0x41 => &[(0xB2, 1, 0, false)],
        0x44 => &[
            (0x9E, 1, 0, false),
            (0x68, 2, 0, false),
            (0x6A, 3, 3, false),
        ],
        _ => &[],
    }
}

/// `lh a + (lh b << 8) + (lh c << 16)`, the unmasked packed word the
/// draw-kind-4 arms build.
fn packed(i: &MoveInsn, a: usize) -> u32 {
    (i.arg(a) + (i.arg(a + 1) << 8) + (i.arg(a + 2) << 16)) as u32
}

/// The effect column: what the instruction stores, from its operands.
pub fn describe(i: &MoveInsn) -> String {
    let mut s = String::new();
    let writes = simple_writes(i.op);
    if !writes.is_empty() {
        for (n, &(off, a, shift, add)) in writes.iter().enumerate() {
            if n > 0 {
                s.push(' ');
            }
            let v = i.arg(a) << shift;
            let _ = write!(s, "+{off:#04X}{}{v}", if add { "+=" } else { "=" });
        }
        return s;
    }
    let _ = match i.op {
        0x03 => write!(s, "advance {} along heading +0x96", i.arg(1)),
        0x08 => write!(s, "+0x10 |= 8, end"),
        0x09 => write!(
            s,
            "+0x54={} ({} vsyncs at rate 8; ends the pass)",
            i.arg(1) << 3,
            i.arg(1)
        ),
        0x0A => {
            let _ = write!(s, "+0x6C={} lanes", i.arg(2));
            for k in 0..i.arg(2).max(0) as usize {
                let _ = write!(
                    s,
                    " [{} up {} dn {}]",
                    i.arg(3 + 3 * k),
                    i.arg(4 + 3 * k),
                    i.arg(5 + 3 * k)
                );
            }
            if i.uarg(1) == 0 {
                s.push_str(" reset weights");
            }
            Ok(())
        }
        0x0B => write!(s, "advance by the previous width"),
        0x0C => write!(
            s,
            "+0x74={:#010X} +0x78={}",
            ((i.arg(1) << 24) as u32 | 0x4000_0000)
                .wrapping_add(i.arg(2) as u32)
                .wrapping_add((i.arg(3) << 8) as u32)
                .wrapping_add((i.arg(4) << 16) as u32),
            i.arg(5)
        ),
        0x13 => write!(
            s,
            "+0x56=4 +0x5A=2 +0x9E={:#06X} +0x9C={} +0xC8={} +0xA0={:#08X} +0xA4={:#08X} +0xB4..+0xBE={},{},{},{},{},{}",
            i.uarg(1),
            i.arg(2),
            i.arg(3) << 3,
            packed(i, 4),
            packed(i, 7),
            i.arg(10),
            i.arg(11),
            i.arg(12),
            i.arg(13),
            i.arg(14),
            i.arg(15)
        ),
        0x16 => write!(s, "FUN_80024C80({}) (bare jr ra)", i.arg(1)),
        0x17 => write!(s, "FUN_801F30C4 burst mode {}", i.arg(1)),
        0x18 | 0x1A => write!(
            s,
            "count {}{}",
            i.uarg(1) & !LOOP_FOREVER,
            if i.uarg(1) & LOOP_FOREVER != 0 {
                " forever"
            } else {
                ""
            }
        ),
        0x19 | 0x1B => write!(s, "back to the open + 2"),
        0x1D => write!(s, "DAT_8007B6DE cue {:#06X}", i.uarg(1)),
        0x1E => write!(
            s,
            "+0x5A=4 +0xC4={} +0xCC..+0xD6={},{},{},{},{},{}",
            i.arg(1),
            i.arg(2),
            i.arg(3),
            i.arg(4),
            i.arg(5),
            i.arg(6),
            i.arg(7)
        ),
        0x1F => write!(
            s,
            "+0x9E|={:#06X} +0xB0={} +0xB2={} +0xA8..+0xAE={},{},{},{}",
            i.uarg(1),
            i.arg(2),
            i.arg(3),
            i.arg(4),
            i.arg(5),
            i.arg(6),
            i.arg(7)
        ),
        0x20 => write!(s, "(*gp[+0x714])(actor, {}, {})", i.arg(1), i.arg(2)),
        0x21 => write!(
            s,
            "row {} = {},{},{},{} flags {}",
            i.arg(1),
            i.arg(2),
            i.arg(3),
            i.arg(4),
            i.arg(5),
            i.arg(6)
        ),
        0x22 => write!(s, "advance 1"),
        0x23 => write!(
            s,
            "+0x56=4 +0x5A=2 +0x9E={:#06X} +0xA0={:#08X} +0xB4={} +0xB6={} tpage +0xB0={:#06X} clut +0xB2={:#06X} uv +0xA8..+0xAE={},{},{},{}",
            i.uarg(1) | 0x4000,
            packed(i, 2),
            i.arg(5),
            i.arg(6),
            i.uarg(7),
            i.uarg(8),
            i.arg(9),
            i.arg(10),
            i.arg(11),
            i.arg(12)
        ),
        0x25 => write!(s, "seat prescript record {}", i.arg(1)),
        0x2C => write!(
            s,
            "capture rect ({}, {}) {}x{}",
            i.arg(1),
            i.arg(2),
            i.arg(3),
            i.arg(4)
        ),
        0x2F => return describe_ext(i),
        0x30 => write!(s, "release the 0x2C buffer"),
        0x31 => write!(s, "+0x62 &= {:#06X}", i.uarg(1)),
        0x32 => write!(s, "+0x62 |= {:#06X}", i.uarg(1)),
        0x33 => write!(s, "+0x74 &= ~0x40000000"),
        0x34 => write!(
            s,
            "cue {} voice {} levels {},{} box {},{},{},{}",
            i.arg(1),
            i.arg(2),
            i.arg(3),
            i.arg(4),
            i.arg(5),
            i.arg(6),
            i.arg(7),
            i.arg(8)
        ),
        0x3A => write!(s, "+0x10 |= 2"),
        0x3B => write!(s, "+0x10 &= ~2"),
        0x3C => write!(s, "{} parts, +0x5A=6", i.arg(1)),
        0x3D => write!(s, "rate +0xD0={} {} parts", i.arg(1), i.arg(2)),
        0x40 => write!(
            s,
            "VRAM ({}, {}) {}x{} -> ({}, {})",
            i.arg(1),
            i.arg(2),
            i.arg(3),
            i.arg(4),
            i.arg(5),
            i.arg(6)
        ),
        0x42 => write!(
            s,
            "+0x56=4 +0x5A=2 +0x9E={:#06X} +0x9C={} +0xC8={} +0xB4..+0xBA={},{},{},{} +0xA8={} +0xA0={:#08X} +0xA4={:#08X}",
            i.uarg(1) | 0x2000,
            i.arg(2),
            i.arg(3),
            i.arg(4),
            i.arg(5),
            i.arg(6),
            i.arg(7),
            i.arg(8),
            packed(i, 9),
            packed(i, 12)
        ),
        0x43 => write!(s, "+0x86 |= 0x2000"),
        0x45 => write!(
            s,
            "+0x5A=7 +0xC4={} +0xCC={} +0xCE={} +0xD4={} +0xD6={} +0xD0={} +0xD2={}",
            i.arg(1),
            i.arg(2),
            i.arg(3),
            i.arg(4),
            i.arg(5),
            i.arg(6),
            i.arg(7)
        ),
        0x46 => write!(
            s,
            "tween +0x90/+0x92 to {},{} over {}",
            i.arg(1),
            i.arg(2),
            i.arg(3)
        ),
        _ => Ok(()),
    };
    s
}

fn describe_ext(i: &MoveInsn) -> String {
    let sub = i.sub.unwrap_or(0xFFFF);
    let a = |k: usize| i.arg(k);
    let branch = |cond: String| match i.flow {
        Flow::Branch { target: Some(t) } => format!("{cond} -> +{t:#X}"),
        _ => format!("{cond} -> (below buffer)"),
    };
    match sub {
        0x05 => format!("word[pc + {} + 5] = {} + rand % {}", a(4), a(2), a(3)),
        0x06 => branch(format!(
            "player outside ({}, {})..({}, {})",
            a(2),
            a(3),
            a(4),
            a(5)
        )),
        0x07 => branch(format!(
            "player inside ({}, {})..({}, {})",
            a(2),
            a(3),
            a(4),
            a(5)
        )),
        0x0A => branch("DAT_801F22F4 set".to_string()),
        0x0B => branch("DAT_801F22F4 clear".to_string()),
        0x0C => format!("+0x50={}", a(2)),
        0x0D => format!("+0x50+={}", a(2)),
        0x13 => branch(format!("flag {} set", a(2))),
        0x14 => branch(format!("flag {} clear", a(2))),
        0x1C => format!("flag {} set", a(2)),
        0x1D => format!("flag {} clear", a(2)),
        0x1E => format!("word[pc + {} + 4] += {}", a(2), a(3)),
        0x21 => format!("+0x3C..+0x40 += {},{},{}", a(2), a(3), a(4)),
        0x22 => format!("world += {},{},{}", a(2), a(3), a(4)),
        0x23 => format!("vel toward ({}, {}, {}) over {}", a(2), a(3), a(4), a(5)),
        0x25 | 0x26 => format!("slot {}", a(2)),
        0x29 => format!("slot {} ramp to {} over {}", a(2), -a(3), a(4)),
        0x2B => format!("strip +0xB4..+0xBA={},{},{},{}", a(2), a(3), a(4), a(5)),
        0x2D => format!("strip +0xB4..+0xBA+={},{},{},{}", a(2), a(3), a(4), a(5)),
        0x2F => format!("_DAT_8007B9D8={}", a(2)),
        0x30 => format!("word[pc + {} + 5] = {} or {}", a(4), a(2), a(3)),
        0x33 => format!("+0xC0..+0xC6+={},{},{},{}", a(2), a(3), a(4), a(5)),
        0x36..=0x39 => branch(format!("arg {}", a(2))),
        0x3B => format!("party member {}", a(2)),
        s if s >= MOVE_EXT_COUNT => "out of range: advance 1".to_string(),
        _ => i.words[2.min(i.words.len())..]
            .iter()
            .map(|w| (*w as i16).to_string())
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// One listing line: address, raw words, mnemonic and effect.
pub fn format_insn(i: &MoveInsn, addr: Addressing) -> String {
    let raw: Vec<String> = i.words.iter().map(|w| format!("{w:04X}")).collect();
    let raw = if raw.len() > 8 {
        format!("{} ..", raw[..8].join(" "))
    } else {
        raw.join(" ")
    };
    format!(
        "{}  {:<42} {:<20} {}",
        addr.at(i.offset),
        raw,
        i.mnemonic(),
        describe(i)
    )
}

/// A record listing: the header line, then one line per instruction.
pub fn format_record(r: &MoveRecord, addr: Addressing) -> String {
    let mut s = format!(
        "{}  record model_sel={} reserved={:#06X}\n",
        addr.at(r.offset),
        r.model_sel,
        r.reserved
    );
    for i in &r.program {
        s.push_str("  ");
        s.push_str(&format_insn(i, addr));
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests;
