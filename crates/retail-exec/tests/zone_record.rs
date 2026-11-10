//! The camera-zone record loader `FUN_801DBC20` (PROT 0897) against
//! `CameraZoneConfig::load_record`: the thirteen globals `0x8007B607..0x8007B627`
//! each mode writes (and the ones it leaves), the mode-5 height's sign
//! extension, the mode-6 skip, and the visible-tile window it latches into
//! `0x801F2778` and the scratchpad `0x1F8003E8`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::camera_zone::CameraZoneConfig;
use legaia_engine_field::field_regions::ZONE_RECORD_STRIDE;
use psx_retail_exec::{Cpu, Stop};

const LOAD: u32 = 0x801D_BC20;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RECORD: u32 = 0x8014_0000;
const WINDOW: u32 = 0x801F_2778;
const SCRATCH_WINDOW: u32 = 0x1F80_03E8;

fn seat(cpu: &mut Cpu, c: &CameraZoneConfig) {
    cpu.mem.wr8(0x8007_B607, c.mode);
    cpu.mem.wr8(0x8007_B608, c.b608);
    cpu.mem.wr8(0x8007_B609, c.b609);
    cpu.mem.wr8(0x8007_B60A, c.b60a);
    cpu.mem.wr8(0x8007_B60B, c.b60b);
    for (at, v) in words(c) {
        cpu.mem.wr32(at, v as u32);
    }
}

fn words(c: &CameraZoneConfig) -> [(u32, i32); 7] {
    [
        (0x8007_B60C, c.pitch),
        (0x8007_B610, c.yaw),
        (0x8007_B614, c.depth),
        (0x8007_B618, c.h),
        (0x8007_B61C, c.anchor_x),
        (0x8007_B620, c.anchor_h),
        (0x8007_B624, c.anchor_z),
    ]
}

#[test]
fn a_zone_record_loads_what_retail_loads() {
    differential!(prot(897, LOAD));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let mut rng = Rng(0x5A4F_4E45);
    let (mut modes, mut windows, mut skipped) = ([0u32; 16], 0, 0);
    for round in 0..40_000 {
        let mut c = CameraZoneConfig {
            mode: rng.next() as u8,
            b608: rng.next() as u8,
            b609: rng.next() as u8,
            b60a: rng.next() as u8,
            b60b: rng.next() as u8,
            pitch: rng.next() as i32,
            yaw: rng.next() as i32,
            depth: rng.next() as i32,
            h: rng.next() as i32,
            anchor_x: rng.next() as i32,
            anchor_h: rng.next() as i32,
            anchor_z: rng.next() as i32,
        };
        seat(&mut cpu, &c);
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        for b in rec.iter_mut() {
            *b = rng.next() as u8;
        }
        rec[5] = (rng.below(8) as u8) << 4 | rng.below(16) as u8;
        if rng.below(4) == 0 {
            rec[1] = 0;
        }
        if rng.below(2) == 0 {
            rec[0] = rng.below(0x24) as u8;
        }
        cpu.mem.load(RECORD, &rec);
        let before_window = cpu.mem.read(SCRATCH_WINDOW, 4);
        cpu.mem.wr32(WINDOW, 0xAAAA_AAAA);
        assert_eq!(cpu.call(LOAD, &[RECORD]), Stop::Returned);
        let window = c.load_record(&rec);
        let label = format!("round {round} rec {rec:02x?}");
        assert_eq!(cpu.mem.rd8(0x8007_B607), c.mode, "{label}");
        assert_eq!(cpu.mem.rd8(0x8007_B608), c.b608, "{label}");
        assert_eq!(cpu.mem.rd8(0x8007_B609), c.b609, "{label}");
        assert_eq!(cpu.mem.rd8(0x8007_B60A), c.b60a, "{label}");
        assert_eq!(cpu.mem.rd8(0x8007_B60B), c.b60b, "{label}");
        for (at, v) in words(&c) {
            assert_eq!(cpu.mem.rd32(at) as i32, v, "{label} {at:#x}");
        }
        match window {
            Some(w) => {
                assert_eq!(cpu.mem.read(SCRATCH_WINDOW, 4), w, "{label}");
                let latched: Vec<u8> = (0..4).map(|i| cpu.mem.rd32(WINDOW + i * 4) as u8).collect();
                // `0x801F2778` holds the words in record order `[3, 1, 4, 2]`.
                assert_eq!(latched, [w[0], w[2], w[1], w[3]], "{label}");
                windows += 1;
            }
            None => {
                assert_eq!(cpu.mem.read(SCRATCH_WINDOW, 4), before_window, "{label}");
                assert_eq!(cpu.mem.rd32(WINDOW), 0xAAAA_AAAA, "{label}");
            }
        }
        modes[usize::from(rec[5] >> 4)] += 1;
        skipped += u32::from(rec[5] >> 4 == 6);
    }
    assert!(
        windows > 5000 && skipped > 2000 && modes[3] > 2000 && modes[5] > 2000,
        "{windows} {skipped} {modes:?}"
    );
}
