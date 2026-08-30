//! Equipment loadout view: the battle model a chosen set of equipped items
//! assembles to, plus the diff highlight against the unequipped default.
//!
//! Retail never loads a party character's battle mesh whole. It walks the
//! player battle file's five-section descriptor table (`data\battle\PLAYER1..4`
//! = extraction PROT `863 + slot`), matches each section's entries against the
//! character record's `+0x196..+0x19A` equip bytes, and splices the selected
//! sections' TMD objects into one merged blob - so the model you fight with is
//! a function of what you are wearing. Every other surface in this project
//! passes the all-defaults loadout; this one lets the visitor choose it.
//!
//! The chain is the same one `site/arts.html` drives, with `equipped` no
//! longer pinned to zero:
//!
//! * **mesh** - [`bca::assemble_character`] over the chosen ids, then
//!   [`bca::relocate_tsb_cba`] into runtime VRAM band 0;
//! * **texture** - [`bca::character_texture_uploads`] for the *same* ids. This
//!   composites the **whole band**: both `record[0]` blocks plus every flagged
//!   section pool, in retail order. That ordering is load-bearing - two of the
//!   blocks ship `clut_n == 0` (pixels only) and sample a palette a sibling
//!   block put on the shared row, so uploading only the selected section would
//!   leave them unpainted.
//! * **pose** - `record[0]`'s own action bank ([`bca::battle_animations`])
//!   plus the four equipment-spliced weapon swings
//!   ([`bca::swing_battle_animations`], which change with the weapon), each
//!   pre-expanded per assembled object so channel `i` drives object `i`.
//!
//! Two shapes of the assembled blob the viewer has to respect:
//!
//! * a `200+`-tagged object is **usually** a byte-copy of the bone it attaches
//!   to, and drawing the copy alongside its host z-fights one limb against
//!   itself (retail reaches the copy only through the actor's `+0xA4` window).
//!   Usually, not always: sixteen of the disc's single-section assemblies
//!   carry a `200+` surplus that differs from its host, and in six of those
//!   the surplus is the bare hand while a `0xFE` extra holds the weapon. So
//!   the drop keys on `AssembledCharacter::duplicate_objects` - a byte
//!   comparison - rather than on the tag.
//! * objects tagged `100+` are ordinary extra geometry and do draw, on the
//!   preceding bone's channel (`anm_bones`).
//!
//! The page also offers a **per-item** export for every equipped section.
//! That is not a contradiction of "equipment is bone geometry": a held item
//! is not a separate object, but it *is* an exact primitive subset selected
//! by palette column, which [`equip_item`] cuts and labels by how cleanly it
//! came away; a piece with no such boundary (armour) exports **fused** with
//! the limb it was sculpted onto, and says so. Completeness over purity.
//!
//! Everything decodes from the visitor's own disc in the browser; no Sony
//! bytes ship with the site.

use super::*;

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::battle_char_assembly::equip_diff;
use legaia_asset::battle_char_assembly::loadout;
use legaia_asset::mesh_raster;
use legaia_asset::monster_archive::MonsterAnimation;

/// The shared loadout kernel (assembly, item cuts, per-item glb bakers) -
/// hoisted to `legaia_asset::battle_char_assembly::loadout` so the native
/// `export-glb --items` and this page run one implementation. What stays
/// here is presentation: diff tints, JSON summaries, canvas accessors.
pub(crate) use legaia_asset::battle_char_assembly::loadout::{
    CHARACTER_LABELS, EquippedCharacter, ItemAloneMesh, PLAYER_FILE_BASE, item_glb, item_only_glb,
    rest_poses, section_catalog, section_labels,
};

/// Modulation triples for the diff highlight. The browser shader draws a
/// textured prim as `texel * rgb / 128`, so `0x80` is neutral, below dims and
/// above brightens - the highlight needs no renderer change.
const TINT_SHARED: [u8; 3] = [0x30, 0x34, 0x3C];
const TINT_ADDED: [u8; 3] = [0xFF, 0xC0, 0x48];
const TINT_BARE_ONLY: [u8; 3] = [0x30, 0x78, 0xE0];

/// A cached single-item build for the equipment panel's card grid: the
/// character wearing exactly one item, so the card's thumbnail, metadata and
/// downloads come off one assembly. See [`LegaiaViewer::equipment_item_card_json`].
pub(crate) struct ItemCard {
    cslot: usize,
    section: usize,
    id: u32,
    character: EquippedCharacter,
}

/// Zero-copy view of one PROT entry's on-disc bytes.
fn entry_bytes<'a>(prot: &'a [u8], entries: &[disc::EntryMeta], index: u32) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
}

/// Resolve the character's player battle file out of the disc and hand off
/// to the shared loadout kernel, logging any tolerated decode degradation
/// (a texture pool or clip bank that didn't parse) to the console.
fn build(
    prot: &[u8],
    entries: &[disc::EntryMeta],
    cslot: usize,
    equipped: [u8; bca::SECTION_COUNT],
    diff: bool,
    arts: &[loadout::ArtClip],
) -> Result<EquippedCharacter, String> {
    let prot_index = PLAYER_FILE_BASE + cslot as u32;
    let raw = entry_bytes(prot, entries, prot_index)
        .ok_or_else(|| format!("player file (PROT {prot_index}) not present"))?;
    let c = loadout::build(raw, cslot, equipped, diff, arts)?;
    for n in &c.notes {
        log_equip(&format!("equipment: {n}"));
    }
    Ok(c)
}

/// Log a decode degradation. Browser console on wasm; stderr natively (the
/// disc-gated tests drive this module natively).
fn log_equip(s: &str) {
    #[cfg(target_arch = "wasm32")]
    console_log(s);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}

/// Flatten a clip to the site animators' pose layout: six `i32` per part per
/// frame, `[tx, ty, tz, rx, ry, rz]`.
fn flatten_pose_frames(anim: &MonsterAnimation) -> Vec<i32> {
    let mut out = Vec::with_capacity(anim.frame_count * anim.part_count * 6);
    for frame in &anim.frames {
        for p in frame {
            out.extend_from_slice(&[
                i32::from(p.tx),
                i32::from(p.ty),
                i32::from(p.tz),
                i32::from(p.rx),
                i32::from(p.ry),
                i32::from(p.rz),
            ]);
        }
    }
    out
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// The four player battle files and every equipment id each section
    /// offers, as JSON:
    ///
    /// ```json
    /// { "slots": [ { "slot": 0, "label": "Vahn", "prot": 863, "records": 54,
    ///     "sections": [ { "index": 0, "label": "Section 0 (Body)",
    ///        "items": [ { "id": 75, "name": "Ra-Seru Armor" }, ... ] }, ... ] } ] }
    /// ```
    ///
    /// The `id = 0` default is not listed as an item - it is the section's
    /// unequipped variant, which the UI offers as its own choice. Section
    /// order is **not** the same for every character, so each section carries
    /// its own label derived from the equipment table's slot byte.
    pub fn equipment_pack_json(&self) -> String {
        let Some(entries) = parse_prot_toc(&self.disc) else {
            return r#"{"slots":[]}"#.to_string();
        };
        let stats = self.equip_stats.as_ref();
        let mut slots = Vec::new();
        for (cslot, label) in CHARACTER_LABELS.iter().enumerate() {
            let prot_index = PLAYER_FILE_BASE + cslot as u32;
            let Some(raw) = entry_bytes(&self.disc, &entries, prot_index) else {
                continue;
            };
            let Ok(pack) = legaia_asset::battle_data_pack::parse(raw) else {
                continue;
            };
            let cat = section_catalog(&pack);
            let labels = section_labels(&cat, stats, cslot);
            let sections: Vec<serde_json::Value> = cat
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let items: Vec<serde_json::Value> = c
                        .ids
                        .iter()
                        .map(|&id| {
                            serde_json::json!({
                                "id": id,
                                "name": u8::try_from(id).ok()
                                    .and_then(|i| self.item_names.as_ref()?.name(i))
                                    .map(str::to_string),
                            })
                        })
                        .collect();
                    serde_json::json!({
                        "index": i,
                        "label": labels[i].clone(),
                        "items": items,
                    })
                })
                .collect();
            slots.push(serde_json::json!({
                "slot": cslot,
                "label": label,
                "prot": prot_index,
                "records": pack.records.len(),
                "sections": sections,
            }));
        }
        serde_json::json!({ "slots": slots }).to_string()
    }

    /// Assemble character `slot` (0=Vahn .. 3=Terra) wearing `ids` (five
    /// equipment ids in the player file's section order; `0` = that section's
    /// unequipped default) and cache it for the mesh accessors.
    ///
    /// `diff` turns on the **diff highlight**: an approximate envelope test
    /// that tints what the loadout adds beyond the unequipped part, and draws
    /// the bare geometry it replaced alongside. See
    /// [`legaia_asset::battle_char_assembly::equip_diff`] for what that
    /// boundary is and is not.
    ///
    /// Returns a JSON summary; `{"ok":false,"why":...}` when the character
    /// doesn't assemble.
    pub fn set_equipped_character(&mut self, slot: u32, ids: &[u8], diff: bool) -> String {
        let cslot = slot as usize;
        if cslot >= CHARACTER_LABELS.len() {
            self.equipped = None;
            return r#"{"ok":false,"why":"character slot out of range"}"#.to_string();
        }
        let mut equipped = [0u8; bca::SECTION_COUNT];
        for (i, b) in ids.iter().take(bca::SECTION_COUNT).enumerate() {
            equipped[i] = *b;
        }
        let Some(entries) = parse_prot_toc(&self.disc) else {
            self.equipped = None;
            return r#"{"ok":false,"why":"PROT.DAT TOC parse failed"}"#.to_string();
        };
        // The art clips are per character, not per loadout: decode them once
        // and re-expand per assembly. A bank that does not decode on this
        // disc leaves the list empty (logged) rather than failing the model.
        if !self.art_clips.contains_key(&cslot) {
            let arts = match crate::art_clip_bank::decode_art_clips(&self.disc, &entries, cslot) {
                Ok(a) => a,
                Err(why) => {
                    log_equip(&format!("equipment: char {cslot} art clips: {why}"));
                    Vec::new()
                }
            };
            self.art_clips.insert(cslot, arts);
        }
        let arts = self.art_clips.get(&cslot).cloned().unwrap_or_default();
        match build(&self.disc, &entries, cslot, equipped, diff, &arts) {
            Ok(c) => {
                let json = self.equipped_summary(&c);
                self.equipped = Some(c);
                json
            }
            Err(why) => {
                self.equipped = None;
                serde_json::json!({ "ok": false, "why": why }).to_string()
            }
        }
    }

    /// JSON summary of a freshly-built loadout (see
    /// [`Self::set_equipped_character`]).
    fn equipped_summary(&self, c: &EquippedCharacter) -> String {
        let name_of = |id: u32| -> Option<String> {
            u8::try_from(id)
                .ok()
                .and_then(|i| self.item_names.as_ref()?.name(i))
                .map(str::to_string)
        };
        let sections: Vec<serde_json::Value> = (0..bca::SECTION_COUNT)
            .map(|i| {
                serde_json::json!({
                    "index": i,
                    "requested": c.equipped[i],
                    "resolved": c.resolved[i],
                    "name": name_of(c.resolved[i]),
                    "default": c.resolved[i] == 0,
                })
            })
            .collect();
        let diffs: Vec<serde_json::Value> = c
            .diffs
            .iter()
            .filter(|d| d.changed())
            .map(|d| {
                serde_json::json!({
                    "bone": d.bone_tag,
                    "bare_vertices": d.bare_vertices,
                    "equipped_vertices": d.equipped_vertices,
                    "bare_primitives": d.bare_primitives,
                    "equipped_primitives": d.equipped_primitives,
                    "added_primitives": d.added_primitives,
                    "straddling_primitives": d.straddling_primitives,
                    "shared_vertex_positions": d.shared_vertex_positions,
                })
            })
            .collect();
        let clips: Vec<serde_json::Value> = c
            .clips
            .iter()
            .map(|clip| {
                let mut v = serde_json::json!({
                    "label": clip.label,
                    "kind": clip.kind.tag(),
                    "frames": clip.anim.frame_count,
                    "rate": clip.anim.rate,
                });
                if let Some(a) = &clip.art {
                    v["art"] = serde_json::json!({
                        "kind": a.kind,
                        "ap": a.ap,
                        "directions": a.directions,
                        "anim_id": a.anim_id,
                        "segments": a.segments,
                    });
                }
                v
            })
            .collect();
        let items: Vec<serde_json::Value> = c
            .items
            .iter()
            .map(|it| {
                serde_json::json!({
                    "section": it.section,
                    "id": it.id,
                    "name": name_of(it.id),
                    "class": it.partition.class.tag(),
                    // The one-line label the file and the UI both carry.
                    "describe": it.partition.class.describe(),
                    // `false` = the grip is open: the shaft inside the closed
                    // fist was never modelled, and no cut recovers it.
                    "complete": it.partition.class.is_complete(),
                    // `false` = the host limb rides along (fused).
                    "pure": it.partition.class.is_pure(),
                    "item_primitives": it.partition.item_primitives,
                    "item_vertices": it.partition.item_vertices,
                    "limb_primitives": it.partition.limb_primitives,
                    "seam_vertices": it.partition.seam_vertices,
                    // The item-alone cut: how it was decided, what it kept,
                    // and whether a committed rule touched this record.
                    "isolation": {
                        "mode": it.isolation.mode.tag(),
                        "kept_primitives": it.isolation.kept_primitives,
                        "dropped_primitives": it.isolation.dropped_primitives,
                        "curated": it.isolation.curated,
                        "note": it.isolation.note,
                        // The grip repair: how many shaft gaps were bridged
                        // and how many triangles that inferred (0 = the
                        // cut needed none, or had no two rims facing each
                        // other to bridge).
                        "bridges": it.alone.bridges.len(),
                        "bridged_triangles": it.alone.bridged_triangles(),
                    },
                })
            })
            .collect();
        serde_json::json!({
            "ok": true,
            "character": CHARACTER_LABELS[c.cslot],
            "slot": c.cslot,
            "part_count": c.part_count,
            "vertices": c.mesh.positions.len(),
            "triangles": c.mesh.indices.len() / 3,
            "diff": c.diff,
            "sections": sections,
            // Per-bone-object geometry change vs the unequipped assembly.
            // Empty for an all-defaults loadout.
            "changed_objects": diffs,
            // One entry per equipped section, each with how cleanly it
            // separates (`class` / `describe`). Armour comes back `fused`,
            // never missing.
            "items": items,
            "clips": clips,
        })
        .to_string()
    }

    /// Per-vertex positions of the cached loadout's mesh (flat `f32`, 3 per
    /// vertex). Empty until [`Self::set_equipped_character`].
    pub fn equipped_mesh_positions(&self) -> Vec<f32> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        c.mesh.positions.iter().flat_map(|p| *p).collect()
    }

    /// Per-vertex `[u, v]` integer texel coords, parallel to the positions.
    pub fn equipped_mesh_uvs(&self) -> Vec<i32> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        c.mesh
            .uvs
            .iter()
            .flat_map(|uv| [i32::from(uv[0]), i32::from(uv[1])])
            .collect()
    }

    /// Per-vertex `[cba, tsb]`, parallel to the positions.
    pub fn equipped_mesh_cba_tsb(&self) -> Vec<u32> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        c.mesh
            .cba_tsb
            .iter()
            .flat_map(|ct| [u32::from(ct[0]), u32::from(ct[1])])
            .collect()
    }

    /// Triangle indices (`u32`, multiple of 3).
    pub fn equipped_mesh_indices(&self) -> Vec<u32> {
        self.equipped
            .as_ref()
            .map(|c| c.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex assembled-object index (the bone a vertex poses on),
    /// parallel to the positions.
    pub fn equipped_mesh_object_ids(&self) -> Vec<u32> {
        self.equipped
            .as_ref()
            .map(|c| c.object_ids.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, 255]` for the browser shader's `a_flat_rgba`.
    ///
    /// With the diff highlight off this is the mesh's own packet colour (the
    /// modulation half of retail's `texel * colour / 128`). With it on, the
    /// colour becomes the class tint: dim for geometry shared with the bare
    /// part, warm for what the loadout adds beyond it, cool for the bare
    /// geometry the loadout replaced.
    pub fn equipped_mesh_flat_rgba(&self) -> Vec<u8> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        if !c.diff {
            return crate::packet_color::textured(&c.mesh);
        }
        let mut out = Vec::with_capacity(c.mesh.positions.len() * 4);
        for v in 0..c.mesh.positions.len() {
            let t = match c
                .diff_class
                .get(v)
                .copied()
                .unwrap_or(equip_diff::CLASS_SHARED)
            {
                equip_diff::CLASS_ADDED => TINT_ADDED,
                equip_diff::CLASS_BARE_ONLY => TINT_BARE_ONLY,
                _ => TINT_SHARED,
            };
            out.extend_from_slice(&[t[0], t[1], t[2], 255]);
        }
        out
    }

    /// Per-vertex diff class (`0` shared, `1` added, `2` bare-only), parallel
    /// to the positions. Exposed so a headless check can assert the highlight
    /// actually classified something.
    pub fn equipped_mesh_diff_class(&self) -> Vec<u8> {
        self.equipped
            .as_ref()
            .map(|c| c.diff_class.clone())
            .unwrap_or_default()
    }

    /// Bounding sphere `[cx, cy, cz, r]` (vertex centroid + max distance).
    pub fn equipped_mesh_bounds(&self) -> Vec<f32> {
        let Some(c) = &self.equipped else {
            return vec![0.0; 4];
        };
        if c.mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        centroid_bounds(&c.mesh.positions)
    }

    /// The 1 MB PSX VRAM for the cached loadout: the whole band this
    /// character uploads (both `record[0]` blocks + every flagged section
    /// pool) at the retail placement.
    pub fn equipped_vram_bytes(&self) -> Vec<u8> {
        self.equipped
            .as_ref()
            .map(|c| c.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Clip `index`'s pose frames (the position in the summary's `clips`
    /// array): six `i32` per part per frame, `[tx, ty, tz, rx, ry, rz]`.
    pub fn equipped_pose_frames(&self, index: u32) -> Vec<i32> {
        self.equipped
            .as_ref()
            .and_then(|c| c.clips.get(index as usize))
            .map(|clip| flatten_pose_frames(&clip.anim))
            .unwrap_or_default()
    }

    /// Bake the cached loadout into an animated binary glTF: one node per
    /// assembled TMD object, textured from the same band VRAM the canvas
    /// renders, carrying the character's whole battle action bank plus the
    /// equipment-spliced weapon swings as named clips.
    ///
    /// The export is always the **whole posed character** - equipment
    /// geometry is skeleton-bone geometry, so there is no item to export on
    /// its own, and the diff highlight is a viewing aid, not a separation.
    /// Empty until [`Self::set_equipped_character`].
    pub fn equipped_character_glb(&self) -> Vec<u8> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        let fps_for_rate = |rate: u8| {
            if rate > 0 {
                7.5 * f32::from(rate)
            } else {
                15.0
            }
        };
        let clips: Vec<legaia_asset::character_gltf::CharacterClip<'_>> = c
            .clips
            .iter()
            .map(|clip| legaia_asset::character_gltf::CharacterClip {
                name: clip.label.clone(),
                fps: fps_for_rate(clip.anim.rate),
                anim: &clip.anim,
            })
            .collect();
        legaia_asset::character_gltf::build_character_glb(
            &self.equipped_glb_name(),
            &c.mesh,
            &c.object_ids,
            &c.vram,
            &clips,
        )
        .unwrap_or_default()
    }

    /// Bake the **separated item** of equipped section `section` into a
    /// binary glTF, alongside the host limb it was cut from.
    ///
    /// Retail models a weapon as primitives of the hand object, selected by
    /// palette column rather than attached as a prop - see
    /// [`equip_item`] for why that is still an exact cut, and why the two
    /// naive readings (connectivity, "geometry the bare hand lacks") are not.
    /// The file carries the item and the limb the cut left behind as
    /// separate named nodes - one item node per source object, each posed by
    /// the object it came from, plus the character's clip bank so the piece
    /// moves with the limb it rides. The limb is ground truth and is always
    /// present, so a reader can see exactly what was and was not taken.
    ///
    /// For a [`equip_item::ItemClass::WeldedSubset`] record the item's grip
    /// is **open** - the shaft inside the closed fist was never modelled, and
    /// no cutting strategy recovers it. The summary's `complete` flag says
    /// so, and callers must pass that on.
    ///
    /// Every equipped section exports. A section with no material boundary
    /// (armour, and the one single-palette weapon) comes back
    /// [`equip_item::ItemClass::Fused`] - its whole contribution, item and
    /// host geometry together, labelled as such in the root name. Empty only
    /// when the section is at its unequipped default or nothing is cached.
    pub fn equipped_item_glb(&self, section: u32) -> Vec<u8> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        item_glb(c, section as usize, self.item_names.as_ref())
    }

    /// Per-vertex item-alone mask for the cached loadout's mesh (parallel to
    /// [`Self::equipped_mesh_positions`]): `0` = not part of this section's
    /// contribution, `1` = the section's geometry the item-alone cut leaves
    /// behind (host limb, skin, unchanged default), `2` = the item alone.
    /// Lets the page preview exactly what [`Self::equipped_item_only_glb`]
    /// will export. Empty when nothing is cached or the section is at its
    /// default.
    pub fn equipped_mesh_item_mask(&self, section: u32) -> Vec<u8> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        let Some(it) = c.items.iter().find(|i| i.section == section as usize) else {
            return Vec::new();
        };
        c.object_ids
            .iter()
            .zip(c.prim_ids.iter())
            .map(|(&obj, &prim)| {
                if prim == u32::MAX || !it.isolation.objects.contains(&(obj as usize)) {
                    0
                } else if it.isolation.claims(obj as usize, prim) {
                    2
                } else {
                    1
                }
            })
            .collect()
    }

    /// The **item alone** of one equipped section as a binary glTF: no host
    /// limb, no skin, no unchanged default geometry - the opinionated cut of
    /// [`equip_isolate`], under the section's default reading or the
    /// record's committed rule. The root name says how it was decided
    /// (`colour-diff` / `identity` / `whole` / `palette`) and whether a rule
    /// touched it (`curated`), so a downloader can tell a heuristic result
    /// from a checked one. One node per source object, each posed by the
    /// object it came from, plus the character's clip bank so the piece
    /// still swings. Empty when the section is at its default, nothing is
    /// cached, or the cut kept nothing.
    ///
    /// This is the second download next to [`Self::equipped_item_glb`], not
    /// a replacement: that one is the record-keeping export (the exact
    /// palette cut with its ground-truth limb), this one is what most people
    /// asking for "just the great axe" want.
    pub fn equipped_item_only_glb(&self, section: u32) -> Vec<u8> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        item_only_glb(c, section as usize, self.item_names.as_ref())
    }

    /// The item-alone preview mesh of equipped section `section` - the same
    /// geometry [`Self::equipped_item_only_glb`] exports, grip repair
    /// included, object-local and posed by the loadout's clips through the
    /// same per-object channels as the whole model. The seven accessors are
    /// parallel to the `equipped_mesh_*` family so the page can swap one for
    /// the other. Empty when the section is at its default or nothing is
    /// cached.
    pub fn equipped_item_only_positions(&self, section: u32) -> Vec<f32> {
        self.item_alone(section)
            .map(|a| a.mesh.positions.iter().flat_map(|p| *p).collect())
            .unwrap_or_default()
    }

    /// See [`Self::equipped_item_only_positions`].
    pub fn equipped_item_only_uvs(&self, section: u32) -> Vec<i32> {
        self.item_alone(section)
            .map(|a| {
                a.mesh
                    .uvs
                    .iter()
                    .flat_map(|uv| [i32::from(uv[0]), i32::from(uv[1])])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// See [`Self::equipped_item_only_positions`].
    pub fn equipped_item_only_cba_tsb(&self, section: u32) -> Vec<u32> {
        self.item_alone(section)
            .map(|a| {
                a.mesh
                    .cba_tsb
                    .iter()
                    .flat_map(|ct| [u32::from(ct[0]), u32::from(ct[1])])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// See [`Self::equipped_item_only_positions`].
    pub fn equipped_item_only_indices(&self, section: u32) -> Vec<u32> {
        self.item_alone(section)
            .map(|a| a.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// See [`Self::equipped_item_only_positions`].
    pub fn equipped_item_only_object_ids(&self, section: u32) -> Vec<u32> {
        self.item_alone(section)
            .map(|a| a.object_ids.clone())
            .unwrap_or_default()
    }

    /// See [`Self::equipped_item_only_positions`].
    pub fn equipped_item_only_flat_rgba(&self, section: u32) -> Vec<u8> {
        self.item_alone(section)
            .map(|a| crate::packet_color::textured(&a.mesh))
            .unwrap_or_default()
    }

    /// Bounding sphere `[cx, cy, cz, r]` of the item-alone mesh (object-local
    /// centroid + max distance; the page refits on the posed clip anyway).
    pub fn equipped_item_only_bounds(&self, section: u32) -> Vec<f32> {
        self.item_alone(section)
            .filter(|a| !a.mesh.positions.is_empty())
            .map(|a| centroid_bounds(&a.mesh.positions))
            .unwrap_or_else(|| vec![0.0; 4])
    }

    fn item_alone(&self, section: u32) -> Option<&ItemAloneMesh> {
        let c = self.equipped.as_ref()?;
        c.items
            .iter()
            .find(|i| i.section == section as usize)
            .map(|i| &i.alone)
    }

    /// Build (or reuse) the **item card** for `(slot, section, id)` - the
    /// character wearing exactly that one item - and return its metadata as
    /// JSON: name, how the palette cut classed it, how the item-alone cut
    /// decided (`mode` / `curated` / `note`), what it kept, and what the grip
    /// repair added. `{"ok":false,"why":...}` when it does not assemble.
    /// The build is cached, so [`Self::equipment_item_card_pixels`] and
    /// [`Self::equipment_item_card_glb`] for the same triple cost nothing
    /// extra.
    pub fn equipment_item_card_json(&mut self, slot: u32, section: u32, id: u32) -> String {
        let cslot = slot as usize;
        let section = section as usize;
        if cslot >= CHARACTER_LABELS.len() || section >= bca::SECTION_COUNT || id == 0 || id > 255 {
            return r#"{"ok":false,"why":"card out of range"}"#.to_string();
        }
        let hit = self
            .item_card
            .as_ref()
            .is_some_and(|c| c.cslot == cslot && c.section == section && c.id == id);
        if !hit {
            let Some(entries) = parse_prot_toc(&self.disc) else {
                return r#"{"ok":false,"why":"PROT.DAT TOC parse failed"}"#.to_string();
            };
            let mut equipped = [0u8; bca::SECTION_COUNT];
            equipped[section] = id as u8;
            match build(&self.disc, &entries, cslot, equipped, false, &[]) {
                Ok(character) => {
                    self.item_card = Some(ItemCard {
                        cslot,
                        section,
                        id,
                        character,
                    });
                }
                Err(why) => {
                    self.item_card = None;
                    return serde_json::json!({ "ok": false, "why": why }).to_string();
                }
            }
        }
        let card = self.item_card.as_ref().expect("just built");
        let Some(it) = card.character.items.iter().find(|i| i.section == section) else {
            return r#"{"ok":false,"why":"the section contributed no geometry"}"#.to_string();
        };
        let name = u8::try_from(id)
            .ok()
            .and_then(|i| self.item_names.as_ref()?.name(i))
            .map(str::to_string);
        serde_json::json!({
            "ok": true,
            "character": CHARACTER_LABELS[cslot],
            "slot": cslot,
            "section": section,
            "id": id,
            "name": name,
            "class": it.partition.class.tag(),
            "describe": it.partition.class.describe(),
            "item_primitives": it.partition.item_primitives,
            "item_vertices": it.partition.item_vertices,
            "isolation": {
                "mode": it.isolation.mode.tag(),
                "kept_primitives": it.isolation.kept_primitives,
                "dropped_primitives": it.isolation.dropped_primitives,
                "curated": it.isolation.curated,
                "note": it.isolation.note,
                "bridges": it.alone.bridges.len(),
                "bridged_triangles": it.alone.bridged_triangles(),
            },
            "alone_triangles": it.alone.mesh.indices.len() / 3,
        })
        .to_string()
    }

    /// The cached card's item-alone thumbnail: `size * size` RGBA8, drawn by
    /// the software rasteriser at the character's rest stance, re-framed on
    /// the item's own principal axes (blade up, flat-on) with a slight
    /// three-quarter tilt, over a transparent background. Empty until
    /// [`Self::equipment_item_card_json`] succeeded.
    pub fn equipment_item_card_pixels(&self, size: u32) -> Vec<u8> {
        let Some(card) = &self.item_card else {
            return Vec::new();
        };
        let Some(it) = card
            .character
            .items
            .iter()
            .find(|i| i.section == card.section)
        else {
            return Vec::new();
        };
        let poses = rest_poses(&card.character);
        let opts = mesh_raster::RasterOptions {
            width: size as usize,
            height: size as usize,
            yaw: 28f32.to_radians(),
            pitch: -14f32.to_radians(),
            margin: 0.08,
            background: [0, 0, 0, 0],
            shade: 0.4,
            auto_orient: true,
        };
        mesh_raster::render_posed(
            &it.alone.mesh,
            &it.alone.object_ids,
            &poses,
            &card.character.vram,
            &opts,
        )
    }

    /// The cached card's download: the item alone (`alone = true`, grip
    /// repaired) or the record-keeping palette cut with its host limb
    /// (`alone = false`), as binary glTF with the character's clip bank.
    /// Empty until [`Self::equipment_item_card_json`] succeeded.
    pub fn equipment_item_card_glb(&self, alone: bool) -> Vec<u8> {
        let Some(card) = &self.item_card else {
            return Vec::new();
        };
        if alone {
            item_only_glb(&card.character, card.section, self.item_names.as_ref())
        } else {
            item_glb(&card.character, card.section, self.item_names.as_ref())
        }
    }

    /// The honest name for the cached loadout's `.glb` root node: the
    /// character's whole battle model, listing what it is wearing - never the
    /// item on its own, which does not exist as a mesh.
    pub fn equipped_glb_name(&self) -> String {
        let Some(c) = &self.equipped else {
            return "legaia-character".to_string();
        };
        let worn: Vec<String> = c
            .resolved
            .iter()
            .filter(|&&id| id != 0)
            .filter_map(|&id| {
                u8::try_from(id)
                    .ok()
                    .and_then(|i| self.item_names.as_ref()?.name(i))
                    .map(str::to_string)
            })
            .collect();
        let who = CHARACTER_LABELS[c.cslot];
        if worn.is_empty() {
            format!("{who} - battle model, no equipment")
        } else {
            format!("{who} - battle model wearing {}", worn.join(", "))
        }
    }
}
