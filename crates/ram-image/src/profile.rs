//! What the generic readers need to be told about the game in a save state.
//!
//! Locating, slicing, diffing and bisecting a 2 MiB PSX main-RAM image needs
//! no knowledge of the game that filled it. Three things do, and they are the
//! parameters a [`GameProfile`] carries:
//!
//! - **Anchor strings** for the fallback RAM search
//!   ([`crate::extract::main_ram_via_exe_anchors`]): byte strings that sit in
//!   the executable's loaded image, matched in both the executable file and
//!   the state payload to derive where RAM starts. Only needed for a state
//!   whose container cannot be walked structurally.
//! - **State identity**: which scene / mode / player position a state is in,
//!   read from the game's own globals.
//! - **Scenario-manifest defaults**: the mednafen save-file name pattern and
//!   the environment variable naming the `mcs/` directory, used when the
//!   manifest itself does not set them.
//!
//! [`GenericGame`] is the profile for a title nobody has described: no
//! anchors, no identity, mednafen's own naming left to the manifest. The
//! Legend of Legaia profile is [`crate::game_anchors::Legaia`], next to the
//! globals it reads; every API that predates profiles uses it, so existing
//! callers see no change.

use crate::game_anchors::StateIdentity;

/// Per-game parameters for the otherwise game-agnostic readers.
pub trait GameProfile {
    /// Short human name (`"legaia"`, `"generic"`), for diagnostics.
    fn name(&self) -> &str;

    /// Strings resident in the executable's loaded image, tried in order by
    /// the anchor RAM search. Empty means the anchor path is unavailable and
    /// only a structural container walk can locate RAM.
    fn exe_anchor_strings(&self) -> &[&'static [u8]] {
        &[]
    }

    /// Identify a state from its main RAM, or `None` when the profile knows
    /// no globals to read.
    fn identify(&self, _ram: &[u8]) -> Option<StateIdentity> {
        None
    }

    /// Human label for the game's mode byte (`StateIdentity::game_mode`).
    fn mode_label(&self, mode: u8) -> String {
        format!("mode_{mode:02X}")
    }

    /// Defaults the scenario manifest falls back to when its `[defaults]`
    /// table leaves a field out.
    fn scenario_defaults(&self) -> ScenarioProfile {
        ScenarioProfile::GENERIC
    }
}

/// Scenario-manifest defaults a [`GameProfile`] supplies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioProfile {
    /// Mednafen save-state file name; `{slot}` is replaced by the slot index.
    pub filename_pattern: &'static str,
    /// Environment variable naming the mednafen `mcs/` directory, consulted
    /// after the manifest's own `mcs_dir` and before `~/.mednafen/mcs/`.
    pub mcs_dir_env: Option<&'static str>,
}

impl ScenarioProfile {
    /// No game-specific naming: the manifest must set `filename_pattern`
    /// unless the states really are called `state.mc{slot}`.
    pub const GENERIC: ScenarioProfile = ScenarioProfile {
        filename_pattern: "state.mc{slot}",
        mcs_dir_env: None,
    };
}

/// The profile for a game this crate knows nothing about.
#[derive(Debug, Clone, Copy, Default)]
pub struct GenericGame;

impl GameProfile for GenericGame {
    fn name(&self) -> &str {
        "generic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_profile_knows_nothing() {
        let g = GenericGame;
        assert!(g.exe_anchor_strings().is_empty());
        assert_eq!(g.identify(&[0u8; 16]), None);
        assert_eq!(g.mode_label(0x15), "mode_15");
        assert_eq!(g.scenario_defaults(), ScenarioProfile::GENERIC);
    }
}
