//! Save-library-gated: the retail play order of the region story-flag gate
//! families, bracketed by consecutive card saves.
//!
//! A card save is a retail snapshot of the system-flag bank
//! (`0x80085758`, the story-flag window's `+0x158..`), and the two
//! playthrough cards hold one save per story milestone. A flag clear in one
//! save and set in the next was written by the play between them - so each
//! row below places a gate family's writes between two named milestones.
//! It does not order the writes *inside* one bracket; where a gate fixes
//! that order, `engine-core`'s `region_gate_in_bracket_write_order` pins it
//! from the MAN bytes.
//!
//! See `docs/reference/open-rev-eng-threads.md` § Region story-flag gate
//! families. Skips (and passes) when the library is absent.

use std::path::{Path, PathBuf};

use legaia_save::card::{read_retail_scene_label, read_retail_story_flags};

const LADDER: &str = "playthrough-ladder-pro00-14.mcr";
const ENDGAME: &str = "playthrough-endgame-7saves.mcr";

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
        .find(|d| d.join("cards").join(LADDER).exists() && d.join("cards").join(ENDGAME).exists())
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect()
}

/// The SC block of the card save whose name ends in `save`.
fn card_sc(library: &Path, card: &str, save: &str) -> Vec<u8> {
    let mounted =
        legaia_save::emu::MountedCard::open(&library.join("cards").join(card)).expect("mount card");
    (1..=15u8)
        .find_map(|block| {
            let frame = mounted.dir_frame(block)?;
            if !ascii(&frame[0x0A..0x0A + 20]).ends_with(save) {
                return None;
            }
            mounted.sc_block(block).map(<[u8]>::to_vec)
        })
        .unwrap_or_else(|| panic!("{card} has a save named {save}"))
}

/// One milestone save: `(card, save name, the scene it was saved in)`.
type Save = (&'static str, &'static str, &'static str);

const TEIEN: Save = (LADDER, "PRO-04", "teien");
const TUNNELB: Save = (LADDER, "PRO-05", "tunnelb");
const RETONA: Save = (LADDER, "PRO-06", "retona");
const RETOCK: Save = (LADDER, "PRO-07", "retock");
const DOHATY: Save = (LADDER, "PRO-08", "dohaty");
const KOR5: Save = (LADDER, "PRO-09", "kor5");
const KORB2: Save = (LADDER, "PRO-10", "korb2");
const DOMAN: Save = (LADDER, "PRO-11", "doman");
const NILBOA: Save = (LADDER, "PRO-13", "nilboa");
const TAIKU: Save = (LADDER, "PRO-14", "taiku");
const CONC: Save = (ENDGAME, "PRO-00", "conc");
const RUGI: Save = (ENDGAME, "PRO-01", "rugi");
const CHITEI2: Save = (ENDGAME, "PRO-02", "chitei2");

/// `(flags, clear in, set in)`.
const BRACKETS: &[(&[u16], Save, Save)] = &[
    // rayman's chain lands whole between Sky Gardens and the Fire Path.
    (&[0x201, 0x1FB, 0x200, 0x1FC], TEIEN, TUNNELB),
    // rayman2's extra gate.
    (&[0x1D5], TUNNELB, RETONA),
    // retock: 0x357 first, then jagaroom's 0x33B and Eliza's 0x502 together.
    (&[0x357], RETONA, RETOCK),
    (&[0x502, 0x33B], RETOCK, DOHATY),
    // bubu2's requires-all list splits across two brackets.
    (&[0x608], DOHATY, KOR5),
    (&[0x3D3, 0x609], KORB2, DOMAN),
    // The kor5 tail.
    (&[0x436, 0x43A, 0x6C4], KOR5, KORB2),
    // doman's P2[4] one-shot.
    (&[0x3FB], KORB2, DOMAN),
    // Nivora's SET.
    (&[0x370], DOMAN, (LADDER, "PRO-12", "nilboa")),
    // son's arrival family and the Nivora-side 0x378.
    (&[0x378, 0x3A6, 0x60D], NILBOA, TAIKU),
    // taiku's 0x38F (gates station / station3) and son's 0x3A7.
    (&[0x38F, 0x3A7], TAIKU, CONC),
    // deroa's one-shot group and its 0x3E1 descent gate.
    (&[0x3E1, 0x46D, 0x46E, 0x46F], RUGI, CHITEI2),
];

#[test]
fn region_gate_families_land_between_consecutive_milestone_saves() {
    let Some(lib) = library() else {
        eprintln!("[skip] save library missing (set LEGAIA_SAVES_LIBRARY)");
        return;
    };
    let flag = |save: Save, f: u16| -> bool {
        let (card, name, scene) = save;
        let sc = card_sc(&lib, card, name);
        assert_eq!(
            read_retail_scene_label(&sc).as_deref(),
            Some(scene),
            "{card} {name} is the {scene} save"
        );
        let sys = &read_retail_story_flags(&sc).expect("flag window")[0x158..];
        sys[usize::from(f >> 3)] & (0x80 >> (f & 7)) != 0
    };
    for (flags, before, after) in BRACKETS {
        for &f in flags.iter() {
            assert!(!flag(*before, f), "0x{f:X} still clear at {}", before.2);
            assert!(flag(*after, f), "0x{f:X} set by {}", after.2);
        }
    }
    eprintln!("[ran] {} card brackets", BRACKETS.len());
}
