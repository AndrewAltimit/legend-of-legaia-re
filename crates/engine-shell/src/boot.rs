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

use crate::bgm::AudioBgmDirector;

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
        })
    }

    /// One per-frame step: tick the world, route field-VM camera + BGM
    /// events, advance the camera follow, return the [`SceneTickEvent`] for
    /// engines that want to react to scene transitions.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
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
