//! Retail-shaped encounter record parser.
//!
//! PORT: FUN_801DA51C
//!
//! On retail hardware the field VM installs a pointer to one of these records
//! at `actor[+0x94]` when an "encounter armed" op fires; the world-map /
//! field entity tick (`FUN_801DA51C`, body at `0x801DA620..0x801DA678`)
//! reads it back, copies up to four monster ids into the global formation
//! cell at `0x8007BD0C..0x8007BD0F`, and then yields control to the battle
//! scene loader (`FUN_800520F0`).
//!
//! This module mirrors that on-disc record shape so engines that already
//! know which byte slice carries an encounter record can decode the four
//! monster ids without re-reading the disassembly.
//!
//! ## Where the bytes come from
//!
//! The dispatcher install handlers all assign `s0 = param_1 + param_2`
//! (== `bytecode_buffer + pc_offset` == current opcode pointer in the
//! field-VM script). So the bytes the reader consumes at `+0x3..+0x4+N`
//! are the **trailing operand bytes of the install opcode itself**, inline
//! in the per-scene field-VM script bytecode. There is no separate on-disc
//! encounter-record table; each scripted encounter is its own opcode site
//! in a [`scene_v12_table`](../../legaia-asset/src/scene_v12_table.rs)
//! sister pair (or its `scene_event_scripts` sibling). See
//! `docs/formats/encounter.md` for the install-opcode catalogue
//! (0x37/0x41, 0x38, 0x43, 0x47, 0x4C). Random-encounter triggers
//! (rate-roll on `_DAT_8007B5F8`) may populate the formation cell via a
//! different path that bypasses `actor[+0x94]` - that's an open thread.
//! The [`EncounterRegistry`](crate::encounter_registry) abstraction is a
//! clean-room composition layer that lets engines synthesize per-scene
//! tables until disc-side decoding catches up.
//!
//! ## Layout (4-byte minimum, monster-count-dependent total)
//!
//! ```text
//! +0x00  u8[3]  opcode header        ; install-opcode + selector + flag,
//!                                    ; consumed by the script-VM dispatcher
//!                                    ; before the encounter reader runs
//! +0x03  u8     monster_count        ; 0..=4
//! +0x04  u8[N]  monster_ids          ; N == monster_count
//! ```
//!
//! Bytes after `0x04 + monster_count` are not consumed by the formation
//! copy; they're whatever script-VM bytecode follows the install opcode.
//!
//! See [`docs/formats/encounter.md`](../../../docs/formats/encounter.md) for
//! the full provenance.
//! REF: FUN_800520F0

use serde::{Deserialize, Serialize};

/// Maximum number of monster slots in the global formation cell.
///
/// Retail clears all four slots before the copy and only writes the first
/// `monster_count` of them; trailing slots stay zero. Records that claim a
/// `monster_count > 4` are rejected as malformed.
pub const FORMATION_SLOTS: usize = 4;

/// Byte offset of the monster-count field inside an encounter record.
pub const COUNT_OFFSET: usize = 0x3;

/// Byte offset of the first monster id inside an encounter record.
pub const IDS_OFFSET: usize = 0x4;

/// Monster id of the opening training opponent in Rim Elm (monster archive
/// id `0x4F`, "Tetsu"). The game's first battle is a scripted single-monster
/// formation built from this id.
///
/// Pinned from the training-fight save-state corpus: the global formation
/// cell at `0x8007BD0C` is empty (`00 00 00 00`) in the pre-battle field
/// state and reads `4F 00 00 00` from battle-load onward - a lone monster in
/// slot 0. See [`docs/formats/encounter.md`](../../../docs/formats/encounter.md).
pub const RIM_ELM_TRAINING_OPPONENT_ID: u8 = 0x4F;

/// Index of the Rim Elm Tetsu tutorial fight in town01's per-scene MAN
/// formation table - a lone [`RIM_ELM_TRAINING_OPPONENT_ID`] monster.
///
/// The scripted carrier entity installs the fight by selecting this index into
/// the per-scene formation table (it is **not** an inline `[count][id]` literal
/// in the field-VM script - the id `0x4F` resolves through the indexed table).
/// Verified against the live "Come at me!" save state: the in-RAM formation
/// table reads `[04][07][0a][3f 3e 3e 3e][4f][0a 0a][3d 3d]`, byte-identical to
/// the engine's MAN parse, so the Tetsu row is index 4. See
/// [`docs/formats/encounter.md`](../../../docs/formats/encounter.md).
pub const RIM_ELM_TRAINING_FORMATION_ID: u16 = 4;

/// town01 partition-1 placement that carries the Rim Elm sparring fight - the
/// NPC whose talk-menu installs the lone-[`RIM_ELM_TRAINING_OPPONENT_ID`]
/// formation ([`RIM_ELM_TRAINING_FORMATION_ID`]).
///
/// Pinned structurally from the town01 MAN: a single partition-1 actor
/// placement sits at this tile carrying this model and a multi-page inline
/// dialog block (the sparring talk menu and its battle-trigger branch). It is
/// the only on-map placement whose inline text runs to that length. The model
/// byte and tile are stable disc invariants; the menu *text* is Sony content
/// and is not reproduced here.
///
/// This resolves the "which MAN-placed entity is the training-fight carrier"
/// half of the opening-battle thread. Note the carrier is **not** identified by
/// the opcode-decoded interact selector: a field interaction record desyncs
/// under linear disassembly (its embedded message bytes alias opcodes), so the
/// entity is recognised by its inline dialog block, found structurally. See
/// [`crate::man_field_scripts::first_inline_dialog_offset`] and
/// [`docs/formats/encounter.md`](../../../docs/formats/encounter.md).
pub const RIM_ELM_SPARRING_CARRIER_TILE: (u8, u8) = (76, 65);

/// Model byte of the [`RIM_ELM_SPARRING_CARRIER_TILE`] placement.
pub const RIM_ELM_SPARRING_CARRIER_MODEL: u8 = 0x6A;

/// The sparring carrier's **runtime tutorial position** `(world_x, world_z)`,
/// distinct from its MAN placement tile ([`RIM_ELM_SPARRING_CARRIER_TILE`] =
/// `(76, 65)` → world `(9792, 8384)`).
///
/// The placement tile is the partner's *post-tutorial* village spot, in a
/// town01 sub-area not walk-reachable from the cold-boot spawn (tile 20). For
/// the opening tutorial fight the opening sequence repositions the partner next
/// to Vahn. This value is **derived from the scene bytecode**: town01 MAN
/// partition-1 record `P1[10]` (`start 0x01370`) carries, at record offsets
/// `+0x1D` and `+0x28`, the field-VM op `4C 51 15 0E 07 22` = NpcRun (run-to-
/// tile) with `x_enc=21, z_enc=14`, i.e. tile `(21, 14)` → world
/// `(21*128+64, 14*128+64)` = `(2752, 1856)`. The two consecutive identical ops
/// are the story-flag two-branch scene-entry prologue that hops the carrier
/// next to the spawn. The clean-room cold boot enters town01 free-roam without
/// replaying that reposition, so a driver that needs the partner where retail's
/// tutorial puts it uses this position. Cross-check: the live actor at this
/// position resolves (via `actor[+0x90]`) to the `(76, 65)` / model `0x6A`
/// placement record - same carrier, repositioned.
pub const RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS: (i16, i16) = (2752, 1856);

/// Decoded encounter record.
///
/// `monster_ids` always has length `FORMATION_SLOTS`; trailing slots beyond
/// `count` carry zero (matching the cleared formation cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterRecord {
    /// Number of active monster slots (`0..=4`).
    pub count: u8,
    /// Monster ids in slot order. Slots `count..` are zero.
    pub monster_ids: [u8; FORMATION_SLOTS],
}

impl EncounterRecord {
    /// Empty record - zero monsters, all slots cleared.
    pub const EMPTY: Self = Self {
        count: 0,
        monster_ids: [0; FORMATION_SLOTS],
    };

    /// Construct directly from a `count` + ids list. Returns `None` if
    /// `count > FORMATION_SLOTS` or `count > monster_ids.len()`.
    pub fn new(count: u8, monster_ids: &[u8]) -> Option<Self> {
        if count as usize > FORMATION_SLOTS {
            return None;
        }
        if count as usize > monster_ids.len() {
            return None;
        }
        let mut ids = [0u8; FORMATION_SLOTS];
        for (slot, id) in monster_ids.iter().take(count as usize).enumerate() {
            ids[slot] = *id;
        }
        Some(Self {
            count,
            monster_ids: ids,
        })
    }

    /// The opening Rim Elm training fight: a lone opponent
    /// ([`RIM_ELM_TRAINING_OPPONENT_ID`]). This is the first scripted battle
    /// the player reaches; the formation is a single monster in slot 0.
    pub fn rim_elm_training() -> Self {
        Self::new(1, &[RIM_ELM_TRAINING_OPPONENT_ID]).expect("1-monster record is always valid")
    }

    /// Parse from a raw byte slice.
    ///
    /// Accepts any slice at least `IDS_OFFSET + monster_count` bytes long.
    /// Reads:
    /// - `count = bytes[COUNT_OFFSET]`
    /// - `monster_ids[i] = bytes[IDS_OFFSET + i]` for `i in 0..count`
    ///
    /// Returns `None` when:
    /// - The slice is shorter than `IDS_OFFSET` (no count field).
    /// - `count > FORMATION_SLOTS`.
    /// - The slice is shorter than `IDS_OFFSET + count` (truncated ids).
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < IDS_OFFSET {
            return None;
        }
        let count = bytes[COUNT_OFFSET];
        if count as usize > FORMATION_SLOTS {
            return None;
        }
        if bytes.len() < IDS_OFFSET + count as usize {
            return None;
        }
        let mut monster_ids = [0u8; FORMATION_SLOTS];
        let n = count as usize;
        monster_ids[..n].copy_from_slice(&bytes[IDS_OFFSET..IDS_OFFSET + n]);
        Some(Self { count, monster_ids })
    }

    /// Iterate the active (`< count`) monster ids.
    pub fn active_ids(&self) -> impl Iterator<Item = u8> + '_ {
        self.monster_ids.iter().copied().take(self.count as usize)
    }

    /// `true` when no monsters spawn this round (count == 0).
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Apply this record to the 4-byte formation cell at `0x8007BD0C`.
    ///
    /// Mirrors the reader at `0x801DA620..0x801DA678`: clears all four
    /// slots, then writes `monster_ids[0..count]` into the first `count`
    /// slots. Trailing slots stay zero.
    pub fn apply_to_formation_cell(&self, cell: &mut [u8; FORMATION_SLOTS]) {
        *cell = [0; FORMATION_SLOTS];
        let n = self.count as usize;
        cell[..n].copy_from_slice(&self.monster_ids[..n]);
    }

    /// Convert this record into a [`FormationDef`] suitable for the runtime
    /// battle session.
    ///
    /// The retail engine identifies a battle by the bytes of the formation
    /// cell - there is no separate "formation id" on disc. We synthesize one
    /// from the cell bytes (`monster_ids` packed little-endian into the low
    /// 32 bits, then folded into the u16 id space) so engines can register
    /// and look up the formation in [`FormationTable`].
    ///
    /// Engines that need a stable / human-meaningful id should prefer
    /// constructing the [`FormationDef`] manually; this method is for the
    /// "decode an on-disc record and play it" path.
    pub fn to_formation_def(
        &self,
        label: impl Into<String>,
    ) -> crate::monster_catalog::FormationDef {
        use crate::monster_catalog::{FormationDef, FormationSlot};
        let slots: Vec<FormationSlot> = self
            .active_ids()
            .map(|id| FormationSlot::new(id as u16))
            .collect();
        let formation_id = self.synthetic_formation_id();
        FormationDef::new(formation_id, slots).with_label(label)
    }

    /// Synthetic formation id derived from `monster_ids`. Two records with
    /// identical id sequences map to the same formation id.
    pub fn synthetic_formation_id(&self) -> u16 {
        // Pack the four slot bytes into a u32 then xor-fold to u16 to keep
        // collisions rare across sane retail formations.
        let packed = u32::from_le_bytes(self.monster_ids);
        ((packed & 0xFFFF) ^ (packed >> 16)) as u16
    }
}

/// Byte offset of the encounter-record pointer slot inside a field actor
/// record. The script-VM ops write the record pointer here; the world-map
/// entity tick reads it back.
///
/// In retail this is `*(s1 + 0x94)` where `s1` is the actor record passed
/// to `FUN_801DA51C`.
pub const ACTOR_ENCOUNTER_RECORD_PTR_OFFSET: usize = 0x94;

/// Byte offset of the per-actor "encounter armed" flag word.
///
/// The script-VM ops set bit `0x400` of `actor[+0x10]` when installing the
/// encounter pointer. The reader checks the flag before consuming the
/// pointer (so a stale pointer with bit `0x400` clear is ignored).
pub const ACTOR_STATE_FLAG_OFFSET: usize = 0x10;

/// Bit mask raised in `actor[+0x10]` when a pointer is installed at
/// `actor[+ACTOR_ENCOUNTER_RECORD_PTR_OFFSET]`.
pub const ACTOR_ENCOUNTER_ARMED_BIT: u32 = 0x400;

/// Formation-cell fill produced by the **battle-id** path.
///
/// The `actor[+0x94]` record above is one of two ways the formation cell at
/// `0x8007BD0C` gets populated. The other is a global battle id
/// (`DAT_8007B7FC`) expanded by [`expand_battle_id`]. This type carries both
/// halves of that expansion: the four-slot cell, and the separate 3-byte
/// staging block at `DAT_8007BD10..0x12` that only the three "bespoke"
/// ids write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BattleIdFormation {
    /// The four formation slots (`DAT_8007BD0C..0x0F`).
    pub cell: [u8; FORMATION_SLOTS],
    /// The expansion staging block (`DAT_8007BD10..0x12`).
    pub expansion: [u8; 3],
    /// `true` when the id-`0` fallback fired, which also clears the
    /// boss-transition arm `DAT_8007B64A`.
    pub clears_boss_arm: bool,
}

/// Battle-id bands that take the "bespoke" expansion branch.
///
/// Ids inside any of these ranges collapse the cell to a **lone** id
/// (`[id, 0, 0, 0]`) instead of the plain 3-slot fill, and the three ids
/// `0xA2..=0xA4` additionally seed the expansion block.
pub const BATTLE_ID_EXPANSION_BANDS: [core::ops::RangeInclusive<u8>; 4] =
    [0x07..=0x09, 0x49..=0x4D, 0x88..=0x8B, 0xA2..=0xFF];

/// Monster id the id-`0` fallback fills every slot with.
pub const BATTLE_ID_FALLBACK_MONSTER: u8 = 4;

/// Expand a battle id into the formation cell.
///
/// The **alternate** formation source to the `actor[+0x94]` record path.
/// Retail reads the transient global `DAT_8007B7FC` and writes
/// `0x8007BD0C..0x0F`:
///
/// | Battle id | Resulting cell | Notes |
/// |---|---|---|
/// | `0` | `[4, 4, 4, 4]` | Fallback; also clears `DAT_8007B64A`. |
/// | in an [expansion band](BATTLE_ID_EXPANSION_BANDS) | `[id, 0, 0, 0]` | Lone monster / boss. |
/// | any other non-zero | `[id, id, id, 0]` | Plain 3-slot fill. |
///
/// The **cell shape** is what distinguishes the two paths at a glance: this
/// writes three slots for a plain id, whereas the record path writes exactly
/// as many slots as the record's `count`.
///
/// Retail's slot 1 (`DAT_8007BD0D`) is not written directly - it is copied
/// from slot 2 (`DAT_8007BD0E`) as the function's last act, which is why the
/// banded and fallback cases stay self-consistent.
///
/// See [`docs/formats/encounter.md`](../../../docs/formats/encounter.md#scripted-battle-id-path-fun_8005567c).
// PORT: FUN_8005567c
pub fn expand_battle_id(battle_id: u8) -> BattleIdFormation {
    // Retail seeds slots 0 and 2 with the raw id, then patches.
    let mut slot0 = battle_id;
    let mut slot2 = battle_id;
    let mut expansion = [0u8; 3];

    if BATTLE_ID_EXPANSION_BANDS
        .iter()
        .any(|b| b.contains(&battle_id))
    {
        slot2 = 0;
        // Only these three ids seed the expansion block; the rest of the
        // banded ids leave it untouched.
        match battle_id {
            0xA2 => expansion = [1, 0, 0],
            0xA3 => expansion = [3, 0, 0],
            0xA4 => expansion = [2, 0, 0],
            _ => {}
        }
    }

    let mut slot3 = 0u8;
    let clears_boss_arm = battle_id == 0;
    if clears_boss_arm {
        slot0 = BATTLE_ID_FALLBACK_MONSTER;
        slot2 = BATTLE_ID_FALLBACK_MONSTER;
        slot3 = BATTLE_ID_FALLBACK_MONSTER;
    }

    // Slot 1 mirrors slot 2 (retail's trailing `DAT_8007BD0D = DAT_8007BD0E`).
    BattleIdFormation {
        cell: [slot0, slot2, slot2, slot3],
        expansion,
        clears_boss_arm,
    }
}

/// Monster id that arms the scripted boss transition when it lands in slot 0.
///
/// Battle-init checks the cell's first slot against this id (retail compares
/// the signed `char` against `-0x4B`) and sets `DAT_8007B64A = 2`.
pub const BOSS_TRANSITION_MONSTER_ID: u8 = 0xB5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_record() {
        let bytes = [0u8, 0, 0, 0]; // count = 0, no ids needed
        let r = EncounterRecord::parse(&bytes).unwrap();
        assert_eq!(r.count, 0);
        assert_eq!(r.monster_ids, [0, 0, 0, 0]);
        assert!(r.is_empty());
    }

    #[test]
    fn parse_two_slot_record() {
        // mc2 retail: 04 04 00 00 in formation cell after copy.
        // Source record: [_, _, _, 2, 4, 4, ...]
        let bytes = [0xAA, 0xBB, 0xCC, 0x02, 0x04, 0x04, 0xDE, 0xAD];
        let r = EncounterRecord::parse(&bytes).unwrap();
        assert_eq!(r.count, 2);
        assert_eq!(r.monster_ids, [0x04, 0x04, 0, 0]);
    }

    #[test]
    fn parse_full_four_slot_record() {
        let bytes = [0, 0, 0, 4, 0x0A, 0x0D, 0x11, 0x12];
        let r = EncounterRecord::parse(&bytes).unwrap();
        assert_eq!(r.count, 4);
        assert_eq!(r.monster_ids, [0x0A, 0x0D, 0x11, 0x12]);
    }

    #[test]
    fn parse_rejects_count_above_max() {
        let bytes = [0, 0, 0, 5, 0x01, 0x02, 0x03, 0x04, 0x05];
        assert!(EncounterRecord::parse(&bytes).is_none());
    }

    #[test]
    fn parse_rejects_truncated_slice() {
        // count says 3 but only 2 ids available
        let bytes = [0, 0, 0, 3, 0x01, 0x02];
        assert!(EncounterRecord::parse(&bytes).is_none());
    }

    #[test]
    fn parse_rejects_too_short_for_count_field() {
        let bytes = [0u8; 3];
        assert!(EncounterRecord::parse(&bytes).is_none());
    }

    #[test]
    fn new_clamps_count_to_provided_ids() {
        let r = EncounterRecord::new(2, &[0x05, 0x06, 0x07, 0x08]).unwrap();
        assert_eq!(r.count, 2);
        // Slots beyond count are zero, regardless of what the input slice held.
        assert_eq!(r.monster_ids, [0x05, 0x06, 0, 0]);
    }

    #[test]
    fn new_rejects_count_above_max() {
        assert!(EncounterRecord::new(5, &[0u8; 5]).is_none());
    }

    #[test]
    fn new_rejects_count_above_slice_len() {
        assert!(EncounterRecord::new(3, &[0x01, 0x02]).is_none());
    }

    #[test]
    fn active_ids_iterates_only_active_slots() {
        let r = EncounterRecord {
            count: 2,
            monster_ids: [1, 2, 3, 4],
        };
        let v: Vec<u8> = r.active_ids().collect();
        assert_eq!(v, vec![1, 2]);
    }

    #[test]
    fn active_ids_empty_when_count_zero() {
        assert!(EncounterRecord::EMPTY.active_ids().next().is_none());
    }

    #[test]
    fn apply_to_formation_cell_clears_then_writes() {
        // Cell already has stale data from previous battle.
        let mut cell = [0xAA, 0xBB, 0xCC, 0xDD];
        let r = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x04, 0, 0],
        };
        r.apply_to_formation_cell(&mut cell);
        assert_eq!(cell, [0x04, 0x04, 0, 0]);
    }

    #[test]
    fn apply_empty_record_clears_cell() {
        let mut cell = [1, 2, 3, 4];
        EncounterRecord::EMPTY.apply_to_formation_cell(&mut cell);
        assert_eq!(cell, [0, 0, 0, 0]);
    }

    #[test]
    fn parse_roundtrips_through_apply() {
        // Record from mc3: 0a 0d 00 00 in formation cell.
        let bytes = [0xFF, 0xFF, 0xFF, 0x02, 0x0A, 0x0D, 0xFF, 0xFF];
        let r = EncounterRecord::parse(&bytes).unwrap();
        let mut cell = [0u8; 4];
        r.apply_to_formation_cell(&mut cell);
        assert_eq!(cell, [0x0A, 0x0D, 0, 0]);
    }

    #[test]
    fn to_formation_def_yields_one_slot_per_active_id() {
        let r = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x07, 0, 0],
        };
        let f = r.to_formation_def("test_scene");
        assert_eq!(f.slots.len(), 2);
        assert_eq!(f.slots[0].monster_id, 4);
        assert_eq!(f.slots[1].monster_id, 7);
        assert_eq!(f.label, "test_scene");
    }

    #[test]
    fn to_formation_def_empty_record_has_no_slots() {
        let f = EncounterRecord::EMPTY.to_formation_def("empty");
        assert!(f.slots.is_empty());
    }

    #[test]
    fn rim_elm_training_is_a_lone_opponent() {
        let r = EncounterRecord::rim_elm_training();
        assert_eq!(r.count, 1);
        assert_eq!(r.monster_ids[0], RIM_ELM_TRAINING_OPPONENT_ID);
        assert_eq!(r.monster_ids[0], 0x4F);
        assert_eq!(r.monster_ids[1..], [0, 0, 0]);
        // Resolves to a single-slot formation carrying the training id.
        let f = r.to_formation_def("town01");
        assert_eq!(f.slots.len(), 1);
        assert_eq!(f.slots[0].monster_id, 0x4F);
    }

    #[test]
    fn synthetic_formation_id_stable_for_same_ids() {
        let a = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x04, 0, 0],
        };
        let b = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x04, 0, 0],
        };
        assert_eq!(a.synthetic_formation_id(), b.synthetic_formation_id());
    }

    #[test]
    fn synthetic_formation_id_differs_for_different_ids() {
        let a = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x04, 0, 0],
        };
        let b = EncounterRecord {
            count: 2,
            monster_ids: [0x0A, 0x0D, 0, 0],
        };
        assert_ne!(a.synthetic_formation_id(), b.synthetic_formation_id());
    }

    // --- battle-id path (FUN_8005567c) ---

    #[test]
    fn plain_battle_id_fills_three_slots() {
        // The signature shape of the battle-id path: slots 0/1/2 all carry
        // the id, slot 3 stays clear.
        let f = expand_battle_id(0x4F);
        assert_eq!(f.cell, [0x4F, 0x4F, 0x4F, 0]);
        assert_eq!(f.expansion, [0, 0, 0]);
        assert!(!f.clears_boss_arm);
    }

    #[test]
    fn battle_id_zero_falls_back_to_all_fours() {
        let f = expand_battle_id(0);
        assert_eq!(f.cell, [4, 4, 4, 4], "every slot, slot 1 included");
        assert!(f.clears_boss_arm);
    }

    #[test]
    fn banded_ids_collapse_to_a_lone_monster() {
        // One id from each of the four bespoke bands.
        for id in [0x07, 0x49, 0x88, 0xA2] {
            let f = expand_battle_id(id);
            assert_eq!(f.cell, [id, 0, 0, 0], "id {id:#04x} is a lone spawn");
        }
    }

    #[test]
    fn band_edges_are_inclusive_and_neighbours_are_not_banded() {
        // 0x07..=0x09 - check both edges and both neighbours.
        assert_eq!(expand_battle_id(0x06).cell, [0x06, 0x06, 0x06, 0]);
        assert_eq!(expand_battle_id(0x07).cell, [0x07, 0, 0, 0]);
        assert_eq!(expand_battle_id(0x09).cell, [0x09, 0, 0, 0]);
        assert_eq!(expand_battle_id(0x0A).cell, [0x0A, 0x0A, 0x0A, 0]);
        // 0x49..=0x4D
        assert_eq!(expand_battle_id(0x48).cell, [0x48, 0x48, 0x48, 0]);
        assert_eq!(expand_battle_id(0x4D).cell, [0x4D, 0, 0, 0]);
        assert_eq!(expand_battle_id(0x4E).cell, [0x4E, 0x4E, 0x4E, 0]);
        // 0x88..=0x8B
        assert_eq!(expand_battle_id(0x87).cell, [0x87, 0x87, 0x87, 0]);
        assert_eq!(expand_battle_id(0x8B).cell, [0x8B, 0, 0, 0]);
        assert_eq!(expand_battle_id(0x8C).cell, [0x8C, 0x8C, 0x8C, 0]);
        // 0xA2..=0xFF runs to the top of the id space.
        assert_eq!(expand_battle_id(0xA1).cell, [0xA1, 0xA1, 0xA1, 0]);
        assert_eq!(expand_battle_id(0xFF).cell, [0xFF, 0, 0, 0]);
    }

    #[test]
    fn only_three_ids_seed_the_expansion_block() {
        assert_eq!(expand_battle_id(0xA2).expansion, [1, 0, 0]);
        assert_eq!(expand_battle_id(0xA3).expansion, [3, 0, 0]);
        assert_eq!(expand_battle_id(0xA4).expansion, [2, 0, 0]);
        // Still banded, but no expansion seed.
        assert_eq!(expand_battle_id(0xA5).expansion, [0, 0, 0]);
        assert_eq!(expand_battle_id(0x49).expansion, [0, 0, 0]);
    }

    #[test]
    fn zeto_id_is_in_band_but_retail_uses_the_record_path() {
        // Zeto (0x4B) falls inside the 0x49..=0x4D boss band, so the
        // battle-id path *would* produce a lone spawn - but retail installs
        // Zeto through the actor[+0x94] record instead. Both paths agree on
        // the resulting cell, which is why the band membership alone never
        // proved the mechanism.
        let via_id = expand_battle_id(0x4B);
        assert_eq!(via_id.cell, [0x4B, 0, 0, 0]);

        let record = EncounterRecord {
            count: 1,
            monster_ids: [0x4B, 0, 0, 0],
        };
        let mut cell = [0u8; FORMATION_SLOTS];
        record.apply_to_formation_cell(&mut cell);
        assert_eq!(cell, via_id.cell);
    }

    #[test]
    fn record_and_battle_id_paths_differ_in_shape_for_a_plain_id() {
        // The distinguishing signature from the docs: a count-1 record
        // writes one slot, the battle-id path writes three.
        let record = EncounterRecord {
            count: 1,
            monster_ids: [0x4F, 0, 0, 0],
        };
        let mut cell = [0u8; FORMATION_SLOTS];
        record.apply_to_formation_cell(&mut cell);
        assert_eq!(cell, [0x4F, 0, 0, 0]);
        assert_ne!(cell, expand_battle_id(0x4F).cell);
    }
}
