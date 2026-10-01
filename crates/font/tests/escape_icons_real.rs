//! The `0xCE` escape sprites decoded from the real `PROT.DAT` +
//! `SCUS_942.54`. Skips and passes when `extracted/` is absent
//! (`LEGAIA_EXTRACTED_DIR` overrides the workspace path).

use legaia_font::escape_icons::{
    ESCAPE_COUNT, EscapeIcons, ICON_PROT_DAT_LEN, ICON_PROT_DAT_OFFSET,
};
use std::path::PathBuf;

fn extracted_root() -> Option<PathBuf> {
    let root = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../extracted")
                .to_path_buf()
        });
    (root.join("PROT.DAT").is_file() && root.join("SCUS_942.54").is_file()).then_some(root)
}

#[test]
fn every_string_escape_draws_an_opaque_sprite() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/PROT.DAT + SCUS_942.54 not present");
        return;
    };
    let prot = std::fs::read(root.join("PROT.DAT")).unwrap();
    let scus = std::fs::read(root.join("SCUS_942.54")).unwrap();
    let o = ICON_PROT_DAT_OFFSET as usize;
    let icons = EscapeIcons::from_disc(&prot[o..o + ICON_PROT_DAT_LEN], &scus).unwrap();
    assert_eq!(icons.icons.len(), ESCAPE_COUNT);
    for i in 0..ESCAPE_COUNT as u8 {
        let numeric = (0x0B..=0x0E).contains(&i);
        match icons.get(i) {
            None => assert!(numeric, "escape {i:#04x} has no sprite"),
            Some(ic) => {
                assert!(!numeric, "numeric escape {i:#04x} decoded a sprite");
                let opaque = ic
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] != 0)
                    .count();
                assert!(
                    opaque * 8 >= (ic.w * ic.h) as usize,
                    "{i:#04x} mostly empty"
                );
            }
        }
    }
    // The buttons are 16x16 and sit 2 px above the line; the winged element
    // icons are 28x12.
    let x = icons.get(0x00).unwrap();
    assert_eq!((x.w, x.h, x.y_offset), (16, 16, -2));
    let fire2 = icons.get(0x1D).unwrap();
    assert_eq!((fire2.w, fire2.h), (28, 12));
    eprintln!("[ran] {} escape sprites decoded", ESCAPE_COUNT - 4);
}
