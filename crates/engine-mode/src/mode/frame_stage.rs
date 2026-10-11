//! What each mode's per-frame routine runs: frame bodies, the debug mode-advance table and the per-frame stage.

use super::*;

/// The mid-frame driver a per-frame mode handler calls between the
/// frame-begin pass and the frame-end pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBody {
    /// `FUN_80016444(param)` - the master frame driver (five actor tick
    /// passes, five render passes, the display flip). The `param` really does
    /// vary by mode: the default handler passes `1`, MAPDISP passes `0`.
    Master { param: i32 },
    /// `FUN_80017978` - the CARD-mode substitute. Mode 23 replaces the master
    /// driver outright rather than parameterising it. See [`CARD_FRAME_BODY`]
    /// for what it does instead.
    CardDriver,
}

/// What mode 23 CARD runs in place of the master frame driver.
///
/// REF: FUN_80017978 - ported, but the `PORT:` tag lives on
/// [`per_frame_stage`], the function that materialises this descriptor. A tag
/// here anchors liveness to a plain data `struct` with no `impl`, which falls
/// back to *file* scope and reports the whole of `mode.rs` as the port's reach.
/// REF: FUN_800179C0, FUN_800188C8, FUN_80020DE0
///
/// The whole body is three calls and a `move v0, zero`
/// (`0x80017978..0x800179BC`):
///
/// 1. `FUN_800179C0` - the debug mode-advance chord. Its first two
///    instructions load `_DAT_8007B98C` and branch straight to `jr ra` when it
///    is zero, which is the retail value, so on a shipped disc this leg does
///    nothing. See [`DEBUG_MODE_ADVANCE`] for the law it encodes.
/// 2. `(*_DAT_8007B8E0)[+0x0C]()` - an indirect call through the CARD actor's
///    tick handler. `_DAT_8007B8E0` is not a mode-table row: it is the actor
///    the mode-entry path spawns from descriptor `0x800706D4` via
///    `FUN_80020DE0` (`sw v0,-0x4720(at)` at `0x800257AC`), and `+0x0C` is the
///    handler slot that spawner copies out of the descriptor's `+0x8`.
/// 3. `FUN_800188C8(_DAT_1F800393)` - the dev pad-driven readout HUD, itself
///    gated on `_DAT_8007B98C` and already out of scope.
///
/// So the load-bearing content is step 2 alone, and two things follow that the
/// declarative [`PerFrameStage`] shape does not otherwise say:
///
/// - **CARD never calls `FUN_80016444`.** Mode 23 runs no actor tick passes,
///   no render passes and no display flip through the master driver; whatever
///   the card actor's handler draws is the entire frame.
/// - **The abort branch is dead for CARD.** `FUN_80017978` ends `move v0,zero`
///   and has no other return path, so the `body_can_abort` test in the mode-23
///   handler `FUN_80025F74` can never fire, and the frame-end pass
///   `FUN_80016B6C` always runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardFrameBody {
    /// Whether the body runs the master frame driver `FUN_80016444`.
    pub runs_master_driver: bool,
    /// Whether it dispatches the CARD actor's `+0x0C` tick handler.
    pub ticks_card_actor: bool,
    /// The value the body always returns. Zero, so the caller's abort test
    /// never fires.
    pub returns: i32,
}

/// The retail shape of [`FrameBody::CardDriver`].
pub const CARD_FRAME_BODY: CardFrameBody = CardFrameBody {
    runs_master_driver: false,
    ticks_card_actor: true,
    returns: 0,
};

/// The debug mode-advance chord `FUN_800179C0` reads, recorded because it is
/// the only place in `SCUS_942.54` that writes the game-mode global
/// `_DAT_8007B83C` from a mode table row's `next` field - the field
/// [`ModeEntry::next`] models.
///
/// REF: FUN_800179C0
///
/// The body is inert in retail (`_DAT_8007B98C == 0` gates it at
/// `0x800179CC`), so this is a description, not a tick path. The law, read off
/// `0x800179C0..0x80017AA8`:
///
/// - A hold-repeat countdown at `_DAT_8007B890` decrements once per call and
///   suppresses the rest of the body until it reaches zero.
/// - The chord tested against the packed pad word `_DAT_8007B850` is `0x900`
///   when `_DAT_8007B868` is zero and `0x100` otherwise; in the `0x100` case a
///   low-nibble-all-set (`pad & 0xF == 0xF`) alternative also triggers.
/// - On a trigger it reads `mode_table[current].next` - the `i16` at `+0xA` of
///   the 24-byte row, table base `0x8007078C` - and a negative value means "no
///   transition", the same `-1` sentinel [`ModeEntry::next`] maps to `None`.
/// - One special case ahead of the table read: from mode 3 (`MainMode`) with
///   `_DAT_8007B8C8` non-zero it jumps to mode `0x0E` instead.
///
/// The `next` read happens twice in the disassembly, once per branch of the
/// mode-3 test, and the second copy works only because the delay slot at
/// `0x80017A60` reloads the table base (`lui v1,0x8007`). Reading the second
/// `addiu v1,v1,0x78c` as an offset *from the first address* would put the
/// table at `0x80070F18`; it does not.
pub const DEBUG_MODE_ADVANCE_TABLE_BASE: u32 = 0x8007_078C;

/// Stride of a row in the `0x8007078C` mode table, in bytes.
pub const DEBUG_MODE_ADVANCE_ROW_STRIDE: u32 = 24;

/// Byte offset of the `next mode` `i16` inside a mode-table row.
pub const DEBUG_MODE_ADVANCE_NEXT_OFFSET: u32 = 0x0A;

/// The per-frame handler shape shared by every odd-indexed (per-frame) mode.
///
/// REF: FUN_80025eec, FUN_80025f2c, FUN_80025f74 - the three handlers this
/// shape describes. Their `PORT:` tags live on [`per_frame_stage`], which is
/// what resolves a mode to one of them; a tag here would anchor liveness to a
/// plain data `struct` with no `impl`, i.e. to the whole file.
/// REF: FUN_8001698C, FUN_80016444, FUN_80016B6C, FUN_80017978, FUN_801CE850
///
/// All three are the same eight-instruction skeleton, and the differences
/// between them are exactly the three fields below. Read off the disassembly
/// (`see ghidra/scripts/funcs/80025eec.txt`, `80025f2c.txt`, `80025f74.txt`)
/// and confirmed against the static-recomp renderings.
///
/// ```text
///   if (FUN_8001698C() != 0) return;      // frame-begin; non-zero = skipped
///   [overlay_hook()]                      // MAPDISP only
///   if (<body>() != 0) return;
///   FUN_80016B6C();                       // frame-end
/// ```
///
/// The **early-out is the load-bearing part**. `FUN_8001698C` returns `1`
/// when it took its frame-skip branch (`gp+0x3D8` set and neither
/// `_DAT_8007B938` nor `gp+0x55C` carrying bit `0x800`), in which case it has
/// already done the pad poll and a `VSync(0)` and the frame ends there - no
/// render, and crucially **no `FUN_80016B6C`**, so the SFX cue ring is neither
/// drained nor re-aged that frame. Modelling the handler as an unconditional
/// three-call sequence loses that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerFrameStage {
    /// Overlay hook called between the begin pass and the body. Only mode 13
    /// has one (`FUN_801CE850`, the world-map render tick in the slot-A
    /// overlay); `None` everywhere else.
    pub overlay_hook: Option<u32>,
    /// Which mid-frame driver runs.
    pub body: FrameBody,
    /// Whether a non-zero body return aborts before the frame-end pass.
    /// True for all three - kept explicit because it is the branch that
    /// makes the shape a state machine rather than a call list.
    pub body_can_abort: bool,
}

/// Does the per-frame handler for `mode` run the **master frame driver**
/// `FUN_80016444`?
///
/// This is [`per_frame_stage`]'s `body` field asked as a question, and it is
/// the one bit of the mode table that decides whether a frame advances the
/// actor pool at all. `FUN_80016444` is the five `FUN_8002519C` tick passes,
/// the five render passes and the display flip; a mode that does not call it
/// runs *none* of that.
///
/// Exactly one shipped mode answers `false`: 23 `CARD`, whose handler
/// `FUN_80025F74` substitutes `FUN_80017978` for the master driver
/// ([`CARD_FRAME_BODY`]). `FUN_80017978` is 18 instructions with three `jal`s
/// and no `0x80016444` among them (`0x80017978..0x800179BC`), so while the
/// pause menu owns the frame retail advances no actor, no effect and no
/// animation - only the CARD actor's own `+0x0C` handler.
///
/// INIT modes have no per-frame handler and answer `true`: they are not
/// frames, and treating them as suspended would stop the world during a
/// single-frame init.
///
/// REF: FUN_80016444, FUN_80017978
pub fn runs_master_frame_driver(mode: GameMode) -> bool {
    match per_frame_stage(mode) {
        Some(stage) => matches!(stage.body, FrameBody::Master { .. }),
        None => true,
    }
}

/// Per-frame staging plan for a mode. `None` for the INIT (even-indexed)
/// modes, which use [`mode_init_stage`] instead.
///
/// This is where the four per-frame-handler addresses are anchored, because it
/// is the only function that turns a mode id into one of them. The shapes they
/// resolve to are [`PerFrameStage`] and [`CardFrameBody`]; both are plain data
/// `struct`s with no `impl`, so a tag on either widens the port's liveness
/// verdict to the whole file instead of naming this routine.
///
/// PORT: FUN_80025eec (the default handler - 12 of the 14 per-frame modes)
/// PORT: FUN_80025f2c (mode 13 MAPDISP)
/// PORT: FUN_80025f74 (mode 23 CARD)
/// PORT: FUN_80017978 (mode 23's body, [`CARD_FRAME_BODY`])
///
/// PARTIALLY WIRED: one field of the resolved stage is load-bearing on every
/// host. [`runs_master_frame_driver`] asks this function whether the current
/// mode's handler calls `FUN_80016444`, and
/// `legaia_engine_core::world::World::tick` suspends its actor / effect /
/// move-VM passes when the answer is no. That is how the port encodes "the
/// pause menu freezes the world" - read off mode 23's `FUN_80017978`
/// substitution ([`CARD_FRAME_BODY`]) rather than written as a
/// `SceneMode::Menu` literal, so the rule cannot drift from its provenance.
/// `FUN_80025F74` and `FUN_80017978` therefore have a live caller on all three
/// hosts, and `CARD_FRAME_BODY` is no longer read only by this file's tests.
///
/// The rest of the shape is still unreached: the `overlay_hook` (mode 13's
/// `FUN_801CE850`), the master driver's `param`, and the abort branch are
/// consulted by nothing outside [`ModeDriver`], whose one remaining
/// prerequisite is a production owner. The engine's hosts drive frames from
/// `SceneHost` / `World` directly and never advance the retail mode word, so
/// there is no seat for a mode-table driver yet. `FUN_80025EEC` in particular
/// is not dead retail code - a five-form reference scan puts it in twelve slots
/// of the mode table at `0x8007078C`, every odd-indexed (per-frame) mode.
pub fn per_frame_stage(mode: GameMode) -> Option<PerFrameStage> {
    let stage = match mode {
        // Mode 13 MAPDISP - the only handler with an overlay hook, and the
        // only one that passes 0 to the master driver (`jal 0x80016444;
        // _clear a0` at 0x80025F4C).
        GameMode::MapdispMode => PerFrameStage {
            overlay_hook: Some(0x801C_E850),
            body: FrameBody::Master { param: 0 },
            body_can_abort: true,
        },
        // Mode 23 CARD - substitutes FUN_80017978 for the master driver.
        GameMode::CardMode => PerFrameStage {
            overlay_hook: None,
            body: FrameBody::CardDriver,
            body_can_abort: true,
        },
        // Every other per-frame (odd-indexed) mode routes through the
        // default handler.
        m if m.as_index() % 2 == 1 => PerFrameStage {
            overlay_hook: None,
            body: FrameBody::Master { param: 1 },
            body_can_abort: true,
        },
        _ => return None,
    };
    Some(stage)
}
