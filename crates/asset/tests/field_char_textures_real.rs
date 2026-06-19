//! Disc-gated regression for [`field_char_textures::parse`] + `upload_to_vram`
//! against the real disc.
//!
//! Pins the eight-entry shape of PROT 0874 §2 (the field-character texture
//! pack), the per-entry image / CLUT VRAM rects, and an FNV fingerprint of the
//! reconstructed VRAM after the `FUN_800198e0`-equivalent upload. The
//! reconstructed bytes are byte-exact against a live field-scene VRAM dump
//! (PCSX `35e640…` / `0f659b…` / `bdbacd…`, identical across all three since
//! the player textures are resident); the FNV pin is a regression guard that
//! commits no Sony bytes.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `PROT.DAT` isn't on disk.
//!
//! What this catches:
//! - PROT 0874 §2 stops being the field texture pack (extractor truncation,
//!   CDNAME shuffle, descriptor-count drift).
//! - The LZS-then-pack chain regresses (wrong section size / offset).
//! - The flat-strip CLUT upload semantic (`clut w*h × 1`) regresses to a rect.

use std::path::{Path, PathBuf};

use legaia_asset::field_char_textures;
use legaia_prot::archive::Archive;
use legaia_tim::Vram;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn read_prot_0874(prot: &Path) -> Vec<u8> {
    let mut archive = Archive::open(prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == field_char_textures::PROT_ENTRY_INDEX)
        .expect("PROT entry 874 present")
        .clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).expect("read PROT 874");
    buf
}

#[test]
fn field_texture_pack_shape_from_prot_0874() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let buf = read_prot_0874(&prot);
    let pack = field_char_textures::parse(&buf).expect("parse PROT 874 §2 field texture pack");

    assert_eq!(pack.textures.len(), 8, "PROT 0874 §2 carries 8 TIM entries");

    // Disc-invariant per-entry (image rect, CLUT rect-as-flat-strip-length).
    // (img_x, img_y, img_w_words, img_h, clut_x, clut_y, clut_colours)
    let expected = [
        (448u16, 0u16, 64u16, 256u16, 0u16, 473u16, 256usize),
        (832, 256, 20, 128, 0, 478, 64),
        (852, 256, 20, 128, 64, 478, 64),
        (872, 256, 20, 128, 128, 478, 64),
        (320, 256, 64, 256, 0, 475, 256),
        (384, 256, 64, 256, 0, 475, 256),
        (880, 384, 16, 64, 192, 478, 32),
        (880, 448, 16, 64, 224, 478, 32),
    ];
    for (i, t) in pack.textures.iter().enumerate() {
        let (ix, iy, iw, ih, cx, cy, cn) = expected[i];
        let img = &t.tim.image;
        assert_eq!((img.fb_x, img.fb_y), (ix, iy), "entry {i} image origin");
        assert_eq!((img.fb_w, img.h), (iw, ih), "entry {i} image size");
        let clut = t.tim.clut.as_ref().expect("entry has CLUT");
        assert_eq!((clut.fb_x, clut.fb_y), (cx, cy), "entry {i} CLUT origin");
        assert_eq!(
            clut.entries.len(),
            cn,
            "entry {i} CLUT colour count (w*h, uploaded as a flat strip)"
        );
    }

    // The three Vahn/Noa/Gala atlas pages tile the 4bpp texpage (832, 256):
    // 832 + 20 + 20 = 872, each 20 words wide.
    assert_eq!(pack.textures[1].tim.image.fb_x, 832, "Vahn page x");
    assert_eq!(pack.textures[2].tim.image.fb_x, 852, "Noa page x");
    assert_eq!(pack.textures[3].tim.image.fb_x, 872, "Gala page x");
    // Their CLUT strips occupy row 478 cols 0..191 (Vahn 0..63 / Noa 64..127 /
    // Gala 128..191), the columns the field meshes' CBA fields sample.
    for t in &pack.textures[1..4] {
        assert_eq!(t.tim.clut.as_ref().unwrap().fb_y, 478, "char CLUT row");
    }
}

#[test]
fn field_texture_upload_matches_pinned_vram() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let buf = read_prot_0874(&prot);
    let pack = field_char_textures::parse(&buf).expect("parse PROT 874 §2");

    // Field upload runs with STP = false (`_DAT_8007b998 == 0`).
    let mut vram = Vram::new();
    pack.upload_to_vram(&mut vram, false);

    // FNV of the full reconstructed VRAM. Deterministic from disc; byte-exact
    // vs a live field-scene VRAM dump over the union of the eight uploaded
    // rects. No Sony bytes are committed - only this digest.
    let digest = fnv1a64(vram.as_bytes());
    assert_eq!(
        digest, 0x64615c6915ba9a80,
        "reconstructed field-texture VRAM fingerprint (parse + flat-strip CLUT upload)"
    );

    // Sanity: the field char texpage (832, 256) and the player CLUT strip at
    // row 478 are populated.
    assert!(
        vram.region_has_data(832, 256, 64, 128),
        "field char texpage (832,256) populated"
    );
    assert!(
        vram.region_has_data(0, 478, 192, 1),
        "player CLUT strip at row 478 populated"
    );
}
