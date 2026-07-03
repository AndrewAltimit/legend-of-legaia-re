use super::*;

pub(super) fn decode_actor_ctrl(
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
            mk(header_size + 20, ActorCtrlKind::WidgetSpriteSpawn)
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
            mk(header_size + 11, ActorCtrlKind::WidgetMaskRect { words })
        }
        0x12 | 0x13 => {
            need(13)?;
            mk(header_size + 13, ActorCtrlKind::WidgetPayload14)
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
            mk(header_size + 9, ActorCtrlKind::WidgetPanelMove { words })
        }
        0x15 => {
            need(13)?;
            mk(header_size + 13, ActorCtrlKind::WidgetLetterbox)
        }
        _ => Err(DisasmError::UnknownSubOp { pc, opcode, sub_op }),
    }
}

pub(super) fn decode_camera(
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

pub(super) fn decode_state_resume(
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

pub(super) fn decode_inventory_cmp(
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

pub(super) fn decode_menu_ctrl(
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
                // Common in placement records - without this arm the linear
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
