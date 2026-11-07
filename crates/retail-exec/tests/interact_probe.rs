//! The field interact probe `FUN_801CF9F4` (PROT 0897) as the touch dispatch
//! calls it (`jal` at `0x801D0854`, extents `0x20`): crowds of actors around
//! the probe point, both axes at and either side of the box edge, coordinates
//! near the `i16` ends.
//!
//! It pins the rule rather than a port kernel: the box is the port's
//! (`FIELD_INTERACT_BOX_HALF`, strict on both sides, the probe point wrapping
//! as a halfword), and on overlapping boxes the **last** hit in actor-list
//! order wins, where `World::field_interact_probe_slot` takes the nearest.
//! The port keeps the nearest until a capture shows the list order a scene
//! spawns: in `koin1` the probe from door `P1[56]` also boxes `P1[63]`.

use crate::common::{Rng, disc, scus_cpu};
use psx_retail_exec::{Stop, V0};

#[derive(Debug, Clone, Copy)]
struct ProbeActor {
    id: u8,
    x: i16,
    z: i16,
    half_x: i32,
    half_z: i32,
}

/// The walk's rule: every hit overwrites the caller's `+0x98` link
/// (`sw t3,0x98(t6)` at `0x801CFB40`), so the last one in list order stands.
fn interact_probe_hit(
    px: i16,
    pz: i16,
    actors: impl IntoIterator<Item = ProbeActor>,
) -> Option<u8> {
    let (px, pz) = (i32::from(px), i32::from(pz));
    let mut hit = None;
    for a in actors {
        let (cx, cz) = (i32::from(a.x), i32::from(a.z));
        if px < cx + a.half_x && cx - a.half_x < px && pz < cz + a.half_z && cz - a.half_z < pz {
            hit = Some(a.id);
        }
    }
    hit
}

const PROBE: u32 = 0x801C_F9F4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8014_0000;
const PLAYER: u32 = 0x8013_0000;
const HEAD: u32 = 0x8013_0800;
const ACTORS: u32 = 0x8013_1000;
const STRIDE: u32 = 0x100;
/// `sp` at entry: the fifth and sixth arguments sit at `+0x10` / `+0x14`.
const STACK: u32 = 0x801F_FF00;
const HALF: i32 = 0x40 + 0x20 - 0x18;

#[test]
fn the_interact_probe_talks_to_the_actor_retail_does() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(MAP_PTR, MAP);
    let mut rng = Rng(0x5441_4C4B);
    let (mut overlapped, mut hits, mut misses) = (0, 0, 0);
    for round in 0..40_000 {
        // A cleared map: the trailing tile test reads it and finds no wall.
        for i in 0..0x80 {
            cpu.mem.wr32(MAP + 0x4000 + i * 4, 0);
        }
        let base: i32 = [0, 4000, -4000, 32700, -32700][rng.below(5) as usize];
        let px = (base + rng.below(200) as i32 - 100) as i16;
        let pz = (base + rng.below(200) as i32 - 100) as i16;
        let (dx, dz) = [(0i16, 64i16), (-64, 64), (-64, 0), (64, -64)][rng.below(4) as usize];
        cpu.mem.wr16(PLAYER + 0x14, px as u16);
        cpu.mem.wr16(PLAYER + 0x18, pz as u16);
        cpu.mem.wr32(PLAYER + 0x98, 0);
        let (qx, qz) = (px.wrapping_add(dx), pz.wrapping_sub(dz));
        let n = rng.below(6);
        let mut list = Vec::new();
        cpu.mem.wr32(HEAD, if n == 0 { 0 } else { ACTORS });
        for k in 0..n {
            let a = ACTORS + k * STRIDE;
            let near = |rng: &mut Rng, q: i16| {
                let off = match rng.below(3) {
                    0 => [HALF - 1, HALF, HALF + 1, -HALF + 1, -HALF, -HALF - 1]
                        [rng.below(6) as usize],
                    _ => rng.below(2 * HALF as u32 + 40) as i32 - HALF - 20,
                };
                (i32::from(q) + off) as i16
            };
            let (x, z) = (near(&mut rng, qx), near(&mut rng, qz));
            cpu.mem.wr32(a, if k + 1 == n { 0 } else { a + STRIDE });
            cpu.mem.wr32(a + 0x10, 0x0002_0000);
            cpu.mem.wr16(a + 0x14, x as u16);
            cpu.mem.wr16(a + 0x18, z as u16);
            list.push(ProbeActor {
                id: k as u8,
                x,
                z,
                half_x: HALF,
                half_z: HALF,
            });
        }
        cpu.mem.wr32(STACK + 0x10, 0x20);
        cpu.mem.wr32(STACK + 0x14, 0x20);
        assert_eq!(
            cpu.call(PROBE, &[PLAYER, HEAD, dx as u16 as u32, dz as u16 as u32]),
            Stop::Returned,
            "round {round}"
        );
        let bits = cpu.regs.r[V0];
        let link = cpu.mem.rd32(PLAYER + 0x98);
        let retail = (link != 0).then(|| ((link - ACTORS) / STRIDE) as u8);
        let port = interact_probe_hit(qx, qz, list.iter().copied());
        assert_eq!(retail, port, "round {round}: probe ({qx}, {qz}) {list:?}");
        assert_eq!(bits & 1 != 0, retail.is_some());
        let inside = list
            .iter()
            .filter(|a| interact_probe_hit(qx, qz, [**a]).is_some())
            .count();
        overlapped += i32::from(inside > 1);
        if retail.is_some() {
            hits += 1;
        } else {
            misses += 1;
        }
    }
    assert!(
        overlapped > 1000 && hits > 5000 && misses > 5000,
        "{overlapped} {hits} {misses}"
    );
}
