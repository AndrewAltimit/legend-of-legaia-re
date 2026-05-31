//! Field-VM bytecode disassembler.
//!
//! Walks a field-VM bytecode buffer (the per-frame opcode stream consumed by
//! the field VM's `step` loop) and yields one [`Insn`] per source-encoded
//! instruction. The decoder mirrors the *width* logic of the field VM's `step`
//!   - it only computes how many bytes each instruction occupies plus a
//!     mnemonic, never executing host calls or mutating ctx state.
//!
//! This is a side-effect-free width/format decoder for the script bytecode, so
//! it lives in the Track-1 asset crate alongside the other format parsers; the
//! engine's executing field VM (`legaia_engine_vm::field::step`) re-uses the
//! same width logic and re-exports this module.
//!
//! For control-flow instructions (jumps, conditional jumps, BBOX tests),
//! the decoder always emits the **encoded** byte length, so a linear walk
//! traverses the script body exactly once. Branch / jump targets are
//! surfaced via the [`InsnInfo`] discriminator for callers that want to
//! follow control flow.
//!
//! For sub-dispatched opcodes (`0x4C`, `0x43`, `0x49`, `0x45`, `0x4E`,
//! `0x34`) where a particular sub-op variant isn't yet ported in the engine's
//! `field::step`, the decoder returns [`DisasmError::UnknownSubOp`]. Callers
//! typically print a `.byte` line for the leading byte and resume one byte
//! later.
//!
//! ## Why not call `step` directly?
//!
//! The engine's `field::step` interleaves width computation with side effects on
//! `ctx` and the `FieldHost` trait, and several opcodes return `StepResult`
//! variants (`Halt`, `Yield`) that don't carry the encoded width. A separate
//! width decoder keeps the disassembler side-effect-free and lets us produce
//! a stable encoded-width answer for every opcode the VM understands.
//!
//! ## Cross-context dispatch
//!
//! When the leading byte's high bit is set, the next byte is a target
//! script ID. Width math accounts for the 2-byte header in those cases;
//! the `extended` field on [`Insn`] surfaces the target ID for callers.

use std::fmt;

/// Length of one variable-length text/data packet, in bytes.
///
/// Ported from `FUN_8003CA38` (see `ghidra/scripts/funcs/8003ca38.txt`). The
/// in-game text encoding terminates a packet with any byte `<= 0x1E`. Bytes
/// `>= 0x1F` are normal payload; bytes whose top nibble is `0xC` are 2-byte
/// escape sequences (the second byte is consumed unconditionally).
///
/// The returned count does **not** include the terminator byte itself, so the
/// input `[0x40, 0x40, 0x00, ...]` yields `2`. On exhaustion it returns the
/// consumed length (matching the original's walk-off-end behaviour). It is the
/// dialog/text packet-width helper the disassembler uses for the `0x4C nE`
/// text-balloon op; the executing field VM re-exports it from here.
pub fn packet_length(buf: &[u8]) -> usize {
    let mut count = 0usize;
    let mut i = 0usize;
    while i < buf.len() {
        let b = buf[i];
        if b <= 0x1E {
            break;
        }
        if (b & 0xF0) == 0xC0 {
            // Escape pair - consume one extra byte and credit it to the count.
            i += 1;
            count += 1;
            if i >= buf.len() {
                break;
            }
        }
        count += 1;
        i += 1;
    }
    count
}

/// A decoded instruction.
#[derive(Debug, Clone)]
pub struct Insn {
    /// Byte offset where the instruction starts.
    pub pc: usize,
    /// Total number of bytes consumed (header + operands).
    pub size: usize,
    /// Raw opcode byte (0x80 bit cleared if `extended` is `Some`).
    pub opcode: u8,
    /// Cross-context target script ID, when bit 0x80 was set on the leading byte.
    pub extended: Option<u8>,
    /// Decoded mnemonic + structured payload.
    pub info: InsnInfo,
}

/// Structured payload for each opcode the disassembler recognises.
///
/// Variants carry the operand fields the printer needs for a readable
/// rendering. Variants are intentionally conservative - they hold the
/// operand bytes the VM actually consumes, with extra opcode-specific
/// detail only where it adds value (e.g. FMV index, jump target).
#[derive(Debug, Clone)]
pub enum InsnInfo {
    /// `0x21 / 0x24 / 0x25 / 0x48` no-op cluster.
    Nop,
    /// `0x22 EXEC_MOVE` - schedule move-table playback.
    ExecMove { move_id: u8 },
    /// `0x23 MOVE_TO` - teleport to grid (xb, zb).
    MoveTo { xb: u8, zb: u8 },
    /// `0x26 JMP_REL` - relative jump. `target` is the absolute byte offset.
    JmpRel { delta: u16, target: usize },
    /// Local-flag set/clear/test.
    LFlag { kind: FlagKind, bit: u8 },
    /// Global-flag (story flag) set/clear/test.
    GFlag { kind: FlagKind, bit: u8 },
    /// Context-flag set/clear/test (`ctx.flags`).
    CFlag { kind: FlagKind, bit: u8 },
    /// `0x35 BGM` - 4-byte instruction.
    Bgm { text_id: u16, sub_op: u8 },
    /// `0x37 / 0x41 / 0x47` yield variants.
    Yield { kind: YieldKind },
    /// `0x38 CAM_CFG` - 3-byte camera config.
    CamCfg { op0: u8, op1: u8 },
    /// `0x39 GIVE_ITEM` — add one of inline item `item_id` to the inventory
    /// (`FUN_8004313C` window setup + `FUN_800421D4(item_id, 1)`; dispatcher
    /// `FUN_801DE840` case `0x39`). This is the treasure-chest item-give op; the
    /// earlier `PLAY_SFX` label was wrong (SFX cues go through `FUN_80035B50`).
    GiveItem { item_id: u8 },
    /// `0x3A ADD_MONEY` - 24-bit signed delta.
    AddMoney { signed_24: i32 },
    /// `0x3B SET_ITEM_COUNT`.
    SetItemCount { slot: u8, count: u8 },
    /// `0x3C PARTY_ADD`.
    PartyAdd { char_id: u8 },
    /// `0x3D PARTY_REMOVE`.
    PartyRemove { char_id: u8 },
    /// `0x42 COND_JMP` - conditional jump on flags / screen mode. `target`
    /// is the absolute byte offset of the jump destination when taken.
    CondJmp {
        mode: u8,
        op1: u8,
        delta: u16,
        target: usize,
    },
    /// `0x3E` - WARP (op0 >= 100) / INTERACT.
    WarpOrInteract { op0: u8, op1: u8, is_warp: bool },
    /// `0x46 RENDER_CFG`.
    RenderCfg {
        long: bool,
        op0: u8,
        bytes: [u8; 5],
        used: usize,
    },
    /// `0x4F SCENE_REGISTER_WRITE`.
    SceneRegisterWrite { b0: u8, b1: u8, b2: u8 },
    /// `0x44 COUNTER`.
    Counter { op0: u8 },
    /// `0x4B ANIMATE` - variable-length keyframe block.
    Animate { count: u8, base_id: u8 },
    /// `0x36 SCENE_FADE`.
    SceneFade { word0: u16, word1: u16 },
    /// `0x45 CAMERA` - sub-dispatched.
    Camera { op0: u8, kind: CameraKind },
    /// `0x4D BBOX_TEST` - inside-the-box → next instr; outside → forward jump.
    BBoxTest {
        x_min: u8,
        z_min: u8,
        x_max: u8,
        z_max: u8,
        skip_delta: u16,
        skip_target: usize,
    },
    /// `0x3F` — **named scene-change** ("warp by name").
    ///
    /// Copies a length-prefixed destination scene *name* from the bytecode and
    /// hands it to the scene-change packet (`FUN_8001FD44`, which writes the
    /// name into the active scene-name buffers `0x8007050C`/`0x80084548`), then
    /// sets the destination entry tile + facing. Operand layout (after the
    /// opcode byte): `[i16 index][u8 name_len][name_len name bytes][entry_x]`
    /// `[entry_z][dir]`, so the instruction is `header + 6 + name_len` bytes.
    ///
    /// The destination name is a *slice of the bytecode* at `operand + 3`, not
    /// carried inline here; recover it with [`scene_change_name`]. This is
    /// **not** a dialog opcode — field dialogue is the `0x4C` nibble-5 sub-3/4
    /// path ([`MenuCtrlKind::Nibble5Dialog`]); only the over-approximating walk
    /// desyncing on a literal `?` (`0x3F`) in message text makes it *look* like
    /// one in text-heavy records.
    SceneChange {
        /// Sign-extended `i16` at `operand[0..2]` — the destination's
        /// story/entry index (`FUN_8003CE9C` read). Not the 7-id door-warp
        /// `map_id`; distinct id space (observed values reach 155+).
        index: i16,
        /// Length of the inline destination-name string at `operand + 3`.
        name_len: u8,
        /// Entry tile X byte (`& 0x7F` = tile, `& 0x80` = +half-tile).
        entry_x: u8,
        /// Entry tile Z byte (same encoding as `entry_x`).
        entry_z: u8,
        /// Facing/depth selector (`& 7` indexes the entry-direction table).
        dir: u8,
    },
    /// `0x40 DATA_BLOCK`.
    DataBlock { len: u8 },
    /// `0x4A WAIT_FRAMES`.
    WaitFrames { target: u16 },
    /// `0x4E` inventory comparison-and-jump (sub-dispatched).
    InventoryCmp {
        page: u8,
        mode_byte: u8,
        kind: InventoryCmpKind,
    },
    /// `0x49 STATE_RESUME` (sub-dispatched).
    StateResume { sub_op: u8, kind: StateResumeKind },
    /// `0x34` EFFECT (sub-dispatched).
    Effect { op0: u8, kind: EffectKind },
    /// `0x43` ACTOR_CTRL (sub-dispatched).
    ActorCtrl { sub_op: u8, kind: ActorCtrlKind },
    /// `0x4C` MENU_CTRL (sub-dispatched).
    MenuCtrl { op0: u8, kind: MenuCtrlKind },
    /// `0x5x / 0x6x / 0x7x` system flag set/clear/test (4-bank flags).
    SystemFlag {
        kind: FlagKind,
        idx: u16,
        delta: Option<u16>,
        target: Option<usize>,
    },
    /// One opaque byte. Used as a fallback when the decoder cannot make
    /// sense of the leading byte; the caller resumes one byte later.
    Byte { value: u8 },
}

/// Kind discriminator for set / clear / test flag opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    Set,
    Clear,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YieldKind {
    /// `0x37 / 0x41` yields - resume at `pc + header + 2`.
    Standard,
    /// `0x47` yield - resume at `pc + header + 3`.
    Wide,
}

#[derive(Debug, Clone)]
pub enum CameraKind {
    Load,
    Save,
    Apply { abs_target: usize },
    Configure { mask: u16, apply_trigger: u16 },
}

#[derive(Debug, Clone)]
pub enum InventoryCmpKind {
    /// Sub-op 0/1 - paginated 7-byte form.
    Compare {
        sub_op: u8,
        arg: u16,
        skip_delta: u16,
        skip_target: usize,
    },
    /// Sub-op 2/3/5/6/7/8/9 - absolute jump.
    AbsJump { target: usize },
    /// Sub-op 4 - BIOS rand → next-PC.
    BiosRandJump,
    /// Sub-op 10/11 - 9-byte party-bank comparison.
    PartyBank {
        sub_op: u8,
        scaled: i32,
        skip_delta: u16,
        skip_target: usize,
    },
    /// Sub-op 12..=15 - dispatcher's default arm (PC += 7).
    DefaultAdvance,
}

#[derive(Debug, Clone)]
pub enum StateResumeKind {
    /// Done arm, sub-op 0 - inline MES bytecode walker.
    DoneSub0Mes { length: u8, mes_bytes: usize },
    /// Done arm, sub-ops 1/3/7 (PC += 3).
    DoneSubShort,
    /// Done arm, sub-ops 2/4 (PC += 7).
    DoneSubMid,
    /// Done arm, sub-op 5 (PC += 14).
    DoneSubLong,
    /// Done arm, sub-ops 6/8/9/0xC/0xD (PC += 4).
    DoneSubAdvance4,
    /// Idle / Armed arm - the decoder doesn't try to track runtime state,
    /// so it always reports the encoded sub-op width.
    Encoded { sub_op: u8 },
}

#[derive(Debug, Clone)]
pub enum EffectKind {
    /// Sub-0: 7-byte color + intensity setup.
    ColorIntensity { rgb: [u8; 3], intensity: i16 },
    /// Sub-1: 13-byte spawn (or 13 + 2 + payload_len with capture flag).
    Spawn { has_capture: bool, payload_len: u8 },
    /// Sub-2: 3-byte capture-yield.
    CaptureYield { b1: u8 },
    /// Sub-3: 2-byte 3D anim trigger.
    AnimTrigger { arg: u8 },
}

#[derive(Debug, Clone)]
pub enum ActorCtrlKind {
    /// Halt-acquire dispatcher (sub-0 / sub-1: 5 bytes, sub-A / sub-B: 9 bytes).
    HaltAcquire { sub_op: u8, target_offset_pc: usize },
    /// Sub-2: 8-byte 3-actor talk.
    ThreeActorTalk {
        actors: [u8; 3],
        arg_word: u16,
        b6: u8,
    },
    /// Sub-3..=6: 10-byte sound register ramp.
    SoundRegisterRamp {
        sub_op: u8,
        bytes: [u8; 4],
        ticks: u16,
        curve: u16,
    },
    /// Sub-7: 17-byte face / body rotation setup.
    FaceRotation { face_id: u8, target: i16 },
    /// Sub-8: 2-byte face reset.
    FaceReset,
    /// Sub-9: 10-byte tween / immediate position write.
    Sub9Tween { x: u16, y: u16, z: u16, ticks: u16 },
    /// Sub-C: 5-byte allocate scripted actor.
    AllocScripted { b: [u8; 3] },
    /// Sub-D / Sub-F: 6-byte allocate actor with mode.
    AllocActorMode { sub_op: u8, b: [u8; 4] },
    /// Sub-E: 2-byte mark currently-iterating actor flag bit 8.
    MarkActorFlag8,
    /// Sub-0x10: 21-byte emitter init.
    EmitterInit,
    /// Sub-0x11: 12-byte 5-word emitter call.
    Emitter5Words { words: [u16; 5] },
    /// Sub-0x12 / Sub-0x13: 14-byte emitter calls.
    EmitterPayload14,
    /// Sub-0x14: 10-byte 4-word emitter call.
    Emitter4Words { words: [i16; 4] },
    /// Sub-0x15: 14-byte emitter struct.
    EmitterStruct12,
}

#[derive(Debug, Clone)]
pub enum MenuCtrlKind {
    /// Outer nibble 0: party leader change.
    PartyLeader { leader_id: u8 },
    /// Outer nibble 1: 7-byte menu/effect dispatcher.
    Menu1 { payload: [u8; 5] },
    /// Outer nibble 2: party view swap (op0 & 7).
    PartyViewSwap { op0_low3: u8 },
    /// Outer nibble 3: per-bit sub-ops; mostly 2-byte simple writes.
    Nibble3 { sub: u8 },
    /// Outer nibble 4: 6-byte immediate-or-ramp cluster (or 11-byte sub-5).
    Nibble4 { sub: u8, target: i16, ticks: u16 },
    /// Outer nibble 4 sub-5 - wider 11-byte form.
    Nibble4Sub5 {
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
        ticks: u16,
    },
    /// Outer nibble 5 sub-0 - 4-byte sound-directional.
    Nibble5Sub0 { value: i16 },
    /// Outer nibble 5 sub-1 - 6-byte NPC / player move-to-tile with run
    /// dispatch. Operand bytes are `[x_enc, z_enc, depth, move_id]`; tile
    /// coords use the same `(byte & 0x7F)<<7 + (bit7 ? 0x80 : 0x40)` decode
    /// the placement header uses, and the `is_player` selector reads
    /// `ctx.flags & 0x0100_0000`. Drives the step handler's
    /// `op4c_n5_sub1_npc_run` host hook.
    Nibble5NpcRun {
        x_enc: u8,
        z_enc: u8,
        depth: u8,
        move_id: u8,
    },
    /// Outer nibble 5 sub-2 - 3-byte menu-activation poll. Operand byte is the
    /// `menu_id`; the script halts at PC until the host's
    /// `op4c_n5_sub2_menu_activation` hook returns true.
    Nibble5MenuPoll { menu_id: u8 },
    /// Outer nibble 5 sub-3 / sub-4 - 2-byte dialog poll.
    Nibble5Dialog { sub: u8 },
    /// Outer nibble 6 sub-0 - 14-byte 6-word emitter call.
    Nibble6Emitter6 { words: [i16; 6] },
    /// Outer nibble 7 - 7-byte VRAM tile-flag bulk operation.
    Nibble7Tile {
        sub: u8,
        x0: u8,
        z0: u8,
        x1: u8,
        z1: u8,
        mask: u8,
    },
    /// Outer nibble 8 - heterogeneous sub-ops.
    Nibble8 { sub: u8, encoded_size: usize },
    /// Outer nibble 0xA / 0xB / 0xC / 0xD / 0xE / 0xF - heterogeneous.
    HighNibble {
        outer: u8,
        sub: u8,
        encoded_size: usize,
    },
    /// Outer nibble 0xE sub-2 - **FMV trigger** (the 0x4C 0xE2 op).
    FmvTrigger { fmv_id: i16 },
}

/// Why the decoder couldn't decode an instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisasmError {
    /// PC is past the end of the buffer.
    EndOfStream { pc: usize },
    /// The opcode byte is recognised but the operand bytes lie past the
    /// buffer end.
    Truncated { pc: usize, opcode: u8 },
    /// A sub-op variant the decoder doesn't know how to size. Caller
    /// typically prints a `.byte` line and resumes one byte later.
    UnknownSubOp { pc: usize, opcode: u8, sub_op: u8 },
    /// Top-level opcode falls outside the field VM's documented range.
    UnknownOpcode { pc: usize, opcode: u8 },
}

impl fmt::Display for DisasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndOfStream { pc } => write!(f, "end of stream at pc=0x{pc:04X}"),
            Self::Truncated { pc, opcode } => write!(
                f,
                "truncated operand for opcode 0x{opcode:02X} at pc=0x{pc:04X}"
            ),
            Self::UnknownSubOp { pc, opcode, sub_op } => write!(
                f,
                "unknown sub-op 0x{sub_op:02X} for opcode 0x{opcode:02X} at pc=0x{pc:04X}"
            ),
            Self::UnknownOpcode { pc, opcode } => {
                write!(f, "unknown opcode 0x{opcode:02X} at pc=0x{pc:04X}")
            }
        }
    }
}

impl std::error::Error for DisasmError {}

/// Decode a single instruction starting at `pc`. Returns the instruction
/// and the byte offset of the next instruction.
///
/// On error the caller chooses recovery - the typical strategy is to
/// emit a `.byte 0xNN` for `bytecode[pc]` and resume at `pc + 1`.
pub fn decode(bytecode: &[u8], pc: usize) -> Result<Insn, DisasmError> {
    let &lead = bytecode.get(pc).ok_or(DisasmError::EndOfStream { pc })?;
    let extended_bit = lead & 0x80 != 0;
    let opcode = lead & 0x7F;
    let header_size = if extended_bit { 2 } else { 1 };
    let extended = if extended_bit {
        Some(
            *bytecode
                .get(pc + 1)
                .ok_or(DisasmError::Truncated { pc, opcode })?,
        )
    } else {
        None
    };
    let operand = pc + header_size;

    let need = |min: usize| -> Result<(), DisasmError> {
        if operand + min > bytecode.len() {
            Err(DisasmError::Truncated { pc, opcode })
        } else {
            Ok(())
        }
    };

    let mk = |size: usize, info: InsnInfo| -> Result<Insn, DisasmError> {
        Ok(Insn {
            pc,
            size,
            opcode,
            extended,
            info,
        })
    };

    match opcode {
        0x21 | 0x24 | 0x25 | 0x48 => mk(header_size, InsnInfo::Nop),
        0x22 => {
            need(1)?;
            mk(
                header_size + 1,
                InsnInfo::ExecMove {
                    move_id: bytecode[operand],
                },
            )
        }
        0x23 => {
            need(2)?;
            mk(
                header_size + 2,
                InsnInfo::MoveTo {
                    xb: bytecode[operand],
                    zb: bytecode[operand + 1],
                },
            )
        }
        0x26 => {
            need(2)?;
            let delta = u16::from_le_bytes([bytecode[operand], bytecode[operand + 1]]);
            let target = (pc + header_size).wrapping_add(delta as usize);
            mk(header_size + 2, InsnInfo::JmpRel { delta, target })
        }
        0x2B..=0x2D => {
            need(1)?;
            let kind = match opcode {
                0x2B => FlagKind::Set,
                0x2C => FlagKind::Clear,
                _ => FlagKind::Test,
            };
            mk(
                header_size + 1,
                InsnInfo::LFlag {
                    kind,
                    bit: bytecode[operand] & 0x1F,
                },
            )
        }
        0x2E..=0x30 => {
            need(1)?;
            let kind = match opcode {
                0x2E => FlagKind::Set,
                0x2F => FlagKind::Clear,
                _ => FlagKind::Test,
            };
            mk(
                header_size + 1,
                InsnInfo::GFlag {
                    kind,
                    bit: bytecode[operand] & 0x1F,
                },
            )
        }
        0x31..=0x33 => {
            need(1)?;
            let kind = match opcode {
                0x31 => FlagKind::Set,
                0x32 => FlagKind::Clear,
                _ => FlagKind::Test,
            };
            mk(
                header_size + 1,
                InsnInfo::CFlag {
                    kind,
                    bit: bytecode[operand] & 0x1F,
                },
            )
        }
        0x34 => {
            need(1)?;
            let op0 = bytecode[operand];
            let sub = op0 >> 4;
            match sub {
                0 => {
                    need(6)?;
                    mk(
                        header_size + 6,
                        InsnInfo::Effect {
                            op0,
                            kind: EffectKind::ColorIntensity {
                                rgb: [
                                    bytecode[operand + 1],
                                    bytecode[operand + 2],
                                    bytecode[operand + 3],
                                ],
                                intensity: i16::from_le_bytes([
                                    bytecode[operand + 4],
                                    bytecode[operand + 5],
                                ]),
                            },
                        },
                    )
                }
                1 => {
                    // Base 13 bytes; if capture_flag at +12 == 0x40, +2 + payload_len.
                    need(12)?;
                    let capture_flag = bytecode.get(operand + 12).copied().unwrap_or(0);
                    let (extra, payload_len, has_capture) = if capture_flag == 0x40 {
                        let len = bytecode.get(operand + 13).copied().unwrap_or(0);
                        (2 + len as usize, len, true)
                    } else {
                        (0, 0, false)
                    };
                    mk(
                        header_size + 12 + extra,
                        InsnInfo::Effect {
                            op0,
                            kind: EffectKind::Spawn {
                                has_capture,
                                payload_len,
                            },
                        },
                    )
                }
                2 => {
                    need(2)?;
                    mk(
                        header_size + 2,
                        InsnInfo::Effect {
                            op0,
                            kind: EffectKind::CaptureYield {
                                b1: bytecode[operand + 1],
                            },
                        },
                    )
                }
                3 => {
                    need(2)?;
                    mk(
                        header_size + 2,
                        InsnInfo::Effect {
                            op0,
                            kind: EffectKind::AnimTrigger {
                                arg: bytecode[operand + 1],
                            },
                        },
                    )
                }
                _ => Err(DisasmError::UnknownSubOp {
                    pc,
                    opcode,
                    sub_op: op0,
                }),
            }
        }
        0x35 => {
            need(3)?;
            let text_id = u16::from_le_bytes([bytecode[operand], bytecode[operand + 1]]);
            mk(
                header_size + 3,
                InsnInfo::Bgm {
                    text_id,
                    sub_op: bytecode[operand + 2],
                },
            )
        }
        0x36 => {
            need(4)?;
            let word0 = u16::from_le_bytes([bytecode[operand], bytecode[operand + 1]]);
            let word1 = u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
            mk(header_size + 4, InsnInfo::SceneFade { word0, word1 })
        }
        0x37 | 0x41 => mk(
            header_size + 2,
            InsnInfo::Yield {
                kind: YieldKind::Standard,
            },
        ),
        0x47 => mk(
            header_size + 3,
            InsnInfo::Yield {
                kind: YieldKind::Wide,
            },
        ),
        0x38 => {
            need(2)?;
            mk(
                header_size + 2,
                InsnInfo::CamCfg {
                    op0: bytecode[operand],
                    op1: bytecode[operand + 1],
                },
            )
        }
        0x39 => {
            need(1)?;
            mk(
                header_size + 1,
                InsnInfo::GiveItem {
                    item_id: bytecode[operand],
                },
            )
        }
        0x3A => {
            need(3)?;
            let raw = u32::from(bytecode[operand])
                | (u32::from(bytecode[operand + 1]) << 8)
                | (u32::from(bytecode[operand + 2]) << 16);
            let signed = if raw & 0x80_0000 != 0 {
                (raw | 0xFF00_0000) as i32
            } else {
                raw as i32
            };
            mk(header_size + 3, InsnInfo::AddMoney { signed_24: signed })
        }
        0x3B => {
            need(2)?;
            mk(
                header_size + 2,
                InsnInfo::SetItemCount {
                    slot: bytecode[operand],
                    count: bytecode[operand + 1],
                },
            )
        }
        0x3C => {
            need(1)?;
            mk(
                header_size + 1,
                InsnInfo::PartyAdd {
                    char_id: bytecode[operand],
                },
            )
        }
        0x3D => {
            need(1)?;
            mk(
                header_size + 1,
                InsnInfo::PartyRemove {
                    char_id: bytecode[operand],
                },
            )
        }
        0x3E => {
            need(1)?;
            let op0 = bytecode[operand];
            let is_warp = !(op0 == 0xFF || op0 < 100);
            if is_warp {
                need(5)?;
                mk(
                    header_size + 5,
                    InsnInfo::WarpOrInteract {
                        op0,
                        op1: 0,
                        is_warp: true,
                    },
                )
            } else {
                need(2)?;
                mk(
                    header_size + 2,
                    InsnInfo::WarpOrInteract {
                        op0,
                        op1: bytecode[operand + 1],
                        is_warp: false,
                    },
                )
            }
        }
        0x3F => {
            need(3)?;
            let index = i16::from_le_bytes([bytecode[operand], bytecode[operand + 1]]);
            let name_len = bytecode[operand + 2];
            let len_usize = name_len as usize;
            need(3 + len_usize + 3)?;
            let pos_start = operand + 3 + len_usize;
            mk(
                header_size + 6 + len_usize,
                InsnInfo::SceneChange {
                    index,
                    name_len,
                    entry_x: bytecode[pos_start],
                    entry_z: bytecode[pos_start + 1],
                    dir: bytecode[pos_start + 2],
                },
            )
        }
        0x40 => {
            need(1)?;
            let len = bytecode[operand];
            need(1 + len as usize)?;
            mk(header_size + 1 + len as usize, InsnInfo::DataBlock { len })
        }
        0x42 => {
            need(4)?;
            let mode = bytecode[operand];
            let op1 = bytecode[operand + 1];
            let delta = u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
            let target = (pc + header_size + 2).wrapping_add(delta as usize);
            mk(
                header_size + 4,
                InsnInfo::CondJmp {
                    mode,
                    op1,
                    delta,
                    target,
                },
            )
        }
        0x43 => decode_actor_ctrl(bytecode, pc, header_size, operand, opcode),
        0x44 => {
            need(1)?;
            mk(
                header_size + 1,
                InsnInfo::Counter {
                    op0: bytecode[operand],
                },
            )
        }
        0x45 => decode_camera(bytecode, pc, header_size, operand, opcode),
        0x46 => {
            need(1)?;
            let op0 = bytecode[operand];
            if op0 == 0x24 {
                need(5)?;
                mk(
                    header_size + 5,
                    InsnInfo::RenderCfg {
                        long: true,
                        op0,
                        bytes: [
                            bytecode[operand + 1],
                            bytecode[operand + 2],
                            bytecode[operand + 3],
                            bytecode[operand + 4],
                            0,
                        ],
                        used: 4,
                    },
                )
            } else {
                need(2)?;
                mk(
                    header_size + 2,
                    InsnInfo::RenderCfg {
                        long: false,
                        op0,
                        bytes: [bytecode[operand + 1], 0, 0, 0, 0],
                        used: 1,
                    },
                )
            }
        }
        0x49 => decode_state_resume(bytecode, pc, header_size, operand, opcode),
        0x4A => {
            need(2)?;
            let target = u16::from_le_bytes([bytecode[operand], bytecode[operand + 1]]);
            mk(header_size + 2, InsnInfo::WaitFrames { target })
        }
        0x4B => {
            need(2)?;
            let count = bytecode[operand];
            let base_id = bytecode[operand + 1];
            let frame_bytes = (count as usize) * 4;
            need(2 + frame_bytes)?;
            mk(
                header_size + 2 + frame_bytes,
                InsnInfo::Animate { count, base_id },
            )
        }
        0x4C => decode_menu_ctrl(bytecode, pc, header_size, operand, opcode),
        0x4D => {
            need(6)?;
            let skip_delta = u16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
            let skip_target = (pc + header_size + 4).wrapping_add(skip_delta as usize);
            mk(
                header_size + 6,
                InsnInfo::BBoxTest {
                    x_min: bytecode[operand],
                    z_min: bytecode[operand + 1],
                    x_max: bytecode[operand + 2],
                    z_max: bytecode[operand + 3],
                    skip_delta,
                    skip_target,
                },
            )
        }
        0x4E => decode_inventory_cmp(bytecode, pc, header_size, operand, opcode),
        0x4F => {
            need(3)?;
            mk(
                header_size + 3,
                InsnInfo::SceneRegisterWrite {
                    b0: bytecode[operand],
                    b1: bytecode[operand + 1],
                    b2: bytecode[operand + 2],
                },
            )
        }
        // System-flag bank: 0x5x SET, 0x6x CLEAR, 0x7x TEST.
        0x50..=0x77 => {
            need(1)?;
            let route = opcode & 0x70;
            let idx = (u16::from(lead & 0x8F) << 8) | u16::from(bytecode[operand]);
            let kind = match route {
                0x50 => FlagKind::Set,
                0x60 => FlagKind::Clear,
                0x70 => FlagKind::Test,
                _ => unreachable!(),
            };
            if route == 0x70 {
                need(3)?;
                let delta = u16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                let target = (pc + header_size + 1).wrapping_add(delta as usize);
                mk(
                    header_size + 3,
                    InsnInfo::SystemFlag {
                        kind,
                        idx,
                        delta: Some(delta),
                        target: Some(target),
                    },
                )
            } else {
                mk(
                    header_size + 1,
                    InsnInfo::SystemFlag {
                        kind,
                        idx,
                        delta: None,
                        target: None,
                    },
                )
            }
        }
        _ => Err(DisasmError::UnknownOpcode { pc, opcode }),
    }
}

fn decode_actor_ctrl(
    bytecode: &[u8],
    pc: usize,
    header_size: usize,
    operand: usize,
    opcode: u8,
) -> Result<Insn, DisasmError> {
    let need = |min: usize| -> Result<(), DisasmError> {
        if operand + min > bytecode.len() {
            Err(DisasmError::Truncated { pc, opcode })
        } else {
            Ok(())
        }
    };
    need(1)?;
    let sub_op = bytecode[operand];
    let mk = |size: usize, kind: ActorCtrlKind| -> Result<Insn, DisasmError> {
        Ok(Insn {
            pc,
            size,
            opcode,
            extended: None,
            info: InsnInfo::ActorCtrl { sub_op, kind },
        })
    };
    match sub_op {
        0 | 1 => {
            need(4)?;
            let target_offset_pc = operand + 3;
            mk(
                header_size + 4,
                ActorCtrlKind::HaltAcquire {
                    sub_op,
                    target_offset_pc,
                },
            )
        }
        0xA | 0xB => {
            need(8)?;
            let target_offset_pc = operand + 7;
            mk(
                header_size + 8,
                ActorCtrlKind::HaltAcquire {
                    sub_op,
                    target_offset_pc,
                },
            )
        }
        2 => {
            need(7)?;
            let arg_word = u16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
            mk(
                header_size + 7,
                ActorCtrlKind::ThreeActorTalk {
                    actors: [
                        bytecode[operand + 1],
                        bytecode[operand + 2],
                        bytecode[operand + 3],
                    ],
                    arg_word,
                    b6: bytecode[operand + 6],
                },
            )
        }
        3..=6 => {
            need(9)?;
            let bytes = [
                bytecode[operand + 1],
                bytecode[operand + 2],
                bytecode[operand + 3],
                bytecode[operand + 4],
            ];
            let ticks = u16::from_le_bytes([bytecode[operand + 5], bytecode[operand + 6]]);
            let curve = u16::from_le_bytes([bytecode[operand + 7], bytecode[operand + 8]]);
            mk(
                header_size + 9,
                ActorCtrlKind::SoundRegisterRamp {
                    sub_op,
                    bytes,
                    ticks,
                    curve,
                },
            )
        }
        7 => {
            need(16)?;
            let face_id = bytecode[operand + 1];
            let target =
                u16::from_le_bytes([bytecode[operand + 14], bytecode[operand + 15]]) as i16;
            mk(
                header_size + 16,
                ActorCtrlKind::FaceRotation { face_id, target },
            )
        }
        8 => mk(header_size + 1, ActorCtrlKind::FaceReset),
        9 => {
            need(9)?;
            let x = u16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let y = u16::from_le_bytes([bytecode[operand + 3], bytecode[operand + 4]]);
            let z = u16::from_le_bytes([bytecode[operand + 5], bytecode[operand + 6]]);
            let ticks = u16::from_le_bytes([bytecode[operand + 7], bytecode[operand + 8]]);
            mk(header_size + 9, ActorCtrlKind::Sub9Tween { x, y, z, ticks })
        }
        0xC => {
            need(4)?;
            mk(
                header_size + 4,
                ActorCtrlKind::AllocScripted {
                    b: [
                        bytecode[operand + 1],
                        bytecode[operand + 2],
                        bytecode[operand + 3],
                    ],
                },
            )
        }
        0xD | 0xF => {
            need(5)?;
            mk(
                header_size + 5,
                ActorCtrlKind::AllocActorMode {
                    sub_op,
                    b: [
                        bytecode[operand + 1],
                        bytecode[operand + 2],
                        bytecode[operand + 3],
                        bytecode[operand + 4],
                    ],
                },
            )
        }
        0xE => mk(header_size + 1, ActorCtrlKind::MarkActorFlag8),
        0x10 => {
            need(20)?;
            mk(header_size + 20, ActorCtrlKind::EmitterInit)
        }
        0x11 => {
            need(11)?;
            let mut words = [0u16; 5];
            for (i, w) in words.iter_mut().enumerate() {
                *w = u16::from_le_bytes([
                    bytecode[operand + 1 + i * 2],
                    bytecode[operand + 2 + i * 2],
                ]);
            }
            mk(header_size + 11, ActorCtrlKind::Emitter5Words { words })
        }
        0x12 | 0x13 => {
            need(13)?;
            mk(header_size + 13, ActorCtrlKind::EmitterPayload14)
        }
        0x14 => {
            need(9)?;
            let mut words = [0i16; 4];
            for (i, w) in words.iter_mut().enumerate() {
                *w = u16::from_le_bytes([
                    bytecode[operand + 1 + i * 2],
                    bytecode[operand + 2 + i * 2],
                ]) as i16;
            }
            mk(header_size + 9, ActorCtrlKind::Emitter4Words { words })
        }
        0x15 => {
            need(13)?;
            mk(header_size + 13, ActorCtrlKind::EmitterStruct12)
        }
        _ => Err(DisasmError::UnknownSubOp { pc, opcode, sub_op }),
    }
}

fn decode_camera(
    bytecode: &[u8],
    pc: usize,
    header_size: usize,
    operand: usize,
    opcode: u8,
) -> Result<Insn, DisasmError> {
    let need = |min: usize| -> Result<(), DisasmError> {
        if operand + min > bytecode.len() {
            Err(DisasmError::Truncated { pc, opcode })
        } else {
            Ok(())
        }
    };
    need(1)?;
    let op0 = bytecode[operand];
    let mk = |size: usize, kind: CameraKind| -> Result<Insn, DisasmError> {
        Ok(Insn {
            pc,
            size,
            opcode,
            extended: None,
            info: InsnInfo::Camera { op0, kind },
        })
    };
    match op0 & 0xC0 {
        0x40 => {
            need(19)?;
            mk(header_size + 19, CameraKind::Load)
        }
        0x80 => mk(header_size + 1, CameraKind::Save),
        0xC0 => {
            need(3)?;
            let abs_target =
                u16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]) as usize;
            mk(header_size + 3, CameraKind::Apply { abs_target })
        }
        0x00 => {
            need(4)?;
            let op1 = bytecode[operand + 1];
            let apply_trigger = u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
            let mask = (u16::from(op0) << 8) | u16::from(op1);
            // Two bytes per set bit, slots 0..10.
            let mut set_count = 0usize;
            for slot in 0u8..10 {
                let bit = 1u16 << (9 - slot);
                if mask & bit != 0 {
                    set_count += 1;
                }
            }
            let consumed = 4 + 2 * set_count;
            need(consumed)?;
            mk(
                header_size + consumed,
                CameraKind::Configure {
                    mask,
                    apply_trigger,
                },
            )
        }
        _ => unreachable!(),
    }
}

fn decode_state_resume(
    bytecode: &[u8],
    pc: usize,
    header_size: usize,
    operand: usize,
    opcode: u8,
) -> Result<Insn, DisasmError> {
    let need = |min: usize| -> Result<(), DisasmError> {
        if operand + min > bytecode.len() {
            Err(DisasmError::Truncated { pc, opcode })
        } else {
            Ok(())
        }
    };
    need(1)?;
    let sub_op = bytecode[operand];
    let mk = |size: usize, kind: StateResumeKind| -> Result<Insn, DisasmError> {
        Ok(Insn {
            pc,
            size,
            opcode,
            extended: None,
            info: InsnInfo::StateResume { sub_op, kind },
        })
    };
    // The decoder reports the encoded-Done-arm width; control flow through
    // Idle / Armed states is irrelevant for linear disassembly.
    match sub_op {
        0 => {
            // [49, 0, length, ...length args..., ...mes_bytes]
            need(2)?;
            let length = bytecode[operand + 1];
            let mes_start = operand + 2 + length as usize;
            if mes_start > bytecode.len() {
                return Err(DisasmError::Truncated { pc, opcode });
            }
            let mes_bytes = walk_mes_bytecode(&bytecode[mes_start..]);
            // size = header + 4 + length + mes_bytes per the step impl
            // (advance = pc + header_size + 4 + length + mes_bytes).
            mk(
                header_size + 4 + length as usize + mes_bytes,
                StateResumeKind::DoneSub0Mes { length, mes_bytes },
            )
        }
        1 | 3 | 7 => mk(header_size + 2, StateResumeKind::DoneSubShort),
        2 | 4 => mk(header_size + 6, StateResumeKind::DoneSubMid),
        5 => mk(header_size + 13, StateResumeKind::DoneSubLong),
        6 | 8 | 9 | 0xC | 0xD => mk(header_size + 4, StateResumeKind::DoneSubAdvance4),
        _ => Err(DisasmError::UnknownSubOp { pc, opcode, sub_op }),
    }
}

fn decode_inventory_cmp(
    bytecode: &[u8],
    pc: usize,
    header_size: usize,
    operand: usize,
    opcode: u8,
) -> Result<Insn, DisasmError> {
    let need = |min: usize| -> Result<(), DisasmError> {
        if operand + min > bytecode.len() {
            Err(DisasmError::Truncated { pc, opcode })
        } else {
            Ok(())
        }
    };
    need(2)?;
    let page = bytecode[operand];
    let mode_byte = bytecode[operand + 1];
    let sub_op = mode_byte >> 4;
    let mk = |size: usize, kind: InventoryCmpKind| -> Result<Insn, DisasmError> {
        Ok(Insn {
            pc,
            size,
            opcode,
            extended: None,
            info: InsnInfo::InventoryCmp {
                page,
                mode_byte,
                kind,
            },
        })
    };
    match sub_op {
        0 | 1 => {
            need(6)?;
            let arg = u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
            let skip_delta = u16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
            let skip_target = (pc + header_size + 4).wrapping_add(skip_delta as usize);
            mk(
                header_size + 6,
                InventoryCmpKind::Compare {
                    sub_op,
                    arg,
                    skip_delta,
                    skip_target,
                },
            )
        }
        2 | 3 | 5 | 6 | 7 | 8 | 9 => {
            need(4)?;
            let target =
                u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]) as usize;
            mk(header_size + 4, InventoryCmpKind::AbsJump { target })
        }
        4 => mk(header_size + 2, InventoryCmpKind::BiosRandJump),
        10 | 11 => {
            need(8)?;
            let lo1 = u16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]) as u32;
            let lo2 = u16::from_le_bytes([bytecode[operand + 6], bytecode[operand + 7]]) as u32;
            let scaled = (lo1 | (lo2 << 16)) as i32;
            let skip_delta = u16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
            let skip_target = (pc + header_size + 4).wrapping_add(skip_delta as usize);
            mk(
                header_size + 8,
                InventoryCmpKind::PartyBank {
                    sub_op,
                    scaled,
                    skip_delta,
                    skip_target,
                },
            )
        }
        12..=15 => {
            need(6)?;
            mk(header_size + 6, InventoryCmpKind::DefaultAdvance)
        }
        _ => unreachable!("mode_byte >> 4 is at most 0xF"),
    }
}

fn decode_menu_ctrl(
    bytecode: &[u8],
    pc: usize,
    header_size: usize,
    operand: usize,
    opcode: u8,
) -> Result<Insn, DisasmError> {
    let need = |min: usize| -> Result<(), DisasmError> {
        if operand + min > bytecode.len() {
            Err(DisasmError::Truncated { pc, opcode })
        } else {
            Ok(())
        }
    };
    need(1)?;
    let op0 = bytecode[operand];
    let mk = |size: usize, kind: MenuCtrlKind| -> Result<Insn, DisasmError> {
        Ok(Insn {
            pc,
            size,
            opcode,
            extended: None,
            info: InsnInfo::MenuCtrl { op0, kind },
        })
    };
    match op0 >> 4 {
        0 => mk(
            header_size + 1,
            MenuCtrlKind::PartyLeader { leader_id: op0 & 7 },
        ),
        1 => {
            need(6)?;
            let payload = [
                bytecode[operand + 1],
                bytecode[operand + 2],
                bytecode[operand + 3],
                bytecode[operand + 4],
                bytecode[operand + 5],
            ];
            mk(header_size + 6, MenuCtrlKind::Menu1 { payload })
        }
        2 => mk(
            header_size + 1,
            MenuCtrlKind::PartyViewSwap { op0_low3: op0 & 7 },
        ),
        3 => {
            // Sub-3 sub-ops are uniformly 2-byte (PC += header + 1).
            let sub = op0 & 0x0F;
            // Sub-ops 0..=0xF are all known and fall into the simple-write
            // bucket. We don't bother distinguishing them here - mnemonic
            // rendering can dispatch on `sub`.
            let _ = sub;
            mk(header_size + 1, MenuCtrlKind::Nibble3 { sub })
        }
        4 => decode_menu_nibble4(bytecode, pc, header_size, operand, opcode),
        5 => {
            let sub = op0 & 0x0F;
            match sub {
                0 => {
                    need(3)?;
                    let value = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                    mk(header_size + 3, MenuCtrlKind::Nibble5Sub0 { value })
                }
                // Sub-1: 6-byte NPC / player move-to-tile with run dispatch.
                // Mirrors the step handler's `[4C, 0x51, x_enc, z_enc, depth,
                // move_id]` encoding (`crates/engine-vm/src/field/step.rs`).
                // Common in placement records — without this arm the linear
                // walker desyncs across every dialog NPC's `[story-flag,
                // move-to-tile, JmpRel]` per-branch prologue.
                1 => {
                    need(5)?;
                    let x_enc = bytecode[operand + 1];
                    let z_enc = bytecode[operand + 2];
                    let depth = bytecode[operand + 3];
                    let move_id = bytecode[operand + 4];
                    mk(
                        header_size + 5,
                        MenuCtrlKind::Nibble5NpcRun {
                            x_enc,
                            z_enc,
                            depth,
                            move_id,
                        },
                    )
                }
                // Sub-2: 3-byte menu-activation poll `[4C, 0x52, menu_id]`.
                // The step handler halts at PC until the host returns
                // activated; for the disasm we just emit the 3-byte form.
                2 => {
                    need(2)?;
                    let menu_id = bytecode[operand + 1];
                    mk(header_size + 2, MenuCtrlKind::Nibble5MenuPoll { menu_id })
                }
                3 | 4 => mk(header_size + 1, MenuCtrlKind::Nibble5Dialog { sub }),
                _ => Err(DisasmError::UnknownSubOp {
                    pc,
                    opcode,
                    sub_op: op0,
                }),
            }
        }
        6 => {
            if op0 == 0x60 {
                need(13)?;
                let mut words = [0i16; 6];
                for (i, w) in words.iter_mut().enumerate() {
                    *w = i16::from_le_bytes([
                        bytecode[operand + 1 + i * 2],
                        bytecode[operand + 2 + i * 2],
                    ]);
                }
                mk(header_size + 13, MenuCtrlKind::Nibble6Emitter6 { words })
            } else {
                Err(DisasmError::UnknownSubOp {
                    pc,
                    opcode,
                    sub_op: op0,
                })
            }
        }
        7 => {
            need(6)?;
            let sub = op0 & 0x0F;
            mk(
                header_size + 6,
                MenuCtrlKind::Nibble7Tile {
                    sub,
                    x0: bytecode[operand + 1],
                    z0: bytecode[operand + 2],
                    x1: bytecode[operand + 3],
                    z1: bytecode[operand + 4],
                    mask: bytecode[operand + 5],
                },
            )
        }
        8 => {
            let sub = op0 & 0x0F;
            let encoded = match sub {
                1 => 9,
                2 | 4 | 0xC => 2,
                3 => 1, // sub-3: box-fill table halt-acquire (pending in step.rs); halts at PC
                5 | 0xE | 0xF => 1, // halt-acquire idiom; step.rs halts at PC, no operand bytes consumed
                6 => 15,
                7 => 1, // halt at PC; encoded width is 1
                8 => 5,
                9 => 3, // sub-9: 4-byte total `[4C, 0x89, lo, hi]`
                0xA => 10,
                0xB => 4, // sub-B: 5-byte `[4C, 0x8B, type_byte, target_lo, target_hi]`
                0xD => 5, // sub-D: 6-byte char-actor search
                _ => {
                    return Err(DisasmError::UnknownSubOp {
                        pc,
                        opcode,
                        sub_op: op0,
                    });
                }
            };
            need(encoded)?;
            mk(
                header_size + encoded,
                MenuCtrlKind::Nibble8 {
                    sub,
                    encoded_size: header_size + encoded,
                },
            )
        }
        0xE => {
            // 0x4C 0xE_ - the FMV trigger (sub-2) lives here. The width math
            // mirrors `step()`: most sub-ops are 6 bytes total (header +
            // 5), sub-3 is 2 bytes, sub-7 is 7 bytes, sub-8 is 10 bytes.
            let sub = op0 & 0x0F;
            match sub {
                0 => {
                    // Halt at PC; encoded width = 2 bytes (4C, E0, b1 - state-write).
                    need(2)?;
                    mk(
                        header_size + 2,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 2,
                        },
                    )
                }
                1 => {
                    // Variable-length text balloon: PC += 3 + packet_length.
                    need(1)?;
                    let payload_start = operand + 1;
                    let length = packet_length(&bytecode[payload_start..]);
                    let total = 2 + length;
                    need(total)?;
                    mk(
                        header_size + total,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + total,
                        },
                    )
                }
                2 => {
                    need(5)?;
                    let fmv_id = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
                    mk(header_size + 5, MenuCtrlKind::FmvTrigger { fmv_id })
                }
                3 => {
                    need(2)?;
                    mk(
                        header_size + 2,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 2,
                        },
                    )
                }
                6 => {
                    need(7)?;
                    mk(
                        header_size + 7,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 7,
                        },
                    )
                }
                7 => {
                    need(7)?;
                    mk(
                        header_size + 7,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 7,
                        },
                    )
                }
                8 => {
                    need(10)?;
                    mk(
                        header_size + 10,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 10,
                        },
                    )
                }
                0xC => {
                    need(2)?;
                    mk(
                        header_size + 2,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 2,
                        },
                    )
                }
                _ => Err(DisasmError::UnknownSubOp {
                    pc,
                    opcode,
                    sub_op: op0,
                }),
            }
        }
        outer @ (0xA | 0xB | 0xC | 0xD | 0xF) => Err(DisasmError::UnknownSubOp {
            pc,
            opcode,
            sub_op: op0 | (outer << 4),
        }),
        _ => Err(DisasmError::UnknownSubOp {
            pc,
            opcode,
            sub_op: op0,
        }),
    }
}

fn decode_menu_nibble4(
    bytecode: &[u8],
    pc: usize,
    header_size: usize,
    operand: usize,
    opcode: u8,
) -> Result<Insn, DisasmError> {
    let need = |min: usize| -> Result<(), DisasmError> {
        if operand + min > bytecode.len() {
            Err(DisasmError::Truncated { pc, opcode })
        } else {
            Ok(())
        }
    };
    let op0 = bytecode[operand];
    let sub = op0 & 0x0F;
    let mk = |size: usize, kind: MenuCtrlKind| -> Result<Insn, DisasmError> {
        Ok(Insn {
            pc,
            size,
            opcode,
            extended: None,
            info: InsnInfo::MenuCtrl { op0, kind },
        })
    };
    if sub == 5 {
        // 11-byte form.
        need(10)?;
        let b1 = bytecode[operand + 1];
        let w94 = i16::from_le_bytes([bytecode[operand + 2], bytecode[operand + 3]]);
        let w96 = i16::from_le_bytes([bytecode[operand + 4], bytecode[operand + 5]]);
        let w98 = i16::from_le_bytes([bytecode[operand + 6], bytecode[operand + 7]]);
        let ticks = u16::from_le_bytes([bytecode[operand + 8], bytecode[operand + 9]]);
        return mk(
            header_size + 10,
            MenuCtrlKind::Nibble4Sub5 {
                b1,
                w94,
                w96,
                w98,
                ticks,
            },
        );
    }
    // 6-byte form for sub-0..=4, 6, 7, 8, 9, A..D.
    match sub {
        0..=4 | 6..=0xD => {
            need(5)?;
            let target = i16::from_le_bytes([bytecode[operand + 1], bytecode[operand + 2]]);
            let ticks = u16::from_le_bytes([bytecode[operand + 3], bytecode[operand + 4]]);
            mk(
                header_size + 5,
                MenuCtrlKind::Nibble4 { sub, target, ticks },
            )
        }
        _ => Err(DisasmError::UnknownSubOp {
            pc,
            opcode,
            sub_op: op0,
        }),
    }
}

/// Walker mirroring the field VM's private `walk_mes_bytecode`.
/// Keeps running until a terminator (`<= 0x1E`) or end-of-buffer.
fn walk_mes_bytecode(buf: &[u8]) -> usize {
    let mut i = 0;
    while let Some(&b) = buf.get(i) {
        if b <= 0x1E {
            break;
        }
        if b & 0xF0 == 0xC0 {
            if buf.get(i + 1).is_none() {
                i += 1;
                break;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

/// Iterator that linearly walks `bytecode` starting at `start_pc`, yielding
/// one `Result<Insn, (usize, DisasmError)>` per source-encoded instruction.
///
/// On error the iterator advances by one byte and continues, so a single
/// truncated / unknown instruction doesn't kill the whole walk.
pub struct LinearWalker<'a> {
    bytecode: &'a [u8],
    pc: usize,
}

impl<'a> LinearWalker<'a> {
    pub fn new(bytecode: &'a [u8], start_pc: usize) -> Self {
        Self {
            bytecode,
            pc: start_pc,
        }
    }
}

impl Iterator for LinearWalker<'_> {
    type Item = Result<Insn, (usize, DisasmError)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pc >= self.bytecode.len() {
            return None;
        }
        match decode(self.bytecode, self.pc) {
            Ok(insn) => {
                let next = self.pc + insn.size.max(1);
                self.pc = next;
                Some(Ok(insn))
            }
            Err(err) => {
                let pc = self.pc;
                self.pc += 1;
                Some(Err((pc, err)))
            }
        }
    }
}

/// Render an instruction as a single text line.
///
/// Format (similar to MIPS objdump):
///
/// ```text
///   0x000A  4C E2 03 00 00 00       FmvTrigger fmv_id=3 (MV4.STR)
/// ```
pub fn format_instruction(insn: &Insn, bytecode: &[u8]) -> String {
    let mut bytes_str = String::new();
    let end = (insn.pc + insn.size).min(bytecode.len());
    for (i, b) in bytecode[insn.pc..end].iter().enumerate() {
        if i > 0 {
            bytes_str.push(' ');
        }
        bytes_str.push_str(&format!("{:02X}", b));
    }
    let mnemonic = render_mnemonic(insn);
    format!("  0x{:04X}  {:24}  {}", insn.pc, bytes_str, mnemonic)
}

fn render_mnemonic(insn: &Insn) -> String {
    use InsnInfo::*;
    let ext = if let Some(t) = insn.extended {
        format!("[ext target=0x{:02X}] ", t)
    } else {
        String::new()
    };
    let body = match &insn.info {
        Nop => "Nop".into(),
        ExecMove { move_id } => format!("ExecMove move_id={move_id}"),
        MoveTo { xb, zb } => format!("MoveTo xb=0x{xb:02X} zb=0x{zb:02X}"),
        JmpRel { delta, target } => {
            format!("JmpRel delta=0x{delta:04X} -> 0x{target:04X}")
        }
        LFlag { kind, bit } => format!("LFlag.{kind:?} bit={bit}"),
        GFlag { kind, bit } => format!("GFlag.{kind:?} bit={bit}"),
        CFlag { kind, bit } => format!("CFlag.{kind:?} bit={bit}"),
        Bgm { text_id, sub_op } => format!("Bgm text_id={text_id} sub={sub_op:#x}"),
        Yield { kind } => format!("Yield ({kind:?})"),
        CamCfg { op0, op1 } => format!("CamCfg op0=0x{op0:02X} op1=0x{op1:02X}"),
        GiveItem { item_id } => format!("GiveItem item_id={item_id}"),
        AddMoney { signed_24 } => format!("AddMoney delta={signed_24}"),
        SetItemCount { slot, count } => format!("SetItemCount slot={slot} count={count}"),
        PartyAdd { char_id } => format!("PartyAdd char_id={char_id}"),
        PartyRemove { char_id } => format!("PartyRemove char_id={char_id}"),
        CondJmp {
            mode,
            op1,
            delta,
            target,
        } => format!("CondJmp mode={mode} op1=0x{op1:02X} delta=0x{delta:04X} -> 0x{target:04X}"),
        WarpOrInteract { op0, op1, is_warp } => {
            if *is_warp {
                format!("Warp map_id={}", op0 - 100)
            } else {
                format!("Interact op0=0x{op0:02X} op1=0x{op1:02X}")
            }
        }
        RenderCfg { long, op0, .. } => format!(
            "RenderCfg {} op0=0x{:02X}",
            if *long { "long" } else { "short" },
            op0
        ),
        SceneRegisterWrite { b0, b1, b2 } => {
            format!("SceneRegisterWrite [{b0}, {b1}, {b2}]")
        }
        Counter { op0 } => format!("Counter op=0x{op0:02X}"),
        Animate { count, base_id } => format!("Animate count={count} base_id={base_id}"),
        SceneFade { word0, word1 } => {
            format!("SceneFade word0=0x{word0:04X} word1=0x{word1:04X}")
        }
        Camera { op0, kind } => format!("Camera op0=0x{op0:02X} {kind:?}"),
        BBoxTest {
            x_min,
            z_min,
            x_max,
            z_max,
            skip_target,
            ..
        } => format!("BBoxTest [{x_min},{z_min}..{x_max},{z_max}] skip-> 0x{skip_target:04X}"),
        SceneChange {
            index,
            name_len,
            entry_x,
            entry_z,
            ..
        } => format!(
            "SceneChange index={index} name_len={name_len} entry=(0x{entry_x:02X},0x{entry_z:02X})"
        ),
        DataBlock { len } => format!("DataBlock len={len}"),
        WaitFrames { target } => format!("WaitFrames target={target}"),
        InventoryCmp {
            page,
            mode_byte,
            kind,
        } => format!("InventoryCmp page={page} mode=0x{mode_byte:02X} {kind:?}"),
        StateResume { sub_op, kind } => format!("StateResume sub={sub_op:#x} {kind:?}"),
        Effect { op0, kind } => format!("Effect op0=0x{op0:02X} {kind:?}"),
        ActorCtrl { sub_op, kind } => format!("ActorCtrl sub={sub_op:#x} {kind:?}"),
        MenuCtrl { op0, kind } => match kind {
            MenuCtrlKind::FmvTrigger { fmv_id } => {
                let name = fmv_filename(*fmv_id);
                format!("FmvTrigger fmv_id={fmv_id} ({name})")
            }
            other => format!("MenuCtrl op0=0x{op0:02X} {other:?}"),
        },
        SystemFlag {
            kind, idx, target, ..
        } => match target {
            Some(t) => format!("SysFlag.{kind:?} idx=0x{idx:04X} -> 0x{t:04X}"),
            None => format!("SysFlag.{kind:?} idx=0x{idx:04X}"),
        },
        Byte { value } => format!(".byte 0x{value:02X}"),
    };
    format!("{ext}{body}")
}

/// Recover the destination scene name of a [`InsnInfo::SceneChange`] (`0x3F`)
/// instruction from the bytecode it was decoded against.
///
/// The name is a `name_len`-byte slice at `insn_start + header + 3` (header is
/// 2 for the `0x80` cross-context form, 1 otherwise). Returns `None` when `insn`
/// is not a `SceneChange`, the slice runs past `bytecode`, or the bytes aren't a
/// clean ASCII scene label — the same desync guard the `0x3E` warp gate uses:
/// the linear walk hits literal `?` (`0x3F`) bytes inside message text, so a
/// caller must reject names that aren't lowercase-ASCII-ish CDNAME labels.
/// Genuine destinations are short (`town01`, `dolk`, `rikuroa`, …).
pub fn scene_change_name(bytecode: &[u8], insn: &Insn) -> Option<String> {
    let InsnInfo::SceneChange { name_len, .. } = insn.info else {
        return None;
    };
    let header = if insn.extended.is_some() { 2 } else { 1 };
    let start = insn.pc + header + 3;
    let raw = bytecode.get(start..start + name_len as usize)?;
    clean_scene_name(raw)
}

/// The clean-CDNAME-label gate shared by [`scene_change_name`] and the field-VM
/// `0x3F` executor. A genuine destination name is short, non-empty, and a
/// lowercase-ASCII / digit CDNAME label (`town01`, `dolk`, `rikuroa`, …).
/// Rejects anything else — the desync guard for a literal `?` (`0x3F`) landing
/// inside message text, which would otherwise decode a bogus "name". Returns the
/// owned name on success.
pub fn clean_scene_name(raw: &[u8]) -> Option<String> {
    if raw.is_empty()
        || raw.len() > 12
        || !raw
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return None;
    }
    Some(String::from_utf8_lossy(raw).into_owned())
}

/// Map a retail FMV index to its filename via the runtime FMV-state
/// table at `0x801D0A6C`. The retail mapping skips `MV2.STR` and
/// `MV5.STR` (disc-resident but not referenced by any FMV slot) and
/// reaches them via `MV3.STR` segments instead. Slots `5..=11`
/// reference cut paths.
pub fn fmv_filename(fmv_id: i16) -> &'static str {
    match fmv_id {
        0 => "MV1.STR",
        1 => "MV3.STR",
        2 => "MV3.STR", // second segment of MV3 (different start sector)
        3 => "MV4.STR",
        4 => "MV6.STR",
        5 => "(cut: MOV15.STR)",
        6..=11 => "(cut: MOV.STR)",
        _ => "(unknown)",
    }
}

/// Convenience: scan a script body for every `0x4C 0xE2` FMV trigger and
/// return the decoded `(pc, fmv_id)` pairs. Useful for the per-scene MV
/// index lift in the cutscene-table workflow.
pub fn find_fmv_triggers(bytecode: &[u8]) -> Vec<(usize, i16)> {
    let mut out = Vec::new();
    for r in LinearWalker::new(bytecode, 0) {
        if let Ok(insn) = r
            && let InsnInfo::MenuCtrl {
                kind: MenuCtrlKind::FmvTrigger { fmv_id },
                ..
            } = insn.info
        {
            out.push((insn.pc, fmv_id));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_length_empty_buffer_is_zero() {
        assert_eq!(packet_length(&[]), 0);
    }

    #[test]
    fn packet_length_immediate_terminator_is_zero() {
        // Any byte <= 0x1E ends the packet without contributing.
        for b in 0..=0x1Eu8 {
            assert_eq!(packet_length(&[b]), 0, "terminator byte {b:#x}");
        }
    }

    #[test]
    fn packet_length_pure_printable_run() {
        let buf = [0x20, 0x40, 0x80, 0xBF, 0x00];
        assert_eq!(packet_length(&buf), 4);
    }

    #[test]
    fn packet_length_escape_sequence_counts_two() {
        // 0xC1 is an escape lead - the next byte is consumed unconditionally.
        let buf = [0xC1, 0xAB, 0x00];
        assert_eq!(packet_length(&buf), 2);
    }

    #[test]
    fn packet_length_multiple_escapes_with_runs() {
        let buf = [0x40, 0xC1, 0xAB, 0x40, 0xCD, 0x05, 0x00];
        assert_eq!(packet_length(&buf), 6);
    }

    #[test]
    fn packet_length_escape_at_buffer_boundary() {
        // 0xC2 with no following byte: we guard and stop; the lead still counts.
        assert_eq!(packet_length(&[0xC2]), 1);
    }

    #[test]
    fn packet_length_no_terminator_runs_to_end() {
        assert_eq!(packet_length(&[0x20, 0x21, 0x22, 0x23]), 4);
    }

    #[test]
    fn packet_length_high_nibble_matters_for_escape() {
        assert_eq!(packet_length(&[0xC0, 0xFF, 0x00]), 2);
        assert_eq!(packet_length(&[0xD0, 0xFF, 0x00]), 2);
        for lead in 0xC0..=0xCFu8 {
            assert_eq!(
                packet_length(&[lead, 0xAB, 0x00]),
                2,
                "escape lead {lead:#x}"
            );
        }
        for lead in [0xB0u8, 0xBFu8, 0xD0u8, 0xDFu8] {
            assert_eq!(packet_length(&[lead, 0x00]), 1, "non-escape lead {lead:#x}");
        }
    }

    #[test]
    fn nop_decodes() {
        let insn = decode(&[0x21], 0).unwrap();
        assert_eq!(insn.size, 1);
        assert!(matches!(insn.info, InsnInfo::Nop));
    }

    #[test]
    fn extended_bit_skips_target_id_byte() {
        // 0xA1 = 0x21 | 0x80 (extended Nop).
        let insn = decode(&[0xA1, 0x07], 0).unwrap();
        assert_eq!(insn.size, 2);
        assert_eq!(insn.extended, Some(0x07));
        assert_eq!(insn.opcode, 0x21);
    }

    #[test]
    fn fmv_trigger_decodes_fmv_id_and_total_size_six_bytes() {
        // [4C, E2, 03, 00, _, _]
        let bc = [0x4C, 0xE2, 0x03, 0x00, 0x00, 0x00];
        let insn = decode(&bc, 0).unwrap();
        assert_eq!(insn.size, 6);
        match insn.info {
            InsnInfo::MenuCtrl {
                op0: 0xE2,
                kind: MenuCtrlKind::FmvTrigger { fmv_id },
            } => assert_eq!(fmv_id, 3),
            other => panic!("unexpected info: {other:?}"),
        }
    }

    #[test]
    fn fmv_trigger_negative_index() {
        let bc = [0x4C, 0xE2, 0xFF, 0xFF, 0x00, 0x00];
        let insn = decode(&bc, 0).unwrap();
        match insn.info {
            InsnInfo::MenuCtrl {
                kind: MenuCtrlKind::FmvTrigger { fmv_id },
                ..
            } => assert_eq!(fmv_id, -1),
            _ => panic!("expected FmvTrigger"),
        }
    }

    #[test]
    fn jmp_rel_target_is_post_header() {
        // 0x26 + LE u16 0x0008. Target = pc + header_size + delta = 0 + 1 + 8 = 9.
        let bc = [0x26, 0x08, 0x00, 0xAA, 0xBB];
        let insn = decode(&bc, 0).unwrap();
        match insn.info {
            InsnInfo::JmpRel { delta, target } => {
                assert_eq!(delta, 0x0008);
                assert_eq!(target, 9);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn animate_consumes_count_times_four_extra_bytes() {
        // count = 2, 4 * 2 = 8 keyframe bytes; total = 1 (op) + 1 (count) + 1 (base) + 8 = 11.
        let mut bc = vec![0x4Bu8, 2, 0xFF];
        bc.extend_from_slice(&[0x11; 8]);
        let insn = decode(&bc, 0).unwrap();
        assert_eq!(insn.size, 11);
    }

    #[test]
    fn data_block_consumes_len_bytes() {
        // len = 5: total = 1 (op) + 1 (len) + 5 (payload) = 7.
        let bc = [0x40u8, 5, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
        let insn = decode(&bc, 0).unwrap();
        assert_eq!(insn.size, 7);
    }

    #[test]
    fn cond_jmp_always_advances_by_five() {
        let bc = [0x42u8, 0, 7, 0x10, 0x00];
        let insn = decode(&bc, 0).unwrap();
        match insn.info {
            InsnInfo::CondJmp { delta, target, .. } => {
                assert_eq!(delta, 0x0010);
                // target = pc + header_size + 2 + delta = 0 + 1 + 2 + 16 = 19.
                assert_eq!(target, 19);
            }
            _ => panic!(),
        }
        assert_eq!(insn.size, 5);
    }

    #[test]
    fn bbox_test_skip_target_offset() {
        // operand bytes: x_min=1, z_min=2, x_max=3, z_max=4, skip=0x0007.
        let bc = [0x4Du8, 1, 2, 3, 4, 0x07, 0x00];
        let insn = decode(&bc, 0).unwrap();
        match insn.info {
            InsnInfo::BBoxTest {
                skip_delta,
                skip_target,
                ..
            } => {
                assert_eq!(skip_delta, 7);
                // skip_target = pc + header + 4 + delta = 0 + 1 + 4 + 7 = 12.
                assert_eq!(skip_target, 12);
            }
            _ => panic!(),
        }
        assert_eq!(insn.size, 7);
    }

    #[test]
    fn warp_uses_six_bytes_when_op0_ge_100() {
        let bc = [0x3Eu8, 100, 0, 0, 0, 0];
        let insn = decode(&bc, 0).unwrap();
        assert_eq!(insn.size, 6);
        match insn.info {
            InsnInfo::WarpOrInteract { is_warp, .. } => assert!(is_warp),
            _ => panic!(),
        }
    }

    #[test]
    fn interact_uses_three_bytes_when_op0_under_100() {
        let bc = [0x3Eu8, 5, 0xAB];
        let insn = decode(&bc, 0).unwrap();
        assert_eq!(insn.size, 3);
        match insn.info {
            InsnInfo::WarpOrInteract { is_warp, .. } => assert!(!is_warp),
            _ => panic!(),
        }
    }

    #[test]
    fn linear_walker_yields_three_instructions_then_stops() {
        // [22, 5] ExecMove; [21] Nop; [37, 0, 0] Yield.
        let bc = [0x22u8, 5, 0x21, 0x37, 0, 0];
        let walked: Vec<_> = LinearWalker::new(&bc, 0).collect();
        assert_eq!(walked.len(), 3);
        assert!(walked.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn linear_walker_recovers_from_unknown_sub_op() {
        // 0x4C 0xFF: outer nibble 0xF has no decoder (HighNibble unknown).
        // Walker should emit the error then advance one byte.
        let bc = [0x4Cu8, 0xFF, 0x21];
        let mut walked = LinearWalker::new(&bc, 0);
        let first = walked.next().unwrap();
        assert!(first.is_err(), "first instruction should error");
        // Walker advanced 1 byte; next instruction is at pc=1 (still inside 0x4C/0xFF/0x21).
        let second = walked.next().unwrap();
        match second {
            Ok(_) | Err(_) => {}
        }
    }

    #[test]
    fn fmv_trigger_search_finds_all() {
        // Build a script with 3 FMV triggers.
        let mut bc = Vec::new();
        // Trigger 0: MV1.STR at pc=0
        bc.extend_from_slice(&[0x4C, 0xE2, 0, 0, 0, 0]);
        // Some unrelated nop
        bc.push(0x21);
        // Trigger 4: MV6.STR
        bc.extend_from_slice(&[0x4C, 0xE2, 4, 0, 0, 0]);
        // Trigger 3: MV4.STR
        bc.extend_from_slice(&[0x4C, 0xE2, 3, 0, 0, 0]);
        let triggers = find_fmv_triggers(&bc);
        assert_eq!(triggers.len(), 3);
        assert_eq!(triggers[0], (0, 0));
        assert_eq!(triggers[1].1, 4);
        assert_eq!(triggers[2].1, 3);
    }

    #[test]
    fn fmv_filename_known_indices() {
        assert_eq!(fmv_filename(0), "MV1.STR");
        assert_eq!(fmv_filename(1), "MV3.STR");
        assert_eq!(fmv_filename(4), "MV6.STR");
        assert_eq!(fmv_filename(5), "(cut: MOV15.STR)");
        assert_eq!(fmv_filename(99), "(unknown)");
    }

    #[test]
    fn scene_change_consumes_inline_payload_and_recovers_name() {
        // [3F, index_lo, index_hi, name_len=4, 'd','o','l','k', entry_x, entry_z, dir].
        let bc = [0x3Fu8, 60, 0, 4, b'd', b'o', b'l', b'k', 0x10, 0x20, 0x30];
        let insn = decode(&bc, 0).unwrap();
        assert_eq!(insn.size, 11); // header 1 + 6 + name_len 4
        match insn.info {
            InsnInfo::SceneChange {
                index,
                name_len,
                entry_x,
                entry_z,
                dir,
            } => {
                assert_eq!(index, 60);
                assert_eq!(name_len, 4);
                assert_eq!(entry_x, 0x10);
                assert_eq!(entry_z, 0x20);
                assert_eq!(dir, 0x30);
            }
            other => panic!("expected SceneChange, got {other:?}"),
        }
        // The destination name is recovered from the bytecode slice.
        assert_eq!(scene_change_name(&bc, &insn).as_deref(), Some("dolk"));
    }

    #[test]
    fn scene_change_name_rejects_text_desync_phantom() {
        // A 0x3F whose "name" bytes are uppercase/punctuation (a literal '?'
        // landing inside message text) is not a clean CDNAME label.
        let bc = [0x3Fu8, 0, 0, 4, b'H', b'i', b'!', b' ', 0x00, 0x00, 0x00];
        let insn = decode(&bc, 0).unwrap();
        assert_eq!(scene_change_name(&bc, &insn), None);
    }

    #[test]
    fn system_flag_test_includes_target() {
        // Opcode 0x70, idx low byte 5, jump 0x000A.
        let bc = [0x70u8, 5, 0x0A, 0x00];
        let insn = decode(&bc, 0).unwrap();
        match insn.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                target,
                ..
            } => assert_eq!(target.unwrap(), (1 + 1) + 0x0A),
            _ => panic!(),
        }
    }

    #[test]
    fn truncated_returns_error_at_pc() {
        let bc = [0x22u8]; // ExecMove with no operand byte.
        let err = decode(&bc, 0).unwrap_err();
        assert!(matches!(err, DisasmError::Truncated { pc: 0, .. }));
    }

    #[test]
    fn format_instruction_includes_byte_dump_and_mnemonic() {
        let bc = [0x4Cu8, 0xE2, 0x03, 0x00, 0x00, 0x00];
        let insn = decode(&bc, 0).unwrap();
        let line = format_instruction(&insn, &bc);
        assert!(line.contains("0x0000"));
        assert!(line.contains("4C E2 03 00 00 00"));
        assert!(line.contains("FmvTrigger"));
        assert!(line.contains("MV4.STR"));
    }
}
