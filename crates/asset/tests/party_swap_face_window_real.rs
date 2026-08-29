//! Disc-gated oracle for `playerize::FACE_STAMP_WINDOWS`: the mirrored
//! per-character live-face reservation must equal the union of the eye +
//! mouth stamp destination rects in the disc's own `SCUS_942.54`
//! face-frame tables (`legaia_asset::face_anim`), and sit inside
//! section 1's texture rect (the tile the per-frame facial animator
//! stamps into).
//!
//! Skips silently when `extracted/SCUS_942.54` or `LEGAIA_DISC_BIN` is
//! missing.

use std::path::PathBuf;

use legaia_asset::battle_char_assembly::SECTION_TEXTURE_RECTS;
use legaia_asset::face_anim;
use legaia_asset::party_swap::playerize::FACE_STAMP_WINDOWS;

fn scus() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted/SCUS_942.54", "../../extracted/SCUS_942.54"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .and_then(|p| std::fs::read(p).ok())
}

#[test]
fn face_stamp_windows_match_the_scus_tables() {
    let Some(scus) = scus() else {
        eprintln!("[skip] extracted/SCUS_942.54 or LEGAIA_DISC_BIN missing");
        return;
    };
    let tables = face_anim::face_tables_from_scus(&scus).expect("face tables");
    let sec1 = SECTION_TEXTURE_RECTS[1];
    for (c, &(x0, y0, x1, y1)) in FACE_STAMP_WINDOWS.iter().enumerate() {
        let e = tables.eye_geo[c];
        let m = tables.mouth_geo[c];
        // Union of the two stamp rects, halfwords -> texels (4bpp).
        let ux0 = (e.dest_x.min(m.dest_x) as usize) * 4;
        let uy0 = e.dest_y.min(m.dest_y) as usize;
        let ux1 = ((e.dest_x + e.w as i16).max(m.dest_x + m.w as i16) as usize) * 4;
        let uy1 = (e.dest_y + e.h as i16).max(m.dest_y + m.h as i16) as usize;
        assert_eq!(
            (x0, y0, x1, y1),
            (ux0, uy0, ux1, uy1),
            "char {c}: FACE_STAMP_WINDOWS drifted from the SCUS tables"
        );
        // Inside section 1's rect (band texels; sec1 sits at band (0,0)).
        assert!(x1 <= sec1.w as usize * 4 && y1 <= sec1.h as usize);
    }
}
