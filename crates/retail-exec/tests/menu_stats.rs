//! The menu's stat block: the seeder `FUN_801CF5D0` and the equipment summer
//! `FUN_801CF650` (menu overlay), the numbers the Status page, the Equip
//! screen's compare columns and the shop's preview draw, against
//! `pause_screens::menu_stat_block` over the disc's equipment table.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_menus::equipment::equip_modifier_table_from_disc;
use legaia_engine_menus::pause_screens::{menu_stat_block, stat_record_from_character};
use legaia_game_tables::equip_stats::EquipStatTable;
use legaia_game_tables::item_effect::ItemEffectTable;
use legaia_save::CharacterRecord;
use psx_retail_exec::Stop;

const SUMMER: u32 = 0x801C_F650;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const BLOCK: u32 = 0x801E_F080;

#[test]
fn the_menu_stat_block_is_what_retail_draws() {
    let (Some(mut cpu), Some(disc), Some(scus)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc.read_entry_footprint(899).expect("PROT 0899"),
    );
    let stats = EquipStatTable::from_scus(&scus).expect("equipment table");
    let effects = ItemEffectTable::from_scus(&scus).expect("item effects");
    let table = equip_modifier_table_from_disc(&stats);
    let equipment: Vec<u8> = (1..=255u8).filter(|&id| effects.kind(id) == 1).collect();
    assert!(equipment.len() > 50, "{} equipment ids", equipment.len());
    let mut rng = Rng(0x4D53_5441);
    let mut failures = Vec::new();
    for round in 0..20000u32 {
        let slot = rng.below(3);
        let mut raw = vec![0u8; STRIDE as usize];
        let put = |raw: &mut Vec<u8>, off: usize, v: u16| {
            raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        put(&mut raw, 0x104, rng.below(10000) as u16);
        put(&mut raw, 0x108, rng.below(1000) as u16);
        for off in (0x110..=0x11A).step_by(2) {
            // A record's stats stand at most at 999; the block's words are
            // 32-bit and the port's 16-bit, which only a word past `0xFFFF`
            // could tell apart.
            let v = match rng.below(4) {
                0 => 999,
                _ => rng.below(1000),
            };
            put(&mut raw, off, v as u16);
        }
        for e in 0..8 {
            raw[0x196 + e] = match rng.below(5) {
                0 => 0,
                1 => rng.below(256) as u8,
                _ => equipment[rng.below(equipment.len() as u32) as usize],
            };
        }
        let r = RECORDS + slot * STRIDE;
        for (i, b) in raw.iter().enumerate() {
            cpu.mem.wr8(r + i as u32, *b);
        }
        assert_eq!(cpu.call(SUMMER, &[slot]), Stop::Returned);
        let retail: Vec<i32> = (0..8).map(|i| cpu.mem.rd32(BLOCK + i * 4) as i32).collect();
        let rec = CharacterRecord::parse(&raw).expect("record");
        let hms = rec.hp_mp_sp();
        let port = menu_stat_block(
            &stat_record_from_character(&rec),
            &table,
            hms.hp_max,
            hms.mp_max,
        );
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} equip {:02x?}: retail {retail:?} port {port:?}",
                &raw[0x196..0x19E]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
