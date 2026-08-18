//! Disc-gated: the equipment-loadout WASM surface (`LegaiaViewer`'s
//! `equipment_*` / `equipped_*` methods) must enumerate every player battle
//! file's equipment sections, assemble a non-default loadout into a drawable
//! textured mesh, classify what the loadout changed, and bake a `.glb`.
//!
//! Structural facts only - no Sony bytes are asserted. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_asset::battle_char_assembly::equip_diff;
use legaia_web_viewer::LegaiaViewer;

/// The live full-party save's Vahn loadout (char record `+0x196..+0x19A`):
/// Hunter Clothes body, bare head, Survival Knife, Ra-Seru Meta, bare feet.
/// Pinned byte-exact by `legaia-asset`'s `battle_char_assembly_real`.
const VAHN_SAVE_LOADOUT: [u8; 5] = [0x43, 0x00, 0x22, 0x01, 0x00];

fn loaded() -> Option<LegaiaViewer> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = match std::fs::read(&disc) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[skip] {disc}: {e}");
            return None;
        }
    };
    let mut v = LegaiaViewer::new_headless();
    v.load_disc(bytes).ok()?;
    Some(v)
}

/// The equipment each player file offers is exactly enumerable off the
/// 12-byte descriptor chain, and the counts are a fixed property of the disc.
#[test]
fn enumerates_every_equipment_section() {
    let Some(v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pack: serde_json::Value =
        serde_json::from_str(&v.equipment_pack_json()).expect("equipment pack JSON");
    let slots = pack["slots"].as_array().expect("slots array");
    assert_eq!(slots.len(), 4, "four player battle files");

    // Per-character real id counts per section (excluding the id = 0 default),
    // and the descriptor-table record count.
    let expected: [([usize; 5], usize, &str); 4] = [
        ([9, 5, 19, 9, 7], 54, "Vahn"),
        ([6, 6, 8, 20, 5], 50, "Noa"),
        ([5, 4, 18, 7, 4], 43, "Gala"),
        ([0, 0, 0, 0, 0], 5, "Terra"),
    ];
    for (s, (counts, records, label)) in slots.iter().zip(expected) {
        assert_eq!(s["label"].as_str(), Some(label));
        assert_eq!(
            s["records"].as_u64().unwrap() as usize,
            records,
            "{label}: descriptor records"
        );
        let sections = s["sections"].as_array().expect("sections");
        assert_eq!(sections.len(), 5, "{label}: five equipment sections");
        for (i, want) in counts.iter().enumerate() {
            let got = sections[i]["items"].as_array().unwrap().len();
            assert_eq!(got, *want, "{label}: section {i} id count");
        }
    }

    // Section order is per-character: Vahn's weapons are section 2 and his
    // Ra-Seru section 3, Noa's are the other way round. The labels are
    // derived from the equipment table, so they must reflect that swap.
    for (slot, weapon, ra_seru) in [(0usize, 2usize, 3usize), (1, 3, 2), (2, 2, 3)] {
        let w = slots[slot]["sections"][weapon]["label"].as_str().unwrap();
        let r = slots[slot]["sections"][ra_seru]["label"].as_str().unwrap();
        let who = slots[slot]["label"].as_str().unwrap();
        assert!(w.contains("Weapon"), "{who} section {weapon}: {w}");
        assert!(r.contains("Ra-Seru"), "{who} section {ra_seru}: {r}");
    }
    for (slot, body, head, feet) in [(0usize, 0usize, 1usize, 4usize), (1, 0, 1, 4), (2, 0, 1, 4)] {
        let s = &slots[slot]["sections"];
        assert!(s[body]["label"].as_str().unwrap().contains("Body"));
        assert!(s[head]["label"].as_str().unwrap().contains("Head"));
        assert!(s[feet]["label"].as_str().unwrap().contains("Feet"));
    }
}

/// A non-default loadout assembles into a drawable, textured mesh whose
/// object ids stay inside the pose rig - and it must differ from the
/// all-defaults assembly, or the picker changes nothing.
#[test]
fn a_non_default_loadout_changes_the_mesh() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let bare: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0; 5], false)).unwrap();
    assert_eq!(bare["ok"], serde_json::json!(true), "{bare}");
    let bare_tris = bare["triangles"].as_u64().unwrap();
    assert!(bare_tris > 0, "bare assembly has triangles");
    assert!(
        bare["changed_objects"].as_array().unwrap().is_empty(),
        "the all-defaults loadout changes nothing"
    );

    let eq: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &VAHN_SAVE_LOADOUT, false)).unwrap();
    assert_eq!(eq["ok"], serde_json::json!(true), "{eq}");
    assert_ne!(
        eq["triangles"].as_u64().unwrap(),
        bare_tris,
        "the equipped assembly must not be the bare one"
    );

    // The sections the loadout named resolved to those ids; the two it left
    // at zero took their section default.
    let sections = eq["sections"].as_array().unwrap();
    for (i, want) in VAHN_SAVE_LOADOUT.iter().enumerate() {
        assert_eq!(
            sections[i]["resolved"].as_u64().unwrap() as u8,
            *want,
            "section {i} resolved id"
        );
    }
    assert!(
        sections[2]["name"].as_str().is_some(),
        "the weapon section resolves to a named item"
    );

    // Mesh buffers are parallel and the pose rig covers every vertex.
    let pos = v.equipped_mesh_positions();
    let ids = v.equipped_mesh_object_ids();
    let uvs = v.equipped_mesh_uvs();
    let ct = v.equipped_mesh_cba_tsb();
    let idx = v.equipped_mesh_indices();
    let verts = pos.len() / 3;
    assert!(verts > 0 && !idx.is_empty(), "drawable mesh");
    assert_eq!(ids.len(), verts, "object ids parallel to positions");
    assert_eq!(uvs.len(), verts * 2, "uvs parallel to positions");
    assert_eq!(ct.len(), verts * 2, "cba/tsb parallel to positions");
    assert_eq!(idx.len() % 3, 0, "triangle list");
    let parts = eq["part_count"].as_u64().unwrap() as u32;
    assert!(
        ids.iter().all(|&o| o < parts),
        "every vertex poses on a channel the clip bank supplies"
    );
    assert!(
        idx.iter().all(|&i| (i as usize) < verts),
        "every index is in range"
    );

    // The band VRAM is the full 1 MB image and carries painted texels - a
    // partial upload (one section only) would leave the `clut_n == 0` blocks
    // unpainted.
    let vram = v.equipped_vram_bytes();
    assert_eq!(vram.len(), 1024 * 512 * 2, "1 MB PSX VRAM");
    assert!(
        vram.iter().filter(|b| **b != 0).count() > 10_000,
        "band VRAM carries the character's texture pages"
    );

    // Every clip's pose buffer is the rig width the mesh needs.
    let clips = eq["clips"].as_array().unwrap();
    assert!(!clips.is_empty(), "the record[0] action bank decoded");
    for (i, clip) in clips.iter().enumerate() {
        let frames = clip["frames"].as_u64().unwrap() as usize;
        let pose = v.equipped_pose_frames(i as u32);
        assert_eq!(
            pose.len(),
            frames * parts as usize * 6,
            "clip {i} ({}) pose buffer",
            clip["label"]
        );
    }
}

/// A `200+`-tagged object that is a byte-copy of its attach bone must not be
/// drawn (it z-fights the real limb) - but one that is **not** a copy carries
/// real geometry and must be kept. Vahn's unequipped assembly is the first
/// case, Noa's the second, so a rule that only looked at the tag would be
/// wrong on one of them whichever way it went.
#[test]
fn duplicate_objects_are_dropped_and_non_duplicates_are_kept() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let drawn = |v: &LegaiaViewer| -> std::collections::BTreeSet<u32> {
        v.equipped_mesh_object_ids().into_iter().collect()
    };
    // Vahn: both `200+` extras copy their attach bone -> neither is drawn.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0; 5], false)).unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");
    let parts = s["part_count"].as_u64().unwrap() as u32;
    let ids = drawn(&v);
    assert!(
        ids.iter().all(|&o| o < parts - 2),
        "Vahn: a duplicate object reached the mesh ({ids:?} of {parts})"
    );
    // Noa: her `200+` extras differ from their hosts and must survive.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(1, &[0; 5], false)).unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");
    let parts = s["part_count"].as_u64().unwrap() as u32;
    let ids = drawn(&v);
    assert!(
        ids.iter().any(|&o| o >= parts - 2),
        "Noa: her non-duplicate 200+ geometry was dropped ({ids:?} of {parts})"
    );
    // Every character still assembles and draws.
    for slot in 2..4u32 {
        let s: serde_json::Value =
            serde_json::from_str(&v.set_equipped_character(slot, &[0; 5], false)).unwrap();
        assert_eq!(s["ok"], serde_json::json!(true), "slot {slot}: {s}");
        assert!(
            !v.equipped_mesh_indices().is_empty(),
            "slot {slot} drawable"
        );
    }
}

/// The **item cut**. A weapon is not its own object, but it is an exact
/// primitive subset of the hand object selected by palette column, and that
/// cut has to hold across the disc - including the two readings that look
/// right and are not.
#[test]
fn a_weapon_separates_from_the_hand_by_palette() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Gala's Chaos Breaker: the item is a connected component of its own.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(2, &[0, 0, 0x27, 0, 0], false)).unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");
    let items = s["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "one equipped item section: {s}");
    let it = &items[0];
    assert_eq!(it["class"], serde_json::json!("separate"), "{it}");
    assert_eq!(it["seam_vertices"].as_u64(), Some(0), "{it}");
    assert_eq!(it["complete"], serde_json::json!(true), "{it}");
    assert!(it["item_primitives"].as_u64().unwrap() > 20, "{it}");
    assert!(it["limb_primitives"].as_u64().unwrap() > 20, "{it}");

    // Vahn's Survival Knife: welded at the grip, so the export is NOT
    // advertised as complete - the shaft inside the closed fist was never
    // modelled, and no cut recovers it.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0, 0, 0x22, 0, 0], false)).unwrap();
    let it = &s["items"].as_array().unwrap()[0];
    assert_eq!(it["class"], serde_json::json!("welded"), "{it}");
    assert!(it["seam_vertices"].as_u64().unwrap() > 0, "{it}");
    assert_eq!(it["complete"], serde_json::json!(false), "{it}");
    // The measurement that rules out a set-difference reading: the cut
    // claims a real slice of the object, not almost all of it.
    let item_p = it["item_primitives"].as_u64().unwrap();
    let limb_p = it["limb_primitives"].as_u64().unwrap();
    assert!(item_p > 0 && limb_p > 0, "{it}");
    assert!(item_p < limb_p * 3, "the cut swallowed the hand: {it}");

    // Noa's Heavy Strike is already its own object - retail shipped the split.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(1, &[0, 0, 0, 0x1e, 0], false)).unwrap();
    let it = &s["items"].as_array().unwrap()[0];
    assert_eq!(it["class"], serde_json::json!("own-object"), "{it}");
    assert_eq!(it["complete"], serde_json::json!(true), "{it}");

    // Armour has no body-without-armour to subtract - so it exports FUSED
    // with its host geometry, and says so. Never nothing: every equipped
    // section yields an item, by policy.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0x43, 0x38, 0, 0, 0x5f], false))
            .unwrap();
    let items = s["items"].as_array().unwrap();
    assert_eq!(items.len(), 3, "one item per equipped section: {s}");
    for it in items {
        assert_eq!(it["class"], serde_json::json!("fused"), "{it}");
        assert_eq!(it["pure"], serde_json::json!(false), "{it}");
        assert_eq!(it["complete"], serde_json::json!(true), "{it}");
        assert!(it["item_primitives"].as_u64().unwrap() > 0, "{it}");
        assert!(
            it["describe"].as_str().unwrap().contains("fused"),
            "caption vocabulary: {it}"
        );
    }
    let sections: Vec<u64> = items
        .iter()
        .map(|i| i["section"].as_u64().unwrap())
        .collect();
    assert_eq!(sections, vec![0, 1, 4]);
}

/// The per-item `.glb` ships the item **and** the limb it was cut from, as
/// two named nodes, so a reader can see what was and was not taken.
#[test]
fn the_item_glb_carries_the_item_and_its_host_limb() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(2, &[0, 0, 0x27, 0, 0], false)).unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");
    let glb = v.equipped_item_glb(2);
    assert!(glb.len() > 512, "item glb baked ({} bytes)", glb.len());
    assert_eq!(&glb[0..4], b"glTF");
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&glb[20..20 + json_len]).expect("glTF JSON");
    let doc: serde_json::Value = serde_json::from_str(json.trim_end()).expect("parse glTF");
    let names: Vec<String> = doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.iter().any(|n| n == "Chaos Breaker"),
        "item node missing: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("host limb")),
        "host limb node missing: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.starts_with("Chaos Breaker - cut from")),
        "root node does not say where the item came from: {names:?}"
    );

    // A standalone item's partition names only its own object, so the export
    // has to pull in the bone it rides - the limb is ground truth and must be
    // in every item file.
    let _: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(1, &[0, 0, 0, 0x1e, 0], false)).unwrap();
    let glb = v.equipped_item_glb(3);
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&glb[20..20 + json_len]).expect("glTF JSON");
    let doc: serde_json::Value = serde_json::from_str(json.trim_end()).expect("parse glTF");
    let names: Vec<String> = doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.iter().any(|n| n == "Heavy Strike"),
        "standalone item node missing: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("host limb")),
        "standalone item shipped without its limb: {names:?}"
    );

    // A welded cut must say the grip is open, in the file itself.
    let _: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0, 0, 0x33, 0, 0], false)).unwrap();
    let glb = v.equipped_item_glb(2);
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&glb[20..20 + json_len]).expect("glTF JSON");
    assert!(json.contains("grip open"), "welded glb hides the open grip");

    // An armour section exports too - fused, and the file says so. A section
    // at its default has nothing to export, and that is the only empty case.
    let _: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0x43, 0, 0, 0, 0], false)).unwrap();
    let glb = v.equipped_item_glb(0);
    assert!(
        glb.len() > 512,
        "armour item glb baked ({} bytes)",
        glb.len()
    );
    assert_eq!(&glb[0..4], b"glTF");
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&glb[20..20 + json_len]).expect("glTF JSON");
    assert!(json.contains("Hunter Clothes"), "armour glb names its item");
    assert!(
        json.contains("fused with the host limb"),
        "armour glb does not say it is fused"
    );
    assert!(json.contains("as spliced into"), "fused root name wording");
    assert!(
        v.equipped_item_glb(2).is_empty(),
        "an unequipped section exported"
    );
}

/// The item-**alone** download is the opinionated second cut: no host limb
/// node, fewer triangles than the record-keeping export, its root says how
/// it was decided, and the per-vertex mask the page previews it with agrees
/// with it triangle for triangle.
#[test]
fn the_item_alone_glb_drops_the_limb_and_matches_the_preview_mask() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gltf_nodes = |glb: &[u8]| -> (serde_json::Value, Vec<String>) {
        assert_eq!(&glb[0..4], b"glTF");
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len]).expect("glTF JSON");
        let doc: serde_json::Value = serde_json::from_str(json.trim_end()).expect("parse glTF");
        let names = doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["name"].as_str().map(str::to_string))
            .collect();
        (doc, names)
    };
    let tri_count = |doc: &serde_json::Value| -> u64 {
        doc["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["primitives"].as_array().unwrap().iter())
            .map(|p| {
                let acc = p["indices"].as_u64().unwrap() as usize;
                doc["accessors"][acc]["count"].as_u64().unwrap() / 3
            })
            .sum()
    };

    // Vahn's Great Axe: welded to the fist, and the palette cut also claims
    // the wrist band. The item-alone file has the axe and nothing else.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0, 0, 0x33, 0, 0], false)).unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");
    let it = s["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["section"] == 2)
        .expect("weapon item");
    let iso = &it["isolation"];
    assert_eq!(iso["mode"], "colour-diff", "held-item default reading");
    assert!(iso["kept_primitives"].as_u64().unwrap() > 0, "{iso}");
    assert!(iso["dropped_primitives"].as_u64().unwrap() > 0, "{iso}");
    // Fewer than the palette cut: the wrist band it claimed is body here.
    assert!(
        iso["kept_primitives"].as_u64().unwrap() < it["item_primitives"].as_u64().unwrap(),
        "axe: item-alone {} vs palette item {}",
        iso["kept_primitives"],
        it["item_primitives"]
    );

    let with_limb = v.equipped_item_glb(2);
    let alone = v.equipped_item_only_glb(2);
    assert!(
        alone.len() > 512,
        "item-alone glb baked ({} bytes)",
        alone.len()
    );
    let (doc_limb, names_limb) = gltf_nodes(&with_limb);
    let (doc_alone, names_alone) = gltf_nodes(&alone);
    assert!(names_limb.iter().any(|n| n.contains("host limb")));
    assert!(
        !names_alone.iter().any(|n| n.contains("host limb")),
        "item-alone file carries a limb node: {names_alone:?}"
    );
    assert!(
        names_alone.iter().any(|n| n == "Great Axe"),
        "item node missing: {names_alone:?}"
    );
    assert!(
        names_alone
            .iter()
            .any(|n| n.starts_with("Great Axe - item alone") && n.contains("colour-diff")),
        "root does not say how it was decided: {names_alone:?}"
    );
    assert!(
        tri_count(&doc_alone) < tri_count(&doc_limb),
        "item-alone ({}) not smaller than item+limb ({})",
        tri_count(&doc_alone),
        tri_count(&doc_limb)
    );
    // The clip bank rides along, so the axe still swings.
    assert!(!doc_alone["animations"].as_array().unwrap().is_empty());
    // The haft leaves the cut in two pieces (the fist covered the middle);
    // the grip repair bridges them, and the file says so.
    let bridges = iso["bridges"].as_u64().unwrap();
    let bridged_tris = iso["bridged_triangles"].as_u64().unwrap();
    assert!(bridges >= 1 && bridged_tris >= 6, "Great Axe grip: {iso}");
    assert!(
        names_alone.iter().any(|n| n.contains("grip inferred")),
        "root does not say the grip was inferred: {names_alone:?}"
    );

    // The preview mask: one byte per cached vertex, and its `2` triangles
    // are exactly the item-alone triangle count before the repair.
    let mask = v.equipped_mesh_item_mask(2);
    let positions = v.equipped_mesh_positions();
    assert_eq!(mask.len(), positions.len() / 3, "mask is per vertex");
    let indices = v.equipped_mesh_indices();
    let kept_tris = indices
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&i| mask[i as usize] == 2))
        .count() as u64;
    assert_eq!(
        kept_tris + bridged_tris,
        tri_count(&doc_alone),
        "mask + bridge vs item-alone glb"
    );
    // The item-only preview mesh IS the exported geometry: parallel streams,
    // the same triangle count as the file, object ids inside the rig.
    let ipos = v.equipped_item_only_positions(2);
    let iidx = v.equipped_item_only_indices(2);
    let iobj = v.equipped_item_only_object_ids(2);
    assert_eq!(iidx.len() as u64 / 3, tri_count(&doc_alone));
    assert_eq!(ipos.len() / 3, iobj.len());
    assert_eq!(v.equipped_item_only_uvs(2).len() / 2, iobj.len());
    assert_eq!(v.equipped_item_only_cba_tsb(2).len() / 2, iobj.len());
    assert_eq!(v.equipped_item_only_flat_rgba(2).len() / 4, iobj.len());
    let parts = s["part_count"].as_u64().unwrap() as u32;
    assert!(iobj.iter().all(|&o| o < parts), "object ids inside the rig");
    assert!(v.equipped_item_only_bounds(2)[3] > 0.0);
    assert!(v.equipped_item_only_positions(0).is_empty());
    assert!(
        mask.contains(&1),
        "the limb the cut left behind is masked 1"
    );
    assert!(
        mask.contains(&0),
        "objects outside the section are masked 0"
    );
    assert!(
        v.equipped_mesh_item_mask(0).is_empty(),
        "unequipped section has no mask"
    );
    assert!(v.equipped_item_only_glb(0).is_empty());

    // A curated record says so, in the summary and in the file: Vahn's
    // Warrior Seal is four dark circlet primitives the colour diff would
    // call hair, and the rule table keeps its palette column.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0, 0x34, 0, 0, 0], false)).unwrap();
    let it = s["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["section"] == 1)
        .expect("head item");
    assert_eq!(it["isolation"]["curated"], serde_json::json!(true), "{it}");
    assert_eq!(
        it["isolation"]["kept_primitives"],
        serde_json::json!(4),
        "{it}"
    );
    assert!(
        it["isolation"]["note"]
            .as_str()
            .unwrap()
            .contains("circlet"),
        "{it}"
    );
    let (_, names) = gltf_nodes(&v.equipped_item_only_glb(1));
    assert!(
        names.iter().any(|n| n.contains("curated")),
        "curated record not marked in the file: {names:?}"
    );

    // Body armour reads by identity: Hunter Clothes keeps most of the torso
    // (a re-sculpt) but leaves the neck skin behind.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0x43, 0, 0, 0, 0], false)).unwrap();
    let it = s["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["section"] == 0)
        .expect("body item");
    assert_eq!(it["isolation"]["mode"], "identity");
    let kept = it["isolation"]["kept_primitives"].as_u64().unwrap();
    let dropped = it["isolation"]["dropped_primitives"].as_u64().unwrap();
    assert!(
        kept > dropped && dropped > 0,
        "Hunter Clothes: kept {kept} dropped {dropped}"
    );
}

/// The equipment panel's item cards: one single-item build per
/// `(character, section, id)`, cached across the metadata / thumbnail /
/// download calls, thumbnail drawn by the software rasteriser.
#[test]
fn item_cards_carry_metadata_a_thumbnail_and_downloads() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Vahn's Great Axe as a card.
    let card: serde_json::Value =
        serde_json::from_str(&v.equipment_item_card_json(0, 2, 0x33)).unwrap();
    assert_eq!(card["ok"], serde_json::json!(true), "{card}");
    assert_eq!(card["name"], "Great Axe");
    assert_eq!(card["class"], "welded");
    assert_eq!(card["isolation"]["mode"], "colour-diff");
    assert!(
        card["isolation"]["bridges"].as_u64().unwrap() >= 1,
        "{card}"
    );
    assert!(card["alone_triangles"].as_u64().unwrap() > 20, "{card}");
    let px = v.equipment_item_card_pixels(96);
    assert_eq!(px.len(), 96 * 96 * 4);
    let opaque = px.chunks_exact(4).filter(|p| p[3] == 255).count();
    // Something drew, and the background stayed transparent.
    assert!(
        opaque > 96 * 96 / 40 && opaque < 96 * 96 * 9 / 10,
        "opaque {opaque}"
    );
    let alone = v.equipment_item_card_glb(true);
    let with_limb = v.equipment_item_card_glb(false);
    assert!(alone.len() > 512 && with_limb.len() > alone.len());
    // The card is independent of the main loadout: nothing was equipped.
    assert!(v.equipped_item_only_glb(2).is_empty());
    // A curated head item and a body item card too.
    let seal: serde_json::Value =
        serde_json::from_str(&v.equipment_item_card_json(0, 1, 0x34)).unwrap();
    assert_eq!(
        seal["isolation"]["curated"],
        serde_json::json!(true),
        "{seal}"
    );
    assert_eq!(
        seal["isolation"]["bridges"],
        serde_json::json!(0),
        "a circlet has no grip"
    );
    let px = v.equipment_item_card_pixels(64);
    assert!(px.chunks_exact(4).any(|p| p[3] == 255));
    // Out of range / default id refuse cleanly.
    let bad: serde_json::Value =
        serde_json::from_str(&v.equipment_item_card_json(0, 2, 0)).unwrap();
    assert_eq!(bad["ok"], serde_json::json!(false));
    let bad: serde_json::Value =
        serde_json::from_str(&v.equipment_item_card_json(7, 2, 0x33)).unwrap();
    assert_eq!(bad["ok"], serde_json::json!(false));

    // Visual pass: `LEGAIA_EQUIP_SHEETS=<dir>` writes one card sheet per
    // character - every item's thumbnail exactly as the page will show it.
    let Some(dir) = std::env::var_os("LEGAIA_EQUIP_SHEETS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let pack: serde_json::Value = serde_json::from_str(&v.equipment_pack_json()).unwrap();
    let size = 96usize;
    let cols = 8usize;
    for slot in pack["slots"].as_array().unwrap() {
        let cslot = slot["slot"].as_u64().unwrap() as u32;
        let mut cards: Vec<(u32, u32)> = Vec::new();
        for sec in slot["sections"].as_array().unwrap() {
            let si = sec["index"].as_u64().unwrap() as u32;
            for it in sec["items"].as_array().unwrap() {
                cards.push((si, it["id"].as_u64().unwrap() as u32));
            }
        }
        if cards.is_empty() {
            continue;
        }
        let rows = cards.len().div_ceil(cols);
        let (w, h) = (cols * (size + 4), rows * (size + 4));
        let mut img = vec![0u8; w * h * 4];
        for px in img.chunks_exact_mut(4) {
            px.copy_from_slice(&[24, 26, 32, 255]);
        }
        for (k, (si, id)) in cards.iter().enumerate() {
            let c: serde_json::Value =
                serde_json::from_str(&v.equipment_item_card_json(cslot, *si, *id)).unwrap();
            if c["ok"] != serde_json::json!(true) {
                continue;
            }
            let px = v.equipment_item_card_pixels(size as u32);
            let panel = legaia_asset::mesh_raster::Rgba {
                pixels: &px,
                width: size,
                height: size,
            };
            legaia_asset::mesh_raster::blit(
                &mut img,
                w,
                h,
                &panel,
                (k % cols) * (size + 4),
                (k / cols) * (size + 4),
            );
        }
        let who = slot["label"].as_str().unwrap().to_ascii_lowercase();
        legaia_tim::write_png(&dir.join(format!("{who}_cards.png")), w, h, &img).unwrap();
    }
}

/// The diff highlight must classify something on a weapon swap, and the
/// per-object summary must show the weapon bone growing rather than a new
/// object appearing - there is no separable item mesh.
#[test]
fn the_diff_highlight_classifies_a_weapon_swap() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Vahn with only a Survival Knife: one section away from bare.
    let eq: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0, 0, 0x22, 0, 0], true)).unwrap();
    assert_eq!(eq["ok"], serde_json::json!(true), "{eq}");
    let changed = eq["changed_objects"].as_array().unwrap();
    assert_eq!(
        changed.len(),
        1,
        "a weapon-only swap re-authors exactly one bone object"
    );
    let d = &changed[0];
    assert!(
        d["equipped_vertices"].as_u64().unwrap() > d["bare_vertices"].as_u64().unwrap(),
        "the weapon bone gains geometry: {d}"
    );
    assert!(
        d["added_primitives"].as_u64().unwrap() > 0,
        "the envelope test finds geometry beyond the bare hand: {d}"
    );
    assert!(
        d["straddling_primitives"].as_u64().unwrap() > 0,
        "and a shared boundary between hand and weapon: {d}"
    );
    // The measurement that rules out a positional set-difference: the
    // equipped hand shares almost no vertex positions with the bare one.
    assert!(
        d["shared_vertex_positions"].as_u64().unwrap() * 4
            < d["equipped_vertices"].as_u64().unwrap(),
        "a set-difference would call nearly everything added: {d}"
    );

    // All three classes must be present in the rendered stream, or the mode
    // colours nothing.
    let class = v.equipped_mesh_diff_class();
    assert_eq!(class.len(), v.equipped_mesh_positions().len() / 3);
    for (name, want) in [
        ("shared", equip_diff::CLASS_SHARED),
        ("added", equip_diff::CLASS_ADDED),
        ("bare-only", equip_diff::CLASS_BARE_ONLY),
    ] {
        assert!(
            class.contains(&want),
            "the diff mesh carries no {name} vertices"
        );
    }
    // The tint stream is parallel and actually differs by class.
    let flat = v.equipped_mesh_flat_rgba();
    assert_eq!(flat.len(), class.len() * 4);
    let tint_of = |want: u8| -> [u8; 3] {
        let v = class.iter().position(|c| *c == want).unwrap();
        [flat[v * 4], flat[v * 4 + 1], flat[v * 4 + 2]]
    };
    let shared = tint_of(equip_diff::CLASS_SHARED);
    let added = tint_of(equip_diff::CLASS_ADDED);
    let bare = tint_of(equip_diff::CLASS_BARE_ONLY);
    assert_ne!(shared, added);
    assert_ne!(shared, bare);
    assert_ne!(added, bare);

    // Off by default: the same loadout without the flag is the plain model.
    let plain: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0, 0, 0x22, 0, 0], false)).unwrap();
    assert_eq!(plain["diff"], serde_json::json!(false));
    assert!(
        v.equipped_mesh_positions().len() < eq["vertices"].as_u64().unwrap() as usize * 3,
        "the plain view drops the replaced bare geometry the diff view adds"
    );
}

/// A full loadout re-sculpts most of the body - the finding the page has to
/// state, and the reason "show me the item" cannot be answered literally.
#[test]
fn a_full_loadout_reauthors_most_of_the_body() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Vahn's endgame set: Ra-Seru Armor / Seal / Blade-tier sword / Meta / Boots.
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0x4b, 0x38, 0xba, 0x09, 0x5f], true))
            .unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");
    let changed = s["changed_objects"].as_array().unwrap();
    assert!(
        changed.len() >= 10,
        "a full loadout re-authors most bone objects, got {}",
        changed.len()
    );
    // At least one object *loses* geometry: the sections are re-sculpts, not
    // layers stacked on top of the bare body.
    assert!(
        changed.iter().any(
            |d| d["equipped_vertices"].as_u64().unwrap() < d["bare_vertices"].as_u64().unwrap()
        ),
        "no object shrank - equipment would then be additive, which it is not"
    );
}

/// The `.glb` bakes the whole posed character with its clip bank, and names
/// itself after the character wearing the gear - never after the item, which
/// has no mesh of its own.
#[test]
fn the_glb_is_the_whole_character_and_says_so() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &VAHN_SAVE_LOADOUT, false)).unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");

    let name = v.equipped_glb_name();
    assert!(name.starts_with("Vahn - battle model"), "glb name: {name}");
    assert!(
        name.contains("Survival Knife"),
        "the name lists what is worn: {name}"
    );

    let glb = v.equipped_character_glb();
    assert!(glb.len() > 1024, "glb baked ({} bytes)", glb.len());
    assert_eq!(&glb[0..4], b"glTF", "glb magic");
    let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
    assert_eq!(total, glb.len(), "glb declared length matches");
    // Chunk 0 is the JSON scene; it must carry the animation bank and the
    // honest root-node name.
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&glb[20..20 + json_len]).expect("glTF JSON chunk");
    let doc: serde_json::Value = serde_json::from_str(json.trim_end()).expect("parse glTF JSON");
    assert!(
        doc["animations"].as_array().is_some_and(|a| !a.is_empty()),
        "the export carries named animation clips"
    );
    assert!(
        json.contains("Survival Knife"),
        "root node names the loadout"
    );
}

// ---------------------------------------------------------------------------
// Item-export placement
//
// A battle pose is flat: each object is placed by its own absolute `R.v + T`
// about the object origin, nothing hangs off a parent. So a glTF node with no
// transform sits at the model origin, and every such node piles onto every
// other. The item export used to pass no clips, and the builder takes its
// per-node rest transform from clip 0 frame 0 - so every node came out
// untransformed and a two-object export (Vahn's weapon spans forearm and
// hand; a fused armour spans the torso chain) read as two limbs stacked on
// each other.
// ---------------------------------------------------------------------------

/// The glTF JSON chunk of a `.glb`.
fn gltf_json(glb: &[u8]) -> serde_json::Value {
    assert_eq!(&glb[0..4], b"glTF", "glb magic");
    let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let text = std::str::from_utf8(&glb[20..20 + n]).expect("glTF JSON chunk");
    serde_json::from_str(text.trim_end()).expect("parse glTF JSON")
}

/// Every node that draws geometry, as `(name, translation, rotation, local
/// POSITION bounds)`. The bounds come from the accessor's own `min`/`max`,
/// which the builder always writes for POSITION.
type MeshNode = (String, [f32; 3], [f32; 4], [f32; 3], [f32; 3]);

fn mesh_nodes(doc: &serde_json::Value) -> Vec<MeshNode> {
    let arr = |v: &serde_json::Value| -> Vec<f32> {
        v.as_array()
            .map(|a| a.iter().map(|x| x.as_f64().unwrap() as f32).collect())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for node in doc["nodes"].as_array().unwrap() {
        let Some(mesh) = node["mesh"].as_u64() else {
            continue;
        };
        let t = arr(&node["translation"]);
        let r = arr(&node["rotation"]);
        let prim = &doc["meshes"][mesh as usize]["primitives"][0];
        let acc = prim["attributes"]["POSITION"].as_u64().unwrap() as usize;
        let lo = arr(&doc["accessors"][acc]["min"]);
        let hi = arr(&doc["accessors"][acc]["max"]);
        assert_eq!(lo.len(), 3, "POSITION accessor carries no bounds");
        out.push((
            node["name"].as_str().unwrap_or("?").to_string(),
            [
                *t.first().unwrap_or(&0.0),
                *t.get(1).unwrap_or(&0.0),
                *t.get(2).unwrap_or(&0.0),
            ],
            [
                *r.first().unwrap_or(&0.0),
                *r.get(1).unwrap_or(&0.0),
                *r.get(2).unwrap_or(&0.0),
                *r.get(3).unwrap_or(&1.0),
            ],
            [lo[0], lo[1], lo[2]],
            [hi[0], hi[1], hi[2]],
        ));
    }
    out
}

/// Rotate `v` by quaternion `q` (`[x, y, z, w]`).
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let uv = [
        y * v[2] - z * v[1],
        z * v[0] - x * v[2],
        x * v[1] - y * v[0],
    ];
    let uuv = [
        y * uv[2] - z * uv[1],
        z * uv[0] - x * uv[2],
        x * uv[1] - y * uv[0],
    ];
    std::array::from_fn(|i| v[i] + 2.0 * (w * uv[i] + uuv[i]))
}

/// World-space AABB of a mesh node: its local bounds' eight corners through
/// `R` then `T`. `posed = false` drops the transform, which is exactly the
/// state the bug left every node in.
fn node_aabb(n: &MeshNode, posed: bool) -> ([f32; 3], [f32; 3]) {
    let (_, t, r, lo, hi) = n;
    let mut out_lo = [f32::MAX; 3];
    let mut out_hi = [f32::MIN; 3];
    for cx in 0..8 {
        let c = [
            if cx & 1 == 0 { lo[0] } else { hi[0] },
            if cx & 2 == 0 { lo[1] } else { hi[1] },
            if cx & 4 == 0 { lo[2] } else { hi[2] },
        ];
        let p = if posed {
            let rc = rotate(*r, c);
            [rc[0] + t[0], rc[1] + t[1], rc[2] + t[2]]
        } else {
            c
        };
        for k in 0..3 {
            out_lo[k] = out_lo[k].min(p[k]);
            out_hi[k] = out_hi[k].max(p[k]);
        }
    }
    (out_lo, out_hi)
}

fn volume(b: ([f32; 3], [f32; 3])) -> f32 {
    (0..3).map(|k| (b.1[k] - b.0[k]).max(0.0)).product()
}

/// Overlap volume of two AABBs as a fraction of the smaller one's volume.
fn overlap_fraction(a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3])) -> f32 {
    let inter: f32 = (0..3)
        .map(|k| (a.1[k].min(b.1[k]) - a.0[k].max(b.0[k])).max(0.0))
        .product();
    let smaller = volume(a).min(volume(b));
    if smaller <= 0.0 { 0.0 } else { inter / smaller }
}

/// Every drawable node in an item export carries the rest transform of the
/// object it came from - including the synthetic item nodes, which no
/// animation channel addresses by id and which therefore have to inherit it.
#[test]
fn every_item_node_is_posed_by_its_source_object() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // One loadout per class, so the guard covers all four.
    let cases: [(u32, u32, [u8; 5], &str); 4] = [
        (0, 2, [0, 0, 0x22, 0, 0], "welded"),
        (2, 2, [0, 0, 0x27, 0, 0], "separate"),
        (1, 3, [0, 0, 0, 0x1e, 0], "own-object"),
        (0, 0, [0x43, 0, 0, 0, 0], "fused"),
    ];
    for (slot, section, ids, class) in cases {
        let s: serde_json::Value =
            serde_json::from_str(&v.set_equipped_character(slot, &ids, false)).unwrap();
        assert_eq!(s["ok"], serde_json::json!(true), "{class}: {s}");
        assert_eq!(s["items"][0]["class"], serde_json::json!(class), "{s}");
        let glb = v.equipped_item_glb(section);
        assert!(!glb.is_empty(), "{class}: no export");
        let doc = gltf_json(&glb);
        let nodes = mesh_nodes(&doc);
        assert!(
            nodes.len() >= 2,
            "{class}: {} drawable node(s)",
            nodes.len()
        );
        for n in &nodes {
            // The regression: every node used to come out with neither.
            assert_ne!(
                n.1,
                [0.0, 0.0, 0.0],
                "{class}: node {:?} has no translation - it will draw at the model origin",
                n.0
            );
            assert_ne!(
                n.2,
                [0.0, 0.0, 0.0, 1.0],
                "{class}: node {:?} has an identity rotation",
                n.0
            );
        }
        // An item piece rides its host object, so where both are present they
        // must sit at the same place - that is what "cut from" means.
        for item in nodes.iter().filter(|n| !n.0.contains("host limb")) {
            let Some(obj) = item
                .0
                .rsplit_once("(object ")
                .and_then(|(_, tail)| tail.trim_end_matches(')').parse::<u32>().ok())
            else {
                continue;
            };
            if let Some(host) = nodes
                .iter()
                .find(|n| n.0 == format!("Vahn - host limb (object {obj})"))
            {
                assert_eq!(item.1, host.1, "{class}: item piece adrift from its host");
                assert_eq!(item.2, host.2, "{class}: item piece unrotated vs its host");
            }
        }
        // Animation channels reach every node, including the synthetic ones.
        let anims = doc["animations"].as_array().expect("clips baked");
        assert!(!anims.is_empty(), "{class}: item export carries no clips");
        let targets: std::collections::BTreeSet<u64> = anims[0]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["target"]["node"].as_u64().unwrap())
            .collect();
        assert_eq!(
            targets.len(),
            nodes.len(),
            "{class}: {} node(s) but {} animated",
            nodes.len(),
            targets.len()
        );
    }
}

/// The user-visible symptom: "two copies of the hand". Vahn's weapon section
/// re-authors his whole right-arm chain, so the export carries two limb
/// objects; unposed they occupy the same space, posed they do not.
#[test]
fn a_multi_object_weapon_export_does_not_stack_its_pieces() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let s: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(0, &[0, 0, 0x22, 0, 0], false)).unwrap();
    assert_eq!(s["ok"], serde_json::json!(true), "{s}");
    let doc = gltf_json(&v.equipped_item_glb(2));
    let nodes = mesh_nodes(&doc);
    let limbs: Vec<&MeshNode> = nodes.iter().filter(|n| n.0.contains("host limb")).collect();
    assert_eq!(
        limbs.len(),
        2,
        "Vahn's knife spans two limb objects: {nodes:?}"
    );

    // Unposed - the state the bug shipped - the two arm objects sit on top of
    // one another, which is what read as a second hand.
    let unposed = overlap_fraction(node_aabb(limbs[0], false), node_aabb(limbs[1], false));
    assert!(
        unposed > 0.5,
        "unposed overlap {unposed:.2} - the contrast this test rests on is gone"
    );

    // Posed, what is left is the wrist. Forearm and hand meet at a joint and
    // both are elongated and rotated, so an axis-aligned box around each
    // overstates its footprint - a fraction of a limb's volume is expected to
    // remain. The claim is the *relationship*: posed overlap is a small
    // fraction of the unposed one, and nowhere near "one part on top of
    // another". Measured on the retail disc: 0.78 -> 0.13.
    let posed = overlap_fraction(node_aabb(limbs[0], true), node_aabb(limbs[1], true));
    eprintln!("[placement] Vahn knife limb overlap: unposed {unposed:.2} -> posed {posed:.2}");
    assert!(
        posed < unposed / 3.0,
        "posing barely moved the arm objects apart: {unposed:.2} -> {posed:.2}"
    );
    assert!(
        posed < 0.25,
        "posed overlap {posed:.2} is still limb-on-limb (unposed was {unposed:.2})"
    );

    // And the two nodes are genuinely far apart, not merely rotated apart.
    let d: f32 = (0..3)
        .map(|k| (limbs[0].1[k] - limbs[1].1[k]).powi(2))
        .sum::<f32>()
        .sqrt();
    assert!(d > 40.0, "limb translations only {d:.1} apart");
}

/// The loadout's clip bank carries the character's **Tactical Arts** as named
/// clips after the action bank and the swings - the same resolution the arts
/// page performs, so every curated art that has a decodable keyframe stream
/// on this disc is listed once, with its curated kind / AP / input, its pose
/// buffer is rig-width, and the same clips come back (weapon in hand) for a
/// non-default loadout. Terra has no curated table and lists none.
#[test]
fn loadout_clip_bank_carries_the_tactical_arts() {
    let Some(mut v) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let db = legaia_gamedata::Database::load();
    let curated = |c: usize| -> Vec<&legaia_gamedata::Art> {
        let ch = match c {
            0 => legaia_gamedata::Character::Vahn,
            1 => legaia_gamedata::Character::Noa,
            _ => legaia_gamedata::Character::Gala,
        };
        db.arts_for(ch).collect()
    };
    for cslot in 0..3usize {
        let s: serde_json::Value =
            serde_json::from_str(&v.set_equipped_character(cslot as u32, &[0; 5], false)).unwrap();
        assert_eq!(s["ok"], serde_json::json!(true), "{s}");
        let parts = s["part_count"].as_u64().unwrap() as usize;
        let clips = s["clips"].as_array().unwrap();
        let kinds: Vec<&str> = clips.iter().map(|c| c["kind"].as_str().unwrap()).collect();
        // Order: actions, then swings, then arts - the page groups on it.
        let first_swing = kinds
            .iter()
            .position(|k| *k == "swing")
            .expect("swings listed");
        let first_art = kinds.iter().position(|k| *k == "art").expect("arts listed");
        assert!(
            kinds[..first_swing].iter().all(|k| *k == "action"),
            "{cslot}: {kinds:?}"
        );
        assert!(
            kinds[first_swing..first_art].iter().all(|k| *k == "swing"),
            "{cslot}: {kinds:?}"
        );
        assert!(
            kinds[first_art..].iter().all(|k| *k == "art"),
            "{cslot}: {kinds:?}"
        );
        assert_eq!(
            &kinds[first_swing..first_art],
            &["swing"; 4],
            "{cslot}: four swings"
        );

        let arts: Vec<&serde_json::Value> = clips.iter().filter(|c| c["kind"] == "art").collect();
        let names: Vec<&str> = arts.iter().map(|a| a["label"].as_str().unwrap()).collect();
        eprintln!("[arts] char {cslot}: {} clips: {names:?}", names.len());
        // Every listed art is a curated art of this character, listed once,
        // in the curated table's order, and its metadata is the table's.
        let table = curated(cslot);
        let mut last = 0usize;
        for a in &arts {
            let name = a["label"].as_str().unwrap();
            let pos = table
                .iter()
                .position(|t| t.name == name)
                .unwrap_or_else(|| panic!("{cslot}: {name} is not a curated art"));
            assert!(pos >= last, "{cslot}: {name} out of curated order");
            last = pos;
            let t = table[pos];
            assert_eq!(a["art"]["ap"].as_u64().unwrap() as u32, t.ap, "{name} AP");
            let dirs: Vec<u8> = a["art"]["directions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(dirs, t.directions, "{name} input");
            assert!(
                a["art"]["segments"].as_u64().unwrap() >= 1,
                "{name} segments"
            );
            assert!(a["frames"].as_u64().unwrap() > 0, "{name} frames");
        }
        assert_eq!(
            names.len(),
            names
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "{cslot}: an art is listed twice: {names:?}"
        );
        // Every art kind is represented, and most of the table lands (a
        // handful of curated rows share a record or have no stream).
        for kind in ["regular", "hyper", "super", "miracle"] {
            assert!(
                arts.iter().any(|a| a["art"]["kind"] == kind),
                "{cslot}: no {kind} art listed: {names:?}"
            );
        }
        assert!(
            arts.len() * 10 >= table.len() * 7,
            "{cslot}: only {} of {} curated arts listed: {names:?}",
            arts.len(),
            table.len()
        );
        // Multi-strike arts concatenate their consecutive records.
        if cslot == 1 {
            let hk = arts
                .iter()
                .find(|a| a["label"] == "Hurricane Kick")
                .expect("Noa's Hurricane Kick");
            assert!(
                hk["art"]["segments"].as_u64().unwrap() >= 2,
                "Hurricane Kick chains its strikes"
            );
        }
        // Rig-width pose buffers for the art clips too.
        for (i, c) in clips.iter().enumerate() {
            let frames = c["frames"].as_u64().unwrap() as usize;
            assert_eq!(
                v.equipped_pose_frames(i as u32).len(),
                frames * parts * 6,
                "clip {i} pose"
            );
        }
        // The same arts come back on a non-default loadout (only the swings
        // are equipment-spliced).
        if cslot == 0 {
            let e: serde_json::Value =
                serde_json::from_str(&v.set_equipped_character(0, &VAHN_SAVE_LOADOUT, false))
                    .unwrap();
            let names2: Vec<&str> = e["clips"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["kind"] == "art")
                .map(|c| c["label"].as_str().unwrap())
                .collect();
            assert_eq!(names, names2, "arts independent of the loadout");
        }
    }
    // Terra: no curated table, so no art clips (and no failure).
    let t: serde_json::Value =
        serde_json::from_str(&v.set_equipped_character(3, &[0; 5], false)).unwrap();
    assert_eq!(t["ok"], serde_json::json!(true), "{t}");
    assert!(
        t["clips"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["kind"] != "art"),
        "Terra lists no arts"
    );
}
