//! The **presentation calls** a slot-B cast module's tick arms make into the
//! shared helpers - screen fades, the armed wash, the 0898 colour wash, the
//! streak ribbon, CLUT copies - decoded off the module image as data.
//!
//! Every module in the band (`PROT 0903..=0966`) runs its choreography as a
//! phase machine on `ctx[+0x279]`, and most of what it puts on screen besides
//! its spawn records goes through a handful of shared routines:
//!
//! | helper | call | what an arm hands it |
//! |---|---|---|
//! | `FUN_80024E80` | `(0x801C9070, id)` | a 13-halfword fade template the arm writes into the block at `0x801C9070` just before the call |
//! | `FUN_8004695C` | `(rgb)` | a 24-bit colour, armed for one frame (`gp+0x9D0`) |
//! | `FUN_801E1D98` | `(&vec3, trail)` | the chained streak ribbon's anchor and trail id |
//! | `FUN_80058490` | `(&rect, x, y)` | a `MoveImage` whose rect the arm builds at the packet cursor `*(0x1F8003A0)` |
//! | `FUN_801F2410` (0898) | none - the dispatchers' tail | the arm stores the wash parameters into `ctx[+0x27A..+0x286]` / `ctx[+0x32A..+0x32C]` |
//! | `FUN_80050120` | none - the battle frame's ambient pass | the arm stores the stage byte `ctx[+0x278]`, which darkens the backdrop pair and takes the ground grid off the screen |
//! | `FUN_801D8DE8` (0898) | `(element, mode)` | a battle HUD element raised or unloaded; for the message bar `0x5B`, the line pointer the arm stores into `0x800774AC` first |
//!
//! Each of those is a call (or a store) whose arguments are literals the arm
//! computes in a few straight-line instructions, so the wiring is **data**:
//! [`decode`] finds every site, places it on the phase arm that reaches it
//! and recovers its arguments with a small constant tracker. A host fires
//! each site from one generic hook on the tick its arm runs, instead of
//! carrying 258 hand-ported fade calls in 52 tick bodies.
//!
//! ## Placing a site
//!
//! A tick body opens by loading the phase byte and switching on it - through
//! a jump table bounded by `sltiu` (most bodies) or a `beq` / `slti` chain
//! (PROT 0919 and a few more). [`decode`] walks that switch with the phase's
//! value range and recovers each arm's entry, then walks each arm's control
//! flow (stopping at the other arms' entries) to the helper sites it reaches.
//! A site in a routine the arm calls (`jal` to an own-image function) inherits
//! the call's arm.
//!
//! Where in the arm a site sits decides when it fires ([`SiteTiming`]):
//! whether it runs on the pass that moves the phase (`sb .., 0x279(ctx)`),
//! on the passes that hold, or on both. That is a property of the arm's
//! control flow and is computed, not guessed.
//!
//! ## What the tracker sees
//!
//! A straight-line constant propagation from the arm's entry to the site
//! (branches are walked in address order, not followed), seeded with the
//! function prologue's stack stores (the busy word `sp+0x38 = 1` several
//! modules store as the fade kind). Loads of the scratchpad frame step
//! `0x1F800393` and speed scalar `0x1F80037D` read as `1` and the normal
//! scalar - the values an engine tick sees. A value the tracker cannot fold
//! stays `None`, and a host fires only a fully resolved site.
//!
//! Provenance: the instruction stream of each image at slot-B base
//! `0x801F69D8`; the helper contracts are `see ghidra/scripts/funcs/80024e80.txt`,
//! `8004695c.txt`, `overlay_battle_action_801e1d98.txt` and
//! `overlay_muscle_dome_801f2410.txt` (PROT 0898's colour wash, dumped under
//! that prefix).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::slot_b_module::{FramedFn, SLOT_B_LINK_BASE, framed_functions};

/// `FUN_80024E80` - the screen-fade actor spawn.
pub const FADE_SPAWN: u32 = 0x8002_4E80;
/// `FUN_8004695C` - arm the one-frame subtractive screen wash.
pub const ARMED_WASH: u32 = 0x8004_695C;
/// `FUN_801E1D98` (PROT 0898) - the chained streak ribbon.
pub const STREAK_RIBBON: u32 = 0x801E_1D98;
/// `FUN_80058490` - `MoveImage`.
pub const MOVE_IMAGE: u32 = 0x8005_8490;
/// `FUN_800583C8` - `LoadImage`.
pub const LOAD_IMAGE: u32 = 0x8005_83C8;
/// `FUN_80050E74` - the part-pool flush: every non-null word of the pool at
/// [`PART_POOL`] gets the actor-list done bit (`+0x10 |= 8`) and is zeroed
/// (`0x80050E80..0x80050EC8`).
pub const PART_POOL_FLUSH: u32 = 0x8005_0E74;
/// The part pool `FUN_80050ED4` seats into (its first `0x60` words) and
/// [`PART_POOL_FLUSH`] walks (all [`PART_POOL_WORDS`]).
pub const PART_POOL: u32 = 0x801C_90F0;
/// Words the flush walks (`slti v0,a1,0x80` at `0x80050EC0`).
pub const PART_POOL_WORDS: u32 = 0x80;
/// `FUN_80050ED4` - the pool wrapper over the part spawn `FUN_80021B04`: it
/// parks the part in the first free word of [`PART_POOL`], which is how a
/// later [`PART_POOL_FLUSH`] reaches it.
pub const PART_POOL_SPAWN: u32 = 0x8005_0ED4;
/// `FUN_801D8DE8` (PROT 0898) - the battle HUD's screen-element spawner,
/// `(element, mode)`: mode bit 0 clear raises the element on its glide from
/// seat A, set unloads it.
pub const HUD_ELEMENT: u32 = 0x801D_8DE8;
/// The content word of placement record 91 (HUD element `0x5B`, the
/// full-width message bar): the text pointer an arm stores before raising
/// the bar.
pub const MESSAGE_BAR_TEXT: u32 = 0x8007_74AC;
/// HUD element `0x5B`, the full-width message bar.
pub const MESSAGE_BAR_ELEMENT: u8 = 0x5B;

/// The fade template block every module arm writes before `FUN_80024E80`.
pub const FADE_TEMPLATE_BLOCK: u32 = 0x801C_9070;
/// Halfwords in a fade template.
pub const FADE_TEMPLATE_WORDS: usize = 13;
/// The battle context pointer `*(0x8007BD24)`.
pub const BATTLE_CTX_PTR: u32 = 0x8007_BD24;
/// The module phase byte `ctx[+0x279]`.
pub const PHASE_OFFSET: u16 = 0x279;
/// The stage byte `ctx[+0x278]`: the battle frame's ambient pass
/// `FUN_80050120` reads it to drive the backdrop pair to black (`>= 2`) and to
/// freeze the ambient with the ground grid off (bit 0).
pub const STAGE_BYTE_OFFSET: u16 = 0x278;
/// First and last `ctx` byte of the colour-wash parameter block
/// (`ctx[+0x27A]` alpha .. `ctx[+0x286]`).
pub const WASH_FIELDS: std::ops::RangeInclusive<u16> = 0x27A..=0x286;
/// The colour wash's scroll halfword and its per-frame rate.
pub const WASH_SCROLL: u16 = 0x32A;
/// See [`WASH_SCROLL`].
pub const WASH_SCROLL_RATE: u16 = 0x32C;
/// The scratchpad packet cursor `*(0x1F8003A0)` (`0x1F800314 + 0x8C`).
pub const PACKET_CURSOR: u32 = 0x1F80_03A0;
/// Scratchpad frame step `*(0x1F800393)`: one vsync per engine tick.
pub const FRAME_STEP: u32 = 0x1F80_0393;
/// Scratchpad speed scalar `*(0x1F80037D)`.
pub const SPEED_SCALAR_ADDR: u32 = 0x1F80_037D;
/// The value an engine tick reads at [`SPEED_SCALAR_ADDR`] (the battle's
/// normal animation rate).
pub const SPEED_SCALAR: i32 = 8;

/// When a placed site runs relative to its arm's gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteTiming {
    /// On every pass the arm runs, held or not (the site sits before the
    /// gate, or the arm has no phase write of its own).
    Every,
    /// Only on the passes that hold (a path to the exit with no phase write).
    Hold,
    /// Only on the pass that moves the phase.
    Pass,
}

/// One fade template as the arm left the block at `0x801C9070`, halfword by
/// halfword (`None` where the tracker could not fold the store), plus the id
/// `a1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeCall {
    pub template: [Option<i16>; FADE_TEMPLATE_WORDS],
    pub id: Option<i16>,
}

impl FadeCall {
    /// The fields a fade actor reads, when all of them resolved: kind, ramp
    /// duration, start / end RGB, start delay and hold
    /// (`[0]`, `[1]`, `[3..=5]`, `[7..=9]`, `[10]`, `[11]`) and the id.
    pub fn resolved(&self) -> Option<ResolvedFade> {
        let t = &self.template;
        Some(ResolvedFade {
            kind: t[0]?,
            duration: t[1]?,
            start_rgb: [t[3]?, t[4]?, t[5]?],
            end_rgb: [t[7]?, t[8]?, t[9]?],
            delay: t[10]?,
            hold: t[11]?,
            id: self.id?,
        })
    }
}

/// A fully resolved [`FadeCall`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedFade {
    pub kind: i16,
    pub duration: i16,
    pub start_rgb: [i16; 3],
    pub end_rgb: [i16; 3],
    pub delay: i16,
    pub hold: i16,
    pub id: i16,
}

/// A store into the colour-wash parameter block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WashStore {
    /// `ctx[field] = value`.
    Set(i32),
    /// `ctx[field] += delta` (the alpha ramps): the delta already folded at
    /// one vsync and the normal scalar.
    Add(i32),
    /// A literal store on the branch a bound test took: the ramp's cap (or
    /// floor) - `if (alpha > 0x80) alpha = 0x80`.
    Clamp(i32),
}

/// Where a module keeps a fade actor's pointer so a later arm can kill it:
/// a battle-context word (`ctx[+0x102C]`, the usual slot) or a word of the
/// module's own image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HandleSlot {
    Ctx(u16),
    Abs(u32),
}

impl HandleSlot {
    /// Whether the slot is a word of the part pool [`PART_POOL_FLUSH`]
    /// walks. The allocator seats into the first `0x60` only, but a module
    /// may park an actor of its own in the tail: PROT 0930 (Horn) and 0931
    /// (Jedo) park their held fades in the last word, `0x801C92EC`, and the
    /// next flush halts them with the parts.
    pub fn in_part_pool(self) -> bool {
        matches!(self, Self::Abs(a) if (PART_POOL..PART_POOL + 4 * PART_POOL_WORDS).contains(&a) && a % 4 == 0)
    }
}

/// What one site does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationCall {
    /// `FUN_80024E80(0x801C9070, id)`.
    Fade(FadeCall),
    /// `FUN_8004695C(rgb)`.
    ArmedWash { rgb: Option<u32> },
    /// `FUN_801E1D98(a0, trail)`: the anchor `a0` resolved only as a stack
    /// trio offset, the trio's halfwords where they folded.
    Streak {
        anchor: Option<[Option<i16>; 3]>,
        trail: Option<u8>,
    },
    /// `FUN_80058490(&rect, x, y)`: rect `(x, y, w, h)` and destination.
    VramMove {
        rect: [Option<i16>; 4],
        dst: [Option<i16>; 2],
    },
    /// `FUN_800583C8(&rect, src)`: the rect only.
    VramLoad { rect: [Option<i16>; 4] },
    /// `FUN_80050E74()`: the part-pool flush, which halts every actor parked
    /// in the pool's words - a fade a module parked there included
    /// ([`HandleSlot::in_part_pool`]).
    PoolFlush,
    /// `FUN_80050ED4(pos, rot, record, scale)`: a part seated through the
    /// pool wrapper, `record` the spawn record (`a2`) when it folded. The
    /// parts a [`Self::PoolFlush`] halts are these.
    PoolSpawn { record: Option<u32> },
    /// The fade the `FUN_80024E80` call at `fade` spawned is parked in
    /// `slot` (`sw v0, 0x102C(ctx)` after the call).
    FadeStore { fade: u32, slot: HandleSlot },
    /// The actor parked in `slot` is killed (`lw; lw +0x10; ori 0x8; sw`
    /// on its flag word - the done bit the actor list drops it on).
    ActorKill { slot: HandleSlot },
    /// The effect part a `FUN_80021B04` / `FUN_80050ED4` call seated off
    /// spawn record `record` is parked in `slot` (`sw v0, slot`), the handle
    /// a later [`Self::ActorKill`] of that slot halts it by.
    PartStore { record: u32, slot: HandleSlot },
    /// A store into the colour-wash block (`ctx[+0x27A..+0x286]`,
    /// `ctx[+0x32A]`, `ctx[+0x32C]`), by field offset and width in bytes.
    WashField {
        field: u16,
        width: u8,
        store: Option<WashStore>,
    },
    /// `sb rt, 0x278(ctx)`: the stage byte, the literal when it folded.
    StageByte { value: Option<u8> },
    /// `FUN_801D8DE8(element, mode)`: a battle HUD element raised (mode bit 0
    /// clear) or unloaded. `text` is the word the arm stored into the
    /// message bar's content word [`MESSAGE_BAR_TEXT`] on its way to the call,
    /// when it folded - for element [`MESSAGE_BAR_ELEMENT`], the VA of the
    /// line the bar shows.
    HudElement {
        element: Option<u8>,
        mode: Option<u8>,
        text: Option<u32>,
        /// The phase the code after the call stores ([`site_exit_phase`]):
        /// which way out of its arm the raise sits on, where an arm forks.
        exit: Option<PhaseExit>,
    },
}

/// The phase a straight-line run stores into `ctx[+0x279]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseExit {
    /// The arm's own phase plus `k` (`lbu; addiu k; sb`).
    Rel(i32),
    /// A literal.
    Abs(u8),
}

impl PhaseExit {
    /// The phase this exit stores from arm `phase_in`.
    pub fn from_arm(self, phase_in: u8) -> u8 {
        match self {
            Self::Rel(k) => (i32::from(phase_in) + k) as u8,
            Self::Abs(v) => v,
        }
    }
}

/// One decoded site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationSite {
    /// VA of the `jal` (or the store).
    pub va: u32,
    /// The phase-dispatching function the site's arm belongs to.
    pub body: u32,
    /// The phase arm.
    pub arm: u8,
    pub timing: SiteTiming,
    pub call: PresentationCall,
}

/// A helper site the decoder found but could not place on a phase arm (a
/// stager arm, a routine nothing in a phase arm calls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnplacedSite {
    pub va: u32,
    pub call: PresentationCall,
}

/// One tick body's phase switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseDispatch {
    /// Entry VA of the dispatching function.
    pub body: u32,
    /// VA of the phase load the switch opens on.
    pub load: u32,
    /// Arm entry VA by phase value.
    pub arms: BTreeMap<u8, u32>,
    /// The arms that can **hold**: arms that write the phase on some path
    /// and return without writing it on another (a countdown or poll gate).
    /// An arm with no phase write (a final arm) is not in it.
    pub holding: BTreeSet<u8>,
}

/// Everything [`decode`] recovers from one image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CastPresentation {
    pub dispatches: Vec<PhaseDispatch>,
    pub sites: Vec<PresentationSite>,
    pub unplaced: Vec<UnplacedSite>,
}

impl CastPresentation {
    /// The sites of `body`'s arm `arm`, in address order. `body = None`
    /// matches an image with a single phase switch.
    pub fn arm_sites(&self, body: Option<u32>, arm: u8) -> impl Iterator<Item = &PresentationSite> {
        let body = body.or_else(|| (self.dispatches.len() == 1).then(|| self.dispatches[0].body));
        self.sites
            .iter()
            .filter(move |s| Some(s.body) == body && s.arm == arm)
    }

    /// Whether `body`'s arm `arm` can hold ([`PhaseDispatch::holding`]).
    /// `body = None` matches an image with a single phase switch.
    pub fn arm_holds(&self, body: Option<u32>, arm: u8) -> bool {
        let d = match body {
            Some(b) => self.dispatches.iter().find(|d| d.body == b),
            None => (self.dispatches.len() == 1).then(|| &self.dispatches[0]),
        };
        d.is_some_and(|d| d.holding.contains(&arm))
    }

    /// Every spawn record the image seats through the pool wrapper
    /// ([`PresentationCall::PoolSpawn`]), placed on an arm or not: the parts
    /// a [`PresentationCall::PoolFlush`] reaches.
    pub fn pool_records(&self) -> BTreeSet<u32> {
        let placed = self.sites.iter().map(|s| s.call);
        let unplaced = self.unplaced.iter().map(|s| s.call);
        placed
            .chain(unplaced)
            .filter_map(|c| match c {
                PresentationCall::PoolSpawn { record } => record,
                _ => None,
            })
            .collect()
    }

    /// Whether `body` is one of this image's phase switches.
    pub fn has_body(&self, body: u32) -> bool {
        self.dispatches.iter().any(|d| d.body == body)
    }
}

// ---------------------------------------------------------------------------
// MIPS fields
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Ins {
    w: u32,
    va: u32,
}

impl Ins {
    fn op(&self) -> u32 {
        self.w >> 26
    }
    fn rs(&self) -> usize {
        ((self.w >> 21) & 31) as usize
    }
    fn rt(&self) -> usize {
        ((self.w >> 16) & 31) as usize
    }
    fn rd(&self) -> usize {
        ((self.w >> 11) & 31) as usize
    }
    fn sa(&self) -> u32 {
        (self.w >> 6) & 31
    }
    fn funct(&self) -> u32 {
        self.w & 0x3F
    }
    fn imm(&self) -> u16 {
        self.w as u16
    }
    fn simm(&self) -> i32 {
        i32::from(self.w as u16 as i16)
    }
    fn branch_target(&self) -> u32 {
        self.va
            .wrapping_add(4)
            .wrapping_add((self.simm() << 2) as u32)
    }
    fn jump_target(&self) -> u32 {
        (self.va.wrapping_add(4) & 0xF000_0000) | ((self.w & 0x03FF_FFFF) << 2)
    }
    /// A conditional branch (no link).
    fn is_branch(&self) -> bool {
        match self.op() {
            0x04..=0x07 | 0x14..=0x17 => true,
            0x01 => matches!(self.rt(), 0x00..=0x03),
            _ => false,
        }
    }
    fn is_j(&self) -> bool {
        self.op() == 0x02
    }
    fn is_jal(&self) -> bool {
        self.op() == 0x03
    }
    fn is_jr(&self) -> bool {
        self.op() == 0 && self.funct() == 0x08
    }
    fn is_jalr(&self) -> bool {
        self.op() == 0 && self.funct() == 0x09
    }
    /// Register written, if any.
    fn dest(&self) -> Option<usize> {
        let r = match self.op() {
            0x00 => match self.funct() {
                0x08 | 0x18..=0x1B | 0x11 | 0x13 | 0x0C | 0x0D => return None,
                0x09 => self.rd(),
                _ => self.rd(),
            },
            0x03 => 31,
            0x08..=0x0F | 0x20..=0x26 => self.rt(),
            0x10 | 0x12 => match self.rs() {
                0x00 | 0x02 => self.rt(),
                _ => return None,
            },
            _ => return None,
        };
        (r != 0).then_some(r)
    }
}

fn word(bytes: &[u8], off: usize) -> Option<u32> {
    bytes
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

struct Image<'a> {
    bytes: &'a [u8],
    base: u32,
}

impl Image<'_> {
    fn ins(&self, va: u32) -> Option<Ins> {
        let off = va.checked_sub(self.base)? as usize;
        word(self.bytes, off).map(|w| Ins { w, va })
    }
    fn contains(&self, va: u32) -> bool {
        va >= self.base && ((va - self.base) as usize) + 4 <= self.bytes.len()
    }
}

// ---------------------------------------------------------------------------
// Phase-switch walk
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DV {
    Unk,
    Const(u32),
    /// `phase + k`.
    Phase(i32),
    /// `(phase + k) << s`.
    PhaseShl(i32, u32),
    /// `table + ((phase + k) << 2)`.
    PhaseAddr(i32, u32),
    /// `(phase + k) < bound` (signed or not), as a 0/1.
    Less(i32, i32),
}

/// What one instruction is to the switch walk.
enum Step {
    /// A compare / constant / address step the switch is made of.
    Dispatch,
    /// Anything else: arm code.
    Other,
    /// A jump-table read feeding the `jr` after it: the table VA and the
    /// phase offset its index carries.
    Table(u32, i32),
}

fn dv_eval(img: &Image, i: &Ins, r: &mut [DV; 32]) -> Step {
    let rs = r[i.rs()];
    let rt = r[i.rt()];
    let mut step = Step::Dispatch;
    let out = match i.op() {
        0x0F => DV::Const(u32::from(i.imm()) << 16),
        0x09 => match rs {
            DV::Const(c) => DV::Const(c.wrapping_add(i.simm() as u32)),
            DV::Phase(k) => DV::Phase(k + i.simm()),
            _ => {
                step = Step::Other;
                DV::Unk
            }
        },
        0x0D => match rs {
            DV::Const(c) => DV::Const(c | u32::from(i.imm())),
            _ => {
                step = Step::Other;
                DV::Unk
            }
        },
        0x0A | 0x0B => match rs {
            DV::Phase(k) => DV::Less(k, i.simm()),
            _ => {
                step = Step::Other;
                DV::Unk
            }
        },
        0x23 => {
            if let DV::PhaseAddr(k, base) = rs {
                // `lw rt, lo(addr)` then `jr rt`, load-delay `nop`s between.
                for gap in 1..=3u32 {
                    let Some(n) = img.ins(i.va + 4 * gap) else {
                        break;
                    };
                    if n.is_jr() && n.rs() == i.rt() {
                        return Step::Table(base.wrapping_add(i.simm() as u32), k);
                    }
                    if n.w != 0 {
                        break;
                    }
                }
            }
            step = Step::Other;
            DV::Unk
        }
        0x00 => match i.funct() {
            0x00 if i.w != 0 => match rt {
                DV::Phase(k) => DV::PhaseShl(k, i.sa()),
                DV::Const(c) => DV::Const(c << i.sa()),
                _ => {
                    step = Step::Other;
                    DV::Unk
                }
            },
            0x00 => return Step::Dispatch, // nop
            0x2A | 0x2B => match (rs, rt) {
                (DV::Phase(k), DV::Const(c)) => DV::Less(k, c as i32),
                _ => {
                    step = Step::Other;
                    DV::Unk
                }
            },
            0x21 | 0x25 => match (rs, rt) {
                (DV::PhaseShl(k, 2), DV::Const(c)) | (DV::Const(c), DV::PhaseShl(k, 2)) => {
                    DV::PhaseAddr(k, c)
                }
                (x, DV::Const(0)) | (DV::Const(0), x) => x,
                _ => {
                    step = Step::Other;
                    DV::Unk
                }
            },
            _ => {
                step = Step::Other;
                DV::Unk
            }
        },
        _ => {
            step = Step::Other;
            DV::Unk
        }
    };
    if let Some(d) = i.dest() {
        r[d] = out;
    }
    step
}

/// A set of phase values `0..=255`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PSet([u64; 4]);

impl PSet {
    const ALL: Self = Self([u64::MAX; 4]);
    fn range(lo: i32, hi: i32) -> Self {
        let mut s = Self([0; 4]);
        for v in lo.max(0)..=hi.min(255) {
            s.0[(v / 64) as usize] |= 1 << (v % 64);
        }
        s
    }
    fn and(self, o: Self) -> Self {
        Self([0, 1, 2, 3].map(|k| self.0[k] & o.0[k]))
    }
    fn minus(self, o: Self) -> Self {
        Self([0, 1, 2, 3].map(|k| self.0[k] & !o.0[k]))
    }
    fn count(self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }
    fn values(self) -> impl Iterator<Item = u8> {
        (0..=255u8).filter(move |&v| self.0[(v / 64) as usize] & (1 << (v % 64)) != 0)
    }
}

/// Walk one phase switch from the load at `load`, whose value lands in
/// `reg`, with `seed` the constants the prologue left in registers. Returns
/// the arm entries by phase value.
fn walk_switch(
    img: &Image,
    load: u32,
    reg: usize,
    seed: [DV; 32],
    fn_end: u32,
) -> BTreeMap<u8, u32> {
    let mut arms = BTreeMap::new();
    let mut regs = seed;
    regs[0] = DV::Const(0);
    regs[reg] = DV::Phase(0);
    type Path = (u32, [DV; 32], PSet, bool);
    let mut stack: Vec<Path> = vec![(load + 4, regs, PSet::ALL, false)];
    let mut seen: BTreeSet<(u32, PSet)> = BTreeSet::new();
    let mut steps = 0;
    let mark = |arms: &mut BTreeMap<u8, u32>, set: PSet, pc: u32| {
        if set.count() <= 4 {
            for v in set.values() {
                arms.entry(v).or_insert(pc);
            }
        }
    };
    while let Some((mut pc, mut r, set, branched)) = stack.pop() {
        if set.count() == 0 || !seen.insert((pc, set)) {
            continue;
        }
        loop {
            steps += 1;
            if steps > 8000 || pc >= fn_end {
                break;
            }
            if branched && set.count() == 1 {
                mark(&mut arms, set, pc);
                break;
            }
            let Some(i) = img.ins(pc) else { break };
            if i.is_branch() {
                let (a, b) = (r[i.rs()], r[i.rt()]);
                // The delay slot runs on both paths.
                if let Some(d) = img.ins(pc + 4) {
                    let _ = dv_eval(img, &d, &mut r);
                }
                let tgt = i.branch_target();
                let after = pc + 8;
                // The values the branch is taken on.
                let taken: Option<PSet> = match (i.op(), a, b) {
                    (0x04 | 0x05, DV::Phase(k), DV::Const(c))
                    | (0x04 | 0x05, DV::Const(c), DV::Phase(k)) => {
                        let v = c as i32 - k;
                        let eq = PSet::range(v, v);
                        Some(if i.op() == 0x04 {
                            eq
                        } else {
                            PSet::ALL.minus(eq)
                        })
                    }
                    (0x04 | 0x05, DV::Less(k, bound), DV::Const(0)) => {
                        let lt = PSet::range(0, bound - k - 1);
                        Some(if i.op() == 0x04 {
                            PSet::ALL.minus(lt)
                        } else {
                            lt
                        })
                    }
                    (0x06, DV::Phase(k), _) => Some(PSet::range(0, -k)),
                    (0x07, DV::Phase(k), _) => Some(PSet::range(1 - k, 255)),
                    (0x01, DV::Phase(k), _) if i.rt() == 0 => Some(PSet::range(0, -k - 1)),
                    (0x01, DV::Phase(k), _) if i.rt() == 1 => Some(PSet::range(-k, 255)),
                    _ => None,
                };
                match taken {
                    Some(t) => {
                        stack.push((tgt, r, set.and(t), true));
                        stack.push((after, r, set.minus(t), true));
                    }
                    None => {
                        if branched {
                            mark(&mut arms, set, pc);
                        }
                    }
                }
                break;
            }
            if i.is_j() {
                // An unconditional jump inside the switch continues it.
                let t = i.jump_target();
                if let Some(d) = img.ins(pc + 4) {
                    let _ = dv_eval(img, &d, &mut r);
                }
                if branched && set.count() == 1 {
                    mark(&mut arms, set, t);
                    break;
                }
                pc = t;
                continue;
            }
            if i.is_jr() || i.is_jal() || i.is_jalr() {
                if branched {
                    mark(&mut arms, set, pc);
                }
                break;
            }
            match dv_eval(img, &i, &mut r) {
                Step::Dispatch => {}
                Step::Table(tbl, k) => {
                    for v in set.values() {
                        let slot = i32::from(v) + k;
                        if !(0..=255).contains(&slot) {
                            continue;
                        }
                        let Some(t) = img.ins(tbl.wrapping_add(slot as u32 * 4)) else {
                            continue;
                        };
                        if img.contains(t.w) && t.w & 3 == 0 {
                            arms.entry(v).or_insert(t.w);
                        }
                    }
                    break;
                }
                Step::Other => {
                    if branched {
                        mark(&mut arms, set, pc);
                        break;
                    }
                }
            }
            pc += 4;
        }
    }
    arms
}

/// Registers that hold `ctx + 0x279` (`addiu r, x, 0x279`) anywhere in the
/// function - PROT 0925 keeps one in `$s3`. A pointer spilled to the stack
/// and reloaded counts too: PROT 0911 stores `t7 = ctx + 0x279` at
/// `0x3C(sp)` in its prologue and its shared advance tail bumps the phase
/// through `lw t6, 0x3C(sp)` / `sb v0, 0(t6)` (`0x801F7ED8..0x801F7EF0`).
fn phase_alias_regs(img: &Image, f: &FramedFn) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    let start = img.base + f.start as u32;
    let end = img.base + f.end as u32;
    let mut va = start;
    while va < end {
        if let Some(i) = img.ins(va)
            && i.op() == 0x09
            && i.imm() == PHASE_OFFSET
            && i.rt() != i.rs()
        {
            out.insert(i.rt());
        }
        va += 4;
    }
    // Stack slots an alias is spilled to (`sw alias, off(sp)`), then the
    // registers reloaded from them (`lw r, off(sp)`).
    let mut slots = BTreeSet::new();
    let mut va = start;
    while va < end {
        if let Some(i) = img.ins(va)
            && i.op() == 0x2B
            && i.rs() == 29
            && out.contains(&i.rt())
        {
            slots.insert(i.imm());
        }
        va += 4;
    }
    let mut va = start;
    while va < end {
        if let Some(i) = img.ins(va)
            && i.op() == 0x23
            && i.rs() == 29
            && slots.contains(&i.imm())
        {
            out.insert(i.rt());
        }
        va += 4;
    }
    out
}

fn is_phase_load(i: &Ins, alias: &BTreeSet<usize>) -> bool {
    i.op() == 0x24 && (i.imm() == PHASE_OFFSET || (i.imm() == 0 && alias.contains(&i.rs())))
}

fn is_phase_store(i: &Ins, alias: &BTreeSet<usize>) -> bool {
    i.op() == 0x28 && (i.imm() == PHASE_OFFSET || (i.imm() == 0 && alias.contains(&i.rs())))
}

fn find_dispatch(img: &Image, f: &FramedFn) -> Option<PhaseDispatch> {
    let alias = phase_alias_regs(img, f);
    let start = img.base + f.start as u32;
    let end = img.base + f.end as u32;
    let mut va = start;
    while va < end {
        if let Some(i) = img.ins(va)
            && is_phase_load(&i, &alias)
        {
            let mut t = Tracker::new();
            let mut p = start;
            while p < va {
                if let Some(x) = img.ins(p) {
                    t.exec(&x);
                }
                p += 4;
            }
            let mut seed = [DV::Unk; 32];
            for (n, v) in t.r.iter().enumerate() {
                if let V::C(c) = v {
                    seed[n] = DV::Const(*c);
                }
            }
            let arms = walk_switch(img, va, i.rt(), seed, end);
            if arms.len() >= 2 {
                return Some(PhaseDispatch {
                    body: start,
                    load: va,
                    arms,
                    holding: BTreeSet::new(),
                });
            }
        }
        va += 4;
    }
    None
}

// ---------------------------------------------------------------------------
// Control flow
// ---------------------------------------------------------------------------

/// Successors of the instruction at `va` (delay slots folded into the
/// branch): `(next pcs, delay slot va)`.
fn succ(img: &Image, va: u32, lo: u32, hi: u32) -> (Vec<u32>, Option<u32>) {
    let Some(i) = img.ins(va) else {
        return (vec![], None);
    };
    let inside = |t: u32| t >= lo && t < hi;
    if i.is_branch() {
        let mut v = vec![va + 8];
        let t = i.branch_target();
        if inside(t) {
            v.push(t);
        }
        (v, Some(va + 4))
    } else if i.is_j() {
        let t = i.jump_target();
        (if inside(t) { vec![t] } else { vec![] }, Some(va + 4))
    } else if i.is_jr() {
        (vec![], Some(va + 4))
    } else {
        (vec![va + 4], None)
    }
}

/// `true` when the code at `va` is the function's exit: a straight run to
/// `jr ra` with no branch, call or store other than register restores.
fn is_epilogue(img: &Image, va: u32) -> bool {
    for k in 0..16 {
        let Some(i) = img.ins(va + 4 * k) else {
            return false;
        };
        if i.is_jr() {
            return i.rs() == 31;
        }
        if i.is_branch() || i.is_j() || i.is_jal() || i.is_jalr() {
            return false;
        }
        if matches!(i.op(), 0x28 | 0x29 | 0x2B) {
            return false;
        }
    }
    false
}

/// Instructions an arm reaches from its entry, not entering another arm.
fn arm_reach(img: &Image, entry: u32, stop: &BTreeSet<u32>, lo: u32, hi: u32) -> BTreeSet<u32> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![entry];
    while let Some(pc) = todo.pop() {
        if pc < lo || pc >= hi || (pc != entry && stop.contains(&pc)) || !seen.insert(pc) {
            continue;
        }
        let (next, delay) = succ(img, pc, lo, hi);
        if let Some(d) = delay {
            seen.insert(d);
        }
        todo.extend(next);
    }
    seen
}

/// Instructions reachable from `from` inside `within` without crossing a
/// phase store; and whether a phase store / a `jr ra` was met.
fn reach_from(
    img: &Image,
    from: u32,
    within: &BTreeSet<u32>,
    alias: &BTreeSet<usize>,
    lo: u32,
    hi: u32,
) -> (BTreeSet<u32>, bool, bool) {
    let mut seen = BTreeSet::new();
    let mut todo = vec![from];
    let (mut bump, mut exit) = (false, false);
    while let Some(pc) = todo.pop() {
        if !within.contains(&pc) || !seen.insert(pc) {
            continue;
        }
        let Some(i) = img.ins(pc) else { continue };
        if is_phase_store(&i, alias) {
            bump = true;
            continue;
        }
        let (next, delay) = succ(img, pc, lo, hi);
        if let Some(d) = delay
            && let Some(di) = img.ins(d)
        {
            if is_phase_store(&di, alias) {
                bump = true;
                continue;
            }
            seen.insert(d);
        }
        if i.is_jr() && i.rs() == 31 {
            exit = true;
        }
        todo.extend(next);
    }
    (seen, bump, exit)
}

// ---------------------------------------------------------------------------
// Constant tracker
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum V {
    Unk,
    C(u32),
    /// The battle context `*(0x8007BD24)`.
    Ctx,
    /// The scratchpad packet cursor `*(0x1F8003A0)`.
    Pkt,
    /// `$sp + off` (off relative to the caller's `$sp`).
    Sp(i32),
    /// `ctx[field] + delta` loaded and adjusted.
    CtxRel(u16, i32),
    /// A value derived from `ctx[field]` the tracker could not fold (a
    /// compare of it, a product of it).
    CtxTaint(u16),
    /// The fade actor a `FUN_80024E80` call at this VA returned.
    Handle(u32),
    /// The effect part a spawn call seated off this record.
    PartHandle(u32),
    /// A word read out of a handle slot: an actor pointer.
    SlotPtr(HandleSlot),
    /// That actor's flag word `+0x10`, possibly with bits or-ed in.
    ActorFlags(HandleSlot, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Mem {
    Abs(u32),
    Stack(i32),
    Pkt(i32),
}

#[derive(Clone)]
struct Tracker {
    r: [V; 32],
    mem: HashMap<Mem, (u32, u8)>,
    /// The last branch tested a value tainted by this ctx field.
    clamp_window: Option<(u16, u32)>,
    /// The multiplier's `LO`.
    lo: Option<u32>,
}

impl Tracker {
    fn new() -> Self {
        let mut r = [V::Unk; 32];
        r[0] = V::C(0);
        r[29] = V::Sp(0);
        Self {
            r,
            mem: HashMap::new(),
            clamp_window: None,
            lo: None,
        }
    }

    fn addr(&self, base: V, off: i32) -> Option<Mem> {
        Some(match base {
            V::C(c) => Mem::Abs(c.wrapping_add(off as u32)),
            V::Sp(s) => Mem::Stack(s + off),
            V::Pkt => Mem::Pkt(off),
            _ => return None,
        })
    }

    fn load(&self, m: Mem, width: u8, signed: bool) -> V {
        if let Mem::Abs(a) = m {
            if a == FRAME_STEP {
                return V::C(1);
            }
            if a == SPEED_SCALAR_ADDR {
                return V::C(SPEED_SCALAR as u32);
            }
            if a == BATTLE_CTX_PTR && width == 4 {
                return V::Ctx;
            }
            if a == PACKET_CURSOR && width == 4 {
                return V::Pkt;
            }
        }
        match self.mem.get(&m) {
            Some(&(v, w)) if w >= width => {
                let v = match (width, signed) {
                    (1, false) => v & 0xFF,
                    (1, true) => v as u8 as i8 as i32 as u32,
                    (2, false) => v & 0xFFFF,
                    (2, true) => v as u16 as i16 as i32 as u32,
                    _ => v,
                };
                V::C(v)
            }
            _ => V::Unk,
        }
    }

    fn store(&mut self, m: Mem, v: V, width: u8) {
        // A store clobbers any overlapping record.
        let key_off = |m: &Mem| match *m {
            Mem::Abs(a) => (0u8, a as i64),
            Mem::Stack(s) => (1, i64::from(s)),
            Mem::Pkt(s) => (2, i64::from(s)),
        };
        let (kk, ko) = key_off(&m);
        self.mem.retain(|k, (_, w)| {
            let (k2, o2) = key_off(k);
            k2 != kk || o2 + i64::from(*w) <= ko || ko + i64::from(width) <= o2
        });
        if let V::C(c) = v {
            self.mem.insert(m, (c, width));
        }
    }

    fn call_clobber(&mut self) {
        for n in [
            1usize, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 31,
        ] {
            self.r[n] = V::Unk;
        }
    }

    /// Execute one instruction (no control flow).
    fn exec(&mut self, i: &Ins) {
        let rs = self.r[i.rs()];
        let rt = self.r[i.rt()];
        let c = |v: V| match v {
            V::C(c) => Some(c),
            _ => None,
        };
        let out: Option<V> = match i.op() {
            0x0F => Some(V::C(u32::from(i.imm()) << 16)),
            0x09 => Some(match rs {
                V::C(x) => V::C(x.wrapping_add(i.simm() as u32)),
                V::Sp(s) => V::Sp(s + i.simm()),
                V::CtxRel(f, d) => V::CtxRel(f, d + i.simm()),
                V::CtxTaint(f) => V::CtxTaint(f),
                _ => V::Unk,
            }),
            0x0D => Some(match rs {
                V::ActorFlags(slot, bits) => V::ActorFlags(slot, bits | u32::from(i.imm())),
                _ => c(rs).map_or(V::Unk, |x| V::C(x | u32::from(i.imm()))),
            }),
            0x0C => Some(c(rs).map_or(V::Unk, |x| V::C(x & u32::from(i.imm())))),
            0x0E => Some(c(rs).map_or(V::Unk, |x| V::C(x ^ u32::from(i.imm())))),
            0x0A | 0x0B => Some(match rs {
                V::C(x) => V::C(if i.op() == 0x0A {
                    u32::from((x as i32) < i.simm())
                } else {
                    u32::from(x < i.simm() as u32)
                }),
                V::CtxRel(f, _) | V::CtxTaint(f) => V::CtxTaint(f),
                _ => V::Unk,
            }),
            0x20 | 0x21 | 0x23 | 0x24 | 0x25 => {
                let width = match i.op() {
                    0x20 | 0x24 => 1,
                    0x21 | 0x25 => 2,
                    _ => 4,
                };
                let signed = matches!(i.op(), 0x20 | 0x21);
                Some(match rs {
                    V::Ctx if width == 4 => V::SlotPtr(HandleSlot::Ctx(i.imm())),
                    V::Ctx => V::CtxRel(i.imm(), 0),
                    V::SlotPtr(slot) if width == 4 && i.imm() == 0x10 => V::ActorFlags(slot, 0),
                    _ => match self.addr(rs, i.simm()) {
                        Some(m) => match (self.load(m, width, signed), m) {
                            (V::Unk, Mem::Abs(a)) if width == 4 && a >> 24 == 0x80 => {
                                V::SlotPtr(HandleSlot::Abs(a))
                            }
                            (v, _) => v,
                        },
                        None => V::Unk,
                    },
                })
            }
            0x28 | 0x29 | 0x2B => {
                let width = match i.op() {
                    0x28 => 1,
                    0x29 => 2,
                    _ => 4,
                };
                if let Some(m) = self.addr(rs, i.simm()) {
                    self.store(m, rt, width);
                }
                None
            }
            0x00 => match i.funct() {
                0x00 => Some(c(rt).map_or(V::Unk, |x| V::C(x << i.sa()))),
                0x02 => Some(c(rt).map_or(V::Unk, |x| V::C(x >> i.sa()))),
                0x03 => Some(match rt {
                    V::C(x) => V::C(((x as i32) >> i.sa()) as u32),
                    V::CtxTaint(f) | V::CtxRel(f, _) => V::CtxTaint(f),
                    _ => V::Unk,
                }),
                0x21 | 0x20 => Some(match (rs, rt) {
                    (V::C(a), V::C(b)) => V::C(a.wrapping_add(b)),
                    (V::Sp(s), V::C(b)) | (V::C(b), V::Sp(s)) => V::Sp(s + b as i32),
                    (V::CtxRel(f, d), V::C(b)) | (V::C(b), V::CtxRel(f, d)) => {
                        V::CtxRel(f, d + b as i32)
                    }
                    (V::CtxRel(f, _) | V::CtxTaint(f), _)
                    | (_, V::CtxRel(f, _) | V::CtxTaint(f)) => V::CtxTaint(f),
                    _ => V::Unk,
                }),
                0x23 | 0x22 => Some(match (rs, rt) {
                    (V::C(a), V::C(b)) => V::C(a.wrapping_sub(b)),
                    (V::CtxRel(f, d), V::C(b)) => V::CtxRel(f, d - b as i32),
                    (V::CtxRel(f, _) | V::CtxTaint(f), _)
                    | (_, V::CtxRel(f, _) | V::CtxTaint(f)) => V::CtxTaint(f),
                    _ => V::Unk,
                }),
                0x25 => Some(match (rs, rt) {
                    (V::C(a), V::C(b)) => V::C(a | b),
                    (x, V::C(0)) | (V::C(0), x) => x,
                    _ => V::Unk,
                }),
                0x24 => Some(match (rs, rt) {
                    (V::C(a), V::C(b)) => V::C(a & b),
                    _ => V::Unk,
                }),
                0x27 => Some(match (rs, rt) {
                    (V::C(a), V::C(b)) => V::C(!(a | b)),
                    _ => V::Unk,
                }),
                0x2A | 0x2B => Some(match (rs, rt) {
                    (V::C(a), V::C(b)) => V::C(if i.funct() == 0x2A {
                        u32::from((a as i32) < (b as i32))
                    } else {
                        u32::from(a < b)
                    }),
                    (V::CtxRel(f, _) | V::CtxTaint(f), _)
                    | (_, V::CtxRel(f, _) | V::CtxTaint(f)) => V::CtxTaint(f),
                    _ => V::Unk,
                }),
                0x04 => Some(match (rs, rt) {
                    (V::C(a), V::C(b)) => V::C(b << (a & 31)),
                    _ => V::Unk,
                }),
                0x18 | 0x19 => {
                    self.lo = match (rs, rt) {
                        (V::C(a), V::C(b)) => Some(if i.funct() == 0x18 {
                            (a as i32).wrapping_mul(b as i32) as u32
                        } else {
                            a.wrapping_mul(b)
                        }),
                        _ => None,
                    };
                    None
                }
                0x12 => Some(self.lo.map_or(V::Unk, V::C)),
                _ => Some(V::Unk),
            },
            _ => Some(V::Unk),
        };
        if let (Some(d), Some(v)) = (i.dest(), out) {
            self.r[d] = v;
        } else if let Some(d) = i.dest()
            && !matches!(i.op(), 0x28 | 0x29 | 0x2B)
        {
            self.r[d] = V::Unk;
        }
    }
}

// ---------------------------------------------------------------------------
// Site decode
// ---------------------------------------------------------------------------

const SITE_CALLS: [u32; 8] = [
    FADE_SPAWN,
    PART_POOL_FLUSH,
    PART_POOL_SPAWN,
    ARMED_WASH,
    STREAK_RIBBON,
    MOVE_IMAGE,
    LOAD_IMAGE,
    HUD_ELEMENT,
];

fn is_wash_field(off: u16) -> bool {
    WASH_FIELDS.contains(&off) || off == WASH_SCROLL || off == WASH_SCROLL_RATE
}

/// The site this instruction is, if it is one, given the tracker state just
/// before it (stores) or just after its delay slot (calls).
fn site_call(i: &Ins, t_before: &Tracker, t_call: &Tracker) -> Option<PresentationCall> {
    if i.is_jal() {
        let tgt = i.jump_target();
        let rv = |n: usize| match t_call.r[n] {
            V::C(c) => Some(c),
            _ => None,
        };
        return Some(match tgt {
            FADE_SPAWN => {
                let mut template = [None; FADE_TEMPLATE_WORDS];
                if rv(4) == Some(FADE_TEMPLATE_BLOCK) {
                    for (k, slot) in template.iter_mut().enumerate() {
                        if let V::C(v) =
                            t_call.load(Mem::Abs(FADE_TEMPLATE_BLOCK + 2 * k as u32), 2, true)
                        {
                            *slot = Some(v as i16);
                        }
                    }
                }
                PresentationCall::Fade(FadeCall {
                    template,
                    id: rv(5).map(|v| v as i16),
                })
            }
            HUD_ELEMENT => PresentationCall::HudElement {
                element: rv(4).map(|v| v as u8),
                mode: rv(5).map(|v| v as u8),
                text: match t_call.load(Mem::Abs(MESSAGE_BAR_TEXT), 4, false) {
                    V::C(v) => Some(v),
                    _ => None,
                },
                exit: None,
            },
            PART_POOL_FLUSH => PresentationCall::PoolFlush,
            PART_POOL_SPAWN => PresentationCall::PoolSpawn { record: rv(6) },
            ARMED_WASH => PresentationCall::ArmedWash {
                rgb: rv(4).map(|v| v & 0x00FF_FFFF),
            },
            STREAK_RIBBON => PresentationCall::Streak {
                anchor: match t_call.r[4] {
                    V::Sp(s) => {
                        Some(
                            [0, 2, 4].map(|k| match t_call.load(Mem::Stack(s + k), 2, true) {
                                V::C(v) => Some(v as i16),
                                _ => None,
                            }),
                        )
                    }
                    _ => None,
                },
                trail: rv(5).map(|v| v as u8),
            },
            MOVE_IMAGE | LOAD_IMAGE => {
                let rect = match t_call.r[4] {
                    V::Pkt => [0, 2, 4, 6].map(|k| match t_call.load(Mem::Pkt(k), 2, true) {
                        V::C(v) => Some(v as i16),
                        _ => None,
                    }),
                    _ => [None; 4],
                };
                if tgt == MOVE_IMAGE {
                    PresentationCall::VramMove {
                        rect,
                        dst: [rv(5).map(|v| v as i16), rv(6).map(|v| v as i16)],
                    }
                } else {
                    PresentationCall::VramLoad { rect }
                }
            }
            _ => return None,
        });
    }
    if i.op() == 0x2B {
        let base = t_before.r[i.rs()];
        match t_before.r[i.rt()] {
            V::Handle(fade) => {
                let slot = match base {
                    V::Ctx => Some(HandleSlot::Ctx(i.imm())),
                    V::C(c) => Some(HandleSlot::Abs(c.wrapping_add(i.simm() as u32))),
                    _ => None,
                };
                if let Some(slot) = slot {
                    return Some(PresentationCall::FadeStore { fade, slot });
                }
            }
            V::PartHandle(record) => {
                let slot = match base {
                    V::Ctx => Some(HandleSlot::Ctx(i.imm())),
                    V::C(c) => Some(HandleSlot::Abs(c.wrapping_add(i.simm() as u32))),
                    _ => None,
                };
                if let Some(slot) = slot {
                    return Some(PresentationCall::PartStore { record, slot });
                }
            }
            V::ActorFlags(slot, bits)
                if bits & 8 != 0 && i.imm() == 0x10 && base == V::SlotPtr(slot) =>
            {
                return Some(PresentationCall::ActorKill { slot });
            }
            _ => {}
        }
    }
    if i.op() == 0x28 && i.imm() == STAGE_BYTE_OFFSET && matches!(t_before.r[i.rs()], V::Ctx) {
        let value = match t_before.r[i.rt()] {
            V::C(c) => Some(c as u8),
            _ => None,
        };
        return Some(PresentationCall::StageByte { value });
    }
    if matches!(i.op(), 0x28 | 0x29 | 0x2B) && matches!(t_before.r[i.rs()], V::Ctx) {
        let field = i.imm();
        if !is_wash_field(field) {
            return None;
        }
        let width = match i.op() {
            0x28 => 1,
            0x29 => 2,
            _ => 4,
        };
        let v = t_before.r[i.rt()];
        let store = match v {
            V::C(c) => {
                let c = c as i32;
                if t_before.clamp_window.is_some_and(|(f, _)| f == field) {
                    Some(WashStore::Clamp(c))
                } else {
                    Some(WashStore::Set(c))
                }
            }
            V::CtxRel(f, d) if f == field => Some(WashStore::Add(d)),
            _ => None,
        };
        return Some(PresentationCall::WashField {
            field,
            width,
            store,
        });
    }
    None
}

/// Run the tracker linearly over `[from, to]` (inclusive), returning every
/// site met with the tracker state it decoded against.
fn scan_range(
    img: &Image,
    from: u32,
    to: u32,
    seed: &Tracker,
    want: &BTreeSet<u32>,
) -> Vec<(u32, PresentationCall)> {
    let mut t = seed.clone();
    let mut out = Vec::new();
    let mut va = from;
    let mut pending_call: Option<Ins> = None;
    while va <= to {
        let Some(i) = img.ins(va) else { break };
        let before = t.clone();
        // Clamp window: a branch on a value tainted by a wash field arms it
        // for the next few instructions.
        if i.is_branch() {
            let f = match (t.r[i.rs()], t.r[i.rt()]) {
                (V::CtxTaint(f) | V::CtxRel(f, _), _) | (_, V::CtxTaint(f) | V::CtxRel(f, _)) => {
                    Some(f)
                }
                _ => None,
            };
            t.clamp_window = f.map(|f| (f, va + 32));
        } else if t.clamp_window.is_some_and(|(_, until)| va > until) {
            t.clamp_window = None;
        }
        t.exec(&i);
        if let Some(ci) = pending_call.take() {
            // The delay slot ran; decode the call against it.
            if want.contains(&ci.va)
                && let Some(call) = site_call(&ci, &before, &t)
            {
                out.push((ci.va, call));
            }
            let record = match t.r[6] {
                V::C(rec) => Some(rec),
                _ => None,
            };
            t.call_clobber();
            if ci.is_jal() && ci.jump_target() == FADE_SPAWN {
                t.r[2] = V::Handle(ci.va);
            }
            if ci.is_jal()
                && matches!(
                    ci.jump_target(),
                    crate::summon_overlay::SPAWN_HELPER | crate::summon_overlay::POOL_SPAWN_HELPER
                )
                && let Some(rec) = record
            {
                t.r[2] = V::PartHandle(rec);
            }
        }
        if i.is_jal() || i.is_jalr() {
            pending_call = Some(i);
        } else if want.contains(&va)
            && let Some(call) = site_call(&i, &before, &t)
        {
            out.push((va, call));
            if let PresentationCall::ActorKill {
                slot: HandleSlot::Abs(a),
            } = call
            {
                for slot in kill_loop_slots(img, va, &t, a) {
                    out.push((va, PresentationCall::ActorKill { slot }));
                }
            }
        }
        va += 4;
    }
    out
}

/// The further handles a kill at `va` reaches when it sits in a counted loop
/// over a handle array: `sltiu rX, rC, N` / `bne rX, $zero, <loop head at or
/// before va>` with `addiu rP, rP, step` in the delay slot, a few
/// instructions on. PROT 0923's arms walk its parked handles this way
/// (`0x801F7C5C..0x801F7CB0`: twelve from `0x801FA478`, then eight from
/// `0x801FA4B4`), fades among the parts, so a straight-line walk that sees
/// only the first slot leaves the rest of the array alive. The counter's
/// value at the compare comes off the tracker run on from the kill; an
/// unknown counter adds nothing.
fn kill_loop_slots(img: &Image, va: u32, after_kill: &Tracker, first: u32) -> Vec<HandleSlot> {
    let mut t = after_kill.clone();
    let mut at = va + 4;
    while at <= va + 16 {
        let Some(i) = img.ins(at) else { break };
        if i.op() == 0x0B {
            let (Some(b), Some(d)) = (img.ins(at + 4), img.ins(at + 8)) else {
                break;
            };
            let head = b.branch_target();
            let loops = b.op() == 0x05
                && b.rs() == i.rt()
                && b.rt() == 0
                && head <= va
                && va - head <= 0x40;
            let step = (d.op() == 0x09 && d.rs() == d.rt()).then(|| d.simm());
            let (true, Some(step), V::C(n)) = (loops, step, t.r[i.rs()]) else {
                break;
            };
            // `n` is the counter as the compare sees it on the first pass;
            // the loop runs while it is below the bound.
            let bound = u32::from(i.imm());
            let more = bound.saturating_sub(n);
            return (1..=more.min(0x40))
                .map(|k| HandleSlot::Abs(first.wrapping_add((step as u32).wrapping_mul(k))))
                .collect();
        }
        t.exec(&i);
        at += 4;
    }
    Vec::new()
}

/// Every candidate site VA in `[lo, hi)`: a `jal` to one of the helpers, or a
/// store whose offset is a wash field.
fn candidate_sites(img: &Image, lo: u32, hi: u32) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    let mut va = lo;
    while va < hi {
        if let Some(i) = img.ins(va) {
            let call = i.is_jal() && SITE_CALLS.contains(&i.jump_target());
            let store = (matches!(i.op(), 0x28 | 0x29 | 0x2B) && is_wash_field(i.imm())
                || i.op() == 0x28 && i.imm() == STAGE_BYTE_OFFSET)
                && i.rs() != 29;
            // A handle parked from `$v0`, or a flag-word store (a kill).
            let handle = i.op() == 0x2B && i.rs() != 29 && (i.rt() == 2 || i.imm() == 0x10);
            if call || store || handle {
                out.insert(va);
            }
        }
        va += 4;
    }
    out
}

/// Decode one band image (exactly one PROT entry, loaded at the slot-B
/// base). `own_end` is the file offset the image's own content ends at;
/// framed functions at or past it are an inherited tail and are skipped.
pub fn decode(bytes: &[u8], own_end: usize) -> CastPresentation {
    let img = Image {
        bytes,
        base: SLOT_B_LINK_BASE,
    };
    let fns: Vec<FramedFn> = framed_functions(bytes)
        .into_iter()
        .filter(|f| f.start < own_end)
        .collect();
    let mut out = CastPresentation::default();
    // Per function: its arms' reach sets.
    let mut placed: BTreeMap<u32, (u32, u8, SiteTiming)> = BTreeMap::new();
    // Own-function calls made from a placed arm: callee entry -> (body, arm, timing).
    let mut callee_arm: BTreeMap<u32, Vec<(u32, u8, SiteTiming)>> = BTreeMap::new();
    let mut seeds: BTreeMap<u32, Tracker> = BTreeMap::new();

    for f in &fns {
        let lo = img.base + f.start as u32;
        let hi = img.base + f.end as u32;
        let Some(d) = find_dispatch(&img, f) else {
            continue;
        };
        let alias = phase_alias_regs(&img, f);
        // Prologue seed: the stack stores before the phase load.
        let mut seed = Tracker::new();
        let mut va = lo;
        while va < d.load {
            if let Some(i) = img.ins(va)
                && !i.is_branch()
                && !i.is_j()
                && !i.is_jal()
            {
                seed.exec(&i);
            }
            va += 4;
        }
        // Keep only the stack and the context register knowledge.
        let mut s2 = Tracker::new();
        s2.mem = seed
            .mem
            .iter()
            .filter(|(k, _)| matches!(k, Mem::Stack(_)))
            .map(|(k, v)| (*k, *v))
            .collect();
        for n in 16..=23 {
            if matches!(seed.r[n], V::Ctx | V::C(_)) {
                s2.r[n] = seed.r[n];
            }
        }
        s2.r[30] = seed.r[30];
        s2.r[29] = seed.r[29];
        seeds.insert(lo, s2.clone());

        // An arm the table points at the shared exit is no arm of its own:
        // the other arms reach that code too, so it does not stop a walk.
        let entries: BTreeSet<u32> = d
            .arms
            .values()
            .copied()
            .filter(|&e| !is_epilogue(&img, e))
            .collect();
        let cands = candidate_sites(&img, lo, hi);
        let mut holding = BTreeSet::new();
        for (&phase, &entry) in &d.arms {
            let reach = arm_reach(&img, entry, &entries, lo, hi);
            let has_bump = reach
                .iter()
                .any(|&v| img.ins(v).is_some_and(|i| is_phase_store(&i, &alias)));
            let (hold_reach, _, exits) = reach_from(&img, entry, &reach, &alias, lo, hi);
            if has_bump && exits {
                holding.insert(phase);
            }
            let timing_of = |site: u32| -> SiteTiming {
                if !has_bump {
                    return SiteTiming::Every;
                }
                if !hold_reach.contains(&site) {
                    return SiteTiming::Pass;
                }
                // A site in a `j`'s delay slot runs on into the jump's
                // target, not the next arm's code below it (PROT 0912 parks
                // its arm-13 fade as `j <advance>; sw v0, 0x102C(v1)`).
                let next = match site.checked_sub(4).and_then(|v| img.ins(v)) {
                    Some(j) if j.is_j() => j.jump_target(),
                    _ => site + 4,
                };
                let (_, bump, exit) = reach_from(&img, next, &reach, &alias, lo, hi);
                // The delay slot of a call site is part of it.
                match (bump, exit) {
                    (true, false) => SiteTiming::Pass,
                    (false, true) => SiteTiming::Hold,
                    _ => SiteTiming::Every,
                }
            };
            for &s in cands.iter().filter(|s| reach.contains(s)) {
                placed.entry(s).or_insert((lo, phase, timing_of(s)));
            }
            // Own-image calls made from this arm.
            for &v in &reach {
                if let Some(i) = img.ins(v)
                    && i.is_jal()
                {
                    let t = i.jump_target();
                    if fns.iter().any(|g| img.base + g.start as u32 == t) {
                        callee_arm
                            .entry(t)
                            .or_default()
                            .push((lo, phase, timing_of(v)));
                    }
                }
            }
        }
        // Decode every site of this function against its arm's linear run.
        for (&phase, &entry) in &d.arms {
            let want: BTreeSet<u32> = placed
                .iter()
                .filter(|(_, (b, p, _))| *b == lo && *p == phase)
                .map(|(v, _)| *v)
                .collect();
            if want.is_empty() {
                continue;
            }
            // An arm entered through a `j` stub runs the stub's delay slot
            // and then the code at its target.
            let mut seed = s2.clone();
            // The register the switch loaded the phase byte into still holds
            // the arm's own phase on entry: PROT 0917 / 0928 store it as the
            // stage byte (`sb s1,0x278(a1)` at `0x801F6E90`, `sb a1,0x278(v0)`
            // at `0x801F6F1C`).
            if let Some(li) = img.ins(d.load)
                && li.op() == 0x24
            {
                seed.r[li.rt()] = V::C(u32::from(phase));
            }
            let entry = match img.ins(entry) {
                Some(i) if i.is_j() => {
                    if let Some(d) = img.ins(entry + 4) {
                        seed.exec(&d);
                    }
                    i.jump_target()
                }
                _ => entry,
            };
            let last = *want.iter().max().unwrap_or(&entry);
            // Sites may sit below the entry (a shared block the arm jumps
            // back to): scan from the lower of the two.
            let first = (*want.iter().min().unwrap_or(&entry)).min(entry);
            let from = if first < entry {
                first.saturating_sub(0x80).max(lo)
            } else {
                entry
            };
            for (va, call) in scan_range(&img, from, last + 4, &seed, &want) {
                let (_, _, timing) = placed[&va];
                out.sites.push(PresentationSite {
                    va,
                    body: lo,
                    arm: phase,
                    timing,
                    call,
                });
            }
        }
        out.dispatches.push(PhaseDispatch { holding, ..d });
    }

    // Sites in routines a placed arm calls, and the rest unplaced.
    for f in &fns {
        let lo = img.base + f.start as u32;
        let hi = img.base + f.end as u32;
        if out.dispatches.iter().any(|d| d.body == lo) {
            continue;
        }
        let cands = candidate_sites(&img, lo, hi);
        if cands.is_empty() {
            continue;
        }
        let decoded = scan_range(&img, lo, hi, &Tracker::new(), &cands);
        match callee_arm.get(&lo) {
            Some(callers) => {
                let mut seen = BTreeSet::new();
                for &(body, arm, timing) in callers {
                    if !seen.insert((body, arm)) {
                        continue;
                    }
                    for &(va, call) in &decoded {
                        out.sites.push(PresentationSite {
                            va,
                            body,
                            arm,
                            timing,
                            call,
                        });
                    }
                }
            }
            None => {
                for (va, call) in decoded {
                    out.unplaced.push(UnplacedSite { va, call });
                }
            }
        }
    }
    // Sites inside a dispatching function no arm reaches.
    for d in &out.dispatches {
        let Some(f) = fns.iter().find(|f| img.base + f.start as u32 == d.body) else {
            continue;
        };
        let lo = d.body;
        let hi = img.base + f.end as u32;
        let cands: BTreeSet<u32> = candidate_sites(&img, lo, hi)
            .into_iter()
            .filter(|s| !placed.contains_key(s))
            .collect();
        if cands.is_empty() {
            continue;
        }
        let seed = seeds.get(&lo).cloned().unwrap_or_else(Tracker::new);
        for (va, call) in scan_range(&img, lo, hi, &seed, &cands) {
            out.unplaced.push(UnplacedSite { va, call });
        }
    }
    // A HUD element raise names the way out of its arm it sits on.
    for site in &mut out.sites {
        if let PresentationCall::HudElement { exit, .. } = &mut site.call
            && let Some(f) = fns
                .iter()
                .find(|f| (img.base + f.start as u32..img.base + f.end as u32).contains(&site.va))
        {
            *exit = site_exit_phase(&img, &phase_alias_regs(&img, f), site.va);
        }
    }
    out.sites.sort_by_key(|s| (s.body, s.arm, s.va));
    out
}

/// The phase the straight-line run after the call at `va` stores, if it
/// stores one before any branch: the phase byte loaded (`lbu`, read as the
/// arm's own phase) and stepped by `addiu`, or a literal. The run stops at a
/// conditional branch or a `jr`; a `j` is followed after its delay slot.
/// Call Wave's arm 0 raises its message bar and then bumps the phase
/// (`0x801F6CF0..0x801F6D00`); Big Wave, forked off the same arm, never
/// reaches the raise.
fn site_exit_phase(img: &Image, alias: &BTreeSet<usize>, va: u32) -> Option<PhaseExit> {
    #[derive(Clone, Copy)]
    enum P {
        Unk,
        C(i32),
        Ph(i32),
    }
    let mut r = [P::Unk; 32];
    r[0] = P::C(0);
    let mut at = va + 4;
    // What happens after the delay slot of the control transfer just met:
    // a call's clobber, a jump's target, or the end of the run.
    enum After {
        Clobber,
        Jump(u32),
        Stop,
    }
    let mut pending: Option<After> = None;
    for _ in 0..96 {
        let i = img.ins(at)?;
        if is_phase_store(&i, alias) {
            return match r[i.rt()] {
                P::Ph(k) => Some(PhaseExit::Rel(k)),
                P::C(c) => u8::try_from(c).ok().map(PhaseExit::Abs),
                P::Unk => None,
            };
        }
        let out = if is_phase_load(&i, alias) {
            P::Ph(0)
        } else {
            match i.op() {
                0x09 => match r[i.rs()] {
                    P::C(c) => P::C(c + i.simm()),
                    P::Ph(k) => P::Ph(k + i.simm()),
                    P::Unk => P::Unk,
                },
                0x0D => match r[i.rs()] {
                    P::C(c) => P::C(c | i32::from(i.imm())),
                    _ => P::Unk,
                },
                0x00 if i.funct() == 0x25 || i.funct() == 0x21 => match (r[i.rs()], r[i.rt()]) {
                    (x, P::C(0)) | (P::C(0), x) => x,
                    _ => P::Unk,
                },
                _ => P::Unk,
            }
        };
        if let Some(d) = i.dest() {
            r[d] = out;
        }
        r[0] = P::C(0);
        match pending.take() {
            Some(After::Clobber) => {
                // The callee clobbers the temporaries; `s` registers survive.
                for n in [
                    1usize, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 31,
                ] {
                    r[n] = P::Unk;
                }
            }
            Some(After::Jump(t)) => {
                at = t;
                continue;
            }
            Some(After::Stop) => return None,
            None => {}
        }
        pending = if i.is_jal() || i.is_jalr() {
            Some(After::Clobber)
        } else if i.is_j() {
            Some(After::Jump(i.jump_target()))
        } else if i.is_branch() || i.is_jr() {
            Some(After::Stop)
        } else {
            None
        };
        at += 4;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flush walks `0x80` words from `0x801C90F0`: the last one,
    /// `0x801C92EC`, is the word PROT 0930 and 0931 park their held fades in.
    #[test]
    fn the_part_pool_spans_the_flush_walk() {
        assert!(HandleSlot::Abs(PART_POOL).in_part_pool());
        assert!(HandleSlot::Abs(0x801C_92EC).in_part_pool());
        assert!(!HandleSlot::Abs(0x801C_92F0).in_part_pool());
        assert!(!HandleSlot::Abs(0x801C_90EC).in_part_pool());
        assert!(!HandleSlot::Ctx(0x102C).in_part_pool());
    }
}
