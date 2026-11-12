//! Baka Fighter's developer action-table dump `FUN_801D553C` (PROT 0976),
//! run over the disc's seventeen action tables and over randomised ones,
//! against `action_table_dump`: the buffer it hands the write `FUN_8003E7F0`,
//! line for line, as the kind of each line and the integers it prints.
//! Retail's labels are overlay rodata, so a header line is held to its kind
//! alone.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::baka_cabinet::action_table_dump;
use legaia_minigame_formats::baka_opponents::{
    ACTION_PTR_TABLE_VA, ACTION_RECORD_STRIDE, ACTIONS_PER_FIGHTER, OPPONENT_COUNT, parse_actions,
};
use psx_retail_exec::{A0, A1, A2, Memory, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const DUMP: u32 = 0x801D_553C;
const ALLOC: u32 = 0x8001_7888;
const FREE: u32 = 0x8001_7B94;
const WRITE: u32 = 0x8003_E7F0;
/// The BIOS `strcpy` / `strlen` thunks (`A0` calls `0x19` / `0x1B`).
const STRCPY: u32 = 0x8005_6758;
const STRLEN: u32 = 0x8005_6768;
const BUFFER: u32 = 0x8010_0000;

fn cstr(m: &mut Memory, mut a: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = m.rd8(a);
        if b == 0 {
            return out;
        }
        out.push(b);
        a += 1;
    }
}

/// A line's kind (its first non-blank byte) and the integers on it; a header
/// line keeps its kind alone.
fn shape(text: &str) -> Vec<(char, Vec<i64>)> {
    text.lines()
        .map(|l| {
            let t = l.trim_start();
            let kind = t.chars().next().unwrap_or(' ');
            if t.starts_with("ACT_TBL") {
                return ('A', Vec::new());
            }
            let nums = t
                .split(|c: char| !(c.is_ascii_digit() || c == '-'))
                .filter(|s| !s.is_empty() && *s != "-")
                .map(|s| s.parse().expect("an integer"))
                .collect();
            (kind, nums)
        })
        .collect()
}

#[test]
fn the_action_dump_prints_what_retail_prints() {
    differential!(prot(976, DUMP));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(976).expect("PROT 0976");
    // The CD-state probe at the head and the buffer's free.
    cpu.stub(0x8005_8104, 0);
    cpu.stub(FREE, 0);
    cpu.hook(ALLOC, |r, _| r.r[V0] = BUFFER);
    cpu.hook(STRCPY, |r, m| {
        let s = cstr(m, r.r[A1]);
        m.load(r.r[A0], &s);
        m.wr8(r.r[A0] + s.len() as u32, 0);
        r.r[V0] = r.r[A0];
    });
    cpu.hook(STRLEN, |r, m| r.r[V0] = cstr(m, r.r[A0]).len() as u32);
    let written = Rc::new(RefCell::new(None::<Vec<u8>>));
    {
        let written = written.clone();
        cpu.hook(WRITE, move |r, m| {
            let len = r.r[A2] as usize;
            *written.borrow_mut() = Some(m.read(r.r[A1], len));
        });
    }
    let mut rng = Rng(0x4455_4D50);
    let mut keyframe_lines = 0usize;
    for round in 0..24 {
        let mut image = overlay.clone();
        if round > 0 {
            // Every record's words noise, its sub-keyframe count `0..=8`.
            let ptrs = (ACTION_PTR_TABLE_VA - OVERLAY_BASE) as usize;
            for f in 0..OPPONENT_COUNT {
                let p = ptrs + 4 * f;
                let va = u32::from_le_bytes(image[p..p + 4].try_into().unwrap());
                let t = (va - OVERLAY_BASE) as usize;
                for a in 0..ACTIONS_PER_FIGHTER {
                    let r = t + a * ACTION_RECORD_STRIDE;
                    for b in &mut image[r..r + ACTION_RECORD_STRIDE] {
                        *b = rng.next() as u8;
                    }
                    let n = rng.below(9);
                    image[r + 0x1C..r + 0x20].copy_from_slice(&n.to_le_bytes());
                }
            }
        }
        cpu.mem.load(OVERLAY_BASE, &image);
        *written.borrow_mut() = None;
        assert_eq!(cpu.call(DUMP, &[]), Stop::Returned, "round {round}");
        let text = written.borrow_mut().take().expect("the dump is written");
        let retail = String::from_utf8(text).expect("ASCII");
        let tables = parse_actions(&image).expect("action tables");
        let port = action_table_dump(&tables);
        let (r, p) = (shape(&retail), shape(&port));
        assert_eq!(r.len(), p.len(), "line count, round {round}");
        for (k, (a, b)) in r.iter().zip(&p).enumerate() {
            assert_eq!(a, b, "line {k}, round {round}");
        }
        keyframe_lines += r.iter().filter(|(c, n)| *c == ',' && n.len() == 4).count();
    }
    assert!(keyframe_lines > 1000, "sub-keyframe lines {keyframe_lines}");
}
