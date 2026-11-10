//! `FUN_801DE840`, the field VM, one instruction at a time over every distinct
//! instruction in the disc's scene scripts, on a context seeded with generated
//! bytes: does `legaia_engine_vm::field::step` leave the context's fields where
//! retail leaves them? `field_vm` asks where the next instruction is; this asks
//! what the instruction did to the context it ran on. Calls outside the field
//! overlay are stubbed on retail's side and the host is `Inert` on the port's,
//! so a field the port leaves to its host is counted, not compared: what is
//! held is every store `step` makes itself.

use crate::common::{Rng, disc, scus_cpu};
use crate::field_vm::{Inert, corpus};
use legaia_engine_vm::field::*;
use psx_retail_exec::{GP, Stop};
use std::collections::BTreeMap;

const VM: u32 = 0x801D_E840;
const OVERLAY_BASE: u32 = 0x801C_E818;
const SCRIPT: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;
const RESOLVE_ACTOR: u32 = 0x8003_C83C;
const READ_U16: u32 = 0x8003_CE9C;
const PACKET_LENGTH: u32 = 0x8003_CA38;

/// `(name, offset, width)` of every context field the port models.
const FIELDS: [(&str, u32, u8); 28] = [
    ("flags", 0x10, 4),
    ("world_x", 0x14, 2),
    ("world_y", 0x16, 2),
    ("world_z", 0x18, 2),
    ("field_24", 0x24, 2),
    ("field_26", 0x26, 2),
    ("field_28", 0x28, 2),
    ("field_42", 0x42, 2),
    ("script_id", 0x50, 2),
    ("wait_accum", 0x54, 2),
    ("move_substate", 0x56, 2),
    ("field_58", 0x58, 2),
    ("saved_26", 0x5A, 2),
    ("move_id", 0x5C, 2),
    ("field_5e", 0x5E, 2),
    ("local_flags", 0x62, 2),
    ("model_id", 0x64, 2),
    ("field_68", 0x68, 2),
    ("field_6a", 0x6A, 2),
    ("anim_count", 0x6C, 1),
    ("face_rotation", 0x6D, 1),
    ("field_72", 0x72, 2),
    ("field_74", 0x74, 4),
    ("field_78", 0x78, 2),
    ("field_8b", 0x8B, 1),
    ("npc_x", 0x8C, 1),
    ("npc_facing", 0x8D, 1),
    ("field_8e", 0x8E, 2),
];

fn port_field(c: &FieldCtx, name: &str) -> u32 {
    match name {
        "flags" => c.flags,
        "world_x" => c.world_x.into(),
        "world_y" => c.world_y.into(),
        "world_z" => c.world_z.into(),
        "field_24" => c.field_24 as u16 as u32,
        "field_26" => c.field_26.into(),
        "field_28" => c.field_28 as u16 as u32,
        "field_42" => c.field_42.into(),
        "script_id" => c.script_id.into(),
        "wait_accum" => c.wait_accum as u16 as u32,
        "move_substate" => c.move_substate.into(),
        "field_58" => c.field_58.into(),
        "saved_26" => c.saved_26.into(),
        "move_id" => c.move_id.into(),
        "field_5e" => c.field_5e.into(),
        "local_flags" => c.local_flags.into(),
        "model_id" => c.model_id.into(),
        "field_68" => c.field_68 as u16 as u32,
        "field_6a" => c.field_6a as u16 as u32,
        "anim_count" => c.anim_count.into(),
        "face_rotation" => c.face_rotation.into(),
        "field_72" => c.field_72.into(),
        "field_74" => c.field_74,
        "field_78" => c.field_78.into(),
        "field_8b" => c.field_8b.into(),
        "npc_x" => c.npc_x.into(),
        "npc_facing" => c.npc_facing.into(),
        "field_8e" => c.field_8e as u16 as u32,
        _ => unreachable!(),
    }
}

fn seat(c: &mut FieldCtx, name: &str, v: u32) {
    match name {
        "flags" => c.flags = v,
        "world_x" => c.world_x = v as u16,
        "world_y" => c.world_y = v as u16,
        "world_z" => c.world_z = v as u16,
        "field_24" => c.field_24 = v as i16,
        "field_26" => c.field_26 = v as u16,
        "field_28" => c.field_28 = v as i16,
        "field_42" => c.field_42 = v as u16,
        "script_id" => c.script_id = v as u16,
        "wait_accum" => c.wait_accum = v as i16,
        "move_substate" => c.move_substate = v as u16,
        "field_58" => c.field_58 = v as u16,
        "saved_26" => c.saved_26 = v as u16,
        "move_id" => c.move_id = v as u16,
        "field_5e" => c.field_5e = v as u16,
        "local_flags" => c.local_flags = v as u16,
        "model_id" => c.model_id = v as u16,
        "field_68" => c.field_68 = v as i16,
        "field_6a" => c.field_6a = v as i16,
        "anim_count" => c.anim_count = v as u8,
        "face_rotation" => c.face_rotation = v as u8,
        "field_72" => c.field_72 = v as u16,
        "field_74" => c.field_74 = v,
        "field_78" => c.field_78 = v as u16,
        "field_8b" => c.field_8b = v as u8,
        "npc_x" => c.npc_x = v as u8,
        "npc_facing" => c.npc_facing = v as u8,
        "field_8e" => c.field_8e = v as i16,
        _ => unreachable!(),
    }
}

fn rd(cpu: &mut psx_retail_exec::Cpu, off: u32, width: u8) -> u32 {
    match width {
        1 => cpu.mem.rd8(CTX + off).into(),
        2 => cpu.mem.rd16(CTX + off).into(),
        _ => cpu.mem.rd32(CTX + off),
    }
}

fn wr(cpu: &mut psx_retail_exec::Cpu, off: u32, width: u8, v: u32) {
    match width {
        1 => cpu.mem.wr8(CTX + off, v as u8),
        2 => cpu.mem.wr16(CTX + off, v as u16),
        _ => cpu.mem.wr32(CTX + off, v),
    }
}

/// The field overlay over the executable, every cross-context target resolved
/// to the one context under test and every call outside the overlay stubbed.
fn retail_vm() -> Option<psx_retail_exec::Cpu> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = CTX);
    cpu.hook(READ_U16, |r, m| r.r[2] = u32::from(m.rd16(r.r[4])));
    cpu.hook(PACKET_LENGTH, |r, m| {
        let bytes = m.read(r.r[4], 0x400);
        r.r[2] = legaia_scene_formats::field_disasm::packet_length(&bytes) as u32;
    });
    cpu.step_budget = 20_000;
    cpu.mem.wr8(0x1F80_0393, 1);
    Some(cpu)
}

#[test]
fn every_instruction_leaves_its_context_as_retail_leaves_it() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let corpus = corpus();
    let mut rng = Rng(0x4354_5856);

    let mut ran = 0u32;
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    let mut missed: BTreeMap<String, u32> = BTreeMap::new();
    for (script, pc, label) in &corpus {
        for round in 0..2 {
            cpu.mem = clean.clone();
            cpu.regs.r[GP] = gp;
            cpu.mem.load(SCRIPT, script);
            cpu.mem.wr32(CTX + 0x94, SCRIPT);
            let mut ctx = FieldCtx::default();
            for (name, off, width) in FIELDS {
                let mut v = rng.next();
                if name == "flags" {
                    // Running, not halted.
                    v &= !0x400;
                }
                if name == "script_id" {
                    v = if round == 0 { 0xFB } else { rng.below(0x40) };
                }
                if width == 1 {
                    v &= 0xFF;
                } else if width == 2 {
                    v &= 0xFFFF;
                }
                wr(&mut cpu, off, width, v);
                seat(&mut ctx, name, v);
            }
            let stop = cpu.call(VM, &[SCRIPT, *pc as u32, CTX]);
            if stop != Stop::Returned {
                continue;
            }
            let seeded = ctx.clone();
            let res = step(&mut Inert { hit: true }, &mut ctx, script, *pc);
            if matches!(res, StepResult::Pending { .. } | StepResult::Unknown { .. }) {
                continue;
            }
            ran += 1;
            let mut wrong = Vec::new();
            for (name, off, width) in FIELDS {
                let r = rd(&mut cpu, off, width);
                let p = port_field(&ctx, name);
                if r == p {
                    continue;
                }
                if port_field(&seeded, name) == p {
                    // The port left it to the host, which `Inert` is not.
                    *missed.entry(format!("{label} {name}")).or_default() += 1;
                } else {
                    wrong.push(format!("{name} retail {r:#x} port {p:#x}"));
                }
            }
            // `4C 50`'s writes are the default `set_actor_model`, the port of
            // `FUN_80024E08`: a SCUS call the stub world skips on retail's side.
            if !wrong.is_empty() && label != "4C 50" {
                let e = differ.entry(label.clone()).or_default();
                e.0 += 1;
                if e.0 == 1 {
                    e.1 = format!(
                        "pc {pc:#x} bytes {:02x?}: {}",
                        &script[*pc..(*pc + 10).min(script.len())],
                        wrong.join(", ")
                    );
                }
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    eprintln!("{ran} runs; left to the host: {missed:?}");
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(ran > 4000, "{ran}");
}

/// The `4C 51` seat is the op the port hands its host whole: `step` decodes
/// it and `World` writes the tile, the class bit, the anim-control word and
/// the move id. Stepped by the real `World` as a live placement channel's own
/// op (the `executing_channel` arm the talk runner, the prop runner and a
/// channel's slice all reach), does the placement end up where retail's
/// `0x801E1828..0x801E1940` arm puts the actor? Compared: the tile on the
/// context and on the placement's drawn seat, the party-bank bit of `+0x10`,
/// `+0x62` and `+0x5C`, and the `+0x26` heading the seat stores. Not
/// compared: `+0x16` (the floor sample is an executable call, stubbed here),
/// `+0x10` bit 1 (the cull `FUN_801D79E8` the arm calls, which the port runs
/// per frame) and the walk-state bytes `+0x5E` / `+0x8B..+0x8D`, which the
/// port's NPC walk does not keep.
#[test]
fn a_seat_lands_the_placement_where_retail_seats_the_actor() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5345_4154);
    let mut ran = 0u32;
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in corpus() {
        if label != "4C 51" {
            continue;
        }
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        let mut ctx = FieldCtx::default();
        for (name, off, width) in FIELDS {
            let mut v = rng.next();
            match name {
                // Running, not halted.
                "flags" => v &= !0x400,
                "script_id" => v = rng.below(0x40),
                _ => {}
            }
            if width == 1 {
                v &= 0xFF;
            } else if width == 2 {
                v &= 0xFFFF;
            }
            wr(&mut cpu, off, width, v);
            seat(&mut ctx, name, v);
        }
        if cpu.call(VM, &[SCRIPT, pc as u32, CTX]) != Stop::Returned {
            continue;
        }
        let mut w = legaia_engine_core::world::World::new();
        w.field_vm.executing_channel = Some(0);
        w.npcs
            .positions
            .insert(0, (ctx.world_x as i16, ctx.world_z as i16));
        w.field_ctx = ctx;
        w.field_bytecode = script.clone();
        w.field_pc = pc;
        let _ = w.step_field();
        ran += 1;
        let c = &w.field_ctx;
        let (rx, rz) = (rd(&mut cpu, 0x14, 2), rd(&mut cpu, 0x18, 2));
        let seat = w.npcs.positions.get(&0).copied();
        let heading = w.npcs.headings.get(&0).copied();
        let mut wrong = Vec::new();
        let mut check = |what: &str, retail: u32, port: u32| {
            if retail != port {
                wrong.push(format!("{what} retail {retail:#x} port {port:#x}"));
            }
        };
        check("+0x14", rx, c.world_x.into());
        check("+0x18", rz, c.world_z.into());
        check(
            "drawn seat",
            rx << 16 | rz,
            seat.map_or(u32::MAX, |(x, z)| {
                u32::from(x as u16) << 16 | u32::from(z as u16)
            }),
        );
        check(
            "+0x10 party bank",
            rd(&mut cpu, 0x10, 4) & 0x0100_0000,
            c.flags & 0x0100_0000,
        );
        check("+0x62", rd(&mut cpu, 0x62, 2), c.local_flags.into());
        check("+0x5C", rd(&mut cpu, 0x5C, 2), c.move_id.into());
        // The port keeps the drawn heading half a turn on from `+0x26`.
        check(
            "+0x26",
            (rd(&mut cpu, 0x26, 2) + 0x800) & 0xFFF,
            heading.map_or(u32::MAX, |h| u32::from(h as u16) & 0xFFF),
        );
        if !wrong.is_empty() {
            let e = differ
                .entry(format!("{:02x?}", &script[pc..pc + 4]))
                .or_default();
            e.0 += 1;
            if e.0 == 1 {
                e.1 = wrong.join(", ");
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(ran > 1000, "{ran}");
}

/// The `4C 81` tint is the op the port hands its host, which keeps the pair in
/// `actor_tints` rather than on a context. Stepped by the real `World` in all
/// three forms (the running placement's own `4C 81`, `CC <id> 81` on another
/// placement, `CC F8 81` on the player), with the actor's tint seeded as the
/// context's `+0x74` / `+0x78`: does the tint land where retail leaves the
/// two words? A tween's ramp is `FUN_8003C5F0`, an executable call stubbed
/// here, so what is compared is the immediate write and the colour a tween
/// writes first (`0x801E1FC4..0x801E2068`).
#[test]
fn a_tint_lands_where_retail_tints_the_actor() {
    differential!(prot(897, VM));
    use legaia_engine_field::object_tint::{ActorTint, ActorTintKey};
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The 24-bit colour read.
    cpu.hook(0x8003_CEB8, |r, m| {
        let a = r.r[4];
        r.r[2] = u32::from(m.rd8(a)) | u32::from(m.rd8(a + 1)) << 8 | u32::from(m.rd8(a + 2)) << 16;
    });
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5449_4E54);
    let (mut ran, mut immediate) = (0u32, 0u32);
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in corpus() {
        if label != "4C 81" {
            continue;
        }
        for _ in 0..4 {
            cpu.mem = clean.clone();
            cpu.regs.r[GP] = gp;
            cpu.mem.load(SCRIPT, &script);
            cpu.mem.wr32(CTX + 0x94, SCRIPT);
            let mut ctx = FieldCtx::default();
            for (name, off, width) in FIELDS {
                let mut v = rng.next();
                match name {
                    // Running, not halted, and not the player's own context.
                    "flags" => v &= !0x0100_0400,
                    "script_id" => v = rng.below(0x40),
                    "field_74" => v &= 0x00FF_FFFF,
                    "field_78" if rng.below(3) == 0 => v = 0,
                    _ => {}
                }
                if width == 1 {
                    v &= 0xFF;
                } else if width == 2 {
                    v &= 0xFFFF;
                }
                wr(&mut cpu, off, width, v);
                seat(&mut ctx, name, v);
            }
            if cpu.call(VM, &[SCRIPT, pc as u32, CTX]) != Stop::Returned {
                continue;
            }
            let mut w = legaia_engine_core::world::World::new();
            w.field_vm.executing_channel = Some(0);
            let key = match (script[pc] & 0x80 != 0).then(|| script[pc + 1]) {
                None => ActorTintKey::Npc(0),
                Some(0xF8) => ActorTintKey::Player,
                Some(0xFB) => continue,
                Some(t) => {
                    let mut tctx = ctx.clone();
                    tctx.script_id = u16::from(t);
                    w.field_vm
                        .channels
                        .push(legaia_engine_field::field_channels::FieldChannel {
                            placement_index: 5,
                            ctx: tctx,
                            record_offset: 0,
                            pc: 0,
                            done: true,
                            object_bind: false,
                        });
                    ActorTintKey::Npc(5)
                }
            };
            w.field_vm.actor_tints.insert(
                key,
                ActorTint {
                    colour: ctx.field_74,
                    blend: ctx.field_78,
                    ..Default::default()
                },
            );
            w.field_ctx = ctx;
            w.field_bytecode = script.clone();
            w.field_pc = pc;
            let _ = w.step_field();
            ran += 1;
            let t = w
                .field_vm
                .actor_tints
                .get(&key)
                .copied()
                .unwrap_or_default();
            let retail = (rd(&mut cpu, 0x74, 4), rd(&mut cpu, 0x78, 2));
            let port = (t.colour, u32::from(t.blend));
            if t.blend_ramp.is_none() {
                immediate += 1;
            }
            if retail != port {
                let e = differ
                    .entry(format!("{:02x?}", &script[pc..(pc + 10).min(script.len())]))
                    .or_default();
                e.0 += 1;
                if e.0 == 1 {
                    e.1 = format!("{key:?}: retail {retail:x?} port {port:x?}");
                }
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(ran > 500 && immediate > 100, "{ran} {immediate}");
}

/// `0x23`, the tile warp, stepped by the real `World` on the running
/// placement and, prefixed, on another placement's channel: the tile, the
/// world pair and the walk byte land on the context the op names. Not
/// compared: `+0x16`, the floor sample (an executable call, stubbed here), and
/// `+0x10` bit 1, which the cull `FUN_801D79E8` the arm calls raises and the
/// port's per-frame cull owns. The player form `A3 F8` moves the player.
#[test]
fn a_tile_warp_lands_on_the_context_it_names() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5741_5250);
    let (mut own, mut other) = (0u32, 0u32);
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in corpus() {
        if label != "23" || (script[pc] & 0x80 != 0 && matches!(script[pc + 1], 0xF8 | 0xFB)) {
            continue;
        }
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        let mut ctx = FieldCtx::default();
        for (name, off, width) in FIELDS {
            let mut v = rng.next();
            match name {
                "flags" => v &= !0x0100_0400,
                "script_id" => v = rng.below(0x40),
                _ => {}
            }
            if width == 1 {
                v &= 0xFF;
            } else if width == 2 {
                v &= 0xFFFF;
            }
            wr(&mut cpu, off, width, v);
            seat(&mut ctx, name, v);
        }
        if cpu.call(VM, &[SCRIPT, pc as u32, CTX]) != Stop::Returned {
            continue;
        }
        let mut w = legaia_engine_core::world::World::new();
        w.field_vm.executing_channel = Some(0);
        let cross = script[pc] & 0x80 != 0;
        if cross {
            let mut tctx = ctx.clone();
            tctx.script_id = u16::from(script[pc + 1]);
            w.field_vm
                .channels
                .push(legaia_engine_field::field_channels::FieldChannel {
                    placement_index: 5,
                    ctx: tctx,
                    record_offset: 0,
                    pc: 0,
                    done: true,
                    object_bind: false,
                });
            other += 1;
        } else {
            own += 1;
        }
        w.field_ctx = ctx;
        w.field_bytecode = script.clone();
        w.field_pc = pc;
        let _ = w.step_field();
        let c = if cross {
            w.field_vm.channels[0].ctx.clone()
        } else {
            w.field_ctx.clone()
        };
        let mut wrong = Vec::new();
        for (name, off, width) in FIELDS {
            let (mut r, mut p) = (rd(&mut cpu, off, width), port_field(&c, name));
            match name {
                "world_y" | "script_id" => continue,
                "flags" => {
                    r &= !2;
                    p &= !2;
                }
                _ => {}
            }
            if r != p {
                wrong.push(format!("{name} retail {r:#x} port {p:#x}"));
            }
        }
        if !wrong.is_empty() {
            let e = differ
                .entry(format!("{:02x?}", &script[pc..(pc + 4).min(script.len())]))
                .or_default();
            e.0 += 1;
            if e.0 == 1 {
                e.1 = wrong.join(", ");
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(own > 100 && other > 500, "{own} {other}");
}
