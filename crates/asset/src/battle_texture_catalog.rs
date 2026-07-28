//! Battle-texture catalog: the headerless 4bpp texture blocks inside the
//! player battle files `data\battle\PLAYER1..4` (PROT extraction entries
//! 0863..0866).
//!
//! ## Why this is a third tier
//!
//! [`crate::tim_catalog`] scans raw bytes for the TIM magic and
//! [`crate::tim_deep_catalog`] scans LZS-decoded sections for the same
//! magic. A player file's character art is invisible to both, and not
//! because it hides better - **it is not a TIM**. The block is
//!
//! ```text
//!   [u16 clut_x][u16 clut_n][clut_n x u16 BGR555][w*h halfwords of 4bpp]
//! ```
//!
//! with no `0x10` magic word, no flag word, no per-half block header and
//! no geometry: the rect comes from the *caller* (the static per-section
//! table [`SECTION_TEXTURE_RECTS`] and the two `record[0]` constants
//! [`RECORD0_TEXTURE_RECTS`]), never from the bytes. So a magic scan
//! cannot find it and a TIM parser cannot read it, at either tier, by
//! construction. Format detail:
//! `docs/formats/battle-data-pack.md` § Texture-pool upload blocks.
//!
//! ## Addressing
//!
//! A block is keyed by `(PROT entry, record, section, pool offset)`:
//!
//! * **section blocks** - one per *flagged* equipment slot: the descriptor
//!   record's decoded `u16` at `+0x12` is non-zero and its `u32` at `+0x0C`
//!   (the TMD body end) is the pool offset. The section index is the count
//!   of `id = 0` boundary records before it, and picks the placement rect.
//! * **`record[0]` blocks** - the two the file header points at
//!   (`clut_a_off` at `+0x04`, `clut_b_off` at `+0x08`) inside the header's
//!   own LZS stream at `+0x10`. These live outside the descriptor table, so
//!   they carry `record_index = -1` and use `section` as the block ordinal.
//!
//! ## Validity rule
//!
//! No magic exists to check, so the gate is structural and exact: a
//! block's declared extent `4 + clut_n*2 + w*h*2` must land **precisely**
//! on the byte the layout says it ends at, and its `clut_n` must be a whole
//! number of 16-colour palettes. The end is the decoded record's for a
//! section block; the two `record[0]` blocks instead **chain** - block 0
//! ends exactly where block 1 begins, and block 1 finishes the record - so
//! only the last of them may run to the buffer end. Retail satisfies this
//! for every block in all four files, which means a short read, a wrong
//! rect or a mis-sized decode fails the catalog rather than emitting a
//! plausible row.
//!
//! A `clut_n` of 0 is admitted: it is a real retail shape (pixels only,
//! sampling a palette a sibling block installed on the shared CLUT row).
//! Decode those through [`assemble_clut_row`].
//!
//! ## For a generic consumer
//!
//! [`BattleTextureBlock`] mirrors a [`crate::tim_deep_catalog`] row's shape
//! (coordinates, dimensions, bpp, palette count, content hash, label), and
//! [`decode_block`] turns one into RGBA given only the `PROT.DAT` image and
//! its TOC spans. A texture browser can therefore list and render this
//! family without knowing what a battle pack, an equipment section or a
//! `record[0]` stream is. Pass an [`ItemNameTable`] to the builder and each
//! row's `label` names the equipment the art belongs to, which is what
//! makes a search box useful ("terra", "ra-seru", "boots").

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::battle_char_assembly::{
    CLUT_ENTRIES_PER_PALETTE, RECORD0_TEXTURE_RECTS, SECTION_TEXTURE_RECTS, TEXELS_PER_HALFWORD,
    TextureRect, TextureUpload, UPLOAD_BPP, parse_upload_block,
};
use crate::battle_data_pack::{self, BattleDataPack};
use crate::item_names::ItemNameTable;

/// The four PROT entries that carry a player battle file
/// (`PLAYER1..4` = Vahn / Noa / Gala / the all-default fourth file).
pub const PLAYER_FILE_ENTRIES: [u32; 4] = [863, 864, 865, 866];

/// Smallest entry the scan bothers to probe - mirrors the same gate
/// [`crate::categorize`] puts in front of `battle_data_pack::detect`, and
/// every retail player file is far above it.
pub const MIN_PACK_BYTES: usize = 0x10000;

/// Equipment sections per player file.
const SECTION_COUNT: usize = SECTION_TEXTURE_RECTS.len();

/// Which block of a player file a catalog row addresses. Parses from (and
/// prints as) the CLI-facing spelling: `header0` / `header1` for the two
/// `record[0]` blocks, a plain decimal descriptor index otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleTextureSlot {
    /// A header-resident `record[0]` block: `0` = `clut_a_off`,
    /// `1` = `clut_b_off`.
    Record0(u8),
    /// A flagged equipment section's texture pool, by descriptor-table
    /// index (the section index is derived from the table, not given).
    Section(usize),
}

impl std::fmt::Display for BattleTextureSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Record0(b) => write!(f, "header{b}"),
            Self::Section(r) => write!(f, "{r}"),
        }
    }
}

impl std::str::FromStr for BattleTextureSlot {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.trim();
        for i in 0..RECORD0_TEXTURE_RECTS.len() {
            if s.eq_ignore_ascii_case(&format!("header{i}")) {
                return Ok(Self::Record0(i as u8));
            }
        }
        s.parse::<usize>().map(Self::Section).map_err(|_| {
            format!("not a battle-texture slot: {s:?} (a record index, or header0 / header1)")
        })
    }
}

/// One headerless battle-texture block. Every field is derived metadata -
/// offsets, dimensions, palette counts, a content fingerprint - never any
/// pixel bytes, so the catalog is safe to commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BattleTextureBlock {
    /// Stable id = scan order (entries ascending, then `record[0]` blocks,
    /// then descriptor records ascending).
    pub id: u32,
    /// Owning PROT TOC entry index.
    pub entry_index: u32,
    /// Descriptor-table index, or `-1` for a header `record[0]` block.
    pub record_index: i32,
    /// Descriptor slot id (the equipment id this section's art belongs to);
    /// `0` for a section default and for `record[0]` blocks.
    pub record_id: u32,
    /// Equipment section `0..5` for a section block; the block ordinal
    /// `0`/`1` for a `record[0]` block (which `record_index = -1` marks).
    pub section: u32,
    /// Byte offset of the block within its decoded record.
    pub pool_offset: u64,
    /// Pixel width (texels).
    pub width: u32,
    /// Pixel height (rows).
    pub height: u32,
    /// Bits per pixel - always 4 in this family.
    pub bpp: u32,
    /// CLUT entries the block ships (`clut_n`).
    pub clut_entries: usize,
    /// 16-colour palettes those entries make up.
    pub clut_count: usize,
    /// VRAM x (halfwords) the CLUT run uploads to on row `0x1E1 + slot`.
    pub clut_x: u16,
    /// Bytes the block occupies in the decoded record
    /// (`4 + clut_entries*2 + width*height/2`).
    pub byte_len: usize,
    /// FNV-1a-64 of the block's decoded bytes **as stored** (before the
    /// runtime STP pass). A hash, not the bytes - it detects drift and
    /// dedupes shared art without committing Sony pixels.
    pub fnv1a: u64,
    /// Human-readable name for the block, for a UI list or a search box:
    /// the owning character plus, when an [`ItemNameTable`] is supplied,
    /// the equipment whose art this is (`"Noa - Ra-Seru Terra $8"`). Falls
    /// back to the equipment id, and to the section ordinal for the `id = 0`
    /// section defaults. Not folded into [`rollup`] - it depends on whether
    /// the caller had `SCUS_942.54` to hand, not on the disc bytes.
    pub label: String,
}

/// Display name of the character whose battle file lives in PROT entry
/// `entry` - the party order the four files are authored in.
pub fn character_of_entry(entry: u32) -> Option<&'static str> {
    match entry {
        863 => Some("Vahn"),
        864 => Some("Noa"),
        865 => Some("Gala"),
        866 => Some("Terra"),
        _ => None,
    }
}

/// Compose a block's [`BattleTextureBlock::label`].
///
/// The equipment ids in the descriptor table are item ids, so an item-name
/// table turns a row into something a person can search for by name -
/// "ra-seru", "terra", the character - instead of by coordinate.
fn label_for(
    entry_index: u32,
    record_index: i32,
    record_id: u32,
    section: u32,
    names: Option<&ItemNameTable>,
) -> String {
    let who = character_of_entry(entry_index)
        .map(str::to_string)
        .unwrap_or_else(|| format!("entry {entry_index}"));
    if record_index < 0 {
        return format!("{who} - shared record[0] block {section}");
    }
    if record_id == 0 {
        return format!("{who} - section {section} default");
    }
    match names.and_then(|n| n.name(record_id as u8)) {
        Some(name) => format!("{who} - {name}"),
        None => format!("{who} - equip 0x{record_id:02X}"),
    }
}

impl BattleTextureBlock {
    /// The slot selector that re-reaches this block.
    pub fn slot(&self) -> BattleTextureSlot {
        if self.record_index < 0 {
            BattleTextureSlot::Record0(self.section as u8)
        } else {
            BattleTextureSlot::Section(self.record_index as usize)
        }
    }

    /// Whether this is one of the two header `record[0]` blocks.
    pub fn is_record0(&self) -> bool {
        self.record_index < 0
    }
}

/// FNV-1a-64 of a byte slice.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A block resolved back out of a player file: the parsed upload plus the
/// addressing a write-back needs.
#[derive(Debug, Clone)]
pub struct ResolvedBlock {
    /// Selector this was reached by.
    pub slot: BattleTextureSlot,
    /// Section index (or the block ordinal for a `record[0]` block).
    pub section: usize,
    /// Descriptor index, or `None` for a `record[0]` block.
    pub record_index: Option<usize>,
    /// Descriptor slot id (`0` for defaults / `record[0]`).
    pub record_id: u32,
    /// Block offset within [`Self::decoded`].
    pub pool_offset: usize,
    /// The whole decoded record - the buffer a replacement splices into
    /// before recompressing.
    pub decoded: Vec<u8>,
    /// File offset of the record's LZS stream (past any `dec_size` prefix).
    pub stream_offset: usize,
    /// Bytes the record's slot allocates for that stream. A replacement
    /// must recompress into this budget; nothing downstream may move,
    /// because the descriptor chain pins every later record's offset.
    pub stream_capacity: usize,
    /// Bytes the retail stream actually consumes (`<= stream_capacity`).
    pub stream_consumed: usize,
    /// The decoded upload block.
    pub upload: TextureUpload,
}

/// Section index of each descriptor record: the number of `id = 0`
/// boundary records that precede it. Records past the fifth section (there
/// are none in retail) get [`SECTION_COUNT`].
fn section_of_each_record(pack: &BattleDataPack) -> Vec<usize> {
    let mut out = Vec::with_capacity(pack.records.len());
    let mut section = 0usize;
    for r in &pack.records {
        out.push(section.min(SECTION_COUNT));
        if r.id == 0 {
            section += 1;
        }
    }
    out
}

/// Validate a candidate block against the exact-extent rule and turn it
/// into a row. `decoded` is the record it lives in and `expected_end` the
/// byte the block must finish on - the end of the record for a section
/// block or for the last `record[0]` block, the *next* block's offset for
/// the first (the two `record[0]` blocks chain).
#[allow(clippy::too_many_arguments)]
fn row_for(
    id: u32,
    entry_index: u32,
    record_index: i32,
    record_id: u32,
    section: u32,
    pool: usize,
    rect: TextureRect,
    decoded: &[u8],
    expected_end: usize,
    names: Option<&ItemNameTable>,
) -> Option<BattleTextureBlock> {
    let clut_n = decoded
        .get(pool + 2..pool + 4)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()) as usize)?;
    if !clut_n.is_multiple_of(CLUT_ENTRIES_PER_PALETTE) || clut_n > CLUT_ROW_ENTRIES {
        return None;
    }
    let byte_len = 4 + clut_n * 2 + rect.pixel_bytes();
    // The exact-extent gate.
    if pool + byte_len != expected_end || expected_end > decoded.len() {
        return None;
    }
    let clut_x = u16::from_le_bytes(decoded.get(pool..pool + 2)?.try_into().unwrap());
    Some(BattleTextureBlock {
        id,
        entry_index,
        record_index,
        record_id,
        section,
        pool_offset: pool as u64,
        width: (rect.w as usize * TEXELS_PER_HALFWORD) as u32,
        height: rect.h as u32,
        bpp: UPLOAD_BPP,
        clut_entries: clut_n,
        clut_count: clut_n / CLUT_ENTRIES_PER_PALETTE,
        clut_x,
        byte_len,
        fnv1a: fnv1a64(&decoded[pool..pool + byte_len]),
        label: label_for(entry_index, record_index, record_id, section, names),
    })
}

/// Halfwords of the per-party-slot CLUT row (`0x1E1 + slot`) this family
/// uploads into: 16 four-bit palettes side by side. Retail's widest run is
/// 240 entries at `clut_x = 0`, so every block's run fits inside it.
pub const CLUT_ROW_ENTRIES: usize = CLUT_ENTRIES_PER_PALETTE * 16;

/// Assemble the CLUT row one player file installs, by replaying every
/// block's `(clut_x, entries)` upload into a 256-halfword line in retail
/// order (the two `record[0]` blocks first, then the flagged sections).
///
/// This is what makes a run of `clut_n = 0` readable: such a block ships
/// pixels only and samples a palette a *sibling* block put on the row, so
/// there is nothing block-local to decode it with. Row palette `n` covers
/// entries `n*16 .. n*16+16` - the same window a primitive's CBA column
/// selects.
pub fn assemble_clut_row(file: &[u8]) -> Result<Vec<u16>> {
    let pack = battle_data_pack::parse(file).context("not a player battle file")?;
    let mut row = vec![0u16; CLUT_ROW_ENTRIES];
    let mut place = |clut_x: u16, clut: &[u16]| -> Result<()> {
        let start = clut_x as usize;
        let end = start + clut.len();
        if end > CLUT_ROW_ENTRIES {
            bail!(
                "CLUT run at x={clut_x} ({} entries) overruns the row",
                clut.len()
            );
        }
        row[start..end].copy_from_slice(clut);
        Ok(())
    };
    if let Ok(decoded) = decode_record0(file) {
        for (i, rect) in RECORD0_TEXTURE_RECTS.iter().enumerate() {
            let Some(pool) = record0_pool(file, i) else {
                continue;
            };
            let Some(block) = decoded.get(pool..) else {
                continue;
            };
            if let Ok(u) = parse_upload_block(block, *rect, 0) {
                place(u.clut_x, &u.clut)?;
            }
        }
    }
    let sections = section_of_each_record(&pack);
    for (r, &section) in pack.records.iter().zip(&sections) {
        if section >= SECTION_COUNT {
            continue;
        }
        let Ok(entry) = battle_data_pack::decode_record(file, &pack, r.index) else {
            continue;
        };
        if let Ok(Some(u)) =
            crate::battle_char_assembly::section_texture_upload(&entry.bytes, section, 0)
        {
            place(u.clut_x, &u.clut)?;
        }
    }
    Ok(row)
}

/// Catalog every headerless texture block in one player battle file.
/// Returns an empty vector when `file` is not a battle-data pack.
pub fn build_from_file(entry_index: u32, file: &[u8], id: &mut u32) -> Vec<BattleTextureBlock> {
    build_from_file_with_names(entry_index, file, id, None)
}

/// [`build_from_file`] with an item-name table, so each row's `label`
/// carries the equipment's real name instead of its id.
pub fn build_from_file_with_names(
    entry_index: u32,
    file: &[u8],
    id: &mut u32,
    names: Option<&ItemNameTable>,
) -> Vec<BattleTextureBlock> {
    let Some(pack) = battle_data_pack::detect(file) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    // The two header `record[0]` blocks, in header-word order. They chain:
    // block 0 ends exactly where block 1 starts, and block 1 finishes the
    // decoded record.
    if let Ok(decoded) = decode_record0(file) {
        for (i, rect) in RECORD0_TEXTURE_RECTS.iter().enumerate() {
            let Some(pool) = record0_pool(file, i) else {
                continue;
            };
            let expected_end = record0_pool(file, i + 1)
                .filter(|_| i + 1 < RECORD0_TEXTURE_RECTS.len())
                .unwrap_or(decoded.len());
            if let Some(row) = row_for(
                *id,
                entry_index,
                -1,
                0,
                i as u32,
                pool,
                *rect,
                &decoded,
                expected_end,
                names,
            ) {
                out.push(row);
                *id += 1;
            }
        }
    }

    // One per flagged equipment-section record.
    let sections = section_of_each_record(&pack);
    for (r, &section) in pack.records.iter().zip(&sections) {
        if section >= SECTION_COUNT {
            continue;
        }
        let Ok(entry) = battle_data_pack::decode_record(file, &pack, r.index) else {
            continue;
        };
        let d = &entry.bytes;
        // The `+0x12` upload flag gates the pool: an unflagged section's
        // pool bytes are dead (retail overwrites them without uploading).
        let flagged = d
            .get(0x12..0x14)
            .is_some_and(|b| u16::from_le_bytes(b.try_into().unwrap()) != 0);
        if !flagged {
            continue;
        }
        let Some(pool) = d
            .get(0x0C..0x10)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        else {
            continue;
        };
        if let Some(row) = row_for(
            *id,
            entry_index,
            r.index as i32,
            r.id,
            section as u32,
            pool,
            SECTION_TEXTURE_RECTS[section],
            d,
            d.len(),
            names,
        ) {
            out.push(row);
            *id += 1;
        }
    }
    out
}

/// Decode the header's `record[0]` stream (the one at `+0x10`, budget at
/// `+0x0C`).
fn decode_record0(file: &[u8]) -> Result<Vec<u8>> {
    let (decoded, _) = decode_record0_tracked(file)?;
    Ok(decoded)
}

fn decode_record0_tracked(file: &[u8]) -> Result<(Vec<u8>, usize)> {
    let budget = file
        .get(0x0C..0x10)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        .context("player file shorter than its header")?;
    if budget == 0 || budget > 0x40_0000 {
        bail!("implausible record[0] budget {budget:#x}");
    }
    let stream = file
        .get(0x10..)
        .context("player file shorter than its record[0] stream")?;
    legaia_lzs::decompress_tracked(stream, budget).context("decompress record[0]")
}

/// Header offset of `record[0]` block `i` (`clut_a_off` / `clut_b_off`).
fn record0_pool(file: &[u8], i: usize) -> Option<usize> {
    let off = 4 + i * 4;
    file.get(off..off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
}

/// Build the catalog from a flat `PROT.DAT` image and its TOC entry spans
/// (`(byte_offset, size_bytes, index)`, the same shape the TIM catalogs
/// take). Entries below [`MIN_PACK_BYTES`] or failing
/// `battle_data_pack::detect` contribute nothing.
pub fn build_from_spans(prot: &[u8], entry_spans: &[(u64, u64, u32)]) -> Vec<BattleTextureBlock> {
    build_from_spans_with_names(prot, entry_spans, None)
}

/// [`build_from_spans`] with an item-name table (parse one with
/// [`ItemNameTable::from_scus`] over the disc's `SCUS_942.54`), so each
/// row's `label` names the equipment its art belongs to.
pub fn build_from_spans_with_names(
    prot: &[u8],
    entry_spans: &[(u64, u64, u32)],
    names: Option<&ItemNameTable>,
) -> Vec<BattleTextureBlock> {
    let mut spans: Vec<(u64, u64, u32)> = entry_spans.to_vec();
    spans.sort_unstable_by_key(|&(_, _, idx)| idx);

    let mut out = Vec::new();
    let mut id = 0u32;
    for &(off, size, index) in &spans {
        let start = off as usize;
        let end = start.saturating_add(size as usize);
        if end > prot.len() || size < MIN_PACK_BYTES as u64 {
            continue;
        }
        out.extend(build_from_file_with_names(
            index,
            &prot[start..end],
            &mut id,
            names,
        ));
    }
    out
}

/// Build from an open [`legaia_prot::archive::Archive`].
pub fn build(archive: &legaia_prot::archive::Archive, prot: &[u8]) -> Vec<BattleTextureBlock> {
    let spans: Vec<(u64, u64, u32)> = archive
        .entries
        .iter()
        .map(|e| (e.byte_offset, e.size_bytes, e.index))
        .collect();
    build_from_spans(prot, &spans)
}

/// Convenience: open a `PROT.DAT` file and build its battle-texture
/// catalog.
pub fn build_from_path(path: &std::path::Path) -> Result<Vec<BattleTextureBlock>> {
    let archive = legaia_prot::archive::Archive::open(path)?;
    let prot = std::fs::read(path)?;
    Ok(build(&archive, &prot))
}

/// Re-read one block out of a player file, with the addressing a
/// write-back needs. `party_slot` only selects the VRAM band the returned
/// upload reports - it does not change a pixel.
pub fn resolve_block(
    file: &[u8],
    slot: BattleTextureSlot,
    party_slot: u8,
) -> Result<ResolvedBlock> {
    let pack = battle_data_pack::parse(file).context("not a player battle file")?;
    match slot {
        BattleTextureSlot::Record0(block) => {
            let i = block as usize;
            let rect = *RECORD0_TEXTURE_RECTS
                .get(i)
                .with_context(|| format!("record[0] block {i} out of range (0 / 1)"))?;
            let (decoded, consumed) = decode_record0_tracked(file)?;
            let pool = record0_pool(file, i).context("header clut offset past end")?;
            let upload = parse_upload_block(
                decoded
                    .get(pool..)
                    .with_context(|| format!("record[0] block at {pool:#x} past decoded end"))?,
                rect,
                party_slot,
            )
            .with_context(|| format!("record[0] block {i} at {pool:#x}"))?;
            Ok(ResolvedBlock {
                slot,
                section: i,
                record_index: None,
                record_id: 0,
                pool_offset: pool,
                decoded,
                stream_offset: 0x10,
                // The header stream runs up to the descriptor table.
                stream_capacity: pack.table_offset.saturating_sub(0x10),
                stream_consumed: consumed,
                upload,
            })
        }
        BattleTextureSlot::Section(record_index) => {
            let record = *pack.records.get(record_index).with_context(|| {
                format!(
                    "record {record_index} out of range: this player file has {}",
                    pack.records.len()
                )
            })?;
            let section = section_of_each_record(&pack)[record_index];
            if section >= SECTION_COUNT {
                bail!("record {record_index} sits past the {SECTION_COUNT} equipment sections");
            }
            let file_off = record.file_offset(pack.data_base);
            let dec_size = file
                .get(file_off..file_off + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
                .context("record dec_size prefix past file end")?;
            let (decoded, consumed) =
                legaia_lzs::decompress_tracked(&file[file_off + 4..], dec_size)
                    .with_context(|| format!("decompress record {record_index}"))?;
            let flag = decoded
                .get(0x12..0x14)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .context("decoded record shorter than its header")?;
            if flag == 0 {
                bail!(
                    "record {record_index} is not flagged for texture upload (+0x12 is 0), so \
                     its pool bytes are dead - retail overwrites them with the next section's \
                     decode without ever uploading them"
                );
            }
            let pool = decoded
                .get(0x0C..0x10)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
                .context("decoded record shorter than its header")?;
            let upload = parse_upload_block(
                decoded
                    .get(pool..)
                    .with_context(|| format!("texture pool at {pool:#x} past decoded end"))?,
                SECTION_TEXTURE_RECTS[section],
                party_slot,
            )
            .with_context(|| format!("record {record_index} pool at {pool:#x}"))?;
            Ok(ResolvedBlock {
                slot,
                section,
                record_index: Some(record_index),
                record_id: record.id,
                pool_offset: pool,
                decoded,
                stream_offset: file_off + 4,
                // The slot's allocation footprint less the dec_size prefix.
                stream_capacity: record.size as usize - 4,
                stream_consumed: consumed,
                upload,
            })
        }
    }
}

/// A cataloged block decoded to pixels: everything a viewer needs and
/// nothing about how it was stored.
#[derive(Debug, Clone)]
pub struct DecodedBattleTexture {
    pub width: usize,
    pub height: usize,
    /// Row-major RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
    /// Palettes the block carries of its own. `0` means the block is
    /// pixel-only and the decode borrowed a window of the file's assembled
    /// CLUT row instead.
    pub palette_count: usize,
    /// Whether `palette` indexed the assembled row rather than the block.
    pub used_assembled_row: bool,
}

/// Decode one cataloged row to RGBA, straight from a flat `PROT.DAT` image
/// and its TOC spans.
///
/// This is the entry point for a consumer that only holds catalog rows -
/// a texture browser, an exporter - and should not have to know what a
/// battle pack, an equipment section or a `record[0]` stream is. `palette`
/// selects one of the block's own 16-colour palettes; for the rare
/// pixel-only block it selects a window of the CLUT row the whole player
/// file assembles, and [`DecodedBattleTexture::used_assembled_row`] says
/// which happened.
pub fn decode_block(
    prot: &[u8],
    entry_spans: &[(u64, u64, u32)],
    block: &BattleTextureBlock,
    palette: usize,
) -> Result<DecodedBattleTexture> {
    let (off, size, _) = entry_spans
        .iter()
        .copied()
        .find(|&(_, _, idx)| idx == block.entry_index)
        .with_context(|| format!("no TOC span for PROT entry {}", block.entry_index))?;
    let start = off as usize;
    let end = start
        .checked_add(size as usize)
        .filter(|&e| e <= prot.len())
        .with_context(|| format!("PROT entry {} span runs off the image", block.entry_index))?;
    let file = &prot[start..end];

    let resolved = resolve_block(file, block.slot(), 0)?;
    let palette_count = resolved.upload.palette_count();
    let (pal, used_assembled_row) = if palette_count > 0 {
        (resolved.upload.palette(palette)?.to_vec(), false)
    } else {
        let row = assemble_clut_row(file)?;
        let start = palette * CLUT_ENTRIES_PER_PALETTE;
        let window = row
            .get(start..start + CLUT_ENTRIES_PER_PALETTE)
            .with_context(|| {
                format!(
                    "assembled-row palette {palette} out of range (the row holds {})",
                    row.len() / CLUT_ENTRIES_PER_PALETTE
                )
            })?
            .to_vec();
        (window, true)
    };
    let rgba = resolved.upload.rgba_with_palette(&pal)?;
    Ok(DecodedBattleTexture {
        width: resolved.upload.pixel_width(),
        height: resolved.upload.pixel_height(),
        rgba,
        palette_count,
        used_assembled_row,
    })
}

/// Canonical, diff-friendly serialization: a one-line header then one
/// tab-separated row per block. `fnv1a` is lowercase 16-hex-digit.
pub fn to_tsv(catalog: &[BattleTextureBlock]) -> String {
    let mut s = String::new();
    s.push_str(
        "id\tentry_index\trecord_index\trecord_id\tsection\tpool_offset\twidth\theight\tbpp\t\
         clut_entries\tclut_count\tclut_x\tbyte_len\tfnv1a\tlabel\n",
    );
    for b in catalog {
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\t{}\n",
            b.id,
            b.entry_index,
            b.record_index,
            b.record_id,
            b.section,
            b.pool_offset,
            b.width,
            b.height,
            b.bpp,
            b.clut_entries,
            b.clut_count,
            b.clut_x,
            b.byte_len,
            b.fnv1a,
            b.label,
        ));
    }
    s
}

/// Count plus an order-sensitive FNV-1a-64 fold of every block's
/// structural fields, so one number guards the whole catalog against
/// drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rollup {
    pub count: usize,
    pub digest: u64,
}

pub fn rollup(catalog: &[BattleTextureBlock]) -> Rollup {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut fold = |v: u64| {
        for b in v.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for b in catalog {
        fold(b.entry_index as u64);
        fold(b.record_index as i64 as u64);
        fold(b.section as u64);
        fold(b.pool_offset);
        fold(b.width as u64);
        fold(b.height as u64);
        fold(b.clut_entries as u64);
        fold(b.byte_len as u64);
        fold(b.fnv1a);
    }
    Rollup {
        count: catalog.len(),
        digest: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic player file with one flagged section record whose
    /// pool block finishes the record exactly. `clut_n` and the trailing
    /// slack are knobs so the validity gate can be exercised.
    fn synth_player_file(clut_n: usize, extra_tail: usize) -> Vec<u8> {
        let rect = SECTION_TEXTURE_RECTS[0];
        // Decoded record: 0x20-byte header, pool right after it.
        let pool = 0x20usize;
        let mut decoded = vec![0u8; pool];
        decoded[0x00..0x04].copy_from_slice(&0x14u32.to_le_bytes()); // frame_off
        decoded[0x0C..0x10].copy_from_slice(&(pool as u32).to_le_bytes()); // pool
        decoded[0x12..0x14].copy_from_slice(&1u16.to_le_bytes()); // upload flag
        decoded.extend_from_slice(&0x30u16.to_le_bytes()); // clut_x
        decoded.extend_from_slice(&(clut_n as u16).to_le_bytes());
        for i in 0..clut_n {
            // Non-zero entries so the STP pass has something to do.
            decoded.extend_from_slice(&((i as u16 + 1) | 0x0100).to_le_bytes());
        }
        decoded.extend(std::iter::repeat_n(0xA5u8, rect.pixel_bytes()));
        decoded.extend(std::iter::repeat_n(0u8, extra_tail));

        let stream = legaia_lzs::compress_optimal(&decoded);
        let slot_size = ((4 + stream.len()).div_ceil(0x800) * 0x800) as u32;

        // record[0]'s own stream: a tiny buffer with two pool offsets that
        // deliberately do NOT satisfy the exact-extent rule, so this
        // fixture yields exactly one row.
        let r0 = vec![0u8; 0x40];
        let r0_stream = legaia_lzs::compress_optimal(&r0);

        let desc_off = 0x10 + r0_stream.len().div_ceil(4) * 4 + 0x10;
        let mut buf = Vec::new();
        buf.extend_from_slice(&(desc_off as u32).to_le_bytes());
        buf.extend_from_slice(&0x10u32.to_le_bytes()); // clut_a_off
        buf.extend_from_slice(&0x20u32.to_le_bytes()); // clut_b_off
        buf.extend_from_slice(&(r0.len() as u32).to_le_bytes()); // budget
        buf.extend_from_slice(&r0_stream);
        buf.resize(desc_off, 0);
        buf.extend_from_slice(&1u32.to_le_bytes()); // id
        buf.extend_from_slice(&0u32.to_le_bytes()); // offset
        buf.extend_from_slice(&slot_size.to_le_bytes());
        // A second, id = 0 record so the table declares a section boundary.
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&slot_size.to_le_bytes());
        buf.extend_from_slice(&0x800u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 12]); // terminator
        while !buf.len().is_multiple_of(0x800) {
            buf.push(0);
        }
        let data_base = buf.len();
        buf.extend_from_slice(&(decoded.len() as u32).to_le_bytes());
        buf.extend_from_slice(&stream);
        buf.resize(data_base + slot_size as usize, 0);
        // The id = 0 slot: a plausible dec_size prefix, contents irrelevant.
        buf.extend_from_slice(&0x40u32.to_le_bytes());
        buf.resize(data_base + slot_size as usize + 0x800, 0);
        buf
    }

    #[test]
    fn catalogs_a_synthetic_flagged_section() {
        let file = synth_player_file(32, 0);
        let mut id = 0u32;
        let rows = build_from_file(863, &file, &mut id);
        assert_eq!(rows.len(), 1, "one flagged section, no record[0] hits");
        let b = &rows[0];
        assert_eq!(b.entry_index, 863);
        assert_eq!(b.record_index, 0);
        assert_eq!(b.record_id, 1);
        assert_eq!(b.section, 0);
        assert_eq!(b.pool_offset, 0x20);
        assert_eq!((b.width, b.height, b.bpp), (128, 128, 4));
        assert_eq!(b.clut_entries, 32);
        assert_eq!(b.clut_count, 2);
        assert_eq!(b.clut_x, 0x30);
        assert_eq!(b.byte_len, 4 + 32 * 2 + 128 * 128 / 2);
        assert_eq!(b.slot(), BattleTextureSlot::Section(0));
        assert!(!b.is_record0());
        // Without a name table the label still carries the character.
        assert_eq!(b.label, "Vahn - equip 0x01");

        // With one, it carries the searchable equipment name.
        let names = ItemNameTable::from_names(
            (0..256u16)
                .map(|i| (i == 1).then(|| "Ra-Seru Meta $1".to_string()))
                .collect(),
        );
        let mut id = 0u32;
        let named = build_from_file_with_names(863, &file, &mut id, Some(&names));
        assert_eq!(named[0].label, "Vahn - Ra-Seru Meta $1");
    }

    #[test]
    fn labels_name_the_character_the_section_and_the_equipment() {
        // The four entries are the four party files; anything else falls
        // back to naming the entry rather than inventing a character.
        assert_eq!(character_of_entry(864), Some("Noa"));
        assert_eq!(character_of_entry(12), None);
        assert_eq!(
            label_for(864, 14, 0x11, 2, None),
            "Noa - equip 0x11",
            "no name table: the id is still the search key"
        );
        let names = ItemNameTable::from_names(
            (0..256u16)
                .map(|i| (i == 0x11).then(|| "Ra-Seru Terra $8".to_string()))
                .collect(),
        );
        assert_eq!(
            label_for(864, 14, 0x11, 2, Some(&names)),
            "Noa - Ra-Seru Terra $8"
        );
        // Section defaults and record[0] blocks have no equipment to name.
        assert_eq!(
            label_for(864, 49, 0, 4, Some(&names)),
            "Noa - section 4 default"
        );
        assert_eq!(
            label_for(863, -1, 0, 1, Some(&names)),
            "Vahn - shared record[0] block 1"
        );
        assert_eq!(label_for(12, 0, 5, 0, None), "entry 12 - equip 0x05");
    }

    #[test]
    fn exact_extent_gate_rejects_a_short_record() {
        // Same block, but the record carries 16 trailing bytes past it -
        // so the declared extent no longer finishes the record.
        let file = synth_player_file(32, 16);
        let mut id = 0u32;
        assert!(build_from_file(863, &file, &mut id).is_empty());
    }

    #[test]
    fn partial_palette_runs_are_rejected() {
        // 20 entries is neither one palette nor two.
        let file = synth_player_file(20, 0);
        let mut id = 0u32;
        assert!(build_from_file(863, &file, &mut id).is_empty());
    }

    /// A player file whose `record[0]` stream carries the two chained
    /// header blocks: `clut_a` runs exactly into `clut_b`, which finishes
    /// the record. `a_clut_n = 0` mirrors retail `PLAYER1`'s pixel-only
    /// first block.
    fn synth_with_record0(a_clut_n: usize, b_clut_n: usize) -> Vec<u8> {
        let mut r0 = vec![0u8; 0x40]; // lead-in the header offsets skip
        let push_block = |buf: &mut Vec<u8>, clut_n: usize, rect: TextureRect, x: u16| {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&(clut_n as u16).to_le_bytes());
            for i in 0..clut_n {
                buf.extend_from_slice(&((i as u16 + 1) | 0x0200).to_le_bytes());
            }
            buf.extend(std::iter::repeat_n(0x5Au8, rect.pixel_bytes()));
        };
        let clut_a_off = r0.len();
        push_block(&mut r0, a_clut_n, RECORD0_TEXTURE_RECTS[0], 0);
        let clut_b_off = r0.len();
        push_block(&mut r0, b_clut_n, RECORD0_TEXTURE_RECTS[1], 64);

        let r0_stream = legaia_lzs::compress_optimal(&r0);
        let desc_off = 0x10 + r0_stream.len().div_ceil(4) * 4 + 0x10;
        let mut buf = Vec::new();
        buf.extend_from_slice(&(desc_off as u32).to_le_bytes());
        buf.extend_from_slice(&(clut_a_off as u32).to_le_bytes());
        buf.extend_from_slice(&(clut_b_off as u32).to_le_bytes());
        buf.extend_from_slice(&(r0.len() as u32).to_le_bytes());
        buf.extend_from_slice(&r0_stream);
        buf.resize(desc_off, 0);
        // One id = 0 record so the table parses; its slot holds no texture.
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0x800u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 12]);
        while !buf.len().is_multiple_of(0x800) {
            buf.push(0);
        }
        let data_base = buf.len();
        buf.extend_from_slice(&0x40u32.to_le_bytes());
        buf.resize(data_base + 0x800, 0);
        buf
    }

    #[test]
    fn record0_blocks_are_admitted_when_they_chain() {
        let file = synth_with_record0(0, 32);
        let mut id = 0u32;
        let rows = build_from_file(865, &file, &mut id);
        assert_eq!(rows.len(), 2, "both header blocks, got {rows:?}");
        assert!(rows.iter().all(|b| b.is_record0()));
        assert_eq!(rows[0].section, 0);
        assert_eq!(
            rows[0].clut_entries, 0,
            "a pixel-only block is still a block"
        );
        assert_eq!(rows[0].clut_count, 0);
        assert_eq!(rows[0].slot(), BattleTextureSlot::Record0(0));
        assert_eq!(rows[1].section, 1);
        assert_eq!(rows[1].clut_entries, 32);
        assert_eq!(rows[1].clut_x, 64);
        // Rect 0 is 32 halfwords wide, rect 1 is 32 as well - both 128 px.
        assert_eq!((rows[0].width, rows[0].height), (128, 128));

        // The paletteless block decodes through the assembled row.
        let row = assemble_clut_row(&file).expect("assemble row");
        assert_eq!(row.len(), CLUT_ROW_ENTRIES);
        let r = resolve_block(&file, BattleTextureSlot::Record0(0), 0).expect("resolve");
        assert_eq!(r.upload.palette_count(), 0);
        assert!(
            r.upload.rgba(0).is_err(),
            "no block-local palette to decode with"
        );
        let rgba = r
            .upload
            .rgba_with_palette(&row[64..80])
            .expect("borrowed row palette");
        assert_eq!(rgba.len(), 128 * 128 * 4);
    }

    #[test]
    fn record0_blocks_that_do_not_chain_are_rejected() {
        // Grow block 0's palette without moving `clut_b_off`: the chain
        // breaks and neither block may be admitted on block 0's word.
        let mut file = synth_with_record0(0, 32);
        let a_off = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
        let budget = u32::from_le_bytes(file[12..16].try_into().unwrap()) as usize;
        let mut r0 = legaia_lzs::decompress(&file[0x10..], budget).unwrap();
        r0[a_off + 2..a_off + 4].copy_from_slice(&16u16.to_le_bytes());
        let stream = legaia_lzs::compress_optimal(&r0);
        file[0x10..0x10 + stream.len()].copy_from_slice(&stream);
        let mut id = 0u32;
        let rows = build_from_file(865, &file, &mut id);
        assert_eq!(rows.len(), 1, "only the still-chained block 1 survives");
        assert_eq!(rows[0].section, 1);
    }

    #[test]
    fn non_pack_input_contributes_nothing() {
        let mut id = 0u32;
        assert!(build_from_file(0, &vec![0xAAu8; 0x20000], &mut id).is_empty());
        assert!(build_from_file(0, &[], &mut id).is_empty());
    }

    #[test]
    fn resolve_round_trips_the_synthetic_block() {
        let file = synth_player_file(32, 0);
        let r = resolve_block(&file, BattleTextureSlot::Section(0), 1).expect("resolve");
        assert_eq!(r.section, 0);
        assert_eq!(r.record_index, Some(0));
        assert_eq!(r.pool_offset, 0x20);
        assert_eq!(r.upload.palette_count(), 2);
        assert_eq!(r.upload.pixel_width(), 128);
        assert_eq!(r.upload.pixel_height(), 128);
        assert_eq!(r.upload.clut_x, 0x30);
        // Every pixel byte is 0xA5 -> nibbles 5 and 10 alternating.
        let rgba = r.upload.rgba(0).expect("rgba");
        assert_eq!(rgba.len(), 128 * 128 * 4);
        // The block finishes its record, and the write budget is the
        // slot footprint less the dec_size prefix.
        assert_eq!(r.pool_offset + r.upload.block_bytes(), r.decoded.len());
        assert!(r.stream_consumed <= r.stream_capacity);
    }

    #[test]
    fn resolve_reports_an_unflagged_record_as_dead_pool() {
        let mut file = synth_player_file(32, 0);
        // Clear the flag inside the *decoded* record by re-encoding it.
        let pack = battle_data_pack::detect(&file).unwrap();
        let off = pack.records[0].file_offset(pack.data_base);
        let dec_size = u32::from_le_bytes(file[off..off + 4].try_into().unwrap()) as usize;
        let mut decoded = legaia_lzs::decompress(&file[off + 4..], dec_size).unwrap();
        decoded[0x12..0x14].copy_from_slice(&0u16.to_le_bytes());
        let stream = legaia_lzs::compress_optimal(&decoded);
        file[off + 4..off + 4 + stream.len()].copy_from_slice(&stream);
        let err = resolve_block(&file, BattleTextureSlot::Section(0), 0)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not flagged"), "{err}");
    }

    #[test]
    fn slot_selector_parses_both_spellings() {
        use std::str::FromStr;
        assert_eq!(
            BattleTextureSlot::from_str("header0").unwrap(),
            BattleTextureSlot::Record0(0)
        );
        assert_eq!(
            BattleTextureSlot::from_str("HEADER1").unwrap(),
            BattleTextureSlot::Record0(1)
        );
        assert_eq!(
            BattleTextureSlot::from_str(" 14 ").unwrap(),
            BattleTextureSlot::Section(14)
        );
        assert_eq!(BattleTextureSlot::Section(14).to_string(), "14");
        assert_eq!(BattleTextureSlot::Record0(1).to_string(), "header1");
        assert!(BattleTextureSlot::from_str("header2").is_err());
        assert!(BattleTextureSlot::from_str("nope").is_err());
    }

    #[test]
    fn tsv_round_trips_header_and_row() {
        let cat = vec![BattleTextureBlock {
            id: 0,
            entry_index: 864,
            record_index: 14,
            record_id: 0x11,
            section: 2,
            pool_offset: 0x3784,
            width: 128,
            height: 128,
            bpp: 4,
            clut_entries: 32,
            clut_count: 2,
            clut_x: 208,
            byte_len: 8260,
            fnv1a: 0xdead_beef_0000_0001,
            label: "Noa - Ra-Seru Terra $8".into(),
        }];
        let tsv = to_tsv(&cat);
        let header = tsv.lines().next().unwrap();
        assert!(header.starts_with("id\tentry_index\trecord_index\trecord_id\tsection\t"));
        assert!(header.ends_with("\tfnv1a\tlabel"));
        assert!(tsv.contains(
            "0\t864\t14\t17\t2\t14212\t128\t128\t4\t32\t2\t208\t8260\tdeadbeef00000001\t\
             Noa - Ra-Seru Terra $8\n"
        ));
        let r = rollup(&cat);
        assert_eq!(r.count, 1);
        assert_ne!(r.digest, 0);
    }
}
