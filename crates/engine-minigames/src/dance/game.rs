//! The dance floor (`DanceGame`): beat clock, chart feed, hit judge and scoring.
//! Split out of `dance.rs`.

use super::*;

/// The dance floor: the beat clock, the chart, and the three dancers' runs.
#[derive(Debug, Clone)]
pub struct DanceGame {
    pub(super) chart: DanceChart,
    pub(super) tables: DanceScoreTables,
    /// Beat phase counter (`DAT_801d581c`); wraps at [`BEAT_PHASE_WRAP`].
    pub(super) phase: u32,
    /// Total-song timer (`DAT_801d5820`).
    pub(super) song_timer: u32,
    /// Song-length limit this run ends at.
    pub(super) song_len: u32,
    /// The floor, slot 0 = the human.
    pub(super) dancers: Vec<Dancer>,
    /// Triangle feedback window (`DAT_801d5144`), armed on the human's spend.
    pub(super) feedback: u32,
    /// The mode global (`DAT_801d514c`), which the HUD driver's score-box
    /// permutation and its solo arm both key off.
    pub(super) mode: DanceMode,
    /// The overlay's HUD widget table with each record's `+0x13` ABR byte
    /// alongside it, when the run was started from a real overlay image.
    pub(super) widgets: Vec<(legaia_asset::dance_art::DanceWidget, u8)>,
    /// The five kind descriptors, when the run was started from a real overlay
    /// image. This is what supplies the clip ids the dancer actors bind into
    /// their `+0x5C`; without it a dancer actor carries no clip and the clip
    /// driver gate reports `false` for it (which is what retail would do too -
    /// an actor with nothing bound is not handed to `FUN_800204F8`).
    pub(super) kinds: Vec<legaia_asset::dance_cast::DanceKind>,
    /// The dancer actor pool - one [`crate::minigame_actor::MinigameActor`]
    /// per floor slot, rebuilt from the dancers every [`DanceGame::advance`].
    pub(super) actors: crate::minigame_actor::MinigameActorPool,
    /// The **sprite-part** pool: what `FUN_801d3fd0` spawns and
    /// [`sprite_part_emit`] draws. A separate pool from the dancers because it
    /// is a separate actor family - the dancer is a 3D body the clip driver
    /// animates, the part is a 2D sprite in the `<< 3` screen space.
    pub(super) parts: crate::minigame_actor::MinigameActorPool,
    /// The overlay's step-marker flipbook script (`0x801D44CC`), when the run
    /// was started from a real overlay image.
    pub(super) marker_script: legaia_engine_vm::dance_marker::MarkerScript,
    /// One flipbook cursor per marker class (clip `6..=9`). Retail runs one
    /// actor per drawn marker **cell**; every cell of a class reads the same
    /// row from the same cursor, so the engine keeps the four cursors rather
    /// than one per cell and a marker draw asks its class.
    pub(super) markers: [legaia_engine_vm::dance_marker::MarkerActor;
        legaia_engine_vm::dance_marker::MARKER_SCRIPT_ROWS],
    /// The tick's camera keyframe track (`FUN_801CF470` at
    /// `0x801CF51C..0x801CF7D8`), when the run was started from a real
    /// overlay image.
    pub(super) camera: Option<super::DanceCameraTrack>,
    /// The how-to mode's Disco King ([`DemoDancer`]), spawned beside the
    /// floor by `FUN_801d0190`'s mode-2 tail.
    pub(super) demo: Option<DemoDancer>,
    /// How long each descriptor clip plays before the clip driver raises its
    /// end flag, keyed by `(anim id, rate)`, once the dance hall's
    /// choreography bank is attached ([`DanceGame::attach_clip_bank`]).
    /// `None` on a chart-only run, which falls back to the note latch.
    pub(super) clip_ticks: Option<std::collections::HashMap<(u16, u16), u32>>,
    /// The song-end countdown's four move programs, off the overlay
    /// ([`super::finish_programs`]); empty on a chart-only run.
    pub(super) finish_programs: Vec<(u16, Vec<u16>)>,
    /// States `0xB` / `0xC` while they run ([`super::FinishCountdown`]).
    pub(super) finish: Option<super::FinishCountdown>,
    /// State `0x14` reached: the countdown and the wipe are over.
    pub(super) finished: bool,
    /// Cues the countdown wrote, for the host to drain.
    pub(super) finish_cues: Vec<u16>,
}

impl DanceGame {
    /// Start a run on `chart` with no disc scoring tables (sequences award no
    /// points and the CPU dancers never spend a triangle). Prefer
    /// [`DanceGame::from_overlay`], which reads the real tables + cast.
    pub fn new(chart: DanceChart, long_song: bool) -> Self {
        Self::with_tables(
            chart,
            DanceScoreTables::default(),
            &QUALIFIER_KINDS,
            long_song,
        )
    }

    /// Start a run on `chart` + the overlay's scoring `tables`, with the floor
    /// cast given as dancer kinds (slot 0 = the human).
    pub fn with_tables(
        chart: DanceChart,
        tables: DanceScoreTables,
        kinds: &[usize],
        long_song: bool,
    ) -> Self {
        let mut game = Self {
            chart,
            tables,
            phase: 0,
            song_timer: 0,
            song_len: if long_song {
                SONG_LEN_LONG
            } else {
                SONG_LEN_SHORT
            },
            dancers: kinds.iter().map(|&k| Dancer::new(k)).collect(),
            feedback: 0,
            mode: DanceMode::Qualifier,
            widgets: Vec::new(),
            kinds: Vec::new(),
            actors: crate::minigame_actor::MinigameActorPool::new(),
            parts: crate::minigame_actor::MinigameActorPool::new(),
            marker_script: Default::default(),
            markers: Default::default(),
            camera: None,
            demo: None,
            clip_ticks: None,
            finish_programs: Vec::new(),
            finish: None,
            finished: false,
            finish_cues: Vec::new(),
        };
        // A chart-only run still spawns its floor - the actors just stand at
        // the origin and bind no clip, because both of those come off the
        // overlay's spawn + kind tables.
        let spawns: Vec<(usize, [i16; 3])> = kinds.iter().map(|&k| (k, [0i16; 3])).collect();
        game.spawn_dancer_actors(&spawns);
        game
    }

    /// Parse the baked step chart + scoring tables + qualifier cast out of the
    /// dance overlay image (PROT 0980) and start a run. `None` when the chart
    /// doesn't decode (see [`legaia_asset::dance_chart::parse`]).
    ///
    /// Starts the **qualifier** floor; [`DanceGame::from_overlay_for_mode`] is
    /// the per-mode entry point.
    pub fn from_overlay(overlay: &[u8], long_song: bool) -> Option<Self> {
        Self::from_overlay_for_mode(overlay, DanceMode::Qualifier, long_song)
    }

    /// Start a run on `mode`'s floor.
    ///
    /// The mode picks the cast **and its size**, which is the part that is easy
    /// to miss: the three spawn tables are not three arrangements of one roster.
    /// Free play puts **six** dancers on the floor and the how-to demo puts a
    /// single one, so a host that always spawns the qualifier's three is wrong
    /// in two of the four modes.
    ///
    /// `long_song` stays a parameter because the caller owns the song choice;
    /// the how-to demo is the one mode whose length retail fixes, and it forces
    /// [`SONG_LEN_SHORT`] regardless.
    // PORT: FUN_801d0190 (per-mode spawn-table + cast-size selection)
    pub fn from_overlay_for_mode(overlay: &[u8], mode: DanceMode, long_song: bool) -> Option<Self> {
        let chart = legaia_asset::dance_chart::parse(overlay)?;
        let tables = legaia_asset::dance_chart::parse_tables(overlay).unwrap_or_default();
        let cast = legaia_asset::dance_cast::parse(overlay);
        // The spawn record carries the dancer's kind **and** its floor
        // position; the position is what `FUN_801d0190` stores into the
        // spawned actor's `+0x14` / `+0x16` / `+0x18`, so it is kept here
        // rather than dropped with the rest of the record.
        let spawns: Vec<(usize, [i16; 3])> = cast
            .as_ref()
            .map(|c| {
                let table = match mode {
                    // The how-to demo reads the qualifier table but spawns only
                    // its first record - one dancer, not three.
                    DanceMode::Qualifier | DanceMode::HowTo => &c.qualifier,
                    DanceMode::Finals => &c.finals,
                    DanceMode::FreePlay => &c.free_play,
                };
                table
                    .iter()
                    .take(mode.cast_size())
                    .map(|s| (s.kind as usize, [s.x, s.y, s.z]))
                    .collect()
            })
            .filter(|k: &Vec<(usize, [i16; 3])>| !k.is_empty())
            .unwrap_or_else(|| {
                QUALIFIER_KINDS[..mode.cast_size().min(DANCER_SLOTS)]
                    .iter()
                    .map(|&k| (k, [0i16; 3]))
                    .collect()
            });
        let kinds: Vec<usize> = spawns.iter().map(|&(k, _)| k).collect();
        let long = long_song && mode != DanceMode::HowTo;
        let mut game = Self::with_tables(chart, tables, &kinds, long);
        game.mode = mode;
        game.widgets = dance_widgets_with_abr(overlay);
        game.kinds = cast.map(|c| c.kinds).unwrap_or_default();
        game.marker_script = legaia_engine_vm::dance_marker::MarkerScript::from_overlay(
            overlay,
            legaia_asset::dance_chart::DANCE_OVERLAY_BASE_VA,
        )
        .unwrap_or_default();
        for (class, m) in game.markers.iter_mut().enumerate() {
            m.class = class as u16;
        }
        game.camera = super::DanceCameraTrack::from_overlay(overlay);
        game.finish_programs = super::finish_programs(overlay);
        game.spawn_dancer_actors(&spawns);
        // PORT: FUN_801d0190 (the mode-2 Disco King spawn, 0x801D0338..0x801D0390)
        if mode == DanceMode::HowTo {
            game.demo = Some(DemoDancer::default());
        }
        Some(game)
    }

    /// Attach the dance hall's choreography bank (the `other7` scene's MOVE
    /// ANM bundle, `legaia_engine_core::dance_venue::dance_clip_bank`): every descriptor
    /// clip's length in ticks - [`legaia_engine_vm::field_player_clip::clip_end_ticks`] on the
    /// record's frame count and the rate's per-record step - so a judge move
    /// holds the dancer for exactly as long as retail's clip driver plays it.
    ///
    /// `FUN_801d1358` rebinds a dancer's standing loop only when the clip
    /// driver raises the bound clip's end flag (`+0x62 & 0x100`, tested at
    /// `0x801D14C8`), and calls the award routine `FUN_801d1af4` only while
    /// the bound clip **is** the idle or dance loop (`0x801D168C..0x801D16B4`).
    /// So a move - a miss reaction included - runs to its last frame, and no
    /// press of that dancer is judged until it has.
    pub fn attach_clip_bank(&mut self, bank: &legaia_asset::player_anm::PlayerAnmBundle) {
        let mut map = std::collections::HashMap::new();
        for k in &self.kinds {
            for c in [&k.idle, &k.dance].into_iter().chain(k.moves.iter()) {
                let id = c.anim_id & 0x1FF;
                let Some(rec) = id
                    .checked_sub(1)
                    .and_then(|r| bank.record_lenient(usize::from(r)).ok())
                else {
                    continue;
                };
                let step = legaia_engine_vm::field_player_clip::clip_step(
                    c.rate,
                    rec.blends(),
                    (rec.flag & 0xFF) as u8,
                );
                map.insert(
                    (id, c.rate),
                    legaia_engine_vm::field_player_clip::clip_end_ticks(rec.frame_count, step)
                        .max(1),
                );
            }
        }
        self.clip_ticks = Some(map);
    }

    /// Ticks `clip` plays before its end flag, when the bank is attached.
    pub fn clip_len(&self, clip: &legaia_asset::dance_cast::DanceClip) -> Option<u32> {
        self.clip_ticks
            .as_ref()?
            .get(&(clip.anim_id & 0x1FF, clip.rate))
            .copied()
    }

    /// Seed the dancer actor pool from the mode's spawn table, mirroring
    /// `FUN_801d0190`'s per-record stores: position into `+0x14`/`+0x16`/`+0x18`,
    /// the floor slot into `+0x5A`, the kind descriptor's idle clip (masked to
    /// `0x1FF`) into `+0x5C` and its rate word into `+0x6A`.
    // PORT: FUN_801d0190 (the per-dancer actor spawn stores)
    pub(super) fn spawn_dancer_actors(&mut self, spawns: &[(usize, [i16; 3])]) {
        self.actors.clear();
        for (slot, &(kind, home)) in spawns.iter().enumerate() {
            if let Some(d) = self.dancers.get_mut(slot) {
                d.home = home;
                if let Some(k) = self.kinds.get(kind) {
                    d.bind_clip(&k.idle);
                }
            }
            let mut a = crate::minigame_actor::MinigameActor::at(home, 0);
            a.live_mask = slot as u16;
            self.actors.push(a);
        }
        self.parts.clear();
        self.sync_dancer_actors();
    }

    /// Advance the four step-marker flipbooks one frame.
    ///
    /// Retail runs one `0x801D0640` actor per **drawn marker cell** on the
    /// floor, all of a class reading the same script row; the engine keeps
    /// one cursor per class because every cell of a class is on the same
    /// step at the same time - the marker's class is its only per-actor
    /// state (`+0x50`) and the pass that spawns it stamps nothing else the
    /// tick reads.
    ///
    /// The clip-selector gate (`+0x5C > 0` or `+0x10 & 0x1000`) is reported
    /// by the kernel and left alone here: no host draws the floor's marker
    /// meshes yet, so there is no clip player to hand the actor to.
    ///
    /// REF: FUN_801D0640 (kernel `legaia_engine_vm::dance_marker::step_marker`),
    /// FUN_801D2A10 (the floor pass that spawns the actors)
    pub(super) fn advance_step_markers(&mut self, frame_delta: u32) {
        let delta = frame_delta.min(u32::from(u8::MAX)) as u8;
        let bias = legaia_asset::field_objects::FIELD_ACTOR_PACK_BIAS as i16;
        for m in self.markers.iter_mut() {
            legaia_engine_vm::dance_marker::step_marker(m, &self.marker_script, delta, bias, 0, 0);
        }
    }

    /// The scene-pool mesh index marker class `class` (clip `6 + class`) is
    /// showing this frame, once its flipbook has run once.
    ///
    /// This is what a floor renderer draws: the value is already biased by
    /// the field actor pack base (`_DAT_8007B6F8`), so it indexes the scene
    /// mesh pool directly.
    pub fn step_marker_mesh(&self, class: usize) -> Option<i16> {
        self.markers.get(class).and_then(|m| m.mesh)
    }

    /// How many flipbook steps marker class `class` carries, `0` for a run
    /// started without an overlay image.
    pub fn step_marker_steps(&self, class: usize) -> usize {
        self.marker_script.steps(class)
    }

    /// Spawn one sprite part, the shape `FUN_801d3fd0` builds: the spec's
    /// already-shifted screen pair into `+0x14`/`+0x16`, the sprite id into
    /// `+0x50`, the fixed `0x1000` scale.
    ///
    /// Both [`step_mark_effect_spawn`] and [`good_banner_spawn`] produce these
    /// specs, and the run spawns the banner set itself on a scoring judge, so
    /// the pool fills from gameplay rather than from a host.
    // PORT: FUN_801d3fd0 (the spawn's actor-field stores)
    pub fn spawn_sprite_part(&mut self, spec: &crate::baka_fighter::EffectSpawnSpec) -> usize {
        let mut a = crate::minigame_actor::MinigameActor::at([spec.x, spec.y, 0], PART_DRAW_MODE);
        a.sprite = spec.sprite_id;
        a.scale = spec.scale;
        // A fresh part starts at the top of the fade ramp - see
        // [`DanceGame::advance`]'s aging pass for why the port drives `+0x78`
        // downward.
        a.beat = crate::minigame_actor::BEAT_FADE_CEILING;
        self.parts.push(a)
    }

    /// Rebuild each dancer actor's live fields off its dancer's state. Runs
    /// once per [`DanceGame::advance`], which is what keeps the record a
    /// production datum rather than a test fixture.
    pub(super) fn sync_dancer_actors(&mut self) {
        for (slot, d) in self.dancers.iter().enumerate() {
            let Some(a) = self.actors.get_mut(slot) else {
                continue;
            };
            a.pos = d.home;
            // `+0x26` is the yaw the groovy-move spin drives (retail wraps it
            // at 0x1000 per turn; `spin_acc` is that same accumulator).
            a.yaw = (d.spin_acc % SPIN_TURN_UNITS) as i16;
            a.field_5c = d.clip;
            a.cursor = d.clip_rate as i16;
            a.flags = d.flags;
            // The `0x1000` arm of the clip gate: retail raises it for an actor
            // whose clip must keep running even with nothing bound, which for
            // the dance floor is the groovy-move spin.
            a.set_drives_clip(d.spin_turns > 0);
        }
    }

    /// Age the sprite parts one frame and retire the faded ones.
    ///
    /// `+0x78` is the slot [`sprite_part_fade_weight`] reads, and its *writer*
    /// is not in the dump corpus - no caller of `FUN_801d387c` exists there
    /// either, because the address sits as an actor-prototype callback word.
    /// The port therefore drives it **down** the prologue's own ramp: a part
    /// spawns at [`crate::minigame_actor::BEAT_FADE_CEILING`] (weight `0xFF`
    /// after the prologue's `>> 4` and clamp) and decays to zero, so it fades
    /// out over its life the way every other banner in the port does. That is
    /// a port decision, disclosed, not a reading of a store - and it is the
    /// choice, not the ramp, that is unpinned: the arithmetic on either side of
    /// `+0x78` is the disassembly's.
    pub(super) fn age_sprite_parts(&mut self, frame_delta: u32) {
        let step = (frame_delta * PART_AGE_STEP).min(u32::from(u16::MAX)) as u16;
        for p in self.parts.actors_mut() {
            p.beat = p.beat.saturating_sub(step);
            if p.beat == 0 {
                p.flags |= crate::minigame_actor::FLAG_KILLED;
            }
        }
        self.parts.retire_dead();
    }

    /// This run's mode (`DAT_801d514c`).
    pub fn mode(&self) -> DanceMode {
        self.mode
    }

    /// Advance the camera keyframe track one tick
    /// ([`DanceCameraTrack::tick`](super::DanceCameraTrack::tick)) and return the pose
    /// it wrote. `None` with no track (a chart-only run) or while the gate
    /// holds the camera (the how-to demo).
    pub fn advance_camera(&mut self, frame_delta: u8) -> Option<super::DanceCameraPose> {
        let mode = self.mode;
        self.camera.as_mut()?.tick(mode, frame_delta)
    }

    /// The camera track's current pose without advancing it.
    pub fn camera_pose(&self) -> Option<super::DanceCameraPose> {
        self.camera.as_ref()?.pose(self.mode)
    }

    /// The camera keyframe track, when the run carries one.
    pub fn camera_track(&self) -> Option<&super::DanceCameraTrack> {
        self.camera.as_ref()
    }

    /// Wired: the play window's dance block (`window/hud.rs`) lays the HUD out
    /// from this list in retail 320x240 framebuffer coordinates each frame,
    /// upscaled through the same stage transform the menu chrome uses. The
    /// `rival_hud` gate is `_DAT_8007B6D0`, which both hosts read through
    /// [`DanceGame::rival_hud_visible`].
    ///
    /// PORT: FUN_801d231c - one frame of the HUD driver, laid out off this
    /// run's own live state.
    ///
    /// `rival_hud` is the `_DAT_8007B6D0` gate: with it clear the two rival
    /// gauges and beat tracks are not drawn at all, even in the versus modes.
    ///
    /// The output is pinned non-vacuous against a real overlay by
    /// `engine-core/tests/dance_minigame_real.rs`.
    pub fn hud_draws(&self, rival_hud: bool) -> Vec<DanceHudDraw> {
        dance_hud_draws(
            self.mode.value(),
            [
                self.dancer_score(0),
                self.dancer_score(1),
                self.dancer_score(2),
            ],
            [
                self.dancer_gauge(0),
                self.dancer_gauge(1),
                self.dancer_gauge(2),
            ],
            rival_hud,
        )
    }

    /// The score-box frame quads for [`DanceGame::hud_draws`], resolved through
    /// the overlay's widget table. Empty when the run was not started from a
    /// real overlay image (the table is disc data).
    pub fn hud_quads(&self, rival_hud: bool) -> Vec<DanceHudQuad> {
        let Some((widget, abr)) = self
            .widgets
            .get(DANCE_SCORE_BOX_WIDGET as usize)
            .map(|(w, a)| (w, *a))
        else {
            return Vec::new();
        };
        self.hud_draws(rival_hud)
            .into_iter()
            .filter_map(|d| match d {
                DanceHudDraw::ScoreBox { x, y } => Some(dance_hud_widget_quad(
                    widget,
                    abr,
                    x,
                    y,
                    DANCE_SCORE_BOX_WIDGET,
                    DANCE_HUD_BRIGHTNESS,
                    0x1000,
                )),
                _ => None,
            })
            .collect()
    }

    /// One number readout as widget quads, mirroring the multi-digit number
    /// renderer's emit loop (`FUN_801d32f8`): per **drawn** slot of
    /// [`dance_number_digits`] the digit's glyph-U is patched into the style's
    /// widget record and the widget is emitted at the style's fixed x step.
    ///
    /// Style A (`style_b == false`) is widget `1` - 16-texel glyphs at a 16-px
    /// step, glyph-U from [`dance_score_digit_u`]. Style B is widget `0x21`,
    /// the narrow counter - 8-texel glyphs at an 8-px step, glyph-U from
    /// [`dance_level_digit_u`]. Empty when the run has no widget table (a
    /// chart-only run not started from a real overlay image).
    pub fn number_quads(&self, style_b: bool, value: u32, x: i16, y: i16) -> Vec<DanceHudQuad> {
        let (widget_id, step, glyph_u): (usize, i16, fn(u8) -> u8) = if style_b {
            (0x21, 8, dance_level_digit_u)
        } else {
            (1, 16, dance_score_digit_u)
        };
        let Some((widget, abr)) = self.widgets.get(widget_id).map(|(w, a)| (*w, *a)) else {
            return Vec::new();
        };
        dance_number_digits(value)
            .iter()
            .enumerate()
            .filter_map(|(i, d)| d.map(|d| (i, d)))
            .map(|(i, d)| {
                let mut w = widget;
                w.u = glyph_u(d);
                dance_hud_widget_quad(
                    &w,
                    abr,
                    x + step * i as i16,
                    y,
                    widget_id as u32,
                    DANCE_HUD_BRIGHTNESS,
                    0x1000,
                )
            })
            .collect()
    }

    /// The `Lv.` gauge readout as widget quads (`FUN_801d3e28`'s emit pair):
    /// the label widget `6` at `(x, y)` and the digit widget `7` eight pixels
    /// on, its glyph-U patched through [`score_thousands_glyph_u`] from the
    /// dancer's raw `gauge` (the `value / 1000` level digit), both at
    /// [`DANCE_HUD_BRIGHTNESS`] and scale `0x1000`. Empty without a widget
    /// table.
    pub fn gauge_readout_quads(&self, gauge: u32, x: i16, y: i16) -> Vec<DanceHudQuad> {
        let mut out = Vec::new();
        if let Some((w, a)) = self.widgets.get(6) {
            out.push(dance_hud_widget_quad(
                w,
                *a,
                x,
                y,
                6,
                DANCE_HUD_BRIGHTNESS,
                0x1000,
            ));
        }
        if let Some((w, a)) = self.widgets.get(7).map(|(w, a)| (*w, *a)) {
            let mut w2 = w;
            w2.u = score_thousands_glyph_u(gauge as i32) as u8;
            out.push(dance_hud_widget_quad(
                &w2,
                a,
                x + 8,
                y,
                7,
                DANCE_HUD_BRIGHTNESS,
                0x1000,
            ));
        }
        out
    }

    /// The full per-frame HUD **quad** list, in retail's emission order: per
    /// [`DanceGame::hud_draws`] element, the style-A digit run for each score
    /// readout, the score-box frame for each box ([`DanceGame::hud_quads`]'s
    /// quad), and the `Lv.` label + digit pair (with the style-B narrow gauge
    /// counter beside it) for each gauge readout. This is the textured-quad
    /// half of the HUD driver frame; which glyph each quad samples is carried
    /// in its patched `uv`, so a host without the dance sprite page resident
    /// still receives the retail geometry + gouraud colours.
    ///
    /// The order is load-bearing. `FUN_801d231c` emits the three digit runs
    /// (`jal 0x801d32f8` at `0x801D23D0..0x801D2428`) **before** the three box
    /// frames (`jal 0x801d2f38` at `0x801D2440..0x801D247C`), and the widget
    /// emitter links every quad into one ordering-table bucket with `AddPrim`
    /// (`jal 0x8003d2c4` at `0x801D32D8`), which prepends - so the frames draw
    /// first and the digits land on top of their opaque interiors. Emitting
    /// the frames first, as this list once did, hid every score under its box
    /// on both hosts.
    pub fn hud_draw_quads(&self, rival_hud: bool) -> Vec<DanceHudQuad> {
        let boxed = self
            .widgets
            .get(DANCE_SCORE_BOX_WIDGET as usize)
            .map(|(w, a)| (*w, *a));
        let mut out = Vec::new();
        for d in self.hud_draws(rival_hud) {
            match d {
                DanceHudDraw::Score { x, y, value, .. } => {
                    out.extend(self.number_quads(false, value, x, y));
                }
                DanceHudDraw::ScoreBox { x, y } => {
                    if let Some((w, abr)) = boxed {
                        out.push(dance_hud_widget_quad(
                            &w,
                            abr,
                            x,
                            y,
                            DANCE_SCORE_BOX_WIDGET,
                            DANCE_HUD_BRIGHTNESS,
                            0x1000,
                        ));
                    }
                }
                DanceHudDraw::Gauge { x, y, value, .. } => {
                    out.extend(self.gauge_readout_quads(value, x, y));
                    out.extend(self.number_quads(true, value, x + 0x18, y));
                }
                DanceHudDraw::BeatTrack { slot, x, y } => {
                    out.extend(self.beat_track_quads(slot, x, y));
                }
            }
        }
        out
    }

    /// One dancer's beat track as widget quads, in submission order - the port of
    /// `FUN_801D2524(slot, x, y)` (`see ghidra/scripts/funcs/overlay_dance_801d2524.txt`).
    ///
    /// Every prim goes to one ordering-table slot through the head-linking
    /// `AddPrim`, so the paint order is the **reverse** of the emission order
    /// this list keeps (the screen-prim builder reproduces the LIFO bucket):
    ///
    /// 1. the triangle-stock markers - widget `0x1F` at `(x + 16i, y + 0x10)`,
    ///    one per remaining triangle of the human (`DAT_801D534C`, a fixed
    ///    address: every track shows the human's stock);
    /// 2. under a draw area of `[x, x + 0x50)` (the `E3` / `E4` pair at
    ///    `0x801D28E8..0x801D2974`): twelve body tiles, widget `0x1E` at
    ///    `(x + 8i, y)`, then eight notes - widget `sym + 0xD` at
    ///    `x + 16i - (phase * 16 / 281 + 5) - 4`, `sym` the chart cell of
    ///    beat `(beat + i - 1) & 31` on the row the dancer's level
    ///    (`gauge / 1000`) selects, CLUT `0x7D0E`;
    /// 3. the draw area back to the full screen, then the right cap
    ///    (`0x11` at `x + 0x54`), the left cap (`0x10` at `x - 4`) and the
    ///    marker arrow (`0x12` at `(x + 8, y - 8)`), unclipped over the body.
    ///
    /// The body and caps take CLUT `0x7D0D` on a flash beat and `0x7D08`
    /// otherwise: a flash beat is `beat & 7 == 3` (`beat & 3` at level `0`)
    /// within the first `0x46` phase units (`0x801D2620..0x801D266C`). The
    /// second note's `0xFF` hit-flash pass (`DAT_801D558C`) is not modelled -
    /// the engine keeps no writer of that counter - so every note draws at
    /// `0x80`. Empty without a widget table.
    ///
    /// PORT: FUN_801d2524 (the emit sequence and draw area; the flash test and
    /// note x are [`dance_combo_window_bright`] / [`dance_beat_track_note_x`])
    pub fn beat_track_quads(&self, slot: usize, x: i16, y: i16) -> Vec<DanceHudQuad> {
        const CLIP_W: i16 = 0x50;
        if self.widgets.is_empty() {
            return Vec::new();
        }
        let level = self.dancer_gauge(slot) / GAUGE_STEP;
        let beat = self.phase / BEAT_PERIOD;
        let phase_in = self.phase - beat * BEAT_PERIOD;
        let track_clut = if dance_combo_window_bright(beat, level, phase_in) {
            BEAT_TRACK_CLUT_COMBO
        } else {
            BEAT_TRACK_CLUT_IDLE
        };
        let quad = |id: usize, qx: i16, qy: i16, clut: Option<u16>| {
            self.widgets.get(id).map(|(w, abr)| {
                let mut w = *w;
                if let Some(c) = clut {
                    w.clut = c;
                }
                dance_hud_widget_quad(&w, *abr, qx, qy, id as u32, DANCE_HUD_BRIGHTNESS, 0x1000)
            })
        };
        let clip = |q: DanceHudQuad| -> Option<DanceHudQuad> {
            let (lo, hi) = (x, x + CLIP_W);
            if q.x1 <= lo || q.x0 >= hi {
                return None;
            }
            let mut q = q;
            let cut_l = (lo - q.x0).max(0);
            let cut_r = (q.x1 - hi).max(0);
            q.x0 += cut_l;
            q.x1 -= cut_r;
            for (i, uv) in q.uv.iter_mut().enumerate() {
                if i % 2 == 0 {
                    uv.0 = uv.0.wrapping_add(cut_l as u8);
                } else {
                    uv.0 = uv.0.wrapping_sub(cut_r as u8);
                }
            }
            Some(q)
        };
        // Emission (submission) order, as retail links the packets; the
        // screen-prim builder paints a shared bucket last-submitted first.
        let mut out = Vec::new();
        out.extend(quad(0x12, x + 8, y - 8, None));
        out.extend(quad(0x10, x - 4, y, Some(track_clut)));
        out.extend(quad(0x11, x + 0x54, y, Some(track_clut)));
        let row = self.chart.rows.get(level as usize);
        for i in 0..8u32 {
            let cell = (beat + i).wrapping_sub(1) & 31;
            let sym = row.map_or(0, |r| r[cell as usize % r.len()]) as usize;
            let nx = dance_beat_track_note_x(i32::from(x), i, phase_in) as i16;
            out.extend(quad(sym + 0xD, nx, y, Some(BEAT_TRACK_CLUT_NOTE)).and_then(clip));
        }
        for i in 0..12 {
            out.extend(quad(0x1E, x + 8 * i, y, Some(track_clut)).and_then(clip));
        }
        for i in 0..self.triangles() as i16 {
            out.extend(quad(0x1F, x + 16 * i, y + 0x10, None));
        }
        out
    }

    /// One record of the run's own widget table, with its `+0x13` ABR byte.
    ///
    /// The table is disc data parsed from the user's image
    /// ([`dance_widgets_with_abr`]), so a chart-only run started without a
    /// real overlay returns `None` and a host falls back to its placeholder.
    /// Both hosts reach the count-in banner's record (index `0`) through this
    /// rather than baking its cell, CLUT and page in twice.
    pub fn widget(&self, index: usize) -> Option<(legaia_asset::dance_art::DanceWidget, u8)> {
        self.widgets.get(index).map(|(w, a)| (*w, *a))
    }

    /// The VRAM rects the run's HUD samples: one `(image_origin, clut_origin)`
    /// pair per distinct widget texture page, in halfword framebuffer
    /// coordinates.
    ///
    /// This is what a host stages the dance's own texture page from. Retail
    /// never has to ask, because the dance **is** `other7`'s scene and the
    /// page is resident the moment the hall loads; a port that hosts the
    /// session over the scene the player walked in from has to name the rects
    /// it needs, and naming them off the live table is what keeps the answer
    /// disc-derived. See [`DANCE_HUD_ART_PROT_ENTRY`].
    pub fn hud_vram_rects(&self) -> Vec<DanceHudRect> {
        let mut out: Vec<DanceHudRect> = Vec::new();
        for (w, _) in &self.widgets {
            let page = w.tpage_xy();
            let clut = (((w.clut & 0x3F) * 16), (w.clut >> 6) & 0x1FF);
            if !out.iter().any(|&(p, c)| p == page && c == clut) {
                out.push((page, clut));
            }
        }
        out
    }

    /// The triangle feedback window's remaining frames (`DAT_801d5144`) - the
    /// raw counter behind [`DanceGame::triangle_feedback`], which the tutorial
    /// actor's practice step reads directly.
    pub fn feedback_frames(&self) -> u32 {
        self.feedback
    }

    // ---------------------------------------------------------------- clock

    /// Intra-beat phase (`phase % BEAT_PERIOD`).
    pub fn intra_beat_phase(&self) -> u32 {
        self.phase % BEAT_PERIOD
    }

    /// Beat index (`phase / BEAT_PERIOD`), `0..=31`.
    pub fn beat_index(&self) -> u32 {
        self.phase / BEAT_PERIOD
    }

    /// `true` when the intra-beat phase is in the dead zone (past the window) -
    /// no note is active, presses miss.
    pub fn in_dead_zone(&self) -> bool {
        self.intra_beat_phase() > BEAT_WINDOW
    }

    /// `true` on a 4-beat combo slot - the beat a triangle should be spent on
    /// (`FUN_801d1af4`: `(beat & 3) == 3 && phase < 0xd2`).
    pub fn on_combo_slot(&self) -> bool {
        self.beat_index() & 3 == 3 && !self.in_dead_zone()
    }

    /// The accuracy weight for the current phase (`FUN_801d1960`:
    /// `0x1000 - phase * 0x1000 / 0xd2`), `0` in the dead zone.
    pub fn accuracy_weight(&self) -> u32 {
        let p = self.intra_beat_phase();
        if p > BEAT_WINDOW {
            return 0;
        }
        ACCURACY_MAX - (p * ACCURACY_MAX) / BEAT_WINDOW
    }

    /// Song-timer position (`DAT_801d5820`), saturating at the song length.
    pub fn song_timer(&self) -> u32 {
        self.song_timer
    }

    /// This run's song-length limit ([`SONG_LEN_SHORT`] / [`SONG_LEN_LONG`]).
    pub fn song_len(&self) -> u32 {
        self.song_len
    }

    /// `true` once the song timer has reached this run's length limit.
    pub fn song_over(&self) -> bool {
        self.song_timer >= self.song_len
    }

    /// `true` once the song is over **and** states `0xB` / `0xC` have run -
    /// the `3 2 1 FINISH!` countdown and the wipe under it - which is when
    /// retail reaches its results state `0x14`. A chart-only run (no
    /// overlay programs) finishes with the song.
    pub fn finished(&self) -> bool {
        self.song_over() && (self.finished || self.finish_programs.is_empty())
    }

    /// Whether the run is in states `0xB` / `0xC` - the song over, the
    /// countdown and wipe still running - where the award routine still
    /// judges and a landed triangle pays [`MULT_FINALE`].
    pub fn in_finale(&self) -> bool {
        self.song_over() && !self.finished()
    }

    /// Whether dancer `i` landed a triangle during the finale
    /// (`DAT_801d538c[player]`).
    pub fn finale_landed(&self, i: usize) -> bool {
        self.dancers.get(i).is_some_and(|d| d.finale_landed)
    }

    /// The countdown's cues since the last call (`0x206..=0x209`).
    pub fn take_finish_cues(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.finish_cues)
    }

    /// Step states `0xB` / `0xC` `frame_delta` vsyncs once the song is over.
    // PORT: FUN_801cf470 (states 0xB / 0xC: the countdown spawns and the wipe)
    fn advance_finish(&mut self, frame_delta: u32) {
        if !self.song_over() || self.finished || self.finish_programs.is_empty() {
            return;
        }
        let fc = self
            .finish
            .get_or_insert_with(|| super::FinishCountdown::new(&self.finish_programs));
        for _ in 0..frame_delta {
            let s = fc.step();
            self.finish_cues.extend(s.cues);
            if s.done {
                self.finished = true;
                break;
            }
        }
        if self.finished {
            self.finish = None;
        }
    }

    // --------------------------------------------------------------- actors

    /// The dancer actor pool - one record per floor slot, live every frame.
    ///
    /// This is the port's equivalent of the actors `FUN_801d0190` spawns:
    /// position, flag word, bound clip id and beat field, in the retail slots
    /// the dance overlay's draw kernels read.
    pub fn dancer_actors(&self) -> &[crate::minigame_actor::MinigameActor] {
        self.actors.actors()
    }

    /// One frame of per-dancer clip work: [`dance_clip_driver_gate`] applied to
    /// each floor slot's actor record.
    ///
    /// `clip_driver` says whether the shared clip driver runs for that dancer
    /// this frame - the whole of `FUN_801d4098`.
    pub fn dancer_clip_frames(&self) -> Vec<DancerClipFrame> {
        self.actors
            .actors()
            .iter()
            .enumerate()
            .map(|(slot, a)| DancerClipFrame {
                slot,
                clip_id: a.field_5c,
                clip_rate: a.cursor as u16,
                clip_driver: dance_clip_driver_gate(a.field_5c, a.flags),
                party_bank: a.flags & crate::minigame_actor::FLAG_PARTY_CLIP_BANK != 0,
            })
            .collect()
    }

    /// The live sprite parts.
    pub fn sprite_parts(&self) -> &[crate::minigame_actor::MinigameActor] {
        self.parts.actors()
    }

    /// One frame of sprite-part draw work: [`sprite_part_emit`] and
    /// [`sprite_part_fade_weight`] applied to every live part.
    ///
    /// This is what makes those two kernels live. A host draws the emitted
    /// quads (the shadowed arm is two of them per part) at
    /// [`SpritePartEmit`]'s screen pair, modulated by `fade`.
    pub fn sprite_part_emits(&self) -> Vec<SpritePartFrame> {
        let n = self.parts.actors().len();
        let mut out: Vec<SpritePartFrame> = self
            .parts
            .actors()
            .iter()
            .enumerate()
            .map(|(index, a)| SpritePartFrame {
                index,
                emit: sprite_part_emit(a.draw_mode, a.pos[0], a.pos[1], a.sprite),
                fade: sprite_part_fade_weight(a.beat),
                sprite: a.sprite,
            })
            .collect();
        // The song-end countdown's parts, on the ticks their programs call
        // the sprite hook - the same case-2 emit (`FUN_801D387C`).
        if let Some(fc) = self.finish.as_ref() {
            out.extend(
                fc.draws()
                    .into_iter()
                    .enumerate()
                    .map(|(i, d)| SpritePartFrame {
                        index: n + i,
                        emit: sprite_part_emit(
                            PART_DRAW_MODE,
                            super::FINISH_SEAT.0,
                            super::FINISH_SEAT.1,
                            d.sprite,
                        ),
                        fade: d.fade,
                        sprite: d.sprite,
                    }),
            );
        }
        out
    }

    // ---------------------------------------------------------------- state

    /// The human's running score.
    pub fn score(&self) -> u32 {
        self.dancers[0].score
    }

    /// The human's groove gauge.
    pub fn gauge(&self) -> u32 {
        self.dancers[0].gauge
    }

    /// The human's difficulty lane (`gauge / GAUGE_STEP`).
    pub fn lane(&self) -> usize {
        self.dancers[0].lane(self.chart.rows.len()) as usize
    }

    /// Triangles the human has left this song.
    pub fn triangles(&self) -> u32 {
        self.dancers[0].triangles
    }

    /// Frames of groovy-move spin still to run on the human - input is ignored
    /// while this is non-zero.
    pub fn groovy_lock(&self) -> u32 {
        self.spin_frames_left(0)
    }

    /// `true` while the human is inside the groovy-move window.
    pub fn in_groovy_move(&self) -> bool {
        self.dancers[0].spin_turns > 0
    }

    /// The triangle feedback window (`DAT_801d5144`) still running, and whether
    /// the spend that armed it landed on the combo slot.
    pub fn triangle_feedback(&self) -> Option<bool> {
        (self.feedback > 0).then(|| self.dancers[0].landed)
    }

    /// Dancers on the floor (slot 0 = the human).
    pub fn dancer_count(&self) -> usize {
        self.dancers.len()
    }

    /// Dancer `i`'s score (`DAT_801d53cc[i]`).
    pub fn dancer_score(&self, i: usize) -> u32 {
        self.dancers.get(i).map(|d| d.score).unwrap_or(0)
    }

    /// Dancer `i`'s groove gauge.
    pub fn dancer_gauge(&self, i: usize) -> u32 {
        self.dancers.get(i).map(|d| d.gauge).unwrap_or(0)
    }

    /// Dancer `i`'s difficulty lane.
    pub fn dancer_lane(&self, i: usize) -> usize {
        self.dancers
            .get(i)
            .map(|d| d.lane(self.chart.rows.len()) as usize)
            .unwrap_or(0)
    }

    /// Dancer `i`'s remaining triangles.
    pub fn dancer_triangles(&self, i: usize) -> u32 {
        self.dancers.get(i).map(|d| d.triangles).unwrap_or(0)
    }

    /// Dancer `i`'s kind (the row both scoring tables are indexed by).
    pub fn dancer_kind(&self, i: usize) -> usize {
        self.dancers.get(i).map(|d| d.kind).unwrap_or(0)
    }

    /// Final solo-style grade (retail mode 2): `true` when the score meets
    /// [`WIN_THRESHOLD_SOLO`].
    pub fn passed(&self) -> bool {
        self.score() >= WIN_THRESHOLD_SOLO
    }

    /// Whether the results state clears the run's pass flag [`WIN_FLAG`]
    /// (`FUN_801cf470` state `0x14`, `0x801CFE80..0x801CFF14`).
    ///
    /// Per mode: the qualifier compares the human's score against score slot
    /// 2 (`lw v1,0x8(a1)`), the finals against slot 1 (`lw v1,0x4(a1)`), and
    /// clears on `human < rival` (`slt`, so a tie keeps the flag). The how-to
    /// demo clears when the score is **not** below `0x12D` (`slti` / `bne` to
    /// the skip), and free play grades nothing.
    // PORT: FUN_801cf470 (results-state grade)
    pub fn results_clear_win_flag(&self) -> bool {
        let me = self.score() as i32;
        let slot = |i: usize| self.dancers.get(i).map_or(0, |d| d.score as i32);
        match self.mode {
            DanceMode::Qualifier => me < slot(2),
            DanceMode::Finals => me < slot(1),
            DanceMode::HowTo => me >= 0x12D,
            DanceMode::FreePlay => false,
        }
    }

    /// The versus grade (retail modes 0/1): the human out-scores every rival on
    /// the floor. Ties go to the human - retail clears the win flag only when
    /// `human < rival`.
    pub fn beating_rivals(&self) -> bool {
        let me = self.score();
        self.dancers.iter().skip(1).all(|d| me >= d.score)
    }

    // ---------------------------------------------------------------- chart

    /// The chart symbol the **hit judge** (`FUN_801d1960`) matches a press
    /// against for the human's lane + beat: `None` in the dead zone, `Some(0)`
    /// when the beat carries no note, else the direction symbol.
    // PORT: FUN_801d1960 (the judged chart cell)
    pub fn judged_symbol(&self) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        Some(self.cell(self.lane(), self.beat_index()))
    }

    /// The symbol the **CPU auto-feed** would press for the human's lane
    /// (`FUN_801d1820` - the display half, which substitutes the triangle symbol
    /// `3` on the combo slot once the dancer's schedule is due). Kept for hosts
    /// that draw the retail "displayed" note; only [`Self::judged_symbol`]
    /// scores a direction.
    // PORT: FUN_801d1820 (chart lookup - the auto-feed / display half)
    pub fn required_symbol(&self) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        let beat = self.beat_index();
        if beat & 3 == 3 {
            return Some(3);
        }
        Some(self.cell(self.lane(), beat))
    }

    /// The chart row `lane`, for a host drawing the note highway.
    pub fn chart_row(&self, lane: usize) -> Option<&[u8; BEATS_PER_ROW]> {
        self.chart.rows.get(lane)
    }

    pub(super) fn cell(&self, lane: usize, beat: u32) -> u8 {
        self.chart
            .symbol(lane, (beat as usize) % BEATS_PER_ROW)
            .unwrap_or(0)
    }

    // ---------------------------------------------------------------- frame

    /// Advance one frame (`FUN_801cf470` state 10 + `FUN_801d1358` per dancer):
    /// step the beat clock, decay each dancer's latches / groovy spin, bank the
    /// combo slot, and run the **CPU dancers' auto-fed presses** through the same
    /// judge + award the human's presses go through.
    // PORT: FUN_801cf470 (beat clock + song-end test, states 10..12)
    // PORT: FUN_801d1358 (per-dancer handler: latch decay, spin, chart auto-feed)
    pub fn advance(&mut self, frame_delta: u32) {
        self.advance_step_markers(frame_delta);
        let step = frame_delta * PHASE_PER_DELTA;
        self.phase = (self.phase + step) % BEAT_PHASE_WRAP;
        // The song timer saturates at the length limit (the retail clock keeps
        // counting but the run ends; clamping keeps `song_over` monotone).
        self.song_timer = self.song_timer.saturating_add(step).min(self.song_len);
        self.advance_finish(frame_delta);
        self.feedback = self.feedback.saturating_sub(frame_delta);

        let beat = self.beat_index();
        let rows = self.chart.rows.len();
        for d in &mut self.dancers {
            // The bound judge move plays on (the clip driver's cursor).
            d.move_left = d.move_left.saturating_sub(frame_delta);
            // Latch decay (`timer -= 2 * delta`; at 0 the latch clears).
            if d.latch_timer > 0 {
                d.latch_timer -= NOTE_LATCH_DECAY * frame_delta as i32;
                if d.latch_timer < 1 {
                    d.latch_timer = 0;
                    d.latch = 0;
                }
            }
            // Groovy-move spin: the dancer turns once per SPIN_TURN_UNITS of
            // accumulated yaw, `lane + 1` turns in all.
            if d.spin_turns > 0 {
                let rate = SPIN_RATE_BASE + d.lane(rows) * SPIN_RATE_PER_LANE;
                d.spin_acc += rate * frame_delta;
                while d.spin_acc >= SPIN_TURN_UNITS && d.spin_turns > 0 {
                    d.spin_acc -= SPIN_TURN_UNITS;
                    d.spin_turns -= 1;
                }
                if d.spin_turns == 0 {
                    d.spin_acc = 0;
                }
            }
            // Chain cursor clears once per 8-beat bar.
            if beat.is_multiple_of(CURSOR_RESET_BEATS) && d.last_reset_beat != Some(beat) {
                d.last_reset_beat = Some(beat);
                d.cursor = 0;
            }
            // Bank one combo slot per 4-beat boundary (the CPU triangle clock).
            if beat & 3 == 3 && d.last_meter_beat != Some(beat) {
                d.last_meter_beat = Some(beat);
                d.tri_meter += 1;
            }
        }

        // The competitors' pad word is synthesised from the chart every frame.
        for i in 1..self.dancers.len() {
            if let Some(sym) = self.auto_feed(i) {
                match sym {
                    1 => {
                        self.award(i, DanceDir::A);
                    }
                    2 => {
                        self.award(i, DanceDir::B);
                    }
                    3 => {
                        self.award(i, DanceDir::C);
                    }
                    _ => {}
                }
            }
        }

        // `FUN_801d1358` rebinds a dancer's loop clip once its judge-triggered
        // reaction / move clip raises the clip driver's end flag. With the
        // choreography bank attached that is the move's own length; a
        // chart-only run has no clip lengths and times it off the note latch
        // the judge arms instead.
        let timed = self.clip_ticks.is_some();
        for i in 0..self.dancers.len() {
            let d = &self.dancers[i];
            let free = if timed {
                d.move_left == 0
            } else {
                d.latch_timer == 0
            };
            if free {
                self.bind_loop_clip(i);
            }
        }
        self.sync_dancer_actors();
        self.age_sprite_parts(frame_delta);
    }

    /// Bind dancer `i`'s standing clip: the kind descriptor's dance-groove loop
    /// during a run, its idle before the beat clock has moved.
    pub(super) fn bind_loop_clip(&mut self, i: usize) {
        let Some(kind) = self.dancers.get(i).map(|d| d.kind) else {
            return;
        };
        let Some(k) = self.kinds.get(kind).cloned() else {
            return;
        };
        let clip = if self.song_timer > 0 { k.dance } else { k.idle };
        if let Some(d) = self.dancers.get_mut(i) {
            d.bind_clip(&clip);
        }
    }

    /// Bind the judge-returned move-pair clip on dancer `i`
    /// (`FUN_801d1af4` returns the pair index, `FUN_801d1358` applies it).
    pub(super) fn bind_move_clip(&mut self, i: usize, pair: usize) {
        let Some(kind) = self.dancers.get(i).map(|d| d.kind) else {
            return;
        };
        let Some(clip) = self
            .kinds
            .get(kind)
            .and_then(|k| k.moves.get(pair))
            .copied()
        else {
            return;
        };
        let len = self.clip_len(&clip).unwrap_or(0);
        if let Some(d) = self.dancers.get_mut(i) {
            d.bind_move(&clip);
            d.move_left = len;
        }
    }

    /// The CPU dancer's synthetic pad symbol for this frame (`FUN_801d1820`):
    /// nothing in the dead zone; on a combo slot the triangle once the kind's
    /// schedule (`DAT_801d41e4`) has banked enough slots; otherwise its own
    /// lane's chart cell.
    // PORT: FUN_801d1820 (the CPU auto-feed)
    pub(super) fn auto_feed(&mut self, i: usize) -> Option<u8> {
        if self.in_dead_zone() {
            return None;
        }
        let beat = self.beat_index();
        let rows = self.chart.rows.len();
        let (lane, due) = {
            let d = &self.dancers[i];
            let due = beat & 3 == 3
                && d.triangles > 0
                && self.tables.schedule(d.kind, d.tri_cursor) <= d.tri_meter;
            (d.lane(rows) as usize, due)
        };
        if due {
            let d = &mut self.dancers[i];
            d.tri_cursor += 1;
            d.tri_meter = 0;
            return Some(3);
        }
        Some(self.cell(lane, beat))
    }

    // ---------------------------------------------------------------- press

    /// Judge a human press. Square / Circle are judged against the chart cell;
    /// **Triangle spends a groovy-move wildcard** (three per song, any beat,
    /// worth the big multiplier only on the 4-beat combo slot, and locking input
    /// out for the length of the spin it throws the dancer into).
    // PORT: FUN_801d1af4 (PROT 0980; score / groove-gauge award; pad-word branches)
    pub fn press(&mut self, dir: DanceDir) -> DanceEvent {
        self.award(0, dir)
    }

    /// Legacy three-way wrapper over [`Self::press`] for hosts matching on
    /// [`Judge`]. An ignored press (mid-groovy-move) folds to [`Judge::Miss`],
    /// but applies no penalty.
    pub fn judge_press(&mut self, dir: DanceDir) -> Judge {
        self.press(dir).judge()
    }

    /// The award routine (`FUN_801d1af4`), for any dancer: the human's presses
    /// and the CPU dancers' auto-fed ones run through exactly this path.
    pub(super) fn award(&mut self, i: usize, dir: DanceDir) -> DanceEvent {
        let beat = self.beat_index();
        if self.dancers[i].locked(beat) {
            return DanceEvent::Ignored;
        }
        let lane = self.dancers[i].lane(self.chart.rows.len()) as usize;
        let ev = if dir.is_triangle() {
            self.spend_triangle(i, beat)
        } else {
            self.judge_direction(i, dir, beat)
        };
        // `FUN_801d1af4`'s return value is a **move-pair index** into the
        // dancer's kind descriptor, which `FUN_801d1358` binds into the
        // actor's `+0x5C` / `+0x6A`. The mapping is the one
        // `legaia_asset::dance_cast` documents: pair 0/1 = the Square/Circle
        // miss reaction, pair `lane*2 + 2 (+1)` = the closed-chain move, pair
        // `8 + lane` = the on-beat / timing-button step. A plain matched note
        // that does not close the chain returns nothing and binds nothing.
        use legaia_asset::dance_cast as dc;
        let circle = dir.symbol() == 2;
        match ev {
            DanceEvent::Miss => {
                let pair = if circle {
                    dc::MOVE_MISS_CIRCLE
                } else {
                    dc::MOVE_MISS_SQUARE
                };
                self.bind_move_clip(i, pair);
            }
            DanceEvent::Sequence { weight, .. } => {
                self.bind_move_clip(i, dc::move_sequence_pair(lane, circle));
                // The human's closed chain fires the sequence-clear banner,
                // which is three `FUN_801d3fd0` spawns into the part pool.
                if i == 0 {
                    let b = good_banner_spawn(weight.min(0xFFFF) as u16);
                    self.spawn_sprite_part(&b.banner);
                    for s in &b.stars {
                        self.spawn_sprite_part(s);
                    }
                }
            }
            DanceEvent::Groovy { .. } => {
                self.bind_move_clip(i, dc::move_beat_pair(lane));
            }
            DanceEvent::Hit { .. } | DanceEvent::Ignored | DanceEvent::NoCharge => {}
        }
        self.sync_dancer_actors();
        ev
    }

    /// The `0x80` / `0x20` branches: judge the press against the chart cell
    /// (`FUN_801d1960`), advance the chain cursor, and award the kind's bonus
    /// when the chain closes. A plain matched note scores **nothing** in retail -
    /// only the closing note does.
    // PORT: FUN_801d1960 (hit judge: dead-zone + accuracy weight + direction match)
    pub(super) fn judge_direction(&mut self, i: usize, dir: DanceDir, beat: u32) -> DanceEvent {
        let rows = self.chart.rows.len();
        let weight = self.accuracy_weight();
        let dead = self.in_dead_zone();
        let lane = self.dancers[i].lane(rows);
        let want = self.cell(lane as usize, beat);
        let bonus = self
            .tables
            .bonus(self.dancers[i].kind, lane as usize)
            .max(0) as u32;

        let d = &mut self.dancers[i];
        // Every judged press latches the dancer (retail binds a reaction / move
        // clip and stops re-judging until it ends).
        d.latch = dir.symbol() as u32;
        d.latch_timer = NOTE_LATCH_TIMER;
        d.last_beat = Some(beat);

        if dead || want == 0 || want != dir.symbol() {
            d.misses += 1;
            return DanceEvent::Miss;
        }
        d.cursor += 1;
        if d.cursor <= lane {
            return DanceEvent::Hit { weight };
        }
        // Chain closed (`cursor + 1 == lane + 1`).
        d.cursor = 0;
        // The human's award is accuracy-weighted (`base/2 + (base * w) >> 13`);
        // a CPU dancer takes the flat table value.
        let points = if i == 0 {
            bonus / 2 + ((bonus * weight) >> 13)
        } else {
            bonus
        };
        d.gauge = (d.gauge + SEQUENCE_GAUGE_STEP).min(GAUGE_MAX);
        d.score = (d.score + points).min(SCORE_MAX);
        d.misses = d.misses.saturating_sub(1);
        DanceEvent::Sequence { weight, points }
    }

    /// The `0x10` branch: **spend a triangle**. Retail gates it on the stock
    /// counter only (no chart match - it is a wildcard on any beat), scores
    /// `(lane+1) * 0x19` when it lands on the 4-beat combo slot inside the window
    /// (plus a full `+1000` gauge step, which promotes the lane) and only
    /// `(lane+1) * 3` when it does not, and throws the dancer into a `lane + 1`
    /// turn spin during which no press is judged.
    // PORT: FUN_801d1af4 (PROT 0980; the pad-0x10 groovy-move branch)
    pub(super) fn spend_triangle(&mut self, i: usize, beat: u32) -> DanceEvent {
        let rows = self.chart.rows.len();
        let landed = self.on_combo_slot();
        let finale = self.in_finale();
        let d = &mut self.dancers[i];
        if d.triangles == 0 {
            return DanceEvent::NoCharge;
        }
        d.triangles -= 1;
        d.latch = 3;
        d.latch_timer = NOTE_LATCH_TIMER;
        d.last_beat = Some(beat);
        let lane = d.lane(rows);
        d.landed = landed;
        let points = if landed {
            d.gauge = (d.gauge + GAUGE_STEP).min(GAUGE_MAX);
            // `0x801D1CE0..0x801D1D30`: in states 0xB / 0xC (`state - 0xB <
            // 2`) the landed triangle pays `(lane + 1) * 17 << 1` and raises
            // `DAT_801d538c[player]`; otherwise `(lane + 1) * 25`.
            if finale {
                d.finale_landed = true;
                (lane + 1) * MULT_FINALE
            } else {
                (lane + 1) * MULT_COMBO
            }
        } else {
            (lane + 1) * MULT_ORDINARY
        };
        d.score = (d.score + points).min(SCORE_MAX);
        // The groovy move: `lane + 1` full turns of the dancer's yaw, spun at
        // `0x80 + lane * 0x20` units per frame - up to 64 frames of locked-out
        // input, the whole time retail is playing the move clip.
        d.spin_turns = lane + 1;
        d.spin_acc = 0;
        let left = d.triangles;
        if i == 0 {
            self.feedback = TRIANGLE_FEEDBACK_WINDOW;
        }
        DanceEvent::Groovy {
            landed,
            points,
            lock: self.spin_frames_left(i),
            left,
        }
    }

    /// Frames of groovy-move spin still to run on dancer `i` - the window its
    /// input is disrupted for. The spin rate is read from the dancer's *current*
    /// lane each frame (`FUN_801d1358`), so a landed triangle's own gauge step
    /// speeds up the move it started.
    pub(super) fn spin_frames_left(&self, i: usize) -> u32 {
        let Some(d) = self.dancers.get(i) else {
            return 0;
        };
        if d.spin_turns == 0 {
            return 0;
        }
        let rate = SPIN_RATE_BASE + d.lane(self.chart.rows.len()) * SPIN_RATE_PER_LANE;
        (d.spin_turns * SPIN_TURN_UNITS - d.spin_acc).div_ceil(rate)
    }
}
