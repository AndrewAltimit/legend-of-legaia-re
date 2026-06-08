//! STR FMV dispatch table — the per-`fmv_id` movie + frame range the cutscene
//! play loop selects, decoded straight from the STR/MDEC overlay.
//!
//! ## How an FMV is triggered + dispatched
//!
//! The field VM's FMV-trigger op (`0x4C 0xE2 lo hi`, handler `FUN_801E30E4`)
//! writes a signed `fmv_id` to `_DAT_8007BA78` and pokes game mode `0x1A` (26,
//! STR INIT). The STR/MDEC overlay (PROT 0970, loaded at `0x801CE818`) then runs
//! its play loop `FUN_801CF098`, which the mode dispatcher reaches via a selector
//! that indexes a **static table at VA `0x801D0A6C`** by `fmv_id * 0x40`. The
//! selected 64-byte slot's leading 32-byte record is:
//!
//! ```text
//! +0x00  u32  path_ptr     ; -> a "\MOV\MVn.STR;1" path string in the overlay
//! +0x04  u32  scale_flag   ; 0 = normal; non-0 scales the frame size by 3/2
//! +0x08  u32  start_frame  ; 1-based; the loop seeks (start-1)*10 sectors in
//! +0x0C  u32  end_frame    ; the strNext frame-count bound
//! +0x10  u32  reserved     ; 0 across every entry
//! +0x14  u32  field_14     ; 8 across every entry
//! +0x18  u32  width        ; framebuffer width (320 retail; dev slots vary)
//! +0x1C  u32  height       ; framebuffer height (240)
//! ```
//!
//! (Each 64-byte slot's *second* 32-byte record is a sibling segment the play
//! loop doesn't read for the primary path; only the leading record is dispatched
//! per `fmv_id`.) The path strings live at the very start of the overlay (the
//! `\MOV\MVn.STR;1` + `\DATA\MOV*.STR;1` table at offset `0`), so the `path_ptr`
//! resolves to `offset = path_ptr - base`.
//!
//! The 10-sectors-per-frame seek (`(start-1)*10`) is the 15 fps cadence every
//! Legaia movie runs at; `start_frame`/`end_frame` are what let one `MVn.STR`
//! file carry several distinct cutscenes (e.g. `MV3.STR` is split across three
//! `fmv_id`s by frame range).
//!
//! ## Retail vs dev slots
//!
//! The table has 12 slots. The five retail FMVs are `fmv_id 0..=4`
//! (`MV1`/`MV3`/`MV3`/`MV4`/`MV6`); the rest reference `MOV15.STR` / `MOV.STR`,
//! dev-only files absent from the released disc. [`FmvEntry::on_retail_disc`]
//! flags the difference.
//!
//! This is the read side that lets the engine source its `fmv_id -> MVn.STR`
//! mapping from the user's own disc instead of a hard-coded table; the disc-gated
//! `fmv_dispatch_real` test pins the five retail entries.

/// Load base of the STR/MDEC overlay (PROT 0970), from the static-overlay map.
pub const STR_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// VA of the per-`fmv_id` dispatch table the play-loop selector indexes.
const FMV_TABLE_VA: u32 = 0x801D_0A6C;
/// Per-`fmv_id` slot stride (`fmv_id * 0x40`); only the leading 32-byte record
/// of each slot is the dispatched movie.
const SLOT_STRIDE: usize = 0x40;
/// Number of `fmv_id` slots in the table.
pub const FMV_SLOT_COUNT: usize = 12;

/// One decoded FMV dispatch entry: which movie file the `fmv_id` plays and over
/// what frame range, plus the framebuffer dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmvEntry {
    /// The `fmv_id` (table slot index) this entry is dispatched by.
    pub fmv_id: u8,
    /// Raw path string the record points at, e.g. `\MOV\MV1.STR;1`.
    pub path: String,
    /// `scale_flag`: non-zero scales the decoded frame size by 3/2 in the loop.
    pub scale_flag: u32,
    /// 1-based start frame (the loop seeks `(start-1)*10` sectors into the file).
    pub start_frame: u32,
    /// End-frame bound for the per-frame read loop.
    pub end_frame: u32,
    /// Framebuffer width (320 for retail movies).
    pub width: u16,
    /// Framebuffer height (240).
    pub height: u16,
}

impl FmvEntry {
    /// Bare filename without directory or ISO9660 `;1` version, e.g. `MV1.STR`.
    pub fn basename(&self) -> &str {
        let no_ver = self.path.split(';').next().unwrap_or(&self.path);
        no_ver.rsplit(['\\', '/']).next().unwrap_or(no_ver)
    }

    /// Engine-shape path: forward slashes, no version, no leading slash — e.g.
    /// `MOV/MV1.STR`. Matches `legaia_engine_core::cutscene::fmv_index_to_str_filename`.
    pub fn engine_path(&self) -> String {
        let no_ver = self.path.split(';').next().unwrap_or(&self.path);
        no_ver.trim_start_matches(['\\', '/']).replace('\\', "/")
    }

    /// Whether this entry's movie is a file present on the released retail disc
    /// (the `MV*.STR` movies); the dev-only `MOV15.STR` / `MOV.STR` slots are not.
    pub fn on_retail_disc(&self) -> bool {
        let b = self.basename();
        b.starts_with("MV") && b.ends_with(".STR")
    }
}

/// The decoded STR FMV dispatch table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmvTable {
    /// One entry per `fmv_id` slot, in slot order (`0..FMV_SLOT_COUNT`).
    pub entries: Vec<FmvEntry>,
}

impl FmvTable {
    /// Decode the dispatch table from the STR overlay loaded at its committed
    /// base ([`STR_OVERLAY_BASE_VA`]). `overlay` is the raw PROT 0970 entry
    /// (byte-identical to the as-loaded overlay — `form = raw`).
    pub fn from_str_overlay(overlay: &[u8]) -> Option<Self> {
        Self::from_overlay(overlay, STR_OVERLAY_BASE_VA)
    }

    /// Decode the dispatch table from an overlay loaded at `base_va`. Returns
    /// `None` if the table region is out of range or the first slot doesn't
    /// resolve to the expected intro movie (`MV1.STR`) — a base/offset-drift
    /// guard.
    pub fn from_overlay(overlay: &[u8], base_va: u32) -> Option<Self> {
        let table_off = FMV_TABLE_VA.checked_sub(base_va)? as usize;
        let mut entries = Vec::with_capacity(FMV_SLOT_COUNT);
        for fmv_id in 0..FMV_SLOT_COUNT {
            let rec = table_off + fmv_id * SLOT_STRIDE;
            let w = |i: usize| -> Option<u32> {
                let o = rec + i * 4;
                Some(u32::from_le_bytes(overlay.get(o..o + 4)?.try_into().ok()?))
            };
            // Stop at the first slot that doesn't resolve to a path (end of
            // table / trailing garbage); the slot-0 guard below rejects a
            // wholesale base/offset miss.
            let Some(path) = w(0)
                .and_then(|ptr| ptr.checked_sub(base_va))
                .and_then(|off| read_cstr(overlay, off as usize))
            else {
                break;
            };
            let (Some(scale_flag), Some(start), Some(end), Some(width), Some(height)) =
                (w(1), w(2), w(3), w(6), w(7))
            else {
                break;
            };
            entries.push(FmvEntry {
                fmv_id: fmv_id as u8,
                path,
                scale_flag,
                start_frame: start,
                end_frame: end,
                width: width as u16,
                height: height as u16,
            });
        }
        // Drift guard: slot 0 is always the intro movie, and the five retail
        // FMVs must all decode.
        if entries.len() < 5 || entries[0].basename() != "MV1.STR" {
            return None;
        }
        Some(Self { entries })
    }

    /// The entry for `fmv_id`, or `None` if out of range / negative.
    pub fn entry(&self, fmv_id: i16) -> Option<&FmvEntry> {
        usize::try_from(fmv_id)
            .ok()
            .and_then(|i| self.entries.get(i))
    }

    /// Engine-shape movie path for `fmv_id`, restricted to retail-disc movies
    /// (`None` for the dev-only `MOV*.STR` slots). Mirrors
    /// `legaia_engine_core::cutscene::fmv_index_to_str_filename`.
    pub fn engine_path(&self, fmv_id: i16) -> Option<String> {
        let e = self.entry(fmv_id)?;
        e.on_retail_disc().then(|| e.engine_path())
    }
}

/// Read a NUL-terminated ASCII string at `off`, or `None` if it runs off the end
/// or isn't printable.
fn read_cstr(buf: &[u8], off: usize) -> Option<String> {
    let rest = buf.get(off..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    let s = &rest[..end];
    if s.is_empty() || !s.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        return None;
    }
    Some(s.iter().map(|&b| b as char).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal overlay: a path-string blob at offset 0, then the
    /// dispatch table at `FMV_TABLE_VA`, and decode it.
    #[test]
    fn decodes_a_synthetic_table() {
        let base = STR_OVERLAY_BASE_VA;
        let table_off = (FMV_TABLE_VA - base) as usize;
        let mut buf = vec![0u8; table_off + FMV_SLOT_COUNT * SLOT_STRIDE];

        // Path strings near the start.
        let p_mv1 = 0x10usize;
        let p_mv3 = 0x20usize;
        let p_mov = 0x30usize;
        buf[p_mv1..p_mv1 + 14].copy_from_slice(b"\\MOV\\MV1.STR;1");
        buf[p_mv3..p_mv3 + 14].copy_from_slice(b"\\MOV\\MV3.STR;1");
        buf[p_mov..p_mov + 15].copy_from_slice(b"\\DATA\\MOV.STR;1");

        let mut put = |slot: usize, words: [u32; 8]| {
            let rec = table_off + slot * SLOT_STRIDE;
            for (i, v) in words.iter().enumerate() {
                buf[rec + i * 4..rec + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        // fmv 0 -> MV1 frames 1..0x53a, 320x240.
        put(0, [base + p_mv1 as u32, 1, 1, 0x53a, 0, 8, 320, 240]);
        // fmv 1 -> MV3 second segment, start 0x1a5, 320x240.
        put(1, [base + p_mv3 as u32, 0, 0x1a5, 0x27b, 0, 8, 320, 240]);
        // fmv 2..4 -> filler MV1 so the retail prefix (>=5) decodes.
        for slot in 2..4 {
            put(slot, [base + p_mv1 as u32, 0, 1, 0x10, 0, 8, 320, 240]);
        }
        // fmv 4 -> a dev MOV.STR slot (not on the retail disc).
        put(4, [base + p_mov as u32, 0, 1, 0x64, 0, 8, 256, 240]);

        let t = FmvTable::from_str_overlay(&buf).expect("decode");
        assert_eq!(t.entries.len(), 5, "decode stops after the filled slots");
        let e0 = t.entry(0).unwrap();
        assert_eq!(e0.basename(), "MV1.STR");
        assert_eq!(e0.engine_path(), "MOV/MV1.STR");
        assert_eq!((e0.start_frame, e0.end_frame), (1, 0x53a));
        assert_eq!((e0.width, e0.height), (320, 240));
        assert!(e0.on_retail_disc());

        let e1 = t.entry(1).unwrap();
        assert_eq!(e1.basename(), "MV3.STR");
        assert_eq!(e1.start_frame, 0x1a5);
        assert_eq!(t.engine_path(1).as_deref(), Some("MOV/MV3.STR"));

        // The dev MOV.STR slot decodes but isn't a retail movie.
        let e4 = t.entry(4).unwrap();
        assert!(!e4.on_retail_disc());
        assert_eq!(t.engine_path(4), None);
    }

    /// A wrong base (slot 0 doesn't resolve to MV1.STR) is rejected.
    #[test]
    fn rejects_drifted_base() {
        let buf =
            vec![0u8; (FMV_TABLE_VA - STR_OVERLAY_BASE_VA) as usize + FMV_SLOT_COUNT * SLOT_STRIDE];
        assert!(FmvTable::from_str_overlay(&buf).is_none());
    }
}
