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
