//! [`ModeTraceFrame`]: one sample of a session's high-level dispatch state -
//! the mode word, the scene mode and the loaded scene. A replay file's
//! expected frames and the mode-trace oracle's retail samples are both
//! compared against it.

use serde::{Deserialize, Serialize};

/// One sample of the engine's (or retail's) high-level dispatch state.
/// A field the sampler cannot fill is left as `None` rather than zeroed, so a
/// downstream diff tool can tell "didn't observe" from "observed 0".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeTraceFrame {
    /// Frame counter. Engine: wall-clock frame from
    /// [`BootSession::frames`](crate::BootSession::frames). Retail snapshot: always 0 (save states
    /// are single-frame).
    pub frame: u64,
    /// `_DAT_8007B83C` byte (an index into the 28-mode table). Retail:
    /// read directly. Engine: the session's
    /// [`ModeSeat`](legaia_engine_mode::mode::ModeSeat) word, on every frame.
    /// `None` only for a sampler that holds no seat.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub game_mode: Option<u8>,
    /// Debug name from `mode::TABLE` (e.g. `"MAPDISP MODE"`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub game_mode_name: Option<String>,
    /// [`SceneMode`](legaia_engine_mode::mode::SceneMode) variant name:
    /// `"Title"` / `"Field"` / `"Battle"` / `"Cutscene"` / `"WorldMap"` /
    /// `"Menu"` / the five minigames. Both emitters fill this. Retail derives
    /// it from the `(game_mode, warp sub-id)` pair via
    /// [`GameMode::scene_mode_with_warp`](legaia_engine_mode::mode::GameMode::scene_mode_with_warp),
    /// because the mode word alone cannot separate the five minigames: they all
    /// run under `0x19`.
    pub scene_mode: String,
    /// CDNAME label of the currently-loaded scene, e.g. `"town01"`.
    /// Engine reads from `host.scene.as_ref().map(|s| s.name)`. Retail
    /// reads from the scene-bundle pool at `0x80084540` slot 0 via
    /// `legaia_parity::capture_observations::field_pack_intra_transition::read_pool_slot_name`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub active_scene: Option<String>,
}
