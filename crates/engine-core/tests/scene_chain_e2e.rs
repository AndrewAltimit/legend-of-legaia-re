//! End-to-end: load every CDNAME scene, walk every typed asset class
//! (MES → message bytes; SEQ → header magic; TMD → object count), and
//! confirm the SceneHost composes them through the documented retail
//! offsets without panicking.
//!
//! This test catches whole-chain regressions that per-class unit tests
//! miss — it's the smoke that proves `SceneHost::open_extracted` →
//! `load_scene` → `SceneAssets::build` → `mes_message_bytes` /
//! `bgm_seq_bytes` / `tmds` all produce well-formed payloads on real
//! disc data.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

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

#[test]
fn scene_chain_resolves_mes_seq_tmd_across_corpus() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut scenes_with_mes = 0usize;
    let mut scenes_with_seq = 0usize;
    let mut scenes_with_tmd = 0usize;
    let mut scenes_with_vab = 0usize;
    let mut total_mes_messages = 0usize;
    let mut total_seq_entries = 0usize;
    let mut total_tmds = 0usize;
    let mut total_vab_entries = 0usize;
    let mut tmd_obj_count = 0u32;
    let mut seq_magic_ok = 0usize;
    let mut seq_magic_bad = 0usize;
    let mut vab_magic_ok = 0usize;
    let mut vab_parse_ok = 0usize;
    let mut vab_parse_bad = 0usize;
    let mut sample_mes_text: Option<(String, u16, usize)> = None;
    let mut sample_vab: Option<(String, u32, usize)> = None;

    for scene_name in &scene_names {
        if host.load_scene(scene_name).is_err() {
            continue;
        }
        let assets = host.assets().expect("scene loaded");

        if let Some(mes) = &assets.mes {
            scenes_with_mes += 1;
            let n = mes.message_count();
            total_mes_messages += n;
            // Probe: text_id 0 should resolve and have at least one byte.
            if let Some(bytes) = mes.message_bytes(0)
                && !bytes.is_empty()
                && sample_mes_text.is_none()
            {
                sample_mes_text = Some((scene_name.clone(), 0, bytes.len()));
            }
        }

        let total_seq = assets.seq_entries.len() + assets.seq_in_stream_entries.len();
        if total_seq > 0 {
            scenes_with_seq += 1;
            total_seq_entries += total_seq;
            for &seq_idx in assets.seq_entries.iter().take(2) {
                let bytes = host.index.entry_bytes(seq_idx).expect("read SEQ entry");
                if bytes.len() >= 4 && &bytes[..4] == b"pQES" {
                    seq_magic_ok += 1;
                } else {
                    seq_magic_bad += 1;
                }
            }
            for &(seq_idx, off) in assets.seq_in_stream_entries.iter().take(2) {
                let bytes = host.index.entry_bytes(seq_idx).expect("read SEQ entry");
                if bytes.len() >= off + 4 && &bytes[off..off + 4] == b"pQES" {
                    seq_magic_ok += 1;
                } else {
                    seq_magic_bad += 1;
                }
            }
        }

        if !assets.tmds.is_empty() {
            scenes_with_tmd += 1;
            total_tmds += assets.tmds.len();
            for tmd in &assets.tmds {
                tmd_obj_count += tmd.n_obj;
            }
        }

        // VAB pass: every `scene_vab_stream` entry must carry the `VABp`
        // magic and parse via `legaia_vab::parse` past the chunk header.
        // This catches regressions in the chunk-header offset math used
        // by `SceneAssets::build` and the BGM resolver.
        //
        // We also probe `seq_in_stream_entries` because most retail BGM
        // entries have a `[u32 chunk0 type=0][VAB][chunk1][SEQ]` layout —
        // chunk0 holds a VAB whose header sits at +4, even though the
        // entry's primary classification is the SEQ-bearing stream. The
        // classifier promotes such entries to `SceneVabStream`, but
        // border-cases (sub-sized VAB, version mismatch, non-canonical
        // `ps`/`ts`) can fail the strict detector and fall through. The
        // SEQ scanner, by contrast, just looks for the `pQES` magic, so
        // it surfaces every BGM entry. Probing both vectors gives the
        // full coverage the test wants.
        let mut probed_idxs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let probe_vab_at = |bytes: &[u8],
                            scene_name: &str,
                            entry_idx: u32,
                            counters: &mut (usize, usize, usize),
                            sample: &mut Option<(String, u32, usize)>| {
            // Try chunk0 wrapper offset (+4) first; fall back to offset 0
            // for raw vab_01-cluster entries.
            let off = 4usize;
            let (resolved_off, ok) = if bytes.len() >= off + 4 && &bytes[off..off + 4] == b"VABp" {
                (off, true)
            } else if bytes.len() >= 4 && &bytes[..4] == b"VABp" {
                (0, true)
            } else {
                (0, false)
            };
            if !ok {
                counters.2 += 1; // vab_parse_bad
                return;
            }
            counters.0 += 1; // vab_magic_ok
            match legaia_vab::parse(bytes, resolved_off) {
                Ok(rep) => {
                    counters.1 += 1; // vab_parse_ok
                    if sample.is_none() {
                        *sample = Some((scene_name.to_string(), entry_idx, rep.programs.len()));
                    }
                }
                Err(_) => counters.2 += 1,
            }
        };

        if !assets.vab_entries.is_empty() {
            scenes_with_vab += 1;
            total_vab_entries += assets.vab_entries.len();
            // Probe the first 2 entries per scene to keep walltime
            // bounded — coverage across hundreds of scenes is ample.
            for &vab_idx in assets.vab_entries.iter().take(2) {
                if !probed_idxs.insert(vab_idx) {
                    continue;
                }
                let bytes = host.index.entry_bytes(vab_idx).expect("read VAB entry");
                let mut counters = (vab_magic_ok, vab_parse_ok, vab_parse_bad);
                probe_vab_at(&bytes, scene_name, vab_idx, &mut counters, &mut sample_vab);
                vab_magic_ok = counters.0;
                vab_parse_ok = counters.1;
                vab_parse_bad = counters.2;
            }
        }

        // Stream-resident VAB probe: any SEQ-stream entry whose chunk0
        // carries a VAB header counts toward the test's coverage even if
        // the strict `scene_vab_stream` detector didn't claim it.
        for &(seq_idx, _seq_off) in assets.seq_in_stream_entries.iter().take(2) {
            if !probed_idxs.insert(seq_idx) {
                continue;
            }
            let bytes = host.index.entry_bytes(seq_idx).expect("read SEQ entry");
            // Only a VABp at chunk0 (+4) qualifies — raw SEQ at 0 doesn't
            // carry a VAB and shouldn't count.
            if bytes.len() >= 8 && &bytes[4..8] == b"VABp" {
                if scenes_with_vab == 0 || !assets.vab_entries.contains(&seq_idx) {
                    // Only bump the per-scene counter once.
                    if assets.vab_entries.is_empty() && scenes_with_vab == 0 {
                        scenes_with_vab += 1;
                    }
                }
                let mut counters = (vab_magic_ok, vab_parse_ok, vab_parse_bad);
                probe_vab_at(&bytes, scene_name, seq_idx, &mut counters, &mut sample_vab);
                vab_magic_ok = counters.0;
                vab_parse_ok = counters.1;
                vab_parse_bad = counters.2;
            }
        }
    }

    eprintln!(
        "[chain] scenes={} mes={} seq={} tmd={} vab={}",
        scene_names.len(),
        scenes_with_mes,
        scenes_with_seq,
        scenes_with_tmd,
        scenes_with_vab
    );
    eprintln!(
        "[chain] total_mes_messages={} total_seq={} (magic-ok={}, magic-bad={}) total_tmds={} total_objs={}",
        total_mes_messages,
        total_seq_entries,
        seq_magic_ok,
        seq_magic_bad,
        total_tmds,
        tmd_obj_count
    );
    eprintln!(
        "[chain] total_vab_entries={} (magic-ok={}, parse-ok={}, parse-bad={})",
        total_vab_entries, vab_magic_ok, vab_parse_ok, vab_parse_bad
    );
    if let Some((scene, id, len)) = &sample_mes_text {
        eprintln!(
            "[chain] sample MES: scene='{}' text_id={} bytes={}",
            scene, id, len
        );
    }
    if let Some((scene, idx, programs)) = &sample_vab {
        eprintln!(
            "[chain] sample VAB: scene='{}' entry={} programs={}",
            scene, idx, programs
        );
    }

    // The retail corpus has hundreds of scenes; we expect non-zero coverage
    // across every class. These bars are deliberately generous to keep the
    // test resilient to corpus drift.
    assert!(
        scenes_with_tmd > 50,
        "TMD coverage too low: {scenes_with_tmd}"
    );
    assert!(total_tmds > 1000, "total TMDs too low: {total_tmds}");
    assert!(scenes_with_seq > 0, "no SEQ-bearing scenes detected");
    // MES is rarer per scene — many scenes are pure asset bundles. The
    // important property is "we found *some* dialog containers and they
    // resolve to non-empty bytes".
    assert!(scenes_with_mes > 0, "no MES-bearing scenes detected");
    assert!(
        sample_mes_text.is_some(),
        "no MES container resolved a non-empty text_id 0"
    );
    // Every SEQ entry we probed must carry the pQES magic.
    assert_eq!(
        seq_magic_bad, 0,
        "{seq_magic_bad} SEQ entries failed the pQES magic check"
    );

    // VAB coverage: at least some scenes carry a VAB; every probe parsed.
    assert!(scenes_with_vab > 0, "no VAB-bearing scenes detected");
    assert!(
        vab_magic_ok > 0,
        "no VAB entries surfaced the VABp magic via SceneAssets::vab_entries"
    );
    assert_eq!(
        vab_parse_bad, 0,
        "{vab_parse_bad} VAB entries failed legaia_vab::parse — chunk-header offset math regressed?"
    );
    assert!(
        sample_vab.is_some(),
        "no VAB program list resolved to a non-empty programs vector"
    );
}

#[test]
fn scene_host_resolves_bgm_bytes_for_ids_in_block() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    // Walk scenes until we find one whose BGM resolver returns a real
    // entry, then verify the bytes it returns parse as pQES.
    let mut tried = 0usize;
    for scene_name in &scene_names {
        if host.load_scene(scene_name).is_err() {
            continue;
        }
        tried += 1;
        // Probe BGM ids 0..16 — the typical scene-local range.
        for id in 0..16u16 {
            if let Ok(Some(bytes)) = host.bgm_seq_bytes(id) {
                assert!(bytes.len() >= 4);
                assert_eq!(
                    &bytes[..4],
                    b"pQES",
                    "BGM bytes for scene='{scene_name}' id={id} don't carry pQES"
                );
                eprintln!(
                    "[bgm] scene='{scene_name}' id={id} resolved {} bytes",
                    bytes.len()
                );
                return;
            }
        }
        if tried > 200 {
            break;
        }
    }
    panic!("walked {tried} scenes without resolving any BGM id");
}
