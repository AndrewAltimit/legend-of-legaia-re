//! The desaturate step of the stone / petrify CLUT-fade builder
//! ([`bgr555_to_grey`], the arithmetic core of `FUN_8004ce2c`).
//!
//! Read out of the instruction stream in the reference dump
//! (`ghidra/scripts/funcs/8004ce2c.txt`), not the decompiled C. No
//! `SCUS_942.54` bytes live in this crate. The render-track parent is
//! documented, with provenance, in `docs/subsystems/battle.md` ("CLUT status
//! recolour").
//!
//! # Wiring
//!
//! Live: `engine-battle::battle_status_clut` calls it per latched party slot.
//! The source is the battle context's own per-actor **240-entry palette** at
//! `ctx[+0x894]` (`0x8004D6F8 lhu v1,0x894(v0)`), the greyed copy is staged at
//! `ctx[+0xE34]` and a 1-pixel-tall rect uploads it to VRAM CLUT row
//! `481 + slot` (`0x8004D764..0x8004D798`).
//! `legaia_actor_formats::battle_char_palette` decodes the party CLUTs and the
//! loader STP-copies them to `481 + slot`; `engine-battle::battle_status_clut`
//! holds the per-actor palette copy and the `actor[+0x220..=+0x223]` latch,
//! `BattleHud::sync_status` arms it, and both play hosts run the pass against
//! their battle VRAM through `engine-session::battle_vram::step_battle_vram_edits`.
//!
//! Two limits on that wire. The **Rot arm** is out: it tints over a
//! per-character index window from `DAT_80078630`, which no crate parses.
//!
//! And the pass, though live, does not fire in ordinary play. Retail's
//! enemy-special status source is the **move-power record's `+0x0A`
//! impact-effect selector**, which `FUN_801DEA50` parks at `ctx+0x1014` and
//! `FUN_801E09F8` branches on at the impact phase;
//! `legaia_game_tables::move_power` parses that byte and `engine-core`'s
//! `World::apply_enemy_move_status` applies it, so a monster special puts
//! Venom / Toxic / Rot on a *party* slot in play. But no `+0x0A` selector
//! value maps to Stone: that ladder ends at `5` (`0x801E1620` tests only
//! `5`), and `FUN_801EC3E4`'s sibling ladder adds only `6` = Curse.
//! Petrification's applier is elsewhere entirely - `ori 0x4` at `0x80041CF4`
//! / `0x80041DE4`, in the SCUS `0x80041...` item/effect band around
//! `FUN_800402F4` - and that one is unported. Since `BattleHud::sync_status`
//! arms the CLUT latch on `StatusKind::Stone` only, rows `481..=483` stay
//! unexercised until that applier lands.
//!
//! It is ported as its own kernel because its edge case (the luminance clamp)
//! is observable and testable on its own.

/// Desaturate one 15-bit `BGR555` pixel to grey - the arithmetic core of the
/// petrify CLUT-fade builder `FUN_8004ce2c`.
///
/// PORT: FUN_8004ce2c
///
/// The retail routine walks the acting actor's 240-entry palette copy in the
/// battle context (`ctx[+0x894]`), and for each entry computes a single
/// luminance value and writes it into all three 5-bit channels, staging the
/// grey CLUT at `ctx[+0xE34]` for upload to VRAM row `481 + slot`. It is a
/// palette, not a framebuffer strip - the pixels on screen are never read.
/// Straight off the disassembly (`0x8004d700`):
///
/// - `r = pixel & 0x1F`, `g = (pixel >> 5) & 0x1F`, `b = (pixel >> 10) & 0x1F`.
/// - `lum = (r + g + b) >> 2` (an arithmetic shift; the sum is non-negative
///   so it is a plain floor-divide by four - note the divisor is 4, not 3, so
///   this is a deliberately-dimmed average, max `0x17`).
/// - `if lum > 0x1F { lum = 0x1F }` (`sltiu ..., 0x20`).
/// - repack `lum | (lum << 5) | (lum << 10)`.
///
/// The top `STP` bit (`0x8000`) is **not** set by this variant - it is added
/// by the sibling brightened path (`lum * 3 / 2`), which is a separate ramp
/// and not reproduced here. The surrounding routine (packet build via
/// `_DAT_1f8003a0`, `FUN_800583c8` submit) is render-track; see
/// `docs/subsystems/battle.md`.
pub fn bgr555_to_grey(pixel: u16) -> u16 {
    let r = pixel & 0x1F;
    let g = (pixel >> 5) & 0x1F;
    let b = (pixel >> 10) & 0x1F;
    let lum = ((r + g + b) >> 2).min(0x1F);
    lum | (lum << 5) | (lum << 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grey_replicates_luminance_into_all_channels() {
        // White (all 0x1F): sum 0x5D >> 2 = 0x17, clamp keeps 0x17.
        let g = bgr555_to_grey(0x7FFF);
        let lum = g & 0x1F;
        assert_eq!(lum, 0x17);
        assert_eq!((g >> 5) & 0x1F, lum);
        assert_eq!((g >> 10) & 0x1F, lum);
        assert_eq!(g & 0x8000, 0, "STP bit is never set by this variant");
    }

    #[test]
    fn grey_black_stays_black() {
        assert_eq!(bgr555_to_grey(0x0000), 0);
    }

    #[test]
    fn grey_averages_the_three_5bit_channels_floor_divided_by_four() {
        // r=0x1F, g=0, b=0 -> sum 0x1F >> 2 = 7.
        let g = bgr555_to_grey(0x001F);
        assert_eq!(g & 0x1F, 7);
        assert_eq!(g, 7 | (7 << 5) | (7 << 10));
    }

    #[test]
    fn grey_output_is_a_valid_15bit_word_with_equal_channels() {
        // A single 555 pixel can never exceed lum 0x17, so the top bit stays
        // clear and all three channels always match for every input.
        for p in [0x7FFFu16, 0x03FF, 0x7C1F, 0x0000] {
            let g = bgr555_to_grey(p);
            let lum = g & 0x1F;
            assert_eq!((g >> 5) & 0x1F, lum);
            assert_eq!((g >> 10) & 0x1F, lum);
            assert!(g < 0x8000, "no STP bit, so top bit clear");
        }
    }
}
