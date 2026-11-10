//! The CD read plumbing of `SCUS_942.54`, run with the drive stood in for:
//! the libcd calls answer "done", and the VSync a blocking wait spins on
//! delivers the sectors the kick asked for out of `PROT.DAT`, at the LBA the
//! kick computed, and raises the completion state the data-ready callback
//! would.
//!
//! - The stream-file shims `FUN_800558FC` (open), `FUN_80055A5C` (seek),
//!   `FUN_800559EC` (read) and `FUN_80055AC8` (close) with the cursor seek
//!   `FUN_8003E964`, as one stream of operations against `StreamFileHost`:
//!   the cursor after every call, the deferred-op park, and the bytes a read
//!   lands.
//! - The DMA primitives `FUN_8003E8A8` (index lookup), `FUN_8003E800` (load),
//!   `FUN_8003EB98` (one-shot), `FUN_8003F128` (kick) and `FUN_8003DE7C`
//!   (wait) against `ProtCdDmaHost`: the cells each leaves, what it returns
//!   and the bytes at the destination.
//!
//! The port reads synchronously, so a read "in progress" exists on the
//! retail side only; the test completes it before comparing data.

use crate::common::{Rng, scus_cpu};
use crate::walk_on::index;
use legaia_engine_scene::cd_dma::{CdDmaHost, LoadFlags, ProtCdDmaHost, ReadWaitOutcome};
use legaia_engine_scene::scene::ProtIndex;
use legaia_engine_scene::stream_file::{SeekWhence, StreamFileHost};
use psx_retail_exec::{Cpu, Memory, Stop, V0};
use std::sync::Arc;

const OPEN: u32 = 0x8005_58FC;
const SEEK: u32 = 0x8005_5A5C;
const READ: u32 = 0x8005_59EC;
const CLOSE: u32 = 0x8005_5AC8;
const SEEK_SECTORS: u32 = 0x8003_E964;
const LOOKUP: u32 = 0x8003_E8A8;
const LOAD: u32 = 0x8003_E800;
const ONE_SHOT: u32 = 0x8003_EB98;
const KICK: u32 = 0x8003_F128;
const WAIT: u32 = 0x8003_DE7C;
const INT_TO_POS: u32 = 0x8005_C328;
const VSYNC: u32 = 0x8005_FB84;

const GP: u32 = 0x8007_B318;
const RAM_TOC: u32 = 0x801C_70F0;
const PROT_BASE_MSF: u32 = 0x8007_BC50;
const CURSOR_MSF: u32 = GP + 0x944;
const LAST_INDEX: u32 = GP + 0x90C;
const LAST_START: u32 = GP + 0x8F0;
const COUNT: u32 = GP + 0x97C;
const DEST: u32 = GP + 0x894;
const IN_PROGRESS: u32 = GP + 0x988;
const STATE: u32 = GP + 0x980;
const ERROR: u32 = GP + 0x928;
const TIMEOUT: u32 = GP + 0x91C;
const KICK_DEST: u32 = GP + 0x940;
const KICK_LBA: u32 = GP + 0x8C8;
const PENDING_OP: u32 = 0x8007_BD08;
const PARKED_OP: u32 = 0x8007_BD38;
const PARKED_TIMER: u32 = 0x8007_BD44;
/// `_DAT_8007B8C2`, clear on a development host that reads files by name.
const BY_INDEX: u32 = 0x8007_B8C2;
/// The LBA `PROT.DAT` starts at, any value a disc could give it.
const PROT_LBA: u32 = 0x5A3;
const BUF: u32 = 0x8010_0000;

fn bcd(b: u8) -> u32 {
    u32::from(b >> 4) * 10 + u32::from(b & 0xF)
}

/// The cursor's LBA within `PROT.DAT`.
fn cursor(m: &mut Memory) -> u32 {
    let (mm, ss, ff) = (
        m.rd8(CURSOR_MSF),
        m.rd8(CURSOR_MSF + 1),
        m.rd8(CURSOR_MSF + 2),
    );
    ((bcd(mm) * 60 + bcd(ss)) * 75 + bcd(ff))
        .wrapping_sub(150)
        .wrapping_sub(PROT_LBA)
}

/// A CPU with the in-RAM TOC, the archive's base and the drive's stand-in.
fn cd_cpu(index: &Arc<ProtIndex>) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    for va in [
        0x8003_EE7C, // the busy check's prelude
        0x8003_F2B8, // the per-frame poll
        0x8005_BEE4, // CdSyncCallback
        0x8005_BECC, // CdReadyCallback
        0x8005_67A8, // printf
        0x8001_A068, // debug print
    ] {
        cpu.stub(va, 0);
    }
    cpu.stub(0x8005_BE8C, 2); // CdSync: complete
    cpu.stub(0x8005_C034, 1); // CdControl: accepted
    cpu.hook(0x8003_ED04, |_, m| m.wr32(ERROR, 0));
    {
        // The drive: every sector the kick asked for, at once.
        let index = index.clone();
        cpu.hook(VSYNC, move |r, m| {
            if m.rd32(IN_PROGRESS) == 1 && m.rd32(STATE) != 6 {
                let lba = m.rd32(KICK_LBA).wrapping_sub(PROT_LBA);
                let n = m.rd32(COUNT) as usize * 0x800;
                if n > 0 {
                    let bytes = index
                        .prot_dat_raw_bytes(u64::from(lba) * 0x800, n)
                        .expect("PROT.DAT sectors");
                    let to = m.rd32(KICK_DEST);
                    m.load(to, &bytes);
                }
                m.wr32(STATE, 6);
            }
            r.r[V0] = 0;
        });
    }
    // The boot loader's copy: the TOC's words two in.
    for (i, w) in index.toc().iter().enumerate().take(0x600) {
        cpu.mem.wr32(RAM_TOC + 4 * (i as u32 + 2), *w);
    }
    assert_eq!(
        cpu.call(INT_TO_POS, &[PROT_LBA, PROT_BASE_MSF]),
        Stop::Returned
    );
    cpu.mem.wr16(BY_INDEX, 1);
    Some(cpu)
}

fn reset(cpu: &mut Cpu) {
    for cell in [
        COUNT,
        DEST,
        IN_PROGRESS,
        STATE,
        ERROR,
        TIMEOUT,
        PENDING_OP,
        PARKED_OP,
        PARKED_TIMER,
    ] {
        cpu.mem.wr32(cell, 0);
    }
}

#[test]
fn the_stream_shims_move_the_cursor_the_port_moves() {
    differential!(
        scus(OPEN),
        scus(SEEK),
        scus(READ),
        scus(CLOSE),
        scus(SEEK_SECTORS)
    );
    if crate::common::disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = Arc::new(index());
    let mut cpu = cd_cpu(&index).expect("disc");
    let entries = index.toc().len() as u32 - 8;
    let mut rng = Rng(0x5354_5246);
    let (mut reads, mut bytes_read, mut parks, mut rewinds) = (0, 0usize, 0, 0);
    for round in 0..400 {
        reset(&mut cpu);
        let mut port = StreamFileHost::new(index.clone());
        // The first operation is an open: the port refuses a cursor it was
        // never given, where retail walks on from stale cells.
        let mut opened = false;
        for step in 0..24 {
            let ctx = format!("round {round} step {step}");
            let token = if rng.below(5) == 0 {
                parks += 1;
                1 + rng.next() % 0xFFFF
            } else {
                0
            };
            let op = if opened { rng.below(10) } else { 0 };
            if op != 9 {
                cpu.mem.wr32(PENDING_OP, token);
                port.set_pending_op(token);
            }
            match op {
                0 => {
                    let raw = 2 + rng.below(entries - 4);
                    assert_eq!(
                        cpu.call(OPEN, &[0, rng.next(), rng.next(), raw]),
                        Stop::Returned
                    );
                    port.open_raw(raw as u16).expect("open");
                    opened = true;
                }
                1..=3 => {
                    let whence = [0u32, 0, 1, 1, 2, 0x100][rng.below(6) as usize];
                    let off = rng.below(0x40) * 0x800 + rng.below(0x800) * rng.below(2);
                    assert_eq!(cpu.call(SEEK, &[0, off, whence]), Stop::Returned);
                    let w = SeekWhence::from_retail(whence as u8);
                    rewinds += usize::from(w == SeekWhence::FromBase);
                    port.seek_bytes(off, w).expect("seek");
                }
                4 | 5 => {
                    let whence = rng.below(3);
                    let n = rng.below(0x40);
                    assert_eq!(cpu.call(SEEK_SECTORS, &[n, whence]), Stop::Returned);
                    port.seek_sectors(n, SeekWhence::from_retail(whence as u8))
                        .expect("seek");
                }
                6..=8 => {
                    let len = rng.below(5) * 0x800 + rng.below(0x800) * rng.below(2);
                    cpu.mem.load(BUF, &vec![0xA5; 0x3000]);
                    assert_eq!(cpu.call(READ, &[0, BUF, len]), Stop::Returned);
                    // The shim only issues the read; its consumer waits.
                    assert_eq!(cpu.call(WAIT, &[0]), Stop::Returned, "{ctx}");
                    let mut dst = vec![0xA5u8; 0x3000];
                    let n = port.read(&mut dst[..len as usize]).expect("read");
                    assert_eq!(n, (len as usize / 0x800) * 0x800, "{ctx}");
                    assert!(cpu.mem.read(BUF, 0x3000) == dst, "{ctx}: bytes read");
                    reads += 1;
                    bytes_read += n;
                }
                _ => {
                    // The deferred-op park is every shim's first act; close
                    // does nothing else.
                    cpu.mem.wr32(PENDING_OP, token);
                    port.set_pending_op(token);
                    assert_eq!(cpu.call(CLOSE, &[]), Stop::Returned);
                    port.close();
                }
            }
            assert_eq!(
                cursor(&mut cpu.mem),
                port.position_sector(),
                "{ctx}: cursor after op {op}"
            );
            // `FUN_8003E964` is no shim and parks nothing.
            if !matches!(op, 4 | 5) {
                assert_eq!(cpu.mem.rd32(PENDING_OP), 0, "{ctx}");
            }
            let parked = (cpu.mem.rd32(PARKED_TIMER) != 0)
                .then(|| (cpu.mem.rd32(PARKED_OP), cpu.mem.rd32(PARKED_TIMER)));
            if matches!(op, 4 | 5) {
                // The port's seek helper is reached through the shim only;
                // take the token back off both sides.
                cpu.mem.wr32(PENDING_OP, 0);
                port.set_pending_op(0);
            }
            assert_eq!(parked, port.parked_op(), "{ctx}: parked op");
        }
    }
    assert!(
        reads > 1_000 && bytes_read > 0x10_0000 && parks > 500 && rewinds > 300,
        "{reads} {bytes_read} {parks} {rewinds}"
    );
}

#[test]
fn the_dma_primitives_leave_what_the_port_host_leaves() {
    differential!(
        scus(LOOKUP),
        scus(LOAD),
        scus(ONE_SHOT),
        scus(KICK),
        scus(WAIT)
    );
    if crate::common::disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = Arc::new(index());
    let mut cpu = cd_cpu(&index).expect("disc");
    let entries = index.toc().len() as u32 - 8;
    let mut rng = Rng(0x444D_4148);
    let mut ops = [0u32; 5];
    let (mut waited_out, mut idle) = (0, 0);
    for round in 0..300 {
        reset(&mut cpu);
        let mut port = ProtCdDmaHost::new(index.clone());
        // A destination either side can check: the port's RAM is its own.
        let dst = BUF + 0x1000 * rng.below(8);
        cpu.mem.load(dst, &vec![0; 0x800 * 0x40]);
        // A fresh host's cursor stands on the archive's first sector.
        assert_eq!(cpu.call(INT_TO_POS, &[PROT_LBA, CURSOR_MSF]), Stop::Returned);
        cpu.mem.wr32(LAST_INDEX, 0);
        cpu.mem.wr32(LAST_START, 0);
        let mut pending = false;
        let mut looked_up = false;
        for step in 0..16 {
            let ctx = format!("round {round} step {step}");
            let op = rng.below(5) as usize;
            ops[op] += 1;
            // Small entries, so a load stays inside the scratch region.
            let pick = |rng: &mut Rng| loop {
                let i = rng.below(entries - 4) as u16;
                if index.entry_lba_count_retail(i).is_some_and(|n| n <= 0x40) {
                    return i;
                }
            };
            match op {
                0 => {
                    let (i, set) = (pick(&mut rng), rng.below(2) == 1);
                    // The routine drains a read in flight first.
                    assert_eq!(
                        cpu.call(LOOKUP, &[u32::from(i) + 2, u32::from(set)]),
                        Stop::Returned
                    );
                    pending = false;
                    looked_up = true;
                    let n = port.prot_index_size_lookup(i, set);
                    assert_eq!(cpu.regs.r[V0], n, "{ctx}: sector count");
                }
                1 => {
                    let flags = rng.below(4);
                    let count = rng.below(4);
                    assert_eq!(cpu.call(LOAD, &[dst, count, flags]), Stop::Returned);
                    port.async_lba_load(dst, count, LoadFlags::new(flags));
                    assert_eq!(cpu.mem.rd32(TIMEOUT), 0x78, "{ctx}");
                    pending = flags == 1;
                }
                2 => {
                    let (i, flags) = (pick(&mut rng), rng.below(4));
                    assert_eq!(
                        cpu.call(ONE_SHOT, &[u32::from(i) + 2, dst, flags]),
                        Stop::Returned
                    );
                    looked_up = true;
                    let n = port.prot_one_shot_load(i, dst, LoadFlags::new(flags));
                    assert_eq!(cpu.regs.r[V0], n, "{ctx}: sector count");
                    pending = flags == 1;
                }
                3 => {
                    assert_eq!(cpu.call(KICK, &[]), Stop::Returned);
                    port.kick_libcd_read();
                    pending = true;
                }
                _ => {
                    let gated = rng.below(2) == 1;
                    if gated && pending && rng.below(2) == 0 {
                        // The drive has not answered: retail waits another
                        // frame, a state the synchronous port never holds.
                        assert_eq!(cpu.call(WAIT, &[1]), Stop::Returned);
                        assert_eq!(cpu.regs.r[V0], 1, "{ctx}: in progress");
                        waited_out += 1;
                        continue;
                    }
                    if gated && pending {
                        // The data-ready callback's last sector.
                        assert_eq!(cpu.call(VSYNC, &[0]), Stop::Returned);
                    }
                    assert_eq!(cpu.call(WAIT, &[u32::from(gated)]), Stop::Returned);
                    let retail = cpu.regs.r[V0];
                    let outcome = port.read_wait_poll(gated);
                    assert_eq!(retail, outcome.to_retail(), "{ctx}: wait({gated})");
                    idle += u32::from(outcome == ReadWaitOutcome::Ready && !pending);
                    pending = false;
                }
            }
            // The port names an entry by extraction index, two below the
            // in-RAM TOC index retail's cell holds.
            if looked_up {
                assert_eq!(
                    cpu.mem.rd32(LAST_INDEX),
                    u32::from(port.last_prot_idx()) + 2,
                    "{ctx}: index cell"
                );
            }
            assert_eq!(cpu.mem.rd32(LAST_START), port.last_start_lba(), "{ctx}");
            assert_eq!(cpu.mem.rd32(COUNT), port.last_count(), "{ctx}: count cell");
            if cpu.mem.rd32(DEST) != 0 || port.last_dst() != 0 {
                assert_eq!(cpu.mem.rd32(DEST), port.last_dst(), "{ctx}: dest cell");
            }
            assert_eq!(cursor(&mut cpu.mem), port.cursor_lba(), "{ctx}: cursor, op {op}");
            if !pending {
                let want = port.read(dst, 0x800 * 0x40).expect("port RAM").to_vec();
                assert!(cpu.mem.read(dst, want.len()) == want, "{ctx}: destination");
            }
        }
    }
    assert!(
        ops.iter().all(|&n| n > 500) && waited_out > 50 && idle > 100,
        "{ops:?} {waited_out} {idle}"
    );
}
