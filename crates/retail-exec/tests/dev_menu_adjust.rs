//! The developer menu's per-row value adjuster `FUN_801E9F64` (PROT 0897)
//! against `engine-dev-menu`'s two ported arms: the 12-bit ring on
//! `_DAT_8007B6D0` (row 7) and the `[1, 255]` clamp on `_DAT_801F2E8C`
//! (row 10), stepped off the newly-pressed pad word `_DAT_8007BB84`.
//!
//! Rows whose jump-table entry is the shared tail (`0x801EA7AC`) are run as
//! well: they leave both words alone.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dev_menu::dev_menu_adjust::{clamp1_255_step, wrap12_step};
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const ADJUST: u32 = 0x801E_9F64;
const PAD_PRESSED: u32 = 0x8007_BB84;
const RING: u32 = 0x8007_B6D0;
const RATE: u32 = 0x801F_2E8C;
const ACTOR: u32 = 0x8012_0000;

#[test]
fn the_ring_and_clamp_rows_step_as_retail_steps() {
    differential!(prot(897, ADJUST));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let mut rng = Rng(0x4144_4A55);
    let (mut both, mut wrapped, mut clamped, mut idle) = (0, 0, 0, 0);
    for run in 0..12_000 {
        // The two ported rows, and the rows that go straight to the tail.
        let row = [7u16, 7, 10, 10, 4, 5, 8, 9, 11, 15, 16, 2, 30][rng.below(13) as usize];
        cpu.mem.wr16(ACTOR + 0x9E, row);
        let pad = match rng.below(5) {
            0 => 0,
            1 => 0x2000,
            2 => 0x8000,
            3 => 0xA000,
            _ => rng.next() & 0xFFFF,
        };
        cpu.mem.wr32(PAD_PRESSED, pad);
        let ring = match rng.below(4) {
            0 => [0u32, 0xFFF, 0x1234, 0xF000][rng.below(4) as usize],
            _ => rng.below(0x1000),
        };
        let rate = match rng.below(4) {
            0 => [0i32, 1, 255, 256, -3, 500][rng.below(6) as usize],
            _ => 1 + rng.below(255) as i32,
        };
        cpu.mem.wr32(RING, ring);
        cpu.mem.wr32(RATE, rate as u32);
        assert_eq!(cpu.call(ADJUST, &[ACTOR]), Stop::Returned, "run {run}");
        let what = format!("run {run}: row {row} pad {pad:#x} ring {ring:#x} rate {rate}");
        let (want_ring, want_rate) = match row {
            7 => (u32::from(wrap12_step(ring as u16, pad)), rate),
            10 => (ring, clamp1_255_step(rate, pad)),
            _ => (ring, rate),
        };
        assert_eq!(cpu.mem.rd32(RING), want_ring, "{what}");
        assert_eq!(cpu.mem.rd32(RATE) as i32, want_rate, "{what}");
        both += u32::from(pad & 0xA000 == 0xA000 && (row == 7 || row == 10));
        wrapped +=
            u32::from(row == 7 && want_ring != ring && (want_ring == 0 || want_ring == 0xFFF));
        clamped += u32::from(row == 10 && (want_rate == 1 || want_rate == 255));
        idle += u32::from(row != 7 && row != 10);
    }
    assert!(
        both > 300 && wrapped > 20 && clamped > 200 && idle > 3000,
        "{both} {wrapped} {clamped} {idle}"
    );
}

const ACTION: u32 = 0x801E_A9B0;

/// The row-action dispatcher `FUN_801EA9B0` against `dev_menu_action` and
/// `dev_menu_action_phase`: the bound, the phase every path stores to
/// `ctx[+0x54]`, and the `1` every path returns. The arms' own pokes are let
/// run, their calls out of the overlay answered with zero.
#[test]
fn the_row_action_parks_and_returns_as_retail_does() {
    differential!(prot(897, ACTION));
    use legaia_engine_dev_menu::dev_menu_action::{dev_menu_action, dev_menu_action_phase};
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = d.read_entry(897).expect("PROT 0897");
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    /// Row 1's gate word.
    const ROW_ONE_GATE: u32 = 0x8007_B868;
    let mut rng = Rng(0x4143_5449);
    let (mut parked, mut ran, mut gated) = (0, 0, 0);
    for run in 0..2_000 {
        // The arms poke the overlay's own data; each run starts clean.
        cpu.mem.load(OVERLAY_BASE, &image);
        let row = match rng.below(4) {
            0 => [-1i16, -2, 0x18, 0x19, 0x400, i16::MIN][rng.below(6) as usize],
            1 => 1,
            _ => rng.below(0x18) as i16,
        };
        let phase = 0x40 + rng.below(8) as i16;
        let gate = rng.below(2) * (1 + rng.below(5));
        cpu.mem.wr32(ROW_ONE_GATE, gate);
        cpu.mem.wr16(ACTOR + 0x9E, row as u16);
        cpu.mem.wr16(ACTOR + 0x54, phase as u16);
        let stop = cpu.call(ACTION, &[ACTOR]);
        assert_eq!(stop, Stop::Returned, "run {run}: row {row}");
        let (park, result) = dev_menu_action(row);
        assert_eq!(
            cpu.regs.r[psx_retail_exec::V0] as i32,
            result,
            "run {run}: row {row}"
        );
        let left = dev_menu_action_phase(row, gate != 0);
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x54) as i16,
            left,
            "run {run}: row {row} gate {gate}: the phase word"
        );
        if let Some(p) = park {
            assert_eq!(p, left, "run {run}: the park phase is the one stored");
        }
        assert_eq!(
            park.is_some(),
            !(0..0x18).contains(&row),
            "run {run}: row {row}"
        );
        parked += u32::from(park.is_some());
        ran += u32::from(park.is_none());
        gated += u32::from(row == 1 && gate == 0);
    }
    assert!(
        parked > 300 && ran > 1000 && gated > 100,
        "{parked} {ran} {gated}"
    );
}

const PICKER: u32 = 0x801E_CA08;

/// The list picker `FUN_801ECA08`'s Active leg (phase `1`) against
/// `engine-dev-menu`: the panel descriptor it sizes (`panel_geometry`), the
/// row step and wrap (`dev_menu_cursor_step`), the leg's exit
/// (`list_picker_active_exit`), the cues, and the draw gate
/// (`list_body_draws`), whose `1` calls the row renderer `FUN_801EAD98`.
/// The value adjuster and the hand are stubbed and logged.
#[test]
fn the_list_pickers_active_leg_steps_as_retail_steps() {
    differential!(prot(897, PICKER));
    use legaia_engine_dev_menu::dev_menu_leaves::{
        ListPickerPhase, SFX_CANCEL, SFX_CONFIRM, SFX_CURSOR_MOVE, dev_menu_cursor_step,
        list_body_draws, list_picker_active_exit, panel_geometry,
    };
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    const PANELS: u32 = 0x801F_2B98;
    const BUSY: u32 = 0x8007_BB80;
    const HELD: u32 = 0x8007_B850;
    const STEP_EDGE: u32 = 0x8007_BB84;
    const EDGE: u32 = 0x8007_B874;
    const CONFIRM_MASK: u32 = 0x8008_46D0;
    const CANCEL_MASK: u32 = 0x8008_46D4;
    type Log = std::rc::Rc<std::cell::RefCell<Vec<(u32, u32)>>>;
    let log: Log = Default::default();
    for entry in [
        0x8003_1D00u32,
        0x8003_5B50,
        0x801E_9F64,
        0x8001_7D98,
        0x8003_5A4C,
        0x801E_AD98,
    ] {
        let l = log.clone();
        cpu.hook(entry, move |r, _| {
            l.borrow_mut().push((entry, r.r[psx_retail_exec::A0]))
        });
    }
    cpu.mem.wr32(CONFIRM_MASK, 0x40);
    cpu.mem.wr32(CANCEL_MASK, 0x20);
    let mut rng = Rng(0x5049_434B);
    let mut seen = [0u32; 4];
    for run in 0..6_000 {
        let start = rng.below(8) as i32;
        let end = start + rng.below(12) as i32;
        let row = match rng.below(6) {
            0 => start - 1 - rng.below(3) as i32,
            1 => end + 1,
            _ => start + rng.below((end - start + 1) as u32) as i32,
        };
        let panel = rng.below(4);
        cpu.mem.wr16(ACTOR + 0x54, 1);
        cpu.mem.wr16(ACTOR + 0x9E, row as u16);
        let busy = rng.below(8) == 0;
        cpu.mem.wr32(BUSY, u32::from(busy));
        let held = if rng.below(6) == 0 { 0x40 } else { 0 };
        cpu.mem.wr32(HELD, held);
        let (up, down) = (rng.below(3) == 0, rng.below(3) == 0);
        cpu.mem
            .wr32(STEP_EDGE, u32::from(up) * 0x1000 | u32::from(down) * 0x4000);
        let (cancel, confirm) = (rng.below(5) == 0, rng.below(5) == 0);
        cpu.mem
            .wr32(EDGE, u32::from(cancel) * 0x20 | u32::from(confirm) * 0x40);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(PICKER, &[ACTOR, start as u32, end as u32, panel]),
            Stop::Returned,
            "run {run}"
        );
        let what = format!(
            "run {run}: rows {start}..={end} row {row} busy {busy} held {held:#x} up {up} down {down} cancel {cancel} confirm {confirm}"
        );
        let calls = log.borrow().clone();
        let geom = panel_geometry(start, end);
        let desc = PANELS + panel * 28;
        assert_eq!(
            (
                cpu.mem.rd16(desc + 0xA) as i16,
                cpu.mem.rd16(desc + 0xE) as i16
            ),
            (geom.y, geom.height),
            "{what}: the panel"
        );
        let drawn = calls.iter().any(|c| c.0 == 0x801E_AD98);
        if busy {
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54), 1, "{what}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9E) as i16 as i32, row, "{what}");
            assert_eq!(drawn, list_body_draws(1, 0), "{what}: the gate");
            seen[0] += 1;
            continue;
        }
        let stepping = held == 0;
        let (want_row, _) = dev_menu_cursor_step(row, start, end, up && stepping, down && stepping);
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x9E) as i16 as i32,
            want_row,
            "{what}: the row"
        );
        let phase = list_picker_active_exit(cancel, confirm);
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x54),
            phase as u16,
            "{what}: the phase"
        );
        let mut cues = Vec::new();
        if stepping && up {
            cues.push(SFX_CURSOR_MOVE);
        }
        if stepping && down {
            cues.push(SFX_CURSOR_MOVE);
        }
        match phase {
            ListPickerPhase::CancelSettle => cues.push(SFX_CANCEL),
            ListPickerPhase::RowAction => cues.push(SFX_CONFIRM),
            _ => {}
        }
        let got: Vec<u32> = calls
            .iter()
            .filter(|c| c.0 == 0x8003_5B50)
            .map(|c| c.1)
            .collect();
        assert_eq!(got, cues, "{what}: the cues");
        assert_eq!(drawn, list_body_draws(1, 1), "{what}: the gate");
        seen[phase as usize] += 1;
    }
    assert!(seen.iter().all(|&n| n > 500), "{seen:?}");
}
