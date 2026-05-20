//! Monster stat archive parser (PROT entry `0867_battle_data`).
//!
//! This is the global monster table the battle loader (`FUN_800542C8`)
//! streams at battle init: one fixed-size `0x14000`-byte slot per monster
//! id (1-based), `slot = (id-1) * 0x14000`. Each slot is
//! `[u32 decompressed_size][Legaia LZS stream]`; the decoded block's head
//! is the stat record that `FUN_80054CB0` copies into the battle actor.
//!
//! Pinned by a runtime watchpoint during live battles (Rim Elm scripted
//! fights): the loader's `disc_read` CdlLOC + relative-seek `(id-1)*40`
//! sectors resolve to PROT.DAT offset `0x38AF000` = entry 867, and three
//! decoded records (Gimard id 10, Killer Bee id 62, Queen Bee id 63) match
//! the live actor HP/MP/stats byte-for-byte. The CDNAME label `monster_data`
//! (PROT 869) is a misleading stub; the real archive is the 15.9 MB
//! `battle_data` entry 867. See `docs/subsystems/battle.md`.
//!
//! ## Record layout (decoded block head)
//!
//! All multi-byte fields are little-endian. Offsets are into the LZS-decoded
//! block; `name_offset` / `xp_offset` are block-relative byte offsets the
//! loader fixes up to absolute pointers at load.
//!
//! ```text
//! +0x00  u32  name_offset   ; -> NUL-terminated name string in the block
//! +0x04  u32  xp_offset     ; -> XP / drop sub-record (actor +0x230)
//! +0x08  u32  record_size   ; stat-record allocation footprint
//! +0x0C  u16  hp            ; -> actor +0x14C/+0x14E/+0x172
//! +0x0E  u16  stat0         ; -> actor +0x154/+0x156
//! +0x10  u16  mp            ; -> actor +0x150/+0x152/+0x174
//! +0x12  u16  stat1         ; -> actor +0x158/+0x15A (defense-like)
//! +0x14  u16  stat2         ; -> actor +0x15C/+0x15E
//! +0x16  u16  stat3         ; -> actor +0x160/+0x162
//! +0x18  u16  stat4         ; -> actor +0x168/+0x16A
//! +0x1A  u16  stat5         ; -> actor +0x164/+0x166
//! +0x4A  u8   magic_count   ; spell-entry count
//! +0x4C  u32[] spell_offsets ; element-resistance source (first byte = element)
//! ```
//!
//! The exact stat-name mapping (which of `stat0..stat5` is attack / defense /
//! agility) is not fully split; `stat0` is empirically the dominant offensive
//! value. Consumers that only need HP/MP/name + a representative attack use
//! [`MonsterRecord::hp`] / [`mp`](MonsterRecord::mp) / [`name`](MonsterRecord::name)
//! / [`stats`](MonsterRecord::stats)`[0]`.

use anyhow::{Result, bail};

/// Fixed per-monster slot stride inside the archive (`0x14000` bytes = 40
/// sectors). Confirmed by the loader's relative-seek `(id-1)*40` sectors.
pub const SLOT_STRIDE: usize = 0x14000;

/// Minimum decoded-block size that can hold the stat record head.
const MIN_RECORD_BYTES: usize = 0x4C;

/// One monster's parsed stat record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterRecord {
    /// 1-based monster id (the archive slot index + 1).
    pub id: u16,
    /// Display name (control-prefix bytes `< 0x20` stripped; the retail
    /// names carry a leading `0x01` icon/color escape).
    pub name: String,
    /// Max HP.
    pub hp: u16,
    /// Max MP.
    pub mp: u16,
    /// The six stat halfwords at record `+0x0E/+0x12/+0x14/+0x16/+0x18/+0x1A`.
    /// `stats[0]` is empirically the dominant offensive stat.
    pub stats: [u16; 6],
    /// Spell-slot count (`+0x4A`).
    pub magic_count: u8,
    /// Raw XP / drop sub-record word (`u32` at the block's `xp_offset`).
    pub xp_drop_raw: u32,
}

/// Number of `0x14000`-byte slots the archive can hold.
pub fn slot_count(entry: &[u8]) -> usize {
    entry.len() / SLOT_STRIDE
}

fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn read_u16(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
}

/// Decode the monster id `id` (1-based) from the archive entry bytes.
///
/// Returns `Ok(None)` for an out-of-range id or an empty / filler slot
/// (one whose decoded block fails the record sanity checks). Returns
/// `Err` only when the slot claims a valid `dec_size` but the LZS stream
/// fails to decode to that length.
pub fn record(entry: &[u8], id: u16) -> Result<Option<MonsterRecord>> {
    if id == 0 {
        return Ok(None);
    }
    let slot = (id as usize - 1) * SLOT_STRIDE;
    let Some(dec_size) = read_u32(entry, slot) else {
        return Ok(None);
    };
    let dec_size = dec_size as usize;
    // Filler / empty slots carry a tiny or absurd dec_size. Bound it to a
    // plausible monster-block size before trusting the LZS decode.
    if !(MIN_RECORD_BYTES..=SLOT_STRIDE * 8).contains(&dec_size) {
        return Ok(None);
    }
    // Hand the decoder a generous source slice (the LZS stream can spill past
    // its own slot, like every other Legaia LZS container).
    let src = &entry[slot + 4..];
    let block = legaia_lzs::decompress(src, dec_size)?;
    if block.len() != dec_size {
        bail!(
            "monster id {id}: LZS decoded {} bytes, expected {dec_size}",
            block.len()
        );
    }
    Ok(parse_block(id, &block))
}

/// Parse a decoded monster block into a [`MonsterRecord`]. Returns `None`
/// when the block fails the record sanity checks (empty / filler slot).
fn parse_block(id: u16, block: &[u8]) -> Option<MonsterRecord> {
    if block.len() < MIN_RECORD_BYTES {
        return None;
    }
    let name_offset = read_u32(block, 0)? as usize;
    let xp_offset = read_u32(block, 4)? as usize;
    // A real record's name offset points inside the block at a printable,
    // NUL-terminated string. Reject slots that don't.
    if name_offset == 0 || name_offset >= block.len() {
        return None;
    }
    let name = read_cstr(block, name_offset)?;
    if name.is_empty() {
        return None;
    }
    let hp = read_u16(block, 0x0C)?;
    let mp = read_u16(block, 0x10)?;
    let stats = [
        read_u16(block, 0x0E)?,
        read_u16(block, 0x12)?,
        read_u16(block, 0x14)?,
        read_u16(block, 0x16)?,
        read_u16(block, 0x18)?,
        read_u16(block, 0x1A)?,
    ];
    let magic_count = *block.get(0x4A)?;
    let xp_drop_raw = read_u32(block, xp_offset).unwrap_or(0);
    Some(MonsterRecord {
        id,
        name,
        hp,
        mp,
        stats,
        magic_count,
        xp_drop_raw,
    })
}

/// Read a NUL-terminated monster name at `off` and clean it to a display
/// string. The on-disc names are printable ASCII carrying in-game text
/// escapes: a leading `^X` caret color-code (e.g. `^A `) and an optional
/// `$N` variant suffix (e.g. `Gimard $2`). The caret escapes are stripped;
/// the variant suffix is kept (it distinguishes `Gimard` from `Gimard $2`).
/// Returns `None` if the bytes aren't a plausible printable name.
fn read_cstr(block: &[u8], off: usize) -> Option<String> {
    let end = block[off..].iter().position(|&b| b == 0)? + off;
    let raw = &block[off..end];
    if raw.is_empty() || raw.len() > 32 {
        return None;
    }
    // The names are plain printable ASCII (caret escapes, `$`, letters,
    // digits, spaces). Reject anything else as a filler / non-name slot.
    if !raw.iter().all(|&b| (0x20..0x7F).contains(&b)) {
        return None;
    }
    // Strip `^X` caret color-escape pairs.
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'^' && i + 1 < raw.len() {
            i += 2;
            continue;
        }
        out.push(raw[i] as char);
        i += 1;
    }
    let name = out.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Decode every populated monster slot in the archive. Skips empty / filler
/// slots silently; propagates an `Err` only on a genuine LZS decode failure.
pub fn records(entry: &[u8]) -> Result<Vec<MonsterRecord>> {
    let mut out = Vec::new();
    for id in 1..=slot_count(entry) as u16 {
        if let Some(rec) = record(entry, id)? {
            out.push(rec);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-slot archive whose head record is a known monster, LZS
    /// stored verbatim (the Legaia LZS decoder round-trips an
    /// uncompressed-flagged stream). We instead lean on a tiny hand-rolled
    /// block and only exercise the byte parser via [`parse_block`].
    #[test]
    fn parse_block_reads_named_record() {
        let mut block = vec![0u8; 0x60];
        // name at 0x40, xp record at 0x50.
        block[0x00..0x04].copy_from_slice(&0x40u32.to_le_bytes());
        block[0x04..0x08].copy_from_slice(&0x50u32.to_le_bytes());
        block[0x0C..0x0E].copy_from_slice(&99u16.to_le_bytes()); // HP
        block[0x0E..0x10].copy_from_slice(&60u16.to_le_bytes()); // stat0
        block[0x10..0x12].copy_from_slice(&20u16.to_le_bytes()); // MP
        block[0x12..0x14].copy_from_slice(&23u16.to_le_bytes()); // stat1
        block[0x14..0x16].copy_from_slice(&12u16.to_le_bytes());
        block[0x16..0x18].copy_from_slice(&15u16.to_le_bytes());
        block[0x18..0x1A].copy_from_slice(&16u16.to_le_bytes());
        block[0x1A..0x1C].copy_from_slice(&22u16.to_le_bytes());
        block[0x4A] = 9; // magic count
        // name "^A Gimard\0" at 0x40 (caret color-escape + space stripped).
        block[0x40..0x49].copy_from_slice(b"^A Gimard");
        // xp word at 0x50.
        block[0x50..0x54].copy_from_slice(&0x1234u32.to_le_bytes());

        let rec = parse_block(10, &block).expect("record parses");
        assert_eq!(rec.id, 10);
        assert_eq!(rec.name, "Gimard");
        assert_eq!(rec.hp, 99);
        assert_eq!(rec.mp, 20);
        assert_eq!(rec.stats, [60, 23, 12, 15, 16, 22]);
        assert_eq!(rec.magic_count, 9);
        assert_eq!(rec.xp_drop_raw, 0x1234);
    }

    #[test]
    fn parse_block_rejects_filler() {
        // All-zero block: name_offset 0 -> rejected.
        assert!(parse_block(1, &[0u8; 0x60]).is_none());
        // Too short.
        assert!(parse_block(1, &[0u8; 8]).is_none());
    }

    #[test]
    fn read_cstr_strips_caret_escapes_keeps_variant() {
        let mut b = vec![0u8; 0x20];
        b[..6].copy_from_slice(b"Hornet");
        assert_eq!(read_cstr(&b, 0).as_deref(), Some("Hornet"));
        // Caret color-escape + space prefix stripped; `$N` variant kept.
        let mut g = vec![0u8; 0x20];
        g[..12].copy_from_slice(b"^A Gimard $2");
        assert_eq!(read_cstr(&g, 0).as_deref(), Some("Gimard $2"));
    }
}
