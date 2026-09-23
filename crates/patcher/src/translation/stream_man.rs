//! The **uncompressed** scene MAN that leads a `data_field_streaming` scene
//! entry - the dungeon scenes (`dolk2`, `rikuroa`, `rayman`, `station`,
//! `balden2`, `ropeway2`, `taiku`, `doman`, `taiku2`, `nilboa2`) whose bundle
//! is a typed-chunk stream (`MAN / MES / MOVE / VDF`, see
//! `docs/formats/data-field.md`) instead of an LZS scene-asset table.
//!
//! The dialog exporter keys those scenes' lines `raw:<entry>:<off>` because
//! the text sits uncompressed in the PROT entry, and the importer used to
//! write them same-size only: no LZS footprint to recompress into, but also
//! no growth path, so every longer line fell back to abbreviation. This
//! module gives them the same generalized rewriter the LZS MANs have:
//!
//! 1. the MAN is chunk 0 (type `0x03`) at entry offset 4, so a `raw:` key's
//!    entry offset minus 4 is a decompressed-MAN offset and
//!    [`legaia_asset::man_edit::apply_text_edits`] relocates every crossing
//!    reference exactly as for a decoded LZS MAN;
//! 2. the chunk's `(type << 24) | size` header is rewritten with the grown
//!    size, and every later chunk (plus the stream terminator and whatever
//!    trails it) shifts up verbatim - the walker (`FUN_8002541C`) steps
//!    `4 + size` per chunk, so nothing else references a chunk's position;
//! 3. the grown entry is padded to whole sectors and handed to the disc
//!    relayout (`DiscPatcher::grow_prot_entries`) when it no longer fits the
//!    entry's own footprint - or written in place when the entry's trailing
//!    sector slack absorbs it.
//!
//! **Why the entry may grow at all.** The field init block-copies the entry
//! whole (`sectors << 6` iterations of 32 bytes, `sectors` = the TOC size)
//! into the `0x62C00`-byte asset arena at `_DAT_8007B85C`, so the copy grows
//! with the TOC and the only bound is the arena. The VDF morph applier
//! (`FUN_8001C604`) borrows the arena's **top** as a rest-pose scratch window
//! (`buf + 0x62C00 - vertex_count * 8`), so the grown entry must leave that
//! headroom: [`MAX_GROWN_FOOTPRINT`] keeps a 64 KiB margin, three times the
//! largest per-group scratch a retail scene needs. The Spanish disc did not
//! grow these ten entries (their Spanish text fits the USA footprints), so
//! unlike the LZS MANs there is no mastered precedent - the safety argument
//! is the arena bound above.

use std::ops::Range;

use legaia_asset::man_section;

use super::segments;

/// Size of the loader's asset arena (`FUN_8001E1B4`: `FUN_80017888(0, 0x62C00)`).
pub const STREAM_ARENA_BYTES: usize = 0x62C00;

/// Largest footprint a grown streaming entry may have: the arena minus a
/// 64 KiB headroom for the VDF morph scratch window at the arena's top.
pub const MAX_GROWN_FOOTPRINT: usize = STREAM_ARENA_BYTES - 0x10000;

const SECTOR: usize = 2048;
const MAN_TYPE: u8 = 0x03;

/// A streaming scene entry's leading MAN chunk, located and parsed.
#[derive(Debug, Clone)]
pub struct StreamManText {
    /// Entry offset of the MAN chunk's `(type << 24) | size` header word.
    pub header_off: usize,
    /// The MAN bytes (`entry[header_off + 4 ..][..man_len]`), uncompressed.
    pub man: Vec<u8>,
    /// Declared chunk size (always 4-aligned on the retail disc).
    pub man_len: usize,
    /// Entry offset one past the stream's zero-size terminator word.
    pub stream_end: usize,
}

impl StreamManText {
    /// Locate the leading MAN chunk of a streaming scene entry, or `None` when
    /// the entry is not a terminated typed-chunk stream led by a MAN that
    /// parses, or is not a genuine dialog carrier (the raw `0x1F <text> 0x00`
    /// framing occurs by coincidence in binary banks - see
    /// [`segments::is_dialog_carrier`]).
    pub fn locate(entry: &[u8]) -> Option<Self> {
        if !segments::is_dialog_carrier(entry) {
            return None;
        }
        let report = legaia_asset::parse_streaming(entry, 64).ok()?;
        if !report.terminated || report.chunks.is_empty() {
            return None;
        }
        let first = &report.chunks[0];
        if first.header_offset != 0 || first.type_byte != MAN_TYPE {
            return None;
        }
        let man_len = first.size as usize;
        // The walker advances by `size & !3`; every retail MAN chunk is
        // 4-aligned, and an unaligned one would put the next header inside
        // the MAN's own tail - refuse rather than guess.
        if man_len == 0 || !man_len.is_multiple_of(4) {
            return None;
        }
        let man = entry.get(4..4 + man_len)?.to_vec();
        man_section::parse(&man).ok()?;
        Some(Self {
            header_off: 0,
            man,
            man_len,
            stream_end: report.bytes_consumed,
        })
    }

    /// Entry-offset range the MAN bytes occupy.
    pub fn man_range(&self) -> Range<usize> {
        let start = self.header_off + 4;
        start..start + self.man_len
    }

    /// Rebuild the entry's **full-footprint payload** around a grown (or
    /// shrunk) MAN: same header offset, re-headered chunk, every later byte of
    /// the stream (chunks, terminator, trailer) shifted verbatim, zero-padded
    /// to whole sectors. `foot` is the entry's true on-disc footprint
    /// (`DiscPatcher::read_entry_footprint`), which must still carry this
    /// MAN. Returns `None` when the result would exceed
    /// [`MAX_GROWN_FOOTPRINT`] or `foot` does not match.
    pub fn rebuild(&self, foot: &[u8], grown_man: &[u8]) -> Option<Vec<u8>> {
        let range = self.man_range();
        if foot.get(range.clone())? != self.man.as_slice() || self.stream_end > foot.len() {
            return None;
        }
        let padded_len = grown_man.len().div_ceil(4) * 4;
        if padded_len >= 1 << 24 {
            return None;
        }
        let header = ((MAN_TYPE as u32) << 24) | padded_len as u32;
        let mut out = Vec::with_capacity(foot.len() + padded_len);
        out.extend_from_slice(&foot[..self.header_off]);
        out.extend_from_slice(&header.to_le_bytes());
        out.extend_from_slice(grown_man);
        out.resize(self.header_off + 4 + padded_len, 0);
        // Everything after the MAN through the terminator, plus any non-zero
        // trailer past it, shifts verbatim; the footprint's zero sector padding
        // is re-derived below rather than carried (carrying it would grow the
        // entry by a sector for a one-byte edit).
        let tail_end = foot
            .iter()
            .rposition(|&b| b != 0)
            .map_or(self.stream_end, |last| (last + 1).max(self.stream_end));
        out.extend_from_slice(&foot[range.end..tail_end]);
        // Never shorter than the footprint it replaces: the entry's TOC slot
        // is fixed, so a MAN that comes out smaller (a translation shorter
        // than the padded original) keeps its sectors and the difference is
        // zero fill - a shrink is a same-size write, not a failure.
        let sectors = out.len().div_ceil(SECTOR).max(foot.len() / SECTOR);
        out.resize(sectors * SECTOR, 0);
        if out.len() > MAX_GROWN_FOOTPRINT {
            return None;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic four-chunk stream around a fake MAN body, sector-padded.
    fn stream(man: &[u8], tail: &[&[u8]]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(((MAN_TYPE as u32) << 24) | man.len() as u32).to_le_bytes());
        v.extend_from_slice(man);
        for (i, t) in tail.iter().enumerate() {
            v.extend_from_slice(&(((4 + i as u32) << 24) | t.len() as u32).to_le_bytes());
            v.extend_from_slice(t);
        }
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(b"TRAILER!");
        let sectors = v.len().div_ceil(SECTOR);
        v.resize(sectors * SECTOR, 0);
        v
    }

    fn fake(len: usize, seed: u8) -> Vec<u8> {
        (0..len).map(|i| seed.wrapping_add(i as u8)).collect()
    }

    #[test]
    fn rebuild_shifts_later_chunks_and_pads_to_sectors() {
        let man = fake(64, 1);
        let mes = fake(40, 7);
        let mov = fake(100, 9);
        let foot = stream(&man, &[&mes, &mov]);
        let sm = StreamManText {
            header_off: 0,
            man: man.clone(),
            man_len: man.len(),
            stream_end: 4 + 64 + 4 + 40 + 4 + 100 + 4,
        };
        // Grow by 2046 bytes (crosses into a second sector) with an unaligned
        // length, so the header carries the 4-aligned size.
        let grown = fake(64 + 2046, 3);
        let out = sm.rebuild(&foot, &grown).expect("rebuild");
        assert_eq!(out.len() % SECTOR, 0);
        assert_eq!(out.len(), foot.len() + SECTOR);
        let hdr = u32::from_le_bytes(out[0..4].try_into().unwrap());
        assert_eq!(hdr >> 24, MAN_TYPE as u32);
        assert_eq!((hdr & 0xFF_FFFF) as usize, 2112);
        assert_eq!(&out[4..4 + grown.len()], grown.as_slice());
        assert_eq!(&out[4 + grown.len()..4 + 2112], &[0, 0]);
        let re = legaia_asset::parse_streaming(&out, 64).expect("re-parse");
        assert!(re.terminated);
        assert_eq!(re.chunks.len(), 3);
        let mes_off = re.chunks[1].header_offset + 4;
        assert_eq!(&out[mes_off..mes_off + 40], mes.as_slice());
        let mov_off = re.chunks[2].header_offset + 4;
        assert_eq!(&out[mov_off..mov_off + 100], mov.as_slice());
        assert_eq!(&out[re.bytes_consumed..re.bytes_consumed + 8], b"TRAILER!");
    }

    #[test]
    fn rebuild_that_fits_the_slack_keeps_the_footprint() {
        let man = fake(64, 1);
        let foot = stream(&man, &[&fake(40, 7)]);
        let sm = StreamManText {
            header_off: 0,
            man: man.clone(),
            man_len: 64,
            stream_end: 4 + 64 + 4 + 40 + 4,
        };
        let out = sm.rebuild(&foot, &fake(80, 2)).expect("rebuild");
        assert_eq!(out.len(), foot.len());
    }

    /// A MAN that comes out shorter than the original keeps the entry's
    /// sectors: the TOC slot is fixed, so a shrink is a same-size write with
    /// zero fill, never a payload smaller than the footprint.
    #[test]
    fn rebuild_never_shrinks_below_the_footprint() {
        let man = fake(3000, 1);
        let foot = stream(&man, &[&fake(40, 7)]);
        assert_eq!(foot.len(), 2 * SECTOR);
        let sm = StreamManText {
            header_off: 0,
            man: man.clone(),
            man_len: 3000,
            stream_end: 4 + 3000 + 4 + 40 + 4,
        };
        let out = sm.rebuild(&foot, &fake(100, 2)).expect("rebuild");
        assert_eq!(out.len(), foot.len());
        assert_eq!(&out[4..104], &fake(100, 2)[..]);
        assert!(out[out.len() - SECTOR..].iter().all(|&b| b == 0));
    }

    #[test]
    fn rebuild_refuses_a_mismatched_footprint_and_the_arena_bound() {
        let man = fake(64, 1);
        let foot = stream(&man, &[]);
        let sm = StreamManText {
            header_off: 0,
            man: fake(64, 2),
            man_len: 64,
            stream_end: 4 + 64 + 4,
        };
        assert!(sm.rebuild(&foot, &man).is_none(), "foot must carry the MAN");
        let sm = StreamManText {
            header_off: 0,
            man: man.clone(),
            man_len: 64,
            stream_end: 4 + 64 + 4,
        };
        assert!(
            sm.rebuild(&foot, &vec![0u8; MAX_GROWN_FOOTPRINT]).is_none(),
            "must stay under the arena headroom"
        );
    }
}
