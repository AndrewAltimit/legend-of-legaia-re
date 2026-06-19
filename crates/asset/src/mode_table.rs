//! Game-mode dispatch table parser (`SCUS_942.54`).
//!
//! The master game-mode state machine is driven by a static 28-entry table
//! at runtime VA `0x8007078C`, each entry 24 bytes
//! (`docs/subsystems/boot.md` § Game-mode state machine). Modes come in
//! init/per-frame pairs: even index = init handler, odd index = per-frame
//! handler. This module reads the table's handler pointers + parameters +
//! dev name strings straight out of the executable so the index → retail
//! handler map is recovered from the disc rather than guessed from the
//! (misleading) dev mode names.
//!
//! Per-entry layout:
//!
//! | Offset | Width | Field |
//! |---|---|---|
//! | `+0x00` | u32 | dev name-string pointer |
//! | `+0x04` | u32 | reserved / zero |
//! | `+0x08` | u16 | reserved / zero (low half of the next-mode word) |
//! | `+0x0A` | i16 | next-mode index: `-1` = self-managed, `0` = return to mode 0 (CONFIG) |
//! | `+0x0C` | u32 | reserved / zero |
//! | `+0x10` | u32 | handler function pointer |
//! | `+0x14` | u32 | handler parameter |
//!
//! The `+0x08` word reads `0xFFFF0000` on self-managed modes - that is not a
//! sentinel constant but the `i16` next-mode field at `+0x0A` holding `-1`
//! over a zero low half. Retail uses only two values: `-1` (the mode manages
//! its own transitions) and `0` (on completion, fall back to mode 0).
//!
//! A key structural fact this recovers: 12 of the 14 per-frame (odd) modes
//! share one generic per-frame handler `0x80025EEC`; only Mode 13 (MAPDISP
//! MODE, `0x80025F2C`) and Mode 23 (CARD MODE, `0x80025F74`) carry their own.

/// Runtime VA of the mode-dispatch table.
pub const MODE_TABLE_VA: u32 = 0x8007_078C;

/// Number of mode-table entries (init + per-frame pairs).
pub const MODE_COUNT: usize = 28;

/// Per-entry stride in bytes.
pub const ENTRY_STRIDE: usize = 24;

/// The generic per-frame handler shared by most per-frame (odd) modes.
pub const SHARED_PER_FRAME_HANDLER: u32 = 0x8002_5EEC;

/// One decoded mode-table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeEntry {
    /// Mode index (0..28). Even = init, odd = per-frame.
    pub index: usize,
    /// Dev name string (e.g. `"MAIN INIT"` / `"MAIN MODE"`), as embedded in
    /// the executable. Empty if it couldn't be resolved.
    pub name: String,
    /// Raw `+0x08` word. The high half is the `i16` next-mode field at
    /// `+0x0A` (see [`ModeEntry::next_mode`]); the low half is always zero
    /// in retail.
    pub next_word: u32,
    /// `+0x10` handler function pointer (runtime VA). May land in the overlay
    /// window `0x801C0000+` when an overlay is resident for that mode.
    pub handler: u32,
    /// `+0x14` handler parameter.
    pub param: u32,
}

impl ModeEntry {
    /// True for per-frame (odd-index) modes; false for init (even) modes.
    pub fn is_per_frame(&self) -> bool {
        self.index % 2 == 1
    }

    /// The `i16` next-mode field at `+0x0A`: the mode index the dispatcher
    /// transitions to when the handler signals completion. `None` for the
    /// retail `-1` sentinel (mode manages its own transitions).
    pub fn next_mode(&self) -> Option<usize> {
        let next = (self.next_word >> 16) as i16;
        usize::try_from(next).ok()
    }

    /// True when this per-frame mode uses the shared generic handler
    /// [`SHARED_PER_FRAME_HANDLER`] rather than its own.
    pub fn uses_shared_handler(&self) -> bool {
        self.handler == SHARED_PER_FRAME_HANDLER
    }
}

/// The full 28-entry mode table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeTable {
    pub entries: Vec<ModeEntry>,
}

/// PSX-EXE `t_addr` → file-offset resolver (sibling of the one in
/// [`crate::spell_names`]).
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

fn read_u32(scus: &[u8], off: usize) -> Option<u32> {
    scus.get(off..off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn read_name(scus: &[u8], map: &ExeMap, va: u32) -> String {
    let Some(start) = map.off(va) else {
        return String::new();
    };
    let mut out = String::new();
    let mut i = start;
    while i < scus.len() && out.len() < 32 {
        let b = scus[i];
        if b == 0 {
            break;
        }
        if (0x20..0x7F).contains(&b) {
            out.push(b as char);
        }
        i += 1;
    }
    out.trim().to_string()
}

impl ModeTable {
    /// Parse the mode table out of a `SCUS_942.54` image. `None` if the image
    /// isn't a PSX-EXE or the table VA falls outside its `.text` segment.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let base = map.off(MODE_TABLE_VA)?;
        let mut entries = Vec::with_capacity(MODE_COUNT);
        for index in 0..MODE_COUNT {
            let e = base + index * ENTRY_STRIDE;
            let name_ptr = read_u32(scus, e)?;
            let next_word = read_u32(scus, e + 0x08)?;
            let handler = read_u32(scus, e + 0x10)?;
            let param = read_u32(scus, e + 0x14)?;
            entries.push(ModeEntry {
                index,
                name: read_name(scus, &map, name_ptr),
                next_word,
                handler,
                param,
            });
        }
        Some(Self { entries })
    }

    /// The entry for `index`, if in range.
    pub fn entry(&self, index: usize) -> Option<&ModeEntry> {
        self.entries.get(index)
    }

    /// Count of per-frame modes routed through [`SHARED_PER_FRAME_HANDLER`].
    pub fn shared_handler_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.uses_shared_handler())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PSX-EXE carrying a mode table with two crafted entries
    /// at the table VA, to exercise the offset math + field decode without a
    /// real disc.
    fn synth_scus() -> Vec<u8> {
        const T_ADDR: u32 = 0x8001_0000;
        // Cover up to the table + 28 entries.
        let table_off = (MODE_TABLE_VA - T_ADDR) as usize + 0x800;
        let total = table_off + MODE_COUNT * ENTRY_STRIDE + 16;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&((total - 0x800) as u32).to_le_bytes());
        // Entry 0 (init): handler 0x80025C68, param 2.
        let e0 = table_off;
        buf[e0 + 0x10..e0 + 0x14].copy_from_slice(&0x8002_5C68u32.to_le_bytes());
        buf[e0 + 0x14..e0 + 0x18].copy_from_slice(&2u32.to_le_bytes());
        buf[e0 + 0x08..e0 + 0x0C].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        // Entry 3 (per-frame): shared handler.
        let e3 = table_off + 3 * ENTRY_STRIDE;
        buf[e3 + 0x10..e3 + 0x14].copy_from_slice(&SHARED_PER_FRAME_HANDLER.to_le_bytes());
        buf
    }

    #[test]
    fn parses_28_entries_with_field_decode() {
        let scus = synth_scus();
        let t = ModeTable::from_scus(&scus).expect("parse");
        assert_eq!(t.entries.len(), MODE_COUNT);
        let e0 = t.entry(0).unwrap();
        assert_eq!(e0.handler, 0x8002_5C68);
        assert_eq!(e0.param, 2);
        assert_eq!(e0.next_word, 0xFFFF_0000);
        assert_eq!(e0.next_mode(), None, "-1 = self-managed");
        assert!(!e0.is_per_frame());
        // A zero next-word decodes as "fall back to mode 0".
        assert_eq!(t.entry(1).unwrap().next_mode(), Some(0));
        let e3 = t.entry(3).unwrap();
        assert!(e3.is_per_frame());
        assert!(e3.uses_shared_handler());
    }

    #[test]
    fn rejects_non_exe() {
        assert!(ModeTable::from_scus(b"not an exe").is_none());
    }
}
