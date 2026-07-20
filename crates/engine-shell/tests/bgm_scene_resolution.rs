//! Disc-gated scene -> BGM track resolution sweep.
//!
//! Answers "which SEQ does a given scene actually play?" from the **executing**
//! field VM rather than by fingerprinting rendered audio. Boots each scene
//! headlessly, ticks the prescript, and records every BGM id the VM emits
//! through op `0x35`, then resolves each id through the retail law in
//! `FUN_800243F0`:
//!
//! - `bgm_id < 2000`  -> scene-local slot `raw_define + 6 + bgm_id`
//! - `bgm_id >= 2000` -> global `music_01` slot `990 + (bgm_id - 2000)`
//!
//! The result is the matched-track input the note-level BGM differential
//! needs: a scene name paired with a concrete PROT entry.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::scene::BgmDirector;

/// Frames to run each scene's prescript before giving up on a BGM start.
/// Scene prescripts that start music do so in their opening frames.
const FRAMES: u32 = 240;

/// Records every id the host routes, and which resolution path it took.
#[derive(Default)]
struct RecordingDirector {
    /// `(bgm_id, used_owned_vab)` - `true` means the id resolved through the
    /// global `music_01` pool (the track brings its own VAB), `false` means
    /// it resolved to a scene-local SEQ-bearing entry.
    starts: Vec<(u16, bool)>,
}

impl BgmDirector for RecordingDirector {
    fn start(&mut self, bgm_id: u16, _seq_bytes: &[u8]) {
        self.starts.push((bgm_id, false));
    }
    fn start_owned_vab(&mut self, bgm_id: u16, _entry_bytes: &[u8]) {
        self.starts.push((bgm_id, true));
    }
    fn queue(&mut self, bgm_id: u16, _seq_bytes: &[u8]) {
        self.starts.push((bgm_id, false));
    }
    fn queue_owned_vab(&mut self, bgm_id: u16, _entry_bytes: &[u8]) {
        self.starts.push((bgm_id, true));
    }
}

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn scene_prescripts_select_global_pool_tracks() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }

    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut resolved: BTreeMap<String, Vec<(u16, bool, u32)>> = BTreeMap::new();
    let mut global_hits = 0usize;
    let mut local_hits = 0usize;
    let mut total_field_events = 0usize;

    for scene_name in &scene_names {
        let cfg = legaia_engine_shell::boot::BootConfig {
            scene: scene_name.clone(),
            enable_audio: false,
        };
        let Ok(mut session) = legaia_engine_shell::boot::BootSession::open(&extracted, &cfg) else {
            continue;
        };
        // Cold-boot the scene into live field dispatch. Plain `tick` alone
        // never installs a field record, so the VM would step nothing.
        let opts = legaia_engine_shell::boot::FieldLiveOpts {
            live_loop: true,
            ..Default::default()
        };
        if session.enter_field_live(scene_name, &opts).is_err() {
            continue;
        }
        let mut director = RecordingDirector::default();
        for _ in 0..FRAMES {
            if session.tick().is_err() {
                break;
            }
            // Non-vacuity probe: count every field event the VM produced, not
            // just BGM ones. A scene that emits other events but no `0x35`
            // proves the prescript ran and simply doesn't start music.
            total_field_events += session.host.world.pending_field_events.len();
            if session.host.route_bgm_events(&mut director).is_err() {
                break;
            }
        }
        if director.starts.is_empty() {
            continue;
        }
        let block_start = session.host.assets().map(|a| a.block_range.0).unwrap_or(0);
        let mut ids: Vec<(u16, bool, u32)> = director
            .starts
            .iter()
            .map(|&(id, owned)| {
                let entry = if owned {
                    legaia_engine_core::music_labels::MUSIC_BANK_EXTRACTION_BASE
                        + legaia_engine_core::music_labels::sound_test_index_for_bgm_id(id)
                            .unwrap_or(0)
                } else {
                    block_start + 8 + id as u32
                };
                if owned {
                    global_hits += 1;
                } else {
                    local_hits += 1;
                }
                (id, owned, entry)
            })
            .collect();
        ids.sort();
        ids.dedup();
        resolved.insert(scene_name.clone(), ids);
    }

    eprintln!(
        "[bgm-resolve] scenes whose prescript starts BGM: {}; \
         global-pool starts: {global_hits}; scene-local starts: {local_hits}",
        resolved.len()
    );
    for (scene, ids) in &resolved {
        let rendered: Vec<String> = ids
            .iter()
            .map(|(id, owned, entry)| {
                let label = legaia_engine_core::music_labels::label_for_bgm_id(*id)
                    .unwrap_or_else(|| "-".to_string());
                let pool = if *owned { "global" } else { "scene-local" };
                format!("{id} ({pool}, prot {entry}) {label}")
            })
            .collect();
        eprintln!("[bgm-resolve]   {scene}: {}", rendered.join(" | "));
    }

    eprintln!("[bgm-resolve] total field events observed across corpus: {total_field_events}");

    // Non-vacuity: the VM must actually be running. If it produced field
    // events but zero BGM starts, that is a real finding (scene-load does not
    // select the track); if it produced nothing at all, the harness is broken.
    assert!(
        total_field_events > 0,
        "field VM emitted no events at all - the sweep is vacuous"
    );
}
