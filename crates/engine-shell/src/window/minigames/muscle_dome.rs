//! The Muscle Dome on the play window: the time meter, the arena upload, and
//! the contest hub (its tick, asset load and sprite draws).

use super::*;

impl PlayWindowApp {
    /// Advance the Muscle Dome contest's round **time meter** one frame.
    ///
    /// Retail runs the meter from the arena's per-frame driver with the frame
    /// delta from scratchpad `0x1F800393`; the engine ticks a fixed one per
    /// frame. No-op outside a contest.
    pub(in crate::window) fn tick_muscle_time_meter(&mut self) {
        if let Some(s) = self.session.host.world.minigames.muscle_dome.as_mut() {
            s.tick_time_meter(1);
        }
    }
}

impl PlayWindowApp {
    /// Pose the Muscle Dome's 3D arena surface for this frame and put it on
    /// the GPU.
    ///
    /// The seat, the choreography, the camera and the buffers are the
    /// engine's (`legaia_engine_core::muscle_dome_scene::MuscleDomeSurface::
    /// frame`, the call the browser play page makes too); this host uploads
    /// the dome VRAM on a generation change and the posed mesh every frame,
    /// and the redraw draws them under `DomeCamera::vp_raw` (the battle
    /// camera script's pose for the leg). Drops the GPU
    /// copy whenever no dome session is on screen.
    pub(in crate::window) fn refresh_muscle_dome_gpu(&mut self) {
        let world = &self.session.host.world;
        let live = world.mode == SceneMode::MuscleDome;
        let index = self.session.host.index.clone();
        let read = |i: usize| index.entry_bytes(i as u32).ok();
        let session = if live {
            world.minigames.muscle_dome.as_ref()
        } else {
            None
        };
        let contest = world.minigames.muscle_contest.as_ref();
        self.muscle_surface
            .set_camera_option(world.toggles.battle_camera as u8);
        let generation_before = self.muscle_surface.generation();
        if self
            .muscle_surface
            .frame_at(
                read,
                session,
                contest,
                MUSCLE_DOME_CHAR_SLOT,
                world.clock.sim_ticks,
            )
            .is_none()
        {
            self.muscle_gpu = None;
            return;
        }
        let (Some(r), Some(scene)) = (self.win.renderer.as_ref(), self.muscle_surface.scene())
        else {
            return;
        };
        let generation = self.muscle_surface.generation();
        let (sw, sh) = r.surface_size();
        let (_, aspect) = super::super::geometry::scene_viewport_for(sw, sh);
        let mvp = Mat4::from_cols_array(&scene.camera.vp_raw(aspect));
        let normals = vec![[0.0f32; 3]; scene.positions.len()];
        let textured = r
            .upload_vram_mesh(
                &scene.positions,
                &scene.uvs,
                &scene.cba_tsb,
                &normals,
                &scene.colors,
                &scene.textured_indices,
            )
            .map_err(|e| log::warn!("muscle dome: textured upload failed: {e:#}"))
            .ok();
        let fill: Vec<[u8; 3]> = scene
            .flat_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        let untextured = (!scene.untextured_indices.is_empty())
            .then(|| {
                r.upload_color_mesh(&scene.positions, &fill, &scene.untextured_indices)
                    .map_err(|e| log::warn!("muscle dome: untextured upload failed: {e:#}"))
                    .ok()
            })
            .flatten();
        let stale = generation != generation_before
            || self
                .muscle_gpu
                .as_ref()
                .is_none_or(|g| g.generation != generation);
        let vram = if stale {
            match self.muscle_surface.vram().map(|v| r.upload_vram(v)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    log::warn!("muscle dome: vram upload failed: {e:#}");
                    return;
                }
                None => return,
            }
        } else {
            match self.muscle_gpu.take() {
                Some(g) => g.vram,
                None => return,
            }
        };
        self.muscle_gpu = Some(BakaDuelGpu {
            generation,
            vram,
            textured,
            untextured,
            mvp,
        });
    }
}

impl PlayWindowApp {
    /// Advance the Muscle Dome hub-screen timers one frame, off the world's
    /// own leg / contest edges (so both the pad path and the `M` abort arm
    /// them):
    ///
    /// * a leg opening on a **fresh** contest arms the "Welcome to the
    ///   Muscle Dome!" intro card, then the ROUND banner;
    /// * a leg opening mid-ladder arms the ROUND banner alone;
    /// * a leg **closing** arms the between-legs INTERVAL + score-tally
    ///   screen, but only when the shared cadence rule
    ///   ([`legaia_engine_core::muscle_dome::leg_boundary_raises_interval`])
    ///   says retail's hub reaches state `0x0A` - a survived leg with the
    ///   course not yet exhausted. A lost, run-from or final leg settles
    ///   instead and raises nothing.
    ///
    /// A **turn** boundary arms nothing here, and cannot: the leg stays open
    /// across turns, so the closing edge never fires. Retail agrees - the turn
    /// boundary is `ctx[6] = 0x14` inside the battle SM, with the arena hub not
    /// running ([`MusclePhase::ends_turn`]).
    ///
    /// Each screen runs retail's own fade / hold envelope
    /// ([`legaia_engine_core::muscle_dome::HubScreen`]) rather than a frame
    /// count this host picked: fade in at the arm's rate, hold at the
    /// measured literal, fade out - and the two card holds end early on a
    /// pad press, the way `FUN_801CF870`'s `& 0xF4` test lets them.
    ///
    /// [`MusclePhase::ends_turn`]: legaia_engine_core::muscle_dome::MusclePhase::ends_turn
    pub(in crate::window) fn tick_muscle_hub(&mut self) {
        // A dome leg the player WALKED into (the mode-24 door warp, drained
        // by the shared scene host) already carries its contest: the warp
        // arm opens it off the unlock flags (`DomeContest::from_overlay`)
        // and seats the lead fighter's costs, so `open_muscle_contest` is a
        // no-op there. It stays as the guard for a leg opened any other way;
        // the hub page's assets are this window's own and load here on every
        // entry, as the browser play page does.
        if self.session.host.world.minigames.muscle_dome.is_some() {
            self.open_muscle_contest();
            self.load_muscle_hub_assets();
        }
        // The screen timers are the world's own (`World::tick_muscle_hub`,
        // run by the shared scene host every tick, which also hands the next
        // leg its fight); this window only sounds what they fired.
        let frame = self.session.host.world.take_muscle_hub_sounds();
        // The first visit's two announcer lines (`FUN_8003D53C` at arms 0 and
        // 0x15), through the same XA path the battle clips use.
        if let Some(c) = frame.xa
            && let Some(bgm) = self.session.bgm.as_mut()
        {
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
                if fired { "playing" } else { "not staged" }
            );
        }
        // Each drained tally lane keys a voice directly, with no cue id in
        // sight (`FUN_801D1288` builds the whole attr set); the director's
        // explicit key-on is the only path that takes it.
        if let Some(bgm) = self.session.bgm.as_mut() {
            for cue in frame.voice_cues {
                bgm.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                    cue.voice,
                    cue.vab_program_tone,
                    cue.note_and_fine,
                    cue.volume,
                ));
            }
        }
    }

    /// Load the Muscle Dome hub-screen assets once: the two hub page TIMs
    /// (the LZS payload of the dome's own data file, extraction 1220 /
    /// `other6.lzs` slot 0 - the pages retail uploads at VRAM
    /// (320,0)/(320,256)) baked to RGBA per referenced 16-colour sub-palette
    /// and stacked into one sprite atlas, plus the PROT 0977 sprite
    /// descriptor table the shared emitters place every hub screen from.
    /// No-op when already loaded; logs and leaves `muscle_hub` empty when the
    /// disc or renderer is absent.
    /// Stage the dome contest (`(course, round)` off the arena overlay and
    /// the party's story flags) unless one is already open.
    ///
    /// The shared mode-24 door warp now opens the contest itself (the warp
    /// operand names an overlay, not a ladder position, so it stages the
    /// rung off the unlock flags), which makes this a no-op on a door entry;
    /// it covers the `M` launcher and any leg opened without the warp.
    pub(in crate::window) fn open_muscle_contest(&mut self) {
        if self.session.host.world.minigames.muscle_contest.is_some() {
            return;
        }
        let Ok(raw) =
            self.session.host.index.entry_bytes_extended(
                legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32,
            )
        else {
            return;
        };
        let flags = self.session.host.world.muscle_contest_flags();
        self.session.host.world.minigames.muscle_contest =
            legaia_engine_core::muscle_dome::DomeContest::from_overlay(&raw, &flags);
    }

    pub(in crate::window) fn load_muscle_hub_assets(&mut self) {
        use legaia_engine_render::other_game_hud as hud;
        if self.muscle_hub.is_some() {
            return;
        }
        use legaia_asset::muscle_dome::HUB_CONTAINER_PROT_INDEX;
        let Some(renderer) = self.win.renderer.as_ref() else {
            return;
        };
        let container = match self
            .session
            .host
            .index
            .entry_bytes_extended(HUB_CONTAINER_PROT_INDEX)
        {
            Ok(b) => b,
            Err(e) => {
                log::warn!("muscle hub: PROT {HUB_CONTAINER_PROT_INDEX} read failed: {e:#}");
                return;
            }
        };
        // Section 0 = `[12-byte header][TIM][TIM]`, decoded with the arena's
        // upload STP applied (the VRAM CLUT words the blend classes key on).
        let Some((tim0, tim1)) = legaia_asset::muscle_dome::hub_page_tims(&container) else {
            log::warn!("muscle hub: page TIMs did not decode from the dome container");
            return;
        };
        let arena_raw =
            match self.session.host.index.entry_bytes_extended(
                legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32,
            ) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("muscle hub: PROT 0977 read failed: {e:#}");
                    return;
                }
            };
        let table = hud::parse_sprite_table(&arena_raw);
        if table.is_empty() {
            log::warn!("muscle hub: PROT 0977 sprite table did not parse");
            return;
        }
        // Every (page, sub-palette) pair the hub draws can reach: each
        // record's own CLUT, its variant-2 sibling (`clut + 1` - the emitter
        // bump), and the digit record's four tally palettes.
        let mut wanted = std::collections::BTreeSet::new();
        for (i, rec) in table.iter().enumerate() {
            let sheet = u8::from(rec.tpage & 0x10 != 0);
            let pal = (rec.clut & 0x3F) as u8;
            wanted.insert((sheet, pal));
            wanted.insert((sheet, pal + 1));
            if i == hud::DIGIT_SPRITE_INDEX {
                for p in 0..4u8 {
                    wanted.insert((sheet, pal + p));
                }
            }
        }
        let tims = [&tim0, &tim1];
        let atlas_w = tims.iter().map(|t| t.pixel_width()).max().unwrap_or(0) as u32;
        if atlas_w == 0 {
            return;
        }
        // The two ringside stills share the atlas: each is a 320-wide sheet
        // (the VRAM region `(384, 0)` the battle end's loader fills).
        let atlas_w = atlas_w.max(legaia_asset::ringside_still::WIDTH as u32);
        let mut blocks: Vec<(u8, u8, u32)> = Vec::new();
        let mut rgba: Vec<u8> = Vec::new();
        let mut atlas_h = 0u32;
        for (sheet, pal) in wanted {
            let tim = tims[sheet as usize];
            // A palette past the sheet's CLUT bank simply isn't baked; the
            // draw that would sample it is skipped at build time.
            let Ok(px) = legaia_tim::decode_rgba8(tim, pal as usize) else {
                continue;
            };
            let (tw, th) = (tim.pixel_width() as u32, tim.pixel_height() as u32);
            for row in 0..th as usize {
                let src = &px[row * tw as usize * 4..(row + 1) * tw as usize * 4];
                rgba.extend_from_slice(src);
                rgba.resize(rgba.len() + ((atlas_w - tw) * 4) as usize, 0);
            }
            blocks.push((sheet, pal, atlas_h));
            atlas_h += th;
        }
        if blocks.is_empty() {
            log::warn!("muscle hub: no page/palette block decoded");
            return;
        }
        // A small white block: the texel the untextured backdrop shade draws
        // with.
        let white_y = atlas_h;
        for _ in 0..4 {
            rgba.extend(std::iter::repeat_n(0xFF, 4 * 4));
            rgba.resize(rgba.len() + ((atlas_w - 4) * 4) as usize, 0);
        }
        atlas_h += 4;
        let mut stills: Vec<(u32, u32)> = Vec::new();
        for variant in 0..2u32 {
            let index = legaia_asset::ringside_still::PROT_INDEX_DEFAULT + variant;
            let Some(sheet) = self
                .session
                .host
                .index
                .entry_bytes_extended(index)
                .ok()
                .and_then(|b| legaia_engine_render::ringside_backdrop::still_sheet_rgba(&b))
            else {
                log::warn!("muscle hub: ringside still {index} did not decode");
                continue;
            };
            let sw = legaia_asset::ringside_still::WIDTH as u32;
            for row in sheet.chunks_exact(sw as usize * 4) {
                rgba.extend_from_slice(row);
                rgba.resize(rgba.len() + ((atlas_w - sw) * 4) as usize, 0);
            }
            stills.push((variant, atlas_h));
            atlas_h += legaia_asset::ringside_still::HEIGHT as u32;
        }
        match renderer.upload_sprite_atlas(&rgba, atlas_w, atlas_h) {
            Ok(atlas) => {
                log::info!(
                    "muscle hub: atlas uploaded ({atlas_w}x{atlas_h}, {} page/palette blocks)",
                    blocks.len()
                );
                self.muscle_hub = Some(MuscleHubAssets {
                    blocks,
                    palette_stp: legaia_engine_render::ringside_backdrop::HubPaletteStp::from_tims(
                        &tim0, &tim1,
                    ),
                    table,
                    atlas,
                    stills,
                    white_y,
                });
            }
            Err(e) => log::warn!("muscle hub: atlas upload skipped: {e:#}"),
        }
    }

    /// The Muscle Dome hub screens as retail-placed sprite draws, through the
    /// shared `engine-ui` emitters both hosts draw with
    /// ([`legaia_engine_render::other_game_hud::hub_screen_quads`] /
    /// [`legaia_engine_render::other_game_hud::score_tally_quads`] - the browser
    /// dome page reaches the same functions via
    /// `minigames_muscle::muscle_hub_quads_json`): the hub's first visit
    /// (`ringside_backdrop::first_visit_hub_draw` - wall, shade, intro strip,
    /// title zoom, course card, ROUND card) over a fresh contest's first leg,
    /// the INTERVAL heading + six-row score tally between legs. Every quad's extent and screen seat come out of the
    /// PROT 0977 descriptor table and recovered draw lists; the host places
    /// nothing itself.
    ///
    /// The tally's six values are the contest's own rows - the four
    /// `LegScoreRows` lanes, then the running tally and the coin bank they
    /// settle into - the same model row set the browser page feeds the same
    /// builder.
    ///
    /// Semi-transparent packets ride the returned [`OverlayBlendSpan`]s: the
    /// first visit's shade through the subtractive (`ABR 2`) sprite
    /// pipeline, between the wall tiles and the screens drawn over them, and
    /// a hub quad through its tpage's ABR wherever its palette carries STP
    /// (`HubPaletteStp::quad_abr` over the CLUTs as the arena uploads them -
    /// STP-set on every non-zero entry, so an STP-free palette, which draws
    /// opaque as retail's GPU draws it, is one that is all zero).
    ///
    /// One disclosed stand-in: a packet's vertical two-stop colour gradient
    /// flattens to the stops' mean (the sprite pipeline is one-colour).
    ///
    /// [`OverlayBlendSpan`]: legaia_engine_render::OverlayBlendSpan
    pub(in crate::window) fn muscle_hub_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> (
        Vec<legaia_engine_render::SpriteDraw>,
        Vec<legaia_engine_render::OverlayBlendSpan>,
    ) {
        use legaia_engine_render::other_game_hud as hud;
        let Some(assets) = self.muscle_hub.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let world = &self.session.host.world;
        // A leg is open (the first visit / the leg-open ROUND card draw over
        // it); otherwise the arena hub is between legs, or the contest has
        // handed the field back, and the INTERVAL + still screens are the
        // hub's own.
        let in_dome = world.mode == SceneMode::MuscleDome && world.minigames.muscle_dome.is_some();
        // The retail emitters mutate the shared table (variant write-back),
        // so run them over a per-frame copy of the pristine parse.
        let mut table = assets.table.clone();
        // The brightness argument is the screen's own fade counter, which
        // clamps at `HUB_FADE_FULL` (0x80) - the emitter's neutral, since it
        // scales each stored channel by `c * brightness / 256`. The host used
        // to pass 0x100, which drew every hub screen at twice retail's
        // brightness.
        let mut quads: Vec<hud::HudQuad> = Vec::new();
        // The first visit's shade and the screens drawn over it: the shade
        // sits between the wall tiles (`quads`) and these.
        let mut shade: Option<legaia_engine_render::ringside_backdrop::BackdropShade> = None;
        let mut front: Vec<hud::HudQuad> = Vec::new();
        if in_dome {
            // A first visit's frame: the brick wall + shade (the backdrop
            // emitter's latch-0 arm) behind the arm's screens, all composed
            // by the shared kernel the play page draws with.
            if let Some(hub) = self.session.host.world.minigames.muscle_hub.first_visit {
                let f = hub.frame();
                let levels = legaia_engine_render::ringside_backdrop::FirstVisitLevels {
                    backdrop: f.backdrop,
                    intro: f.intro,
                    title_scale: f.title_scale,
                    course_card: f.course_card,
                    round_card: f.round_card,
                };
                let (course, round) = world
                    .minigames
                    .muscle_contest
                    .as_ref()
                    .map_or((0, 1), |c| (c.course() as i32, c.round() as i32 + 1));
                let d = legaia_engine_render::ringside_backdrop::first_visit_hub_draw(
                    &mut table, &levels, course, round,
                );
                quads.extend(d.tiles);
                shade = d.shade;
                front.extend(d.hud);
            } else if let Some((round, banner)) =
                self.session.host.world.minigames.muscle_hub.round_banner
            {
                quads.extend(hud::hub_screen_quads(
                    &mut table,
                    &hud::round_banner_draws(round),
                    banner.brightness(),
                ));
            }
        } else if let Some(interval) = self.session.host.world.minigames.muscle_hub.interval {
            let bright = interval.brightness();
            quads.extend(hud::hub_screen_quads(
                &mut table,
                hud::HUB_INTERVAL_HEADING,
                bright,
            ));
            // The six rows are the roll's own, not the settled totals: three
            // recovery lanes counting down, the HP they count into, the score
            // lane counting down and the coin tally counting up. With no roll
            // armed the screen draws the settled values, which is where the
            // roll ends anyway.
            let (values, row_bright) =
                match self.session.host.world.minigames.muscle_hub.tally.as_ref() {
                    Some((ramp, tally)) => (ramp.row_values(*tally), ramp.row_brightness(bright)),
                    None => {
                        let (rows, tally) = world
                            .minigames
                            .muscle_contest
                            .as_ref()
                            .map_or((Default::default(), 0), |c| (c.rows(), c.tally()));
                        ([0, 0, 0, rows.hp_restore(), 0, tally], [bright; 6])
                    }
                };
            quads.extend(hud::score_tally_quads(&mut table, values, row_bright));
        }
        // The re-entered hub's ROUND card (arms 0x15 / 0x16) over the still.
        if !in_dome
            && self
                .session
                .host
                .world
                .minigames
                .muscle_hub
                .interval
                .is_none()
            && let Some(card) = self
                .session
                .host
                .world
                .minigames
                .muscle_hub
                .backdrop
                .and_then(|b| b.card_brightness())
        {
            let round = world
                .minigames
                .muscle_contest
                .as_ref()
                .map_or(1, |c| c.round() as i32 + 1);
            quads.extend(hud::hub_screen_quads(
                &mut table,
                &hud::round_banner_draws(round),
                card,
            ));
        }
        let mut out: Vec<legaia_engine_render::SpriteDraw> = Vec::new();
        // The backdrop goes first: retail links the still's two packets at
        // the ordering table's far end (`OT + 0xFA0`), behind every sprite.
        if !in_dome
            && let Some(b) = self
                .session
                .host
                .world
                .minigames
                .muscle_hub
                .backdrop
                .filter(|b| b.visible())
            && let Some(&(_, still_y)) = assets.stills.iter().find(|(v, _)| *v == b.variant())
        {
            use legaia_engine_render::ringside_backdrop as rb;
            for q in rb::ringside_still_quads(b.level()) {
                let Some(d) = rb::StillDraw::from_quad(&q) else {
                    continue;
                };
                // A flat packet colour: texture modulation `texel * c / 128`.
                let c = f32::from(d.level) / 128.0;
                out.push(legaia_engine_render::SpriteDraw {
                    dst: d.dst,
                    src: (d.src.0, still_y + d.src.1, d.src.2, d.src.3),
                    color: [c, c, c, 1.0],
                });
            }
        }
        if quads.is_empty() && out.is_empty() && front.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let shade_at = quads.len();
        quads.extend(front);
        let mut blend = Vec::new();
        let push_shade =
            |out: &mut Vec<legaia_engine_render::SpriteDraw>,
             blend: &mut Vec<legaia_engine_render::OverlayBlendSpan>,
             sh: &legaia_engine_render::ringside_backdrop::BackdropShade| {
                let rows = shade_row_draws(sh, assets.white_y);
                blend.push(legaia_engine_render::OverlayBlendSpan {
                    start: out.len() as u32,
                    count: rows.len() as u32,
                    abr: sh.abr,
                });
                out.extend(rows);
            };
        for (i, q) in quads.iter().enumerate() {
            if i == shade_at
                && let Some(sh) = shade
            {
                push_shade(&mut out, &mut blend, &sh);
            }
            let sheet = u8::from(q.tpage & 0x10 != 0);
            let pal = (q.clut & 0x3F) as u8;
            let Some(&(_, _, block_y)) = assets
                .blocks
                .iter()
                .find(|(s, p, _)| *s == sheet && *p == pal)
            else {
                continue;
            };
            // A semi packet blends only through STP texels. The arena
            // uploads the hub CLUTs STP-set (`hub_page_tims`), so the
            // variant-2 knockout palettes blend subtractively (`B - F`)
            // under the face the variant-1 pass adds over them.
            let semi_abr = assets.palette_stp.quad_abr(q);
            let dw = (q.xy[1].0 as i32 - q.xy[0].0 as i32 + 1).max(0) as u32;
            let dh = (q.xy[2].1 as i32 - q.xy[0].1 as i32 + 1).max(0) as u32;
            let sw = (q.uv[1].0 as i32 - q.uv[0].0 as i32 + 1).max(0) as u32;
            let sh = (q.uv[2].1 as i32 - q.uv[0].1 as i32 + 1).max(0) as u32;
            if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
                continue;
            }
            // PSX texture modulation is `texel * c / 128`; the per-vertex
            // colours are a vertical two-stop gradient, flattened here to
            // the stops' mean.
            let tint = |k: usize| (q.rgb[0][k] as f32 + q.rgb[2][k] as f32) / 2.0 / 128.0;
            // The retail display is the 320x240 frame and the GPU clips to
            // it; the first visit's wall tiles run past it (three 128-wide
            // columns, two 128-high rows), so clip here - the texel window
            // shrinks with the same ratio - or the stage transform carries
            // the overhang onto the window beside the frame.
            let (dx, dy) = (q.xy[0].0 as i64, q.xy[0].1 as i64);
            let (x0, x1) = (dx.max(0), (dx + dw as i64).min(320));
            let (y0, y1) = (dy.max(0), (dy + dh as i64).min(240));
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let sx = q.uv[0].0 as i64 + (x0 - dx) * sw as i64 / dw as i64;
            let sy = q.uv[0].1 as i64 + (y0 - dy) * sh as i64 / dh as i64;
            let csw = ((x1 - x0) * sw as i64 / dw as i64).max(1) as u32;
            let csh = ((y1 - y0) * sh as i64 / dh as i64).max(1) as u32;
            if let Some(abr) = semi_abr {
                blend.push(legaia_engine_render::OverlayBlendSpan {
                    start: out.len() as u32,
                    count: 1,
                    abr,
                });
            }
            out.push(legaia_engine_render::SpriteDraw {
                dst: (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32),
                src: (sx as u32, block_y + sy as u32, csw, csh),
                color: [tint(0), tint(1), tint(2), 1.0],
            });
        }
        if shade_at >= quads.len()
            && let Some(sh) = shade
        {
            push_shade(&mut out, &mut blend, &sh);
        }
        // The quads sit in the retail 320x240 frame; map them through the
        // same stage transform every minigame chrome layer uses.
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::scale_stage_text_draws(&mut out, stage_origin, stage_scale);
        (out, blend)
    }
}

/// The first visit's backdrop shade (`FUN_801D1610`, a subtractive `B - F`
/// Gouraud quad, `0x64` at the top fading to `0` at the bottom) as sprite
/// draws over the atlas's white block, for the caller to put in an `ABR 2`
/// [`legaia_engine_render::OverlayBlendSpan`].
///
/// The sprite pipeline carries one colour per draw, and the quad's colour
/// only varies with `y`, so one draw per display row reproduces the ramp:
/// row `y` subtracts `top + (bottom - top) * y / h`, the Gouraud value at
/// that row. A row that subtracts nothing is skipped.
fn shade_row_draws(
    sh: &legaia_engine_render::ringside_backdrop::BackdropShade,
    white_y: u32,
) -> Vec<legaia_engine_render::SpriteDraw> {
    let (x0, y0) = (i32::from(sh.xy[0].0), i32::from(sh.xy[0].1));
    let w = (i32::from(sh.xy[1].0) - x0).max(0) as u32;
    let h = (i32::from(sh.xy[2].1) - y0).max(0);
    let top = i32::from(sh.rgb[0][0]);
    let bottom = i32::from(sh.rgb[2][0]);
    let mut out = Vec::new();
    for row in 0..h {
        let f = top + (bottom - top) * row / h;
        if f <= 0 {
            continue;
        }
        let f = f as f32 / 255.0;
        out.push(legaia_engine_render::SpriteDraw {
            dst: (x0, y0 + row, w, 1),
            src: (0, white_y, 1, 1),
            color: [f, f, f, 1.0],
        });
    }
    out
}
