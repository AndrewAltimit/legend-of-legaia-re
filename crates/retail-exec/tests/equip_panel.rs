//! The hub entry list's equipment stat sub-panel `FUN_801E5B4C` (PROT 0897)
//! against `legaia_engine_dev_menu::dev_menu_leaves::equip_stat_panel`:
//! random characters, loadouts, base stats, menu modes, cursors and
//! inventories, with the string drawer `FUN_80036888`, the numeral field
//! `FUN_80034B78` and the arrow glyph `FUN_8003C1F8` recorded with the ink
//! word `_DAT_8007B454` each one ran under. The item and equipment tables
//! and the weapon-slot table are the executable's own. The trial equip the
//! routine makes on the live record must be undone.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dev_menu::dev_menu_leaves::{
    EquipPanelDraw, EquipPanelInput, EquipProps, ItemProps, equip_stat_panel,
};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const PANEL: u32 = 0x801E_5B4C;
const DRAW_STRING: u32 = 0x8003_6888;
const DRAW_VALUE: u32 = 0x8003_4B78;
const DRAW_GLYPH: u32 = 0x8003_C1F8;
const INK: u32 = 0x8007_B454;
const CHAR: u32 = 0x8007_B469;
const TIGHT: u32 = 0x8007_B450;
const MODE: u32 = 0x8007_BB9C;
const CURSOR: u32 = 0x8007_BB88;
const WEAPON_SLOTS: u32 = 0x8007_B42C;
const RECORDS: u32 = 0x8008_4140;
const INVENTORY: u32 = 0x8008_4140 + 0x1818;
const ITEMS: u32 = 0x8007_4368;
const EQUIP: u32 = 0x8007_4F68;
const NODE: u32 = 0x8014_0000;

#[test]
fn the_equip_stat_panel_draws_what_retail_draws() {
    differential!(prot(897, PANEL));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(897).expect("PROT 0897"),
    );
    // `memset`, a BIOS A-table call.
    cpu.hook(0x8005_6788, |r, m| {
        for k in 0..r.r[A2] {
            m.wr8(r.r[A0] + k, r.r[A1] as u8);
        }
    });
    let log = Rc::new(RefCell::new(Vec::<EquipPanelDraw>::new()));
    let l = log.clone();
    cpu.hook(DRAW_STRING, move |r, m| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0));
        let y = m.rd32(r.r[SP] + 0x10) as i16;
        let ink = m.rd32(INK) as i32;
        l.borrow_mut().push(EquipPanelDraw::Label {
            label_va: r.r[A0],
            x: r.r[A3] as i16,
            y,
            ink,
        });
    });
    let l = log.clone();
    cpu.hook(DRAW_VALUE, move |r, m| {
        assert_eq!(r.r[A1], 3, "three digits");
        let ink = m.rd32(INK) as i32;
        l.borrow_mut().push(EquipPanelDraw::Value {
            value: r.r[A0] as i32,
            x: r.r[A2] as i16,
            y: r.r[A3] as i16,
            ink,
        });
    });
    let l = log.clone();
    cpu.hook(DRAW_GLYPH, move |r, m| {
        let ink = m.rd32(INK) as i32;
        l.borrow_mut().push(EquipPanelDraw::Arrow {
            glyph: r.r[A0] as i32,
            x: r.r[A1] as i16,
            y: r.r[A2] as i16,
            ink,
        });
    });

    // The executable's tables, read once for the port.
    let item_props: Vec<ItemProps> = (0..256u32)
        .map(|id| ItemProps {
            kind: cpu.mem.rd8(ITEMS + 12 * id),
            stat_index: cpu.mem.rd8(ITEMS + 12 * id + 1),
        })
        .collect();
    let equip_props: Vec<EquipProps> = (0..256u32)
        .map(|i| {
            let at = EQUIP + 8 * i;
            EquipProps {
                bonuses: std::array::from_fn(|k| cpu.mem.rd8(at + k as u32)),
                char_mask: cpu.mem.rd8(at + 6),
                slot_bits: cpu.mem.rd8(at + 7),
            }
        })
        .collect();
    let weapon_slots: Vec<i16> = (0..4u32)
        .map(|c| cpu.mem.rd16(WEAPON_SLOTS + 2 * c) as i16)
        .collect();
    let equipment: Vec<u8> = (0..=255u8)
        .filter(|&id| item_props[usize::from(id)].kind == 1)
        .collect();

    let mut rng = Rng(0x4551_5550);
    let (mut rejected, mut compared, mut arrows) = (0, 0, 0);
    for case in 0..6000 {
        let char_index = rng.below(4) as usize;
        let pick = |rng: &mut Rng| match rng.below(6) {
            0 => 0,
            1 => rng.next() as u8,
            _ => equipment[rng.below(equipment.len() as u32) as usize],
        };
        let slots: [u8; 5] = std::array::from_fn(|_| pick(&mut rng));
        let base_stats: [i32; 3] = std::array::from_fn(|_| rng.below(1000) as i32);
        let mode = [0x1000, 0x6000, 0x9000, 0x3000, 0x4000, 0x2000][rng.below(6) as usize];
        let inventory: Vec<u8> = (0..0x100).map(|_| pick(&mut rng)).collect();
        let cursor = match mode {
            0x4000 => rng.below(3) as i32,
            0x3000 => i32::from(pick(&mut rng)),
            _ => rng.below(0x100) as i32,
        };
        let tight = rng.below(2) == 0;
        let (x, y) = (rng.below(200) as i16, rng.below(200) as i16);

        let rec = RECORDS + 0x414 * char_index as u32;
        for (k, s) in slots.iter().enumerate() {
            cpu.mem.wr8(rec + 0x75E + k as u32, *s);
        }
        for (k, b) in base_stats.iter().enumerate() {
            cpu.mem.wr16(rec + 0x6DA + 2 * k as u32, *b as u16);
        }
        for (k, id) in inventory.iter().enumerate() {
            cpu.mem.wr16(INVENTORY + 2 * k as u32, u16::from(*id));
        }
        cpu.mem.wr8(CHAR, char_index as u8);
        cpu.mem.wr32(MODE, mode);
        cpu.mem.wr32(CURSOR, cursor as u32);
        cpu.mem.wr32(TIGHT, u32::from(tight));
        cpu.mem.wr32(INK, 0x55);
        cpu.mem.wr16(NODE + 0xA, x as u16);
        cpu.mem.wr16(NODE + 0xC, y as u16);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(PANEL, &[NODE]), Stop::Returned, "case {case}");
        for (k, s) in slots.iter().enumerate() {
            assert_eq!(
                cpu.mem.rd8(rec + 0x75E + k as u32),
                *s,
                "case {case}: slot {k} restored"
            );
        }

        let input = EquipPanelInput {
            x,
            y,
            char_index,
            slots,
            base_stats,
            mode,
            cursor,
            inventory: inventory.clone(),
            weapon_slots: weapon_slots.clone(),
            tight_rows: tight,
        };
        let mut want = equip_stat_panel(
            &input,
            |id| item_props[usize::from(id)],
            |i| equip_props[usize::from(i)],
        );
        // The port names each label by the VA of its pointer word; retail
        // hands the drawer the pointer.
        for d in &mut want {
            if let EquipPanelDraw::Label { label_va, .. } = d {
                *label_va = cpu.mem.rd32(*label_va);
            }
        }
        let got = log.borrow().clone();
        rejected += usize::from(got.len() == 1);
        compared += usize::from(got.len() > 6);
        arrows += got
            .iter()
            .filter(|d| matches!(d, EquipPanelDraw::Arrow { .. }))
            .count();
        assert_eq!(got, want, "case {case}: {input:?}");
    }
    assert!(
        rejected > 100 && compared > 1000 && arrows > 1000,
        "rejected {rejected} compared {compared} arrows {arrows}"
    );
}
