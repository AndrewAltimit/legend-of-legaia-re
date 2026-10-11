//! Player-side of the party swap: rebuild a `PLAYERn` battle file so the
//! character wears a Delilas sibling's model under their own animations,
//! whatever equipment is worn.
//!
//! Every equipment section record embeds its own slice of the skeleton
//! (`select_sections` picks by equipped id), so a swap that survives
//! equipment changes must rewrite **every** section record. The whole
//! PROT entry is rebuilt: header + `record[0]` (kept verbatim - it is
//! the character's own animation/palette/face source) + a rewritten
//! descriptor chain + one rewritten record per equipment id.
//!
//! Per record, everything except the mesh and the pool survives byte
//! for byte: the 0x14-byte header (offsets updated), the attach-object
//! offsets, the loader frame's attach list, and the swing/attach action
//! records that sit between the mesh end and `body_end` (shifted
//! whole). The embedded TMD keeps its retail object count - skeleton
//! objects get the baked Delilas part for their bone, surplus
//! (equipment-visual) objects go empty. The pool becomes the section's
//! fixed VRAM tile of the re-laid Delilas texture page, and every
//! record's upload flag is forced on so the full band uploads
//! regardless of equipment.
//!
//! Palette columns: all battle palette bands flow through the section
//! pools' trailing CLUT structs plus `record[0]`'s CLUT A/B structs
//! (`battle_char_palette`). The sections are ours, so the free columns
//! are `16 - record[0]'s claims`; the Delilas palettes union-merge down
//! to fit (two 16-colour palettes merge when their union still fits,
//! texel indices remapped during the island copy).

use super::*;
use legaia_actor_formats::battle_char_assembly::record0_texture_uploads;
use legaia_actor_formats::battle_data_pack::decode_record;

mod frame_bake;
mod hands;
mod rebuild;
mod section_record;
mod texture_band;

pub(crate) use frame_bake::bake_frames;
pub use hands::kept_welded_hand;
use hands::{HandTwins, WAIST_CURTAIN_IDS, WELDED_WEAPON_FISTS, fk_inset_hand, mirror_fist};
pub use rebuild::{rebuild_player_file, rebuild_player_file_unbounded};
use section_record::rewrite_section_record;
pub use section_record::{alias_variant_onto_bone, splice_record_tmd, variant_object};
pub use texture_band::FACE_STAMP_WINDOWS;
use texture_band::relayout_to_band;
pub(super) use texture_band::{merge_palettes, monster_pool_texels, nearest_color};
/// Bytes per stored monster CLUT (16 BGR555 entries).
const CLUT_BYTES: usize = 32;

/// The rebuilt player file.
#[derive(Debug, Clone)]
pub struct PlayerizedFile {
    /// The full PROT-entry bytes (same length as `entry_len`).
    pub file: Vec<u8>,
    pub warnings: Vec<String>,
}

/// Rebuild a `PLAYERn` battle file so the character wears the Delilas
/// model of `source_id` (162/163/164). `entry_len` is the PROT entry's
/// exact byte length (the output is padded to it). `char_slot` is the
/// player-file slot (0 Vahn / 1 Noa / 2 Gala) - it keys the committed
/// item-isolation rules the weapon fusion runs under.
/// The band CLUT columns reserved for the fused weapon's palettes: the
/// [`weapon_fuse::WEAPON_PALETTE_MAX`] highest columns record[0]'s own
/// uploads do not claim. The relayout keeps sibling palette groups off
/// them; each weapon-section record's CLUT run installs its weapon's
/// palettes there (see `weapon_fuse`).
///
/// [`weapon_fuse::WEAPON_PALETTE_MAX`]: super::weapon_fuse::WEAPON_PALETTE_MAX
fn weapon_clut_cols(player_file: &[u8]) -> Result<Vec<u16>> {
    let reserved: Vec<u16> = record0_texture_uploads(player_file, 0)?
        .iter()
        .filter(|u| !u.clut.is_empty())
        .flat_map(|u| {
            let first = u.clut_x / 16;
            let n_cols = (u.clut.len() as u16).div_ceil(16);
            (first..first + n_cols).collect::<Vec<u16>>()
        })
        .collect();
    let mut cols: Vec<u16> = (0..16u16)
        .rev()
        .filter(|c| !reserved.contains(c))
        .take(super::weapon_fuse::WEAPON_PALETTE_MAX)
        .collect();
    cols.reverse();
    if cols.len() < super::weapon_fuse::WEAPON_PALETTE_MAX {
        bail!("record[0] claims too many CLUT columns to reserve a weapon palette");
    }
    Ok(cols)
}

pub fn playerize_player_file(
    player_file: &[u8],
    entry_len: usize,
    rig: &PlayerRig,
    archive_entry: &[u8],
    source_id: u16,
    char_slot: usize,
    readef: Option<&[u8]>,
) -> Result<PlayerizedFile> {
    playerize_player_file_with(
        player_file,
        entry_len,
        rig,
        archive_entry,
        source_id,
        char_slot,
        readef,
        false,
    )
}

/// [`playerize_player_file`] with the welded-weapon-fist policy
/// explicit: `keep_welded_fist = true` keeps a sibling's welded weapon
/// on the mesh (Che's hammer-fist, Gi's blade-fist -
/// [`WELDED_WEAPON_FISTS`]) instead of mirroring the other fist in, and
/// suppresses the host weapon fusion for that file so the hand does not
/// carry two weapons. The kept weapon rides the host's hand channel
/// with its authored reach, so clips that assume a hand-sized part
/// swing it wide - the trade the caller opts into.
#[allow(clippy::too_many_arguments)]
pub fn playerize_player_file_with(
    player_file: &[u8],
    entry_len: usize,
    rig: &PlayerRig,
    archive_entry: &[u8],
    source_id: u16,
    char_slot: usize,
    readef: Option<&[u8]>,
    keep_welded_fist: bool,
) -> Result<PlayerizedFile> {
    // Every clip the character can play - record[0]'s battle bank plus,
    // when readef.DAT is supplied, the full art bank (the Spirit charge,
    // the Super flourishes, every art). The FK hand inset optimizes the
    // wrist seam across ALL of them; a record0-only objective left one
    // art clip's seam a hair past the audit band.
    let mut host_anims = legaia_actor_formats::battle_char_assembly::battle_animations(player_file)
        .unwrap_or_default();
    if let Some(readef) = readef
        && let Ok(rec0) = legaia_actor_formats::battle_char_assembly::decode_record0(player_file)
        && let Ok(bank) = legaia_actor_formats::battle_char_assembly::art_animation_bank(&rec0)
    {
        for rec in &bank {
            if let Ok(archive) = legaia_actor_formats::battle_char_assembly::art_me_archive(
                readef,
                char_slot,
                rec.uses_base_archive(),
            ) && let Ok(clip) =
                legaia_actor_formats::battle_char_assembly::art_animation(rec, &archive)
            {
                host_anims.push(clip);
            }
        }
    }
    // The host's own equipped-weapon geometry, cut once per record and
    // fused back into the swapped hand at record-rewrite time (see
    // `weapon_fuse`). Non-fatal: a failed cut leaves the swap bare-handed
    // exactly as before, with a note.
    let weapon_cols = weapon_clut_cols(player_file)?;
    let keeps_welded =
        keep_welded_fist && WELDED_WEAPON_FISTS.iter().any(|&(id, _)| id == source_id);
    let (fusions, fuse_warning) = if keeps_welded {
        // The sibling's own welded weapon stays in the hand - fusing the
        // host's weapon on top would put two weapons in one fist.
        (
            super::weapon_fuse::WeaponFusion::default(),
            Some("welded fist kept; host weapon not fused".to_string()),
        )
    } else {
        match super::weapon_fuse::weapon_fusions(player_file, char_slot, &weapon_cols) {
            Ok(f) => {
                let n = f.per_record.len();
                (
                    f,
                    Some(format!("host weapon fused into {n} held-item records")),
                )
            }
            Err(e) => (
                super::weapon_fuse::WeaponFusion::default(),
                Some(format!(
                    "weapon fusion unavailable ({e:#}); hands stay bare"
                )),
            ),
        }
    };
    let mut last_err = None;
    // Fit ladder: texture resolution OUTRANKS hand twins (the quality
    // oracle holds full-resolution textures for every pairing), and
    // within one resolution the twins degrade full -> open-boundary
    // prims only -> none. The reversed twins keep the open-shell fists
    // visible from every angle; some pairings (Gi/Che -> Gala) miss the
    // entry by about a sector with the full treatment, and the
    // boundary-only set is the half-cost fallback that still covers the
    // faces that read as holes.
    for downscale in [1u32, 2, 4] {
        for hand_twins in [HandTwins::Full, HandTwins::WeaponHand, HandTwins::None] {
            match playerize_scaled(
                player_file,
                entry_len,
                rig,
                archive_entry,
                source_id,
                downscale,
                hand_twins,
                &fusions,
                &weapon_cols,
                &host_anims,
                char_slot,
                keep_welded_fist,
            ) {
                Ok(mut out) => {
                    if let Some(w) = &fuse_warning {
                        out.warnings.push(w.clone());
                    }
                    match hand_twins {
                        HandTwins::Full => {}
                        HandTwins::WeaponHand => out.warnings.push(
                            "hand twins on the armB (weapon) hand only (entry budget)".into(),
                        ),
                        HandTwins::None => out.warnings.push(
                            "hand prims single-sided (entry budget; fists may cull edge-on)".into(),
                        ),
                    }
                    return Ok(out);
                }
                Err(e) => last_err = Some(e),
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("playerize failed")))
        .context("rebuilt player file does not fit its PROT entry at any texture resolution")
}

#[allow(clippy::too_many_arguments)]
fn playerize_scaled(
    player_file: &[u8],
    entry_len: usize,
    rig: &PlayerRig,
    archive_entry: &[u8],
    source_id: u16,
    texture_downscale: u32,
    hand_twins: HandTwins,
    fusions: &super::weapon_fuse::WeaponFusion,
    weapon_cols: &[u16],
    host_anims: &[legaia_actor_formats::monster_archive::MonsterAnimation],
    char_slot: usize,
    keep_welded_fist: bool,
) -> Result<PlayerizedFile> {
    let mut warnings = Vec::new();
    if texture_downscale > 1 {
        warnings.push(format!(
            "texture at 1/{texture_downscale} resolution to fit the player file"
        ));
    }
    let pack = battle_data_pack::parse(player_file).context("parse player battle file")?;

    // Source model: the Delilas mesh + pool + rest pose.
    let mesh = monster_archive::mesh(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let src_tmd = legaia_tmd::parse(mesh.tmd_bytes()).context("delilas TMD")?;
    let mut src_model = decode_model(&src_tmd, mesh.tmd_bytes())?;
    if src_model.len() != CANONICAL_PARTS {
        bail!("monster id {source_id} has {} parts", src_model.len());
    }
    let pool = mesh
        .texture_pool_bytes()
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no texture pool"))?;
    let (cluts, indices, src_width) = monster_pool_texels(pool)?;
    let src_idle = monster_archive::idle_animation(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle"))?;
    let mut src_rest = src_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle empty"))?
        .clone();
    // A sibling whose armB "fist" is a welded weapon (Che's hammer-fist,
    // authored 150 units out and never closing the wrist like a hand)
    // drops the weapon: the armA fist stands in, mirrored, and the host's
    // own equipped weapon takes the hammer's role via `weapon_fuse`.
    // `keep_welded_fist` opts out (Che's hammer kept on the mesh; the
    // caller also suppresses the host weapon fusion so the hand does not
    // carry two weapons).
    if let Some(&(_, welded)) = WELDED_WEAPON_FISTS.iter().find(|(id, _)| *id == source_id) {
        if keep_welded_fist {
            warnings.push(format!(
                "welded weapon fist (canonical {welded}) kept on the mesh"
            ));
        } else {
            mirror_fist(&mut src_model, &src_rest, welded)?;
            warnings.push(format!(
                "welded weapon fist (canonical {welded}) replaced by the mirrored other fist"
            ));
        }
    }

    // Player rest pose + the retail per-channel part anchors.
    let idle = battle_char_assembly::idle_battle_animation(player_file)?
        .ok_or_else(|| anyhow::anyhow!("player file has no idle"))?;
    let dst_rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("player idle empty"))?
        .clone();
    // Player-shaped ankles (mirrored in `winpose` - the two must agree).
    normalize_battle_rest_feet(&mut src_rest, &dst_rest, rig);
    let asm = battle_char_assembly::assemble_character(player_file, &pack, &[0; SECTION_COUNT])?;
    let dst_tmd = legaia_tmd::parse(&asm.tmd)?;
    let dst_model = decode_model(&dst_tmd, &asm.tmd)?;

    // Bake each canonical part into its player channel's rest frame,
    // pivot-anchored (`bake_object_pivot`): the pivot is the joint the
    // engine rotates the part about, the bone frames re-aim the part
    // from the sibling's rest stance onto the player's (twist pinned by
    // the bend plane), and the axial scale puts the part's far end on
    // the player's child joint - so the player's OWN clips (run / arts
    // / block / spirit) move each baked part exactly like the retail
    // geometry it replaced, joints staying closed. The radial scale is
    // UNIFORM (whole-rig height ratio) so the sibling's shapes survive.
    let skeleton = dst_rest.len();
    let dst_stats: Vec<PartStats> = (0..CANONICAL_PARTS)
        .map(|c| {
            let ch = rig.channel_for_canonical[c] as usize;
            let dst_core = dst_model
                .get(ch)
                .filter(|_| ch < skeleton)
                .ok_or_else(|| anyhow::anyhow!("player model missing channel {ch}"))?;
            let dst_pose = dst_rest
                .get(ch)
                .ok_or_else(|| anyhow::anyhow!("player rest pose missing channel {ch}"))?;
            Ok(part_world_stats(dst_core, dst_pose))
        })
        .collect::<Result<_>>()?;
    let src_stats: Vec<PartStats> = src_model
        .iter()
        .enumerate()
        .map(|(c, o)| part_world_stats(o, &src_rest[c]))
        .collect();
    let radial = global_height_scale(&src_stats, &dst_stats)[0];
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_pivots: Vec<[f32; 3]> = src_rest
        .iter()
        .take(CANONICAL_PARTS)
        .map(pivot_of)
        .collect();
    let dst_pivots: Vec<[f32; 3]> = (0..CANONICAL_PARTS)
        .map(|c| pivot_of(&dst_rest[rig.channel_for_canonical[c] as usize]))
        .collect();
    let (src_frames, dst_frames) = bake_frames(
        &src_pivots,
        &dst_pivots,
        &CANONICAL_CHILD,
        &CANONICAL_PARENT,
    );
    let mut baked: Vec<ModelObject> = Vec::with_capacity(CANONICAL_PARTS);
    for (c, src_obj) in src_model.iter().enumerate() {
        let ch = rig.channel_for_canonical[c] as usize;
        let mut o = src_obj.clone();
        // The torso keeps the AXIAL fit like every chained part: its
        // bone spans torso->head pivot, and the host's head + shoulder
        // pivots are authored at that span's end - a uniform-scaled
        // long torso (Che: 164 vs Gala's 90) towers past them, so the
        // head pokes out mid-hump and the shoulder pads ride above the
        // head (the in-game screenshot). The axial crush is the price
        // of attachment; width stays radial.
        let pb = pivot_bake_params(&src_frames[c], &dst_frames[c], radial);
        bake_object_pivot(&mut o, &src_rest[c], src_pivots[c], &dst_rest[ch], &pb)
            .with_context(|| format!("bake canonical part {c}"))?;
        if c == 0 {
            // Seat the head on the neck: its near edge along the bone
            // axis lands where the replaced head's sat.
            if let Some(dst_head) = dst_model.get(ch) {
                seat_terminal_axial(&mut o, dst_head, &dst_rest[ch], pb.x_dst)?;
            }
        }
        if c == 5 || c == 8 {
            // Seat each HAND by centroid match: translate the baked fist
            // so its local centroid lands where the replaced hand's was.
            // A sibling's hand can be authored far from its own pivot
            // (Gi's armA fist sits 60 units out - his rig swings it from
            // up the arm; Che's hammer-fist 150), and the player channels
            // anchor the pivot AT the wrist, so an unseated fist floats a
            // forearm-length off the arm (the move-input floater in the
            // `delilas_gi_move_input_hand_gap` catalogue state, pinned to
            // the armA hand object by live bisection). The head's axial
            // near-edge seat is the wrong instrument here (measured: it
            // slides Gi's fist further out) - a hand is a closed blob
            // whose only seam is the wrist, and matching centroids puts
            // it exactly in the local region the retail hand occupied
            // under every clip. Feet stay unseated: their ankles go
            // through `normalize_battle_rest_feet` and re-seating would
            // fight that alignment.
            if let Some(dst_part) = dst_model.get(ch) {
                let cen = |obj: &ModelObject| -> Option<[f32; 3]> {
                    if obj.vertices.is_empty() {
                        return None;
                    }
                    let n = obj.vertices.len() as f32;
                    let s = obj.vertices.iter().fold([0f32; 3], |a, v| {
                        [a[0] + v[0] as f32, a[1] + v[1] as f32, a[2] + v[2] as f32]
                    });
                    Some([s[0] / n, s[1] / n, s[2] / n])
                };
                if let (Some(cb), Some(cd)) = (cen(&o), cen(dst_part)) {
                    let shift = [cd[0] - cb[0], cd[1] - cb[1], cd[2] - cb[2]];
                    for v in o.vertices.iter_mut() {
                        *v = [
                            round_coord(v[0] as f32 + shift[0])?,
                            round_coord(v[1] as f32 + shift[1])?,
                            round_coord(v[2] as f32 + shift[2])?,
                        ];
                    }
                }
            }
            // FK inset on top of the centroid seat: the seat puts the
            // fist in the right region; this closes the wrist SEAM the
            // way the host's own hand does, across the host's clips
            // (see fk_inset_hand).
            let cf = rig.channel_for_canonical[c - 1] as usize;
            if let (Some(fore), Some(dst_f), Some(dst_h)) =
                (baked.get(c - 1), dst_model.get(cf), dst_model.get(ch))
                && let Some(d) =
                    fk_inset_hand(&mut o, fore, dst_f, dst_h, cf, ch, host_anims, 12.0)?
            {
                warnings.push(format!(
                    "hand {c}: FK inset ({:+.0},{:+.0},{:+.0}) closes the wrist seam",
                    d[0], d[1], d[2]
                ));
            }
            // The sibling fists are OPEN SHELLS authored for the enemy
            // camera's angles (the body parts survive as closed tubes);
            // seen from the player side they cull to one visible side -
            // Gi's hand vanishes edge-on, Che's hammer shows holes. A
            // winding-reversed twin per prim keeps one of the pair alive
            // from every angle (same treatment the field bake gives the
            // NPC meshes). First repair inconsistent winding in place
            // (free): a topologically closed fist with minority faces
            // wound inward shows holes no twin budget is needed for.
            repair_winding(&mut o);
            match hand_twins {
                HandTwins::Full => double_side(&mut o),
                HandTwins::WeaponHand if c == 8 => double_side(&mut o),
                HandTwins::WeaponHand | HandTwins::None => {}
            }
        }
        compact_object(&mut o);
        baked.push(o);
    }

    // NB a "buried-head lift" (raise the head when its rest CENTROID
    // sat below the torso top) was tried here and REVERTED: a tall
    // helmet biases the centroid low even when the face is seated
    // correctly, so the lift hoisted Gi's whole head ~25 units off the
    // neck - in-game (where head channels animate) that read as a
    // DETACHED head with sky through the gap. `seat_terminal_axial`
    // alone owns the neck seam.

    // Waist curtain (round-torso siblings only): raise the pelvis's top
    // ring up INSIDE the torso so
    // it forms an inner wall behind the torso's bottom rim. A round
    // sibling torso's rim curves high at the sides while the baked
    // pelvis stays narrow and flat-topped, and a low command-ring camera
    // looking up sees sky through the side wedges (Che's belly hole).
    // The raised ring is invisible - it climbs into the torso's
    // interior - and any sight line that passes under the rim now hits
    // the pelvis wall instead of the sky. Sized from measurement: per
    // angular sector around the waist axis, the pelvis top must clear
    // the torso's bottom edge by a tuck margin; the worst deficit sets
    // one uniform raise (per-sector raises would ripple the loincloth).
    // Gated by id: the sector metric over-fires on the slim siblings
    // (sparse side sectors read as huge deficits), and on Gi the cone
    // showed as a pants-coloured panel over the jacket. Che's round
    // boulder torso over a narrow loincloth is the construction that
    // opens sky wedges, and his pelvis texels (belly skin + loincloth)
    // read naturally where the cone shows.
    if WAIST_CURTAIN_IDS.contains(&source_id) {
        const SECTORS: usize = 16;
        const TUCK: f32 = 10.0;
        const MIN_RAISE: f32 = 4.0;
        let ch_t = rig.channel_for_canonical[1] as usize; // torso
        let ch_p = rig.channel_for_canonical[2] as usize; // pelvis
        let ch_h = rig.channel_for_canonical[0] as usize; // head
        let posed = |o: &ModelObject, pose: &PartPose| -> Vec<[f32; 3]> {
            let m = rot_matrix(pose);
            o.vertices
                .iter()
                .map(|v| {
                    let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
                    [
                        w[0] + pose.tx as f32,
                        w[1] + pose.ty as f32,
                        w[2] + pose.tz as f32,
                    ]
                })
                .collect()
        };
        let up = {
            let d = vsub(pivot_of(&dst_rest[ch_h]), pivot_of(&dst_rest[ch_p]));
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0);
            [d[0] / n, d[1] / n, d[2] / n]
        };
        let center = pivot_of(&dst_rest[ch_t]);
        let tmp = if up[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let e1 = {
            let c = [
                up[1] * tmp[2] - up[2] * tmp[1],
                up[2] * tmp[0] - up[0] * tmp[2],
                up[0] * tmp[1] - up[1] * tmp[0],
            ];
            let n = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().max(1e-6);
            [c[0] / n, c[1] / n, c[2] / n]
        };
        let e2 = [
            up[1] * e1[2] - up[2] * e1[1],
            up[2] * e1[0] - up[0] * e1[2],
            up[0] * e1[1] - up[1] * e1[0],
        ];
        let classify = |v: &[f32; 3]| -> (usize, f32) {
            let d = vsub(*v, center);
            let h = d[0] * up[0] + d[1] * up[1] + d[2] * up[2];
            let ang = (d[0] * e2[0] + d[1] * e2[1] + d[2] * e2[2])
                .atan2(d[0] * e1[0] + d[1] * e1[1] + d[2] * e1[2]);
            let sec = (((ang / std::f32::consts::TAU) + 0.5) * SECTORS as f32) as usize % SECTORS;
            (sec, h)
        };
        let torso_posed = posed(&baked[1], &dst_rest[ch_t]);
        let pelvis_posed = posed(&baked[2], &dst_rest[ch_p]);
        let mut rim = [f32::MAX; SECTORS];
        let mut top = [f32::MIN; SECTORS];
        for v in &torso_posed {
            let (sec, h) = classify(v);
            if h < rim[sec] {
                rim[sec] = h;
            }
        }
        for v in &pelvis_posed {
            let (sec, h) = classify(v);
            if h > top[sec] {
                top[sec] = h;
            }
        }
        let mut deficit = 0f32;
        for sec in 0..SECTORS {
            if rim[sec] == f32::MAX || top[sec] == f32::MIN {
                continue; // sector one part never reaches - no sight line issue
            }
            deficit = deficit.max(rim[sec] + TUCK - top[sec]);
        }
        let deficit = deficit.min(60.0);
        if deficit > MIN_RAISE {
            // Raise the top band of the pelvis (upper 40% along up) by
            // the deficit, tapered to zero at the band's lower edge -
            // and CONE IT INWARD as it rises (top ring radius halved):
            // the torso's surface slopes inward going up, so a straight
            // vertical wall pokes out of the front; a cone stays inside
            // the round interior while still blocking every sight line
            // that slips under the rim. All in pelvis-LOCAL frame.
            let hs: Vec<f32> = pelvis_posed.iter().map(|v| classify(v).1).collect();
            let (h_lo, h_hi) = hs
                .iter()
                .fold((f32::MAX, f32::MIN), |a, &h| (a.0.min(h), a.1.max(h)));
            let band0 = h_lo + (h_hi - h_lo) * 0.6;
            let mp = rot_matrix(&dst_rest[ch_p]);
            let lup = apply_transposed(&mp, up);
            let laxis_pt = apply_transposed(&mp, vsub(center, pivot_of(&dst_rest[ch_p])));
            for (vi, v) in baked[2].vertices.iter_mut().enumerate() {
                let w = ((hs[vi] - band0) / (h_hi - band0).max(1.0)).clamp(0.0, 1.0);
                if w <= 0.0 {
                    continue;
                }
                let p0 = [v[0] as f32, v[1] as f32, v[2] as f32];
                // radial part = offset from the waist axis (local frame),
                // minus its along-up component.
                let rel = vsub(p0, laxis_pt);
                let along = rel[0] * lup[0] + rel[1] * lup[1] + rel[2] * lup[2];
                let radial = [
                    rel[0] - lup[0] * along,
                    rel[1] - lup[1] * along,
                    rel[2] - lup[2] * along,
                ];
                let shrink = 0.5 * w; // top ring pulls halfway to the axis
                let d = deficit * w;
                *v = [
                    round_coord(p0[0] + lup[0] * d - radial[0] * shrink)?,
                    round_coord(p0[1] + lup[1] * d - radial[1] * shrink)?,
                    round_coord(p0[2] + lup[2] * d - radial[2] * shrink)?,
                ];
            }
            warnings.push(format!(
                "waist curtain: pelvis top coned {deficit:.0} up into the torso"
            ));
        }
    }

    // Hip-clearance taper: shrink each thigh's radial extent near the
    // hip (12% at the pivot, fading to zero 60% down the bone) so the
    // thigh's upper rim stops cutting through the pelvis skirt when the
    // host's clips swing the leg (Lu's thigh clipped through her rear).
    for c in [9usize, 12usize] {
        let Some(len) = dst_frames[c].len.filter(|l| *l >= 2.0) else {
            continue;
        };
        let ch = rig.channel_for_canonical[c] as usize;
        let md = rot_matrix(&dst_rest[ch]);
        let ax = apply_transposed(&md, dst_frames[c].axes[0]);
        for v in baked[c].vertices.iter_mut() {
            let p = [v[0] as f32, v[1] as f32, v[2] as f32];
            let t = p[0] * ax[0] + p[1] * ax[1] + p[2] * ax[2];
            let w = (1.0 - t / (0.6 * len)).clamp(0.0, 1.0) * 0.12;
            let perp = [p[0] - ax[0] * t, p[1] - ax[1] * t, p[2] - ax[2] * t];
            *v = [
                round_coord(p[0] - perp[0] * w)?,
                round_coord(p[1] - perp[1] * w)?,
                round_coord(p[2] - perp[2] * w)?,
            ];
        }
    }

    // Seat the upper arms on the sibling's OWN shoulder sockets. The
    // arms hang at the PLAYER's shoulder pivots (the clips' channel
    // translations dictate that), but the baked torso's socket surface
    // lands where the SIBLING's shoulder sits after the torso bake -
    // on a narrower rig the arm floats outboard of the body (Lu's
    // shoulder gap: her torso wraps her sockets at ~4 units, the baked
    // pair left ~26). The shoulder END of each upper arm tucks in by
    // the socket delta, TAPERING to zero at the elbow: a rigid
    // whole-chain shift instead makes the per-part offsets rotate
    // apart mid-swing and opens the elbow/wrist, so the elbow end (and
    // the forearm + hand parts) must stay exactly where the clips
    // expect them.
    {
        let pb_torso = pivot_bake_params(&src_frames[1], &dst_frames[1], radial);
        for c in [3usize, 6usize] {
            let socket = bake_point_pivot(src_pivots[c], src_pivots[1], dst_pivots[1], &pb_torso);
            let delta = vsub(socket, dst_pivots[c]);
            let Some(len) = dst_frames[c].len.filter(|l| *l >= 2.0) else {
                continue;
            };
            let ch = rig.channel_for_canonical[c] as usize;
            let md = rot_matrix(&dst_rest[ch]);
            let ld = apply_transposed(&md, delta);
            let ax = apply_transposed(&md, dst_frames[c].axes[0]);
            for v in baked[c].vertices.iter_mut() {
                let t = v[0] as f32 * ax[0] + v[1] as f32 * ax[1] + v[2] as f32 * ax[2];
                let w = (1.0 - t / len).clamp(0.0, 1.0);
                *v = [
                    round_coord(v[0] as f32 + ld[0] * w)?,
                    round_coord(v[1] as f32 + ld[1] * w)?,
                    round_coord(v[2] as f32 + ld[2] * w)?,
                ];
            }
        }
    }

    // Hip-socket tuck - the shoulder tuck's mirror for the legs: the
    // thigh tops ride the HOST's hip pivots, which sit wider than a
    // slimmer sibling's authored hips, so the thigh's upper rim pokes
    // through the pelvis skirt from OUTSIDE (Lu's in-game hip overlap).
    // Each thigh's hip end tucks to where the sibling's own hip socket
    // lands through the pelvis bake, tapering to zero at the knee (the
    // knee/shin/foot stay exactly where the clips expect them).
    {
        let pb_pelvis = pivot_bake_params(&src_frames[2], &dst_frames[2], radial);
        for c in [9usize, 12usize] {
            let socket = bake_point_pivot(src_pivots[c], src_pivots[2], dst_pivots[2], &pb_pelvis);
            let delta = vsub(socket, dst_pivots[c]);
            let Some(len) = dst_frames[c].len.filter(|l| *l >= 2.0) else {
                continue;
            };
            let ch = rig.channel_for_canonical[c] as usize;
            let md = rot_matrix(&dst_rest[ch]);
            let ld = apply_transposed(&md, delta);
            let ax = apply_transposed(&md, dst_frames[c].axes[0]);
            for v in baked[c].vertices.iter_mut() {
                let t = v[0] as f32 * ax[0] + v[1] as f32 * ax[1] + v[2] as f32 * ax[2];
                let w = (1.0 - t / len).clamp(0.0, 1.0);
                *v = [
                    round_coord(v[0] as f32 + ld[0] * w)?,
                    round_coord(v[1] as f32 + ld[1] * w)?,
                    round_coord(v[2] as f32 + ld[2] * w)?,
                ];
            }
        }
    }

    // Texture re-layout (rewrites the baked objects' UV/CBA/TSB).
    let mut reserved: Vec<u16> = record0_texture_uploads(player_file, 0)?
        .iter()
        .filter(|u| !u.clut.is_empty())
        .flat_map(|u| {
            let first = u.clut_x / 16;
            let n_cols = (u.clut.len() as u16).div_ceil(16);
            (first..first + n_cols).collect::<Vec<u16>>()
        })
        .collect();
    reserved.extend_from_slice(weapon_cols);
    let layout = relayout_to_band(
        &mut baked,
        &cluts,
        &indices,
        src_width,
        &reserved,
        fusions.weapon_section,
        texture_downscale,
        char_slot,
        &mut warnings,
    )?;

    // Per-channel geometry map.
    let mut channel_geom: BTreeMap<u8, ModelObject> = BTreeMap::new();
    for (c, o) in baked.into_iter().enumerate() {
        channel_geom.insert(rig.channel_for_canonical[c], o);
    }
    // The hair channel (Noa) intentionally has no entry - it goes empty.

    // Rewrite every record except record[0].
    let table_offset = pack.table_offset;
    // (id, slot bytes, decoded bytes - kept for the optimal-LZS retry)
    let mut new_records: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut section = 0usize;
    // Streamer-anchor latch: the first variant-carrying section.
    let mut anchor_section: Option<usize> = None;
    for (idx, rec) in pack.records.iter().enumerate() {
        let decoded = decode_record(player_file, &pack, idx)
            .with_context(|| format!("decode record {idx}"))?;
        let shared_block = layout
            .section_pools
            .get(section)
            .ok_or_else(|| anyhow::anyhow!("record {idx}: section {section} out of range"))?;
        // Weapon-section records keep their OWN retail tile pixels - each
        // record's fixed-size upload repaints the tile with its weapon's
        // texels at the UVs the fused prims already carry - and install
        // the weapon's palettes at the reserved columns via their own
        // CLUT run. The sibling never samples this tile (the relayout
        // excluded it), so per-record content is safe. A record without
        // a retail upload (some defaults) falls back to the shared
        // (empty) sibling block.
        let weapon_block;
        let pool_block: &[u8] = if Some(section) == fusions.weapon_section
            && let Some(up) = legaia_actor_formats::battle_char_assembly::section_texture_upload(
                &decoded.bytes,
                section,
                0,
            )
            .with_context(|| format!("record {idx}: retail pool"))?
        {
            let mut block = Vec::new();
            match fusions.palettes.get(&(section, rec.id)) {
                Some(pals) if !weapon_cols.is_empty() => {
                    let lo = *weapon_cols.iter().min().unwrap();
                    let hi = *weapon_cols.iter().max().unwrap();
                    let n = (hi - lo + 1) * 16;
                    block.extend_from_slice(&(lo * 16).to_le_bytes());
                    block.extend_from_slice(&n.to_le_bytes());
                    let mut run = vec![0u16; n as usize];
                    for (i, pal) in pals.iter().enumerate() {
                        let base = ((weapon_cols[i] - lo) * 16) as usize;
                        run[base..base + 16].copy_from_slice(pal);
                    }
                    for w in run {
                        block.extend_from_slice(&w.to_le_bytes());
                    }
                }
                _ => {
                    block.extend_from_slice(&0u16.to_le_bytes());
                    block.extend_from_slice(&0u16.to_le_bytes());
                }
            }
            block.extend_from_slice(&up.pixels);
            weapon_block = block;
            &weapon_block
        } else {
            shared_block
        };
        // Held-item records get the host's own weapon fused into the baked
        // hand geometry (see `weapon_fuse`) - after the seat + FK inset,
        // so the blade never drags the fist's placement.
        let fused_geom;
        let geom = match fusions.per_record.get(&(section, rec.id)) {
            Some(per_channel) => {
                let mut g = channel_geom.clone();
                for (ch, add) in per_channel {
                    if let Some(dst) = g.get_mut(ch) {
                        super::weapon_fuse::merge_into(dst, add);
                    }
                }
                fused_geom = g;
                &fused_geom
            }
            None => &channel_geom,
        };
        let rewritten = rewrite_section_record(
            &decoded.bytes,
            geom,
            pool_block,
            section,
            &mut anchor_section,
        )
        .with_context(|| format!("rewrite record {idx} (id {:#x})", rec.id))?;
        new_records.push((rec.id, rewritten));
        if rec.id == 0 {
            section += 1;
        }
    }
    if section != SECTION_COUNT {
        bail!("descriptor chain closed after {section} sections");
    }
    let file = rebuild_player_file(
        player_file,
        table_offset,
        pack.data_base,
        new_records,
        entry_len,
    )?;
    Ok(PlayerizedFile { file, warnings })
}
