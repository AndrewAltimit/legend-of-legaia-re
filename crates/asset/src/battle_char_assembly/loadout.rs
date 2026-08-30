//! One assembled **equipment loadout**: the battle model a chosen set of
//! equipped items splices to, its VRAM band, its labelled clip bank, the
//! per-item palette cuts, and the two per-item `.glb` bakers.
//!
//! Hoisted out of the browser equipment viewer so every host shares one
//! implementation (the host-drift law): the wasm characters page and the
//! native `legaia-engine export-glb --items` both assemble a loadout here.
//! The web-viewer keeps only its presentation layer (diff tints, JSON
//! summaries, canvas accessors) on top of these types.
//!
//! Retail never loads a party character's battle mesh whole. It walks the
//! player battle file's five-section descriptor table (`data\battle\
//! PLAYER1..4` = extraction PROT [`PLAYER_FILE_BASE`]` + slot`), matches
//! each section's entries against the character record's `+0x196..+0x19A`
//! equip bytes, and splices the selected sections' TMD objects into one
//! merged blob - the model you fight with is a function of what you wear.
//!
//! Two shapes of the assembled blob every consumer has to respect:
//!
//! * a `200+`-tagged object is **usually** a byte-copy of the bone it
//!   attaches to, and drawing the copy alongside its host z-fights one limb
//!   against itself. Usually, not always - sixteen single-section assemblies
//!   carry a `200+` surplus that differs from its host - so the drop keys on
//!   [`bca::AssembledCharacter::duplicate_objects`] (a byte comparison),
//!   not on the tag.
//! * objects tagged `100+` are ordinary extra geometry and do draw, on the
//!   preceding bone's channel (`anm_bones`).

use crate::battle_char_assembly as bca;
use crate::battle_char_assembly::{equip_diff, equip_isolate, equip_item, equip_repair};
use crate::battle_data_pack::BattleDataPack;
use crate::mesh_raster;
use crate::monster_archive::MonsterAnimation;

/// Player battle files start at extraction PROT entry 863 (Vahn).
pub const PLAYER_FILE_BASE: u32 = 863;
/// Character labels in player-file order.
pub const CHARACTER_LABELS: [&str; 4] = ["Vahn", "Noa", "Gala", "Terra"];
/// Party VRAM band the loadout relocates into (Vahn's: texpages
/// `(512,256)` / `(576,256)`, CLUT row 481).
pub const VIEW_BAND: u8 = 0;

/// One assembled loadout, cached by the host so per-buffer accessors don't
/// re-run the assembly.
pub struct EquippedCharacter {
    pub cslot: usize,
    pub equipped: [u8; bca::SECTION_COUNT],
    /// Pose rig width = the assembled object count (channel `i` -> object `i`).
    pub part_count: usize,
    /// Drawable mesh: the equipped assembly minus its `200+` duplicates, plus
    /// (in diff mode) the bare geometry of every object the loadout replaced.
    pub mesh: legaia_tmd::mesh::VramMesh,
    /// Per-vertex assembled-TMD object index, parallel to `mesh.positions`.
    /// Bare-only vertices carry the index of the equipped object they replace,
    /// so they pose on the same bone.
    pub object_ids: Vec<u32>,
    /// Per-vertex source-primitive ordinal within its object (the mesh
    /// builder's flat group-walk numbering), parallel to `mesh.positions`.
    /// `u32::MAX` on bare-only (diff) vertices. The item-alone mask keys on
    /// `(object id, ordinal)`.
    pub prim_ids: Vec<u32>,
    /// Per-vertex [`equip_diff`] class, parallel to `mesh.positions`.
    pub diff_class: Vec<u8>,
    /// Whether the cached mesh was built with the diff highlight on.
    pub diff: bool,
    pub vram: legaia_tim::Vram,
    /// The character's battle action bank + the equipment-spliced swings +
    /// its Tactical Arts, labeled and expanded per assembled object.
    pub clips: Vec<EquippedClip>,
    /// Per-bone-object diff against the all-defaults assembly.
    pub diffs: Vec<equip_diff::ObjectDiff>,
    /// Resolved section ids (what `select_sections` actually picked).
    pub resolved: [u32; bca::SECTION_COUNT],
    /// The relocated assembled TMD the mesh was built from, kept so the item
    /// cut can re-walk its primitives for the per-item export.
    pub tmd_bytes: Vec<u8>,
    /// Post-sort bone tag per object, and the animation channel each object
    /// rides. The item export uses them to find the limb a standalone
    /// (`own-object`) item hangs off, which its partition does not name.
    pub bone_tags: Vec<u8>,
    pub anm_bones: Vec<u8>,
    /// Per-equipped-**item** section (2 / 3 only), where the held item lives
    /// inside the assembly. See [`equip_item`].
    pub items: Vec<EquippedItem>,
    /// Decode degradations the build tolerated (a texture pool or clip bank
    /// that didn't parse). The build never fails on these - hosts decide
    /// where to log them (browser console / stderr).
    pub notes: Vec<String>,
}

/// One labelled clip of a loadout's bank, expanded per assembled object.
pub struct EquippedClip {
    /// Display label: the action slot's role, the swing direction, or the
    /// art's curated name.
    pub label: String,
    /// Which bank the clip came from (`action` / `swing` / `art`).
    pub kind: ClipKind,
    /// Art-only metadata (curated kind, AP, input, bank record, segments).
    pub art: Option<ArtClipMeta>,
    pub anim: MonsterAnimation,
}

/// Where a loadout clip comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    /// `record[0]`'s action bank (idle, walk, flinches, knockdown, ...).
    Action,
    /// A loader-spliced direction-command weapon swing (changes with the
    /// equipped weapon).
    Swing,
    /// A Tactical Art out of the character's `readef.DAT` ME archive.
    Art,
}

impl ClipKind {
    pub fn tag(self) -> &'static str {
        match self {
            ClipKind::Action => "action",
            ClipKind::Swing => "swing",
            ClipKind::Art => "art",
        }
    }
}

/// The curated facts an art clip carries next to its keyframes.
pub struct ArtClipMeta {
    pub kind: &'static str,
    pub ap: u32,
    pub directions: Vec<u8>,
    pub anim_id: u8,
    pub segments: usize,
}

/// One curated art resolved to its on-disc keyframes, **unexpanded** (raw
/// bank part layout - [`build`] re-indexes per assembled object with
/// [`bca::expand_animation_for_objects`]). The resolution ladder that fills
/// this (curated name -> bank record, via `legaia_gamedata`) lives with the
/// host; this is the data the loadout consumes.
#[derive(Clone)]
pub struct ArtClip {
    /// The curated display name (arts table).
    pub name: String,
    /// `regular` / `hyper` / `super` / `miracle`.
    pub kind: &'static str,
    /// AP the art costs when fully expressed (arts table).
    pub ap: u32,
    /// Direction bytes of the input (`1=L 2=R 3=D 4=U`).
    pub directions: Vec<u8>,
    /// Staged anim id of the first bank record the art resolved to.
    pub anim_id: u8,
    /// How many consecutive bank records the clip concatenates.
    pub segments: usize,
    /// The concatenated keyframe stream (first segment's rate byte).
    pub anim: MonsterAnimation,
}

/// One separated item of the current loadout.
pub struct EquippedItem {
    pub section: usize,
    pub id: u32,
    pub partition: equip_item::ItemPartition,
    /// The opinionated item-alone cut (see [`equip_isolate`]).
    pub isolation: equip_isolate::IsolatedItem,
    /// The item-alone geometry, object-local, with its grip repaired
    /// ([`equip_repair`]) - what the alone export, its preview and its
    /// thumbnail all draw. Built once per loadout.
    pub alone: ItemAloneMesh,
}

/// The item-alone mesh plus what the repair added to it.
pub struct ItemAloneMesh {
    pub mesh: legaia_tmd::mesh::VramMesh,
    pub object_ids: Vec<u32>,
    pub bridges: Vec<equip_repair::Bridge>,
}

impl ItemAloneMesh {
    fn build(tmd: &legaia_tmd::Tmd, blob: &[u8], iso: &equip_isolate::IsolatedItem) -> Self {
        let (mut mesh, mut object_ids) = equip_isolate::item_mesh(tmd, blob, iso);
        let bridges = equip_repair::bridge_open_loops(&mut mesh, &mut object_ids);
        ItemAloneMesh {
            mesh,
            object_ids,
            bridges,
        }
    }
    pub fn bridged_triangles(&self) -> usize {
        self.bridges.iter().map(|b| b.triangles).sum()
    }
}

/// One equipment section of a player file: the ids it offers, in table order.
pub struct SectionCatalog {
    /// Real equipment ids (the `id = 0` default is not one of them).
    pub ids: Vec<u32>,
}

/// Walk a player file's descriptor table into its five sections. Entries with
/// `id == 0` are the section default *and* the section separator.
pub fn section_catalog(pack: &BattleDataPack) -> [SectionCatalog; bca::SECTION_COUNT] {
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
pub fn section_labels(
    cats: &[SectionCatalog; bca::SECTION_COUNT],
    stats: Option<&crate::equip_stats::EquipStatTable>,
    cslot: usize,
) -> [String; bca::SECTION_COUNT] {
    use crate::equip_stats::EquipSlot;
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
    prim_ids: &[u32],
    duplicate: &[bool],
) -> (legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u32>) {
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
    let mut prims = Vec::new();
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
        prims.push(prim_ids.get(v).copied().unwrap_or(u32::MAX));
    }
    for tri in mesh.indices.chunks_exact(3) {
        let m: Vec<u32> = tri.iter().map(|&i| remap[i as usize]).collect();
        if m.iter().all(|&i| i != u32::MAX) {
            out.indices.extend_from_slice(&m);
        }
    }
    (out, ids, prims)
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

/// Keep only the vertices whose object is in `remap_to`, remapping each to
/// the object index it maps to.
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
///
/// `raw` is the character's player battle file - PROT entry
/// [`PLAYER_FILE_BASE`]` + cslot` - and `arts` is the character's decoded
/// Tactical Arts clip bank (empty is fine; only the labels are lost).
/// Tolerated decode degradations land in the result's `notes`.
pub fn build(
    raw: &[u8],
    cslot: usize,
    equipped: [u8; bca::SECTION_COUNT],
    diff: bool,
    arts: &[ArtClip],
) -> Result<EquippedCharacter, String> {
    let pack = crate::battle_data_pack::parse(raw).map_err(|e| format!("player file: {e}"))?;
    let mut notes: Vec<String> = Vec::new();

    let mut asm = bca::assemble_character(raw, &pack, &equipped)
        .map_err(|e| format!("battle-mesh assembly: {e}"))?;
    bca::relocate_tsb_cba(&mut asm.tmd, VIEW_BAND).map_err(|e| format!("TSB/CBA: {e}"))?;
    let tmd = legaia_tmd::parse(&asm.tmd).map_err(|e| format!("assembled TMD parse: {e}"))?;
    let (full_mesh, full_ids, full_prims) =
        legaia_tmd::mesh::tmd_to_vram_mesh_with_prim_ids(&tmd, &asm.tmd);
    if full_mesh.indices.is_empty() {
        return Err("assembled mesh has no textured primitives".to_string());
    }
    let duplicate = asm.duplicate_objects(&tmd);
    let (mut mesh, mut object_ids, mut prim_ids) =
        drop_duplicate_objects(&full_mesh, &full_ids, &full_prims, &duplicate);

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
                prim_ids.extend(std::iter::repeat_n(u32::MAX, bare_mesh.positions.len()));
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
        Err(e) => notes.push(format!("char {cslot} texture pool: {e}")),
    }

    // ---- Clip bank: record[0] actions + the equipment-spliced swings +
    // the character's Tactical Arts (decoded once per character by the
    // caller, re-expanded here per assembled object) ----
    let mut clips: Vec<EquippedClip> = Vec::new();
    match bca::battle_animations(raw) {
        Ok(anims) => {
            for a in &anims {
                clips.push(EquippedClip {
                    label: bca::action_slot_label_or_hex(a.action_id as usize),
                    kind: ClipKind::Action,
                    art: None,
                    anim: bca::expand_animation_for_objects(a, &asm.anm_bones),
                });
            }
        }
        Err(e) => notes.push(format!("char {cslot} action bank: {e}")),
    }
    match bca::swing_battle_animations(raw, &pack, &equipped) {
        Ok(swings) => {
            for s in &swings {
                clips.push(EquippedClip {
                    label: bca::action_slot_label_or_hex(s.slot as usize),
                    kind: ClipKind::Swing,
                    art: None,
                    anim: bca::expand_animation_for_objects(&s.anim, &asm.anm_bones),
                });
            }
        }
        Err(e) => notes.push(format!("char {cslot} swings: {e}")),
    }
    for art in arts {
        clips.push(EquippedClip {
            label: art.name.clone(),
            kind: ClipKind::Art,
            art: Some(ArtClipMeta {
                kind: art.kind,
                ap: art.ap,
                directions: art.directions.clone(),
                anim_id: art.anim_id,
                segments: art.segments,
            }),
            anim: bca::expand_animation_for_objects(&art.anim, &asm.anm_bones),
        });
    }

    // ---- Item cut: where each equipped piece actually lives ----
    // Every equipped section gets one. A held item (sections 2/3) is diffed
    // against *the same loadout with that one section stripped*, so the
    // palette cut is exact regardless of what else is worn; anything with no
    // material boundary comes back `fused` - the section's whole contribution
    // - rather than nothing. Completeness over purity, by policy.
    let mut items = Vec::new();
    for s in 0..bca::SECTION_COUNT {
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
            // The item-alone cut compares texels, so it needs the stripped
            // loadout's VRAM band too (its own texture pool + CLUT run).
            let mut w_vram = legaia_tim::Vram::new();
            match bca::character_texture_uploads(raw, &pack, &without, VIEW_BAND) {
                Ok(uploads) => {
                    for u in &uploads {
                        w_vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                        if !u.clut.is_empty() {
                            w_vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                        }
                    }
                }
                Err(e) => notes.push(format!("char {cslot} stripped pool: {e}")),
            }
            let id = asm.sections[s].id;
            let isolation = equip_isolate::isolate_item(
                &equip_isolate::IsolationInputs {
                    section: s,
                    bare: &w,
                    bare_tmd: &w_tmd,
                    bare_vram: &w_vram,
                    equipped: &asm,
                    equipped_tmd: &tmd,
                    vram: &vram,
                    partition: &partition,
                },
                equip_isolate::rules().rule_for(cslot, id),
            );
            let alone = ItemAloneMesh::build(&tmd, &asm.tmd, &isolation);
            items.push(EquippedItem {
                section: s,
                id,
                partition,
                isolation,
                alone,
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
        prim_ids,
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
        notes,
    })
}

fn fps_for_rate(rate: u8) -> f32 {
    if rate > 0 {
        7.5 * f32::from(rate)
    } else {
        15.0
    }
}

fn clip_bank<'a>(c: &'a EquippedCharacter) -> Vec<crate::character_gltf::CharacterClip<'a>> {
    c.clips
        .iter()
        .map(|clip| crate::character_gltf::CharacterClip {
            name: clip.label.clone(),
            fps: fps_for_rate(clip.anim.rate),
            anim: &clip.anim,
        })
        .collect()
}

/// The record-keeping per-item export: the exact palette cut of equipped
/// `section` **with its ground-truth host limb**, one node per source
/// object, each posed by the object it came from, plus the clip bank.
///
/// A standalone (`own-object`) item's partition names only the item's own
/// object, so the file would ship the weapon with nothing to place it
/// against - the bone it rides is pulled in too. Empty when the section is
/// at its default or contributed no geometry.
pub fn item_glb(
    c: &EquippedCharacter,
    section: usize,
    names: Option<&crate::item_names::ItemNameTable>,
) -> Vec<u8> {
    let Some(it) = c.items.iter().find(|i| i.section == section) else {
        return Vec::new();
    };
    let Ok(tmd) = legaia_tmd::parse(&c.tmd_bytes) else {
        return Vec::new();
    };
    let (full, ids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &c.tmd_bytes);
    // A synthetic object id per **source object** the item occupies, so
    // each piece lands in its own glTF node and can take that object's
    // pose. One shared id would collapse a multi-object item (Vahn's
    // sword spans forearm and hand; a fused armour spans the whole
    // torso chain) into a single node, and a single node has a single
    // transform - the pieces would stack at the model origin.
    let item_id_base = tmd.objects.len() as u32;
    let item_id_of: std::collections::BTreeMap<u32, u32> = it
        .partition
        .parts
        .iter()
        .enumerate()
        .map(|(k, p)| (p.object as u32, item_id_base + k as u32))
        .collect();
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
            item_id_of.get(&obj).copied().unwrap_or(obj)
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
    let item_name = item_display_name(it.id, names);
    let mut layout = crate::character_gltf::CharacterGlbLayout::default();
    // Each item piece is named for the item, and inherits the rest pose
    // and animation channels of the object it was cut from.
    let multi = item_id_of.len() > 1;
    for (&src, &synth) in &item_id_of {
        layout.names.insert(
            synth,
            if multi {
                format!("{item_name} (object {src})")
            } else {
                item_name.clone()
            },
        );
        layout.pose_source.insert(synth, src);
    }
    for obj in &keep {
        layout
            .names
            .entry(*obj)
            .or_insert_with(|| format!("{who} - host limb (object {obj})"));
    }
    let root = format!(
        "{item_name} - {} {who}'s battle model ({})",
        if it.partition.class.is_pure() {
            "cut from"
        } else {
            "as spliced into"
        },
        it.partition.class.describe()
    );
    // The clip bank is what supplies the **rest pose**: the builder reads
    // clip 0 frame 0 into each node's TRS. Passing none leaves every node
    // at the model origin, which is what made a two-object export read as
    // two hands stacked on each other.
    let clips = clip_bank(c);
    crate::character_gltf::build_character_glb_named(
        &root, &mesh, &out_ids, &c.vram, &clips, None, &layout,
    )
    .unwrap_or_default()
}

/// The **item alone** export: the repaired item-alone mesh as one node per
/// source object, each posed by the object it came from, plus the clip
/// bank. The root name says how the cut was decided (`colour-diff` /
/// `identity` / `whole` / `palette`), whether a rule touched it
/// (`curated`), and whether the grip was inferred (`grip inferred`), so a
/// downloader can tell a heuristic result from a checked one and a disc
/// fact from a repair.
pub fn item_only_glb(
    c: &EquippedCharacter,
    section: usize,
    names: Option<&crate::item_names::ItemNameTable>,
) -> Vec<u8> {
    let Some(it) = c.items.iter().find(|i| i.section == section) else {
        return Vec::new();
    };
    if it.isolation.kept_primitives == 0 || it.alone.mesh.indices.is_empty() {
        return Vec::new();
    }
    let who = CHARACTER_LABELS[c.cslot];
    let item_name = item_display_name(it.id, names);
    let mut layout = crate::character_gltf::CharacterGlbLayout::default();
    let objects: Vec<u32> = it
        .alone
        .object_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<u32>>()
        .into_iter()
        .collect();
    for &obj in &objects {
        layout.names.insert(
            obj,
            if objects.len() > 1 {
                format!("{item_name} (object {obj})")
            } else {
                item_name.clone()
            },
        );
    }
    let root = format!(
        "{item_name} - item alone, cut from {who}'s battle model ({}{}{})",
        it.isolation.mode.tag(),
        if it.isolation.curated {
            ", curated"
        } else {
            ""
        },
        if it.alone.bridges.is_empty() {
            String::new()
        } else {
            format!(", grip inferred: {} bridge(s)", it.alone.bridges.len())
        }
    );
    let clips = clip_bank(c);
    crate::character_gltf::build_character_glb_named(
        &root,
        &it.alone.mesh,
        &it.alone.object_ids,
        &c.vram,
        &clips,
        None,
        &layout,
    )
    .unwrap_or_default()
}

/// The item's display name out of the SCUS table, or `id N` without one.
pub fn item_display_name(id: u32, names: Option<&crate::item_names::ItemNameTable>) -> String {
    u8::try_from(id)
        .ok()
        .and_then(|i| names?.name(i))
        .map(str::to_string)
        .unwrap_or_else(|| format!("id {id}"))
}

/// The character's rest stance - clip 0 (the action bank's first record)
/// at frame 0 - as one rigid placement per assembled object. Identity for
/// every object when the bank is missing.
pub fn rest_poses(c: &EquippedCharacter) -> Vec<mesh_raster::Pose> {
    let Some(clip) = c.clips.first() else {
        return vec![mesh_raster::Pose::IDENTITY; c.part_count];
    };
    let Some(frame) = clip.anim.frames.first() else {
        return vec![mesh_raster::Pose::IDENTITY; c.part_count];
    };
    frame
        .iter()
        .map(|p| mesh_raster::Pose::from_keyframe([p.tx, p.ty, p.tz], [p.rx, p.ry, p.rz]))
        .collect()
}
