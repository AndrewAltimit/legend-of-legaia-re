//! Disc-gated: the PROT-0900 screen-effect widget family layout pins against
//! the real overlay bytes.
//!
//! Validates, on the user's disc extraction, the structural constants the
//! [`legaia_engine_core::screen_fx`] port is built on:
//!
//! * the 5-entry sprite-script sub-op dispatch table at the overlay head
//!   (`jr *(0x801F69D8 + sub*4)`);
//! * the four 0x18-byte handler-binding descriptors (`FUN_80020DE0` input:
//!   `[u32 0][u16 0][u16 0xFFFF][u32 handler][u32 0]...`) at
//!   `0x801F8FE4/8FFC/9014/902C`, naming the four per-frame handlers
//!   `FUN_801F7A9C` (sprite) / `FUN_801F811C` (mask) / `FUN_801F849C` (panel)
//!   / `FUN_801F8A34` (letterbox);
//! * the mask handler `FUN_801F811C` body: prologue word, its four
//!   `jal FUN_8003D2C4` (addPrim) quad-emit sites, and a
//!   `jal FUN_801DE4C8` tween site.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_engine_core::screen_fx::{
    SCREEN_FX_DESCRIPTOR_VAS, SCREEN_FX_HANDLER_VAS, SCREEN_FX_LINK_BASE, SCREEN_FX_OVERLAY_PROT,
    SPRITE_SUBOP_ENTRY_VAS,
};
use legaia_prot::archive::Archive;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

fn u32_at(bytes: &[u8], va: u32) -> u32 {
    let off = (va - SCREEN_FX_LINK_BASE) as usize;
    u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

fn u16_at(bytes: &[u8], va: u32) -> u16 {
    let off = (va - SCREEN_FX_LINK_BASE) as usize;
    u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
}

/// MIPS `jal target` instruction word.
fn jal(target: u32) -> u32 {
    0x0C00_0000 | ((target & 0x0FFF_FFFF) >> 2)
}

#[test]
fn screen_fx_overlay_layout_pins_against_disc() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(SCREEN_FX_OVERLAY_PROT)
        .cloned()
        .expect("PROT 0900 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0900");
    assert!(
        bytes.len() >= 0x2660,
        "overlay must cover the widget family (got {} bytes)",
        bytes.len()
    );

    // 1. Sprite-script sub-op dispatch table at the overlay head.
    for (i, &expect) in SPRITE_SUBOP_ENTRY_VAS.iter().enumerate() {
        let got = u32_at(&bytes, SCREEN_FX_LINK_BASE + (i as u32) * 4);
        assert_eq!(
            got, expect,
            "dispatch entry {i}: got {got:#010x}, expected {expect:#010x}"
        );
    }

    // 2. Handler-binding descriptors: [u32 0][u16 0xFFFF][u16 0][u32 handler][u32 0].
    for (&desc, &handler) in SCREEN_FX_DESCRIPTOR_VAS
        .iter()
        .zip(SCREEN_FX_HANDLER_VAS.iter())
    {
        assert_eq!(u32_at(&bytes, desc), 0, "descriptor {desc:#x} word 0");
        assert_eq!(u16_at(&bytes, desc + 4), 0, "descriptor {desc:#x} +4");
        assert_eq!(
            u16_at(&bytes, desc + 6),
            0xFFFF,
            "descriptor {desc:#x} sentinel halfword"
        );
        assert_eq!(
            u32_at(&bytes, desc + 8),
            handler,
            "descriptor {desc:#x} handler pointer"
        );
        assert_eq!(
            u32_at(&bytes, desc + 0xc),
            0,
            "descriptor {desc:#x} flags word"
        );
    }

    // 3. Mask handler FUN_801F811C body shape.
    let mask = SCREEN_FX_HANDLER_VAS[1];
    assert_eq!(
        u32_at(&bytes, mask),
        0x27BD_FFC0,
        "FUN_801F811C prologue (addiu sp,sp,-0x40)"
    );
    // The four border-quad addPrim links (FUN_8003D2C4).
    for va in [0x801F_8340u32, 0x801F_83B4, 0x801F_841C, 0x801F_8478] {
        assert_eq!(
            u32_at(&bytes, va),
            jal(0x8003_D2C4),
            "quad-emit addPrim site {va:#x}"
        );
    }
    // One per-axis tween site (the multi-mode interpolator FUN_801DE4C8).
    assert_eq!(
        u32_at(&bytes, 0x801F_81D4),
        jal(0x801D_E4C8),
        "edge tween lerp site"
    );

    eprintln!(
        "[ok] PROT 0900 widget-family layout pinned: 5 dispatch entries, \
         4 handler descriptors, FUN_801F811C quad-emit + tween sites"
    );
}
