//! Disc + library-gated: every sounding sequence voice of every retail
//! mednafen state in the library (pitch register, ADSR words, left / right
//! volume, reverb send) is one the engine's sequencer programs for the same
//! track at the same sequence volume.
//!
//! A mednafen `.mc` freezes one SPU cycle, so the comparison is a membership
//! test, not a frame alignment: for each retail voice a sequence owns
//! (libsnd note record `+0x10`, the owner key `seq | track << 8`, anything
//! but the cue slot `0x21`) with a live envelope, some engine voice over a
//! pinned trace of the same track carries the identical
//! `(pitch, adsr, vol_left, vol_right, reverb_send)`. That pins, at once,
//! the pitch law, the tone ADSR words, the whole `FUN_80067550` volume chain
//! (CC7 folded into the velocity, the sequence volume, three pan stages, the
//! square taper), libsnd's mono fold and the per-tone reverb send - against
//! retail's register values (mednafen's sweep `Current` halved; see
//! `legaia_mednafen::spu`).
//!
//! Everything the engine needs is read off the state, not off the scenario's
//! scene label:
//!
//! - **the track** - the owner key's sequence record
//!   (`*(0x801CD2C0 + seq * 4) + track * 0xB0`) holds the sequence start at
//!   `+0x04`; the `music_01` entry whose SEQ carries those bytes, and whose
//!   VAB header matches the one installed in the note's VAB slot (two tracks
//!   share one score), is the track;
//! - **the sequence volume** - the record's `+0x58`, which a summon duck
//!   lowers;
//! - **mono** - `_DAT_801CE330`.
//!
//! The same states pin the duck law: each playing record's `+0x58` is the
//! applied audio level `DAT_8007B6EC` halved, the volume
//! `legaia_engine_audio::duck::ducked_master_vol` hands the sequencer for
//! that level.
//!
//! The engine trace is long enough to sound every note of the longest
//! track. A state captured while a duck is still ramping reads its voices one
//! step behind the record's volume (the SPU registers trail the shadow
//! registers `FUN_80067E9C` writes by one flush), so a ducked record also
//! accepts the neighbouring volume; an un-ducked one does not.
//!
//! Skip-pass: `LEGAIA_DISC_BIN` unset, `extracted/` or the save library
//! missing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_audio::duck::ducked_master_vol;
use legaia_engine_audio::sequencer::RETAIL_BGM_SEQ_VOL;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::BootConfig;
use legaia_mednafen::SaveState;
use legaia_parity::audio_trace_oracle::{
    build_pinned_track_trace, load_runtime_audio_trace_from_save,
};
use legaia_ram_image::ScenarioManifest;

fn first_dir(cands: &[&str], probe: &str) -> Option<PathBuf> {
    cands
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join(probe).exists())
}

/// The trace cap: ten minutes at 60 Hz. Each trace stops once its track has
/// looped, so every note of the score sounds somewhere in it.
const FRAMES: u64 = 36_000;

/// The `music_01` id range searched for a state's track.
const TRACK_IDS: std::ops::RangeInclusive<u16> = 2000..=2120;

type VoiceKey = (
    Option<u16>,
    Option<u32>,
    Option<i16>,
    Option<i16>,
    Option<bool>,
);

#[test]
fn retail_sequence_voices_match_the_engine_register_for_register() {
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
    let lib = root.join("saves/library");
    if !lib.join("mednafen").is_dir() {
        eprintln!("[skip] saves/library/mednafen missing");
        return;
    }
    let manifest = ScenarioManifest::from_path(root.join("scripts/scenarios.toml")).unwrap();
    let session = NativeSession::open(
        &extracted,
        &BootConfig {
            scene: "town01".into(),
            enable_audio: false,
        },
    )
    .expect("open session");
    let tracks: Vec<(u16, Arc<Vec<u8>>)> = TRACK_IDS
        .filter_map(|id| Some((id, session.host.music_bank_entry_bytes(id).ok()??)))
        .collect();

    // (track, sequence volume, mono) -> every voice the engine programs.
    let mut programmed: BTreeMap<(u16, u8, bool), BTreeSet<VoiceKey>> = BTreeMap::new();
    let mut program = |id: u16, vol: u8, mono: bool| -> BTreeSet<VoiceKey> {
        programmed
            .entry((id, vol, mono))
            .or_insert_with(|| {
                let trace = build_pinned_track_trace(&session, id, vol, FRAMES, true)
                    .expect("engine trace")
                    .unwrap_or_default();
                trace
                    .iter()
                    .flat_map(|f| f.voices.iter())
                    .filter(|v| v.active && v.env_level.unwrap_or(0) > 0)
                    .map(|v| {
                        let (l, r) = if mono {
                            let m = v.vol_left.max(v.vol_right);
                            (m, m)
                        } else {
                            (v.vol_left, v.vol_right)
                        };
                        (v.pitch, v.adsr_control, l, r, v.reverb_send)
                    })
                    .collect()
            })
            .clone()
    };

    let mut checked = 0usize;
    let mut levels_checked = 0usize;
    let mut states = 0usize;
    let mut misses = Vec::new();
    for scn in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(scn, &lib).filter(|p| p.exists()) else {
            continue;
        };
        let Ok(state) = SaveState::from_path(&path) else {
            continue;
        };
        let Ok(ram) = state.main_ram() else {
            continue;
        };
        let at = |va: u32| (va & 0x1F_FFFF) as usize;
        let rd16 = |va: u32| u16::from_le_bytes([ram[at(va)], ram[at(va) + 1]]);
        let rd32 = |va: u32| {
            let o = at(va);
            u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
        };
        let retail = load_runtime_audio_trace_from_save(&path).expect("retail SPU");
        let mono = rd16(0x801C_E330) == 1;

        let mut by_owner = BTreeMap::<u16, Vec<usize>>::new();
        for (i, v) in retail.voices.iter().enumerate() {
            let owner = rd16(0x801C_DB50 + i as u32 * 0x36 + 0x10);
            if v.env_level.unwrap_or(0) > 0 && owner != 0x21 && owner != 0xFFFF {
                by_owner.entry(owner).or_default().push(i);
            }
        }
        if by_owner.is_empty() {
            continue;
        }
        states += 1;
        for (&owner, voices) in &by_owner {
            let record = rd32(0x801C_D2C0 + u32::from(owner & 0xFF) * 4)
                .wrapping_add(u32::from(owner >> 8) * 0xB0);
            let start = at(rd32(record + 4));
            let score = &ram[start..(start + 24).min(ram.len())];
            let vab_slot = u32::from(rd16(0x801C_DB50 + voices[0] as u32 * 0x36 + 0x18));
            let hdr = at(rd32(0x8009_1508 + vab_slot * 12));
            let vab_size = &ram[hdr + 12..hdr + 0x20];
            let Some(id) = tracks
                .iter()
                .filter(|(_, b)| score.len() == 24 && b.windows(24).any(|w| w == score))
                .find(|(_, b)| {
                    b.windows(4)
                        .position(|w| w == b"pBAV")
                        .and_then(|p| b.get(p + 12..p + 0x20))
                        == Some(vab_size)
                })
                .map(|(id, _)| *id)
            else {
                misses.push(format!(
                    "{}: owner {owner:#06x} plays no known track",
                    scn.label
                ));
                continue;
            };
            let vol = rd16(record + 0x58).min(127) as u8;
            // The duck law: the record's volume is the applied audio level
            // `DAT_8007B6EC` halved, which is what the port's duck hands the
            // sequencer for the same level.
            let level = rd32(0x8007_B6EC) as i32;
            if (0..=255).contains(&level) {
                levels_checked += 1;
                let want = ducked_master_vol(RETAIL_BGM_SEQ_VOL, level as u8);
                if want != vol {
                    misses.push(format!(
                        "{}: owner {owner:#06x} record volume {vol}, the port's duck gives \
                         {want} for applied level {level}",
                        scn.label
                    ));
                }
            }
            let mut sets = vec![program(id, vol, mono)];
            if vol != RETAIL_BGM_SEQ_VOL {
                sets.push(program(id, vol.saturating_sub(1), mono));
                sets.push(program(id, (vol + 1).min(127), mono));
            }
            for &i in voices {
                let v = retail.voices[i];
                let key = (
                    v.pitch,
                    v.adsr_control,
                    v.vol_left,
                    v.vol_right,
                    v.reverb_send,
                );
                checked += 1;
                if !sets.iter().any(|s| s.contains(&key)) {
                    misses.push(format!(
                        "{} voice {i}: retail {key:?} is no voice the engine programs for \
                         {id} at sequence volume {vol} (mono {mono})",
                        scn.label
                    ));
                }
            }
        }
    }
    eprintln!(
        "[ran] {checked} sequence voices over {states} states, {levels_checked} duck levels, \
         {} unmatched",
        misses.len()
    );
    assert!(misses.is_empty(), "{misses:#?}");
}
