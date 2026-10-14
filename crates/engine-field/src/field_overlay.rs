//! The field overlay image (PROT 0897) as data: its slot-A load base and
//! the reader for the NUL-terminated strings its data segment holds - the
//! submode windows' legends and headers, the incense notice, the coin
//! counter's labels.

/// Load base of the field overlay (PROT 0897), slot A.
pub const FIELD_OVERLAY_BASE: u32 = 0x801C_E818;

/// The NUL-terminated string at `va` in the field overlay image (PROT entry
/// `0897` as the disc holds it), without its terminator. `None` when `va`
/// sits below the load base, past the image, or the image ends before a
/// `NUL`.
pub fn field_overlay_cstr(image: &[u8], va: u32) -> Option<&[u8]> {
    let off = va.checked_sub(FIELD_OVERLAY_BASE)? as usize;
    let tail = image.get(off..)?;
    let end = tail.iter().position(|&b| b == 0)?;
    Some(&tail[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_to_the_nul_and_refuses_what_is_not_there() {
        let image = [b'a', b'b', 0, b'c'];
        assert_eq!(
            field_overlay_cstr(&image, FIELD_OVERLAY_BASE),
            Some(&b"ab"[..])
        );
        assert_eq!(
            field_overlay_cstr(&image, FIELD_OVERLAY_BASE + 2),
            Some(&b""[..])
        );
        // No terminator before the image ends.
        assert_eq!(field_overlay_cstr(&image, FIELD_OVERLAY_BASE + 3), None);
        // Below the load base, and past the image.
        assert_eq!(field_overlay_cstr(&image, FIELD_OVERLAY_BASE - 1), None);
        assert_eq!(field_overlay_cstr(&image, FIELD_OVERLAY_BASE + 9), None);
    }
}
