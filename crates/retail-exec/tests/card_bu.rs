//! The menu overlay's memory-card device layer (PROT 0899) against
//! `engine-menus`' `card_bu_io` and the `save_select` card leaves.
//!
//! The routines are thin wrappers over BIOS `B()` calls - `TestEvent`,
//! `open`, `lseek`, `read`, `write`, `close`, `format`, `firstfile`,
//! `nextfile`, `erase` - which no interpreter run can service. Each stub is
//! hooked: it logs the call with its arguments and answers from a script,
//! the way `rand()` is scripted elsewhere. What is compared is everything the
//! routine itself decides: the order of the calls, the path `sprintf`
//! (run live) builds, the open flags, which handle wins, the value returned
//! and the globals left behind.
//!
//! The port describes a transfer as a `CardOp` for a host to service; the
//! BIOS calls a `CardOp` stands for are written out in [`calls_of`], off the
//! type's own documentation.

use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_menus::card_bu_io::{
    CARD_DIR_SLOTS, CARD_DIR_STRIDE, CardIoState, CardIssue, CardOp, CardPhase, CardStep, bu_path,
    drain_events_b, erase_file, format_card, format_card_result, open_flags, poll_events_a,
    wait_events_b,
};
use legaia_engine_menus::save_select::{
    CARD_DIR_FRAMES, CardDirEntry, CardStatus, card_directory_scan, card_events_drain,
    card_free_blocks, card_status_poll,
};
use psx_retail_exec::{A0, A1, A2, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const STATUS_POLL: u32 = 0x801E_3900;
const DRAIN_A: u32 = 0x801E_39A8;
const WAIT_B: u32 = 0x801E_3A00;
const DRAIN_B: u32 = 0x801E_3A98;
const POLL_A: u32 = 0x801E_435C;
const FORMAT: u32 = 0x801E_3E7C;
const ERASE: u32 = 0x801E_37CC;
const READ: u32 = 0x801E_3C90;
const WRITE: u32 = 0x801E_3D68;
const STEP: u32 = 0x801E_380C;
const FIND_NAME: u32 = 0x801E_3BEC;
const RESET: u32 = 0x801E_0598;
const DIR_SCAN: u32 = 0x801E_3AF0;
const FREE_BLOCKS: u32 = 0x801E_3BA0;

const TEST_EVENT: u32 = 0x8005_6658;
const OPEN: u32 = 0x8005_6698;
const LSEEK: u32 = 0x8005_66A8;
const BIOS_READ: u32 = 0x8005_66B8;
const BIOS_WRITE: u32 = 0x8005_66C8;
const CLOSE: u32 = 0x8005_66D8;
const BIOS_FORMAT: u32 = 0x8005_66E8;
const FIRSTFILE: u32 = 0x8005_66F8;
const NEXTFILE: u32 = 0x8005_6708;
const BIOS_ERASE: u32 = 0x8005_6718;
const STRCMP: u32 = 0x8005_6738;
const PRINTF: u32 = 0x8005_67A8;

/// The two event-handle arrays, and the handles the test plants in them.
const ARRAY_A: u32 = 0x8007_B9F0;
const ARRAY_B: u32 = 0x8007_BA04;
const HANDLE_A: u32 = 0xF100_0000;
const HANDLE_B: u32 = 0xF200_0000;

const POLL_COUNTER: u32 = 0x801E_F17C;
const PHASE: u32 = 0x801F_329C;
const FD: u32 = 0x801E_F138;
const WRITE_FAILED: u32 = 0x801E_F13C;
const READ_FAILED: u32 = 0x801E_F140;
const TRANSFER_LEN: u32 = 0x801F_01B8;
const DIR_TABLE: u32 = 0x801F_32A8;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Test(u32),
    Open(String, u32),
    Seek(u32, u32, u32),
    Read(u32, u32, u32),
    Write(u32, u32, u32),
    Close(u32),
    Format(String),
    Erase(String),
    First(String),
    Next,
}

#[derive(Default)]
struct Bios {
    log: Vec<Call>,
    /// Pending events by handle; `TestEvent` consumes the one it reports.
    pending: Vec<u32>,
    /// Events that latch once this many `TestEvent` calls have been made.
    late: Option<(usize, Vec<u32>)>,
    tests: usize,
    /// What each `open` answers, in order.
    opens: Vec<u32>,
    seek: u32,
    /// The card's files, for `firstfile` / `nextfile`.
    files: Vec<CardDirEntry>,
    cursor: usize,
}

fn cstr(m: &mut psx_retail_exec::Memory, a: u32) -> String {
    String::from_utf8(crate::menu_painters::cstr(m, a)).expect("ASCII")
}

fn rig() -> Option<(Cpu, Rc<RefCell<Bios>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    for i in 0..4 {
        cpu.mem.wr32(ARRAY_A + 4 * i, HANDLE_A + i);
        cpu.mem.wr32(ARRAY_B + 4 * i, HANDLE_B + i);
    }
    let bios: Rc<RefCell<Bios>> = Rc::default();
    let b = bios.clone();
    cpu.hook(TEST_EVENT, move |r, _| {
        let mut b = b.borrow_mut();
        if let Some((at, set)) = b.late.clone()
            && b.tests >= at
        {
            b.pending.extend(set);
            b.late = None;
        }
        b.tests += 1;
        let h = r.r[A0];
        b.log.push(Call::Test(h));
        let hit = b.pending.iter().position(|&p| p == h);
        if let Some(i) = hit {
            b.pending.remove(i);
        }
        r.r[V0] = u32::from(hit.is_some());
    });
    let b = bios.clone();
    cpu.hook(OPEN, move |r, m| {
        let mut b = b.borrow_mut();
        let path = cstr(m, r.r[A0]);
        b.log.push(Call::Open(path, r.r[A1]));
        r.r[V0] = if b.opens.is_empty() {
            3
        } else {
            b.opens.remove(0)
        };
    });
    let b = bios.clone();
    cpu.hook(LSEEK, move |r, _| {
        let mut b = b.borrow_mut();
        b.log.push(Call::Seek(r.r[A0], r.r[A1], r.r[A2]));
        r.r[V0] = b.seek;
    });
    let b = bios.clone();
    cpu.hook(BIOS_READ, move |r, _| {
        b.borrow_mut()
            .log
            .push(Call::Read(r.r[A0], r.r[A1], r.r[A2]));
        r.r[V0] = 0;
    });
    let b = bios.clone();
    cpu.hook(BIOS_WRITE, move |r, _| {
        b.borrow_mut()
            .log
            .push(Call::Write(r.r[A0], r.r[A1], r.r[A2]));
        r.r[V0] = 0;
    });
    let b = bios.clone();
    cpu.hook(CLOSE, move |r, _| {
        b.borrow_mut().log.push(Call::Close(r.r[A0]));
        r.r[V0] = 0;
    });
    let b = bios.clone();
    cpu.hook(BIOS_FORMAT, move |r, m| {
        let path = cstr(m, r.r[A0]);
        b.borrow_mut().log.push(Call::Format(path));
        r.r[V0] = 1;
    });
    let b = bios.clone();
    cpu.hook(BIOS_ERASE, move |r, m| {
        let path = cstr(m, r.r[A0]);
        b.borrow_mut().log.push(Call::Erase(path));
        r.r[V0] = 1;
    });
    let put = |m: &mut psx_retail_exec::Memory, at: u32, e: &CardDirEntry| {
        for (i, c) in e.name.iter().enumerate() {
            m.wr8(at + i as u32, *c);
        }
        m.wr32(at + 0x18, e.size);
    };
    let b = bios.clone();
    cpu.hook(FIRSTFILE, move |r, m| {
        let mut b = b.borrow_mut();
        let pattern = cstr(m, r.r[A0]);
        b.log.push(Call::First(pattern));
        b.cursor = 0;
        r.r[V0] = match b.files.first() {
            Some(e) => {
                put(m, r.r[A1], e);
                b.cursor = 1;
                r.r[A1]
            }
            None => 0,
        };
    });
    let b = bios.clone();
    cpu.hook(NEXTFILE, move |r, m| {
        let mut b = b.borrow_mut();
        b.log.push(Call::Next);
        let at = b.cursor;
        r.r[V0] = match b.files.get(at) {
            Some(e) => {
                put(m, r.r[A0], e);
                b.cursor += 1;
                r.r[A0]
            }
            None => 0,
        };
    });
    cpu.hook(STRCMP, |r, m| {
        let (a, b) = (
            crate::menu_painters::cstr(m, r.r[A0]),
            crate::menu_painters::cstr(m, r.r[A1]),
        );
        r.r[V0] = u32::from(a != b);
    });
    cpu.stub(PRINTF, 0);
    // `sprintf` runs live; its `%s` arm asks the BIOS for `A(0x1B)` `strlen`
    // or `A(0x2E)` `memchr`.
    cpu.hook(0x8005_6768, |r, m| {
        r.r[V0] = crate::menu_painters::cstr(m, r.r[A0]).len() as u32;
    });
    cpu.hook(0x8005_7014, |r, m| {
        let (p, c, n) = (r.r[A0], r.r[A1] as u8, r.r[A2]);
        r.r[V0] = (0..n).find(|&i| m.rd8(p + i) == c).map_or(0, |i| p + i);
    });
    Some((cpu, bios))
}

fn flags(rng: &mut Rng) -> [bool; 4] {
    let m = match rng.below(4) {
        0 => 0,
        1 => 1 << rng.below(4),
        _ => rng.below(16),
    };
    [0, 1, 2, 3].map(|i| m >> i & 1 != 0)
}

fn pend(base: u32, e: [bool; 4]) -> Vec<u32> {
    (0..4)
        .filter(|&i| e[i as usize])
        .map(|i| base + i)
        .collect()
}

fn tests_of(base: u32, n: u32) -> Vec<Call> {
    (0..n).map(|i| Call::Test(base + i)).collect()
}

#[test]
fn the_event_probes_answer_as_retail_does() {
    differential!(
        prot(899, STATUS_POLL),
        prot(899, DRAIN_A),
        prot(899, WAIT_B),
        prot(899, DRAIN_B),
        prot(899, POLL_A)
    );
    let Some((mut cpu, bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4556_4E54);
    let mut seen = [0u32; 5];
    let mut timed_out = 0;
    for run in 0..4_000 {
        // The backstop poll: every handle is tested, the last to fire wins,
        // and the frame counter overrides once it is spent.
        let e = flags(&mut rng);
        let counter = match rng.below(3) {
            0 => 0x70 + rng.below(0x10),
            _ => rng.below(0x100),
        };
        *bios.borrow_mut() = Bios {
            pending: pend(HANDLE_A, e),
            ..Default::default()
        };
        cpu.mem.wr32(POLL_COUNTER, counter);
        assert_eq!(cpu.call(STATUS_POLL, &[]), Stop::Returned);
        let mut c = counter as u16;
        let port = card_status_poll(e, &mut c);
        let raw = match port {
            CardStatus::Pending => 0,
            CardStatus::Ready => 1,
            CardStatus::Aborted => 2,
            CardStatus::NoCard => 3,
            CardStatus::Complete => 4,
        };
        assert_eq!(cpu.regs.r[V0], raw, "run {run}: poll {e:?} at {counter:#x}");
        assert_eq!(cpu.mem.rd32(POLL_COUNTER), u32::from(c), "run {run}");
        assert_eq!(bios.borrow().log, tests_of(HANDLE_A, 4), "run {run}");
        seen[raw as usize] += 1;
        timed_out += u32::from(counter >= 0x78);

        // The two drains: four tests each, every pending event consumed.
        for (entry, base) in [(DRAIN_A, HANDLE_A), (DRAIN_B, HANDLE_B)] {
            let e = flags(&mut rng);
            *bios.borrow_mut() = Bios {
                pending: pend(base, e),
                ..Default::default()
            };
            assert_eq!(cpu.call(entry, &[]), Stop::Returned);
            let mut port = e;
            if base == HANDLE_A {
                card_events_drain(&mut port);
            } else {
                drain_events_b(&mut port);
            }
            let left = bios.borrow().pending.clone();
            assert_eq!(left, pend(base, port), "run {run}: drain {e:?}");
            assert_eq!(bios.borrow().log, tests_of(base, 4), "run {run}");
        }

        // The first-wins poll.
        let e = flags(&mut rng);
        *bios.borrow_mut() = Bios {
            pending: pend(HANDLE_A, e),
            ..Default::default()
        };
        assert_eq!(cpu.call(POLL_A, &[]), Stop::Returned);
        let fired = poll_events_a(e);
        assert_eq!(cpu.regs.r[V0], fired, "run {run}: poll A {e:?}");
        let tested = if fired == 0 { 4 } else { fired };
        assert_eq!(bios.borrow().log, tests_of(HANDLE_A, tested), "run {run}");

        // The blocking wait: whole empty sweeps until a handle is latched,
        // then the first latched handle in order.
        let mut e = flags(&mut rng);
        if e == [false; 4] {
            e[rng.below(4) as usize] = true;
        }
        let sweeps = rng.below(4) as usize;
        *bios.borrow_mut() = Bios {
            late: Some((sweeps * 4, pend(HANDLE_B, e))),
            ..Default::default()
        };
        assert_eq!(cpu.call(WAIT_B, &[]), Stop::Returned);
        assert_eq!(wait_events_b([false; 4]), None);
        let fired = wait_events_b(e).expect("a latched handle");
        assert_eq!(cpu.regs.r[V0], fired, "run {run}: wait B {e:?}");
        assert_eq!(
            bios.borrow().tests,
            sweeps * 4 + fired as usize,
            "run {run}: {sweeps} empty sweeps"
        );
    }
    assert!(seen.iter().all(|&n| n > 100), "{seen:?}");
    assert!(timed_out > 500, "{timed_out}");
}

/// The BIOS calls a [`CardOp`] stands for, on descriptor `fd`.
fn calls_of(op: &CardOp, fd: u32, buf: u32) -> Vec<Call> {
    match op {
        CardOp::Read {
            path,
            flags,
            len,
            seek_second_frame,
        } => {
            let mut v = vec![Call::Open(path.clone(), *flags)];
            v.extend(tests_of(HANDLE_A, 4));
            if *seek_second_frame {
                v.push(Call::Seek(fd, 0x200, 0));
            }
            v.push(Call::Read(fd, buf, *len));
            v
        }
        CardOp::Write {
            path,
            flags,
            len,
            create,
        } => {
            let mut v = Vec::new();
            if *create {
                v.push(Call::Open(path.clone(), open_flags::CREATE_ONE_BLOCK));
                v.push(Call::Close(fd + 1));
            }
            v.push(Call::Open(path.clone(), *flags));
            v.extend(tests_of(HANDLE_A, 4));
            v.push(Call::Write(fd, buf, *len));
            v
        }
        CardOp::Erase { path } => vec![Call::Erase(path.clone())],
        CardOp::Format { path } => {
            let mut v = tests_of(HANDLE_B, 4);
            v.push(Call::Format(path.clone()));
            v
        }
    }
}

#[test]
fn the_file_calls_issue_what_retail_issues() {
    differential!(
        prot(899, READ),
        prot(899, WRITE),
        prot(899, ERASE),
        prot(899, FORMAT),
        prot(899, STEP)
    );
    let Some((mut cpu, bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let name_va = 0x8013_0000;
    let buf = 0x8014_0000;
    let mut rng = Rng(0x4255_494F);
    let (mut seeks, mut failed, mut created, mut steps) = (0, 0, 0, [0u32; 3]);
    for run in 0..3_000 {
        let (port, unit) = (rng.below(2) as u8, rng.below(2) as u8);
        let name = format!("BASCUS-94254PRO-{:02}", rng.below(15));
        for (i, c) in name.bytes().chain([0]).enumerate() {
            cpu.mem.wr8(name_va + i as u32, c);
        }
        let fd = 2 + rng.below(6);

        // A read. The seek rides the transfer-length global, which the one
        // caller loads the length argument from.
        let len = [0x80u32, 0x2000, 0x2000, 0x100][rng.below(4) as usize];
        let open_fails = rng.below(5) == 0;
        *bios.borrow_mut() = Bios {
            opens: vec![if open_fails { u32::MAX } else { fd }],
            seek: if rng.below(4) == 0 { u32::MAX } else { 0x200 },
            ..Default::default()
        };
        cpu.mem.wr32(TRANSFER_LEN, len);
        cpu.mem.wr32(READ_FAILED, rng.below(2));
        cpu.mem.wr32(WRITE_FAILED, 0);
        cpu.mem.wr32(FD, 0xDEAD);
        cpu.mem.wr32(STACK_TOP + 0x10, len);
        let what = format!("run {run}: read {name} len {len:#x} fd {fd} fails {open_fails}");
        assert_eq!(
            cpu.call(READ, &[u32::from(port), u32::from(unit), name_va, buf]),
            Stop::Returned
        );
        let mut state = CardIoState {
            read_failed: true,
            ..Default::default()
        };
        let CardIssue::Issued(op) = state.read_file(port, unit, &name, len) else {
            panic!("{what}: the port issues")
        };
        let want = calls_of(&op, fd, buf);
        if open_fails {
            failed += 1;
            assert_eq!(state.open_failed(false), CardIssue::OpenFailed);
            assert_eq!(cpu.regs.r[V0], u32::MAX, "{what}");
            assert_eq!(bios.borrow().log, want[..1], "{what}");
            assert_eq!(cpu.mem.rd32(FD), 0xDEAD, "{what}: no descriptor kept");
        } else {
            assert_eq!(cpu.regs.r[V0], 0, "{what}");
            assert_eq!(bios.borrow().log, want, "{what}");
            assert_eq!(cpu.mem.rd32(FD), fd, "{what}");
            seeks += u32::from(len == 0x80);
        }
        assert_eq!(
            (
                cpu.mem.rd32(READ_FAILED) != 0,
                cpu.mem.rd32(WRITE_FAILED) != 0
            ),
            (state.read_failed, state.write_failed),
            "{what}: the failure latches"
        );

        // A write, with or without the create pass.
        let create = rng.below(2);
        let fails_at = match rng.below(6) {
            0 => Some(0),
            1 => Some(1),
            _ => None,
        };
        let mut opens = Vec::new();
        if create == 1 {
            opens.push(if fails_at == Some(0) {
                u32::MAX
            } else {
                fd + 1
            });
        }
        opens.push(if fails_at == Some(1) { u32::MAX } else { fd });
        *bios.borrow_mut() = Bios {
            opens,
            ..Default::default()
        };
        cpu.mem.wr32(WRITE_FAILED, rng.below(2));
        cpu.mem.wr32(READ_FAILED, 0);
        cpu.mem.wr32(FD, 0xDEAD);
        cpu.mem.wr32(STACK_TOP + 0x10, 0x2000);
        cpu.mem.wr32(STACK_TOP + 0x14, create);
        let what = format!("run {run}: write {name} create {create} fails at {fails_at:?}");
        assert_eq!(
            cpu.call(WRITE, &[u32::from(port), u32::from(unit), name_va, buf]),
            Stop::Returned
        );
        let mut state = CardIoState {
            write_failed: true,
            ..Default::default()
        };
        let CardIssue::Issued(op) = state.write_file(port, unit, &name, 0x2000, create == 1) else {
            panic!("{what}: the port issues")
        };
        let want = calls_of(&op, fd, buf);
        created += create;
        let stopped = match (create, fails_at) {
            (1, Some(0)) => Some(1),
            (1, Some(1)) => Some(3),
            (0, Some(1)) => Some(1),
            _ => None,
        };
        match stopped {
            Some(n) => {
                failed += 1;
                assert_eq!(state.open_failed(true), CardIssue::OpenFailed);
                assert_eq!(cpu.regs.r[V0], u32::MAX, "{what}");
                assert_eq!(bios.borrow().log, want[..n], "{what}");
            }
            None => {
                assert_eq!(cpu.regs.r[V0], 0, "{what}");
                assert_eq!(bios.borrow().log, want, "{what}");
                assert_eq!(cpu.mem.rd32(FD), fd, "{what}");
            }
        }
        assert_eq!(
            (
                cpu.mem.rd32(READ_FAILED) != 0,
                cpu.mem.rd32(WRITE_FAILED) != 0
            ),
            (state.read_failed, state.write_failed),
            "{what}: the failure latches"
        );

        // The erase and the format.
        *bios.borrow_mut() = Bios::default();
        assert_eq!(
            cpu.call(ERASE, &[u32::from(port), u32::from(unit), name_va]),
            Stop::Returned
        );
        assert_eq!(
            bios.borrow().log,
            calls_of(&erase_file(port, unit, &name), 0, 0),
            "run {run}: erase"
        );
        let mut e = flags(&mut rng);
        if e == [false; 4] {
            e[rng.below(4) as usize] = true;
        }
        let stale = flags(&mut rng);
        *bios.borrow_mut() = Bios {
            // What was pending before the call is drained, not answered.
            pending: pend(HANDLE_B, stale),
            late: Some((4, pend(HANDLE_B, e))),
            ..Default::default()
        };
        assert_eq!(
            cpu.call(FORMAT, &[u32::from(port), u32::from(unit)]),
            Stop::Returned
        );
        let mut events = stale;
        let op = format_card(port, unit, &mut events);
        assert_eq!(events, [false; 4]);
        let fired = wait_events_b(e).expect("latched");
        let mut want = calls_of(&op, 0, 0);
        want.extend(tests_of(HANDLE_B, fired));
        assert_eq!(bios.borrow().log, want, "run {run}: format {e:?}");
        assert_eq!(
            cpu.regs.r[V0] as i32,
            format_card_result(fired).code(),
            "run {run}: format answered by handle {fired}"
        );

        // One frame of the in-flight step.
        let raw = [0u32, 4, 6, 4, 6, 1, 5, 7][rng.below(8) as usize];
        let e = flags(&mut rng);
        *bios.borrow_mut() = Bios {
            pending: pend(HANDLE_A, e),
            ..Default::default()
        };
        cpu.mem.wr32(PHASE, raw);
        cpu.mem.wr32(FD, fd);
        cpu.mem.wr32(WRITE_FAILED, 0);
        cpu.mem.wr32(READ_FAILED, 0);
        assert_eq!(cpu.call(STEP, &[]), Stop::Returned);
        let mut state = CardIoState {
            phase: CardPhase::from_raw(raw),
            fd_open: true,
            ..Default::default()
        };
        let step = state.step(e);
        let what = format!("run {run}: step phase {raw} events {e:?}");
        let closed = bios.borrow().log.contains(&Call::Close(fd));
        assert_eq!(closed, !state.fd_open, "{what}: the close");
        assert_eq!(closed, matches!(step, CardStep::Finished { .. }), "{what}");
        // A word the step ignores is left as it lies; the port's `Idle`
        // stands for all of them.
        let live = raw == 4 || raw == 6;
        let after = cpu.mem.rd32(PHASE);
        assert_eq!(after, if live { state.phase.raw() } else { raw }, "{what}");
        assert_eq!(
            (
                cpu.mem.rd32(READ_FAILED) != 0,
                cpu.mem.rd32(WRITE_FAILED) != 0
            ),
            (state.read_failed, state.write_failed),
            "{what}: the failure latches"
        );
        steps[match step {
            CardStep::Waiting => 0,
            CardStep::Finished { ok: true } => 1,
            CardStep::Finished { ok: false } => 2,
        }] += 1;
    }
    assert!(
        seeks > 300 && failed > 500 && created > 1000,
        "{seeks} {failed} {created}"
    );
    assert!(steps.iter().all(|&n| n > 200), "{steps:?}");
}

#[test]
fn the_directory_table_fills_and_counts_as_retail_does() {
    differential!(
        prot(899, DIR_SCAN),
        prot(899, FREE_BLOCKS),
        prot(899, FIND_NAME),
        prot(899, RESET)
    );
    let Some((mut cpu, bios)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let name_va = 0x8013_0000;
    let mut rng = Rng(0x4449_5253);
    let (mut empty, mut full, mut found, mut negative) = (0, 0, 0, 0);
    for run in 0..3_000 {
        // Stale names and sizes from the walk before.
        for i in 0..(CARD_DIR_FRAMES * 0x28) as u32 {
            cpu.mem.wr8(DIR_TABLE + i, rng.next() as u8);
        }
        let n = match rng.below(4) {
            0 => 0,
            1 => CARD_DIR_FRAMES,
            _ => rng.below(CARD_DIR_FRAMES as u32 + 1) as usize,
        };
        let files: Vec<CardDirEntry> = (0..n)
            .map(|_| {
                let mut e = CardDirEntry::default();
                let s = format!("BASCUS-94254PRO-{:02}", rng.below(20));
                e.name[..s.len()].copy_from_slice(s.as_bytes());
                e.size = match rng.below(8) {
                    0 => rng.next(),
                    _ => 0x2000 * (1 + rng.below(3)),
                };
                e
            })
            .collect();
        let (port, unit) = (rng.below(2) as u8, rng.below(2) as u8);
        *bios.borrow_mut() = Bios {
            files: files.clone(),
            ..Default::default()
        };
        assert_eq!(
            cpu.call(DIR_SCAN, &[u32::from(port), u32::from(unit)]),
            Stop::Returned
        );
        let (table, count) = card_directory_scan(&files);
        assert_eq!(cpu.regs.r[V0] as usize, count, "run {run}: {n} files");
        assert_eq!(
            bios.borrow().log.first(),
            Some(&Call::First(bu_path(port, unit, Some("*")))),
            "run {run}"
        );
        for (i, e) in table.iter().enumerate() {
            let at = DIR_TABLE + (i * 0x28) as u32;
            assert_eq!(cpu.mem.read(at, 0x14), e.name, "run {run}: name {i}");
            assert_eq!(cpu.mem.rd32(at + 0x18), e.size, "run {run}: size {i}");
        }
        empty += u32::from(n == 0);
        full += u32::from(n == CARD_DIR_FRAMES);

        let take = rng.below(count as u32 + 1);
        assert_eq!(cpu.call(FREE_BLOCKS, &[0, take]), Stop::Returned);
        let free = card_free_blocks(&table, take as usize);
        assert_eq!(cpu.regs.r[V0] as i32, free, "run {run}: {take} entries");
        negative += u32::from(free < 0);

        // The name search over the first `take` records.
        let probe = match (rng.below(2), files.is_empty()) {
            (0, false) => files[rng.below(n as u32) as usize].name,
            _ => {
                let mut name = [0u8; 0x14];
                name[..5].copy_from_slice(b"OTHER");
                name
            }
        };
        let probe: String = probe
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as char)
            .collect();
        for (i, c) in probe.bytes().chain([0]).enumerate() {
            cpu.mem.wr8(name_va + i as u32, c);
        }
        assert_eq!(cpu.call(FIND_NAME, &[take, name_va]), Stop::Returned);
        let mut state = CardIoState {
            dir_names: table
                .iter()
                .map(|e| {
                    e.name
                        .iter()
                        .take_while(|&&c| c != 0)
                        .map(|&c| c as char)
                        .collect()
                })
                .collect(),
            ..Default::default()
        };
        let hit = state.find_directory_name(take as usize, &probe);
        assert_eq!(cpu.regs.r[V0], u32::from(hit), "run {run}: find {probe}");
        found += u32::from(hit);

        // The session reset: the two failure latches always, the 32-slot
        // name cache only on the cold path. The phase word and the open
        // descriptor are not this routine's to clear.
        let warm = rng.below(2);
        let phase = [0u32, 4, 6][rng.below(3) as usize];
        for i in 0..CARD_DIR_SLOTS {
            cpu.mem.wr8(DIR_TABLE + (i * CARD_DIR_STRIDE) as u32, b'X');
        }
        cpu.mem.wr32(WRITE_FAILED, 1);
        cpu.mem.wr32(READ_FAILED, 1);
        cpu.mem.wr32(PHASE, phase);
        cpu.mem.wr32(FD, 5);
        assert_eq!(cpu.call(RESET, &[warm]), Stop::Returned);
        state.write_failed = true;
        state.read_failed = true;
        state.phase = CardPhase::from_raw(phase);
        state.fd_open = true;
        state.reset(warm != 0);
        assert_eq!(
            (
                cpu.mem.rd32(READ_FAILED) != 0,
                cpu.mem.rd32(WRITE_FAILED) != 0
            ),
            (state.read_failed, state.write_failed),
            "run {run}: reset"
        );
        let kept = (0..CARD_DIR_SLOTS)
            .filter(|&i| cpu.mem.rd8(DIR_TABLE + (i * CARD_DIR_STRIDE) as u32) != 0)
            .count();
        assert_eq!(
            kept,
            if warm != 0 { CARD_DIR_SLOTS } else { 0 },
            "run {run}"
        );
        assert_eq!(state.dir_names.is_empty(), warm == 0, "run {run}");
        assert_eq!(
            (cpu.mem.rd32(PHASE), cpu.mem.rd32(FD) == 5),
            (state.phase.raw(), state.fd_open),
            "run {run}: the reset leaves the transfer in flight alone"
        );
    }
    assert!(
        empty > 300 && full > 300 && found > 300 && negative > 50,
        "{empty} {full} {found} {negative}"
    );
}
