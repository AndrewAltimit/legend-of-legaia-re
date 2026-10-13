//! The Best-equipment row: candidate ranking per armament slot and its application.

use super::*;

impl EquipSession {
    /// The four Best Equipment candidate ids for this session's character,
    /// in the candidate array's weapon-first order.
    ///
    /// The engine's binding of [`best_equipment_candidates`]: the incumbent
    /// seed is the character's four armament slots
    /// ([`ARMAMENT_ENGINE_SLOTS`]), the per-id record join comes from the
    /// session's [`DiscEquipInfo`] + [`EquipmentTable`], and the weapon
    /// category score comes from the installed favour table
    /// ([`Self::with_weapon_category`]) or is `0` when none is installed.
    pub fn best_equipment_now(&self) -> [u8; 4] {
        let mut equipped = [0u8; 4];
        for (i, &slot) in ARMAMENT_ENGINE_SLOTS.iter().enumerate() {
            equipped[i] = self.record.equip.get(slot).copied().unwrap_or(0);
        }
        let bag: Vec<(u8, u8)> = {
            let mut v: Vec<(u8, u8)> = self.inventory.iter().map(|(&id, &n)| (id, n)).collect();
            v.sort_unstable();
            v
        };
        let char_mask = if self.active_party_slot < 3 {
            1u8 << self.active_party_slot
        } else {
            0
        };
        let table = &self.weapon_category;
        let party_slot = u32::from(self.active_party_slot);
        best_equipment_candidates(
            equipped,
            char_mask,
            &bag,
            |id| self.equip_candidate(id),
            |id| {
                if table.is_empty() {
                    0
                } else {
                    crate::menu_item_category::category_check(table, party_slot, id, 1)
                }
            },
        )
    }

    /// Join one bag id into the [`EquipCandidate`] shape the Best Equipment
    /// scan ranks on. `None` for anything that is not armament-class
    /// equipment this session can rank.
    fn equip_candidate(&self, id: u8) -> Option<EquipCandidate> {
        let m = self.equipment.get(id)?;
        let clamp = |v: i16| v.clamp(0, 255) as u8;
        let (char_mask, slot_bits) = match self.restrictions.as_ref().and_then(|r| r.entry(id)) {
            Some(e) => (e.mask, disc_category_slot_bits(e.category)),
            // Legacy placeholder table: the id's upper bits name the engine
            // slot, and only the four armament slots compete.
            None => {
                let engine_slot = ((id >> 5) & 0x07) as usize;
                let armament = ARMAMENT_ENGINE_SLOTS
                    .iter()
                    .position(|&s| s == engine_slot)?;
                (0xFF, ARMAMENT_SLOT_BITS[armament])
            }
        };
        Some(EquipCandidate {
            kind: 1,
            attack: clamp(m.atk),
            defence: (clamp(m.udf), clamp(m.ldf)),
            char_mask,
            slot_bits,
        })
    }

    /// The Equip screen's slot-browse confirm dispatch: row `0` is the
    /// "Best Equipment" auto-equip, rows `1..` open the candidate list
    /// for slot [`slot_for_browse_row`]. (Retail's cancel leaves for the character
    /// picker, sub-screen `0x12`; the engine host owns that transition.)
    ///
    /// `candidates` are the four best-armament ids the retail candidate
    /// computer `FUN_801CF88C` parks at `DAT_801EF0C0`;
    /// [`Self::best_equipment_now`] is the engine's producer, and
    /// [`Self::input`]'s slot-picker confirm arm runs it just before this
    /// call the way `FUN_801D99F0` does at `0x801D9B60`.
    ///
    /// PORT: FUN_801d99f0 (menu-overlay sub-screen `0x13`; see
    /// `ghidra/scripts/funcs/overlay_menu_801d99f0.txt` - confirm on row 0
    /// runs the applier and cues SFX `0x24` on change / buzz `0x23` on
    /// none, other rows hand off to sub-screen `0x14`)
    pub fn slot_browse_confirm(&mut self, row: u8, candidates: [u8; 4]) -> SlotBrowseOutcome {
        if row == SLOT_BROWSE_BEST_ROW {
            let before = self.record.equip;
            let changed = apply_best_equipment(
                &mut self.record.equip,
                candidates,
                ARMAMENT_ENGINE_SLOTS,
                &mut self.inventory,
            );
            for slot in ARMAMENT_ENGINE_SLOTS {
                if self.record.equip[slot] != before[slot] {
                    self.best_swaps
                        .push((slot as u8, before[slot], self.record.equip[slot]));
                }
            }
            if changed > 0 {
                self.preview_stats = compute_battle_stats(
                    &self.record,
                    &self.equipment,
                    &self.active_status,
                    &self.modifiers,
                );
                SlotBrowseOutcome::BestEquipApplied(changed)
            } else {
                SlotBrowseOutcome::BestEquipNothing
            }
        } else {
            SlotBrowseOutcome::OpenCandidates {
                slot: slot_for_browse_row(row),
            }
        }
    }
}

/// The `+7 & 0x60` slot bits a parsed disc equip category carries.
fn disc_category_slot_bits(cat: legaia_game_tables::equip_stats::EquipSlot) -> u8 {
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    match cat {
        Disc::Body => 0x00,
        Disc::Head => 0x20,
        Disc::Weapon => 0x40,
        Disc::Footwear => 0x60,
    }
}

/// `+7 & 0x60` bits per armament index, the inverse of [`armament_slot_of`].
const ARMAMENT_SLOT_BITS: [u8; 4] = [0x40, 0x20, 0x00, 0x60];

/// The **armament** slot a piece of equipment competes for, in the order the
/// Best Equipment candidate array `DAT_801EF0C0` uses.
///
/// Retail derives it from the equipment record's `+7` slot bits and then
/// permutes: the raw `(bits & 0x60) >> 5` order is body / head / weapon /
/// footwear, and the candidate array is indexed **weapon first**. The
/// permutation is a four-way branch chain in the body, not a table.
///
/// PORT: FUN_801cf88c (`0x801CFA38..0x801CFA64`)
pub fn armament_slot_of(slot_bits: u8) -> usize {
    // raw 0 body -> 2, 1 head -> 1, 2 weapon -> 0, 3 footwear -> 3.
    [2usize, 1, 0, 3][((slot_bits & 0x60) >> 5) as usize]
}

/// The joined item + equipment record fields the Best Equipment scan reads
/// for one bag id: item record `+0` / `+1` chained into the equipment record
/// at `0x80074F68 + index*8`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EquipCandidate {
    /// Item record `+0` - the class byte. Only `1` (equipment) competes.
    pub kind: u8,
    /// Equipment record `+1` - the ATK bonus, the weapon slot's only stat term.
    pub attack: u8,
    /// Equipment record `+2` / `+3` - the UDF / LDF bonuses, the **only**
    /// stat terms the armour slots rank on.
    pub defence: (u8, u8),
    /// Equipment record `+6` - the equippable-character mask.
    pub char_mask: u8,
    /// Equipment record `+7` - the slot-type bits.
    pub slot_bits: u8,
}

/// Compute the four Best Equipment candidate ids for a character.
///
/// This is the scan half of retail's `FUN_801CF88C`: it seeds the candidate
/// array with what the character already wears and then walks the bag,
/// keeping the best eligible id per armament slot. The function's other half
/// (back up the eight equip bytes, write the candidates in, re-run the stat
/// aggregator `FUN_801CF650` so the preview block reflects them, then restore
/// the backup) is host bookkeeping the engine expresses through
/// [`EquipSession::preview_candidate`] and [`compute_battle_stats`].
///
/// Two ranking laws fall straight out of the disassembly and are easy to get
/// wrong from the screen alone:
///
/// * **Armour is ranked on `UDF + LDF` only.** The INT (`+0`) and SPD (`+4`)
///   bonuses are never read here, so a head accessory that is pure INT or a
///   pair of boots that is pure SPD never displaces an incumbent.
/// * **The weapon's category check dominates its ATK.** The weapon score is
///   `equip[+1] + FUN_801DD0C0(char, id, 1)`, and that check contributes a
///   flat [`crate::menu_item_category::CATEGORY_MATCH_SCORE`] (`1000`) or
///   `0`. An ATK byte cannot reach 1000, so a category-favoured weapon beats
///   every unfavoured one outright regardless of raw attack.
///
/// A slot whose incumbent is `0` (nothing equipped) is filled by the **first**
/// eligible bag entry for it - the empty-slot test runs before the comparison.
/// Ties keep the incumbent (`<` is strict in both arms).
///
/// `weapon_category_score` is the caller's `FUN_801DD0C0` binding; pass
/// `|_| 0` when no category table is loaded.
///
/// PORT: FUN_801cf88c (menu-overlay Best Equipment candidate scan;
/// `see ghidra/scripts/funcs/overlay_menu_801cf88c.txt`)
/// REF: FUN_801cf650 (the stat aggregator the retail body re-runs twice
/// around the trial equip)
/// REF: FUN_801dd0c0 (the category check `weapon_category_score` binds;
/// ported as `crate::menu_item_category::category_check`)
///
/// Bound by [`EquipSession::best_equipment_now`], which the slot-picker's
/// row-0 confirm runs before dispatching to
/// [`EquipSession::slot_browse_confirm`].
pub fn best_equipment_candidates(
    equipped: [u8; 4],
    char_mask: u8,
    bag: &[(u8, u8)],
    lookup: impl Fn(u8) -> Option<EquipCandidate>,
    weapon_category_score: impl Fn(u8) -> u32,
) -> [u8; 4] {
    let mut best = equipped;
    for &(id, count) in bag {
        if id == 0 || count == 0 {
            continue;
        }
        let Some(cand) = lookup(id) else {
            continue;
        };
        if cand.kind != 1 || cand.char_mask & char_mask == 0 {
            continue;
        }
        let slot = armament_slot_of(cand.slot_bits);
        let incumbent = best[slot];
        if incumbent == 0 {
            best[slot] = id;
            continue;
        }
        let cur = lookup(incumbent).unwrap_or_default();
        let (cand_score, cur_score) = if slot == 0 {
            (
                u32::from(cand.attack) + weapon_category_score(id),
                u32::from(cur.attack) + weapon_category_score(incumbent),
            )
        } else {
            (
                u32::from(cand.defence.0) + u32::from(cand.defence.1),
                u32::from(cur.defence.0) + u32::from(cur.defence.1),
            )
        };
        if cur_score < cand_score {
            best[slot] = id;
        }
    }
    best
}

/// Outcome of [`EquipSession::slot_browse_confirm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotBrowseOutcome {
    /// Best Equipment applied `n` swaps (retail SFX `0x24` + stat
    /// re-aggregate).
    BestEquipApplied(u32),
    /// Nothing to change (retail buzz `0x23`).
    BestEquipNothing,
    /// A slot row confirmed - open the candidate list (retail sub-screen
    /// `0x14`).
    OpenCandidates { slot: u8 },
}

/// The "Best Equipment" applier over the four armament slots: for each
/// slot whose best-candidate id differs from the equipped id, take one
/// candidate from the bag (a candidate not in the bag is skipped - no
/// swap, no count), return the old item when non-zero, and write the
/// candidate into the slot. Returns the number of slots changed.
///
/// `armament_slots` names the equip-array index each armament index writes.
/// Retail does the same indirection through `DAT_801E43E8[i]` (and, for the
/// weapon, the per-character halfword at `DAT_8007B42C + char*2`) rather
/// than writing byte `i` - see the `bne s1,zero` / `lbu v1,0x0(v0)` pair at
/// `0x801CF7B4..0x801CF7C8`. The engine needs it for a second reason: its
/// equip array inserts a Hand Guard slot retail has no row for, so footwear
/// is index `4` ([`ARMAMENT_ENGINE_SLOTS`]).
///
/// PORT: FUN_801cf760 (see `ghidra/scripts/funcs/overlay_menu_801cf760.txt`:
/// per-slot `beq candidate, equipped` skip, `FUN_80042EE0` bag-slot find
/// with the `0x100` not-found sentinel, `FUN_80043048(slot, 1)` take,
/// `FUN_800421D4(old, 1)` give-back, `sb` commit, changed-count return)
/// REF: FUN_801cf88c (the best-candidate computer filling `DAT_801EF0C0` -
/// ported as [`best_equipment_candidates`])
pub fn apply_best_equipment(
    equips: &mut [u8; 8],
    candidates: [u8; 4],
    armament_slots: [usize; 4],
    inventory: &mut crate::item_bag::ItemBag,
) -> u32 {
    let mut changed = 0;
    for (armament, &candidate) in candidates.iter().enumerate() {
        let slot = armament_slots[armament];
        let equipped = equips[slot];
        if candidate == equipped {
            continue;
        }
        // Bag must actually hold the candidate (retail's 0x100 sentinel
        // check); the Remove direction never happens here - a zero
        // candidate is never in the bag.
        let Some(qty) = inventory.get_mut(&candidate) else {
            continue;
        };
        if *qty == 0 {
            continue;
        }
        *qty -= 1;
        if equipped != 0 {
            let _ = inventory.add(equipped, 1);
        }
        equips[slot] = candidate;
        changed += 1;
    }
    changed
}
