//! The fishing lure tick `FUN_801D26CC` run whole from its entry, frame by
//! frame through its `+0x22` states `0 -> 1 -> 2`, with the rod actor
//! `FUN_801D1C5C` ahead of it in pool order, against `PondSession::tick` on one
//! pad script and one `rand()` stream.
//!
//! Live beneath them: the hooked fish `FUN_801D4004`, the cadence recogniser
//! `FUN_801D3DB4`, the record `FUN_801D765C` with the SCUS root, the polar
//! helper `FUN_801D7BB8`, the line clip `FUN_801D56E4` and the bearing
//! `FUN_80019B28`. Stubbed: the sound calls, the debug printf, the actor
//! spawns, the OT add, the bag consume, the rod's matrix and model calls. The
//! perspective transform `FUN_8003D368` is hooked to an on-screen point, so the
//! off-screen break latch stays clear: `PondSession::tick` leaves that latch to
//! the host's projection (`line_frame_retail`), which this rig does not run.
//!
//! What the harness stands in for, and says so: the cast lock of the session
//! machine `FUN_801CF3BC` (the lure, rod and power words are seeded from the
//! port's lock), the landing's reset of the band check and the cadence ring
//! (`FUN_801D746C`, state `0x19` of the machine), and the HUD readout
//! `DAT_801D9280`, written each frame from the record as the machine does.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::{
    CAST_POWER_STEP, FishingRecord, PondInput, PondPhase, PondSession,
};
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{A0, A1, Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const LURE_TICK: u32 = 0x801D_26CC;
const ROD_TICK: u32 = 0x801D_1C5C;

const LURE: u32 = 0x8014_0000;
const ROD_ACTOR: u32 = 0x8014_1000;
const ANGLER: u32 = 0x8014_2000;
const FISH: u32 = 0x8014_3000;
const RING_ACTOR: u32 = 0x8014_4000;
const MODEL: u32 = 0x8014_5000;
const SCENE: u32 = 0x8015_0000;
const SPAWN_TABLE: u32 = 0x8016_0000;
const PACKETS: u32 = 0x8017_0000;

const ALIVE: u32 = 0x801D_91A8;
const ACC_X: u32 = 0x801D_9174;
const TRACKED: u32 = 0x801D_9184;
const SPAWN: u32 = 0x801D_918C;
const POWER: u32 = 0x801D_9274;
const PULL: u32 = 0x801D_9148;
const SWAY: u32 = 0x801D_9140;
const BEND: u32 = 0x801D_9150;
const LIFT: u32 = 0x801D_914C;
const BOB: u32 = 0x801D_9134;
const SWING: u32 = 0x801D_91AC;
const YAW: u32 = 0x801D_911C;
const BEND_TRIM: u32 = 0x801D_90F4;
const HOOKED: u32 = 0x801D_91B4;
const BAND: u32 = 0x801D_90E8;
const COUNTDOWN: u32 = 0x801D_90EC;
const SPECIES: u32 = 0x801D_91CC;
const STRENGTH: u32 = 0x801D_91B8;
const FISH_SCALE: u32 = 0x801D_9108;
const TENSION: u32 = 0x801D_9168;
const DEPTH: u32 = 0x801D_9298;
const MODE: u32 = 0x801D_926C;
const RECORD: u32 = 0x801D_927C;
const READOUT: u32 = 0x801D_9280;
const ANGLER_PTR: u32 = 0x801D_9284;
const ROD_PTR: u32 = 0x801D_928C;
const SPAWN_PTR: u32 = 0x801D_9114;
const BGM_LATCH: u32 = 0x801D_92A4;
const REEL_EDGE: u32 = 0x801D_90D8;
const CUE_SLOT: u32 = 0x8007_B6DA;
const LURE_ROW: u32 = 0x8008_4450;
const ROD: u32 = 0x8008_4454;
const CASTS: u32 = 0x8008_4460;
const VENUE: u32 = 0x801D_90D0;
const HELD: u32 = 0x8007_B850;
const PRESSED: u32 = 0x8007_B874;
const FRAME_STEP: u32 = 0x1F80_0393;
const CURSOR: u32 = 0x1F80_03A0;

/// The fish's fight words `FUN_801D4004` reads and the hook does not seed.
const FISH_WORDS: [u32; 4] = [0x801D_910C, 0x801D_9110, 0x801D_9068, 0x801D_9290];
/// The cadence recogniser's ring, its index and last-button latch.
const CADENCE: [u32; 2] = [0x801D_91DC, 0x801D_9064];
const CADENCE_RING: u32 = 0x801D_91E4;

fn rig(rand: Rc<Cell<u32>>, cue: Rc<Cell<u32>>) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let overlay = disc()?
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    cpu.hook(0x8005_6798, move |r, _| {
        let next = rand.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        rand.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    // The real quadrature tables the boot installs (`FUN_80026BE0`).
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    // The perspective transform: an on-screen point.
    cpu.hook(0x8003_D368, |r, m| {
        m.wr16(r.r[A1], 160);
        m.wr16(r.r[A1] + 2, 120);
    });
    cpu.stub(0x8002_1B04, RING_ACTOR);
    cpu.stub(0x8002_4C88, FISH);
    // The creak's cue slot write is the sound ring's; the key-on beside it
    // is the cue that marks it.
    cpu.hook(0x8006_5034, move |_, _| cue.set(cue.get() + 1));
    for f in [
        0x8001_A068,
        0x8005_67A8,
        0x8001_E54C,
        0x8002_6478,
        0x8002_6520,
        0x8002_66E0,
        0x8003_D2C4,
        0x8004_2310,
        0x8005_FB84,
        0x8006_53C8,
        0x8001_80EC,
        0x801D_7030,
        // The rod actor's matrix, model and draw calls.
        0x8003_D1A4,
        0x8003_D344,
        0x8004_61A4,
        0x8004_629C,
        0x8004_638C,
        0x8005_B6A8,
        0x8001_C604,
        0x8003_D254,
        0x8004_3390,
        0x8001_72C0,
    ] {
        cpu.stub(f, 0);
    }
    cpu.mem.wr32(0x1F80_03EC, SCENE);
    cpu.mem.wr32(ANGLER_PTR, ANGLER);
    cpu.mem.wr32(ROD_PTR, ROD_ACTOR);
    cpu.mem.wr32(BGM_LATCH, 1);
    // The rod's model chain: `+0x44 -> [+4] -> [0]`, the tip at `+0x128`.
    cpu.mem.wr32(ROD_ACTOR + 0x44, MODEL);
    cpu.mem.wr32(MODEL + 4, MODEL + 0x100);
    cpu.mem.wr32(MODEL + 0x100, MODEL + 0x200);
    Some(cpu)
}

/// The phase a retail frame left, in the port's terms.
fn retail_phase(cpu: &mut Cpu) -> Option<PondPhase> {
    match cpu.mem.rd32(MODE) {
        0x28 => return Some(PondPhase::Snapped),
        0x1E => return Some(PondPhase::Landed),
        0x32 => return Some(PondPhase::Idle),
        _ => {}
    }
    match cpu.mem.rd16(LURE + 0x22) {
        0 => None,
        1 => Some(PondPhase::Flight),
        _ if cpu.mem.rd32(HOOKED) != 0 => Some(PondPhase::Hooked),
        _ => Some(PondPhase::Waiting),
    }
}

#[test]
fn a_cast_runs_through_the_lure_tick_as_retail_runs_it() {
    differential!(prot(972, LURE_TICK), prot(972, ROD_TICK));
    let rand = Rc::new(Cell::new(0u32));
    let cues = Rc::new(Cell::new(0u32));
    let Some(mut cpu) = rig(rand.clone(), cues.clone()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc()
        .unwrap()
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .unwrap();
    let species = fs::parse(&overlay).expect("species");
    let templates = fs::parse_cadence_templates(&overlay).expect("templates");
    // Distinct species ids, so the lookup shows.
    let n = species.len() as u32;
    let mut spawn = vec![[0u32; 8]; 3];
    for (l, row) in spawn.iter_mut().enumerate() {
        for (b, id) in row.iter_mut().enumerate() {
            *id = (l as u32 * 3 + b as u32) % n;
            cpu.mem
                .wr32(SPAWN_TABLE + (l as u32 * 8 + b as u32) * 4, *id);
        }
    }
    cpu.mem.wr32(SPAWN_PTR, SPAWN_TABLE);

    let mut rng = Rng(0x4C55_5245);
    let mut ends = [0u32; 4];
    let mut frames_in = [0u32; 3];
    let mut failures = Vec::new();
    for scenario in 0..1200 {
        let seed = rng.next();
        let (lure, rod, venue) = (rng.below(3), rng.below(3) as i32, rng.below(2));
        let casts = rng.below(120) as i32;
        let fs = 1 + rng.below(3) as i32;
        let facing = rng.below(0x1000) as i16;
        let anchor = (
            rng.below(0x2000) as i16,
            rng.below(0x100) as i16 - 0x40,
            rng.below(0x2000) as i16,
        );
        let mut port = PondSession::new(
            species.clone(),
            spawn.clone(),
            templates.clone(),
            venue as usize,
            lure,
            rod,
            casts,
            FishingRecord::default(),
            0,
            seed,
        );
        port.attach_venue(legaia_engine_fishing::fishing::PondVenue {
            map: Vec::new(),
            region_block: None,
            anchor_x: anchor.0,
            anchor_z: anchor.2,
            anchor_y: anchor.1,
            facing,
            rod_mesh: None,
        });
        let input = |held: u32, edge: u32, cast: bool| PondInput {
            reel_mask: held,
            cast_edge: cast,
            leave_edge: false,
            edge_bonus: legaia_engine_fishing::fishing_actors::bite_pad_nudge(edge),
            reel_edge: edge & 0xC0,
            face_edge: false,
        };
        // The shore: the cast press, the wind-up and a meter of random length.
        port.tick(input(0, 0x40, true), fs, CAST_POWER_STEP);
        // Most casts are locked strong enough to reach the water past
        // `0x136`; a few are not, which recast on their landing frame.
        let want_power = if rng.below(8) == 0 {
            0
        } else {
            0x600 + rng.below(0x900) as i32
        };
        for i in 0..400 {
            port.tick(input(0, 0, false), fs, CAST_POWER_STEP);
            if i >= 14 && port.phase() == PondPhase::Power && port.cast_power() >= want_power {
                break;
            }
        }
        port.tick(input(0, 0x40, true), fs, CAST_POWER_STEP);
        assert_eq!(port.phase(), PondPhase::Flight);
        let l = port.lure_actor().expect("lure");
        let r = *port.rod_actor().expect("rod");

        // The lock's seeding, off the port's lock.
        for a in (0x801D_9000..0x801D_9300).step_by(4) {
            cpu.mem.wr32(a, 0);
        }
        for a in (LURE..LURE + 0x6000).step_by(4) {
            cpu.mem.wr32(a, 0);
        }
        cpu.mem.wr32(ANGLER_PTR, ANGLER);
        cpu.mem.wr32(ROD_PTR, ROD_ACTOR);
        cpu.mem.wr32(BGM_LATCH, 1);
        cpu.mem.wr32(SPAWN_PTR, SPAWN_TABLE);
        cpu.mem.wr32(ROD_ACTOR + 0x44, MODEL);
        cpu.mem.wr32(MODEL + 4, MODEL + 0x100);
        cpu.mem.wr32(MODEL + 0x100, MODEL + 0x200);
        cpu.mem.wr32(ALIVE, 1);
        cpu.mem.wr16(ANGLER + 0x26, facing as u16);
        cpu.mem.wr16(ROD_ACTOR + 0x26, facing as u16);
        cpu.mem.wr32(ACC_X, l.x_fixed as u32);
        cpu.mem.wr32(ACC_X + 4, l.y_fixed as u32);
        cpu.mem.wr32(ACC_X + 8, l.z_fixed as u32);
        for (i, v) in [l.tracked.0, l.tracked.1, l.tracked.2]
            .into_iter()
            .enumerate()
        {
            cpu.mem.wr16(TRACKED + 2 * i as u32, v as u16);
        }
        cpu.mem.wr16(SPAWN, l.spawn.0 as u16);
        cpu.mem.wr16(SPAWN + 4, l.spawn.1 as u16);
        cpu.mem.wr32(POWER, l.power as u32);
        cpu.mem.wr32(RECORD, port.line_record() as u32);
        cpu.mem.wr32(SWING, 1);
        cpu.mem.wr16(BOB, r.bob as u16);
        // The rod's yaw, which its tick copies to `+0x26` every frame
        // (`0x801D1DC0..0x801D1DC8`) and the sway arm reads back as the facing.
        cpu.mem.wr16(BOB + 2, facing as u16);
        cpu.mem.wr32(BEND, r.bend as u32);
        cpu.mem.wr32(BEND_TRIM, 0);
        cpu.mem.wr32(LURE_ROW, lure);
        cpu.mem.wr32(ROD, rod as u32);
        cpu.mem.wr32(CASTS, casts as u32);
        cpu.mem.wr32(VENUE, venue);
        cpu.mem.wr32(BAND, 3);
        cpu.mem.wr8(FRAME_STEP, fs as u8);
        cpu.mem.wr32(0x8007_B9B0, 0);
        rand.set(port.rng_state());
        // The lock frame's lure tick: state `0`.
        cpu.regs.r[A0] = LURE;
        assert_eq!(cpu.call(LURE_TICK, &[LURE]), Stop::Returned);

        let mut held_for = [0i32; 2];
        let mut held = 0u32;
        let mut ended = false;
        for frame in 0..3000 {
            // A held reel button is let go before the session's auto-repeat
            // (32 vsyncs) could fire, which the machine folds into the edge.
            // A waiting line is mostly left alone, so a fish bites; a fight
            // is reeled hard, so it ends one way or the other.
            let busy = match port.phase() {
                PondPhase::Hooked => 8,
                _ => 2,
            };
            let mut next = match rng.below(10) {
                r if r >= busy => 0,
                0 => held,
                r => [0x40u32, 0x80, 0xC0][r as usize % 3],
            };
            if rng.below(6) == 0 {
                next |= [0x2000u32, 0x4000, 0x8000][rng.below(3) as usize];
            }
            for (i, bit) in [0x40u32, 0x80].into_iter().enumerate() {
                held_for[i] = if next & bit != 0 { held_for[i] + fs } else { 0 };
                if held_for[i] >= 24 {
                    next &= !bit;
                    held_for[i] = 0;
                }
            }
            let edge = next & !held;
            held = next;
            let creaks_before = cues.get();
            port.tick(input(held, edge, false), fs, CAST_POWER_STEP);
            let creaks = port.take_rod_creaks();
            port.take_spent_lures();
            port.take_events();

            cpu.mem.wr32(HELD, held);
            cpu.mem.wr32(PRESSED, edge);
            cpu.mem.wr32(REEL_EDGE, edge & 0xC0);
            cpu.mem.wr8(FRAME_STEP, fs as u8);
            let rec = cpu.mem.rd32(RECORD) as i32;
            cpu.mem.wr32(READOUT, (rec - 300).max(0) as u32);
            cpu.mem.wr32(CURSOR, PACKETS);
            cpu.mem.wr16(CUE_SLOT, 0);
            let state_before = cpu.mem.rd16(LURE + 0x22);
            if cpu.mem.rd32(SWING) != 0x14 {
                assert_eq!(cpu.call(ROD_TICK, &[ROD_ACTOR]), Stop::Returned);
            }
            assert_eq!(
                cpu.call(LURE_TICK, &[LURE]),
                Stop::Returned,
                "{:x?}",
                cpu.calls
            );
            // The landing hands over to the machine's state `0x19`, whose
            // entry resets the band check and the cadence ring.
            if state_before == 1 && cpu.mem.rd16(LURE + 0x22) == 2 {
                cpu.mem.wr32(BAND, port.band());
                cpu.mem.wr32(COUNTDOWN, port.band_countdown() as u32);
                for a in CADENCE {
                    cpu.mem.wr32(a, 0);
                }
                for i in 0..32 {
                    cpu.mem.wr32(CADENCE_RING + i * 4, 0);
                }
                for a in FISH_WORDS {
                    cpu.mem.wr32(a, 0);
                }
                cpu.mem.wr32(TENSION, 0);
            }
            let phase = retail_phase(&mut cpu);
            let at = format!(
                "scenario {scenario} frame {frame} fs {fs} lure {lure} rod {rod} held {held:#x} edge {edge:#x}"
            );
            let mut want = |what: &str, retail: i64, ported: i64| {
                if retail != ported {
                    failures.push(format!("{at}: {what} retail {retail:#x} port {ported:#x}"));
                }
            };
            want("phase", phase.map_or(-1, |p| p as i64), port.phase() as i64);
            want("draws", i64::from(rand.get()), i64::from(port.rng_state()));
            // A recast zeroes the port's record for the shore's readout;
            // retail leaves the word to the next cast's seed.
            if port.phase() != PondPhase::Idle {
                want(
                    "record",
                    i64::from(cpu.mem.rd32(RECORD) as i32),
                    i64::from(port.line_record()),
                );
            }
            want(
                "casts",
                i64::from(cpu.mem.rd32(CASTS)),
                i64::from(port.casts),
            );
            // The creak is cue `0x201` in the ring's slot 1 (`0x801D2BD0`);
            // the key-on hook counts the reel sound (`0x801D2EB8`) beside it.
            let _ = cues.get() - creaks_before;
            want(
                "creak",
                i64::from(cpu.mem.rd16(CUE_SLOT) == 0x201),
                i64::from(creaks),
            );
            if let Some(l) = port.lure_actor() {
                for (i, v) in [l.x_fixed, l.y_fixed, l.z_fixed].into_iter().enumerate() {
                    want(
                        ["x", "y", "z"][i],
                        i64::from(cpu.mem.rd32(ACC_X + 4 * i as u32) as i32),
                        i64::from(v),
                    );
                }
                want(
                    "power",
                    i64::from(cpu.mem.rd32(POWER) as i32),
                    i64::from(l.power),
                );
                want(
                    "pull",
                    i64::from(cpu.mem.rd32(PULL) as i32),
                    i64::from(l.pull),
                );
                want(
                    "fall",
                    i64::from(cpu.mem.rd16(LURE + 0x3E) as i16),
                    i64::from(l.fall),
                );
                want(
                    "sway",
                    i64::from(cpu.mem.rd32(SWAY) as i32),
                    i64::from(l.sway.0),
                );
                want(
                    "sway target",
                    i64::from(cpu.mem.rd32(SWAY + 4) as i32),
                    i64::from(l.sway.1),
                );
            }
            if let Some(r) = port.rod_actor() {
                want(
                    "bend",
                    i64::from(cpu.mem.rd32(BEND) as i32),
                    i64::from(r.bend),
                );
                want(
                    "lift",
                    i64::from(cpu.mem.rd32(LIFT) as i32),
                    i64::from(r.lift),
                );
                want("bob", i64::from(cpu.mem.rd16(BOB) as i16), i64::from(r.bob));
                want("yaw", i64::from(cpu.mem.rd32(YAW) as i32), i64::from(r.yaw));
                want(
                    "lean",
                    i64::from(cpu.mem.rd32(SWAY) as i32),
                    i64::from(r.lean),
                );
            }
            match port.phase() {
                PondPhase::Waiting => {
                    frames_in[0] += 1;
                    want(
                        "band",
                        i64::from(cpu.mem.rd32(BAND)),
                        i64::from(port.band()),
                    );
                    want(
                        "countdown",
                        i64::from(cpu.mem.rd32(COUNTDOWN) as i32),
                        i64::from(port.band_countdown()),
                    );
                }
                PondPhase::Hooked => {
                    frames_in[1] += 1;
                    want(
                        "band",
                        i64::from(cpu.mem.rd32(BAND)),
                        i64::from(port.band()),
                    );
                    want(
                        "species",
                        i64::from(cpu.mem.rd32(SPECIES)),
                        port.hooked().map_or(-1, |s| s.index as i64),
                    );
                    want(
                        "strength",
                        i64::from(cpu.mem.rd32(STRENGTH) as i32),
                        i64::from(port.strength()),
                    );
                    want(
                        "scale",
                        i64::from(cpu.mem.rd32(FISH_SCALE) as u16),
                        i64::from(port.fish_scale()),
                    );
                    want(
                        "tension",
                        i64::from(cpu.mem.rd32(TENSION) as i32),
                        i64::from(port.tension()),
                    );
                    want(
                        "depth",
                        i64::from(cpu.mem.rd32(DEPTH) as i32),
                        i64::from(port.depth()),
                    );
                }
                PondPhase::Flight => frames_in[2] += 1,
                _ => {}
            }
            if failures.len() > 40 {
                break;
            }
            let p = port.phase();
            if matches!(p, PondPhase::Landed | PondPhase::Snapped | PondPhase::Idle) {
                ends[match p {
                    PondPhase::Landed => 0,
                    PondPhase::Snapped => 1,
                    _ => 2,
                }] += 1;
                ended = true;
                break;
            }
        }
        if !ended {
            ends[3] += 1;
        }
        if !failures.is_empty() {
            break;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "ends landed/snapped/recast/open {ends:?}, frames waiting/hooked/flight {frames_in:?}"
    );
    // A strike wants a held reel against a credit that mostly comes from a
    // recognised gesture (`0x801D31F4`), which a random pad seldom makes:
    // fights are rare, and each still runs hundreds of frames.
    assert!(
        ends[0] >= 10 && ends[1] >= 3 && ends[2] >= 100 && frames_in[1] > 2000,
        "{ends:?} {frames_in:?}"
    );
}
