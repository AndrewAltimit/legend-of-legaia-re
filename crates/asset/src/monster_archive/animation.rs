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
    /// Attach-key / clip-identity byte (entry `+0x77`, the byte before the
    /// rate). Two retail consumers read it off the committed record:
    /// the equipment attach-object scan (`FUN_80052FA0` matches it against
    /// attach-record `+0x07` - see
    /// [`crate::battle_char_assembly::ArtAnimRecord::attach_key`]) and the
    /// per-character weapon-trail trigger (`FUN_8005112C` compares
    /// `record[+0x77]` against a per-character constant to fire the swept
    /// `POLY_G4` trail). `0` when the source stream carries no entry header.
    pub attach_key: u8,
    /// Number of animated objects per frame (one per TMD object).
    pub part_count: usize,
    /// Number of keyframes.
    pub frame_count: usize,
    /// `frame_count` frames, each `part_count` [`PartPose`]s (`frames[f][p]`).
    pub frames: Vec<Vec<PartPose>>,
    /// The per-action entry's head bytes (`+0x00..+0x54`,
    /// [`EFFECT_SCRIPT_HEAD_BYTES`]), whose `+0x14..+0x53` region is the
    /// action's **battle effect script**: up to eight 8-byte
    /// `[frame_gate, effect_id, i16 x, i16 y, i16 z]` records the battle
    /// anim-node tick walks each frame to place the action's visual effects
    /// (`FUN_801DEA50`, reached from `FUN_80047430`; engine stepper
    /// `legaia_engine_core::action_effect_script::step_effect_script`, which
    /// reads records at `RECORD_BASE = 0x14` of exactly this slice). Empty
    /// when the source carries no entry header (field ANM adapters, synthetic
    /// clips) - the stepper then finds no records and does nothing.
    pub effect_script: Vec<u8>,
}

impl MonsterAnimation {
    /// The poses for frame `f` (one per part), or `None` if out of range.
    pub fn frame(&self, f: usize) -> Option<&[PartPose]> {
        self.frames.get(f).map(|v| v.as_slice())
    }
}

/// Offset of the packed animation stream inside a per-action entry.
const ANIM_STREAM_OFFSET: usize = 0x8c;
/// Bytes of a per-action entry's head that carry the battle effect script:
/// the eight 8-byte records live at `+0x14..+0x53`, so `0x54` bytes cover the
/// whole scriptable region (record base + 8 records; `FUN_801DEA50` scales
/// its cursor by `8` and adds `0x14`, cursor bound `8`). Shared by the
/// monster archive's entries and the player battle files' record[0] entries
/// (whose keyframe stream sits at `+0xAC` instead of `+0x8C`, but whose head
/// layout below `+0x8C` is the same family).
pub const EFFECT_SCRIPT_HEAD_BYTES: usize = 0x54;

/// Slice a per-action entry's effect-script head (`entry+0x00..+0x54`) out of
/// its containing block, clamped to the block end. An entry cut short by the
/// block boundary yields the truncated prefix (the engine stepper bounds every
/// record read itself); an out-of-range offset yields an empty vec.
pub(crate) fn effect_script_head(block: &[u8], entry_off: usize) -> Vec<u8> {
    let end = entry_off
        .saturating_add(EFFECT_SCRIPT_HEAD_BYTES)
        .min(block.len());
    block.get(entry_off..end).unwrap_or_default().to_vec()
}
/// Offset of the playback-rate byte inside a per-action entry (shared with
/// the player battle files' record[0] entries).
pub(crate) const ANIM_RATE_OFFSET: usize = 0x78;
/// Offset of the attach-key / clip-identity byte inside a per-action entry
/// (see [`MonsterAnimation::attach_key`]).
pub(crate) const ATTACH_KEY_OFFSET: usize = 0x77;
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
    let attach_key = block
        .get(entry_off + ATTACH_KEY_OFFSET)
        .copied()
        .unwrap_or(0);
    parse_animation_stream(
        block,
        action_id,
        rate,
        attach_key,
        entry_off + ANIM_STREAM_OFFSET,
        effect_script_head(block, entry_off),
    )
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
    attach_key: u8,
    s: usize,
    effect_script: Vec<u8>,
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
        attach_key,
        part_count,
        frame_count,
        frames,
        effect_script,
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

/// One per-action entry's authored **loop window**: `(loop_count,
/// start_frame, end_frame)` from entry `+0x84..+0x87`.
///
/// The shared anim tick (`FUN_80047430`) tests the window BEFORE its
/// natural-end test: while the hold counter at actor `+0x176` (seeded
/// `loop_count << 4` at install, `FUN_8004AD80`) is non-zero, a cursor
/// reaching `end` wraps back to `start` (or parks there when
/// `start == end`), so a windowed clip never hits the natural-end
/// re-commit that otherwise replays a still-staged clip from frame 0.
/// The retail Delilas cast clips lean on this: Gi's crouch holds
/// `[9, 10]`, his finale parks on its last frame, Lu's charge parks on
/// hers, and Lu's flourish `[16, 24]` loops its tail sway. `end` may
/// equal the frame count (the whole-tail loop form).
pub type ActionLoopWindow = (u8, usize, usize);

/// The loop window of every action entry, **index-aligned with
/// [`animations`]** (same walk, same skip rules - pairing the two by
/// index is sound by construction). Entries whose window bytes are
/// zero / degenerate yield `None`. Returns `Ok(None)` for an empty /
/// filler / non-mesh slot.
pub fn animation_loop_windows(
    entry: &[u8],
    id: u16,
) -> Result<Option<Vec<Option<ActionLoopWindow>>>> {
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
        // Mirror `animations`: only entries it keeps take an index.
        let Some(anim) = parse_animation(&block, action_id, entry_off) else {
            continue;
        };
        let w = block
            .get(entry_off + 0x84..entry_off + 0x87)
            .map(|b| (b[0], b[1] as usize, b[2] as usize));
        out.push(w.filter(|&(cnt, s, e)| {
            cnt != 0 && e >= s && s < anim.frame_count && e <= anim.frame_count
        }));
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
/// Provenance note: the first-match-wins shape and the `0xFF` sentinel below
/// were read off the executable directly - `SCUS_942.54` text VA `0x80010000`,
/// so file offset `0x800 + (0x80050e2c - 0x80010000)` - because the dump then
/// carried decompiled C over an empty disassembly section. The dump has since
/// been re-extracted and now disassembles the whole 72-byte body, so it
/// corroborates the transcription below rather than being unusable:
///
/// ```text
/// 80050e2c  andi a2,a2,0xff      ; count
/// 80050e30  beqz a2,0x80050e64   ; empty table -> 0xFF
/// 80050e3c  lw   v0,(a0)         ; record pointer
/// 80050e44  lbu  v0,(v0)         ; tag byte = record[0]
/// 80050e4c  beq  v0,a1,0x80050e6c ; match -> return index
/// 80050e5c  bnez v0,0x80050e3c
/// 80050e68  addiu v0,zero,0xff   ; not found
/// ```
///
/// NOT WIRED: no engine caller - see [`reaction_map`].
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
/// `+0x1EF..+0x1F3` (`{2, 3, 4, 5, 0x0B}` = light flinch, second flinch,
/// knockdown, get-up, block).
///
/// Retail makes **one** pass over the `+0x4C` action-record array, testing all
/// five tags per entry, and every match stores unconditionally:
///
/// ```text
/// 80055338  loop:  a0 = &record[i]
/// 80055348         lbu v1,0x0(v0)     ; entry tag
/// 80055350         bne v1,2, +0x14
/// 80055360         sb  a1,0x1ef(v0)   ; <- store, NO break
/// 80055374         bne v1,3, +0x14
/// 80055384         sb  a1,0x1f0(v0)
/// ...                                  ; tags 4, 5, 0x0B likewise
/// 80055404         bne v0,zero,0x80055338
/// ```
///
/// There is no `break` and no "already set" test, so when a monster carries
/// two entries with the same tag the map keeps the **last** one, not the
/// first. This is the opposite of the entry search
/// [`find_action_by_tag`] (`FUN_80050E2C`) performs, which returns on its
/// first match - the two routines are not the same mechanism and must not be
/// built out of each other.
///
/// The sentinel is likewise different. `FUN_80050E2C` returns `0xFF` when
/// nothing matches; this loop pre-initialises nothing at all (the actor block
/// arrives zeroed) and the knockdown fallback tests against **zero**:
///
/// ```text
/// 80055428  lbu v0,0x1f1(v1)
/// 80055430  bne v0,zero, done
/// 80055438  lbu v0,0x1ef(v1)
/// 80055440  sb  v0,0x1f1(v1)     ; knockdown <- light flinch
/// ```
///
/// So a monster with no knockdown entry reuses its light-flinch animation and
/// the damage primitive always has a heavy-hit reaction to queue - but note
/// the quirk that falls out of the zero sentinel: a monster whose knockdown
/// entry really is at **index 0** is indistinguishable from "absent" and gets
/// the fallback applied over a perfectly good entry. Entry 0 is the idle loop
/// for every monster in the archive, so this never fires on retail data; it is
/// modelled because the mechanism, not the data, is what the port owes.
///
/// Party actors take the sibling path (`FUN_80053CB8`), which hardcodes
/// `[2, 3, 4, 5, 0xB]` because the player battle files store this family
/// identity-ordered - index equals tag. That is a property of those files, not
/// a general rule, and it does **not** hold for monster archives.
///
/// `None` is this port's spelling of retail's zero: "no entry claimed this
/// slot".
///
/// NOT WIRED: no engine caller. Nothing in `engine-core` / `engine-vm` queues a
/// monster hit reaction from this map yet; its only consumers are the unit tests
/// and the disc-gated archive oracles in `crates/asset/tests`.
///
/// PORT: FUN_80054cb0 (the `+0x1EF..+0x1F3` tag-map half; the stat-block copy
/// and battle-load stat boost live in `engine-vm::battle_formulas`)
pub fn reaction_map(tags: &[u8]) -> [Option<u8>; 5] {
    let mut map: [Option<u8>; 5] = [None; 5];
    // Single forward pass, last write wins - retail's loop has no `break`.
    for (i, &tag) in tags.iter().enumerate() {
        let Ok(i) = u8::try_from(i) else { break };
        for (slot, &want) in REACTION_TAGS.iter().enumerate() {
            if tag == want {
                map[slot] = Some(i);
            }
        }
    }
    // Knockdown (tag 4) falls back to light flinch (tag 2). Retail tests the
    // stored byte against zero, so an index-0 knockdown also takes the
    // fallback - see the doc comment.
    if map[2].is_none_or(|i| i == 0) {
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

/// One per-action entry's authored **sound-cue track**: up to eight
/// `(frame, cue_id)` pairs from entry `+0x54..+0x74`, in track order,
/// truncated at the first zero-cue slot (the retail walk
/// `FUN_800508DC` stops there, so trailing pairs are unreachable).
///
/// The shared per-frame cue player (`FUN_800508DC`, called per actor
/// with the LIVE entry pointer) walks this table against the clip
/// cursor - a pair fires once when the cursor first reaches `frame` -
/// and hands each cue to the ring producer `FUN_8004FE5C`. This is the
/// carrier of every clip-synchronised battle sound that is not an
/// effect-script cue: footsteps, swing whooshes, and the per-punch
/// impact volleys of the Delilas cast flurries (monster 164 entries
/// 12/13 fire `0x4A` + `0x172` pairs at each punch frame - the ring
/// receives them as one static-bank and one runtime-bank id).
pub type ActionCueTrack = Vec<(u16, u16)>;

/// The sound-cue track of every action entry, **index-aligned with
/// [`animations`]** (same walk, same skip rules). An entry whose track
/// opens with a zero cue yields an empty vec. Returns `Ok(None)` for an
/// empty / filler / non-mesh slot.
pub fn animation_cue_tracks(entry: &[u8], id: u16) -> Result<Option<Vec<ActionCueTrack>>> {
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
        // Mirror `animations`: only entries it keeps take an index.
        if parse_animation(&block, action_id, entry_off).is_none() {
            continue;
        }
        let mut track = Vec::new();
        for k in 0..8 {
            let base = entry_off + 0x54 + k * 4;
            let (Some(frame), Some(cue)) = (
                legaia_bytes::u16_le(&block, base),
                legaia_bytes::u16_le(&block, base + 2),
            ) else {
                break;
            };
            if cue == 0 {
                break;
            }
            track.push((frame, cue));
        }
        out.push(track);
    }
    Ok(Some(out))
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
    fn parse_animation_captures_the_entry_effect_script_head() {
        // The entry head +0x00..+0x54 rides along on the decoded animation:
        // its +0x14..+0x53 region is the battle effect-script record block
        // (FUN_801DEA50 reads records at cursor*8 + 0x14).
        let mut block = vec![0u8; 0x8c + 2 + ANIM_PART_STRIDE + 4];
        block[0x8c] = 1; // part_count
        block[0x8c + 1] = 1; // frame_count
        // One effect record at +0x14: frame gate 5, effect 0x86, x=100.
        block[0x14] = 5;
        block[0x15] = 0x86;
        block[0x16..0x18].copy_from_slice(&100i16.to_le_bytes());
        let anim = parse_animation(&block, 0x0C, 0).expect("animation parses");
        assert_eq!(anim.effect_script.len(), EFFECT_SCRIPT_HEAD_BYTES);
        assert_eq!(anim.effect_script[0x14], 5);
        assert_eq!(anim.effect_script[0x15], 0x86);
        assert_eq!(
            i16::from_le_bytes([anim.effect_script[0x16], anim.effect_script[0x17]]),
            100
        );
        // An entry cut short by the block end keeps the truncated prefix.
        let head = effect_script_head(&block, block.len() - 8);
        assert_eq!(head.len(), 8);
        // An out-of-range offset yields an empty head.
        assert!(effect_script_head(&block, block.len() + 1).is_empty());
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
        // A present tag 4 at a non-zero index is never overwritten.
        assert_eq!(reaction_map(&[2, 4])[2], Some(1));
    }

    /// `FUN_80054CB0`'s tag loop stores without a `break`, so a duplicated tag
    /// resolves to the **last** matching entry. This is the one behaviour that
    /// separates the port from a `position()`-style first-match scan, and it is
    /// not observable on retail archives (no shipped monster duplicates a
    /// reaction tag), so only a synthetic table can pin it.
    #[test]
    fn reaction_map_takes_the_last_entry_for_a_duplicated_tag() {
        // Tag 2 at indices 1 and 4; tag 0x0B at indices 2 and 5.
        let tags = [0u8, 2, 0x0B, 3, 2, 0x0B];
        let map = reaction_map(&tags);
        assert_eq!(map[0], Some(4), "light flinch keeps the last tag-2 entry");
        assert_eq!(map[4], Some(5), "block keeps the last tag-0x0B entry");
        // A first-wins scan would answer Some(1) / Some(2) here.
        assert_ne!(map[0], Some(1));
        assert_ne!(map[4], Some(2));
        // The single-match slots are unaffected.
        assert_eq!(map[1], Some(3));
    }

    /// Retail's knockdown fallback tests the stored byte against **zero**, not
    /// against a `0xFF` sentinel, so a tag-4 entry sitting at index 0 reads as
    /// "absent" and gets overwritten by the light flinch.
    #[test]
    fn reaction_map_zero_sentinel_swallows_an_index_zero_knockdown() {
        let tags = [4u8, 2];
        let map = reaction_map(&tags);
        assert_eq!(map[2], Some(1), "index-0 knockdown loses to the fallback");
        // Sanity: it really did find the tag-4 entry first.
        assert_eq!(find_action_by_tag(&tags, 4), Some(0));
    }

    #[test]
    fn reaction_tags_are_the_documented_family() {
        assert_eq!(REACTION_TAGS, [2, 3, 4, 5, 0x0B]);
        assert_eq!(NO_ACTION_ENTRY, 0xFF);
    }
}
