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
//! | `0x80084464` | `0x324` | dance hall best score |
//! | `0x80084468` | `0x328` | dance free play's song counter |
//! | `0x8008446C` | `0x32C` | one-time prize purchased bitmask |
//! | `0x800845A4` | `0x464` | casino coin bank ([`crate::card::RETAIL_COINS_OFFSET`]) |
//! | `0x800845B4` | `0x474` | Point Card bank |
//!
//! An engine `LGSF` file carries the same eleven words in the optional `LGX7`
//! block (see `crate::ext`), emitted only when one of them is non-zero so a
//! save without minigame progress stays byte-identical to an older file. The
//! dance best and the free-play song counter are the tenth and eleventh
//! words, appended after the block first shipped with nine: a shorter body
//! reads each missing word as `0`.

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
/// SC offset of the dance hall's best score (`0x80084464`). The dance tick
/// `FUN_801CF470` raises it to the human's score `DAT_801D53CC` whenever that
/// is higher (unsigned `sltu`, `0x801CF584..0x801CF5A4`) in every state past
/// `0` outside the how-to, and free play draws it on its personal-best panel
/// (`0x801CF5DC`).
pub const RETAIL_DANCE_BEST_OFFSET: usize = 0x324;
/// SC offset of dance free play's song counter (`0x80084468`): free play
/// streams song `[0x81D, 0x80C, 0x812][counter]` (less `0x3F2`), then bumps
/// the counter and wraps it to `0` at `3` (`lw`/`sw 0x328(0x80084140)`,
/// `0x801CFB48..0x801CFBA0` in PROT 0980), so the rotation carries across
/// visits and saves.
pub const RETAIL_DANCE_FREE_PLAY_SONG_OFFSET: usize = 0x328;
/// SC offset of the one-time prize bitmask (`0x8008446C`).
pub const RETAIL_FISHING_PRIZES_OFFSET: usize = 0x32C;
/// SC offset of the Point Card bank (`0x800845B4`).
pub const RETAIL_POINT_CARD_OFFSET: usize = 0x474;

/// Ceiling of the casino coin bank (`9_999_999`): every retail credit to
/// [`MinigameSave::casino_coins`] - the coin exchange, a slot payout, a
/// Muscle Dome contest tally - saturates here.
pub const COIN_BANK_MAX: i32 = 0x0098_967F;

/// Bytes of the `LGX7` body: eleven little-endian words.
pub const MINIGAME_SAVE_BODY_LEN: usize = 11 * 4;
/// Bytes of the shortest `LGX7` body a reader accepts: the nine words the
/// block carried before the dance best.
pub const MINIGAME_SAVE_BODY_MIN_LEN: usize = 9 * 4;

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
    /// Dance hall best score (`0x80084464`, [`RETAIL_DANCE_BEST_OFFSET`]).
    pub dance_best: u32,
    /// Dance free play's song counter (`0x80084468`,
    /// [`RETAIL_DANCE_FREE_PLAY_SONG_OFFSET`]).
    pub dance_free_play_song: u32,
}

impl MinigameSave {
    /// The eleven words in `LGX7` / table order, each with its SC offset.
    fn fields(&self) -> [(usize, u32); 11] {
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
            (RETAIL_DANCE_BEST_OFFSET, self.dance_best),
            (
                RETAIL_DANCE_FREE_PLAY_SONG_OFFSET,
                self.dance_free_play_song,
            ),
        ]
    }

    fn from_words(w: [u32; 11]) -> Self {
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
            dance_best: w[9],
            dance_free_play_song: w[10],
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

    /// Decode an `LGX7` body. A nine- or ten-word body (written before the
    /// dance best or the song counter) reads each missing word as `0`; bytes
    /// past the eleven words are ignored, so a later writer can grow the
    /// block.
    pub fn parse_body(buf: &[u8]) -> Result<Self> {
        if buf.len() < MINIGAME_SAVE_BODY_MIN_LEN {
            bail!("LGX7: block too short ({} bytes)", buf.len());
        }
        let mut w = [0u32; 11];
        for (i, word) in w.iter_mut().enumerate() {
            if let Some(b) = buf.get(i * 4..i * 4 + 4) {
                *word = u32::from_le_bytes(b.try_into().unwrap());
            }
        }
        Ok(Self::from_words(w))
    }

    /// Read the eleven words off a retail SC block. `None` if it is too small.
    pub fn from_retail_sc_block(sc_block: &[u8]) -> Option<Self> {
        let mut w = [0u32; 11];
        for (i, (off, _)) in Self::default().fields().iter().enumerate() {
            let b = sc_block.get(*off..*off + 4)?;
            w[i] = u32::from_le_bytes(b.try_into().unwrap());
        }
        Some(Self::from_words(w))
    }

    /// Write the eleven words into a retail SC block in place and restamp the
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
            dance_best: 568,
            dance_free_play_song: 2,
        }
    }

    #[test]
    fn body_round_trips() {
        let m = sample();
        assert_eq!(m.body().len(), MINIGAME_SAVE_BODY_LEN);
        assert_eq!(MinigameSave::parse_body(&m.body()).unwrap(), m);
        assert!(MinigameSave::parse_body(&[0; 8]).is_err());
    }

    /// A body written before the dance best (nine words) still parses, with
    /// the best at `0`.
    #[test]
    fn a_nine_word_body_reads_no_dance_best() {
        let m = sample();
        let old = &m.body()[..MINIGAME_SAVE_BODY_MIN_LEN];
        let parsed = MinigameSave::parse_body(old).unwrap();
        assert_eq!(parsed.dance_best, 0);
        assert_eq!(
            parsed,
            MinigameSave {
                dance_best: 0,
                dance_free_play_song: 0,
                ..m
            }
        );
        // A ten-word body (written before the song counter) keeps the best.
        let ten = MinigameSave::parse_body(&m.body()[..40]).unwrap();
        assert_eq!(
            ten,
            MinigameSave {
                dance_free_play_song: 0,
                ..m
            }
        );
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
        assert_eq!(at(RETAIL_DANCE_BEST_OFFSET), 568);
        assert_eq!(at(RETAIL_DANCE_FREE_PLAY_SONG_OFFSET), 2);
        assert!(crate::card::sc_block_checksum_valid(&block));
    }
}
