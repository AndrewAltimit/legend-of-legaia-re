//! The opponent ladder: secret-opponent gates, stage advance, rung fold and the match bonus.

use super::*;

/// The secret-opponent override (`DAT_801DBF06`): which of the two bonus
/// rungs the cabinet is about to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretOpponent {
    /// None pending - the rung comes from the stage counter.
    None,
    /// The stage-5 rung (roster id `3`).
    First,
    /// The stage-13 rung (roster id `4`).
    Second,
}

impl SecretOpponent {
    /// Raw override value the overlay stores.
    pub fn raw(self) -> i16 {
        match self {
            Self::None => 0,
            Self::First => 1,
            Self::Second => 2,
        }
    }

    /// Decode the raw override value the overlay stores (anything outside
    /// `1..=2` is "none", which is how retail's own `!= 0` / `== 2` tests read
    /// it).
    pub fn from_raw(v: i16) -> Self {
        match v {
            1 => Self::First,
            2 => Self::Second,
            _ => Self::None,
        }
    }
}

/// PORT: FUN_801cf388 (`0x801D06FC`..`0x801D0798`) - the **secret-opponent
/// score gate**.
///
/// After a match win, before the stage counter advances, the cabinet tests the
/// rung it just cleared against the running high score. Stage
/// [`SECRET_A_STAGE`] past [`SECRET_A_SCORE`] arms [`SecretOpponent::First`];
/// stage [`SECRET_B_STAGE`] past [`SECRET_B_SCORE`] arms
/// [`SecretOpponent::Second`]. An armed override *suppresses* the stage
/// advance, so the bonus rung is inserted into the ladder rather than
/// replacing one - which is the route roster ids `3` and `4` are reached by
/// without a full lap of the counter.
///
/// The comparisons are strict (`slt`): a score exactly on the threshold does
/// not qualify.
pub fn secret_opponent_gate(stage: i32, high_score: i32) -> SecretOpponent {
    if stage == SECRET_A_STAGE && high_score > SECRET_A_SCORE {
        SecretOpponent::First
    } else if stage == SECRET_B_STAGE && high_score > SECRET_B_SCORE {
        SecretOpponent::Second
    } else {
        SecretOpponent::None
    }
}

/// Outcome of advancing the ladder's stage counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageAdvance {
    /// Stage counter after the step.
    pub stage: i32,
    /// `true` when the counter reached [`STAGE_WRAP`] and wrapped to `0`,
    /// which is the ladder's all-clear.
    pub all_clear: bool,
}

/// PORT: FUN_801cf388 (`0x801D0808`..`0x801D083C`) - the stage advance.
///
/// The counter only steps when no secret rung is pending. Reaching
/// [`STAGE_WRAP`] raises the all-clear flag and wraps the counter to `0`, so
/// a second lap starts at roster id [`ROSTER_FROM_STAGE`].
pub fn advance_stage(stage: i32, secret: SecretOpponent) -> StageAdvance {
    let stage = if secret == SecretOpponent::None {
        stage + 1
    } else {
        stage
    };
    // Retail tests `(u32)stage < 0xE`, so a negative stage folds to the wrap
    // arm as well.
    if (stage as u32) < STAGE_WRAP as u32 {
        StageAdvance {
            stage,
            all_clear: false,
        }
    } else {
        StageAdvance {
            stage: 0,
            all_clear: true,
        }
    }
}

/// PORT: FUN_801cf388 (`0x801CFD14`..`0x801CFD4C`) - the rung fold.
///
/// Which roster record and which mesh index the opponent-install state loads.
/// With no secret rung pending the stage counter supplies both
/// (`stage + 3` / `stage + 5`); an armed override replaces them with
/// `override + 2` / `override + 4`, which is how roster ids `3` and `4` pair
/// with mesh indices `5` and `6`.
pub fn rung_fold(stage: i32, secret: SecretOpponent) -> (i32, i32) {
    match secret {
        SecretOpponent::None => (stage + ROSTER_FROM_STAGE, stage + MESH_FROM_STAGE),
        other => {
            let sel = other.raw() as i32;
            (sel + 2, sel + 4)
        }
    }
}

/// PORT: FUN_801cf388 (`0x801D0844`..`0x801D085C`) - the match bonus row.
///
/// The tally's first score row is seeded [`PERFECT_BONUS`] when the opponent
/// won no round at all, else [`MATCH_BONUS`].
pub fn match_bonus(opponent_round_wins: u32) -> i32 {
    if opponent_round_wins == 0 {
        PERFECT_BONUS
    } else {
        MATCH_BONUS
    }
}
