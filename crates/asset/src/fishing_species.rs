//! Fishing-minigame **per-species parameter table** (overlay VA `0x801D81A4`).
//!
//! The hooked-fight AI tick [`FUN_801d4004`] and the catch-scoring routine
//! [`FUN_801d5298`] (`ghidra/scripts/funcs/overlay_fishing_801d4004.txt`,
//! `overlay_fishing_801d5298.txt`) index a fixed-stride table by the hooked-fish
//! species id `DAT_801d91cc`:
//!
//! ```text
//! 801d4??? :  iVar5 = DAT_801d91cc * 0x28;            ; record byte offset
//! 801d4??? :  ... * *(int *)(&DAT_801d81ac + iVar5)   ; +0x08 read
//! 801d5??? :  *(int *)(&DAT_801d81a8 + DAT_801d91cc*0x28) * (strength+0x9c0) / 0x32000
//! ```
//!
//! and the decompiler resolved the record head to a fish-name pointer:
//! `(&PTR_s_Spikefish_801d81a4)[DAT_801d91cc * 10]` - i.e. record `N` is at
//! `0x801D81A4 + N*0x28`, its first word (`+0x00`) is a pointer to the species'
//! name string (which lives in this same overlay's `.rodata`).
//!
//! ## Record layout (10 words, stride `0x28`)
//!
//! Every field below has a *confirmed reader* in the fishing overlay; the
//! designer-level meaning is the consuming formula (`Inferred` where a label is
//! a judgement, `Confirmed` for the read + arithmetic):
//!
//! | Off | Field | Consuming site / formula |
//! |---|---|---|
//! | `+0x00` | name pointer | `FUN_801d4004:620` - the hooked-fish name banner string |
//! | `+0x04` | score base value | `FUN_801d5298` - `points = value*(strength+0x9c0)/0x32000` |
//! | `+0x08` | pull factor | `FUN_801d4004:629` - per-frame pull `((rand&0xff)+bias)*f/150`; also a `/0xc8000` term |
//! | `+0x0c` | dart push factor | `FUN_801d4004:696` - dart-state lateral push `((step>>2)+0x20)*f/100` |
//! | `+0x10` | depth-sink factor | `FUN_801d4004:641` - run-state line-sink `(pull*f)/150` |
//! | `+0x14` | depth gate | `FUN_801d4004:765` - behaviour pick when `f < line-depth` |
//! | `+0x18` | behaviour-roll cutoff A | `FUN_801d4004:766` - `f <= rand&0xfff` |
//! | `+0x1c` | behaviour-roll cutoff B | `FUN_801d4004:768` - `rand&0xfff < f` |
//! | `+0x20` | behaviour-roll cutoff C | `FUN_801d4004:754` - `rand&0xfff < f` |
//! | `+0x24` | strike/record gate | `FUN_801d4004:753` - hook check `record < f + 300` |
//!
//! ## Provenance - static overlay data, pinned on disc
//!
//! The table is **static** `.rodata` in the fishing minigame overlay (PROT
//! entry **0972**, `data\OTHER1`; base [`FISHING_OVERLAY_BASE_VA`], see
//! `crates/asset/data/static-overlays.toml`). The name pointers are absolute
//! VAs baked into the overlay image, so the whole table is reproducible from the
//! user's `PROT.DAT` with no capture (`fishing_species_real`). No Sony bytes are
//! committed - this module decodes them from the user's disc at runtime.
//!
//! ## Extent
//!
//! The clean structure runs for [`SPECIES_COUNT`] records; record 10's `+0x00`
//! is no longer an in-overlay pointer, which bounds the table.

/// CDNAME / PROT index of the fishing minigame overlay (`data\OTHER1`).
pub const FISHING_OVERLAY_PROT_INDEX: usize = 972;

/// Load base of the fishing overlay (the shared slot-A minigame base). A runtime
/// VA in this overlay maps to a file offset as `va - FISHING_OVERLAY_BASE_VA`.
pub const FISHING_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Runtime VA of the species table head (record 0, `&PTR_s_Spikefish_801d81a4`).
pub const SPECIES_TABLE_VA: u32 = 0x801D_81A4;

/// File offset of the species table within the as-loaded overlay image.
pub const SPECIES_TABLE_FILE_OFFSET: usize = (SPECIES_TABLE_VA - FISHING_OVERLAY_BASE_VA) as usize;

/// Per-record stride (the `DAT_801d91cc * 0x28` index math).
pub const SPECIES_RECORD_STRIDE: usize = 0x28;

/// Number of fish species records before the table ends (`+0x00` of record 10 is
/// no longer an in-overlay name pointer).
pub const SPECIES_COUNT: usize = 10;

/// Strength bias added before the score divide (`FUN_801d5298`: `+ 0x9c0`).
pub const SCORE_STRENGTH_BIAS: i32 = 0x9c0;

/// Score divisor (`FUN_801d5298`: `/ 0x32000`).
pub const SCORE_DIVISOR: i32 = 0x3_2000;

/// One decoded fishing-species record (stride [`SPECIES_RECORD_STRIDE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingSpecies {
    /// Species id = index into the table (`DAT_801d91cc`).
    pub index: usize,
    /// `+0x00` - VA of the species' name C-string (within this overlay).
    pub name_ptr_va: u32,
    /// `+0x04` - score base value (feeds the catch-points formula).
    pub score_value: i32,
    /// `+0x08` - per-frame pull factor.
    pub pull_factor: i32,
    /// `+0x0c` - dart-state lateral push factor.
    pub dart_factor: i32,
    /// `+0x10` - run-state line-sink factor.
    pub sink_factor: i32,
    /// `+0x14` - line-depth gate for the behaviour sub-state pick.
    pub depth_gate: i32,
    /// `+0x18` - behaviour-roll cutoff A (`rand & 0xfff`).
    pub roll_cutoff_a: i32,
    /// `+0x1c` - behaviour-roll cutoff B (`rand & 0xfff`).
    pub roll_cutoff_b: i32,
    /// `+0x20` - behaviour-roll cutoff C (`rand & 0xfff`).
    pub roll_cutoff_c: i32,
    /// `+0x24` - hook / record gate (`record < f + 300`).
    pub strike_gate: i32,
}

impl FishingSpecies {
    /// The awarded catch points for a fight of accumulated `strength`
    /// (`FUN_801d5298`: `score_value * (strength + 0x9c0) / 0x32000`).
    pub fn score_for(&self, strength: i32) -> i32 {
        ((self.score_value as i64 * (strength as i64 + SCORE_STRENGTH_BIAS as i64))
            / SCORE_DIVISOR as i64) as i32
    }

    /// Resolve the `+0x00` name pointer to the in-overlay C-string. Returns
    /// `None` if the pointer falls outside the overlay or is not NUL-terminated
    /// printable ASCII.
    pub fn name<'a>(&self, overlay: &'a [u8]) -> Option<&'a str> {
        resolve_name(overlay, self.name_ptr_va)
    }
}

/// Resolve an in-overlay VA to its NUL-terminated ASCII string.
pub fn resolve_name(overlay: &[u8], va: u32) -> Option<&str> {
    if va < FISHING_OVERLAY_BASE_VA {
        return None;
    }
    let off = (va - FISHING_OVERLAY_BASE_VA) as usize;
    let rest = overlay.get(off..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    let s = &rest[..end];
    if s.is_empty() || !s.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        return None;
    }
    std::str::from_utf8(s).ok()
}

/// Parse the [`SPECIES_COUNT`] species records out of the as-loaded fishing
/// overlay image (PROT entry [`FISHING_OVERLAY_PROT_INDEX`]).
pub fn parse(overlay: &[u8]) -> Option<Vec<FishingSpecies>> {
    parse_at(overlay, SPECIES_TABLE_FILE_OFFSET, SPECIES_COUNT)
}

/// Parse `count` records starting at file offset `off`. Returns `None` if the
/// buffer is too short to hold them.
pub fn parse_at(overlay: &[u8], off: usize, count: usize) -> Option<Vec<FishingSpecies>> {
    let need = off + count * SPECIES_RECORD_STRIDE;
    if overlay.len() < need {
        return None;
    }
    let rd = |base: usize, field: usize| -> i32 {
        let p = base + field;
        i32::from_le_bytes([overlay[p], overlay[p + 1], overlay[p + 2], overlay[p + 3]])
    };
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let b = off + i * SPECIES_RECORD_STRIDE;
        out.push(FishingSpecies {
            index: i,
            name_ptr_va: rd(b, 0x00) as u32,
            score_value: rd(b, 0x04),
            pull_factor: rd(b, 0x08),
            dart_factor: rd(b, 0x0c),
            sink_factor: rd(b, 0x10),
            depth_gate: rd(b, 0x14),
            roll_cutoff_a: rd(b, 0x18),
            roll_cutoff_b: rd(b, 0x1c),
            roll_cutoff_c: rd(b, 0x20),
            strike_gate: rd(b, 0x24),
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offset_math() {
        assert_eq!(SPECIES_TABLE_FILE_OFFSET, 0x998C);
        assert_eq!(SPECIES_RECORD_STRIDE, 0x28);
    }

    #[test]
    fn score_formula_matches_kernel() {
        // Synthetic record shaped like the disc's id-0 fish (score base 10000).
        let f = FishingSpecies {
            index: 0,
            name_ptr_va: 0,
            score_value: 10_000,
            pull_factor: 250,
            dart_factor: 60,
            sink_factor: 4,
            depth_gate: 1024,
            roll_cutoff_a: 200,
            roll_cutoff_b: 512,
            roll_cutoff_c: 90,
            strike_gate: 400,
        };
        // points = 10000 * (strength + 0x9c0) / 0x32000
        assert_eq!(f.score_for(0), (10_000 * 0x9c0) / 0x3_2000);
        assert_eq!(f.score_for(0x32000 - 0x9c0), 10_000); // unit-strength check
    }

    #[test]
    fn parse_reads_stride_and_fields() {
        let off = 0x10;
        let mut buf = vec![0u8; off + 2 * SPECIES_RECORD_STRIDE];
        // record 1: name ptr 0x801ceb68, score 14000, pull 270.
        let b = off + SPECIES_RECORD_STRIDE;
        buf[b..b + 4].copy_from_slice(&0x801c_eb68u32.to_le_bytes());
        buf[b + 4..b + 8].copy_from_slice(&14_000i32.to_le_bytes());
        buf[b + 8..b + 12].copy_from_slice(&270i32.to_le_bytes());
        let recs = parse_at(&buf, off, 2).expect("parses");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[1].name_ptr_va, 0x801c_eb68);
        assert_eq!(recs[1].score_value, 14_000);
        assert_eq!(recs[1].pull_factor, 270);
    }

    #[test]
    fn resolve_name_in_overlay() {
        // overlay base + a string at offset 0x20.
        let mut ov = vec![0u8; 0x40];
        ov[0x20..0x28].copy_from_slice(b"Lippian\0");
        let va = FISHING_OVERLAY_BASE_VA + 0x20;
        assert_eq!(resolve_name(&ov, va), Some("Lippian"));
        // out-of-range / below-base pointers reject.
        assert_eq!(resolve_name(&ov, 0x1000), None);
        assert_eq!(resolve_name(&ov, FISHING_OVERLAY_BASE_VA + 0x1000), None);
    }
}
