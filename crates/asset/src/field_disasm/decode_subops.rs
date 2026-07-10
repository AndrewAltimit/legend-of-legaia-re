//! Decoders for the multi-byte sub-opcode groups (actor-control, camera,
//! state-resume, inventory-compare, menu-control) dispatched out of the main
//! instruction decoder.

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
        // Sub-ops 0/1 (inventory) share the 6-operand compare shape with
        // 2/3/9 (char level byte +0x130 / party gold _DAT_8008459C / coin
        // bank 0x800845A4): the overlay-0897 jump table at VA 0x801CEE30
        // routes 2/3/9 to dedicated value loaders (0x801E0AC0/0AEC/0B34)
        // that fall into the same compare-and-skip continuation as 0/1;
        // only sub-ops 5..8 take the 0x801C6460 absolute-jump arm.
        0 | 1 | 2 | 3 | 9 => {
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
        5..=8 => {
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
        // Nibble 8 - large multi-purpose dispatcher. Widths are the retail
        // dispatcher's `s8 += N` advances, pinned from the raw asm of the
        // nibble-8 switch in FUN_801DE840 (overlay 0897, base 0x801CE818):
        // sub-1 `addiu fp,fp,9` @ 0x801E1FC4, sub-3 exit `addiu fp,fp,7` @
        // 0x801E2130, sub-5/E/F `li s7,5; addu fp,fp,s7` @ 0x801E21B8/21D4,
        // sub-6 `addiu fp,fp,0xf` @ 0x801E21E8 (branch-delay slot). They
        // match the executing VM's port (engine-vm menu_ctrl/nibble_8.rs).
        // `encoded` counts the sub-op byte + operands, so total = header + N
        // with N = retail advance - 1. The earlier sub-1 width (encoded 9)
        // was one byte wide and desynced the walker right after every
        // `[4C, 0x81, ..]` actor model+anim set (the vozz P1[7]
        // `.byte 0x05` case).
        8 => {
            let sub = op0 & 0x0F;
            let encoded = match sub {
                // Sub-0: actor-allocator halt-acquire `[4C, 0x80, count]`;
                // retail advances +3 on spawn (the inline child records past
                // +2 belong to the spawned actor's bytecode pointer).
                0 => 2,
                // Sub-1: 9-byte actor model + anim set
                // `[4C, 0x81, m0..m2, anim_lo, anim_hi, frames_lo, frames_hi]`.
                1 => 8,
                2 | 4 => 2,
                3 => 6, // sub-3: 7-byte rectangular tile fill
                // Sub-5/E/F: halt-acquire `[4C, op0, p0, p1, p2]`; retail
                // advances +5 on acquire (only the predicate-failure path
                // halts at PC) - the linear walk uses the acquire width.
                5 | 0xE | 0xF => 4,
                6 => 14, // sub-6: 15-byte actor set position+rotation
                7 => 1,  // halt at PC; footprint width is the 2-byte op
                8 => 5,
                9 => 3, // sub-9: 4-byte total `[4C, 0x89, lo, hi]`
                0xA => 10,
                0xB => 4, // sub-B: 5-byte `[4C, 0x8B, type_byte, target_lo, target_hi]`
                // Sub-C: 4-byte conditional jump on ctx+0x68
                // `[4C, 0x8C, target_lo, target_hi]`; fall-through width.
                0xC => 3,
                0xD => 5, // sub-D: 6-byte char-actor search
                _ => unreachable!("op0 & 0x0F is at most 0xF"),
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
            // 0x4C 0xE_ - the FMV trigger (sub-2) lives here. Widths follow the
            // retail dispatcher's `param_2 + N` advances (FUN_801DE840, outer
            // nibble 0xE switch): sub-2/6 = 6/8 total, sub-4 = 8, sub-5 = 5,
            // sub-7 = 7, sub-8 = 10, sub-9/C/E = 2, sub-B = 5 (or an absolute
            // jump when the actor is missing - linear walk uses 5), sub-D = 3
            // (the `_DAT_8007BA66` write - runtime-pinned by the flag-549
            // capture: town01 P2[3] executes `4C ED 01` then `52 25`).
            // Sub-0 and sub-3 are pinned from the raw asm (the decompile hides
            // their advances behind `goto LAB_801e00bc`): sub-0 (0x801E306C)
            // exits every write path through the `addiu s8,s8,0x3` entry at
            // 0x801E00B8, sub-3 (0x801E3108) advances +3 either there or in
            // the `j 0x801E00BC` branch-delay slot - both 3 bytes total,
            // confirming the previously-empirical widths. Case arms verified
            // against the overlay-0897 jump table at VA 0x801CF008.
            let sub = op0 & 0x0F;
            match sub {
                0 => {
                    // 3-way state write; retail advances +3 total (4C, E0, b1).
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
                // Sub-4: bbox halt-or-advance, retail `param_2 + 8`.
                4 => {
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
                // Sub-5: 24-bit signed coin-bank add + SysFlag.Set(8); 5 total.
                5 => {
                    need(4)?;
                    mk(
                        header_size + 4,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 4,
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
                // Sub-7: camera animate, retail `param_2 + 7`.
                7 => {
                    need(6)?;
                    mk(
                        header_size + 6,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 6,
                        },
                    )
                }
                // Sub-8: camera zoom, retail `param_2 + 10`.
                8 => {
                    need(9)?;
                    mk(
                        header_size + 9,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 9,
                        },
                    )
                }
                // Sub-9: clear `_DAT_8007B9C4`, retail `caseD_4` = PC += 2.
                // Sub-A: call `func_0x8003C7EC` then halt at PC; no operands.
                // Sub-C: capture `FUN_801DDF48`, retail `param_2 + 2`.
                // Sub-E: snapshot `_DAT_800845DC = _DAT_80084570`, `param_2 + 2`.
                9 | 0xA | 0xC | 0xE => {
                    need(1)?;
                    mk(
                        header_size + 1,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 1,
                        },
                    )
                }
                // Sub-B: conditional actor lookup, retail `param_2 + 5` when
                // the actor resolves, else absolute jump to LE_u16(+2..+3).
                // Linear walk uses the resolved width (5 total).
                0xB => {
                    need(4)?;
                    mk(
                        header_size + 4,
                        MenuCtrlKind::HighNibble {
                            outer: 0xE,
                            sub,
                            encoded_size: header_size + 4,
                        },
                    )
                }
                // Sub-D: `_DAT_8007BA66 = b1`, retail `param_2 + 3`. The op
                // whose missing width hid the town01 P2[3] flag-549 self-latch
                // (`4C ED 01` immediately followed by `52 25` SysFlag.Set
                // 0x225 - runtime-pinned via the field-VM script-PC capture).
                0xD => {
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
        // Outer nibbles 9 / A / C / D / F - widths mirror the executing VM's
        // menu_ctrl port (crates/engine-vm/src/field/step/menu_ctrl/
        // nibble_9_a.rs / nibble_c.rs / nibble_d.rs), which took them from the
        // retail dispatcher's `param_2 + N` advances. Before these arms the
        // disassembler returned UnknownSubOp for all five nibbles, so ANY
        // record crossing one desynced exactly like the flag-549 `4C ED`
        // case - notably nibble-A (flag-conditional absolute jump; sub-2
        // tests a global story flag) and nibble-D sub-3 (SCHEDULE_TIMED_FLAGS,
        // a timed flag writer). First seen live on jouinc's P2 J-family
        // records (`CC 06 A1 0A 1D 00` at body +0xE).
        //
        // Nibble 9 - fade family. Sub-0/1/2 = 9 total, sub-E = 34-byte
        // 16-word table copy, sub-F = 2-byte callback registration (halts at
        // PC until the callback fires; linear walk uses the footprint).
        // Sub-3..=0xD have no `case` arm in retail (dispatcher returns
        // `param_2`) - genuinely undefined, kept as UnknownSubOp.
        9 => {
            let sub = op0 & 0x0F;
            let encoded = match sub {
                0..=2 => 8,
                0xE => 33,
                0xF => 1,
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
                MenuCtrlKind::HighNibble {
                    outer: 9,
                    sub,
                    encoded_size: header_size + encoded,
                },
            )
        }
        // Nibble A - conditional jump on a flag bit, 5 bytes total
        // `[4C, 0xAN, bit, lo, hi]` for every sub (sub-0 ctx.flags, sub-1
        // ctx.local_flags, sub-2 the global story-flag word; sub-3..=0xF test
        // nothing and always fall through). Bit SET takes the absolute jump
        // from LE_u16(+2..+3); the linear walk uses the fall-through width.
        0xA => {
            need(4)?;
            mk(
                header_size + 4,
                MenuCtrlKind::HighNibble {
                    outer: 0xA,
                    sub: op0 & 0x0F,
                    encoded_size: header_size + 4,
                },
            )
        }
        // Nibble C - small per-actor / per-scene writes. Encoded widths per
        // sub (counting the sub-op byte + operands): 0/1/3/8/9/D = 1,
        // 2/E = 2, 4/5/6/7/F = 3, A/B/C = 4. Sub-5/6 are party-flag
        // conditional jumps (linear walk uses the fall-through width);
        // sub-9/D halt at PC (footprint width).
        0xC => {
            let sub = op0 & 0x0F;
            let encoded = match sub {
                0 | 1 | 3 | 8 | 9 | 0xD => 1,
                2 | 0xE => 2,
                4..=7 | 0xF => 3,
                0xA..=0xC => 4,
                _ => unreachable!("op0 & 0x0F is at most 0xF"),
            };
            need(encoded)?;
            mk(
                header_size + encoded,
                MenuCtrlKind::HighNibble {
                    outer: 0xC,
                    sub,
                    encoded_size: header_size + encoded,
                },
            )
        }
        // Nibble D - heterogeneous. Encoded widths per sub: 2/6/7/A = 1
        // (sub-2/6/7 halt at PC; footprint width), D/F = 2, 1/9 = 3,
        // C/E = 4 (party search; linear walk uses the miss width), 0/4/5 = 5,
        // 8 = 8, B = 12, 3 = 13 (SCHEDULE_TIMED_FLAGS - the timed
        // flag-scheduler write the flag census must be able to walk).
        0xD => {
            let sub = op0 & 0x0F;
            let encoded = match sub {
                2 | 6 | 7 | 0xA => 1,
                0xD | 0xF => 2,
                1 | 9 => 3,
                0xC | 0xE => 4,
                0 | 4 | 5 => 5,
                8 => 8,
                0xB => 12,
                3 => 13,
                _ => unreachable!("op0 & 0x0F is at most 0xF"),
            };
            need(encoded)?;
            mk(
                header_size + encoded,
                MenuCtrlKind::HighNibble {
                    outer: 0xD,
                    sub,
                    encoded_size: header_size + encoded,
                },
            )
        }
        // Nibble F - only `op0 == 0xFF` is meaningful in retail; every sub
        // falls through to the default arm's PC += 2.
        0xF => {
            need(1)?;
            mk(
                header_size + 1,
                MenuCtrlKind::HighNibble {
                    outer: 0xF,
                    sub: op0 & 0x0F,
                    encoded_size: header_size + 1,
                },
            )
        }
        // Nibble B has no `case 0xb` in the retail 0x4C switch (the dump goes
        // case 0xa -> default -> case 0xc; the default arm prints
        // SUB_CMD_ERROR and halts at PC) - genuinely undefined.
        outer @ 0xB => Err(DisasmError::UnknownSubOp {
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
