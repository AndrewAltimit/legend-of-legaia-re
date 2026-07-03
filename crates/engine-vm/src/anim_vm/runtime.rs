use super::*;
use legaia_anm::{AnimPlayer, PoseFrame};

/// Per-actor animation state.
///
/// `Idle` slots hold no record. `Playing` slots own the record bytes,
/// the bone count, and either an `AnimPlayer` (for `RecordKind::Keyframe`)
/// or a frame counter for the opaque kinds. The runtime advances the
/// counter every `tick`; the host fills in the per-kind side-effects.
#[derive(Debug, Clone, Default)]
pub enum AnimSlot {
    /// No animation is registered on this actor.
    #[default]
    Idle,
    /// A keyframe-style animation is playing - handled natively.
    Keyframe {
        player: AnimPlayer,
        kind: RecordKind,
    },
    /// An opaque-kind record is registered; the runtime advances a
    /// frame counter and surfaces it to the host on each tick. The host
    /// owns interpretation.
    Opaque {
        record: Vec<u8>,
        bone_count: usize,
        kind: RecordKind,
        /// Per-frame counter (mirrors retail `actor[+0x68]`, which
        /// `FUN_80024CFC` initialises to 100).
        frame_counter: u32,
    },
}

impl AnimSlot {
    /// Helper: explicit constructor for the idle variant.
    pub fn idle() -> Self {
        AnimSlot::Idle
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, AnimSlot::Idle)
    }

    /// Header `a` byte for the active record, or `None` if idle. Useful
    /// when routing by record kind without re-parsing.
    pub fn header_a(&self) -> Option<u16> {
        match self {
            AnimSlot::Idle => None,
            AnimSlot::Keyframe { .. } => Some(0x06),
            AnimSlot::Opaque { kind, .. } => match kind {
                RecordKind::Kind2 => Some(0x02),
                RecordKind::Kind3 => Some(0x03),
                RecordKind::KindA => Some(0x0A),
                RecordKind::Keyframe => Some(0x06),
                RecordKind::Other(v) => Some(*v),
            },
        }
    }
}

/// Frame events surfaced by the runtime. Engines drain these to
/// drive renderer / SFX side-effects without inspecting actor state.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimEvent {
    /// A keyframe pose was produced for this actor on this frame.
    /// `pose.finished` mirrors `PoseFrame::finished` (true after the
    /// non-looping cycle has clamped at 0xFF).
    PoseUpdated { actor: u8, pose: PoseFrame },
    /// An opaque-kind record advanced one frame; the host's
    /// `on_opaque_record` hook saw it. The runtime only reports that
    /// the tick happened - the host is responsible for any side
    /// effect (sprite frame swap, voice cue, etc.).
    OpaqueTick {
        actor: u8,
        kind: RecordKind,
        frame: u32,
    },
    /// A non-looping animation has finished. Engines typically clear
    /// the slot or transition to an idle pose.
    Finished { actor: u8, kind: RecordKind },
    /// `play` was called on a slot that was already busy. Host can
    /// treat this as either a forced replace or a no-op.
    Replaced { actor: u8 },
}

/// Engine-side callbacks the runtime makes for opaque-kind records.
///
/// The retail per-frame consumer at `FUN_8004AD80` dispatches on the
/// consumer-struct kind byte at `+0x00` (an [`OpaqueRecordKind`]
/// classification - not the same byte as the [`RecordKind`] header
/// `a` field, though they happen to live at the same offset in the
/// runtime record) into five explicit branches plus a default "raw
/// playback" path. The runtime here invokes one trait method per
/// branch so the engine can supply per-kind behaviour without having
/// to re-classify in user code.
///
/// `FUN_8004AD80` also calls several per-frame helpers as it walks
/// each opaque slot (`FUN_80047430` movement step, `FUN_80048A08`
/// render loop, `FUN_80049348` child-step iterator, `FUN_8004998C`
/// per-bone interpolator, and `FUN_8004E13C` special-effect spawn).
/// The corresponding trait methods are exposed here as host-trait
/// abstractions; engines call them from their per-kind handlers in
/// whatever order matches the SCUS contract.
///
/// `on_opaque_record` remains the catch-all fallback - every per-kind
/// method defaults to routing through it, so existing host
/// implementations that only override `on_opaque_record` continue to
/// work.
pub trait Host {
    /// Catch-all fallback: called by the default body of every
    /// per-kind dispatcher below. Engines that want a single
    /// handler for every kind can override just this method; engines
    /// that want per-kind specialisation override the per-kind
    /// methods directly.
    ///
    /// `frame_counter` mirrors `actor[+0x68]` and increments each
    /// tick while the slot is alive. Returning `true` keeps the slot
    /// alive; `false` ends it (transitions to `Idle` on the next
    /// tick boundary).
    fn on_opaque_record(
        &mut self,
        actor: u8,
        kind: RecordKind,
        record: &[u8],
        frame_counter: u32,
    ) -> bool {
        let _ = (actor, kind, record, frame_counter);
        true
    }

    /// `OpaqueRecordKind::Kind2` (`+0x00 == 0x02`) handshake. When
    /// the global character-action byte is `-0x4D` / `-0x4B` and the
    /// actor's anim flag at `+0x14C` is zero, the consumer flips the
    /// kind byte to `Kind4` and ticks `+0x56`. Per-kind port of the
    /// `Kind2` branch in `FUN_8004AD80`.
    fn on_kind2_handshake(&mut self, actor: u8, record: &[u8], frame_counter: u32) -> bool {
        self.on_opaque_record(actor, RecordKind::Kind2, record, frame_counter)
    }

    /// `OpaqueRecordKind::Kind4` (`+0x00 == 0x04`) action engaged. If
    /// the actor's anim-flag at `+0x14C` is zero, set
    /// `actor[+0x1DA] = 7`; otherwise copy `actor[+0x1F2]` into
    /// `+0x1DA`. Per-kind port of the `Kind4` branch in
    /// `FUN_8004AD80`.
    fn on_kind4_action_engaged(&mut self, actor: u8, record: &[u8], frame_counter: u32) -> bool {
        self.on_opaque_record(actor, RecordKind::Other(0x04), record, frame_counter)
    }

    /// `OpaqueRecordKind::Kind5` (`+0x00 == 0x05`): `actor[+0x1DC] |=
    /// 4`. Per-kind port of the `Kind5` branch in `FUN_8004AD80`.
    fn on_kind5_or_bit2(&mut self, actor: u8, record: &[u8], frame_counter: u32) -> bool {
        self.on_opaque_record(actor, RecordKind::Other(0x05), record, frame_counter)
    }

    /// `OpaqueRecordKind::Kind7` (`+0x00 == 0x07`): set
    /// `actor[+0x1DA] = 8`. Per-kind port of the `Kind7` branch in
    /// `FUN_8004AD80`.
    fn on_kind7_set_da_8(&mut self, actor: u8, record: &[u8], frame_counter: u32) -> bool {
        self.on_opaque_record(actor, RecordKind::Other(0x07), record, frame_counter)
    }

    /// `OpaqueRecordKind::Kind8` (`+0x00 == 0x08`): `actor[+0x1DC] |=
    /// 8`. Per-kind port of the `Kind8` branch in `FUN_8004AD80`.
    fn on_kind8_or_bit3(&mut self, actor: u8, record: &[u8], frame_counter: u32) -> bool {
        self.on_opaque_record(actor, RecordKind::Other(0x08), record, frame_counter)
    }

    /// Catch-all branch for `OpaqueRecordKind::Other(byte)`: raw
    /// playback. The default `FUN_8004AD80` path runs the per-field
    /// reads but no kind-specific sub-state mutates.
    fn on_other_kind(&mut self, actor: u8, byte: u8, record: &[u8], frame_counter: u32) -> bool {
        self.on_opaque_record(actor, RecordKind::Other(byte.into()), record, frame_counter)
    }

    /// Per-frame movement step - host-trait abstraction of
    /// `FUN_80047430`. Reads `OpaqueAnimRecord::movement_scale` and
    /// `OpaqueAnimRecord::nested_data_ptr_raw`, computes
    /// `(angle_lookup * movement_scale * frame_index) / frame_count`,
    /// applies the translation to the actor's world position.
    ///
    /// Default no-op so engines without a movement pipeline still
    /// build.
    fn on_movement_step(&mut self, actor: u8, record: &[u8], frame_index: u16) {
        let _ = (actor, record, frame_index);
    }

    /// Per-frame render-loop body - host-trait abstraction of
    /// `FUN_80048A08`. Reads `+0x84` (`depth_84`) and
    /// `count_86` to drive a per-frame primitive emission loop.
    fn on_render_loop(&mut self, actor: u8, record: &[u8]) {
        let _ = (actor, record);
    }

    /// Child-step iterator - host-trait abstraction of
    /// `FUN_80049348`. Reads the actor's `+0x21D`
    /// ([`ACTOR_LOD_STEP_OFFSET`]) byte and iterates the actor's
    /// child-actor chain at `+0x1FB..` with stride `lod_step`.
    /// `lod_step` is the folded `8 / max(stride, 1)` value (see
    /// [`ActorAnimState::lod_step_factor`]).
    fn on_child_step_iter(&mut self, actor: u8, lod_step: u8) {
        let _ = (actor, lod_step);
    }

    /// Per-bone keyframe interpolator - host-trait abstraction of
    /// `FUN_8004998C`. Unpacks the two `[i16; 3]` vectors per bone
    /// from the nested per-frame data, lerps by
    /// `sub_frame_factor / 16`, writes the result into the GPU OT
    /// scratch buffer.
    fn on_per_bone_interp(&mut self, actor: u8, record: &[u8], sub_frame_factor: u8) {
        let _ = (actor, record, sub_frame_factor);
    }

    /// Special-effect spawn - host-trait abstraction of
    /// `FUN_8004E13C`. Invoked when the consumer struct's `+0x87`
    /// `effect_id` is non-zero. The retail body walks an actor table
    /// at `DAT_801C9370` (7 slots) and toggles per-actor flag bytes
    /// based on inter-actor state; some branches use the RNG at
    /// `FUN_80056798` to seed actor `+0x6DA` and `+0x26D`.
    fn on_special_effect_spawn(&mut self, actor: u8, effect_id: u8) {
        let _ = (actor, effect_id);
    }
}

/// Default host that does nothing on opaque records (slots stay alive
/// indefinitely until `stop` is called). Useful for tests and engines
/// that don't yet wire the overlay dispatcher.
#[derive(Debug, Default)]
pub struct NullHost;

impl Host for NullHost {}

/// Per-actor animation runtime - fixed-size pool of `AnimSlot`s.
#[derive(Debug)]
pub struct AnimRuntime {
    pub(crate) slots: Vec<AnimSlot>,
    /// Latest pose per actor (mirrors `World::Actor::pose_frame`).
    /// Engines that don't want the event stream can read this directly.
    poses: Vec<Option<PoseFrame>>,
    /// Pending events surfaced from the most recent `tick`. Engines
    /// drain via `take_events`.
    events: Vec<AnimEvent>,
    /// Frame counter - useful for trace logs / fingerprinting.
    pub frame: u64,
}

impl Default for AnimRuntime {
    fn default() -> Self {
        Self::with_slots(MAX_ACTOR_SLOTS)
    }
}

impl AnimRuntime {
    /// Build a runtime with `n` actor slots. The retail per-scene actor
    /// table tops out at `MAX_ACTOR_SLOTS`; engines can pick a smaller
    /// pool to bound memory.
    pub fn with_slots(n: usize) -> Self {
        Self {
            slots: vec![AnimSlot::Idle; n],
            poses: vec![None; n],
            events: Vec::new(),
            frame: 0,
        }
    }

    /// Register `record` on actor `id`. Errors if `id` is out of range.
    /// If the slot was busy, emits `AnimEvent::Replaced` and overwrites
    /// it (matching the retail convention - `FUN_80024CFC` always
    /// overwrites).
    pub fn play(
        &mut self,
        id: u8,
        record: Vec<u8>,
        bone_count: usize,
    ) -> Result<RecordKind, AnimError> {
        let i = id as usize;
        if i >= self.slots.len() {
            return Err(AnimError::ActorOutOfRange { actor: id });
        }
        let kind = RecordKind::from_record(&record).ok_or(AnimError::HeaderTooSmall)?;
        let was_busy = !self.slots[i].is_idle();
        let new_slot = match kind {
            RecordKind::Keyframe => {
                let player = AnimPlayer::new(record, bone_count.max(1))
                    .map_err(|e| AnimError::PlayerInit(e.to_string()))?;
                AnimSlot::Keyframe { player, kind }
            }
            _ => AnimSlot::Opaque {
                record,
                bone_count,
                kind,
                frame_counter: 100, // mirrors actor[+0x68] init
            },
        };
        self.slots[i] = new_slot;
        if was_busy {
            self.events.push(AnimEvent::Replaced { actor: id });
        }
        Ok(kind)
    }

    /// Stop animation on actor `id`. Out-of-range actor is a no-op.
    pub fn stop(&mut self, id: u8) {
        if let Some(s) = self.slots.get_mut(id as usize) {
            *s = AnimSlot::Idle;
        }
        if let Some(p) = self.poses.get_mut(id as usize) {
            *p = None;
        }
    }

    /// Snapshot the current pose for actor `id`, if any.
    pub fn pose(&self, id: u8) -> Option<&PoseFrame> {
        self.poses.get(id as usize).and_then(|p| p.as_ref())
    }

    /// Read the current slot for actor `id`.
    pub fn slot(&self, id: u8) -> Option<&AnimSlot> {
        self.slots.get(id as usize)
    }

    /// `true` if any slot is non-idle.
    pub fn any_active(&self) -> bool {
        self.slots.iter().any(|s| !s.is_idle())
    }

    /// Number of actor slots in this runtime.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Drain any pending events from the most recent `tick`.
    pub fn take_events(&mut self) -> Vec<AnimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Advance one frame. Walks every slot and dispatches per-kind:
    ///
    /// - `Keyframe` slots: tick the embedded `AnimPlayer`, write the
    ///   resulting pose into `poses[i]`, emit `PoseUpdated`. If the
    ///   player reports `finished`, also emit `Finished` and clear
    ///   the slot.
    /// - `Opaque` slots: bump the frame counter, ask the host whether
    ///   to keep going via `on_opaque_record`, emit `OpaqueTick`.
    ///   If the host returns `false`, emit `Finished` and clear.
    /// - `Idle` slots: skipped.
    pub fn tick<H: Host>(&mut self, host: &mut H) {
        self.frame = self.frame.saturating_add(1);
        for i in 0..self.slots.len() {
            let actor = i as u8;
            // Take ownership briefly so we can mutate the slot while
            // also borrowing the host.
            let mut slot = std::mem::replace(&mut self.slots[i], AnimSlot::Idle);
            match &mut slot {
                AnimSlot::Idle => {
                    // Already idle - restore (still idle).
                    self.slots[i] = AnimSlot::Idle;
                    continue;
                }
                AnimSlot::Keyframe { player, kind } => {
                    let pose = player.tick();
                    let finished = pose.finished;
                    self.poses[i] = Some(pose.clone());
                    self.events.push(AnimEvent::PoseUpdated {
                        actor,
                        pose: pose.clone(),
                    });
                    if finished {
                        self.events.push(AnimEvent::Finished { actor, kind: *kind });
                        self.slots[i] = AnimSlot::Idle;
                    } else {
                        self.slots[i] = slot;
                    }
                }
                AnimSlot::Opaque {
                    record,
                    bone_count: _,
                    kind,
                    frame_counter,
                } => {
                    *frame_counter = frame_counter.saturating_add(1);
                    // Per-kind dispatch on the consumer-struct kind
                    // byte at `+0x00` of the runtime record. Matches
                    // the outer dispatch in FUN_8004AD80.
                    let consumer_kind =
                        OpaqueRecordKind::from_byte(record.first().copied().unwrap_or(0));
                    let alive = match consumer_kind {
                        OpaqueRecordKind::Kind2 => {
                            host.on_kind2_handshake(actor, record, *frame_counter)
                        }
                        OpaqueRecordKind::Kind4 => {
                            host.on_kind4_action_engaged(actor, record, *frame_counter)
                        }
                        OpaqueRecordKind::Kind5 => {
                            host.on_kind5_or_bit2(actor, record, *frame_counter)
                        }
                        OpaqueRecordKind::Kind7 => {
                            host.on_kind7_set_da_8(actor, record, *frame_counter)
                        }
                        OpaqueRecordKind::Kind8 => {
                            host.on_kind8_or_bit3(actor, record, *frame_counter)
                        }
                        OpaqueRecordKind::Other(b) => {
                            host.on_other_kind(actor, b, record, *frame_counter)
                        }
                    };
                    self.events.push(AnimEvent::OpaqueTick {
                        actor,
                        kind: *kind,
                        frame: *frame_counter,
                    });
                    if !alive {
                        self.events.push(AnimEvent::Finished { actor, kind: *kind });
                        self.slots[i] = AnimSlot::Idle;
                    } else {
                        self.slots[i] = slot;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Staged battle-anim commit (the FUN_8004AD80 id -> slot/record ladder)
// ---------------------------------------------------------------------------

/// First staged anim id that resolves through the per-character
/// **art-animation bank** instead of the runtime action table
/// (`docs/formats/battle-data-pack.md` § Art-animation bank).
pub const ART_ANIM_ID_BASE: u8 = 0x10;

/// The two **dynamic** action-table slots the anim commit materializes an
/// art-bank record into (`0x801C9360 + slot*4`, slots `0x10` / `0x11`).
pub const DYNAMIC_ART_SLOT_A: u8 = 0x10;
pub const DYNAMIC_ART_SLOT_B: u8 = 0x11;

/// Where a staged battle anim id (`actor[+0x1DA]`, written by the battle
/// action SM) resolves when the anim system commits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedAnimTarget {
    /// `q < 0x10`: play action-table entry `q` directly. For a party
    /// member the table is the player file's record[0] slots widened with
    /// the equipment-spliced swings (`0` idle, `1` walk/approach, `2..5`
    /// hit reactions, `7..9` ready/recover/defeat, `0xC..0xF` the four
    /// direction-command weapon swings); for a monster, its archive entry
    /// index.
    Direct { slot: u8 },
    /// `q >= 0x10`: materialize **art-bank record** `q - 0x10` into
    /// dynamic action-table slot `slot` (`0x10` or `0x11`) and rewrite the
    /// staged id to `slot`. Only meaningful for actors that carry an art
    /// bank (party members); a monster's ids stay plain entry indices.
    ArtBank { record: u8, slot: u8 },
}

/// Resolve a staged anim id through the retail commit ladder: ids below
/// [`ART_ANIM_ID_BASE`] play their action-table entry directly; ids
/// `q >= 0x10` select art-bank record `q - 0x10`, installing at dynamic
/// slot `0x11` for ids `0x10` and `0x1A` and at slot `0x10` for every
/// other id (the staged id is then rewritten to the slot number, so the
/// SM's `+0x1D9 == +0x1DA` equality checks compare slot numbers).
// PORT: FUN_8004AD80 (dynamic-art commit arm) - staged id q >= 0x10 reads
// bank record q - 0x10 (`q*0xD0 + bank + 4 - 0xCDC` install arithmetic),
// installs at action-table slot 0x11 when q == 0x10 || q == 0x1A else
// 0x10, and rewrites the staged byte to the slot number. See
// docs/formats/battle-data-pack.md § Art-animation bank.
pub fn resolve_staged_anim(q: u8) -> StagedAnimTarget {
    if q < ART_ANIM_ID_BASE {
        return StagedAnimTarget::Direct { slot: q };
    }
    let slot = if q == 0x10 || q == 0x1A {
        DYNAMIC_ART_SLOT_B
    } else {
        DYNAMIC_ART_SLOT_A
    };
    StagedAnimTarget::ArtBank {
        record: q - ART_ANIM_ID_BASE,
        slot,
    }
}

/// Errors the runtime can return from `play`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimError {
    ActorOutOfRange { actor: u8 },
    HeaderTooSmall,
    PlayerInit(String),
}

impl std::fmt::Display for AnimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnimError::ActorOutOfRange { actor } => {
                write!(f, "actor {actor} is out of range for this AnimRuntime")
            }
            AnimError::HeaderTooSmall => write!(f, "record buffer too small for an 8-byte header"),
            AnimError::PlayerInit(msg) => write!(f, "AnimPlayer init failed: {msg}"),
        }
    }
}

impl std::error::Error for AnimError {}
