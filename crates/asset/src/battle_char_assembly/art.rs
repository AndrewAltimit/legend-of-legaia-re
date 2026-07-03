//! Art-animation bank (record[0] +0x58 -> dynamic slots 0x10/0x11)
//! plus per-object animation channel expansion.

use anyhow::{Context, Result, bail};

use super::read_u32;

/// Stride of one art-animation bank record.
pub const ART_RECORD_STRIDE: usize = 0xD0;

/// Offset of the embedded `0xAC`-byte action-entry header inside a bank
/// record (`0x24 + 0xAC` = the full `0xD0` stride).
pub const ART_ENTRY_OFFSET: usize = 0x24;

/// First staged anim id that resolves through the bank: id `q >= 0x10`
/// selects bank record `q - 0x10` (`FUN_8004AD80`).
pub const ART_ANIM_ID_BASE: u8 = 0x10;

/// One record of the per-character **art-animation bank** (record[0] image
/// word `+0x58`): `[u32 count]` then `count` `0xD0`-stride records, each a
/// `0x24`-byte arts-matcher head + a standard `0xAC`-byte action entry.
///
/// The anim commit `FUN_8004AD80` materializes a staged anim id
/// `q >= 0x10` from record `q - 0x10`: ids `0x10` and `0x1A` install at
/// runtime slot `0x11`, every other id at slot `0x10`; ids `> 0x1A`
/// additionally drive the HUD art-name display from [`Self::name`] and
/// `FUN_8004C650(char, id - 0x1B)`. The record's keyframe stream is **not
/// inline**: `FUN_8002B28C(_DAT_8007BD74, scratch, stream_source)` pulls it
/// from the `"ME"` archive resident in the side-band streaming buffer - see
/// [`art_me_archive`] / [`crate::me_archive`].
#[derive(Debug, Clone)]
pub struct ArtAnimRecord {
    /// Bank record index.
    pub index: usize,
    /// The staged anim id this record materializes (`0x10 + index`).
    pub anim_id: u8,
    /// Arts-matcher direction-command sequence (record `+0x00`, values
    /// `1..=4`, zero-terminated; empty for the un-named base records). The
    /// same combo bytes the arts matcher reads (the canonical
    /// `record +0` combo of the arts-randomizer corpus).
    pub combo: Vec<u8>,
    /// Index into the character's art `"ME"` stream archive (record
    /// `+0x0A`) - the `FUN_8002B28C` third argument.
    pub stream_source: u8,
    /// Inline art-name string (record `+0x10`, NUL-terminated ASCII, up to
    /// 20 bytes; empty on the base / un-named records).
    pub name: String,
    /// Action tag byte (entry `+0x00`; presentation-class id space
    /// `0x16..0x1F` on named arts, `0` on base records).
    pub entry_tag: u8,
    /// Attach key (entry `+0x77` = record `+0x9B`): the id the equipment
    /// sections' attach-object records match against (`FUN_80052FA0`'s
    /// bank scan compares attach-record `+0x07` with it).
    pub attach_key: u8,
    /// Playback-rate byte (entry `+0x78`, the `FUN_80047430` cursor
    /// multiplier; `1..=7` observed).
    pub rate: u8,
    /// Entry `+0x84` (a secondary anim-rate field - `FUN_8004AD80` copies
    /// it into actor `+0x21B`). `0xFF` marks the eight **base-archive**
    /// records present in every character's bank - see
    /// [`Self::uses_base_archive`].
    pub rate_alt: u8,
    /// The embedded entry's facial keyframe tracks (entry `+0x8C` eyes /
    /// `+0x98` mouth = record `+0xB0` / `+0xBC`). `FUN_8004AD80` installs
    /// the embedded entry (record `+0x24`) as the action-table slot
    /// `0x10`/`0x11` pointer, so while the materialized art clip plays the
    /// render node's `+0x4C` anim context is this entry and the per-frame
    /// facial animator `FUN_8004C7B4` reads these tracks - the mid-battle
    /// art-strike faces. Sibling of
    /// [`SwingAnimation::face`]; `None` only for a truncated record.
    pub face: Option<crate::face_anim::FaceTracks>,
    /// record[0]-image byte offset of the record's action-entry header.
    pub entry_offset: usize,
}

impl ArtAnimRecord {
    /// Whether this record's [`Self::stream_source`] indexes the
    /// character's **base** art archive (readef slot `3*char + 2`) instead
    /// of the main one (slot `3*char + 1`).
    ///
    /// Disc-pinned mapping (the art-path caller of the side-band request
    /// arm `FUN_80055B4C`, which stages the readef slot via the
    /// `ctx+0x26B` byte, is not in the dumped corpus): in all four
    /// retail files the records with
    /// `rate_alt == 0xFF` are exactly eight per character with
    /// `stream_source` `0..=7` = the base archive's exact entry range,
    /// while the remaining records' max `stream_source` equals the main
    /// archive's `count - 1` exactly (17/18/19/1 entries for
    /// Vahn/Noa/Gala/Terra).
    pub fn uses_base_archive(&self) -> bool {
        self.rate_alt == 0xFF
    }
}

/// Parse the art-animation bank out of a decoded record[0] image
/// ([`decode_record0`]): the self-relative word at `+0x58` locates
/// `[u32 count][count x 0xD0-stride records]`.
// PORT: FUN_8004AD80 (bank-record select) - dynamic anim id q >= 0x10 reads
// record q - 0x10 (entry pointer = bank + 4 + (q - 0x10)*0xD0 + 0x24, the
// `q*0xD0 + bank + 4 - 0xCDC` install arithmetic), name at -0xCF0
// (record +0x10), stream-source byte at -0xCF6 (record +0x0A).
// REF: FUN_80052FA0 - rebases the +0x58 word and scans the bank's
// `+0x9B` attach keys for the equipment attach-object records.
pub fn art_animation_bank(record0: &[u8]) -> Result<Vec<ArtAnimRecord>> {
    let bank_off = read_u32(record0, 0x58)? as usize;
    let count = read_u32(record0, bank_off).context("art bank count word")? as usize;
    if count == 0 || count > 0x40 {
        bail!("implausible art bank count {count}");
    }
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let base = bank_off + 4 + index * ART_RECORD_STRIDE;
        let rec = record0
            .get(base..base + ART_RECORD_STRIDE)
            .ok_or_else(|| anyhow::anyhow!("art bank record {index} past record[0] end"))?;
        let combo: Vec<u8> = rec[..0x0A]
            .iter()
            .copied()
            .take_while(|&b| b != 0)
            .collect();
        if combo.iter().any(|&b| !(1..=4).contains(&b)) {
            bail!("art bank record {index} combo byte outside 1..=4");
        }
        let name_raw = &rec[0x10..ART_ENTRY_OFFSET];
        let name_end = name_raw
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_raw.len());
        let name_bytes = &name_raw[..name_end];
        if !name_bytes.iter().all(|&b| (0x20..0x7F).contains(&b)) {
            bail!("art bank record {index} name is not printable ASCII");
        }
        let entry = &rec[ART_ENTRY_OFFSET..];
        out.push(ArtAnimRecord {
            index,
            anim_id: ART_ANIM_ID_BASE + index as u8,
            combo,
            stream_source: rec[0x0A],
            name: String::from_utf8_lossy(name_bytes).into_owned(),
            entry_tag: entry[0],
            attach_key: entry[0x77],
            rate: entry[0x78],
            rate_alt: entry[0x84],
            face: crate::face_anim::FaceTracks::from_entry(rec, ART_ENTRY_OFFSET),
            entry_offset: base + ART_ENTRY_OFFSET,
        });
    }
    Ok(out)
}

/// `readef.DAT` slot index of a character's art `"ME"` stream archive:
/// slot `3*char_index + 1` (main archive - the named arts) or
/// `3*char_index + 2` (base archive - the eight `rate_alt == 0xFF`
/// records). `char_index` is 0..=3 for Vahn/Noa/Gala/Terra. Disc-pinned
/// (see [`ArtAnimRecord::uses_base_archive`]); slot `3*char_index` is the
/// character's non-ME texture slot.
pub fn art_me_slot(char_index: usize, base: bool) -> usize {
    char_index * 3 + if base { 2 } else { 1 }
}

/// Slice + parse a character's art `"ME"` archive out of the raw
/// `readef.DAT` bytes (extraction PROT entry 894 - the
/// `crate::summon_readef` side-band file whose `0x10800`-byte slots the
/// battle streamer `FUN_801F17F8` reads into `_DAT_8007BD74`).
pub fn art_me_archive(
    readef: &[u8],
    char_index: usize,
    base: bool,
) -> Result<crate::me_archive::MeArchive<'_>> {
    use crate::summon_readef::SLOT_BYTES;
    let slot = art_me_slot(char_index, base);
    let bytes = readef
        .get(slot * SLOT_BYTES..(slot + 1) * SLOT_BYTES)
        .ok_or_else(|| anyhow::anyhow!("readef slot {slot} past file end"))?;
    crate::me_archive::parse(bytes).with_context(|| format!("art ME archive in readef slot {slot}"))
}

/// Resolve + decode one art record's keyframe animation through its `"ME"`
/// archive (the caller picks the archive per
/// [`ArtAnimRecord::uses_base_archive`]). The returned animation's
/// `action_id` is the record's staged anim id (`0x10 + index`) and `rate`
/// the record's entry `+0x78` byte.
pub fn art_animation(
    record: &ArtAnimRecord,
    archive: &crate::me_archive::MeArchive<'_>,
) -> Result<crate::monster_archive::MonsterAnimation> {
    let stream = archive
        .entry(record.stream_source as usize)
        .with_context(|| format!("art record {} stream", record.index))?;
    crate::monster_archive::parse_animation_stream(&stream, record.anim_id, record.rate, 0)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "art record {} stream (source {}) is not a valid keyframe stream",
                record.index,
                record.stream_source
            )
        })
}

/// Re-index an animation's pose channels **per assembled object**: output
/// part `i` is input part `anm_bones[i]` (each equipment extra duplicates
/// its attach bone's channel), so `frames[f][obj]` can drive TMD object
/// `obj` directly through the engine's posed-mesh builders. Channels
/// referencing a part the stream doesn't carry come out as the identity
/// pose.
pub fn expand_animation_for_objects(
    anim: &crate::monster_archive::MonsterAnimation,
    anm_bones: &[u8],
) -> crate::monster_archive::MonsterAnimation {
    let frames: Vec<Vec<crate::monster_archive::PartPose>> = anim
        .frames
        .iter()
        .map(|frame| {
            anm_bones
                .iter()
                .map(|&b| frame.get(b as usize).copied().unwrap_or_default())
                .collect()
        })
        .collect();
    crate::monster_archive::MonsterAnimation {
        action_id: anim.action_id,
        rate: anim.rate,
        part_count: anm_bones.len(),
        frame_count: anim.frame_count,
        frames,
    }
}
