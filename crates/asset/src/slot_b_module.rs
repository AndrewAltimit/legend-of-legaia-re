//! End-to-end layout of a **slot-B module image** (PROT `0903..=0966`), the
//! band of per-spell summon stagers and capture-class cast modules that
//! timeshare the second overlay window at
//! [`SLOT_B_LINK_BASE`](crate::summon_overlay::SUMMON_OVERLAY_LINK_BASE).
//!
//! [`crate::cast_effect_pool`] indexes the band by PROT entry and hands each
//! image to [`crate::summon_overlay::parse`], which answers *what a module
//! spawns*. This module answers the sibling question the byte-accounting
//! instruments ask - **which bytes of the image are what** - and it answers it
//! at a granularity the spawn parser does not: a per-record half-open extent,
//! cut where the image's own code partition resumes.
//!
//! ## The three regions
//!
//! | region | what it is | how it is recovered |
//! |---|---|---|
//! | head table | 0 to 256 words of in-image VAs: the jump table of *one* of the module's two switches (which one is settled by the `sltiu` immediate - see [`cast-module.md`](https://andrewaltimit.github.io/legend-of-legaia-re/subsystems/cast-module.html)) | leading run of words inside `[base, base + len)`, capped at the first framed function |
//! | code | the tick's `ctx+0x279` phase machine, the `0x801F6734` spawn stager, the capture-class trampolines | frame matching: `addiu sp, sp, -F` to the first `jr ra` whose delay slot restores the same `F` |
//! | spawn-record band | the module's own data: `[i16 model_sel][u16 reserved][move-VM bytecode]` records the stager hands to `FUN_80021B04` / `FUN_80050ED4` | the **consumer's** pointer-forming instruction (below) |
//!
//! The three are not laid out head-to-tail in every image. PROT 0943
//! (`cast_curse`) and PROT 0961 (`cast_dead_end_crisis`) both put a record band
//! *between* two code partitions, so a rule that reads the band as "everything
//! past the last function" mislabels real routines as data. [`records`] cuts
//! every claim at the next framed-function start for exactly that reason.
//!
//! ## The evidence a record extent rests on
//!
//! A record is named by an instruction, never by a statistic: the module forms
//! its address with a `lui` / `addiu` pair and passes it in `$a2` to a `jal`
//! into one of the two spawn helpers. The end of one record is the start of the
//! next, so both ends of a claim are addresses the module's own code computes.
//! Four filters keep a spurious pointer out. Two of them fire on retail and two
//! are guards that do not - the split is measured over the band and recorded on
//! the format page, so a reader knows which is evidence and which is caution:
//!
//! - the resolved address must land in the image with room for a header (drops
//!   the neighbours' records reached through the shared link base);
//! - the **call site** must lie inside a framed body of this image - a band
//!   image's tail is a byte-identical, same-offset copy of another image's
//!   bytes, and an inherited fragment's spawn calls name the sibling's
//!   records, not this image's;
//! - an intervening `jal` between the pair and the consuming call voids the
//!   value - `$a2` is caller-saved, so a pointer formed before another call is
//!   not the one the consumer sees;
//! - a target inside a framed function is dropped (a stale register the static
//!   window mis-read, not a record) - never fires on retail;
//! - a `model_sel` outside the set `FUN_80021B04` dispatches (`-1`, a library
//!   index below [`crate::summon_overlay::LIBRARY_MESH_SEL_MAX`], or the two
//!   render-mode sentinels) is dropped - never fires on retail.
//!
//! ## The one span this cannot bound
//!
//! The image's **highest** record has no next pointer above it, and the module
//! carries no length field, so its program's end is not derivable from the band
//! itself. [`records`] therefore stops at the highest record and reports it as
//! [`SlotBLayout::unbounded_record`] instead of claiming it. Everything below
//! it is bounded on both sides.
//!
//! Provenance: disassembly of the 64 extracted band images against
//! `crates/asset/data/static-overlays.toml`; the frame-matched partition
//! mirrors `ghidra/scripts/dump_static_overlay.py`, and reproduces its
//! committed `RANGES` rows.

use std::ops::Range;

use crate::summon_overlay::{
    LIBRARY_MESH_SEL_MAX, POOL_SPAWN_HELPER, RENDER_NODE_MODE_A, RENDER_NODE_MODE_B, SPAWN_HELPER,
    SUMMON_OVERLAY_LINK_BASE,
};

/// Link base every slot-B image is linked at (`*DAT_80010390`).
pub const SLOT_B_LINK_BASE: u32 = SUMMON_OVERLAY_LINK_BASE;

/// First extraction PROT entry of the module band.
pub const SLOT_B_PROT_FIRST: u32 = crate::cast_effect_pool::CAST_MODULE_PROT_FIRST;

/// Last extraction PROT entry of the module band.
pub const SLOT_B_PROT_LAST: u32 = crate::cast_effect_pool::CAST_MODULE_PROT_LAST;

/// Instructions of `$a2`-forming context scanned back from a spawn call. The
/// same window [`crate::summon_overlay::parse`] uses.
const A2_WINDOW_INSNS: usize = 22;

/// Widest head jump table in the band (PROT 0958 fills file `0x0..0x400`).
pub const MAX_HEAD_TABLE_WORDS: usize = 256;

/// `true` when `entry` is one of the 64 band entries.
pub fn is_slot_b_module(entry: u32) -> bool {
    (SLOT_B_PROT_FIRST..=SLOT_B_PROT_LAST).contains(&entry)
}

/// One frame-matched function body: `addiu sp, sp, -frame` through the first
/// `jr ra` whose delay slot restores the same `frame`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramedFn {
    /// File offset of the prologue.
    pub start: usize,
    /// File offset one past the `jr ra` delay slot.
    pub end: usize,
    /// Stack frame size the prologue reserves.
    pub frame: u16,
}

/// One spawn record, bounded on both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSpan {
    /// File offset of the record header (`model_sel`).
    pub start: usize,
    /// File offset the record ends at: the next record, or the next framed
    /// function's prologue, whichever comes first.
    pub end: usize,
    /// `record[+0]` mesh selector.
    pub model_sel: i16,
    /// `record[+2]`, the reserved halfword (see the format page:
    /// nothing reads it and it is zero in every band record).
    pub reserved: u16,
}

impl RecordSpan {
    /// Byte length of the record, header included.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// `true` when the record is header-only.
    pub fn is_empty(&self) -> bool {
        self.end <= self.start + 4
    }
}

/// One slot-B image, region by region.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotBLayout {
    /// Link base the offsets were resolved against.
    pub link_base: u32,
    /// Leading run of in-image VA words, if the image heads with one.
    pub head_table: Option<Range<usize>>,
    /// Frame-matched function partition, ascending.
    pub functions: Vec<FramedFn>,
    /// `jal` sites into either spawn helper, resolvable or not.
    pub spawn_sites: usize,
    /// Credited record offsets, ascending - including the highest one, which
    /// [`Self::records`] does not bound.
    pub record_offsets: Vec<usize>,
    /// The bounded record extents.
    pub records: Vec<RecordSpan>,
    /// The highest credited record offset, whose end the band cannot derive.
    pub unbounded_record: Option<usize>,
}

impl SlotBLayout {
    /// File offset one past the last framed function, or 0 for an image with
    /// none. This is the code/data boundary the frame scan puts where
    /// `scripts/ci/disc-coverage.py`'s `data_floor` independently puts it.
    pub fn code_end(&self) -> usize {
        self.functions.last().map_or(0, |f| f.end)
    }

    /// Bytes the bounded record extents cover.
    pub fn record_bytes(&self) -> usize {
        self.records.iter().map(RecordSpan::len).sum()
    }
}

fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// `jr ra`.
const MIPS_JR_RA: u32 = 0x03E0_0008;
/// `addiu $sp, $sp, imm` with the immediate's sign bit set.
const ADDIU_SP_MASK: u32 = 0xFFFF_0000;
const ADDIU_SP_NEG: u32 = 0x27BD_0000;

/// `jal <target>` word for a kseg0 address.
fn jal_word(addr: u32) -> u32 {
    0x0c00_0000 | ((addr >> 2) & 0x03ff_ffff)
}

/// Frame-matched function partition of a raw image.
///
/// Mirrors `ghidra/scripts/dump_static_overlay.py`: a body opens at
/// `addiu sp, sp, -F` and closes at the first `jr ra` whose delay slot is
/// `addiu sp, sp, +F` for the same `F`. Unlike a count-and-interleave rule this
/// survives a frameless leaf, an early `jr ra` inside a body, and a `jr ra`
/// word that is data in the image's tail.
pub fn framed_functions(bytes: &[u8]) -> Vec<FramedFn> {
    let n = bytes.len() / 4;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        let w = rd_u32(bytes, i * 4);
        if (w & ADDIU_SP_MASK) == ADDIU_SP_NEG && (w & 0x8000) != 0 {
            let frame = (0x1_0000u32 - (w & 0xFFFF)) as u16;
            let want = ADDIU_SP_NEG | u32::from(frame);
            let mut j = i + 1;
            while j + 1 < n {
                if rd_u32(bytes, j * 4) == MIPS_JR_RA && rd_u32(bytes, (j + 1) * 4) == want {
                    out.push(FramedFn {
                        start: i * 4,
                        end: (j + 2) * 4,
                        frame,
                    });
                    i = j + 1;
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
    out
}

/// Leading run of words that are VAs inside the image, capped at the first
/// framed function's prologue. `None` when word 0 is not such a VA.
fn head_table(bytes: &[u8], link_base: u32, functions: &[FramedFn]) -> Option<Range<usize>> {
    let cap = functions
        .first()
        .map_or(bytes.len(), |f| f.start)
        .min(MAX_HEAD_TABLE_WORDS * 4)
        .min(bytes.len());
    let hi = link_base as u64 + bytes.len() as u64;
    let mut k = 0usize;
    while k + 4 <= cap {
        let w = rd_u32(bytes, k);
        if w & 3 != 0 || u64::from(w) < u64::from(link_base) || u64::from(w) >= hi {
            break;
        }
        k += 4;
    }
    (k > 0).then_some(0..k)
}

/// Resolve the `$a2` a `jal` at word index `site` is handed, by walking the
/// `lui` / `addiu` writes over the preceding [`A2_WINDOW_INSNS`] instructions.
///
/// Returns `None` when `$a2` is last written by something the static window
/// cannot follow (a `move`, a load, a saved register) **or** when another `jal`
/// sits between the pair and this call: `$a2` is caller-saved, so a value
/// formed across a call is not the one the consumer reads.
fn resolve_a2(bytes: &[u8], site: usize) -> Option<u32> {
    let start = site.saturating_sub(A2_WINDOW_INSNS * 4);
    let mut a2: Option<u32> = None;
    let mut o = start;
    while o + 4 <= site {
        let w = rd_u32(bytes, o);
        let op = w >> 26;
        let rs = (w >> 21) & 31;
        let rt = (w >> 16) & 31;
        let imm = w & 0xffff;
        if op == 3 {
            a2 = None;
        } else if rt == 6 {
            match op {
                0x0f => a2 = Some(imm << 16),
                0x09 if rs == 6 => {
                    let s = if imm & 0x8000 != 0 {
                        imm as i32 - 0x1_0000
                    } else {
                        imm as i32
                    };
                    a2 = a2.map(|v| (v as i32).wrapping_add(s) as u32);
                }
                0x09 if rs == 0 => {
                    let s = if imm & 0x8000 != 0 {
                        imm as i32 - 0x1_0000
                    } else {
                        imm as i32
                    };
                    a2 = Some(s as u32);
                }
                _ => a2 = None,
            }
        }
        o += 4;
    }
    a2
}

/// `true` when `sel` is a value `FUN_80021B04` dispatches on.
fn dispatchable_model_sel(sel: i16) -> bool {
    sel == crate::summon_overlay::MODEL_SEL_TRANSFORM_NODE
        || (0..LIBRARY_MESH_SEL_MAX).contains(&sel)
        || sel == RENDER_NODE_MODE_A
        || sel == RENDER_NODE_MODE_B
}

/// Walk one slot-B image at the band's own link base.
pub fn parse(bytes: &[u8]) -> SlotBLayout {
    parse_at(bytes, SLOT_B_LINK_BASE)
}

/// Walk one slot-B image at an explicit link base.
///
/// `bytes` must be exactly one PROT entry
/// (`legaia_prot::archive::Archive::read_entry`). A window that over-reads into
/// the next entry resolves record pointers that belong to the neighbour's own
/// load at the shared base.
pub fn parse_at(bytes: &[u8], link_base: u32) -> SlotBLayout {
    let functions = framed_functions(bytes);
    let head_table = head_table(bytes, link_base, &functions);

    let spawn = jal_word(SPAWN_HELPER);
    let pooled = jal_word(POOL_SPAWN_HELPER);
    let mut spawn_sites = 0usize;
    let mut offsets: Vec<usize> = Vec::new();
    let mut o = 0usize;
    while o + 4 <= bytes.len() {
        let w = rd_u32(bytes, o);
        if w == spawn || w == pooled {
            spawn_sites += 1;
            // The call must be one this image's own code issues. A call word
            // outside every framed body here is an inherited fragment of a
            // sibling module's routine (a band image's tail is a same-offset
            // copy of another image's bytes), and the pointer it forms belongs
            // to that sibling's load, not to this image.
            let own_call = functions.iter().any(|f| f.start <= o && o < f.end);
            if let (true, Some(a2)) = (own_call, resolve_a2(bytes, o)) {
                let f = a2.wrapping_sub(link_base) as usize;
                if f + 4 <= bytes.len()
                    && !functions.iter().any(|fun| fun.start <= f && f < fun.end)
                    && dispatchable_model_sel(i16::from_le_bytes([bytes[f], bytes[f + 1]]))
                {
                    offsets.push(f);
                }
            }
        }
        o += 4;
    }
    offsets.sort_unstable();
    offsets.dedup();

    // Boundaries a record can end at: the next record, or the next framed
    // function's prologue. The image length closes the set so the arithmetic
    // is total, but the highest record is dropped rather than run to it - see
    // the module docs.
    let mut bounds: Vec<usize> = offsets.clone();
    bounds.extend(functions.iter().map(|f| f.start));
    bounds.push(bytes.len());
    bounds.sort_unstable();
    bounds.dedup();

    let mut records = Vec::new();
    for (i, &f) in offsets.iter().enumerate() {
        if i + 1 == offsets.len() {
            break;
        }
        let Some(&end) = bounds.iter().find(|&&x| x > f) else {
            continue;
        };
        records.push(RecordSpan {
            start: f,
            end,
            model_sel: i16::from_le_bytes([bytes[f], bytes[f + 1]]),
            reserved: u16::from_le_bytes([bytes[f + 2], bytes[f + 3]]),
        });
    }

    SlotBLayout {
        link_base,
        head_table,
        functions,
        spawn_sites,
        unbounded_record: offsets.last().copied(),
        record_offsets: offsets,
        records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(out: &mut Vec<u8>, word: u32) {
        out.extend_from_slice(&word.to_le_bytes());
    }

    fn word_of(b: &[u8], off: usize) -> u32 {
        u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
    }

    /// A synthetic image: one framed function that spawns two records, then
    /// the two records at the tail.
    fn synthetic() -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        // 0x00 addiu sp,sp,-0x18
        w(&mut b, 0x27BD_0000 | 0xFFE8);
        // 0x04 lui a2, 0x801f ; 0x08 addiu a2,a2,0x6a58 (= base + 0x80)
        w(&mut b, 0x3C06_801F);
        w(&mut b, 0x24C6_6A58);
        // 0x0c jal FUN_80021B04
        w(&mut b, jal_word(SPAWN_HELPER));
        w(&mut b, 0); // delay slot
        // 0x14 lui a2, 0x801f ; 0x18 addiu a2,a2,0x6a78 (= base + 0xa0)
        w(&mut b, 0x3C06_801F);
        w(&mut b, 0x24C6_6A78);
        // 0x1c jal FUN_80050ED4
        w(&mut b, jal_word(POOL_SPAWN_HELPER));
        w(&mut b, 0);
        // 0x24 jr ra ; 0x28 addiu sp,sp,0x18
        w(&mut b, MIPS_JR_RA);
        w(&mut b, 0x27BD_0018);
        while b.len() < 0xC0 {
            b.push(0);
        }
        // record A at 0x80: model_sel -1, reserved 0
        b[0x80] = 0xFF;
        b[0x81] = 0xFF;
        // record B at 0xa0: model_sel 3
        b[0xA0] = 0x03;
        b
    }

    #[test]
    fn frame_matching_finds_the_one_body() {
        let img = synthetic();
        let fns = framed_functions(&img);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].start, 0);
        assert_eq!(fns[0].end, 0x2C);
        assert_eq!(fns[0].frame, 0x18);
    }

    #[test]
    fn records_are_bounded_by_the_next_pointer() {
        let img = synthetic();
        let l = parse(&img);
        assert_eq!(l.spawn_sites, 2);
        assert_eq!(l.record_offsets, vec![0x80, 0xA0]);
        // The highest record is reported, not claimed.
        assert_eq!(l.unbounded_record, Some(0xA0));
        assert_eq!(l.records.len(), 1);
        assert_eq!(l.records[0].start, 0x80);
        assert_eq!(l.records[0].end, 0xA0);
        assert_eq!(l.records[0].model_sel, -1);
    }

    #[test]
    fn a_pointer_inside_a_function_is_dropped() {
        let mut img = synthetic();
        // Re-aim the first spawn at file 0x10, inside the framed body.
        img[0x08..0x0C].copy_from_slice(&0x24C6_69E8u32.to_le_bytes());
        let l = parse(&img);
        assert_eq!(l.spawn_sites, 2);
        assert_eq!(l.record_offsets, vec![0xA0]);
        assert!(l.records.is_empty());
    }

    #[test]
    fn an_intervening_call_voids_the_pointer() {
        let mut img = synthetic();
        // Slip an unrelated `jal` between the second pair and its consuming
        // call, overwriting the first call's delay slot with the pair's own
        // `lui` is not needed - the second pair is at 0x14/0x18, so putting a
        // call at 0x10 leaves the pair intact but a call at 0x1C would be the
        // consumer itself. Use the delay slot at 0x20 instead: move the pooled
        // call one word later and put the stray `jal` where it was.
        let pooled = word_of(&img, 0x1C);
        img[0x1C..0x20].copy_from_slice(&jal_word(0x8001_0000).to_le_bytes());
        img[0x20..0x24].copy_from_slice(&pooled.to_le_bytes());
        let l = parse(&img);
        // Site 0x20's `$a2` was formed before the stray call at 0x1C, so it is
        // not credited; the first record still is.
        assert_eq!(l.spawn_sites, 2);
        assert_eq!(l.record_offsets, vec![0x80]);
    }

    #[test]
    fn a_call_outside_every_framed_body_names_nothing() {
        // The inherited-tail shape: a sibling module's spawn call, copied into
        // this image's build buffer past the end of its own code, forms a real
        // pointer that belongs to the sibling's load.
        let mut img = synthetic();
        let at = img.len();
        assert_eq!(at, 0xC0);
        let mut tail: Vec<u8> = Vec::new();
        w(&mut tail, 0x3C06_801F);
        w(&mut tail, 0x24C6_6AB8); // = base + 0xE0, a real record below
        w(&mut tail, jal_word(SPAWN_HELPER));
        w(&mut tail, 0);
        img.extend_from_slice(&tail);
        img.resize(at + 0x40, 0);
        img[at + 0x20] = 0xFF; // model_sel -1 at 0xE0
        img[at + 0x21] = 0xFF;
        let l = parse(&img);
        assert_eq!(l.spawn_sites, 3);
        // Only the two calls inside the framed body name records.
        assert_eq!(l.record_offsets, vec![0x80, 0xA0]);
    }

    #[test]
    fn a_head_va_table_is_bounded_by_the_first_prologue() {
        let mut b: Vec<u8> = Vec::new();
        w(&mut b, SLOT_B_LINK_BASE + 0x10);
        w(&mut b, SLOT_B_LINK_BASE + 0x20);
        w(&mut b, 0x27BD_0000 | 0xFFE8);
        w(&mut b, MIPS_JR_RA);
        w(&mut b, 0x27BD_0018);
        while b.len() < 0x40 {
            b.push(0);
        }
        let l = parse(&b);
        // The run stops at the prologue, not at the first non-VA word.
        assert_eq!(l.head_table, Some(0..8));
        assert_eq!(l.functions.len(), 1);
        assert_eq!(l.functions[0].start, 8);
    }

    #[test]
    fn band_membership_is_the_dispatcher_band() {
        assert!(is_slot_b_module(903));
        assert!(is_slot_b_module(966));
        assert!(!is_slot_b_module(902));
        assert!(!is_slot_b_module(967));
    }
}
