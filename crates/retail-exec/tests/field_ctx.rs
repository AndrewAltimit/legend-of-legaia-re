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
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
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

/// The answers a stubbed retail world gives the VM, read off the same RAM
/// the test seeds: the story-flag word `0x1F800394`, the held pad
/// `0x8007B850`, the region mask `0x8007B8F4`, the menu-resume slot
/// `0x8007B450`, the d-pad compass table `0x801F28D0` and the spawn bracket
/// `*(0x801C6EA4) + 8`. Everything else answers as [`Inert`] does.
struct Seeded {
    flags: u32,
    pad: u32,
    regions: u32,
    compass: [u32; 8],
    op49: Op49State,
    bracket: bool,
    /// The context carries an attached light (the actor list holds an
    /// `FUN_801E4470` actor whose `+0x90` is the context).
    light: bool,
    /// The length of the keyframe block `34 2x` handed over, if it did.
    captured: Option<usize>,
    /// The system flag bank `0x80085758` (`FUN_8003CE64`'s, MSB first).
    system: std::rc::Rc<Vec<u8>>,
}
impl FieldFlagsHost for Seeded {
    fn global_flags(&self) -> u32 {
        self.flags
    }
    fn set_global_flags(&mut self, value: u32) {
        self.flags = value;
    }
    fn frame_delta(&self) -> u16 {
        1
    }
    fn extra_flags(&self) -> u32 {
        self.regions
    }
    fn held_pad(&self) -> u32 {
        self.pad
    }
    fn pad_compass_entry(&self, index: u8) -> Option<u32> {
        self.compass.get(usize::from(index)).copied()
    }
    fn system_flag_test(&self, idx: u16) -> bool {
        self.system
            .get(usize::from(idx >> 3))
            .is_some_and(|b| b & (0x80 >> (idx & 7)) != 0)
    }
}
impl FieldSceneHost for Seeded {}
impl FieldTerrainHost for Seeded {
    fn world_to_tile_use_alt(&self) -> bool {
        self.flags & 0x0002_0000 != 0
    }
}
impl FieldCameraHost for Seeded {}
impl FieldCameraMotionHost for Seeded {
    fn camera_glide_live(&mut self) -> bool {
        false
    }
}
impl FieldScreenHost for Seeded {}
impl FieldAudioHost for Seeded {
    fn bgm_ready(&mut self, sub_op: u8) -> bool {
        sub_op != 0xA
    }
}
impl FieldPartyInventoryHost for Seeded {
    fn op4c_nd_subc_item_held(&self, _needle: u8) -> bool {
        true
    }
}
impl FieldDialogMenuHost for Seeded {
    fn press_prompt(&mut self, _x: u16, _y: u16) -> bool {
        false
    }
    fn op4c_n5_sub3_dialog_wait(&mut self, _ctx: &mut FieldCtx) -> bool {
        true
    }
    fn op49_state(&self) -> Op49State {
        self.op49
    }
}
impl FieldActorHost for Seeded {}
impl FieldActorSpawnHost for Seeded {
    fn actor_guard(&mut self, _actor_id: u8) -> Option<()> {
        Some(())
    }
    fn op34_sub2_capture_pc(
        &mut self,
        _ctx: &FieldCtx,
        _ext: Option<u8>,
        block: Option<&[u8]>,
    ) -> bool {
        match block {
            Some(b) if self.light => {
                self.captured = Some(b.len());
                true
            }
            _ => false,
        }
    }
}
impl FieldActorFxHost for Seeded {
    fn spawn_attached_light(
        &mut self,
        _ctx: &FieldCtx,
        _ext: Option<u8>,
        _spawn: &legaia_engine_vm::field_actor_billboard::AttachedSpriteSpawn,
        _script: Option<&[u8]>,
    ) -> bool {
        // A context that already carries a light spawns none.
        !self.light
    }
}
impl FieldPlayerHost for Seeded {}
impl FieldHaltHost for Seeded {
    fn spawn_bracket(&self) -> bool {
        self.bracket
    }
}

/// The corpus again, with the generated sub-ops of
/// [`crate::field_vm::synthetic`], each instruction under seeded world state
/// the plain corpus runs leave clear: random story flags, held pad and region mask;
/// a generated system flag bank (`0x80085758`, read through `FUN_8003CE64`);
/// the menu-resume slot `0x8007B450` armed and done; a context that carries
/// the halt bit, with the spawn bracket up or down and the system script
/// `0xFB` calling it or not; the player's context `0x8007C364`; and a context that runs no
/// script (`+0x94 == 0`), the player's or not. Retail reads that state from RAM; the port
/// asks its host, which answers from the same seeds. Compared: the next PC,
/// every modelled context field and the story-flag word.
#[test]
fn every_instruction_under_seeded_state_steps_as_retail_steps() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const FLAGS: u32 = 0x1F80_0394;
    const PAD: u32 = 0x8007_B850;
    const REGIONS: u32 = 0x8007_B8F4;
    const OP49: u32 = 0x8007_B450;
    const SCENE: u32 = 0x8013_0000;
    const PLAYER: u32 = 0x8007_C364;
    // The spawn bracket's scene record.
    cpu.mem.wr32(0x801C_6EA4, SCENE);
    // A generated system flag bank, read through `FUN_8003CE64` (a set bit
    // answers `0xFF`) on retail's side and `system_flag_test` on the port's.
    const SYSTEM: u32 = 0x8008_5758;
    let mut bank_rng = Rng(0x5359_5346);
    let mut bank: Vec<u8> = (0..0x200).map(|_| bank_rng.next() as u8).collect();
    // Character record 4's `+0x6CC..+0x6F8` lies inside the bank, where
    // `4E` reads it for a page past the party: kept clear, as the port's
    // host answers an absent page.
    bank[0x104..0x140].fill(0);
    let system = std::rc::Rc::new(bank);
    cpu.mem.load(SYSTEM, &system);
    cpu.hook(0x8003_CE64, |r, m| {
        let idx = r.r[4] & 0xFFFF;
        let set = m.rd8(SYSTEM + (idx >> 3)) & (0x80 >> (idx & 7)) != 0;
        r.r[2] = if set { 0xFF } else { 0 };
    });
    let mut compass = [0u32; 8];
    for (i, c) in compass.iter_mut().enumerate() {
        *c = cpu.mem.rd32(0x801F_28D0 + 4 * i as u32);
    }
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut corpus = corpus();
    corpus.extend(crate::field_vm::synthetic());
    let mut rng = Rng(0x5345_4544);

    let mut ran = 0u32;
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in &corpus {
        for variant in 0..8u32 {
            cpu.mem = clean.clone();
            cpu.regs.r[GP] = gp;
            cpu.mem.load(SCRIPT, script);
            cpu.mem.wr32(CTX + 0x94, SCRIPT);
            let mut host = Seeded {
                flags: rng.next(),
                pad: rng.next(),
                regions: rng.next(),
                compass,
                op49: Op49State::Idle,
                bracket: false,
                light: false,
                captured: None,
                system: system.clone(),
            };
            if rng.below(4) == 0 {
                // A pad that matches a compass entry.
                host.pad = (host.pad & !0xF000) | (compass[rng.below(8) as usize] & 0xF000);
            }
            let mut halted = false;
            let mut id = rng.below(0x40);
            match variant {
                1 => {
                    host.op49 = Op49State::Done;
                    cpu.mem.wr32(OP49, 1);
                }
                2 => {
                    host.op49 = Op49State::Armed;
                    cpu.mem.wr32(OP49, SCRIPT + 0x10);
                }
                3 => {
                    halted = true;
                    host.bracket = true;
                    cpu.mem.wr16(SCENE + 8, 1);
                }
                4 => {
                    halted = true;
                    if rng.below(2) == 0 {
                        id = 0xFB;
                    }
                }
                // The context is the player's.
                5 => cpu.mem.wr32(PLAYER, CTX),
                // The context runs no script, and is or is not the player's.
                6 => cpu.mem.wr32(CTX + 0x94, 0),
                7 => {
                    cpu.mem.wr32(CTX + 0x94, 0);
                    cpu.mem.wr32(PLAYER, CTX);
                }
                _ => {}
            }
            cpu.mem.wr32(FLAGS, host.flags);
            cpu.mem.wr32(PAD, host.pad);
            cpu.mem.wr32(REGIONS, host.regions);
            let mut ctx = FieldCtx::default();
            for (name, off, width) in FIELDS {
                let mut v = rng.next();
                match name {
                    "flags" if halted => v |= 0x400,
                    "flags" if variant >= 5 && variant != 6 => v = (v & !0x400) | 0x0100_0000,
                    // Half the time a seat near the origin, inside the boxes
                    // small operands name.
                    "world_x" | "world_z" if rng.below(2) == 0 => v = rng.below(0x1000),
                    "flags" => v &= !0x0100_0400,
                    "script_id" => v = id,
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
            if cpu.call(VM, &[SCRIPT, *pc as u32, CTX]) != Stop::Returned {
                continue;
            }
            let retail_pc = cpu.regs.r[2] as usize & 0xFFFF;
            let seeded = ctx.clone();
            let port_pc = match step(&mut host, &mut ctx, script, *pc) {
                StepResult::Advance { next_pc } => next_pc,
                StepResult::Yield { resume_pc } => resume_pc,
                StepResult::Halt { final_pc } => final_pc,
                StepResult::Pending { .. } | StepResult::Unknown { .. } => continue,
            };
            ran += 1;
            let mut wrong = Vec::new();
            if port_pc & 0xFFFF != retail_pc {
                wrong.push(format!("pc retail {retail_pc:#x} port {port_pc:#x}"));
            }
            for (name, off, width) in FIELDS {
                let (r, p) = (rd(&mut cpu, off, width), port_field(&ctx, name));
                // A field the port leaves to its host is `field_ctx`'s count.
                if r != p && port_field(&seeded, name) != p {
                    wrong.push(format!("{name} retail {r:#x} port {p:#x}"));
                }
            }
            let flags = cpu.mem.rd32(FLAGS);
            if flags != host.flags {
                wrong.push(format!(
                    "story flags retail {flags:#x} port {:#x}",
                    host.flags
                ));
            }
            // A halt-acquire is also refused on retail when its context runs
            // no script (`+0x94 == 0`) and is not the player (the guard's
            // first clause, `0x801DEE90..0x801DEEA8` for `38`). No context an
            // op addresses is ever in that state - retail gives each one
            // `+0x94 = -1` at creation (`0x8003A464`, `0x8003A8C0`,
            // `0x8003ACCC`) - and the port, whose `saved_pc` is an offset,
            // does not model it, so that one state is not compared. The
            // halted clause is (variants 3 and 4).
            let acquire = matches!(label.as_str(), "37" | "38" | "41" | "47")
                || label.starts_with("43 0")
                || label.starts_with("4C 6")
                || matches!(label.as_str(), "4C 80" | "4C 85" | "4C 8E" | "4C 8F");
            if variant == 6 && acquire {
                continue;
            }
            // The port has no player context: `A2 F8` raises an ExecMove
            // event the player model binds (`leader * 7 + slot + 1`), and
            // `A3 F8` moves the player, so retail's player arms
            // (`0x801DE9A8..0x801DEA84`, `0x801DEC80..0x801DECA8`) are held
            // to the next PC only.
            if matches!(variant, 5 | 7) && matches!(label.as_str(), "22" | "23") {
                wrong.retain(|w| w.starts_with("pc "));
            }
            if !wrong.is_empty() && label != "4C 50" {
                let e = differ.entry(format!("v{variant} {label}")).or_default();
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
    assert!(report.is_empty(), "{ran} runs\n{}", report.join("\n"));
    assert!(ran > 10_000, "{ran}");
}

/// `34 1x` and `34 2x` with the context's attached light present or absent:
/// retail walks the actor list `*0x8007C354` for an `FUN_801E4470` actor
/// whose `+0x90` is the context (`0x801DFF04..0x801DFF48`,
/// `0x801E0020..0x801E0068`). The walk `FUN_8003CF04` is answered here from
/// a generated list of lights, owned by the context or by others; the port's
/// host answers from the same list. Compared: the next PC, and for `34 2x`
/// the keyframe block retail parks on the light's `+0x94` against the block
/// the port hands its host.
#[test]
fn an_attached_light_is_found_where_retail_finds_it() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const HEAD: u32 = 0x8013_0000;
    const LIGHTS: u32 = 0x8013_1000;
    cpu.mem.wr32(0x8007_C354, HEAD);
    // The walk: the head yields the first light, each light the next.
    let count = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let n = count.clone();
    cpu.hook(0x8003_CF04, move |r, _| {
        let i = if r.r[4] == HEAD {
            0
        } else {
            (r.r[4] - LIGHTS) / 0x100 + 1
        };
        r.r[2] = if i < n.get() { LIGHTS + i * 0x100 } else { 0 };
    });
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x4C49_4748);
    let (mut ran, mut found, mut captured) = (0u32, 0u32, 0u32);
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    let mut scripts = corpus();
    scripts.extend(crate::field_vm::synthetic());
    for (script, pc, label) in scripts {
        if !(label.starts_with("34")
            && matches!(script[pc + 1 + usize::from(script[pc] >> 7)] >> 4, 1 | 2))
        {
            continue;
        }
        for _ in 0..4 {
            cpu.mem = clean.clone();
            cpu.regs.r[GP] = gp;
            cpu.mem.load(SCRIPT, &script);
            cpu.mem.wr32(CTX + 0x94, SCRIPT);
            let lights = rng.below(4);
            let mine = (lights > 0 && rng.below(3) != 0).then(|| rng.below(lights));
            count.set(lights);
            for i in 0..lights {
                let owner = if Some(i) == mine { CTX } else { CTX + 0x200 };
                cpu.mem.wr32(LIGHTS + i * 0x100 + 0x90, owner);
                cpu.mem.wr32(LIGHTS + i * 0x100 + 0x94, 0);
            }
            if cpu.call(VM, &[SCRIPT, pc as u32, CTX]) != Stop::Returned {
                continue;
            }
            let retail = cpu.regs.r[2] as usize & 0xFFFF;
            let parked = mine
                .map(|i| cpu.mem.rd32(LIGHTS + i * 0x100 + 0x94))
                .filter(|&p| p != 0);
            let mut host = Seeded {
                flags: 0,
                pad: 0,
                regions: 0,
                compass: [0; 8],
                op49: Op49State::Idle,
                bracket: false,
                light: mine.is_some(),
                captured: None,
                system: std::rc::Rc::new(Vec::new()),
            };
            let mut ctx = FieldCtx::default();
            let port = match step(&mut host, &mut ctx, &script, pc) {
                StepResult::Advance { next_pc } => next_pc,
                StepResult::Yield { resume_pc } => resume_pc,
                StepResult::Halt { final_pc } => final_pc,
                StepResult::Pending { .. } | StepResult::Unknown { .. } => continue,
            };
            ran += 1;
            found += u32::from(mine.is_some());
            let port_block = host
                .captured
                .map(|len| SCRIPT + (script.len() - len) as u32);
            captured += u32::from(parked.is_some());
            let mut wrong = Vec::new();
            if port & 0xFFFF != retail {
                wrong.push(format!("pc retail {retail:#x} port {port:#x}"));
            }
            if parked != port_block {
                wrong.push(format!("block retail {parked:x?} port {port_block:x?}"));
            }
            if !wrong.is_empty() {
                let e = differ.entry(label.clone()).or_default();
                e.0 += 1;
                if e.0 == 1 {
                    e.1 = format!(
                        "pc {pc:#x} bytes {:02x?}: {}",
                        &script[pc..(pc + 10).min(script.len())],
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
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(
        ran > 200 && found > 50 && captured > 5,
        "{ran} {found} {captured}"
    );
}
