//! Disc + library-gated: every sounding BGM voice of a retail mednafen state
//! (pitch register, ADSR words, left / right volume, reverb send) is one
//! the engine's sequencer programs for the same track.
//!
//! A mednafen `.mc` freezes one SPU cycle, so the comparison is a membership
//! test, not a frame alignment: for each retail voice the BGM sequence owns
//! (libsnd note record `+0x10 == 0x0001`) with a live envelope, some engine
//! voice over a pinned trace of the same track carries the identical
//! `(pitch, adsr, vol_left, vol_right, reverb_send)`. That pins, at once,
//! the pitch law, the tone ADSR words, the whole `FUN_80067550` volume chain
//! (CC7 folded into the velocity, the `107` sequence volume, three pan
//! stages, the square taper) and the per-tone reverb send - against retail's
//! register values (mednafen's sweep `Current` halved; see
//! `legaia_mednafen::spu`).
//!
//! The states are stereo ones whose sequence volume is the steady `107`
//! (a summon or Delilas mid-cast state catches a fade in progress) and
//! whose track is known.
//!
//! Skip-pass: `LEGAIA_DISC_BIN` unset, `extracted/` or the save library
//! missing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_mednafen::{SaveState, ScenarioManifest};
use legaia_parity::audio_trace_oracle::{
    AudioTraceBuildOptions, build_engine_audio_trace, load_runtime_audio_trace_from_save,
};

fn first_dir(cands: &[&str], probe: &str) -> Option<PathBuf> {
    cands
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join(probe).exists())
}

/// `(scenario label, BGM id)`.
const STATES: &[(&str, u16)] = &[
    ("title_screen_new_game", 2065),
    ("sebucus_overworld_resident", 2001),
    ("karisto_overworld_resident", 2001),
];

#[test]
fn retail_bgm_voices_match_the_engine_register_for_register() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir(
        &["extracted", "../extracted", "../../extracted"],
        "PROT.DAT",
    ) else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Some(root) = first_dir(&[".", "..", "../.."], "scripts/scenarios.toml") else {
        eprintln!("[skip] scripts/scenarios.toml missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(root.join("scripts/scenarios.toml")).unwrap();
    let lib = root.join("saves/library");
    let mut checked = 0usize;
    for &(label, bgm) in STATES {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            continue;
        };
        let Some(path) = manifest.library_save_path(scn, &lib).filter(|p| p.exists()) else {
            eprintln!("[skip] {label}: no library save");
            continue;
        };
        let state = SaveState::from_path(&path).expect("load state");
        let ram = state.main_ram().expect("main RAM");
        let rd16 = |va: u32| {
            let o = (va & 0x1F_FFFF) as usize;
            u16::from_le_bytes([ram[o], ram[o + 1]])
        };
        let retail = load_runtime_audio_trace_from_save(&path).expect("retail SPU");
        let opts = AudioTraceBuildOptions {
            scene: "town01".into(),
            bgm_id: Some(bgm),
            frames: 3000,
            pin_bgm: true,
            ..Default::default()
        };
        let engine = build_engine_audio_trace(&extracted, None, &opts).expect("engine trace");
        let programmed: BTreeSet<_> = engine
            .iter()
            .flat_map(|f| f.voices.iter())
            .filter(|v| v.active && v.env_level.unwrap_or(0) > 0)
            .map(|v| {
                (
                    v.pitch,
                    v.adsr_control,
                    v.vol_left,
                    v.vol_right,
                    v.reverb_send,
                )
            })
            .collect();
        for (i, v) in retail.voices.iter().enumerate() {
            let owner = rd16(0x801C_DB50 + i as u32 * 0x36 + 0x10);
            if owner != 0x0001 || v.env_level.unwrap_or(0) == 0 {
                continue;
            }
            let key = (
                v.pitch,
                v.adsr_control,
                v.vol_left,
                v.vol_right,
                v.reverb_send,
            );
            assert!(
                programmed.contains(&key),
                "{label} voice {i}: retail {key:?} is no voice the engine programs for {bgm}"
            );
            checked += 1;
        }
        eprintln!("[ran] {label}: retail BGM voices all programmed by the engine");
    }
    eprintln!("[ran] {checked} voices checked");
}
