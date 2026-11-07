//! The Items screen's Arrange `FUN_801D64A8` (menu overlay) against
//! `ItemBag::arrange` over the disc's rank table: bags with holes, zero
//! counts, repeated ids and every window the selector installs.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::item_bag::ItemBag;
use legaia_engine_menus::menu_arrange::parse_arrange_rank_table;
use legaia_save::retail_inventory::ItemWindow;
use psx_retail_exec::{Stop, V0};

const ARRANGE: u32 = 0x801D_64A8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ALLOC: u32 = 0x8001_7888;
const FREE: u32 = 0x8001_7B94;
const SCRATCH: u32 = 0x8014_0000;
const BAG: u32 = 0x8008_5958;
const WINDOW_START: u32 = 0x8007_B5EA;
const WINDOW_END: u32 = 0x8007_B5EC;

#[test]
fn arrange_sorts_the_bag_retail_sorts() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry_footprint(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let rank = parse_arrange_rank_table(&overlay).expect("rank table");
    // The display order is a permutation, so every id has a rank and the
    // allocator's stale scratch never decides one.
    let mut seen = [false; 256];
    for id in 0..=255u8 {
        seen[usize::from(rank.rank(id))] = true;
    }
    assert!(
        seen.iter().all(|&s| s),
        "the display order is not a permutation"
    );
    cpu.stub(ALLOC, SCRATCH);
    cpu.stub(FREE, 0);
    let mut rng = Rng(0x4152_5241);
    let mut failures = Vec::new();
    let mut moved = 0;
    for round in 0..4000 {
        let pool: Vec<u8> = (0..1 + rng.below(40))
            .map(|_| rng.below(256) as u8)
            .collect();
        let fill = rng.below(256) + 1;
        let slots: Vec<(u8, u8)> = (0..256)
            .map(|_| {
                if rng.below(256) >= fill {
                    return (0, 0);
                }
                let id = pool[rng.below(pool.len() as u32) as usize];
                let count = match rng.below(8) {
                    0 => 0,
                    _ => 1 + rng.below(99) as u8,
                };
                (id, count)
            })
            .collect();
        let window = [ItemWindow::Full, ItemWindow::Low, ItemWindow::High][rng.below(3) as usize];
        let mut bag = ItemBag::from_slots(&slots);
        bag.set_window(window);
        let (start, end) = bag.window_bounds();
        for (i, &(id, count)) in slots.iter().enumerate() {
            cpu.mem.wr8(BAG + i as u32 * 2, id);
            cpu.mem.wr8(BAG + i as u32 * 2 + 1, count);
        }
        cpu.mem.wr16(WINDOW_START, start as u16);
        cpu.mem.wr16(WINDOW_END, end as u16);
        for b in 0..0x100 {
            cpu.mem.wr8(SCRATCH + b, 0xFF);
        }
        cpu.regs.r[V0] = 0;
        assert_eq!(cpu.call(ARRANGE, &[]), Stop::Returned);
        let retail: Vec<(u8, u8)> = (0..256u32)
            .map(|i| (cpu.mem.rd8(BAG + i * 2), cpu.mem.rd8(BAG + i * 2 + 1)))
            .collect();
        bag.arrange(&rank);
        let port = bag.slots().to_vec();
        moved += u32::from(retail != slots);
        if retail != port && failures.len() < 6 {
            let at = (0..256).find(|&i| retail[i] != port[i]).unwrap_or(0);
            failures.push(format!(
                "round {round} window {start}..{end} slot {at}: retail {:?} port {:?}",
                &retail[at..(at + 6).min(256)],
                &port[at..(at + 6).min(256)]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(moved > 3000, "{moved} rounds moved a slot");
}
