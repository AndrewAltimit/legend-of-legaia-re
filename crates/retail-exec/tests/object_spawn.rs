//! The scene-init object sweep `FUN_8003A55C` over every field scene's real
//! `.MAP` and fallback window: which objects spawn an actor bound to a
//! script, where each is seated, its angles and the record it runs. Held
//! against `parse_map_objects` joined with the kind-1 lookup, the decode the
//! port's bind seats, walk-touch binds and object channels are built from -
//! and the port's own bind list and context set: one script context per
//! object retail spawns, two objects bound to one record included.

use crate::common::{Rng, disc, scus_cpu};
use crate::walk_on::{index, map_image};
use legaia_engine_core::world::World;
use legaia_engine_field::field_regions::{
    MAP_REGION_BLOCK_OFFSET, MAP_TRIGGER_FALLBACK_OFFSET, lookup_tile_trigger, parse_map_objects,
    parse_tile_triggers,
};
use legaia_engine_field::man_field_scripts::object_script_bind_objects;
use legaia_engine_scene::scene::Scene;
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SWEEP: u32 = 0x8003_A55C;
const SPAWN: u32 = 0x8002_4C88;
const FIELD_VM: u32 = 0x801D_E840;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const LADDER: u32 = 0x1F80_035C;
const MAP: u32 = 0x8010_0000;
const MAN_PTR: u32 = 0x8007_B898;
const MAN: u32 = 0x8016_0000;
const ACTORS: u32 = 0x8017_0000;
const ACTOR_STRIDE: u32 = 0x100;

/// One spawned actor: descriptor, its tile bytes `+0x8C` / `+0x8D`, seat
/// `(x, y, z)`, angles, record.
type Spawn = (u16, (u8, u8), (u16, u16, u16), [u16; 3], u16);

#[test]
fn the_object_sweep_binds_what_retail_binds() {
    differential!(scus(SWEEP));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let index = index();
    // The spawn: a fresh actor seated on the position block `a0` points at.
    let spawned: Rc<RefCell<Vec<u32>>> = Rc::default();
    let log = spawned.clone();
    cpu.hook(SPAWN, move |r, m| {
        let n = log.borrow().len() as u32;
        let actor = ACTORS + n * ACTOR_STRIDE;
        m.load(actor, &[0u8; ACTOR_STRIDE as usize]);
        for k in 0..3 {
            let v = m.rd16(r.r[A0] + k * 2);
            m.wr16(actor + 0x14 + k * 2, v);
        }
        log.borrow_mut().push(actor);
        r.r[V0] = actor;
    });
    // Every record is three zero bytes, so no prologue op runs.
    cpu.stub(FIELD_VM, 0);
    let mut man = vec![0u8; 0x2B + 0xFF * 3 + 0x10];
    man[0x22] = 0xFF;
    cpu.mem.load(MAN, &man);
    cpu.mem.wr32(MAN_PTR, MAN);
    let mut rng = Rng(0x4F42_4A53);
    let (mut scenes, mut binds, mut shared, mut failures) = (0, 0, 0, Vec::new());
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        if scene.field_map_index(&index).is_none() {
            continue;
        }
        let Some(image) = map_image(&index, &scene) else {
            continue;
        };
        scenes += 1;
        let ladder: [i16; 16] = std::array::from_fn(|_| rng.below(0x400) as i16 - 0x200);
        for (k, v) in ladder.iter().enumerate() {
            cpu.mem.wr16(LADDER + k as u32 * 2, *v as u16);
        }
        cpu.mem.load(MAP, &image);
        cpu.mem.wr32(MAP_PTR, MAP);
        spawned.borrow_mut().clear();
        cpu.step_budget = 50_000_000;
        assert_eq!(cpu.call(SWEEP, &[]), Stop::Returned, "{name}");
        let mut retail: Vec<Spawn> = Vec::new();
        for &a in spawned.borrow().iter() {
            let pos = (
                cpu.mem.rd16(a + 0x14),
                cpu.mem.rd16(a + 0x16),
                cpu.mem.rd16(a + 0x18),
            );
            retail.push((
                cpu.mem.rd16(a + 0x60),
                (
                    ((pos.0 as i16).wrapping_sub(0x40) >> 7) as u8,
                    ((pos.2 as i16).wrapping_sub(0x40) >> 7) as u8,
                ),
                pos,
                [
                    cpu.mem.rd16(a + 0x24),
                    cpu.mem.rd16(a + 0x26),
                    cpu.mem.rd16(a + 0x28),
                ],
                cpu.mem.rd16(a + 0x50),
            ));
        }
        // The tile byte pair `+0x8C` / `+0x8D` is the seat quantised again.
        for (&a, s) in spawned.borrow().iter().zip(&retail) {
            assert_eq!(
                (cpu.mem.rd8(a + 0x8C), cpu.mem.rd8(a + 0x8D)),
                s.1,
                "{name}"
            );
        }

        let k1p = parse_tile_triggers(&image[MAP_REGION_BLOCK_OFFSET..]);
        let k1f = parse_tile_triggers(&image[MAP_TRIGGER_FALLBACK_OFFSET..]);
        let port: Vec<Spawn> = parse_map_objects(&image[..MAP_TRIGGER_FALLBACK_OFFSET])
            .into_iter()
            .filter_map(|o| {
                let t = lookup_tile_trigger(&k1p, &k1f, o.key_tile.0, o.key_tile.1)?;
                // `+0x8C` / `+0x8D`: the seat quantised back to a tile, which
                // is not the placement tile once a descriptor offsets the seat.
                let q = |w: i16| (w.wrapping_sub(0x40) >> 7) as u8;
                Some((
                    o.descriptor,
                    (q(o.world.0), q(o.world.1)),
                    (o.world.0 as u16, o.seat_y(&ladder) as u16, o.world.1 as u16),
                    o.rot,
                    u16::from(t.record),
                ))
            })
            .collect();
        binds += retail.len();
        // The port's bind list and the contexts the world spawns from it:
        // one per object, in the sweep's order, with retail's seat, height,
        // angles and script id.
        let triggers: Vec<_> = k1p.iter().chain(&k1f).cloned().collect();
        let objects = object_script_bind_objects(&image, &triggers, &ladder);
        let port_binds: Vec<_> = objects
            .iter()
            .map(|b| (b.seat, b.seat_y as u16, b.rot, b.record as u16))
            .collect();
        let retail_binds: Vec<_> = retail
            .iter()
            .map(|s| ((s.2.0 as i16, s.2.2 as i16), s.2.1, s.3, s.4))
            .collect();
        if port_binds != retail_binds && failures.len() < 12 {
            failures.push(format!(
                "{name}: object_script_bind_objects differs from the sweep"
            ));
        }
        let distinct: std::collections::BTreeSet<u16> = retail.iter().map(|s| s.4).collect();
        shared += retail.len() - distinct.len();
        if let Ok(Some(man_bytes)) = scene.field_man_payload(&index)
            && let Ok(man_file) = legaia_scene_formats::man_section::parse(&man_bytes)
        {
            let mut world = World::default();
            world.seed_field_channels(&man_file, &man_bytes);
            world.seed_object_channels(&man_file, &man_bytes, &objects);
            let ids: Vec<u16> = world
                .field_vm
                .channels
                .iter()
                .filter(|c| c.object_bind)
                .map(|c| c.ctx.script_id)
                .collect();
            let want: Vec<u16> = retail.iter().map(|s| s.4).collect();
            if ids != want && failures.len() < 12 {
                failures.push(format!(
                    "{name}: object context ids {ids:?}, retail spawns {want:?}"
                ));
            }
        }
        if retail != port && failures.len() < 12 {
            let only_r: Vec<_> = retail
                .iter()
                .filter(|s| !port.contains(s))
                .take(4)
                .collect();
            let only_p: Vec<_> = port
                .iter()
                .filter(|s| !retail.contains(s))
                .take(4)
                .collect();
            failures.push(format!(
                "{name}: retail {} port {}; retail only {only_r:?}; port only {only_p:?}",
                retail.len(),
                port.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "{scenes} scenes, {binds} object binds, {shared} on a record an earlier object binds"
    );
    assert!(scenes > 100 && binds > 500, "{scenes} {binds}");
}
