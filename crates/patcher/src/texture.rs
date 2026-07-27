//! Texture replacement: swap a TIM on the disc for a user-authored image.
//!
//! Targets come in two tiers, matching the two TIM catalogs in
//! `legaia_asset`:
//!
//! - **raw** - a TIM stored uncompressed (the flat-scan
//!   `legaia_asset::tim_catalog` population: standalone entries, `timpack`
//!   members, and the unindexed system-UI gap before entry 0). Addressed as
//!   `(entry, byte offset within the entry)`, or a flat `PROT.DAT` offset
//!   for gap TIMs. Always replaceable: the encoder preserves dimensions /
//!   bpp / CLUT layout, so the write is same-size in place.
//! - **lzs** - a TIM inside an LZS-compressed section
//!   (`legaia_asset::tim_deep_catalog`). Addressed as `(entry, section,
//!   offset within the decoded section)`. Replaceable only when the edited
//!   section **recompresses into the retail stream's byte footprint**
//!   ([`legaia_lzs::compress_optimal`]); otherwise the replacement fails
//!   with a clear size report rather than corrupting neighbours.
//!
//! Every write goes through [`DiscPatcher`], so touched sectors get their
//! EDC/ECC re-encoded and nothing moves an LBA.

use anyhow::{Context, Result, bail};

use crate::disc::DiscPatcher;
use legaia_tim::encode::{EncodeOptions, Encoded, encode_replacement};
use legaia_tim::{Tim, parse_strict};

/// Where the target TIM lives on the disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureTarget {
    /// Owning PROT entry, or `None` for a TIM in the unindexed gap before
    /// entry 0 (then [`Self::offset`] is a flat `PROT.DAT` byte offset).
    pub entry: Option<u32>,
    /// LZS section index for the compressed tier; `None` = raw tier.
    pub lzs_section: Option<u32>,
    /// Byte offset of the TIM magic: within the entry (raw), within the
    /// decoded section (lzs), or flat into `PROT.DAT` (gap).
    pub offset: u64,
}

impl std::fmt::Display for TextureTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.entry, self.lzs_section) {
            (Some(e), Some(s)) => {
                write!(f, "entry {e} lzs-section {s} +0x{:X}", self.offset)
            }
            (Some(e), None) => write!(f, "entry {e} +0x{:X}", self.offset),
            (None, _) => write!(f, "PROT.DAT gap +0x{:X}", self.offset),
        }
    }
}

/// Upper bound on the window read for a raw target: the largest retail TIM is
/// well under 1 MiB, so 4 MiB always covers a valid target.
const RAW_WINDOW: usize = 4 << 20;

/// The resolved original texture: the parsed TIM plus how it was reached
/// (enough context to write a replacement back).
#[derive(Debug)]
pub struct OriginalTexture {
    pub tim: Tim,
    /// The raw TIM bytes as stored (raw tier) / as decoded (lzs tier).
    pub tim_bytes: Vec<u8>,
    kind: TargetKind,
}

#[derive(Debug)]
enum TargetKind {
    /// Patch via `patch_prot_entry(entry, offset, ..)`.
    RawEntry { entry: usize, offset: u64 },
    /// Patch via `patch_named_file("PROT.DAT", offset, ..)` (gap TIM).
    RawGap { offset: u64 },
    /// Patch the recompressed section stream back into the entry.
    Lzs {
        entry: usize,
        /// Section stream's byte offset within the entry.
        section_offset: u64,
        /// Bytes the retail compressed stream occupies (the fit budget).
        consumed: usize,
        /// The whole decoded section (the TIM is spliced into it).
        decoded: Vec<u8>,
        /// TIM offset within `decoded`.
        offset: usize,
    },
}

/// Locate and strictly parse the target TIM on the current (possibly already
/// patched) image.
pub fn read_texture(patcher: &DiscPatcher, target: &TextureTarget) -> Result<OriginalTexture> {
    match (target.entry, target.lzs_section) {
        (Some(entry), None) => {
            let entry = entry as usize;
            let bytes = patcher
                .read_entry(entry)
                .with_context(|| format!("read PROT entry {entry}"))?;
            let off = target.offset as usize;
            if off >= bytes.len() {
                bail!(
                    "offset 0x{off:X} past PROT entry {entry} ({} bytes)",
                    bytes.len()
                );
            }
            let tim = parse_strict(&bytes[off..])
                .with_context(|| format!("no strict-valid TIM at {}", target))?;
            let extent = tim.byte_extent();
            Ok(OriginalTexture {
                tim_bytes: bytes[off..off + extent].to_vec(),
                tim,
                kind: TargetKind::RawEntry {
                    entry,
                    offset: target.offset,
                },
            })
        }
        (None, None) => {
            let bytes = patcher
                .read_prot_bytes(target.offset, RAW_WINDOW)
                .context("read PROT.DAT window")?;
            let tim = parse_strict(&bytes)
                .with_context(|| format!("no strict-valid TIM at {}", target))?;
            let extent = tim.byte_extent();
            Ok(OriginalTexture {
                tim_bytes: bytes[..extent].to_vec(),
                tim,
                kind: TargetKind::RawGap {
                    offset: target.offset,
                },
            })
        }
        (Some(entry), Some(section)) => {
            let entry = entry as usize;
            let bytes = patcher
                .read_entry(entry)
                .with_context(|| format!("read PROT entry {entry}"))?;
            let container = legaia_lzs::parse_container(&bytes)
                .with_context(|| format!("PROT entry {entry} is not an LZS container"))?;
            let sec = container.sections.get(section as usize).with_context(|| {
                format!(
                    "entry {entry} has {} LZS section(s), no section {section}",
                    container.sections.len()
                )
            })?;
            let stream = &bytes[sec.byte_offset as usize..];
            let (decoded, consumed) = legaia_lzs::decompress_tracked(stream, sec.size as usize)
                .with_context(|| format!("decompress {}", target))?;
            let off = target.offset as usize;
            if off >= decoded.len() {
                bail!(
                    "offset 0x{off:X} past decoded section ({} bytes)",
                    decoded.len()
                );
            }
            let tim = parse_strict(&decoded[off..])
                .with_context(|| format!("no strict-valid TIM at {}", target))?;
            let extent = tim.byte_extent();
            Ok(OriginalTexture {
                tim_bytes: decoded[off..off + extent].to_vec(),
                tim,
                kind: TargetKind::Lzs {
                    entry,
                    section_offset: sec.byte_offset as u64,
                    consumed,
                    decoded,
                    offset: off,
                },
            })
        }
        (None, Some(_)) => bail!("an LZS section needs an owning --entry"),
    }
}

/// Compressed-tier fit numbers (only present for LZS targets).
#[derive(Debug, Clone, Copy)]
pub struct LzsFit {
    /// Bytes the retail compressed stream occupies (the budget).
    pub capacity: usize,
    /// Bytes the edited section recompressed to.
    pub recompressed: usize,
}

/// What a replacement did (or, for a dry run, would do).
#[derive(Debug, Clone)]
pub struct ReplaceOutcome {
    pub width: usize,
    pub height: usize,
    pub bpp: u32,
    pub clut_count: usize,
    pub byte_len: usize,
    /// Palette slots overwritten with new colors (indexed modes).
    pub new_palette_entries: usize,
    /// Pixels folded to a nearest color (quantize mode only).
    pub quantized_pixels: usize,
    /// Whether the rebuilt palette was replicated into every CLUT row.
    pub clut_rows_rewritten: bool,
    /// Compressed-tier fit (None for raw targets).
    pub lzs: Option<LzsFit>,
}

fn bpp_of(tim: &Tim) -> u32 {
    match tim.mode {
        legaia_tim::PixelMode::Bpp4 => 4,
        legaia_tim::PixelMode::Bpp8 => 8,
        legaia_tim::PixelMode::Bpp16 => 16,
        legaia_tim::PixelMode::Bpp24 => 24,
        legaia_tim::PixelMode::Mixed => 0,
    }
}

/// Encode `rgba` against the target's original TIM and, unless `dry_run`,
/// write it back through the patcher (same-size raw write, or a
/// recompressed-in-budget section write for the LZS tier).
pub fn replace_texture(
    patcher: &mut DiscPatcher,
    target: &TextureTarget,
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
    dry_run: bool,
) -> Result<ReplaceOutcome> {
    let original = read_texture(patcher, target)?;
    let enc: Encoded = encode_replacement(&original.tim, rgba, width, height, opts)
        .with_context(|| format!("encode replacement for {}", target))?;
    debug_assert_eq!(enc.bytes.len(), original.tim_bytes.len());

    let mut outcome = ReplaceOutcome {
        width,
        height,
        bpp: bpp_of(&original.tim),
        clut_count: original.tim.palette_count(),
        byte_len: enc.bytes.len(),
        new_palette_entries: enc.new_palette_entries,
        quantized_pixels: enc.quantized_pixels,
        clut_rows_rewritten: enc.clut_rows_rewritten,
        lzs: None,
    };

    match original.kind {
        TargetKind::RawEntry { entry, offset } => {
            if !dry_run {
                patcher.patch_prot_entry(entry, offset, &enc.bytes)?;
            }
        }
        TargetKind::RawGap { offset } => {
            if !dry_run {
                patcher.patch_named_file("PROT.DAT", offset, &enc.bytes)?;
            }
        }
        TargetKind::Lzs {
            entry,
            section_offset,
            consumed,
            mut decoded,
            offset,
        } => {
            decoded[offset..offset + enc.bytes.len()].copy_from_slice(&enc.bytes);
            let recompressed = legaia_lzs::compress_optimal(&decoded);
            outcome.lzs = Some(LzsFit {
                capacity: consumed,
                recompressed: recompressed.len(),
            });
            if recompressed.len() > consumed {
                bail!(
                    "edited section recompresses to {} bytes but the retail stream \
                     occupies only {} - this compressed texture does not fit in place \
                     ({} bytes over). Try an image with more repetition / fewer \
                     distinct regions, or pick a raw-tier texture.",
                    recompressed.len(),
                    consumed,
                    recompressed.len() - consumed
                );
            }
            if !dry_run {
                // Zero-fill up to the retail stream's extent so no stale tail
                // bytes survive; bytes past `consumed` (inter-section padding)
                // are left untouched. The decoder stops at the section's
                // declared output size, so the fill is inert either way.
                let mut padded = recompressed;
                padded.resize(consumed, 0);
                patcher.patch_prot_entry(entry, section_offset, &padded)?;
            }
        }
    }
    Ok(outcome)
}

/// Build both TIM catalogs (raw flat-scan tier + LZS deep tier) from the
/// patcher's current image. One `PROT.DAT` payload copy; the returned rows
/// are derived metadata only.
pub fn texture_catalogs(
    patcher: &DiscPatcher,
) -> Result<(
    Vec<legaia_asset::tim_catalog::CatalogTim>,
    Vec<legaia_asset::tim_deep_catalog::DeepCatalogTim>,
)> {
    let prot = patcher
        .read_named_file("PROT.DAT")
        .context("PROT.DAT not found in disc image")?;
    let spans = patcher.entry_spans();
    let raw = legaia_asset::tim_catalog::build_from_spans(&prot, &spans);
    let deep = legaia_asset::tim_deep_catalog::build_from_spans(&prot, &spans);
    Ok((raw, deep))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disc::synth::{synth_disc, synth_prot};
    use legaia_tim::decode_rgba8;

    /// A minimal strict-valid 4bpp TIM: 16-entry CLUT (red/green/black + STP
    /// black), 4x4 pixels.
    fn tim_4bpp() -> Vec<u8> {
        let mut buf = vec![];
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes());
        buf.extend_from_slice(&44u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&479u16.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        let pal: [u16; 16] = [
            0x801F, 0x03E0, 0x8000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        for e in pal {
            buf.extend_from_slice(&e.to_le_bytes());
        }
        buf.extend_from_slice(&20u32.to_le_bytes());
        buf.extend_from_slice(&64u16.to_le_bytes());
        buf.extend_from_slice(&32u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        for _ in 0..4 {
            buf.extend_from_slice(&[0x10, 0x02]); // indices 0,1,2,0
        }
        buf
    }

    #[test]
    fn target_display_names_all_three_shapes() {
        let raw = TextureTarget {
            entry: Some(898),
            lzs_section: None,
            offset: 0x1F00,
        };
        assert_eq!(raw.to_string(), "entry 898 +0x1F00");
        let lzs = TextureTarget {
            entry: Some(12),
            lzs_section: Some(3),
            offset: 0x40,
        };
        assert_eq!(lzs.to_string(), "entry 12 lzs-section 3 +0x40");
        let gap = TextureTarget {
            entry: None,
            lzs_section: None,
            offset: 0x9000,
        };
        assert_eq!(gap.to_string(), "PROT.DAT gap +0x9000");
    }

    #[test]
    fn lzs_section_without_entry_is_rejected() {
        let disc = synth_disc(&synth_prot(b"x"));
        let patcher = DiscPatcher::open(disc).unwrap();
        let target = TextureTarget {
            entry: None,
            lzs_section: Some(0),
            offset: 0,
        };
        let err = read_texture(&patcher, &target).unwrap_err();
        assert!(err.to_string().contains("owning --entry"));
    }

    #[test]
    fn raw_replacement_round_trips_through_a_synthetic_disc() {
        // Entry 1 holds a TIM at +0x20.
        let tim = tim_4bpp();
        let mut entry = vec![0u8; 0x20];
        entry.extend_from_slice(&tim);
        let disc = synth_disc(&synth_prot(&entry));
        let mut patcher = DiscPatcher::open(disc).unwrap();
        let target = TextureTarget {
            entry: Some(1),
            lzs_section: None,
            offset: 0x20,
        };

        // Read finds the TIM and its bytes verbatim.
        let orig = read_texture(&patcher, &target).unwrap();
        assert_eq!(orig.tim_bytes, tim);

        // Repaint one pixel blue and write it back.
        let mut rgba = decode_rgba8(&orig.tim, 0).unwrap();
        rgba[0..4].copy_from_slice(&[0, 0, 255, 255]);
        let outcome = replace_texture(
            &mut patcher,
            &target,
            &rgba,
            4,
            4,
            &EncodeOptions::default(),
            false,
        )
        .unwrap();
        assert_eq!((outcome.width, outcome.height, outcome.bpp), (4, 4, 4));
        assert!(outcome.lzs.is_none());

        // Re-read off the patched image: same placement fields, new pixel.
        let after = read_texture(&patcher, &target).unwrap();
        assert_eq!(after.tim.image.fb_x, 64);
        assert_eq!(after.tim.image.fb_y, 32);
        assert_eq!(after.tim.clut.as_ref().unwrap().fb_y, 479);
        let got = decode_rgba8(&after.tim, 0).unwrap();
        assert_eq!(&got[0..4], &[0, 0, 255, 255]);
        assert_eq!(after.tim_bytes.len(), tim.len());
    }

    #[test]
    fn dry_run_writes_nothing() {
        let tim = tim_4bpp();
        let disc = synth_disc(&synth_prot(&tim));
        let mut patcher = DiscPatcher::open(disc).unwrap();
        let target = TextureTarget {
            entry: Some(1),
            lzs_section: None,
            offset: 0,
        };
        let orig = read_texture(&patcher, &target).unwrap();
        let mut rgba = decode_rgba8(&orig.tim, 0).unwrap();
        rgba[0..4].copy_from_slice(&[0, 0, 255, 255]);
        replace_texture(
            &mut patcher,
            &target,
            &rgba,
            4,
            4,
            &EncodeOptions::default(),
            true,
        )
        .unwrap();
        let after = read_texture(&patcher, &target).unwrap();
        assert_eq!(after.tim_bytes, tim, "dry run must not touch the image");
    }

    /// A 16bpp 8x8 TIM whose pixel words come from `pix(i)`.
    fn tim_16bpp_8x8(pix: impl Fn(u32) -> u16) -> Vec<u8> {
        let mut tim = Vec::new();
        tim.extend_from_slice(&0x10u32.to_le_bytes());
        tim.extend_from_slice(&0x02u32.to_le_bytes()); // 16bpp, no CLUT
        tim.extend_from_slice(&(12u32 + 8 * 8 * 2).to_le_bytes());
        tim.extend_from_slice(&0u16.to_le_bytes());
        tim.extend_from_slice(&0u16.to_le_bytes());
        tim.extend_from_slice(&8u16.to_le_bytes());
        tim.extend_from_slice(&8u16.to_le_bytes());
        for i in 0..64 {
            tim.extend_from_slice(&pix(i).to_le_bytes());
        }
        tim
    }

    /// Wrap `decoded` as a one-section LZS container entry:
    /// `[meta u32 x2][size,off pair][compressed stream @ +0x10]`.
    fn lzs_container_entry(decoded: &[u8]) -> Vec<u8> {
        let stream = legaia_lzs::compress_optimal(decoded);
        let mut entry = Vec::new();
        entry.extend_from_slice(&0u32.to_le_bytes());
        entry.extend_from_slice(&0u32.to_le_bytes());
        entry.extend_from_slice(&(decoded.len() as u32).to_le_bytes());
        entry.extend_from_slice(&16u32.to_le_bytes());
        entry.extend_from_slice(&stream);
        assert!(entry.len() <= 2048, "container must fit the 1-sector entry");
        entry
    }

    #[test]
    fn lzs_replacement_respects_the_fit_budget() {
        // The stored TIM's pixels are noise (a fat compressed stream); the
        // replacement is a flat color, so the re-encoded stream easily fits.
        let noise = |i: u32| {
            (0x1234u32
                .wrapping_add(i.wrapping_mul(0x9E37_79B9))
                .wrapping_shr(7) as u16)
                & 0x7FFF
        };
        let tim = tim_16bpp_8x8(noise);
        let mut decoded = vec![0u8; 0x40];
        decoded.extend_from_slice(&tim);
        decoded.extend(std::iter::repeat_n(0u8, 0x80));
        let disc = synth_disc(&synth_prot(&lzs_container_entry(&decoded)));
        let mut patcher = DiscPatcher::open(disc).unwrap();
        let target = TextureTarget {
            entry: Some(1),
            lzs_section: Some(0),
            offset: 0x40,
        };

        let orig = read_texture(&patcher, &target).unwrap();
        assert_eq!(orig.tim_bytes, tim);

        // Flat blue everywhere.
        let rgba: Vec<u8> = std::iter::repeat_n([0u8, 0, 255, 255], 64)
            .flatten()
            .collect();
        let outcome = replace_texture(
            &mut patcher,
            &target,
            &rgba,
            8,
            8,
            &EncodeOptions::default(),
            false,
        )
        .unwrap();
        let fit = outcome.lzs.expect("lzs tier reports fit numbers");
        assert!(fit.recompressed <= fit.capacity);

        // Re-read through the container: the new pixels are there.
        let after = read_texture(&patcher, &target).unwrap();
        let got = decode_rgba8(&after.tim, 0).unwrap();
        assert_eq!(&got[0..4], &[0, 0, 255, 255]);
        assert_eq!(&got[252..256], &[0, 0, 255, 255]);
    }

    #[test]
    fn lzs_replacement_that_cannot_fit_errors_and_writes_nothing() {
        // The decoded section is *incompressible noise* around a 16bpp TIM,
        // but the stored stream is a compressed run of the same length -
        // patched with noisy pixels the re-encode cannot fit the budget.
        // Build: decoded = zeros + 16bpp TIM (8x8, all-zero pixels) + zeros;
        // stream compresses tiny; then replace pixels with noise -> the TIM
        // region stops compressing and the stream outgrows the budget.
        let tim = tim_16bpp_8x8(|_| 0);
        let mut decoded = vec![0u8; 0x20];
        decoded.extend_from_slice(&tim);
        let disc = synth_disc(&synth_prot(&lzs_container_entry(&decoded)));
        let mut patcher = DiscPatcher::open(disc).unwrap();
        let target = TextureTarget {
            entry: Some(1),
            lzs_section: Some(0),
            offset: 0x20,
        };

        // Pseudo-random noise pixels: 64 distinct colors defeat the LZ matches.
        let mut rgba = Vec::new();
        let mut x = 0x12345678u32;
        for _ in 0..64 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            rgba.extend_from_slice(&[(x >> 8) as u8, (x >> 16) as u8, (x >> 24) as u8, 255]);
        }
        let err = replace_texture(
            &mut patcher,
            &target,
            &rgba,
            8,
            8,
            &EncodeOptions::default(),
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("does not fit in place"),
            "unexpected error: {err}"
        );
        // Nothing was written: the original TIM still reads back.
        let after = read_texture(&patcher, &target).unwrap();
        assert_eq!(after.tim_bytes, tim);
    }
}
