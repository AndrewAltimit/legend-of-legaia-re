//! The title overlay tick `FUN_801DD35C` (PROT 0899)'s attract-idle arm
//! (sub-mode `0x10`, `0x801DDB74..0x801DDCF4`) frame by frame against
//! `legaia_engine_title::title_overlay::TitleMenuState::step`: random pad
//! edges and holds over random countdowns, every call outside the routine
//! stubbed. Compared each frame: the row counter `_DAT_8007B820`, the
//! countdown `0x801EF16C`, the cue `0x8007B6D8`, and the arm's exits - the
//! confirmed row's sub-mode (`0x16` for row 0, `0x18` for row 1) and the
//! attract hand-off (game mode `0x1A`).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_title::title_overlay::{TitleMenuEvent, TitleMenuState};
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801D_D35C;
const TICK_END: u32 = TICK + 12104;
const SUB_MODE: u32 = 0x801F_0204;
const ROW_COUNTER: u32 = 0x8007_B820;
const COUNTDOWN: u32 = 0x801E_F16C;
const CURSOR_PAD: u32 = 0x801E_F114;
const PRESS: u32 = 0x8007_B874;
const HELD: u32 = 0x8007_B850;
const CUE: u32 = 0x8007_B6D8;
const GAME_MODE: u32 = 0x8007_B83C;
const FRAME_STEP: u32 = 0x1F80_0393;
/// The tick's once-only setup latch: set, the per-frame body runs.
const SET_UP: u32 = 0x801F_0230;
/// The word OR-ed into the pad edge the cursor reads.
const PAD_EXTRA: u32 = 0x8007_B93C;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;

#[test]
fn the_title_menu_steps_as_retail_does() {
    differential!(prot(899, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((TICK, TICK_END));
    // The sub-mode -> case identity `FUN_801E38D0` sits past the routine.
    cpu.hook(0x801E_38D0, |r, _| {
        r.r[psx_retail_exec::V0] = r.r[psx_retail_exec::A0]
    });
    cpu.mem.wr32(SET_UP, 1);
    cpu.mem.wr32(PAD_EXTRA, 0);
    let mut rng = Rng(0x5449_544C);
    let (mut confirmed, mut fired, mut moved) = (0, 0, 0);
    for run in 0..400 {
        let mut port = TitleMenuState::new();
        port.countdown = [0x5DC, 0x20, 0x12, 0x10, 3][rng.below(5) as usize];
        port.row_counter = rng.below(2) as i32;
        cpu.mem.wr32(COUNTDOWN, port.countdown as u32);
        cpu.mem.wr32(ROW_COUNTER, port.row_counter as u32);
        for frame in 0..30 {
            let edge: u16 = match rng.below(8) {
                0..=3 => 0,
                4 => 0x4000,
                5 => 0x1000,
                6 => [0x0800, 0x0004, 0x0040][rng.below(3) as usize],
                _ => rng.next() as u16,
            };
            let held: u16 = if rng.below(6) == 0 {
                rng.next() as u16
            } else {
                0
            };
            let step = 1 + rng.below(3) as u8;
            cpu.mem.wr32(SUB_MODE, 0x10);
            cpu.mem.wr32(CURSOR_PAD, u32::from(edge));
            cpu.mem.wr32(PRESS, u32::from(edge));
            cpu.mem.wr32(HELD, u32::from(held));
            cpu.mem.wr8(FRAME_STEP, step);
            cpu.mem.wr16(CUE, 0xAAAA);
            cpu.mem.wr16(GAME_MODE, 0);
            cpu.mem.wr32(POOL_PTR, 0x8014_0000);
            cpu.mem.wr32(OT_PTR, 0x8016_0000);
            assert_eq!(cpu.call(TICK, &[0, 0]), Stop::Returned);
            let events = port.step(edge, held, step, 2);
            let ctx = format!("run {run} frame {frame} edge {edge:#x} held {held:#x}");
            assert_eq!(
                cpu.mem.rd32(ROW_COUNTER) as i32,
                port.row_counter,
                "{ctx}: the row"
            );
            let cue = cpu.mem.rd16(CUE);
            assert_eq!((cue != 0xAAAA).then_some(cue), port.sfx, "{ctx}: the cue");
            let row_pick = events.iter().find_map(|e| match e {
                TitleMenuEvent::Confirmed { row } => Some(*row),
                _ => None,
            });
            let fires = events.contains(&TitleMenuEvent::AttractFired);
            let sub = cpu.mem.rd32(SUB_MODE);
            match row_pick {
                Some(0) => assert_eq!(sub, 0x16, "{ctx}"),
                Some(_) => assert_eq!(sub, 0x18, "{ctx}"),
                None if !fires => assert_eq!(sub, 0x10, "{ctx}"),
                None => {}
            }
            if row_pick == Some(0) {
                confirmed += 1;
                break;
            }
            assert_eq!(
                cpu.mem.rd32(COUNTDOWN) as i32,
                port.countdown,
                "{ctx}: the countdown"
            );
            assert_eq!(cpu.mem.rd16(GAME_MODE) == 0x1A, fires, "{ctx}: the attract");
            moved += usize::from(
                events
                    .iter()
                    .any(|e| matches!(e, TitleMenuEvent::CursorMoved { .. })),
            );
            if row_pick.is_some() {
                confirmed += 1;
                break;
            }
            if fires {
                fired += 1;
                break;
            }
        }
    }
    assert!(
        confirmed > 50 && fired > 50 && moved > 200,
        "{confirmed} {fired} {moved}"
    );
}

/// The title tick's state words, as `TitleTickState` names them.
mod tick_words {
    pub const MENU_INDEX: u32 = 0x801F_0200;
    pub const LINEAR: u32 = 0x801F_01FC;
    pub const CURSOR_X: u32 = 0x801F_01F4;
    pub const CURSOR_Y: u32 = 0x801F_01F8;
    pub const SLIDER_DIR: u32 = 0x801F_01E0;
    pub const PREV: u32 = 0x801F_0208;
    pub const ATTRACT_DELAY: u32 = 0x8007_BAB4;
    pub const PREROLL: u32 = 0x801E_F120;
    pub const FADE: u32 = 0x801E_F154;
    pub const PANEL_FADE: u32 = 0x801E_F1A4;
    pub const SLIDER_X: u32 = 0x801E_F14C;
    pub const OP_KIND: u32 = 0x801E_F158;
    pub const SAVE_TIMER: u32 = 0x801E_F118;
    pub const CHECK_TIMER: u32 = 0x801E_F124;
    pub const LAUNCH_TIMER: u32 = 0x801E_F104;
    pub const REMAP: u32 = 0x801E_F18C;
    pub const ENTRY: u32 = 0x8007_BB00;
    pub const MENU_ROW: u32 = 0x8007_B834;
    pub const MAIN_ROW: u32 = 0x801E_F164;
    pub const PROBED_ROW: u32 = 0x801E_F168;
    pub const SLOT_INDEX: u32 = 0x8007_B7CC;
    pub const ALPHA_A: u32 = 0x801E_F190;
    pub const ALPHA_B: u32 = 0x801E_F194;
    pub const ALPHA_C: u32 = 0x801E_F1A0;
    pub const ALPHA_GATE: u32 = 0x801E_F198;
    pub const FAULT_FLASH: u32 = 0x801E_F1A8;
    pub const SWEEP_DIR: u32 = 0x801E_F15C;
    pub const SWEEP: u32 = 0x801E_F160;
    pub const FRAMES: u32 = 0x801E_F170;
    pub const CANCEL_ARM: u32 = 0x801E_F130;
    pub const PROBED: u32 = 0x801E_F108;
    pub const FAULT_SEEN: u32 = 0x801E_F12C;
    pub const FAULT_TIMER: u32 = 0x801E_F134;
    pub const MUSIC: u32 = 0x8007_B910;
    pub const FMV_ID: u32 = 0x8007_BA78;
    // The card words.
    pub const BUSY: u32 = 0x801E_F140;
    pub const ERROR: u32 = 0x801E_F10C;
    pub const RESULT: u32 = 0x801E_F13C;
    pub const LATCH: u32 = 0x801E_F144;
    pub const RETRIES: u32 = 0x801E_F0F8;
    pub const REMOVED: u32 = 0x801E_F148;
    pub const FAULT: u32 = 0x801F_0218;
    pub const SCAN_TOTAL: u32 = 0x801F_01E4;
    pub const SCAN_DONE: u32 = 0x801F_01D4;
    pub const SLOT_KIND: u32 = 0x801F_021C;
    pub const SCAN_REQUEST: u32 = 0x801F_0224;
    pub const BLOCK_COUNT: u32 = 0x801F_01BC;
    pub const HAS_BLOCKS: u32 = 0x801F_01F0;
    pub const READY: u32 = 0x801F_01DC;
    pub const OP_STATUS: u32 = 0x801F_329C;
    pub const DRAW_DONE: u32 = 0x801F_01D0;
    pub const GRACE: u32 = 0x801F_0228;
    pub const OP_PHASE: u32 = 0x801F_01C0;
    pub const OP_TIMER: u32 = 0x801F_3288;
    pub const OP_ATTEMPTS: u32 = 0x801F_01C4;
    pub const OP_RESULT: u32 = 0x801F_328C;
    pub const OP_REQUEST: u32 = 0x801F_0220;
    pub const OP_REPLY: u32 = 0x801F_3804;
    pub const PROBE: u32 = 0x801E_F180;
    pub const BUSY_GATE: u32 = 0x801E_F100;
    /// `state[-0xe8c]`, the loaded block mode `0x05` checksums.
    pub const BLOCK_PTR: u32 = 0x801E_F174;
    /// Test scratch: the block, `FUN_801E3F74`'s answer, and the log of
    /// `FUN_801E373C` calls (count, then the arguments).
    pub const BLOCK: u32 = 0x8010_0000;
    pub const SLOT_STATUS: u32 = 0x8011_0000;
    pub const RESET_LOG: u32 = 0x8011_0010;
    pub const LOADED: u32 = 0x8011_0008;
    /// Test scratch: each backdrop drawer's call count and last `a0`.
    pub const BACKDROP_LOG: u32 = 0x8011_0100;
}

/// Every word of `TitleTickState` and `TitleCardStatus` a frame can move,
/// paired retail-address to port-value, for one comparison.
fn tick_snapshot(
    s: &legaia_engine_title::title_overlay::TitleTickState,
    c: &legaia_engine_title::title_overlay::TitleCardStatus,
) -> Vec<(&'static str, u32, i32)> {
    use tick_words::*;
    vec![
        ("submode", SUB_MODE, i32::from(s.submode)),
        ("prev_submode", PREV, i32::from(s.prev_submode)),
        ("menu_index", MENU_INDEX, s.menu_index as i32),
        ("linear_cursor", LINEAR, s.linear_cursor),
        ("cursor_x", CURSOR_X, s.cursor_x),
        ("cursor_y", CURSOR_Y, s.cursor_y),
        ("row_counter", ROW_COUNTER, s.row_counter),
        ("countdown", COUNTDOWN, s.countdown),
        ("attract_delay", ATTRACT_DELAY, s.attract_delay),
        ("preroll", PREROLL, s.preroll),
        ("fade", FADE, s.fade),
        ("panel_fade", PANEL_FADE, s.panel_fade),
        ("slider_x", SLIDER_X, s.slider_x),
        ("slider_dir", SLIDER_DIR, s.slider_dir as i32),
        ("op_kind", OP_KIND, s.op_kind as i32),
        ("save_timer", SAVE_TIMER, s.save_timer),
        ("check_timer", CHECK_TIMER, s.check_timer),
        ("launch_timer", LAUNCH_TIMER, s.launch_timer),
        ("remap_gate", REMAP, s.remap_gate),
        ("entry_word", ENTRY, s.entry_word as i32),
        ("menu_row", MENU_ROW, s.menu_row),
        ("main_row", MAIN_ROW, s.main_row),
        ("probed_row", PROBED_ROW, s.probed_row),
        ("slot_index", SLOT_INDEX, s.slot_index),
        ("alpha_a", ALPHA_A, s.alpha_a),
        ("alpha_b", ALPHA_B, s.alpha_b),
        ("alpha_c", ALPHA_C, s.alpha_c),
        ("alpha_gate", ALPHA_GATE, s.alpha_gate),
        ("fault_flash", FAULT_FLASH, s.fault_flash),
        ("sweep_dir", SWEEP_DIR, s.sweep_dir),
        ("sweep", SWEEP, s.sweep),
        ("frame_counter", FRAMES, s.frame_counter),
        ("cancel_arm", CANCEL_ARM, s.cancel_arm),
        ("probed", PROBED, s.probed),
        ("fault_seen", FAULT_SEEN, s.fault_seen),
        ("fault_timer", FAULT_TIMER, s.fault_timer),
        ("music_level", MUSIC, s.music_level),
        ("busy", BUSY, c.busy),
        ("error", ERROR, c.error),
        ("result", RESULT, c.result),
        ("retry_latch", LATCH, c.retry_latch),
        ("retries", RETRIES, c.retries),
        ("removed", REMOVED, c.removed),
        ("fault", FAULT, c.fault),
        ("scan_total", SCAN_TOTAL, c.scan_total),
        ("scan_done", SCAN_DONE, c.scan_done),
        ("slot_kind", SLOT_KIND, c.slot_kind),
        ("scan_request", SCAN_REQUEST, c.scan_request),
        ("block_count", BLOCK_COUNT, c.block_count),
        ("has_blocks", HAS_BLOCKS, c.has_blocks),
        ("ready", READY, c.ready),
        ("op_status", OP_STATUS, c.op_status),
        ("draw_done", DRAW_DONE, c.draw_done),
        ("grace", GRACE, c.grace),
        ("op_phase", OP_PHASE, c.op_phase),
        ("op_timer", OP_TIMER, c.op_timer),
        ("op_attempts", OP_ATTEMPTS, c.op_attempts),
        ("op_result", OP_RESULT, c.op_result),
        ("op_request", OP_REQUEST, c.op_request),
        ("op_reply", OP_REPLY, c.op_reply),
        ("probe", PROBE, c.probe),
        ("busy_gate", BUSY_GATE, c.busy_gate),
    ]
}

/// A value near the thresholds the tick tests: zero, small, the fade
/// span, or anything.
fn near(rng: &mut Rng, picks: &[i32]) -> i32 {
    if rng.below(5) == 0 {
        rng.next() as i32 >> rng.below(31)
    } else {
        picks[rng.below(picks.len() as u32) as usize]
    }
}

#[test]
fn every_title_sub_mode_steps_as_retail_does() {
    use legaia_engine_title::title_overlay::{
        TitleCardStatus, TitleTickEffect, TitleTickPad, TitleTickState,
    };
    use tick_words::*;
    differential!(prot(899, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((TICK, TICK_END));
    let identity = |r: &mut psx_retail_exec::Regs, _: &mut psx_retail_exec::Memory| {
        r.r[psx_retail_exec::V0] = r.r[psx_retail_exec::A0]
    };
    // The sub-mode identity, and `FUN_801E1114`'s, whose card pass is the
    // driver's.
    cpu.hook(0x801E_38D0, identity);
    cpu.hook(0x801E_1114, identity);
    cpu.hook(0x801E_3F74, |r, m| {
        r.r[psx_retail_exec::V0] = m.rd32(SLOT_STATUS)
    });
    cpu.hook(0x801E_38D8, |r, _| r.r[psx_retail_exec::V0] = 0x1234);
    cpu.hook(0x801E_373C, |r, m| {
        let n = m.rd32(RESET_LOG);
        m.wr32(RESET_LOG + 4 + 4 * n, r.r[psx_retail_exec::A0]);
        m.wr32(RESET_LOG, n + 1);
    });
    // The tail's two backdrop drawers, each handed the shade in `a0`.
    for (k, drawer) in [0x801E_0418u32, 0x801E_02A4].into_iter().enumerate() {
        let log = BACKDROP_LOG + 8 * k as u32;
        cpu.hook(drawer, move |r, m| {
            let n = m.rd32(log);
            m.wr32(log, n + 1);
            m.wr32(log + 4, r.r[psx_retail_exec::A0]);
        });
    }
    for load in [0x8003_E6BC, 0x8003_EB98] {
        cpu.hook(load, |_, m| m.wr32(LOADED, 1));
    }
    // `FUN_80056748`, the name compare on the SAVE route: either answer.
    cpu.hook(0x8005_6748, |r, m| {
        r.r[psx_retail_exec::V0] = m.rd32(SLOT_STATUS) & 1
    });
    cpu.mem.wr32(SET_UP, 1);
    let mut rng = Rng(0x5449_434B);
    let mut seen = [0u32; 0x1B];
    let mut backdrops = 0usize;
    for run in 0..8000 {
        let mut s = TitleTickState::cold_boot();
        s.submode = match rng.below(12) {
            0 => [0x01, 0x19, 0x1A, 0xFF][rng.below(4) as usize],
            _ => rng.below(0x19) as u8,
        };
        s.prev_submode = if rng.below(2) == 0 { s.submode } else { 0 };
        s.arg1 = [0, 0, 0, 1, 2][rng.below(5) as usize];
        s.entry_word = [0, 1, 2, 1][rng.below(4) as usize];
        s.menu_index = rng.below(2);
        s.linear_cursor = near(&mut rng, &[0, 1, 2, -1]);
        s.cursor_x = near(&mut rng, &[0, 2, 3, 4, 5, -1]);
        s.cursor_y = near(&mut rng, &[0, 1, 2, 3, -1, 25]);
        s.row_counter = near(&mut rng, &[0, 1, 2]);
        s.countdown = near(&mut rng, &[0x5DC, 0x11, 0x10, 1, 0]);
        s.attract_delay = near(&mut rng, &[0, 8, 0x100, 3]);
        s.preroll = near(&mut rng, &[0, 0x80, 0x1000, 0xF80, -1]);
        s.fade = near(&mut rng, &[0, 0x100, 0x1000, 0x1100, 0x1200, 0x11F0]);
        s.panel_fade = near(&mut rng, &[0, 0x100, 0x1000, 0xF00, 0x200]);
        s.slider_x = near(&mut rng, &[0x100, 0x2C, 0x30, 0x20, 0x2D]);
        s.slider_dir = rng.below(4);
        s.op_kind = rng.below(3);
        s.save_timer = near(&mut rng, &[0, 0x4B0, 0x4B1]);
        s.check_timer = near(&mut rng, &[0, 0x258, 0x259, 0x257]);
        s.launch_timer = near(&mut rng, &[0, 0x59, 0x5A, 0x5B, 0x10]);
        s.remap_gate = near(&mut rng, &[-1, 0, 1]);
        s.menu_row = rng.below(2) as i32;
        s.main_row = near(&mut rng, &[0, 1, 2]);
        s.probed_row = near(&mut rng, &[-1, 0, 1]);
        s.slot_index = near(&mut rng, &[0, 7, 14, 0x80]);
        s.alpha_a = near(&mut rng, &[0, 0x1000, 0xF80]);
        s.alpha_b = near(&mut rng, &[0, 0x100, 0x1000, 0xF80]);
        s.alpha_c = near(&mut rng, &[0, 0x100, 0x1000, 0xFF0, 1]);
        s.alpha_gate = rng.below(2) as i32;
        s.fault_flash = near(&mut rng, &[0, 1, 0x10, -1]);
        s.sweep_dir = near(&mut rng, &[0, 1, 2]);
        s.sweep = near(&mut rng, &[0, 0x100, 0x1000, 0xF80]);
        s.frame_counter = rng.next() as i32;
        s.cancel_arm = rng.below(2) as i32;
        s.probed = rng.below(2) as i32;
        s.fault_seen = rng.below(2) as i32;
        s.fault_timer = near(&mut rng, &[0, 0xB4, 0xB5]);
        s.music_level = rng.next() as i32;
        let mut c = TitleCardStatus::default();
        let frames = 1 + rng.below(12);
        for frame in 0..frames {
            // Re-deal the card's words now and then, as the driver moves
            // them between ticks.
            if frame == 0 || rng.below(3) == 0 {
                let flag = |rng: &mut Rng| [0, 0, 0, 1, 2, -1][rng.below(6) as usize];
                c.busy = flag(&mut rng);
                c.error = flag(&mut rng);
                c.result = flag(&mut rng);
                c.retry_latch = flag(&mut rng);
                c.retries = near(&mut rng, &[0, 3, 4, 5]);
                c.removed = flag(&mut rng);
                c.fault = flag(&mut rng);
                c.scan_total = near(&mut rng, &[0, 1, 3]);
                c.scan_done = near(&mut rng, &[0, 1, 3]);
                c.slot_kind = [3, 3, 0, 1][rng.below(4) as usize];
                c.scan_request = flag(&mut rng);
                c.block_count = near(&mut rng, &[0, 0, 1, 2, 3]);
                c.has_blocks = flag(&mut rng);
                c.ready = flag(&mut rng);
                c.op_status = [0, 0, 1, 2][rng.below(4) as usize];
                c.draw_done = [0, 0x1000, 0x800][rng.below(3) as usize];
                c.grace = near(&mut rng, &[0, 0, 1, 0x78]);
                c.op_phase = rng.below(0x17) as i32;
                c.op_timer = near(&mut rng, &[0, 1, 0x5A, 0x5B, 0x78]);
                c.op_attempts = near(&mut rng, &[0, 2, 3]);
                c.op_result = flag(&mut rng);
                c.op_request = [0, 0, 3, 1][rng.below(4) as usize];
                c.op_reply = [0, -2, -1][rng.below(3) as usize];
                c.probe = flag(&mut rng);
                c.slot_status = 1 + rng.below(4) as i32;
                c.verify_ok = rng.below(2) == 0;
                c.busy_gate = flag(&mut rng);
                // A quiet card, so the arms behind all six flags run.
                if rng.below(3) == 0 {
                    c.busy = 0;
                    c.error = 0;
                    c.removed = 0;
                    c.fault = 0;
                    c.block_count = 0;
                    c.busy_gate = 0;
                    c.grace = 0;
                    c.probe = 1;
                    c.scan_request = 0;
                    c.slot_kind = 3;
                }
            }
            let edge: u16 = match rng.below(8) {
                0..=2 => 0,
                3 => [0x0044, 0x0021, 0x0004, 0x0040, 0x0001, 0x0020][rng.below(6) as usize],
                4 => [0x4000, 0x1000, 0x8000, 0x2000][rng.below(4) as usize],
                5 => 0x0800 | (rng.next() as u16 & 0x00F5),
                _ => rng.next() as u16,
            };
            let extra: u16 = if rng.below(5) == 0 {
                rng.next() as u16
            } else {
                0
            };
            let held: u16 = if rng.below(4) == 0 {
                rng.next() as u16
            } else {
                0
            };
            let step = rng.below(4) as u8;
            if frame == 0 {
                for (_, addr, v) in tick_snapshot(&s, &c) {
                    cpu.mem.wr32(addr, v as u32);
                }
            } else {
                // The card words the driver re-dealt.
                for (_, addr, v) in tick_snapshot(&s, &c).into_iter().skip(37) {
                    cpu.mem.wr32(addr, v as u32);
                }
            }
            cpu.mem.wr32(SLOT_STATUS, c.slot_status as u32);
            // `state[+0x210]`, the block index `0x05` marks a directory
            // byte under; the port keeps no directory.
            cpu.mem.wr32(0x801F_0210, rng.below(15));
            cpu.mem.wr32(BLOCK_PTR, BLOCK);
            cpu.mem
                .wr32(BLOCK + 0x1FFC, if c.verify_ok { 0x1234 } else { 0x1235 });
            cpu.mem.wr32(RESET_LOG, 0);
            cpu.mem.wr32(LOADED, 0);
            for w in 0..4 {
                cpu.mem.wr32(BACKDROP_LOG + 4 * w, 0);
            }
            // `0x8007B8C2`, which picks the cold boot's loader; both load.
            cpu.mem.wr16(0x8007_B8C2, rng.below(2) as u16);
            cpu.mem.wr32(PRESS, u32::from(edge));
            cpu.mem.wr32(PAD_EXTRA, u32::from(extra));
            cpu.mem.wr32(HELD, u32::from(held));
            cpu.mem.wr8(FRAME_STEP, step);
            cpu.mem.wr16(CUE, 0xAAAA);
            cpu.mem.wr16(GAME_MODE, 0);
            cpu.mem.wr16(FMV_ID, 0xAAAA);
            cpu.mem.wr32(POOL_PTR, 0x8014_0000);
            cpu.mem.wr32(OT_PTR, 0x8016_0000);
            let before = s;
            seen[usize::from(s.submode.min(0x1A))] += 1;
            let stop = cpu.call(TICK, &[0, s.arg1]);
            assert_eq!(stop, Stop::Returned, "run {run} frame {frame}: {before:?}");
            let pad = TitleTickPad {
                edge,
                nav: edge | extra,
                held,
                frame_scalar: step,
            };
            let fx = s.step(pad, &mut c);
            let ctx = format!(
                "run {run} frame {frame}: mode {:#x} arg {} entry {} edge {edge:#x} nav {:#x} held {held:#x} step {step} card {c:?} before {before:?}",
                before.submode,
                before.arg1,
                before.entry_word,
                edge | extra
            );
            for (name, addr, v) in tick_snapshot(&s, &c) {
                assert_eq!(cpu.mem.rd32(addr) as i32, v, "{ctx}: {name}");
            }
            let cue = cpu.mem.rd16(CUE);
            let last_cue = fx.iter().rev().find_map(|e| match e {
                TitleTickEffect::Sfx(c) => Some(*c),
                _ => None,
            });
            assert_eq!((cue != 0xAAAA).then_some(cue), last_cue, "{ctx}: the cue");
            let resets: Vec<u32> = (0..cpu.mem.rd32(RESET_LOG))
                .map(|k| cpu.mem.rd32(RESET_LOG + 4 + 4 * k))
                .collect();
            let port_resets: Vec<u32> = fx
                .iter()
                .filter_map(|e| match e {
                    TitleTickEffect::CardReset { arg } => Some(*arg),
                    _ => None,
                })
                .collect();
            assert_eq!(resets, port_resets, "{ctx}: the card resets");
            // The backdrop: drawn by both drawers at one shade, or by neither.
            let drawn: Vec<(u32, u32)> = (0..2)
                .map(|k| {
                    let log = BACKDROP_LOG + 8 * k;
                    (cpu.mem.rd32(log), cpu.mem.rd32(log + 4))
                })
                .collect();
            let port_shade = fx.iter().find_map(|e| match e {
                TitleTickEffect::Backdrop { shade } => Some(u32::from(*shade)),
                _ => None,
            });
            let want = match port_shade {
                Some(v) => vec![(1, v), (1, v)],
                None => vec![(0, 0), (0, 0)],
            };
            assert_eq!(drawn, want, "{ctx}: the backdrop");
            backdrops += usize::from(port_shade.is_some_and(|v| v != 0 && v != 0xFF));
            assert_eq!(
                cpu.mem.rd32(LOADED) != 0,
                fx.contains(&TitleTickEffect::LoadTitleAssets),
                "{ctx}: the asset load"
            );
            let mode = cpu.mem.rd16(GAME_MODE);
            let want = if fx
                .iter()
                .any(|e| matches!(e, TitleTickEffect::LaunchGame { .. }))
            {
                2
            } else if fx
                .iter()
                .any(|e| matches!(e, TitleTickEffect::FireAttract { .. }))
            {
                assert_eq!(cpu.mem.rd16(FMV_ID), 0, "{ctx}: the attract's fmv");
                0x1A
            } else {
                0
            };
            assert_eq!(mode, want, "{ctx}: the master mode");
        }
    }
    assert!(seen.iter().take(0x19).all(|&n| n > 50), "{seen:?}");
    assert!(backdrops > 1000, "{backdrops} part-lit backdrops");
}
