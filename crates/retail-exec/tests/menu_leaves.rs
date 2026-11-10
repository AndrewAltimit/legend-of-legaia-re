//! Two leaves of the menu overlay (PROT 0899): the weapon-category favor
//! check `FUN_801DD0C0` against `category_check` over the overlay's own
//! table (`0x801E4B88`) at every character, item and nibble, and the
//! target-panel preview mode `FUN_801D6A54` against `target_panel_mode` over
//! the SCUS item and item-effect tables, real and generated.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::menu_item_category::{category_check, parse_category_table};
use legaia_engine_menus::pause_screens::target_panel_mode;
use psx_retail_exec::{Cpu, Stop, V0};

const CATEGORY: u32 = 0x801D_D0C0;
const PREVIEW: u32 = 0x801D_6A54;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ITEMS: u32 = 0x8007_4368;
const EFFECTS: u32 = 0x8007_52C0;
const PREVIEW_WORD: u32 = 0x801E_46CC;

fn call(cpu: &mut Cpu, entry: u32, args: &[u32]) -> u32 {
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(
        cpu.call(entry, args),
        Stop::Returned,
        "{entry:#x} {args:x?}"
    );
    cpu.regs.r[V0]
}

#[test]
fn the_category_check_scores_what_retail_scores() {
    differential!(prot(899, CATEGORY));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry_footprint(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let table = parse_category_table(&overlay).expect("category table");
    assert!(!table.is_empty());
    let mut scored = 0;
    for item in 0..=255u32 {
        for char_index in 0..5u32 {
            for group in 0..3u32 {
                let r = call(&mut cpu, CATEGORY, &[char_index, item, group]);
                let p = category_check(&table, char_index, item as u8, group);
                assert_eq!(r, p, "char {char_index} item {item:#x} group {group}");
                scored += u32::from(r != 0);
            }
        }
    }
    assert!(scored > 20, "{scored}");
}

#[test]
fn the_preview_mode_matches_retail() {
    differential!(prot(899, PREVIEW));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let mut rng = Rng(0x5052_4556);
    let mut previews = 0;
    for pass in 0..4 {
        if pass > 0 {
            // Generated records, so every class and argument is reached.
            for id in 0..256u32 {
                cpu.mem
                    .wr8(ITEMS + id * 12, [2u8, 2, 1, 0][rng.below(4) as usize]);
                cpu.mem.wr8(ITEMS + id * 12 + 1, rng.below(130) as u8);
            }
            for e in 0..130u32 {
                cpu.mem
                    .wr8(EFFECTS + e * 4, [6u8, 6, 5, 7][rng.below(4) as usize]);
                cpu.mem.wr8(EFFECTS + e * 4 + 1, rng.below(8) as u8);
            }
        }
        for id in 0..256u32 {
            cpu.mem.wr32(PREVIEW_WORD, 0xAAAA_AAAA);
            let r = call(&mut cpu, PREVIEW, &[id]);
            let kind = cpu.mem.rd8(ITEMS + id * 12);
            let eff = u32::from(cpu.mem.rd8(ITEMS + id * 12 + 1));
            let (class, arg) = (
                cpu.mem.rd8(EFFECTS + eff * 4),
                cpu.mem.rd8(EFFECTS + eff * 4 + 1),
            );
            let p = target_panel_mode(kind, class, arg);
            assert_eq!(r, p, "pass {pass} item {id:#x}");
            assert_eq!(cpu.mem.rd32(PREVIEW_WORD), p, "pass {pass} item {id:#x}");
            previews += u32::from(p != 0);
        }
    }
    assert!(previews > 100, "{previews}");
}
