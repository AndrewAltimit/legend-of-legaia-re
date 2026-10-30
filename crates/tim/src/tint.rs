//! Per-channel tint of BGR555 palette words: the multiply an expansion
//! pack's stage `tint` and map `tint` materials apply to retail palettes.

/// The neutral tint channel: `128` leaves a channel unchanged, above it
/// brightens (up to about twice at `255`), below it darkens.
pub const TINT_NEUTRAL: u8 = 128;

/// One BGR555 word under a per-channel tint `[r, g, b]`
/// ([`TINT_NEUTRAL`] = unchanged). The STP bit is kept, and so is the
/// transparent word `0x0000`; a colour the tint takes to black becomes
/// opaque black (`0x8000`) rather than turning transparent.
pub fn tint_bgr555(word: u16, tint: [u8; 3]) -> u16 {
    if word == 0 {
        return 0;
    }
    let ch = |shift: u16, t: u8| -> u16 {
        let c = u32::from((word >> shift) & 0x1F);
        ((c * u32::from(t) + 64) / 128).min(0x1F) as u16
    };
    let rgb = ch(0, tint[0]) | (ch(5, tint[1]) << 5) | (ch(10, tint[2]) << 10);
    match rgb | (word & 0x8000) {
        0 => 0x8000,
        w => w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_neutral_tint_is_the_identity_and_black_stays_opaque() {
        for w in [0u16, 0x0001, 0x7FFF, 0x8000, 0x801F, 0x3DEF, 0xFC00] {
            assert_eq!(tint_bgr555(w, [TINT_NEUTRAL; 3]), w, "{w:#06x}");
        }
        // Red channel to nothing: a pure red goes opaque black, not clear.
        assert_eq!(tint_bgr555(0x001F, [0, 128, 128]), 0x8000);
        assert_eq!(tint_bgr555(0, [255; 3]), 0);
        // Doubling clamps at full.
        assert_eq!(tint_bgr555(0x0210, [255, 255, 255]) & 0x7FFF, 0x03FF);
    }
}
