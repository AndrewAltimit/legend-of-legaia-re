//! Disc-gated integration test for the synchronous actor-spawn opcode
//! `0x4C 0xD8`. Companion to the synthetic unit + integration tests in
//! `field_actor_spawn_materialize_e2e.rs`.
//!
//! What this catches:
//!  - The opcode encoding `[0x4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo,
//!    kind_hi, var_lo, var_hi]` is consistent across real PROT scenes
//!    (smoke: the field-VM packet length walker advances exactly 9 bytes
//!    per chained opcode and lands on the next valid 0x4C dispatch byte).
//!  - The 0x4C 0xD8 host hook synchronously allocates an actor with the
//!    bytecode-encoded `kind` / `variant` when fed a real on-disc byte
//!    slice.
//!
//! Why a synthetic-position drive instead of a natural one: scans of the
//! retail event-script corpus surface 0x4C 0xD8 hits in 14 scenes
//! (`agumon`, `balden2`, `dolk2`, …) but every hit lives deep inside a
//! large record (offsets >= 0x4365 bytes into the record). The field VM
//! steps sequentially from offset 0; reaching those offsets naturally
//! would require driving thousands of opcodes and resolving every halt /
//! cross-context branch in between, most of which are still uncaptured.
//!
//! The cleanest cluster is `balden2` record 7 at offset 0x52D76: four
//! 0x4C 0xD8 opcodes chained at a 9-byte stride with sequential
//! `vdf_idx` (0x01..0x04) and identical `kind = 0x0066, variant = 0x0066`.
//! That stride alignment is the structural proof that the opcode
//! encoding pinned in `World::op4c_n_d_sub8_call_d77f4` matches retail.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{FIELD_SPAWN_START_SLOT, SceneMode, World};

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

fn skip_if_no_disc() -> Option<PathBuf> {
    let extracted = extracted_dir()?;
    std::env::var_os("LEGAIA_DISC_BIN")?;
    Some(extracted)
}

/// Walk every CDNAME scene and collect `(scene_name, record_index,
/// byte_offset, sliced_bytecode)` for every `0x4C 0xD8` pattern found in
/// event-script records. Lifts the scan logic the
/// `examples/scan_4c_d8.rs` example uses into the test so this catches
/// corpus shifts even when the example isn't run.
fn collect_0x4c_d8_hits(extracted: &std::path::Path) -> Vec<(String, usize, usize, Vec<u8>)> {
    let p = ProtIndex::open_extracted(extracted).expect("open ProtIndex");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse CDNAME");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut out: Vec<(String, usize, usize, Vec<u8>)> = Vec::new();
    for name in &scene_names {
        let Ok(scene) = Scene::load(&p, name) else {
            continue;
        };
        let Some(scripts) = scene.find_event_scripts() else {
            continue;
        };
        for r in 0..scripts.len() {
            let Some(rec) = scripts.record(r) else {
                continue;
            };
            for i in 0..rec.len().saturating_sub(1) {
                if rec[i] == 0x4C && rec[i + 1] == 0xD8 {
                    let end = (i + 9).min(rec.len());
                    out.push((name.clone(), r, i, rec[i..end].to_vec()));
                }
            }
        }
    }
    out
}

/// Smoke: the corpus has at least one `0x4C 0xD8` hit. Catches a
/// regression where the scene-event-scripts walker stops surfacing this
/// opcode (or, more plausibly, the asset categorizer demotes
/// `SceneScriptedAssetTable` to a class that doesn't get walked).
#[test]
fn disc_corpus_contains_4c_d8_opcode_pattern() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let hits = collect_0x4c_d8_hits(&extracted);
    eprintln!(
        "[disc] {} total 0x4C 0xD8 hits across event scripts",
        hits.len()
    );
    // A handful of scenes (currently 14) hit the opcode; a hard
    // assertion of N would rot if the categorizer reshuffles scene
    // classes. Assert non-empty + spot-check the cluster `balden2`
    // record 7 still has the structured 4-spawn block.
    assert!(
        !hits.is_empty(),
        "expected at least one 0x4C 0xD8 hit in event scripts; corpus may have shifted"
    );
    let cluster: Vec<&(String, usize, usize, Vec<u8>)> = hits
        .iter()
        .filter(|(n, r, _, _)| n == "balden2" && *r == 7)
        .collect();
    assert!(
        cluster.len() >= 4,
        "expected >= 4 chained 0x4C 0xD8 spawns in balden2 record 7, got {}: {:?}",
        cluster.len(),
        cluster
    );
    // The chained block is at a 9-byte stride - that's how we know the
    // opcode encoding matches retail (each opcode body is exactly
    // `[vdf_idx, tmd:u16, kind:u16, variant:u16]` = 7 bytes after the
    // 2-byte opcode prefix).
    let mut offsets: Vec<usize> = cluster.iter().map(|(_, _, off, _)| *off).collect();
    offsets.sort();
    for w in offsets.windows(2) {
        assert_eq!(
            w[1] - w[0],
            9,
            "balden2 rec7 chained 0x4C 0xD8 cluster must stride by 9 bytes (got {:?})",
            offsets
        );
    }
}

/// Drive the field VM over the real on-disc `0x4C 0xD8` byte sequence
/// from `balden2` record 7 and verify it synchronously spawns one actor
/// with the bytecode-encoded `kind` / `variant`.
///
/// Slices the record starting at the first opcode hit so the field VM
/// dispatches the opcode on tick 1 without having to step through every
/// preceding opcode in the record. The slice is small (one opcode + a
/// few trailing bytes) and ends with a 0x00 terminator so the field
/// VM's halt-acquire prelude treats it as a complete record.
#[test]
fn drives_real_balden2_4c_d8_into_synchronous_spawn() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let hits = collect_0x4c_d8_hits(&extracted);
    let first = hits
        .iter()
        .find(|(n, r, _, _)| n == "balden2" && *r == 7)
        .expect("balden2 record 7 should have a 0x4C 0xD8 hit");
    let (_, _, _, op_bytes) = first;
    assert_eq!(
        op_bytes.len(),
        9,
        "balden2 0x4C 0xD8 hit should be 9 bytes long; got {op_bytes:?}"
    );
    eprintln!("[disc] balden2 rec7 4C D8 bytes: {op_bytes:02X?}");

    let mut bytecode: Vec<u8> = op_bytes.clone();
    // Trailing 0x00 = halt so the prelude (`FUN_8003CA38` walker) treats
    // the slice as a complete record.
    bytecode.push(0x00);

    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.load_field_record(&bytecode);
    let _ = world.tick();

    // Decode the on-disc encoding to compare against actor state.
    let kind = u16::from_le_bytes([op_bytes[5], op_bytes[6]]);
    let variant = u16::from_le_bytes([op_bytes[7], op_bytes[8]]);
    eprintln!("[disc] expecting kind=0x{kind:04X} variant=0x{variant:04X}");

    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(
        world.actors[slot].active,
        "expected synchronous spawn into slot {slot}, but slot is inactive"
    );
    assert_eq!(world.actors[slot].kind, kind);
    assert_eq!(world.actors[slot].variant, variant);

    let events = world.drain_field_events();
    let mut saw_spawned = false;
    let mut saw_allocate = false;
    for ev in &events {
        match ev {
            FieldEvent::ActorSpawned {
                slot: s,
                kind: k,
                variant: v,
                ..
            } => {
                assert_eq!(*s, FIELD_SPAWN_START_SLOT);
                assert_eq!(*k, kind);
                assert_eq!(*v, variant);
                saw_spawned = true;
            }
            FieldEvent::ActorAllocate { .. } => {
                saw_allocate = true;
            }
            _ => {}
        }
    }
    assert!(
        saw_spawned,
        "expected ActorSpawned from real 0x4C 0xD8 byte stream, got {events:?}"
    );
    assert!(
        !saw_allocate,
        "0x4C 0xD8 must spawn synchronously - no ActorAllocate event should be emitted; got {events:?}"
    );
}

/// SceneHost loads `doman` (count=1 VDF chunk in the corpus) and the
/// `0x4C 0xD8` host hook resolves VDF body 0 onto the spawned actor's
/// `spawn_record`. Confirms the simple-branch VDF plumbing (item 2 of
/// the actor-spawn handoff) lands end-to-end against real disc data.
#[test]
fn scene_host_loads_doman_vdf_buffer_and_spawn_resolves_body() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut host =
        legaia_engine_core::scene::SceneHost::open_extracted(&extracted).expect("open SceneHost");
    if host.load_scene("doman").is_err() {
        eprintln!("[skip] doman scene not loadable");
        return;
    }
    // The scan_vdf_chunks example shows `doman` has count=1 - so the
    // host should install a Some(_) buffer with one resolvable record.
    host.enter_field_scene("doman", 0)
        .expect("enter doman record 0");
    let vdf = host
        .world
        .vdf_buffer
        .as_deref()
        .expect("doman should have installed a VDF buffer");
    assert!(
        vdf.len() >= 8,
        "doman VDF buffer should be at least 8 bytes; got {}",
        vdf.len()
    );
    let count = u32::from_le_bytes(vdf[0..4].try_into().unwrap());
    assert_eq!(count, 1, "doman VDF should carry exactly 1 record");

    let body0 = host
        .world
        .vdf_record_bytes(0)
        .expect("VDF record 0 should resolve");
    assert!(!body0.is_empty(), "VDF body 0 should not be empty");
    eprintln!(
        "[disc] doman VDF body 0: {} bytes (first 8: {:02X?})",
        body0.len(),
        &body0[..body0.len().min(8)]
    );

    // Drive a synthetic `0x4C 0xD8 vdf_idx=0` opcode against the loaded
    // world. We synthesise the bytecode to bypass the natural-stepping
    // problem (record 0 doesn't reach the deep-offset 0x4C 0xD8 hits in
    // this scene either) - what we're testing here is that the host
    // hook reads the real on-disc VDF buffer onto the spawned actor.
    let body0_owned = body0.to_vec();
    let bytecode = vec![0x4C, 0xD8, 0x00, 0x00, 0x00, 0x77, 0x77, 0x88, 0x88, 0x00];
    host.world.load_field_record(&bytecode);
    let _ = host.world.tick();

    let slot = legaia_engine_core::world::FIELD_SPAWN_START_SLOT as usize;
    assert!(
        host.world.actors[slot].active,
        "synchronous spawn should land in slot {slot}"
    );
    assert_eq!(host.world.actors[slot].kind, 0x7777);
    assert_eq!(host.world.actors[slot].variant, 0x8888);
    assert_eq!(
        host.world.actors[slot].spawn_record.as_deref(),
        Some(&body0_owned[..]),
        "spawn_record should mirror VDF body 0 from the doman buffer"
    );

    let evs = host.world.drain_field_events();
    let spawn_records: Vec<&Vec<u8>> = evs
        .iter()
        .filter_map(|e| match e {
            FieldEvent::ActorSpawned { record, .. } => Some(record),
            _ => None,
        })
        .collect();
    assert_eq!(
        spawn_records.len(),
        1,
        "should see exactly one ActorSpawned event"
    );
    assert_eq!(spawn_records[0], &body0_owned);
}

/// SceneHost-driven variant: boot `balden2` through `enter_field_scene`
/// at record 0 (the natural entry record), drive many frames, and
/// surface a per-frame summary. Documents the negative finding -
/// stepping naturally from record 0 will not reach the deep-offset
/// 0x4C 0xD8 cluster, so no ActorSpawned event is expected here.
/// The test is informational: it just verifies the scene boots
/// without panic.
#[test]
fn balden2_natural_drive_does_not_reach_4c_d8_cluster() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };

    let mut host =
        legaia_engine_core::scene::SceneHost::open_extracted(&extracted).expect("open SceneHost");
    if host.load_scene("balden2").is_err() {
        eprintln!("[skip] balden2 scene not loadable");
        return;
    }
    host.enter_field_scene("balden2", 0)
        .expect("enter balden2 record 0");

    let mut spawned = 0usize;
    for _ in 0..500 {
        let _ = host.tick();
        for ev in host.world.drain_field_events() {
            if matches!(ev, FieldEvent::ActorSpawned { .. }) {
                spawned += 1;
            }
        }
        host.world.field_ctx.flags &= !0x400;
    }
    eprintln!(
        "[disc] balden2 record 0 natural drive: {spawned} ActorSpawned events across 500 frames"
    );
    // Negative-finding assert: natural stepping from record 0 doesn't
    // reach the deep-offset 0x4C 0xD8 cluster. If this ever starts
    // firing real spawns, the field-VM's halt-recovery wiring or
    // cross-context dispatch reached new territory and the
    // `drives_real_balden2_4c_d8_into_synchronous_spawn` test above
    // becomes redundant.
    assert_eq!(
        spawned, 0,
        "balden2 record 0 unexpectedly hit a synchronous spawn - update this test"
    );
}
