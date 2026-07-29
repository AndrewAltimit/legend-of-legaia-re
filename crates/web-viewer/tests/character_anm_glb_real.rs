//! Disc-gated: the characters page's animation surface - the party
//! locomotion bundle exposed through the player-ANM corpus (PROT 0874 §1,
//! invisible to the type-0x05 detector), the pinned bank labels
//! (locomotion + the PROT 1203 battle bank), and the animated `.glb`
//! builds the `character_glb` wasm entry performs (replicated here via the
//! same public crate APIs - `LegaiaViewer` itself needs a wasm canvas).
//!
//! No Sony bytes are asserted - only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::disc::{extract_prot_dat, parse_prot_toc};
use legaia_web_viewer::player_anm::{bundle_json, corpus_record_meta};
use std::env;

fn prot_entry(prot: &[u8], index: u32) -> Option<&[u8]> {
    let meta = parse_prot_toc(prot)?
        .into_iter()
        .find(|e| e.index == index)?;
    let off = meta.byte_offset as usize;
    let end = off.saturating_add(meta.size_bytes as usize);
    prot.get(off..end)
}

fn loaded_prot() -> Option<Vec<u8>> {
    let path = env::var_os("LEGAIA_DISC_BIN")?;
    let disc = std::fs::read(&path).ok()?;
    extract_prot_dat(&disc)
}

#[test]
fn locomotion_bundle_surfaces_in_the_corpus_with_labels() {
    let Some(prot) = loaded_prot() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let raw = prot_entry(&prot, legaia_asset::character_pack::PROT_ENTRY_INDEX)
        .expect("PROT 0874 present");
    // The corpus detector cannot see this section (type byte 0x02)...
    for desc in [6, 3, 5, 7] {
        assert!(
            legaia_asset::player_anm::find_in_entry(raw, desc).is_empty(),
            "0874 must stay invisible to the 0x05 detector (desc {desc})"
        );
    }
    // ...the dedicated parser is what exposes it.
    let b = legaia_asset::character_pack::field_locomotion_anm(raw).expect("locomotion bundle");
    let v = bundle_json(874, "party_locomotion", &b);
    assert_eq!(v["source"], "party_locomotion");
    assert_eq!(v["record_count"], 23);
    let recs = v["records"].as_array().unwrap();
    assert_eq!(recs.len(), 23);
    // The pinned slots carry their semantic labels + bank association.
    assert_eq!(recs[0]["label"], "Vahn - Walk");
    assert_eq!(recs[0]["character"], 0);
    assert_eq!(recs[0]["bank_slot"], 0);
    assert_eq!(recs[1]["label"], "Vahn - Idle");
    assert_eq!(recs[8]["label"], "Noa - Idle");
    assert_eq!(recs[15]["label"], "Gala - Idle");
    // The unpinned family stays neutral; the two actors are labeled by role.
    assert_eq!(recs[2]["label"], "Vahn - Locomotion 2");
    assert_eq!(recs[21]["label"], "Savepoint");
    assert_eq!(recs[22]["label"], "Aux actor");
    // All three party banks are 10-bone (the runtime-capped party rig).
    for r in recs.iter().take(21) {
        assert_eq!(r["bone_count"], 10, "party bank record: {r}");
    }
    assert_eq!(recs[21]["bone_count"], 3, "savepoint rig");
    assert_eq!(recs[22]["bone_count"], 2, "aux rig");
}

#[test]
fn battle_bank_corpus_records_carry_baka_labels() {
    let Some(prot) = loaded_prot() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let raw = prot_entry(&prot, 1203).expect("PROT 1203 present");
    let mut found = Vec::new();
    for desc in [6, 3, 5, 7] {
        found = legaia_asset::player_anm::find_in_entry(raw, desc);
        if !found.is_empty() {
            break;
        }
    }
    let b = found.first().expect("1203 carries the battle bank");
    assert_eq!(b.record_count, 30);
    let v = bundle_json(1203, "battle_bank", b);
    let recs = v["records"].as_array().unwrap();
    // Bank slot 0 is the idle (display id base+1 through the container
    // header - see legaia_asset::baka_opponents::action_slot_label).
    assert_eq!(recs[0]["label"], "Vahn - Idle");
    assert_eq!(recs[4]["label"], "Vahn - Special");
    assert_eq!(recs[8]["label"], "Vahn - Win");
    assert_eq!(recs[9]["label"], "Noa - Idle");
    assert_eq!(recs[18]["label"], "Gala - Idle");
    assert_eq!(recs[27]["label"], "10-bone rig 0");
    assert_eq!(recs[27]["character"], serde_json::Value::Null);
    // Scene pools stay unlabeled.
    assert_eq!(corpus_record_meta(4, 0), None);
}

#[test]
fn field_glb_bakes_the_locomotion_bank() {
    let Some(prot) = loaded_prot() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let raw = prot_entry(&prot, legaia_asset::character_pack::PROT_ENTRY_INDEX)
        .expect("PROT 0874 present");
    let pack = legaia_asset::character_pack::parse(raw).expect("character pack");
    let bundle = legaia_asset::character_pack::field_locomotion_anm(raw).expect("locomotion");
    // Vahn, disc form, retail 10-group cap (what `character_glb(0, -1, 0)`
    // builds wasm-side).
    let mut tmd_bytes = pack.slot(0).unwrap().tmd_bytes.clone();
    tmd_bytes[0x08..0x0C].copy_from_slice(&10u32.to_le_bytes());
    let tmd = legaia_tmd::parse(&tmd_bytes).expect("field TMD");
    let (mesh, object_ids, shading) =
        legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &tmd_bytes);
    let mut vram = legaia_tim::Vram::new();
    legaia_asset::field_char_textures::parse(raw)
        .expect("field textures")
        .upload_to_vram(&mut vram, false);
    use legaia_asset::character_pack as cp;
    let picks = [
        cp::locomotion_record_index(0, cp::LOCOMOTION_IDLE_SLOT),
        cp::locomotion_record_index(0, cp::LOCOMOTION_WALK_SLOT),
    ];
    let anims: Vec<(String, _)> = picks
        .iter()
        .map(|&r| {
            let a = bundle.record_to_monster_animation(r).expect("clip adapts");
            assert_eq!(a.part_count, tmd.objects.len(), "clip fits the capped rig");
            (cp::locomotion_slot_label(r % cp::LOCOMOTION_BANK_STRIDE), a)
        })
        .collect();
    let clips: Vec<legaia_asset::character_gltf::CharacterClip<'_>> = anims
        .iter()
        .map(|(n, a)| legaia_asset::character_gltf::CharacterClip {
            name: n.clone(),
            fps: 14.0,
            anim: a,
        })
        .collect();
    let glb = legaia_asset::character_gltf::build_character_glb_hybrid(
        "Vahn_field",
        &mesh,
        &object_ids,
        &vram,
        &clips,
        Some(&shading),
    )
    .expect("field glb builds");
    assert_eq!(&glb[0..4], b"glTF");
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let root: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    let anims_j = root["animations"].as_array().unwrap();
    assert_eq!(anims_j.len(), 2);
    assert_eq!(anims_j[0]["name"], "Idle");
    assert_eq!(anims_j[1]["name"], "Walk");
    // The hybrid split is live: the field mesh has untextured body prims, so
    // both materials exist.
    assert_eq!(
        root["materials"].as_array().unwrap().len(),
        2,
        "textured + vertex-colour materials"
    );
}

#[test]
fn battle_glb_bakes_the_1203_bank() {
    let Some(prot) = loaded_prot() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let pack_raw = prot_entry(&prot, legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)
        .expect("PROT 1204 present");
    let slots = legaia_asset::battle_char_pack::parse_slots(pack_raw).expect("1204 slots");
    let bank_raw = prot_entry(&prot, 1203).expect("PROT 1203 present");
    let mut found = Vec::new();
    for desc in [6, 3, 5, 7] {
        found = legaia_asset::player_anm::find_in_entry(bank_raw, desc);
        if !found.is_empty() {
            break;
        }
    }
    let bundle = found.first().expect("battle bank");
    use legaia_asset::baka_opponents as bo;
    for (slot, name) in [(0usize, "Vahn"), (1, "Noa"), (2, "Gala")] {
        let tmd_bytes = slots[slot].tmd_bytes.clone();
        let tmd = legaia_tmd::parse(&tmd_bytes).expect("battle TMD");
        let (mesh, object_ids) =
            legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &tmd_bytes);
        let vram = legaia_tim::Vram::new();
        let anims: Vec<(String, _)> = (0..bo::ACTIONS_PER_FIGHTER)
            .map(|k| {
                let rec = slot * bo::ACTIONS_PER_FIGHTER + k;
                let a = bundle.record_to_monster_animation(rec).expect("clip");
                assert_eq!(a.part_count, tmd.objects.len(), "{name} bank fits the rig");
                (bo::action_slot_label(k).unwrap().to_string(), a)
            })
            .collect();
        let clips: Vec<legaia_asset::character_gltf::CharacterClip<'_>> = anims
            .iter()
            .map(|(n, a)| legaia_asset::character_gltf::CharacterClip {
                name: n.clone(),
                fps: 14.0,
                anim: a,
            })
            .collect();
        let glb = legaia_asset::character_gltf::build_character_glb(
            name,
            &mesh,
            &object_ids,
            &vram,
            &clips,
        )
        .expect("battle glb builds");
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let root: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let names: Vec<&str> = root["animations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "Idle",
                "Attack 1 (Square)",
                "Attack 2 (Circle)",
                "Attack 3 (Cross)",
                "Special",
                "Hit / Knockdown 1",
                "Hit / Knockdown 2",
                "Hit / Knockdown 3",
                "Win",
            ],
            "{name}: the 9-record bank under its pinned labels"
        );
    }
}
