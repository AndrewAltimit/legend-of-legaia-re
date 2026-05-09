//! Per-actor animation runtime — scaffold for the overlay-resident
//! `actor[+0x4C]` per-record bytecode dispatcher.
//!
//! ## Background
//!
//! `FUN_80024CFC` in `SCUS_942.54` is the only static-binary entry point
//! that touches an animation record. It stows the per-record byte pointer
//! in `actor[+0x4C]`, sets `actor[+0x56] = 0xB` and `actor[+0x68] = 100`,
//! and returns. The thing that actually consumes those fields and walks
//! the record is a **per-frame actor tick** that lives in the field
//! overlay loaded into `0x801C0000+` at runtime — it has not been captured
//! yet (see `docs/subsystems/actor-vm.md`).
//!
//! For the bulk of retail ANM data — records the runtime calls "opcode 6"
//! — the per-record body is a per-bone keyframe table, and the
//! interpolation math is statically reachable in `FUN_80021DF4`. That
//! algorithm is already ported in [`legaia_anm::AnimPlayer`].
//!
//! For everything else — records with header `a` field other than `0x06`
//! or `0x0A` — the per-record interpreter is opaque. The scaffold below
//! lets engines wire the runtime they need *now* without waiting for the
//! overlay capture: the dispatcher's `Host` trait exposes a single hook
//! the eventual port will fill in (`on_opaque_record`), and the
//! keyframe-driven case is fully handled by delegating to `AnimPlayer`.
//!
//! ## What this scaffold provides
//!
//! - A typed `RecordKind` derived from the header `a` field, populated
//!   from real-data observations in `crates/anm`.
//! - `AnimSlot`: per-actor playback state (record bytes, bone count,
//!   factor, finished flag).
//! - `AnimRuntime`: a fixed-size pool of `AnimSlot`s indexed by actor id,
//!   with `play(actor, record, bone_count, kind)` / `tick(actor)` /
//!   `stop(actor)` / `reset(actor)` operations.
//! - `Host`: callbacks the runtime makes when it sees a record kind it
//!   can't handle on its own (the eventual overlay-resident dispatcher).
//! - `AnimEvent`: stream surface so engines can react without polling
//!   per-actor state.
//!
//! When the overlay capture lands, the only change required is to fill
//! the `Host::on_opaque_record` body with the real per-kind dispatch —
//! every other piece of plumbing (per-actor pool, frame stepping,
//! lifecycle hooks, event stream) stays as-is.

use legaia_anm::{AnimPlayer, PoseFrame, RecordHeader};

/// Maximum actor pool size — matches the retail per-scene actor count
/// observed in the field overlay (`actor[+0x*]` table at
/// `0x801E473C`, 16-byte stride, ≤ 32 entries used).
pub const MAX_ACTOR_SLOTS: usize = 32;

/// Coarse classification of the per-record body, derived from the
/// header `a` field (first u16 of the 8-byte common header).
///
/// Values are observed across the title-screen + town-overlay corpus.
/// The `Keyframe` variant is the only one whose body layout is fully
/// understood (see `KeyframeReader` in `legaia_anm`); all other variants
/// are opaque until the overlay dispatcher is captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// `header.a == 0x06` — per-bone keyframe table. Handled by
    /// `AnimPlayer`. Dominant in retail field/town animations.
    Keyframe,
    /// `header.a == 0x0A` — observed on a small subset of records. The
    /// body shape is unknown; treated as opaque for now.
    KindA,
    /// `header.a == 0x02` — observed on title-screen records.
    Kind2,
    /// `header.a == 0x03` — observed on title-screen records.
    Kind3,
    /// Any other `header.a` value. Carries the raw byte so the host can
    /// route by it.
    Other(u16),
}

impl RecordKind {
    /// Classify a record by reading its 8-byte header.
    ///
    /// Returns `None` if the buffer is too small for a header.
    pub fn from_record(record: &[u8]) -> Option<Self> {
        let h = RecordHeader::from_bytes(record).ok()?;
        Some(match h.a {
            0x06 => RecordKind::Keyframe,
            0x0A => RecordKind::KindA,
            0x02 => RecordKind::Kind2,
            0x03 => RecordKind::Kind3,
            other => RecordKind::Other(other),
        })
    }

    /// `true` for the only record kind the engine can drive without
    /// the overlay-resident dispatcher (i.e. the keyframe path).
    pub fn handled_natively(self) -> bool {
        matches!(self, RecordKind::Keyframe)
    }
}

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
    /// A keyframe-style animation is playing — handled natively.
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
    /// the tick happened — the host is responsible for any side
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
/// The eventual overlay-resident dispatcher will replace
/// `on_opaque_record` with the real per-kind interpretation. Until
/// then, engines override this trait to wire whatever fallback they
/// need (e.g. swap to a static pose, defer to a static animation
/// asset, log for analysis).
pub trait Host {
    /// Called once per `AnimRuntime::tick` for every Opaque-kind slot.
    ///
    /// `frame_counter` mirrors `actor[+0x68]` and increments each tick
    /// while the slot is alive. The default body just records the
    /// event; engines that have a fallback override this.
    ///
    /// Returning `true` keeps the slot alive; `false` ends it
    /// (transitions to `Idle` on the next tick boundary).
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
}

/// Default host that does nothing on opaque records (slots stay alive
/// indefinitely until `stop` is called). Useful for tests and engines
/// that don't yet wire the overlay dispatcher.
#[derive(Debug, Default)]
pub struct NullHost;

impl Host for NullHost {}

/// Per-actor animation runtime — fixed-size pool of `AnimSlot`s.
#[derive(Debug)]
pub struct AnimRuntime {
    slots: Vec<AnimSlot>,
    /// Latest pose per actor (mirrors `World::Actor::pose_frame`).
    /// Engines that don't want the event stream can read this directly.
    poses: Vec<Option<PoseFrame>>,
    /// Pending events surfaced from the most recent `tick`. Engines
    /// drain via `take_events`.
    events: Vec<AnimEvent>,
    /// Frame counter — useful for trace logs / fingerprinting.
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
    /// it (matching the retail convention — `FUN_80024CFC` always
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
                    // Already idle — restore (still idle).
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
                    let alive = host.on_opaque_record(actor, *kind, record, *frame_counter);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic keyframe-style record: 8-byte header (a=0x06),
    /// `bone_count` 8-byte output slots, `bone_count` 24-byte keyframe
    /// entries.
    fn synth_keyframe_record(bone_count: usize) -> Vec<u8> {
        let mut buf = vec![0u8; legaia_anm::RECORD_HEADER_SIZE + 8 * bone_count + 24 * bone_count];
        // header.a = 0x06 (Keyframe)
        buf[0..2].copy_from_slice(&0x0006u16.to_le_bytes());
        // header.b = 0x14 (frame count)
        buf[2..4].copy_from_slice(&0x0014u16.to_le_bytes());
        // marker_1 = 0x080C (canonical)
        buf[4..6].copy_from_slice(&0x080Cu16.to_le_bytes());
        // flag = 0x0002
        buf[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
        // Set first bone keyframe so interpolation has something non-trivial
        let kf_off = legaia_anm::RECORD_HEADER_SIZE + 8 * bone_count;
        buf[kf_off..kf_off + 2].copy_from_slice(&10i16.to_le_bytes());
        buf[kf_off + 6..kf_off + 8].copy_from_slice(&100i16.to_le_bytes());
        buf
    }

    /// Build a synthetic opaque record with header `a` set to `kind_byte`.
    fn synth_opaque_record(kind_byte: u16, body_len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; legaia_anm::RECORD_HEADER_SIZE + body_len];
        buf[0..2].copy_from_slice(&kind_byte.to_le_bytes());
        buf[4..6].copy_from_slice(&0x080Cu16.to_le_bytes());
        buf[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
        buf
    }

    #[test]
    fn record_kind_from_header_a() {
        let r6 = synth_keyframe_record(2);
        assert_eq!(RecordKind::from_record(&r6), Some(RecordKind::Keyframe));
        let r2 = synth_opaque_record(0x02, 16);
        assert_eq!(RecordKind::from_record(&r2), Some(RecordKind::Kind2));
        let r3 = synth_opaque_record(0x03, 16);
        assert_eq!(RecordKind::from_record(&r3), Some(RecordKind::Kind3));
        let ra = synth_opaque_record(0x0A, 16);
        assert_eq!(RecordKind::from_record(&ra), Some(RecordKind::KindA));
        let other = synth_opaque_record(0x42, 16);
        assert_eq!(
            RecordKind::from_record(&other),
            Some(RecordKind::Other(0x42))
        );
        // Too-small buffer → None.
        assert!(RecordKind::from_record(&[0u8; 4]).is_none());
    }

    #[test]
    fn record_kind_handled_natively_only_for_keyframe() {
        assert!(RecordKind::Keyframe.handled_natively());
        assert!(!RecordKind::Kind2.handled_natively());
        assert!(!RecordKind::Kind3.handled_natively());
        assert!(!RecordKind::KindA.handled_natively());
        assert!(!RecordKind::Other(0x42).handled_natively());
    }

    #[test]
    fn play_keyframe_creates_keyframe_slot() {
        let mut rt = AnimRuntime::with_slots(4);
        let kind = rt.play(2, synth_keyframe_record(3), 3).unwrap();
        assert_eq!(kind, RecordKind::Keyframe);
        assert!(matches!(rt.slot(2), Some(AnimSlot::Keyframe { .. })));
        assert!(rt.slot(0).map(|s| s.is_idle()).unwrap_or(false));
    }

    #[test]
    fn play_opaque_creates_opaque_slot_with_init_counter_100() {
        let mut rt = AnimRuntime::with_slots(4);
        let kind = rt.play(1, synth_opaque_record(0x02, 32), 0).unwrap();
        assert_eq!(kind, RecordKind::Kind2);
        match rt.slot(1).unwrap() {
            AnimSlot::Opaque {
                kind,
                frame_counter,
                ..
            } => {
                assert_eq!(*kind, RecordKind::Kind2);
                assert_eq!(*frame_counter, 100);
            }
            other => panic!("expected Opaque slot, got {other:?}"),
        }
    }

    #[test]
    fn play_replacing_busy_slot_emits_replaced_event() {
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_keyframe_record(2), 2).unwrap();
        rt.play(0, synth_keyframe_record(2), 2).unwrap();
        let evs = rt.take_events();
        assert!(evs.contains(&AnimEvent::Replaced { actor: 0 }));
    }

    #[test]
    fn play_actor_out_of_range_errors() {
        let mut rt = AnimRuntime::with_slots(4);
        let err = rt
            .play(7, synth_keyframe_record(1), 1)
            .expect_err("should be an error");
        assert_eq!(err, AnimError::ActorOutOfRange { actor: 7 });
    }

    #[test]
    fn play_too_small_buffer_errors() {
        let mut rt = AnimRuntime::with_slots(4);
        let err = rt.play(0, vec![0u8; 4], 1).expect_err("should error");
        assert_eq!(err, AnimError::HeaderTooSmall);
    }

    #[test]
    fn tick_keyframe_emits_pose_updated_and_writes_pose() {
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_keyframe_record(1), 1).unwrap();
        let mut host = NullHost;
        rt.tick(&mut host);
        let evs = rt.take_events();
        assert!(matches!(evs[0], AnimEvent::PoseUpdated { actor: 0, .. }));
        assert!(rt.pose(0).is_some());
    }

    #[test]
    fn tick_opaque_calls_host_and_emits_opaque_tick() {
        struct CountingHost {
            calls: Vec<(u8, RecordKind, u32)>,
        }
        impl Host for CountingHost {
            fn on_opaque_record(
                &mut self,
                actor: u8,
                kind: RecordKind,
                _record: &[u8],
                frame_counter: u32,
            ) -> bool {
                self.calls.push((actor, kind, frame_counter));
                true
            }
        }
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(2, synth_opaque_record(0x02, 16), 0).unwrap();
        let mut host = CountingHost { calls: vec![] };
        rt.tick(&mut host);
        assert_eq!(host.calls.len(), 1);
        let (actor, kind, frame) = host.calls[0];
        assert_eq!(actor, 2);
        assert_eq!(kind, RecordKind::Kind2);
        // Initial counter 100 → after one tick it's 101.
        assert_eq!(frame, 101);
        let evs = rt.take_events();
        assert!(matches!(
            evs[0],
            AnimEvent::OpaqueTick {
                actor: 2,
                kind: RecordKind::Kind2,
                frame: 101
            }
        ));
    }

    #[test]
    fn tick_opaque_host_returning_false_clears_slot_and_emits_finished() {
        struct EarlyExitHost;
        impl Host for EarlyExitHost {
            fn on_opaque_record(
                &mut self,
                _actor: u8,
                _kind: RecordKind,
                _record: &[u8],
                _frame: u32,
            ) -> bool {
                false
            }
        }
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_opaque_record(0x03, 16), 0).unwrap();
        let mut host = EarlyExitHost;
        rt.tick(&mut host);
        let evs = rt.take_events();
        assert!(evs.iter().any(|e| matches!(
            e,
            AnimEvent::Finished {
                actor: 0,
                kind: RecordKind::Kind3
            }
        )));
        assert!(rt.slot(0).unwrap().is_idle());
    }

    #[test]
    fn tick_keyframe_finished_clears_slot_when_non_looping() {
        let mut rt = AnimRuntime::with_slots(4);
        // Build a keyframe record that finishes quickly.
        rt.play(1, synth_keyframe_record(1), 1).unwrap();
        // Force the embedded player into non-looping mode + max delta.
        if let Some(AnimSlot::Keyframe { player, .. }) = rt.slots.get_mut(1) {
            player.looping = false;
            player.frame_delta = 0xFF;
        } else {
            panic!("expected keyframe slot");
        }
        let mut host = NullHost;
        // 0xFF + 0xFF wraps past 0xFF on the second tick → finished=true.
        rt.tick(&mut host);
        rt.tick(&mut host);
        let evs = rt.take_events();
        assert!(evs.iter().any(|e| matches!(
            e,
            AnimEvent::Finished {
                actor: 1,
                kind: RecordKind::Keyframe
            }
        )));
        assert!(rt.slot(1).unwrap().is_idle());
    }

    #[test]
    fn idle_slots_are_skipped_during_tick() {
        let mut rt = AnimRuntime::with_slots(4);
        // Don't register anything; tick should produce no events.
        let mut host = NullHost;
        rt.tick(&mut host);
        assert!(rt.take_events().is_empty());
        assert!(!rt.any_active());
    }

    #[test]
    fn stop_clears_slot_and_pose() {
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_keyframe_record(1), 1).unwrap();
        let mut host = NullHost;
        rt.tick(&mut host);
        assert!(rt.pose(0).is_some());
        rt.stop(0);
        assert!(rt.slot(0).unwrap().is_idle());
        assert!(rt.pose(0).is_none());
    }

    #[test]
    fn frame_counter_advances_each_tick() {
        let mut rt = AnimRuntime::with_slots(2);
        let mut host = NullHost;
        rt.tick(&mut host);
        rt.tick(&mut host);
        rt.tick(&mut host);
        assert_eq!(rt.frame, 3);
    }

    #[test]
    fn header_a_round_trips_through_slot() {
        let mut rt = AnimRuntime::with_slots(2);
        rt.play(0, synth_keyframe_record(1), 1).unwrap();
        rt.play(1, synth_opaque_record(0x0A, 16), 0).unwrap();
        assert_eq!(rt.slot(0).unwrap().header_a(), Some(0x06));
        assert_eq!(rt.slot(1).unwrap().header_a(), Some(0x0A));
    }
}
