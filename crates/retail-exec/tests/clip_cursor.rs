//! The per-actor clip cursor `FUN_800204F8` against
//! `move_buffer::cursor_advance`, frame by frame over generated clip banks in
//! all three pools (`_DAT_8007B888` MOVE, `_DAT_8007B840` MOVE2 past id
//! `0x400`, `_DAT_8007B75C` under `+0x10 & 0x01000000`): the record latch and
//! its kick, the halved-rate records, the start request forward and reversed,
//! pause, the wrap and clamp at both ends, and the envelope pre-tick.
//!
//! Every record has at least one frame and a non-zero divisor: retail's
//! halved-rate `div` has no zero guard and a zero-frame record's reversed
//! start wraps through `0xFFFF`, neither of which a shipped clip reaches.

use crate::common::{Rng, scus_cpu};
use legaia_engine_vm::move_buffer::{MoveBufferHost, MoveBufferState, cursor_advance};
use psx_retail_exec::Stop;
use std::collections::HashMap;

const CURSOR: u32 = 0x8002_04F8;
const ACTOR: u32 = 0x8013_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const DEBUG: u32 = 0x8007_B6D0;
/// `(pointer word, bank address)` for MOVE, MOVE2 and the party pool.
const POOLS: [(u32, u32); 3] = [
    (0x8007_B888, 0x8014_0000),
    (0x8007_B840, 0x8015_0000),
    (0x8007_B75C, 0x8016_0000),
];
const IDS: u32 = 24;

struct Banks(HashMap<(usize, u16), Vec<u8>>);

fn pool(flags: u32, id: i16) -> usize {
    if flags & 0x0100_0000 != 0 {
        2
    } else if id >= 0x400 {
        1
    } else {
        0
    }
}

impl MoveBufferHost for Banks {
    fn resolve_record(&self, flags: u32, id: i16) -> Option<&[u8]> {
        self.0
            .get(&(pool(flags, id), id as u16 & 0x3FF))
            .map(Vec::as_slice)
    }
}

#[test]
fn the_clip_cursor_steps_as_retail_steps() {
    differential!(scus(CURSOR));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(0x8001_A068, 0);
    cpu.mem.wr32(DEBUG, 0);
    let mut rng = Rng(0x4355_5253);
    let mut banks = Banks(HashMap::new());
    for (p, (word, base)) in POOLS.iter().enumerate() {
        cpu.mem.wr32(*word, *base);
        for id in 0..IDS {
            let frames = [1u16, 2, 8, 30, 0x100][rng.below(5) as usize];
            let rec = vec![
                rng.next() as u8,
                rng.next() as u8,
                frames as u8,
                (frames >> 8) as u8,
                rng.next() as u8,
                rng.next() as u8,
                1 + rng.below(8) as u8,
                rng.next() as u8,
            ];
            let at = base + 0x200 + id * 0x10;
            cpu.mem.wr32(base + id * 4, at - base);
            cpu.mem.load(at, &rec);
            banks.0.insert((p, id as u16), rec);
        }
    }
    let (mut latches, mut clamps, mut halved) = (0, 0, 0);
    for run in 0..2000 {
        let mut s = MoveBufferState {
            status_flags: rng.next() & !0x0100_1000
                | if rng.below(3) == 0 { 0x0100_0000 } else { 0 }
                | if rng.below(4) == 0 { 0x1000 } else { 0 },
            env_flags: rng.next() as u16 & 0x038A,
            phase_rate: [1i16, 4, 16, 32, -8][rng.below(5) as usize],
            bone_count: rng.below(4) as u8,
            cursor_active: -1,
            ..Default::default()
        };
        for l in 0..8 {
            s.up_velocity[l] = 0x100;
            s.down_velocity[l] = 0x80;
        }
        cpu.mem.wr32(ACTOR + 0x10, s.status_flags);
        cpu.mem.wr16(ACTOR + 0x5E, s.cursor_active as u16);
        cpu.mem.wr16(ACTOR + 0x62, s.env_flags);
        cpu.mem.wr16(ACTOR + 0x68, 0);
        cpu.mem.wr16(ACTOR + 0x6A, s.phase_rate as u16);
        cpu.mem.wr8(ACTOR + 0x6C, s.bone_count);
        cpu.mem.wr32(ACTOR + 0x7C, 0);
        for l in 0..8u32 {
            cpu.mem.wr16(ACTOR + 0xA0 + l * 2, 0);
            cpu.mem.wr16(ACTOR + 0xB8 + l * 2, 0x100);
            cpu.mem.wr16(ACTOR + 0xC8 + l * 2, 0x80);
        }
        for frame in 0..60 {
            if frame == 0 || rng.below(15) == 0 {
                let id = match rng.below(6) {
                    0 => 0,
                    1 => 0x400 + rng.below(IDS) as i16,
                    _ => rng.below(IDS) as i16,
                };
                s.cursor_requested = id;
                cpu.mem.wr16(ACTOR + 0x5C, id as u16);
            }
            if rng.below(10) == 0 {
                s.env_flags ^= [0x0002u16, 0x0080, 0x0008, 0x0200][rng.below(4) as usize];
                cpu.mem.wr16(ACTOR + 0x62, s.env_flags);
            }
            let dt = [1u8, 1, 2, 3][rng.below(4) as usize];
            cpu.mem.wr8(FRAME_STEP, dt);
            cpu.mem.wr16(ACTOR + 0x56, 0);
            s.move_vm_kick = 0;
            assert_eq!(cpu.call(CURSOR, &[ACTOR]), Stop::Returned);
            let before = s.clone();
            cursor_advance(&mut s, &banks, dt);
            let label = format!("run {run} frame {frame} dt {dt} from {before:?}");
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x5E) as i16,
                s.cursor_active,
                "{label}"
            );
            assert_eq!(cpu.mem.rd16(ACTOR + 0x56) as i16, s.move_vm_kick, "{label}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x62), s.env_flags, "{label}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x68) as i16, s.phase, "{label}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x7C), s.done_mask, "{label}");
            if s.cursor_requested > 0 {
                let rec = banks
                    .resolve_record(s.status_flags, s.cursor_requested)
                    .unwrap();
                halved += u32::from(rec[1] & 1 != 0);
            }
            latches += u32::from(s.move_vm_kick == 1);
            clamps += u32::from(s.env_flags & 0x100 != 0);
        }
    }
    assert!(
        latches > 5000 && clamps > 10_000 && halved > 10_000,
        "{latches} {clamps} {halved}"
    );
}
