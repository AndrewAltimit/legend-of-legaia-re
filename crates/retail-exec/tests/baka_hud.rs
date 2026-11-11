//! Baka Fighter's duel HUD renderer `FUN_801D2AFC` (PROT 0976), whole, over
//! generated fighter blocks and cabinet globals, against `hud_frame` and
//! `duel_hud_packets`: every packet it links itself (`FUN_8003D2C4`) decoded -
//! the two VITAL bars, the bar frames, the combo digits and "HIT!", the round
//! pips and the attack icons - and its calls to the widget emitter
//! `FUN_801D5ED0`, the digit drawer `FUN_801D69E4` and the number drawer
//! `FUN_801D6A18` by their arguments, with the running-max latches it writes.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::baka_cabinet::{
    DuelHudPacket, HudInput, VitalBar, duel_hud_packets, hud_frame,
};
use legaia_engine_minigames::baka_fighter::HudWidgetQuad;
use legaia_minigame_formats::baka_opponents::parse_baka_bar_frame;
use psx_retail_exec::{A0, A1, A2, A3, Memory, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const HUD: u32 = 0x801D_2AFC;
const ADD_PRIM: u32 = 0x8003_D2C4;
const EMIT: u32 = 0x801D_5ED0;
const DIGIT: u32 = 0x801D_69E4;
const NUMBER: u32 = 0x801D_6A18;
const BLOCKS: u32 = 0x801D_BFBC;
const BLOCK_STRIDE: u32 = 0xA8;
const CURSOR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;
const WIN_TARGET: u32 = 0x801D_BED0;
const STAGE: u32 = 0x801D_C110;
const HIGH_SCORE: u32 = 0x801D_BEE4;
const LATCHES: [u32; 2] = [0x801D_BEC8, 0x801D_BF58];
const BLINK: u32 = 0x801D_C12C;

fn xy(m: &mut Memory, a: u32) -> (i16, i16) {
    (m.rd16(a) as i16, m.rd16(a + 2) as i16)
}

fn rgb(m: &mut Memory, a: u32) -> [u8; 3] {
    [m.rd8(a), m.rd8(a + 1), m.rd8(a + 2)]
}

/// One linked packet in the port's terms: a gouraud bar (`0x3C`) by its
/// vertices, a flat textured cell (`0x2C`) as a `HudWidgetQuad`.
fn decode(m: &mut Memory, p: u32, bars: &mut usize) -> DuelHudPacket {
    let code = m.rd8(p + 7);
    if code & 0xFC == 0x3C {
        let slot = *bars;
        *bars += 1;
        let ((x0, y0), (x1, _), (x2, y1), (x3, y3)) = (
            xy(m, p + 8),
            xy(m, p + 0x14),
            xy(m, p + 0x20),
            xy(m, p + 0x2C),
        );
        assert_eq!((x2, x3, y3), (x0, x1, y1), "bar corners");
        let (c0, c1) = (rgb(m, p + 4), rgb(m, p + 0x10));
        assert_eq!(
            (rgb(m, p + 0x1C), rgb(m, p + 0x28)),
            (c0, c1),
            "bar gouraud"
        );
        let uv = [
            m.rd16(p + 0xC),
            m.rd16(p + 0x18),
            m.rd16(p + 0x24),
            m.rd16(p + 0x30),
        ];
        assert_eq!(uv, [0xF000, 0xF002, 0xF200, 0xF202], "bar texel");
        assert_eq!(
            (m.rd16(p + 0xE), m.rd16(p + 0x1A)),
            (0x7F84, 0x1F),
            "bar clut / page"
        );
        let (rgb_far, rgb_anchor) = if slot == 0 { (c0, c1) } else { (c1, c0) };
        DuelHudPacket::Bar {
            slot,
            bar: VitalBar {
                x0,
                y0,
                x1,
                y1,
                rgb_far,
                rgb_anchor,
            },
        }
    } else {
        let uvs: [(u8, u8); 4] =
            [0xC, 0x14, 0x1C, 0x24].map(|o: u32| (m.rd8(p + o), m.rd8(p + o + 1)));
        let level = rgb(m, p + 4);
        let ((x0, y0), (x1, _), (_, y1)) = (xy(m, p + 8), xy(m, p + 0x10), xy(m, p + 0x18));
        DuelHudPacket::Cell {
            quad: HudWidgetQuad {
                poly_code: code,
                x0,
                y0,
                x1,
                y1,
                uv: uvs,
                rgb_top: level,
                rgb_bottom: level,
                clut: m.rd16(p + 0xE),
                tpage_attr: m.rd16(p + 0x16),
            },
            ot: 5,
        }
    }
}

#[test]
fn the_duel_hud_draws_what_the_port_draws() {
    differential!(prot(976, HUD));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(976).expect("PROT 0976");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let frame_cells = parse_baka_bar_frame(&overlay).expect("bar frame table");
    cpu.mem.wr32(OT_PTR, OT);
    let linked = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let l = linked.clone();
    cpu.hook(ADD_PRIM, move |r, _| {
        l.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    type Call = (u32, i32, i32, i32, i32);
    let calls = Rc::new(RefCell::new(Vec::<Call>::new()));
    for va in [EMIT, DIGIT, NUMBER] {
        let c = calls.clone();
        cpu.hook(va, move |r, _| {
            c.borrow_mut().push((
                va,
                r.r[A0] as i32,
                r.r[A1] as i32,
                r.r[A2] as i32,
                r.r[A3] as i32,
            ))
        });
    }
    let mut rng = Rng(0x4855_4444);
    let mut packets = 0usize;
    for case in 0..3000 {
        let pick = |rng: &mut Rng, xs: &[i32]| xs[rng.below(xs.len() as u32) as usize];
        let input = HudInput {
            hp: std::array::from_fn(|_| rng.below(0xC81) as i32),
            round_wins: std::array::from_fn(|_| rng.below(4)),
            win_target: rng.below(4),
            combo_taken: std::array::from_fn(|_| pick(&mut rng, &[0, 0, 1, 7, 10, 42, 123, 1000])),
            chosen_type: std::array::from_fn(|_| rng.below(5) as i32),
            type_hold: std::array::from_fn(|_| rng.below(20) as i32 - 2),
            stage_display: 1 + rng.below(20) as i32,
            high_score: pick(&mut rng, &[0, 9, 10, 99, 100, 12345, 99_999_999]),
            latch: rng.below(200) as i32,
            blink: rng.next() as i32,
        };
        for s in 0..2u32 {
            let b = BLOCKS + s * BLOCK_STRIDE;
            let i = s as usize;
            cpu.mem.wr32(b + 8, input.hp[i] as u32);
            cpu.mem.wr32(b + 0x34, input.round_wins[i]);
            cpu.mem.wr32(b + 0x30, input.combo_taken[i] as u32);
            cpu.mem.wr32(b + 0x24, input.chosen_type[i] as u32);
            cpu.mem.wr32(b + 0x8C, input.type_hold[i] as u32);
        }
        cpu.mem.wr32(WIN_TARGET, input.win_target);
        cpu.mem.wr32(STAGE, (input.stage_display - 1) as u32);
        cpu.mem.wr32(HIGH_SCORE, input.high_score as u32);
        for a in LATCHES {
            cpu.mem.wr32(a, input.latch as u32);
        }
        cpu.mem.wr32(BLINK, input.blink as u32);
        cpu.mem.wr32(CURSOR, PACKETS);
        linked.borrow_mut().clear();
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(HUD, &[]), Stop::Returned, "case {case}");

        let hud = hud_frame(&input);
        let want = duel_hud_packets(&hud, &[], &frame_cells);
        let mut bars = 0;
        let got: Vec<DuelHudPacket> = linked
            .borrow()
            .iter()
            .map(|&(ot, p)| {
                assert_eq!(ot, OT + 4 * 5, "case {case}: the HUD's bucket");
                decode(&mut cpu.mem, p, &mut bars)
            })
            .collect();
        assert_eq!(got.len(), want.len(), "case {case}: {input:?}");
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g, w, "case {case} packet {k}: {input:?}");
        }
        packets += got.len();

        let mut want_calls: Vec<Call> = vec![
            (EMIT, 0xEA, 0x1E, 0x14, hud.select_level),
            (EMIT, 0x30, 0x1E, 0x12, 0x80),
        ];
        for &(x, y, digit) in &hud.stage {
            want_calls.push((DIGIT, i32::from(x), i32::from(y), i32::from(digit), 0x80));
        }
        if let Some((x0, score)) = hud.high_score {
            want_calls.push((NUMBER, i32::from(x0), 0x36, score, 0xA0));
        }
        assert_eq!(*calls.borrow(), want_calls, "case {case}: {input:?}");
        for a in LATCHES {
            assert_eq!(
                cpu.mem.rd32(a) as i32,
                hud.max_streak,
                "case {case}: latch {a:#x}"
            );
        }
    }
    assert!(packets > 30_000, "only {packets} packets");
}
