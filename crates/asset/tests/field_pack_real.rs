//! Disc-gated corpus facts about the entries the categorizer calls
//! `field_pack` - and about the format they turned out not to be.
//!
//! What these pin (see `docs/formats/field-pack.md`):
//!
//! 1. `field_pack::MAGIC` is a `(TIM_LIST << 24) | size` DATA_FIELD chunk
//!    header, not a magic: it occurs **once** in the whole archive, at offset 0
//!    of one entry, its low 24 bits are that entry's payload length, and its
//!    type byte agrees with the members' own magic.
//! 2. The "97-entry schema" is that chunk's `[u32 count][u32 word_offset[count]]`
//!    pack table. Its first word is the member count, and the tables are NOT
//!    byte-identical across carriers.
//! 3. The `~91 KB byte-identical region` is town01 and town0c sharing the
//!    first three members of their texture packs - real, and a shared-texture
//!    fact rather than a template.
//! 4. A carrier sits at raw-TOC offset `+4` of its CDNAME block, and the form
//!    it takes (bare pack vs chunk-headered) is selected by the `Flag`
//!    descriptor in that block's asset table: `Flag(0x0A)` -> bare,
//!    `Flag(0x14)` -> chunk-headered.
//!
//! Skips silently when `extracted/PROT.DAT` / `extracted/CDNAME.TXT` or
//! `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::AssetType;
use legaia_asset::field_pack;
use legaia_asset::scene_asset_table;
use legaia_prot::archive::Archive;
use legaia_prot::cdname;

const TIM_MAGIC: [u8; 4] = [0x10, 0x00, 0x00, 0x00];
const TMD_MAGIC: [u8; 4] = [0x02, 0x00, 0x00, 0x80];

fn extracted(rel: &str) -> Option<PathBuf> {
    [
        PathBuf::from("extracted").join(rel),
        PathBuf::from("../../extracted").join(rel),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

fn gated() -> bool {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return false;
    }
    true
}

/// Read the block-`+4` entry of every CDNAME `#define`, in extraction space.
fn block_plus_four(archive: &mut Archive, map: &cdname::IndexMap) -> Vec<(String, u32, Vec<u8>)> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (&raw, name) in map {
        if !seen.insert(raw) {
            continue;
        }
        // raw + 4 in the TOC's own space; extraction index is raw - 2.
        let Some(ex) = (raw + 4).checked_sub(cdname::RAW_TOC_INDEX_OFFSET) else {
            continue;
        };
        let Some(entry) = archive.entries.iter().find(|e| e.index == ex).cloned() else {
            continue;
        };
        let mut buf = Vec::new();
        if archive.read_entry(&entry, &mut buf).is_err() {
            continue;
        }
        out.push((name.clone(), ex, buf));
    }
    out
}

#[test]
fn the_field_pack_magic_is_a_tim_list_chunk_header() {
    let (Some(prot), true) = (extracted("PROT.DAT"), gated()) else {
        eprintln!("[skip] extracted/PROT.DAT missing or disc gate unset");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entries = archive.entries.clone();
    let mut buf = Vec::new();

    let needle = field_pack::MAGIC.to_le_bytes();
    let mut occurrences: Vec<(u32, usize)> = Vec::new();
    let mut detector_hits: Vec<u32> = Vec::new();

    for entry in &entries {
        archive.read_entry(entry, &mut buf).expect("read entry");
        for (i, w) in buf.windows(4).enumerate() {
            if w == needle {
                occurrences.push((entry.index, i));
            }
        }
        if field_pack::detect(&buf).is_some() {
            detector_hits.push(entry.index);
        }
    }

    eprintln!("[field-pack] magic occurrences: {occurrences:?}, detector: {detector_hits:?}");

    // One word, one place, offset 0 - not a family of carriers.
    assert_eq!(
        occurrences.len(),
        1,
        "the 'magic' should occur exactly once in the archive, got {occurrences:?}"
    );
    assert_eq!(occurrences[0].1, 0, "and at offset 0 of its entry");
    assert_eq!(detector_hits, vec![occurrences[0].0]);

    // It decodes as a chunk header whose payload fills the entry.
    let carrier = entries
        .iter()
        .find(|e| e.index == occurrences[0].0)
        .expect("carrier entry");
    archive.read_entry(carrier, &mut buf).expect("read carrier");
    let pack = field_pack::scene_pack(&buf).expect("carrier parses as a scene pack");
    assert_eq!(pack.asset_type(), Some(AssetType::TimList));
    assert_eq!(pack.declared_size(), Some(field_pack::MAGIC & 0x00FF_FFFF));
    assert!(
        4 + pack.declared_size().unwrap() as usize <= buf.len(),
        "declared payload must fit the entry"
    );
    // Every member is a real PSX TIM - the type byte is not decorative.
    assert_eq!(pack.members.len(), 96);
    for (i, r) in pack.members.iter().enumerate() {
        assert_eq!(
            &buf[r.start..r.start + 4],
            &TIM_MAGIC,
            "member {i} is a TIM"
        );
    }
    // And the "97-entry schema" is that pack's own table: count then offsets.
    assert_eq!(field_pack::CANONICAL_SCHEMA[0] as usize, pack.members.len());
    assert_eq!(
        field_pack::CANONICAL_SCHEMA[1] as usize * 4,
        4 + 4 * pack.members.len(),
        "member 0 starts where the offset table ends"
    );
}

#[test]
fn every_block_plus_four_pack_agrees_with_its_flag_descriptor() {
    let (Some(prot), Some(cd), true) = (extracted("PROT.DAT"), extracted("CDNAME.TXT"), gated())
    else {
        eprintln!("[skip] extracted/PROT.DAT or CDNAME.TXT missing, or disc gate unset");
        return;
    };
    let map = cdname::parse(&cd).expect("parse CDNAME.TXT");
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entries = archive.entries.clone();

    let carriers = block_plus_four(&mut archive, &map);
    let mut bare = 0usize;
    let mut chunked = 0usize;
    let mut agreed = 0usize;
    let mut flagless = Vec::new();

    let mut table_buf = Vec::new();
    for (name, ex, buf) in &carriers {
        let Some(pack) = field_pack::scene_pack(buf) else {
            continue;
        };
        // The chunk type byte, when present, must match the member magic.
        let expect_magic = match pack.asset_type() {
            Some(AssetType::TimList) => Some(TIM_MAGIC),
            Some(AssetType::Tmd) => Some(TMD_MAGIC),
            Some(other) => panic!("{name}: unexpected chunk type {other:?}"),
            None => None,
        };
        if let Some(magic) = expect_magic {
            for (i, r) in pack.members.iter().enumerate() {
                assert_eq!(
                    &buf[r.start..r.start + 4],
                    &magic,
                    "{name} entry {ex}: member {i} magic disagrees with the chunk type"
                );
            }
        }
        if pack.chunk_header.is_some() {
            chunked += 1;
        } else {
            bare += 1;
        }

        // The block's asset table sits one entry earlier.
        let Some(table_entry) = entries.iter().find(|e| e.index + 1 == *ex).cloned() else {
            continue;
        };
        archive
            .read_entry(&table_entry, &mut table_buf)
            .expect("read table entry");
        let Some(table) = scene_asset_table::detect(&table_buf) else {
            continue;
        };
        let flag = table.used().iter().find_map(|d| match d.asset_type() {
            AssetType::Flag(f) => Some(f),
            _ => None,
        });
        match flag {
            Some(0x14) => {
                assert!(
                    pack.chunk_header.is_some(),
                    "{name}: Flag(0x14) streams a DATA_FIELD chunk, so the carrier must \
                     lead with a chunk header"
                );
                agreed += 1;
            }
            Some(0x0A) => {
                assert!(
                    pack.chunk_header.is_none(),
                    "{name}: Flag(0x0A) walks a bare pack, so the carrier must not lead \
                     with a chunk header"
                );
                agreed += 1;
            }
            Some(other) => panic!("{name}: unexpected Flag(0x{other:02X}) in the bundle"),
            None => flagless.push(name.clone()),
        }
    }

    eprintln!(
        "[field-pack] block+4 packs: {chunked} chunk-headered + {bare} bare, \
         {agreed} matched a Flag descriptor, flagless: {flagless:?}"
    );
    assert!(bare > 0 && chunked > 0, "both forms must be represented");
    // 18 of the 23 carriers reach the check: two blocks have no `Flag` to
    // reach them (below), two carry a `count`-5 table `scene_asset_table::detect`
    // does not accept, and `monster_data` has no descriptor table at all.
    assert!(
        agreed >= 18,
        "expected the Flag/form correspondence on >=18 blocks, got {agreed}"
    );
    // Only the two dev-leftover blocks carry a pack with no Flag to reach it.
    assert!(
        flagless.len() <= 2,
        "unexpected flagless carriers: {flagless:?}"
    );
}

#[test]
fn the_constant_region_is_two_scenes_sharing_leading_textures() {
    let (Some(prot), Some(cd), true) = (extracted("PROT.DAT"), extracted("CDNAME.TXT"), gated())
    else {
        eprintln!("[skip] extracted/PROT.DAT or CDNAME.TXT missing, or disc gate unset");
        return;
    };
    let map = cdname::parse(&cd).expect("parse CDNAME.TXT");
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");

    let mut load = |scene: &str| -> Vec<u8> {
        let raw = map
            .iter()
            .find(|(_, v)| v.as_str() == scene)
            .map(|(k, _)| *k)
            .unwrap_or_else(|| panic!("{scene} not in CDNAME"));
        let ex = raw + 4 - cdname::RAW_TOC_INDEX_OFFSET;
        let entry = archive
            .entries
            .iter()
            .find(|e| e.index == ex)
            .cloned()
            .unwrap_or_else(|| panic!("no entry {ex}"));
        let mut buf = Vec::new();
        archive.read_entry(&entry, &mut buf).expect("read entry");
        buf
    };

    let a = load("town01");
    let b = load("town0c");
    let pa = field_pack::scene_pack(&a).expect("town01 pack");
    let pb = field_pack::scene_pack(&b).expect("town0c pack");

    // Same member count, different tables - so not one shared template.
    assert_eq!(pa.members.len(), pb.members.len());
    let table_a = &a[pa.pack_base..pa.header_end()];
    let table_b = &b[pb.pack_base..pb.header_end()];
    assert_ne!(
        table_a, table_b,
        "the two pack tables must not be byte-identical"
    );

    // The leading members are the same texture bytes - what the old
    // "byte-identical global constant block" measured.
    let shared = pa
        .members
        .iter()
        .zip(pb.members.iter())
        .take_while(|(ra, rb)| a[ra.start..ra.end] == b[rb.start..rb.end])
        .count();
    eprintln!("[field-pack] town01/town0c share {shared} leading pack members");
    assert!(
        shared >= 3,
        "expected the leading Rim Elm atlases to be shared, got {shared}"
    );
    assert!(
        shared < pa.members.len(),
        "the packs must diverge somewhere - they are different scenes"
    );
}
