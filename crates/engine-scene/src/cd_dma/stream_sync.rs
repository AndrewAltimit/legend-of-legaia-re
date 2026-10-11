//! Streaming-read completion sync (`FUN_8005EA84`) and the BCD MSF conversion it issues seeks with.

// =========================================================================
// Streaming-read completion sync - FUN_8005EA84
// =========================================================================

/// Overall streaming-read timeout in vsyncs (`0x4B0` = 1200 frames = 20
/// seconds NTSC). Measured from the read-start timestamp `DAT_800796E0`;
/// once exceeded, [`stream_read_sync`] reports [`STREAM_SYNC_TIMED_OUT`].
pub const STREAM_SYNC_TIMEOUT_VSYNCS: i32 = 0x4B0;

/// Per-sector stall window in vsyncs (`0x3C` = 60 frames = 1 second NTSC).
/// Measured from the last-sector-IRQ timestamp `DAT_800796DC`; once
/// exceeded, [`stream_read_sync`] restarts the streaming read.
pub const STREAM_SYNC_STALL_VSYNCS: i32 = 0x3C;

/// Return value of [`stream_read_sync`] when the overall timeout expired
/// (the `li s0,-0x1` delay-slot default at `0x8005EAC8` that survives to
/// the return when the timeout branch is taken).
pub const STREAM_SYNC_TIMED_OUT: i32 = -1;

/// Host surface for the streaming-read completion sync
/// ([`stream_read_sync`], the port of `FUN_8005EA84`). Maps 1:1 onto the
/// retail globals + callees the function touches:
///
/// | Method                    | Retail                                        |
/// |---------------------------|-----------------------------------------------|
/// | `vsync_now`               | `FUN_8005FB84(-1)` (VSync counter read)       |
/// | `read_start_vsync`        | `DAT_800796E0` (read-start timestamp)         |
/// | `last_sector_vsync`       | `DAT_800796DC` (last-sector-IRQ timestamp)    |
/// | `sectors_remaining`       | `DAT_800796D8` (negative = error state)       |
/// | `total_sector_count`      | `DAT_800796C4` (request's full sector count)  |
/// | `restart_streaming_read`  | `FUN_8005E788(1)` (re-arm the IRQ chain)      |
/// | `deliver_status`          | `FUN_8005BEAC(1, result)` (CdSync-shape poll) |
pub trait StreamReadSyncHost {
    /// Current vsync counter (`FUN_8005FB84(-1)` - absolute count, no wait).
    fn vsync_now(&mut self) -> i32;
    /// Vsync timestamp at which the streaming read started (`DAT_800796E0`).
    fn read_start_vsync(&self) -> i32;
    /// Vsync timestamp of the most recent sector-IRQ (`DAT_800796DC`).
    fn last_sector_vsync(&self) -> i32;
    /// Sectors still outstanding (`DAT_800796D8`). Negative marks the
    /// error state the IRQ callback leaves behind on a failed read.
    fn sectors_remaining(&self) -> i32;
    /// Full sector count of the current request (`DAT_800796C4`).
    fn total_sector_count(&self) -> i32;
    /// Restart the streaming read (`FUN_8005E788(1)`): re-copies the
    /// source globals and re-registers the per-IRQ callback.
    fn restart_streaming_read(&mut self);
    /// Deliver the drive status (`FUN_8005BEAC(1, result)`, which forwards
    /// both register arguments into the CdSync-shaped poller
    /// `FUN_8005CCB4`). `result` receives the status bytes when the host
    /// has them; retail callers pass a small stack buffer.
    fn deliver_status(&mut self, result: Option<&mut [u8]>);
}

/// PORT: FUN_8005EA84
///
/// Streaming-read completion sync - the libcd-layer `CdReadSync`-shaped
/// poll/block primitive above the
/// per-IRQ streaming reader (`FUN_8005E574` / `FUN_8005E788`). Two modes,
/// selected by `poll_once` (retail `a0`):
///
/// - `poll_once = true` (retail `a0 = 1`): one health-check pass, then
///   return. Used by the boot TOC loader `FUN_8003E4E8` and the path
///   loader `FUN_8003D3C4`, which loop at the call site.
/// - `poll_once = false` (retail `a0 = 0`): block until the read
///   completes (`sectors_remaining` reaches 0) or the overall timeout
///   expires. Used by the sync LBA reader `FUN_8005E4D4`.
///
/// Each pass (loop head `0x8005EAAC`):
///
/// 1. Default the report to [`STREAM_SYNC_TIMED_OUT`].
/// 2. If `vsync_now() > read_start + 0x4B0`, the whole read timed out -
///    keep the `-1` and fall through to the exit check.
/// 3. Otherwise, a negative `sectors_remaining` (IRQ-side error state)
///    or a stalled transfer (`last_sector + 0x3C < vsync_now()`, i.e.
///    more than 1 s without a sector IRQ) restarts the stream via
///    `restart_streaming_read` and reports the full
///    `total_sector_count`; a healthy transfer reports
///    `sectors_remaining` as-is.
/// 4. Exit when `poll_once` is set, or when the report is `<= 0`
///    (complete, or timed out).
///
/// On exit the drive status is delivered into `result` via
/// `deliver_status` (retail `FUN_8005BEAC(1, result)`), and the last
/// report is returned: sectors remaining, `0` on completion, or
/// [`STREAM_SYNC_TIMED_OUT`].
///
/// REPLACED-BY: `legaia_iso`'s in-memory disc image, read synchronously
/// behind `crate::scene::ProtIndex`.
///
/// Nothing implements [`StreamReadSyncHost`] and nothing can: there is no
/// IRQ-driven sector chain to wait on - no outstanding-sector counter, no
/// per-sector IRQ timestamp, and no vsync counter to time the two watchdogs
/// against - so every value the trait asks for would have to be invented.
/// The completion this routine waits for has already happened by the time a
/// port-side read returns.
pub fn stream_read_sync(
    host: &mut impl StreamReadSyncHost,
    poll_once: bool,
    result: Option<&mut [u8]>,
) -> i32 {
    let mut report;
    loop {
        // Delay-slot default at 0x8005EAC8: s0 = -1 (timed out).
        report = STREAM_SYNC_TIMED_OUT;
        let now = host.vsync_now();
        if now <= host.read_start_vsync() + STREAM_SYNC_TIMEOUT_VSYNCS {
            let stalled = if host.sectors_remaining() < 0 {
                // bltz at 0x8005EAD4: IRQ-side error state.
                true
            } else {
                // Second vsync read at 0x8005EADC: per-sector stall check.
                let now = host.vsync_now();
                host.last_sector_vsync() + STREAM_SYNC_STALL_VSYNCS < now
            };
            if stalled {
                host.restart_streaming_read();
                report = host.total_sector_count();
            } else {
                // Fresh re-read at 0x8005EB10 - the IRQ chain may have
                // delivered sectors between the two loads.
                report = host.sectors_remaining();
            }
        }
        // Exit check at 0x8005EB14: poll mode returns after one pass;
        // block mode loops while the report stays positive.
        if poll_once || report <= 0 {
            break;
        }
    }
    host.deliver_status(result);
    report
}

/// LBA → BCD `(minutes, seconds, frames)` triple. Mirrors the retail
/// helper at `FUN_8005c42c` + `FUN_8005c328` chain used inside
/// `FUN_8003e8a8` to materialise the per-request MSF into
/// `gp+0x95C..+0x95E`. The 2-second pregap offset is folded in (the
/// PSX clock starts the data area at MSF `00:02:00`, i.e. LBA 0 →
/// `(0, 2, 0)`).
pub(super) fn lba_to_bcd_msf(lba: u32) -> (u8, u8, u8) {
    let lba = lba.wrapping_add(150);
    let mins = lba / (60 * 75);
    let secs = (lba / 75) % 60;
    let frames = lba % 75;
    (
        bin_to_bcd(mins as u8),
        bin_to_bcd(secs as u8),
        bin_to_bcd(frames as u8),
    )
}

fn bin_to_bcd(v: u8) -> u8 {
    ((v / 10) << 4) | (v % 10)
}
