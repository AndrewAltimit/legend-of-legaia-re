//! Baka Fighter's arena initialiser in PROT 0976.

use crate::common::{disc, scus_cpu};
use psx_retail_exec::{Cpu, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;

fn rig() -> Option<Cpu> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
    Some(cpu)
}

/// The duel arena's initialiser `FUN_801CF00C`, run whole with everything it
/// calls stood in, against `duel_overlay_init`: the arguments it hands the
/// display, sound and stream setup, and the words it leaves.
#[test]
fn the_duel_init_seeds_what_the_port_records() {
    differential!(prot(976, DUEL_INIT));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    const DUEL_INIT: u32 = 0x801C_F00C;
    const DUEL_INIT_END: u32 = 0x801C_F384;
    cpu.stub_outside = Some((DUEL_INIT, DUEL_INIT_END));
    type Calls = Rc<RefCell<Vec<(u32, [u32; 4])>>>;
    let calls: Calls = Rc::default();
    for va in [
        0x8001_DAF8u32, // display width
        0x8001_DCF8,    // ordering-table depth
        0x8006_2004,    // sequence volume
        0x801D_4C50,    // fighter install
        0x8001_FC00,    // stream load
    ] {
        let calls = calls.clone();
        cpu.hook(va, move |r, _| {
            calls.borrow_mut().push((
                va,
                [
                    r.r[psx_retail_exec::A0],
                    r.r[psx_retail_exec::A1],
                    r.r[psx_retail_exec::A2],
                    r.r[psx_retail_exec::A3],
                ],
            ));
            r.r[psx_retail_exec::V0] = 0;
        });
    }
    cpu.stub(0x8001_7888, 0x8018_0000);
    // The load buffer and the id-load form of the disc build.
    cpu.mem.wr32(0x8007_B85C, 0x8010_0000);
    cpu.mem.wr16(0x8007_B8C2, 1);
    assert_eq!(cpu.call(DUEL_INIT, &[]), Stop::Returned);
    let port = legaia_engine_system::mode_entry_init::duel_overlay_init();
    let calls = calls.borrow();
    let of =
        |va: u32| -> Vec<[u32; 4]> { calls.iter().filter(|c| c.0 == va).map(|c| c.1).collect() };
    assert_eq!(of(0x8001_DAF8)[0][0], u32::from(port.screen_width));
    assert_eq!(of(0x8001_DCF8)[0][0], u32::from(port.ot_depth));
    assert_eq!(of(0x8006_2004)[0][2], u32::from(port.bgm_arg3));
    assert_eq!(
        of(0x801D_4C50)
            .iter()
            .map(|a| a[0] as u8)
            .collect::<Vec<_>>(),
        port.fighter_slots
    );
    let streams: Vec<u32> = of(0x8001_FC00).iter().map(|a| a[0]).collect();
    assert_eq!(
        streams,
        [port.sfx_stream_id, port.sfx_stream_id, port.voice_stream_id]
    );
    // The second SFX load carries the re-read flag.
    assert_eq!(
        of(0x8001_FC00).iter().map(|a| a[3]).collect::<Vec<_>>(),
        [0, 1, 0]
    );
    assert_eq!(cpu.mem.rd32(0x801D_BED0), port.round_win_target);
    assert_eq!(cpu.mem.rd32(0x801D_BEBC), port.stage_seed);
    assert_eq!(
        (cpu.mem.rd32(0x8008_40BC), cpu.mem.rd32(0x8008_40C0)),
        port.camera_pair
    );
    assert_eq!(
        (cpu.mem.rd16(0x1F80_03F8), cpu.mem.rd16(0x1F80_03FA)),
        port.window_tiles
    );
}
