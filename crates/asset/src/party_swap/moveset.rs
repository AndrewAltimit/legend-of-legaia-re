//! The swapped party's **whole** art-animation stream set: one `"ME"`
//! archive rebuilt from nothing but the mapped sibling's own monster
//! clips, so every art a hero performs plays a Delilas motion.
//!
//! ## Why the archive is rebuilt rather than extended
//!
//! A character's art streams live in one `0x10800`-byte `readef.DAT`
//! slot (`3*char + 1`, [`winpose::READEF_SLOT`]), and retail fills most
//! of it: measured on the USA disc the three main slots have 20374 /
//! 2446 / 17361 bytes free. The Noa slot therefore cannot take even one
//! more full-length clip, so "add the sibling's motions alongside the
//! host's" is not a shape the disc has room for.
//!
//! It does not need to be. Retail already points several art records at
//! one stream (Vahn's 25 art records resolve to at most 17 streams), so
//! the record's `+0x0A` stream index is a free-standing pointer and the
//! archive can be re-authored wholesale as long as every record that
//! reads it is repointed in the same pass. This module emits that
//! archive: the signature-special stream carried over byte-identical,
//! the sibling's locomotion clip, and one entry per distinct sibling
//! swing. Everything the host shipped is dropped, which is what buys the
//! space back.
//!
//! ## What counts as a swing
//!
//! The monster archive tags each action entry
//! (`crate::monster_archive::MonsterAnimation::action_id`). The three
//! siblings' attack families sit in the `0x0C..=0x1F` band (Gi
//! `0x0F/0x0D/0x0E`, Che `0x0F/0x0D/0x10/0x0E`, Lu `0x0D/0x13/0x0E/0x12`),
//! while their signature specials are tagged `0x23` and their
//! locomotion `0x01`. Selecting the band and subtracting the caller's
//! signature chain therefore yields 3 / 4 / 4 distinct swings without
//! any per-sibling table. The `0x23` tag alone cannot separate a special
//! from an ordinary castable (Lu carries an unstaged one), which is why
//! the chain is passed in rather than inferred.

use anyhow::{Context, Result, bail};

use crate::me_archive;
use crate::monster_archive::{self, MonsterAnimation};
use crate::party_swap::PlayerRig;
use crate::party_swap::winpose::{self, READEF_SLOT, pack_part, retarget_clip_wrist};

/// Action-tag band the siblings' ordinary attack clips occupy.
pub const SWING_TAG_LO: u8 = 0x0C;
/// Inclusive top of [`SWING_TAG_LO`]'s band.
pub const SWING_TAG_HI: u8 = 0x1F;
/// Action tag of a monster's locomotion / approach clip.
pub const APPROACH_TAG: u8 = 0x01;

/// One stream of a rebuilt moveset archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovesetStream {
    /// Monster-archive entry index the stream was retargeted from, or
    /// `None` for the carried-over signature body.
    pub source_entry: Option<usize>,
    /// Action tag of the source clip (`0` for the carried signature).
    pub action_tag: u8,
    /// Keyframes in the emitted stream.
    pub frames: usize,
    /// Playback-rate byte an art record pointed here should carry, so
    /// the clip runs at its authored pace.
    pub rate: u8,
}

/// A rebuilt art-stream archive plus the role of each of its entries.
#[derive(Debug, Clone)]
pub struct RebuiltMoveset {
    /// The full `READEF_SLOT`-byte slot, zero-padded past the archive.
    pub bytes: Vec<u8>,
    /// Per-entry descriptors, indexed by `"ME"` entry index.
    pub streams: Vec<MovesetStream>,
    /// Entry index of the carried signature stream (always `0`).
    pub signature: usize,
    /// Entry index of the sibling's locomotion clip - what the two
    /// combo-starter records play.
    pub approach: usize,
    /// Entry indices of the swing streams, in source order.
    pub swings: Vec<usize>,
    /// Bytes the emitted archive occupies (header + bodies).
    pub used: usize,
}

impl RebuiltMoveset {
    /// The swing entry the `nth` repointed art record should read.
    /// Records are distributed round-robin over the swings so a
    /// character with more arts than the sibling has motions still
    /// spreads them evenly instead of piling onto one clip.
    pub fn swing_for(&self, nth: usize) -> usize {
        self.swings[nth % self.swings.len()]
    }
}

/// The sibling's distinct ordinary-attack clips: every entry whose action
/// tag falls in the swing band and that no stage of `signature_chain`
/// claims, in archive order.
pub fn swing_entries(anims: &[MonsterAnimation], signature_chain: &[usize]) -> Vec<usize> {
    (0..anims.len())
        .filter(|i| !signature_chain.contains(i))
        .filter(|&i| (SWING_TAG_LO..=SWING_TAG_HI).contains(&anims[i].action_id))
        .collect()
}

/// The sibling's locomotion clip, if it carries one.
pub fn approach_entry(anims: &[MonsterAnimation]) -> Option<usize> {
    anims.iter().position(|a| a.action_id == APPROACH_TAG)
}

/// Rebuild a character's main art `"ME"` slot around the sibling's own
/// motions.
///
/// `slot` is the **current** slot (so the signature reskin's own rebuild
/// is already in it); entry `signature_entry`'s stored body is carried
/// over byte-identical, flag bit included, and lands at index `0`. Every
/// other entry is discarded and replaced by the retargeted sibling
/// clips, each written at its own authored frame count.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_moveset_archive(
    slot: &[u8],
    signature_entry: usize,
    anims: &[MonsterAnimation],
    approach: Option<usize>,
    swings: &[usize],
    rig: &PlayerRig,
    player_file: &[u8],
    archive_entry: &[u8],
    source_id: u16,
    natural_wrist_hand: Option<usize>,
) -> Result<RebuiltMoveset> {
    if slot.len() != READEF_SLOT {
        bail!("art slot is {} bytes, expected {READEF_SLOT}", slot.len());
    }
    if swings.is_empty() {
        bail!("monster {source_id} carries no swing clips to build a moveset from");
    }
    let ar = me_archive::parse(slot).context("parse the art ME archive")?;
    if signature_entry >= ar.len() {
        bail!(
            "signature stream {signature_entry} is past the archive's {} entries",
            ar.len()
        );
    }
    // Every stream in a slot is authored at one part count; take it from
    // the entry being carried over so the new bodies match the rig the
    // renderer poses.
    let carried = ar
        .raw_body(signature_entry)
        .ok_or_else(|| anyhow::anyhow!("signature stream body missing"))?;
    let carried_flag = ar.is_compressed(signature_entry) == Some(true);
    let sig_decoded = ar
        .entry(signature_entry)
        .context("decode the signature stream")?;
    if sig_decoded.len() < 2 {
        bail!("signature stream is empty");
    }
    let parts = sig_decoded[0] as usize;

    let mut streams = vec![MovesetStream {
        source_entry: None,
        action_tag: 0,
        frames: sig_decoded[1] as usize,
        // The signature's rate is written by its own reskin pass; this
        // record is descriptive only.
        rate: 0,
    }];
    let mut bodies: Vec<(Vec<u8>, bool)> = vec![(carried.to_vec(), carried_flag)];

    let emit = |entry: usize,
                streams: &mut Vec<MovesetStream>,
                bodies: &mut Vec<(Vec<u8>, bool)>|
     -> Result<usize> {
        let clip = anims
            .get(entry)
            .ok_or_else(|| anyhow::anyhow!("monster {source_id} has no entry {entry}"))?;
        let frames = clip.frame_count.clamp(1, u8::MAX as usize);
        let rows = retarget_clip_wrist(
            clip,
            rig,
            player_file,
            archive_entry,
            source_id,
            parts,
            frames,
            natural_wrist_hand,
        )
        .with_context(|| format!("retarget monster {source_id} entry {entry}"))?;
        let mut decoded = Vec::with_capacity(2 + parts * frames * 9);
        decoded.push(parts as u8);
        decoded.push(frames as u8);
        for row in &rows {
            for p in row {
                decoded.extend_from_slice(&pack_part(p));
            }
        }
        let encoded = winpose::encode_channel_delta(&decoded)
            .with_context(|| format!("encode entry {entry}"))?;
        // Never ship a body the retail decoder would read back
        // differently - the delta codec's state is subtle.
        let back = me_archive::decode_channel_delta(&encoded)
            .with_context(|| format!("re-decode entry {entry}"))?;
        if back != decoded {
            bail!("moveset entry from monster clip {entry}: codec round-trip mismatch");
        }
        let index = streams.len();
        streams.push(MovesetStream {
            source_entry: Some(entry),
            action_tag: clip.action_id,
            frames,
            rate: clip.rate.max(1),
        });
        bodies.push((encoded, true));
        Ok(index)
    };

    // The locomotion clip goes first after the signature, so its index is
    // stable whatever the swing count. Falling back to the first swing
    // keeps the archive well-formed for a sibling with no `0x01` entry.
    let approach_index = match approach {
        Some(e) => emit(e, &mut streams, &mut bodies)?,
        None => emit(swings[0], &mut streams, &mut bodies)?,
    };
    let mut swing_indices = Vec::with_capacity(swings.len());
    for &e in swings {
        swing_indices.push(emit(e, &mut streams, &mut bodies)?);
    }

    let n = bodies.len();
    let used = 3 + 2 * n + bodies.iter().map(|(b, _)| b.len()).sum::<usize>();
    if used > READEF_SLOT {
        bail!("rebuilt moveset archive ({used} bytes) exceeds the {READEF_SLOT}-byte slot");
    }
    let mut out = Vec::with_capacity(READEF_SLOT);
    out.extend_from_slice(&me_archive::MAGIC);
    out.push(n as u8);
    for (b, compressed) in &bodies {
        let flag = if *compressed { 0x8000u16 } else { 0 };
        out.extend_from_slice(&((b.len() as u16) | flag).to_le_bytes());
    }
    for (b, _) in &bodies {
        out.extend_from_slice(b);
    }
    out.resize(READEF_SLOT, 0);
    Ok(RebuiltMoveset {
        bytes: out,
        streams,
        signature: 0,
        approach: approach_index,
        swings: swing_indices,
        used,
    })
}

/// The frame count of each entry of an art `"ME"` slot - what an art
/// record's frame-indexed fields were authored against before a repoint.
pub fn entry_frames(slot: &[u8]) -> Result<Vec<usize>> {
    let ar = me_archive::parse(slot).context("parse the art ME archive")?;
    (0..ar.len())
        .map(|i| {
            let d = ar.entry(i).with_context(|| format!("decode entry {i}"))?;
            Ok(d.get(1).copied().unwrap_or(0) as usize)
        })
        .collect()
}

/// Read a monster's animation entries, or an empty list when the slot
/// carries none.
pub fn sibling_animations(archive_entry: &[u8], source_id: u16) -> Result<Vec<MonsterAnimation>> {
    Ok(monster_archive::animations(archive_entry, source_id)
        .with_context(|| format!("read monster {source_id} animations"))?
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(tag: u8) -> MonsterAnimation {
        MonsterAnimation {
            action_id: tag,
            rate: 2,
            attach_key: 0,
            part_count: 15,
            frame_count: 20,
            frames: Vec::new(),
            effect_script: Vec::new(),
        }
    }

    #[test]
    fn swing_entries_take_the_band_minus_the_signature_chain() {
        // Gi's shape: idle/move/reactions, three attacks, a 3-stage
        // `0x23` signature, a victory clip.
        let anims: Vec<MonsterAnimation> = [
            0x00, 0x01, 0x03, 0x02, 0x04, 0x05, 0x0B, 0x0F, 0x0D, 0x0E, 0x23, 0x23, 0x23, 0x22,
        ]
        .iter()
        .map(|&t| anim(t))
        .collect();
        assert_eq!(swing_entries(&anims, &[10, 11, 12]), vec![7, 8, 9]);
        assert_eq!(approach_entry(&anims), Some(1));
    }

    #[test]
    fn a_chain_stage_inside_the_band_is_still_excluded() {
        // Lu's shape: her signature chain reaches down into the band
        // (`0x0C` stages), so tag alone would over-collect.
        let anims: Vec<MonsterAnimation> = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x0B, 0x0D, 0x13, 0x0E, 0x12, 0x23, 0x0C, 0x0C,
            0x23, 0x22,
        ]
        .iter()
        .map(|&t| anim(t))
        .collect();
        assert_eq!(swing_entries(&anims, &[14, 12, 13]), vec![7, 8, 9, 10]);
    }

    #[test]
    fn swing_for_wraps_round_robin() {
        let m = RebuiltMoveset {
            bytes: Vec::new(),
            streams: Vec::new(),
            signature: 0,
            approach: 1,
            swings: vec![2, 3, 4],
            used: 0,
        };
        assert_eq!(
            (0..7).map(|i| m.swing_for(i)).collect::<Vec<_>>(),
            vec![2, 3, 4, 2, 3, 4, 2]
        );
    }
}
