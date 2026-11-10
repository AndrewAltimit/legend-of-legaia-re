//! The camera-zone arrival actor's tick `FUN_801DBE9C` (PROT 0897). Its
//! `_DAT_8007B868 != 0` leg is what `camera_zone_arrival_tick` ports: the
//! `+0x54` countdown, the `(pos - 0x40) >> 7` tile handed to the zone query,
//! the record load or the nine default stores, then the follow update and
//! the `+0x10 |= 8` retire. The `== 0` leg retail boots into is held to what
//! the port's docs say of it: an unengaged player is re-pinned and snapped at
//! once, an engaged one (`+0x10 & 0x80000`) counts the countdown and retires
//! at `-1` without a snap.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::camera::{
    CAMERA_ZONE_DEFAULTS, CameraZoneArrival, camera_zone_arrival_tick,
};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const TICK: u32 = 0x801D_BE9C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const DEV_GATE: u32 = 0x8007_B868;
const PLAYER_PTR: u32 = 0x8007_C364;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const ZONE_QUERY: u32 = 0x801D_BA20;
const LOAD: u32 = 0x801D_BC20;
const FLOOR: u32 = 0x8001_9278;
const SNAP: u32 = 0x801D_B8EC;
const ACTOR: u32 = 0x8013_0000;
const PLAYER: u32 = 0x8013_1000;
const RECORD: u32 = 0x8014_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Query(i32, i32),
    Load,
    Floor,
    Snap,
}

#[test]
fn the_arrival_tick_matches_retail() {
    differential!(prot(897, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let calls: Rc<RefCell<Vec<Call>>> = Rc::default();
    let hit = Rc::new(std::cell::Cell::new(false));
    {
        let (calls, hit) = (calls.clone(), hit.clone());
        cpu.hook(ZONE_QUERY, move |r, _| {
            calls
                .borrow_mut()
                .push(Call::Query(r.r[A0] as i32, r.r[A1] as i32));
            r.r[V0] = if hit.get() { RECORD } else { 0 };
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(LOAD, move |r, _| {
            assert_eq!(r.r[A0], RECORD);
            calls.borrow_mut().push(Call::Load);
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(FLOOR, move |r, _| {
            calls.borrow_mut().push(Call::Floor);
            r.r[V0] = 0x1234;
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(SNAP, move |r, _| {
            assert_eq!(r.r[A0], PLAYER);
            calls.borrow_mut().push(Call::Snap);
        });
    }
    let mut rng = Rng(0x4152_5256);
    let mut arms = [0u32; 3];
    let mut retail_leg = [0u32; 3];
    for run in 0..20_000 {
        let dev = rng.below(2) == 0;
        cpu.mem.wr32(DEV_GATE, if dev { rng.next() | 1 } else { 0 });
        let countdown = [0i16, 1, 2, -1, -2, rng.next() as i16][rng.below(6) as usize];
        let actor_flags = rng.next();
        cpu.mem.wr16(ACTOR + 0x54, countdown as u16);
        cpu.mem.wr32(ACTOR + 0x10, actor_flags);
        let (px, pz) = (rng.next() as i16, rng.next() as i16);
        let mut player_flags = rng.next();
        if rng.below(2) == 0 {
            player_flags &= !0x8_0000;
        }
        if rng.below(2) == 0 {
            player_flags &= !0x2000_0000;
        }
        let py = rng.next() as i16;
        cpu.mem.wr16(PLAYER + 0x14, px as u16);
        cpu.mem.wr16(PLAYER + 0x16, py as u16);
        cpu.mem.wr16(PLAYER + 0x18, pz as u16);
        cpu.mem.wr32(PLAYER + 0x10, player_flags);
        let (fx, fz) = (rng.next(), rng.next());
        cpu.mem.wr32(FOCUS_X, fx);
        cpu.mem.wr32(FOCUS_Z, fz);
        let block: Vec<u8> = (0..0x15).map(|_| rng.next() as u8).collect();
        cpu.mem.load(0x8007_B607, &block);
        hit.set(rng.below(2) == 0);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(TICK, &[ACTOR]), Stop::Returned, "run {run}");
        let label = format!("run {run} dev {dev} countdown {countdown} flags {player_flags:#x}");

        let engaged = player_flags & 0x8_0000 != 0;
        let mut expect = Vec::new();
        let mut cd = countdown;
        let retired;
        let mut defaults = false;
        if dev {
            let mut queried = None;
            let arm = camera_zone_arrival_tick(&mut cd, (px, pz), |tx, tz| {
                queried = Some((tx, tz));
                hit.get()
            });
            if let Some((tx, tz)) = queried {
                expect.push(Call::Query(tx, tz));
            }
            arms[match arm {
                CameraZoneArrival::Waiting => 0,
                CameraZoneArrival::LoadZoneConfig => 1,
                CameraZoneArrival::LoadDefaults => 2,
            }] += 1;
            match arm {
                CameraZoneArrival::Waiting => retired = false,
                CameraZoneArrival::LoadZoneConfig => {
                    expect.push(Call::Load);
                    retired = true;
                }
                CameraZoneArrival::LoadDefaults => {
                    defaults = true;
                    retired = true;
                }
            }
            if retired && !engaged {
                if player_flags & 0x2000_0000 == 0 {
                    expect.push(Call::Floor);
                }
                expect.push(Call::Snap);
            }
        } else if engaged {
            cd = cd.wrapping_sub(1);
            retired = cd == -1;
            retail_leg[usize::from(retired)] += 1;
        } else {
            if player_flags & 0x2000_0000 == 0 {
                expect.push(Call::Floor);
            }
            expect.push(Call::Snap);
            retired = true;
            retail_leg[2] += 1;
        }
        assert_eq!(*calls.borrow(), expect, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54) as i16, cd, "{label}");
        assert_eq!(
            cpu.mem.rd32(ACTOR + 0x10),
            actor_flags | if retired { 8 } else { 0 },
            "{label}"
        );
        let snapped = expect.contains(&Call::Snap);
        let focus = (cpu.mem.rd32(FOCUS_X), cpu.mem.rd32(FOCUS_Z));
        let want = if snapped {
            ((-i32::from(px)) as u32, (-i32::from(pz)) as u32)
        } else {
            (fx, fz)
        };
        assert_eq!(focus, want, "{label}: focus");
        let floor_y = if expect.contains(&Call::Floor) {
            0x1234
        } else {
            py as u16
        };
        assert_eq!(cpu.mem.rd16(PLAYER + 0x16), floor_y, "{label}: player Y");
        let mut want_block = block.clone();
        if defaults {
            for (va, v) in CAMERA_ZONE_DEFAULTS {
                let o = (va - 0x8007_B607) as usize;
                let w = if va < 0x8007_B60C { 1 } else { 4 };
                want_block[o..o + w].copy_from_slice(&v.to_le_bytes()[..w]);
            }
        }
        assert_eq!(
            cpu.mem.read(0x8007_B607, 0x15),
            want_block,
            "{label}: block"
        );
    }
    assert!(
        arms.iter().all(|&n| n > 500) && retail_leg.iter().all(|&n| n > 300),
        "{arms:?} {retail_leg:?}"
    );
}
