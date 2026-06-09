//! Player-character mesh pack — the head of PROT 0874 (`befect_data`) §0.
//!
//! Section 0 of the LZS-container at PROT 0874 decompresses to a canonical
//! [`crate::pack`]-shaped TMD pack with **five** Legaia TMDs. These are the
//! five character meshes the retail engine keeps resident across every field
//! scene at `DAT_8007C018[0..=4]`; the three active-party slots (`[0..=2]`)
//! are the ones the per-frame equipment-swap pass [`equipment_swap`] patches.
//!
//! ```text
//! Pack slot | nobj (disc) | Runtime body bytes | Role
//! ----------|-------------|--------------------|--------------------------
//!     0     |     12      |       13 220       | Vahn — active party slot 0
//!     1     |     12      |       13 800       | Noa  — active party slot 1
//!     2     |     12      |       11 656       | Gala — active party slot 2
//!     3     |      3      |        6 488       | Savepoint (save crystal)
//!     4     |      2      |        1 048       | Auxiliary actor (untriaged)
//! ```
//!
//! The "runtime body bytes" column is what the LZS-bounded decode produces
//! (and what retail allocates at `DAT_8007C018[..]`). Slot 4's underlying
//! compressed stream would expand to ~20 KB if decoded unbounded, but the
//! descriptor's compressed-size hint caps the decode at the first ~46 KB so
//! slot 4 receives only its 1 048-byte TMD prefix — byte-equality verified
//! against the live `DAT_8007C018[4]` allocation (see
//! [`docs/formats/world-map-overlay.md` § Disc-side source of `[0..4]`](../../../docs/formats/world-map-overlay.md#disc-side-source-of-04)).
//!
//! ## Equipment-conditional group templates
//!
//! Slots 0/1/2 each ship with `nobj=12` even though retail caps live
//! `group_count` to 10 in `FUN_8001E890`. Groups 10 and 11 are *templates*
//! that [`FUN_8001EBEC`](crate::character_pack::equipment_swap) picks between
//! at runtime to overwrite one of the visible groups (0 / 3 / 5) based on a
//! per-character equipment byte. The decoded TMD's group descriptor array
//! starts at `+0x0C`; each descriptor is 28 bytes (`0x1C`), so:
//!
//! - group 10 lives at TMD byte offset `0x0C + 10*0x1C` = `0x124`
//! - group 11 lives at TMD byte offset `0x0C + 11*0x1C` = `0x140`
//!
//! See [`equipment_swap`] for the patch semantic.
//!
//! ## Retail loader
//!
//! The retail loader chain that installs section 0 into `DAT_8007C018[0..4]`
//! via `FUN_8001F05C` case 2 → `FUN_80026B4C` is not yet pinned. The
//! engine routes the disc bytes directly through this parser (see
//! `engine_core::scene::seed_global_tmd_pool_from_befect_data`).

use anyhow::{Context, Result, bail};

use crate::{DecodeMode, decode, pack, parse_player_lzs};

/// PROT entry index that carries the player-character pack (`befect_data` head).
pub const PROT_ENTRY_INDEX: u32 = 874;

/// Short display label for one player-character pack slot. Pack slots 0/1/2
/// are the active-party characters (Vahn / Noa / Gala). Slot 3 is the
/// **savepoint** mesh — the star-crystal interactable that lets the player
/// save their game in towns and dungeons (its mesh is small enough at
/// `nobj=3` / ~6.5 KB that it's worth pinning resident alongside the party
/// so the engine doesn't re-page it every time the player approaches a save
/// point). Slot 4 is a small auxiliary actor whose runtime role is still
/// untriaged.
pub fn slot_label(slot: usize) -> &'static str {
    match slot {
        0 => "Vahn",
        1 => "Noa",
        2 => "Gala",
        3 => "Savepoint",
        4 => "Aux 1",
        _ => "(out of range)",
    }
}

/// Number of TMDs in the player-character pack (slots 0..=4).
pub const SLOT_COUNT: usize = 5;

/// Index of the LZS-compressed section inside the PROT 0874 container that
/// carries the character pack (the head section).
pub const CONTAINER_SECTION: usize = 0;

/// Number of descriptors the PROT 0874 container header carries (the
/// `parse_player_lzs` shape; the other two sections are `vdf.dat` /
/// `etim.dat`).
pub const CONTAINER_DESCRIPTORS: usize = 3;

/// Legaia TMD magic (`0x80000002`).
const TMD_MAGIC: u32 = 0x8000_0002;

/// Byte size of a Legaia TMD group descriptor (`0x1C` = 28 bytes). The disc
/// pack's group-10 and group-11 descriptors are at `0x124` / `0x140` from the
/// TMD start.
pub const GROUP_DESCRIPTOR_BYTES: usize = 0x1C;

/// Byte offset of the first group descriptor inside a Legaia TMD (post-header).
pub const FIRST_GROUP_DESCRIPTOR_OFFSET: usize = 0x0C;

/// Byte offset of group 10 (the equipment-swap "non-zero byte" template) in a
/// disc-form character TMD. Equals `FIRST_GROUP_DESCRIPTOR_OFFSET + 10 * GROUP_DESCRIPTOR_BYTES`.
pub const EQUIP_GROUP_NONZERO_OFFSET: usize = 0x124;
/// Byte offset of group 11 (the equipment-swap "zero byte" template) in a
/// disc-form character TMD. Equals `FIRST_GROUP_DESCRIPTOR_OFFSET + 11 * GROUP_DESCRIPTOR_BYTES`.
pub const EQUIP_GROUP_ZERO_OFFSET: usize = 0x140;

/// One decoded slot of the character pack: a Legaia TMD plus its provenance.
#[derive(Debug, Clone)]
pub struct CharacterSlot {
    /// 0-based slot inside the disc pack (`0..=4`).
    pub slot: usize,
    /// `nobj` from the TMD header on disc. Slots 0/1/2 ship `12`; slots 3/4
    /// ship `3` / `2`.
    pub disc_nobj: u32,
    /// Raw disc-form TMD bytes (`.0` of the pack body). Parses cleanly with
    /// [`legaia_tmd::parse`]; the trailing pack-padding past the TMD's own
    /// extent is harmless.
    pub tmd_bytes: Vec<u8>,
}

impl CharacterSlot {
    /// True for the three active-party slots (`0..=2`) the engine caps to 10
    /// live groups and runs the equipment-swap pass on. Slots 3/4 ship
    /// `nobj=3` / `nobj=2`, never have group 10 / 11 templates, and are not
    /// covered by the swap.
    pub fn is_active_party(&self) -> bool {
        self.slot < 3
    }

    /// Read the equipment-conditional template at `template_offset`
    /// (one of [`EQUIP_GROUP_NONZERO_OFFSET`] / [`EQUIP_GROUP_ZERO_OFFSET`]) as
    /// a borrowed 28-byte slice. Returns `None` for slots that don't carry the
    /// template (slots 3 / 4) or when the TMD is shorter than the templates'
    /// region (would indicate a corrupt slot).
    pub fn equipment_template(&self, template_offset: usize) -> Option<&[u8]> {
        if !self.is_active_party() {
            return None;
        }
        let end = template_offset + GROUP_DESCRIPTOR_BYTES;
        if end > self.tmd_bytes.len() {
            return None;
        }
        Some(&self.tmd_bytes[template_offset..end])
    }

    /// The disc-form template that retail copies into a visible group when the
    /// per-character equipment byte at the character record's per-slot offset
    /// (`+0x196` Vahn / `+0x199` Noa / `+0x19B` Gala) is **non-zero**. Group 10
    /// on disc; lives at TMD byte offset [`EQUIP_GROUP_NONZERO_OFFSET`].
    pub fn equipped_template(&self) -> Option<&[u8]> {
        self.equipment_template(EQUIP_GROUP_NONZERO_OFFSET)
    }

    /// The disc-form template retail copies in when the equipment byte is
    /// **zero**. Group 11 on disc; lives at TMD byte offset
    /// [`EQUIP_GROUP_ZERO_OFFSET`].
    pub fn unequipped_template(&self) -> Option<&[u8]> {
        self.equipment_template(EQUIP_GROUP_ZERO_OFFSET)
    }
}

/// The full parsed character pack — five slots in disc order.
#[derive(Debug, Clone)]
pub struct CharacterPack {
    pub slots: [CharacterSlot; SLOT_COUNT],
}

impl CharacterPack {
    /// Borrowed view of all five slots.
    pub fn slots(&self) -> &[CharacterSlot; SLOT_COUNT] {
        &self.slots
    }

    /// Get one slot by its 0-based pack index.
    pub fn slot(&self, idx: usize) -> Option<&CharacterSlot> {
        self.slots.get(idx)
    }

    /// The three active-party slots (`0..=2`). These are the ones the
    /// equipment-swap pass touches.
    pub fn active_party(&self) -> &[CharacterSlot] {
        &self.slots[..3]
    }
}

/// Parse the player-character pack from the raw bytes of PROT entry 874.
///
/// The retail chain is `parse_player_lzs(buf, 3)` -> section 0 descriptor ->
/// LZS-decompress -> [`crate::pack::extract_pack`] -> 5 TMD bodies. This
/// function mirrors that chain and validates each slot's TMD magic.
pub fn parse(prot_0874_bytes: &[u8]) -> Result<CharacterPack> {
    let container = parse_player_lzs(prot_0874_bytes, CONTAINER_DESCRIPTORS)
        .context("parse PROT 0874 as a 3-descriptor player.lzs-shaped container")?;
    let section0 = container
        .descriptors
        .get(CONTAINER_SECTION)
        .ok_or_else(|| {
            anyhow::anyhow!("PROT 0874 container has no section {}", CONTAINER_SECTION)
        })?;
    let decoded = decode(prot_0874_bytes, section0, DecodeMode::Lzs)
        .context("LZS-decode PROT 0874 section 0 (character pack)")?;
    let bodies = pack::extract_pack(&decoded).context("walk PROT 0874 section 0 as a TMD pack")?;
    if bodies.len() < SLOT_COUNT {
        bail!(
            "expected {} character slots in PROT 0874 §0, found {}",
            SLOT_COUNT,
            bodies.len()
        );
    }

    // Build each slot. We validate TMD magic and read the disc-form nobj
    // (TMD header `+0x08`) directly; full TMD parsing is deferred to the
    // caller via `legaia_tmd::parse(tmd_bytes())`.
    let mut slots: Vec<CharacterSlot> = Vec::with_capacity(SLOT_COUNT);
    for (slot, body) in bodies.into_iter().take(SLOT_COUNT).enumerate() {
        if body.len() < 0x0C {
            bail!(
                "character slot {slot}: pack body too short ({} bytes) for a Legaia TMD header",
                body.len()
            );
        }
        let magic = u32::from_le_bytes(body[..4].try_into().unwrap());
        if magic != TMD_MAGIC {
            bail!(
                "character slot {slot}: expected Legaia TMD magic 0x{:08X}, got 0x{:08X}",
                TMD_MAGIC,
                magic
            );
        }
        let disc_nobj = u32::from_le_bytes(body[0x08..0x0C].try_into().unwrap());
        slots.push(CharacterSlot {
            slot,
            disc_nobj,
            tmd_bytes: body.to_vec(),
        });
    }
    let slots: [CharacterSlot; SLOT_COUNT] = slots
        .try_into()
        .map_err(|v: Vec<_>| anyhow::anyhow!("expected {SLOT_COUNT} slots, got {}", v.len()))?;
    Ok(CharacterPack { slots })
}

/// Equipment-swap descriptor patch — the [`FUN_8001EBEC`] runtime patch.
///
/// At runtime, for each of the 3 active-party slots, retail picks one of the
/// two pre-built 28-byte group templates (group 10 / group 11) baked into the
/// disc-form TMD and overwrites a **visible** group descriptor with it. The
/// patched group's index inside the TMD is character-specific:
///
/// | Pack slot | Patched group | Equip-byte record offset | Common reading |
/// |---:|:---:|:---:|---|
/// |     0     | 0 | `+0x196` | Vahn (active party slot 0) |
/// |     1     | 3 | `+0x199` | Noa (active party slot 1)  |
/// |     2     | 5 | `+0x19B` | Gala (active party slot 2) |
///
/// (The "patched group index" and "equip-byte offset within the per-slot
/// byte window" are the same three numbers `{0, 3, 5}` — retail's
/// `FUN_8001EBEC` reuses one tiny stack table for both roles. See the asm
/// trace in `ghidra/scripts/funcs/8001ebec.txt`.)
///
/// The swap is binary: a non-zero equip byte copies the group-10 template
/// (`TMD+0x124`), a zero byte copies the group-11 template (`TMD+0x140`).
/// Per-character-slot naming follows the standard active-party roster
/// (slot 0 = Vahn / slot 1 = Noa / slot 2 = Gala); slot identity is asserted
/// by the disc layout (the three slots are the only ones with `nobj=12`).
pub mod equipment_swap {
    use super::{EQUIP_GROUP_NONZERO_OFFSET, EQUIP_GROUP_ZERO_OFFSET, GROUP_DESCRIPTOR_BYTES};

    /// The four per-slot constants pinned by `FUN_8001EBEC`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PatchSlot {
        /// Active-party slot index (`0..=2`).
        pub slot: u8,
        /// Group descriptor index inside the slot's TMD whose 28 bytes get
        /// overwritten by the chosen template. `0` (Vahn), `3` (Noa), `5` (Gala).
        pub patched_group_index: u8,
        /// Per-character record byte offset of the equipment toggle. Read by
        /// retail as `*(0x80084140 + slot*0x414 + 0x75e + local_10[slot])`;
        /// folded down to a flat record offset for clean-room consumers.
        pub equip_byte_record_offset: u16,
    }

    /// The three active-party patch slots in pack order.
    pub const ACTIVE_PARTY_SLOTS: [PatchSlot; 3] = [
        PatchSlot {
            slot: 0,
            patched_group_index: 0,
            equip_byte_record_offset: 0x196,
        },
        PatchSlot {
            slot: 1,
            patched_group_index: 3,
            equip_byte_record_offset: 0x199,
        },
        PatchSlot {
            slot: 2,
            patched_group_index: 5,
            equip_byte_record_offset: 0x19B,
        },
    ];

    /// Pick the template byte offset to source for an equip toggle byte.
    /// Mirrors `FUN_8001EBEC`'s `if (byte == 0) source = TMD+0x140; else source = TMD+0x124;`.
    pub fn template_offset_for_equip_byte(equip_byte: u8) -> usize {
        if equip_byte == 0 {
            EQUIP_GROUP_ZERO_OFFSET
        } else {
            EQUIP_GROUP_NONZERO_OFFSET
        }
    }

    /// Apply the runtime equipment-swap patch to a disc-form character TMD.
    ///
    /// PORT: FUN_8001EBEC (one active-party-slot iteration of the group-swap)
    ///
    /// Mirrors one iteration of `FUN_8001EBEC` for a single active-party slot:
    /// copies the 28-byte template at [`template_offset_for_equip_byte`] over
    /// the visible group descriptor at index `patched_group_index`. The result
    /// is a TMD buffer that — when the engine caps `group_count` to 10 — renders
    /// with the equip-conditional mesh swap applied.
    ///
    /// `tmd_bytes` must be the disc-form TMD body (e.g. `slot.tmd_bytes`).
    /// Returns the patched buffer (the caller owns it). `equip_byte` is the
    /// character record byte at `patch.equip_byte_record_offset` (see
    /// [`ACTIVE_PARTY_SLOTS`]).
    pub fn apply(tmd_bytes: &[u8], patch: PatchSlot, equip_byte: u8) -> Vec<u8> {
        let mut out = tmd_bytes.to_vec();
        let src_off = template_offset_for_equip_byte(equip_byte);
        let dst_off = super::FIRST_GROUP_DESCRIPTOR_OFFSET
            + patch.patched_group_index as usize * GROUP_DESCRIPTOR_BYTES;
        let src_end = src_off + GROUP_DESCRIPTOR_BYTES;
        let dst_end = dst_off + GROUP_DESCRIPTOR_BYTES;
        if src_end > out.len() || dst_end > out.len() {
            // Refuse to patch a corrupt / truncated TMD.
            return out;
        }
        let template: [u8; GROUP_DESCRIPTOR_BYTES] = out[src_off..src_end].try_into().unwrap();
        out[dst_off..dst_end].copy_from_slice(&template);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_slots_match_fun_8001ebec_table() {
        let want = [(0u8, 0u8, 0x196u16), (1, 3, 0x199), (2, 5, 0x19B)];
        for (i, (slot, group, off)) in want.into_iter().enumerate() {
            let p = equipment_swap::ACTIVE_PARTY_SLOTS[i];
            assert_eq!(p.slot, slot);
            assert_eq!(p.patched_group_index, group);
            assert_eq!(p.equip_byte_record_offset, off);
        }
    }

    #[test]
    fn template_offset_picks_group_11_for_zero() {
        assert_eq!(
            equipment_swap::template_offset_for_equip_byte(0),
            EQUIP_GROUP_ZERO_OFFSET
        );
        for b in 1u8..=255 {
            assert_eq!(
                equipment_swap::template_offset_for_equip_byte(b),
                EQUIP_GROUP_NONZERO_OFFSET
            );
        }
    }

    #[test]
    fn apply_overwrites_target_group_descriptor() {
        // Build a synthetic TMD-shaped buffer with 12 distinct group descriptors
        // so we can prove the patch hits the right one. Header = 12 bytes,
        // descriptors follow at +0x0C, each 0x1C bytes, filled with their index.
        let mut tmd = Vec::with_capacity(0x0C + 12 * GROUP_DESCRIPTOR_BYTES);
        tmd.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // magic
        tmd.extend_from_slice(&1u32.to_le_bytes()); // flags
        tmd.extend_from_slice(&12u32.to_le_bytes()); // group_count
        for g in 0u8..12 {
            tmd.extend_from_slice(&[g; GROUP_DESCRIPTOR_BYTES]);
        }
        // Group 10 = nonzero template, group 11 = zero template.
        assert_eq!(tmd[EQUIP_GROUP_NONZERO_OFFSET], 10);
        assert_eq!(tmd[EQUIP_GROUP_ZERO_OFFSET], 11);

        // Vahn patch with equip_byte=1 should copy group 10's bytes over group 0.
        let v = equipment_swap::apply(&tmd, equipment_swap::ACTIVE_PARTY_SLOTS[0], 1);
        assert_eq!(v[FIRST_GROUP_DESCRIPTOR_OFFSET], 10);
        // Other groups untouched.
        assert_eq!(v[FIRST_GROUP_DESCRIPTOR_OFFSET + GROUP_DESCRIPTOR_BYTES], 1);

        // Noa patch with equip_byte=0 should copy group 11 over group 3.
        let n = equipment_swap::apply(&tmd, equipment_swap::ACTIVE_PARTY_SLOTS[1], 0);
        let dst = FIRST_GROUP_DESCRIPTOR_OFFSET + 3 * GROUP_DESCRIPTOR_BYTES;
        assert_eq!(n[dst], 11);

        // Gala patch with equip_byte=0xFF should copy group 10 over group 5.
        let g = equipment_swap::apply(&tmd, equipment_swap::ACTIVE_PARTY_SLOTS[2], 0xFF);
        let dst = FIRST_GROUP_DESCRIPTOR_OFFSET + 5 * GROUP_DESCRIPTOR_BYTES;
        assert_eq!(g[dst], 10);
    }

    #[test]
    fn apply_adds_no_objects() {
        // `FUN_8001EBEC` only copies a 28-byte transform over an existing
        // group; it never grows the object/group count. The runtime `nobj`
        // +2 (15->17) seen in battle comes from a *different* (still-unpinned)
        // loader — D-WEAP — not from this swap. Pin that here so the doc claim
        // can't silently drift back to "the equipment swap adds the +2 groups".
        let mut tmd = Vec::new();
        tmd.extend_from_slice(&0x8000_0002u32.to_le_bytes()); // magic
        tmd.extend_from_slice(&1u32.to_le_bytes()); // flags
        tmd.extend_from_slice(&12u32.to_le_bytes()); // group_count
        for g in 0u8..12 {
            tmd.extend_from_slice(&[g; GROUP_DESCRIPTOR_BYTES]);
        }
        let before_len = tmd.len();
        let before_count = u32::from_le_bytes(tmd[0x08..0x0C].try_into().unwrap());

        let out = equipment_swap::apply(&tmd, equipment_swap::ACTIVE_PARTY_SLOTS[0], 1);

        // Buffer length unchanged, and the group_count word at +0x08 is untouched.
        assert_eq!(out.len(), before_len, "swap must not resize the TMD");
        let after_count = u32::from_le_bytes(out[0x08..0x0C].try_into().unwrap());
        assert_eq!(
            after_count, before_count,
            "swap must not change the object/group count"
        );
        // Exactly one group descriptor's worth of bytes may differ (the patched
        // group); everything outside it is byte-identical.
        let diff_bytes = tmd.iter().zip(&out).filter(|(a, b)| a != b).count();
        assert!(
            diff_bytes <= GROUP_DESCRIPTOR_BYTES,
            "swap touched {diff_bytes} bytes; should be within one 0x1C group descriptor"
        );
    }
}
