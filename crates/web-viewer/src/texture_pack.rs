//! Texture change packs: a shareable, reviewable bundle of texture
//! replacements.
//!
//! ## What a pack may contain, and what it may not
//!
//! A pack carries the **user's own replacement images** and, for each one,
//! the *coordinates* and a *content fingerprint* of the retail texture it
//! replaces. It never carries the retail texture itself. That is a hard
//! property, not a convention: the fingerprint is a 64-bit hash, so a pack
//! identifies which texture it targets without reproducing a single Sony
//! byte, and there is no code path here that can put original pixels into
//! one.
//!
//! ## Why JSON
//!
//! A pack is one UTF-8 JSON file, pretty-printed. The manifest half - which
//! textures, at which coordinates, with what expected fingerprint - is
//! exactly the part a person wants to read and diff before running someone
//! else's pack, and JSON diffs line by line. The images are base64 PNGs, one
//! long line each: a replaced image changes its own line and nothing else. A
//! zip container would make the images marginally smaller and the manifest
//! completely opaque, and would need an archive library inside the WASM
//! bundle to read it.
//!
//! ## Versioning
//!
//! [`FORMAT`] and [`VERSION`] are written from the first pack and checked on
//! every import. A reader rejects a format it does not know rather than
//! guessing, and rejects a *newer* version rather than half-applying it.
//!
//! ## What import actually verifies
//!
//! Import resolves each entry against the user's own disc, in place, and
//! grades it ([`EntryStatus`]). It does not need a prior full scan - it reads
//! the one texture the coordinate names - and it reads through the *current*
//! image, so a texture that has already been patched reports a fingerprint
//! mismatch instead of being silently replaced twice.

use legaia_patcher::disc::DiscPatcher;

use crate::texture_registry::{ReplaceOp, TexCoord, fnv1a64, replace_op, tier};

/// Format tag written into every pack.
pub const FORMAT: &str = "legaia-texture-pack";
/// Current pack version. A reader accepts `<= VERSION`.
pub const VERSION: u64 = 1;

/// One replacement in a pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackEntry {
    pub coord: TexCoord,
    /// FNV-1a-64 of the retail texture this replacement was authored against.
    pub original_fnv1a: u64,
    /// The authored-against dimensions, so a mismatch is reported rather than
    /// discovered as an encode failure.
    pub original_width: u32,
    pub original_height: u32,
    pub original_bpp: u32,
    /// The curated label at authoring time, carried for human review only.
    pub label: String,
    /// Whether the replacement was authored with palette quantization on.
    pub quantize: bool,
    /// The user's replacement image, PNG bytes.
    pub png: Vec<u8>,
}

/// Pack-level metadata. All optional; present so a shared pack can say what
/// it is without a README.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackMeta {
    pub name: String,
    pub author: String,
    pub note: String,
}

/// A parsed pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub version: u64,
    pub meta: PackMeta,
    pub entries: Vec<PackEntry>,
}

/// How one pack entry resolved against the user's disc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryStatus {
    /// Coordinate resolves and the retail texture is the one the pack was
    /// authored against.
    Ok,
    /// The pack names a texture family this build does not have.
    UnknownFamily(String),
    /// The coordinate could not be resolved to a write: it names nothing on
    /// this disc (a different game, a different revision, a corrupt pack), or
    /// it names a family this build can read but not write.
    NotFound(String),
    /// The coordinate resolves but holds different bytes: a different disc
    /// revision, or a texture this disc has already had patched.
    HashMismatch { expected: u64, found: u64 },
    /// The coordinate resolves to a texture of different dimensions - the
    /// replacement cannot fit it.
    SizeMismatch {
        expected: (u32, u32),
        found: (u32, u32),
    },
}

impl EntryStatus {
    /// Short machine-readable tag (what the page keys its per-entry report
    /// rows on).
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::UnknownFamily(_) => "unknown-family",
            Self::NotFound(_) => "not-found",
            Self::HashMismatch { .. } => "hash-mismatch",
            Self::SizeMismatch { .. } => "size-mismatch",
        }
    }

    /// One sentence a person can act on.
    pub fn detail(&self) -> String {
        match self {
            Self::Ok => "matches this disc".to_string(),
            Self::UnknownFamily(f) => format!("this build has no texture family {f:?}"),
            // Deliberately not "not on this disc" - the same status covers a
            // family this build can read but not write, and that one IS on
            // the disc.
            Self::NotFound(why) => format!("cannot be applied: {why}"),
            Self::HashMismatch { expected, found } => format!(
                "this disc holds a different texture here (pack expected \
                 {expected:016x}, disc has {found:016x}) - a different disc \
                 revision, or this texture was already patched"
            ),
            Self::SizeMismatch { expected, found } => format!(
                "this disc's texture is {}x{}, the pack was authored against {}x{}",
                found.0, found.1, expected.0, expected.1
            ),
        }
    }
}

// --- Identifying a texture in place -----------------------------------------

/// What a coordinate currently holds on a disc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ident {
    pub fnv1a: u64,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
}

/// Read the texture a coordinate names off the *current* image and fingerprint
/// it. Targeted: reads only the entry the coordinate names, so importing a
/// pack does not pay for a full-disc scan.
pub fn identify(patcher: &DiscPatcher, coord: &TexCoord) -> Result<Ident, String> {
    match replace_op(coord)? {
        ReplaceOp::Tim(target) => {
            let orig = legaia_patcher::texture::read_texture(patcher, &target)
                .map_err(|e| format!("{e:#}"))?;
            let bpp = match orig.tim.mode {
                legaia_tim::PixelMode::Bpp4 => 4,
                legaia_tim::PixelMode::Bpp8 => 8,
                legaia_tim::PixelMode::Bpp16 => 16,
                legaia_tim::PixelMode::Bpp24 => 24,
                legaia_tim::PixelMode::Mixed => 0,
            };
            Ok(Ident {
                fnv1a: fnv1a64(&orig.tim_bytes),
                width: orig.tim.pixel_width() as u32,
                height: orig.tim.pixel_height() as u32,
                bpp,
            })
        }
        ReplaceOp::SaveIconSlot(slot) => {
            use legaia_asset::save_icon as si;
            let sheet =
                legaia_patcher::save_icon::read_sheet(patcher).map_err(|e| format!("{e:#}"))?;
            // Same identity bytes the scan fingerprints: palette then pixels.
            let mut ident = Vec::with_capacity(si::TILE_CLUT_BYTES + si::TILE_BLOCK_BYTES);
            ident.extend_from_slice(&sheet.tile_clut_bytes(slot).map_err(|e| format!("{e:#}"))?);
            ident.extend_from_slice(
                &sheet
                    .tile_block_pixels(slot)
                    .map_err(|e| format!("{e:#}"))?,
            );
            Ok(Ident {
                fnv1a: fnv1a64(&ident),
                width: si::TILE_SIZE as u32,
                height: si::TILE_SIZE as u32,
                bpp: 4,
            })
        }
    }
}

/// Grade one pack entry against a disc.
pub fn verify(patcher: &DiscPatcher, e: &PackEntry) -> EntryStatus {
    if tier(e.coord.tier).is_none() {
        return EntryStatus::UnknownFamily(e.coord.tier.to_string());
    }
    let got = match identify(patcher, &e.coord) {
        Ok(i) => i,
        Err(why) => return EntryStatus::NotFound(why),
    };
    if (got.width, got.height) != (e.original_width, e.original_height) {
        return EntryStatus::SizeMismatch {
            expected: (e.original_width, e.original_height),
            found: (got.width, got.height),
        };
    }
    if got.fnv1a != e.original_fnv1a {
        return EntryStatus::HashMismatch {
            expected: e.original_fnv1a,
            found: got.fnv1a,
        };
    }
    EntryStatus::Ok
}

// --- Serialization ----------------------------------------------------------

/// Render a pack as pretty-printed JSON.
pub fn to_json(meta: &PackMeta, entries: &[PackEntry]) -> String {
    let items: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "tier": e.coord.tier,
                "entry": e.coord.entry,
                "section": e.coord.section,
                "offset": e.coord.offset,
                "original": {
                    "fnv1a": format!("{:016x}", e.original_fnv1a),
                    "width": e.original_width,
                    "height": e.original_height,
                    "bpp": e.original_bpp,
                    "label": e.label,
                },
                "quantize": e.quantize,
                "png_base64": base64_encode(&e.png),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "format": FORMAT,
        "version": VERSION,
        "name": meta.name,
        "author": meta.author,
        "note": meta.note,
        "textures": items,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
}

/// Parse a pack, rejecting an unknown format or a future version outright.
pub fn from_json(text: &str) -> Result<Pack, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let format = v.get("format").and_then(|f| f.as_str()).unwrap_or("");
    if format != FORMAT {
        return Err(format!(
            "not a Legaia texture pack (its \"format\" is {format:?}, expected {FORMAT:?})"
        ));
    }
    let version = v.get("version").and_then(|f| f.as_u64()).unwrap_or(0);
    if version == 0 || version > VERSION {
        return Err(format!(
            "this pack is version {version}; this page reads up to version {VERSION} - \
             update the page, or ask its author for an older export"
        ));
    }
    let meta = PackMeta {
        name: str_at(&v, "name"),
        author: str_at(&v, "author"),
        note: str_at(&v, "note"),
    };
    let list = v
        .get("textures")
        .and_then(|t| t.as_array())
        .ok_or_else(|| "pack has no \"textures\" list".to_string())?;
    let mut entries = Vec::with_capacity(list.len());
    for (i, t) in list.iter().enumerate() {
        entries.push(entry_from_json(t).map_err(|e| format!("texture {i}: {e}"))?);
    }
    Ok(Pack {
        version,
        meta,
        entries,
    })
}

fn str_at(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|s| s.as_str())
        .unwrap_or_default()
        .to_string()
}

fn entry_from_json(t: &serde_json::Value) -> Result<PackEntry, String> {
    let tier_id = t
        .get("tier")
        .and_then(|s| s.as_str())
        .ok_or("missing \"tier\"")?;
    // Resolve to the registry's own 'static id where possible. An unknown
    // family is NOT a parse failure - it is a reportable per-entry status, so
    // a pack authored on a newer build still imports the entries this build
    // does understand.
    let tier_static: &'static str = tier(tier_id).map(|t| t.id).unwrap_or("");
    let coord = TexCoord {
        tier: if tier_static.is_empty() {
            UNKNOWN_TIER
        } else {
            tier_static
        },
        entry: t
            .get("entry")
            .and_then(|n| n.as_i64())
            .ok_or("missing \"entry\"")?,
        section: t
            .get("section")
            .and_then(|n| n.as_i64())
            .ok_or("missing \"section\"")?,
        offset: t
            .get("offset")
            .and_then(|n| n.as_u64())
            .ok_or("missing \"offset\"")?,
    };
    let orig = t.get("original").ok_or("missing \"original\"")?;
    let fnv_hex = orig
        .get("fnv1a")
        .and_then(|s| s.as_str())
        .ok_or("missing \"original.fnv1a\"")?;
    let original_fnv1a = u64::from_str_radix(fnv_hex, 16)
        .map_err(|_| format!("\"original.fnv1a\" is not 16 hex digits: {fnv_hex:?}"))?;
    let num = |k: &str| -> Result<u32, String> {
        orig.get(k)
            .and_then(|n| n.as_u64())
            .map(|n| n as u32)
            .ok_or_else(|| format!("missing \"original.{k}\""))
    };
    let png_b64 = t
        .get("png_base64")
        .and_then(|s| s.as_str())
        .ok_or("missing \"png_base64\"")?;
    let png = base64_decode(png_b64).map_err(|e| format!("\"png_base64\": {e}"))?;
    Ok(PackEntry {
        coord,
        original_fnv1a,
        original_width: num("width")?,
        original_height: num("height")?,
        original_bpp: num("bpp").unwrap_or(0),
        label: orig
            .get("label")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string(),
        quantize: t.get("quantize").and_then(|b| b.as_bool()).unwrap_or(false),
        png,
    })
}

/// Placeholder family id for a pack entry naming a family this build lacks.
/// Never matches a registry id, so such an entry can only ever be reported.
const UNKNOWN_TIER: &str = "(unknown family)";

// --- base64 -----------------------------------------------------------------
//
// Small enough to own. A dependency here would be a third-party crate inside
// the WASM bundle for forty lines of table lookup.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with `=` padding.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Standard base64, tolerating whitespace (a hand-edited pack may have
/// wrapped a long line).
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a') as u32 + 26),
            b'0'..=b'9' => Ok((c - b'0') as u32 + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 character {:?}", c as char)),
        }
    };
    let clean: Vec<u8> = text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        if chunk.len() == 1 {
            return Err("truncated base64 (a trailing orphan character)".to_string());
        }
        let mut n: u32 = 0;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture_registry::{TIER_LZS, TIER_SAVE_ICON};

    fn sample() -> (PackMeta, Vec<PackEntry>) {
        let meta = PackMeta {
            name: "Sample".to_string(),
            author: "someone".to_string(),
            note: "two textures".to_string(),
        };
        let entries = vec![
            PackEntry {
                coord: TexCoord {
                    tier: TIER_LZS,
                    entry: 512,
                    section: 2,
                    offset: 0x1234,
                },
                original_fnv1a: 0x0123_4567_89ab_cdef,
                original_width: 64,
                original_height: 32,
                original_bpp: 4,
                label: "character".to_string(),
                quantize: true,
                png: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3],
            },
            PackEntry {
                coord: TexCoord {
                    tier: TIER_SAVE_ICON,
                    entry: 899,
                    section: 3,
                    offset: 0,
                },
                original_fnv1a: 0xffff_0000_ffff_0000,
                original_width: 16,
                original_height: 16,
                original_bpp: 4,
                label: "save-slot portrait".to_string(),
                quantize: false,
                png: (0u8..=255).collect(),
            },
        ];
        (meta, entries)
    }

    #[test]
    fn pack_round_trips_through_json() {
        let (meta, entries) = sample();
        let text = to_json(&meta, &entries);
        let back = from_json(&text).expect("re-reads");
        assert_eq!(back.version, VERSION);
        assert_eq!(back.meta, meta);
        assert_eq!(back.entries, entries);
    }

    #[test]
    fn pack_json_is_human_readable() {
        let (meta, entries) = sample();
        let text = to_json(&meta, &entries);
        // Pretty-printed, and the manifest half is greppable.
        assert!(text.contains("\n  \"format\""), "should be pretty-printed");
        assert!(text.contains("\"tier\": \"lzs\""));
        assert!(text.contains("\"fnv1a\": \"0123456789abcdef\""));
    }

    #[test]
    fn a_pack_never_carries_original_pixels() {
        // Structural guard on the promise in the module docs: the only image
        // field is the user's own PNG, and the original is present solely as
        // a hash plus dimensions.
        let (meta, entries) = sample();
        let text = to_json(&meta, &entries);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        for t in v["textures"].as_array().unwrap() {
            let orig = t["original"].as_object().unwrap();
            let keys: Vec<&str> = orig.keys().map(|k| k.as_str()).collect();
            for k in &keys {
                assert!(
                    matches!(*k, "fnv1a" | "width" | "height" | "bpp" | "label"),
                    "\"original\" grew a field {k:?} - if that ever carries pixels \
                     the pack stops being shareable"
                );
            }
        }
    }

    #[test]
    fn foreign_format_is_rejected_not_guessed() {
        let e = from_json(r#"{"format":"something-else","version":1,"textures":[]}"#).unwrap_err();
        assert!(e.contains("not a Legaia texture pack"), "{e}");
    }

    #[test]
    fn a_future_version_is_refused_rather_than_half_applied() {
        let text = format!(
            r#"{{"format":"{FORMAT}","version":{},"textures":[]}}"#,
            VERSION + 1
        );
        let e = from_json(&text).unwrap_err();
        assert!(e.contains("version"), "{e}");
    }

    #[test]
    fn an_unknown_family_parses_but_can_never_resolve() {
        let text = format!(
            r#"{{"format":"{FORMAT}","version":1,"textures":[{{
                "tier":"from-a-newer-build","entry":1,"section":-1,"offset":0,
                "original":{{"fnv1a":"0000000000000001","width":8,"height":8,"bpp":4}},
                "png_base64":""}}]}}"#
        );
        let pack = from_json(&text).expect("parses");
        assert_eq!(pack.entries[0].coord.tier, UNKNOWN_TIER);
        assert!(tier(pack.entries[0].coord.tier).is_none());
        assert!(replace_op(&pack.entries[0].coord).is_err());
    }

    #[test]
    fn base64_round_trips_every_length_residue() {
        for n in 0..64usize {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let enc = base64_encode(&data);
            assert_eq!(enc.len() % 4, 0, "padded to a multiple of 4");
            assert_eq!(base64_decode(&enc).expect("decodes"), data, "n = {n}");
        }
    }

    #[test]
    fn base64_tolerates_wrapped_lines_and_rejects_junk() {
        let enc = base64_encode(&(0u8..=200).collect::<Vec<u8>>());
        let wrapped: String = enc
            .as_bytes()
            .chunks(40)
            .map(|c| format!("{}\n", std::str::from_utf8(c).unwrap()))
            .collect();
        assert_eq!(
            base64_decode(&wrapped).expect("decodes"),
            (0u8..=200).collect::<Vec<u8>>()
        );
        assert!(base64_decode("abc$def").is_err());
    }

    #[test]
    fn status_tags_are_distinct_and_explain_themselves() {
        let all = [
            EntryStatus::Ok,
            EntryStatus::UnknownFamily("x".into()),
            EntryStatus::NotFound("y".into()),
            EntryStatus::HashMismatch {
                expected: 1,
                found: 2,
            },
            EntryStatus::SizeMismatch {
                expected: (8, 8),
                found: (16, 16),
            },
        ];
        let mut tags: Vec<&str> = all.iter().map(|s| s.tag()).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), all.len(), "each status needs its own tag");
        for s in &all {
            assert!(!s.detail().is_empty());
        }
    }
}
