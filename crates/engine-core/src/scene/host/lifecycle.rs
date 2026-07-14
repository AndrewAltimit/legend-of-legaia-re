//! `SceneHost` construction, PROT/disc/extracted openers, scene loading, and simple accessors.
//!
//! Extracted verbatim from `scene/host.rs` as an additional `impl SceneHost` block.

use super::*;

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
            field_triggers: (Vec::new(), Vec::new()),
            field_man_cache: None,
            scene_gold_charges: Vec::new(),
            last_trigger_tile: None,
            sustained_sfx: SustainedSfx::new(),
            mode_cell: 0,
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
        let scene = Scene::load(&self.index, name)?;
        let assets = crate::scene_assets::SceneAssets::build(&scene);
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
        self.field_man_cache = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten())
            .map(Arc::new);
        // Scan the cached MAN for scripted gold charges (inn gate + debit
        // pairs) so the inn UI can open with this scene's real cost.
        self.scene_gold_charges = self
            .field_man_cache
            .as_ref()
            .map(|man| legaia_asset::inn_costs::scan(man))
            .unwrap_or_default();
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
