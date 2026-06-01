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
//! [`give_item_sites`] walks each partition-1 record's interaction script from
//! its true entry PC with the Track-1 field-VM disassembler
//! ([`legaia_asset::field_disasm`]).
//!
//! A chest's give-item op almost always comes **after** the inline dialogue that
//! announces it ("There is a {item} in the treasure chest!" → give → "{name} now
//! has the {item}!"). The dialogue is a stream of `0x1F`-lead glyph segments, not
//! field-VM bytecode, so the walk treats a decode error **at a `0x1F` byte** as a
//! dialogue segment to skip (advance past `0x1F`, consume glyphs to the
//! terminating `0x00`, treating `0xC?` top-nibble bytes as 2-byte escapes per the
//! dialog box-pack format), then resumes decoding. The control bytes between
//! segments (`0x24/0x25/0x48` Nop, `0x26` `JMP_REL`, `0x36` `SCENE_FADE`, …) are
//! genuine field-VM ops that stay in sync, so the walk reaches the post-dialogue
//! `0x39`. Any **other** decode error stops the walk. Each record's walk is
//! bounded to the next record's start offset, so it can never run off the end of
//! a record into unrelated data and mis-read a `0x39` data byte as an op.
//!
//! Multi-`0x39` runs are genuine multi-item gifts (e.g. a 10× consumable chest,
//! or the fishing starter kit of a rod + several lures), not false positives —
//! each `0x39 <id>` is its own 2-byte op in a coherent script flow.
//!
//! An earlier version stopped at the **first** `0x1F` (dialogue) instead of
//! skipping it, which silently missed the ~85% of give-item sites that sit after
//! their announcement text — including every chest in scenes whose first
//! interactable record opens with dialogue.
//!
//! **Display vs grant.** A chest's announcement renders the item *name* from a
//! separate dialogue token ([`ITEM_NAME_ESCAPE`] `<id>` — "There is a {item} in
//! the treasure chest!" / "{name} now has the {item}!"), which is a **different
//! byte** from the `0x39` give operand that actually adds the item to the bag.
//! Patching only the give operand grants the new item but leaves the text naming
//! the old one (verified in-game: the bag receives the new item while the message
//! still reads the original). So [`SceneChests::set_site`] rewrites the operand
//! **and** its item-name tokens together, keeping flavor text == grant. The
//! tokens are recovered per site by [`give_sites_and_display_tokens`].
//!
//! Edits are same-size (rewrite the id byte + its display tokens), then the MAN
//! is recompressed and written back exactly like the [encounter](crate::encounter)
//! path.

use legaia_asset::field_disasm;
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
    /// Per-site (parallel to [`Self::sites`]) absolute offsets of the **item-name
    /// display-token** argument bytes (`0xC2 <id>`) in the same chest record whose
    /// id equals that site's original give operand — the "There is a {item}…" /
    /// "{name} now has the {item}!" flavor text the game renders. Keeping these in
    /// sync with the give operand (via [`Self::set_site`]) makes the chest's
    /// announcement match the item it actually grants. Empty for sites whose
    /// dialogue doesn't name the item (e.g. multi-item gift chests).
    pub display_tokens: Vec<Vec<usize>>,
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
        let (sites, display_tokens) = give_sites_and_display_tokens(&decoded);
        if sites.is_empty() {
            return None;
        }
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget: consumed,
            decoded,
            sites,
            display_tokens,
        })
    }

    /// The current item id at each chest site, in `sites` order.
    pub fn current_items(&self) -> Vec<u8> {
        self.sites.iter().map(|&o| self.decoded[o]).collect()
    }

    /// Set chest site `k`'s granted item to `new_id`, rewriting both the
    /// `GIVE_ITEM` operand **and** its associated item-name display tokens so the
    /// chest's flavor text names the item it now grants. Out-of-range `k` is a
    /// no-op.
    pub fn set_site(&mut self, k: usize, new_id: u8) {
        let Some(&off) = self.sites.get(k) else {
            return;
        };
        if let Some(b) = self.decoded.get_mut(off) {
            *b = new_id;
        }
        for &t in &self.display_tokens[k] {
            if let Some(b) = self.decoded.get_mut(t) {
                *b = new_id;
            }
        }
    }

    /// Recompress the (mutated) MAN; `None` if it would overflow the footprint.
    pub fn repack(&self) -> Option<Vec<u8>> {
        let stream = legaia_lzs::compress(&self.decoded);
        (stream.len() <= self.compressed_budget).then_some(stream)
    }
}

/// The dialogue escape byte that renders an item's name (`0xC2 <item_id>`). The
/// announcement ("There is a {item}…") and the result ("{name} now has the
/// {item}!") both use it. Pinned across the chest corpus: of every `0xC?` 2-byte
/// dialogue escape inside chest records, only `0xC2`'s argument matches the
/// record's `GIVE_ITEM` operand (the other escapes are character-name / glyph
/// controls and never coincide with the give id).
const ITEM_NAME_ESCAPE: u8 = 0xC2;

/// Walk a decompressed MAN's partition-1 record scripts and return the absolute
/// offsets (within `man`) of every `GIVE_ITEM` (op `0x39`) operand byte reached
/// by a dialogue-skipping opcode-aware walk from each record's entry PC. Sites
/// are sorted and de-duplicated.
pub fn give_item_sites(man: &[u8]) -> Vec<usize> {
    give_sites_and_display_tokens(man).0
}

/// Like [`give_item_sites`], but also returns, per site (parallel to the sites
/// vec), the absolute offsets of the item-name display-token argument bytes
/// ([`ITEM_NAME_ESCAPE`] `<id>`) in the same chest record whose id equals that
/// site's give operand. Patching the give operand and these tokens together
/// keeps a chest's announcement text in sync with the item it grants.
///
/// Each display token is associated with the **nearest** give site in its record
/// (so a multi-item-gift record routes each `0xC2` to the right give), and only
/// when the token's id already equals that give's operand — so tokens that name a
/// *different* item the dialogue happens to mention are left untouched.
pub fn give_sites_and_display_tokens(man: &[u8]) -> (Vec<usize>, Vec<Vec<usize>>) {
    use std::collections::BTreeMap;

    let mut site_tokens: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let Ok(mf) = man_section::parse(man) else {
        return (Vec::new(), Vec::new());
    };
    // All record offsets across every partition, sorted, to bound each record's
    // walk to its own extent (the next record's start).
    let mut bounds: Vec<usize> = Vec::new();
    for part in &mf.partitions {
        for ri in 0..part.len() {
            if let Some(o) = mf.actor_placement_record_offset(ri, man.len()) {
                bounds.push(o);
            }
        }
    }
    bounds.sort_unstable();
    bounds.dedup();

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
        // Extent = up to the next record's start (or end of MAN).
        let end = bounds
            .iter()
            .copied()
            .find(|&o| o > rec)
            .unwrap_or(man.len());

        let (gives, tokens) = walk_record(man, rec, pc0, end);
        for &g in &gives {
            site_tokens.entry(g).or_default();
        }
        // Route each item-name token to its nearest give whose operand it names.
        for (t_off, t_id) in tokens {
            if let Some(&g) = gives
                .iter()
                .min_by_key(|&&g| (g as isize - t_off as isize).unsigned_abs())
                && man.get(g).copied() == Some(t_id)
            {
                site_tokens.entry(g).or_default().push(t_off);
            }
        }
    }

    let sites: Vec<usize> = site_tokens.keys().copied().collect();
    let display_tokens: Vec<Vec<usize>> = sites
        .iter()
        .map(|s| {
            let mut v = site_tokens[s].clone();
            v.sort_unstable();
            v.dedup();
            v
        })
        .collect();
    (sites, display_tokens)
}

/// Walk one record's interaction script from `pc0` to `end` (both relative to
/// `rec`), returning the absolute offsets of each `0x39` give-item operand byte
/// and the `(offset, id)` of each item-name display token ([`ITEM_NAME_ESCAPE`])
/// seen inside the record's inline-dialogue segments. A decode error at a `0x1F`
/// byte scans that dialogue segment (collecting its tokens) and continues; any
/// other error stops the walk.
fn walk_record(man: &[u8], rec: usize, pc0: usize, end: usize) -> (Vec<usize>, Vec<(usize, u8)>) {
    let script = &man[rec..end.min(man.len())];
    let mut gives = Vec::new();
    let mut tokens = Vec::new();
    let mut pc = pc0;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 100_000 || pc >= script.len() {
            break;
        }
        match field_disasm::decode(script, pc) {
            Ok(insn) => {
                if insn.size == 0 {
                    break;
                }
                // GIVE_ITEM is [0x39, item_id]; the id is the operand byte after
                // the opcode. Skip the cross-context (extended) form.
                if insn.opcode == 0x39 && insn.extended.is_none() {
                    let id_off = rec + pc + 1;
                    if id_off < man.len() {
                        gives.push(id_off);
                    }
                }
                pc += insn.size;
            }
            // A decode error AT a 0x1F byte is the start of an inline dialogue
            // segment (glyph text, not bytecode) — scan it for item-name tokens
            // and resume decoding past it.
            Err(_) if script.get(pc) == Some(&0x1F) => {
                pc = scan_dialogue_segment(script, pc, rec, &mut tokens);
            }
            Err(_) => break,
        }
    }
    (gives, tokens)
}

/// Scan one inline-dialogue `0x1F` segment beginning at `pc` (a `0x1F` byte),
/// pushing `(absolute_offset, id)` for each item-name display token
/// ([`ITEM_NAME_ESCAPE`] `<id>`), and returning the offset just past the
/// segment's terminating `0x00`. `0xC?` top-nibble bytes are 2-byte escapes (the
/// box-pack control codes), so a `0x00` inside an escape's argument does not
/// terminate the segment.
fn scan_dialogue_segment(
    script: &[u8],
    mut pc: usize,
    rec: usize,
    tokens: &mut Vec<(usize, u8)>,
) -> usize {
    pc += 1; // past the 0x1F lead.
    while pc < script.len() {
        let b = script[pc];
        if b == 0x00 {
            return pc + 1;
        }
        if b & 0xF0 == 0xC0 {
            if b == ITEM_NAME_ESCAPE && pc + 1 < script.len() {
                tokens.push((rec + pc + 1, script[pc + 1]));
            }
            pc += 2;
        } else {
            pc += 1;
        }
    }
    pc
}

/// Test-only thin wrapper retaining the original give-only walk signature.
#[cfg(test)]
fn walk_record_gives(man: &[u8], rec: usize, pc0: usize, end: usize, out: &mut Vec<usize>) {
    out.extend(walk_record(man, rec, pc0, end).0);
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
        walk_record_gives(&man, 0, 5, man.len(), &mut sites);
        assert_eq!(sites, vec![5 + 2], "operand byte of the 0x39 op");
        assert_eq!(man[sites[0]], 0xAB);
    }

    #[test]
    fn stops_at_desync_without_false_positives() {
        // A 0x39 that appears only AFTER a non-dialogue decode error must NOT be
        // reported. Use an unknown sub-op to force desync: 0x4C with a bogus
        // sub-op (not a 0x1F dialogue byte, so the walk stops rather than skips).
        let mut man = vec![0u8; 5];
        man.extend_from_slice(&[0x4C, 0xFF, 0xFF, 0x39, 0xAB, 0x00]);
        let mut sites = Vec::new();
        walk_record_gives(&man, 0, 5, man.len(), &mut sites);
        // The 0x39 after the desync point is not collected.
        assert!(
            !sites.contains(&(5 + 3)),
            "0x39 past a non-dialogue desync must not be a site"
        );
    }

    #[test]
    fn skips_inline_dialogue_and_finds_post_text_give() {
        // The give-item op sits AFTER an inline dialogue segment, exactly as a
        // real chest does. Script at offset 5:
        //   0x1F "Hi" 0xC1 0x00 0x00   (dialogue: glyphs + a 0xC? 2-byte escape
        //                               whose arg is 0x00, then the 0x00 end)
        //   0x39 0xAB                  (GIVE_ITEM after the text)
        //   0x00                       (next-record prefix-like low byte: stop)
        let mut man = vec![0u8; 5];
        man.extend_from_slice(&[
            0x1F, b'H', b'i', 0xC1, 0x00, 0x00, // dialogue segment
            0x39, 0xAB, // GIVE_ITEM item 0xAB
            0x00,
        ]);
        let give = 5 + 7; // operand byte of the 0x39
        assert_eq!(man[give], 0xAB);
        let mut sites = Vec::new();
        walk_record_gives(&man, 0, 5, man.len(), &mut sites);
        assert_eq!(sites, vec![give], "give-item after dialogue must be found");
    }

    #[test]
    fn record_extent_bound_stops_the_walk() {
        // A 0x39 beyond the record's extent (next record start) must NOT be a
        // site even though it would decode cleanly on an unbounded walk.
        let mut man = vec![0u8; 5];
        man.extend_from_slice(&[0x21, 0x39, 0xAB, 0x00]); // 0x39 at abs offset 6
        let mut sites = Vec::new();
        // Bound the walk to end at offset 6 (before the 0x39 op).
        walk_record_gives(&man, 0, 5, 6, &mut sites);
        assert!(
            sites.is_empty(),
            "give-item past the record extent is excluded"
        );
    }
}
