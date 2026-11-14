//! Disc-gated census: monster action entries whose sound-cue track holds a
//! pair on or past the clip's frame count.
//!
//! The battle anim tick `FUN_80047430` hands the cue walk `FUN_800508DC` the
//! node cursor as the rate advance left it, ahead of the natural-end test, so
//! on the tick a clip ends the walk reads the frame count and such a pair
//! sounds as the clip ends. A walk that reads the clamped last keyframe never
//! reaches it. The anchor is the ropeway monster `0x53`'s get-up, entry 5,
//! whose one cue `(28, 0x17)` sits on its 28-keyframe count: the thud retail
//! plays as it stands at the end of `battle_ropeway_party_noa_sonic_javelin_open`.
//!
//! Skips when `extracted/PROT/0867_battle_data.BIN` or `LEGAIA_DISC_BIN` is
//! missing.

use legaia_actor_formats::monster_archive;
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn cue_pairs_on_a_clips_frame_count_are_census() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut at_end = 0usize;
    let mut past_end = 0usize;
    let mut entries = 0usize;
    for id in 1..=194u16 {
        let (Ok(Some(anims)), Ok(Some(tracks))) = (
            monster_archive::animations(&entry, id),
            monster_archive::animation_cue_tracks(&entry, id),
        ) else {
            continue;
        };
        assert_eq!(
            anims.len(),
            tracks.len(),
            "monster {id}: tracks index-align with clips"
        );
        for (anim, track) in anims.iter().zip(&tracks) {
            let count = anim.frame_count as u16;
            let mut hit = false;
            for &(frame, _) in track {
                if frame == count {
                    at_end += 1;
                    hit = true;
                } else if frame > count {
                    past_end += 1;
                }
            }
            entries += usize::from(hit);
        }
    }
    eprintln!("cue pairs on the frame count: {at_end} over {entries} entries; past it: {past_end}");

    let anims = monster_archive::animations(&entry, 0x53).unwrap().unwrap();
    let tracks = monster_archive::animation_cue_tracks(&entry, 0x53)
        .unwrap()
        .unwrap();
    let k = anims
        .iter()
        .position(|a| a.action_id == 5)
        .expect("monster 0x53 has action entry 5");
    assert_eq!(anims[k].frame_count, 28);
    assert_eq!(tracks[k], vec![(28, 0x17)]);
    // A disc invariant: four entries carry a pair on their frame count.
    assert_eq!((at_end, entries), (4, 4));
}
