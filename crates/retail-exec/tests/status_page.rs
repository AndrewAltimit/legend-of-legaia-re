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
