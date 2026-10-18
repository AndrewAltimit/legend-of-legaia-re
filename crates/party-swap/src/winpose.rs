//! Win-pose replacement: the party's victory animations (the eight
//! streams of each character's **base "ME" archive**, readef.DAT slot
//! `3*char + 2` - see `docs/formats/battle-data-pack.md` § "ME" stream
//! archives) rebuild from the mapped sibling's own victory clip, so a
//! swapped character celebrates like the Delilas they depict.
//!
//! The sibling's monster clip poses canonical parts; the baked player
//! model wears those parts on the player rig via the pivot bake, so the
//! clip retargets with the bake's own per-part conjugation:
//! `R_play = R_sib * R_src_rest^T * R_align^T * R_dst_rest` (the bake's
//! linear map cancelled out of the played pose). Translations do NOT
//! carry over: only the torso keeps the clip's own placement
//! (`T_play = r * T_sib`) and the rest of the skeleton hangs off it by
//! forward kinematics over the baked rig's bone vectors, because the
//! baked parts sit at the HOST's joint spacing, not the sibling's.
//! Each retail entry keeps its exact frame count (the art records'
//! timing fields stay retail) by nearest-frame resampling, and the
//! rebuilt entries re-encode with the retail channel-delta codec.
//!
//! A win pose is **not** a one-shot stream. Every one of the 24 retail
//! base records carries a loop window: entry `+0x84 = 0xFF` seeds the
//! hold counter `actor +0x176` and `+0x85`/`+0x86` bound the frames the
//! tick replays (`FUN_80047430` at `0x80047768`: once the cursor reaches
//! `+0x86 << 4` it subtracts `(+0x86 - +0x85) << 4` and decrements the
//! counter, so the stream cycles frames `[+0x85, +0x86]` up to 255 times
//! before the results sequencer moves on). Retail authors those frames
//! as a seamless celebration cycle - measured over all 24 records, the
//! pose gap across the wrap is 0.1-1.0 model units and 0.35-2.01 degrees
//! per part, at or below the window's own mean frame step.
//!
//! Uniformly resampling a one-shot flourish into that shape points the
//! window at the flourish's own tail, so the last second of the clip
//! replays for as long as the results panel is up - measured on the
//! rebuilt streams, a 4.0-164.1 unit / 5.4-122.9 degree snap at every
//! wrap against retail's 0.1-1.0 / 0.35-2.01.
//!
//! So the rebuilt stream is composed, not uniformly resampled: the
//! sibling's flourish plays over the lead-in `[0, +0x85)` and what the
//! sibling itself replays after winning fills `[+0x85, +0x86]`,
//! phase-mapped so the frame the wrap lands on is the frame it wraps to.
//! See [`victory_cycle`] and [`compose_base_stream`].

use super::*;
use legaia_actor_formats::me_archive;

mod art_slot;
mod idle;
mod pose_codec;
mod retarget;
mod victory_loop;

pub use art_slot::{RebuiltArtSlot, art_entry_shape, rebuild_art_slot_entry, retimed_rate};
pub use idle::{RebuiltIdle, idle_anchor, rebuild_idle_stream};
pub use pose_codec::encode_channel_delta;
pub(crate) use pose_codec::{mmul, pack_part, to_euler, transpose, unpack_part};
pub use retarget::{retarget_clip, retarget_clip_wrist};
use victory_loop::pose_dist;
pub use victory_loop::{
    LoopWindow, base_loop_windows, compose_base_stream, victory_clip, victory_cycle,
};

/// readef.DAT slot stride.
pub const READEF_SLOT: usize = 0x10800;

/// The base ("ME") archive slot carrying character `char_index`'s win
/// poses (0 = Vahn, 1 = Noa, 2 = Gala).
pub fn base_slot_index(char_index: usize) -> usize {
    3 * char_index + 2
}

/// Rebuild a base "ME" archive slot: every entry becomes the retargeted
/// sibling victory clip at that entry's RETAIL frame count, re-encoded
/// with the retail codec. Returns the full `READEF_SLOT`-byte slot
/// (zero-padded past the archive).
pub fn rebuild_base_slot(
    slot: &[u8],
    clip: &legaia_actor_formats::monster_archive::MonsterAnimation,
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    natural_wrist_hand: Option<usize>,
) -> Result<Vec<u8>> {
    if slot.len() != READEF_SLOT {
        bail!("base slot is {} bytes, expected {READEF_SLOT}", slot.len());
    }
    let ar = me_archive::parse(slot).context("parse base ME archive")?;
    let n = ar.len();
    // Entries 4/5 back the WEAK victory actions (0x15/0x16), which the
    // results sequencer LOOPS - retail authors them as near-static
    // breathing so the loop is invisible. A looping victory flourish
    // visibly replays, and a frozen final pose reads as the animation
    // skipping to its end - so those entries carry the sibling's IDLE
    // clip instead: authored as a seamless cycle, it survives the loop
    // the way retail's breathing does.
    let idle = monster_archive::idle_animation(archive_entry, source_id)
        .context("weak-entry idle clip")?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle for weak entries"))?;
    // What fills each entry's loop window, for the six flourish entries:
    // the sibling's OWN post-win loop (Lu's `[16, 24]` sway, Gi's
    // single-frame hold), else a hold on the flourish's last frame -
    // which is also Che's own house style on the flourishes she does
    // annotate. Retail's 24 hero windows carry 0.07-0.64 degrees of drift
    // per frame, i.e. a held pose with a breath in it, so a hold is the
    // retail-shaped filler and not a compromise; taking it out of the
    // clip also means both boundaries of the composed stream - the
    // handoff into the window and the wrap inside it - are exact.
    let cycle_range =
        victory_cycle(archive_entry, source_id, clip).context("sibling victory loop window")?;
    let windows = base_loop_windows(player_file).context("host base loop windows")?;
    let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(n);
    for i in 0..n {
        let retail = ar.entry(i).with_context(|| format!("decode entry {i}"))?;
        let (parts, frames) = (retail[0] as usize, retail[1] as usize);
        // Retail frames stay VERBATIM outside the loop window. The base
        // streams are not victory-only: anim 0x11 (entry 0) is the
        // Spirit / Super power-up flourish the battle plays right after
        // every 0x10 charge (probe: actor `+0x1D9` steps 0x10 -> 0x11 in
        // the e2e telemetry, retail and swapped alike), and a
        // victory-retargeted flourish there swept the sibling's limbs
        // across the battle closeup as dark mesh streaks - the "Spirit
        // distortion". Mid-battle playback only reaches the lead-in, so
        // the sibling's celebration lives ONLY in the loop window the
        // results screen parks on; the flourish frames every in-battle
        // context can reach are the host's own.
        let retail_frames: Vec<Vec<PartPose>> = (0..frames)
            .map(|h| {
                (0..parts)
                    .map(|q| unpack_part(&retail[2 + (h * parts + q) * 9..]))
                    .collect()
            })
            .collect();
        let frames_out = match windows.get(&i).filter(|w| w.loops(frames)) {
            Some(&w) => {
                // What fills the window: the sibling's own post-win
                // cycle (or its idle for the looped weak-victory
                // entries), retargeted alone and phase-picked in HOST
                // space against the retail lead-in's last frame.
                let cycle_frames: Vec<Vec<PartPose>> = match (i, &cycle_range) {
                    (4..=5, _) => idle.frames.clone(),
                    (_, Some(r)) => clip.frames[r.clone()].to_vec(),
                    _ => vec![clip.frames[clip.frames.len() - 1].clone()],
                };
                let cyc_anim = legaia_actor_formats::monster_archive::MonsterAnimation {
                    variant_track: Default::default(),
                    frame_count: cycle_frames.len(),
                    frames: cycle_frames,
                    ..clip.clone()
                };
                let cyc_len = cyc_anim.frame_count;
                let cyc = retarget_clip_wrist(
                    &cyc_anim,
                    rig,
                    player_file,
                    archive_entry,
                    source_id,
                    parts,
                    cyc_len,
                    natural_wrist_hand,
                )?;
                let hw0 = w.start.min(frames);
                let mut out: Vec<Vec<PartPose>> = retail_frames[..hw0].to_vec();
                let k = match out.last() {
                    Some(tail) => (0..cyc.len())
                        .min_by(|&a, &b| {
                            pose_dist(tail, &cyc[a]).total_cmp(&pose_dist(tail, &cyc[b]))
                        })
                        .unwrap_or(0),
                    None => 0,
                };
                let span = w.end.saturating_sub(w.start).max(1);
                for h in hw0..frames {
                    let j = h - w.start;
                    out.push(cyc[(k + j * cyc.len() / span) % cyc.len()].clone());
                }
                out
            }
            None => retail_frames,
        };
        let mut decoded = Vec::with_capacity(2 + parts * frames * 9);
        decoded.push(parts as u8);
        decoded.push(frames as u8);
        for row in &frames_out {
            for p in row {
                decoded.extend_from_slice(&pack_part(p));
            }
        }
        let encoded = encode_channel_delta(&decoded)?;
        // Self-check: the retail decoder must reproduce the stream
        // bit-exact (the codec's delta state is subtle; never ship an
        // entry the game would mis-decode).
        let back = me_archive::decode_channel_delta(&encoded)
            .with_context(|| format!("re-decode entry {i}"))?;
        if back != decoded {
            bail!("entry {i}: codec round-trip mismatch");
        }
        bodies.push(encoded);
    }
    let total = 3 + 2 * n + bodies.iter().map(|b| b.len()).sum::<usize>();
    if total > READEF_SLOT {
        bail!("rebuilt base archive ({total} bytes) exceeds the slot");
    }
    let mut out = Vec::with_capacity(READEF_SLOT);
    out.extend_from_slice(&me_archive::MAGIC);
    out.push(n as u8);
    for b in &bodies {
        out.extend_from_slice(&((b.len() as u16) | 0x8000).to_le_bytes());
    }
    for b in &bodies {
        out.extend_from_slice(b);
    }
    out.resize(READEF_SLOT, 0);
    Ok(out)
}

#[cfg(test)]
mod tests;
