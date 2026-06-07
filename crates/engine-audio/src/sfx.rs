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
/// `vag_index` references the [`VabBank::samples`] entry that holds the
/// sample's start address + ADPCM-coded loop point in SPU RAM.
/// `program_index` references the bank's program table entry that supplies
/// envelope + pitch shape. `key` / `vel` follow MIDI conventions
/// (0..=127). `voice_pref` lets the engine pin a cue to a specific SPU
/// voice - `None` means "first available."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxEntry {
    /// Cue id this entry handles. The art-record `HitCue::kind` byte for
    /// strike-fired cues; engines extend the namespace for menu blips,
    /// footstep cues, etc.
    pub id: u8,
    /// Index into [`VabBank::programs`].
    pub program_index: u8,
    /// MIDI-style note number (0..=127). Determines pitch via the
    /// VAB's per-program tone table.
    pub key: u8,
    /// MIDI-style velocity (0..=127). Engines map this to the SPU's
    /// voice volume.
    pub vel: u8,
    /// Optional preferred voice slot (0..=23). `None` = round-robin.
    pub voice_pref: Option<u8>,
}

impl SfxEntry {
    /// Construct an entry with the canonical "use first available voice"
    /// preference and unity velocity.
    pub fn new(id: u8, program_index: u8, key: u8) -> Self {
        Self {
            id,
            program_index,
            key,
            vel: 100,
            voice_pref: None,
        }
    }

    /// With explicit velocity.
    pub fn with_vel(self, vel: u8) -> Self {
        Self { vel, ..self }
    }

    /// With pinned voice.
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

    /// Build a bank from decoded SFX descriptors - the `(id, program, key)`
    /// triples [`legaia_asset::sfx_table::SfxTable::active`] yields (program =
    /// `SfxDescriptor::program`, key = `SfxDescriptor::note`). Keeping the
    /// argument a plain tuple iterator avoids a dependency on the asset crate
    /// from the audio layer; the host (engine-core) wires the disc-decoded
    /// table in. Later triples overwrite earlier ones with the same id.
    pub fn from_descriptors<I: IntoIterator<Item = (u8, u8, u8)>>(descriptors: I) -> Self {
        let mut bank = Self::new();
        for (id, program_index, key) in descriptors {
            bank.insert(SfxEntry::new(id, program_index, key));
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

    /// Fire one shot. Resolves the cue id, picks a target voice, and
    /// delegates to [`VabBank::play_note`] which handles tone lookup,
    /// pitch math, and ADSR setup. Returns the voice index used (or
    /// `None` if the cue isn't in the bank, or no free voice was
    /// available, or the bank's program / tone / sample is missing).
    pub fn play_one_shot(&self, id: u8, spu: &mut Spu, vab: &VabBank) -> Option<u8> {
        let entry = self.get(id)?;
        let voice_idx = match entry.voice_pref {
            Some(v) => v.min(23),
            None => first_idle_voice(spu)?,
        };
        if vab.play_note(
            spu,
            voice_idx as usize,
            entry.program_index as usize,
            entry.key,
            entry.vel,
        ) {
            Some(voice_idx)
        } else {
            None
        }
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
    /// (`DAT_8007B724`) to suppress an immediate repeat — see
    /// [`CueDispatch::ring_suppressed_by`].
    Ring { ring_value: u16, dedup_key: i32 },
    /// A streamed voice trigger (`FUN_8003D53C`). `channel` is the voice channel
    /// after the `1 → 0x1A`, `3 → 0x1B`, `5 → 0x1C` remap; `submode = id & 7`;
    /// `pitch_index` indexes the pitch table (`DAT_800788B8`) — feed its `u16`
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
/// note-on stay with the caller — the ring is [`SfxScheduler`] / `FUN_80035B50`).
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
}

impl SfxScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a cue. The cue fires when `frames_remaining` reaches zero
    /// during a [`Self::tick_frame`] call.
    ///
    // PORT: FUN_80035B50 - retail's SFX-cue enqueue writes the cue id into the
    // next slot of a fixed 4-entry u16 ring at &DAT_8007B6D8 and advances the
    // head; this models the same "queue a one-shot SFX" contract with an
    // unbounded queue + per-cue countdown instead of the 4-slot ring.
    // REF: FUN_80035BD0 - the retail "overwrite current slot" variant (replace
    // an in-flight cue without advancing the head, e.g. bonk overriding a
    // pending step) is not separately modeled; cues simply queue here.
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
    /// `frames_remaining` reached zero this tick (in queue order).
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
        batch
    }

    /// Drop every queued cue. Engines call this on scene transitions /
    /// battle abort.
    pub fn clear(&mut self) {
        self.queue.clear();
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
    fn bank_from_descriptors_maps_program_and_key() {
        // Mirrors the retail descriptors for ids 0x1A (p3 note67) and 0x4C
        // (p3 note64) decoded from DAT_8006F198.
        let bank = SfxBank::from_descriptors([(0x1A, 3, 67), (0x4C, 3, 64)]);
        assert_eq!(bank.len(), 2);
        let a = bank.get(0x1A).unwrap();
        assert_eq!((a.program_index, a.key), (3, 67));
        let b = bank.get(0x4C).unwrap();
        assert_eq!((b.program_index, b.key), (3, 64));
        // Later triple overwrites an earlier same-id one.
        let over = SfxBank::from_descriptors([(0x1A, 1, 10), (0x1A, 9, 90)]);
        assert_eq!(over.len(), 1);
        assert_eq!(over.get(0x1A).unwrap().program_index, 9);
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
        use crate::vab_bind::{UploadedVag, VabBank};
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
            programs: vec![vec![tone]],
        };
        let mut spu = Spu::new();

        // Bank maps cue 0x1A -> program 0, key 60 (the tone above). Firing it
        // claims the first idle voice and keys it on.
        let bank = SfxBank::from_descriptors([(0x1A, 0, 60)]);
        let voice = bank.play_one_shot(0x1A, &mut spu, &vab);
        assert_eq!(voice, Some(0), "first idle voice keyed on");
        assert!(!spu.voices[0].is_off(), "voice 0 is now playing");

        // A cue id not in the bank is a no-op and never touches the SPU.
        assert_eq!(bank.play_one_shot(0x4C, &mut spu, &vab), None);
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
