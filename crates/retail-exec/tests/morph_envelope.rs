//! The per-lane morph envelope `FUN_80020740` against
//! `move_buffer::envelope_tick`, frame by frame over generated lane sets: the
//! init prime, the up-ramp cascade with its peak clamp, the hold and
//! reset-on-complete flags, the down-ramp cascade with its wrap detector and
//! the lane-0 snap.

use crate::common::{Rng, scus_cpu};
use legaia_engine_vm::move_buffer::{MoveBufferState, envelope_tick};
use psx_retail_exec::{Cpu, Stop};

const TICK: u32 = 0x8002_0740;
const ACTOR: u32 = 0x8013_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
/// The debug print's gate word; held clear.
const DEBUG: u32 = 0x8007_B6D0;
const LANES: usize = 8;

fn seat(cpu: &mut Cpu, s: &MoveBufferState) {
    cpu.mem.wr16(ACTOR + 0x62, s.env_flags);
    cpu.mem.wr8(ACTOR + 0x6C, s.bone_count);
    cpu.mem.wr32(ACTOR + 0x7C, s.done_mask);
    for l in 0..LANES as u32 {
        cpu.mem.wr16(ACTOR + 0xA0 + l * 2, s.lanes[l as usize]);
        cpu.mem
            .wr16(ACTOR + 0xB8 + l * 2, s.up_velocity[l as usize] as u16);
        cpu.mem
            .wr16(ACTOR + 0xC8 + l * 2, s.down_velocity[l as usize] as u16);
    }
}

#[test]
fn the_morph_envelope_ramps_as_retail_ramps() {
    differential!(scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(0x8001_A068, 0);
    cpu.mem.wr32(DEBUG, 0);
    let mut rng = Rng(0x454E_5645);
    let (mut peaks, mut drains, mut advances) = (0, 0, 0);
    for run in 0..3000 {
        let mut s = MoveBufferState {
            env_flags: (rng.next() as u16 & 0x7C00)
                | if rng.below(3) == 0 { 0x2000 } else { 0 }
                | if rng.below(20) == 0 { 0x8000 } else { 0 },
            bone_count: 1 + rng.below(LANES as u32) as u8,
            done_mask: 0,
            ..Default::default()
        };
        for l in 0..LANES {
            s.up_velocity[l] = [
                0x10i16,
                0x40,
                0x100,
                0x400,
                0x1000,
                rng.next() as i16 & 0x7FF,
            ][rng.below(6) as usize];
            s.down_velocity[l] = [
                0x10i16,
                0x40,
                0x100,
                0x400,
                0x1000,
                rng.next() as i16 & 0x7FF,
            ][rng.below(6) as usize];
        }
        seat(&mut cpu, &s);
        for frame in 0..80 {
            let dt = [1u8, 1, 2, 3, 4][rng.below(5) as usize];
            cpu.mem.wr8(FRAME_STEP, dt);
            assert_eq!(cpu.call(TICK, &[ACTOR]), Stop::Returned);
            let before = s.clone();
            envelope_tick(&mut s, dt);
            let label = format!("run {run} frame {frame} dt {dt} from {before:?}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x62), s.env_flags, "{label}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x7C), s.done_mask, "{label}");
            for l in 0..LANES as u32 {
                assert_eq!(
                    cpu.mem.rd16(ACTOR + 0xA0 + l * 2),
                    s.lanes[l as usize],
                    "{label} lane {l}"
                );
            }
            peaks +=
                u32::from(s.done_mask & 0x8000_0000 != 0 && before.done_mask & 0x8000_0000 == 0);
            drains += u32::from(s.done_mask.count_ones() < before.done_mask.count_ones());
            advances += u32::from(s.env_flags & 0x800 != 0 && before.env_flags & 0x800 == 0);
            // A move VM acknowledges the advance; clear it now and then.
            if rng.below(4) == 0 {
                s.env_flags &= !0x800;
                cpu.mem.wr16(ACTOR + 0x62, s.env_flags);
            }
        }
    }
    assert!(
        peaks > 1000 && drains > 1000 && advances > 1000,
        "{peaks} {drains} {advances}"
    );
}
