//! Disc-gated regression for [`legaia_asset::interior_page`]: the shared
//! interior texture page in `PROT.DAT`'s unindexed head gap.
//!
//! Pins that the family TIM parses at its fixed byte offset with the
//! documented rects (image `(960,256)` 64×256, CLUT block `(0,510)`
//! 16×16), and that the flat-strip upload populates the row-510 CLUT
//! cells town01's env meshes reference (CBA `(64,510)` = strip entry 64).
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `PROT.DAT` is
//! missing.

use std::path::PathBuf;

use legaia_asset::interior_page;

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

#[test]
fn interior_page_tim_at_pinned_gap_offset() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let tim = interior_page::read_from_prot_dat(&prot).expect("gap TIM parses");
    let clut = tim.clut.as_ref().expect("has CLUT block");
    assert_eq!(
        (clut.fb_x, clut.fb_y, clut.w, clut.h),
        interior_page::CLUT_RECT
    );
    assert_eq!(
        (tim.image.fb_x, tim.image.fb_y, tim.image.fb_w, tim.image.h),
        interior_page::IMAGE_RECT
    );

    let mut vram = legaia_tim::Vram::new();
    interior_page::upload_to_vram(&tim, &mut vram);
    // The strip populates row 510 out to x=256 (223 of the 256 entries are
    // non-zero on the disc base). The town01 CBA (64,510) selects the
    // palette whose base sits at strip entry 64: its own entry 0 is the
    // conventional transparent black, so assert the FOLLOWING entries.
    let lit = (0..256).filter(|&x| vram.pixel(x, 510) != 0).count();
    assert!(lit > 200, "row-510 strip mostly populated (lit={lit})");
    let pal_lit = (65..80).filter(|&x| vram.pixel(x, 510) != 0).count();
    assert!(
        pal_lit >= 12,
        "palette at strip entry 64 populated (lit={pal_lit}/15)"
    );
    // The image block lands at its declared rect.
    assert!(vram.region_has_data(960, 256, 64, 256));
}
