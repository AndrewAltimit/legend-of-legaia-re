//! The monster action-by-tag lookup `FUN_80050E2C` against
//! `monster_archive::find_action_by_tag`: a table of record pointers, each
//! record's first byte its tag, searched first-match for a tag, `0xFF` when
//! none matches (or the table is empty).

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::monster_archive::find_action_by_tag;
use psx_retail_exec::{Stop, V0};

const LOOKUP: u32 = 0x8005_0E2C;
const TABLE: u32 = 0x8012_0000;
const RECORDS: u32 = 0x8012_1000;

#[test]
fn the_tag_lookup_matches_retail() {
    differential!(scus(LOOKUP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5441_4753);
    let (mut found, mut missed) = (0, 0);
    for run in 0..100_000 {
        let count = [0, 1, 4, 12, 0x20, rng.below(40)][rng.below(6) as usize] as usize;
        // The action tags a monster archive carries: low ids, often repeated.
        let tags: Vec<u8> = (0..count)
            .map(|_| {
                [1, 2, 3, 4, 5, 0x0B, 0x20, 0x21, 0x22, rng.below(0x40) as u8]
                    [rng.below(10) as usize]
            })
            .collect();
        for (i, &t) in tags.iter().enumerate() {
            let rec = RECORDS + i as u32 * 0x40;
            cpu.mem.wr32(TABLE + i as u32 * 4, rec);
            cpu.mem.wr8(rec, t);
        }
        let tag =
            [2, 3, 4, 5, 0x0B, 0x20, 0x21, 0x22, rng.below(0x40) as u8][rng.below(9) as usize];
        assert_eq!(
            cpu.call(LOOKUP, &[TABLE, u32::from(tag), count as u32]),
            Stop::Returned
        );
        let retail = cpu.regs.r[V0] as u8;
        let port = find_action_by_tag(&tags, tag).unwrap_or(0xFF);
        assert_eq!(retail, port, "run {run} tag {tag:#x} tags {tags:x?}");
        found += u32::from(port != 0xFF);
        missed += u32::from(port == 0xFF);
    }
    assert!(found > 20_000 && missed > 20_000, "{found} {missed}");
}
