//! Mode 2/2352 sector write-back: EDC/ECC re-encoding + logical-file patching.
//!
//! Reading a PSX disc only needs the 2048-byte user payload (see [`crate::raw`]).
//! *Writing* one back needs the rest of the physical sector to stay valid: each
//! Mode 2 Form 1 sector carries a 4-byte EDC (error-detection CRC) and 276 bytes
//! of P/Q ECC (Reed-Solomon parity) that a real console's CD controller checks.
//! Overwrite the user data without fixing those and the sector reads as
//! corrupt. This module recomputes them.
//!
//! ## Not Sony IP
//!
//! The EDC/ECC algorithm is the generic CD-ROM error-correction scheme defined
//! by ECMA-130 / the Yellow Book - the same math every PSX disc (and every
//! mastering tool) uses. It is not game-specific and embeds no game bytes. The
//! disc-gated test [`tests`] proves the encoder reproduces the *existing*
//! EDC/ECC of thousands of real PROT.DAT sectors bit-for-bit, which is the
//! decisive correctness check.
//!
//! ## Sector layout (Mode 2 Form 1, 2352 bytes)
//!
//! ```text
//! 0x000  sync (12)
//! 0x00C  header (4): min, sec, frac (BCD), mode=0x02
//! 0x010  subheader (8): two copies of [file, channel, submode, coding]
//! 0x018  user data (2048)
//! 0x818  EDC (4)            ; CRC over 0x010..0x818 (subheader + user data)
//! 0x81C  ECC P parity (172)
//! 0x8C8  ECC Q parity (104)
//! ```
//!
//! The ECC is computed with the 4-byte header (`0x00C..0x010`) treated as zero
//! - the Form 1 convention - so the parity does not depend on the sector's MSF
//! address.

use crate::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use anyhow::{Result, bail};

// Sector field offsets.
const HEADER_OFF: usize = 0x00C;
const SUBHEADER_OFF: usize = 0x010;
const EDC_OFF: usize = 0x818;
const ECC_P_OFF: usize = 0x81C;
const ECC_Q_OFF: usize = 0x8C8;
/// EDC covers the subheader + user data: `0x010..0x818` = 2056 bytes.
const EDC_RANGE: usize = 0x808;
/// Submode byte within each subheader copy; bit `0x20` selects Form 2.
const SUBMODE_OFF: usize = SUBHEADER_OFF + 2;
const FORM2_BIT: u8 = 0x20;

// --- EDC: CRC-32 variant, reversed polynomial 0xD8018001 -------------------

const fn build_edc_lut() -> [u32; 256] {
    let mut lut = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut edc = i as u32;
        let mut j = 0;
        while j < 8 {
            edc = (edc >> 1) ^ if edc & 1 != 0 { 0xD801_8001 } else { 0 };
            j += 1;
        }
        lut[i] = edc;
        i += 1;
    }
    lut
}

static EDC_LUT: [u32; 256] = build_edc_lut();

fn edc_compute(data: &[u8]) -> u32 {
    let mut edc = 0u32;
    for &b in data {
        edc = (edc >> 8) ^ EDC_LUT[((edc ^ b as u32) & 0xFF) as usize];
    }
    edc
}

// --- ECC: P/Q Reed-Solomon over GF(2^8), generator polynomial 0x11D --------

const fn build_ecc_luts() -> ([u8; 256], [u8; 256]) {
    let mut f = [0u8; 256];
    let mut b = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        // j stays < 256 (the 0x11D xor clears the bit the <<1 set for i>=0x80).
        let j = (i << 1) ^ if i & 0x80 != 0 { 0x11D } else { 0 };
        f[i] = j as u8;
        b[i ^ j] = i as u8;
        i += 1;
    }
    (f, b)
}

static ECC_LUTS: ([u8; 256], [u8; 256]) = build_ecc_luts();

/// Compute one ECC parity field (P or Q) in place. Reads and writes the same
/// `sector` slice; within a single call the read range (`src_off..src_off+size`)
/// and the write range (`dest_off..`) never overlap, so the in-place form is
/// well defined. Mirrors the canonical ECMA-130 block ECC.
fn ecc_compute_block(
    sector: &mut [u8],
    src_off: usize,
    major_count: u32,
    minor_count: u32,
    major_mult: u32,
    minor_inc: u32,
    dest_off: usize,
) {
    let (f_lut, b_lut) = &ECC_LUTS;
    let size = major_count * minor_count;
    for major in 0..major_count {
        let mut index = (major >> 1) * major_mult + (major & 1);
        let mut ecc_a = 0u8;
        let mut ecc_b = 0u8;
        for _ in 0..minor_count {
            let temp = sector[src_off + index as usize];
            index += minor_inc;
            if index >= size {
                index -= size;
            }
            ecc_a ^= temp;
            ecc_b ^= temp;
            ecc_a = f_lut[ecc_a as usize];
        }
        ecc_a = b_lut[(f_lut[ecc_a as usize] ^ ecc_b) as usize];
        sector[dest_off + major as usize] = ecc_a;
        sector[dest_off + (major + major_count) as usize] = ecc_a ^ ecc_b;
    }
}

/// `true` if a 2352-byte sector's subheader marks it Form 2 (`0x800`-byte
/// payload, no ECC). Form 1 sectors are the data sectors this module patches.
pub fn is_form2(sector: &[u8]) -> bool {
    sector.len() > SUBMODE_OFF && sector[SUBMODE_OFF] & FORM2_BIT != 0
}

/// Recompute the EDC and P/Q ECC of a Mode 2 Form 1 sector in place.
///
/// Call after overwriting any of the sector's 2048-byte user data. Errors if
/// the slice is not exactly one sector or if the subheader marks it Form 2
/// (which has no ECC and a different EDC range - patching those is unsupported
/// here because the game's data files are all Form 1).
pub fn encode_mode2_form1_sector(sector: &mut [u8]) -> Result<()> {
    if sector.len() != SECTOR_SIZE {
        bail!("sector must be {SECTOR_SIZE} bytes, got {}", sector.len());
    }
    if is_form2(sector) {
        bail!("sector is Form 2 (no ECC); Form 1 expected");
    }

    // EDC over the subheader + user data, before ECC (which covers the EDC).
    let edc = edc_compute(&sector[SUBHEADER_OFF..SUBHEADER_OFF + EDC_RANGE]);
    sector[EDC_OFF..EDC_OFF + 4].copy_from_slice(&edc.to_le_bytes());

    // ECC treats the 4-byte header as zero (the Form 1 convention), so save and
    // restore the real MSF address around the computation.
    let addr = [
        sector[HEADER_OFF],
        sector[HEADER_OFF + 1],
        sector[HEADER_OFF + 2],
        sector[HEADER_OFF + 3],
    ];
    sector[HEADER_OFF..HEADER_OFF + 4].fill(0);

    // P parity: 86 majors x 24 minors. Q parity: 52 majors x 43 minors.
    ecc_compute_block(sector, HEADER_OFF, 86, 24, 2, 86, ECC_P_OFF);
    ecc_compute_block(sector, HEADER_OFF, 52, 43, 86, 88, ECC_Q_OFF);

    sector[HEADER_OFF..HEADER_OFF + 4].copy_from_slice(&addr);
    Ok(())
}

/// `true` if the sector's stored EDC/ECC already match a fresh encode - i.e.
/// the sector is internally consistent. Used by validators / tests.
pub fn mode2_form1_sector_is_valid(sector: &[u8]) -> bool {
    if sector.len() != SECTOR_SIZE || is_form2(sector) {
        return false;
    }
    let mut copy = sector.to_vec();
    if encode_mode2_form1_sector(&mut copy).is_err() {
        return false;
    }
    copy[EDC_OFF..SECTOR_SIZE] == sector[EDC_OFF..SECTOR_SIZE]
}

/// Overwrite `new_bytes` into an ISO file's logical payload starting at
/// `logical_off` bytes into the file, then re-encode the EDC/ECC of every
/// physical sector the write touched.
///
/// `file_lba` is the file's start sector on the disc (e.g. from
/// [`crate::iso9660::find_file_in_image`]). `logical_off` is a byte offset into
/// the file's 2048-byte-per-sector logical payload (e.g. a PROT.DAT-relative
/// offset). The write stays inside existing sectors - it never grows the image
/// or shifts any LBA, which is exactly what same-size asset edits need.
///
/// Errors (leaving the image unchanged at the point of failure) if a touched
/// sector runs past the image or is not Form 1.
pub fn patch_file_logical(
    image: &mut [u8],
    file_lba: u32,
    logical_off: u64,
    new_bytes: &[u8],
) -> Result<()> {
    if new_bytes.is_empty() {
        return Ok(());
    }
    // Pre-flight: every sector the write touches must exist and be Form 1, so a
    // mid-write bail can't leave half a patch behind.
    let first_sector = logical_off / USER_DATA_SIZE as u64;
    let last_byte = logical_off + new_bytes.len() as u64 - 1;
    let last_sector = last_byte / USER_DATA_SIZE as u64;
    for internal in first_sector..=last_sector {
        let disc_sector = file_lba as u64 + internal;
        let base = disc_sector as usize * SECTOR_SIZE;
        if base + SECTOR_SIZE > image.len() {
            bail!("write touches sector {disc_sector} past end of image");
        }
        if is_form2(&image[base..base + SECTOR_SIZE]) {
            bail!("write touches Form 2 sector {disc_sector} (unsupported)");
        }
    }

    let mut written = 0usize;
    let mut off = logical_off;
    while written < new_bytes.len() {
        let internal = off / USER_DATA_SIZE as u64;
        let in_sector = (off % USER_DATA_SIZE as u64) as usize;
        let disc_sector = file_lba as u64 + internal;
        let base = disc_sector as usize * SECTOR_SIZE;

        let take = (USER_DATA_SIZE - in_sector).min(new_bytes.len() - written);
        let ud = base + USER_DATA_OFFSET + in_sector;
        image[ud..ud + take].copy_from_slice(&new_bytes[written..written + take]);
        encode_mode2_form1_sector(&mut image[base..base + SECTOR_SIZE])?;

        written += take;
        off += take as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal but structurally valid Mode 2 Form 1 sector with the
    /// given user data, EDC/ECC freshly computed.
    fn make_sector(user: &[u8; USER_DATA_SIZE]) -> Vec<u8> {
        let mut s = vec![0u8; SECTOR_SIZE];
        // sync pattern
        s[0] = 0x00;
        for b in s.iter_mut().take(11).skip(1) {
            *b = 0xFF;
        }
        s[11] = 0x00;
        // header: arbitrary BCD MSF + mode 2
        s[HEADER_OFF] = 0x00;
        s[HEADER_OFF + 1] = 0x02;
        s[HEADER_OFF + 2] = 0x16;
        s[HEADER_OFF + 3] = 0x02;
        // subheader: Form 1 (submode bit 0x20 clear), data bit 0x08 set
        s[SUBMODE_OFF] = 0x08;
        s[SUBMODE_OFF + 4] = 0x08;
        s[USER_DATA_OFFSET..USER_DATA_OFFSET + USER_DATA_SIZE].copy_from_slice(user);
        encode_mode2_form1_sector(&mut s).unwrap();
        s
    }

    #[test]
    fn encode_is_idempotent_and_self_consistent() {
        let user = std::array::from_fn(|i| (i * 31 + 7) as u8);
        let s = make_sector(&user);
        assert!(mode2_form1_sector_is_valid(&s));
        // Re-encoding an already-valid sector changes nothing.
        let mut again = s.clone();
        encode_mode2_form1_sector(&mut again).unwrap();
        assert_eq!(again, s, "encode must be idempotent");
    }

    #[test]
    fn corrupting_user_data_invalidates_until_reencoded() {
        let user = [0xABu8; USER_DATA_SIZE];
        let mut s = make_sector(&user);
        s[USER_DATA_OFFSET + 100] ^= 0xFF; // flip a user byte
        assert!(
            !mode2_form1_sector_is_valid(&s),
            "stale EDC/ECC must fail validation"
        );
        encode_mode2_form1_sector(&mut s).unwrap();
        assert!(
            mode2_form1_sector_is_valid(&s),
            "re-encode restores validity"
        );
    }

    #[test]
    fn ecc_is_independent_of_the_msf_address() {
        // Two sectors with identical user data but different MSF headers must
        // get identical EDC/ECC (the Form 1 zero-header convention).
        let user = std::array::from_fn(|i| (i ^ 0x5A) as u8);
        let a = make_sector(&user);
        let mut b = a.clone();
        b[HEADER_OFF] = 0x12;
        b[HEADER_OFF + 1] = 0x34;
        b[HEADER_OFF + 2] = 0x56;
        encode_mode2_form1_sector(&mut b).unwrap();
        assert_eq!(
            a[EDC_OFF..SECTOR_SIZE],
            b[EDC_OFF..SECTOR_SIZE],
            "EDC/ECC must not depend on the sector address"
        );
    }

    #[test]
    fn patch_writes_bytes_and_keeps_sectors_valid() {
        // Two-sector file: patch a run that straddles the boundary.
        let lba = 3u32;
        let mut image = vec![0u8; (lba as usize + 2) * SECTOR_SIZE];
        for sec in 0..2 {
            let user = [(sec as u8) * 17 + 1; USER_DATA_SIZE];
            let s = make_sector(&user);
            let base = (lba as usize + sec) * SECTOR_SIZE;
            image[base..base + SECTOR_SIZE].copy_from_slice(&s);
        }

        // Patch 8 bytes starting 4 bytes before the first/second sector seam.
        let logical_off = USER_DATA_SIZE as u64 - 4;
        let new = [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];
        patch_file_logical(&mut image, lba, logical_off, &new).unwrap();

        // Both touched sectors stay valid.
        for sec in 0..2 {
            let base = (lba as usize + sec) * SECTOR_SIZE;
            assert!(
                mode2_form1_sector_is_valid(&image[base..base + SECTOR_SIZE]),
                "sector {sec} must be valid after patch"
            );
        }
        // The bytes read back exactly (split across the seam).
        let s0 = lba as usize * SECTOR_SIZE + USER_DATA_OFFSET;
        assert_eq!(
            &image[s0 + USER_DATA_SIZE - 4..s0 + USER_DATA_SIZE],
            &new[..4]
        );
        let s1 = (lba as usize + 1) * SECTOR_SIZE + USER_DATA_OFFSET;
        assert_eq!(&image[s1..s1 + 4], &new[4..]);
    }

    #[test]
    fn patch_past_eof_errors() {
        let mut image = vec![0u8; SECTOR_SIZE]; // one sector at lba 0
        assert!(patch_file_logical(&mut image, 0, USER_DATA_SIZE as u64, &[1, 2, 3]).is_err());
    }

    #[test]
    fn empty_patch_is_a_noop() {
        let mut image = vec![0u8; SECTOR_SIZE];
        let before = image.clone();
        patch_file_logical(&mut image, 0, 0, &[]).unwrap();
        assert_eq!(image, before);
    }
}
