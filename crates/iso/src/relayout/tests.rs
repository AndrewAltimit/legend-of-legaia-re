//! Structural oracle for [`super::grow_prot_dat`]: build a synthetic Mode 2
//! Form 1 disc with the same *shape* as the real one (a subdirectory extent and
//! files living AFTER PROT.DAT), grow PROT.DAT, and assert the whole ISO stays
//! self-consistent and every file resolves by name to its new position.

use super::*;
use crate::iso9660::{find_file_in_image, read_file_in_image};
use crate::write::mode2_form1_sector_is_valid;

const PVD_LBA: u32 = 16;
const TERM_LBA: u32 = 17;
const PTBL_L_LBA: u32 = 18;
const PTBL_M_LBA: u32 = 20;
const ROOT_LBA: u32 = 22;

/// Layout after the front matter:
/// - PROT.DAT at LBA 24, `prot_sectors` long
/// - "XA" subdirectory extent right after PROT.DAT
/// - XAF.XA file after the subdir extent
/// - DMY.DAT file (in root) after XAF.XA
struct Synth {
    image: Vec<u8>,
    prot_lba: u32,
    prot_sectors: u32,
}

fn empty_form1(lba: u32) -> [u8; SECTOR_SIZE] {
    let mut s = [0u8; SECTOR_SIZE];
    set_sector_address(&mut s, lba);
    s[15] = 0x02; // mode 2
    s[18] = 0x08; // submode: data, Form 1
    s[22] = 0x08;
    s
}

fn put_sector(image: &mut [u8], lba: u32, user: &[u8]) {
    let base = lba as usize * SECTOR_SIZE;
    let mut s = empty_form1(lba);
    let n = user.len().min(USER_DATA_SIZE);
    s[USER_DATA_OFFSET..USER_DATA_OFFSET + n].copy_from_slice(&user[..n]);
    encode_mode2_form1_sector(&mut s).unwrap();
    image[base..base + SECTOR_SIZE].copy_from_slice(&s);
}

/// Encode a directory record.
fn dir_record(name: &[u8], lba: u32, size: u32, is_dir: bool) -> Vec<u8> {
    let rec_len = 33 + name.len() + ((name.len() + 1) % 2); // even pad
    let mut r = vec![0u8; rec_len];
    r[0] = rec_len as u8;
    r[2..6].copy_from_slice(&lba.to_le_bytes());
    r[6..10].copy_from_slice(&lba.to_be_bytes());
    r[10..14].copy_from_slice(&size.to_le_bytes());
    r[14..18].copy_from_slice(&size.to_be_bytes());
    r[25] = if is_dir { 0x02 } else { 0x00 };
    r[32] = name.len() as u8;
    r[33..33 + name.len()].copy_from_slice(name);
    r
}

/// Path-table record (LE or BE extent field).
fn ptbl_record(name: &[u8], ext: u32, parent: u16, big_endian: bool) -> Vec<u8> {
    let mut r = vec![0u8; 8 + name.len() + (name.len() & 1)];
    r[0] = name.len() as u8;
    if big_endian {
        r[2..6].copy_from_slice(&ext.to_be_bytes());
        r[6..8].copy_from_slice(&parent.to_be_bytes());
    } else {
        r[2..6].copy_from_slice(&ext.to_le_bytes());
        r[6..8].copy_from_slice(&parent.to_le_bytes());
    }
    r[8..8 + name.len()].copy_from_slice(name);
    r
}

fn build(prot_sectors: u32) -> Synth {
    let prot_lba = 24u32;
    let sub_lba = prot_lba + prot_sectors; // "XA" subdir extent, AFTER PROT.DAT
    let xaf_lba = sub_lba + 1;
    let dmy_lba = xaf_lba + 2; // XAF.XA is 2 sectors
    let total = dmy_lba + 1;
    let mut image = vec![0u8; total as usize * SECTOR_SIZE];
    for lba in 0..total {
        let mut s = empty_form1(lba);
        encode_mode2_form1_sector(&mut s).unwrap();
        image[lba as usize * SECTOR_SIZE..(lba as usize + 1) * SECTOR_SIZE].copy_from_slice(&s);
    }

    // PVD.
    let mut pvd = vec![0u8; USER_DATA_SIZE];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    let vol_space = total;
    pvd[80..84].copy_from_slice(&vol_space.to_le_bytes());
    pvd[84..88].copy_from_slice(&vol_space.to_be_bytes());
    let ptbl_size = 30u32; // any nonzero
    pvd[132..136].copy_from_slice(&ptbl_size.to_le_bytes());
    pvd[140..144].copy_from_slice(&PTBL_L_LBA.to_le_bytes());
    pvd[148..152].copy_from_slice(&PTBL_M_LBA.to_be_bytes());
    let root_rec = dir_record(&[0], ROOT_LBA, USER_DATA_SIZE as u32, true);
    pvd[156..156 + root_rec.len()].copy_from_slice(&root_rec);
    put_sector(&mut image, PVD_LBA, &pvd);

    // Terminator.
    let mut term = vec![0u8; USER_DATA_SIZE];
    term[0] = 255;
    term[1..6].copy_from_slice(b"CD001");
    put_sector(&mut image, TERM_LBA, &term);

    // Path tables (LE @18, BE @20): root(22) + "XA"(sub_lba).
    let mut ptl = Vec::new();
    ptl.extend_from_slice(&ptbl_record(&[0], ROOT_LBA, 1, false));
    ptl.extend_from_slice(&ptbl_record(b"XA", sub_lba, 1, false));
    put_sector(&mut image, PTBL_L_LBA, &ptl);
    let mut ptm = Vec::new();
    ptm.extend_from_slice(&ptbl_record(&[0], ROOT_LBA, 1, true));
    ptm.extend_from_slice(&ptbl_record(b"XA", sub_lba, 1, true));
    put_sector(&mut image, PTBL_M_LBA, &ptm);

    // Root dir extent: ., .., PROT.DAT, XA(dir), DMY.DAT.
    let mut root = Vec::new();
    root.extend_from_slice(&dir_record(&[0], ROOT_LBA, USER_DATA_SIZE as u32, true));
    root.extend_from_slice(&dir_record(&[1], ROOT_LBA, USER_DATA_SIZE as u32, true));
    root.extend_from_slice(&dir_record(
        b"PROT.DAT;1",
        prot_lba,
        prot_sectors * USER_DATA_SIZE as u32,
        false,
    ));
    root.extend_from_slice(&dir_record(b"XA", sub_lba, USER_DATA_SIZE as u32, true));
    root.extend_from_slice(&dir_record(b"DMY.DAT;1", dmy_lba, 100, false));
    put_sector(&mut image, ROOT_LBA, &root);

    // PROT.DAT payload: fill each sector with a recognizable pattern.
    for i in 0..prot_sectors {
        let user: Vec<u8> = (0..USER_DATA_SIZE)
            .map(|b| (i as usize + b) as u8)
            .collect();
        put_sector(&mut image, prot_lba + i, &user);
    }

    // XA subdirectory extent: ., .., XAF.XA.
    let mut sub = Vec::new();
    sub.extend_from_slice(&dir_record(&[0], sub_lba, USER_DATA_SIZE as u32, true));
    sub.extend_from_slice(&dir_record(&[1], ROOT_LBA, USER_DATA_SIZE as u32, true));
    sub.extend_from_slice(&dir_record(
        b"XAF.XA;1",
        xaf_lba,
        2 * USER_DATA_SIZE as u32,
        false,
    ));
    put_sector(&mut image, sub_lba, &sub);

    // XAF.XA payload (2 sectors) + DMY.DAT.
    put_sector(&mut image, xaf_lba, &[0xAA; USER_DATA_SIZE]);
    put_sector(&mut image, xaf_lba + 1, &[0xBB; USER_DATA_SIZE]);
    put_sector(&mut image, dmy_lba, &[0xCC; 100]);

    Synth {
        image,
        prot_lba,
        prot_sectors,
    }
}

/// Build a new PROT.DAT payload = the old logical bytes + `growth` blank sectors
/// appended (the caller's TOC-rewrite is irrelevant to the disc-level oracle).
fn grown_payload(synth: &Synth, growth: u32) -> Vec<u8> {
    let mut p =
        read_file_in_image(&synth.image, "PROT.DAT").expect("read PROT.DAT logical from synth");
    p.resize(p.len() + growth as usize * USER_DATA_SIZE, 0x5A);
    p
}

fn assert_all_sectors_valid(image: &[u8]) {
    let n = image.len() / SECTOR_SIZE;
    for lba in 0..n {
        let s = &image[lba * SECTOR_SIZE..(lba + 1) * SECTOR_SIZE];
        if is_form2(s) {
            continue;
        }
        assert!(
            mode2_form1_sector_is_valid(s),
            "sector {lba} EDC/ECC invalid"
        );
        // MSF header matches physical position.
        let msf = lba_to_msf_bcd(lba as u32);
        assert_eq!(
            &s[12..15],
            &msf,
            "sector {lba} MSF header does not match its position"
        );
    }
}

#[test]
fn grows_prot_and_cascades_every_reference() {
    let synth = build(4);
    let growth = 3u32;
    let new_payload = grown_payload(&synth, growth);
    let out = grow_prot_dat(
        &synth.image,
        synth.prot_lba,
        synth.prot_sectors,
        &new_payload,
    )
    .unwrap();

    // Image grew by exactly `growth` sectors.
    assert_eq!(out.len(), synth.image.len() + growth as usize * SECTOR_SIZE);
    // Every sector is EDC/ECC-valid and MSF-correct.
    assert_all_sectors_valid(&out);

    // Every file still resolves by name to a self-consistent (lba, size).
    let (prot_lba, prot_size) = find_file_in_image(&out, "PROT.DAT").unwrap();
    assert_eq!(prot_lba, synth.prot_lba, "PROT.DAT start LBA must not move");
    assert_eq!(
        prot_size,
        (synth.prot_sectors + growth) * USER_DATA_SIZE as u32,
        "PROT.DAT record size must grow by G*2048"
    );

    // DMY.DAT (a root file after PROT.DAT) shifted by exactly `growth` and reads
    // back byte-identical.
    let (dmy_lba, dmy_size) = find_file_in_image(&out, "DMY.DAT").unwrap();
    let (dmy_old, dmy_old_size) = find_file_in_image(&synth.image, "DMY.DAT").unwrap();
    assert_eq!(dmy_lba, dmy_old + growth);
    assert_eq!(dmy_size, dmy_old_size, "DMY.DAT size unchanged");
    assert_eq!(
        read_file_in_image(&out, "DMY.DAT").unwrap(),
        read_file_in_image(&synth.image, "DMY.DAT").unwrap()
    );

    // The "XA" subdirectory extent (which lives AFTER PROT.DAT) relocated by
    // `growth`, its self "." record points at the new extent, and its XAF.XA
    // file record shifted - so a subdirectory walk still resolves the file to
    // relocated, byte-identical content.
    let new_sub_lba = (synth.prot_lba + synth.prot_sectors) + growth;
    let sub = super::read_logical(&out, new_sub_lba, USER_DATA_SIZE as u32).unwrap();
    // record 0 = "." -> new_sub_lba
    assert_eq!(
        u32::from_le_bytes(sub[2..6].try_into().unwrap()),
        new_sub_lba
    );
    // walk to the XAF.XA record and read its content.
    let xaf_new = super::collect_dir_records(&out)
        .unwrap()
        .into_iter()
        .find(|r| r.name == "XAF.XA")
        .expect("XAF.XA record in relocated subdir");
    let xaf_content = super::read_logical(&out, xaf_new.target_lba, xaf_new.size).unwrap();
    assert_eq!(xaf_content.len(), 2 * USER_DATA_SIZE);
    assert_eq!(&xaf_content[..USER_DATA_SIZE], &[0xAA; USER_DATA_SIZE]);
    assert_eq!(&xaf_content[USER_DATA_SIZE..], &[0xBB; USER_DATA_SIZE]);

    // PROT.DAT reads back as exactly the new payload.
    assert_eq!(read_file_in_image(&out, "PROT.DAT").unwrap(), new_payload);

    // PVD volume space grew by `growth`.
    let pvd = 16 * SECTOR_SIZE + USER_DATA_OFFSET;
    let vol = u32::from_le_bytes(out[pvd + 80..pvd + 84].try_into().unwrap());
    assert_eq!(vol as usize, out.len() / SECTOR_SIZE);
}

#[test]
fn zero_growth_is_identity() {
    let synth = build(4);
    let same = read_file_in_image(&synth.image, "PROT.DAT").unwrap();
    let out = grow_prot_dat(&synth.image, synth.prot_lba, synth.prot_sectors, &same).unwrap();
    assert_eq!(out, synth.image);
}

#[test]
fn rejects_non_sector_payload() {
    let synth = build(4);
    let mut p = read_file_in_image(&synth.image, "PROT.DAT").unwrap();
    p.push(1); // not a whole sector
    assert!(grow_prot_dat(&synth.image, synth.prot_lba, synth.prot_sectors, &p).is_err());
}

#[test]
fn msf_roundtrips_bcd() {
    for &lba in &[0u32, 15, 150, 59448, 138985, 213158] {
        let msf = lba_to_msf_bcd(lba);
        // decode back
        let un = |b: u8| ((b >> 4) * 10 + (b & 0xF)) as u32;
        let v = (un(msf[0]) * 60 + un(msf[1])) * 75 + un(msf[2]);
        assert_eq!(v, lba + 150);
    }
}
