//! Battle **effect CLUT stage** - the palette-row copy the action-effect
//! script's table arm performs before it spawns a move-FX prototype.
//!
//! The arm lives in `FUN_801DEA50` at `0x801df0dc..0x801df134` (see
//! `ghidra/scripts/funcs/overlay_battle_action_801dea50.txt`). Read straight
//! off the disassembly:
//!
//! ```text
//! 801df0dc  beq   v0,zero,0x801df138   ; skip unless code < 0x32
//! 801df0e4  addiu t5,t5,0x6418         ; t5 = 0x801F6418
//! 801df0ec  lbu   v0,0x0(a3)           ; map[code]
//! 801df0f4  beq   v0,zero,0x801df138   ; zero = no copy
//! 801df100  li    a1,0xe0              ; dest x   = 224
//! 801df108  li    a2,0x1dc             ; dest y   = 476
//! 801df10c  addiu v0,a0,0x8            ; bump the 0x1F80031C+0x8C cursor
//! 801df11c  sh    a2,0x2(a0)           ; rect.y   = 476
//! 801df124  sh    v0,0x4(a0)           ; rect.w   = 16
//! 801df12c  sh    v0,0x6(a0)           ; rect.h   = 1
//! 801df130  jal   0x80058490           ; MoveImage
//! 801df134  _sh   v1,0x0(a0)           ; rect.x   = map[code]
//! ```
//!
//! `FUN_80058490` is **`MoveImage`**, not a sound submit: it registers itself
//! through `FUN_80058170(0x800156EC, self)` where `0x800156EC` is the ASCII
//! name, early-outs on a zero `rect->w` / `rect->h`, and packs
//! `(dest_y << 16) | dest_x` into a GP0 VRAM-to-VRAM blit
//! (`ghidra/scripts/funcs/80058490.txt`). The eight bytes at `a0` are a PsyQ
//! `RECT`, so the whole arm is one 16x1 VRAM block copy from
//! `(0x801F6418[code], 476)` to `(224, 476)` - a sixteen-entry palette row
//! swapped in under whatever the spawned effect draws.
//!
//! ## The claim this replaces
//!
//! `0x801F6418` was read across this repo as a per-effect **SFX cue map**, on
//! the strength of `FUN_80058490` being "the sound-driver command lane". Both
//! halves are false. The engine consequence was concrete: the effect-script
//! drain pushed the table byte into `World::battle_sfx_cues` as a cue id, so
//! the SFX scheduler was handed the values `0xB0` / `0xC0` / `0xD0` - VRAM x
//! coordinates - to look up in a sound bank. `docs/subsystems/battle-action.md`
//! and `docs/formats/move-power.md` carry the settled reading.
//!
//! The table's whole live value set over its `0x32` reachable entries is
//! `{0x00, 0xB0, 0xC0, 0xD0}`: three non-zero values, each a multiple of 16
//! and each a plausible CLUT column. A sound map would not be three-valued.

use legaia_tim::{VRAM_WIDTH, Vram};

/// VRAM row the effect palette lives on (`0x1DC`), source and destination
/// both - the copy moves sideways along one row.
pub const EFFECT_CLUT_ROW: u16 = 476;

/// Destination x of the copy (`0xE0`): the column the move-FX prototypes'
/// CBA words point at.
pub const EFFECT_CLUT_DEST_X: u16 = 224;

/// Entries copied (`0x10`) - one 16-colour CLUT.
pub const EFFECT_CLUT_ENTRIES: usize = 16;

/// Highest effect code that reads the map (`sltiu v0,v1,0x32` at
/// `0x801df0d8`): a code at or above this reads nothing, whatever the map
/// holds.
pub const EFFECT_CLUT_CODE_BOUND: u8 = 0x32;

/// Perform one effect CLUT stage against a host's software VRAM: copy the
/// sixteen entries at `(src_x, 476)` onto `(224, 476)`.
///
/// `src_x == 0` is retail's "no copy" sentinel and returns `false` without
/// touching VRAM, as does a source window that would run off the right edge.
/// Returns whether VRAM changed, so a host can skip its re-upload.
///
/// PORT: FUN_801DEA50 (the CLUT-stage arm, `0x801df0dc..0x801df134`)
/// REF: FUN_80058490 - `MoveImage`, the blit this reproduces.
pub fn stage_effect_clut(vram: &mut Vram, src_x: u8) -> bool {
    if src_x == 0 {
        return false;
    }
    let src_x = u16::from(src_x);
    if usize::from(src_x) + EFFECT_CLUT_ENTRIES > VRAM_WIDTH {
        return false;
    }
    let mut bytes = [0u8; EFFECT_CLUT_ENTRIES * 2];
    for (i, pair) in bytes.chunks_exact_mut(2).enumerate() {
        let px = vram.pixel(usize::from(src_x) + i, usize::from(EFFECT_CLUT_ROW));
        pair.copy_from_slice(&px.to_le_bytes());
    }
    // A copy onto itself is retail-legal and a no-op; report it as unchanged
    // so hosts do not re-upload for nothing.
    if src_x == EFFECT_CLUT_DEST_X {
        return false;
    }
    vram.write_clut_row(EFFECT_CLUT_DEST_X, EFFECT_CLUT_ROW, &bytes);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vram_with_row(x: u16, colours: &[u16]) -> Vram {
        let mut v = Vram::new();
        let mut bytes = Vec::new();
        for c in colours {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        v.write_clut_row(x, EFFECT_CLUT_ROW, &bytes);
        v
    }

    #[test]
    fn a_zero_map_byte_is_the_no_copy_sentinel() {
        let mut v = vram_with_row(0xD0, &[0x1234; EFFECT_CLUT_ENTRIES]);
        assert!(!stage_effect_clut(&mut v, 0));
        assert_eq!(v.pixel(usize::from(EFFECT_CLUT_DEST_X), 476), 0);
    }

    #[test]
    fn a_non_zero_map_byte_moves_sixteen_entries_onto_column_224() {
        let src: Vec<u16> = (0..EFFECT_CLUT_ENTRIES as u16).map(|i| 0x100 + i).collect();
        let mut v = vram_with_row(0xD0, &src);
        assert!(stage_effect_clut(&mut v, 0xD0));
        for (i, want) in src.iter().enumerate() {
            assert_eq!(v.pixel(usize::from(EFFECT_CLUT_DEST_X) + i, 476), *want);
        }
        // Only the sixteen destination entries moved.
        assert_eq!(
            v.pixel(usize::from(EFFECT_CLUT_DEST_X) + EFFECT_CLUT_ENTRIES, 476),
            0
        );
    }

    #[test]
    fn a_copy_onto_the_destination_column_reports_unchanged() {
        let mut v = vram_with_row(EFFECT_CLUT_DEST_X, &[0x7FFF; EFFECT_CLUT_ENTRIES]);
        assert!(!stage_effect_clut(&mut v, EFFECT_CLUT_DEST_X as u8));
    }
}
