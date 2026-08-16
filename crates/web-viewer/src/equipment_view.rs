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
//! The page also offers a **per-item** export for the two weapon-bearing
//! sections. That is not a contradiction of "equipment is bone geometry":
//! the item is not a separate object, but it *is* an exact primitive subset
//! selected by palette column, which [`equip_item`] cuts and labels by how
//! cleanly it came away.
//!
//! Everything decodes from the visitor's own disc in the browser; no Sony
//! bytes ship with the site.

use super::*;

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::battle_char_assembly::{equip_diff, equip_item};
use legaia_asset::battle_data_pack::BattleDataPack;
use legaia_asset::monster_archive::MonsterAnimation;

/// Player battle files start at extraction PROT entry 863 (Vahn).
const PLAYER_FILE_BASE: u32 = 863;
/// Character labels in player-file order.
const CHARACTER_LABELS: [&str; 4] = ["Vahn", "Noa", "Gala", "Terra"];
/// Party VRAM band the viewer renders in (Vahn's: texpages `(512,256)` /
/// `(576,256)`, CLUT row 481).
const VIEW_BAND: u8 = 0;

/// Modulation triples for the diff highlight. The browser shader draws a
/// textured prim as `texel * rgb / 128`, so `0x80` is neutral, below dims and
/// above brightens - the highlight needs no renderer change.
const TINT_SHARED: [u8; 3] = [0x30, 0x34, 0x3C];
const TINT_ADDED: [u8; 3] = [0xFF, 0xC0, 0x48];
const TINT_BARE_ONLY: [u8; 3] = [0x30, 0x78, 0xE0];

/// One assembled loadout, cached on the host so the per-buffer accessors
/// don't re-run the assembly.
pub(crate) struct EquippedCharacter {
    cslot: usize,
    equipped: [u8; bca::SECTION_COUNT],
    /// Pose rig width = the assembled object count (channel `i` -> object `i`).
    part_count: usize,
    /// Drawable mesh: the equipped assembly minus its `200+` duplicates, plus
    /// (in diff mode) the bare geometry of every object the loadout replaced.
    mesh: legaia_tmd::mesh::VramMesh,
    /// Per-vertex assembled-TMD object index, parallel to `mesh.positions`.
    /// Bare-only vertices carry the index of the equipped object they replace,
    /// so they pose on the same bone.
    object_ids: Vec<u32>,
    /// Per-vertex [`equip_diff`] class, parallel to `mesh.positions`.
    diff_class: Vec<u8>,
    /// Whether the cached mesh was built with the diff highlight on.
    diff: bool,
    vram: legaia_tim::Vram,
    /// The character's battle action bank + the equipment-spliced swings,
    /// labeled and expanded per assembled object.
    clips: Vec<(String, MonsterAnimation)>,
    /// Per-bone-object diff against the all-defaults assembly.
    diffs: Vec<equip_diff::ObjectDiff>,
    /// Resolved section ids (what `select_sections` actually picked).
    resolved: [u32; bca::SECTION_COUNT],
    /// The relocated assembled TMD the mesh was built from, kept so the item
    /// cut can re-walk its primitives for the per-item export.
    tmd_bytes: Vec<u8>,
    /// Post-sort bone tag per object, and the animation channel each object
    /// rides. The item export uses them to find the limb a standalone
    /// (`own-object`) item hangs off, which its partition does not name.
    bone_tags: Vec<u8>,
    anm_bones: Vec<u8>,
    /// Per-equipped-**item** section (2 / 3 only), where the held item lives
    /// inside the assembly. See [`equip_item`].
    items: Vec<EquippedItem>,
}

/// One separated item of the current loadout.
struct EquippedItem {
    section: usize,
    id: u32,
    partition: equip_item::ItemPartition,
}

/// One equipment section of a player file: the ids it offers, in table order.
struct SectionCatalog {
    /// Real equipment ids (the `id = 0` default is not one of them).
    ids: Vec<u32>,
}

/// Walk a player file's descriptor table into its five sections. Entries with
/// `id == 0` are the section default *and* the section separator.
fn section_catalog(pack: &BattleDataPack) -> [SectionCatalog; bca::SECTION_COUNT] {
    let mut out: [SectionCatalog; bca::SECTION_COUNT] =
        std::array::from_fn(|_| SectionCatalog { ids: Vec::new() });
    let mut slot = 0usize;
    for r in &pack.records {
        if slot >= bca::SECTION_COUNT {
            break;
        }
        if r.id == 0 {
            slot += 1;
        } else {
            out[slot].ids.push(r.id);
        }
    }
    out
}

/// Human labels for one player file's five sections, derived from the SCUS
/// equipment stat table rather than from a fixed order - the sections are
/// **not** in the same order for every character (Vahn's section 2 holds his
/// weapons and section 3 his Ra-Seru; Noa's are the other way round).
///
/// Two of the five sections take the table's `Weapon` slot, because a
/// character's Ra-Seru *is* their weapon arm mechanically. The one that is
/// the Ra-Seru is the one whose every id is equippable by this character
/// alone (`equip_mask` = just this character's bit): the weapon section
/// always mixes in ids other party members can also carry. `is_ra_seru()` is
/// no help - it flags the story-upgrade tier, and it is set on the top
/// armour, seal and boots too.
fn section_labels(
    cats: &[SectionCatalog; bca::SECTION_COUNT],
    stats: Option<&legaia_asset::equip_stats::EquipStatTable>,
    cslot: usize,
) -> [String; bca::SECTION_COUNT] {
    use legaia_asset::equip_stats::EquipSlot;
    let mut slot_of: [Option<EquipSlot>; bca::SECTION_COUNT] = [None; bca::SECTION_COUNT];
    let mut exclusive = [false; bca::SECTION_COUNT];
    if let Some(stats) = stats {
        for (i, cat) in cats.iter().enumerate() {
            let mut tally = [0usize; 4];
            let mut mine = 0usize;
            let mut rated = 0usize;
            for &id in &cat.ids {
                let Ok(id) = u8::try_from(id) else { continue };
                let Some(b) = stats.bonus(id) else { continue };
                rated += 1;
                tally[match b.slot() {
                    EquipSlot::Body => 0,
                    EquipSlot::Head => 1,
                    EquipSlot::Weapon => 2,
                    EquipSlot::Footwear => 3,
                }] += 1;
                if b.equip_mask() == 1u8 << cslot {
                    mine += 1;
                }
            }
            if rated == 0 {
                continue;
            }
            exclusive[i] = mine == rated;
            let best = tally
                .iter()
                .enumerate()
                .max_by_key(|(_, n)| **n)
                .map(|(k, _)| k)
                .unwrap_or(0);
            slot_of[i] = Some(
                [
                    EquipSlot::Body,
                    EquipSlot::Head,
                    EquipSlot::Weapon,
                    EquipSlot::Footwear,
                ][best],
            );
        }
    }
    let weapon_sections = (0..bca::SECTION_COUNT)
        .filter(|&i| slot_of[i] == Some(EquipSlot::Weapon))
        .count();
    std::array::from_fn(|i| {
        let name = match slot_of[i] {
            Some(EquipSlot::Body) => "Body",
            Some(EquipSlot::Head) => "Head",
            Some(EquipSlot::Footwear) => "Feet",
            Some(EquipSlot::Weapon) if weapon_sections > 1 && exclusive[i] => "Ra-Seru",
            Some(EquipSlot::Weapon) => "Weapon",
            None => return format!("Section {i}"),
        };
        format!("Section {i} ({name})")
    })
}

/// Zero-copy view of one PROT entry's on-disc bytes.
fn entry_bytes<'a>(prot: &'a [u8], entries: &[disc::EntryMeta], index: u32) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
}

/// Drop every vertex belonging to an object that is a **byte-copy of the
/// bone it attaches to** (`AssembledCharacter::duplicate_objects`) and remap
/// the surviving triangles. Drawing a copy alongside its host z-fights one
/// limb against itself; retail reaches the copy only through the actor's
/// `+0xA4` window. Sixteen of the disc's single-section assemblies carry a
/// `200+` surplus that is *not* a copy, so this keys on the measurement
/// rather than on the tag.
fn drop_duplicate_objects(
    mesh: &legaia_tmd::mesh::VramMesh,
    object_ids: &[u32],
    duplicate: &[bool],
) -> (legaia_tmd::mesh::VramMesh, Vec<u32>) {
    let keep = |v: usize| -> bool {
        object_ids
            .get(v)
            .and_then(|o| duplicate.get(*o as usize))
            .is_none_or(|d| !*d)
    };
    let mut remap = vec![u32::MAX; mesh.positions.len()];
    let mut out = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    let mut ids = Vec::new();
    for v in 0..mesh.positions.len() {
        if !keep(v) {
            continue;
        }
        remap[v] = out.positions.len() as u32;
        out.positions.push(mesh.positions[v]);
        out.uvs.push(mesh.uvs[v]);
        out.cba_tsb.push(mesh.cba_tsb[v]);
        out.normals
            .push(mesh.normals.get(v).copied().unwrap_or([0.0; 3]));
        out.colors
            .push(mesh.colors.get(v).copied().unwrap_or([0x80; 3]));
        ids.push(object_ids[v]);
    }
    for tri in mesh.indices.chunks_exact(3) {
        let m: Vec<u32> = tri.iter().map(|&i| remap[i as usize]).collect();
        if m.iter().all(|&i| i != u32::MAX) {
            out.indices.extend_from_slice(&m);
        }
    }
    (out, ids)
}

/// Append `src` (with per-vertex object ids `src_ids`) onto `dst`.
fn append_mesh(
    dst: &mut legaia_tmd::mesh::VramMesh,
    dst_ids: &mut Vec<u32>,
    src: &legaia_tmd::mesh::VramMesh,
    src_ids: &[u32],
) {
    let base = dst.positions.len() as u32;
    dst.positions.extend_from_slice(&src.positions);
    dst.uvs.extend_from_slice(&src.uvs);
    dst.cba_tsb.extend_from_slice(&src.cba_tsb);
    dst.normals.extend_from_slice(&src.normals);
    dst.colors.extend_from_slice(&src.colors);
    dst.indices.extend(src.indices.iter().map(|i| i + base));
    dst_ids.extend_from_slice(src_ids);
}

/// Keep only the vertices whose object is in `objects`, remapping each to the
/// object index given by `remap_to`.
fn keep_objects(
    mesh: &legaia_tmd::mesh::VramMesh,
    object_ids: &[u32],
    remap_to: &std::collections::BTreeMap<u32, u32>,
) -> (legaia_tmd::mesh::VramMesh, Vec<u32>) {
    let mut remap = vec![u32::MAX; mesh.positions.len()];
    let mut out = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    let mut ids = Vec::new();
    for v in 0..mesh.positions.len() {
        let Some(&to) = remap_to.get(&object_ids[v]) else {
            continue;
        };
        remap[v] = out.positions.len() as u32;
        out.positions.push(mesh.positions[v]);
        out.uvs.push(mesh.uvs[v]);
        out.cba_tsb.push(mesh.cba_tsb[v]);
        out.normals
            .push(mesh.normals.get(v).copied().unwrap_or([0.0; 3]));
        out.colors
            .push(mesh.colors.get(v).copied().unwrap_or([0x80; 3]));
        ids.push(to);
    }
    for tri in mesh.indices.chunks_exact(3) {
        let m: Vec<u32> = tri.iter().map(|&i| remap[i as usize]).collect();
        if m.iter().all(|&i| i != u32::MAX) {
            out.indices.extend_from_slice(&m);
        }
    }
    (out, ids)
}

/// Assemble one character at one loadout, with the mesh, the VRAM band, the
/// clip bank and (optionally) the diff against the all-defaults assembly.
fn build(
    prot: &[u8],
    entries: &[disc::EntryMeta],
    cslot: usize,
    equipped: [u8; bca::SECTION_COUNT],
    diff: bool,
) -> Result<EquippedCharacter, String> {
    let prot_index = PLAYER_FILE_BASE + cslot as u32;
    let raw = entry_bytes(prot, entries, prot_index)
        .ok_or_else(|| format!("player file (PROT {prot_index}) not present"))?;
    let pack =
        legaia_asset::battle_data_pack::parse(raw).map_err(|e| format!("player file: {e}"))?;

    let mut asm = bca::assemble_character(raw, &pack, &equipped)
        .map_err(|e| format!("battle-mesh assembly: {e}"))?;
    bca::relocate_tsb_cba(&mut asm.tmd, VIEW_BAND).map_err(|e| format!("TSB/CBA: {e}"))?;
    let tmd = legaia_tmd::parse(&asm.tmd).map_err(|e| format!("assembled TMD parse: {e}"))?;
    let (full_mesh, full_ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &asm.tmd);
    if full_mesh.indices.is_empty() {
        return Err("assembled mesh has no textured primitives".to_string());
    }
    let duplicate = asm.duplicate_objects(&tmd);
    let (mut mesh, mut object_ids) = drop_duplicate_objects(&full_mesh, &full_ids, &duplicate);

    // ---- Diff against the unequipped default assembly ----
    let mut diffs: Vec<equip_diff::ObjectDiff> = Vec::new();
    let mut diff_class = vec![equip_diff::CLASS_SHARED; mesh.positions.len()];
    if equipped != [0u8; bca::SECTION_COUNT] {
        let mut bare = bca::assemble_character(raw, &pack, &[0; bca::SECTION_COUNT])
            .map_err(|e| format!("bare assembly: {e}"))?;
        bca::relocate_tsb_cba(&mut bare.tmd, VIEW_BAND)
            .map_err(|e| format!("bare TSB/CBA: {e}"))?;
        let bare_tmd = legaia_tmd::parse(&bare.tmd).map_err(|e| format!("bare TMD parse: {e}"))?;
        diffs = equip_diff::diff_objects(&bare, &bare_tmd, &asm, &tmd);

        if diff {
            // Equipped side: a vertex outside its bare counterpart's radius
            // envelope is what the equipment added beyond the bare part.
            let env: std::collections::BTreeMap<u32, equip_diff::Envelope> = diffs
                .iter()
                .filter(|d| d.changed())
                .map(|d| {
                    (
                        d.equipped_object as u32,
                        equip_diff::Envelope::of(&equip_diff::object_points(
                            &bare_tmd,
                            d.bare_object,
                        )),
                    )
                })
                .collect();
            for (v, p) in mesh.positions.iter().enumerate() {
                if let Some(e) = env.get(&object_ids[v])
                    && e.outside(*p)
                {
                    diff_class[v] = equip_diff::CLASS_ADDED;
                }
            }
            // Bare side: the whole bare geometry of every replaced object,
            // posed on the equipped object's channel.
            let remap: std::collections::BTreeMap<u32, u32> = diffs
                .iter()
                .filter(|d| d.changed())
                .map(|d| (d.bare_object as u32, d.equipped_object as u32))
                .collect();
            if !remap.is_empty() {
                let (bare_full, bare_full_ids) =
                    legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&bare_tmd, &bare.tmd);
                let (bare_mesh, bare_ids) = keep_objects(&bare_full, &bare_full_ids, &remap);
                diff_class.extend(std::iter::repeat_n(
                    equip_diff::CLASS_BARE_ONLY,
                    bare_mesh.positions.len(),
                ));
                append_mesh(&mut mesh, &mut object_ids, &bare_mesh, &bare_ids);
            }
        }
    }

    // ---- VRAM: the whole band for this loadout ----
    let mut vram = legaia_tim::Vram::new();
    match bca::character_texture_uploads(raw, &pack, &equipped, VIEW_BAND) {
        Ok(uploads) => {
            for u in &uploads {
                vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                // A `clut_n == 0` block uploads pixels only and samples the
                // palette a sibling block already put on the shared row.
                if !u.clut.is_empty() {
                    vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                }
            }
        }
        Err(e) => log_equip(&format!("equipment: char {cslot} texture pool: {e}")),
    }

    // ---- Clip bank: record[0] actions + the equipment-spliced swings ----
    let mut clips: Vec<(String, MonsterAnimation)> = Vec::new();
    match bca::battle_animations(raw) {
        Ok(anims) => {
            for a in &anims {
                clips.push((
                    bca::action_slot_label_or_hex(a.action_id as usize),
                    bca::expand_animation_for_objects(a, &asm.anm_bones),
                ));
            }
        }
        Err(e) => log_equip(&format!("equipment: char {cslot} action bank: {e}")),
    }
    match bca::swing_battle_animations(raw, &pack, &equipped) {
        Ok(swings) => {
            for s in &swings {
                clips.push((
                    bca::action_slot_label_or_hex(s.slot as usize),
                    bca::expand_animation_for_objects(&s.anim, &asm.anm_bones),
                ));
            }
        }
        Err(e) => log_equip(&format!("equipment: char {cslot} swings: {e}")),
    }

    // ---- Item cut: where each equipped weapon / Ra-Seru actually lives ----
    // Each item section is diffed against *the same loadout with that one
    // section stripped*, so the cut is exact regardless of what else is worn.
    let mut items = Vec::new();
    for s in equip_item::ITEM_SECTIONS {
        if equipped[s] == 0 {
            continue;
        }
        let mut without = equipped;
        without[s] = 0;
        let Ok(mut w) = bca::assemble_character(raw, &pack, &without) else {
            continue;
        };
        if bca::relocate_tsb_cba(&mut w.tmd, VIEW_BAND).is_err() {
            continue;
        }
        let Ok(w_tmd) = legaia_tmd::parse(&w.tmd) else {
            continue;
        };
        if let Some(partition) = equip_item::item_partition(s, &w, &w_tmd, &asm, &tmd) {
            items.push(EquippedItem {
                section: s,
                id: asm.sections[s].id,
                partition,
            });
        }
    }

    let resolved = std::array::from_fn(|i| asm.sections[i].id);
    Ok(EquippedCharacter {
        cslot,
        equipped,
        part_count: asm.anm_bones.len(),
        mesh,
        object_ids,
        diff_class,
        diff,
        vram,
        clips,
        diffs,
        resolved,
        tmd_bytes: asm.tmd.clone(),
        bone_tags: asm.bone_tags.clone(),
        anm_bones: asm.anm_bones.clone(),
        items,
    })
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
        match build(&self.disc, &entries, cslot, equipped, diff) {
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
            .map(|(label, a)| {
                serde_json::json!({
                    "label": label,
                    "frames": a.frame_count,
                    "rate": a.rate,
                })
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
                    // `false` = the grip is open: the shaft inside the closed
                    // fist was never modelled, and no cut recovers it.
                    "complete": it.partition.class.is_complete(),
                    "item_primitives": it.partition.item_primitives,
                    "item_vertices": it.partition.item_vertices,
                    "limb_primitives": it.partition.limb_primitives,
                    "seam_vertices": it.partition.seam_vertices,
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
            // The held items this loadout carries, one per equipped weapon /
            // Ra-Seru section, each with how cleanly it separates. Empty for
            // armour-only loadouts - sections 0/1/4 have no item to cut.
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
            .map(|(_, a)| flatten_pose_frames(a))
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
            .map(
                |(label, anim)| legaia_asset::character_gltf::CharacterClip {
                    name: label.clone(),
                    fps: fps_for_rate(anim.rate),
                    anim,
                },
            )
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
    /// The file carries **two named nodes**: the item, and the limb the cut
    /// left behind. The limb is ground truth and is always present, so a
    /// reader can see exactly what was and was not taken.
    ///
    /// For a [`equip_item::ItemClass::WeldedSubset`] record the item's grip
    /// is **open** - the shaft inside the closed fist was never modelled, and
    /// no cutting strategy recovers it. The summary's `complete` flag says
    /// so, and callers must pass that on.
    ///
    /// Empty when the section carries no separable item (armour sections
    /// never do) or nothing is cached.
    pub fn equipped_item_glb(&self, section: u32) -> Vec<u8> {
        let Some(c) = &self.equipped else {
            return Vec::new();
        };
        let section = section as usize;
        let Some(it) = c.items.iter().find(|i| i.section == section) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&c.tmd_bytes) else {
            return Vec::new();
        };
        let (full, ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &c.tmd_bytes);
        // One synthetic object id for the item so it lands in its own glTF
        // node; the limb halves keep their real object ids.
        let item_id = tmd.objects.len() as u32;
        let mut keep: std::collections::BTreeSet<u32> =
            it.partition.parts.iter().map(|p| p.object as u32).collect();
        // A standalone (`own-object`) item's partition names only the item's
        // own object, so the file would ship the weapon with nothing to place
        // it against. Pull in the bone it rides, so every item export carries
        // its ground-truth limb.
        let host_of: std::collections::BTreeSet<u32> = it
            .partition
            .parts
            .iter()
            .filter(|p| p.whole_object)
            .filter_map(|p| {
                let bone = *c.anm_bones.get(p.object)?;
                c.bone_tags
                    .iter()
                    .position(|&t| t == bone)
                    .map(|i| i as u32)
            })
            .collect();
        keep.extend(host_of);
        let mut mesh = legaia_tmd::mesh::VramMesh {
            positions: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            indices: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
        };
        let mut out_ids: Vec<u32> = Vec::new();
        let mut remap = vec![u32::MAX; full.positions.len()];
        for v in 0..full.positions.len() {
            let obj = ids[v];
            if !keep.contains(&obj) {
                continue;
            }
            remap[v] = mesh.positions.len() as u32;
            mesh.positions.push(full.positions[v]);
            mesh.uvs.push(full.uvs[v]);
            mesh.cba_tsb.push(full.cba_tsb[v]);
            mesh.normals
                .push(full.normals.get(v).copied().unwrap_or([0.0; 3]));
            mesh.colors
                .push(full.colors.get(v).copied().unwrap_or([0x80; 3]));
            out_ids.push(if it.partition.claims(obj as usize, full.cba_tsb[v][0]) {
                item_id
            } else {
                obj
            });
        }
        for tri in full.indices.chunks_exact(3) {
            let m: Vec<u32> = tri.iter().map(|&i| remap[i as usize]).collect();
            if m.iter().all(|&i| i != u32::MAX) {
                mesh.indices.extend_from_slice(&m);
            }
        }
        if mesh.indices.is_empty() {
            return Vec::new();
        }
        let who = CHARACTER_LABELS[c.cslot];
        let item_name = u8::try_from(it.id)
            .ok()
            .and_then(|i| self.item_names.as_ref()?.name(i))
            .map(str::to_string)
            .unwrap_or_else(|| format!("id {}", it.id));
        let mut names: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
        names.insert(item_id, item_name.clone());
        for obj in &keep {
            if names.contains_key(obj) {
                continue;
            }
            names.insert(*obj, format!("{who} - host limb (object {obj})"));
        }
        let root = format!(
            "{item_name} - cut from {who}'s battle model ({}{})",
            it.partition.class.tag(),
            if it.partition.class.is_complete() {
                ""
            } else {
                ", grip open"
            }
        );
        legaia_asset::character_gltf::build_character_glb_named(
            &root,
            &mesh,
            &out_ids,
            &c.vram,
            &[],
            None,
            &names,
        )
        .unwrap_or_default()
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
