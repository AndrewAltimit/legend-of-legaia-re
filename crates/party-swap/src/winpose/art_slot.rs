//! Rebuilding an art slot's entry: its shape, the retimed rate, the chain it
//! builds and the assembled slot.

use super::super::*;
use super::READEF_SLOT;
use super::pose_codec::{encode_channel_delta, pack_part};
use super::retarget::retarget_clip_wrist;
use legaia_actor_formats::me_archive;

/// Rebuild ONE entry of a (main art) "ME" archive slot: entry
/// `entry_index` becomes the retargeted sibling `clip` at the entry's
/// RETAIL `(parts, frames)` shape - so the art record's timing, effect
/// script and cue track stay valid - and every other entry's stored
/// body is carried over byte-identical (size-table flags included).
/// Returns the full `READEF_SLOT`-byte slot, zero-padded.
/// The `(parts, frames)` shape a readef art slot's entry is authored at.
///
/// A retarget writes into this shape, so the caller needs it before it
/// can work out what playback rate keeps the source clip's pace.
pub fn art_entry_shape(slot: &[u8], entry_index: usize) -> Result<(usize, usize)> {
    let ar = me_archive::parse(slot).context("parse art ME archive")?;
    let decoded = ar
        .entry(entry_index)
        .with_context(|| format!("decode art entry {entry_index}"))?;
    if decoded.len() < 2 {
        bail!("art entry {entry_index} is empty");
    }
    Ok((decoded[0] as usize, decoded[1] as usize))
}

/// The rate byte that makes `src_frames` of source choreography, authored
/// at `src_rate`, take the same wall time once resampled into a
/// `host_frames` stream.
///
/// The cursor advances `rate / 8` keyframes per 60 Hz tick at the normal
/// `actor[+0x21D] == 4` (`FUN_80047430`), so a clip runs for
/// `frames * 8 / rate` ticks and holding that constant across the
/// resample gives `rate' = host_frames * rate / src_frames`. Retail rates
/// are not all 2 - Noa's Vulture Blade stream is authored at 6 - so this
/// cannot be a constant.
pub fn retimed_rate(host_frames: usize, src_frames: usize, src_rate: u8) -> u8 {
    if src_frames == 0 {
        return src_rate.max(1);
    }
    let scaled = (host_frames * src_rate as usize * 2)
        .div_ceil(src_frames)
        .div_ceil(2);
    scaled.clamp(1, u8::MAX as usize) as u8
}

/// A rebuilt art slot, plus the frame count the new entry ended up with.
///
/// The caller needs the count: the art record's own frame-indexed fields
/// (hit events, effect-script gates, the loop window) are relative to it,
/// so they have to be rescaled by whatever ratio the rebuild chose.
pub struct RebuiltArtSlot {
    pub bytes: Vec<u8>,
    /// Frames in the rebuilt entry.
    pub frames: usize,
    /// Frames the retail entry carried.
    pub retail_frames: usize,
    /// How many chain stages made it in (the front is dropped first when
    /// the slot is tight).
    pub stages: usize,
    /// The rate byte the concatenated stream wants.
    pub rate: u8,
    /// Frame index where the **payoff** stage - the last one, the strike
    /// itself - begins in the concatenated stream. Everything before it is
    /// wind-up, so it is the floor for anything the art record schedules
    /// on contact.
    pub payoff_start: usize,
    /// How many frames the payoff stage occupies, after the rate
    /// normalisation that stretched it to the chain's common rate.
    pub payoff_frames: usize,
}

/// Rebuild one entry of a readef art slot around a monster clip.
///
/// The entry is written at the **clip's own length** when the slot has
/// room for it, and only falls back to the retail frame count when it
/// does not. Resampling a 39-frame clip down to a 21-frame stream throws
/// away 18 of its poses and forces a compensating rate edit; keeping the
/// authored length costs about 2 KB in a slot that has 20 KB free, needs
/// no rate edit at all, and doubles the pose rate the player sees.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_art_slot_entry(
    slot: &[u8],
    entry_index: usize,
    chain: &[&legaia_actor_formats::monster_archive::MonsterAnimation],
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    natural_wrist_hand: Option<usize>,
) -> Result<RebuiltArtSlot> {
    if slot.len() != READEF_SLOT {
        bail!("art slot is {} bytes, expected {READEF_SLOT}", slot.len());
    }
    if chain.is_empty() {
        bail!("no clip to retarget");
    }
    let ar = me_archive::parse(slot).context("parse art ME archive")?;
    let n = ar.len();
    if entry_index >= n {
        bail!("art archive has {n} entries, wanted {entry_index}");
    }
    let retail = ar
        .entry(entry_index)
        .with_context(|| format!("decode art entry {entry_index}"))?;
    let (parts, retail_frames) = (retail[0] as usize, retail[1] as usize);
    // Everything but the rebuilt entry is carried verbatim, so the room
    // available is the slot's free space plus what this entry gives back.
    let others: usize = (0..n)
        .filter(|&i| i != entry_index)
        .map(|i| ar.raw_body(i).map_or(0, |b| b.len()))
        .sum();
    let headroom = READEF_SLOT.saturating_sub(3 + 2 * n + others);

    // Try the whole chain, then drop stages from the FRONT until it fits
    // (the last stage is the payoff - a move that loses its wind-up still
    // reads; one that loses its strike does not), and only then fall back
    // to the retail shape.
    for start in 0..chain.len() {
        let stages = &chain[start..];
        let full = chain_rate(stages);
        // Coarser keyframe density BEFORE a lost stage. Halving the rate
        // and the frame count together keeps the clip's duration exactly
        // (`frames * 8 / rate`) while costing half its poses, and losing
        // pose density is a far smaller loss than losing the whole
        // wind-up: several of these stages are authored at rate 1 in the
        // first place, so the coarser stream is a density retail itself
        // ships. Only exact divisors, so no stage is re-timed.
        for rate in [full, full / 2, full / 4] {
            if rate == 0 || !full.is_multiple_of(rate) {
                continue;
            }
            let Some((frames, per_stage)) = chain_frames(stages, rate) else {
                continue;
            };
            let built = build_chain(
                stages,
                &per_stage,
                rig,
                player_file,
                archive_entry,
                source_id,
                parts,
                natural_wrist_hand,
            )?;
            if built.len() <= headroom {
                let payoff_frames = *per_stage.last().unwrap_or(&frames);
                return Ok(RebuiltArtSlot {
                    bytes: assemble_slot(&ar, n, entry_index, &built)?,
                    frames,
                    retail_frames,
                    stages: stages.len(),
                    rate,
                    payoff_start: frames - payoff_frames,
                    payoff_frames,
                });
            }
        }
    }
    // Last resort: the final stage squeezed into the retail frame count.
    let last = chain[chain.len() - 1];
    let built = build_chain(
        &[last],
        &[retail_frames],
        rig,
        player_file,
        archive_entry,
        source_id,
        parts,
        natural_wrist_hand,
    )?;
    Ok(RebuiltArtSlot {
        bytes: assemble_slot(&ar, n, entry_index, &built)?,
        frames: retail_frames,
        retail_frames,
        stages: 1,
        rate: winpose_rate(last),
        // The one surviving stage IS the payoff, so it starts at zero.
        payoff_start: 0,
        payoff_frames: retail_frames,
    })
}

/// The playback rate a concatenated chain runs at: the fastest stage's,
/// so every slower stage can be stretched up to it rather than any stage
/// being decimated down.
fn chain_rate(stages: &[&legaia_actor_formats::monster_archive::MonsterAnimation]) -> u8 {
    stages.iter().map(|c| winpose_rate(c)).max().unwrap_or(1)
}

fn winpose_rate(clip: &legaia_actor_formats::monster_archive::MonsterAnimation) -> u8 {
    clip.rate.max(1)
}

/// Per-stage frame counts that preserve each stage's authored duration
/// once the whole chain plays at one rate, and their total.
///
/// A clip runs for `frames * 8 / rate` ticks, so a stage authored at
/// `rate_i` needs `frames_i * R / rate_i` frames to last as long at the
/// common rate `R`. `None` when the total will not fit the stream head's
/// `u8` frame count.
fn chain_frames(
    stages: &[&legaia_actor_formats::monster_archive::MonsterAnimation],
    target_rate: u8,
) -> Option<(usize, Vec<usize>)> {
    let r = target_rate.max(1) as usize;
    let per: Vec<usize> = stages
        .iter()
        .map(|c| {
            (c.frame_count * r)
                .div_ceil(winpose_rate(c) as usize)
                .max(1)
        })
        .collect();
    let total: usize = per.iter().sum();
    (total <= u8::MAX as usize).then_some((total, per))
}

/// Retarget each stage at its own resampled length and concatenate them
/// into one stream.
#[allow(clippy::too_many_arguments)]
fn build_chain(
    stages: &[&legaia_actor_formats::monster_archive::MonsterAnimation],
    per_stage: &[usize],
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    parts: usize,
    natural_wrist_hand: Option<usize>,
) -> Result<Vec<u8>> {
    let mut rows: Vec<Vec<PartPose>> = Vec::new();
    for (clip, &frames) in stages.iter().zip(per_stage) {
        rows.extend(retarget_clip_wrist(
            clip,
            rig,
            player_file,
            archive_entry,
            source_id,
            parts,
            frames,
            natural_wrist_hand,
        )?);
    }
    let frames = rows.len();
    let mut decoded = Vec::with_capacity(2 + parts * frames * 9);
    decoded.push(parts as u8);
    decoded.push(frames as u8);
    for row in &rows {
        for p in row {
            decoded.extend_from_slice(&pack_part(p));
        }
    }
    let encoded = encode_channel_delta(&decoded)?;
    let back = me_archive::decode_channel_delta(&encoded).context("re-decode rebuilt art entry")?;
    if back != decoded {
        bail!("rebuilt art entry: codec round-trip mismatch");
    }
    Ok(encoded)
}

/// Re-emit the whole slot with `entry_index`'s body replaced; every other
/// entry is carried verbatim, flag bit included.
fn assemble_slot(
    ar: &me_archive::MeArchive<'_>,
    n: usize,
    entry_index: usize,
    encoded: &[u8],
) -> Result<Vec<u8>> {
    let mut sizes: Vec<u16> = Vec::with_capacity(n);
    let mut bodies: Vec<&[u8]> = Vec::with_capacity(n);
    for i in 0..n {
        if i == entry_index {
            sizes.push((encoded.len() as u16) | 0x8000);
            bodies.push(encoded);
        } else {
            let body = ar
                .raw_body(i)
                .ok_or_else(|| anyhow::anyhow!("art entry {i} body missing"))?;
            let flag = if ar.is_compressed(i) == Some(true) {
                0x8000
            } else {
                0
            };
            sizes.push((body.len() as u16) | flag);
            bodies.push(body);
        }
    }
    let total = 3 + 2 * n + bodies.iter().map(|b| b.len()).sum::<usize>();
    if total > READEF_SLOT {
        bail!("rebuilt art archive ({total} bytes) exceeds the slot");
    }
    let mut out = Vec::with_capacity(READEF_SLOT);
    out.extend_from_slice(&me_archive::MAGIC);
    out.push(n as u8);
    for s in &sizes {
        out.extend_from_slice(&s.to_le_bytes());
    }
    for b in &bodies {
        out.extend_from_slice(b);
    }
    out.resize(READEF_SLOT, 0);
    Ok(out)
}
