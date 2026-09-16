//! Disc-gated: the play page composites the field fog sheets, off the same
//! pool the native window draws.
//!
//! The pool (`legaia_engine_core::fog_particles`) is one simulation on one
//! world; this file pins that the page's per-tick screen-prim assembly
//! (`tick_battle_intro` -> `tick_field_fog_prims`) reaches it: after the
//! scene's script raises the gate, `play_fog_stats()` reports drawn quads
//! and the screen-prim geometry the JS `ScreenPrimPass` uploads is
//! non-empty on a field frame - which no page test asserted before, because
//! the pass had only ever carried the field-to-battle transition.
//!
//! The scene is the first the census in
//! `crates/engine-shell/tests/w1h_fog_gate_census.rs` finds raising the gate
//! from its entry script; re-derived here from the same walk so the two
//! oracles cannot pick different scenes.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};
use legaia_web_viewer::runtime::LegaiaRuntime;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// First scene whose scene-controller record (`P1[0]`) carries an
/// **unconditional** `[4C, 0x30]` - no story-flag test, player box test or
/// conditional jump ahead of it in the record; falls back to any
/// unconditional raise. The same rule the native census uses.
fn entry_raising_scene(index: &ProtIndex) -> Option<String> {
    let mut fallback = None;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            let partitions = man_file.header.partition_counts.len();
            for partition in 0..partitions {
                let records = man_file.header.partition_counts[partition].max(0) as usize;
                for record in 0..records {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    let mut conditional_seen = false;
                    for insn in LinearWalker::new(body, pc0).flatten() {
                        match &insn.info {
                            InsnInfo::SystemFlag {
                                kind: legaia_engine_vm::field_disasm::FlagKind::Test,
                                ..
                            }
                            | InsnInfo::BBoxTest { .. }
                            | InsnInfo::CondJmp { .. } => conditional_seen = true,
                            _ => {}
                        }
                        let InsnInfo::MenuCtrl { op0: 0x30, .. } = insn.info else {
                            continue;
                        };
                        if insn.extended.is_some() || conditional_seen {
                            continue;
                        }
                        if partition == 1 && record == 0 {
                            return Some(name.clone());
                        }
                        fallback.get_or_insert_with(|| name.clone());
                    }
                }
            }
        }
    }
    fallback
}

#[test]
fn play_page_composites_fog_sheets_once_the_script_raises_the_gate() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = entry_raising_scene(&index).expect("a scene raises the fog gate");
    eprintln!("[fog page oracle] scene {scene}");
    rt.enter_field(&scene).expect("enter field scene");

    let mut first = None;
    let mut peak_quads = 0u32;
    let mut peak_live = 0u32;
    let mut prim_frames = 0u32;
    for f in 0..1800u32 {
        let _ = rt.tick_frame();
        let stats = rt.play_fog_stats();
        let (quads, live) = (stats[0], stats[1]);
        peak_quads = peak_quads.max(quads);
        peak_live = peak_live.max(live);
        if quads > 0 {
            first.get_or_insert(f);
            // The sheets ride the page's screen-prim pass: the geometry the
            // JS uploads carries them on this field frame.
            let n = rt.play_screen_prim_count();
            assert!(
                n >= quads,
                "tick {f}: {quads} fog quads but the screen-prim pass carries {n}"
            );
            assert!(
                !rt.play_screen_prim_vertex_bytes().is_empty(),
                "tick {f}: screen-prim geometry empty with fog up"
            );
            prim_frames += 1;
            if prim_frames >= 30 {
                break;
            }
        }
    }
    eprintln!(
        "[fog page oracle] first draw at tick {first:?}; peak quads/frame {peak_quads}; peak live {peak_live}"
    );
    assert!(
        first.is_some(),
        "the page never composited a fog sheet in 1800 ticks of {scene}"
    );
}
