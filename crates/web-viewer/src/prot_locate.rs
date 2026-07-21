//! PROT.DAT offset -> owning-entry reverse lookup, in the browser.
//!
//! Mirrors `prot-extract locate` (`legaia_prot::locate`): given a byte offset
//! into `PROT.DAT` - or an offset inside one extracted `.BIN` file - resolve the
//! entry whose true on-disc *footprint* owns those bytes, and flag when the
//! offset lands in an entry's **over-read tail** (a neighbour's bytes that the
//! extractor's oversized declared window copied into this file). The canonical
//! case: entries 865/866 (Gala/Terra player-battle files) over-read into the
//! monster archive (867), so enemy text shows up at the tail of `0866_*.BIN`.
//!
//! Sony bytes never leave the browser: everything is computed from the user's
//! own loaded `PROT.DAT`, the same client-side model the rest of this viewer
//! uses. The offset arithmetic here is the exact `legaia_prot::locate` code the
//! CLI runs - the wasm layer only parses the TOC and formats JSON.
use super::*;
use legaia_prot::archive::Archive;
use legaia_prot::{cdname, locate};

/// PROT entry that carries the global monster stat archive (enemy data), the
/// one the `monsters.html` page decodes. Surfaced so the locator can point a
/// user who lands here straight at the enemy browser instead of a raw `.BIN`.
const MONSTER_ARCHIVE_INDEX: u32 = 867;

impl LegaiaViewer {
    /// Parse the loaded `PROT.DAT` into a full archive (header TOC + entries
    /// with `start_lba` / declared size / footprint). One clone of the disc
    /// buffer per call - fine for a user-triggered locate; not on a hot path.
    fn locate_archive(&self) -> Option<Archive> {
        if self.disc.is_empty() {
            return None;
        }
        Archive::from_bytes(self.disc.clone()).ok()
    }

    /// The CDNAME block map, if a full disc (which carries `CDNAME.TXT`) was
    /// loaded. Resolved the same way `prot-extract` labels entries.
    fn cdname_map(&self) -> Option<cdname::IndexMap> {
        cdname::parse_str(self.cdname_text.as_deref()?).ok()
    }
}

/// Block name for an entry index, in the extraction frame `prot-extract`'s
/// filenames use (so it matches the `NNNN_<block>.BIN` a user has on disk).
fn block_of(map: Option<&cdname::IndexMap>, index: u32) -> Option<String> {
    map.and_then(|m| cdname::block_for(m, index))
        .map(str::to_owned)
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// Locate a `PROT.DAT` byte offset -> the entry that truly owns it.
    ///
    /// `offset` is decimal or `0x`-hex text (as a hex editor / the CLI shows).
    /// When `in_entry` is `Some(n)`, `offset` is instead read as an offset
    /// inside entry `n`'s extracted `.BIN` file and first translated to an
    /// absolute `PROT.DAT` offset - the common "my hex editor is 0x… into
    /// `0866_*.BIN`" case.
    ///
    /// Shape (all `*_hex` are strings; byte quantities are also given raw):
    /// ```json
    /// { "query": "0x17855", "in_entry": 866,
    ///   "abs_offset": 96341, "abs_offset_hex": "0x17855",
    ///   "owner": { "index": 867, "block": "battle_data",
    ///              "start": 96256, "start_hex": "0x17800",
    ///              "footprint": 15622144, "footprint_hex": "0xEE6000",
    ///              "local_offset": 85, "local_offset_hex": "0x55",
    ///              "is_monster_archive": true },
    ///   "over_read": { "queried_entry": 866, "queried_footprint": 2048,
    ///                  "queried_footprint_hex": "0x800",
    ///                  "message": "0x17855 is past entry 866's footprint ...",
    ///                  "true_owner_label": "entry 867 battle_data" },
    ///   "covering": [ { "index": 866, "block": "battle_data", "role": "over-read copy" },
    ///                 { "index": 867, "block": "battle_data", "role": "true source" } ] }
    /// ```
    /// `owner` is `null` when the offset is past every entry's footprint (tail
    /// padding). `over_read` is `null` unless the offset sits in more than one
    /// extracted window (i.e. at least one file carries it as a neighbour's
    /// over-read copy). `{ "error": "..." }` on a bad offset / unparsable disc.
    pub fn locate_offset_json(&self, offset: &str, in_entry: Option<u32>) -> String {
        let Some(archive) = self.locate_archive() else {
            return err_json("no PROT.DAT loaded (open a disc or raw PROT.DAT first)");
        };
        let raw = match parse_offset(offset) {
            Some(v) => v,
            None => {
                return err_json(&format!(
                    "invalid offset {offset:?} (use decimal or 0x-hex)"
                ));
            }
        };
        let map = self.cdname_map();

        // Resolve the queried offset to an absolute PROT.DAT offset.
        let abs = match in_entry {
            Some(n) => match locate::abs_from_entry_offset(&archive.entries, n, raw) {
                Some(a) => a,
                None => return err_json(&format!("no extraction entry with index {n}")),
            },
            None => raw,
        };

        let loc = locate::locate(&archive.toc, &archive.entries, abs);

        // True owner card.
        let owner = loc.owner.map(|i| {
            let e = &archive.entries[i];
            let footprint = locate::footprint_bytes(&archive.toc, e);
            let local = abs - e.byte_offset;
            serde_json::json!({
                "index": e.index,
                "block": block_of(map.as_ref(), e.index),
                "start": e.byte_offset,
                "start_hex": hex(e.byte_offset),
                "footprint": footprint,
                "footprint_hex": hex(footprint),
                "local_offset": local,
                "local_offset_hex": hex(local),
                "is_monster_archive": e.index == MONSTER_ARCHIVE_INDEX,
            })
        });

        // Label helper for prose ("entry 867 battle_data").
        let owner_label = || -> String {
            loc.owner
                .map(|i| {
                    let e = &archive.entries[i];
                    match block_of(map.as_ref(), e.index) {
                        Some(b) => format!("entry {} {b}", e.index),
                        None => format!("entry {}", e.index),
                    }
                })
                .unwrap_or_else(|| "another entry".to_string())
        };

        // Over-read note. Mirrors the CLI: when the offset was queried against a
        // specific entry AND sits past that entry's footprint, the reader is in
        // its over-read tail. Also surfaced generically whenever the offset is
        // covered by more than one extracted window.
        let over_read = build_over_read(&archive, raw, in_entry, &loc, &owner_label);

        // Every extracted file that physically contains these bytes, tagged by
        // whether it's the true source or an over-read copy.
        let covering: Vec<serde_json::Value> = loc
            .covering
            .iter()
            .map(|&i| {
                let e = &archive.entries[i];
                serde_json::json!({
                    "index": e.index,
                    "block": block_of(map.as_ref(), e.index),
                    "role": if Some(i) == loc.owner { "true source" } else { "over-read copy" },
                    "is_monster_archive": e.index == MONSTER_ARCHIVE_INDEX,
                })
            })
            .collect();

        serde_json::json!({
            "query": hex(raw),
            "in_entry": in_entry,
            "abs_offset": abs,
            "abs_offset_hex": hex(abs),
            "owner": owner,
            "over_read": over_read,
            "covering": covering,
        })
        .to_string()
    }

    /// Every entry whose extracted `.BIN` window over-reads its true footprint -
    /// i.e. its tail carries the next entry's bytes. Mirrors the `OVR` column of
    /// `prot-extract list`, but returns only the flagged rows (the trap-bearing
    /// ones); the vast majority of entries declare exactly their footprint.
    ///
    /// Shape:
    /// ```json
    /// { "total_entries": 1231, "over_read_count": 2,
    ///   "entries": [ { "index": 865, "block": "battle_data", "lba": 76288,
    ///                  "byte_offset": 156237824, "byte_offset_hex": "0x9500000",
    ///                  "declared_size": 16777216, "declared_size_hex": "0x1000000",
    ///                  "footprint": 2048, "footprint_hex": "0x800",
    ///                  "over_read_bytes": 16775168 }, ... ] }
    /// ```
    /// `{ "error": "..." }` when no disc is loaded / the TOC won't parse.
    pub fn prot_over_read_json(&self) -> String {
        let Some(archive) = self.locate_archive() else {
            return err_json("no PROT.DAT loaded (open a disc or raw PROT.DAT first)");
        };
        let map = self.cdname_map();
        let mut over = Vec::new();
        for e in &archive.entries {
            let footprint = locate::footprint_bytes(&archive.toc, e);
            if e.size_bytes <= footprint {
                continue;
            }
            over.push(serde_json::json!({
                "index": e.index,
                "block": block_of(map.as_ref(), e.index),
                "lba": e.start_lba,
                "byte_offset": e.byte_offset,
                "byte_offset_hex": hex(e.byte_offset),
                "declared_size": e.size_bytes,
                "declared_size_hex": hex(e.size_bytes),
                "footprint": footprint,
                "footprint_hex": hex(footprint),
                "over_read_bytes": e.size_bytes - footprint,
                "is_monster_archive": e.index == MONSTER_ARCHIVE_INDEX,
            }));
        }
        serde_json::json!({
            "total_entries": archive.entries.len(),
            "over_read_count": over.len(),
            "entries": over,
        })
        .to_string()
    }
}

/// Build the `over_read` card, or `None` when the offset isn't in an over-read
/// region. Two triggers, matching the CLI: an `in_entry` query that runs past
/// that entry's footprint, or any offset carried by more than one window.
fn build_over_read(
    archive: &Archive,
    raw: u64,
    in_entry: Option<u32>,
    loc: &locate::Located,
    owner_label: &dyn Fn() -> String,
) -> Option<serde_json::Value> {
    if let Some(n) = in_entry
        && let Some(e) = archive.entries.iter().find(|e| e.index == n)
    {
        let footprint = locate::footprint_bytes(&archive.toc, e);
        if raw >= footprint {
            return Some(serde_json::json!({
                "queried_entry": n,
                "queried_footprint": footprint,
                "queried_footprint_hex": hex(footprint),
                "true_owner_label": owner_label(),
                "message": format!(
                    "0x{raw:X} is past entry {n}'s footprint (0x{footprint:X}) - you are in its \
                     over-read tail. Those bytes belong to {}, not entry {n}.",
                    owner_label()
                ),
            }));
        }
    }
    // Generic case: the offset is present in several extracted files.
    if loc.covering.len() > 1 {
        return Some(serde_json::json!({
            "queried_entry": in_entry,
            "true_owner_label": owner_label(),
            "message": format!(
                "This offset is carried by {} extracted files (declared windows overlap here); \
                 the true source is {}.",
                loc.covering.len(),
                owner_label()
            ),
        }));
    }
    None
}

/// Parse a `0x…` hex or decimal offset string (mirrors `prot-extract`'s
/// `parse_offset`).
fn parse_offset(s: &str) -> Option<u64> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u64>().ok()
    }
}

fn hex(v: u64) -> String {
    format!("0x{v:X}")
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}
