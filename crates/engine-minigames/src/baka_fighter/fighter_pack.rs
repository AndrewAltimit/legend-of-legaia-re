//! The per-fighter pack installer: PROT entry fold and chunk walk.

// ------------------------------------------------------- per-fighter installer

/// PROT TOC base the per-fighter mesh installer adds the folded roster index
/// to. Entry `0x4b6` is extraction 1204, the battle-form party pack.
pub const FIGHTER_PACK_TOC_BASE: u32 = 0x4B6;
/// Roster ids at or above this fold down by the same amount - the three party
/// fighters share the one party pack.
pub const FIGHTER_PACK_FOLD: usize = 3;
/// Scratch buffer the installer allocates for the pack before walking it.
pub const FIGHTER_PACK_BUFFER: usize = 0x4_6000;
/// Mask that separates a chunk header word's size from its type byte.
pub const FIGHTER_CHUNK_SIZE_MASK: u32 = 0x00FF_FFFF;

/// One chunk of a fighter pack, as the installer's walk sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FighterChunk {
    /// Header byte `3` - the asset type the dispatcher switches on.
    pub kind: u8,
    /// Byte offset of the payload inside the pack.
    pub offset: usize,
    /// Payload length as the header declares it.
    pub len: usize,
}

/// Which PROT TOC entry the installer loads for a roster index, and the folded
/// index it uses.
///
/// REF: FUN_801d4c50 (`0x801D4D58..0x801D4D70`)
pub fn fighter_pack_entry(roster: usize) -> (usize, u32) {
    let folded = if roster >= FIGHTER_PACK_FOLD {
        roster - FIGHTER_PACK_FOLD
    } else {
        roster
    };
    (folded, FIGHTER_PACK_TOC_BASE + folded as u32)
}

// REPLACED-BY: `legaia_minigame_formats::baka_opponents::parse_fighter_pack`. The port
// resolves the fighter's PROT entry on demand and walks the same
// `[u32 (type<<24)|size][payload]` chunk chain into typed sub-asset bytes
// (TIM / TMD / ANM), which the browser duel host consumes directly; retail's
// transient `0x46000` buffer, its dev-vs-CD load fork and its dispatcher
// hand-off have no analogue to host.
/// PORT: FUN_801d4c50 - the per-fighter **mesh installer**'s chunk walk.
///
/// The installer allocates [`FIGHTER_PACK_BUFFER`] bytes, fills it either from
/// a named `data_field` path (the dev arm, taken only while the streaming flag
/// `_DAT_8007b8c2` is `0`) or from raw PROT entry [`fighter_pack_entry`] (the
/// retail arm), then walks the buffer as a chain of
/// `[u32 (type << 24) | size][payload]` chunks, handing each to the asset
/// dispatcher `FUN_8001F05C` with the "already decompressed" flag. The chain
/// ends at the first header whose size field is zero, and the buffer is freed
/// on the way out - the pack is transient, and whatever the dispatcher keeps
/// it has already copied.
///
/// The stride is the retail one and it is **not** `4 + size`: the size is
/// rounded down to a multiple of four first (`sra 2` then `sll 2`), so a chunk
/// whose declared length is not word-aligned advances short of its own end.
pub fn fighter_pack_chunks(pack: &[u8]) -> Vec<FighterChunk> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= pack.len() {
        let hdr = u32::from_le_bytes([pack[pos], pack[pos + 1], pack[pos + 2], pack[pos + 3]]);
        let size = (hdr & FIGHTER_CHUNK_SIZE_MASK) as usize;
        if size == 0 {
            break;
        }
        out.push(FighterChunk {
            kind: (hdr >> 24) as u8,
            offset: pos + 4,
            len: size,
        });
        pos += (size / 4) * 4 + 4;
    }
    out
}
