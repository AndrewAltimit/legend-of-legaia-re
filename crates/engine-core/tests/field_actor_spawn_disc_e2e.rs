//! Disc-gated integration test for the synchronous actor-spawn opcode
//! `0x4C 0xD8`. Companion to the synthetic unit + integration tests in
//! `field_actor_spawn_materialize_e2e.rs`.
//!
//! What this catches:
//!  - The opcode encoding `[0x4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo,
//!    kind_hi, var_lo, var_hi]` is consistent across real PROT scenes
//!    (the field-VM packet length walker advances exactly 9 bytes per
//!    chained opcode and lands on the next valid `0x4C` dispatch byte).
//!  - The 0x4C 0xD8 host hook synchronously allocates an actor with the
//!    bytecode-encoded `kind` / `variant` when fed a real on-disc byte
//!    slice.
//!
//! ## Which carrier this census reads, and why
//!
//! `0x4C` (`MENU_CTRL`) is a **field-VM** opcode, and the field VM's
//! bytecode lives in the scene **MAN** - the asset-table bundle MAN plus
//! each block's streaming variant carrier. This census therefore walks MAN
//! records (see [`legaia_engine_core::man_field_scripts`]).
//!
//! It used to walk `Scene::find_event_scripts` instead. Those entries carry
//! **move-VM prescripts**, not field-VM bytecode, so a census over them was
//! aimed at the wrong carrier from the start; the non-zero count it reported
//! came from a one-sector prescript entry read under the old declared-span
//! PROT size (`toc[p+5] - toc[p+3] + 4`, which measures entry `p`'s two
//! *successors*), whose read window ran past itself into the neighbouring
//! bundle - i.e. it was reporting the bundle MAN's opcodes filed under the
//! prescript's name. With entry sizes corrected to `toc[p+3] - toc[p+2]` the
//! event-script scan reports zero, correctly. See `docs/formats/prot.md` and
//! `docs/subsystems/script-vm-menuctrl.md`.
//!
//! Sites are taken at real instruction boundaries via the field-VM
//! disassembler rather than by scanning for the `4C D8` byte pair, since a
//! byte scan also matches operand and Shift-JIS bytes.
//! [`decoded_4c_d8_census_matches_the_walker_independent_byte_scan`] is the
//! cross-check that the two agree here.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{FIELD_SPAWN_START_SLOT, SceneMode, World};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

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

/// One decoded `0x4C 0xD8` instruction site in a scene MAN.
#[derive(Debug, Clone)]
struct SpawnOpSite {
    scene: String,
    /// PROT extraction index of the MAN carrier the site lives in.
    entry_idx: u32,
    /// `true` for a streaming variant carrier, `false` for the bundle MAN.
    variant: bool,
    partition: usize,
    record: usize,
    /// Byte offset of the opcode within the record body.
    body_pc: usize,
    /// Decode coherence: the walk had `CLEAN_RESYNC_INSNS` error-free
    /// instructions behind it, so this is not a resync artifact.
    clean: bool,
    /// Bytes the disassembler consumed for this instruction (must be 9).
    size: usize,
    /// The 9 encoded bytes.
    bytes: Vec<u8>,
}

/// Walk every CDNAME scene's MAN carriers and collect every decoded
/// `0x4C 0xD8` instruction. Mirrors `examples/scan_4c_d8.rs` so the census
/// holds even when the example isn't run.
///
/// Records are bounded and header-decoded per partition by
/// [`partition_record_span`], then disassembled from the record's first
/// opcode - the same instrument [`legaia_engine_core::man_field_scripts`]
/// uses for the flag censuses.
fn collect_0x4c_d8_sites(extracted: &std::path::Path) -> Vec<SpawnOpSite> {
    let index = ProtIndex::open_extracted(extracted).expect("open ProtIndex");
    let mut out = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = (*man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .unwrap_or(&0))
                .max(0) as usize;
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        if let InsnInfo::MenuCtrl { op0: 0xD8, .. } = insn.info {
                            let end = (insn.pc + 9).min(body.len());
                            out.push(SpawnOpSite {
                                scene: name.clone(),
                                entry_idx: carrier.entry_idx,
                                variant: carrier.is_variant(),
                                partition,
                                record,
                                body_pc: insn.pc,
                                clean,
                                size: insn.size,
                                bytes: body[insn.pc..end].to_vec(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// The disc-wide `0x4C 0xD8` census, over the MAN.
///
/// The pinned number is a count of **opcode sites** (individual decoded
/// instructions), with the count of **scenes** carrying at least one
/// reported alongside - the two are different figures and the pre-re-aim
/// census conflated them (its prose said "14 scenes", its assertion counted
/// byte-pair occurrences in event-script records).
///
/// Every retail site is:
///  - in **partition 1 record 0** - the scene-entry system script, the one
///    `Scene::field_man_entry_script` resolves. No per-actor interaction
///    script and no cutscene-timeline record uses the synchronous spawn;
///  - decoded 9 bytes wide, matching the encoding
///    `[0x4C, 0xD8, vdf_idx, tmd:u16, kind:u16, variant:u16]`;
///  - `clean` - the walker had a clear run-up, so no site is a resync
///    artifact of a desync inside dialogue.
///
/// Within a scene the sites form a contiguous 9-byte-stride chain: that
/// stride alignment is the structural proof that the encoding pinned in
/// `World::op4c_n_d_sub8_call_d77f4` matches retail, since a wrong width
/// would not land the next decode on a `0x4C` dispatch byte five times over.
#[test]
fn disc_corpus_contains_4c_d8_opcode_pattern() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let sites = collect_0x4c_d8_sites(&extracted);
    let mut per_scene: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &sites {
        *per_scene.entry(s.scene.as_str()).or_default() += 1;
    }
    eprintln!(
        "[disc] {} 0x4C 0xD8 opcode sites across {} scenes: {per_scene:?}",
        sites.len(),
        per_scene.len()
    );

    assert_eq!(
        per_scene,
        BTreeMap::from([
            ("balden", 4),
            ("balden2", 4),
            ("garmel", 2),
            ("jagaroom", 6),
            ("juui2", 1),
        ]),
        "the disc's synchronous-spawn sites, per scene (17 opcode sites in 5 scenes)"
    );

    // Every site is a clean, 9-byte-wide decode in the scene-entry script.
    for s in &sites {
        assert_eq!(s.size, 9, "0x4C 0xD8 decodes 9 bytes wide: {s:?}");
        assert_eq!(s.bytes.len(), 9, "site should carry its 9 bytes: {s:?}");
        assert!(s.clean, "no site may be a decode-resync artifact: {s:?}");
        assert_eq!(
            (s.partition, s.record),
            (1, 0),
            "every retail synchronous spawn sits in the scene-entry system \
             script P1[0]: {s:?}"
        );
    }

    // `balden` carries the cluster in its bundle MAN; `balden2` carries an
    // equivalent one in its streaming *variant* MAN - a different PROT entry
    // with different bytes, not the same MAN seen twice (the disc-wide
    // no-two-carriers-share-bytes property is asserted in
    // `man_variant_carrier_census_disc`).
    assert!(
        sites
            .iter()
            .any(|s| s.scene == "balden" && !s.variant && s.entry_idx == 183),
        "balden's cluster lives in its bundle MAN (PROT 183)"
    );
    assert!(
        sites
            .iter()
            .any(|s| s.scene == "balden2" && s.variant && s.entry_idx == 320),
        "balden2's cluster lives in its streaming variant MAN (PROT 320)"
    );

    // Per scene the sites chain at a 9-byte stride.
    for scene in per_scene.keys() {
        let mut offsets: Vec<usize> = sites
            .iter()
            .filter(|s| &s.scene.as_str() == scene)
            .map(|s| s.body_pc)
            .collect();
        offsets.sort_unstable();
        for w in offsets.windows(2) {
            assert_eq!(
                w[1] - w[0],
                9,
                "{scene}'s chained 0x4C 0xD8 cluster must stride by 9 bytes (got {offsets:?})"
            );
        }
    }

    // `balden2`'s four spawns carry sequential `vdf_idx` 0x01..=0x04 - the
    // cluster the synchronous-spawn drive below replays.
    let vdf: Vec<u8> = sites
        .iter()
        .filter(|s| s.scene == "balden2")
        .map(|s| s.bytes[2])
        .collect();
    assert_eq!(
        vdf,
        vec![0x01, 0x02, 0x03, 0x04],
        "balden2's cluster walks VDF bodies 1..=4 in order"
    );
}

/// Walker-independent cross-check of the census above: for every MAN
/// carrier on the disc, the raw `4C D8` **byte-pair** count equals the
/// decoded **instruction** count.
///
/// This is the corroboration that makes the pinned number a measurement
/// rather than a property of the disassembler. The two instruments fail in
/// opposite directions - a byte scan over-counts (operand and Shift-JIS
/// bytes alias the pair) while an opcode walk under-counts (a desync inside
/// dialogue silently drops real ops) - so their agreeing exactly, carrier by
/// carrier, rules out both. Compare `flag_test_bytescan`, which exists for
/// the same reason on the flag census.
///
/// A carrier where they diverge is the interesting case: `raw > decoded`
/// means the walker desynced past a real op, `decoded > raw` is impossible
/// and would mean the record bounds overlap.
#[test]
fn decoded_4c_d8_census_matches_the_walker_independent_byte_scan() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let sites = collect_0x4c_d8_sites(&extracted);

    let mut raw_total = 0usize;
    let mut carriers_with_pairs = 0usize;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let raw = (0..man.len().saturating_sub(1))
                .filter(|&o| man[o] == 0x4C && man[o + 1] == 0xD8)
                .count();
            let decoded = sites
                .iter()
                .filter(|s| s.scene == name && s.entry_idx == carrier.entry_idx)
                .count();
            assert_eq!(
                raw, decoded,
                "{name} PROT {}: {raw} raw `4C D8` byte pairs vs {decoded} decoded \
                 instructions - a divergence means the walk desynced past a real op \
                 (raw > decoded) or the record bounds overlap (decoded > raw)",
                carrier.entry_idx
            );
            raw_total += raw;
            if raw > 0 {
                carriers_with_pairs += 1;
            }
        }
    }
    eprintln!(
        "[disc] {raw_total} raw `4C D8` byte pairs across {carriers_with_pairs} carriers, \
         all decoding as instructions"
    );
    assert_eq!(
        raw_total,
        sites.len(),
        "disc-wide byte-pair total must equal the decoded site total"
    );
}

/// Drive the field VM over the real on-disc `0x4C 0xD8` byte sequence from
/// `balden2`'s scene-entry system script (variant MAN P1[0]) and verify it
/// synchronously spawns one actor with the bytecode-encoded `kind` /
/// `variant`.
///
/// Slices the record starting at the first opcode site so the field VM
/// dispatches the opcode on tick 1 without having to step through every
/// preceding opcode in the record. The slice is small (one opcode + a
/// few trailing bytes) and ends with a 0x00 terminator so the field
/// VM's halt-acquire prelude treats it as a complete record.
///
/// `balden2_natural_drive_reaches_4c_d8_cluster_via_entry_script` below is
/// the un-sliced companion: the same P1[0] script driven from its own
/// entry point reaches this cluster organically.
#[test]
fn drives_real_balden2_4c_d8_into_synchronous_spawn() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let sites = collect_0x4c_d8_sites(&extracted);
    let first = sites
        .iter()
        .find(|s| s.scene == "balden2")
        .expect("balden2's variant-MAN entry script should carry a 0x4C 0xD8 site");
    let op_bytes = &first.bytes;
    assert_eq!(
        op_bytes.len(),
        9,
        "balden2 0x4C 0xD8 site should be 9 bytes long; got {op_bytes:?}"
    );
    eprintln!(
        "[disc] balden2 P{}[{}] 4C D8 bytes: {op_bytes:02X?}",
        first.partition, first.record
    );

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
    // Synthesise a 0x4C 0xD8 with tmd_idx=0x0000 so we also exercise the
    // global TMD-pool lookup (`Actor::tmd_ref`). The `enter_field_scene`
    // path seeds pool[0..=4] from PROT 0874 section 0, so slot 0 must
    // resolve to a parsed character-mesh TMD here.
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

    // The global TMD-pool head is seeded from PROT 0874 section 0, so
    // tmd_idx = 0 must resolve to a real character-mesh TMD.
    let tmd_ref = host.world.actors[slot]
        .tmd_ref
        .as_ref()
        .expect("tmd_idx=0 should resolve to befect_data section-0 slot 0");
    assert_eq!(
        tmd_ref.tmd.header.id, 0x8000_0002,
        "slot-0 TMD should carry the Legaia TMD magic",
    );
    assert!(
        tmd_ref.tmd.header.flist_bit_set,
        "befect_data TMDs ship with FLIST_BIT set",
    );
    assert!(
        !tmd_ref.raw.is_empty(),
        "raw bytes should round-trip through the global pool",
    );
    eprintln!(
        "[disc] doman tmd_idx=0 -> nobj={}, raw={}B",
        tmd_ref.tmd.header.nobj,
        tmd_ref.raw.len()
    );

    // The renderer-side spawn handler (`legaia-engine` play-window) drains
    // ActorSpawned events, takes the actor's `tmd_ref`, builds a VRAM
    // mesh via `legaia_tmd::mesh::tmd_to_vram_mesh`, and uploads it as a
    // new mesh slot. Assert the seeded slot-0 TMD is mesh-buildable here
    // so the renderer-side path has something to upload (the upload
    // itself needs a wgpu device and isn't unit-testable, but the
    // mesh-build is).
    let vmesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd_ref.tmd, &tmd_ref.raw);
    assert!(
        !vmesh.indices.is_empty(),
        "seeded slot-0 TMD must produce a non-empty mesh for the renderer to upload",
    );
    assert!(
        vmesh.indices.len().is_multiple_of(3),
        "vram mesh indices must be a multiple of 3 (triangle list); got {}",
        vmesh.indices.len(),
    );
    eprintln!(
        "[disc] doman tmd_idx=0 mesh: {} verts, {} indices",
        vmesh.positions.len(),
        vmesh.indices.len()
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

/// SceneHost-driven check that the global TMD-pool head (5 character-mesh
/// TMDs from PROT 0874 section 0) is seeded into `World::global_tmd_pool`
/// on field-scene entry. Companion to
/// [`scene_host_loads_doman_vdf_buffer_and_spawn_resolves_body`] which
/// covers the spawn-time resolver - this one isolates the seed.
#[test]
fn scene_host_seeds_global_tmd_pool_head_from_befect_data() {
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
    // Pool starts empty.
    assert!(host.world.global_tmd_pool.is_empty());
    host.enter_field_scene("doman", 0)
        .expect("enter doman record 0");
    // Head of 5 must be populated.
    assert!(
        host.world.global_tmd_pool.len() >= 5,
        "global TMD pool head should be at least 5 slots; got {}",
        host.world.global_tmd_pool.len()
    );
    for idx in 0..5 {
        let entry = host
            .world
            .global_tmd_pool
            .get(idx)
            .and_then(|s| s.as_ref())
            .unwrap_or_else(|| panic!("befect_data slot {idx} should be populated"));
        assert_eq!(
            entry.tmd.header.id, 0x8000_0002,
            "befect_data slot {idx} should carry the Legaia TMD magic",
        );
        assert!(
            entry.tmd.header.flist_bit_set,
            "befect_data slot {idx} should have FLIST_BIT set",
        );
        eprintln!(
            "[disc] befect_data slot {idx}: nobj={}, raw={}B",
            entry.tmd.header.nobj,
            entry.raw.len()
        );
    }
}

/// SceneHost-driven variant: boot `balden2` through `enter_field_scene`
/// at record 0 (the natural entry record), drive many frames, and
/// surface a per-frame summary.
///
/// `balden2` has no bundle MAN - its only MAN is the block's streaming
/// variant carrier (PROT 320), and `Scene::field_man_entry_script`
/// resolves it (the same fallback `field_man_payload` uses), so the
/// natural drive runs the carrier's real `P1[0]` entry script and
/// reaches the `0x4C 0xD8` synchronous-spawn cluster organically. (The
/// earlier negative finding - "natural stepping never reaches the
/// cluster" - held only while the entry-script resolver was
/// bundle-MAN-only and the scene fell back to the event-record-0 load,
/// which parks at the trigger table.)
#[test]
fn balden2_natural_drive_reaches_4c_d8_cluster_via_entry_script() {
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
    // The streaming-carrier P1[0] entry script reaches the 0x4C 0xD8
    // synchronous-spawn cluster during the natural drive, so real
    // ActorSpawned events fire without the forced-pc harness above.
    assert!(
        spawned > 0,
        "balden2's entry script reaches the 0x4C 0xD8 spawn cluster naturally"
    );
}

/// Every shipped `0x4C 0xD8` morph block is **one record**, and that is what
/// makes retail's three disagreeing record pitches unobservable.
///
/// `FUN_801D77F4` walks the block twice - `0xC` per record for its size sum
/// (`0x801D78D0..0x801D7900`), `n_vert * 8` per record for its copy pass
/// (`0x801D792C..0x801D799C`) - and the apply pass `FUN_8002174C` walks it a
/// third time at `n_vert * 0x60`. Three strides over one buffer can only
/// agree where no stride is ever consumed, i.e. at a count of `1`. This
/// census is the measurement that says the disc never leaves that case, so
/// the port can reproduce each loop's own stride without any shipped content
/// diverging - and so that a future block with two records is recognised as
/// new territory rather than assumed covered.
#[test]
fn morph_weight_disc_blocks_are_single_record() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let sites = collect_0x4c_d8_sites(&extracted);
    assert!(!sites.is_empty(), "census should find the shipped sites");

    let mut resolved = 0usize;
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    let mut groups: BTreeMap<u32, usize> = BTreeMap::new();
    let mut by_scene: BTreeMap<String, usize> = BTreeMap::new();
    for name in index.cdname_scene_names() {
        let scene_sites: Vec<&SpawnOpSite> = sites.iter().filter(|s| s.scene == name).collect();
        if scene_sites.is_empty() {
            continue;
        }
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let mut world = World::default();
        world.set_vdf_buffer(legaia_engine_core::scene_bundle::find_vdf_buffer(&scene));
        for s in scene_sites {
            let Some(block) = world.vdf_record_bytes(s.bytes[2]).map(<[u8]>::to_vec) else {
                continue;
            };
            assert!(
                block.len() >= 16,
                "{name}: a morph block must hold its count word and one 12-byte header"
            );
            resolved += 1;
            *by_scene.entry(name.clone()).or_default() += 1;
            let count = u32::from_le_bytes(block[0..4].try_into().unwrap());
            *counts.entry(count).or_default() += 1;
            *groups
                .entry(u32::from_le_bytes(block[4..8].try_into().unwrap()))
                .or_default() += 1;
        }
    }
    eprintln!(
        "[disc] {resolved} of {} 0x4C 0xD8 sites resolve a morph block; \
         record counts {counts:?}, leading group ids {groups:?}, per scene {by_scene:?}",
        sites.len()
    );
    assert_eq!(
        resolved,
        sites.len(),
        "every shipped synchronous-spawn site should resolve its VDF morph block"
    );
    assert_eq!(
        counts.keys().copied().collect::<Vec<_>>(),
        vec![1],
        "every shipped morph block holds exactly one record - the only shape \
         retail's three record pitches agree on"
    );
    assert_eq!(
        groups.keys().copied().collect::<Vec<_>>(),
        vec![0],
        "every shipped morph record names TMD object 0"
    );
}

/// The whole `4C D8` chain on real disc bytes: the allocator seats a
/// morph-weight actor with a rest-pose snapshot, the pool tick ramps its
/// weight, and the shared render read poses the mesh off it.
///
/// Drives the scene's own opcode bytes rather than a synthetic encoding, so
/// a change in the operand layout fails here rather than passing against a
/// fixture that agrees with the port by construction.
#[test]
fn morph_weight_actor_seats_and_blends_on_a_shipped_carrier() {
    let Some(extracted) = skip_if_no_disc() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let sites = collect_0x4c_d8_sites(&extracted);

    let slot = FIELD_SPAWN_START_SLOT as usize;
    let mut seated = 0usize;
    for scene in ["jagaroom", "garmel", "juui2", "balden", "balden2"] {
        let Some(site) = sites.iter().find(|s| s.scene == scene) else {
            continue;
        };
        // One host per scene: a second `enter_field_scene` on the same host
        // leaves the previous scene's pool actors in the auto-spawn slots, so
        // the slot this drive lands in would not be this scene's.
        let mut host = legaia_engine_core::scene::SceneHost::open_extracted(&extracted)
            .expect("open SceneHost");
        if host.load_scene(scene).is_err() || host.enter_field_scene(scene, 0).is_err() {
            continue;
        }
        let mut bytecode = site.bytes.clone();
        bytecode.push(0x00);
        host.world.mode = SceneMode::Field;
        host.world.load_field_record(&bytecode);
        let _ = host.world.tick();

        let actor = &host.world.actors[slot];
        if actor.tmd_ref.is_none() {
            // The site's TMD index is outside the seeded global pool for this
            // scene: retail's own bail-through, and nothing to pose.
            eprintln!("[disc] {scene}: site's TMD index not resident; no morph seat");
            continue;
        }
        assert_eq!(
            actor.handler,
            legaia_engine_core::actor_handler::ActorHandler::MorphWeights,
            "{scene}: the 4C D8 allocator stamps the morph handler"
        );
        let morph = actor
            .morph_weights
            .as_ref()
            .expect("a seated morph actor carries its block + snapshot");
        assert!(
            !morph.rest_pose.is_empty(),
            "{scene}: the spawner's rest-pose snapshot must be non-empty"
        );
        assert_eq!(morph.envelope.weight, 0, "{scene}: spawns at the rest pose");
        assert!(
            !morph.envelope.descending,
            "{scene}: the direction halfword is zeroed, so the ramp rises"
        );
        let up = morph.envelope.up_rate;

        // At weight 0 the posed mesh is the rest pose exactly.
        let Some((rest_posed, _, w0)) = host.world.morph_weight_posed_tmd(slot) else {
            eprintln!("[disc] {scene}: block names no object this mesh has; nothing to pose");
            continue;
        };
        assert_eq!(w0, 0);

        // Ramp it and the pose moves - unless the scene authored a zero rise
        // rate, which is a legal envelope and not this test's business.
        for _ in 0..8 {
            host.world.tick_handler_actors(1);
        }
        let (moved, _, w1) = host
            .world
            .morph_weight_posed_tmd(slot)
            .expect("still seated after the ramp");
        eprintln!("[disc] {scene}: up_rate={up} weight {w0} -> {w1}");
        if up > 0 {
            assert!(
                w1 > w0,
                "{scene}: a positive rise rate must move the weight"
            );
            let before: Vec<i16> = rest_posed.objects[0].vertices.iter().map(|v| v.x).collect();
            let after: Vec<i16> = moved.objects[0].vertices.iter().map(|v| v.x).collect();
            assert_ne!(
                before, after,
                "{scene}: a moved weight must move the posed geometry"
            );
        }
        seated += 1;
    }
    assert!(
        seated > 0,
        "at least one shipped carrier should seat a morph-weight actor"
    );
    eprintln!("[disc] {seated} shipped carriers seated + blended a morph-weight actor");
}
