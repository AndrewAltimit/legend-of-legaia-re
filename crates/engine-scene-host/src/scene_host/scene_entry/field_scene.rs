//! `SceneHost::enter_field_scene` and the two installers it owns: field props and scene shops.

use super::*;

impl SceneHost {
    /// Install the placed-prop collision + interaction layer for the current
    /// field scene: one [`legaia_engine_core::world::FieldPropCollider`] per placed `.MAP`
    /// object (retail's collision candidate list, `FUN_801CF754` - **solid by
    /// default**, a closed door blocks), classed by its bind record's
    /// spawn-prologue `0x31` ops (`31 1E` = interact-gated cupboard class,
    /// `31 00` = born collision-exempt), plus the
    /// [`legaia_engine_scene::field_env::PropAnimBank`] whose entries carry each posed
    /// prop's clip runtime and its bind record - the script a touch /
    /// interact runs through the field VM.
    ///
    /// REF: FUN_8003A55C, FUN_801CF754, FUN_801CFC40
    fn install_field_props(
        &mut self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        man_bytes: &[u8],
    ) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let placements = match scene.field_object_placements(&self.index) {
            Ok(Some(p)) => p,
            Ok(None) => return,
            Err(err) => {
                eprintln!("[scene] field prop-collider load skipped: {err:#}");
                return;
            }
        };
        let binds = match scene.field_object_binds(&self.index) {
            Ok(b) => b.unwrap_or_default(),
            Err(err) => {
                eprintln!("[scene] field object-bind load skipped: {err:#}");
                Default::default()
            }
        };
        // The scene ANM bundle resolves each posed prop's clip metadata
        // (frame count + step scaling) for the bank's end-latch timing.
        let bundle = legaia_engine_scene::npc_catalog::scene_anm_bundle(scene);
        let clip = |anim: u8| -> Option<(u16, bool, u8)> {
            let b = bundle.as_ref()?;
            let r = b.record(anim.checked_sub(1)? as usize).ok()?;
            Some((r.frame_count, (r.a >> 8) & 1 != 0, (r.flag & 0xFF) as u8))
        };
        // Each object's spawn pass runs against the live story flags, and its
        // flag writes land (`FUN_8003A55C` hands the prologue to the field-VM
        // dispatcher): `korb3`'s collection stones park on the `0x21` their
        // touch resumes from, or set their `0x43E..` flags, by `0x403`.
        let world = &mut self.world;
        let mut flags = |idx: u16, kind: legaia_scene_formats::field_disasm::FlagKind| -> bool {
            use legaia_scene_formats::field_disasm::FlagKind;
            match kind {
                FlagKind::Test => world.system_flag_test(idx),
                FlagKind::Set => {
                    world.system_flag_set(idx);
                    false
                }
                FlagKind::Clear => {
                    world.system_flag_clear(idx);
                    false
                }
            }
        };
        let bank = legaia_engine_scene::field_env::PropAnimBank::build_with_flags(
            &placements,
            &binds,
            man_file,
            man_bytes,
            clip,
            Some(&mut flags),
        );
        self.world.props.bank = bank;
        self.world.props.object_shifts.clear();
        self.world.props.colliders = placements
            .iter()
            .map(|p| {
                let anchor = (p.anchor_col, p.anchor_row);
                let bind = binds.get(&anchor);
                let cflags = bind
                    .map(|b| {
                        legaia_engine_scene::field_env::record_spawn_cflags(
                            man_file,
                            man_bytes,
                            b.record as usize,
                        )
                    })
                    .unwrap_or(0);
                // Only the scene-init sweep's actors collide. The walk
                // controller hands `FUN_801CFE4C` / `FUN_801CFC40` the actor
                // list at the scene control block's `+0x0C` (`lw s6,
                // -0x3CAC(v0)` = `0x8007C354` at `0x801D0344`), and the
                // candidate gather `FUN_801CF754` walks that one list. The
                // window sweep (`FUN_801D7B50`) spawns every placement whose
                // anchor cell lacks `CELL_BIND_OWNED` onto its own list at
                // `+0x24`, which no collision routine reads - so those
                // placements are scenery the player walks through. Octam's
                // gondola (`ropeway`) is one: `P2[6]` seats the player on its
                // footprint (`A3 F8 24 1F`), and a solid box there boxed the
                // player in on all four probes once the cutscene ended.
                // REF: FUN_801CF754, FUN_801D7B50, FUN_801D01B0
                let init_owned =
                    p.anchor_cell & legaia_engine_field::field_regions::CELL_BIND_OWNED != 0;
                legaia_engine_core::world::FieldPropCollider {
                    anchor: bind.map(|_| anchor),
                    center: (p.collider_x, p.collider_z),
                    live: (p.world_x, p.world_z),
                    moving_box: cflags & 0x0102_0000 != 0,
                    interact: cflags & 0x4002_0000 != 0,
                    solid: init_owned && cflags & 3 == 0,
                }
            })
            .collect();
    }

    pub fn enter_field_scene(&mut self, name: &str, record_index: usize) -> Result<()> {
        self.load_scene(name)?;
        // Sparring-tutorial prompt corpus (PROT 0967). Unconditional and
        // scene-independent: the tutorial's trigger is a disc flag any scene's
        // script could raise, so the text has to be resident before the first
        // battle rather than gated on a scene name.
        self.ensure_battle_tutorial_script();
        // Free-roam picker staging: every scene entered while staging is on
        // re-arms the entry window the BGM pause-drop measures against (a
        // door walked through in free roam runs the next scene's entry
        // script, whose authored pause has the same no-choreography problem
        // as the picked scene's).
        if self.world.field_vm.free_roam_staging {
            self.world.field_vm.free_roam_entry_frame = self.world.clock.display_frames;
        }
        // Drop any cutscene timeline from a previous scene; only `opdeene`
        // re-installs one below, so it must not leak into the scene we hand off
        // to (Rim Elm). The per-actor channels are timeline-scoped and drop
        // with it.
        self.world.cutscene.timeline = None;
        // Concurrent spawned-record contexts (and any not-yet-drained op-0x44
        // requests) are scene-scoped like the timeline: their bytecode slices
        // came from the previous scene's MAN.
        self.world.field_vm.helper_contexts.clear();
        self.world.field_vm.pending_record_spawns.clear();
        // So is a conversation or prop record run: the talker or the touched
        // object is an actor of the departing scene's list, which the load
        // retires with the dialog SM context stepping its record. Carried
        // across, the run parks for good on a wait no actor of the new scene
        // answers (a cupboard's `2D 08` end-latch, a talker's actor op) and
        // holds the pad (a Door of Light or Wind art that lands while a
        // cupboard or a conversation is open). The player's engaged bit the
        // touch raised drops with the hop lock below. Battle entry drops the
        // same pair.
        // REF: FUN_80039B7C, FUN_801D5B5C
        self.world.dialog.inline = None;
        self.world.dialog.current = None;
        self.world.cutscene.card = None;
        // Drop the previous scene's caption image (only `opdeene` re-decodes one
        // below); reset its fade + hold so a re-entry starts hidden.
        self.world.cutscene.caption = None;
        self.world.cutscene.caption_alpha = 0.0;
        self.world.cutscene.caption_shown_frames = 0;
        self.world.field_vm.channels.clear();
        self.world.field_vm.object_slot_ramps.clear();
        self.world.field_vm.actor_tints.clear();
        self.world.field_vm.player_field_42 = 0;
        self.world.field_vm.stepping_view.clear();
        self.world.field_vm.channels_man = None;
        self.world.npcs.anim_cues.clear();
        self.world.npcs.morphs.clear();
        self.world.npcs.object_pack_slots.clear();
        self.world.npcs.morph_dirty.clear();
        self.world.npcs.clip_current.clear();
        self.world.npcs.clip_bones.clear();
        // An in-flight ledge hop is scene-scoped, and its steering lock is
        // one-way: `start_field_ledge_hop` ORs `0x0008_0000` into the player's
        // `move_state.flags` (retail `0x801D25A8..0x801D25B8` on the player
        // context's `+0x10`) and only the hop phase machine's end arm clears
        // it. A scene change that lands while the hop is still in flight tears
        // the phase machine down without that arm ever running, so the lock
        // survives into the next scene - and `step_field_locomotion` returns
        // early on it, which means the player never walks again for the rest
        // of the session. Retail cannot reach the state (the hop always
        // finishes before a transition); the port can, so scene entry drops
        // the hop and its lock together.
        self.world.locomotion.ledge_hop = None;
        // Likewise the kind-0 warp and the clip base: a warp does not carry
        // across a scene change, and the base starts at idle.
        self.world.reset_field_warp_and_clip();
        if let Some(slot) = self.world.player_actor_slot
            && let Some(actor) = self.world.actors.get_mut(slot as usize)
        {
            actor.move_state.flags &= !0x0008_0000;
        }
        // Reset the persistent op-0x45 camera-param set so a new scene's
        // cutscene shots start clean (the params now MERGE per-slot across beats
        // in `camera_configure`, so a stale set would leak the prior scene's
        // focus / depth into a beat that omits those slots).
        self.world.camera.state.params.clear();
        self.world.camera.state.glide_frames = 0;
        // The op-0x34 screen effect is scene-scoped (the opening timeline's
        // between-beat black fades); retire its tween and forget the slot.
        // Retail reaches the same state through the transition sweep, which
        // names the tween's handler among its three retire classes. The
        // op-0x4C-0x12 global screen tint (`World::presentation.tint`) deliberately
        // PERSISTS - retail's cross-scene fade continuity: a departure
        // fade-to-black carries into the next scene, whose `P1[0]` arrival
        // arm fades back in.
        self.world.presentation.effect_tween_slot = None;
        self.world.retire_actors_by_handler(
            legaia_engine_field::actor_handler::ActorHandler::ColourTween,
        );
        // The MAN loader zeroes the clear colour on every scene load
        // (`FUN_8003AEB0`, `0x8003B470..0x8003B48C`); the scene's own
        // `4C 13` sets it again.
        self.world.presentation.clear_rgb = [0; 3];
        self.world.presentation.clear_ramp = None;
        // The same loader resets the field light - angle trio and back
        // colour (`0x8003B4B4..0x8003B4D8`); a scene's own `4C 8A` sets it.
        self.world.presentation.field_light =
            legaia_render_kernels::field_light::FieldLight::SCENE_LOAD;
        // Scripted CLUT-cell effects are scene-scoped (their cell operands
        // came from the previous scene's MAN); drop any in flight and re-pin
        // the frame-step factor `dt` (retail `DAT_1F800393`, the adaptive
        // vsyncs-per-game-tick factor the frame-flip path `FUN_80016B6C`
        // writes): live poll baselines run field/town scenes at 2 (30 fps)
        // and the overworld kingdom scenes (`mapNN`) at 3 (20 fps). See
        // `World::clock.frame_step`.
        self.world.ambient.clut_fx.clear();
        self.world.ambient.clut_blend_fx.clear();
        self.world.ambient.clut_vsync_accum = 0;
        self.world.ambient.clut_pending_game_ticks = 0;
        // Ambient move-VM effect parts (jou's flesh cyclers / lightning) are
        // per-scene; the auto-spawn below re-seeds the new scene's tree.
        self.world.ambient.fx.clear();
        self.world.ambient.vsync_accum = 0;
        self.world.ambient.pending_game_ticks = 0;
        self.world.ambient.cell_captures.clear();
        self.world.ambient.flash_applied.clear();
        // Same scene-scoping for the sibling `4C 60` MoveImage stamps: any
        // still-queued rect operands belong to the previous scene's MAN.
        self.world.ambient.script_vram_moves.clear();
        self.world.ambient.script_vram_stp.clear();
        // Retail installs this as the per-mode floor `DAT_8007B9D8`; the
        // adaptive resolver (`World::resolve_frame_step`) can only raise it.
        self.world.clock.frame_step_floor = if legaia_engine_scene::scene::is_world_map_scene(name)
        {
            3
        } else {
            2
        };
        self.world.clock.frame_step = self.world.clock.frame_step_floor;
        let (record_bytes, stager_entry_bytes): (Vec<u8>, Vec<u8>) = {
            let scene = self
                .scene
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("scene was not loaded"))?;
            let scripts = scene
                .find_event_scripts()
                .ok_or_else(|| anyhow::anyhow!("scene '{}' has no event scripts", name))?;
            let record = scripts.record(record_index).ok_or_else(|| {
                anyhow::anyhow!(
                    "record index {} out of range (scene has {} records)",
                    record_index,
                    scripts.len()
                )
            })?;
            (record.to_vec(), scripts.bytes.to_vec())
        };
        self.world.mode = legaia_engine_mode::mode::SceneMode::Field;
        // Cold-boot default init: entering a scene with no party / save ever
        // loaded seeds the retail new-game defaults (template party, starting
        // bag, starting gold) when the host carries them, so the pause menu
        // always has valid data to read. The roster guard inside makes this a
        // no-op on every later entry (door transitions, save-loaded sessions).
        if let Some(defaults) = self.new_game_defaults.as_ref() {
            // The starting-party template is a disc table, not New Game
            // state: a member who joins after a card load takes their
            // level-1 record from it (`World::set_active_party`), and the
            // roster guard below skips the seed on exactly those sessions.
            // Installed here it reaches every host that carries the
            // defaults, the browser page included.
            if self.world.tables.starting_party.is_none() {
                self.world.tables.starting_party = Some(defaults.party.clone());
            }
            self.world.seed_cold_boot_defaults(defaults);
        }
        // Prescript-record-as-field-VM is a MAN-less fallback only: the
        // consumer census resolved every prescript record as a move-VM
        // stager (record 0 = the master ambient record), with no retail
        // field-VM reader (`find_event_scripts` docs). Scenes with a bundle
        // MAN get the real partition-1 system script loaded below, which
        // replaces this buffer.
        self.world.load_field_record(&record_bytes);
        // Install the scene's move-VM stager table (the same prescript bundle,
        // read as `FUN_800252EC` stager records) so field-VM op 0x34 sub-3 can
        // spawn 3D-anim effects via `World::spawn_field_stager`.
        self.world.install_field_stagers(&stager_entry_bytes);
        // Configure the party leader (slot 0) as the free-movement player.
        // (This also clears the collision grid; we repopulate it below.)
        // Mirrors the retail scene-entry player setup in `FUN_8003aeb0`.
        self.world.install_field_player(0);
        // Field entry seat, through the field initialiser's own placement
        // kernel (`legaia_engine_system::mode_entry_init::field_spawn`, PORT: FUN_801D6704).
        //
        // Retail's seat is the **destination-entry operand** `_DAT_80073EF4` /
        // `_DAT_80073EF8`, written by whatever requested the scene change (a
        // door's `0x3F`, the world-map arrival kernel, the New Game seed's
        // `0xE40` / `0x2DC0`) and read back on BOTH arms of the initialiser.
        // That is what `field_spawn`'s `anchor` is, and what
        // `FieldSpawn::player` returns; `FieldSpawn::extra_actor` is the
        // ambient emitter's spawn (template `0x801F271C`), which the port used
        // to read as the player's.
        //
        // With no operand - only the engine's scene picker, which enters
        // scenes nobody walked into - the seat is the engine's own synthesis:
        // the camera-window centre `FIELD_COLD_SPAWN_XZ`, then relocated by
        // `World::resolve_cold_field_spawn` onto a standable, reachable tile
        // in the scene's main region (it keeps `0xA40` wherever that already
        // qualifies, so town01's New Game opening is unchanged).
        // PORT: FUN_801D6704 (the entry seat)
        // A New Game's seed operand stands in for a host-armed one on the
        // prologue scene's entry (a host-armed operand is the later write and
        // wins), and is spent by whichever entry comes first.
        let new_game_seat = self.world.cutscene.new_game_entry_seat.take();
        let operand = self.pending_entry_seat.take().or_else(|| {
            new_game_seat.filter(|_| name == legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE)
        });
        // A scene-picker entry's ranked door arrivals
        // (`SceneHost::arm_picker_entry_seat`), spent by this entry whether or
        // not an operand overrides them.
        let picker_arrivals = self.pending_picker_arrivals.take();
        self.picker_seat = None;
        let cold = legaia_engine_system::mode_entry_init::field_spawn(
            legaia_engine_system::mode_entry_init::FieldEntryMode::Cold,
            operand.unwrap_or((0, 0)),
            (0, 0),
            false,
            (0, 0),
        );
        let (seat_x, seat_y, seat_z) = match operand {
            Some(_) => (cold.player.0, 0, cold.player.1),
            None => legaia_engine_system::mode_entry_init::FIELD_COLD_SPAWN,
        };
        if let Some(player) = self.world.actors.get_mut(0) {
            player.move_state.world_x = seat_x;
            player.move_state.world_y = seat_y;
            player.move_state.world_z = seat_z;
        }
        // The same initialiser's OTHER spawn: `FieldSpawn::extra_actor` is the
        // ambient particle emitter (`FUN_80024C88(&pos, 0x801F271C, pool)` at
        // `0x801D6FD8`, then `+0x1A = 1` for the scene arm). It is a cold-entry
        // spawn only - a warp back from a battle, a minigame or an FMV skips
        // it - and it lands with its master gate `_DAT_8007B854` clear, so it
        // emits nothing until a script raises the gate through op `0x4C`
        // nibble 3 sub-0. The span is the field draw context's own window; op
        // `0x46` retunes it per scene.
        let (wx0, wz0, wx1, wz1) = legaia_engine_system::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW;
        self.world.install_field_scene_elements(
            legaia_engine_system::mode_entry_init::FieldEntryMode::Cold,
            legaia_engine_field::ambient_emitter::SceneSpan {
                x_min: wx0,
                y_min: wz0,
                x_max: wx1,
                y_max: wz1,
            },
        );
        // Load the per-scene base collision/floor grid from the field map
        // file (retail `DATA\FIELD\<scene>.MAP`, the unique 0x12000-byte block
        // entry). The grid is the file's `+0x4000..+0x8000` region; the
        // field-VM `0x4C` nibble-7 ops layer story-conditional deltas on top
        // as the prescript runs. Verified byte-exact against live RAM
        // (town01). See `docs/subsystems/field-locomotion.md`.
        let base_grid: Option<Vec<u8>> = match self.scene.as_ref() {
            Some(scene) => match scene.field_collision_grid(&self.index) {
                Ok(grid) => grid,
                Err(err) => {
                    eprintln!("[scene] field collision-grid load skipped: {err:#}");
                    None
                }
            },
            None => None,
        };
        if let Some(grid) = base_grid {
            self.world.load_field_collision_grid(&grid);
        }
        // The floor sampler's second input: the `.MAP` object-grid cell words
        // (`+0x8000`) and the kind-2 elevation-override records (the `+0x10000`
        // trigger block's third sub-table, plus the `+0x12000` fallback block
        // the retail loader reads contiguously). Together they carry the
        // scene's ramps and staircases - a tile with the `0x800` cell bit takes
        // its height from its override record, NOT from the collision grid's
        // corner nibbles (see `World::sample_field_floor_height`). Installed
        // unconditionally (cleared when absent) so a stale scene's ramps never
        // leak across a transition.
        let map_bytes: Option<Vec<u8>> = self.scene.as_ref().and_then(|scene| {
            let idx = scene.field_map_index(&self.index)?;
            match self.index.entry_bytes_extended(idx) {
                Ok(bytes) => Some(bytes),
                Err(err) => {
                    eprintln!("[scene] field elevation-override load skipped: {err:#}");
                    None
                }
            }
        });
        match map_bytes {
            Some(mut bytes) => {
                // `MAIN_INIT` runs the grid-mark refresh over the freshly
                // streamed `.MAP` before anything reads its cells
                // (`jal 0x80017BEC` at `0x801D6BF8`, unconditional). The
                // disc's cells are already stamped almost everywhere, but not
                // everywhere: `retona`'s tile `(0x1D, 0x18)` carries
                // `0x3000` in a live capture and not on the disc.
                // PORT: FUN_80017BEC (scene-load call site)
                legaia_engine_field::field_regions::refresh_object_grid_marks(&mut bytes);
                // The object-descriptor region (`+0x0000..0x4000`) stays
                // resident: the sub-area window sweep indexes it on every
                // camera re-centre, not just at load.
                self.world.load_field_object_descriptors(
                    bytes
                        .get(..legaia_engine_field::field_regions::MAP_WALK_GRID_OFFSET)
                        .unwrap_or_default(),
                );
                self.world.load_field_object_cells(
                    bytes
                        .get(legaia_scene_formats::field_objects::OBJECT_GRID_OFFSET..)
                        .unwrap_or_default(),
                );
                self.world.load_field_elevation_overrides(
                    bytes
                        .get(legaia_engine_field::field_regions::MAP_REGION_BLOCK_OFFSET..)
                        .unwrap_or_default(),
                    bytes
                        .get(legaia_engine_field::field_regions::MAP_TRIGGER_FALLBACK_OFFSET..)
                        .unwrap_or_default(),
                );
            }
            None => {
                self.world.terrain.object_cells.clear();
                self.world.terrain.elevation_overrides.clear();
                self.world.load_field_object_descriptors(&[]);
            }
        }
        // Resolve the provisional cold spawn against the just-loaded collision
        // grid + object cells - but ONLY when this entry carried no
        // destination-entry operand. An operand is a retail-authored arrival
        // spot; second-guessing it with the picker's own heuristic would move
        // a door arrival off the door. `resolve_cold_field_spawn` returns the
        // seat unchanged when it is standable, inside the scene's largest
        // connected open-floor region, AND not on a kind-0 intra-scene
        // teleport tile (a door pad - spawning on one warps the player on the
        // first tile-crossing dispatch), so town01's opening stays
        // byte-identical; every other scene relocates to a retail-authored
        // kind-0 door-arrival anchor in that region, or to the region's
        // centroid, and snaps Y onto that spot's floor so the player isn't
        // left floating / sunk before the first locomotion step. Only
        // overrides when the resolver actually moved the spawn, keeping the
        // seed (incl. `world_y = 0`) intact otherwise.
        let teleport_tiles: Vec<(u8, u8)> = self
            .field_intra_teleports
            .0
            .iter()
            .chain(self.field_intra_teleports.1.iter())
            .map(|t| (t.tile_x, t.tile_z))
            .collect();
        let anchors: Vec<(i16, i16)> = self
            .field_intra_teleports
            .0
            .iter()
            .chain(self.field_intra_teleports.1.iter())
            .map(|t| t.dest_world())
            .collect();
        // A picker entry first tries where the disc's doors land the party
        // (`World::picker_arrival_seat`); an arrival the scene's grid rejects,
        // or a scene no door leads into, falls through to the cold resolver.
        let picker_arrival = match (operand, picker_arrivals.as_deref()) {
            (None, Some(ranked)) if !ranked.is_empty() => {
                let spots: Vec<(i16, i16)> = ranked.iter().map(|a| a.world_xz()).collect();
                // Never seat a picker visit on a tile the first tick's
                // stale-tile compare would fire: a kind-0 pad or a gate-1
                // walk-on trigger (an exit, or the arrival's cutscene beat).
                let mut avoid = teleport_tiles.clone();
                avoid.extend(
                    self.field_triggers
                        .0
                        .iter()
                        .chain(self.field_triggers.1.iter())
                        .filter(|t| t.gate == 1)
                        .map(|t| (t.tile_x, t.tile_z)),
                );
                self.world
                    .picker_arrival_seat(&spots, &avoid)
                    .map(|(i, seat)| (ranked[i].clone(), seat, i))
            }
            _ => None,
        };
        let resolved = match (operand, &picker_arrival) {
            (Some(seat), _) => seat,
            (None, Some((_, seat, _))) => *seat,
            (None, None) => self
                .world
                .resolve_cold_field_spawn(&teleport_tiles, &anchors),
        };
        if operand.is_none()
            && let Some(ranked) = picker_arrivals.as_deref()
        {
            self.picker_seat = Some(match &picker_arrival {
                Some((arrival, seat, rejected)) => {
                    // The door's facing: armed for the entry script's
                    // `4C 3A` and applied now, as the `0x3F` arrival does.
                    self.world.face_player_sector(arrival.dir);
                    crate::scene_host::PickerSeat::Arrival {
                        arrival: arrival.clone(),
                        seat: *seat,
                        rejected: *rejected,
                    }
                }
                None => crate::scene_host::PickerSeat::ColdFallback {
                    arrivals: ranked.len(),
                    seat: resolved,
                },
            });
        }
        // Remembered for the helper-context teardown rescue: a spawned
        // record that ends with the player inside a wall re-seats them here
        // (see `World::step_helper_contexts`).
        self.world.props.resolved_cold_spawn = Some(resolved);
        if operand.is_none()
            && resolved
                != (
                    legaia_engine_core::world::FIELD_COLD_SPAWN_XZ,
                    legaia_engine_core::world::FIELD_COLD_SPAWN_XZ,
                )
        {
            let floor_y = self
                .world
                .sample_field_floor_height(resolved.0 as i32, resolved.1 as i32)
                as i16;
            if let Some(player) = self.world.actors.get_mut(0) {
                player.move_state.world_x = resolved.0;
                player.move_state.world_z = resolved.1;
                player.move_state.world_y = floor_y;
            }
        }
        // Per-scene region / zone tables: the `.MAP` `+0x10000` region-record
        // block + the MAN section-3 camera-region table. Drives the per-tile
        // region-type mask (`extra_flags`, field-VM op 0x42 mode 0) and the
        // camera-zone selection via `World::refresh_field_regions` (the
        // `FUN_80017FBC` / `FUN_800180EC` / `FUN_801DBA20` ports). Installed
        // unconditionally (empty when absent) so stale tables never leak
        // across a transition.
        let region_block: Vec<u8> = match self.scene.as_ref() {
            Some(scene) => match scene.field_map_region_block(&self.index) {
                Ok(block) => block.unwrap_or_default(),
                Err(err) => {
                    eprintln!("[scene] field region-table load skipped: {err:#}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        let zone_table: Vec<u8> = match self.scene.as_ref() {
            Some(scene) => match scene.field_zone_table(&self.index) {
                Ok(table) => table.unwrap_or_default(),
                Err(err) => {
                    eprintln!("[scene] field zone-table load skipped: {err:#}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        self.world
            .load_field_region_tables(&region_block, &zone_table);
        // Placed-prop colliders + interaction bank are installed further
        // down, once the scene MAN is parsed (the bind records carry each
        // prop's collision class and its touch script). Cleared here so a
        // stale scene's props never leak across a transition into a scene
        // whose MAN fails to parse.
        self.world.props.colliders = Vec::new();
        self.world.props.object_shifts.clear();
        self.world.props.bank = Default::default();
        self.world.props.pending_touch = None;
        // The 16-entry floor-height LUT the collision grid's low nibble
        // indexes - resident so the floor-height sampler
        // (`World::sample_field_floor_height`, port of `FUN_80019278`) can
        // resolve terrain elevation from the grid. The MAN header stores the
        // tiers as POSITIVE elevations; retail's runtime copy is NEGATED
        // (`FUN_8003AEB0`), and every consumer - the placement resolver's
        // `-lut[nibble]` world Y, the sampler's callers - assumes the
        // negated (PSX Y-down, up = negative) convention. Copying the raw
        // MAN values here made the sampler return +192 where retail returns
        // -192 (Vahn's house tier), sinking floor-snapped actors under the
        // drawn terrain.
        match self
            .scene
            .as_ref()
            .map(|s| s.field_floor_height_lut(&self.index))
        {
            Some(Ok(Some(lut))) => {
                self.world.terrain.floor_height_lut = lut.map(|v| v.wrapping_neg());
                // The camera composer reads this pristine copy, never the
                // live rungs the floor-tier oscillators write.
                self.world.terrain.floor_height_lut_static = self.world.terrain.floor_height_lut;
                // The ladder `FUN_8003A55C` spawns the bound placed objects
                // against: the one just installed (the system script that may
                // replace it runs after the sweep).
                self.world.terrain.placed_spawn_lut = self.world.terrain.floor_height_lut;
            }
            Some(Err(err)) => eprintln!("[scene] field floor-height LUT load skipped: {err:#}"),
            _ => {}
        }
        // The installed ladder is the baseline a scripted floor wave moves
        // from (`FieldLocomotion::ladder_seen`).
        self.world.locomotion.ladder_seen = self.world.terrain.floor_height_lut;
        // The seat lands at `y = 0`, and retail's every-frame glide then
        // carries the player onto the floor tier under it. Under the port's
        // snap the standing-still re-read fires only when that ladder moves,
        // so a player seated on a raised tier (a card load at a save point, a
        // door onto a ledge) stayed buried at `0` until its first step: the
        // browser page drew it inside the rock with the camera focus under
        // the ground, while the native window hid it behind a host-side snap
        // at mesh upload. The entry settles the seat itself, for every host.
        if self.world.locomotion.follow_terrain_height
            && let Some(player) = self.world.actors.first()
            && player.move_state.world_y == 0
        {
            let (x, z) = (
                i32::from(player.move_state.world_x),
                i32::from(player.move_state.world_z),
            );
            let floor = self.world.sample_field_floor_height(x, z);
            if let Some(player) = self.world.actors.first_mut() {
                player.move_state.world_y =
                    floor.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            }
        }
        // The field initialiser's camera-window install, `FUN_80017DD4(seat >>
        // 7, ...)` at `0x801D6ECC` - after the grid marks (`0x801D6BF8`) and
        // the MAN decode that installs the floor ladder (`0x801D6DA8`), which
        // is the order here too. It latches the region box at the seat tile and
        // runs the sub-area window sweep over it, planning the scene's first
        // windowed static-object list.
        // REF: FUN_80017DD4
        self.world.recentre_field_window_on_player();
        // Prefer the real scene-entry system script (ctx 0xFB) over event-
        // script record 0. Record 0 is a per-scene trigger/dispatch table,
        // not linear bytecode, so the field VM halts at its pc 0 and the
        // scene-entry logic (actor placement, BGM, conditional wall deltas)
        // never runs. The retail per-frame driver `FUN_8003ab2c` builds the
        // system script from the MAN asset's partition[1][0]; resolve and run
        // that instead. This resolves for any `scene_asset_table` bundle that
        // carries a MAN - including `town01` and the other `count=6`
        // `SceneAssetTable` field scenes, not just the kingdom bundles. (For
        // those scenes the runtime `_DAT_8007B898` MAN buffer IS this bundle
        // MAN, so the source is in the static bundle.) Only scenes with no
        // bundle MAN keep the record-0 fallback above. Flag-gated nibble-7
        // wall deltas in the entry script still need seeded story flags to
        // fire; the base grid loaded above is independent.
        let entry_script = match self.scene.as_ref() {
            Some(scene) => match scene.field_man_entry_script(&self.index) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("[scene] MAN entry-script resolve skipped: {err:#}");
                    None
                }
            },
            None => None,
        };
        if let Some((bytecode, pc0)) = entry_script {
            self.world.load_field_script_at(bytecode, pc0);
        }
        // Install the scene's random-encounter table straight from its MAN
        // asset (the disc-resident `_DAT_8007B898` source) - the retail
        // per-scene table, not a synthetic pattern. Resolves for the
        // `count=6` `scene_asset_table` field scenes (town01 etc.) now that
        // the detector covers them, same as the entry script above. The
        // per-row formation defs are merged into the formation table so the
        // table's row-index ids resolve to monster sets at battle-load.
        // Scenes whose static bundle carries no MAN - or towns whose MAN has
        // no rollable formations - leave the encounter unset here; the host
        // falls back to the synthetic registry (`install_encounter_for_scene`).
        self.world.set_active_scene_label(name);
        // The name-entry screen's prompts (field overlay, PROT 0897), so a
        // field VM that opens the screen draws the disc's wording.
        self.ensure_name_entry_prompts();
        let man_encounter = match self.scene.as_ref() {
            Some(scene) => match crate::disc_readers::encounter_man::field_man_encounter_table(
                scene,
                &self.index,
                name,
            ) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("[scene] MAN encounter-table resolve skipped: {err:#}");
                    None
                }
            },
            None => None,
        };
        if let Some((table, formations)) = man_encounter {
            // Collect the formation monster-ids before `install_man_encounter`
            // consumes the defs.
            let mut ids: Vec<u16> = formations
                .iter()
                .flat_map(|f| f.slots.iter().map(|s| s.monster_id))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            self.world.install_man_encounter(table, formations);
            // Merge real per-id monster stats from the disc archive (PROT 867)
            // over the catalog so the just-installed formations resolve to
            // genuine HP/MP/attack at battle-load instead of synthetic
            // placeholders. Archive entries win for the scene's ids; ids the
            // archive doesn't cover keep whatever catalog was installed.
            if !ids.is_empty()
                && let Some(archive) = self.monster_archive_bytes()
            {
                let cat = legaia_engine_battle::monster_catalog::catalog_from_monster_archive(
                    &archive, &ids,
                );
                for def in cat.by_id.into_values() {
                    self.world.tables.monster_catalog.insert(def);
                }
                // The player Seru summons' creature records, base and
                // evolved blocks both. The catalog above is the scene's own
                // monsters, so a summon creature is not in it; the summon
                // roll reads its HP / INT / element off this record.
                if self.world.tables.summon_creatures.is_empty() {
                    for spell_id in (0..=u8::MAX)
                        .filter(|&id| legaia_engine_effects::summon::has_summon_body(id))
                    {
                        let Some(cid) =
                            legaia_engine_effects::summon::summon_creature_id(spell_id, &archive)
                        else {
                            continue;
                        };
                        if let Ok(Some(rec)) =
                            legaia_actor_formats::monster_archive::record(&archive, cid)
                        {
                            self.world.tables.summon_creatures.insert(
                                spell_id,
                                legaia_engine_battle::monster_catalog::monster_def_from_record(
                                    &rec,
                                ),
                            );
                        }
                    }
                    // The body's stats are the cast's own `summon.dat`
                    // record, not the twin's: the seat arm copies the
                    // streamed record onto slot 7 (`FUN_80054CB0(record, 4)`
                    // from `FUN_801F19EC`). A cast with no twin - the flutes
                    // and the big summons - gets a body from the record alone,
                    // without which its roll fell to the MP-scaled
                    // placeholder (a Terra cast hit for 1).
                    self.install_summon_cast_records();
                }
                if self.world.tables.summon_elements.is_empty() {
                    for spell_id in (0..=u8::MAX)
                        .filter(|&id| legaia_engine_effects::summon::has_summon_body(id))
                    {
                        let Some(cid) =
                            legaia_engine_effects::summon::summon_creature_id(spell_id, &archive)
                        else {
                            continue;
                        };
                        if let Ok(Some(rec)) =
                            legaia_actor_formats::monster_archive::record(&archive, cid)
                        {
                            self.world
                                .tables
                                .summon_elements
                                .insert(spell_id, rec.element);
                        }
                    }
                }
                // Pair the per-move power table with the just-merged monster
                // stats so the special-attack damage path can resolve real
                // per-move power (PROT 0898; falls back to the placeholder if
                // the disc read fails).
                self.ensure_move_power_table();
                // ... and the cast-effect pool, so a cast can stage its own
                // module's spawn records.
                self.ensure_cast_effect_pool();
                // ... and the per-(character, equipped set) swing-cost
                // bytes the Arts command input charges per press.
                self.refresh_battle_swing_costs();
                // ... and the Auto attack's pool-arm inputs from the same files.
                self.refresh_battle_auto_combo_inputs();
            }
        }
        // Route the per-region random-encounter table from the same MAN so
        // field steps roll against the player's active region (per-region
        // rate + formation-range, `FUN_801D9E1C`) rather than the aggregated
        // mean rate the `EncounterSession` above carries. Scenes whose MAN
        // has no encounter-region section (towns) resolve to `None`, which
        // clears the field tracker and leaves the mean path in place - so
        // this is additive: the mean session stays installed either way and
        // supplies the transition / grace bracketing.
        let field_region_table = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten())
            .as_ref()
            .and_then(|man| {
                legaia_engine_battle::region_encounter::region_encounter_table_from_man(name, man)
            });
        self.world.set_field_regions(field_region_table);
        // Install the scene's field entity-SM carriers derived from the same
        // MAN actor-placement partition (retail builds one record per
        // MAN-placed entity at scene load). They sit Idle - the sparring
        // carrier only advances when `engage_field_carrier` is called on the
        // dialogue-accept - so this is inert for the cold-boot path but makes
        // the derived carrier set live (and is the counterpart to the MAN
        // encounter-table install above). Soft-fail: a scene without a MAN, or
        // with no interactable placements, just installs an empty set.
        // The fog pool: MAIN INIT re-allocates it on every field entry and
        // the MAN installer clears its gate, so both happen here before the
        // section-4 region table (below) is handed over.
        self.world.reset_fog_for_scene_entry();
        // Arcs, NPC heights and attached lights are field-overlay pool
        // actors: the pool is re-allocated with the overlay on every entry.
        self.world.reset_field_script_actors();
        match self
            .scene
            .as_ref()
            .map(|s| s.field_man_payload(&self.index))
        {
            Some(Ok(Some(man_bytes))) => match legaia_scene_formats::man_section::parse(&man_bytes)
            {
                Ok(man_file) => {
                    self.world
                        .install_field_carriers_from_man(&man_file, &man_bytes);
                    // Section 4 -> `DAT_80073ED8`: the fog-region table the
                    // spawner `FUN_801D629C` looks every named tile up in.
                    self.world.install_fog_regions(&man_file, &man_bytes);
                    // Initial NPC facings: retail's placement installer
                    // (`FUN_8003A1E4`) pre-runs each record's spawn prologue
                    // at scene load, and the prologue's `0x4C 0x51` / `0x38`
                    // ops write the actor's `+0x26` heading from the SCUS
                    // direction LUT. Seed those headings now (after the
                    // carrier install, which clears the heading map) so a
                    // never-walked NPC stands with its retail facing.
                    // REF: FUN_8003A1E4
                    self.world.seed_field_npc_facings(&man_file, &man_bytes);
                    // Ambient effect stagers are installed by the prologue
                    // pre-run below and nowhere else: retail's placement
                    // installer (`FUN_8003A1E4`) runs each P1 placement's
                    // spawn prologue for one frame slice at scene load, and
                    // each op-0x34 sub-3 that slice executes calls
                    // `FUN_800252EC(arg + 1)` at the executing context's
                    // position - one install per op. The slice's static census
                    // (`man_field_scripts::scene_entry_ambient_installs`)
                    // names the same records; installing from it as well runs
                    // every ambient part twice (kor5's cue `0x204` then reaches
                    // the SFX ring twice per period where retail stores it
                    // once).
                    // REF: FUN_8003A1E4, FUN_800252EC
                    // Initial NPC POSITIONS: the same retail pre-run also
                    // executes each record's story-flag-tested opening ops -
                    // the `0x23 MoveTo` park to the off-map sentinel for
                    // actors the current story state despawns, and the
                    // story relocations that seat an actor away from its MAN
                    // header tile. Run one field-VM frame slice per channel
                    // (unconditionally - load-time behaviour, not the opt-in
                    // free-roam liveliness) so the first rendered frame shows
                    // the story-true town, not the raw placement headers.
                    // Runtime-pinned against the retail town01 actor list
                    // (`field_npc_entry_positions_town01` disc test).
                    // REF: FUN_8003A1E4
                    self.world.pre_run_field_channel_prologues();
                    // Placed-prop colliders + the prop animation/interaction
                    // bank: every placed `.MAP` object is a solid actor in
                    // retail's collision candidate list (`FUN_801CF754`),
                    // classed by its bind record's spawn-prologue `0x31` ops;
                    // a bound, posed prop additionally gets a bank entry
                    // whose record runs through the field VM on touch /
                    // interact (the door swing, the cupboard search).
                    self.install_field_props(&man_file, &man_bytes);
                    // `.MAP` **object** binds (retail scene-init `FUN_8003A55C`):
                    // the object layer is walked, each spawnable object's key
                    // tile (`object_tile + descriptor (dx,dz)`) resolves a
                    // kind-1 trigger, and the trigger's `record` byte selects a
                    // MAN record by **flat** index - that record IS the object's
                    // script. House doors are these; their scripts
                    // cross-context-teleport the player (`0x23` MOVE_TO or the
                    // `0x4C 0x51` teleport-plus-anim form).
                    //
                    // The bind sits at the object's **contact-box centre**
                    // (`FUN_801CFC40`), not at the trigger tile: the trigger
                    // tile is a lookup key and is routinely a wall the player
                    // can never stand on (Rim Elm's own house-door key tile
                    // (38,25) is inside the house's collision wall). Falls back
                    // to the trigger-tile centre only when the scene has no
                    // readable `.MAP` object layer.
                    // REF: FUN_8003A55C, FUN_801CFC40
                    let map_bytes = self
                        .scene
                        .as_ref()
                        .and_then(|s| s.field_map_index(&self.index))
                        .and_then(|idx| self.index.entry_bytes_extended(idx).ok());
                    let triggers: Vec<legaia_engine_field::field_regions::TileTrigger> = self
                        .field_triggers
                        .0
                        .iter()
                        .chain(self.field_triggers.1.iter())
                        .copied()
                        .collect();
                    type Bind = (
                        (i16, i16),
                        legaia_engine_field::man_field_scripts::WalkTouchEvent,
                        Option<usize>,
                    );
                    let binds: Vec<Bind> = match map_bytes.as_deref() {
                        Some(map) => legaia_engine_field::man_field_scripts::object_walk_touch_binds(
                            map, &triggers, &man_file, &man_bytes,
                        )
                        .into_iter()
                        .map(|b| (b.contact, b.event, Some(b.record)))
                        .collect(),
                        None => triggers
                            .iter()
                            .filter(|t| t.gate == 0)
                            .filter_map(|t| {
                                let record = t.record as usize;
                                let event = legaia_engine_field::man_field_scripts::flat_record_walk_touch_event(
                                    &man_file, &man_bytes, record,
                                )?;
                                let pos = (
                                    i16::from(t.tile_x) * 128 + 0x40,
                                    i16::from(t.tile_z) * 128 + 0x40,
                                );
                                Some((pos, event, Some(record)))
                            })
                            .collect(),
                    };
                    if !binds.is_empty() {
                        log::info!(
                            "field: installed {} object door bind(s) from the .MAP object layer",
                            binds.len()
                        );
                    }
                    self.world.install_trigger_walk_touch_with_records(&binds);
                    // Object-bind script CONTEXTS (the other half of
                    // `FUN_8003A55C`): every bound object's actor carries the
                    // trigger's flat record index as its script id
                    // (`actor[+0x50] = trigger[2]`), making partition-0
                    // records resolvable cross-context targets - the town01
                    // Mei walk-on beat pokes the Vahn's-house door object as
                    // channel `0x01`. Spawn one poke-target channel per bind
                    // (bind-time `0x24`/`0x25` prologues pre-run inside).
                    // Falls back to the gate-0 trigger tiles when the scene
                    // has no readable `.MAP` object layer.
                    // REF: FUN_8003A55C, FUN_8003C83C
                    let object_binds: Vec<(usize, (i16, i16))> = match map_bytes.as_deref() {
                        Some(map) => {
                            legaia_engine_field::man_field_scripts::object_script_bind_seats(
                                map, &triggers,
                            )
                        }
                        None => triggers
                            .iter()
                            .filter(|t| t.gate == 0)
                            .map(|t| {
                                (
                                    t.record as usize,
                                    (
                                        i16::from(t.tile_x) * 128 + 0x40,
                                        i16::from(t.tile_z) * 128 + 0x40,
                                    ),
                                )
                            })
                            .collect(),
                    };
                    // The pack meshes each bound record's placed draws use,
                    // so an op-`0x4B` morph a bind's prologue arms reaches
                    // them (`World::arm_field_morph`).
                    if let Some(scene) = self.scene.as_ref()
                        && let (Ok(Some(placements)), Ok(Some(binds))) = (
                            scene.field_object_placements(&self.index),
                            scene.field_object_binds(&self.index),
                        )
                    {
                        self.world.set_object_morph_targets(
                            legaia_engine_scene::field_env::object_record_pack_slots(
                                &placements,
                                &binds,
                            ),
                        );
                    }
                    self.world.set_object_bind_rots(
                        map_bytes
                            .as_deref()
                            .map(|map| {
                                legaia_engine_field::man_field_scripts::object_script_bind_rots(
                                    map, &triggers,
                                )
                            })
                            .unwrap_or_default(),
                    );
                    // Each bind's seat Y (`ladder[nibble] + y_off`), against
                    // the ladder the scene installed above: a height write
                    // on the actor is absolute, and the draw measures it
                    // from here (`World::object_draw_displacements`).
                    let ladder = self.world.terrain.floor_height_lut;
                    self.world.set_object_bind_heights(
                        map_bytes
                            .as_deref()
                            .map(|map| {
                                legaia_engine_field::man_field_scripts::object_script_bind_heights(
                                    map, &triggers, &ladder,
                                )
                            })
                            .unwrap_or_default(),
                    );
                    self.world
                        .seed_object_channels(&man_file, &man_bytes, &object_binds);
                    // Boss-stager placements (chapter-1: Mt. Rikuroa's Caruban
                    // stager P1[3]): partition-1 records carrying the
                    // scripted-battle op `3E FF <row>`, armed as approach /
                    // interact bindings while their own park gate is clear.
                    // Runs after the MAN encounter install above (the
                    // formation-row validator) and after the walk-touch map
                    // was rebuilt. The record's own script bytes do the rest
                    // (marker SET -> `trigger_scripted_battle`).
                    self.world
                        .install_boss_stagers_from_man(&man_file, &man_bytes);
                }
                Err(err) => eprintln!("[scene] field-carrier MAN parse skipped: {err:#}"),
            },
            Some(Err(err)) => eprintln!("[scene] field-carrier MAN payload skipped: {err:#}"),
            _ => {}
        }
        // Install the VDF ("set_mime") buffer so the `0x4C 0xD8`
        // synchronous-spawn host hook can resolve actor templates. Only
        // a handful of scenes carry VDF data (8/124 in the retail
        // corpus); the lookup is cheap and returns `None` for the rest.
        if let Some(scene) = self.scene.as_ref() {
            self.world
                .set_vdf_buffer(legaia_engine_scene::scene_bundle::find_vdf_buffer(scene));
        }
        // Install the per-scene MOVE pool (retail `_DAT_8007B888`). The
        // bytes come from the scene's `scene_asset_table` slot-4
        // `Asset(0x05) = Move` descriptor (see `docs/formats/mdt.md`).
        // The descriptor offsets reference positions in the full
        // on-disc footprint (including trailing-overlay sectors), so
        // we fetch via `entry_bytes_extended` rather than the indexed
        // view that `Scene::load` keeps in `entry.bytes`. Without this
        // the `MoveBufferHost` resolver returns `None` and the move-
        // table cursor stays idle.
        let move_install = self.scene.as_ref().and_then(|scene| {
            legaia_engine_scene::scene_bundle::find_bundle(scene).map(|b| (b.entry_idx(), b))
        });
        if let Some((entry_idx, bundle)) = move_install {
            match self
                .index
                .entry_bytes_extended(entry_idx)
                .and_then(|bytes| {
                    legaia_engine_scene::scene_bundle::extract_move_payload(&bundle, &bytes)
                }) {
                Ok(Some(bytes)) => self.world.set_move_buffer_root(bytes),
                Ok(None) => self.world.set_move_buffer_root(Vec::new()),
                Err(err) => eprintln!("[scene] move-table extract skipped: {err:#}"),
            }
        }
        // Seed the global TMD-pool head from PROT 0874 section 0 (the
        // 5 character-mesh TMDs that retail's `DAT_8007C018[0..4]`
        // resolves to), byte-equal to a Drake post-warp RAM snapshot.
        // Producers for the
        // remaining 138 kingdom-bundle entries are not yet pinned;
        // those slots stay empty until the full chain lands. Idempotent:
        // the head re-seeds across scene transitions but only on the
        // first call (subsequent calls early-return when the head is
        // already populated).
        let head_populated = self.world.global_tmd_pool.len() >= 5
            && self.world.global_tmd_pool[..5].iter().all(|s| s.is_some())
            && self.world.field_head_pool.len() >= 5
            && self.world.field_head_pool[..5].iter().all(|s| s.is_some());
        if !head_populated
            && let Err(err) = seed_global_tmd_pool_from_befect_data(&self.index, &mut self.world)
        {
            eprintln!("[scene] global TMD-pool seed skipped: {err:#}");
        }
        // Load the battle effect-model library from PROT 0871 (`etmd.dat`)
        // into `DAT_8007C018[3..=32]`. Retail pulls this at battle init; the
        // engine keeps it resident across the battle scene-mode overlay (like
        // the etim VRAM and effect catalog), so seeding it at field entry is
        // equivalent. It overwrites the two trailing slots of the §0 field
        // head (`[3]`, `[4]`) - matching retail's temporal layout - and gives
        // the effect-model render path the real Gimard *Tail Fire* mesh at
        // `[26]`. Idempotent: only loads when the library isn't already
        // resident. Soft-fails - the §0 preview stand-in remains the fallback.
        if !effect_model_library_loaded(&self.world)
            && let Err(err) = seed_effect_model_library_from_etmd(&self.index, &mut self.world)
        {
            eprintln!("[scene] effect-model library (PROT 0871) load skipped: {err:#}");
        }
        // The battle VDF morph pack (PROT 0872, `vdf.dat`), loaded beside the
        // effect-model library for the same reason: effect parts' morph lanes
        // index it. Idempotent; soft-fails to rest meshes.
        if self.world.tables.battle_vdf.is_none()
            && let Err(err) = seed_battle_vdf(&self.index, &mut self.world)
        {
            eprintln!("[scene] battle VDF (PROT 0872) load skipped: {err:#}");
        }
        // Load the runtime effect-script catalog from PROT 0873 (`efect.dat`)
        // so the battle-action SM's `ui_element` spawns resolve to real
        // effect scripts. Idempotent: only loads when the catalog is empty
        // (it persists on `World` across field/battle transitions, like the
        // global TMD pool). Soft-fails - an empty catalog just doesn't spawn.
        if self.world.effect_catalog.is_empty()
            && let Err(err) = seed_effect_catalog_from_efect_dat(&self.index, &mut self.world)
        {
            eprintln!("[scene] effect-catalog load skipped: {err:#}");
        }
        // Pre-bind actor ↔ TMD/ANM resources so they survive the first
        // field-VM actor-spawn opcode (see `World::init_scene_animations`).
        //
        // Uses [`SceneResources::build_targeted_with_options`] with
        // `SceneLoadKind::Field` so the per-TIM image / CLUT block
        // decisions match the retail field loader: only field /
        // terrain / NPC meshes contribute, scene_tmd_stream battle
        // character meshes (loaded by `FUN_8001FE70` at battle init)
        // and battle_data records (`FUN_8001E890` chain) are excluded.
        if let Some(scene) = self.scene.as_ref() {
            // The shared blocks the retail field engine keeps resident
            // across scene transitions (player TMD + shared UI atlas).
            let mut shared_scenes: Vec<Scene> = Vec::new();
            for name in legaia_engine_scene::scene_resources::FIELD_SHARED_BLOCKS {
                if let Ok(s) = Scene::load(&self.index, name) {
                    shared_scenes.push(s);
                }
            }
            let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
            // World-map scenes (`map\d\d` = the three kingdom bundles) carry
            // their landmark geometry in slot 1 of a 7-asset descriptor table,
            // not as raw / loosely-LZS-packed TMDs. `SceneLoadKind::WorldMap`
            // makes the resource build decode that slot explicitly (the
            // faithful retail path) and routes per-prim emit through the
            // distance-cue overlay variant. Every other field uses the plain
            // field loader. See [`docs/subsystems/world-map.md`].
            let load_kind = if legaia_engine_scene::scene::is_world_map_scene(name) {
                legaia_engine_scene::scene_resources::SceneLoadKind::WorldMap
            } else {
                legaia_engine_scene::scene_resources::SceneLoadKind::Field
            };
            // The boot-resident system-UI bundle (raw PROT TOC entries 0/1)
            // layers under every scene build - it is what makes CLUT rows
            // 510/511 + the (960,256) menu-glyph atlas resident for the env
            // meshes that sample them. Soft-fails to None (the scene still
            // builds; those prims just drop like they used to).
            let system_ui = match self.index.system_ui_bundle() {
                Ok(b) => Some(b),
                Err(err) => {
                    eprintln!("[scene] system-UI bundle parse skipped: {err:#}");
                    None
                }
            };
            if let Ok((mut res, _stats)) =
                legaia_engine_scene::scene_resources::SceneResources::build_targeted_with_options(
                    scene,
                    &shared_refs,
                    legaia_engine_scene::scene_resources::BuildOptions {
                        kind: load_kind,
                        // Retail's field loader (FUN_8001F7C0) DMA-uploads
                        // every TIM in the scene, not just the subset the
                        // first-frame meshes sample. The town's environment
                        // geometry (the LZS-packed mesh pack now parsed out of
                        // the scene_asset_table) samples texture pages across
                        // the whole atlas, so a render-targeted upload drops
                        // ~75% of its prims (missing texture page). Uploading
                        // all TIMs lifts the town keep ratio to ~95%.
                        upload_all_tims: true,
                        system_ui: system_ui.as_deref(),
                    },
                )
            {
                // Layer the effect-texture pool (etim.dat, PROT 0874 section 2)
                // UNDER the scene VRAM: the 3D effect models (etmd.dat) and the
                // field fog sheets (page `(448, 0)`, CLUT row 473) sample it.
                // Retail uploads the pool before the scene, so where a scene
                // TIM covers a pool rect the scene's texels win - `dolk` keeps
                // its own `(448, 0)` texels on the 1010 halfwords the two
                // differ on. Writing the pool over the build clobbered them.
                // Same order as the native window's build (`window/run.rs`).
                // Soft-fails (the pool just stays absent).
                let mut effect_pool = legaia_tim::Vram::new();
                match upload_effect_textures_into_vram(&self.index, &mut effect_pool, true) {
                    Ok(_) => res.vram.underlay(&effect_pool),
                    Err(err) => {
                        eprintln!("[scene] effect-texture VRAM underlay skipped: {err:#}");
                    }
                }
                self.world.init_scene_animations(&res);
                // Scene-entry VDF pulse (enhancement): when the scene's VDF
                // pack is populated but the entry-ambient tree armed no
                // retail morph lanes (jou's flesh-ground deltas are
                // cutscene-armed), install the rolling pulse so the morphs
                // move at plain entry. `install_entry_vdf_pulse` self-guards
                // on retail-armed parts (town0e / rikuroa keep the faithful
                // envelope). See `legaia_engine_field::vdf_pulse`.
                if let Some(scene) = self.scene.as_ref() {
                    let env = legaia_engine_scene::field_env::env_pack_tmd_indices(scene, &res);
                    let pack_objects: Vec<Vec<usize>> = env
                        .iter()
                        .map(|&ti| {
                            res.tmds[ti]
                                .tmd
                                .objects
                                .iter()
                                .map(|o| o.vertices.len())
                                .collect()
                        })
                        .collect();
                    if self.world.install_entry_vdf_pulse(name, &pack_objects) {
                        log::info!(
                            "scene '{name}': entry VDF pulse armed over {} pack meshes",
                            pack_objects.len()
                        );
                    }
                }
                self.resources = Some(res);
            }
        }
        // The scene's window of the global model pool, rebuilt with the
        // resources: it is what resolves a scripted mesh re-bind's operand
        // (motion-VM op `0x0E`) to bytes on either host.
        self.model_bank = match self.scene.as_ref() {
            Some(sc) => legaia_engine_scene::model_bank::SceneModelBank::build(sc),
            None => legaia_engine_scene::model_bank::SceneModelBank::default(),
        };
        // The same bank, materialised for the world: the field VM's
        // `0x4C 0xD8` allocator resolves its model operand here.
        let field_bank = match self.scene.as_ref() {
            Some(sc) => self
                .model_bank
                .materialise(sc)
                .into_iter()
                .map(|bytes| {
                    let raw = bytes?;
                    let tmd = legaia_tmd::parse(&raw).ok()?;
                    Some(std::sync::Arc::new(
                        legaia_engine_scene::model_bank::GlobalTmd { tmd, raw },
                    ))
                })
                .collect(),
            None => Vec::new(),
        };
        self.world.install_field_scene_bank(field_bank);
        // Opening-prologue hand-off arm. When entering the cutscene scene
        // `opdeene`, derive the `town01` hand-off arm from the scene's own MAN
        // bytecode instead of a blind constant: walk the cutscene-timeline
        // partition for the `GFLAG_SET 26` write retail's gate waits on and
        // arm only when it is present. A cutscene scene that never issues that
        // write never produces a false hand-off. See
        // [`legaia_engine_core::world::World::arm_prologue_handoff_from_man`].
        if name == legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE {
            // The New-Game opening chain starts here: while it plays (through
            // the `opstati` / `opurud` legs the timelines chain into), a
            // confirm press with the hand-off bit armed skips the whole
            // remaining opening to `town01` (retail `FUN_801D1344`).
            self.world.cutscene.opening_chain_active = true;
            // Decode the "It was the Seru." caption image from the scene's
            // geometry pack (PROT 0749). It is a baked TIM, not text - the host
            // blits it, faded, in the gap between the two narration crawls (see
            // `legaia_engine_scene::cutscene_caption`). `None` when the disc / entry is absent.
            if let Some(scene) = self.scene.as_ref() {
                self.world.cutscene.caption =
                    legaia_engine_scene::cutscene_caption::decode_opdeene_caption(scene);
                if self.world.cutscene.caption.is_some() {
                    log::info!("prologue: decoded 'It was the Seru.' caption image (PROT 0749)");
                }
            }
            match self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            {
                Some(Ok(Some(man_bytes))) => {
                    match legaia_scene_formats::man_section::parse(&man_bytes) {
                        Ok(man_file) => {
                            // Execute the cutscene timeline as a spawned field-VM
                            // context: its camera path + actor moves play and the
                            // closing `GFLAG_SET 26` fires by execution. Fall back
                            // to the static MAN-walk arm only when the timeline
                            // record can't be resolved, so the prologue still hands
                            // off either way.
                            if self
                                .world
                                .load_cutscene_timeline_from_man(&man_file, &man_bytes)
                            {
                                log::info!(
                                    "prologue: executing '{}' cutscene timeline -> '{}' hand-off by GFLAG_SET {}",
                                    legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE,
                                    legaia_game_tables::new_game::OPENING_SCENE,
                                    legaia_engine_core::world::PROLOGUE_HANDOFF_BIT,
                                );
                            } else if self
                                .world
                                .arm_prologue_handoff_from_man(&man_file, &man_bytes)
                            {
                                log::info!(
                                    "prologue: armed '{}' -> '{}' hand-off from MAN GFLAG_SET {} (static fallback)",
                                    legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE,
                                    legaia_game_tables::new_game::OPENING_SCENE,
                                    legaia_engine_core::world::PROLOGUE_HANDOFF_BIT,
                                );
                            }
                            // The inline narration is script-driven: the timeline
                            // stepper installs each block's pages on the roller
                            // presenter when its PC reaches the block's op and
                            // suspends the timeline until the crawl completes
                            // (retail `FUN_80037174`). Nothing to install here.
                            if let Some(tl) = self.world.cutscene.timeline.as_ref() {
                                log::info!(
                                    "prologue: '{}' timeline carries {} narration block(s)",
                                    legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE,
                                    tl.narration_blocks.len(),
                                );
                            }
                        }
                        Err(err) => eprintln!("[scene] prologue MAN parse skipped: {err:#}"),
                    }
                }
                Some(Err(err)) => eprintln!("[scene] prologue MAN payload skipped: {err:#}"),
                _ => {}
            }
        }
        // New-game opening: when `town01` is entered via the prologue hand-off
        // (not a normal visit), install its opening cutscene timeline so the
        // establishing camera sweep + Vahn's walk-out play and the pinned
        // op-`0x49` STATE_RESUME opens the name-entry overlay at the right beat
        // (rather than the host opening it blindly at the hand-off). One-shot:
        // consume the flag so re-entering `town01` later never re-runs it.
        if name == legaia_game_tables::new_game::OPENING_SCENE
            && (self.world.cutscene.entering_town01_opening
                || self.world.cutscene.opening_chain_active)
        {
            self.world.cutscene.entering_town01_opening = false;
            // Arriving at Rim Elm ends the opening cutscene chain, whether via
            // the skip packet or the natural scene-change chain.
            self.world.cutscene.opening_chain_active = false;
            self.world.cutscene.narration = None;
            self.world.cutscene.card = None;
            match self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            {
                Some(Ok(Some(man_bytes))) => {
                    match legaia_scene_formats::man_section::parse(&man_bytes) {
                        Ok(man_file) => {
                            if self
                                .world
                                .install_town01_opening_timeline(&man_file, &man_bytes)
                            {
                                log::info!(
                                    "opening: executing '{}' opening timeline (P2[{}]); name entry opens at its op-0x49 STATE_RESUME",
                                    legaia_game_tables::new_game::OPENING_SCENE,
                                    legaia_engine_core::world::World::TOWN01_OPENING_TIMELINE_RECORD,
                                );
                            }
                        }
                        Err(err) => eprintln!("[scene] town01 opening MAN parse skipped: {err:#}"),
                    }
                }
                Some(Err(err)) => eprintln!("[scene] town01 opening MAN payload skipped: {err:#}"),
                _ => {}
            }
        }
        // Decode the scene's gold-shop stock from its MAN so the field-menu
        // shop-open path offers real per-scene items at real prices instead of a
        // hand-authored list. Cheap when the scene has no merchant.
        self.populate_scene_shops();
        // Run the entry system script's passes up to the one that spawns the
        // opening record (`World::pre_run_entry_script`): retail runs them
        // over the first frames, before that record takes the player - flag
        // routing, walls, BGM cue, the region selector and the 0x52F
        // arrival-fade arm (fade-in from black). The engine installs the
        // record at entry, so they run in the load frame here. Placed at the
        // end of entry so every host hook the script fires sees the
        // fully-installed scene (channels, props, shops).
        // Scoped to the opening prologue cutscene scenes (the same gate as
        // `scene_color_grade`): their `P1[0]` is pure choreography setup, so
        // the linear load-frame run is safe, and it is what puts the fade
        // instant-black on screen before the first frame. Interactive scenes
        // keep the per-tick trickle - a linear pre-run through their
        // dialog-carrying entry scripts can wander into text bytes that
        // decode as story-flag writes (seen as a story-gate drift in the
        // chapter-2 hub sweep), so widening this gate needs a text-boundary
        // stop first.
        if matches!(name, "opdeene" | "opstati" | "opurud") {
            self.world.pre_run_entry_script();
        }
        // Drain any pending transition the previous scene left behind.
        self.world.pending_scene_transition = None;
        Ok(())
    }

    /// Decode the active scene's gold shops from its MAN(s) and park them on
    /// [`legaia_engine_menus::shop_state::ShopState::scene_shops`], priced from
    /// [`legaia_engine_menus::shop_state::ShopState::item_shop_data`]. Scans every entry in the scene's
    /// CDNAME block (most carry one bundle MAN); cheap for non-bundle entries -
    /// the locator returns early without decompressing when an entry isn't a
    /// scene bundle with a MAN. No-op shop list when the disc / item data is
    /// absent.
    fn populate_scene_shops(&mut self) {
        let entry_idxs: Vec<u32> = match self.scene.as_ref() {
            Some(s) => s.entries.iter().map(|e| e.idx).collect(),
            None => {
                self.world.shops.scene_shops.clear();
                return;
            }
        };
        let item_data = self.world.shops.item_shop_data.clone();
        let mut shops = Vec::new();
        for idx in entry_idxs {
            let bytes = match self.index.entry_bytes_extended(idx) {
                Ok(b) => b,
                Err(_) => continue,
            };
            shops.extend(legaia_engine_menus::shop_catalog::scene_shops(
                &bytes,
                idx as usize,
                item_data.as_ref(),
            ));
        }
        self.world.shops.scene_shops = shops;
    }
}
