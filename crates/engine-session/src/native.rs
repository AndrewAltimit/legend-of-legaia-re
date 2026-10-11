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
use legaia_engine_core::scene_host::SceneHost;

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
            // The cast voices (seventeen files) are not decoded here: the
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
