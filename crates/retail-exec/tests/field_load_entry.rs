//! The field-load entry step `FUN_80020118` against
//! `legaia_engine_scene::scene_bundle::field_load_entry_plan`: the staged
//! bundle index `DAT_8007B768` and the scene record over their ranges, the
//! path builder, the player-pack load and the stream request `FUN_8001EEF0`
//! hooked. Compared: whether the bundle is requested, the PROT index it asks
//! for, the kingdom-TMD prefix reset and the cleared descriptor-0 flags word.

use crate::common::{Rng, scus_cpu};
use legaia_engine_scene::scene_bundle::field_load_entry_plan;
use psx_retail_exec::{A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const ENTRY: u32 = 0x8002_0118;
const STRCAT: u32 = 0x8005_6728;
const PLAYER_PACK: u32 = 0x8001_E890;
const REQUEST: u32 = 0x8001_EEF0;
const WAIT: u32 = 0x8003_DE7C;
const STAGED: u32 = 0x8007_B768;
const BLOCKING: u32 = 0x8007_B8C2;
const PREFIX: u32 = 0x8007_B6F8;
const MAP_PTR: u32 = 0x1F80_03EC;
const MAP: u32 = 0x8012_0000;

#[test]
fn the_field_load_entry_step_matches_retail() {
    differential!(scus(ENTRY));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let requested = Rc::new(RefCell::new(None));
    let r = requested.clone();
    cpu.hook(REQUEST, move |regs, _| {
        *r.borrow_mut() = Some(regs.r[A1] as i32);
        regs.r[V0] = 1;
    });
    for stub in [STRCAT, PLAYER_PACK, WAIT] {
        cpu.hook(stub, |_, _| {});
    }
    cpu.mem.wr32(MAP_PTR, MAP);
    let mut rng = Rng(0x4649_454C);
    let mut staged_runs = 0;
    for case in 0..4000 {
        let record = if case % 4 == 0 {
            rng.next() as i16 & 0x3FFF
        } else {
            rng.below(1233) as i16
        };
        let staged = rng.next() as i16;
        cpu.mem.wr16(STAGED, staged as u16);
        cpu.mem.wr16(BLOCKING, rng.below(2) as u16);
        cpu.mem.wr32(PREFIX, rng.next());
        cpu.mem.wr32(MAP + 0x10, 0xFFFF_FFFF);
        *requested.borrow_mut() = None;
        assert_eq!(
            cpu.call(ENTRY, &[0x8013_0000, record as u32]),
            Stop::Returned
        );
        let plan = field_load_entry_plan(record, staged);
        let want = plan
            .stage_needed
            .then_some(i32::from(plan.data_field_chunk));
        assert_eq!(
            *requested.borrow(),
            want,
            "case {case}: record {record} staged {staged}"
        );
        assert_eq!(cpu.mem.rd32(PREFIX), 0, "the kingdom-TMD prefix resets");
        assert_eq!(
            cpu.mem.rd32(MAP + 0x10),
            0x0000_FFFF,
            "descriptor 0's flags clear"
        );
        staged_runs += usize::from(plan.stage_needed);
    }
    assert!(staged_runs > 1000);
}
