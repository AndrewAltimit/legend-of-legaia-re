//! Every write to the sub-mode word (`+0x204`) and the pad masks that gate them.
//! Split out of `title_overlay.rs`.

/// One `state[+0x204] = value` write inside the tick function. Each row
/// pins the store's instruction address, the sub-mode handler whose body
/// contains it, the value stored, and the guard the disassembly puts in
/// front of it.
///
/// Three store sites sit inside one handler's body but are also reached
/// by another handler's unconditional `j` into the middle of it; those
/// carry the extra source in [`State204Write::also_from`]. The shared
/// epilogue's own stores use [`SUBMODE_TAIL_SOURCE`] as their `from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State204Write {
    /// PSX virtual address of the `sw` instruction.
    pub pc: u32,
    /// Sub-mode whose handler body contains this write, or
    /// [`SUBMODE_TAIL_SOURCE`] for the shared epilogue.
    pub from: u8,
    /// Extra sub-modes that reach this same store through a `j` into
    /// another handler's body. Empty for all but three sites.
    pub also_from: &'static [u8],
    /// Either a static value the dispatcher transitions to, or
    /// [`TransitionTarget::Register`] when the source is a register.
    pub target: TransitionTarget,
    /// The condition the disassembly guards this store with, in one line.
    pub guard: &'static str,
}

/// Static-vs-dynamic transition target for [`State204Write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionTarget {
    /// The write stores a known literal mode value.
    Mode(u8),
    /// One store site whose source register carries one of two literals,
    /// picked by a branch a few instructions earlier.
    OneOf(&'static [u8]),
    /// The write stores a register value (commonly `s3`, the per-handler
    /// cancel target, which is `-1` in the handlers that take no cancel).
    Register(&'static str),
    /// The store writes back what it just read - the shared epilogue's
    /// `submode = FUN_801E1114(submode)`, whose callee returns its
    /// argument on every path. Not a transition.
    Identity,
}

/// `from` marker for the six stores that live in the shared epilogue at
/// [`SUBMODE_BODY_PC`](super::SUBMODE_BODY_PC) rather than in one sub-mode's handler body. The
/// epilogue runs after **every** handler, so these apply from any mode.
pub const SUBMODE_TAIL_SOURCE: u8 = 0xFF;

/// Every `state[+0x204] = N` store in the tick function, ordered by PC.
///
/// This is the complete set: the body carries exactly 56 `sw` to
/// `0x204(base)` and all 56 are here, each with the handler it belongs to
/// and its guard. Read off the disassembly of `FUN_801DD35C` (PROT entry
/// 0899 file `+0xEB44`; see `ghidra/scripts/funcs/overlay_title_801ddccc.txt`)
/// and cross-checked against a Capstone pass over the entry's own bytes.
pub const STATE_204_WRITES: &[State204Write] = &[
    State204Write {
        pc: 0x801D_D920,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x02),
        guard: "unconditional; every retail path overwrites it below",
    },
    State204Write {
        pc: 0x801D_D97C,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x11),
        guard: "entry word `_DAT_8007BB00 != 0` (lw a0,-0x4500(v0) @0x801DD968)",
    },
    State204Write {
        pc: 0x801D_DA58,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "`param_2 == 1`; also stashes `state[+0x200]=0`, `state[+0x1FC]=0`",
    },
    State204Write {
        pc: 0x801D_DA80,
        from: 0x00,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "`param_2 == 2`; also stashes `state[+0x200]=1`",
    },
    State204Write {
        pc: 0x801D_DAC4,
        from: 0x11,
        also_from: &[],
        target: TransitionTarget::Mode(0x10),
        guard: "`_DAT_8007BAB4 <= 0`; re-arms the countdown to 0x5DC",
    },
    State204Write {
        pc: 0x801D_DC38,
        from: 0x10,
        also_from: &[],
        target: TransitionTarget::Mode(0x00),
        guard: "confirm on row 0; the next instruction stores 0x16 over it",
    },
    State204Write {
        pc: 0x801D_DC3C,
        from: 0x10,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "confirm (`_DAT_8007B874 & 0x844`) on row 0 - NEW GAME",
    },
    State204Write {
        pc: 0x801D_DC5C,
        from: 0x10,
        also_from: &[],
        target: TransitionTarget::Mode(0x18),
        guard: "confirm on row 1 - CONTINUE; stashes `state[+0x200]=1`",
    },
    State204Write {
        pc: 0x801D_DDD8,
        from: 0x18,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "`state[-0xee0]` ramps up by `0x80 * scalar` and reaches 0x1000",
    },
    State204Write {
        pc: 0x801D_DE4C,
        from: 0x02,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "fade-out finished and `param_2 != 0`",
    },
    State204Write {
        pc: 0x801D_DF1C,
        from: 0x02,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "confirm (`& 0x44`); stashes the picked row in `state[+0x200]`",
    },
    State204Write {
        pc: 0x801D_E11C,
        from: 0x14,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "confirm (`& 0x44`) once both alpha ramps have drained",
    },
    State204Write {
        pc: 0x801D_E25C,
        from: 0x07,
        also_from: &[],
        target: TransitionTarget::Mode(0x15),
        guard: "`state[-0xec0] == 0 && state[-0xef4] == 0` (no card op in flight)",
    },
    State204Write {
        pc: 0x801D_E3E8,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x09),
        guard: "check timer >= 0x259, `state[+0x21C] == 3 && state[+0x224] == 0`",
    },
    State204Write {
        pc: 0x801D_E418,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::OneOf(&[0x0C, 0x0D]),
        guard: "same arm with `state[+0x1E4] == 0`: 0x0C when `state[+0x200] != 0`, \
                0x0D when it is 0 and `state[+0x1F0] == 0`",
    },
    State204Write {
        pc: 0x801D_E434,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x08),
        guard: "`state[+0x218] > 0` (a card fault); also sets `state[-0xe58] = 0xA`",
    },
    State204Write {
        pc: 0x801D_E484,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x0F),
        guard: "`state[+0x1BC] >= 2 && state[+0x200] == 0`",
    },
    State204Write {
        pc: 0x801D_E4A0,
        from: 0x15,
        also_from: &[],
        target: TransitionTarget::Mode(0x0C),
        guard: "`state[+0x1BC] >= 2 && state[+0x200] != 0`",
    },
    State204Write {
        pc: 0x801D_E5D0,
        from: 0x08,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "`state[+0x218] == 0` (the fault cleared); re-arms `state[+0x228] = 0x78`",
    },
    State204Write {
        pc: 0x801D_E624,
        from: 0x08,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_E654,
        from: 0x09,
        also_from: &[],
        target: TransitionTarget::Mode(0x0A),
        guard: "unconditional - 0x09 is a one-frame setup state",
    },
    State204Write {
        pc: 0x801D_E6DC,
        from: 0x0C,
        also_from: &[],
        target: TransitionTarget::Register("s3 = 0x14"),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_E748,
        from: 0x0D,
        also_from: &[],
        target: TransitionTarget::Register("s3 = 0x14"),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_E844,
        from: 0x0A,
        also_from: &[0x15],
        target: TransitionTarget::Mode(0x07),
        guard: "any of six abort flags set; also reached by mode 0x15's `j 0x801DE838`",
    },
    State204Write {
        pc: 0x801D_E888,
        from: 0x0A,
        also_from: &[],
        target: TransitionTarget::Mode(0x0C),
        guard: "`state[+0x200] != 0 && state[+0x1E4] == 0`",
    },
    State204Write {
        pc: 0x801D_E934,
        from: 0x0A,
        also_from: &[],
        target: TransitionTarget::Mode(0x0B),
        guard: "block scan complete (`state[+0x1D4] == state[+0x1E4]`)",
    },
    State204Write {
        pc: 0x801D_EB24,
        from: 0x0B,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "chosen cell == 0x80; the 5x3 grid only yields 0..14, so this arm is dead",
    },
    State204Write {
        pc: 0x801D_EC00,
        from: 0x0B,
        also_from: &[],
        target: TransitionTarget::Mode(0x0E),
        guard: "a cell below 15 confirmed and `state[+0x200] == 0`",
    },
    State204Write {
        pc: 0x801D_EC34,
        from: 0x0B,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "`state[-0xeb8] != 0` (the card went away)",
    },
    State204Write {
        pc: 0x801D_ECA4,
        from: 0x0E,
        also_from: &[],
        target: TransitionTarget::Mode(0x0B),
        guard: "cancel (`& 0x21`); cue 0x37",
    },
    State204Write {
        pc: 0x801D_EDF8,
        from: 0x0E,
        also_from: &[],
        target: TransitionTarget::Mode(0x03),
        guard: "confirm with `state[+0x1FC] == 1` and `state[+0x200] == 0` - the SAVE arm",
    },
    State204Write {
        pc: 0x801D_EE00,
        from: 0x0E,
        also_from: &[],
        target: TransitionTarget::Mode(0x0B),
        guard: "confirm with `state[+0x1FC] != 1`",
    },
    State204Write {
        pc: 0x801D_EF28,
        from: 0x0F,
        also_from: &[],
        target: TransitionTarget::Mode(0x12),
        guard: "confirm with `state[+0x1FC] == 1`",
    },
    State204Write {
        pc: 0x801D_EF34,
        from: 0x0F,
        also_from: &[0x15],
        target: TransitionTarget::Mode(0x14),
        guard: "confirm with `state[+0x1FC] != 1`; also reached by mode 0x15's `j 0x801DEF2C`",
    },
    State204Write {
        pc: 0x801D_EFE8,
        from: 0x12,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "inner phase 0, any button (`& 0xF5`)",
    },
    State204Write {
        pc: 0x801D_F2FC,
        from: 0x12,
        also_from: &[],
        target: TransitionTarget::Mode(0x07),
        guard: "inner phase 0x0D, the `state[+0x3288]` hold timer underflows",
    },
    State204Write {
        pc: 0x801D_F364,
        from: 0x04,
        also_from: &[],
        target: TransitionTarget::Mode(0x13),
        guard: "`state[+0x218] > 0` (delay slot - taken on the whole fault arm)",
    },
    State204Write {
        pc: 0x801D_F400,
        from: 0x04,
        also_from: &[],
        target: TransitionTarget::Mode(0x05),
        guard: "`state[+0x1DC] != 0 && state[+0x1D0] == 0x1000`",
    },
    State204Write {
        pc: 0x801D_F484,
        from: 0x13,
        also_from: &[0x0E],
        target: TransitionTarget::Mode(0x04),
        guard: "retry with `state[-0xf08] < 5`; also reached by mode 0x0E's `j 0x801DF47C` \
                (the LOAD entry into the card-op state)",
    },
    State204Write {
        pc: 0x801D_F588,
        from: 0x13,
        also_from: &[],
        target: TransitionTarget::Mode(0x14),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_F5FC,
        from: 0x03,
        also_from: &[],
        target: TransitionTarget::Mode(0x17),
        guard: "the save hold timer `state[-0xee8]` reaches 0x4B1",
    },
    State204Write {
        pc: 0x801D_F638,
        from: 0x03,
        also_from: &[],
        target: TransitionTarget::Mode(0x17),
        guard: "`state[-0xef4] != 0` (the write errored)",
    },
    State204Write {
        pc: 0x801D_F68C,
        from: 0x03,
        also_from: &[],
        target: TransitionTarget::Mode(0x17),
        guard: "`state[+0x218] != 0` (a card fault during the write)",
    },
    State204Write {
        pc: 0x801D_F778,
        from: 0x17,
        also_from: &[],
        target: TransitionTarget::Mode(0x03),
        guard: "retry with `state[-0xf08] < 5`",
    },
    State204Write {
        pc: 0x801D_F7A8,
        from: 0x17,
        also_from: &[],
        target: TransitionTarget::Register("s3 = 0x14"),
        guard: "any button (`& 0xF5`); cue 0x20",
    },
    State204Write {
        pc: 0x801D_F8B0,
        from: 0x05,
        also_from: &[],
        target: TransitionTarget::Mode(0x13),
        guard: "the loaded block fails its `slot[0x1FFC] == 1` check",
    },
    State204Write {
        pc: 0x801D_F8BC,
        from: 0x05,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "the block verifies; also sets `state[-0xea8] = 1` (the LOAD banner)",
    },
    State204Write {
        pc: 0x801D_FA88,
        from: 0x16,
        also_from: &[],
        target: TransitionTarget::Mode(0x06),
        guard: "the white-out `state[-0xeac]` reaches 0x1200",
    },
    State204Write {
        pc: 0x801D_FC1C,
        from: 0x06,
        also_from: &[],
        target: TransitionTarget::Mode(0x00),
        guard: "unconditional - the launcher re-arms Init behind the mode-2 hand-off",
    },
    State204Write {
        pc: 0x801D_FE88,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Identity,
        guard: "`state[-0xe74] >= 0`: `FUN_801E1114` returns its argument on every path",
    },
    State204Write {
        pc: 0x801D_FED0,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x08),
        guard: "`s5 != 0 && state[+0x218] > 0 && state[+0x228] == 0`",
    },
    State204Write {
        pc: 0x801D_FEF8,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x10),
        guard: "`_DAT_8007BB00 != 0 && submode == 2` - the second place 0x02 is bypassed",
    },
    State204Write {
        pc: 0x801D_FF28,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x16),
        guard: "`param_2 != 0 && submode == 2`",
    },
    State204Write {
        pc: 0x801D_FF74,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::OneOf(&[0x16, 0x17]),
        guard: "`state[+0x329C] == 0 && submode == 3`: 0x17 when `state[-0xec4] != 0`, else 0x16",
    },
    State204Write {
        pc: 0x801D_FF9C,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Mode(0x13),
        guard: "`state[+0x329C] == 0 && submode == 4 && state[-0xec0] != 0`",
    },
    State204Write {
        pc: 0x801D_FFE0,
        from: SUBMODE_TAIL_SOURCE,
        also_from: &[],
        target: TransitionTarget::Register("s3"),
        guard: "cancel (`& 0x21`) with `s3 >= 0` and submode not 3 or 4; cue 0x37",
    },
];

/// PSX virtual address of the `sh v0, -0x47C4(v1)` instruction inside
/// [`TitleOverlaySubMode::LaunchGame`](super::TitleOverlaySubMode::LaunchGame) that writes
/// [`MASTER_GAME_MODE_FIELD_LAUNCH`](super::MASTER_GAME_MODE_FIELD_LAUNCH) to [`MASTER_GAME_MODE_ADDR`](super::MASTER_GAME_MODE_ADDR).
///
/// This is the title-screen -> main-game transition's hard pin: an
/// engine-side observer watching `MASTER_GAME_MODE_ADDR` and noticing
/// the value flip to `0x02` knows it's time to swap out the title
/// overlay and load the field/town runtime.
pub const PHASE06_LAUNCH_GAME_PC: u32 = 0x801D_FC00;

// Pad-mask combinations - see `project_legaia_pad_mask_layout`.
// Legaia repacks the raw PSX 16-bit pad word: dpad lives in the HIGH byte,
// face/shoulder buttons in the LOW byte. These constants use the repacked
// layout (i.e. they match the literals the dispatcher uses verbatim).

/// `Cross | L1` - confirm (Cross with L1 as alt).
pub const PADMASK_CONFIRM_L1_CROSS: u16 = 0x0044;

/// `Circle | L2` - cancel (Circle with L2 as alt).
pub const PADMASK_CANCEL_L2_CIRCLE: u16 = 0x0021;

/// All face buttons + L1 + L2 - "any non-R-shoulder button"
/// (used as the generic "user interacted" filter to break attract).
pub const PADMASK_ANY_FACE_OR_L: u16 = 0x00F5;

/// `Start | L1 | Cross` - "press Start / confirm" mask the
/// `AttractIdle` polling path tests at `0x801DDC04`.
pub const PADMASK_START_L1_CROSS: u16 = 0x0844;
