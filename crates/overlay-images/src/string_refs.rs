//! Where a code image forms a string's address, so a reader finds a string a
//! language pack moved.
//!
//! A translated string longer than its span **moves**: `legaia-patcher
//! translate import` places it elsewhere in its image and rewrites every
//! instruction and pointer word that forms its address
//! (`docs/tooling/translation/space-and-budgets.md`, "Moving a label").
//! Retail follows the rewritten code; a reader that slices the image at the
//! retail VA reads whatever the compaction left there. A reader that keeps a
//! table of one [`StrRef`] per string decodes the site on the image at hand
//! instead ([`resolve`]).
//!
//! The tables are coordinates only (instruction and pointer-word addresses),
//! each the output of the translation pipeline's own reference scan
//! (`legaia_translate::code_refs::scan`) over the retail image; the bytes are
//! read from the image the user supplied.

/// One site that forms a string's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrRef {
    /// `lui rX, %hi` at `lui`, completed by `addiu rX, rX, %lo` (or `ori`)
    /// at `lo`.
    Pair { lui: u32, lo: u32 },
    /// An aligned pointer word at this VA.
    Word(u32),
}

impl StrRef {
    /// The address this site forms in `image` (loaded so that byte 0 sits at
    /// `base_va`) as it reads now, or `None` when the bytes there are not the
    /// site's shape - another build, or a code mod over the same
    /// instructions.
    pub fn address(self, image: &[u8], base_va: u32) -> Option<u32> {
        let word = |va: u32| -> Option<u32> {
            let off = va.checked_sub(base_va)? as usize;
            image
                .get(off..off.checked_add(4)?)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        match self {
            Self::Word(va) => word(va),
            Self::Pair { lui, lo } => {
                let (l, c) = (word(lui)?, word(lo)?);
                let rt = (l >> 16) & 0x1F;
                if l >> 26 != 0x0F || rt == 0 || (c >> 21) & 0x1F != rt {
                    return None;
                }
                let hi = (l & 0xFFFF) << 16;
                let imm = c & 0xFFFF;
                match c >> 26 {
                    0x09 => Some(hi.wrapping_add(imm as u16 as i16 as i32 as u32)),
                    0x0D => Some(hi | imm),
                    _ => None,
                }
            }
        }
    }
}

/// The site `table` (sorted by retail VA) names for `retail_va`.
pub fn lookup(table: &[(u32, StrRef)], retail_va: u32) -> Option<StrRef> {
    table
        .binary_search_by_key(&retail_va, |&(va, _)| va)
        .ok()
        .map(|i| table[i].1)
}

/// Where the string retail keeps at `retail_va` lives in `image`: the
/// address its site forms there, which is `retail_va` itself on an
/// unmodified image. A VA with no site, a site whose bytes no longer read as
/// one, or an address outside the image answers `retail_va`.
pub fn resolve(table: &[(u32, StrRef)], image: &[u8], base_va: u32, retail_va: u32) -> u32 {
    lookup(table, retail_va)
        .and_then(|site| site.address(image, base_va))
        .filter(|&a| {
            a.checked_sub(base_va)
                .is_some_and(|off| (off as usize) < image.len())
        })
        .unwrap_or(retail_va)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u32 = 0x8001_0000;

    fn put(img: &mut [u8], va: u32, w: u32) {
        let o = (va - BASE) as usize;
        img[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }

    #[test]
    fn a_pair_and_a_word_follow_a_move() {
        let table = [
            (
                0x8001_0800,
                StrRef::Pair {
                    lui: 0x8001_0100,
                    lo: 0x8001_0108,
                },
            ),
            (0x8001_0900, StrRef::Word(0x8001_0200)),
        ];
        let mut img = vec![0u8; 0x9000];
        // `lui a0,0x8001 ; ... ; addiu a0,a0,0x0a10`.
        put(&mut img, 0x8001_0100, 0x3C04_8001);
        put(&mut img, 0x8001_0108, 0x2484_0A10);
        put(&mut img, 0x8001_0200, 0x8001_0A40);
        assert_eq!(resolve(&table, &img, BASE, 0x8001_0800), 0x8001_0A10);
        assert_eq!(resolve(&table, &img, BASE, 0x8001_0900), 0x8001_0A40);
        // A negative low half: `0x8002_0000 - 0x75F0`.
        put(&mut img, 0x8001_0100, 0x3C04_8002);
        put(&mut img, 0x8001_0108, 0x2484_8A10);
        assert_eq!(resolve(&table, &img, BASE, 0x8001_0800), 0x8001_8A10);
    }

    #[test]
    fn an_unrecognised_site_keeps_the_retail_va() {
        let table = [
            (
                0x8001_0800,
                StrRef::Pair {
                    lui: 0x8001_0100,
                    lo: 0x8001_0108,
                },
            ),
            (0x8001_0900, StrRef::Word(0x8001_0200)),
        ];
        let mut img = vec![0u8; 0x1000];
        assert_eq!(resolve(&table, &img, BASE, 0x8001_0800), 0x8001_0800);
        // The completion names another register.
        put(&mut img, 0x8001_0100, 0x3C04_8001);
        put(&mut img, 0x8001_0108, 0x24A5_0A10);
        assert_eq!(resolve(&table, &img, BASE, 0x8001_0800), 0x8001_0800);
        // A word pointing outside the image.
        put(&mut img, 0x8001_0200, 0x8002_0000);
        assert_eq!(resolve(&table, &img, BASE, 0x8001_0900), 0x8001_0900);
        assert_eq!(resolve(&table, &img, BASE, 0x8001_0904), 0x8001_0904);
        assert_eq!(resolve(&table, &[], BASE, 0x8001_0800), 0x8001_0800);
    }
}
