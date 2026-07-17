//! `font-extract` - produce `extracted/font/` artifacts (atlas PNG + widths CSV +
//! metadata JSON + raw 4bpp tile-page sheet) from a Legaia disc, with the glyph
//! pixels sourced either straight from the disc (`--disc`) or from a mednafen
//! save state with the dialog font live in VRAM (`--save`).
//!
//! See `docs/formats/dialog-font.md` for the format spec. The extractor reads:
//!
//! 1. `SCUS_942.54` for the static width table at RAM `0x80073F1C` and the
//!    escape table at `0x80074050` (PSX-EXE header gives the file -> RAM offset).
//! 2. Glyph bitmaps + CLUT from one of:
//!    - `--disc <bin-or-PROT.DAT>`: the 4bpp font TIM carried inside `PROT.DAT`
//!      at file offset [`legaia_font::FONT_TIM_PROT_DAT_OFFSET`] (a raw Mode2/2352
//!      disc image is walked via ISO9660 to find `PROT.DAT` first). No emulator
//!      needed.
//!    - `--save <state>`: a mednafen save state for the live VRAM tile-page at
//!      pixel `(896, 0)` and the dialog CLUT at pixel `(96, 510)`. The save state
//!      is gzipped, has an `MDFNSVST` magic header, and carries VRAM as a variable
//!      named `&GPURAM[0][0]` inside the `GPU` section.
//!
//! Both modes emit the same 5 files. The extractor produces no Sony bytes by
//! itself - the inputs are user-supplied and the outputs land in
//! `extracted/font/` which is gitignored.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use flate2::read::GzDecoder;
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SCUS_LOAD_ADDR_FALLBACK: u32 = 0x8001_0000;
const PSX_EXE_HEADER: u32 = 0x800;
const PSX_EXE_T_ADDR_OFFSET: usize = 0x18;

const WIDTH_TABLE_RAM: u32 = 0x8007_3F1C;
const WIDTH_TABLE_LEN: usize = 256;
const ESCAPE_TABLE_RAM: u32 = 0x8007_4050;
const ESCAPE_ENTRY_COUNT: usize = 38;
const ESCAPE_ENTRY_SIZE: usize = 4;

const VRAM_BYTES: usize = 1024 * 512 * 2; // 1 MB; row stride 2048
const VRAM_ROW_STRIDE: usize = 1024 * 2;

const FONT_VRAM_X16: usize = 896; // 16-bit-pixel column of font tile-page
const FONT_VRAM_Y: usize = 0;
const FONT_VRAM_W16: usize = 64; // 64 16-bit pixels = 256 4bpp pixels wide
const FONT_VRAM_H: usize = 256;

const CLUT_VRAM_X16: usize = 96; // pixel x of dialog CLUT 0
const CLUT_VRAM_Y: usize = 510;
const CLUT_ENTRIES: usize = 16;

const ATLAS_GLYPH_W: u32 = 14;
const ATLAS_GLYPH_H: u32 = 15;
const ATLAS_COLS: u32 = 16;
const ATLAS_ROWS: u32 = 14;
const ATLAS_FIRST_CHAR: u8 = 0x20;

#[derive(Parser, Debug)]
#[command(
    name = "font-extract",
    version,
    about = "Extract the proportional dialog font from SCUS_942.54 + either the disc itself (--disc) or a mednafen save state (--save).",
    long_about = "Extract the proportional dialog font (atlas PNG, tile-page sheet PNG, widths \
CSV, metadata JSON, raw 4bpp page) from SCUS_942.54 plus ONE glyph source:\n\n\
  --disc <bin-or-PROT.DAT>  disc-only mode - reads the font TIM carried inside PROT.DAT \
(a raw .bin disc image or an already-extracted PROT.DAT both work); no emulator needed.\n\
  --save <state>            mednafen save-state mode - reads the live VRAM tile-page. Any \
in-game save state works (the font page is byte-identical across captures); mednafen keeps \
them under ~/.mednafen/mcs/.\n\n\
Both modes write the same 5 files. Default paths (extracted/SCUS_942.54, extracted/font) \
are resolved against the current directory.",
    group(clap::ArgGroup::new("glyph_source").required(true).args(["save", "disc"]))
)]
struct Args {
    /// Path to extracted SCUS_942.54 (default resolves against the current
    /// directory; produced by `legaia-extract` / `disc-extract extract`).
    #[arg(long, default_value = "extracted/SCUS_942.54")]
    scus: PathBuf,
    /// Path to a mednafen save state (.mc0..mc9, typically under
    /// ~/.mednafen/mcs/). The save must have the dialog font live in VRAM -
    /// any in-game capture works, the font tile-page is byte-identical
    /// across captures. Mutually exclusive with --disc.
    #[arg(long)]
    save: Option<PathBuf>,
    /// Disc-only mode: path to a raw Mode2/2352 disc image (.bin) or to an
    /// already-extracted PROT.DAT. The font TIM is read straight off the
    /// disc - no emulator or save state needed. Mutually exclusive with
    /// --save.
    #[arg(long)]
    disc: Option<PathBuf>,
    /// Output directory (default resolves against the current directory).
    #[arg(short, long, default_value = "extracted/font")]
    out: PathBuf,
    /// Print extra diagnostics.
    #[arg(long)]
    verbose: bool,
}

/// The glyph page + CLUT, whatever the source.
struct GlyphPage {
    /// 256×256 row-major 4-bit indices (one byte per pixel).
    indexed: Vec<u8>,
    /// 16-entry BGR555 CLUT used to render the sheet/atlas PNGs.
    clut: [u16; CLUT_ENTRIES],
    /// On-wire 4bpp packed bytes (two pixels per byte, low nibble first),
    /// 32768 bytes.
    raw_4bpp: Vec<u8>,
    /// Human-readable provenance for the metadata JSON.
    source: String,
    /// Per-source explanation of what the recorded CLUT is.
    clut_note: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    create_dir_all(&args.out).with_context(|| format!("create {}", args.out.display()))?;

    // 1. SCUS - width table + escape table.
    let scus_bytes =
        std::fs::read(&args.scus).with_context(|| format!("read {}", args.scus.display()))?;
    let t_addr = parse_psx_exe_t_addr(&scus_bytes).unwrap_or(SCUS_LOAD_ADDR_FALLBACK);
    if args.verbose {
        eprintln!("[scus] t_addr = 0x{t_addr:08X}");
    }

    let widths = read_scus(&scus_bytes, t_addr, WIDTH_TABLE_RAM, WIDTH_TABLE_LEN)
        .context("read width table")?;
    let escape_bytes = read_scus(
        &scus_bytes,
        t_addr,
        ESCAPE_TABLE_RAM,
        ESCAPE_ENTRY_COUNT * ESCAPE_ENTRY_SIZE,
    )
    .context("read escape table")?;
    let escape = parse_escape_table(escape_bytes);

    // 2. Glyph page - from the disc's font TIM or a save state's VRAM.
    let page = match (&args.save, &args.disc) {
        (Some(save), None) => glyph_page_from_save(save, args.verbose)?,
        (None, Some(disc)) => glyph_page_from_disc(disc, &scus_bytes, args.verbose)?,
        // clap's ArgGroup(required, single) makes the other arms unreachable.
        _ => bail!("pass exactly one of --save <state> or --disc <bin-or-PROT.DAT>"),
    };
    let (indexed, clut) = (&page.indexed, &page.clut);
    if args.verbose {
        eprintln!("[clut] ({}):", page.source);
        for (i, c) in clut.iter().enumerate() {
            eprintln!("       [{i:2}] 0x{c:04X}");
        }
    }

    // 3. Write the five artifacts.
    let sheet_rgba = render_indexed_to_rgba(indexed, 256, 256, clut);
    write_png(
        &args.out.join("dialog_font_sheet.png"),
        &sheet_rgba,
        256,
        256,
    )
    .context("write font sheet PNG")?;

    let atlas_rgba = pack_atlas(indexed, clut);
    let atlas_w = ATLAS_COLS * ATLAS_GLYPH_W;
    let atlas_h = ATLAS_ROWS * ATLAS_GLYPH_H;
    write_png(
        &args.out.join("dialog_font_atlas.png"),
        &atlas_rgba,
        atlas_w,
        atlas_h,
    )
    .context("write atlas PNG")?;

    write_widths_csv(&args.out.join("dialog_font_widths.csv"), widths)
        .context("write widths CSV")?;

    write_metadata_json(
        &args.out.join("dialog_font_metadata.json"),
        widths,
        &escape,
        clut,
        &page.source,
        &page.clut_note,
    )
    .context("write metadata JSON")?;

    // Also dump the raw 4bpp page bytes - needed for downstream tooling
    // that searches PROT entries for the on-disc carrier of the font.
    // The bytes are the literal 4bpp packed pixels (two pixels per byte,
    // low nibble first), 32768 bytes total = 256 × 256 / 2.
    let raw_path = args.out.join("dialog_font_vram_4bpp.bin");
    std::fs::write(&raw_path, &page.raw_4bpp)
        .with_context(|| format!("write {}", raw_path.display()))?;

    eprintln!("[ok] wrote 5 files into {}", args.out.display());
    Ok(())
}

/// Save-state mode: pull the glyph page + dialog CLUT out of the live VRAM.
fn glyph_page_from_save(save: &Path, verbose: bool) -> Result<GlyphPage> {
    let vram = read_vram_from_save(save, verbose)
        .with_context(|| format!("read VRAM from {}", save.display()))?;
    let clut = read_clut(&vram, CLUT_VRAM_X16, CLUT_VRAM_Y);
    let indexed = decode_4bpp_tile_page(
        &vram,
        FONT_VRAM_X16,
        FONT_VRAM_Y,
        FONT_VRAM_W16,
        FONT_VRAM_H,
    );
    let raw_4bpp = collect_raw_4bpp(
        &vram,
        FONT_VRAM_X16,
        FONT_VRAM_Y,
        FONT_VRAM_W16,
        FONT_VRAM_H,
    );
    Ok(GlyphPage {
        indexed,
        clut,
        raw_4bpp,
        source: format!("mednafen save-state VRAM dump ({})", save.display()),
        clut_note: format!(
            "16-color BGR555 CLUT at VRAM ({CLUT_VRAM_X16}, {CLUT_VRAM_Y}). Index 0 is BGR555 \
             0x0000 - treated transparent in the renderer; index 1 is foreground white; indices \
             2..5 are anti-aliasing gray ramp; the upper half of the CLUT is unused by the \
             dialog renderer."
        ),
    })
}

/// Disc-only mode: slice the font TIM out of `PROT.DAT` (either given
/// directly or found inside a raw Mode2/2352 disc image), validate it
/// through the shared tested decoder, then unpack page + CLUT locally.
///
/// The TIM's baked CLUT is a mastering placeholder (bright primaries), NOT
/// the runtime dialog CLUT - retail uploads that separately at runtime, so
/// it isn't reachable without a VRAM capture. The PNGs are therefore
/// rendered as the same whitewashed stencil
/// [`legaia_font::Font::from_disc_tim_and_scus`] produces (index 0 ->
/// transparent, index 14 -> the (32,32,32) drop shadow, everything else ->
/// pure white), which is exactly what the engine's atlas loader would
/// reduce any capture to anyway.
fn glyph_page_from_disc(disc: &Path, scus_bytes: &[u8], verbose: bool) -> Result<GlyphPage> {
    let tim = read_font_tim_bytes(disc, verbose)
        .with_context(|| format!("read font TIM from {}", disc.display()))?;
    // Cross-check through the library's tested disc-font path so a wrong
    // slice fails loudly with its diagnostics (bad magic / framebuffer).
    let font = legaia_font::Font::from_disc_tim_and_scus(&tim, scus_bytes)
        .with_context(|| format!("validate font TIM from {}", disc.display()))?;
    let (indexed, _tim_clut, raw_4bpp) = decode_disc_font_tim(&tim)?;
    // Stencil CLUT: index 0 transparent, FONT_SHADOW_INDEX = 14 the
    // (32,32,32) shadow (BGR555 0x1084), every other index pure white.
    let mut clut = [0x7FFFu16; CLUT_ENTRIES];
    clut[0] = 0x0000;
    clut[14] = 0x1084;
    // Self-check: rendering our page through the stencil CLUT must equal
    // the library's tested atlas bake bit-for-bit.
    if pack_atlas(&indexed, &clut) != font.atlas_rgba() {
        bail!("internal error: stencil atlas diverges from legaia_font::Font's bake");
    }
    Ok(GlyphPage {
        indexed,
        clut,
        raw_4bpp,
        source: format!("PROT.DAT font TIM read from disc ({})", disc.display()),
        clut_note: "Whitewashed stencil palette (disc mode): the font TIM's baked CLUT is a \
                    mastering placeholder, and the real runtime dialog CLUT is uploaded at \
                    runtime (VRAM (96, 510)) rather than carried on disc. Index 0 = transparent, \
                    index 14 = the (32,32,32) drop shadow, all other indices = pure white - \
                    identical to what the engine's atlas loader normalises a VRAM capture to."
            .to_string(),
    })
}

/// Get [`legaia_font::FONT_TIM_LEN`] bytes starting at
/// [`legaia_font::FONT_TIM_PROT_DAT_OFFSET`] of `PROT.DAT`, from either an
/// already-extracted PROT.DAT or a raw Mode2/2352 `.bin` disc image
/// (detected by the 12-byte CD sector sync pattern).
fn read_font_tim_bytes(path: &Path, verbose: bool) -> Result<Vec<u8>> {
    const SYNC: [u8; 12] = [
        0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
    ];
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut head = [0u8; 12];
    let n = f.read(&mut head)?;
    if n == 12 && head == SYNC {
        // Raw 2352-byte-sector disc image: walk ISO9660 for PROT.DAT.
        let (lba, size) = find_prot_dat(&mut f)
            .with_context(|| format!("locate PROT.DAT in disc image {}", path.display()))?;
        if verbose {
            eprintln!("[disc] PROT.DAT @ LBA {lba} ({size} bytes)");
        }
        let off = legaia_font::FONT_TIM_PROT_DAT_OFFSET as usize;
        let len = legaia_font::FONT_TIM_LEN;
        if (off + len) as u64 > size as u64 {
            bail!("PROT.DAT too small ({size} bytes) for the font TIM slice");
        }
        let first = off / 2048;
        let last = (off + len - 1) / 2048;
        let mut buf = Vec::with_capacity((last - first + 1) * 2048);
        for s in first..=last {
            buf.extend_from_slice(&read_user_sector(&mut f, lba + s as u32)?);
        }
        let rel = off - first * 2048;
        Ok(buf[rel..rel + len].to_vec())
    } else {
        // Assume an extracted PROT.DAT: the TIM sits at a fixed offset.
        f.seek(SeekFrom::Start(legaia_font::FONT_TIM_PROT_DAT_OFFSET))
            .with_context(|| format!("seek in {}", path.display()))?;
        let mut buf = vec![0u8; legaia_font::FONT_TIM_LEN];
        f.read_exact(&mut buf).with_context(|| {
            format!(
                "{} is neither a raw .bin disc image (no CD sync pattern) nor a PROT.DAT \
                 big enough to hold the font TIM",
                path.display()
            )
        })?;
        Ok(buf)
    }
}

/// Read the 2048-byte user-data payload of Mode2/Form1 sector `lba` from a
/// raw 2352-byte-sector image (sync 12 + header 4 + subheader 8 = 24-byte
/// prefix).
fn read_user_sector(f: &mut File, lba: u32) -> Result<[u8; 2048]> {
    f.seek(SeekFrom::Start(lba as u64 * 2352 + 24))?;
    let mut buf = [0u8; 2048];
    f.read_exact(&mut buf)
        .with_context(|| format!("read sector LBA {lba}"))?;
    Ok(buf)
}

/// Minimal ISO9660 walk: PVD at LBA 16, root directory record at PVD+156,
/// then scan the root directory for `PROT.DAT` (Legaia keeps it at the disc
/// root). Returns `(start_lba, size_bytes)`.
fn find_prot_dat(f: &mut File) -> Result<(u32, u32)> {
    let pvd = read_user_sector(f, 16)?;
    if &pvd[1..6] != b"CD001" {
        bail!("no ISO9660 primary volume descriptor at LBA 16");
    }
    let root = &pvd[156..190];
    let dir_lba = u32::from_le_bytes(root[2..6].try_into().unwrap());
    let dir_size = u32::from_le_bytes(root[10..14].try_into().unwrap());
    let n_sectors = dir_size.div_ceil(2048);
    for s in 0..n_sectors {
        let sec = read_user_sector(f, dir_lba + s)?;
        let mut p = 0usize;
        while p + 33 <= sec.len() {
            let rec_len = sec[p] as usize;
            if rec_len == 0 {
                break; // records never span sectors; rest of sector is pad
            }
            let name_len = sec[p + 32] as usize;
            if p + 33 + name_len <= sec.len() {
                let name = &sec[p + 33..p + 33 + name_len];
                if name == b"PROT.DAT" || name == b"PROT.DAT;1" {
                    let lba = u32::from_le_bytes(sec[p + 2..p + 6].try_into().unwrap());
                    let size = u32::from_le_bytes(sec[p + 10..p + 14].try_into().unwrap());
                    return Ok((lba, size));
                }
            }
            p += rec_len;
        }
    }
    bail!("PROT.DAT not found in the ISO9660 root directory - is this a Legend of Legaia disc?")
}

/// Unpack the font TIM (4bpp, CLUT block + image block) into the 256×256
/// indexed page, its 16-entry CLUT, and the on-wire packed pixel bytes.
/// Layout already validated by [`legaia_font::Font::from_disc_tim_and_scus`].
fn decode_disc_font_tim(tim: &[u8]) -> Result<(Vec<u8>, [u16; CLUT_ENTRIES], Vec<u8>)> {
    let rd_u32 = |o: usize| -> Result<u32> {
        tim.get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| anyhow!("font TIM truncated at 0x{o:X}"))
    };
    if rd_u32(0)? != 0x10 {
        bail!("not a TIM (bad magic)");
    }
    let flags = rd_u32(4)?;
    let mut p = 8usize;
    let mut clut = [0u16; CLUT_ENTRIES];
    if flags & 0x8 != 0 {
        // CLUT block: [u32 block_len][u16 dx][u16 dy][u16 w][u16 h][entries].
        let clut_len = rd_u32(p)? as usize;
        let entries_off = p + 12;
        for (i, slot) in clut.iter_mut().enumerate() {
            let o = entries_off + i * 2;
            *slot = tim
                .get(o..o + 2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| anyhow!("font TIM CLUT truncated at 0x{o:X}"))?;
        }
        p = p
            .checked_add(clut_len)
            .filter(|&e| e <= tim.len())
            .ok_or_else(|| anyhow!("font TIM CLUT block overruns"))?;
    } else {
        bail!("font TIM carries no CLUT block");
    }
    // Image block: [u32 len][u16 dx][u16 dy][u16 w16][u16 h][pixels].
    let pixels_off = p + 12;
    let n_pixel_bytes = FONT_VRAM_W16 * 2 * FONT_VRAM_H; // 32768
    let raw_4bpp = tim
        .get(pixels_off..pixels_off + n_pixel_bytes)
        .ok_or_else(|| anyhow!("font TIM pixel data truncated"))?
        .to_vec();
    let width = FONT_VRAM_W16 * 4;
    let mut indexed = vec![0u8; width * FONT_VRAM_H];
    for (i, &b) in raw_4bpp.iter().enumerate() {
        indexed[i * 2] = b & 0x0F;
        indexed[i * 2 + 1] = (b >> 4) & 0x0F;
    }
    Ok((indexed, clut, raw_4bpp))
}

/// Pack the live VRAM tile-page back to its on-wire 4bpp bytes (so
/// downstream tooling can hash / search PROT for the carrier).
fn collect_raw_4bpp(
    vram: &[u8],
    fb_x16: usize,
    fb_y: usize,
    width_16bit: usize,
    height: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(width_16bit * 2 * height);
    for y in 0..height {
        let row_off = (fb_y + y) * VRAM_ROW_STRIDE + fb_x16 * 2;
        out.extend_from_slice(&vram[row_off..row_off + width_16bit * 2]);
    }
    out
}

// ----- SCUS parsing -----

fn parse_psx_exe_t_addr(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 0x40 || &bytes[0..8] != b"PS-X EXE" {
        return None;
    }
    Some(u32::from_le_bytes(
        bytes[PSX_EXE_T_ADDR_OFFSET..PSX_EXE_T_ADDR_OFFSET + 4]
            .try_into()
            .ok()?,
    ))
}

fn read_scus(scus: &[u8], t_addr: u32, ram_addr: u32, len: usize) -> Result<&[u8]> {
    let ram_off = ram_addr
        .checked_sub(t_addr)
        .ok_or_else(|| anyhow!("RAM 0x{ram_addr:08X} below t_addr 0x{t_addr:08X}"))?;
    // `t_addr` comes from the (attacker-controllable) PSX-EXE header, so the
    // offset arithmetic must be checked: a junk t_addr could otherwise make
    // `ram_off + PSX_EXE_HEADER` (or the `+ len` below) overflow.
    let file_off = ram_off
        .checked_add(PSX_EXE_HEADER)
        .map(|v| v as usize)
        .ok_or_else(|| anyhow!("RAM 0x{ram_addr:08X} offset overflows file address"))?;
    if file_off.checked_add(len).is_none_or(|end| end > scus.len()) {
        bail!(
            "RAM 0x{ram_addr:08X} → file 0x{file_off:X}+{len} past SCUS end 0x{:X}",
            scus.len()
        );
    }
    Ok(&scus[file_off..file_off + len])
}

#[derive(Clone, Copy, Debug)]
struct EscapeEntry {
    string_id: i16,
    advance_px: u8,
    y_offset: i8,
}

fn parse_escape_table(bytes: &[u8]) -> Vec<EscapeEntry> {
    bytes
        .chunks_exact(ESCAPE_ENTRY_SIZE)
        .map(|c| EscapeEntry {
            string_id: i16::from_le_bytes([c[0], c[1]]),
            advance_px: c[2],
            y_offset: c[3] as i8,
        })
        .collect()
}

// ----- Save state parsing -----

/// Decompress the save state and pull out the 1 MB VRAM payload.
fn read_vram_from_save(path: &std::path::Path, verbose: bool) -> Result<Vec<u8>> {
    let raw = std::fs::read(path)?;
    let state = if raw.len() >= 2 && raw[0] == 0x1F && raw[1] == 0x8B {
        let mut decoded = Vec::with_capacity(raw.len() * 4);
        let mut gz = GzDecoder::new(&raw[..]);
        gz.read_to_end(&mut decoded)?;
        decoded
    } else {
        raw
    };
    if state.len() < 8 || &state[..8] != b"MDFNSVST" {
        bail!("not a mednafen save state (magic bytes missing)");
    }
    if verbose {
        eprintln!("[save] decompressed: {} bytes", state.len());
    }

    // The VRAM lives in a variable named "&GPURAM[0][0]" inside the GPU
    // section. The format of each variable record is:
    //   u8 name_len; bytes[name_len] name; u32 data_size; bytes[data_size] data;
    // Search for the literal name with its length prefix - robust across the
    // sections that wrap it.
    let needle = b"\x0d&GPURAM[0][0]";
    let pos = find_subsequence(&state, needle).ok_or_else(|| {
        anyhow!("no `&GPURAM[0][0]` variable in save state - wrong game/version?")
    })?;
    let size_off = pos + needle.len();
    if size_off + 4 > state.len() {
        bail!("save state truncated at `&GPURAM` size");
    }
    let size = u32::from_le_bytes(state[size_off..size_off + 4].try_into().unwrap()) as usize;
    if size != VRAM_BYTES {
        bail!(
            "VRAM size {size} bytes but expected {VRAM_BYTES} - save state from a different core?"
        );
    }
    let data_off = size_off + 4;
    if data_off + size > state.len() {
        bail!("save state truncated inside VRAM payload");
    }
    if verbose {
        eprintln!("[save] VRAM @ 0x{data_off:X} ({size} bytes)");
    }
    Ok(state[data_off..data_off + size].to_vec())
}

fn find_subsequence(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

// ----- VRAM decode -----

/// Read a 16-entry CLUT at 16-bit-pixel `(x, y)`.
fn read_clut(vram: &[u8], x: usize, y: usize) -> [u16; CLUT_ENTRIES] {
    let mut out = [0u16; CLUT_ENTRIES];
    let row = y * VRAM_ROW_STRIDE;
    for (i, slot) in out.iter_mut().enumerate() {
        let off = row + (x + i) * 2;
        *slot = u16::from_le_bytes([vram[off], vram[off + 1]]);
    }
    out
}

/// Decode a 4bpp tile-page region. Returns row-major 8-bit indices,
/// `width_16bit_pixels * 4` wide × `height` tall.
fn decode_4bpp_tile_page(vram: &[u8], x16: usize, y: usize, w16: usize, h: usize) -> Vec<u8> {
    let pixel_w = w16 * 4;
    let mut out = vec![0u8; pixel_w * h];
    for row in 0..h {
        let row_off = (y + row) * VRAM_ROW_STRIDE + x16 * 2;
        for col_byte in 0..(w16 * 2) {
            let b = vram[row_off + col_byte];
            out[row * pixel_w + col_byte * 2] = b & 0x0F;
            out[row * pixel_w + col_byte * 2 + 1] = (b >> 4) & 0x0F;
        }
    }
    out
}

/// Convert a BGR555 PSX pixel to 8-bit-per-channel RGB. PSX bit 15 doesn't
/// directly map to alpha; we treat raw value `0x0000` as transparent (the
/// dialog renderer does this) and everything else as opaque.
fn bgr555_to_rgba8(c: u16) -> [u8; 4] {
    if c == 0x0000 {
        return [0, 0, 0, 0];
    }
    let r = ((c & 0x1F) as u32 * 255 / 31) as u8;
    let g = (((c >> 5) & 0x1F) as u32 * 255 / 31) as u8;
    let b = (((c >> 10) & 0x1F) as u32 * 255 / 31) as u8;
    [r, g, b, 0xFF]
}

/// Render an 8-bit indexed bitmap with a 16-color CLUT to packed RGBA8.
fn render_indexed_to_rgba(indexed: &[u8], w: u32, h: u32, clut: &[u16; CLUT_ENTRIES]) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for &idx in indexed {
        let px = bgr555_to_rgba8(clut[idx as usize & 0x0F]);
        out.extend_from_slice(&px);
    }
    out
}

/// Pack 14×15 glyphs out of the 16×16-cell tile-page into a tight
/// 16-col × 14-row atlas (224 × 210 px).
fn pack_atlas(indexed: &[u8], clut: &[u16; CLUT_ENTRIES]) -> Vec<u8> {
    let atlas_w = (ATLAS_COLS * ATLAS_GLYPH_W) as usize;
    let atlas_h = (ATLAS_ROWS * ATLAS_GLYPH_H) as usize;
    let mut atlas = vec![0u8; atlas_w * atlas_h * 4];

    for c in ATLAS_FIRST_CHAR..=0xFFu8 {
        let col = (c & 0x0F) as u32;
        let row = ((c as u32) - ATLAS_FIRST_CHAR as u32) >> 4;
        if row >= ATLAS_ROWS {
            break;
        }
        // Source cell origin in the 256x256 tile-page is (col*16, V) where
        // V = (c & 0xF0) - 0x20. Cells are 16x16 with the drawn 14x15 in the
        // top-left corner.
        let src_x = (col as usize) * 16;
        let src_y = ((c as usize) & 0xF0) - 0x20;
        let dst_x = (col as usize) * (ATLAS_GLYPH_W as usize);
        let dst_y = (row as usize) * (ATLAS_GLYPH_H as usize);
        for y in 0..(ATLAS_GLYPH_H as usize) {
            for x in 0..(ATLAS_GLYPH_W as usize) {
                let idx = indexed[(src_y + y) * 256 + (src_x + x)] as usize & 0x0F;
                let px = bgr555_to_rgba8(clut[idx]);
                let p = ((dst_y + y) * atlas_w + (dst_x + x)) * 4;
                atlas[p..p + 4].copy_from_slice(&px);
            }
        }
    }
    atlas
}

// ----- File output -----

fn write_png(path: &std::path::Path, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    let f = BufWriter::new(File::create(path)?);
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgba)?;
    Ok(())
}

fn write_widths_csv(path: &std::path::Path, widths: &[u8]) -> Result<()> {
    let mut f = BufWriter::new(File::create(path)?);
    writeln!(f, "char_hex,char_dec,char_repr,width_px")?;
    for (i, &w) in widths.iter().enumerate() {
        let c = i as u8;
        let repr = if c.is_ascii_graphic() {
            format!("\"{}\"", c as char)
        } else {
            format!("\"\\x{c:02X}\"")
        };
        writeln!(f, "0x{c:02X},{c},{repr},{w}")?;
    }
    Ok(())
}

fn write_metadata_json(
    path: &std::path::Path,
    widths: &[u8],
    escape: &[EscapeEntry],
    clut: &[u16; CLUT_ENTRIES],
    source: &str,
    clut_note: &str,
) -> Result<()> {
    use serde_json::{Value, json};

    let escape_json: Vec<Value> = escape
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let kind = if e.string_id == 0 {
                "variable"
            } else {
                "string"
            };
            json!({
                "idx": i,
                "string_id": e.string_id,
                "kind": kind,
                "advance_px": e.advance_px,
                "y_offset": e.y_offset,
            })
        })
        .collect();

    let palette: Vec<String> = clut.iter().map(|c| format!("0x{c:04X}")).collect();
    let widths_json: Vec<u8> = widths.to_vec();

    let v = json!({
        "format": "legend-of-legaia-dialog-font",
        "version": 1,
        "description": format!(
            "Proportional dialog font, extracted from Legend of Legaia (NA, SCUS_942.54). \
             Width table + escape table come from the SCUS executable; glyph pixel data \
             and the CLUT come from: {source}."
        ),
        "glyph_source": source,
        "vram_source": {
            "x_pixels_16bit": FONT_VRAM_X16,
            "y_pixels": FONT_VRAM_Y,
            "width_16bit_pixels": FONT_VRAM_W16,
            "height_pixels": FONT_VRAM_H,
            "pixel_format": "4bpp_indexed",
            "tpage_4bpp_x": FONT_VRAM_X16 / 64,
            "tpage_4bpp_y": FONT_VRAM_Y,
            "note": "Font lives in VRAM tile-page 14 row 0 (4bpp). 64 VRAM 16-bit pixels wide x 256 tall = 256x256 source 4bpp pixels."
        },
        "clut": {
            "vram_x_pixels_16bit": CLUT_VRAM_X16,
            "vram_y_pixels": CLUT_VRAM_Y,
            "colors": CLUT_ENTRIES,
            "index_for_dialog": 0,
            "note": clut_note,
            "palette_bgr555": palette
        },
        "glyph_layout": {
            "cell_width_px": 16,
            "cell_height_px": 16,
            "drawn_width_px": ATLAS_GLYPH_W,
            "drawn_height_px": ATLAS_GLYPH_H,
            "columns": ATLAS_COLS,
            "rows": ATLAS_ROWS,
            "first_char": ATLAS_FIRST_CHAR,
            "last_char": 0xFF,
            "u_formula": "(char & 0x0F) * 16",
            "v_formula": "(char & 0xF0) - 0x20"
        },
        "widths": widths_json,
        "escape_table": {
            "ram_address": format!("0x{ESCAPE_TABLE_RAM:08X}"),
            "entries": escape_json
        },
        "rendering_pipeline": {
            "dialog_renderer": "FUN_80036888",
            "wrapper_with_word_wrap": "FUN_8003CC98",
            "preprocessor": "FUN_80036514",
            "gpu_primitive": "GP0 0x64 (variable-size textured rectangle)",
            "newline_byte": "0x7C",
            "color_change_byte": "0xCF (operand: u8 clut_index)",
            "escape_byte": "0xCE (operand: u8 escape_idx)",
            "string_terminator": "0x00"
        }
    });

    let mut f = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut f, &v)?;
    f.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_psx_exe_extracts_t_addr() {
        let mut buf = vec![0u8; 0x100];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&0x8001_0000u32.to_le_bytes());
        assert_eq!(parse_psx_exe_t_addr(&buf), Some(0x8001_0000));
    }

    #[test]
    fn read_scus_catches_out_of_range() {
        let mut buf = vec![0u8; 0x1000];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&0x8001_0000u32.to_le_bytes());
        assert!(read_scus(&buf, 0x8001_0000, 0x8000_0000, 4).is_err());
    }

    #[test]
    fn parse_escape_table_reads_38_entries() {
        let mut bytes = Vec::with_capacity(ESCAPE_ENTRY_COUNT * ESCAPE_ENTRY_SIZE);
        for i in 0..ESCAPE_ENTRY_COUNT {
            let id = (i as i16).wrapping_mul(2);
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.push(12);
            bytes.push((-2i8) as u8);
        }
        let entries = parse_escape_table(&bytes);
        assert_eq!(entries.len(), ESCAPE_ENTRY_COUNT);
        assert_eq!(entries[5].advance_px, 12);
        assert_eq!(entries[5].y_offset, -2);
        assert_eq!(entries[5].string_id, 10);
    }

    #[test]
    fn bgr555_zero_is_transparent() {
        assert_eq!(bgr555_to_rgba8(0x0000), [0, 0, 0, 0]);
    }

    #[test]
    fn bgr555_white_is_opaque_white() {
        let [r, g, b, a] = bgr555_to_rgba8(0x7FFF);
        assert_eq!((r, g, b, a), (255, 255, 255, 255));
    }

    #[test]
    fn read_scus_rejects_t_addr_causing_overflow() {
        // t_addr = 0 with a huge ram_addr makes ram_off near u32::MAX; the
        // checked add must reject it rather than overflow-panic.
        let buf = vec![0u8; 0x1000];
        assert!(read_scus(&buf, 0, 0xFFFF_FFFF, 4).is_err());
        assert!(read_scus(&buf, 0, 0xFFFF_F900, 0x1000).is_err());
    }

    #[test]
    fn parse_psx_exe_rejects_short_or_wrong_magic() {
        assert!(parse_psx_exe_t_addr(&[]).is_none());
        assert!(parse_psx_exe_t_addr(&[0u8; 0x3F]).is_none());
        let mut buf = vec![0u8; 0x40];
        buf[0..8].copy_from_slice(b"NOTANEXE");
        assert!(parse_psx_exe_t_addr(&buf).is_none());
    }

    #[test]
    fn parse_escape_table_ignores_trailing_partial_entry() {
        // A buffer whose length isn't a multiple of 4 must not panic; the
        // partial tail is dropped by chunks_exact.
        let bytes = vec![0u8; ESCAPE_ENTRY_SIZE * 2 + 3];
        let entries = parse_escape_table(&bytes);
        assert_eq!(entries.len(), 2);
        // Empty input yields no entries.
        assert!(parse_escape_table(&[]).is_empty());
    }

    #[test]
    fn read_vram_from_save_rejects_garbage() {
        // Non-mednafen bytes must Err, not panic.
        let dir = std::env::temp_dir().join("legaia-font-extract-fuzz");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("garbage.mc0");
        std::fs::write(&p, b"not a save state at all").unwrap();
        assert!(read_vram_from_save(&p, false).is_err());
        // Has the magic but no GPURAM variable / truncated.
        let p2 = dir.join("magic_only.mc0");
        std::fs::write(&p2, b"MDFNSVST\x00\x00\x00\x00").unwrap();
        assert!(read_vram_from_save(&p2, false).is_err());
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn decode_4bpp_unpacks_low_then_high_nibble() {
        // Build a synthetic VRAM where row 0 has bytes 0xAB, 0xCD at column 0.
        let mut vram = vec![0u8; VRAM_BYTES];
        vram[0] = 0xAB;
        vram[1] = 0xCD;
        let out = decode_4bpp_tile_page(&vram, 0, 0, 1, 1);
        // Width = 1 * 4 = 4 pixels: byte0 → (B, A), byte1 → (D, C).
        assert_eq!(out, vec![0xB, 0xA, 0xD, 0xC]);
    }
}
