//! Expansion-pack maps prepared ahead of their entry.
//!
//! Entering a pack map ([`crate::scene_host::SceneHost::enter_expansion_map`])
//! does two kinds of work. Most of it is a pure function of the disc and the
//! pack: the mesh library (the base scene's resources, meshes carried in
//! from other scenes, a terrain's borrowed cells and blend palettes), the
//! synthetic field image (collision grid, terrain tile meshes, baked
//! decoration cells), the NPC and scenery placements, and the scene's
//! resource build over the result. The rest seats that product on the
//! running `World`. [`PreparedMap`] is the first half, built by
//! [`PrepareJob`] off nothing but a [`ProtIndex`] and the pack, so it can be
//! made before the player reaches the exit that needs it.
//!
//! [`MapPreloader`] keeps the prepared maps: a small least-recently-used
//! cache, filled by the entries themselves and - when the host turns
//! preloading on - by a background preparation of every map the entered
//! map's exits lead to. On the native host that is a worker thread; the
//! browser build has none, so the play page runs the job a phase per idle
//! frame ([`MapPreloader::step`]). Nothing here reads or writes `World`, and
//! the product is the one the entry would have built itself, so a map
//! entered from the cache plays bit-identically to one built cold.

use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_scene::expansion_map::detail::{
    MeshLibrary, needs_library, resolve_materials, resolve_mesh_library,
};
use legaia_engine_scene::expansion_map::{
    BaseInputs, SceneOverride, base_scenery, build_field_image, fill_borrowed_npc_clips, floor_lut,
    mesh_rest_shape, prop_records_at, prop_scales_at, resolve_map_npcs,
};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_resources::SceneResources;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::map::{ExitTarget, MapDef, TileGrid};

/// How many prepared maps the cache holds: the map the player is on and the
/// maps its exits lead to, with room for the one just left.
pub const PREPARED_MAP_CAPACITY: usize = 5;

/// The cache's footprint budget ([`PreparedMap::approx_bytes`] summed): past
/// it the least recently used maps go, the newest always staying. A
/// terrain-heavy island map prepares to tens of MiB; a tower floor to one
/// or two.
pub const PREPARED_MAP_BUDGET: usize = 96 * 1024 * 1024;

/// Everything a pack map's entry builds before it touches the running world.
pub struct PreparedMap {
    /// The map's id (`maps/<id>.toml`).
    pub map_id: String,
    /// The retail scene the map is entered over.
    pub base_name: String,
    /// The map's tiles resolved.
    pub grid: Arc<TileGrid>,
    /// The map as the runtime animates it: materials naming another scene's
    /// VRAM pointed at the library's copies, borrowed NPCs' clips filled in.
    pub live_def: Arc<MapDef>,
    /// The field layer the base scene's load seats.
    pub overrides: Arc<SceneOverride>,
    /// How many of `overrides.npcs` are the base scene's own scenery (after
    /// the map's NPCs).
    pub n_scenery: usize,
    /// What an in-place prop move re-derives the collision grid from
    /// (`SceneHost::expansion_collision`).
    pub collision: Arc<legaia_engine_scene::expansion_map::CollisionInputs>,
    /// The emitters' sprite sheet, when the map has emitters.
    pub particle_sheet: Option<legaia_expansion_format::map_detail::ParticleSheet>,
    /// The shoreline foam's palettes, when the map's water has foam.
    pub foam_cluts: Option<[u16; legaia_expansion_format::terrain::FOAM_BANDS]>,
    /// What the preparation found worth saying (logged at the entry, so a
    /// map prepared on a worker says it when the player arrives).
    pub notes: Vec<(log::Level, String)>,
    /// The base scene's field resource build with the override seated, before
    /// the effect-texture underlay ([`crate::scene_host::field_scene_build`]).
    /// `None` when that build failed: the entry then runs it itself.
    pub build: Option<Arc<SceneResources>>,
}

impl PreparedMap {
    /// Build every phase of `map_id` at once.
    pub fn prepare(index: &ProtIndex, pack: &ExpansionPack, map_id: &str) -> Result<Self> {
        let mut job = PrepareJob::new(map_id);
        loop {
            if let Some(done) = job.step(index, pack)? {
                return Ok(done);
            }
        }
    }

    /// Roughly how many bytes the prepared map holds: the field image, the
    /// added meshes, the VRAM patches and the resource build's VRAM and
    /// meshes. A measure for the cache's footprint, not an exact count.
    pub fn approx_bytes(&self) -> usize {
        let o = &self.overrides;
        let mut n = o.field_map.len()
            + o.added_tmds.iter().map(Vec::len).sum::<usize>()
            + o.vram_patches
                .iter()
                .map(|p| p.words.len() * 2)
                .sum::<usize>();
        if let Some(b) = self.build.as_ref() {
            n += legaia_tim::VRAM_PIXELS * 2;
            n += b
                .tmds
                .iter()
                .map(|t| {
                    t.raw.len()
                        + t.tmd
                            .objects
                            .iter()
                            .map(|o| (o.vertices.len() + o.normals.len()) * 8)
                            .sum::<usize>()
                })
                .sum::<usize>();
        }
        n
    }
}

/// The maps `def`'s exits lead to, in exit order, each once.
pub fn connected_maps(def: &MapDef) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in &def.exits {
        if let ExitTarget::Map { map, .. } = &e.to
            && !out.contains(map)
        {
            out.push(map.clone());
        }
    }
    out
}

/// One pack map's preparation, run a phase at a time ([`Self::step`]).
pub struct PrepareJob {
    map_id: String,
    phase: Phase,
}

enum Phase {
    /// The base scene's field layer and the mesh library.
    Library,
    /// The field image and the materials.
    Image(Box<LibraryDone>),
    /// NPCs and scenery.
    Npcs(Box<ImageDone>),
    /// The resource build over the finished override.
    Build(Box<PreparedMap>),
    Done,
}

struct LibraryDone {
    grid: TileGrid,
    base_name: String,
    base: Scene,
    base_map: Option<Vec<u8>>,
    base_lut: Option<[i16; legaia_expansion_format::map::FLOOR_TIERS]>,
    geometry_entry: Option<u32>,
    geometry: Option<(u32, usize)>,
    library: Option<MeshLibrary>,
}

struct ImageDone {
    grid: TileGrid,
    base_name: String,
    base_lut: Option<[i16; legaia_expansion_format::map::FLOOR_TIERS]>,
    geometry: Option<(u32, usize)>,
    image: legaia_engine_scene::expansion_map::FieldImage,
    library: Option<MeshLibrary>,
    live_def: MapDef,
    notes: Vec<(log::Level, String)>,
}

impl PrepareJob {
    /// A job for map `map_id`, nothing run yet.
    pub fn new(map_id: &str) -> Self {
        Self {
            map_id: map_id.to_string(),
            phase: Phase::Library,
        }
    }

    /// Whether any phase has run.
    pub fn started(&self) -> bool {
        !matches!(self.phase, Phase::Library)
    }

    /// The map this job prepares.
    pub fn map_id(&self) -> &str {
        &self.map_id
    }

    /// Run the next phase. `Some` once the map is prepared; an error ends
    /// the job.
    pub fn step(&mut self, index: &ProtIndex, pack: &ExpansionPack) -> Result<Option<PreparedMap>> {
        let map_id = self.map_id.clone();
        let def = pack
            .maps
            .get(&map_id)
            .with_context(|| format!("pack '{}' has no map '{map_id}'", pack.manifest.id))?;
        let phase = std::mem::replace(&mut self.phase, Phase::Done);
        self.phase = match phase {
            Phase::Library => Phase::Image(Box::new(library_phase(index, pack, def, &map_id)?)),
            Phase::Image(l) => Phase::Npcs(Box::new(image_phase(def, &map_id, *l)?)),
            Phase::Npcs(i) => Phase::Build(Box::new(npc_phase(index, def, &map_id, *i)?)),
            Phase::Build(mut p) => {
                let mut scene = Scene::load(index, &p.base_name)?;
                scene.overrides = Some(p.overrides.clone());
                p.build = crate::scene_host::field_scene_build(index, &scene)
                    .ok()
                    .map(Arc::new);
                return Ok(Some(*p));
            }
            Phase::Done => anyhow::bail!("map '{map_id}' is already prepared"),
        };
        Ok(None)
    }
}

fn library_phase(
    index: &ProtIndex,
    pack: &ExpansionPack,
    def: &MapDef,
    map_id: &str,
) -> Result<LibraryDone> {
    let grid = def
        .tile_grid()
        .with_context(|| format!("map '{map_id}' tiles"))?;
    let base_name = def
        .base
        .scene_name()
        .with_context(|| format!("map '{map_id}' names no base scene"))?
        .to_string();
    // The base scene's own field layer, read before the override hides
    // it: its map (a kept geometry layer, a borrowed ground cell), its
    // floor ladder (a map that names none), and the entry holding its
    // geometry pack (a prop named by TMD coordinate).
    let base = Scene::load(index, &base_name)?;
    let base_map = base.field_map_bytes(index)?;
    let base_lut = base.field_floor_height_lut(index)?;
    let geometry_entry =
        legaia_engine_scene::scene_bundle::find_bundle(&base).map(|b| b.entry_idx());
    // The geometry pack as the field load registers it: where generated
    // wall boxes join it.
    let geometry = legaia_engine_scene::scene_resources::field_geometry_pack(&base);
    // Meshes from other scenes, model assets and decorations resolve
    // against the disc and the pack's assets before the image is built.
    let library = if needs_library(def, geometry.map(|(e, _)| e)) {
        Some(
            resolve_mesh_library(index, def, &pack.assets, &base, geometry.map(|(e, _)| e))
                .with_context(|| format!("map '{map_id}' meshes"))?,
        )
    } else {
        None
    };
    Ok(LibraryDone {
        grid,
        base_name,
        base,
        base_map,
        base_lut,
        geometry_entry,
        geometry,
        library,
    })
}

fn image_phase(def: &MapDef, map_id: &str, l: LibraryDone) -> Result<ImageDone> {
    // Items resting `on` another stand on its measured top; the map is
    // built and run from the resolved placement.
    let shapes = |m: &legaia_expansion_format::map::PropMesh, r: [f32; 3], s: f32| {
        l.library
            .as_ref()
            .and_then(|lib| mesh_rest_shape(lib, m, r, s))
    };
    let (rested, unresolved) = legaia_expansion_format::map_rest::rest_on(def, &shapes);
    let def = &rested;
    let image = build_field_image(
        def,
        &l.grid,
        BaseInputs {
            base_map: l.base_map.as_deref(),
            geometry_entry: l.geometry_entry,
            geometry_len: l.geometry.map(|(_, n)| n),
            library: l.library.as_ref(),
        },
    )
    .with_context(|| format!("map '{map_id}' field image"))?;
    drop(l.base);
    let mut notes = Vec::new();
    for p in &unresolved {
        notes.push((
            log::Level::Warn,
            format!("expansion: map '{map_id}': {}: {}", p.field, p.message),
        ));
    }
    // What rests on nothing, measured off the meshes themselves.
    for issue in legaia_expansion_format::map_rest::rest_issues(def, &shapes) {
        notes.push((
            log::Level::Warn,
            format!("expansion: map '{map_id}': rest: {issue}"),
        ));
    }
    // Materials naming another scene's VRAM point at the copies the
    // library made; the map runtime animates the resolved list.
    let mut live_def = def.clone();
    if let Some(lib) = l.library.as_ref() {
        for p in resolve_materials(&mut live_def, lib, &l.base_name) {
            notes.push((log::Level::Warn, format!("expansion: map '{map_id}': {p}")));
        }
    }
    if image.dropped_decorations > 0 {
        notes.push((
            log::Level::Warn,
            format!(
                "expansion: map '{map_id}': {} decorations on placed props' tiles left out",
                image.dropped_decorations
            ),
        ));
    }
    if !image.covered_cells.is_empty() {
        notes.push((
            log::Level::Warn,
            format!(
                "expansion: map '{map_id}': props replace the cell mesh of tiles {:?}",
                image.covered_cells
            ),
        ));
    }
    if !image.open_large_props.is_empty() {
        notes.push((
            log::Level::Warn,
            format!(
                "expansion: map '{map_id}': props {:?} draw more than a tile and a half across but close no ground (set `solid`)",
                image.open_large_props
            ),
        ));
    }
    if image.decoration_chunk > 1 {
        notes.push((
            log::Level::Info,
            format!(
                "expansion: map '{map_id}' bakes its decorations in {0} x {0} tile cells",
                image.decoration_chunk
            ),
        ));
    }
    Ok(ImageDone {
        grid: l.grid,
        base_name: l.base_name,
        base_lut: l.base_lut,
        geometry: l.geometry,
        image,
        library: l.library,
        live_def,
        notes,
    })
}

fn npc_phase(index: &ProtIndex, def: &MapDef, map_id: &str, i: ImageDone) -> Result<PreparedMap> {
    let ImageDone {
        grid,
        base_name,
        base_lut,
        geometry,
        image,
        library,
        mut live_def,
        notes,
    } = i;
    // Borrowed NPCs rest and walk in the clips their retail streams
    // name, where the pack names none.
    fill_borrowed_npc_clips(index, &mut live_def)
        .with_context(|| format!("map '{map_id}' NPC clips"))?;
    let mut library = library;
    let bank = library.as_mut().and_then(|l| l.npc_bank.take());
    let mut npcs = resolve_map_npcs(index, &live_def, &base_name, bank.as_ref())
        .with_context(|| format!("map '{map_id}' NPCs"))?;
    // The base scene's own animated props, as scenery after the NPCs.
    let scenery_props = base_scenery(index, def, &base_name, 1 + def.npcs.len())
        .with_context(|| format!("map '{map_id}' base scenery"))?;
    let n_scenery = scenery_props.len();
    npcs.extend(scenery_props);
    let particle_sheet = library.as_ref().and_then(|l| l.particle_sheet);
    let foam_cluts = library.as_ref().and_then(|l| l.foam_sheet).map(|f| f.cluts);
    let collision = Arc::new(image.collision);
    let overrides = SceneOverride {
        map_id: map_id.to_string(),
        field_map: image.image,
        floor_lut: floor_lut(def, base_lut),
        prop_scales: prop_scales_at(&live_def, &image.prop_anchors),
        npcs,
        added_tmds: image.added_tmds,
        geometry_entry: geometry.map(|(e, _)| e),
        vram_patches: library.map(|l| l.vram_patches).unwrap_or_default(),
        prop_records: prop_records_at(&live_def, &image.prop_anchors),
        npc_model_base: bank.as_ref().map_or(0, |b| b.first),
        npc_clips: bank.as_ref().and_then(|b| b.clips.clone()),
        npc_tmds: bank.map(|b| b.tmds).unwrap_or_default(),
    };
    Ok(PreparedMap {
        map_id: map_id.to_string(),
        base_name,
        grid: Arc::new(grid),
        live_def: Arc::new(live_def),
        overrides: Arc::new(overrides),
        n_scenery,
        collision,
        particle_sheet,
        foam_cluts,
        notes,
        build: None,
    })
}

/// A preparation away from the entry: queued (and, without threads, stepped
/// in place) or running on a worker thread.
enum Pending {
    #[cfg(not(target_arch = "wasm32"))]
    Worker(std::sync::mpsc::Receiver<Result<PreparedMap, String>>),
    Stepped(PrepareJob),
}

/// The prepared-map cache and the background preparation that fills it.
#[derive(Default)]
pub struct MapPreloader {
    /// Prepare the maps an entered map's exits lead to in the background.
    /// Off by default (tests and tools enter maps directly); the play hosts
    /// turn it on.
    pub enabled: bool,
    /// The pack the cache belongs to: a different pack active - a hot
    /// reload included - empties it.
    pack: Option<Arc<ExpansionPack>>,
    /// Most recently used last.
    ready: VecDeque<Arc<PreparedMap>>,
    /// In order of request.
    pending: Vec<(String, Pending)>,
}

impl MapPreloader {
    /// Forget everything prepared or preparing (a worker still running
    /// finishes into a dropped channel).
    pub fn clear(&mut self) {
        self.ready.clear();
        self.pending.clear();
        self.pack = None;
    }

    fn sync_pack(&mut self, pack: &Arc<ExpansionPack>) {
        if !self.pack.as_ref().is_some_and(|p| Arc::ptr_eq(p, pack)) {
            self.clear();
            self.pack = Some(pack.clone());
        }
    }

    /// The ids of the maps prepared and ready, least recently used first.
    pub fn ready_maps(&self) -> Vec<&str> {
        self.ready.iter().map(|p| p.map_id.as_str()).collect()
    }

    /// The ids of the maps still preparing.
    pub fn pending_maps(&self) -> Vec<&str> {
        self.pending.iter().map(|(m, _)| m.as_str()).collect()
    }

    /// The cache's footprint ([`PreparedMap::approx_bytes`] summed).
    pub fn approx_bytes(&self) -> usize {
        self.ready.iter().map(|p| p.approx_bytes()).sum()
    }

    fn insert(&mut self, p: Arc<PreparedMap>) {
        self.ready.retain(|r| r.map_id != p.map_id);
        self.ready.push_back(p);
        while self.ready.len() > PREPARED_MAP_CAPACITY
            || (self.ready.len() > 1 && self.approx_bytes() > PREPARED_MAP_BUDGET)
        {
            self.ready.pop_front();
        }
    }

    fn take_ready(&mut self, map_id: &str) -> Option<Arc<PreparedMap>> {
        let i = self.ready.iter().position(|p| p.map_id == map_id)?;
        let p = self.ready.remove(i)?;
        // Most recently used: it stays cached for the way back.
        self.ready.push_back(p.clone());
        Some(p)
    }

    /// Move every finished worker result into the cache without waiting.
    pub fn drain(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut done = Vec::new();
            self.pending.retain(|(id, p)| match p {
                Pending::Worker(rx) => match rx.try_recv() {
                    Ok(r) => {
                        done.push((id.clone(), r));
                        false
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => true,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
                },
                Pending::Stepped(_) => true,
            });
            for (id, r) in done {
                match r {
                    Ok(p) => self.insert(Arc::new(p)),
                    Err(e) => log::warn!("expansion: preparing map '{id}' failed: {e}"),
                }
            }
        }
    }

    /// The prepared `map_id` of `pack`: from the cache, from a preparation
    /// already under way (waited for), or built here and now. The result is
    /// cached either way.
    pub fn obtain(
        &mut self,
        index: &Arc<ProtIndex>,
        pack: &Arc<ExpansionPack>,
        map_id: &str,
    ) -> Result<Arc<PreparedMap>> {
        self.sync_pack(pack);
        self.drain();
        if let Some(p) = self.take_ready(map_id) {
            return Ok(p);
        }
        let prepared = match self.pending.iter().position(|(m, _)| m == map_id) {
            Some(i) => match self.pending.remove(i).1 {
                #[cfg(not(target_arch = "wasm32"))]
                Pending::Worker(rx) => match rx.recv() {
                    Ok(r) => r.map_err(|e| anyhow::anyhow!(e))?,
                    Err(_) => PreparedMap::prepare(index, pack, map_id)?,
                },
                Pending::Stepped(mut job) => loop {
                    if let Some(done) = job.step(index, pack)? {
                        break done;
                    }
                },
            },
            None => PreparedMap::prepare(index, pack, map_id)?,
        };
        let p = Arc::new(prepared);
        self.insert(p.clone());
        Ok(p)
    }

    /// Start preparing every map `map_id`'s exits lead to that is neither
    /// cached nor under way. A no-op while [`Self::enabled`] is off.
    pub fn request_neighbours(&mut self, pack: &Arc<ExpansionPack>, map_id: &str) {
        if !self.enabled {
            return;
        }
        self.sync_pack(pack);
        let Some(def) = pack.maps.get(map_id) else {
            return;
        };
        for next in connected_maps(def) {
            if next == map_id
                || !pack.maps.contains_key(&next)
                || self.ready.iter().any(|p| p.map_id == next)
                || self.pending.iter().any(|(m, _)| *m == next)
            {
                continue;
            }
            // Queued, not started: the entry's own host work (the render
            // build of the map just entered) runs first, and the next
            // `step` sends the queue to the workers.
            self.pending
                .push((next.clone(), Pending::Stepped(PrepareJob::new(&next))));
        }
    }

    /// Start the queued preparations on worker threads (the native build),
    /// or, where no thread starts (the browser build), run one phase of the
    /// oldest queued one - the idle slice. `true` while queued work remains
    /// to be stepped.
    pub fn step(&mut self, index: &Arc<ProtIndex>, pack: &Arc<ExpansionPack>) -> bool {
        self.sync_pack(pack);
        for (id, p) in &mut self.pending {
            if matches!(p, Pending::Stepped(j) if !j.started())
                && let Some(w) = spawn_worker(index, pack, id)
            {
                *p = w;
            }
        }
        let Some(i) = self
            .pending
            .iter()
            .position(|(_, p)| matches!(p, Pending::Stepped(_)))
        else {
            return false;
        };
        let job = match &mut self.pending[i].1 {
            Pending::Stepped(job) => job,
            #[cfg(not(target_arch = "wasm32"))]
            Pending::Worker(_) => return false,
        };
        match job.step(index, pack) {
            Ok(None) => {}
            Ok(Some(done)) => {
                self.pending.remove(i);
                self.insert(Arc::new(done));
            }
            Err(e) => {
                let (id, _) = self.pending.remove(i);
                log::warn!("expansion: preparing map '{id}' failed: {e:#}");
            }
        }
        self.pending
            .iter()
            .any(|(_, p)| matches!(p, Pending::Stepped(_)))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_worker(
    index: &Arc<ProtIndex>,
    pack: &Arc<ExpansionPack>,
    map_id: &str,
) -> Option<Pending> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (index, pack, id) = (index.clone(), pack.clone(), map_id.to_string());
    std::thread::Builder::new()
        .name(format!("map-prep-{map_id}"))
        .spawn(move || {
            let r = PreparedMap::prepare(&index, &pack, &id).map_err(|e| format!("{e:#}"));
            let _ = tx.send(r);
        })
        .ok()?;
    Some(Pending::Worker(rx))
}

#[cfg(target_arch = "wasm32")]
fn spawn_worker(_: &Arc<ProtIndex>, _: &Arc<ExpansionPack>, _: &str) -> Option<Pending> {
    None
}
