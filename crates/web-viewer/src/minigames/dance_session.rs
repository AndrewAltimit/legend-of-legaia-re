//! The dance rules-engine shell: start / tutorial / count-in / tick / press
//! and the state, chart and actor JSON the page reads back.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    // ---------------------------------------------------------------- dance

    /// Start a dance run on the disc's baked step chart, scoring tables and
    /// qualifier cast (all rodata of PROT 0980). `long_song` picks the long
    /// song-length limit. Returns `false` when the overlay didn't decode.
    pub fn dance_start(&mut self, long_song: bool) -> bool {
        let Some(img) = self.dance_overlay() else {
            return false;
        };
        let Some(mut game) = DanceGame::from_overlay(&img, long_song) else {
            return false;
        };
        // The hall's choreography bank (decoded with the bodies) times each
        // judge move, as on the play hosts.
        if let Some(b) = self.dance_bodies.as_ref() {
            game.attach_clip_bank(b.clip_bank());
        }
        self.dance = Some(game);
        self.dance_countin = Some(legaia_engine_minigames::dance::CountIn::new());
        self.dance_tutorial = None;
        self.dance_tutorial_frame = None;
        true
    }

    /// Start a run in one of the overlay's four modes (`DAT_801D514C`):
    /// `0` qualifier, `1` finals, `2` the **how-to** (one dancer, short song,
    /// and the Disco King tutorial actor `FUN_801D0750` beside it), `3` free
    /// play - what the native window's `start_dance_minigame_mode` and the
    /// play page's door-warp entry run. Returns `false` when the overlay
    /// didn't decode.
    pub fn dance_start_mode(&mut self, mode: u8, long_song: bool) -> bool {
        use legaia_engine_minigames::dance::DanceMode as M;
        let mode = match mode {
            1 => M::Finals,
            2 => M::HowTo,
            3 => M::FreePlay,
            _ => M::Qualifier,
        };
        let Some(img) = self.dance_overlay() else {
            return false;
        };
        let Some(mut game) = DanceGame::from_overlay_for_mode(&img, mode, long_song) else {
            return false;
        };
        if let Some(b) = self.dance_bodies.as_ref() {
            game.attach_clip_bank(b.clip_bank());
        }
        self.dance = Some(game);
        self.dance_countin = Some(legaia_engine_minigames::dance::CountIn::new());
        self.dance_tutorial =
            (mode == M::HowTo).then(legaia_engine_minigames::dance_tutorial::DanceTutorial::new);
        self.dance_tutorial_frame = None;
        true
    }

    /// Whether a how-to run's tutorial actor is live.
    pub fn dance_tutorial_active(&self) -> bool {
        self.dance_tutorial.is_some()
    }

    /// Step the Disco King tutorial one frame beside the run - the step
    /// `World::step_dance_tutorial` runs on the play hosts, count-in
    /// included. `buttons` is this frame's newly pressed set as bits: `1` a
    /// face button (advance / confirm), `2` cursor back, `4` cursor on.
    /// Returns the cue id the frame fires (`0` for none).
    pub fn dance_tutorial_step(&mut self, buttons: u8) -> u32 {
        use legaia_engine_minigames::dance_tutorial as dt;
        let mut pad = 0u16;
        if buttons & 1 != 0 {
            pad |= dt::PAD_ADVANCE;
        }
        if buttons & 2 != 0 {
            pad |= dt::PAD_CURSOR_PREV;
        }
        if buttons & 4 != 0 {
            pad |= dt::PAD_CURSOR_NEXT;
        }
        let (score, feedback_frames, combo_hit) = self
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
        let Some(tut) = self.dance_tutorial.as_mut() else {
            return 0;
        };
        let frame = tut.step(pad, score, feedback_frames, combo_hit, 1);
        let cue = frame.cue.map_or(0, u32::from);
        if frame.done {
            self.dance_tutorial = None;
            self.dance_tutorial_frame = None;
        } else {
            self.dance_tutorial_frame = Some(frame);
        }
        cue
    }

    /// The tutorial's captions / options / cursor / feedback for this frame
    /// as a `w x h` RGBA8 overlay (transparent elsewhere), through the shared
    /// `dance::dance_tutorial_draws_for` both play hosts draw. Empty with
    /// no live tutorial or no dialog font.
    pub fn dance_tutorial_rgba(&self, w: u32, h: u32) -> Vec<u8> {
        use legaia_engine_ui::dance as ui_dance;
        let (Some(tf), Some(font)) = (self.dance_tutorial_frame.as_ref(), self.slot_font.as_ref())
        else {
            return Vec::new();
        };
        let draws = ui_dance::dance_tutorial_draws_for(
            font,
            ui_dance::DanceTutorialView {
                captions: &tf.captions,
                options: tf.options,
                cursor_pos: tf.cursor_pos,
                feedback: tf.feedback,
            },
            (0, 0),
            1,
        );
        let mut px = vec![0u8; (w * h * 4) as usize];
        blit_text(&mut px, w, h, font, &draws);
        // Alpha where the text landed, so the page can composite it.
        for p in px.as_chunks_mut::<4>().0 {
            p[3] = if p[0] | p[1] | p[2] != 0 { 0xFF } else { 0 };
        }
        px
    }

    /// Step the count-in one vsync - the engine's `dance::CountIn`, which the
    /// play hosts' `World::tick_dance` steps the same way. Returns
    /// `[active, ready, x_offset, brightness, hold, go, cue, done]`:
    /// `active` `0` once the song runs (the rest then zero); `ready` `1`
    /// while the READY banner draws, with its envelope in the next three;
    /// `go` the `GO!` brightness or `-1`; `cue` the cue id this vsync fires
    /// (`0x200` intro, `0x201` run start) or `0`; `done` `1` on the vsync the
    /// song starts. The page holds the beat clock and the music until `done`.
    pub fn dance_countin_step(&mut self) -> Vec<i32> {
        let Some(ci) = self.dance_countin.as_mut() else {
            return vec![0; 8];
        };
        let s = ci.step();
        if s.done {
            self.dance_countin = None;
        }
        let (ready, x, b, hold) = match s.banner {
            Some(e) => (1, e.x_offset, e.brightness, i32::from(e.hold)),
            None => (0, 0, 0, 0),
        };
        vec![
            1,
            ready,
            x,
            b,
            hold,
            s.go.unwrap_or(-1),
            s.cue.map_or(0, i32::from),
            i32::from(s.done),
        ]
    }

    /// Advance the beat clock by `frames` frames (the retail clock steps
    /// `frame_delta * 10` phase units per frame). This also runs the **CPU
    /// dancers**: retail feeds them the chart every frame through the same judge
    /// and award routine the human's presses take, so their scores climb here.
    ///
    /// The run's camera keyframe track steps with it (`FUN_801CF470`'s
    /// camera block, `frames` as its frame delta), which is what
    /// `dance_venue_vp` frames the hall through.
    pub fn dance_tick(&mut self, frames: u32) {
        if let Some(g) = self.dance.as_mut() {
            g.advance(frames);
            g.advance_camera(frames.min(u32::from(u8::MAX)) as u8);
        }
    }

    /// Press a dance button. `1` = Square, `2` = Circle (the judged directions),
    /// `3` = **Triangle**, the three-per-song "groovy move" wildcard.
    ///
    /// Returns the event name: `"miss"` / `"hit"` / `"sequence"` for a direction,
    /// `"groovy"` / `"groovy_off"` for a triangle spent on / off the 4-beat combo
    /// slot, `"no_charge"` when the stock is empty, and `"ignored"` while the
    /// dancer is inside a groovy move (input is disrupted for its whole spin).
    /// `"none"` with no live run.
    pub fn dance_press(&mut self, dir: u8) -> String {
        let Some(g) = self.dance.as_mut() else {
            return "none".to_string();
        };
        let d = match dir {
            1 => DanceDir::A,
            2 => DanceDir::B,
            3 => DanceDir::C,
            _ => return "none".to_string(),
        };
        match g.press(d) {
            DanceEvent::Miss => "miss",
            DanceEvent::Hit { .. } => "hit",
            DanceEvent::Sequence { .. } => "sequence",
            DanceEvent::Groovy { landed: true, .. } => "groovy",
            DanceEvent::Groovy { landed: false, .. } => "groovy_off",
            DanceEvent::NoCharge => "no_charge",
            DanceEvent::Ignored => "ignored",
        }
        .to_string()
    }

    /// Live dance state.
    ///
    /// ```json
    /// { "live": true, "score": 0, "gauge": 0, "lane": 0, "beat": 3,
    ///   "phase": 40, "period": 281, "window": 210, "accuracy": 3200, "dead_zone": false,
    ///   "combo_slot": true, "judged": 2, "displayed": 3,
    ///   "triangles": 3, "lock": 0, "feedback": null,
    ///   "rivals": [ {"score": 12, "gauge": 500, "lane": 0, "kind": 2, "triangles": 3}, .. ],
    ///   "song_timer": 900, "song_len": 16860, "over": false, "passed": false,
    ///   "winning": true }
    /// ```
    ///
    /// **`judged` is the step to press.** Retail splits the chart lookup
    /// (`FUN_801d1820`) into two halves: the hit judge (`FUN_801d1960`) matches
    /// a press against the raw chart cell (`judged`), while the display /
    /// auto-feed half substitutes the triangle symbol `3` on every 4th beat
    /// (`displayed`). Both are surfaced; only `judged` scores a direction. `0` =
    /// the beat carries no step, `null` = the dead zone between beats.
    ///
    /// `triangles` is the groovy-move stock (3 per song); `lock` is the frames of
    /// groovy-move spin still disrupting input; `feedback` is `true`/`false`
    /// while the post-spend caption window runs (whether it landed on the combo
    /// slot), `null` otherwise. `rivals` are the two CPU dancers, scoring live
    /// off the same chart.
    pub fn dance_state_json(&self) -> String {
        let Some(g) = self.dance.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let opt = |s: Option<u8>| match s {
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        let rivals = (1..g.dancer_count())
            .map(|i| {
                format!(
                    r#"{{"score":{},"gauge":{},"lane":{},"kind":{},"triangles":{}}}"#,
                    g.dancer_score(i),
                    g.dancer_gauge(i),
                    g.dancer_lane(i),
                    g.dancer_kind(i),
                    g.dancer_triangles(i),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                r#"{{"live":true,"score":{},"gauge":{},"lane":{},"beat":{},"phase":{},"#,
                r#""period":{},"window":{},"accuracy":{},"dead_zone":{},"combo_slot":{},"#,
                r#""judged":{},"displayed":{},"triangles":{},"lock":{},"feedback":{},"#,
                r#""rivals":[{}],"song_timer":{},"song_len":{},"over":{},"passed":{},"winning":{}}}"#
            ),
            g.score(),
            g.gauge(),
            g.lane(),
            g.beat_index(),
            g.intra_beat_phase(),
            legaia_engine_minigames::dance::BEAT_PERIOD,
            legaia_engine_minigames::dance::BEAT_WINDOW,
            g.accuracy_weight(),
            g.in_dead_zone(),
            g.on_combo_slot(),
            opt(g.judged_symbol()),
            opt(g.required_symbol()),
            g.triangles(),
            g.groovy_lock(),
            match g.triangle_feedback() {
                Some(landed) => landed.to_string(),
                None => "null".to_string(),
            },
            rivals,
            g.song_timer(),
            g.song_len(),
            g.finished(),
            g.passed(),
            g.beating_rivals(),
        )
    }

    /// The whole decoded step chart, for the page's scrolling note lane:
    /// `{"rows":[[u8; 32], ...]}` (one row per difficulty lane).
    pub fn dance_chart_json(&self) -> String {
        let Some(c) = self.dance_chart() else {
            return r#"{"rows":[]}"#.to_string();
        };
        let rows = c
            .rows
            .iter()
            .map(|r| {
                let cells = r
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                format!("[{cells}]")
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"rows":[{rows}]}}"#)
    }

    /// The dance run's **actor records** - the per-frame output of the three
    /// draw kernels the dance overlay runs over its own actor pools.
    ///
    /// ```json
    /// { "dancers": [ {"slot":0,"x":0,"y":-128,"z":0,"clip":6,"rate":8,
    ///                 "clip_driver":true,"party_bank":false} ],
    ///   "parts":   [ {"sprite":11,"x":160,"y":144,"fade":32,"shadow":true} ] }
    /// ```
    ///
    /// `dancers` is [`legaia_engine_minigames::dance::dance_clip_driver_gate`]
    /// (`FUN_801d4098`) over the pool `FUN_801d0190`'s spawn stores seed:
    /// `clip` is the actor's `+0x5C` bound anim id and `clip_driver` whether
    /// the shared clip driver runs for it this frame.
    ///
    /// `parts` is [`legaia_engine_minigames::dance::sprite_part_emit`]
    /// (`FUN_801d387c`) plus its fade prologue over the sprite parts the run
    /// spawns on a scoring judge - `x`/`y` are already through the emit's
    /// `>> 3`, so they are page pixels in the retail 320x240 stage, and
    /// `shadow` marks the two-emit arm (draw the cell twice, the second copy
    /// dimmer).
    pub fn dance_actors_json(&self) -> String {
        use legaia_engine_minigames::dance::SpritePartEmit;
        let Some(g) = self.dance.as_ref() else {
            return r#"{"dancers":[],"parts":[]}"#.to_string();
        };
        let dancers = g
            .dancer_clip_frames()
            .iter()
            .zip(g.dancer_actors().iter())
            .map(|(f, a)| {
                format!(
                    concat!(
                        r#"{{"slot":{},"x":{},"y":{},"z":{},"clip":{},"rate":{},"#,
                        r#""clip_driver":{},"party_bank":{}}}"#
                    ),
                    f.slot,
                    a.pos[0],
                    a.pos[1],
                    a.pos[2],
                    f.clip_id,
                    f.clip_rate,
                    f.clip_driver,
                    f.party_bank,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let parts =
            g.sprite_part_emits()
                .iter()
                .filter_map(|f| {
                    let (x, y, shadow) = match f.emit {
                        SpritePartEmit::Shadowed { x, y, .. } => (x, y, true),
                        SpritePartEmit::Plain { x, y, .. }
                        | SpritePartEmit::Marker { x, y, .. } => (x, y, false),
                        _ => return None,
                    };
                    Some(format!(
                        r#"{{"sprite":{},"x":{x},"y":{y},"fade":{},"shadow":{shadow}}}"#,
                        f.sprite, f.fade,
                    ))
                })
                .collect::<Vec<_>>()
                .join(",");
        format!(r#"{{"dancers":[{dancers}],"parts":[{parts}]}}"#)
    }
}

impl LegaiaMinigames {
    /// The as-loaded dance overlay image (PROT 0980) - chart, scoring tables and
    /// cast tables all live in its rodata.
    fn dance_overlay(&self) -> Option<Vec<u8>> {
        overlay_image(
            &self.prot,
            &self.entries,
            legaia_minigame_formats::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32,
        )
    }

    /// Decode the dance step chart out of the dance overlay (PROT 0980).
    pub(super) fn dance_chart(&self) -> Option<legaia_minigame_formats::dance_chart::DanceChart> {
        legaia_minigame_formats::dance_chart::parse(&self.dance_overlay()?)
    }
}
