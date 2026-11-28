//! `FUN_801DE840`, the field VM, on the arms the one-context differentials
//! cannot tell apart: an arm that branches on whether an actor lookup
//! (`FUN_8003C83C`) found anything, on whether the target is the player, or on
//! a flag whose only effect is a call into the executable. `field_ctx` and
//! `field_vm` answer every lookup with the context under test and stub every
//! call outside the overlay, so both directions of such a branch write the
//! same context. Here the retail side runs with a second context, a seeded
//! actor table behind the lookup, and the callees it reaches recorded with
//! their arguments; the port's side is `step_with_caller` on two contexts or
//! the real `World` with the same actors seated, and what the World did is
//! held against the calls retail made.

use crate::common::Rng;
use crate::field_ctx::{FIELDS, LEFT_TO_HOST, port_field, retail_vm, seat};
use crate::field_vm::{Inert, corpus};
use legaia_engine_vm::field::*;
use psx_retail_exec::{Cpu, GP, Stop};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

const VM: u32 = 0x801D_E840;
const SCRIPT: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;
/// The second context: the target a prefixed instruction resolves to.
const CTX2: u32 = 0x8012_8000;
const RESOLVE_ACTOR: u32 = 0x8003_C83C;
const PLAYER_PTR: u32 = 0x8007_C364;

fn rd_at(cpu: &mut Cpu, base: u32, off: u32, width: u8) -> u32 {
    match width {
        1 => cpu.mem.rd8(base + off).into(),
        2 => cpu.mem.rd16(base + off).into(),
        _ => cpu.mem.rd32(base + off),
    }
}

fn wr_at(cpu: &mut Cpu, base: u32, off: u32, width: u8, v: u32) {
    match width {
        1 => cpu.mem.wr8(base + off, v as u8),
        2 => cpu.mem.wr16(base + off, v as u16),
        _ => cpu.mem.wr32(base + off, v),
    }
}

/// Seed every modelled field of the context at `base` from `rng`, mirrored
/// into a `FieldCtx`.
fn seed_ctx(cpu: &mut Cpu, base: u32, rng: &mut Rng, halted: bool) -> FieldCtx {
    let mut ctx = FieldCtx::default();
    for (name, off, width) in FIELDS {
        let mut v = rng.next();
        if name == "flags" {
            v &= !0x400;
            if halted {
                v |= 0x400;
            }
        }
        if width == 1 {
            v &= 0xFF;
        } else if width == 2 {
            v &= 0xFFFF;
        }
        wr_at(cpu, base, off, width, v);
        seat(&mut ctx, name, v);
    }
    ctx
}

/// The fields of the context at `base` that differ from the port's `c`,
/// `skip` naming fields the comparison leaves out.
fn ctx_diff(cpu: &mut Cpu, base: u32, c: &FieldCtx, skip: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, off, width) in FIELDS {
        if skip(name) {
            continue;
        }
        let (r, p) = (rd_at(cpu, base, off, width), port_field(c, name));
        if r != p {
            wrong.push(format!("{name} retail {r:#x} port {p:#x}"));
        }
    }
    wrong
}

/// Every prefixed instruction of the disc corpus with the target resolving
/// to a second context, the player or not: does `step_with_caller` leave
/// both contexts where retail leaves them? The acquiring arms (`37` / `41`,
/// `47`, the `43` arcs and walks, `4C 8x`'s halt-acquire family) seat the op
/// on the target and, when the target is the player `_DAT_8007C364`, park
/// the caller as well (`bne s5,v0` at `0x801DEEFC`, `0x801DF02C`,
/// `0x801DF3F0`, `0x801E1C2C`, `0x801E21B4`, ...): with one context the two
/// directions write the same words. The parked `+0x94` is the opcode's
/// address on both contexts, compared as the offset `saved_pc` holds.
#[test]
fn a_player_target_parks_the_caller_as_retail_parks_it() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = CTX2);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5041_524B);
    let (mut ran, mut parked_both, mut parked_target) = (0u32, 0u32, 0u32);
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in corpus() {
        // `4C 50`'s writes are `FUN_80024E08`'s, a SCUS call the stub world
        // skips; `4C 37` copies the player's pose, which `Inert` has none of
        // (`every_instruction_under_seeded_state_steps_as_retail_steps` seats
        // one).
        if script[pc] & 0x80 == 0 || label == "4C 50" || label == "4C 37" {
            continue;
        }
        for round in 0..2 {
            let is_player = round == 1;
            cpu.mem = clean.clone();
            cpu.regs.r[GP] = gp;
            cpu.mem.load(SCRIPT, &script);
            cpu.mem.wr32(CTX + 0x94, SCRIPT);
            let mut caller = seed_ctx(&mut cpu, CTX, &mut rng, false);
            // A header naming the caller's own id runs on the caller without
            // a resolve, and the system script (`+0x50 == 0xFB`) runs a
            // halted target's op anyway (`0x801DE938..0x801DE940`): both are
            // `World`'s routing (`step_on_resolved_channel`), not the VM's.
            if caller.script_id == u16::from(script[pc + 1]) || caller.script_id == 0xFB {
                caller.script_id = 0xFA - u16::from(script[pc + 1] == 0xFA);
                cpu.mem.wr16(CTX + 0x50, caller.script_id);
            }
            let halted = rng.below(4) == 0;
            let mut target = seed_ctx(&mut cpu, CTX2, &mut rng, halted);
            // A live target's `+0x94` is never zero (`-1` at creation);
            // the saved PCs start on a sentinel both sides know.
            cpu.mem.wr32(CTX2 + 0x94, 0xFFFF_FFFF);
            caller.saved_pc = 0xDEAD;
            target.saved_pc = 0xDEAD;
            cpu.mem
                .wr32(PLAYER_PTR, if is_player { CTX2 } else { 0x8012_C000 });
            if cpu.call(VM, &[SCRIPT, pc as u32, CTX]) != Stop::Returned {
                continue;
            }
            let retail_pc = cpu.regs.r[2] as usize & 0xFFFF;
            let res = step_with_caller(
                &mut Inert { hit: true },
                &mut target,
                &mut caller,
                is_player,
                &script,
                pc,
            );
            let port_pc = match res {
                StepResult::Advance { next_pc } => next_pc,
                StepResult::Yield { resume_pc } => resume_pc,
                StepResult::Halt { final_pc } => final_pc,
                StepResult::Pending { .. } | StepResult::Unknown { .. } => continue,
            };
            ran += 1;
            let mut wrong = Vec::new();
            if retail_pc != port_pc & 0xFFFF {
                wrong.push(format!("pc retail {retail_pc:#x} port {port_pc:#x}"));
            }
            // The caller: the park, and nothing else.
            wrong.extend(
                ctx_diff(&mut cpu, CTX, &caller, &|_| false)
                    .into_iter()
                    .map(|w| format!("caller {w}")),
            );
            // The target, but a field the port leaves to its host and the
            // player's own arms (`is_player_context` answers `false` here).
            let host_owned =
                |name: &str| LEFT_TO_HOST.contains(&format!("{label} {name}").as_str());
            if !is_player {
                wrong.extend(
                    ctx_diff(&mut cpu, CTX2, &target, &host_owned)
                        .into_iter()
                        .map(|w| format!("target {w}")),
                );
            }
            for (who, base, c) in [("caller", CTX, &caller), ("target", CTX2, &target)] {
                let r = cpu.mem.rd32(base + 0x94);
                let seed = if base == CTX { SCRIPT } else { 0xFFFF_FFFF };
                let retail = (r != seed).then(|| r.wrapping_sub(SCRIPT));
                let port = (c.saved_pc != 0xDEAD).then_some(c.saved_pc);
                if retail != port {
                    wrong.push(format!("{who} +0x94 retail {retail:x?} port {port:x?}"));
                }
            }
            let caller_parked = cpu.mem.rd32(CTX + 0x10) & 0x400 != 0;
            let target_parked = cpu.mem.rd32(CTX2 + 0x94) != 0xFFFF_FFFF;
            parked_both += u32::from(caller_parked && target_parked);
            parked_target += u32::from(!caller_parked && target_parked);
            if !wrong.is_empty() {
                let e = differ
                    .entry(format!("{label} player {is_player}"))
                    .or_default();
                e.0 += 1;
                if e.0 == 1 {
                    e.1 = format!(
                        "pc {pc:#x} bytes {:02x?}: {}",
                        &script[pc..(pc + 8).min(script.len())],
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
    assert!(report.is_empty(), "{ran} ran\n{}", report.join("\n"));
    assert!(
        ran > 2000 && parked_both > 100 && parked_target > 100,
        "{ran} {parked_both} {parked_target}"
    );
}

/// `4C E4`, the world-unit box test (`0x801E31C0..0x801E3288`), with the
/// actor seated on and around each corner: the half-tile bit of each
/// corner byte moves that corner by `0x40` (`beq` at `0x801E3204`,
/// `0x801E3214`, `0x801E3220`, `0x801E322C`), which a random 16-bit seat
/// almost never lands between. Compared: the next PC (inside, or the
/// relative skip).
#[test]
fn a_box_test_takes_each_corners_half_tile_as_retail_takes_it() {
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x4243_4F58);
    let (mut ran, mut inside) = (0u32, 0u32);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for _ in 0..20_000 {
        let b: [u8; 4] = std::array::from_fn(|_| rng.next() as u8);
        let lo = |b: u8| (i32::from(b & 0x7F) << 7) + if b & 0x80 != 0 { 0x60 } else { 0x20 };
        let hi = |b: u8| (i32::from(b & 0x7F) << 7) + if b & 0x80 != 0 { 0xA0 } else { 0x60 };
        // A seat on one of the four corners, give or take, either axis.
        let near = |rng: &mut Rng, c: i32| c + rng.below(0x90) as i32 - 0x48;
        let x = match rng.below(3) {
            0 => near(&mut rng, lo(b[0])),
            1 => near(&mut rng, hi(b[2])),
            _ => rng.next() as i16 as i32,
        };
        let z = match rng.below(3) {
            0 => near(&mut rng, lo(b[1])),
            1 => near(&mut rng, hi(b[3])),
            _ => rng.next() as i16 as i32,
        };
        let skip = rng.below(0x40) as u16 + 2;
        let mut script = vec![0x4C, 0xE4, b[0], b[1], b[2], b[3]];
        script.extend_from_slice(&skip.to_le_bytes());
        script.extend_from_slice(&[0x21; 0x48]);
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        cpu.mem.wr16(CTX + 0x14, x as u16);
        cpu.mem.wr16(CTX + 0x18, z as u16);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let retail = cpu.regs.r[2] as usize & 0xFFFF;
        let mut ctx = FieldCtx {
            world_x: x as u16,
            world_z: z as u16,
            ..FieldCtx::default()
        };
        let port = match step(&mut Inert { hit: true }, &mut ctx, &script, 0) {
            StepResult::Advance { next_pc } => next_pc,
            other => panic!("{script:02x?}: {other:?}"),
        };
        ran += 1;
        inside += u32::from(retail == 8);
        if retail != port {
            *differ
                .entry(format!(
                    "{:02x?} at ({x}, {z}): retail {retail} port {port}",
                    &script[..8]
                ))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(12).cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(inside > 1000 && ran - inside > 1000, "{ran} {inside}");
}

/// `4B count base (up down)*count`, the VDF morph-lane arm
/// (`0x801E0820..0x801E08C0`), stepped by the real `World` as a placement's
/// own op, against the actor record retail writes: per lane `i < count` the
/// sub-entry byte `+0xB0 + i`, the ramp velocities `+0xB8 + 2i` /
/// `+0xC8 + 2i` and the zero weight `+0xA0 + 2i`, and no lane at or past
/// the count (a zero count skips the loop at `0x801E082C`, which writes the
/// lanes as a do-while), over a seeded record. The port's lanes are the
/// placement's `World::npcs.morphs` state. Counts run `0..=8`, the lanes the
/// record has.
#[test]
fn a_morph_arm_writes_the_lanes_retail_writes() {
    use legaia_engine_core::world::{MorphOwner, World};
    use legaia_engine_vm::vdf_morph::keyframe_weight;
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x4D4F_5246);
    let (mut ran, mut empty) = (0u32, 0u32);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for round in 0..3000 {
        let count = if round % 5 == 0 {
            0
        } else {
            rng.below(9) as u8
        };
        let base = rng.next() as u8;
        let mut script = vec![0x4B, count, base];
        for _ in 0..count * 4 {
            script.push(rng.next() as u8);
        }
        script.push(0x21);
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        let seed: Vec<u8> = (0..0x38).map(|_| rng.next() as u8).collect();
        cpu.mem.load(CTX + 0xA0, &seed);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let mut w = World::new();
        w.field_vm.executing_channel = Some(5);
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        let _ = w.step_field();
        let st = w
            .npcs
            .morphs
            .get(&MorphOwner::Placement(5))
            .cloned()
            .unwrap_or_default();
        ran += 1;
        empty += u32::from(count == 0);
        let mut wrong = Vec::new();
        for i in 0..8usize {
            let r_id = cpu.mem.rd8(CTX + 0xB0 + i as u32);
            let r_up = cpu.mem.rd16(CTX + 0xB8 + 2 * i as u32);
            let r_down = cpu.mem.rd16(CTX + 0xC8 + 2 * i as u32);
            let r_w = cpu.mem.rd16(CTX + 0xA0 + 2 * i as u32);
            let want = if i < usize::from(count) {
                (
                    st.anim_block_u8(0x04 + i),
                    st.anim_block_u16(0x0C + 2 * i),
                    st.anim_block_u16(0x1C + 2 * i),
                    keyframe_weight(&st, i),
                )
            } else {
                let h = |o: usize| u16::from_le_bytes([seed[o], seed[o + 1]]);
                (seed[0x10 + i], h(0x18 + 2 * i), h(0x28 + 2 * i), h(2 * i))
            };
            if (r_id, r_up, r_down, r_w) != want {
                wrong.push(format!(
                    "lane {i} retail {:x?} port {want:x?}",
                    (r_id, r_up, r_down, r_w)
                ));
            }
        }
        if st.keyframe_count != count || cpu.mem.rd8(CTX + 0x6C) != count {
            wrong.push(format!(
                "count retail {} port {}",
                cpu.mem.rd8(CTX + 0x6C),
                st.keyframe_count
            ));
        }
        if !wrong.is_empty() {
            *differ
                .entry(format!("{:02x?}: {}", &script[..3], wrong.join(", ")))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(12).cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(ran > 2000 && empty > 500, "{ran} {empty}");
}

/// `4C 46` / `4C 47`, the rumble motor words `_DAT_8007B92C` /
/// `_DAT_8007B930`, stepped by the real `World`: under the options screen's
/// Events row Off (`_DAT_800845A8 != 0`) both words clear
/// (`0x801E13F8..0x801E1404`); otherwise a zero tick count stores the target
/// and any other hands the ramp scheduler `FUN_8003C5F0` the word, the
/// target and the ticks (`bne s2,zero` at `0x801E1408`, `0x801E13A4`).
/// Compared: both words against `World::rumble.motors`, and the ramps
/// retail schedules (the scheduler hooked) against the ones the World runs.
#[test]
fn a_rumble_write_moves_the_motor_words_as_retail_moves_them() {
    use legaia_engine_core::world::World;
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const PULSE: u32 = 0x8007_B92C;
    const LEVEL: u32 = 0x8007_B930;
    const EVENTS_OFF: u32 = 0x8008_45A8;
    let ramps: Rc<RefCell<Vec<(u32, i32, i32)>>> = Default::default();
    {
        let ramps = ramps.clone();
        cpu.hook(0x8003_C5F0, move |r, m| {
            let sp = r.r[29];
            ramps
                .borrow_mut()
                .push((r.r[5], m.rd32(sp + 0x10) as i32, m.rd32(sp + 0x14) as i32));
        });
    }
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5255_4D42);
    let (mut ran, mut ramped, mut cleared) = (0u32, 0u32, 0u32);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for _ in 0..4000 {
        let sub = 6 + rng.below(2) as u8;
        let target = match rng.below(4) {
            0 => 0,
            1 => 0x7FFF,
            2 => 0x8000 | rng.below(0x8000) as u16,
            _ => rng.next() as u16,
        };
        let ticks = match rng.below(3) {
            0 => 0,
            1 => 1 + rng.below(0x40) as u16,
            _ => rng.next() as u16,
        };
        let off = rng.below(3) == 0;
        let (pulse, level) = (rng.next() as i32, rng.next() as i32);
        let mut script = vec![0x4C, 0x40 | sub];
        script.extend_from_slice(&target.to_le_bytes());
        script.extend_from_slice(&ticks.to_le_bytes());
        script.push(0x21);
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        cpu.mem.wr32(PULSE, pulse as u32);
        cpu.mem.wr32(LEVEL, level as u32);
        cpu.mem.wr32(EVENTS_OFF, u32::from(off));
        ramps.borrow_mut().clear();
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let mut w = World::new();
        w.toggles.retail_options.vibration_events = u8::from(off);
        w.rumble.motors.pulse = pulse;
        w.rumble.motors.level = level;
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        let _ = w.step_field();
        ran += 1;
        let retail = (
            cpu.mem.rd32(PULSE) as i32,
            cpu.mem.rd32(LEVEL) as i32,
            ramps.borrow().clone(),
        );
        let port = (
            w.rumble.motors.pulse,
            w.rumble.motors.level,
            w.drive_rumble_ramps(),
        );
        ramped += u32::from(!retail.2.is_empty());
        cleared += u32::from(off);
        if retail != port {
            *differ
                .entry(format!(
                    "{:02x?} off {off}: retail {retail:x?} port {port:x?}",
                    &script[..6]
                ))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(12).cloned().collect();
    assert!(
        report.is_empty(),
        "{} differ\n{}",
        differ.len(),
        report.join("\n")
    );
    assert!(
        ran > 3000 && ramped > 1000 && cleared > 500,
        "{ran} {ramped} {cleared}"
    );
}

/// The address the seeded actor table answers `id` with.
fn actor_at(id: u8) -> u32 {
    0x8013_0000 + 0x100 * u32::from(id)
}

/// The three arms that resolve an operand-named actor themselves and call
/// the overlay's spawner only on a hit, stepped by the real `World` with
/// the same actors seated as placement channels (and the player for
/// `0xF8`): `43 02`'s three talkers (`FUN_801D2D38` behind three misses,
/// `0x801DF5E0..0x801DF5F0`), `4C 14`'s clone source (`FUN_801D835C`
/// behind `beq s5,zero` at `0x801E0E90`) and `4C 86`'s reflection source
/// (`FUN_801E573C` behind `beq s7,zero` at `0x801E21F0`). Retail's actor
/// lookup `FUN_8003C83C` answers from the same table, the spawners are
/// recorded with their arguments, and what the World staged (the talk
/// record, a clone actor, a reflection controller) is held against them.
#[test]
fn an_actor_spawner_runs_only_on_the_actors_retail_finds() {
    use legaia_engine_core::world::World;
    use legaia_engine_field::actor_handler::ActorHandler;
    use legaia_engine_field::field_channels::FieldChannel;
    differential!(prot(897, VM));
    let Some(mut cpu) = retail_vm() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const TALK: u32 = 0x801D_2D38;
    const CLONE: u32 = 0x801D_835C;
    const REFLECT: u32 = 0x801E_573C;
    let present: Rc<RefCell<Vec<u8>>> = Default::default();
    {
        let present = present.clone();
        cpu.hook(RESOLVE_ACTOR, move |r, _| {
            let id = r.r[4] as u8;
            r.r[2] = if present.borrow().contains(&id) {
                actor_at(id)
            } else {
                0
            };
        });
    }
    // The 24-bit operand reader `4C 14` loads its colour word through.
    cpu.hook(0x8003_CEB8, |r, m| {
        let a = r.r[4];
        r.r[2] = u32::from(m.rd8(a)) | u32::from(m.rd8(a + 1)) << 8 | u32::from(m.rd8(a + 2)) << 16;
    });
    let log: Rc<RefCell<Vec<String>>> = Default::default();
    let id_of = |a: u32| (a.wrapping_sub(0x8013_0000) / 0x100) as u8;
    for va in [TALK, CLONE, REFLECT] {
        let log = log.clone();
        cpu.hook(va, move |r, m| {
            let sp = r.r[29];
            let mut arg = |k: u32| m.rd32(sp + 0x10 + 4 * k);
            let line = match va {
                TALK => format!(
                    "talk {:02x?} word {:#x} byte {:#x}",
                    [id_of(r.r[4]), id_of(r.r[5]), id_of(r.r[6])],
                    r.r[7] as u16,
                    arg(0) as u8
                ),
                CLONE => format!(
                    "clone {:#x} rgb {:x?} rate {}",
                    id_of(r.r[4]),
                    legaia_engine_field::field_actor_clone::modulation_rgb(r.r[5]),
                    r.r[6] as i16
                ),
                _ => format!(
                    "reflect {:#x} words {:x?}",
                    id_of(r.r[5]),
                    [r.r[6], r.r[7], arg(0), arg(1), arg(2), arg(3)].map(|w| w as i16)
                ),
            };
            log.borrow_mut().push(line);
        });
    }
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x464F_554E);
    let ids = [3u8, 9, 0x20, 0xF8];
    let (mut ran, mut hits, mut misses) = (0u32, 0u32, 0u32);
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for round in 0..6000 {
        let pick = |rng: &mut Rng| ids[rng.below(4) as usize];
        let kind = round % 3;
        let mut script = match kind {
            0 => {
                // Three distinct talkers: the World keys the talk by id.
                let mut t = [0u8; 3];
                let mut k = 0;
                while k < 3 {
                    let id = pick(&mut rng);
                    if id != 0xF8 && !t[..k].contains(&id) {
                        t[k] = id;
                        k += 1;
                    }
                }
                let mut b = vec![0x43, 0x02, t[0], t[1], t[2]];
                b.extend_from_slice(&(rng.next() as u16).to_le_bytes());
                b.push(rng.next() as u8);
                b
            }
            1 => {
                let mut b = vec![0x4C, 0x14];
                b.extend((0..5).map(|_| rng.next() as u8));
                b.push(pick(&mut rng));
                b
            }
            _ => {
                let mut b = vec![0x4C, 0x86];
                b.extend((0..12).map(|_| rng.next() as u8));
                b.push(pick(&mut rng));
                b
            }
        };
        script.push(0x21);
        // The player is always there: `FUN_8003C83C(0xF8)` answers the
        // player object `_DAT_8007C364`, which a field never lacks.
        let here: Vec<u8> = ids
            .iter()
            .copied()
            .filter(|&id| id == 0xF8 || rng.below(10) < 7)
            .collect();
        *present.borrow_mut() = here.clone();
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        cpu.mem.wr32(PLAYER_PTR, actor_at(0xF8));
        log.borrow_mut().clear();
        if cpu.call(VM, &[SCRIPT, 0, CTX]) != Stop::Returned {
            continue;
        }
        let retail_pc = cpu.regs.r[2] as usize & 0xFFFF;
        let retail = log.borrow().clone();

        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        // The script running the op: a placement of its own, the mirror
        // image a reflection writes.
        let mut channels = vec![FieldChannel {
            placement_index: 0x30,
            ctx: FieldCtx {
                script_id: 0x30,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        }];
        for &id in &here {
            if id == 0xF8 {
                w.player_actor_slot = Some(0);
                continue;
            }
            channels.push(FieldChannel {
                placement_index: usize::from(id),
                ctx: FieldCtx {
                    script_id: u16::from(id),
                    ..FieldCtx::default()
                },
                record_offset: 0,
                pc: 0,
                done: false,
                object_bind: false,
            });
            w.npcs.positions.insert(id, (0x100 * i16::from(id), 0x80));
        }
        w.field_vm.channels = channels;
        w.field_vm.executing_channel = Some(0x30);
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        let port_pc = match w.step_field() {
            Some(StepResult::Advance { next_pc }) => next_pc,
            Some(StepResult::Yield { resume_pc }) => resume_pc,
            Some(StepResult::Halt { final_pc }) => final_pc,
            other => panic!("{script:02x?}: {other:?}"),
        };
        let mut port = Vec::new();
        if let Some(t) = w.dialog.three_actor_talk {
            port.push(format!(
                "talk {:02x?} word {:#x} byte {:#x}",
                t.actor_ids, t.script_id, t.duration
            ));
        }
        for a in w.actors.iter().filter(|a| a.active) {
            match a.handler {
                ActorHandler::ClipFade => port.push(format!(
                    "clone {:#x} rgb {:x?} rate {}",
                    script[7],
                    a.modulation_rgb.unwrap_or_default(),
                    a.physics.timer
                )),
                ActorHandler::Reflection => {
                    let words: [i16; 6] = std::array::from_fn(|k| {
                        i16::from_le_bytes([script[2 + 2 * k], script[3 + 2 * k]])
                    });
                    port.push(format!("reflect {:#x} words {words:x?}", script[14]));
                }
                _ => {}
            }
        }
        ran += 1;
        hits += u32::from(!retail.is_empty());
        misses += u32::from(retail.is_empty());
        let mut wrong = Vec::new();
        if retail_pc != port_pc {
            wrong.push(format!("pc retail {retail_pc:#x} port {port_pc:#x}"));
        }
        if retail != port {
            wrong.push(format!("retail {retail:?} port {port:?}"));
        }
        if !wrong.is_empty() {
            let e = differ.entry(format!("{:02x?}", &script[..2])).or_default();
            e.0 += 1;
            if e.0 == 1 {
                e.1 = format!(
                    "bytes {:02x?} present {here:x?}: {}",
                    &script[..script.len() - 1],
                    wrong.join(", ")
                );
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{ran} ran\n{}", report.join("\n"));
    assert!(
        ran > 5000 && hits > 1000 && misses > 1000,
        "{ran} {hits} {misses}"
    );
}

/// `4C D2 <channel>`, the placement re-seat (`0x801E2B7C..0x801E2BD8`),
/// stepped by the real `World` over a disc scene's own MAN: when the lookup
/// `FUN_8003C83C` answers the channel the op does nothing (`bne v0,zero` at
/// `0x801E2B88`), and when it answers nothing the op seats placement
/// `channel - N0` afresh through `FUN_8003A1E4`. The World answers the
/// lookup from its channels: a live placement is found, one whose context
/// retired (`+0x10 & 8`, swept) is not, and the frame's sweep re-seats it.
/// Compared: the placement retail's spawner is handed against the one the
/// World re-seats.
#[test]
fn a_reseat_spawns_only_the_placement_retail_cannot_find() {
    use legaia_engine_core::world::World;
    use legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT;
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (retail_vm(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // A scene with a handful of placements.
    let (man, mf) = (0..863)
        .filter_map(|e| d.read_entry(e).ok())
        .flat_map(|buf| crate::field_vm::man_payloads(&buf))
        .find_map(|man| {
            let mf = legaia_scene_formats::man_section::parse(&man).ok()?;
            (mf.actor_placements(&man).len() >= 6).then_some((man, mf))
        })
        .expect("a scene with placements");
    let n0 = mf.header.partition_counts[0].max(0) as u32;
    let placements: Vec<usize> = mf.actor_placements(&man).iter().map(|p| p.index).collect();
    let present: Rc<RefCell<bool>> = Default::default();
    {
        let present = present.clone();
        cpu.hook(RESOLVE_ACTOR, move |r, _| {
            r.r[2] = if *present.borrow() {
                actor_at(r.r[4] as u8)
            } else {
                0
            };
        });
    }
    // The MAN header's partition counts, `N0` first.
    cpu.hook(0x8003_D064, move |r, m| {
        m.wr32(r.r[5], n0);
        m.wr32(r.r[6], 0);
        m.wr32(r.r[7], 0);
    });
    let spawned: Rc<RefCell<Vec<(u32, u32)>>> = Default::default();
    {
        let spawned = spawned.clone();
        cpu.hook(0x8003_A1E4, move |r, _| {
            spawned.borrow_mut().push((r.r[4], r.r[5]))
        });
    }
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5245_5345);
    let (mut ran, mut reseated) = (0u32, 0u32);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for _ in 0..400 {
        let index = placements[rng.below(placements.len() as u32) as usize];
        let Ok(channel) = u8::try_from(n0 as usize + index) else {
            continue;
        };
        let here = rng.below(2) == 0;
        *present.borrow_mut() = here;
        let script = vec![0x4C, 0xD2, channel, 0x21];
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        spawned.borrow_mut().clear();
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let retail: Vec<(u32, u32)> = spawned.borrow().clone();

        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.seed_field_channels(&mf, &man);
        if !here {
            for c in w.field_vm.channels.iter_mut() {
                if !c.object_bind && c.placement_index == index {
                    c.ctx.flags |= CONTEXT_RETIRE_BIT;
                }
            }
            w.drive_field_channel_sweep();
        }
        let slot = index as u8;
        let retired = w.field_placement_retired(slot);
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        let _ = w.step_field();
        w.drive_field_channel_sweep();
        let port: Vec<(u32, u32)> = if retired && !w.field_placement_retired(slot) {
            vec![(index as u32, n0)]
        } else {
            Vec::new()
        };
        ran += 1;
        reseated += u32::from(!retail.is_empty());
        if retail != port {
            *differ
                .entry(format!(
                    "channel {channel:#x} present {here} retired {retired}: retail {retail:x?} port {port:x?}"
                ))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(12).cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(
        ran > 300 && reseated > 100 && ran - reseated > 100,
        "{ran} {reseated}"
    );
}
