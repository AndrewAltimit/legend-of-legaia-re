//! The status-palette restore `FUN_8005133C` (a cure taking Stone or a Rot
//! limb off a party seat) against `StatusClutState`'s cure: retail uploads
//! the seat's pristine palette copy `ctx[+0x894 + slot * 0x1E0]` back over
//! VRAM row `481 + slot` (`LoadImage` `FUN_800583C8` on the rect it builds in
//! the packet buffer) and sets the four tint latches `+0x220..+0x223`; the
//! port, after a mark leaves the status word, writes its snapshot of the same
//! row back on the next pass, and a mark the cure left re-fires its tint.
//!
//! Every slot runs over generated palettes and marks: the rect, the entries
//! retail hands the upload, and the row the port leaves, compared whole.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_status_clut::{
    PARTY_CLUT_ENTRIES, PARTY_CLUT_ROW_BASE, PARTY_CLUT_SLOTS, StatusClutState,
};
use psx_retail_exec::{A0, A1, Stop};
use psx_tim::Vram;
use std::cell::RefCell;
use std::rc::Rc;

const RESTORE: u32 = 0x8005_133C;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
/// The packet buffer's cursor `0x1F800314 + 0x8C`.
const CURSOR: u32 = 0x1F80_03A0;
const PACKETS: u32 = 0x8010_0000;

/// One `LoadImage`: its rect and the entries it hands over.
type Load = ([u16; 4], Vec<u16>);

#[test]
fn the_cure_puts_the_palette_back_as_retail_does() {
    differential!(scus(RESTORE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    let loads: Rc<RefCell<Vec<Load>>> = Rc::default();
    {
        let loads = loads.clone();
        cpu.hook(LOAD_IMAGE, move |r, m| {
            let rect = [0, 2, 4, 6].map(|o| m.rd16(r.r[A0] + o));
            let n = u32::from(rect[2]) * u32::from(rect[3]);
            let data = (0..n).map(|i| m.rd16(r.r[A1] + 2 * i)).collect();
            loads.borrow_mut().push((rect, data));
        });
    }
    let mut rng = Rng(0x434C_5554);
    let mut failures = Vec::new();
    let mut re_fired = 0;
    for round in 0..60 {
        let slot = round % PARTY_CLUT_SLOTS as u32;
        // A resident palette (no all-zero row: the port snapshots only a
        // resident one), the mark the cure takes off and the one it leaves.
        let pristine: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
            .map(|_| 1 + rng.below(0x7FFF) as u16)
            .collect();
        let taken = [0x04u16, 0x08, 0x10, 0x20][rng.below(4) as usize];
        let left = if rng.below(2) == 0 {
            0
        } else {
            [0x08u16, 0x10, 0x20][rng.below(3) as usize] & !taken
        };

        // Retail: the copy in the context, the call.
        for (i, &c) in pristine.iter().enumerate() {
            cpu.mem.wr16(CTX + 0x894 + slot * 0x1E0 + 2 * i as u32, c);
        }
        for s in 0..8 {
            cpu.mem.wr32(ACTORS + 4 * s, ACTOR + s * 0x400);
        }
        let a = ACTOR + slot * 0x400;
        for o in 0..4 {
            cpu.mem.wr8(a + 0x220 + o, 0);
        }
        cpu.mem.wr32(CURSOR, PACKETS);
        loads.borrow_mut().clear();
        assert_eq!(cpu.call(RESTORE, &[slot]), Stop::Returned);
        let retail_loads = loads.borrow().clone();
        let latches: Vec<u8> = (0..4).map(|o| cpu.mem.rd8(a + 0x220 + o)).collect();

        // Port: the row resident, the mark raised and tinted, then cured.
        let mut vram = Vram::new();
        let row = PARTY_CLUT_ROW_BASE + slot as u16;
        let bytes: Vec<u8> = pristine.iter().flat_map(|c| c.to_le_bytes()).collect();
        vram.write_clut_row(0, row, &bytes);
        let mut st = StatusClutState::default();
        st.set_rot_windows([Some([0x00, 0x40, 0x40, 0x80, 0x80, 0xC0]); PARTY_CLUT_SLOTS]);
        st.sync_word(slot as u8, taken | left);
        st.step(&mut vram);
        st.sync_word(slot as u8, left);
        st.step(&mut vram);
        let port_row: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
            .map(|x| vram.pixel(x, usize::from(row)))
            .collect();

        let want_rect = [0u16, row, PARTY_CLUT_ENTRIES as u16, 1];
        match retail_loads.as_slice() {
            [(rect, data)] => {
                if *rect != want_rect {
                    failures.push(format!(
                        "round {round} slot {slot}: rect retail {rect:?} port {want_rect:?}"
                    ));
                }
                // With no mark left the row is the copy; a mark left
                // re-tints its own window on the same pass, outside which
                // the row is the copy too.
                let window = match left {
                    0x08 => 0x00..0x40,
                    0x10 => 0x40..0x80,
                    0x20 => 0x80..0xC0,
                    _ => 0..0,
                };
                let outside = |v: &[u16]| -> Vec<u16> {
                    v.iter()
                        .enumerate()
                        .filter(|(i, _)| !window.contains(i))
                        .map(|(_, &c)| c)
                        .collect()
                };
                if outside(data) != outside(&port_row) {
                    failures.push(format!(
                        "round {round} slot {slot}: the restored row differs (taken {taken:#x}, left {left:#x})"
                    ));
                }
                if left != 0 {
                    re_fired += 1;
                    if port_row[window.clone()] == data[window.clone()] {
                        failures.push(format!(
                            "round {round} slot {slot}: the mark left ({left:#x}) did not re-fire"
                        ));
                    }
                }
            }
            other => failures.push(format!(
                "round {round} slot {slot}: retail made {} uploads",
                other.len()
            )),
        }
        // The re-arm: every latch up, so a mark the cure left re-fires.
        if latches != [1, 1, 1, 1] {
            failures.push(format!("round {round} slot {slot}: latches {latches:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(re_fired > 10, "{re_fired}");
}
