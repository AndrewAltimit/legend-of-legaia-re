//! The fishing HUD's sprite path (PROT 0972) against `engine-ui`'s fishing
//! HUD: the shared emitter `FUN_801D63B0` over generated records and
//! arguments, the two digit emitters `FUN_801D7DD8` / `FUN_801D7D44` and the
//! digit field `FUN_801D76E0` over the shipped table, the five banner
//! animators over their whole lives, the two gauge bars `FUN_801D1870` /
//! `FUN_801D1A90`, and the two HUD builders `FUN_801D13F0` / `FUN_801D1580`
//! with everything beneath them live.
//!
//! The retail side runs into a packet buffer and the ordering table; every
//! `POLY_GT4` it wrote is decoded and compared with the port's screen
//! primitives in emission order. The bar fills are the one declared
//! difference: retail textures them from a system page at `tpage 0x1F`, the
//! port fills them flat, so a fill compares its geometry and vertex colours
//! and the retail side's fixed page, CLUT and UVs.

use crate::common::gt4::{self, Gt4};
use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_ui::fishing::sprite::{
    fishing_hud_sprite_prims, fishing_large_digit, fishing_small_digit, fishing_sprite_quad,
};
use legaia_engine_ui::{
    CatchHudState, HudCaption, HudDraw, banner_converge_draws, banner_from_left_draw,
    banner_from_right_draw, banner_miss_draw, catch_hud_draws, number_digit_cells,
    persistent_hud_draws, strike_splash_draws,
};
use legaia_minigame_formats::fishing_species as fs;
use legaia_minigame_formats::fishing_sprites::{
    FISHING_SPRITE_COUNT, FISHING_SPRITE_STRIDE, FISHING_SPRITE_TABLE_VA, FishingSprite,
};
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const EMIT: u32 = 0x801D_63B0;
const SMALL: u32 = 0x801D_7DD8;
const LARGE: u32 = 0x801D_7D44;
const FIELD: u32 = 0x801D_76E0;
const FROM_LEFT: u32 = 0x801D_78EC;
const FROM_RIGHT: u32 = 0x801D_75DC;
const MISS: u32 = 0x801D_6F10;
const CONVERGE: u32 = 0x801D_7528;
const SPLASH: u32 = 0x801D_71D4;
const BAR: u32 = 0x801D_1870;
const POWER_BAR: u32 = 0x801D_1A90;
const PERSISTENT: u32 = 0x801D_13F0;
const CATCH: u32 = 0x801D_1580;

/// The bucket word `_DAT_801D9158`.
const BUCKET: u32 = 0x801D_9158;

fn rig() -> Option<(Cpu, Vec<u8>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    Some((cpu, overlay))
}

fn table_of(overlay: &[u8]) -> Vec<FishingSprite> {
    let off = (FISHING_SPRITE_TABLE_VA - fs::FISHING_OVERLAY_BASE_VA) as usize;
    overlay[off..off + FISHING_SPRITE_COUNT * FISHING_SPRITE_STRIDE]
        .chunks(FISHING_SPRITE_STRIDE)
        .map(|r| FishingSprite::from_bytes(r).expect("record"))
        .collect()
}

/// Empty the packet buffer and the ordering table, bucket `3`.
fn reset(cpu: &mut Cpu) {
    gt4::reset(cpu, BUCKET);
}

/// The packets written since [`reset`] (`gt4::packets`).
fn packets(cpu: &mut Cpu) -> Vec<Gt4> {
    gt4::packets(cpu, BUCKET)
}

fn rgb(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// Compare the retail packets with the port's primitives, one for one. A flat
/// port quad is a bar fill: its geometry and colour against the retail fill,
/// whose page, CLUT and UVs are the system strip's.
fn same(retail: &[Gt4], port: &[ScreenPrim], what: &str) -> u32 {
    assert_eq!(retail.len(), port.len(), "{what}: count\n{retail:?}");
    let mut fills = 0;
    for (i, (r, p)) in retail.iter().zip(port).enumerate() {
        match p {
            ScreenPrim::Textured(q) => {
                assert_eq!(q.ot_index, 3, "{what} #{i}");
                let g = q.gouraud.expect("gouraud");
                let port = Gt4 {
                    code: 0x3C | if q.semi_transparent { 2 } else { 0 },
                    rgb: [rgb(g[0]), rgb(g[1]), rgb(g[2]), rgb(g[3])],
                    xy: q.xy,
                    uv: q.uv,
                    clut: q.clut,
                    tpage: q.tpage,
                };
                assert_eq!(*r, port, "{what} #{i}");
            }
            ScreenPrim::Flat(f) => {
                fills += 1;
                assert_eq!(f.ot_index, 3, "{what} #{i}");
                assert_eq!(r.xy, f.xy, "{what} #{i}: fill geometry");
                for c in r.rgb {
                    assert_eq!(c, [f.color[0], f.color[1], f.color[2]], "{what} #{i}");
                }
                assert_eq!(
                    (r.code, r.clut, r.tpage, r.uv),
                    (
                        0x3C,
                        0x7F84,
                        0x1F,
                        [(0, 0xF0), (2, 0xF0), (0, 0xF2), (2, 0xF2)]
                    ),
                    "{what} #{i}: the fill's system strip"
                );
            }
        }
    }
    fills
}

fn call(cpu: &mut Cpu, entry: u32, args: &[u32], stack: &[u32]) -> u32 {
    for (i, v) in stack.iter().enumerate() {
        cpu.mem.wr32(STACK_TOP + 0x10 + 4 * i as u32, *v);
    }
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(
        cpu.call(entry, args),
        Stop::Returned,
        "{entry:#x} {args:x?}"
    );
    cpu.regs.r[V0]
}

#[test]
fn the_shipped_table_reads_as_the_emitter_reads_it() {
    not_a_differential!("holds the shipped sprite table's semi bytes to 0 / 1; runs no routine");
    let Some((_, overlay)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The emitter shifts the raw byte into the code (`(semi << 1) | 0x3C`);
    // the port reads it as `!= 0`, which agrees on these two values only.
    let off = (FISHING_SPRITE_TABLE_VA - fs::FISHING_OVERLAY_BASE_VA) as usize;
    for i in 0..FISHING_SPRITE_COUNT {
        let semi = overlay[off + i * FISHING_SPRITE_STRIDE + 0xF];
        assert!(semi <= 1, "record {i}: semi {semi}");
    }
}

#[test]
fn the_emitter_writes_the_quad_the_port_builds() {
    differential!(prot(972, EMIT));
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4653_5052);
    let (mut centred, mut moded) = (0, 0);
    for run in 0..20_000 {
        let idx = rng.below(FISHING_SPRITE_COUNT as u32);
        let mut rec = [0u8; FISHING_SPRITE_STRIDE];
        for b in rec.iter_mut() {
            *b = rng.next() as u8;
        }
        let scale: i32 = match rng.below(4) {
            0 => 0x1000,
            1 => rng.below(0x3000) as i32,
            2 => -(rng.below(0x3000) as i32),
            _ => 0x800,
        };
        rec[..4].copy_from_slice(&scale.to_le_bytes());
        rec[0xF] = rng.below(2) as u8;
        rec[0x13] = rng.below(4) as u8;
        let rec_va = FISHING_SPRITE_TABLE_VA + idx * FISHING_SPRITE_STRIDE as u32;
        cpu.mem.load(rec_va, &rec);
        let mode = [0, 0, 1, 2, 3][rng.below(5) as usize];
        let id = (mode << 10) | idx;
        let anchor = [0u32, 1, rng.next()][rng.below(3) as usize];
        let x = rng.below(0x200) as i32 - 0x80;
        let y = rng.below(0x200) as i32 - 0x80;
        let brightness = [0xFF, 0x80, rng.below(0x200) as i32][rng.below(3) as usize];
        let sx = [
            0x1000,
            rng.below(0xC000) as i32,
            -(rng.below(0x4000) as i32),
        ][rng.below(3) as usize];
        let sy = [
            0x1000,
            rng.below(0xC000) as i32,
            -(rng.below(0x4000) as i32),
        ][rng.below(3) as usize];
        reset(&mut cpu);
        call(
            &mut cpu,
            EMIT,
            &[anchor, x as u32, y as u32, id],
            &[brightness as u32, sx as u32, sy as u32],
        );
        let retail = packets(&mut cpu);
        let port = fishing_sprite_quad(
            &FishingSprite::from_bytes(&rec).unwrap(),
            anchor as i32,
            x,
            y,
            id,
            brightness,
            sx,
            sy,
        );
        same(&retail, &[port], &format!("run {run}"));
        centred += u32::from(anchor == 0);
        moded += u32::from(mode != 0);
    }
    assert!(centred > 5000 && moded > 5000, "{centred} {moded}");
}

#[test]
fn the_digits_and_the_digit_field_match() {
    differential!(prot(972, SMALL), prot(972, LARGE), prot(972, FIELD));
    let Some((mut cpu, overlay)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = table_of(&overlay);
    let mut rng = Rng(0x4449_4749);
    let mut blanks = 0;
    for run in 0..6_000 {
        let (x, y) = (rng.below(0x140) as i32, rng.below(0xF0) as i32);
        let brightness = [0x80, 0xFF, rng.below(0x100) as i32][rng.below(3) as usize];
        let what = format!("run {run}");
        // The two emitters, on any digit and scale.
        let digit = rng.below(12) as i32;
        let scale = [0x1000, rng.below(0x3000) as i32][rng.below(2) as usize];
        reset(&mut cpu);
        call(
            &mut cpu,
            SMALL,
            &[x as u32, y as u32, digit as u32, brightness as u32],
            &[scale as u32],
        );
        let port: Vec<_> = fishing_small_digit(&table, x, y, digit, brightness, scale)
            .into_iter()
            .collect();
        same(&packets(&mut cpu), &port, &format!("{what} small"));
        reset(&mut cpu);
        call(
            &mut cpu,
            LARGE,
            &[x as u32, y as u32, digit as u32, brightness as u32],
            &[scale as u32],
        );
        let port = fishing_large_digit(&table, x, y, digit, brightness, scale);
        same(&packets(&mut cpu), &port, &format!("{what} large"));
        // The field, both styles.
        let value = match rng.below(5) {
            0 => rng.below(10) as i32,
            1 => rng.below(100_000) as i32,
            2 => -(rng.below(1000) as i32),
            3 => rng.next() as i32,
            _ => [0, 9, 10, 99_999_999, 100_000_000, 123_456_789][rng.below(6) as usize],
        };
        let style = [0u32, 1, rng.next()][rng.below(3) as usize];
        reset(&mut cpu);
        call(
            &mut cpu,
            FIELD,
            &[style, x as u32, y as u32, value as u32],
            &[brightness as u32, 0],
        );
        let mut port = Vec::new();
        for c in number_digit_cells(style as i32, x, y, value) {
            if style == 0 {
                port.extend(fishing_small_digit(
                    &table, c.x, c.y, c.digit, brightness, 0x1000,
                ));
            } else {
                port.extend(fishing_large_digit(
                    &table, c.x, c.y, c.digit, brightness, 0x1000,
                ));
            }
        }
        same(
            &packets(&mut cpu),
            &port,
            &format!("{what} field {value} {style}"),
        );
        blanks += u32::from(value > 0 && value < 10_000_000);
    }
    assert!(blanks > 1000, "{blanks}");
}

#[test]
fn the_banners_play_their_whole_lives_as_retail() {
    differential!(
        prot(972, FROM_LEFT),
        prot(972, FROM_RIGHT),
        prot(972, MISS),
        prot(972, CONVERGE),
        prot(972, SPLASH)
    );
    let Some((mut cpu, overlay)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = table_of(&overlay);
    type Port = fn(i32) -> Vec<HudDraw>;
    let banners: [(u32, Port); 5] = [
        (FROM_LEFT, |f| {
            banner_from_left_draw(f).into_iter().collect()
        }),
        (FROM_RIGHT, |f| {
            banner_from_right_draw(f).into_iter().collect()
        }),
        (MISS, |f| banner_miss_draw(f).into_iter().collect()),
        (CONVERGE, |f| {
            banner_converge_draws(f).into_iter().flatten().collect()
        }),
        (SPLASH, |f| {
            strike_splash_draws(f).into_iter().flatten().collect()
        }),
    ];
    for (entry, port) in banners {
        let mut live = 0;
        for frame in 0..0xE0 {
            reset(&mut cpu);
            let active = call(&mut cpu, entry, &[frame as u32], &[]);
            let draws = port(frame);
            assert_eq!(active != 0, !draws.is_empty(), "{entry:#x} frame {frame}");
            let what = format!("{entry:#x} frame {frame}");
            same(
                &packets(&mut cpu),
                &fishing_hud_sprite_prims(&draws, &table),
                &what,
            );
            live += u32::from(active != 0);
        }
        assert!(live >= 0x98, "{entry:#x}: {live}");
    }
}

#[test]
fn the_gauge_bars_match() {
    differential!(prot(972, BAR), prot(972, POWER_BAR));
    let Some((mut cpu, overlay)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = table_of(&overlay);
    let mut rng = Rng(0x4241_5253);
    let mut filled = 0;
    for run in 0..10_000 {
        let (x, y) = (rng.below(0x140) as i32, rng.below(0xF0) as i32);
        let value = match rng.below(4) {
            0 => 0,
            1 => 0x1000,
            2 => rng.below(0x1001) as i32,
            _ => rng.below(0x40) as i32,
        };
        let segments = [10, 0xC, 1 + rng.below(16) as i32][rng.below(3) as usize];
        let style = rng.below(2) as i32;
        let what = format!("run {run} value {value} segments {segments} style {style}");
        reset(&mut cpu);
        call(
            &mut cpu,
            BAR,
            &[style as u32, x as u32, y as u32, value as u32],
            &[segments as u32],
        );
        let items = [HudDraw::Bar {
            style,
            x,
            y,
            value,
            step: segments,
        }];
        filled += same_bar(&mut cpu, &items, &table, &what);
        reset(&mut cpu);
        call(
            &mut cpu,
            POWER_BAR,
            &[x as u32, y as u32, value as u32, segments as u32],
            &[],
        );
        let items = [HudDraw::PowerBar {
            x,
            y,
            power: value,
            step: segments,
        }];
        filled += same_bar(&mut cpu, &items, &table, &format!("power {what}"));
    }
    assert!(filled > 10_000, "{filled}");
}

/// A bar against the port's: retail adds its fill quad even when there is no
/// fill, a zero-width quad the GPU draws nothing for; the port leaves it out.
fn same_bar(cpu: &mut Cpu, items: &[HudDraw], table: &[FishingSprite], what: &str) -> u32 {
    let mut retail = packets(cpu);
    let port = fishing_hud_sprite_prims(items, table);
    if port.len() + 1 == retail.len() {
        let fill = retail.remove(0);
        let w = fill.xy[1].0 - fill.xy[0].0;
        let h = fill.xy[0].1 - fill.xy[2].1;
        assert!(w == 0 || h == 0, "{what}: a fill the port dropped {fill:?}");
    }
    same(&retail, &port, what)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Side {
    Caption(u32, i32, i32),
    Count(i32, u32, i32, i32),
}

#[test]
fn the_persistent_hud_matches() {
    differential!(prot(972, PERSISTENT));
    let Some((mut cpu, overlay)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = table_of(&overlay);
    let side: Rc<RefCell<Vec<Side>>> = Rc::default();
    let s = side.clone();
    cpu.hook(0x8003_6888, move |r, m| {
        s.borrow_mut().push(Side::Caption(
            r.r[A0],
            r.r[A3] as i32,
            m.rd32(r.r[psx_retail_exec::SP] + 0x10) as i32,
        ))
    });
    let s = side.clone();
    cpu.hook(0x8003_4B78, move |r, _| {
        s.borrow_mut().push(Side::Count(
            r.r[A0] as i32,
            r.r[A1],
            r.r[A2] as i32,
            r.r[A3] as i32,
        ))
    });
    let lures = Rc::new(RefCell::new((0u32, 0i32)));
    let l = lures.clone();
    cpu.hook(0x8004_2F4C, move |r, _| {
        let mut l = l.borrow_mut();
        l.0 = r.r[A0];
        r.r[V0] = l.1 as u32;
    });
    let mut rng = Rng(0x5045_5253);
    for run in 0..4_000 {
        let best = [0, rng.below(1_000_000) as i32, rng.next() as i32][rng.below(3) as usize];
        let points = [
            0,
            999_999,
            1_000_000,
            rng.below(2_000_000) as i32,
            rng.next() as i32,
        ][rng.below(5) as usize];
        let rod = [0u32, 1, 2, 3, rng.next()][rng.below(5) as usize];
        let count = rng.below(100) as i32;
        lures.borrow_mut().1 = count;
        cpu.mem.wr32(0x8008_4458, best as u32);
        cpu.mem.wr32(0x8008_444C, points as u32);
        cpu.mem.wr32(0x8008_4450, rod);
        side.borrow_mut().clear();
        reset(&mut cpu);
        call(&mut cpu, PERSISTENT, &[], &[]);
        let what = format!("run {run} rod {rod}");
        let draws = persistent_hud_draws(points, best, rod, count);
        same(
            &packets(&mut cpu),
            &fishing_hud_sprite_prims(&draws, &table),
            &what,
        );
        assert_eq!(
            lures.borrow().0,
            rod.wrapping_add(0x9D),
            "{what}: the lure item"
        );
        // The text side: the rod labels are three overlay strings in order.
        let port: Vec<Side> = draws
            .iter()
            .filter_map(|d| match *d {
                HudDraw::Caption { text, x, y } => Some(Side::Caption(
                    match text {
                        HudCaption::RodName(i) => 0x801C_EFA4 + 4 * i,
                        HudCaption::LuresLeft => 0x801C_EFBC,
                        HudCaption::LureCountSuffix => 0x801C_EFC4,
                        other => panic!("{other:?}"),
                    },
                    x,
                    y,
                )),
                HudDraw::Count {
                    value,
                    digits,
                    x,
                    y,
                } => Some(Side::Count(value, digits, x, y)),
                _ => None,
            })
            .collect();
        assert_eq!(*side.borrow(), port, "{what}");
    }
}

#[test]
fn the_catch_hud_matches() {
    differential!(prot(972, CATCH));
    let Some((mut cpu, overlay)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = table_of(&overlay);
    cpu.stub(0x8001_A068, 0);
    let mut rng = Rng(0x4341_5443);
    let (mut hooked, mut fills) = (0, 0);
    for run in 0..6_000 {
        let s = CatchHudState {
            record: [0, 300, rng.below(0x2000) as i32, rng.below(100_000) as i32]
                [rng.below(4) as usize],
            line_extent: [0, rng.below(0x40000) as i32, -(rng.below(0x1000) as i32)]
                [rng.below(3) as usize],
            cast_power: [0, 0x1000, rng.below(0x1001) as i32][rng.below(3) as usize],
            depth: rng.below(0x1001) as i32,
            tension: rng.below(0x1001) as i32,
            gauges_visible: rng.below(2) == 0,
        };
        cpu.mem.wr32(0x801D_927C, s.record as u32);
        cpu.mem.wr32(0x801D_9178, s.line_extent as u32);
        cpu.mem.wr32(0x801D_9274, s.cast_power as u32);
        cpu.mem.wr32(0x801D_9298, s.depth as u32);
        cpu.mem.wr32(0x801D_9168, s.tension as u32);
        cpu.mem.wr32(0x801D_91B4, u32::from(s.gauges_visible));
        cpu.mem.wr32(0x8007_B9B0, rng.below(2));
        reset(&mut cpu);
        call(&mut cpu, CATCH, &[], &[]);
        let what = format!("run {run} {s:?}");
        let mut retail = packets(&mut cpu);
        let port = fishing_hud_sprite_prims(&catch_hud_draws(&s), &table);
        // Drop the zero-area fills the port leaves out, as `same_bar` does.
        let flats = port
            .iter()
            .filter(|p| matches!(p, ScreenPrim::Flat(_)))
            .count();
        let bars = if s.gauges_visible { 3 } else { 1 };
        if flats < bars {
            retail.retain(|g| {
                !(g.tpage == 0x1F
                    && g.clut == 0x7F84
                    && (g.xy[1].0 == g.xy[0].0 || g.xy[0].1 == g.xy[2].1))
            });
        }
        fills += same(&retail, &port, &what);
        assert_eq!(
            cpu.mem.rd32(0x801D_9280) as i32,
            (s.record - 300).max(0),
            "{what}: the length term"
        );
        hooked += u32::from(s.gauges_visible);
    }
    assert!(hooked > 2000 && fills > 6000, "{hooked} {fills}");
}
