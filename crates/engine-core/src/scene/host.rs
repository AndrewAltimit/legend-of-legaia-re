//! Scene host: drives the world per tick and routes scene transitions + BGM.

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
    /// The current scene's `.MAP` kind-1 tile-trigger tables
    /// (`(primary, fallback)` - see
    /// [`crate::field_regions::parse_tile_triggers`]), cached at scene load
    /// for the per-frame walk-on dispatch. Empty for scenes without a field
    /// map.
    field_triggers: (
        Vec<crate::field_regions::TileTrigger>,
        Vec<crate::field_regions::TileTrigger>,
    ),
    /// The current scene's MAN payload, cached at scene load so a walk-on
    /// trigger hit can resolve its partition-2 record without a disc re-read.
    field_man_cache: Option<Arc<Vec<u8>>>,
    /// The current scene's paired **scripted gold charges** (op-`0x4E`
    /// gold-gate + negative `0x3A` debit pairs - inn stays, tours, casino
    /// counters), scanned from the cached MAN at scene load via
    /// [`legaia_asset::inn_costs::scan`]. Empty for scenes with no MAN or no
    /// charge site. Drives [`Self::scene_inn_cost`].
    scene_gold_charges: Vec<legaia_asset::inn_costs::GoldCharge>,
    /// Player collision tile at the previous tick - the engine mirror of the
    /// retail last-tile globals `FUN_801D1EC4` compares to fire the walk-on
    /// tile trigger only on a tile **crossing**. `None` = stale (scene entry
    /// / warp arrival), which fires the trigger at the current tile, matching
    /// retail's stale-globals first-frame dispatch.
    last_trigger_tile: Option<(u8, u8)>,
    /// Sustained-SFX voice bookkeeping (retail `gp+0x5D0` held count +
    /// `gp+0x40C` current cue). Released by
    /// [`SceneHost::release_sustained_sfx`] on scene load; see
    /// [`host::sustained_sfx`](self) for the retail provenance.
    pub sustained_sfx: SustainedSfx,
    /// The global mode cell (retail `DAT_80073F20`, a byte). Written by
    /// [`SceneHost::set_mode_cell`]; zero until set (retail BSS default).
    mode_cell: u8,
}

mod audio_dialog;
mod effects;
mod lifecycle;
mod scene_entry;
mod sustained_sfx;

pub use effects::*;
pub use sustained_sfx::{SPU_VOICE_COUNT, SUSTAINED_BASE_VOICE, SustainedSfx};
