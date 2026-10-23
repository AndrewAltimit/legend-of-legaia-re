//! The per-tick steps both play hosts run around [`BootSession::tick`], held
//! here so each is written once.
//!
//! A play host's frame is the session tick plus a tail of steps the host
//! sequences itself, because some of them sit between two pieces of render
//! work only that host owns (a mesh upload, a VRAM re-read). The steps in
//! this file are the ones with no such host half: they read and write the
//! world and the audio director and nothing else, so the native window and
//! the browser play page call the same body and differ only in when their
//! frame reaches it.
//!
//! Each returns what a host still owes - a session to hold, a count for a
//! readout - rather than performing a host duty through a callback.

use legaia_engine_audio::AudioSink;
use legaia_engine_field::field_anim::FieldClipPlayer;
use legaia_engine_menus::game_over::GameOverSession;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_save_state::WorldSaveState;

use super::BootSession;
use crate::audio_director::SfxFrameReport;

/// Cue id fired for a footstep: `None`, and pinned there by capture -
/// **retail plays no footstep sound at all**, so the cadence
/// ([`BootSession::step_footstep_cadence`]) runs and keys no voice because
/// there is nothing to key.
///
/// The contrast that settles it is
/// `scripts/pcsx-redux/autorun_footstep_cue.lua`, which watches every cue path
/// at once - both ring producers (`FUN_80035B50` / `FUN_80035BD0`), the
/// dispatcher `FUN_8004FCC8`, the per-actor trigger `FUN_800250D4`, the voice
/// programmer `FUN_80065034`, and the four ring slots themselves - and runs one
/// field save state twice for the same number of vsyncs, once standing still
/// and once with the D-pad held. Standing still, a house-interior walk and a
/// kingdom-overworld walk each fire **nothing**. The one walk that fires
/// anything fires exactly two scene-script cues (`0x2E`, `0x2F`) hundreds of
/// vsyncs apart, out of the field VM's script SFX op - triggers the player
/// crossed, not a step cadence. Write-up: `docs/formats/sfx-table.md`.
///
/// So there is no retail id to copy, and a guessed one would not be a
/// near-miss but an arbitrary sample: an id resolves through the descriptor
/// table (`DAT_8006F198 + id*8`) to a `(program, tone)` pair, and that pair
/// selects a different sample in every resident bank. Firing `0x21` in a
/// field scene played an impact sample: walking punched.
///
/// Giving the port a footstep is therefore an *enhancement* choice - author a
/// cue and set it here, where every host's cadence step queues it - not a
/// fidelity gap waiting on more RE.
pub const CUE_FOOTSTEP: Option<u8> = None;

/// World-unit displacement per tick below which the player counts as still.
/// The controller steps 2 units at a time, so anything under one unit is
/// numerical drift rather than a walk.
const WALK_EPSILON: i32 = 1;

/// Movement magnitude handed to the footstep cadence while the player walks.
///
/// **This is a port pick, and it has to be, because the two engines do not
/// carry the same quantity.** Retail feeds `FUN_80018db0` a controller speed
/// word, and the kernel's own constants bound where that word must live for a
/// step to fire at all: `interval = 0xF - (min(speed + 0x20, 0xFA) >> 4)` and
/// the `interval < 0xB` gate together require `speed >= 0x30`, saturating at
/// `0xDA`. The port has no such word - `World` exposes a walking *flag* and a
/// 2-units-per-tick step, and feeding that raw world delta in leaves `interval`
/// at `0xD`, i.e. permanently below the gate, so no step would ever fire.
///
/// A single-speed walker therefore has to be placed somewhere in retail's
/// moving band, and `0x30` is the deliberately conservative end of it: the
/// slowest speed retail treats as moving, so the cadence this produces is the
/// slowest retail would ever produce for a walking player and cannot overstate
/// the step rate.
///
/// Capture adds one thing worth stating plainly: retail's own speed word does
/// **not** reach `0x30` while the player walks a field scene or the kingdom
/// overworld - `_DAT_8007B8A4` stays pinned at `2`, the gate's else-branch, for
/// every observed frame. Feeding `0x30` in here therefore makes the port's
/// cadence fire where retail's stays shut, which is fine precisely because
/// [`CUE_FOOTSTEP`] keys no voice: what runs is a timing counter, not a sound
/// retail does not make.
const WALK_SPEED_UNITS: i32 = 0x30;

/// One sim tick of the hosts' field-NPC clip players.
///
/// The players are host-side - each host keys its pose cache or its JSON on
/// them - but the step is one rule: while the field owns the frame
/// (`World::field_npc_clips_advance`), a slot the world drives is posed off
/// the world's cursor (`World::sync_npc_clip`, the actor's `+0x62` word as
/// `World::tick_npc_clips` left it this tick) and any other slot free-runs
/// one frame. Returns whether the players ran.
///
/// The gate is asked **per tick**, as retail's per-actor anim tick is
/// reached per field frame: a display frame that drains several ticks
/// across a mode edge (the tick that enters a battle, the one that returns
/// from it) advances the clips for exactly the ticks the field owned. A
/// host that asked once per redraw and advanced by the frame's tick count
/// ran the whole batch under whichever mode the last tick left.
///
/// A host calls it from its clip-cue step, after the tick's `A2` / `4C 51`
/// re-targets, so a clip bound this tick takes its first step this tick.
// REF: FUN_800204F8 (advance half; the world-driven cursor is `World::tick_npc_clips`)
pub fn step_npc_clip_players<'a>(
    world: &legaia_engine_core::world::World,
    players: impl IntoIterator<Item = (u8, &'a mut FieldClipPlayer)>,
) -> bool {
    if !world.field_npc_clips_advance() {
        return false;
    }
    for (slot, player) in players {
        if !world.sync_npc_clip(slot, player) {
            player.advance(1);
        }
    }
    true
}

impl<S: AudioSink> BootSession<S> {
    /// Push the persisted options' simulation knobs into the world and the
    /// follow camera: precise movement, the Field Move default, reduce
    /// flashing, the battle options, the tile windows and the ground fog
    /// (`legaia_engine_core::options::apply_to_world`), then the camera
    /// distance preset.
    ///
    /// Both play hosts call it on every sim tick ahead of the session tick,
    /// and again whenever an option changes. A scene or New Game transition
    /// may reseed world state, and the knobs are host policy (an options file,
    /// `localStorage`, a hotkey) rather than world state, so the re-assert is
    /// what keeps the two hosts' worlds from parting on a knob one of them
    /// pushed only at disc load.
    ///
    /// `precise_override` is a session-only state laid over the persisted
    /// option (the page's VR first-person drive); the native window passes
    /// `None`.
    pub fn apply_option_knobs(
        &mut self,
        opts: &legaia_engine_menus::options::OptionsState,
        precise_override: Option<bool>,
    ) {
        legaia_engine_core::options::apply_to_world(opts, &mut self.host.world);
        if let Some(on) = precise_override {
            self.host.world.locomotion.precise_movement = on;
        }
        self.camera.distance = opts.camera_distance;
    }

    /// Adopt the option words the last save load restored into the host's
    /// options, as retail's card load restores them with its live-state
    /// window: every options-screen row takes the save's value, except a row
    /// the session pinned (`OptionsPins::adopt_saved`). Returns whether the
    /// options changed, so the host can persist them and apply their side
    /// effects (the Sound row's monaural fold).
    ///
    /// Both play hosts call it on every sim tick, ahead of
    /// [`Self::apply_option_knobs`] - which would otherwise push the host's
    /// pre-load options back over the mirrors the load set.
    pub fn adopt_loaded_options(
        &mut self,
        opts: &mut legaia_engine_menus::options::OptionsState,
        pins: &mut legaia_engine_menus::options::OptionsPins,
    ) -> bool {
        match self.host.world.take_loaded_options() {
            Some(saved) => pins.adopt_saved(opts, saved),
            None => false,
        }
    }

    /// The party-wipe probe: consume the world's `game_over` flag and hand
    /// back the return-to-title hold the host runs from here.
    ///
    /// `frame_owned` is the one term only the host can answer - whether a
    /// screen of its own (the boot UI, a hold already running) owns the frame.
    /// While it does the flag is left raised and nothing is returned.
    ///
    /// The flag is consumed on the take, so a host cannot start a second hold
    /// from one wipe however long it keeps ticking. The world stays parked on
    /// the frozen battle frame (`World::game_over_hold`) until the host's hold
    /// resolves and it calls `World::resolve_game_over_hold`.
    ///
    /// Retail's wipe arm has one exit store - mode 22 CARD INIT with the
    /// title context word set - so the hold has one destination and asks the
    /// player nothing.
    // REF: FUN_8003AEB0 (`0x8003B5D0` / `0x8003B5E0`, the wipe arm's stores)
    pub fn take_party_wipe(&mut self, frame_owned: bool) -> Option<GameOverSession> {
        if frame_owned || !self.host.world.game_over {
            return None;
        }
        self.host.world.game_over = false;
        log::info!("party wipe -> title screen");
        Some(GameOverSession::new())
    }

    /// One frame of the return-to-title hold [`Self::take_party_wipe`]
    /// handed back. `true` on the frame it resolves: the world's deferred
    /// field restore is complete (`World::resolve_game_over_hold` - the hold
    /// froze it on the last battle frame) and the wipe's battle track is
    /// stopped, so a failed title-theme start leaves silence rather than the
    /// stale score. The host then drops the hold and raises its title screen.
    ///
    /// No input: retail's wipe arm offers the player nothing.
    pub fn step_party_wipe_hold(&mut self, hold: &mut GameOverSession) -> bool {
        use legaia_engine_menus::game_over::GameOverOutcome;
        use legaia_engine_scene_host::scene_host::BgmDirector;
        hold.tick();
        if hold.outcome() != Some(GameOverOutcome::ReturnToTitle) {
            return false;
        }
        self.host.world.resolve_game_over_hold();
        if let Some(bgm) = self.bgm.as_mut() {
            bgm.stop();
        }
        true
    }

    /// The field-overlay poll of the frame tail: open the shop (field-VM op
    /// `0x49` sub-0) or casino prize counter (sub-7) the field VM requested
    /// this tick, take the screen's first step on no edge
    /// (`MenuRuntime::poll_field_overlay`), and queue the blip that step
    /// raised on the director, to sound on this tick's audio frame.
    ///
    /// The press this tick carried already went to the field - typically the
    /// Cross that closed the merchant's line and ran the script into op
    /// `0x49` - so the screen steps on no edge. The field VM stays suspended
    /// (op-`0x49` Armed) until the player leaves.
    ///
    /// A host calls it once per sim tick after the session tick and ahead of
    /// [`Self::step_audio_frame`]. `menu` is the host's menu runtime, which
    /// its draw pass reads. Returns whether a screen opened.
    pub fn poll_field_overlay(
        &mut self,
        menu: &mut legaia_engine_menu_runtime::menu_runtime::MenuRuntime,
    ) -> bool {
        let Some(cue) = menu.poll_field_overlay(&mut self.host.world) else {
            return false;
        };
        if let (Some(cue), Some(bgm)) = (cue, self.bgm.as_mut()) {
            bgm.enqueue_sfx(u16::from(cue), 0, 0, 0);
        }
        true
    }

    /// The effect step of the frame tail: seat the pending move-FX and queue
    /// its sound cue on the director
    /// ([`crate::battle_fx::spawn_pending_move_fx`]), then advance the three
    /// move-VM effect scene-graphs one tick
    /// (`World::tick_effect_scene_graphs`: the Seru-magic summon, the battle
    /// move-FX, the field op-`0x34` stagers). Each half self-gates when
    /// nothing is live.
    ///
    /// A host seats the pending summon creature
    /// (`World::take_pending_summon_spawn`) before calling this, so the
    /// creature's first frame ticks on the tick that cast it - that seat is a
    /// mesh bind, which is the host's.
    ///
    /// Returns the ring value of the cue the spawned move carries, whether or
    /// not a director was attached to hear it.
    pub fn step_effect_scene_graphs(&mut self) -> Option<u16> {
        let ring = crate::battle_fx::spawn_pending_move_fx(&mut self.host.world);
        if let Some(ring_value) = ring {
            if let Some(bgm) = self.bgm.as_mut() {
                bgm.enqueue_sfx(ring_value, 0, 0, 0);
            }
            log::debug!("battle move-FX cue -> SFX {ring_value:#06x}");
        }
        self.host.world.tick_effect_scene_graphs();
        ring
    }

    /// Advance the Muscle Dome leg's round **time meter** one frame
    /// (`MuscleDomeSession::tick_time_meter`): it climbs through the
    /// selection phase and drains outside it. No-op with no leg open.
    ///
    /// Retail runs the meter from the arena's per-frame driver with the frame
    /// delta from scratchpad `0x1F800393`; the port ticks a fixed one per sim
    /// tick.
    pub fn step_muscle_time_meter(&mut self) {
        if let Some(s) = self.host.world.minigames.muscle_dome.as_mut() {
            s.tick_time_meter(1);
        }
    }

    /// Queue the minigame sessions' SFX cues (the fishing hub's and the point
    /// exchange's blips) on the director, to sound this tick. The dance's
    /// cues do not come this way: `World::tick_dance` stores them into the SFX
    /// ring, which [`BootSession::tick`] routes.
    ///
    /// The world's queue is drained with or without audio, so it cannot grow
    /// on a muted host. Returns how many cues it held.
    pub fn enqueue_minigame_sfx_cues(&mut self) -> u32 {
        let cues = self.host.world.drain_minigame_sfx_cues();
        if let Some(bgm) = self.bgm.as_mut() {
            for &cue in &cues {
                bgm.enqueue_sfx(cue, 0, 0, 0);
            }
        }
        cues.len() as u32
    }

    /// Queue the Baka Fighter duel's SFX cues (the exchange hit the rules
    /// kernel's damage step writes) on the director, to sound this tick. The
    /// duel has no actor or target slots, so the cue's HUD-context fields are
    /// zero. No-op outside the duel.
    ///
    /// The fight's buffer is drained with or without audio. Returns how many
    /// cues it held.
    pub fn enqueue_baka_sfx_cues(&mut self) -> u32 {
        if self.host.world.mode != SceneMode::BakaFighter {
            return 0;
        }
        let cues = self
            .host
            .world
            .minigames
            .baka_fighter
            .as_mut()
            .map(|f| f.take_cues())
            .unwrap_or_default();
        match self.bgm.as_mut() {
            Some(bgm) => {
                for &id in &cues {
                    bgm.enqueue_sfx(u16::from(id), 0, 0, 0);
                }
            }
            None => {
                for id in &cues {
                    log::debug!("baka SFX cue {id:#04x} (no audio)");
                }
            }
        }
        cues.len() as u32
    }

    /// The battle tick's **audio routing**: everything the fight queued for a
    /// host's sound this tick, handed to the director in one place.
    ///
    /// * the duck - a `DuckAudioLevel` among `events` (the summon / capture
    ///   arms lower the BGM to 75% of its reference, the Done band's `0x51`
    ///   arm raises it back) sets the director's target, which the audio
    ///   frame ramps one retail unit per tick;
    /// * the round's cast voices, staged ahead of their casts
    ///   (`AudioDirector::prestage_xa_clip`);
    /// * the one-shot CD-XA clips (the melee grunt / attack sting, a cast's
    ///   own voice), in the retail starter's `(clip, channel, dur)` terms;
    /// * the strike / cast SFX cues at their strike-relative timing
    ///   (`AudioDirector::enqueue_battle_cues`: full `u16` ids, classified at
    ///   fire time, and a results-frame `LEVEL_UP_CUE` first parks the
    ///   PROT 0889 reward bank behind the BGM);
    /// * the arts-voice shouts, one per executed party art, queued on the
    ///   art's animation-start frame.
    ///
    /// `events` is the tick's battle-event drain, which the host makes - it
    /// is **observation only** (`World::live_battle_tick` owns the gameplay
    /// fold and re-publishes the stream), and a host logs the same events.
    /// The world's four audio queues are drained with or without audio, so
    /// none grows on a muted host. SFX touch the SPU only - no RNG - so
    /// battle determinism stays bit-exact.
    ///
    /// A host calls it in its battle-event step, ahead of
    /// [`Self::step_audio_frame`]. Returns how many SFX cues the tick held.
    pub fn route_battle_audio(
        &mut self,
        events: &[legaia_engine_battle::battle_events::BattleEvent],
    ) -> u32 {
        use legaia_engine_battle::battle_events::BattleEvent;
        let world = &mut self.host.world;
        // The shout selector's one formation read (`gp+0x9F4`, formation
        // slot 0): retail reads the live cell at pick time, so it is handed
        // over every step rather than latched at battle entry.
        let first_enemy = world.formation_slot0_monster_id();
        let cues = world.drain_battle_sfx_cues();
        let shouts = world.drain_battle_shout_cues();
        let xa_cues = world.drain_battle_xa_cues();
        let xa_prestage = world.drain_battle_xa_prestage();
        let queued = cues.len() as u32;
        let Some(bgm) = self.bgm.as_mut() else {
            for cue in &cues {
                log::debug!(
                    "battle SFX cue {:#04x} @ +{} frames (actor {} -> target {}; no audio)",
                    cue.kind,
                    cue.timing_frames,
                    cue.actor_slot,
                    cue.target_slot
                );
            }
            return queued;
        };
        for ev in events {
            if let BattleEvent::DuckAudioLevel { target_pct } = ev {
                bgm.set_duck_pct(*target_pct);
            }
        }
        for xa in &xa_prestage {
            bgm.prestage_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
        }
        for xa in &xa_cues {
            let fired = bgm
                .play_xa_clip(xa.clip, xa.channel, xa.duration_sectors)
                .is_some();
            log::debug!(
                "battle XA clip slot {} ch {} dur {} -> {}",
                xa.clip,
                xa.channel,
                xa.duration_sectors,
                if fired { "playing" } else { "not playing" }
            );
        }
        bgm.enqueue_battle_cues(&cues, &self.host.index);
        bgm.set_shout_first_enemy_id(first_enemy);
        for shout in &shouts {
            match bgm.play_art_shout(shout.cslot, shout.action) {
                Some(ch) => log::debug!(
                    "arts shout cslot {} action {:#04x} fired on XA channel {ch}",
                    shout.cslot,
                    shout.action
                ),
                None => log::debug!(
                    "arts shout cslot {} action {:#04x} unvoiced / bank absent",
                    shout.cslot,
                    shout.action
                ),
            }
        }
        queued
    }

    /// The dance's per-tick tail, after the world tick: the auto-end, then the
    /// venue globals.
    ///
    /// * **Auto-end** (`World::finish_dance_if_over`). `World::tick_dance`'s
    ///   song-over arm restores the interrupted mode but leaves the run
    ///   installed so its final score can be read; closing it is what gives
    ///   the hall its own track back (`restore_minigame_bgm` queues the start
    ///   the tick's BGM routing plays) and clears the session. The finished
    ///   game is handed back for a host to log.
    /// * **Venue** (`dance_venue::sync_dance_venue`): the dance entry's
    ///   globals - block base, the session camera's view window, the venue
    ///   camera - staged on the first dance frame and restored on the first
    ///   frame after it, and the run's camera keyframe track advanced on
    ///   every staged frame.
    ///
    /// The two are independent - the auto-end acts only once the mode has
    /// left the dance, and the venue keys on the mode and its own stage - so
    /// one order serves both hosts.
    // REF: FUN_801CEF54, FUN_801D414C
    pub fn step_dance_tail(
        &mut self,
    ) -> (
        Option<legaia_engine_minigame_scenes::dance_venue::DanceVenueEdge>,
        bool,
    ) {
        let finished = self.host.world.finish_dance_if_over();
        if let Some(g) = finished.as_ref() {
            log::info!(
                "dance: song finished - score {} (pass={})",
                g.score(),
                g.passed()
            );
        }
        let world = &mut self.host.world;
        let edge = legaia_engine_minigame_scenes::dance_venue::sync_dance_venue(
            world.mode,
            &mut world.minigames,
            &mut world.battle.map_id,
            &mut self.camera,
        );
        (edge, finished.is_some())
    }

    /// Sound what the Muscle Dome hub fired this tick
    /// (`World::take_muscle_hub_sounds`): the first visit's announcer line
    /// through the XA path the battle clips use, and each tally lane's voice.
    /// The screen timers themselves are the world's (`World::tick_muscle_hub`,
    /// run by the scene host every tick).
    ///
    /// A tally lane names no cue id: `FUN_801D1288` resolves a whole
    /// `(voice, VAB id, program, tone, note, fine, vol_l, vol_r)` set, so the
    /// director's explicit key-on is the only path that takes it.
    ///
    /// The world's frame is drained with or without audio.
    // REF: FUN_801D1288, FUN_8003D53C
    pub fn sound_muscle_hub(&mut self) {
        let frame = self.host.world.take_muscle_hub_sounds();
        let cd_reverb = self.host.world.cd_input_reverb();
        let Some(bgm) = self.bgm.as_mut() else {
            return;
        };
        bgm.set_cd_reverb(cd_reverb);
        if let Some(c) = frame.xa {
            let fired = bgm
                .play_xa_clip(
                    u32::from(c.clip),
                    u32::from(c.channel),
                    u32::from(c.duration_sectors),
                )
                .is_some();
            log::debug!(
                "dome hub XA clip slot {} ch {} dur {} -> {}",
                c.clip,
                c.channel,
                c.duration_sectors,
                if fired { "playing" } else { "not playing" }
            );
        }
        for cue in frame.voice_cues {
            bgm.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                cue.voice,
                cue.vab_program_tone,
                cue.note_and_fine,
                cue.volume,
            ));
        }
    }

    /// Play the announcer line the Baka Fighter round chrome fired this tick
    /// (`FUN_8003D53C`), through the XA path the battle clips use. No-op
    /// outside the duel, and with no audio.
    // REF: FUN_8003D53C
    pub fn sound_baka_announcer(&mut self) {
        if self.host.world.mode != SceneMode::BakaFighter {
            return;
        }
        let Some(xa) = self
            .host
            .world
            .minigames
            .baka_fighter
            .as_ref()
            .and_then(|f| f.chrome_frame().xa)
        else {
            return;
        };
        let cd_reverb = self.host.world.cd_input_reverb();
        let Some(bgm) = self.bgm.as_mut() else {
            return;
        };
        bgm.set_cd_reverb(cd_reverb);
        let fired = bgm
            .play_xa_clip(u32::from(xa.clip), u32::from(xa.chan), u32::from(xa.dur))
            .is_some();
        log::debug!(
            "baka chrome: announcer XA clip {} chan {} ({}) -> {}",
            xa.clip,
            xa.chan,
            xa.dur,
            if fired { "playing" } else { "not playing" }
        );
    }

    /// One sim tick of the footstep / ambient cadence (`FUN_80018db0`,
    /// [`FootstepCadence`]): feed it this tick's movement magnitude and
    /// queue [`CUE_FOOTSTEP`] when its step gate opens. Returns whether the
    /// gate opened, cue or no cue, so a host can count it.
    ///
    /// The cadence runs in the field-run modes only; a suspended scene (menu,
    /// minigame, cutscene) is not walking, and retail's field audio update
    /// does not run there either. The walk clip stays running when a step is
    /// blocked by a wall - retail's walk-in-place - so either the clip or a
    /// displacement counts as moving.
    ///
    /// A host calls it once per sim tick ahead of [`Self::step_audio_frame`].
    ///
    /// [`FootstepCadence`]: legaia_engine_audio::footstep::FootstepCadence
    // REF: FUN_80018db0
    pub fn step_footstep_cadence(&mut self) -> bool {
        let world = &self.host.world;
        let mag = if matches!(world.mode, SceneMode::Field | SceneMode::WorldMap) {
            let pos = world
                .player_actor_slot
                .and_then(|s| world.actors.get(s as usize))
                .map(|a| (a.move_state.world_x as i32, a.move_state.world_z as i32))
                .unwrap_or((0, 0));
            let displaced = self
                .footstep_prev_pos
                .is_some_and(|(px, pz)| (pos.0 - px).abs().max((pos.1 - pz).abs()) >= WALK_EPSILON);
            self.footstep_prev_pos = Some(pos);
            let walking = world
                .locomotion
                .player_anim
                .as_ref()
                .is_some_and(|f| f.walking);
            if walking || displaced {
                WALK_SPEED_UNITS
            } else {
                0
            }
        } else {
            self.footstep_prev_pos = None;
            0
        };
        let fired = self.footstep.tick_cadence(mag, mag).step_fired;
        if fired
            && let Some(cue) = CUE_FOOTSTEP
            && let Some(bgm) = self.bgm.as_mut()
        {
            bgm.enqueue_sfx(u16::from(cue), 0, 0, 0);
        }
        fired
    }

    /// The **audio frame** of a field or battle tick: the duck ramp toward
    /// the world's configured level, the delay scheduler, and every cue that
    /// matured this tick (`AudioDirector::tick_audio_frame`).
    ///
    /// A host calls it once per sim tick, after every step of the tick that
    /// can queue a cue - the session tick's field routing, the minigame and
    /// Baka drains, the effect step, the menu session, the battle-event
    /// drain - and ahead of the field-event drain. A zero-delay cue therefore
    /// sounds on the tick that raised it on both hosts. The battle's CD-XA
    /// requests (clips, arts shouts) are issued ahead of it, in the
    /// battle-event step.
    ///
    /// Under a screen that freezes the field the frame tail does not run and
    /// the scheduler alone is stepped, by [`Self::step_overlay_sfx`].
    ///
    /// `None` with no director attached.
    pub fn step_audio_frame(&mut self) -> Option<SfxFrameReport> {
        let configured = self.host.world.audio.levels.configured_level;
        // A field script's duck (op `4C 4D`) re-targets the same ramp the
        // battle's summon / capture arms drive.
        let duck = self.host.world.take_field_audio_duck();
        let cd_reverb = self.host.world.cd_input_reverb();
        let bgm = self.bgm.as_mut()?;
        bgm.set_cd_reverb(cd_reverb);
        if let Some(pct) = duck {
            bgm.set_duck_pct(pct);
        }
        let report = bgm.tick_audio_frame(configured);
        for (id, voice) in &report.fired {
            log::debug!("SFX cue {id:#04x} fired on voice {voice}");
        }
        Some(report)
    }

    /// One sim tick of the SFX scheduler while a menu-overlay screen (the
    /// pause menu, a shop, the prize exchange) freezes the field. The field's
    /// frame tail - and the audio frame it ends on - does not run under one,
    /// and this is the scheduler step the frame still owes: a cue queued by
    /// the screen sounds, and one already delayed when the screen opened keeps
    /// ageing and fires on time.
    ///
    /// Retail runs those screens at game mode `0x17`, whose per-frame handler
    /// still calls the cue drainer every frame. The duck ramp is not stepped:
    /// it belongs to the audio frame the field tail runs.
    ///
    /// `None` with no director attached.
    // REF: FUN_80025F74 (`jal FUN_80016B6C` at `0x80025F9C`)
    pub fn step_overlay_sfx(&mut self) -> Option<SfxFrameReport> {
        let report = self.bgm.as_mut()?.tick_sfx_frame();
        for (id, voice) in &report.fired {
            log::debug!("menu SFX cue {id:#04x} fired on voice {voice}");
        }
        Some(report)
    }
}
