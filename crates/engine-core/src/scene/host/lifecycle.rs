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
