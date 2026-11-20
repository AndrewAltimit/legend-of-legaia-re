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
        if p + 1 < pages {
            out.push(if rng.below(3) == 0 { 0x48 } else { 0x24 });
        } else {
            // The last page ends the box: `0x25`, `4C FF`, or a byte outside
            // the dispatch table.
            match rng.below(5) {
                0 => out.extend([0x4C, 0xFF]),
                1 => out.extend([0x4C, 0x12]),
                2 => out.push(0x30),
                _ => out.push(0x25),
            }
        }
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

/// The overlay loaded, and every call the pager makes stubbed but the string
/// length `FUN_8003CA38`; the confirm binding is `0x40`.
fn stub_pager(cpu: &mut Cpu, d: &legaia_disc_patch::disc::DiscPatcher) {
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
}

const SEEN: [u8; 6] = [0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x19];

#[test]
fn the_pager_pages_as_retail_pages() {
    differential!(prot(897, PAGER));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    stub_pager(&mut cpu, &d);
    use legaia_engine_dialog::cursor_sprite::CursorSprites;
    use legaia_engine_dialog::dialog::pager_reads_pad;
    use legaia_engine_dialog::dialog_window::WINDOW_ROWS;
    use legaia_engine_field::inline_dialogue::HeldBox;
    // Page marks (`FUN_8002B994` kind `1`) a call drew.
    let marks = std::rc::Rc::new(std::cell::Cell::new(0u32));
    {
        let m = marks.clone();
        cpu.hook(SPRITE, move |r, _| {
            if r.r[psx_retail_exec::A0] == 1 {
                m.set(m.get() + 1);
            }
        });
    }
    let mut rng = Rng(0x5041_4745);
    let (mut waits, mut taps, mut autos) = (0, 0, 0);
    let mut ends = std::collections::BTreeMap::<u32, u32>::new();
    let mut idles = [0u32; 2];
    let mut resumes = 0usize;
    for scenario in 0..1500 {
        let (text, _) = gen_text(&mut rng);
        let dt = [1u8, 2, 2, 3, 5][rng.below(5) as usize];
        let capacity = [3u32, 3, 3, 1, 2, 5][rng.below(6) as usize];
        let tap_odds = [0u32, 0, 30, 8, 1][rng.below(5) as usize];
        let instant = rng.below(5) == 0;
        let mut auto: i16 = [0, 0, 0, 1, 6, 13, 40][rng.below(7) as usize];
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
        cpu.mem.wr32(INSTANT, u32::from(instant));
        // The speed a box before left; an instant first page keeps it.
        cpu.mem.wr32(SPEED, 0x24);
        cpu.mem.wr16(AUTO_PRESS, auto as u16);
        cpu.mem.wr32(PACE, 1);
        cpu.mem.wr32(STATE, 2);
        cpu.mem.wr32(FEED, TEXT);

        let mut port = OwnedDialogPanel::at_segment(Arc::new(text.clone()), 0);
        port.frame_step = dt;
        port.set_window_format(capacity as usize, instant);
        let mut cursors = CursorSprites::default();
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
            marks.set(0);
            assert_eq!(
                cpu.call(PAGER, &[ACTOR]),
                Stop::Returned,
                "{:x?}",
                cpu.calls
            );
            cpu.mem.wr32(PRESSED, 0);
            // The script feed sees a free state as the call returns - or a
            // call later, when the record runs ops first.
            let mut resumed = idle_call(&mut cpu, &text, &retail_prev, &mut rng, &mut idles);
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
                marks.set(0);
                assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
                resumed |= idle_call(&mut cpu, &text, &retail_prev, &mut rng, &mut idles);
                feed(&mut cpu, &text, &retail_prev);
                retail_calls += 1;
            }
            let retail = retail_record(&mut cpu);

            // The same press on the panel, one call's worth of vsyncs, unless
            // the automatic press is counting. A host hands a waiting page's
            // press, or the automatic one, over after that frame's call.
            let press = press && pager_reads_pad(auto);
            let turn = press && port.is_waiting_for_input();
            if press && !turn {
                port.confirm_while_typing();
            }
            let mut ours;
            let mut port_calls = 0;
            loop {
                port_calls += 1;
                for _ in 0..dt {
                    port.tick_at_auto_drawn(dt, &mut auto, &mut cursors);
                }
                let fired = port.take_auto_press();
                if port_calls == 1 && (turn || fired) {
                    if resumed {
                        // The record ran ops while the `0x24` box stood in
                        // the idle state `4`: the press left the held box,
                        // and the next line goes on in it as the runner
                        // feeds it (`HeldBox::next_line_panel`).
                        let lead = find_dispatch(&text, retail_prev.as_ref().unwrap()) + 1;
                        port = port
                            .held_after_press()
                            .resumed_at_segment(lead)
                            .expect("the held box goes on");
                    } else {
                        port.advance_page();
                    }
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
            // The resumed box spends no call on the press's state `3`: that
            // call falls in the frames the record runs ops.
            assert_eq!(
                retail_calls,
                port_calls + usize::from(resumed),
                "scenario {scenario} dt {dt}: calls"
            );
            resumes += usize::from(resumed);
            assert_eq!(
                cpu.mem.rd16(AUTO_PRESS) as i16,
                auto,
                "scenario {scenario} call {call}: the automatic press"
            );
            if retail.0 == 0x19 {
                assert_eq!(
                    marks.get() > 0,
                    port.advance_icon().is_some(),
                    "scenario {scenario} call {call}: the page mark (auto {auto})"
                );
                autos += usize::from(auto > 0);
            }
            waits += usize::from(retail.0 == 0x19);
            taps += usize::from(tap);
            retail_prev = Some(retail);
            if port.is_done() {
                // The press on the last page: the dispatch byte's state
                // (`0x801D8FD8..0x801D903C`), then the reset that state runs.
                let page =
                    legaia_mes::pack_page(&text, *retail_prev.as_ref().unwrap().7.first().unwrap())
                        .unwrap();
                let held =
                    HeldBox::after_press(&port, text.get(page.dispatch_at).map(|b| b & 0x7F));
                cpu.mem.wr16(AUTO_PRESS, 0);
                cpu.mem.wr32(PRESSED, 0x40);
                cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
                assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
                cpu.mem.wr32(PRESSED, 0);
                let want = match page.dispatch {
                    legaia_mes::Dispatch::End => 0,
                    legaia_mes::Dispatch::Terminate => 6,
                    _ => 9,
                };
                let at = format!("scenario {scenario}: {:?}", page.dispatch);
                assert_eq!(cpu.mem.rd32(STATE), want, "{at}");
                assert!(!held.until_next_line, "{at}");
                *ends.entry(want).or_default() += 1;
                // States `0` and `6` reset the window for the next box,
                // `9` collapses it first; every box starts at the default
                // format.
                cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
                assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
                if want != 9 {
                    assert_eq!(cpu.mem.rd32(STATE), want + 1, "{at}");
                    assert_eq!(cpu.mem.rd32(ROWS_MAX) as usize, WINDOW_ROWS, "{at}");
                    assert!((0..16).all(|i| cpu.mem.rd32(TABLE + i * 4) == 0), "{at}");
                    // The idle state the reset leaves, while the record runs
                    // on: no rows, as the held box of anything but `0x24`.
                    let st = cpu.mem.rd32(STATE);
                    cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
                    assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
                    assert_eq!(cpu.mem.rd32(STATE), st, "{at}");
                    assert!((0..16).all(|i| cpu.mem.rd32(TABLE + i * 4) == 0), "{at}");
                    idles[0] += 1;
                }
                break;
            }
        }
    }
    assert!(
        waits > 1000 && taps > 1000 && autos > 200,
        "{waits} {taps} {autos}"
    );
    assert!(
        ends.len() == 3 && ends.values().all(|&n| n > 50),
        "{ends:?}"
    );
    assert!(idles.iter().all(|&n| n > 50), "{idles:?}");
    assert!(resumes > 100, "{resumes}");
}

/// A pager call in a free state before the feed, as when the record runs
/// ops between two lines: the idle arm (`0x801D8708..0x801D8768`) clears the
/// row table in states `1` and `7` and keeps it in `4`, the state a `0x24`
/// continuation leaves. Against `HeldBox::after_press`, whose box goes on
/// drawing until the next line exactly for a `0x24`. Returns whether a
/// held `0x24` box (state `4`) idled, so the panel resumes it.
fn idle_call(
    cpu: &mut Cpu,
    text: &[u8],
    prev: &Option<Rec>,
    rng: &mut Rng,
    seen: &mut [u32; 2],
) -> bool {
    use legaia_engine_field::inline_dialogue::HeldBox;
    let st = cpu.mem.rd32(STATE);
    // A collapsing box's countdown (`_DAT_801F273C`) returns ahead of the
    // states, so no idle arm runs under it.
    if !matches!(st, 1 | 4 | 7) || cpu.mem.rd32(GONE) != 0 || rng.below(3) != 0 {
        return false;
    }
    let table: Vec<u32> = (0..16).map(|i| cpu.mem.rd32(TABLE + i * 4)).collect();
    cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
    assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
    assert_eq!(cpu.mem.rd32(STATE), st, "an idle state holds");
    let kept = (0..16).all(|i| cpu.mem.rd32(TABLE + i * 4) == table[i as usize]);
    let cleared = (0..16).all(|i| cpu.mem.rd32(TABLE + i * 4) == 0);
    let byte = text[find_dispatch(text, &prev.clone().unwrap_or_default())] & 0x7F;
    let panel = OwnedDialogPanel::at_segment(Arc::new(text.to_vec()), 0);
    let held = HeldBox::after_press(&panel, Some(byte));
    assert_eq!(held.until_next_line, st == 4, "state {st} byte {byte:#x}");
    assert!(
        if st == 4 { kept } else { cleared },
        "state {st}: the row table"
    );
    seen[usize::from(st == 4)] += 1;
    st == 4
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

const SCENE_PTR: u32 = 0x801C_6EA4;
const SCENE: u32 = 0x8016_0000;
const AUTO_PRESS: u32 = 0x8007_3F00;
const CUE: u32 = 0x8003_5B50;
const BOX_DRAW: u32 = 0x8002_C69C;
const SPRITE: u32 = 0x8002_B994;
const TEXT_DRAW: u32 = 0x8003_6888;
/// The pad word's Up and Down bits as the pager reads them (`andi 0x1000`,
/// `andi 0x4000` at `0x801D9454` / `0x801D9460`).
const PAD_UP: u32 = 0x1000;
const PAD_DOWN: u32 = 0x4000;

/// A prompt page of one to three short rows, then a picker: the open byte
/// (`0x2A` / `0x27` / `0x28` / `0x29`, sometimes with bit 7), the jump
/// table, a continuation (`0x24`, `0x25`, `0x48`, `4C FF`, or none, the
/// labels straight after the table), the labels, and one reply line per
/// option the table jumps to.
fn gen_picker_text(rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..(1 + rng.below(3)) {
        out.push(0x1F);
        for _ in 0..(1 + rng.below(12)) {
            out.push(0x41 + rng.below(26) as u8);
        }
        out.push(0);
    }
    let open_byte =
        [0x2Au8, 0x27, 0x28, 0x29][rng.below(4) as usize] | (rng.below(4) as u8 & 1) << 7;
    let n = match open_byte & 0x7F {
        0x28 => 3,
        0x29 => 4,
        _ => 2,
    };
    let open = out.len();
    out.push(open_byte);
    out.extend(std::iter::repeat_n(0, n * 2));
    match rng.below(5) {
        0 => out.push(0x24),
        1 => out.push(0x25),
        2 => out.push(0x48),
        3 => out.extend([0x4C, 0xFF]),
        _ => {}
    }
    for _ in 0..n {
        out.push(0x1F);
        for _ in 0..(1 + rng.below(8)) {
            out.push(0x41 + rng.below(26) as u8);
        }
        out.push(0);
    }
    for i in 0..n {
        let rel = (out.len() as i32 - (open + 1 + i * 2) as i32) as i16;
        out[open + 1 + i * 2..open + 3 + i * 2].copy_from_slice(&rel.to_le_bytes());
        out.extend([0x1F, 0x52, 0x45, 0, 0x25]);
    }
    out.push(0);
    out
}

/// What one pager call drew, from the hooked draw calls: every box frame
/// (`FUN_8002C69C`), every cursor sprite (`FUN_8002B994`: kind, x, y) and
/// every text run (`FUN_80036888`: lead, x, y).
#[derive(Default)]
struct Drawn {
    boxes: Vec<(i32, i32, i32, i32)>,
    sprites: Vec<(u32, i32, i32)>,
    texts: Vec<(usize, i32, i32)>,
    cues: Vec<u32>,
}

/// The picker states `0x11..=0x18` call by call against the panel's open
/// menu: the press in `0x19` that installs the entry state for each open
/// byte, the entry state's slide (`0x801D92F4..0x801D9418`), the active
/// state's cursor handler with its wrap or clamp and its move cue
/// (`0x801D941C..0x801D9514`), the commit (`0x801D9518..0x801D95A4`) and the
/// picker draw (`0x801D9A08..0x801D9BE4`). The panel is driven in the order
/// the world's dialogue drive runs it - the cursor moves before the pager
/// call, the commit after it - and the press, the slide words, the cursor,
/// the drawn box, hand and labels, the cues and the state the commit leaves
/// are compared each call.
#[test]
fn the_picker_states_step_as_retail() {
    differential!(prot(897, PAGER));
    use legaia_engine_field::inline_dialogue::HeldBox;
    use std::cell::RefCell;
    use std::rc::Rc;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    stub_pager(&mut cpu, d);
    let drawn: Rc<RefCell<Drawn>> = Rc::default();
    {
        let dr = drawn.clone();
        cpu.hook(BOX_DRAW, move |r, _| {
            dr.borrow_mut().boxes.push((
                r.r[psx_retail_exec::A0] as i16 as i32,
                r.r[psx_retail_exec::A1] as i16 as i32,
                r.r[psx_retail_exec::A2] as i16 as i32,
                r.r[psx_retail_exec::A3] as i16 as i32,
            ))
        });
        let dr = drawn.clone();
        cpu.hook(SPRITE, move |r, _| {
            dr.borrow_mut().sprites.push((
                r.r[psx_retail_exec::A0],
                r.r[psx_retail_exec::A2] as i16 as i32,
                r.r[psx_retail_exec::A3] as i16 as i32,
            ))
        });
        let dr = drawn.clone();
        cpu.hook(TEXT_DRAW, move |r, m| {
            let y = m.rd32(r.r[psx_retail_exec::SP] + 0x10) as i16 as i32;
            dr.borrow_mut().texts.push((
                (r.r[psx_retail_exec::A0].wrapping_sub(1 + TEXT)) as usize,
                r.r[psx_retail_exec::A3] as i16 as i32,
                y,
            ))
        });
        let dr = drawn.clone();
        cpu.hook(CUE, move |r, _| {
            dr.borrow_mut().cues.push(r.r[psx_retail_exec::A0])
        });
    }
    let mut rng = Rng(0x5049_434B);
    let mut seen = std::collections::BTreeMap::<String, u32>::new();
    for scenario in 0..1200 {
        let text = gen_picker_text(&mut rng);
        let dt = [1u8, 2, 2, 3, 4, 5][rng.below(6) as usize];
        cpu.mem.load(TEXT, &text);
        cpu.mem.wr8(FRAME_STEP, dt);
        for i in 0..16 {
            cpu.mem.wr32(TABLE + i * 4, 0);
        }
        for a in [
            SCROLL, COUNTER, ACC, HOLD, LAST, ON_PAGE, INSTANT, RAMP, GONE,
        ] {
            cpu.mem.wr32(a, 0);
        }
        cpu.mem.wr32(ROWS_MAX, 3);
        cpu.mem.wr32(PACE, 1);
        cpu.mem.wr32(STATE, 2);
        cpu.mem.wr32(FEED, TEXT);
        cpu.mem.load(SCENE, &[0u8; 0x40]);
        cpu.mem.wr32(SCENE_PTR, SCENE);
        cpu.mem.wr16(AUTO_PRESS, 0);
        cpu.mem.load(ACTOR, &[0u8; 0x60]);
        let preset = rng.below(4) as usize;

        let mut port = OwnedDialogPanel::at_segment(Arc::new(text.clone()), 0);
        port.frame_step = dt;
        port.set_window_format(3, false);
        let picker = port.picker().cloned().expect("the prompt's picker");
        let preset = preset % picker.n;
        cpu.mem.wr16(SCENE + 0xC, preset as u16);
        port.seat_picker_cursor(preset);

        // Type the prompt on both sides until it waits.
        for _ in 0..4000 {
            if cpu.mem.rd32(STATE) == 0x19 {
                break;
            }
            cpu.mem.wr32(PRESSED, 0);
            cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
            assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
        }
        assert_eq!(cpu.mem.rd32(STATE), 0x19, "scenario {scenario}");
        for _ in 0..4000 {
            if port.is_waiting_for_input() {
                break;
            }
            port.tick();
        }
        assert!(port.is_waiting_for_input() && !port.menu_active());

        // The press that opens the menu.
        cpu.mem.wr32(PRESSED, 0x40);
        cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
        assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
        port.advance_page();
        let entry = cpu.mem.rd32(STATE) as u8;
        let want_entry = match picker.open_byte & 0x7F {
            0x2A => 0x11,
            0x27 => 0x13,
            0x28 => 0x15,
            _ => 0x17,
        };
        let at = format!("scenario {scenario} dt {dt} open {:#x}", picker.open_byte);
        assert_eq!(entry, want_entry, "{at}: the press's entry state");
        assert!(port.menu_active(), "{at}");
        let slide = |cpu: &mut Cpu| {
            let mut h = |o: u32| cpu.mem.rd16(ACTOR + o) as i16;
            (
                h(0x54),
                h(0x50),
                (h(0x3C), h(0x3E)),
                (h(0x14), h(0x16)),
                (h(0x24), h(0x26)),
            )
        };
        let port_slide = |p: &OwnedDialogPanel| {
            let s = p.picker_slide().expect("an open menu's slide");
            (s.count, s.span, s.start, s.target, s.size)
        };
        assert_eq!(
            slide(&mut cpu).0,
            port_slide(&port).0,
            "{at}: the press's sentinel"
        );

        for call in 0..200 {
            let pad = match rng.below(20) {
                0..=2 => PAD_UP,
                3..=5 => PAD_DOWN,
                6 => PAD_UP | PAD_DOWN,
                7..=8 => 0x40,
                9 => 0x40 | PAD_DOWN,
                _ => 0,
            };
            let at = format!("{at} call {call} pad {pad:#x}");
            *drawn.borrow_mut() = Drawn::default();
            cpu.mem.wr32(PRESSED, pad);
            cpu.mem.wr32(0x1F80_03A0, 0x8018_0000);
            assert_eq!(cpu.call(PAGER, &[ACTOR]), Stop::Returned);
            let state = cpu.mem.rd32(STATE) as u8;

            // The world's order: moves, the pager call, the commit.
            let menu_was_open = port.menu_active();
            let moved = port.menu_active()
                && port.step_picker_cursor(pad & PAD_UP != 0, pad & PAD_DOWN != 0);
            for _ in 0..dt {
                port.tick();
            }
            let commit = pad & 0x40 != 0 && menu_was_open && port.picker_takes_input();

            let dr = drawn.borrow();
            assert_eq!(
                dr.cues.contains(&0x21),
                moved,
                "{at}: the move cue {:?}",
                dr.cues
            );
            assert_eq!(
                cpu.mem.rd16(SCENE + 0xC) as usize,
                port.picker_cursor(),
                "{at}: the cursor"
            );
            if commit {
                assert_eq!(dr.cues.last(), Some(&0x20), "{at}: the commit cue");
                let held = HeldBox::after_press(&port, Some(picker.continuation));
                let want = match (
                    picker.continuation,
                    text.get(picker.open + picker.n * 2 + 2),
                ) {
                    (0x24, _) => 3,
                    (0x48, _) => 9,
                    (0x4C, Some(0xFF)) => 6,
                    _ => 0,
                };
                assert_eq!(state, want, "{at}: the commit's state");
                assert_eq!(held.until_next_line, state == 3, "{at}: the held box");
                *seen.entry(format!("commit {state}")).or_default() += 1;
                break;
            }
            assert!((0x11..=0x18).contains(&state), "{at}: state {state:#x}");
            assert_eq!(
                (state - 0x11) / 2,
                (want_entry - 0x11) / 2,
                "{at}: the picker's states"
            );
            assert_eq!(
                state == want_entry + 1,
                port.picker_slide().is_some_and(|s| s.count == 0),
                "{at}: the active state"
            );
            assert_eq!(slide(&mut cpu), port_slide(&port), "{at}: the slide words");

            // The picker draw: the box last, the hand at rest, the labels.
            let rect = port.picker_rect().expect("a slide past the press");
            assert_eq!(dr.boxes.last().copied(), Some(rect), "{at}: the box");
            let hand: Vec<_> = dr.sprites.iter().filter(|s| s.0 == 0).collect();
            if port.picker_hand_drawn() {
                let want = (0, rect.0 - 6, rect.1 + port.picker_cursor() as i32 * 0xF);
                assert_eq!(hand, vec![&want], "{at}: the hand");
                *seen.entry("hand".into()).or_default() += 1;
            } else {
                assert!(hand.is_empty(), "{at}: no hand while it slides");
            }
            let labels: Vec<_> = dr.texts[dr.texts.len() - picker.n..].to_vec();
            let want: Vec<_> = picker
                .options
                .iter()
                .enumerate()
                .map(|(i, o)| (o.lead, rect.0 + 0x10, rect.1 + i as i32 * 0xF))
                .collect();
            assert_eq!(labels, want, "{at}: the labels");
            *seen.entry(format!("state {state:#x}")).or_default() += 1;
            if moved {
                *seen.entry(format!("moved {state:#x}")).or_default() += 1;
            }
        }
    }
    for k in [
        "commit 0",
        "commit 3",
        "commit 6",
        "commit 9",
        "state 0x11",
        "state 0x12",
        "state 0x13",
        "state 0x14",
        "state 0x15",
        "state 0x16",
        "state 0x17",
        "state 0x18",
        "moved 0x12",
        "moved 0x18",
        "hand",
    ] {
        assert!(seen.get(k).copied().unwrap_or(0) > 10, "{k}: {seen:?}");
    }
}
