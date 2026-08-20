//! Enemy-side animation mirror: the swapped Delilas monster blocks
//! (`--delilas-party` re-skins 162/163/164 with the mapped hero's battle
//! model) get the HERO's own clips, retargeted onto the monster rig, so
//! the Nivora ravine duels fight a hero who moves like the hero.
//!
//! Three coordinated pieces:
//!
//! 1. **The shared bake context** ([`monster_bake_ctx`]). The enemy-side
//!    mesh bake (`monsterize_player`) and this module's clip retarget are
//!    a conjugate pair, exactly like `playerize` / `winpose` on the
//!    player side: the played pose cancels the bake's `R_align`, so both
//!    must build their frames the same way (`playerize::bake_frames`, the
//!    whole-rig re-face + minimal swing that fixed the per-part roll
//!    artifacts) over the same normalized rest, or the conjugation stops
//!    cancelling by exactly the difference.
//! 2. **The hero→monster clip retarget** ([`HeroRetarget`]) - the inverse
//!    direction of `winpose::retarget_clip`: a clip that poses the hero's
//!    player channels is converted into canonical monster-part space,
//!    rotations by the per-part conjugation, translations rebuilt by
//!    forward kinematics over the monster rig's own rest joint spacing
//!    (the bake anchors every part at the monster's rest pivots and
//!    applies no socket tucks, so at destination-rest rotations the FK
//!    reduces exactly to the monster's retail pivots).
//! 3. **In-place entry rewriting** ([`mirror_block_animations`]). The
//!    monster archive's per-action entries are rebuilt inside the decoded
//!    block: entry heads survive from the retail block (tags, AGL costs,
//!    effect indices, root-motion words, sound cues), frame-indexed head
//!    fields (event-frame list, effect-script gates, the `+0x84..+0x86`
//!    loop window) are rescaled to the new stream length, and the packed
//!    keyframe stream is replaced. The staged special entries (raw
//!    indices the per-spell cast module writes into `actor[+0x1DA]` -
//!    `docs/formats/monster-animation.md` § "A special attack can be a
//!    chain of entries") keep their indices and honour per-entry frame
//!    floors ([`PAYOFF_FLOOR_FRAMES`] where a cursor gate is measured,
//!    [`RETAIL_STAGED_FLOOR`] elsewhere), stretch-resampled with the
//!    rate byte adjusted so `frames * 8 / rate` (the retail tick
//!    formula) preserves each stage's authored duration.
//!
//! The monster stream encoding is the **raw** 9-byte packed record family
//! (`[u8 parts][u8 frames][frames×parts×9]`, `FUN_8004998C`) - the same
//! flat family as the player record[0] streams and NOT the readef ME
//! archives' channel-delta codec. Measured on the disc: every entry of
//! blocks 162/163/164 spans exactly `0x8E + parts*frames*9` bytes and the
//! `+0x4C` offsets are ascending-contiguous over that reading (the
//! disc-gated mirror test re-asserts it).

use super::*;
use crate::battle_char_assembly as bca;
use crate::battle_char_assembly::swing_battle_animations;
use crate::monster_archive::MonsterAnimation;
use std::collections::BTreeSet;
use winpose::{mmul, to_euler, transpose};

/// Keyframe floor for a staged entry that plays during a measured
/// cursor gate. Module 0960 (Lu, spell `0x7B`) has a damage tick that
/// waits for the CASTER's clip cursor to reach `0x160` sixteenths
/// (keyframe 22), so the payoff stage it binds must carry at least 23
/// keyframes.
pub const PAYOFF_FLOOR_FRAMES: usize = 23;

/// Keyframe floor for staged entries with no measured cursor gate:
/// retail's own smallest module-staged entry (Gi's 11-frame crouch,
/// block 162 entry 10). Module 0959 carries no `slti` cursor test at
/// all; 0958 is unmeasured and is held at the retail floor.
pub const RETAIL_STAGED_FLOOR: usize = 11;

/// The staged anim id of each hero slot's 50-AP Hyper art (the move the
/// swapped enemy's signature cast is renamed to): bank record index =
/// `anim_id - 0x10` (`FUN_8004AD80`). Vahn Burning Flare / Gala Explosive
/// Fist share action constant `0x1C`; Noa Vulture Blade is `0x1F`. The
/// record0 inline names are dev labels ("Fiery Miyawaki", "Double
/// 4-Punch") - the anim id, not the name, is the stable key.
pub const HYPER_ANIM_ID: [u8; 3] = [0x1C, 0x1F, 0x1C];

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

// ---------------------------------------------------------------------------
// Hero -> monster clip retarget.

struct Carried {
    part: usize,
    root: usize,
    /// The monster's own rest offset, in the root's rest frame.
    rest_local: [f32; 3],
    /// The player's rest offset, in the player root's rest frame.
    src_rest_local: [f32; 3],
    /// Maps a player-authored root-local displacement onto the monster.
    transfer: [[f32; 3]; 3],
}

struct ChainFk {
    chain: [usize; 3],
    root: usize,
    /// The chain root's rest offset from its carrier, in the carrier's
    /// monster rest frame. The enemy-side bake anchors every part at the
    /// monster's own rest pivots and applies NO socket tuck, so the
    /// baked chain-root geometry hangs at the monster's rest socket -
    /// which is exactly where the FK has to put the pivot.
    socket_local: [f32; 3],
    /// Bone vectors (joint-to-joint) in each carrying part's monster
    /// rest frame.
    bv_local: [[f32; 3]; 2],
}

/// Converts clips that pose the HERO's player channels into canonical
/// monster-part space - the inverse direction of `winpose::retarget_clip`,
/// built over the same [`MonsterBakeCtx`] the mesh bake reads so the
/// per-part conjugation cancels the bake exactly.
pub(crate) struct HeroRetarget {
    rig: PlayerRig,
    radial: f32,
    /// Per canonical part: `A = R_p_rest^T * R_align^T * R_m_rest`, so a
    /// hero channel pose `R_h` plays the baked part as `R_h * A`.
    conj: Vec<[[f32; 3]; 3]>,
    carried: Vec<Carried>,
    chains: Vec<ChainFk>,
}

impl HeroRetarget {
    pub(crate) fn new(ctx: &MonsterBakeCtx, rig: &PlayerRig) -> Self {
        let conj: Vec<[[f32; 3]; 3]> = (0..CANONICAL_PARTS)
            .map(|c| {
                let ch = rig.channel_for_canonical[c] as usize;
                let r_align = frame_align(&ctx.src_frames[c], &ctx.dst_frames[c]);
                let a = mmul(&transpose(&rot_matrix(&ctx.rest[ch])), &transpose(&r_align));
                mmul(&a, &rot_matrix(&ctx.target_rest[c]))
            })
            .collect();
        let m_rest_local = |part: usize, root: usize| -> [f32; 3] {
            apply_transposed(
                &rot_matrix(&ctx.target_rest[root]),
                vsub(ctx.dst_pivots[part], ctx.dst_pivots[root]),
            )
        };
        let p_rest_local = |part: usize, root: usize| -> [f32; 3] {
            let rch = rig.channel_for_canonical[root] as usize;
            apply_transposed(
                &rot_matrix(&ctx.rest[rch]),
                vsub(ctx.src_pivots[part], ctx.src_pivots[root]),
            )
        };
        let carried = [(0usize, 1usize), (2, 1)]
            .iter()
            .map(|&(part, root)| Carried {
                part,
                root,
                rest_local: m_rest_local(part, root),
                src_rest_local: p_rest_local(part, root),
                transfer: transpose(&conj[root]),
            })
            .collect();
        let chains = [
            ([3usize, 4, 5], 1usize),
            ([6, 7, 8], 1),
            ([9, 10, 11], 2),
            ([12, 13, 14], 2),
        ]
        .iter()
        .map(|&(chain, root)| ChainFk {
            chain,
            root,
            socket_local: m_rest_local(chain[0], root),
            bv_local: [
                m_rest_local(chain[1], chain[0]),
                m_rest_local(chain[2], chain[1]),
            ],
        })
        .collect();
        HeroRetarget {
            rig: *rig,
            radial: ctx.radial,
            conj,
            carried,
            chains,
        }
    }

    /// Retarget hero-channel pose rows into canonical monster-part rows
    /// (same frame count; resample the source first if needed).
    pub(crate) fn retarget_frames(&self, frames: &[Vec<PartPose>]) -> Vec<Vec<PartPose>> {
        let mut out = Vec::with_capacity(frames.len());
        for sf in frames {
            let hero = |ch: usize| sf.get(ch).copied().unwrap_or_default();
            let mut row = vec![PartPose::default(); CANONICAL_PARTS];
            for (c, slot) in row.iter_mut().enumerate() {
                let pose = hero(self.rig.channel_for_canonical[c] as usize);
                let r = mmul(&rot_matrix(&pose), &self.conj[c]);
                let (rx, ry, rz) = to_euler(&r);
                let t = |v: i16| ((v as f32) * self.radial).round().clamp(-2048.0, 2047.0) as i16;
                *slot = PartPose {
                    tx: t(pose.tx),
                    ty: t(pose.ty),
                    tz: t(pose.tz),
                    rx,
                    ry,
                    rz,
                };
            }
            let set = |p: &mut PartPose, w: [f32; 3]| {
                p.tx = w[0].round().clamp(-2048.0, 2047.0) as i16;
                p.ty = w[1].round().clamp(-2048.0, 2047.0) as i16;
                p.tz = w[2].round().clamp(-2048.0, 2047.0) as i16;
            };
            let world = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
            // Carried parts first (the leg chains socket onto the pelvis).
            for cr in &self.carried {
                let root_ch = self.rig.channel_for_canonical[cr.root] as usize;
                let part_ch = self.rig.channel_for_canonical[cr.part] as usize;
                let (hp, hr) = (hero(part_ch), hero(root_ch));
                // This frame's deviation from the player's rest
                // attachment, in the player root's local frame.
                let u = apply_transposed(
                    &rot_matrix(&hr),
                    vsub(
                        [hp.tx as f32, hp.ty as f32, hp.tz as f32],
                        [hr.tx as f32, hr.ty as f32, hr.tz as f32],
                    ),
                );
                let dev = apply(&cr.transfer, vsub(u, cr.src_rest_local));
                let local = [
                    cr.rest_local[0] + dev[0] * self.radial,
                    cr.rest_local[1] + dev[1] * self.radial,
                    cr.rest_local[2] + dev[2] * self.radial,
                ];
                let o = apply(&rot_matrix(&row[cr.root]), local);
                let t = world(&row[cr.root]);
                let target = [t[0] + o[0], t[1] + o[1], t[2] + o[2]];
                set(&mut row[cr.part], target);
            }
            for fk in &self.chains {
                let rt = world(&row[fk.root]);
                let s = apply(&rot_matrix(&row[fk.root]), fk.socket_local);
                let mut pos = [rt[0] + s[0], rt[1] + s[1], rt[2] + s[2]];
                set(&mut row[fk.chain[0]], pos);
                let b0 = apply(&rot_matrix(&row[fk.chain[0]]), fk.bv_local[0]);
                pos = [pos[0] + b0[0], pos[1] + b0[1], pos[2] + b0[2]];
                set(&mut row[fk.chain[1]], pos);
                let b1 = apply(&rot_matrix(&row[fk.chain[1]]), fk.bv_local[1]);
                pos = [pos[0] + b1[0], pos[1] + b1[1], pos[2] + b1[2]];
                set(&mut row[fk.chain[2]], pos);
            }
            out.push(row);
        }
        out
    }
}

/// Linear pose resampling: `out` frames over the input's full span,
/// translations lerped, rotations shortest-path lerped. Endpoint
/// preserving (frame 0 and the last frame survive exactly).
pub(crate) fn resample_poses(frames: &[Vec<PartPose>], out_frames: usize) -> Vec<Vec<PartPose>> {
    let n = frames.len();
    if n == 0 || out_frames == 0 {
        return Vec::new();
    }
    let lerp_angle = |a: u16, b: u16, t: f32| -> u16 {
        let mut d = (b as i32 - a as i32).rem_euclid(4096);
        if d > 2048 {
            d -= 4096;
        }
        ((a as i32 + (d as f32 * t).round() as i32).rem_euclid(4096)) as u16
    };
    let lerp_i =
        |a: i16, b: i16, t: f32| -> i16 { (a as f32 + (b as f32 - a as f32) * t).round() as i16 };
    (0..out_frames)
        .map(|j| {
            let pos = if out_frames == 1 {
                0.0
            } else {
                j as f32 * (n as f32 - 1.0) / (out_frames as f32 - 1.0)
            };
            let i0 = (pos.floor() as usize).min(n - 1);
            let i1 = (i0 + 1).min(n - 1);
            let t = pos - i0 as f32;
            let (fa, fb) = (&frames[i0], &frames[i1]);
            (0..fa.len().min(fb.len()))
                .map(|p| PartPose {
                    tx: lerp_i(fa[p].tx, fb[p].tx, t),
                    ty: lerp_i(fa[p].ty, fb[p].ty, t),
                    tz: lerp_i(fa[p].tz, fb[p].tz, t),
                    rx: lerp_angle(fa[p].rx, fb[p].rx, t),
                    ry: lerp_angle(fa[p].ry, fb[p].ry, t),
                    rz: lerp_angle(fa[p].rz, fb[p].rz, t),
                })
                .collect()
        })
        .collect()
}

/// The single whole-body translation that seats the retargeted hero
/// streams on the monster block's own rest - the canonical-space mirror
/// of `winpose::idle_anchor`: x/z from the torso (canonical part 1, the
/// FK root), y from the DEEPEST ankle pivot over both cycles (canonical
/// feet 11/14; GTE y-down, so the largest `ty` is the floor). Battle
/// poses are flat absolute transforms, so this MUST be applied rigidly to
/// every part of every frame.
pub(crate) fn monster_anchor(rows: &[Vec<PartPose>], host: &[Vec<PartPose>]) -> [i16; 3] {
    let floor = |frames: &[Vec<PartPose>]| -> Option<i16> {
        frames
            .iter()
            .filter_map(|f| {
                [11usize, 14]
                    .iter()
                    .filter_map(|&c| f.get(c))
                    .map(|p| p.ty)
                    .max()
            })
            .max()
    };
    let (Some(first), Some(rest)) = (
        rows.first().and_then(|f| f.get(1)),
        host.first().and_then(|f| f.get(1)),
    ) else {
        return [0; 3];
    };
    let dy = match (floor(rows), floor(host)) {
        (Some(a), Some(b)) => b - a,
        _ => rest.ty - first.ty,
    };
    [rest.tx - first.tx, dy, rest.tz - first.tz]
}

fn apply_anchor(rows: &mut [Vec<PartPose>], d: [i16; 3]) {
    let put = |v: i16, d: i16| (v as i32 + d as i32).clamp(-2048, 2047) as i16;
    for row in rows.iter_mut() {
        for p in row.iter_mut() {
            p.tx = put(p.tx, d[0]);
            p.ty = put(p.ty, d[1]);
            p.tz = put(p.tz, d[2]);
        }
    }
}

// ---------------------------------------------------------------------------
// In-place entry rewriting.

/// One rewritten entry: the retargeted rows and the rate byte that keeps
/// the source clip's wall-clock duration (`frames * 8 / rate`).
pub(crate) struct EntryRewrite {
    pub rows: Vec<Vec<PartPose>>,
    pub rate: u8,
}

const COUNT_OFF: usize = 0x4A;
const ARRAY_OFF: usize = 0x4C;
const ENTRY_STREAM_OFF: usize = 0x8C;
const TABLE_WORD_BIAS: usize = 0x12;

struct EntrySpan {
    off: usize,
    span: usize,
    parts: usize,
    frames: usize,
}

/// Parse a block's entry table, requiring the ascending-contiguous layout
/// (the same law `slim` and `replace_mesh_and_pool` verify).
fn parse_entry_table(block: &[u8]) -> Result<(Vec<EntrySpan>, usize, usize, usize)> {
    let pool_off = legaia_bytes::u32_le(block, 8).context("pool offset")? as usize;
    if pool_off == 0 || pool_off > block.len() {
        bail!("texture-pool offset {pool_off:#x} out of range");
    }
    let count = *block
        .get(COUNT_OFF)
        .ok_or_else(|| anyhow::anyhow!("block too short for the entry count"))?
        as usize;
    if count == 0 {
        bail!("block has no action entries");
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let off = legaia_bytes::u32_le(block, ARRAY_OFF + i * 4)
            .with_context(|| format!("entry {i} offset"))? as usize;
        let head = block
            .get(off..off + ENTRY_STREAM_OFF + 2)
            .ok_or_else(|| anyhow::anyhow!("entry {i} at +{off:#x} out of range"))?;
        let parts = head[ENTRY_STREAM_OFF] as usize;
        let frames = head[ENTRY_STREAM_OFF + 1] as usize;
        let span = ENTRY_STREAM_OFF + 2 + frames * parts * 9;
        if off + span > pool_off {
            bail!("entry {i} stream runs past the texture pool");
        }
        entries.push(EntrySpan {
            off,
            span,
            parts,
            frames,
        });
    }
    for w in entries.windows(2) {
        let gap = w[1].off as i64 - (w[0].off + w[0].span) as i64;
        if !(0..=3).contains(&gap) {
            bail!(
                "entries not ascending-contiguous: gap {gap} between +{:#x} and +{:#x}",
                w[0].off,
                w[1].off
            );
        }
    }
    let region_start = entries[0].off;
    let last = entries.last().expect("count >= 1");
    let tail_start = (last.off + last.span).div_ceil(4) * 4;
    if tail_start > pool_off {
        bail!("entry region overlaps the texture pool");
    }
    Ok((entries, region_start, tail_start, pool_off))
}

/// Round `v * new / old` to the nearest frame index.
fn rescale_frame_value(v: usize, old: usize, new: usize) -> usize {
    if old == 0 {
        return v;
    }
    (v * new + old / 2) / old
}

/// Rewrite the frame-indexed fields of a retail entry head for a stream
/// resized `old_frames` → `new_frames`: the playback-rate byte, the
/// `+0x10..+0x13` event-frame list (count preserved - it is how many
/// times damage applies - strictly ascending, capped inside the clip),
/// the `+0x14..` effect-script frame gates, and the `+0x84..+0x86` loop
/// window when the `+0x84` seed is non-zero (a zero-length hold window
/// stays zero-length). Everything else - tag, AGL cost, effect indices,
/// root-motion words, `+0x76`/`+0x77`, the `+0x87` sound cue - survives
/// byte for byte.
fn transform_head(head: &[u8], old_frames: usize, new_frames: usize, rate: u8) -> Vec<u8> {
    let mut h = head[..ENTRY_STREAM_OFF].to_vec();
    h[0x78] = rate;
    // Event-frame list: zero-terminated, strictly ascending.
    let mut prev = 0usize;
    for j in 0..4 {
        let v = h[0x10 + j] as usize;
        if v == 0 {
            break;
        }
        let mut nv = rescale_frame_value(v, old_frames, new_frames).max(prev + 1);
        nv = nv.min(new_frames.saturating_sub(1).max(prev + 1)).min(255);
        h[0x10 + j] = nv as u8;
        prev = nv;
    }
    // Effect-script gates: 8 records at +0x14, gate 0 ends the walk.
    for r in 0..8 {
        let at = 0x14 + r * 8;
        let gate = h[at] as usize;
        if gate == 0 {
            break;
        }
        h[at] = rescale_frame_value(gate, old_frames, new_frames).clamp(1, 255) as u8;
    }
    // Loop window.
    if h[0x84] != 0 {
        let s = h[0x85] as usize;
        let e = h[0x86] as usize;
        let ns = rescale_frame_value(s, old_frames, new_frames).min(new_frames.saturating_sub(1));
        let len = e.saturating_sub(s);
        let nlen = if len == 0 {
            0
        } else {
            rescale_frame_value(len, old_frames, new_frames).max(1)
        };
        h[0x85] = ns.min(255) as u8;
        h[0x86] = (ns + nlen).min(new_frames).min(255) as u8;
    }
    h
}

/// Rebuild `current_block`'s entry region: every entry's head comes from
/// the RETAIL block (so the rewrite is idempotent - a second pass over an
/// already-mirrored block reproduces it byte for byte), streams come from
/// `rewrites` where present and from the retail block otherwise, and the
/// tail + pool of the current block shift by the region's size delta.
pub(crate) fn rebuild_block_entries(
    current_block: &[u8],
    retail_block: &[u8],
    rewrites: &BTreeMap<usize, EntryRewrite>,
) -> Result<Vec<u8>> {
    let (cur, cur_region_start, cur_tail_start, cur_pool_off) = parse_entry_table(current_block)?;
    let (ret, _, _, _) = parse_entry_table(retail_block)?;
    if cur.len() != ret.len() {
        bail!(
            "entry count mismatch: current {} vs retail {}",
            cur.len(),
            ret.len()
        );
    }
    let count = cur.len();
    if let Some(&bad) = rewrites.keys().find(|&&i| i >= count) {
        bail!("rewrite index {bad} out of range ({count} entries)");
    }
    let name_off = legaia_bytes::u32_le(current_block, 0).context("name offset")? as usize;
    let tmd_off = legaia_bytes::u32_le(current_block, 4).context("tmd offset")? as usize;
    if name_off >= cur_region_start || tmd_off >= cur_region_start {
        bail!("name/TMD offsets inside the entry region - layout not understood");
    }

    let mut out = current_block[..cur_region_start].to_vec();
    let mut new_offs = Vec::with_capacity(count);
    for (i, r) in ret.iter().enumerate() {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        new_offs.push(out.len());
        match rewrites.get(&i) {
            Some(rw) => {
                let frames = rw.rows.len();
                if frames == 0 || frames > 255 {
                    bail!("entry {i}: rewritten stream has {frames} frames");
                }
                if rw.rows.iter().any(|row| row.len() != r.parts) {
                    bail!(
                        "entry {i}: rewritten rows do not carry the retail part count {}",
                        r.parts
                    );
                }
                let head = &retail_block[r.off..r.off + ENTRY_STREAM_OFF];
                out.extend_from_slice(&transform_head(head, r.frames, frames, rw.rate));
                out.push(r.parts as u8);
                out.push(frames as u8);
                for row in &rw.rows {
                    for p in row {
                        out.extend_from_slice(&winpose::pack_part(p));
                    }
                }
            }
            None => out.extend_from_slice(&retail_block[r.off..r.off + r.span]),
        }
    }
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let new_tail_start = out.len();
    let delta = new_tail_start as i64 - cur_tail_start as i64;
    out.extend_from_slice(&current_block[cur_tail_start..cur_pool_off]);
    let new_pool_off = out.len();
    out.extend_from_slice(&current_block[cur_pool_off..]);

    // Fixups: pool offset, entry-offset array, effect-table words whose
    // values point into the moved tail (each word visited once).
    out[8..12].copy_from_slice(&(new_pool_off as u32).to_le_bytes());
    for (i, &off) in new_offs.iter().enumerate() {
        out[ARRAY_OFF + i * 4..ARRAY_OFF + i * 4 + 4].copy_from_slice(&(off as u32).to_le_bytes());
    }
    let mut effect_indices: BTreeSet<u32> = BTreeSet::new();
    for r in &ret {
        for at in [r.off + 4, r.off + 8] {
            let idx = legaia_bytes::u32_le(retail_block, at).context("effect index")?;
            if idx != 0 {
                if idx >= 0x100 {
                    bail!("effect index {idx:#x} is not a table index");
                }
                effect_indices.insert(idx);
            }
        }
    }
    for &idx in &effect_indices {
        let word_off = (idx as usize + count + TABLE_WORD_BIAS) * 4;
        let val = legaia_bytes::u32_le(current_block, word_off)
            .with_context(|| format!("effect table word {idx}"))? as usize;
        if val >= cur_tail_start {
            out[word_off..word_off + 4]
                .copy_from_slice(&((val as i64 + delta) as u32).to_le_bytes());
        } else if val >= cur_region_start {
            bail!("effect descriptor +{val:#x} lives inside the entry region");
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Orchestration.

/// Which optional entry families to rewrite (the required set - the idle
/// and the module-staged special chain - is always rewritten). The
/// patcher drops families front-to-back when the re-encoded slot misses
/// the fixed archive budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirrorOptions {
    /// Hit reactions: flinches (tags 2/3), knockdown (4), get-up (5),
    /// block (0x0B) ← the hero's own same-tag clips.
    pub reactions: bool,
    /// Walk/approach cycle (tag 1) ← the hero's walk (the entry's
    /// root-motion words are preserved, so approach speed is retail's).
    pub walk: bool,
    /// AI-rollable castable attacks (tags 0x0C..=0x1F with a real AGL
    /// cost) ← the hero's default-equipment weapon swings, round-robin.
    pub attacks: bool,
    /// Density rung: halve the keyframe count AND the rate byte of
    /// non-staged rewritten streams where both divide exactly
    /// (`frames * 8 / rate` - the duration - is invariant; a stream
    /// with an odd count or rate is left alone).
    pub halve_non_staged: bool,
    /// The same exact halving on the staged streams, floors respected.
    pub halve_staged: bool,
    /// Close-entry rung: source the closing settle pose from the hero's
    /// short record0 "Recover" clip (tag 8) instead of the base-ME
    /// victory flourish (the largest single stream the mirror writes).
    pub compact_close: bool,
}

impl MirrorOptions {
    pub const ALL: MirrorOptions = MirrorOptions {
        reactions: true,
        walk: true,
        attacks: true,
        halve_non_staged: false,
        halve_staged: false,
        compact_close: false,
    };
    pub const NONE: MirrorOptions = MirrorOptions {
        reactions: false,
        walk: false,
        attacks: false,
        halve_non_staged: true,
        halve_staged: true,
        compact_close: true,
    };
}

/// The module-staged entries of one block, with per-entry keyframe
/// floors. `chain` is the caster chain in stage order (the LAST element
/// is the payoff - the strike the damage window plays over); `floors`
/// is parallel to it.
#[derive(Debug, Clone, Copy)]
pub struct StagedPlan<'a> {
    pub chain: &'a [usize],
    pub chain_floors: &'a [usize],
    pub close: Option<usize>,
    pub close_floor: usize,
}

/// A mirrored block plus what happened to it.
pub struct MirroredBlock {
    /// The rebuilt decoded block (caller re-encodes the archive slot).
    pub block: Vec<u8>,
    /// Entry indices whose streams were replaced.
    pub rewritten: Vec<usize>,
    /// Human-readable notes.
    pub notes: Vec<String>,
}

/// The hero's 50-AP Hyper art clip and its rate, resolved from the RETAIL
/// player file + RETAIL readef image (the reskin pass overwrites both on
/// disc, so post-patch images would hand back the sibling's motion).
pub fn hero_hyper_clip(
    player_file: &[u8],
    readef: &[u8],
    char_index: usize,
) -> Result<MonsterAnimation> {
    let anim_id = *HYPER_ANIM_ID
        .get(char_index)
        .ok_or_else(|| anyhow::anyhow!("char index {char_index} out of range"))?;
    let rec0 = bca::decode_record0(player_file).context("decode record0")?;
    let bank = bca::art_animation_bank(&rec0).context("art bank")?;
    let record = bank
        .iter()
        .find(|r| r.anim_id == anim_id && !r.uses_base_archive())
        .ok_or_else(|| anyhow::anyhow!("no art bank record with anim id {anim_id:#04x}"))?;
    let archive = bca::art_me_archive(readef, char_index, false).context("main ME archive")?;
    bca::art_animation(record, &archive)
}

/// The hero's primary victory flourish (base "ME" archive entry 0) - the
/// settle/close clip for the staged chain's closing entry, which doubles
/// as the monster's tag-`0x22` victory pose when the duel is lost.
fn hero_victory_clip(
    player_file: &[u8],
    readef: &[u8],
    char_index: usize,
) -> Result<MonsterAnimation> {
    let rec0 = bca::decode_record0(player_file).context("decode record0")?;
    let bank = bca::art_animation_bank(&rec0).context("art bank")?;
    let rate = bank
        .iter()
        .find(|r| r.uses_base_archive() && r.stream_source == 0)
        .map(|r| r.rate.max(1))
        .unwrap_or(1);
    let archive = bca::art_me_archive(readef, char_index, true).context("base ME archive")?;
    let stream = archive.entry(0).context("base entry 0")?;
    crate::monster_archive::parse_animation_stream(&stream, 0x22, rate, 0, 0, Vec::new())
        .ok_or_else(|| anyhow::anyhow!("base ME entry 0 is not a keyframe stream"))
}

/// The close clip under `compact_close`: the hero's record0 "Recover"
/// clip (action tag 8) - a short settle pose (7-15 frames across the
/// three heroes) against the 59-60 frame victory flourish.
fn hero_recover_clip(hero_anims: &[MonsterAnimation]) -> Option<&MonsterAnimation> {
    hero_anims.iter().find(|a| a.action_id == 8)
}

/// Stretch a retargeted clip to at least `min_frames`, preserving its
/// wall-clock duration: an integer stretch factor `m` multiplies both the
/// frame count and the rate byte (`frames * 8 / rate` is invariant).
fn stretch_to_min(
    rows: Vec<Vec<PartPose>>,
    rate: u8,
    min_frames: usize,
) -> (Vec<Vec<PartPose>>, u8) {
    let f = rows.len().max(1);
    let m = min_frames.div_ceil(f).max(1);
    if m == 1 {
        return (rows, rate.max(1));
    }
    let stretched = resample_poses(&rows, f * m);
    let rate = (rate.max(1) as usize * m).min(255) as u8;
    (stretched, rate)
}

/// Exact keyframe-density halving: frames and rate both even (and the
/// halved count still at or above `floor`) → halve both, which keeps
/// the duration `frames * 8 / rate` bit-exact. Returns whether it
/// applied.
fn halve_exact(rows: &mut Vec<Vec<PartPose>>, rate: &mut u8, floor: usize) -> bool {
    let f = rows.len();
    if !f.is_multiple_of(2) || !rate.is_multiple_of(2) || f / 2 < floor.max(4) {
        return false;
    }
    *rows = resample_poses(rows, f / 2);
    *rate /= 2;
    true
}

/// Source-frame count for the payoff stage of an `h`-frame Hyper split
/// into `n` stages: the uniform share, unless giving the payoff
/// `payoff_floor` REAL source frames (leaving at least one per earlier
/// stage) produces a smaller stretched stream - a 58-frame clip split
/// 3 ways stretches its 20-frame payoff to 40 under a 23 floor, while a
/// 23-frame payoff needs no stretch at all.
fn payoff_source_frames(h: usize, n: usize, payoff_floor: usize) -> usize {
    let uniform = h - (n - 1) * h / n;
    if uniform >= payoff_floor {
        return uniform;
    }
    let out = |p: usize| p * payoff_floor.div_ceil(p);
    let widened = payoff_floor.min(h.saturating_sub(n - 1)).max(1);
    if out(widened) < out(uniform) {
        widened
    } else {
        uniform
    }
}

/// Rewrite the swapped monster block's animation entries with the mapped
/// hero's own clips. `current_block` is the decoded block as it sits on
/// the (already `--delilas-party`-patched) disc; every retail-derived
/// input (`retail_archive`, `player_file`, `readef`) must be the
/// PRE-patch image - the party-swap pass rewrites the player files, the
/// readef ME slots and the archive itself, so post-patch copies would
/// feed the retarget the wrong data. Idempotent: entry content is a pure
/// function of the retail sources, so a second pass reproduces the block.
#[allow(clippy::too_many_arguments)]
pub fn mirror_block_animations(
    current_block: &[u8],
    retail_archive: &[u8],
    target_id: u16,
    player_file: &[u8],
    readef: &[u8],
    char_index: usize,
    rig: &PlayerRig,
    plan: &StagedPlan<'_>,
    opts: &MirrorOptions,
) -> Result<MirroredBlock> {
    let retail_block = monster_archive::decode_block(retail_archive, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: empty slot"))?;
    let (ret_entries, _, _, _) = parse_entry_table(&retail_block)?;
    let ctx = monster_bake_ctx(player_file, rig, retail_archive, target_id)?;
    let rt = HeroRetarget::new(&ctx, rig);
    let mut notes = Vec::new();

    // Hero clip lookup by action tag (player files store the family
    // identity-ordered: slot index == tag == `action_id`).
    let hero_anims = bca::battle_animations(player_file).context("hero record0 animations")?;
    let hero_by_tag =
        |tag: u8| -> Option<&MonsterAnimation> { hero_anims.iter().find(|a| a.action_id == tag) };

    // The rigid whole-body anchor, from the retargeted hero idle against
    // the block's own retail idle.
    let hero_idle = hero_by_tag(0).ok_or_else(|| anyhow::anyhow!("hero file has no idle"))?;
    let host_idle = monster_archive::idle_animation(retail_archive, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: no idle"))?;
    let idle_rows = rt.retarget_frames(&hero_idle.frames);
    let anchor = monster_anchor(&idle_rows, &host_idle.frames);
    notes.push(format!(
        "anchor [{}, {}, {}]",
        anchor[0], anchor[1], anchor[2]
    ));

    struct Pending {
        rows: Vec<Vec<PartPose>>,
        rate: u8,
        floor: usize,
        staged: bool,
    }
    let mut pending: BTreeMap<usize, Pending> = BTreeMap::new();
    let mut push = |idx: usize, mut rows: Vec<Vec<PartPose>>, rate: u8, floor: usize, st: bool| {
        apply_anchor(&mut rows, anchor);
        pending.insert(
            idx,
            Pending {
                rows,
                rate: rate.max(1),
                floor,
                staged: st,
            },
        );
    };

    // Idle (entry 0) - always.
    push(0, idle_rows.clone(), hero_idle.rate, 1, false);

    // The staged special chain: the hero's Hyper art split across the
    // module's staged entries (early stages = wind-up, the last stage =
    // the payoff swing), each stage held at or above its plan floor by a
    // duration-preserving integer stretch. The payoff boundary widens to
    // its floor when that avoids a stretch (a real 23-frame strike beats
    // a 20-frame strike doubled to 40).
    let hyper = hero_hyper_clip(player_file, readef, char_index).context("hero Hyper clip")?;
    let staged = plan.chain;
    if staged.is_empty() {
        bail!("no staged entries for monster id {target_id}");
    }
    if plan.chain_floors.len() != staged.len() {
        bail!("staged plan floors do not match the chain length");
    }
    let n = staged.len();
    let h = hyper.frames.len();
    if h < n {
        bail!("hero Hyper clip has {h} frames for {n} stages");
    }
    let payoff_floor = *plan.chain_floors.last().expect("chain non-empty");
    let payoff_src = payoff_source_frames(h, n, payoff_floor);
    let lead = h - payoff_src;
    for (k, &idx) in staged.iter().enumerate() {
        let (b0, b1) = if k + 1 == n {
            (lead, h)
        } else {
            let b0 = k * lead / (n - 1);
            let b1 = ((k + 1) * lead / (n - 1)).max(b0 + 1);
            (b0, b1)
        };
        let seg = &hyper.frames[b0..b1];
        let (rows, rate) =
            stretch_to_min(rt.retarget_frames(seg), hyper.rate, plan.chain_floors[k]);
        push(idx, rows, rate, plan.chain_floors[k], true);
    }
    notes.push(format!(
        "hyper {h}f rate {} split over {n} staged entries {staged:?} (payoff {payoff_src} source frames)",
        hyper.rate
    ));

    // Closing entry: the hero's victory flourish (also the monster's
    // tag-0x22 victory pose), or the short record0 Recover settle under
    // the compact-close budget rung. Staged by the module, so it honours
    // the plan floor.
    if let Some(idx) = plan.close {
        let compact = opts
            .compact_close
            .then(|| hero_recover_clip(&hero_anims).cloned())
            .flatten();
        let (clip, label) = match compact {
            Some(c) => (c, "hero recover"),
            None => (
                hero_victory_clip(player_file, readef, char_index).context("hero victory")?,
                "hero victory",
            ),
        };
        let (rows, rate) = stretch_to_min(
            rt.retarget_frames(&clip.frames),
            clip.rate,
            plan.close_floor,
        );
        push(idx, rows, rate, plan.close_floor, true);
        notes.push(format!(
            "close entry {idx} <- {label} ({}f)",
            clip.frame_count
        ));
    }

    // Optional families, selected by retail entry tag.
    let staged_set: Vec<usize> = staged.iter().copied().chain(plan.close).collect();
    let tag_of = |i: usize| retail_block[ret_entries[i].off];
    let agl_of = |i: usize| retail_block[ret_entries[i].off + 0x74];
    if opts.walk {
        for i in 0..ret_entries.len() {
            if tag_of(i) == 1
                && !staged_set.contains(&i)
                && let Some(clip) = hero_by_tag(1)
            {
                push(i, rt.retarget_frames(&clip.frames), clip.rate, 1, false);
            }
        }
    }
    if opts.reactions {
        for i in 0..ret_entries.len() {
            let tag = tag_of(i);
            if [2u8, 3, 4, 5, 0x0B].contains(&tag)
                && !staged_set.contains(&i)
                && let Some(clip) = hero_by_tag(tag)
            {
                push(i, rt.retarget_frames(&clip.frames), clip.rate, 1, false);
            }
        }
    }
    if opts.attacks {
        let pack = battle_data_pack::parse(player_file)?;
        match swing_battle_animations(player_file, &pack, &[0u8; SECTION_COUNT]) {
            Ok(swings) if !swings.is_empty() => {
                let mut next = 0usize;
                for i in 0..ret_entries.len() {
                    let tag = tag_of(i);
                    if (0x0C..=0x1F).contains(&tag) && agl_of(i) != 0xFF && !staged_set.contains(&i)
                    {
                        let sw = &swings[next % swings.len()];
                        next += 1;
                        push(
                            i,
                            rt.retarget_frames(&sw.anim.frames),
                            sw.anim.rate,
                            1,
                            false,
                        );
                    }
                }
                if next > 0 {
                    notes.push(format!("{next} attack entries <- hero weapon swings"));
                }
            }
            Ok(_) => notes.push("hero file has no weapon swings; attacks stay".into()),
            Err(e) => notes.push(format!("weapon swings unavailable ({e:#}); attacks stay")),
        }
    }

    // Density rungs: exact halving (frames and rate both even, floors
    // respected) - duration-invariant, so module pacing and the look's
    // tempo are untouched; only pose density drops, to a coarseness
    // retail itself ships (several retail entries are authored at rate 1).
    let mut halved = 0usize;
    for p in pending.values_mut() {
        let want = if p.staged {
            opts.halve_staged
        } else {
            opts.halve_non_staged
        };
        if want && halve_exact(&mut p.rows, &mut p.rate, p.floor) {
            halved += 1;
        }
    }
    if halved > 0 {
        notes.push(format!("{halved} streams at halved keyframe density"));
    }

    let rewrites: BTreeMap<usize, EntryRewrite> = pending
        .into_iter()
        .map(|(i, p)| {
            (
                i,
                EntryRewrite {
                    rows: p.rows,
                    rate: p.rate,
                },
            )
        })
        .collect();
    let rewritten: Vec<usize> = rewrites.keys().copied().collect();
    let block = rebuild_block_entries(current_block, &retail_block, &rewrites)?;
    notes.push(format!(
        "block {} -> {} bytes ({:+})",
        current_block.len(),
        block.len(),
        block.len() as i64 - current_block.len() as i64
    ));
    Ok(MirroredBlock {
        block,
        rewritten,
        notes,
    })
}

// ---------------------------------------------------------------------------
// Bake-parity instrument: per-part exact affine fit.

/// Per-part affine fit of the enemy-side bake, `source geometry posed at
/// the (normalized) player rest` → `baked geometry posed at the monster
/// rest`, polar-decomposed. Gap/centroid metrics are structurally blind
/// to per-part roll; this is the instrument that sees it.
#[derive(Debug, Clone)]
pub struct PartAffineFit {
    pub part: usize,
    /// Whether the part is a chain terminal (head / hand / foot).
    pub terminal: bool,
    /// Rotation angle (degrees) between the fitted rotation and the
    /// whole-rig ideal (body re-face + minimal swing onto the target
    /// bone), computed from the rest PIVOTS alone - independent of the
    /// bake's own frame machinery. For terminals the ideal is the
    /// authored world orientation carried over unchanged, measured
    /// against the RAW (un-normalized) rest.
    pub excess_deg: f32,
    /// Principal scales of the fitted linear map, descending.
    pub principal_scales: [f32; 3],
    /// RMS non-affine residual over the part's RMS radius.
    pub residual: f32,
}

fn mat_angle_deg(m: &[[f32; 3]; 3]) -> f32 {
    let tr = (m[0][0] + m[1][1] + m[2][2]).clamp(-1.0, 3.0);
    (((tr - 1.0) / 2.0).clamp(-1.0, 1.0)).acos().to_degrees()
}

/// 3x3 symmetric Jacobi eigenvalues (descending).
fn sym_eigenvalues(a: [[f32; 3]; 3]) -> [f32; 3] {
    let mut m = a;
    for _ in 0..32 {
        // Largest off-diagonal.
        let (mut p, mut q, mut big) = (0usize, 1usize, m[0][1].abs());
        for &(i, j) in &[(0usize, 2usize), (1, 2)] {
            if m[i][j].abs() > big {
                big = m[i][j].abs();
                p = i;
                q = j;
            }
        }
        if big < 1e-7 {
            break;
        }
        let theta = 0.5 * (2.0 * m[p][q]).atan2(m[q][q] - m[p][p] + 1e-20);
        let (s, c) = theta.sin_cos();
        let mut r = [[0.0f32; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            row[i] = 1.0;
        }
        r[p][p] = c;
        r[q][q] = c;
        r[p][q] = s;
        r[q][p] = -s;
        // m = r^T m r
        let mut t = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                t[i][j] = (0..3).map(|k| r[k][i] * m[k][j]).sum();
            }
        }
        let mut m2 = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m2[i][j] = (0..3).map(|k| t[i][k] * r[k][j]).sum();
            }
        }
        m = m2;
    }
    let mut ev = [m[0][0], m[1][1], m[2][2]];
    ev.sort_by(|a, b| b.partial_cmp(a).unwrap());
    ev
}

fn mat3_inv(m: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv_det = 1.0 / det;
    let mut inv = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let (a, b, c, d) = (
                m[(j + 1) % 3][(i + 1) % 3],
                m[(j + 2) % 3][(i + 2) % 3],
                m[(j + 1) % 3][(i + 2) % 3],
                m[(j + 2) % 3][(i + 1) % 3],
            );
            inv[i][j] = (a * b - c * d) * inv_det;
        }
    }
    Some(inv)
}

/// Least-squares affine fit `w_dst ≈ A * w_src + b`, then polar
/// decomposition of `A` (Higham iteration).
fn affine_fit(src: &[[f32; 3]], dst: &[[f32; 3]]) -> Option<([[f32; 3]; 3], f32, [f32; 3])> {
    let n = src.len().min(dst.len());
    if n < 4 {
        return None;
    }
    let mean = |pts: &[[f32; 3]]| {
        let mut m = [0.0f32; 3];
        for p in pts.iter().take(n) {
            for k in 0..3 {
                m[k] += p[k];
            }
        }
        [m[0] / n as f32, m[1] / n as f32, m[2] / n as f32]
    };
    let (ms, md) = (mean(src), mean(dst));
    let mut cov_ds = [[0.0f32; 3]; 3]; // dst x src
    let mut cov_ss = [[0.0f32; 3]; 3];
    for i in 0..n {
        let s = vsub(src[i], ms);
        let d = vsub(dst[i], md);
        for r in 0..3 {
            for c in 0..3 {
                cov_ds[r][c] += d[r] * s[c];
                cov_ss[r][c] += s[r] * s[c];
            }
        }
    }
    // Regularize a flat part.
    let trace = cov_ss[0][0] + cov_ss[1][1] + cov_ss[2][2];
    for (k, row) in cov_ss.iter_mut().enumerate() {
        row[k] += trace * 1e-6 + 1e-6;
    }
    let a = mmul(&cov_ds, &mat3_inv(cov_ss)?);
    // Residual.
    let mut se = 0.0f32;
    let mut rad = 0.0f32;
    for i in 0..n {
        let s = vsub(src[i], ms);
        let d = vsub(dst[i], md);
        let p = apply(&a, s);
        let e = vsub(d, p);
        se += vdot(e, e);
        rad += vdot(d, d);
    }
    let residual = (se / n as f32).sqrt() / (rad / n as f32).sqrt().max(1.0);
    // Polar rotation via Higham: R <- (R + R^-T)/2.
    let mut r = a;
    for _ in 0..24 {
        let Some(rinv) = mat3_inv(r) else { break };
        let rinv_t = transpose(&rinv);
        let mut next = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                next[i][j] = 0.5 * (r[i][j] + rinv_t[i][j]);
            }
        }
        r = next;
    }
    // Principal scales: sqrt of eigenvalues of A^T A.
    let ata = mmul(&transpose(&a), &a);
    let ev = sym_eigenvalues(ata);
    let scales = [
        ev[0].max(0.0).sqrt(),
        ev[1].max(0.0).sqrt(),
        ev[2].max(0.0).sqrt(),
    ];
    Some((r, residual, scales))
}

/// The whole-rig ideal rotation for canonical part `c`: read the source
/// bone in the source body frame, rebuild it in the destination body
/// frame (re-face), then swing minimally onto the destination bone.
/// Pure function of the rest pivots - shares no code with the bake's
/// frame machinery beyond the two public axis helpers.
fn ideal_rotation(
    src_pivots: &[[f32; 3]],
    dst_pivots: &[[f32; 3]],
    c: usize,
) -> Option<[[f32; 3]; 3]> {
    let axes = |pivots: &[[f32; 3]]| -> [[f32; 3]; 3] {
        let unit = |v: [f32; 3], f: [f32; 3]| {
            let l = vnorm(v);
            if l < 1e-3 {
                f
            } else {
                [v[0] / l, v[1] / l, v[2] / l]
            }
        };
        let up = unit(vsub(pivots[0], pivots[2]), [0.0, -1.0, 0.0]);
        let lat = vsub(pivots[6], pivots[3]);
        let d = vdot(lat, up);
        let lat = unit(
            [lat[0] - up[0] * d, lat[1] - up[1] * d, lat[2] - up[2] * d],
            [0.0, 0.0, 1.0],
        );
        [up, lat, unit(vcross(up, lat), [1.0, 0.0, 0.0])]
    };
    let (bs, bd) = (axes(src_pivots), axes(dst_pivots));
    // R_reface: world -> world, source body frame onto destination's.
    let mut reface = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            reface[i][j] = (0..3).map(|k| bd[k][i] * bs[k][j]).sum();
        }
    }
    let child = CANONICAL_CHILD[c]?;
    let bone = |pivots: &[[f32; 3]], from: usize, to: usize| -> Option<[f32; 3]> {
        let b = vsub(pivots[to], pivots[from]);
        let l = vnorm(b);
        (l >= 2.0).then(|| [b[0] / l, b[1] / l, b[2] / l])
    };
    let xs = bone(src_pivots, c, child)?;
    let xd = bone(dst_pivots, c, child)?;
    let refaced = apply(&reface, xs);
    // Rodrigues swing refaced -> xd.
    let v = vcross(refaced, xd);
    let cos = vdot(refaced, xd).clamp(-1.0, 1.0);
    let s = vnorm(v);
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if s < 1e-6 {
        return (cos > 0.0).then_some(reface);
    }
    let k = [v[0] / s, v[1] / s, v[2] / s];
    let kx = [[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]];
    let (st, ct) = s.atan2(cos).sin_cos();
    let mut swing = ident;
    for i in 0..3 {
        for j in 0..3 {
            let kk: f32 = (0..3).map(|q| kx[i][q] * kx[q][j]).sum();
            swing[i][j] = ident[i][j] + st * kx[i][j] + (1.0 - ct) * kk;
        }
    }
    Some(mmul(&swing, &reface))
}

/// Run the enemy-side bake and fit each part's produced geometry against
/// its source, reporting excess rotation / principal scales / residual.
/// Disc-gated tests assert bounds on this - the instrument that catches
/// the roll-defect class the gap metrics were blind to.
pub fn monsterize_fit_report(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
) -> Result<Vec<PartAffineFit>> {
    let ctx = monster_bake_ctx(player_file, rig, archive_entry, target_id)?;
    let mut out = Vec::with_capacity(CANONICAL_PARTS);
    #[allow(clippy::needless_range_loop)]
    for c in 0..CANONICAL_PARTS {
        let ch = rig.channel_for_canonical[c] as usize;
        let terminal = CANONICAL_CHILD[c].is_none();
        // Source world: pre-bake geometry posed at the player rest. For
        // non-terminals the normalized and raw rests agree; terminals are
        // measured against the RAW rest so the fit checks the authored
        // orientation really is carried over.
        let pose = if terminal {
            ctx.rest_raw[ch]
        } else {
            ctx.rest[ch]
        };
        let ms = rot_matrix(&pose);
        let src_world: Vec<[f32; 3]> = ctx.objects[c]
            .vertices
            .iter()
            .map(|v| {
                let w = apply(&ms, [v[0] as f32, v[1] as f32, v[2] as f32]);
                [
                    w[0] + pose.tx as f32,
                    w[1] + pose.ty as f32,
                    w[2] + pose.tz as f32,
                ]
            })
            .collect();
        // Baked world: run the bake on a clone, pose at the monster rest.
        let mut baked = ctx.objects[c].clone();
        let pb = pivot_bake_params(&ctx.src_frames[c], &ctx.dst_frames[c], ctx.radial);
        bake_object_pivot(
            &mut baked,
            &ctx.rest[ch],
            ctx.src_pivots[c],
            &ctx.target_rest[c],
            &pb,
        )?;
        let md = rot_matrix(&ctx.target_rest[c]);
        let tp = &ctx.target_rest[c];
        let dst_world: Vec<[f32; 3]> = baked
            .vertices
            .iter()
            .map(|v| {
                let w = apply(&md, [v[0] as f32, v[1] as f32, v[2] as f32]);
                [
                    w[0] + tp.tx as f32,
                    w[1] + tp.ty as f32,
                    w[2] + tp.tz as f32,
                ]
            })
            .collect();
        let Some((r_fit, residual, scales)) = affine_fit(&src_world, &dst_world) else {
            continue;
        };
        let ideal = if terminal {
            // Normalization carries the authored world orientation over
            // unchanged: the ideal is the identity.
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        } else {
            match ideal_rotation(&ctx.src_pivots, &ctx.dst_pivots, c) {
                Some(m) => m,
                None => continue,
            }
        };
        let excess = mmul(&r_fit, &transpose(&ideal));
        out.push(PartAffineFit {
            part: c,
            terminal,
            excess_deg: mat_angle_deg(&excess),
            principal_scales: scales,
            residual,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_preserves_endpoints_and_wraps_rotation_short_path() {
        let mk = |tx: i16, ry: u16| {
            vec![PartPose {
                tx,
                ry,
                ..PartPose::default()
            }]
        };
        let frames = vec![mk(0, 4000), mk(100, 200)];
        let out = resample_poses(&frames, 5);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0][0].tx, 0);
        assert_eq!(out[4][0].tx, 100);
        // Midpoint rotation goes the short way through 0, not through 2048.
        let mid = out[2][0].ry as i32;
        assert!(mid >= 4048 || mid <= 152, "mid ry {mid} took the long path");
    }

    #[test]
    fn stretch_to_min_holds_duration() {
        let mk = |tx: i16| {
            vec![PartPose {
                tx,
                ..PartPose::default()
            }]
        };
        let rows: Vec<_> = (0..7i16).map(mk).collect();
        let (out, rate) = stretch_to_min(rows, 2, 23);
        // 7 frames -> ceil(23/7)=4x stretch, rate 2 -> 8.
        assert_eq!(out.len(), 28);
        assert_eq!(rate, 8);
        // frames * 8 / rate invariant: 7*8/2 == 28*8/8.
        assert_eq!(7 * 8 / 2, 28 * 8 / 8);
    }

    #[test]
    fn payoff_split_widens_only_when_it_shrinks_the_stream() {
        // 58f / 3 stages, floor 23: uniform payoff (20f) stretches to 40;
        // a widened 23f payoff needs none - widen.
        assert_eq!(payoff_source_frames(58, 3, 23), 23);
        // 21f / 3 stages, floor 23: widening to 19f still stretches (x2 =
        // 38) and loses to the uniform 7f x4 = 28 - keep uniform.
        assert_eq!(payoff_source_frames(21, 3, 23), 7);
        // 20f / 2 stages, floor 11: widened 11f payoff needs no stretch
        // (11) vs uniform 10f doubled (20) - widen.
        assert_eq!(payoff_source_frames(20, 2, 11), 11);
        // Floor already met by the uniform share: unchanged.
        assert_eq!(payoff_source_frames(60, 3, 11), 20);
    }

    #[test]
    fn halve_exact_is_duration_invariant_and_floor_guarded() {
        let mk = |n: usize| -> Vec<Vec<PartPose>> {
            (0..n as i16)
                .map(|f| {
                    vec![PartPose {
                        tx: f,
                        ..PartPose::default()
                    }]
                })
                .collect()
        };
        // 24f rate 4 -> 12f rate 2: duration 24*8/4 == 12*8/2.
        let mut rows = mk(24);
        let mut rate = 4u8;
        assert!(halve_exact(&mut rows, &mut rate, 11));
        assert_eq!((rows.len(), rate), (12, 2));
        // Odd rate: refused.
        let mut rows = mk(24);
        let mut rate = 1u8;
        assert!(!halve_exact(&mut rows, &mut rate, 1));
        // Floor guard: halving 24 under a 13 floor is refused.
        let mut rows = mk(24);
        let mut rate = 4u8;
        assert!(!halve_exact(&mut rows, &mut rate, 13));
    }

    #[test]
    fn transform_head_rescales_frame_indexed_fields_only() {
        let mut head = vec![0u8; ENTRY_STREAM_OFF];
        head[0x00] = 0x23; // tag
        head[0x74] = 0x1C; // AGL
        head[0x76] = 0; // event-path gate
        head[0x77] = 0x07; // attach key
        head[0x78] = 2; // rate
        head[0x7A] = 0x01; // impact class
        head[0x84] = 0xFF; // loop seed
        head[0x85] = 9;
        head[0x86] = 10;
        head[0x87] = 0x01; // sound cue
        head[0x10] = 6; // event beat
        head[0x14] = 6; // fx gate
        head[0x15] = 0x84; // fx id
        head[0x0C] = 4; // approach speed word (low byte)
        let out = transform_head(&head, 11, 22, 4);
        assert_eq!(out[0x00], 0x23);
        assert_eq!(out[0x74], 0x1C);
        assert_eq!(out[0x77], 0x07);
        assert_eq!(out[0x78], 4, "rate byte rewritten");
        assert_eq!(out[0x7A], 0x01);
        assert_eq!(out[0x87], 0x01, "sound cue untouched");
        assert_eq!(out[0x0C], 4, "root motion untouched");
        assert_eq!(out[0x10], 12, "event beat rescaled");
        assert_eq!(out[0x14], 12, "fx gate rescaled");
        assert_eq!(out[0x15], 0x84, "fx id untouched");
        assert_eq!(out[0x84], 0xFF);
        assert_eq!(out[0x85], 18, "loop start rescaled");
        assert_eq!(out[0x86], 20, "loop window length rescaled");
    }

    #[test]
    fn transform_head_keeps_single_frame_holds_single_frame() {
        let mut head = vec![0u8; ENTRY_STREAM_OFF];
        head[0x84] = 0xFF;
        head[0x85] = 15;
        head[0x86] = 15;
        let out = transform_head(&head, 16, 32, 1);
        assert_eq!(out[0x85], out[0x86], "zero-length hold stays zero-length");
        assert!(out[0x85] as usize <= 31);
    }

    #[test]
    fn transform_head_keeps_event_count_and_ascending_order() {
        let mut head = vec![0u8; ENTRY_STREAM_OFF];
        head[0x10..0x14].copy_from_slice(&[3, 7, 11, 15]);
        let out = transform_head(&head, 19, 8, 1);
        let ev: Vec<u8> = out[0x10..0x14].to_vec();
        assert!(ev.iter().all(|&v| v != 0), "count preserved: {ev:?}");
        assert!(ev.windows(2).all(|w| w[0] < w[1]), "ascending: {ev:?}");
        assert!(
            ev.iter().all(|&v| (v as usize) < 8),
            "inside the clip: {ev:?}"
        );
    }

    #[test]
    fn affine_fit_recovers_a_known_rotation_and_scales() {
        // Points on a box, rotated 30 deg about z and scaled [2, 1, 0.5].
        let rot = 30f32.to_radians();
        let (s, c) = rot.sin_cos();
        let src: Vec<[f32; 3]> = (0..64)
            .map(|i| {
                [
                    ((i % 4) as f32) * 10.0,
                    (((i / 4) % 4) as f32) * 7.0,
                    ((i / 16) as f32) * 5.0,
                ]
            })
            .collect();
        let dst: Vec<[f32; 3]> = src
            .iter()
            .map(|p| {
                let scaled = [p[0] * 2.0, p[1] * 1.0, p[2] * 0.5];
                [
                    c * scaled[0] - s * scaled[1] + 3.0,
                    s * scaled[0] + c * scaled[1] - 8.0,
                    scaled[2] + 1.0,
                ]
            })
            .collect();
        let (r, residual, scales) = affine_fit(&src, &dst).expect("fit");
        assert!(residual < 1e-3, "residual {residual}");
        assert!((scales[0] - 2.0).abs() < 0.01, "scales {scales:?}");
        assert!((scales[2] - 0.5).abs() < 0.01, "scales {scales:?}");
        // Rotation angle 30 deg.
        let ang = mat_angle_deg(&r);
        assert!((ang - 30.0).abs() < 0.5, "angle {ang}");
    }

    #[test]
    fn rebuild_is_idempotent_on_a_synthetic_block() {
        // A tiny block in the retail layout: head, 2 entries, tail
        // descriptor, pool.
        let count = 2usize;
        let entry_area = 0x70usize;
        let mk_entry = |id: u8, idx4: u32, frames: u8| {
            let mut e = vec![0u8; ENTRY_STREAM_OFF + 2 + frames as usize * 9];
            e[0] = id;
            e[4..8].copy_from_slice(&idx4.to_le_bytes());
            e[0x78] = 2;
            e[ENTRY_STREAM_OFF] = 1;
            e[ENTRY_STREAM_OFF + 1] = frames;
            e
        };
        let e0 = mk_entry(0x00, 0, 4);
        let e1 = mk_entry(0x23, 1, 6);
        let o0 = entry_area;
        let o1 = (o0 + e0.len()).div_ceil(4) * 4;
        let tail = (o1 + e1.len()).div_ceil(4) * 4;
        let pool = tail + 8;
        let total = pool + 16;
        let mut b = vec![0u8; total];
        b[0..4].copy_from_slice(&0x40u32.to_le_bytes());
        b[4..8].copy_from_slice(&0x48u32.to_le_bytes());
        b[8..12].copy_from_slice(&(pool as u32).to_le_bytes());
        b[COUNT_OFF] = count as u8;
        for (k, o) in [o0, o1].iter().enumerate() {
            b[ARRAY_OFF + k * 4..ARRAY_OFF + k * 4 + 4].copy_from_slice(&(*o as u32).to_le_bytes());
        }
        let word = (1 + count + TABLE_WORD_BIAS) * 4;
        b[word..word + 4].copy_from_slice(&(tail as u32).to_le_bytes());
        b[o0..o0 + e0.len()].copy_from_slice(&e0);
        b[o1..o1 + e1.len()].copy_from_slice(&e1);
        b[tail..tail + 8].copy_from_slice(&[0xAA; 8]);
        b[pool..].fill(0x55);

        let rows: Vec<Vec<PartPose>> = (0..9i16)
            .map(|f| {
                vec![PartPose {
                    tx: f * 3,
                    ..PartPose::default()
                }]
            })
            .collect();
        let mut rw = BTreeMap::new();
        rw.insert(
            1usize,
            EntryRewrite {
                rows: rows.clone(),
                rate: 3,
            },
        );
        let once = rebuild_block_entries(&b, &b, &rw).expect("rebuild");
        // The rewritten entry decodes at the new shape through the raw parser.
        let (entries, ..) = parse_entry_table(&once).expect("parse rebuilt");
        assert_eq!(entries[1].frames, 9);
        assert_eq!(once[entries[1].off + 0x78], 3);
        // The tail descriptor is still reachable through the shifted word.
        let desc = u32::from_le_bytes(once[word..word + 4].try_into().unwrap()) as usize;
        assert_eq!(&once[desc..desc + 8], &[0xAA; 8]);
        // Pool intact.
        let p = u32::from_le_bytes(once[8..12].try_into().unwrap()) as usize;
        assert!(once[p..].iter().all(|&x| x == 0x55));
        // Idempotence: a second pass over the mirrored block reproduces it.
        let twice = rebuild_block_entries(&once, &b, &rw).expect("rebuild twice");
        assert_eq!(once, twice, "second pass must be byte-identical");
    }
}
