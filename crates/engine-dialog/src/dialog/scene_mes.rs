//! `SceneMes`: a scene's shared MES container and its format.

use super::*;

/// MES dialog container resolved out of one of the scene's PROT entries.
/// Holds an owned copy of the entry bytes plus the parsed offset table /
/// record markers so `SceneAssets::mes_message_bytes` can resolve a
/// `text_id` to a bytecode slice without re-parsing.
///
/// Two formats coexist (see [`docs/formats/mes.md`]):
/// - [`MesFormat::Compact`] - 0x404 magic + 16-byte runtime header at
///   `0x28` + offset table from `0x62..0xC8` (3-byte little-endian
///   offsets). Bytecode lives past the table at `0xC8`.
/// - [`MesFormat::Records`] - variable-stride records marked by
///   `0x44 0x78` (per-record byte counts inferred from neighbouring
///   markers).
#[derive(Debug, Clone)]
pub struct SceneMes {
    pub entry_idx: u32,
    /// Byte offset where the MES blob starts within `bytes`. Always 0 for
    /// entries whose entire body is the MES container.
    pub offset: usize,
    /// Owned blob bytes (the slice the offset table indexes into).
    pub bytes: Vec<u8>,
    pub format: MesFormat,
    /// `Compact` only - 3-byte LE offset table from `0x62..0xC8`,
    /// rebuilt as 32-bit values. `None` for `Records`.
    pub offset_table: Option<Vec<u32>>,
    /// `Records` only - record-start offsets (where each `0x44 0x78`
    /// marker lives). Empty for `Compact`.
    pub record_offsets: Vec<usize>,
}

impl SceneMes {
    /// Resolve `text_id` to the bytecode start within [`SceneMes::bytes`].
    /// Returns `None` if the id is past the offset table or out of range
    /// for the records vector.
    pub fn message_offset(&self, text_id: u16) -> Option<usize> {
        match self.format {
            MesFormat::Compact => {
                let table = self.offset_table.as_ref()?;
                let raw = *table.get(text_id as usize)?;
                Some(raw as usize)
            }
            MesFormat::Records => self.record_offsets.get(text_id as usize).copied(),
        }
    }

    /// Borrow the bytecode slice starting at `text_id`'s offset. Slice runs
    /// to the buffer end - the iterator stops at the first
    /// [`legaia_mes::Token::EndOfMessage`].
    pub fn message_bytes(&self, text_id: u16) -> Option<&[u8]> {
        let off = self.message_offset(text_id)?;
        self.bytes.get(off..)
    }

    /// Number of messages - table length for `Compact`, marker count for
    /// `Records`.
    pub fn message_count(&self) -> usize {
        match self.format {
            MesFormat::Compact => self.offset_table.as_ref().map_or(0, Vec::len),
            MesFormat::Records => self.record_offsets.len(),
        }
    }
}
