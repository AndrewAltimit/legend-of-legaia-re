//! Intra-town ("house / interior") door randomization.
//!
//! Unlike the [scene-transition doors](crate::door) (the `0x3F` named-scene-
//! change that leaves to another *scene*), entering a house in a town is an
//! **intra-scene reposition**: the field VM runs a `0x23 MOVE_TO` op that
//! teleports the player to an interior sub-area tile within the *same* scene
//! (pinned by a PCSX-Redux `probe.step.find_writer` trace — the writer lands in
//! the field-VM dispatcher `FUN_801de840` `case 0x23` at `0x801debc4`; see
//! `docs/tooling/pcsx-redux-automation.md`). The destination tile is the op's
//! two operand bytes `[0x23][xb][zb]` (`tile = byte & 0x7F`).
//!
//! **Caveat — the op is shared.** `0x23 MOVE_TO` is *also* how NPC / cutscene
//! scripts move actors, and there is no clean structural marker separating door
//! warps from those. So this does the bounded-risk thing: a **per-scene,
//! multiset-preserving shuffle** of the non-sentinel `MOVE_TO` target tiles.
//! Every target stays a tile the scene already uses (no off-map placement), and
//! the edit is a same-size 2-byte operand swap (no MAN relocation — recompress
//! in place like [`crate::encounter`]). The effect is "intra-scene warps +
//! some actor positions scrambled within each town" — house doors lead to
//! different interiors, NPCs/cutscene actors may stand in swapped spots. It is
//! opt-in and experimental. The `(0x7F, 0x7F)` "here" sentinel is excluded.

use legaia_asset::man_edit::{self, MoveToSite};
use legaia_asset::scene_asset_table;

use crate::drops::DropMode;
use crate::rng::SplitMix64;

const MAN_TYPE: u8 = 0x03;
/// The `(tile_x, tile_z)` "stay here" sentinel that pervades MOVE_TO data; not a
/// door, never shuffled.
const SENTINEL: (u8, u8) = (0x7F, 0x7F);

/// A scene bundle's MAN with its shuffleable `0x23 MOVE_TO` target sites.
pub struct SceneHouseDoors {
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Bytes the recompressed MAN must fit within (original compressed length).
    pub compressed_budget: usize,
    /// Decompressed MAN (mutate the operand bytes in place, then [`Self::repack`]).
    pub decoded: Vec<u8>,
    /// Non-sentinel MOVE_TO sites (op offset + operand byte offsets).
    pub sites: Vec<HouseDoorSite>,
}

/// One shuffleable MOVE_TO site: where its `[xb][zb]` operand bytes live in the
/// decoded MAN, and the tile they currently encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HouseDoorSite {
    pub op_pc: usize,
    /// Absolute offset of the `xb` operand byte in `decoded`.
    pub xb_off: usize,
    /// Absolute offset of the `zb` operand byte in `decoded`.
    pub zb_off: usize,
}

impl SceneHouseDoors {
    /// Locate a scene bundle's MAN and its non-sentinel MOVE_TO sites, or `None`
    /// when the entry isn't a scene bundle, has no MAN, the MAN doesn't decode,
    /// or it carries no shuffleable site.
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
        let sites: Vec<HouseDoorSite> = man_edit::move_to_sites(&decoded)
            .into_iter()
            .filter(|s| s.tile() != SENTINEL)
            .filter_map(|s| operand_offsets(&decoded, &s))
            .collect();
        // Need at least two distinct targets for a shuffle to do anything.
        if sites.len() < 2 {
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

    /// The current `(xb, zb)` operand byte pair at each site, in `sites` order.
    pub fn current_targets(&self) -> Vec<(u8, u8)> {
        self.sites
            .iter()
            .map(|s| (self.decoded[s.xb_off], self.decoded[s.zb_off]))
            .collect()
    }

    /// Shuffle the target tiles among this scene's sites (multiset-preserving):
    /// each site keeps a `(xb, zb)` pair that some site in this scene already
    /// used, so no off-map tile is introduced. Returns the number of sites whose
    /// target actually changed. Deterministic from `(seed, entry_idx)`.
    pub fn shuffle(&mut self, seed: u64) -> usize {
        let mut pairs = self.current_targets();
        let mut rng =
            SplitMix64::new(seed ^ (self.entry_idx as u64).wrapping_mul(0x9E3779B97F4A7C15));
        rng.shuffle(&mut pairs);
        let mut changed = 0;
        for (s, (xb, zb)) in self.sites.iter().zip(pairs) {
            if self.decoded[s.xb_off] != xb || self.decoded[s.zb_off] != zb {
                self.decoded[s.xb_off] = xb;
                self.decoded[s.zb_off] = zb;
                changed += 1;
            }
        }
        changed
    }

    /// Recompress the (mutated) MAN; `None` if it would overflow the footprint
    /// (these are same-size edits, so it fits whenever the original did).
    pub fn repack(&self) -> Option<Vec<u8>> {
        let stream = legaia_lzs::compress(&self.decoded);
        (stream.len() <= self.compressed_budget).then_some(stream)
    }
}

/// Resolve a MOVE_TO site's operand byte offsets in the decoded MAN. The op is
/// `[0x23][xb][zb]` (header 1); skip the rare `0x80`-extended form (header 2),
/// whose operand layout differs and which is never a door.
fn operand_offsets(decoded: &[u8], s: &MoveToSite) -> Option<HouseDoorSite> {
    // Confirm the base (non-extended) opcode byte and operand bounds.
    if decoded.get(s.op_pc).copied()? != 0x23 {
        return None;
    }
    let xb_off = s.op_pc + 1;
    let zb_off = s.op_pc + 2;
    if zb_off >= decoded.len() {
        return None;
    }
    Some(HouseDoorSite {
        op_pc: s.op_pc,
        xb_off,
        zb_off,
    })
}

/// House-door randomization only supports `Shuffle` (multiset-preserving keeps
/// every target a valid scene tile); `Random` would place actors off-map.
pub fn supported_mode(mode: DropMode) -> bool {
    matches!(mode, DropMode::Shuffle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_preserves_target_multiset_within_scene() {
        // Two non-sentinel sites at known offsets in a hand-built decoded buf.
        let mut decoded = vec![0u8; 16];
        // site A: 0x23 0x10 0x20 ; site B: 0x23 0x30 0x40
        decoded[4] = 0x23;
        decoded[5] = 0x10;
        decoded[6] = 0x20;
        decoded[8] = 0x23;
        decoded[9] = 0x30;
        decoded[10] = 0x40;
        let mut sd = SceneHouseDoors {
            entry_idx: 5,
            man_offset: 0,
            compressed_budget: 9999,
            decoded,
            sites: vec![
                HouseDoorSite {
                    op_pc: 4,
                    xb_off: 5,
                    zb_off: 6,
                },
                HouseDoorSite {
                    op_pc: 8,
                    xb_off: 9,
                    zb_off: 10,
                },
            ],
        };
        let before = sd.current_targets();
        sd.shuffle(0xABCD);
        let mut a = before.clone();
        let mut b = sd.current_targets();
        a.sort();
        b.sort();
        assert_eq!(a, b, "shuffle preserves the per-scene target multiset");
    }
}
