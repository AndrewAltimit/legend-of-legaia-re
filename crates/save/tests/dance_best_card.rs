//! Save-library-gated: the dance hall's best score `_DAT_80084464` (SC block
//! `0x324`) on the playthrough cards. The ladder's saves read `0` through
//! `PRO-09` (`kor5`) and the score of the run danced before `PRO-10`
//! (`korb2`) from there on, and the engine's lift reads it into
//! [`legaia_save::MinigameSave::dance_best`]. Skips (and passes) when the
//! library is absent.

use std::path::{Path, PathBuf};

const LADDER: &str = "playthrough-ladder-pro00-14.mcr";

fn library() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("saves/library");
    std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .into_iter()
        .chain([root])
        .find(|d| d.join("cards").join(LADDER).exists())
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect()
}

/// The SC block of the ladder save whose name ends in `save`.
fn card_sc(library: &Path, save: &str) -> Vec<u8> {
    let mounted = legaia_save::emu::MountedCard::open(&library.join("cards").join(LADDER))
        .expect("mount card");
    (1..=15u8)
        .find_map(|block| {
            let frame = mounted.dir_frame(block)?;
            if !ascii(&frame[0x0A..0x0A + 20]).ends_with(save) {
                return None;
            }
            mounted.sc_block(block).map(<[u8]>::to_vec)
        })
        .unwrap_or_else(|| panic!("{LADDER} has a save named {save}"))
}

#[test]
fn the_ladder_cards_carry_the_dance_best_from_korb2_on() {
    let Some(library) = library() else {
        eprintln!("[skip] save library absent");
        return;
    };
    for (save, best) in [("PRO-09", 0u32), ("PRO-10", 568), ("PRO-14", 568)] {
        let sc = card_sc(&library, save);
        let lifted = legaia_save::SaveFile::from_retail_sc_block(&sc, 1).expect("lift the block");
        assert_eq!(lifted.ext.minigames.dance_best, best, "{save}");
    }
}
