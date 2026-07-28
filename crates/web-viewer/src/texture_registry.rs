//! The texture-family registry behind the browser ROM patcher's replacement
//! grid.
//!
//! ## Why a registry
//!
//! A "texture" on this disc is not one thing. Plain PSX TIMs sit raw inside
//! PROT entries, more of them sit inside LZS-compressed sections, and some
//! pixel families are neither - the save-slot portraits are sixteen
//! row-interleaved tiles of one sheet with a palette each, addressed by slot
//! rather than by byte offset. Every such family needs its own enumeration,
//! its own decode, and its own writer.
//!
//! Expressing that as hand-written branches inside the WASM entry points cost
//! three edits per family (a loop in the scan, an arm in the preview, an arm
//! in the apply), which is why the grid stalled at three families. Here a
//! family is one [`Tier`] value: it declares how to enumerate its rows, how
//! each row decodes to RGBA, and how a row resolves to a write operation.
//! Adding a family is adding a registry entry.
//!
//! ## The scan is a streaming pass, not a collection
//!
//! [`Tier::scan`] pushes each row into a caller-supplied sink instead of
//! returning a `Vec`. That is deliberate: the browser runs this in 32-bit
//! WASM against the whole `PROT.DAT` payload, and a full-size RGBA decode of
//! every texture on the disc held at once would not fit. The sink thumbnails
//! and drops each decode as it arrives, so peak memory is one texture, not
//! all of them. For the same reason [`ScanCtx`] caches exactly one
//! decompressed entry - the compressed tier's rows arrive grouped by entry,
//! so a one-slot cache still decompresses each hosting entry exactly once.
//!
//! ## What is derived and what is not
//!
//! Every field on a [`TexRow`] is derived metadata - coordinates, dimensions,
//! a content fingerprint, a curated label - never pixel bytes. Pixels exist
//! only transiently in the [`Rgba`] the sink consumes and never leave the
//! user's browser.

use std::borrow::Cow;
use std::cell::RefCell;

use legaia_patcher::battle_texture::BattleTextureTarget;
use legaia_patcher::texture::TextureTarget;

/// A VRAM rectangle: `(x, y, width in framebuffer units, height)`.
pub type VramRect = (u16, u16, u16, u16);
/// A VRAM position: `(x, y)`.
pub type VramPoint = (u16, u16);

/// A decoded image handed to the scan sink. Transient: the sink is expected
/// to thumbnail it and drop it.
pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// The coordinates that identify one texture row, and the only thing a change
/// pack stores about *where* a replacement goes.
///
/// `entry < 0` names the unindexed `PROT.DAT` gap that precedes entry 0.
/// `section` is overloaded per family by design - it is the LZS section index
/// on the compressed tier, the slot number on families addressed by slot,
/// `-1` where neither applies, and on the battle-equipment tier a signed
/// selector over *two* slot spaces (see [`battle_slot`]) - because the page,
/// the queue and the pack have carried this triple since the first tier and
/// widening it would invalidate every stored coordinate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct TexCoord {
    pub tier: &'static str,
    pub entry: i64,
    pub section: i64,
    pub offset: u64,
}

/// One row of the replacement grid: what a person needs to decide "is this
/// the texture, and can I change it".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TexRow {
    pub coord: TexCoord,
    pub width: u32,
    pub height: u32,
    /// 4, 8, 16 or 24.
    pub bpp: u32,
    /// CLUT palettes attached to the texture (0 for 16/24bpp).
    pub cluts: usize,
    /// Bytes the texture occupies at rest.
    pub bytes: usize,
    /// What this texture is, in words - the page's whole search vocabulary.
    /// `None` when the family has nothing to say about this row.
    ///
    /// Borrowed for the families whose labels are curated constants (the TIM
    /// tiers key [`legaia_asset::tim_labels`] by content fingerprint, so those
    /// are `&'static str`); owned for a family that *composes* a label per row
    /// from disc data, which the battle-equipment tier does - it joins the
    /// character to the equipment's own item name.
    pub label: Option<Cow<'static, str>>,
    /// FNV-1a-64 over the texture's stored bytes. A hash, never the bytes -
    /// this is what a change pack pins a replacement to, so importing onto a
    /// different disc revision (or onto an already-patched texture) is
    /// detected rather than silently mis-applied.
    pub fnv1a: u64,
    /// Where the pixels land in VRAM, when the family records it.
    pub vram: Option<VramRect>,
    /// Where the palette lands in VRAM, when there is one.
    pub clut_vram: Option<VramPoint>,
}

/// FNV-1a-64, the fingerprint both TIM catalogs already key their labels by.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// How a row is written back. The registry resolves a coordinate to one of
/// these so neither the preview nor the apply path needs to know which
/// families exist.
#[derive(Debug)]
pub enum ReplaceOp {
    /// A standard TIM, written through `legaia_patcher::texture` (same-size
    /// raw write, or recompress-into-budget on the compressed tier).
    Tim(TextureTarget),
    /// A save-slot portrait, written through `legaia_patcher::save_icon` -
    /// the generic TIM path would repaint every portrait, because the sheet
    /// stores one tile as sixteen scattered runs sharing a strip.
    SaveIconSlot(usize),
    /// A headerless battle-equipment block, written through
    /// `legaia_patcher::battle_texture`. Not a TIM write and not a
    /// same-size write: the block is the tail of an LZS record whose slot
    /// allocation is pinned by the descriptor chain, so the whole record is
    /// re-compressed into that allocation or the write is refused.
    BattleEquip(BattleTextureTarget),
}

/// A texture family.
pub struct Tier {
    /// Stable id. Appears in coordinates, in queue keys and in change packs,
    /// so it is part of the persisted format - never rename one.
    pub id: &'static str,
    /// Human name, shown as the family's filter preset.
    pub title: &'static str,
    /// One line of "what this family is", shown next to the preset.
    pub about: &'static str,
    /// Whether a row of this family can be written back in place at all. A
    /// read-only family still lists and exports; it just cannot be queued.
    pub replaceable: bool,
    /// Enumerate every row, decoding to RGBA when `want_pixels`. One pass:
    /// the compressed tier decompresses each hosting entry exactly once.
    pub scan: fn(&ScanCtx<'_>, bool, &mut Sink<'_>) -> Result<(), String>,
    /// Decode one row full-size, straight from the `PROT.DAT` payload.
    ///
    /// The replaceable families are previewed through the patcher instead
    /// (that path also measures the write), so this exists for the read-only
    /// ones - without it a family the grid can list would be a family the
    /// grid cannot show at full size or export.
    pub read: fn(&ScanCtx<'_>, &TexCoord) -> Result<Rgba, String>,
    /// Resolve a coordinate of this family to its write operation.
    pub op: fn(&TexCoord) -> Result<ReplaceOp, String>,
}

/// The scan sink. Returning `Err` aborts the scan (the WASM binding uses this
/// to propagate a JS allocation failure).
pub type Sink<'s> = dyn FnMut(TexRow, Option<Rgba>) -> Result<(), String> + 's;

/// The one-slot decompression cache: `(entry index, sections)`, where `None`
/// sections means that entry is not an LZS container.
type LzsCache = Option<(u32, Option<Vec<Vec<u8>>>)>;

/// Shared read-only scan state plus a one-slot decompression cache.
pub struct ScanCtx<'a> {
    /// The whole `PROT.DAT` payload.
    pub prot: &'a [u8],
    /// Every TOC entry's `(byte_offset, size_bytes, index)`.
    pub spans: &'a [(u64, u64, u32)],
    /// The disc executable's item-name table, when the caller had the
    /// executable to hand. Not derivable from `prot` - it lives in
    /// `SCUS_942.54` - and a family that names its rows after equipment
    /// needs it, so it is scan state rather than a parameter of one family.
    names: Option<legaia_asset::item_names::ItemNameTable>,
    /// Last decompressed entry. Deliberately one slot - see the module docs.
    lzs: RefCell<LzsCache>,
}

impl<'a> ScanCtx<'a> {
    /// A context with no executable, so families that label rows from the
    /// item table fall back to ids. Enough for every decode path - only the
    /// scan produces labels.
    pub fn new(prot: &'a [u8], spans: &'a [(u64, u64, u32)]) -> Self {
        Self {
            prot,
            spans,
            names: None,
            lzs: RefCell::new(None),
        }
    }

    /// A context that can name equipment. `scus` is the raw `SCUS_942.54`
    /// image; an unparseable one is the same as none.
    pub fn with_scus(prot: &'a [u8], spans: &'a [(u64, u64, u32)], scus: Option<&[u8]>) -> Self {
        Self {
            names: scus.and_then(legaia_asset::item_names::ItemNameTable::from_scus),
            ..Self::new(prot, spans)
        }
    }

    /// The item-name table, when one was supplied.
    pub fn item_names(&self) -> Option<&legaia_asset::item_names::ItemNameTable> {
        self.names.as_ref()
    }

    /// The `(byte_offset, size_bytes)` footprint of a TOC entry.
    pub fn span(&self, entry: u32) -> Option<(u64, u64)> {
        self.spans
            .iter()
            .find(|&&(_, _, idx)| idx == entry)
            .map(|&(off, size, _)| (off, size))
    }

    /// The raw bytes of a TOC entry.
    pub fn entry_bytes(&self, entry: u32) -> Option<&'a [u8]> {
        let (off, size) = self.span(entry)?;
        let start = off as usize;
        let stop = (off + size).min(self.prot.len() as u64) as usize;
        self.prot.get(start..stop)
    }

    /// Run `f` over an entry's LZS sections, decompressing it if it is not
    /// already the cached entry. `f` sees `None` when the entry is not an
    /// LZS container.
    pub fn with_lzs_sections<R>(&self, entry: u32, f: impl FnOnce(Option<&[Vec<u8>]>) -> R) -> R {
        let mut slot = self.lzs.borrow_mut();
        if slot.as_ref().map(|(e, _)| *e) != Some(entry) {
            let sections = self
                .entry_bytes(entry)
                .and_then(|b| legaia_lzs::decompress_container(b).ok());
            *slot = Some((entry, sections));
        }
        f(slot.as_ref().and_then(|(_, s)| s.as_deref()))
    }
}

// --- The families -----------------------------------------------------------

/// `tier` id of the raw TIM family.
pub const TIER_RAW: &str = "raw";
/// `tier` id of the LZS-compressed TIM family.
pub const TIER_LZS: &str = "lzs";
/// `tier` id of the save-slot portrait family.
pub const TIER_SAVE_ICON: &str = "save-icon";
/// `tier` id of the summon / readef battle side-band texture pages.
pub const TIER_SUMMON: &str = "summon";
/// `tier` id of the battle-equipment character art (PROT 863..866).
pub const TIER_BATTLE_EQUIP: &str = "battle-equip";

/// Sub-palette used to preview a summon texture page. The page is 4bpp
/// against a 256-entry CLUT row, so a viewer must pick one 16-colour window;
/// retail picks per draw. Window 0 is the preview convention, and the row is
/// labelled so nobody reads the thumbnail as the only colouring.
pub const SUMMON_PREVIEW_CLUT_SUB: usize = 0;

/// Palette used to view, export and re-encode a battle-equipment block.
///
/// Most of these blocks ship two or three 16-colour palettes in one CLUT run
/// and a mesh primitive's CBA column decides which it samples, so there is no
/// single "the" colouring. Palette 0 is the convention across the grid, the
/// exported PNG and the write, which is what makes the exported PNG the thing
/// the write expects back: an edit re-encodes against the palette it was
/// exported through and leaves the sibling palettes untouched.
pub const BATTLE_PREVIEW_PALETTE: usize = 0;

/// Every family the grid offers, in the order rows are emitted. The order is
/// part of the observable scan output (the page pages through it), so new
/// families append.
pub fn tiers() -> &'static [Tier] {
    &[
        Tier {
            id: TIER_RAW,
            title: "Uncompressed TIMs",
            about: "Standard PSX TIMs stored raw in a PROT entry (or the \
                    unindexed gap). Always replaceable in place.",
            replaceable: true,
            scan: scan_raw,
            read: read_tim,
            op: op_tim,
        },
        Tier {
            id: TIER_LZS,
            title: "Compressed TIMs",
            about: "TIMs inside an LZS-compressed section. Replaceable only \
                    when the edit recompresses back into the retail footprint.",
            replaceable: true,
            scan: scan_lzs,
            read: read_tim,
            op: op_tim,
        },
        Tier {
            id: TIER_SAVE_ICON,
            title: "Save-slot portraits",
            about: "The fifteen memory-card icons, tiles of one shared sheet \
                    with a palette each. Addressed by save slot.",
            replaceable: true,
            scan: scan_save_icons,
            read: read_save_icon,
            op: op_save_icon,
        },
        Tier {
            id: TIER_SUMMON,
            title: "Summon / special-attack pages",
            about: "4bpp battle side-band texture pages (PROT 893/894). Not \
                    TIMs, so no TIM scan reaches them. View and export only - \
                    this build has no encoder for the format.",
            replaceable: false,
            scan: scan_summon,
            read: read_summon,
            op: op_read_only,
        },
        Tier {
            id: TIER_BATTLE_EQUIP,
            title: "Battle character art",
            about: "The party's in-battle skins, one block per equipment \
                    variant (PROT 863..866). Headerless, so no TIM scan \
                    reaches them. Replaceable when the edit recompresses \
                    into the record's own slot allocation.",
            replaceable: true,
            scan: scan_battle_equip,
            read: read_battle_equip,
            op: op_battle_equip,
        },
    ]
}

/// The registry entry for a tier id.
pub fn tier(id: &str) -> Option<&'static Tier> {
    tiers().iter().find(|t| t.id == id)
}

/// Resolve any coordinate to its write operation, whichever family it names.
pub fn replace_op(coord: &TexCoord) -> Result<ReplaceOp, String> {
    let t = tier(coord.tier).ok_or_else(|| format!("unknown texture family {:?}", coord.tier))?;
    let resolved = (t.op)(coord);
    // The family's own `op` produces the message, so the reason a write is
    // refused reads in that family's terms. The flag is the backstop: a
    // family that declares itself read-only can never leak a write op, however
    // its `op` is written.
    if !t.replaceable && resolved.is_ok() {
        return Err(format!("{} are read-only", t.title));
    }
    resolved
}

/// Run every family's scan through one sink, in registry order.
pub fn scan_all(ctx: &ScanCtx<'_>, want_pixels: bool, sink: &mut Sink<'_>) -> Result<(), String> {
    for t in tiers() {
        (t.scan)(ctx, want_pixels, sink)?;
    }
    Ok(())
}

/// The VRAM placement a parsed TIM declares.
fn tim_vram(tim: &legaia_tim::Tim) -> (Option<VramRect>, Option<VramPoint>) {
    let img = &tim.image;
    (
        Some((img.fb_x, img.fb_y, img.fb_w, img.h)),
        tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y)),
    )
}

/// Decode a TIM's first palette, or `None` if it does not decode.
fn tim_rgba(tim: &legaia_tim::Tim) -> Option<Rgba> {
    legaia_tim::decode_rgba8(tim, 0).ok().map(|data| Rgba {
        w: tim.pixel_width(),
        h: tim.pixel_height(),
        data,
    })
}

/// Raw tier: the flat catalog, re-validated at its recorded offset.
///
/// A catalog row whose bytes no longer strict-parse emits no grid row - the
/// grid only offers what the writer can round-trip.
fn scan_raw(ctx: &ScanCtx<'_>, want_pixels: bool, sink: &mut Sink<'_>) -> Result<(), String> {
    for t in legaia_asset::tim_catalog::build_from_spans(ctx.prot, ctx.spans) {
        let Some(bytes) = ctx.prot.get(t.abs_offset as usize..) else {
            continue;
        };
        let Ok(tim) = legaia_tim::parse_strict(bytes) else {
            continue;
        };
        let (vram, clut_vram) = tim_vram(&tim);
        let row = TexRow {
            coord: TexCoord {
                tier: TIER_RAW,
                entry: t.entry_index.map(|e| e as i64).unwrap_or(-1),
                section: -1,
                offset: t.offset_in_entry,
            },
            width: t.width,
            height: t.height,
            bpp: t.bpp,
            cluts: t.clut_count,
            bytes: t.byte_len,
            label: t.label.map(Cow::Borrowed),
            fnv1a: t.fnv1a,
            vram,
            clut_vram,
        };
        sink(row, want_pixels.then(|| tim_rgba(&tim)).flatten())?;
    }
    Ok(())
}

/// Compressed tier: the deep catalog, grouped by hosting entry so each entry
/// decompresses once.
///
/// Unlike the raw tier every catalog row emits a grid row even when the
/// decode fails - the coordinate is still valid and the writer still reaches
/// it; only the thumbnail is missing.
fn scan_lzs(ctx: &ScanCtx<'_>, want_pixels: bool, sink: &mut Sink<'_>) -> Result<(), String> {
    for t in legaia_asset::tim_deep_catalog::build_from_spans(ctx.prot, ctx.spans) {
        let (vram, clut_vram, rgba) = ctx.with_lzs_sections(t.entry_index, |sections| {
            let tim = sections.and_then(|s| {
                s.get(t.lzs_section as usize)
                    .and_then(|sec| sec.get(t.offset_in_section as usize..))
                    .and_then(|at| legaia_tim::parse_strict(at).ok())
            });
            let (vram, clut_vram) = match tim.as_ref() {
                Some(tim) => tim_vram(tim),
                None => (None, None),
            };
            let rgba = if want_pixels {
                tim.as_ref().and_then(tim_rgba)
            } else {
                None
            };
            (vram, clut_vram, rgba)
        });
        let row = TexRow {
            coord: TexCoord {
                tier: TIER_LZS,
                entry: t.entry_index as i64,
                section: t.lzs_section as i64,
                offset: t.offset_in_section,
            },
            width: t.width,
            height: t.height,
            bpp: t.bpp,
            cluts: t.clut_count,
            bytes: t.byte_len,
            label: t.label.map(Cow::Borrowed),
            fnv1a: t.fnv1a,
            vram,
            clut_vram,
        };
        sink(row, rgba)?;
    }
    Ok(())
}

/// Save-slot portraits. Fifteen rows, not sixteen - tile 15 of the strip is
/// blank padding nothing in game selects.
fn scan_save_icons(
    ctx: &ScanCtx<'_>,
    want_pixels: bool,
    sink: &mut Sink<'_>,
) -> Result<(), String> {
    use legaia_asset::save_icon as si;

    let entry = si::PROT_ENTRY as u32;
    let Some(bytes) = ctx.entry_bytes(entry) else {
        return Ok(());
    };
    let Ok(sheet) = si::parse_entry(bytes) else {
        return Ok(());
    };
    for slot in 0..si::USABLE_TILE_COUNT {
        let Ok(rgba) = sheet.tile_rgba(slot) else {
            continue;
        };
        // Fingerprint the tile's own bytes (palette then pixels) so a
        // portrait pins a change pack the same way a TIM does.
        let mut ident = Vec::with_capacity(si::TILE_CLUT_BYTES + si::TILE_BLOCK_BYTES);
        if let Ok(clut) = sheet.tile_clut_bytes(slot) {
            ident.extend_from_slice(&clut);
        }
        if let Ok(px) = sheet.tile_block_pixels(slot) {
            ident.extend_from_slice(&px);
        }
        let (vx, vy, vw, vh) = si::slot_vram_rect(slot);
        let row = TexRow {
            coord: TexCoord {
                tier: TIER_SAVE_ICON,
                entry: entry as i64,
                section: slot as i64,
                offset: sheet.tile_clut_offset(slot) as u64,
            },
            width: si::TILE_SIZE as u32,
            height: si::TILE_SIZE as u32,
            bpp: 4,
            cluts: 1,
            bytes: si::TILE_BLOCK_BYTES + si::TILE_CLUT_BYTES,
            label: Some(Cow::Borrowed("save-slot portrait")),
            fnv1a: fnv1a64(&ident),
            vram: Some((vx, vy, vw, vh)),
            clut_vram: Some((si::CLUT_RECT.0, si::CLUT_RECT.1)),
        };
        sink(
            row,
            want_pixels.then_some(Rgba {
                w: si::TILE_SIZE,
                h: si::TILE_SIZE,
                data: rgba,
            }),
        )?;
    }
    Ok(())
}

/// Both TIM tiers share one writer; the coordinate triple is exactly a
/// [`TextureTarget`].
fn op_tim(c: &TexCoord) -> Result<ReplaceOp, String> {
    Ok(ReplaceOp::Tim(TextureTarget {
        entry: (c.entry >= 0).then_some(c.entry as u32),
        lzs_section: (c.section >= 0).then_some(c.section as u32),
        offset: c.offset,
    }))
}

fn op_save_icon(c: &TexCoord) -> Result<ReplaceOp, String> {
    if c.section < 0 {
        return Err("save-slot portraits are addressed by slot".to_string());
    }
    Ok(ReplaceOp::SaveIconSlot(c.section as usize))
}

/// A family this build can decode but not encode.
fn op_read_only(c: &TexCoord) -> Result<ReplaceOp, String> {
    Err(format!(
        "{} are view-and-export only in this build",
        tier(c.tier).map(|t| t.title).unwrap_or("these textures")
    ))
}

// --- Summon / readef side-band texture pages --------------------------------
//
// The user-visible reason this family exists as its own tier: these pages are
// NOT TIMs. They are `[u32 mode][CLUT rows][4bpp page]` blocks inside a
// `0x10800`-slot side-band file, so every TIM scan - ours and any external
// one - reports zero textures for PROT 893/894, and no search string over a
// TIM catalog can reach them. Listing them is what makes them findable.

/// Byte offsets and geometry of one summon texture page inside its entry.
struct SummonPage {
    /// Byte offset of the slot's CLUT block within the entry.
    clut_offset: usize,
    /// Byte offset of the 4bpp page within the entry.
    page_offset: usize,
    clut_bytes: usize,
    page_bytes: usize,
    width: usize,
    height: usize,
}

/// Locate the page a coordinate names. `offset` is the page's byte offset in
/// the entry, which is what makes this family byte-addressable despite being
/// slot-structured.
fn summon_page(entry_bytes: &[u8], page_offset: usize) -> Option<SummonPage> {
    use legaia_asset::summon_readef as sr;
    let slot_index = page_offset / sr::SLOT_BYTES;
    let file = sr::parse(entry_bytes).ok()?;
    let slot = file.slots.iter().find(|s| s.index == slot_index)?;
    let sr::SlotKind::Texture(t) = &slot.kind else {
        return None;
    };
    let base = slot_index * sr::SLOT_BYTES;
    (base + t.texture_offset == page_offset).then(|| SummonPage {
        clut_offset: base + 4,
        page_offset,
        clut_bytes: t.clut_bytes(),
        page_bytes: t.texture_bytes(),
        width: t.texture_width_halfwords * 4,
        height: 256,
    })
}

/// Decode a 4bpp page against one 16-colour window of its first CLUT row.
fn summon_rgba(entry_bytes: &[u8], p: &SummonPage, clut_sub: usize) -> Option<Rgba> {
    let clut_base = p.clut_offset + clut_sub * 32;
    let pal: Vec<[u8; 4]> = (0..16)
        .map(|i| {
            let o = clut_base + i * 2;
            let w = u16::from_le_bytes([*entry_bytes.get(o)?, *entry_bytes.get(o + 1)?]);
            Some(legaia_tim::bgr555_to_rgba8(w))
        })
        .collect::<Option<_>>()?;
    let page = entry_bytes.get(p.page_offset..p.page_offset + p.page_bytes)?;
    let mut data = vec![0u8; p.width * p.height * 4];
    for (texel, px) in data.chunks_exact_mut(4).enumerate() {
        let byte = *page.get(texel / 2)?;
        let idx = if texel % 2 == 0 {
            byte & 0xF
        } else {
            byte >> 4
        };
        px.copy_from_slice(&pal[idx as usize]);
    }
    Some(Rgba {
        w: p.width,
        h: p.height,
        data,
    })
}

fn scan_summon(ctx: &ScanCtx<'_>, want_pixels: bool, sink: &mut Sink<'_>) -> Result<(), String> {
    use legaia_asset::summon_readef as sr;

    for entry in [sr::SUMMON_PROT_INDEX as u32, sr::READEF_PROT_INDEX as u32] {
        let Some(bytes) = ctx.entry_bytes(entry) else {
            continue;
        };
        let Ok(file) = sr::parse(bytes) else {
            continue;
        };
        for slot in &file.slots {
            let sr::SlotKind::Texture(t) = &slot.kind else {
                continue;
            };
            let page_offset = slot.index * sr::SLOT_BYTES + t.texture_offset;
            let Some(p) = summon_page(bytes, page_offset) else {
                continue;
            };
            // Fingerprint palette + pixels, the same identity shape every
            // other family uses.
            let mut ident = Vec::with_capacity(p.clut_bytes + p.page_bytes);
            ident.extend_from_slice(
                bytes
                    .get(p.clut_offset..p.clut_offset + p.clut_bytes)
                    .unwrap_or_default(),
            );
            ident.extend_from_slice(
                bytes
                    .get(p.page_offset..p.page_offset + p.page_bytes)
                    .unwrap_or_default(),
            );
            let row = TexRow {
                coord: TexCoord {
                    tier: TIER_SUMMON,
                    entry: entry as i64,
                    section: slot.index as i64,
                    offset: page_offset as u64,
                },
                width: p.width as u32,
                height: p.height as u32,
                bpp: 4,
                cluts: t.clut_rows,
                bytes: p.clut_bytes + p.page_bytes,
                label: Some(Cow::Borrowed("summon texture page")),
                fnv1a: fnv1a64(&ident),
                vram: None,
                clut_vram: None,
            };
            let rgba = want_pixels
                .then(|| summon_rgba(bytes, &p, SUMMON_PREVIEW_CLUT_SUB))
                .flatten();
            sink(row, rgba)?;
        }
    }
    Ok(())
}

// --- Battle-equipment character art -----------------------------------------
//
// The family behind "I ripped Terra's armband out of an emulator and cannot
// find it on the disc". A player battle file's character art is
//
//     [u16 clut_x][u16 clut_n][BGR555 run][w*h halfwords of 4bpp]
//
// and that is the entire header: no magic, no flag word, no geometry (the
// rect comes from the loader's static table). So both TIM catalogs report
// zero rows for PROT 863..866 - not because the art hides well but because
// there is no TIM there to find - and before this tier no filter string on
// the page could reach a single block of it.
//
// Labels are composed rather than curated: the descriptor ids are item ids,
// so with the disc's own name table a row reads "Noa - Ra-Seru Terra $8".
// That is the whole point of plumbing `SCUS_942.54` into [`ScanCtx`] - the
// label IS the search vocabulary, and "Noa - equip 0x11" is not searchable
// by anything a person would type.

/// The `section` half of a battle-equipment coordinate.
///
/// A block is either a flagged equipment section's pool, addressed by its
/// descriptor-table index, or one of the two header-resident `record[0]`
/// blocks, which sit outside that table entirely. The two spaces fold into
/// one signed field the way the parser already marks them apart: a
/// descriptor index is itself, and `record[0]` block `n` is `-1 - n`.
/// `offset` stays the block's byte offset inside its decoded record, so the
/// coordinate carries both an addressable slot and a checkable position.
fn battle_section(block: &legaia_asset::battle_texture_catalog::BattleTextureBlock) -> i64 {
    if block.is_record0() {
        -1 - block.section as i64
    } else {
        block.record_index as i64
    }
}

/// The inverse: the slot selector a coordinate's `section` names.
fn battle_slot(
    section: i64,
) -> Result<legaia_asset::battle_texture_catalog::BattleTextureSlot, String> {
    use legaia_asset::battle_texture_catalog::BattleTextureSlot;
    let bad = || format!("{section} is not a battle-equipment slot");
    if section < 0 {
        u8::try_from(-1 - section)
            .map(BattleTextureSlot::Record0)
            .map_err(|_| bad())
    } else {
        usize::try_from(section)
            .map(BattleTextureSlot::Section)
            .map_err(|_| bad())
    }
}

/// The catalog rows of one player file, with labels when the context has an
/// item table. Per-entry rather than whole-disc so a single-row read pays
/// for one file, not four.
fn battle_blocks_of_entry(
    ctx: &ScanCtx<'_>,
    entry: u32,
) -> Vec<legaia_asset::battle_texture_catalog::BattleTextureBlock> {
    use legaia_asset::battle_texture_catalog as btc;
    let Some(file) = ctx.entry_bytes(entry) else {
        return Vec::new();
    };
    let mut id = 0u32;
    btc::build_from_file_with_names(entry, file, &mut id, ctx.item_names())
}

fn battle_rgba(
    ctx: &ScanCtx<'_>,
    block: &legaia_asset::battle_texture_catalog::BattleTextureBlock,
) -> Option<Rgba> {
    use legaia_asset::battle_texture_catalog as btc;
    btc::decode_block(ctx.prot, ctx.spans, block, BATTLE_PREVIEW_PALETTE)
        .ok()
        .map(|d| Rgba {
            w: d.width,
            h: d.height,
            data: d.rgba,
        })
}

fn scan_battle_equip(
    ctx: &ScanCtx<'_>,
    want_pixels: bool,
    sink: &mut Sink<'_>,
) -> Result<(), String> {
    use legaia_asset::battle_texture_catalog as btc;

    for entry in btc::PLAYER_FILE_ENTRIES {
        for b in battle_blocks_of_entry(ctx, entry) {
            let row = TexRow {
                coord: TexCoord {
                    tier: TIER_BATTLE_EQUIP,
                    entry: b.entry_index as i64,
                    section: battle_section(&b),
                    offset: b.pool_offset,
                },
                width: b.width,
                height: b.height,
                bpp: b.bpp,
                cluts: b.clut_count,
                bytes: b.byte_len,
                label: Some(Cow::Owned(b.label.clone())),
                fnv1a: b.fnv1a,
                // Deliberately unreported. Where these pixels land is a
                // function of which party slot the character occupies in a
                // given battle, decided at battle load - so there is no one
                // rect, and naming one would be a claim the bytes do not
                // make.
                vram: None,
                clut_vram: None,
            };
            let rgba = want_pixels.then(|| battle_rgba(ctx, &b)).flatten();
            sink(row, rgba)?;
        }
    }
    Ok(())
}

fn read_battle_equip(ctx: &ScanCtx<'_>, c: &TexCoord) -> Result<Rgba, String> {
    let entry = u32::try_from(c.entry).map_err(|_| format!("no PROT entry {}", c.entry))?;
    let slot = battle_slot(c.section)?;
    let block = battle_blocks_of_entry(ctx, entry)
        .into_iter()
        .find(|b| b.slot() == slot && b.pool_offset == c.offset)
        .ok_or_else(|| {
            format!(
                "no battle-equipment block at entry {entry} slot {slot} +0x{:X}",
                c.offset
            )
        })?;
    battle_rgba(ctx, &block).ok_or_else(|| "battle-equipment block does not decode".to_string())
}

fn op_battle_equip(c: &TexCoord) -> Result<ReplaceOp, String> {
    let entry = u32::try_from(c.entry)
        .map_err(|_| "battle-equipment art lives in a PROT entry, not the gap".to_string())?;
    Ok(ReplaceOp::BattleEquip(BattleTextureTarget {
        entry,
        slot: battle_slot(c.section)?,
    }))
}

// --- Targeted single-row decode ---------------------------------------------

/// Decode one row full-size, whichever family it belongs to.
pub fn read_row(ctx: &ScanCtx<'_>, coord: &TexCoord) -> Result<Rgba, String> {
    let t = tier(coord.tier).ok_or_else(|| format!("unknown texture family {:?}", coord.tier))?;
    (t.read)(ctx, coord)
}

fn read_tim(ctx: &ScanCtx<'_>, c: &TexCoord) -> Result<Rgba, String> {
    let tim = match (c.entry >= 0, c.section >= 0) {
        // Compressed tier.
        (true, true) => ctx.with_lzs_sections(c.entry as u32, |sections| {
            sections
                .ok_or_else(|| format!("PROT entry {} is not an LZS container", c.entry))
                .and_then(|s| {
                    s.get(c.section as usize)
                        .and_then(|sec| sec.get(c.offset as usize..))
                        .ok_or_else(|| "offset past the decoded section".to_string())
                        .and_then(|at| legaia_tim::parse_strict(at).map_err(|e| e.to_string()))
                })
        })?,
        // Raw tier, inside an entry.
        (true, false) => {
            let bytes = ctx
                .entry_bytes(c.entry as u32)
                .ok_or_else(|| format!("no PROT entry {}", c.entry))?;
            let at = bytes
                .get(c.offset as usize..)
                .ok_or_else(|| "offset past the entry".to_string())?;
            legaia_tim::parse_strict(at).map_err(|e| e.to_string())?
        }
        // Raw tier, in the unindexed gap.
        (false, _) => {
            let at = ctx
                .prot
                .get(c.offset as usize..)
                .ok_or_else(|| "offset past PROT.DAT".to_string())?;
            legaia_tim::parse_strict(at).map_err(|e| e.to_string())?
        }
    };
    tim_rgba(&tim).ok_or_else(|| "texture does not decode".to_string())
}

fn read_save_icon(ctx: &ScanCtx<'_>, c: &TexCoord) -> Result<Rgba, String> {
    use legaia_asset::save_icon as si;
    let bytes = ctx
        .entry_bytes(si::PROT_ENTRY as u32)
        .ok_or("save-icon sheet entry is missing")?;
    let sheet = si::parse_entry(bytes).map_err(|e| format!("{e:#}"))?;
    let data = sheet
        .tile_rgba(c.section.max(0) as usize)
        .map_err(|e| format!("{e:#}"))?;
    Ok(Rgba {
        w: si::TILE_SIZE,
        h: si::TILE_SIZE,
        data,
    })
}

fn read_summon(ctx: &ScanCtx<'_>, c: &TexCoord) -> Result<Rgba, String> {
    let bytes = ctx
        .entry_bytes(c.entry.max(0) as u32)
        .ok_or_else(|| format!("no PROT entry {}", c.entry))?;
    let p = summon_page(bytes, c.offset as usize)
        .ok_or_else(|| format!("no summon texture page at +0x{:X}", c.offset))?;
    summon_rgba(bytes, &p, SUMMON_PREVIEW_CLUT_SUB)
        .ok_or_else(|| "summon page does not decode".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_ids_are_unique_and_resolvable() {
        let mut seen = Vec::new();
        for t in tiers() {
            assert!(!seen.contains(&t.id), "duplicate tier id {:?}", t.id);
            assert!(tier(t.id).is_some());
            seen.push(t.id);
        }
    }

    #[test]
    fn tim_tiers_round_trip_their_coordinates() {
        let c = TexCoord {
            tier: TIER_LZS,
            entry: 42,
            section: 3,
            offset: 0x1234,
        };
        match replace_op(&c).expect("resolves") {
            ReplaceOp::Tim(t) => {
                assert_eq!(t.entry, Some(42));
                assert_eq!(t.lzs_section, Some(3));
                assert_eq!(t.offset, 0x1234);
            }
            _ => panic!("lzs tier must resolve to a TIM write"),
        }
    }

    #[test]
    fn gap_and_raw_coordinates_drop_their_optional_halves() {
        let c = TexCoord {
            tier: TIER_RAW,
            entry: -1,
            section: -1,
            offset: 7,
        };
        match replace_op(&c).expect("resolves") {
            ReplaceOp::Tim(t) => {
                assert_eq!(t.entry, None);
                assert_eq!(t.lzs_section, None);
            }
            _ => panic!("raw tier must resolve to a TIM write"),
        }
    }

    #[test]
    fn save_icon_resolves_to_its_own_writer() {
        let c = TexCoord {
            tier: TIER_SAVE_ICON,
            entry: 899,
            section: 4,
            offset: 0,
        };
        match replace_op(&c).expect("resolves") {
            ReplaceOp::SaveIconSlot(s) => assert_eq!(s, 4),
            _ => panic!("portraits must not take the generic TIM writer"),
        }
    }

    #[test]
    fn a_read_only_family_refuses_to_resolve_a_write() {
        let c = TexCoord {
            tier: TIER_SUMMON,
            entry: 893,
            section: 0,
            offset: 0x204,
        };
        let e = replace_op(&c).expect_err("read-only families must not write");
        assert!(e.contains("export"), "the error should say why: {e}");
    }

    #[test]
    fn every_tier_declares_whether_it_can_be_written() {
        // A family that claims `replaceable` must resolve a coordinate to a
        // write op, and one that does not must refuse. Nothing may be
        // silently half-wired.
        for t in tiers() {
            let c = TexCoord {
                tier: t.id,
                entry: 1,
                section: 0,
                offset: 0,
            };
            assert_eq!(
                (t.op)(&c).is_ok(),
                t.replaceable,
                "tier {:?} disagrees with its own `replaceable` flag",
                t.id
            );
        }
    }

    #[test]
    fn the_battle_slot_field_names_both_of_its_slot_spaces() {
        use legaia_asset::battle_texture_catalog::BattleTextureSlot;
        // A descriptor index is itself ...
        for i in [0usize, 1, 14, 51] {
            assert_eq!(
                battle_slot(i as i64).expect("a section index"),
                BattleTextureSlot::Section(i)
            );
        }
        // ... and the two header blocks take the negative half, which is
        // what lets one signed field address both. `-1` is a real slot here,
        // not the "does not apply" the other families spell it as.
        assert_eq!(
            battle_slot(-1).expect("header block 0"),
            BattleTextureSlot::Record0(0)
        );
        assert_eq!(
            battle_slot(-2).expect("header block 1"),
            BattleTextureSlot::Record0(1)
        );
        assert!(battle_slot(-9999).is_err());
    }

    #[test]
    fn battle_coordinates_resolve_to_the_battle_writer() {
        use legaia_asset::battle_texture_catalog::BattleTextureSlot;
        for (section, want) in [
            (14i64, BattleTextureSlot::Section(14)),
            (-1, BattleTextureSlot::Record0(0)),
        ] {
            let c = TexCoord {
                tier: TIER_BATTLE_EQUIP,
                entry: 864,
                section,
                offset: 0x3784,
            };
            match replace_op(&c).expect("resolves") {
                ReplaceOp::BattleEquip(t) => {
                    assert_eq!(t.entry, 864);
                    assert_eq!(t.slot, want);
                }
                other => panic!("battle art must not take another writer: {other:?}"),
            }
        }
        // The unindexed gap holds no player file, so there is nothing to
        // address there.
        let gap = TexCoord {
            tier: TIER_BATTLE_EQUIP,
            entry: -1,
            section: 0,
            offset: 0,
        };
        assert!(replace_op(&gap).is_err());
    }

    #[test]
    fn unknown_family_is_an_error_not_a_silent_tim_write() {
        let c = TexCoord {
            tier: "no-such-family",
            entry: 0,
            section: -1,
            offset: 0,
        };
        assert!(replace_op(&c).is_err());
    }

    #[test]
    fn fingerprint_matches_the_catalogs_hash() {
        // The catalogs' own FNV-1a-64 constant, so a pack's hash and a
        // catalog label key the same value.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
