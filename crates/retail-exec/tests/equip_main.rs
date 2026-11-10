//! The Equip screen's main window `FUN_801D21C0` (PROT 0899) against
//! `engine-ui`'s `equip_screen_draws_for_with`, its sprite builders and the
//! slot browse's item panel, on `menu_painters`' rig.
//!
//! The first pass (Best Equipment line, seven slot rows) runs for any
//! screen; the second runs only on the settled slot browse, and is the Best
//! Equipment change list with its compare block on row 0, or the worn item's
//! panel on a slot row.

use crate::common::Rng;
use crate::menu_painters::{Ev, cstr, hands, keys, pictos, rect, rig};
use legaia_engine_menus::equip_session::{BROWSE_SLOT_ORDER, engine_equip_from_record};
use legaia_engine_ui::menu::window_painters::ChoiceFlags;
use legaia_engine_ui::{
    EquipDrawPhase, EquipScreenView, EquipSlotRow, EquipStatRow, SaveMenuAtlasRects, TextDraw,
    equip_best_change_sprites_for, equip_browse_info_draws, equip_screen_draws_for_with,
    equip_screen_sprites_for,
};
use legaia_game_tables::menu_pool::{MenuLabel, MenuPoolLabels};
use psx_retail_exec::{A0, A1, A2};
use std::cell::RefCell;
use std::rc::Rc;

const EQUIP_MAIN: u32 = 0x801D_21C0;
const ARROW: u32 = 0x8003_C310;
const ITEM_PANEL: u32 = 0x801D_0F1C;
const PANEL_MODE: u32 = 0x8003_4B6C;

const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const LIST_CURSOR: u32 = 0x801E_46C4;
const ROW_CURSOR: u32 = 0x801E_46C0;
const SCREEN: u32 = 0x801E_46A4;
const SCREEN_NEXT: u32 = 0x801E_46A8;
const STAGED_ID: u32 = 0x801E_46B0;
const BUSY: u32 = 0x8007_BB80;
const BEST_PICKS: u32 = 0x801E_F0C0;
const BLOCK: u32 = 0x801E_F080;
const TRIAL_BLOCK: u32 = 0x801E_F0A0;
const ITEMS: u32 = 0x8007_4368;
const EQUIPMENT: u32 = 0x8007_4F68;
const ROW_ICONS: u32 = 0x801E_43F4;
const WINDOW: u32 = 0x8012_0000;

/// A hook's log.
type Calls<T> = Rc<RefCell<Vec<T>>>;

#[test]
fn the_equip_main_window_matches() {
    differential!(prot(899, EQUIP_MAIN));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pool = MenuPoolLabels::from_menu_overlay(
        &crate::common::disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    let cell = |id| legaia_font::SpriteCellSrc {
        id,
        w: 12,
        h: 12,
        rgba: vec![0xFF; 12 * 12 * 4],
    };
    rig.font = rig
        .font
        .clone()
        .with_sprite_cells(&[cell(legaia_font::sprite_cell_ids::ink_arrow(0, 2))]);
    let arrow_cell = rig
        .font
        .sprite_cell(legaia_font::sprite_cell_ids::ink_arrow(0, 2))
        .expect("the right arrow");
    // Names the string writer would expand get an ASCII stand-in.
    let stand_in = 0x8013_0100;
    rig.poke_str(stand_in, "Stand-in");
    let mut item_names = Vec::new();
    for id in 0..256u32 {
        let p = rig.cpu.mem.rd32(ITEMS + id * 12 + 4);
        let ascii = (0x8001_0000..0x8008_0000).contains(&p)
            && cstr(&mut rig.cpu.mem, p)
                .iter()
                .all(|b| (0x20..0x80).contains(b));
        if !ascii {
            rig.cpu.mem.wr32(ITEMS + id * 12 + 4, stand_in);
        }
        let p = rig.cpu.mem.rd32(ITEMS + id * 12 + 4);
        item_names.push(rig.string(p));
    }
    // Equipment ids by the armament class their record's `+7` names.
    let mut by_class: [Vec<u8>; 4] = Default::default();
    for id in 1..256u32 {
        if rig.cpu.mem.rd8(ITEMS + id * 12) == 1 {
            let rec = EQUIPMENT + u32::from(rig.cpu.mem.rd8(ITEMS + id * 12 + 1)) * 8;
            let class = (rig.cpu.mem.rd8(rec + 7) & 0x60) >> 5;
            by_class[class as usize].push(id as u8);
        }
    }
    // Armament row -> the class its picks carry (weapon 2, helmet 1, body 0,
    // footwear 3).
    const ROW_CLASS: [usize; 4] = [2, 1, 0, 3];
    assert!(by_class.iter().all(|v| v.len() > 3), "{by_class:?}");

    let arrows: Calls<(u32, i32, i32, u8)> = Rc::default();
    let a = arrows.clone();
    rig.cpu.hook(ARROW, move |r, m| {
        let ink = m.rd32(0x8007_B454) as u8;
        a.borrow_mut()
            .push((r.r[A0], r.r[A1] as i32, r.r[A2] as i32, ink));
    });
    // The panel and the skin call, with the window they ran on.
    let panels: Calls<(u32, i32, i32, u32)> = Rc::default();
    rig.cpu.unhook(ITEM_PANEL);
    rig.cpu.unhook(PANEL_MODE);
    for entry in [ITEM_PANEL, PANEL_MODE] {
        let p = panels.clone();
        rig.cpu.hook(entry, move |r, m| {
            p.borrow_mut().push((
                entry,
                m.rd16(WINDOW + 0xA) as i16 as i32,
                m.rd16(WINDOW + 0xC) as i16 as i32,
                if entry == PANEL_MODE {
                    r.r[A0]
                } else {
                    m.rd32(STAGED_ID)
                },
            ));
        });
    }

    let tag = |n: u32| (n, 0, 1, 1);
    let atlas = SaveMenuAtlasRects {
        cursor: tag(1),
        icon_weapon: tag(0x100),
        icon_helmet: tag(0x101),
        icon_armor: tag(0x102),
        icon_boot: tag(0x103),
        icon_goods: tag(0x104),
        ..Default::default()
    };
    let row_icons: Vec<u32> = (0..7)
        .map(|i| u32::from(rig.cpu.mem.rd16(ROW_ICONS + i * 2)))
        .collect();
    let icon_id = |src: u32| match src {
        n @ 0x100..=0x103 => row_icons[(n - 0x100) as usize],
        _ => row_icons[4],
    };
    let labels: Vec<&str> = [MenuLabel::Atk, MenuLabel::Udf, MenuLabel::Ldf]
        .into_iter()
        .map(|l| pool.get(l).expect("stat cell"))
        .collect();

    let mut rng = Rng(0x4551_5549);
    let (mut bests, mut infos, mut changed, mut settled) = (0, 0, 0, 0);
    for run in 0..6_000 {
        let id = rng.below(3);
        let seat = rng.below(3);
        // A seat past the three heroes draws nothing at all.
        let stranger = rng.below(40) == 0;
        rig.cpu
            .mem
            .wr8(ROSTER + seat, if stranger { 3 } else { id as u8 });
        rig.cpu.mem.wr32(LIST_CURSOR, seat | (rng.below(2) << 12));
        let row = rng.below(8);
        let flags = match rng.below(6) {
            0 => 0x4000 | row,
            1 => 0x2000 | (rng.below(2) << 12) | row,
            _ => row | (rng.below(2) << 12),
        };
        rig.cpu.mem.wr32(ROW_CURSOR, flags);
        let rec = RECORDS + id * STRIDE;
        let mut equip = [0u8; 8];
        for (e, b) in equip.iter_mut().enumerate() {
            *b = match rng.below(4) {
                0 => 0,
                _ => rng.below(256) as u8,
            };
            rig.cpu.mem.wr8(rec + 0x196 + e as u32, *b);
        }
        let engine = engine_equip_from_record(equip, id as u8);
        let worn: Vec<u8> = BROWSE_SLOT_ORDER
            .iter()
            .map(|&s| engine[usize::from(s)])
            .collect();
        // The second pass's gates.
        let screen = if rng.below(4) == 0 { 0x12 } else { 0x13 };
        let next = if rng.below(6) == 0 { 0x14 } else { 0x13 };
        let busy = u32::from(rng.below(6) == 0);
        rig.cpu.mem.wr32(SCREEN, screen);
        rig.cpu.mem.wr32(SCREEN_NEXT, next);
        rig.cpu.mem.wr32(BUSY, busy);
        let second = screen == 0x13 && next == 0x13 && busy == 0 && !stranger;
        settled += u32::from(second);
        // The Best Equipment picks: the worn piece, or another of its class.
        let mut picks = [0u8; 4];
        for (i, p) in picks.iter_mut().enumerate() {
            let class = &by_class[ROW_CLASS[i]];
            *p = match rng.below(3) {
                0 => worn[i],
                _ => class[rng.below(class.len() as u32) as usize],
            };
            rig.cpu.mem.wr8(BEST_PICKS + i as u32, *p);
        }
        let mut cur = [0u32; 3];
        let mut new = [0u32; 3];
        for i in 0..3 {
            cur[i] = rng.below(1200);
            new[i] = match rng.below(3) {
                0 => cur[i],
                _ => rng.below(1200),
            };
            rig.cpu.mem.wr32(BLOCK + 0xC + i as u32 * 4, cur[i]);
            rig.cpu.mem.wr32(TRIAL_BLOCK + 0xC + i as u32 * 4, new[i]);
        }
        rig.cpu.mem.wr32(STAGED_ID, 0xEE);

        let r = rect(&mut rng);
        arrows.borrow_mut().clear();
        panels.borrow_mut().clear();
        let log = rig.run(EQUIP_MAIN, r);
        let what = format!(
            "run {run} id {id} flags {flags:#x} screen {screen:#x}/{next:#x} busy {busy} at {r:?}"
        );
        if stranger {
            assert!(log.is_empty() && panels.borrow().is_empty(), "{what}");
            continue;
        }

        let on_best = second && flags & 0xFFF == 0;
        let best_changes: Vec<(u8, &str)> = if on_best {
            (0..4u8)
                .filter(|&i| picks[usize::from(i)] != worn[usize::from(i)])
                .map(|i| (i, item_names[usize::from(picks[usize::from(i)])].as_str()))
                .collect()
        } else {
            Vec::new()
        };
        let stat_compare: Vec<EquipStatRow<'_>> = if on_best {
            (0..3)
                .map(|i| EquipStatRow {
                    label: labels[i],
                    current: cur[i] as u16,
                    preview: new[i] as u16,
                })
                .collect()
        } else {
            Vec::new()
        };
        let slots: Vec<EquipSlotRow<'_>> = worn
            .iter()
            .map(|&w| EquipSlotRow {
                label: "",
                current_name: &item_names[usize::from(w)],
            })
            .collect();
        let view = EquipScreenView {
            party_names: &[],
            party_cursor: 0,
            slots: &slots,
            candidates: &[],
            stat_compare: &stat_compare,
            best_changes: &best_changes,
            phase: EquipDrawPhase::SlotPicker,
            cursor: (flags & 0xFFF) as u16,
            active_slot: 0,
            text_cursor: false,
        };
        let mut port: Vec<TextDraw> = equip_screen_draws_for_with(
            &rig.font,
            Some(&pool),
            &view,
            (0x4000, 0x4000),
            (0x4000, 0x4000),
            (r.x, r.y),
        );
        // The change arrows: retail's arrow primitive, the port's cell.
        for &(dir, x, y, ink) in arrows.borrow().iter() {
            assert_eq!((dir, ink), (2, 0), "{what}: the change arrow");
            let i = port
                .iter()
                .position(|d| {
                    (d.src.0, d.src.1) == (arrow_cell.atlas_x, arrow_cell.atlas_y)
                        && (d.dst.0, d.dst.1) == (x, y)
                })
                .unwrap_or_else(|| panic!("{what}: the arrow at {:?}", (x, y)));
            port.remove(i);
        }
        assert_eq!(arrows.borrow().len(), best_changes.len(), "{what}");
        changed += best_changes.len() as u32;
        bests += u32::from(on_best);
        let text: Vec<Ev> = log
            .iter()
            .filter(|e| matches!(e, Ev::Text(..) | Ev::Number(..) | Ev::Separator(..)))
            .cloned()
            .collect();
        // A rise / fall arrow is the compare panel's arrow field; the rest
        // lays out through the rig.
        let mut want_text: Vec<TextDraw> = Vec::new();
        for e in &text {
            match e {
                Ev::Separator(g @ (4 | 5), x, y, ink) => {
                    want_text.extend(legaia_engine_ui::compare_panel_draws_for(
                        &rig.font,
                        &[legaia_engine_ui::ComparePanelField::Arrow {
                            x: *x,
                            y: *y,
                            glyph: *g,
                            ink: *ink,
                        }],
                    ))
                }
                e => want_text.extend(rig.text_of(std::slice::from_ref(e))),
            }
        }
        assert_eq!(keys(&want_text), keys(&port), "{what}\n{text:?}");

        // The hands: the row the flag word names, in its variant.
        let want_hands: Vec<(i32, i32, u8)> = (0..8u32)
            .filter_map(|n| {
                ChoiceFlags(flags)
                    .marker_variant(n)
                    .map(|v| (r.x, r.y + n as i32 * 0xE, v))
            })
            .collect();
        let got: Vec<(i32, i32, u8)> = hands(&log).iter().map(|h| (h.x, h.y, h.variant)).collect();
        assert_eq!(got, want_hands, "{what}: hands");
        // Each mode bobs by its own offset here, so a hand names its mode.
        let bob = legaia_engine_ui::menu::window_painters::HandBob {
            animated: (0, 100),
            clocked: (0, 200),
        };
        let sprites = equip_screen_sprites_for(
            &atlas,
            7,
            (r.x, r.y),
            (0x4000, 0x4000),
            0,
            Some(ChoiceFlags(flags)),
            bob,
            (0, 0),
            1,
        );
        let port_hands: Vec<(i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 == 1 && s.dst.0 < 0x2000)
            .map(|s| (s.dst.0, s.dst.1))
            .collect();
        // Static (mode 0) unmoved, animated (1) and clocked (4) by their bobs.
        let moved: Vec<(i32, i32)> = got
            .iter()
            .map(|h| (h.0, h.1 + [0, 100, 0, 0, 200][usize::from(h.2)]))
            .collect();
        assert_eq!(
            port_hands, moved,
            "{what}: the port's hands and their modes"
        );
        // The pictograms: the slot column, then the change list's.
        let mut port_icons: Vec<(u32, i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 >= 0x100)
            .map(|s| (icon_id(s.src.0), s.dst.0, s.dst.1))
            .collect();
        let rows: Vec<u8> = best_changes.iter().map(|c| c.0).collect();
        port_icons.extend(
            equip_best_change_sprites_for(&atlas, &rows, (r.x, r.y), (0, 0), 1)
                .iter()
                .map(|s| (icon_id(s.src.0), s.dst.0, s.dst.1)),
        );
        let want: Vec<(u32, i32, i32)> = pictos(&log)
            .iter()
            .map(|p| (u32::from(p.id), p.x, p.y))
            .collect();
        assert_eq!(port_icons, want, "{what}: pictograms");

        // The slot rows' item panel.
        let hovered = (second && flags & 0xFFF != 0).then(|| worn[(flags & 0xFFF) as usize - 1]);
        let frames: Vec<&Ev> = log.iter().filter(|e| matches!(e, Ev::Frame(..))).collect();
        match hovered {
            Some(item) if item != 0 => {
                let info = legaia_engine_ui::PauseItemInfo {
                    name: "",
                    count: 0,
                    desc: "",
                    passive: None,
                    detail: None,
                    scope: None,
                };
                let (_, boxes) = equip_browse_info_draws(&rig.font, (r.x, r.y), &info);
                let (px, py) = (boxes[0].0, boxes[0].1);
                assert_eq!(
                    *panels.borrow(),
                    vec![
                        (ITEM_PANEL, px, py, u32::from(item)),
                        (PANEL_MODE, px, py, 3)
                    ],
                    "{what}: the panel on its moved window"
                );
                let want: Vec<Ev> = boxes
                    .iter()
                    .map(|b| Ev::Frame(b.0, b.1, b.2, b.3))
                    .collect();
                assert_eq!(frames, want.iter().collect::<Vec<_>>(), "{what}: boxes");
                infos += 1;
            }
            _ => {
                assert!(panels.borrow().is_empty() && frames.is_empty(), "{what}");
            }
        }
        if let Some(item) = hovered {
            assert_eq!(rig.cpu.mem.rd32(STAGED_ID), u32::from(item), "{what}");
        }
        // The window record is put back.
        assert_eq!(
            (
                rig.cpu.mem.rd16(WINDOW + 0xA) as i16 as i32,
                rig.cpu.mem.rd16(WINDOW + 0xC) as i16 as i32
            ),
            (r.x, r.y),
            "{what}"
        );
    }
    assert!(
        bests > 300 && infos > 1500 && changed > 500 && settled > 2000,
        "{bests} {infos} {changed} {settled}"
    );
}
