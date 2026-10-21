//! The native host's file-system policy: a session over the cpal output,
//! opened from an extracted tree or a disc image on disk, with the CD-XA
//! banks read off that image.
//!
//! Everything else in this crate is host-neutral and builds for `wasm32`;
//! this module is compiled for native targets only. The browser host builds
//! its session with [`BootSession::from_host`] over bytes it already holds
//! and stages the same banks through [`crate::xa_banks`].

use std::path::Path;

use anyhow::{Context, Result};
use legaia_engine_audio::{AudioOut, AudioSink};
use legaia_engine_scene_host::scene_host::SceneHost;

use crate::audio_director::AudioDirector;
use crate::session::{BootConfig, BootSession};

mod xa_disc;

pub use xa_disc::{
    XaDiscSource, read_arts_shout_bank, read_battle_xa_clip_bank, read_xa_channel_span,
};

/// The session over the native cpal output.
pub type NativeSession = BootSession<AudioOut>;

/// The audio director over the native cpal output.
pub type NativeAudioDirector = AudioDirector<AudioOut>;

/// Source of PROT.DAT + CDNAME.TXT bytes for a [`BootSession::open*`]
/// call. Internal - public construction is via the typed entry points
/// [`BootSession::open`] and [`BootSession::open_disc`].
enum SceneSource<'a> {
    Extracted(&'a Path),
    Disc(&'a Path),
}

/// The native constructors: a session over the cpal output.
impl BootSession<AudioOut> {
    /// Open an extracted disc tree and load the configured scene. Errors if
    /// the directory isn't an extracted PROT or the scene name isn't in
    /// CDNAME.TXT.
    pub fn open(extracted_root: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Extracted(extracted_root), cfg, AudioOut::new)
    }

    /// Open the engine straight from a `.bin` disc image. The disc is walked
    /// once to extract `PROT.DAT` and `CDNAME.TXT`; no on-disk extraction
    /// step is required.
    pub fn open_disc(disc_bin: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Disc(disc_bin), cfg, AudioOut::new)
    }

    /// [`Self::open_disc`] over a device-less output
    /// ([`AudioOut::detached`]) when `cfg.enable_audio` is set: the director
    /// stages and routes as it does over a device, and nothing is heard.
    pub fn open_disc_detached_audio(disc_bin: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Disc(disc_bin), cfg, || {
            Ok(AudioOut::detached(legaia_engine_audio::SPU_INTERNAL_RATE))
        })
    }
}

impl<S: AudioSink> BootSession<S> {
    /// `open_audio` opens the host's output; it runs only when
    /// `cfg.enable_audio` is set.
    fn open_with_source(
        source: SceneSource<'_>,
        cfg: &BootConfig,
        open_audio: impl FnOnce() -> Result<S>,
    ) -> Result<Self> {
        let scus = read_scus(&source);
        let host = match source {
            SceneSource::Extracted(root) => SceneHost::open_extracted(root)
                .with_context(|| format!("open extracted dir {}", root.display()))?,
            SceneSource::Disc(path) => SceneHost::open_disc(path)
                .with_context(|| format!("open disc image {}", path.display()))?,
        };
        #[allow(unused_mut)]
        let mut session = Self::from_host(host, scus.as_deref(), cfg, open_audio)?;
        apply_env_world_toggles(&mut session.host.world.toggles);
        // Demux + decode the arts-voice shout banks (XA2/XA4/XA6) and the
        // SCUS cue tables. Disc-image boots only (channel demux needs the
        // raw CD-XA subheaders). Best-effort - an absent bank leaves arts
        // silent.
        if let (SceneSource::Disc(path), Some(director)) = (&source, session.bgm.as_mut()) {
            match read_arts_shout_bank(path) {
                Some(bank) => director.set_shout_bank(bank),
                None => log::warn!("arts-voice shout bank not staged"),
            }
            // The battle's one-shot clips (`XA27` stings, `XA30` grunts) -
            // the melee kernel's two sound sites.
            match read_battle_xa_clip_bank(path) {
                Some(bank) => director.set_xa_clip_bank(bank),
                None => log::warn!("battle CD-XA clip bank not staged"),
            }
            // The cast voices (fifteen files) are not decoded here: the
            // director stages one channel span from the disc the first time
            // a cast names it.
            let n = director.set_xa_lazy_source(path);
            if n == 0 {
                log::warn!("no XA<n>.XA files resolved; cast voices stay silent");
            }
        }
        Ok(session)
    }
}

/// `LEGAIA_BATTLE_FRAME_CADENCE`: `Some(true)` set and not `0`,
/// `Some(false)` set to `0`, `None` unset.
pub fn env_battle_frame_cadence() -> Option<bool> {
    battle_frame_cadence_switch(std::env::var_os("LEGAIA_BATTLE_FRAME_CADENCE").as_deref())
}

/// The value of a `LEGAIA_BATTLE_FRAME_CADENCE` switch: on unless it reads
/// `0`, no opinion when unset.
pub fn battle_frame_cadence_switch(v: Option<&std::ffi::OsStr>) -> Option<bool> {
    v.map(|v| v != "0")
}

/// Whether a native session's battles run on retail's battle-frame cadence:
/// `LEGAIA_BATTLE_FRAME_CADENCE` when set, the engine's single default
/// ([`legaia_engine_core::world::DEFAULT_BATTLE_SM_FRAME_CADENCE`]) otherwise.
/// The retail comparison reads it to know which clock its replays run on.
pub fn battle_frame_cadence_on() -> bool {
    env_battle_frame_cadence().unwrap_or(legaia_engine_core::world::DEFAULT_BATTLE_SM_FRAME_CADENCE)
}

/// `LEGAIA_ARTS_SAVED_LIST`: set at all.
pub fn env_arts_saved_list() -> bool {
    std::env::var_os("LEGAIA_ARTS_SAVED_LIST").is_some()
}

/// `LEGAIA_SMART_MONSTERS`: set at all.
pub fn env_smart_monsters() -> bool {
    std::env::var_os("LEGAIA_SMART_MONSTERS").is_some()
}

/// `LEGAIA_WORLD_MAP_MARKERS`: set at all.
pub fn env_world_map_markers() -> bool {
    std::env::var_os("LEGAIA_WORLD_MAP_MARKERS").is_some()
}

/// Apply the gameplay switches a native session takes from the process
/// environment. The engine itself reads no environment for a gameplay
/// switch (a `wasm32` build has none, so a switch read there could never be
/// thrown in the browser); this is the native host's half, and the play
/// page's twins are `set_battle_frame_cadence` / `set_arts_saved_list` /
/// `set_smart_monsters` / `set_world_map_markers`, applied together by its
/// `apply_page_toggles`.
/// Every native session open applies it, so a corpus run's child processes
/// inherit the switch. An unset variable leaves the toggle as the world
/// built it; `LEGAIA_BATTLE_FRAME_CADENCE` sets the cadence either way
/// (`0` lowers it), `LEGAIA_ARTS_SAVED_LIST`, `LEGAIA_SMART_MONSTERS` and
/// `LEGAIA_WORLD_MAP_MARKERS` only raise theirs.
pub fn apply_env_world_toggles(toggles: &mut legaia_engine_core::world::WorldToggles) {
    if let Some(on) = env_battle_frame_cadence() {
        toggles.battle_sm_frame_cadence = on;
    }
    if env_arts_saved_list() {
        toggles.arts_saved_list = true;
    }
    // Opt-in, not retail: a monster's single-target attack goes to the
    // lowest-HP living party member instead of a uniform random pick. The
    // RNG stream is unchanged. The page's twin is `set_smart_monsters`.
    if env_smart_monsters() {
        toggles.smarter_monster_targeting = true;
    }
    // The overworld's per-placement entity markers, a port debug overlay
    // (retail draws nothing over a town entrance). The page's twin is
    // `set_world_map_markers`.
    if env_world_map_markers() {
        toggles.overworld_marker_overlay = true;
    }
}

/// Read the raw `SCUS_942.54` bytes from a boot source. Returns `None`
/// (not an error) when the executable isn't reachable.
fn read_scus(source: &SceneSource<'_>) -> Option<Vec<u8>> {
    use legaia_engine_scene::vfs::Vfs;
    match source {
        SceneSource::Extracted(root) => legaia_engine_scene::vfs::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
        SceneSource::Disc(path) => legaia_engine_scene::vfs::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
    }
}

#[cfg(test)]
mod cadence_switch_tests {
    use super::battle_frame_cadence_switch;
    use std::ffi::OsStr;

    /// The switch sets the cadence both ways, and its absence defers to the
    /// engine's default.
    #[test]
    fn the_cadence_switch_raises_and_lowers() {
        assert_eq!(battle_frame_cadence_switch(None), None);
        assert_eq!(
            battle_frame_cadence_switch(Some(OsStr::new("0"))),
            Some(false)
        );
        assert_eq!(
            battle_frame_cadence_switch(Some(OsStr::new("1"))),
            Some(true)
        );
        let mut t = legaia_engine_core::world::WorldToggles::new();
        assert_eq!(
            t.battle_sm_frame_cadence,
            legaia_engine_core::world::DEFAULT_BATTLE_SM_FRAME_CADENCE
        );
        t.battle_sm_frame_cadence = true;
        if let Some(on) = battle_frame_cadence_switch(Some(OsStr::new("0"))) {
            t.battle_sm_frame_cadence = on;
        }
        assert!(!t.battle_sm_frame_cadence);
    }
}
