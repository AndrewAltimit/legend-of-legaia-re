//! Sourcing a field slot from an NPC model: the NPC pack coordinates, rest-leg
//! symmetrising, head levelling and the slot source itself.

use super::super::*;
use super::FIELD_BONES;
use super::atlas::paint_vram;
use super::roles::derive_field_roles;
use super::slot::SlotSource;
use legaia_container_formats::parse_player_lzs;

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
pub(in super::super) fn npc_coords(monster_id: u16) -> Option<(usize, usize)> {
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
pub(in super::super) fn symmetrize_rest_legs(rest: &mut [PartPose]) {
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
pub(super) fn npc_slot_source(
    npc_pack: &[u8],
    npc_bundle: &[u8],
    monster_id: u16,
) -> Result<SlotSource> {
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
    let entries = legaia_container_formats::pack::parse_pack(pack)?;
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
    let bundle = legaia_actor_formats::player_anm::find_in_entry(npc_bundle, 5)
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
    let tims_payload = legaia_container_formats::decode(
        npc_bundle,
        &container.descriptors[0],
        legaia_container_formats::DecodeMode::Lzs,
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
