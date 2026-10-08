//! Engine core primitives: virtual filesystem, asset cache, frame time, and
//! the composite [`world::World`] that wires the per-VM hosts from
//! `legaia-engine-vm` into a single runtime.
//!
//! Engine-agnostic. No wgpu / windowing / audio dependencies - the asset
//! crates talk to this layer, the render and audio crates read from it.

// The battle kernels live in `legaia-engine-battle`; re-exported so every
// `legaia_engine_core::<module>` path (and `crate::<module>` inside this
// crate) keeps resolving.
pub use legaia_engine_battle::{
    accessory_passives, ap_gauge, art_strike, battle_afterimage, battle_anim, battle_body_blend,
    battle_effect_clut, battle_events, battle_return_flags, battle_seats, battle_sideband,
    battle_stats, battle_status_clut, battle_steal, encounter, encounter_man, encounter_record,
    encounter_registry, levelup, magic_xp, monster_ai, monster_catalog, move_power,
    region_encounter, retail_magic, seru_learning, seru_stats, seru_trade, spells, tactical_arts,
    tactical_arts_editor,
};
pub mod actor_alloc_host;
pub mod actor_handler;
pub mod actor_look;
pub mod anim_cue;
pub use legaia_engine_battle::arts_command_input;
pub mod baka_duel_scene;
pub use legaia_engine_battle::battle_arts;
pub mod battle_cam_inputs;
pub mod battle_flow;
pub mod battle_hud;
pub mod battle_input;
pub use legaia_engine_battle::battle_magic;
pub mod battle_open;
pub mod battle_party_form;
pub mod battle_round;
pub mod battle_sideband_textures;
pub mod battle_stage_module;
pub mod battle_tutorial;
pub mod camera;
pub mod camera_view;
pub mod camera_zone;
pub use legaia_engine_menus::card_bu_io;
pub use legaia_engine_menus::card_flow;
pub use legaia_engine_system::capture_observations;
pub mod card_write;
pub mod cd_dma;
pub mod cheat_applier;
pub mod cheats;
pub use legaia_engine_system::chunk_install;
pub mod clut_cell_fx;
pub mod clut_fx;
pub mod clut_walk_anim;
pub mod coplanar_draws;
pub use legaia_engine_system::cutscene;
pub mod cutscene_caption;
pub mod cutscene_narration;
pub mod cutscene_script_elements;
pub mod cutscene_timeline;
pub mod dance;
pub mod dance_cast_scene;
pub mod dance_venue;
pub use legaia_engine_menus::debug_char_editor;
pub mod dev_menu;
pub mod dev_menu_host;
pub mod dialog;
pub use legaia_engine_menus::dialog_pacing;
pub use legaia_engine_menus::dialog_picker_slide;
pub use legaia_engine_menus::dialog_window;
pub use legaia_engine_system::draw_census;
pub mod drop_shadow;
pub mod equip_session;
pub use legaia_engine_menus::equipment;
pub use legaia_engine_system::fade;
pub use legaia_engine_system::fade_ramp;
pub mod field_anim;
pub mod field_audio_release;
pub mod field_channels;
pub mod field_env;
pub mod field_events;
pub mod field_ground;
pub mod field_lit_mesh;
pub mod field_menu;
pub mod field_menu_dispatch;
pub mod field_occlusion;
pub use legaia_engine_vm::field_regions;
pub mod field_view_window;
pub mod fishing_actors;
pub mod fishing_exchange_input;
pub mod fishing_hub;
pub mod fishing_scene;
pub mod fishing_venue;
pub mod fog_particles;
pub mod fog_volume;
pub mod frame_step;
pub use legaia_engine_menus::game_over;
pub mod glb_export;
pub mod inline_dialogue;
pub use legaia_engine_menus::inn;
pub use legaia_engine_menus::inventory_use;
pub use legaia_engine_menus::items;
pub use legaia_engine_menus::key_rebind;
pub use legaia_engine_system::input;
pub mod list_order;
pub mod live_loop;
pub mod man_field_scripts;
pub use legaia_engine_menus::menu_arrange;
pub use legaia_engine_menus::menu_cues;
pub use legaia_engine_menus::menu_glyph_atlas;
pub use legaia_engine_menus::menu_item_category;
pub use legaia_engine_menus::menu_list_rows;
pub use legaia_engine_menus::menu_open_sequence;
pub use legaia_engine_system::mdec_dma_sync;
pub use legaia_engine_vm::menu_input;
pub mod menu_runtime;
pub mod menu_validator;
pub use legaia_engine_menus::menu_widget;
pub mod minigame_entry;
pub mod minigame_status;
pub mod mode;
pub mod model_bank;
pub mod move_buffer_host;
pub use legaia_engine_system::movie_audio;
pub mod muscle_dome;
pub mod muscle_dome_scene;
pub mod muscle_ringside;
pub use legaia_engine_menus::name_entry;
pub use legaia_engine_system::music_labels;
pub mod new_game;
pub mod npc_catalog;
pub use legaia_engine_effects::object_effect;
pub mod options;
pub mod overlay_loader;
pub mod overworld_curvature;
pub mod overworld_draw_order;
pub mod overworld_ground_cue;
pub mod packet_color;
pub use legaia_engine_effects::part_motion;
pub mod pause_screens;
pub use legaia_engine_system::pause_wipe;
pub mod place_name_banner;
pub use legaia_engine_menus::publisher_logos;
pub use legaia_engine_system::ram_map;
pub mod register_ramp;
pub mod resume;
pub use legaia_engine_menus::save_menu_atlas;
pub use legaia_engine_system::retail_pad;
pub mod save_screen;
pub mod save_select;
pub mod save_subscreen;
pub mod scene;
pub mod scene_assembly;
pub mod scene_assets;
pub mod scene_bundle;
pub mod scene_live;
pub use legaia_engine_system::scene_name_sync;
pub mod scene_resources;
pub use legaia_engine_effects::screen_fx;
pub mod scus_leaf_kernels;
pub mod sfx_cue;
pub mod shop;
pub mod shop_catalog;
pub use legaia_engine_menus::spell_menu;
pub use legaia_engine_menus::spell_party_broadcast;
pub use legaia_engine_system::sound_state;
pub mod status_screen;
pub mod stream_file;
pub use legaia_engine_battle::target_picker;
pub use legaia_engine_effects::summon;
pub use legaia_engine_menus::text_balloon;
pub mod tile_board;
pub mod timed_fight;
pub use legaia_engine_menus::title;
pub use legaia_engine_menus::title_screen_atlas;
pub mod vdf_pulse;
pub mod walk_regen;
pub mod world;
pub mod world_map;
pub mod world_map_markers;
pub mod world_map_sky;

pub mod float_tween;

pub use legaia_engine_effects::action_effect_script;
pub mod camera_ease;
pub mod casino_coin_bank;
pub use legaia_engine_effects::effect_default_arm;
pub use legaia_engine_effects::effect_ribbon;
pub use legaia_engine_effects::effect_sprite_arm;
pub mod field_actor_clone;
pub mod field_actor_kernels;
pub mod field_actor_program;
pub mod field_submode;
pub mod field_submode_code_lock;
pub mod field_submode_flag_window;
pub use legaia_engine_system::mode_entry_init;

pub mod field_submode_screen;
pub mod incense_notice;
pub mod world_map_panel_host;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// Lock a mutex, tolerating poison. A poisoned mutex means some thread
/// panicked while holding the lock; every mutex in this crate guards either a
/// read-through cache (a partially-populated map is still correct - the
/// missing entries are simply recomputed) or a read-only disc / archive
/// reader (byte reads are idempotent). In all those cases the guarded data is
/// still usable, so recover the guard via [`std::sync::PoisonError::into_inner`]
/// rather than cascading a second panic into an unrelated caller. Used by the
/// asset caches here and the PROT index caches in [`scene::prot_index`].
pub(crate) fn lock_poison_tolerant<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod lock_poison_tolerant_tests {
    use super::lock_poison_tolerant;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recovers_a_poisoned_mutex_instead_of_panicking() {
        let m = Arc::new(Mutex::new(vec![1u8, 2, 3]));
        // Poison the mutex: panic while holding the lock on another thread.
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("intentional poison");
        })
        .join();
        assert!(m.lock().is_err(), "mutex should be poisoned for this test");
        // The tolerant lock recovers the guard and the data is intact.
        let g = lock_poison_tolerant(&m);
        assert_eq!(&*g, &[1u8, 2, 3]);
    }
}

/// Source of asset bytes.
///
/// Two backends planned: an extracted-directory backend (for development -
/// reads from `extracted/` produced by `legaia-extract`) and a disc-backed
/// backend (for end users - reads directly from a disc image).
///
/// Both yield raw bytes addressed by a logical name (e.g.
/// `"prot/0123_some_entry.bin"`). The asset crates above this layer turn
/// bytes into typed structures.
pub trait Vfs: Send + Sync {
    fn read(&self, name: &str) -> Result<Vec<u8>>;
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
    fn exists(&self, name: &str) -> bool;
}

/// Filesystem-backed Vfs rooted at a directory (e.g. `extracted/`).
pub struct DirVfs {
    root: PathBuf,
}

impl DirVfs {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.is_dir() {
            anyhow::bail!("DirVfs root is not a directory: {}", root.display());
        }
        Ok(Self { root })
    }
}

impl Vfs for DirVfs {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let p = self.root.join(name);
        std::fs::read(&p).with_context(|| format!("read {}", p.display()))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let dir = self.root.join(prefix);
        let mut out = Vec::new();
        if !dir.is_dir() {
            return Ok(out);
        }
        for ent in std::fs::read_dir(&dir).with_context(|| format!("list {}", dir.display()))? {
            let ent = ent?;
            let rel = ent
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .into_owned();
            out.push(rel);
        }
        out.sort();
        Ok(out)
    }

    fn exists(&self, name: &str) -> bool {
        self.root.join(name).exists()
    }
}

/// Vfs backed by a PSX `.bin` disc image. Reads files directly from the
/// ISO9660 filesystem using `legaia-iso`.
///
/// Names are normalised to forward-slash, case-insensitive. Both
/// `"PROT.DAT"` and `"prot.dat"` resolve to the same entry.
///
/// `legaia-iso` is `cfg(not(target_arch = "wasm32"))` only - DiscVfs is
/// only available on native targets. WASM builds keep `MemoryVfs`.
#[cfg(not(target_arch = "wasm32"))]
pub struct DiscVfs {
    raw: Mutex<legaia_iso::raw::RawDisc>,
    /// Normalised lowercase forward-slash path → directory record.
    files: HashMap<String, legaia_iso::iso9660::DirectoryRecord>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DiscVfs {
    /// Open a `.bin` disc image and walk its ISO9660 tree once.
    ///
    /// Subsequent reads are O(1) lookups into the file map plus a sector
    /// fetch from the disc.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut raw = legaia_iso::raw::RawDisc::open(path.as_ref())
            .with_context(|| format!("open disc image {}", path.as_ref().display()))?;
        let volume = legaia_iso::iso9660::read_volume(&mut raw).context("read ISO9660 volume")?;
        let walked =
            legaia_iso::iso9660::walk_files(&mut raw, &volume.root).context("walk ISO9660 tree")?;
        let mut files = HashMap::with_capacity(walked.len());
        for (path_in_iso, rec) in walked {
            let key = normalise_disc_name(&path_in_iso);
            files.insert(key, rec);
        }
        Ok(Self {
            raw: Mutex::new(raw),
            files,
        })
    }

    /// Number of files indexed. For a retail Legaia disc this is in the
    /// low hundreds (mostly inside `DATA/`).
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Iterator over the indexed file paths in arbitrary order.
    pub fn iter_paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }

    /// Read raw bytes for a logical file name.
    fn read_record(&self, rec: &legaia_iso::iso9660::DirectoryRecord) -> Result<Vec<u8>> {
        let sector_count = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let mut buf = Vec::with_capacity(rec.size as usize);
        lock_poison_tolerant(&self.raw)
            .read_user_data(rec.lba, sector_count, &mut buf)
            .with_context(|| format!("read disc file {}", rec.name))?;
        buf.truncate(rec.size as usize);
        Ok(buf)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Vfs for DiscVfs {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        let key = normalise_disc_name(name);
        let rec = self
            .files
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("DiscVfs: '{}' not found in ISO9660 tree", name))?
            .clone();
        self.read_record(&rec)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let key_prefix = normalise_disc_name(prefix);
        let mut out: Vec<String> = self
            .files
            .keys()
            .filter(|k| k.starts_with(&key_prefix))
            .cloned()
            .collect();
        out.sort();
        Ok(out)
    }

    fn exists(&self, name: &str) -> bool {
        self.files.contains_key(&normalise_disc_name(name))
    }
}

/// Normalise a disc-relative path: backslashes → forward slashes,
/// lowercased, leading slash stripped.
#[cfg(not(target_arch = "wasm32"))]
fn normalise_disc_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    s = s.to_ascii_lowercase();
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// In-memory Vfs backed by a `HashMap`. Useful for tests and WASM targets
/// where no real filesystem is available.
pub struct MemoryVfs {
    files: std::collections::HashMap<String, Vec<u8>>,
}

impl MemoryVfs {
    pub fn new() -> Self {
        Self {
            files: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        self.files.insert(name.into(), bytes);
    }
}

impl Default for MemoryVfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs for MemoryVfs {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.files
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MemoryVfs: '{}' not found", name))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut out: Vec<String> = self
            .files
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        out.sort();
        Ok(out)
    }

    fn exists(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }
}

/// Trivial bytes cache keyed by Vfs name.
///
/// Lives behind a Mutex so it can be shared across loader threads later. The
/// API is intentionally narrow - a real engine would need eviction policy,
/// per-asset-type typed caches, and pinning. We add those when we need them.
pub struct AssetCache {
    inner: Mutex<HashMap<String, Arc<Vec<u8>>>>,
}

impl AssetCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_or_load(&self, vfs: &dyn Vfs, name: &str) -> Result<Arc<Vec<u8>>> {
        if let Some(b) = lock_poison_tolerant(&self.inner).get(name).cloned() {
            return Ok(b);
        }
        let bytes = Arc::new(vfs.read(name)?);
        lock_poison_tolerant(&self.inner).insert(name.to_string(), bytes.clone());
        Ok(bytes)
    }
}

impl Default for AssetCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Wall-clock + delta accumulator. Used by the frame loop to drive
/// fixed-timestep gameplay updates while letting render run uncapped.
///
/// On `wasm32-unknown-unknown` `std::time::Instant` is not implemented, so
/// this type becomes a zero-size stub - callers (JS `requestAnimationFrame`
/// loop) supply their own delta timing.
pub struct FrameTime {
    #[cfg(not(target_arch = "wasm32"))]
    started_at: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    last_frame: Instant,
}

impl FrameTime {
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            Self {
                started_at: now,
                last_frame: now,
            }
        }
        #[cfg(target_arch = "wasm32")]
        Self {}
    }

    pub fn tick(&mut self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            let dt = now - self.last_frame;
            self.last_frame = now;
            dt
        }
        #[cfg(target_arch = "wasm32")]
        Duration::ZERO
    }

    pub fn elapsed(&self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Instant::now() - self.started_at
        }
        #[cfg(target_arch = "wasm32")]
        Duration::ZERO
    }
}

impl Default for FrameTime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_time_starts_at_zero() {
        let ft = FrameTime::new();
        assert!(ft.elapsed() < Duration::from_millis(50));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn disc_vfs_name_normalisation() {
        // Mixed case + backslashes + leading slash → lowercase forward-slash
        // form. Disc paths come from the ISO9660 walker which uses
        // forward slashes already, but user input via --disc-relative
        // names may use the retail-style backslashes.
        assert_eq!(super::normalise_disc_name("PROT.DAT"), "prot.dat");
        assert_eq!(super::normalise_disc_name("/PROT.DAT"), "prot.dat");
        assert_eq!(
            super::normalise_disc_name("DATA\\FIELD\\TOWN01\\STAGE.LZS"),
            "data/field/town01/stage.lzs"
        );
        assert_eq!(
            super::normalise_disc_name("data/cdname.txt"),
            "data/cdname.txt"
        );
    }
}

pub use legaia_engine_vm::camera_rel_glide;
pub mod field_save_screen_actor;
pub mod morph_weight_apply;
pub mod scene_transition_actor;

// The minigame rules engines live in `legaia-engine-minigames`; re-exported
// here so every host and test keeps its `legaia_engine_core::<module>` path.
pub use legaia_engine_minigames::{
    baka_cabinet, baka_fighter, baka_fighter_chrome, baka_impact_fx, dance_tutorial, fishing,
    fishing_chrome, minigame_actor, minigame_floor, minigame_fx, other_game_overlay,
    prize_exchange, slot_machine,
};
