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
const WINDOW: u32 = 0x801D_388C;
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
    differential!(prot(898, STATE_MACHINE), prot(898, WINDOW));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    // The window opener's cases `0xB` / `0xC` / `0x26` / `0x2F` run nested;
    // the ledger notes them as they are entered.
    cpu.watched.insert(WINDOW);
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
            // The idle pass: case `0xB` returns ahead of the pool scan on a
            // pass that carries a direction, so both sides end an exhausted
            // entry on this one.
            s.input(ArtsCommandPad::default(), [dead; 3], [dead; 5]);
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

/// The entry over a preseeded window (`FUN_801DA34C`'s load, under the
/// staging byte `DAT_8007BD04`): a bare confirm replays the string as the
/// turn with no debit (`0x801D1FA0..0x801D2044`), and an empty cancel
/// measures the string still in the window into the count before it leaves
/// for the ring or the attack prompt (`0x801D211C..0x801D21A4`, the fork on
/// `_DAT_800846C4`). The first direction press wipes it on both sides.
#[test]
fn a_preseeded_arts_entry_replays_and_cancels_as_retail_does() {
    differential!(prot(898, STATE_MACHINE), prot(898, WINDOW));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    // The window opener's cases `0xB` / `0xC` / `0x26` / `0x2F` run nested;
    // the ledger notes them as they are entered.
    cpu.watched.insert(WINDOW);
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
    let mut rng = Rng(0x5245_504C);
    const STAGED: u32 = 0x8007_BD04;
    const SELECT_ATTACK: u32 = 0x8008_46C4;
    let dead = SlotState::default();
    let (mut replays, mut kept, mut wiped) = (0, 0, 0);
    let mut windows_wiped = 0;
    const HANDLES: u32 = 0x8018_0000;
    let freed: std::rc::Rc<std::cell::RefCell<Vec<u32>>> = Default::default();
    let log = freed.clone();
    cpu.hook(0x8003_19A8, move |r, _| {
        log.borrow_mut().push(r.r[psx_retail_exec::A0])
    });
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
        // The auto command string `FUN_801DA34C` loaded: under the staging
        // byte only (it zero-fills the window otherwise), cut to the prefix
        // a full pool affords as the gauge build `0x2C` cuts it.
        let staged = rng.below(4) != 0;
        let wanted: Vec<u8> = if staged {
            (0..rng.below(8)).map(|_| 1 + rng.below(4) as u8).collect()
        } else {
            Vec::new()
        };
        let mut s = ArtsCommandInputSession::new(0, 0, pool, port_costs, 0).with_preseed(wanted);
        let preseed = s.preseed.clone();
        cpu.mem.wr8(STAGED, u8::from(staged));
        cpu.mem.wr32(SELECT_ATTACK, rng.below(2));
        for (i, b) in preseed.iter().enumerate() {
            cpu.mem.wr8(ACTOR + 0x1DF + i as u32, b + 0x0B);
        }
        // The gauge build's text windows, which the first press's wipe
        // retires: handles in `ctx[+0x1074..]` whose kind `+0x8` is
        // `5..=0xD` (`0x801D3C2C..0x801D3C94`), the pennant windows the
        // port draws from `bar_commands` alone.
        let kinds: Vec<u16> = (0..40).map(|_| rng.below(0x14) as u16).collect();
        for (k, kind) in kinds.iter().enumerate() {
            let h = HANDLES + 0x10 * k as u32;
            cpu.mem.wr16(h + 8, *kind);
            cpu.mem.wr32(CTX + 0x1074 + 4 * k as u32, h);
        }
        freed.borrow_mut().clear();
        let mut windows_before = kinds.iter().any(|k| (5..=0xD).contains(k));
        for step in 0..14 {
            let phase_before = s.phase.clone();
            let shown_before = !s.preseed.is_empty();
            let press = match rng.below(10) {
                0..=2 => COMMANDS[rng.below(4) as usize].0,
                3..=5 => CONFIRM,
                6 | 7 => CANCEL,
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
            // The idle pass: case `0xB` returns ahead of the pool scan on a
            // pass that carries a direction, so both sides end an exhausted
            // entry on this one.
            s.input(ArtsCommandPad::default(), [dead; 3], [dead; 5]);
            // The port drops the preseed's pennants on the entry's first
            // press; retail frees the windows on the same press.
            let wiped_port = matches!(phase_before, ArtsInputPhase::Entering)
                && shown_before
                && s.preseed.is_empty()
                && !s.replay;
            let live: Vec<bool> = (0..40)
                .map(|k| cpu.mem.rd32(CTX + 0x1074 + 4 * k) != 0)
                .collect();
            let windows_left = kinds
                .iter()
                .zip(&live)
                .any(|(&kind, &l)| (5..=0xD).contains(&kind) && l);
            if wiped_port {
                assert!(
                    !windows_left,
                    "windows outlive the wipe: round {round} step {step}"
                );
                windows_wiped += 1;
            }
            // And the converse: a step that retired the windows and nothing
            // else (no handle-table reset, `FUN_801D99BC`) is the wipe.
            let others_live = kinds
                .iter()
                .zip(&live)
                .all(|(&kind, &l)| (5..=0xD).contains(&kind) || l);
            if windows_before && !windows_left && others_live && shown_before {
                assert!(
                    wiped_port,
                    "round {round} step {step}: retail wiped, port kept"
                );
            }
            windows_before = windows_left;
            let retail_state = cpu.mem.rd8(CTX + 0x06);
            let retail_pool = cpu.mem.rd16(CTX + 0x6DC) as i16;
            let retail_count = cpu.mem.rd8(CTX + 0x19);
            let port_state = match s.phase {
                ArtsInputPhase::Entering => 0x50,
                ArtsInputPhase::Review => 0x5A,
                // `_DAT_800846C4` picks the ring over the attack prompt.
                ArtsInputPhase::Aborted if cpu.mem.rd32(SELECT_ATTACK) != 0 => 0x28,
                ArtsInputPhase::Aborted => 0x78,
                _ => 0xFF,
            };
            let context = format!(
                "round {round} step {step} press {press:#x} pool {pool} costs {costs:?} status {status:#x} before {before_count}"
            );
            assert_eq!(retail_state, port_state, "state: {context}");
            if retail_state != 0x50 && retail_state != 0x5A {
                // An empty cancel under the staging byte measures the string
                // still in the window into the count (`0x801D211C..
                // 0x801D2174`): the preseed the session still holds.
                let want = if staged { s.preseed.len() } else { 0 };
                assert_eq!(usize::from(retail_count), want, "cancel count: {context}");
                if want != 0 {
                    kept += 1;
                } else if staged {
                    wiped += 1;
                }
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
                // A bare confirm on the preseed charges nothing.
                if before_count == 0 && press == CONFIRM && !preseed.is_empty() {
                    assert!(s.replay, "{context}");
                    assert_eq!(i32::from(retail_pool), i32::from(pool), "{context}");
                    replays += 1;
                }
                break;
            }
        }
    }
    assert!(
        replays > 300 && kept > 100 && wiped > 30 && windows_wiped > 100,
        "{replays} {kept} {wiped} {windows_wiped}"
    );
}
