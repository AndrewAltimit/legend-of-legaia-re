//! The party target panel `FUN_801D0520` (PROT 0899, window 14) against
//! `engine-ui`'s `target_panel_draws_for` and `target_panel_sprites_for`, on
//! `menu_painters`' rig: every preview mode, every cursor word, parties of
//! one to three in any order, with the aggregator `FUN_801CF650` and the two
//! ink leaves live beneath the painter.

use crate::common::{Rng, scus};
use crate::menu_painters::{Ev, hands, pictos, rect, rig, same_text};
use legaia_engine_menus::equip_session::engine_equip_from_record;
use legaia_engine_menus::equipment::equip_modifier_table_from_disc;
use legaia_engine_menus::pause_screens::{menu_stat_block, stat_record_from_character};
use legaia_engine_ui::{
    SaveMenuAtlasRects, TargetPanelCursor, TargetPanelMember, TargetPanelMode, TargetPanelView,
    target_panel_draws_for, target_panel_sprites_for,
};
use legaia_game_tables::equip_stats::EquipStatTable;
use legaia_save::CharacterRecord;

const TARGET_PANEL: u32 = 0x801D_0520;
const PARTY_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const CURSOR: u32 = 0x801E_46C4;
const PREVIEW: u32 = 0x801E_46CC;

fn put16(raw: &mut [u8], off: usize, v: u16) {
    raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn the_target_panel_matches() {
    differential!(prot(899, TARGET_PANEL));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus().unwrap();
    let table =
        equip_modifier_table_from_disc(&EquipStatTable::from_scus(&scus).expect("equipment table"));
    let tag = |n: u32| (n, 0, 1, 1);
    let atlas = SaveMenuAtlasRects {
        cursor: tag(1),
        label_lv: tag(0x0A),
        label_hp: tag(0x3F),
        label_mp: tag(0x40),
        label_max_hp: tag(0x64),
        label_max_mp: tag(0x65),
        ..Default::default()
    };
    let mut rng = Rng(0x5447_5450);
    let mut modes = [0u32; 6];
    let (mut ailing, mut all) = (0, 0);
    for run in 0..5_000 {
        let n = 1 + rng.below(3);
        let mut pool = vec![0u32, 1, 2];
        let ids: Vec<u32> = (0..n)
            .map(|_| pool.remove(rng.below(pool.len() as u32) as usize))
            .collect();
        rig.cpu.mem.wr8(PARTY_COUNT, n as u8);
        let mut raws = Vec::new();
        let mut names = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            rig.cpu.mem.wr8(ROSTER + i as u32, id as u8);
            let mut raw = vec![0u8; STRIDE as usize];
            let hp_max = 1 + rng.below(9999) as u16;
            let mp_max = rng.below(1000) as u16;
            put16(&mut raw, 0x104, hp_max);
            put16(&mut raw, 0x106, rng.below(u32::from(hp_max) + 1) as u16);
            put16(&mut raw, 0x108, mp_max);
            put16(&mut raw, 0x10A, rng.below(u32::from(mp_max) + 1) as u16);
            for off in (0x110..=0x11A).step_by(2) {
                let v = match rng.below(5) {
                    0 => 999,
                    _ => rng.below(1000),
                };
                put16(&mut raw, off, v as u16);
            }
            put16(&mut raw, 0x11C, rng.below(10_000) as u16);
            put16(&mut raw, 0x11E, rng.below(1000) as u16);
            for off in (0x122..=0x12C).step_by(2) {
                put16(&mut raw, off, rng.below(1000) as u16);
            }
            if rng.below(4) == 0 {
                put16(&mut raw, 0x12E, 1 << rng.below(16));
                ailing += 1;
            }
            raw[0x130] = 1 + rng.below(99) as u8;
            for e in 0..8 {
                raw[0x196 + e] = match rng.below(4) {
                    0 => 0,
                    _ => rng.below(256) as u8,
                };
            }
            let name = format!("Hero{id}{}", rng.below(100));
            raw[0x2A7..0x2A7 + name.len()].copy_from_slice(name.as_bytes());
            let va = RECORDS + id * STRIDE;
            for (o, b) in raw.iter().enumerate() {
                rig.cpu.mem.wr8(va + o as u32, *b);
            }
            raws.push(raw);
            names.push(name);
        }
        let word = rng.below(6);
        rig.cpu.mem.wr32(PREVIEW, word);
        modes[word as usize] += 1;
        let pressed = rng.below(2) == 1;
        let (flags, cursor) = match rng.below(6) {
            0 => (0x4000 | rng.below(0x1000), TargetPanelCursor::Hidden),
            1 => {
                all += 1;
                (
                    0x2000 | (u32::from(pressed) << 12) | rng.below(3),
                    TargetPanelCursor::All { pressed },
                )
            }
            _ => {
                let row = rng.below(n);
                (
                    row | (u32::from(pressed) << 12),
                    TargetPanelCursor::Single {
                        row: row as u8,
                        pressed,
                    },
                )
            }
        };
        rig.cpu.mem.wr32(CURSOR, flags);

        let r = rect(&mut rng);
        let log = rig.run(TARGET_PANEL, r);

        let members: Vec<TargetPanelMember<'_>> = ids
            .iter()
            .zip(&raws)
            .zip(&names)
            .map(|((&id, raw), name)| {
                let rec = CharacterRecord::parse(raw).expect("record");
                let hms = rec.hp_mp_sp();
                let base = rec.record_stats();
                let mut sr = stat_record_from_character(&rec);
                sr.equip = engine_equip_from_record(sr.equip, id as u8);
                let block = menu_stat_block(&sr, &table, hms.hp_max, hms.mp_max);
                let w = |i: usize| block[i].clamp(0, i32::from(u16::MAX)) as u16;
                TargetPanelMember {
                    name,
                    level: rec.magic_rank(),
                    hp: hms.hp_cur,
                    mp: hms.mp_cur,
                    hp_max: hms.hp_max,
                    mp_max: hms.mp_max,
                    base_hp_max: base.hp_max,
                    base_mp_max: base.mp_max,
                    stat_eff: [w(3), w(4), w(5), w(6), w(7)],
                    stat_base: [base.atk, base.udf, base.ldf, base.spd, base.int],
                    status: u16::from_le_bytes([raw[0x12E], raw[0x12F]]),
                }
            })
            .collect();
        let view = TargetPanelView {
            members: &members,
            mode: TargetPanelMode::from_preview_word(word),
            cursor,
            label_icons: true,
            text_cursor: false,
        };
        let what = format!("run {run} ids {ids:?} mode {word} cursor {flags:#x} at {r:?}");
        let port = target_panel_draws_for(&rig.font, &view, (r.x, r.y));
        let text: Vec<Ev> = log
            .iter()
            .filter(|e| matches!(e, Ev::Text(..) | Ev::Number(..) | Ev::Separator(..)))
            .cloned()
            .collect();
        same_text(&rig, &text, &port, &what);

        let sprites = target_panel_sprites_for(&atlas, &view, (r.x, r.y), (0, 0), 1);
        let mut port_icons: Vec<(u32, i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 != 1)
            .map(|s| (s.src.0, s.dst.0, s.dst.1))
            .collect();
        let mut want: Vec<(u32, i32, i32)> = pictos(&log)
            .iter()
            .map(|p| (u32::from(p.id), p.x, p.y))
            .collect();
        port_icons.sort();
        want.sort();
        assert_eq!(port_icons, want, "{what}: pictograms");
        let port_hands: Vec<(i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 == 1)
            .map(|s| (s.dst.0, s.dst.1))
            .collect();
        let got = hands(&log);
        assert_eq!(
            port_hands,
            got.iter().map(|h| (h.x, h.y)).collect::<Vec<_>>(),
            "{what}: hands"
        );
        // The variant: the static hand once a confirm is staged, the
        // all-party hand's own pair.
        for h in &got {
            let want = match cursor {
                TargetPanelCursor::Single { pressed, .. } => u8::from(!pressed),
                TargetPanelCursor::All { pressed } => {
                    if pressed {
                        0
                    } else {
                        4
                    }
                }
                TargetPanelCursor::Hidden => unreachable!(),
            };
            assert_eq!((h.sprite, h.variant), (0, want), "{what}");
        }
    }
    assert!(modes.iter().all(|&m| m > 500), "{modes:?}");
    assert!(ailing > 1000 && all > 500, "{ailing} {all}");
}
