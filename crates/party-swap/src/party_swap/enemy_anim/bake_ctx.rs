//! The monster bake context: what a monster block needs resolved before its
//! animations can be rebuilt, and the player rest-terminal normalisation it
//! depends on.

use super::super::*;
use winpose::{mmul, to_euler, transpose};

// ---------------------------------------------------------------------------
// Shared bake context.

/// Everything the enemy-side bake and the hero→monster retarget must
/// agree on. Built once from the RETAIL player file and the RETAIL
/// monster archive; both consumers read the same rest poses, the same
/// bone frames and the same radial scale, which is what keeps the
/// retarget's conjugation cancelling the bake exactly.
pub(crate) struct MonsterBakeCtx {
    /// Player geometry in canonical (Delilas) part order: equipment
    /// extras merged into their attach bones, Noa's hair rebased into the
    /// head, objects compacted - the PRE-bake mesh.
    pub objects: Vec<ModelObject>,
    /// Player rest pose (idle frame 0) with the terminal parts
    /// (head/hands/feet) pre-rotated by the inverse of their frame
    /// alignment, so they keep their authored world orientation through
    /// the stance realignment (mirror of `normalize_battle_rest_feet`).
    pub rest: Vec<PartPose>,
    /// The authored player rest, untouched (fit-instrument reference).
    pub rest_raw: Vec<PartPose>,
    /// Retail monster mesh objects (canonical order).
    pub target_model: Vec<ModelObject>,
    /// Monster rest pose (idle frame 0, canonical order).
    pub target_rest: Vec<PartPose>,
    /// Canonical-order player rest pivots.
    pub src_pivots: Vec<[f32; 3]>,
    /// Canonical-order monster rest pivots.
    pub dst_pivots: Vec<[f32; 3]>,
    /// `playerize::bake_frames` output - the whole-rig re-face + minimal
    /// swing frames both the bake and the retarget align through.
    pub src_frames: Vec<BoneFrame>,
    pub dst_frames: Vec<BoneFrame>,
    /// Uniform radial scale (monster/player rest-height ratio).
    pub radial: f32,
}

/// Keep the player rest's TERMINAL parts (head, hands, feet) at their
/// authored world orientation through the player→monster stance
/// realignment - the exact mirror of `normalize_battle_rest_feet`
/// (playerize's monster→player direction): a terminal inherits its chain
/// parent's frame, and that frame's alignment encodes the stance delta
/// between the player idle and the Delilas idle, so the terminal dragged
/// rigidly along pitches/rolls by the whole delta. Pre-rotating the
/// channel by the alignment's inverse cancels the drag. Pivots are
/// untouched. Must be applied to the SAME rest by both the mesh bake
/// (`monsterize_player`) and the clip retarget ([`HeroRetarget`]).
pub(crate) fn normalize_player_rest_terminals(
    rest: &mut [PartPose],
    target_rest: &[PartPose],
    rig: &PlayerRig,
) {
    if target_rest.len() < CANONICAL_PARTS {
        return;
    }
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let Some(src_pivots) = (0..CANONICAL_PARTS)
        .map(|c| {
            rest.get(rig.channel_for_canonical[c] as usize)
                .map(pivot_of)
        })
        .collect::<Option<Vec<[f32; 3]>>>()
    else {
        return;
    };
    let dst_pivots: Vec<[f32; 3]> = target_rest
        .iter()
        .take(CANONICAL_PARTS)
        .map(pivot_of)
        .collect();
    let src_frames = bone_frames(&src_pivots, &CANONICAL_CHILD, &CANONICAL_PARENT);
    let dst_frames = bone_frames(&dst_pivots, &CANONICAL_CHILD, &CANONICAL_PARENT);
    for term in [0usize, 5, 8, 11, 14] {
        let ch = rig.channel_for_canonical[term] as usize;
        let a = frame_align(&src_frames[term], &dst_frames[term]);
        let m = mmul(&transpose(&a), &rot_matrix(&rest[ch]));
        let (rx, ry, rz) = to_euler(&m);
        rest[ch].rx = rx;
        rest[ch].ry = ry;
        rest[ch].rz = rz;
    }
}

/// Build the shared bake context from the RETAIL player file and the
/// RETAIL monster archive. This is the front half of the enemy-side mesh
/// bake, factored out so the clip retarget reads the identical data.
pub(crate) fn monster_bake_ctx(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
) -> Result<MonsterBakeCtx> {
    let pack = battle_data_pack::parse(player_file).context("parse player battle file")?;
    let equipped = [0u8; SECTION_COUNT];
    let asm = battle_char_assembly::assemble_character(player_file, &pack, &equipped)
        .context("assemble default-equipment battle mesh")?;
    let tmd = legaia_tmd::parse(&asm.tmd).context("parse assembled TMD")?;
    let mut source = decode_model(&tmd, &asm.tmd).context("decode assembled model")?;

    let idle = battle_char_assembly::idle_battle_animation(player_file)?
        .ok_or_else(|| anyhow::anyhow!("player file has no idle animation"))?;
    let rest_raw = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("idle animation has no frames"))?
        .clone();

    // Target rig: retail mesh + rest pose.
    let target_mesh = monster_archive::mesh(archive_entry, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: empty slot"))?;
    let target_tmd = legaia_tmd::parse(target_mesh.tmd_bytes()).context("target monster TMD")?;
    let target_model = decode_model(&target_tmd, target_mesh.tmd_bytes())?;
    if target_model.len() != CANONICAL_PARTS {
        bail!(
            "monster id {target_id} has {} parts, expected {CANONICAL_PARTS}",
            target_model.len()
        );
    }
    let target_idle = monster_archive::idle_animation(archive_entry, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: no idle animation"))?;
    let target_rest = target_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle has no frames"))?
        .clone();

    // Player-shaped terminals (mirror of playerize's feet/head/hand
    // normalization - the bake and the retarget both read this rest).
    let mut rest = rest_raw.clone();
    normalize_player_rest_terminals(&mut rest, &target_rest, rig);

    // Per-channel CORE part anchors, snapshotted before the extras merge.
    let skeleton = rest.len();
    let mut core_stats: Vec<PartStats> = source
        .iter()
        .take(skeleton)
        .enumerate()
        .map(|(ch, o)| part_world_stats(o, &rest[ch]))
        .collect();

    // Merge the equipment extras into their attach bones' objects.
    let mut merged: Vec<Option<ModelObject>> = source.drain(..).map(Some).collect();
    for (oi, &ch) in asm.anm_bones.iter().enumerate() {
        if oi < skeleton {
            continue;
        }
        let Some(extra) = merged[oi].take() else {
            continue;
        };
        if let Some(dst) = merged.get_mut(ch as usize).and_then(|d| d.as_mut()) {
            merge_object(dst, &extra);
        }
    }
    // Noa's hair: rebase into the head frame; the head anchor recomputes
    // over the merged head+hair.
    if let Some(hair_ch) = rig.hair_channel {
        let head_ch = rig.channel_for_canonical[0] as usize;
        if let Some(hair) = merged.get_mut(hair_ch as usize).and_then(|h| h.take()) {
            let head_pose = rest[head_ch];
            let hair_pose = rest[hair_ch as usize];
            if let Some(head) = merged.get_mut(head_ch).and_then(|d| d.as_mut()) {
                rebase_merge(head, &head_pose, &hair, &hair_pose)?;
                core_stats[head_ch] = part_world_stats(head, &head_pose);
            }
        }
    }

    // Permute into the canonical (Delilas) part order + compact.
    let mut objects: Vec<ModelObject> = Vec::with_capacity(CANONICAL_PARTS);
    for c in 0..CANONICAL_PARTS {
        let ch = rig.channel_for_canonical[c] as usize;
        let obj = merged
            .get_mut(ch)
            .and_then(|o| o.take())
            .ok_or_else(|| anyhow::anyhow!("player channel {ch} has no object"))?;
        objects.push(obj);
    }
    for o in objects.iter_mut() {
        compact_object(o);
    }

    // Scale + frames.
    let dst_stats: Vec<PartStats> = target_model
        .iter()
        .enumerate()
        .map(|(c, o)| part_world_stats(o, &target_rest[c]))
        .collect();
    let src_stats: Vec<PartStats> = (0..CANONICAL_PARTS)
        .map(|c| core_stats[rig.channel_for_canonical[c] as usize])
        .collect();
    let radial = global_height_scale(&src_stats, &dst_stats)[0];
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_pivots: Vec<[f32; 3]> = (0..CANONICAL_PARTS)
        .map(|c| pivot_of(&rest[rig.channel_for_canonical[c] as usize]))
        .collect();
    let dst_pivots: Vec<[f32; 3]> = target_rest.iter().map(pivot_of).collect();
    // The whole-rig re-face + minimal-swing frames (NOT raw bone_frames:
    // per-joint bend-plane references are not comparable across two
    // independently authored rigs and roll parts about their own axis -
    // see playerize::bake_frames).
    let (src_frames, dst_frames) = playerize::bake_frames(
        &src_pivots,
        &dst_pivots,
        &CANONICAL_CHILD,
        &CANONICAL_PARENT,
    );

    Ok(MonsterBakeCtx {
        objects,
        rest,
        rest_raw,
        target_model,
        target_rest,
        src_pivots,
        dst_pivots,
        src_frames,
        dst_frames,
        radial,
    })
}
