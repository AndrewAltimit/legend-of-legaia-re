//! Disc-gated: the site's sound-cue chain must resolve off a real disc.
//!
//! Walks exactly what `site/js/legaia-sfx.js` walks - SCUS -> the static SFX
//! descriptor table, PROT 869 (the class-2 sound bank the battle scene loader
//! `FUN_800520F0` and the Baka init `FUN_801CF00C` both load) -> a VAB, each
//! cue through the clean-room SPU - and asserts every cue the pages fire
//! renders to **audible** PCM (a non-zero peak), not just "decodes without
//! error".
//!
//! Structural facts only - no Sony bytes asserted. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::arts_view::LegaiaArts;
use legaia_web_viewer::sfx_view::{
    CUE_ART_STRIKE, CUE_CANCEL, CUE_CONFIRM, CUE_CURSOR, CUE_HIT, LegaiaSfx, SFX_BANK_PROT_INDEX,
};

fn disc() -> Option<Vec<u8>> {
    std::fs::read(std::env::var("LEGAIA_DISC_BIN").ok()?).ok()
}

fn loaded() -> Option<(LegaiaSfx, serde_json::Value)> {
    let bytes = disc()?;
    let mut sfx = LegaiaSfx::new();
    let info: serde_json::Value = serde_json::from_str(&sfx.load_disc(bytes).ok()?).ok()?;
    Some((sfx, info))
}

#[test]
fn every_site_cue_renders_to_audible_pcm_from_the_class2_bank() {
    let Some((sfx, info)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };

    // The bank is the traced class-2 sound bank, not a fallback.
    assert_eq!(
        info["bank"].as_u64().unwrap(),
        SFX_BANK_PROT_INDEX as u64,
        "cues resolve in the class-2 sound bank (PROT {SFX_BANK_PROT_INDEX})"
    );
    assert_eq!(info["rate"].as_u64().unwrap(), 44100);

    // Every cue the two pages fire: the four the duel overlay writes into the
    // cue ring, plus the arts strike cue.
    for (id, what) in [
        (CUE_HIT, "duel hit (FUN_801D3B18)"),
        (CUE_CONFIRM, "menu confirm"),
        (CUE_CURSOR, "menu cursor / tally tick"),
        (CUE_CANCEL, "menu cancel"),
        (CUE_ART_STRIKE, "art strike"),
    ] {
        let pcm = sfx.cue_pcm_i16(id as u32);
        let peak = sfx.cue_peak(id as u32);
        assert!(!pcm.is_empty(), "cue 0x{id:02X} ({what}) renders some PCM");
        assert_eq!(pcm.len() % 2, 0, "interleaved stereo");
        assert!(peak > 0, "cue 0x{id:02X} ({what}) is audible, not silence");
        // The rendered buffer's own peak must agree with the reported one -
        // the page stages its gain off it.
        let measured = pcm.iter().map(|s| s.unsigned_abs() as u32).max().unwrap();
        assert_eq!(measured, peak, "cue 0x{id:02X} peak matches its PCM");
        // A one-shot, not a stuck sustain: well under the 2 s render cap.
        let secs = (pcm.len() / 2) as f32 / 44100.0;
        assert!(secs < 2.0, "cue 0x{id:02X} is a one-shot ({secs:.2}s)");
    }
}

#[test]
fn event_names_resolve_to_the_traced_cue_ids() {
    let Some((sfx, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The duel's four disc-traced ring writes.
    assert_eq!(sfx.cue_for_event("baka", "hit"), CUE_HIT as u32);
    assert_eq!(sfx.cue_for_event("baka", "confirm"), CUE_CONFIRM as u32);
    assert_eq!(sfx.cue_for_event("baka", "cursor"), CUE_CURSOR as u32);
    assert_eq!(sfx.cue_for_event("baka", "cancel"), CUE_CANCEL as u32);
    // The tally tick is the cursor blip (FUN_801D239C writes the same id).
    assert_eq!(sfx.cue_for_event("baka", "tally"), CUE_CURSOR as u32);
    assert_eq!(sfx.cue_for_event("arts", "strike"), CUE_ART_STRIKE as u32);

    // Provenance is carried per event, and only the cues retail actually
    // fires are labelled disc-sourced.
    let map: serde_json::Value = serde_json::from_str(&sfx.baka_cues_json()).unwrap();
    for ev in ["hit", "confirm", "cursor", "cancel", "tally"] {
        assert_eq!(map[ev]["source"], "disc", "{ev} is a traced ring write");
    }
    for ev in ["round_start", "match_lose"] {
        assert_eq!(map[ev]["source"], "site", "{ev} has no retail cue");
    }
}

#[test]
fn every_decodable_art_clip_yields_strike_frames_inside_the_clip() {
    let Some(bytes) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut arts = LegaiaArts::new();
    arts.load_disc(bytes).expect("arts disc load");

    let mut clips = 0usize;
    for cslot in 0..3u32 {
        let st: serde_json::Value = serde_json::from_str(&arts.set_character(cslot)).unwrap();
        assert_eq!(st["ok"], true, "character {cslot} assembles");
        assert_eq!(arts.art_strike_cue(), CUE_ART_STRIKE as u32);

        for a in st["arts"].as_array().unwrap() {
            if a["ok"] != true {
                continue;
            }
            let idx = a["index"].as_u64().unwrap() as u32;
            let frames = a["frames"].as_u64().unwrap() as u32;
            let strikes = arts.art_strike_frames(idx);
            clips += 1;
            assert!(
                !strikes.is_empty(),
                "art {idx} (char {cslot}) has at least one impact frame"
            );
            assert!(
                strikes.len() <= 4,
                "art {idx} fires at most the record's four hits"
            );
            for f in &strikes {
                assert!(
                    *f < frames,
                    "impact frame {f} is inside the {frames}-frame clip"
                );
            }
            // Ascending + unique - the page keys a Set of clip frames on them.
            assert!(strikes.windows(2).all(|w| w[0] < w[1]), "ascending");
        }
    }
    assert!(
        clips > 30,
        "the three characters' art banks decoded ({clips})"
    );
}
