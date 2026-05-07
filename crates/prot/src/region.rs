/// Retail region of a Legend of Legaia disc image.
///
/// The TOC formula and CDNAME layout are identical across all regions. Region
/// is stored as metadata on [`crate::archive::Archive`] / `ProtIndex` so that
/// region-specific addresses (debug flags, RAM map shifts) can be derived from
/// one place. See `docs/reference/builds.md` for the per-region address table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Region {
    /// NA retail — `SCUS-94254` (1999-01-29). Anchor build for this project.
    #[default]
    Na,
    /// JP retail — `SCPS-10059` (1998-09-09) and its reissues.
    Jp,
    /// EU retail — `SCES-01752` (EN) and the FR/DE/IT/ES localisations.
    Eu,
}

impl Region {
    /// Infer region from a PSX product code prefix (e.g. `"SCUS-94254"`).
    /// Returns `None` for unknown prefixes (demo discs, prototypes, etc.).
    pub fn from_product_code(code: &str) -> Option<Self> {
        if code.starts_with("SCUS") || code.starts_with("SLUS") {
            Some(Self::Na)
        } else if code.starts_with("SCPS") || code.starts_with("SLPS") {
            Some(Self::Jp)
        } else if code.starts_with("SCES") || code.starts_with("SLED") || code.starts_with("SLES") {
            Some(Self::Eu)
        } else {
            None
        }
    }

    /// Short display string (`"NA"`, `"JP"`, `"EU"`).
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Na => "NA",
            Self::Jp => "JP",
            Self::Eu => "EU",
        }
    }

    /// Byte offset added to NA debug-flag addresses to reach this region's
    /// equivalent. Based on the 0x1B90-byte shift between NA and JP documented
    /// in `docs/reference/builds.md`. EU binaries follow the NA layout.
    pub fn debug_flag_shift(self) -> u32 {
        match self {
            Self::Na | Self::Eu => 0,
            Self::Jp => 0x1B90,
        }
    }

    /// Translate an NA-anchor address to this region.
    pub fn translate_addr(self, na_addr: u32) -> u32 {
        na_addr.wrapping_add(self.debug_flag_shift())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_product_code_roundtrip() {
        assert_eq!(Region::from_product_code("SCUS-94254"), Some(Region::Na));
        assert_eq!(Region::from_product_code("SCPS-10059"), Some(Region::Jp));
        assert_eq!(Region::from_product_code("SCES-01752"), Some(Region::Eu));
        assert_eq!(Region::from_product_code("PAPX-90040"), None);
    }

    #[test]
    fn translate_addr_na_is_identity() {
        let addr = 0x8007_B98F_u32;
        assert_eq!(Region::Na.translate_addr(addr), addr);
        assert_eq!(Region::Eu.translate_addr(addr), addr);
        assert_eq!(Region::Jp.translate_addr(addr), addr + 0x1B90);
    }
}
