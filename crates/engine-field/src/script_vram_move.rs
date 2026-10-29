//! The field VM's literal-operand VRAM `MoveImage` stamp (op `4C 60`), the
//! rect copy both the field queue and the battle-side stamps carry.

/// One queued literal-operand VRAM rect copy - the field-VM op `4C 60`.
///
/// The 14-byte instruction `[4C, 60, src_x, src_y, w, h, dst_x, dst_y]`
/// carries six little-endian u16s (read via the misaligned-u16 helper
/// `FUN_8003CE9C`); the handler arm at `0x801E1B28..0x801E1B90` inside the
/// field-VM dispatcher `FUN_801DE840` hands them straight to the libgpu
/// `MoveImage` wrapper (`jal FUN_80058490` at `0x801E1B84`) - a `w x h`
/// halfword VRAM-to-VRAM rect copy. Retail scripts use it for the one-shot
/// face-frame stamps onto the player texture atlas (blink / mouth variants;
/// see `docs/formats/character-mesh.md` "Runtime scroll-cell residue").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptVramMove {
    /// Source rect origin `(x, y)` in VRAM halfword coordinates.
    pub src: (i16, i16),
    /// Rect size `(w, h)` - halfwords x rows.
    pub size: (i16, i16),
    /// Destination rect origin `(x, y)`.
    pub dst: (i16, i16),
}

impl ScriptVramMove {
    /// Build from the six decoded operand words in instruction order
    /// `[src_x, src_y, w, h, dst_x, dst_y]` - the payload the field VM hands
    /// the `FieldScreenHost::vram_move_image` host hook.
    pub fn from_words(words: [i16; 6]) -> Self {
        Self {
            src: (words[0], words[1]),
            size: (words[2], words[3]),
            dst: (words[4], words[5]),
        }
    }
}
