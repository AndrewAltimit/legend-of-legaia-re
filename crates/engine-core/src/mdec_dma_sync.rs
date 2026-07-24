//! The FMV overlay's **MDEC DMA sync** pair - the two entry points every
//! decode step of the STR playback loop funnels its channel waits through.
//!
//! Both live in the slot-A STR/FMV overlay and are byte-identical in PROT
//! 0970 (`cutscene_str`) and PROT 0971 (`debug_menu`), the same co-residency
//! the MDECin DMA-callback hook `FUN_801CFE98` shows (see
//! `docs/subsystems/cutscene.md`). They are *not* debug-menu logic: the
//! `overlay_debug_menu_*` dumps at these VAs are that capture's copy of the
//! same overlay bytes.
//!
//! Each takes one argument that selects between a **blocking** wait and a
//! **non-blocking poll** of the same busy bit:
//!
//! | Entry | argument `0` | argument non-zero |
//! |---|---|---|
//! | `FUN_801CFE20` | spin until MDEC-**in** idle | read the in-busy bit |
//! | `FUN_801CFE5C` | spin until MDEC-**out** idle | read the out-busy bit |
//!
//! The blocking halves are `FUN_801D0100` / `FUN_801D0198`: a
//! [`SPIN_BUDGET`]-iteration countdown re-reading the status word each pass,
//! returning [`SYNC_OK`] the moment the bit clears and [`SYNC_TIMEOUT`] with
//! a `"MDEC in sync"` / `"MDEC out sync"` diagnostic if the budget runs out.
//! The polling halves both read the *same* status word (`FUN_801D0230`,
//! a six-instruction leaf that dereferences the pointer global) and differ
//! only in which bit they extract - bit [`IN_BUSY_BIT`] and bit
//! [`OUT_POLL_BIT`] respectively.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_str_fmv_0x801CFE20.txt`,
//! `..._0x801CFE5C.txt`, `..._0x801D0100.txt`, `..._0x801D0198.txt`,
//! `..._0x801D0230.txt`. Ported from the disassembly.
//!
//! # NOT WIRED
//!
//! The engine's MDEC path is a software decoder (`legaia_mdec`) driven
//! frame-at-a-time by `crate::cutscene`; it has no DMA channels and no MDEC
//! status register, so nothing produces the status words these helpers read.
//! Wiring needs a hardware-shaped MDEC front end - which the clean-room port
//! deliberately does not have - or a host that models the two busy bits as
//! decoder back-pressure.

/// Countdown the blocking waits start from (`0x100000` iterations).
pub const SPIN_BUDGET: i32 = 0x0010_0000;

/// Return value of a wait that saw the channel go idle.
pub const SYNC_OK: i32 = 0;

/// Return value of a wait that exhausted [`SPIN_BUDGET`].
pub const SYNC_TIMEOUT: i32 = -1;

/// Mask of the MDEC-**in** busy bit in the status word (`0x2000_0000`).
pub const IN_BUSY_MASK: u32 = 0x2000_0000;

/// Bit index the non-blocking in-poll extracts (`>> 0x1D & 1`) - the same
/// bit [`IN_BUSY_MASK`] selects.
pub const IN_BUSY_BIT: u32 = 0x1D;

/// Mask of the MDEC-**out** busy bit in its own status word
/// (`0x0100_0000`).
pub const OUT_BUSY_MASK: u32 = 0x0100_0000;

/// Bit index the non-blocking out-poll extracts (`>> 0x18 & 1`).
///
/// The poll reads the **in** status word, not the out one: `FUN_801CFE5C`'s
/// non-zero arm calls the same `FUN_801D0230` leaf its sibling does. Only
/// the blocking arm reads the out-side pointer. That asymmetry is retail's.
pub const OUT_POLL_BIT: u32 = 0x18;

/// Diagnostic the in-side wait prints when it times out.
pub const IN_TIMEOUT_MESSAGE: &str = "MDEC in sync";

/// Diagnostic the out-side wait prints when it times out.
pub const OUT_TIMEOUT_MESSAGE: &str = "MDEC out sync";

/// Outcome of one blocking wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncResult {
    /// [`SYNC_OK`] or [`SYNC_TIMEOUT`].
    pub code: i32,
    /// How many status reads the wait performed, including the first.
    pub reads: u32,
    /// The diagnostic string retail would print, or `None` on success.
    pub timeout_message: Option<&'static str>,
}

/// The blocking-wait kernel shared by both entries.
///
/// `status` yields the current status word on each read. The first read is
/// unconditional; if the busy bit is already clear the routine returns
/// immediately without touching the countdown. Otherwise it decrements from
/// [`SPIN_BUDGET`] and re-reads, and gives up when the counter reaches `-1` -
/// so the budget allows exactly `SPIN_BUDGET` retries after the first read.
fn wait_until_idle<F: FnMut() -> u32>(
    mut status: F,
    mask: u32,
    message: &'static str,
) -> SyncResult {
    let mut reads = 1;
    let mut word = status();
    let mut budget = SPIN_BUDGET;
    while word & mask != 0 {
        budget -= 1;
        if budget == -1 {
            return SyncResult {
                code: SYNC_TIMEOUT,
                reads,
                timeout_message: Some(message),
            };
        }
        reads += 1;
        word = status();
    }
    SyncResult {
        code: SYNC_OK,
        reads,
        timeout_message: None,
    }
}

/// Wait for the MDEC-**in** channel to go idle.
///
/// PORT: FUN_801d0100
pub fn wait_mdec_in_idle<F: FnMut() -> u32>(status: F) -> SyncResult {
    wait_until_idle(status, IN_BUSY_MASK, IN_TIMEOUT_MESSAGE)
}

/// Wait for the MDEC-**out** channel to go idle.
///
/// PORT: FUN_801d0198
pub fn wait_mdec_out_idle<F: FnMut() -> u32>(status: F) -> SyncResult {
    wait_until_idle(status, OUT_BUSY_MASK, OUT_TIMEOUT_MESSAGE)
}

/// What one call of either entry did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdecSync {
    /// The blocking arm ran.
    Waited(SyncResult),
    /// The polling arm ran; the payload is the extracted bit (`0` or `1`).
    Polled(u32),
}

impl MdecSync {
    /// The value retail returns in `v0` for this call.
    pub fn value(self) -> i32 {
        match self {
            MdecSync::Waited(r) => r.code,
            MdecSync::Polled(bit) => bit as i32,
        }
    }
}

/// MDEC-**in** sync entry: `mode == 0` blocks, anything else polls.
///
/// `in_status` is the status word both arms read; the blocking arm re-reads
/// it per spin, the polling arm reads it once.
///
/// PORT: FUN_801cfe20
pub fn mdec_in_sync<F: FnMut() -> u32>(mode: i32, mut in_status: F) -> MdecSync {
    if mode == 0 {
        MdecSync::Waited(wait_mdec_in_idle(in_status))
    } else {
        MdecSync::Polled((in_status() >> IN_BUSY_BIT) & 1)
    }
}

/// MDEC-**out** sync entry: `mode == 0` blocks on the out-side status word,
/// anything else polls bit [`OUT_POLL_BIT`] of the **in**-side word.
///
/// PORT: FUN_801cfe5c
pub fn mdec_out_sync<Fo: FnMut() -> u32, Fi: FnMut() -> u32>(
    mode: i32,
    out_status: Fo,
    mut in_status: Fi,
) -> MdecSync {
    if mode == 0 {
        MdecSync::Waited(wait_mdec_out_idle(out_status))
    } else {
        MdecSync::Polled((in_status() >> OUT_POLL_BIT) & 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A status source that returns `busy` for the first `n` reads then
    /// `idle` forever.
    fn busy_for(n: u32, busy: u32) -> impl FnMut() -> u32 {
        let mut left = n;
        move || {
            if left > 0 {
                left -= 1;
                busy
            } else {
                0
            }
        }
    }

    #[test]
    fn an_already_idle_channel_returns_on_the_first_read() {
        let r = wait_mdec_in_idle(|| 0);
        assert_eq!(r.code, SYNC_OK);
        assert_eq!(r.reads, 1);
        assert_eq!(r.timeout_message, None);
    }

    #[test]
    fn the_wait_spins_until_the_busy_bit_clears() {
        let r = wait_mdec_in_idle(busy_for(4, IN_BUSY_MASK));
        assert_eq!(r.code, SYNC_OK);
        assert_eq!(r.reads, 5);
    }

    #[test]
    fn an_unrelated_bit_does_not_hold_the_wait() {
        // Only IN_BUSY_MASK gates the in-side wait.
        let r = wait_mdec_in_idle(|| OUT_BUSY_MASK);
        assert_eq!(r.code, SYNC_OK);
    }

    #[test]
    fn each_side_watches_its_own_mask() {
        assert_eq!(wait_mdec_out_idle(|| IN_BUSY_MASK).code, SYNC_OK);
        let r = wait_mdec_out_idle(busy_for(2, OUT_BUSY_MASK));
        assert_eq!(r.code, SYNC_OK);
        assert_eq!(r.reads, 3);
    }

    #[test]
    fn a_stuck_channel_times_out_with_its_own_message() {
        let r = wait_mdec_in_idle(|| IN_BUSY_MASK);
        assert_eq!(r.code, SYNC_TIMEOUT);
        assert_eq!(r.timeout_message, Some(IN_TIMEOUT_MESSAGE));
        assert_eq!(r.reads, SPIN_BUDGET as u32 + 1);
        let r = wait_mdec_out_idle(|| OUT_BUSY_MASK);
        assert_eq!(r.timeout_message, Some(OUT_TIMEOUT_MESSAGE));
    }

    #[test]
    fn mode_zero_blocks_and_anything_else_polls() {
        assert_eq!(
            mdec_in_sync(0, || 0),
            MdecSync::Waited(wait_mdec_in_idle(|| 0))
        );
        assert_eq!(mdec_in_sync(1, || IN_BUSY_MASK), MdecSync::Polled(1));
        assert_eq!(mdec_in_sync(1, || 0), MdecSync::Polled(0));
        // The mode argument is only tested against zero.
        assert_eq!(mdec_in_sync(-5, || IN_BUSY_MASK), MdecSync::Polled(1));
    }

    #[test]
    fn the_out_poll_reads_the_in_side_word() {
        // Bit 0x18 of the *in* status decides, and the out-side source is
        // not consulted at all on the polling path.
        let got = mdec_out_sync(
            1,
            || panic!("out source must not be read"),
            || OUT_BUSY_MASK,
        );
        assert_eq!(got, MdecSync::Polled(1));
        let got = mdec_out_sync(1, || panic!("out source must not be read"), || 0);
        assert_eq!(got, MdecSync::Polled(0));
    }

    #[test]
    fn the_return_value_matches_the_retail_v0() {
        assert_eq!(mdec_in_sync(0, || 0).value(), SYNC_OK);
        assert_eq!(mdec_in_sync(0, || IN_BUSY_MASK).value(), SYNC_TIMEOUT);
        assert_eq!(mdec_in_sync(1, || IN_BUSY_MASK).value(), 1);
    }
}
