//! The MAN fog-region record: one `0xB`-byte box per fog region of a scene.

/// One fog-region record's stride in the MAN section-4 table (`addiu v1,v1,0xb`
/// at `0x801D63B4`).
pub const FOG_REGION_STRIDE: usize = 0xB;

/// One fog-region record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FogRegion {
    /// `+0` - the spawner keeps a match only when this is non-zero.
    pub enabled: bool,
    /// `+1` / `+2` - open lower bounds (`rec < tile`).
    pub x0: u8,
    pub z0: u8,
    /// `+3` / `+4` - open upper bounds (`tile < rec`).
    pub x1: u8,
    pub z1: u8,
    /// `+5` - angle base, in sixteenths of the 4096-step circle.
    pub angle_base: u8,
    /// `+6` - angle spread, same units; the random term is `rand & 0xFFF`
    /// scaled by it.
    pub angle_spread: u8,
    /// `+7` - drift speed; `sin/cos * speed >> 14` per axis.
    pub speed: u8,
    /// `+8` - read by nothing traced here.
    pub byte_8: u8,
    /// `+9..+10` - the story-flag index the nibble-C sub-1 reset tests.
    pub flag_index: u16,
}

impl FogRegion {
    /// Decode one `0xB`-byte record.
    pub fn parse(rec: &[u8]) -> Option<Self> {
        if rec.len() < FOG_REGION_STRIDE {
            return None;
        }
        Some(Self {
            enabled: rec[0] != 0,
            x0: rec[1],
            z0: rec[2],
            x1: rec[3],
            z1: rec[4],
            angle_base: rec[5],
            angle_spread: rec[6],
            speed: rec[7],
            byte_8: rec[8],
            flag_index: u16::from_le_bytes([rec[9], rec[10]]),
        })
    }

    /// The spawner's box test - strict on all four sides
    /// (`0x801D6340..0x801D6388`: `slt rec,tile` then `slt tile,rec`).
    pub fn contains(&self, tile_x: i32, tile_z: i32) -> bool {
        i32::from(self.x0) < tile_x
            && tile_x < i32::from(self.x1)
            && i32::from(self.z0) < tile_z
            && tile_z < i32::from(self.z1)
    }
}

/// Parse a MAN section-4 payload (the bytes after its `u24` length:
/// `[count][records]`) into fog regions.
pub fn parse_fog_regions(section_body: &[u8]) -> Vec<FogRegion> {
    let Some(&count) = section_body.first() else {
        return Vec::new();
    };
    (0..usize::from(count))
        .filter_map(|i| {
            let at = 1 + i * FOG_REGION_STRIDE;
            section_body
                .get(at..at + FOG_REGION_STRIDE)
                .and_then(FogRegion::parse)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_table_parses_count_prefixed_records() {
        let mut body = vec![2u8];
        body.extend_from_slice(&[1, 2, 3, 40, 50, 0x10, 0x20, 0x30, 0, 0x34, 0x12]);
        body.extend_from_slice(&[0, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0]);
        let regs = parse_fog_regions(&body);
        assert_eq!(regs.len(), 2);
        assert!(regs[0].enabled);
        assert_eq!(
            (regs[0].x0, regs[0].z0, regs[0].x1, regs[0].z1),
            (2, 3, 40, 50)
        );
        assert_eq!(regs[0].flag_index, 0x1234);
        assert!(!regs[1].enabled);
        // Strict on every side.
        assert!(regs[0].contains(3, 4));
        assert!(!regs[0].contains(2, 4));
        assert!(!regs[0].contains(40, 4));
    }
}
