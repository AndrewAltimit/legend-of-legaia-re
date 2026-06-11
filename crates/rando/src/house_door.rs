//! Intra-town ("house / interior") door randomization.
//!
//! Unlike the [scene-transition doors](crate::door) (the `0x3F` named-scene-
//! change that leaves to another *scene*), entering a house in a town is an
//! **intra-scene reposition**: the field VM runs a `MOVE_TO` op that teleports
//! the player to an interior sub-area tile within the *same* scene (pinned by a
//! PCSX-Redux `probe.step.find_writer` trace — the writer lands in the field-VM
//! dispatcher `FUN_801de840` `case 0x23` at `0x801debc4`; see
//! `docs/tooling/pcsx-redux-automation.md`).
//!
//! **The door warp has a clean structural signature.** A house-door reposition
//! is not a plain `0x23 xb zb` (that form moves the *executing actor* — NPC /
//! prop / cutscene positioning). It is the **cross-context form**
//! `0xA3 0xF8 xb zb`: opcode `0x23 | 0x80` dispatched into the system/player
//! script channel `0xF8` (the channel pair documented for `FUN_8003C83C` /
//! `FUN_8001FD44`), i.e. "make the *player* MOVE_TO this tile". These ops live
//! in **partition-0 interaction records** of the scene MAN — the named
//! door-trigger records — and the records carry an explicit pairing convention
//! in their SJIS names: fullwidth `IN`/`OUT` (optionally digit-suffixed,
//! e.g. the Ratayu inn's one `IN` and three numbered `OUT`s), the kanji pair
//! entrance/exit (`0x93FC 0x8CFB` / `0x8F6F 0x8CFB`, the Sol city gates), or
//! the endpoint letters `A`/`B` (the tower elevators). The runtime-pinned
//! Mei's-house entry (`town01`, interior tile `(97, 54)`) is exactly the
//! `0xA3 0xF8 0x61 0x36` in the record named "...IN" — the anchor the
//! disc-gated classifier test (`house_door_classifier_real`) re-checks.
//!
//! The randomizer does a **per-scene, class-preserving shuffle**: `IN`-class
//! warp targets (interior landing tiles) permute among the scene's `IN` sites,
//! `OUT`-class targets (exterior doorstep tiles) among its `OUT` sites. Every
//! target stays a tile the scene's door system already uses, and every
//! `OUT`-class warp still lands at an exterior doorstep, so a player inside any
//! interior always exits back to the town proper — no interior-to-interior
//! cycle (no softlock) is constructible. The edit is a same-size 2-byte operand
//! swap (no MAN relocation — recompress in place like [`crate::encounter`]).
//!
//! Player warps that carry **no** door-name class (a handful of partition-0
//! story warps, e.g. the town01 intro "inside the house" reposition) and the
//! partition-1/2 cutscene player warps are deliberately left vanilla, as are
//! all plain (actor-context) `MOVE_TO`s — NPC and prop positions never move.
//!
//! Record walk notes: partition-0 records are `[u8 n][n*2 SJIS name][u8 attr]`
//! then bytecode (NOT the partition-1 `[n][n*2][4-byte header]` shape), and the
//! walk skips inline-dialogue `0x1F` segments exactly like [`crate::chest`]'s
//! ground-truthed give-item walk.

use legaia_asset::field_disasm::{self, InsnInfo};
use legaia_asset::{man_section, scene_asset_table};

use crate::drops::DropMode;
use crate::rng::SplitMix64;

const MAN_TYPE: u8 = 0x03;
/// The system/player script channel a cross-context op targets to act on the
/// player (`0xF8` — see the script-VM context channels in
/// `docs/subsystems/script-vm.md`).
const PLAYER_CHANNEL: u8 = 0xF8;
/// `0x23 MOVE_TO` with the cross-context bit set.
const MOVE_TO_EXTENDED: u8 = 0xA3;

/// Which side of a door passage a warp site is, from its record's name class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorSide {
    /// Warp INTO the interior / far endpoint (record named `IN` / entrance /
    /// `A`). Target = interior landing tile.
    In,
    /// Warp back OUT (record named `OUT` / exit / `B`). Target = exterior
    /// doorstep tile.
    Out,
}

/// One classified player door warp: a `0xA3 0xF8 xb zb` op in a partition-0
/// door record. Operand bytes live at `op_pc + 2` / `op_pc + 3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HouseDoorSite {
    /// Partition-0 record index carrying the warp.
    pub record: usize,
    /// Absolute offset of the `0xA3` opcode byte in the decoded MAN.
    pub op_pc: usize,
    /// Door side from the record-name class.
    pub side: DoorSide,
}

impl HouseDoorSite {
    /// Absolute offset of the `xb` operand byte.
    pub fn xb_off(&self) -> usize {
        self.op_pc + 2
    }
    /// Absolute offset of the `zb` operand byte.
    pub fn zb_off(&self) -> usize {
        self.op_pc + 3
    }
}

/// A scene bundle's MAN with its classified house-door warp sites.
pub struct SceneHouseDoors {
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Bytes the recompressed MAN must fit within (original compressed length).
    pub compressed_budget: usize,
    /// Decompressed MAN (mutate the operand bytes in place, then [`Self::repack`]).
    pub decoded: Vec<u8>,
    /// Classified door-warp sites, in record order.
    pub sites: Vec<HouseDoorSite>,
    /// Partition-0 player warps found but carrying no door-name class
    /// (story repositions) — counted for audit, never shuffled.
    pub unclassified: usize,
}

impl SceneHouseDoors {
    /// Locate a scene bundle's MAN and its classified door-warp sites, or
    /// `None` when the entry isn't a scene bundle, has no MAN, the MAN doesn't
    /// decode, or it carries no classified door warp.
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
        let (sites, unclassified) = door_warp_sites(&decoded);
        if sites.is_empty() {
            return None;
        }
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget: consumed,
            decoded,
            sites,
            unclassified,
        })
    }

    /// The current `(xb, zb)` operand byte pair at each site, in `sites` order.
    pub fn current_targets(&self) -> Vec<(u8, u8)> {
        self.sites
            .iter()
            .map(|s| (self.decoded[s.xb_off()], self.decoded[s.zb_off()]))
            .collect()
    }

    /// Shuffle the warp targets within each door-side class (multiset- and
    /// class-preserving): every `IN` site keeps a target some `IN` site in this
    /// scene already used, likewise for `OUT`. Returns the number of sites
    /// whose target actually changed. Deterministic from `(seed, entry_idx)`.
    pub fn shuffle(&mut self, seed: u64) -> usize {
        let mut rng =
            SplitMix64::new(seed ^ (self.entry_idx as u64).wrapping_mul(0x9E3779B97F4A7C15));
        let mut changed = 0;
        for side in [DoorSide::In, DoorSide::Out] {
            let idxs: Vec<usize> = (0..self.sites.len())
                .filter(|&i| self.sites[i].side == side)
                .collect();
            let orig: Vec<(u8, u8)> = idxs
                .iter()
                .map(|&i| {
                    let s = &self.sites[i];
                    (self.decoded[s.xb_off()], self.decoded[s.zb_off()])
                })
                .collect();
            let mut pairs = orig.clone();
            rng.shuffle(&mut pairs);
            // Avoid a no-op permutation: whenever the class has at least two
            // distinct targets, a single rotation guarantees movement (a
            // rotation has no fixed arrangement unless every target is
            // identical).
            let distinct: std::collections::HashSet<(u8, u8)> = orig.iter().copied().collect();
            if pairs == orig && distinct.len() > 1 {
                pairs.rotate_left(1);
            }
            for (&i, (xb, zb)) in idxs.iter().zip(pairs) {
                let (xo, zo) = (self.sites[i].xb_off(), self.sites[i].zb_off());
                if self.decoded[xo] != xb || self.decoded[zo] != zb {
                    self.decoded[xo] = xb;
                    self.decoded[zo] = zb;
                    changed += 1;
                }
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

/// Enumerate the classified player door warps in a decompressed MAN's
/// partition-0 records: `(classified sites, unclassified player-warp count)`.
fn door_warp_sites(man: &[u8]) -> (Vec<HouseDoorSite>, usize) {
    let Ok(mf) = man_section::parse(man) else {
        return (Vec::new(), 0);
    };
    let dro = mf.data_region_offset;
    // Every record start (all partitions) + section starts bound each walk.
    let mut bounds: Vec<usize> = Vec::new();
    for part in &mf.partitions {
        for &off in part {
            bounds.push(dro + off as usize);
        }
    }
    for s in &mf.sections {
        bounds.push(s.offset);
    }
    bounds.sort_unstable();
    bounds.dedup();

    let mut sites = Vec::new();
    let mut unclassified = 0usize;
    for (ri, &off) in mf.partitions[0].iter().enumerate() {
        let start = dro + off as usize;
        let Some(&n) = man.get(start) else { continue };
        // Partition-0 interaction record: [u8 n][n*2 SJIS name][u8 attr].
        let pc0 = 1 + n as usize * 2 + 1;
        let end = bounds
            .iter()
            .copied()
            .find(|&b| b > start)
            .unwrap_or(man.len());
        if start + pc0 >= end {
            continue;
        }
        let warps = player_warps_in_script(man, start + pc0, end);
        if warps.is_empty() {
            continue;
        }
        let name_end = (start + 1 + n as usize * 2).min(man.len());
        match name_class(&man[start + 1..name_end]) {
            Some(side) => {
                for op_pc in warps {
                    // Structural re-check before trusting the site.
                    if man.get(op_pc) == Some(&MOVE_TO_EXTENDED)
                        && man.get(op_pc + 1) == Some(&PLAYER_CHANNEL)
                        && op_pc + 3 < man.len()
                    {
                        sites.push(HouseDoorSite {
                            record: ri,
                            op_pc,
                            side,
                        });
                    }
                }
            }
            None => unclassified += warps.len(),
        }
    }
    (sites, unclassified)
}

/// Walk one record's bytecode from `pc` to `end`, returning the op offsets of
/// every cross-context player MOVE_TO (`0xA3 0xF8`). A decode error AT a `0x1F`
/// byte is an inline-dialogue segment — skip past its terminating `0x00` and
/// resume (the [`crate::chest`] walk rule); any other decode error ends the
/// walk.
fn player_warps_in_script(man: &[u8], mut pc: usize, end: usize) -> Vec<usize> {
    let script = &man[..end.min(man.len())];
    let mut out = Vec::new();
    let mut guard = 0usize;
    while pc < script.len() {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        match field_disasm::decode(script, pc) {
            Ok(insn) if insn.size > 0 => {
                if matches!(insn.info, InsnInfo::MoveTo { .. })
                    && insn.extended == Some(PLAYER_CHANNEL)
                {
                    out.push(insn.pc);
                }
                pc += insn.size;
            }
            Ok(_) => break,
            Err(_) if script.get(pc) == Some(&0x1F) => {
                pc = skip_dialogue_segment(script, pc);
            }
            Err(_) => break,
        }
    }
    out
}

/// Skip one inline-dialogue `0x1F` segment beginning at `pc`, returning the
/// offset just past its terminating `0x00`. `0xC?` top-nibble bytes are 2-byte
/// escapes whose argument byte can't terminate the segment.
fn skip_dialogue_segment(script: &[u8], mut pc: usize) -> usize {
    pc += 1;
    while pc < script.len() {
        let b = script[pc];
        if b == 0x00 {
            return pc + 1;
        }
        pc += if b & 0xF0 == 0xC0 { 2 } else { 1 };
    }
    pc
}

/// Classify a partition-0 record's SJIS name into a door side. The retail
/// conventions (all observed on disc, validated by
/// `house_door_classifier_real`):
///
/// - fullwidth `IN` (`0x8268 0x826D`) / `OUT` (`0x826E 0x8274 0x8273`),
///   optionally digit-suffixed (multi-exit interiors),
/// - the entrance / exit kanji pair (`0x93FC 0x8CFB` / `0x8F6F 0x8CFB`,
///   city gates),
/// - trailing fullwidth `A` (`0x8260`) / `B` (`0x8261`) endpoint letters
///   (elevator passages).
fn name_class(name_bytes: &[u8]) -> Option<DoorSide> {
    // The names are sequences of 2-byte SJIS codes; compare on u16 chars so a
    // pattern can't match across a character boundary.
    let chars: Vec<u16> = name_bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    const IN_SEQ: [u16; 2] = [0x8268, 0x826D];
    const OUT_SEQ: [u16; 3] = [0x826E, 0x8274, 0x8273];
    const ENTRANCE: [u16; 2] = [0x93FC, 0x8CFB];
    const EXIT: [u16; 2] = [0x8F6F, 0x8CFB];
    let contains = |needle: &[u16]| chars.windows(needle.len()).any(|w| w == needle);
    if contains(&OUT_SEQ) || contains(&EXIT) {
        return Some(DoorSide::Out);
    }
    if contains(&IN_SEQ) || contains(&ENTRANCE) {
        return Some(DoorSide::In);
    }
    match chars.last() {
        Some(0x8260) => Some(DoorSide::In),
        Some(0x8261) => Some(DoorSide::Out),
        _ => None,
    }
}

/// House-door randomization only supports `Shuffle` (class-preserving keeps
/// every target a tile the scene's door system uses); `Random` would place the
/// player off-map.
pub fn supported_mode(mode: DropMode) -> bool {
    matches!(mode, DropMode::Shuffle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(record: usize, op_pc: usize, side: DoorSide) -> HouseDoorSite {
        HouseDoorSite {
            record,
            op_pc,
            side,
        }
    }

    /// Hand-built scene: two IN warps + two OUT warps at known offsets.
    fn make_scene() -> SceneHouseDoors {
        let mut decoded = vec![0u8; 32];
        // IN sites: A3 F8 10 20 at 4; A3 F8 30 40 at 8.
        decoded[4..8].copy_from_slice(&[0xA3, 0xF8, 0x10, 0x20]);
        decoded[8..12].copy_from_slice(&[0xA3, 0xF8, 0x30, 0x40]);
        // OUT sites: A3 F8 50 60 at 16; A3 F8 70 11 at 20.
        decoded[16..20].copy_from_slice(&[0xA3, 0xF8, 0x50, 0x60]);
        decoded[20..24].copy_from_slice(&[0xA3, 0xF8, 0x70, 0x11]);
        SceneHouseDoors {
            entry_idx: 5,
            man_offset: 0,
            compressed_budget: 9999,
            decoded,
            sites: vec![
                site(0, 4, DoorSide::In),
                site(1, 8, DoorSide::In),
                site(2, 16, DoorSide::Out),
                site(3, 20, DoorSide::Out),
            ],
            unclassified: 0,
        }
    }

    #[test]
    fn shuffle_preserves_per_class_target_multisets() {
        for seed in 0u64..32 {
            let mut sd = make_scene();
            let before = sd.current_targets();
            sd.shuffle(seed);
            let after = sd.current_targets();
            // IN multiset preserved among IN sites.
            let (mut bi, mut ai) = (before[..2].to_vec(), after[..2].to_vec());
            bi.sort_unstable();
            ai.sort_unstable();
            assert_eq!(bi, ai, "seed {seed}: IN targets stay within the IN class");
            // OUT multiset preserved among OUT sites.
            let (mut bo, mut ao) = (before[2..].to_vec(), after[2..].to_vec());
            bo.sort_unstable();
            ao.sort_unstable();
            assert_eq!(bo, ao, "seed {seed}: OUT targets stay within the OUT class");
        }
    }

    #[test]
    fn two_distinct_targets_per_class_never_shuffle_to_identity() {
        for seed in 0u64..64 {
            let mut sd = make_scene();
            let changed = sd.shuffle(seed);
            assert_eq!(
                changed, 4,
                "seed {seed}: both classes have two distinct targets, all four must move"
            );
        }
    }

    #[test]
    fn name_class_recognises_the_retail_conventions() {
        // fullwidth IN / OUT (BE-encoded SJIS bytes).
        assert_eq!(
            name_class(&[0x96, 0xD8, 0x82, 0x68, 0x82, 0x6D]),
            Some(DoorSide::In)
        );
        assert_eq!(
            name_class(&[0x96, 0xD8, 0x82, 0x6E, 0x82, 0x74, 0x82, 0x73]),
            Some(DoorSide::Out)
        );
        // Digit-suffixed OUT (multi-exit interior).
        assert_eq!(
            name_class(&[0x8F, 0xE9, 0x82, 0x6E, 0x82, 0x74, 0x82, 0x73, 0x82, 0x50]),
            Some(DoorSide::Out)
        );
        // Entrance / exit kanji, mid-name.
        assert_eq!(
            name_class(&[0x93, 0xEC, 0x93, 0xFC, 0x8C, 0xFB, 0x83, 0x6F, 0x83, 0x43]),
            Some(DoorSide::In)
        );
        assert_eq!(
            name_class(&[0x93, 0xEC, 0x8F, 0x6F, 0x8C, 0xFB, 0x83, 0x6F, 0x83, 0x43]),
            Some(DoorSide::Out)
        );
        // Trailing fullwidth A / B (elevator endpoints).
        assert_eq!(
            name_class(&[0x83, 0x47, 0x83, 0x8C, 0x82, 0x50, 0x82, 0x60]),
            Some(DoorSide::In)
        );
        assert_eq!(
            name_class(&[0x83, 0x47, 0x83, 0x8C, 0x82, 0x50, 0x82, 0x61]),
            Some(DoorSide::Out)
        );
        // No class.
        assert_eq!(name_class(&[0x8E, 0xE5, 0x90, 0x6C]), None);
        assert_eq!(name_class(&[]), None);
    }
}
