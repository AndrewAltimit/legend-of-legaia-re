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
        prot(965, DOOMSDAY)
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
        for round in 0..6usize {
            let (caster, victim) = seed(&mut cpu, &mut rng, &module, body, 0);
            let (ca, va_) = (ACTOR + caster * STRIDE, ACTOR + victim * STRIDE);
            // The caster's formation id, which a body can fork on: Terror
            // Scream's owners (Terror Face `0x7F`, the `0x28` row).
            let monster = match body.action {
                0x70 => [0x28u8, 0x7F][round % 2],
                _ => 0,
            };
            for i in 0..5 {
                cpu.mem.wr8(FORMATION + i, monster);
            }
            // Curse All takes the party as a group.
            let group = (body.action == 0x53).then_some(8u8);
            if let Some(g) = group {
                cpu.mem.wr8(ca + 0x1DD, g);
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
            stream.set(1);
            let c0 = format!(
                "PROT {:04} body {:#010x} ({:#04x}) round {round}",
                body.entry, body.va, body.action
            );
            let mut finished = false;
            for pass in 0..3000 {
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
                if let Some(g) = group {
                    assert_eq!(
                        cpu.call(AIM, &[u32::from(g), OUT_CENTRE, OUT_EXTENT]),
                        Stop::Returned
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
                // Death Game's arms from 9 deal on the cards the host feeds
                // (`ModuleCamState::death_game`), which this run deals none of.
                if body.action == 0x76 && phase >= 9 {
                    finished = true;
                    break;
                }
                let arm = (body.director)(&mut st, phase, seats);
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
                    "{c0}: no finish in 3000 passes at arm {}",
                    cpu.mem.rd8(CTX + 0x279)
                ));
            }
            casts += 1;
        }
    }
    eprintln!("{} bodies, {casts} casts, {passes} passes", all.len());
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
