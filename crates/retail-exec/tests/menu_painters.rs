//! The menu overlay's window content painters (PROT 0899) against
//! `engine-ui`'s `menu::window_painters`: the six title tabs, the record
//! tab, the two counters, the prompt and amount panels, the count panel, the
//! three choice / label panels, and the shop's item, buy, sell and
//! equip-target panels.
//!
//! The leaf draws are hooked and logged with the ink word `_DAT_8007B454`
//! each is called under: the string writer `FUN_80036888`, the number writers
//! `FUN_80034B78` / `FUN_80034E4C`, the separator glyph `FUN_8003C1F8`, the
//! hand `FUN_8002B994`, the pictogram `FUN_8002C488`, the frame
//! `FUN_8002C69C` and the two description writers. Each logged text draw is
//! laid out with the port's own primitives (the font, `decimal_field_draws`,
//! the numeral cells) in the ink `menu_ink_color` gives the logged word, and
//! the result is held to the painter's output in order; hands and pictograms
//! are compared as they are.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_ui::menu::decimal_field::decimal_field_draws;
use legaia_engine_ui::menu::window_painters::{
    BuyQuantityLabels, ChoiceFlags, DescriptionSource, EquipTargetRow, PainterPictogram,
    PainterRect, PainterSprite, amount_prompt_draws_for, buy_quantity_draws_for,
    char_prompt_draws_for, choice_panel_draws_for, count_panel_draws_for, counter_panel_draws_for,
    description_source, equip_row_enabled, equip_target_list_draws_for, item_description_draws_for,
    label_list_draws_for, record_title_tab_draws_for, sell_quantity_draws_for, title_tab_draws_for,
    two_line_choice_panel_draws_for,
};
use legaia_engine_ui::{
    OptionsPopupDraw, OptionsRowView, PauseMagicCaster, PauseMagicInfo, PauseMagicPhase,
    PauseMagicView, TextDraw, confirm_prompt_draws, confirm_prompt_hand_pos,
    magic_screen_draws_for_with, menu_ink_color, menu_numeral_cell, options_draws_for,
    options_popup_hand_pos, text_draws_for,
};
use legaia_font::Font;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Memory, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TABS: [u32; 6] = [
    0x801D_CA0C,
    0x801D_CA50,
    0x801D_CA94,
    0x801D_CAD8,
    0x801D_CB1C,
    0x801D_CFE4,
];
const TAB_0: u32 = 0x801D_CA0C;
const TAB_1: u32 = 0x801D_CA50;
const TAB_2: u32 = 0x801D_CA94;
const TAB_3: u32 = 0x801D_CAD8;
const TAB_4: u32 = 0x801D_CB1C;
const TAB_43: u32 = 0x801D_CFE4;
const RECORD_TAB: u32 = 0x801D_CF14;
const GOLD_PANEL: u32 = 0x801D_CF84;
const COIN_PANEL: u32 = 0x801D_D028;
const AMOUNT: u32 = 0x801D_CE20;
const COUNT: u32 = 0x801D_CC20;
const CHAR_PROMPT: u32 = 0x801D_CCB4;
const CHOICE_46: u32 = 0x801D_603C;
const CHOICE_5: u32 = 0x801D_61B0;
const LABELS_6: u32 = 0x801D_6360;
const ITEM_INFO: u32 = 0x801D_4A80;
const BUY_QTY: u32 = 0x801D_5510;
const SELL_QTY: u32 = 0x801D_5944;
const EQUIP_TARGET: u32 = 0x801D_56FC;
const CONFIRM_2: u32 = 0x801D_1DAC;
const CONFIRM_3: u32 = 0x801D_1F10;
const OPTIONS: u32 = 0x801D_CEF0;
const OPTIONS_ROWS: u32 = 0x801D_2910;
const OPTIONS_POPUP: u32 = 0x801D_2B44;
const MAGIC_CASTERS: u32 = 0x801D_2C98;
const PRIZE_ROWS: u32 = 0x801D_5DE0;
const MAGIC_INFO: u32 = 0x801D_2E74;
/// The Magic screen's caster cursor word.
const CASTER_CHOICE: u32 = 0x801E_46C8;
const SPELLS: u32 = 0x8007_54C8;
const SPELL_DESCS: u32 = 0x8007_5DB0;
/// The settings window's row layout (`id`, advance) and its descriptor list
/// (`[value word ptr][choices][ink][id][string index]`), and the string
/// pointer table both index.
const ROW_LAYOUT: u32 = 0x801E_4404;
const DESCRIPTORS: u32 = 0x801E_44B8;
const STRINGS: u32 = 0x801E_442C;

const TEXT: u32 = 0x8003_6888;
const NUMBER: u32 = 0x8003_4B78;
const NUMBER_ZERO: u32 = 0x8003_4E4C;
const SEPARATOR: u32 = 0x8003_C1F8;
const HAND: u32 = 0x8002_B994;
const PICTO: u32 = 0x8002_C488;
const FRAME: u32 = 0x8002_C69C;
const LABEL: u32 = 0x8003_CD00;
const DETAIL: u32 = 0x8003_37B0;
const BAG_SLOT: u32 = 0x8004_2EE0;
const BAG_COUNT: u32 = 0x8004_2F4C;
const ITEM_PANEL: u32 = 0x801D_0F1C;
const PANEL_MODE: u32 = 0x8003_4B6C;

const INK: u32 = 0x8007_B454;
const ADVANCE: u32 = 0x8007_3F20;
const WINDOW: u32 = 0x8012_0000;
const SCRATCH: u32 = 0x8013_0000;
const ITEMS: u32 = 0x8007_4368;
const SELECTED: u32 = 0x801E_46B0;
const QUANTITY: u32 = 0x801E_46B4;
const BOUND: u32 = 0x801E_46B8;
const CHOICE: u32 = 0x801E_46D0;
const TARGET_CHOICE: u32 = 0x801E_46C0;
/// The bag's count bytes, `0x80084140 + 0x1819 + slot * 2`.
const BAG_COUNTS: u32 = 0x8008_5959;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ev {
    Text(Vec<u8>, i32, i32, u8),
    Number(i32, i32, i32, i32, u8, bool),
    Separator(u8, i32, i32, u8),
    Hand(PainterSprite),
    Picto(PainterPictogram),
    Frame(i32, i32, i32, i32),
    Label(u32, i32, i32, u8),
    Detail(u32, u32, i32, i32, u8),
    Call(u32),
}

pub(crate) fn cstr(m: &mut Memory, mut a: u32) -> Vec<u8> {
    let mut s = Vec::new();
    loop {
        let b = m.rd8(a);
        if b == 0 {
            return s;
        }
        s.push(b);
        a += 1;
    }
}

pub(crate) struct Rig {
    pub(crate) cpu: Cpu,
    pub(crate) font: Font,
    pub(crate) log: Rc<RefCell<Vec<Ev>>>,
    /// What the bag hooks answer: `(slot or 0x100, count)`.
    pub(crate) bag: Rc<RefCell<(u32, u32)>>,
}

pub(crate) fn rig() -> Option<Rig> {
    let (mut cpu, d, scus) = (scus_cpu()?, disc()?, scus()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let tim = d
        .read_prot_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .expect("font TIM");
    let head = d
        .read_prot_bytes(
            legaia_font::escape_icons::ICON_PROT_DAT_OFFSET,
            legaia_font::escape_icons::ICON_PROT_DAT_LEN,
        )
        .expect("escape icons");
    let font = Font::from_disc_tim_and_scus(&tim, &scus)
        .expect("font")
        .with_escape_icons_from_disc(&head, &scus);
    assert!(font.escape_cell(0x14).is_some(), "the element plates");
    let log: Rc<RefCell<Vec<Ev>>> = Rc::default();
    let ink = |m: &mut Memory| m.rd32(INK) as u8;
    let l = log.clone();
    cpu.hook(TEXT, move |r, m| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0));
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        let s = cstr(m, r.r[A0]);
        let i = ink(m);
        l.borrow_mut().push(Ev::Text(s, r.r[A3] as i32, y, i));
    });
    for (entry, zero) in [(NUMBER, false), (NUMBER_ZERO, true)] {
        let l = log.clone();
        cpu.hook(entry, move |r, m| {
            let i = ink(m);
            l.borrow_mut().push(Ev::Number(
                r.r[A0] as i32,
                r.r[A1] as i32,
                r.r[A2] as i32,
                r.r[A3] as i32,
                i,
                zero,
            ));
        });
    }
    let l = log.clone();
    cpu.hook(SEPARATOR, move |r, m| {
        let i = ink(m);
        l.borrow_mut().push(Ev::Separator(
            r.r[A0] as u8,
            r.r[A1] as i32,
            r.r[A2] as i32,
            i,
        ));
    });
    let l = log.clone();
    cpu.hook(HAND, move |r, _| {
        l.borrow_mut().push(Ev::Hand(PainterSprite {
            sprite: r.r[A0] as u8,
            variant: r.r[A1] as u8,
            x: r.r[A2] as i32,
            y: r.r[A3] as i32,
        }))
    });
    let l = log.clone();
    cpu.hook(PICTO, move |r, _| {
        l.borrow_mut().push(Ev::Picto(PainterPictogram {
            id: r.r[A2] as u8,
            x: r.r[A0] as i32,
            y: r.r[A1] as i32,
        }))
    });
    let l = log.clone();
    cpu.hook(FRAME, move |r, _| {
        l.borrow_mut().push(Ev::Frame(
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3] as i32,
        ))
    });
    let l = log.clone();
    cpu.hook(LABEL, move |r, m| {
        let i = ink(m);
        l.borrow_mut()
            .push(Ev::Label(r.r[A0], r.r[A1] as i32, r.r[A2] as i32, i));
    });
    let l = log.clone();
    cpu.hook(DETAIL, move |r, m| {
        let i = ink(m);
        l.borrow_mut().push(Ev::Detail(
            r.r[A0],
            r.r[A1],
            r.r[A2] as i32,
            r.r[A3] as i32,
            i,
        ));
    });
    for entry in [ITEM_PANEL, PANEL_MODE] {
        let l = log.clone();
        cpu.hook(entry, move |_, _| l.borrow_mut().push(Ev::Call(entry)));
    }
    let bag: Rc<RefCell<(u32, u32)>> = Rc::default();
    let b = bag.clone();
    cpu.hook(BAG_SLOT, move |r, _| r.r[V0] = b.borrow().0);
    let b = bag.clone();
    cpu.hook(BAG_COUNT, move |r, _| r.r[V0] = b.borrow().1);
    Some(Rig {
        cpu,
        font,
        log,
        bag,
    })
}

impl Rig {
    /// Run `entry` on a window record at `rect`; the log it left.
    pub(crate) fn run(&mut self, entry: u32, rect: PainterRect) -> Vec<Ev> {
        self.run_with(entry, rect, &[])
    }

    /// [`Rig::run`] with arguments after the window record.
    pub(crate) fn run_with(&mut self, entry: u32, rect: PainterRect, more: &[u32]) -> Vec<Ev> {
        for (o, v) in [(0xA, rect.x), (0xC, rect.y), (0xE, rect.w), (0x10, rect.h)] {
            self.cpu.mem.wr16(WINDOW + o, v as u16);
        }
        self.cpu.mem.wr32(INK, 0x33);
        self.log.borrow_mut().clear();
        let args: Vec<u32> = [WINDOW].iter().chain(more).copied().collect();
        assert_eq!(self.cpu.call(entry, &args), Stop::Returned, "{entry:#x}");
        self.log.borrow().clone()
    }

    /// Lay the logged text draws out with the port's primitives.
    pub(crate) fn text_of(&self, log: &[Ev]) -> Vec<TextDraw> {
        let mut out = Vec::new();
        for e in log {
            match e {
                Ev::Text(s, x, y, ink) => out.extend(text_draws_for(
                    &self.font.layout(s),
                    (*x, *y),
                    menu_ink_color(*ink),
                )),
                Ev::Number(v, digits, x, y, ink, zero) => out.extend(decimal_field_draws(
                    &self.font,
                    i64::from(*v),
                    (*x, *y),
                    *digits,
                    *zero,
                    menu_ink_color(*ink),
                )),
                Ev::Separator(g, x, y, ink) => {
                    let c = menu_ink_color(*ink);
                    if let Some((cell, tint)) =
                        menu_numeral_cell(&self.font, true, u16::from(*g), c)
                    {
                        out.push(TextDraw {
                            dst: (*x, *y, cell.w, cell.h),
                            src: (cell.atlas_x, cell.atlas_y, cell.w, cell.h),
                            color: tint,
                        });
                    } else {
                        let ch = legaia_engine_ui::menu::window_painters::separator_glyph_char(*g)
                            .expect("a text separator");
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&ch.to_string()),
                            (*x, *y),
                            c,
                        ));
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub(crate) fn string(&mut self, va: u32) -> String {
        String::from_utf8(cstr(&mut self.cpu.mem, va)).expect("ASCII")
    }

    pub(crate) fn poke_str(&mut self, va: u32, s: &str) {
        for (i, b) in s.bytes().chain([0]).enumerate() {
            self.cpu.mem.wr8(va + i as u32, b);
        }
    }
}

pub(crate) type Key = ((i32, i32, u32, u32), (u32, u32, u32, u32), [u32; 4]);

pub(crate) fn keys(d: &[TextDraw]) -> Vec<Key> {
    d.iter()
        .map(|t| (t.dst, t.src, t.color.map(f32::to_bits)))
        .collect()
}

pub(crate) fn hands(log: &[Ev]) -> Vec<PainterSprite> {
    log.iter()
        .filter_map(|e| match e {
            Ev::Hand(h) => Some(*h),
            _ => None,
        })
        .collect()
}

pub(crate) fn pictos(log: &[Ev]) -> Vec<PainterPictogram> {
    log.iter()
        .filter_map(|e| match e {
            Ev::Picto(p) => Some(*p),
            _ => None,
        })
        .collect()
}

pub(crate) fn same_text(rig: &Rig, log: &[Ev], port: &[TextDraw], what: &str) {
    let want = rig.text_of(log);
    assert!(!want.is_empty() || port.is_empty(), "{what}: nothing drawn");
    let (w, p) = (keys(&want), keys(port));
    let first = w.iter().zip(&p).position(|(a, b)| a != b);
    assert_eq!(
        w,
        p,
        "{what}: first difference at {first:?} of {} / {}\n{log:?}\nTAIL {:?}\nPORT {:?}",
        w.len(),
        p.len(),
        &w[first.unwrap_or(0)..],
        &p[first.unwrap_or(0)..]
    );
}

pub(crate) fn rect(rng: &mut Rng) -> PainterRect {
    PainterRect::new(
        rng.below(0x110) as i32,
        rng.below(0xB0) as i32,
        0x20 + rng.below(0x100) as i32,
        0x10 + rng.below(0x60) as i32,
    )
}

pub(crate) fn flags(rng: &mut Rng, rows: u32) -> u32 {
    match rng.below(5) {
        0 => 0x4000 | rng.below(0x4000),
        1 => 0x2000 | (rng.below(2) << 12) | rng.below(0x1000),
        2 => rng.below(rows + 1) | (rng.below(2) << 12),
        3 => rng.below(rows + 1),
        _ => rng.next() & 0x7FFF,
    }
}

#[test]
fn the_title_tabs_match() {
    differential!(
        prot(899, TAB_0),
        prot(899, TAB_1),
        prot(899, TAB_2),
        prot(899, TAB_3),
        prot(899, TAB_4),
        prot(899, TAB_43),
        prot(899, RECORD_TAB)
    );
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5441_4253);
    for run in 0..2_000 {
        let r = rect(&mut rng);
        for entry in TABS {
            let log = rig.run(entry, r);
            let Ev::Text(s, ..) = &log[0] else {
                panic!("{log:?}")
            };
            let label = String::from_utf8(s.clone()).expect("ASCII");
            let port = title_tab_draws_for(&rig.font, r, &label);
            same_text(&rig, &log, &port, &format!("run {run} {entry:#x}"));
            assert!(hands(&log).is_empty() && log.len() == 1);
        }
        // The record tab: the string past the record's skip count.
        let skip = rng.below(6) as u8;
        rig.cpu.mem.wr32(0x8007_B450, SCRATCH);
        rig.cpu.mem.wr8(SCRATCH + 2, skip);
        let name = ["@Rim Elm shop", "Bottle Shop", "x", "Weapons & Armor"][rng.below(4) as usize];
        rig.poke_str(SCRATCH + 3 + u32::from(skip), name);
        let advance = rng.below(0x20) as u8;
        rig.cpu.mem.wr8(ADVANCE, advance);
        let log = rig.run(RECORD_TAB, r);
        let port = record_title_tab_draws_for(&rig.font, r, name);
        same_text(&rig, &log, &port, &format!("run {run} record tab"));
        assert_eq!(
            rig.cpu.mem.rd8(ADVANCE),
            advance,
            "the advance byte restored"
        );
    }
}

#[test]
fn the_counters_and_the_amount_panel_match() {
    differential!(
        prot(899, GOLD_PANEL),
        prot(899, COIN_PANEL),
        prot(899, AMOUNT)
    );
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The heading's `0xC2 0xFE` item token is the host's to expand.
    rig.poke_str(0x801C_EA40, "@Point Card has earned");
    let heading = rig.string(0x801C_EA40);
    let unit = rig.string(0x801C_EA50);
    let mut rng = Rng(0x434E_5452);
    for run in 0..4_000 {
        let r = rect(&mut rng);
        let value = [
            0,
            rng.below(1000),
            rng.below(10_000_000),
            rng.below(0x7FFF_FFFF),
        ][rng.below(4) as usize];
        for (entry, cell, pictogram) in [
            (GOLD_PANEL, 0x8008_459C, 0x62),
            (COIN_PANEL, 0x8008_45A4, 0x66),
        ] {
            rig.cpu.mem.wr32(cell, value);
            let log = rig.run(entry, r);
            let (text, pic) = counter_panel_draws_for(&rig.font, r, pictogram, u64::from(value));
            same_text(&rig, &log, &text, &format!("run {run} {entry:#x} {value}"));
            assert_eq!(pictos(&log), vec![pic], "run {run}");
        }
        rig.cpu.mem.wr32(0x8008_45B4, value);
        let log = rig.run(AMOUNT, r);
        let (text, hand) = amount_prompt_draws_for(&rig.font, r, &heading, u64::from(value), &unit);
        same_text(&rig, &log, &text, &format!("run {run} amount {value}"));
        assert_eq!(hands(&log), vec![hand], "run {run}");
    }
}

#[test]
fn the_count_and_char_prompts_match() {
    differential!(prot(899, COUNT), prot(899, CHAR_PROMPT));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The template's spell token is the host's to expand; the byte the
    // routine patches is its `+1`.
    rig.poke_str(0x801E_46E4, "@? learned a spell level");
    let mut rng = Rng(0x434F_554E);
    let mut counted = 0;
    for run in 0..4_000 {
        let r = rect(&mut rng);
        let item = [0i32, -1, 1 + rng.below(255) as i32][rng.below(3) as usize];
        let count = rng.below(100);
        rig.cpu.mem.wr32(SELECTED, item as u32);
        rig.bag.borrow_mut().1 = count;
        let log = rig.run(COUNT, r);
        let selection = (item > 0).then_some(u64::from(count));
        let (text, frame) = count_panel_draws_for(&rig.font, r, selection);
        same_text(&rig, &log, &text, &format!("run {run} count {item}"));
        assert_eq!(
            log.last(),
            Some(&Ev::Frame(frame.0, frame.1, frame.2, frame.3))
        );
        assert_eq!(
            log.contains(&Ev::Call(ITEM_PANEL)),
            item > 0,
            "run {run}: the item panel under it"
        );
        counted += u32::from(item > 0);
        // Window 7: one patched byte off the slot's learned-spell list.
        let (slot, index) = (rng.below(3), rng.below(8));
        let byte = b'A' + rng.below(26) as u8;
        rig.cpu.mem.wr16(0x8007_BB70, slot as u16);
        rig.cpu.mem.wr16(0x8007_BB78, index as u16);
        rig.cpu
            .mem
            .wr8(0x8008_4140 + slot * 0x414 + index + 0x705, byte);
        let log = rig.run(CHAR_PROMPT, r);
        let line = cstr(&mut rig.cpu.mem, 0x801E_46E4);
        assert_eq!(line[1], byte, "run {run}: the patched byte");
        let (text, hand) = char_prompt_draws_for(&rig.font, r, &line);
        same_text(&rig, &log, &text, &format!("run {run} char prompt"));
        assert_eq!(hands(&log), vec![hand], "run {run}");
    }
    assert!(counted > 1000, "{counted}");
}

#[test]
fn the_choice_and_label_panels_match() {
    differential!(
        prot(899, CHOICE_46),
        prot(899, CHOICE_5),
        prot(899, LABELS_6)
    );
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The last label is a lone `0xC1` name token, the host's to expand.
    rig.poke_str(0x801C_ED58, "@Vahn");
    let heading = rig.string(0x801C_EAC8);
    let yes = rig.string(0x801C_EA84);
    let no = rig.string(0x801C_EA8C);
    let headings = [rig.string(0x801C_EC78), rig.string(0x801C_EC94)];
    let labels: Vec<String> = [
        0x801C_ECA8,
        0x801C_ECD4,
        0x801C_ECFC,
        0x801C_ED20,
        0x801C_ED38,
        0x801C_ED58,
    ]
    .into_iter()
    .map(|va| rig.string(va))
    .collect();
    let mut rng = Rng(0x4348_4F49);
    let mut marked = 0;
    for run in 0..4_000 {
        let r = rect(&mut rng);
        let f = flags(&mut rng, 1);
        rig.cpu.mem.wr32(CHOICE, f);
        let log = rig.run(CHOICE_46, r);
        let (text, marks) =
            choice_panel_draws_for(&rig.font, r, &heading, [&yes, &no], ChoiceFlags(f));
        let what = format!("run {run} 46 flags {f:#x}");
        same_text(&rig, &log, &text, &what);
        assert_eq!(hands(&log), marks, "{what}");
        assert_eq!(rig.cpu.mem.rd32(INK), 7, "{what}: the ink restored");
        marked += marks.len() as u32;
        let log = rig.run(CHOICE_5, r);
        let (text, marks) = two_line_choice_panel_draws_for(
            &rig.font,
            r,
            [&headings[0], &headings[1]],
            [&yes, &no],
            ChoiceFlags(f),
        );
        let what = format!("run {run} 5 flags {f:#x}");
        same_text(&rig, &log, &text, &what);
        assert_eq!(hands(&log), marks, "{what}");
        let log = rig.run(LABELS_6, r);
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let (text, hand) = label_list_draws_for(&rig.font, r, &refs);
        same_text(&rig, &log, &text, &format!("run {run} 6"));
        assert_eq!(hands(&log), vec![hand], "run {run} 6");
    }
    assert!(marked > 2000, "{marked}");
}

#[test]
fn the_shop_panels_match() {
    differential!(
        prot(899, ITEM_INFO),
        prot(899, BUY_QTY),
        prot(899, SELL_QTY),
        prot(899, EQUIP_TARGET)
    );
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The row labels open on a `0xC1` name token, the host's to expand.
    rig.poke_str(0x801C_EC08, "@Vahn will equip");
    rig.poke_str(0x801C_EC18, "@Noa will equip");
    rig.poke_str(0x801C_EC28, "@Gala will equip");
    let row_labels = [
        rig.string(0x801C_EC08),
        rig.string(0x801C_EC18),
        rig.string(0x801C_EC28),
    ];
    let header = rig.string(0x801C_EBF8);
    let labels_owned = [
        rig.string(0x801C_EBC8),
        rig.string(0x801C_EBD0),
        rig.string(0x801C_EBD4),
        rig.string(0x801C_EBDC),
    ];
    let buy_labels = BuyQuantityLabels {
        have_head: &labels_owned[0],
        have_tail: &labels_owned[1],
        none_held: &labels_owned[2],
        prompt: &labels_owned[3],
    };
    let sell_heading = rig.string(0x801C_EC38);
    // Names the string writer would expand get an ASCII stand-in.
    let stand_in = SCRATCH + 0x100;
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
    let mut rng = Rng(0x5348_4F50);
    let (mut passives, mut details, mut dimmed) = (0, 0, 0);
    for run in 0..6_000 {
        let r = rect(&mut rng);
        let item = [
            0i32,
            -3,
            1 + rng.below(255) as i32,
            1 + rng.below(255) as i32,
        ][rng.below(4) as usize];
        let held = rng.below(3) != 0;
        let slot = rng.below(0x100);
        let count = rng.below(100) as u8;
        *rig.bag.borrow_mut() = (if held { slot } else { 0x100 }, 0);
        rig.cpu.mem.wr8(BAG_COUNTS + slot * 2, count);
        let (qty, bound) = (rng.below(100), rng.below(100));
        rig.cpu.mem.wr32(SELECTED, item as u32);
        rig.cpu.mem.wr32(QUANTITY, qty);
        rig.cpu.mem.wr32(BOUND, bound);
        let id = item as u32;
        let rec = ITEMS.wrapping_add(id.wrapping_mul(12));
        let (kind, sub) = (rig.cpu.mem.rd8(rec), rig.cpu.mem.rd8(rec + 1));
        let price = u32::from(rig.cpu.mem.rd16(rec + 2));
        let name_ptr = rig.cpu.mem.rd32(rec + 4);
        let what = format!("run {run} item {item}");

        // Window 34: name and owned count, then one description writer.
        let log = rig.run(ITEM_INFO, r);
        let name = if item > 0 {
            rig.string(name_ptr)
        } else {
            String::new()
        };
        let owned = if held { count } else { 0 };
        let text = item_description_draws_for(&rig.font, r, item > 0, &name, owned, "");
        let head: Vec<Ev> = log
            .iter()
            .filter(|e| !matches!(e, Ev::Label(..) | Ev::Detail(..)))
            .cloned()
            .collect();
        same_text(&rig, &head, &text, &format!("{what} 34"));
        let desc: Vec<&Ev> = log
            .iter()
            .filter(|e| matches!(e, Ev::Label(..) | Ev::Detail(..)))
            .collect();
        if item > 0 {
            let passive = rig.cpu.mem.rd8(0x8007_52C0 + u32::from(sub) * 4 + 3);
            let pen = (r.x + 8, r.y + 0xE);
            match description_source(kind, passive) {
                DescriptionSource::AccessoryPassive(p) => {
                    let va = rig.cpu.mem.rd32(0x8007_625C + u32::from(p) * 12 + 8);
                    assert_eq!(desc, vec![&Ev::Label(va, pen.0, pen.1, 7)], "{what}");
                    passives += 1;
                }
                DescriptionSource::Item => {
                    let va = rig.cpu.mem.rd32(rec + 8);
                    assert_eq!(desc, vec![&Ev::Detail(va, id, pen.0, pen.1, 7)], "{what}");
                    details += 1;
                }
            }
        } else {
            assert!(log.is_empty(), "{what}: {log:?}");
        }

        // Window 35: the buy stepper.
        let log = rig.run(BUY_QTY, r);
        let (text, pic, hand) = buy_quantity_draws_for(
            &rig.font,
            r,
            buy_labels,
            held.then_some(u32::from(count)),
            qty,
            bound,
            price,
        );
        same_text(&rig, &log, &text, &format!("{what} 35"));
        assert_eq!(
            pictos(&log),
            pic.into_iter().collect::<Vec<_>>(),
            "{what} 35"
        );
        assert_eq!(
            hands(&log),
            hand.into_iter().collect::<Vec<_>>(),
            "{what} 35"
        );

        // Window 37: the sell stepper.
        let log = rig.run(SELL_QTY, r);
        let (text, pic, hand) =
            sell_quantity_draws_for(&rig.font, r, item > 0, &sell_heading, qty, bound, price);
        same_text(&rig, &log, &text, &format!("{what} 37"));
        assert_eq!(
            pictos(&log),
            pic.into_iter().collect::<Vec<_>>(),
            "{what} 37"
        );
        assert_eq!(
            hands(&log),
            hand.into_iter().collect::<Vec<_>>(),
            "{what} 37"
        );

        // Window 36: the equip-target list over the present party.
        let n = rng.below(5);
        let members: Vec<u8> = (0..n)
            .map(|_| [0u8, 1, 2, 0, 1, 2, 3][rng.below(7) as usize])
            .collect();
        rig.cpu.mem.wr8(0x8008_4594, n as u8);
        for (i, m) in members.iter().enumerate() {
            rig.cpu.mem.wr8(0x8008_4598 + i as u32, *m);
        }
        let f = flags(&mut rng, n);
        rig.cpu.mem.wr32(TARGET_CHOICE, f);
        let mask = rig.cpu.mem.rd8(0x8007_4F68 + u32::from(sub) * 8 + 6);
        let rows: Vec<(EquipTargetRow, &str)> = members
            .iter()
            .map(|&m| {
                (
                    EquipTargetRow {
                        member_class: m,
                        equippable: equip_row_enabled(mask, m),
                    },
                    row_labels.get(usize::from(m)).map_or("", String::as_str),
                )
            })
            .collect();
        let log = rig.run(EQUIP_TARGET, r);
        let (text, marks) =
            equip_target_list_draws_for(&rig.font, r, &header, &rows, ChoiceFlags(f));
        same_text(&rig, &log, &text, &format!("{what} 36 {members:?} {f:#x}"));
        assert_eq!(hands(&log), marks, "{what} 36 {members:?} {f:#x}");
        dimmed += rows.iter().filter(|(r, _)| !r.equippable).count() as u32;
    }
    assert!(
        passives > 200 && details > 1000 && dimmed > 1000,
        "{passives} {details} {dimmed}"
    );
}

#[test]
fn the_confirm_prompts_match() {
    differential!(prot(899, CONFIRM_2), prot(899, CONFIRM_3));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The 3-line prompt's item token is the host's to expand.
    rig.poke_str(0x801C_EAB4, "@Will use the Door of Wind.");
    let one = rig.string(0x801C_EA94);
    let two = [rig.string(0x801C_EAB4), rig.string(0x801C_EAC8)];
    let options = [rig.string(0x801C_EA84), rig.string(0x801C_EA8C)];
    let opts: Vec<&str> = options.iter().map(String::as_str).collect();
    let mut rng = Rng(0x434F_4E46);
    for run in 0..4_000 {
        let r = rect(&mut rng);
        let f = flags(&mut rng, 1);
        rig.cpu.mem.wr32(CHOICE, f);
        for (entry, prompt) in [
            (CONFIRM_2, vec![one.as_str()]),
            (CONFIRM_3, vec![&two[0], &two[1]]),
        ] {
            let what = format!("run {run} {entry:#x} flags {f:#x}");
            let log = rig.run(entry, r);
            let text = confirm_prompt_draws(&rig.font, &prompt, &opts, (r.x, r.y));
            same_text(&rig, &log, &text, &what);
            let want: Vec<(i32, i32)> = (0..2u8)
                .filter(|&row| ChoiceFlags(f).marker_variant(u32::from(row)).is_some())
                .map(|row| confirm_prompt_hand_pos((r.x, r.y), prompt.len(), row))
                .collect();
            let got: Vec<(i32, i32)> = hands(&log).iter().map(|h| (h.x, h.y)).collect();
            assert_eq!(got, want, "{what}");
            assert_eq!(rig.cpu.mem.rd32(INK), 7, "{what}: the ink restored");
        }
    }
}

/// One settings row as the descriptor list describes it.
struct OptionRow {
    label: String,
    value: Option<String>,
    teal: bool,
    advance: i32,
    choices: Vec<String>,
}

fn option_rows(rig: &mut Rig) -> Vec<OptionRow> {
    let mut rows = Vec::new();
    for i in 0..10u32 {
        let id = rig.cpu.mem.rd16(ROW_LAYOUT + i * 4);
        let advance = rig.cpu.mem.rd16(ROW_LAYOUT + i * 4 + 2) as i16 as i32;
        let mut d = DESCRIPTORS;
        while rig.cpu.mem.rd32(d) != 0 && u16::from(rig.cpu.mem.rd8(d + 6)) != id {
            d += 8;
        }
        assert_ne!(rig.cpu.mem.rd32(d), 0, "row {i}: id {id}");
        let n = u32::from(rig.cpu.mem.rd8(d + 4));
        let s = u32::from(rig.cpu.mem.rd8(d + 7));
        let text = |rig: &mut Rig, k: u32| {
            let va = rig.cpu.mem.rd32(STRINGS + k * 4);
            rig.string(va)
        };
        let choices: Vec<String> = (0..n).map(|k| text(rig, s + 1 + k)).collect();
        let value = (n > 0).then(|| {
            let word = rig.cpu.mem.rd32(d);
            let v = rig.cpu.mem.rd32(word);
            text(rig, s + 1 + v)
        });
        rows.push(OptionRow {
            label: text(rig, s),
            value,
            teal: rig.cpu.mem.rd8(d + 5) == 5,
            advance,
            choices,
        });
    }
    rows
}

#[test]
fn the_options_window_and_its_popup_match() {
    differential!(
        prot(899, OPTIONS),
        prot(899, OPTIONS_ROWS),
        prot(899, OPTIONS_POPUP)
    );
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The `0xCE 0x00` button icon is the host's to resolve.
    rig.poke_str(0x801C_E8E4, "O button");
    let mut rng = Rng(0x4F50_5453);
    let (mut edits, mut greys) = (0, 0);
    for run in 0..3_000 {
        let r = rect(&mut rng);
        // Each row's value word, inside its choice count.
        let mut d = DESCRIPTORS;
        while rig.cpu.mem.rd32(d) != 0 {
            let n = u32::from(rig.cpu.mem.rd8(d + 4));
            if n > 0 {
                let word = rig.cpu.mem.rd32(d);
                rig.cpu.mem.wr32(word, rng.below(n));
            }
            d += 8;
        }
        let cursor = rng.below(10);
        let edit = rng.below(2) == 1;
        let f = cursor | if edit { 0x1000 } else { 0 };
        rig.cpu.mem.wr32(TARGET_CHOICE, f);
        let rows = option_rows(&mut rig);
        let views: Vec<OptionsRowView<'_>> = rows
            .iter()
            .map(|o| OptionsRowView {
                label: &o.label,
                value: o.value.as_deref(),
                teal: o.teal,
                advance: o.advance,
            })
            .collect();
        // A popup off the stage stands for "editing" without covering a row.
        let away = OptionsPopupDraw {
            rect: (0x4000, 0x4000, 0, 0),
            choices: &[],
            cursor: 0,
        };
        let what = format!("run {run} flags {f:#x}");
        // The window renderer is `FUN_801D2910(win, 0, 9)`; call either.
        let log = if run % 2 == 0 {
            rig.run(OPTIONS, r)
        } else {
            rig.run_with(OPTIONS_ROWS, r, &[0, 9])
        };
        let text = options_draws_for(
            &rig.font,
            &views,
            cursor as u8,
            edit.then_some(&away),
            (r.x, r.y),
        );
        same_text(&rig, &log, &text, &what);
        edits += u32::from(edit);
        greys += log
            .iter()
            .filter(|e| matches!(e, Ev::Text(_, _, _, 0)))
            .count() as u32;
        // The popup over the cursor row.
        let choice = rng.below(4);
        rig.cpu.mem.wr32(CHOICE, choice);
        let log = rig.run(OPTIONS_POPUP, r);
        let row = &rows[cursor as usize];
        let choices: Vec<&str> = row.choices.iter().map(String::as_str).collect();
        let popup = OptionsPopupDraw {
            rect: (r.x, r.y, r.w, r.h),
            choices: &choices,
            cursor: choice as u8,
        };
        let text = options_draws_for(&rig.font, &[], 0, Some(&popup), (r.x, r.y));
        let labels: Vec<Ev> = log
            .iter()
            .map(|e| match e {
                Ev::Label(va, x, y, ink) => Ev::Text(cstr(&mut rig.cpu.mem, *va), *x, *y, *ink),
                e => e.clone(),
            })
            .collect();
        same_text(&rig, &labels, &text, &format!("{what} popup {choice}"));
        let got: Vec<(i32, i32)> = hands(&log).iter().map(|h| (h.x, h.y)).collect();
        let want: Vec<(i32, i32)> = ((choice as usize) < choices.len())
            .then(|| options_popup_hand_pos(&popup))
            .into_iter()
            .collect();
        assert_eq!(got, want, "{what} popup {choice}");
    }
    assert!(edits > 1000 && greys > 5000, "{edits} {greys}");
}

fn magic_view<'a>(
    casters: &'a [PauseMagicCaster<'a>],
    info: Option<PauseMagicInfo<'a>>,
) -> PauseMagicView<'a> {
    PauseMagicView {
        arrows_blink: false,
        casters,
        rows: &[],
        page: 0,
        pages: 0,
        phase: PauseMagicPhase::Caster,
        caster_cursor: 0,
        list_cursor: 0,
        info,
        label_icons: true,
        text_cursor: false,
    }
}

#[test]
fn the_magic_casters_and_info_match() {
    differential!(prot(899, MAGIC_CASTERS), prot(899, MAGIC_INFO));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pool = legaia_game_tables::menu_pool::MenuPoolLabels::from_menu_overlay(
        &disc()
            .unwrap()
            .read_entry_footprint(899)
            .expect("PROT 0899"),
    );
    let mut rng = Rng(0x4D41_4749);
    let mut levels = 0;
    for run in 0..3_000 {
        let r = rect(&mut rng);
        // The present party, every member a caster (ids below 3).
        let n = 1 + rng.below(3);
        let ids: Vec<u32> = (0..n).map(|_| rng.below(3)).collect();
        rig.cpu.mem.wr8(0x8008_4594, n as u8);
        let mut names = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            rig.cpu.mem.wr8(0x8008_4598 + i as u32, id as u8);
            let rec = 0x8008_4708 + id * 0x414;
            let max = rng.below(1000) as u16;
            rig.cpu.mem.wr16(rec + 0x108, max);
            rig.cpu
                .mem
                .wr16(rec + 0x10A, rng.below(u32::from(max) + 1) as u16);
            rig.cpu.mem.wr8(rec + 0x130, 1 + rng.below(99) as u8);
            let name = format!("@C{id}{}", rng.below(10));
            rig.poke_str(rec + 0x2A7, &name);
            names.push(name);
        }
        let f = flags(&mut rng, n - 1);
        rig.cpu.mem.wr32(CASTER_CHOICE, f);
        let log = rig.run(MAGIC_CASTERS, r);
        let casters: Vec<PauseMagicCaster<'_>> = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let rec = 0x8008_4708 + id * 0x414;
                PauseMagicCaster {
                    name: &names[i],
                    level: u16::from(rig.cpu.mem.rd8(rec + 0x130)),
                    mp: rig.cpu.mem.rd16(rec + 0x10A),
                    mp_max: rig.cpu.mem.rd16(rec + 0x108),
                }
            })
            .collect();
        // A member seated twice shows its last name in both blocks.
        let casters: Vec<PauseMagicCaster<'_>> = casters
            .iter()
            .enumerate()
            .map(|(i, c)| PauseMagicCaster {
                name: &names[ids.iter().rposition(|&x| x == ids[i]).unwrap()],
                ..*c
            })
            .collect();
        let text = magic_screen_draws_for_with(
            &rig.font,
            None,
            &magic_view(&casters, None),
            (r.x, r.y),
            (0, 0),
            (0, 0),
        );
        same_text(&rig, &log, &text, &format!("run {run} casters {ids:?}"));
        let want: Vec<(i32, i32)> = (0..n)
            .filter(|&i| ChoiceFlags(f).marker_variant(i).is_some())
            .map(|i| (r.x, r.y + 1 + i as i32 * 0x23))
            .collect();
        let got: Vec<(i32, i32)> = hands(&log).iter().map(|h| (h.x, h.y)).collect();
        assert_eq!(got, want, "run {run}: the caster hand");

        // The info window over the cursor's caster and a learned spell.
        let caster = ids[(f & 0xFFF).min(n - 1) as usize];
        rig.cpu.mem.wr32(CASTER_CHOICE, (f & 0xFFF).min(n - 1));
        let rec = 0x8008_4708 + caster * 0x414;
        let spell = 0x81 + rng.below(0x0B);
        let known = 1 + rng.below(8);
        let slot = rng.below(known);
        rig.cpu.mem.wr8(rec + 0x13C, known as u8);
        for k in 0..known {
            let id = if k == slot { spell } else { 0x60 + k };
            rig.cpu.mem.wr8(rec + 0x13D + k, id as u8);
            rig.cpu.mem.wr8(rec + 0x161 + k, rng.below(10) as u8);
        }
        rig.cpu.mem.wr32(SELECTED, spell);
        let log = rig.run(MAGIC_INFO, r);
        let srec = SPELLS + spell * 12;
        let name_va = rig.cpu.mem.rd32(srec + 8);
        let raw = cstr(&mut rig.cpu.mem, name_va);
        let desc_ix = u32::from(rig.cpu.mem.rd8(srec + 4));
        let desc = if desc_ix > 0 {
            let va = rig.cpu.mem.rd32(SPELL_DESCS + desc_ix * 4);
            String::from_utf8(cstr(&mut rig.cpu.mem, va))
                .expect("ASCII")
                .replace('|', "\n")
        } else {
            String::new()
        };
        let cost = log
            .iter()
            .find_map(|e| match e {
                Ev::Number(v, 3, ..) => Some(*v as u16),
                _ => None,
            })
            .expect("the cost");
        let level = rig.cpu.mem.rd8(rec + 0x161 + slot);
        let name = String::from_utf8(raw[3..].to_vec()).expect("ASCII");
        let info = PauseMagicInfo {
            name: &name,
            icon: Some(raw[1]),
            ra_seru: false,
            level,
            desc: &desc,
            mp_cost: cost,
        };
        assert_eq!(raw[0], 0xCE, "run {run}: a plate-led name");
        let labels: Vec<Ev> = log
            .iter()
            .map(|e| match e {
                Ev::Label(va, x, y, ink) => Ev::Text(cstr(&mut rig.cpu.mem, *va), *x, *y, *ink),
                e => e.clone(),
            })
            .collect();
        let text = magic_screen_draws_for_with(
            &rig.font,
            Some(&pool),
            &magic_view(&[], Some(info)),
            (0, 0),
            (0, 0),
            (r.x, r.y),
        );
        same_text(
            &rig,
            &labels,
            &text,
            &format!("run {run} spell {spell:#x} level {level}"),
        );
        levels += 1;
    }
    assert!(levels > 2000, "{levels}");
}

#[test]
fn the_prize_rows_take_the_ink_retail_stages() {
    differential!(prot(899, PRIZE_ROWS));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const TABLE: u32 = 0x801E_4518;
    const ROW_COUNT: u32 = 0x801E_F0D0;
    const ROW_IDS: u32 = 0x801E_F0E0;
    const COINS: u32 = 0x8008_45A4;
    let context = SCRATCH + 0x200;
    rig.cpu.mem.wr32(0x8007_B450, context);
    let mut rng = Rng(0x5052_5A45);
    let mut seen = [0u32; 8];
    for run in 0..3_000 {
        let r = rect(&mut rng);
        let block = rng.below(4);
        rig.cpu.mem.wr8(context + 1, block as u8);
        let n = 1 + rng.below(8);
        rig.cpu.mem.wr8(ROW_COUNT, n as u8);
        let coins = [0, rng.below(10_000), rng.below(1_000_000)][rng.below(3) as usize];
        rig.cpu.mem.wr32(COINS, coins);
        let mut rows = Vec::new();
        for i in 0..n {
            let row = rng.below(12);
            rig.cpu.mem.wr8(ROW_IDS + i, row as u8);
            let rec = TABLE + block * 0x60 + row * 8;
            // A generated record: the item id stays the shipped one.
            let marker = [0u16, 0, 1, rng.next() as u16][rng.below(4) as usize];
            let price = [0, 100, rng.below(20_000)][rng.below(3) as usize];
            rig.cpu.mem.wr16(rec + 2, marker);
            rig.cpu.mem.wr32(rec + 4, price);
            rows.push((rec, marker as i16, price as i32));
        }
        let f = flags(&mut rng, n - 1);
        rig.cpu.mem.wr32(0x8007_BB98, f);
        // The held count the bag scan answers, one per run.
        let held = [0u32, 98, 99, 100, rng.below(120)][rng.below(5) as usize];
        rig.bag.borrow_mut().1 = held;
        let log = rig.run(PRIZE_ROWS, r);
        let names: Vec<&Ev> = log.iter().filter(|e| matches!(e, Ev::Text(..))).collect();
        assert_eq!(names.len(), n as usize, "run {run}");
        for (i, (e, &(rec, _, _))) in names.iter().zip(&rows).enumerate() {
            // A row picked twice holds the later write.
            let marker = rig.cpu.mem.rd16(rec + 2) as i16;
            let price = rig.cpu.mem.rd32(rec + 4) as i32;
            let want = legaia_engine_ui::prize_exchange::prize_row_ink(
                held as i16,
                marker,
                coins as i32,
                price,
            );
            let Ev::Text(_, x, y, ink) = e else {
                unreachable!()
            };
            let what = format!("run {run} row {i} rec {rec:#x}");
            assert_eq!(*ink, want, "{what}");
            assert_eq!((*x, *y), (r.x + 0x14, r.y + 0xE * i as i32), "{what}");
            seen[usize::from(want)] += 1;
        }
    }
    assert!(seen[0] > 1000 && seen[6] > 500 && seen[7] > 500, "{seen:?}");
}
