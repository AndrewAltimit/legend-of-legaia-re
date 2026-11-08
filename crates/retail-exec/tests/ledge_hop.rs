//! The ledge-hop probe `FUN_801D1878` against `World::try_field_ledge_hop`,
//! the floor sampler live under both: which forward steps start a hop, which
//! class, where it lands, over generated tier grids and walls. The actor sweep
//! `FUN_801CFC40` answers clear on retail's side; the arc setup `FUN_801D2404`
//! is hooked and its landing, apex and frame count compared.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::{Actor, World};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const HOP: u32 = 0x801D_1878;
const SWEEP: u32 = 0x801C_FC40;
const ARC: u32 = 0x801D_2404;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const LUT: u32 = 0x1F80_035C;
const MAP: u32 = 0x8010_0000;
const GRID: u32 = MAP + 0x4000;
const CELLS: u32 = MAP + 0x8000;
const ACTOR: u32 = 0x8013_0000;
const STEP_DELTA: u32 = 0x8007_BDE0;

#[test]
fn a_ledge_hops_where_retail_hops() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(MAP_PTR, MAP);
    cpu.stub(SWEEP, 0);
    type Arc = (i16, i16, i16, u32, u32);
    let arcs: Rc<RefCell<Vec<Arc>>> = Rc::default();
    {
        let arcs = arcs.clone();
        cpu.hook(ARC, move |r, m| {
            let a = r.r[A0];
            arcs.borrow_mut().push((
                m.rd16(a) as i16,
                m.rd16(a + 2) as i16,
                m.rd16(a + 4) as i16,
                r.r[A1],
                r.r[A2],
            ));
        });
    }
    let mut rng = Rng(0x484F_5053);
    let (mut hops, mut ups, mut failures) = (0, 0, Vec::new());
    for map_round in 0..40 {
        let tiers = 1 + rng.below(16);
        let grid: Vec<u8> = (0..0x4000)
            .map(|_| {
                let hi = if rng.below(10) == 0 {
                    rng.below(16) << 4
                } else {
                    0
                };
                (hi | rng.below(tiers)) as u8
            })
            .collect();
        let cells = vec![0u8; 0x8000];
        let mut w = World::new();
        w.actors = vec![Actor::default(); 1];
        w.actors[0].active = true;
        cpu.mem.load(GRID, &grid);
        cpu.mem.load(CELLS, &cells);
        w.load_field_collision_grid(&grid);
        w.load_field_object_cells(&cells);
        let lut: Vec<i16> = (0..16).map(|_| -(rng.below(0x400) as i16) * 2).collect();
        for (i, v) in lut.iter().enumerate() {
            cpu.mem.wr16(LUT + i as u32 * 2, *v as u16);
            w.terrain.floor_height_lut[i] = *v;
        }
        for round in 0..4000 {
            let (dx, dz): (i16, i16) = [
                (0, -2),
                (-2, 0),
                (0, 2),
                (2, 0),
                (2, 2),
                (-2, 2),
                (2, -2),
                (-2, -2),
                (0, 0),
                (0, 1),
                (-1, -1),
            ][rng.below(11) as usize];
            let (x, z) = (
                (0x400 + rng.below(0x3400) as i16) & !1,
                (0x400 + rng.below(0x3400) as i16) & !1,
            );
            let floor = w.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
            cpu.mem.wr16(ACTOR + 0x14, x as u16);
            cpu.mem.wr16(ACTOR + 0x16, floor as u16);
            cpu.mem.wr16(ACTOR + 0x18, z as u16);
            cpu.mem.wr32(ACTOR + 0x10, 0);
            cpu.mem.wr16(STEP_DELTA, dx as u16);
            cpu.mem.wr16(STEP_DELTA + 4, dz as u16);
            arcs.borrow_mut().clear();
            cpu.regs.r[V0] = 0;
            assert_eq!(
                cpu.call(HOP, &[ACTOR]),
                Stop::Returned,
                "map {map_round} round {round}"
            );
            let retail_hop = cpu.regs.r[V0] != 0;
            let retail_arc = arcs.borrow().first().copied();

            let ms = &mut w.actors[0].move_state;
            ms.world_x = x;
            ms.world_y = floor;
            ms.world_z = z;
            ms.flags = 0;
            w.locomotion.step_delta = (dx, dz);
            w.locomotion.ledge_hop = None;
            let port_hop = w.try_field_ledge_hop(0);
            let port_arc = w
                .locomotion
                .ledge_hop
                .as_ref()
                .map(|h| (h.target_x, h.target_y, h.target_z, u32::from(h.kind)));
            let retail_view = retail_arc.map(|(a, b, c, d, _)| (a, b, c, d));
            if retail_hop != port_hop || retail_view != port_arc {
                failures.push(format!(
                    "map {map_round} ({x}, {z}) delta ({dx}, {dz}) floor {floor}: \
                     retail {retail_hop} {retail_view:?}, port {port_hop} {port_arc:?}"
                ));
            }
            if let Some(a) = retail_arc {
                hops += 1;
                ups += u32::from(a.3 == 0x18);
                assert_eq!(a.4, 0x10, "frames");
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    assert!(hops > 2000 && ups > 500, "{hops} {ups}");
}
