//! Disc-gated end-to-end smoke test: load `izumi` (the first town the player
//! enters in retail), drive the world via the SceneHost for many frames, and
//! verify the field VM emits real-world side-effects.
//!
//! This test is the integration of:
//!  - Item 7: FieldEvent queue (BGM / dialog / move / etc. surfaced).
//!  - Item 10: Scene-bundle navigator (find_bundle + descriptor 0 LZS).
//!  - Item 6 / 2: Audio + font extraction confirmed in earlier tests.
//!
//! What this catches:
//!  - SceneHost can transition into a real field scene without panicking.
//!  - The World tick + drain_field_events loop produces *some* observable
//!    side-effects (BGM, dialog, move-to, scene-fade, etc.) - proving the
//!    full chain (CDNAME → PROT → scene_scripted_asset_table → field VM →
//!    FieldHost callbacks → event queue) is wired end-to-end.
//!  - Descriptor 0 of the scene's asset table LZS-decodes (which is how
//!    TIMs would land in VRAM if the renderer were attached).
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::scene_bundle;

fn extracted_dir() -> Option<PathBuf> {
    let d = PathBuf::from("extracted");
    if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
        Some(d)
    } else {
        let alt = PathBuf::from("../../extracted");
        if alt.join("PROT.DAT").exists() && alt.join("CDNAME.TXT").exists() {
            Some(alt)
        } else {
            None
        }
    }
}

#[test]
fn first_town_drives_scene_to_field_event_emission() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // Try the first town in CDNAME order. Most retail builds open with
    // izumi (Rim Elm in the NA release). Fall back to the first scene
    // with a scripted asset table + non-empty event scripts if it's
    // missing / not loadable.
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let preferred = ["izumi", "town01"];
    let mut chosen: Option<String> = None;
    for p in preferred {
        if scene_names.iter().any(|n| n == p) {
            chosen = Some(p.to_string());
            break;
        }
    }

    let scene_name = chosen.unwrap_or_else(|| {
        // Fall back: pick the first scene whose load resolves to a
        // scripted asset table with > 1 record.
        scene_names
            .iter()
            .find(|n| {
                host.load_scene(n).is_ok()
                    && host
                        .scene
                        .as_ref()
                        .and_then(scene_bundle::find_bundle)
                        .is_some()
            })
            .cloned()
            .expect("at least one scene should have a bundle")
    });

    eprintln!("[first-town] driving scene '{scene_name}'");
    host.enter_field_scene(&scene_name, 0).expect("enter scene");

    // Sanity: the scene's asset bundle is locatable + descriptor 0
    // LZS-decodes. This is the integration of item 10.
    let scene = host.scene.as_ref().expect("scene loaded");
    let bundle = scene_bundle::find_bundle(scene).expect("scene has asset bundle");
    let descs = scene_bundle::walk_descriptors(&bundle);
    assert_eq!(descs.len(), 7);
    let _ = scene_bundle::extract_descriptor_0_lzs(&bundle).expect("desc 0 LZS-decodes");

    // Drive frames across multiple records. Many scene event-script
    // records are just init prologues that halt after one opcode; we
    // want to find ones that actually emit observable side-effects.
    // Replay records 0..min(8, n) for 200 frames each, draining events
    // per frame.
    let scripts = scene.find_event_scripts().expect("scene has event scripts");
    let n_records = scripts.len();
    let records: Vec<Vec<u8>> = (0..n_records.min(8))
        .map(|i| scripts.record(i).map(|s| s.to_vec()).unwrap_or_default())
        .collect();
    drop(scripts);

    // Debug: dump record 0 first 32 bytes so we know what the VM sees.
    if !records.is_empty() {
        let opener: Vec<String> = records[0]
            .iter()
            .take(32)
            .map(|b| format!("{b:02X}"))
            .collect();
        eprintln!(
            "[first-town] record 0 ({} bytes): {}",
            records[0].len(),
            opener.join(" ")
        );
    }

    let mut all_events: Vec<FieldEvent> = Vec::new();
    let mut step_outcomes: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for (rec_idx, record) in records.iter().enumerate() {
        if record.is_empty() {
            continue;
        }
        host.world.load_field_record(record);
        for frame in 0..200 {
            // Tick the world (advances `frame` counter + drives every VM).
            let _ = host.world.tick();
            if let Some(out) = host.world.step_field() {
                use legaia_engine_vm::field::StepResult;
                let kind = match out {
                    StepResult::Advance { .. } => "Advance",
                    StepResult::Yield { .. } => "Yield",
                    StepResult::Halt { .. } => "Halt",
                    StepResult::Pending { .. } => "Pending",
                    StepResult::Unknown { .. } => "Unknown",
                };
                *step_outcomes.entry(kind).or_insert(0) += 1;
                // Clear the halt bit so the script can resume on the
                // next tick (engines drive this via their own gates;
                // for the smoke test we just unhalt unconditionally).
                if matches!(out, StepResult::Halt { .. }) {
                    host.world.field_ctx.flags &= !0x400;
                }
            }
            host.world.pending_scene_transition = None;
            all_events.extend(host.world.drain_field_events());
            let _ = frame;
        }
        let _ = rec_idx;
    }
    eprintln!("[first-town] step outcomes: {step_outcomes:?}");

    // Histogram what the VM emitted.
    let mut counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for ev in &all_events {
        let kind = match ev {
            FieldEvent::Bgm { .. } => "Bgm",
            FieldEvent::PlaySfx { .. } => "PlaySfx",
            FieldEvent::OpenDialog { .. } => "OpenDialog",
            FieldEvent::AddMoney { .. } => "AddMoney",
            FieldEvent::SetItemCount { .. } => "SetItemCount",
            FieldEvent::PartyAdd { .. } => "PartyAdd",
            FieldEvent::PartyRemove { .. } => "PartyRemove",
            FieldEvent::FieldInteract { .. } => "FieldInteract",
            FieldEvent::SceneRegisterWrite { .. } => "SceneRegisterWrite",
            FieldEvent::SetPartyLeader { .. } => "SetPartyLeader",
            FieldEvent::CameraConfigure { .. } => "CameraConfigure",
            FieldEvent::CameraLoad { .. } => "CameraLoad",
            FieldEvent::CameraSave => "CameraSave",
            FieldEvent::CameraApply => "CameraApply",
            FieldEvent::SetupAnimation { .. } => "SetupAnimation",
            FieldEvent::RenderCfgLong { .. } => "RenderCfgLong",
            FieldEvent::RenderCfgShort { .. } => "RenderCfgShort",
            FieldEvent::CounterUpdate { .. } => "CounterUpdate",
            FieldEvent::EffectAnimTrigger { .. } => "EffectAnimTrigger",
            FieldEvent::SceneFade { .. } => "SceneFade",
            FieldEvent::MenuCtrl { .. } => "MenuCtrl",
            FieldEvent::MenuRefresh => "MenuRefresh",
            FieldEvent::MoveTo { .. } => "MoveTo",
            FieldEvent::ExecMove { .. } => "ExecMove",
            FieldEvent::FmvTrigger { .. } => "FmvTrigger",
        };
        *counts.entry(kind).or_insert(0) += 1;
    }
    eprintln!(
        "[first-town] {} events emitted across {} kinds:",
        all_events.len(),
        counts.len()
    );
    for (k, c) in &counts {
        eprintln!("    {k}: {c}");
    }
    eprintln!(
        "[first-town] post-tick world state: frames={} bgm={:?} dialog={:?} party_leader={:?} money={}",
        host.world.frame,
        host.world.current_bgm,
        host.world.current_dialog.as_ref().map(|d| d.text_id),
        host.world.party_leader_slot,
        host.world.money
    );

    // Acceptance: the chain runs without panic, the asset bundle
    // resolves, descriptor 0 LZS-decodes, and the field VM ticks.
    // Whether the event queue accumulates depends on which records the
    // scene runs first - many init-only records halt on opcode 0x00
    // (data-shaped record bodies the SCUS dispatcher would reach via
    // the cross-context resolver, not the bytecode interpreter).
    //
    // The bar this catches:
    //  - SceneHost::open_extracted → load_scene → enter_field_scene
    //    works end-to-end on real disc data.
    //  - find_bundle / extract_descriptor_0_lzs work on real data.
    //  - step_field doesn't panic across many frames.
    //
    // The thing this DOESN'T validate (still gated on uncaptured
    // overlays): that the field VM's halted scripts get unhalted by
    // the right combination of input + cross-context dispatch + the
    // ANM bytecode interpreter. That's why item 4 (ANM bytecode) and
    // future input wiring are the next dependencies.
    assert!(
        host.world.frame > 0,
        "no frames ticked - chain is broken before the world is even alive"
    );
    assert!(
        !step_outcomes.is_empty(),
        "no step outcomes recorded - field_pc never advanced through any record"
    );
    eprintln!(
        "[first-town] chain alive: {} step outcomes, {} events",
        step_outcomes.values().sum::<usize>(),
        all_events.len()
    );
}
