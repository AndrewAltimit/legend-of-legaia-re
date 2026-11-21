//! The HUD element spawner `FUN_801D8DE8` (PROT 0898) raising a member's
//! HP / MP readout - the roster panels (elements `6` / `0x4E` / `0x4F`, one
//! per seat) and the active-actor bar (elements `7` / `0x34`) - in every
//! spawn mode, against `legaia_engine_ui::battle_chrome`'s readout kernels
//! (`readout_panel_element`, `status_readout_records`), the records the
//! hosts lay their figure fields out from.
//!
//! The text-actor register `FUN_8003541C`, the glide leaf `FUN_801DB7B0` and
//! the number-record append `FUN_8003563C` are hooked; every other call is
//! stubbed. Each record is compared whole: the actor field its pointer
//! names, its kind, its seat inside the window, its cell count and its ink.
//! Random parties mix Terra (character id `4`, the hidden kind-5 rows) with
//! members whose displayed HP sits at zero, on a band edge or under a
//! status word.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_ui::battle_chrome::{
    ReadoutField, ReadoutRecord, ReadoutSurface, readout_panel_element, status_readout_records,
};
use psx_retail_exec::{A1, A2, A3, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SPAWN: u32 = 0x801D_8DE8;
const REGISTER: u32 = 0x8003_541C;
const GLIDE: u32 = 0x801D_B7B0;
const RECORD: u32 = 0x8003_563C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const CHAR_IDS: u32 = 0x8007_BD10;
/// `gp+0xA49`: the seat the bar's gauges follow.
const SHOWN: u32 = 0x8007_BD61;

fn field_of(off: u32) -> ReadoutField {
    match off {
        0x172 => ReadoutField::Hp,
        0x14E => ReadoutField::HpMax,
        0x174 => ReadoutField::Mp,
        0x152 => ReadoutField::MpMax,
        _ => panic!("record at actor +{off:#x}"),
    }
}

/// A figure near a band edge of `max` (or anywhere), sometimes zero.
fn figure(rng: &mut Rng, max: u16) -> u16 {
    let m = u32::from(max);
    let v = match rng.below(6) {
        0 => 0,
        1 => m >> 1,
        2 => (m >> 1) + 1,
        3 => m >> 2,
        4 => (m >> 2) + 1,
        _ => rng.below(m + 1),
    };
    v.min(m) as u16
}

#[test]
fn the_status_readouts_register_retails_records() {
    differential!(prot(898, SPAWN));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(898).expect("PROT 0898"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    let registered = Rc::new(RefCell::new(0u32));
    let r2 = registered.clone();
    cpu.hook(REGISTER, move |r, _| {
        *r2.borrow_mut() += 1;
        r.r[V0] = 0x8015_0000;
    });
    cpu.hook(GLIDE, |_, _| {});
    let records = Rc::new(RefCell::new(Vec::<(u32, ReadoutRecord)>::new()));
    let rec2 = records.clone();
    cpu.hook(RECORD, move |r, m| {
        let ptr = r.r[A1];
        let seat = (ptr - ACTOR) / ACTOR_STRIDE;
        let rec = ReadoutRecord {
            field: field_of(ptr - ACTOR - seat * ACTOR_STRIDE),
            kind: r.r[A2] as u8,
            x: r.r[A3] as i16,
            y: m.rd32(r.r[SP] + 0x10) as i16,
            cells: m.rd32(r.r[SP] + 0x14) as u8,
            tier: m.rd32(r.r[SP] + 0x18) as u8,
        };
        rec2.borrow_mut().push((seat, rec));
        r.r[V0] = 0x8016_0000;
    });
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..3 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * ACTOR_STRIDE);
    }
    let mut rng = Rng(0x5245_4144);
    let (mut refused, mut hidden, mut dead, mut ailing) = (0, 0, 0, 0);
    for round in 0..3000 {
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        let count = 1 + rng.below(3) as u8;
        cpu.mem.wr8(CTX, count);
        let (acting, target) = (rng.below(3) as u8, rng.below(4) as u8);
        cpu.mem.wr8(CTX + 0x20, acting);
        cpu.mem.wr8(CTX + 0x21, target);
        cpu.mem.wr8(CTX + 0x1C, 8);
        cpu.mem.wr8(CTX + 0x1B, rng.below(2) as u8);
        cpu.mem.wr8(SHOWN, 0xEE);
        let mut figures = [((0u16, 0u16), (0u16, 0u16), 0u16, 0u8); 3];
        for (s, f) in figures.iter_mut().enumerate() {
            let s = s as u32;
            let char_id = 1 + rng.below(4) as u8;
            cpu.mem.wr8(CHAR_IDS + s, char_id);
            let hp_max = 1 + rng.below(9999) as u16;
            let mp_max = rng.below(1000) as u16;
            let hp = figure(&mut rng, hp_max);
            let mp = figure(&mut rng, mp_max);
            let status = if rng.below(4) == 0 {
                1 << rng.below(16)
            } else {
                0
            };
            let a = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr16(a + 0x172, hp);
            cpu.mem.wr16(a + 0x14E, hp_max);
            cpu.mem.wr16(a + 0x174, mp);
            cpu.mem.wr16(a + 0x152, mp_max);
            cpu.mem.wr16(a + 0x16E, status);
            *f = ((hp, hp_max), (mp, mp_max), status, char_id);
        }
        let (element, surface, seat) = match rng.below(5) {
            0 => (6, ReadoutSurface::Panel, 0u8),
            1 => (0x4E, ReadoutSurface::Panel, 1),
            2 => (0x4F, ReadoutSurface::Panel, 2),
            3 => (7, ReadoutSurface::Bar, acting),
            _ => (0x34, ReadoutSurface::Bar, target.min(2)),
        };
        let mode = rng.below(4);
        *registered.borrow_mut() = 0;
        records.borrow_mut().clear();
        assert_eq!(
            cpu.call(SPAWN, &[element, mode]),
            Stop::Returned,
            "round {round}"
        );
        let label =
            format!("round {round}: element {element:#x} mode {mode} party {count} seat {seat}");
        let raised = match surface {
            ReadoutSurface::Panel => readout_panel_element(seat, count) == Some(element as u8),
            ReadoutSurface::Bar => true,
        };
        if !raised {
            refused += 1;
            assert_eq!(*registered.borrow(), 0, "{label}: no window");
            assert!(records.borrow().is_empty(), "{label}: no records");
            continue;
        }
        assert_eq!(*registered.borrow(), 1, "{label}");
        let (hp, mp, status, char_id) = figures[seat as usize];
        let is_hidden = char_id == 4;
        hidden += usize::from(is_hidden);
        dead += usize::from(!is_hidden && hp.0 == 0);
        ailing += usize::from(!is_hidden && hp.0 != 0 && status != 0);
        let port = status_readout_records(surface, is_hidden, hp, mp, status);
        let retail = records.borrow().clone();
        assert_eq!(retail.len(), 4, "{label}");
        for (i, (s, r)) in retail.iter().enumerate() {
            assert_eq!(
                u32::from(seat),
                *s,
                "{label}: record {i} reads its own seat"
            );
            assert_eq!(
                *r, port[i],
                "{label}: record {i} ({hp:?} {mp:?} {status:#x})"
            );
        }
        // The bar names the seat its gauges follow; a hidden one does not.
        let shown = cpu.mem.rd8(SHOWN);
        if surface == ReadoutSurface::Bar && !is_hidden {
            assert_eq!(shown, seat, "{label}: gp+0xA49");
        } else {
            assert_eq!(shown, 0xEE, "{label}: gp+0xA49 untouched");
        }
    }
    assert!(
        refused > 50 && hidden > 50 && dead > 50 && ailing > 50,
        "{refused} {hidden} {dead} {ailing}"
    );
}
