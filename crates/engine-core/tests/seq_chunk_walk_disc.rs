//! The scene loader finds a stream's score by walking its DATA_FIELD chunk
//! list the way the retail installer `FUN_8001E54C` does (its type-2 arm is
//! the SEQ open), not by hunting the bytes for `pQES`. This pins that the walk
//! finds a score in every scene carrier the hunt did, at the same offset, so
//! the replacement changed no BGM resolution on the disc.
//!
//! Skips (and passes) when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::chunk_install::seq_chunk_offset;
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The byte hunt the walk replaced: first `pQES` past offset 0.
fn first_magic_past_zero(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .enumerate()
        .skip(1)
        .find(|(_, w)| *w == b"pQES")
        .map(|(i, _)| i)
}

#[test]
fn every_scene_score_resolves_through_the_installer_walk() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let (mut scenes, mut scores) = (0usize, 0usize);
    for name in &names {
        if host.load_scene(name).is_err() {
            continue;
        }
        let entries: Vec<_> = host
            .scene
            .as_ref()
            .expect("loaded")
            .entries
            .iter()
            .map(|e| (e.idx, e.class, e.bytes.clone()))
            .collect();
        let assets = host.assets().expect("loaded");
        let mut found = false;
        for (idx, class, bytes) in entries {
            if class == legaia_asset::categorize::Class::SeqContainer {
                continue;
            }
            let walked = seq_chunk_offset(&bytes);
            let hunted = first_magic_past_zero(&bytes);
            assert_eq!(
                walked, hunted,
                "{name}: entry {idx} - walk and hunt disagree"
            );
            if let Some(off) = walked {
                assert!(
                    assets.seq_in_stream_entries.contains(&(idx, off)),
                    "{name}: entry {idx} score at {off:#x} not surfaced"
                );
                found = true;
                scores += 1;
            }
        }
        scenes += usize::from(found);
    }
    eprintln!("[ok] {scores} stream scores over {scenes} scenes resolved by the installer walk");
    assert!(
        scores > 0,
        "no scene carried a stream score - the walk found nothing"
    );
}

/// Every global-pool track (`2000 + sound-test slot`) splits into its own
/// bank and score through the installer walk at exactly the offsets the old
/// two-magic hunt found, and the two agree on which tracks carry no pair at
/// all - so the play hosts' owned-VAB staging moved onto the walk without
/// moving a single track.
#[test]
fn every_music_bank_entry_splits_through_the_installer_walk() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let (mut paired, mut unpaired) = (0u32, Vec::new());
    let total = legaia_engine_core::music_labels::MUSIC_TRACK_COUNT;
    for slot in 0..total {
        let bgm_id = 2000 + slot as u16;
        let entry = host
            .music_bank_entry_bytes(bgm_id)
            .expect("read")
            .unwrap_or_else(|| panic!("bgm {bgm_id}: no bank entry"));
        let walked =
            legaia_engine_core::chunk_install::owned_bank_offsets(&entry).map(|s| (s.vab, s.seq));
        let hunted = entry.windows(4).position(|w| w == b"pBAV").and_then(|vab| {
            let rel = entry[vab..].windows(4).position(|w| w == b"pQES")?;
            Some((vab, vab + rel))
        });
        assert_eq!(walked, hunted, "bgm {bgm_id}: walk and hunt disagree");
        match walked {
            Some((_, seq)) => {
                legaia_seq::Seq::parse(&entry[seq..])
                    .unwrap_or_else(|e| panic!("bgm {bgm_id}: score does not parse: {e}"));
                paired += 1;
            }
            None => unpaired.push(bgm_id),
        }
    }
    assert!(paired > 0, "no track carried its own bank + score");
    eprintln!(
        "[ok] {paired}/{total} music-bank tracks split by the installer walk; \
         {} carry no [bank][score] pair (both readers agree): {unpaired:?}",
        unpaired.len()
    );
}
