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

// --- Species-spawn tables -------------------------------------------------
//
// Directly after the species table sit two per-venue **spawn tables** paged
// into `PTR_DAT_801d9114` by the same venue select that pages the exchange
// tables (`FUN_801cf3bc` case 1; see [`crate::fishing_exchange`]). The
// hooked-fish handler picks the hooked species as
// `species = table[(lure * 8 + band) * 4]` (`FUN_801d26cc`,
// `overlay_fishing_801d26cc.txt` line ~1856) where `lure` = the equipped
// **lure** index `_DAT_80084450` (0..2: Light / Normal / Heavy - rows 3..8
// are zero padding) and `band` = the cast band `DAT_801d90e8` (0..4). Band 4
// is reachable only through the strike-time band-4 gate (venue-hardwired
// lure + third rod + band 0 + a `rand` mask; Buma additionally needs > 50
// lifetime casts with the counter even) - see
// `docs/subsystems/minigame-fishing.md` "Species selection and the band-4
// gate". (An earlier revision of this comment indexed the rows by rod and
// described a deep-cast band-4 path; both halves were wrong.)

/// Runtime VA of the venue-0 spawn table (`&DAT_801d8334`).
pub const SPAWN_TABLE_VA_PAGE0: u32 = 0x801D_8334;

/// Runtime VA of the venue-1 spawn table (`&DAT_801d8434`).
pub const SPAWN_TABLE_VA_PAGE1: u32 = 0x801D_8434;

/// Rows per spawn table (indexed by the equipped-rod id; rows 3..8 are
/// zero-filled padding - only 3 rods exist).
pub const SPAWN_RODS: usize = 8;

/// Slots per rod row (indexed by the cast band 0..4; slots 5..8 unused).
pub const SPAWN_BANDS: usize = 8;

/// Parse one venue's spawn table (`SPAWN_RODS x SPAWN_BANDS` u32 species
/// ids) at overlay VA `table_va`.
pub fn parse_spawn_table(overlay: &[u8], table_va: u32) -> Option<Vec<[u32; SPAWN_BANDS]>> {
    let off = table_va.checked_sub(FISHING_OVERLAY_BASE_VA)? as usize;
    let need = off + SPAWN_RODS * SPAWN_BANDS * 4;
    if overlay.len() < need {
        return None;
    }
    let mut rows = Vec::with_capacity(SPAWN_RODS);
    for rod in 0..SPAWN_RODS {
        let mut row = [0u32; SPAWN_BANDS];
        for (band, slot) in row.iter_mut().enumerate() {
            let p = off + (rod * SPAWN_BANDS + band) * 4;
            *slot =
                u32::from_le_bytes([overlay[p], overlay[p + 1], overlay[p + 2], overlay[p + 3]]);
        }
        rows.push(row);
    }
    Some(rows)
}

/// Parse both venue spawn tables (`[0]` = venue 0 / Buma page,
/// `[1]` = venue 1 / Vidna page).
pub fn parse_spawn_tables(overlay: &[u8]) -> Option<[Vec<[u32; SPAWN_BANDS]>; 2]> {
    Some([
        parse_spawn_table(overlay, SPAWN_TABLE_VA_PAGE0)?,
        parse_spawn_table(overlay, SPAWN_TABLE_VA_PAGE1)?,
    ])
}

// --- Reel-cadence gesture templates ----------------------------------------
//
// The rodata the reel-cadence recogniser (`FUN_801d3db4`) walks its 16-slot
// `{button, held-frames}` ring buffer against: four `0x40`-byte records at
// `DAT_801d87d4`, each `u32 step_count`, `u32 history_window` (frame-steps),
// then `step_count` pairs of `{u32 duration, u32 button}` matched backwards
// from the newest ring slot with a +-10 frame-step tolerance. The matched
// template's id is stored **as the cast band** by the pre-hook check in
// `FUN_801d26cc`. Button values are the `FUN_801d7450` decode: `0` idle,
// `1` reel A (Cross), `2` reel B (Square). See
// `docs/subsystems/minigame-fishing.md` "Reel-button decode and cadence".

/// Runtime VA of the gesture-template rodata (`DAT_801d87d4`).
pub const CADENCE_TEMPLATE_VA: u32 = 0x801D_87D4;

/// Number of gesture templates (template id = cast band 0..=3).
pub const CADENCE_TEMPLATE_COUNT: usize = 4;

/// Byte stride of one template record.
pub const CADENCE_TEMPLATE_STRIDE: usize = 0x40;

/// Matching tolerance on each step's held duration, in frame-steps.
pub const CADENCE_TOLERANCE: i32 = 10;

/// One reel-cadence step: hold `button` (`0` idle / `1` Cross / `2` Square)
/// for `duration` frame-steps (+- [`CADENCE_TOLERANCE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CadenceStep {
    /// Held duration in frame-steps.
    pub duration: i32,
    /// Decoded reel button (`FUN_801d7450` value space).
    pub button: u8,
}

/// One decoded gesture template (`DAT_801d87d4` record; id = cast band).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CadenceTemplate {
    /// History window the match may span, in frame-steps.
    pub history_window: i32,
    /// The steps, in chronological order (matched backwards from the newest
    /// ring slot).
    pub steps: Vec<CadenceStep>,
}

/// Parse the four gesture templates out of the as-loaded fishing overlay
/// image. Rejects records whose step count doesn't fit the `0x40`-byte
/// record (`2 + 2*steps` words), so a wrong base or a truncated image
/// returns `None` instead of garbage cadences.
pub fn parse_cadence_templates(overlay: &[u8]) -> Option<Vec<CadenceTemplate>> {
    let base = CADENCE_TEMPLATE_VA.checked_sub(FISHING_OVERLAY_BASE_VA)? as usize;
    let need = base + CADENCE_TEMPLATE_COUNT * CADENCE_TEMPLATE_STRIDE;
    if overlay.len() < need {
        return None;
    }
    let word = |p: usize| -> u32 {
        u32::from_le_bytes([overlay[p], overlay[p + 1], overlay[p + 2], overlay[p + 3]])
    };
    let mut out = Vec::with_capacity(CADENCE_TEMPLATE_COUNT);
    for t in 0..CADENCE_TEMPLATE_COUNT {
        let rec = base + t * CADENCE_TEMPLATE_STRIDE;
        let step_count = word(rec) as usize;
        let history_window = word(rec + 4) as i32;
        // A 0x40-byte record holds at most (0x40/4 - 2) / 2 = 7 steps.
        if step_count == 0 || step_count > (CADENCE_TEMPLATE_STRIDE / 4 - 2) / 2 {
            return None;
        }
        let mut steps = Vec::with_capacity(step_count);
        for s in 0..step_count {
            let p = rec + 8 + s * 8;
            let duration = word(p) as i32;
            let button = word(p + 4);
            // A zero duration is real: template 1 ends in a "release" step
            // ({0, idle}) matched inside the +-10 tolerance.
            if button > 2 || !(0..=0x1000).contains(&duration) {
                return None;
            }
            steps.push(CadenceStep {
                duration,
                button: button as u8,
            });
        }
        out.push(CadenceTemplate {
            history_window,
            steps,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_templates_parse_and_reject_garbage() {
        let base = (CADENCE_TEMPLATE_VA - FISHING_OVERLAY_BASE_VA) as usize;
        let mut ov = vec![0u8; base + CADENCE_TEMPLATE_COUNT * CADENCE_TEMPLATE_STRIDE];
        // Template shapes from the doc's table (durations are placeholders -
        // the real values come off the visitor's disc).
        let shapes: [&[(u32, u32)]; 4] = [
            &[(40, 0), (25, 1), (40, 0), (15, 2)],
            &[(15, 2), (25, 1), (10, 0)],
            &[(15, 2), (40, 0), (15, 2)],
            &[(25, 1), (40, 0), (25, 1)],
        ];
        for (t, steps) in shapes.iter().enumerate() {
            let rec = base + t * CADENCE_TEMPLATE_STRIDE;
            ov[rec..rec + 4].copy_from_slice(&(steps.len() as u32).to_le_bytes());
            ov[rec + 4..rec + 8].copy_from_slice(&200u32.to_le_bytes());
            for (s, &(d, b)) in steps.iter().enumerate() {
                let p = rec + 8 + s * 8;
                ov[p..p + 4].copy_from_slice(&d.to_le_bytes());
                ov[p + 4..p + 8].copy_from_slice(&b.to_le_bytes());
            }
        }
        let parsed = parse_cadence_templates(&ov).expect("parses");
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].steps.len(), 4);
        assert_eq!(
            parsed[0].steps[3],
            CadenceStep {
                duration: 15,
                button: 2
            }
        );
        assert_eq!(parsed[3].steps[0].button, 1);
        // A garbage step count rejects the whole parse.
        ov[base..base + 4].copy_from_slice(&99u32.to_le_bytes());
        assert!(parse_cadence_templates(&ov).is_none());
        // Too-short image rejects.
        assert!(parse_cadence_templates(&ov[..base]).is_none());
    }

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
