//! End-to-end disc-gated test: patch a real monster's item drop onto a scratch
//! copy of the disc and confirm it decodes back through the full
//! disc → ISO → PROT → LZS chain, with the touched sectors staying valid.
//!
//! Needs the full disc image (it rewrites physical sectors), so it gates on
//! `LEGAIA_DISC_BIN` and skips+passes when unset. It never writes the patched
//! image anywhere - the edit lives only in memory.

use legaia_asset::monster_archive::{self, SLOT_STRIDE};
use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Read `sector_count` sectors of 2048-byte user data starting at `lba` from an
/// in-memory disc image (mirror of the patcher's internal reader).
fn read_user_data(image: &[u8], lba: u32, sector_count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(sector_count * USER_DATA_SIZE);
    for i in 0..sector_count {
        let base = (lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        out.extend_from_slice(&image[base..base + USER_DATA_SIZE]);
    }
    out
}

#[test]
fn patch_monster_drop_round_trips_through_the_disc() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");

    // Gimard (id 10) is a known, well-decoded record.
    let id = 10u16;
    let entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    let original = monster_archive::record(&entry, id).unwrap().unwrap();

    // New drop values guaranteed to differ.
    let new_item = original.drop_item.wrapping_add(1);
    let new_chance = (original.drop_chance_pct % 100) + 1;

    // Build the re-packed slot and patch it onto the disc.
    let slot = patcher.monster_slot(id).unwrap();
    assert_eq!(slot.len(), SLOT_STRIDE);
    let repacked = legaia_patcher::monster::set_drop(&slot, new_item, new_chance).unwrap();
    patcher.patch_monster_slot(id, &repacked).unwrap();

    // Re-decode the patched archive straight off the patched disc image.
    let patched_entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).unwrap();
    let patched = monster_archive::record(&patched_entry, id)
        .unwrap()
        .unwrap();
    assert_eq!(patched.drop_item, new_item, "patched drop item must decode");
    assert_eq!(
        patched.drop_chance_pct, new_chance,
        "patched chance must decode"
    );
    // The edit was surgical: everything else is intact.
    assert_eq!(patched.name, original.name);
    assert_eq!(patched.hp, original.hp);
    assert_eq!(patched.stats, original.stats);
    assert_eq!(patched.gold, original.gold);

    // A neighbouring monster's record is untouched by the patch.
    let neighbour = monster_archive::record(&patched_entry, id + 1).unwrap();
    let neighbour_before = monster_archive::record(&entry, id + 1).unwrap();
    assert_eq!(
        neighbour.map(|r| (r.drop_item, r.hp, r.name)),
        neighbour_before.map(|r| (r.drop_item, r.hp, r.name)),
        "adjacent monster slot must be unchanged"
    );

    // Spot-check that PROT.DAT sectors in the patched region are EDC/ECC-valid.
    // Re-derive the entry's disc sectors via PROT.DAT's directory + TOC.
    let img = patcher.image();
    let (prot_lba, prot_size) = find_file_in_image(img, "PROT.DAT").unwrap();
    let prot_sectors = (prot_size as usize).div_ceil(USER_DATA_SIZE);
    let mut payload = read_user_data(img, prot_lba, prot_sectors);
    payload.truncate(prot_size as usize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).unwrap();
    let entry_start_lba = archive.entries[MONSTER_ARCHIVE_ENTRY].start_lba;
    let slot_logical =
        entry_start_lba as u64 * USER_DATA_SIZE as u64 + (id as u64 - 1) * SLOT_STRIDE as u64;
    let first_internal_sector = slot_logical / USER_DATA_SIZE as u64;
    let mut checked = 0;
    for k in 0..4u64 {
        let disc_sector = prot_lba as u64 + first_internal_sector + k;
        let base = disc_sector as usize * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[base..base + SECTOR_SIZE]),
            "patched slot sector {k} must be EDC/ECC-valid"
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
    eprintln!(
        "patched Gimard drop {} -> {new_item} on disc; round-trips + sectors valid",
        original.drop_item
    );
}
