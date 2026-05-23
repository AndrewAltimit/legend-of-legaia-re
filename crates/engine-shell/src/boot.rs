//! Top-level engine boot session.
//!
//! Composes the per-crate primitives ([`legaia_engine_core::scene::SceneHost`],
//! [`legaia_engine_core::camera::Camera`], the BGM director from
//! [`crate::bgm::AudioBgmDirector`]) into one struct the binary drives per
//! frame. Mirrors the retail boot flow:
//!
//! 1. Open the extracted PROT + CDNAME map.
//! 2. Load a starting scene (the binary defaults to `town01`).
//! 3. Pick the scene's primary VAB bank, upload it to the SPU, and stash
//!    in the BGM director for subsequent op-`0x35` triggers.
//! 4. Drive the world tick + camera tick + event routing each frame.
//!
//! No window / renderer here - the binary owns winit + wgpu (or in headless
//! CI mode, no window). [`BootSession::tick`] is the per-frame driver
//! callable from either path.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_audio::{AudioOut, Spu, SpuAllocator, VabBank};
use legaia_engine_core::camera::Camera;
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost, SceneTickEvent};
use legaia_engine_core::world::SceneMode;

use crate::bgm::AudioBgmDirector;

/// Options for [`BootSession::enter_field_live`] - how much of the live
/// gameplay loop to arm when dropping into a field scene.
#[derive(Debug, Clone, Default)]
pub struct FieldLiveOpts {
    /// Arm the Field<->Battle live gameplay loop (`World::live_gameplay_loop`).
    /// `player_battle` implies this.
    pub live_loop: bool,
    /// Make battles player-driven (command menu). Implies `live_loop` and
    /// installs the item / spell / Seru catalogs a player-driven battle needs.
    pub player_battle: bool,
    /// Optional Battle<->Field BGM swap id (resolved through the scene's BGM
    /// table by the live loop).
    pub battle_bgm: Option<u16>,
}

/// Default scene the binary boots into when no `--scene` is supplied. Uses
/// the canonical first-town label from CDNAME.TXT.
pub const DEFAULT_BOOT_SCENE: &str = "town01";

/// Total SPU RAM in bytes (PSX hardware constant).
const SPU_RAM_BYTES: u32 = 512 * 1024;
/// Byte offset reserved for voice-0 / scratchpad - banks are allocated
/// above this. Mirrors the asset-viewer SEQ playback path.
const SPU_RESERVED_BYTES: u32 = 0x1000;

/// One-time configuration for [`BootSession::open`].
#[derive(Debug, Clone)]
pub struct BootConfig {
    /// Starting scene name (CDNAME label).
    pub scene: String,
    /// Whether to open the audio output. Set `false` for headless tests
    /// (cpal will fail to enumerate devices in CI).
    pub enable_audio: bool,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            scene: DEFAULT_BOOT_SCENE.to_string(),
            enable_audio: true,
        }
    }
}

/// Source of PROT.DAT + CDNAME.TXT bytes for a [`BootSession::open*`]
/// call. Internal - public construction is via the typed entry points
/// [`BootSession::open`] and [`BootSession::open_disc`].
enum SceneSource<'a> {
    Extracted(&'a Path),
    #[cfg(not(target_arch = "wasm32"))]
    Disc(&'a Path),
}

/// Per-frame session bundle. The binary owns one of these and calls
/// [`tick`](Self::tick) every frame.
pub struct BootSession {
    pub host: SceneHost,
    pub camera: Camera,
    pub audio: Option<Arc<AudioOut>>,
    pub bgm: Option<AudioBgmDirector>,
    /// Wall-clock frame counter, separate from `host.world.frame` (which
    /// includes pause-time skips when those land).
    pub frames: u64,
    /// New-game starting-party template parsed from the boot source's
    /// `SCUS_942.54`, if present. Used by [`BootSession::begin_new_game`] to
    /// seed a faithful starting roster; `None` when the executable couldn't be
    /// read (e.g. a raw PROT.DAT-only source), in which case New Game keeps the
    /// world's default scaffold party.
    pub starting_party: Option<legaia_asset::new_game::StartingParty>,
}

/// Read + parse the new-game starting-party template from a boot source's
/// `SCUS_942.54`. Returns `None` (not an error) when the executable isn't
/// reachable or doesn't parse, so a boot never fails just because the seed
/// data is unavailable.
fn read_starting_party(source: &SceneSource<'_>) -> Option<legaia_asset::new_game::StartingParty> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::new_game::StartingParty::from_scus(&scus)
}

impl BootSession {
    /// Open an extracted disc tree and load the configured scene. Errors if
    /// the directory isn't an extracted PROT or the scene name isn't in
    /// CDNAME.TXT.
    pub fn open(extracted_root: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Extracted(extracted_root), cfg)
    }

    /// Open the engine straight from a `.bin` disc image. The disc is walked
    /// once to extract `PROT.DAT` and `CDNAME.TXT`; no on-disk extraction
    /// step is required. Native targets only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_disc(disc_bin: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Disc(disc_bin), cfg)
    }

    fn open_with_source(source: SceneSource<'_>, cfg: &BootConfig) -> Result<Self> {
        // Parse the new-game starting-party template from the same source
        // (best-effort; never fails the boot).
        let starting_party = read_starting_party(&source);
        let mut host = match source {
            SceneSource::Extracted(root) => SceneHost::open_extracted(root)
                .with_context(|| format!("open extracted dir {}", root.display()))?,
            #[cfg(not(target_arch = "wasm32"))]
            SceneSource::Disc(path) => SceneHost::open_disc(path)
                .with_context(|| format!("open disc image {}", path.display()))?,
        };
        // Wire the CDNAME-derived map-id resolver so field-VM scene
        // transitions resolve to the right CDNAME label.
        host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
        host.load_scene(&cfg.scene)
            .with_context(|| format!("load scene '{}'", cfg.scene))?;

        // Audio + BGM director (optional - disabled for headless tests).
        let (audio, bgm) = if cfg.enable_audio {
            match AudioOut::new() {
                Ok(audio) => {
                    // AudioOut owns a cpal::Stream which is Send but not Sync.
                    // BootSession is single-threaded (binary + WASM both
                    // tick on one thread); the Arc just gives the BGM
                    // director a refcounted handle.
                    #[allow(clippy::arc_with_non_send_sync)]
                    let audio = Arc::new(audio);
                    let mut director = AudioBgmDirector::new(audio.clone());
                    if let Err(e) = stage_scene_vab(&mut director, audio.as_ref(), &host) {
                        log::warn!("BGM bank not staged (scene VAB resolution failed): {e:#}");
                    }
                    (Some(audio), Some(director))
                }
                Err(e) => {
                    log::warn!("audio disabled - open failed: {e:#}");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(Self {
            host,
            camera: Camera::default(),
            audio,
            bgm,
            frames: 0,
            starting_party,
        })
    }

    /// Begin a New Game: clear the world to a fresh slate
    /// ([`legaia_engine_core::world::World::begin_new_game`]) and seed the
    /// starting party (Vahn) from the boot source's `SCUS_942.54` template.
    ///
    /// Mirrors the retail NEW GAME → field-launch chain (master mode 2 → 3,
    /// see `docs/subsystems/boot.md`). The opening scene
    /// ([`legaia_asset::new_game::OPENING_CUTSCENE_SCENE`] = `opdeene`, the
    /// prologue cutscene, which hands off to `town01`) is entered through the
    /// usual [`BootSession::enter_field_live`] path; this call only resets and
    /// seeds the world state. When the SCUS template isn't available the world
    /// keeps its default scaffold party so the slice stays runnable.
    pub fn begin_new_game(&mut self) {
        self.host.world.begin_new_game();
        if let Some(starting) = &self.starting_party {
            self.host.world.seed_starting_party(starting);
        }
    }

    /// One per-frame step: tick the world, route field-VM camera + BGM
    /// events, advance the camera follow, return the [`SceneTickEvent`] for
    /// engines that want to react to scene transitions.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        // Feed the previous frame's camera azimuth into the world so the
        // field free-movement controller remaps the d-pad camera-relative
        // ("screen up" walks away from the camera). The follow camera's
        // default yaw is 0, which maps straight to world +Z.
        let azimuth = (self.camera.yaw / std::f32::consts::TAU * 4096.0).rem_euclid(4096.0);
        self.host.world.field_camera_azimuth = azimuth as u16;
        let event = self.host.tick()?;
        self.camera.route_camera_events(&mut self.host.world);
        if let Some(bgm) = self.bgm.as_mut() {
            // SceneHost::route_bgm_events drains the world's pending BGM
            // events and dispatches into the director.
            let _ = self.host.route_bgm_events(bgm)?;
        }
        // After events: camera tick + scene-transition BGM rebind.
        self.camera.tick(&self.host.world);
        if let SceneTickEvent::SceneEntered { .. } = &event
            && let (Some(bgm), Some(audio)) = (self.bgm.as_mut(), self.audio.as_ref())
        {
            // New scene -> upload its VAB bank.
            if let Err(e) = stage_scene_vab(bgm, audio.as_ref(), &self.host) {
                log::warn!("BGM bank not staged after scene enter: {e:#}");
            }
        }
        self.frames += 1;
        Ok(event)
    }

    /// Drop the world into a live field scene: run the scene's event-script
    /// record 0 (the init prologue) so the field VM actually ticks, install
    /// the per-scene encounter table, and arm the live gameplay loop per
    /// `opts`.
    ///
    /// [`BootSession::open`] only calls `load_scene`, which leaves the world
    /// in [`SceneMode::Title`] with no field events firing. This is the
    /// reusable core of the windowed host's `--live-loop` setup, shared so the
    /// v0.1 oracle and headless drivers reach Field/Battle the same way the
    /// window does.
    ///
    /// Soft-fails the same way the window does: a scene with no event script
    /// logs and continues (the world stays in whatever mode it was in).
    /// Returns the active [`SceneMode`] after the attempt.
    pub fn enter_field_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        match self.host.enter_field_scene(scene, 0) {
            Ok(()) => log::info!("entered field scene '{scene}' record 0 (field VM live)"),
            Err(e) => log::warn!(
                "enter_field_scene('{scene}', 0) failed ({e:#}); staying on the load_scene-only \
                 path (field VM will not tick)"
            ),
        }

        let world = &mut self.host.world;
        world.set_active_scene_label(scene);

        // `enter_field_scene` already installs the disc-resident per-scene
        // encounter table from the MAN asset. Only fall back to the synthetic
        // registry + vanilla tables when no MAN encounter was installed.
        if world.encounter.is_none() && matches!(world.mode, SceneMode::Field) {
            world.set_formation_table(
                legaia_engine_core::monster_catalog::vanilla_formation_table(),
                legaia_engine_core::monster_catalog::vanilla_monster_catalog(),
            );
            let registry = legaia_engine_core::encounter_registry::vanilla_encounter_registry();
            world.install_encounter_for_scene(&registry, scene);
        }

        if opts.live_loop || opts.player_battle {
            world.live_gameplay_loop = true;
            // Equipped-gear bonuses fold onto party attack/defense at battle
            // entry (no-op until a real roster with non-zero stats is loaded).
            world.set_equipment_table(
                legaia_engine_core::equipment::vanilla_equipment_catalog().to_modifier_table(),
            );
        }
        world.set_battle_bgm(opts.battle_bgm);
        if opts.player_battle {
            world.battle_player_driven = true;
            world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
            world.set_spell_catalog(legaia_engine_core::retail_magic::retail_seru_magic_catalog());
            world.set_seru_registry(legaia_engine_core::seru_learning::SeruRegistry::retail());
        }

        Ok(world.mode)
    }

    /// Enter a world-map scene live: load the scene's resources, route its
    /// region-keyed encounter table onto the overworld, install the player
    /// actor, and switch into [`SceneMode::WorldMap`].
    ///
    /// The window's `--world-map` flag used to call [`World::enter_world_map`]
    /// directly, which only installs the camera controller (a camera-only
    /// debug viewer). This is the playable counterpart to [`Self::enter_field_live`]:
    /// it loads the scene through [`SceneHost::enter_field_scene`] (which seeds
    /// the formation table + monster catalog from the MAN, so overworld
    /// encounters resolve to real monsters), builds the
    /// [`RegionEncounterTable`](legaia_engine_core::region_encounter::RegionEncounterTable)
    /// from the same MAN, routes it via
    /// [`World::set_world_map_regions`], installs the field player so
    /// `tick_world_map`'s locomotion + per-tile encounter roll run, and enters
    /// world-map mode with the live loop armed.
    ///
    /// Soft-fails like [`Self::enter_field_live`]: a scene that fails to load
    /// logs and continues into world-map mode without a region table (camera
    /// only). Returns the active [`SceneMode`].
    pub fn enter_world_map_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        match self.host.enter_field_scene(scene, 0) {
            Ok(()) => log::info!("entered world-map scene '{scene}' (resources loaded)"),
            Err(e) => log::warn!(
                "enter_field_scene('{scene}', 0) failed ({e:#}); world map will be camera-only"
            ),
        }
        self.host.world.set_active_scene_label(scene);

        // Build the region-keyed encounter table from the scene's MAN (clone
        // the bytes out first so the immutable host borrow drops before the
        // mutable world borrow below).
        let man = self
            .host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.host.index).ok().flatten());
        if let Some(man) = man
            && let Some(table) =
                legaia_engine_core::region_encounter::region_encounter_table_from_man(scene, &man)
        {
            log::info!(
                "world-map '{scene}': routed {} encounter regions",
                table.regions.len()
            );
            self.host.world.set_world_map_regions(table);
        }

        let world = &mut self.host.world;
        // `enter_field_scene` already installed slot 0 as the field player and
        // loaded the per-scene walkability grid. Do NOT re-install here:
        // `install_field_player` ends with `reset_field_collision_grid`, which
        // would wipe the overworld's walls (the kingdom maps carry thousands of
        // wall sub-cells) and leave the player roaming unbounded. Just enter
        // world-map mode (installs the camera controller).
        world.enter_world_map();
        world.live_gameplay_loop = true;
        world.set_equipment_table(
            legaia_engine_core::equipment::vanilla_equipment_catalog().to_modifier_table(),
        );
        world.set_battle_bgm(opts.battle_bgm);
        if opts.player_battle {
            world.battle_player_driven = true;
            world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
            world.set_spell_catalog(legaia_engine_core::retail_magic::retail_seru_magic_catalog());
            world.set_seru_registry(legaia_engine_core::seru_learning::SeruRegistry::retail());
        }
        Ok(world.mode)
    }

    /// Enter a field scene live, then seed the world from a saved game.
    ///
    /// [`Self::enter_field_live`] cold-boots the scene at record 0 (a fresh
    /// party, no story progress). This variant runs that path and then
    /// hydrates the world from `save` via [`legaia_engine_core::World::load_full`]
    /// (party records, story flags, money, inventory) so the field VM sees the
    /// saved story state on its first tick. It is the building block for
    /// "continue a saved game" and for the story-gated paths that a cold boot
    /// into record 0 can't reach, such as a scripted-encounter trigger armed
    /// by story state.
    ///
    /// The save is applied *after* the scene is entered, so the scene record
    /// is still 0; selecting the story-appropriate record from the seeded
    /// flags is a separate concern (the field VM's record picker).
    ///
    /// To seed from a retail memory-card SC block, parse it first with
    /// [`legaia_save::SaveFile::from_retail_sc_block`].
    pub fn enter_field_live_from_save(
        &mut self,
        scene: &str,
        opts: &FieldLiveOpts,
        save: legaia_save::SaveFile,
    ) -> Result<SceneMode> {
        self.enter_field_live(scene, opts)?;
        self.host.world.load_full(save);
        log::info!("seeded world from save ({} party records)", {
            self.host.world.party_count
        });
        Ok(self.host.world.mode)
    }

    /// Shut down the audio stream and clear the scene. Idempotent.
    pub fn shutdown(&mut self) {
        if let Some(audio) = self.audio.take() {
            audio.detach_sequencer();
        }
        self.bgm = None;
    }
}

impl Drop for BootSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Pull the scene's first VAB-bearing entry through the scene host, parse
/// it, upload its samples into the SPU, and stash the resulting [`VabBank`]
/// in the director.
fn stage_scene_vab(
    director: &mut AudioBgmDirector,
    audio: &AudioOut,
    host: &SceneHost,
) -> Result<()> {
    let Some(bytes) = host.scene_vab_bytes()? else {
        return Ok(());
    };
    let report = legaia_vab::parse(&bytes, 0).context("parse scene VAB header")?;
    let bank = audio.with_spu(|spu: &mut Spu| {
        let mut alloc = SpuAllocator::new(SPU_RESERVED_BYTES, SPU_RAM_BYTES - SPU_RESERVED_BYTES);
        VabBank::upload(spu, &mut alloc, &report, &bytes)
    });
    director.set_bank(bank);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_boot_config_uses_town01() {
        let c = BootConfig::default();
        assert_eq!(c.scene, "town01");
        assert!(c.enable_audio);
    }
}
