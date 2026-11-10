//! The sub-area window rebuild `FUN_801D7B50` (PROT 0897) over every field
//! scene's real `.MAP`, at random region boxes with random ownership marks
//! (`0x400`) stamped on the object grid, against
//! `window_rebuild_spawns_resident`: which placements spawn, in what order,
//! the seat each is handed, its angles, its template kind and the three
//! flag bits the arm sets after the spawn, and the spawn count
//! `0x8007B924`.

use crate::common::{Rng, disc, scus_cpu};
use crate::walk_on::{index, map_image};
use legaia_engine_field::field_regions::window_rebuild_spawns_resident;
use legaia_engine_scene::scene::Scene;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const REBUILD: u32 = 0x801D_7B50;
const SPAWN: u32 = 0x8002_4C88;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const BOX: u32 = 0x1F80_0384;
const LADDER: u32 = 0x1F80_035C;
const MAP: u32 = 0x8010_0000;
const LIST_PTR: u32 = 0x8007_C36C;
const LIST: u32 = 0x8015_0000;
const GATE_A: u32 = 0x8007_B868;
const GATE_B: u32 = 0x8007_B8B8;
const COUNT: u32 = 0x8007_B924;
const ACTORS: u32 = 0x8017_0000;
const ACTOR_STRIDE: u32 = 0x100;

/// One spawn: descriptor, seat, angles, template kind, `+0x74` bits
/// `0x40000000` / `0x10000000`, `+0x10` bit `4`.
type Spawn = (u16, (i16, i16, i16), (u16, u16, u16), u16, bool, bool, bool);

#[test]
fn the_window_rebuild_spawns_what_retail_spawns() {
    differential!(prot(897, REBUILD));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let index = index();
    // The spawn: a fresh actor seated on the vector `a0` points at, and the
    // template kind `a1 + 0x14` as the arm left it.
    let spawned: Rc<RefCell<Vec<(u32, u16)>>> = Rc::default();
    let log = spawned.clone();
    cpu.hook(SPAWN, move |r, m| {
        let n = log.borrow().len() as u32;
        let actor = ACTORS + n * ACTOR_STRIDE;
        m.load(actor, &[0u8; ACTOR_STRIDE as usize]);
        for k in 0..3 {
            let v = m.rd16(r.r[A0] + k * 2);
            m.wr16(actor + 0x14 + k * 2, v);
        }
        log.borrow_mut().push((actor, m.rd16(r.r[A1] + 0x14)));
        r.r[V0] = actor;
    });
    cpu.mem.wr32(LIST_PTR, LIST);
    cpu.mem.wr32(LIST, 0);
    cpu.mem.wr32(GATE_A, 0);
    cpu.mem.wr32(GATE_B, 0);
    cpu.step_budget = 50_000_000;
    let mut rng = Rng(0x5749_4E44);
    let (mut scenes, mut spawns, mut gated, mut failures) = (0, 0, 0, Vec::new());
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        if scene.field_map_index(&index).is_none() {
            continue;
        }
        let Some(mut image) = map_image(&index, &scene) else {
            continue;
        };
        scenes += 1;
        // Ownership marks on one cell in eight.
        for c in 0..0x4000 {
            if rng.below(8) == 0 {
                let o = 0x8000 + c * 2;
                image[o + 1] ^= 0x04;
            }
        }
        cpu.mem.load(MAP, &image);
        cpu.mem.wr32(MAP_PTR, MAP);
        let ladder: [i16; 16] = std::array::from_fn(|_| rng.below(0x400) as i16 - 0x200);
        for (k, v) in ladder.iter().enumerate() {
            cpu.mem.wr16(LADDER + k as u32 * 2, *v as u16);
        }
        let cells: Vec<u16> = image[0x8000..0x10000]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        for pass in 0..6 {
            let window = if pass == 0 {
                (0u8, 0u8, 0x80u8, 0x80u8)
            } else {
                let (x0, z0) = (rng.below(0x80) as u8, rng.below(0x80) as u8);
                (
                    x0,
                    z0,
                    x0.saturating_add(rng.below(0x30) as u8).min(0x80),
                    z0.saturating_add(rng.below(0x30) as u8).min(0x80),
                )
            };
            cpu.mem.load(BOX, &[window.0, window.1, window.2, window.3]);
            cpu.mem.wr32(COUNT, 0);
            spawned.borrow_mut().clear();
            assert_eq!(cpu.call(REBUILD, &[]), Stop::Returned, "{name}");
            let retail: Vec<Spawn> = spawned
                .borrow()
                .iter()
                .map(|&(a, kind)| {
                    (
                        cpu.mem.rd16(a + 0x60),
                        (
                            cpu.mem.rd16(a + 0x14) as i16,
                            cpu.mem.rd16(a + 0x16) as i16,
                            cpu.mem.rd16(a + 0x18) as i16,
                        ),
                        (
                            cpu.mem.rd16(a + 0x24),
                            cpu.mem.rd16(a + 0x26),
                            cpu.mem.rd16(a + 0x28),
                        ),
                        kind,
                        cpu.mem.rd32(a + 0x74) & 0x4000_0000 != 0,
                        cpu.mem.rd32(a + 0x74) & 0x1000_0000 != 0,
                        cpu.mem.rd32(a + 0x10) & 4 != 0,
                    )
                })
                .collect();
            assert_eq!(cpu.mem.rd32(COUNT) as usize, retail.len(), "{name}");
            let port: Vec<Spawn> = window_rebuild_spawns_resident(
                &image[..0x4000],
                &image[0x4000..0x8000],
                &cells,
                window,
                ladder,
            )
            .into_iter()
            .map(|s| {
                (
                    s.descriptor,
                    s.world,
                    s.rotation,
                    s.template_kind,
                    s.flag_40000000,
                    s.flag_10000000,
                    s.flag_state_4,
                )
            })
            .collect();
            spawns += retail.len();
            gated += u32::from(retail.iter().any(|s| s.3 == 5));
            if retail != port && failures.len() < 12 {
                failures.push(format!(
                    "{name} {window:?}: retail {} port {}; first retail {:?} port {:?}",
                    retail.len(),
                    port.len(),
                    retail.first(),
                    port.first()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{scenes} scenes, {spawns} window spawns");
    assert!(scenes > 100 && spawns > 1000, "{scenes} {spawns} {gated}");
}
