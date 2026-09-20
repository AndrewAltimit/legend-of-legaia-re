//! Single-instruction decoder: turns the bytes at a program counter into one
//! decoded instruction plus the offset of the next instruction.

use super::*;
use decode_subops::walk_mes_bytecode;

/// Resolve a relative-jump target. Retail keeps each script's PC in a 16-bit
/// field (`*(short *)(ctx + 0x9e)`), so a branch lands at
/// `(base + delta) mod 0x10000` and a delta with the high bit set is a
/// **backward** jump - `0xFFFE` = -2 is the per-frame "park here" wait loop
/// (`[21] [26 FE FF]`; see `docs/subsystems/script-vm.md`). Decoding works in
/// buffer-absolute offsets and a record's script never spans 32 KiB, so the
/// sign-extended delta is that wrap expressed in absolute coordinates; a
/// plain unsigned add turned every wait loop into a `+0xFFxx` overrun past
/// the buffer.
pub(crate) fn rel_target(base: usize, delta: u16) -> usize {
    base.wrapping_add(delta as i16 as isize as usize)
}

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

    let decoded = match opcode {
        // Inline text segment (bare form only - the field corpus stores the
        // lead without the cross-context prefix). Size covers the lead, the
        // glyphs and the terminator byte, which must be present.
        0x1F if extended.is_none() => {
            let len = walk_mes_bytecode(&bytecode[operand.min(bytecode.len())..]);
            need(len + 1)?;
            mk(header_size + len + 1, InsnInfo::TextSegment { len })
        }
        0x21 | 0x24 | 0x25 | 0x48 => mk(header_size, InsnInfo::Nop),
        // Picker open byte + N x i16 jump table. Each entry's delta is
        // relative to the entry's own offset (`FUN_80038050`, mirrored by
        // `legaia_mes::picker`); the continuation byte and the option labels
        // that follow the table are ordinary stream elements.
        0x27..=0x2A if extended.is_none() => {
            let count: u8 = match opcode {
                0x27 | 0x2A => 2,
                0x28 => 3,
                _ => 4,
            };
            need(count as usize * 2)?;
            let mut deltas = [0i16; 4];
            let mut targets = [0usize; 4];
            for i in 0..count as usize {
                let base = operand + i * 2;
                let d = i16::from_le_bytes([bytecode[base], bytecode[base + 1]]);
                deltas[i] = d;
                targets[i] = rel_target(base, d as u16);
            }
            mk(
                header_size + count as usize * 2,
                InsnInfo::Picker {
                    count,
                    deltas,
                    targets,
                },
            )
        }
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
            let target = rel_target(pc + header_size, delta);
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
                    // Two bytes (`[34, 2N]`): the fall-through advance is
                    // `PC += 2` (port `op_34`, `code_r0x801df098`). `b1` is
                    // the byte *after* the op, peeked, never consumed: when it
                    // is `0x40` the arm captures the forward PC for the actor
                    // - and that `0x40` is the `DATA_BLOCK` op carrying the
                    // captured payload, which the dispatcher skips on its own
                    // once the capture has yielded. Consuming it here made
                    // the walk land inside the data block's bytes.
                    mk(
                        header_size + 1,
                        InsnInfo::Effect {
                            op0,
                            kind: EffectKind::CaptureYield {
                                b1: bytecode.get(operand + 1).copied().unwrap_or(0),
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
            let target = rel_target(pc + header_size + 2, delta);
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
                InsnInfo::SpawnRecord {
                    global_index: bytecode[operand],
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
                    InsnInfo::ViewWindow {
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
                    InsnInfo::ViewWindow {
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
            let skip_target = rel_target(pc + header_size + 4, skip_delta);
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
        // System-flag bank: 0x5x SET, 0x6x CLEAR, 0x7x TEST. The VM routes the
        // whole `0x50..=0x7F` range by `opcode & 0x70` (and masks the flag index
        // with `0x8F`), so high-index flags reach TEST opcodes `0x78..=0x7F` -
        // covered here too, not just `0x70..=0x77`.
        0x50..=0x7F => {
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
                let target = rel_target(pc + header_size + 1, delta);
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
    };
    // The sub-dispatched decoders (0x34 / 0x43 / 0x45 / 0x49 / 0x4C / 0x4E)
    // construct their own `Insn` without seeing the lead byte's `0x80`
    // cross-context target; re-attach it here so `Insn::extended` is
    // populated uniformly across every opcode family.
    let mut insn = decoded?;
    insn.extended = extended;
    Ok(insn)
}

/// Offset of the `0x1F` lead of the text an instruction carries, if it carries
/// one: the [`InsnInfo::TextSegment`] pseudo-op itself, a `0x49` sub-0 state
/// resume whose inline MES starts with a lead, or a `0x4C` nibble-E sub-1
/// text balloon whose packet does. This is what makes a `0x1F <text> 0x00`
/// run *dialog* rather than a coincidence: the same framing occurs inside
/// instruction operands (an actor index `0x1F` followed by two printable
/// bytes and a zero), and only a clean walk can tell the two apart.
pub fn text_lead(bytecode: &[u8], insn: &Insn) -> Option<usize> {
    let operand = insn.pc + if insn.extended.is_some() { 2 } else { 1 };
    let lead_at = |at: usize| (bytecode.get(at) == Some(&0x1F)).then_some(at);
    match &insn.info {
        InsnInfo::TextSegment { .. } => Some(insn.pc),
        InsnInfo::StateResume {
            sub_op: 0,
            kind: StateResumeKind::DoneSub0Mes { length, .. },
        } => lead_at(operand + 2 + *length as usize),
        InsnInfo::MenuCtrl {
            kind: MenuCtrlKind::HighNibble {
                outer: 0xE, sub: 1, ..
            },
            ..
        } => lead_at(operand + 1),
        _ => None,
    }
}
