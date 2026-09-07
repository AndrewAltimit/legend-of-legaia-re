//! Disc-gated regression test for extraction entry **0892** (`card_data`) -
//! the memory-card screen's kanji font pack.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The entry being read as a DATA_FIELD stream again. `FUN_8002574C`
//!   (`0x8002581C..0x80025850`) walks it as an [`legaia_asset::pack`]:
//!   `count = *buf`, member `i` at `word_offsets[i] * 4`. The
//!   `data_field_truncated` detector still fires on it, so the byte-accounting
//!   walker is selected by **index**, and this pins that selection.
//! - The two members ceasing to be whole TIMs at the VRAM addresses the
//!   `MoveImage` park/restore pair in the same routine moves - CLUT `(0, 475)`
//!   and pages `(320, 256)` / `(384, 256)`, which tile
//!   `(320..447, 256..511)` exactly.
//! - The CLUT block ceasing to be a **bit-plane selector bank**. Rows `0..3`
//!   select bit 0..3 of the 4bpp index against one ink and rows `4..7` do the
//!   same against a second ink; that is what makes the page a 1bpp font packed
//!   four planes deep rather than a 16-colour image.
//! - The glyph grid drifting off the 12-pixel pitch, or the inked-cell count
//!   drifting off 2965 - the JIS X 0208 level-1 kanji count, which is the
//!   whole identification.
//!
//! Format: `docs/formats/data-field.md` § "Entry 0892 (`card_data`) is a pack".

use legaia_asset::byte_account::{self, AccountOptions, Walker};
use std::path::{Path, PathBuf};

/// Extraction entry of the card-screen font pack (raw TOC `0x37E`, minus the
/// +2 CDNAME numbering skew).
const CARD_FONT_ENTRY: u32 = 892;
/// Byte length of one member TIM (header + 524-byte CLUT + 32,780-byte image).
const MEMBER_BYTES: usize = 0x8220;
/// Cell pitch in pixels, both axes.
const CELL_PITCH: usize = 12;
/// Inked box inside a cell; the 12th column and row are always blank.
const CELL_INK: usize = 11;
/// Cells per plane row / column.
const CELLS_PER_AXIS: usize = 20;
/// JIS X 0208 level-1 kanji count.
const LEVEL1_KANJI: usize = 2965;

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT").is_dir())
}

fn entry_bytes(root: &Path, index: u32) -> Option<Vec<u8>> {
    let dir = root.join("PROT");
    let prefix = format!("{index:04}_");
    let name = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with(&prefix))?;
    std::fs::read(dir.join(name)).ok()
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().expect("u16"))
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().expect("u32"))
}

/// One member's `(clut_rect, image_rect, palette, 4bpp pixels, row stride)`.
struct MemberTim {
    clut: (u16, u16, u16, u16),
    image: (u16, u16, u16, u16),
    palette: Vec<u16>,
    pixels: Vec<u8>,
    row_bytes: usize,
}

fn parse_member(buf: &[u8], at: usize) -> MemberTim {
    assert_eq!(u32_at(buf, at), 0x10, "TIM magic at member start");
    assert_eq!(u32_at(buf, at + 4), 8, "4bpp + CLUT flags");
    let clut_len = u32_at(buf, at + 8) as usize;
    let clut = (
        u16_at(buf, at + 12),
        u16_at(buf, at + 14),
        u16_at(buf, at + 16),
        u16_at(buf, at + 18),
    );
    let entries = clut.2 as usize * clut.3 as usize;
    let palette: Vec<u16> = (0..entries).map(|i| u16_at(buf, at + 20 + i * 2)).collect();
    let img = at + 8 + clut_len;
    let img_len = u32_at(buf, img) as usize;
    let image = (
        u16_at(buf, img + 4),
        u16_at(buf, img + 6),
        u16_at(buf, img + 8),
        u16_at(buf, img + 10),
    );
    let row_bytes = image.2 as usize * 2;
    MemberTim {
        clut,
        image,
        palette,
        pixels: buf[img + 12..img + img_len].to_vec(),
        row_bytes,
    }
}

impl MemberTim {
    fn total_bytes(&self) -> usize {
        // header + clut block + image block, all fixed for this pack.
        8 + (12 + self.clut.2 as usize * self.clut.3 as usize * 2)
            + (12 + self.image.2 as usize * self.image.3 as usize * 2)
    }

    fn index_at(&self, x: usize, y: usize) -> u8 {
        let b = self.pixels[y * self.row_bytes + x / 2];
        if x.is_multiple_of(2) { b & 0xF } else { b >> 4 }
    }

    /// Inked cells in one bit-plane, and whether the plane's blank gutters
    /// hold (column and row `CELL_INK` of every cell empty).
    fn plane_cells(&self, plane: u8) -> (usize, bool) {
        let width = self.image.2 as usize * 4;
        let height = self.image.3 as usize;
        let mut inked = 0usize;
        let mut gutters_clean = true;
        for cy in 0..CELLS_PER_AXIS {
            for cx in 0..CELLS_PER_AXIS {
                let mut any = false;
                for dy in 0..CELL_PITCH {
                    for dx in 0..CELL_PITCH {
                        let (x, y) = (cx * CELL_PITCH + dx, cy * CELL_PITCH + dy);
                        assert!(x < width && y < height, "cell grid inside the page");
                        let bit = (self.index_at(x, y) >> plane) & 1 == 1;
                        if dx >= CELL_INK || dy >= CELL_INK {
                            gutters_clean &= !bit;
                        } else {
                            any |= bit;
                        }
                    }
                }
                if any {
                    inked += 1;
                }
            }
        }
        (inked, gutters_clean)
    }
}

#[test]
fn card_font_pack_is_two_kanji_tims_or_skips() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some(buf) = entry_bytes(&root, CARD_FONT_ENTRY) else {
        eprintln!("[skip] extraction entry 0892 missing");
        return;
    };

    // The runtime's own walk: count then word offsets (FUN_8002574C).
    let entries = legaia_asset::pack::parse_pack(&buf).expect("0892 parses as asset::pack");
    assert_eq!(entries.len(), 2, "two members");
    assert_eq!(entries[0].byte_offset, 0x0C, "member 0 at word offset 3");
    assert_eq!(
        entries[1].byte_offset, 0x822C,
        "member 1 at word offset 0x208B"
    );

    let m0 = parse_member(&buf, entries[0].byte_offset);
    let m1 = parse_member(&buf, entries[1].byte_offset);
    for m in [&m0, &m1] {
        assert_eq!(m.total_bytes(), MEMBER_BYTES, "member extent");
        assert_eq!(m.clut, (0, 475, 16, 16), "CLUT bank rect");
        assert_eq!(m.image.1, 256, "page row");
        assert_eq!(
            (m.image.2, m.image.3),
            (64, 256),
            "page extent in halfwords"
        );
    }
    assert_eq!(m0.image.0, 320, "member 0 page origin");
    assert_eq!(m1.image.0, 384, "member 1 page origin");
    // The two pages tile the rect FUN_8002574C's MoveImage arm moves.
    assert_eq!(
        m1.image.0 as usize + m1.image.2 as usize - m0.image.0 as usize,
        128,
        "(320,256)..(447,511) = the 128-halfword MoveImage rect"
    );
    assert_eq!(
        m0.palette, m1.palette,
        "the CLUT bank is shared boilerplate"
    );

    // Rows 0..3 select bit 0..3 against one ink; rows 4..7 repeat against a
    // second. That is the 1bpp-packed-four-deep contract.
    let ink_a = m0.palette[1];
    let ink_b = m0.palette[4 * 16 + 1];
    assert_ne!(ink_a, 0, "plane rows carry a non-zero ink");
    assert_ne!(ink_a, ink_b, "the two ink banks differ");
    for plane in 0..4usize {
        for (row, ink) in [(plane, ink_a), (plane + 4, ink_b)] {
            for idx in 0..16usize {
                let want = if (idx >> plane) & 1 == 1 { ink } else { 0 };
                assert_eq!(
                    m0.palette[row * 16 + idx],
                    want,
                    "CLUT row {row} selects bit {plane} of index {idx}"
                );
            }
        }
    }

    // Eight planes of 20x20 cells on a 12-pixel pitch, 2965 inked.
    let mut inked = 0usize;
    for m in [&m0, &m1] {
        for plane in 0..4u8 {
            let (cells, gutters_clean) = m.plane_cells(plane);
            assert!(
                gutters_clean,
                "plane {plane} keeps its 12th column/row blank"
            );
            inked += cells;
        }
    }
    assert_eq!(
        inked, LEVEL1_KANJI,
        "inked cells == the JIS X 0208 level-1 kanji count"
    );

    // Nothing past member 1 belongs to the pack.
    let tail = buf.len() - (entries[1].byte_offset + MEMBER_BYTES);
    assert_eq!(tail, 948, "unreferenced tail slack");
}

#[test]
fn byte_account_picks_the_pack_walker_or_skips() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some(buf) = entry_bytes(&root, CARD_FONT_ENTRY) else {
        eprintln!("[skip] extraction entry 0892 missing");
        return;
    };
    let opts = AccountOptions {
        prot_index: Some(byte_account::CARD_FONT_PROT_INDEX),
        ..Default::default()
    };
    let account = byte_account::account(&buf, &opts);
    assert_eq!(
        account.walker.name(),
        Walker::CardFontPack.name(),
        "index override beats the data_field_truncated class"
    );
    // Two whole TIMs plus the 12-byte pack header; the 948-byte tail stays
    // residue rather than being swallowed by the last member.
    assert_eq!(account.accounted, 2 * MEMBER_BYTES + 12);
    assert_eq!(buf.len() - account.accounted, 948);
}
