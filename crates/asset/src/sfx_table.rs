//! Static actor / battle sound-effect descriptor table (`DAT_8006F198`).
//!
//! The retail sound system keys each cue id to an 8-byte descriptor in a
//! static `SCUS_942.54` rodata table at VA `0x8006F198`. Two consumers index
//! it (both at `&DAT_8006F198 + id*8`, gated `id < 0x200`):
//!
//! * **`FUN_800250d4(sound_id, voice)`** — the per-actor SFX trigger (called
//!   from the actor tick `FUN_80021DF4`). It reads `entry[3] & 0x1F` as a voice
//!   count and `SpuKeyOn`s (`FUN_800653c8`) that many consecutive voices.
//! * **`FUN_80016b6c`** — the SFX cue-ring drainer. It walks the 4-entry ring
//!   `DAT_8007B6D8` (the same ring `FUN_8004fcc8` / [`crate::move_power`] sound
//!   cues write into), reads the descriptor, and programs each voice through
//!   `FUN_80065034` (the libsnd `SpuSetVoiceAttr` analogue).
//!
//! From those two functions the 8-byte entry decodes as (the designer field
//! names come from the runtime debug string `"setbl p:%d t:%d l:%d n:%d id:%d"`):
//!
//! | Off | Name | Meaning |
//! |---|---|---|
//! | `+0` | `p` | program / VAG index — `FUN_80065034` arg 3, indexes the loaded VAB program-attr table at `_DAT_801ce334` (stride `0x10`). |
//! | `+1` | `t` | tone / region base — `FUN_80065034` arg 4 (`+ i` per voice), indexes the ADSR region table at `_DAT_801ce340` (stride `0x20`). |
//! | `+2` | `l` | note-level voice attribute — `FUN_80065034` arg 5 (values cluster around `60`, MIDI-ish). |
//! | `+3` | `n` | low 5 bits = **voice count**; bit `0x20` = sustained / continuous mode. |
//! | `+4` | `id` | category / channel index — selects a column in the channel-volume tables `DAT_80091510` / `DAT_80091513`. |
//! | `+5..7` | — | no observed runtime reader (zero across the whole table). |
//!
//! Only the **first 100 entries (ids `0x00..=0x63`)** are real descriptors —
//! every one is populated (voice count 1..=3, `+5..7` all zero). Id `0x64`
//! onward is unrelated rodata (the `\PSX.EXE` dev-path string and friends); the
//! `id < 0x200` runtime check is an upper bound, not the table size. Sound ids
//! at or above `0x200` resolve through the *runtime* bank `_DAT_8007b8d0`
//! instead (loaded from `.dpk` / `monster.snd`), which this parser does not
//! cover.
//!
//! The actual SPU programming (`FUN_80065034` -> `SpuSetVoiceAttr`) is libsnd
//! plumbing and out of clean-room scope; what this module ports is the static
//! **data**. The engine's `legaia_engine_audio::SfxBank` consumes the decoded
//! descriptors (`program` -> program index, `note` -> key).
//!
//! Parser: `legaia_asset::sfx_table`.

/// Virtual address of the table base (`DAT_8006F198`).
pub const SFX_TABLE_VA: u32 = 0x8006_F198;

/// Bytes per descriptor.
pub const SFX_ENTRY_STRIDE: usize = 8;

/// Number of real descriptors (ids `0x00..=0x63`). Empirically pinned: every
/// entry below this is populated and `+5..7` are zero; id `0x64` starts
/// unrelated rodata. The runtime's `id < 0x200` guard is a bound, not a size.
pub const SFX_TABLE_ENTRIES: usize = 0x64;

/// One decoded 8-byte sound-effect descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SfxDescriptor {
    /// `+0` `p` — program / VAG index into the loaded bank's program-attr table.
    pub program: u8,
    /// `+1` `t` — tone / ADSR-region base (the per-voice loop adds the voice
    /// index, so a multi-voice cue spans consecutive regions).
    pub tone: u8,
    /// `+2` `l` — note-level voice attribute (MIDI-ish, clusters near 60).
    pub note: u8,
    /// `+3` `n` — raw flags byte (voice count in the low 5 bits, sustained bit
    /// `0x20`).
    pub flags: u8,
    /// `+4` `id` — category / channel-volume index.
    pub category: u8,
    /// `+5..7` — no observed runtime reader (zero across the real table).
    pub reserved: [u8; 3],
}

impl SfxDescriptor {
    /// Decode one 8-byte entry.
    pub fn from_bytes(b: &[u8; SFX_ENTRY_STRIDE]) -> Self {
        Self {
            program: b[0],
            tone: b[1],
            note: b[2],
            flags: b[3],
            category: b[4],
            reserved: [b[5], b[6], b[7]],
        }
    }

    /// Number of SPU voices the cue keys on (`flags & 0x1F`). A count of 0
    /// means the trigger does nothing.
    pub fn voice_count(&self) -> u8 {
        self.flags & 0x1F
    }

    /// Sustained / continuous mode (`flags & 0x20`) — the `FUN_80016b6c`
    /// branch that holds the voices on rather than firing a one-shot.
    pub fn sustained(&self) -> bool {
        self.flags & 0x20 != 0
    }

    /// `true` when the descriptor actually fires (`voice_count() != 0`).
    pub fn is_active(&self) -> bool {
        self.voice_count() != 0
    }
}

/// The decoded static SFX descriptor table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SfxTable {
    entries: Vec<SfxDescriptor>,
}

impl SfxTable {
    /// Parse a raw table slice (the bytes at [`SFX_TABLE_VA`]). Decodes up to
    /// [`SFX_TABLE_ENTRIES`] descriptors, stopping early if the slice is
    /// shorter. Useful for reading the live table straight out of a save
    /// state's main RAM (no PS-X EXE header required).
    pub fn from_table_bytes(bytes: &[u8]) -> Self {
        let n = (bytes.len() / SFX_ENTRY_STRIDE).min(SFX_TABLE_ENTRIES);
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let off = i * SFX_ENTRY_STRIDE;
            let chunk: &[u8; SFX_ENTRY_STRIDE] =
                bytes[off..off + SFX_ENTRY_STRIDE].try_into().unwrap();
            entries.push(SfxDescriptor::from_bytes(chunk));
        }
        Self { entries }
    }

    /// Parse the table out of a `SCUS_942.54` image via its PS-X EXE header.
    /// `None` if `scus` isn't a PS-X EXE or the table falls outside the loaded
    /// data segment.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let start = map.off(SFX_TABLE_VA)?;
        let end = start + SFX_TABLE_ENTRIES * SFX_ENTRY_STRIDE;
        let slice = scus.get(start..end)?;
        Some(Self::from_table_bytes(slice))
    }

    /// Descriptor for `sound_id`, or `None` if outside the static table.
    pub fn get(&self, sound_id: u8) -> Option<&SfxDescriptor> {
        self.entries.get(sound_id as usize)
    }

    /// All descriptors (id == index).
    pub fn entries(&self) -> &[SfxDescriptor] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `(id, descriptor)` pairs for the active (non-zero voice-count) cues.
    pub fn active(&self) -> impl Iterator<Item = (u8, &SfxDescriptor)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, d)| d.is_active())
            .map(|(i, d)| (i as u8, d))
    }
}

/// PSX-EXE `t_addr` -> file-offset resolver. `SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800` (same shape as the resolvers in
/// [`crate::item_names`] / [`crate::steal_table`]; kept local).
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_entry_fields() {
        // id 0x4C from retail: p=3 t=8 l=64 n=0x02 cat=2.
        let d = SfxDescriptor::from_bytes(&[3, 8, 64, 0x02, 2, 0, 0, 0]);
        assert_eq!(d.program, 3);
        assert_eq!(d.tone, 8);
        assert_eq!(d.note, 64);
        assert_eq!(d.voice_count(), 2);
        assert!(!d.sustained());
        assert!(d.is_active());
        assert_eq!(d.category, 2);
    }

    #[test]
    fn flags_split_count_and_sustained() {
        let d = SfxDescriptor::from_bytes(&[0, 0, 0, 0x23, 0, 0, 0, 0]);
        assert_eq!(d.voice_count(), 3, "low 5 bits");
        assert!(d.sustained(), "bit 0x20");

        let inert = SfxDescriptor::from_bytes(&[5, 5, 60, 0x00, 1, 0, 0, 0]);
        assert_eq!(inert.voice_count(), 0);
        assert!(!inert.is_active());
    }

    #[test]
    fn from_table_bytes_caps_at_static_extent() {
        // A buffer longer than the static table only yields SFX_TABLE_ENTRIES.
        let buf = vec![1u8; (SFX_TABLE_ENTRIES + 50) * SFX_ENTRY_STRIDE];
        let t = SfxTable::from_table_bytes(&buf);
        assert_eq!(t.len(), SFX_TABLE_ENTRIES);
    }

    #[test]
    fn from_table_bytes_handles_short_slice() {
        let buf = vec![0u8; 3 * SFX_ENTRY_STRIDE];
        let t = SfxTable::from_table_bytes(&buf);
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn from_scus_round_trips_synthetic_image() {
        // Minimal PS-X EXE: header at 0, data from file 0x800, t_addr base.
        const T_ADDR: u32 = 0x8001_0000;
        let table_off_in_seg = (SFX_TABLE_VA - T_ADDR) as usize;
        let total = 0x800 + table_off_in_seg + SFX_TABLE_ENTRIES * SFX_ENTRY_STRIDE;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&((total - 0x800) as u32).to_le_bytes());
        // Plant id 0x1A = p3 t0 l67 n1 cat0.
        let e = 0x800 + table_off_in_seg + 0x1A * SFX_ENTRY_STRIDE;
        buf[e..e + 8].copy_from_slice(&[3, 0, 67, 1, 0, 0, 0, 0]);

        let t = SfxTable::from_scus(&buf).expect("parse synthetic SCUS");
        assert_eq!(t.len(), SFX_TABLE_ENTRIES);
        let d = t.get(0x1A).unwrap();
        assert_eq!((d.program, d.note, d.voice_count()), (3, 67, 1));
    }

    #[test]
    fn from_scus_rejects_non_exe() {
        assert!(SfxTable::from_scus(b"not an exe").is_none());
    }
}
