//! The Status screen's main panel `FUN_801D33D8` (PROT 0899), Condition page,
//! against `engine-ui`'s `status_screen_draws_for_with` and
//! `status_icon_sprites_for`, on `menu_painters`' rig.
//!
//! The record is generated, the view is read off it the way the host reads
//! it (`CharacterRecord`'s accessors, the port's stat block and equip-row
//! order), and every text draw is held to retail's in order. Pictograms are
//! compared by position under the id each atlas rect stands for.

use crate::common::{Rng, disc, scus};
use crate::menu_painters::{Ev, cstr, pictos, rect, rig, same_text};
use legaia_engine_menus::equip_session::{BROWSE_SLOT_ORDER, engine_equip_from_record};
use legaia_engine_menus::equipment::equip_modifier_table_from_disc;
use legaia_engine_menus::pause_screens::{menu_stat_block, stat_record_from_character};
use legaia_engine_ui::{
    SaveMenuAtlasRects, StatusIconPage, StatusPanelView, StatusStatRow, TextDraw,
    status_icon_sprites_for, status_screen_draws_for_with,
};
use legaia_game_tables::equip_stats::EquipStatTable;
use legaia_game_tables::menu_pool::{MenuLabel, MenuPoolLabels};
use legaia_save::CharacterRecord;

const STATUS_PANEL: u32 = 0x801D_33D8;
const GAUGE: u32 = 0x8002_C69C;

const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const LIST_CURSOR: u32 = 0x801E_46C4;
const PAGE: u32 = 0x801E_46C0;
const RASERU_SEAT: u32 = 0x8007_B424;
const RASERU_BASE: u32 = 0x8007_B434;
const SERU_NAMES: u32 = 0x801E_46D4;
const ITEMS: u32 = 0x8007_4368;
/// The seven equipment rows' pictogram ids, halfwords.
const ROW_ICONS: u32 = 0x801E_43F4;

/// A hook's log.
type Calls<T> = std::rc::Rc<std::cell::RefCell<Vec<T>>>;

fn put16(raw: &mut [u8], off: usize, v: u16) {
    raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn the_condition_page_matches() {
    differential!(prot(899, STATUS_PANEL));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus().unwrap();
    let pool = MenuPoolLabels::from_menu_overlay(
        &disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    let table =
        equip_modifier_table_from_disc(&EquipStatTable::from_scus(&scus).expect("equipment table"));
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
    // Every item name the string writer would expand gets an ASCII stand-in,
    // and the port is handed the same bytes.
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
    // One distinct source per atlas rect, so a sprite names its pictogram.
    let tag = |n: u32| (n, 0, 1, 1);
    let atlas = SaveMenuAtlasRects {
        label_lv: tag(0x0A),
        label_hp: tag(0x3F),
        label_mp: tag(0x40),
        icon_weapon: tag(0x100),
        icon_helmet: tag(0x101),
        icon_armor: tag(0x102),
        icon_boot: tag(0x103),
        icon_goods: tag(0x104),
        gauge_cap: tag(0x200),
        gauge_trough: tag(0x201),
        gauge_box: tag(0x202),
        gauge_tip: tag(0x203),
        gauge_digits: (0x300, 0, 6, 6),
        gauge_100: tag(0x204),
        gauge_fill: tag(0x205),
        ..Default::default()
    };
    let row_icons: Vec<u32> = (0..7)
        .map(|i| u32::from(rig.cpu.mem.rd16(ROW_ICONS + i * 2)))
        .collect();
    assert_eq!(row_icons[4], row_icons[5], "the Goods rows share an icon");
    let stat_labels: Vec<&str> = [
        MenuLabel::Atk,
        MenuLabel::Udf,
        MenuLabel::Ldf,
        MenuLabel::Spd,
        MenuLabel::Int,
        MenuLabel::Agl,
    ]
    .into_iter()
    .map(|l| pool.get(l).expect("stat cell"))
    .collect();

    rig.cpu.unhook(GAUGE);
    let gauge: Calls<(i32, i32, u32, u32)> = Default::default();
    let g = gauge.clone();
    rig.cpu.hook(GAUGE, move |regs, _| {
        g.borrow_mut().push((
            regs.r[psx_retail_exec::A0] as i32,
            regs.r[psx_retail_exec::A1] as i32,
            regs.r[psx_retail_exec::A2],
            regs.r[psx_retail_exec::A3],
        ))
    });
    let mut rng = Rng(0x5354_4154);
    let (mut worn, mut boosted, mut capped) = (0, 0, 0);
    for run in 0..4_000 {
        let id = rng.below(3);
        let seat = rng.below(3);
        rig.cpu.mem.wr8(ROSTER + seat, id as u8);
        rig.cpu.mem.wr32(LIST_CURSOR, seat | (rng.below(2) << 12));
        // The Condition page is page 0, or 5 once the pager has wrapped.
        let page = [0u32, 5][rng.below(2) as usize];
        rig.cpu.mem.wr32(PAGE, page | (rng.below(2) << 12));

        let mut raw = vec![0u8; STRIDE as usize];
        raw[0..4].copy_from_slice(&rng.below(100_000_000).to_le_bytes());
        raw[4..8].copy_from_slice(&rng.below(100_000_000).to_le_bytes());
        let hp_max = 1 + rng.below(9999) as u16;
        let mp_max = rng.below(1000) as u16;
        put16(&mut raw, 0x104, hp_max);
        put16(&mut raw, 0x106, rng.below(u32::from(hp_max) + 1) as u16);
        put16(&mut raw, 0x108, mp_max);
        put16(&mut raw, 0x10A, rng.below(u32::from(mp_max) + 1) as u16);
        put16(&mut raw, 0x10E, rng.below(101) as u16);
        // The live stats, at times past what the page prints.
        for off in (0x110..=0x11A).step_by(2) {
            let v = match rng.below(5) {
                0 => 999,
                _ => rng.below(1000),
            };
            put16(&mut raw, off, v as u16);
        }
        // The record-side window, apart from the live one.
        put16(&mut raw, 0x11C, rng.below(10_000) as u16);
        put16(&mut raw, 0x11E, rng.below(1000) as u16);
        for off in (0x122..=0x12C).step_by(2) {
            put16(&mut raw, off, rng.below(1000) as u16);
        }
        if rng.below(4) == 0 {
            put16(&mut raw, 0x12E, 1 << rng.below(16));
        }
        raw[0x130] = 1 + rng.below(99) as u8;
        for e in 0..8 {
            raw[0x196 + e] = match rng.below(4) {
                0 => 0,
                _ => rng.below(256) as u8,
            };
        }
        let seru_seat = rig.cpu.mem.rd16(RASERU_SEAT + id * 2) as usize;
        let base = rig.cpu.mem.rd8(RASERU_BASE + id);
        let seru_id = match rng.below(3) {
            0 => 0,
            _ => base + rng.below(9) as u8,
        };
        raw[0x196 + seru_seat] = seru_id;
        let name = format!("Hero{id}{}", rng.below(100));
        raw[0x2A7..0x2A7 + name.len()].copy_from_slice(name.as_bytes());
        let rec_va = RECORDS + id * STRIDE;
        for (i, b) in raw.iter().enumerate() {
            rig.cpu.mem.wr8(rec_va + i as u32, *b);
        }

        let r = rect(&mut rng);
        gauge.borrow_mut().clear();
        let log = rig.run(STATUS_PANEL, r);

        // The host's reading of the record.
        let rec = CharacterRecord::parse(&raw).expect("record");
        let hms = rec.hp_mp_sp();
        let growth = rec.record_stats();
        let mut stat_record = stat_record_from_character(&rec);
        stat_record.equip = engine_equip_from_record(stat_record.equip, id as u8);
        let block = menu_stat_block(&stat_record, &table, hms.hp_max, hms.mp_max);
        let word = |w: usize| block[w].clamp(0, 999) as u32;
        capped += u32::from((2..8).any(|w| block[w] > 999));
        let pairs = [
            (word(3), growth.atk),
            (word(4), growth.udf),
            (word(5), growth.ldf),
            (word(6), growth.spd),
            (word(7), growth.int),
            (word(2), growth.agl),
        ];
        let stat_rows: Vec<StatusStatRow<'_>> = pairs
            .iter()
            .zip(&stat_labels)
            .map(|(&(value, growth), label)| StatusStatRow {
                label,
                value,
                growth: u32::from(growth),
            })
            .collect();
        let engine = engine_equip_from_record(rec.equipment().slots, id as u8);
        let equip_rows: Vec<(&str, &str)> = BROWSE_SLOT_ORDER
            .iter()
            .map(|&s| ("", item_names[usize::from(engine[usize::from(s)])].as_str()))
            .collect();
        let seru_name = (seru_id != 0).then(|| {
            let va = rig.cpu.mem.rd32(SERU_NAMES + id * 4);
            String::from_utf8(cstr(&mut rig.cpu.mem, va)).expect("ASCII")
        });
        let seru = seru_name
            .as_deref()
            .map(|s| (s, seru_id.wrapping_sub(base).wrapping_add(1)));
        boosted += u32::from(growth.hp_max != hms.hp_max);
        let panel = StatusPanelView {
            name: &name,
            level: rec.magic_rank(),
            xp: rec.cumulative_xp(),
            xp_to_next: u32::from_le_bytes(raw[4..8].try_into().unwrap()),
            hp: hms.hp_cur,
            hp_max: hms.hp_max,
            hp_base: growth.hp_max,
            mp: hms.mp_cur,
            mp_max: hms.mp_max,
            mp_base: growth.mp_max,
            ap: hms.sp_cur as u8,
            ap_max: 100,
            stat_rows: &stat_rows,
            equip_rows: &equip_rows,
            status: u16::from_le_bytes([raw[0x12E], raw[0x12F]]),
            seru,
            page: 0,
            skills: &[],
            spells: &[],
            moves: &[],
            list: None,
            scroll: 0,
        };
        let what = format!("run {run} id {id} page {page} seru {seru_id:#x} at {r:?}");
        let mut port: Vec<TextDraw> =
            status_screen_draws_for_with(&rig.font, Some(&pool), &panel, None, (r.x, r.y), true);
        // The Seru tag is a text cell in the port, a pictogram in retail.
        for p in pictos(&log).iter().filter(|p| p.id == 0x45) {
            let i = port
                .iter()
                .position(|d| {
                    (d.src.0, d.src.1) == (seru_tag.atlas_x, seru_tag.atlas_y)
                        && (d.dst.0, d.dst.1) == (p.x, p.y)
                })
                .unwrap_or_else(|| panic!("{what}: the Seru tag at {:?}", (p.x, p.y)));
            port.remove(i);
            worn += 1;
        }
        let text: Vec<Ev> = log
            .iter()
            .filter(|e| matches!(e, Ev::Text(..) | Ev::Number(..) | Ev::Separator(..)))
            .cloned()
            .collect();
        same_text(&rig, &text, &port, &what);

        // The pictograms, then the gauge the bar widget is called for.
        let sprites = status_icon_sprites_for(
            &atlas,
            (r.x, r.y),
            hms.sp_cur,
            StatusIconPage {
                page: 0,
                seru: seru.is_some(),
                ..Default::default()
            },
            (0, 0),
            1,
        );
        let port_icons: Vec<(u32, i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 < 0x200)
            .map(|s| {
                let id = match s.src.0 {
                    n @ 0x100..=0x103 => row_icons[(n - 0x100) as usize],
                    0x104 => row_icons[4],
                    n => n,
                };
                (id, s.dst.0, s.dst.1)
            })
            .collect();
        let want: Vec<(u32, i32, i32)> = pictos(&log)
            .iter()
            .filter(|p| p.id != 0x45)
            .map(|p| (u32::from(p.id), p.x, p.y))
            .collect();
        let sorted = |mut v: Vec<(u32, i32, i32)>| {
            v.sort();
            v
        };
        assert_eq!(sorted(port_icons), sorted(want), "{what}: pictograms");
        assert_eq!(
            *gauge.borrow(),
            vec![(r.x + 0x40, r.y + 0x2D, 1, u32::from(hms.sp_cur))],
            "{what}: the AP bar"
        );
        let cap = sprites
            .iter()
            .find(|s| s.src.0 == 0x200)
            .expect("the gauge cap");
        assert_eq!((cap.dst.0, cap.dst.1), (r.x + 0x40, r.y + 0x2D), "{what}");
    }
    assert!(
        worn > 1000 && boosted > 1000 && capped > 100,
        "{worn} {boosted} {capped}"
    );
}

/// The Skills page (`0x801D3E64..0x801D4098`): one block per Goods byte whose
/// item grants a passive under `0x40` - the row's pictogram, the item's name,
/// the passive's name and its description - or the one "no skills" line.
/// A label draw is laid out with the port's own `|` line rule; what is held
/// is every string, pen and ink.
#[test]
fn the_skills_page_matches() {
    differential!(prot(899, STATUS_PANEL));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus().unwrap();
    let pool = MenuPoolLabels::from_menu_overlay(
        &disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    let text = legaia_engine_menus::pause_screens::MenuTextTables::from_scus(&scus);
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
    let granting: Vec<u8> = (1..=255u8)
        .filter(|&id| text.item_passive_text(id).is_some())
        .collect();
    assert!(granting.len() > 20, "{} passive items", granting.len());
    let goods_icon = u32::from(rig.cpu.mem.rd16(ROW_ICONS + 8));
    let tag = |n: u32| (n, 0, 1, 1);
    let atlas = SaveMenuAtlasRects {
        label_lv: tag(0x0A),
        icon_goods: tag(0x104),
        ..Default::default()
    };
    let mut rng = Rng(0x534B_494C);
    let (mut empty, mut rows_seen) = (0, 0);
    for run in 0..3_000 {
        let id = rng.below(3);
        let seat = rng.below(3);
        rig.cpu.mem.wr8(ROSTER + seat, id as u8);
        rig.cpu.mem.wr32(LIST_CURSOR, seat);
        // Page 1, or 6 once the pager has wrapped.
        rig.cpu.mem.wr32(PAGE, [1u32, 6][rng.below(2) as usize]);
        let rec_va = RECORDS + id * STRIDE;
        for o in 0..STRIDE {
            rig.cpu.mem.wr8(rec_va + o, 0);
        }
        let name = format!("Hero{id}");
        rig.poke_str(rec_va + 0x2A7, &name);
        rig.cpu.mem.wr8(rec_va + 0x130, 7);
        let mut goods = [0u8; 3];
        for (i, g) in goods.iter_mut().enumerate() {
            *g = match rng.below(4) {
                0 => 0,
                1 => rng.below(256) as u8,
                _ => granting[rng.below(granting.len() as u32) as usize],
            };
            rig.cpu.mem.wr8(rec_va + 0x196 + 5 + i as u32, *g);
        }
        let r = rect(&mut rng);
        let log = rig.run(STATUS_PANEL, r);
        let what = format!("run {run} id {id} goods {goods:02x?} at {r:?}");

        let rows: Vec<(String, String, String)> = goods
            .iter()
            .filter_map(|&g| {
                let (effect, desc) = text.item_passive_text(g)?;
                Some((item_names[usize::from(g)].clone(), effect, desc))
            })
            .collect();
        empty += u32::from(rows.is_empty());
        rows_seen += rows.len() as u32;
        let skills: Vec<legaia_engine_ui::StatusSkillRowView<'_>> = rows
            .iter()
            .map(
                |(item, effect, desc)| legaia_engine_ui::StatusSkillRowView {
                    item_name: item,
                    effect_name: effect,
                    description: desc,
                },
            )
            .collect();
        let panel = StatusPanelView {
            name: &name,
            level: 7,
            xp: 0,
            xp_to_next: 0,
            hp: 0,
            hp_max: 0,
            hp_base: 0,
            mp: 0,
            mp_max: 0,
            mp_base: 0,
            ap: 0,
            ap_max: 100,
            stat_rows: &[],
            equip_rows: &[],
            status: 0,
            seru: None,
            page: 1,
            skills: &skills,
            spells: &[],
            moves: &[],
            list: None,
            scroll: 0,
        };
        let port =
            status_screen_draws_for_with(&rig.font, Some(&pool), &panel, None, (r.x, r.y), true);
        let mut want: Vec<TextDraw> = Vec::new();
        for e in &log {
            match e {
                Ev::Label(va, x, y, ink) => {
                    let bytes = cstr(&mut rig.cpu.mem, *va);
                    want.extend(legaia_engine_ui::label_lines_draws(
                        &rig.font,
                        &bytes,
                        (*x, *y),
                        legaia_engine_ui::menu_ink_color(*ink),
                    ));
                }
                Ev::Text(..) | Ev::Number(..) | Ev::Separator(..) => {
                    want.extend(rig.text_of(std::slice::from_ref(e)))
                }
                _ => {}
            }
        }
        assert_eq!(
            crate::menu_painters::keys(&want),
            crate::menu_painters::keys(&port),
            "{what}\n{log:?}"
        );
        // The pictograms: the header's LV, then one Goods icon a row.
        let sprites = status_icon_sprites_for(
            &atlas,
            (r.x, r.y),
            0,
            StatusIconPage {
                page: 1,
                skills: rows.len(),
                ..Default::default()
            },
            (0, 0),
            1,
        );
        let got: Vec<(u32, i32, i32)> = sprites
            .iter()
            .map(|s| {
                let id = if s.src.0 == 0x104 {
                    goods_icon
                } else {
                    s.src.0
                };
                (id, s.dst.0, s.dst.1)
            })
            .collect();
        let want: Vec<(u32, i32, i32)> = pictos(&log)
            .iter()
            .map(|p| (u32::from(p.id), p.x, p.y))
            .collect();
        assert_eq!(got, want, "{what}: pictograms");
    }
    assert!(empty > 100 && rows_seen > 3000, "{empty} {rows_seen}");
}

/// The label writer `FUN_8003CD00(str, x, y)`, re-hooked so a log entry
/// carries the bytes it drew: the Magic page rewrites its `Lv` buffer
/// (`0x801E46E0`) between rows.
const LABEL: u32 = 0x8003_CD00;
/// The Magic list's scroll top and cursor word, and the latch word (bit
/// `0x1000` set: nothing latched).
const LIST_TOP: u32 = 0x8007_BB90;
const LIST_CURSOR_WORD: u32 = 0x8007_BB98;
const LATCH: u32 = 0x801E_46D0;

/// The Magic page (`0x801D40A8..0x801D43BC`): header, up to seven spell rows
/// from the list's top, the hand and the latched row's second hand, and the
/// hovered spell's preview. Every text draw is held to the port's in order;
/// the hands by seat.
#[test]
fn the_magic_page_matches() {
    differential!(prot(899, STATUS_PANEL));
    use legaia_engine_battle_vm::battle_formulas::{MpCostModifier, mp_cost_after_ability_bits};
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus().unwrap();
    let pool = MenuPoolLabels::from_menu_overlay(
        &disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    let names = legaia_engine_menus::pause_screens::MenuTextTables::from_scus(&scus)
        .spell_names
        .expect("spell names");
    let castable: Vec<u8> = (1..=255u8)
        .filter(|&id| names.name(id).is_some_and(|n| !n.is_empty()))
        .collect();
    assert!(castable.len() > 10, "{} named spells", castable.len());
    let l = rig.log.clone();
    rig.cpu.unhook(LABEL);
    rig.cpu.hook(LABEL, move |r, m| {
        let ink = m.rd32(0x8007_B454) as u8;
        let s = cstr(m, r.r[psx_retail_exec::A0]);
        l.borrow_mut().push(Ev::Text(
            s,
            r.r[psx_retail_exec::A1] as i32,
            r.r[psx_retail_exec::A2] as i32,
            ink,
        ));
    });
    let tag = |n: u32| (n, 0, 1, 1);
    // The preview box's frame needs real piece sizes to tile.
    let atlas = SaveMenuAtlasRects {
        label_lv: tag(0x0A),
        cursor: tag(0x900),
        panel_tl: (0x700, 0, 4, 4),
        panel_tr: (0x704, 0, 4, 4),
        panel_bl: (0x708, 0, 4, 4),
        panel_br: (0x70C, 0, 4, 4),
        panel_top: (0x710, 0, 24, 4),
        panel_bot: (0x710, 4, 24, 4),
        panel_left: (0x730, 0, 4, 24),
        panel_right: (0x734, 0, 4, 24),
        panel_interior: (0x740, 0, 16, 16),
        ..Default::default()
    };
    let mut rng = Rng(0x4D41_4749);
    let (mut empty, mut previewed, mut latched, mut halved) = (0, 0, 0, 0);
    for run in 0..3_000 {
        let id = rng.below(3);
        let seat = rng.below(3);
        rig.cpu.mem.wr8(ROSTER + seat, id as u8);
        rig.cpu.mem.wr32(LIST_CURSOR, seat);
        rig.cpu.mem.wr32(PAGE, [2u32, 7][rng.below(2) as usize]);
        let rec_va = RECORDS + id * STRIDE;
        for o in 0..STRIDE {
            rig.cpu.mem.wr8(rec_va + o, 0);
        }
        let name = format!("Hero{id}");
        rig.poke_str(rec_va + 0x2A7, &name);
        rig.cpu.mem.wr8(rec_va + 0x130, 7);
        let ability = [0u32, 0x10, 0x20, 0x30][rng.below(4) as usize];
        rig.cpu.mem.wr32(rec_va + 0xF4, ability);
        let seru_seat = u32::from(rig.cpu.mem.rd16(RASERU_SEAT + id * 2));
        let base = rig.cpu.mem.rd8(RASERU_BASE + id);
        let seru_id = match rng.below(6) {
            0 => 0,
            _ => base + rng.below(9) as u8,
        };
        rig.cpu.mem.wr8(rec_va + 0x196 + seru_seat, seru_id);
        let count = match rng.below(6) {
            0 => 0,
            _ => 1 + rng.below(14),
        };
        let mut spells = Vec::new();
        for i in 0..count {
            let s = castable[rng.below(castable.len() as u32) as usize];
            let lv = 1 + rng.below(9) as u8;
            rig.cpu.mem.wr8(rec_va + 0x13D + i, s);
            rig.cpu.mem.wr8(rec_va + 0x161 + i, lv);
            spells.push((s, lv));
        }
        rig.cpu.mem.wr8(rec_va + 0x13C, count as u8);
        // The list: its top, the hand (or none) on a visible row, the latch.
        let top = if count > 7 { rng.below(count - 6) } else { 0 };
        let visible = (count - top).min(7);
        // The list holds the pad only over rows: no Seru, no rows.
        let hand =
            (count > 0 && seru_id != 0 && rng.below(3) != 0).then(|| top + rng.below(visible));
        let latch = (hand.is_some() && rng.below(3) == 0).then(|| top + rng.below(visible));
        rig.cpu.mem.wr32(LIST_TOP, top);
        rig.cpu
            .mem
            .wr32(LIST_CURSOR_WORD, hand.unwrap_or(0x4000 | rng.below(0x1000)));
        rig.cpu
            .mem
            .wr32(LATCH, latch.unwrap_or(0x1000 | rng.below(0x1000)));
        let r = legaia_engine_ui::menu::window_painters::PainterRect::new(
            rng.below(100) as i32,
            rng.below(50) as i32,
            218,
            188,
        );
        let log = rig.run(STATUS_PANEL, r);
        let what = format!(
            "run {run} id {id} seru {seru_id:#x} spells {spells:02x?} top {top} hand {hand:?} latch {latch:?} at {r:?}"
        );

        let rows: Vec<(String, Option<u8>, u8, u16, String)> = spells
            .iter()
            .map(|&(s, lv)| {
                let mp = u16::from(names.mp(s).unwrap_or(0));
                (
                    names.name(s).unwrap_or_default().to_string(),
                    names.icon(s),
                    lv,
                    mp_cost_after_ability_bits(mp, MpCostModifier::from_ability_flags(ability)),
                    names.desc(s).unwrap_or_default().to_string(),
                )
            })
            .collect();
        halved += u32::from(ability != 0 && count > 0);
        let views: Vec<legaia_engine_ui::StatusSpellRowView<'_>> = rows
            .iter()
            .map(
                |(n, icon, lv, mp, d)| legaia_engine_ui::StatusSpellRowView {
                    name: n,
                    icon: *icon,
                    level: *lv,
                    mp_cost: *mp,
                    description: d,
                },
            )
            .collect();
        let seru_name = (seru_id != 0).then(|| {
            let va = rig.cpu.mem.rd32(SERU_NAMES + id * 4);
            String::from_utf8(cstr(&mut rig.cpu.mem, va)).expect("ASCII")
        });
        let seru = seru_name
            .as_deref()
            .map(|s| (s, seru_id.wrapping_sub(base).wrapping_add(1)));
        let list = hand.map(|h| legaia_engine_ui::StatusListView {
            cursor: h as usize,
            scroll: top as usize,
            latched: latch.map(|l| l as usize),
        });
        let panel = StatusPanelView {
            name: &name,
            level: 7,
            xp: 0,
            xp_to_next: 0,
            hp: 0,
            hp_max: 0,
            hp_base: 0,
            mp: 0,
            mp_max: 0,
            mp_base: 0,
            ap: 0,
            ap_max: 100,
            stat_rows: &[],
            equip_rows: &[],
            status: 0,
            seru,
            page: 2,
            skills: &[],
            spells: &views,
            moves: &[],
            list,
            scroll: top as usize,
        };
        let port =
            status_screen_draws_for_with(&rig.font, Some(&pool), &panel, None, (r.x, r.y), true);
        let text: Vec<Ev> = log
            .iter()
            .filter(|e| matches!(e, Ev::Text(..) | Ev::Number(..) | Ev::Separator(..)))
            .cloned()
            .collect();
        let on_grid = |d: &[TextDraw]| -> Vec<_> {
            d.iter()
                .map(|t| (t.dst, t.src, t.color.map(|c| (c * 31.0).round() as u32)))
                .collect()
        };
        // Retail writes the preview from inside the row loop, when it
        // reaches the hovered row; the port after the rows. The two regions
        // do not overlap, so the draws are held as a set.
        let want = rig.text_of(&text);
        let (mut w, mut p) = (on_grid(&want), on_grid(&port));
        w.sort_by_key(|k| format!("{k:?}"));
        p.sort_by_key(|k| format!("{k:?}"));
        let first = w.iter().zip(&p).position(|(a, b)| a != b);
        assert_eq!(
            w,
            p,
            "{what}: first difference at {first:?}\nRETAIL {:?}\nPORT {:?}\n{text:02x?}",
            &w[first.unwrap_or(0).saturating_sub(2)..(first.unwrap_or(0) + 6).min(w.len())],
            &p[first.unwrap_or(0).saturating_sub(2)..(first.unwrap_or(0) + 6).min(p.len())]
        );

        // The hands: the list hand, and the latch's second one.
        let sprites = status_icon_sprites_for(
            &atlas,
            (r.x, r.y),
            0,
            StatusIconPage {
                page: 2,
                seru: seru.is_some(),
                hand_row: hand.map(|h| (h - top) as usize),
                latched_row: latch.map(|l| (l - top) as usize),
                ..Default::default()
            },
            (0, 0),
            1,
        );
        let mut got: Vec<(i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 == 0x900)
            .map(|s| (s.dst.0, s.dst.1))
            .collect();
        let mut want: Vec<(i32, i32)> = crate::menu_painters::hands(&log)
            .iter()
            .filter(|h| h.sprite == 0)
            .map(|h| (h.x, h.y))
            .collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{what}: hands");
        empty += u32::from(count == 0 || seru_id == 0);
        previewed += u32::from(hand.is_some());
        latched += u32::from(latch.is_some());
    }
    assert!(
        empty > 300 && previewed > 1000 && latched > 300 && halved > 1000,
        "{empty} {previewed} {latched} {halved}"
    );
}

/// The command-arrow writer `FUN_8003C310(dir, x, y)`.
const ARROW: u32 = 0x8003_C310;
/// The line renderer `FUN_80036888(str, 0, 0, x)`, `y` on the stack.
const TEXT: u32 = 0x8003_6888;

/// A string as the expander `FUN_80036514` walks it: a two-byte lead's
/// operand is read whatever it is, so a `0xC1 00` (the first member's name)
/// does not end the string.
fn token_str(m: &mut psx_retail_exec::Memory, mut a: u32) -> Vec<u8> {
    let mut s = Vec::new();
    loop {
        let b = m.rd8(a);
        if b == 0 {
            return s;
        }
        s.push(b);
        a += 1;
        if b & 0xF0 == 0xC0 || b == 0x5E || b == 0xFF {
            s.push(m.rd8(a));
            a += 1;
        }
    }
}

/// A line-renderer string as `FUN_80036888` lays it: expanded (`0xC1` to the
/// member's name), then written run by run, each `0xCF XX` restaging the ink
/// for what follows - the rule the `text_writer` differential holds.
fn line_draws(
    font: &legaia_font::Font,
    bytes: &[u8],
    (x, y): (i32, i32),
    ink0: u8,
    member: &str,
) -> (Vec<TextDraw>, u8) {
    let name = member.as_bytes().to_vec();
    let expand = move |op: u8, _: u8| (op == 0xC1).then(|| name.clone());
    let opts = legaia_font::MeasureOptions {
        expand: Some(&expand),
        ..Default::default()
    };
    let (expanded, _) = legaia_font::measure::expand_text(bytes, &opts, true);
    let (mut out, mut ink, mut pen, mut run) = (Vec::new(), ink0, x, Vec::new());
    let flush = |run: &mut Vec<u8>, pen: &mut i32, ink: u8, out: &mut Vec<TextDraw>| {
        let layout = font.layout(run);
        out.extend(legaia_engine_ui::text_draws_for(
            &layout,
            (*pen, y),
            legaia_engine_ui::menu_ink_color(ink),
        ));
        *pen += layout.advance_x as i32;
        run.clear();
    };
    let mut i = 0;
    while i < expanded.len() {
        if expanded[i] == 0xCF && i + 1 < expanded.len() {
            flush(&mut run, &mut pen, ink, &mut out);
            ink = expanded[i + 1];
            i += 2;
        } else {
            run.push(expanded[i]);
            i += 1;
        }
    }
    flush(&mut run, &mut pen, ink, &mut out);
    (out, ink)
}

/// The Moves page (`0x801D43CC..0x801D477C`): header, up to seven rows - the
/// art's name (its `0xCF` inks and the `0xC1` member name) and its AP, halved
/// by the record's `+0xF8` bit `0x800` - and on the hovered row the preview:
/// `Command:`, the command arrows (`FUN_8003C310`, `0xFF XX` markers
/// restaging the ink) and the description.
///
/// The description call widens width-table entry `0x80073F20` (byte `0x04`)
/// to `0x10` and puts back `0x0C`; no description on the disc carries the
/// byte, so the test does not model it.
#[test]
fn the_moves_page_matches() {
    differential!(prot(899, STATUS_PANEL));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus().unwrap();
    let pool = MenuPoolLabels::from_menu_overlay(
        &disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    let text = legaia_engine_menus::pause_screens::MenuTextTables::from_scus(&scus);
    let arts = text.arts.clone().expect("arts table");
    let descs = text.art_descriptions.clone().expect("art descriptions");
    // An ink-staged cell for every numeral and arrow, so a sprite names its
    // ink, glyph and seat.
    let mut cells = Vec::new();
    for staging in 0..10u8 {
        for n in 0..10u16 {
            cells.push(legaia_font::SpriteCellSrc {
                id: legaia_font::sprite_cell_ids::ink_numeral(staging, n),
                w: 8,
                h: 12,
                rgba: vec![0xFF; 8 * 12 * 4],
            });
        }
        for dir in 0..4u16 {
            cells.push(legaia_font::SpriteCellSrc {
                id: legaia_font::sprite_cell_ids::ink_arrow(staging, dir),
                w: 12,
                h: 12,
                rgba: vec![0xFF; 12 * 12 * 4],
            });
        }
    }
    rig.font = rig.font.clone().with_sprite_cells(&cells);
    let font_cells = rig.font.clone();
    let cell = move |id: u16| {
        let c = font_cells.sprite_cell(id).expect("a cell");
        (c.atlas_x, c.atlas_y, c.w, c.h)
    };
    let l = rig.log.clone();
    rig.cpu.unhook(LABEL);
    rig.cpu.hook(LABEL, move |r, m| {
        let ink = m.rd32(0x8007_B454) as u8;
        let s = cstr(m, r.r[psx_retail_exec::A0]);
        l.borrow_mut().push(Ev::Text(
            s,
            r.r[psx_retail_exec::A1] as i32,
            r.r[psx_retail_exec::A2] as i32,
            ink,
        ));
    });
    let l = rig.log.clone();
    rig.cpu.unhook(TEXT);
    rig.cpu.hook(TEXT, move |r, m| {
        let ink = m.rd32(0x8007_B454) as u8;
        let y = m.rd32(r.r[psx_retail_exec::SP] + 0x10) as i32;
        let s = token_str(m, r.r[psx_retail_exec::A0]);
        // The renderer leaves the last `0xCF` operand staged as the ink
        // (`sw v1,-0x4bac(v0)` at `0x80036958`); the AP after it reads it.
        let mut last = None;
        let mut i = 0;
        while i + 1 < s.len() {
            if s[i] == 0xCF || s[i] == 0xFF {
                last = Some(s[i + 1]);
            }
            i += if s[i] & 0xF0 == 0xC0 || s[i] == 0x5E || s[i] == 0xFF {
                2
            } else {
                1
            };
        }
        if let Some(ink) = last {
            m.wr32(0x8007_B454, u32::from(ink));
        }
        l.borrow_mut()
            .push(Ev::Text(s, r.r[psx_retail_exec::A3] as i32, y, ink));
    });
    let arrows: Calls<(u32, i32, i32, u8)> = Default::default();
    let a = arrows.clone();
    rig.cpu.hook(ARROW, move |r, m| {
        a.borrow_mut().push((
            r.r[psx_retail_exec::A0],
            r.r[psx_retail_exec::A1] as i32,
            r.r[psx_retail_exec::A2] as i32,
            m.rd32(0x8007_B454) as u8,
        ))
    });
    let tag = |n: u32| (n, 0, 1, 1);
    let atlas = SaveMenuAtlasRects {
        label_lv: tag(0x0A),
        cursor: tag(0x900),
        panel_tl: (0x700, 0, 4, 4),
        panel_tr: (0x704, 0, 4, 4),
        panel_bl: (0x708, 0, 4, 4),
        panel_br: (0x70C, 0, 4, 4),
        panel_top: (0x710, 0, 24, 4),
        panel_bot: (0x710, 4, 24, 4),
        panel_left: (0x730, 0, 4, 24),
        panel_right: (0x734, 0, 4, 24),
        panel_interior: (0x740, 0, 16, 16),
        ..Default::default()
    };
    let mut rng = Rng(0x4D4F_5645);
    let (mut empty, mut previewed, mut halved, mut inked, mut arrowed) = (0, 0, 0, 0, 0);
    for run in 0..3_000 {
        let id = rng.below(3);
        let seat = rng.below(3);
        rig.cpu.mem.wr8(ROSTER + seat, id as u8);
        rig.cpu.mem.wr32(LIST_CURSOR, seat);
        rig.cpu.mem.wr32(PAGE, [3u32, 8][rng.below(2) as usize]);
        let rec_va = RECORDS + id * STRIDE;
        for o in 0..STRIDE {
            rig.cpu.mem.wr8(rec_va + o, 0);
        }
        let name = format!("Hero{id}");
        rig.poke_str(rec_va + 0x2A7, &name);
        rig.cpu.mem.wr8(rec_va + 0x130, 7);
        let half = rng.below(3) == 0;
        rig.cpu
            .mem
            .wr32(rec_va + 0xF8, if half { 0x800 } else { 0 });
        // The member's own arts, by table position.
        let own: Vec<usize> = (0..arts.len())
            .filter(|&k| u32::from(arts[k].character as u8) == id)
            .collect();
        let count = match rng.below(6) {
            0 => 0,
            _ => 1 + rng.below(12),
        };
        let mut picks = Vec::new();
        for i in 0..count {
            let k = own[rng.below(own.len() as u32) as usize];
            rig.cpu.mem.wr8(rec_va + 0x186 + i, arts[k].index);
            picks.push(k);
        }
        rig.cpu.mem.wr8(rec_va + 0x185, count as u8);
        let top = if count > 7 { rng.below(count - 6) } else { 0 };
        let visible = (count - top).min(7);
        let hand = (count > 0 && rng.below(3) != 0).then(|| top + rng.below(visible));
        rig.cpu.mem.wr32(LIST_TOP, top);
        rig.cpu
            .mem
            .wr32(LIST_CURSOR_WORD, hand.unwrap_or(0x4000 | rng.below(0x1000)));
        let r = legaia_engine_ui::menu::window_painters::PainterRect::new(
            rng.below(100) as i32,
            rng.below(50) as i32,
            218,
            188,
        );
        arrows.borrow_mut().clear();
        let log = rig.run(STATUS_PANEL, r);
        let what = format!(
            "run {run} id {id} arts {picks:?} top {top} hand {hand:?} half {half} at {r:?}"
        );

        let views: Vec<legaia_engine_ui::StatusMoveRowView<'_>> = picks
            .iter()
            .map(|&k| legaia_engine_ui::StatusMoveRowView {
                name_bytes: &arts[k].name_bytes,
                ap: if half { arts[k].ap >> 1 } else { arts[k].ap },
                glyphs: &arts[k].glyphs,
                description: descs.get(k).map_or(&[][..], |d| d.as_slice()),
            })
            .collect();
        let list = hand.map(|h| legaia_engine_ui::StatusListView {
            cursor: h as usize,
            scroll: top as usize,
            latched: None,
        });
        let panel = StatusPanelView {
            name: &name,
            level: 7,
            xp: 0,
            xp_to_next: 0,
            hp: 0,
            hp_max: 0,
            hp_base: 0,
            mp: 0,
            mp_max: 0,
            mp_base: 0,
            ap: 0,
            ap_max: 100,
            stat_rows: &[],
            equip_rows: &[],
            status: 0,
            seru: None,
            page: 3,
            skills: &[],
            spells: &[],
            moves: &views,
            list,
            scroll: top as usize,
        };
        let port =
            status_screen_draws_for_with(&rig.font, Some(&pool), &panel, None, (r.x, r.y), true);
        // Retail's side: the line-renderer strings, the numerals, the arrows.
        let mut want: Vec<TextDraw> = Vec::new();
        for e in &log {
            match e {
                Ev::Text(s, x, y, ink) => {
                    let (d, last) = line_draws(&rig.font, s, (*x, *y), *ink, &name);
                    inked += u32::from(last != *ink);
                    want.extend(d);
                }
                Ev::Number(..) | Ev::Separator(..) => {
                    want.extend(rig.text_of(std::slice::from_ref(e)))
                }
                _ => {}
            }
        }
        for &(dir, x, y, ink) in arrows.borrow().iter() {
            let src = cell(legaia_font::sprite_cell_ids::ink_arrow(ink, dir as u16));
            want.push(TextDraw {
                dst: (x, y, src.2, src.3),
                src,
                color: [1.0; 4],
            });
            arrowed += 1;
        }
        let on_grid = |d: &[TextDraw]| -> Vec<_> {
            let mut v: Vec<_> = d
                .iter()
                .map(|t| (t.dst, t.src, t.color.map(|c| (c * 31.0).round() as u32)))
                .collect();
            v.sort_by_key(|k| format!("{k:?}"));
            v
        };
        let (w, p) = (on_grid(&want), on_grid(&port));
        let first = w.iter().zip(&p).position(|(a, b)| a != b);
        assert_eq!(
            w,
            p,
            "{what}: first difference at {first:?}\nRETAIL {:?}\nPORT {:?}\n{log:02x?}",
            &w[first.unwrap_or(0)..(first.unwrap_or(0) + 4).min(w.len())],
            &p[first.unwrap_or(0)..(first.unwrap_or(0) + 4).min(p.len())]
        );

        let sprites = status_icon_sprites_for(
            &atlas,
            (r.x, r.y),
            0,
            StatusIconPage {
                page: 3,
                hand_row: hand.map(|h| (h - top) as usize),
                ..Default::default()
            },
            (0, 0),
            1,
        );
        let got: Vec<(i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 == 0x900)
            .map(|s| (s.dst.0, s.dst.1))
            .collect();
        let want: Vec<(i32, i32)> = crate::menu_painters::hands(&log)
            .iter()
            .filter(|h| h.sprite == 0)
            .map(|h| (h.x, h.y))
            .collect();
        assert_eq!(got, want, "{what}: hands");
        empty += u32::from(count == 0);
        previewed += u32::from(hand.is_some());
        halved += u32::from(half && count > 0);
    }
    assert!(
        empty > 300 && previewed > 1000 && halved > 500 && arrowed > 3000,
        "{empty} {previewed} {halved} {arrowed} {inked}"
    );
}
