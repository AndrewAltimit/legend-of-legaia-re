//! The two travel-art handlers of the field overlay (PROT 0897), Riremito
//! `FUN_801EE094` and Rula `FUN_801EE328`, against
//! `legaia_engine_field::travel_art_actor::TravelArtActor::tick`.
//!
//! The scan runs over a generated define table with retail's own number
//! reader `FUN_8003CE9C`; the effect gate, the name install, the fade spawn
//! and the diagnostic print are hooked and logged, the fade with the template
//! it was handed.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::travel_art_actor::{
    FLASH_FADE_END, FLASH_FADE_FRAMES, FLASH_FADE_HOLD, FLASH_FADE_ID, FLASH_FADE_KIND,
    PHASE_UNFOUND, RIREMITO_RESTORE_CLEARED_FLAG, RIREMITO_RESTORE_SCALE, RULA_LIFT_PLAYER_FLAG,
    RULA_LIFT_SCRATCH_FLAG, TravelArt, TravelArtActor, destination_for, find_visited_map,
};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const RIREMITO: u32 = 0x801E_E094;
const RULA: u32 = 0x801E_E328;

const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_TEST: u32 = 0x8003_CE64;
const OPENER: u32 = 0x801D_5A24;
const POST_PRIM: u32 = 0x8002_4E80;
const DEFINE_TABLE: u32 = 0x8001_9788;
const INSTALL_NAME: u32 = 0x8001_FD44;
const PRINT: u32 = 0x8001_A068;

const FRAME_STEP: u32 = 0x1F80_0393;
const PAD_LATCH: u32 = 0x1F80_0394;
const PLAYER_PTR: u32 = 0x8007_C364;
const WARP_HOLD: u32 = 0x8007_B6B4;
const DEFINE_COUNT: u32 = 0x8007_B806;
const DEST: u32 = 0x8007_3EF4;
const TILE_X: u32 = 0x8008_4624;
const MAP: u32 = 0x8008_4628;
const TILE_Z: u32 = 0x8008_462C;

const ACTOR: u32 = 0x8012_0000;
const PLAYER: u32 = 0x8012_3000;
const TABLE: u32 = 0x8012_4000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    FlagSet(u32),
    Opener(u32),
    /// The fade template's thirteen halfwords less the three the handler
    /// leaves unwritten (`+4`, `+0xC`, `+0x18`), and the id in `a1`.
    Fade([i16; 10], u32),
    InstallName(u32, u32),
    Print,
}

fn run(entry: u32, art: TravelArt, seed: u64) {
    let disc = disc().expect("checked by the caller");
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log = Rc::new(RefCell::new(Vec::<Call>::new()));
    let busy = Rc::new(Cell::new(0u32));
    let l = log.clone();
    cpu.hook(FLAG_SET, move |r, _| {
        l.borrow_mut().push(Call::FlagSet(r.r[A0]))
    });
    let l = log.clone();
    cpu.hook(OPENER, move |r, _| l.borrow_mut().push(Call::Opener(r.r[A0])));
    let b = busy.clone();
    cpu.hook(FLAG_TEST, move |r, _| {
        assert_eq!(r.r[A0], 0xB, "the gate reads system flag 0xB");
        r.r[V0] = b.get();
    });
    let l = log.clone();
    cpu.hook(POST_PRIM, move |r, m| {
        let at = r.r[A0];
        let t = [0, 2, 6, 8, 0xA, 0xE, 0x10, 0x12, 0x14, 0x16].map(|o| m.rd16(at + o) as i16);
        l.borrow_mut().push(Call::Fade(t, r.r[A1]));
    });
    cpu.hook(DEFINE_TABLE, |r, _| r.r[V0] = TABLE);
    let l = log.clone();
    cpu.hook(INSTALL_NAME, move |r, _| {
        l.borrow_mut().push(Call::InstallName(r.r[A0], r.r[A1]))
    });
    let l = log.clone();
    cpu.hook(PRINT, move |_, _| l.borrow_mut().push(Call::Print));
    cpu.mem.wr32(PLAYER_PTR, PLAYER);

    let mut rng = Rng(seed);
    let (mut queued, mut gated, mut flashed, mut lifted, mut hit, mut miss, mut parked) =
        (0, 0, 0, 0, 0, 0, 0);
    for round in 0..40000 {
        let phase: u16 = match rng.below(12) {
            0 => 4,
            1 => PHASE_UNFOUND,
            2 => 5 + rng.below(3) as u16,
            n => (n % 4) as u16,
        };
        let dwell = match rng.below(4) {
            0 => 0x50 - rng.below(6) as i16,
            1 => 0x28 - rng.below(6) as i16,
            2 => -(rng.below(4) as i16),
            _ => rng.below(0x60) as i16,
        };
        let delta = 1 + rng.below(4) as i16;
        let player_y = match rng.below(3) {
            0 => -0x618 + rng.below(0x40) as i16 - 0x10,
            1 => -0x618 + rng.below(0x200) as i16,
            _ => rng.next() as i16,
        };
        let player_flags = rng.next();
        let scale = rng.next() as u16;
        let latch = rng.next();
        let is_busy = rng.below(4) == 0;
        // A define table of up to eight records, numbers anywhere an `s16`
        // goes, and a map word that names one of them, or none, or one whose
        // high half is set (which no sign-extended number can equal unless
        // it is negative).
        let count = rng.below(9) as usize;
        let numbers: Vec<i16> = (0..count)
            .map(|_| match rng.below(3) {
                0 => rng.below(8) as i16,
                1 => -(rng.below(8) as i16),
                _ => rng.next() as i16,
            })
            .collect();
        let map = match rng.below(4) {
            0 => rng.next(),
            1 => rng.below(8),
            _ if count > 0 => {
                let n = numbers[rng.below(count as u32) as usize];
                if rng.below(6) == 0 {
                    u32::from(n as u16)
                } else {
                    n as i32 as u32
                }
            }
            _ => 3,
        };
        let (tile_x, tile_z) = (rng.below(0x200) as i32 - 0x40, rng.below(0x200) as i32 - 0x40);
        let dest_before = [rng.next(), rng.next(), rng.next()];

        cpu.mem.wr16(ACTOR + 0x54, phase);
        cpu.mem.wr16(ACTOR + 0x9E, dwell as u16);
        cpu.mem.wr8(FRAME_STEP, delta as u8);
        cpu.mem.wr32(PAD_LATCH, latch);
        cpu.mem.wr16(PLAYER + 0x16, player_y as u16);
        cpu.mem.wr32(PLAYER + 0x10, player_flags);
        cpu.mem.wr16(PLAYER + 0x72, scale);
        cpu.mem.wr32(WARP_HOLD, 0x1234);
        cpu.mem.wr16(DEFINE_COUNT, count as u16);
        for (i, n) in numbers.iter().enumerate() {
            cpu.mem.wr16(TABLE + 0x10 * i as u32 + 0xC, *n as u16);
        }
        cpu.mem.wr32(TILE_X, tile_x as u32);
        cpu.mem.wr32(MAP, map);
        cpu.mem.wr32(TILE_Z, tile_z as u32);
        for (i, w) in dest_before.iter().enumerate() {
            cpu.mem.wr32(DEST + 4 * i as u32, *w);
        }
        busy.set(u32::from(is_busy));
        log.borrow_mut().clear();
        assert_eq!(cpu.call(entry, &[ACTOR]), Stop::Returned, "round {round}");

        let mut actor = TravelArtActor {
            art,
            phase,
            dwell,
            player_y,
        };
        let frame = actor.tick(is_busy, delta, || {
            find_visited_map(count, map, |i| numbers[i] as i32 as u32)
                .map(|i| destination_for(i, tile_x, tile_z))
        });
        let mut calls = Vec::new();
        let (mut y, mut flags, mut want_scale, mut want_latch, mut hold, mut dest) =
            (player_y, player_flags, scale, latch, 0x1234u32, dest_before);
        if let Some(arg) = frame.queue_effect {
            calls.push(Call::FlagSet(0xB));
            calls.push(Call::Opener(arg));
            queued += 1;
        }
        if let Some(lifted_y) = frame.lift_player_y {
            y = lifted_y;
            flags |= RULA_LIFT_PLAYER_FLAG;
            want_latch |= RULA_LIFT_SCRATCH_FLAG;
            lifted += 1;
        }
        if frame.clear_warp_hold {
            hold = 0;
        }
        if frame.spawn_flash {
            let e = FLASH_FADE_END;
            calls.push(Call::Fade(
                [
                    FLASH_FADE_KIND,
                    FLASH_FADE_FRAMES,
                    0,
                    0,
                    0,
                    e,
                    e,
                    e,
                    0,
                    FLASH_FADE_HOLD,
                ],
                FLASH_FADE_ID as u32,
            ));
            flashed += 1;
        }
        if let Some(d) = frame.destination {
            if frame.restore_player {
                want_scale = RIREMITO_RESTORE_SCALE as u16;
                flags &= !RIREMITO_RESTORE_CLEARED_FLAG;
            }
            calls.push(Call::InstallName(
                TABLE + 0x10 * d.record_index as u32,
                map as i16 as i32 as u32,
            ));
            dest = [d.x as u32, d.z as u32, d.y as u32];
            hit += 1;
        }
        if phase == PHASE_UNFOUND {
            // The diagnostic phase prints two lines and does nothing else.
            calls.push(Call::Print);
            calls.push(Call::Print);
            parked += 1;
        }
        miss += u32::from(frame.unfound);
        gated += u32::from(phase == 1 && is_busy);

        let ctx = format!(
            "round {round} {art:?} phase {phase} dwell {dwell} delta {delta} y {player_y} \
             busy {is_busy} count {count} map {map:#x} numbers {numbers:?}"
        );
        assert_eq!(*log.borrow(), calls, "calls: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54), actor.phase, "phase: {ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x9E) as i16, actor.dwell, "dwell: {ctx}");
        assert_eq!(cpu.mem.rd16(PLAYER + 0x16) as i16, y, "player y: {ctx}");
        assert_eq!(cpu.mem.rd32(PLAYER + 0x10), flags, "player flags: {ctx}");
        assert_eq!(cpu.mem.rd16(PLAYER + 0x72), want_scale, "scale: {ctx}");
        assert_eq!(cpu.mem.rd32(PAD_LATCH), want_latch, "pad latch: {ctx}");
        assert_eq!(cpu.mem.rd32(WARP_HOLD), hold, "warp hold: {ctx}");
        for (i, w) in dest.iter().enumerate() {
            assert_eq!(cpu.mem.rd32(DEST + 4 * i as u32), *w, "dest {i}: {ctx}");
        }
    }
    let lift_floor = if art == TravelArt::Rula { 5000 } else { 0 };
    assert!(
        queued > 5000
            && gated > 1000
            && flashed > 500
            && lifted >= lift_floor
            && hit > 1500
            && miss > 1500
            && parked > 2000,
        "{queued} {gated} {flashed} {lifted} {hit} {miss} {parked}"
    );
}

#[test]
fn riremito_matches_retail() {
    differential!(prot(897, RIREMITO));
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    run(RIREMITO, TravelArt::Riremito, 0x5249_5245);
}

#[test]
fn rula_matches_retail() {
    differential!(prot(897, RULA));
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    run(RULA, TravelArt::Rula, 0x5255_4C41);
}
