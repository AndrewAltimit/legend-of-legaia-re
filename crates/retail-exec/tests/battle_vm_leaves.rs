//! Three battle-band leaves of `SCUS_942.54` whose ports sit in
//! `engine-battle-vm`:
//!
//! - `FUN_80046898`, the out-of-battle gate arm `0x82` tails into, against
//!   `validator::item_count_gate` over every sign and the `0xE0` edge.
//! - `FUN_80046978`, the armed screen wash's drain, against
//!   `rgb_wash_scale::scale_rgb24`: the submit `FUN_80024EE4` must receive
//!   the scaled colour, ABR `2` and the farthest bucket, the trigger word must
//!   be cleared, and an unarmed drain must submit nothing.
//! - `FUN_80055854`, the two-level record copier, against
//!   `nested_record_copy::copy_nested_records`: the whole destination and the
//!   returned cursor, over empty, negative and filled counts.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_battle_vm::battle_action::item_count_gate;
use legaia_engine_battle_vm::nested_record_copy::copy_nested_records;
use legaia_engine_battle_vm::rgb_wash_scale::scale_rgb24;
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const GATE: u32 = 0x8004_6898;
const DRAIN: u32 = 0x8004_6978;
const COPY: u32 = 0x8005_5854;
const SUBMIT: u32 = 0x8002_4EE4;
/// `gp[0x2E8]`, `gp[0x9D0]` and `gp[0x9D4]` with `gp = 0x8007B318`.
const COOLDOWN: u32 = 0x8007_B600;
const WASH_RGB: u32 = 0x8007_BCE8;
const WASH_ARMED: u32 = 0x8007_BCEC;
const SCALE: u32 = 0x1F80_0393;
const OT_LEN: u32 = 0x1F80_03A6;

#[test]
fn the_cooldown_gate_is_retails() {
    differential!(scus(GATE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4741_5445);
    let mut seen = [0u32; 2];
    for n in 0..4_000 {
        let v = match n % 4 {
            0 => 0xDE + rng.below(4) as i32,
            1 => -(rng.below(0x1000) as i32),
            2 => rng.next() as i32,
            _ => rng.below(0x200) as i32,
        };
        cpu.mem.wr32(COOLDOWN, v as u32);
        assert_eq!(cpu.call(GATE, &[]), Stop::Returned);
        let retail = cpu.regs.r[V0] != 0;
        assert_eq!(retail, item_count_gate(v), "cooldown {v:#x}");
        seen[usize::from(retail)] += 1;
    }
    assert!(seen[0] > 100 && seen[1] > 100, "{seen:?}");
}

#[test]
fn the_wash_drain_submits_what_retail_submits() {
    differential!(scus(DRAIN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let got: Rc<RefCell<Vec<(u32, u32, u32)>>> = Rc::default();
    {
        let got = got.clone();
        cpu.hook(SUBMIT, move |r, _| {
            got.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]));
            r.r[V0] = 0;
        });
    }
    let mut rng = Rng(0x5741_5348);
    let (mut armed, mut clamped) = (0, 0);
    for n in 0..20_000 {
        let rgb = rng.next();
        let scale = match n % 3 {
            0 => 1,
            1 => rng.below(4) as u8,
            _ => rng.next() as u8,
        };
        let ot = 1 + rng.below(0x1000) as u16;
        let arm = if rng.below(4) == 0 { 0 } else { rng.next() | 1 };
        cpu.mem.wr32(WASH_RGB, rgb);
        cpu.mem.wr32(WASH_ARMED, arm);
        cpu.mem.wr8(SCALE, scale);
        cpu.mem.wr16(OT_LEN, ot);
        got.borrow_mut().clear();
        assert_eq!(cpu.call(DRAIN, &[]), Stop::Returned);
        let calls = got.borrow().clone();
        if arm == 0 {
            assert!(calls.is_empty(), "unarmed drain submitted: {calls:x?}");
            assert_eq!(cpu.mem.rd32(WASH_ARMED), 0);
            continue;
        }
        armed += 1;
        let want = scale_rgb24(rgb, scale);
        if want != (rgb & 0xFF_FFFF) {
            clamped += 1;
        }
        assert_eq!(
            calls,
            vec![(u32::from(ot) - 1, 2, want)],
            "rgb {rgb:#x} scale {scale}"
        );
        assert_eq!(cpu.mem.rd32(WASH_ARMED), 0, "trigger not cleared");
        assert_eq!(cpu.mem.rd32(WASH_RGB), rgb, "colour word moved");
    }
    assert!(armed > 10_000 && clamped > 5_000, "{armed} {clamped}");
}

#[test]
fn the_nested_copy_copies_what_retail_copies() {
    differential!(scus(COPY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const SRC: u32 = 0x8011_0000;
    const DST: u32 = 0x8012_0000;
    const LEN: usize = 0x800;
    let mut rng = Rng(0x4E45_5354);
    let (mut empty, mut filled) = (0, 0);
    for round in 0..3_000 {
        let mut src: Vec<u32> = vec![0; 3];
        src[0] = rng.next();
        src[1] = rng.next();
        let outer: i32 = match rng.below(5) {
            0 => 0,
            1 => -(1 + rng.below(4) as i32),
            _ => 1 + rng.below(6) as i32,
        };
        src[2] = outer as u32;
        for _ in 0..outer.max(0) {
            let inner: i32 = match rng.below(5) {
                0 => 0,
                1 => -(1 + rng.below(3) as i32),
                _ => 1 + rng.below(12) as i32,
            };
            src.extend([rng.next(), rng.next(), inner as u32]);
            src.extend((0..inner.max(0) * 2).map(|_| rng.next()));
        }
        if outer <= 0 {
            empty += 1;
        } else {
            filled += 1;
        }
        // Words past the structure, which neither side may read.
        let tail: Vec<u32> = (0..16).map(|_| rng.next()).collect();
        let mut src_bytes: Vec<u8> = src.iter().flat_map(|w| w.to_le_bytes()).collect();
        src_bytes.extend(tail.iter().flat_map(|w| w.to_le_bytes()));
        cpu.mem.load(SRC, &src_bytes);
        let junk: Vec<u8> = (0..LEN * 4).map(|_| rng.next() as u8).collect();
        cpu.mem.load(DST, &junk);
        assert_eq!(cpu.call(COPY, &[DST, SRC]), Stop::Returned);
        let retail_end = cpu.regs.r[V0];
        let retail = cpu.mem.read(DST, LEN * 4);

        let mut dst: Vec<u32> = junk
            .chunks(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let mut port_src = src.clone();
        port_src.extend(&tail);
        let n = copy_nested_records(&port_src, &mut dst);
        let port: Vec<u8> = dst.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(
            retail,
            port,
            "round {round}: {:?}",
            diff_offsets(&retail, &port)
        );
        assert_eq!(retail_end, DST + 4 * n as u32, "round {round}: cursor");
        assert_eq!(
            n,
            src.len(),
            "round {round}: the port copied past the structure"
        );
    }
    assert!(empty > 300 && filled > 1_000, "{empty} {filled}");
}

/// The target-select cursor tint `FUN_801DA6B4` (PROT 0898).
const CURSOR_TINT: u32 = 0x801D_A6B4;

struct Seats(Vec<legaia_engine_battle_vm::battle_action::BattleActor>);

impl legaia_engine_battle_vm::battle_action::BattleActionHost for Seats {
    fn actor(&self, slot: u8) -> Option<&legaia_engine_battle_vm::battle_action::BattleActor> {
        self.0.get(usize::from(slot))
    }
    fn actor_mut(
        &mut self,
        slot: u8,
    ) -> Option<&mut legaia_engine_battle_vm::battle_action::BattleActor> {
        self.0.get_mut(usize::from(slot))
    }
}

#[test]
fn the_target_cursor_tints_what_retail_tints() {
    differential!(prot(898, CURSOR_TINT));
    use legaia_engine_battle_vm::battle_action::{
        BattleActionCtx, BattleActor, target_cursor_highlight,
    };
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const ACTORS: u32 = 0x801C_9370;
    const ACTOR: u32 = 0x8013_0000;
    const CTX: u32 = 0x8012_0000;
    cpu.mem
        .load(0x801C_E818, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(0x8007_BD24, CTX);
    let mut rng = Rng(0x5447_5443);
    let (mut lit, mut cleared) = (0, 0);
    for round in 0..20_000 {
        let enable = rng.below(3) != 0;
        let acting = rng.below(8) as u8;
        cpu.mem.wr8(CTX + 0x13, acting);
        let mut host = Seats(Vec::new());
        for s in 0..8u32 {
            let a = ACTOR + s * 0x400;
            cpu.mem.wr32(ACTORS + s * 4, a);
            let hp = if rng.below(4) == 0 {
                0
            } else {
                1 + rng.below(999) as u16
            };
            let (flag, color, blend) = (rng.next() as u8, rng.next(), rng.next());
            let target = rng.below(9) as u8;
            cpu.mem.wr16(a + 0x14C, hp);
            cpu.mem.wr8(a + 0x21C, flag);
            cpu.mem.wr32(a + 0x04, color);
            cpu.mem.wr32(a + 0x0C, blend);
            cpu.mem.wr8(a + 0x1DD, target);
            host.0.push(BattleActor {
                liveness: hp,
                render_flag: flag,
                render_color: color,
                render_blend: blend,
                active_target: target,
                ..BattleActor::default()
            });
        }
        let before = cpu.mem.read(ACTOR, 8 * 0x400);
        assert_eq!(
            cpu.call(CURSOR_TINT, &[u32::from(!enable)]),
            Stop::Returned,
            "round {round}"
        );
        let ctx = BattleActionCtx {
            active_actor: acting,
            ..BattleActionCtx::default()
        };
        target_cursor_highlight(&mut host, &ctx, enable);
        let mut want = before.clone();
        for (s, a) in host.0.iter().enumerate() {
            let o = s * 0x400;
            want[o + 0x04..o + 0x08].copy_from_slice(&a.render_color.to_le_bytes());
            want[o + 0x0C..o + 0x10].copy_from_slice(&a.render_blend.to_le_bytes());
            want[o + 0x21C] = a.render_flag;
        }
        let got = cpu.mem.read(ACTOR, 8 * 0x400);
        assert_eq!(got, want, "round {round}: {:?}", diff_offsets(&got, &want));
        if enable {
            lit += 1;
        } else {
            cleared += 1;
        }
    }
    assert!(lit > 10_000 && cleared > 5_000, "{lit} {cleared}");
}

/// The arts-list window's register `FUN_801DBB8C` (PROT 0898).
const LABEL_OPEN: u32 = 0x801D_BB8C;

#[test]
fn the_arts_window_opens_as_retail_opens_it() {
    differential!(prot(898, LABEL_OPEN));
    use legaia_engine_battle_vm::battle_party_panel::{LabelState, open_selector};
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const CTX: u32 = 0x8012_0000;
    const BLOCK: u32 = 0x801F_4E08;
    const SELECTOR: u32 = 0x8007_BB8C;
    const CLEARED: u32 = 0x8007_B458;
    const ROSTER: u32 = 0x8007_BD10;
    const REGISTER: u32 = 0x8003_541C;
    cpu.mem
        .load(0x801C_E818, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(0x8007_BD24, CTX);
    let args: Rc<RefCell<Vec<u32>>> = Rc::default();
    let handle = Rc::new(std::cell::Cell::new(0u32));
    {
        let (args, handle) = (args.clone(), handle.clone());
        cpu.hook(REGISTER, move |r, m| {
            let sp = r.r[psx_retail_exec::SP];
            let mut a = vec![r.r[A0], r.r[A1], r.r[A2], r.r[psx_retail_exec::A3]];
            a.extend((0..4).map(|i| m.rd32(sp + 0x10 + 4 * i)));
            *args.borrow_mut() = a;
            r.r[V0] = handle.get();
        });
    }
    let mut rng = Rng(0x4C41_424C);
    for round in 0..4_000 {
        let seat = rng.below(3);
        cpu.mem.wr8(CTX + 0x13, seat as u8);
        let id = rng.next() as u8;
        cpu.mem.wr8(ROSTER + seat, id);
        for b in 0..8 {
            cpu.mem.wr8(BLOCK + b, rng.next() as u8);
        }
        cpu.mem.wr32(CLEARED, rng.next());
        handle.set(rng.next());
        assert_eq!(cpu.call(LABEL_OPEN, &[]), Stop::Returned, "round {round}");
        let want = LabelState::opened(handle.get());
        let mut block = vec![want.kind, want.flags, want.phase];
        block.push(cpu.mem.rd8(BLOCK + 3));
        block.extend(want.handle.to_le_bytes());
        assert_eq!(cpu.mem.read(BLOCK, 8), block, "round {round}");
        assert_eq!(
            cpu.mem.rd32(SELECTOR) as i32,
            open_selector(id),
            "round {round}"
        );
        assert_eq!(cpu.mem.rd32(CLEARED), 0, "round {round}");
        assert_eq!(
            *args.borrow(),
            [0, 0xC, 0, -146i32 as u32, 36, 138, 144, 3],
            "round {round}"
        );
    }
}

/// The battle-UI widget pool release `FUN_801D9AE8` (PROT 0898).
const WIDGET_RELEASE: u32 = 0x801D_9AE8;

#[test]
fn the_widget_release_frees_what_retail_frees() {
    differential!(prot(898, WIDGET_RELEASE));
    use legaia_engine_battle_vm::battle_cursor_pose::{
        WIDGET_SCRATCH_WORDS, WIDGET_SLOTS, WidgetHandle, WidgetSlot, release_widget_pool,
    };
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const CTX: u32 = 0x8012_0000;
    const WIDGETS: u32 = 0x8014_0000;
    const SCRATCH: u32 = 0x801C_8FA0;
    const TRIGGER: u32 = 0x8003_19A8;
    cpu.mem
        .load(0x801C_E818, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(0x8007_BD24, CTX);
    let ids: Rc<RefCell<Vec<u16>>> = Rc::default();
    {
        let ids = ids.clone();
        cpu.hook(TRIGGER, move |r, _| ids.borrow_mut().push(r.r[A0] as u16));
    }
    let mut rng = Rng(0x5749_4447);
    let mut released = 0;
    for round in 0..2_000 {
        let ctx: Vec<u8> = (0..0x1400).map(|_| rng.next() as u8).collect();
        cpu.mem.load(CTX, &ctx);
        let mut slots = vec![WidgetSlot::default(); WIDGET_SLOTS];
        for (k, s) in slots.iter_mut().enumerate() {
            let k = k as u32;
            let live = if rng.below(2) == 0 {
                0
            } else {
                rng.next() as u8
            };
            let ptr = if rng.below(3) == 0 {
                0
            } else {
                WIDGETS + k * 0x10
            };
            let actor = rng.next() as u16;
            cpu.mem.wr8(CTX + 0x11B7 + k * 0xC, live);
            cpu.mem.wr32(CTX + 0x1074 + k * 4, ptr);
            if ptr != 0 {
                cpu.mem.wr16(ptr + 8, actor);
            }
            *s = WidgetSlot {
                live,
                tag: cpu.mem.rd8(CTX + 0x11B4 + k * 0xC),
                widget: (ptr != 0).then_some(WidgetHandle { actor_id: actor }),
            };
        }
        for i in 0..16 {
            cpu.mem.wr32(SCRATCH + i * 4, rng.next());
        }
        ids.borrow_mut().clear();
        let before = cpu.mem.read(CTX, 0x1400);
        assert_eq!(
            cpu.call(WIDGET_RELEASE, &[]),
            Stop::Returned,
            "round {round}"
        );
        let mut scratch = [1u32; WIDGET_SCRATCH_WORDS];
        let port_ids = release_widget_pool(&mut slots, &mut scratch);
        assert_eq!(*ids.borrow(), port_ids, "round {round}: released ids");
        released += port_ids.len();
        let mut want = before;
        for (k, s) in slots.iter().enumerate() {
            want[0x11B7 + k * 0xC] = s.live;
            want[0x11B4 + k * 0xC] = s.tag;
            let ptr = s.widget.map_or(0, |_| WIDGETS + k as u32 * 0x10);
            want[0x1074 + k * 4..0x1078 + k * 4].copy_from_slice(&ptr.to_le_bytes());
        }
        let got = cpu.mem.read(CTX, 0x1400);
        assert_eq!(got, want, "round {round}: {:?}", diff_offsets(&got, &want));
        let wiped: Vec<u32> = (0..16).map(|i| cpu.mem.rd32(SCRATCH + i * 4)).collect();
        assert_eq!(wiped, scratch.to_vec(), "round {round}: scratch");
    }
    assert!(released > 20_000, "{released}");
}
