//! A capture gate on the evolved Cort's arrival columns
//! (`LEGAIA_CAPTURE_ARRIVAL`): a round-prompt capture of that fight is dated
//! by the PROT 0968 drop's crystal columns, which outlive the hand-back by
//! their own program (a wait, a depth-cue ramp toward black, a halt). The
//! retail state's column level places the frame where the fixed settle tick
//! cannot: `cort_evolved_card_first_menu` holds the column ten vsyncs into
//! the prompt, early in its fade, and a later capture holds none.

use legaia_engine_battle::battle_stage_module::records;
use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;

/// Ticks the image child's deadline gains under the gate: the columns live
/// about a hundred ticks past the hand-back.
pub const ARRIVAL_GATE_SLACK: u64 = 240;

/// Where in the arrival columns' life the capture stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrivalGate {
    /// The drop's first column is up with its depth cue `+0x78` at least
    /// this far along.
    Cue(u16),
    /// No column is up any more.
    Gone,
}

impl ArrivalGate {
    /// Parse `cue:<level>` or `gone`.
    pub fn from_env(v: &str) -> Option<Self> {
        let v = v.trim();
        if v == "gone" {
            return Some(Self::Gone);
        }
        v.strip_prefix("cue:")?.trim().parse().ok().map(Self::Cue)
    }

    /// Whether the engine stands where the capture did: past the arrival's
    /// hand-back (stage id `0`, the round prompt open), on the column's
    /// level or past its life.
    pub fn met(self, world: &World) -> bool {
        if world.mode != SceneMode::Battle
            || world.battle_stage_id() != 0
            || world.battle.command.is_none()
        {
            return false;
        }
        let column = world
            .casting
            .stage_fx
            .iter()
            .flat_map(|s| s.parts.iter())
            .find(|p| !p.finished && p.tag == Some(records::ARRIVAL_DROP[0]));
        match self {
            Self::Cue(level) => column.is_some_and(|p| p.state.field_78 >= level),
            Self::Gone => column.is_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env_form_round_trips() {
        assert_eq!(ArrivalGate::from_env("gone"), Some(ArrivalGate::Gone));
        assert_eq!(
            ArrivalGate::from_env("cue:333"),
            Some(ArrivalGate::Cue(333))
        );
        assert_eq!(ArrivalGate::from_env("cue:x"), None);
    }
}
