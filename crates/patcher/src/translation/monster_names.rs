//! Monster names: the `monster_names` pack section (`mon:<id>` keys).
//!
//! A monster's display name lives in its own record, not in the executable:
//! each id's slot of the monster archive (PROT entry 867,
//! `legaia_asset::monster_archive`) is an LZS stream whose decoded block opens
//! with a block-relative `name_offset` word. The battle loader fixes that word
//! to a pointer and copies the string into the actor's display-name buffer
//! (`FUN_80054CB0`: `strlen + 1` bytes into `actor + 0x1BC`), and the name
//! plaque and every battle message that names the enemy read it from there.
//! So one edit per record reaches every place battle shows the name.
//!
//! The string keeps its markup: a leading `^X` element-badge escape (decoded as
//! the `{5e:xx}` token - the plaque draws badge `X - 'A'`) and a trailing
//! ` $N` variant suffix (the loader stops the plaque copy at `$`). A
//! translation keeps both.
//!
//! Import is same-size inside the record: the new name must fit the bytes
//! from `name_offset` up to the next thing the record addresses (the lowest
//! block-relative offset in its pointer words above the name), less the
//! terminator, and never longer than the longest retail name
//! ([`RETAIL_LONGEST_NAME`]) - the loader's copy is unbounded, so no name may
//! outgrow what the retail data already puts in the actor's buffer. The edited block is re-packed into its fixed
//! `0x14000`-byte slot (`monster_archive::encode_slot`), so no other slot and
//! no PROT offset moves.

use anyhow::Result;
use legaia_asset::monster_archive::{self, SLOT_STRIDE};

use super::markup;

/// Longest raw name on the retail USA archive (`^A Gola Gola $2`, fifteen
/// bytes). The loader's copy into the actor's name buffer is unbounded, so no
/// name may outgrow what retail already puts there - a constant rather than
/// the live archive's maximum, so a budget never shrinks when the longest name
/// is itself translated shorter.
pub const RETAIL_LONGEST_NAME: usize = 15;

/// Record head size up to the spell-offset array (`+0x4C`).
const HEAD: usize = 0x4C;

/// One monster's name field inside its decoded block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameField {
    /// Block-relative offset of the name's first byte.
    pub offset: usize,
    /// The name as stored (no terminator).
    pub bytes: Vec<u8>,
    /// Bytes a replacement may occupy before the next addressed data in the
    /// record (excluding its own terminator).
    pub room: usize,
}

fn u32_at(b: &[u8], off: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?) as usize)
}

/// Locate a decoded block's name field. `None` for a block that is not a
/// named monster record.
pub fn name_field(block: &[u8]) -> Option<NameField> {
    if block.len() < HEAD {
        return None;
    }
    let offset = u32_at(block, 0)?;
    if offset < HEAD || offset >= block.len() {
        return None;
    }
    let end = block[offset..].iter().position(|&b| b == 0)? + offset;
    let bytes = block[offset..end].to_vec();
    // Glyph bytes only (a localized build's accent tiles included).
    if bytes.is_empty() || bytes.len() > 64 || bytes.iter().any(|&b| b < 0x20) {
        return None;
    }
    // Everything the record addresses: the two head pointers, then every word
    // from the spell-offset array up to the name - the spell offsets and the
    // effect-offset table that follows them (`monster_archive` resolves both
    // block-relative). The name may grow only up to the lowest of them above
    // it; a word that is not an offset at all lands past the block and is
    // ignored.
    let mut next = block.len();
    let mut consider = |v: usize| {
        if v > offset && v < next {
            next = v;
        }
    };
    for w in [4usize, 8] {
        if let Some(v) = u32_at(block, w) {
            consider(v);
        }
    }
    for w in (HEAD..offset).step_by(4) {
        if let Some(v) = u32_at(block, w) {
            consider(v);
        }
    }
    let room = (next - offset).saturating_sub(1).max(bytes.len());
    Some(NameField {
        offset,
        bytes,
        room,
    })
}

/// Every named record in an archive entry: `(id, field)`.
pub fn fields(entry: &[u8]) -> Vec<(u16, NameField)> {
    let mut out = Vec::new();
    for id in 1..=monster_archive::slot_count(entry) as u16 {
        // A populated record (the parser's sanity checks), then its block.
        if !matches!(monster_archive::record(entry, id), Ok(Some(_))) {
            continue;
        }
        let Ok(Some(block)) = monster_archive::decode_block(entry, id) else {
            continue;
        };
        if let Some(f) = name_field(&block) {
            out.push((id, f));
        }
    }
    out
}

/// The byte budget each id's name may use: its record's room, capped at
/// [`RETAIL_LONGEST_NAME`] (and never below the name it holds).
pub fn budgets(fields: &[(u16, NameField)]) -> Vec<(u16, usize)> {
    fields
        .iter()
        .map(|(id, f)| (*id, f.room.min(RETAIL_LONGEST_NAME).max(f.bytes.len())))
        .collect()
}

/// Parse a `mon:<id>` key.
pub fn key_id(key: &str) -> Option<u16> {
    key.strip_prefix("mon:")?.parse().ok().filter(|&id| id > 0)
}

/// Rewrite one slot's name: decode, overwrite the name (terminated, the rest
/// of the old span zeroed), re-pack into a full slot. `entry` is the whole
/// archive (a stream may run past its own slot, so the decoder is handed the
/// tail of the entry).
pub fn rewrite_slot(entry: &[u8], id: u16, name: &[u8], budget: usize) -> Result<Vec<u8>> {
    let Some(mut block) = monster_archive::decode_block(entry, id)? else {
        anyhow::bail!("monster {id}: empty slot");
    };
    let Some(field) = name_field(&block) else {
        anyhow::bail!("monster {id}: no name field in the record");
    };
    if name.len() > budget.min(field.room) {
        anyhow::bail!(
            "monster {id}: name needs {} bytes but the record holds {}",
            name.len(),
            budget.min(field.room)
        );
    }
    let span = name.len().max(field.bytes.len()) + 1;
    let dst = &mut block[field.offset..field.offset + span];
    dst.fill(0);
    dst[..name.len()].copy_from_slice(name);
    monster_archive::encode_slot(&block)
}

/// `true` when the slot at `id` decodes and reads `expected` as its name.
pub fn name_is(entry: &[u8], id: u16, expected: &[u8]) -> bool {
    monster_archive::decode_block(entry, id)
        .ok()
        .flatten()
        .and_then(|b| name_field(&b))
        .is_some_and(|f| f.bytes == expected)
}

/// The markup form of a stored name.
pub fn decode(bytes: &[u8]) -> String {
    markup::decode(bytes)
}

/// Byte offset of monster `id`'s slot inside the archive entry.
pub fn slot_offset(id: u16) -> usize {
    (id as usize - 1) * SLOT_STRIDE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_with(name: &[u8], next: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[0..4].copy_from_slice(&0x50u32.to_le_bytes());
        b[4..8].copy_from_slice(&next.to_le_bytes());
        b[0x50..0x50 + name.len()].copy_from_slice(name);
        // Filler between the terminator and the next addressed data.
        b[0x50 + name.len() + 1] = 0x5B;
        b
    }

    #[test]
    fn room_runs_to_the_next_addressed_offset() {
        let b = block_with(b"^A Gimard", 0x5C);
        let f = name_field(&b).unwrap();
        assert_eq!(f.bytes, b"^A Gimard");
        assert_eq!(f.room, 0x5C - 0x50 - 1);
    }

    #[test]
    fn budget_is_capped_by_the_longest_retail_name() {
        let a = (1, name_field(&block_with(b"Gob", 0xF0)).unwrap());
        let b = (2, name_field(&block_with(b"Killer Bee", 0x5C)).unwrap());
        let got = budgets(&[a, b]);
        assert_eq!(got, vec![(1, RETAIL_LONGEST_NAME), (2, 0x5C - 0x50 - 1)]);
    }

    #[test]
    fn key_parser() {
        assert_eq!(key_id("mon:10"), Some(10));
        assert_eq!(key_id("mon:0"), None);
        assert_eq!(key_id("man:10:0x1"), None);
    }
}
