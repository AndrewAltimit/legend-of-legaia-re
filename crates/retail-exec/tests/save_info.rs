//! The save screen's bottom info panel `FUN_801E08D8` (PROT 0899) against
//! `engine-ui`'s `slot_info_panel_draws_for` / `slot_info_panel_text_draws_for`
//! and the caption pair, on `menu_painters`' rig.
//!
//! The panel reads the focused block's save summary (`0x801EF1B8 + slot *
//! 0x100`), its directory filename (`0x801F2A88 + slot * 0x40`) and the slide
//! timer. The summary is generated, the port reads the same bytes through
//! `apply_retail_summary` the way a mounted card hands them over, and every
//! leaf draw retail makes - the string writer, both numeral writers, the
//! separator, the pictograms, the save UI's record quads, the centred caption
//! and the box - is held to the port's two draw lists.
//!
//! The routine never sets the ink word before its first numerals: the play
//! time and the first column's level draw in whatever `_DAT_8007B454` holds
//! on entry. It leaves `7` there itself once a column has drawn, which is
//! what the test seats.
//!
//! Not generated: a location holding a byte in `0x01..0x1F` (retail's length
//! scan `FUN_8003CA38` stops at one, the port's at the NUL), and a summary
//! naming no member (retail prints no column; the port prints the leader for
//! saves an older engine build composed without a summary).

use crate::common::Rng;
use crate::menu_painters::{Ev, keys, rig};
use legaia_engine_menus::save_select::{
    INFO_PANEL_OFFSCREEN_Y, INFO_PANEL_PARKED_Y, SaveSelectMode, SlotInfoMode, SlotSnapshot,
    apply_retail_summary, interpolate_anim,
};
use legaia_engine_ui::menu::decimal_field::decimal_field_cells;
use legaia_engine_ui::{
    BattleChromeRects, SaveMenuAtlasRects, SlotInfoMember, SlotInfoView, block_badge_draws_for,
    menu_ink_color, numeral_cell_tint, slot_info_caption_draws_for, slot_info_caption_sprites_for,
    slot_info_panel_draws_for, slot_info_panel_text_draws_for,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const INFO: u32 = 0x801E_08D8;
const RECORD_QUAD: u32 = 0x801E_3FF0;
const CAPTION: u32 = 0x801E_3EE0;
const BOX: u32 = 0x801E_36C4;
/// BIOS `A(0x18)`, `strncmp`.
const STRNCMP: u32 = 0x8005_6748;

const SLIDE: u32 = 0x801E_F1A0;
const SUMMARIES: u32 = 0x801E_F1B8;
const SCANNED: u32 = 0x801F_2A68;
const NAMES: u32 = 0x801F_2A88;
const STATE: u32 = 0x801F_0204;
const BUSY: u32 = 0x801E_F160;
const OP_FLAG: u32 = 0x801F_0200;
const JP_PREFIX: u32 = 0x801E_F054;
const RECORDS: u32 = 0x801E_5048;
const INK: u32 = 0x8007_B454;

/// A hooked save-UI call.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ui {
    /// `(record, rgb, x, y, record 0's u, record 2's u)`.
    Quad(u32, u32, i32, i32, u16, u16),
    Caption(Vec<u8>, i32, i32),
    Box(i32, i32, i32, i32),
}

type Sprite = ((u32, u32, u32, u32), (i32, i32), [u32; 4]);

fn sprite_keys(d: &[legaia_engine_ui::SpriteDraw]) -> Vec<Sprite> {
    let mut v: Vec<Sprite> = d
        .iter()
        .map(|s| (s.src, (s.dst.0, s.dst.1), s.color.map(f32::to_bits)))
        .collect();
    v.sort();
    v
}

#[test]
fn the_info_panel_draws_what_retail_draws() {
    differential!(prot(899, INFO));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let ui: Rc<RefCell<Vec<Ui>>> = Rc::default();
    let u = ui.clone();
    rig.cpu.hook(RECORD_QUAD, move |r, m| {
        u.borrow_mut().push(Ui::Quad(
            r.r[A0],
            r.r[A1],
            r.r[A2] as i32,
            r.r[A3] as i32,
            m.rd16(RECORDS + 2),
            m.rd16(RECORDS + 0x1A),
        ))
    });
    let u = ui.clone();
    rig.cpu.hook(CAPTION, move |r, m| {
        let s = crate::menu_painters::cstr(m, r.r[A0]);
        u.borrow_mut()
            .push(Ui::Caption(s, r.r[A1] as i32, r.r[A2] as i32))
    });
    let u = ui.clone();
    rig.cpu.hook(BOX, move |r, _| {
        u.borrow_mut().push(Ui::Box(
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3] as i32,
        ))
    });
    rig.cpu.hook(STRNCMP, |r, m| {
        let mut out = 0i32;
        for i in 0..r.r[A2] {
            let (a, b) = (m.rd8(r.r[A0] + i), m.rd8(r.r[A1] + i));
            if a != b {
                out = i32::from(a) - i32::from(b);
                break;
            }
            if a == 0 {
                break;
            }
        }
        r.r[V0] = out as u32;
    });
    let jp = legaia_save::card::LEGAIA_SAVE_FILENAME_PREFIX_JP;
    let us = legaia_save::card::LEGAIA_SAVE_FILENAME_PREFIX;
    assert_eq!(
        crate::menu_painters::cstr(&mut rig.cpu.mem, JP_PREFIX),
        jp.as_bytes(),
        "the literal the Old Filename test compares against"
    );

    // One distinct source per atlas rect, so a sprite names what it is.
    let tag = |n: u32| (n, 0, 1, 1);
    let digits = (0x400, 0, 80, 12);
    let separator = (0x500, 0, 8, 16);
    let mut atlas = SaveMenuAtlasRects {
        panel_tl: (0x700, 0, 4, 4),
        panel_tr: (0x704, 0, 4, 4),
        panel_bl: (0x708, 0, 4, 4),
        panel_br: (0x70C, 0, 4, 4),
        panel_top: (0x710, 0, 24, 4),
        panel_bot: (0x710, 4, 24, 4),
        panel_left: (0x730, 0, 4, 24),
        panel_right: (0x734, 0, 4, 24),
        panel_interior: (0x740, 0, 16, 16),
        label_lv: tag(0x0A),
        label_hp: tag(0x3F),
        label_mp: tag(0x40),
        load_empty_frame: Some((0x600, 0, 32, 32)),
        battle: Some(BattleChromeRects {
            digits: Some(digits),
            separator,
            ..Default::default()
        }),
        ..Default::default()
    };
    let face = |i: u32| (0x100 + i * 16, 0x80, 16, 16);
    for i in 0..16 {
        atlas.save_icons[i] = Some(face(i as u32));
    }
    let white = [1.0f32; 4].map(f32::to_bits);

    let mut rng = Rng(0x494E_464F);
    let (mut previews, mut invalid, mut old, mut tiers, mut captions) = (0, 0, 0, [0u32; 3], 0);
    for run in 0..6_000 {
        let slot = rng.below(15);
        let t = match rng.below(3) {
            0 => 0x1000,
            _ => rng.below(0x1001),
        };
        rig.cpu.mem.wr32(SLIDE, t);
        let scanned = rng.below(8) != 0;
        rig.cpu.mem.wr8(SCANNED + slot, u8::from(scanned));
        let checking = rng.below(10) == 0;
        let (state, busy) = match (checking, rng.below(3)) {
            (false, _) => ([0u32, 3, 11, 14][rng.below(4) as usize], 0),
            (true, 0) => (12, 0),
            (true, 1) => (13, rng.below(2)),
            (true, _) => (rng.below(12), 1),
        };
        rig.cpu.mem.wr32(STATE, state);
        rig.cpu.mem.wr32(BUSY, busy);
        let saving = rng.below(2) == 0;
        rig.cpu.mem.wr32(OP_FLAG, u32::from(!saving));
        let arg = [1u32, 1, 1, 2, 3, 4, 0, 5][rng.below(8) as usize];

        // The summary.
        let mut sum = [0u8; 0x100];
        let loc_len = match rng.below(8) {
            0 => 0,
            1 => 0x20,
            2 => 0x21 + rng.below(3) as usize,
            _ => 1 + rng.below(0x1F) as usize,
        };
        for b in sum.iter_mut().take(loc_len) {
            *b = match rng.below(40) {
                0 => 0xB0 + rng.below(0x10) as u8,
                1 => 0x1F,
                _ => 0x20 + rng.below(0x5F) as u8,
            };
        }
        let frames = match rng.below(6) {
            0 => rng.below(216_000),
            1 => 99 * 216_000 + rng.below(3 * 216_000),
            _ => rng.below(100 * 216_000),
        };
        sum[0x24..0x28].copy_from_slice(&frames.to_le_bytes());
        let count = 1 + rng.below(3) as usize;
        sum[0x28] = count as u8;
        sum[0x29] = 1;
        for k in 0..count {
            sum[0x2C + k] = rng.below(3) as u8;
            // At times past what a field holds, to walk the overflow rule.
            let wide = rng.below(12) == 0;
            sum[0x30 + k] = rng.below(if wide { 256 } else { 100 }) as u8;
            let hp_max = 1 + rng.below(if wide { 32_767 } else { 9999 }) as u16;
            let mp_max = rng.below(if wide { 32_768 } else { 1000 }) as u16;
            let cur = |rng: &mut Rng, max: u16| match rng.below(4) {
                0 => max,
                1 => rng.below(u32::from(max) / 4 + 1) as u16,
                _ => rng.below(u32::from(max) + 1) as u16,
            };
            let (hp, mp) = (cur(&mut rng, hp_max), cur(&mut rng, mp_max));
            sum[0x3C + 2 * k..][..2].copy_from_slice(&hp_max.to_le_bytes());
            sum[0x4C + 2 * k..][..2].copy_from_slice(&hp.to_le_bytes());
            sum[0x34 + 2 * k..][..2].copy_from_slice(&mp_max.to_le_bytes());
            sum[0x44 + 2 * k..][..2].copy_from_slice(&mp.to_le_bytes());
            let name = format!("Hero{k}{}", rng.below(1000));
            sum[0x54 + 12 * k..][..name.len()].copy_from_slice(name.as_bytes());
        }
        for (i, b) in sum.iter().enumerate() {
            rig.cpu.mem.wr8(SUMMARIES + slot * 0x100 + i as u32, *b);
        }
        let japanese = rng.below(4) == 0;
        let file = format!("{}{:02}", if japanese { jp } else { us }, slot + 1);
        for i in 0..0x40u32 {
            let b = file.as_bytes().get(i as usize).copied().unwrap_or(0);
            rig.cpu.mem.wr8(NAMES + slot * 0x40 + i, b);
        }

        rig.cpu.mem.wr32(INK, 7);
        rig.log.borrow_mut().clear();
        ui.borrow_mut().clear();
        assert_eq!(rig.cpu.call(INFO, &[slot, arg]), Stop::Returned);
        let log = rig.log.borrow().clone();
        let calls = ui.borrow().clone();
        let what = format!(
            "run {run} slot {slot} arg {arg} t {t:#x} scanned {scanned} state {state} busy {busy}"
        );

        // The slide, and the box every arm ends on.
        let (_, y) = interpolate_anim(
            (0, INFO_PANEL_OFFSCREEN_Y),
            (0, INFO_PANEL_PARKED_Y),
            t as u16,
        );
        let off = y - INFO_PANEL_PARKED_Y;
        assert_eq!(calls.last(), Some(&Ui::Box(0xA0, y, 0x11C, 0x40)), "{what}");
        let chrome = slot_info_panel_draws_for(&atlas, None, off, (0, 0), 1);
        let (x0, y0) = chrome.iter().fold((i32::MAX, i32::MAX), |a, s| {
            (a.0.min(s.dst.0), a.1.min(s.dst.1))
        });
        let (x1, y1) = chrome.iter().fold((i32::MIN, i32::MIN), |a, s| {
            (
                a.0.max(s.dst.0 + s.dst.2 as i32),
                a.1.max(s.dst.1 + s.dst.3 as i32),
            )
        });
        // `FUN_801E36C4`'s box around a centre rect: 8 px of frame a side.
        assert_eq!(
            (x0, y0, x1 - x0, y1 - y0),
            (0xA0 - 0x11C / 2 - 10, y - 2, 0x11C + 16, 0x40 + 16),
            "{what}: the chrome's footprint"
        );

        // The mode the routine settles on.
        let mode = if state == 12 || state == 13 || busy != 0 {
            0x64
        } else if !scanned {
            2
        } else {
            arg
        };
        let body: Vec<Ui> = calls[..calls.len() - 1].to_vec();
        if mode != 1 {
            assert!(log.is_empty(), "{what}: {log:?}");
            let select = if saving {
                SaveSelectMode::Save
            } else {
                SaveSelectMode::Load
            };
            let info_mode = match mode {
                2 => Some(SlotInfoMode::NotLegaiaSave),
                3 => Some(SlotInfoMode::FreeBlock),
                4 => Some(SlotInfoMode::Return),
                _ => None,
            };
            let Some(info_mode) = info_mode else {
                assert!(body.is_empty(), "{what}: a blank panel, {body:?}");
                continue;
            };
            captions += 1;
            let caption = info_mode.caption(select).expect("a caption");
            let said: Vec<&Ui> = body
                .iter()
                .filter(|c| matches!(c, Ui::Caption(..)))
                .collect();
            assert_eq!(
                said,
                vec![&Ui::Caption(caption.as_bytes().to_vec(), 0xA0, y + 0x18)],
                "{what}"
            );
            // The caption wrapper draws from `x - width / 2` at `y + 7`
            // (`save_grid`'s differential): the port's line sits there.
            let text = slot_info_caption_draws_for(&rig.font, caption, off, (0, 0), 1);
            let width = rig.font.layout_ascii(caption).advance_x as i32;
            assert_eq!(
                (text[0].dst.0, text[0].dst.1),
                (
                    0xA0 - width / 2 + rig.font.layout_ascii(caption).glyphs[0].dst_x,
                    y + 0x18 + 7 + rig.font.layout_ascii(caption).glyphs[0].dst_y
                ),
                "{what}"
            );
            let quads: Vec<&Ui> = body.iter().filter(|c| matches!(c, Ui::Quad(..))).collect();
            let icon = info_mode == SlotInfoMode::FreeBlock && saving;
            if !icon {
                assert!(quads.is_empty(), "{what}: {quads:?}");
                continue;
            }
            let s16 = (slot * 16) as u16;
            assert_eq!(
                quads,
                vec![
                    &Ui::Quad(0, 0x80_8080, 0x58, y + 0x1C, s16, quads_u2(&body, 0)),
                    &Ui::Quad(1, 0x80_8080, 0x50, y + 0x14, s16, quads_u2(&body, 1)),
                    &Ui::Quad(3, 0x80_8080, 8, y - 8, s16, quads_u2(&body, 2)),
                    &Ui::Quad(2, 0x80_8080, 0x1E, y - 8, s16, s16),
                ],
                "{what}"
            );
            let mut want = vec![
                (face(slot), (0x58, y + 0x1C), white),
                (atlas.load_empty_frame.unwrap(), (0x50, y + 0x14), white),
            ];
            want.extend(sprite_keys(&block_badge_draws_for(
                slot as u8,
                (8, y - 8),
                (0, 0),
                1,
            )));
            want.sort();
            assert_eq!(
                sprite_keys(&slot_info_caption_sprites_for(
                    &atlas,
                    slot as u8,
                    off,
                    (0, 0),
                    1
                )),
                want,
                "{what}"
            );
            continue;
        }

        // Mode 1: the port's reading of the same summary.
        previews += 1;
        let mut block = vec![0u8; 0x400];
        block[0x200..0x300].copy_from_slice(&sum);
        let mut snap = SlotSnapshot::empty(slot as u8);
        snap.present = true;
        apply_retail_summary(&mut snap, &block, file.as_bytes());
        let members = snap.panel_members();
        let views: Vec<SlotInfoMember<'_>> = members
            .iter()
            .map(|m| SlotInfoMember {
                id: m.id,
                name: &m.name,
                level: m.level,
                hp: m.hp,
                mp: m.mp,
            })
            .collect();
        let view = SlotInfoView {
            slot_no: snap.slot + 1,
            location: &snap.location,
            location_valid: snap.location_valid,
            old_filename: snap.old_filename,
            play_time: snap.play_time_hms(),
            members: &views,
        };
        invalid += u32::from(!snap.location_valid);
        old += u32::from(snap.old_filename);

        // Text: every string retail writes, in order, in the ink it holds.
        let text: Vec<Ev> = log
            .iter()
            .filter(|e| matches!(e, Ev::Text(..)))
            .cloned()
            .collect();
        let port_text =
            slot_info_panel_text_draws_for(&rig.font, Some(&view), off, (0, 0), 1, Some(&atlas));
        // The port's panel ink is the framebuffer reading of the white
        // CLUT entry, the rig's the 5-bit entry itself: one ink, compared on
        // the grid the CLUT has.
        let on_grid = |d: &[legaia_engine_ui::TextDraw]| -> Vec<_> {
            d.iter()
                .map(|t| (t.dst, t.src, t.color.map(|c| (c * 31.0).round() as u32)))
                .collect()
        };
        assert_eq!(
            on_grid(&rig.text_of(&text)),
            on_grid(&port_text),
            "{what}: text\n{text:?}"
        );

        // Sprites: numerals cell by cell, separators, pictograms, quads.
        let mut want: Vec<Sprite> = Vec::new();
        for e in &log {
            match e {
                Ev::Number(v, cells, x, y, ink, zero) => {
                    let tint = numeral_cell_tint(menu_ink_color(*ink)).map(f32::to_bits);
                    tiers[match ink {
                        6 => 1,
                        9 => 2,
                        _ => 0,
                    }] += 1;
                    for (dx, d) in decimal_field_cells(*v, *cells, *zero) {
                        want.push(((digits.0 + u32::from(d) * 8, 0, 8, 12), (x + dx, *y), tint));
                    }
                }
                Ev::Separator(g, x, y, ink) => {
                    assert_eq!((*g, *ink), (6, 7), "{what}");
                    want.push((
                        separator,
                        (
                            x + legaia_engine_ui::field_party_hud::SEPARATOR_SPRITE_DX,
                            y + legaia_engine_ui::field_party_hud::SEPARATOR_SPRITE_DY,
                        ),
                        numeral_cell_tint(menu_ink_color(7)).map(f32::to_bits),
                    ));
                }
                Ev::Picto(p) => want.push((tag(u32::from(p.id)), (p.x, p.y), white)),
                Ev::Text(..) => {}
                other => panic!("{what}: {other:?}"),
            }
        }
        let mut badge = 0;
        for c in &body {
            match *c {
                Ui::Quad(0, 0x80_8080, x, y, u0, _) => {
                    assert_eq!(u0 % 16, 0, "{what}");
                    want.push((face(u32::from(u0) / 16), (x, y), white));
                }
                Ui::Quad(3, 0x80_8080, x, qy, _, _) => {
                    assert_eq!((x, qy), (8, y - 8), "{what}");
                    badge += 1;
                }
                Ui::Quad(2, 0x80_8080, x, qy, _, u2) => {
                    assert_eq!((x, qy, u2), (0x1E, y - 8, (slot * 16) as u16), "{what}");
                    badge += 1;
                }
                ref other => panic!("{what}: {other:?}"),
            }
        }
        assert_eq!(badge, 2, "{what}: the block badge");
        want.extend(sprite_keys(&block_badge_draws_for(
            slot as u8,
            (8, y - 8),
            (0, 0),
            1,
        )));
        want.sort();
        let port = slot_info_panel_draws_for(&atlas, Some(&view), off, (0, 0), 1);
        assert_eq!(
            keys(&port[..chrome.len()]),
            keys(&chrome),
            "{what}: chrome first"
        );
        assert_eq!(sprite_keys(&port[chrome.len()..]), want, "{what}: sprites");
    }
    assert!(
        previews > 1500 && invalid > 100 && old > 200 && captions > 1000,
        "{previews} {invalid} {old} {captions}"
    );
    assert!(tiers.iter().all(|&n| n > 500), "{tiers:?}");
}

/// Record 2's `u` as the `n`th quad call saw it.
fn quads_u2(body: &[Ui], n: usize) -> u16 {
    body.iter()
        .filter_map(|c| match c {
            Ui::Quad(.., u2) => Some(*u2),
            _ => None,
        })
        .nth(n)
        .expect("a quad")
}
