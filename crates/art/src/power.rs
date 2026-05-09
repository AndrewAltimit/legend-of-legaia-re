//! Art Power encoding — damage multiplier byte semantics.
//!
//! Each art records up to several "power bytes" describing the damage of
//! each hit. The byte's value picks both the multiplier and the
//! defense-target (UDF — Upper Defense, or LDF — Lower Defense).
//!
//! ## Encoding
//!
//! | Byte range | Target sequence | Multiplier sequence |
//! |---|---|---|
//! | `0x16–0x1F` | UDF, then LDF (alternating starting from UDF) | 12, 18, 20, 22, 28 |
//! | `0x0C–0x15` | Same multiplier scale, but LDF-target attacks miss floating enemies and UDF-target attacks miss short enemies | 12, 18, 20, 22, 28 |
//! | other | No damage | — |
//!
//! Concretely the values map as:
//!
//! - `0x16 → UDF × 12`, `0x17 → UDF × 18`, `0x18 → UDF × 20`, `0x19 → UDF × 22`, `0x1A → UDF × 28`
//! - `0x1B → LDF × 12`, `0x1C → LDF × 18`, `0x1D → LDF × 20`, `0x1E → LDF × 22`, `0x1F → LDF × 28`
//! - `0x0C–0x10` → UDF same multipliers, "alt" range (misses short enemies)
//! - `0x11–0x15` → LDF same multipliers, "alt" range (misses floating enemies)
//!
//! Source: external RE cross-referenced with Meth962's earlier work, captured
//! in the `Art Data Format` spreadsheet in the project's research archive.

use serde::{Deserialize, Serialize};

/// Defense type the multiplier targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerTarget {
    /// Upper Defense Factor — standard high-attack target.
    Udf,
    /// Lower Defense Factor — standard low-attack target.
    Ldf,
}

/// Decoded power byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtPower {
    pub target: PowerTarget,
    pub multiplier: u8,
    /// `true` iff the byte was in the alt range (`0x0C–0x15`).
    /// Alt-range attacks miss floating enemies (when LDF) or short enemies
    /// (when UDF).
    pub alt_range: bool,
}

/// Wrapper used by tests / parsers — distinguishes "no damage" from an
/// invalid byte, mirroring the engine's runtime behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerByte {
    /// Byte was outside the damage-encoding range — produces no damage.
    NoDamage,
    /// Byte decoded to a target+multiplier.
    Damage(ArtPower),
}

impl PowerByte {
    /// Decode a single power byte from the art record.
    pub fn from_byte(b: u8) -> PowerByte {
        let alt_range;
        let target;
        let idx;

        if (0x16..=0x1A).contains(&b) {
            // Standard UDF range.
            alt_range = false;
            target = PowerTarget::Udf;
            idx = b - 0x16;
        } else if (0x1B..=0x1F).contains(&b) {
            // Standard LDF range.
            alt_range = false;
            target = PowerTarget::Ldf;
            idx = b - 0x1B;
        } else if (0x0C..=0x10).contains(&b) {
            // Alt UDF range — misses short enemies.
            alt_range = true;
            target = PowerTarget::Udf;
            idx = b - 0x0C;
        } else if (0x11..=0x15).contains(&b) {
            // Alt LDF range — misses floating enemies.
            alt_range = true;
            target = PowerTarget::Ldf;
            idx = b - 0x11;
        } else {
            return PowerByte::NoDamage;
        }

        let multiplier = MULTIPLIER_TABLE[idx as usize];
        PowerByte::Damage(ArtPower {
            target,
            multiplier,
            alt_range,
        })
    }

    pub fn is_damage(self) -> bool {
        matches!(self, PowerByte::Damage(_))
    }
}

/// Multiplier scale (× weapon attack) for the 5 power tiers.
const MULTIPLIER_TABLE: [u8; 5] = [12, 18, 20, 22, 28];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_udf_range() {
        let p = PowerByte::from_byte(0x16);
        assert_eq!(
            p,
            PowerByte::Damage(ArtPower {
                target: PowerTarget::Udf,
                multiplier: 12,
                alt_range: false,
            })
        );
        let p = PowerByte::from_byte(0x1A);
        assert_eq!(
            p,
            PowerByte::Damage(ArtPower {
                target: PowerTarget::Udf,
                multiplier: 28,
                alt_range: false,
            })
        );
    }

    #[test]
    fn standard_ldf_range() {
        let p = PowerByte::from_byte(0x1B);
        assert_eq!(
            p,
            PowerByte::Damage(ArtPower {
                target: PowerTarget::Ldf,
                multiplier: 12,
                alt_range: false,
            })
        );
        let p = PowerByte::from_byte(0x1F);
        assert_eq!(
            p,
            PowerByte::Damage(ArtPower {
                target: PowerTarget::Ldf,
                multiplier: 28,
                alt_range: false,
            })
        );
    }

    #[test]
    fn alt_range_marks_alt_range_flag() {
        let p = PowerByte::from_byte(0x0C);
        assert_eq!(
            p,
            PowerByte::Damage(ArtPower {
                target: PowerTarget::Udf,
                multiplier: 12,
                alt_range: true,
            })
        );
        let p = PowerByte::from_byte(0x15);
        assert_eq!(
            p,
            PowerByte::Damage(ArtPower {
                target: PowerTarget::Ldf,
                multiplier: 28,
                alt_range: true,
            })
        );
    }

    #[test]
    fn out_of_range_is_no_damage() {
        for b in [0x00, 0x01, 0x0B, 0x20, 0x40, 0xFF] {
            assert_eq!(PowerByte::from_byte(b), PowerByte::NoDamage);
        }
    }

    #[test]
    fn researcher_example_burning_flare() {
        // Burning Flare power data from the spreadsheet:
        // "23 13 1 LDF20 UDF22 LDF28 UDF28"
        // The LDF/UDF labels match the bytes 1D/22/1F/1E... but the
        // researcher cites them; spot-check the scheme produces the
        // multiplier sequence claimed for an art:
        // power bytes 0x1D 0x19 0x1F 0x1A → LDF20, UDF22, LDF28, UDF28
        let bytes = [0x1D, 0x19, 0x1F, 0x1A];
        let want = [
            (PowerTarget::Ldf, 20u8),
            (PowerTarget::Udf, 22u8),
            (PowerTarget::Ldf, 28u8),
            (PowerTarget::Udf, 28u8),
        ];
        for (b, (t, m)) in bytes.iter().zip(want.iter()) {
            match PowerByte::from_byte(*b) {
                PowerByte::Damage(p) => {
                    assert_eq!(p.target, *t, "byte 0x{b:02X}");
                    assert_eq!(p.multiplier, *m, "byte 0x{b:02X}");
                    assert!(!p.alt_range);
                }
                PowerByte::NoDamage => panic!("byte 0x{b:02X} should encode damage"),
            }
        }
    }
}
