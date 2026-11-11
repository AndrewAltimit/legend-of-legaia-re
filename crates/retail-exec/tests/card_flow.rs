//! The save screen's per-frame card flow (PROT 0899) against `engine-menus`:
//! the two-op I/O machine `FUN_801E3294` against `CardIoMachine::tick`, the
//! poll fold `FUN_801E16E0` against `CardHealth::fold_poll`, the directory
//! classifier `FUN_801E1208` against `classify_card_directory`, and the
//! ticker `FUN_801E1114` that sequences them against `card_frame_tick`.
//!
//! On `card_bu`'s rig: the BIOS calls and the three libcard entry points
//! (`FUN_8006EE14` / `EE24` / `EE34`) are hooked, logged and scripted.
//!
//! The I/O machine's Ready arm also zeroes the fault counter `_DAT_801F0218`,
//! which the port keeps in `CardHealth`; the test holds that store apart.

use crate::card_bu::{Bios, Call, HANDLE_A, HANDLE_B, flags, pend, rig, tests_of};
use crate::common::Rng;
use legaia_engine_menus::card_flow::{
    CardHealth, CardPollResult, CardWriteEvent, CardWriteIo, CardWriteMachine, CardWriteState,
};
use legaia_engine_menus::save_select::{
    CARD_DIR_FRAMES, CardDirEntry, CardIoEffect, CardIoMachine, CardStatus, SlotContent,
    card_frame_tick, card_status_poll, classify_card_directory,
};
use psx_retail_exec::{A0, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const IO_MACHINE: u32 = 0x801E_3294;
const POLL_FOLD: u32 = 0x801E_16E0;
const CLASSIFY: u32 = 0x801E_1208;
const TICKER: u32 = 0x801E_1114;
const WRITE_SM: u32 = 0x801E_13B8;
const STEP: u32 = 0x801E_380C;

const STATE: u32 = 0x801E_F188;
const RETRY: u32 = 0x801E_4FC4;
const BOTH_ACKED: u32 = 0x801E_ED20;
const PENDING: u32 = 0x801E_F184;
const RESULT: u32 = 0x801E_F180;
const NOT_CARD: u32 = 0x801F_0214;
const LAST_RESULT: u32 = 0x801F_3808;
const ABORTED: u32 = 0x801E_F0FC;
const POLL_COUNTER: u32 = 0x801E_F17C;
const PAD_A: u32 = 0x8007_B850;
const PAD_B: u32 = 0x8007_B874;
const ERROR_FLAG: u32 = 0x801E_F10C;

const SM_WORD: u32 = 0x801F_329C;
const FRAME_RESULT: u32 = 0x801F_3804;
const LATCHED_RESULT: u32 = 0x801F_3800;
const FAULT_COUNT: u32 = 0x801F_0218;
const CHANGE_COUNT: u32 = 0x801F_01BC;
const CARD_PHASE: u32 = 0x801F_021C;
const RESCAN: u32 = 0x801F_0224;
const UNFORMATTED: u32 = 0x801E_F100;
const STALE: u32 = 0x801E_F168;
const DIR_COUNT: u32 = 0x801F_01EC;
const FREE_BLOCKS: u32 = 0x801F_01F0;
const SAVE_COUNT: u32 = 0x801F_01E4;
const CHANNEL: u32 = 0x801E_F18C;

const DIR_TABLE: u32 = 0x801F_32A8;
const CLASSES: u32 = 0x801F_2A48;
const SCANNED: u32 = 0x801F_2A68;
const NAMES: u32 = 0x801F_2A88;

type Lib = Rc<RefCell<Vec<(u32, u32)>>>;

fn lib_hooks(cpu: &mut Cpu) -> Lib {
    let lib: Lib = Rc::default();
    for entry in [0x8006_EE14u32, 0x8006_EE24, 0x8006_EE34] {
        let l = lib.clone();
        cpu.hook(entry, move |r, _| {
            l.borrow_mut().push((entry, r.r[A0]));
            r.r[V0] = 0;
        });
    }
    // BIOS `A(0x18)` `strncmp` and `A(0x19)` `strcpy`.
    cpu.hook(0x8005_6748, |r, m| {
        let mut out = 0i32;
        for i in 0..r.r[psx_retail_exec::A2] {
            let (a, b) = (m.rd8(r.r[A0] + i), m.rd8(r.r[psx_retail_exec::A1] + i));
            if a != b {
                out = i32::from(a) - i32::from(b);
                break;
            }
            if a == 0 {
                break;
            }
        }
        r.r[V0] = out as u32;
    });
    cpu.hook(0x8005_6758, |r, m| {
        let s = crate::menu_painters::cstr(m, r.r[psx_retail_exec::A1]);
        for (i, c) in s.iter().chain(&[0]).enumerate() {
            m.wr8(r.r[A0] + i as u32, *c);
        }
        r.r[V0] = r.r[A0];
    });
    lib
}

fn machine_words(cpu: &mut Cpu) -> [i32; 7] {
    [
        STATE,
        RETRY,
        BOTH_ACKED,
        PENDING,
        RESULT,
        NOT_CARD,
        LAST_RESULT,
    ]
    .map(|a| cpu.mem.rd32(a) as i32)
}

fn seat_machine(cpu: &mut Cpu, w: [i32; 7]) {
    for (a, v) in [
        STATE,
        RETRY,
        BOTH_ACKED,
        PENDING,
        RESULT,
        NOT_CARD,
        LAST_RESULT,
    ]
    .into_iter()
    .zip(w)
    {
        cpu.mem.wr32(a, v as u32);
    }
}

fn status_of(e: [bool; 4], counter: &mut u16) -> CardStatus {
    card_status_poll(e, counter)
}

/// The calls one frame of the machine makes, for the effect the port names.
fn frame_calls(
    state: i32,
    status: CardStatus,
    effect: Option<CardIoEffect>,
    chan: u32,
    b_handle: u32,
) -> (Vec<Call>, Vec<(u32, u32)>) {
    let mut calls = Vec::new();
    let mut lib = Vec::new();
    match state {
        0 => {
            assert_eq!(effect, Some(CardIoEffect::StartOp));
            lib.push((0x8006_EE14, chan));
        }
        1 | 3 => {
            calls.extend(tests_of(HANDLE_A, 4));
            if state == 1 && status == CardStatus::Complete {
                // The finalize pair, whichever way the hardware word falls.
                assert!(
                    matches!(
                        effect,
                        Some(CardIoEffect::Finalize | CardIoEffect::PadClear)
                    ),
                    "{effect:?}"
                );
                calls.extend(tests_of(HANDLE_B, 4));
                lib.push((0x8006_EE34, chan));
                calls.extend(tests_of(HANDLE_B, b_handle + 1));
            } else {
                assert_eq!(effect, None);
            }
        }
        2 => {
            assert_eq!(effect, Some(CardIoEffect::SecondOp));
            calls.extend(tests_of(HANDLE_A, 4));
            lib.push((0x8006_EE24, chan));
        }
        _ => assert_eq!(effect, None),
    }
    (calls, lib)
}

#[test]
fn the_io_machine_steps_as_retail_steps() {
    differential!(prot(899, IO_MACHINE));
    let Some((mut cpu, bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let lib = lib_hooks(&mut cpu);
    let mut rng = Rng(0x494F_4D43);
    let mut published = std::collections::BTreeMap::<i32, u32>::new();
    let (mut finalized, mut pad_cleared) = (0, 0);
    for walk in 0..600 {
        // A walk from a seated machine, the events drawn each frame.
        let start = [
            rng.below(5) as i32,
            rng.below(6) as i32,
            rng.below(2) as i32,
            [0, 1, 2, -1, -2, -3][rng.below(6) as usize],
            [0, 1, 2, -1, -3][rng.below(5) as usize],
            match rng.below(4) {
                0 => -1 - rng.below(3) as i32,
                _ => rng.below(4) as i32,
            },
            [0, 1, 2, -1, -1, -2, -3][rng.below(7) as usize],
        ];
        seat_machine(&mut cpu, start);
        let mut port = CardIoMachine::from_retail_words(start);
        let mut counter = rng.below(0x80) as u16;
        cpu.mem.wr32(POLL_COUNTER, u32::from(counter));
        cpu.mem.wr32(ABORTED, 0);
        let (port_no, unit) = (rng.below(2), rng.below(2));
        let chan = (port_no << 4) + unit;
        // Mostly quiet frames, so a wait runs into the backstop too.
        let quiet = rng.below(3) == 0;
        for frame in 0..40 {
            let e = if quiet || rng.below(3) != 0 {
                [false; 4]
            } else {
                flags(&mut rng)
            };
            let b_handle = rng.below(4);
            *bios.borrow_mut() = Bios {
                pending: pend(HANDLE_A, e),
                // After the poll and the array-B drain, one B handle answers
                // the finalize wait.
                late: Some((8, vec![HANDLE_B + b_handle])),
                ..Default::default()
            };
            lib.borrow_mut().clear();
            for a in [PAD_A, PAD_B] {
                cpu.mem.wr32(a, 0x5A5A);
            }
            cpu.mem.wr32(ERROR_FLAG, 0);
            cpu.mem.wr32(FAULT_COUNT, 9);
            let before = machine_words(&mut cpu);
            let what = format!("walk {walk} frame {frame} from {before:?} events {e:?}");
            assert_eq!(
                cpu.call(IO_MACHINE, &[port_no, unit]),
                Stop::Returned,
                "{what}"
            );

            let state = before[0];
            let status = if state == 1 || state == 3 {
                status_of(e, &mut counter)
            } else {
                CardStatus::Pending
            };
            let (result, effect) = port.tick(status, &mut counter);
            assert_eq!(cpu.regs.r[V0] as i32, result, "{what}: {status:?}");
            assert_eq!(
                machine_words(&mut cpu),
                port.retail_words(),
                "{what}: {status:?}"
            );
            assert_eq!(cpu.mem.rd32(POLL_COUNTER), u32::from(counter), "{what}");
            assert_eq!(cpu.mem.rd32(ABORTED), port.aborted_publishes, "{what}");
            let (calls, libs) = frame_calls(state, status, effect, chan, b_handle);
            assert_eq!(bios.borrow().log, calls, "{what}: {status:?} {effect:?}");
            assert_eq!(*lib.borrow(), libs, "{what}: {status:?} {effect:?}");
            let cleared = cpu.mem.rd32(PAD_A) == 0 && cpu.mem.rd32(PAD_B) == 0;
            assert_eq!(cleared, effect == Some(CardIoEffect::PadClear), "{what}");
            // The hardware error flag: a finalize that found a negative word
            // other than `-1`.
            assert_eq!(
                cpu.mem.rd32(ERROR_FLAG) != 0,
                effect == Some(CardIoEffect::PadClear) && before[6] < 0,
                "{what}"
            );
            // The Ready arm of the first wait is the one store outside the
            // machine's own words.
            let ready = state == 1 && status == CardStatus::Ready;
            assert_eq!(
                cpu.mem.rd32(FAULT_COUNT),
                if ready { 0 } else { 9 },
                "{what}"
            );
            finalized += u32::from(effect == Some(CardIoEffect::Finalize));
            pad_cleared += u32::from(effect == Some(CardIoEffect::PadClear));
            if state == 4 {
                *published.entry(result).or_default() += 1;
            }
        }
    }
    for want in [-3, -2, -1, 0, 1, 2] {
        assert!(
            published.get(&want).copied().unwrap_or(0) > 10,
            "result {want} published: {published:?}"
        );
    }
    assert!(
        finalized > 20 && pad_cleared > 20,
        "{finalized} {pad_cleared}"
    );
}

fn health_words(cpu: &mut Cpu) -> (i32, i32, u32, bool, bool, bool, bool) {
    (
        cpu.mem.rd32(FAULT_COUNT) as i32,
        cpu.mem.rd32(CHANGE_COUNT) as i32,
        cpu.mem.rd32(CARD_PHASE),
        cpu.mem.rd32(RESCAN) != 0,
        cpu.mem.rd32(UNFORMATTED) != 0,
        cpu.mem.rd32(STALE) == u32::MAX,
        cpu.mem.rd32(DIR_COUNT) != 0
            && cpu.mem.rd32(FREE_BLOCKS) != 0
            && cpu.mem.rd32(SAVE_COUNT) != 0,
    )
}

fn seat_health(cpu: &mut Cpu, h: &CardHealth) {
    cpu.mem.wr32(FAULT_COUNT, h.fault_count as u32);
    cpu.mem.wr32(CHANGE_COUNT, h.change_count as u32);
    cpu.mem.wr32(CARD_PHASE, h.phase);
    cpu.mem.wr32(RESCAN, u32::from(h.rescan_requested));
    cpu.mem.wr32(UNFORMATTED, u32::from(h.unformatted));
    cpu.mem
        .wr32(STALE, if h.directory_stale { u32::MAX } else { 0 });
    for a in [DIR_COUNT, FREE_BLOCKS, SAVE_COUNT] {
        cpu.mem.wr32(a, u32::from(h.directory_cache_valid) * 3);
    }
}

fn health_of(h: &CardHealth) -> (i32, i32, u32, bool, bool, bool, bool) {
    (
        h.fault_count,
        h.change_count,
        h.phase,
        h.rescan_requested,
        h.unformatted,
        h.directory_stale,
        h.directory_cache_valid,
    )
}

#[test]
fn the_poll_fold_counts_as_retail_counts() {
    differential!(prot(899, POLL_FOLD));
    let Some((mut cpu, _bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x464F_4C44);
    let mut seen = std::collections::BTreeMap::<i32, u32>::new();
    let (mut busy_runs, mut capped) = (0, 0);
    for run in 0..8_000 {
        let count = |rng: &mut Rng| match rng.below(6) {
            0 => 0x3FE + rng.below(5) as i32,
            1 => -1 - rng.below(3) as i32,
            _ => rng.below(4) as i32,
        };
        let mut h = CardHealth {
            fault_count: count(&mut rng),
            change_count: count(&mut rng),
            phase: [0, 3][rng.below(2) as usize],
            rescan_requested: rng.below(2) == 0,
            unformatted: rng.below(4) == 0,
            directory_stale: rng.below(4) == 0,
            directory_cache_valid: rng.below(4) != 0,
        };
        seat_health(&mut cpu, &h);
        let code = match rng.below(8) {
            0 => [-4i32, 3, 100, -100][rng.below(4) as usize],
            _ => rng.below(6) as i32 - 3,
        };
        cpu.mem.wr32(LATCHED_RESULT, code as u32);
        // The first switch only picks a debug string; any word will do.
        cpu.mem.wr32(FRAME_RESULT, rng.below(8).wrapping_sub(4));
        let sm = rng.below(8);
        cpu.mem.wr32(SM_WORD, sm);
        let what = format!("run {run}: code {code} sm {sm} from {h:?}");
        assert_eq!(cpu.call(POLL_FOLD, &[1]), Stop::Returned, "{what}");
        h.fold_poll(CardPollResult::from_code(code), sm > 2);
        assert_eq!(health_words(&mut cpu), health_of(&h), "{what}");
        *seen.entry(code.clamp(-4, 3)).or_default() += 1;
        busy_runs += u32::from(sm > 2);
        capped += u32::from(h.fault_count == 0x400 || h.change_count == 0x400);
    }
    assert!(
        seen.values().all(|&n| n > 100) && seen.len() == 8,
        "{seen:?}"
    );
    assert!(busy_runs > 1000 && capped > 200, "{busy_runs} {capped}");
}

fn put_table(cpu: &mut Cpu, files: &[CardDirEntry]) {
    for i in 0..CARD_DIR_FRAMES {
        let at = DIR_TABLE + (i * 0x28) as u32;
        let e = files.get(i).cloned().unwrap_or_default();
        for (k, c) in e.name.iter().enumerate() {
            cpu.mem.wr8(at + k as u32, *c);
        }
        cpu.mem.wr32(at + 0x18, e.size);
    }
}

fn files(rng: &mut Rng) -> Vec<CardDirEntry> {
    let n = rng.below(CARD_DIR_FRAMES as u32 + 1) as usize;
    (0..n)
        .map(|_| {
            let mut e = CardDirEntry::default();
            let prefix = match rng.below(6) {
                0 => "BISCPS-10059PRO-",
                1 => "BASLUS-00000XXX-",
                2 => "BASCUS-94254PRO_",
                _ => "BASCUS-94254PRO-",
            };
            // Mostly a two-digit save number; at times one digit, or a
            // number past the grid, which the class array has no cell for.
            let tail = match rng.below(8) {
                0 => format!("{}", rng.below(10)),
                1 => format!("{}x", rng.below(10)),
                2 => format!("{:02}", 16 + rng.below(4)),
                _ => format!("{:02}", rng.below(16)),
            };
            let s = format!("{prefix}{tail}");
            e.name[..s.len()].copy_from_slice(s.as_bytes());
            e.size = 0x2000;
            e
        })
        .collect()
}

fn class_raw(c: SlotContent) -> u8 {
    match c {
        SlotContent::Foreign => 0,
        SlotContent::LegaiaSave => 1,
        SlotContent::Free => 2,
    }
}

#[test]
fn the_directory_classes_are_the_ones_retail_stamps() {
    differential!(prot(899, CLASSIFY));
    let Some((mut cpu, _bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let _lib = lib_hooks(&mut cpu);
    let mut rng = Rng(0x434C_4153);
    let mut seen = [0u32; 3];
    for run in 0..4_000 {
        let files = files(&mut rng);
        put_table(&mut cpu, &files);
        for i in 0..0x20 {
            cpu.mem.wr8(CLASSES + i, rng.next() as u8);
            cpu.mem.wr8(SCANNED + i, rng.next() as u8);
        }
        let names: Vec<&[u8]> = files.iter().map(|e| e.name.as_slice()).collect();
        // The budget a card can report: its fifteen blocks less its files.
        // Retail's spend loop has no cell bound, only the budget's.
        let avail = match rng.below(5) {
            0 => -(rng.below(3) as i32),
            _ => rng.below(CARD_DIR_FRAMES as u32 - files.len() as u32 + 1) as i32,
        };
        cpu.mem.wr32(FREE_BLOCKS, avail as u32);
        assert_eq!(cpu.call(CLASSIFY, &[0, files.len() as u32]), Stop::Returned);
        let port = classify_card_directory(&names, avail.max(0) as u32);
        let got: Vec<u8> = (0..16).map(|i| cpu.mem.rd8(CLASSES + i)).collect();
        let want: Vec<u8> = port.iter().map(|&c| class_raw(c)).collect();
        assert_eq!(got, want, "run {run}: avail {avail} files {names:?}");
        for c in &want {
            seen[usize::from(*c)] += 1;
        }
        // The count returned is the files that matched a prefix.
        let matched = names
            .iter()
            .filter(|n| legaia_engine_menus::save_select::card_dir_slot_of(n).is_some())
            .count();
        assert_eq!(cpu.regs.r[V0] as usize, matched, "run {run}");
        // Each matched file's name is copied to its slot's name row.
        for n in &names {
            if let Some(slot) = legaia_engine_menus::save_select::card_dir_slot_of(n)
                && slot < 16
            {
                let row = crate::menu_painters::cstr(&mut cpu.mem, NAMES + (slot as u32) * 0x40);
                let last = names
                    .iter()
                    .rfind(|m| legaia_engine_menus::save_select::card_dir_slot_of(m) == Some(slot))
                    .unwrap();
                let end = last.iter().position(|&c| c == 0).unwrap_or(last.len());
                assert_eq!(row, &last[..end], "run {run}: slot {slot}");
            }
        }
    }
    assert!(seen.iter().all(|&n| n > 2000), "{seen:?}");
}

#[test]
fn the_ticker_sequences_the_frame_as_retail_does() {
    differential!(prot(899, TICKER));
    let Some((mut cpu, bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let _lib = lib_hooks(&mut cpu);
    // The write machine and the transfer step have their own differentials.
    cpu.stub(WRITE_SM, 0);
    cpu.stub(STEP, 0);
    let mut rng = Rng(0x5449_434B);
    let (mut rebuilds, mut gated) = (0, 0);
    for walk in 0..300 {
        let start = [
            rng.below(5) as i32,
            rng.below(6) as i32,
            rng.below(2) as i32,
            0,
            0,
            0,
            0,
        ];
        seat_machine(&mut cpu, start);
        let mut io = CardIoMachine::from_retail_words(start);
        let mut health = CardHealth::at_init();
        seat_health(&mut cpu, &health);
        cpu.mem.wr32(LATCHED_RESULT, 0);
        let mut latched = 0i32;
        let mut counter = rng.below(0x80) as u16;
        cpu.mem.wr32(POLL_COUNTER, u32::from(counter));
        let port_no = rng.below(2);
        cpu.mem.wr32(CHANNEL, port_no);
        for frame in 0..30 {
            let e = if rng.below(2) == 0 {
                [false; 4]
            } else {
                flags(&mut rng)
            };
            let listing = files(&mut rng);
            *bios.borrow_mut() = Bios {
                pending: pend(HANDLE_A, e),
                late: Some((8, vec![HANDLE_B])),
                files: listing.clone(),
                ..Default::default()
            };
            // A no-card run raises the request; raise it here as well so
            // the rebuild is reached often.
            if rng.below(3) == 0 {
                health.rescan_requested = true;
                cpu.mem.wr32(RESCAN, 1);
            }
            let sm = [0u32, 1, 2, 2, 3, 5][rng.below(6) as usize];
            cpu.mem.wr32(SM_WORD, sm);
            for i in 0..16 {
                cpu.mem.wr8(CLASSES + i, 0xEE);
            }
            let state = cpu.mem.rd32(STATE) as i32;
            let token = rng.next();
            let what = format!("walk {walk} frame {frame} state {state} sm {sm} events {e:?}");
            assert_eq!(cpu.call(TICKER, &[token]), Stop::Returned, "{what}");
            assert_eq!(cpu.regs.r[V0], token, "{what}: the argument is handed back");

            // The port's frame, wired the way the routine wires it: the
            // machine's result latches when non-zero, the fold reads the
            // latch, and the rebuild keys on the phase the fold leaves.
            let gate = sm < 3;
            gated += u32::from(!gate);
            let status = if gate && (state == 1 || state == 3) {
                status_of(e, &mut counter)
            } else {
                CardStatus::Pending
            };
            let probe = if gate {
                let mut p = io.clone();
                let mut c = counter;
                p.tick(status, &mut c).0
            } else {
                0
            };
            if probe != 0 {
                latched = probe;
            }
            // The machine's Ready arm zeroes the fault counter before the
            // fold reads it.
            if state == 1 && status == CardStatus::Ready {
                health.fault_count = 0;
            }
            health.fold_poll(CardPollResult::from_code(latched), !gate);
            let mut request = health.rescan_requested;
            let (result, _, rebuilt) = card_frame_tick(
                &mut io,
                status,
                &mut counter,
                gate,
                health.phase,
                &mut request,
                &listing,
            );
            assert_eq!(result, if gate { probe } else { io.result() }, "{what}");
            assert_eq!(machine_words(&mut cpu), io.retail_words(), "{what}");
            assert_eq!(cpu.mem.rd32(LATCHED_RESULT) as i32, latched, "{what}");
            match rebuilt {
                Some(slots) => {
                    rebuilds += 1;
                    health.rescan_requested = request;
                    assert!(!request, "{what}: the request is spent");
                    health.directory_cache_valid = true;
                    let got: Vec<u8> = (0..15).map(|i| cpu.mem.rd8(CLASSES + i)).collect();
                    let want: Vec<u8> = slots.iter().map(|s| class_raw(s.content)).collect();
                    assert_eq!(got, want, "{what}: the rebuilt classes");
                    assert_eq!(cpu.mem.rd32(DIR_COUNT) as usize, listing.len(), "{what}");
                }
                None => assert_eq!(cpu.mem.rd8(CLASSES), 0xEE, "{what}: no rebuild"),
            }
            assert_eq!(cpu.mem.rd32(RESCAN) != 0, health.rescan_requested, "{what}");
            assert_eq!(cpu.mem.rd32(CARD_PHASE), health.phase, "{what}");
            assert_eq!(
                cpu.mem.rd32(FAULT_COUNT) as i32,
                health.fault_count,
                "{what}"
            );
            // The directory words are the rebuild's to refill.
            seat_health(&mut cpu, &health);
        }
    }
    assert!(rebuilds > 100 && gated > 1000, "{rebuilds} {gated}");
}

/// One call the write machine makes to the layer under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Io {
    OpenDir(u32),
    Free(u32, i32),
    Find(i32, u32),
    Compose,
    Write(u32, u32, u32, bool),
    Read(u32, u32, u32),
    Format(u32),
    Close(u32),
}

/// The scripted answers both sides read.
#[derive(Debug, Clone, Default)]
struct Script {
    dir: i32,
    free: i32,
    found: i32,
    write: i32,
    read: i32,
    formats: Vec<i32>,
}

struct Double<'a> {
    script: &'a Script,
    log: Vec<Io>,
    formats: usize,
    port: u32,
    name: u32,
    write_buf: u32,
    read_buf: u32,
    read_len: u32,
}

impl CardWriteIo for Double<'_> {
    fn open_directory(&mut self) -> i32 {
        self.log.push(Io::OpenDir(self.port));
        self.script.dir
    }
    fn free_blocks(&mut self, dir: i32) -> i32 {
        self.log.push(Io::Free(self.port, dir));
        self.script.free
    }
    fn find_file(&mut self, dir: i32) -> i32 {
        self.log.push(Io::Find(dir, self.name));
        self.script.found
    }
    fn write_block(&mut self, create: bool) -> i32 {
        self.log
            .push(Io::Write(self.port, self.write_buf, 0x2000, create));
        self.script.write
    }
    fn read_block(&mut self) -> i32 {
        self.log
            .push(Io::Read(self.port, self.read_buf, self.read_len));
        self.script.read
    }
    fn format(&mut self) -> i32 {
        self.log.push(Io::Format(self.port));
        let r = self.script.formats[self.formats];
        self.formats += 1;
        r
    }
    fn compose_block(&mut self) {
        self.log.push(Io::Compose);
    }
}

#[test]
fn the_write_machine_steps_as_retail_steps() {
    differential!(prot(899, WRITE_SM));
    let Some((mut cpu, _bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const DELAY: u32 = 0x801E_F128;
    const NAME: u32 = 0x801E_F024;
    const WRITE_BUF: u32 = 0x801E_F178;
    const READ_BUF: u32 = 0x801E_F174;
    const READ_LEN: u32 = 0x801F_01B8;
    const FD: u32 = 0x801E_F138;
    const WRITE_FAILED: u32 = 0x801E_F13C;
    const READ_FAILED: u32 = 0x801E_F140;
    const MENU_MODE: u32 = 0x801F_0204;
    const FORMAT_DONE: u32 = 0x801F_0220;
    const FRAME_DELTA: u32 = 0x1F80_0393;
    let script: Rc<RefCell<Script>> = Rc::default();
    let log: Rc<RefCell<Vec<Io>>> = Rc::default();
    let formats: Rc<RefCell<usize>> = Rc::default();
    let arg5 = |m: &mut psx_retail_exec::Memory, sp: u32, n: u32| m.rd32(sp + 0x10 + 4 * n);
    let (s, l) = (script.clone(), log.clone());
    cpu.hook(0x801E_3AF0, move |r, _| {
        assert_eq!(r.r[psx_retail_exec::A1], 0);
        l.borrow_mut().push(Io::OpenDir(r.r[A0]));
        r.r[V0] = s.borrow().dir as u32;
    });
    let (s, l) = (script.clone(), log.clone());
    cpu.hook(0x801E_3BA0, move |r, _| {
        l.borrow_mut()
            .push(Io::Free(r.r[A0], r.r[psx_retail_exec::A1] as i32));
        r.r[V0] = s.borrow().free as u32;
    });
    let (s, l) = (script.clone(), log.clone());
    cpu.hook(0x801E_3BEC, move |r, _| {
        l.borrow_mut()
            .push(Io::Find(r.r[A0] as i32, r.r[psx_retail_exec::A1]));
        r.r[V0] = s.borrow().found as u32;
    });
    let l = log.clone();
    cpu.hook(0x801E_1934, move |_, _| l.borrow_mut().push(Io::Compose));
    let (s, l) = (script.clone(), log.clone());
    cpu.hook(0x801E_3D68, move |r, m| {
        assert_eq!(
            (r.r[psx_retail_exec::A1], r.r[psx_retail_exec::A2]),
            (0, NAME)
        );
        let sp = r.r[psx_retail_exec::SP];
        l.borrow_mut().push(Io::Write(
            r.r[A0],
            r.r[psx_retail_exec::A3],
            arg5(m, sp, 0),
            arg5(m, sp, 1) != 0,
        ));
        r.r[V0] = s.borrow().write as u32;
    });
    let (s, l) = (script.clone(), log.clone());
    cpu.hook(0x801E_3C90, move |r, m| {
        assert_eq!(
            (r.r[psx_retail_exec::A1], r.r[psx_retail_exec::A2]),
            (0, NAME)
        );
        let sp = r.r[psx_retail_exec::SP];
        l.borrow_mut()
            .push(Io::Read(r.r[A0], r.r[psx_retail_exec::A3], arg5(m, sp, 0)));
        r.r[V0] = s.borrow().read as u32;
    });
    let (s, l, f) = (script.clone(), log.clone(), formats.clone());
    cpu.hook(0x801E_3E7C, move |r, _| {
        assert_eq!(r.r[psx_retail_exec::A1], 0);
        l.borrow_mut().push(Io::Format(r.r[A0]));
        let mut n = f.borrow_mut();
        r.r[V0] = s.borrow().formats[*n] as u32;
        *n += 1;
    });
    let l = log.clone();
    cpu.unhook(0x8005_66D8);
    cpu.hook(0x8005_66D8, move |r, _| {
        l.borrow_mut().push(Io::Close(r.r[A0]))
    });

    let mut rng = Rng(0x5752_534D);
    let mut events = std::collections::BTreeMap::<String, u32>::new();
    let mut retried = 0;
    for walk in 0..1_500 {
        let mut port = CardWriteMachine::new();
        let port_no = rng.below(2);
        cpu.mem.wr32(CHANNEL, port_no);
        cpu.mem.wr32(DELAY, 0);
        let (wbuf, rbuf) = (
            0x8015_0000 + rng.below(16) * 4,
            0x8016_0000 + rng.below(16) * 4,
        );
        let read_len = [0x80u32, 0x2000][rng.below(2) as usize];
        cpu.mem.wr32(WRITE_BUF, wbuf);
        cpu.mem.wr32(READ_BUF, rbuf);
        cpu.mem.wr32(READ_LEN, read_len);
        let mut state = [1u8, 2, 7, 5, 3, 0, 4, 6, 9][rng.below(9) as usize];
        for frame in 0..40 {
            port.set_state(CardWriteState::from_code(state));
            cpu.mem.wr32(SM_WORD, u32::from(state));
            let poll = match rng.below(8) {
                0 => [-4i32, 3, 50][rng.below(3) as usize],
                _ => rng.below(6) as i32 - 3,
            };
            cpu.mem.wr32(FRAME_RESULT, poll as u32);
            let delta = 1 + rng.below(6);
            cpu.mem.wr8(FRAME_DELTA, delta as u8);
            let draw = |rng: &mut Rng| match rng.below(3) {
                0 => 0,
                _ => 1 + rng.below(4) as i32,
            };
            let format = |rng: &mut Rng| [-3, -3, -3, -1, 1, 0, 2, -2][rng.below(8) as usize];
            *script.borrow_mut() = Script {
                dir: rng.below(16) as i32,
                free: draw(&mut rng),
                found: draw(&mut rng),
                write: [0, 0, -1][rng.below(3) as usize],
                read: [0, 0, -1][rng.below(3) as usize],
                formats: (0..5).map(|_| format(&mut rng)).collect(),
            };
            log.borrow_mut().clear();
            *formats.borrow_mut() = 0;
            let fd = 2 + rng.below(6);
            cpu.mem.wr32(FD, fd);
            for a in [WRITE_FAILED, READ_FAILED, FORMAT_DONE, FAULT_COUNT] {
                cpu.mem.wr32(a, 0);
            }
            cpu.mem.wr32(MENU_MODE, 0x55);
            cpu.mem.wr32(CHANGE_COUNT, 7);
            let what = format!("walk {walk} frame {frame} state {state} poll {poll} delta {delta}");
            assert_eq!(cpu.call(WRITE_SM, &[]), Stop::Returned, "{what}");

            let s = script.borrow().clone();
            let mut io = Double {
                script: &s,
                log: Vec::new(),
                formats: 0,
                port: port_no,
                name: NAME,
                write_buf: wbuf,
                read_buf: rbuf,
                read_len,
            };
            let event = port.tick(CardPollResult::from_code(poll), delta as i32, &mut io);
            // The close after a failed transfer is the routine's own call,
            // on the descriptor the layer below left.
            let retail: Vec<Io> = log.borrow().clone();
            let closes: Vec<Io> = retail
                .iter()
                .copied()
                .filter(|c| matches!(c, Io::Close(_)))
                .collect();
            let failed = matches!(
                event,
                Some(CardWriteEvent::WriteFailed | CardWriteEvent::ReadFailed)
            );
            assert_eq!(
                closes,
                if failed { vec![Io::Close(fd)] } else { vec![] },
                "{what}"
            );
            let calls: Vec<Io> = retail
                .into_iter()
                .filter(|c| !matches!(c, Io::Close(_)))
                .collect();
            assert_eq!(calls, io.log, "{what}: {event:?}");
            retried += u32::from(io.formats > 1);
            assert_eq!(
                cpu.mem.rd32(SM_WORD),
                u32::from(port.state().code()),
                "{what}: {event:?}"
            );
            assert_eq!(cpu.mem.rd32(DELAY) as i32, port.delay(), "{what}");
            let (mode, wf, rf, fault, done) = (
                cpu.mem.rd32(MENU_MODE),
                cpu.mem.rd32(WRITE_FAILED),
                cpu.mem.rd32(READ_FAILED),
                cpu.mem.rd32(FAULT_COUNT),
                cpu.mem.rd32(FORMAT_DONE),
            );
            let want = match event {
                Some(CardWriteEvent::Interrupted { mode, saving }) => {
                    (mode, u32::from(saving), u32::from(!saving), 1, 0)
                }
                Some(CardWriteEvent::WriteFailed) => (0x55, 1, 0, 0, 0),
                Some(CardWriteEvent::ReadFailed) => (0x55, 0, 1, 0, 0),
                Some(CardWriteEvent::FormatOk) => (0x55, 0, 0, 0, 3),
                Some(CardWriteEvent::FormatNoCard) => (0x55, 0, 0, 1, 0),
                _ => (0x55, 0, 0, 0, 0),
            };
            assert_eq!((mode, wf, rf, fault, done), want, "{what}: {event:?}");
            // The format arm also zeroes the change debounce; the port
            // keeps that counter in `CardHealth` and does not model it here.
            let formatted = state == 7;
            assert_eq!(
                cpu.mem.rd32(CHANGE_COUNT),
                if formatted { 0 } else { 7 },
                "{what}"
            );
            *events
                .entry(
                    format!("{event:?}")
                        .split([' ', '{'])
                        .next()
                        .unwrap()
                        .to_string(),
                )
                .or_default() += 1;
            state = match port.state().code() {
                0 | 4 | 6 | 9 => [1u8, 2, 7, 0, 4, 6, 9][rng.below(7) as usize],
                live => live,
            };
        }
    }
    for want in [
        "Some(Interrupted",
        "Some(WriteOk)",
        "Some(WriteFailed)",
        "Some(OutOfSpace)",
        "Some(ReadOk)",
        "Some(ReadFailed)",
        "Some(FormatOk)",
        "Some(FormatNoCard)",
        "Some(FormatFailed)",
        "None",
    ] {
        assert!(
            events.get(want).copied().unwrap_or(0) > 50,
            "{want}: {events:?}"
        );
    }
    assert!(retried > 200, "{retried}");
}
