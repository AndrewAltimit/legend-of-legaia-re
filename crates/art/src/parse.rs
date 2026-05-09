//! Best-effort raw-bytes parser for [`ArtRecord`].
//!
//! ## Provenance
//!
//! The on-disc art records are not pinned at a fixed stride: the
//! researcher documented field positions but each record's command
//! sequence and power-data section both vary in length. We parse what we
//! can confidently and surface unknowns rather than fabricating a strict
//! schema.
//!
//! Concretely, this module assumes each record begins with:
//!
//! ```text
//!   u8       command sequence terminator-delimited (0 ends the list)
//!   u8       action constant (0x1B..=0x32)
//!   u8       anim_index (primary)
//!   ...      remainder is parsed lazily; tests pin specific fields
//! ```
//!
//! Until a memory-write watchpoint pins the exact byte layout, the parser
//! returns a [`ParsedArtRecord`] that captures the bytes we recognised
//! plus the unconsumed tail for further analysis.

use serde::{Deserialize, Serialize};

use crate::queue::{ActionConstant, Command};
use crate::record::ArtRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Input ran out before all required fields were read.
    Truncated { needed: usize, found: usize },
    /// Action constant was not in `0x1B..=0x32`.
    InvalidAction(u8),
    /// One of the command bytes was not 1..=4 (or 0 terminator).
    InvalidCommand(u8),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Truncated { needed, found } => {
                write!(f, "art record truncated (needed {needed}, have {found})")
            }
            ParseError::InvalidAction(b) => {
                write!(
                    f,
                    "art record action byte 0x{b:02X} out of range 0x1B..=0x32"
                )
            }
            ParseError::InvalidCommand(b) => {
                write!(f, "art record command byte 0x{b:02X} not 0..=4")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Result of [`parse_record`]. The strict-decoded fields are in `record`;
/// any bytes the parser couldn't interpret are returned in `tail` so
/// downstream tooling (asset-viewer) can render them as raw hex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedArtRecord {
    pub record: ArtRecord,
    pub bytes_consumed: usize,
    pub tail: Vec<u8>,
}

/// Best-effort decode of the opening section of an art record.
///
/// Reads:
/// 1. Command sequence — bytes `1..=4` until a `0` terminator (max 16
///    commands as a defensive cap; real arts have ≤ 16).
/// 2. Action constant — single byte, must be in `0x1B..=0x32`.
/// 3. Animation index — single byte (`anim_index`).
///
/// Everything past field (3) is left in `tail` for callers that have more
/// detailed knowledge (e.g. parser for a specific PROT entry whose layout
/// has been pinned via watchpoint).
pub fn parse_record(bytes: &[u8]) -> Result<ParsedArtRecord, ParseError> {
    let mut cur = 0usize;
    // 1. Commands.
    let mut commands = Vec::new();
    let cmd_cap = 16;
    loop {
        if cur >= bytes.len() {
            return Err(ParseError::Truncated {
                needed: cur + 1,
                found: bytes.len(),
            });
        }
        let b = bytes[cur];
        cur += 1;
        if b == 0 {
            break;
        }
        let Some(cmd) = Command::from_byte(b) else {
            return Err(ParseError::InvalidCommand(b));
        };
        commands.push(cmd);
        if commands.len() >= cmd_cap {
            // Reached defensive cap without terminator — assume the
            // sequence ends here.
            break;
        }
    }
    // 2. Action constant.
    if cur >= bytes.len() {
        return Err(ParseError::Truncated {
            needed: cur + 1,
            found: bytes.len(),
        });
    }
    let action_byte = bytes[cur];
    cur += 1;
    let action = ActionConstant::from_byte(action_byte)
        .filter(|a| a.is_art())
        .ok_or(ParseError::InvalidAction(action_byte))?;
    // 3. Animation index.
    if cur >= bytes.len() {
        return Err(ParseError::Truncated {
            needed: cur + 1,
            found: bytes.len(),
        });
    }
    let anim_index = bytes[cur];
    cur += 1;

    let record = ArtRecord {
        action,
        commands,
        anim_index,
        anim_extra: vec![],
        name: None,
        power: vec![],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: crate::record::EnemyEffect::None,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };

    Ok(ParsedArtRecord {
        record,
        bytes_consumed: cur,
        tail: bytes[cur..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_record() {
        // Vahn's Craze: commands RDLULURDL, action 0x1B, anim 0x02
        // Encoded: 02 03 01 04 01 04 02 03 01 00 1B 02
        let bytes = [
            0x02, 0x03, 0x01, 0x04, 0x01, 0x04, 0x02, 0x03, 0x01, // RDLULURDL
            0x00, // command terminator
            0x1B, // action constant
            0x02, // anim index
        ];
        let parsed = parse_record(&bytes).unwrap();
        assert_eq!(
            parsed.record.commands,
            vec![
                Command::Right,
                Command::Down,
                Command::Left,
                Command::Up,
                Command::Left,
                Command::Up,
                Command::Right,
                Command::Down,
                Command::Left,
            ]
        );
        assert_eq!(parsed.record.action, ActionConstant::Art1B);
        assert_eq!(parsed.record.anim_index, 0x02);
        assert_eq!(parsed.bytes_consumed, bytes.len());
        assert!(parsed.tail.is_empty());
    }

    #[test]
    fn parse_invalid_command_rejected() {
        // 0x05 is not a valid command.
        let bytes = [0x05, 0x00, 0x1B, 0x00];
        assert_eq!(
            parse_record(&bytes).unwrap_err(),
            ParseError::InvalidCommand(0x05)
        );
    }

    #[test]
    fn parse_invalid_action_rejected() {
        // 0x1A is the special starter, not an art constant.
        let bytes = [0x00, 0x1A, 0x00];
        assert_eq!(
            parse_record(&bytes).unwrap_err(),
            ParseError::InvalidAction(0x1A)
        );
        // 0x33 is past the end of the action range.
        let bytes = [0x00, 0x33, 0x00];
        assert_eq!(
            parse_record(&bytes).unwrap_err(),
            ParseError::InvalidAction(0x33)
        );
    }

    #[test]
    fn parse_truncated_input() {
        let err = parse_record(&[]).unwrap_err();
        assert!(matches!(err, ParseError::Truncated { .. }));

        // Has command + terminator + action but no anim byte.
        let err = parse_record(&[0x01, 0x00, 0x1B]).unwrap_err();
        assert!(matches!(err, ParseError::Truncated { .. }));
    }

    #[test]
    fn tail_carries_unparsed_bytes() {
        let bytes = [0x00, 0x1B, 0x02, 0xDE, 0xAD, 0xBE, 0xEF];
        let parsed = parse_record(&bytes).unwrap();
        assert_eq!(parsed.tail, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(parsed.bytes_consumed, 3);
    }
}
