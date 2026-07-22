//! Sound-effect catalog + scheduler.
//!
//! Maps battle / field cue IDs (the `kind` byte the art-record `HitCue`
//! / overlay scripts emit) to per-cue [`SfxEntry`] descriptors that
//! describe how to fire a one-shot through the SPU. Engines populate the
//! catalog at startup, then forward `ScheduledCue`-like requests through
//! [`SfxScheduler`] which queues each request with its retail timing
//! offset and dispatches when the per-frame tick reaches the firing
//! frame.
//!
//! ## Cue ID conventions
//!
//! - `0x1A` - generic SFX trigger (the canonical `HitCue` "play sound"
//!   kind). The catalog typically maps this to per-strike weapon impact
//!   tones.
//! - `0x4C` - hit-effect visual (no sound on its own; engines that fold
//!   the visual into a synced sound use this slot).
//! - `0x80..=0xFE` - reserved per-character or per-art SFX IDs (the
//!   retail engine indexes from the per-actor `+0x9C0` table).
//!
//! Beyond these documented bytes the catalog is open - the scheduler
//! itself is agnostic to ID ranges.
//!
//! ## Frame timing
//!
//! Strikes carry a `timing_frames` value (the `HitCue::timing_frames`
//! field). The scheduler wraps that as a per-cue countdown. `tick_frame`
//! advances every queued cue by one frame; cues whose countdown hits zero
//! are returned in the [`SfxFireBatch`] for the host to dispatch through
//! [`SfxBank::play_one_shot`] (or its own SPU bridge).
//!
//! Pure data - no SPU access here. The scheduler is a queue + clock; the
//! catalog is a lookup table; firing the actual note-on is the engine's
//! call.

use crate::spu::Spu;
use crate::vab_bind::VabBank;

/// Per-cue descriptor. Engines populate one entry per cue id, then look
/// the entry up to drive the actual SPU note-on parameters.
///
/// The fields mirror the retail static SFX descriptor
/// (`legaia_asset::sfx_table::SfxDescriptor`): `program_index` is the
/// descriptor `+0` program, `tone` is the descriptor `+1` **region index**
/// (NOT a key-range window), `key` is the `+2` note-level attribute, and
/// `voices` is the `+3 & 0x1F` voice count. The retail SFX path
/// (`FUN_80016B6C` → `FUN_80065034`) names a cue's tone by explicit region
/// index and fans a multi-voice cue across consecutive regions `tone + i`,
/// so [`SfxBank::play_one_shot`] resolves through [`VabBank::play_tone`],
/// not the sequencer's key-range [`VabBank::play_note`]. `voice_pref` pins
/// the first voice slot - `None` means "first available."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxEntry {
    /// Cue id this entry handles. The art-record `HitCue::kind` byte for
    /// strike-fired cues; engines extend the namespace for menu blips,
    /// footstep cues, etc.
    pub id: u8,
    /// Index into [`VabBank::programs`] (descriptor `+0` `p`).
    pub program_index: u8,
    /// Explicit tone / ADSR-region base index (descriptor `+1` `t`). Voice
    /// `i` of a multi-voice cue plays region `tone + i`. This is an index,
    /// not a key-range window - see [`VabBank::play_tone`].
    pub tone: u8,
    /// Note-level attribute (descriptor `+2` `l`, MIDI-ish 0..=127). Feeds
    /// the tone's pitch math; does NOT select the tone.
    pub key: u8,
    /// Voice count (descriptor `+3 & 0x1F`): how many consecutive regions
    /// `tone..tone+voices` the cue keys on. Clamped to at least 1 at fire
    /// time.
    pub voices: u8,
    /// MIDI-style velocity (0..=127). Engines map this to the SPU's
    /// voice volume.
    pub vel: u8,
    /// Optional preferred first voice slot (0..=23). `None` = round-robin;
    /// a multi-voice cue keys on `voice_pref + i` when pinned.
    pub voice_pref: Option<u8>,
}

impl SfxEntry {
    /// Construct a single-voice entry (tone 0) with the canonical "use first
    /// available voice" preference and unity velocity. Prefer
    /// [`SfxEntry::from_descriptor`] when the disc descriptor's tone /
    /// voice-count are known.
    pub fn new(id: u8, program_index: u8, key: u8) -> Self {
        Self {
            id,
            program_index,
            tone: 0,
            key,
            voices: 1,
            vel: 100,
            voice_pref: None,
        }
    }

    /// Construct from the full retail descriptor fields (program, tone,
    /// note, voice count) - the shape [`SfxBank::from_descriptors`] carries.
    pub fn from_descriptor(id: u8, program_index: u8, tone: u8, key: u8, voices: u8) -> Self {
        Self {
            id,
            program_index,
            tone,
            key,
            voices: voices.max(1),
            vel: 100,
            voice_pref: None,
        }
    }

    /// With explicit tone / region index.
    pub fn with_tone(self, tone: u8) -> Self {
        Self { tone, ..self }
    }

    /// With explicit voice count.
    pub fn with_voices(self, voices: u8) -> Self {
        Self {
            voices: voices.max(1),
            ..self
        }
    }

    /// With explicit velocity.
    pub fn with_vel(self, vel: u8) -> Self {
        Self { vel, ..self }
    }

    /// With pinned first voice.
    pub fn with_voice(self, voice: u8) -> Self {
        Self {
            voice_pref: Some(voice),
            ..self
        }
    }
}

/// Catalog of cue-id → [`SfxEntry`] mappings.
///
/// Stores entries in a `Vec` keyed by id. Lookup is O(N) but the table is
/// small (≤256 cues) and rebuilt at scene transitions.
#[derive(Debug, Default, Clone)]
pub struct SfxBank {
    entries: Vec<SfxEntry>,
}

impl SfxBank {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a minimal default bank used by the asset viewer's preview
    /// path: a single generic hit-cue tied to program 0, key 60.
    pub fn vanilla() -> Self {
        let mut s = Self::new();
        s.insert(SfxEntry::new(0x1A, 0, 60));
        s.insert(SfxEntry::new(0x4C, 0, 67));
        s
    }

    /// Build a bank from decoded SFX descriptors - the
    /// `(id, program, tone, note, voice_count)` tuples the disc
    /// `legaia_asset::sfx_table::SfxTable::active` iterator yields (program =
    /// `SfxDescriptor::program`, tone = `SfxDescriptor::tone`, note =
    /// `SfxDescriptor::note`, voice_count = `SfxDescriptor::voice_count()`).
    /// Keeping the argument a plain tuple iterator avoids a dependency on the
    /// asset crate from the audio layer; the host (engine-shell) wires the
    /// disc-decoded table in. Carrying the tone + voice count is required:
    /// the SFX path names its tone by explicit region index, and several
    /// retail cues (e.g. the strike cue `0x1A`) place `note` outside the
    /// tone's key window, so a key-range resolve would render silence. Later
    /// tuples overwrite earlier ones with the same id.
    pub fn from_descriptors<I: IntoIterator<Item = (u8, u8, u8, u8, u8)>>(descriptors: I) -> Self {
        let mut bank = Self::new();
        for (id, program_index, tone, note, voices) in descriptors {
            bank.insert(SfxEntry::from_descriptor(
                id,
                program_index,
                tone,
                note,
                voices,
            ));
        }
        bank
    }

    /// Insert (or overwrite) the entry for `entry.id`.
    pub fn insert(&mut self, entry: SfxEntry) {
        if let Some(slot) = self.entries.iter_mut().find(|e| e.id == entry.id) {
            *slot = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Look up an entry by cue id.
    pub fn get(&self, id: u8) -> Option<&SfxEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &SfxEntry> {
        self.entries.iter()
    }

    /// Fire one shot. Resolves the cue id and keys the descriptor's
    /// `voices` consecutive tone regions (`tone + i`) on `voices` idle SPU
    /// voices, delegating to [`VabBank::play_tone`] - the retail SFX shape
    /// (`FUN_80016B6C` → `FUN_80065034`): a cue names its tone by an explicit
    /// region **index**, not by a key-range window. This differs from the
    /// sequencer's [`VabBank::play_note`]; several retail cues place `note`
    /// outside the tone's authored `min..=max` window (the generic strike cue
    /// `0x1A` = program 3 / tone 8 / note 67 is one), so the old key-range
    /// resolve rendered silence for them.
    ///
    /// Returns the FIRST voice keyed on, or `None` if the cue isn't in the
    /// bank or nothing sounded (no free voice, or the bank's program / tone /
    /// sample is missing - e.g. the cue's program isn't resident in this
    /// bank). Mirrors `crates/web-viewer/src/sfx_view.rs::render_cue`.
    pub fn play_one_shot(&self, id: u8, spu: &mut Spu, vab: &VabBank) -> Option<u8> {
        let entry = self.get(id)?;
        let voices = entry.voices.max(1);
        let mut first_voice: Option<u8> = None;
        for i in 0..voices {
            let voice_idx = match entry.voice_pref {
                Some(v) => (v as usize + i as usize).min(23),
                None => match first_idle_voice(spu) {
                    Some(v) => v as usize,
                    // No idle voice left - keep whatever already sounded.
                    None => break,
                },
            };
            if vab.play_tone(
                spu,
                voice_idx,
                entry.program_index as usize,
                entry.tone as usize + i as usize,
                entry.key,
                entry.vel,
            ) {
                first_voice.get_or_insert(voice_idx as u8);
            }
        }
        first_voice
    }
}

fn first_idle_voice(spu: &Spu) -> Option<u8> {
    for (idx, v) in spu.voices.iter().enumerate() {
        if v.is_off() {
            return Some(idx as u8);
        }
    }
    None
}

/// How the cue dispatcher `FUN_8004fcc8` routes a raw cue id.
///
/// `FUN_8004fcc8` is the front-end in front of the [`SfxScheduler`]'s cue ring
/// (`FUN_80035B50`): it classifies the raw id, de-dups UI/SFX cues against the
/// currently-selected cue, and routes voice ids (`>= 0x100`) to the streamed
/// voice trigger `FUN_8003D53C` instead. This enum is the pure classification;
/// the actual ring write, the runtime voice gates (`gp[0xA0C]+0x276 == 0`,
/// `FUN_8003DE7C(1) == 0`), and the SPU note-on stay with the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueDispatch {
    /// A UI / SFX one-shot to enqueue into the cue ring. `ring_value` is the
    /// value stored (`id - 1` for `id < 0x40`, else `id`); `dedup_key` is what
    /// the dispatcher compares against the currently-selected cue
    /// (`DAT_8007B724`) to suppress an immediate repeat - see
    /// [`CueDispatch::ring_suppressed_by`].
    Ring { ring_value: u16, dedup_key: i32 },
    /// A streamed voice trigger (`FUN_8003D53C`). `channel` is the voice channel
    /// after the `1 → 0x1A`, `3 → 0x1B`, `5 → 0x1C` remap; `submode = id & 7`;
    /// `pitch_index` indexes the pitch table (`DAT_800788B8`) - feed its `u16`
    /// entry through [`voice_pitch`] to get the playback pitch.
    Voice {
        channel: u8,
        submode: u8,
        pitch_index: u16,
    },
}

impl CueDispatch {
    /// For a [`CueDispatch::Ring`] cue, whether the dispatcher suppresses it
    /// because `current_selection` (`DAT_8007B724`) already equals its
    /// `dedup_key` (so a held/repeated selection cue doesn't re-fire). Always
    /// `false` for [`CueDispatch::Voice`].
    pub fn ring_suppressed_by(&self, current_selection: i32) -> bool {
        matches!(self, CueDispatch::Ring { dedup_key, .. } if *dedup_key == current_selection)
    }
}

/// Classify a raw cue id the way `FUN_8004fcc8` does (the pure decode; no ring
/// write, no voice gate, no SPU access).
///
/// - `id < 0x40` → [`CueDispatch::Ring`] storing `id - 1`, de-dup key `id - 1`.
/// - `0x40 <= id < 0x100` → [`CueDispatch::Ring`] storing `id`, de-dup key
///   `id + 0x19C` (the retail `param_1 + 0x19c` compare).
/// - `id >= 0x100` → [`CueDispatch::Voice`]: `channel = remap((id - 0x100) >> 3)`
///   (`1 → 0x1A`, `3 → 0x1B`, `5 → 0x1C`), `submode = (id - 0x100) & 7`,
///   `pitch_index = id - 0x100`.
///
/// PORT: FUN_8004FCC8 (cue dispatch decode; the ring write / voice gate / SPU
/// note-on stay with the caller - the ring is [`SfxScheduler`] / `FUN_80035B50`).
pub fn classify_cue(id: u32) -> CueDispatch {
    if id < 0x100 {
        if id < 0x40 {
            let v = id.wrapping_sub(1);
            CueDispatch::Ring {
                ring_value: v as u16,
                dedup_key: v as i32,
            }
        } else {
            CueDispatch::Ring {
                ring_value: id as u16,
                dedup_key: (id + 0x19C) as i32,
            }
        }
    } else {
        let v = id - 0x100;
        let channel = match v >> 3 {
            1 => 0x1A,
            3 => 0x1B,
            5 => 0x1C,
            other => other as u8,
        };
        CueDispatch::Voice {
            channel,
            submode: (v & 7) as u8,
            pitch_index: v as u16,
        }
    }
}

/// The voice playback pitch `FUN_8004fcc8` computes for a [`CueDispatch::Voice`]:
/// `(pitch_table_value * 0x3C + 99) / 100` (the integer round-up of
/// `pitch_table_value * 0.6`). `pitch_table_value` is the `u16` at
/// `DAT_800788B8[pitch_index]`.
pub fn voice_pitch(pitch_table_value: u16) -> u16 {
    // Retail: `(value * 0x3C + 99) / 100`; the `+ 99` over `/ 100` is the
    // round-up of `value * 0.6`, i.e. `(value * 0x3C).div_ceil(100)`.
    (pitch_table_value as u32 * 0x3C).div_ceil(100) as u16
}

/// One queued cue waiting for its firing frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingCue {
    /// Cue id (resolved through [`SfxBank`] at fire time).
    pub id: u16,
    /// Frames remaining until the cue fires. Zero = fire on next tick.
    pub frames_remaining: u16,
    /// Optional source actor slot. Engines surface this through HUD logs.
    pub actor_slot: Option<u8>,
    /// Optional target slot.
    pub target_slot: Option<u8>,
}

impl PendingCue {
    pub fn new(id: u16, frames: u16) -> Self {
        Self {
            id,
            frames_remaining: frames,
            actor_slot: None,
            target_slot: None,
        }
    }

    pub fn with_actors(self, actor: u8, target: u8) -> Self {
        Self {
            actor_slot: Some(actor),
            target_slot: Some(target),
            ..self
        }
    }
}

/// Result of one [`SfxScheduler::tick_frame`]. Engines drain `fired` to
/// emit the actual SPU note-ons (or HUD popups for hit-effect cues).
#[derive(Debug, Default, Clone)]
pub struct SfxFireBatch {
    pub fired: Vec<PendingCue>,
}

impl SfxFireBatch {
    pub fn is_empty(&self) -> bool {
        self.fired.is_empty()
    }
}

/// Frame-driven scheduler.
///
/// `enqueue` adds a [`PendingCue`] with a delay; `tick_frame` advances
/// the clock by one frame and returns any cues whose countdown reached
/// zero. Cues with `frames_remaining = 0` fire on the *next* tick (so a
/// cue queued mid-frame doesn't fire immediately and gives the host a
/// chance to clear render state first).
#[derive(Debug, Default, Clone)]
pub struct SfxScheduler {
    queue: Vec<PendingCue>,
    ring: crate::sfx_ring::SfxCueRing,
    frame_step: u8,
}

impl SfxScheduler {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            ring: crate::sfx_ring::SfxCueRing::new(),
            frame_step: 1,
        }
    }

    /// Install the adaptive frame step (`DAT_1F800393`) the **retail ring**
    /// ages by. Defaults to `1`; field play runs at `2`
    /// (`legaia_engine_vm::actor_tick::FrameCadence::FIELD`). Only the ring
    /// half reads it - the approximate [`Self::enqueue`] queue is still
    /// per-tick, which is the difference the ring exists to remove.
    pub fn set_frame_step(&mut self, step: u8) {
        self.frame_step = step.max(1);
    }

    /// Write a cue into a slot of the byte-faithful retail ring
    /// ([`crate::sfx_ring::SfxCueRing`]), with its delay in **vsyncs**.
    ///
    /// This is the retail producer contract: four slots, the producer picks
    /// the slot, a fifth cue replaces one. [`Self::tick_frame`] ages the ring
    /// and folds any due cue into the same [`SfxFireBatch`].
    pub fn arm_ring_cue(&mut self, slot: usize, id: i16, delay_vsyncs: i32) {
        self.ring.arm(slot, id, delay_vsyncs);
    }

    /// Borrow the retail ring (inspection / tests).
    pub fn ring(&self) -> &crate::sfx_ring::SfxCueRing {
        &self.ring
    }

    /// Queue a cue. The cue fires when `frames_remaining` reaches zero
    /// during a [`Self::tick_frame`] call.
    ///
    // PORT: FUN_80035B50 - retail's SFX-cue enqueue writes the cue id into the
    // next slot of a fixed 4-entry u16 ring at &DAT_8007B6D8 and advances the
    // head; this models the same "queue a one-shot SFX" contract with an
    // unbounded queue + per-cue countdown instead of the 4-slot ring.
    // REF: FUN_80035BD0 - the retail "overwrite current slot" variant (replace
    // an in-flight cue without advancing the head, e.g. the deny buzz 0x23
    // replacing a queued accept when a menu-open is refused) is not separately
    // modeled; cues simply queue here.
    pub fn enqueue(&mut self, cue: PendingCue) {
        self.queue.push(cue);
    }

    /// Bulk-queue several cues from a slice of `(id, frames)` pairs.
    pub fn enqueue_all<I: IntoIterator<Item = PendingCue>>(&mut self, cues: I) {
        self.queue.extend(cues);
    }

    /// Number of cues still waiting to fire.
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Advance the clock by one frame. Returns the cues whose
    /// `frames_remaining` reached zero this tick (in queue order), followed
    /// by any retail-ring slot that came due this frame.
    ///
    /// The ring half runs the exact retail order - age
    /// (`FUN_8001698C`), then drain (`FUN_80016B6C`) - so a ring cue fires on
    /// the frame its countdown first reads zero and is cleared before the next
    /// drain can see it. See [`crate::sfx_ring`].
    pub fn tick_frame(&mut self) -> SfxFireBatch {
        let mut batch = SfxFireBatch::default();
        // Decrement, partition into fired vs. still-pending.
        let mut still = Vec::with_capacity(self.queue.len());
        for mut cue in self.queue.drain(..) {
            if cue.frames_remaining == 0 {
                batch.fired.push(cue);
            } else {
                cue.frames_remaining -= 1;
                if cue.frames_remaining == 0 {
                    // Just hit zero - fire on the *next* tick (matches
                    // retail timing where a `timing_frames = 1` cue plays
                    // one frame after the strike begins).
                    still.push(cue);
                } else {
                    still.push(cue);
                }
            }
        }
        self.queue = still;

        // The retail ring pair, phase-rotated for a host that arms *before*
        // the tick instead of in the middle of one.
        //
        // Retail's frame is age (FUN_8001698C) -> game logic, which is where
        // FUN_80035B50 arms a slot with timer 0 -> drain (FUN_80016B6C). So a
        // cue armed on frame N is drained on frame N, and cleared by frame
        // N+1's aging pass. Here the host arms and *then* calls this, so
        // draining first and ageing after reproduces exactly that schedule:
        // delay 0 fires on this tick, delay d fires d ticks later, and the
        // slot is cleared before it can fire twice. Ageing first would
        // clear a just-armed delay-0 cue before it ever played.
        for (_slot, id) in self.ring.drain() {
            if id >= 0 {
                batch.fired.push(PendingCue::new(id as u16, 0));
            }
        }
        self.ring.age(self.frame_step);
        batch
    }

    /// Drop every queued cue. Engines call this on scene transitions /
    /// battle abort.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.ring.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_insert_and_get_round_trip() {
        let mut bank = SfxBank::new();
        bank.insert(SfxEntry::new(0x1A, 3, 72));
        let e = bank.get(0x1A).unwrap();
        assert_eq!(e.program_index, 3);
        assert_eq!(e.key, 72);
        assert_eq!(e.vel, 100);
        assert!(e.voice_pref.is_none());
    }

    #[test]
    fn bank_insert_overwrites_existing() {
        let mut bank = SfxBank::new();
        bank.insert(SfxEntry::new(0x1A, 0, 60));
        bank.insert(SfxEntry::new(0x1A, 5, 70).with_vel(80));
        let e = bank.get(0x1A).unwrap();
        assert_eq!(e.program_index, 5);
        assert_eq!(e.vel, 80);
        assert_eq!(bank.len(), 1);
    }

    #[test]
    fn bank_vanilla_has_two_default_entries() {
        let bank = SfxBank::vanilla();
        assert!(bank.get(0x1A).is_some());
        assert!(bank.get(0x4C).is_some());
        assert!(bank.get(0xFF).is_none());
    }

    #[test]
    fn bank_from_descriptors_maps_program_tone_and_key() {
        // Mirrors the retail descriptors for ids 0x1A (p3 tone8 note67 v1) and
        // 0x4C (p3 tone8 note64 v2) decoded from DAT_8006F198.
        let bank = SfxBank::from_descriptors([(0x1A, 3, 8, 67, 1), (0x4C, 3, 8, 64, 2)]);
        assert_eq!(bank.len(), 2);
        let a = bank.get(0x1A).unwrap();
        assert_eq!((a.program_index, a.tone, a.key, a.voices), (3, 8, 67, 1));
        let b = bank.get(0x4C).unwrap();
        assert_eq!((b.program_index, b.tone, b.key, b.voices), (3, 8, 64, 2));
        // A zero voice-count clamps to 1 (still fires one voice).
        let clamped = SfxBank::from_descriptors([(0x10, 0, 0, 60, 0)]);
        assert_eq!(clamped.get(0x10).unwrap().voices, 1);
        // Later tuple overwrites an earlier same-id one.
        let over = SfxBank::from_descriptors([(0x1A, 1, 0, 10, 1), (0x1A, 9, 2, 90, 1)]);
        assert_eq!(over.len(), 1);
        assert_eq!(over.get(0x1A).unwrap().program_index, 9);
        assert_eq!(over.get(0x1A).unwrap().tone, 2);
    }

    #[test]
    fn bank_with_vel_and_with_voice_chain() {
        let e = SfxEntry::new(0x1A, 0, 60).with_vel(127).with_voice(7);
        assert_eq!(e.vel, 127);
        assert_eq!(e.voice_pref, Some(7));
    }

    #[test]
    fn scheduler_empty_tick_returns_no_fires() {
        let mut s = SfxScheduler::new();
        let batch = s.tick_frame();
        assert!(batch.is_empty());
        assert_eq!(s.pending_count(), 0);
    }

    #[test]
    fn scheduler_immediate_cue_fires_on_first_tick() {
        let mut s = SfxScheduler::new();
        s.enqueue(PendingCue::new(0x1A, 0));
        let batch = s.tick_frame();
        assert_eq!(batch.fired.len(), 1);
        assert_eq!(batch.fired[0].id, 0x1A);
    }

    #[test]
    fn scheduler_delayed_cue_waits_n_frames() {
        let mut s = SfxScheduler::new();
        s.enqueue(PendingCue::new(0x1A, 3));
        // Tick 1: 3 -> 2, no fire.
        // Tick 2: 2 -> 1, no fire.
        // Tick 3: 1 -> 0, no fire.
        // Tick 4: 0 fires.
        for _ in 0..3 {
            assert!(s.tick_frame().fired.is_empty());
        }
        let batch = s.tick_frame();
        assert_eq!(batch.fired.len(), 1);
    }

    #[test]
    fn scheduler_preserves_queue_order_on_simultaneous_fire() {
        let mut s = SfxScheduler::new();
        s.enqueue(PendingCue::new(0x10, 0));
        s.enqueue(PendingCue::new(0x20, 0));
        s.enqueue(PendingCue::new(0x30, 0));
        let batch = s.tick_frame();
        let ids: Vec<u16> = batch.fired.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![0x10, 0x20, 0x30]);
    }

    #[test]
    fn scheduler_clear_drops_pending() {
        let mut s = SfxScheduler::new();
        s.enqueue(PendingCue::new(0x1A, 5));
        s.enqueue(PendingCue::new(0x4C, 10));
        assert_eq!(s.pending_count(), 2);
        s.clear();
        assert_eq!(s.pending_count(), 0);
        assert!(s.tick_frame().is_empty());
    }

    #[test]
    fn scheduler_actor_slots_propagate_through_fire() {
        let mut s = SfxScheduler::new();
        s.enqueue(PendingCue::new(0x1A, 0).with_actors(2, 5));
        let batch = s.tick_frame();
        assert_eq!(batch.fired[0].actor_slot, Some(2));
        assert_eq!(batch.fired[0].target_slot, Some(5));
    }

    #[test]
    fn scheduler_long_delay_does_not_fire_early() {
        let mut s = SfxScheduler::new();
        s.enqueue(PendingCue::new(0x1A, 60));
        for _ in 0..60 {
            assert!(s.tick_frame().fired.is_empty());
        }
        assert_eq!(s.tick_frame().fired.len(), 1);
    }

    #[test]
    fn ring_cue_fires_once_through_the_scheduler_and_is_then_cleared() {
        let mut s = SfxScheduler::new();
        s.arm_ring_cue(0, 0x1A, 0);
        let ids: Vec<u16> = s.tick_frame().fired.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![0x1A], "delay 0 fires on the arming frame");
        assert!(
            s.tick_frame().fired.is_empty(),
            "the aging pass cleared the slot - a ring cue never repeats"
        );
    }

    #[test]
    fn ring_delay_is_denominated_in_vsyncs_not_ticks() {
        let mut s = SfxScheduler::new();
        // Field cadence: one tick spans two vsyncs.
        s.set_frame_step(2);
        s.arm_ring_cue(1, 0x33, 4);
        assert!(s.tick_frame().fired.is_empty());
        assert!(s.tick_frame().fired.is_empty());
        let ids: Vec<u16> = s.tick_frame().fired.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![0x33], "4 vsyncs at cadence 2 = 2 ticks");
    }

    #[test]
    fn scheduler_clear_empties_the_ring_too() {
        let mut s = SfxScheduler::new();
        s.arm_ring_cue(2, 0x40, 0);
        s.clear();
        assert!(s.tick_frame().fired.is_empty());
    }

    #[test]
    fn scheduler_mixed_delays_fire_in_correct_frames() {
        let mut s = SfxScheduler::new();
        s.enqueue(PendingCue::new(0xA, 1));
        s.enqueue(PendingCue::new(0xB, 3));
        s.enqueue(PendingCue::new(0xC, 0));
        // Frame 1: C fires.
        let f1 = s.tick_frame();
        assert_eq!(f1.fired.len(), 1);
        assert_eq!(f1.fired[0].id, 0xC);
        // Frame 2: A fires.
        let f2 = s.tick_frame();
        assert_eq!(f2.fired.len(), 1);
        assert_eq!(f2.fired[0].id, 0xA);
        // Frame 3: nothing.
        assert!(s.tick_frame().fired.is_empty());
        // Frame 4: B fires.
        let f4 = s.tick_frame();
        assert_eq!(f4.fired.len(), 1);
        assert_eq!(f4.fired[0].id, 0xB);
    }

    #[test]
    fn classify_cue_low_range_stores_id_minus_one() {
        // id < 0x40: ring_value = id - 1, dedup_key = id - 1.
        match classify_cue(0x05) {
            CueDispatch::Ring {
                ring_value,
                dedup_key,
            } => {
                assert_eq!(ring_value, 4);
                assert_eq!(dedup_key, 4);
            }
            _ => panic!("expected Ring"),
        }
        // id 0 underflows to 0xFFFF / -1 (mirrors the retail uint - 1).
        match classify_cue(0) {
            CueDispatch::Ring {
                ring_value,
                dedup_key,
            } => {
                assert_eq!(ring_value, 0xFFFF);
                assert_eq!(dedup_key, -1);
            }
            _ => panic!("expected Ring"),
        }
    }

    #[test]
    fn classify_cue_high_range_stores_id_with_offset_dedup() {
        // 0x40..0x100: ring_value = id, dedup_key = id + 0x19C.
        match classify_cue(0x50) {
            CueDispatch::Ring {
                ring_value,
                dedup_key,
            } => {
                assert_eq!(ring_value, 0x50);
                assert_eq!(dedup_key, 0x50 + 0x19C);
            }
            _ => panic!("expected Ring"),
        }
    }

    #[test]
    fn classify_cue_voice_range_remaps_channel() {
        // id >= 0x100: voice. v = id - 0x100; channel = remap(v >> 3),
        // submode = v & 7, pitch_index = v.
        // v = 8 -> v>>3 = 1 -> channel 0x1A; submode 0; pitch_index 8.
        match classify_cue(0x108) {
            CueDispatch::Voice {
                channel,
                submode,
                pitch_index,
            } => {
                assert_eq!(channel, 0x1A);
                assert_eq!(submode, 0);
                assert_eq!(pitch_index, 8);
            }
            _ => panic!("expected Voice"),
        }
        // v = 0x1B -> v>>3 = 3 -> channel 0x1B; submode 3.
        match classify_cue(0x100 + 0x1B) {
            CueDispatch::Voice {
                channel, submode, ..
            } => {
                assert_eq!(channel, 0x1B);
                assert_eq!(submode, 3);
            }
            _ => panic!("expected Voice"),
        }
        // v = 0x28 -> v>>3 = 5 -> channel 0x1C.
        match classify_cue(0x128) {
            CueDispatch::Voice { channel, .. } => assert_eq!(channel, 0x1C),
            _ => panic!("expected Voice"),
        }
        // v = 0x10 -> v>>3 = 2 (no remap) -> channel 2.
        match classify_cue(0x110) {
            CueDispatch::Voice { channel, .. } => assert_eq!(channel, 2),
            _ => panic!("expected Voice"),
        }
    }

    #[test]
    fn cue_ring_suppression_matches_current_selection() {
        let cue = classify_cue(0x05); // dedup_key = 4
        assert!(cue.ring_suppressed_by(4));
        assert!(!cue.ring_suppressed_by(3));
        // Voice cues are never ring-suppressed.
        assert!(!classify_cue(0x108).ring_suppressed_by(0));
    }

    #[test]
    fn voice_pitch_rounds_up_times_point_six() {
        // (100 * 0x3C + 99) / 100 = (6000 + 99)/100 = 60.
        assert_eq!(voice_pitch(100), 60);
        // (1 * 60 + 99)/100 = 159/100 = 1 (round-up of 0.6).
        assert_eq!(voice_pitch(1), 1);
        assert_eq!(voice_pitch(0), 0);
        // (200*60+99)/100 = 12099/100 = 120.
        assert_eq!(voice_pitch(200), 120);
    }

    #[test]
    fn play_one_shot_keys_on_a_voice_through_an_uploaded_bank() {
        use crate::spu::Spu;
        use crate::vab_bind::{UploadedVag, VabBank, VabProgram};
        use legaia_vab::VagAtr;

        // Minimal one-program bank: a single tone covering every note, bound
        // to VAG #1 (1-based, so samples[0]).
        let tone = VagAtr {
            prior: 0,
            mode: 0,
            vol: 127,
            pan: 64,
            center: 60,
            shift: 0,
            min: 0,
            max: 127,
            vibw: 0,
            vibt: 0,
            porw: 0,
            port: 0,
            pbmin: 0,
            pbmax: 0,
            reserved1: 0,
            reserved2: 0,
            adsr1: 0,
            adsr2: 0,
            prog: 0,
            vag: 1,
            reserved3: [0; 4],
        };
        let vab = VabBank {
            master_vol: 127,
            samples: vec![Some(UploadedVag {
                addr: 0x1010,
                size: 0x20,
            })],
            programs: vec![VabProgram {
                mvol: 0x7F,
                mpan: 0x40,
                tones: vec![tone],
            }],
        };
        let mut spu = Spu::new();

        // Bank maps cue 0x1A -> program 0, tone 0, note 60, 1 voice. Firing
        // it claims the first idle voice and keys it on.
        let bank = SfxBank::from_descriptors([(0x1A, 0, 0, 60, 1)]);
        let voice = bank.play_one_shot(0x1A, &mut spu, &vab);
        assert_eq!(voice, Some(0), "first idle voice keyed on");
        assert!(!spu.voices[0].is_off(), "voice 0 is now playing");

        // A cue id not in the bank is a no-op and never touches the SPU.
        assert_eq!(bank.play_one_shot(0x4C, &mut spu, &vab), None);
    }

    /// The core regression this fix targets: a cue whose descriptor `note`
    /// falls OUTSIDE the target tone's authored `min..=max` key window still
    /// sounds, because the SFX path resolves the tone by explicit **index**
    /// (`play_tone`), not by the sequencer's key-range window (`play_note`).
    /// This mirrors the retail strike cue `0x1A` (program 3 / tone 8 / note
    /// 67), whose tone's window excludes 67.
    #[test]
    fn play_one_shot_resolves_tone_by_index_not_key_range() {
        use crate::spu::Spu;
        use crate::vab_bind::{UploadedVag, VabBank, VabProgram};
        use legaia_vab::VagAtr;

        let mk = |center: u8, min: u8, max: u8| VagAtr {
            prior: 0,
            mode: 0,
            vol: 127,
            pan: 64,
            center,
            shift: 0,
            min,
            max,
            vibw: 0,
            vibt: 0,
            porw: 0,
            port: 0,
            pbmin: 0,
            pbmax: 0,
            reserved1: 0,
            reserved2: 0,
            adsr1: 0,
            adsr2: 0,
            prog: 0,
            vag: 1,
            reserved3: [0; 4],
        };
        // Program 0 has two tones. The cue targets tone index 1, whose key
        // window is 0..=40 - which does NOT contain the descriptor note 67.
        let vab = VabBank {
            master_vol: 127,
            samples: vec![Some(UploadedVag {
                addr: 0x1010,
                size: 0x20,
            })],
            programs: vec![VabProgram {
                mvol: 0x7F,
                mpan: 0x40,
                tones: vec![mk(60, 50, 60), mk(30, 0, 40)],
            }],
        };
        let bank = SfxBank::from_descriptors([(0x1A, 0, 1, 67, 1)]);

        // Key-range lookup would miss tone 1 (67 outside 0..=40) -> silence.
        let mut spu_kr = Spu::new();
        assert!(
            !vab.play_note(&mut spu_kr, 0, 0, 67, 100),
            "key-range resolve finds no tone for note 67 (the silence bug)"
        );

        // The one-shot path resolves tone index 1 directly and sounds.
        let mut spu = Spu::new();
        let voice = bank.play_one_shot(0x1A, &mut spu, &vab);
        assert_eq!(voice, Some(0), "tone-index resolve keys on a voice");
        assert!(!spu.voices[0].is_off(), "voice 0 plays");
    }

    /// A multi-voice cue keys `voices` consecutive regions on consecutive
    /// idle voices (`FUN_80016B6C`'s `tone + i` fan-out).
    #[test]
    fn play_one_shot_multi_voice_keys_consecutive_regions() {
        use crate::spu::Spu;
        use crate::vab_bind::{UploadedVag, VabBank, VabProgram};
        use legaia_vab::VagAtr;

        let tone = |center: u8| VagAtr {
            prior: 0,
            mode: 0,
            vol: 127,
            pan: 64,
            center,
            shift: 0,
            min: 0,
            max: 127,
            vibw: 0,
            vibt: 0,
            porw: 0,
            port: 0,
            pbmin: 0,
            pbmax: 0,
            reserved1: 0,
            reserved2: 0,
            adsr1: 0,
            adsr2: 0,
            prog: 0,
            vag: 1,
            reserved3: [0; 4],
        };
        let vab = VabBank {
            master_vol: 127,
            samples: vec![Some(UploadedVag {
                addr: 0x1010,
                size: 0x20,
            })],
            programs: vec![VabProgram {
                mvol: 0x7F,
                mpan: 0x40,
                tones: vec![tone(60), tone(48), tone(72)],
            }],
        };
        let mut spu = Spu::new();
        // Cue 0x4C: program 0, tone base 0, note 64, 2 voices -> regions 0 & 1
        // key on voices 0 & 1.
        let bank = SfxBank::from_descriptors([(0x4C, 0, 0, 64, 2)]);
        let voice = bank.play_one_shot(0x4C, &mut spu, &vab);
        assert_eq!(voice, Some(0), "first voice returned");
        assert!(!spu.voices[0].is_off(), "voice 0 playing");
        assert!(!spu.voices[1].is_off(), "voice 1 playing (2nd of the cue)");
        assert!(spu.voices[2].is_off(), "only two voices keyed");
    }

    #[test]
    fn scheduler_enqueue_all_appends_in_order() {
        let mut s = SfxScheduler::new();
        s.enqueue_all([
            PendingCue::new(0x1, 0),
            PendingCue::new(0x2, 1),
            PendingCue::new(0x3, 0),
        ]);
        assert_eq!(s.pending_count(), 3);
        // Frame 1: 0x1 and 0x3 fire (both delay=0).
        let batch = s.tick_frame();
        let ids: Vec<u16> = batch.fired.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![0x1, 0x3]);
    }
}
