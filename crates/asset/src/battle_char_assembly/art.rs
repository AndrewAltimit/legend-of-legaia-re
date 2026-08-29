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
    /// First of the art's four **power bytes**, not a tag - the name is a
    /// misnomer kept for source compatibility.
    ///
    /// The entry begins at record `+0x24` ([`ART_ENTRY_OFFSET`]), which
    /// `docs/formats/art-data.md` already pins as the power run, so
    /// `entry[0x00..0x04]` IS that run and this is its first byte. The
    /// old "presentation-class id space `0x16..0x1F`" reading
    /// survived because a first power byte often lands in that range by
    /// coincidence, and the disc contradicts it: Vahn's Burning Flare
    /// reads `1D 19 1F 1A` - four ascending Hyper power bytes, not one
    /// tag and three unknowns - Gala's two-hit Thunder Punch reads
    /// `19 1A 00 00`, and the no-damage Regular Art Starter reads
    /// `00 00 00 00`, which is a sensible power run and a nonsensical
    /// tag. Entry `+0x10..+0x13` carries the matching damage-timing run.
    pub entry_tag: u8,
    /// Attach key (entry `+0x77` = record `+0x9B`): the id the equipment
    /// sections' attach-object records match against (`FUN_80052FA0`'s
    /// bank scan compares attach-record `+0x07` with it).
    pub attach_key: u8,
    /// Playback-rate byte (entry `+0x78`, the `FUN_80047430` cursor
    /// multiplier; `1..=7` observed).
    pub rate: u8,
    /// Entry `+0x84`: the clip's **loop count**, not a rate.
    ///
    /// `FUN_8004AD80` (`0x8004BDEC`) copies it into both `actor+0x21B`
    /// and `actor+0x176 << 4`; the tick `FUN_80047430` (`0x80047768`)
    /// then, once the 12.4 cursor reaches `entry[+0x86] << 4`, subtracts
    /// `(+0x86 - +0x85) << 4` and decrements `+0x176`, so the stream
    /// cycles frames `[+0x85, +0x86]` that many times. `+0x21B` is only
    /// the remaining-count mirror - the cursor multiplier is `+0x21D`.
    /// (This field was previously documented here as "a secondary
    /// anim-rate field", which conflicted with
    /// `docs/formats/monster-animation.md`; the disassembly settles it
    /// in favour of the loop-count reading. A rate of `0` would freeze
    /// the clip, and `0` is what every playable art record but five
    /// carries.)
    ///
    /// `0xFF` marks the eight **base-archive** records present in every
    /// character's bank - see [`Self::uses_base_archive`].
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
    /// Entry `+0x7A` - the action's **impact-effect class**, a `1..=5`
    /// selector (`0` = none) bounded by `FUN_801EC3E4`'s `sltiu v0,v0,6`.
    /// That routine stores the selector at `actor[+0x21F]` and the row it
    /// indexes out of `0x801F53D4` at `actor[+0x04]`, and two renderers
    /// then read those, both of them **outside** the entry's own
    /// [`Self::effect_script`]:
    ///
    /// - `FUN_8004998C` streams an element spark along the swing path at
    ///   random cadence, `efect.dat` sprite `0x0B` for selector `1` and
    ///   `0x10` for selector `2`;
    /// - `FUN_80049348` draws fading afterimage copies of the mesh,
    ///   tinted from the per-CHARACTER table at `0x80076908`.
    ///
    /// So this byte, not the effect script, is what makes an art read as
    /// its owner's element: a reskin that rewrites only the script still
    /// shows the host character's sparks and ghost trail.
    pub impact_class: u8,
    /// record[0]-image byte offset of the record's action-entry header.
    pub entry_offset: usize,
    /// The embedded entry's head bytes (`+0x00..+0x54` of the entry = record
    /// `+0x24..+0x78`) - the action's battle **effect script** region (see
    /// [`crate::monster_archive::MonsterAnimation::effect_script`]). While
    /// the materialized art clip plays, the render node's anim context is
    /// this entry, so the per-frame effect-script walker (`FUN_801DEA50`)
    /// reads these records.
    pub effect_script: Vec<u8>,
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
            impact_class: entry[0x7A],
            entry_offset: base + ART_ENTRY_OFFSET,
            effect_script: crate::monster_archive::effect_script_head(rec, ART_ENTRY_OFFSET),
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
    crate::monster_archive::parse_animation_stream(
        &stream,
        record.anim_id,
        record.rate,
        record.attach_key,
        0,
        record.effect_script.clone(),
    )
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
        attach_key: anim.attach_key,
        rate: anim.rate,
        part_count: anm_bones.len(),
        frame_count: anim.frame_count,
        frames,
        effect_script: anim.effect_script.clone(),
    }
}
