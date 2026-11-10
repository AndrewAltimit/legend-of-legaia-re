//! The dance minigame session: enter, tick, the ring cue, award sounds, the
//! tutorial step and exit.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// The hall's ambient tree: the dance's setup state spawns records `2` and
    /// `1` of the block's stager bundle at the origin (`FUN_800252EC` at
    /// `0x801CF9A8` and `0x801CF9C0`, state `1` of `FUN_801CF470`). They are
    /// the floor and wall palette cyclers and the strip scroller, which repaint
    /// the hall's VRAM for as long as the dance runs. The walked-in scene's
    /// own seat is kept aside for [`Self::restore_dance_ambient`]: retail
    /// replaces it, the port keeps that scene underneath.
    ///
    /// PORT: FUN_801cf470 (state `1`'s two stager installs)
    pub fn stage_dance_ambient(&mut self, stager_bytes: &[u8]) {
        if self.ambient.venue_restore.is_some() {
            return;
        }
        let restore = crate::world::VenueAmbientRestore {
            ambient: std::mem::take(&mut self.ambient),
            stagers: std::mem::take(&mut self.props.stagers),
            stager_bytes: std::mem::take(&mut self.props.stager_bytes),
            frame_step: self.clock.frame_step,
        };
        // A capture seat aims at the frame the comparison scores, which the
        // hall's seat draws.
        let mut ambient = restore.ambient;
        self.ambient.cell_fx_seed = std::mem::take(&mut ambient.cell_fx_seed);
        self.ambient.vram_rect_seed = std::mem::take(&mut ambient.vram_rect_seed);
        self.ambient.cell_source_seed = std::mem::take(&mut ambient.cell_source_seed);
        let restore = crate::world::VenueAmbientRestore { ambient, ..restore };
        self.ambient.venue_restore = Some(Box::new(restore));
        // The hall runs at a game tick per three vsyncs: `DAT_1F800393 = 3`
        // in the parked dance states, the step the count-in's banner reads too
        // ([`legaia_engine_minigames::dance::COUNTIN_ANIM_PERIOD_VSYNCS`]).
        self.clock.frame_step = legaia_engine_minigames::dance::COUNTIN_ANIM_PERIOD_VSYNCS;
        self.install_field_stagers(stager_bytes);
        self.spawn_ambient_record(2, [0, 0, 0]);
        self.spawn_ambient_record(1, [0, 0, 0]);
        // State `1` itself is a game tick ahead of the first READY run
        // (state `3`): the parts' gate reads `25` at banner counter `72`.
        self.ambient.pending_game_ticks = 1;
    }

    /// Carry the walked-in scene's painted palette cells into the hall's VRAM,
    /// ahead of [`Self::stage_dance_ambient`].
    ///
    /// The hall's init loads its art pack only with the field-entry word
    /// `_DAT_8007B8B8` clear (`lw` at `0x801CF190`, `jal 0x8002541C` at
    /// `0x801CF1A0`), so a door entry stands on the VRAM the field left. The
    /// hall's cyclers capture their cells from there (op `0x2C`), and on `koin3`
    /// those cells hold the strobe the field's own cyclers last painted: every
    /// later hue and V of the hall's light show is a rotation of that paint.
    /// `minigame_dance_retail_go_in` holds `(4, 4, 13)` as the floor's captured
    /// entry where the disc's palette has `(20, 15, 6)`.
    ///
    /// PORT: FUN_801cef54 (the art pack's door-entry skip)
    pub fn carry_walked_in_cells(&self, walked_in: &psx_tim::Vram, hall: &mut psx_tim::Vram) {
        if self.ambient.venue_restore.is_some() {
            return;
        }
        // The drop shadow's blob and palette are boot-resident
        // (`FUN_8001C394`'s `(960, 256)` page, CLUT `0x7F86`), which the venue
        // build does not hold.
        const SHADOW_BLOB: (u16, u16, u16, u16) = (1016, 256, 4, 16);
        const SHADOW_CLUT: (u16, u16, u16, u16) = (96, 510, 16, 1);
        let rects = self.ambient.cell_captures.keys().copied();
        for (x, y, w, h) in rects.chain([SHADOW_BLOB, SHADOW_CLUT]) {
            let painted = crate::world::ambient::read_rect(walked_in, x, y, w, h);
            crate::world::ambient::write_rect(hall, x, y, w, &painted);
        }
    }

    /// Put the walked-in scene's ambient seat back when the hall comes down.
    pub fn restore_dance_ambient(&mut self) {
        if let Some(r) = self.ambient.venue_restore.take() {
            self.ambient = r.ambient;
            self.props.stagers = r.stagers;
            self.props.stager_bytes = r.stager_bytes;
            self.clock.frame_step = r.frame_step;
        }
    }

    /// Step the hall's tree against the hall's own VRAM; `true` when it wrote.
    pub fn step_dance_ambient(&mut self, vram: &mut psx_tim::Vram) -> bool {
        if self.ambient.venue_restore.is_none() {
            return false;
        }
        // Ahead of the tree's first pass, whose op `0x2C`s capture from here.
        for ((x, y, w, _), texels) in std::mem::take(&mut self.ambient.cell_source_seed) {
            crate::world::ambient::write_rect(vram, x, y, w, &texels);
        }
        let moved = self.apply_script_vram_moves(vram);
        let ambient = self.step_ambient_fx(vram);
        let seeded = self.apply_vram_rect_seed(vram);
        moved | ambient | seeded
    }

    /// Enter the Noa dance (rhythm) minigame on `game`, suspending the current
    /// scene mode. The suspended mode is restored by [`World::exit_dance`] (and
    /// automatically once the song ends). Mirrors the pause-menu suspend/restore
    /// contract: the interrupted field/battle state stays intact underneath.
    ///
    /// Applies the dance stager's pad-latch clear
    /// ([`legaia_engine_minigames::dance::dance_scene_stage`]): retail zeroes `_DAT_8007B880` on
    /// the frame the hall is staged, so the confirm press that starts the
    /// minigame is not also read as its first judged note.
    pub fn enter_dance(&mut self, game: legaia_engine_minigames::dance::DanceGame) {
        // Don't stack a suspend: if the dance is already running, just swap the
        // game so a re-entry keeps the true return mode.
        if self.mode != SceneMode::Dance {
            self.minigames.dance_return_mode = self.mode;
        }
        // The pre-song count-in, and the how-to run's tutorial actor, are
        // staged here rather than in a host: retail runs `FUN_801cf470`'s
        // below-10 states before the beat clock, and owning that phase in the
        // world is what gives the **door-warp** entry a count-in on both
        // hosts. Previously only the native debug launcher ran one, from a
        // driver of its own.
        let how_to = game.mode() == legaia_engine_minigames::dance::DanceMode::HowTo;
        // State 1's flag writes, after it has read the mode off them: the
        // three one-shot mode requests are cleared (`0x801CF950..0x801CF964`;
        // the free-play flag stays) and the pass flag is raised for the
        // results state to clear on a loss (`0x801CF968`).
        // PORT: FUN_801cf470 (state 1's flag writes)
        for flag in [
            legaia_engine_minigames::dance::MODE_FLAG_FINALS,
            legaia_engine_minigames::dance::MODE_FLAG_QUALIFIER,
            legaia_engine_minigames::dance::MODE_FLAG_HOW_TO,
        ] {
            self.system_flag_clear(flag);
        }
        self.system_flag_set(legaia_engine_minigames::dance::WIN_FLAG);
        // The run raises and draws the saved best `_DAT_80084464` in place;
        // it starts from the save's.
        let mut game = game;
        let mode = game.mode();
        game.set_best_score(self.minigames.dance_best);
        self.minigames.dance = Some(game);
        self.minigames.dance_last_judge = None;
        self.minigames.dance_countin = Some(legaia_engine_minigames::dance::CountIn::new());
        self.minigames.dance_countin_banner = None;
        self.minigames.dance_countin_go = None;
        // State 3 streams the song the mode names, and free play one of
        // three by the save's counter, which it then bumps
        // (`0x801CFAC0..0x801CFBA0`). Held until the count-in clears, which
        // is when retail's beat clock starts.
        let counter = self.minigames.dance_free_play_song;
        let loader = legaia_engine_minigames::dance::song_loader_index(mode, counter);
        if mode == legaia_engine_minigames::dance::DanceMode::FreePlay {
            self.minigames.dance_free_play_song =
                legaia_engine_minigames::dance::next_free_play_song(counter);
        }
        self.minigames.dance_pending_bgm =
            legaia_engine_system::music_labels::sound_test_index_for_prot_entry(
                u32::from(loader) - 2,
            )
            .map(|i| 2000 + i as u16);
        self.minigames.dance_tutorial =
            how_to.then(legaia_engine_minigames::dance_tutorial::DanceTutorial::new);
        self.minigames.dance_tutorial_frame = None;
        self.mode = SceneMode::Dance;
        if legaia_engine_minigames::dance::dance_scene_stage().clear_pad_latch {
            self.input.clear_edges();
        }
    }

    /// The dance's **auto-end**: once the song-over arm of
    /// [`tick_dance`](Self::tick_dance) has restored the interrupted mode, tear
    /// the run down ([`Self::exit_dance`]) and hand back its final game for the
    /// host to log. `None` while the hall still owns the frame, or with no run.
    /// The per-frame poll both play hosts make; each used to spell the
    /// `mode != Dance` test beside its own `exit_dance` call.
    pub fn finish_dance_if_over(&mut self) -> Option<legaia_engine_minigames::dance::DanceGame> {
        if self.mode == SceneMode::Dance {
            return None;
        }
        self.exit_dance()
    }

    /// Clear the dance minigame and return the final [`DanceGame`] so the host
    /// can read the score / pass result. Restores the interrupted mode if it is
    /// still `Dance` (a mid-song abort); when the song already auto-ended
    /// [`tick_dance`](Self::tick_dance) has restored the mode but left the game
    /// installed for one frame so the host can read it - this take clears it.
    ///
    /// **Nothing happens without a run to tear down.** Both hosts poll this
    /// from their frame path with the same `mode != Dance` test, which is
    /// true on every ordinary field frame, so an unguarded body ran the whole
    /// teardown - `restore_minigame_bgm` (self-gating, harmless) and the
    /// stager's PAD-LATCH CLEAR (not harmless) - sixty times a second. The
    /// clear forces `pad_prev = pad`, so every edge consumer that runs after
    /// the poll in its host's frame order saw no edges at all: on the browser
    /// play page, where the poll sits in `tick_minigame_ui` ahead of
    /// `tick_dev_menu`, the developer menu stopped taking input entirely.
    /// `enter_dance` is the only writer of `minigames.dance`, so "a run is
    /// installed, or the mode is still the hall" is exactly the set of frames
    /// with something to tear down.
    pub fn exit_dance(&mut self) -> Option<legaia_engine_minigames::dance::DanceGame> {
        self.restore_dance_ambient();
        if self.mode != SceneMode::Dance && self.minigames.dance.is_none() {
            return None;
        }
        if self.mode == SceneMode::Dance {
            self.mode = self.minigames.dance_return_mode;
        }
        self.minigames.dance_last_judge = None;
        self.minigames.dance_countin = None;
        self.minigames.dance_countin_banner = None;
        self.minigames.dance_countin_go = None;
        self.minigames.dance_pending_bgm = None;
        self.minigames.dance_tutorial = None;
        self.minigames.dance_tutorial_frame = None;
        // Give the hall its own music back when the chart loop displaced it.
        self.restore_minigame_bgm();
        // The stager runs on teardown as well as on entry, so the press that
        // leaves the hall does not carry into the restored field mode.
        if legaia_engine_minigames::dance::dance_scene_stage().clear_pad_latch {
            self.input.clear_edges();
        }
        self.minigames.dance.take()
    }

    /// Advance the dance minigame one frame: step the beat clock, judge this
    /// frame's directional presses, and end the run when the song finishes.
    ///
    /// The judged buttons are the retail ones: `FUN_801d1af4` reads the
    /// newly-pressed word `_DAT_8007B874` and tests the three face bits
    /// `0x80` / `0x20` / `0x10` = Square / Circle / Triangle, which is exactly
    /// [`legaia_engine_minigames::dance::DanceDir::pad_bit`]. This frame's pad edges are packed
    /// into that layout and the direction is picked by matching `pad_bit`, so
    /// the bit-to-direction binding lives in the ported kernel. Edge-triggered
    /// (`just_pressed`) so a held button scores at most one note per press.
    ///
    /// PORT: the dance overlay's per-frame driver (`FUN_801cf470` beat clock ->
    /// `FUN_801d1960` hit judge), one advance + one judged press pass per frame.
    pub(in crate::world::frame_tick) fn tick_dance(&mut self) {
        if self.minigames.dance.is_none() {
            // Mode is Dance but no game installed - drop back to a sane mode.
            self.mode = self.minigames.dance_return_mode;
            return;
        }
        // Every body's clip runs every frame, count-in included: the clip
        // driver ticks each actor whatever state the dance is in. The song's
        // own advance steps the marker tiles; through the count-in, this does:
        // they are actors on the same lists.
        if let Some(g) = self.minigames.dance.as_mut() {
            g.advance_body_clips(1);
            if self.minigames.dance_countin.is_some() {
                g.advance_step_markers(1);
            }
        }
        // The pre-song count-in owns the frame while it runs: the beat clock
        // does not advance and no press is judged, which is retail's
        // below-10 state band. The song starts on the frame it clears.
        if let Some(mut ci) = self.minigames.dance_countin.take() {
            let step = ci.step();
            self.minigames.dance_countin_banner = step.banner;
            self.minigames.dance_countin_go = step.go;
            if let Some(cue) = step.cue {
                self.write_dance_ring_cue(legaia_engine_minigames::dance::STAGE_CUE_RING_SLOT, cue);
            }
            if step.done {
                self.minigames.dance_countin_banner = None;
                self.minigames.dance_countin_go = None;
                if let Some(bgm) = self.minigames.dance_pending_bgm.take() {
                    self.swap_to_minigame_bgm(bgm);
                }
            } else {
                self.minigames.dance_countin = Some(ci);
            }
            self.step_dance_tutorial();
            return;
        }
        let Some(game) = self.minigames.dance.as_mut() else {
            return;
        };
        // Judge at most one directional press this frame (retail tests all
        // three bits in one pass, but a rhythm player presses one at a time);
        // the scan order is retail's - Triangle, then Square, then Circle.
        //
        // The engine's `PadButton` word and the packed word `FUN_8001822C`
        // builds hold the same 16 buttons with the two bytes swapped (see
        // `legaia_engine_system::retail_pad`: face/shoulder cluster low, dpad/system high),
        // so one rotate turns this frame's edges into the mask the retail
        // judge reads.
        use legaia_engine_minigames::dance::DanceDir;
        let pressed = (self.input.pad() & !self.input.pad_prev()).rotate_right(8);
        let dir = [DanceDir::C, DanceDir::A, DanceDir::B]
            .into_iter()
            .find(|d| pressed & d.pad_bit() != 0);
        if let Some(event) = game.advance_vsync(dir) {
            self.minigames.dance_last_judge = Some(event.judge());
        }
        let finished = game.finished();
        if finished {
            // Retail's head runs again over the results state, so the last
            // frame's score is raised before the hall comes down.
            game.raise_best_score();
        }
        self.minigames.dance_best = game.best_score();
        let finish_cues = game.take_finish_cues();
        let award_sounds = game.take_award_sounds();
        let clear_win = finished && game.results_clear_win_flag();
        let counter_slots = finished.then(|| game.results_counter_slots());
        // The countdown parts' move-VM op `0x1D` stores straight into ring
        // slot 3 (`DAT_8007B6DE`), as the award does.
        for cue in finish_cues {
            self.write_dance_ring_cue(legaia_engine_minigames::dance::AWARD_CUE_RING_SLOT, cue);
        }
        self.route_dance_award_sounds(&award_sounds);
        if finished {
            // Song finished: the results state grades the run into the pass
            // flag, then the interrupted mode is restored, leaving `dance` in
            // place so the host can read the final score before clearing.
            self.mode = self.minigames.dance_return_mode;
            // ... and copies the floor's scores into the script-counter
            // slots the host town's dialogue reads back.
            if let Some(slots) = counter_slots {
                self.field_vm.slot_table[..3].copy_from_slice(&slots);
            }
            if clear_win {
                self.system_flag_clear(legaia_engine_minigames::dance::WIN_FLAG);
            }
        }
        self.step_dance_tutorial();
    }

    /// Store one dance cue straight into a ring slot, the way every dance
    /// cue site does (`sh id, DAT_8007B6D8[slot]`, no cursor pair). Ids
    /// `>= 0x200` resolve through the dance's own descriptor bank
    /// ([`Self::runtime_sfx_bundle`]); the tutorial's `0x20` / `0x21` through
    /// the static table.
    pub(in crate::world::frame_tick) fn write_dance_ring_cue(&mut self, slot: u8, cue: u16) {
        self.audio
            .sfx_ring_ops
            .push(legaia_engine_system::sound_state::SfxRingOp::WriteSlot(
                slot, cue as i16,
            ));
    }

    /// Hand the human's award sounds to the audio side: cues into ring slot
    /// 3, stings as two directly keyed voices (`FUN_801d3d78` ->
    /// `FUN_80065034`) on the queue both hosts drain. The tier-2 sting's
    /// variant is `rand() % 3` off the one world stream, as retail's
    /// `jal 0x80056798` at `0x801D2138`.
    pub(crate) fn route_dance_award_sounds(
        &mut self,
        sounds: &[legaia_engine_minigames::dance::DanceAwardSound],
    ) {
        use legaia_engine_minigames::dance::DanceAwardSound;
        for &s in sounds {
            match s {
                DanceAwardSound::Cue(cue) => self
                    .write_dance_ring_cue(legaia_engine_minigames::dance::AWARD_CUE_RING_SLOT, cue),
                DanceAwardSound::Sting { r, random } => {
                    let r = if random {
                        (self.next_rand()
                            % u32::from(legaia_engine_minigames::dance::STING_RANDOM_VARIANTS))
                            as u16
                    } else {
                        r
                    };
                    self.audio.sfx_voice_keys.extend(
                        legaia_engine_minigames::dance::dance_hit_sting_cues(
                            r,
                            self.audio.levels.voice_volume as u32,
                        ),
                    );
                }
            }
        }
    }

    /// Run the Disco King how-to tutorial actor for one frame beside the live
    /// session, on the retail pad-word layout (the same rotate the judge
    /// applies). No-op unless the installed run is a
    /// [`legaia_engine_minigames::dance::DanceMode::HowTo`] one.
    ///
    /// The handler runs during the count-in too - retail's actor ticks
    /// independently of the dance states, and its opening prompt is what the
    /// player answers before the song.
    // REF: FUN_801D0750 (the actor handler; the per-state kernels are
    //      `legaia_engine_minigames::dance_tutorial`)
    pub(in crate::world::frame_tick) fn step_dance_tutorial(&mut self) {
        if self.minigames.dance_tutorial.is_none() {
            self.minigames.dance_tutorial_frame = None;
            return;
        }
        let (score, feedback_frames, combo_hit) = self
            .minigames
            .dance
            .as_ref()
            .map(|g| {
                (
                    g.score() as i32,
                    g.feedback_frames() as i32,
                    matches!(g.triangle_feedback(), Some(true)),
                )
            })
            .unwrap_or((0, 0, false));
        let pressed = (self.input.pad() & !self.input.pad_prev()).rotate_right(8);
        let Some(tut) = self.minigames.dance_tutorial.as_mut() else {
            return;
        };
        let frame = tut.step(pressed, score, feedback_frames, combo_hit, 1);
        if let Some(cue) = frame.cue {
            // `FUN_801d0750` stores its cursor / confirm blips straight into
            // ring slot 0 (`sh id, 0x8007B6D8`).
            self.write_dance_ring_cue(legaia_engine_minigames::dance::STAGE_CUE_RING_SLOT, cue);
        }
        if frame.done {
            self.minigames.dance_tutorial = None;
            self.minigames.dance_tutorial_frame = None;
        } else {
            self.minigames.dance_tutorial_frame = Some(frame);
        }
    }
}
