//! Spawned field-VM record context (the modal cutscene timeline).
//!
//! Retail runs a scene's scripted beats as **spawned** field-VM contexts: a
//! partition-2 record of the scene MAN dispatched by `FUN_8003BDE0` (the walk-on
//! trigger path) beside the scene-entry system script. The opening prologue
//! (`opdeene`) is the first one a game meets - its record stages the closing
//! camera path and actor `MoveTo`s and ends with `GFLAG_SET 26`, the write the
//! `town01` hand-off gate (`FUN_801D1344`) waits on - but the same context type
//! carries every walk-on vignette, `town01`'s opening and the inline-dialog
//! beats.
//!
//! [`CutsceneTimeline`] is that context's state: cursor, halt and park
//! bookkeeping (narration blocks, an owned inline dialog panel, channel
//! handshakes, player / NPC walk, glide, facing and clip waits). The driver is
//! [`crate::world::World::step_cutscene_timeline`], which runs one frame slice
//! through the shared field VM ([`legaia_engine_vm::field::step`]): camera ops
//! (`0x45`) and `MoveTo`s (`0x23`) fire by execution and emit the
//! [`crate::field_events::FieldEvent`]s the runtime camera folds in, and flag
//! writes such as `GFLAG_SET 26` land by execution rather than by a static MAN
//! walk. Installation is
//! [`crate::world::World::install_cutscene_timeline_record`].
//!
//! ## Port boundary
//!
//! No Sony bytes live here. The record body is sliced from the user's disc MAN
//! at runtime and handed in; this module only holds the per-context state.
//!
//! ## Context semantics
//!
//! Cross-context (`0x80`-bit) ops resolve onto the spawned per-actor channels
//! (one per partition-1 placement, retail `FUN_8003AEB0`'s spawn loop), so a
//! vignette's pokes land on real per-actor contexts, and a `B3 <id> <bit>`
//! handshake parks the timeline until the channel answers. The inline
//! narration op (`0xCC 0xF8 0x80 N`, which retail routes to the `FUN_8003C764`
//! text-balloon path) is presented by
//! [`crate::cutscene_narration::CutsceneNarration`]; the timeline suspends at
//! each block while its pages play, and the actor-allocator host hook is
//! scoped off while a modal timeline steps.

use legaia_engine_vm::field::FieldCtx;

/// One executed instruction in a timeline op-stream trace.
///
/// Recorded by [`crate::world::World::step_cutscene_timeline`] when the
/// timeline's [`CutsceneTimeline::trace_enabled`] flag is set. The trace is the
/// engine VM's *authoritative* decode of the record bytecode - it follows the
/// real per-op PC stride, so it never drifts the way a linear disassembler does
/// through the variable-width `0x4C` menu-control op. Used to correlate which
/// field-VM op opens a downstream UI (e.g. the `town01` opening's name-entry
/// prompt) against a save-state oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEntry {
    /// Byte offset of the opcode in the record bytecode.
    pub pc: usize,
    /// Raw opcode byte, including the `0x80` cross-context (extended) bit.
    pub opcode_byte: u8,
    /// Decoded opcode (`opcode_byte & 0x7F`).
    pub opcode: u8,
    /// PC after the step (the resume / advance target).
    pub next_pc: usize,
    /// How the VM resolved this step.
    pub result: TraceResult,
}

/// The [`legaia_engine_vm::field::StepResult`] discriminant for a [`TraceEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceResult {
    /// Normal advance to the next instruction.
    Advance,
    /// Per-frame yield (the VM ran until a `YIELD`).
    Yield,
    /// Held at PC (WAIT_FRAMES / conditional hold / un-advanceable op).
    Halt,
    /// A host hook fired but the op needs more support to advance.
    Pending,
    /// Unknown / out-of-range opcode.
    Unknown,
}

/// An inline narration block inside a timeline record: the byte offset of its
/// introducing op (`0xCC 0xF8 0x80 N`), the offset just past its last page's
/// terminator, and the decoded pages.
///
/// Retail routes the introducing op to the on-screen-text spawner
/// (`FUN_8003C764`): a caption child context is spawned over the inline pages
/// and the *parent* timeline halt-suspends at the op until the child exhausts
/// them - so the choreography around a block runs between blocks, never under
/// them. [`crate::world::World::step_cutscene_timeline`] mirrors that: when the
/// timeline PC reaches `op_offset` it installs the pages on the
/// [`crate::cutscene_narration::CutsceneNarration`] presenter and parks until
/// the presenter completes, then resumes at `end`.
// REF: FUN_8003C764
#[derive(Debug, Clone)]
pub struct NarrationSite {
    /// Byte offset of the introducing `0x4C` narration op in the record body.
    pub op_offset: usize,
    /// Byte offset just past the block (the next opcode after the pages).
    pub end: usize,
    /// The decoded subtitle pages, in display order.
    pub pages: Vec<String>,
    /// Presentation form: a crawl suspends the timeline while the roller
    /// plays; a static title card installs (or, when its pages are blank,
    /// clears) the card overlay and the timeline continues.
    pub kind: legaia_asset::cutscene_text::NarrationKind,
}

/// A cross-context channel wait the timeline is PARKED on.
///
/// The retail opdeene timeline halt-acquires its vignette channels (a `4C 85`
/// freeze sweep), pokes each beat by beat, then waits on a per-channel flag
/// via `B3 <id> <bit>` = op `0x33` (CFLAG_TST) with the cross-context (`0x80`)
/// bit set, targeting the channel's `ctx[+0x50]` id and testing
/// `ctx.flags & (1 << bit)`. The arm holds the caller at the flag-test PC
/// **while the bit is SET** and advances once it is clear: it bumps `s8` by 2
/// at `0x801DEE2C`, takes that advanced PC on a zero mask (`beq` at
/// `0x801DEE44`) and otherwise restores the entry PC `s4` (`0x801DEE4C`). So
/// the wait is on a busy bit dropping, not on a completion bit rising.
///
/// [`crate::world::World::step_cutscene_timeline`] models that park with this
/// record instead of stepping past the flag-test by instruction width: it
/// leaves the PC on the op and, each subsequent tick, re-tests the awaited
/// channel's flag, resuming past the op once the bit is clear (bounded by
/// [`crate::world::CHANNEL_WAIT_PARK_TIMEOUT`] so a channel our port never
/// clears falls back to the by-width step-past).
///
/// Bit 10 (`0x400`, the halt/busy bit the acquire sweep toggles) is excluded -
/// a `B3 <id> 0A` is a suspension *verify*, not a completion wait, and keeps
/// the width step-past.
// REF: FUN_8003BDE0 (timeline dispatch)
// REF: FUN_8003C83C (cross-context target resolve)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelWait {
    /// Cross-context target id (`ctx[+0x50]`) the timeline is waiting on - the
    /// raw operand byte resolved through
    /// [`crate::field_channels::resolve_target`].
    pub target_id: u8,
    /// Context-flag bit index (`ctx.flags & (1 << bit)`) that signals the
    /// awaited channel's beat completed.
    pub bit: u8,
    /// Frames the timeline has been parked here, bounded by
    /// [`crate::world::CHANNEL_WAIT_PARK_TIMEOUT`].
    pub frames: u32,
}

/// A spawned field-VM context running the `opdeene` cutscene-timeline record.
///
/// Built by [`crate::world::World::load_cutscene_timeline_from_man`] from the
/// partition-2 record that issues `GFLAG_SET 26`; stepped by
/// [`crate::world::World::step_cutscene_timeline`] until it executes that write
/// (the timeline's terminal op) or its safety cap forces it complete.
#[derive(Debug, Clone)]
pub struct CutsceneTimeline {
    /// The spawned cutscene context (camera / lead-actor anchor). Its
    /// `script_id` is set to the system channel (`0xFB`) so cross-context
    /// (`0x80`-bit) ops keep running after the record's first `YIELD` sets the
    /// context halt bit - see the `step` prelude halt carve-out.
    pub ctx: FieldCtx,
    /// The partition-2 record body, sliced from its `script_start` so relative
    /// jumps wrap against the record base (retail `buffer_base = script_start`).
    /// Shared (`Arc`) so an inline dialog panel can page over the same bytes
    /// while the timeline is parked at the segment.
    pub bytecode: std::sync::Arc<Vec<u8>>,
    /// Current byte offset into [`Self::bytecode`]. Starts at the record's
    /// first-opcode offset (`pc0`, the named-record header end).
    pub pc: usize,
    /// Set once the timeline completes - it executed its closing `GFLAG_SET 26`,
    /// hit an op it cannot advance past, or exceeded its frame cap.
    pub done: bool,
    /// Frames the timeline has been stepping (for the safety cap).
    pub frames: u32,
    /// When `true`, [`crate::world::World::step_cutscene_timeline`] appends a
    /// [`TraceEntry`] per executed instruction to [`Self::trace`]. Off by
    /// default (no overhead on the normal opening path); turned on for the RE
    /// op-stream correlation harness.
    pub trace_enabled: bool,
    /// The recorded op stream when [`Self::trace_enabled`] is set.
    pub trace: Vec<TraceEntry>,
    /// When `true`, this timeline's terminal op is the `opdeene` prologue's
    /// `GFLAG_SET 26`, so completing it (or hitting the frame cap) arms the
    /// `town01` hand-off. The `town01` opening timeline sets this `false` - it
    /// drives the establishing shot + name-entry handoff, not a scene change,
    /// so it must never arm a prologue hand-off. See
    /// [`crate::world::World::step_cutscene_timeline`].
    pub arms_prologue_handoff: bool,
    /// The record's inline narration blocks, in script order (parsed once at
    /// install). Every crawl block opens its roller **non-blocking** - the
    /// retail roller is a child context and the parent keeps executing, so the
    /// camera cuts / fades / `WaitFrames` authored after a block play UNDER
    /// the scrolling text. (The `map01` fly-in pins this for the LAST block
    /// too: its authored `4A` 600 + 330 tail runs concurrent with the 3-page
    /// Mist crawl in the retail capture - serializing them overshoots the leg
    /// by the whole roller duration.) The final pages are protected by the
    /// terminal-SceneChange hold in the stepper, not by a park at the block.
    pub narration_blocks: Vec<NarrationSite>,
    /// `Some(op_offset)` while the timeline is held AT that narration block's
    /// op because a PRIOR roller is still scrolling - two rollers never stack,
    /// so the block waits for the active one to drain before opening. The
    /// pre-step gate then re-enters the block's op to open it.
    pub narration_pc: Option<usize>,
    /// Kept alongside [`Self::narration_pc`]; always `true` while a hold is
    /// live (the only remaining hold shape is "waiting to OPEN this block
    /// once the prior roller drains").
    pub narration_pending_open: bool,
    /// An open inline dialog box (`0x1F`-lead glyph segment reached by the
    /// record's own flow, e.g. the Mei walk-on beat's conversation). While
    /// `Some`, the timeline is parked at the segment lead - the retail dialog
    /// state machine's `pc byte & 0x7F < 0x20` transition - and the stepper
    /// routes pad input to the panel (confirm advances / dismisses, Up/Down
    /// move a picker cursor). On dismissal the timeline resumes at the
    /// panel's final PC (past the consumed segment).
    pub dialog: Option<crate::dialog::OwnedDialogPanel>,
    /// When [`Self::dialog`] was opened, in
    /// [`crate::world::FieldVmState::dialog_claims`] order - which of several
    /// contexts holding text got to the shared box first.
    pub dialog_claim: u64,
    /// Per-byte "an instruction was executed here" map over
    /// [`Self::bytecode`], kept for the timeline's whole life. A backward
    /// jump into an already-executed PC means the record's linear
    /// choreography has wrapped - the on-disc records have no end opcode;
    /// they either park in a tight `Nop`+`JmpRel`-to-self spin or loop as a
    /// **resident** actor-driver context (e.g. the town01 Mei beat re-enters
    /// its conversation loop from the top). Retail leaves that context
    /// looping as a *parallel* context; the engine's modal timeline
    /// completes there instead so control returns to the player.
    pub visited: Vec<bool>,
    /// `Some` while the timeline is PARKED on a cross-context channel
    /// handshake (`B3 <id> <bit>` CFLAG_TST); see [`ChannelWait`]. The stepper
    /// leaves the PC on the flag-test op and resumes past it once the awaited
    /// channel's bit is clear (or the park times out).
    pub channel_wait: Option<ChannelWait>,
    /// Frames remaining on an in-flight **player-channel move**: armed when
    /// the timeline executes an ExecMove against the player-anchor target
    /// `0xF8` (`A2 F8 <move_id>`). Retail resolves `0xF8` to the live player
    /// object (`_DAT_8007C364`, `FUN_8003C83C`) and lets the poked move-table
    /// clip play out over the following frames; the engine's player pokes
    /// complete synchronously, so this countdown stands in for the playout -
    /// a following player-channel halt-acquire parks until it drains. Armed
    /// to [`crate::world::CHANNEL_WAIT_PARK_TIMEOUT`]; decremented once per
    /// parked tick.
    // REF: FUN_8003C83C
    pub player_move_frames: u32,
    /// `Some(step_past_width)` while the timeline is PARKED at a
    /// **player-channel arc jump** (`C3 F8 <sub> …` = op `0x43`
    /// sub-0/1/A/B against `0xF8`). Retail raises the caller's halt bit and
    /// arcs the player to the operand's landing tile (`FUN_801D25EC`); the
    /// arc's release watcher clears the caller's halt on landing, and the
    /// context resumes at the **next instruction** - the operand halfwords
    /// are the arc's apex / frame count, not a resume PC (see
    /// `docs/subsystems/script-vm.md`, "0x43 sub-0/1/A/B"). The engine parks
    /// at the op while `World::player_script_arc_live` holds - or, when no arc
    /// could start, until [`Self::player_move_frames`] drains - then steps
    /// PAST it by this encoded width, so the record reaches its trailing ops
    /// (the door record's terminal `0x3F` scene change).
    // REF: FUN_8003BDE0
    pub player_wait: Option<usize>,
    /// When `true`, completing this timeline un-parks every NPC left at the
    /// off-map hide box ([`crate::world::World::restore_hidden_field_npcs`]).
    /// Set ONLY by the `town01` opening install: that record hides the
    /// townsfolk for its establishing shot and nothing reloads the scene
    /// before free-roam, so the overrides must be dropped explicitly.
    /// A mid-scene walk-on beat keeps the default `false` - a record that
    /// seats an actor at the hide box did so as story choreography (the Mei
    /// beat's closing `4C 51` walks her out of Vahn's house and despawns
    /// her; retail leaves her hidden until the next scene entry re-runs her
    /// spawn prologue), and restoring would resurrect the ghost in-room.
    pub restore_hidden_on_complete: bool,
    /// `Some` while the timeline is PARKED on a cross-context **walk-to-tile
    /// yield** (`C7 <id> <tx> <tz> <mode>` = op `0x47` against an NPC channel
    /// or the player anchor `0xF8`). Retail's dispatcher saves the yield-op
    /// pointer into the TARGET actor's `+0x94` and sets its `0x400` walk bit;
    /// the per-frame walk kernel (`FUN_8003774C` case `0x47`) then moves the
    /// actor toward the decoded tile at `0x80 >> (2 + (mode & 7))` units per
    /// frame, and the parked record resumes when it arrives. The engine
    /// mirrors that with a motion-VM glide on the target and this park.
    // REF: FUN_8003774C (case 0x47: walk-to-tile interpreted in place)
    pub walk_wait: Option<TimelineWalk>,
    /// `Some` while the timeline is PARKED on a cross-context **rotate
    /// yield** (`B8 <id> <dir|flags> <budget|dir>` = op `0x38` CAM_CFG with a
    /// non-zero budget against an NPC channel). Retail's halt-acquire arm
    /// parks the record and the yield-op bytes run in place as the
    /// `FUN_8003774C` `0x38` RotateToAngle leg on the target actor: a linear
    /// per-frame ramp (`arc * speed / frames_remaining`, raw pre-unwrap
    /// write-back) onto the compass-LUT entry over the operand's own frame
    /// budget, terminal frame snapping exactly. This is where a story beat's
    /// per-op turn rates come from - the budget is op data, not an engine
    /// constant. The engine mirrors it with a parked motion-VM rotate leg
    /// writing the NPC's render heading each frame.
    // REF: FUN_8003774C (case 0x38: rotate-to-angle interpreted in place)
    pub facing_wait: Option<TimelineFacing>,
    /// `Some` while the timeline is PARKED on a cross-context
    /// **halt-acquire of the player** (`CC F8 85|8E|8F <lo> <hi> <id>`).
    /// Retail's acquire arm (`0x801E2148..0x801E21DC`) stores the op's
    /// address into the player's `+0x94` and raises `0x400` on the player
    /// and on the calling record; the walk kernel `FUN_8003774C` then reads
    /// the bytes back as its `0x4C` FaceTarget leg on the player - turn
    /// toward the actor bind `<id>` over the `u16` frame budget - and its
    /// terminal frame clears both halt bits (`0x80038004` / `0x80038028`).
    /// So the player turns to face the named actor and the record waits for
    /// the turn. The leg is the one the talk runner plays
    /// ([`crate::inline_dialogue::TalkFaceRamp`]); `.1` is the PC past the
    /// op, `.2` the ticks parked so far.
    // REF: FUN_801DE840 (the acquire arm), FUN_8003774C (the 0x4C arm)
    pub player_face: Option<(crate::inline_dialogue::TalkFaceRamp, usize, u32)>,
    /// `Some` while a cross-context **compass walk** this timeline armed
    /// against the player plays (`B7 F8 <b0> <b1>` / `C1 F8 <b0> <b1>` = op
    /// `0x37` / `0x41` with the player-anchor target). Retail's dispatcher
    /// seats the op on the player's `+0x94`, raises its `0x400` and advances
    /// the record past the op; the record's next cross-context op on the
    /// player waits until the leg lands. The walk kernel
    /// (`FUN_8003774C`, arm `0x8003789C..0x800379F8`) translates the player
    /// along one of eight compass directions for `(b1 & 0x3F) * (4 << sel)`
    /// speed units, spending `DAT_1F800393` of them per game tick - one per
    /// vsync. `map01`'s cave-mouth record walks the player out of the cave
    /// this way (`B7 F8 00 81`: one tile along `-Z` over sixteen vsyncs).
    // REF: FUN_8003774C (the 0x37 / 0x41 arm interpreted in place)
    pub player_glide: Option<TimelinePlayerGlide>,
    /// Compass walks this timeline armed against **NPC** placements
    /// (`B7 <id> <b0> <b1>` / `C1 <id> ..` on a placement channel). The same
    /// arm as [`Self::player_glide`]: retail seats the op on the target's
    /// `+0x94`, raises its `0x400` and advances the record past the op
    /// (`s7 = 3`, taken for every target), so the record runs on and its
    /// next cross-context op on that actor waits until the leg lands.
    /// bylon's first Maya meeting (`P2[9]`) walks her down from the shrine
    /// stairs this way (`B7 3F 00 84`: 512 units along `-Z`) before she
    /// speaks.
    // REF: FUN_801DE840 (0x801DEE90..0x801DEF1C), FUN_8003774C (the 0x37 / 0x41 arm)
    pub npc_glides: Vec<TimelineNpcGlide>,
    /// Placement slots of the **NPC walk-to-tile legs** this context armed
    /// (`C7 <id> <tx> <tz> <mode>` against a placement channel) and that are
    /// still walking. Retail's op-`0x47` arm (`0x801DEFC0..0x801DF054`) seats
    /// the op on the target's `+0x94`, raises its `0x400` and advances the
    /// record past the op - `li s7,4` sits in the delay slot at `0x801DF030`,
    /// so the advance is taken for every target, and only a **player**
    /// target also parks the caller (`0x801DF034..0x801DF044`). The record
    /// therefore runs on while the NPC walks; its next cross-context op on
    /// that actor waits for the leg to land (the halted-target refusal).
    // REF: FUN_801DE840 (0x801DEFC0..0x801DF054), FUN_8003774C (case 0x47)
    pub npc_walks: Vec<u8>,
    /// Budgeted **NPC rotate legs** this context armed (`B8 <id> <dir>
    /// <budget>` against a placement channel), stepped once per tick. The
    /// op-`0x38` budget arm advances the record by 3 for every target - the
    /// `li s7,3` at `0x801DEEFC` sits in a delay slot - and parks the caller
    /// only for a **player** target (`0x801DEF04..0x801DEF14`), so a beat
    /// that turns an NPC runs on while the turn plays out; its next
    /// cross-context op on that NPC waits for the ramp to snap.
    // REF: FUN_801DE840 (0x801DEE58..0x801DEF24), FUN_8003774C (case 0x38)
    pub npc_facings: Vec<TimelineFacing>,
    /// Face-at legs this context armed on **NPC** placements
    /// (`CC <id> 85|8E|8F <lo> <hi> <bind>` against a placement channel):
    /// the same halt-acquire the player form takes
    /// ([`Self::player_face`]), on another target. The acquire arm
    /// (`0x801E2148..0x801E21DC`) stores the op's address into the target's
    /// `+0x94` and raises its `0x400`, and advances the caller by five
    /// (`li s7,5` at `0x801E21B8`); only a **player** target also halts the
    /// caller (`0x801E21B4..0x801E21CC`). The target's actor tick then runs
    /// the walk kernel's `0x4C` FaceTarget leg over the op's frame budget,
    /// turning it toward the bind, and the terminal frame clears its
    /// `0x400` (`0x80038004`). So the record runs on while the actor turns,
    /// and its next cross-context op on that actor waits for the turn.
    /// Cutscene records carry well over a thousand of these on NPCs and as
    /// many again on the party placements (Noa / Gala turning to face Vahn).
    // REF: FUN_801DE840 (0x801E2148..0x801E21DC), FUN_8003774C (the 0x4C arm)
    pub npc_faces: Vec<TimelineNpcFace>,
    /// Ticks left on the **scene-bank** clip the timeline last poked onto the
    /// player (`A2 F8 <move_id>` with the party-bank bit down): the clip's
    /// end-latch length at its own step ([`crate::field_anim::clip_end_ticks`]).
    /// Retail's clip tick `FUN_800204F8` latches the end flag `0x100` into
    /// the player's `+0x62` when the cursor reaches the last frame
    /// (`0x800206E4..0x8002072C`), and a record waits for it with
    /// `AC F8 08` / `AD F8 08`. Counted down once per slice; `0` when no
    /// clip is in flight or its length is unknown.
    pub player_clip_ticks: u32,
    /// `Some(step_past_width)` while the timeline is PARKED on a player
    /// end-latch spin (`AD F8 08`, op `0x2D` LFLAG_TST bit 8 against the
    /// player) with [`Self::player_clip_ticks`] still running. Released, and
    /// stepped past by this width, when the countdown drains.
    // REF: FUN_800204F8 (the latch), FUN_80039B7C (the per-frame re-entry)
    pub player_clip_wait: Option<usize>,
    /// Consecutive frames the record has spun on an **NPC** end latch
    /// (`AD <id> 08` against a placement whose clip cursor the world owns).
    /// The spin re-tests each frame until the actor's clip tick
    /// (`FUN_800204F8`) latches `+0x62 & 0x100`; past
    /// [`crate::world::NPC_CLIP_SPIN_TIMEOUT`] frames (a held clip never
    /// latches) the record steps past by width instead.
    // REF: FUN_800204F8 (the latch), FUN_801DE840 (op 0x2D)
    pub npc_clip_spin_frames: u32,
    /// `true` once the context has taken its first frame slice. A spawned
    /// helper holds the pad from then on (retail's engaged bit is raised by
    /// the script runner's step, not by the spawn); see
    /// `World::script_context_engages_player`.
    pub stepped: bool,
    /// `Some(slot)` when this timeline is placement `slot`'s OWN parked
    /// context resumed by a touch (the boss-stager dispatch), not a spawned
    /// record. Retail has one context per placement: the touch raises its
    /// engaged bit (`+0x10 & 0x100`) and the script runner `FUN_80039B7C`
    /// steps it from its parked PC until it executes a raw `0x21`, where the
    /// interaction ends (`0x80039E20` exits the loop on `0x21`,
    /// `0x80039E68..0x80039E7C` clears `0x100`). So such a timeline starts at
    /// the placement channel's PC, completes at its first executed `0x21`,
    /// hands its PC back to the channel, and the channel does not step on
    /// its own while the timeline holds it.
    // REF: FUN_80039B7C
    pub interaction_slot: Option<u8>,
}

/// State of a parked player compass walk (see
/// [`CutsceneTimeline::player_glide`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelinePlayerGlide {
    /// The walk leg: position seeded from the player, `op_accum` the spent
    /// units (`+0x54`), `speed` 1 (one unit per engine tick = per vsync).
    pub state: legaia_engine_vm::motion_vm::MotionState,
    /// Direction / divisor-selector byte.
    pub body0: u8,
    /// Length / divisor-selector byte.
    pub body1: u8,
    /// `0x80` for op `0x37`, `0x40` for op `0x41`.
    pub rate: i32,
    /// PC past the op (`yield pc + 4`), where the record runs on while
    /// the leg plays.
    pub resume_pc: usize,
    /// Ticks the leg has played, bounded by the walk park timeout.
    pub frames: u32,
}

/// State of an NPC compass walk (see [`CutsceneTimeline::npc_glides`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineNpcGlide {
    /// Placement slot of the gliding NPC.
    pub slot: u8,
    /// The walk leg: position seeded from the NPC, `op_accum` the spent
    /// units (`+0x54`), `speed` 1 (one unit per vsync).
    pub state: legaia_engine_vm::motion_vm::MotionState,
    /// Direction / divisor-selector byte.
    pub body0: u8,
    /// Length / divisor-selector byte.
    pub body1: u8,
    /// `0x80` for op `0x37`, `0x40` for op `0x41`.
    pub rate: i32,
    /// Ticks the leg has played, bounded by the walk park timeout.
    pub frames: u32,
}

/// An NPC face-at leg (see [`CutsceneTimeline::npc_faces`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineNpcFace {
    /// Placement slot of the turning NPC.
    pub slot: u8,
    /// The walk-kernel leg: `[0x4C, sub-mode, budget lo, budget hi, bind]`.
    pub ramp: crate::inline_dialogue::TalkFaceRamp,
    /// Ticks the leg has played, bounded by the walk park timeout.
    pub frames: u32,
}

/// State of a parked cross-context rotate yield (see
/// [`CutsceneTimeline::facing_wait`]): one motion-VM `0x38` RotateToAngle leg
/// stepped once per timeline tick against the target's render heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineFacing {
    /// Placement slot of the turning NPC, or `None` for the player
    /// (`B8 F8 ..`).
    pub slot: Option<u8>,
    /// The parked rotate leg: yaw seeded from the NPC's current heading,
    /// speed 1 (the engine ticks the timeline once per retail display frame,
    /// so the operand budget maps 1:1 to parked ticks).
    pub state: legaia_engine_vm::motion_vm::MotionState,
    /// The yield-op bytes re-encoded for the in-place kernel:
    /// `[0x38, dir|flags, budget|dir]`.
    pub program: [u8; 3],
    /// PC to resume at once the ramp snaps (`yield pc + 4`).
    pub resume_pc: usize,
    /// Ticks parked so far, bounded by the walk park timeout.
    pub frames: u32,
}

/// State of a parked cross-context walk-to-tile yield (see
/// [`CutsceneTimeline::walk_wait`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineWalk {
    /// Placement slot of the walking NPC, or `None` for the player anchor
    /// (`C7 F8 …`).
    pub slot: Option<u8>,
    /// Decoded world-coordinate target (tile centre + half-tile bits).
    pub target: (i16, i16),
    /// PC to resume at once the walk arrives (`yield pc + 5`).
    pub resume_pc: usize,
    /// Per-tick step magnitude (`field_npc_walk_step_speed(0x80, mode & 7)`).
    pub speed: u16,
    /// Ticks parked so far, bounded by the walk park timeout.
    pub frames: u32,
}

impl CutsceneTimeline {
    /// The actors this context's in-place park is moving: its walk-to-tile
    /// leg (`C7 <id|F8> ..`), rotate leg (`B8 <id> ..`) or player compass
    /// glide (`B7 F8 ..` / `C1 F8 ..`) or NPC compass glides (`B7 <id> ..`). `None` is the player, `Some(slot)` an
    /// NPC placement. Retail's park leaves the target's halt bit `0x400` set
    /// until the walk kernel lands it.
    pub fn halted_targets(&self) -> impl Iterator<Item = Option<u8>> + '_ {
        let live = !self.done;
        let walk = self.walk_wait.as_ref().map(|w| w.slot);
        let facing = self.facing_wait.as_ref().map(|f| f.slot);
        let glide = self.player_glide.as_ref().map(|_| None);
        [walk, facing, glide]
            .into_iter()
            .flatten()
            .chain(self.npc_glides.iter().map(|g| Some(g.slot)))
            .chain(self.npc_walks.iter().map(|&s| Some(s)))
            .chain(self.npc_facings.iter().map(|f| f.slot))
            .chain(self.npc_faces.iter().map(|f| Some(f.slot)))
            .filter(move |_| live)
    }

    /// System-channel id for the spawned context (see [`Self::ctx`]).
    const SYSTEM_SCRIPT_ID: u16 = 0xFB;

    /// Build a timeline over `bytecode` starting at `pc` (the record's
    /// first-opcode offset). The context is seeded on the system channel so
    /// cross-context ops survive the first `YIELD`.
    pub fn new(bytecode: Vec<u8>, pc: usize) -> Self {
        let ctx = FieldCtx {
            script_id: Self::SYSTEM_SCRIPT_ID,
            ..FieldCtx::default()
        };
        let visited = vec![false; bytecode.len()];
        Self {
            ctx,
            bytecode: std::sync::Arc::new(bytecode),
            pc,
            done: false,
            frames: 0,
            trace_enabled: false,
            trace: Vec::new(),
            arms_prologue_handoff: false,
            narration_blocks: Vec::new(),
            narration_pc: None,
            narration_pending_open: false,
            dialog: None,
            dialog_claim: 0,
            visited,
            channel_wait: None,
            player_move_frames: 0,
            player_wait: None,
            restore_hidden_on_complete: false,
            walk_wait: None,
            facing_wait: None,
            player_face: None,
            player_glide: None,
            npc_glides: Vec::new(),
            npc_walks: Vec::new(),
            npc_facings: Vec::new(),
            npc_faces: Vec::new(),
            player_clip_ticks: 0,
            player_clip_wait: None,
            npc_clip_spin_frames: 0,
            stepped: false,
            interaction_slot: None,
        }
    }

    /// `true` once the timeline has completed.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Enable op-stream tracing (see [`Self::trace`]). Returns `self` for
    /// builder-style use on the RE correlation harness.
    pub fn with_trace(mut self) -> Self {
        self.trace_enabled = true;
        self
    }

    /// Mark this timeline as the `opdeene` prologue (its terminal `GFLAG_SET 26`
    /// arms the `town01` hand-off; see [`Self::arms_prologue_handoff`]).
    pub fn arming_prologue_handoff(mut self) -> Self {
        self.arms_prologue_handoff = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_seeds_system_channel_and_pc() {
        let tl = CutsceneTimeline::new(vec![0x21, 0x2E, 0x1A], 1);
        assert_eq!(tl.ctx.script_id, CutsceneTimeline::SYSTEM_SCRIPT_ID);
        assert_eq!(tl.pc, 1);
        assert!(!tl.is_done());
        assert_eq!(tl.frames, 0);
        assert!(!tl.trace_enabled);
    }

    #[test]
    fn with_trace_enables_tracing() {
        let tl = CutsceneTimeline::new(vec![0x21], 0).with_trace();
        assert!(tl.trace_enabled);
        assert!(tl.trace.is_empty());
    }
}
