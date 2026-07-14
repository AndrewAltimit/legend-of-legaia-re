//! Per-frame time/RNG/pad, the top-level tick dispatcher, and the minigame ticks (tile board, dance, fishing, slot machine, baka fighter, muscle dome).
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Advance the wall-clock play-time counter by `delta_seconds`. Engines
    /// drive this from the frame loop's wall-clock delta. Mirrors the
    /// retail "play time" field shown on the save screen.
    pub fn advance_play_time(&mut self, delta_seconds: u32) {
        self.play_time_seconds = self.play_time_seconds.saturating_add(delta_seconds);
    }

    /// Increment the deterministic LCG and return the new value.
    pub fn next_rng(&mut self) -> u32 {
        // Numerical Recipes LCG. Cheap, deterministic.
        self.rng_state = self
            .rng_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.rng_state
    }

    /// Replace the per-frame pad bitmask snapshot. Equivalent to
    /// `self.input.set_pad(mask)` but available without importing
    /// [`input::InputState`] at the call site. Hosts that drive the
    /// world from a scripted timeline (`legaia-engine replay`, the
    /// v0.1 playthrough oracle) call this before each [`Self::tick`].
    pub fn set_pad(&mut self, mask: u16) {
        self.input.set_pad(mask);
    }

    /// Per-frame world tick. Drives whichever scene-mode VMs are live.
    /// Returns the battle-step outcome when in [`SceneMode::Battle`], else
    /// `None`.
    ///
    /// Order of operations:
    ///  1. Effect pool tick - runs every frame regardless of mode.
    ///  2. Per-actor move-VM tick - only for actors with bytecode loaded.
    ///  3. Per-actor physics tick (`FUN_80021DF4`) - drains timer,
    ///     advances motion, kicks the move-buffer cursor on
    ///     [`TickEvent::MoveVmKick`]. Runs over every active actor.
    ///  4. Per-actor keyframe / anim-player tick.
    ///  5. Mode-specific VM:
    ///     - `Battle`     → battle-action state machine step.
    ///     - `Field`      → field-VM step (or no-op if no bytecode loaded).
    ///     - `Cutscene`   → field-VM step (cutscenes use the same script VM).
    ///     - `Title`      → no further VM.
    pub fn tick(&mut self) -> Option<StepOutcome> {
        self.frame += 1;
        // Retail-frame sub-clock for the narration crawl roller. The sim ticks
        // at 100 Hz, but the roller's scroll is authored in retail's ~60 fps
        // field frames; advance a fixed-point accumulator by RETAIL_FPS each
        // tick and emit a whole retail frame on the ~60 % of ticks that cross
        // SIM_HZ, so the crawl scrolls at retail wall-speed (otherwise it drains
        // ~1.7x too fast, opening a long between-crawl gap). See
        // `field_frame_accum`.
        const SIM_HZ: u32 = 100;
        const RETAIL_FPS: u32 = 60;
        self.field_frame_accum += RETAIL_FPS;
        if self.field_frame_accum >= SIM_HZ {
            self.field_frame_accum -= SIM_HZ;
            self.field_frame_step = 1;
        } else {
            self.field_frame_step = 0;
        }
        // Retail game-tick clock for the scripted CLUT-cell effects: one game
        // tick spans `frame_step` vsyncs (the adaptive `DAT_1F800393` factor
        // written by `FUN_80016B6C`; see [`Self::frame_step`]). Count the sim
        // ticks that map to a retail vsync and bank a game tick every
        // `frame_step` of them; [`Self::step_clut_fx`] drains the bank
        // against the host's VRAM. Only accumulates while effects are live
        // (capped so an undrained host can't wind up a backlog).
        if self.field_frame_step == 1 && !self.clut_fx.is_empty() {
            self.clut_vsync_accum += 1;
            if self.clut_vsync_accum >= self.frame_step.max(1) {
                self.clut_vsync_accum = 0;
                self.clut_pending_game_ticks = (self.clut_pending_game_ticks + 1).min(600);
            }
        }
        // Step the active full-screen fade (escape teardown ramp); drop it
        // once the ramp lands on its target so hosts stop drawing the overlay.
        if let Some(fade) = &mut self.screen_fade
            && !fade.step()
        {
            self.screen_fade = None;
        }
        // Step the field-VM colour fade (op 0x34 sub-0, e.g. the opening white
        // flash); drop it when the ramp completes.
        if let Some(fade) = &mut self.color_fade
            && !fade.step()
        {
            self.color_fade = None;
        }
        // Consume a pending FMV transition the field VM signalled last frame
        // (op `0x4C 0xE2`). Retail's main mode dispatcher reads the
        // next-game-mode global one frame after the op writes it, so the flip
        // into the cutscene mode lands here, at the top of the following tick.
        self.maybe_enter_pending_cutscene();
        self.tick_effects();
        self.tick_move_vms();
        self.tick_actor_physics();
        self.tick_actors();
        // Actor-VM glides (op 0x09 `MotionAt` -> `start_motion`): one
        // motion-VM pursue step per frame toward the recorded target.
        self.tick_actor_motions();
        // Tick art-learned banner countdown - clear when it reaches zero.
        if let Some(banner) = &mut self.current_art_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_art_banner = None;
            }
        }
        // Tick level-up banner countdown.
        if let Some(banner) = &mut self.current_level_up_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.current_level_up_banner = None;
            }
        }
        // Advance the post-battle Seru-capture banner; clear when it finishes.
        if let Some(banner) = &mut self.current_capture_banner {
            banner.tick_frame();
            if banner.is_done() {
                self.current_capture_banner = None;
            }
        }
        // Advance the opening-cutscene narration roller. The crawl is
        // timer-driven only (retail `FUN_80037174` has no per-line confirm
        // skip; the player skips the WHOLE opening through the hand-off
        // packet instead - see `take_prologue_handoff`). Clear it once every
        // page has scrolled off so the suspended cutscene timeline resumes.
        // Scroll the roller on the 60 fps sub-clock (0 or 1 retail frame per
        // sim tick) so the crawl reads at retail wall-speed, not 100 Hz.
        if let Some(narration) = &mut self.cutscene_narration
            && !narration.tick(self.field_frame_step as u32)
        {
            self.cutscene_narration = None;
        }
        // Fade the "It was the Seru." caption image (opdeene's baked-TIM
        // caption, `Self::cutscene_caption`). It is target-visible in the
        // FIRST gap after a narration crawl block has shown (a block opened,
        // `seq >= 1`, and has since scrolled out - narration inactive), and
        // fades out on the next block or scene end (the image is cleared on
        // scene entry). At the retail-video-pinned crawl rate the blocks run
        // back-to-back, so the first real gap lands after the LAST crawl -
        // the caption fades in over the held villager tableau, which is
        // where the retail capture shows it. The smooth alpha ramp stands in
        // for the TIM's two-CLUT fade steps.
        //
        // The timeline's post-crawl hold can run long, so `in_gap` alone
        // would freeze the caption on screen. Bound it to a retail-like ~2 s
        // beat: once it has been fully shown for `CAPTION_HOLD_FRAMES`, fade
        // it back out and keep it hidden (the counter never resets within
        // the scene, so the caption shows exactly once).
        if self.cutscene_caption.is_some() {
            const CAPTION_FADE_STEP: f32 = 0.06;
            const CAPTION_HOLD_FRAMES: u32 = 180;
            let in_gap = self.cutscene_narration_seq >= 1 && !self.cutscene_narration_active();
            if in_gap && self.cutscene_caption_alpha >= 1.0 {
                self.cutscene_caption_shown_frames =
                    self.cutscene_caption_shown_frames.saturating_add(1);
            }
            let hold_elapsed = self.cutscene_caption_shown_frames >= CAPTION_HOLD_FRAMES;
            let target = if in_gap && !hold_elapsed { 1.0 } else { 0.0 };
            if self.cutscene_caption_alpha < target {
                self.cutscene_caption_alpha =
                    (self.cutscene_caption_alpha + CAPTION_FADE_STEP).min(target);
            } else if self.cutscene_caption_alpha > target {
                self.cutscene_caption_alpha =
                    (self.cutscene_caption_alpha - CAPTION_FADE_STEP).max(target);
            }
        }
        match self.mode {
            SceneMode::Battle => {
                if self.live_gameplay_loop {
                    self.live_battle_tick()
                } else {
                    Some(self.step_battle())
                }
            }
            SceneMode::Field => {
                // Per-tick: one Cross/Circle edge feeds at most one of the
                // script's 0x4C dialog poll or the interaction probe.
                self.dialog_input_consumed = false;
                self.step_cutscene_timeline();
                // Concurrent spawned-record contexts (mid-play op-0x44 helper
                // spawns): independent field-VM contexts that never seize the
                // camera or lock the player.
                self.step_helper_contexts();
                // Per-actor script channels (spawned with a cutscene
                // timeline): each vignette actor's own placement script runs
                // its frame slice - animate cues, scripted moves, flag
                // handshakes with the timeline.
                self.step_field_channels();
                self.step_field();
                // Field-NPC walk legs (autonomous patrol routes + scripted
                // interaction-prologue runs) - one motion-VM step per frame,
                // writing back into `field_npc_positions` so collision /
                // interact probes follow the live NPC.
                self.tick_field_npc_motions();
                self.tick_tile_board();
                // Rebuild the tile-actor draw list from the current board +
                // player cell (retail's per-frame board render pass).
                self.refresh_tile_board_draw_list();
                self.step_field_locomotion();
                // Locomotion animation: idle vs walk off the movement flag
                // the step above just set, folded into the player's
                // `pose_frame` for the host's posed-mesh rebuild.
                self.tick_field_player_anim();
                // Placed-prop layer: advance the prop clips, step an
                // in-flight prop record run (a door swing / cupboard search
                // through the field VM), and start a run for a movement
                // touch the locomotion just posted (the retail bit-4
                // auto-post of FUN_801D5B5C).
                self.tick_prop_interactions();
                // Interaction probe (retail FUN_801cf9f4): talk to an adjacent
                // NPC / dismiss its box on the action button. Runs before the
                // carrier tick so a dialogue-accept engage launches the battle
                // the same frame.
                self.tick_field_interaction_probe();
                self.tick_field_carriers();
                // Faithful dialogue path (opt-in): drive a just-opened field
                // dialogue through the field VM so branch handlers execute.
                self.drive_inline_dialogue();
                // Screen-effect widgets (mask / sprite / panel / letterbox,
                // the ending-scene op-0x43 family) tick after the script step
                // that may have spawned them this frame.
                self.tick_screen_fx();
                if self.live_gameplay_loop {
                    self.live_field_tick();
                }
                None
            }
            SceneMode::Cutscene => {
                // An in-engine choreography cutscene (no STR FMV) is just a
                // field scene that suppresses field/battle dispatch, so the
                // field VM keeps stepping. While an STR FMV is playing
                // ([`active_fmv`] set), the field VM is suspended - retail
                // hands the frame to the cutscene/MDEC overlay - and the host
                // drives playback, calling [`finish_cutscene`] when it ends.
                if self.active_fmv.is_none() {
                    self.step_cutscene_timeline();
                    self.step_helper_contexts();
                    self.step_field_channels();
                    self.step_field();
                    self.tick_screen_fx();
                }
                None
            }
            SceneMode::WorldMap => {
                // The opening chain's `map01` fly-in leg runs its cutscene
                // record over the world map (Mist title card + crawl + the
                // terminal SceneChange into Rim Elm), and a free-roam overworld
                // walk-on **beat** record (a Drake mist-wall force-walk band, a
                // gate-1 non-portal partition-2 record spawned by
                // `SceneHost::dispatch_walk_on_trigger` in WorldMap mode) is the
                // same single-context cutscene timeline. Step whichever is
                // installed; `step_world_map_locomotion` stands the overworld
                // player down while it plays (the force-walk lock).
                if self.opening_chain_active || self.cutscene_timeline_active() {
                    self.step_cutscene_timeline();
                }
                // Overworld helper spawns (an op-0x44 issued by a world-map
                // record) execute concurrently, same as the field arm.
                self.step_helper_contexts();
                self.tick_world_map();
                None
            }
            SceneMode::Dance => {
                self.tick_dance();
                None
            }
            SceneMode::Fishing => {
                self.tick_fishing();
                None
            }
            SceneMode::SlotMachine => {
                self.tick_slot_machine();
                None
            }
            SceneMode::BakaFighter => {
                self.tick_baka_fighter();
                None
            }
            SceneMode::MuscleDome => {
                self.tick_muscle_dome();
                None
            }
            // The pause menu owns the frame (retail CARD mode 0x17): field /
            // battle dispatch is suspended; the hosting session drives the
            // menu state machine and restores the suspended mode on close.
            SceneMode::Menu => None,
            SceneMode::Title => None,
        }
    }

    /// Tile-board player step: read one d-pad direction from
    /// [`World.input`](Self::input), gate it against the board's
    /// collision cells, and interpolate the player actor toward the
    /// destination tile centre. Drives the puzzle / board minigame mode,
    /// not general town locomotion.
    ///
    /// PORT: the walk state machine in `overlay_0897_801ef2b0`. The
    /// player is either *idle* (`tile_board_target == None`, accepting a
    /// new direction) or *interpolating* toward a committed target tile
    /// (case 2). A direction is only consumed while idle, so holding the
    /// d-pad steps tile-by-tile - matching retail, where the SM re-reads
    /// the pad only after the previous step's interpolation completes.
    ///
    /// No-ops without a player actor slot or an installed
    /// [`tile_board`](crate::tile_board), and while a dialog box is up
    /// (the field VM owns the frame). Reads only pad bits + board state,
    /// so it is deterministic across identical pad streams.
    fn tick_tile_board(&mut self) {
        if self.current_dialog.is_some() {
            return;
        }
        let Some(player_slot) = self.player_actor_slot else {
            return;
        };
        let slot = player_slot as usize;
        if self.tile_board.is_none() || slot >= self.actors.len() {
            return;
        }

        // Interpolating toward a committed target tile.
        if let Some((tx, tz)) = self.tile_board_target {
            let ms = &mut self.actors[slot].move_state;
            let nx = step_toward(ms.world_x as i32, tx, TILE_BOARD_SPEED);
            let nz = step_toward(ms.world_z as i32, tz, TILE_BOARD_SPEED);
            ms.world_x = nx as i16;
            ms.world_z = nz as i16;
            if nx == tx && nz == tz {
                self.tile_board_target = None;
                self.tile_board_arrival();
            }
            return;
        }

        // Idle: decode one direction and try to step.
        let Some(dir) = tile_step_from_input(&self.input) else {
            return;
        };
        if let Some((tx, tz)) = self.tile_board.as_mut().and_then(|b| b.try_step(dir)) {
            self.tile_board_target = Some((tx, tz));
        }
    }

    /// Advance the screen-effect widgets one frame and refresh
    /// [`Self::screen_fx_frame`]. Runs in the Field / Cutscene tick after
    /// the script step (so a sub-op spawned this frame draws this frame,
    /// matching retail's actor-pool order). The engine ticks the widget
    /// clocks by 1 per world tick (retail's per-frame byte
    /// `DAT_1F800393`); the sprite scripts' flag waits probe the shared
    /// system flag bank ([`Self::system_flag_test`], `FUN_8003CE64`).
    fn tick_screen_fx(&mut self) {
        if !self.screen_fx.is_active() {
            if !self.screen_fx_frame.is_empty() {
                self.screen_fx_frame = Default::default();
            }
            return;
        }
        let mut fx = std::mem::take(&mut self.screen_fx);
        self.screen_fx_frame = fx.tick(1, |idx| self.system_flag_test(idx));
        self.screen_fx = fx;
    }

    /// Walk-SM arrival pass (`overlay_0897_801ef2b0` case 3), run when the
    /// player's interpolation reaches the committed tile centre:
    ///
    /// - an **event / transition cell** (`8..=0xA`) leaves the board mode -
    ///   the board uninstalls, and the suspended op-0x49 script reads `Done`
    ///   and resumes (retail reads the header `+7`/`+9` flag operands here;
    ///   the engine surfaces the exit through the op-49 tristate);
    /// - an **animated cell** (`0xB..=0xE`) cycles its value one step,
    ///   wrapping `0xE -> 0xB` (the arrival sub-state's decay pass).
    ///
    /// PORT: overlay_0897_801ef2b0 (arrival sub-states)
    fn tile_board_arrival(&mut self) {
        use crate::tile_board::{
            CELL_ANIM_FIRST, CELL_ANIM_LAST, CELL_EVENT_FIRST, CELL_EVENT_LAST,
        };
        let Some(board) = self.tile_board.as_mut() else {
            return;
        };
        let (col, row) = (board.player_col as i32, board.player_row as i32);
        let Some(cell) = board.cell(col, row) else {
            return;
        };
        if (CELL_EVENT_FIRST..=CELL_EVENT_LAST).contains(&cell) {
            // Event / transition tile: exit the board. `tile_board_armed`
            // stays set so the op-49 tristate reads Done and the field
            // script resumes past the install op. Despawn the tile actors
            // so they don't leak into the next scene.
            self.tile_board = None;
            self.tile_board_header = None;
            self.despawn_tile_actors();
        } else if (CELL_ANIM_FIRST..=CELL_ANIM_LAST).contains(&cell) {
            let next = if cell == CELL_ANIM_LAST {
                CELL_ANIM_FIRST
            } else {
                cell + 1
            };
            let idx = row as usize * board.width as usize + col as usize;
            if let Some(c) = board.cells.get_mut(idx) {
                *c = next;
            }
        }
    }

    /// Install a tile board from a field-VM op-0x49 **sub-op 5** instruction
    /// (`instr` = the bytes from the opcode onward, as handed to
    /// `FieldHost::op49_menu_request`). Parses the 13-byte inline header
    /// (`instr[1..]`, the window retail points `_DAT_8007b450` at), fills the
    /// cells with the retail procedural fill (`overlay_0897_801e0b1c`, seeded
    /// from the world RNG the way retail seeds from BIOS `rand`), seats the
    /// player actor at the board's start-cell centre, and holds the script
    /// suspended (`tile_board_armed`) until the board exits.
    ///
    /// Returns `false` (leaving the op merely suspended, matching the other
    /// op-49 consumers) when a board is already up or the header is
    /// malformed.
    ///
    /// PORT: overlay_0897_801e0b1c (board alloc + fill; cells only - the
    /// per-cell tile-actor spawns are a renderer concern)
    /// REF: overlay_0897_801de840 (op 0x49 arm, `_DAT_8007b450 = pbVar47`)
    pub fn try_install_tile_board(&mut self, instr: &[u8]) -> bool {
        if self.tile_board_armed || self.tile_board.is_some() {
            return false;
        }
        let Some(window) = instr.get(1..) else {
            return false;
        };
        let Some(header) = crate::tile_board::TileBoardHeader::parse(window) else {
            return false;
        };
        let cells = crate::tile_board::procedural_fill(header.width, header.height, || {
            self.next_rng() & 0x7FFF
        });
        let board = crate::tile_board::TileBoard::from_header(&header, cells);

        // Spawn one tile actor per distinct drawn cell value present on the
        // board (retail `DAT_801f35bc[value]`, slots `2..=14`): resolve the
        // template `tile_template_base + (value - 2)` through the same
        // global-TMD + VDF-buffer path the `0x4C 0xD8` field allocator uses
        // (`spawn_field_actor`). The renderer repositions + draws these each
        // frame; unresolved templates still allocate a slot (empty mesh).
        let mut present = [false; crate::tile_board::TILE_ACTOR_TABLE_LEN];
        for &c in &board.cells {
            if crate::tile_board::is_drawable_cell(c) {
                present[c as usize] = true;
            }
        }
        let mut tile_slots = [None; crate::tile_board::TILE_ACTOR_TABLE_LEN];
        for value in crate::tile_board::CELL_DRAW_FIRST..=crate::tile_board::CELL_DRAW_LAST {
            if !present[value as usize] {
                continue;
            }
            let tpl = crate::tile_board::tile_template_for(header.tile_template_base, value);
            if let Some(slot) = self.spawn_field_actor(tpl as i16, tpl, value as u16, 0) {
                tile_slots[value as usize] = Some(slot as u8);
            }
        }
        // Table slot 0 = the player actor (retail spawns it from header
        // `+0xb`). The engine reuses the existing player actor: seat it at
        // the start cell's tile centre so the first step interpolates from
        // the board frame, and bind its mesh from `player_template` when the
        // global TMD pool carries it (else keep the field mesh).
        if let Some(slot) = self.player_actor_slot {
            tile_slots[0] = Some(slot);
            let (x, z) = board.player_world();
            let player_tmd = self.global_tmd(header.player_template as i16).cloned();
            if let Some(a) = self.actors.get_mut(slot as usize) {
                a.move_state.world_x = x as i16;
                a.move_state.world_z = z as i16;
                if let Some(tmd) = player_tmd {
                    a.tmd_ref = Some(tmd);
                }
            }
        }

        self.tile_actor_slots = tile_slots;
        self.tile_board_target = None;
        self.tile_board = Some(board);
        self.tile_board_header = Some(header);
        self.tile_board_armed = true;
        true
    }

    /// Despawn the tile-board tile actors (the `2..=14` entries of the
    /// tile-actor table) and clear the table + draw list. The player actor
    /// (table slot 0) outlives the board and is left in place. Called on
    /// board teardown so tile actors don't leak into the next scene.
    ///
    /// PORT: the walk-SM board-exit teardown (`overlay_0897_801ef2b0`
    /// case 8 -> board free).
    fn despawn_tile_actors(&mut self) {
        for value in crate::tile_board::CELL_DRAW_FIRST..=crate::tile_board::CELL_DRAW_LAST {
            if let Some(slot) = self.tile_actor_slots[value as usize]
                && let Some(a) = self.actors.get_mut(slot as usize)
            {
                *a = Actor::new();
            }
        }
        self.tile_actor_slots = [None; crate::tile_board::TILE_ACTOR_TABLE_LEN];
        self.tile_board_draw_list.clear();
    }

    /// Rebuild the per-frame tile-board draw list (retail
    /// `overlay_0897_801e0f3c`): for every drawable cell in the active draw
    /// set (full board or the windowed radius around the player, per header
    /// `+6`/`+5`), select the cell value's tile actor from the tile-actor
    /// table and record it at the cell's world centre, then reposition that
    /// actor there (retail moves the selected actor before drawing). When a
    /// value repeats across cells the shared actor ends at the last drawn
    /// cell; the draw list still carries the full per-cell set the deferred
    /// renderer needs. Clears the list when no board is installed. The
    /// player actor is drawn by the normal field path, so it is not seated
    /// here (that would fight the step interpolation).
    fn refresh_tile_board_draw_list(&mut self) {
        let Some(header) = self.tile_board_header else {
            self.tile_board_draw_list.clear();
            return;
        };
        let Some(board) = self.tile_board.as_ref() else {
            self.tile_board_draw_list.clear();
            return;
        };
        let mut list = Vec::new();
        for (col, row) in board.draw_cells(header.mode_flag, header.radius) {
            let Some(cell) = board.cell(col, row) else {
                continue;
            };
            if !crate::tile_board::is_drawable_cell(cell) {
                continue;
            }
            let Some(slot) = self.tile_actor_slots[cell as usize] else {
                continue;
            };
            let (world_x, world_z) = board.tile_world(col, row);
            list.push(crate::tile_board::TileDraw {
                col: col as u8,
                row: row as u8,
                cell_value: cell,
                slot,
                world_x,
                world_z,
            });
        }
        for d in &list {
            if let Some(a) = self.actors.get_mut(d.slot as usize) {
                a.move_state.world_x = d.world_x as i16;
                a.move_state.world_z = d.world_z as i16;
            }
        }
        self.tile_board_draw_list = list;
    }

    /// Enter the Noa dance (rhythm) minigame on `game`, suspending the current
    /// scene mode. The suspended mode is restored by [`World::exit_dance`] (and
    /// automatically once the song ends). Mirrors the pause-menu suspend/restore
    /// contract: the interrupted field/battle state stays intact underneath.
    pub fn enter_dance(&mut self, game: crate::dance::DanceGame) {
        // Don't stack a suspend: if the dance is already running, just swap the
        // game so a re-entry keeps the true return mode.
        if self.mode != SceneMode::Dance {
            self.dance_return_mode = self.mode;
        }
        self.dance = Some(game);
        self.dance_last_judge = None;
        self.mode = SceneMode::Dance;
    }

    /// Clear the dance minigame and return the final [`DanceGame`] so the host
    /// can read the score / pass result. Restores the interrupted mode if it is
    /// still `Dance` (a mid-song abort); when the song already auto-ended
    /// [`tick_dance`](Self::tick_dance) has restored the mode but left the game
    /// installed for one frame so the host can read it - this take clears it.
    pub fn exit_dance(&mut self) -> Option<crate::dance::DanceGame> {
        if self.mode == SceneMode::Dance {
            self.mode = self.dance_return_mode;
        }
        self.dance_last_judge = None;
        self.dance.take()
    }

    /// Advance the dance minigame one frame: step the beat clock, judge this
    /// frame's directional presses, and end the run when the song finishes.
    ///
    /// The three judged directions map to the retail pad bits
    /// ([`crate::dance::DanceDir::pad_bit`] = `0x80`/`0x20`/`0x10`), which are
    /// this pad's [`Left`](input::PadButton::Left) / [`Right`](input::PadButton::Right)
    /// / [`Up`](input::PadButton::Up) - a press on any of them on this frame is
    /// judged against the active beat's chart cell. Edge-triggered
    /// (`just_pressed`) so a held button scores at most one note per press.
    ///
    /// PORT: the dance overlay's per-frame driver (`FUN_801cf470` beat clock ->
    /// `FUN_801d1960` hit judge), one advance + one judged press pass per frame.
    fn tick_dance(&mut self) {
        let Some(game) = self.dance.as_mut() else {
            // Mode is Dance but no game installed - drop back to a sane mode.
            self.mode = self.dance_return_mode;
            return;
        };
        game.advance(1);
        // Judge at most one directional press this frame (retail reads one pad
        // word per beat-clock tick). Priority Left -> Right -> Up is arbitrary
        // among simultaneous presses; a rhythm player presses one at a time.
        use crate::dance::DanceDir;
        let dir = if self.input.just_pressed(input::PadButton::Left) {
            Some(DanceDir::A)
        } else if self.input.just_pressed(input::PadButton::Right) {
            Some(DanceDir::B)
        } else if self.input.just_pressed(input::PadButton::Up) {
            Some(DanceDir::C)
        } else {
            None
        };
        if let Some(dir) = dir {
            self.dance_last_judge = Some(game.judge_press(dir));
        }
        if game.song_over() {
            // Song finished: restore the interrupted mode, leaving `dance`
            // in place so the host can read the final score before clearing.
            self.mode = self.dance_return_mode;
        }
    }

    /// Enter the fishing minigame on `session`, suspending the current scene
    /// mode (restored by [`World::exit_fishing`]). Like the dance / pause-menu
    /// suspend contract, the interrupted field state stays intact underneath.
    pub fn enter_fishing(&mut self, session: crate::fishing::FishingSession) {
        if self.mode != SceneMode::Fishing {
            self.fishing_return_mode = self.mode;
        }
        self.fishing = Some(session);
        self.mode = SceneMode::Fishing;
    }

    /// Leave the fishing minigame and restore the interrupted mode, returning
    /// the session so the host can read the final [`FishingRecord`]. The
    /// record's point total is banked into the persistent
    /// [`World::fishing_points`] pool (retail credits `_DAT_8008444C`
    /// directly; hosts seed the next session's record from the pool). No-op
    /// when fishing isn't active.
    ///
    /// [`FishingRecord`]: crate::fishing::FishingRecord
    pub fn exit_fishing(&mut self) -> Option<crate::fishing::FishingSession> {
        if self.mode == SceneMode::Fishing {
            self.mode = self.fishing_return_mode;
        }
        let session = self.fishing.take();
        self.fishing_exchange = None;
        if let Some(s) = &session {
            self.fishing_points = s.record().points;
        }
        session
    }

    /// Open the fishing point-exchange (prize shop) list on `exchange`.
    /// The host renders [`World::fishing_exchange`] and commits buys through
    /// [`World::fishing_exchange_buy`].
    pub fn open_fishing_exchange(&mut self, mut exchange: crate::fishing::PrizeExchange) {
        // Row 0 hides until strictly affordable - floor the cursor to the
        // first visible row for the current point pool.
        exchange.cursor = exchange
            .cursor
            .max(exchange.first_visible(self.fishing_points));
        self.fishing_exchange = Some(exchange);
    }

    /// Close the point-exchange list.
    pub fn close_fishing_exchange(&mut self) {
        self.fishing_exchange = None;
    }

    /// Commit a point-exchange purchase of `qty` units of `row`
    /// (`FUN_801d06c8`'s Yes arm): validates through
    /// [`crate::fishing::PrizeExchange::buy`] against the persistent pool /
    /// purchased mask / live inventory count, then deducts
    /// [`World::fishing_points`], latches the one-time bit, and grants the
    /// item into [`World::inventory`]. While a fishing session is live its
    /// record is synced to the reduced pool so the on-screen point total
    /// matches. `None` when no exchange is open or the buy doesn't validate.
    pub fn fishing_exchange_buy(
        &mut self,
        row: usize,
        qty: u32,
    ) -> Option<crate::fishing::PrizePurchase> {
        let ex = self.fishing_exchange.as_ref()?;
        let item_id = ex.rows.get(row)?.item_id;
        let owned = *self.inventory.get(&item_id).unwrap_or(&0) as u32;
        let purchase = ex.buy(
            row,
            qty,
            self.fishing_points,
            owned,
            self.fishing_prizes_purchased,
        )?;
        self.fishing_points -= purchase.cost as i32;
        if let Some(bit) = purchase.latched_bit {
            self.fishing_prizes_purchased |= 1 << bit;
        }
        let count = self.inventory.entry(purchase.item_id).or_insert(0);
        *count = count.saturating_add(purchase.qty.min(255) as u8);
        if let Some(s) = &mut self.fishing {
            s.set_points(self.fishing_points);
        }
        Some(purchase)
    }

    /// Advance the fishing minigame one frame, reading this frame's pad:
    ///
    /// - **Casting**: the power meter oscillates; a confirm press
    ///   ([`Cross`](input::PadButton::Cross)) locks the cast and hooks a fish.
    /// - **Fighting**: holding a reel button raises tension - [`Cross`] is reel
    ///   A (the `rod*9 + 0x23` divisor), [`Circle`] reel B (`rod*6 + 0x19`);
    ///   neither held bleeds tension off. The line snaps at max tension.
    /// - **Done**: a confirm press recasts.
    ///
    /// [`Cross`]: input::PadButton::Cross
    /// [`Circle`]: input::PadButton::Circle
    ///
    /// PORT: the fishing overlay's per-frame driver (`FUN_801cf3bc` mode SM ->
    /// `FUN_801d4004` tension). The casting-meter step is not byte-pinned (the
    /// retail meter sweeps visibly fast); `FISHING_CAST_STEP` is the host rate.
    fn tick_fishing(&mut self) {
        use crate::fishing::{FishingPhase, ReelInput};
        /// Per-frame casting-meter step (see the method note - not byte-pinned).
        const FISHING_CAST_STEP: i32 = 0x80;
        let Some(phase) = self.fishing.as_ref().map(|s| s.phase()) else {
            // Mode is Fishing but no session installed - drop back to a sane mode.
            self.mode = self.fishing_return_mode;
            return;
        };
        match phase {
            FishingPhase::Casting => {
                if let Some(s) = self.fishing.as_mut() {
                    s.advance_cast(FISHING_CAST_STEP);
                }
                if self.input.just_pressed(input::PadButton::Cross)
                    && let Some(s) = self.fishing.as_mut()
                {
                    s.lock_cast();
                }
            }
            FishingPhase::Fighting => {
                let input = if self.input.pressed(input::PadButton::Cross) {
                    ReelInput::ReelA
                } else if self.input.pressed(input::PadButton::Circle) {
                    ReelInput::ReelB
                } else {
                    ReelInput::Idle
                };
                if let Some(s) = self.fishing.as_mut() {
                    s.reel(input, 1);
                }
            }
            FishingPhase::Done => {
                if self.input.just_pressed(input::PadButton::Cross)
                    && let Some(s) = self.fishing.as_mut()
                {
                    s.recast();
                }
            }
        }
    }

    /// Enter the casino slot-machine minigame on `machine`, suspending the
    /// current scene mode (restored by [`World::exit_slot_machine`]). Like
    /// the dance / fishing / pause-menu suspend contract, the interrupted
    /// field state stays intact underneath.
    pub fn enter_slot_machine(&mut self, machine: crate::slot_machine::SlotMachine) {
        if self.mode != SceneMode::SlotMachine {
            self.slot_return_mode = self.mode;
        }
        self.slot_machine = Some(machine);
        self.mode = SceneMode::SlotMachine;
    }

    /// Leave the slot machine and restore the interrupted mode, committing
    /// the session's final balance into the casino coin bank
    /// ([`World::casino_coins`] - the retail state-100 assignment
    /// `_DAT_800845A4 = DAT_801d4114`). Returns the session so the host can
    /// read the final state. No-op when the machine isn't active.
    pub fn exit_slot_machine(&mut self) -> Option<crate::slot_machine::SlotMachine> {
        if self.mode == SceneMode::SlotMachine {
            self.mode = self.slot_return_mode;
        }
        let mut machine = self.slot_machine.take();
        if let Some(m) = machine.as_mut() {
            self.casino_coins = m.cash_out().max(0) as u32;
        }
        machine
    }

    /// Arm the mode-24 minigame door-warp: back up the active scene name and
    /// zero the session-winnings accumulator, so [`Self::minigame_return_warp`]
    /// can round-trip back to the departure scene.
    ///
    /// Mirrors the two retail halves of the entry: the field-VM `0x3E` warp
    /// arm zeroes the winnings accumulator `_DAT_80084440`, and the mode-24
    /// OTHER-INIT entry `FUN_80025980` copies the active scene name
    /// `0x80084548` into the backup at `0x8007BAE8` before the minigame
    /// overlay clobbers the field.
    // REF: FUN_80025980 (scene-name backup half), FUN_801DE840 case 0x3E
    //      (winnings-accumulator zero half)
    pub fn arm_minigame_warp(&mut self) {
        self.minigame_scene_backup = Some(self.active_scene_label.clone());
        self.minigame_winnings = 0;
    }

    /// Mode-24 minigame exit / return-warp: restore the backed-up scene name
    /// into [`Self::active_scene_label`], commit the session winnings into
    /// the casino coin bank (`casino_coins += minigame_winnings`, saturating
    /// at the retail `9_999_999` cap), and drop back to [`SceneMode::Field`]
    /// (retail latches `_DAT_8007B83C = 2`, mode 2 MAIN INIT, whose
    /// per-scene initializer reloads the restored scene; the engine keeps
    /// the field state resident underneath its minigame sessions, so
    /// restoring the label + mode completes the same round trip without a
    /// reload).
    ///
    /// Distinct from the slot overlay's cash-out ([`Self::exit_slot_machine`],
    /// an *assignment* into the bank): this commit is a delta-add of the
    /// accumulator (`_DAT_800845A4 += _DAT_80084440`).
    ///
    /// The winnings commit runs even when no warp is armed (retail's add is
    /// unconditional); only the name restore needs the backup.
    // PORT: FUN_80026018
    pub fn minigame_return_warp(&mut self) {
        self.casino_coins = self
            .casino_coins
            .saturating_add(self.minigame_winnings)
            .min(9_999_999);
        if let Some(name) = self.minigame_scene_backup.take() {
            self.active_scene_label = name;
        }
        self.mode = SceneMode::Field;
    }

    /// Advance the slot machine one frame, reading this frame's pad:
    ///
    /// - **Idle**: a [`Cross`](input::PadButton::Cross) press charges the
    ///   flat bet (3 coins, 1 in feature modes) and spins - all three
    ///   paylines always play.
    /// - **Spinning**: the spin-up timer runs down on its own.
    /// - **Stopping**: a [`Cross`] press stops the leftmost live reel (host
    ///   simplification of the retail three stop buttons, pad bits
    ///   `0x80`/`0x40`/`0x20` → reels 0/1/2).
    /// - **Payout**: a [`Cross`] press collects the win into the balance.
    ///
    /// [`Cross`]: input::PadButton::Cross
    ///
    /// PORT: the slot overlay's per-frame driver (`FUN_801cf0d8` reel SM;
    /// the confirmed kernels live in [`crate::slot_machine`]).
    fn tick_slot_machine(&mut self) {
        use crate::slot_machine::SlotPhase;
        let Some(phase) = self.slot_machine.as_ref().map(|m| m.phase()) else {
            // Mode is SlotMachine but no session installed - drop back.
            self.mode = self.slot_return_mode;
            return;
        };
        let confirm = self.input.just_pressed(input::PadButton::Cross);
        let Some(m) = self.slot_machine.as_mut() else {
            return;
        };
        m.tick();
        match phase {
            SlotPhase::Idle => {
                if confirm {
                    m.spin();
                }
            }
            SlotPhase::Spinning => {}
            SlotPhase::Stopping => {
                if confirm {
                    m.stop_next_reel();
                }
            }
            SlotPhase::Payout => {
                if confirm {
                    m.collect();
                }
            }
            SlotPhase::CashedOut => {
                // Committed: restore the interrupted mode (the host reads the
                // session out via [`World::exit_slot_machine`]).
                self.mode = self.slot_return_mode;
            }
        }
    }

    /// Enter the Baka Fighter duel on `fight`, suspending the current scene
    /// mode (restored by [`World::exit_baka_fighter`]). Like the dance /
    /// fishing / slot / pause-menu suspend contract, the interrupted field
    /// state stays intact underneath.
    pub fn enter_baka_fighter(&mut self, fight: crate::baka_fighter::BakaFight) {
        if self.mode != SceneMode::BakaFighter {
            self.baka_return_mode = self.mode;
        }
        self.baka_fighter = Some(fight);
        self.mode = SceneMode::BakaFighter;
    }

    /// Leave the Baka Fighter duel and restore the interrupted mode. On a
    /// decided match with a player win, the beaten opponent's gold prize is
    /// credited into the party gold (the retail end-of-match tally drains
    /// `DAT_801dbee8` into `_DAT_80084440`). Returns the fight so the host
    /// can read the final state. No-op when no duel is active.
    pub fn exit_baka_fighter(&mut self) -> Option<crate::baka_fighter::BakaFight> {
        if self.mode == SceneMode::BakaFighter {
            self.mode = self.baka_return_mode;
        }
        let fight = self.baka_fighter.take();
        if let Some(f) = fight.as_ref()
            && f.winner() == Some(0)
        {
            let new_money = (self.money as i64).saturating_add(f.gold_reward() as i64);
            self.money = new_money.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        }
        fight
    }

    /// Advance the Baka Fighter duel one frame, reading this frame's pad:
    ///
    /// - [`Left`](input::PadButton::Left) / [`Right`](input::PadButton::Right)
    ///   / [`Up`](input::PadButton::Up) commit attack types 1 / 2 / 3 for the
    ///   player slot (retail folds the face/shoulder mask bits
    ///   `0x80`/`0x20`/`0x40` into the same three types);
    ///   [`Down`](input::PadButton::Down) commits the special (type 4).
    /// - The CPU slot picks through the ported `FUN_801d487c` roll inside
    ///   [`crate::baka_fighter::BakaFight::tick`].
    /// - When the match is decided, a [`Cross`](input::PadButton::Cross)
    ///   press leaves the duel (via [`World::exit_baka_fighter`], crediting
    ///   the gold prize on a player win).
    ///
    /// PORT: the Baka Fighter per-frame drive (`FUN_801d3f44` player input →
    /// type commit; `FUN_801d3468` resolution SM via `BakaFight::tick`).
    fn tick_baka_fighter(&mut self) {
        use crate::baka_fighter::BakaAttack;
        let Some(fight) = self.baka_fighter.as_ref() else {
            // Mode is BakaFighter but no fight installed - drop back.
            self.mode = self.baka_return_mode;
            return;
        };
        if fight.match_over() {
            if self.input.just_pressed(input::PadButton::Cross) {
                self.exit_baka_fighter();
            }
            return;
        }
        let attack = if self.input.just_pressed(input::PadButton::Left) {
            Some(BakaAttack::A)
        } else if self.input.just_pressed(input::PadButton::Right) {
            Some(BakaAttack::B)
        } else if self.input.just_pressed(input::PadButton::Up) {
            Some(BakaAttack::C)
        } else if self.input.just_pressed(input::PadButton::Down) {
            Some(BakaAttack::Special)
        } else {
            None
        };
        if let Some(fight) = self.baka_fighter.as_mut() {
            if let Some(attack) = attack {
                fight.choose(0, attack);
            }
            fight.tick(1);
        }
    }

    /// Enter the Muscle Dome contest on `session`, suspending the current
    /// scene mode (restored by [`World::exit_muscle_dome`]). Same suspend
    /// contract as the other minigames / the pause menu.
    pub fn enter_muscle_dome(&mut self, session: crate::muscle_dome::MuscleDomeSession) {
        if self.mode != SceneMode::MuscleDome {
            self.muscle_return_mode = self.mode;
        }
        self.muscle_dome = Some(session);
        self.mode = SceneMode::MuscleDome;
    }

    /// Leave the Muscle Dome and restore the interrupted mode. On a won
    /// contest, the reward Seru is credited through the capture kernel
    /// ([`crate::seru_learning::record_capture`] against the installed
    /// registry, resolved by the reward spell id) - the engine's stand-in
    /// for the retail outright award message. Returns the session so the
    /// host can read the final state.
    pub fn exit_muscle_dome(&mut self) -> Option<crate::muscle_dome::MuscleDomeSession> {
        if self.mode == SceneMode::MuscleDome {
            self.mode = self.muscle_return_mode;
        }
        let session = self.muscle_dome.take();
        if let Some(s) = session.as_ref()
            && s.phase() == crate::muscle_dome::MusclePhase::Won
            && let Some(seru) = self
                .seru_registry
                .seru_for_spell(s.reward_spell_id())
                .map(|d| d.id)
        {
            let party: Vec<u8> = (0..self.roster.members.len() as u8).collect();
            crate::seru_learning::record_capture(
                &self.seru_registry,
                &mut self.seru_log,
                seru,
                &party,
            );
        }
        session
    }

    /// Advance the Muscle Dome one frame, reading this frame's pad:
    ///
    /// - **Select**: [`Left`](input::PadButton::Left) /
    ///   [`Right`](input::PadButton::Right) / [`Up`](input::PadButton::Up) /
    ///   [`Down`](input::PadButton::Down) commit hand cards 0..3 (the retail
    ///   four card-selection direction bits, in the `ctx+0x1114..+0x1120`
    ///   slot order); [`Cross`](input::PadButton::Cross) confirms the queue.
    ///   The opponent commits through the shared selection logic when the
    ///   player confirms.
    /// - **Resolve**: the queues play out. Per-card damage here is a dev
    ///   stand-in for the retail battle-action playback (see the constants) -
    ///   the session's [`resolve_round`] is damage-model-agnostic.
    /// - **RoundOver / decided**: [`Cross`] continues to the next round, or
    ///   leaves a decided contest (via [`World::exit_muscle_dome`], crediting
    ///   the reward Seru capture on a win).
    ///
    /// [`resolve_round`]: crate::muscle_dome::MuscleDomeSession::resolve_round
    ///
    /// PORT: FUN_801d0748 (match SM phase loop: pick / commit / resolve /
    /// score), with the card playback simplified per above.
    fn tick_muscle_dome(&mut self) {
        use crate::muscle_dome::MusclePhase;
        // Dev stand-in stats for the card playback (retail resolves each
        // queued command through the battle-action path against the actor
        // records).
        const PLAYER_ATK: i32 = 60;
        const OPPONENT_ATK: i32 = 50;
        const PLAYER_DEF: i32 = 20;
        const OPPONENT_DEF: i32 = 15;
        let Some(phase) = self.muscle_dome.as_ref().map(|s| s.phase()) else {
            self.mode = self.muscle_return_mode;
            return;
        };
        let confirm = self.input.just_pressed(input::PadButton::Cross);
        match phase {
            MusclePhase::Select => {
                let card = if self.input.just_pressed(input::PadButton::Left) {
                    Some(0)
                } else if self.input.just_pressed(input::PadButton::Right) {
                    Some(1)
                } else if self.input.just_pressed(input::PadButton::Up) {
                    Some(2)
                } else if self.input.just_pressed(input::PadButton::Down) {
                    Some(3)
                } else {
                    None
                };
                if let Some(s) = self.muscle_dome.as_mut() {
                    if let Some(card) = card {
                        s.commit_card(0, card);
                    }
                    if confirm {
                        s.ai_commit_all(1);
                        s.end_selection();
                    }
                }
            }
            MusclePhase::Resolve => {
                if let Some(s) = self.muscle_dome.as_mut() {
                    s.resolve_round(|attacker, _cmd| {
                        if attacker == 0 {
                            (PLAYER_ATK - OPPONENT_DEF).max(1)
                        } else {
                            (OPPONENT_ATK - PLAYER_DEF).max(1)
                        }
                    });
                }
            }
            MusclePhase::RoundOver => {
                if confirm && let Some(s) = self.muscle_dome.as_mut() {
                    s.next_round();
                }
            }
            MusclePhase::Won | MusclePhase::Lost => {
                if confirm {
                    self.exit_muscle_dome();
                }
            }
        }
    }
}
