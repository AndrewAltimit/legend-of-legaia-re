//! The EVENT FLAG editor's list-cursor steps in the field overlay's developer
//! menu (PROT 0897): the next-entry arm at `0x801EA0CC` and the previous-entry
//! arm at `0x801EA10C`, against `flag_list_next` / `flag_list_prev` over
//! generated stride-`0xA` flag lists. Both are arms of the dev-menu dispatcher
//! rather than called routines: each is entered with the registers the
//! dispatcher holds there, and its jumps back into the dispatcher are hooked
//! as the end of the run. The `overlay_0897_*` dumps print both arms
//! `0xE818` low (`0x801DB8B4` / `0x801DB8F4`), the shape
//! `docs/tooling/dump-corpus-integrity.md` describes; the VAs here are the
//! slot-A load base's.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dev_menu::dev_menu_flags::{FLAG_LIST_END_TAG, flag_list_next, flag_list_prev};
use psx_retail_exec::{Cpu, Stop, V0};

const OVERLAY_BASE: u32 = 0x801C_E818;
const NEXT: u32 = 0x801E_A0CC;
const PREV: u32 = 0x801E_A10C;
/// Where each arm rejoins the dispatcher.
const JOINS: [u32; 2] = [0x801E_A104, 0x801E_A7AC];
const CURSOR: u32 = 0x801F_2E90;
const TABLE: u32 = 0x801F_2E94;

fn rig() -> Option<Cpu> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(897).expect("PROT 0897"),
    );
    for j in JOINS {
        cpu.stub(j, 0);
    }
    Some(cpu)
}

fn seat(cpu: &mut Cpu, tags: &[u8]) {
    for (i, &t) in tags.iter().enumerate() {
        cpu.mem.wr8(TABLE + 0xA * i as u32 + 2, t);
    }
}

fn list(rng: &mut Rng) -> Vec<u8> {
    let n = rng.below(40) as usize;
    let mut tags: Vec<u8> = (0..n)
        .map(|_| match rng.next() as u8 {
            FLAG_LIST_END_TAG => 0,
            t => t,
        })
        .collect();
    tags.push(FLAG_LIST_END_TAG);
    // Bytes past the sentinel that the scan must never reach.
    tags.extend((0..4).map(|_| rng.next() as u8));
    tags
}

#[test]
fn the_flag_list_cursor_steps_as_retail_steps() {
    differential!(prot(897, NEXT), prot(897, PREV));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x464C_4147);
    let (mut wraps_next, mut wraps_prev) = (0, 0);
    for case in 0..3000 {
        let tags = list(&mut rng);
        let real = tags.iter().position(|&t| t == FLAG_LIST_END_TAG).unwrap() as i32;
        seat(&mut cpu, &tags);
        // Every real row, plus the cursor the empty list leaves (`-1`).
        let cursor = rng.below(real.max(1) as u32) as i32 - i32::from(real == 0);
        if real > 0 {
            cpu.mem.wr32(CURSOR, cursor as u32);
            assert_eq!(cpu.call(NEXT, &[0x801F_0000, 0x801F_0000]), Stop::Returned);
            let p = flag_list_next(cursor, &tags);
            assert_eq!(
                cpu.mem.rd32(CURSOR) as i32,
                p,
                "case {case} next from {cursor} of {real}"
            );
            wraps_next += u32::from(p == 0);
        }
        cpu.mem.wr32(CURSOR, cursor.max(0) as u32);
        // The arm opens on the held-pad test; bit 15 is the step.
        cpu.regs.r[V0] = 0x8000;
        assert_eq!(cpu.call(PREV, &[]), Stop::Returned);
        let p = flag_list_prev(cursor.max(0), &tags);
        assert_eq!(
            cpu.mem.rd32(CURSOR) as i32,
            p,
            "case {case} prev from {cursor} of {real}"
        );
        wraps_prev += u32::from(cursor <= 0);
    }
    assert!(
        wraps_next > 50 && wraps_prev > 50,
        "{wraps_next} {wraps_prev}"
    );
}
