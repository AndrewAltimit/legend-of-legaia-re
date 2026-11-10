//! The Arts entry: `FUN_801D0748` state `0x50` with `FUN_801D388C` cases `0xB`,
//! `0xC`, `0x26` and `0x2F`, against `ArtsCommandInputSession`. A generated pad
//! script runs on both sides: every press, the debit, the refusal, the auto end,
//! the confirm and cancel forks, the Rot limb gates and the pool's boundaries.

use crate::common::{Rng, disc, scus_cpu};
use legaia_art::queue::Command;
use legaia_engine_battle::arts_command_input::{
    ArtsCommandInputSession, ArtsCommandPad, ArtsInputPhase, rot_blocks,
};
use legaia_engine_battle::target_picker::SlotState;
use psx_retail_exec::{Cpu, Stop};

const STATE_MACHINE: u32 = 0x801D_0748;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const PAD: u32 = 0x8007_B874;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const COMMANDS: [(u32, Command); 4] = [
    (0x8000, Command::Left),
    (0x1000, Command::Up),
    (0x4000, Command::Down),
    (0x2000, Command::Right),
];
const CONFIRM: u32 = 0x0040;
const CANCEL: u32 = 0x0020;

fn frame(cpu: &mut Cpu, pad: u32) {
    cpu.mem.wr32(PAD, pad);
    assert_eq!(cpu.call(STATE_MACHINE, &[]), Stop::Returned, "pad {pad:#x}");
}

#[test]
fn an_arts_entry_debits_and_ends_as_retail_does() {
    differential!(prot(898, STATE_MACHINE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    for stub in [
        0x8004_FCC8u32, // the blip
        0x801D_3444,    // the frame's HUD pass
        0x801D_9BBC,    // the window glide: settled
        0x801D_B8F4,    // the chip draw
        0x801D_BDDC,    // the Rot marks
        0x801D_8A88,
        0x801D_A6B4,
        0x801D_3748,
        0x801D_8DE8,
        0x801D_5854, // the camera
        0x801D_5718,
        0x8003_5F04,
    ] {
        cpu.stub(stub, 0);
    }
    let mut rng = Rng(0x4152_5453);
    let dead = SlotState::default();
    let (mut ends, mut refused, mut restarts) = (0, 0, 0);
    for round in 0..3000 {
        let costs: [u8; 4] =
            std::array::from_fn(|_| [0x1E, 0x1E, 0x2A, 0x36][rng.below(4) as usize]);
        let pool: u16 = [
            0,
            1,
            29,
            30,
            31,
            60,
            84,
            100,
            0x120,
            0xCF,
            rng.below(0x120) as u16,
        ][rng.below(11) as usize];
        let status: u16 = [0, 0, 0, 8, 0x10, 0x20, 0x38][rng.below(7) as usize];
        cpu.mem.wr32(CTX_PTR, CTX);
        for b in 0..0x900 {
            cpu.mem.wr8(CTX + b, 0);
        }
        for b in 0..0x400 {
            cpu.mem.wr8(ACTOR + b, 0);
        }
        cpu.mem.wr32(ACTORS, ACTOR);
        cpu.mem.wr8(CTX + 0x06, 0x50);
        cpu.mem.wr16(CTX + 0x6DC, pool);
        cpu.mem.wr16(ACTOR + 0x154, pool);
        cpu.mem.wr16(ACTOR + 0x16E, status);
        cpu.mem.wr32(CONFIRM_MASK, CONFIRM);
        cpu.mem.wr32(CANCEL_MASK, CANCEL);
        for (i, c) in costs.iter().enumerate() {
            cpu.mem.wr8(CTX + 0x14 + i as u32, *c);
        }
        // Retail's cost seats are Left, Up, Down, Right; the session's run
        // Left, Right, Down, Up.
        let port_costs = [
            u16::from(costs[0]),
            u16::from(costs[3]),
            u16::from(costs[2]),
            u16::from(costs[1]),
        ];
        let mut s = ArtsCommandInputSession::new(0, 0, pool, port_costs, 0);
        for step in 0..14 {
            let press = match rng.below(10) {
                0..=5 => COMMANDS[rng.below(4) as usize].0,
                6 => CONFIRM,
                7 => CANCEL,
                // Two edges on one frame: the arm tests overwrite each other.
                8 => COMMANDS[rng.below(4) as usize].0 | COMMANDS[rng.below(4) as usize].0,
                _ => 0,
            };
            let before_count = cpu.mem.rd8(CTX + 0x19);
            frame(&mut cpu, press);
            // Retail's automatic end is read on the frame after the press.
            frame(&mut cpu, 0);
            let mut ev = ArtsCommandPad::default();
            for (mask, cmd) in COMMANDS {
                if press & mask != 0 && !rot_blocks(status, cmd) {
                    match cmd {
                        Command::Up => ev.up = true,
                        Command::Down => ev.down = true,
                        Command::Left => ev.left = true,
                        Command::Right => ev.right = true,
                    }
                }
            }
            ev.cross = press == CONFIRM;
            ev.circle = press == CANCEL;
            s.input(ev, [dead; 3], [dead; 5]);
            let retail_state = cpu.mem.rd8(CTX + 0x06);
            let retail_pool = cpu.mem.rd16(CTX + 0x6DC) as i16;
            let retail_count = cpu.mem.rd8(CTX + 0x19);
            let port_state = match s.phase {
                ArtsInputPhase::Entering => 0x50,
                ArtsInputPhase::Review => 0x5A,
                ArtsInputPhase::Aborted => 0x78,
                _ => 0xFF,
            };
            let context = format!(
                "round {round} step {step} press {press:#x} pool {pool} costs {costs:?} status {status:#x} before {before_count}"
            );
            assert_eq!(retail_state, port_state, "state: {context}");
            if retail_state != 0x50 && retail_state != 0x5A {
                break;
            }
            assert_eq!(i32::from(retail_pool), i32::from(s.pool), "pool: {context}");
            assert_eq!(
                usize::from(retail_count),
                s.committed_string().len(),
                "count: {context}"
            );
            let buffer = cpu.mem.read(ACTOR + 0x1DF, usize::from(retail_count));
            // The session keeps `Command` bytes; `+ 0x0B` is the queue's alphabet.
            let port_buffer: Vec<u8> = s.committed_string().iter().map(|b| b + 0x0B).collect();
            assert_eq!(buffer, port_buffer, "buffer: {context}");
            if retail_state == 0x5A {
                ends += 1;
                break;
            }
            if retail_count == before_count && press & 0xF000 != 0 {
                refused += 1;
            }
            if press == CANCEL && before_count != 0 {
                restarts += 1;
            }
        }
    }
    assert!(
        ends > 300 && refused > 100 && restarts > 50,
        "{ends} {refused} {restarts}"
    );
}
