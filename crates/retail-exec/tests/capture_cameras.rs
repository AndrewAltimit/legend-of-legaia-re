//! The capture-class bodies' cameras and countdowns, arm by arm off their module
//! images, against the camera directors
//! (`cast_module_camera::capture_camera_director`).
//!
//! Each arm runs twice: once with its countdown word high, where a gated arm
//! must hold, and once with the word one drain from zero, where it must pass.
//! The run compares the tween builder call `FUN_801D829C` the arm makes (its
//! angle, translation and focus trios and its frame count) with the
//! director's `shot`, the countdown word it leaves with the director's, and a
//! hold with a phase the body left alone. Every call out of the module image
//! but the builder and `rand()` returns `0`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_target_group::GroupAim;
use legaia_engine_cast::cast_module_camera::{
    self as cam, CaptureCamDirector, ModuleCamSeats, ModuleCamState, ModuleSeat, SPEED_SCALAR,
};
use legaia_engine_cast::cast_module_ticks as t;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Memory, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const NODES: u32 = 0x8014_0000;
const RAND: u32 = 0x8005_6798;
const BUILDER: u32 = 0x801D_829C;
const BEARING: u32 = 0x8001_9B28;
const AIM: u32 = 0x801D_CEAC;
/// The range metric `FUN_8004E2F0` (its own differential is `range_metric`).
const RANGE: u32 = 0x8004_E2F0;
const OUT_CENTRE: u32 = 0x8011_0000;
const OUT_EXTENT: u32 = 0x8011_0100;
/// The battle frame's speed scalar and step (`0x1F800314 + 0x69` / `+ 0x7F`).
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const FORMATION: u32 = 0x8007_BD0C;
const ROSTER: u32 = 0x8007_BD10;

/// One tween-builder call, as its arguments name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shot {
    angles: [i16; 3],
    tr: [i16; 3],
    focus: [i16; 3],
    frames: u16,
}

fn trio(m: &mut Memory, a: u32) -> [i16; 3] {
    [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16]
}

struct Body {
    entry: u32,
    va: u32,
    action: u8,
    director: CaptureCamDirector,
}

fn bodies() -> Vec<Body> {
    let mut out = Vec::new();
    for action in 0..=0xFFu8 {
        if let Some((entry, va)) = t::capture_body_for_action(action)
            && let Some(director) = cam::capture_camera_director(entry, va)
            && !out.iter().any(|b: &Body| (b.entry, b.va) == (entry, va))
        {
            out.push(Body {
                entry,
                va,
                action,
                director,
            });
        }
    }
    // Cort's Evil Seru Magic (`0xAD`) casts through the boss band rather
    // than the capture table, on the same single body and director shape.
    out.push(Body {
        entry: 966,
        va: EVIL_SERU_MAGIC,
        action: 0xAD,
        director: cam::capture_camera_director(966, cam::SINGLE_BODY).expect("directed"),
    });
    out
}

/// Every `jal` target the image's words encode.
fn jal_targets(module: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(BASE..BASE + 0x8000).contains(&t))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn seat_at(m: &mut Memory, a: u32, display: bool) -> ModuleSeat {
    let o = if display { 0x3C } else { 0x34 };
    ModuleSeat {
        x: m.rd16(a + o) as i16,
        y: m.rd16(a + o + 2) as i16,
        z: m.rd16(a + o + 4) as i16,
        facing: m.rd16(a + 0x46) & 0xFFF,
    }
}

/// Seed the context, the eight seats and the frame globals for one pass of
/// `body`'s arm `phase`, and return the caster's and victim's seats.
fn seed(cpu: &mut Cpu, rng: &mut Rng, module: &[u8], body: &Body, phase: u8) -> (u32, u32) {
    cpu.mem.load(BASE, module);
    for b in 0..0x1100 {
        cpu.mem.wr8(CTX + b, 0);
    }
    let caster = 3 + rng.below(5);
    let victim = rng.below(3);
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 5);
    cpu.mem.wr8(CTX + 0x13, caster as u8);
    cpu.mem.wr8(CTX + 0x279, phase);
    cpu.mem.wr16(CTX + 0x6D0, 0x800 + rng.below(0x1000) as u16);
    cpu.mem.wr16(CTX + 0x6D8, rng.below(0x100) as u16);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE).step_by(4) {
            cpu.mem.wr32(a + o, 0);
        }
        for o in [0x34u32, 0x36, 0x38, 0x3C, 0x3E, 0x40] {
            cpu.mem
                .wr16(a + o, (rng.below(0x1000) as u16).wrapping_sub(0x800));
        }
        cpu.mem.wr16(a + 0x46, rng.below(0x1000) as u16);
        cpu.mem.wr16(a + 0x14C, 1 + rng.below(500) as u16);
        cpu.mem.wr16(a + 0x168, 1 + rng.below(200) as u16);
        cpu.mem.wr16(a + 0x16A, 1 + rng.below(200) as u16);
        cpu.mem.wr8(a + 0x1DF, body.action);
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x58, 0x40 + rng.below(0x100) as u16);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
    cpu.mem.wr8(SCALAR, SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, 1);
    cpu.mem.wr8(FORMATION, 0);
    for i in 0..3 {
        cpu.mem.wr8(ROSTER + i, 1 + i as u8);
    }
    (caster, victim)
}

#[test]
fn the_capture_cameras_frame_and_gate_as_retail_does() {
    differential!(
        prot(938, MYSTIC_CIRCLE),
        prot(938, CHAOS_BREATH),
        prot(940, MYSTIC_SHIELD),
        prot(940, GLARE),
        prot(941, STONE_CIRCLE),
        prot(941, STEAL),
        prot(942, POWER_UP),
        prot(942, DARK_TYPHOON),
        prot(943, LAPIS_WAVE),
        prot(944, GUILTY_CROSS),
        prot(944, CURSE_ALL),
        prot(945, WATER_COLUMN),
        prot(950, SHADOW_BREAK),
        prot(950, ROLLING_FLARE),
        prot(951, SCYTHE_WIND),
        prot(951, CHAOS_FLARE),
        prot(952, BLOODY_HORNS),
        prot(952, ASTRAL_SLASH),
        prot(955, KISS_OF_DEATH),
        prot(955, TERROR_SCREAM),
        prot(955, MELT_SPRAY),
        prot(955, VOID_ACCESSORIES),
        prot(955, WHITE_SHIELD),
        prot(955, POWER_CHARGE),
        prot(956, PARALYZING_WAVE),
        prot(956, WATER_HAZARD),
        prot(957, DEATH_GAME),
        prot(957, THUNDER_STORM),
        prot(958, BLAZING_SLASH),
        prot(959, MEGATON_PRESS),
        prot(960, PLASMA_STRIKE),
        prot(960, NEO_STAR_SLASH),
        prot(961, DEAD_END_CRISIS),
        prot(962, ULTRA_CHARGE),
        prot(962, BLADE_BREATH),
        prot(962, THUNDER_NEEDLE),
        prot(962, GIGATON_PRESS),
        prot(964, ELEMENT_CHANGE),
        prot(964, ELEMENT_STRIKE),
        prot(965, DOOMSDAY),
        prot(966, EVIL_SERU_MAGIC)
    );
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let shots = Rc::new(RefCell::new(Vec::<Shot>::new()));
    let stream = Rc::new(std::cell::Cell::new(1u32));
    // A fresh machine per body, so no body runs on what another's stubbed
    // calls left in RAM.
    let fresh = || {
        let mut cpu = scus_cpu().expect("SCUS");
        cpu.mem
            .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        {
            let shots = shots.clone();
            cpu.hook(BUILDER, move |r, m| {
                shots.borrow_mut().push(Shot {
                    angles: trio(m, r.r[A0]),
                    tr: trio(m, r.r[A1]),
                    focus: trio(m, r.r[A2]),
                    frames: r.r[A3] as u16,
                });
            });
        }
        {
            let stream = stream.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
            });
        }
        // The 12-bit bearing, as its own differential holds it (`bearing`).
        cpu.hook(BEARING, |r, _| {
            let a = |i: usize| r.r[A0 + i] as i16;
            r.r[V0] = u32::from(bearing_12bit_approx(a(0), a(1), a(2), a(3)));
        });
        cpu
    };
    let polls = Rc::new(std::cell::Cell::new(0u32));
    let mut rng = Rng(0x4341_4D53);
    let mut failures = Vec::new();
    let (mut passes, mut casts) = (0u32, 0u32);
    let mut outcomes = std::collections::BTreeSet::new();
    let mut unsettled_holds = 0usize;
    let (mut results, mut heal_capped) = (0usize, false);
    let all = bodies();
    for body in &all {
        let module = disc.read_entry(body.entry as usize).expect("module");
        let mut cpu = fresh();
        // Every routine the image calls returns `0`, but the group aim,
        // which the bodies' group framings need whole.
        for target in jal_targets(&module) {
            if ![AIM, RANGE, BUILDER, BEARING, RAND].contains(&target) {
                cpu.stub(target, 0);
            }
        }
        // Death Game keeps its card handles (`0x801FA2B8`) and walks them
        // through the shuffle and the pick: each spawn gets a part.
        let death_game = body.action == 0x76;
        if death_game {
            let next = Rc::new(std::cell::Cell::new(PARTS));
            for spawn in [SPAWN_PART, SPAWN_SEATED] {
                let next = next.clone();
                cpu.hook(spawn, move |r, m| {
                    let at = next.get();
                    next.set(at + 0x80);
                    for o in (0..0x80).step_by(4) {
                        m.wr32(at + o, 0);
                    }
                    r.r[V0] = at;
                });
            }
            cpu.mem.wr32(CONFIRM_MASK, 0x20);
        }
        // The range poll answers "in reach", the approach being the host's,
        // but every fourth poll of a pass answers "out of reach": PROT
        // 0941's backstep (`0x801F7620..0x801F76E4`) walks until it is.
        {
            let polls = polls.clone();
            let backstep = body.action == 0x51;
            cpu.hook(RANGE, move |r, _| {
                polls.set(polls.get() + 1);
                r.r[V0] = u32::from(backstep && polls.get().is_multiple_of(4));
            });
        }
        let countdown_va = cam::capture_countdown_va(body.action);
        let rounds = match body.action {
            0x76 => 30,
            0x71 | 0x70 => 12,
            _ => 6,
        };
        for round in 0..rounds {
            let (caster, victim) = seed(&mut cpu, &mut rng, &module, body, 0);
            let (ca, va_) = (ACTOR + caster * STRIDE, ACTOR + victim * STRIDE);
            // The caster's formation id, which a body can fork on: Terror
            // Scream's owners (Terror Face `0x7F`, the `0x28` row).
            let monster = match body.action {
                // The three caster bands its arm 0 forks on (`0x801F7764`,
                // `0x801F78B8`): Terror Face `0x7F`, the `0x28` row and the
                // `0x83` row, each picking its cue and cast clip.
                0x70 => [0x28u8, 0x7F, 0x83, 0x29, 0x84, 0x2A, 0x85][round % 7],
                // The two casters that deal a stacked table.
                0x76 => [0u8, 0x9D, 0x9E][round % 3],
                _ => 0,
            };
            for i in 0..5 {
                cpu.mem.wr8(FORMATION + i, monster);
            }
            // Death Game's heal reads each seat's maximum `+0x14E` and caps
            // the quarter at the missing HP: a seat at full, one short and
            // one wounded.
            if death_game {
                for a in [ca, va_] {
                    let hp = cpu.mem.rd16(a + 0x14C);
                    let short = [0, 1, hp / 8, hp][(round + (a == va_) as usize) % 4];
                    cpu.mem.wr16(a + 0x14E, hp + short);
                }
            }
            // Curse All takes the party as a group.
            // Water Hazard sweeps the party or the monster row as well as a
            // seat, and its arm 3 waits on the row its code names.
            let water_hazard = body.action == 0x71;
            let group = match body.action {
                0x53 => Some(8u8),
                0x71 => [None, Some(8u8), Some(9)][round % 3],
                _ => None,
            };
            if let Some(g) = group {
                cpu.mem.wr8(ca + 0x1DD, g);
            }
            // The tint word the group aim counts a monster seat up by
            // (`FUN_800513F0`'s seating, `0x80051874`).
            if water_hazard {
                for s in 3..8 {
                    cpu.mem.wr32(ACTOR + s * STRIDE + 4, 0x1004_0100);
                }
            }
            // Plasma Strike's arm 5 waits for its strike clip `0x0D` past
            // a cursor, and arm 6 for the clip's loops to run out.
            if body.action == 0x7B {
                cpu.mem.wr8(ca + 0x1D9, 0x0D);
            }
            // PROT 0958's arm 24 holds while the victim's playing clip
            // `+0x1D9` is its knockdown slot `+0x1F1` (`0x801F8B0C..0x801F8B18`).
            // The clip's end is the animation driver's, which is stubbed, so
            // the rig plays it: on the clip for `round` passes of the arm,
            // then off it.
            const KNOCKDOWN: u8 = 5;
            let blazing = (body.entry, body.va) == (958, BLAZING_SLASH);
            if blazing {
                cpu.mem.wr8(va_ + 0x1F1, KNOCKDOWN);
            }
            let mut arm_24_passes = 0usize;
            let (mut arm_passes, mut last_phase) = (0usize, 0xFEu8);
            let mut arm3_passes = 0usize;
            // Every strike-frame wait (PROT 0960's arm 5, PROT 0962's arm 3)
            // finds the caster's node cursor past its mark.
            cpu.mem.wr16(NODES + caster * 0x100 + 0x68, 0x7FFF);
            let mut st = ModuleCamState {
                lead_roster: 1,
                opens_pass: true,
                frame_step: 1,
                victim_settled: true,
                caster_in_reach: true,
                party_settled: true,
                target_code: Some(victim as u8),
                ..ModuleCamState::default()
            };
            stream.set(if death_game {
                1 + 0x9E37 * round as u32
            } else {
                1
            });
            let c0 = format!(
                "PROT {:04} body {:#010x} ({:#04x}) round {round}",
                body.entry, body.va, body.action
            );
            let mut finished = false;
            // Evil Seru Magic's gates re-arm in vsyncs and add up to past
            // three thousand.
            let cap = if body.entry == 966 { 8000 } else { 3000 };
            for pass in 0..cap {
                let phase = cpu.mem.rd8(CTX + 0x279);
                // PROT 0961's arm 0 stands the seats before its cut; the host
                // runs the line-up ahead of the director
                // (`World::dead_end_crisis_seats`), so the director frames the
                // stood caster.
                let lined = (body.entry, body.va, phase) == (961, DEAD_END_CRISIS, 0);
                let caster_seat = if lined {
                    let mut poses =
                        [cam::LineUpPose::default(); cam::DEAD_END_CRISIS_LINE_UP_SEATS];
                    for (s, p) in poses.iter_mut().enumerate() {
                        let a = ACTOR + s as u32 * STRIDE;
                        p.pos = [0u32, 2, 4, 6].map(|o| cpu.mem.rd16(a + 0x34 + o) as i16);
                        p.facing = cpu.mem.rd16(a + 0x46);
                    }
                    let row_x = std::array::from_fn(|s| {
                        legaia_engine_battle::battle_seats::monster_seat(1, s, false).x
                    });
                    cam::dead_end_crisis_line_up(&mut poses, row_x);
                    let mut seat = seat_at(&mut cpu.mem, ca, false);
                    if let Some(p) = poses.get(caster as usize) {
                        (seat.x, seat.z, seat.facing) = (p.pos[0], p.pos[2], p.facing & 0xFFF);
                    }
                    seat
                } else {
                    seat_at(&mut cpu.mem, ca, false)
                };
                let seats = ModuleCamSeats {
                    caster: caster_seat,
                    victim: seat_at(&mut cpu.mem, va_, false),
                    band_timer: i32::from(cpu.mem.rd16(CTX + 0x6D8) as i16),
                    action: body.action,
                    depth_raw: i32::from(cpu.mem.rd16(CTX + 0x6D0) as i16),
                    victim_radius: cpu.mem.rd16(NODES + victim * 0x100 + 0x58) as i16,
                    caster_scale: 0x1000,
                    caster_monster: monster,
                    first_monster: monster,
                    ..ModuleCamSeats::default()
                };
                // Water Hazard aims once, at arm 0 (`0x801F7394..0x801F73E4`),
                // before it hides the row the aim counts.
                if let Some(g) = group.filter(|_| !water_hazard || phase == 0) {
                    assert_eq!(
                        cpu.call(AIM, &[u32::from(g), OUT_CENTRE, OUT_EXTENT]),
                        Stop::Returned,
                        "{c0} pass {pass} arm {phase}: the group aim {g}"
                    );
                    st.target_aim = Some(GroupAim {
                        centroid_x: cpu.mem.rd16(OUT_CENTRE) as i16,
                        centroid_z: cpu.mem.rd16(OUT_CENTRE + 4) as i16,
                        extent: cpu.mem.rd16(OUT_EXTENT + 4) as i16,
                    });
                    st.target_code = Some(g);
                }
                // What the host feeds a director every pass
                // (`feed_capture_cam`).
                st.caster_clip = Some(cpu.mem.rd8(ca + 0x1D9));
                st.caster_frame = Some(cpu.mem.rd16(NODES + caster * 0x100 + 0x68));
                st.caster_loops = Some(cpu.mem.rd8(ca + 0x21B));
                polls.set(0);
                st.caster_body = Some(seat_at(&mut cpu.mem, ca, true));
                st.victim_body = Some(seat_at(&mut cpu.mem, va_, true));
                if blazing {
                    let on = phase != 24 || arm_24_passes < round;
                    arm_24_passes += usize::from(phase == 24);
                    cpu.mem.wr8(va_ + 0x1D9, if on { KNOCKDOWN } else { 0 });
                    st.victim_on_knockdown_clip = on;
                }
                // Evil Seru Magic's arm 0 saves the caster's pose before its
                // re-seat, and arm 27's shot frames it restored; the host
                // keeps the saved pose (`evil_seru_magic_row`).
                if body.entry == 966 && phase == 0 {
                    st.caster_saved = Some((caster_seat.x, caster_seat.z, caster_seat.facing));
                }
                // Death Game's host feeds: the deal and each swap's draws off
                // the stream ahead of the pass, the cards' live points, the
                // pad, and the seats the closing arms wait on.
                if death_game {
                    let lcg = |seed: &mut u32| {
                        *seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                        (*seed >> 16) & 0x7FFF
                    };
                    let mut seed = stream.get();
                    let g = &mut st.death_game;
                    if phase == 0 {
                        g.deal(monster, || lcg(&mut seed));
                    }
                    if phase == 4 && g.counter == 0 {
                        g.draw_swap(|| lcg(&mut seed));
                    }
                    for i in 0..4u32 {
                        let h = cpu.mem.rd32(CARDS + 4 * i);
                        if h != 0 {
                            g.cards[i as usize] =
                                [0u32, 2, 4].map(|o| cpu.mem.rd16(h + 0x14 + o) as i16);
                        }
                    }
                    arm_passes = if phase == last_phase {
                        arm_passes + 1
                    } else {
                        0
                    };
                    last_phase = phase;
                    let press = match (phase, arm_passes) {
                        (5, n) if n == round + 3 => 0x20,
                        (5, n) if n % 5 == 1 => [0x2000, 0x8000][(n / 5) % 2],
                        _ => 0,
                    };
                    cpu.mem.wr32(PRESSED, press);
                    g.pad.right = press & 0x2000 != 0;
                    g.pad.left = press & 0x8000 != 0;
                    g.pad.confirm = press & 0x20 != 0;
                    // The closing arms: the victim busy, then down on clip 8;
                    // the caster on a clip, then idle and faded.
                    if (10..=12).contains(&phase) {
                        let done = arm_passes >= round + 2;
                        cpu.mem.wr8(va_ + 0x1D9, if done { 8 } else { 5 });
                        cpu.mem.wr8(ca + 0x1D9, if done { 0 } else { 1 });
                        cpu.mem.wr32(ca + 4, if done { 0 } else { 0x2008_0200 });
                        st.victim_settled = if phase == 10 { done } else { true && done };
                        st.caster_settled = done;
                        st.death_game.caster_faded = done;
                    }
                }
                // Water Hazard's arm 3: a seat of the target row plays its
                // reaction (or, dead, has not reached its rest) for a few
                // passes, then settles; the host's flag off the same bytes
                // (`World::capture_target_settled`).
                if water_hazard {
                    // Passes of arm 3 since its word ran out (it drains
                    // `scalar` a pass off the `1 << 7` arm 2 left).
                    arm3_passes = if phase == 3 && st.countdown.0 <= SPEED_SCALAR {
                        arm3_passes + 1
                    } else {
                        0
                    };
                    let code = cpu.mem.rd8(ca + 0x1DD);
                    let party = (0..3u32).collect::<Vec<_>>();
                    let row = (3..8u32).collect::<Vec<_>>();
                    let rows = match code {
                        0..=2 => vec![u32::from(code)],
                        8 => party,
                        _ => row,
                    };
                    if phase == 3 {
                        let s = rows[round % rows.len()];
                        let a = ACTOR + s * STRIDE;
                        let dead = round % 2 == 1;
                        let busy = arm3_passes <= round + 1;
                        if dead {
                            cpu.mem.wr16(a + 0x14C, 0);
                        }
                        let rest = if dead && s < 3 { 8 } else { 0 };
                        cpu.mem
                            .wr8(a + 0x1D9, if busy && !(dead && s >= 3) { 5 } else { rest });
                        if dead && s >= 3 {
                            cpu.mem.wr32(a + 4, if busy { 0x2008_0200 } else { 0 });
                        }
                    }
                    let settled = |cpu: &mut Cpu, s: u32| {
                        let a = ACTOR + s * STRIDE;
                        let clip = cpu.mem.rd8(a + 0x1D9);
                        match (cpu.mem.rd16(a + 0x14C) != 0, s < 3) {
                            (true, _) => clip == 0,
                            (false, true) => clip == 8,
                            (false, false) => cpu.mem.rd32(a + 4) == 0,
                        }
                    };
                    st.target_unsettled = !rows.iter().all(|&s| settled(&mut cpu, s));
                    unsettled_holds += usize::from(phase == 3 && st.target_unsettled);
                }
                let arm = (body.director)(&mut st, phase, seats);
                // The two seats as the pass opens, for Death Game's result.
                let before = [ca, va_].map(|a| crate::cast_view::read_actor(&mut cpu.mem, a));
                shots.borrow_mut().clear();
                let stop = cpu.call(body.va, &[]);
                let c = format!("{c0} pass {pass} arm {phase}");
                if stop != Stop::Returned {
                    failures.push(format!("{c}: {stop:?}"));
                    break;
                }
                passes += 1;
                let busy = cpu.regs.r[V0] != 0;
                // The last builder call of a pass is the tween the camera runs.
                let got = shots.borrow().last().copied();
                let want = arm.shot.map(|s| Shot {
                    angles: s.angles,
                    tr: s.tr,
                    focus: s.focus,
                    frames: s.frames,
                });
                // The builder masks the yaw; compare it as it is held.
                let held = |s: Option<Shot>| {
                    s.map(|mut s| {
                        s.angles[1] &= 0xFFF;
                        s
                    })
                };
                let known = KNOWN_GAPS
                    .iter()
                    .any(|g| (g.0, g.1, g.2) == (body.entry, body.va, phase));
                if held(got) != held(want) && !known {
                    failures.push(format!("{c}: shot retail {got:?} port {want:?}"));
                }
                let after = cpu.mem.rd8(CTX + 0x279);
                if death_game {
                    // The word `0x801FA278`, which `capture_countdown_va`
                    // does not list.
                    let word = cpu.mem.rd32(DEATH_GAME_WORD) as i32;
                    if word != st.countdown.0 {
                        failures.push(format!("{c}: word retail {word} port {}", st.countdown.0));
                        st.countdown.0 = word;
                    }
                    if phase == 0 {
                        let table: Vec<u8> = (0..4u32)
                            .map(|i| cpu.mem.rd32(TABLE + 4 * i) as u8)
                            .collect();
                        if table != st.death_game.table {
                            failures.push(format!(
                                "{c}: deal retail {table:?} port {:?}",
                                st.death_game.table
                            ));
                        }
                    }
                    if !arm.hold && arm.next.is_some_and(|n| n != after) {
                        failures.push(format!(
                            "{c}: retail moved to {after:#x}, port {:?}",
                            arm.next
                        ));
                    }
                    // The picked card's result (`0x801F922C..0x801F9794`)
                    // on the seats it lands on, against the kernel the host
                    // runs (`death_game_outcome`).
                    if let Some(o) = st.death_game.outcome {
                        outcomes.insert(format!("{o:?}"));
                        let [mut pc, mut pv] = before;
                        let max = [ca, va_].map(|a| cpu.mem.rd16(a + 0x14E));
                        t::death_game_outcome(o, &mut pc, &mut pv, max);
                        // The arm's seat writes beside the kernel's: the
                        // caster quieted, the victim shown.
                        for (v, w) in [(&mut pc, &arm.caster_write), (&mut pv, &arm.victim_write)] {
                            if let Some(x) = w.stage {
                                v.staged_anim = x;
                            }
                            if let Some(x) = w.rate {
                                v.anim_rate = x;
                            }
                            if let Some(x) = w.render_flag {
                                v.render_flag = x;
                            }
                            if let Some(x) = w.colour {
                                v.present_04 = x;
                            }
                        }
                        for (who, a, port) in [("caster", ca, pc), ("victim", va_, pv)] {
                            let got = crate::cast_view::read_actor(&mut cpu.mem, a);
                            let key = |x: &t::CastActorState| {
                                (
                                    x.hp,
                                    x.hp_bar_delta,
                                    x.staged_anim,
                                    x.restage,
                                    x.anim_rate,
                                    x.render_flag,
                                    x.present_04,
                                )
                            };
                            if key(&got) != key(&port) {
                                failures.push(format!(
                                    "{c}: {o:?} {who} retail {:?} port {:?}",
                                    key(&got),
                                    key(&port)
                                ));
                            }
                        }
                        results += 1;
                        heal_capped |= o == cam::DeathGameOutcome::BothHeal
                            && before
                                .iter()
                                .zip(max)
                                .any(|(b, m)| u32::from(m - b.hp) < (u32::from(m) + 3) >> 2);
                    }
                }
                if water_hazard && phase == 3 && !arm.hold && after == phase {
                    failures.push(format!("{c}: port passes, retail holds"));
                }
                if arm.hold && after != phase {
                    failures.push(format!("{c}: port holds, retail moved to {after}"));
                }
                if let Some(va) = countdown_va {
                    let word = cpu.mem.rd32(va) as i32 * cam::capture_countdown_units(body.action);
                    if word != st.countdown.0 {
                        failures.push(format!(
                            "{c}: countdown retail {word} port {}",
                            st.countdown.0
                        ));
                        st.countdown.0 = word;
                    }
                }
                if !busy {
                    finished = true;
                    break;
                }
            }
            if !finished {
                failures.push(format!(
                    "{c0}: no finish in {cap} passes at arm {}",
                    cpu.mem.rd8(CTX + 0x279)
                ));
            }
            casts += 1;
        }
    }
    eprintln!("{} bodies, {casts} casts, {passes} passes", all.len());
    eprintln!("Death Game outcomes {outcomes:?}");
    assert_eq!(outcomes.len(), 4, "Death Game outcomes {outcomes:?}");
    assert!(
        results >= 4 && heal_capped,
        "Death Game results compared {results}, a heal capped by the missing HP {heal_capped}"
    );
    assert!(
        unsettled_holds > 10,
        "Water Hazard's settle held {unsettled_holds} passes"
    );
    let n = failures.len();
    failures.truncate(400);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}

/// Shots the port is known to frame differently, as `(entry, body, arm,
/// why)`; the rest of each arm is still compared.
const KNOWN_GAPS: [(u32, u32, u8, &str); 1] = [(
    961,
    DEAD_END_CRISIS,
    5,
    "the no-one-standing branch frames nothing; the director cuts on it (documented \
     on dead_end_crisis_camera)",
)];

const MYSTIC_CIRCLE: u32 = 0x801F_69EC;
const CHAOS_BREATH: u32 = 0x801F_726C;
const MYSTIC_SHIELD: u32 = 0x801F_7240;
const GLARE: u32 = 0x801F_69F8;
const STONE_CIRCLE: u32 = 0x801F_6A04;
const STEAL: u32 = 0x801F_730C;
const POWER_UP: u32 = 0x801F_7D34;
const DARK_TYPHOON: u32 = 0x801F_69F4;
const LAPIS_WAVE: u32 = 0x801F_6A04;
const GUILTY_CROSS: u32 = 0x801F_6A04;
const CURSE_ALL: u32 = 0x801F_7470;
const WATER_COLUMN: u32 = 0x801F_6EDC;
const SHADOW_BREAK: u32 = 0x801F_6A24;
const ROLLING_FLARE: u32 = 0x801F_79F8;
const SCYTHE_WIND: u32 = 0x801F_77E8;
const CHAOS_FLARE: u32 = 0x801F_6A20;
const BLOODY_HORNS: u32 = 0x801F_7118;
const ASTRAL_SLASH: u32 = 0x801F_6A0C;
const KISS_OF_DEATH: u32 = 0x801F_86A4;
const TERROR_SCREAM: u32 = 0x801F_767C;
const MELT_SPRAY: u32 = 0x801F_7FA4;
const VOID_ACCESSORIES: u32 = 0x801F_6A28;
const WHITE_SHIELD: u32 = 0x801F_8F0C;
const POWER_CHARGE: u32 = 0x801F_7158;
const PARALYZING_WAVE: u32 = 0x801F_69D8;
const WATER_HAZARD: u32 = 0x801F_7298;
const DEATH_GAME: u32 = 0x801F_798C;
const THUNDER_STORM: u32 = 0x801F_6A14;
const BLAZING_SLASH: u32 = 0x801F_6DD8;
const MEGATON_PRESS: u32 = 0x801F_69F0;
const PLASMA_STRIKE: u32 = 0x801F_74E4;
const NEO_STAR_SLASH: u32 = 0x801F_69D8;
const DEAD_END_CRISIS: u32 = 0x801F_69D8;
const ULTRA_CHARGE: u32 = 0x801F_69D8;
const BLADE_BREATH: u32 = 0x801F_7AE4;
const THUNDER_NEEDLE: u32 = 0x801F_74A0;
const GIGATON_PRESS: u32 = 0x801F_6D54;
const ELEMENT_CHANGE: u32 = 0x801F_88EC;
const ELEMENT_STRIKE: u32 = 0x801F_69D8;
const DOOMSDAY: u32 = 0x801F_69D8;
const EVIL_SERU_MAGIC: u32 = 0x801F_6A74;
/// The part spawn `FUN_80021B04` and the pool the walk hands parts from.
const SPAWN_PART: u32 = 0x8002_1B04;
const PARTS: u32 = 0x8016_0000;
/// The seated part spawn `FUN_80050ED4` Death Game's faces come from
/// (`0x801F7F08`).
const SPAWN_SEATED: u32 = 0x8005_0ED4;
/// Death Game's card handles and dealt table (`0x801FA2B8`, `0x801FA288`),
/// the pad's press word and the confirm button's mask.
const CARDS: u32 = 0x801F_A2B8;
const DEATH_GAME_WORD: u32 = 0x801F_A278;
const TABLE: u32 = 0x801F_A288;
const PRESSED: u32 = 0x8007_B874;
const CONFIRM_MASK: u32 = 0x8008_46D0;

/// Water Hazard's arm 3 (`0x801F7B90`): once its countdown runs out it walks
/// the seats the caster's target code names and passes (phase `0xFF`) only
/// when every one has settled (`0x801F7C2C..0x801F7DA4`): a code below `8`
/// one seat, `8` the `ctx[+0]` party seats, any other the `ctx[+1]` monster
/// seats from seat `3`; a live seat idle (`+0x1D9 == 0`), a downed party seat
/// on clip `8`, a dead monster faded (`+0x04 == 0`). Against
/// `World::capture_target_settled` through `BattleArmDrivers`, on a world
/// seated from the same table: small parties (the engine's compacted row),
/// short rows, and now and then an active actor past the seated row that the
/// walk must not reach.
#[test]
fn water_hazard_waits_for_the_seats_it_names_as_retail_does() {
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
    use legaia_save::Party;
    differential!(prot(956, WATER_HAZARD));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Arm 3's countdown word (`lw v1,-0x7960(a0)` at `0x801F7C10`).
    const COUNTDOWN: u32 = 0x801F_86A0;
    let body = bodies()
        .into_iter()
        .find(|b| b.action == 0x71)
        .expect("Water Hazard");
    let module = disc.read_entry(956).expect("PROT 0956");
    let mut cpu = scus_cpu().expect("SCUS");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for target in jal_targets(&module) {
        cpu.stub(target, 0);
    }
    let mut rng = Rng(0x5741_5452);
    let (mut passed, mut held, mut extras) = (0u32, 0u32, 0u32);
    let mut codes = std::collections::BTreeSet::new();
    for round in 0..20_000u32 {
        let (_, _) = seed(&mut cpu, &mut rng, &module, &body, 3);
        let pc = 1 + rng.below(3) as u8;
        let m = 1 + rng.below(5) as u8;
        let caster = 3 + rng.below(u32::from(m)) as u8;
        let code = match rng.below(4) {
            0 => rng.below(u32::from(pc)) as u8,
            1 => 3 + rng.below(u32::from(m)) as u8,
            2 => 8,
            _ => 9,
        };
        cpu.mem.wr8(CTX, pc);
        cpu.mem.wr8(CTX + 1, m);
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr32(COUNTDOWN, 1);
        // (hp, clip, colour word) per retail seat; the empty party seats
        // are the zeroed structs the loader leaves.
        let mut seats = [(0u16, 0u8, 0u32); 8];
        for (s, seat) in seats.iter_mut().enumerate() {
            let used = s < usize::from(pc) || (3..3 + usize::from(m)).contains(&s);
            if !used {
                continue;
            }
            let alive = rng.below(3) != 0;
            *seat = (
                if alive { 1 + rng.below(300) as u16 } else { 0 },
                [0u8, 0, 5, 8][rng.below(4) as usize],
                if rng.below(2) == 0 { 0 } else { 0x1004_0100 },
            );
        }
        for (s, &(hp, clip, colour)) in seats.iter().enumerate() {
            let a = ACTOR + s as u32 * STRIDE;
            cpu.mem.wr16(a + 0x14C, hp);
            cpu.mem.wr8(a + 0x1D9, clip);
            cpu.mem.wr32(a + 4, colour);
        }
        cpu.mem
            .wr8(ACTOR + u32::from(caster) * STRIDE + 0x1DD, code);
        assert_eq!(cpu.call(WATER_HAZARD, &[]), Stop::Returned, "{round}");
        let retail = cpu.mem.rd8(CTX + 0x279) == 0xFF;

        let mut w = World::new();
        w.load_party(Party::zeroed(pc as usize));
        w.enter_battle(pc, m);
        let engine = |r: usize| if r < 3 { r } else { r - 3 + usize::from(pc) };
        for (r, &(hp, clip, colour)) in seats.iter().enumerate() {
            if !(r < usize::from(pc) || (3..3 + usize::from(m)).contains(&r)) {
                continue;
            }
            let a = &mut w.actors[engine(r)];
            a.active = true;
            a.battle.hp = hp;
            a.battle.current_anim = clip;
            a.battle.render_color = colour;
            a.battle_action_clips = Some(std::sync::Arc::new(Vec::new()));
            if r >= 3 {
                a.battle_monster_id = Some(0x20 + r as u16);
            }
        }
        // An actor past the seated row: no seat the walk counts.
        let past = usize::from(pc + m);
        let extra = rng.below(4) == 0 && past < w.actors.len();
        if extra {
            let a = &mut w.actors[past];
            a.active = true;
            a.battle_monster_id = None;
            a.battle.hp = 0;
            a.battle.render_color = 0x1004_0100;
            extras += 1;
        }
        let ce = engine(usize::from(caster));
        w.actors[ce].battle.active_target = match code {
            3..=7 => engine(usize::from(code)) as u8,
            c => c,
        };
        let port = w.capture_target_settled(ce as u8);
        assert_eq!(
            port, retail,
            "round {round}: party {pc} row {m} code {code} seats {seats:x?} extra {extra}"
        );
        passed += u32::from(retail);
        held += u32::from(!retail);
        codes.insert(code.min(8 + u8::from(code > 8)));
    }
    assert!(
        passed > 3000 && held > 3000 && extras > 2000 && codes.len() >= 4,
        "{passed} {held} {extras} {codes:?}"
    );
}

/// Every formation id each battle MAN on the disc can seat: the bundle MAN
/// and every type-3 streaming chunk, each formation's `count` ids.
fn formation_ids_disc_wide(disc: &legaia_disc_patch::disc::DiscPatcher) -> Vec<u8> {
    use legaia_scene_formats::{man_section, scene_asset_table};
    let ids_of = |man: &[u8], out: &mut Vec<u8>| {
        let Ok(file) = man_section::parse(man) else {
            return;
        };
        let Some(body) = file.encounter_section_body(man) else {
            return;
        };
        let Ok(sec) = man_section::parse_encounter_section(body) else {
            return;
        };
        let base = file.encounter_section().body_offset() + sec.formation_range.0;
        for f in 0..usize::from(sec.formation_count) {
            let rec = base + f * usize::from(sec.formation_stride);
            let Some(&count) = man.get(rec + 3) else {
                return;
            };
            let n = usize::from(count.min(4));
            out.extend(man.get(rec + 4..rec + 4 + n).unwrap_or(&[]));
        }
    };
    let mut out = Vec::new();
    for idx in 0..disc.entry_count() {
        let Ok(entry) = disc.read_entry(idx) else {
            continue;
        };
        if let Some(table) = scene_asset_table::detect(&entry)
            && let Some(man) = table.used().iter().find(|d| d.type_byte == 3)
            && man.size != 0
            && man.data_offset != 0
            && let Some(body) = entry.get(man.data_offset as usize..)
            && let Ok(decoded) = psx_lzs::decompress(body, man.size as usize)
        {
            ids_of(&decoded, &mut out);
        }
        if let Ok(report) = legaia_container_formats::parse_streaming(&entry, 4096) {
            for chunk in report.chunks.iter().filter(|c| c.type_byte == 3) {
                let start = chunk.header_offset + 4;
                if let Some(man) = entry.get(start..start + chunk.size as usize) {
                    ids_of(man, &mut out);
                }
            }
        }
    }
    out
}

/// Terror Scream's arm 0 (`0x801F7758..0x801F78F0`) and arm 1 fork on the
/// caster's monster id: `0x7D..=0x7F` play cue `0x190`, `0x83..=0x85` `0x191`,
/// `0x28..=0x2A` `0x197`, and only `0x7F`, `0x28..=0x2A` and `0x83..=0x85`
/// frame a shot; `0x7D` and `0x7E` play the cue and leave the camera where it
/// is, and any other id does neither. The camera director frames the near
/// shot for every id but `0x7F`, which is retail's only while the caster is
/// in a framed band. This holds that it always is.
///
/// Action `0x70` reaches a monster two ways, both in the AI picker
/// `FUN_801E9FD4` (PROT 0898): the record's magic window `+0x20..+0x27`
/// (`lbu v0,0x21(v0)` at `0x801EA538`, stored at `0x801EA544`), and the
/// monster-id switch, whose only `0x70` (`li v0,0x70` at `0x801EB3FC`, the one
/// immediate `0x70` stored to `+0x1DF` on the disc) closes the arm
/// `0x801EB2EC..0x801EB408` that the jump table `0x801CF1CC` (index id - 4,
/// bound `0xB3` at `0x801EA9D8`) enters for `0x28`, `0x29` (two `addiu s2`
/// later), `0x2A`, `0x7F` and `0x85`. Gold Face `0x7D` and Lava Face `0x7E`
/// sit in formations (Lava Face in random ones) but take the switch's common
/// tail, and no record's window holds `0x70`; Medusa `0x83` and Nurga `0x84`
/// do not cast it either.
#[test]
fn terror_scream_casters_all_sit_in_a_framed_band() {
    not_a_differential!(
        "censuses PROT 0898's AI jump table, the PROT 0867 records' magic window and every \
         formation on the disc against Terror Scream's framed caster bands; runs no routine"
    );
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const TABLE: u32 = 0x801C_F1CC;
    const TERROR_ARM: std::ops::RangeInclusive<u32> = 0x801E_B2EC..=0x801E_B2F4;
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let word = |va: u32| {
        let o = (va - OVERLAY_BASE) as usize;
        u32::from_le_bytes(overlay[o..o + 4].try_into().unwrap())
    };
    // The one immediate `0x70` the picker stores to `+0x1DF`.
    assert_eq!(word(0x801E_B3FC), 0x2402_0070, "li v0,0x70");
    assert_eq!(word(0x801E_B400), 0xA282_01DF, "sb v0,0x1df(s4)");
    let switch_casters: Vec<u8> = (0..0xB3u32)
        .filter(|i| TERROR_ARM.contains(&word(TABLE + 4 * i)))
        .map(|i| (i + 4) as u8)
        .collect();
    assert_eq!(switch_casters, [0x28, 0x29, 0x2A, 0x7F, 0x85]);
    let archive = disc.read_entry(867).expect("PROT 0867");
    let records = legaia_actor_formats::monster_archive::records(&archive).expect("records");
    let window_casters: Vec<u16> = records
        .iter()
        .filter(|r| r.ai_bytes.contains(&0x70))
        .map(|r| r.id)
        .collect();
    assert!(window_casters.is_empty(), "{window_casters:#x?}");
    let framed = |id: u8| matches!(id, 0x28..=0x2A | 0x7F | 0x83..=0x85);
    assert!(switch_casters.iter().all(|&id| framed(id)));
    // The two cue-only ids are seated, so it is the AI, not the formations,
    // that keeps them off the cast.
    let seated = formation_ids_disc_wide(disc);
    assert!(seated.len() > 500, "{} seated ids", seated.len());
    for id in [0x7Du8, 0x7E, 0x28, 0x7F, 0x85] {
        assert!(seated.contains(&id), "{id:#x} in no formation");
    }
}
