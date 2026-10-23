//! Where the menu overlay's own code reaches each string the port reads out
//! of it (PROT 0899), so a reader finds a string that a language pack moved.
//!
//! A translated label longer than its span **moves**: `legaia-patcher
//! translate import` places it elsewhere in the image and rewrites every
//! instruction and pointer word that forms its address
//! (`docs/tooling/translation/space-and-budgets.md`, "Moving a label"). Retail
//! follows the rewritten code; a reader that slices the image at the retail
//! VA would read whatever the compaction left there. Each entry below names
//! one site that forms the string's address on the retail disc - the `lui` /
//! `addiu` pair the renderer loads into `a0`, or the pointer word of the
//! Options screen's string table at `0x801E442C..` - and [`resolve_va`]
//! decodes that site on the image at hand.
//!
//! Coordinates only: the sites are instruction and table addresses; the
//! bytes are read from the image the user supplied. The table is the output
//! of the translation pipeline's own reference scan
//! (`legaia_translate::code_refs::scan`) over the retail image, and the
//! disc-gated `translated_menu_strings_disc` test (`engine-menus`) checks
//! every entry against it and against a relocating import.

use crate::menu_windows::MENU_OVERLAY_BASE_VA;

/// One site that forms a pool string's address
/// (`legaia_overlay_images::string_refs`).
pub use legaia_overlay_images::string_refs::StrRef as PoolRef;

/// Retail VA -> one site that forms it, sorted by VA.
pub const POOL_REFS: &[(u32, PoolRef)] = &[
    (0x801C_E81C, PoolRef::Word(0x801E_44B0)),
    (0x801C_E828, PoolRef::Word(0x801E_44AC)),
    (0x801C_E830, PoolRef::Word(0x801E_44A8)),
    (0x801C_E83C, PoolRef::Word(0x801E_44A4)),
    (0x801C_E850, PoolRef::Word(0x801E_4498)),
    (0x801C_E860, PoolRef::Word(0x801E_448C)),
    (0x801C_E86C, PoolRef::Word(0x801E_4488)),
    (0x801C_E87C, PoolRef::Word(0x801E_4484)),
    (0x801C_E88C, PoolRef::Word(0x801E_4480)),
    (0x801C_E898, PoolRef::Word(0x801E_447C)),
    (0x801C_E8A4, PoolRef::Word(0x801E_4478)),
    (0x801C_E8B0, PoolRef::Word(0x801E_4474)),
    (0x801C_E8BC, PoolRef::Word(0x801E_4470)),
    (0x801C_E8CC, PoolRef::Word(0x801E_446C)),
    (0x801C_E8D0, PoolRef::Word(0x801E_4468)),
    (0x801C_E8D8, PoolRef::Word(0x801E_4464)),
    (0x801C_E8F0, PoolRef::Word(0x801E_445C)),
    (0x801C_E904, PoolRef::Word(0x801E_4458)),
    (0x801C_E914, PoolRef::Word(0x801E_4454)),
    (0x801C_E91C, PoolRef::Word(0x801E_4450)),
    (0x801C_E928, PoolRef::Word(0x801E_444C)),
    (0x801C_E930, PoolRef::Word(0x801E_4448)),
    (0x801C_E948, PoolRef::Word(0x801E_4444)),
    (0x801C_E94C, PoolRef::Word(0x801E_4440)),
    (0x801C_E954, PoolRef::Word(0x801E_443C)),
    (0x801C_E95C, PoolRef::Word(0x801E_4438)),
    (0x801C_E96C, PoolRef::Word(0x801E_4434)),
    (0x801C_E978, PoolRef::Word(0x801E_4430)),
    (0x801C_E980, PoolRef::Word(0x801E_442C)),
    (
        0x801C_E9A0,
        PoolRef::Pair {
            lui: 0x801D_08C4,
            lo: 0x801D_08C8,
        },
    ),
    (
        0x801C_E9A4,
        PoolRef::Pair {
            lui: 0x801D_0980,
            lo: 0x801D_0984,
        },
    ),
    (
        0x801C_E9A8,
        PoolRef::Pair {
            lui: 0x801D_0A50,
            lo: 0x801D_0A54,
        },
    ),
    (
        0x801C_E9AC,
        PoolRef::Pair {
            lui: 0x801D_0AEC,
            lo: 0x801D_0AF0,
        },
    ),
    (
        0x801C_E9B0,
        PoolRef::Pair {
            lui: 0x801D_0BA4,
            lo: 0x801D_0BA8,
        },
    ),
    (
        0x801C_E9B4,
        PoolRef::Pair {
            lui: 0x801D_181C,
            lo: 0x801D_1820,
        },
    ),
    (
        0x801C_E9B8,
        PoolRef::Pair {
            lui: 0x801D_3DDC,
            lo: 0x801D_3DE0,
        },
    ),
    (
        0x801C_E9C4,
        PoolRef::Pair {
            lui: 0x801D_3E24,
            lo: 0x801D_3E28,
        },
    ),
    (
        0x801C_E9D0,
        PoolRef::Pair {
            lui: 0x801C_FD74,
            lo: 0x801C_FD78,
        },
    ),
    (
        0x801C_E9D8,
        PoolRef::Pair {
            lui: 0x801C_FE08,
            lo: 0x801C_FE0C,
        },
    ),
    (
        0x801C_E9E0,
        PoolRef::Pair {
            lui: 0x801C_FE7C,
            lo: 0x801C_FE80,
        },
    ),
    (
        0x801C_E9E8,
        PoolRef::Pair {
            lui: 0x801C_FEF0,
            lo: 0x801C_FEF4,
        },
    ),
    (
        0x801C_E9F4,
        PoolRef::Pair {
            lui: 0x801C_FF64,
            lo: 0x801C_FF68,
        },
    ),
    (
        0x801C_EA00,
        PoolRef::Pair {
            lui: 0x801D_000C,
            lo: 0x801D_0010,
        },
    ),
    (
        0x801C_EA08,
        PoolRef::Pair {
            lui: 0x801D_00A8,
            lo: 0x801D_00AC,
        },
    ),
    (
        0x801C_EA10,
        PoolRef::Pair {
            lui: 0x801D_0D9C,
            lo: 0x801D_0DA8,
        },
    ),
    (
        0x801C_EA18,
        PoolRef::Pair {
            lui: 0x801D_0E20,
            lo: 0x801D_0E24,
        },
    ),
    (
        0x801C_EA24,
        PoolRef::Pair {
            lui: 0x801D_0E94,
            lo: 0x801D_0E98,
        },
    ),
    (
        0x801C_EA30,
        PoolRef::Pair {
            lui: 0x801D_0FD8,
            lo: 0x801D_0FDC,
        },
    ),
    (
        0x801C_EA40,
        PoolRef::Pair {
            lui: 0x801D_CE2C,
            lo: 0x801D_CE30,
        },
    ),
    (
        0x801C_EA50,
        PoolRef::Pair {
            lui: 0x801D_CE94,
            lo: 0x801D_CE98,
        },
    ),
    (
        0x801C_EA60,
        PoolRef::Pair {
            lui: 0x801D_1C38,
            lo: 0x801D_1C3C,
        },
    ),
    (
        0x801C_EA74,
        PoolRef::Pair {
            lui: 0x801D_1C58,
            lo: 0x801D_1C5C,
        },
    ),
    (
        0x801C_EA84,
        PoolRef::Pair {
            lui: 0x801D_1C7C,
            lo: 0x801D_1C80,
        },
    ),
    (
        0x801C_EA8C,
        PoolRef::Pair {
            lui: 0x801D_1CFC,
            lo: 0x801D_1D00,
        },
    ),
    (
        0x801C_EA94,
        PoolRef::Pair {
            lui: 0x801D_1DB8,
            lo: 0x801D_1DBC,
        },
    ),
    (
        0x801C_EAB4,
        PoolRef::Pair {
            lui: 0x801D_1F1C,
            lo: 0x801D_1F20,
        },
    ),
    (
        0x801C_EAC8,
        PoolRef::Pair {
            lui: 0x801D_1F50,
            lo: 0x801D_1F54,
        },
    ),
    (
        0x801C_EAD8,
        PoolRef::Pair {
            lui: 0x801D_2280,
            lo: 0x801D_2284,
        },
    ),
    (
        0x801C_EAEC,
        PoolRef::Pair {
            lui: 0x801D_3024,
            lo: 0x801D_3028,
        },
    ),
    (
        0x801C_EAF8,
        PoolRef::Pair {
            lui: 0x801D_30FC,
            lo: 0x801D_3100,
        },
    ),
    (
        0x801C_EB04,
        PoolRef::Pair {
            lui: 0x801D_3124,
            lo: 0x801D_3128,
        },
    ),
    (
        0x801C_EB10,
        PoolRef::Pair {
            lui: 0x801D_3174,
            lo: 0x801D_3178,
        },
    ),
    (
        0x801C_EB18,
        PoolRef::Pair {
            lui: 0x801D_32AC,
            lo: 0x801D_32B0,
        },
    ),
    (
        0x801C_EB20,
        PoolRef::Pair {
            lui: 0x801D_4060,
            lo: 0x801D_4064,
        },
    ),
    (
        0x801C_EB40,
        PoolRef::Pair {
            lui: 0x801D_4384,
            lo: 0x801D_4388,
        },
    ),
    (
        0x801C_EB58,
        PoolRef::Pair {
            lui: 0x801D_4408,
            lo: 0x801D_440C,
        },
    ),
    (
        0x801C_EB64,
        PoolRef::Pair {
            lui: 0x801D_4590,
            lo: 0x801D_4594,
        },
    ),
    (
        0x801C_EB70,
        PoolRef::Pair {
            lui: 0x801D_4744,
            lo: 0x801D_4748,
        },
    ),
    (
        0x801C_EB94,
        PoolRef::Pair {
            lui: 0x801D_4874,
            lo: 0x801D_4878,
        },
    ),
    (
        0x801C_EB9C,
        PoolRef::Pair {
            lui: 0x801D_4984,
            lo: 0x801D_4988,
        },
    ),
    (
        0x801C_EBA4,
        PoolRef::Pair {
            lui: 0x801D_49F8,
            lo: 0x801D_49FC,
        },
    ),
    (
        0x801C_EBAC,
        PoolRef::Pair {
            lui: 0x801D_4DA4,
            lo: 0x801D_4DA8,
        },
    ),
    (
        0x801C_EBB8,
        PoolRef::Pair {
            lui: 0x801D_4EA4,
            lo: 0x801D_4EA8,
        },
    ),
    (
        0x801C_EBC8,
        PoolRef::Pair {
            lui: 0x801D_555C,
            lo: 0x801D_5560,
        },
    ),
    (
        0x801C_EBD0,
        PoolRef::Pair {
            lui: 0x801D_559C,
            lo: 0x801D_55A0,
        },
    ),
    (
        0x801C_EBD4,
        PoolRef::Pair {
            lui: 0x801D_55BC,
            lo: 0x801D_55C0,
        },
    ),
    (
        0x801C_EBDC,
        PoolRef::Pair {
            lui: 0x801D_55D8,
            lo: 0x801D_55DC,
        },
    ),
    (
        0x801C_EBF8,
        PoolRef::Pair {
            lui: 0x801D_5704,
            lo: 0x801D_5708,
        },
    ),
    (
        0x801C_EC08,
        PoolRef::Pair {
            lui: 0x801D_5848,
            lo: 0x801D_584C,
        },
    ),
    (
        0x801C_EC18,
        PoolRef::Pair {
            lui: 0x801D_5870,
            lo: 0x801D_5874,
        },
    ),
    (
        0x801C_EC28,
        PoolRef::Pair {
            lui: 0x801D_5898,
            lo: 0x801D_589C,
        },
    ),
    (
        0x801C_EC38,
        PoolRef::Pair {
            lui: 0x801D_597C,
            lo: 0x801D_5980,
        },
    ),
    (
        0x801C_EC54,
        PoolRef::Pair {
            lui: 0x801D_5BB8,
            lo: 0x801D_5BBC,
        },
    ),
    (
        0x801C_EC5C,
        PoolRef::Pair {
            lui: 0x801D_5C24,
            lo: 0x801D_5C28,
        },
    ),
    (
        0x801C_EC6C,
        PoolRef::Pair {
            lui: 0x801D_CFFC,
            lo: 0x801D_D000,
        },
    ),
    (
        0x801C_EC78,
        PoolRef::Pair {
            lui: 0x801D_61BC,
            lo: 0x801D_61C0,
        },
    ),
    (
        0x801C_EC94,
        PoolRef::Pair {
            lui: 0x801D_6210,
            lo: 0x801D_6214,
        },
    ),
    (
        0x801C_ECA8,
        PoolRef::Pair {
            lui: 0x801D_636C,
            lo: 0x801D_6370,
        },
    ),
    (
        0x801C_ECD4,
        PoolRef::Pair {
            lui: 0x801D_63BC,
            lo: 0x801D_63C0,
        },
    ),
    (
        0x801C_ECFC,
        PoolRef::Pair {
            lui: 0x801D_63DC,
            lo: 0x801D_63E0,
        },
    ),
    (
        0x801C_ED20,
        PoolRef::Pair {
            lui: 0x801D_63FC,
            lo: 0x801D_6400,
        },
    ),
    (
        0x801C_ED38,
        PoolRef::Pair {
            lui: 0x801D_641C,
            lo: 0x801D_6420,
        },
    ),
    (
        0x801C_ED58,
        PoolRef::Pair {
            lui: 0x801D_643C,
            lo: 0x801D_6440,
        },
    ),
    (
        0x801E_46E4,
        PoolRef::Pair {
            lui: 0x801D_CCC0,
            lo: 0x801D_CCC4,
        },
    ),
    (
        0x801E_4700,
        PoolRef::Pair {
            lui: 0x801D_CD68,
            lo: 0x801D_CD6C,
        },
    ),
];

/// The site [`POOL_REFS`] names for a retail VA.
pub fn pool_ref(retail_va: u32) -> Option<PoolRef> {
    legaia_overlay_images::string_refs::lookup(POOL_REFS, retail_va)
}

/// Where the string retail keeps at `retail_va` lives in `overlay`: the
/// address its [`PoolRef`] forms there, which is `retail_va` itself on an
/// unmodified image. A VA with no site, a site whose bytes no longer read as
/// one, or an address outside the image answers `retail_va`.
pub fn resolve_va(overlay: &[u8], retail_va: u32) -> u32 {
    legaia_overlay_images::string_refs::resolve(POOL_REFS, overlay, MENU_OVERLAY_BASE_VA, retail_va)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(img: &mut [u8], va: u32, w: u32) {
        let o = (va - MENU_OVERLAY_BASE_VA) as usize;
        img[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }

    #[test]
    fn the_table_is_sorted_and_unique() {
        assert!(POOL_REFS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn a_pair_and_a_word_follow_a_move() {
        let mut img = vec![0u8; 0x1_7000];
        // `lui a0,0x801d ; addiu a0,a0,-0x156c` -> 0x801CEA94, moved to 0x801E5010.
        put(&mut img, 0x801D_1DB8, 0x3C04_801E);
        put(&mut img, 0x801D_1DBC, 0x2484_5010);
        assert_eq!(resolve_va(&img, 0x801C_EA94), 0x801E_5010);
        put(&mut img, 0x801E_44B0, 0x801C_E900);
        assert_eq!(resolve_va(&img, 0x801C_E81C), 0x801C_E900);
        // The retail pair forms the retail VA.
        put(&mut img, 0x801D_1DB8, 0x3C04_801D);
        put(&mut img, 0x801D_1DBC, 0x2484_EA94);
        assert_eq!(resolve_va(&img, 0x801C_EA94), 0x801C_EA94);
    }

    #[test]
    fn an_unrecognised_site_keeps_the_retail_va() {
        let mut img = vec![0u8; 0x1_7000];
        // Not a `lui`: a code mod or another build.
        put(&mut img, 0x801D_1DB8, 0x0000_0000);
        assert_eq!(resolve_va(&img, 0x801C_EA94), 0x801C_EA94);
        // A word pointing outside the image.
        put(&mut img, 0x801E_44B0, 0x8020_0000);
        assert_eq!(resolve_va(&img, 0x801C_E81C), 0x801C_E81C);
        // A VA with no site, and an image too short to hold the site.
        assert_eq!(resolve_va(&img, 0x801C_E820), 0x801C_E820);
        assert_eq!(resolve_va(&[], 0x801C_EA94), 0x801C_EA94);
    }
}
