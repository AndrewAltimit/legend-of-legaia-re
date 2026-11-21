//! The presentation tint machine: the commit's bystander sweep `FUN_8004E13C`
//! against `solo_commit_state`, and the per-actor arms of `FUN_80050120` against
//! `tint_sm_step` over every state, lane shape and fade gate.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_formulas::{
    FadeInputs, TintWords, solo_commit_state, tint_sm_step,
};
use psx_retail_exec::Stop;

const SWEEP: u32 = 0x8004_E13C;
const SM: u32 = 0x8005_0120;
const RAND: u32 = 0x8005_6798;
const CUE_STOP: u32 = 0x8001_7910;
const GRID_HOOK: u32 = 0x8004_695C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const FRAME_STEP: u32 = 0x1F80_0393;

fn actor(s: u32) -> u32 {
    0x8013_0000 + s * 0x400
}

#[test]
fn the_solo_sweep_is_retails() {
    differential!(scus(SWEEP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(RAND, 1);
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x534F_4C4F);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, actor(s));
    }
    for round in 0..40_000 {
        let acting = rng.below(7);
        let target = match rng.below(6) {
            0 => 8,
            1 => 9,
            2 => 0xFF,
            _ => rng.below(7),
        };
        cpu.mem.wr8(CTX + 0x13, acting as u8);
        cpu.mem.wr8(actor(acting) + 0x1DD, target as u8);
        let mut seeded = Vec::new();
        for s in 0..7 {
            let hp = if rng.below(4) == 0 {
                0
            } else {
                1 + rng.below(99) as u16
            };
            let state = [0u8, 0, 1, 2, 3, 0xC8][rng.below(6) as usize];
            cpu.mem.wr16(actor(s) + 0x14C, hp);
            cpu.mem.wr8(actor(s) + 0x21C, state);
            seeded.push((hp, state));
        }
        assert_eq!(cpu.call(SWEEP, &[1]), Stop::Returned, "{round}");
        for s in 0..7u32 {
            let (hp, state) = seeded[s as usize];
            let port = solo_commit_state(s as u8, hp, state, acting as u8, target as u8);
            let retail = cpu.mem.rd8(actor(s) + 0x21C);
            assert_eq!(
                retail, port,
                "round {round} seat {s} hp {hp} state {state:#x} acting {acting} target {target:#x}"
            );
        }
    }
}

#[test]
fn the_tint_arms_are_retails() {
    differential!(scus(SM));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(CUE_STOP, 0);
    cpu.stub(GRID_HOOK, 0);
    cpu.mem.wr32(CTX_PTR, CTX);
    // The fading monster's bookkeeping path never takes the capture arms.
    cpu.mem.wr8(CTX + 0x269, 1);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, actor(s));
        cpu.mem.wr32(actor(s) + 0x22C, 0x8014_0000 + s * 0x100);
    }
    for i in 0..4 {
        cpu.mem.wr32(RECORDS + i * 4, 0x8015_0000 + i * 0x100);
    }
    let mut rng = Rng(0x5449_4E54);
    let mut failures = Vec::new();
    let mut faded = 0u32;
    for round in 0..60_000 {
        let dt = 1 + rng.below(4) as u8;
        cpu.mem.wr8(FRAME_STEP, dt);
        let mut seeded = Vec::new();
        for s in 0..8u32 {
            let a = actor(s);
            let lane = |rng: &mut Rng| match rng.below(4) {
                0 => 0,
                1 => 0x80,
                2 => rng.below(0x10),
                _ => rng.below(0x400),
            };
            // A quarter of the words sit a step from neutral, so the decay
            // arm reaches its blend drain and selector clear.
            let color = if rng.below(4) == 0 {
                legaia_engine_battle_vm::battle_formulas::TINT_NEUTRAL
                    + [0, 0, 4, 0x1000][rng.below(4) as usize]
            } else {
                lane(&mut rng) | lane(&mut rng) << 10 | lane(&mut rng) << 20 | rng.below(4) << 30
            };
            let state = rng.below(0x0D) as u8;
            let blend = [0u32, 0x10, 0x800, 0x1000, 0x2000][rng.below(5) as usize];
            let selector = rng.below(4) as u8;
            let committed = [0u8, 8, 3, 0x20][rng.below(4) as usize];
            let knockdown = [3u8, 0x20, 9][rng.below(3) as usize];
            let capture = u8::from(rng.below(4) == 0);
            let mode = rng.next() & 0x00FF_FFFF;
            cpu.mem.wr32(a + 4, color);
            cpu.mem.wr32(a + 8, mode);
            cpu.mem.wr32(a + 0xC, blend);
            cpu.mem.wr8(a + 0x21C, state);
            cpu.mem.wr8(a + 0x21F, selector);
            cpu.mem.wr8(a + 0x1D9, committed);
            cpu.mem.wr8(a + 0x1F1, knockdown);
            cpu.mem.wr8(a + 0x225, capture);
            cpu.mem.wr8(a + 0x1DA, 0x33);
            cpu.mem.wr8(a + 0x1DC, 0x22);
            seeded.push((
                state, color, blend, selector, committed, knockdown, capture, mode,
            ));
        }
        assert_eq!(cpu.call(SM, &[]), Stop::Returned, "{round}");
        for s in 0..8u32 {
            let a = actor(s);
            let (state, color, blend, selector, committed, knockdown, capture, mode) =
                seeded[s as usize];
            let (w, fx) = tint_sm_step(
                state,
                TintWords {
                    color,
                    blend,
                    selector,
                },
                FadeInputs {
                    party: s < 3,
                    committed_anim: committed,
                    knockdown_entry: knockdown,
                    captured: capture != 0,
                },
                dt,
            );
            faded += u32::from(state == 2);
            let want_state = if fx.party_fade_done { 0 } else { state };
            let want_mode = if fx.mode_semi_transparent {
                mode | 0x8100_0000
            } else {
                mode
            };
            let want = (w.color, w.blend, w.selector, want_state, want_mode);
            let got = (
                cpu.mem.rd32(a + 4),
                cpu.mem.rd32(a + 0xC),
                cpu.mem.rd8(a + 0x21F),
                cpu.mem.rd8(a + 0x21C),
                cpu.mem.rd32(a + 8),
            );
            if want != got && failures.len() < 16 {
                failures.push(format!(
                    "round {round} seat {s} state {state} dt {dt} colour {color:#x} blend {blend:#x}: \
                     retail {got:x?} port {want:x?}"
                ));
            }
            if fx.party_fade_done {
                assert_eq!(cpu.mem.rd8(a + 0x1DA), 0, "staged clears");
                assert_eq!(cpu.mem.rd8(a + 0x1DC), 1, "commit flags");
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(faded > 20_000, "{faded}");
}

const AMBIENT: u32 = 0x8007_B7B0;
const FAR: u32 = 0x8007_BB48;
const OUTDOOR: u32 = 0x8007_BDA8;
const SCRATCH_FLAGS: u32 = 0x1F80_0394;
const BACKDROP_A: u32 = 0x8016_0000;
const BACKDROP_B: u32 = 0x8016_0100;

/// The pass's trailing scene block (`0x800505B0..0x800508B4`), with no actor
/// seated: the ambient base ramp (`ambient_base_step`), the backdrop pair's
/// cue weight (`backdrop_cue_step`) and its draw switch at the full weight,
/// the floor's skip (`ambient_store_skipped`) with the scratch draw bit, and
/// the ambient and far words (`battle_ambient_colour`, `grid_far_colour`).
#[test]
fn the_scene_ramps_are_retails() {
    use legaia_engine_battle_vm::battle_ground_grid as grid;

    differential!(scus(SM));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(GRID_HOOK, 0);
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, actor(s));
        cpu.mem.wr32(actor(s) + 0x22C, 0);
    }
    cpu.mem.wr32(CTX + 0x106C, BACKDROP_A);
    cpu.mem.wr32(CTX + 0x1070, BACKDROP_B);
    let mut rng = Rng(0x5343_454E);
    let (mut skipped, mut dimmed, mut full) = (0, 0, 0);
    for round in 0..40000 {
        let dt = 1 + rng.below(4) as u8;
        let channel =
            |rng: &mut Rng| -> u32 { [0x20, 0x21, 0x22, 0x40, 0x7E, 0x80][rng.below(6) as usize] };
        let base = match rng.below(4) {
            0 => grid::AMBIENT_BASE_FLOOR,
            1 => grid::AMBIENT_BASE_CEIL,
            // Every writer of the base stores one value in all three lanes
            // (the floor, the ceiling, the cast exits) and the ramp steps them
            // together, so the lanes stay equal.
            _ => {
                let c = channel(&mut rng);
                c << 2 | c << 12 | c << 22
            }
        };
        let latch = [0u8, 0, 1, 2, 3][rng.below(5) as usize];
        let c278 = [0u8, 0, 1, 2, 3][rng.below(5) as usize];
        let outdoor = rng.below(2) != 0;
        let cue = [
            0u16, 0x40, 0x7C0, 0x800, 0xBC0, 0xC00, 0xFC0, 0x1000, 0x1040,
        ][rng.below(9) as usize];
        let flags = rng.next();
        cpu.mem.wr8(FRAME_STEP, dt);
        cpu.mem.wr32(CTX + 0x890, base);
        cpu.mem.wr8(CTX + 0x243, latch);
        cpu.mem.wr8(CTX + 0x278, c278);
        cpu.mem.wr8(OUTDOOR, u8::from(outdoor));
        cpu.mem.wr16(BACKDROP_A + 0x78, cue);
        cpu.mem.wr16(BACKDROP_B + 0x78, cue);
        cpu.mem.wr16(BACKDROP_A + 0x56, 0x77);
        cpu.mem.wr16(BACKDROP_B + 0x56, 0x77);
        cpu.mem.wr32(SCRATCH_FLAGS, flags);
        cpu.mem.wr32(AMBIENT, 0x1234_5678);
        cpu.mem.wr32(FAR, 0x0ABC_DEF0);
        assert_eq!(cpu.call(SM, &[]), Stop::Returned, "{round}");

        let next_base = grid::ambient_base_step(base, latch != 0, dt);
        let next_cue = grid::backdrop_cue_step(cue, latch, c278, outdoor, dt);
        let rgb = grid::ambient_base_rgb(next_base);
        let skip = grid::ambient_store_skipped(rgb, latch, c278);
        let pack = |c: [u8; 3]| u32::from(c[0]) | u32::from(c[1]) << 8 | u32::from(c[2]) << 16;
        let (ambient, far, scratch) = if skip {
            (0x1234_5678, 0x0ABC_DEF0, flags & !8)
        } else {
            (
                pack(grid::battle_ambient_colour(rgb)),
                pack(grid::grid_far_colour(rgb, outdoor)),
                flags | 8,
            )
        };
        let draw = if next_cue == grid::BACKDROP_CUE_FULL {
            0
        } else {
            3
        };
        let at = format!(
            "round {round}: dt {dt} base {base:#x} latch {latch} 278 {c278} outdoor {outdoor} \
             cue {cue:#x}"
        );
        let retail = (
            cpu.mem.rd32(CTX + 0x890),
            cpu.mem.rd16(BACKDROP_A + 0x78),
            cpu.mem.rd16(BACKDROP_B + 0x78),
            cpu.mem.rd16(BACKDROP_A + 0x56),
            cpu.mem.rd16(BACKDROP_B + 0x56),
            cpu.mem.rd32(AMBIENT),
            cpu.mem.rd32(FAR),
            cpu.mem.rd32(SCRATCH_FLAGS),
        );
        assert_eq!(
            retail,
            (
                next_base, next_cue, next_cue, draw, draw, ambient, far, scratch
            ),
            "{at}"
        );
        skipped += i32::from(skip);
        dimmed += i32::from(latch != 0);
        full += i32::from(draw == 0);
    }
    assert!(
        skipped > 300 && dimmed > 10000 && full > 1000,
        "{skipped} {dimmed} {full}"
    );
}

const MONSTER_IDS: u32 = 0x8007_BD0C;
const NODES: u32 = 0x8014_0000;
const MONSTER_RECORD: u32 = 0x8015_0000;

/// Arm 2's bookkeeping on a fading monster body (`0x80050368..0x800504E8`),
/// from the pass's entry, against `World::tick_battle_defeat_sink` through
/// `BattleArmDrivers`: the Jette count (`jette_count_clears`) and the second
/// formation cell it clears, the sink by `(size * step) >> 2` with the floor
/// clip, and the lone scripted fight's latch `ctx[+0x288]` - with and without
/// a staged Seru absorb or a capture, which skip the sink but not the latch.
#[test]
fn the_defeat_fade_sinks_and_latches_as_retail_does() {
    use legaia_engine_battle::monster_catalog::MonsterDef;
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
    use legaia_save::Party;

    differential!(scus(SM));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(CUE_STOP, 0);
    cpu.stub(GRID_HOOK, 0);
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x5349_4E4B);
    let (mut sunk, mut latched, mut cleared) = (0, 0, 0);
    for round in 0..20000 {
        let n = 1 + rng.below(4) as u8;
        let jette = rng.below(2) == 0;
        let ids: Vec<u8> = (0..n)
            .map(|k| if jette && k == 0 { 0xAA } else { 0x20 + k })
            .collect();
        let dt = 1 + rng.below(3) as u8;
        let scripted = rng.below(2) == 0;
        let seru = [0u8, 0, 0, 5][rng.below(4) as usize];
        let mut seats = Vec::new();
        for _ in 0..n {
            let fading = rng.below(3) != 0;
            let lit = rng.below(4) != 0;
            let hp = if rng.below(3) == 0 { 30 } else { 0 };
            let capture = u8::from(rng.below(6) == 0);
            let size = [0u8, 8, 0x16, 0x30][rng.below(4) as usize];
            seats.push((fading, lit, hp, capture, size));
        }

        cpu.mem = clean.clone();
        let m = &mut cpu.mem;
        m.wr8(FRAME_STEP, dt);
        m.wr32(CTX_PTR, CTX);
        m.wr8(CTX, 3);
        m.wr8(CTX + 1, n);
        m.wr8(CTX + 0x269, seru);
        m.wr8(CTX + 0x287, u8::from(scripted));
        m.wr8(CTX + 0x288, 0);
        for k in 0..4u32 {
            m.wr8(MONSTER_IDS + k, ids.get(k as usize).copied().unwrap_or(0));
        }
        for s in 0..8u32 {
            m.wr32(ACTORS + s * 4, actor(s));
            m.wr32(actor(s) + 0x22C, 0);
        }
        for (k, &(fading, lit, hp, capture, size)) in seats.iter().enumerate() {
            let s = 3 + k as u32;
            let a = actor(s);
            let node = NODES + s * 0x100;
            let rec = MONSTER_RECORD + k as u32 * 0x100;
            m.wr32(RECORDS + k as u32 * 4, rec);
            m.wr8(rec + 0x1F, size);
            m.wr32(a + 0x22C, node);
            m.wr32(node + 0x74, if lit { 0x0060_6060 } else { 0 });
            m.wr16(node + 0x42, 0);
            m.wr32(a + 4, if lit { 0x1806_0180 } else { 0 });
            m.wr8(a + 0x21C, if fading { 2 } else { 5 });
            m.wr8(a + 0x1D9, 0);
            m.wr16(a + 0x14C, hp);
            m.wr8(a + 0x225, capture);
            m.wr16(a + 0x36, 0);
        }
        assert_eq!(cpu.call(SM, &[]), Stop::Returned, "{round}");
        let m = &mut cpu.mem;
        let retail_bodies: Vec<(i16, bool)> = (0..u32::from(n))
            .map(|k| {
                let s = 3 + k;
                (
                    m.rd16(actor(s) + 0x36) as i16,
                    m.rd16(NODES + s * 0x100 + 0x42) != 0,
                )
            })
            .collect();
        let retail = (m.rd8(CTX + 0x288), m.rd8(MONSTER_IDS + 1), m.rd8(CTX + 1));

        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(3, n);
        w.party.party_count = 3;
        w.battle.frame_clock.step = dt;
        w.battle_ctx.absorbed_seru = seru;
        w.battle_ctx.scripted_fight = u8::from(scripted);
        for (k, &(fading, lit, hp, capture, size)) in seats.iter().enumerate() {
            let id = u16::from(ids[k]);
            let mut def = MonsterDef::new(id, "m", 100, 10);
            def.size_class = size;
            w.tables.monster_catalog.insert(def);
            let a = &mut w.actors[3 + k];
            a.active = true;
            a.battle_monster_id = Some(id);
            a.battle.render_flag = if fading { 2 } else { 5 };
            a.battle.render_color = if lit { 0x1806_0180 } else { 0 };
            a.battle.hp = hp;
            a.battle.liveness = hp;
            a.battle.capture_state = capture;
            a.move_state.world_y = 0;
        }
        w.defeat_sink();
        let port_bodies: Vec<(i16, bool)> = (0..usize::from(n))
            .map(|k| {
                let a = &w.actors[3 + k];
                (a.move_state.world_y, a.battle.floor_clip)
            })
            .collect();
        let at = format!(
            "round {round}: ids {ids:x?} dt {dt} scripted {scripted} seru {seru} seats {seats:?}"
        );
        assert_eq!(retail_bodies, port_bodies, "bodies, {at}");
        assert_eq!(retail.0, w.battle_ctx.lone_defeat_latch, "latch, {at}");
        assert_eq!(retail.1, w.formation_second_cell(), "second cell, {at}");
        sunk += i32::from(retail_bodies.iter().any(|b| b.0 != 0));
        latched += i32::from(retail.0 != 0);
        cleared += i32::from(retail.2 == 1 && n > 1);
    }
    assert!(
        sunk > 2000 && latched > 2000 && cleared > 300,
        "{sunk} {latched} {cleared}"
    );
}
