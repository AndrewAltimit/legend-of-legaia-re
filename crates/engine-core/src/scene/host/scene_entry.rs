//! `SceneHost` field/world-map scene entry, shop population, archive/move-power seeding, and per-frame tick.
//!
//! Extracted verbatim from `scene/host.rs` as an additional `impl SceneHost` block.

use super::*;

impl SceneHost {
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
        self.world.field_channels.clear();
        self.world.field_channels_man = None;
        self.world.field_npc_anim_cues.clear();
        // Reset the persistent op-0x45 camera-param set so a new scene's
        // cutscene shots start clean (the params now MERGE per-slot across beats
        // in `camera_configure`, so a stale set would leak the prior scene's
        // focus / depth into a beat that omits those slots).
        self.world.camera_state.params.clear();
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
        // Cold field entry: place the player at the retail cold-boot spawn.
        // `FUN_801D6704` creates the player actor at the camera-window centre
        // `(0xA40, 0, 0xA40)` on a non-warp entry; for the New Game opening
        // (town01) this is Vahn's authored Rim Elm spawn, and it also seeds the
        // follow camera onto the right region. Engines that arrive via a warp
        // override X/Z from the saved transition coords before the first tick.
        // See [`crate::world::FIELD_COLD_SPAWN_XZ`].
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
        // Static prop colliders: one box centre per placed `.MAP` object
        // (spawn position + the record's collision-footprint offset - the
        // static-entity arm of the actor probe). Installed unconditionally
        // (empty for scenes with no field map) so a stale scene's props
        // never leak across a transition; blocking stays behind the opt-in
        // `World::solid_field_npcs` flag.
        self.world.field_prop_colliders = match self.scene.as_ref() {
            Some(scene) => match scene.field_object_placements(&self.index) {
                Ok(Some(placements)) => placements
                    .iter()
                    .map(|p| (p.collider_x, p.collider_z))
                    .collect(),
                Ok(None) => Vec::new(),
                Err(err) => {
                    eprintln!("[scene] field prop-collider load skipped: {err:#}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
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
                        // Install the inline narration the cutscene-timeline
                        // partition (partition 2) carries, so the opening plays
                        // its subtitle pages before the Rim Elm hand-off.
                        let pages = crate::man_field_scripts::collect_partition_narration(
                            &man_file, &man_bytes, 2,
                        );
                        if !pages.is_empty() {
                            log::info!(
                                "prologue: '{}' carries {} inline narration page(s)",
                                legaia_asset::new_game::OPENING_CUTSCENE_SCENE,
                                pages.len(),
                            );
                            self.world.open_cutscene_narration(pages);
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
        if name == legaia_asset::new_game::OPENING_SCENE && self.world.entering_town01_opening {
            self.world.entering_town01_opening = false;
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
        let entities: Vec<(crate::world::WorldMapEntityConfig, (i16, i16))> = man_bytes
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
        let _ = self.world.tick();
        self.world
            .materialize_actor_spawns(crate::world::FIELD_SPAWN_START_SLOT);
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
            return Ok(SceneTickEvent::SceneEntered { name });
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
