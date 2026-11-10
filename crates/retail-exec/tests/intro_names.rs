//! The battle-intro enemy-name banner `FUN_801D9D3C` (PROT 0898) against
//! `enemy_menu_rows` + `layout_enemy_menu_rows`: formations with runs, gaps and
//! repeats, names of every length, monsters spread across the stage or bunched
//! so the labels collide and clamp. The width measure on the port side is
//! `Font::measure`, which `text_width` holds to retail's `FUN_80035F04`.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_battle::target_picker::{DEDUP_SUFFIX, enemy_menu_rows, layout_enemy_menu_rows};
use legaia_font::{Font, MeasureOptions};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const BANNER: u32 = 0x801D_9D3C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FORMATION: u32 = 0x8007_BD0C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const PAD: u32 = 0x8007_40E8;

const BASES: [&str; 6] = [
    "Gobu Gobu",
    "Killer Bee",
    "Rim",
    "Seru Mule",
    "X",
    "Lapis Vine",
];

#[test]
fn the_intro_banner_lays_the_names_out_as_retail_does() {
    differential!(prot(898, BANNER));
    let (Some(mut cpu), Some(disc), Some(scus)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let tim = disc
        .read_prot_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .expect("font TIM");
    let font = Font::from_disc_tim_and_scus(&tim, &scus).expect("font");
    cpu.hook(0x8005_6788, |r, m| {
        for i in 0..r.r[A2] {
            m.wr8(r.r[A0] + i, r.r[A1] as u8);
        }
    });
    // The text-actor spawn: record each label and its pen X.
    let drawn: Rc<RefCell<Vec<(String, i16)>>> = Rc::default();
    let sink = drawn.clone();
    cpu.hook(0x8003_541C, move |r, m| {
        let mut s = Vec::new();
        let mut p = r.r[A2];
        loop {
            let b = m.rd8(p);
            if b == 0 {
                break;
            }
            s.push(b);
            p += 1;
        }
        let _ = m.rd32(r.r[SP] + 0x10);
        sink.borrow_mut()
            .push((String::from_utf8_lossy(&s).into_owned(), r.r[A3] as i16));
    });
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(PAD, 0);
    let mut rng = Rng(0x494E_5452);
    let (mut collided, mut runs, mut clamped, mut spins) = (0, 0, 0, 0);
    for round in 0..8000 {
        for b in 0..0x300 {
            cpu.mem.wr8(CTX + b, 0);
        }
        let mut ids = [0u8; 4];
        let mut names: Vec<String> = vec![String::new(); 4];
        let spread = [0x80i32, 0x400, 0x1000][rng.below(3) as usize];
        let mut xs = [0i16; 4];
        for s in 0..4usize {
            ids[s] = match rng.below(4) {
                0 => 0,
                1 if s > 0 => ids[s - 1],
                _ => 1 + rng.below(4) as u8,
            };
            let base = BASES[usize::from(ids[s]) % BASES.len()];
            names[s] = format!("{base} {}", (b'A' + rng.below(4) as u8) as char);
            xs[s] = (rng.below(2 * spread as u32) as i32 - spread) as i16;
            let a = ACTOR + s as u32 * 0x400;
            cpu.mem.wr32(ACTORS + (3 + s as u32) * 4, a);
            let mut z = names[s].clone().into_bytes();
            z.push(0);
            cpu.mem.load(a + 0x1BC, &z);
            cpu.mem.wr16(a + 0x34, xs[s] as u16);
            cpu.mem.wr8(FORMATION + s as u32, ids[s]);
        }
        drawn.borrow_mut().clear();
        cpu.step_budget = 400_000;
        if let Stop::StepLimit { .. } = cpu.call(BANNER, &[]) {
            // Retail's relax-and-clamp loop has no bound and never settles on
            // labels that cannot fit the screen side by side; the port stops
            // after its safety bound instead.
            let widths: Vec<u32> = (0..4)
                .map(|s| {
                    font.measure(names[s].as_bytes(), &MeasureOptions::default())
                        .max_px
                })
                .collect();
            if spins == 0 {
                eprintln!("spins: ids {ids:?} names {names:?} xs {xs:?} widths {widths:?}");
            }
            spins += 1;
            continue;
        }
        let retail = drawn.borrow().clone();

        let mut rows = enemy_menu_rows(
            ids,
            DEDUP_SUFFIX,
            |s| names[usize::from(s)].clone(),
            |s| xs[usize::from(s)],
        );
        let width = |s: &str| {
            font.measure(s.as_bytes(), &MeasureOptions::default())
                .max_px as i16
        };
        let centred: Vec<i16> = rows
            .iter()
            .map(|r| ((r.x / i16::from(r.members)) >> 3) - width(&r.label) / 2 + 0xA0)
            .collect();
        layout_enemy_menu_rows(&mut rows, width);
        let port: Vec<(String, i16)> = rows.iter().map(|r| (r.label.clone(), r.x)).collect();
        assert_eq!(
            retail, port,
            "round {round}: ids {ids:?} names {names:?} xs {xs:?}"
        );
        runs += i32::from(rows.iter().any(|r| r.members > 1));
        collided += i32::from(port.iter().zip(&centred).any(|(p, c)| p.1 != *c));
        clamped += i32::from(port.iter().any(|p| p.1 == 6));
    }
    assert!(
        runs > 1000 && collided > 1000 && clamped > 50,
        "{runs} {collided} {clamped}"
    );
}
