//! Finding and composing the victory loop: the clip, its loop windows, the
//! cycle picked from them, and the composed base stream.

use super::super::*;

/// The mapped sibling's victory clip: the LAST tag-`0x22` entry, else
/// the last tag-`0x23` flourish (Che ships no `0x22`), else the idle.
pub fn victory_clip(
    archive_entry: &[u8],
    source_id: u16,
) -> Result<legaia_actor_formats::monster_archive::MonsterAnimation> {
    let tags = monster_archive::action_tags(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let anims = monster_archive::animations(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no animations"))?;
    for want in [0x22u8, 0x23, 0x00] {
        if let Some(a) = tags
            .iter()
            .rposition(|&t| t == want)
            .and_then(|i| anims.get(i))
            && a.part_count == CANONICAL_PARTS
            && !a.frames.is_empty()
        {
            return Ok(a.clone());
        }
    }
    bail!("monster id {source_id}: no usable victory clip")
}

/// One base "ME" entry's retail loop window, read off the character's own
/// art bank: `count` is entry `+0x84` (the hold-counter seed - `0` means
/// the stream never loops), `start`/`end` are `+0x85`/`+0x86`.
///
/// `end` may equal the stream's frame count: the tick's window test runs
/// **before** its natural-end test (`0x80047768` vs `0x80047a48`), so a
/// window ending one past the last frame simply loops the whole tail and
/// the clip never reaches its commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopWindow {
    /// Entry `+0x84`, the `actor +0x176` seed (`0xFF` on every retail
    /// base record). Also the byte [`battle_char_assembly::ArtAnimRecord::uses_base_archive`]
    /// keys on, which is why the rebuild never rewrites it.
    pub count: u8,
    /// Entry `+0x85` - the frame the wrap jumps back to.
    pub start: usize,
    /// Entry `+0x86` - the frame the wrap fires on.
    pub end: usize,
}

impl LoopWindow {
    /// Whether this window actually replays frames of a stream `frames`
    /// long (retail: true on all 24 base records).
    pub fn loops(&self, frames: usize) -> bool {
        self.count != 0 && self.end > self.start && self.start < frames
    }
}

/// The loop window of every base "ME" entry of a character, keyed by the
/// entry index (= the art record's `stream_source`, a bijection over
/// `0..=7` in all three retail player files).
///
/// The window bytes live in the player file's own art bank, not in the
/// readef archive, so the rebuild has to go back to `record[0]` for them.
pub fn base_loop_windows(player_file: &[u8]) -> Result<BTreeMap<usize, LoopWindow>> {
    let rec0 = battle_char_assembly::decode_record0(player_file)
        .context("decode record[0] for the base loop windows")?;
    let bank = battle_char_assembly::art_animation_bank(&rec0).context("art bank")?;
    let mut out = BTreeMap::new();
    for r in bank.iter().filter(|r| r.uses_base_archive()) {
        let e = r.entry_offset;
        let Some(w) = rec0.get(e + 0x84..e + 0x87) else {
            continue;
        };
        out.insert(
            r.stream_source as usize,
            LoopWindow {
                count: w[0],
                start: w[1] as usize,
                end: w[2] as usize,
            },
        );
    }
    Ok(out)
}

/// The frames the sibling's OWN victory entry replays after it wins,
/// as a half-open range into `clip.frames`.
///
/// Retail declares one on two of the three siblings: Lu replays her
/// `[16, 24]` celebration sway, Gi declares the single-frame hold
/// `[30, 30]` - a freeze on his last pose - and Che's stand-in `0x23`
/// flourish declares none (her other flourishes hold at their last
/// frame). A one-frame range is therefore a legitimate answer, not a
/// degenerate one: it is exactly what the retail heroes' own win-pose
/// windows look like, which carry 0.07-0.64 degrees of drift per frame.
/// `None` = the sibling declares nothing and the caller holds the
/// flourish's own last frame.
///
/// The entry is matched by tag **and** stream shape rather than by index:
/// [`victory_clip`] indexes `animations()` with a position from
/// `action_tags()`, and `animations()` drops entries whose stream fails to
/// parse, so the two index spaces are not guaranteed to agree.
pub fn victory_cycle(
    archive_entry: &[u8],
    source_id: u16,
    clip: &legaia_actor_formats::monster_archive::MonsterAnimation,
) -> Result<Option<std::ops::Range<usize>>> {
    let Some(block) = monster_archive::decode_block(archive_entry, source_id)? else {
        return Ok(None);
    };
    let Some(&magic_count) = block.get(0x4a) else {
        return Ok(None);
    };
    let mut found = None;
    for i in 0..magic_count as usize {
        let Some(off) = legaia_bytes::u32_le(&block, 0x4c + i * 4).map(|v| v as usize) else {
            break;
        };
        // `+0x8c` is the stream head (`[parts][frames]`); `+0x84..+0x87`
        // the loop fields.
        let Some(head) = block.get(off..off + 0x8e) else {
            continue;
        };
        if head[0] != clip.action_id
            || head[0x8c] as usize != clip.part_count
            || head[0x8d] as usize != clip.frame_count
        {
            continue;
        }
        found = Some(LoopWindow {
            count: head[0x84],
            start: head[0x85] as usize,
            end: head[0x86] as usize,
        });
    }
    let Some(w) = found else { return Ok(None) };
    // `start == 0` would leave no flourish at all, only the loop.
    if w.count == 0 || w.start == 0 || w.start >= clip.frame_count {
        return Ok(None);
    }
    // `end == start` is retail's single-frame hold arm (the tick snaps the
    // cursor back to `start` instead of subtracting a window length), so a
    // range of one frame is the faithful rendering of it.
    Ok(Some(w.start..w.end.clamp(w.start + 1, clip.frame_count)))
}

/// Pose distance between two frames, in model units: translations
/// directly, rotations as shortest-path angle converted to the arc a
/// nominal 100-unit limb sweeps. Used only to phase-align a cycle
/// against the pose the lead-in ends on.
pub(super) fn pose_dist(a: &[PartPose], b: &[PartPose]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut d = 0.0f64;
    for i in 0..n {
        d += (a[i].tx - b[i].tx).abs() as f64
            + (a[i].ty - b[i].ty).abs() as f64
            + (a[i].tz - b[i].tz).abs() as f64;
        for (x, y) in [(a[i].rx, b[i].rx), (a[i].ry, b[i].ry), (a[i].rz, b[i].rz)] {
            let mut t = (x as i32 - y as i32).rem_euclid(4096);
            if t > 2048 {
                t = 4096 - t;
            }
            d += t as f64 / 4096.0 * std::f64::consts::TAU * 100.0;
        }
    }
    d / n as f64
}

/// Compose one base entry's source frame sequence: `lead` resampled over
/// the lead-in `[0, w.start)`, then `cycle` mapped over the loop window
/// so the wrap is seamless.
///
/// The tick wraps by subtracting `(+0x86 - +0x85)` frames, so host frame
/// `w.start + j` carries cycle position `(k + j * cycle.len() / L) %
/// cycle.len()` with `L = +0x86 - +0x85`: at `j == L` - the frame the
/// wrap fires on - that is position `0`, which is exactly the pose at
/// `j == 0` that the cursor lands back on. The seam is therefore an
/// identity, not an approximation, whenever the window ends inside the
/// stream; where retail put `+0x86` one past the last frame the seam is
/// one authored step of `cycle`, which is a cycle and closes on itself.
///
/// `k` is the cycle phase whose pose sits closest to the one the lead-in
/// ends on, so the single handoff into the loop is as quiet as the cycle
/// allows (and is the natural `0` when `cycle` is the continuation of
/// `lead` in the same clip).
pub fn compose_base_stream(
    lead: &[Vec<PartPose>],
    cycle: &[Vec<PartPose>],
    frames: usize,
    w: LoopWindow,
) -> Vec<Vec<PartPose>> {
    let hw0 = w.start.min(frames);
    let span = w.end.saturating_sub(w.start).max(1);
    let mut out: Vec<Vec<PartPose>> = Vec::with_capacity(frames);
    for h in 0..hw0 {
        out.push(lead[(h * lead.len() / hw0).min(lead.len() - 1)].clone());
    }
    let k = match out.last() {
        Some(tail) => (0..cycle.len())
            .min_by(|&i, &j| pose_dist(tail, &cycle[i]).total_cmp(&pose_dist(tail, &cycle[j])))
            .unwrap_or(0),
        None => 0,
    };
    for h in hw0..frames {
        let j = h - w.start;
        out.push(cycle[(k + j * cycle.len() / span) % cycle.len()].clone());
    }
    out
}
