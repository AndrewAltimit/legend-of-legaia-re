//! The shop's root command window `FUN_801D4868` (PROT 0899, window `0x2A`)
//! against `engine-menus`' `shop_root_command_rows`, on `menu_painters`' rig:
//! the three row pens, the ink each string is written under, and the hand's
//! mode and seat.

use crate::common::Rng;
use crate::menu_painters::{Ev, cstr, flags, hands, rect, rig};
use legaia_engine_menus::shop::shop_root_command_rows;

const ROOT: u32 = 0x801D_4868;
/// The root picker's cursor word.
const CURSOR_WORD: u32 = 0x801E_46BC;
/// The bag's active window `[start, end)` and its `[id, count]` pairs.
const WINDOW_START: u32 = 0x8007_B5EA;
const WINDOW_END: u32 = 0x8007_B5EC;
const BAG: u32 = 0x8008_5958;

#[test]
fn the_shop_root_window_matches() {
    differential!(prot(899, ROOT));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let labels = legaia_engine_menus::shop::ShopLabels::from_menu_overlay(
        &crate::common::disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    assert!(labels.picker.iter().all(|l| l.0.len() > 2));
    let chrome = legaia_engine_ui::SaveMenuAtlasRects {
        cursor: (152, 64, 16, 16),
        ..Default::default()
    };
    let mut composed = 0;
    let mut rng = Rng(0x5348_4F50);
    let (mut greyed, mut pointed, mut parked) = (0, 0, 0);
    for run in 0..6_000 {
        let word = flags(&mut rng, 3);
        rig.cpu.mem.wr32(CURSOR_WORD, word);
        // A bag window, at times empty, at times holding only half-records.
        let start = rng.below(200);
        let len = rng.below(40);
        rig.cpu.mem.wr16(WINDOW_START, start as u16);
        rig.cpu.mem.wr16(WINDOW_END, (start + len) as u16);
        for slot in 0..256u32 {
            let (id, count) = match rng.below(12) {
                0 => (1 + rng.below(200) as u8, 1 + rng.below(99) as u8),
                1 => (1 + rng.below(200) as u8, 0),
                2 => (0, 1 + rng.below(99) as u8),
                _ => (0, 0),
            };
            // Past the window the bag may hold anything.
            let inside = slot >= start && slot < start + len;
            let (id, count) = if !inside && rng.below(2) == 0 {
                (7, 7)
            } else {
                (id, count)
            };
            rig.cpu.mem.wr8(BAG + slot * 2, id);
            rig.cpu.mem.wr8(BAG + slot * 2 + 1, count);
        }
        let sellable = (start..start + len)
            .any(|s| rig.cpu.mem.rd8(BAG + s * 2) != 0 && rig.cpu.mem.rd8(BAG + s * 2 + 1) != 0);
        let r = rect(&mut rng);
        let log = rig.run(ROOT, r);
        let what = format!("run {run}: word {word:#x} window {start}+{len} at {r:?}");

        let rows = shop_root_command_rows((r.x as i16, r.y as i16), word, sellable);
        let texts: Vec<(Vec<u8>, i32, i32, u8)> = log
            .iter()
            .filter_map(|e| match e {
                Ev::Text(s, x, y, ink) => Some((s.clone(), *x, *y, *ink)),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 3, "{what}");
        for (row, t) in rows.iter().zip(&texts) {
            assert_eq!(
                (i32::from(row.text.0), i32::from(row.text.1), row.ink),
                (t.1, t.2, t.3),
                "{what}: row {}",
                row.row
            );
        }
        let want_hands: Vec<(u8, u8, i32, i32)> = rows
            .iter()
            .filter_map(|row| row.cursor)
            .map(|(mode, (x, y))| (0, mode, i32::from(x), i32::from(y)))
            .collect();
        let got: Vec<(u8, u8, i32, i32)> = hands(&log)
            .iter()
            .map(|h| (h.sprite, h.variant, h.x, h.y))
            .collect();
        assert_eq!(got, want_hands, "{what}: hands");
        // The picker as the screen composes it, while it has the pad: the
        // host's rows through `shop_picker_draws`, at the window's own rect.
        if word & 0x6000 == 0 && (word & 0xFFF) < 3 {
            let rects = legaia_engine_ui::pause_menu::MenuRects::new(None);
            let (wx, wy, w, h) = rects.rect(legaia_engine_ui::shop_screen::WIN_SHOP_PICKER);
            let at = legaia_engine_ui::menu::window_painters::PainterRect::new(wx, wy, w, h);
            let log = rig.run(ROOT, at);
            let ctx = legaia_engine_ui::shop_screen::ShopScreenCtx {
                font: &rig.font,
                rects,
                chrome: Some(&chrome),
                origin: (0, 0),
                scale: 1,
            };
            let picker_rows =
                legaia_engine_menus::shop_cues::shop_root_labels(&labels, false, sellable);
            let picker = legaia_engine_ui::shop_screen::ShopPickerView {
                rows: &picker_rows,
                cursor: Some((word & 0xFFF) as usize),
            };
            let port = legaia_engine_ui::shop_screen::shop_picker_draws(&ctx, &picker);
            let text: Vec<Ev> = log
                .iter()
                .filter(|e| matches!(e, Ev::Text(..)))
                .cloned()
                .collect();
            let on_grid = |d: &[legaia_engine_ui::TextDraw]| -> Vec<_> {
                d.iter()
                    .map(|t| (t.dst, t.src, t.color.map(|c| (c * 31.0).round() as u32)))
                    .collect()
            };
            assert_eq!(
                on_grid(&rig.text_of(&text)),
                on_grid(&port.texts),
                "{what}: the composed picker's text"
            );
            let hand = hands(&log)[0];
            let seats: Vec<(i32, i32)> = port.sprites.iter().map(|s| (s.dst.0, s.dst.1)).collect();
            assert_eq!(seats, vec![(hand.x, hand.y)], "{what}: the composed hand");
            composed += 1;
        }
        greyed += u32::from(!sellable);
        pointed += u32::from(got.len() == 1);
        parked += u32::from(got.len() == 3);
        // The strings are the ones the port reads its picker rows from.
        let names: Vec<&[u8]> = texts.iter().map(|t| t.0.as_slice()).collect();
        let want: Vec<&[u8]> = labels.picker.iter().map(|l| l.0.as_slice()).collect();
        assert_eq!(names, want, "{what}: labels");
    }
    assert!(
        greyed > 500 && pointed > 1000 && parked > 500 && composed > 500,
        "{greyed} {pointed} {parked} {composed}"
    );
}

const SELL_DETAIL: u32 = 0x801D_5AE8;
/// The staged item id word.
const STAGED: u32 = 0x801E_46B0;
/// The accessory-passive name / description table (12-byte stride).
const PASSIVES: u32 = 0x8007_625C;
const ITEMS: u32 = 0x8007_4368;

/// The item detail / sell panel `FUN_801D5AE8` (PROT 0899, window 39)
/// against `engine-menus`' `shop_sell_detail_panel` with the passive index
/// `item_passive_index` resolves - fed from the tables the host installs
/// (`ShopItemData`, `ItemEffectTable`, `DiscEquipInfo`). Every leaf draw
/// the routine makes is held to the panel's pen, ink and content.
#[test]
fn the_sell_detail_panel_matches() {
    differential!(prot(899, SELL_DETAIL));
    use legaia_engine_menus::shop::{PASSIVE_NONE, item_passive_index, shop_sell_detail_panel};
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = crate::common::scus().unwrap();
    let prices = legaia_engine_menus::shop_catalog::ShopItemData::from_scus(&scus).expect("prices");
    let effects =
        legaia_game_tables::item_effect::ItemEffectTable::from_scus(&scus).expect("item effects");
    let equip = legaia_engine_menus::equipment::DiscEquipInfo::from_disc(
        &legaia_game_tables::equip_stats::EquipStatTable::from_scus(&scus).expect("equipment"),
    );
    let mut rng = Rng(0x5345_4C4C);
    let (mut sold, mut unsellable, mut passives, mut empty) = (0, 0, 0, 0);
    for run in 0..4_000 {
        let staged: i32 = match rng.below(8) {
            0 => 0,
            1 => -(1 + rng.below(5) as i32),
            _ => 1 + rng.below(255) as i32,
        };
        rig.cpu.mem.wr32(STAGED, staged as u32);
        let r = rect(&mut rng);
        let log = rig.run(SELL_DETAIL, r);
        let what = format!("run {run}: staged {staged} at {r:?}");
        let id = staged.clamp(0, 255) as u8;
        let passive = (staged > 0)
            .then(|| {
                item_passive_index(
                    effects.kind(id),
                    effects.subtype(id),
                    |sub| equip.row_passive_index(sub),
                    |sub| {
                        effects
                            .descriptor(sub)
                            .map(|e| e.marker)
                            .unwrap_or(PASSIVE_NONE + 1)
                    },
                )
            })
            .flatten();
        let p = shop_sell_detail_panel((r.x as i16, r.y as i16), staged, prices.price(id), passive);
        let pen = |p: (i16, i16)| (i32::from(p.0), i32::from(p.1));
        let item = ITEMS + u32::from(id) * 12;
        let mut want: Vec<Ev> = Vec::new();
        if staged > 0 {
            let name = rig.cpu.mem.rd32(item + 4);
            want.push(Ev::Text(
                cstr(&mut rig.cpu.mem, name),
                pen(p.name_pen).0,
                pen(p.name_pen).1,
                6,
            ));
            want.push(Ev::Detail(
                rig.cpu.mem.rd32(item + 8),
                u32::from(id),
                pen(p.desc_pen).0,
                pen(p.desc_pen).1,
                7,
            ));
            match p.sell {
                Some(row) => {
                    sold += 1;
                    want.push(Ev::Text(
                        cstr(&mut rig.cpu.mem, 0x801C_EC54),
                        pen(row.label_pen).0,
                        pen(row.label_pen).1,
                        5,
                    ));
                    want.push(Ev::Picto(
                        legaia_engine_ui::menu::window_painters::PainterPictogram {
                            id: 0x62,
                            x: pen(row.icon_pen).0,
                            y: pen(row.icon_pen).1,
                        },
                    ));
                    want.push(Ev::Number(
                        i32::from(row.price),
                        5,
                        pen(row.value_pen).0,
                        pen(row.value_pen).1,
                        7,
                        false,
                    ));
                }
                None => {
                    unsellable += 1;
                    want.push(Ev::Text(
                        cstr(&mut rig.cpu.mem, 0x801C_EC5C),
                        pen(p.cannot_sell_pen).0,
                        pen(p.cannot_sell_pen).1,
                        9,
                    ));
                }
            }
            if let Some(ix) = p.passive {
                passives += 1;
                let rec = PASSIVES + u32::from(ix) * 12;
                want.push(Ev::Label(
                    rig.cpu.mem.rd32(rec + 4),
                    pen(p.passive_name_pen).0,
                    pen(p.passive_name_pen).1,
                    4,
                ));
                want.push(Ev::Label(
                    rig.cpu.mem.rd32(rec + 8),
                    pen(p.passive_desc_pen).0,
                    pen(p.passive_desc_pen).1,
                    7,
                ));
            }
        } else {
            empty += 1;
        }
        let ((bx, by), (bw, bh)) = p.shade_box;
        want.push(Ev::Call(0x8003_4B6C));
        want.push(Ev::Frame(
            i32::from(bx),
            i32::from(by),
            i32::from(bw),
            i32::from(bh),
        ));
        assert_eq!(log, want, "{what}");
    }
    assert!(
        sold > 1000 && unsellable > 100 && passives > 200 && empty > 500,
        "{sold} {unsellable} {passives} {empty}"
    );
}
