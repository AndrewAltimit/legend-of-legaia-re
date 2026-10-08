use super::*;

impl World {
    /// One frame of the tile board's walk state machine (`FUN_801EF2B0`,
    /// states in [`crate::tile_board::sm`]):
    ///
    /// - **fade-in** (`1`): input is ignored while `+0x9C` - every tile
    ///   actor's render scale - ramps up to `0x1000`;
    /// - **walking** (`4`, and `2` while the player interpolates toward a
    ///   committed tile, [`crate::world::TileBoardState::target`]): read one
    ///   d-pad direction, gate it against the board's cells, step. The menu
    ///   edge (Triangle, `_DAT_8007B874 & 0x10`) opens the quit prompt with
    ///   the cursor on its second row (`0x801EF824..0x801EF84C`);
    /// - **quit prompt** (`5`): the two-row picker `FUN_801E9DC8` (wrapping,
    ///   Up/Down); confirm on row `0` quits, confirm on row `1` or cancel go
    ///   back to walking;
    /// - **exit** (`6`/`7` -> `9` -> `0xA`, the event exit `0xB` -> `0xC`):
    ///   one tick per step state, then the fade-out shrinks every tile actor
    ///   (the event tile under the player excepted), `0xD` parks them and
    ///   `0xE` tears the board down.
    ///
    /// A direction is only consumed while idle, so holding the d-pad steps
    /// tile-by-tile - retail re-reads the pad only after the previous step's
    /// interpolation completes. The fades count one `DAT_1F800393` unit per
    /// world tick, which is one vsync: the same wall-clock ramp as retail's
    /// per-game-tick `d = 2`.
    ///
    /// No-ops without a player actor slot or an installed
    /// [`tile_board`](crate::tile_board), and while a dialog box is up
    /// (the field VM owns the frame). Deterministic across identical pad
    /// streams.
    ///
    /// PORT: FUN_801EF2B0 (states 1, 2, 4, 5, 6, 7, 9..0xE; the arrival
    /// state 3 and event state 8 are [`Self::tile_board_arrival`])
    pub(super) fn tick_tile_board(&mut self) {
        use crate::tile_board::sm;
        const D: u8 = 1;
        if self.dialogue_owns_input() {
            return;
        }
        let Some(player_slot) = self.player_actor_slot else {
            return;
        };
        let slot = player_slot as usize;
        if self.board.grid.is_none() || slot >= self.actors.len() {
            return;
        }
        match self.board.sm {
            sm::FADE_IN => {
                let (fade, done) = crate::tile_board::fade_in_step(self.board.fade, D);
                self.board.fade = fade;
                if done {
                    self.board.sm = sm::WALK;
                }
                return;
            }
            sm::PROMPT => {
                let nav_in = crate::menu_input::NavButtons::new(
                    self.input.just_pressed(input::PadButton::Cross),
                    self.input.just_pressed(input::PadButton::Circle),
                    self.input.just_pressed(input::PadButton::Up),
                    self.input.just_pressed(input::PadButton::Down),
                );
                let nav = crate::menu_input::menu_cursor_nav(
                    &mut self.board.prompt_cursor,
                    2,
                    true,
                    nav_in,
                );
                if let Some(cue) = nav.sfx_cue() {
                    self.push_sfx_cue(i16::from(cue));
                }
                self.board.sm = crate::tile_board::prompt_next(nav, self.board.prompt_cursor);
                return;
            }
            sm::QUIT | sm::TRIGGER_EXIT => {
                self.board.sm = sm::EXIT_STEP;
                return;
            }
            sm::EXIT_STEP | sm::EVENT_STEP => {
                self.board.sm += 1;
                return;
            }
            sm::FADE_OUT | sm::EVENT_FADE_OUT => {
                let (fade, done) = crate::tile_board::fade_out_step(self.board.fade, D);
                self.board.fade = fade;
                if done {
                    self.board.sm = sm::PARK;
                }
                return;
            }
            sm::PARK => {
                self.board.sm = sm::TEARDOWN;
                return;
            }
            sm::TEARDOWN => {
                self.tile_board_teardown();
                return;
            }
            _ => {}
        }

        // Interpolating toward a committed target tile (state 2).
        if let Some((tx, tz)) = self.board.target {
            self.tile_board_walk_step(slot, tx, tz);
            return;
        }

        // Idle (state 4): the menu edge first, then one direction.
        if self.input.just_pressed(input::PadButton::Triangle) {
            self.board.fade = crate::tile_board::FADE_FULL;
            self.board.prompt_cursor = 1;
            self.board.sm = sm::PROMPT;
            return;
        }
        // The walker's octant store off the cell under the player
        // (`0x801EF8A4..0x801EF8CC`), then the held pad remapped through it
        // (`jal 0x800467E8` at `0x801EF8D0`) and decoded in retail order.
        if let Some(board) = self.board.grid.as_ref() {
            let under = board
                .cell(board.player_col as i32, board.player_row as i32)
                .unwrap_or(0);
            self.locomotion.pad_octant = crate::tile_board::walker_octant(under);
        }
        let held = tile_board_held_mask(&self.input);
        let remapped = Self::remap_pad_direction(held, self.locomotion.pad_octant);
        let Some(dir) = crate::tile_board::step_for_mask(remapped) else {
            return;
        };
        match self.board.grid.as_mut().and_then(|b| b.try_step(dir)) {
            Some((tx, tz)) => {
                // An accepted step: the step cue through the ring's push
                // producer (`jal 0x80035B50` with `0x21`, `0x801EF990`), then
                // the run clip - `_DAT_8007BDD8 = 3`, `+0x5C = leader * 7 +
                // 3`, bind (`0x801EF998..0x801EF9D0`).
                self.push_sfx_cue(crate::tile_board::STEP_SFX);
                self.field_player_strided_clip(vm::field_player_clip::BASE_RUN);
                self.board.target = Some((tx, tz));
                // State 4 stores state 2 and falls straight into it
                // (`sh s3,0x54(s4)` at `0x801EFA84`, then `0x801EFA88`), so
                // the first step moves on the accepting tick.
                self.tile_board_walk_step(slot, tx, tz);
            }
            None => {
                // Off the board or into a wall: the bonk through the ring's
                // overwrite producer (`jal 0x80035BD0` with `0x23`,
                // `0x801EF980`). Retail re-runs state 4 once per game tick
                // with the pad held, so the bonk repeats at that rate; the
                // port's state 4 runs every vsync and fires it on the vsyncs
                // the actor game tick fired.
                if self.clock.actor_vsync_accum == 0 {
                    self.replace_last_sfx_cue(crate::tile_board::BONK_SFX);
                }
            }
        }
    }

    /// One tick of the walker's state 2 (`0x801EFA88`): step toward the
    /// committed target, facing the octant of the remaining delta
    /// (`+0x26 = octant << 9`, `0x801EFB30..0x801EFBCC`); on arrival bind
    /// the idle clip - `_DAT_8007BDD8 = 2`, `+0x5C = leader * 7 + 2`,
    /// `FUN_800204F8` (`0x801EFAC0..0x801EFAEC`) - and run the arrival pass.
    ///
    /// PORT: FUN_801EF2B0 (state 2)
    pub(super) fn tile_board_walk_step(&mut self, slot: usize, tx: i32, tz: i32) {
        let ms = &mut self.actors[slot].move_state;
        let (x, z) = (ms.world_x as i32, ms.world_z as i32);
        if let Some(octant) = crate::tile_board::walker_facing_octant(tx - x, tz - z) {
            ms.render_26 = crate::tile_board::engine_heading_for_octant(octant);
        }
        let nx = step_toward(x, tx, TILE_BOARD_SPEED);
        let nz = step_toward(z, tz, TILE_BOARD_SPEED);
        ms.world_x = nx as i16;
        ms.world_z = nz as i16;
        // The walk is the pad's, not a script's: the clip player keeps the
        // bound bank clip instead of the motion-derived pair.
        if let Some(anim) = &mut self.locomotion.player_anim {
            anim.pad_drove_this_frame = true;
        }
        if nx == tx && nz == tz {
            self.board.target = None;
            self.field_player_strided_clip(vm::field_player_clip::BASE_IDLE);
            self.tile_board_arrival();
        }
    }

    /// The quit prompt's cursor row while the tile board's walk SM sits in
    /// state `5`, for the hosts' panel draw; `None` otherwise.
    pub fn tile_board_prompt_cursor(&self) -> Option<usize> {
        (self.board.grid.is_some() && self.board.sm == crate::tile_board::sm::PROMPT)
            .then_some((self.board.prompt_cursor & crate::menu_input::CURSOR_INDEX_MASK) as usize)
    }

    /// The render scale a board tile of `value` draws at this frame: the
    /// walk SM's fade (`+0x9C` copied into every tile actor's `+0x72`),
    /// except the event tile the player stands on during the exit fade,
    /// which keeps its full size. `1.0` with no board up.
    pub fn tile_board_cell_scale(&self, value: u8) -> f32 {
        use crate::tile_board::sm;
        let Some(board) = self.board.grid.as_ref() else {
            return 1.0;
        };
        if matches!(self.board.sm, sm::FADE_OUT | sm::EVENT_FADE_OUT | sm::PARK) {
            let under = board.cell(board.player_col as i32, board.player_row as i32);
            if crate::tile_board::fade_exempt_value(under) == Some(value) {
                return 1.0;
            }
        }
        f32::from(self.board.fade) / f32::from(crate::tile_board::FADE_FULL)
    }

    /// Advance the screen-effect widgets one frame and refresh
    /// [`crate::world::ScreenFxState::fx_frame`]. Runs in the Field / Cutscene tick after
    /// the script step (so a sub-op spawned this frame draws this frame,
    /// matching retail's actor-pool order). The engine ticks the widget
    /// clocks by 1 per world tick (retail's per-frame byte
    /// `DAT_1F800393`); the sprite scripts' flag waits probe the shared
    /// system flag bank ([`Self::system_flag_test`], `FUN_8003CE64`).
    pub(super) fn tick_screen_fx(&mut self) {
        if !self.presentation.fx.is_active() {
            if !self.presentation.fx_frame.is_empty() {
                self.presentation.fx_frame = Default::default();
            }
            return;
        }
        let mut fx = std::mem::take(&mut self.presentation.fx);
        self.presentation.fx_frame = fx.tick(1, |idx| self.system_flag_test(idx));
        self.presentation.fx = fx;
    }

    /// Walk-SM arrival pass (state 3 of `FUN_801EF2B0`, `0x801EF6FC`), run
    /// when the player's interpolation reaches the committed tile centre. The
    /// arrived cell picks one of three arms ([`crate::tile_board::arrival_action`]):
    ///
    /// - a **trigger cell** (`7`) starts the exit with no flag write
    ///   (state `7`);
    /// - an **event cell** (`8..=0xA`) first writes the state-8 system flags
    ///   ([`crate::tile_board::event_cell_flag_writes`]: SET `A + v + 1`, and
    ///   SET `A` when TEST `B + v` is clear, `v = cell - 8`, `A`/`B` the header
    ///   `+7`/`+9` bases), then starts the event exit (state `0xB`);
    /// - any other cell advances **every** animated cell on the board one step
    ///   (`0xB -> 0xC -> 0xD -> 0xE -> 0xB`) and returns to input.
    ///
    /// The exits run the fade in [`Self::tick_tile_board`] and end in
    /// [`Self::tile_board_teardown`].
    ///
    /// PORT: FUN_801EF2B0 (states 3 and 8)
    pub(super) fn tile_board_arrival(&mut self) {
        use crate::tile_board::ArrivalAction;
        let Some(board) = self.board.grid.as_mut() else {
            return;
        };
        let (col, row) = (board.player_col as i32, board.player_row as i32);
        let Some(cell) = board.cell(col, row) else {
            return;
        };
        match crate::tile_board::arrival_action(cell) {
            ArrivalAction::Continue => {
                crate::tile_board::advance_animated_cells(&mut board.cells);
            }
            ArrivalAction::ExitTrigger => self.board.sm = crate::tile_board::sm::TRIGGER_EXIT,
            ArrivalAction::ExitEvent => {
                if let Some(header) = self.board.header {
                    let writes = crate::tile_board::event_cell_flag_writes(&header, cell, |i| {
                        self.system_flag_test(i)
                    });
                    for idx in writes {
                        self.system_flag_set(idx);
                    }
                }
                self.board.sm = crate::tile_board::sm::EVENT_STEP;
            }
        }
    }

    /// Teardown (state `0xE`, `0x801EFE64`): the board and its tile actors go,
    /// and `tile_board_armed` stays set, so the suspended op-0x49 script
    /// reads `Done` and resumes past the install op - the engine form of
    /// retail zeroing the controller's `+0x3E`, which the op-49 handler reads
    /// to raise `_DAT_8007B450 = 1`.
    pub(super) fn tile_board_teardown(&mut self) {
        // Put the incoming pad-rotation octant back (`0x801EFE7C`).
        self.locomotion.pad_octant = self.board.saved_octant;
        self.board.grid = None;
        self.board.header = None;
        self.board.target = None;
        self.board.sm = crate::tile_board::sm::WALK;
        self.board.fade = crate::tile_board::FADE_FULL;
        self.despawn_tile_actors();
    }

    /// Install the **demo** tile board: a 7x7 board centred on the player's
    /// tile, through the same op-`0x49` sub-op-5 bytecode a script would hand
    /// [`Self::try_install_tile_board`]. No retail scene installs a board, so
    /// this developer trigger is the only way a host reaches the per-cell draw
    /// pass. Both hosts call it (native under `LEGAIA_TILE_BOARD_DEMO=1`, the
    /// page from `play_install_demo_tile_board`); they decide only *when*.
    ///
    /// Returns `false` off the field, with a board already up or armed, with
    /// no player actor, or when the install is refused.
    pub fn install_demo_tile_board(&mut self) -> bool {
        if self.mode != crate::world::SceneMode::Field
            || self.board.grid.is_some()
            || self.board.armed
        {
            return false;
        }
        let Some(pslot) = self.player_actor_slot else {
            return false;
        };
        let Some(actor) = self.actors.get(pslot as usize) else {
            return false;
        };
        let (px, pz) = (
            i32::from(actor.move_state.world_x),
            i32::from(actor.move_state.world_z),
        );
        let origin_x = ((px >> 7) - 3).clamp(0, 255) as u8;
        let origin_z = ((pz >> 7) - 3).clamp(0, 255) as u8;
        let instr: [u8; 14] = [
            0x49, 0x05, // op, sub-op
            origin_x, origin_z, // +1/+2 tile origin
            7, 7, // +3/+4 width x height
            5, // +5 draw radius
            0, // +6 mode flag (full-board draw)
            0, 0, 0, 0, // +7/+9 event-flag bases (unused by the demo)
            0, // +0xb player template (character-mesh head)
            3, // +0xc tile template base (effect-model library)
        ];
        self.try_install_tile_board(&instr)
    }

    /// Install a tile board from a field-VM op-0x49 **sub-op 5** instruction
    /// (`instr` = the bytes from the opcode onward, as handed to
    /// `FieldHost::op49_menu_request`). Parses the 13-byte inline header
    /// (`instr[1..]`, the window retail points `_DAT_8007b450` at), fills the
    /// cells with the retail procedural fill (`overlay_0897_801e0b1c`, seeded
    /// from the world RNG the way retail seeds from BIOS `rand`), puts the
    /// player's cell at column 4, row 0 and aims the walk-in at its centre,
    /// saves the pad-rotation octant, and holds the script suspended
    /// (`tile_board_armed`) until the board exits.
    ///
    /// Returns `false` (leaving the op merely suspended, matching the other
    /// op-49 consumers) when a board is already up or the header is
    /// malformed.
    ///
    /// PORT: FUN_801ef2b0 (the board alloc + fill arm at `0x801EF334`, an
    /// interior label of the walk SM; cells only - the per-cell tile-actor
    /// spawns are a renderer concern. `0x801E0B1C` is that arm printed
    /// `0xE818` low, not a function.)
    /// REF: overlay_0897_801de840 (op 0x49 arm, `_DAT_8007b450 = pbVar47`)
    pub fn try_install_tile_board(&mut self, instr: &[u8]) -> bool {
        if self.board.armed || self.board.grid.is_some() {
            return false;
        }
        let Some(window) = instr.get(1..) else {
            return false;
        };
        let Some(header) = crate::tile_board::TileBoardHeader::parse(window) else {
            return false;
        };
        let cells =
            crate::tile_board::procedural_fill(header.width, header.height, || self.next_rand());
        let mut board = crate::tile_board::TileBoard::from_header(&header, cells);
        // State 0 seats the player's cell at column 4, row 0; the actor
        // walks there after the fade-in (below).
        board.player_col = crate::tile_board::START_COL;
        board.player_row = crate::tile_board::START_ROW;

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
        // `+0xb`). The engine reuses the existing player actor and binds its
        // mesh from `player_template` when the global TMD pool carries it
        // (else keeps the field mesh). It is **not** seated: retail leaves
        // it where it stands and aims the walk-in at the start cell's centre
        // (`DAT_801F35D0/D4`), which state 2 walks to once the fade-in ends.
        if let Some(slot) = self.player_actor_slot {
            tile_slots[0] = Some(slot);
            let player_tmd = self.global_tmd(header.player_template as i16).cloned();
            if let Some(a) = self.actors.get_mut(slot as usize)
                && let Some(tmd) = player_tmd
            {
                a.tmd_ref = Some(tmd);
            }
        }
        let walk_in = board.player_world();

        // State 0's tail clears the header's four set-base flags
        // (`FUN_8003CE34(A + i)`, `i < 4`) before the fade-in starts.
        for i in 0..4 {
            self.system_flag_clear((header.flag_base_set as u16).wrapping_add(i));
        }
        self.board.actor_slots = tile_slots;
        self.board.target = Some(walk_in);
        // Save the incoming pad-rotation octant (`0x801EF320`).
        self.board.saved_octant = self.locomotion.pad_octant;
        self.board.grid = Some(board);
        self.board.header = Some(header);
        self.board.armed = true;
        self.board.sm = crate::tile_board::sm::FADE_IN;
        self.board.fade = 0;
        true
    }

    /// Despawn the tile-board tile actors (the `2..=14` entries of the
    /// tile-actor table) and clear the table + draw list. The player actor
    /// (table slot 0) outlives the board and is left in place. Called on
    /// board teardown so tile actors don't leak into the next scene.
    ///
    /// PORT: the walk-SM board-exit teardown (`overlay_0897_801ef2b0`
    /// case 8 -> board free).
    pub(super) fn despawn_tile_actors(&mut self) {
        for value in crate::tile_board::CELL_DRAW_FIRST..=crate::tile_board::CELL_DRAW_LAST {
            if let Some(slot) = self.board.actor_slots[value as usize]
                && let Some(a) = self.actors.get_mut(slot as usize)
            {
                *a = Actor::new();
            }
        }
        self.board.actor_slots = [None; crate::tile_board::TILE_ACTOR_TABLE_LEN];
        self.board.draw_list.clear();
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
    pub(super) fn refresh_tile_board_draw_list(&mut self) {
        let Some(header) = self.board.header else {
            self.board.draw_list.clear();
            return;
        };
        let Some(board) = self.board.grid.as_ref() else {
            self.board.draw_list.clear();
            return;
        };
        let mut list = Vec::new();
        // State 0xD parks every tile actor off-board but the event tile the
        // player stands on, and 0xE draws nothing (`0x801EFDA8`, the render
        // tail's state gate).
        let parked = match self.board.sm {
            crate::tile_board::sm::PARK => Some(crate::tile_board::fade_exempt_value(
                board.cell(board.player_col as i32, board.player_row as i32),
            )),
            crate::tile_board::sm::TEARDOWN => Some(None),
            _ => None,
        };
        for (col, row) in board.draw_cells(header.mode_flag, header.radius) {
            let Some(cell) = board.cell(col, row) else {
                continue;
            };
            if !crate::tile_board::is_drawable_cell(cell) {
                continue;
            }
            if let Some(keep) = parked
                && keep != Some(cell)
            {
                continue;
            }
            let Some(slot) = self.board.actor_slots[cell as usize] else {
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
        self.board.draw_list = list;
    }
}
