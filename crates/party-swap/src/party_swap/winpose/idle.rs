//! Rebuilding the idle stream around an anchor pose.

use super::super::*;
use super::pose_codec::pack_part;
use super::retarget::retarget_clip_wrist;

/// One rebuilt battle-idle stream: the packed bytes and where they go in
/// the player file's decoded `record0`.
pub struct RebuiltIdle {
    /// `record0`-image byte offset of the stream body (past the
    /// `[parts][frames]` head, which is left exactly as retail wrote it).
    pub offset: usize,
    /// Packed 9-byte-per-part records, the same length as the bytes they
    /// replace.
    pub bytes: Vec<u8>,
    /// Frames the host idle carries - unchanged, because the length is
    /// the budget.
    pub frames: usize,
    /// Speed multiplier against the sibling's authored cycle: `1.0` is
    /// its own tempo, `2.0` twice as fast. Always `> 1` in practice - the
    /// host idles are 8-9 frames against the siblings' 13-35, and the
    /// rate byte floors at `1` where all three hosts already sit, so a
    /// stream that has to fit in fewer frames cannot be slowed back down.
    pub pace: f32,
}

/// The single whole-body translation that seats a retargeted idle on the
/// host's own rest: `[dx, dy, dz]`, to be added to EVERY part of every
/// frame.
///
/// Rigid is not a nicety. Battle poses are flat - each part carries an
/// absolute `R * v + T` about the object origin and nothing in the stream
/// hangs one part off another - so a translation written to one channel
/// moves that part and nothing else. Applied to the torso channel alone
/// it does not re-seat the character at all; it shears the torso off the
/// body, which is what a 21-90 unit tear at every torso and pelvis joint
/// looked like in game (measured against the 1.8-2.4 units retail's own
/// idles carry: `crates/patcher/tests/party_swap_idle_continuity_real.rs`).
///
/// The two axes come from different places because they answer different
/// questions.
///
/// **x / z from the torso.** The torso is the FK root the retarget hangs
/// the skeleton off, and it is the part the actor's world position, the
/// attack camera and the damage numbers all frame. Anchoring the SUPPORT
/// POINT (the ankles' midpoint) instead is the physically tidier reading
/// but costs more: the siblings plant their feet 16-101 units from where
/// the host plants theirs, so foot-anchoring slides the visible body that
/// far off its mark, up to a third of a body height. The feet landing
/// where the sibling's stance puts them IS the stance.
///
/// **y from the floor.** The character has to stand ON the ground, and
/// the torso's height above it is exactly the part of the stance worth
/// keeping - Lu stands taller than Gala, Che crouches under Noa. Taking y
/// from the torso as well pins the wrong end of the leg: measured, it
/// floated Lu 46-50 units clear of the floor on both her hosts and sank
/// Che 48 into it on Noa's. The floor is read as the DEEPEST (largest y -
/// GTE space is y-down) ankle pivot over the whole cycle, on both sides,
/// so a lifted foot in either frame 0 cannot bias it and no frame of the
/// rebuilt idle plants deeper than retail's own idle does.
pub fn idle_anchor(rows: &[Vec<PartPose>], host: &[Vec<PartPose>], rig: &PlayerRig) -> [i16; 3] {
    let torso = rig.channel_for_canonical[1] as usize;
    let feet = [
        rig.channel_for_canonical[11] as usize,
        rig.channel_for_canonical[14] as usize,
    ];
    // Deepest ankle over the cycle; `None` if the stream cannot supply it.
    let floor = |frames: &[Vec<PartPose>]| -> Option<i16> {
        frames
            .iter()
            .filter_map(|f| feet.iter().filter_map(|&c| f.get(c)).map(|p| p.ty).max())
            .max()
    };
    let (Some(first), Some(rest)) = (
        rows.first().and_then(|f| f.get(torso)),
        host.first().and_then(|f| f.get(torso)),
    ) else {
        return [0; 3];
    };
    let dy = match (floor(rows), floor(host)) {
        (Some(a), Some(b)) => b - a,
        // No ankle channel to read: the torso's own height is the only
        // answer left, and it is the one this used to give unconditionally.
        _ => rest.ty - first.ty,
    };
    [rest.tx - first.tx, dy, rest.tz - first.tz]
}

/// Rebuild a character's battle idle from the mapped sibling's own idle.
///
/// This is a **stance** change more than a motion one. Retail authors each
/// character's combat stance in idle frame 0, and the siblings' stances
/// suit their own proportions - Che stands wider than Gala's staggered
/// guard - so a swapped character holding the host's stance reads as the
/// wrong body wearing the right model.
///
/// The stream is rewritten **in place at its exact retail length**: the
/// player idle is raw packed (`2 + frames * parts * 9`) inline in
/// `record0`, so keeping the frame and part counts keeps every offset in
/// the block valid and needs no relocation. The sibling's cycle is
/// resampled to the host's frame count, which is the whole cost of the
/// approach: the rate byte is already 1 on all three host idles and
/// cannot go lower, so a 35-frame sibling cycle resampled to 9 frames
/// breathes faster. Stance is worth more than tempo here.
///
/// The stream is then re-anchored onto the host's own rest by ONE RIGID
/// whole-body translation - see [`idle_anchor`] for what that means and
/// why a per-part one would tear the model apart.
pub fn rebuild_idle_stream(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    source_id: u16,
    natural_wrist_hand: Option<usize>,
) -> Result<RebuiltIdle> {
    let block = battle_char_assembly::decode_record0(player_file)?;
    let entry_off = u32::from_le_bytes(
        block
            .get(0..4)
            .ok_or_else(|| anyhow::anyhow!("record0 too short for the action table"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let head = entry_off + battle_char_assembly::PLAYER_ANIM_STREAM_OFFSET;
    let (parts, frames) = match block.get(head..head + 2) {
        Some([p, f]) if *p > 0 && *f > 0 => (*p as usize, *f as usize),
        _ => bail!("player idle stream head is empty"),
    };
    let body = head + 2;
    let want = parts * frames * 9;
    if body + want > block.len() {
        bail!("player idle stream runs past record0");
    }

    let src = monster_archive::idle_animation(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id} has no idle"))?;
    let mut rows = retarget_clip_wrist(
        &src,
        rig,
        player_file,
        archive_entry,
        source_id,
        parts,
        frames,
        natural_wrist_hand,
    )?;

    let host_idle = battle_char_assembly::idle_battle_animation(player_file)?
        .ok_or_else(|| anyhow::anyhow!("player file has no idle"))?;
    let [dx, dy, dz] = idle_anchor(&rows, &host_idle.frames, rig);
    // Clamped, not wrapped: `pack_part` keeps 12 bits and the decoder
    // sign-extends them, so a coordinate pushed past the range would come
    // back out the far side and teleport that part. Retail idles peak
    // around 450 of the 2047 the field holds, and the anchor is tens of
    // units, so the clamp is a guard rail rather than a working limit -
    // and a clamped part is the only thing here that is not rigid.
    let put = |v: i16, d: i16| (v as i32 + d as i32).clamp(-2048, 2047) as i16;
    for row in &mut rows {
        for t in row.iter_mut() {
            t.tx = put(t.tx, dx);
            t.ty = put(t.ty, dy);
            t.tz = put(t.tz, dz);
        }
    }

    let mut bytes = Vec::with_capacity(want);
    for row in &rows {
        for p in row {
            bytes.extend_from_slice(&pack_part(p));
        }
    }
    if bytes.len() != want {
        bail!(
            "rebuilt idle is {} bytes against the retail {want}",
            bytes.len()
        );
    }
    let host_rate = block
        .get(entry_off + monster_archive::ANIM_RATE_OFFSET)
        .copied()
        .unwrap_or(1)
        .max(1) as f32;
    let src_rate = src.rate.max(1) as f32;
    Ok(RebuiltIdle {
        offset: body,
        bytes,
        frames,
        // Duration is `frames / rate`; the multiplier is the ratio the
        // other way round, so a shorter stream reads as faster.
        pace: (src.frame_count as f32 / src_rate) / (frames as f32 / host_rate),
    })
}
