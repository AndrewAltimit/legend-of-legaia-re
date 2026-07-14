//! `SceneHost` field/world-map scene entry, shop population, archive/move-power seeding, and per-frame tick.
//!
//! Extracted verbatim from `scene/host.rs` as an additional `impl SceneHost` block.

use super::*;

impl SceneHost {
    /// Install the placed-prop collision + interaction layer for the current
    /// field scene: one [`crate::world::FieldPropCollider`] per placed `.MAP`
    /// object (retail's collision candidate list, `FUN_801CF754` - **solid by
    /// default**, a closed door blocks), classed by its bind record's
    /// spawn-prologue `0x31` ops (`31 1E` = interact-gated cupboard class,
    /// `31 00` = born collision-exempt), plus the
    /// [`crate::field_env::PropAnimBank`] whose entries carry each posed
    /// prop's clip runtime and its bind record - the script a touch /
    /// interact runs through the field VM.
    ///
    /// REF: FUN_8003A55C, FUN_801CF754, FUN_801CFC40
    fn install_field_props(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
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
        let bundle = scene.entries.iter().find_map(|e| {
            [3usize, 5, 6, 7]
                .into_iter()
                .find_map(|d| legaia_asset::player_anm::find_in_entry(&e.bytes, d).pop())
        });
        let clip = |anim: u8| -> Option<(u16, bool, u8)> {
            let b = bundle.as_ref()?;
            let r = b.record(anim.checked_sub(1)? as usize).ok()?;
            Some((r.frame_count, (r.a >> 8) & 1 != 0, (r.flag & 0xFF) as u8))
        };
        self.world.field_prop_bank =
            crate::field_env::PropAnimBank::build(&placements, &binds, man_file, man_bytes, clip);
        self.world.field_prop_colliders = placements
            .iter()
            .map(|p| {
                let anchor = (p.anchor_col, p.anchor_row);
                let bind = binds.get(&anchor);
                let cflags = bind
                    .map(|b| {
                        crate::field_env::record_spawn_cflags(
                            man_file,
                            man_bytes,
                            b.record as usize,
                        )
                    })
                    .unwrap_or(0);
                crate::world::FieldPropCollider {
                    anchor: bind.map(|_| anchor),
                    center: (p.collider_x, p.collider_z),
                    live: (p.world_x, p.world_z),
                    moving_box: cflags & 0x0102_0000 != 0,
                    interact: cflags & 0x4002_0000 != 0,
                    solid: cflags & 3 == 0,
                }
            })
            .collect();
    }

    /// Lazily load + cache the monster stat archive (PROT 867, extended
    /// footprint - the archive lives in the entry's trailing-gap sectors,
    /// not the small indexed payload, so `entry_bytes` would truncate it).
    /// Returns `None` if the entry can't be read.
    fn monster_archive_bytes(&mut self) -> Option<Arc<Vec<u8>>> {
        if self.monster_archive_cache.is_none() {
            match self.index.entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY) {
                Ok(b) => self.monster_archive_cache = Some(Arc::new(b)),
                Err(err) => {
                    eprintln!(
                        "[scene] monster archive (PROT {MONSTER_ARCHIVE_PROT_ENTRY}) load skipped: {err:#}"
                    );
                    return None;
                }
            }
        }
        self.monster_archive_cache.clone()
    }

    /// Install the battle-action move-power table onto the world from PROT
    /// entry 0898 (the battle-action overlay), once per host. The monster
    /// special-attack damage path reads it to roll faithful per-move damage;
    /// a read/parse failure leaves [`crate::world::World::move_power`] `None`
    /// (the placeholder damage path stays active) and is not retried.
    fn ensure_move_power_table(&mut self) {
        if self.move_power_loaded {
            return;
        }
        self.move_power_loaded = true;
        let entry = crate::move_power::BATTLE_ACTION_OVERLAY_PROT_ENTRY;
        match self.index.entry_bytes(entry) {
            Ok(bytes) => {
                if let Some(cat) = crate::move_power::MovePowerCatalog::from_overlay_0898(&bytes) {
                    self.world.move_power = Some(cat);
                    // Retain the overlay so the move-FX render path can read the
                    // 0x801f6324 prototype records' move-VM bytecode.
                    self.world.move_power_overlay = Some(std::sync::Arc::from(bytes.as_slice()));
                } else {
                    eprintln!(
                        "[scene] move-power table (PROT {entry}) parse failed - placeholder damage stays active"
                    );
                }
                // The element-affinity matrix + per-character element table are
                // sibling static data in the same overlay, so parse them from the
                // same bytes. A failure leaves the neutral 100% multiplier active.
                if let Some(aff) = legaia_asset::element_affinity::parse(&bytes) {
                    self.world.element_affinity = Some(aff);
                } else {
                    eprintln!(
                        "[scene] element-affinity tables (PROT {entry}) parse failed - neutral affinity stays active"
                    );
                }
            }
            Err(err) => {
                eprintln!("[scene] battle-action overlay (PROT {entry}) load skipped: {err:#}");
            }
        }
    }

    /// Load `name`, switch the world to [`crate::world::SceneMode::Field`],
    /// and load the requested event-script record (default 0) into the
    /// field-VM bytecode buffer. Returns `Err` if the scene has no event
    /// scripts or the record index is out of range.
    pub fn enter_field_scene(&mut self, name: &str, record_index: usize) -> Result<()> {
        self.load_scene(name)?;
        // Drop any cutscene timeline from a previous scene; only `opdeene`
        // re-installs one below, so it must not leak into the scene we hand off
        // to (Rim Elm). The per-actor channels are timeline-scoped and drop
        // with it.
        self.world.cutscene_timeline = None;
        // Concurrent spawned-record contexts (and any not-yet-drained op-0x44
        // requests) are scene-scoped like the timeline: their bytecode slices
        // came from the previous scene's MAN.
        self.world.helper_contexts.clear();
        self.world.pending_record_spawns.clear();
        self.world.cutscene_card = None;
        // Drop the previous scene's caption image (only `opdeene` re-decodes one
        // below); reset its fade + hold so a re-entry starts hidden.
        self.world.cutscene_caption = None;
        self.world.cutscene_caption_alpha = 0.0;
        self.world.cutscene_caption_shown_frames = 0;
        self.world.field_channels.clear();
        self.world.field_channels_man = None;
        self.world.field_npc_anim_cues.clear();
        // Reset the persistent op-0x45 camera-param set so a new scene's
        // cutscene shots start clean (the params now MERGE per-slot across beats
        // in `camera_configure`, so a stale set would leak the prior scene's
        // focus / depth into a beat that omits those slots).
        self.world.camera_state.params.clear();
        // Scripted CLUT-cell effects are scene-scoped (their cell operands
        // came from the previous scene's MAN); drop any in flight and re-pin
        // the frame-step factor `dt` (retail `DAT_1F800393`, the adaptive
        // vsyncs-per-game-tick factor the frame-flip path `FUN_80016B6C`
        // writes): live poll baselines run field/town scenes at 2 (30 fps)
        // and the overworld kingdom scenes (`mapNN`) at 3 (20 fps). See
        // `World::frame_step`.
        self.world.clut_fx.clear();
        self.world.clut_vsync_accum = 0;
        self.world.clut_pending_game_ticks = 0;
        self.world.frame_step = if crate::scene::is_world_map_scene(name) {
            3
        } else {
            2
        };
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
        self.world.mode = crate::world::SceneMode::Field;
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
        // Cold field entry: seed the player at the retail cold-boot spawn.
        // `FUN_801D6704` creates the player actor at the camera-window centre
        // `(0xA40, 0, 0xA40)` on a non-warp entry; for the New Game opening
        // (town01) this is Vahn's authored Rim Elm spawn, and it also seeds the
        // follow camera onto the right region. Engines that arrive via a warp
        // override X/Z from the saved transition coords before the first tick.
        // This is provisional: once the scene's collision grid + object cells
        // load (just below) the spawn is resolved to an in-bounds, standable
        // tile via `World::resolve_cold_field_spawn` - which keeps this exact
        // seat for town01 (and any scene where `0xA40` is standable, reachable,
        // and not a teleport-door tile) and relocates every other scene onto a
        // door-arrival anchor or the centroid of its largest connected walkable
        // region. See [`crate::world::FIELD_COLD_SPAWN_XZ`].
        if let Some(player) = self.world.actors.get_mut(0) {
            player.move_state.world_x = crate::world::FIELD_COLD_SPAWN_XZ;
            player.move_state.world_y = 0;
            player.move_state.world_z = crate::world::FIELD_COLD_SPAWN_XZ;
        }
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
            Some(bytes) => {
                self.world.load_field_object_cells(
                    bytes
                        .get(legaia_asset::field_objects::OBJECT_GRID_OFFSET..)
                        .unwrap_or_default(),
                );
                self.world.load_field_elevation_overrides(
                    bytes
                        .get(crate::field_regions::MAP_REGION_BLOCK_OFFSET..)
                        .unwrap_or_default(),
                    bytes
                        .get(crate::field_regions::MAP_TRIGGER_FALLBACK_OFFSET..)
                        .unwrap_or_default(),
                );
            }
            None => {
                self.world.field_object_cells.clear();
                self.world.field_elevation_overrides.clear();
            }
        }
        // Resolve the provisional cold spawn against the just-loaded collision
        // grid + object cells. `resolve_cold_field_spawn` returns the retail
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
        let resolved = self
            .world
            .resolve_cold_field_spawn(&teleport_tiles, &anchors);
        // Remembered for the helper-context teardown rescue: a spawned
        // record that ends with the player inside a wall re-seats them here
        // (see `World::step_helper_contexts`).
        self.world.resolved_cold_spawn = Some(resolved);
        if resolved
            != (
                crate::world::FIELD_COLD_SPAWN_XZ,
                crate::world::FIELD_COLD_SPAWN_XZ,
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
        self.world.field_prop_colliders = Vec::new();
        self.world.field_prop_bank = Default::default();
        self.world.pending_prop_touch = None;
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
                self.world.field_floor_height_lut = lut.map(|v| v.wrapping_neg());
            }
            Some(Err(err)) => eprintln!("[scene] field floor-height LUT load skipped: {err:#}"),
            _ => {}
        }
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
        let man_encounter = match self.scene.as_ref() {
            Some(scene) => match scene.field_man_encounter_table(&self.index, name) {
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
                let cat = crate::monster_catalog::catalog_from_monster_archive(&archive, &ids);
                for def in cat.by_id.into_values() {
                    self.world.monster_catalog.insert(def);
                }
                // Pair the per-move power table with the just-merged monster
                // stats so the special-attack damage path can resolve real
                // per-move power (PROT 0898; falls back to the placeholder if
                // the disc read fails).
                self.ensure_move_power_table();
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
            .and_then(|man| crate::region_encounter::region_encounter_table_from_man(name, man));
        self.world.set_field_regions(field_region_table);
        // Install the scene's field entity-SM carriers derived from the same
        // MAN actor-placement partition (retail builds one record per
        // MAN-placed entity at scene load). They sit Idle - the sparring
        // carrier only advances when `engage_field_carrier` is called on the
        // dialogue-accept - so this is inert for the cold-boot path but makes
        // the derived carrier set live (and is the counterpart to the MAN
        // encounter-table install above). Soft-fail: a scene without a MAN, or
        // with no interactable placements, just installs an empty set.
        match self
            .scene
            .as_ref()
            .map(|s| s.field_man_payload(&self.index))
        {
            Some(Ok(Some(man_bytes))) => match legaia_asset::man_section::parse(&man_bytes) {
                Ok(man_file) => {
                    self.world
                        .install_field_carriers_from_man(&man_file, &man_bytes);
                    // Initial NPC facings: retail's placement installer
                    // (`FUN_8003A1E4`) pre-runs each record's spawn prologue
                    // at scene load, and the prologue's `0x4C 0x51` / `0x38`
                    // ops write the actor's `+0x26` heading from the SCUS
                    // direction LUT. Seed those headings now (after the
                    // carrier install, which clears the heading map) so a
                    // never-walked NPC stands with its retail facing.
                    // REF: FUN_8003A1E4
                    self.world.seed_field_npc_facings(&man_file, &man_bytes);
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
                    let triggers: Vec<crate::field_regions::TileTrigger> = self
                        .field_triggers
                        .0
                        .iter()
                        .chain(self.field_triggers.1.iter())
                        .copied()
                        .collect();
                    type Bind = (
                        (i16, i16),
                        crate::man_field_scripts::WalkTouchEvent,
                        Option<usize>,
                    );
                    let binds: Vec<Bind> = match map_bytes.as_deref() {
                        Some(map) => crate::man_field_scripts::object_walk_touch_binds(
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
                                let event = crate::man_field_scripts::flat_record_walk_touch_event(
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
                .set_vdf_buffer(crate::scene_bundle::find_vdf_buffer(scene));
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
        let move_install = self
            .scene
            .as_ref()
            .and_then(|scene| crate::scene_bundle::find_bundle(scene).map(|b| (b.entry_idx(), b)));
        if let Some((entry_idx, bundle)) = move_install {
            match self
                .index
                .entry_bytes_extended(entry_idx)
                .and_then(|bytes| crate::scene_bundle::extract_move_payload(&bundle, &bytes))
            {
                Ok(Some(bytes)) => self.world.set_move_buffer_root(bytes),
                Ok(None) => self.world.set_move_buffer_root(Vec::new()),
                Err(err) => eprintln!("[scene] move-table extract skipped: {err:#}"),
            }
        }
        // Seed the global TMD-pool head from PROT 0874 section 0 (the
        // 5 character-mesh TMDs that retail's `DAT_8007C018[0..4]`
        // resolves to). Byte-equality verified - see
        // `project_global_tmd_pool_source.md`. Producers for the
        // remaining 138 kingdom-bundle entries are not yet pinned;
        // those slots stay empty until the full chain lands. Idempotent:
        // the head re-seeds across scene transitions but only on the
        // first call (subsequent calls early-return when the head is
        // already populated).
        let head_populated = self.world.global_tmd_pool.len() >= 5
            && self.world.global_tmd_pool[..5].iter().all(|s| s.is_some());
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
            for name in crate::scene_resources::FIELD_SHARED_BLOCKS {
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
            let load_kind = if crate::scene::is_world_map_scene(name) {
                crate::scene_resources::SceneLoadKind::WorldMap
            } else {
                crate::scene_resources::SceneLoadKind::Field
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
                crate::scene_resources::SceneResources::build_targeted_with_options(
                    scene,
                    &shared_refs,
                    crate::scene_resources::BuildOptions {
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
                // Upload the battle effect-model textures (etim.dat, PROT 0874
                // section 2) into the scene VRAM so the 3D effect models
                // (etmd.dat) have their texels resident. Kept across the battle
                // scene-mode overlay; soft-fails (textures just stay absent).
                if let Err(err) = upload_effect_textures_into_vram(&self.index, &mut res.vram, true)
                {
                    eprintln!("[scene] effect-texture VRAM upload skipped: {err:#}");
                }
                self.world.init_scene_animations(&res);
                self.resources = Some(res);
            }
        }
        // Opening-prologue hand-off arm. When entering the cutscene scene
        // `opdeene`, derive the `town01` hand-off arm from the scene's own MAN
        // bytecode instead of a blind constant: walk the cutscene-timeline
        // partition for the `GFLAG_SET 26` write retail's gate waits on and
        // arm only when it is present. A cutscene scene that never issues that
        // write never produces a false hand-off. See
        // [`crate::world::World::arm_prologue_handoff_from_man`].
        if name == legaia_asset::new_game::OPENING_CUTSCENE_SCENE {
            // The New-Game opening chain starts here: while it plays (through
            // the `opstati` / `opurud` legs the timelines chain into), a
            // confirm press with the hand-off bit armed skips the whole
            // remaining opening to `town01` (retail `FUN_801D1344`).
            self.world.opening_chain_active = true;
            // Decode the "It was the Seru." caption image from the scene's
            // geometry pack (PROT 0749). It is a baked TIM, not text - the host
            // blits it, faded, in the gap between the two narration crawls (see
            // `crate::cutscene_caption`). `None` when the disc / entry is absent.
            if let Some(scene) = self.scene.as_ref() {
                self.world.cutscene_caption =
                    crate::cutscene_caption::decode_opdeene_caption(scene);
                if self.world.cutscene_caption.is_some() {
                    log::info!("prologue: decoded 'It was the Seru.' caption image (PROT 0749)");
                }
            }
            match self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            {
                Some(Ok(Some(man_bytes))) => match legaia_asset::man_section::parse(&man_bytes) {
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
                                legaia_asset::new_game::OPENING_CUTSCENE_SCENE,
                                legaia_asset::new_game::OPENING_SCENE,
                                crate::world::PROLOGUE_HANDOFF_BIT,
                            );
                        } else if self
                            .world
                            .arm_prologue_handoff_from_man(&man_file, &man_bytes)
                        {
                            log::info!(
                                "prologue: armed '{}' -> '{}' hand-off from MAN GFLAG_SET {} (static fallback)",
                                legaia_asset::new_game::OPENING_CUTSCENE_SCENE,
                                legaia_asset::new_game::OPENING_SCENE,
                                crate::world::PROLOGUE_HANDOFF_BIT,
                            );
                        }
                        // The inline narration is script-driven: the timeline
                        // stepper installs each block's pages on the roller
                        // presenter when its PC reaches the block's op and
                        // suspends the timeline until the crawl completes
                        // (retail `FUN_80037174`). Nothing to install here.
                        if let Some(tl) = self.world.cutscene_timeline.as_ref() {
                            log::info!(
                                "prologue: '{}' timeline carries {} narration block(s)",
                                legaia_asset::new_game::OPENING_CUTSCENE_SCENE,
                                tl.narration_blocks.len(),
                            );
                        }
                    }
                    Err(err) => eprintln!("[scene] prologue MAN parse skipped: {err:#}"),
                },
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
        if name == legaia_asset::new_game::OPENING_SCENE
            && (self.world.entering_town01_opening || self.world.opening_chain_active)
        {
            self.world.entering_town01_opening = false;
            // Arriving at Rim Elm ends the opening cutscene chain, whether via
            // the skip packet or the natural scene-change chain.
            self.world.opening_chain_active = false;
            self.world.cutscene_narration = None;
            self.world.cutscene_card = None;
            match self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            {
                Some(Ok(Some(man_bytes))) => match legaia_asset::man_section::parse(&man_bytes) {
                    Ok(man_file) => {
                        if self
                            .world
                            .install_town01_opening_timeline(&man_file, &man_bytes)
                        {
                            log::info!(
                                "opening: executing '{}' opening timeline (P2[{}]); name entry opens at its op-0x49 STATE_RESUME",
                                legaia_asset::new_game::OPENING_SCENE,
                                crate::world::World::TOWN01_OPENING_TIMELINE_RECORD,
                            );
                        }
                    }
                    Err(err) => eprintln!("[scene] town01 opening MAN parse skipped: {err:#}"),
                },
                Some(Err(err)) => eprintln!("[scene] town01 opening MAN payload skipped: {err:#}"),
                _ => {}
            }
        }
        // Decode the scene's gold-shop stock from its MAN so the field-menu
        // shop-open path offers real per-scene items at real prices instead of a
        // hand-authored list. Cheap when the scene has no merchant.
        self.populate_scene_shops();
        // Drain any pending transition the previous scene left behind.
        self.world.pending_scene_transition = None;
        Ok(())
    }

    /// Decode the active scene's gold shops from its MAN(s) and park them on
    /// [`crate::world::World::scene_shops`], priced from
    /// [`crate::world::World::item_shop_data`]. Scans every entry in the scene's
    /// CDNAME block (most carry one bundle MAN); cheap for non-bundle entries -
    /// the locator returns early without decompressing when an entry isn't a
    /// scene bundle with a MAN. No-op shop list when the disc / item data is
    /// absent.
    fn populate_scene_shops(&mut self) {
        let entry_idxs: Vec<u32> = match self.scene.as_ref() {
            Some(s) => s.entries.iter().map(|e| e.idx).collect(),
            None => {
                self.world.scene_shops.clear();
                return;
            }
        };
        let item_data = self.world.item_shop_data.clone();
        let mut shops = Vec::new();
        for idx in entry_idxs {
            let bytes = match self.index.entry_bytes_extended(idx) {
                Ok(b) => b,
                Err(_) => continue,
            };
            shops.extend(crate::shop_catalog::scene_shops(
                &bytes,
                idx as usize,
                item_data.as_ref(),
            ));
        }
        self.world.scene_shops = shops;
    }

    /// Enter `name` as the **overworld** (world-map) scene.
    ///
    /// The counterpart to [`Self::enter_field_scene`] for the three kingdom
    /// overworld scenes ([`is_world_map_scene`]). It runs the full field-entry
    /// load first - the world-map-walk overlay shares the field's locomotion,
    /// walkability grid, and asset pipeline (see
    /// `docs/subsystems/world-map.md`) - then:
    ///
    /// 1. Routes the region-keyed random-encounter table from the scene's MAN
    ///    ([`crate::region_encounter::region_encounter_table_from_man`]) onto
    ///    the overworld via [`crate::world::World::set_world_map_regions`], so
    ///    `tick_world_map`'s per-tile roll fires real encounters.
    /// 2. Switches the world into [`crate::world::SceneMode::WorldMap`]
    ///    (installs the camera controller); the player actor + collision grid
    ///    that `enter_field_scene` set up stay in place.
    ///
    /// 3. Installs the scene's **interactive overworld entities** - the
    ///    portals (town/dungeon entrances) and dialog NPCs decoded from the
    ///    MAN actor-placement table and classified by their field-VM scripts
    ///    ([`crate::man_field_scripts::classify_placements`]). Decorative /
    ///    model-only placements are skipped; the entity *kind* comes from the
    ///    per-entity script, so this is disc-sourced, not synthetic.
    ///
    /// The random-encounter driver (the region table) is fully sourced from
    /// the MAN; the per-entity auto-engage trigger (walk onto a portal tile)
    /// is still host-driven via [`crate::world::World::engage_world_map_entity`].
    pub fn enter_world_map_scene(&mut self, name: &str) -> Result<()> {
        // Full field-entry load: resources, walkability grid, player, monster
        // catalog, scene label. Leaves the world in `Field` mode.
        self.enter_field_scene(name, 0)?;
        // Decode the MAN once, then derive the region table + the typed entity
        // configs from it while only `self.scene` / `self.index` are borrowed
        // (both immutable), so the owned results outlive the borrow before the
        // mutable `world` accesses below.
        let man_bytes = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten());
        let table = man_bytes
            .as_ref()
            .and_then(|man| crate::region_encounter::region_encounter_table_from_man(name, man));
        // Each interactive placement → (config, spawn world position). The
        // positions drive the auto-engage-on-walkover trigger; Plain
        // (decorative / model-only) placements are skipped.
        let mut entities: Vec<(crate::world::WorldMapEntityConfig, (i16, i16))> = man_bytes
            .as_ref()
            .and_then(|man| {
                legaia_asset::man_section::parse(man)
                    .ok()
                    .map(|mf| (mf, man))
            })
            .map(|(mf, man)| {
                use crate::man_field_scripts::PlacementKind;
                crate::man_field_scripts::classify_placements(&mf, man)
                    .into_iter()
                    .filter_map(|(p, kind)| {
                        let cfg = match kind {
                            PlacementKind::Portal { target_map } => {
                                crate::world::WorldMapEntityConfig::Portal {
                                    target_map: target_map as u16,
                                }
                            }
                            PlacementKind::Npc {
                                interact_id,
                                dialog_inline,
                            } => crate::world::WorldMapEntityConfig::Npc {
                                interact_id: interact_id.unwrap_or(0),
                                // No `text_id` from the MAN classifier: the `0x3F`
                                // op it was sourced from is the scene-change
                                // opcode, not a dialog op. The real NPC text is the
                                // structural `inline` block below.
                                text_id: None,
                                inline: dialog_inline.unwrap_or_default(),
                            },
                            PlacementKind::Plain => return None,
                        };
                        Some((cfg, (p.world_x, p.world_z)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Overworld town/dungeon entrances: the `.MAP` walk-on kind-1 tile
        // triggers joined to their partition-2 records' `0x3F`
        // named-scene-change ops (see
        // [`crate::man_field_scripts::overworld_portal_sites`]). This is the
        // real overworld hop mechanism - `map01` has NO partition-1 `Portal`
        // placements (the classifier finds zero there); its town/dungeon
        // entrances are these gate-1 triggers. Each site becomes an
        // `OverworldPortal` entity at its trigger tile centre
        // (`world = tile*128 + 0x40`, the same tile->world mapping the placement
        // spawns and the arrival seat use), and the auto-engage-on-walkover
        // trigger fires it when the player steps onto that tile.
        if let (Some(man), Some(scene)) = (man_bytes.as_ref(), self.scene.as_ref())
            && let Ok(mf) = legaia_asset::man_section::parse(man)
            && let Ok((primary, fallback)) = scene.field_tile_triggers(&self.index)
        {
            let mut triggers = primary;
            triggers.extend(fallback);
            for site in crate::man_field_scripts::overworld_portal_sites(&mf, man, &triggers) {
                // Story-gate the entrance by its partition-2 record's C1/C2
                // gates (retail `FUN_8003BDE0`): C1 blocks the spawn if ANY
                // listed flag is set, C2 requires ALL set. Most overworld
                // entrances carry empty gates and install unconditionally; the
                // Ravine (`keikoku`) portals carry `C1=[0x193]` so they drop
                // out of the installed set once that story flag latches. A fresh
                // Drake arrival (flag clear) keeps them reachable. See
                // [`docs/subsystems/world-map.md`].
                if let Some((c1, c2)) = crate::man_field_scripts::partition2_record_gates(
                    &mf,
                    man,
                    site.record as usize,
                ) && !self.world.p2_record_gates_pass(&c1, &c2)
                {
                    continue;
                }
                let world = (
                    i16::from(site.overworld_x) * 128 + 0x40,
                    i16::from(site.overworld_z) * 128 + 0x40,
                );
                // Story-conditional entrance: when the record selects its
                // destination by an op-0x70 flag branch (retail's post-beat
                // dungeon-variant entrance, e.g. `map01`'s dolk -> dolk2 on flag
                // `0x142`), resolve to the flag-SET alternative once that story
                // flag latches; otherwise the primary (flag-CLEAR) destination
                // stands. Mirrors the op-0x70 semantics in the field VM.
                let (scene_name, index, entry_x, entry_z, dir) = match site.conditional {
                    Some(cd) if self.world.system_flag_test(cd.flag) => {
                        (cd.scene_name, cd.index, cd.entry_x, cd.entry_z, cd.dir)
                    }
                    _ => (
                        site.scene_name,
                        site.index,
                        site.entry_x,
                        site.entry_z,
                        site.dir,
                    ),
                };
                entities.push((
                    crate::world::WorldMapEntityConfig::OverworldPortal {
                        scene_name,
                        index,
                        entry_x,
                        entry_z,
                        dir,
                    },
                    world,
                ));
            }
        }
        if let Some(table) = table {
            log::info!(
                "world-map '{name}': routed {} encounter region(s)",
                table.regions.len()
            );
            self.world.set_world_map_regions(table);
        }
        if !entities.is_empty() {
            log::info!(
                "world-map '{name}': installed {} interactive entit(ies) from placements",
                entities.len()
            );
            self.world.install_world_map_entities_at(entities);
        }
        // Switch to world-map mode (idempotent; keeps the installed player +
        // collision grid).
        self.world.enter_world_map();
        Ok(())
    }

    /// Spawn the partition-2 record referenced by a gate-1 kind-1 tile
    /// trigger at the arrival tile - the retail walk-on dispatch
    /// (`FUN_801D1EC4` -> `FUN_801D5630` -> `FUN_8003BDE0`), which fires on
    /// the first post-arrival frame because the last-tile globals are stale.
    ///
    /// Scoped to the opening chain (this immediate same-tick spawn is the
    /// chain's fly-in / arrival mechanism; door records resolve through the
    /// dedicated warp path, and an ordinary scene's arrival tile is covered by
    /// the per-frame [`Self::dispatch_walk_on_trigger`] stale-tile compare on
    /// the first post-entry tick). Live-probe-pinned: retail's `map01` and
    /// `town01` opening records spawn exactly this way - the entry seat
    /// lands on the trigger tile and the stale-tile compare fires the same
    /// tick; `opstati` / `opurud` spawn via op-`0x44` in their entry scripts
    /// instead and never reach this path.
    // REF: FUN_801D1EC4, FUN_801D5630, FUN_8003BDE0
    fn spawn_arrival_trigger_record(&mut self, tile_x: u8, tile_z: u8) {
        if !self.world.opening_chain_active || self.world.cutscene_timeline_active() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Ok((primary, fallback)) = scene.field_tile_triggers(&self.index) else {
            return;
        };
        let Some(Ok(Some(man_bytes))) = self
            .scene
            .as_ref()
            .map(|s| s.field_man_payload(&self.index))
        else {
            return;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man_bytes) else {
            return;
        };
        let Some(trigger) =
            crate::field_regions::lookup_tile_trigger(&primary, &fallback, tile_x, tile_z)
                .filter(|t| t.gate == 1)
        else {
            return;
        };
        if self
            .world
            .install_gated_p2_record(&man_file, &man_bytes, trigger.record as usize)
        {
            log::info!(
                "opening: walk-on trigger at ({tile_x:#x},{tile_z:#x}) spawned P2[{}]",
                trigger.record,
            );
        }
    }

    /// Per-frame walk-on tile-trigger dispatch - the retail field loop's
    /// tile compare (`FUN_801D1EC4`): when the player crosses into a new
    /// 128-unit collision tile during free-roam field play, the `.MAP`
    /// kind-1 trigger table is consulted (`FUN_801D5630`) and a gate-1 hit
    /// spawns MAN partition-2 record `record` as a field-VM context
    /// (`FUN_8003BDE0`, C1/C2 story-flag gates checked inside
    /// [`crate::world::World::install_gated_p2_record`]).
    ///
    /// This is how town exits work: e.g. Rim Elm's south-gate tiles carry a
    /// gate-1 trigger for the partition-2 record whose script runs the
    /// `0x3F` named scene-change to `map01` - and how walk-on story beats
    /// (the post-naming Vahn's-house scenes) launch. A `None` last tile
    /// (scene entry / warp arrival) fires the trigger at the *current* tile,
    /// mirroring retail's stale last-tile globals on the first post-arrival
    /// frame.
    ///
    /// Skipped while a modal cutscene timeline owns the frame (a walk-on beat
    /// record is cutscene-class: it seizes the camera and locks locomotion,
    /// one at a time) and while any dialog or name entry is up. Concurrent
    /// helper contexts ([`crate::world::World::helper_contexts`]) do NOT
    /// block the dispatch.
    ///
    /// Runs in both plain field mode **and** the overworld (world-map) mode.
    /// On the overworld a gate-1 trigger whose record is a **portal** (carries
    /// a `0x3F` named scene-change) is left to the world-map entity SM (the
    /// `OverworldPortal` walk-onto path); only gate-1 **beat** records - the
    /// Drake mist-wall force-walk bands (`map01` partition-2 records gated on
    /// `C1=[0x482]`) and their kin - are spawned here as a cutscene timeline,
    /// exactly as a town walk-on beat is. This is what makes those bands honor
    /// their C1 one-shot latch on the overworld instead of never firing.
    /// `true` when partition-2 record `record` carries a `0x3F` named
    /// scene-change op (a portal / door record) - the same test
    /// [`crate::man_field_scripts::overworld_portal_sites`] uses to select
    /// entrance records. Used by the overworld walk-on dispatch to leave portal
    /// records to the entity SM and spawn only beat records. A clean
    /// fall-through walk from the record's true `pc0`; `false` on a bad span.
    // REF: FUN_8003BDE0
    fn p2_record_is_portal(
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        record: usize,
    ) -> bool {
        let Some((start, pc0, len)) =
            crate::man_field_scripts::partition_record_span(man_file, man, 2, record)
        else {
            return false;
        };
        let body = &man[start..start + len];
        let mut pc = pc0;
        while pc < body.len() {
            let Ok(insn) = legaia_asset::field_disasm::decode(body, pc) else {
                return false;
            };
            if insn.size == 0 {
                return false;
            }
            if matches!(
                insn.info,
                legaia_asset::field_disasm::InsnInfo::SceneChange { .. }
            ) {
                return true;
            }
            pc += insn.size;
        }
        false
    }

    // REF: FUN_801D1EC4, FUN_801D5630, FUN_8003BDE0
    fn dispatch_walk_on_trigger(&mut self) {
        let on_world_map = match self.world.mode {
            crate::world::SceneMode::Field => false,
            crate::world::SceneMode::WorldMap => true,
            _ => return,
        };
        if self.world.cutscene_timeline_active()
            || self.world.name_entry_active()
            || self.world.current_dialog.is_some()
            || self.world.tile_board.is_some()
            || self.world.active_fmv().is_some()
        {
            return;
        }
        let Some(slot) = self.world.player_actor_slot else {
            return;
        };
        let Some(actor) = self.world.actors.get(slot as usize) else {
            return;
        };
        // Retail's tile quantisation **in this dispatcher** is the raw
        // `world >> 7` (`FUN_801D1EC4` at `0x801d2068`: `sll 0x10; sra 0x17`
        // on each of `player+0x14` / `+0x18`), not the `(world - 0x40) >> 7`
        // form the region refresh uses. The two agree at tile centres and
        // differ by a half-tile band elsewhere; the kind-0 door tiles are one
        // tile deep, so the band matters.
        let quant = |w: i16| -> i32 { i32::from(w) >> 7 };
        let (tx, tz) = (
            quant(actor.move_state.world_x),
            quant(actor.move_state.world_z),
        );
        if !(0..=0x7F).contains(&tx) || !(0..=0x7F).contains(&tz) {
            return;
        }
        let tile = (tx as u8, tz as u8);
        if self.last_trigger_tile == Some(tile) {
            return; // same tile as last tick - triggers fire on crossings
        }
        self.last_trigger_tile = Some(tile);
        self.dispatch_kind1_walk_on(tile, on_world_map);
        // Retail runs the kind-0 arm on the SAME crossing, after the kind-1
        // spawn (`FUN_801D1EC4` falls through to `0x801d21c0`), so a tile can
        // both spawn its record and teleport.
        if !on_world_map {
            self.dispatch_intra_scene_teleport(tile);
        }
    }

    /// The kind-1 arm of the tile-crossing dispatch: a gate-1 trigger spawns
    /// its partition-2 record.
    // REF: FUN_801D1EC4, FUN_8003BDE0
    fn dispatch_kind1_walk_on(&mut self, tile: (u8, u8), on_world_map: bool) {
        let (primary, fallback) = &self.field_triggers;
        let Some(trigger) =
            crate::field_regions::lookup_tile_trigger(primary, fallback, tile.0, tile.1)
                .filter(|t| t.gate == 1)
        else {
            return;
        };
        let Some(man_bytes) = self.field_man_cache.clone() else {
            return;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man_bytes) else {
            return;
        };
        // On the overworld, a gate-1 trigger whose record is a **portal**
        // (carries a `0x3F` named scene-change) belongs to the world-map entity
        // SM (`OverworldPortal`), which already gates + engages it on walk-onto.
        // Dispatching it here too would double-handle the hop (and would let a
        // story-gated-out entrance fall through to a raw timeline scene-change).
        // Only spawn gate-1 **beat** records - the mist-wall force-walk bands -
        // as a cutscene timeline on the overworld.
        if on_world_map && Self::p2_record_is_portal(&man_file, &man_bytes, trigger.record as usize)
        {
            return;
        }
        if self
            .world
            .install_gated_p2_record(&man_file, &man_bytes, trigger.record as usize)
        {
            log::info!(
                "{}: walk-on trigger at ({},{}) spawned P2[{}]",
                if on_world_map { "world-map" } else { "field" },
                tile.0,
                tile.1,
                trigger.record,
            );
        }
    }

    /// The **kind-0** arm: the `.MAP`'s intra-scene-teleport table - the door
    /// class whose destination is map data, not a MAN script.
    ///
    /// This is how most house **exits** work. The interior is a sub-area of the
    /// same collision grid; the exit is a plain tile just inside the doorway
    /// carrying a kind-0 record, and crossing onto it repositions the player.
    /// No object, no script, no ＩＮ/ＯＵＴ record name - so a MAN-only door
    /// census cannot see it, and an engine that dispatches only the kind-1
    /// table lets the player walk in and never back out.
    ///
    /// Retail seats the player, re-samples the floor height, resets the camera
    /// and then re-queries the **kind-1** table at the landing tile so the
    /// arrival's own record spawns; the engine gets the arrival record by
    /// leaving the last-tile compare stale, which fires it on the next tick.
    /// (Retail also runs a ~0x26-frame fade across the reposition; the engine
    /// warps instantly.)
    ///
    /// PORT: FUN_801D1EC4 (kind-0 arm, `0x801d21c0..0x801d2268`)
    fn dispatch_intra_scene_teleport(&mut self, tile: (u8, u8)) {
        let (primary, fallback) = &self.field_intra_teleports;
        let Some(tp) =
            crate::field_regions::lookup_intra_scene_teleport(primary, fallback, tile.0, tile.1)
        else {
            return;
        };
        let Some(slot) = self.world.player_actor_slot else {
            return;
        };
        let Some(actor) = self.world.actors.get(slot as usize) else {
            return;
        };
        // Retail skips the teleport while the player's movement-disabled flag
        // (`+0x10 & 0x80000`) is set - an encounter / cutscene owns the body.
        if actor.move_state.flags & 0x0008_0000 != 0 {
            return;
        }
        let (wx, wz) = tp.dest_world();
        let y = self
            .world
            .sample_field_floor_height(i32::from(wx), i32::from(wz)) as i16;
        if let Some(actor) = self.world.actors.get_mut(slot as usize) {
            actor.move_state.world_x = wx;
            actor.move_state.world_z = wz;
            actor.move_state.world_y = y;
        }
        // Arrival tile is a fresh crossing: leaving the compare stale makes the
        // next tick run the landing tile's own kind-1 record (retail queries it
        // inline at `0x801d2030`).
        self.last_trigger_tile = None;
        self.world
            .pending_field_events
            .push(crate::field_events::FieldEvent::MoveTo {
                world_x: wx as u16,
                world_z: wz as u16,
                is_player: true,
            });
        log::info!(
            "field: intra-scene teleport at ({},{}) -> world ({wx},{wz}) tile {:?}",
            tile.0,
            tile.1,
            tp.dest_tile(),
        );
    }

    /// One frame: tick the world, materialize any actor-spawn requests
    /// queued by the field VM's `0x4C 0x80` opcode, then process any
    /// pending `scene_transition(map_id)` request. Returns the
    /// [`SceneTickEvent`] describing what happened.
    ///
    /// A transition whose resolved scene is an overworld scene
    /// ([`is_world_map_scene`]) routes through [`Self::enter_world_map_scene`]
    /// (world-map mode + region table) instead of the plain field path, so the
    /// boot/transition path seeds the overworld the same way the explicit
    /// `--world-map` entry does.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        let was_battle = matches!(self.world.mode, crate::world::SceneMode::Battle);
        let _ = self.world.tick();
        // Post-battle field return: retail re-enters the field scene after a
        // battle (game-mode battle -> field reload), which re-runs the
        // scene-entry system script `P1[0]` from the MAN - the "P2 timeline
        // re-scan". That re-run is how a post-battle beat record spawns:
        // rikuroa's `P1[0]` tests the battle-staged marker `0x289` and issues
        // the op-`0x44` spawn of the post-victory record `P2[50]` (C1-gated on
        // `0x142`, the self-latching one-shot), whose own script bytes SET the
        // gate flag. The engine keeps the world state in place across the
        // battle, so the re-entry's script side is reproduced by reloading the
        // entry script here.
        // REF: FUN_8003ab2c (system-script rebuild on field entry)
        if was_battle && matches!(self.world.mode, crate::world::SceneMode::Field) {
            let entry_script = self
                .scene
                .as_ref()
                .and_then(|scene| scene.field_man_entry_script(&self.index).ok().flatten());
            if let Some((bytecode, pc0)) = entry_script {
                self.world.load_field_script_at(bytecode, pc0);
                log::info!("field: post-battle return re-ran the scene-entry script");
            }
        }
        self.world
            .materialize_actor_spawns(crate::world::FIELD_SPAWN_START_SLOT);
        // Field-VM op-0x44 SPAWN_RECORD: resolve each queued request against
        // the current scene MAN and install the partition-2 record as a
        // spawned context (retail FUN_8003BDE0 runs every spawned record as
        // an independent field-VM context). Cutscene-class spawns - the
        // opening chain's opstati / opurud entry scripts launching their
        // prologue records - install as THE modal cutscene timeline (cutscene
        // camera + locomotion lock + the chain's beat sequencing); an
        // ordinary scene's mid-play helper spawn installs as a concurrent
        // helper context that executes without seizing either.
        let pending_spawns = std::mem::take(&mut self.world.pending_record_spawns);
        if !pending_spawns.is_empty()
            && let Some(Ok(Some(man_bytes))) = self
                .scene
                .as_ref()
                .map(|s| s.field_man_payload(&self.index))
            && let Ok(man_file) = legaia_asset::man_section::parse(&man_bytes)
        {
            for global_index in pending_spawns {
                if self.world.opening_chain_active {
                    // Opening-chain sequencing: one modal beat at a time. A
                    // request issued while a beat still plays is dropped -
                    // the retail opening never issues one - preserving the
                    // chain's install-at-scene-entry cadence.
                    if !self.world.cutscene_timeline_active()
                        && self
                            .world
                            .install_spawned_record(&man_file, &man_bytes, global_index)
                    {
                        log::info!(
                            "opening: op-0x44 spawned P2 record (global index {global_index}) as the cutscene timeline",
                        );
                    }
                } else if self.world.install_spawned_helper_record(
                    &man_file,
                    &man_bytes,
                    global_index,
                ) {
                    log::info!(
                        "field: op-0x44 spawned P2 record (global index {global_index}) as a concurrent helper context",
                    );
                }
            }
        }
        // Walk-on tile trigger: the per-frame tile compare that spawns a
        // gate-1 trigger's partition-2 record when the player crosses onto
        // its tile - town exits + walk-on story beats. Runs after the world
        // stepped (player position is current) and before the transition
        // drains (a record installed this frame steps from the next tick).
        self.dispatch_walk_on_trigger();
        // Named scene-change (field-VM op 0x3F) takes precedence over the
        // map-id door-warp: its destination name is carried inline by the op,
        // so it loads directly without the map-id resolver. This is the live
        // consumer of the disc-sourced scene-destination data - the same names
        // [`crate::man_field_scripts::scene_destinations`] catalogs.
        if let Some((name, entry_x, entry_z, dir)) =
            self.world.pending_named_scene_transition.take()
        {
            // Drop a stale map-id request from the same frame; the named target
            // is unambiguous.
            self.world.pending_scene_transition = None;
            if is_world_map_scene(&name) {
                self.enter_world_map_scene(&name)?;
            } else {
                self.enter_field_scene(&name, 0)?;
            }
            // A warp arrival seats the player at the op-0x3F entry tile
            // (overriding the cold-boot spawn / stale overworld position), so
            // the player stands at the destination door - a town exit onto
            // the overworld arrives on the continent beside that town (e.g.
            // Rim Elm -> map01 tile (0x60, 0x19)), not at the map origin.
            self.world.seat_player_at_tile(entry_x, entry_z);
            // ...facing the op's trailing `dir` compass sector (retail
            // resolves it through the SCUS 0x80073F04 table into the
            // arrival-facing global; the engine sets the heading directly).
            self.world.face_player_sector(dir);
            // Walk-on tile trigger at the arrival tile: retail's per-frame
            // tile compare fires immediately on the first post-arrival frame
            // (the last-tile globals are stale from the previous scene), so
            // an arrival onto a gate-1 kind-1 trigger spawns its partition-2
            // record - this is how the opening chain's `map01` fly-in and the
            // natural `town01` opening launch.
            self.spawn_arrival_trigger_record(entry_x, entry_z);
            return Ok(SceneTickEvent::SceneEntered { name });
        }
        // GAP-1: overworld-portal scene transition. The world-map entity SM
        // emits [`crate::field_events::FieldEvent::WorldMapTransition`] when the
        // player walks onto a portal tile
        // ([`crate::world::World::auto_engage_world_map_portals`] ->
        // `on_scene_transition`); without a consumer it was dropped to the
        // BGM-router `leftover`. Drain it here (the overworld->dungeon hop) and
        // load the destination.
        //
        // The `slot` points back at the engaged entity's config:
        // - an [`crate::world::WorldMapEntityConfig::OverworldPortal`] carries
        //   the exact CDNAME destination + arrival tile (the `map01` `0x3F`
        //   bridge), so it loads that scene and seats the player at the entry
        //   tile - the same arrival semantics as the named `0x3F` warp above;
        // - a door-warp [`crate::world::WorldMapEntityConfig::Portal`] carries
        //   only the 7-id door `map_id`, resolved through the [`MapIdResolver`]
        //   (the same `0..=6` scene-*type* space the field-VM `0x3E` warp uses).
        if let Some((target_map, slot)) = self.world.take_world_map_transition() {
            self.world.pending_scene_transition = None;
            match self.world.world_map_entity_configs.get(slot as usize) {
                Some(crate::world::WorldMapEntityConfig::OverworldPortal {
                    scene_name,
                    entry_x,
                    entry_z,
                    dir,
                    ..
                }) => {
                    let name = scene_name.clone();
                    let (entry_x, entry_z, dir) = (*entry_x, *entry_z, *dir);
                    if is_world_map_scene(&name) {
                        self.enter_world_map_scene(&name)?;
                    } else {
                        self.enter_field_scene(&name, 0)?;
                    }
                    self.world.seat_player_at_tile(entry_x, entry_z);
                    self.world.face_player_sector(dir);
                    self.spawn_arrival_trigger_record(entry_x, entry_z);
                    return Ok(SceneTickEvent::SceneEntered { name });
                }
                _ => {
                    // Door-warp portal (or a portal whose config row is gone):
                    // resolve the 7-id door `map_id` through the resolver.
                    match self.map_resolver.resolve(target_map as u8) {
                        Some(name) => {
                            if is_world_map_scene(&name) {
                                self.enter_world_map_scene(&name)?;
                            } else {
                                self.enter_field_scene(&name, 0)?;
                            }
                            return Ok(SceneTickEvent::SceneEntered { name });
                        }
                        None => {
                            return Ok(SceneTickEvent::UnknownMapId {
                                map_id: target_map as u8,
                            });
                        }
                    }
                }
            }
        }
        if let Some(map_id) = self.world.pending_scene_transition.take() {
            match self.map_resolver.resolve(map_id) {
                Some(name) => {
                    if is_world_map_scene(&name) {
                        self.enter_world_map_scene(&name)?;
                    } else {
                        self.enter_field_scene(&name, 0)?;
                    }
                    return Ok(SceneTickEvent::SceneEntered { name });
                }
                None => {
                    return Ok(SceneTickEvent::UnknownMapId { map_id });
                }
            }
        }
        Ok(SceneTickEvent::Stepped)
    }
}
