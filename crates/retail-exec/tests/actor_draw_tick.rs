//! The per-actor battle draw tick `FUN_800480D8` against
//! `battle_actor_tick::battle_actor_tick` and the preamble kernels beside it
//! (`battle_frame_passes`, `voided_effect_nodes`), with every pass it
//! sequences hooked. Compared: the calls in order, the colour word the draw
//! reads at `+0x74`, the trail flag `+0x6A`, the request byte `ctx[+0x272]`
//! and the effect-node table `DAT_801C90F0` after the sweep, over every seat,
//! both grey gates and an inert node.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_actor_draw::body_shadow_skip;
use legaia_engine_battle_vm::battle_actor_tick::{
    BattleActorView, BattleDrawStep, BattleTickGates, battle_actor_tick, battle_frame_passes,
    voided_effect_nodes,
};
use psx_retail_exec::Stop;
use std::cell::RefCell;
use std::rc::Rc;

const TICK: u32 = 0x8004_80D8;
const TINT: u32 = 0x8004_A908;
const DRAW: u32 = 0x8004_8A08;
const AFTERIMAGE: u32 = 0x8004_9348;
const SIGNATURE: u32 = 0x8005_112C;
const FRAME_PASSES: [u32; 4] = [0x801E_0080, 0x801E_09F8, 0x801D_F6B8, 0x801E_2524];
const EXTRA_HOOK: u32 = 0x801F_7B88;
const CTX_PTR: u32 = 0x8007_BD24;
const BATTLE_SIGNAL: u32 = 0x8007_BD71;
const EXTRA_ARMED: u32 = 0x8007_BDC0;
const SECOND_MONSTER: u32 = 0x8007_BD0D;
const EFFECT_NODES: u32 = 0x801C_90F0;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const NODE: u32 = 0x8013_0000;
const ACTOR: u32 = 0x8014_0000;
const TARGETS: u32 = 0x8015_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Step(BattleDrawStep),
    /// The draw, with the colour word and trail flag it read.
    Draw(u32, u16),
    /// The after-image walk, with the trail flag it ran under.
    Afterimage(u16),
    Pass(u32),
}

#[test]
fn the_battle_draw_tick_is_retails() {
    differential!(scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let calls = Rc::new(RefCell::new(Vec::new()));
    for (va, step) in [
        (TINT, BattleDrawStep::Tint),
        (SIGNATURE, BattleDrawStep::SignatureEffect),
    ] {
        let c = calls.clone();
        cpu.hook(va, move |_, _| c.borrow_mut().push(Call::Step(step)));
    }
    let c = calls.clone();
    cpu.hook(DRAW, move |_, m| {
        c.borrow_mut()
            .push(Call::Draw(m.rd32(NODE + 0x74), m.rd16(NODE + 0x6A)));
    });
    let c = calls.clone();
    cpu.hook(AFTERIMAGE, move |_, m| {
        c.borrow_mut().push(Call::Afterimage(m.rd16(NODE + 0x6A)));
    });
    for va in FRAME_PASSES.into_iter().chain([EXTRA_HOOK]) {
        let c = calls.clone();
        cpu.hook(va, move |_, _| c.borrow_mut().push(Call::Pass(va)));
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    for k in 0..8u32 {
        cpu.mem.wr32(ACTORS + 4 * k, ACTOR + k * 0x400);
    }
    let mut rng = Rng(0x4454_494B);
    let (mut grey, mut passes, mut voided, mut inert) = (0, 0, 0, 0);
    for case in 0..30_000 {
        let flags = rng.next() & !8 | if rng.below(10) == 0 { 8 } else { 0 };
        let colour = match rng.below(3) {
            0 => rng.next() & 0xFF00_0000,
            _ => rng.next(),
        };
        let seat = rng.below(8) as i16;
        let gates = BattleTickGates {
            no_escape: rng.below(3) != 0,
            second_monster: rng.below(3) == 0,
            seat_state: [0u8, 1, 2, 2, 3][rng.below(5) as usize],
        };
        let requested = rng.below(3) == 0;
        let signal = if rng.below(4) == 0 {
            rng.next() as u8
        } else {
            0xFF
        };
        let extra = rng.below(2) == 0;
        let nodes: Vec<Option<u32>> = (0..0x80)
            .map(|_| match rng.below(4) {
                0 => Some(rng.next()),
                _ => None,
            })
            .collect();
        let trail_before = rng.next() as u16;

        cpu.mem.wr32(NODE + 0x10, flags);
        cpu.mem.wr32(NODE + 0x74, colour);
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr16(NODE + 0x6A, trail_before);
        cpu.mem.wr8(CTX + 0x272, u8::from(requested));
        cpu.mem.wr8(CTX + 0x287, u8::from(gates.no_escape));
        cpu.mem.wr8(SECOND_MONSTER, u8::from(gates.second_monster));
        cpu.mem.wr8(BATTLE_SIGNAL, signal);
        cpu.mem.wr32(EXTRA_ARMED, u32::from(extra));
        cpu.mem
            .wr8(ACTOR + seat as u32 * 0x400 + 0x21C, gates.seat_state);
        for (i, n) in nodes.iter().enumerate() {
            let target = TARGETS + 0x20 * i as u32;
            cpu.mem
                .wr32(EFFECT_NODES + 4 * i as u32, n.map_or(0, |_| target));
            cpu.mem.wr32(target + 0x10, n.unwrap_or(0));
        }
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned);

        let view = BattleActorView {
            flags,
            colour,
            seat,
        };
        let tick = battle_actor_tick(&view, &gates);
        let pre = if tick.inert {
            Default::default()
        } else {
            battle_frame_passes(requested, signal, extra)
        };
        let mut want = Vec::new();
        if pre.ran {
            want.extend(FRAME_PASSES.map(Call::Pass));
            if pre.extra_hook {
                want.push(Call::Pass(EXTRA_HOOK));
            }
            passes += 1;
        }
        // The draw reads the stamped colour and the flag the bracket left.
        let trail_at_draw = tick.trail_flag.unwrap_or(trail_before);
        for s in &tick.steps {
            want.push(match s {
                BattleDrawStep::Draw => Call::Draw(tick.colour, trail_at_draw),
                BattleDrawStep::Afterimage => Call::Afterimage(body_shadow_skip(seat, true) as u16),
                s => Call::Step(*s),
            });
        }
        let ctx =
            format!("case {case}: {view:?} {gates:?} requested {requested} signal {signal:#x}");
        assert_eq!(*calls.borrow(), want, "{ctx}");
        assert_eq!(
            cpu.mem.rd16(NODE + 0x6A),
            trail_at_draw,
            "{ctx}: the trail flag"
        );
        if let Some(t) = tick.trail_flag {
            assert_eq!(t, body_shadow_skip(seat, false) as u16);
        }
        assert_eq!(
            cpu.mem.rd32(NODE + 0x74),
            tick.colour,
            "{ctx}: the colour word"
        );
        assert_eq!(
            cpu.mem.rd8(CTX + 0x272) != 0,
            requested && tick.inert,
            "{ctx}: the request byte"
        );
        // The sweep drops every node whose target is voided.
        let dropped = if pre.ran {
            voided_effect_nodes(&nodes)
        } else {
            vec![]
        };
        for (i, n) in nodes.iter().enumerate() {
            let held = cpu.mem.rd32(EFFECT_NODES + 4 * i as u32) != 0;
            assert_eq!(
                held,
                n.is_some() && !dropped.contains(&i),
                "{ctx}: node {i}"
            );
        }
        voided += dropped.len();
        grey += usize::from(tick.colour != colour);
        inert += usize::from(tick.inert);
    }
    assert!(
        grey > 500 && passes > 5000 && voided > 10_000 && inert > 2000,
        "{grey} {passes} {voided} {inert}"
    );
}
