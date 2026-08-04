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

// The Legaia RAM anchors are shared with the mednafen reader - both land on the
// same KSEG0-addressed 2 MiB image, so "which scene is this state in?" has one
// implementation. Re-exported here so existing `legaia_pcsxr::SCENE_NAME_VA`
// consumers keep resolving.
pub use legaia_mednafen::game_anchors::{
    GAME_MODE_VA, PLAYER_PTR_VA, PLAYER_X_OFF, PLAYER_Z_OFF, SCENE_NAME_VA, StateIdentity,
};

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

    /// Same as [`Self::from_path`] from in-memory `.sstate` bytes.
    ///
    /// PCSX-Redux writes a `.sstate` **either** gzipped **or** as the bare
    /// protobuf, depending on how it was produced: the emulator's own
    /// save-state slots are gzipped, while a state written from a Lua probe's
    /// snapshot call is not (the repo's `captures/**/snap_*.sstate` and
    /// `autosave_*.sstate` are all bare, ~19 MB each). Treating the format as
    /// "always gzip" silently drops most of the capture corpus with
    /// `invalid gzip header`, so dispatch on the magic instead: `1f 8b` is
    /// gzip, anything else is already the payload.
    ///
    /// Either way the main RAM is found by the same format-agnostic anchor
    /// search, so nothing downstream needs to know which shape it came from.
    pub fn from_sstate_bytes(bytes: &[u8]) -> Result<Self> {
        let owned: Vec<u8>;
        let payload: &[u8] = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut buf = Vec::new();
            flate2::read::GzDecoder::new(bytes)
                .read_to_end(&mut buf)
                .context("gunzip .sstate")?;
            owned = buf;
            &owned
        } else {
            bytes
        };
        let ram = legaia_mednafen::extract::main_ram_via_anchor(payload)
            .context("locate main RAM in PCSX-Redux payload (anchor search)")?
            .to_vec();
        Ok(Self { ram })
    }

    /// The 2 MiB main RAM; index `0` is PSX virtual address `0x80000000`.
    pub fn main_ram(&self) -> &[u8] {
        &self.ram
    }

    pub fn u8_at(&self, va: u32) -> u8 {
        legaia_mednafen::game_anchors::u8_at(&self.ram, va)
    }
    pub fn u16_at(&self, va: u32) -> u16 {
        legaia_mednafen::game_anchors::u16_at(&self.ram, va)
    }
    pub fn i16_at(&self, va: u32) -> i16 {
        legaia_mednafen::game_anchors::i16_at(&self.ram, va)
    }
    pub fn u32_at(&self, va: u32) -> u32 {
        legaia_mednafen::game_anchors::u32_at(&self.ram, va)
    }

    /// Active CDNAME scene label (e.g. `"town01"`), trimmed at the first NUL /
    /// non-printable byte.
    pub fn scene_name(&self) -> String {
        legaia_mednafen::game_anchors::scene_name(&self.ram)
    }

    /// Next game-mode index (`0x03` = field-run, `0x15` = battle, ...).
    pub fn game_mode(&self) -> u8 {
        legaia_mednafen::game_anchors::game_mode(&self.ram)
    }

    /// The player actor struct pointer (`*0x8007C364`), or `None` if it is not a
    /// plausible KSEG0 main-RAM pointer.
    pub fn player_ptr(&self) -> Option<u32> {
        legaia_mednafen::game_anchors::player_ptr(&self.ram)
    }

    /// Player world position `(x, z)` read as 16-bit signed from the player
    /// struct, or `None` if the struct pointer is implausible.
    pub fn player_pos(&self) -> Option<(i16, i16)> {
        legaia_mednafen::game_anchors::player_pos(&self.ram)
    }

    /// Scene + mode + player position in one record.
    pub fn identity(&self) -> StateIdentity {
        legaia_mednafen::game_anchors::identify(&self.ram)
    }
}
