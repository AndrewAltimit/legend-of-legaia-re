//! The party's field forms (`export-glb --party`) and their manifest.

use super::*;

// ---------------------------------------------------------------------------
// `--party`: the party's field forms
// ---------------------------------------------------------------------------

/// One party member's **field form** exported as an animated `.glb`: the PROT
/// 0874 §0 mesh (retail's 10-live-group cap applied, no equipment swap -
/// see the body) in the §2 field texture pages, with the character's
/// own 7-clip locomotion bank baked as named takes - `Idle` first (frame 0 =
/// the standing rest pose a non-autoplaying consumer shows), then `Walk`,
/// then the unpinned rest of the bank as `Locomotion N`. The same assembly
/// the site's characters page renders and downloads
/// (`web-viewer::character::character_glb`, field form).
pub struct PartyGlb {
    /// Pack slot: 0 Vahn, 1 Noa, 2 Gala.
    pub slot: usize,
    /// Display name (`Vahn` / `Noa` / `Gala`).
    pub character: String,
    /// File stem (`vahn`).
    pub file_stem: String,
    pub glb: Vec<u8>,
    /// Baked clip names in file order (`Idle`, `Walk`, `Locomotion 2`, ...).
    pub clips: Vec<String>,
    /// Rigid nodes driven by the clips (the live TMD object count).
    pub bone_count: usize,
}

/// The three active party members' field forms.
pub struct PartyExport {
    pub members: Vec<PartyGlb>,
    /// Tolerated decode degradations, labelled by character.
    pub notes: Vec<String>,
}

/// Export Vahn, Noa and Gala's field forms (the rigs that walk the towns)
/// with their locomotion banks. Files carry the export scale on their root
/// node like the scene NPC glbs ([`scale_glb_scene_roots`]), so a world
/// builder places them exactly like any villager - which is the point: the
/// Unity kit's `add_npcs` rule takes `{"scene": "party", "model": <slot>}`
/// against the [`party_manifest`] and the living town treats the pair as
/// two more talk villagers.
pub fn export_party_field_glbs(
    index: &ProtIndex,
    opts: &GlbExportOptions,
) -> Result<PartyExport, String> {
    use legaia_actor_formats::character_pack as cp;

    let raw = index
        .entry_bytes(cp::PROT_ENTRY_INDEX)
        .map_err(|e| format!("party pack (PROT {}): {e}", cp::PROT_ENTRY_INDEX))?;
    let pack = cp::parse(&raw).map_err(|e| format!("party pack: {e}"))?;
    let mut vram = legaia_tim::Vram::new();
    match legaia_actor_formats::field_char_textures::parse(&raw) {
        Ok(tex) => tex.upload_to_vram(&mut vram, false),
        Err(e) => return Err(format!("party field textures: {e}")),
    }
    let bundle = cp::field_locomotion_anm(&raw).map_err(|e| format!("party locomotion: {e}"))?;

    let mut members = Vec::new();
    let mut notes = Vec::new();
    for patch in cp::equipment_swap::ACTIVE_PARTY_SLOTS {
        let slot = patch.slot as usize;
        let character = cp::slot_label(slot).to_string();
        let Some(cslot) = pack.slot(slot) else {
            notes.push(format!("{character}: pack has no slot {slot}"));
            continue;
        };
        // The disc-form mesh, live groups capped at 10 the way FUN_8001E890
        // does so the two equipment templates (groups 10/11) are never drawn
        // as geometry. The equipment swap itself is NOT applied: for Noa and
        // Gala the raw group already equals the template-zero variant (the
        // cold new-game look), and for Vahn the port's patch index (0) names
        // his HEAD - applying it exported him headless. Which group retail
        // really patches for Vahn is an open RE question
        // (docs/formats/character-mesh.md); the raw mesh is right either way.
        let mut tmd_bytes = cslot.tmd_bytes.clone();
        if tmd_bytes.len() >= 0x0C {
            tmd_bytes[0x08..0x0C].copy_from_slice(&10u32.to_le_bytes());
        }
        let tmd = match legaia_tmd::parse(&tmd_bytes) {
            Ok(t) => t,
            Err(e) => {
                notes.push(format!("{character}: field TMD: {e}"));
                continue;
            }
        };
        let (mesh, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &tmd_bytes);
        if mesh.positions.is_empty() || mesh.indices.is_empty() {
            notes.push(format!("{character}: field mesh has no triangles"));
            continue;
        }
        let nobj = tmd.objects.len();

        // Idle leads, then Walk, then the rest of the bank in slot order.
        let mut order = vec![cp::LOCOMOTION_IDLE_SLOT, cp::LOCOMOTION_WALK_SLOT];
        for k in 0..cp::LOCOMOTION_BANK_STRIDE {
            if !order.contains(&k) {
                order.push(k);
            }
        }
        let mut anims: Vec<(
            String,
            legaia_actor_formats::monster_archive::MonsterAnimation,
        )> = Vec::new();
        for k in order {
            let rec = cp::locomotion_record_index(slot, k);
            let Some(anim) = bundle.record_to_monster_animation(rec) else {
                notes.push(format!(
                    "{character}: locomotion record {rec} did not decode"
                ));
                continue;
            };
            if anim.part_count != nobj {
                notes.push(format!(
                    "{character}: record {rec} drives {} bones, mesh has {nobj}",
                    anim.part_count
                ));
                continue;
            }
            anims.push((cp::locomotion_slot_label(k), anim));
        }
        if anims.is_empty() {
            notes.push(format!("{character}: no locomotion clip matched the mesh"));
            continue;
        }
        let clips: Vec<CharacterClip<'_>> = anims
            .iter()
            .map(|(name, anim)| CharacterClip {
                name: name.clone(),
                fps: CLIP_FPS,
                anim,
            })
            .collect();
        let stem = character.to_ascii_lowercase();
        let Some(glb) = build_character_glb_hybrid(
            &format!("{stem}_field"),
            &mesh,
            &object_ids,
            &vram,
            &clips,
            Some(&shading),
        ) else {
            notes.push(format!("{character}: the glb baker produced nothing"));
            continue;
        };
        members.push(PartyGlb {
            slot,
            character,
            file_stem: stem,
            glb: scale_glb_scene_roots(glb, opts.scale),
            clips: anims.into_iter().map(|(n, _)| n).collect(),
            bone_count: nobj,
        });
    }
    Ok(PartyExport { members, notes })
}

/// The `party/manifest.json` document: the same `npcs[]` shape a scene
/// manifest carries (file, kind, label, `model_index`, clips), so a consumer
/// that resolves villagers from other scenes resolves these the same way -
/// `model_index` is the pack slot. Every entry also names its walk cycle
/// (`walk_clip`), which scene rigs express as a rig-family constant instead.
pub fn party_manifest(e: &PartyExport, opts: &GlbExportOptions) -> Value {
    json!({
        "generator": "legaia-engine export-glb --party",
        "scene": "party",
        "source": "PROT 0874 (player.lzs): section 0 field meshes, section 1 locomotion bank, section 2 field texture pages",
        "scale": opts.scale,
        "conventions": {
            "axes": "glTF +Y up; geometry keeps the site viewers' mirror-handedness (same as the scene NPC glbs)",
            "npc_prop_units": "scaled",
            "units": "glTF meters = PSX world units * scale, baked on each file's root node",
            "clips": "clip 0 is the standing idle (frame 0 = rest pose), clip 1 the walk cycle; the rest of the 7-clip bank follows as `Locomotion N`",
            "model_index": "the party pack slot: 0 Vahn, 1 Noa, 2 Gala",
        },
        "spawn": { "position": [0.0, 0.0, 0.0] },
        "npcs": e.members.iter().map(|m| json!({
            "file": format!("npcs/{}.glb", m.file_stem),
            "kind": "talk",
            "label": m.character,
            "name": m.character,
            "model_index": m.slot,
            "anim_id": 0,
            "clips": m.clips,
            "walk_clip": m.clips.get(1).cloned().unwrap_or_default(),
            "bones": m.bone_count,
            "conditional": false,
            "position": [0.0, 0.0, 0.0],
            "target_map": null,
        })).collect::<Vec<_>>(),
        "notes": e.notes,
    })
}
