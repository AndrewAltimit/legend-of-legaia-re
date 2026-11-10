//! The field collision broad phase `FUN_801CF754` (PROT 0897) against
//! `build_contact_candidates`: which actors of the list land in the probe's
//! candidate table and in what order, over actors seated directly and through
//! tile descriptors, mirrored or not, on and around the window's edges, with
//! lists long enough to fill the table.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::contact_candidates::{
    CULL_WINDOW, CullActor, CullCentre, DIRECT_POS_MASK, MIRROR_FLAG, SKIP_STATE_MASK,
    TileDescriptor, VISIBLE_ACTOR_CAP, build_contact_candidates,
};
use psx_retail_exec::Stop;

const BROAD_PHASE: u32 = 0x801C_F754;
const OVERLAY_BASE: u32 = 0x801C_E818;
const TABLE: u32 = 0x801C_93C8;
const COUNT: u32 = 0x8007_B6B8;
const DESC_PTR: u32 = 0x1F80_03EC;
const CENTRE: u32 = 0x1F80_0020;
const PLAYER: u32 = 0x8010_0000;
const HEAD: u32 = 0x8010_0100;
const ACTORS: u32 = 0x8011_0000;
const DESCS: u32 = 0x8012_0000;
const ACTOR_LEN: u32 = 0x80;

#[test]
fn the_broad_phase_keeps_the_candidates_the_port_keeps() {
    differential!(prot(897, BROAD_PHASE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let mut rng = Rng(0x434F_4E54);
    let (mut full, mut kept, mut dropped, mut empty) = (0, 0, 0, 0);
    for round in 0..3_000 {
        let centre = CullCentre {
            x: rng.below(0x4000) as i16 - 0x1000,
            y: rng.below(0x4000) as i16 - 0x1000,
        };
        // Tile descriptors, `0x20` bytes each.
        let descs: Vec<TileDescriptor> = (0..16)
            .map(|_| TileDescriptor {
                mirror_dx: rng.below(0x100) as i16 - 0x80,
                mirror_dy: rng.below(0x100) as i16 - 0x80,
                coarse_x: (i32::from(centre.x) >> 7).clamp(-128, 127) as i8
                    + (rng.below(7) as i8 - 3),
                coarse_y: (i32::from(centre.y) >> 7).clamp(-128, 127) as i8
                    + (rng.below(7) as i8 - 3),
                fine_x: rng.next() as i8,
                fine_y: rng.next() as i8,
            })
            .collect();
        for (k, t) in descs.iter().enumerate() {
            let mut rec: Vec<u8> = (0..0x20).map(|_| rng.next() as u8).collect();
            rec[0..2].copy_from_slice(&t.mirror_dx.to_le_bytes());
            rec[4..6].copy_from_slice(&t.mirror_dy.to_le_bytes());
            rec[6] = t.coarse_x as u8;
            rec[7] = t.coarse_y as u8;
            rec[0xE] = t.fine_x as u8;
            rec[0xF] = t.fine_y as u8;
            cpu.mem.load(DESCS + 0x20 * k as u32, &rec);
        }
        cpu.mem.wr32(DESC_PTR, DESCS);
        let n = match rng.below(8) {
            0 => 0,
            1 => 40 + rng.below(30),
            _ => rng.below(24),
        };
        let mut actors = Vec::new();
        for k in 0..n {
            // A long list crowds the player, so the table fills.
            let crowd = n >= 40 && rng.below(8) != 0;
            let direct = crowd || rng.below(2) == 0;
            let state_flags = (rng.next() & !(SKIP_STATE_MASK | DIRECT_POS_MASK))
                | if !crowd && rng.below(6) == 0 { 1 + rng.below(3) } else { 0 }
                | if direct {
                    [0x0100_0000, 0x0002_0000, DIRECT_POS_MASK][rng.below(3) as usize]
                } else {
                    0
                };
            // Around the window's edge on one axis, anywhere on the other.
            let near = |rng: &mut Rng, c: i16| -> i16 {
                let edge = if crowd {
                    0
                } else {
                    [-CULL_WINDOW, CULL_WINDOW, 0][rng.below(3) as usize]
                };
                (i32::from(c) + edge + rng.below(9) as i32 - 4) as i16
            };
            let (local_x, local_y) = if direct {
                (near(&mut rng, centre.x), near(&mut rng, centre.y))
            } else {
                (rng.below(0x300) as i16 - 0x180, rng.below(0x300) as i16 - 0x180)
            };
            let a = CullActor {
                state_flags,
                local_x,
                local_y,
                sprite_flags: (rng.next() as u16 & !MIRROR_FLAG)
                    | if rng.below(2) == 0 { MIRROR_FLAG } else { 0 },
                tile_slot: rng.below(16) as u16,
            };
            let at = ACTORS + ACTOR_LEN * k;
            let mut rec: Vec<u8> = (0..ACTOR_LEN).map(|_| rng.next() as u8).collect();
            let next = if k + 1 < n { at + ACTOR_LEN } else { 0 };
            rec[0..4].copy_from_slice(&next.to_le_bytes());
            rec[0x10..0x14].copy_from_slice(&a.state_flags.to_le_bytes());
            rec[0x14..0x16].copy_from_slice(&a.local_x.to_le_bytes());
            rec[0x18..0x1A].copy_from_slice(&a.local_y.to_le_bytes());
            rec[0x52..0x54].copy_from_slice(&a.sprite_flags.to_le_bytes());
            rec[0x60..0x62].copy_from_slice(&a.tile_slot.to_le_bytes());
            cpu.mem.load(at, &rec);
            actors.push(a);
        }
        cpu.mem.wr32(HEAD, if n > 0 { ACTORS } else { 0 });
        cpu.mem.wr16(PLAYER + 0x14, centre.x as u16);
        cpu.mem.wr16(PLAYER + 0x18, centre.y as u16);
        cpu.mem.wr32(COUNT, 0xDEAD);
        for k in 0..VISIBLE_ACTOR_CAP as u32 + 1 {
            cpu.mem.wr32(TABLE + 4 * k, 0xA5A5_A5A5);
        }
        assert_eq!(cpu.call(BROAD_PHASE, &[PLAYER, HEAD]), Stop::Returned, "round {round}");
        let count = cpu.mem.rd32(COUNT);
        let retail: Vec<usize> = (0..count)
            .map(|k| ((cpu.mem.rd32(TABLE + 4 * k) - ACTORS) / ACTOR_LEN) as usize)
            .collect();
        let port = build_contact_candidates(centre, &actors, &descs);
        assert_eq!(retail, port, "round {round}: candidates of {n}");
        // Nothing past the count, and the centre the probes read.
        assert_eq!(cpu.mem.rd32(TABLE + 4 * count), 0xA5A5_A5A5, "round {round}");
        assert_eq!(
            (cpu.mem.rd16(CENTRE) as i16, cpu.mem.rd16(CENTRE + 4) as i16),
            (centre.x, centre.y),
            "round {round}"
        );
        full += usize::from(port.len() == VISIBLE_ACTOR_CAP);
        kept += port.len();
        dropped += actors.len() - port.len();
        empty += usize::from(n == 0);
    }
    assert!(
        full > 50 && kept > 5_000 && dropped > 5_000 && empty > 100,
        "{full} {kept} {dropped} {empty}"
    );
}
