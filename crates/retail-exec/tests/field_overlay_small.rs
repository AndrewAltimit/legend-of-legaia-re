//! Two short routines of the field overlay (PROT 0897): the save-screen
//! hand-off `FUN_801D841C` against
//! `legaia_engine_field::save_screen_spawn::save_screen_spawn`, and the slot-6
//! audio release `FUN_801D8450` against
//! `legaia_engine_field::field_audio_release::field_audio_release_steps`.
//!
//! Each is a call list and a store or two, so the calls are hooked and logged
//! and the stores read back.

use crate::common::{disc, scus_cpu};
use legaia_engine_field::field_audio_release::{ReleaseStep, field_audio_release_steps};
use legaia_engine_field::save_screen_spawn::save_screen_spawn;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SAVE_SPAWN: u32 = 0x801D_841C;
const AUDIO_RELEASE: u32 = 0x801D_8450;

const SPAWN: u32 = 0x8002_0DE0;
const STOP_VOICE: u32 = 0x8006_53C8;
const CLOSE_VAB: u32 = 0x8001_FF58;

const ACTOR: u32 = 0x8012_0000;

#[test]
fn the_save_screen_spawn_matches_retail() {
    differential!(prot(897, SAVE_SPAWN));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let l = log.clone();
    cpu.hook(SPAWN, move |r, _| {
        l.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[V0] = ACTOR;
    });
    let want = save_screen_spawn();
    cpu.mem.wr32(want.pool, 0x8012_7000);
    // The spawned actor's record, so the one store shows against its
    // neighbours.
    for o in (0..0x100).step_by(2) {
        cpu.mem.wr16(ACTOR + o, 0xA5A5);
    }
    assert_eq!(cpu.call(SAVE_SPAWN, &[]), Stop::Returned);
    assert_eq!(*log.borrow(), [(want.descriptor, 0x8012_7000)]);
    for o in (0..0x100).step_by(2) {
        let expect = if o == 0x5C {
            want.field_5c as u16
        } else {
            0xA5A5
        };
        assert_eq!(cpu.mem.rd16(ACTOR + o), expect, "actor +{o:#x}");
    }
}

#[test]
fn the_field_audio_release_matches_retail() {
    differential!(prot(897, AUDIO_RELEASE));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let steps = field_audio_release_steps();
    let globals: Vec<u32> = steps
        .iter()
        .filter_map(|s| match *s {
            ReleaseStep::ClearGlobal(va) => Some(va),
            _ => None,
        })
        .collect();
    // The log carries each call with the two latches as they stand at it:
    // both clears come after the last call.
    let log = Rc::new(RefCell::new(Vec::<(u32, u32, Vec<u32>)>::new()));
    for va in [STOP_VOICE, CLOSE_VAB] {
        let (l, g) = (log.clone(), globals.clone());
        cpu.hook(va, move |r, m| {
            let seen = g.iter().map(|&a| m.rd32(a)).collect();
            l.borrow_mut().push((va, r.r[A0], seen));
        });
    }
    for &g in &globals {
        cpu.mem.wr32(g, 0x1234_5678);
        cpu.mem.wr32(g - 4, 0x0BAD_F00D);
        cpu.mem.wr32(g + 4, 0x0BAD_F00D);
    }
    assert_eq!(cpu.call(AUDIO_RELEASE, &[]), Stop::Returned);
    let live = vec![0x1234_5678; globals.len()];
    let want: Vec<(u32, u32, Vec<u32>)> = steps
        .iter()
        .filter_map(|s| match *s {
            ReleaseStep::StopVoice(v) => Some((STOP_VOICE, u32::from(v), live.clone())),
            ReleaseStep::ReleaseVabSlot(n) => Some((CLOSE_VAB, u32::from(n), live.clone())),
            ReleaseStep::ClearGlobal(_) => None,
        })
        .collect();
    assert_eq!(*log.borrow(), want);
    assert_eq!(globals.len(), 2);
    assert!(
        matches!(
            steps[steps.len() - 2..],
            [ReleaseStep::ClearGlobal(_), ReleaseStep::ClearGlobal(_)]
        ),
        "the clears close the list"
    );
    for &g in &globals {
        assert_eq!(cpu.mem.rd32(g), 0, "{g:#x}");
        assert_eq!(cpu.mem.rd32(g - 4), 0x0BAD_F00D, "below {g:#x}");
        assert_eq!(cpu.mem.rd32(g + 4), 0x0BAD_F00D, "above {g:#x}");
    }
}

/// The kind-0 warp's fade repack `FUN_801D58F0`, over generated arguments,
/// against `warp_fade_template`: the template it hands `FUN_80024E80` (whose
/// second argument is `0`), `[0]`, `[1]`, `[3..=5]`, `[7..=9]`, `[10]` and
/// `[11]`. The loader takes `[12]` as the id it stamps and never reads `[2]` /
/// `[6]`; the repack leaves all three as the stack had them.
#[test]
fn the_warp_fade_repack_matches_retail() {
    use legaia_engine_core::world::warp_fade_template;
    use legaia_engine_field::field_warp_tile::WarpFade;
    const REPACK: u32 = 0x801D_58F0;
    const LOAD_FADE: u32 = 0x8002_4E80;
    differential!(prot(897, REPACK));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let seen = Rc::new(RefCell::new(Vec::<(Vec<i16>, u32)>::new()));
    let s = seen.clone();
    cpu.hook(LOAD_FADE, move |r, m| {
        let words = (0..13).map(|i| m.rd16(r.r[A0] + 2 * i) as i16).collect();
        s.borrow_mut().push((words, r.r[A1]));
    });
    let mut rng = crate::common::Rng(0x5741_5250);
    for case in 0..5000 {
        let f = WarpFade {
            kind: rng.next() as i16,
            from_rgb: rng.next(),
            to_rgb: rng.next(),
            delay: rng.next() as i16,
            duration: rng.next() as i16,
            hold: rng.next() as i16,
        };
        cpu.mem
            .wr32(crate::common::STACK_TOP + 0x10, f.duration as i32 as u32);
        cpu.mem
            .wr32(crate::common::STACK_TOP + 0x14, f.hold as i32 as u32);
        seen.borrow_mut().clear();
        let args = [
            f.kind as i32 as u32,
            f.from_rgb,
            f.to_rgb,
            f.delay as i32 as u32,
        ];
        assert_eq!(cpu.call(REPACK, &args), Stop::Returned, "case {case}");
        let seen = seen.borrow();
        assert_eq!(seen.len(), 1, "case {case}: one load");
        let (w, second) = &seen[0];
        assert_eq!(*second, 0, "case {case}: the loader's second argument");
        let t = warp_fade_template(&f, f.delay);
        let got = (
            w[0],
            w[1],
            [w[3], w[4], w[5]],
            [w[7], w[8], w[9]],
            [w[10], w[11]],
        );
        let want = (
            t.kind,
            t.duration,
            t.start_rgb,
            t.end_rgb,
            [t.mode[0], t.mode[1]],
        );
        assert_eq!(got, want, "case {case}: {f:?}");
    }
}

/// The actor clone `FUN_801D835C` over generated source actors and operands,
/// the allocator answering a zeroed actor: the descriptor's `+4` low half and
/// every field of the clone against `clone_plan` - position, rotation, rate,
/// modulation word, `+0x68`, the untouched fraction `+0x78` - with the two
/// fourth halfwords and the model word `+0x4C` copied as the plan's doc says.
#[test]
fn the_actor_clone_matches_retail() {
    use legaia_engine_field::field_actor_clone::{CloneSource, clone_plan};
    const CLONE: u32 = 0x801D_835C;
    const DESCRIPTOR: u32 = 0x8007_0644;
    const SRC: u32 = 0x8013_0000;
    const DST: u32 = 0x8013_1000;
    differential!(prot(897, CLONE));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let head = 0x8012_7000;
    cpu.mem.wr32(0x8007_C34C, head);
    let asked = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let a = asked.clone();
    cpu.hook(SPAWN, move |r, m| {
        a.borrow_mut().push((r.r[A0], r.r[A1]));
        m.load(DST, &[0; 0x100]);
        r.r[V0] = DST;
    });
    let mut rng = crate::common::Rng(0x434C_4F4E);
    for case in 0..5000 {
        let junk: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        cpu.mem.load(SRC, &junk);
        let h = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(SRC + o) as i16;
        let src = CloneSource {
            pos: (h(&mut cpu, 0x14), h(&mut cpu, 0x16), h(&mut cpu, 0x18)),
            rot: (h(&mut cpu, 0x24), h(&mut cpu, 0x26), h(&mut cpu, 0x28)),
            anim_set: h(&mut cpu, 0x64) as u16,
            field_68: h(&mut cpu, 0x68),
        };
        let word = rng.next() & 0x00FF_FFFF;
        let rate = rng.next() as i16;
        asked.borrow_mut().clear();
        let args = [SRC, word, rate as i32 as u32];
        assert_eq!(cpu.call(CLONE, &args), Stop::Returned, "case {case}");
        assert_eq!(*asked.borrow(), [(DESCRIPTOR, head)], "case {case}");
        let plan = clone_plan(src, word, rate);
        let d = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(DST + o) as i16;
        let got = (
            (d(&mut cpu, 0x14), d(&mut cpu, 0x16), d(&mut cpu, 0x18)),
            (d(&mut cpu, 0x24), d(&mut cpu, 0x26), d(&mut cpu, 0x28)),
            d(&mut cpu, 0x54),
            cpu.mem.rd32(DST + 0x74),
            d(&mut cpu, 0x68),
            d(&mut cpu, 0x78),
            cpu.mem.rd16(DESCRIPTOR + 4),
        );
        let want = (
            plan.pos,
            plan.rot,
            plan.rate,
            plan.modulation,
            plan.field_68,
            plan.fraction,
            plan.anim_set,
        );
        assert_eq!(got, want, "case {case}");
        for o in [0x1A, 0x2A] {
            assert_eq!(
                cpu.mem.rd16(DST + o),
                cpu.mem.rd16(SRC + o),
                "case {case} +{o:#x}"
            );
        }
        assert_eq!(
            cpu.mem.rd32(DST + 0x4C),
            cpu.mem.rd32(SRC + 0x4C),
            "case {case}"
        );
    }
}

/// The morph-weight spawner `FUN_801D77F4` over generated VDF blocks (one to
/// three records, so the three record pitches disagree) and generated meshes:
/// the snapshot buffer it allocates (its size, and the bytes it copies into it)
/// against `rest_pose_snapshot`, and the actor stores the envelope starts from
/// (`+0x3C` / `+0x3E` the rates, `+0x6E` / `+0x68` / `+0x56` / `+0x5C` zero,
/// `+0x48` the mesh, `+0x4C` the block). The debug prints are stubbed. Where the
/// copy pass's pitch reads fewer vertices than the size pass summed, retail
/// leaves the buffer's tail as the heap had it and the port leaves zeros; where
/// it reads more, retail writes past its allocation and the port stops at it.
#[test]
fn the_morph_spawner_snapshots_as_retail_does() {
    use legaia_engine_field::morph_weight_apply::rest_pose_snapshot;
    const SPAWNER: u32 = 0x801D_77F4;
    const ACTOR: u32 = 0x8013_0000;
    const VDF_TABLE: u32 = 0x8013_2000;
    const TMD: u32 = 0x8013_4000;
    const VERTS: u32 = 0x8013_8000;
    const SNAP: u32 = 0x8015_0000;
    differential!(prot(897, SPAWNER));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.stub(0x8005_67A8, 0);
    cpu.stub(SPAWN, ACTOR);
    // The heap hands back whatever the buffer held: two fills, so a byte the
    // copy pass never wrote shows as one that differs between them.
    let sizes = Rc::new(RefCell::new(Vec::<u32>::new()));
    let fill = Rc::new(std::cell::Cell::new(0xEEu8));
    let (s, f) = (sizes.clone(), fill.clone());
    cpu.hook(0x8001_7888, move |r, m| {
        s.borrow_mut().push(r.r[A1]);
        m.load(SNAP, &[f.get(); 0x4000]);
        r.r[V0] = SNAP;
    });
    cpu.mem.wr32(0x8007_B7DC, VDF_TABLE);
    let mut rng = crate::common::Rng(0x4D4F_5250);
    for case in 0..3000 {
        // Six objects, each `0..12` vertices at its own pointer.
        let mut groups: Vec<Vec<u8>> = Vec::new();
        let mut at = VERTS;
        for g in 0..6u32 {
            let n = rng.below(12);
            let bytes: Vec<u8> = (0..n * 8).map(|_| rng.next() as u8).collect();
            cpu.mem.load(at, &bytes);
            let entry = TMD + 0xC + g * 0x1C;
            cpu.mem.wr32(entry, at);
            cpu.mem.wr32(entry + 4, n);
            at += 0x100;
            groups.push(bytes);
        }
        let mesh_slot = rng.below(8);
        cpu.mem.wr32(0x8007_C018 + 4 * mesh_slot, TMD);
        // The block: a count, its records, and a tail of small words for the
        // copy pass's own pitch to read as group ids.
        let count = 1 + rng.below(3);
        let mut block = count.to_le_bytes().to_vec();
        while block.len() < 0x400 {
            block.extend_from_slice(&rng.below(6).to_le_bytes());
        }
        let idx = rng.below(4);
        cpu.mem.wr32(VDF_TABLE + 4 + 4 * idx, 0x100);
        cpu.mem.load(VDF_TABLE + 0x100, &block);
        let (up, down) = (rng.next() as u16, rng.next() as u16);
        let args = [idx, mesh_slot, u32::from(up), u32::from(down)];
        let mut runs = Vec::new();
        for fill_byte in [0xEE, 0x11] {
            fill.set(fill_byte);
            cpu.mem.load(ACTOR, &[0xA5; 0x100]);
            sizes.borrow_mut().clear();
            assert_eq!(cpu.call(SPAWNER, &args), Stop::Returned, "case {case}");
            runs.push(cpu.mem.read(SNAP, 0x4000));
        }
        let refs: Vec<&[u8]> = groups.iter().map(Vec::as_slice).collect();
        let want = rest_pose_snapshot(&block, &refs);
        assert_eq!(
            *sizes.borrow(),
            [want.len() as u32],
            "case {case}: snapshot size"
        );
        // Bytes the copy pass wrote match; one it left as the heap had it is
        // the port's zero.
        for (i, &w) in want.iter().enumerate() {
            let (a, b) = (runs[0][i], runs[1][i]);
            let got = if a == b { a } else { 0 };
            assert_eq!(got, w, "case {case}: {count} records, byte {i}");
        }
        assert_eq!(cpu.mem.rd32(ACTOR + 0x90), SNAP, "case {case}");
        let h = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(ACTOR + o);
        assert_eq!(
            (h(&mut cpu, 0x3C), h(&mut cpu, 0x3E)),
            (up, down),
            "case {case}: rates"
        );
        for o in [0x56, 0x5C, 0x68, 0x6E] {
            assert_eq!(h(&mut cpu, o), 0, "case {case} +{o:#x}");
        }
        assert_eq!(cpu.mem.rd32(ACTOR + 0x48), TMD, "case {case}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x4C), VDF_TABLE + 0x100, "case {case}");
    }
}

/// The seat ops' list relink `FUN_801D81E0`, with the SCUS list primitives
/// it calls (`FUN_800204A4` unlink, `FUN_80020454` append) live, over
/// generated placement lists, against `World::relink_placement_for_seat`:
/// outside a spawn prologue on the whole list in any order; inside one on the
/// placements spawned so far (the running one last), the later ones appended
/// after as the spawn loop goes on to link them.
#[test]
fn the_seat_relink_matches_retail() {
    use legaia_engine_core::world::{FieldNpcAmbient, World};
    use legaia_engine_motion_vm::ambient_motion::AmbientMotion;
    const RELINK: u32 = 0x801D_81E0;
    const APPEND: u32 = 0x8002_0454;
    const FREE_TOP: u32 = 0x8007_C348;
    const HEAD: u32 = 0x8015_0000;
    let node = |slot: u8| 0x8014_0000 + u32::from(slot) * 0x40;
    differential!(prot(897, RELINK));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    // Link `order` onto an empty list through the append itself, each node
    // handed over through the free stack as a spawn does.
    let link = |cpu: &mut psx_retail_exec::Cpu, slots: &[u8]| {
        for &s in slots {
            cpu.mem.wr32(FREE_TOP, 0);
            cpu.mem.wr32(FREE_TOP + 0x28, node(s));
            assert_eq!(cpu.call(APPEND, &[HEAD]), Stop::Returned);
        }
    };
    let walk = |cpu: &mut psx_retail_exec::Cpu| {
        let mut out = Vec::new();
        let mut at = cpu.mem.rd32(HEAD);
        while at != 0 && out.len() < 64 {
            out.push(((at - 0x8014_0000) / 0x40) as u8);
            at = cpu.mem.rd32(at);
        }
        out
    };
    let channel = |slot: u8| FieldNpcAmbient {
        defers: false,
        walks: false,
        variants: Vec::new(),
        live: None,
        vm: AmbientMotion::new(u32::from(slot), 0),
    };
    let mut rng = crate::common::Rng(0x5345_4154);
    for case in 0..4000 {
        let mut slots: Vec<u8> = (0..16u8).filter(|_| rng.below(2) == 0).collect();
        if slots.len() < 2 {
            continue;
        }
        let prologue = case % 2 == 1;
        if !prologue {
            for i in (1..slots.len()).rev() {
                slots.swap(i, rng.below(i as u32 + 1) as usize);
            }
        }
        let running_at = rng.below(slots.len() as u32) as usize;
        let linked = if prologue {
            &slots[..=running_at]
        } else {
            &slots[..]
        };
        let k = linked[rng.below(linked.len() as u32) as usize];
        // An empty list: the first-node word zero, the tail pointer on it.
        cpu.mem.wr32(HEAD, 0);
        cpu.mem.wr32(HEAD + 0xC, HEAD);
        cpu.mem.wr32(FREE_TOP, u32::MAX);
        link(&mut cpu, linked);
        cpu.mem.wr32(FREE_TOP, u32::MAX);
        assert_eq!(
            cpu.call(RELINK, &[node(k), HEAD]),
            Stop::Returned,
            "case {case}"
        );
        if prologue {
            link(&mut cpu, &slots[running_at + 1..]);
        }
        let mut w = World::new();
        for &s in &slots {
            w.npcs.ambient.insert(s, channel(s));
        }
        w.npcs.ambient_order = slots.clone();
        w.field_vm.prerun_running_slot = prologue.then_some(slots[running_at]);
        w.relink_placement_for_seat(k);
        assert_eq!(
            walk(&mut cpu),
            w.npcs.ambient_order,
            "case {case}: {slots:?} relink {k} prologue {prologue}"
        );
    }
}

/// The contact sweep `FUN_801CFC40` at one probe point, as the ledge
/// classifier calls it (both stack widenings `0`), over generated
/// moving-class candidate tables (`+0x10 & 0x20000`, the class every placement
/// actor carries) and probe offsets, against
/// `World::field_actor_point_blocked` with the same actors as live NPC
/// positions: result bit `1` against the port's verdict, and the mutual
/// `+0x98` links a hit writes. The hit's event post `FUN_8003D038` is stubbed.
#[test]
fn the_contact_sweep_matches_retail_on_moving_actors() {
    use legaia_engine_core::world::World;
    const SWEEP: u32 = 0x801C_FC40;
    const COUNT: u32 = 0x8007_B6B8;
    const TABLE: u32 = 0x801C_93C8;
    const PROBER: u32 = 0x8013_0000;
    let actor = |i: u32| 0x8013_1000 + i * 0x100;
    differential!(prot(897, SWEEP));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.stub(0x8003_D038, 0);
    let mut rng = crate::common::Rng(0x5357_4550);
    let (mut hits, mut misses) = (0, 0);
    for case in 0..20_000 {
        let n = 1 + rng.below(8);
        let (x, z) = (rng.below(0x1000) as i16, rng.below(0x1000) as i16);
        let mut w = World::new();
        w.npcs.solid = true;
        cpu.mem.load(PROBER, &[0; 0x100]);
        cpu.mem.wr16(PROBER + 0x14, x as u16);
        cpu.mem.wr16(PROBER + 0x18, z as u16);
        for i in 0..n {
            let a = actor(i);
            cpu.mem.load(a, &[0; 0x100]);
            // Near the prober, so a fair share of probes land in a box.
            let ax = x.wrapping_add(rng.below(0xC0) as i16 - 0x60);
            let az = z.wrapping_add(rng.below(0xC0) as i16 - 0x60);
            cpu.mem
                .wr32(a + 0x10, 0x0002_0000 | (rng.next() & 0x0800_0884));
            cpu.mem.wr16(a + 0x14, ax as u16);
            cpu.mem.wr16(a + 0x18, az as u16);
            cpu.mem.wr32(TABLE + 4 * i, a);
            w.npcs.positions.insert(i as u8, (ax, az));
        }
        cpu.mem.wr32(COUNT, n);
        let (dx, dz) = (rng.below(0x80) as i32 - 0x40, rng.below(0x80) as i32 - 0x40);
        cpu.mem.wr32(crate::common::STACK_TOP + 0x10, 0);
        cpu.mem.wr32(crate::common::STACK_TOP + 0x14, 0);
        let args = [PROBER, 0, dx as u32, dz as u32];
        assert_eq!(cpu.call(SWEEP, &args), Stop::Returned, "case {case}");
        let bits = cpu.regs.r[V0];
        let port = w.field_actor_point_blocked(i32::from(x) + dx, i32::from(z) - dz);
        assert_eq!(bits & 1 != 0, port, "case {case}: bits {bits:#x}");
        assert_eq!(
            bits & !1,
            0,
            "case {case}: a moving actor is never static-class"
        );
        if port {
            hits += 1;
            let linked = cpu.mem.rd32(PROBER + 0x98);
            assert_eq!(
                cpu.mem.rd32(linked + 0x98),
                PROBER,
                "case {case}: mutual link"
            );
        } else {
            misses += 1;
        }
    }
    assert!(hits > 3000 && misses > 3000, "{hits} {misses}");
}

/// The `4C DB` CLUT blend spawn `FUN_801E57F0` over generated records, the
/// allocator answering an actor: the descriptor and list it asks for
/// (`0x801F2930`, `_DAT_8007C34C`), the record pointer `+0x90` and the state
/// `+0x54 = 0` it stores, against `World::spawn_clut_blend_fx` - one effect,
/// its record decoded from the bytes that pointer names, no fade begun (the
/// actor's state `0`, whose first tick stores the cell).
#[test]
fn the_clut_blend_spawn_matches_retail() {
    use legaia_engine_core::world::World;
    use legaia_engine_field::clut_fade_actor::FadeRecord;
    const BLEND_SPAWN: u32 = 0x801E_57F0;
    const RECORD: u32 = 0x8013_2000;
    const ACTOR: u32 = 0x8013_0000;
    differential!(prot(897, BLEND_SPAWN));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let head = 0x8012_7000;
    cpu.mem.wr32(0x8007_C34C, head);
    let asked = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let a = asked.clone();
    cpu.hook(SPAWN, move |r, m| {
        a.borrow_mut().push((r.r[A0], r.r[A1]));
        m.load(ACTOR, &[0xA5; 0x100]);
        r.r[V0] = ACTOR;
    });
    let mut rng = crate::common::Rng(0x424C_4E44);
    for case in 0..2000 {
        let bytes: Vec<u8> = std::iter::once(0xDB)
            .chain((0..15).map(|_| rng.next() as u8))
            .collect();
        cpu.mem.load(RECORD, &bytes);
        asked.borrow_mut().clear();
        assert_eq!(
            cpu.call(BLEND_SPAWN, &[RECORD]),
            Stop::Returned,
            "case {case}"
        );
        assert_eq!(*asked.borrow(), [(0x801F_2930, head)], "case {case}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x90), RECORD, "case {case}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54), 0, "case {case}");
        let mut w = World::new();
        w.spawn_clut_blend_fx(&bytes);
        let fx = &w.ambient.clut_blend_fx;
        assert_eq!(fx.len(), 1, "case {case}");
        let at = cpu.mem.rd32(ACTOR + 0x90);
        let retail_record = cpu.mem.read(at, 16);
        assert_eq!(
            fx[0].record,
            FadeRecord::from_bytes(&retail_record),
            "case {case}"
        );
        assert!(fx[0].fade.is_none(), "case {case}: state 0");
    }
}
