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

/// The effect-script walk (`FUN_801DEA50`) and the hit walk (`FUN_801EC3E4`)
/// fire a gate `g` once `frame + 1 >= g`, so the clamped last keyframe
/// already reaches a gate on the frame count; the crossing tick's advanced
/// cursor reaches one past it (`count + 1`), and a step over a keyframe a
/// pass one further. Lists, per walk, the gates only the crossing reaches.
#[test]
fn effect_and_hit_gates_past_the_frame_count_are_census() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut effects = Vec::new();
    let mut hits = Vec::new();
    for id in 1..=194u16 {
        let Ok(Some(anims)) = monster_archive::animations(&entry, id) else {
            continue;
        };
        for (k, anim) in anims.iter().enumerate() {
            let count = anim.frame_count as u32;
            let head = &anim.effect_script;
            // The effect walk stops on the first gate it defers and on a
            // zero gate, so only an in-order run of records is reachable.
            for r in 0..8 {
                let Some(&gate) = head.get(0x14 + r * 8) else {
                    break;
                };
                if gate == 0 {
                    break;
                }
                if u32::from(gate) > count {
                    effects.push((id, k, anim.action_id, count, gate, head[0x14 + r * 8 + 1]));
                }
            }
            // The hit walk takes the four event frames in index order and a
            // zero frame ends the run.
            for s in 0..4 {
                let Some(&f) = head.get(0x10 + s) else {
                    break;
                };
                if f == 0 {
                    break;
                }
                if u32::from(f) > count {
                    hits.push((id, k, anim.action_id, count, f));
                }
            }
        }
    }
    eprintln!("effect gates past the count (id, entry, tag, count, gate, effect): {effects:?}");
    eprintln!("hit frames past the count (id, entry, tag, count, frame): {hits:?}");

    // The crossing tick reads the count, or one past it on a pass that
    // steps two keyframes, so it reaches a gate of `count + 1` (`count + 2`
    // at most). Disc invariants: the get-up of monsters 0x53..0x55 spawns
    // its three effect-`130` records at gate 29 over 28 keyframes, 0x4B's
    // entry 3 one effect-`133` record, and 0xB5's walk (entry 1, 12
    // keyframes) one effect-`131` record at gate 13 each cycle.
    let reached: Vec<_> = effects
        .iter()
        .filter(|e| u32::from(e.4) == e.3 + 1)
        .collect();
    let mut ids: Vec<u16> = reached.iter().map(|e| e.0).collect();
    ids.dedup();
    assert_eq!(ids, vec![75, 83, 84, 85, 181]);
    assert_eq!(reached.len(), 11);
    assert!(
        effects
            .iter()
            .filter(|e| (83..=85).contains(&e.0))
            .all(|e| (e.1, e.3, e.4, e.5) == (5, 28, 29, 130))
    );
    // No hit frame is in reach: each stands three or more keyframes past its
    // count, so the port's damage timing is the clamp's.
    assert!(hits.iter().all(|h| u32::from(h.4) > h.3 + 2));
    assert_eq!(hits.len(), 4);
}
