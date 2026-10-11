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
use legaia_actor_formats::battle_char_assembly as bca;
use legaia_actor_formats::battle_char_assembly::swing_battle_animations;
use legaia_actor_formats::monster_archive::MonsterAnimation;

mod bake_ctx;
mod block_entries;
mod fit_report;
mod hero_retarget;

pub(crate) use bake_ctx::monster_bake_ctx;
pub(crate) use block_entries::{EntryRewrite, monster_anchor, rebuild_block_entries};
use block_entries::{apply_anchor, parse_entry_table, scale_block_tmd_vertices};
pub use fit_report::{PartAffineFit, monsterize_fit_report};
pub(crate) use hero_retarget::{HeroRetarget, resample_poses};

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
    legaia_actor_formats::monster_archive::parse_animation_stream(
        &stream,
        0x22,
        rate,
        0,
        0,
        Vec::new(),
    )
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

    // Native-stature pass: the bake + retarget work at the MONSTER's
    // scale (`ctx.radial` = monster/player rest-height ratio), so the
    // finished figure wears the DELILAS sibling's stature - conspicuous
    // on Che's block, where hero Gala towers at ~1.5x his own height.
    // `native = 1/radial` undoes that uniformly: every rewritten row's
    // translations scale by `native` (rotations untouched), every KEPT
    // host clip is converted into a rewrite of its own decoded rows at
    // the same scale, and the block TMD's pivot-relative vertices scale
    // by the same factor after the rebuild - a whole-figure uniform
    // scale that keeps every joint relationship intact at every ladder
    // rung. Feet stay planted: the anchor is computed from the SCALED
    // idle against the host's own foot plane, and rescaled host clips
    // are re-planted on that plane explicitly. The graceful-skip path
    // (no mirror at all) keeps the monster-scale mesh with the retail
    // clips - coherent, just sibling-statured, as before.
    let native = 1.0 / ctx.radial;
    let scale_native = (native - 1.0).abs() > 0.02;
    let scale_rows = |rows: &mut [Vec<PartPose>], s: f32| {
        for row in rows.iter_mut() {
            for p in row.iter_mut() {
                let t = |v: i16| ((v as f32) * s).round().clamp(-2048.0, 2047.0) as i16;
                p.tx = t(p.tx);
                p.ty = t(p.ty);
                p.tz = t(p.tz);
            }
        }
    };
    let anchor = {
        let mut probe = idle_rows.clone();
        if scale_native {
            scale_rows(&mut probe, native);
        }
        monster_anchor(&probe, &host_idle.frames)
    };
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
        if scale_native {
            scale_rows(&mut rows, native);
        }
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

    // Native stature: convert every entry the options KEPT into a
    // rewrite of its own decoded retail rows at the native scale,
    // re-planted on the host's foot plane - a kept host clip at raw
    // monster translations would fling the rescaled mesh apart.
    if scale_native {
        let host_floor_y: i16 = host_idle
            .frames
            .iter()
            .filter_map(|f| {
                [11usize, 14]
                    .iter()
                    .filter_map(|&c| f.get(c))
                    .map(|p| p.ty)
                    .max()
            })
            .max()
            .unwrap_or(0);
        let replant = (host_floor_y as f32 * (1.0 - native)).round() as i16;
        let retail_anims = monster_archive::animations(retail_archive, target_id)?
            .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: no animations"))?;
        let mut kept_scaled = 0usize;
        for (i, a) in retail_anims.iter().enumerate() {
            if pending.contains_key(&i) || i >= ret_entries.len() {
                continue;
            }
            if a.frames.is_empty()
                || a.frames.len() > 255
                || a.frames[0].len() != ret_entries[i].parts
            {
                continue;
            }
            let mut rows = a.frames.clone();
            scale_rows(&mut rows, native);
            apply_anchor(&mut rows, [0, replant, 0]);
            pending.insert(
                i,
                Pending {
                    rows,
                    rate: a.rate.max(1),
                    floor: 1,
                    staged: false,
                },
            );
            kept_scaled += 1;
        }
        notes.push(format!(
            "figure at native hero stature (x{native:.2}){}",
            if kept_scaled > 0 {
                format!("; {kept_scaled} kept host clips rescaled")
            } else {
                String::new()
            }
        ));
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
    let mut block = rebuild_block_entries(current_block, &retail_block, &rewrites)?;
    if scale_native {
        scale_block_tmd_vertices(&mut block, native)
            .context("scale block TMD vertices to native stature")?;
    }
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

#[cfg(test)]
mod tests;
