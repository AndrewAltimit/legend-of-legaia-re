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

mod audio_dialog;
mod effects;
mod lifecycle;
mod scene_entry;

pub use effects::*;
