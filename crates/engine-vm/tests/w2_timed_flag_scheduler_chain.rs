//! The installer -> scheduler chain for retail's timed-flag (escape) timer,
//! exercised end to end across the two ports that make it up.
//!
//! Field VM `0x4C 0xD3` (`SCHEDULE_TIMED_FLAGS`, `FUN_801DE840` case `0xD`
//! sub `3`) arms the timer; `FUN_801D2EBC` drains it once per frame, firing a
//! below-threshold flag on the way down and an expiry flag at zero. The two
//! halves are ported in different modules (`field` and `world_map_overlay`),
//! and `FieldHost::op4c_n_d_sub3_party_setup` is the seam between them.
//!
//! This test is the crate-local statement of that join, against a minimal
//! host: it decodes a synthetic installer instruction through the real VM,
//! takes the operand triple off the host hook, seeds an [`EscapeTimer`] from
//! it, and asserts the flags the drain fires. It pins that the two ports agree
//! on the operand layout independently of any one host's wiring; the world
//! side of the same chain (`World::schedule_timed_flags` ->
//! `World::tick_escape_timer`) is covered by `engine-core`'s
//! `escape_timer_world`.
//!
//! Bytecode is hand-authored - no Sony bytes.

use legaia_engine_vm::field::{FieldCtx, FieldHost, StepResult, step};
use legaia_engine_vm::world_map_overlay::EscapeTimer;

/// Minimal host: records the one hook this chain needs.
#[derive(Default)]
struct CaptureHost {
    flags: u32,
    /// `(ab, cd, ef)` from the last `0x4C 0xD3`.
    schedule: Option<(u32, u32, u32)>,
}

impl FieldHost for CaptureHost {
    fn global_flags(&self) -> u32 {
        self.flags
    }
    fn set_global_flags(&mut self, value: u32) {
        self.flags = value;
    }
    fn frame_delta(&self) -> u16 {
        1
    }
    fn op4c_n_d_sub3_party_setup(&mut self, ab: u32, cd: u32, ef: u32) {
        self.schedule = Some((ab, cd, ef));
    }
}

/// Build `[4C, D3, expiry: u16, below: u16, duration: u32, threshold: u32]`.
fn schedule_insn(expiry: u16, below: u16, duration: u32, threshold: u32) -> Vec<u8> {
    let mut v = vec![0x4C, 0xD3];
    v.extend_from_slice(&expiry.to_le_bytes());
    v.extend_from_slice(&below.to_le_bytes());
    v.extend_from_slice(&duration.to_le_bytes());
    v.extend_from_slice(&threshold.to_le_bytes());
    v
}

/// Seed a scheduler from the installer's operand triple, the way the retail
/// globals are laid out: `_DAT_800845C0 = ab` (expiry in the high half, below
/// in the low half), `_DAT_800845B8`/`_DAT_800845A0 = cd` (duration),
/// `_DAT_800845BC = ef` (threshold).
fn timer_from_schedule(cd: u32, ef: u32) -> EscapeTimer {
    EscapeTimer {
        remaining: cd as i32,
        warn_threshold: ef as i32,
        armed: true,
    }
}

#[test]
fn installer_operands_reach_the_host_hook() {
    // The retail `chitei2` values: expiry flag 0x4C7, duration 2400,
    // threshold 910 (docs/subsystems/script-vm-menuctrl.md).
    let code = schedule_insn(0x04C7, 0x0123, 2400, 910);
    let mut host = CaptureHost::default();
    let mut ctx = FieldCtx::default();

    let r = step(&mut host, &mut ctx, &code, 0);
    // 14-byte instruction: PC advances past the whole record.
    assert_eq!(r, StepResult::Advance { next_pc: 0xE });

    let (ab, cd, ef) = host.schedule.expect("0x4C 0xD3 must reach the host hook");
    assert_eq!(ab >> 16, 0x04C7, "expiry flag lands in the high half");
    assert_eq!(ab & 0xFFFF, 0x0123, "below flag lands in the low half");
    assert_eq!(cd, 2400);
    assert_eq!(ef, 910);
}

#[test]
fn scheduler_fires_below_then_expiry_from_installed_operands() {
    let code = schedule_insn(0x04C7, 0x0123, 2400, 910);
    let mut host = CaptureHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &code, 0);
    let (ab, cd, ef) = host.schedule.expect("scheduled");

    let mut timer = timer_from_schedule(cd, ef);

    // Well above the threshold: nothing fires.
    let ev = timer.tick(1000, ab, false);
    assert_eq!(timer.remaining, 1400);
    assert_eq!(ev.warning_flag, None);
    assert_eq!(ev.expiry_flag, None);

    // Crossing the threshold fires the below-flag only.
    let ev = timer.tick(600, ab, false);
    assert_eq!(timer.remaining, 800);
    assert_eq!(ev.warning_flag, Some(0x0123));
    assert_eq!(ev.expiry_flag, None);
    assert!(timer.armed);

    // Running out fires the expiry flag and disarms.
    let ev = timer.tick(900, ab, false);
    assert_eq!(ev.expiry_flag, Some(0x04C7));
    assert!(!timer.armed);
}

#[test]
fn a_busy_frame_freezes_the_countdown() {
    let code = schedule_insn(0x04C7, 0x0123, 60, 30);
    let mut host = CaptureHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &code, 0);
    let (ab, cd, ef) = host.schedule.expect("scheduled");

    let mut timer = timer_from_schedule(cd, ef);
    let ev = timer.tick(60, ab, true);
    assert_eq!(timer.remaining, 60, "busy frames do not drain the counter");
    assert_eq!(ev.warning_flag, None);
    assert_eq!(ev.expiry_flag, None);
}

#[test]
fn hud_decomposition_matches_the_remaining_count() {
    // 2400 frames = 40 s at 60 Hz -> 0:40.00
    let timer = EscapeTimer {
        remaining: 2400,
        warn_threshold: 910,
        armed: true,
    };
    assert_eq!(timer.hud_fields(), (0, 40, 0));
}
