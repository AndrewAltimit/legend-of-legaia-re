//! `FUN_801DE840`, the field VM, one instruction at a time over every distinct
//! instruction in the disc's scene scripts, on a context seeded with generated
//! bytes: does `legaia_engine_vm::field::step` leave the context's fields where
//! retail leaves them? `field_vm` asks where the next instruction is; this asks
//! what the instruction did to the context it ran on. Calls outside the field
//! overlay are stubbed on retail's side and the host is `Inert` on the port's,
//! so a field the port leaves to its host is counted, not compared, and must
//! be one of the pairs [`LEFT_TO_HOST`] names: what is held is every store
//! `step` makes itself, and no store retail makes that the port makes nowhere.

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

/// The `(instruction, field)` pairs whose context store `step` leaves to
/// its host (the field differs from retail's and `step` left the seed):
/// a store the port routes to the host by design. Any other such pair is a
/// store retail makes that nothing in the port makes.
const LEFT_TO_HOST: [&str; 35] = [
    "23 flags",
    "23 world_y",
    "38 field_26",
    "3E field_8b",
    "43 00 field_8e",
    "43 0A field_8e",
    "4C 51 field_26",
    "4C 51 field_5e",
    "4C 51 field_8b",
    "4C 51 flags",
    "4C 51 local_flags",
    "4C 51 move_id",
    "4C 51 npc_facing",
    "4C 51 npc_x",
    "4C 51 world_x",
    "4C 51 world_y",
    "4C 51 world_z",
    "4C 81 field_74",
    "4C 81 field_78",
    "4C C3 field_6a",
    "4C C3 field_72",
    "4C C3 field_78",
    "4C C3 field_8b",
    "4C C3 field_8e",
    "4C C3 flags",
    "4C C3 local_flags",
    "4C C3 move_id",
    "4C C3 npc_facing",
    "4C C3 npc_x",
    "4C C3 wait_accum",
    "4C C3 world_x",
    "4C C3 world_z",
    "4C DA world_y",
    "4C E3 field_8b",
    "4C E3 field_8e",
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
    let stray: Vec<&String> = missed
        .keys()
        .filter(|k| !LEFT_TO_HOST.contains(&k.as_str()))
        .collect();
    assert!(
        stray.is_empty(),
        "context stores retail makes that neither step nor its host's share makes: {stray:?}"
    );
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
    let (mut own, mut other, mut player, mut refloored) = (0u32, 0u32, 0u32, 0u32);
    // The floor sample op 23 stores under its gate: the empty World's floor.
    cpu.hook(0x8001_9278, |r, _| r.r[2] = 0);
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in corpus() {
        if label != "23" || (script[pc] & 0x80 != 0 && matches!(script[pc + 1], 0xF8 | 0xFB)) {
            continue;
        }
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        let cross = script[pc] & 0x80 != 0;
        // The player context now and then (`_DAT_8007C364`, the identity the
        // arm compares at `0x801DEC7C`), and the per-scene MAN flag
        // `_DAT_8007B6A8` that gates the floor sample.
        let is_player = !cross && rng.below(3) == 0;
        cpu.mem
            .wr32(0x8007_C364, if is_player { CTX } else { 0x8012_8000 });
        let scene_flag = rng.below(4) == 0;
        cpu.mem.wr8(0x8007_B6A8, u8::from(scene_flag));
        let mut ctx = FieldCtx::default();
        for (name, off, width) in FIELDS {
            let mut v = rng.next();
            match name {
                "flags" => {
                    v &= !0x0100_0400;
                    if is_player {
                        v |= 0x0100_0000;
                    }
                }
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
        w.party.scene_save_allowed = scene_flag;
        w.field_vm.executing_channel = (!is_player).then_some(0);
        player += u32::from(is_player);
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
                "script_id" => continue,
                "flags" => {
                    r &= !2;
                    p &= !2;
                }
                _ => {}
            }
            if name == "world_y" && r == 0 && p == 0 {
                refloored += 1;
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
    assert!(
        own > 100 && other > 500 && player > 30 && refloored > 100,
        "{own} {other} {player} {refloored}"
    );
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
    /// The options screen's event-vibration word `_DAT_800845A8` is set.
    vibration_off: bool,
    /// `FUN_801D65D8`, the staged mode switch, is still stepping.
    switch_stepping: bool,
    /// The camera mover `FUN_8003CF04` finds is up and live.
    camera_glide: bool,
    /// The confirm mask is in this frame's new-press word.
    pressed: bool,
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
        self.camera_glide
    }
}
impl FieldScreenHost for Seeded {
    fn event_vibration_off(&self) -> bool {
        self.vibration_off
    }
}
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
        self.pressed
    }
    fn op4c_n5_sub3_dialog_wait(&mut self, _ctx: &mut FieldCtx) -> bool {
        true
    }
    fn op4c_n5_sub4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        self.switch_stepping
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
    // `43 09`'s seat words with the keep sentinel `-1` (`FUN_8003CE9C`
    // reads `0xFFFF`; `0x801DF7E0` / `0x801DF7F8` skip the store), which no
    // shipped script writes.
    let kept: Vec<_> = corpus
        .iter()
        .filter(|(_, _, label)| label == "43 09")
        .flat_map(|(script, pc, label)| {
            (0..3).map(move |k| {
                let mut s = script.clone();
                if let Some(w) = s.get_mut(pc + 2 + 2 * k..pc + 4 + 2 * k) {
                    w.copy_from_slice(&[0xFF, 0xFF]);
                }
                (s, *pc, label.clone())
            })
        })
        .collect();
    assert!(!kept.is_empty(), "the corpus carries 43 09");
    corpus.extend(kept);
    // `4C 51` with its fourth operand `0x63`, the player arm's clip
    // re-seat (`0x801E196C`), which no shipped script writes.
    let clip: Vec<_> = corpus
        .iter()
        .filter(|(_, _, label)| label == "4C 51")
        .filter_map(|(script, pc, label)| {
            let mut s = script.clone();
            *s.get_mut(pc + 5)? = 0x63;
            Some((s, *pc, label.clone()))
        })
        .collect();
    corpus.extend(clip);
    let mut rng = Rng(0x5345_4544);
    let reruns = std::rc::Rc::new(std::cell::Cell::new(0u32));

    let mut ran = 0u32;
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in &corpus {
        for variant in 0..10u32 {
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
                vibration_off: false,
                switch_stepping: false,
                camera_glide: false,
                pressed: false,
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
                // The globals the corpus leaves clear, which gate host
                // effects only (the heading store under `_DAT_8007B6A8`, the
                // rumble option `_DAT_800845A8`, `_DAT_8007B6AC`'s arm, the
                // pad-octant store under the idle sentinel `_DAT_8007B6B0 ==
                // -1000`) and so leave the next PC and the modelled fields as
                // the port's step leaves them.
                8 => {
                    // Calls the corpus answers with a miss, answered with a
                    // hit: the bag consume finds nothing (`0x100`, `4C 52`'s
                    // unequip fallback), the trigger-block lookup a record
                    // (`4C 83`'s tile write) and the name lookup an actor
                    // (`43`'s flag store), each a host effect at the same PC.
                    cpu.hook(0x8004_2310, |r, _| r.r[2] = 0x100);
                    cpu.hook(0x801D_5630, |r, _| r.r[2] = 0x8016_0000);
                    cpu.hook(0x8003_CF04, |r, _| r.r[2] = 0x8016_0100);
                    // The name lookup is also `4C CD` / `4C D1`'s camera mover,
                    // live with its dead bit `+0x10 & 8` clear, or dead
                    // (`0x801E2B60..0x801E2B6C`).
                    let dead = rng.below(2) == 0;
                    cpu.mem.load(0x8016_0100, &[0u8; 0x20]);
                    cpu.mem.wr32(0x8016_0110, if dead { 8 } else { 0 });
                    host.camera_glide = !dead;
                    // `4C C3`'s placement record (`FUN_8003D0BC`), its seat
                    // bytes with the half-tile bit 7 (`0x801E2754`,
                    // `0x801E2778`); the seat is the host's.
                    // Half the time the placement's first opcode is `0x25`,
                    // which `0x801E2808` runs inline through `FUN_8003CF7C`
                    // (stubbed: the port's re-run is its host's,
                    // `rerun_spawn_section`, gated on the same byte).
                    let rerun = if rng.below(2) == 0 { 0x25 } else { 0x00 };
                    cpu.mem.load(
                        0x8016_0200,
                        &[0x01, 0x00, 0x85, 0x83, rerun, 0x00, 0x00, 0x00],
                    );
                    cpu.hook(0x8003_D0BC, |r, _| r.r[2] = 0x8016_0200);
                    // The context's script base, so the rebased `+0x9E`
                    // (`0x0204`, the header's end less a stubbed `0`
                    // record base) lands on that byte.
                    cpu.mem.wr32(CTX + 0x90, 0x8016_0000);
                    let n = reruns.clone();
                    cpu.hook(0x8003_CF7C, move |_, _| n.set(n.get() + 1));
                    // A confirm pressed this frame: `4C D0`'s prompt takes it
                    // (`0x801E2AE4..0x801E2B04`).
                    cpu.mem.wr32(0x8008_46D0, 0x40);
                    cpu.mem.wr32(0x8007_B874, 0x40);
                    host.pressed = true;
                    host.vibration_off = true;
                    // A player context that is not this one: the player arms
                    // (`0x801E0FB0..`) write it, which the port leaves to its
                    // host.
                    cpu.mem.load(0x8014_0000, &[0u8; 0x100]);
                    cpu.mem.wr32(PLAYER, 0x8014_0000);
                    cpu.mem.wr8(0x8007_B6A8, 1);
                    cpu.mem.wr32(0x8008_45A8, 1);
                    cpu.mem.wr32(0x8007_B6AC, 1);
                    cpu.mem.wr32(0x8007_B6B0, (-1000i32) as u32);
                }
                // The player's own context under the same globals: the
                // player arms of `4C 51` read the clip override
                // `_DAT_8007B6AC` (`0x801E19B4`).
                9 => {
                    // The staged mode switch still stepping: `4C 54` holds
                    // (`0x801E1B10..0x801E1B18`).
                    cpu.hook(0x801D_65D8, |r, _| r.r[2] = 1);
                    host.switch_stepping = true;
                    host.vibration_off = true;
                    cpu.mem.wr32(PLAYER, CTX);
                    cpu.mem.wr8(0x8007_B6A8, 1);
                    cpu.mem.wr32(0x8008_45A8, 1);
                    cpu.mem.wr32(0x8007_B6AC, 1 + rng.below(8));
                    cpu.mem.wr32(0x8007_B6B0, (-1000i32) as u32);
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
            let returned = cpu.call(VM, &[SCRIPT, *pc as u32, CTX]) == Stop::Returned;
            for va in [
                0x8004_2310,
                0x801D_5630,
                0x8003_CF04,
                0x801D_65D8,
                0x8003_D0BC,
                0x8003_CF7C,
            ] {
                cpu.unhook(va);
            }
            if !returned {
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
            if matches!(variant, 5 | 7 | 9) && matches!(label.as_str(), "22" | "23") {
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
    assert!(reruns.get() > 0, "no 4C C3 ran its placement's 0x25 inline");
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
                vibration_off: false,
                switch_stepping: false,
                camera_glide: false,
                pressed: false,
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

/// The field VM's host globals the port's `FieldHost` hooks name one to one:
/// `(address, width)`. A store the hook owns lands here on the port's side,
/// as the hook's documentation spells it.
/// The host-globals pass's scene record (`*0x801C6EA4`) and player context
/// (`*0x8007C364`).
const HG_SCENE: u32 = 0x8013_0000;
const HG_PLAYER: u32 = 0x8014_0000;
/// The script-register table, `0x801C6460 + slot * 2`.
const SLOT_TABLE: u32 = 0x801C_6460;

const HOST_GLOBALS: [(u32, u8); 20] = [
    (0x8007_3EF4, 4), // op 3F, the destination-entry X and Z words
    (0x8007_3EF8, 4),
    (0x8007_3EFC, 4), // op 3F, the arrival facing
    (0x8007_B910, 4), // op 4C 4D, the audio duck level (scaled by 0x8008457C)
    (0x8007_BCD0, 4), // ops 4C 4A..4C, the horizon gate's scale / step / layer
    (0x8007_BCD4, 4),
    (0x8007_BCD8, 4),
    (0x8007_B44C, 4), // op 49 sub 1, the actor handle the menu park keeps
    (0x8007_B628, 2), // op 4C CF, the script camera-focus X
    (0x8007_B62A, 2), // op 4C CF, the focus Z
    (0x8007_B630, 4), // op 4C 84, the shake amplitude
    (0x8007_B6AC, 4), // op 4C CE, the player clip override
    (0x8007_B780, 2), // op 4C 8A, the field light's three angles
    (0x8007_B782, 2),
    (0x8007_B784, 2),
    (0x8007_B788, 4),     // op 4C 8A, the packed back colour
    (0x8007_B854, 4),     // ops 4C 30 / 31, the ambient-particle gate
    (0x8007_BA66, 2),     // op 4C ED, the dither word
    (0x8007_BCAC, 4),     // op 4C 49, the camera-offset accumulator
    (HG_SCENE + 0x4A, 2), // op 4C 49, the scene record's camera offset
];

/// `Inert`, with the hooks whose whole effect is a store to a
/// [`HOST_GLOBALS`] word making that store into `g`.
struct Recording {
    inert: Inert,
    g: BTreeMap<u32, u32>,
    /// The ramps the hooks schedule: `(global, target, ticks)`, as retail's
    /// `FUN_8003C5F0` takes them.
    ramps: Vec<(u32, u32, u32)>,
    /// `_DAT_8008457C`, the duck slot's scale.
    duck_scale: u32,
    /// Stores to words outside [`HOST_GLOBALS`] a hook names by an
    /// operand (op 43 07's rotation row): `address -> (width, value)`.
    rows: BTreeMap<u32, (u8, u32)>,
    /// The scene names op `3F` handed the scene-change packet.
    warps: Vec<String>,
    /// How many of those warps carried a far-half entry byte.
    half_tiles: usize,
    /// The story-flag word `0x1F800394`, seeded alike on retail's side.
    flags: u32,
    /// The player context's `+0x16`, seeded alike on retail's side.
    player_y: i16,
    /// The script-register table `0x801C6460` (s16 per slot), seeded alike.
    slots: Vec<i16>,
}
impl Recording {
    fn put(&mut self, at: u32, v: u32) {
        self.g.insert(at, v);
    }
}
impl FieldFlagsHost for Recording {
    fn global_flags(&self) -> u32 {
        self.flags
    }
    fn set_global_flags(&mut self, value: u32) {
        self.flags = value;
    }
    fn frame_delta(&self) -> u16 {
        self.inert.frame_delta()
    }
    fn slot_table_read(&self, slot: u8) -> i16 {
        self.slots[usize::from(slot)]
    }
    // `4C CA` / `CB` / `CC` (`0x801E2930..0x801E29DC`): a slot set, or moved
    // by the operand (the frame step for `0xFFFF`), as a 16-bit store.
    fn slot_table_write(&mut self, slot: u8, value: i16) {
        self.slots[usize::from(slot)] = value;
        let at = SLOT_TABLE + 2 * u32::from(slot);
        self.rows.insert(at, (2, value as u16 as u32));
    }
    fn slot_table_adjust(&mut self, slot: u8, delta: i16, subtract: bool) {
        let old = self.slots[usize::from(slot)];
        let new = if subtract {
            old.wrapping_sub(delta)
        } else {
            old.wrapping_add(delta)
        };
        self.slot_table_write(slot, new);
    }
}
impl FieldSceneHost for Recording {
    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8, dir: u8) {
        // `0x801DEB1C..0x801DEBAC`: the scene-change packet, then the entry
        // words and the facing the scene host arms from these bytes.
        use legaia_engine_vm::field::dispatch_leaves::{warp_arrival_facing, warp_entry_coord};
        self.warps.push(scene.to_string());
        self.put(0x8007_3EF4, warp_entry_coord(entry_x) as u16 as u32);
        self.put(0x8007_3EF8, warp_entry_coord(entry_z) as u16 as u32);
        self.put(0x8007_3EFC, i32::from(warp_arrival_facing(dir)) as u32);
        self.half_tiles += usize::from((entry_x | entry_z) & 0x80 != 0);
    }
}
impl FieldTerrainHost for Recording {}
impl FieldCameraHost for Recording {
    fn set_camera_focus_override(&mut self, x_global: i16, z_global: i16) {
        self.put(0x8007_B628, x_global as u16 as u32);
        self.put(0x8007_B62A, z_global as u16 as u32);
    }
}
impl FieldCameraMotionHost for Recording {
    fn camera_glide_live(&mut self) -> bool {
        self.inert.camera_glide_live()
    }
    fn set_shake_amplitude(&mut self, value: u8) {
        self.put(0x8007_B630, value.into());
    }
    // `4C 49` (`0x801E1480..0x801E162C`): the scene record's `+0x4A` and the
    // accumulator `_DAT_8007BCAC`, written or ramped on the story-flag bits.
    fn write_camera_offset(&mut self, target: i16) {
        self.put(HG_SCENE + 0x4A, target as u16 as u32);
    }
    fn ramp_camera_offset(&mut self, target: i16, ticks: u16) {
        self.ramps.push((
            HG_SCENE + 0x4A,
            i32::from(target) as u32,
            i32::from(ticks as i16) as u32,
        ));
    }
    fn write_camera_offset_player_relative(&mut self, target: i16, ticks: u16) {
        // Bit 24: `BCAC = target`, `+0x4A = target + player+0x16`.
        let raised = i32::from(target) + i32::from(self.player_y);
        if ticks == 0 {
            self.put(0x8007_BCAC, i32::from(target) as u32);
            self.put(HG_SCENE + 0x4A, raised as u16 as u32);
        } else {
            let t = i32::from(ticks as i16) as u32;
            self.ramps.push((0x8007_BCAC, i32::from(target) as u32, t));
            self.ramps.push((HG_SCENE + 0x4A, raised as u32, t));
        }
    }
    fn write_camera_offset_delta(&mut self, target: i16, ticks: u16) {
        // Bit 25: `+0x4A = target`, `BCAC = target - player+0x16`.
        let delta = i32::from(target) - i32::from(self.player_y);
        if ticks == 0 {
            self.put(HG_SCENE + 0x4A, target as u16 as u32);
            self.put(0x8007_BCAC, delta as u32);
        } else {
            let t = i32::from(ticks as i16) as u32;
            self.ramps.push((0x8007_BCAC, delta as u32, t));
            self.ramps
                .push((HG_SCENE + 0x4A, i32::from(target) as u32, t));
        }
    }
}
impl FieldScreenHost for Recording {
    fn write_or_ramp_global_slot(&mut self, sub: u8, target: i32, ticks: u16) {
        // `0x801E1630..0x801E1768`: subs `A..D` write their word at once
        // with no ticks, or hand `FUN_8003C5F0` a ramp; `D` scales the
        // target by `_DAT_8008457C` (`mult`, `srl 0xc`).
        let (at, value) = match sub {
            0xA => (0x8007_BCD0, target as u32),
            0xB => (0x8007_BCD4, target as u32),
            0xC => (0x8007_BCD8, target as u32),
            0xD => (
                0x8007_B910,
                (target as u32).wrapping_mul(self.duck_scale) >> 12,
            ),
            _ => return,
        };
        if ticks == 0 {
            self.put(at, value);
        } else {
            // The ticks word is `FUN_8003CE9C`'s sign-extended `s16`.
            self.ramps.push((at, value, i32::from(ticks as i16) as u32));
        }
    }
    fn set_ambient_particle_gate(&mut self, enabled: bool) {
        self.put(0x8007_B854, enabled.into());
    }
    fn set_field_light(&mut self, slots: [i16; 3], packed: u32) {
        for (k, s) in slots.iter().enumerate() {
            self.put(0x8007_B780 + 2 * k as u32, *s as u16 as u32);
        }
        self.put(0x8007_B788, packed);
    }
    fn set_dither(&mut self, value: u8) {
        self.put(0x8007_BA66, value.into());
    }
}
impl FieldAudioHost for Recording {
    fn bgm_ready(&mut self, sub_op: u8) -> bool {
        self.inert.bgm_ready(sub_op)
    }
}
impl FieldPartyInventoryHost for Recording {
    fn op4c_nd_subc_item_held(&self, needle: u8) -> bool {
        self.inert.op4c_nd_subc_item_held(needle)
    }
}
impl FieldDialogMenuHost for Recording {
    fn op49_arm(&mut self, sub_op: u8, _pc: usize, field_90: u32) {
        // `sw v1,-0x4bb4(v0)` at `0x801E09C8`, on sub-op 1 alone; the park
        // word `_DAT_8007B450` itself is a RAM pointer the port does not keep.
        if sub_op == 1 {
            self.put(0x8007_B44C, field_90);
        }
    }
    fn press_prompt(&mut self, x: u16, y: u16) -> bool {
        self.inert.press_prompt(x, y)
    }
    fn op4c_n5_sub3_dialog_wait(&mut self, ctx: &mut FieldCtx) -> bool {
        self.inert.op4c_n5_sub3_dialog_wait(ctx)
    }
}
impl FieldActorHost for Recording {
    fn actor_face_rotation_setup(
        &mut self,
        _ctx: &mut FieldCtx,
        face_id: u8,
        payload_4: u32,
        params: [u16; 4],
        target: i16,
    ) {
        // `0x801DF6F8..0x801DF7A0`: the 12-byte row at `0x8007BE60 +
        // face * 12` (four halfwords, then the packed word), then a ramp of
        // `+0x7A` from 0 to `0x1000` over `target` ticks, or back down over
        // `-target`.
        let row = 0x8007_BE60 + u32::from(face_id) * 12;
        for (k, p) in params.iter().enumerate() {
            self.rows.insert(row + 2 * k as u32, (2, u32::from(*p)));
        }
        self.rows.insert(row + 8, (4, payload_4));
        let (to, ticks) = if target > 0 {
            (0x1000, i32::from(target))
        } else {
            (0, -i32::from(target))
        };
        self.ramps.push((CTX + 0x7A, to, ticks as u32));
    }
    fn ramp_ctx_slot(&mut self, _ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        // The nibble-4 context slots' ramps (`0x801E118C..0x801E12A0`, sub 8
        // beside them): the scheduler takes the slot's address, the target
        // sign-extended and the ticks.
        let off = match sub {
            0 => 0x72,
            1 => 0x6A,
            2 => 0x8E,
            3 => 0x24,
            4 => 0x28,
            8 => 0x26,
            _ => return,
        };
        self.ramps.push((
            CTX + off,
            i32::from(target) as u32,
            i32::from(ticks as i16) as u32,
        ));
    }
}
impl FieldActorSpawnHost for Recording {
    fn actor_guard(&mut self, actor_id: u8) -> Option<()> {
        self.inert.actor_guard(actor_id)
    }
}
impl FieldActorFxHost for Recording {
    fn set_actor_tint(
        &mut self,
        ctx: &mut FieldCtx,
        _target: Option<u8>,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        // `0x801E1FC4..0x801E2068`: with ticks, a blend at rest tweens
        // `+0x78` alone; otherwise the colour `+0x74` tweens first (skipped
        // for a zero blend), then `+0x78`.
        if ticks == 0 {
            return;
        }
        let blend_word = i32::from(blend as i16) as u32;
        if ctx.field_78 != 0 && blend != 0 {
            self.ramps
                .push((CTX + 0x74, colour, i32::from(ticks as i16) as u32));
        }
        self.ramps
            .push((CTX + 0x78, blend_word, i32::from(ticks as i16) as u32));
    }
    fn spawn_attached_light(
        &mut self,
        ctx: &FieldCtx,
        ext: Option<u8>,
        spawn: &legaia_engine_vm::field_actor_billboard::AttachedSpriteSpawn,
        script: Option<&[u8]>,
    ) -> bool {
        self.inert.spawn_attached_light(ctx, ext, spawn, script)
    }
}
impl FieldPlayerHost for Recording {
    fn set_player_clip_override(&mut self, value: u8) {
        self.put(0x8007_B6AC, value.into());
    }
}
impl FieldHaltHost for Recording {}

/// The host half of [`every_instruction_leaves_its_context_as_retail_leaves_it`]:
/// over the same corpus, on a seeded context, do the globals the field VM
/// writes outside the context (every [`HOST_GLOBALS`] word, seeded at random
/// on both sides) end where retail leaves them? Retail's side runs the
/// dispatcher with its SCUS calls stubbed (but the operand readers); the port's runs `step` on a
/// [`Recording`] host whose hooks store what their documentation says the
/// arm stores. A word no hook touched must keep its seed, so a store the
/// port routes nowhere fails as surely as a wrong value.
#[test]
fn every_instruction_leaves_the_host_globals_as_retail_leaves_them() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The ramp scheduler: `(global, target, ticks)` of each ramp the
    // dispatcher schedules on a hook-owned word.
    let ramps: std::rc::Rc<std::cell::RefCell<Vec<(u32, u32, u32)>>> = Default::default();
    {
        let ramps = ramps.clone();
        cpu.hook(0x8003_C5F0, move |r, m| {
            let at = r.r[5];
            if HOST_GLOBALS.iter().any(|g| g.0 == at) || (CTX..CTX + 0x100).contains(&at) {
                let sp = r.r[29];
                ramps
                    .borrow_mut()
                    .push((at, m.rd32(sp + 0x10), m.rd32(sp + 0x14)));
            }
        });
    }
    // The name-based scene-change packet `FUN_8001FD44(name, index)` op 3F
    // hands its copied name: the names, in call order.
    let packets: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();
    {
        let packets = packets.clone();
        cpu.hook(0x8001_FD44, move |r, m| {
            let at = r.r[4];
            let name: Vec<u8> = (0..32)
                .map(|k| m.rd8(at + k))
                .take_while(|&b| b != 0)
                .collect();
            packets
                .borrow_mut()
                // The resolver `FUN_8001D7F8` lower-cases it; so does the
                // port's name gate.
                .push(String::from_utf8_lossy(&name).to_ascii_lowercase());
        });
    }
    let (mut warped, mut half_tiles) = (0usize, 0usize);
    // The 32-bit operand reader op 43 07 loads its packed word through.
    cpu.hook(0x8003_CED8, |r, m| {
        let a = r.r[4];
        r.r[2] = (0..4).fold(0, |w, k| w | u32::from(m.rd8(a + k)) << (8 * k));
    });
    // The 24-bit operand reader op 4C 8A loads its packed colour through.
    cpu.hook(0x8003_CEB8, |r, m| {
        let a = r.r[4];
        r.r[2] = u32::from(m.rd8(a)) | u32::from(m.rd8(a + 1)) << 8 | u32::from(m.rd8(a + 2)) << 16;
    });
    cpu.mem.wr32(0x801C_6EA4, HG_SCENE);
    cpu.mem.wr32(0x8007_C364, HG_PLAYER);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    // The disc corpus, then every op over generated operand bytes, so an
    // arm only a mutated operand reaches (a ramp's non-zero ticks, an
    // all-zero colour) stores where retail stores. A generated `3F` is left
    // out: its name bytes are not a CDNAME label, which the port's name gate
    // refuses (a phantom `3F` inside message text must not warp) and retail
    // hands the packet as they are.
    let mut corpus = corpus();
    corpus.extend(
        crate::field_vm::synthetic()
            .into_iter()
            .filter(|(script, _, _)| script.first() != Some(&0x3F)),
    );
    let mut rng = Rng(0x484F_5354);
    let read = |cpu: &mut psx_retail_exec::Cpu, at: u32, w: u8| {
        if w == 2 {
            u32::from(cpu.mem.rd16(at))
        } else {
            cpu.mem.rd32(at)
        }
    };
    let (mut ran, mut stored) = (0u32, BTreeMap::<String, u32>::new());
    let mut ramped = 0usize;
    let mut rows_held = 0usize;
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
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
            let handle = rng.next();
            cpu.mem.wr32(CTX + 0x90, handle);
            ctx.field_90 = handle;
            let mut host = Recording {
                inert: Inert { hit: true },
                g: BTreeMap::new(),
                ramps: Vec::new(),
                duck_scale: cpu.mem.rd32(0x8008_457C),
                rows: BTreeMap::new(),
                warps: Vec::new(),
                half_tiles: 0,
                // Only `4C 49`'s two bits: the other flag readers answer
                // from the same word on both sides, clear as before.
                flags: [0, 0x0100_0000, 0x0200_0000, 0x0300_0000][rng.below(4) as usize],
                player_y: rng.next() as i16,
                slots: (0..0x100).map(|_| rng.next() as i16).collect(),
            };
            for (i, &v) in host.slots.iter().enumerate() {
                cpu.mem.wr16(SLOT_TABLE + 2 * i as u32, v as u16);
            }
            cpu.mem.wr32(0x1F80_0394, host.flags);
            cpu.mem.wr16(HG_PLAYER + 0x16, host.player_y as u16);
            ramps.borrow_mut().clear();
            packets.borrow_mut().clear();
            let mut seeds = BTreeMap::new();
            for (at, w) in HOST_GLOBALS {
                let v = if w == 2 {
                    rng.next() & 0xFFFF
                } else {
                    rng.next()
                };
                if w == 2 {
                    cpu.mem.wr16(at, v as u16);
                } else {
                    cpu.mem.wr32(at, v);
                }
                seeds.insert(at, v);
            }
            if cpu.call(VM, &[SCRIPT, *pc as u32, CTX]) != Stop::Returned {
                continue;
            }
            let res = step(&mut host, &mut ctx, script, *pc);
            if matches!(res, StepResult::Pending { .. } | StepResult::Unknown { .. }) {
                continue;
            }
            ran += 1;
            let mut wrong = Vec::new();
            for (at, w) in HOST_GLOBALS {
                let r = read(&mut cpu, at, w);
                let p = host.g.get(&at).copied().unwrap_or(seeds[&at]);
                if r != seeds[&at] {
                    *stored.entry(format!("{label} {at:08X}")).or_default() += 1;
                }
                if r != p {
                    wrong.push(format!(
                        "{at:08X} retail {r:#x} port {p:#x} seed {:#x}",
                        seeds[&at]
                    ));
                }
            }
            for (&at, &(w, v)) in &host.rows {
                let r = read(&mut cpu, at, w);
                if r != v {
                    wrong.push(format!("{at:08X} retail {r:#x} port {v:#x}"));
                }
            }
            rows_held += host.rows.len();
            let retail_ramps = ramps.borrow().clone();
            if retail_ramps != host.ramps {
                wrong.push(format!(
                    "ramps retail {retail_ramps:x?} port {:x?}",
                    host.ramps
                ));
            }
            ramped += usize::from(!retail_ramps.is_empty());
            let retail_packets = packets.borrow().clone();
            if retail_packets != host.warps {
                wrong.push(format!(
                    "scene packet retail {retail_packets:?} port {:?}",
                    host.warps
                ));
            }
            warped += host.warps.len();
            half_tiles += host.half_tiles;
            if !wrong.is_empty() {
                let e = differ.entry(label.clone()).or_default();
                e.0 += 1;
                if e.0 == 1 {
                    e.1 = format!(
                        "pc {pc:#x} bytes {:02x?}: {}",
                        &script[*pc..(*pc + 12).min(script.len())],
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
    eprintln!("{ran} runs; host-global stores: {stored:?}");
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(ran > 4000, "{ran}");
    assert!(ramped > 0, "no instruction ramps a hook-owned word");
    assert!(rows_held > 0, "no rotation row held");
    eprintln!("{warped} scene changes, {half_tiles} with a far-half entry byte");
    assert!(warped > 50, "{warped} scene changes");
    // Every hook's word is stored by some instruction of the corpus.
    for (at, _) in HOST_GLOBALS {
        assert!(
            stored.keys().any(|k| k.ends_with(&format!("{at:08X}"))),
            "no instruction stores {at:08X}"
        );
    }
}
