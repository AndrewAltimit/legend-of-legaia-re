//! The battle cue router `FUN_8004FE5C(id, category)` against
//! `legaia_engine_battle::sfx_cue::route_sfx_cue`: the pending ring and its
//! counter, the dedupe against the last-played word `DAT_8007B724`, the VAB-slot
//! byte both tinted legs write into the runtime bank, and the CD-XA voice clip
//! the party leg starts.
//!
//! The two calls are hooked: `FUN_8003DE7C(1)` answers the CD-busy draw and
//! `FUN_8003D53C(clip, channel, sectors)` is logged.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::sfx_cue::{SfxCueRing, SfxCueSources, route_sfx_cue};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const ROUTE: u32 = 0x8004_FE5C;
const CD_BUSY: u32 = 0x8003_DE7C;
const XA_START: u32 = 0x8003_D53C;
const CTX_PTR: u32 = 0x8007_BD24;
const RING: u32 = 0x8007_B6D8;
const LAST_PLAYED: u32 = 0x8007_B724;
const BANK_PTR: u32 = 0x8007_B990;
const ACTORS: u32 = 0x801C_9370;
const XA_DURATIONS: u32 = 0x8007_88B8;

const CTX: u32 = 0x8012_0000;
const NODE: u32 = 0x8013_0000;
const ACTOR: u32 = 0x8014_0000;
const BANK: u32 = 0x8015_0000;
const BANK_LEN: u32 = 0x1000;

#[test]
fn the_router_matches_retail() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let busy = Rc::new(Cell::new(0u32));
    let b = busy.clone();
    cpu.hook(CD_BUSY, move |r, _| r.r[V0] = b.get());
    let xa: Rc<RefCell<Option<(u32, u32, u32)>>> = Rc::new(RefCell::new(None));
    let x = xa.clone();
    cpu.hook(XA_START, move |r, _| {
        *x.borrow_mut() = Some((r.r[A0], r.r[A1], r.r[A2]));
    });
    // Each category slot's render node carries its own VAB-slot byte.
    for c in 0..8u32 {
        cpu.mem.wr32(ACTORS + c * 4, ACTOR + c * 0x400);
        cpu.mem.wr32(ACTOR + c * 0x400 + 0x22C, NODE + c * 0x100);
        cpu.mem.wr8(NODE + c * 0x100 + 0x80, 0x30 + c as u8);
    }
    let durations: Vec<u16> = (0..0x100u32)
        .map(|i| cpu.mem.rd16(XA_DURATIONS + i * 2))
        .collect();
    let mut rng = Rng(0x5346_5843);
    let (mut legs, mut failures) = ([0u32; 5], Vec::new());
    for round in 0..40000 {
        let id = match rng.below(6) {
            0 => rng.below(0x48),
            1 => 0x48 + rng.below(0x1C),
            2 => 0x64 + rng.below(0x9C),
            3 => 0x100 + rng.below(0x100),
            4 => [
                0, 1, 0x1A, 0x1B, 0x47, 0x48, 0x63, 0x64, 0xA6, 0xA7, 0xFF, 0x100,
            ][rng.below(12) as usize],
            _ => rng.below(0x200),
        };
        let category = rng.below(8) as u8;
        let counter = rng.below(4) as u8;
        let side_band = if rng.below(4) == 0 {
            1 + rng.below(255) as u8
        } else {
            0
        };
        let cd_busy = u32::from(rng.below(4) == 0);
        let last = match rng.below(4) {
            0 => id.wrapping_sub(1),
            1 => id + 0x19C,
            _ => rng.below(0x400),
        };
        let slots: [u16; 4] = std::array::from_fn(|_| rng.below(0x400) as u16);

        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr8(CTX + 9, counter);
        cpu.mem.wr8(CTX + 0x276, side_band);
        cpu.mem.wr32(LAST_PLAYED, last);
        cpu.mem.wr32(BANK_PTR, BANK);
        for i in 0..BANK_LEN {
            cpu.mem.wr8(BANK + i, 0);
        }
        for (i, s) in slots.iter().enumerate() {
            cpu.mem.wr16(RING + i as u32 * 2, *s);
        }
        busy.set(cd_busy);
        *xa.borrow_mut() = None;
        assert_eq!(cpu.call(ROUTE, &[id, u32::from(category)]), Stop::Returned);
        let r_slots: [u16; 4] = std::array::from_fn(|i| cpu.mem.rd16(RING + i as u32 * 2));
        let r_counter = cpu.mem.rd8(CTX + 9);
        let r_bank = cpu.mem.read(BANK, BANK_LEN as usize);
        let r_xa = *xa.borrow();

        // Port.
        let mut ring = SfxCueRing {
            slots,
            counter,
            last_played: last,
        };
        let src = SfxCueSources {
            element_of: &|c| 0x30 + c,
            xa_duration_raw: &|v| durations[v as usize],
            side_band_streaming: side_band != 0,
            cd_read_busy: cd_busy != 0,
        };
        let out = route_sfx_cue(&mut ring, id, category, &src);
        let mut p_bank = vec![0u8; BANK_LEN as usize];
        if let (Some(rid), Some(e)) = (out.enqueued, out.element_write) {
            p_bank[(usize::from(rid) - 0x200) * 8 + 4] = e;
        }
        let p_xa = out.xa.map(|c| (c.clip, c.channel, c.duration_sectors));

        legs[match (id >= 0x100 && category < 3, out.enqueued) {
            (true, _) if p_xa.is_some() => 0,
            (true, _) => 1,
            (false, None) => 2,
            (false, Some(_)) if out.element_write.is_some() => 3,
            _ => 4,
        }] += 1;
        let retail = (r_slots, r_counter, r_xa);
        let port = (ring.slots, ring.counter, p_xa);
        if (retail != port || r_bank != p_bank) && failures.len() < 8 {
            failures.push(format!(
                "round {round}: id {id:#x} cat {category} counter {counter} last {last:#x} \
                 side {side_band} busy {cd_busy}\n retail {retail:04x?}\n port   {port:04x?}\n \
                 bank {:?}",
                crate::common::diff_offsets(&r_bank, &p_bank)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(legs.iter().all(|&n| n > 500), "{legs:?}");
}
