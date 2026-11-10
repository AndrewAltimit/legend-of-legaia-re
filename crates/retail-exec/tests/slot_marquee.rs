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

/// The slot overlay's init `FUN_801CEC94`, whole with its other calls stubbed,
/// over the shipped record table and generated records: for each of the 21
/// message records it `StoreImage`s the rect at `(0x340 + u / 4, 0x100 + v)`
/// out of a generated page-3 VRAM, allocates `h << 8` bytes, stores the
/// pointer at the record's `+4` and expands the 4bpp row into one byte per
/// texel. Each expanded texel is the nibble times four (the swatch's `u`), the
/// rows `0x100` apart; held against `parse_messages` over the same page.
/// Retail expands `w / 2` byte pairs a row, so an odd width's last column is
/// never written and is left out of the compare.
#[test]
fn the_message_bank_expands_as_retail_expands_it() {
    use legaia_minigame_formats::minigame_slot_scene::{
        MESSAGE_COUNT, MESSAGE_STRIDE, MESSAGE_TABLE_OFFSET, parse_messages,
    };
    use psx_retail_exec::{A0, A1, V0};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    const INIT: u32 = 0x801C_EC94;
    const STORE_IMAGE: u32 = 0x8005_842C;
    const ALLOC: u32 = 0x8001_7888;
    const SPAWN_ACTOR: u32 = 0x8002_0DE0;
    const HEAP: u32 = 0x8015_0000;
    const PAGE_W: usize = 256;
    const PAGE_H: usize = 256;
    differential!(prot(975, INIT));
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut overlay = d.read_entry(975).expect("PROT 0975");
    let mut rng = Rng(0x4241_4E4B);
    let mut compared = 0usize;
    for run in 0..40 {
        // Run 0 keeps the shipped records; the others generate their own,
        // `u` on a VRAM halfword and the rect inside the page, as every
        // shipped one is.
        if run > 0 {
            for i in 0..MESSAGE_COUNT {
                let o = MESSAGE_TABLE_OFFSET + i * MESSAGE_STRIDE;
                let u = rng.below(0x30) * 4;
                overlay[o] = u as u8;
                overlay[o + 1] = rng.below(0x80) as u8;
                overlay[o + 2] = (1 + rng.below(0x80).min(PAGE_W as u32 - u - 1)) as u8;
                overlay[o + 3] = rng.below(20) as u8;
            }
        }
        let page: Vec<u8> = (0..PAGE_W * PAGE_H).map(|_| rng.below(16) as u8).collect();
        let mut cpu = scus_cpu().expect("disc present");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.stub_outside = Some((INIT, INIT + 0x444));
        let page_rc = Rc::new(page.clone());
        let pg = page_rc.clone();
        cpu.hook(STORE_IMAGE, move |r, m| {
            let rect: [u16; 4] = std::array::from_fn(|i| m.rd16(r.r[A0] + 2 * i as u32));
            let (x0, y0) = ((rect[0] - 0x340) as usize * 4, (rect[1] - 0x100) as usize);
            let mut out = r.r[A1];
            for row in 0..rect[3] as usize {
                for hw in 0..rect[2] as usize {
                    let at = |k: usize| {
                        let (x, y) = (x0 + hw * 4 + k, y0 + row);
                        if x < PAGE_W && y < PAGE_H {
                            pg[y * PAGE_W + x]
                        } else {
                            0
                        }
                    };
                    m.wr8(out, at(0) | (at(1) << 4));
                    m.wr8(out + 1, at(2) | (at(3) << 4));
                    out += 2;
                }
            }
        });
        let next = Rc::new(Cell::new(HEAP));
        let sizes = Rc::new(RefCell::new(Vec::new()));
        let (n, s) = (next.clone(), sizes.clone());
        cpu.hook(ALLOC, move |r, m| {
            let at = n.get();
            s.borrow_mut().push(r.r[A1]);
            m.load(at, &vec![0xEE; r.r[A1] as usize]);
            n.set(at + ((r.r[A1] + 0xFF) & !0xFF).max(0x100));
            r.r[V0] = at;
        });
        cpu.stub(SPAWN_ACTOR, 0x8014_0000);
        // Its decompression scratch at `_DAT_8007B85C`.
        cpu.mem.wr32(0x8007_B85C, 0x8018_0000);
        cpu.mem.wr16(0x8007_B8C2, 1);
        assert_eq!(cpu.call(INIT, &[]), Stop::Returned, "run {run}");
        let port = parse_messages(&overlay, &page, PAGE_W).expect("records");
        for (i, msg) in port.iter().enumerate() {
            let rec = RECORDS + (i * MESSAGE_STRIDE) as u32;
            let buf = cpu.mem.rd32(rec + 4);
            assert_eq!(
                sizes.borrow()[i],
                u32::from(msg.h) << 8,
                "run {run} msg {i}: size"
            );
            let pairs = usize::from(msg.w) / 2 * 2;
            for row in 0..usize::from(msg.h) {
                for col in 0..pairs {
                    let got = cpu.mem.rd8(buf + (row * 0x100 + col) as u32);
                    let want = msg.bitmap[row * usize::from(msg.w) + col] << 2;
                    let rect = (msg.u, msg.v, msg.w, msg.h);
                    assert_eq!(got, want, "run {run} msg {i} {rect:?} row {row} col {col}");
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 200_000, "only {compared} texels");
}
