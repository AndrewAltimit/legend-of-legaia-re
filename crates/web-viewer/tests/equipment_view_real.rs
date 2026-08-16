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
