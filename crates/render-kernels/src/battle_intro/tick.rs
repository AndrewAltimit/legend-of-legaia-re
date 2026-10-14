//! The per-frame advance of a [`BattleIntro`].

use super::*;
use crate::battle_intro_styles as styles;
use crate::battle_intro_styles::IntroStyle;
use crate::battle_intro_swirl as swirl;
use crate::battle_intro_tiles as tiles;
use crate::screen_prim::ScreenPrim;

impl BattleIntro {
    /// Bring the working set up to the live transition clock and emit its
    /// frame - the call both play hosts make, once per **drawn** frame.
    ///
    /// The working set integrates per [`Self::tick`] call (every style steps
    /// its tiles / particles / strips by `frame_step` each time it is called),
    /// so a host whose draw cadence is not one-per-simulation-tick cannot call
    /// [`Self::tick`] directly: the native window draws once per redraw and a
    /// redraw can drain several world ticks (the capture redraw that lands
    /// the field frame is the slow one), which left its shatter two ticks
    /// behind the browser page's, whose step runs one call per tick. Here the
    /// emitter steps exactly once per clock unit the entity has advanced since
    /// the last call - from the entity's first frame on the first call, so a
    /// host that arms late catches up - and a call that finds the clock where
    /// it was re-emits the cached frame without stepping.
    pub fn advance_to(&mut self, elapsed: i16) -> IntroFrame {
        if let Some((last, frame)) = self.last_frame.as_ref()
            && *last == elapsed
        {
            return frame.clone();
        }
        let first = match self.last_frame.as_ref() {
            Some((last, _)) if *last < elapsed => last.wrapping_add(1),
            // A clock that moved backwards: one step at the new value.
            Some(_) => elapsed,
            // A fresh emitter: from the entity's first frame.
            None => 0.min(elapsed),
        };
        let mut e = first;
        let mut frame = self.tick(e, 1);
        while e < elapsed {
            e = e.wrapping_add(1);
            frame = self.tick(e, 1);
        }
        self.last_frame = Some((elapsed, frame.clone()));
        frame
    }

    /// Advance one frame and emit.
    ///
    /// `elapsed` is the live transition entity's `+0x1A`; the emitter adopts it
    /// rather than counting for itself, so a host that stalls or repeats a
    /// simulation tick cannot desynchronise the visuals from the handoff.
    /// `frame_step` is retail's per-frame display-frame delta (`1` at the
    /// steady NTSC cadence).
    ///
    /// `prims[0]` is always [`backdrop_prim`] - the frame the style composes
    /// onto - including on frames the style itself draws nothing. Its OT
    /// bucket is the farthest one, so its position in submission order is
    /// immaterial to the draw order and it is emitted first only to keep the
    /// fade quad the list's last element.
    pub fn tick(&mut self, elapsed: i16, frame_step: u8) -> IntroFrame {
        self.clock = elapsed;
        let mut out = IntroFrame::default();
        out.prims.push(backdrop_prim());
        // The curtain's display trail: the previous frames' pixels, decayed by
        // the per-frame wash, drawn under this frame's live strips - retail
        // gets it by never clearing the display buffer. Empty (no quads) until
        // the first capture landing has seeded the model, which is also
        // the frame the capture itself lands.
        if self.style == IntroStyle::Curtain {
            self.curtain_trail_prims(&mut out.prims);
        }

        match &mut self.set {
            WorkingSet::Particles { grid, style } => {
                // Same first-frame gate as the tiles below: retail's first
                // frame projects through the field camera's stale view
                // matrix; from frame two the view is identity rotation +
                // zero translation (pinned live), which is what
                // `project_intro_corner` hard-codes.
                let emit = self.clock != 0;
                // `FUN_801CFDA0` (and only it - `stamp_field_16` is its
                // marker) washes the screen near-black on every frame after
                // the first; the confetti reconstructs the frame over it
                // until the delays expire.
                if emit && style.stamp_field_16 {
                    out.prims.push(wash_prim(PARTICLE_WASH_RGB));
                }
                // The spin-up tail's ring phase uses the pre-increment clock.
                let ring_phase = i32::from(self.clock) * SPINUP_RING_PHASE_STEP;
                let drawn = emit_particle_field(
                    grid,
                    style,
                    &mut self.clock,
                    frame_step,
                    emit,
                    &mut out.prims,
                );
                // `FUN_801D0370`'s tail (`spin_up` is its marker): the
                // expanding ring behind the confetti.
                let ring = emit && style.spin_up && emit_spinup_ring(ring_phase, &mut out.prims);
                out.style_drawn = drawn || ring;
            }
            WorkingSet::Tiles(grid) => {
                // Retail's first shatter frame projects through the field
                // camera's stale view matrix (the transition setup rewrites
                // the scratch matrix only after it), which puts every tile
                // behind the near plane - so frame one draws no tiles, and
                // `not_first_frame` (`_DAT_8007B6CC`) is the same signal.
                // From frame two the view is identity rotation + zero
                // translation (pinned live), which is what `emit_tile`
                // hard-codes.
                let emit = self.clock != 0;
                let before = out.prims.len();
                tiles::tick_tile_grid_emit(grid, &mut self.clock, frame_step, |rec| {
                    if emit {
                        emit_tile(rec, &mut out.prims);
                    }
                });
                out.style_drawn = out.prims.len() > before;
            }
            WorkingSet::Curtain(table) => {
                let tick = styles::tick_curtain(&mut table.0, &mut self.clock, frame_step);
                // The two passes go to two different places. Retail installs a
                // draw area at VRAM (320, 0) for the column pass and restores
                // the back buffer before the row pass, so only the rows are on
                // screen; the columns render the intermediate the rows sample.
                // See the module docs for the OT-bucket evidence.
                self.pending_columns.clear();
                self.pending_rows.clear();
                out.prims.reserve(tick.quads.len());
                for q in &tick.quads {
                    if q.desc_index == styles::CURTAIN_COL_DESC {
                        self.pending_columns.push(q.quad);
                    } else {
                        self.pending_rows.push(q.quad);
                        out.prims
                            .push(ScreenPrim::Textured(intro_quad_to_screen(&q.quad)));
                    }
                }
                out.style_drawn = !tick.quads.is_empty();
            }
            WorkingSet::Swirl { mesh, prev_clock } => {
                // Latch each band's view depth before the tick integrates it:
                // retail stages the per-band translation and *then* advances
                // the scalar, so the draw uses the frame-entry value.
                let emit = self.clock != 0;
                let zs: [i32; swirl::BANDS] = std::array::from_fn(|i| mesh.bands[i].angle);
                let tick = swirl::tick_swirl(mesh, &mut self.clock, frame_step, prev_clock);
                if tick.late_wash {
                    out.prims.push(wash_prim(swirl::LATE_WASH_RGB));
                }
                if emit {
                    for d in &tick.draws {
                        let band = d.first_vertex / swirl::VERTS_PER_BAND;
                        out.style_drawn |= emit_swirl_band(mesh, zs[band], d, &mut out.prims);
                    }
                }
            }
        }

        out.fade = styles::intro_fade(
            self.style,
            self.sub_style,
            i32::from(elapsed),
            self.total_duration,
        );
        if let Some(f) = out.fade {
            out.prims.push(fade_quad(&f));
        }
        out
    }
}
