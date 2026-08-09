//! Disc-gated oracle for `monster_archive::slim_castables` on the two blocks
//! it exists for - Che (163) and Lu (164) Delilas, the Delilas Challenge
//! double-team clones. Asserts the slim block re-parses identically for
//! everything the fight uses: stat record, name, mesh bytes, texture bytes,
//! and every kept action animation byte-for-byte; the dropped set is exactly
//! the generic-AI castable menu; and the heap footprint (`+0x08`) lands under
//! the measured pair budget. Skips without the extracted archive.

use legaia_asset::monster_archive::{self, encode_slot, slim_castables};
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

/// Wrap a decoded block as a synthetic single-slot archive so the id-keyed
/// parsers can re-read it as monster id 1.
fn as_archive(block: &[u8]) -> Vec<u8> {
    encode_slot(block).expect("slim block re-encodes into a slot")
}

#[test]
fn che_and_lu_slim_blocks_keep_everything_the_fight_uses() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // (id, expected dropped castable ids in entry order, minimum heap saving)
    let cases: &[(u16, &[u8], usize)] = &[
        (163, &[0x0F, 0x0D, 0x10, 0x0E], 0x5000),
        (164, &[0x0D, 0x13, 0x0E, 0x12], 0x2000),
    ];

    let mut pair_heap = 0usize;
    for &(id, want_dropped, min_saved) in cases {
        let block = monster_archive::decode_block(&entry, id)
            .unwrap()
            .expect("Delilas slot decodes");
        let slim = slim_castables(&block).unwrap();
        let dropped_ids: Vec<u8> = slim.dropped.iter().map(|d| d.id).collect();
        assert_eq!(dropped_ids, want_dropped, "id {id} dropped set");
        assert!(
            slim.heap_saved >= min_saved,
            "id {id} saved {:#x} < {min_saved:#x}",
            slim.heap_saved
        );

        let orig_arch = as_archive(&block);
        let slim_arch = as_archive(&slim.bytes);

        // Stat record + name identical.
        let orig_rec = monster_archive::record(&orig_arch, 1).unwrap().unwrap();
        let slim_rec = monster_archive::record(&slim_arch, 1).unwrap().unwrap();
        assert_eq!(orig_rec.name, slim_rec.name);
        assert_eq!(orig_rec.hp, slim_rec.hp);
        assert_eq!(orig_rec.mp, slim_rec.mp);
        assert_eq!(orig_rec.battle_stats(), slim_rec.battle_stats());
        assert_eq!(orig_rec.gold, slim_rec.gold);
        assert_eq!(orig_rec.exp, slim_rec.exp);
        assert_eq!(
            orig_rec.spells.len() - want_dropped.len(),
            slim_rec.spells.len(),
            "id {id} entry count"
        );

        // Mesh + texture pools byte-identical.
        let orig_mesh = monster_archive::mesh(&orig_arch, 1).unwrap().unwrap();
        let slim_mesh = monster_archive::mesh(&slim_arch, 1).unwrap().unwrap();
        assert_eq!(orig_mesh.tmd_offset, slim_mesh.tmd_offset);
        let tmd = legaia_tmd::parse(orig_mesh.tmd_bytes()).unwrap();
        let tmd_len = tmd.stats().total_bytes_consumed;
        assert_eq!(
            &orig_mesh.tmd_bytes()[..tmd_len],
            &slim_mesh.tmd_bytes()[..tmd_len],
            "id {id} TMD bytes"
        );
        assert_eq!(
            orig_mesh.texture_pool_bytes().unwrap(),
            slim_mesh.texture_pool_bytes().unwrap(),
            "id {id} texture pool"
        );

        // Every kept action animation byte-identical (matched by position
        // among the kept set - order is preserved).
        let orig_anims = monster_archive::animations(&orig_arch, 1).unwrap().unwrap();
        let slim_anims = monster_archive::animations(&slim_arch, 1).unwrap().unwrap();
        let kept_orig: Vec<_> = orig_anims
            .iter()
            .filter(|a| {
                !want_dropped.contains(&a.action_id)
                    || orig_anims
                        .iter()
                        .filter(|b| b.action_id == a.action_id)
                        .count()
                        > 1
            })
            .collect();
        // The dropped castable ids are unique within each block, so the
        // filter above is exact; sanity-check the counts line up.
        assert_eq!(kept_orig.len(), slim_anims.len(), "id {id} kept anims");
        for (o, s) in kept_orig.iter().zip(slim_anims.iter()) {
            assert_eq!(o.action_id, s.action_id, "id {id} anim order");
            assert_eq!(o.part_count, s.part_count);
            assert_eq!(
                o.frames, s.frames,
                "id {id} action {:#x} frames",
                o.action_id
            );
        }

        // The slot re-encodes within the fixed stride (asserted by
        // `encode_slot` above) - record the pair heap cost.
        let heap = u32::from_le_bytes(slim.bytes[8..12].try_into().unwrap()) as usize;
        pair_heap += heap;
    }

    // The measured workable distinct-monster budget is ~145 KB
    // (docs/subsystems/battle.md); the slim pair must clear it with margin.
    assert!(
        pair_heap <= 140 * 1024,
        "slim pair heap cost {pair_heap} bytes exceeds the budget margin"
    );
}
