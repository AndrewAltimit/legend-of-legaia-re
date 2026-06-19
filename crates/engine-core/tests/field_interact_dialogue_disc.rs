//! Disc-gated: the field-interact -> inline-dialogue mapping holds end to end
//! on real town01 data, not just the synthetic fixture.
//!
//! How a field NPC's line is shown (the mechanism the engine re-grounded to):
//! each placed actor's dialogue is its own inline interaction-script MES (retail
//! `actor[+0x90]`), keyed by the actor's **partition-1 placement index**. On
//! field entry the engine populates [`World::field_npc_dialog`] from that real
//! placement table ([`World::install_field_carriers_from_man`]). A field-VM
//! field-interact op (`0x3E` with `op0 < 100`) then carries that index as its
//! `slot` operand, and the host opens `field_npc_dialog[slot]`.
//!
//! The placement-decode, classification, and segment-pool layers are already
//! pinned on real data elsewhere (`field_actor_placements_disc`,
//! `rim_elm_sparring_carrier`). The gap this closes is the **round-trip**: that
//! driving a real `[0x3E, op0, slot]` interact through the field VM opens
//! exactly the interacted placement's own inline dialogue, for every populated
//! slot - previously exercised only with hand-seeded `field_npc_dialog` entries.
//!
//! It also pins two correctness properties of the mapping:
//!   - **install == classify**: the dialogue map the engine installs matches an
//!     independent `classify_placements` pass over the same bytes.
//!   - **lossless slot space**: every NPC placement index fits the `u8` the
//!     field-VM interact operand carries (so the `u8::try_from` in
//!     `install_field_carriers_from_man` never silently drops an NPC).
//!
//! No Sony text is asserted - only structural shape (slot keys, byte-identical
//! inline buffers between install / classify / the opened dialog). Skip-passes
//! without disc data / extracted assets (CLAUDE.md convention).
//!
//! A second test (`field_interact_slot_mapping_holds_across_field_scene_corpus`)
//! generalises the same round-trip from town01 to **every** CDNAME scene that
//! carries a MAN actor-placement partition, so the slot→placement→dialogue
//! mapping is pinned across the whole field-scene corpus rather than one scene.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placements};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn field_interact_slot_opens_real_npc_dialogue() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man_bytes = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 has a MAN payload");
    let man_file = parse_man(&man_bytes).expect("parse MAN");

    // Independent ground truth: placement index -> inline dialogue, straight
    // from classify_placements (the same source install_field_carriers_from_man
    // consumes, computed here separately so a divergence is caught).
    let expected: HashMap<usize, Vec<u8>> = classify_placements(&man_file, &man_bytes)
        .into_iter()
        .filter_map(|(p, k)| match k {
            PlacementKind::Npc {
                dialog_inline: Some(inline),
                ..
            } => Some((p.index, inline)),
            _ => None,
        })
        .collect();
    assert!(
        expected.len() >= 12,
        "town01 populates many NPC dialogue slots (got {})",
        expected.len()
    );

    // Lossless slot space: every NPC placement index fits the u8 field-interact
    // operand, so the index -> slot mapping drops nobody.
    for idx in expected.keys() {
        assert!(
            *idx <= u8::MAX as usize,
            "NPC placement index {idx} exceeds the u8 field-interact slot space"
        );
    }

    // The engine's real install path populates field_npc_dialog from the same
    // bytes; it must agree with the independent classify pass (keys cast to u8).
    let mut world = World::new();
    let sparring_idx = world.install_field_carriers_from_man(&man_file, &man_bytes);
    assert!(
        sparring_idx.is_some(),
        "town01 derives the Rim Elm sparring carrier from its MAN"
    );

    let installed = &world.field_npc_dialog;
    assert_eq!(
        installed.len(),
        expected.len(),
        "install populated {} dialogue slots, classify found {}",
        installed.len(),
        expected.len()
    );
    for (idx, inline) in &expected {
        let slot = *idx as u8;
        assert_eq!(
            installed.get(&slot),
            Some(inline),
            "installed field_npc_dialog[{slot}] must equal the placement's classify inline"
        );
    }

    // The prologue-aware companion map must agree byte-for-byte: every dialogue
    // slot has an untruncated record whose tail from `first_segment` equals the
    // truncated `field_npc_dialog` buffer, and whose `entry_pc <= first_segment`.
    // At least one NPC must carry a real prologue (`entry_pc < first_segment`) so
    // this is non-vacuous - proving the segment-selection bytecode the opt-in
    // VM-dialogue runner executes is actually present on real disc data.
    let mut npcs_with_prologue = 0usize;
    for (idx, inline) in &expected {
        let slot = *idx as u8;
        let prologue = world
            .field_npc_dialog_prologue
            .get(&slot)
            .unwrap_or_else(|| panic!("field_npc_dialog_prologue[{slot}] must be populated"));
        assert!(
            prologue.entry_pc <= prologue.first_segment,
            "prologue entry_pc {} must precede first_segment {} (slot {slot})",
            prologue.entry_pc,
            prologue.first_segment
        );
        assert_eq!(
            &prologue.body[prologue.first_segment..],
            inline.as_slice(),
            "prologue body tail from first_segment must equal field_npc_dialog[{slot}]"
        );
        if prologue.entry_pc < prologue.first_segment {
            npcs_with_prologue += 1;
        }
    }
    assert!(
        npcs_with_prologue > 0,
        "at least one town01 NPC must carry a pre-first-segment interaction prologue"
    );

    // End-to-end: feed each populated slot through the field VM as a real
    // [0x3E, op0, slot] interact (op0 = 5 < 100 -> field-interact arm) and assert
    // it opens exactly that placement's own inline dialogue.
    world.mode = SceneMode::Field;
    let mut verified = 0usize;
    for (idx, inline) in &expected {
        let slot = *idx as u8;
        world.current_dialog = None;
        let _ = world.drain_field_events();
        world.load_field_script(vec![0x3E, 0x05, slot]);
        let _ = world.tick();
        let req = world.current_dialog.as_ref().unwrap_or_else(|| {
            panic!("field-interact on NPC slot {slot} must open a dialogue box")
        });
        assert_eq!(
            &req.inline, inline,
            "slot {slot} must open its own placement's inline dialogue"
        );
        verified += 1;
    }
    assert_eq!(verified, expected.len(), "every NPC slot round-trips");

    // The sparring carrier's own slot is one of the verified NPC slots: its
    // dialogue (the long talk-menu block) opens on interaction like any other.
    // (Engaging the fight is a separate confirm path, not exercised here.)
    eprintln!(
        "[town01] {verified} field-interact slots round-trip to their NPC dialogue; \
         sparring carrier index {sparring_idx:?}"
    );
}

/// The same field-interact slot→placement→dialogue round-trip, swept across
/// **every** CDNAME scene that carries a MAN actor-placement partition (not just
/// town01). For each such scene this asserts:
///
///   - **install == classify**: the dialogue map `install_field_carriers_from_man`
///     populates matches an independent `classify_placements` pass (keys cast to
///     `u8`), so the install loop drops no NPC.
///   - **lossless slot space**: every NPC placement index fits the `u8`
///     field-interact operand.
///   - **round-trip**: driving a real `[0x3E, op0<100, slot]` op through the
///     field VM opens exactly that placement's own inline dialogue.
///
/// This is the corpus generalisation of the single-scene check above - the
/// mapping was previously validated only on town01, so a scene whose partition
/// layout exposed a different index/slot relationship would have gone unnoticed.
/// A coverage floor keeps the sweep from passing vacuously.
///
/// Skip-passes without disc data / extracted assets (CLAUDE.md convention).
#[test]
fn field_interact_slot_mapping_holds_across_field_scene_corpus() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let mut scene_names = index.cdname_scene_names();
    scene_names.sort();
    scene_names.dedup();

    let mut scenes_with_npcs = 0usize;
    let mut total_slots = 0usize;
    let mut total_round_trips = 0usize;
    for name in &scene_names {
        // Not every CDNAME label loads as a field scene (battle bundles, audio
        // banks, etc.); skip the ones without a MAN actor partition.
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok(Some(man_bytes)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(man_file) = parse_man(&man_bytes) else {
            continue;
        };

        let expected: HashMap<usize, Vec<u8>> = classify_placements(&man_file, &man_bytes)
            .into_iter()
            .filter_map(|(p, k)| match k {
                PlacementKind::Npc {
                    dialog_inline: Some(inline),
                    ..
                } => Some((p.index, inline)),
                _ => None,
            })
            .collect();
        if expected.is_empty() {
            continue;
        }

        let mut world = World::new();
        let _ = world.install_field_carriers_from_man(&man_file, &man_bytes);

        // install == classify, and a lossless u8 slot space.
        assert_eq!(
            world.field_npc_dialog.len(),
            expected.len(),
            "[{name}] install populated {} dialogue slots, classify found {}",
            world.field_npc_dialog.len(),
            expected.len(),
        );
        for (idx, inline) in &expected {
            assert!(
                *idx <= u8::MAX as usize,
                "[{name}] NPC placement index {idx} exceeds the u8 field-interact slot space"
            );
            assert_eq!(
                world.field_npc_dialog.get(&(*idx as u8)),
                Some(inline),
                "[{name}] installed field_npc_dialog[{idx}] must equal the classify inline"
            );
        }

        // Round-trip every populated slot through a real field-VM interact op.
        world.mode = SceneMode::Field;
        for (idx, inline) in &expected {
            let slot = *idx as u8;
            world.current_dialog = None;
            let _ = world.drain_field_events();
            world.load_field_script(vec![0x3E, 0x05, slot]);
            let _ = world.tick();
            let req = world.current_dialog.as_ref().unwrap_or_else(|| {
                panic!("[{name}] field-interact on NPC slot {slot} must open a dialogue box")
            });
            assert_eq!(
                &req.inline, inline,
                "[{name}] slot {slot} must open its own placement's inline dialogue"
            );
            total_round_trips += 1;
        }

        scenes_with_npcs += 1;
        total_slots += expected.len();
    }

    // The field-scene corpus has many NPC-bearing scenes; a regression that
    // broke scene loading or MAN parsing would collapse this to ~0.
    assert!(
        scenes_with_npcs >= 10,
        "expected the field-scene corpus to expose NPC dialogue in many scenes, got {scenes_with_npcs}"
    );
    assert_eq!(
        total_round_trips, total_slots,
        "every NPC slot across the corpus round-trips to its own dialogue"
    );
    eprintln!(
        "[corpus] field-interact slot->dialogue mapping holds across {scenes_with_npcs} scenes \
         ({total_slots} NPC slots, all round-tripped)"
    );
}
