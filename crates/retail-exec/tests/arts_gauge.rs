//! The Arts entry: `FUN_801D0748` state `0x50` with `FUN_801D388C` cases `0xB`,
//! `0xC`, `0x26` and `0x2F`, against `ArtsCommandInputSession`. A generated pad
//! script runs on both sides: every press, the debit, the refusal, the auto end,
//! the confirm and cancel forks, the Rot limb gates and the pool's boundaries.

use crate::common::{Rng, disc, scus_cpu};
use legaia_art::queue::Command;
use legaia_engine_battle::arts_command_input::{
    ArtsCommandInputSession, ArtsCommandPad, ArtsInputPhase, rot_blocks,
};
use legaia_engine_battle::target_picker::{PickerState, SlotState};
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
const LEFT_PAD: u32 = 0x8000;
const RIGHT_PAD: u32 = 0x2000;
const UP_PAD: u32 = 0x1000;
const DOWN_PAD: u32 = 0x4000;
const CROSS_ENTRY: u32 = 0x8008_46C8;
const SPAWN: u32 = 0x801D_8DE8;
const FRAME_STEP: u32 = 0x1F80_0393;
const PENNANT_HANDLES: u32 = 0x8017_0000;

fn frame(cpu: &mut Cpu, pad: u32) {
    cpu.mem.wr32(PAD, pad);
    assert_eq!(cpu.call(STATE_MACHINE, &[]), Stop::Returned, "pad {pad:#x}");
}

/// Lets the glide stepper `FUN_801D9BBC` run for real over the glide table
/// `ctx[+0x11B4]` and has the element spawner register what retail's
/// `FUN_801DB7B0` would for a spawn under the glide flag `ctx[+0x1B] = 0`:
/// a committed pennant (case `0xB`, `0x801D3D3C..0x801D3D48`, over the count
/// `ctx[+0x1C] = 0x18`) and the pennants' retreat (cases `0x26` / `0xF`,
/// `0x801D52E4..0x801D5340`). A flag-0 record does not gate the pass, but
/// while one glides the entry's and the review's cancels are not read (`s4`,
/// the `0x801D0C64` scan, at `0x801D20E4` / `0x801D2304`). The other spawns
/// (flag `1`) are the chips' and the window's, which the stepper's gate and
/// the port's own pad hold cover elsewhere.
fn register_pennant_glides(cpu: &mut Cpu) {
    cpu.mem.wr8(FRAME_STEP, 1);
    cpu.hook(SPAWN, |r, m| {
        let ctx = m.rd32(CTX_PTR);
        // A mode with bit 1 set spawns on its seat with no glide record
        // (`andi v0,s5,0x2` / `bne` at `0x801D93A8..0x801D93B0`): case
        // `0xD`'s re-spawns on a cursor step.
        if m.rd8(ctx + 0x1B) != 0 || r.r[psx_retail_exec::A1] & 2 != 0 {
            return;
        }
        // The spawner takes the first free handle (`0x801D91D4..0x801D9260`).
        let Some(slot) = (0..0x28).find(|&k| m.rd32(ctx + 0x1074 + 4 * k) == 0) else {
            return;
        };
        let rec = ctx + 0x11B4 + 0xC * slot;
        m.load(rec, &[0u8; 0xC]);
        let total = m.rd8(ctx + 0x1C);
        m.wr8(rec, total);
        m.wr8(rec + 2, slot as u8);
        m.wr32(ctx + 0x1074 + 4 * slot, PENNANT_HANDLES + 0x10 * slot);
    });
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
        0x801D_B8F4,    // the chip draw
        0x801D_BDDC,    // the Rot marks
        0x801D_8A88,
        0x801D_A6B4,
        0x801D_3748,
        0x801D_5854, // the camera
        0x801D_5718,
        0x8003_5F04,
        0x801D_BB8C, // the review's re-entry leaf
    ] {
        cpu.stub(stub, 0);
    }
    register_pennant_glides(&mut cpu);
    let mut rng = Rng(0x4152_5453);
    let dead = SlotState::default();
    let (mut ends, mut refused, mut restarts, mut held, mut backed) = (0, 0, 0, 0, 0);
    let (mut stepped, mut committed) = (0u32, 0u32);
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
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        // The round reset's glide flag and count (`FUN_801D88CC`,
        // `0x801D88FC..0x801D8914`: `ctx[+0x1B] = 1`, `ctx[+0x1C] = 0x10`).
        cpu.mem.wr8(CTX + 0x1B, 1);
        cpu.mem.wr8(CTX + 0x1C, 0x10);
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
            // Now and then the player waits for the pennants to land.
            if rng.below(4) == 0 {
                for _ in 0..rng.below(30) {
                    frame(&mut cpu, 0);
                    s.input(ArtsCommandPad::default(), [dead; 3], [dead; 5]);
                }
            }
            let gliding = !s.pennant_glides.is_empty();
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
                // The target cursor the entry ends on (`0x5A`,
                // `0x801D21CC..0x801D23EC`): Right, then Left step the ring
                // (`FUN_801D8D00`, run for real over the queue `ctx[+0x244]`)
                // and stay; Up or Down commit under the directional entry
                // and the confirm commits, on the cursor's seat `+0x1DD`;
                // the cancel goes back to the entry through case `0xF`
                // (`0x801D23C8..0x801D23DC`, with `ctx[+0x266 + seat]` clear
                // and the option word not `1`), wiping the string. The port
                // runs the same presses from the member's current target.
                use legaia_engine_battle_vm::battle_action::{
                    PoolActor, bearing_12bit_approx, build_attack_target_queue,
                };
                let count = 1 + rng.below(4) as usize;
                let seats: Vec<(i16, i16)> = (0..count)
                    .map(|_| {
                        (
                            rng.below(0x1000) as i16 - 0x800,
                            rng.below(0x1000) as i16 - 0x800,
                        )
                    })
                    .collect();
                let me = (
                    rng.below(0x1000) as i16 - 0x800,
                    rng.below(0x1000) as i16 - 0x800,
                );
                let cur = rng.below(count as u32) as u8;
                let mut pool = [PoolActor::default(); 8];
                pool[0] = PoolActor {
                    alive: true,
                    x: me.0,
                    z: me.1,
                    target_slot: 3 + cur,
                    status: 0,
                };
                for (k, p) in seats.iter().enumerate() {
                    pool[3 + k] = PoolActor {
                        alive: true,
                        x: p.0,
                        z: p.1,
                        target_slot: 0,
                        status: 0,
                    };
                }
                let q = build_attack_target_queue(&pool, 0, bearing_12bit_approx);
                for (k, b) in [
                    q.count,
                    q.wrap_slot,
                    q.ordered[0],
                    q.ordered[1],
                    q.ordered[2],
                ]
                .iter()
                .enumerate()
                {
                    cpu.mem.wr8(CTX + 0x244 + k as u32, *b);
                }
                cpu.mem.wr8(ACTOR + 0x1DD, 3 + cur);
                let cross_entry = rng.below(2) == 1;
                cpu.mem.wr32(CROSS_ENTRY, u32::from(cross_entry));
                s.cross_entry = cross_entry;
                s.target_seat = Some(cur);
                let mut foe = [dead; 5];
                for (k, p) in seats.iter().enumerate() {
                    foe[k] = SlotState::alive(true, true).with_seat(p.0, p.1);
                }
                let mut me_row = [dead; 3];
                me_row[0] = SlotState::alive(true, true).with_seat(me.0, me.1);
                let mut hist: Vec<u32> = Vec::new();
                for sub in 0..4 {
                    let press = [LEFT_PAD, RIGHT_PAD, CANCEL, 0, UP_PAD, DOWN_PAD, CONFIRM]
                        [rng.below(7) as usize];
                    frame(&mut cpu, press);
                    let ev = ArtsCommandPad {
                        left: press == LEFT_PAD,
                        right: press == RIGHT_PAD,
                        up: press == UP_PAD,
                        down: press == DOWN_PAD,
                        cross: press == CONFIRM,
                        circle: press == CANCEL,
                        ..ArtsCommandPad::default()
                    };
                    s.input(ev, me_row, foe);
                    let state = cpu.mem.rd8(CTX + 0x06);
                    let target = cpu.mem.rd8(ACTOR + 0x1DD);
                    let table = cpu.mem.read(CTX + 0x11B4, 0x28 * 0xC);
                    let live: Vec<(usize, u8, u8, u8)> = (0..0x28)
                        .filter(|&k| table[0xC * k] != 0)
                        .map(|k| (k, table[0xC * k], table[0xC * k + 1], table[0xC * k + 3]))
                        .collect();
                    let label = format!(
                        "cursor {sub} press {press:#x} cross-entry {cross_entry} foes {count} cur {cur} glides {live:?} held {} hist {hist:x?}: {context}",
                        s.cancel_held()
                    );
                    hist.push(press);
                    match &s.phase {
                        ArtsInputPhase::Entering => {
                            assert_eq!((state, cpu.mem.rd8(CTX + 0x19)), (0x50, 0), "{label}");
                            backed += 1;
                            break;
                        }
                        ArtsInputPhase::Review => {
                            assert_eq!((state, target), (0x5A, 3 + cur), "{label}");
                        }
                        ArtsInputPhase::Targeting { picker } => {
                            let PickerState::Cursor { slot, .. } = picker.state() else {
                                panic!("{label}");
                            };
                            assert_eq!((state, target), (0x5A, 3 + slot), "{label}");
                            stepped += u32::from(slot != cur);
                        }
                        ArtsInputPhase::Confirmed { target_slot, .. } => {
                            assert!(matches!(state, 0x28 | 0x6E), "{label}: retail {state:#x}");
                            assert_eq!(target, 3 + target_slot, "{label}");
                            committed += 1;
                            break;
                        }
                        other => panic!("port {other:?} retail {state:#x}: {label}"),
                    }
                }
                break;
            }
            if retail_count == before_count && press & 0xF000 != 0 {
                refused += 1;
            }
            if press == CANCEL && before_count != 0 {
                if gliding {
                    held += 1;
                } else {
                    restarts += 1;
                }
            }
        }
    }
    assert!(
        ends > 300 && refused > 100 && restarts > 30 && held > 50 && backed > 10,
        "{ends} {refused} {restarts} {held} {backed}"
    );
    assert!(
        stepped > 30 && committed > 50,
        "stepped {stepped} committed {committed}"
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
        0x801D_B8F4,    // the chip draw
        0x801D_BDDC,    // the Rot marks
        0x801D_8A88,
        0x801D_A6B4,
        0x801D_3748,
        0x801D_5854, // the camera
        0x801D_5718,
        0x8003_5F04,
    ] {
        cpu.stub(stub, 0);
    }
    register_pennant_glides(&mut cpu);
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
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        // The round reset's glide flag and count (`FUN_801D88CC`,
        // `0x801D88FC..0x801D8914`: `ctx[+0x1B] = 1`, `ctx[+0x1C] = 0x10`).
        cpu.mem.wr8(CTX + 0x1B, 1);
        cpu.mem.wr8(CTX + 0x1C, 0x10);
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
            // A slot the wipe freed may hold a pennant's handle since.
            let live: Vec<bool> = (0..40)
                .map(|k| cpu.mem.rd32(CTX + 0x1074 + 4 * k) == HANDLES + 0x10 * k)
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
