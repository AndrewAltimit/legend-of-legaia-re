//! The minigame **purses and records** a save carries: the casino coin bank,
//! the Point Card bank and the fishing point record.
//!
//! All of them live inside retail's `0x1A18`-byte live-state window at
//! `0x80084140` (the bytes the save composer copies to the front of the SC
//! block and the loader copies back), so retail persists every one of them in
//! the block at the same linear offset `VA - 0x80084140`
//! (`docs/subsystems/save-screen.md`):
//!
//! | VA | SC offset | field |
//! |---|---|---|
//! | `0x8008444C` | `0x30C` | fishing-point pool (`s32`, capped `999999`) |
//! | `0x80084450` | `0x310` | equipped lure row |
//! | `0x80084454` | `0x314` | rod index |
//! | `0x80084458` | `0x318` | best single-catch award |
//! | `0x8008445C` | `0x31C` | species of the best catch |
//! | `0x80084460` | `0x320` | lifetime cast counter |
//! | `0x8008446C` | `0x32C` | one-time prize purchased bitmask |
//! | `0x800845A4` | `0x464` | casino coin bank ([`crate::card::RETAIL_COINS_OFFSET`]) |
//! | `0x800845B4` | `0x474` | Point Card bank |
//!
//! An engine `LGSF` file carries the same nine words in the optional `LGX7`
//! block (see `crate::ext`), emitted only when one of them is non-zero so a
//! save without minigame progress stays byte-identical to an older file.

use anyhow::{Result, bail};

/// SC offset of the fishing-point pool (`0x8008444C`).
pub const RETAIL_FISHING_POINTS_OFFSET: usize = 0x30C;
/// SC offset of the equipped lure row (`0x80084450`).
pub const RETAIL_FISHING_LURE_OFFSET: usize = 0x310;
/// SC offset of the rod index (`0x80084454`).
pub const RETAIL_FISHING_ROD_OFFSET: usize = 0x314;
/// SC offset of the best single-catch award (`0x80084458`).
pub const RETAIL_FISHING_BEST_POINTS_OFFSET: usize = 0x318;
/// SC offset of the best catch's species (`0x8008445C`).
pub const RETAIL_FISHING_BEST_FISH_OFFSET: usize = 0x31C;
/// SC offset of the lifetime cast counter (`0x80084460`).
pub const RETAIL_FISHING_CASTS_OFFSET: usize = 0x320;
/// SC offset of the one-time prize bitmask (`0x8008446C`).
pub const RETAIL_FISHING_PRIZES_OFFSET: usize = 0x32C;
/// SC offset of the Point Card bank (`0x800845B4`).
pub const RETAIL_POINT_CARD_OFFSET: usize = 0x474;

/// Bytes of the `LGX7` body: nine little-endian words.
pub const MINIGAME_SAVE_BODY_LEN: usize = 9 * 4;

/// The minigame purses and records one save carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MinigameSave {
    /// Casino coin bank (`0x800845A4`).
    pub casino_coins: u32,
    /// Point Card bank (`0x800845B4`).
    pub point_card: i32,
    /// Fishing-point pool (`0x8008444C`).
    pub fishing_points: i32,
    /// Equipped lure row (`0x80084450`).
    pub fishing_lure: u32,
    /// Rod index (`0x80084454`).
    pub fishing_rod: u32,
    /// Best single-catch award (`0x80084458`).
    pub fishing_best_points: i32,
    /// Species of the best catch (`0x8008445C`).
    pub fishing_best_fish: u32,
    /// Lifetime cast counter (`0x80084460`).
    pub fishing_casts: i32,
    /// One-time prize purchased bitmask (`0x8008446C`).
    pub fishing_prizes_purchased: u32,
}

impl MinigameSave {
    /// The nine words in `LGX7` / table order, each with its SC offset.
    fn fields(&self) -> [(usize, u32); 9] {
        [
            (RETAIL_FISHING_POINTS_OFFSET, self.fishing_points as u32),
            (RETAIL_FISHING_LURE_OFFSET, self.fishing_lure),
            (RETAIL_FISHING_ROD_OFFSET, self.fishing_rod),
            (
                RETAIL_FISHING_BEST_POINTS_OFFSET,
                self.fishing_best_points as u32,
            ),
            (RETAIL_FISHING_BEST_FISH_OFFSET, self.fishing_best_fish),
            (RETAIL_FISHING_CASTS_OFFSET, self.fishing_casts as u32),
            (RETAIL_FISHING_PRIZES_OFFSET, self.fishing_prizes_purchased),
            (crate::card::RETAIL_COINS_OFFSET, self.casino_coins),
            (RETAIL_POINT_CARD_OFFSET, self.point_card as u32),
        ]
    }

    fn from_words(w: [u32; 9]) -> Self {
        Self {
            fishing_points: w[0] as i32,
            fishing_lure: w[1],
            fishing_rod: w[2],
            fishing_best_points: w[3] as i32,
            fishing_best_fish: w[4],
            fishing_casts: w[5] as i32,
            fishing_prizes_purchased: w[6],
            casino_coins: w[7],
            point_card: w[8] as i32,
        }
    }

    /// `true` when every field is zero - the new-game state, and what a file
    /// written before the `LGX7` block reads as.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The `LGX7` body.
    pub fn body(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MINIGAME_SAVE_BODY_LEN);
        for (_, w) in self.fields() {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }

    /// Decode an `LGX7` body. Bytes past the nine words are ignored, so a
    /// later writer can grow the block.
    pub fn parse_body(buf: &[u8]) -> Result<Self> {
        if buf.len() < MINIGAME_SAVE_BODY_LEN {
            bail!("LGX7: block too short ({} bytes)", buf.len());
        }
        let mut w = [0u32; 9];
        for (i, word) in w.iter_mut().enumerate() {
            *word = u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Ok(Self::from_words(w))
    }

    /// Read the nine words off a retail SC block. `None` if it is too small.
    pub fn from_retail_sc_block(sc_block: &[u8]) -> Option<Self> {
        let mut w = [0u32; 9];
        for (i, (off, _)) in Self::default().fields().iter().enumerate() {
            let b = sc_block.get(*off..*off + 4)?;
            w[i] = u32::from_le_bytes(b.try_into().unwrap());
        }
        Some(Self::from_words(w))
    }

    /// Write the nine words into a retail SC block in place and restamp the
    /// block checksum.
    pub fn write_into_retail_sc_block(&self, sc_block: &mut [u8]) -> Result<()> {
        let end = RETAIL_POINT_CARD_OFFSET + 4;
        if sc_block.len() < end {
            bail!("sc_block too small for the minigame fields (need >= {end})");
        }
        for (off, w) in self.fields() {
            sc_block[off..off + 4].copy_from_slice(&w.to_le_bytes());
        }
        crate::card::restamp_sc_block_checksum(sc_block);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MinigameSave {
        MinigameSave {
            casino_coins: 12_345,
            point_card: 678,
            fishing_points: 9_000,
            fishing_lure: 2,
            fishing_rod: 1,
            fishing_best_points: 450,
            fishing_best_fish: 7,
            fishing_casts: 31,
            fishing_prizes_purchased: 0x0102,
        }
    }

    #[test]
    fn body_round_trips() {
        let m = sample();
        assert_eq!(m.body().len(), MINIGAME_SAVE_BODY_LEN);
        assert_eq!(MinigameSave::parse_body(&m.body()).unwrap(), m);
        assert!(MinigameSave::parse_body(&[0; 8]).is_err());
    }

    #[test]
    fn the_retail_block_carries_each_word_at_its_window_offset() {
        let mut block = vec![0u8; crate::card::BLOCK_SIZE];
        sample().write_into_retail_sc_block(&mut block).unwrap();
        assert_eq!(MinigameSave::from_retail_sc_block(&block), Some(sample()));
        assert_eq!(crate::card::read_retail_coins(&block), Some(12_345));
        let at = |off: usize| u32::from_le_bytes(block[off..off + 4].try_into().unwrap());
        assert_eq!(at(RETAIL_FISHING_POINTS_OFFSET), 9_000);
        assert_eq!(at(RETAIL_POINT_CARD_OFFSET), 678);
        assert!(crate::card::sc_block_checksum_valid(&block));
    }
}
