//! The field camera's **zone-query sites**: the four `0x4C` script arms, the
//! per-frame re-query flag, and the rule that nothing else re-queries.
//!
//! Retail's camera parameter block is loaded from a MAN section-3 record by
//! exactly five things (`FUN_801DE3E0`'s seven `jal` sites, resolved against
//! the disc's own jump tables `0x801CEEB8` / `0x801CEF88` in PROT `0897`):
//!
//! * the **player seat / warp path** at `0x801D1FE8..0x801D2014` and its
//!   sibling at `0x801D2BCC` - query, conform the footing, snap, clamp;
//! * the field VM arms `[4C 38]` (`0x801E1048`), `[4C 39]` (`0x801E1078`)
//!   and `[4C C4 x z]` (`0x801E2878`);
//! * the **per-frame** path at `0x801D17FC..0x801D1830`, gated on scratchpad
//!   flag bit 22 (`_DAT_1F800394 & 0x400000`).
//!
//! A plain tile crossing is **not** on that list, which is why a scene like
//! `edbylon` holds a camera-region record the player's own tile does not
//! select. The two halves below assert that: the arms reach `World` through
//! the VM, and walking without one leaves the block alone.
//!
//! The census half prints the per-scene site counts so the shape of the
//! answer is visible rather than asserted from memory; the invariants it
//! asserts are structural (every arm exists somewhere on the disc, the
//! `[4C 3E]` / `[4C C4]` arms are rare).
//!
//! Skips + passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::camera::Camera;
use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use legaia_engine_core::world::{CameraZoneRequest, ZONE_REQUERY_FLAG};
use legaia_engine_vm::field_disasm::{CameraKind, InsnInfo, LinearWalker};

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Count the five camera-block sites in one scene's MAN carriers and its
/// event-script records.
fn scene_sites(index: &ProtIndex, scene: &Scene) -> [usize; 5] {
    let mut c = [0usize; 5];
    let tally = |body: &[u8], pc0: usize, c: &mut [usize; 5]| {
        for insn in LinearWalker::new(body, pc0).flatten() {
            match insn.info {
                InsnInfo::Camera {
                    kind: CameraKind::Load,
                    ..
                } => c[0] += 1,
                InsnInfo::MenuCtrl { op0, .. } => match op0 {
                    0x38 => c[1] += 1,
                    0x39 => c[2] += 1,
                    0x3E => c[3] += 1,
                    0xC4 => c[4] += 1,
                    _ => {}
                },
                _ => {}
            }
        }
    };
    for carrier in scene_man_carriers(index, scene) {
        let man = &carrier.payload;
        let Ok(man_file) = legaia_asset::man_section::parse(man) else {
            continue;
        };
        for partition in 0..3 {
            let count = man_file
                .header
                .partition_counts
                .get(partition)
                .copied()
                .unwrap_or(0)
                .max(0) as usize;
            for rec in 0..count {
                if let Some((start, pc0, len)) =
                    partition_record_span(&man_file, man, partition, rec)
                {
                    tally(&man[start..start + len], pc0, &mut c);
                }
            }
        }
    }
    if let Some(es) = scene.find_event_scripts() {
        for &(a, b) in &es.record_ranges {
            tally(&es.bytes[a..b], 0, &mut c);
        }
    }
    c
}

/// Disc-wide census of the camera-block sites. The headline it establishes:
/// the arms are sparse, so the block spends most of a scene untouched.
#[test]
fn the_camera_block_sites_are_script_arms_not_tile_crossings() {
    let Some(ex) = extracted_dir() else { return };
    let index = ProtIndex::open_extracted(&ex).expect("open ProtIndex");
    let scenes = index.cdname_scene_names();
    let labels = ["0x45 LOAD", "[4C 38]", "[4C 39]", "[4C 3E]", "[4C C4]"];
    let mut totals = [0usize; 5];
    let mut carriers = [0usize; 5];
    let mut silent = 0usize;
    let mut per_scene: BTreeMap<String, [usize; 5]> = BTreeMap::new();
    for name in &scenes {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let c = scene_sites(&index, &scene);
        for i in 0..5 {
            totals[i] += c[i];
            carriers[i] += usize::from(c[i] > 0);
        }
        if c.iter().sum::<usize>() == 0 {
            silent += 1;
        }
        per_scene.insert(name.clone(), c);
    }
    let n = per_scene.len();
    assert!(n > 100, "expected the whole CDNAME scene set, got {n}");
    for i in 0..5 {
        eprintln!(
            "[ok] {}: {} sites in {}/{n} scenes",
            labels[i], totals[i], carriers[i]
        );
    }
    eprintln!("[ok] {silent}/{n} scenes carry no camera-block site at all");
    // Every arm is authored somewhere - a decoder change that stopped
    // recognising one would show up here rather than as a silent camera bug.
    for i in 0..5 {
        assert!(totals[i] > 0, "{} has no site on the disc", labels[i]);
    }
    // The explicit-tile query and the bare snap are the rare pair: the whole
    // disc uses them in a handful of scenes, which is what makes them easy
    // to miss when reading a single scene's script.
    assert!(
        carriers[3] < 10 && carriers[4] < 10,
        "[4C 3E] / [4C C4] carrier counts {} / {} - expected a handful",
        carriers[3],
        carriers[4]
    );
    // `edbylon` - the ending vignette whose resident block the player's own
    // tile does not select - carries exactly one site, a `[4C 38]`.
    if let Some(c) = per_scene.get("edbylon") {
        eprintln!("[ok] edbylon sites {c:?}");
        assert_eq!(c[1], 1, "edbylon's single site is a [4C 38]");
        assert_eq!(c[0] + c[2] + c[3] + c[4], 0);
    }
}

/// The arms reach `World` through the field VM, and a tile crossing does not.
#[test]
fn walking_holds_the_block_while_the_arms_reload_it() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("scene host");
    host.world.begin_new_game();
    host.enter_field_scene("town01", 0).expect("enter town01");
    let slot = host.world.player_actor_slot.expect("player actor");

    // Every arm queues its request through the VM's host impl.
    for (bytes, want) in [
        (vec![0x4Cu8, 0x38], CameraZoneRequest::QueryAtPlayer),
        (vec![0x4C, 0x39], CameraZoneRequest::QueryConformAndSnap),
        (vec![0x4C, 0x3E], CameraZoneRequest::SnapAndClamp),
        (vec![0x4C, 0x3D], CameraZoneRequest::RefreshAttributes),
        (
            vec![0x4C, 0xC4, 0x11, 0x22],
            CameraZoneRequest::QueryAtTile { x: 0x11, z: 0x22 },
        ),
    ] {
        host.world.field_bytecode = bytes;
        host.world.field_pc = 0;
        host.world.step_field().expect("a step");
        let got = host.world.take_camera_zone_requests();
        assert_eq!(got.first().copied(), Some(want), "arm did not reach World");
    }
    // `[4C 39]` also runs the sub-E tail, so it queues two requests.
    host.world.field_bytecode = vec![0x4C, 0x39];
    host.world.field_pc = 0;
    host.world.step_field();
    assert_eq!(
        host.world.take_camera_zone_requests(),
        vec![
            CameraZoneRequest::QueryConformAndSnap,
            CameraZoneRequest::SnapAndClamp
        ]
    );

    // `edbylon` is the scene the tile-crossing re-query got wrong: its one
    // walk region (x `77..104`, z `34..50`) contains none of the three
    // camera-region records' kind-0 anchor points, while most of the map's
    // default-fill region selects record `#0`. Seat the camera where the
    // query hits, then walk into the region where it misses.
    let mut host = SceneHost::open_extracted(&ex).expect("scene host");
    host.world.begin_new_game();
    host.enter_field_scene("edbylon", 0).expect("enter edbylon");
    let slot = host.world.player_actor_slot.expect("player actor");
    let seat = |h: &mut SceneHost, tx: i16, tz: i16| {
        let a = &mut h.world.actors[slot as usize];
        a.move_state.world_x = (tx << 7) + 0x40;
        a.move_state.world_z = (tz << 7) + 0x40;
    };
    let mut cam = Camera::default();
    seat(&mut host, 4, 4);
    cam.reset_globals_for_scene_entry();
    cam.tick(&host.world);
    let hit_block = cam.zone.config;
    assert!(
        cam.zone.loaded_record.is_some(),
        "edbylon tile (4,4) should select a camera-region record"
    );

    // Walk into the tile the ending-vignette state stands on. With the
    // per-frame re-query flag clear - its state on every mode entry - the
    // block must hold the record the seat loaded.
    for tz in 5..=43i16 {
        seat(&mut host, 4 + (tz - 5) * 90 / 38, tz);
        cam.tick(&host.world);
    }
    assert_eq!(
        cam.zone.config, hit_block,
        "a tile crossing re-framed the camera without a script arm"
    );

    // Raise the flag and the same tile does re-query - onto the zone-miss
    // set, because no record covers it. That difference is the whole of the
    // behaviour the port used to have unconditionally.
    host.world.flags.story_flags |= ZONE_REQUERY_FLAG;
    cam.tick(&host.world);
    assert_ne!(
        cam.zone.config, hit_block,
        "the per-frame re-query flag did not re-query"
    );
    assert_eq!(cam.zone.loaded_record, None, "the tile is a query miss");
    eprintln!(
        "[ok] edbylon: block held across 39 tile crossings with bit 22 clear, \
         re-queried to the miss set with it set"
    );

    // And an explicit-tile arm reloads from a tile the player is not on.
    // Note what the query does *not* re-read: `FUN_801DBA20` tests a kind-0
    // record against the attribute box that is currently latched in
    // scratchpad, which is the **player's**, not the named tile's - so
    // `[4C C4]` moves the tile and keeps the box. Seat the player back where
    // the box is the full-map fill and name the far tile.
    host.world.flags.story_flags &= !ZONE_REQUERY_FLAG;
    seat(&mut host, 4, 4);
    cam.tick(&host.world);
    assert_eq!(
        cam.zone.loaded_record, None,
        "walking back must not re-query either"
    );
    host.world
        .push_camera_zone_request(CameraZoneRequest::QueryAtTile { x: 94, z: 43 });
    cam.route_camera_events(&mut host.world);
    cam.tick(&host.world);
    assert_eq!(
        cam.zone.config, hit_block,
        "[4C C4] did not reload from the named tile"
    );
    eprintln!("[ok] [4C C4 5E 2B] reloaded the record from a tile the player is not on");
}
