//! Disc-gated validation of the Mode 2/2352 EDC/ECC encoder against a real
//! disc. Skips (passes) when `LEGAIA_DISC_BIN` is unset, so CI runs without
//! redistributing Sony data.
//!
//! The decisive correctness check: for a large sample of real PROT.DAT sectors,
//! our freshly computed EDC/ECC must equal the bytes Sony's mastering tool wrote
//! — if we reproduce the disc's parity exactly, the encoder is correct. Then we
//! exercise the write path on a scratch copy: patch a byte, confirm the touched
//! sector is still valid and the byte reads back, and confirm restoring the
//! original byte returns the sector byte-for-byte.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_iso::write::{
    encode_mode2_form1_sector, is_form2, mode2_form1_sector_is_valid, patch_file_logical,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn encoder_reproduces_real_prot_dat_ecc() {
    let Some(image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (lba, size) = find_file_in_image(&image, "PROT.DAT").expect("PROT.DAT present");
    let n_sectors = (size as usize).div_ceil(USER_DATA_SIZE);

    // Sample widely across the 121 MB file (a prime stride avoids any periodic
    // alignment), checking each sampled sector's stored EDC/ECC matches a fresh
    // encode.
    let mut form1 = 0usize;
    let mut checked = 0usize;
    let mut s = 0usize;
    while s < n_sectors {
        let base = (lba as usize + s) * SECTOR_SIZE;
        if base + SECTOR_SIZE > image.len() {
            break;
        }
        let sec = &image[base..base + SECTOR_SIZE];
        if !is_form2(sec) {
            form1 += 1;
            assert!(
                mode2_form1_sector_is_valid(sec),
                "encoder disagrees with the disc's stored EDC/ECC at PROT sector {s}"
            );
        }
        checked += 1;
        s += 97;
    }
    assert!(
        checked > 500,
        "expected to sample many sectors, got {checked}"
    );
    assert!(
        form1 > 500,
        "PROT.DAT sectors are Form 1; sampled {form1} of {checked}"
    );
    eprintln!("validated {form1}/{checked} real PROT.DAT sectors against the encoder");
}

#[test]
fn patch_round_trips_a_real_sector() {
    let Some(mut image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (lba, _size) = find_file_in_image(&image, "PROT.DAT").expect("PROT.DAT present");

    // Pick a logical offset well inside PROT.DAT, away from the sector seam.
    let logical_off: u64 = 10 * USER_DATA_SIZE as u64 + 500;
    let disc_sector = lba as usize + 10;
    let base = disc_sector * SECTOR_SIZE;
    let byte_pos = base + USER_DATA_OFFSET + 500;

    let original_sector = image[base..base + SECTOR_SIZE].to_vec();
    let original_byte = image[byte_pos];
    let new_byte = original_byte ^ 0xFF;

    // The disc sector is valid to begin with.
    assert!(mode2_form1_sector_is_valid(
        &image[base..base + SECTOR_SIZE]
    ));

    // Patch one byte; sector stays valid and the byte reads back.
    patch_file_logical(&mut image, lba, logical_off, &[new_byte]).expect("patch");
    assert_eq!(image[byte_pos], new_byte, "patched byte not written");
    assert!(
        mode2_form1_sector_is_valid(&image[base..base + SECTOR_SIZE]),
        "patched sector must stay EDC/ECC-valid"
    );
    // The patch changed exactly the user byte + the sector's EDC/ECC trailer,
    // nothing else in the sector.
    let mut expected = original_sector.clone();
    expected[USER_DATA_OFFSET + 500] = new_byte;
    encode_mode2_form1_sector(&mut expected).unwrap();
    assert_eq!(
        &image[base..base + SECTOR_SIZE],
        &expected[..],
        "patch must touch only the user byte and the recomputed parity"
    );

    // Restoring the original byte yields the original sector byte-for-byte —
    // proving our re-encode reproduces Sony's parity, not just a valid one.
    patch_file_logical(&mut image, lba, logical_off, &[original_byte]).expect("restore");
    assert_eq!(
        &image[base..base + SECTOR_SIZE],
        &original_sector[..],
        "restoring the byte must reproduce the original sector exactly"
    );
}
