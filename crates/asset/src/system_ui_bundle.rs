//! Boot-resident **system-UI TIM bundle** - the `prot::timpack`s at **raw
//! PROT TOC entries 0 and 1**.
//!
//! `PROT.DAT`'s data region starts at the sector the first TOC word names
//! (retail: sector 3), but the extraction index space starts at raw entry 2
//! (`init_data`; see [`docs/formats/prot.md`]). Raw entries 0 and 1 - the
//! region extraction filenames never visit - are two standalone TIM-packs
//! ([`docs/formats/tim-pack.md`]) carrying the boot-time system UI: the
//! menu-glyph / interior-page atlas (`PROT.DAT[0x11218]`, image `(960,256)`
//! 64x256), the system-UI sprite sheet (`0x018E0`, image `(896,256)`
//! 64x192), the boot cursor / icon parts, and (raw entry 1) a single UI
//! page at image `(640,0)` with a 256-entry row-480 CLUT.
//!
//! The retail per-TIM uploader `FUN_800198E0` (see
//! `ghidra/scripts/funcs/800198e0.txt`) walks the pack members in table
//! order and uploads each TIM's image block at its declared rect and its
//! CLUT block as a **flattened strip** - `LoadImage(rect = {clut_x, clut_y,
//! w*h, 1})`, NOT the declared `w x h` rectangle (which for the row-510/511
//! banks would overflow VRAM at y >= 512). The strips populate VRAM CLUT
//! rows 510/511 (plus the `(896, 498..=501)` and `(976, 304..=307)` side
//! cells and the raw-entry-1 row-480 strip); the images populate the
//! `x >= 896` texture-page band + the `(640,0)` page. All of it is uploaded
//! once at boot - resident from the title screen through every scene/mode,
//! never evicted - which is why field env meshes can reference CBA cells on
//! row 510 (`town01` env slots 21/26/74, `rikuroa` slots 50/51/63: CBA
//! `(64,510)`, texpage `(960,256)`) that no scene TIM ever uploads. See
//! [`docs/formats/npc-palette.md`] "Boot-resident strip band".
//!
//! Later pack members legitimately overwrite earlier ones: the sprite strip
//! at `0x19438` and the cursor parts at `0x19490..` land inside the
//! `(960,256)` atlas image and overwrite ~70 of its rows, and six
//! **row-patch members** (`0x1A018..0x1AA7C`, a bare 20-byte-header image
//! block - `[u32, u32]` preamble + TIM-style `[u32 bnum][u16 x, y, w, h]`
//! rect + halfword data, no TIM magic) patch single rows at
//! `(960, 456..=458)` / `(960, 460..=462)` inside the atlas (declared 256
//! words wide, clipped at the VRAM edge to the visible 64 - byte-verified
//! against live captures). Table-order upload reproduces the retail VRAM
//! byte-for-byte (this is the overwrite the [`crate::interior_page`]
//! module docs describe).
//!
//! [`crate::interior_page`] extracts the single atlas TIM; this module is
//! the whole-bundle sibling the engine's field VRAM pre-pass uploads.

use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context, Result, bail};
use legaia_tim::{Tim, Vram};

/// Byte offset of the first TOC sector word inside `PROT.DAT` (after the
/// two file-header words). Raw TOC entry `n` spans sectors
/// `word[n] .. word[n+1]`.
const TOC_WORDS_OFFSET: usize = 8;

/// How many raw TOC entries the bundle spans (raw entries 0 and 1).
pub const RAW_ENTRY_COUNT: usize = 2;

/// One TIM member of the bundle, in pack-table order.
#[derive(Clone)]
pub struct SystemUiTim {
    /// Which raw TOC entry the member came from (0 or 1).
    pub raw_entry: u8,
    /// Member index in that entry's TIM-pack offset table.
    pub member: usize,
    /// Byte offset of the TIM header within the raw entry's bytes.
    pub entry_offset: usize,
    /// Parsed TIM (declared rects preserved; strip semantics applied at
    /// upload time, not parse time).
    pub tim: Tim,
}

impl SystemUiTim {
    /// The **flat-strip** CLUT rect `FUN_800198E0` uploads for this TIM:
    /// `(clut_x, clut_y, w*h, 1)`. `None` for CLUT-less members.
    pub fn clut_strip_rect(&self) -> Option<(u16, u16, u16, u16)> {
        let clut = self.tim.clut.as_ref()?;
        Some((clut.fb_x, clut.fb_y, clut.w * clut.h, 1))
    }
}

/// A non-TIM **row-patch** member: a bare image block (no TIM magic) that
/// patches a horizontal strip of the atlas page. Layout:
/// `[u32, u32]` preamble, then a TIM-style data block
/// `[u32 bnum][u16 fb_x][u16 fb_y][u16 w_words][u16 h]` + `w*h` halfwords
/// (`bnum == 12 + w*h*2`). The retail six declare `(960, y, 256, 1)` -
/// wider than VRAM, clipped at the edge to the visible 64 words.
#[derive(Clone)]
pub struct RowPatch {
    /// Which raw TOC entry the member came from (0 or 1).
    pub raw_entry: u8,
    /// Member index in that entry's TIM-pack offset table.
    pub member: usize,
    /// Byte offset of the member within the raw entry's bytes.
    pub entry_offset: usize,
    /// Destination rect (declared; upload clips at the VRAM edge).
    pub fb_x: u16,
    pub fb_y: u16,
    pub w_words: u16,
    pub h: u16,
    /// `w_words * h` halfwords, little-endian bytes.
    pub data: Vec<u8>,
}

/// Try to read a member as a [`RowPatch`]. `None` when the bytes don't
/// carry a self-consistent bare image block.
fn parse_row_patch(bytes: &[u8]) -> Option<(u16, u16, u16, u16, Vec<u8>)> {
    if bytes.len() < 20 {
        return None;
    }
    let bnum = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let fb_x = u16::from_le_bytes(bytes[12..14].try_into().unwrap());
    let fb_y = u16::from_le_bytes(bytes[14..16].try_into().unwrap());
    let w = u16::from_le_bytes(bytes[16..18].try_into().unwrap());
    let h = u16::from_le_bytes(bytes[18..20].try_into().unwrap());
    if w == 0 || h == 0 || fb_y >= 512 {
        return None;
    }
    let data_len = (w as usize) * (h as usize) * 2;
    if bnum as usize != 12 + data_len || 20 + data_len > bytes.len() {
        return None;
    }
    Some((fb_x, fb_y, w, h, bytes[20..20 + data_len].to_vec()))
}

/// The parsed bundle: every TIM member of raw TOC entries 0 and 1, in
/// upload order (entry 0's table order, then entry 1's).
#[derive(Clone)]
pub struct SystemUiBundle {
    pub tims: Vec<SystemUiTim>,
    /// Non-TIM row-patch members (retail: six, patching atlas rows
    /// `(960, 456..=458)` / `(960, 460..=462)`). Kept separate from
    /// [`Self::tims`] but uploaded in the same pack-member order.
    pub row_patches: Vec<RowPatch>,
    /// Pack-member counts per raw entry (TIM + row-patch members; retail:
    /// 20 and 1).
    pub member_counts: [usize; RAW_ENTRY_COUNT],
}

impl std::fmt::Debug for SystemUiBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemUiBundle")
            .field("tims", &self.tims.len())
            .field("row_patches", &self.row_patches.len())
            .field("member_counts", &self.member_counts)
            .finish()
    }
}

impl SystemUiBundle {
    /// Upload the bundle the way `FUN_800198E0` leaves it in VRAM: members
    /// in pack-table order, image block at its declared rect, CLUT block
    /// as a flattened `(clut_x, clut_y, w*h, 1)` strip. Sequential
    /// last-write-wins, matching the retail per-TIM `LoadImage` sequence
    /// (later members overwrite - e.g. the cursor parts over the atlas
    /// image, the 256-entry atlas strip over the 16-entry member-3 strip).
    pub fn upload_to_vram(&self, vram: &mut Vram) {
        // Walk TIM + row-patch members merged back into pack-member order
        // (raw entry, then member index) so the layering matches the
        // retail sequence: atlas (member 8), sprite strip (9), row
        // patches (10..15), cursor parts (16..19).
        let mut ti = self.tims.iter().peekable();
        let mut pi = self.row_patches.iter().peekable();
        loop {
            let t_key = ti.peek().map(|m| (m.raw_entry, m.member));
            let p_key = pi.peek().map(|p| (p.raw_entry, p.member));
            match (t_key, p_key) {
                (None, None) => break,
                (Some(tk), pk) if pk.is_none() || tk < pk.unwrap() => {
                    let m = ti.next().unwrap();
                    vram.upload_tim_partial(&m.tim, true, false);
                    if let Some(clut) = m.tim.clut.as_ref() {
                        let mut bytes = Vec::with_capacity(clut.entries.len() * 2);
                        for &c in &clut.entries {
                            bytes.extend_from_slice(&c.to_le_bytes());
                        }
                        vram.write_clut_row(clut.fb_x, clut.fb_y, &bytes);
                    }
                }
                _ => {
                    let p = pi.next().unwrap();
                    vram.write_block(p.fb_x, p.fb_y, p.w_words, p.h, &p.data);
                }
            }
        }
    }

    /// Declared image rects `(fb_x, fb_y, width_in_words, height)` of every
    /// member, in upload order. VRAM-parity consumers use these (plus
    /// [`Self::clut_strip_rects`]) to know which cells the boot upload owns.
    pub fn image_rects(&self) -> Vec<(u16, u16, u16, u16)> {
        let mut rects: Vec<(u16, u16, u16, u16)> = self
            .tims
            .iter()
            .map(|m| {
                let img = &m.tim.image;
                (img.fb_x, img.fb_y, img.fb_w, img.h)
            })
            .collect();
        rects.extend(
            self.row_patches
                .iter()
                .map(|p| (p.fb_x, p.fb_y, p.w_words, p.h)),
        );
        rects
    }

    /// Flat-strip CLUT rects `(fb_x, fb_y, w*h, 1)` of every member that
    /// carries a CLUT, in upload order.
    pub fn clut_strip_rects(&self) -> Vec<(u16, u16, u16, u16)> {
        self.tims
            .iter()
            .filter_map(|m| m.clut_strip_rect())
            .collect()
    }
}

/// Read the three head TOC sector words (raw entry 0 start / raw entry 1
/// start / raw entry 2 start) from the PROT.DAT header bytes.
fn head_toc_sectors(header: &[u8]) -> Result<[u32; 3]> {
    let mut out = [0u32; 3];
    for (i, s) in out.iter_mut().enumerate() {
        let off = TOC_WORDS_OFFSET + i * 4;
        let Some(word) = header.get(off..off + 4) else {
            bail!("PROT.DAT header too short for TOC word {i}");
        };
        *s = u32::from_le_bytes(word.try_into().unwrap());
    }
    if out[0] == 0 || out[0] >= out[1] || out[1] >= out[2] {
        bail!(
            "PROT.DAT head TOC words not monotonic: {:?} (expected 0 < s0 < s1 < s2)",
            out
        );
    }
    Ok(out)
}

/// Parse the bundle from a full in-memory `PROT.DAT` image (the
/// web-viewer / `ProtIndex::from_bytes` path).
pub fn parse_from_prot_dat_bytes(prot: &[u8]) -> Result<SystemUiBundle> {
    let sectors = head_toc_sectors(prot)?;
    let ranges: Vec<&[u8]> = (0..RAW_ENTRY_COUNT)
        .map(|n| {
            let start = sectors[n] as usize * 0x800;
            let end = sectors[n + 1] as usize * 0x800;
            prot.get(start..end)
                .with_context(|| format!("raw TOC entry {n} range {start:#X}..{end:#X} past EOF"))
        })
        .collect::<Result<_>>()?;
    parse_entries(ranges[0], ranges[1])
}

/// Read + parse the bundle straight from a `PROT.DAT` file.
pub fn read_from_prot_dat(path: &std::path::Path) -> Result<SystemUiBundle> {
    let mut f = std::fs::File::open(path)
        .with_context(|| format!("open PROT.DAT at {}", path.display()))?;
    let mut header = [0u8; TOC_WORDS_OFFSET + 12];
    f.read_exact(&mut header).context("read PROT.DAT header")?;
    let sectors = head_toc_sectors(&header)?;
    let mut entries: Vec<Vec<u8>> = Vec::with_capacity(RAW_ENTRY_COUNT);
    for n in 0..RAW_ENTRY_COUNT {
        let start = sectors[n] as u64 * 0x800;
        let len = (sectors[n + 1] - sectors[n]) as usize * 0x800;
        f.seek(SeekFrom::Start(start))
            .with_context(|| format!("seek to raw TOC entry {n}"))?;
        let mut buf = vec![0u8; len];
        f.read_exact(&mut buf)
            .with_context(|| format!("read raw TOC entry {n}"))?;
        entries.push(buf);
    }
    parse_entries(&entries[0], &entries[1])
}

/// Parse the bundle from the two raw-entry byte slices (raw TOC entry 0,
/// then raw TOC entry 1). Validates both are TIM-packs and that entry 0
/// carries the menu-glyph atlas fingerprint (the
/// [`crate::interior_page::CLUT_RECT`] / [`crate::interior_page::IMAGE_RECT`]
/// member), so unrelated bytes error out instead of placing garbage in
/// VRAM. Non-TIM pack members are skipped (they are counted in
/// [`SystemUiBundle::member_counts`]).
pub fn parse_entries(entry0: &[u8], entry1: &[u8]) -> Result<SystemUiBundle> {
    let mut tims = Vec::new();
    let mut row_patches: Vec<RowPatch> = Vec::new();
    let mut member_counts = [0usize; RAW_ENTRY_COUNT];
    for (raw_entry, bytes) in [entry0, entry1].into_iter().enumerate() {
        if !legaia_prot::timpack::is_tim_pack(bytes) {
            bail!("raw TOC entry {raw_entry} is not a prot::timpack");
        }
        // Offset table: u32 count at +4, word offsets at +8; byte offset =
        // word * 4 + 4 (docs/formats/tim-pack.md).
        let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        member_counts[raw_entry] = count;
        for member in 0..count {
            let toff = 8 + member * 4;
            let word = i32::from_le_bytes(bytes[toff..toff + 4].try_into().unwrap());
            let off = (word as i64) * 4 + 4;
            if off < 0 || off as usize >= bytes.len() {
                continue;
            }
            let off = off as usize;
            match legaia_tim::parse(&bytes[off..]) {
                Ok(tim) => tims.push(SystemUiTim {
                    raw_entry: raw_entry as u8,
                    member,
                    entry_offset: off,
                    tim,
                }),
                Err(_) => {
                    // Non-TIM member: the atlas row-patch blocks (bare
                    // 20-byte-header image strips). Anything else is
                    // skipped.
                    if let Some((fb_x, fb_y, w, h, data)) = parse_row_patch(&bytes[off..]) {
                        row_patches.push(RowPatch {
                            raw_entry: raw_entry as u8,
                            member,
                            entry_offset: off,
                            fb_x,
                            fb_y,
                            w_words: w,
                            h,
                            data,
                        });
                    }
                }
            }
        }
    }
    // Fingerprint: the bundle must carry the menu-glyph / interior-page
    // atlas (raw entry 0 member with the pinned rects).
    let has_atlas = tims.iter().any(|m| {
        let img = &m.tim.image;
        let clut_ok = m
            .tim
            .clut
            .as_ref()
            .is_some_and(|c| (c.fb_x, c.fb_y, c.w, c.h) == crate::interior_page::CLUT_RECT);
        clut_ok
            && (img.fb_x, img.fb_y, img.fb_w, img.h) == crate::interior_page::IMAGE_RECT
            && m.raw_entry == 0
    });
    if !has_atlas {
        bail!(
            "system-UI bundle fingerprint missing: no raw-entry-0 TIM with CLUT {:?} + image {:?}",
            crate::interior_page::CLUT_RECT,
            crate::interior_page::IMAGE_RECT
        );
    }
    Ok(SystemUiBundle {
        tims,
        row_patches,
        member_counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise a minimal TIM with the given CLUT + image rects. CLUT
    /// entries are `base | 0x8000`, image words are `0xAB` bytes.
    fn synth_tim(
        clut: Option<(u16, u16, u16, u16)>,
        img: (u16, u16, u16, u16),
        clut_base: u16,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&(if clut.is_some() { 0x08u32 } else { 0x00 }).to_le_bytes());
        if let Some((cx, cy, cw, ch)) = clut {
            let n = cw as u32 * ch as u32;
            buf.extend_from_slice(&(12 + n * 2).to_le_bytes());
            for v in [cx, cy, cw, ch] {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            for i in 0..n {
                buf.extend_from_slice(&((clut_base + i as u16) | 0x8000).to_le_bytes());
            }
        }
        let (ix, iy, iw, ih) = img;
        buf.extend_from_slice(&(12 + iw as u32 * ih as u32 * 2).to_le_bytes());
        for v in [ix, iy, iw, ih] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf.extend_from_slice(&vec![0xAB; iw as usize * ih as usize * 2]);
        buf
    }

    /// Wrap members into a timpack blob (header signature + offset table).
    fn synth_pack(members: &[Vec<u8>]) -> Vec<u8> {
        let mut blob = vec![0u8; 8 + members.len() * 4];
        blob[2] = 0x01;
        blob[3] = 0x01;
        blob[4..8].copy_from_slice(&(members.len() as u32).to_le_bytes());
        // Data starts after the table, aligned to 4 so byte = word*4 + 4.
        let mut offs = Vec::new();
        for m in members {
            while !(blob.len() - 4).is_multiple_of(4) {
                blob.push(0);
            }
            offs.push(((blob.len() - 4) / 4) as i32);
            blob.extend_from_slice(m);
        }
        for (i, w) in offs.iter().enumerate() {
            blob[8 + i * 4..12 + i * 4].copy_from_slice(&w.to_le_bytes());
        }
        blob
    }

    fn synth_bundle_entries() -> (Vec<u8>, Vec<u8>) {
        // Entry 0: a small member-3-style strip TIM, the atlas fingerprint
        // TIM, and a non-TIM member.
        let strip16 = synth_tim(Some((0, 510, 16, 1)), (896, 0, 4, 4), 0x100);
        let atlas = synth_tim(
            Some(crate::interior_page::CLUT_RECT),
            crate::interior_page::IMAGE_RECT,
            0,
        );
        let not_a_tim = vec![0xEEu8; 64];
        // Row patch: preamble + bare image block (960, 456, 4 words x 1).
        let mut patch = Vec::new();
        patch.extend_from_slice(&0u32.to_le_bytes());
        patch.extend_from_slice(&0u32.to_le_bytes());
        patch.extend_from_slice(&(12u32 + 8).to_le_bytes());
        for v in [960u16, 456, 4, 1] {
            patch.extend_from_slice(&v.to_le_bytes());
        }
        for w in [0x1111u16, 0x2222, 0x3333, 0x4444] {
            patch.extend_from_slice(&w.to_le_bytes());
        }
        let entry0 = synth_pack(&[strip16, atlas, not_a_tim, patch]);
        // Entry 1: the single row-480 CLUT TIM.
        let ui = synth_tim(Some((0, 480, 256, 1)), (640, 0, 4, 4), 0x400);
        let entry1 = synth_pack(&[ui]);
        (entry0, entry1)
    }

    #[test]
    fn parses_and_uploads_strip_semantics() {
        let (e0, e1) = synth_bundle_entries();
        let bundle = parse_entries(&e0, &e1).expect("bundle parses");
        assert_eq!(bundle.member_counts, [4, 1]);
        assert_eq!(bundle.tims.len(), 3, "junk member skipped");
        assert_eq!(bundle.row_patches.len(), 1, "row patch parsed");

        let mut vram = Vram::new();
        bundle.upload_to_vram(&mut vram);
        // Atlas strip (256 entries from block base 0) overwrites the earlier
        // 16-entry member's strip on row 510 - table order, last write wins.
        assert_eq!(vram.pixel(0, 510), 0x8000);
        assert_eq!(vram.pixel(64, 510), 64 | 0x8000);
        assert_eq!(vram.pixel(255, 510), 255 | 0x8000);
        // The declared 16-row rect is NOT placed (row 511 untouched here).
        assert_eq!(vram.pixel(0, 511), 0);
        // Raw entry 1's row-480 strip.
        assert_eq!(vram.pixel(0, 480), 0x400 | 0x8000);
        assert_eq!(vram.pixel(255, 480), (0x400 + 255) | 0x8000);
        // Image blocks land at their declared rects.
        assert_ne!(vram.pixel(960, 300), 0);
        assert_ne!(vram.pixel(640, 0), 0);
        // The row patch overwrites the atlas image at (960, 456) - it is a
        // LATER pack member, so it wins the pack-order layering.
        assert_eq!(vram.pixel(960, 456), 0x1111);
        assert_eq!(vram.pixel(963, 456), 0x4444);
    }

    #[test]
    fn strip_rects_flatten_declared_banks() {
        let (e0, e1) = synth_bundle_entries();
        let bundle = parse_entries(&e0, &e1).expect("bundle parses");
        let strips = bundle.clut_strip_rects();
        assert!(strips.contains(&(0, 510, 16, 1)));
        assert!(
            strips.contains(&(0, 510, 256, 1)),
            "16x16 bank -> 256x1 strip"
        );
        assert!(strips.contains(&(0, 480, 256, 1)));
    }

    #[test]
    fn rejects_non_pack_and_missing_fingerprint() {
        let (e0, e1) = synth_bundle_entries();
        assert!(parse_entries(&[0u8; 64], &e1).is_err(), "entry0 not a pack");
        // A pack without the atlas fingerprint is refused.
        let no_atlas = synth_pack(&[synth_tim(Some((0, 510, 16, 1)), (896, 0, 4, 4), 0)]);
        assert!(parse_entries(&no_atlas, &e1).is_err());
        // Fingerprint in entry 1 doesn't count (must be raw entry 0).
        let atlas_pack = synth_pack(&[synth_tim(
            Some(crate::interior_page::CLUT_RECT),
            crate::interior_page::IMAGE_RECT,
            0,
        )]);
        assert!(parse_entries(&no_atlas, &atlas_pack).is_err());
        let _ = e0;
    }

    #[test]
    fn parse_from_prot_dat_bytes_reads_head_toc() {
        let (e0, e1) = synth_bundle_entries();
        // Synthesise a PROT.DAT head: header sectors 0..1, entry0 at
        // sector 1, entry1 after it, one trailing sector for raw entry 2.
        let e0_sectors = e0.len().div_ceil(0x800) as u32;
        let e1_sectors = e1.len().div_ceil(0x800) as u32;
        let s0 = 1u32;
        let s1 = s0 + e0_sectors;
        let s2 = s1 + e1_sectors;
        let mut prot = vec![0u8; (s2 as usize + 1) * 0x800];
        prot[8..12].copy_from_slice(&s0.to_le_bytes());
        prot[12..16].copy_from_slice(&s1.to_le_bytes());
        prot[16..20].copy_from_slice(&s2.to_le_bytes());
        prot[s0 as usize * 0x800..s0 as usize * 0x800 + e0.len()].copy_from_slice(&e0);
        prot[s1 as usize * 0x800..s1 as usize * 0x800 + e1.len()].copy_from_slice(&e1);
        let bundle = parse_from_prot_dat_bytes(&prot).expect("bundle parses from image");
        assert_eq!(bundle.tims.len(), 3);
        assert_eq!(bundle.row_patches.len(), 1);
    }
}
