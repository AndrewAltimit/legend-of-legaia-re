//! Weapon-swing animations (equipment-section records -> runtime slots 0xC..0xF).

use anyhow::{Context, Result, bail};

use crate::battle_data_pack::{BattleDataPack, decode_record};

use super::animation::PLAYER_ANIM_STREAM_OFFSET;
use super::assembly::select_sections;
use super::{SECTION_COUNT, read_u32};

/// First runtime action slot filled from the equipment sections: the four
/// direction-command swings live at slots `0xC` (L) / `0xD` (R) / `0xE` (D) /
/// `0xF` (U) - the same byte values the Tactical-Arts command queue stages
/// as anim ids.
pub const SWING_SLOT_BASE: u8 = 0xC;

/// Byte offset of the per-command **AP cost** inside a swing action entry
/// (`record + 0x74`). Copied verbatim into the runtime command-cost record
/// (`DAT_801C9360[char][cmd] + 0x74`) at battle load (`FUN_800557B8`); read
/// as the Arts-gauge arm width and as the Muscle Dome card cost
/// (`FUN_801d388c` case 9). Retail value set per weapon class: favored
/// `0x1E`, off-class `0x2A`, far `0x36`.
pub const SWING_COST_OFFSET: usize = 0x74;

/// One weapon-swing animation spliced from an equipment section's payload
/// into the runtime action table (see [`swing_battle_animations`]).
#[derive(Debug, Clone)]
pub struct SwingAnimation {
    /// Runtime action-table slot (`0xC..=0xF`).
    pub slot: u8,
    /// Equipment section the record came from (`2..=4`; sections 0/1 carry
    /// no swing records - their `+0x04`/`+0x08` words are zero on disc).
    pub section: usize,
    /// Descriptor id of the section slot (equippable item id; `0` =
    /// section default).
    pub item_id: u32,
    /// The record's first byte - a presentation-class tag in the same id
    /// space as the art entries' (`0x0E..0x1F` observed), **not** the slot.
    pub entry_tag: u8,
    /// `+0x74` - the command's AP cost (the Arts-gauge arm width for slot
    /// `0xC`; the Muscle Dome card cost for every slot). See
    /// [`SWING_COST_OFFSET`].
    pub cost: u8,
    /// The decoded keyframe animation. `action_id` is the runtime slot.
    pub anim: crate::monster_archive::MonsterAnimation,
    /// The entry's facial keyframe tracks (`+0x8C` eyes / `+0x98` mouth),
    /// consumed by the per-frame facial animator while the swing plays
    /// (see [`crate::face_anim`]). `None` only for a truncated header.
    pub face: Option<crate::face_anim::FaceTracks>,
}

/// Parse the standard `0xAC`-byte action entry at `off` in `block`: action
/// tag at `+0x00`, rate byte at `+0x78`
/// ([`crate::monster_archive::ANIM_RATE_OFFSET`]), packed keyframe stream at
/// `+0xAC` ([`PLAYER_ANIM_STREAM_OFFSET`]).
// PORT: FUN_800557b8 - the record copy that pins this shape: 0x2B words
// (= 0xAC bytes) of header, then `(parts * frames * 9 + 5) >> 2` words of
// the packed stream read from the bytes at +0xAC.
fn parse_action_entry(
    block: &[u8],
    off: usize,
    action_id: u8,
) -> Option<crate::monster_archive::MonsterAnimation> {
    let rate = block
        .get(off + crate::monster_archive::ANIM_RATE_OFFSET)
        .copied()?;
    crate::monster_archive::parse_animation_stream(
        block,
        action_id,
        rate,
        off + PLAYER_ANIM_STREAM_OFFSET,
    )
}

/// Decode the **weapon-swing animations** the battle loader splices into the
/// runtime action table from the equipped-item sections: per selected
/// section 2/3/4, the decoded payload's `+0x04` word is a self-relative
/// offset to a standard action-entry record (header + keyframe stream at
/// `+0xAC`), installed at slot `0xC + (section - 2)`; section 4's `+0x08`
/// word carries a **second** record, installed at slot `0xF`. Sections 0/1
/// contribute none (their words are zero on disc).
///
/// `equipped` is the char record's `+0x196..+0x19A` bytes, as for
/// [`assemble_character`]; the returned animations' `action_id` is the
/// runtime slot (`0xC..=0xF`).
// PORT: FUN_80052FA0 (swing-splice half) - the `if (1 < iVar3)` section
// loop: copies the section-base + `+0x04` record via FUN_800557b8 into the
// action-table word at 0x28 + section*4 (= slot 0xC..0xE for sections
// 2..4), and section 4's `+0x08` record into word 0x3C (slot 0xF), pointing
// each installed entry's +0x88 stream pointer at entry+0xAC.
pub fn swing_battle_animations(
    buf: &[u8],
    pack: &BattleDataPack,
    equipped: &[u8; SECTION_COUNT],
) -> Result<Vec<SwingAnimation>> {
    let records = select_sections(pack, equipped)?;
    let mut out = Vec::with_capacity(4);
    for (section, rec) in records.iter().enumerate().take(SECTION_COUNT).skip(2) {
        let entry = decode_record(buf, pack, rec.index)
            .with_context(|| format!("decode section {section} (id {:#x})", rec.id))?;
        let d = &entry.bytes;
        let mut offsets = vec![(SWING_SLOT_BASE + (section as u8 - 2), read_u32(d, 4)?)];
        if section == 4 {
            offsets.push((0xF, read_u32(d, 8)?));
        }
        for (slot, off) in offsets {
            let off = off as usize;
            if off == 0 || off >= d.len() {
                bail!("section {section} swing record offset {off:#x} out of range");
            }
            let entry_tag = d[off];
            let cost = d.get(off + SWING_COST_OFFSET).copied().unwrap_or(0);
            let anim = parse_action_entry(d, off, slot).ok_or_else(|| {
                anyhow::anyhow!("section {section} swing record at {off:#x} has no valid stream")
            })?;
            out.push(SwingAnimation {
                slot,
                section,
                item_id: rec.id,
                entry_tag,
                cost,
                anim,
                face: crate::face_anim::FaceTracks::from_entry(d, off),
            });
        }
    }
    Ok(out)
}
