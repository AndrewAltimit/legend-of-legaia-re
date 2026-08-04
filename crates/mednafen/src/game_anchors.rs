//! Legaia-specific RAM anchors readable out of any 2 MiB main-RAM image.
//!
//! Both emulators' state readers land on the same thing - a KSEG0-addressed
//! 2 MiB main-RAM byte slice - so the "which scene is this state in?" question
//! has one implementation, not two. `legaia_pcsxr::SaveState` already exposed
//! these as inherent methods; this module is the shared body both crates call,
//! so a mednafen `.mc{0..9}` and a PCSX-Redux `.sstate` answer identically.
//!
//! The anchors themselves are documented in
//! [`docs/reference/memory-map.md`](../../../docs/reference/memory-map.md).

/// Player context pointer global (`*0x8007C364` = the player actor struct).
pub const PLAYER_PTR_VA: u32 = 0x8007_C364;
/// Active-scene CDNAME name (8 bytes at `0x8007050C`).
pub const SCENE_NAME_VA: u32 = 0x8007_050C;
/// Next game-mode index (`0x02` field-init, `0x03` field-run, `0x15` battle, ...).
pub const GAME_MODE_VA: u32 = 0x8007_B83C;
/// Player position fields (16-bit signed; `+0x16` facing sits between them, so
/// they MUST be read as `i16`, never `u32`).
pub const PLAYER_X_OFF: u32 = 0x14;
pub const PLAYER_Z_OFF: u32 = 0x18;

/// The per-scene field-environment block pointer lives in the scratchpad at
/// `0x1F8003EC`; the collision/object grid hangs off it at `+0x4000`. Mednafen
/// states carry the scratchpad as its own container entry (`ScratchRAM.data8`),
/// which is why the live grid is readable offline from a `.mc` but not from a
/// PCSX-Redux `.sstate` (main RAM only).
pub const FIELD_ENV_PTR_SPAD: u32 = 0x1F80_03EC;
/// Byte offset of the scratchpad window inside the 1 KiB scratchpad blob.
pub const SPAD_BASE: u32 = 0x1F80_0000;

fn off(va: u32) -> usize {
    (va & 0x1F_FFFF) as usize
}

/// Read a byte at a KSEG0 virtual address out of a main-RAM image.
pub fn u8_at(ram: &[u8], va: u32) -> u8 {
    ram.get(off(va)).copied().unwrap_or(0)
}

/// Read a little-endian halfword at a KSEG0 virtual address.
pub fn u16_at(ram: &[u8], va: u32) -> u16 {
    let o = off(va);
    if o + 1 >= ram.len() {
        return 0;
    }
    u16::from_le_bytes([ram[o], ram[o + 1]])
}

/// Read a signed little-endian halfword at a KSEG0 virtual address.
pub fn i16_at(ram: &[u8], va: u32) -> i16 {
    u16_at(ram, va) as i16
}

/// Read a little-endian word at a KSEG0 virtual address.
pub fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = off(va);
    if o + 3 >= ram.len() {
        return 0;
    }
    u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
}

/// Active CDNAME scene label (e.g. `"town01"`), trimmed at the first NUL /
/// non-printable byte.
pub fn scene_name(ram: &[u8]) -> String {
    let mut s = String::new();
    for i in 0..8 {
        let b = u8_at(ram, SCENE_NAME_VA + i);
        if !(0x20..0x7f).contains(&b) {
            break;
        }
        s.push(b as char);
    }
    s
}

/// Next game-mode index (`0x03` = field-run, `0x15` = battle, ...).
pub fn game_mode(ram: &[u8]) -> u8 {
    u8_at(ram, GAME_MODE_VA)
}

/// The player actor struct pointer (`*0x8007C364`), or `None` if it is not a
/// plausible KSEG0 main-RAM pointer.
pub fn player_ptr(ram: &[u8]) -> Option<u32> {
    let p = u32_at(ram, PLAYER_PTR_VA);
    ((p & 0xFFE0_0000) == 0x8000_0000).then_some(p)
}

/// Player world position `(x, z)` read as 16-bit signed from the player struct.
pub fn player_pos(ram: &[u8]) -> Option<(i16, i16)> {
    let p = player_ptr(ram)?;
    Some((i16_at(ram, p + PLAYER_X_OFF), i16_at(ram, p + PLAYER_Z_OFF)))
}

/// A one-line identity for a state: what scene, what mode, where the player is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StateIdentity {
    pub scene: String,
    pub game_mode: u8,
    pub player: Option<(i16, i16)>,
}

/// Human label for a game-mode byte. Unknown modes render as `mode_NN`.
pub fn game_mode_label(mode: u8) -> String {
    match mode {
        0x00 => "boot".into(),
        0x02 => "field-init".into(),
        0x03 => "field-run".into(),
        0x04 => "field-exit".into(),
        0x14 => "battle-init".into(),
        0x15 => "battle".into(),
        0x1A => "cutscene-str".into(),
        0x1B => "cutscene-str2".into(),
        m => format!("mode_{m:02X}"),
    }
}

/// Read the identity triple out of a main-RAM image.
pub fn identify(ram: &[u8]) -> StateIdentity {
    StateIdentity {
        scene: scene_name(ram),
        game_mode: game_mode(ram),
        player: player_pos(ram),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_ram() -> Vec<u8> {
        vec![0u8; 2 * 1024 * 1024]
    }

    #[test]
    fn scene_name_stops_at_nul() {
        let mut ram = blank_ram();
        let o = (SCENE_NAME_VA & 0x1F_FFFF) as usize;
        ram[o..o + 6].copy_from_slice(b"teien\0");
        assert_eq!(scene_name(&ram), "teien");
    }

    #[test]
    fn implausible_player_ptr_reads_as_none() {
        let ram = blank_ram();
        assert_eq!(player_ptr(&ram), None);
        assert_eq!(player_pos(&ram), None);
    }

    #[test]
    fn player_pos_reads_signed_halfwords() {
        let mut ram = blank_ram();
        let actor = 0x8010_0000u32;
        let po = (PLAYER_PTR_VA & 0x1F_FFFF) as usize;
        ram[po..po + 4].copy_from_slice(&actor.to_le_bytes());
        let ao = (actor & 0x1F_FFFF) as usize;
        ram[ao + 0x14..ao + 0x16].copy_from_slice(&(-1234i16).to_le_bytes());
        ram[ao + 0x18..ao + 0x1A].copy_from_slice(&(4321i16).to_le_bytes());
        assert_eq!(player_pos(&ram), Some((-1234, 4321)));
    }

    #[test]
    fn out_of_range_reads_are_saturating_not_panicking() {
        let tiny = vec![0u8; 8];
        assert_eq!(u32_at(&tiny, SCENE_NAME_VA), 0);
        assert_eq!(scene_name(&tiny), "");
    }
}
