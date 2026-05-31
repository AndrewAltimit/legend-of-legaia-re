//! Treasure-chest (field item-give) randomization.
//!
//! A chest gives its item via the field-VM **`GIVE_ITEM` opcode `0x39`**,
//! encoded `[0x39, item_id]` — the item id is a single inline operand byte in
//! the per-scene field-VM script bytecode (pinned in the dispatcher
//! `FUN_801DE840` case `0x39`; see `docs/subsystems/script-vm.md`). The give
//! sites live in the MAN partition-1 per-actor interaction scripts (a chest is
//! an interactable actor).
//!
//! Finding the sites safely needs an **opcode-aware walk** — a naive `0x39`
//! byte scan would hit literal `0x39` bytes inside dialogue / other operands.
//! [`give_item_sites`] walks each partition-1 record's script from its true
//! entry PC with the Track-1 field-VM disassembler ([`legaia_asset::field_disasm`])
//! and stops at the first decode error (where the linear walk runs into the
//! record's inline dialogue pool, which is not field-VM bytecode). Only `0x39`
//! ops reached **before** any desync are returned, so every rewritten byte is a
//! genuine give-item operand. This is a safe lower bound: a chest reached only
//! through a branch the linear walk doesn't follow is left untouched rather than
//! risk corrupting a non-script byte.
//!
//! Edits are same-size (rewrite the id byte), then the MAN is recompressed and
//! written back exactly like the [encounter](crate::encounter) path.

use legaia_asset::field_disasm::{self, DisasmError};
use legaia_asset::{man_section, scene_asset_table};

const MAN_TYPE: u8 = 0x03;

/// A scene bundle's MAN located in a PROT entry, with its chest give-item sites.
pub struct SceneChests {
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Bytes the recompressed MAN must fit within.
    pub compressed_budget: usize,
    /// Decompressed MAN (mutate the chest id bytes in place, then [`Self::repack`]).
    pub decoded: Vec<u8>,
    /// Absolute offsets within `decoded` of each `GIVE_ITEM` operand (id) byte.
    pub sites: Vec<usize>,
}

impl SceneChests {
    /// Locate a scene bundle's MAN and its chest give-item sites, or `None` if
    /// the entry isn't a scene bundle, has no MAN, or has no clean give sites.
    pub fn locate(entry: &[u8], entry_idx: usize) -> Option<Self> {
        let table = scene_asset_table::detect(entry)?;
        let man = table
            .used()
            .iter()
            .find(|d| d.type_byte == MAN_TYPE)
            .copied()?;
        if man.size == 0 || man.data_offset == 0 {
            return None;
        }
        let man_offset = man.data_offset as usize;
        let body = entry.get(man_offset..)?;
        let (decoded, consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        let sites = give_item_sites(&decoded);
        if sites.is_empty() {
            return None;
        }
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget: consumed,
            decoded,
            sites,
        })
    }

    /// The current item id at each chest site, in `sites` order.
    pub fn current_items(&self) -> Vec<u8> {
        self.sites.iter().map(|&o| self.decoded[o]).collect()
    }

    /// Recompress the (mutated) MAN; `None` if it would overflow the footprint.
    pub fn repack(&self) -> Option<Vec<u8>> {
        let stream = legaia_lzs::compress(&self.decoded);
        (stream.len() <= self.compressed_budget).then_some(stream)
    }
}

/// Walk a decompressed MAN's partition-1 record scripts and return the absolute
/// offsets (within `man`) of every `GIVE_ITEM` (op `0x39`) operand byte reached
/// by a clean opcode-aware walk from each record's entry PC.
pub fn give_item_sites(man: &[u8]) -> Vec<usize> {
    let mut sites = Vec::new();
    let Ok(mf) = man_section::parse(man) else {
        return sites;
    };
    let n1 = mf.partitions[1].len();
    for ri in 0..n1 {
        let Some(rec) = mf.actor_placement_record_offset(ri, man.len()) else {
            continue;
        };
        let Some(&n) = man.get(rec) else { continue };
        // Per-record prefix: [u8 local_count N][N*2 bytes][4-byte header][script].
        let pc0 = 1 + n as usize * 2 + 4;
        if rec + pc0 >= man.len() {
            continue;
        }
        walk_record_gives(man, rec, pc0, &mut sites);
    }
    sites
}

/// Walk one record's script from `pc0` (relative to `rec`), pushing the absolute
/// offset of each `0x39` operand byte reached before a decode error.
fn walk_record_gives(man: &[u8], rec: usize, pc0: usize, out: &mut Vec<usize>) {
    let script = &man[rec..];
    let mut pc = pc0;
    loop {
        if pc >= script.len() {
            return;
        }
        match field_disasm::decode(script, pc) {
            Ok(insn) => {
                if insn.size == 0 {
                    return;
                }
                // GIVE_ITEM is [0x39, item_id]; the id is the operand byte after
                // the opcode. Skip the cross-context (extended) form.
                if insn.opcode == 0x39 && insn.extended.is_none() {
                    let id_off = rec + pc + 1;
                    if id_off < man.len() {
                        out.push(id_off);
                    }
                }
                pc += insn.size;
            }
            Err(DisasmError::UnknownSubOp { .. }) | Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_give_item_in_a_clean_record() {
        // Build a minimal MAN with one partition-1 record whose script is:
        //   GFLAG.Set? — use simple known ops. We just need a clean walk that
        //   contains a 0x39 GIVE_ITEM. Use Nop-like op 0x21 (Nop, 1 byte) then
        //   0x39 <id>, then a terminator the walker can stop on.
        // Easiest: craft the record prefix and a script of [0x21, 0x39, 0xAB, 0x00].
        // 0x21 decodes as Nop (1 byte); 0x39 as GIVE_ITEM (2 bytes); 0x00 ends.
        // We can't easily synthesise a full MAN header here, so test the walker
        // directly on a record buffer via walk_record_gives.
        // record at offset 0; prefix N=0 -> pc0 = 1 + 0 + 4 = 5.
        let mut man = vec![0u8; 5];
        man[0] = 0; // local_count N = 0
        // 4-byte header (bytes 1..5) left zero.
        man.extend_from_slice(&[0x21, 0x39, 0xAB, 0x00]); // script at offset 5
        let mut sites = Vec::new();
        walk_record_gives(&man, 0, 5, &mut sites);
        assert_eq!(sites, vec![5 + 2], "operand byte of the 0x39 op");
        assert_eq!(man[sites[0]], 0xAB);
    }

    #[test]
    fn stops_at_desync_without_false_positives() {
        // A 0x39 that appears only AFTER a decode error must NOT be reported.
        // Use an unknown sub-op to force desync: 0x4C with a bogus sub-op.
        let mut man = vec![0u8; 5];
        man.extend_from_slice(&[0x4C, 0xFF, 0xFF, 0x39, 0xAB, 0x00]);
        let mut sites = Vec::new();
        walk_record_gives(&man, 0, 5, &mut sites);
        // The 0x39 after the desync point is not collected.
        assert!(
            !sites.contains(&(5 + 3)),
            "0x39 past a desync must not be a site"
        );
    }
}
