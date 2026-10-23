//! In-world minigames on the play page: the draw + input side of the
//! sessions the shared scene host installs.
//!
//! Entry is already host-agnostic: the field-VM op-`0x3E` `op0 >= 100` arm
//! (reached from a town door's or an overworld signboard's own interaction
//! record) publishes `World::minigames.pending_warp`, and `SceneHost::tick`
//! drains it into
//! `enter_fishing / enter_slot / enter_baka / enter_muscle / enter_dance`
//! on both hosts. Fishing has its own module ([`crate::play_fishing`]); this
//! one is the surface for the other four `SceneMode`s, which the page used
//! to enter with a frozen field and no UI.
//!
//! # Shape
//!
//! Three things happen here and nowhere else on the page:
//!
//! 1. **Presentation assets.** The standalone minigames page already decodes
//!    every one of the four games' art off the disc - the slot machine's
//!    scene graph + art pack, the dome's arena / fighter / monster meshes and
//!    hub pages, the Baka roster meshes + stage, the dance hall + dancers -
//!    through [`legaia_web_minigames::LegaiaMinigames`]. Rather than port those
//!    builders a second time against the engine's `ProtIndex`, the play page
//!    hands that same type a **compact PROT image**: the minigame entries
//!    copied out of the loaded archive behind a synthetic TOC of the retail
//!    shape ([`compact_prot_image`]), so every `*_rgba` / `*_positions` /
//!    `*_json` export the standalone page calls works unchanged here. The
//!    image is built once, on the first minigame entry.
//! 2. **Session read-outs.** The rules engines run in the engine's own
//!    `World` (`tick_slot_machine` / `tick_baka_fighter` / `tick_muscle_dome`
//!    / `tick_dance`, off the pad word the page already routes through
//!    `set_pad`), so the page adds no input path. The per-game state JSON the
//!    standalone renderers read (`slot_state_json` ...) is rebuilt here over
//!    the *world's* session instead of the standalone's private one.
//! 3. **The text HUD**, in two parts that never share a surface.
//!    [`LegaiaRuntime::minigame_overlay_draws`] carries the text that belongs
//!    to the game picture - the retail-placed digit strips, chrome labels and
//!    frame rows - stage-scaled onto the overlay canvas.
//!    [`LegaiaRuntime::play_mg_caption_json`] carries the engine's **status
//!    rows** (`legaia_engine_minigame_scenes::minigame_status`: the port's readout of
//!    the rules state plus the key hints) as strings, which the page shows in
//!    the caption strip under the canvas. They are never glyph quads in the
//!    frame: the page draws a game's screen primitives over its text layer,
//!    so a row placed there is covered by the game's own chrome. The native
//!    window reads the same model and shows it in its title bar. A disc
//!    whose art does not decode still tells the player which phase the
//!    session is in, through the caption.
//!
//! Exit is the engine's: Start leaves any minigame (`World::poll_minigame_escape`)
//! and each game's own `exit_*` runs the bookkeeping - the slot cash-out into
//! the coin bank, the Baka winnings through the mode-24 return warp, the dome
//! contest's settle. This module only observes the edge and tells the page to
//! put the field VRAM back.
//!
//! Per-game exports live in the sibling modules
//! [`crate::play_minigame_slots`] and [`crate::play_minigame_arena`].

use legaia_engine_scene::scene::ProtIndex;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};
use legaia_engine_world_views::minigame_caption::WorldMinigameCaption;

use crate::runtime::LegaiaRuntime;
use legaia_web_minigames::LegaiaMinigames;

/// Which in-world minigame owns the screen: the engine's
/// [`StatusGame`](legaia_engine_minigame_scenes::minigame_status::StatusGame),
/// whose `of_mode` / `label` the page used to carry a copy of.
pub(crate) use legaia_engine_minigame_scenes::minigame_status::StatusGame as ActiveGame;

/// Presentation state for the in-world minigame screens.
#[derive(Default)]
pub(crate) struct MinigameUi {
    /// The standalone page's presentation bundle over the compact PROT image
    /// - built on the first minigame entry, kept for the session.
    pub(crate) art: Option<Box<LegaiaMinigames>>,
    /// The build was attempted and failed (no disc, or the image did not
    /// parse); don't retry every frame.
    art_failed: bool,
    /// The game that owned the screen last tick (edge detection).
    pub(crate) game: Option<ActiveGame>,
    /// Bumped on every entry so the page rebuilds its scene.
    pub(crate) generation: u32,
    /// Muscle Dome hub-screen timers + staged opponent.
    pub(crate) muscle: crate::play_minigame_arena::MuscleUi,
    /// Baka Fighter's staged opponent.
    pub(crate) baka: crate::play_minigame_arena::BakaUi,
    /// Slot machine payout edges.
    pub(crate) slot: crate::play_minigame_slots::SlotUi,
    /// A minigame that uploaded its own VRAM just left: the page must put
    /// the field texture back. Drained by `play_mg_take_vram_restore`.
    pub(crate) vram_restore_pending: bool,
    /// The Baka duel's 3D surface - the engine cache the native window
    /// drives too (`legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface`).
    pub(crate) baka_surface: legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface,
    /// The dance floor's bodies - the engine cache the native window drives
    /// too (`legaia_engine_minigame_scenes::dance_cast_scene::DanceCastSurface`) - and
    /// the hall origin its positions are re-based on for this page.
    pub(crate) dance_surface: legaia_engine_minigame_scenes::dance_cast_scene::DanceCastSurface,
    pub(crate) dance_origin: (f32, f32, f32),
    /// The Muscle Dome's 3D arena surface - the engine cache the native
    /// window drives too (`legaia_engine_minigame_scenes::muscle_dome_scene`).
    pub(crate) muscle_surface: legaia_engine_minigame_scenes::muscle_dome_scene::MuscleDomeSurface,
    /// The fishing pond surface - the engine cache the native window drives
    /// too (`legaia_engine_minigame_scenes::fishing_scene`).
    pub(crate) fishing_surface: legaia_engine_minigame_scenes::fishing_scene::FishingSurface,
}

/// The PROT entries the standalone presentation bundle reads, in extraction
/// index space: the player battle files + monster archive (dome bodies),
/// the SFX banks the dome / duel cues resolve through, `etim`, the party
/// field pack (Noa's dance atlas), the battle overlay (dome tables), the five
/// minigame overlays, and the `1198..=1231` art/scene band (slot art + SFX,
/// Baka HUD/stage/roster packs, dome hub + arena, the dance hall scene block
/// + its art, chart SFX and choreography).
const COMPACT_PROT_ENTRIES: &[std::ops::RangeInclusive<u32>] = &[
    863..=870,
    874..=874,
    876..=876,
    889..=889,
    898..=898,
    972..=972,
    975..=977,
    980..=980,
    1198..=1231,
];

/// PROT sector size.
const SECTOR: usize = 0x800;

/// Build a **compact PROT.DAT image** out of the loaded archive: a header +
/// TOC of the retail shape (`[pad][file_num - 1][header_sectors]`, then one
/// start LBA per entry with entry `p` at `toc[p + 2]`), followed by the
/// wanted entries' bytes sector-aligned. Every other entry is one zero
/// sector, so the TOC stays monotonic, every index keeps its number, and a
/// parser pointed at an absent entry fails its magic check rather than
/// reading a neighbour.
///
/// The standalone bundle's readers (`parse_prot_toc`, `Archive::from_bytes`)
/// walk this exactly as they walk the disc's own file, so nothing about
/// their entry arithmetic is duplicated here - only the container is.
pub(crate) fn compact_prot_image(index: &ProtIndex) -> Option<Vec<u8>> {
    let entries = index.entries();
    let count = entries.iter().map(|e| e.index + 1).max()? as usize;
    let header_bytes = 16 + 4 * (count + 1);
    let header_sectors = header_bytes.div_ceil(SECTOR);
    let mut body: Vec<u8> = Vec::new();
    let mut lbas: Vec<u32> = Vec::with_capacity(count + 1);
    let mut cursor = header_sectors as u32;
    for p in 0..count as u32 {
        lbas.push(cursor);
        let wanted = COMPACT_PROT_ENTRIES.iter().any(|r| r.contains(&p));
        let bytes = if wanted {
            entries
                .iter()
                .find(|e| e.index == p)
                .and_then(|e| {
                    index
                        .prot_dat_raw_bytes(e.byte_offset, e.size_bytes as usize)
                        .ok()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let sectors = bytes.len().div_ceil(SECTOR).max(1);
        let padded = sectors * SECTOR;
        body.extend_from_slice(&bytes);
        body.resize(body.len() + (padded - bytes.len()), 0);
        cursor += sectors as u32;
    }
    lbas.push(cursor);
    let mut img = vec![0u8; header_sectors * SECTOR];
    img[4..8].copy_from_slice(&(count as u32).to_le_bytes());
    img[8..12].copy_from_slice(&(header_sectors as u32).to_le_bytes());
    for (i, lba) in lbas.iter().enumerate() {
        let off = 16 + 4 * i;
        img[off..off + 4].copy_from_slice(&lba.to_le_bytes());
    }
    img.extend_from_slice(&body);
    Some(img)
}

impl LegaiaRuntime {
    /// The presentation bundle, built on first use. `None` when no disc is
    /// loaded or the compact image did not parse.
    pub(crate) fn ensure_minigame_art(&mut self) -> bool {
        if self.minigame_ui.art.is_some() {
            return true;
        }
        if self.minigame_ui.art_failed {
            return false;
        }
        let Some(host) = self.scene_host.host() else {
            return false;
        };
        let Some(img) = compact_prot_image(&host.index) else {
            self.minigame_ui.art_failed = true;
            return false;
        };
        let mut art = Box::new(LegaiaMinigames::new());
        if art.load_disc(img).is_err() {
            self.minigame_ui.art_failed = true;
            return false;
        }
        // The compact image carries no pre-TOC head, so the slot machine's
        // resident system-UI sheet (its submenu box border) comes off the
        // real PROT.DAT.
        let cabinet = art.slot_cabinet_mut();
        if let Some(c) = cabinet.take() {
            use legaia_ui_formats::title_pak as tp;
            *cabinet = Some(
                match host.index.prot_dat_raw_bytes(
                    tp::OVERLAY_SYSTEM_UI_TIM_OFFSET as u64,
                    tp::OVERLAY_SYSTEM_UI_TIM_SIZE,
                ) {
                    Ok(head) if !c.system_ui => c.with_system_ui(&head),
                    _ => c,
                },
            );
        }
        self.minigame_ui.art = Some(art);
        true
    }

    /// The presentation bundle, when built.
    pub(crate) fn minigame_art(&self) -> Option<&LegaiaMinigames> {
        self.minigame_ui.art.as_deref()
    }

    /// Fire the minigame sessions' queued SFX cue ids (the fishing hub's and
    /// point exchange's blips) through the
    /// page's scheduler - the browser twin of the native window's
    /// `drain_minigame_sfx_cues`. Drained every frame, audio up or not, so
    /// the queue cannot grow on a muted page.
    ///
    /// The drain and the enqueue are the session's step
    /// (`BootSession::enqueue_minigame_sfx_cues`); the count feeds the page's
    /// readout.
    pub(crate) fn drain_minigame_sfx_cues_web(&mut self) {
        let queued = self
            .scene_host
            .session_mut()
            .map_or(0, |s| s.enqueue_minigame_sfx_cues());
        self.sfx.queued += queued;
    }

    /// Per-tick presentation step. Cheap no-op outside a minigame mode.
    pub(crate) fn tick_minigame_ui(&mut self) {
        self.drain_minigame_sfx_cues_web();
        // The fishing venue actors (wander / ripple / floor / camera publish /
        // line / catch bursts / sway): the shared kernel the native window's
        // minigame frame runs, reading the fishing events this world tick
        // raised.
        self.tick_fishing_actors();
        // The dance's auto-end and the entry's venue globals: the session's
        // step (`BootSession::step_dance_tail`), the native window's call.
        // The hall geometry itself is the minigame art's `DanceVenue` build
        // (`play_mg_dance_*`).
        if let Some(s) = self.scene_host.session_mut() {
            let _ = s.step_dance_tail();
        }
        let now = self
            .scene_host
            .host()
            .and_then(|h| ActiveGame::of_mode(h.world.mode));
        let prev = self.minigame_ui.game;
        if now != prev {
            if let Some(g) = prev {
                self.on_minigame_exit(g);
            }
            if let Some(g) = now {
                self.on_minigame_enter(g);
            }
            self.minigame_ui.game = now;
        }
        match now {
            Some(ActiveGame::Slot) => self.tick_slot_ui(),
            Some(ActiveGame::Baka) => self.tick_baka_ui(),
            Some(ActiveGame::Muscle) => self.tick_muscle_ui(),
            Some(ActiveGame::Dance) | Some(ActiveGame::Fishing) | None => {}
        }
        // Publish the dance's VRAM residency claim (see `play_dance_art`), so
        // the count-in banner's sprite-or-placeholder choice is the same
        // world-side predicate the native window reads.
        self.sync_dance_hud_residency();
        // The dome's between-leg hub screens (INTERVAL + tally) run after
        // the leg has closed, i.e. back in the field mode - so the hub tick
        // runs every frame, not only inside the dome.
        self.tick_muscle_hub();
    }

    fn on_minigame_enter(&mut self, game: ActiveGame) {
        self.minigame_ui.generation = self.minigame_ui.generation.wrapping_add(1);
        self.ensure_minigame_art();
        match game {
            ActiveGame::Slot => self.enter_slot_ui(),
            ActiveGame::Baka => self.enter_baka_ui(),
            ActiveGame::Muscle => self.enter_muscle_ui(),
            ActiveGame::Dance | ActiveGame::Fishing => {}
        }
    }

    fn on_minigame_exit(&mut self, game: ActiveGame) {
        match game {
            // The slot machine replaces the VRAM texture with its art pack
            // whenever its resident set decoded (the shared cabinet pass);
            // a restore is harmless when it drew on the 2D layer instead.
            ActiveGame::Slot
            | ActiveGame::Baka
            | ActiveGame::Muscle
            | ActiveGame::Dance
            | ActiveGame::Fishing => {
                self.minigame_ui.vram_restore_pending = true;
            }
        }
    }

    /// The live minigame's status caption (`engine-minigame-scenes::minigame_status`),
    /// the one row model the native window shows in its title bar. `None`
    /// outside a minigame.
    ///
    /// This host's two answers: the slot machine draws itself once its
    /// resident set decoded, and the fishing rows name the keys of the
    /// page's **live** binding table ([`crate::pad_bindings::live_mapping`],
    /// so a rebind shows) plus the prize-exchange button beside the frame.
    pub(crate) fn minigame_caption(
        &self,
    ) -> Option<legaia_engine_minigame_scenes::minigame_status::MinigameCaption> {
        use legaia_engine_minigame_scenes::minigame_status as ms;
        let host = self.scene_host.host()?;
        let mapping = crate::pad_bindings::live_mapping();
        let key = |button: &'static str| mapping.key_label_for_button(button).unwrap_or(button);
        host.world.minigame_caption(&ms::StatusHost {
            slot_machine_drawn: self.play_mg_slot_cabinet_ready(),
            fishing_keys: ms::FishingKeys {
                cast: key("Circle"),
                reel_a: key("Cross"),
                reel_b: key("Square"),
                menu: key("Triangle"),
                quit: key("Start"),
                prizes: "Prize exchange button",
            },
        })
    }

    /// Overlay quads (surface pixels) for the active in-world minigame,
    /// appended to `play_overlay_draws_json`'s lists. Empty outside one.
    /// Read-only by design (the composite holds the menu assets borrowed);
    /// any per-frame state moves in [`Self::tick_minigame_ui`].
    ///
    /// The text here is the game picture's own: the retail-placed strips,
    /// labels and frame rows. The engine's status rows are not in it - they
    /// are the caption ([`Self::play_mg_caption_json`]).
    pub(crate) fn minigame_overlay_draws(
        &self,
        font: &legaia_font::Font,
        surface_w: u32,
        surface_h: u32,
    ) -> (Vec<SpriteDraw>, Vec<TextDraw>) {
        // The dance HUD frame is withheld while the pre-song count-in banner
        // is up, off the same shared phase predicate the native HUD reads -
        // the rule lives with the phase, not with either draw list.
        let dance_hud = self
            .scene_host
            .host()
            .is_some_and(|h| h.world.minigames.dance_status_visible());
        let mut texts = match self.minigame_ui.game {
            Some(ActiveGame::Slot) => self.slot_rules_text_draws(font),
            Some(ActiveGame::Baka) => self.baka_hud_text_draws(font),
            Some(ActiveGame::Dance) if dance_hud => self.dance_hud_text_draws(font),
            Some(ActiveGame::Dance)
            | Some(ActiveGame::Muscle)
            | Some(ActiveGame::Fishing)
            | None => Vec::new(),
        };
        if self.minigame_ui.game == Some(ActiveGame::Dance) {
            texts.extend(self.dance_countin_and_tutorial_draws(font));
        }
        // The effect parts. OUTSIDE the `game` gate deliberately: the pool's
        // busiest producer is the fishing venue, and fishing is not one of
        // this page's `ActiveGame` screens - it draws from `play_fishing`.
        // A part also outlives its mode by the length of its fade ramp.
        texts.extend(self.minigame_fx_stage_draws(font));
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        ui::scale_stage_text_draws(&mut texts, origin, scale);
        (Vec::new(), texts)
    }

    /// The minigame effect parts this frame, in **stage** space.
    ///
    /// Two pools, one builder. The shared
    /// [`legaia_engine_minigames::minigame_fx::MinigameFxPool`] on
    /// `World::minigames.fx` carries the fishing venue's splash, ripples and
    /// catch bursts; the dance run carries its own, because its sequence-clear
    /// banner is spawned by the judge rather than by a host. Both used to be
    /// drawn only by the native window - the pool because it lived inside
    /// that window, the dance parts because this page had no emit site for
    /// them at all.
    fn minigame_fx_stage_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        use legaia_engine_screens::minigame_hud::{dance_sprite_part_views, fx_part_views};
        use legaia_engine_ui::minigame_fx as fx;
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        let mg = &host.world.minigames;
        let mut out = fx::fx_part_draws(font, &fx_part_views(&mg.fx), (0, 0), 1);
        if let Some(g) = mg.dance.as_ref() {
            let dance_parts = dance_sprite_part_views(&g.sprite_part_emits());
            out.extend(fx::dance_sprite_part_draws(font, &dance_parts, (0, 0), 1));
        }
        out
    }

    /// The dance's pre-song count-in banner and the Disco King how-to
    /// tutorial's captions, in **stage** space (the caller applies the
    /// transform with the rest of the minigame chrome).
    ///
    /// Both come out of `legaia_engine_ui::dance`, the builders the native
    /// window's HUD draws them with, off the world state
    /// `World::tick_dance` produces - so the banner slides on the same frames
    /// on both hosts. Neither existed on this page, and the count-in did not
    /// exist on *either* host for a door-warp entry: the native window ran
    /// one only from its own debug launcher.
    fn dance_countin_and_tutorial_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        use legaia_engine_ui::dance as ui_dance;
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        let mg = &host.world.minigames;
        let mut out = Vec::new();
        // The banner's placeholder letterforms, drawn ONLY while the hall's
        // HUD texture page is not resident. With it staged the banner is
        // retail's own sprite, emitted as screen-space primitives through
        // `play_dance_art` - the same either/or, off the same world flag, as
        // the native window's HUD builder.
        if let Some(env) = mg
            .dance_countin_banner
            .as_ref()
            .filter(|_| !mg.dance_hud_art_staged)
        {
            out.extend(ui_dance::dance_countin_draws_for(
                font,
                ui_dance::DanceCountInView {
                    x_offset: env.x_offset,
                    brightness: env.brightness,
                    hold: env.hold,
                },
                (0, 0),
                1,
            ));
        }
        // `GO!` after READY (`FUN_801cf470` states 4 / 5), same either/or.
        if let Some(go) = mg.dance_countin_go.filter(|_| !mg.dance_hud_art_staged) {
            out.extend(ui_dance::dance_go_draws_for(font, go, (0, 0), 1));
        }
        if let Some(tf) = mg.dance_tutorial_frame.as_ref() {
            out.extend(ui_dance::dance_tutorial_draws_for(
                font,
                ui_dance::DanceTutorialView {
                    captions: &tf.captions,
                    options: tf.options,
                    cursor_pos: tf.cursor_pos,
                    feedback: tf.feedback,
                },
                (0, 0),
                1,
            ));
        }
        out
    }
}

/// Stage-space row inks for this page's own status lines.
pub(crate) const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub(crate) const DIM: [f32; 4] = [0.65, 0.72, 0.8, 1.0];

/// One text row in stage space.
pub(crate) fn row(
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    ui::text_draws_for(&font.layout_ascii(text), pen, color)
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl LegaiaRuntime {
    /// Which in-world minigame owns the screen this frame, with the scene
    /// generation the page keys its uploads on:
    ///
    /// ```json
    /// { "game": "slot" | "baka" | "muscle" | "dance" | null, "gen": 3,
    ///   "art": true,
    ///   "muscle": { "monster_id": 170, "char_slot": 0 },
    ///   "baka": { "opponent": 5, "player_char": 0 } }
    /// ```
    ///
    /// `art` says whether the presentation bundle decoded; without it the
    /// page draws the text HUD alone and says so.
    pub fn play_mg_game_json(&self) -> String {
        let ui = &self.minigame_ui;
        // The seated opponent is the live session's (a rung the cabinet
        // installs mid-visit replaces it), the entry snapshot only without one.
        let opponent = self
            .scene_host
            .host()
            .and_then(|h| h.world.minigames.baka_fighter.as_ref())
            .map_or(ui.baka.opponent, |f| f.opponent_roster());
        serde_json::json!({
            "game": ui.game.map(ActiveGame::label),
            "gen": ui.generation,
            "art": ui.art.is_some(),
            "muscle": {
                "monster_id": ui.muscle.monster_id,
                "char_slot": ui.muscle.char_slot,
            },
            // The three party-side roster rows (`0..=2`) share the PROT 1204
            // party pack (retail folds them at `FIGHTER_PACK_FOLD`), so an
            // opponent drawn from one of them is a side-0 mesh; the ladder
            // rows `3..=16` carry their own packs (side 1).
            "baka": {
                "opponent": opponent,
                "opponent_side": u32::from(
                    opponent >= legaia_engine_minigames::baka_fighter::FIGHTER_PACK_FOLD
                ),
                "player_char": ui.baka.player_char,
            },
        })
        .to_string()
    }

    /// The live minigame's status caption, as strings for the strip under
    /// the canvas:
    ///
    /// ```json
    /// { "game": "baka", "rows": [
    ///     { "kind": "readout", "clauses": ["BAKA", "you 3200hp (wins 0)"] },
    ///     { "kind": "hint", "clauses": ["choose your attack", "Start = quit"] } ] }
    /// ```
    ///
    /// `game` is `null` outside a minigame, and the page hides the strip.
    /// With a game live `rows` may still be empty (a phase with nothing to
    /// say); the page keeps the strip's seat so the controls under it do not
    /// jump. Rows are in reading order; `kind` is `readout` (the rules state)
    /// or `hint` (what to press); a clause is one statement, and the page
    /// picks the separator. The model is
    /// `legaia_engine_minigame_scenes::minigame_status::caption`, the one the native
    /// window writes into its title bar.
    pub fn play_mg_caption_json(&self) -> String {
        let caption = self.minigame_caption();
        serde_json::json!({
            "game": caption.as_ref().map(|c| c.game.label()),
            "rows": caption
                .map(|c| c.rows)
                .unwrap_or_default()
                .iter()
                .map(|r| serde_json::json!({
                    "kind": r.kind.label(),
                    "clauses": r.clauses,
                }))
                .collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// `true` once per minigame exit that replaced the VRAM texture: the
    /// page re-uploads `field_vram_bytes` on it. Clears the flag.
    pub fn play_mg_take_vram_restore(&mut self) -> bool {
        std::mem::take(&mut self.minigame_ui.vram_restore_pending)
    }

    /// Developer / test affordance: arm the mode-24 door warp for `sub_id`
    /// exactly as the field-VM `0x3E` arm does (`3` = slot machine, `4` =
    /// Baka Fighter, `5` = Muscle Dome, `6` = dance, `0` = fishing). The
    /// next `tick_frame` drains it through the scene host's own loader, so
    /// the session that results is the one a casino door installs. Returns
    /// `false` when no scene is loaded. The native window's `O` / `B` / `M`
    /// hotkeys are this affordance's counterpart.
    ///
    /// The request lists the dome hub's announcer lines for prestage
    /// (`World::request_minigame_warp`), and this export stages them **now**:
    /// the next tick enters the hub and its intro line starts on that same
    /// tick, so a stage left to that tick's SFX pass would be a stage on
    /// first use. A walked door gets its lead from the door scene's list
    /// instead (`field_xa::scene_minigame_door_xa_prestage`).
    pub fn play_mg_debug_warp(&mut self, sub_id: u8) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        host.world.request_minigame_warp(sub_id);
        let lead = host.world.drain_field_xa_prestage();
        if let Some(d) = self.audio_director() {
            for xa in &lead {
                d.prestage_xa_clip(xa.clip, xa.channel, xa.duration_sectors);
            }
        }
        true
    }
}
