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

#[test]
fn the_stream_queue_plans_what_retail_plans() {
    differential!(scus(ENQUEUE));
    if crate::common::disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    const ENQUEUE: u32 = 0x8003_DDA0;
    const TABLE_PTR: u32 = GP + 0x1A8;
    const QUEUED: u32 = GP + 0x8BC;
    const BYTE_CURSOR: u32 = GP + 0x984;
    const TABLE: u32 = 0x8010_0000;
    let index = Arc::new(index());
    let mut cpu = cd_cpu(&index).expect("disc");
    // The in-RAM TOC as the port's wrapper reads it, header words included.
    let ram_toc: Vec<u32> = (0..index.toc().len() as u32 + 2)
        .map(|i| cpu.mem.rd32(RAM_TOC + 4 * i))
        .collect();
    let entries = index.toc().len() as u32 - 8;
    let mut rng = Rng(0x5155_4555);
    for round in 0..200 {
        reset(&mut cpu);
        cpu.mem.wr32(TABLE_PTR, TABLE);
        cpu.mem.wr32(QUEUED, 0);
        cpu.mem.wr32(BYTE_CURSOR, 0);
        cpu.mem.load(TABLE, &[0xA5; 0x200]);
        let mut port = legaia_engine_scene::cd_dma::StreamLoadQueue::new();
        for step in 0..1 + rng.below(40) {
            let idx = rng.below(entries);
            assert_eq!(cpu.call(ENQUEUE, &[idx]), Stop::Returned);
            let d = port.enqueue_from_toc(idx as u16, &ram_toc).expect("in range");
            let at = TABLE + 8 * step;
            assert_eq!(
                (cpu.mem.rd32(at), cpu.mem.rd32(at + 4)),
                (u32::from(d.prot_idx), d.byte_offset),
                "round {round} step {step}"
            );
            assert_eq!(cpu.mem.rd32(QUEUED) as usize, port.count());
            assert_eq!(cpu.mem.rd32(BYTE_CURSOR), port.byte_cursor());
            assert_eq!(cpu.mem.rd32(LAST_INDEX), u32::from(port.last_prot_idx()));
            assert_eq!(cpu.mem.rd32(LAST_START), port.last_start_lba());
            assert_eq!(cpu.mem.rd32(TIMEOUT), port.read_wait_timeout());
            // The slot after the one written stands.
            assert_eq!(cpu.mem.rd32(at + 8), 0xA5A5_A5A5);
        }
    }
}

#[test]
fn the_overlay_loaders_cache_what_the_port_caches() {
    differential!(scus(LOADER_A), scus(LOADER_B));
    if crate::common::disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    use legaia_engine_scene::overlay_loader::{OverlayCacheSlot, load_overlay_a, load_overlay_b};
    const LOADER_A: u32 = 0x8003_EBE4;
    const LOADER_B: u32 = 0x8003_EC70;
    const SLOT_A: u32 = GP + 0x924;
    const SLOT_B: u32 = GP + 0x934;
    const DST_A_PTR: u32 = 0x8001_038C;
    const DST_B_PTR: u32 = 0x8001_0390;
    const DEV_FLAG: u32 = 0x8007_B868;
    const MODE_WORD: u32 = 0x8007_B83C;
    const DST_A: u32 = 0x8010_0000;
    const DST_B: u32 = 0x8014_0000;
    const SPAN: usize = 0x3_0000;
    let index = Arc::new(index());
    let mut cpu = cd_cpu(&index).expect("disc");
    // Overlay parameters whose entry fits the scratch destinations.
    let params: Vec<i32> = (0..0x60)
        .filter(|p| {
            index
                .entry_lba_count_retail((p + 0x37F) as u16)
                .is_some_and(|n| n as usize * 0x800 <= SPAN)
        })
        .collect();
    assert!(params.len() > 20, "{} loadable overlays", params.len());
    let (dst_a_word, dst_b_word) = (cpu.mem.rd32(DST_A_PTR), cpu.mem.rd32(DST_B_PTR));
    let mut rng = Rng(0x4F56_4C59);
    let (mut fresh, mut hits, mut dev_calls, mut invalidated) = (0, 0, 0, 0);
    for round in 0..120 {
        reset(&mut cpu);
        cpu.mem.wr32(SLOT_A, -1i32 as u32);
        cpu.mem.wr32(SLOT_B, -1i32 as u32);
        cpu.mem.wr32(DST_A_PTR, DST_A);
        cpu.mem.wr32(DST_B_PTR, DST_B);
        cpu.mem.load(DST_A, &vec![0; SPAN]);
        cpu.mem.load(DST_B, &vec![0; SPAN]);
        let mut port = ProtCdDmaHost::new(index.clone());
        port.set_overlay_destinations(DST_A, DST_B);
        for step in 0..12 {
            let ctx = format!("round {round} step {step}");
            let dev = if rng.below(8) == 0 { 1 + rng.below(9) } else { 0 };
            let mode = if rng.below(4) == 0 { 0x15 } else { rng.below(0x1C) as u16 };
            // A small pool, so the cache hits.
            let param = params[rng.below(4.min(params.len() as u32)) as usize + (round % 8)];
            cpu.mem.wr32(DEV_FLAG, dev);
            cpu.mem.wr16(MODE_WORD, mode);
            port.set_overlay_dev_flag(dev);
            port.set_overlay_mode_state(mode);
            let a = rng.below(2) == 0;
            let was = (port.overlay_slot(OverlayCacheSlot::A), port.overlay_slot(OverlayCacheSlot::B));
            assert_eq!(
                cpu.call(if a { LOADER_A } else { LOADER_B }, &[param as u32]),
                Stop::Returned,
                "{ctx}"
            );
            let retail = cpu.regs.r[V0] as i32;
            // The loader only issues the read; the mode switch waits on it.
            cpu.mem.wr32(DEV_FLAG, 0);
            assert_eq!(cpu.call(WAIT, &[0]), Stop::Returned, "{ctx}");
            let got = if a {
                load_overlay_a(&mut port, param)
            } else {
                load_overlay_b(&mut port, param)
            };
            let _ = port.read_wait_poll(false);
            assert_eq!(retail, got, "{ctx}: return, loader {}", if a { 'A' } else { 'B' });
            let now = (port.overlay_slot(OverlayCacheSlot::A), port.overlay_slot(OverlayCacheSlot::B));
            assert_eq!(
                (cpu.mem.rd32(SLOT_A) as i32, cpu.mem.rd32(SLOT_B) as i32),
                now,
                "{ctx}: cache slots"
            );
            for dst in [DST_A, DST_B] {
                let want = port.read(dst, SPAN).expect("port RAM").to_vec();
                assert!(cpu.mem.read(dst, SPAN) == want, "{ctx}: overlay image");
            }
            if dev != 0 {
                dev_calls += 1;
            } else if was == now {
                hits += 1;
            } else {
                fresh += 1;
                invalidated += usize::from(a && was.1 != -1);
            }
        }
    }
    cpu.mem.wr32(DST_A_PTR, dst_a_word);
    cpu.mem.wr32(DST_B_PTR, dst_b_word);
    assert!(
        fresh > 300 && hits > 100 && dev_calls > 50 && invalidated > 50,
        "{fresh} {hits} {dev_calls} {invalidated}"
    );
}

/// The drive a streaming read waits on, as both sides see it: each look at
/// the vsync counter moves time on, and now and then a sector lands.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Drive {
    now: i32,
    start: i32,
    last_sector: i32,
    remaining: i32,
    total: i32,
    steps: Vec<(i32, bool)>,
    looks: usize,
    restarts: u32,
    delivered: Vec<u32>,
}

impl Drive {
    fn look(&mut self) -> i32 {
        let (dt, sector) = self.steps[self.looks % self.steps.len()];
        self.looks += 1;
        self.now += dt;
        if sector && self.remaining > 0 {
            self.remaining -= 1;
            self.last_sector = self.now;
        }
        self.now
    }
    fn restart(&mut self) {
        self.restarts += 1;
        self.remaining = self.total;
        self.last_sector = self.now;
    }
}

impl legaia_engine_scene::cd_dma::StreamReadSyncHost for Drive {
    fn vsync_now(&mut self) -> i32 {
        self.look()
    }
    fn read_start_vsync(&self) -> i32 {
        self.start
    }
    fn last_sector_vsync(&self) -> i32 {
        self.last_sector
    }
    fn sectors_remaining(&self) -> i32 {
        self.remaining
    }
    fn total_sector_count(&self) -> i32 {
        self.total
    }
    fn restart_streaming_read(&mut self) {
        self.restart();
    }
    fn deliver_status(&mut self, result: Option<&mut [u8]>) {
        self.delivered.push(u32::from(result.is_some()));
    }
}

#[test]
fn the_stream_read_sync_waits_as_retail_waits() {
    differential!(scus(SYNC));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    const SYNC: u32 = 0x8005_EA84;
    const RESTART: u32 = 0x8005_E788;
    const DELIVER: u32 = 0x8005_BEAC;
    const READ_START: u32 = 0x8007_96E0;
    const LAST_SECTOR: u32 = 0x8007_96DC;
    const REMAINING: u32 = 0x8007_96D8;
    const TOTAL: u32 = 0x8007_96C4;
    const RESULT: u32 = 0x8010_0000;
    let drive: Rc<RefCell<Option<Drive>>> = Rc::default();
    // The cells are the drive's: the hooks carry them across each call.
    fn pull(d: &mut Drive, m: &mut Memory) {
        d.start = m.rd32(READ_START) as i32;
        d.last_sector = m.rd32(LAST_SECTOR) as i32;
        d.remaining = m.rd32(REMAINING) as i32;
        d.total = m.rd32(TOTAL) as i32;
    }
    fn push(d: &Drive, m: &mut Memory) {
        m.wr32(READ_START, d.start as u32);
        m.wr32(LAST_SECTOR, d.last_sector as u32);
        m.wr32(REMAINING, d.remaining as u32);
        m.wr32(TOTAL, d.total as u32);
    }
    {
        let drive = drive.clone();
        cpu.hook(VSYNC, move |r, m| {
            assert_eq!(r.r[psx_retail_exec::A0], -1i32 as u32, "a counter read");
            let mut g = drive.borrow_mut();
            let d = g.as_mut().expect("drive");
            pull(d, m);
            r.r[V0] = d.look() as u32;
            push(d, m);
        });
    }
    {
        let drive = drive.clone();
        cpu.hook(RESTART, move |r, m| {
            assert_eq!(r.r[psx_retail_exec::A0], 1);
            let mut g = drive.borrow_mut();
            let d = g.as_mut().expect("drive");
            pull(d, m);
            d.restart();
            push(d, m);
        });
    }
    {
        let drive = drive.clone();
        cpu.hook(DELIVER, move |r, _| {
            assert_eq!(r.r[psx_retail_exec::A0], 1);
            let mut g = drive.borrow_mut();
            g.as_mut()
                .expect("drive")
                .delivered
                .push(u32::from(r.r[psx_retail_exec::A1] != 0));
        });
    }
    let mut rng = Rng(0x5359_4E43);
    let (mut timed_out, mut done, mut restarted, mut polled) = (0, 0, 0, 0);
    for round in 0..4_000 {
        let total = 1 + rng.below(40) as i32;
        let now = rng.below(0x1_0000) as i32;
        let start = now - rng.below(0x500) as i32;
        let seed = Drive {
            now,
            start,
            last_sector: now - rng.below(0x50) as i32,
            remaining: match rng.below(8) {
                0 => -1 - rng.below(3) as i32,
                1 => 0,
                _ => 1 + rng.below(total as u32) as i32,
            },
            total,
            // Slow drives stall; quick ones finish.
            steps: (0..7)
                .map(|_| (rng.below(0x30) as i32, rng.below(3) != 0))
                .collect(),
            looks: 0,
            restarts: 0,
            delivered: Vec::new(),
        };
        let poll_once = rng.below(2) == 1;
        let with_result = rng.below(2) == 1;

        *drive.borrow_mut() = Some(seed.clone());
        push(&seed, &mut cpu.mem);
        let args = [u32::from(poll_once), if with_result { RESULT } else { 0 }];
        assert_eq!(cpu.call(SYNC, &args), Stop::Returned, "round {round}");
        let retail_ret = cpu.regs.r[V0] as i32;
        let retail = drive.borrow_mut().take().expect("drive");

        let mut port = seed;
        let mut buf = [0u8; 8];
        let port_ret = legaia_engine_scene::cd_dma::stream_read_sync(
            &mut port,
            poll_once,
            with_result.then_some(&mut buf[..]),
        );
        assert_eq!(retail_ret, port_ret, "round {round}: report");
        assert_eq!(retail, port, "round {round}: the drive after the wait");
        timed_out += usize::from(port_ret == -1);
        done += usize::from(port_ret == 0);
        restarted += usize::from(port.restarts > 0);
        polled += usize::from(poll_once && port_ret > 0);
    }
    assert!(
        timed_out > 100 && done > 300 && restarted > 300 && polled > 300,
        "{timed_out} {done} {restarted} {polled}"
    );
}
