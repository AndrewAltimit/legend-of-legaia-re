//! nilboa (Nivora Ravine) field-side mirror of the Delilas party swap:
//! rebuild the scene's three Delilas NPC field meshes as the mapped
//! **heroes'** field meshes, so the duel scene fields Vahn / Noa / Gala
//! instead of a second set of Delilas siblings.
//!
//! This is the exact inverse of [`fieldize::fieldize_pack_npc`] (which
//! rebuilds the party's PROT 0874 walking models from these NPC meshes):
//! each hero's retail field rig (PROT 0874 §0, the 10 posed bones of the
//! 12-group TMD - the two unposed equipment-template groups are dropped)
//! bakes part-by-part into the corresponding Delilas NPC part's rest
//! frame, so the scene's own idle/gesture ANM records (which pose parts
//! **by index** with flat rigid transforms) animate the hero mesh exactly
//! as they animated the sibling's.
//!
//! **Ordering constraint (load-bearing):** `prot_0874` must be the
//! **pre-`fieldize`** entry bytes. The `--delilas-party` pass rewrites
//! PROT 0874 with sibling geometry; sourcing the heroes from the
//! rewritten entry would bake siblings back onto themselves. Capture the
//! entry before `apply_delilas_party` runs, or run this pass first.
//!
//! Geometry: the three retail members (30 276 bytes, tiling the pack
//! with no slack) are the budget; the hero rigs re-encode to ~33 348
//! bytes at full detail, so body texture flattening (retail field
//! bodies are flat-shaded; only the face stays textured) plus a
//! decimation ladder close the gap. nilboa is a fixed-camera scene, so
//! no winding-doubling or boundary sealing is needed.
//!
//! Texture: each sibling's head TIM in the PROT 0638 bundle is a
//! 64x64-texel 4bpp window with a 16x2 CLUT block (two 16-colour
//! palettes) on row 481; the hero's head islands re-lay into that
//! window (halving on overflow) and the CLUT block repaints, leaving
//! every other scene TIM byte-identical. Only pack members 106/107/108
//! reference those windows / palettes (measured across all 112
//! members), so the repaint cannot bleed into other scene actors.

use super::fieldize::{
    self, FIELD_BONES, FIELD_CHILD, FIELD_PARENT, decimate_object, derive_field_roles,
    flatten_prim, group_world_stats, paint_vram,
};
use super::playerize::{merge_palettes, nearest_color};
use super::*;
use crate::character_pack;
use crate::pack::parse_pack;
use crate::parse_player_lzs;
use legaia_tmd::descriptor::PacketShape;

/// PROT entry of the nilboa TMD pack (re-exported from [`fieldize`]).
pub const NPC_PACK_ENTRY: usize = fieldize::NPC_PACK_ENTRY;
/// PROT entry of the nilboa scene bundle (ANM records + TIM list).
pub const NPC_BUNDLE_ENTRY: usize = fieldize::NPC_BUNDLE_ENTRY;

/// The rebuilt nilboa entries.
#[derive(Debug, Clone)]
pub struct NivoraFieldPatch {
    /// Rebuilt PROT 0639 entry bytes (same length as the input).
    pub pack_entry: Vec<u8>,
    /// Rebuilt PROT 0638 entry bytes (same length as the input).
    pub bundle_entry: Vec<u8>,
    /// Non-fatal notes (decimation level, texture downscales).
    pub warnings: Vec<String>,
}

/// One hero's field-mesh source: the 10 posed bones of the PROT 0874 §0
/// rig, the locomotion idle rest pose, and the atlas texel space its
/// textured prims sample (`cba` re-keyed to index `cluts`).
pub(super) struct HeroSource {
    model: Vec<ModelObject>,
    rest: Vec<PartPose>,
    cluts: Vec<[u16; 16]>,
    indices: Vec<u8>,
    width: usize,
}

/// One sibling NPC target: the retail member mesh (for anatomy + budget),
/// its scene idle rest pose, and its head-TIM window inside the bundle.
pub(super) struct NpcTarget {
    pub(super) model: Vec<ModelObject>,
    pub(super) rest: Vec<PartPose>,
    /// Object-index -> body-role assignment. Derived from the first
    /// frame of the scene record that splits the limb chains cleanly -
    /// the assignment is pose-independent, but the geometric splitter
    /// needs a frame where left and right limbs sit on their own sides
    /// (an event stance like taiku2's kneel crosses them at frame 0).
    pub(super) roles: [usize; FIELD_BONES],
    pub(super) head_tim: HeadTimWindow,
}

/// Where a sibling's head texture lives: the TIM's byte offset inside
/// the decoded §0 TIM list, its texel window inside texpage 5, and the
/// CLUT columns its prims address.
pub(super) struct HeadTimWindow {
    /// Byte offset of the TIM (its 0x10 magic) inside the decoded §0.
    tim_offset: usize,
    /// Texel-window origin inside the page (u = (fb_x - page_x) * 4).
    u_base: usize,
    v_base: usize,
    /// Window size in texels.
    w: usize,
    h: usize,
    /// First CLUT column (cba column units, 16-texel steps).
    clut_col: u16,
    /// CLUT row the sibling's prims address (row 481 in retail).
    clut_row: u16,
    /// Number of palette rows the CLUT block holds.
    clut_rows: usize,
    /// The tsb the sibling's own textured prims author.
    tsb: u16,
}

/// Decode one hero slot from the pre-fieldize PROT 0874 entry.
pub(super) fn hero_slot_source(prot_0874: &[u8], slot: usize) -> Result<HeroSource> {
    let pack = character_pack::parse(prot_0874).context("parse PROT 0874")?;
    let cs = pack
        .slot(slot)
        .ok_or_else(|| anyhow::anyhow!("hero slot {slot} missing"))?;
    let tmd = legaia_tmd::parse(&cs.tmd_bytes).context("hero field TMD")?;
    let mut model = decode_model(&tmd, &cs.tmd_bytes)?;
    if model.len() < FIELD_BONES {
        bail!(
            "hero slot {slot} has {} groups, expected at least {FIELD_BONES}",
            model.len()
        );
    }
    // Drop the two unposed equipment-template groups: the NPC rig has 10
    // parts and the scene ANM records pose exactly 10 channels.
    model.truncate(FIELD_BONES);

    let anm = character_pack::field_locomotion_anm(prot_0874).context("locomotion bundle")?;
    let idle_rec =
        character_pack::locomotion_record_index(slot, character_pack::LOCOMOTION_IDLE_SLOT);
    let idle = anm
        .record_to_monster_animation(idle_rec)
        .ok_or_else(|| anyhow::anyhow!("hero slot {slot}: idle clip missing"))?;
    if idle.part_count < FIELD_BONES {
        bail!(
            "hero idle poses {} bones, expected {FIELD_BONES}",
            idle.part_count
        );
    }
    let rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("hero idle has no frames"))?
        .clone();

    // Texel space: paint the §2 atlas pack's TIMs into a virtual VRAM
    // and sample the one texpage the hero prims reference (0x1D). Same
    // side-by-side multi-row CLUT flatten as the scene-bundle path.
    let container = parse_player_lzs(prot_0874, character_pack::CONTAINER_DESCRIPTORS)?;
    let sec2 = crate::decode(prot_0874, &container.descriptors[2], crate::DecodeMode::Lzs)
        .context("PROT 0874 atlas section")?;
    let mut vram = vec![0u16; 1024 * 512];
    for bytes in crate::pack::extract_pack(&sec2)? {
        let Ok(tim) = legaia_tim::parse(bytes) else {
            continue;
        };
        if let Some(c) = tim.clut.as_ref() {
            for row in 0..c.h as usize {
                let row_bytes: Vec<u8> = c.entries[row * c.w as usize..(row + 1) * c.w as usize]
                    .iter()
                    .flat_map(|e| e.to_le_bytes())
                    .collect();
                paint_vram(
                    &mut vram,
                    c.fb_x + (row as u16) * c.w,
                    c.fb_y,
                    c.w,
                    1,
                    &row_bytes,
                );
            }
        }
        paint_vram(
            &mut vram,
            tim.image.fb_x,
            tim.image.fb_y,
            tim.image.fb_w,
            tim.image.h,
            &tim.image.data,
        );
    }
    let mut pages: Vec<u16> = Vec::new();
    let mut cbas: Vec<u16> = Vec::new();
    for o in &model {
        for g in &o.groups {
            if !g.shape.is_textured() {
                continue;
            }
            for p in &g.prims {
                if !pages.contains(&(p.tsb & 0x1F)) {
                    pages.push(p.tsb & 0x1F);
                }
                if !cbas.contains(&p.cba) {
                    cbas.push(p.cba);
                }
            }
        }
    }
    let page = match pages.as_slice() {
        [] => 0u16,
        [one] => *one,
        more => bail!("hero mesh references {} texpages, expected 1", more.len()),
    };
    let (page_x, page_y) = (
        ((page & 0xF) as usize) * 64,
        (((page >> 4) & 1) as usize) * 256,
    );
    let width = UV_SPACE;
    let mut indices = vec![0u8; width * PAGE_HEIGHT];
    for v in 0..PAGE_HEIGHT {
        for u in 0..width {
            let hw = vram[(page_y + v) * 1024 + page_x + u / 4];
            indices[v * width + u] = ((hw >> ((u % 4) * 4)) & 0xF) as u8;
        }
    }
    let cluts: Vec<[u16; 16]> = cbas
        .iter()
        .map(|&cba| {
            let (cx, cy) = (((cba & 0x3F) as usize) * 16, (cba >> 6) as usize);
            let mut pal = [0u16; 16];
            for (i, p) in pal.iter_mut().enumerate() {
                *p = vram[cy * 1024 + cx + i];
            }
            pal
        })
        .collect();
    for o in model.iter_mut() {
        for g in o.groups.iter_mut() {
            if !g.shape.is_textured() {
                continue;
            }
            for p in g.prims.iter_mut() {
                p.cba = cbas.iter().position(|&c| c == p.cba).unwrap_or(0) as u16;
            }
        }
    }
    Ok(HeroSource {
        model,
        rest,
        cluts,
        indices,
        width,
    })
}

/// Locate one sibling's NPC member + rest pose + head-TIM window.
///
/// The rest pose is the scene idle record **verbatim** (no leg
/// symmetrization, no head-roll leveling): unlike the fieldize
/// direction - where the scene stance would ride into every party
/// locomotion pose - here the scene's own records ARE the playback, so
/// the authored stance is exactly the frame the bake must anchor in.
fn npc_target(
    npc_pack: &[u8],
    sec0_tims: &[u8],
    npc_bundle: &[u8],
    monster_id: u16,
) -> Result<NpcTarget> {
    let (member, idle_rec) = fieldize::npc_coords(monster_id)
        .ok_or_else(|| anyhow::anyhow!("monster id {monster_id} has no nilboa coordinates"))?;
    let head = u32::from_le_bytes(
        npc_pack
            .get(0..4)
            .ok_or_else(|| anyhow::anyhow!("NPC pack entry too short"))?
            .try_into()
            .unwrap(),
    );
    if head >> 24 != 0x02 {
        bail!("NPC pack entry head {head:#x} is not a type-2 TMD stream");
    }
    let bundle = crate::player_anm::find_in_entry(npc_bundle, 5)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("NPC bundle entry carries no ANM bundle"))?;
    npc_target_at(&npc_pack[4..], sec0_tims, &bundle, member, idle_rec, None)
}

/// Scene-generic core of [`npc_target`]: locate one sibling member + rest
/// pose + head-TIM window given the raw coordinates instead of the nilboa
/// lookup. `pack` is the member pack **body** (count word first - no
/// entry-head type word), `tims` any buffer holding the scene's raw TIMs
/// at 4-aligned offsets (a decoded `TIM_LIST` section or a whole
/// `tim_pack` entry), `anm` the scene's actor-animation bundle, and
/// `idle_rec` the ANM record the scene's placement binds to the member
/// (`ActorPlacement::anim_id - 1`).
pub(super) fn npc_target_at(
    pack: &[u8],
    sec0_tims: &[u8],
    bundle: &crate::player_anm::PlayerAnmBundle,
    member: usize,
    idle_rec: usize,
    roles_override: Option<[usize; FIELD_BONES]>,
) -> Result<NpcTarget> {
    let entries = parse_pack(pack)?;
    let e = entries
        .get(member)
        .ok_or_else(|| anyhow::anyhow!("NPC pack member {member} missing"))?;
    let tmd_bytes = pack
        .get(e.byte_offset..e.byte_offset + e.size)
        .ok_or_else(|| anyhow::anyhow!("NPC pack member {member} out of range"))?;
    let tmd = legaia_tmd::parse(tmd_bytes).context("NPC TMD")?;
    let model = decode_model(&tmd, tmd_bytes)?;
    if model.len() != FIELD_BONES {
        bail!("NPC mesh has {} parts, expected {FIELD_BONES}", model.len());
    }

    let idle = bundle
        .record_to_monster_animation(idle_rec)
        .ok_or_else(|| anyhow::anyhow!("NPC idle record {idle_rec} missing"))?;
    if idle.part_count != FIELD_BONES {
        bail!(
            "NPC idle poses {} bones, expected {FIELD_BONES}",
            idle.part_count
        );
    }
    let rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("NPC idle has no frames"))?
        .clone();
    // An event stance can fool the geometric splitter into a clean-looking
    // but wrong pairing (taiku2's kneel puts the torso on a leg bone), so a
    // spec may pin the assignment measured off a trusted stance of the
    // byte-identical rig instead.
    let roles = match roles_override {
        Some(r) => r,
        None => idle
            .frames
            .iter()
            .find_map(|f| derive_field_roles(&model, f).ok())
            .ok_or_else(|| {
                anyhow::anyhow!("no frame of NPC record {idle_rec} splits the rig into roles")
            })?,
    };

    // The member's own texture conventions: one texpage, a run of CLUT
    // columns on one row.
    let mut cbas: Vec<u16> = Vec::new();
    let mut tsbs: Vec<u16> = Vec::new();
    for o in &model {
        for g in &o.groups {
            if !g.shape.is_textured() {
                continue;
            }
            for p in &g.prims {
                if !cbas.contains(&p.cba) {
                    cbas.push(p.cba);
                }
                if !tsbs.contains(&p.tsb) {
                    tsbs.push(p.tsb);
                }
            }
        }
    }
    if cbas.is_empty() {
        bail!("NPC member {member} has no textured prims");
    }
    cbas.sort_unstable();
    let clut_col = cbas[0] & 0x3F;
    let clut_row = cbas[0] >> 6;
    let page = tsbs[0] & 0x1F;
    let (page_x, page_y) = ((page & 0xF) * 64, ((page >> 4) & 1) * 256);

    // Find the sibling's head TIM in the decoded §0 stream: the CLUT'd
    // TIM whose CLUT block starts at this member's first CLUT column.
    let mut found = None;
    let mut off = 0usize;
    while off + 8 <= sec0_tims.len() {
        if u32::from_le_bytes(sec0_tims[off..off + 4].try_into().unwrap()) == 0x10
            && let Ok(tim) = legaia_tim::parse(&sec0_tims[off..])
            && let Some(c) = tim.clut.as_ref()
            && c.fb_x == clut_col * 16
            && c.fb_y == clut_row
        {
            found = Some(HeadTimWindow {
                tim_offset: off,
                u_base: (tim.image.fb_x.saturating_sub(page_x) as usize) * 4,
                v_base: tim.image.fb_y.saturating_sub(page_y) as usize,
                w: (tim.image.fb_w as usize) * 4,
                h: tim.image.h as usize,
                clut_col,
                clut_row,
                clut_rows: c.h as usize,
                tsb: tsbs[0],
            });
            break;
        }
        off += 4;
    }
    let head_tim = found.ok_or_else(|| {
        anyhow::anyhow!("no scene TIM with CLUT at ({}, {clut_row})", clut_col * 16)
    })?;
    Ok(NpcTarget {
        model,
        rest,
        roles,
        head_tim,
    })
}

/// One rebuilt member: the baked 10-part TMD plus its head-window
/// repaint (texels in window coordinates + palettes).
pub(super) struct HeroizedSlot {
    pub(super) tmd: Vec<u8>,
    pub(super) window: Vec<u8>,
    pub(super) palettes: Vec<Vec<u16>>,
}

/// Bake one hero rig onto one sibling's NPC rest conventions.
pub(super) fn heroize_slot(
    hero: &HeroSource,
    npc: &NpcTarget,
    decimate: f32,
    warnings: &mut Vec<String>,
) -> Result<HeroizedSlot> {
    let src_roles = derive_field_roles(&hero.model, &hero.rest)?;
    let dst_roles = npc.roles;

    // Radial scale: whole-rig height ratio (NPC over hero) - keeps the
    // hero's own proportions while standing at the sibling's height, so
    // the scene camera framing and floor contact survive.
    let radial = {
        let src_parts: Vec<(&ModelObject, &PartPose)> = src_roles
            .iter()
            .map(|&b| (&hero.model[b], &hero.rest[b]))
            .collect();
        let dst_parts: Vec<(&ModelObject, &PartPose)> = dst_roles
            .iter()
            .map(|&b| (&npc.model[b], &npc.rest[b]))
            .collect();
        let (_, e_src) = group_world_stats(&src_parts);
        let (_, e_dst) = group_world_stats(&dst_parts);
        (e_dst[1] / e_src[1]).clamp(0.25, 4.0)
    };
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_pivots: Vec<[f32; 3]> = src_roles.iter().map(|&b| pivot_of(&hero.rest[b])).collect();
    let dst_pivots: Vec<[f32; 3]> = dst_roles.iter().map(|&b| pivot_of(&npc.rest[b])).collect();
    let src_frames = bone_frames(&src_pivots, &FIELD_CHILD, &FIELD_PARENT);
    let dst_frames = bone_frames(&dst_pivots, &FIELD_CHILD, &FIELD_PARENT);

    let mut parts: Vec<Option<ModelObject>> = vec![None; FIELD_BONES];
    for role in 0..FIELD_BONES {
        let src_bone = src_roles[role];
        let dst_bone = dst_roles[role];
        let mut part = hero.model[src_bone].clone();
        if role != 0 {
            // Flatten body texture to the retail field NPC style (and
            // the byte budget): one colour word per prim, sampled from
            // the hero atlas.
            for g in part.groups.iter_mut() {
                if !g.shape.is_textured() {
                    continue;
                }
                let new_shape = if g.shape.n_vertices() == 4 {
                    PacketShape::F4
                } else {
                    PacketShape::F3
                };
                for p in g.prims.iter_mut() {
                    *p = flatten_prim(p, &hero.cluts, &hero.indices, hero.width);
                }
                g.shape = new_shape;
                g.semi_transparent = false;
            }
        }
        let pb = pivot_bake_params(&src_frames[role], &dst_frames[role], radial);
        bake_object_pivot(
            &mut part,
            &hero.rest[src_bone],
            src_pivots[role],
            &npc.rest[dst_bone],
            &pb,
        )
        .with_context(|| format!("field bake role {role}"))?;
        if role == 0 {
            // Seat the hero head on the sibling's neck.
            seat_terminal_axial(
                &mut part,
                &npc.model[dst_bone],
                &npc.rest[dst_bone],
                pb.x_dst,
            )?;
            // Scene context: draw the face opaque (the hero atlas head
            // is authored opaque; ABE against the scene would see-through).
            for g in part.groups.iter_mut() {
                g.semi_transparent = false;
            }
        }
        compact_object(&mut part);
        decimate_object(&mut part, decimate);
        parts[dst_bone] = Some(part);
    }
    let mut model: Vec<ModelObject> = Vec::with_capacity(FIELD_BONES);
    for (i, p) in parts.into_iter().enumerate() {
        model.push(p.ok_or_else(|| anyhow::anyhow!("NPC bone {i} not covered by any role"))?);
    }

    // Head texture into the sibling's own TIM window.
    let head_bone = dst_roles[0];
    let (window, palettes) = layout_window(
        &mut model[head_bone],
        &hero.cluts,
        &hero.indices,
        hero.width,
        &npc.head_tim,
        warnings,
    )?;
    let tmd = encode(&model).context("encode nilboa member TMD")?;
    Ok(HeroizedSlot {
        tmd,
        window,
        palettes,
    })
}

/// A UV-space bounding box `(min_u, min_v, max_u, max_v)` (the
/// [`face_bbox`] shape).
type UvBox = (u8, u8, u8, u8);

/// Face clusters that pack as one island: member face indices + the
/// union bbox.
type UvCluster = (Vec<usize>, UvBox);

/// Re-lay the (baked) head part's texture islands into the sibling's
/// TIM window, rewriting UV/CBA/TSB in place. Same cluster shelf-pack +
/// halve-on-overflow ladder as the party-atlas layout, parameterized on
/// the window geometry.
fn layout_window(
    head: &mut ModelObject,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
    win: &HeadTimWindow,
    warnings: &mut Vec<String>,
) -> Result<(Vec<u8>, Vec<Vec<u16>>)> {
    let mut faces: Vec<(usize, usize, UvBox)> = Vec::new();
    let mut used: Vec<u8> = Vec::new();
    for (gi, g) in head.groups.iter().enumerate() {
        if !g.shape.is_textured() {
            continue;
        }
        for (pi, p) in g.prims.iter().enumerate() {
            faces.push((gi, pi, face_bbox(&p.uvs)));
            let pal = (p.cba & 0x3F) as u8;
            if !used.contains(&pal) {
                used.push(pal);
            }
        }
    }
    let mut window = vec![0u8; win.w * win.h];
    if faces.is_empty() {
        return Ok((window, Vec::new()));
    }
    used.sort_unstable();
    let (group_of, group_colors) = merge_palettes(&used, cluts, win.clut_rows, warnings)?;

    // Cluster faces by inflated-bbox overlap so shared texels pack once.
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let r = find(parent, parent[i]);
            parent[i] = r;
        }
        parent[i]
    }
    for i in 0..faces.len() {
        for j in (i + 1)..faces.len() {
            if boxes_touch(faces[i].2, faces[j].2) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut clusters: BTreeMap<usize, UvCluster> = BTreeMap::new();
    #[allow(clippy::needless_range_loop)]
    for i in 0..faces.len() {
        let root = find(&mut parent, i);
        let e = clusters.entry(root).or_insert((Vec::new(), faces[i].2));
        e.1 = union_bbox(e.1, faces[i].2);
        e.0.push(i);
    }
    let clusters: Vec<UvCluster> = clusters.into_values().collect();

    // Per-cluster scale ladder: shelf-pack tallest-first at full
    // resolution, and on overflow halve only the largest still-scalable
    // cluster - the party-atlas layout's global halve costs the whole
    // face when one side island is what overflowed.
    let mut scales: Vec<usize> = vec![1; clusters.len()];
    let mut placed: Vec<(usize, usize)> = vec![(0, 0); clusters.len()];
    'retry: loop {
        let dims = |ci: usize| {
            let bb = clusters[ci].1;
            (
                ((bb.2 - bb.0) as usize + 1).div_ceil(scales[ci]),
                ((bb.3 - bb.1) as usize + 1).div_ceil(scales[ci]),
            )
        };
        // Tallest-first order packs shelves tightly and deterministically.
        let mut order: Vec<usize> = (0..clusters.len()).collect();
        order.sort_by_key(|&ci| {
            let (w, h) = dims(ci);
            (std::cmp::Reverse(h), std::cmp::Reverse(w), ci)
        });
        let (mut x, mut y, mut shelf) = (0usize, 0usize, 0usize);
        let mut ok = true;
        for &ci in &order {
            let (w, h) = dims(ci);
            if w > win.w || {
                if x + w > win.w {
                    y += shelf;
                    x = 0;
                    shelf = 0;
                }
                y + h > win.h
            } {
                ok = false;
                break;
            }
            placed[ci] = (x, y);
            x += w;
            shelf = shelf.max(h);
        }
        if ok {
            break;
        }
        // Halve the largest scaled cluster that can still shrink.
        let grow = (0..clusters.len())
            .filter(|&ci| scales[ci] < 8)
            .max_by_key(|&ci| {
                let (w, h) = dims(ci);
                (w * h, std::cmp::Reverse(ci))
            });
        let Some(ci) = grow else {
            bail!("hero head texture does not fit the sibling TIM window");
        };
        scales[ci] *= 2;
        warnings.push(format!(
            "head island at 1/{} resolution (sibling TIM window)",
            scales[ci]
        ));
        continue 'retry;
    }

    for (ci, (members, bb)) in clusters.iter().enumerate() {
        let (dx, dy) = placed[ci];
        let scale = scales[ci];
        for &fi in members {
            let (gi, pi, _) = faces[fi];
            let p = &mut head.groups[gi].prims[pi];
            let src_pal = (p.cba & 0x3F) as usize;
            let group = group_of[&(src_pal as u8)];
            let colors = &group_colors[group];
            let fb = faces[fi].2;
            for sy in (fb.1 as usize..=fb.3 as usize).step_by(scale) {
                for sx in (fb.0 as usize..=fb.2 as usize).step_by(scale) {
                    let tx = dx + (sx - bb.0 as usize) / scale;
                    let ty = dy + (sy - bb.1 as usize) / scale;
                    if tx >= win.w || ty >= win.h {
                        continue;
                    }
                    let idx = if sx < width && sy < PAGE_HEIGHT {
                        indices[sy * width + sx] as usize
                    } else {
                        0
                    };
                    let color = cluts[src_pal][idx];
                    let new_idx = if idx == 0 || color == 0 {
                        0
                    } else {
                        match colors.iter().position(|&c| c == color) {
                            Some(pos) => pos + 1,
                            None => nearest_color(colors, color) + 1,
                        }
                    };
                    window[ty * win.w + tx] = new_idx as u8;
                }
            }
            for uv in p.uvs.iter_mut() {
                let nx = win.u_base + dx + (uv.0 as usize - bb.0 as usize) / scale;
                let ny = win.v_base + dy + (uv.1 as usize - bb.1 as usize) / scale;
                *uv = (nx.min(255) as u8, ny.min(255) as u8);
            }
            p.cba = (win.clut_row << 6) | (win.clut_col + group as u16);
            p.tsb = win.tsb;
        }
    }
    Ok((window, group_colors))
}

/// Rewrite one sibling head TIM in place inside the decoded §0 stream:
/// CLUT rows + pixel data replaced, geometry untouched.
pub(super) fn repaint_head_tim(
    sec0: &mut [u8],
    win: &HeadTimWindow,
    slot: &HeroizedSlot,
) -> Result<()> {
    let tim = &mut sec0[win.tim_offset..];
    let magic = u32::from_le_bytes(tim.get(0..4).unwrap_or(&[0; 4]).try_into().unwrap());
    let flags = u32::from_le_bytes(tim.get(4..8).unwrap_or(&[0; 4]).try_into().unwrap());
    if magic != 0x10 || flags & 0x8 == 0 {
        bail!("not a CLUT'd TIM (magic {magic:#x}, flags {flags:#x})");
    }
    let clut_len = u32::from_le_bytes(tim[8..12].try_into().unwrap()) as usize;
    let clut_w = u16::from_le_bytes(tim[16..18].try_into().unwrap()) as usize;
    let clut_h = u16::from_le_bytes(tim[18..20].try_into().unwrap()) as usize;
    if clut_w * clut_h != 16 * win.clut_rows {
        bail!(
            "sibling head CLUT holds {} colours, expected {}",
            clut_w * clut_h,
            16 * win.clut_rows
        );
    }
    let dst = tim
        .get_mut(20..20 + clut_w * clut_h * 2)
        .ok_or_else(|| anyhow::anyhow!("CLUT data out of range"))?;
    dst.fill(0);
    for (g, colors) in slot.palettes.iter().enumerate().take(win.clut_rows) {
        for (i, &c) in colors.iter().enumerate().take(15) {
            let at = (g * 16 + 1 + i) * 2;
            dst[at..at + 2].copy_from_slice(&c.to_le_bytes());
        }
    }
    let img = 8 + clut_len;
    let img_w = u16::from_le_bytes(
        tim.get(img + 8..img + 10)
            .ok_or_else(|| anyhow::anyhow!("image block out of range"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let img_h = u16::from_le_bytes(tim[img + 10..img + 12].try_into().unwrap()) as usize;
    if img_w * 4 != win.w || img_h != win.h {
        bail!(
            "sibling head image is {img_w}hw x {img_h}, expected {}x{}",
            win.w / 4,
            win.h
        );
    }
    let data = img + 12;
    let dst = tim
        .get_mut(data..data + img_w * 2 * img_h)
        .ok_or_else(|| anyhow::anyhow!("image data out of range"))?;
    for yy in 0..win.h {
        for xb in 0..win.w / 2 {
            let lo = slot.window[yy * win.w + xb * 2] & 0xF;
            let hi = slot.window[yy * win.w + xb * 2 + 1] & 0xF;
            dst[yy * (win.w / 2) + xb] = lo | (hi << 4);
        }
    }
    Ok(())
}

/// Rebuild the PROT 0639 pack entry with the three sibling members
/// replaced. The members' contiguous byte span is the budget; every
/// other member (and the offset table shape) stays byte-identical.
fn rebuild_pack(npc_pack: &[u8], slots: &BTreeMap<usize, &[u8]>) -> Result<Vec<u8>> {
    let body = rebuild_pack_body(&npc_pack[4..], slots)?;
    let mut out = npc_pack[..4].to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

/// [`rebuild_pack`] on a member-pack **body** (count word first, no
/// entry-head type word) - the shape a scene bundle's decoded TMD
/// section has. Output length always equals the input length.
pub(super) fn rebuild_pack_body(pack: &[u8], slots: &BTreeMap<usize, &[u8]>) -> Result<Vec<u8>> {
    let entries = parse_pack(pack)?;
    let members: Vec<usize> = slots.keys().copied().collect();
    // The replaced members must be one contiguous ascending run.
    for w in members.windows(2) {
        if w[1] != w[0] + 1 {
            bail!("replaced members {members:?} are not contiguous");
        }
    }
    let first = members[0];
    let last = *members.last().unwrap();
    let region_start = entries[first].byte_offset;
    let region_end = if last + 1 < entries.len() {
        entries[last + 1].byte_offset
    } else {
        entries[last].byte_offset + entries[last].size
    };
    let budget = region_end - region_start;
    let sizes: Vec<usize> = members
        .iter()
        .map(|m| slots[m].len().div_ceil(4) * 4)
        .collect();
    let need: usize = sizes.iter().sum();
    if need > budget {
        bail!("rebuilt members need {need} bytes, the pack span holds {budget}");
    }
    let mut out = pack.to_vec();
    let mut cursor = region_start;
    for (k, m) in members.iter().enumerate() {
        // Offset table word for member m at pack byte 4 + m*4.
        let word = (cursor / 4) as u32;
        out[4 + m * 4..4 + m * 4 + 4].copy_from_slice(&word.to_le_bytes());
        let bytes = slots[m];
        out[cursor..cursor + bytes.len()].copy_from_slice(bytes);
        // Zero the alignment pad.
        for b in out
            .iter_mut()
            .skip(cursor + bytes.len())
            .take(sizes[k] - bytes.len())
        {
            *b = 0;
        }
        cursor += sizes[k];
    }
    // Zero the residue up to the next untouched member.
    for b in out.iter_mut().skip(cursor).take(region_end - cursor) {
        *b = 0;
    }
    Ok(out)
}

/// Rebuild the PROT 0638 bundle entry with the repainted §0 TIM list.
/// Prefers a pure in-place §0 rewrite (offsets and meta untouched);
/// falls back to re-laying the three sections inside the entry when the
/// recompressed §0 outgrows its retail span.
fn rebuild_bundle(npc_bundle: &[u8], sec0: &[u8]) -> Result<Vec<u8>> {
    let container = parse_player_lzs(npc_bundle, 3)?;
    let d0 = &container.descriptors[0];
    if d0.size as usize != sec0.len() {
        bail!("§0 decoded size changed ({} vs {})", sec0.len(), d0.size);
    }
    let span0_start = d0.data_offset as usize;
    let span0_end = container.descriptors[1].data_offset as usize;
    let mut s0 = legaia_lzs::compress(sec0);
    if span0_start + s0.len() > span0_end {
        s0 = legaia_lzs::compress_optimal(sec0);
    }
    if span0_start + s0.len() <= span0_end {
        let mut out = npc_bundle.to_vec();
        out[span0_start..span0_start + s0.len()].copy_from_slice(&s0);
        out[span0_start + s0.len()..span0_end].fill(0);
        return Ok(out);
    }
    // Re-lay: §0 grows into the entry's tail slack; §1/§2 keep their raw
    // compressed byte spans, shifted.
    let entry_len = npc_bundle.len();
    let mut offs: Vec<usize> = container
        .descriptors
        .iter()
        .map(|d| d.data_offset as usize)
        .collect();
    offs.push(entry_len);
    let sec1_raw = npc_bundle[offs[1]..offs[2]].to_vec();
    // §2's raw span runs to the entry end (it includes the tail slack);
    // take only up to the slack so the re-lay can place it tighter.
    let sec2_raw = npc_bundle[offs[2]..offs[3]].to_vec();
    let mut out = npc_bundle[..span0_start].to_vec();
    out.extend_from_slice(&s0);
    let mut new_offs = [span0_start as u32, 0, 0];
    let mut cursor = out.len().div_ceil(4) * 4;
    out.resize(cursor, 0);
    new_offs[1] = cursor as u32;
    out.extend_from_slice(&sec1_raw);
    cursor = out.len().div_ceil(4) * 4;
    out.resize(cursor, 0);
    new_offs[2] = cursor as u32;
    out.extend_from_slice(&sec2_raw);
    if out.len() > entry_len {
        bail!(
            "rebuilt PROT 0638 is {} bytes, entry holds {entry_len}",
            out.len()
        );
    }
    out.resize(entry_len, 0);
    // Patch the three descriptor offsets (type/size words unchanged).
    for (i, off) in new_offs.iter().enumerate() {
        let p = 8 + i * 8 + 4;
        out[p..p + 4].copy_from_slice(&off.to_le_bytes());
    }
    Ok(out)
}

/// Per-sibling verification numbers for the disc-gated oracle.
#[derive(Debug, Clone)]
pub struct SlotReport {
    /// The sibling's monster id.
    pub monster_id: u16,
    /// The hero slot (0 Vahn / 1 Noa / 2 Gala) whose mesh it now wears.
    pub hero_slot: usize,
    /// Pack member index rebuilt.
    pub member: usize,
    /// Encoded TMD size (pre word-align).
    pub tmd_len: usize,
    /// Per-part vertex counts of the rebuilt member.
    pub part_verts: Vec<usize>,
}

/// Rebuild the nilboa scene so the three Delilas NPC field meshes wear
/// the mapped heroes' field models.
///
/// `mapping[slot]` = the sibling monster id character `slot` wears in
/// battle (the same array `fieldize_pack_npc` takes) - the hero shown
/// for sibling S is therefore the slot whose entry is S. `prot_0874`
/// MUST be the pre-fieldize entry bytes (see the module doc).
pub fn heroize_nilboa(
    npc_pack: &[u8],
    npc_bundle: &[u8],
    prot_0874: &[u8],
    mapping: [u16; 3],
) -> Result<(NivoraFieldPatch, Vec<SlotReport>)> {
    let mut warnings = Vec::new();
    let container = parse_player_lzs(npc_bundle, 3).context("PROT 0638 container")?;
    let mut sec0 = crate::decode(
        npc_bundle,
        &container.descriptors[0],
        crate::DecodeMode::Lzs,
    )
    .context("PROT 0638 TIM list")?;

    // Budget: the contiguous span of the three retail members.
    let pack_entries = parse_pack(&npc_pack[4..])?;
    let mut coords: Vec<(u16, usize, usize)> = Vec::new(); // (monster, hero slot, member)
    for (slot, &id) in mapping.iter().enumerate() {
        let (member, _) = fieldize::npc_coords(id)
            .ok_or_else(|| anyhow::anyhow!("monster id {id} has no nilboa coordinates"))?;
        coords.push((id, slot, member));
    }
    coords.sort_by_key(|c| c.2);
    let first = coords[0].2;
    let last = coords[2].2;
    let budget = pack_entries
        .get(last + 1)
        .map(|e| e.byte_offset)
        .unwrap_or_else(|| pack_entries[last].byte_offset + pack_entries[last].size)
        - pack_entries[first].byte_offset;

    // Detail ladder: full detail first, then progressively drop prims
    // under a size threshold until the three members fit the span.
    for decimate in [0.0f32, 1.5, 2.5, 3.5, 5.0, 7.0, 9.0, 12.0] {
        let mut trial_warnings = Vec::new();
        let mut slots = Vec::with_capacity(3);
        for &(id, hero_slot, _) in &coords {
            let hero = hero_slot_source(prot_0874, hero_slot)
                .with_context(|| format!("hero slot {hero_slot}"))?;
            let npc = npc_target(npc_pack, &sec0, npc_bundle, id)
                .with_context(|| format!("sibling {id}"))?;
            slots.push((
                heroize_slot(&hero, &npc, decimate, &mut trial_warnings)
                    .with_context(|| format!("bake hero {hero_slot} onto sibling {id}"))?,
                npc,
            ));
        }
        let need: usize = slots.iter().map(|(s, _)| s.tmd.len().div_ceil(4) * 4).sum();
        if need <= budget {
            if decimate > 0.0 {
                trial_warnings.push(format!(
                    "nilboa field detail reduced (min prim size {decimate})"
                ));
            }
            warnings.extend(trial_warnings);
            // Repaint the head TIMs now that this ladder rung is final.
            let mut reports = Vec::with_capacity(3);
            let mut replacements: BTreeMap<usize, &[u8]> = BTreeMap::new();
            for ((s, npc), &(id, hero_slot, member)) in slots.iter().zip(&coords) {
                repaint_head_tim(&mut sec0, &npc.head_tim, s)
                    .with_context(|| format!("repaint sibling {id} head TIM"))?;
                replacements.insert(member, s.tmd.as_slice());
                let tmd = legaia_tmd::parse(&s.tmd)?;
                let model = decode_model(&tmd, &s.tmd)?;
                reports.push(SlotReport {
                    monster_id: id,
                    hero_slot,
                    member,
                    tmd_len: s.tmd.len(),
                    part_verts: model.iter().map(|o| o.vertices.len()).collect(),
                });
            }
            let pack_entry = rebuild_pack(npc_pack, &replacements)?;
            let bundle_entry = rebuild_bundle(npc_bundle, &sec0)?;
            return Ok((
                NivoraFieldPatch {
                    pack_entry,
                    bundle_entry,
                    warnings,
                },
                reports,
            ));
        }
    }
    bail!("hero field meshes do not fit the nilboa pack span at any detail level (budget {budget})")
}
