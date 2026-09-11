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
//! A `.sstate` is **not** main RAM only. The protobuf's memory submessage
//! carries four length-delimited blobs - RAM, BIOS ROM, parallel port and a
//! 64 KiB `hardware` region - and the PSX **scratchpad** (`0x1F800000`, 1 KiB)
//! is that last blob's first kilobyte, with the memory-mapped I/O registers
//! (`0x1F801000+`) filling the rest. [`SaveState::scratchpad`] exposes it, so
//! the scratchpad globals the field code lives on (the visible-tile window at
//! `0x1F8003E8`, the scene-map pointer at `0x1F8003EC`, the floor LUT at
//! `0x1F80035C`) are readable from a capture instead of only from a live
//! probe.
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

/// PSX scratchpad base address (`0x1F800000`), the "fast RAM" the field and
/// battle code keep their hot globals in.
pub const SCRATCHPAD_BASE: u32 = 0x1F80_0000;

/// Scratchpad size in bytes (1 KiB). The `.sstate` hardware blob it starts is
/// 64 KiB; the rest is the memory-mapped I/O register window.
pub const SCRATCHPAD_LEN: usize = 0x400;

/// Size of the `hardware` blob a PCSX-Redux state carries (`psxH`).
const HARDWARE_LEN: usize = 0x1_0000;

/// Smallest blob in the same submessage that can be the RAM image - the guard
/// that keeps a stray 64 KiB field elsewhere in the state from being read as
/// the hardware region.
const MIN_RAM_LEN: usize = 0x20_0000;

/// A loaded PCSX-Redux save state: its main RAM (KSEG0-addressed) and, when
/// the state carries one, the 64 KiB hardware blob whose first kilobyte is the
/// scratchpad.
pub struct SaveState {
    ram: Vec<u8>,
    hardware: Option<Vec<u8>>,
}

/// Read one protobuf varint at `off`, returning `(value, next_offset)`.
fn varint(buf: &[u8], mut off: usize) -> Option<(u64, usize)> {
    let mut out: u64 = 0;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(off)?;
        off += 1;
        out |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Some((out, off));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// Walk one protobuf message, yielding every length-delimited field as
/// `(payload_start, len)`. Returns `None` if the message does not consume
/// exactly `[start, end)` - which is what makes "does this blob parse as a
/// message?" a usable test rather than a guess.
fn message_fields(buf: &[u8], start: usize, end: usize) -> Option<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    let mut off = start;
    while off < end {
        let (tag, o) = varint(buf, off)?;
        match tag & 7 {
            0 => {
                let (_, o2) = varint(buf, o)?;
                off = o2;
            }
            1 => off = o.checked_add(8)?,
            2 => {
                let (len, o2) = varint(buf, o)?;
                let len = usize::try_from(len).ok()?;
                let stop = o2.checked_add(len)?;
                if stop > end {
                    return None;
                }
                out.push((o2, len));
                off = stop;
            }
            5 => off = o.checked_add(4)?,
            _ => return None,
        }
    }
    (off == end).then_some(out)
}

/// Locate the 64 KiB `hardware` blob in a decompressed `.sstate` payload.
///
/// Structural rather than positional: walk the top-level message, and for each
/// length-delimited field that itself parses cleanly as a message, accept it
/// when it holds **both** a blob of exactly [`HARDWARE_LEN`] and one of at
/// least [`MIN_RAM_LEN`] - i.e. the memory submessage, whose 64 KiB member is
/// the hardware region. Nothing here depends on the field numbers or on the
/// blob's file offset, both of which are PCSX-Redux build details.
fn find_hardware(payload: &[u8]) -> Option<&[u8]> {
    for (start, len) in message_fields(payload, 0, payload.len())? {
        let Some(inner) = message_fields(payload, start, start + len) else {
            continue;
        };
        let Some(hw) = inner.iter().find(|(_, l)| *l == HARDWARE_LEN) else {
            continue;
        };
        if !inner.iter().any(|(_, l)| *l >= MIN_RAM_LEN) {
            continue;
        }
        return payload.get(hw.0..hw.0 + hw.1);
    }
    None
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
        // Absent rather than fatal: a state written by a build whose memory
        // submessage differs still yields main RAM, and every existing
        // consumer only wants that.
        let hardware = find_hardware(payload).map(<[u8]>::to_vec);
        Ok(Self { ram, hardware })
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

    /// The state's 64 KiB `hardware` blob (`psxH`), or `None` when the payload
    /// carries no memory submessage this reader recognises.
    pub fn hardware(&self) -> Option<&[u8]> {
        self.hardware.as_deref()
    }

    /// The 1 KiB PSX **scratchpad** (`0x1F800000..0x1F800400`) - the first
    /// kilobyte of [`Self::hardware`]. `None` when the blob is absent.
    pub fn scratchpad(&self) -> Option<&[u8]> {
        self.hardware().map(|h| &h[..SCRATCHPAD_LEN])
    }

    /// Read a `u32` from a scratchpad address (`0x1F8003EC`, ...). `None` when
    /// the state has no scratchpad or the address is outside it.
    pub fn scratchpad_u32_at(&self, va: u32) -> Option<u32> {
        let off = usize::try_from(va.checked_sub(SCRATCHPAD_BASE)?).ok()?;
        let sp = self.scratchpad()?;
        let b = sp.get(off..off + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a `u8` from a scratchpad address.
    pub fn scratchpad_u8_at(&self, va: u32) -> Option<u8> {
        let off = usize::try_from(va.checked_sub(SCRATCHPAD_BASE)?).ok()?;
        self.scratchpad()?.get(off).copied()
    }

    /// Scene + mode + player position in one record.
    pub fn identity(&self) -> StateIdentity {
        legaia_mednafen::game_anchors::identify(&self.ram)
    }
}
