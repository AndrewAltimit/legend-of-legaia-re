//! Disc-gated round trip of the `monster_names` pack section: export every
//! named record of the monster archive (PROT 867), fill a few names - one at
//! its full budget, one shorter than the retail name, one longer than its
//! record's room (the record grows) - import onto a scratch copy, and re-read
//! the archive: the renamed records decode to the new name with every stat,
//! the model and the texture pool untouched, every other slot is
//! byte-identical, a name past the longest retail name is refused, and every
//! touched sector stays EDC/ECC-valid.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_asset::monster_archive::{self, SLOT_STRIDE};
use legaia_iso::raw::SECTOR_SIZE;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::translation::{export_pack, import_pack, monster_names};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn monster_names_export_import_round_trip() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(original.clone()).expect("open disc");
    let mut pack = export_pack(&src).expect("export");
    let names = &pack.sections.monster_names;
    assert!(names.len() > 150, "named records: {}", names.len());
    assert!(
        names
            .iter()
            .all(|e| e.budget >= e.source.len().min(e.budget)),
        "every budget holds its own retail name"
    );
    // The element-badge escape exports as a markup token a translator keeps.
    assert!(
        names.iter().any(|e| e.source.starts_with("{5e:")),
        "badge escapes surface as {{5e:xx}}"
    );

    // Pick: a badge name at full budget, a plain name made shorter, one
    // entry pushed past the longest retail name, and a 7-byte-room name grown
    // to the longest retail name.
    let badge = names
        .iter()
        .position(|e| e.source.starts_with("{5e:") && e.budget >= 8)
        .expect("a badge name");
    let plain = names
        .iter()
        .position(|e| !e.source.contains('{') && e.source.len() > 4)
        .expect("a plain name");
    let over = names
        .iter()
        .enumerate()
        .position(|(i, e)| i != plain && i != badge && !e.source.contains('{'))
        .expect("a third name");
    let grow = names
        .iter()
        .enumerate()
        .position(|(i, e)| {
            i != plain && i != badge && i != over && e.budget == 7 && !e.source.contains('{')
        })
        .expect("a 7-byte-room name");
    let tok = &names[badge].source[..7];
    let full = format!("{tok}{}", "Z".repeat(names[badge].budget - 2));
    let short = "Qq".to_string();
    let too_long = "W".repeat(monster_names::RETAIL_LONGEST_NAME + 1);
    let long = "Grown Name Xy$2".to_string();
    assert_eq!(long.len(), monster_names::RETAIL_LONGEST_NAME);
    let sections = &mut pack.sections.monster_names;
    sections[badge].translation = full.clone();
    sections[plain].translation = short.clone();
    sections[over].translation = too_long;
    sections[grow].translation = long.clone();
    let keys = [
        sections[badge].key.clone(),
        sections[plain].key.clone(),
        sections[over].key.clone(),
        sections[grow].key.clone(),
    ];

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert_eq!(report.applied, 3, "issues: {:?}", report.issues);
    assert_eq!(report.grown_monster_names, 1);
    assert!(
        report
            .issues
            .iter()
            .any(|(k, m)| k == &keys[2] && m.contains("at most")),
        "a name past the longest retail name refused: {:?}",
        report.issues
    );

    let before = src.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    let patched = patcher.into_image();
    let post = DiscPatcher::open(patched.clone()).expect("open patched");
    let after = post.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    assert_eq!(before.len(), after.len());
    let id_of = |k: &str| k.strip_prefix("mon:").unwrap().parse::<u16>().unwrap();
    let touched = [id_of(&keys[0]), id_of(&keys[1]), id_of(&keys[3])];
    for id in 1..=monster_archive::slot_count(&before) as u16 {
        let range = (id as usize - 1) * SLOT_STRIDE..id as usize * SLOT_STRIDE;
        if !touched.contains(&id) {
            assert_eq!(before[range.clone()], after[range], "slot {id} untouched");
            continue;
        }
        let a = monster_archive::record(&before, id).unwrap().unwrap();
        let b = monster_archive::record(&after, id).unwrap().unwrap();
        assert_eq!(
            (a.hp, a.mp, a.stats, a.gold, a.exp),
            (b.hp, b.mp, b.stats, b.gold, b.exp)
        );
        assert_eq!(a.spells.len(), b.spells.len());
        // Everything past the name moved as one piece: the model, the spell
        // blobs and the texture pool read the same bytes through the bumped
        // offsets.
        let ba = monster_archive::decode_block(&before, id).unwrap().unwrap();
        let bb = monster_archive::decode_block(&after, id).unwrap().unwrap();
        let fa = monster_names::name_field(&ba).unwrap();
        let fb = monster_names::name_field(&bb).unwrap();
        let k = bb.len() - ba.len();
        assert_eq!(fb.next - fa.next, k, "slot {id}");
        assert_eq!(ba[fa.next..], bb[fb.next..], "slot {id} tail");
        assert_eq!(ba[..0x4C], {
            let mut h = bb[..0x4C].to_vec();
            for w in [4usize, 8] {
                let v = u32::from_le_bytes(h[w..w + 4].try_into().unwrap()) - k as u32;
                h[w..w + 4].copy_from_slice(&v.to_le_bytes());
            }
            h
        });
        for w in (0x4C..fa.offset).step_by(4) {
            let va = u32::from_le_bytes(ba[w..w + 4].try_into().unwrap()) as usize;
            let vb = u32::from_le_bytes(bb[w..w + 4].try_into().unwrap()) as usize;
            assert_eq!(
                vb,
                if va >= fa.next { va + k } else { va },
                "slot {id} +0x{w:x}"
            );
        }
        assert!(
            legaia_tmd::parse(&bb[fb.next..]).is_ok(),
            "slot {id}: model still parses at the bumped +0x04"
        );
    }
    let re = export_pack(&post).expect("re-export");
    let got = |k: &str| {
        re.sections
            .monster_names
            .iter()
            .find(|e| e.key == k)
            .map(|e| e.source.clone())
    };
    assert_eq!(got(&keys[0]).as_deref(), Some(full.as_str()));
    assert_eq!(got(&keys[1]).as_deref(), Some(short.as_str()));
    assert_eq!(got(&keys[3]).as_deref(), Some(long.as_str()));
    // Budgets are a property of the record, not of the current name: the
    // re-export offers the same room - except the grown record, which now
    // has the longest retail name's.
    for (a, b) in pack
        .sections
        .monster_names
        .iter()
        .zip(&re.sections.monster_names)
    {
        let want = if a.key == keys[3] {
            monster_names::RETAIL_LONGEST_NAME
        } else {
            a.budget
        };
        assert_eq!((&a.key, want), (&b.key, b.budget));
    }

    for (i, (a, b)) in original
        .chunks(SECTOR_SIZE)
        .zip(patched.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a != b && a.len() == SECTOR_SIZE {
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(b),
                "sector {i} invalid"
            );
        }
    }
}
