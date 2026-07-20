//! Animation (per-object transform keyframes).

use anyhow::Result;

use super::{MIN_RECORD_BYTES, decode_block};

/// One animated object's transform for a single keyframe.
///
/// The battle renderer treats each TMD object as a rigid part and poses it with
/// a translation + a Euler rotation. The decoder (`FUN_8004998c`) unpacks one
/// part's 9 bytes into six 12-bit fields: `[tx, ty, tz]` are **sign-extended**
/// (signed translation in TMD model units) and `[rx, ry, rz]` are **unsigned
/// 12-bit angles** (`4096` = a full turn). The renderer interpolates these
/// across frames (linear for translation, shortest-path angle-wrap for
/// rotation) and applies them per object via the GTE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartPose {
    /// Translation X (signed, TMD model units).
    pub tx: i16,
    /// Translation Y.
    pub ty: i16,
    /// Translation Z.
    pub tz: i16,
    /// Rotation about X (`0..4096` = `0..360°`).
    pub rx: u16,
    /// Rotation about Y.
    pub ry: u16,
    /// Rotation about Z.
    pub rz: u16,
}

/// One monster action's animation: `frame_count` keyframes, each holding a
/// [`PartPose`] for every animated object (`part_count`).
///
/// Sourced from a per-action entry's packed stream at entry `+0x8c` (the
/// entries the `+0x4C` offset array points at - see [`MonsterRecord::spells`]).
/// The stream head is `[u8 part_count][u8 frame_count]` followed by
/// `frame_count * part_count` nine-byte part records. `part_count` matches the
/// monster TMD's object count (one part per object). Action **index 0** is the
/// neutral **idle** animation the engine loops when the monster isn't acting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterAnimation {
    /// Action id (entry `+0x00`) - a semantic **type tag**, not just an index.
    /// `0` idle loop, `1` walk/approach, `2`/`3` light hit reactions, `4`
    /// knockdown (heavy hit / death fall), `5` get-up, `0x0B` block; monster
    /// archives additionally carry the attack family (`0x20` pre-approach,
    /// `0x21` close-in, `0x22` victory) and spell actions. The battle loaders
    /// cache a tag → entry-index map at actor `+0x1EF..+0x1F3` for tags
    /// `{2,3,4,5,0x0B}` (`FUN_80054CB0` scans the entry table; `FUN_80053CB8`
    /// hardcodes `[2,3,4,5,0xB]` for party files, whose layout is identity).
    pub action_id: u8,
    /// Playback-rate byte (entry `+0x78`). The retail per-frame cursor advance
    /// is `(frame_dt * actor[+0x21D] * rate) >> 1` in 12.4 fixed point
    /// (`FUN_80047430`), i.e. `rate / 8` keyframes per 60 Hz tick with the
    /// normal `actor[+0x21D] == 4`. Retail data uses `1` or `2`.
    pub rate: u8,
    /// Number of animated objects per frame (one per TMD object).
    pub part_count: usize,
    /// Number of keyframes.
    pub frame_count: usize,
    /// `frame_count` frames, each `part_count` [`PartPose`]s (`frames[f][p]`).
    pub frames: Vec<Vec<PartPose>>,
}

impl MonsterAnimation {
    /// The poses for frame `f` (one per part), or `None` if out of range.
    pub fn frame(&self, f: usize) -> Option<&[PartPose]> {
        self.frames.get(f).map(|v| v.as_slice())
    }
}

/// Offset of the packed animation stream inside a per-action entry.
const ANIM_STREAM_OFFSET: usize = 0x8c;
/// Offset of the playback-rate byte inside a per-action entry (shared with
/// the player battle files' record[0] entries).
pub(crate) const ANIM_RATE_OFFSET: usize = 0x78;
/// Bytes per part record in the packed stream (six 12-bit fields).
const ANIM_PART_STRIDE: usize = 9;

/// Sign-extend a 12-bit field to `i16`.
fn sx12(v: u16) -> i16 {
    if v & 0x800 != 0 {
        (v | 0xf000) as i16
    } else {
        v as i16
    }
}

/// Unpack one nine-byte part record into its six 12-bit fields, mirroring the
/// bit layout in `FUN_8004998c`: low bytes at `[0,1,3,4,6,7]`, the high nibbles
/// packed into `[2,5,8]`.
fn unpack_part(b: &[u8]) -> PartPose {
    let v0 = b[0] as u16 | ((b[2] as u16 & 0x0f) << 8);
    let v1 = b[1] as u16 | ((b[2] as u16 & 0xf0) << 4);
    let v2 = b[3] as u16 | ((b[5] as u16 & 0x0f) << 8);
    let v3 = b[4] as u16 | ((b[5] as u16 & 0xf0) << 4);
    let v4 = b[6] as u16 | ((b[8] as u16 & 0x0f) << 8);
    let v5 = b[7] as u16 | ((b[8] as u16 & 0xf0) << 4);
    PartPose {
        tx: sx12(v0),
        ty: sx12(v1),
        tz: sx12(v2),
        rx: v3 & 0xfff,
        ry: v4 & 0xfff,
        rz: v5 & 0xfff,
    }
}

/// Parse one per-action entry's packed animation stream at block offset
/// `entry_off`. Returns `None` when the stream head or frame data falls outside
/// the block, or the part/frame counts are zero.
fn parse_animation(block: &[u8], action_id: u8, entry_off: usize) -> Option<MonsterAnimation> {
    let rate = block
        .get(entry_off + ANIM_RATE_OFFSET)
        .copied()
        .unwrap_or(0);
    parse_animation_stream(block, action_id, rate, entry_off + ANIM_STREAM_OFFSET)
}

/// Parse a packed `[u8 parts][u8 frames][9-byte TRS records]` stream starting
/// at block offset `s`. Shared between the monster archive's per-action
/// entries (stream at entry `+0x8c`) and the player battle files' record[0]
/// action entries (stream at entry `+0xAC`;
/// see [`crate::battle_char_assembly::battle_animations`]).
pub(crate) fn parse_animation_stream(
    block: &[u8],
    action_id: u8,
    rate: u8,
    s: usize,
) -> Option<MonsterAnimation> {
    let part_count = *block.get(s)? as usize;
    let frame_count = *block.get(s + 1)? as usize;
    if part_count == 0 || frame_count == 0 {
        return None;
    }
    let data = s + 2;
    let need = frame_count * part_count * ANIM_PART_STRIDE;
    if data + need > block.len() {
        return None;
    }
    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let mut parts = Vec::with_capacity(part_count);
        for p in 0..part_count {
            let o = data + (f * part_count + p) * ANIM_PART_STRIDE;
            parts.push(unpack_part(&block[o..o + ANIM_PART_STRIDE]));
        }
        frames.push(parts);
    }
    Some(MonsterAnimation {
        action_id,
        rate,
        part_count,
        frame_count,
        frames,
    })
}

/// Decode every action animation for monster id `id`, in `+0x4C`-array order.
///
/// Each entry in the monster's action/spell table (`magic_count` of them)
/// carries a packed transform-keyframe stream at entry `+0x8c`; this returns
/// one [`MonsterAnimation`] per entry that decodes cleanly (a malformed or
/// empty stream is skipped, so the returned indices may be sparser than the
/// raw table). Returns `Ok(None)` for an empty / filler / non-mesh slot.
pub fn animations(entry: &[u8], id: u16) -> Result<Option<Vec<MonsterAnimation>>> {
    let Some(block) = decode_block(entry, id)? else {
        return Ok(None);
    };
    if block.len() < MIN_RECORD_BYTES {
        return Ok(None);
    }
    let magic_count = block[0x4a] as usize;
    let mut out = Vec::with_capacity(magic_count);
    for i in 0..magic_count {
        let Some(entry_off) = legaia_bytes::u32_le(&block, 0x4c + i * 4).map(|v| v as usize) else {
            break;
        };
        let Some(&action_id) = block.get(entry_off) else {
            continue;
        };
        if let Some(anim) = parse_animation(&block, action_id, entry_off) {
            out.push(anim);
        }
    }
    Ok(Some(out))
}

/// The raw action **tag** of every entry in the monster's `+0x4C` action-record
/// array, in table order.
///
/// This is the index space the battle engine actually addresses: the anim id at
/// actor `+0x1DA` is a raw entry index into this array, so a tag lookup must run
/// over *every* entry - including ones whose keyframe stream is empty or
/// malformed, which [`animations`] drops. Pairing the two would mis-map indices;
/// use this (not `animations`) whenever an index is going back into the engine.
///
/// Returns `Ok(None)` for an empty / filler / non-mesh slot.
pub fn action_tags(entry: &[u8], id: u16) -> Result<Option<Vec<u8>>> {
    let Some(block) = decode_block(entry, id)? else {
        return Ok(None);
    };
    if block.len() < MIN_RECORD_BYTES {
        return Ok(None);
    }
    let magic_count = block[0x4a] as usize;
    let mut out = Vec::with_capacity(magic_count);
    for i in 0..magic_count {
        let Some(entry_off) = legaia_bytes::u32_le(&block, 0x4c + i * 4).map(|v| v as usize) else {
            break;
        };
        // An entry pointer past the end of the block terminates the usable
        // table - retail would read garbage, we stop.
        let Some(&tag) = block.get(entry_off) else {
            break;
        };
        out.push(tag);
    }
    Ok(Some(out))
}

/// First-byte tag search over the action-record array.
///
/// Retail signature is `(table, tag, count) -> idx_or_0xFF`: a linear scan of a
/// pointer table that dereferences each entry, compares its **first byte**
/// against `tag`, and returns the entry *index* - or the sentinel `0xFF` when no
/// entry matches. This port returns `None` for the sentinel; callers that need
/// the raw retail byte can map `None` to `0xFF`.
///
/// The battle-action SM resolves a monster's attack animations through this with
/// tags `0x20` (pre-approach), `1` (walk), `0x21` (close-in) and `0x22`
/// (victory), staging the returned index into actor `+0x1DA`.
///
/// Note the first byte is a semantic **tag**, not the entry's own index - a
/// monster may carry several entries sharing a tag (the search takes the first)
/// and may omit a tag entirely.
///
/// PORT: FUN_80050e2c
pub fn find_action_by_tag(tags: &[u8], tag: u8) -> Option<u8> {
    tags.iter()
        .position(|&t| t == tag)
        // Retail's return is a byte, so an index that cannot be expressed as
        // one could never round-trip through actor `+0x1DA` anyway.
        .and_then(|i| u8::try_from(i).ok())
        .filter(|&i| i != NO_ACTION_ENTRY)
}

/// Retail's "no entry matched" sentinel returned by [`find_action_by_tag`]'s
/// source routine (`FUN_80050E2C`).
pub const NO_ACTION_ENTRY: u8 = 0xFF;

/// The five hit-reaction tags the battle loaders cache per actor, in
/// `+0x1EF..+0x1F3` order.
pub const REACTION_TAGS: [u8; 5] = [2, 3, 4, 5, 0x0B];

/// The hit-reaction tag → entry-index map the monster loader builds at actor
/// `+0x1EF..+0x1F3`.
///
/// Retail scans the `+0x4C` action-record array once per tag in
/// [`REACTION_TAGS`] (`{2, 3, 4, 5, 0x0B}` = light flinch, second flinch,
/// knockdown, get-up, block) and stores the matching entry index. The one
/// special case is **tag 4 falls back to tag 2**: a monster with no knockdown
/// entry reuses its light-flinch animation, so the damage primitive always has
/// a heavy-hit reaction to queue.
///
/// Party actors take the sibling path (`FUN_80053CB8`), which hardcodes
/// `[2, 3, 4, 5, 0xB]` because the player battle files store this family
/// identity-ordered - index equals tag. That is a property of those files, not
/// a general rule, and it does **not** hold for monster archives.
///
/// Entries are `None` where retail leaves the sentinel `0xFF`.
///
/// PORT: FUN_80054cb0 (the `+0x1EF..+0x1F3` tag-map half; the stat-block copy
/// and battle-load stat boost live in `engine-vm::battle_formulas`)
pub fn reaction_map(tags: &[u8]) -> [Option<u8>; 5] {
    let mut map = REACTION_TAGS.map(|tag| find_action_by_tag(tags, tag));
    // Tag 4 (knockdown) falls back to tag 2 (light flinch) when absent.
    if map[2].is_none() {
        map[2] = map[0];
    }
    map
}

/// Short, display-ready labels for a monster's decoded animations, parallel to
/// the slice returned by [`animations`]. Index 0 is the idle loop; `action_id`
/// `1` is the locomotion cycle the battle engine plays while the monster
/// advances on a target (a walk for grounded enemies, a flight cycle for
/// fliers - hence "Move", not "Attack"). The named tags follow the action-tag
/// space ([`monster-animation.md` § Action
/// tags](../../../docs/formats/monster-animation.md)): `2` the light hit
/// reaction, `4` knockdown, `5` get-up, `0x0B` block, and `0x0D..0x0F` the
/// monster's attack actions (each a distinct move - the `#N` suffix keeps
/// them apart). Everything else stays `Action 0xNN`. When two entries would
/// share a label (several actions with the same `action_id`, or the multiple
/// attack tags), a ` #N` suffix disambiguates so every label is unique -
/// handy for toggle buttons and glTF animation names.
pub fn action_labels(anims: &[MonsterAnimation]) -> Vec<String> {
    use std::collections::HashMap;
    let base: Vec<String> = anims
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if i == 0 {
                return "Idle".to_string();
            }
            match a.action_id {
                1 => "Move".to_string(),
                2 => "Damaged".to_string(),
                4 => "Knocked Down".to_string(),
                5 => "Getting Up".to_string(),
                0x0B => "Block".to_string(),
                0x0D..=0x0F => "Attack".to_string(),
                id => format!("Action 0x{id:02X}"),
            }
        })
        .collect();
    let mut totals: HashMap<String, usize> = HashMap::new();
    for b in &base {
        *totals.entry(b.clone()).or_default() += 1;
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    base.into_iter()
        .map(|b| {
            if totals.get(&b).copied().unwrap_or(0) > 1 {
                let n = seen.entry(b.clone()).or_default();
                *n += 1;
                format!("{b} #{n}")
            } else {
                b
            }
        })
        .collect()
}

/// Decode just the **idle** animation (action index 0) for monster id `id`.
///
/// This is the neutral pose loop the battle engine plays when the monster is
/// not performing a move. Returns `Ok(None)` if the slot is empty or carries no
/// decodable action animations.
pub fn idle_animation(entry: &[u8], id: u16) -> Result<Option<MonsterAnimation>> {
    Ok(animations(entry, id)?.and_then(|mut a| {
        if a.is_empty() {
            None
        } else {
            Some(a.swap_remove(0))
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_part_splits_six_12bit_fields() {
        // Low bytes at [0,1,3,4,6,7]; high nibbles packed into [2,5,8].
        // Field i = low | (nibble << 8). tx/ty/tz sign-extend; rx/ry/rz don't.
        // b2=0x81 -> v0 high nibble 0x1 (0x081=129), v1 high nibble 0x8 (0x812).
        let b = [0x80, 0x12, 0x81, 0x34, 0x56, 0x07, 0x9a, 0xbc, 0x21];
        let p = unpack_part(&b);
        assert_eq!(p.tx, 0x180); // 0x80 | (0x1<<8) = 0x180, positive
        assert_eq!(p.ty, sx12(0x812)); // 0x12 | (0x8<<8) = 0x812, sign bit set -> negative
        assert_eq!(p.tz, 0x734); // 0x34 | (0x7<<8)
        assert_eq!(p.rx, 0x056); // 0x56 | (0x0<<8) (rotation, unsigned)
        assert_eq!(p.ry, 0x19a); // 0x9a | (0x1<<8)
        assert_eq!(p.rz, 0x2bc); // 0xbc | (0x2<<8)
    }

    #[test]
    fn parse_animation_reads_part_and_frame_counts() {
        // Build an entry whose +0x8c stream is [parts=2][frames=3][3*2 parts].
        let mut block = vec![0u8; 0x8c + 2 + 3 * 2 * ANIM_PART_STRIDE + 4];
        let s = 0x8c;
        block[s] = 2; // part_count
        block[s + 1] = 3; // frame_count
        // Frame 1, part 0: tx low byte 0x10 at its slot (frame 1 * 2 parts).
        let f1p0 = s + 2 + 2 * ANIM_PART_STRIDE;
        block[f1p0] = 0x10;
        let anim = parse_animation(&block, 0x00, 0).expect("animation parses");
        assert_eq!(anim.action_id, 0x00);
        assert_eq!(anim.part_count, 2);
        assert_eq!(anim.frame_count, 3);
        assert_eq!(anim.frames.len(), 3);
        assert_eq!(anim.frames[0].len(), 2);
        assert_eq!(anim.frame(1).unwrap()[0].tx, 0x10);
        // Out-of-range / zero-count streams yield None.
        assert!(parse_animation(&[0u8; 0x8c + 2], 0, 0).is_none());
    }

    #[test]
    fn find_action_by_tag_returns_first_match_or_none() {
        // The tag is the entry's first byte, not its index - here the idle
        // entry (tag 0) sits at index 0 but tag 4 is at index 3.
        let tags = [0u8, 1, 2, 4, 5, 0x0B];
        assert_eq!(find_action_by_tag(&tags, 0), Some(0));
        assert_eq!(find_action_by_tag(&tags, 4), Some(3));
        assert_eq!(find_action_by_tag(&tags, 0x0B), Some(5));
        // Absent tag -> retail's 0xFF sentinel, surfaced as None.
        assert_eq!(find_action_by_tag(&tags, 0x20), None);
        // Duplicate tags: the first wins.
        assert_eq!(find_action_by_tag(&[7, 7, 7], 7), Some(0));
        // Empty table returns the sentinel immediately.
        assert_eq!(find_action_by_tag(&[], 0), None);
    }

    #[test]
    fn find_action_by_tag_cannot_return_the_sentinel_index() {
        // Retail truncates the index to a byte, so index 0xFF is
        // indistinguishable from "not found" - we report None rather than
        // handing back an index the engine would read as the sentinel.
        let mut tags = vec![0u8; 300];
        tags[0xFF] = 0x42;
        assert_eq!(find_action_by_tag(&tags, 0x42), None);
        // One slot earlier is representable and comes back normally.
        let mut tags = vec![0u8; 300];
        tags[0xFE] = 0x42;
        assert_eq!(find_action_by_tag(&tags, 0x42), Some(0xFE));
    }

    #[test]
    fn reaction_map_caches_the_five_tags() {
        // A full family: every reaction tag present at a distinct index.
        let tags = [0u8, 1, 2, 3, 4, 5, 0x0B];
        assert_eq!(
            reaction_map(&tags),
            [Some(2), Some(3), Some(4), Some(5), Some(6)]
        );
    }

    #[test]
    fn reaction_map_falls_back_knockdown_to_light_flinch() {
        // No tag-4 entry: the knockdown slot reuses the tag-2 light flinch so
        // the damage primitive always has a heavy-hit reaction to queue.
        let tags = [0u8, 1, 2, 3, 5, 0x0B];
        let map = reaction_map(&tags);
        assert_eq!(map[2], map[0], "tag 4 falls back to tag 2");
        assert_eq!(map[2], Some(2));
        // With neither tag present the slot stays empty rather than
        // fabricating an index.
        assert_eq!(reaction_map(&[0, 1])[2], None);
        // The fallback is one-way: a present tag 4 is never overwritten.
        assert_eq!(reaction_map(&[2, 4])[2], Some(1));
    }

    #[test]
    fn reaction_tags_are_the_documented_family() {
        assert_eq!(REACTION_TAGS, [2, 3, 4, 5, 0x0B]);
        assert_eq!(NO_ACTION_ENTRY, 0xFF);
    }
}
