//! PROT 0920's (Slippery) Deadly Rain routine `FUN_801F7B88` off the disc,
//! run frame after frame against `legaia_render_kernels::cast_rain::DeadlyRain`
//! over random battle cameras, facings, budgets, frame steps and colours.
//! `rand()` is recorded on the retail side and replayed to the port; the
//! GTE holds the port's `RainCamera`; the splash spawn `FUN_80021B04` is
//! hooked. Compared each frame: all `0x400` records, the splashes spawned
//! (record and `+0x78` depth), and every streak drawn - its `LINE_G2`
//! corners and head colour, read off the packet pool, and the ordering-table
//! entry its depth picks.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_render_kernels::cast_rain::{DeadlyRain, RAIN_DROPS, RainCamera};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SLOT_B: u32 = 0x801F_69D8;
const RAIN: u32 = 0x801F_7B88;
const RAND: u32 = 0x8005_6798;
const SPAWN: u32 = 0x8002_1B04;
const DROPS_PTR: u32 = 0x801F_8448;
const COLOUR: u32 = 0x801F_8444;
const CREATURE_PTR: u32 = 0x801C_938C;
const BUDGET: u32 = 0x8007_BDC0;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_SHIFT: u32 = 0x1F80_03A4;
const OT_PTR: u32 = 0x1F80_03F4;
const DROPS: u32 = 0x8010_0000;
const CREATURE: u32 = 0x8011_0000;
const PART: u32 = 0x8011_1000;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8016_0000;
const STACK_TOP: u32 = 0x801F_FF00;

fn drop_bytes(r: &DeadlyRain) -> Vec<u8> {
    r.drops
        .iter()
        .flat_map(|d| [d.x, d.y, d.z, d.len])
        .flat_map(|h| h.to_le_bytes())
        .collect()
}

#[test]
fn the_deadly_rain_matches_the_module() {
    differential!(prot(920, RAIN));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        SLOT_B,
        &disc().expect("disc").read_entry(920).expect("PROT 0920"),
    );
    let draws: Rc<RefCell<(Rng, Vec<u32>)>> = Rc::new(RefCell::new((Rng(0x5241_494E), Vec::new())));
    let d = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let mut g = d.borrow_mut();
        let v = g.0.next() & 0x7FFF;
        g.1.push(v);
        r.r[V0] = v;
    });
    let spawned: Rc<RefCell<Vec<u32>>> = Rc::default();
    let s = spawned.clone();
    cpu.hook(SPAWN, move |r, m| {
        s.borrow_mut().push(r.r[A0]);
        m.wr16(PART + 0x78, 0xEEEE);
        r.r[V0] = PART;
    });
    // The trig table pointers the boot code sets: sine, cosine.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    cpu.mem.wr32(DROPS_PTR, DROPS);
    cpu.mem.wr32(CREATURE_PTR, CREATURE);
    cpu.mem.wr32(POOL_PTR, POOL);
    cpu.mem.wr32(OT_PTR, OT);
    let mut rng = Rng(0x534C_4950);
    let (mut lines_seen, mut splashes_seen) = (0usize, 0usize);
    for scene in 0..12 {
        let cam = RainCamera::battle(
            rng.below(0x200) as i16 - 0x100,
            rng.below(0x1000) as i16,
            [
                rng.below(0x200) as i32 - 0x100,
                0x300 + rng.below(0x300) as i32,
                0x800 + rng.below(0xC00) as i32,
            ],
            [rng.below(0x200) as i32 - 0x100, 0, rng.below(0x400) as i32],
        );
        let e: Vec<i16> = cam.rot.m.iter().flatten().copied().collect();
        let p = |a: i16, b: i16| (a as u16 as u32) | (b as u16 as u32) << 16;
        let words = [
            p(e[0], e[1]),
            p(e[2], e[3]),
            p(e[4], e[5]),
            p(e[6], e[7]),
            e[8] as u16 as u32,
            cam.tr.x as u32,
            cam.tr.y as u32,
            cam.tr.z as u32,
        ];
        for (k, w) in words.iter().enumerate() {
            cpu.gte.write_ctrl(k, *w);
        }
        cpu.gte.write_ctrl(24, (cam.ofx << 16) as u32);
        cpu.gte.write_ctrl(25, (cam.ofy << 16) as u32);
        cpu.gte.write_ctrl(26, u32::from(cam.h));
        let mut port = DeadlyRain::default();
        cpu.mem.load(DROPS, &vec![0; RAIN_DROPS * 8]);
        let facing = rng.below(0x1000) as u16;
        cpu.mem.wr16(CREATURE + 0x46, facing);
        let shift = rng.below(3) as u8;
        cpu.mem.wr8(OT_SHIFT, shift);
        for frame in 0..60 {
            let budget = [0x204, 4, 1, 0x40][rng.below(4) as usize];
            let rate = 1 + rng.below(3) as u8;
            let colour = rng.next() & 0x00FF_FFFF;
            cpu.mem.wr32(BUDGET, budget);
            cpu.mem.wr8(0x1F80_0393, rate);
            cpu.mem.wr32(COLOUR, colour);
            cpu.mem.wr32(POOL_PTR, POOL);
            cpu.mem.load(OT, &[0xFF; 0x4000]);
            // The splash test's two stale halfwords: off screen.
            cpu.mem.wr16(STACK_TOP - 0x60 + 0x24, 0xFFFF);
            cpu.mem.wr16(STACK_TOP - 0x60 + 0x26, 0xFFFF);
            draws.borrow_mut().1.clear();
            spawned.borrow_mut().clear();
            assert_eq!(
                cpu.call(RAIN, &[]),
                Stop::Returned,
                "scene {scene} frame {frame}"
            );

            let recorded = std::mem::take(&mut draws.borrow_mut().1);
            let mut it = recorded.iter().copied();
            let out = port.step(budget as i32, rate, facing, colour, &cam, || {
                it.next().expect("the port drew more rand() than retail")
            });
            assert_eq!(
                it.next(),
                None,
                "scene {scene} frame {frame}: retail drew more"
            );
            let at = format!("scene {scene} frame {frame}");
            let retail = cpu.mem.read(DROPS, RAIN_DROPS * 8);
            let want = drop_bytes(&port);
            assert!(
                retail == want,
                "{at}: records {:?}",
                diff_offsets(&retail, &want)
            );

            // Splashes: how many, and the last one's depth.
            assert_eq!(spawned.borrow().len(), out.splashes.len(), "{at}: splashes");
            if let Some(last) = out.splashes.last() {
                assert_eq!(
                    cpu.mem.rd16(PART + 0x78) as i16,
                    last.depth_half,
                    "{at}: splash depth"
                );
            }
            splashes_seen += out.splashes.len();

            // Streaks: walk the pool; a line followed by a draw-mode packet
            // was linked.
            let end = cpu.mem.rd32(POOL_PTR);
            let mut p = POOL;
            let mut drawn = Vec::new();
            let mut last_mode = 0u32;
            while p < end {
                assert_eq!(cpu.mem.rd8(p + 7), 0x53, "{at}: a LINE_G2 at {p:#x}");
                let linked = p + 0x14 < end && cpu.mem.rd8(p + 0x14 + 7) != 0x53;
                if linked {
                    let head = (cpu.mem.rd16(p + 8) as i16, cpu.mem.rd16(p + 10) as i16);
                    let tail = (cpu.mem.rd16(p + 0x10) as i16, cpu.mem.rd16(p + 0x12) as i16);
                    let rgb = cpu.mem.read(p + 4, 3);
                    assert_eq!(
                        cpu.mem.read(p + 0xC, 3),
                        vec![0, 0, 0],
                        "{at}: the black tail"
                    );
                    drawn.push((head, tail, [rgb[0], rgb[1], rgb[2]]));
                    last_mode = p + 0x14;
                    p += 0x14 + 0xC;
                } else {
                    p += 0x14;
                }
            }
            let want: Vec<_> = out.lines.iter().map(|l| (l.head, l.tail, l.rgb)).collect();
            assert_eq!(drawn, want, "{at}: streaks");
            if let Some(l) = out.lines.last() {
                let e = (l.depth / 2) >> shift;
                // The entry's newest link is the last line's draw mode.
                let word = cpu.mem.rd32(OT + 4 * e as u32);
                assert_eq!(
                    word & 0x00FF_FFFF,
                    last_mode & 0x00FF_FFFF,
                    "{at}: OT entry {e:#x}"
                );
            }
            lines_seen += out.lines.len();
        }
    }
    assert!(
        lines_seen > 2000 && splashes_seen > 100,
        "lines {lines_seen} splashes {splashes_seen}"
    );
}
