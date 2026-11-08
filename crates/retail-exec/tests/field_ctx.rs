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

#[test]
fn every_instruction_leaves_its_context_as_retail_leaves_it() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
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
                    *missed.entry(label.clone()).or_default() += 1;
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
