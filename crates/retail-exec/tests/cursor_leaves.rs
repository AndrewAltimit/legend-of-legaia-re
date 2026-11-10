//! Four frameless leaves of the battle command flow:
//!
//! - `FUN_801D32BC`, the living-actor cursor step, against
//!   `step_actor_cursor`: both directions and an unknown one, every start
//!   seat, actor count and mix of fallen and disabled seats, the four context
//!   bytes it rewrites.
//! - `FUN_801D57E8`, `FUN_801D5718` and `FUN_801D5778`, the placement-record
//!   copies, against `element_placement_copy`, `element_placement_land` and
//!   `element_placement_copy_remapped` over the whole placement table at
//!   `0x80076C10`, the source and destination the same record included.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle_vm::battle_cursor_pose::{
    ActorCursor, CursorActor, CursorStep, ELEMENT_PLACEMENT_STRIDE, ElementPlacement,
    element_placement_copy, element_placement_copy_remapped, element_placement_land,
    step_actor_cursor,
};
use psx_retail_exec::Stop;

const STEP: u32 = 0x801D_32BC;
const COPY: u32 = 0x801D_57E8;
const LAND: u32 = 0x801D_5718;
const REMAP: u32 = 0x801D_5778;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const PLACEMENTS: u32 = 0x8007_6C10;
const RECORDS: usize = 0x50;

#[test]
fn the_cursor_step_lands_where_retail_lands() {
    differential!(prot(898, STEP));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let mut rng = Rng(0x4355_5253);
    let (mut found, mut ran_off, mut ignored) = (0, 0, 0);
    for n in 0..40_000 {
        let count = 1 + rng.below(8) as u8;
        let actors: Vec<CursorActor> = (0..8)
            .map(|_| CursorActor {
                liveness: if rng.below(4) == 0 {
                    0
                } else {
                    1 + rng.below(999) as u16
                },
                status: match rng.below(4) {
                    0 => 1 << rng.below(16),
                    1 => rng.next() as u16,
                    _ => 0,
                },
            })
            .collect();
        for (s, a) in actors.iter().enumerate() {
            let at = ACTOR + s as u32 * 0x400;
            cpu.mem.wr16(at + 0x14C, a.liveness);
            cpu.mem.wr16(at + 0x16E, a.status);
        }
        let mut cursor = ActorCursor {
            active: rng.below(u32::from(count)) as u8,
            latest: rng.next() as u8,
            previous: rng.next() as u8,
            depth: rng.next() as u8,
        };
        cpu.mem.wr8(CTX, count);
        cpu.mem.wr8(CTX + 0x13, cursor.active);
        cpu.mem.wr8(CTX + 0x20, cursor.latest);
        cpu.mem.wr8(CTX + 0x21, cursor.previous);
        cpu.mem.wr8(CTX + 0x1F, cursor.depth);
        let dir = [0u32, 1, 2][n % 3];
        assert_eq!(cpu.call(STEP, &[dir]), Stop::Returned);
        match dir {
            0 => step_actor_cursor(&mut cursor, CursorStep::Forward, count, &actors),
            1 => step_actor_cursor(&mut cursor, CursorStep::Backward, count, &actors),
            _ => ignored += 1,
        }
        let retail = ActorCursor {
            active: cpu.mem.rd8(CTX + 0x13),
            latest: cpu.mem.rd8(CTX + 0x20),
            previous: cpu.mem.rd8(CTX + 0x21),
            depth: cpu.mem.rd8(CTX + 0x1F),
        };
        assert_eq!(retail, cursor, "n {n} dir {dir} count {count} {actors:?}");
        if dir < 2 {
            if retail.active < count {
                found += 1;
            } else {
                ran_off += 1;
            }
        }
    }
    assert!(
        found > 5000 && ran_off > 5000 && ignored > 5000,
        "{found} {ran_off} {ignored}"
    );
}

fn parse(raw: &[u8]) -> Vec<ElementPlacement> {
    let h = |r: &[u8], o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
    raw.chunks(ELEMENT_PLACEMENT_STRIDE)
        .map(|r| ElementPlacement {
            f00: h(r, 0),
            f02: h(r, 2),
            f04: h(r, 4),
            f06: h(r, 6),
            f08: h(r, 8),
            f0a: h(r, 0xA),
            f0c: h(r, 0xC),
            f0e: h(r, 0xE),
            f10: h(r, 0x10),
            f12: h(r, 0x12),
            anim: u32::from_le_bytes([r[0x14], r[0x15], r[0x16], r[0x17]]),
        })
        .collect()
}

fn write(slots: &[ElementPlacement]) -> Vec<u8> {
    let mut out = Vec::with_capacity(slots.len() * ELEMENT_PLACEMENT_STRIDE);
    for s in slots {
        for h in [
            s.f00, s.f02, s.f04, s.f06, s.f08, s.f0a, s.f0c, s.f0e, s.f10, s.f12,
        ] {
            out.extend(h.to_le_bytes());
        }
        out.extend(s.anim.to_le_bytes());
    }
    out
}

#[test]
fn the_placement_copies_write_what_retail_writes() {
    differential!(prot(898, COPY), prot(898, LAND), prot(898, REMAP));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let mut rng = Rng(0x504C_4143);
    let mut same = 0;
    for n in 0..6000 {
        let raw: Vec<u8> = (0..RECORDS * ELEMENT_PLACEMENT_STRIDE)
            .map(|_| rng.next() as u8)
            .collect();
        cpu.mem.load(PLACEMENTS, &raw);
        let dst = rng.below(RECORDS as u32) as usize;
        let src = if rng.below(8) == 0 {
            same += 1;
            dst
        } else {
            rng.below(RECORDS as u32) as usize
        };
        let (entry, port): (u32, fn(&mut [ElementPlacement], usize, usize)) = match n % 3 {
            0 => (COPY, element_placement_copy),
            1 => (LAND, element_placement_land),
            _ => (REMAP, element_placement_copy_remapped),
        };
        assert_eq!(cpu.call(entry, &[dst as u32, src as u32]), Stop::Returned);
        let retail = cpu.mem.read(PLACEMENTS, raw.len());
        let mut slots = parse(&raw);
        port(&mut slots, dst, src);
        let port = write(&slots);
        assert!(
            retail == port,
            "{entry:#x} dst {dst} src {src}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
    assert!(same > 300, "{same}");
}
