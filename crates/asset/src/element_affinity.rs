//! Battle **element-affinity** matrix + per-character element table
//! (battle-action overlay, PROT 0898).
//!
//! The damage-scale stage `FUN_801dd864` (dump:
//! `ghidra/scripts/funcs/overlay_battle_action_801dd864.txt`) multiplies the
//! attacker's pre-damage roll by a percentage drawn from an 8×8 element-affinity
//! matrix:
//!
//! ```text
//! 801dd938  addiu v1,v1,0x53e8       ; v1 = matrix base 0x801F53E8
//! 801dd93c  sll   v0,t0,0x3          ; v0 = atk_elem * 8
//! 801dd940  addu  v0,a1,v0           ; v0 = def_elem + atk_elem*8
//! 801dd944  addu  v0,v0,v1
//! 801dd948  lbu   v1,0x0(v0)         ; pct = matrix[atk_elem*8 + def_elem]
//! 801dd954  mult  v0,v1              ; roll * pct
//! 801dd95c..6c                       ; / 100  (reciprocal-multiply by 0x51eb851f >> 5)
//! ```
//!
//! So the matrix is **row-major, rows = attacker element, columns = defender
//! element** (`matrix[atk][def]`), each cell a percentage applied as
//! `roll * pct / 100`. (An earlier engine comment had the axes transposed.) The
//! retail values are a small ±4% nudge — diagonal (same-element) `0x60` = 96,
//! reciprocal opposite-element pairs `0x68` = 104, everything else `0x64` = 100.
//!
//! ## Per-actor element source
//!
//! `FUN_801dd864` resolves each side's element id by actor kind:
//!
//! - **party member** (actor slot `< 3`): `element = CHARACTER_ELEMENTS[char_id]`,
//!   the per-character table at runtime VA [`CHARACTER_ELEMENTS_VA`] indexed by
//!   the **1-based** char id (`(byte)DAT_8007bd10[slot]`, 1=Vahn 2=Noa 3=Gala
//!   4=Terra). Disasm reads `*(byte*)(char_id + 0x801F547F)`, i.e. char id 1 →
//!   first table byte.
//! - **enemy** (actor slot `>= 3`): `element = actor[+0x1d]` — a byte on the live
//!   battle actor (`DAT_801c9348[slot-3] + 0x1d`). The monster-record field that
//!   the battle loader copies into `actor[+0x1d]` is **not yet pinned** (the
//!   monster→actor builder is an indirect-dispatch handler absent from the
//!   captured dumps), so the enemy element source remains open. See
//!   `docs/subsystems/battle-formulas.md`.
//!
//! ## Provenance
//!
//! Static overlay data: VA `0x801F53E8` maps to **PROT 0898 file offset
//! `0x26BD0`** under the same link base ([`OVERLAY_LINK_BASE`] `0x801CE818`) that
//! pins the move-power table (`0x801F4F5C` → `0x26744`; see [`crate::move_power`]).

/// CDNAME / PROT index of the battle-action overlay holding the tables.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize = 898;

/// The battle-action overlay's link/load base (`VA − file_offset`). Pinned by
/// the move-power table (`0x801F4F5C` → file `0x26744`).
pub const OVERLAY_LINK_BASE: u32 = 0x801C_E818;

/// Runtime VA of the 8×8 element-affinity matrix (`FUN_801dd864`'s base).
pub const AFFINITY_MATRIX_VA: u32 = 0x801F_53E8;

/// Raw PROT 0898 file offset of the affinity matrix (= `VA − OVERLAY_LINK_BASE`).
pub const AFFINITY_MATRIX_FILE_OFFSET: usize = 0x26BD0;

/// Element-id space size (matrix is `ELEMENT_COUNT × ELEMENT_COUNT`).
pub const ELEMENT_COUNT: usize = 8;

/// Runtime VA of the per-character element table (1-based char id; char id 1 =
/// first byte). Disasm: `lbu …,-0x1(char_id + 0x801F5480)`.
pub const CHARACTER_ELEMENTS_VA: u32 = 0x801F_5480;

/// Raw PROT 0898 file offset of the per-character element table.
pub const CHARACTER_ELEMENTS_FILE_OFFSET: usize = 0x26C68;

/// Number of per-character element entries parsed (Vahn / Noa / Gala / Terra /
/// + one). Entries past these read as `0` in the retail image.
pub const CHARACTER_ELEMENTS_LEN: usize = 8;

/// A battle element id (`0..=7`). Ids 2/3/4 (Fire/Wind/Thunder) and 7 (Neutral)
/// are byte-pinned via the per-character table + the all-100 neutral row/column;
/// 0/1/5/6 are **inferred** from the matrix's reciprocal opposite-element pairs
/// (`0↔3`, `1↔2`, `5↔6` each carry the 104% bonus) plus the spell-table element
/// vocabulary, not pinned to a numeric byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Element {
    /// Inferred (opposite of [`Wind`](Element::Wind)).
    Earth = 0,
    /// Inferred (opposite of [`Fire`](Element::Fire)).
    Water = 1,
    /// Pinned — Vahn.
    Fire = 2,
    /// Pinned — Noa / Terra.
    Wind = 3,
    /// Pinned — Gala.
    Thunder = 4,
    /// Inferred (opposite of [`Dark`](Element::Dark)).
    Light = 5,
    /// Inferred (opposite of [`Light`](Element::Light)).
    Dark = 6,
    /// Pinned — the all-100 affinity row + column (no element interaction).
    Neutral = 7,
}

impl Element {
    /// Resolve an element id (`0..=7`) to its [`Element`]; `None` past the table.
    pub fn from_id(id: u8) -> Option<Element> {
        match id {
            0 => Some(Element::Earth),
            1 => Some(Element::Water),
            2 => Some(Element::Fire),
            3 => Some(Element::Wind),
            4 => Some(Element::Thunder),
            5 => Some(Element::Light),
            6 => Some(Element::Dark),
            7 => Some(Element::Neutral),
            _ => None,
        }
    }

    /// Lowercase element name.
    pub fn name(self) -> &'static str {
        match self {
            Element::Earth => "earth",
            Element::Water => "water",
            Element::Fire => "fire",
            Element::Wind => "wind",
            Element::Thunder => "thunder",
            Element::Light => "light",
            Element::Dark => "dark",
            Element::Neutral => "neutral",
        }
    }
}

/// Parsed battle element-affinity tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementAffinity {
    /// The 8×8 affinity matrix, `matrix[attacker_element][defender_element]`,
    /// each cell a percentage (`roll * pct / 100`).
    pub matrix: [[u8; ELEMENT_COUNT]; ELEMENT_COUNT],
    /// Per-character element ids; index 0 = char id 1 (Vahn). Length
    /// [`CHARACTER_ELEMENTS_LEN`].
    pub character_elements: Vec<u8>,
}

impl ElementAffinity {
    /// Parse the matrix + per-character table out of the raw PROT 0898
    /// (battle-action overlay) entry. Returns `None` if the entry is too short
    /// to hold either table at its pinned offset.
    pub fn parse(prot_0898: &[u8]) -> Option<ElementAffinity> {
        let mend = AFFINITY_MATRIX_FILE_OFFSET + ELEMENT_COUNT * ELEMENT_COUNT;
        let cend = CHARACTER_ELEMENTS_FILE_OFFSET + CHARACTER_ELEMENTS_LEN;
        if prot_0898.len() < mend || prot_0898.len() < cend {
            return None;
        }
        let mut matrix = [[0u8; ELEMENT_COUNT]; ELEMENT_COUNT];
        for (atk, row) in matrix.iter_mut().enumerate() {
            let base = AFFINITY_MATRIX_FILE_OFFSET + atk * ELEMENT_COUNT;
            row.copy_from_slice(&prot_0898[base..base + ELEMENT_COUNT]);
        }
        let character_elements = prot_0898[CHARACTER_ELEMENTS_FILE_OFFSET..cend].to_vec();
        Some(ElementAffinity {
            matrix,
            character_elements,
        })
    }

    /// Affinity percentage for an attacker element hitting a defender element
    /// (`matrix[attacker][defender]`). `None` if either id is `>= ELEMENT_COUNT`.
    pub fn affinity_pct(&self, attacker: u8, defender: u8) -> Option<u8> {
        let (a, d) = (attacker as usize, defender as usize);
        if a < ELEMENT_COUNT && d < ELEMENT_COUNT {
            Some(self.matrix[a][d])
        } else {
            None
        }
    }

    /// Element id for a **1-based** char id (1 = Vahn). `None` past the table.
    pub fn character_element(&self, char_id_1based: u8) -> Option<u8> {
        if char_id_1based == 0 {
            return None;
        }
        self.character_elements
            .get(char_id_1based as usize - 1)
            .copied()
    }
}

/// Parse helper mirroring the other format modules.
pub fn parse(prot_0898: &[u8]) -> Option<ElementAffinity> {
    ElementAffinity::parse(prot_0898)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_index_is_attacker_row_defender_col() {
        // Synthetic image: matrix at the pinned offset, distinct cells so the
        // [attacker][defender] orientation is unambiguous.
        let mut buf = vec![0u8; CHARACTER_ELEMENTS_FILE_OFFSET + CHARACTER_ELEMENTS_LEN];
        for atk in 0..ELEMENT_COUNT {
            for def in 0..ELEMENT_COUNT {
                buf[AFFINITY_MATRIX_FILE_OFFSET + atk * ELEMENT_COUNT + def] =
                    (atk * 10 + def) as u8;
            }
        }
        let aff = ElementAffinity::parse(&buf).expect("parses");
        assert_eq!(aff.affinity_pct(2, 5), Some(25)); // fire attacks light
        assert_eq!(aff.affinity_pct(5, 2), Some(52)); // light attacks fire
        assert_eq!(aff.affinity_pct(8, 0), None);
    }

    #[test]
    fn character_element_is_one_based() {
        let mut buf = vec![0u8; CHARACTER_ELEMENTS_FILE_OFFSET + CHARACTER_ELEMENTS_LEN];
        buf[CHARACTER_ELEMENTS_FILE_OFFSET] = 2; // char id 1 = Vahn = fire
        buf[CHARACTER_ELEMENTS_FILE_OFFSET + 1] = 3; // char id 2 = Noa = wind
        let aff = ElementAffinity::parse(&buf).expect("parses");
        assert_eq!(aff.character_element(0), None);
        assert_eq!(aff.character_element(1), Some(2));
        assert_eq!(aff.character_element(2), Some(3));
    }

    #[test]
    fn element_ids_round_trip() {
        for id in 0u8..8 {
            assert_eq!(Element::from_id(id).unwrap() as u8, id);
        }
        assert!(Element::from_id(8).is_none());
        assert_eq!(Element::Fire.name(), "fire");
        assert_eq!(Element::Neutral.name(), "neutral");
    }
}
