//! The Options screen of the menu overlay (PROT 0899): the wrapper
//! `FUN_801DD330` against the argument tuple the port takes from it, and the
//! picker `FUN_801DA9F8`'s popup-open arm against `options_popup_content_rect`:
//! the popup `y` / `h` it stamps into window descriptor 47, over every display
//! row and settings-window `y`, through the overlay's own layout and option
//! tables. The port's per-setting choice counts and row advances are checked
//! against the tables the arm reads.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::options::{
    OPTIONS_DISPLAY_ROWS, OPTIONS_SUBSCREEN_INIT, OPTIONS_SUBSCREEN_RETURN,
    OPTIONS_SUBSCREEN_ROW_SPAN, options_popup_content_rect,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const WRAPPER: u32 = 0x801D_D330;
const PICKER: u32 = 0x801D_A9F8;
const VM: u32 = 0x801D_6628;
const CUE: u32 = 0x8003_5BD0;
const WINDOW_TICK: u32 = 0x8003_1D00;
const STEP: u32 = 0x801E_46AC;
const CURSOR: u32 = 0x801E_46C0;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;
/// The display layout table: `[option id, advance]` halfwords per row.
const LAYOUT: u32 = 0x801E_4404;
/// The option records: `[value ptr][count][..][id][..]`, 8 bytes each.
const OPTIONS: u32 = 0x801E_44B8;
/// Window descriptor table; 48's `y` is the settings window's, 47's `y` /
/// `h` the popup's.
const SETTINGS_Y: u32 = 0x801E_4A3E;
const POPUP_Y: u32 = 0x801E_4A2E;
const POPUP_H: u32 = 0x801E_4A32;

fn rig() -> Option<(Cpu, Rc<RefCell<Vec<u32>>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let cues = Rc::new(RefCell::new(Vec::new()));
    let c = cues.clone();
    cpu.hook(CUE, move |r, _| c.borrow_mut().push(r.r[A0]));
    cpu.stub(VM, 0);
    cpu.stub(WINDOW_TICK, 0);
    cpu.stub(0x8003_5B50, 0);
    Some((cpu, cues))
}

#[test]
fn the_options_wrapper_passes_what_the_port_takes() {
    differential!(prot(899, WRAPPER));
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let args = Rc::new(RefCell::new(None));
    let a = args.clone();
    cpu.hook(PICKER, move |r, _| {
        *a.borrow_mut() = Some((r.r[A0], r.r[A1], r.r[A2], r.r[A3]))
    });
    assert_eq!(cpu.call(WRAPPER, &[]), Stop::Returned);
    let (lo, hi) = OPTIONS_SUBSCREEN_ROW_SPAN;
    assert_eq!(
        *args.borrow(),
        Some((
            u32::from(lo),
            u32::from(hi),
            OPTIONS_SUBSCREEN_INIT,
            u32::from(OPTIONS_SUBSCREEN_RETURN)
        ))
    );
}

#[test]
fn the_options_popup_opens_where_retail_opens_it() {
    differential!(prot(899, PICKER));
    let Some((mut cpu, cues)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    cpu.mem.wr32(REPEAT, 0);
    // Each display row's option record, as the arm finds it.
    let mut records = Vec::new();
    for (row, def) in OPTIONS_DISPLAY_ROWS.iter().enumerate() {
        let id = cpu.mem.rd16(LAYOUT + 4 * row as u32) as i16;
        assert_eq!(
            cpu.mem.rd16(LAYOUT + 4 * row as u32 + 2) as i16 as i32,
            def.advance,
            "row {row} advance"
        );
        let mut rec = OPTIONS;
        let found = loop {
            if cpu.mem.rd32(rec) == 0 {
                break None;
            }
            if i16::from(cpu.mem.rd8(rec + 6)) == id {
                break Some(rec);
            }
            rec += 8;
        };
        let count = found.map(|r| cpu.mem.rd8(r + 4));
        if let (Some(setting), Some(n)) = (def.setting, count) {
            assert_eq!(setting.choices().len(), usize::from(n), "row {row} choices");
        }
        records.push(count);
    }
    let mut rng = Rng(0x4F50_5453);
    let (mut opened, mut flipped) = (0, 0);
    for case in 0..4000 {
        let row = rng.below(10) as usize;
        let Some(count) = records[row].filter(|&n| n != 0) else {
            continue;
        };
        let settings_y = [rng.below(240) as i16, rng.next() as i16][rng.below(2) as usize];
        cpu.mem.wr16(SETTINGS_Y, settings_y as u16);
        cpu.mem.wr16(POPUP_Y, 0xAAAA);
        cpu.mem.wr16(POPUP_H, 0xAAAA);
        cpu.mem.wr32(CURSOR, row as u32);
        cpu.mem.wr32(STEP, 2);
        cpu.mem.wr32(EDGE, 0x40);
        cues.borrow_mut().clear();
        assert_eq!(
            cpu.call(PICKER, &[0, 9, 0x30, 1]),
            Stop::Returned,
            "case {case}"
        );
        assert_eq!(
            cpu.mem.rd32(STEP),
            3,
            "case {case} row {row}: the popup opened"
        );
        let (_, y, _, h) =
            options_popup_content_rect(i32::from(settings_y), 0, 0, row, usize::from(count));
        let what = format!("case {case} row {row} settings y {settings_y} count {count}");
        assert_eq!(cpu.mem.rd16(POPUP_Y) as i16, y as i16, "{what}");
        assert_eq!(cpu.mem.rd16(POPUP_H) as i16, h as i16, "{what}");
        opened += 1;
        flipped += u32::from(y < i32::from(settings_y) + 0x16);
    }
    assert!(opened > 1000 && flipped > 200, "{opened} {flipped}");
}

/// The popup choice word `DAT_801E46D0` and the sub-screen id the exit arm
/// writes.
const CHOICE: u32 = 0x801E_46D0;
const SUBSCREEN: u32 = 0x801E_46A4;
/// The rumble motor words the Events commit zeroes (`0x801DACF0..0x801DAD24`).
const RUMBLE: [u32; 2] = [0x8007_B92C, 0x8007_B930];

#[test]
fn the_options_picker_steps_as_retail_steps() {
    use legaia_engine_menus::options::{
        OptionsEvent, OptionsInput, OptionsPhase, OptionsSession, OptionsSetting, OptionsState,
    };
    differential!(prot(899, PICKER));
    let Some((mut cpu, cues)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    // Each display row's value word, as the arms find its record.
    let ptrs: Vec<Option<u32>> = (0..OPTIONS_DISPLAY_ROWS.len() as u32)
        .map(|row| {
            let id = cpu.mem.rd16(LAYOUT + 4 * row) as i16;
            let mut rec = OPTIONS;
            while cpu.mem.rd32(rec) != 0 {
                if i16::from(cpu.mem.rd8(rec + 6)) == id {
                    return (cpu.mem.rd8(rec + 4) != 0).then(|| cpu.mem.rd32(rec));
                }
                rec += 8;
            }
            None
        })
        .collect();
    let events_row = OPTIONS_DISPLAY_ROWS
        .iter()
        .position(|d| d.setting == Some(OptionsSetting::VibrationEvents))
        .unwrap();
    let mut rng = Rng(0x4F50_5453);
    let (mut commits, mut cancels, mut exits, mut wraps, mut stops) = (0, 0, 0, 0, 0);
    for case in 0..600 {
        let mut state = OptionsState::default();
        for (def, ptr) in OPTIONS_DISPLAY_ROWS.iter().zip(&ptrs) {
            if let (Some(s), Some(p)) = (def.setting, ptr) {
                let v = rng.below(s.choices().len() as u32) as u8;
                s.set(&mut state, v);
                cpu.mem.wr32(*p, u32::from(v));
            }
        }
        let mut port = OptionsSession::new(state);
        // State 0: the cursor home, the window opened, then the browse.
        cpu.mem.wr32(STEP, 0);
        cpu.mem.wr32(CURSOR, 0xFFF);
        cpu.mem.wr32(SUBSCREEN, 0xEE);
        cpu.mem.wr32(EDGE, 0);
        cpu.mem.wr32(REPEAT, 0);
        assert_eq!(cpu.call(PICKER, &[0, 9, 0x30, 1]), Stop::Returned);
        for frame in 0..48 {
            let what = format!("case {case} frame {frame}");
            let retail_phase = match cpu.mem.rd32(STEP) {
                2 if cpu.mem.rd32(SUBSCREEN) == 1 => None,
                2 => Some((cpu.mem.rd32(CURSOR) as u8, None)),
                3 => Some((cpu.mem.rd32(CURSOR) as u8, Some(cpu.mem.rd32(CHOICE) as u8))),
                s => panic!("{what}: step {s}"),
            };
            let port_phase = match port.phase() {
                OptionsPhase::Browsing { cursor } => Some((cursor, None)),
                OptionsPhase::Editing { cursor, choice } => Some((cursor, Some(choice))),
                OptionsPhase::Done(_) => None,
                p => panic!("{what}: {p:?}"),
            };
            assert_eq!(retail_phase, port_phase, "{what}");
            for (def, ptr) in OPTIONS_DISPLAY_ROWS.iter().zip(&ptrs) {
                if let (Some(s), Some(p)) = (def.setting, ptr) {
                    assert_eq!(
                        cpu.mem.rd32(*p),
                        u32::from(s.get(port.state())),
                        "{what}: {s:?}"
                    );
                }
            }
            let Some((_, choice)) = port_phase else {
                exits += 1;
                break;
            };
            let input = match rng.below(9) {
                0..=2 => OptionsInput {
                    up: true,
                    ..Default::default()
                },
                3..=5 => OptionsInput {
                    down: true,
                    ..Default::default()
                },
                6..=7 => OptionsInput {
                    cross: true,
                    ..Default::default()
                },
                _ => OptionsInput {
                    // Closing ends the case; keep it rarer than a commit.
                    circle: choice.is_some() || rng.below(3) == 0,
                    ..Default::default()
                },
            };
            cpu.mem.wr32(
                EDGE,
                if input.cross { 0x40 } else { 0 } | if input.circle { 0x20 } else { 0 },
            );
            cpu.mem.wr32(
                REPEAT,
                if input.up { 0x1000 } else { 0 } | if input.down { 0x4000 } else { 0 },
            );
            for w in RUMBLE {
                cpu.mem.wr32(w, 0x55);
            }
            let events_was = OptionsSetting::VibrationEvents.get(port.state());
            cues.borrow_mut().clear();
            assert_eq!(cpu.call(PICKER, &[0, 9, 0x30, 1]), Stop::Returned, "{what}");
            // A closed popup's state 1 reruns the window script and is back on
            // the browse the frame after, which takes no pad.
            if cpu.mem.rd32(STEP) == 1 {
                cpu.mem.wr32(EDGE, 0);
                cpu.mem.wr32(REPEAT, 0);
                assert_eq!(cpu.call(PICKER, &[0, 9, 0x30, 1]), Stop::Returned);
            }
            let before = port.phase();
            let events = port.tick(input);
            let committed = events
                .iter()
                .any(|e| matches!(e, OptionsEvent::ValueChanged { .. }));
            assert_eq!(
                cues.borrow().contains(&0x20),
                committed,
                "{what}: the commit cue"
            );
            commits += usize::from(committed);
            cancels += usize::from(events.contains(&OptionsEvent::EditCancelled));
            if let (
                OptionsPhase::Editing { choice: a, .. },
                OptionsPhase::Editing { choice: b, .. },
            ) = (before, port.phase())
            {
                wraps += usize::from(a.abs_diff(b) > 1);
            }
            // Committing Events Off stops the motors; the port's push does
            // it on the On-to-Off edge (`legaia_engine_core::options`).
            if committed
                && before.eq(&OptionsPhase::Editing {
                    cursor: events_row as u8,
                    choice: 1,
                })
                && events_was == 0
            {
                let mut world = legaia_engine_core::world::World::new();
                let mut was = OptionsState::default();
                OptionsSetting::VibrationEvents.set(&mut was, 0);
                legaia_engine_core::options::apply_to_world(&was, &mut world);
                world.rumble.motors.pulse = 0x55;
                world.rumble.motors.level = 0x55;
                legaia_engine_core::options::apply_to_world(port.state(), &mut world);
                assert_eq!(
                    RUMBLE.map(|w| cpu.mem.rd32(w)),
                    [0, 0],
                    "{what}: the motors stop"
                );
                assert_eq!(
                    (world.rumble.motors.pulse, world.rumble.motors.level),
                    (0, 0),
                    "{what}: the port's motors"
                );
                stops += 1;
            } else if !(committed && port.cursor() as usize == events_row) {
                assert_eq!(RUMBLE.map(|w| cpu.mem.rd32(w)), [0x55, 0x55], "{what}");
            }
        }
    }
    assert!(
        commits > 300 && cancels > 100 && exits > 100 && wraps > 50 && stops > 10,
        "{commits} {cancels} {exits} {wraps} {stops}"
    );
}
