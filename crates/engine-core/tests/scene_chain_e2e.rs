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
    let mut total_mes_messages = 0usize;
    let mut total_seq_entries = 0usize;
    let mut total_tmds = 0usize;
    let mut tmd_obj_count = 0u32;
    let mut seq_magic_ok = 0usize;
    let mut seq_magic_bad = 0usize;
    let mut sample_mes_text: Option<(String, u16, usize)> = None;

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
    }

    eprintln!(
        "[chain] scenes={} mes={} seq={} tmd={}",
        scene_names.len(),
        scenes_with_mes,
        scenes_with_seq,
        scenes_with_tmd
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
    if let Some((scene, id, len)) = &sample_mes_text {
        eprintln!(
            "[chain] sample MES: scene='{}' text_id={} bytes={}",
            scene, id, len
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
