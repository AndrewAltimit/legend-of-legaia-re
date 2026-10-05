//! `SceneHost` construction, PROT/disc/extracted openers, scene loading, and simple accessors.
//!
//! Extracted verbatim from `scene/host.rs` as an additional `impl SceneHost` block.

use super::*;

impl SceneHost {
    /// Where a save written now would resume: the loaded scene's CDNAME
    /// label and its banner name (the scene MAN's section 2, the string
    /// retail's `FUN_8003AEB0` installs as the entry banner and copies into
    /// the save-state location field - `docs/formats/place-names.md`). Both
    /// empty when no scene is loaded; the location alone is empty for a
    /// scene whose MAN carries no printable banner (the Shift-JIS endings).
    ///
    /// The MAN is re-read from the PROT index rather than cached: it runs
    /// once per save. The native session and the page's card / LGSF writers
    /// each carried a copy of this derivation; this is the one both call.
    pub fn current_resume(&self) -> legaia_save::SaveResume {
        let Some(scene) = self.scene.as_ref() else {
            return legaia_save::SaveResume::default();
        };
        let location = scene
            .field_man_payload(&self.index)
            .ok()
            .flatten()
            .and_then(|man| legaia_asset::place_names::scene_name(&man))
            .map(|n| n.name)
            .unwrap_or_default();
        legaia_save::SaveResume {
            scene: scene.name.clone(),
            location,
        }
    }

    /// Build a host over an already-opened ProtIndex.
    pub fn new(index: Arc<ProtIndex>) -> Self {
        let mut world = crate::world::World::default();
        // The menu-warp drain resolves a quick-travel `scene_id` (a raw
        // CDNAME TOC index) against this map; installing it here means every
        // host that builds a SceneHost gets the Door of Wind warp for free.
        if let Some(map) = index.cdname_map() {
            world.install_scene_toc_names(map.clone());
        }
        Self {
            index,
            world,
            scene: None,
            assets: None,
            resources: None,
            model_bank: crate::model_bank::SceneModelBank::default(),
            frame_time: crate::FrameTime::new(),
            map_resolver: Box::new(NullMapIdResolver),
            monster_archive_cache: None,
            bse_bank_cache: None,
            move_power_loaded: false,
            battle_tutorial_loaded: false,
            cast_effect_pool_loaded: false,
            battle_party_forms: None,
            last_minigame_warp: None,
            pending_entry_seat: None,
            scene_destinations: Vec::new(),
            field_triggers: (Vec::new(), Vec::new()),
            field_intra_teleports: (Vec::new(), Vec::new()),
            field_man_cache: None,
            scene_gold_charges: Vec::new(),
            last_trigger_tile: None,
            sustained_sfx: SustainedSfx::new(),
            mode_cell: 0,
            new_game_defaults: None,
            // DAT_8007B6EC boot value - FUN_8001FFA4 stores -1.
            bgm_volume_raw: crate::new_game::GAME_STATE_COLD_RESET.bgm_volume_raw,
            bgm_track_word: None,
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
        // Release any sustained-SFX voices the outgoing scene still holds -
        // the retail teardown runs from the mode initializer on mode entry
        // (and from the battle anim commit on anim transitions, which
        // engines drive via [`SceneHost::release_sustained_sfx`] directly).
        // REF: FUN_80017910, FUN_8001DCF8, FUN_8004AD80
        self.release_sustained_sfx();
        // Scene-to-scene teardown sweep over the actor pool. Retail's field
        // initialiser `FUN_801D6704` runs `FUN_801D7518` once per actor list
        // on a **warp** entry (`_DAT_8007B8B8 == 2`) and not on a cold one -
        // and retail's warp entry is a return from battle / a minigame / an
        // FMV, NOT a door change (`docs/subsystems/field-locomotion.md`).
        // "a scene is already loaded" is therefore a wider condition than
        // retail's: it also covers the door changes retail takes the
        // fresh-actor arm on. The engine has no surviving-actor pool across a
        // scene load, so the sweep is the engine's teardown either way.
        // PORT: FUN_801D7518 (live wiring; kernel = `field_actor_kernels::sweep_actor`)
        if self.scene.is_some() {
            self.world.scene_transition_actor_sweep();
        }
        // Re-allocate + reset the scene control block `_DAT_801C6EA4` before
        // the new scene's records are installed. Retail's reset also clears
        // the tile-descriptor pointer `_DAT_8007B450`, which is what keeps a
        // tile board from surviving into the next scene.
        // PORT: FUN_8003A024 (live wiring; the reset image itself is
        // `crate::scus_leaf_kernels::SCENE_CONTROL_BLOCK_RESET`)
        self.world.reset_scene_control_block();
        // A scene load is a mode switch in retail, and the mode initialiser's
        // per-stage init resets the whole actor pool (`FUN_8001E1B4`,
        // `0x8001E324..0x8001E364`): no battle effect - nor a dev spawn left
        // in the move-FX / summon seats - rides into the next scene.
        // REF: FUN_8001E1B4
        self.world.teardown_battle_effects();
        // The narration roller is a pool actor (`FUN_80037174`) and goes with
        // the rest: a record's terminal SceneChange does not wait for it, so
        // `opdeene`'s Seru-history crawl is still up when the scene ends.
        self.world.cutscene.narration = None;
        let scene = Scene::load(&self.index, name)?;
        // `_DAT_80084540`: the scene's raw CDNAME define (the `block_range`
        // start, before the extraction-frame shift `Scene::start` carries).
        self.world.battle.map_id = self.index.block_range(name).map_or(0, |(raw, _)| raw);
        let assets = crate::scene_assets::SceneAssets::build(&scene);
        // The scene ANM bundle's per-record end-latch lengths: how long any
        // scene-bank clip a cutscene record pokes onto the player plays, at
        // the record's own step (a gated record's scaled step included).
        let scene_anm = crate::npc_catalog::scene_anm_bundle(&scene);
        self.world.locomotion.scene_clip_meta = scene_anm
            .as_ref()
            .map(|b| {
                (0..b.record_count as usize)
                    .map(|i| {
                        b.record(i).map_or((0, false, 0), |r| {
                            (r.frame_count, r.blends(), (r.flag & 0xFF) as u8)
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.world.locomotion.scene_clip_ticks = self
            .world
            .locomotion
            .scene_clip_meta
            .iter()
            .map(|&(frames, gated, div)| {
                let step = crate::field_anim::clip_step(crate::field_anim::CLIP_RATE, gated, div);
                crate::field_anim::clip_end_ticks(frames, step)
            })
            .collect();
        self.scene = Some(scene);
        self.assets = Some(assets);
        self.refresh_scene_destinations();
        // Cache the `.MAP` kind-1 tile-trigger tables + the MAN payload for
        // the per-frame walk-on dispatch, and mark the last-tile compare
        // stale (retail's scene-init state - the first tick fires the
        // trigger at the spawn/arrival tile).
        self.field_triggers = self
            .scene
            .as_ref()
            .and_then(|s| s.field_tile_triggers(&self.index).ok())
            .unwrap_or_default();
        self.field_intra_teleports = self
            .scene
            .as_ref()
            .and_then(|s| s.field_intra_scene_teleports(&self.index).ok())
            .unwrap_or_default();
        self.field_man_cache = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten())
            .map(Arc::new);
        // Per-scene save permission (`_DAT_8007B6A8`). Retail's MAN loader
        // seeds it from the header's `[0x01] & 1` as it walks the buffer;
        // seeding here covers both entry paths (field + world map) off the
        // one cached payload, and a scene with no MAN clears it.
        // PORT: FUN_8003AEB0 (live wiring; kernel =
        //       `World::install_scene_save_permission`)
        let man_header = self
            .field_man_cache
            .as_ref()
            .and_then(|man| legaia_asset::man_section::parse(man).ok());
        self.world
            .install_scene_save_permission(man_header.as_ref());
        // The actor-list work retail's MAN loader `FUN_8003AEB0` does around
        // its own decode: two inlined `FUN_8003CF40` retire sweeps, the
        // submode open `FUN_801D9C3C()`, and the fixed-template scene-actor
        // spawn `FUN_801DE478(0xF)`. Runs whether or not this scene carries a
        // MAN, matching retail - `FUN_8003AEB0` reaches all four before any
        // payload-dependent branch.
        // PORT: FUN_801D9C3C, FUN_801DE478 (live wiring; kernels in
        //       `field_submode`)
        self.world.man_load_actor_reset();
        // The loader's last act: the place-name banner - a `4C E1` text
        // balloon carrying the MAN's section-2 name, seated when system flag
        // 2 is armed, and the flag cleared either way.
        // PORT: FUN_8003AEB0 (live wiring; kernel =
        //       `crate::place_name_banner::man_load_banner`)
        let banner_name = self
            .field_man_cache
            .as_ref()
            .map(|man| crate::place_name_banner::man_scene_name_bytes(man))
            .unwrap_or_default();
        self.world.man_load_place_name_banner(&banner_name);
        // Scan the cached MAN for scripted gold charges (inn gate + debit
        // pairs) so the inn UI can open with this scene's real cost.
        self.scene_gold_charges = self
            .field_man_cache
            .as_ref()
            .map(|man| legaia_asset::inn_costs::scan(man))
            .unwrap_or_default();
        // The scene's CD-XA one-shots, for a host that stages clips
        // asynchronously to have them resident before the op fires - plus the
        // announcer lines a minigame door in this scene opens onto.
        self.world.audio.field_xa_prestage = Vec::new();
        if let Some(man) = self.field_man_cache.as_ref() {
            let own = crate::world::field_xa::scene_xa_prestage(man);
            let door = crate::world::field_xa::scene_minigame_door_xa_prestage(man);
            self.world.queue_xa_prestage(own);
            self.world.queue_xa_prestage(door);
        }
        self.last_trigger_tile = None;
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

    /// The current scene's disc-sourced **scripted gold charges**: every
    /// op-`0x4E` gold-gate + negative `0x3A` debit pair in its field-VM
    /// script (inn stays, paid tours, casino gold-to-coin counters), in
    /// script order. Scanned from the MAN at [`Self::load_scene`] via
    /// [`legaia_asset::inn_costs::scan`]. Empty when no scene is loaded or
    /// the scene charges nothing.
    pub fn scene_gold_charges(&self) -> &[legaia_asset::inn_costs::GoldCharge] {
        &self.scene_gold_charges
    }

    /// The current scene's **inn cost** in gold: the first sub-op-3 (u16
    /// literal) gold charge in its script - the inn / paid-lodging class of
    /// site (sub-op-10 u32 sites are the casino counters). `None` when the
    /// scene has no scripted charge: free rests (Rim Elm's bed, Biron) have
    /// no gate + debit pair at all. Feed this to
    /// [`crate::menu_runtime::MenuRuntime::open_inn`] (or call
    /// [`crate::menu_runtime::MenuRuntime::open_scene_inn`], which resolves
    /// it for you) when the scene's innkeeper dialogue hands off to the
    /// inn prompt.
    pub fn scene_inn_cost(&self) -> Option<u32> {
        self.scene_gold_charges
            .iter()
            .find(|c| c.sub_op == 3)
            .map(|c| c.cost)
    }

    /// Seat the player on raw world `(x, z)` ([`crate::world::World::debug_seat_player`])
    /// as a player **standing** there, not one arriving: the walk-on
    /// dispatcher's last-tile pair is stamped with the seat tile, so a trigger
    /// under the seat does not fire on the first tick. Walk-on records fire
    /// on a tile *crossing* (`FUN_801D1EC4` compares the player's tile with
    /// the stored pair and returns on a match), and a retail capture of a
    /// player stood on a trigger tile holds that tile in the pair already -
    /// the crossing that fired it, if any, is behind it. A debug affordance
    /// shared by every seat host (`LEGAIA_SEAT`, the play page's
    /// `play_debug_seat`, the retail comparison corpus).
    ///
    /// REF: FUN_801D1EC4
    pub fn debug_seat_standing(&mut self, x: i16, z: i16) -> bool {
        if !self.world.debug_seat_player(x, z) {
            return false;
        }
        let quant = |w: i16| i32::from(w) >> 7;
        let (tx, tz) = (quant(x), quant(z));
        if (0..=0x7F).contains(&tx) && (0..=0x7F).contains(&tz) {
            self.last_trigger_tile = Some((tx as u8, tz as u8));
        }
        // The overworld's portals are entity auto-engages, not walk-on
        // records, so the stamp above does not reach them: hold the seat tile
        // for them the same way (a walked crossing caught between its portal
        // firing and the next scene loading stands on that portal).
        if self.world.mode == crate::world::SceneMode::WorldMap {
            self.world.world_map.seat_hold_tile = Some((tx, tz));
        }
        true
    }

    /// `true` when the world position `(world_x, world_z)` falls on a tile that
    /// carries a **gate-1 walk-on trigger** - the per-tile compare the field loop
    /// fires on a tile crossing (a town exit, a scripted story beat). Read-only
    /// view of the `.MAP` kind-1 trigger tables cached at scene load.
    ///
    /// A host that seats the player somewhere other than a door arrival (the
    /// browser play page's scene picker does exactly that) needs this: dropping
    /// them onto a trigger tile fires it on the first tick, which reads to the
    /// player as "the scene immediately warped somewhere else".
    pub fn tile_has_walk_on_trigger(&self, world_x: i16, world_z: i16) -> bool {
        // Retail tile quantisation, matching the walk-on dispatch.
        let quant = |w: i16| -> i32 { (i32::from(w) - 0x40) >> 7 };
        let (tx, tz) = (quant(world_x), quant(world_z));
        if !(0..=0x7F).contains(&tx) || !(0..=0x7F).contains(&tz) {
            return false;
        }
        let (primary, fallback) = &self.field_triggers;
        crate::field_regions::lookup_tile_trigger(primary, fallback, tx as u8, tz as u8)
            .is_some_and(|t| t.gate == 1)
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
