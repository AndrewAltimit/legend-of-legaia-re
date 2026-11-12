//! Baka Fighter's stage-model placement `FUN_801D6D60` (PROT 0976): the
//! camera the field build `FUN_800172C0` loads from the duel's globals, a
//! position and a yaw, against `place_stage_model`. The draw `FUN_80043390`
//! is hooked: whether it is reached is the cull verdict, and the GTE matrix
//! it is handed (`R * Ry(yaw)` with `TR` moved to the position's eye point)
//! takes model vertices to the eye space the port's placed vertices project
//! to.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigame_scenes::baka_duel_scene::{WALL_PLACEMENTS, place_stage_model};
use legaia_engine_minigames::baka_duel::{DUEL_WORLD_SCALE, DuelCamera};
use psx_retail_exec::Stop;
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PLACE: u32 = 0x801D_6D60;
const BUILD: u32 = 0x8001_72C0;
const DRAW: u32 = 0x8004_3390;
/// The `jal` to the draw: the GTE as it stands is what the draw is handed.
const DRAW_CALL: u32 = 0x801D_6DE0;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const BASE: u32 = 0x8007_BF10;
const POS: u32 = 0x8010_0000;
const ROT: u32 = 0x8010_0010;

/// The GTE rotation and translation the draw was handed.
type Handed = ([[i32; 3]; 3], [i32; 3]);

#[test]
fn the_stage_model_lands_where_retail_draws_it() {
    differential!(prot(976, PLACE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    for r in 0..3u32 {
        for c in 0..3u32 {
            let v = if r == c { 0x6000 } else { 0 };
            cpu.mem.wr16(BASE + 2 * (3 * r + c), v);
        }
    }
    let handed = Rc::new(RefCell::new(None::<Handed>));
    {
        let handed = handed.clone();
        cpu.probe(DRAW_CALL, move |_, _, gte| {
            let rd = |r: usize| gte.read_ctrl(r);
            let lo = |w: u32| i32::from(w as i16);
            let hi = |w: u32| i32::from((w >> 16) as i16);
            let m = [
                [lo(rd(0)), hi(rd(0)), lo(rd(1))],
                [hi(rd(1)), lo(rd(2)), hi(rd(2))],
                [lo(rd(3)), hi(rd(3)), lo(rd(4))],
            ];
            let tr = [rd(5) as i32, rd(6) as i32, rd(7) as i32];
            *handed.borrow_mut() = Some((m, tr));
        });
    }
    cpu.stub(DRAW, 0);
    let mut rng = Rng(0x5354_4147);
    let (mut drew, mut culled, mut points) = (0usize, 0usize, 0usize);
    for round in 0..3000 {
        // The duel's own cameras, and the spin's frames between them.
        let mut cam = match round % 4 {
            0 => DuelCamera::settled(),
            1 => DuelCamera::default(),
            2 => {
                let mut c = DuelCamera::default();
                for _ in 0..rng.below(40) {
                    c.spin(1 + rng.below(3) as i32);
                }
                c
            }
            _ => {
                let mut c = DuelCamera::settled();
                c.follow_push(rng.below(0x400) as i32 - 0x200);
                c
            }
        };
        cam.tick(0);
        let g = cam.globals().0;
        for k in 0..3u32 {
            cpu.mem.wr16(ANGLES + 2 * k, g[k as usize] as u16);
            cpu.mem.wr32(EYE + 4 * k, g[3 + k as usize] as u32);
            cpu.mem.wr32(FOCUS + 4 * k, g[6 + k as usize] as u32);
        }
        assert_eq!(cpu.call(BUILD, &[]), Stop::Returned);
        let (pos, yaw) = if round % 3 == 0 {
            WALL_PLACEMENTS[rng.below(4) as usize]
        } else {
            (
                std::array::from_fn(|_| rng.below(0x1000) as i16 - 0x800),
                rng.below(0x1000) as i16,
            )
        };
        for k in 0..3u32 {
            cpu.mem.wr16(POS + 2 * k, pos[k as usize] as u16);
            cpu.mem
                .wr16(ROT + 2 * k, if k == 1 { yaw as u16 } else { 0 });
        }
        *handed.borrow_mut() = None;
        assert_eq!(
            cpu.call(PLACE, &[POS, ROT]),
            Stop::Returned,
            "round {round}"
        );
        let got = *handed.borrow();
        let verts: Vec<[f32; 3]> = (0..6)
            .map(|_| std::array::from_fn(|_| rng.below(0x800) as f32 - 0x400 as f32))
            .collect();
        let mut out = vec![[0f32; 3]; verts.len()];
        let port = place_stage_model(&cam, pos, yaw, &verts, &mut out);
        let origin = pos.map(f32::from);
        let depth = cam.eye_depth(origin);
        let ctx = format!("round {round} pos {pos:?} yaw {yaw:#x} depth {depth}");
        // The cull verdict, away from the threshold the port's reals can
        // straddle.
        if (depth - 10001.0).abs() > 4.0 {
            assert_eq!(got.is_some(), port, "drawn, {ctx}");
        }
        let Some((m, tr)) = got else {
            culled += 1;
            continue;
        };
        if !port {
            continue;
        }
        drew += 1;
        let view = cam.view();
        for (v, o) in verts.iter().zip(&out) {
            let retail: [f64; 3] = std::array::from_fn(|r| {
                let s: i64 = (0..3).map(|c| i64::from(m[r][c]) * v[c] as i64).sum();
                (s >> 12) as f64 + f64::from(tr[r])
            });
            let e = view.eye_space(*o).map(|x| f64::from(x * DUEL_WORLD_SCALE));
            for k in 0..3 {
                // The port's reals against the SCUS tables' rounding, over
                // eye depths of tens of thousands of units.
                let tol = 4.0 + retail[k].abs() / 1500.0;
                assert!(
                    (e[k] - retail[k]).abs() <= tol,
                    "axis {k} of {v:?}: retail {:.1} port {:.1}, {ctx}",
                    retail[k],
                    e[k]
                );
            }
            points += 1;
        }
    }
    assert!(
        drew > 500 && culled > 100 && points > 3000,
        "drew {drew} culled {culled} points {points}"
    );
}
