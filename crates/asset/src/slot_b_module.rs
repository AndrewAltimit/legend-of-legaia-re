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
//! ## Bounding the highest record
//!
//! The image's highest record has no next pointer above it and the module
//! carries no length field, so the *band* cannot bound it. Its **program** can:
//! a record's payload is a move-VM program, and the move VM's own width table
//! ([`move_program_end`]) walks it to its terminator. Two words terminate a
//! program, and both of them mean "nothing above this ever executes":
//!
//! - `0x08` `HALT`, which sets `flags |= 8` and drops out of the tick loop; and
//! - an **armed idle loop** - `0x19` (or its `0x1B` mirror) whose paired `0x18`
//!   / `0x1A` loaded a counter with bit `0x4000` set. That bit makes the branch
//!   back to the saved PC unconditional, so the VM never advances past it.
//!
//! A third word bounds a record only where neither of those turns up: a `0x09`
//! `WAIT` carrying [`MOVE_WAIT_FOREVER`]. That is a layout argument rather than
//! a VM one - `WAIT` retires like any other instruction - and the band emits
//! the operand mid-program too, so the walk remembers the last one and returns
//! it only when it would otherwise have no bound at all.
//!
//! The end is then rounded up to a 4-byte boundary, because the records are
//! word-aligned: a program whose last halfword lands mid-word is followed by one
//! halfword of padding before the next record's header. That alignment step is
//! not cosmetic - it is the difference between reproducing a record's measured
//! extent and missing it by exactly 4 bytes.
//!
//! The rule is checked against the records the band *does* bound: chaining
//! `[header][program]` from each bounded record's start lands exactly on that
//! record's measured end for 1021 of the band's 1027 bounded extents, and none
//! of the chains overruns a measured end. The six that miss stall below it and
//! are a stated residue, not a rounding tolerance - see
//! [`slot-b-module-layout.md`](https://andrewaltimit.github.io/legend-of-legaia-re/formats/slot-b-module-layout.html).
//! Where the walk does **not** terminate, the record stays
//! [`SlotBLayout::unbounded_record`] and is claimed by nothing; on retail no
//! image is in that state.
//!
//! ## Inherited call sites
//!
//! The call-site filter above ("inside a framed body of this image") is
//! necessary but not sufficient: a module's inherited tail is a byte-identical
//! copy of a longer image's bytes, and a *whole function* of the donor can sit
//! in it, frame-matching locally and issuing the donor's spawn calls. Six of the
//! 64 images have such a site. [`parse_with_tail`] takes the image's own content
//! end and drops every call site and every record target at or above it; the
//! tail start comes from `scripts/ghidra-analysis/inherited_tail.py`, or from
//! [`content_end`] when only the one image is in hand.
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

/// `true` when `entry` is one of the 64 band entries - the cast / summon images
/// the three PROT 0898 entry tables reach.
pub fn is_slot_b_module(entry: u32) -> bool {
    (SLOT_B_PROT_FIRST..=SLOT_B_PROT_LAST).contains(&entry)
}

/// `true` when `entry` is a mapped image **linked at the slot-B base**.
///
/// The band above is an index range, and it answers "which images does the cast
/// dispatcher reach". This answers the different question the layout walk asks:
/// the three regions this module recovers - a head table of in-window VAs, the
/// frame-matched code partition, and a spawn-record band addressed by the
/// consumer's own `lui`/`addiu` - are properties of the **link base**, not of an
/// index range, because each is recovered by resolving words against
/// [`SLOT_B_LINK_BASE`]. Six mapped images sit at that base outside the band:
/// the two render occupants (PROT 0900 `summon_render`, 0901
/// `world_map_render`), the three battle stage / tutorial modules (0967 / 0968
/// / 0969) and the staged texture loader (0978 `field_back_read`). Selecting
/// this walk on the index band left every one of them measured as if it had no
/// head table, which is what kept 0967's 408-byte table of in-window VAs in the
/// residue.
pub fn is_slot_b_image(entry: u32) -> bool {
    crate::static_overlay::overlay_map()
        .by_prot_index(entry)
        .is_some_and(|r| r.base_va == SLOT_B_LINK_BASE)
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
    /// [`Self::records`] bounds only when its program terminates.
    pub record_offsets: Vec<usize>,
    /// The bounded record extents. Every start is an address the module's own
    /// code hands to a spawn helper in `$a2`.
    pub records: Vec<RecordSpan>,
    /// Records **above** the highest consumer-credited one, each found by
    /// chaining `[header][program]` from the end of the record below it. Their
    /// starts are computed by [`move_program_end`], not by a pointer, so they
    /// are kept apart from [`Self::records`].
    pub chained_records: Vec<RecordSpan>,
    /// The highest credited record offset, kept only when its program does not
    /// terminate - then nothing bounds it and nothing claims it.
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

// ---------------------------------------------------------------------------
// The move-VM program walk
// ---------------------------------------------------------------------------

/// Width in **halfwords** of every move-VM opcode `0x00..=0x46`, as the
/// dispatcher `FUN_80023070` sets its per-arm `param_3`. `0` marks the five
/// opcodes whose width is not a constant of the opcode alone, handled by name
/// in [`move_program_end`]: `0x08` (`HALT`, no advance), `0x0A`
/// (`3 + 3*count`), `0x0B` (the default break, no advance), `0x2F`
/// (`OVERLAY_EXT`, the sub-op's width), `0x3C` (`2 + count*6`) and `0x3D`
/// (`3 + count*6`).
///
/// Mirrors the widths in [`move-vm.md`](https://andrewaltimit.github.io/legend-of-legaia-re/subsystems/move-vm.html).
const MOVE_OP_HALFWORDS: [u8; 0x47] = [
    4, 4, 2, 2, 4, 4, 2, 4, // 0x00
    0, 2, 0, 0, 6, 2, 2, 2, // 0x08
    2, 2, 2, 16, 5, 2, 2, 2, // 0x10
    2, 1, 2, 1, 2, 2, 8, 8, // 0x18
    3, 7, 1, 13, 3, 2, 5, 3, // 0x20
    2, 2, 2, 4, 5, 4, 4, 0, // 0x28
    1, 2, 2, 1, 9, 3, 3, 3, // 0x30
    2, 4, 1, 1, 0, 0, 2, 2, // 0x38
    7, 2, 15, 1, 4, 8, 4, // 0x40
];

/// Width in halfwords of each `0x2F` `OVERLAY_EXT` sub-opcode `0x00..=0x3C`,
/// from the extension dispatcher's jump table (`FUN_801D362C`, JT
/// `0x801CE868`). Mirrors `move-vm-overlay-ext.md`.
const MOVE_EXT_HALFWORDS: [u8; 0x3D] = [
    16, 2, 2, 2, 3, 5, 7, 7, // 0x00
    2, 2, 3, 3, 3, 3, 11, 2, // 0x08
    2, 2, 8, 4, 4, 2, 2, 8, // 0x10
    5, 8, 8, 5, 3, 3, 4, 5, // 0x18
    5, 5, 5, 6, 8, 3, 3, 3, // 0x20
    5, 5, 8, 6, 7, 6, 13, 3, // 0x28
    5, 3, 3, 6, 3, 3, 4, 4, // 0x30
    4, 4, 3, 4, 6, // 0x38
];

/// `HALT`.
const MOVE_OP_HALT: u16 = 0x08;
/// `WAIT` - stall the actor for the operand's frame count.
const MOVE_OP_WAIT: u16 = 0x09;
/// The `WAIT` operand the band's authoring tool emits where a part is finished.
///
/// 4095 frames is a little over a minute, which no cast or summon part is on
/// screen for, so a part that reaches it never advances again in practice. It
/// is not a terminator the dispatcher knows about - `WAIT` retires like any
/// other instruction once its counter runs out - which is why the evidence for
/// reading it as one is the band's own layout rather than the VM.
const MOVE_WAIT_FOREVER: u16 = 0x0FFF;
/// `LOOP_SET` / `LOOP_SET_B` - arm a counter the matching `0x19` / `0x1B` tests.
const MOVE_OP_LOOP_SET_A: u16 = 0x18;
const MOVE_OP_LOOP_SET_B: u16 = 0x1A;
/// `LOOP_BACK` / `LOOP_BACK_B`.
const MOVE_OP_LOOP_BACK_A: u16 = 0x19;
const MOVE_OP_LOOP_BACK_B: u16 = 0x1B;
/// The counter bit that makes a `LOOP_BACK` unconditional - the branch is taken
/// forever and the program never advances past it.
const MOVE_LOOP_FOREVER: u16 = 0x4000;
/// Highest move-VM opcode; the dispatcher rejects anything above it.
const MOVE_OP_MAX: u16 = 0x46;
/// One past the highest `0x2F` sub-opcode.
const MOVE_EXT_MAX: u16 = 0x3D;
/// Guard on a walk that neither terminates nor advances off the end.
const MOVE_WALK_MAX_STEPS: usize = 4096;

/// How a move-VM program walk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramEnd {
    /// `0x08` `HALT`. The payload is the word-aligned byte offset one past it.
    Halt(usize),
    /// An armed `0x19` / `0x1B` idle loop. Same payload.
    IdleLoop(usize),
    /// A `0x09` `WAIT` carrying [`MOVE_WAIT_FOREVER`]. Same payload.
    WaitForever(usize),
    /// The walk ran into a halfword that is not a dispatchable opcode, or off
    /// the end of the buffer, without meeting a terminator. The payload is the
    /// offset it stopped at, which bounds nothing.
    Unterminated(usize),
}

impl ProgramEnd {
    /// The word-aligned end offset, for the two terminating outcomes.
    pub fn bounded(self) -> Option<usize> {
        match self {
            ProgramEnd::Halt(e) | ProgramEnd::IdleLoop(e) | ProgramEnd::WaitForever(e) => Some(e),
            ProgramEnd::Unterminated(_) => None,
        }
    }
}

fn rd_u16(b: &[u8], o: usize) -> Option<u16> {
    (o + 2 <= b.len()).then(|| u16::from_le_bytes([b[o], b[o + 1]]))
}

/// Walk the move-VM program that starts at byte offset `start`, returning where
/// it ends.
///
/// This is a **static** walk: `0x19` / `0x1B` with the loop bit clear retire
/// with width 1 rather than branching, and no `0x18` target is followed, so the
/// walk is a width sum over the instruction stream rather than an execution.
/// The one place control flow matters is the armed idle loop, which is a
/// terminator precisely because its branch is unconditional.
pub fn move_program_end(bytes: &[u8], start: usize) -> ProgramEnd {
    let mut pc = start;
    let mut loop_a = 0u16;
    let mut loop_b = 0u16;
    let mut last_forever_wait: Option<usize> = None;
    let stalled = |pc: usize, wait: Option<usize>| match wait {
        Some(w) => ProgramEnd::WaitForever(align4(w + 4)),
        None => ProgramEnd::Unterminated(pc),
    };
    for _ in 0..MOVE_WALK_MAX_STEPS {
        let Some(op) = rd_u16(bytes, pc) else {
            return stalled(pc, last_forever_wait);
        };
        if op > MOVE_OP_MAX {
            return stalled(pc, last_forever_wait);
        }
        let arg = |i: usize| rd_u16(bytes, pc + i * 2).unwrap_or(0);
        match op {
            MOVE_OP_HALT => return ProgramEnd::Halt(align4(pc + 2)),
            MOVE_OP_LOOP_SET_A => loop_a = arg(1),
            MOVE_OP_LOOP_SET_B => loop_b = arg(1),
            // An armed idle loop ends the program - UNLESS the author emitted
            // the record's `HALT` right behind it, which most of them do. The
            // `HALT` is then the end and the loop is one halfword of body
            // before it; taking the loop instead lands 4 bytes short.
            MOVE_OP_LOOP_BACK_A | MOVE_OP_LOOP_BACK_B
                if armed_idle_loop(op, loop_a, loop_b)
                    && rd_u16(bytes, pc + 2) != Some(MOVE_OP_HALT) =>
            {
                return ProgramEnd::IdleLoop(align4(pc + 2));
            }
            // A `WAIT` long enough that nothing runs after it. Remembered, not
            // returned: the band emits this operand mid-program too, and
            // ending there costs 48 of the extents the `HALT` rule reproduces
            // exactly. It is only a bound where the walk has no other one.
            MOVE_OP_WAIT if arg(1) == MOVE_WAIT_FOREVER => last_forever_wait = Some(pc),
            _ => {}
        }
        let halfwords = match op {
            // `KEYFRAME_LOAD`: a 3-halfword header then `count` 3-halfword
            // lanes, `count` in the header's second operand.
            0x0A => 3 + 3 * usize::from(arg(2)),
            // `OVERLAY_EXT`: the sub-op at +1 carries the width.
            0x2F => {
                let sub = arg(1);
                if sub >= MOVE_EXT_MAX {
                    return stalled(pc, last_forever_wait);
                }
                usize::from(MOVE_EXT_HALFWORDS[sub as usize])
            }
            // `SCRATCH_WRITE` / anim interpolate: `count` 6-halfword slots.
            0x3C => 2 + 6 * (arg(1) as i16).max(0) as usize,
            0x3D => 3 + 6 * (arg(2) as i16).max(0) as usize,
            _ => usize::from(MOVE_OP_HALFWORDS[op as usize]),
        };
        if halfwords == 0 {
            // `0x0B` - the dispatcher's default break sets no width, so the PC
            // does not move. Statically that is a stall, not an end.
            return stalled(pc, last_forever_wait);
        }
        pc += halfwords * 2;
    }
    stalled(pc, last_forever_wait)
}

fn align4(x: usize) -> usize {
    (x + 3) & !3
}

/// `true` when `op` is a `LOOP_BACK` whose paired counter carries the
/// unconditional-branch bit, so the VM never advances past it.
fn armed_idle_loop(op: u16, loop_a: u16, loop_b: u16) -> bool {
    let counter = match op {
        MOVE_OP_LOOP_BACK_A => loop_a,
        MOVE_OP_LOOP_BACK_B => loop_b,
        _ => return false,
    };
    counter & MOVE_LOOP_FOREVER != 0
}

/// `true` when the eight bytes at `p` are zero - the module's padding between
/// its last record and whatever the packer's buffer left above it, never a
/// record.
///
/// A zero header is a legal `[model_sel 0][reserved 0]` and a zero halfword is
/// a legal opcode `0x00`, so the chain would otherwise walk the padding as a
/// four-halfword record and run on into the inherited tail. The evidence is the
/// band's own pointers: no pointer-credited record on the disc opens with eight
/// zero bytes, and every chained one that did sat in that gap. PROT 0944 is the
/// worked case - its top record ends at `0x1988`, the padding runs to `0x199C`,
/// and from there the bytes are PROT 0942's records at the same file offsets.
fn is_record_padding(bytes: &[u8], p: usize) -> bool {
    bytes
        .get(p..p + 8)
        .is_some_and(|w| w.iter().all(|&b| b == 0))
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
    parse_with_tail(bytes, link_base, None)
}

/// Structural end of this image's **own** content, in file bytes.
///
/// The top of the spawn-record chain when the image has records, and the end of
/// the frame-matched code partition otherwise. Everything above is either the
/// module's trailing padding or a longer image's residue - and this is the
/// figure that decides which of two equal-extent siblings owns a shared suffix
/// (`scripts/ghidra-analysis/inherited_tail.py`).
///
/// It is deliberately *not* the frame partition alone: a donor's whole function
/// can sit in the tail and frame-match there, so on PROT 0949 the code
/// partition reaches `0x1B8C` while the image's own content stops at `0x1828`.
///
/// This walks whatever slice it is handed, and the slice matters: over a whole
/// image the chain runs on into the donor's residue and the figure overshoots.
/// Measured over the band, handing it the uncut image moves the figure on **10
/// of 83** images (PROT 0908 / 0910 / 0919 / 0920 / 0932 / 0943 / 0960 / 0961,
/// plus the slot-A pair 0974 / 0980, whose figure is the frame partition rather
/// than a record chain). On five of those - 0908 / 0910 / 0920 / 0943 / 0961 -
/// the overshoot also credits a spawn pointer that belongs to the donor.
///
/// The cut and this measurement are mutually recursive, so the band-level
/// caller iterates them rather than taking the first estimate: see
/// `scripts/ghidra-analysis/inherited_tail.py`'s `tail_starts_fixpoint`, and
/// `crates/asset/tests/slot_b_record_bounds_real.rs` for the same loop on this
/// side. It settles in two rounds and moves no tail cut - the asymmetry this
/// comment used to argue for (an overshoot can only make an image look less
/// like a recipient, never invent a tail) is now measured rather than asserted.
pub fn content_end(bytes: &[u8], link_base: u32) -> usize {
    let layout = parse_at(bytes, link_base);
    let chained = layout.chained_records.last().map(|r| r.end);
    let credited = layout.records.last().map(|r| r.end);
    chained
        .or(credited)
        .or(layout.unbounded_record)
        .unwrap_or_else(|| layout.code_end())
}

/// Walk one slot-B image, dropping everything at or above `tail_start`.
///
/// `tail_start` is the file offset at which the image stops being its own
/// content. A spawn call site there belongs to the donor whose bytes those are,
/// and so does the record pointer it forms - see the module docs. `None` runs
/// the parse with no tail known, which is what [`parse_at`] does.
pub fn parse_with_tail(bytes: &[u8], link_base: u32, tail_start: Option<usize>) -> SlotBLayout {
    let limit = tail_start.unwrap_or(bytes.len()).min(bytes.len());
    let functions = framed_functions(bytes);
    let head_table = head_table(bytes, link_base, &functions);

    let spawn = jal_word(SPAWN_HELPER);
    let pooled = jal_word(POOL_SPAWN_HELPER);
    let mut spawn_sites = 0usize;
    let mut offsets: Vec<usize> = Vec::new();
    let mut o = 0usize;
    while o + 4 <= limit {
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
                if f + 4 <= limit
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
    // function's prologue. The image's own content end closes the set so the
    // arithmetic is total.
    let mut bounds: Vec<usize> = offsets.clone();
    bounds.extend(functions.iter().map(|f| f.start).filter(|&s| s <= limit));
    bounds.push(limit);
    bounds.sort_unstable();
    bounds.dedup();

    let span_at = |f: usize, end: usize| RecordSpan {
        start: f,
        end,
        model_sel: i16::from_le_bytes([bytes[f], bytes[f + 1]]),
        reserved: u16::from_le_bytes([bytes[f + 2], bytes[f + 3]]),
    };

    let mut records = Vec::new();
    let mut chained = Vec::new();
    let mut unbounded = None;
    for (i, &f) in offsets.iter().enumerate() {
        let next_bound = bounds.iter().find(|&&x| x > f).copied();
        if i + 1 < offsets.len() {
            // Bounded below the top: the next consumer pointer (or the next
            // function, whichever comes first) closes it.
            if let Some(end) = next_bound {
                records.push(span_at(f, end));
            }
            continue;
        }
        // The top record. Nothing above computes an address, so its end comes
        // from its own program - and the chain above it keeps going while the
        // bytes keep reading as `[header][program]`.
        let cap = next_bound.unwrap_or(limit);
        match move_program_end(bytes, f + 4).bounded() {
            Some(end) if end > f && end <= cap => {
                records.push(span_at(f, end));
                let mut p = end;
                while p + 4 <= cap
                    && !is_record_padding(bytes, p)
                    && dispatchable_model_sel(i16::from_le_bytes([bytes[p], bytes[p + 1]]))
                {
                    match move_program_end(bytes, p + 4).bounded() {
                        Some(q) if q > p && q <= cap => {
                            chained.push(span_at(p, q));
                            p = q;
                        }
                        _ => break,
                    }
                }
            }
            _ => unbounded = Some(f),
        }
    }

    SlotBLayout {
        link_base,
        head_table,
        functions,
        spawn_sites,
        unbounded_record: unbounded,
        record_offsets: offsets,
        records,
        chained_records: chained,
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
