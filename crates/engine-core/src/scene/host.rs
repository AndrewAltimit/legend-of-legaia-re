use super::*;

/// Per-tick outcome from [`SceneHost::tick`]. Engines route this back into
/// their UI layer (e.g. log scene transitions, update HUD on battle end).
#[derive(Debug, Clone)]
pub enum SceneTickEvent {
    /// World stepped normally - no scene-level events this frame.
    Stepped,
    /// Field VM requested a scene transition that the resolver mapped to
    /// `name`; the host loaded it and reset the field VM.
    SceneEntered { name: String },
    /// `scene_transition(map_id)` was requested but the resolver returned
    /// `None`. The host left the existing scene loaded; the engine can
    /// log / surface the unknown id.
    UnknownMapId { map_id: u8 },
}

/// BGM dispatch hook - implemented by the audio layer (or test stubs) and
/// driven by [`SceneHost::route_bgm_events`]. The default
/// [`NullBgmDirector`] discards every request.
///
/// Sub-op semantics mirror retail field-VM op `0x35` - see
/// [`docs/subsystems/script-vm.md`] for the full table. The hook only
/// receives sub-ops that change playback state (1 = start, 2 = pause,
/// 3 = resume, 4 = stop, 9 = queue); other sub-ops are control words
/// that the host can route without sequencer state.
pub trait BgmDirector {
    /// Start playing the given SEQ bytes for `bgm_id`. The bytes have
    /// already been resolved by the host through
    /// [`SceneHost::bgm_seq_bytes`]; the director parses + attaches them.
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        let _ = (bgm_id, seq_bytes);
    }
    fn pause(&mut self) {}
    fn resume(&mut self) {}
    fn stop(&mut self) {}
    /// Sub-op 9 - queue a BGM for later trigger. The bytes are pre-resolved
    /// like [`BgmDirector::start`].
    fn queue(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        let _ = (bgm_id, seq_bytes);
    }
}

/// Discards every BGM event. Useful for tests + engines that haven't wired
/// audio yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullBgmDirector;
impl BgmDirector for NullBgmDirector {}

/// Bundles the runtime composite (`World`) with a loaded `Scene`, a frame
/// timer, and a [`MapIdResolver`] for field-VM scene transitions. The host
/// owns the engine-vm world (per-actor data + every VM's `Host` impl) and
/// exposes a single `tick()` that drives the active VMs and processes any
/// transitions the field VM requested.
pub struct SceneHost {
    pub index: Arc<ProtIndex>,
    pub world: crate::world::World,
    pub scene: Option<Scene>,
    /// Typed asset snapshot for the currently loaded scene - refreshed
    /// every time [`SceneHost::load_scene`] or [`SceneHost::enter_field_scene`]
    /// runs. `None` until the first scene loads.
    pub assets: Option<crate::scene_assets::SceneAssets>,
    /// Runtime resource snapshot built by [`SceneHost::enter_field_scene`] -
    /// holds the populated PSX VRAM, parsed TMD pool, and parsed ANM packs.
    /// `None` until the first `enter_field_scene` call. Use for rendering
    /// and for driving `World::init_scene_animations`.
    pub resources: Option<crate::scene_resources::SceneResources>,
    pub frame_time: crate::FrameTime,
    /// Map-id → scene-name resolver for `scene_transition(map_id)`.
    /// Default is [`NullMapIdResolver`] so transitions are silently
    /// dropped until the engine wires its own table.
    pub map_resolver: Box<dyn MapIdResolver + Send + Sync>,
    /// Lazily-loaded monster stat archive (PROT entry 867, extended
    /// footprint). Cached because it's 15.9 MB and the same global table
    /// serves every scene. Populated on the first field entry that needs
    /// real monster stats. See [`legaia_asset::monster_archive`].
    monster_archive_cache: Option<Arc<Vec<u8>>>,
    /// Tracks whether the move-power table install was attempted, so the disc
    /// read (PROT 0898) only happens once per host even when it fails.
    move_power_loaded: bool,
    /// The current scene's disc-sourced **named scene-change destinations**
    /// (`0x3F` ops), decoded from its MAN on entry via
    /// [`crate::man_field_scripts::scene_destinations`]. Empty for scenes with
    /// no MAN / no destination table. Drives [`Self::destination_resolver`].
    scene_destinations: Vec<crate::man_field_scripts::SceneDestination>,
}

impl SceneHost {
    /// Build a host over an already-opened ProtIndex.
    pub fn new(index: Arc<ProtIndex>) -> Self {
        Self {
            index,
            world: crate::world::World::default(),
            scene: None,
            assets: None,
            resources: None,
            frame_time: crate::FrameTime::new(),
            map_resolver: Box::new(NullMapIdResolver),
            monster_archive_cache: None,
            move_power_loaded: false,
            scene_destinations: Vec::new(),
        }
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

    /// Open the host directly from an extracted directory.
    pub fn open_extracted(extracted_root: impl AsRef<Path>) -> Result<Self> {
        let p = ProtIndex::open_extracted(extracted_root.as_ref())?;
        Ok(Self::new(Arc::new(p)))
    }

    /// Open the host directly from a `.bin` disc image. The disc is walked
    /// once to extract `PROT.DAT` and `CDNAME.TXT` from the ISO9660 tree;
    /// the extracted bytes are then handed to [`ProtIndex::from_bytes`].
    ///
    /// This is the user-facing path: ship the engine, the user supplies a
    /// disc image, no extraction step needed. Native targets only - WASM
    /// uses `from_prot_bytes` with the bytes supplied via JS.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_disc(disc_bin: impl AsRef<Path>) -> Result<Self> {
        use crate::Vfs;
        let vfs = crate::DiscVfs::open(disc_bin.as_ref())?;
        let prot_bytes = vfs
            .read("prot.dat")
            .with_context(|| "PROT.DAT not present in disc image")?;
        // CDNAME.TXT may live at either DATA/CDNAME.TXT or top-level. The
        // ISO walker stores the path verbatim.
        let cdname_bytes = vfs
            .read("cdname.txt")
            .or_else(|_| vfs.read("data/cdname.txt"))
            .ok();
        let cdname_text = match cdname_bytes {
            Some(b) => Some(String::from_utf8(b).context("CDNAME.TXT is not valid UTF-8")?),
            None => None,
        };
        let p = ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())?;
        Ok(Self::new(Arc::new(p)))
    }

    /// Build a host from raw in-memory PROT.DAT bytes. WASM-safe - no
    /// filesystem access. Pass `cdname_text` if the CDNAME.TXT contents are
    /// available; omit to skip scene-name resolution.
    pub fn from_prot_bytes(prot_bytes: Vec<u8>, cdname_text: Option<&str>) -> Result<Self> {
        let p = ProtIndex::from_bytes(prot_bytes, cdname_text)?;
        Ok(Self::new(Arc::new(p)))
    }

    /// Replace the map-id → scene-name resolver. Call once at startup with
    /// the engine's preferred resolver.
    pub fn set_map_resolver(&mut self, resolver: Box<dyn MapIdResolver + Send + Sync>) {
        self.map_resolver = resolver;
    }

    /// Load (or reload) the active scene without entering it. The world's
    /// `SceneMode` is left untouched. Use [`enter_field_scene`] if you want
    /// the field VM kicked off too.
    ///
    /// [`enter_field_scene`]: SceneHost::enter_field_scene
    pub fn load_scene(&mut self, name: &str) -> Result<&Scene> {
        let scene = Scene::load(&self.index, name)?;
        let assets = crate::scene_assets::SceneAssets::build(&scene);
        self.scene = Some(scene);
        self.assets = Some(assets);
        self.refresh_scene_destinations();
        Ok(self.scene.as_ref().unwrap())
    }

    /// Decode + cache the just-loaded scene's named scene-change destinations
    /// (`0x3F` ops) from its MAN, via
    /// [`crate::man_field_scripts::scene_destinations`]. Clears to empty when
    /// the scene carries no MAN or it doesn't parse. Called by [`Self::load_scene`]
    /// so every scene-entry path keeps the table current.
    fn refresh_scene_destinations(&mut self) {
        self.scene_destinations = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten())
            .and_then(|man| {
                let mf = legaia_asset::man_section::parse(&man).ok()?;
                Some(crate::man_field_scripts::scene_destinations(&mf, &man))
            })
            .unwrap_or_default();
    }

    /// The current scene's disc-sourced **named scene-change destinations**
    /// (`0x3F` ops): every town / dungeon its controller script can warp to,
    /// each with its `i16` index + entry tile. Empty when no scene is loaded or
    /// the scene has no destination table. See
    /// [`crate::man_field_scripts::scene_destinations`].
    pub fn scene_destinations(&self) -> &[crate::man_field_scripts::SceneDestination] {
        &self.scene_destinations
    }

    /// A [`SceneDestinationResolver`] over the current scene's destinations -
    /// the live resolver for the `0x3F` named-scene-change `i16` index space,
    /// rebuilt from disc each scene entry. (The `0x3E` door-warp keeps the
    /// separate `u8`-keyed [`map_resolver`](Self::map_resolver).)
    pub fn destination_resolver(&self) -> SceneDestinationResolver {
        SceneDestinationResolver::new(self.scene_destinations.clone())
    }

    /// Borrow the current scene's typed asset snapshot. `None` if no scene
    /// is loaded.
    pub fn assets(&self) -> Option<&crate::scene_assets::SceneAssets> {
        self.assets.as_ref()
    }

    /// Resolve a BGM id to the raw SEQ bytes the runtime would pass to its
    /// sequencer. Mirrors `FUN_800243F0` (the BGM resolver): scene-local ids
    /// (`< 2000`) live at `block_start + 6 + id`; global-pool ids
    /// (`>= 2000`) are not modeled. Returns `None` when no scene is loaded
    /// or no SEQ-bearing entry maps to the id.
    ///
    /// Engines parse the returned bytes with [`legaia_seq::Seq::parse`] and
    /// attach to [`legaia_engine_audio::Sequencer::new`] alongside the
    /// scene's VAB bank.
    pub fn bgm_seq_bytes(&self, bgm_id: u16) -> Result<Option<Arc<Vec<u8>>>> {
        let Some(assets) = self.assets.as_ref() else {
            return Ok(None);
        };
        let Some(entry_idx) = assets.bgm_seq_entry(bgm_id) else {
            return Ok(None);
        };
        let bytes = self.index.entry_bytes(entry_idx)?;
        let offset = assets.bgm_seq_offset(bgm_id).unwrap_or(0);
        if offset == 0 {
            Ok(Some(bytes))
        } else if offset < bytes.len() {
            // Slice past the chunk-header wrapper so the returned bytes
            // start at the `pQES` magic. Allocates a fresh Arc - the
            // caller usually parses once and caches the resulting Seq.
            Ok(Some(Arc::new(bytes[offset..].to_vec())))
        } else {
            Ok(None)
        }
    }

    /// First VAB-bearing entry in the scene, ready for parsing as a sound
    /// bank. Mirrors the asset chain's "load the scene's bank before the
    /// first sound plays" pre-pass. Returns `None` when no VAB-tagged
    /// entries are in the scene.
    pub fn scene_vab_bytes(&self) -> Result<Option<Arc<Vec<u8>>>> {
        let Some(assets) = self.assets.as_ref() else {
            return Ok(None);
        };
        let Some(&entry_idx) = assets.vab_entries.first() else {
            return Ok(None);
        };
        let bytes = self.index.entry_bytes(entry_idx)?;
        Ok(Some(bytes))
    }

    /// If the world has a pending dialog request and no panel is currently
    /// running, build an [`crate::dialog::OwnedDialogPanel`] resolved through
    /// the scene's MES container and return it. The caller drives the
    /// panel per-frame; when [`crate::dialog::OwnedDialogPanel::is_done`]
    /// reports true, the caller calls [`SceneHost::clear_dialog`] to
    /// release the field-VM script.
    ///
    /// Returns `None` when no dialog is pending or the scene has no MES
    /// container. The resolved request is left on the world; calling
    /// [`SceneHost::clear_dialog`] cleans it up when the user dismisses
    /// the box.
    pub fn open_pending_dialog(&mut self) -> Option<crate::dialog::OwnedDialogPanel> {
        let req = self.world.current_dialog.as_ref()?;
        // Placement-NPC / event dialogue carries its text inline (the field-VM
        // `0x3F` op's buffer); its `text_id` is a box-config id, not an MES
        // index, so it never resolves through the scene MES. Prefer the inline
        // text when present, falling back to the MES `text_id` lookup (used by
        // the message-table dialogue paths).
        if !req.inline.is_empty()
            && let Some(panel) = crate::dialog::OwnedDialogPanel::from_inline_dialog(&req.inline)
        {
            return Some(panel);
        }
        let mes = self.assets.as_ref()?.mes.as_ref()?;
        crate::dialog::OwnedDialogPanel::from_scene_mes(mes, req.text_id)
    }

    /// Clear the world's pending dialog request. Call after the user
    /// dismisses the box (the field VM resumes the next frame).
    pub fn clear_dialog(&mut self) {
        self.world.current_dialog = None;
    }

    /// Drain the world's pending BGM events through `director`, resolving
    /// each `Bgm{text_id, sub_op}` into the right director hook. Mirrors
    /// the field-VM op `0x35` sub-op table: `1` = start (resolve SEQ
    /// bytes), `2` = pause, `3` = resume, `4` = stop, `9` = queue.
    /// Other sub-ops are passed through as no-ops (the host already
    /// surfaced them on the world's event queue for richer engines to
    /// consume).
    ///
    /// Returns the number of events that the director acted on. Call once
    /// per frame after [`SceneHost::tick`].
    pub fn route_bgm_events(&mut self, director: &mut dyn BgmDirector) -> Result<usize> {
        let mut acted = 0usize;
        let mut leftover = Vec::new();
        for ev in self.world.drain_field_events() {
            match ev {
                crate::field_events::FieldEvent::Bgm { text_id, sub_op } => match sub_op {
                    1 => {
                        if let Some(bytes) = self.bgm_seq_bytes(text_id)? {
                            director.start(text_id, &bytes);
                            acted += 1;
                        }
                    }
                    9 => {
                        if let Some(bytes) = self.bgm_seq_bytes(text_id)? {
                            director.queue(text_id, &bytes);
                            acted += 1;
                        }
                    }
                    2 => {
                        director.pause();
                        acted += 1;
                    }
                    3 => {
                        director.resume();
                        acted += 1;
                    }
                    4 => {
                        director.stop();
                        acted += 1;
                    }
                    _ => {
                        // Other sub-ops (5/6/7/8/10/11) are control words -
                        // surface them back on the queue for richer engines.
                        leftover.push(crate::field_events::FieldEvent::Bgm { text_id, sub_op });
                    }
                },
                other => leftover.push(other),
            }
        }
        // Restore non-BGM (and unhandled-BGM) events so engine layers that
        // also consume them aren't shorted by this routing pass.
        self.world.pending_field_events.extend(leftover);
        Ok(acted)
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

    /// Replace the effect-script catalog used by the effect VM pool.
    ///
    /// Call once after loading PROT 873 (`efect.dat`) and parsing its
    /// pack1 slice via [`legaia_engine_vm::effect_vm::EffectCatalog::from_pack1_bytes`].
    /// An empty catalog is safe - `BattleHostImpl::ui_element` will simply
    /// not spawn any pool entries until a real catalog is wired.
    pub fn set_effect_catalog(&mut self, catalog: legaia_engine_vm::effect_vm::EffectCatalog) {
        self.world.effect_catalog = catalog;
    }

    /// Convenience: hand off a path to the SCUS `extracted/` root, get a
    /// host with no scene loaded yet.
    pub fn from_extracted_root(root: impl Into<PathBuf>) -> Result<Self> {
        Self::open_extracted(root.into())
    }
}

/// PROT entry index for `befect_data` carrying the global TMD-pool head
/// (the 5 character-mesh TMDs at retail `DAT_8007C018[0..4]`). Pinned in
/// `project_global_tmd_pool_source.md` via byte-equality vs a Drake post-warp
/// RAM snapshot.
const PROT_BEFECT_DATA_ENTRY: u32 = 874;

/// PROT entry holding the battle effect-texture atlas (the "flame atlas"):
/// three 64x256 4bpp PSX TIMs blitted to VRAM `(320,0)`, `(384,0)`, `(448,0)`
/// with CLUTs in rows 474..=476 (the effect-CLUT band). Stored uncompressed,
/// back-to-back behind a 16-byte prefix. Despite its CDNAME label
/// (`sound_data`, shared with PROT 871) it carries no audio - the label is one
/// of the documented CDNAME mislabels. Byte-verified pixel-exact in VRAM
/// against every stable Rim Elm battle capture (command-menu / submenu /
/// pre- and post-Seru-capture frames); the partial match in a still-loading
/// frame is just the mid-DMA snapshot. Unlike `etim.dat` (PROT 874 section 2,
/// pages at `fb_y=256`), these pages sit at `fb_y=0` in the same VRAM columns
/// the field uses for town stage textures, so they are *battle-only* uploads -
/// the field captures hold unrelated town texels there. Retail blits them at
/// battle load (not by the `FUN_800520F0` etmd/befect path, which pulls
/// indices `0x367..=0x36d` - PROT 870 = index `0x366` is loaded by a separate
/// site). See `docs/formats/effect.md`.
const PROT_FLAME_ATLAS_ENTRY: u32 = 870;

/// PROT entry holding the runtime effect buffer `data\battle\efect.dat` - the
/// 2-pack wrapper (inline sprite atlas + pack0 anim batches + pack1 effect
/// scripts) the battle effect VM consumes. Stored uncompressed; the raw entry
/// bytes are byte-identical to the post-init runtime buffer (`docs/formats/effect.md`).
const PROT_EFECT_DAT_ENTRY: u32 = 873;

/// PROT entry holding the global monster stat archive (one `0x14000`-byte
/// LZS slot per monster id; the CDNAME label `battle_data` is shared across
/// 0865-0868). The misleading `monster_data` label (PROT 869) is a stub.
/// See [`legaia_asset::monster_archive`] + `docs/subsystems/battle.md`.
const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;

/// Number of slots PROT 0874 section 0 contributes to the head of the
/// global TMD pool. Set by the section's TMD-pack `count` field; the
/// retail pack carries exactly 5 character meshes.
pub(crate) const GLOBAL_TMD_POOL_HEAD_COUNT: usize = 5;

/// Index into [`crate::world::World::global_tmd_pool`] of the PROT 0874 §0
/// *preview* flame mesh - the smallest of that section's five TMDs (2 objects,
/// 18 verts, 25 prims). It bakes the `etim` CLUT (`cba=0x778E@(224,478)`,
/// `tsb=0x001D@(832,256)`) and looks flame-shaped, so the engine could render
/// it through the standard VRAM-mesh pipeline as a stand-in.
///
/// **This is a preview mesh, not the model retail draws.** The real battle
/// flame is [`GIMARD_TAIL_FIRE_MODEL_INDEX`], pulled from the PROT 0871
/// effect-model library (`seed_effect_model_library_from_etmd`). The
/// stand-in is kept only as a fallback when that library isn't loaded (e.g.
/// raw-PROT.DAT inspection without the battle assets). See
/// `docs/formats/effect.md`.
pub const ETMD_TAIL_FIRE_MODEL_INDEX: usize = 4;

/// PROT entry holding the battle effect-model library (`etmd.dat`): a 30-entry
/// `asset::pack` of Legaia TMDs (`word[0]=30`, every entry magic `0x80000002`),
/// stored uncompressed. Retail registers all 30 verbatim into
/// `DAT_8007C018[3..=32]` at battle init (`FUN_800520F0` debug index `0x367` ->
/// `FUN_80026B4C`); the dev-path name is `h:\prot\battle\etmd.dat`. The CDNAME
/// label `sound_data` is misleading - this is the effect-model library, not
/// audio. See `docs/formats/effect.md`.
const PROT_EFFECT_MODEL_LIBRARY_ENTRY: u32 = 871;

/// Base index in [`crate::world::World::global_tmd_pool`] (= `DAT_8007C018`)
/// where the PROT 0871 effect-model library registers. Its 30 models occupy
/// `[3..=32]`, overwriting the two trailing slots of the PROT 0874 §0 field
/// head (`[3]`, `[4]`) - exactly retail's temporal layout (the field head
/// seeds `[0..=4]`; battle init reloads `[3..=32]`).
///
/// This is the engine's analogue of the retail **battle `gp[0x754]` value** -
/// the additive base `FUN_80021B04` applies to a move-FX / summon part record's
/// `model_sel` (`DAT_8007C018[model_sel + gp[0x754]]`). In retail that base is
/// *not* a constant: it is `party_count + 2` (the two fixed pool slots + the live
/// party-character meshes precede the library), i.e. `3` for the 1-member
/// training party and `5` for the full 3-member party - save-corpus-pinned by
/// `crates/mednafen/tests/summon_model_base.rs` (see `docs/formats/move-power.md`).
/// The engine instead registers the library at a *fixed* `[3..=32]` and keeps
/// `model_sel` library-relative, so `model_sel + 3` lands on the same library
/// model retail reaches via `model_sel + gp[0x754]` - the library content is
/// identical, only its pool offset shifts with party size, so the two layouts are
/// equivalent. `World::spawn_move_fx` uses this fixed base.
pub(crate) const EFFECT_MODEL_LIBRARY_BASE: usize = 3;

/// Number of TMDs in the PROT 0871 effect-model library (`word[0]`).
pub(crate) const EFFECT_MODEL_LIBRARY_COUNT: usize = 30;

/// Index in [`crate::world::World::global_tmd_pool`] of Gimard's *Tail Fire*
/// flame model (`DAT_8007C018[26]`) - the model retail draws for the Gimard
/// Seru cast. Equals `EFFECT_MODEL_LIBRARY_BASE`` + 23` (pack entry 23). Its
/// fire flicker is CLUT/palette cycling driven by the summon stager overlay (extraction PROT 0903)
/// (the model geometry is static). Supersedes the PROT 0874 §0 preview
/// stand-in at [`ETMD_TAIL_FIRE_MODEL_INDEX`]. See `docs/formats/effect.md`.
pub const GIMARD_TAIL_FIRE_MODEL_INDEX: usize = 26;

/// Seed `World::global_tmd_pool[0..=4]` from PROT 0874 (`befect_data`)
/// section 0. Soft-fails (returns `Err`) when the entry is missing, the
/// section header is malformed, the LZS decode fails, or the inner
/// TMD-pack walk fails - the field-VM `0x4C 0xD8` host hook then leaves
/// `Actor::tmd_ref` at `None` rather than aborting scene-load.
///
/// The retail loader chain that produces these 5 entries via
/// `FUN_8001F05C case 2 → FUN_80026B4C` is not yet pinned (see open work
/// item in `docs/formats/world-map-overlay.md`); this routes the disc
/// bytes directly through the `parse_player_lzs + pack` parsers and
/// installs the parsed TMDs onto the world.
/// Load the effect-script catalog from PROT 0873 (`efect.dat`) into
/// `World::effect_catalog`. Soft-fails when the entry is missing or the
/// 2-pack is malformed (the catalog stays empty and nothing spawns). Parsing
/// itself never errors - [`EffectCatalog::from_efect_dat_bytes`] returns an
/// empty catalog on bad data - so the only error is the disc read.
fn seed_effect_catalog_from_efect_dat(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_EFECT_DAT_ENTRY)
        .with_context(|| format!("read PROT entry {} (efect.dat)", PROT_EFECT_DAT_ENTRY))?;
    let catalog = legaia_engine_vm::effect_vm::EffectCatalog::from_efect_dat_bytes(&raw);
    if catalog.is_empty() {
        anyhow::bail!("efect.dat parsed to an empty catalog (unexpected 2-pack shape)");
    }
    world.effect_catalog = catalog;
    Ok(())
}

pub(crate) fn seed_global_tmd_pool_from_befect_data(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_BEFECT_DATA_ENTRY)
        .with_context(|| format!("read PROT entry {} (befect_data)", PROT_BEFECT_DATA_ENTRY))?;
    let container = legaia_asset::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section0 = container
        .descriptors
        .first()
        .ok_or_else(|| anyhow::anyhow!("befect_data has no section 0"))?;
    let decoded = legaia_asset::decode(&raw, section0, legaia_asset::DecodeMode::Lzs)
        .context("LZS-decode befect_data section 0")?;
    let pack_entries = legaia_asset::pack::extract_pack(&decoded)
        .context("walk befect_data section 0 as a TMD-pack")?;
    let head = pack_entries
        .into_iter()
        .take(GLOBAL_TMD_POOL_HEAD_COUNT)
        .enumerate();
    for (i, body) in head {
        let tmd = match legaia_tmd::parse(body) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("[scene] befect_data slot {i} did not parse as TMD ({err:#}); skipping");
                continue;
            }
        };
        world.set_global_tmd(
            i,
            std::sync::Arc::new(crate::world::GlobalTmd {
                tmd,
                raw: body.to_vec(),
            }),
        );
    }
    Ok(())
}

/// Seed the battle effect-model library from PROT 0871 (`etmd.dat`) into
/// `World::global_tmd_pool[3..=32]` (retail `DAT_8007C018[3..=32]`).
///
/// PROT 0871 is an uncompressed 30-entry [`legaia_asset::pack`] of Legaia
/// TMDs; the engine walks it directly (no LZS) and parses each entry, mapping
/// pack entry `i` -> pool index [`EFFECT_MODEL_LIBRARY_BASE`]` + i`. This is
/// the library retail loads at battle init (`FUN_800520F0`); the live
/// Tail-Fire RAM confirms these 30 models are resident during a Seru cast
/// while PROT 0874 §0's five TMDs are not - so this supersedes the §0 preview
/// head for the effect-model render path ([`GIMARD_TAIL_FIRE_MODEL_INDEX`] is
/// the flame retail draws).
///
/// Soft-fails (returns `Err`) when the entry is missing or the pack walk
/// fails; entries that don't parse as TMDs are skipped individually. The two
/// overlapping slots (`[3]`, `[4]`) from the PROT 0874 §0 head are overwritten
/// here, matching retail's temporal load order.
pub(crate) fn seed_effect_model_library_from_etmd(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    // The pack body spans PROT 0871's full on-disc footprint (the last TMD
    // sits past the TOC-indexed end), so read the extended footprint - the
    // indexed-only view truncates the pack mid-table.
    let raw = index
        .entry_bytes_extended(PROT_EFFECT_MODEL_LIBRARY_ENTRY)
        .with_context(|| {
            format!(
                "read PROT entry {} (etmd.dat effect-model library)",
                PROT_EFFECT_MODEL_LIBRARY_ENTRY
            )
        })?;
    let pack_entries = legaia_asset::pack::extract_pack(&raw)
        .context("walk PROT 0871 (etmd.dat) as a TMD pack")?;
    let mut loaded = 0usize;
    for (i, body) in pack_entries
        .iter()
        .enumerate()
        .take(EFFECT_MODEL_LIBRARY_COUNT)
    {
        let tmd = match legaia_tmd::parse(body) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("[scene] etmd library slot {i} did not parse as TMD ({err:#}); skipping");
                continue;
            }
        };
        world.set_global_tmd(
            EFFECT_MODEL_LIBRARY_BASE + i,
            std::sync::Arc::new(crate::world::GlobalTmd {
                tmd,
                raw: body.to_vec(),
            }),
        );
        loaded += 1;
    }
    if loaded == 0 {
        anyhow::bail!("etmd library (PROT 0871) carried no parseable TMDs");
    }
    Ok(())
}

/// True when the PROT 0871 effect-model library is already resident in the
/// pool (every slot in `[3..=32]` populated). Used to keep
/// [`seed_effect_model_library_from_etmd`] idempotent across scene
/// transitions, mirroring the field-head guard.
pub(crate) fn effect_model_library_loaded(world: &crate::world::World) -> bool {
    let end = EFFECT_MODEL_LIBRARY_BASE + EFFECT_MODEL_LIBRARY_COUNT;
    world.global_tmd_pool.len() >= end
        && world.global_tmd_pool[EFFECT_MODEL_LIBRARY_BASE..end]
            .iter()
            .all(|s| s.is_some())
}

/// PROT 0874 (`befect_data`) section index carrying `etim.dat` - the battle
/// effect-sprite TIMs. The three LZS sections are: 0 = effect 3D models
/// (`etmd.dat`, the global-TMD-pool head), 1 = `vdf.dat`, 2 = `etim.dat`.
const BEFECT_ETIM_SECTION: usize = 2;

/// Upload the player `player.lzs` texture section (PROT 0874 section 2) into
/// `vram`. This 8-TIM pack carries **both** the 3D effect-model textures
/// (`etim.dat`, the texel source for `etmd.dat` / section 0's global-TMD-pool
/// head) **and the field-character texture atlas**: entries 1/2/3 are the
/// Vahn/Noa/Gala atlas pages at texpage `(832, 256)` with per-character CLUTs
/// on row 478 (the field-form player meshes sample exactly these). Retail
/// uploads the whole section at field-init via `FUN_8001E890 → FUN_800198E0`
/// (`LoadImage`) and keeps it resident across the battle scene-mode overlay, so
/// uploading at scene entry is equivalent. See
/// [`docs/formats/character-mesh.md` § Textures (field form)] for the full
/// entry table.
///
/// CLUT blocks are uploaded as **flat horizontal strips** (`FUN_800198e0`:
/// `LoadImage(rect = { x, y, w*h, 1 })`), not as the declared `w x h`
/// rectangle - see the inline note. (`legaia_asset::field_char_textures` is the
/// standalone parser + verifier for the same section, byte-exact against a live
/// field VRAM dump.)
///
/// This makes the texels resident for effect-model rendering. (It does *not*
/// feed the 2D `efect.dat` sprite-atlas billboards, which sample a separate
/// page-`(0,0)` 8bpp source - see [`crate::world::World::active_effect_sprites`]
/// and the open atlas-source thread in `docs/formats/effect.md`.)
///
/// Mirrors `seed_global_tmd_pool_from_befect_data`'s LZS path. Soft-fails;
/// returns the number of TIMs uploaded.
///
/// Public so the VRAM-parity oracle's lightweight pre-pass can apply the same
/// effect-texture upload the live field-entry path performs - without it the
/// oracle reports the `fb_y=256` effect pages (fb_x 320/384/832/852/872/880)
/// as a phantom static gap that the real engine never has.
///
/// `upload_clut` controls whether the TIMs' CLUT rows (473..=478) are written
/// alongside the image pages. Retail keeps the effect-texture *pixel* pages
/// (fb_y=256) resident from field through battle, but uploads their CLUTs at
/// battle entry - so a field-VRAM parity build wants `upload_clut = false`
/// (image pages only) while the live field-entry seed passes `true` to keep
/// the CLUTs resident through the battle scene-mode overlay.
pub fn upload_effect_textures_into_vram(
    index: &ProtIndex,
    vram: &mut legaia_tim::Vram,
    upload_clut: bool,
) -> Result<usize> {
    let decoded = befect_etim_section_bytes(index)?;
    let mut uploaded = 0;
    for target in legaia_asset::befect_cluster::scan_tims(&decoded) {
        match legaia_tim::parse(&decoded[target.offset..]) {
            Ok(tim) => {
                // Image page: declared rect, verbatim.
                vram.upload_tim_partial(&tim, true, false);
                // CLUT: `FUN_800198e0` uploads the CLUT block as a FLAT
                // horizontal strip - `LoadImage(rect = { x, y, w*h, 1 })` -
                // not the declared `w x h` rectangle. This matters for §2's
                // field-character TIMs (entries 1/2/3, CLUT `w=16 h=4`): a
                // rect upload puts Vahn's four 16-colour palettes at rows
                // 478..481 col 0, but the meshes sample them as columns
                // 0/16/32/48 of row 478. The strip places them correctly.
                // (Field upload runs with STP off, `_DAT_8007b998 == 0`.)
                if upload_clut && let Some(clut) = tim.clut.as_ref() {
                    let strip: Vec<u8> =
                        clut.entries.iter().flat_map(|c| c.to_le_bytes()).collect();
                    vram.write_clut_row(clut.fb_x, clut.fb_y, &strip);
                }
                uploaded += 1;
            }
            Err(err) => {
                eprintln!(
                    "[scene] etim TIM @0x{:x} did not parse ({err:#}); skipping",
                    target.offset
                );
            }
        }
    }
    if uploaded == 0 {
        anyhow::bail!("etim section carried no uploadable TIMs");
    }
    Ok(uploaded)
}

/// Decoded `befect_data` (PROT 874) etim-section bytes - the shared
/// effect-texture TIM pool [`upload_effect_textures_into_vram`] and
/// [`effect_texture_image_rects`] both walk.
fn befect_etim_section_bytes(index: &ProtIndex) -> Result<Vec<u8>> {
    let raw = index
        .entry_bytes(PROT_BEFECT_DATA_ENTRY)
        .with_context(|| format!("read PROT entry {} (befect_data)", PROT_BEFECT_DATA_ENTRY))?;
    let container = legaia_asset::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section = container
        .descriptors
        .get(BEFECT_ETIM_SECTION)
        .ok_or_else(|| {
            anyhow::anyhow!("befect_data has no section {BEFECT_ETIM_SECTION} (etim)")
        })?;
    legaia_asset::decode(&raw, section, legaia_asset::DecodeMode::Lzs)
        .with_context(|| format!("LZS-decode befect_data section {BEFECT_ETIM_SECTION} (etim)"))
}

/// VRAM image rects `(fb_x, fb_y, width_in_words, height)` of the
/// `befect_data` effect-texture TIMs - the upload set of
/// [`upload_effect_textures_into_vram`].
///
/// The band is **global shared state**, not per-scene texture: one disc
/// source is resident across every field scene. A handful of its pixels are
/// *history-dependent* - the pause-menu entry path writes an F-variant of
/// three row-271 words (pinned at `(853, 271)`: pause-menu-lineage captures
/// hold `0xFFFF` where the disc TIM carries `0x3333`; each variant word
/// equals the same TIM's row-273 value), and the first battle effect use
/// restores the disc bytes. A per-scene static mask misclassifies those
/// pixels as static whenever a scene's captures share menu/battle history,
/// so the VRAM parity oracle uses these rects to demand staticity across
/// **all** scenes' captures instead.
pub fn effect_texture_image_rects(index: &ProtIndex) -> Result<Vec<(u16, u16, u16, u16)>> {
    let decoded = befect_etim_section_bytes(index)?;
    let mut rects = Vec::new();
    for target in legaia_asset::befect_cluster::scan_tims(&decoded) {
        if let Ok(tim) = legaia_tim::parse(&decoded[target.offset..]) {
            let img = &tim.image;
            rects.push((img.fb_x, img.fb_y, img.fb_w, img.h));
        }
    }
    Ok(rects)
}

/// VRAM image rects `(fb_x, fb_y, width_in_words, height)` of every TIM a
/// CDNAME block carries, via the same scanner the scene VRAM build uses.
///
/// Sibling of [`effect_texture_image_rects`] for the **shared-block** upload
/// set ([`crate::scene_resources::FIELD_SHARED_BLOCKS`]): `init_data`'s UI
/// tile pages at `fb=(704, 0)` / `fb=(704, 256)` are *journey-dependent*
/// residency, not per-scene texture - an overworld transit leaves kingdom-
/// bundle content over parts of the rect, so a field scene reached through
/// the world map holds kingdom bytes there while a boot-fresh scene holds
/// the disc tiles. A per-scene static mask misclassifies those words as
/// static whenever a scene's captures share route history; the VRAM parity
/// oracle pools captures across all scenes against these rects instead.
pub fn block_image_rects(index: &ProtIndex, block: &str) -> Result<Vec<(u16, u16, u16, u16)>> {
    let scene = Scene::load(index, block)?;
    let mut rects = Vec::new();
    for entry in &scene.entries {
        let scan = legaia_asset::tim_scan::scan_entry(&entry.bytes);
        for (source, hit) in &scan.hits {
            let payload: &[u8] = match source {
                legaia_asset::tim_scan::Source::Raw => &entry.bytes,
                legaia_asset::tim_scan::Source::Lzs(idx) => scan.lzs_sections[*idx].as_slice(),
            };
            if let Some(slice) = payload.get(hit.offset..)
                && let Ok(tim) = legaia_tim::parse(slice)
            {
                let img = &tim.image;
                rects.push((img.fb_x, img.fb_y, img.fb_w, img.h));
            }
        }
    }
    Ok(rects)
}

/// Upload the battle effect-texture atlas (PROT 870, the "flame atlas") into
/// `vram`. These three 64x256 4bpp TIMs (pages at `(320,0)`, `(384,0)`,
/// `(448,0)`, CLUTs in rows 474..=476) are the texel source for the
/// fire/flame effect meshes during battle, byte-verified against live battle
/// VRAM (see `PROT_FLAME_ATLAS_ENTRY`).
///
/// Call this on **battle entry**, not field entry: the pages land in the same
/// VRAM columns (`fb_x` 320..512, `fb_y` 0) the field stage textures occupy,
/// so uploading them while a field scene is resident would clobber town
/// rendering. Retail overwrites that region for battle and the field reloads
/// its textures on return - the play-window battle path mirrors this by
/// blitting into a throwaway VRAM copy that battle exit discards.
///
/// `upload_clut` writes the CLUT rows (474..=476) alongside the image pages.
/// Mirrors [`upload_effect_textures_into_vram`]; PROT 870 is uncompressed, so
/// the TIMs are walked straight out of the entry bytes (read via the extended
/// footprint, like the PROT 871 effect-model library - the indexed size can
/// truncate the trailing TIM). Soft-fails; returns the number of TIMs uploaded.
pub fn upload_flame_atlas_into_vram(
    index: &ProtIndex,
    vram: &mut legaia_tim::Vram,
    upload_clut: bool,
) -> Result<usize> {
    let raw = index
        .entry_bytes_extended(PROT_FLAME_ATLAS_ENTRY)
        .with_context(|| format!("read PROT entry {PROT_FLAME_ATLAS_ENTRY} (flame atlas)"))?;
    let mut uploaded = 0;
    for target in legaia_asset::befect_cluster::scan_tims(&raw) {
        match legaia_tim::parse(&raw[target.offset..]) {
            Ok(tim) => {
                vram.upload_tim_partial(&tim, true, upload_clut);
                uploaded += 1;
            }
            Err(err) => {
                eprintln!(
                    "[scene] flame-atlas TIM @0x{:x} did not parse ({err:#}); skipping",
                    target.offset
                );
            }
        }
    }
    if uploaded == 0 {
        anyhow::bail!("flame atlas (PROT {PROT_FLAME_ATLAS_ENTRY}) carried no uploadable TIMs");
    }
    Ok(uploaded)
}
