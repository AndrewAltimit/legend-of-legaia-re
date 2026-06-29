//! Baka Fighter **per-opponent table** (overlay VA `0x801D76BC`).
//!
//! Each of the minigame's opponents has a `0x6c`-byte record in a table the
//! match code indexes by the ladder/opponent id. Two fields are pinned:
//!
//! - **`+0x00` gold reward** - the match-win payout. On a player win
//!   [`FUN_801d0fe4`] does `DAT_801dbee8 = *(u32*)(opp*0x6c + 0x801d76bc)`, and
//!   the end-of-match tally [`FUN_801d239c`] drains `DAT_801dbee8` into the
//!   party gold bank `_DAT_80084440`. So this byte field *is* the gold prize for
//!   beating that opponent.
//! - **`+0x2c` AI move-pattern string** - a NUL-terminated run of attack-type
//!   symbols (`1`/`2`/`3` = the three rock-paper-scissors attacks). The opponent
//!   move picker `FUN_801d487c` reads `DAT_801d76e8[i + opp*0x6c]`
//!   (`0x801d76e8 = 0x801d76bc + 0x2c`), scanning to the NUL to get the cycle
//!   length, then plays `pattern[cursor] - 1` as the next attack type. So a CPU
//!   fighter follows a fixed, readable attack loop.
//!
//! `+0x24` (`i16`) is a per-opponent actor-anchor value (read `+ 200` as a spawn
//! Y in `FUN_801d0fe4`); the other record fields are not yet labelled.
//!
//! ## Extent - 17 opponents
//!
//! The table holds [`OPPONENT_COUNT`] = 17 records (the same `0x11` count the
//! action-table dump loop walks, see `docs/subsystems/minigame-baka-fighter.md`);
//! record 17 is past the table (its `+0x2c` is not a valid `1/2/3` pattern).
//!
//! ## Match length - best of 3
//!
//! Independent of this table, the match is **first to [`ROUND_WIN_TARGET`] = 2
//! round wins** (best of 3): `FUN_801cf00c` inits `DAT_801dbed0 = 2`, and the
//! match-over check in `FUN_801d0fe4` ends the match when a fighter's round-win
//! count (`DAT_801dbff0` / `DAT_801dc098`) equals `DAT_801dbed0`.
//!
//! ## Provenance - baked overlay data, pinned on disc
//!
//! The table is static `.rodata` in the Baka Fighter overlay (PROT entry
//! **0976**, base [`BAKA_OVERLAY_BASE_VA`]) at file offset
//! [`OPPONENT_TABLE_FILE_OFFSET`]; reproducible from the user's `PROT.DAT`
//! (disc-gated `baka_opponents_real`). No Sony bytes are committed - the gold
//! values + AI patterns decode from the user's disc.

/// CDNAME / PROT index of the Baka Fighter overlay (`data\OTHER5`).
pub const BAKA_OVERLAY_PROT_INDEX: usize = 976;

/// Load base of the Baka Fighter overlay (the shared slot-A minigame base).
pub const BAKA_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Runtime VA of the per-opponent table head (`DAT_801d76bc`).
pub const OPPONENT_TABLE_VA: u32 = 0x801D_76BC;

/// File offset of the opponent table within the as-loaded overlay image.
pub const OPPONENT_TABLE_FILE_OFFSET: usize = (OPPONENT_TABLE_VA - BAKA_OVERLAY_BASE_VA) as usize;

/// Per-opponent record stride (`opp * 0x6c` index math).
pub const OPPONENT_RECORD_STRIDE: usize = 0x6C;

/// Byte offset of the gold-reward field within a record.
pub const RECORD_GOLD_OFFSET: usize = 0x00;

/// Byte offset of the AI move-pattern string within a record (`DAT_801d76e8`).
pub const RECORD_AI_PATTERN_OFFSET: usize = 0x2C;

/// Number of opponents (the `0x11` records the action-table loop walks).
pub const OPPONENT_COUNT: usize = 17;

/// Round wins needed to win a match (`DAT_801dbed0`): first to 2 = best of 3.
pub const ROUND_WIN_TARGET: u32 = 2;

/// One decoded Baka Fighter opponent record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakaOpponent {
    /// Opponent id = index into the table.
    pub index: usize,
    /// `+0x00` - gold credited to the party on beating this opponent.
    pub gold_reward: u32,
    /// `+0x2c` - the CPU attack-type loop (symbols `1`/`2`/`3`), NUL-terminated.
    pub ai_pattern: Vec<u8>,
}

impl BakaOpponent {
    /// The next attack type (`0`/`1`/`2`) the CPU plays at a cycle cursor, the
    /// `pattern[cursor % len] - 1` the move picker computes.
    pub fn attack_at(&self, cursor: usize) -> Option<u8> {
        if self.ai_pattern.is_empty() {
            return None;
        }
        self.ai_pattern
            .get(cursor % self.ai_pattern.len())
            .map(|&s| s - 1)
    }
}

/// Parse the [`OPPONENT_COUNT`] opponent records out of the as-loaded Baka
/// Fighter overlay image (PROT entry [`BAKA_OVERLAY_PROT_INDEX`]).
pub fn parse(overlay: &[u8]) -> Option<Vec<BakaOpponent>> {
    parse_at(overlay, OPPONENT_TABLE_FILE_OFFSET, OPPONENT_COUNT)
}

/// Parse `count` records starting at file offset `off`.
pub fn parse_at(overlay: &[u8], off: usize, count: usize) -> Option<Vec<BakaOpponent>> {
    if overlay.len() < off + count * OPPONENT_RECORD_STRIDE {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let b = off + i * OPPONENT_RECORD_STRIDE;
        let gold = u32::from_le_bytes([overlay[b], overlay[b + 1], overlay[b + 2], overlay[b + 3]]);
        // AI pattern: NUL-terminated, bounded by the record tail.
        let pat_start = b + RECORD_AI_PATTERN_OFFSET;
        let pat_end = b + OPPONENT_RECORD_STRIDE;
        let mut ai_pattern = Vec::new();
        for &byte in &overlay[pat_start..pat_end] {
            if byte == 0 {
                break;
            }
            ai_pattern.push(byte);
        }
        out.push(BakaOpponent {
            index: i,
            gold_reward: gold,
            ai_pattern,
        });
    }
    Some(out)
}

/// Whether a decoded AI pattern is a valid non-empty attack loop (every symbol
/// is one of the three attack types `1`/`2`/`3`). A real opponent always has
/// one; the field bounds the table.
pub fn is_valid_pattern(pattern: &[u8]) -> bool {
    !pattern.is_empty() && pattern.iter().all(|&s| (1..=3).contains(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offset_and_consts() {
        assert_eq!(OPPONENT_TABLE_FILE_OFFSET, 0x8EA4);
        assert_eq!(OPPONENT_RECORD_STRIDE, 0x6C);
        assert_eq!(OPPONENT_COUNT, 17);
        assert_eq!(ROUND_WIN_TARGET, 2);
    }

    #[test]
    fn parse_gold_and_pattern() {
        let off = 0x10;
        let mut buf = vec![0u8; off + 2 * OPPONENT_RECORD_STRIDE];
        // record 1: gold 25, pattern [1,2,3] then NUL.
        let b = off + OPPONENT_RECORD_STRIDE;
        buf[b..b + 4].copy_from_slice(&25u32.to_le_bytes());
        buf[b + RECORD_AI_PATTERN_OFFSET..b + RECORD_AI_PATTERN_OFFSET + 3]
            .copy_from_slice(&[1, 2, 3]);
        let recs = parse_at(&buf, off, 2).expect("parses");
        assert_eq!(recs[1].gold_reward, 25);
        assert_eq!(recs[1].ai_pattern, vec![1, 2, 3]);
        assert!(is_valid_pattern(&recs[1].ai_pattern));
        // cursor wraps and subtracts 1 → attack type.
        assert_eq!(recs[1].attack_at(0), Some(0));
        assert_eq!(recs[1].attack_at(3), Some(0)); // wraps
        assert_eq!(recs[1].attack_at(2), Some(2));
        // record 0 empty pattern → invalid / no attack.
        assert!(!is_valid_pattern(&recs[0].ai_pattern));
        assert_eq!(recs[0].attack_at(0), None);
    }

    #[test]
    fn too_short_is_none() {
        assert!(parse_at(&[0u8; 4], 0, 1).is_none());
    }
}
