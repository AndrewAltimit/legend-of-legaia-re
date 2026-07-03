//! Battle animation (record[0] per-action TRS streams).

use anyhow::{Result, bail};

use super::read_u32;

/// Number of action slots scanned in record[0]'s head offset table. On disc
/// only `+0x00..+0x2C` (slots 0..0xB) are populated; the runtime table is
/// wider: the battle loader `FUN_80052FA0` rebases the 12 disc words, fills
/// slots `0xC..0xF` (offsets `0x30..0x3C`) with swing records spliced from
/// the equipped-item sections ([`swing_battle_animations`]), and the anim
/// commit `FUN_8004AD80` installs dynamically-materialized art records at
/// slots `0x10`/`0x11` ([`art_animation_bank`]). The word at `+0x58` is the
/// **art-animation record bank** pointer (`[u32 count]` + `0xD0`-stride
/// records the dynamic slots are built from; also the art matcher's table),
/// and `+0x5C` a sibling pointer (rebased at load) - in all four retail
/// files it equals `clut_a_off - 4`, the zero word immediately before
/// record[0]'s first image block (consumer untraced; the "it points at the
/// art ME stream archive" hypothesis is disc-refuted - those archives live
/// in `readef.DAT`, see [`art_me_archive`]). Not texture-block offsets as
/// earlier noted.
pub const ACTION_SLOT_COUNT: usize = 22;

/// Offset of the packed `[u8 parts][u8 frames][9-byte TRS records]` stream
/// inside a record[0] action entry (the monster archive's sibling entries
/// keep theirs at `+0x8C`). The runtime loader points the entry's `+0x88`
/// stream pointer here (`FUN_80047430` / `FUN_8004AD80` consume it). The
/// entry's first byte is its **action tag** (identity with the slot index in
/// the player files: `0` idle, `1` walk/approach, `2`/`3` light flinches,
/// `4` knockdown, `5` get-up, `7`/`8`/`9` ready/recover/defeat poses, `0x0B`
/// block) - the key space of the actor `+0x1EF..+0x1F3` reaction map.
pub const PLAYER_ANIM_STREAM_OFFSET: usize = 0xAC;

/// LZS-decode a player file's `record[0]` (header
/// `[desc_off][clut_a][clut_b][budget]`, LZS stream at `+0x10`). Scans
/// 4-byte-aligned offsets for a plausible header (skipping any `"pochi"`
/// filler prefix on the historical over-read copies) and accepts the first
/// whose stream decompresses to its declared budget. Unlike
/// [`crate::battle_char_palette::find_record0`] this does **not** require
/// the fixed-stride palette-chain assembly to succeed (it overflows for
/// Noa / Gala - see [`crate::battle_char_palette::collect_palette`]).
pub fn decode_record0(file: &[u8]) -> Result<Vec<u8>> {
    let mut o = 0;
    while o + 0x10 <= file.len() {
        let desc_off = read_u32(file, o)? as usize;
        let clut_a = read_u32(file, o + 4)? as usize;
        let clut_b = read_u32(file, o + 8)? as usize;
        let budget = read_u32(file, o + 0xC)? as usize;
        let plausible = (0x100..file.len() - o).contains(&desc_off)
            && (0x1000..=0x4_0000).contains(&budget)
            && (0x10..budget).contains(&clut_a)
            && (0x10..budget).contains(&clut_b);
        if plausible && let Ok(decoded) = legaia_lzs::decompress(&file[o + 0x10..], budget) {
            return Ok(decoded);
        }
        o += 4;
    }
    bail!("no record[0] header found")
}

/// Decode the character's **battle action animations** from `record[0]` of
/// their player file: per populated action slot, the packed
/// `[u8 parts][u8 frames][9-byte TRS records]` stream at entry
/// `+`[`PLAYER_ANIM_STREAM_OFFSET`] - the same rigid-transform keyframe
/// format as the monster archive's per-action streams
/// (`docs/formats/monster-animation.md`), with `parts` = the character's
/// **skeleton bone count** (equipment extras carry no channel of their own
/// and ride their attach bone - see [`AssembledCharacter::anm_bones`]).
/// Slot 0 is the neutral idle loop; its frame 0 is the combat-stance rest
/// pose that sockets the assembled mesh.
///
/// `action_id` on the returned animations is the slot index.
// PORT: FUN_80047430 (anim-context consumer) - the battle party render
// node's +0x4C anim context is a record[0] action entry; its +0x88 stream
// pointer (loader-reconstructed, entry+0xAC) feeds the FUN_8004AD80 /
// FUN_8004998C keyframe decode chain shared with battle monsters.
pub fn battle_animations(file: &[u8]) -> Result<Vec<crate::monster_archive::MonsterAnimation>> {
    let block = decode_record0(file)?;
    let mut out = Vec::new();
    for slot in 0..ACTION_SLOT_COUNT {
        let Ok(entry_off) = read_u32(&block, slot * 4) else {
            break;
        };
        let entry_off = entry_off as usize;
        if entry_off == 0 || entry_off >= block.len() {
            continue;
        }
        let rate = block
            .get(entry_off + crate::monster_archive::ANIM_RATE_OFFSET)
            .copied()
            .unwrap_or(0);
        if let Some(anim) = crate::monster_archive::parse_animation_stream(
            &block,
            slot as u8,
            rate,
            entry_off + PLAYER_ANIM_STREAM_OFFSET,
        ) {
            out.push(anim);
        }
    }
    Ok(out)
}

/// Decode just the **idle** animation (action slot 0) of a player file -
/// the loop the battle engine plays while the character awaits commands.
/// Frame 0 is the rest pose that sockets the assembled battle mesh.
/// `Ok(None)` when slot 0 is absent or its stream doesn't decode.
pub fn idle_battle_animation(
    file: &[u8],
) -> Result<Option<crate::monster_archive::MonsterAnimation>> {
    let block = decode_record0(file)?;
    let entry_off = read_u32(&block, 0)? as usize;
    if entry_off == 0 || entry_off >= block.len() {
        return Ok(None);
    }
    let rate = block
        .get(entry_off + crate::monster_archive::ANIM_RATE_OFFSET)
        .copied()
        .unwrap_or(0);
    Ok(crate::monster_archive::parse_animation_stream(
        &block,
        0,
        rate,
        entry_off + PLAYER_ANIM_STREAM_OFFSET,
    ))
}
