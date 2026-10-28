//! PCSX-Redux save-state (`.sstate`) main-RAM reader - the bridge that lets the
//! cataloged PCSX-Redux playthrough anchors (`s1_newgame_field` ..
//! `s5_tetsu_battle`) feed the engine's disc-gated oracle tests the same way the
//! mednafen `.mc` saves already do.
//!
//! A `.sstate` is PCSX-Redux's protobuf-encoded state, gzipped or bare. We
//! don't need the protobuf schema: the 2 MiB main RAM is located
//! **structurally**, as the first large member of the one submessage that also
//! holds a 64 KiB member (the memory submessage: RAM, BIOS, parallel port,
//! hardware). That rule reads only the emulator's layout, so it works on any
//! game's state ([`structural_ram_offset`]). For the captured anchors the RAM
//! starts at payload offset `0x27`.
//!
//! The older Legaia-specific route, matching a string from `SCUS_942.54` in
//! both the executable and the payload
//! ([`legaia_ram_image::extract::main_ram_via_anchor`]), stays as the fallback
//! for a payload the walk does not recognise, and a disc-gated test checks
//! that both routes land on the same offset for every library state.
//! [`SaveState::from_sstate_bytes_with_anchors`] is the same fallback with
//! another game's executable and [`GameProfile`].
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
//! The scene / mode / player accessors read Legaia's globals
//! ([`legaia_ram_image::game_anchors`]); [`SaveState::identity_with`] takes
//! another game's profile.

pub mod fixture;
pub mod gpu;

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

// The Legaia RAM anchors are shared with the mednafen reader - both land on the
// same KSEG0-addressed 2 MiB image, so "which scene is this state in?" has one
// implementation, in `legaia_ram_image::game_anchors`.
use legaia_ram_image::GameProfile;
use legaia_ram_image::game_anchors::StateIdentity;

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
    ram_source: RamSource,
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

/// The memory submessage of a decompressed `.sstate` payload, as its
/// length-delimited members `(payload_start, len)` in file order.
///
/// Structural rather than positional: walk the top-level message, and for each
/// length-delimited field that itself parses cleanly as a message, accept it
/// when it holds **both** a blob of exactly [`HARDWARE_LEN`] and one of at
/// least [`MIN_RAM_LEN`]. Nothing here depends on the field numbers or on the
/// blobs' file offsets, both of which are PCSX-Redux build details, and nothing
/// depends on the game: the rule reads only the emulator's own layout.
fn memory_members(payload: &[u8]) -> Option<Vec<(usize, usize)>> {
    for (start, len) in message_fields(payload, 0, payload.len())? {
        let Some(inner) = message_fields(payload, start, start + len) else {
            continue;
        };
        if inner.iter().any(|(_, l)| *l == HARDWARE_LEN)
            && inner.iter().any(|(_, l)| *l >= MIN_RAM_LEN)
        {
            return Some(inner);
        }
    }
    None
}

/// Locate the 64 KiB `hardware` blob: the memory submessage's member of
/// exactly that length.
fn find_hardware(payload: &[u8]) -> Option<&[u8]> {
    let (start, len) = *memory_members(payload)?
        .iter()
        .find(|(_, l)| *l == HARDWARE_LEN)?;
    payload.get(start..start + len)
}

/// Payload offset of main RAM, found structurally: the **first** member of the
/// memory submessage at least [`MIN_RAM_LEN`] long.
///
/// PCSX-Redux serialises the memory submessage as RAM, BIOS ROM (512 KiB),
/// parallel-port ROM and the 64 KiB hardware region, in that order. Its RAM
/// member is the emulator's whole RAM allocation (8 MiB, which covers the
/// debug-unit expansion); the console's 2 MiB is its first
/// [`legaia_ram_image::PSX_RAM_SIZE`] bytes. The parallel-port member can be
/// as large, which is why the rule takes the first such member and not any.
///
/// Works for any game's state; `None` when the payload carries no memory
/// submessage this rule recognises (then only the anchor search is left).
pub fn structural_ram_offset(payload: &[u8]) -> Option<usize> {
    memory_members(payload)?
        .iter()
        .find(|(_, l)| *l >= MIN_RAM_LEN)
        .map(|(start, _)| *start)
}

/// The decompressed protobuf payload of a `.sstate`: gunzipped when the bytes
/// start with the gzip magic `1f 8b`, else the bytes themselves (a state
/// written from a Lua probe's snapshot call is bare).
pub fn sstate_payload(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut buf = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut buf)
            .context("gunzip .sstate")?;
        Ok(std::borrow::Cow::Owned(buf))
    } else {
        Ok(std::borrow::Cow::Borrowed(bytes))
    }
}

/// How a [`SaveState`]'s main RAM was located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RamSource {
    /// Walked out of the protobuf memory submessage
    /// ([`structural_ram_offset`]); game-agnostic.
    Structural,
    /// Matched by an executable anchor string
    /// ([`legaia_ram_image::extract::main_ram_via_exe_anchors`]).
    Anchor,
}

impl SaveState {
    /// Load a `.sstate` from disk; see [`Self::from_sstate_bytes`].
    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_sstate_bytes(&raw)
    }

    /// Load a `.sstate` from disk with no fallback to an anchor search; see
    /// [`Self::from_sstate_bytes_structural`]. Needs no executable.
    pub fn from_path_structural(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_sstate_bytes_structural(&raw)
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
    /// Main RAM is located structurally first, which works for any game. Only
    /// a payload whose memory submessage is not recognised falls back to the
    /// Legaia anchor search, which reads `extracted/SCUS_942.54` (or
    /// `$LEGAIA_SCUS`); [`Self::from_sstate_bytes_with_anchors`] is that
    /// fallback for another game.
    pub fn from_sstate_bytes(bytes: &[u8]) -> Result<Self> {
        let payload = sstate_payload(bytes)?;
        if let Some(st) = Self::structural(&payload) {
            return Ok(st);
        }
        let ram = legaia_ram_image::extract::main_ram_via_anchor(&payload)
            .context("locate main RAM in PCSX-Redux payload (anchor search)")?
            .to_vec();
        Ok(Self::with_ram(ram, &payload, RamSource::Anchor))
    }

    /// Main RAM located structurally only, for any game; an error when the
    /// payload's memory submessage is not recognised.
    pub fn from_sstate_bytes_structural(bytes: &[u8]) -> Result<Self> {
        let payload = sstate_payload(bytes)?;
        Self::structural(&payload).context(
            "no PCSX-Redux memory submessage (a 64 KiB member beside one of at least 2 MiB) \
             in the state payload",
        )
    }

    /// Structural first, then the anchor search with `profile`'s strings
    /// matched in `exe` (the game's PSX executable, header included).
    pub fn from_sstate_bytes_with_anchors(
        bytes: &[u8],
        exe: &[u8],
        profile: &dyn GameProfile,
    ) -> Result<Self> {
        let payload = sstate_payload(bytes)?;
        Self::from_payload_with_anchors(&payload, exe, profile)
    }

    pub(crate) fn from_payload_with_anchors(
        payload: &[u8],
        exe: &[u8],
        profile: &dyn GameProfile,
    ) -> Result<Self> {
        if let Some(st) = Self::structural(payload) {
            return Ok(st);
        }
        let ram = legaia_ram_image::extract::main_ram_via_profile(payload, exe, profile)
            .with_context(|| {
                format!(
                    "locate main RAM in PCSX-Redux payload (anchor search, profile {})",
                    profile.name()
                )
            })?
            .to_vec();
        Ok(Self::with_ram(ram, payload, RamSource::Anchor))
    }

    fn structural(payload: &[u8]) -> Option<Self> {
        let at = structural_ram_offset(payload)?;
        let ram = payload
            .get(at..at + legaia_ram_image::PSX_RAM_SIZE)?
            .to_vec();
        Some(Self::with_ram(ram, payload, RamSource::Structural))
    }

    fn with_ram(ram: Vec<u8>, payload: &[u8], ram_source: RamSource) -> Self {
        // Absent rather than fatal: a state written by a build whose memory
        // submessage differs still yields main RAM, and most consumers only
        // want that.
        let hardware = find_hardware(payload).map(<[u8]>::to_vec);
        Self {
            ram,
            hardware,
            ram_source,
        }
    }

    /// How main RAM was located.
    pub fn ram_source(&self) -> RamSource {
        self.ram_source
    }

    /// The state's identity read through another game's [`GameProfile`];
    /// `None` when the profile knows no identity globals.
    pub fn identity_with(&self, profile: &dyn GameProfile) -> Option<StateIdentity> {
        profile.identify(&self.ram)
    }

    /// The 2 MiB main RAM; index `0` is PSX virtual address `0x80000000`.
    pub fn main_ram(&self) -> &[u8] {
        &self.ram
    }

    pub fn u8_at(&self, va: u32) -> u8 {
        legaia_ram_image::game_anchors::u8_at(&self.ram, va)
    }
    pub fn u16_at(&self, va: u32) -> u16 {
        legaia_ram_image::game_anchors::u16_at(&self.ram, va)
    }
    pub fn i16_at(&self, va: u32) -> i16 {
        legaia_ram_image::game_anchors::i16_at(&self.ram, va)
    }
    pub fn u32_at(&self, va: u32) -> u32 {
        legaia_ram_image::game_anchors::u32_at(&self.ram, va)
    }

    /// Active CDNAME scene label (e.g. `"town01"`), trimmed at the first NUL /
    /// non-printable byte.
    pub fn scene_name(&self) -> String {
        legaia_ram_image::game_anchors::scene_name(&self.ram)
    }

    /// Next game-mode index (`0x03` = field-run, `0x15` = battle, ...).
    pub fn game_mode(&self) -> u8 {
        legaia_ram_image::game_anchors::game_mode(&self.ram)
    }

    /// The player actor struct pointer (`*0x8007C364`), or `None` if it is not a
    /// plausible KSEG0 main-RAM pointer.
    pub fn player_ptr(&self) -> Option<u32> {
        legaia_ram_image::game_anchors::player_ptr(&self.ram)
    }

    /// Player world position `(x, z)` read as 16-bit signed from the player
    /// struct, or `None` if the struct pointer is implausible.
    pub fn player_pos(&self) -> Option<(i16, i16)> {
        legaia_ram_image::game_anchors::player_pos(&self.ram)
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
        legaia_ram_image::game_anchors::identify(&self.ram)
    }
}
