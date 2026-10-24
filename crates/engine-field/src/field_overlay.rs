//! The field overlay image (PROT 0897) as data: its slot-A load base and
//! the reader for the NUL-terminated strings its data segment holds - the
//! submode windows' legends and headers, the incense notice, the coin
//! counter's labels, the tile board's quit prompt.
//!
//! ## Following a moved string
//!
//! A translated string longer than its span **moves**: `legaia-patcher
//! translate import` places it elsewhere in the image and rewrites every
//! instruction and pointer word that forms its address
//! (`docs/tooling/translation/space-and-budgets.md`, "Moving a label").
//! Retail follows the rewritten code. [`field_overlay_cstr`] does the same:
//! [`STRING_REFS`] names one site per string the port reads - the drawing
//! routine's `lui` / `addiu` pair that loads the string into `a0` - and the
//! reader decodes that site on the image at hand
//! (`legaia_overlay_images::string_refs`) before slicing. On retail every
//! site forms its own VA; a site whose bytes no longer read as one (another
//! build, a code mod over the same instructions) falls back to the retail
//! VA. The name-entry prompts are read through the overlay's own pointer
//! table instead (`legaia_ui_formats::name_entry_prompts`).
//!
//! The table is coordinates only (instruction addresses), the output of the
//! translation pipeline's own reference scan (`legaia_translate::code_refs::scan`)
//! over the retail image; the disc-gated `translated_field_strings_disc`
//! test (`engine-scene-host`) checks every entry against that scan and
//! against a relocating import.

use legaia_overlay_images::string_refs::{self, StrRef};

/// Load base of the field overlay (PROT 0897), slot A.
pub const FIELD_OVERLAY_BASE: u32 = 0x801C_E818;

const fn pair(lui: u32, lo: u32) -> StrRef {
    StrRef::Pair { lui, lo }
}

/// Retail VA of each field-overlay string the port reads -> one site that
/// forms it, sorted by VA. Every string here has exactly one referencing
/// pair except `Yes` / `No`, which the tile board's render tail, the
/// three-line panel painter and the name-entry pointer table all reach; the
/// site listed is the tile board's (an import rewrites every site of a
/// string it moves, so any one of them names the new address).
pub const STRING_REFS: &[(u32, StrRef)] = &[
    // The code lock's header (`FUN_801E9B3C`'s record-12 painter).
    (0x801C_F09C, pair(0x801F_17E4, 0x801F_17E8)),
    // The flag window's legend.
    (0x801C_F0B8, pair(0x801E_6AB8, 0x801E_6ABC)),
    (0x801C_F0C4, pair(0x801E_6ADC, 0x801E_6AE0)),
    // The coin counter's entry-panel labels.
    (0x801C_F0D4, pair(0x801E_6FE8, 0x801E_6FEC)),
    (0x801C_F0E0, pair(0x801E_7034, 0x801E_7038)),
    (0x801C_F0F0, pair(0x801E_70E0, 0x801E_70E4)),
    (0x801C_F0FC, pair(0x801E_7118, 0x801E_711C)),
    // The three-line panel: `OK?`, then the `Yes` / `No` the tile board's
    // quit prompt shares.
    (0x801C_F108, pair(0x801F_1898, 0x801F_189C)),
    (0x801C_F10C, pair(0x801E_FF08, 0x801E_FF0C)),
    (0x801C_F110, pair(0x801E_FF54, 0x801E_FF58)),
    // The two-option panel.
    (0x801C_F138, pair(0x801F_19A4, 0x801F_19A8)),
    (0x801C_F140, pair(0x801F_19E4, 0x801F_19E8)),
    // The count-gated recovery lines.
    (0x801C_F14C, pair(0x801F_1A58, 0x801F_1A60)),
    (0x801C_F170, pair(0x801F_1A68, 0x801F_1A6C)),
    // The two-line panel.
    (0x801C_F190, pair(0x801F_1AB8, 0x801F_1ABC)),
    (0x801C_F198, pair(0x801F_1AFC, 0x801F_1B00)),
    // The single-line panel: the Incense wear-off notice.
    (0x801C_F1A4, pair(0x801F_1B70, 0x801F_1B74)),
    // The tile board's quit-prompt title.
    (0x801C_F650, pair(0x801E_FEDC, 0x801E_FEE0)),
];

/// Where the string retail keeps at `retail_va` lives in `image`: the
/// address its [`STRING_REFS`] site forms there, or `retail_va` itself on an
/// unmodified image, for a VA without a site, or when the site no longer
/// reads as one.
pub fn resolve_va(image: &[u8], retail_va: u32) -> u32 {
    string_refs::resolve(STRING_REFS, image, FIELD_OVERLAY_BASE, retail_va)
}

/// The NUL-terminated string retail keeps at `va` in the field overlay image
/// (PROT entry `0897` as the disc holds it), without its terminator, read
/// where the image's code now forms its address ([`resolve_va`]). `None`
/// when that address sits below the load base, past the image, or the image
/// ends before a `NUL`.
pub fn field_overlay_cstr(image: &[u8], va: u32) -> Option<&[u8]> {
    let off = resolve_va(image, va).checked_sub(FIELD_OVERLAY_BASE)? as usize;
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

    #[test]
    fn the_table_is_sorted_and_unique() {
        assert!(STRING_REFS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    fn put(img: &mut [u8], va: u32, b: &[u8]) {
        let o = (va - FIELD_OVERLAY_BASE) as usize;
        img[o..o + b.len()].copy_from_slice(b);
    }

    /// The `lui a0,%hi ; addiu a0,a0,%lo` pair a site names, forming `to`.
    fn put_pair(img: &mut [u8], site: StrRef, to: u32) {
        let StrRef::Pair { lui, lo } = site else {
            unreachable!()
        };
        let hi = (to.wrapping_add(0x8000) >> 16) as u16;
        put(img, lui, &(0x3C04_0000 | u32::from(hi)).to_le_bytes());
        put(img, lo, &(0x2484_0000 | (to & 0xFFFF)).to_le_bytes());
    }

    #[test]
    fn a_moved_string_is_read_where_its_site_now_points() {
        let mut img = vec![0u8; 0x25000];
        let header = 0x801C_F09C;
        let site = string_refs::lookup(STRING_REFS, header).unwrap();
        put(&mut img, header, b"Please enter the password.\0");
        // Retail: the site forms the retail VA.
        put_pair(&mut img, site, header);
        assert_eq!(
            field_overlay_cstr(&img, header),
            Some(&b"Please enter the password."[..])
        );
        // The import moved the translation and rewrote the pair; the stale
        // bytes at the retail VA belong to a neighbour now.
        put(&mut img, header, b"Entrez\0");
        put(&mut img, 0x801C_F200, b"Entrez le mot de passe\x82.\0");
        put_pair(&mut img, site, 0x801C_F200);
        assert_eq!(
            field_overlay_cstr(&img, header),
            Some(&b"Entrez le mot de passe\x82."[..])
        );
        // A code mod over the pair (no longer a `lui`): the retail VA.
        put(&mut img, 0x801F_17E4, &0u32.to_le_bytes());
        assert_eq!(field_overlay_cstr(&img, header), Some(&b"Entrez"[..]));
    }
}
