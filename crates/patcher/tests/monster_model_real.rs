//! Disc-gated round-trip oracle for the `monster-model` replacement path:
//! the committed Twintail Duelist example model is imported onto Lu Delilas
//! (id 164), spliced into her decoded block, re-packed into the archive
//! slot, and patched in place - then everything re-decodes off the patched
//! image: the mesh parses with the retail part count, every animation still
//! addresses every part, the touched sectors stay EDC/ECC-valid, and a
//! second run is byte-deterministic.
//!
//! Also proves the codec's identity path on the same disc: export -> import
//! of the retail model itself patches cleanly and keeps the prim count.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_asset::{monster_archive, monster_model};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

const LU_ID: u16 = 164;
const LU_PARTS: usize = 15;

fn load_disc() -> Option<Vec<u8>> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    std::fs::read(p).ok()
}

fn example_model() -> Option<(String, Vec<u8>)> {
    for root in [
        "../../data/models/twintail_duelist",
        "data/models/twintail_duelist",
    ] {
        let obj = PathBuf::from(root).join("twintail_duelist.obj");
        let png = PathBuf::from(root).join("twintail_duelist.png");
        if obj.is_file() && png.is_file() {
            return Some((std::fs::read_to_string(obj).ok()?, std::fs::read(png).ok()?));
        }
    }
    None
}

/// Patch `disc` with the given imported model onto Lu's slot; returns the
/// patched image.
fn patch_with(disc: Vec<u8>, imported: &monster_model::ImportedModel) -> Vec<u8> {
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let entry = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("entry 867");
    let mesh = monster_archive::mesh(&entry, LU_ID)
        .expect("decode")
        .expect("Lu populated");
    let block = monster_archive::replace_mesh_and_pool(
        &mesh.block,
        Some(&imported.tmd),
        Some(&imported.pool),
    )
    .expect("rebuild");
    assert!(
        block.len() <= mesh.block.len(),
        "the example must fit the retail heap budget without --allow-grow"
    );
    let slot = monster_archive::encode_slot(&block).expect("slot fit");
    patcher.patch_monster_slot(LU_ID, &slot).expect("patch");
    patcher.into_image()
}

fn assert_patched_image_healthy(patched: Vec<u8>, want_prims: usize) {
    let check = DiscPatcher::open(patched).expect("re-open");
    let entry = check
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("entry 867");
    let mesh = monster_archive::mesh(&entry, LU_ID)
        .expect("decode")
        .expect("still populated");
    let tmd = legaia_tmd::parse(mesh.tmd_bytes()).expect("mesh parses");
    assert_eq!(tmd.objects.len(), LU_PARTS, "part count preserved");
    assert_eq!(
        tmd.stats().total_primitives,
        want_prims,
        "prim count matches the imported model"
    );
    // The stat record survives the splice (name / HP untouched).
    let rec = monster_archive::record(&entry, LU_ID)
        .expect("record")
        .expect("record present");
    assert_eq!(rec.name, "Lu Delilas");
    assert_eq!(rec.hp, 9500);
    // Every animation still poses every part.
    let anims = monster_archive::animations(&entry, LU_ID)
        .expect("anims")
        .expect("anims present");
    assert_eq!(anims.len(), 16, "all 16 action entries survive");
    for a in &anims {
        assert_eq!(a.part_count, LU_PARTS, "animation part count intact");
    }
    // Touched sectors stay EDC/ECC-valid.
    let img = check.image();
    assert_eq!(img.len() % SECTOR_SIZE, 0);
    let lba = check.entry_disc_lba(MONSTER_ARCHIVE_ENTRY).unwrap() as usize;
    let slot_first = (LU_ID as usize - 1) * monster_archive::SLOT_STRIDE / USER_DATA_SIZE;
    let slot_sectors = monster_archive::SLOT_STRIDE / USER_DATA_SIZE;
    for s in 0..slot_sectors {
        let sb = (lba + slot_first + s) * SECTOR_SIZE;
        assert!(
            mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
            "slot sector {s} must stay EDC/ECC-valid"
        );
    }
}

#[test]
fn committed_example_model_patches_verifies_and_is_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let Some((obj_text, png_bytes)) = example_model() else {
        panic!("committed example model missing from data/models/twintail_duelist");
    };
    let (w, h, rgba) = legaia_tim::encode::decode_png_rgba(&png_bytes).expect("png");
    assert_eq!((w, h), (256, 256), "example page matches Lu's retail page");
    let imported = monster_model::import_obj(&obj_text, &rgba, w, LU_PARTS).expect("import");
    let want_prims = legaia_tmd::parse(&imported.tmd)
        .unwrap()
        .stats()
        .total_primitives;
    assert!(want_prims > 100, "example is a real model, not a stub");

    let patched = patch_with(disc.clone(), &imported);
    assert_ne!(patched, disc, "something was written");
    assert_eq!(patched.len(), disc.len(), "image size unchanged");

    // Byte-deterministic: the same import patched twice is identical.
    let imported2 = monster_model::import_obj(&obj_text, &rgba, w, LU_PARTS).expect("import 2");
    let patched2 = patch_with(disc, &imported2);
    assert_eq!(patched, patched2, "replacement is deterministic");

    assert_patched_image_healthy(patched, want_prims);
}

#[test]
fn retail_export_reimport_identity_patches_and_verifies() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open");
    let entry = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("867");
    let mesh = monster_archive::mesh(&entry, LU_ID)
        .expect("decode")
        .expect("populated");
    let retail_prims = legaia_tmd::parse(mesh.tmd_bytes())
        .unwrap()
        .stats()
        .total_primitives;
    assert_eq!(retail_prims, 857, "baseline: retail Lu prim count");

    let exported = monster_model::export_obj(&mesh, "lu").expect("export");
    let imported =
        monster_model::import_obj(&exported.obj, &exported.rgba, exported.page_width, LU_PARTS)
            .expect("import");
    let want_prims = legaia_tmd::parse(&imported.tmd)
        .unwrap()
        .stats()
        .total_primitives;
    assert_eq!(want_prims, retail_prims, "identity keeps the prim count");

    let patched = patch_with(disc, &imported);
    assert_patched_image_healthy(patched, want_prims);
}
