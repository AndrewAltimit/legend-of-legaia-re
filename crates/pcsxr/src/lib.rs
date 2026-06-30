//! PCSX-Redux save-state (`.sstate`) main-RAM reader - the bridge that lets the
//! cataloged PCSX-Redux playthrough anchors (`s1_newgame_field` ..
//! `s5_tetsu_battle`) feed the engine's disc-gated oracle tests the same way the
//! mednafen `.mc` saves already do.
//!
//! A `.sstate` is `gzip(rawsstate)`, where `rawsstate` is PCSX-Redux's
//! protobuf-encoded state. We don't need the protobuf schema: the 2 MiB main RAM
//! is located **format-agnostically** by the existing SCUS anchor search
//! ([`legaia_mednafen::extract::main_ram_via_anchor`]) - it matches a string
//! known to live in the loaded SCUS region (e.g. `h:\prot\cdname.dat`) in both
//! the SCUS binary and the decompressed payload and derives the RAM base. (For
//! the captured anchors the RAM happens to start at payload offset `0x27`, but
//! the anchor search makes the reader robust to that offset.)
//!
//! Disc-gated: the anchor search reads `extracted/SCUS_942.54` (or `$LEGAIA_SCUS`).

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

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

/// A loaded PCSX-Redux save state: just its 2 MiB main RAM, KSEG0-addressed.
pub struct SaveState {
    ram: Vec<u8>,
}

impl SaveState {
    /// Load + gunzip a `.sstate`, then locate main RAM via the SCUS anchor search.
    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_sstate_bytes(&raw)
    }

    /// Same as [`Self::from_path`] from in-memory `.sstate` (gzip) bytes.
    pub fn from_sstate_bytes(gz: &[u8]) -> Result<Self> {
        let mut payload = Vec::new();
        flate2::read::GzDecoder::new(gz)
            .read_to_end(&mut payload)
            .context("gunzip .sstate")?;
        let ram = legaia_mednafen::extract::main_ram_via_anchor(&payload)
            .context("locate main RAM in PCSX-Redux payload (anchor search)")?
            .to_vec();
        Ok(Self { ram })
    }

    /// The 2 MiB main RAM; index `0` is PSX virtual address `0x80000000`.
    pub fn main_ram(&self) -> &[u8] {
        &self.ram
    }

    fn off(&self, va: u32) -> usize {
        (va & 0x1F_FFFF) as usize
    }

    pub fn u8_at(&self, va: u32) -> u8 {
        self.ram[self.off(va)]
    }
    pub fn u16_at(&self, va: u32) -> u16 {
        let o = self.off(va);
        u16::from_le_bytes([self.ram[o], self.ram[o + 1]])
    }
    pub fn i16_at(&self, va: u32) -> i16 {
        self.u16_at(va) as i16
    }
    pub fn u32_at(&self, va: u32) -> u32 {
        let o = self.off(va);
        u32::from_le_bytes([
            self.ram[o],
            self.ram[o + 1],
            self.ram[o + 2],
            self.ram[o + 3],
        ])
    }

    /// Active CDNAME scene label (e.g. `"town01"`), trimmed at the first NUL /
    /// non-printable byte.
    pub fn scene_name(&self) -> String {
        let mut s = String::new();
        for i in 0..8 {
            let b = self.u8_at(SCENE_NAME_VA + i);
            if !(0x20..0x7f).contains(&b) {
                break;
            }
            s.push(b as char);
        }
        s
    }

    /// Next game-mode index (`0x03` = field-run, `0x15` = battle, ...).
    pub fn game_mode(&self) -> u8 {
        self.u8_at(GAME_MODE_VA)
    }

    /// The player actor struct pointer (`*0x8007C364`), or `None` if it is not a
    /// plausible KSEG0 main-RAM pointer.
    pub fn player_ptr(&self) -> Option<u32> {
        let p = self.u32_at(PLAYER_PTR_VA);
        ((p & 0xFFE0_0000) == 0x8000_0000).then_some(p)
    }

    /// Player world position `(x, z)` read as 16-bit signed from the player
    /// struct, or `None` if the struct pointer is implausible.
    pub fn player_pos(&self) -> Option<(i16, i16)> {
        let p = self.player_ptr()?;
        Some((self.i16_at(p + PLAYER_X_OFF), self.i16_at(p + PLAYER_Z_OFF)))
    }
}
