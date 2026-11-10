//! The op-`0x45` staging pair off the disc (PROT 0897): `FUN_801DDFE4` /
//! `FUN_801DE004`, which snapshot the live camera globals into the staging
//! block `0x801C6EA8`, and `FUN_801DE084`, which commits a block back - its
//! snap leg against `RetailCamGlobals::stage` / `commit_staged`, its glide
//! leg for the two cells it clears before handing the block to the mover
//! arm `FUN_801DD310`. Random globals and blocks, every slot value shape,
//! the ten globals compared whole.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_camera::camera_mover::AXIS_COUNT;
use legaia_engine_camera::retail_cam::{RetailCamGlobals, StagedShot};
use psx_retail_exec::{A0, A1, Memory, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const STAGE_WRAP: u32 = 0x801D_DFE4;
const STAGE: u32 = 0x801D_E004;
const COMMIT: u32 = 0x801D_E084;
const MOVER_ARM: u32 = 0x801D_D310;
const FIND_ACTOR: u32 = 0x8003_CF04;
const SET_H: u32 = 0x8003_D254;
const MOVER_TICK: u32 = 0x801D_C0BC;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTOR_LIST: u32 = 0x8007_C34C;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const GTE_H: u32 = 0x8007_B6F4;
const STAGING: u32 = 0x801C_6EA8;
const ACTORS: u32 = 0x8012_0000;

fn read_globals(m: &mut Memory) -> [i32; AXIS_COUNT] {
    let mut g = [0; AXIS_COUNT];
    for k in 0..3 {
        g[k] = i32::from(m.rd16(ANGLES + 2 * k as u32) as i16);
        g[3 + k] = m.rd32(EYE + 4 * k as u32) as i32;
        g[6 + k] = m.rd32(FOCUS + 4 * k as u32) as i32;
    }
    g[9] = i32::from(m.rd16(GTE_H) as i16);
    g
}

fn write_globals(m: &mut Memory, g: &[i32; AXIS_COUNT]) {
    for k in 0..3 {
        m.wr16(ANGLES + 2 * k as u32, g[k] as u16);
        m.wr32(EYE + 4 * k as u32, g[3 + k] as u32);
        m.wr32(FOCUS + 4 * k as u32, g[6 + k] as u32);
    }
    m.wr16(GTE_H, g[9] as u16);
}

fn read_staged(m: &mut Memory) -> StagedShot {
    std::array::from_fn(|k| m.rd16(STAGING + 2 + 4 * k as u32) as i16)
}

fn value(rng: &mut Rng) -> i32 {
    match rng.below(4) {
        0 => i32::from(rng.next() as i16),
        1 => rng.next() as i32,
        2 => (rng.below(0x2000) as i32) - 0x1000,
        _ => 0,
    }
}

#[test]
fn the_staging_snapshot_and_the_snap_commit_match_retail() {
    differential!(prot(897, STAGE_WRAP), prot(897, STAGE), prot(897, COMMIT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(ACTOR_LIST, 0x8011_0000);
    let movers: Rc<RefCell<Vec<u32>>> = Rc::default();
    {
        let movers = movers.clone();
        cpu.hook(FIND_ACTOR, move |r, _| {
            assert_eq!(r.r[A0], 0x8011_0000, "the scene list");
            assert_eq!(r.r[A1], MOVER_TICK, "the mover's tick");
            r.r[V0] = movers.borrow_mut().pop().unwrap_or(0);
        });
    }
    let h_loaded = Rc::new(Cell::new(None));
    {
        let h_loaded = h_loaded.clone();
        cpu.hook(SET_H, move |r, _| h_loaded.set(Some(r.r[A0] as i32)));
    }
    let armed = Rc::new(Cell::new(false));
    {
        let armed = armed.clone();
        cpu.hook(MOVER_ARM, move |r, _| {
            assert_eq!(r.r[A0], STAGING);
            armed.set(true);
        });
    }
    let mut rng = Rng(0x5354_4147);
    let mut slot7_cleared = 0;
    for run in 0..20_000 {
        let live: [i32; AXIS_COUNT] = std::array::from_fn(|_| value(&mut rng));
        write_globals(&mut cpu.mem, &live);
        let seen = RetailCamGlobals(read_globals(&mut cpu.mem));

        // FUN_801DDFE4 / FUN_801DE004: the snapshot.
        cpu.mem.load(STAGING, &[0xAA; 0x28]);
        let via = if run % 2 == 0 { STAGE_WRAP } else { STAGE };
        assert_eq!(cpu.call(via, &[STAGING]), Stop::Returned, "run {run}");
        let mut staged = seen.stage();
        assert_eq!(read_staged(&mut cpu.mem), staged, "run {run}: stage");

        // The Configure arm's masked writes, then the commit.
        for slot in staged.iter_mut() {
            if rng.below(2) == 0 {
                *slot = value(&mut rng) as i16;
            }
        }
        for (k, v) in staged.iter().enumerate() {
            cpu.mem.wr16(STAGING + 2 + 4 * k as u32, *v as u16);
        }
        let n_movers = rng.below(3);
        let mut ids: Vec<u32> = (0..n_movers).map(|i| ACTORS + 0x100 * i).collect();
        let flags: Vec<u32> = ids.iter().map(|_| rng.next()).collect();
        for (id, f) in ids.iter().zip(&flags) {
            cpu.mem.wr32(id + 0x10, *f);
        }
        ids.reverse();
        *movers.borrow_mut() = ids;
        h_loaded.set(None);
        armed.set(false);
        let glide = rng.below(4) == 0;
        let trigger = if glide { 1 + rng.below(0x1000) } else { 0 };
        let before = read_globals(&mut cpu.mem);
        assert_eq!(
            cpu.call(COMMIT, &[STAGING, trigger, rng.below(16)]),
            Stop::Returned,
            "run {run}"
        );
        let label = format!("run {run} trigger {trigger} staged {staged:?}");
        assert_eq!(cpu.mem.rd16(STAGING + 0x1E), 0, "{label}: block focus Y");
        if glide {
            assert!(armed.get(), "{label}: the mover arm");
            let mut want = before;
            want[7] = 0;
            assert_eq!(read_globals(&mut cpu.mem), want, "{label}: glide");
            assert_eq!(h_loaded.get(), None, "{label}");
            continue;
        }
        assert!(!armed.get(), "{label}");
        if staged[7] != 0 {
            slot7_cleared += 1;
        }
        let port = RetailCamGlobals::commit_staged(staged);
        assert_eq!(read_globals(&mut cpu.mem), port.0, "{label}: globals");
        assert_eq!(h_loaded.get(), Some(port.h()), "{label}: GTE H");
        for (i, f) in flags.iter().enumerate() {
            assert_eq!(
                cpu.mem.rd32(ACTORS + 0x100 * i as u32 + 0x10),
                f | 8,
                "{label}: mover {i} retired"
            );
        }
    }
    assert!(slot7_cleared > 1000, "{slot7_cleared}");
}
