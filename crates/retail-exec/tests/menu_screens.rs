//! The pause menu's larger screens (PROT 0899) against `engine-ui`, on
//! `menu_painters`' rig: every leaf draw logged with the ink word
//! `_DAT_8007B454` it ran under, laid out with the port's primitives and held
//! to the port's builder in order. Pictograms and label tokens the port draws
//! as atlas sprites are compared by position.

use crate::common::{Rng, disc};
use crate::menu_painters::{Ev, cstr, hands, pictos, rect, rig, same_text};
use legaia_engine_ui::{
    SaveMenuAtlasRects, SpriteDraw, StatusSatelliteView, TextDraw, status_satellite_draws_for_with,
    status_satellite_icon_sprites_for,
};
use legaia_game_tables::menu_pool::MenuPoolLabels;

const PARTY_LIST: u32 = 0x801D_2094;
const CONDITION: u32 = 0x801D_30A4;
const SUMMARY: u32 = 0x801D_31EC;

const PARTY_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
/// The party-list cursor word and the pager's page word.
const LIST_CURSOR: u32 = 0x801E_46C4;
const PAGE: u32 = 0x801E_46C0;
/// Per character: the Ra-Seru equip byte's seat and its id run's first id.
const RASERU_SEAT: u32 = 0x8007_B424;
const RASERU_BASE: u32 = 0x8007_B434;
/// Per character: the worn Seru's name.
const SERU_NAMES: u32 = 0x801E_46D4;

fn pool() -> MenuPoolLabels {
    MenuPoolLabels::from_menu_overlay(
        &disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    )
}

/// Where the port's sprites land on a 1:1 stage at the origin.
fn spots(sprites: &[SpriteDraw]) -> Vec<(i32, i32)> {
    sprites.iter().map(|s| (s.dst.0, s.dst.1)).collect()
}

fn text_only(log: &[Ev]) -> Vec<Ev> {
    log.iter()
        .filter(|e| matches!(e, Ev::Text(..) | Ev::Number(..) | Ev::Separator(..)))
        .cloned()
        .collect()
}

/// Drop the port draw whose atlas source is `src` and destination `at`.
fn take_cell(port: &mut Vec<TextDraw>, src: (u32, u32), at: (i32, i32)) -> bool {
    let Some(i) = port
        .iter()
        .position(|d| (d.src.0, d.src.1) == src && (d.dst.0, d.dst.1) == at)
    else {
        return false;
    };
    port.remove(i);
    true
}

#[test]
fn the_status_satellites_match() {
    differential!(
        prot(899, PARTY_LIST),
        prot(899, CONDITION),
        prot(899, SUMMARY)
    );
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pool = pool();
    rig.font = rig
        .font
        .clone()
        .with_sprite_cells(&[legaia_font::SpriteCellSrc {
            id: legaia_font::sprite_cell_ids::SERU_TAG,
            w: 8,
            h: 8,
            rgba: vec![0xFF; 8 * 8 * 4],
        }]);
    let seru_tag = rig
        .font
        .sprite_cell(legaia_font::sprite_cell_ids::SERU_TAG)
        .expect("the Seru tag cell");
    let atlas = SaveMenuAtlasRects {
        atr_icons: [(0, 0, 8, 8); 3],
        ..Default::default()
    };
    let mut rng = Rng(0x5341_5445);
    let mut worn = 0;
    for run in 0..3_000 {
        let n = 1 + rng.below(3);
        let mut pool_ids = vec![0u32, 1, 2];
        let ids: Vec<u32> = (0..n)
            .map(|_| pool_ids.remove(rng.below(pool_ids.len() as u32) as usize))
            .collect();
        rig.cpu.mem.wr8(PARTY_COUNT, n as u8);
        let mut names = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            rig.cpu.mem.wr8(ROSTER + i as u32, id as u8);
            let rec = RECORDS + id * STRIDE;
            let name = format!("Name{id}{}", rng.below(100));
            rig.poke_str(rec + 0x2A7, &name);
            rig.cpu.mem.wr8(rec + 0x130, 1 + rng.below(99) as u8);
            names.push(name);
        }
        let cursor = rng.below(n);
        // The hand on its row, the editing bit either way.
        rig.cpu.mem.wr32(LIST_CURSOR, cursor | (rng.below(2) << 12));
        let id = ids[cursor as usize];
        let rec = RECORDS + id * STRIDE;
        let seat = rig.cpu.mem.rd16(RASERU_SEAT + id * 2) as i16 as u32;
        let base = u32::from(rig.cpu.mem.rd8(RASERU_BASE + id));
        let seru_id = if rng.below(3) == 0 {
            0
        } else {
            base + rng.below(9)
        };
        rig.cpu.mem.wr8(rec + 0x196 + seat, seru_id as u8);
        let page = rng.below(4);
        rig.cpu.mem.wr32(PAGE, page | (rng.below(2) << 12));

        let (r1, r2, r3) = (rect(&mut rng), rect(&mut rng), rect(&mut rng));
        let mut log = rig.run(PARTY_LIST, r1);
        let list_hands = hands(&log);
        let cond = rig.run(CONDITION, r2);
        let cond_hands = hands(&cond);
        log.extend(cond);
        let summary = rig.run(SUMMARY, r3);
        log.extend(summary.clone());

        let seru_name = (seru_id != 0).then(|| {
            let va = rig.cpu.mem.rd32(SERU_NAMES + id * 4);
            String::from_utf8(cstr(&mut rig.cpu.mem, va)).expect("ASCII")
        });
        let seru = seru_name
            .as_deref()
            .map(|s| (s, (seru_id - base + 1) as u8));
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let view = StatusSatelliteView {
            party_names: &refs,
            cursor: cursor as usize,
            name: &names[cursor as usize],
            level: rig.cpu.mem.rd8(rec + 0x130),
            page: page as u8,
            seru,
        };
        let pens = ((r1.x, r1.y), (r2.x, r2.y), (r3.x, r3.y));
        let mut port = status_satellite_draws_for_with(
            &rig.font,
            Some(&pool),
            &view,
            pens.0,
            pens.1,
            pens.2,
            true,
        );
        let what = format!("run {run} ids {ids:?} cursor {cursor} page {page} seru {seru_id:#x}");
        // The Seru tag is a text cell in the port, a pictogram in retail.
        let tags: Vec<(i32, i32)> = pictos(&summary)
            .iter()
            .filter(|p| p.id == 0x45)
            .map(|p| (p.x, p.y))
            .collect();
        for at in &tags {
            assert!(
                take_cell(&mut port, (seru_tag.atlas_x, seru_tag.atlas_y), *at),
                "{what}: the Seru tag at {at:?}"
            );
            worn += 1;
        }
        same_text(&rig, &text_only(&log), &port, &what);

        // The atlas sprites: list hand, pager triangles, LV labels, the
        // element token.
        let sprites = status_satellite_icon_sprites_for(
            &atlas,
            cursor as usize,
            id as usize,
            seru.is_some(),
            pens.0,
            pens.1,
            pens.2,
            (0, 0),
            1,
        );
        let mut want: Vec<(i32, i32)> = list_hands.iter().map(|h| (h.x, h.y)).collect();
        want.extend(cond_hands.iter().map(|h| (h.x, h.y)));
        want.extend(
            pictos(&summary)
                .iter()
                .filter(|p| p.id == 0x0A)
                .map(|p| (p.x, p.y))
                .take(1),
        );
        want.extend(
            pictos(&summary)
                .iter()
                .filter(|p| p.id == 0x0A)
                .map(|p| (p.x, p.y))
                .skip(1),
        );
        want.extend(summary.iter().filter_map(|e| match e {
            Ev::Label(_, x, y, _) => Some((*x, *y)),
            _ => None,
        }));
        assert_eq!(spots(&sprites), want, "{what}: sprites");
    }
    assert!(worn > 1000, "{worn}");
}

const COMMANDS: u32 = 0x801D_0D18;
const INFO_WINDOW: u32 = 0x801D_CB60;
const INFO_PANEL: u32 = 0x801D_0F1C;
const STAGED_ID: u32 = 0x801E_46B0;
const COMMAND_CURSOR: u32 = 0x801E_46C0;
const BAG: u32 = 0x8008_5958;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;
const ITEMS: u32 = 0x8007_4368;
const POINTS: u32 = 0x8008_45B4;

#[test]
fn the_item_screens_command_and_info_windows_match() {
    differential!(
        prot(899, COMMANDS),
        prot(899, INFO_WINDOW),
        prot(899, INFO_PANEL)
    );
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The info panel runs inside its window here.
    rig.cpu.unhook(INFO_PANEL);
    let pool = pool();
    let scus = crate::common::scus().unwrap();
    let text = legaia_engine_menus::pause_screens::MenuTextTables::from_scus(&scus);
    let effects =
        legaia_game_tables::item_effect::ItemEffectTable::from_scus(&scus).expect("item effects");
    let cell = |id| legaia_font::SpriteCellSrc {
        id,
        w: 12,
        h: 12,
        rgba: vec![0xFF; 12 * 12 * 4],
    };
    rig.font = rig.font.clone().with_sprite_cells(&[
        cell(legaia_font::sprite_cell_ids::ITEM_SCOPE_ONE),
        cell(legaia_font::sprite_cell_ids::ITEM_SCOPE_ALL),
    ]);
    // Names the string writer would expand get an ASCII stand-in.
    let stand_in = 0x8013_0100;
    rig.poke_str(stand_in, "@Stand-in");
    for id in 0..256u32 {
        let p = rig.cpu.mem.rd32(ITEMS + id * 12 + 4);
        let ascii = (0x8001_0000..0x8008_0000).contains(&p)
            && cstr(&mut rig.cpu.mem, p)
                .iter()
                .all(|b| (0x20..0x80).contains(b));
        if !ascii {
            rig.cpu.mem.wr32(ITEMS + id * 12 + 4, stand_in);
        }
    }
    let mut rng = Rng(0x4954_454D);
    let (mut passives, mut greys, mut cards, mut scopes) = (0, 0, 0, 0);
    for run in 0..6_000 {
        let r = rect(&mut rng);
        // The command window over a bag window that holds something, or not.
        let (lo, hi) = [(0u16, 256u16), (0, 128), (128, 256)][rng.below(3) as usize];
        rig.cpu.mem.wr16(WINDOW_LO, lo);
        rig.cpu.mem.wr16(WINDOW_HI, hi);
        let stocked = rng.below(3) != 0;
        for i in 0..256u32 {
            let (id, n) = match rng.below(8) {
                0 if stocked => (1 + rng.below(255) as u8, 1 + rng.below(99) as u8),
                1 => (0, rng.next() as u8),
                2 => (rng.next() as u8, 0),
                _ => (0, 0),
            };
            rig.cpu.mem.wr8(BAG + 2 * i, id);
            rig.cpu.mem.wr8(BAG + 2 * i + 1, n);
        }
        let held = (lo..hi).any(|i| {
            let a = BAG + 2 * u32::from(i);
            rig.cpu.mem.rd8(a) != 0 && rig.cpu.mem.rd8(a + 1) != 0
        });
        let cursor = rng.below(3);
        rig.cpu
            .mem
            .wr32(COMMAND_CURSOR, cursor | (rng.below(2) << 12));
        let log = rig.run(COMMANDS, r);
        let view = legaia_engine_ui::PauseItemsView {
            arrows_blink: false,
            rows: &[],
            page: 0,
            pages: 0,
            phase: legaia_engine_ui::PauseItemsPhase::Command,
            command_cursor: cursor as u8,
            list_cursor: 0,
            bag_empty: !held,
            info: None,
            text_cursor: false,
        };
        // The list page is off the stage, so only the command rows remain.
        let port: Vec<TextDraw> = legaia_engine_ui::items_screen_draws_for_with(
            &rig.font,
            Some(&pool),
            &view,
            (r.x, r.y),
            (0x4000, 0x4000),
            (0x4000, 0x4000),
        )
        .into_iter()
        .filter(|d| d.dst.0 < 0x2000)
        .collect();
        let what = format!("run {run} window {lo}..{hi} held {held}");
        same_text(&rig, &log, &port, &format!("{what} commands"));
        greys += u32::from(!held);
        let got: Vec<(i32, i32)> = hands(&log).iter().map(|h| (h.x, h.y)).collect();
        assert_eq!(
            got,
            vec![(r.x, r.y + cursor as i32 * 0xE)],
            "{what}: the command hand"
        );

        // The info window over a staged item.
        let item = [0u32, 0xFE, 1 + rng.below(255), 1 + rng.below(255)][rng.below(4) as usize];
        rig.cpu.mem.wr32(STAGED_ID, item);
        let slot = rng.below(0x100);
        let count = rng.below(100) as u8;
        let found = rng.below(3) != 0;
        *rig.bag.borrow_mut() = (if found { slot } else { 0x100 }, 0);
        rig.cpu.mem.wr8(BAG + slot * 2 + 1, count);
        let points = rng.below(100_000_000);
        rig.cpu.mem.wr32(POINTS, points);
        let log = rig.run(INFO_WINDOW, r);
        let what = format!("run {run} item {item:#x}");
        assert!(
            log.last() == Some(&Ev::Frame(r.x, r.y + 0x38, 0x90, 0x28)),
            "{what}: the widget box {log:?}"
        );
        if item == 0 {
            assert!(
                !log.iter()
                    .any(|e| matches!(e, Ev::Text(..) | Ev::Number(..))),
                "{what}"
            );
            continue;
        }
        let id = item as u8;
        let rec = ITEMS + item * 12;
        let name_va = rig.cpu.mem.rd32(rec + 4);
        let name = rig.string(name_va);
        let passive = text.item_passive_text(id).filter(|_| id != 0xFE);
        let scope = passive.as_ref().and_then(|_| {
            (effects.kind(id) == 2)
                .then(|| effects.descriptor(effects.subtype(id)))
                .flatten()
                .map(|e| legaia_engine_menus::pause_screens::item_scope_icon(&e))
        });
        let info = legaia_engine_ui::PauseItemInfo {
            name: &name,
            count: if found { u16::from(count) } else { 0 },
            desc: "",
            passive: passive.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            detail: None,
            scope,
        };
        let mut port = legaia_engine_ui::item_info_panel_draws_for(&rig.font, (r.x, r.y), &info);
        if id == 0xFE {
            port.extend(legaia_engine_ui::item_points_panel_draws_with(
                &rig.font,
                Some(&pool),
                (r.x, r.y),
                points,
            ));
            cards += 1;
        }
        // The scope pictogram: retail's icon `0x84` / `0x85`, the port's cell.
        let picked: Vec<_> = pictos(&log).into_iter().filter(|p| p.id >= 0x84).collect();
        let want_cell = picked.first().map(|p| {
            if p.id == 0x84 {
                legaia_font::sprite_cell_ids::ITEM_SCOPE_ONE
            } else {
                legaia_font::sprite_cell_ids::ITEM_SCOPE_ALL
            }
        });
        assert_eq!(scope, want_cell, "{what}: the scope icon");
        if let (Some(p), Some(cell)) = (picked.first(), scope) {
            let c = rig.font.sprite_cell(cell).expect("the scope cell");
            assert!(
                take_cell(&mut port, (c.atlas_x, c.atlas_y), (p.x, p.y)),
                "{what}: the scope icon at {p:?}"
            );
            scopes += 1;
        }
        // The passive lines go through the label writer, the rest through the
        // string and number writers.
        let as_text: Vec<Ev> = log
            .iter()
            .map(|e| match e {
                Ev::Label(va, x, y, ink) => Ev::Text(cstr(&mut rig.cpu.mem, *va), *x, *y, *ink),
                e => e.clone(),
            })
            .collect();
        same_text(&rig, &as_text, &port, &what);
        let desc_va = rig.cpu.mem.rd32(rec + 8);
        let details: Vec<&Ev> = log.iter().filter(|e| matches!(e, Ev::Detail(..))).collect();
        assert_eq!(
            details,
            vec![&Ev::Detail(desc_va, item, r.x, r.y + 0x10, 7)],
            "{what}"
        );
        passives += u32::from(passive.is_some());
        // The panel on its own is the window's draws past the count, up to
        // the widget box.
        let alone = rig.run(INFO_PANEL, r);
        let inner: Vec<Ev> = log
            .iter()
            .skip(1)
            .filter(|e| !matches!(e, Ev::Call(_) | Ev::Frame(..)))
            .cloned()
            .collect();
        assert_eq!(alone, inner, "{what}: the panel alone");
    }
    assert!(
        passives > 300 && greys > 500 && cards > 500 && scopes > 300,
        "{passives} {greys} {cards} {scopes}"
    );
}
