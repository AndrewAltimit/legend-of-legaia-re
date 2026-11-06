//! The camera-register zone ramp's per-frame handler `FUN_80037018` against
//! `ambient_motion::zone_ramp_tick`: the engaged and system-lock gates, the X
//! gate, the Z window with its interpolation, every store width and the
//! retire arm, over windows of every shape including a negative ramp.

use crate::common::{Rng, scus_cpu};
use legaia_engine_motion_vm::ambient_motion::{
    ZoneRamp, ZoneRampTick, ZoneRampWidth, zone_ramp_tick,
};
use legaia_retail_exec::Stop;

const TICK: u32 = 0x8003_7018;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8013_0000;
const ACTOR: u32 = 0x8014_0000;
const DEST: u32 = 0x8015_0000;
const LOCK: u32 = 0x1F80_0394;

#[test]
fn the_zone_ramp_writes_what_retail_writes() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x5241_4D50);
    let (mut failures, mut writes) = (Vec::new(), 0);
    let coord = |rng: &mut Rng| rng.below(0x4000) as i16 - 0x400;
    for round in 0..100_000 {
        let (a, b) = (coord(&mut rng), coord(&mut rng));
        let (c, d) = (coord(&mut rng), coord(&mut rng));
        let ramp = ZoneRamp {
            start: rng.below(0x10000) as i32 - 0x8000,
            end: rng.below(0x10000) as i32 - 0x8000,
            x_lo: a.min(b),
            x_hi: a.max(b),
            z_lo: c.min(d),
            z_hi: if rng.below(50) == 0 {
                c.min(d)
            } else {
                c.max(d)
            },
            kind: [1, 2, 3, 4, 4, 4, 0, 5][rng.below(8) as usize],
        };
        let (px, pz) = match rng.below(3) {
            0 => (coord(&mut rng), coord(&mut rng)),
            _ => (
                ramp.x_lo + rng.below((ramp.x_hi - ramp.x_lo) as u32 + 1) as i16,
                ramp.z_lo + rng.below((ramp.z_hi - ramp.z_lo) as u32 + 1) as i16,
            ),
        };
        let engaged = rng.below(8) == 0;
        let lock = rng.below(8) == 0;
        cpu.mem
            .wr32(PLAYER + 0x10, if engaged { 0x80000 } else { 0 });
        cpu.mem.wr16(PLAYER + 0x14, px as u16);
        cpu.mem.wr16(PLAYER + 0x18, pz as u16);
        cpu.mem.wr32(LOCK, if lock { 0x400 } else { 0 });
        cpu.mem.wr32(ACTOR + 0x10, 0);
        cpu.mem.wr32(ACTOR + 0x80, ramp.start as u32);
        cpu.mem.wr32(ACTOR + 0x84, ramp.end as u32);
        cpu.mem.wr16(ACTOR + 0x88, ramp.x_lo as u16);
        cpu.mem.wr16(ACTOR + 0x8A, ramp.z_lo as u16);
        cpu.mem.wr16(ACTOR + 0xC8, ramp.x_hi as u16);
        cpu.mem.wr16(ACTOR + 0xCA, ramp.z_hi as u16);
        cpu.mem.wr32(ACTOR + 0x8C, ramp.kind as u32);
        cpu.mem.wr32(ACTOR + 0x94, DEST);
        cpu.mem.wr32(DEST, 0xA5A5_A5A5);
        let stop = cpu.call(TICK, &[ACTOR]);
        let retail = match stop {
            Stop::Returned if cpu.mem.rd32(ACTOR + 0x10) & 8 != 0 => "retire".to_string(),
            Stop::Returned => format!("{:#x}", cpu.mem.rd32(DEST)),
            Stop::DivideByZero { .. } => "trap".to_string(),
            other => panic!("round {round}: {other:?}"),
        };
        let port = match zone_ramp_tick(&ramp, px, pz, engaged, lock) {
            ZoneRampTick::Idle => "0xa5a5a5a5".to_string(),
            ZoneRampTick::Retire => "retire".to_string(),
            ZoneRampTick::DivideByZero => "trap".to_string(),
            ZoneRampTick::Write { value, width } => {
                writes += 1;
                let v = value as u32;
                format!(
                    "{:#x}",
                    match width {
                        ZoneRampWidth::U8 => 0xA5A5_A500 | (v & 0xFF),
                        ZoneRampWidth::U16 => 0xA5A5_0000 | (v & 0xFFFF),
                        ZoneRampWidth::U32 => v,
                    }
                )
            }
        };
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} {ramp:?} at ({px}, {pz}) engaged {engaged} lock {lock}: retail {retail} port {port}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(writes > 20000, "{writes} writes");
}
