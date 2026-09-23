//! Audio-side state: BGM selection and resume, the SFX cue / delay slots, sound-bank handshakes and the battle SFX / XA / shout cue queues.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Audio-side state: BGM selection and resume, the SFX cue / delay slots, sound-bank handshakes and the battle SFX / XA / shout cue queues.
pub struct AudioState {
    /// "Sound bank ready" gate.
    pub sound_bank_ready: bool,
    /// Per-strike battle sound cues surfaced this frame for the host to play
    /// through its SFX bank (the art-record `HitCue` sound cues that
    /// [`crate::world::World::fold_battle_event`] resolves from an `ApplyArtStrike` outcome -
    /// previously dropped). Cosmetic, like [`crate::world::BattleState::hit_fx`]: no gameplay
    /// state depends on them. Drained via [`crate::world::World::drain_battle_sfx_cues`];
    /// cleared on battle exit.
    pub battle_sfx_cues: Vec<BattleSfxCue>,
    /// CD-XA one-shot clip requests the battle raised this tick - the
    /// `FUN_8003D53C(clip, channel, dur)` calls the melee kernel makes (the
    /// per-character `XA30` grunt) and the party voice leg of the sound
    /// funnel resolves (`XA27` for `0x10C`). Drained by the hosts into the
    /// XA mixing path ([`crate::world::World::drain_battle_xa_cues`]).
    pub battle_xa_cues: Vec<crate::sfx_cue::XaVoiceClip>,
    /// Frames the modelled CD drive stays busy after a clip start - the
    /// read span in vsyncs (`dur * 2.5` sectors at 150/s = `dur / 60` s). The
    /// funnel's voice leg drops a request while it is non-zero
    /// (`FUN_8003DE7C(1) != 0` at `0x8004FE9C`), so two `0x10C` stings
    /// inside one read span collapse to the first. Counted down once per
    /// battle tick.
    pub battle_xa_busy_frames: u16,
    /// The static `SCUS_942.54` XA cue duration table (`DAT_800788B8`) the
    /// voice legs read (`legaia_asset::xa_cue_table`); installed at boot,
    /// `None` on a disc-free build (a voice cue then requests no span and
    /// is dropped).
    pub xa_cue_durations: Option<Vec<u16>>,
    /// Tactical-Arts **shout** cues queued this frame - one per executed
    /// party art carrying a real action constant, pushed on the art's
    /// animation-start frame (see [`crate::battle_events::BattleShoutCue`]).
    /// Cosmetic: the host resolves each against the arts-voice tables + XA
    /// clip banks and plays the CD-XA shout; nothing here mutates gameplay
    /// state. Drained via [`crate::world::World::drain_battle_shout_cues`]; cleared on
    /// battle exit.
    pub battle_shout_cues: Vec<crate::battle_events::BattleShoutCue>,
    /// Last BGM the field VM started (op 0x35 sub-1 / sub-9). `None` until
    /// a scene starts one. Updated synchronously when the VM emits the
    /// corresponding `Bgm` event.
    pub current_bgm: Option<u16>,
    /// BGM id to swap to when a live-loop encounter begins, restored to the
    /// field track when the battle ends. `None` (the default) leaves music
    /// untouched across the Battle transition - set it via
    /// [`crate::world::World::set_battle_bgm`] to enable the swap. The swap is routed as an
    /// ordinary `FieldEvent::Bgm` start (sub-op 1), so the host's existing
    /// BGM director resolves the SEQ and cross-fades exactly like a field
    /// op-`0x35` start.
    pub battle_bgm: Option<u16>,
    /// Field track stashed at battle entry so `World::restore_field_bgm`
    /// can resume it after the encounter. Managed by the swap helpers; not
    /// meant to be set directly.
    pub field_bgm_resume: Option<u16>,
    /// `true` while the battle track is playing (set by
    /// `World::swap_to_battle_bgm`, cleared by
    /// `World::restore_field_bgm`). Guards against double-swap / spurious
    /// restore.
    pub battle_bgm_active: bool,
    /// Field track stashed when an in-world minigame's overlay init took the
    /// score over with its own global-pool track
    /// ([`crate::world::World::swap_to_minigame_bgm`]), resumed by
    /// [`crate::world::World::restore_minigame_bgm`] on the mode-24 return
    /// warp. Managed by that pair; not meant to be set directly.
    pub minigame_bgm_resume: Option<u16>,
    /// `true` while a minigame's own track owns the director. Guards the
    /// restore so a minigame that started no track of its own (the slot
    /// machine, fishing - both inherit the host scene's BGM) does not
    /// re-emit the field track on exit.
    pub minigame_bgm_active: bool,
    /// Retail's timed sound-source auto-release (`gp+0x808`/`0x814`/`0x81C`),
    /// serviced by the frame-begin driver. Advanced by [`crate::world::World::tick`] on the
    /// sim ticks that map to a retail vsync, by [`crate::world::FrameClock::frame_step`] - the
    /// same cadence-invariant clock every other retail duration uses.
    ///
    /// REF: FUN_800267FC, FUN_8001698C
    pub sound_release: crate::sound_state::SoundReleaseTimer,
    /// Set by [`crate::world::World::tick`] on the frame [`crate::world::AudioState::sound_release`] expires;
    /// hosts drain it with [`crate::world::World::take_pending_sound_release`] and stop the
    /// bound voice. Retail does the stop inline through libsnd
    /// (`FUN_8002657C` + `FUN_80064370`), which the engine replaces with its
    /// own voice pool - so the port surfaces the *event*, not the teardown.
    pub pending_sound_release: bool,
    /// The five `gp` cells retail's **arm** half writes alongside
    /// [`crate::world::AudioState::sound_release`] (`gp+0x80C`/`0x810`), latched by
    /// [`crate::world::World::arm_sound_release`]. `None` until the field VM's BGM op
    /// sub-`5` arms the release.
    ///
    /// REF: FUN_800267A8
    pub sound_arm: Option<crate::scus_leaf_kernels::TimedSoundArm>,
    /// The per-slot SFX-cue delay table (`DAT_8007C338`) the field VM's op
    /// `0x36` sub-`4` writes.
    ///
    /// REF: FUN_80035BAC
    pub sfx_cue_delays: crate::scus_leaf_kernels::SfxCueDelays,
    /// This frame's calls into the SFX cue ring's producer trio, in call
    /// order, for the host's audio scheduler to replay onto its own ring
    /// (`legaia_engine_audio::SfxScheduler`). Retail's producers write the
    /// ring directly; the engine's ring lives with the SPU on the host side of
    /// the crate boundary, so the calls cross it as data. Drained by
    /// [`crate::world::World::take_sfx_ring_ops`].
    pub sfx_ring_ops: Vec<SfxRingOp>,
    /// The slot the SFX enqueue last parked (`gp+0x15A`) - the index
    /// [`crate::world::AudioState::sfx_cue_delays`] is written through.
    pub sfx_parked_slot: i16,
    /// The enqueue's round-robin write cursor (`gp+0x158`), wrapping at
    /// [`crate::scus_leaf_kernels::SFX_CUE_SLOTS`].
    pub sfx_cue_cursor: i16,
    /// The side-band sound-bank request / acknowledge pair
    /// `_DAT_8007BABC` / `_DAT_8007BAA0`, which op `0x36`'s bit-15 subs
    /// `1` and `2` drive and whose settled state gates sub `0` and the
    /// whole bit-15-clear XA arm.
    ///
    /// REF: FUN_800243F0
    pub sound_stream: crate::scus_leaf_kernels::SoundStreamRequest,
    /// `_DAT_8007B868` - the dev/dual-mode gate. Retail boots it `0` and no
    /// static writer ever sets it non-zero, so the engine keeps it `0`; the
    /// field applies it as retail does (it *skips* op `0x36`'s whole
    /// bit-15-set arm and *bypasses* the bit-15-clear arm's stream barrier).
    pub dual_mode_gate: i32,
    /// The one-shot sound-detach latch (`gp+0x804`). Idempotent: the mode-INIT
    /// chain can call it repeatedly and only the first has any effect.
    ///
    /// REF: FUN_8002689C
    pub sound_detach: crate::sound_state::SoundDetachLatch,
    /// Which bank the SPU region VAB slots `2` and `6` share holds, and the
    /// field-bank latch `0x8007BAFC` - see
    /// [`crate::world::World::sync_sfx_residency`].
    pub residency: crate::world::SfxBankResidency,
    /// SPU voices a field-VM op asked the host to stop this tick (the
    /// side-band teardown's `FUN_800653C8(0x17)` / `(0x16)`), drained by
    /// [`crate::world::World::take_sfx_voice_stops`].
    pub sfx_voice_stops: Vec<u8>,
}

impl AudioState {
    pub fn new() -> Self {
        Self {
            sound_bank_ready: true,
            battle_sfx_cues: Vec::new(),
            battle_xa_cues: Vec::new(),
            battle_xa_busy_frames: 0,
            xa_cue_durations: None,
            battle_shout_cues: Vec::new(),
            current_bgm: None,
            battle_bgm: None,
            field_bgm_resume: None,
            battle_bgm_active: false,
            minigame_bgm_resume: None,
            minigame_bgm_active: false,
            sound_release: crate::sound_state::SoundReleaseTimer::default(),
            pending_sound_release: false,
            sound_arm: None,
            sfx_cue_delays: crate::scus_leaf_kernels::SfxCueDelays::new(
                crate::scus_leaf_kernels::SFX_CUE_SLOTS,
            ),
            sfx_ring_ops: Vec::new(),
            sfx_parked_slot: 0,
            sfx_cue_cursor: 0,
            // Retail's field init writes `(8, -1)` (`0x801D6880`) and
            // `FUN_800243F0` latches it settled on the next frame. The
            // engine's bank loads are synchronous - there is no in-flight
            // window - so the pair is born settled instead, the same way the
            // BGM barrier is satisfied on arrival.
            sound_stream: crate::scus_leaf_kernels::SoundStreamRequest::IDLE_PAIR,
            dual_mode_gate: 0,
            sound_detach: crate::sound_state::SoundDetachLatch::default(),
            residency: crate::world::SfxBankResidency::default(),
            sfx_voice_stops: Vec::new(),
        }
    }
}

impl Default for AudioState {
    fn default() -> Self {
        Self::new()
    }
}

/// One call into the retail SFX cue ring's producer trio.
///
/// The ring itself is `legaia_engine_audio::sfx_ring::SfxCueRing` - two
/// parallel four-slot arrays, `DAT_8007B6D8` (cue ids) and `DAT_8007C338`
/// (countdowns in vsyncs), with the round-robin cursor at `gp+0x158` and the
/// last-written slot at `gp+0x15A`. The three producers are read off
/// `0x80035B50..0x80035BFC` and each maps one-to-one onto a scheduler call:
///
/// | Variant | Retail | Scheduler |
/// |---|---|---|
/// | [`Self::Push`] | `FUN_80035B50(id)` | `push_ring_cue` |
/// | [`Self::SetLastDelay`] | `FUN_80035BAC(delay)` | `set_ring_cue_delay` |
/// | [`Self::ReplaceLast`] | `FUN_80035BD0(id)` | `replace_ring_cue` |
///
/// The id is the **resolved** ring id the drainer `FUN_80016B6C` consumes:
/// below `0x200` a row of the static `DAT_8006F198` table, at or above it a
/// row of the runtime bank ([`crate::world::World::runtime_sfx_descriptor`]).
///
/// REF: FUN_80035B50, FUN_80035BAC, FUN_80035BD0
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfxRingOp {
    /// `FUN_80035B50(id)` - write `id` into the cursor's slot with a zero
    /// countdown and advance the cursor.
    Push(i16),
    /// `FUN_80035BAC(delay)` - set the last-written slot's countdown.
    SetLastDelay(i16),
    /// `FUN_80035BD0(id)` - overwrite the last-written slot's id, zero its
    /// countdown, leave the cursor.
    ReplaceLast(i16),
}

/// Where a side-band bank request lands: the VAB slot `FUN_800243F0` installs
/// it into and the PROT entry (extraction index) it streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideBandBank {
    /// The request id (`_DAT_8007BABC`) this resolves.
    pub request: i32,
    /// VAB slot: `3` (the side-band slot) or `6` (the field bank's slot, which
    /// a `>= 3000` request refills).
    pub slot: u8,
    /// Extraction-frame PROT index (raw TOC index `- 2`).
    pub prot_entry: u32,
}

/// Raw in-RAM TOC index of the `vab_01` block, `*(0x8007BBE4)` - `1072`, the
/// CDNAME `#define vab_01 1072`. Read as a runtime word from every catalogued
/// mednafen state checked (field and battle alike).
pub const VAB_01_RAW_BASE: u32 = 1072;

/// The side-band request the field overlay seeds at init (`(8, -1)` at
/// `0x801D6880`), which the driver settles on the next frame.
pub const FIELD_INIT_SIDE_BAND_REQUEST: i32 = 8;

/// The streaming slots' park sentinel: a `0x1000` request copies itself onto
/// the acknowledge cell (`0x800244CC..0x800244F0`) and loads nothing, so the
/// slot keeps whatever bank it held.
pub const SIDE_BAND_PARK: i32 = 0x1000;

/// Resolve a side-band bank request the way `FUN_800243F0`'s second streaming
/// slot does (`0x800248B4..0x8002494C`, read off the disassembly).
///
/// The arms run in sequence and later ones overwrite earlier ones:
///
/// 1. `s0 = id + *(0x80084540)`, `s1 = 3` - a scene-local index;
/// 2. `1000 <= id < 2000`: `s0 = id + base - 1000`, `s1 = 6`;
/// 3. `2000 <= id < 3000`: `s0 = *(0x8007BBE4) + id - 2000`, `s1 = 3`;
/// 4. `id >= 3000`: `s0 = *(0x8007BBE4) + id - 3000`, `s1 = 6`;
/// 5. `id < 2000`: `s0 = *(0x8007BBE4) + 2`, and `gp+0x72C = id`.
///
/// Arm 5 overwrites both scene-local indices, so arms 1 and 2 survive only as
/// the slot choice: every request below 2000 streams `vab_01 + 2`. The field
/// init request `8` is one of those. The disc's scripts request only the two
/// global arms (every op-`0x36` sub-`1` operand is `>= 2000`).
///
/// Returns `None` for the park sentinel and for a negative (idle) id.
pub fn side_band_bank_for_request(id: i32) -> Option<SideBandBank> {
    if id < 0 || id == SIDE_BAND_PARK {
        return None;
    }
    let (raw, slot) = if id < 2000 {
        let slot = if (1000..2000).contains(&id) { 6 } else { 3 };
        (VAB_01_RAW_BASE + 2, slot)
    } else if id < 3000 {
        (VAB_01_RAW_BASE + (id - 2000) as u32, 3)
    } else {
        (VAB_01_RAW_BASE + (id - 3000) as u32, 6)
    };
    Some(SideBandBank {
        request: id,
        slot,
        prot_entry: raw.checked_sub(2)?,
    })
}

impl World {
    /// Drain this frame's SFX ring producer calls ([`SfxRingOp`]), oldest
    /// first. Both hosts replay them onto their `SfxScheduler` before its
    /// per-frame tick, which is the order retail's frame runs them in
    /// (producers inside the game-logic phase, the drainer after).
    pub fn take_sfx_ring_ops(&mut self) -> Vec<SfxRingOp> {
        std::mem::take(&mut self.audio.sfx_ring_ops)
    }

    /// Queue `FUN_80035B50(id)` - the push producer - for the host ring, and
    /// advance the engine-core mirror of the cursor pair the way field-VM op
    /// `0x36` sub `0` does: write the cursor's slot, park it, advance.
    pub fn push_sfx_cue(&mut self, id: i16) {
        let slot = self.audio.sfx_cue_cursor;
        self.audio.sfx_cue_cursor = self.audio.sfx_cue_delays.park(slot);
        self.audio.sfx_parked_slot = slot;
        self.audio.sfx_ring_ops.push(SfxRingOp::Push(id));
    }

    /// Queue `FUN_80035BD0(id)` - the overwrite producer - for the host ring.
    /// The engine-core mirror of the cursor pair is unchanged by it (it
    /// neither advances the cursor nor moves the parked slot); only the parked
    /// slot's delay is zeroed, as retail's `sw zero` does.
    pub fn replace_last_sfx_cue(&mut self, id: i16) {
        let parked = self.audio.sfx_parked_slot;
        self.audio.sfx_cue_delays.set_delay(parked, 0);
        self.audio.sfx_ring_ops.push(SfxRingOp::ReplaceLast(id));
    }

    /// The runtime-bank descriptor row for ring id `id` (`>= 0x200`), out of
    /// the current field scene's prescript bundle.
    ///
    /// `FUN_80016B6C` `0x80016C30..0x80016C70`: load the current-bundle
    /// pointer `gp+0x5B8` (`_DAT_8007B8D0`), take the bundle header's `+2`
    /// halfword (`offsets[0]`, rounded toward zero to even), and index
    /// `(id - 0x200) * 8` past it. In the field that bundle is the scene's
    /// prescript (`docs/formats/sfx-table.md`), which the engine holds as
    /// [`crate::world::FieldPropState::stager_bytes`]. Returns `None` below
    /// `0x200`, with no bundle, or past its end - retail reads whatever lies
    /// there, the port declines to.
    // REF: FUN_80016B6C
    pub fn runtime_sfx_descriptor(&self, id: i16) -> Option<[u8; 8]> {
        runtime_sfx_descriptor_in(&self.props.stager_bytes, id)
    }

    /// The side-band bank the driver holds for the current request pair.
    ///
    /// The engine's loads are synchronous, so the acknowledged id is what the
    /// slot holds. An idle pair in a field-family mode reads as the field
    /// overlay's init request ([`FIELD_INIT_SIDE_BAND_REQUEST`]), which retail
    /// settles one frame after field init - the engine's scene entry does not
    /// replay that seed, so it is supplied here. A park leaves `None` (the
    /// host keeps whatever bank it staged).
    pub fn side_band_bank(&self) -> Option<SideBandBank> {
        let pair = self.audio.sound_stream;
        let id = if pair.acked == crate::scus_leaf_kernels::SoundStreamRequest::IDLE
            && pair.requested == crate::scus_leaf_kernels::SoundStreamRequest::IDLE
        {
            if matches!(self.mode, SceneMode::Field | SceneMode::WorldMap) {
                FIELD_INIT_SIDE_BAND_REQUEST
            } else {
                return None;
            }
        } else {
            pair.acked
        };
        side_band_bank_for_request(id)
    }
}

/// [`World::runtime_sfx_descriptor`] over an explicit bundle.
pub fn runtime_sfx_descriptor_in(bundle: &[u8], id: i16) -> Option<[u8; 8]> {
    if id < 0x200 {
        return None;
    }
    let hdr = i16::from_le_bytes([*bundle.get(2)?, *bundle.get(3)?]);
    // `sra 16; srl 31; addu; sra 1; sll 1` - round toward zero to even.
    let rec0 = usize::try_from((hdr / 2) * 2).ok()?;
    let at = rec0.checked_add(usize::from((id - 0x200) as u16) * 8)?;
    bundle.get(at..at + 8)?.try_into().ok()
}

#[cfg(test)]
mod sfx_ring_op_tests {
    use super::*;

    #[test]
    fn side_band_requests_resolve_through_the_vab_01_block() {
        // town01's own request: raw 1074 = extraction 1072, slot 3.
        let b = side_band_bank_for_request(2002).unwrap();
        assert_eq!((b.slot, b.prot_entry), (3, 1072));
        // >= 3000 refills slot 6 from the same block.
        let b = side_band_bank_for_request(3001).unwrap();
        assert_eq!((b.slot, b.prot_entry), (6, 1071));
        // Below 2000 every id streams vab_01 + 2; only the slot varies.
        assert_eq!(side_band_bank_for_request(8).unwrap().prot_entry, 1072);
        assert_eq!(side_band_bank_for_request(8).unwrap().slot, 3);
        assert_eq!(side_band_bank_for_request(1500).unwrap().slot, 6);
        assert_eq!(side_band_bank_for_request(1500).unwrap().prot_entry, 1072);
        // Park and idle load nothing.
        assert!(side_band_bank_for_request(SIDE_BAND_PARK).is_none());
        assert!(side_band_bank_for_request(-1).is_none());
    }

    #[test]
    fn runtime_rows_index_past_record_zero() {
        // [u16 count = 2][u16 offsets = 6, 0x16] then two rows.
        let mut b = vec![2u8, 0, 6, 0, 0x16, 0];
        b.extend_from_slice(&[1, 2, 60, 1, 3, 0, 0, 0]);
        b.extend_from_slice(&[4, 5, 61, 2, 3, 0, 0, 0]);
        assert_eq!(
            runtime_sfx_descriptor_in(&b, 0x201),
            Some([4, 5, 61, 2, 3, 0, 0, 0])
        );
        assert_eq!(runtime_sfx_descriptor_in(&b, 0x200).unwrap()[0], 1);
        assert!(runtime_sfx_descriptor_in(&b, 0x1FF).is_none());
        assert!(runtime_sfx_descriptor_in(&b, 0x202).is_none());
        // An odd header word rounds toward zero, as `sra 1; sll 1` does.
        let mut odd = b.clone();
        odd[2] = 7;
        assert_eq!(runtime_sfx_descriptor_in(&odd, 0x200).unwrap()[0], 1);
    }

    #[test]
    fn the_idle_pair_reads_as_the_field_init_request_only_in_the_field() {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        assert_eq!(w.side_band_bank().unwrap().request, 8);
        w.audio.sound_stream.request(2016);
        w.audio.sound_stream.settle();
        assert_eq!(w.side_band_bank().unwrap().prot_entry, 1070 + 16);
        w.audio.sound_stream = crate::scus_leaf_kernels::SoundStreamRequest::IDLE_PAIR;
        w.mode = SceneMode::Battle;
        assert!(w.side_band_bank().is_none());
    }
}
