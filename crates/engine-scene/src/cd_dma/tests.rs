use super::*;

/// Synthetic host that fakes a 4-sector LBA range per PROT
/// entry. Used by the smoke tests.
#[derive(Default)]
struct FakeCdHost {
    calls: Vec<String>,
    sizes: std::collections::HashMap<ProtIndex, u32>,
    ready: bool,
}

impl CdDmaHost for FakeCdHost {
    fn prot_index_size_lookup(&mut self, prot_idx: ProtIndex, set_msf: bool) -> u32 {
        self.calls
            .push(format!("size_lookup({prot_idx}, {set_msf})"));
        self.sizes.get(&prot_idx).copied().unwrap_or(4)
    }
    fn async_lba_load(&mut self, dst: DestAddr, count: u32, flags: LoadFlags) {
        self.calls.push(format!(
            "async_load({dst:#x}, {count}, {:#x})",
            flags.bits()
        ));
        if flags.issue() {
            self.kick_libcd_read();
        }
    }
    fn kick_libcd_read(&mut self) {
        self.calls.push("kick".into());
        self.ready = true;
    }
    fn read_wait_poll(&mut self, gated: bool) -> ReadWaitOutcome {
        self.calls.push(format!("wait_poll({gated})"));
        if self.ready {
            ReadWaitOutcome::Ready
        } else {
            ReadWaitOutcome::InProgress
        }
    }
}

#[test]
fn load_flags_bit_decode_round_trips() {
    assert!(LoadFlags::ISSUE.issue());
    assert!(!LoadFlags::ISSUE.block());
    assert!(LoadFlags::BLOCK.block());
    assert!(!LoadFlags::BLOCK.issue());
    assert!(LoadFlags::SYNCHRONOUS.issue());
    assert!(LoadFlags::SYNCHRONOUS.block());
    // Bit-or composes correctly.
    let f = LoadFlags::ISSUE | LoadFlags::BLOCK;
    assert!(f.issue());
    assert!(f.block());
}

#[test]
fn read_wait_outcome_round_trips_retail_codes() {
    for code in 0..=2 {
        let r = ReadWaitOutcome::from_retail(code);
        assert_eq!(r.to_retail(), code);
    }
    // Unknown codes fold into InProgress (the retail catch-all).
    assert_eq!(
        ReadWaitOutcome::from_retail(99),
        ReadWaitOutcome::InProgress
    );
    assert_eq!(ReadWaitOutcome::InProgress.to_retail(), 1);
}

#[test]
fn one_shot_default_chains_size_lookup_and_async_load() {
    let mut h = FakeCdHost::default();
    let n = h.prot_one_shot_load(42, 0x80100000, LoadFlags::SYNCHRONOUS);
    // Default LBA count from the fake = 4.
    assert_eq!(n, 4);
    assert_eq!(
        h.calls,
        vec![
            "size_lookup(42, true)".to_string(),
            "async_load(0x80100000, 4, 0x3)".to_string(),
            "kick".to_string(),
        ]
    );
}

#[test]
fn one_shot_without_issue_skips_kick() {
    let mut h = FakeCdHost::default();
    // Empty flags = neither ISSUE nor BLOCK.
    let n = h.prot_one_shot_load(7, 0x80200000, LoadFlags::default());
    assert_eq!(n, 4);
    assert_eq!(
        h.calls,
        vec![
            "size_lookup(7, false)".to_string(),
            "async_load(0x80200000, 4, 0x0)".to_string(),
        ]
    );
}

#[test]
fn poll_reports_ready_after_kick() {
    let mut h = FakeCdHost::default();
    assert_eq!(h.read_wait_poll(true), ReadWaitOutcome::InProgress);
    h.kick_libcd_read();
    assert_eq!(h.read_wait_poll(true), ReadWaitOutcome::Ready);
}

#[test]
fn host_records_size_override() {
    let mut h = FakeCdHost::default();
    h.sizes.insert(100, 17);
    assert_eq!(h.prot_index_size_lookup(100, false), 17);
    assert_eq!(h.prot_index_size_lookup(101, false), 4); // default
}

// -- ProtCdDmaHost tests -------------------------------------------

/// Build a minimal valid PROT.DAT byte image with two entries:
///
/// - entry 0: `start_lba = 1`, retail LBA count = 4 (footprint).
/// - entry 1: `start_lba = 5`, retail LBA count = 2.
///
/// Entry 0's 4 sectors are filled with `0xAA`, entry 1's 2 sectors
/// with `0xBB`, so a successful read can be detected by checking
/// the destination buffer's first byte.
fn build_synthetic_prot_dat() -> Vec<u8> {
    const TOTAL_BYTES: usize = 0x3800;
    let mut buf = vec![0u8; TOTAL_BYTES];
    // Header head at offset 0: [unused u32][file_num_minus_1 u32].
    // `header_sectors` aliases `toc[0]` in the on-disc layout (the
    // `detect_header` reader pulls bytes 8..12, and the TOC slice
    // starts at byte 8). We set header_sectors = 1 via toc[0] below.
    buf[0..4].copy_from_slice(&0u32.to_le_bytes());
    buf[4..8].copy_from_slice(&2u32.to_le_bytes()); // file_num - 1 = 2 (file_num = 3)
    // TOC starts at file offset 0x08. We write 7 dwords:
    //   toc[0] = 1                       (= header_sectors)
    //   toc[2] = 1, toc[3] = 5,
    //   toc[4] = 7, toc[5] = 6, toc[6] = 5
    // Retail-formula counts: toc[3]-toc[2]=4, toc[4]-toc[3]=2.
    // Archive-parser indexed sizes: toc[5]-toc[3]+4=5, toc[6]-toc[4]+4=2.
    const TOC: [u32; 7] = [1, 0, 1, 5, 7, 6, 5];
    for (i, v) in TOC.iter().enumerate() {
        let off = 0x08 + i * 4;
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    // Entry 0 body: LBA 1..5 (4 sectors), filled with 0xAA.
    for b in &mut buf[0x0800..0x2800] {
        *b = 0xAA;
    }
    // Entry 1 body: LBA 5..7 (2 sectors), filled with 0xBB.
    for b in &mut buf[0x2800..0x3800] {
        *b = 0xBB;
    }
    buf
}

fn make_synthetic_host() -> ProtCdDmaHost {
    let bytes = build_synthetic_prot_dat();
    let prot = crate::scene::ProtIndex::from_bytes(bytes, None).expect("parse synthetic PROT.DAT");
    ProtCdDmaHost::new(std::sync::Arc::new(prot))
}

#[test]
fn synthetic_prot_dat_yields_two_entries() {
    let host = make_synthetic_host();
    assert_eq!(host.prot.entry_count(), 2);
    // Retail formula matches what we crafted.
    assert_eq!(host.prot.entry_start_lba_retail(0), Some(1));
    assert_eq!(host.prot.entry_lba_count_retail(0), Some(4));
    assert_eq!(host.prot.entry_start_lba_retail(1), Some(5));
    assert_eq!(host.prot.entry_lba_count_retail(1), Some(2));
}

#[test]
fn prot_size_lookup_stashes_start_lba_and_returns_count() {
    let mut host = make_synthetic_host();
    let n = host.prot_index_size_lookup(0, false);
    assert_eq!(n, 4);
    assert_eq!(host.last_prot_idx(), 0);
    assert_eq!(host.last_start_lba(), 1);
    assert!(host.last_msf().is_none(), "set_msf=false must not set MSF");
    let n = host.prot_index_size_lookup(1, true);
    assert_eq!(n, 2);
    assert_eq!(host.last_start_lba(), 5);
    // BCD MSF: (5+150) sectors = 155 sectors = 0:02:05 + 2-sec pregap.
    // Actually 155/75 = 2 seconds 5 frames. mins=0, secs=2, frames=5.
    assert_eq!(host.last_msf(), Some((0x00, 0x02, 0x05)));
}

#[test]
fn prot_size_lookup_out_of_range_returns_zero() {
    let mut host = make_synthetic_host();
    // Way past the last valid TOC entry - retail formula wraps,
    // but our None-on-out-of-range fallback yields zero.
    let n = host.prot_index_size_lookup(u16::MAX, false);
    assert_eq!(n, 0);
}

#[test]
fn one_shot_load_copies_entry_bytes_into_main_ram() {
    let mut host = make_synthetic_host();
    let dst: DestAddr = 0x8010_0000;
    let n = host.prot_one_shot_load(0, dst, LoadFlags::SYNCHRONOUS);
    assert_eq!(n, 4, "retail LBA count for entry 0");
    // The 4 sectors (0x2000 bytes) of entry 0 are 0xAA.
    let slice = host.read(dst, 0x2000).expect("read back");
    assert!(slice.iter().all(|&b| b == 0xAA), "entry 0 bytes mismatch");
    // Just past the read window the buffer is still zero.
    let tail = host.read(dst + 0x2000, 1).unwrap();
    assert_eq!(tail, &[0u8]);
}

#[test]
fn one_shot_load_for_entry_1_copies_at_a_different_offset() {
    let mut host = make_synthetic_host();
    let dst: DestAddr = 0x8014_0000;
    let n = host.prot_one_shot_load(1, dst, LoadFlags::SYNCHRONOUS);
    assert_eq!(n, 2);
    let slice = host.read(dst, 0x1000).expect("read back");
    assert!(slice.iter().all(|&b| b == 0xBB), "entry 1 bytes mismatch");
}

#[test]
fn poll_after_synchronous_kick_reports_ready() {
    let mut host = make_synthetic_host();
    // Boot state: no kick issued => poll returns Ready immediately
    // (matches retail FUN_8003de7c's `read_in_progress == 0` early-out).
    assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
    // The synchronous one-shot load completes inline. Poll still Ready.
    host.prot_one_shot_load(0, 0x8010_0000, LoadFlags::SYNCHRONOUS);
    assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
    // Drain returns Ready unconditionally and the post-drain poll is
    // still Ready (state is back to the boot configuration).
    assert_eq!(host.read_wait_poll(false), ReadWaitOutcome::Ready);
    assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
}

#[test]
fn issue_without_block_still_drives_state_to_ready() {
    // The offline host's "issue" path performs the copy synchronously,
    // so the BLOCK bit doesn't gate anything in practice - the buffer
    // is already populated after the kick.
    let mut host = make_synthetic_host();
    host.prot_index_size_lookup(0, false);
    host.async_lba_load(0x8010_0000, 4, LoadFlags::ISSUE);
    assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
    assert_eq!(host.last_count(), 4);
    assert_eq!(host.last_dst(), 0x8010_0000);
}

#[test]
fn high_psx_pointers_fold_into_synthetic_main_ram() {
    // Retail uses both kseg0 (0x80xxxxxx) and kseg1 (0xA0xxxxxx) for
    // DMA targets; both should alias the same offset.
    let mut host = make_synthetic_host();
    host.prot_one_shot_load(0, 0xA010_0000, LoadFlags::SYNCHRONOUS);
    let folded = host.read(0x8010_0000, 0x2000).unwrap();
    assert!(folded.iter().all(|&b| b == 0xAA));
}

#[test]
fn overlay_loader_a_drives_prot_cd_dma_host_end_to_end() {
    use crate::overlay_loader::{
        OVERLAY_CACHE_EMPTY, OVERLAY_PROT_BASE, OverlayCacheSlot, load_overlay_a,
    };
    let mut host = make_synthetic_host();
    host.set_overlay_destinations(0x8010_0000, 0x8011_0000);
    // The synthetic PROT only has 2 entries; the overlay loaders' real
    // call site uses param values whose `+ OVERLAY_PROT_BASE` resolves
    // to a real overlay PROT index. For the smoke test we accept the
    // synthetic PROT's "garbage size" return - the wiring is what we
    // verify.
    let param = -(OVERLAY_PROT_BASE); // → prot_idx 0
    let result = load_overlay_a(&mut host, param);
    assert_eq!(result, param, "fresh load returns param");
    assert_eq!(host.overlay_slot(OverlayCacheSlot::A), param);
    assert_eq!(
        host.overlay_slot(OverlayCacheSlot::B),
        OVERLAY_CACHE_EMPTY,
        "sister slot invalidated"
    );
    assert_eq!(host.last_prot_idx(), 0, "PROT 0 was looked up");
    assert_eq!(host.last_dst(), 0x8010_0000);
}

#[test]
fn overlay_loader_b_drives_prot_cd_dma_host_end_to_end() {
    use crate::overlay_loader::{OVERLAY_PROT_BASE, OverlayCacheSlot, load_overlay_b};
    let mut host = make_synthetic_host();
    host.set_overlay_destinations(0x8010_0000, 0x8011_0000);
    let param = -(OVERLAY_PROT_BASE); // → prot_idx 0
    let result = load_overlay_b(&mut host, param);
    assert_eq!(result, param);
    assert_eq!(host.overlay_slot(OverlayCacheSlot::B), param);
    assert_eq!(host.last_dst(), 0x8011_0000, "uses slot B's dst buffer");
}

#[test]
fn overlay_loader_dev_branch_short_circuits_real_host() {
    use crate::overlay_loader::{OverlayCacheSlot, load_overlay_a};
    let mut host = make_synthetic_host();
    host.set_overlay_dev_flag(0x42);
    // Dev branch: stash and return the flag value. No PROT load.
    let last_dst_before = host.last_dst();
    let result = load_overlay_a(&mut host, 100);
    assert_eq!(result, 0x42);
    assert_eq!(host.overlay_slot(OverlayCacheSlot::A), 100);
    assert_eq!(
        host.last_dst(),
        last_dst_before,
        "dev branch must not trigger a CD-DMA read"
    );
}

// -- stream_read_sync (FUN_8005EA84) tests -------------------------

/// Scripted host for the streaming-read sync: the vsync counter
/// advances by one per read, and `sectors_remaining` counts down by
/// one per vsync read past `complete_at` (simulating the IRQ chain
/// delivering sectors while the CPU polls).
struct FakeSyncHost {
    vsync: i32,
    start_vsync: i32,
    last_sector_vsync: i32,
    sectors: i32,
    total: i32,
    restarts: u32,
    delivered: u32,
    /// When `Some(n)`, each `vsync_now` call past `n` decrements
    /// `sectors` (floor 0) and refreshes `last_sector_vsync`.
    drain_after: Option<i32>,
}

impl FakeSyncHost {
    fn healthy(sectors: i32) -> Self {
        Self {
            vsync: 100,
            start_vsync: 100,
            last_sector_vsync: 100,
            sectors,
            total: sectors,
            restarts: 0,
            delivered: 0,
            drain_after: None,
        }
    }
}

impl StreamReadSyncHost for FakeSyncHost {
    fn vsync_now(&mut self) -> i32 {
        self.vsync += 1;
        if let Some(n) = self.drain_after
            && self.vsync > n
            && self.sectors > 0
        {
            self.sectors -= 1;
            self.last_sector_vsync = self.vsync;
        }
        self.vsync
    }
    fn read_start_vsync(&self) -> i32 {
        self.start_vsync
    }
    fn last_sector_vsync(&self) -> i32 {
        self.last_sector_vsync
    }
    fn sectors_remaining(&self) -> i32 {
        self.sectors
    }
    fn total_sector_count(&self) -> i32 {
        self.total
    }
    fn restart_streaming_read(&mut self) {
        self.restarts += 1;
        self.sectors = self.total;
        self.last_sector_vsync = self.vsync;
    }
    fn deliver_status(&mut self, result: Option<&mut [u8]>) {
        self.delivered += 1;
        if let Some(buf) = result
            && let Some(b) = buf.first_mut()
        {
            *b = 0x02; // "ready" status marker for the test
        }
    }
}

#[test]
fn stream_sync_poll_reports_remaining_without_restart() {
    let mut h = FakeSyncHost::healthy(5);
    let n = stream_read_sync(&mut h, true, None);
    assert_eq!(n, 5, "healthy in-flight read reports sectors remaining");
    assert_eq!(h.restarts, 0);
    assert_eq!(h.delivered, 1, "status delivered exactly once on exit");
}

#[test]
fn stream_sync_poll_reports_zero_when_complete() {
    let mut h = FakeSyncHost::healthy(0);
    assert_eq!(stream_read_sync(&mut h, true, None), 0);
    assert_eq!(h.restarts, 0);
}

#[test]
fn stream_sync_block_loops_until_drained() {
    let mut h = FakeSyncHost::healthy(3);
    h.drain_after = Some(100); // sectors arrive from the first poll on
    let n = stream_read_sync(&mut h, false, None);
    assert_eq!(n, 0, "block mode returns only once the read completes");
    assert_eq!(h.restarts, 0);
    assert_eq!(h.delivered, 1);
}

#[test]
fn stream_sync_overall_timeout_reports_minus_one() {
    let mut h = FakeSyncHost::healthy(4);
    // Jump the clock past start + 0x4B0: the next vsync read exceeds
    // the overall window, so even block mode exits with -1.
    h.vsync = h.start_vsync + STREAM_SYNC_TIMEOUT_VSYNCS;
    assert_eq!(stream_read_sync(&mut h, false, None), STREAM_SYNC_TIMED_OUT);
    assert_eq!(h.restarts, 0, "timeout path does not restart the read");
    assert_eq!(h.delivered, 1, "status still delivered on the way out");
}

#[test]
fn stream_sync_error_state_restarts_and_reports_total() {
    let mut h = FakeSyncHost::healthy(6);
    h.sectors = -1; // IRQ-side error state (DAT_800796D8 < 0)
    let n = stream_read_sync(&mut h, true, None);
    assert_eq!(n, 6, "restart reports the request's full sector count");
    assert_eq!(h.restarts, 1);
}

#[test]
fn stream_sync_stall_restarts_and_reports_total() {
    let mut h = FakeSyncHost::healthy(2);
    // Last sector arrived long ago: > 0x3C vsyncs before "now".
    h.last_sector_vsync = h.vsync - STREAM_SYNC_STALL_VSYNCS - 2;
    let n = stream_read_sync(&mut h, true, None);
    assert_eq!(n, 2, "total == remaining here; restart path taken");
    assert_eq!(h.restarts, 1, "stalled transfer is re-armed");
}

#[test]
fn stream_sync_fresh_transfer_is_not_stalled() {
    let mut h = FakeSyncHost::healthy(2);
    // Exactly at the stall boundary: last + 0x3C == now is NOT a
    // stall (retail slt is strict less-than).
    h.last_sector_vsync = h.vsync + 2 - STREAM_SYNC_STALL_VSYNCS;
    let n = stream_read_sync(&mut h, true, None);
    assert_eq!(n, 2);
    assert_eq!(h.restarts, 0, "boundary case must not restart");
}

#[test]
fn stream_sync_writes_status_into_result_buffer() {
    let mut h = FakeSyncHost::healthy(0);
    let mut status = [0u8; 8];
    stream_read_sync(&mut h, true, Some(&mut status));
    assert_eq!(status[0], 0x02, "deliver_status saw the result buffer");
}

#[test]
fn lba_to_bcd_msf_round_trips_known_landmarks() {
    // LBA 0 is the PSX-pregap entry: MSF 00:02:00.
    assert_eq!(lba_to_bcd_msf(0), (0x00, 0x02, 0x00));
    // LBA 75 = 1 second past the pregap: MSF 00:03:00.
    assert_eq!(lba_to_bcd_msf(75), (0x00, 0x03, 0x00));
    // LBA 60*75 = 1 minute past pregap: MSF 01:02:00.
    assert_eq!(lba_to_bcd_msf(60 * 75), (0x01, 0x02, 0x00));
}

// ---- StreamLoadQueue (FUN_8003DDA0) ----

#[test]
fn stream_queue_first_enqueue_lands_at_offset_zero() {
    let mut q = StreamLoadQueue::new();
    // Entry 897 spans 4 sectors (start 1000, next 1004).
    let d = q.enqueue(897, 1000, 1004);
    assert_eq!(
        d,
        StreamLoadDescriptor {
            prot_idx: 897,
            byte_offset: 0
        }
    );
    assert_eq!(q.count(), 1);
    assert_eq!(q.last_prot_idx(), 897);
    assert_eq!(q.last_start_lba(), 1000);
    assert_eq!(q.read_wait_timeout(), 0x78);
    // 4 sectors * 0x800 = 0x2000 bytes queued.
    assert_eq!(q.byte_cursor(), 4 * 0x800);
}

#[test]
fn stream_queue_accumulates_byte_cursor_across_entries() {
    let mut q = StreamLoadQueue::new();
    q.enqueue(10, 0, 3); // 3 sectors -> +0x1800
    let d2 = q.enqueue(11, 100, 105); // 5 sectors -> +0x2800
    // Second entry's payload lands right after the first (0x1800).
    assert_eq!(d2.byte_offset, 3 * 0x800);
    assert_eq!(q.byte_cursor(), (3 + 5) * 0x800);
    assert_eq!(q.count(), 2);
    assert_eq!(
        q.entries()[0],
        StreamLoadDescriptor {
            prot_idx: 10,
            byte_offset: 0
        }
    );
    // last_* track the most recent enqueue only.
    assert_eq!(q.last_prot_idx(), 11);
    assert_eq!(q.last_start_lba(), 100);
}

#[test]
fn stream_queue_from_toc_reads_idx_plus_2_and_3() {
    // Raw in-RAM TOC head: word[idx+2] = start, word[idx+3] = next.
    // Lay out so entry idx=1 -> start=toc[3], next=toc[4].
    let toc: Vec<u32> = vec![0, 0, 0, 500, 512, 600];
    let mut q = StreamLoadQueue::new();
    let d = q.enqueue_from_toc(1, &toc).expect("idx+3 in range");
    assert_eq!(d.prot_idx, 1);
    assert_eq!(q.last_start_lba(), 500);
    // 512 - 500 = 12 sectors * 0x800.
    assert_eq!(q.byte_cursor(), 12 * 0x800);
}

#[test]
fn stream_queue_from_toc_out_of_range_returns_none() {
    let toc: Vec<u32> = vec![0, 0, 500];
    let mut q = StreamLoadQueue::new();
    // idx=0 needs toc[2] and toc[3]; toc[3] is missing.
    assert!(q.enqueue_from_toc(0, &toc).is_none());
    assert_eq!(q.count(), 0, "a failed lookup must not mutate the queue");
}

#[test]
fn stream_queue_degenerate_size_does_not_panic() {
    let mut q = StreamLoadQueue::new();
    // next < start: retail computes a wrapping size; port must not panic.
    let d = q.enqueue(5, 200, 100);
    assert_eq!(d.byte_offset, 0);
    assert_eq!(q.count(), 1);
}
