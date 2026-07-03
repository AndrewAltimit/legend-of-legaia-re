//! Battle character-mesh assembly from a player battle file.
//!
//! Clean-room port of the retail battle-setup chain that builds each party
//! member's in-battle TMD out of their `data\battle\PLAYER<n>` file (see
//! [`crate::battle_data_pack`] for the container and
//! `docs/formats/character-mesh.md` § Battle form for the full chain):
//!
//! 1. **Section selection** ([`select_sections`]): walk the descriptor
//!    table's five sections, matching each entry's id against the
//!    character's equipped item id for that slot; an `id = 0` entry
//!    supplies the section default and advances the slot.
//! 2. **Object splice** ([`assemble_character`]): LZS-decode the five
//!    selected sections and splice their TMD objects into one merged TMD -
//!    object entries relocated into the merged pool, one bone-id byte per
//!    attached object, surplus objects (the equipment's visual meshes)
//!    tagged and sorted to the end, with their attach bones recorded.
//!
//! The output mirrors the retail blob the engine registers into
//! `DAT_8007C018[slot]` (standard relative-offset Legaia TMD + the bone-tag
//! and attach-bone side tables), byte-verified against a full-party battle
//! save (Vahn: `nobj = 17`, tags `[0..14, 200, 201]`, attach `[5, 8]`; the
//! 24-slot object table and the data pool are byte-exact vs the live blob,
//! with the only differences being each primitive's TSB +3 / CBA +0x40
//! rewrite - the separate per-slot runtime-band relocation pass applied at
//! registration, see `docs/formats/character-mesh.md` § Battle render).
//! [`assemble_character`] emits the disc-authentic (authoring) TSB/CBA
//! values; [`relocate_tsb_cba`] applies the registration-time pass that
//! moves them into the per-slot runtime VRAM band.

mod animation;
mod art;
mod assembly;
mod swing;
mod texture;

#[cfg(test)]
mod tests;

pub use animation::*;
pub use art::*;
pub use assembly::*;
pub use swing::*;
pub use texture::*;

use anyhow::Result;

/// Number of equipment sections per player file (= equip slots in the
/// character record's `+0x196..+0x19A` byte order).
pub const SECTION_COUNT: usize = 5;

/// Legaia TMD magic.
const TMD_MAGIC: u32 = 0x8000_0002;
/// Bytes per TMD object-table entry (7 u32 words).
const OBJ_ENTRY_BYTES: usize = 0x1C;

fn read_u32(buf: &[u8], off: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        buf.get(off..off + 4)
            .ok_or_else(|| anyhow::anyhow!("u32 read at {off:#x} past end"))?
            .try_into()
            .unwrap(),
    ))
}
