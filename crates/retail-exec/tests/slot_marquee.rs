//! The slot machine's dot-matrix marquee in PROT 0975: the scroll composer
//! `FUN_801D069C` (its negative-id clear and its windowed blit) against
//! `compose_marquee` / `clear_dots`, and the placement blit `FUN_801D3230`
//! against `place_message`, over generated messages written into the
//! overlay's record table (`0x801D34F0`, `[u, v, w, h][u32 source]`, the
//! source at the decoded page's `0x100` stride) and the whole dot buffer
//! `0x801D37A0` compared.
//!
//! The composer resets its scroll to column `0` when the id differs from the
//! last one (`0x801D3C88`); the port's caller owns that
//! (`SlotMarqueeClock`), so every compose here repeats the last id.

use crate::common::{Rng, disc, scus_cpu};
use legaia_minigame_formats::minigame_slot_scene::{
    DOT_COLS, DOT_ROWS, DOT_STRIDE, MarqueeMessage, clear_dots, compose_marquee, place_message,
};
use psx_retail_exec::{Cpu, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const COMPOSE: u32 = 0x801D_069C;
const PLACE: u32 = 0x801D_3230;
const RECORDS: u32 = 0x801D_34F0;
const DOTS: u32 = 0x801D_37A0;
const LAST_ID: u32 = 0x801D_3C88;
const PAGE: u32 = 0x8014_0000;
const BUF: usize = DOT_COLS * DOT_STRIDE;

fn gen_message(rng: &mut Rng, cpu: &mut Cpu, id: u32) -> MarqueeMessage {
    let w = [1, 13, 40, 78, 90, 1 + rng.below(120)][rng.below(6) as usize] as u8;
    let h = [13, 13, 1, 5, 1 + rng.below(20)][rng.below(5) as usize] as u8;
    let src = PAGE + id * 0x2000;
    let mut bitmap = Vec::new();
    for r in 0..u32::from(h) {
        for c in 0..u32::from(w) {
            let n = if rng.below(3) == 0 {
                0
            } else {
                rng.below(16) as u8
            };
            cpu.mem.wr8(src + r * 0x100 + c, n);
            bitmap.push(n);
        }
    }
    let rec = RECORDS + id * 8;
    cpu.mem.wr8(rec, rng.next() as u8);
    cpu.mem.wr8(rec + 1, rng.next() as u8);
    cpu.mem.wr8(rec + 2, w);
    cpu.mem.wr8(rec + 3, h);
    cpu.mem.wr32(rec + 4, src);
    MarqueeMessage {
        u: 0,
        v: 0,
        w,
        h,
        bitmap,
    }
}

fn rig() -> Option<Cpu> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(975).expect("PROT 0975"));
    Some(cpu)
}

#[test]
fn the_scroll_composer_matches_retail() {
    differential!(prot(975, COMPOSE));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4D41_5251);
    for run in 0..5000 {
        let id = rng.below(21);
        let msg = gen_message(&mut rng, &mut cpu, id);
        // Junk on the 13 live rows of every column; the stride's three spare
        // bytes stay zero, as nothing writes them.
        let junk: Vec<u8> = (0..BUF)
            .map(|i| {
                if i % DOT_STRIDE < DOT_ROWS {
                    rng.next() as u8
                } else {
                    0
                }
            })
            .collect();
        cpu.mem.load(DOTS, &junk);
        // The clear, then the compose of the id already shown.
        assert_eq!(cpu.call(COMPOSE, &[u32::MAX, 0, 0]), Stop::Returned);
        assert_eq!(
            cpu.mem.read(DOTS, BUF),
            clear_dots(),
            "run {run}: the clear"
        );
        cpu.mem.wr32(LAST_ID, id);
        let x = rng.below(260) as i32 - 130;
        let y = rng.below(30) as i32 - 15;
        assert_eq!(cpu.call(COMPOSE, &[id, x as u32, y as u32]), Stop::Returned);
        let retail = cpu.mem.read(DOTS, BUF);
        let port = compose_marquee(&msg, x, y);
        assert_eq!(
            retail, port,
            "run {run} id {id} x {x} y {y} w {} h {}",
            msg.w, msg.h
        );
    }
}

#[test]
fn the_placement_blit_matches_retail() {
    differential!(prot(975, PLACE));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x504C_4143);
    for run in 0..5000 {
        let id = rng.below(21);
        let msg = gen_message(&mut rng, &mut cpu, id);
        let prefill: Vec<u8> = (0..BUF).map(|_| rng.below(16) as u8).collect();
        cpu.mem.load(DOTS, &prefill);
        // Negative offsets included: the payout caption slides in from above.
        let col = rng.below(200) as i32 - 100;
        let row = rng.below(40) as i32 - 20;
        assert_eq!(
            cpu.call(PLACE, &[id, col as u32, row as u32]),
            Stop::Returned
        );
        let retail = cpu.mem.read(DOTS, BUF);
        let mut port = prefill.clone();
        place_message(&mut port, &msg, col, row);
        assert_eq!(retail, port, "run {run} id {id} col {col} row {row}");
    }
}
