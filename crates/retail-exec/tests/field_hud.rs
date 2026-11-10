//! The field party HUD's draw half in `FUN_801D0D38` (PROT 0897) against
//! `engine-ui::field_party_hud`: the routine is driven to its draw (no
//! script lock, a still player, the countdown spent) over generated parties
//! and records, at both of its rows, and every leaf draw is logged with the
//! ink word it ran under: the name `FUN_80036888`, the label pictograms
//! `FUN_8002C488`, the numerals `FUN_80034B78` and the separators
//! `FUN_8003C1F8`. The two ink routines `FUN_800349EC` / `FUN_80035EA8` run
//! live. The log is held to the one the port's seats and its ink law
//! (`field_hud_readout_inks`) call for.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_ui::field_party_hud::{
    CUR_FIELD_X, FIELD_HUD_PITCH, FIELD_HUD_X0, FieldHudMember, HP_LABEL_DY, LV_FIELD_CELLS,
    LV_FIELD_X, LV_LABEL, MAX_FIELD_X, MP_LABEL_DY, ROW_HP_DY, ROW_MP_DY, SEPARATOR_DX,
    VALUE_FIELD_CELLS, field_hud_readout_inks,
};
use psx_retail_exec::{A0, A1, A2, A3, Memory, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const HUD: u32 = 0x801D_0D38;
const INK: u32 = 0x8007_B454;
const RECORDS: u32 = 0x8008_4708;
const PARTY_COUNT: u32 = 0x8008_4594;
const PARTY: u32 = 0x8008_4598;
const ACTOR_SLOT: u32 = 0x8007_C364;
const ACTOR: u32 = 0x8012_0000;
/// The HUD's remembered player position and its countdown.
const LAST_POS: u32 = 0x801F_3488;
const COUNTDOWN: u32 = 0x801F_348C;
/// The labels' pictogram ids (`li a2,0xa` / `0x3f` / `0x40`).
const PICTO_LV: u32 = 0xA;
const PICTO_HP: u32 = 0x3F;
const PICTO_MP: u32 = 0x40;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ev {
    Text(Vec<u8>, i32, i32, u8),
    Picto(u32, i32, i32),
    Number(i32, i32, i32, i32, u8),
    Separator(u32, i32, i32, u8),
}

fn cstr(m: &mut Memory, mut a: u32) -> Vec<u8> {
    let mut s = Vec::new();
    while m.rd8(a) != 0 {
        s.push(m.rd8(a));
        a += 1;
    }
    s
}

#[test]
fn the_field_hud_draws_what_retail_draws() {
    differential!(prot(897, HUD));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let log: Rc<RefCell<Vec<Ev>>> = Rc::default();
    let l = log.clone();
    cpu.hook(0x8003_6888, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        let s = cstr(m, r.r[A0]);
        let ink = m.rd32(INK) as u8;
        l.borrow_mut().push(Ev::Text(s, r.r[A3] as i32, y, ink));
    });
    let l = log.clone();
    cpu.hook(0x8002_C488, move |r, _| {
        l.borrow_mut()
            .push(Ev::Picto(r.r[A2], r.r[A0] as i32, r.r[A1] as i32));
    });
    let l = log.clone();
    cpu.hook(0x8003_4B78, move |r, m| {
        let ink = m.rd32(INK) as u8;
        l.borrow_mut().push(Ev::Number(
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3] as i32,
            ink,
        ));
    });
    let l = log.clone();
    cpu.hook(0x8003_C1F8, move |r, m| {
        let ink = m.rd32(INK) as u8;
        l.borrow_mut()
            .push(Ev::Separator(r.r[A0], r.r[A1] as i32, r.r[A2] as i32, ink));
    });
    // The scene test that sends the HUD to its low row; the other arm projects
    // the player, and the hook answers with a screen y on either side of the
    // dodge line.
    let low = Rc::new(RefCell::new((true, 0i16)));
    let lo = low.clone();
    cpu.hook(0x8003_CF04, move |r, _| {
        r.r[psx_retail_exec::V0] = u32::from(lo.borrow().0)
    });
    cpu.stub(0x8001_72C0, 0);
    let lo = low.clone();
    cpu.hook(0x8001_95A8, move |r, m| {
        let p = m.rd32(r.r[SP] + 0x10);
        m.wr16(p + 2, lo.borrow().1 as u16);
    });
    let mut rng = Rng(0x4655_4444);
    let mut fallen = 0;
    for run in 0..6_000 {
        // The gates the decision half reads, all open.
        for (va, v) in [
            (0x8007_B868u32, 0u32),
            (0x8007_B850, 0),
            (0x8007_B6B4, 0),
            (0x8007_B6B0, 0),
            (0x1F80_0394, 0),
        ] {
            cpu.mem.wr32(va, v);
        }
        cpu.mem.wr32(0x8008_45C4, rng.below(2));
        cpu.mem.wr8(0x1F80_0393, 1);
        cpu.mem.wr32(ACTOR_SLOT, ACTOR);
        cpu.mem.wr32(ACTOR + 0x10, 0);
        let (px, pz) = (rng.next() as u16, rng.next() as u16);
        cpu.mem.wr16(ACTOR + 0x14, px);
        cpu.mem.wr16(ACTOR + 0x18, pz);
        cpu.mem.wr16(LAST_POS, px);
        cpu.mem.wr16(LAST_POS + 2, pz);
        cpu.mem.wr16(COUNTDOWN, 1);
        cpu.mem.wr32(0x1F80_03A0, 0x8014_0000);
        cpu.mem.wr32(0x1F80_03F4, 0x8016_0000);
        let projected = rng.below(0x60) as i16;
        *low.borrow_mut() = (rng.below(2) == 0, projected);
        let y = if low.borrow().0 || projected < 0x30 {
            0xAA
        } else {
            0xC
        };
        // The party and its records.
        let n = 1 + rng.below(3);
        let ids: Vec<u32> = (0..n).map(|_| rng.below(4)).collect();
        cpu.mem.wr8(PARTY_COUNT, n as u8);
        let mut members = Vec::new();
        let mut names = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            cpu.mem.wr8(PARTY + i as u32, id as u8);
            let rec = RECORDS + id * 0x414;
            let hp_max = [rng.below(10_000), 0, 9999][rng.below(3) as usize] as u16;
            let hp = match rng.below(4) {
                0 => 0,
                1 => hp_max,
                _ => rng.below(u32::from(hp_max) + 1) as u16,
            };
            let mp_max = rng.below(1000) as u16;
            let mp = rng.below(u32::from(mp_max) + 1) as u16;
            let status = [0u16, 0, 1, rng.next() as u16][rng.below(4) as usize];
            let level = 1 + rng.below(99) as u8;
            cpu.mem.wr16(rec + 0x104, hp_max);
            cpu.mem.wr16(rec + 0x106, hp);
            cpu.mem.wr16(rec + 0x108, mp_max);
            cpu.mem.wr16(rec + 0x10A, mp);
            cpu.mem.wr16(rec + 0x12E, status);
            cpu.mem.wr8(rec + 0x130, level);
            let name = format!("M{id}{}", rng.below(100));
            for (k, b) in name.bytes().chain([0]).enumerate() {
                cpu.mem.wr8(rec + 0x2A7 + k as u32, b);
            }
            names.push(name);
            fallen += u32::from(hp == 0 && mp > mp_max / 4);
            members.push((hp, hp_max, mp, mp_max, status, level));
        }
        log.borrow_mut().clear();
        assert_eq!(cpu.call(HUD, &[]), Stop::Returned, "run {run}");
        // A record seated twice shows the later write in both columns.
        let mut want = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            let rec = RECORDS + id * 0x414;
            let m = FieldHudMember {
                name: &names[i],
                level: cpu.mem.rd8(rec + 0x130),
                hp: cpu.mem.rd16(rec + 0x106),
                hp_max: cpu.mem.rd16(rec + 0x104),
                mp: cpu.mem.rd16(rec + 0x10A),
                mp_max: cpu.mem.rd16(rec + 0x108),
                status: cpu.mem.rd16(rec + 0x12E),
            };
            let name = cstr(&mut cpu.mem, rec + 0x2A7);
            let (hp_ink, mp_ink) = field_hud_readout_inks(&m);
            let x = FIELD_HUD_X0 + i as i32 * FIELD_HUD_PITCH;
            let (hp_y, mp_y) = (y + ROW_HP_DY, y + ROW_MP_DY);
            want.extend([
                Ev::Text(name, x, y, 7),
                Ev::Picto(PICTO_LV, x + LV_LABEL.0, y + LV_LABEL.1),
                Ev::Number(i32::from(m.level), LV_FIELD_CELLS, x + LV_FIELD_X, y, 7),
                Ev::Picto(PICTO_HP, x, y + HP_LABEL_DY),
                Ev::Number(
                    i32::from(m.hp),
                    VALUE_FIELD_CELLS,
                    x + CUR_FIELD_X,
                    hp_y,
                    hp_ink,
                ),
                Ev::Number(
                    i32::from(m.hp_max),
                    VALUE_FIELD_CELLS,
                    x + MAX_FIELD_X,
                    hp_y,
                    hp_ink,
                ),
                Ev::Separator(6, x + SEPARATOR_DX, hp_y, 7),
                Ev::Picto(PICTO_MP, x, y + MP_LABEL_DY),
                Ev::Number(
                    i32::from(m.mp),
                    VALUE_FIELD_CELLS,
                    x + CUR_FIELD_X,
                    mp_y,
                    mp_ink,
                ),
                Ev::Number(
                    i32::from(m.mp_max),
                    VALUE_FIELD_CELLS,
                    x + MAX_FIELD_X,
                    mp_y,
                    mp_ink,
                ),
                Ev::Separator(6, x + SEPARATOR_DX, mp_y, 7),
            ]);
        }
        assert_eq!(*log.borrow(), want, "run {run} y {y} {members:?}");
    }
    assert!(fallen > 300, "{fallen}");
}
