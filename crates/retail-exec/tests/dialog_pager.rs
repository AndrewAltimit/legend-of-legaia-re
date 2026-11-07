//! The field dialog pager `FUN_801D84D0` (PROT 0897) call by call against the
//! panel's row window: typewriter pacing, the short-row hold, scrolling, page
//! turns and confirm taps, over generated pages at every frame step and window
//! size. The script feed `_DAT_801F3538` is the test's; transitional load states
//! are dropped on both sides, so what compares is the record of states the
//! player can see. Text is generated; no disc text is read.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dialog::dialog::OwnedDialogPanel;
use legaia_engine_dialog::dialog_window::WindowPhase;
use psx_retail_exec::{Cpu, Stop};
use std::sync::Arc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PAGER: u32 = 0x801D_84D0;
const STATE: u32 = 0x801F_2734;
const SCROLL: u32 = 0x801F_2738;
const ROWS_MAX: u32 = 0x801F_2740;
const INSTANT: u32 = 0x801F_2744;
const COUNTER: u32 = 0x801F_2748;
const RAMP: u32 = 0x801F_274C;
const GONE: u32 = 0x801F_273C;
const SPEED: u32 = 0x801F_2750;
const PACE: u32 = 0x801F_2754;
const ACC: u32 = 0x801F_2758;
const HOLD: u32 = 0x801F_275C;
const LAST: u32 = 0x801F_3530;
const ON_PAGE: u32 = 0x801F_3534;
const FEED: u32 = 0x801F_3538;
const TABLE: u32 = 0x801F_3540;
const FRAME_STEP: u32 = 0x1F80_0393;
const PRESSED: u32 = 0x8007_B874;
const CONFIRM_A: u32 = 0x8008_46D0;
const CONFIRM_B: u32 = 0x8008_46D4;
const TEXT: u32 = 0x8014_0000;
const ACTOR: u32 = 0x8015_0000;

/// `(state, counter, acc, hold, rows on page, scroll, speed, row leads)`.
type Rec = (u8, i32, i32, i32, i32, i32, i32, Vec<usize>);

fn gen_text(rng: &mut Rng) -> (Vec<u8>, Vec<usize>) {
    let mut out = Vec::new();
    let mut leads = Vec::new();
    let pages = 1 + rng.below(4);
    for p in 0..pages {
        for _ in 0..(1 + rng.below(6)) {
            leads.push(out.len());
            out.push(0x1F);
            let n = [1u32, 3, 11, 25, 33, 34, 35, 40][rng.below(8) as usize];
            for _ in 0..n {
                out.push(0x41 + rng.below(26) as u8);
            }
            out.push(0);
        }
        out.push(if p + 1 < pages {
            if rng.below(3) == 0 { 0x48 } else { 0x24 }
        } else {
            0x25
        });
    }
    out.push(0);
    (out, leads)
}

fn retail_record(cpu: &mut Cpu) -> Rec {
    let mut leads = Vec::new();
    for i in 0..16 {
        let p = cpu.mem.rd32(TABLE + i * 4);
        if p != 0 {
            leads.push((p - TEXT) as usize);
        }
    }
    (
        cpu.mem.rd32(STATE) as u8,
        cpu.mem.rd32(COUNTER) as i32,
        cpu.mem.rd32(ACC) as i32,
        cpu.mem.rd32(HOLD) as i32,
        cpu.mem.rd32(ON_PAGE) as i32,
        cpu.mem.rd32(SCROLL) as i32,
        cpu.mem.rd32(SPEED) as i32,
        leads,
    )
}

fn port_record(p: &OwnedDialogPanel) -> Option<Rec> {
    let w = p.window()?;
    Some((
        match w.phase {
            WindowPhase::PressTurn => 3,
            phase => phase.retail_state(),
        },
        w.pacer.counter,
        w.pacer.acc,
        w.pacer.hold,
        w.rows_on_page,
        w.scroll,
        w.speed,
        p.window_row_leads(),
    ))
}

const SEEN: [u8; 6] = [0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x19];

#[test]
fn the_pager_pages_as_retail_pages() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    for f in [
        0x8003_6888u32,
        0x8003_D2C4,
        0x8002_C69C,
        0x8002_B994,
        0x8003_4B6C,
        0x8004_31D0,
        0x8004_313C,
        0x8003_5B50,
        0x8003_CE9C,
        0x8003_CE64,
        0x8003_CE34,
        0x8003_CE08,
        0x8003_CF04,
        0x8002_43F0,
        0x8002_0DE0,
        0x8005_6798,
        0x801D_E190,
        0x801D_A390,
        0x801D_9E1C,
    ] {
        cpu.stub(f, 0);
    }
    cpu.mem.wr32(CONFIRM_A, 0x40);
    cpu.mem.wr32(CONFIRM_B, 0x20);
    let mut rng = Rng(0x5041_4745);
    let (mut waits, mut taps) = (0, 0);
    for scenario in 0..1500 {
        let (text, _) = gen_text(&mut rng);
        let dt = [1u8, 2, 2, 3, 5][rng.below(5) as usize];
        let capacity = [3u32, 3, 3, 1, 2, 5][rng.below(6) as usize];
        let tap_odds = [0u32, 0, 30, 8][rng.below(4) as usize];
        cpu.mem.load(TEXT, &text);
        cpu.mem.wr8(TEXT + text.len() as u32, 0);
        cpu.mem.wr8(FRAME_STEP, dt);
        // State 2 on a fresh box, as the reset and the first feed leave it.
        for i in 0..16 {
            cpu.mem.wr32(TABLE + i * 4, 0);
        }
        for a in [
            SCROLL, COUNTER, ACC, HOLD, LAST, ON_PAGE, INSTANT, RAMP, GONE,
        ] {
            cpu.mem.wr32(a, 0);
        }
        cpu.mem.wr32(ROWS_MAX, capacity);
        cpu.mem.wr32(PACE, 1);
        cpu.mem.wr32(STATE, 2);
        cpu.mem.wr32(FEED, TEXT);

        let mut port = OwnedDialogPanel::at_segment(Arc::new(text.clone()), 0);
        port.frame_step = dt;
        port.set_window_format(capacity as usize, false);
        let mut retail_prev: Option<Rec> = None;
        'run: for call in 0..6000 {
            let tap = tap_odds != 0 && rng.below(tap_odds) == 0;
            let state = cpu.mem.rd32(STATE) as u8;
            let waiting = state == 0x19;
            let wait_press = waiting && rng.below(6) == 0;
            let press = tap || wait_press;
            // One retail call, until it shows a state the player can see.
            cpu.mem.wr32(PRESSED, if press { 0x40 } else { 0 });
            // The draw's primitive cursor, so it never climbs into code.
            cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
            assert_eq!(
                cpu.call(PAGER, &[ACTOR]),
                Stop::Returned,
                "{:x?}",
                cpu.calls
            );
            cpu.mem.wr32(PRESSED, 0);
            // The script feed sees a free state as the call returns.
            feed(&mut cpu, &text, &retail_prev);
            let mut guard = 0;
            let mut retail_calls = 1;
            while !SEEN.contains(&(cpu.mem.rd32(STATE) as u8)) && guard < 256 {
                guard += 1;
                let st = cpu.mem.rd32(STATE);
                if st == 0
                    || st == 9 && text[find_dispatch(&text, &retail_record(&mut cpu))] == 0x25
                {
                    break 'run;
                }
                cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
                assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
                feed(&mut cpu, &text, &retail_prev);
                retail_calls += 1;
            }
            let retail = retail_record(&mut cpu);

            // The same press on the panel, one call's worth of vsyncs.
            // A host hands a waiting page's press over after that frame's call.
            let turn = press && port.is_waiting_for_input();
            if press && !turn {
                port.confirm_while_typing();
            }
            let mut ours;
            let mut port_calls = 0;
            loop {
                port_calls += 1;
                for _ in 0..dt {
                    port.tick();
                }
                if port_calls == 1 && turn {
                    port.advance_page();
                }
                ours = port_record(&port);
                if port_calls > 256 || ours.as_ref().is_some_and(|r| SEEN.contains(&r.0)) {
                    break;
                }
            }
            let ours = ours.expect("a window");
            assert_eq!(
                retail, ours,
                "scenario {scenario} call {call} dt {dt} cap {capacity} press {press} (state before {state:#x})"
            );
            assert_eq!(
                retail_calls, port_calls,
                "scenario {scenario} dt {dt}: calls"
            );
            waits += usize::from(retail.0 == 0x19);
            taps += usize::from(tap);
            retail_prev = Some(retail);
            if port.is_done() {
                break;
            }
        }
    }
    assert!(waits > 1000 && taps > 1000, "{waits} {taps}");
}

/// The field VM's half: a free pager state takes the next text pointer, the
/// lead past the dispatch byte.
fn feed(cpu: &mut Cpu, text: &[u8], prev: &Option<Rec>) {
    let st = cpu.mem.rd32(STATE);
    if matches!(st, 1 | 4 | 7) {
        let at = find_dispatch(text, &prev.clone().unwrap_or_default());
        cpu.mem.wr32(FEED, TEXT + at as u32 + 1);
        cpu.mem.wr32(STATE, st + 1);
    }
}

/// The dispatch byte after the last row a record shows.
fn find_dispatch(text: &[u8], rec: &Rec) -> usize {
    let lead = rec.7.last().copied().unwrap_or(0);
    let mut at = lead;
    while at < text.len() && text[at] != 0 {
        at += 1;
    }
    at + 1
}
