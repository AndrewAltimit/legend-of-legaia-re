//! Kingdom-bundle decoder: open one of the three world-map kingdom PROT
//! entries (`map01`/`map02`/`map03`) and pull a requested slot out of its
//! 7-asset table as decoded bytes.
//!
//! Locates the `scene_asset_table` (or its `scene_scripted_asset_table`
//! prescript-prefixed variant) by scanning 0x800-aligned offsets for
//! `[u32 count = 7]` with `descriptor[0].data_offset == 0x40`, then
//! reads each slot's `(type, size, data_offset)` triple and LZS-decodes
//! the payload.
//!
//! ## Per-slot semantics (per the asset-type table)
//!
//! Cross-referenced with [`docs/formats/asset-type.md`] and verified by
//! pulling the three kingdom bundles:
//!
//! | Slot | Type | What it carries | Format docs |
//! |---|---|---|---|
//! | 0 | `0x01` TIM_LIST | Packed PSX TIMs (texture atlases) | [tim-pack.md] |
//! | 1 | `0x02` TMD pack | Landmark TMDs (40 / 36 / 56 for Drake / Sebucus / Karisto) | [tmd.md] |
//! | 2 | `0x03` MAN | Entity placement records (50 / 23 / 40 records) | scene-bundles.md |
//! | 3 | `0x04` HD-OBJ index | Small structural index (~500B); semantic unpinned | - |
//! | 4 | `0x05` "MOVE" | Top-view wireframe / coastline outlines | [world-map-overlay.md] |
//! | 5 | `0x06` | Unknown | - |
//! | 6 | `0x07` | Unknown | - |
//!
//! The kingdom bundle re-purposes the slot-4 byte (the standard "MOVE"
//! type) as world-map overlay data; the same tag is used differently
//! elsewhere in the disc. See `world-map-overlay.md` for the slot-4
//! format and `world_map_overlay::parse` for the parser.

use legaia_lzs as lzs;

/// One slot's `(type, size, data_offset)` descriptor plus its
/// LZS-decoded payload.
#[derive(Clone, Debug)]
pub struct KingdomSlot {
    /// Slot index (0..7).
    pub index: u8,
    /// Type byte from the descriptor's `type_size` high byte.
    pub type_byte: u8,
    /// LZS-decoded size declared by the descriptor.
    pub declared_size: usize,
    /// Byte offset of the LZS-compressed payload, relative to the
    /// `scene_asset_table` start.
    pub data_offset: usize,
    /// Decoded slot bytes. `Err` if LZS decode failed.
    pub decoded: Result<Vec<u8>, String>,
}

/// All seven slots of one kingdom bundle plus the table base offset.
#[derive(Clone, Debug)]
pub struct KingdomBundle {
    /// Byte offset of the 7-asset table inside the PROT entry buffer.
    pub table_offset: usize,
    /// Seven slots in declaration order.
    pub slots: Vec<KingdomSlot>,
}

/// Locate the 7-asset table inside a kingdom PROT entry buffer.
///
/// Scans 0x800-aligned offsets for `u32_le[0] == 7` and
/// `descriptor[0].data_offset == 0x40`. Catches both the bare
/// `scene_asset_table` and the prescript-prefixed
/// `scene_scripted_asset_table` variants without needing a separate
/// detector.
pub fn find_asset_table_offset(buf: &[u8]) -> Option<usize> {
    let mut off = 0usize;
    while off + 64 <= buf.len() {
        let count = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        if count == 7 {
            let d0 = u32::from_le_bytes(buf[off + 12..off + 16].try_into().unwrap());
            if d0 == 0x40 {
                return Some(off);
            }
        }
        off += 0x800;
    }
    None
}

/// Open a kingdom PROT entry buffer and LZS-decode all seven slots.
///
/// Returns the table base offset plus per-slot `KingdomSlot` records.
/// Each slot's `decoded` field is `Err` if LZS decode failed (e.g.
/// when a slot's declared size doesn't match the compressed stream);
/// callers that only need a subset can probe individual slots without
/// being blocked by a bad sibling.
pub fn parse(buf: &[u8]) -> Option<KingdomBundle> {
    let table_offset = find_asset_table_offset(buf)?;
    let table = &buf[table_offset..];
    let mut slots = Vec::with_capacity(7);
    for k in 0..7u8 {
        // Slot descriptor: 8 bytes of (type<<24 | size_24) + 4 bytes of
        // table-relative data_offset. Slots start at table+8 (skipping
        // the u32 count + u32 reserved/unused word at offset +4).
        let ts_off = 8 + (k as usize) * 8;
        let do_off = ts_off + 4;
        if table.len() < do_off + 4 {
            break;
        }
        let ts = u32::from_le_bytes(table[ts_off..ts_off + 4].try_into().unwrap());
        let do_ = u32::from_le_bytes(table[do_off..do_off + 4].try_into().unwrap()) as usize;
        let type_byte = (ts >> 24) as u8;
        let declared_size = (ts & 0x00FF_FFFF) as usize;
        let decoded = if declared_size == 0 {
            Err("declared_size == 0".to_string())
        } else if do_ >= table.len() {
            Err(format!("data_offset 0x{do_:X} out of range"))
        } else {
            lzs::decompress(&table[do_..], declared_size).map_err(|e| format!("lzs: {e}"))
        };
        slots.push(KingdomSlot {
            index: k,
            type_byte,
            declared_size,
            data_offset: do_,
            decoded,
        });
    }
    Some(KingdomBundle {
        table_offset,
        slots,
    })
}

/// Decode just one slot. Faster than `parse` when only a single slot
/// is needed (e.g. slot 4 for the wireframe overlay).
pub fn decode_slot(buf: &[u8], slot: u8) -> Result<Vec<u8>, String> {
    let table_offset = find_asset_table_offset(buf).ok_or("no 7-asset table found")?;
    let table = &buf[table_offset..];
    if slot >= 7 {
        return Err(format!("slot {slot} >= 7"));
    }
    let ts_off = 8 + (slot as usize) * 8;
    let do_off = ts_off + 4;
    if table.len() < do_off + 4 {
        return Err("table truncated".into());
    }
    let ts = u32::from_le_bytes(table[ts_off..ts_off + 4].try_into().unwrap());
    let do_ = u32::from_le_bytes(table[do_off..do_off + 4].try_into().unwrap()) as usize;
    let declared_size = (ts & 0x00FF_FFFF) as usize;
    if declared_size == 0 {
        return Err(format!("slot {slot} has size 0"));
    }
    if do_ >= table.len() {
        return Err(format!("slot {slot} data_offset 0x{do_:X} out of range"));
    }
    lzs::decompress(&table[do_..], declared_size).map_err(|e| format!("lzs: {e}"))
}
