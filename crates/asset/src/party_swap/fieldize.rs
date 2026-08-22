//! Field-side of the party swap: rebuild PROT 0874 so the party's
//! walking-around (field) models depict the Delilas siblings.
//!
//! The preferred source ([`fieldize_pack_npc`]) is the siblings' **own
//! field NPC meshes** from the nilboa duel scene ([`NPC_PACK_ENTRY`] /
//! [`NPC_BUNDLE_ENTRY`]): retail-authored 10-part chibi rigs (duel
//! costume, matching the battle forms) whose geometry fits the §0
//! budget at full detail. The battle-monster conversion
//! ([`fieldize_pack`]) survives as a fallback; its high-entropy baked
//! geometry only fits through the decimation ladder.
//!
//! The party field rig is a 12-group TMD per character (10 posed bones
//! plus 2 unposed equipment-template groups), driven by the shared
//! locomotion ANM bundle whose records decode to the same flat per-part
//! pose model as the battle streams - so a rest-pose bake retargets
//! either source: each source part bakes into the party bone's local
//! frame anchored on the retail part's rest **bbox** with a per-axis
//! span (the clips' translations dictate joint spacing, so matching
//! retail extents is what keeps limb chains connected under every
//! clip); the head keeps its proportions under a uniform fit.
//!
//! Texture: the field atlas gives each character an 80x128-texel window
//! of texpage 0x1D plus four 16-colour CLUT columns on row 478. Retail
//! field models are mostly flat-shaded (bodies are vertex-coloured;
//! only faces carry texture) and the NPC meshes share that split. The
//! converter keeps the **head** textured (its islands re-lay into the
//! atlas window, palettes union-merged to four) and converts every
//! other textured face to a flat prim whose colour samples the source
//! texture.

use super::*;
use crate::character_pack;
use crate::pack::extract_pack;
use crate::parse_player_lzs;
use crate::party_swap::playerize::{merge_palettes, monster_pool_texels, nearest_color};
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::ModelPrim;

/// PROT entry the field-form container lives in.
pub const PROT_ENTRY_INDEX: usize = 874;

/// Field bones per character (the locomotion clips' channel count).
pub const FIELD_BONES: usize = 10;

/// Groups per field character slot (bones + 2 equipment templates).
pub const FIELD_GROUPS: usize = 12;

/// Field atlas geometry: per-character U window width (texels), page
/// height, CLUT columns per character.
const ATLAS_WINDOW_W: usize = 80;
const ATLAS_WINDOW_H: usize = 128;
const ATLAS_COLS_PER_CHAR: usize = 4;
/// Every field-character prim authors texpage 0x1D, ABR 1.
const FIELD_TSB: u16 = 0x003D;
/// Field CLUT row.
const FIELD_CLUT_ROW: u16 = 478;

/// Canonical battle parts feeding each field bone role.
/// Field roles: 0 head, 1 torso, 2/3 armA up/lo, 4/5 armB up/lo,
/// 6/7 legA up/lo, 8/9 legB up/lo.
const FIELD_ROLE_SOURCES: [&[usize]; FIELD_BONES] = [
    &[0],      // head
    &[1, 2],   // torso + pelvis
    &[3],      // armA upper
    &[4, 5],   // armA fore + hand
    &[6],      // armB upper
    &[7, 8],   // armB fore + hand
    &[9],      // legA thigh
    &[10, 11], // legA shin + foot
    &[12],     // legB thigh
    &[13, 14], // legB shin + foot
];

/// Field role chain child (role space, see [`FIELD_ROLE_SOURCES`]):
/// torso -> head, each upper limb -> its lower limb; lowers and the
/// head are terminal.
pub(super) const FIELD_CHILD: [Option<usize>; FIELD_BONES] = [
    None,
    Some(0),
    Some(3),
    None,
    Some(5),
    None,
    Some(7),
    None,
    Some(9),
    None,
];

/// Field role chain parent (chain-internal edges only).
pub(super) const FIELD_PARENT: [Option<usize>; FIELD_BONES] = [
    Some(1),
    None,
    None,
    Some(2),
    None,
    Some(4),
    None,
    Some(6),
    None,
    Some(8),
];

/// Derived anatomy of one retail field rig: `bone_for_role[r]` = the
/// field bone index playing canonical role `r` (see
/// [`FIELD_ROLE_SOURCES`]). Derived from rest-pose world centroids per
/// slot - the group order differs between characters (Vahn's arm pair
/// orders upper/lower differently from Noa's).
pub(super) fn derive_field_roles(
    model: &[ModelObject],
    rest: &[PartPose],
) -> Result<[usize; FIELD_BONES]> {
    if model.len() < FIELD_BONES || rest.len() < FIELD_BONES {
        bail!(
            "field rig has {} groups / {} channels, expected at least {FIELD_BONES}",
            model.len(),
            rest.len()
        );
    }
    #[derive(Clone, Copy)]
    struct Info {
        idx: usize,
        cx: f32,
        cy: f32,
        max_y: f32,
    }
    let mut infos = Vec::with_capacity(FIELD_BONES);
    for (i, o) in model.iter().take(FIELD_BONES).enumerate() {
        let m = rot_matrix(&rest[i]);
        let mut c = (0f32, 0f32);
        let mut max_y = f32::MIN;
        for v in &o.vertices {
            let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
            c.0 += w[0] + rest[i].tx as f32;
            c.1 += w[1] + rest[i].ty as f32;
            max_y = max_y.max(w[1] + rest[i].ty as f32);
        }
        let n = o.vertices.len().max(1) as f32;
        infos.push(Info {
            idx: i,
            cx: c.0 / n,
            cy: c.1 / n,
            max_y,
        });
    }
    // Head = highest centroid (y-down: most negative).
    infos.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap());
    let head = infos[0].idx;
    // Torso = the most x-central of the rest.
    let torso = infos[1..]
        .iter()
        .min_by(|a, b| a.cx.abs().partial_cmp(&b.cx.abs()).unwrap())
        .unwrap()
        .idx;
    // Legs = the four remaining bones whose geometry reaches lowest
    // (largest max_y); arms = the other four.
    let mut rest_bones: Vec<Info> = infos
        .iter()
        .filter(|i| i.idx != head && i.idx != torso)
        .copied()
        .collect();
    rest_bones.sort_by(|a, b| b.max_y.partial_cmp(&a.max_y).unwrap());
    let (legs, arms) = rest_bones.split_at(4);
    let side = |set: &[Info], neg: bool| -> Vec<Info> {
        let mut v: Vec<Info> = set
            .iter()
            .filter(|i| (i.cx < 0.0) == neg)
            .copied()
            .collect();
        // Upper = higher centroid (more negative y).
        v.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap());
        v
    };
    let (arm_l, arm_r) = (side(arms, true), side(arms, false));
    let (leg_l, leg_r) = (side(legs, true), side(legs, false));
    if arm_l.len() != 2 || arm_r.len() != 2 || leg_l.len() != 2 || leg_r.len() != 2 {
        bail!(
            "field rig chains did not split 2/2/2/2 (arms {}/{}, legs {}/{})",
            arm_l.len(),
            arm_r.len(),
            leg_l.len(),
            leg_r.len()
        );
    }
    Ok([
        head,
        torso,
        arm_l[0].idx,
        arm_l[1].idx,
        arm_r[0].idx,
        arm_r[1].idx,
        leg_l[0].idx,
        leg_l[1].idx,
        leg_r[0].idx,
        leg_r[1].idx,
    ])
}

/// Rest-pose world bbox centre + per-axis extents of a set of posed
/// parts. The bbox centre (not the vertex average, which vertex density
/// skews) is the anchor: with the per-axis span, the baked part's bbox
/// coincides with the target part's, so limb chains meet exactly where
/// retail's do.
pub(super) fn group_world_stats(
    parts: &[(&ModelObject, &PartPose)],
) -> ((f32, f32, f32), [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for (o, p) in parts {
        let m = rot_matrix(p);
        for v in &o.vertices {
            let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
            let w = [w[0] + p.tx as f32, w[1] + p.ty as f32, w[2] + p.tz as f32];
            for k in 0..3 {
                min[k] = min[k].min(w[k]);
                max[k] = max[k].max(w[k]);
            }
        }
    }
    if min[0] > max[0] {
        (min, max) = ([0.0; 3], [0.0; 3]);
    }
    let center = (
        (min[0] + max[0]) / 2.0,
        (min[1] + max[1]) / 2.0,
        (min[2] + max[2]) / 2.0,
    );
    let ext = [
        (max[0] - min[0]).max(1.0),
        (max[1] - min[1]).max(1.0),
        (max[2] - min[2]).max(1.0),
    ];
    (center, ext)
}

/// Drop prims whose baked extent falls under `threshold` (invisible at
/// the chibi field scale) and prune the vertices nothing references.
pub(super) fn decimate_object(o: &mut ModelObject, threshold: f32) {
    if threshold <= 0.0 {
        return;
    }
    for g in o.groups.iter_mut() {
        g.prims.retain(|p| {
            let mut min = [i16::MAX; 3];
            let mut max = [i16::MIN; 3];
            for &vi in &p.vertices {
                let Some(v) = o.vertices.get(vi as usize) else {
                    return true;
                };
                for k in 0..3 {
                    min[k] = min[k].min(v[k]);
                    max[k] = max[k].max(v[k]);
                }
            }
            let d = (0..3)
                .map(|k| (max[k] - min[k]) as f32)
                .fold(0f32, |a, b| a.max(b));
            d >= threshold
        });
    }
    o.groups.retain(|g| !g.prims.is_empty());
    // Prune unreferenced vertices.
    let mut used = vec![false; o.vertices.len()];
    for g in &o.groups {
        for p in &g.prims {
            for &vi in &p.vertices {
                if let Some(u) = used.get_mut(vi as usize) {
                    *u = true;
                }
            }
        }
    }
    let mut remap = vec![0u16; o.vertices.len()];
    let mut kept = Vec::new();
    for (i, v) in o.vertices.iter().enumerate() {
        if used[i] {
            remap[i] = kept.len() as u16;
            kept.push(*v);
        }
    }
    o.vertices = kept;
    for g in o.groups.iter_mut() {
        for p in g.prims.iter_mut() {
            for vi in p.vertices.iter_mut() {
                *vi = remap[*vi as usize];
            }
        }
    }
}

/// Convert a textured prim into an untextured gouraud prim whose corner
/// colours sample the source page (colour = texel through its palette,
/// modulated by the prim's own colour, `0x80` = neutral).
pub(super) fn flatten_prim(
    p: &ModelPrim,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
) -> ModelPrim {
    let sample = |uv: (u8, u8), k: usize| -> [u8; 3] {
        let (u, v) = (uv.0 as usize, uv.1 as usize);
        let idx = if u < width && v < PAGE_HEIGHT {
            indices[v * width + u] as usize
        } else {
            0
        };
        let pal = (p.cba & 0x3F) as usize;
        let c = cluts.get(pal).map_or(0, |cl| cl[idx]);
        let scale5 = |x: u16| ((x << 3) | (x >> 2)) as u32;
        let (r, g, b) = (
            scale5(c & 0x1F),
            scale5((c >> 5) & 0x1F),
            scale5((c >> 10) & 0x1F),
        );
        let mc = if p.colors.len() == p.vertices.len() {
            p.colors[k]
        } else {
            p.colors.first().copied().unwrap_or([0x80; 3])
        };
        [
            ((r * mc[0] as u32) / 0x80).min(255) as u8,
            ((g * mc[1] as u32) / 0x80).min(255) as u8,
            ((b * mc[2] as u32) / 0x80).min(255) as u8,
        ]
    };
    // One flat colour (the corner average): F3/F4 store a single colour
    // word, half the size of a gouraud prim - the field budget is tight
    // and the sprites are tiny on screen.
    let mut acc = [0u32; 3];
    for (k, &uv) in p.uvs.iter().enumerate() {
        let c = sample(uv, k);
        for (a, ch) in acc.iter_mut().zip(c) {
            *a += ch as u32;
        }
    }
    let n = p.uvs.len().max(1) as u32;
    let avg = [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8];
    ModelPrim {
        vertices: p.vertices.clone(),
        uvs: Vec::new(),
        cba: 0,
        tsb: 0,
        colors: vec![avg],
    }
}

/// PROT entry of the nilboa (Nivora Ravine duel scene) TMD pack that
/// carries the Delilas siblings' own field NPC meshes.
pub const NPC_PACK_ENTRY: usize = 639;

/// PROT entry of the nilboa bundle carrying the scene ANM records (the
/// siblings' idle rest poses) and the TIM list their head textures live
/// in.
pub const NPC_BUNDLE_ENTRY: usize = 638;

/// nilboa coordinates per Delilas monster id: `(pack member, idle ANM
/// record)`. The pack members are 10-object field rigs (duel costume,
/// matching the battle forms); the idle records are the placements'
/// anim bytes minus one.
pub(super) fn npc_coords(monster_id: u16) -> Option<(usize, usize)> {
    match monster_id {
        162 => Some((106, 55)), // Gi
        163 => Some((107, 68)), // Che
        164 => Some((108, 78)), // Lu
        _ => None,
    }
}

/// Angle (degrees) of the relative rotation between two rest channels -
/// the joint bend a terminal part carries through the pivot bake.
fn rel_bend_deg(a: &PartPose, b: &PartPose) -> f32 {
    let (ma, mb) = (rot_matrix(a), rot_matrix(b));
    let mut tr = 0.0f32;
    for i in 0..3 {
        for j in 0..3 {
            tr += ma[j][i] * mb[j][i]; // trace(Ma^T * Mb)
        }
    }
    ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Sagittal mirror of one rest channel (mirror plane x = 0): under the
/// `Rz*Ry*Rx` convention, `M*R(rx,ry,rz)*M = R(rx,-ry,-rz)` for
/// `M = diag(-1,1,1)`, and the pivot's x negates.
fn mirror_channel(p: &PartPose) -> PartPose {
    let neg = |r: u16| (4096 - (r & 0xFFF) as i32) as u16 & 0xFFF;
    PartPose {
        tx: -p.tx,
        ty: p.ty,
        tz: p.tz,
        rx: p.rx & 0xFFF,
        ry: neg(p.ry),
        rz: neg(p.rz),
    }
}

/// Straighten an asymmetric standing rest: some NPC placements rest in
/// a scene-flair stance (Lu's nilboa taunt props one leg at a ~71
/// degree knee bend - her ONLY static record), and a terminal part's
/// relative bend rides through the pivot bake verbatim, so the walking
/// model limps. When the two legs' knee bends disagree badly, mirror
/// the straighter leg's channels onto the bent one (10-part scene rig:
/// legs at parts 6/7 and 8/9). The head channel (part 0) gets its ROLL
/// zeroed the same way: the NPC heads are level-authored (the channel
/// is the stance's gaze), so a lateral `rz` term is scene flair - Lu's
/// taunt cocks her head 4.2 degrees - that would ride into every field
/// pose.
pub(super) fn symmetrize_rest_legs(rest: &mut [PartPose]) {
    if rest.len() < FIELD_BONES {
        return;
    }
    level_head_roll(&mut rest[0]);
    let bend_a = rel_bend_deg(&rest[6], &rest[7]);
    let bend_b = rel_bend_deg(&rest[8], &rest[9]);
    if (bend_a - bend_b).abs() < 20.0 {
        return;
    }
    let (from, to) = if bend_a < bend_b { (6, 8) } else { (8, 6) };
    rest[to] = mirror_channel(&rest[from]);
    rest[to + 1] = mirror_channel(&rest[from + 1]);
}

/// Zero the roll (canonical `rz`) of one rest channel, preserving pitch
/// and yaw. Works on the MATRIX, not the raw fields: retail channels
/// may encode a small pose as a near-180 Euler composite (Gi's head is
/// `(2144, 2016, 2032)` = an 8-degree nod), where zeroing the raw `rz`
/// would wreck the pose. Canonical ZYX extraction for `R = Rz*Ry*Rx`:
/// `rz = atan2(m10, m00)`, `ry = asin(-m20)`, `rx = atan2(m21, m22)`.
fn level_head_roll(p: &mut PartPose) {
    let m = rot_matrix(p);
    let to_u16 = |rad: f32| {
        (((rad / std::f32::consts::TAU * 4096.0).round() as i32).rem_euclid(4096)) as u16
    };
    let rx = m[2][1].atan2(m[2][2]);
    let ry = (-m[2][0]).clamp(-1.0, 1.0).asin();
    p.rx = to_u16(rx);
    p.ry = to_u16(ry);
    p.rz = 0;
}

/// Source from the sibling's own field NPC mesh in the nilboa scene:
/// already field-scale, flat-colored body + textured head - retail
/// authoring that fits the §0 budget with no decimation. The NPC rig is
/// a 10-part humanoid, so roles map 1:1 through the same centroid
/// anatomy used for the retail party rig.
fn npc_slot_source(npc_pack: &[u8], npc_bundle: &[u8], monster_id: u16) -> Result<SlotSource> {
    let (member, idle_rec) = npc_coords(monster_id)
        .ok_or_else(|| anyhow::anyhow!("monster id {monster_id} has no field NPC coordinates"))?;

    // The pack entry is a raw stream: [u32 (type<<24)|size][asset::pack].
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
    let pack = &npc_pack[4..];
    let entries = crate::pack::parse_pack(pack)?;
    let e = entries
        .get(member)
        .ok_or_else(|| anyhow::anyhow!("NPC pack member {member} missing"))?;
    let tmd_bytes = pack
        .get(e.byte_offset..e.byte_offset + e.size)
        .ok_or_else(|| anyhow::anyhow!("NPC pack member {member} out of range"))?;
    let tmd = legaia_tmd::parse(tmd_bytes).context("NPC TMD")?;
    let mut model = decode_model(&tmd, tmd_bytes)?;
    if model.len() != FIELD_BONES {
        bail!("NPC mesh has {} parts, expected {FIELD_BONES}", model.len());
    }

    // Rest pose from the scene ANM bundle's idle record.
    let bundle = crate::player_anm::find_in_entry(npc_bundle, 5)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("NPC bundle entry carries no ANM bundle"))?;
    let idle = bundle
        .record_to_monster_animation(idle_rec)
        .ok_or_else(|| anyhow::anyhow!("NPC idle record {idle_rec} missing"))?;
    if idle.part_count != FIELD_BONES {
        bail!(
            "NPC idle poses {} bones, expected {FIELD_BONES}",
            idle.part_count
        );
    }
    let mut rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("NPC idle has no frames"))?
        .clone();
    symmetrize_rest_legs(&mut rest);

    // Texel space: paint the bundle's TIM list into a virtual VRAM and
    // sample the one 4bpp texpage the prims reference; CLUTs re-key to
    // local palette ids (cba & 0x3F indexes `cluts`, the pipeline's
    // convention).
    let container = parse_player_lzs(npc_bundle, 3).context("NPC bundle container")?;
    let tims_payload = crate::decode(
        npc_bundle,
        &container.descriptors[0],
        crate::DecodeMode::Lzs,
    )
    .context("NPC bundle TIM list")?;
    let mut vram = vec![0u16; 1024 * 512];
    let mut off = 0usize;
    while off + 8 <= tims_payload.len() {
        if u32::from_le_bytes(tims_payload[off..off + 4].try_into().unwrap()) == 0x10
            && let Ok(tim) = legaia_tim::parse(&tims_payload[off..])
        {
            // Multi-row CLUT blocks flatten SIDE BY SIDE at upload: a
            // 16x2 block at (x, y) lands as two 16-colour palettes at
            // (x, y) and (x+16, y) - which is exactly how the siblings'
            // prim CBAs address their second palettes (Gi's block at
            // (128,481) h2 backs cba columns 8 AND 9). Painting the rows
            // stacked left every second palette zero - transparent face
            // prims.
            if let Some(c) = tim.clut.as_ref() {
                for row in 0..c.h as usize {
                    let row_bytes: Vec<u8> = c.entries
                        [row * c.w as usize..(row + 1) * c.w as usize]
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
        off += 4;
    }

    // Collect the textured prims' page + palettes; re-key cba.
    // Page identity is tsb bits 0..4 (x tile + y half); bits 5..6 are the
    // ABR semi-transparency rate and vary per prim on one page.
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
        more => bail!("NPC mesh references {} texpages, expected 1", more.len()),
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

    // Roles from the NPC's own rest anatomy, mapped 1:1.
    let npc_roles = derive_field_roles(&model, &rest)?;
    Ok(SlotSource {
        role_sources: npc_roles.map(|b| vec![b]),
        model,
        rest,
        cluts,
        indices,
        width,
    })
}

/// Paint one TIM block (16-bit framebuffer units) into a 1024x512 VRAM
/// image.
pub(super) fn paint_vram(vram: &mut [u16], fb_x: u16, fb_y: u16, w: u16, h: u16, data: &[u8]) {
    for row in 0..h as usize {
        for col in 0..w as usize {
            let src = (row * w as usize + col) * 2;
            let (x, y) = (fb_x as usize + col, fb_y as usize + row);
            if src + 2 <= data.len() && x < 1024 && y < 512 {
                vram[y * 1024 + x] = u16::from_le_bytes(data[src..src + 2].try_into().unwrap());
            }
        }
    }
}

/// A tiny flat-colored box spanning `part`'s local bbox: the low-poly
/// equipment-template stand-in (mimics retail's ~16-vert world-map
/// variants; see the template block in `fieldize_slot`).
/// Average colour of a part's flat prims; a fully-textured part (the
/// head) falls back to the source palettes' average - the dominant
/// cloth/hair tone.
fn template_fill_color(part: &ModelObject, cluts: &[[u16; 16]]) -> [u8; 3] {
    let mut acc = [0u32; 3];
    let mut n = 0u32;
    for g in &part.groups {
        if g.shape.is_textured() {
            continue;
        }
        for p in &g.prims {
            for c in &p.colors {
                for k in 0..3 {
                    acc[k] += c[k] as u32;
                }
                n += 1;
            }
        }
    }
    if n == 0 {
        let s5 = |x: u16| ((x << 3) | (x >> 2)) as u32;
        for pal in cluts {
            for &c in pal.iter().skip(1) {
                if c != 0 {
                    acc[0] += s5(c & 0x1F);
                    acc[1] += s5((c >> 5) & 0x1F);
                    acc[2] += s5((c >> 10) & 0x1F);
                    n += 1;
                }
            }
        }
    }
    let n = n.max(1);
    [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]
}

fn template_box(
    part: &ModelObject,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
) -> ModelObject {
    let _ = (indices, width);
    let color = template_fill_color(part, cluts);

    // Fixed-tiny geometry, tucked just BELOW the part's local origin
    // (y-down: positive y). Unposed in the field the box sits under the
    // ground plane at the actor origin; posed by the world-map toggle it
    // is a small blob at the joint. Never size it to the part's bbox - a
    // head-sized unposed box parks itself over the walker's legs.
    let _ = part;
    let (lo, hi) = ([-4i16, 1, -4], [4i16, 9, 4]);
    let vertices: Vec<[i16; 3]> = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        })
        .collect();
    let quads: [[u16; 4]; 6] = [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 4, 5],
        [2, 3, 6, 7],
        [0, 2, 4, 6],
        [1, 3, 5, 7],
    ];
    let prims = quads
        .iter()
        .map(|q| ModelPrim {
            vertices: q.to_vec(),
            uvs: Vec::new(),
            cba: 0,
            tsb: 0,
            colors: vec![color],
        })
        .collect();
    ModelObject {
        vertices,
        groups: vec![ModelGroup {
            shape: PacketShape::F4,
            semi_transparent: false,
            prims,
        }],
        scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
    }
}

/// The rebuilt PROT 0874 entry.
#[derive(Debug, Clone)]
pub struct FieldizedPack {
    pub entry: Vec<u8>,
    pub warnings: Vec<String>,
}

/// One character's field conversion: the sibling's model on the field
/// rig + its atlas window content.
struct FieldSlot {
    tmd: Vec<u8>,
    /// 4bpp texel indices, `ATLAS_WINDOW_W x ATLAS_WINDOW_H`.
    window: Vec<u8>,
    /// Up to four 16-colour palettes.
    palettes: Vec<Vec<u16>>,
}

/// A field-conversion source: a posed multi-part model plus the texel
/// space its textured prims sample (`cba & 0x3F` indexes `cluts`, UVs
/// index `indices` as a `width x PAGE_HEIGHT` 4bpp page). Built from
/// either the battle monster archive or the siblings' own field NPC
/// meshes ([`npc_slot_source`]).
struct SlotSource {
    model: Vec<ModelObject>,
    rest: Vec<PartPose>,
    /// Canonical field role -> source part indices.
    role_sources: [Vec<usize>; FIELD_BONES],
    cluts: Vec<[u16; 16]>,
    indices: Vec<u8>,
    width: usize,
}

/// Source from the monster archive (battle model, 15 canonical parts
/// merged onto the 10 field roles).
fn monster_slot_source(archive_entry: &[u8], source_id: u16) -> Result<SlotSource> {
    let mesh = monster_archive::mesh(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let src_tmd = legaia_tmd::parse(mesh.tmd_bytes())?;
    let model = decode_model(&src_tmd, mesh.tmd_bytes())?;
    if model.len() != CANONICAL_PARTS {
        bail!("monster id {source_id} has {} parts", model.len());
    }
    let pool = mesh
        .texture_pool_bytes()
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no texture pool"))?;
    let (cluts, indices, width) = monster_pool_texels(pool)?;
    let src_idle = monster_archive::idle_animation(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle"))?;
    let rest = src_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle empty"))?
        .clone();
    Ok(SlotSource {
        model,
        rest,
        role_sources: FIELD_ROLE_SOURCES.map(|s| s.to_vec()),
        cluts,
        indices,
        width,
    })
}

/// Close a part's OPEN boundary loops with flat fan fills. The NPC
/// meshes leave genuine openings (the hair shell's underside, regions
/// the fixed scene camera never exposed) that read as missing polygons
/// from the field camera's free angles - no winding trick can draw a
/// polygon that does not exist. A boundary edge is an undirected
/// triangle edge used exactly once; each closed loop of them gains a
/// centroid vertex + an F3 fan in the part's fill colour. Winding is
/// arbitrary - the head is double-sided afterwards.
fn seal_boundaries(o: &mut ModelObject, color: [u8; 3]) {
    use std::collections::{BTreeMap, BTreeSet};
    let key = |a: u16, b: u16| (a.min(b), a.max(b));
    let mut count: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    for g in &o.groups {
        for p in &g.prims {
            let v = &p.vertices;
            let tris: Vec<[u16; 3]> = match v.len() {
                3 => vec![[v[0], v[1], v[2]]],
                4 => vec![[v[0], v[1], v[2]], [v[1], v[3], v[2]]],
                _ => continue,
            };
            for t in tris {
                for e in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *count.entry(key(e.0, e.1)).or_default() += 1;
                }
            }
        }
    }
    let boundary: Vec<(u16, u16)> = count
        .iter()
        .filter(|&(_, &c)| c == 1)
        .map(|(&e, _)| e)
        .collect();
    if boundary.is_empty() {
        return;
    }
    let mut adj: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for &(a, b) in &boundary {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    let mut used: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut loops: Vec<Vec<u16>> = Vec::new();
    for &(a0, b0) in &boundary {
        if used.contains(&key(a0, b0)) {
            continue;
        }
        used.insert(key(a0, b0));
        let mut path = vec![a0, b0];
        loop {
            let cur = *path.last().unwrap();
            let prev = path[path.len() - 2];
            let next = adj[&cur]
                .iter()
                .copied()
                .find(|&n| n != prev && !used.contains(&key(cur, n)));
            let Some(next) = next else { break };
            used.insert(key(cur, next));
            if next == path[0] {
                loops.push(path.clone());
                break;
            }
            path.push(next);
        }
    }
    for lv in loops {
        if lv.len() < 3 {
            continue;
        }
        let mut c = [0f32; 3];
        for &vi in &lv {
            for (k, ck) in c.iter_mut().enumerate() {
                *ck += o.vertices[vi as usize][k] as f32;
            }
        }
        let n = lv.len() as f32;
        let ci = o.vertices.len() as u16;
        o.vertices.push([
            (c[0] / n).round() as i16,
            (c[1] / n).round() as i16,
            (c[2] / n).round() as i16,
        ]);
        let prims: Vec<ModelPrim> = (0..lv.len())
            .map(|w| ModelPrim {
                vertices: vec![ci, lv[w], lv[(w + 1) % lv.len()]],
                uvs: Vec::new(),
                cba: 0,
                tsb: 0,
                colors: vec![color],
            })
            .collect();
        o.groups.push(ModelGroup {
            shape: PacketShape::F3,
            semi_transparent: false,
            prims,
        });
    }
}

/// Emit a winding-reversed twin of every prim: the retail renderer's
/// NCLIP winding cull drops screen-clockwise faces, and the NPC source
/// meshes are authored for fixed-camera scenes - not watertight from
/// the free angles a walking player model is seen from (culled side
/// faces read as "missing polygons"). A reversed twin guarantees one
/// of the pair survives the cull from every angle. Reversal = swap the
/// middle two entries: tri `[a,b,c] -> [a,c,b]`; quad `[a,b,c,d]`
/// (Z-order, tris (0,1,2)+(1,3,2)) -> `[a,c,b,d]`.
fn double_side(o: &mut ModelObject) {
    // A prim whose exact reversed twin is already authored needs no
    // duplicate.
    let existing: std::collections::BTreeSet<Vec<u16>> = o
        .groups
        .iter()
        .flat_map(|g| g.prims.iter().map(|p| p.vertices.clone()))
        .collect();
    for g in o.groups.iter_mut() {
        let mut extra: Vec<ModelPrim> = Vec::new();
        for p in &g.prims {
            let mut q = p.clone();
            if q.vertices.len() >= 3 {
                q.vertices.swap(1, 2);
            }
            if q.uvs.len() >= 3 {
                q.uvs.swap(1, 2);
            }
            if q.colors.len() >= 3 {
                q.colors.swap(1, 2);
            }
            if !existing.contains(&q.vertices) {
                extra.push(q);
            }
        }
        g.prims.append(&mut extra);
    }
}

/// How much of the rig gets the reversed-twin treatment - the doubled
/// geometry must still fit the container's pinned decoded budget, so a
/// ladder tries progressively smaller scopes (the head is where a
/// culled hole is most visible: the textured face).
#[derive(Clone, Copy, PartialEq, Eq)]
enum DoubleSideScope {
    All,
    HeadTorso,
    Head,
    None,
}

fn fieldize_slot(
    pack: &character_pack::CharacterPack,
    anm: &crate::player_anm::PlayerAnmBundle,
    slot: usize,
    source: &SlotSource,
    decimate: f32,
    double_sided: DoubleSideScope,
    warnings: &mut Vec<String>,
) -> Result<FieldSlot> {
    // Retail field rig + rest pose.
    let cs = pack
        .slot(slot)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot} missing"))?;
    let field_tmd = legaia_tmd::parse(&cs.tmd_bytes).context("field TMD")?;
    let field_model = decode_model(&field_tmd, &cs.tmd_bytes)?;
    let idle_rec =
        character_pack::locomotion_record_index(slot, character_pack::LOCOMOTION_IDLE_SLOT);
    let idle = anm
        .record_to_monster_animation(idle_rec)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot}: idle clip missing"))?;
    let field_rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("field idle has no frames"))?;
    let roles = derive_field_roles(&field_model, field_rest)?;

    let (src_model, src_rest) = (&source.model, &source.rest);
    let (cluts, indices, width) = (&source.cluts, &source.indices, source.width);

    // Per field bone: bake the mapped canonical parts, anchored at the
    // retail field part's centroid + extent.
    let mut bones: Vec<ModelObject> = Vec::with_capacity(FIELD_GROUPS);
    for _ in 0..FIELD_GROUPS {
        bones.push(ModelObject {
            vertices: Vec::new(),
            groups: Vec::new(),
            scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
        });
    }
    // Pivot-anchored bake (see `party_swap::bake_object_pivot`): each
    // role's geometry is expressed relative to its source bone's rest
    // pivot, re-aimed into the retail field bone's rest frame, and
    // scaled axially so the far end lands on the retail child joint -
    // the locomotion clips rotate each bone about its pivot, so this is
    // what keeps the walking chains (and the neck) closed. The radial
    // scale is the uniform whole-rig height ratio, so the sibling's own
    // proportions survive.
    let radial = {
        let dst_parts: Vec<(&ModelObject, &PartPose)> = roles
            .iter()
            .map(|&b| (&field_model[b], &field_rest[b]))
            .collect();
        let src_parts: Vec<(&ModelObject, &PartPose)> = source
            .role_sources
            .iter()
            .flatten()
            .map(|&c| (&src_model[c], &src_rest[c]))
            .collect();
        let (_, e_dst) = group_world_stats(&dst_parts);
        let (_, e_src) = group_world_stats(&src_parts);
        (e_dst[1] / e_src[1]).clamp(0.25, 4.0)
    };
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_role_pivots: Vec<[f32; 3]> = source
        .role_sources
        .iter()
        .map(|s| pivot_of(&src_rest[s[0]]))
        .collect();
    let dst_role_pivots: Vec<[f32; 3]> = roles.iter().map(|&b| pivot_of(&field_rest[b])).collect();
    let src_frames = bone_frames(&src_role_pivots, &FIELD_CHILD, &FIELD_PARENT);
    let dst_frames = bone_frames(&dst_role_pivots, &FIELD_CHILD, &FIELD_PARENT);
    for (role, sources) in source.role_sources.iter().enumerate() {
        let bone = roles[role];
        let dst_pose = field_rest[bone];
        let pb = pivot_bake_params(&src_frames[role], &dst_frames[role], radial);
        for &c in sources.iter() {
            let mut part = src_model[c].clone();
            // Flatten every textured prim except on the head.
            if role != 0 {
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
                        *p = flatten_prim(p, cluts, indices, width);
                    }
                    g.shape = new_shape;
                    g.semi_transparent = false;
                }
            }
            bake_object_pivot(
                &mut part,
                &src_rest[c],
                src_role_pivots[role],
                &dst_pose,
                &pb,
            )
            .with_context(|| format!("field bake role {role} canonical {c}"))?;
            if role == 0 {
                // Seat the head on the neck (near edge along the bone
                // axis lands where the retail head's sat).
                seat_terminal_axial(&mut part, &field_model[bone], &dst_pose, pb.x_dst)?;
            }
            compact_object(&mut part);
            merge_object(&mut bones[bone], &part);
        }
    }
    for b in bones.iter_mut() {
        compact_object(b);
        decimate_object(b, decimate);
    }

    // Head texture: re-lay its islands into the 80x128 atlas window.
    // The NPC source authors some face prims semi-transparent (scene
    // ABR context); in the party atlas context ABE blends them against
    // the field background - a see-through face. Retail field heads are
    // opaque; clear the flag.
    let head_bone = roles[0];
    for g in bones[head_bone].groups.iter_mut() {
        g.semi_transparent = false;
    }
    // Close the head's real openings (hair underside) before the UV
    // relayout - the fills are flat prims the layout never touches.
    let fill = template_fill_color(&bones[head_bone], cluts);
    seal_boundaries(&mut bones[head_bone], fill);
    let window = layout_head_window(&mut bones[head_bone], cluts, indices, width, slot, warnings)?;

    // Equipment templates (groups 10/11). Retail's are LOW-POLY
    // world-map variants of group {0 Vahn, 3 Noa, 5 Gala}: the world-map
    // walker's equip toggle (FUN_8001EBEC) copies one of them over that
    // group, while the FIELD renderer draws all 12 objects - the
    // templates render unposed at the actor origin, so they must be
    // SMALL like retail's (16-vert bits that hide at the feet). The
    // first ship made them byte-copies of the full patched group, which
    // drew a second unposed HEAD riding the walker on the Vahn slot
    // (whose patched group is the head). Emit a tiny flat-colored box
    // spanning the patched group's local bbox instead: harmless in the
    // field, a plausible low-poly stand-in on the world map.
    const PATCHED_GROUP: [usize; 3] = [0, 3, 5];
    let patched = PATCHED_GROUP.get(slot).copied().unwrap_or(0);
    let template = template_box(&bones[patched], cluts, indices, width);
    bones[10] = template.clone();
    bones[11] = template;

    let ds_bones: Vec<usize> = match double_sided {
        DoubleSideScope::All => roles.to_vec(),
        DoubleSideScope::HeadTorso => vec![roles[0], roles[1]],
        DoubleSideScope::Head => vec![roles[0]],
        DoubleSideScope::None => Vec::new(),
    };
    for b in ds_bones {
        double_side(&mut bones[b]);
    }

    let tmd = encode(&bones).context("encode field TMD")?;
    Ok(FieldSlot {
        tmd,
        window: window.0,
        palettes: window.1,
    })
}

/// Pack the head part's texture islands into the character's atlas
/// window; rewrites the head prims' UV/CBA/TSB in place. Returns the
/// window texels + palettes.
#[allow(clippy::type_complexity)]
fn layout_head_window(
    head: &mut ModelObject,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
    slot: usize,
    warnings: &mut Vec<String>,
) -> Result<(Vec<u8>, Vec<Vec<u16>>)> {
    let u_base = slot * ATLAS_WINDOW_W;
    let col_base = (slot * ATLAS_COLS_PER_CHAR) as u16;

    // Collect the head's textured faces + their palettes.
    let mut faces: Vec<(usize, usize, (u8, u8, u8, u8))> = Vec::new();
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
    let mut window = vec![0u8; ATLAS_WINDOW_W * ATLAS_WINDOW_H];
    if faces.is_empty() {
        return Ok((window, Vec::new()));
    }
    used.sort_unstable();
    let (group_of, group_colors) = merge_palettes(&used, cluts, ATLAS_COLS_PER_CHAR, warnings)?;

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
    let mut clusters: BTreeMap<usize, (Vec<usize>, (u8, u8, u8, u8))> = BTreeMap::new();
    #[allow(clippy::needless_range_loop)]
    for i in 0..faces.len() {
        let root = find(&mut parent, i);
        let e = clusters.entry(root).or_insert((Vec::new(), faces[i].2));
        e.1 = union_bbox(e.1, faces[i].2);
        e.0.push(i);
    }
    let clusters: Vec<(Vec<usize>, (u8, u8, u8, u8))> = clusters.into_values().collect();

    // Shelf-pack the cluster bboxes into the window; halve on overflow.
    let mut placed: Vec<(usize, usize)> = Vec::with_capacity(clusters.len());
    let mut scale = 1usize;
    'retry: loop {
        placed.clear();
        let (mut x, mut y, mut shelf) = (0usize, 0usize, 0usize);
        for &(_, bb) in &clusters {
            let w = ((bb.2 - bb.0) as usize + 1).div_ceil(scale);
            let h = ((bb.3 - bb.1) as usize + 1).div_ceil(scale);
            if w > ATLAS_WINDOW_W || {
                if x + w > ATLAS_WINDOW_W {
                    y += shelf;
                    x = 0;
                    shelf = 0;
                }
                y + h > ATLAS_WINDOW_H
            } {
                if scale >= 8 {
                    bail!("head texture does not fit the field atlas window");
                }
                scale *= 2;
                warnings.push(format!(
                    "field head texture at 1/{scale} resolution (atlas window)"
                ));
                continue 'retry;
            }
            placed.push((x, y));
            x += w;
            shelf = shelf.max(h);
        }
        break;
    }

    for (ci, (members, bb)) in clusters.iter().enumerate() {
        let (dx, dy) = placed[ci];
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
                    if tx >= ATLAS_WINDOW_W || ty >= ATLAS_WINDOW_H {
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
                    window[ty * ATLAS_WINDOW_W + tx] = new_idx as u8;
                }
            }
            for uv in p.uvs.iter_mut() {
                let nx = u_base + dx + (uv.0 as usize - bb.0 as usize) / scale;
                let ny = dy + (uv.1 as usize - bb.1 as usize) / scale;
                *uv = (nx.min(255) as u8, ny.min(255) as u8);
            }
            p.cba = (FIELD_CLUT_ROW << 6) | (col_base + group as u16);
            p.tsb = FIELD_TSB;
        }
    }
    Ok((window, group_colors))
}

/// Rebuild the PROT 0874 entry: slots 0..2 wear the mapped siblings'
/// models, the locomotion bundle survives verbatim, and each
/// character's atlas TIM entry repaints to the new head window.
/// `mapping[slot]` = the monster id whose model character `slot` wears.
pub fn fieldize_pack(
    prot_0874: &[u8],
    entry_len: usize,
    archive_entry: &[u8],
    mapping: [u16; 3],
) -> Result<FieldizedPack> {
    let sources = mapping
        .iter()
        .map(|&id| monster_slot_source(archive_entry, id))
        .collect::<Result<Vec<_>>>()?;
    fieldize_pack_laddered(prot_0874, entry_len, &sources, mapping)
}

/// Like [`fieldize_pack`], but sourcing each character's model from the
/// sibling's own field NPC mesh (nilboa scene, [`NPC_PACK_ENTRY`] /
/// [`NPC_BUNDLE_ENTRY`]) instead of converting the battle monster model.
/// Retail-authored field geometry fits the §0 budget at full detail, so
/// this is the quality-preserving path; the monster conversion stays the
/// fallback.
pub fn fieldize_pack_npc(
    prot_0874: &[u8],
    entry_len: usize,
    npc_pack: &[u8],
    npc_bundle: &[u8],
    mapping: [u16; 3],
) -> Result<FieldizedPack> {
    let sources = mapping
        .iter()
        .map(|&id| npc_slot_source(npc_pack, npc_bundle, id))
        .collect::<Result<Vec<_>>>()?;
    fieldize_pack_laddered(prot_0874, entry_len, &sources, mapping)
}

fn fieldize_pack_laddered(
    prot_0874: &[u8],
    entry_len: usize,
    sources: &[SlotSource],
    mapping: [u16; 3],
) -> Result<FieldizedPack> {
    // Detail ladder: full detail DOUBLE-SIDED first (the NPC meshes are
    // not watertight from free angles - see `double_side`), then
    // single-sided, then progressively drop prims under a size
    // threshold (invisible at the chibi field scale) until the rebuilt
    // container fits its PROT entry.
    let mut last_err = None;
    use DoubleSideScope as Ds;
    for (decimate, double_sided) in [
        (0.0f32, Ds::All),
        (0.0, Ds::HeadTorso),
        (0.0, Ds::Head),
        (0.0, Ds::None),
        (2.0, Ds::None),
        (3.5, Ds::None),
        (5.0, Ds::None),
        (7.0, Ds::None),
        (9.0, Ds::None),
        (12.0, Ds::None),
    ] {
        match fieldize_pack_at(
            prot_0874,
            entry_len,
            sources,
            mapping,
            decimate,
            double_sided,
        ) {
            Ok(mut out) => {
                match double_sided {
                    Ds::All => {}
                    Ds::HeadTorso => out
                        .warnings
                        .push("field mesh double-sided on head+torso only (container size)".into()),
                    Ds::Head => out
                        .warnings
                        .push("field mesh double-sided on the head only (container size)".into()),
                    Ds::None => out
                        .warnings
                        .push("field mesh single-sided (container size)".into()),
                }
                if decimate > 0.0 {
                    out.warnings
                        .push(format!("field detail reduced (min prim size {decimate})"));
                }
                return Ok(out);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("field rebuild failed")))
        .context("rebuilt PROT 0874 does not fit its entry at any detail level")
}

/// Sibling-native / retail-host posed height ratio for one field slot,
/// measured over the role-mapped parts on both sides (the same stats the
/// bake's own radial uses, inverted). Clamped to a sane figure range.
fn slot_height_ratio(
    pack: &character_pack::CharacterPack,
    retail_anm: &crate::player_anm::PlayerAnmBundle,
    slot: usize,
    source: &SlotSource,
) -> Result<f32> {
    let cs = pack
        .slot(slot)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot} missing"))?;
    let field_tmd = legaia_tmd::parse(&cs.tmd_bytes).context("field TMD")?;
    let field_model = decode_model(&field_tmd, &cs.tmd_bytes)?;
    let idle_rec =
        character_pack::locomotion_record_index(slot, character_pack::LOCOMOTION_IDLE_SLOT);
    let idle = retail_anm
        .record_to_monster_animation(idle_rec)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot}: idle clip missing"))?;
    let field_rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("field idle has no frames"))?;
    let roles = derive_field_roles(&field_model, field_rest)?;
    let dst_parts: Vec<(&ModelObject, &PartPose)> = roles
        .iter()
        .map(|&b| (&field_model[b], &field_rest[b]))
        .collect();
    let src_parts: Vec<(&ModelObject, &PartPose)> = source
        .role_sources
        .iter()
        .flatten()
        .map(|&c| (&source.model[c], &source.rest[c]))
        .collect();
    let (_, e_dst) = group_world_stats(&dst_parts);
    let (_, e_src) = group_world_stats(&src_parts);
    Ok((e_src[1] / e_dst[1]).clamp(0.5, 2.0))
}

fn fieldize_pack_at(
    prot_0874: &[u8],
    entry_len: usize,
    sources: &[SlotSource],
    mapping: [u16; 3],
    decimate: f32,
    double_sided: DoubleSideScope,
) -> Result<FieldizedPack> {
    let mut warnings = Vec::new();
    let pack = character_pack::parse(prot_0874).context("parse PROT 0874")?;
    let container = parse_player_lzs(prot_0874, character_pack::CONTAINER_DESCRIPTORS)?;

    // Height-preserving locomotion retarget: the shared §1 clips pose
    // each slot at the retail HERO's joint spacing, so a sibling baked
    // onto them walks at the hero's stature (Che loses ~40% of his
    // nilboa height, and the compressed skeleton swallows the head into
    // the chest). The per-(bone, frame) translations are flat
    // actor-space joint positions with the origin on the floor plane,
    // so scaling every translation of a slot's 7-record bank by the
    // sibling/host height ratio rescales the whole walking skeleton in
    // place - same byte size, rotations untouched - and the §0 bake
    // below then anchors on the SCALED rest, keeping the chains closed.
    // The two shared records (21 savepoint / 22 aux) stay retail: they
    // already pose three differently-proportioned heroes in retail, and
    // scaling them for one slot would break the other two.
    let sec1_desc = &container.descriptors[character_pack::LOCOMOTION_SECTION];
    let mut sec1_decoded = crate::decode(prot_0874, sec1_desc, crate::DecodeMode::Lzs)
        .context("LZS-decode PROT 0874 section 1 (party locomotion ANM)")?;
    let retail_anm =
        crate::player_anm::parse(&sec1_decoded).context("parse retail locomotion bundle")?;
    let mut sec1_scaled = false;
    for (slot, source) in sources.iter().enumerate().take(3) {
        let r = slot_height_ratio(&pack, &retail_anm, slot, source)
            .with_context(|| format!("field slot {slot} height ratio"))?;
        if (r - 1.0).abs() < 0.02 {
            continue;
        }
        for bank in 0..character_pack::LOCOMOTION_BANK_STRIDE {
            let rec = character_pack::locomotion_record_index(slot, bank);
            crate::player_anm::scale_record_translations(&mut sec1_decoded, rec, r)
                .with_context(|| format!("scale slot {slot} locomotion record {rec}"))?;
        }
        sec1_scaled = true;
        warnings.push(format!(
            "field slot {slot}: walking skeleton scaled x{r:.2} to the sibling's own height"
        ));
    }
    let anm = if sec1_scaled {
        crate::player_anm::parse(&sec1_decoded).context("re-parse scaled locomotion bundle")?
    } else {
        retail_anm
    };

    let mut slots: Vec<FieldSlot> = Vec::with_capacity(3);
    for (slot, source) in sources.iter().enumerate() {
        slots.push(
            fieldize_slot(
                &pack,
                &anm,
                slot,
                source,
                decimate,
                double_sided,
                &mut warnings,
            )
            .with_context(|| format!("field slot {slot} <- monster {}", mapping[slot]))?,
        );
    }

    // New §0: the 5-body pack with slots 0..2 replaced. Its DECODED size
    // must stay EXACTLY retail's: the battle scene loader registers the
    // container's first three header words (`meta[1]`, `type<<24|size0`,
    // `offset0`) as battle-VDF pointers off the raw entry base at every
    // battle load (`FUN_800520F0` state 0xc first loop -> `FUN_8001FBCC`),
    // and `meta[1]` doubles as the byte offset of the VDF tail that lives
    // PAST the LZS payload inside the same PROT entry. Changing either
    // word points the effect system at garbage and hangs the battle load.
    // Retail's own §0 carries ~19 KB of trailing pack padding, so padding
    // to the exact size is the retail shape.
    let sec0 = &container.descriptors[character_pack::CONTAINER_SECTION];
    let sec0_decoded = crate::decode(prot_0874, sec0, crate::DecodeMode::Lzs)?;
    let bodies = extract_pack(&sec0_decoded)?;
    let mut new_bodies: Vec<Vec<u8>> = Vec::with_capacity(bodies.len());
    for (i, b) in bodies.iter().enumerate() {
        if i < 3 {
            new_bodies.push(slots[i].tmd.clone());
        } else {
            new_bodies.push(b.to_vec());
        }
    }
    let mut sec0_new = Vec::new();
    sec0_new.extend_from_slice(&(new_bodies.len() as u32).to_le_bytes());
    let table_words = 1 + new_bodies.len();
    let mut cursor = table_words;
    let mut offsets = Vec::with_capacity(new_bodies.len());
    for b in &new_bodies {
        offsets.push(cursor as u32);
        cursor += b.len().div_ceil(4);
    }
    for off in &offsets {
        sec0_new.extend_from_slice(&off.to_le_bytes());
    }
    for b in &new_bodies {
        sec0_new.extend_from_slice(b);
        while sec0_new.len() % 4 != 0 {
            sec0_new.push(0);
        }
    }
    if sec0_new.len() > sec0_decoded.len() {
        bail!(
            "rebuilt §0 pack decodes to {} bytes, retail's {} is a hard cap \
             (battle-VDF header words must stay byte-exact)",
            sec0_new.len(),
            sec0_decoded.len()
        );
    }
    sec0_new.resize(sec0_decoded.len(), 0);

    // New §2: the atlas with each character entry's pixels + CLUT
    // repainted in place (same TIM sizes).
    let sec2 = &container.descriptors[2];
    let mut sec2_new = crate::decode(prot_0874, sec2, crate::DecodeMode::Lzs)?;
    patch_atlas(&mut sec2_new, &slots)?;

    // §1 (locomotion): the original compressed byte span when untouched,
    // a re-compress of the scaled bundle otherwise.
    let sec1 = &container.descriptors[1];
    let spans: Vec<(usize, usize)> = {
        let mut offs: Vec<usize> = container
            .descriptors
            .iter()
            .map(|d| d.data_offset as usize)
            .collect();
        offs.push(entry_len);
        (0..container.descriptors.len())
            .map(|i| (offs[i], offs[i + 1]))
            .collect()
    };
    let sec1_raw = if sec1_scaled {
        legaia_lzs::compress(&sec1_decoded)
    } else {
        prot_0874
            .get(spans[1].0..spans[1].1.min(prot_0874.len()))
            .ok_or_else(|| anyhow::anyhow!("section 1 span out of range"))?
            .to_vec()
    };

    // Reassemble the container: header + pairs + section data. Greedy
    // LZS first; the optimal parse only when the entry budget misses.
    let mut s0 = legaia_lzs::compress(&sec0_new);
    let mut s2 = legaia_lzs::compress(&sec2_new);
    let mut sec1_raw = sec1_raw;
    let header_and_slack = 0x20 + 8;
    if header_and_slack + s0.len() + sec1_raw.len() + s2.len() > entry_len {
        s0 = legaia_lzs::compress_optimal(&sec0_new);
        s2 = legaia_lzs::compress_optimal(&sec2_new);
        if sec1_scaled {
            sec1_raw = legaia_lzs::compress_optimal(&sec1_decoded);
        }
    }
    if std::env::var("LEGAIA_FIELDIZE_DEBUG").is_ok() {
        eprintln!(
            "[fieldize] decimate {decimate}: sec0 {} (lzs {}), sec1 raw {}, sec2 {} (lzs {}), entry {}",
            sec0_new.len(),
            s0.len(),
            sec1_raw.len(),
            sec2_new.len(),
            s2.len(),
            entry_len
        );
    }
    let streams = [s0, sec1_raw, s2];
    let sizes = [sec0_new.len() as u32, sec1.size, sec2_new.len() as u32];
    let types = [sec0.type_byte, sec1.type_byte, sec2.type_byte];
    let mut out = Vec::with_capacity(entry_len);
    out.extend_from_slice(&container.meta[0].to_le_bytes());
    // meta[1] = total decompressed budget (sum of the three sizes).
    let meta1: u32 = sizes.iter().sum();
    out.extend_from_slice(&meta1.to_le_bytes());
    let header_len = 8 + 3 * 8;
    let mut offset = header_len.max(container.descriptors[0].data_offset as usize);
    let mut pairs = Vec::new();
    for i in 0..3 {
        pairs.push(((types[i] as u32) << 24 | sizes[i], offset as u32));
        offset += streams[i].len();
        offset = offset.div_ceil(4) * 4;
    }
    for (ts, off) in &pairs {
        out.extend_from_slice(&ts.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    for (i, s) in streams.iter().enumerate() {
        let start = pairs[i].1 as usize;
        if out.len() > start {
            bail!("container section {i} overlaps the previous one");
        }
        out.resize(start, 0);
        out.extend_from_slice(s);
    }
    if out.len() > entry_len {
        bail!(
            "rebuilt PROT 0874 is {} bytes, entry holds {}",
            out.len(),
            entry_len
        );
    }
    // Preserve the RETAIL bytes past the rebuilt payload instead of
    // zero-filling: the band after the compressed streams carries the
    // battle-VDF effect payload the scene loader registers off this
    // entry's header words at every battle load (see the battle-VDF
    // note above sec0). Zero-filling it fed the battle effect system
    // zeros - measured live as thin garbage streaks over the Spirit
    // charge (the `delilas_gi_spirit_artifact` catalogue state), and
    // statically as ~4.5 KB of retail-nonzero bytes at the entry tail
    // (0x186DB..0x197FF on the USA disc) that the rebuild wiped. The
    // copy keeps every retained byte at its ORIGINAL offset (the tail
    // is addressed relative to the raw entry base); the stretch between
    // the shorter rebuilt streams and retail's stream end is stale
    // compressed data nothing reads.
    let payload_end = out.len();
    let retail_rest = prot_0874
        .get(payload_end..entry_len)
        .ok_or_else(|| anyhow::anyhow!("retail entry shorter than its own length"))?;
    out.extend_from_slice(retail_rest);
    Ok(FieldizedPack {
        entry: out,
        warnings,
    })
}

/// Repaint each character's atlas TIM (entries 1..=3 of §2) with the new
/// head window + palettes, in place inside the decoded §2 pack bytes.
fn patch_atlas(sec2: &mut [u8], slots: &[FieldSlot]) -> Result<()> {
    let entries = crate::pack::parse_pack(sec2)?;
    for (slot, fs) in slots.iter().enumerate() {
        let e = entries
            .get(slot + 1)
            .ok_or_else(|| anyhow::anyhow!("atlas entry {} missing", slot + 1))?;
        let (start, len) = (e.byte_offset, e.size);
        let tim = sec2
            .get_mut(start..start + len)
            .ok_or_else(|| anyhow::anyhow!("atlas entry {} out of range", slot + 1))?;
        patch_tim(tim, fs).with_context(|| format!("atlas entry {}", slot + 1))?;
    }
    Ok(())
}

/// Patch one 4bpp CLUT'd TIM in place: CLUT entries (4 x 16 colours) and
/// pixel data (20 halfwords x 128 rows) replaced, geometry untouched.
/// TIM layout: `[u32 0x10][u32 flags][clut block][image block]`, each
/// block `[u32 len][u16 x][u16 y][u16 w][u16 h][data]`.
fn patch_tim(tim: &mut [u8], fs: &FieldSlot) -> Result<()> {
    let magic = u32::from_le_bytes(tim.get(0..4).unwrap_or(&[0; 4]).try_into().unwrap());
    let flags = u32::from_le_bytes(tim.get(4..8).unwrap_or(&[0; 4]).try_into().unwrap());
    if magic != 0x10 || flags & 0x8 == 0 {
        bail!("not a CLUT'd TIM (magic {magic:#x}, flags {flags:#x})");
    }
    let clut_len = u32::from_le_bytes(tim[8..12].try_into().unwrap()) as usize;
    let clut_w = u16::from_le_bytes(tim[16..18].try_into().unwrap()) as usize;
    let clut_h = u16::from_le_bytes(tim[18..20].try_into().unwrap()) as usize;
    let clut_data = 20;
    let clut_n = clut_w * clut_h;
    if clut_n != ATLAS_COLS_PER_CHAR * 16 {
        bail!("atlas CLUT holds {clut_n} colours, expected 64");
    }
    let dst = tim
        .get_mut(clut_data..clut_data + clut_n * 2)
        .ok_or_else(|| anyhow::anyhow!("CLUT data out of range"))?;
    dst.fill(0);
    for (g, colors) in fs.palettes.iter().enumerate().take(ATLAS_COLS_PER_CHAR) {
        for (i, &c) in colors.iter().enumerate().take(15) {
            let at = (g * 16 + 1 + i) * 2;
            dst[at..at + 2].copy_from_slice(&c.to_le_bytes());
        }
    }
    // Image block follows the CLUT block (block len covers len..data end).
    let img = 8 + clut_len;
    let img_w = u16::from_le_bytes(
        tim.get(img + 8..img + 10)
            .ok_or_else(|| anyhow::anyhow!("image block out of range"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let img_h = u16::from_le_bytes(tim[img + 10..img + 12].try_into().unwrap()) as usize;
    if img_w * 2 * 2 != ATLAS_WINDOW_W || img_h != ATLAS_WINDOW_H {
        bail!("atlas image is {img_w}hw x {img_h}, expected 20x128");
    }
    let data = img + 12;
    let dst = tim
        .get_mut(data..data + img_w * 2 * img_h)
        .ok_or_else(|| anyhow::anyhow!("image data out of range"))?;
    for yy in 0..ATLAS_WINDOW_H {
        for xb in 0..ATLAS_WINDOW_W / 2 {
            let lo = fs.window[yy * ATLAS_WINDOW_W + xb * 2] & 0xF;
            let hi = fs.window[yy * ATLAS_WINDOW_W + xb * 2 + 1] & 0xF;
            dst[yy * (ATLAS_WINDOW_W / 2) + xb] = lo | (hi << 4);
        }
    }
    Ok(())
}
