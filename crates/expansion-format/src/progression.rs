//! `progression.toml`: level curve, XP and rewards to level 99.
//!
//! ```toml
//! xp_mul = 1.5                 # every battle's EXP
//! gold_mul = 1.25              # every battle's gold
//!
//! [xp_curve]                   # the XP needed per level
//! kind = "banded"
//! bands = [{ from_level = 60, mul = 0.4 }]
//!
//! [enemy_scaling]              # enemies grow with the party
//! from_level = 40
//! per_level = 0.03
//! max_mul = 3.0
//!
//! [[level_rewards]]
//! level = 99
//! items = [{ item = { kind = "item", id = 100 } }]
//! ```

use serde::{Deserialize, Serialize};

use crate::battle::ItemReward;
use crate::refs::DiscRef;

/// The highest level a character reaches.
pub const MAX_LEVEL: u8 = 99;

/// The cumulative-XP ceiling the character record holds (`9,999,999`).
pub const XP_CAP: u32 = 9_999_999;

/// The `progression.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressionSection {
    /// Multiplier on every battle's summed EXP; `1.0` when left out.
    #[serde(default)]
    pub xp_mul: Option<f64>,
    /// Multiplier on every battle's gold; `1.0` when left out.
    #[serde(default)]
    pub gold_mul: Option<f64>,
    /// The XP curve; retail's when left out.
    #[serde(default)]
    pub xp_curve: XpCurve,
    /// Enemy stats scaled to the party's level; off when left out.
    #[serde(default)]
    pub enemy_scaling: Option<EnemyScaling>,
    /// Items granted when a character reaches a level.
    #[serde(default)]
    pub level_rewards: Vec<LevelReward>,
}

/// The cumulative XP a character needs for each level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum XpCurve {
    /// Retail's curve.
    #[default]
    Retail,
    /// Retail's curve times `mul` at every level.
    Scaled { mul: f64 },
    /// Retail's per-level XP steps scaled band by band: each band's `mul`
    /// applies to the step into every level from its `from_level` up to the
    /// next band. Levels below the first band keep retail's steps.
    Banded { bands: Vec<XpBand> },
    /// The curve outright: `levels[i]` is the cumulative XP to reach level
    /// `i + 2`, 98 strictly rising values no higher than [`XP_CAP`].
    Explicit { levels: Vec<u32> },
}

/// One band of [`XpCurve::Banded`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XpBand {
    /// The first level whose step this band scales (`2..=99`).
    pub from_level: u8,
    pub mul: f64,
}

/// Which battles [`EnemyScaling`] applies to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalingScope {
    /// The pack's own formations only.
    #[default]
    Pack,
    /// Every battle, retail ones too.
    All,
}

/// Enemies scale to the party's level: past `from_level`, every level of the
/// strongest present member adds `per_level` to a stat multiplier that tops
/// out at `max_mul`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyScaling {
    #[serde(default = "default_from_level")]
    pub from_level: u8,
    pub per_level: f64,
    #[serde(default = "default_max_mul")]
    pub max_mul: f64,
    #[serde(default)]
    pub scope: ScalingScope,
    /// Scale EXP and gold with the stats.
    #[serde(default)]
    pub rewards: bool,
}

fn default_from_level() -> u8 {
    1
}

fn default_max_mul() -> f64 {
    4.0
}

/// Items granted when a character first reaches `level`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelReward {
    pub level: u8,
    pub items: Vec<ItemReward>,
    /// Only this character (roster slot: `0` Vahn, `1` Noa, `2` Gala); every
    /// character that reaches the level when left out.
    #[serde(default)]
    pub character: Option<u8>,
}

fn bad_mul(m: f64) -> bool {
    !m.is_finite() || m <= 0.0
}

impl ProgressionSection {
    /// Every problem a pack author must fix, one line each.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (what, m) in [("xp_mul", self.xp_mul), ("gold_mul", self.gold_mul)] {
            if m.is_some_and(bad_mul) {
                out.push(format!("{what} must be a positive number"));
            }
        }
        match &self.xp_curve {
            XpCurve::Retail => {}
            XpCurve::Scaled { mul } => {
                if bad_mul(*mul) {
                    out.push("xp_curve: mul must be a positive number".into());
                }
            }
            XpCurve::Banded { bands } => {
                let mut prev = 1u8;
                for b in bands {
                    if b.from_level <= prev || b.from_level > MAX_LEVEL {
                        out.push(format!(
                            "xp_curve: band from_level {} must rise within 2..=99",
                            b.from_level
                        ));
                    }
                    prev = prev.max(b.from_level);
                    if bad_mul(b.mul) {
                        out.push(format!(
                            "xp_curve: band at level {} needs a positive mul",
                            b.from_level
                        ));
                    }
                }
            }
            XpCurve::Explicit { levels } => {
                if levels.len() != usize::from(MAX_LEVEL) - 1 {
                    out.push(format!(
                        "xp_curve: explicit curve has {} levels, needs 98",
                        levels.len()
                    ));
                }
                if levels.windows(2).any(|w| w[1] <= w[0]) || levels.first() == Some(&0) {
                    out.push("xp_curve: explicit curve must rise strictly from above 0".into());
                }
                if levels.iter().any(|&x| x > XP_CAP) {
                    out.push(format!("xp_curve: explicit curve passes {XP_CAP}"));
                }
            }
        }
        if let Some(s) = &self.enemy_scaling {
            if !s.per_level.is_finite() || s.per_level < 0.0 {
                out.push("enemy_scaling: per_level must be zero or more".into());
            }
            if bad_mul(s.max_mul) || s.max_mul < 1.0 {
                out.push("enemy_scaling: max_mul must be at least 1".into());
            }
        }
        for r in &self.level_rewards {
            if !(2..=MAX_LEVEL).contains(&r.level) {
                out.push(format!("level_rewards: level {} is not 2..=99", r.level));
            }
            if r.items
                .iter()
                .any(|i| !matches!(i.item, DiscRef::Item { .. }))
            {
                out.push(format!("level_rewards: level {} names a non-item", r.level));
            }
            if r.character.is_some_and(|c| c > 3) {
                out.push(format!(
                    "level_rewards: level {} character is not 0..=3",
                    r.level
                ));
            }
        }
        out
    }
}

impl crate::validate::SectionRules for ProgressionSection {
    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        for p in self.problems() {
            cx.error("", p);
        }
        for (i, r) in self.level_rewards.iter().enumerate() {
            for (k, item) in r.items.iter().enumerate() {
                cx.disc_ref(&format!("level_rewards[{i}].items[{k}]"), &item.item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_doc_sample_parses_clean() {
        let s: ProgressionSection = toml::from_str(
            r#"
xp_mul = 1.5
gold_mul = 1.25
[xp_curve]
kind = "banded"
bands = [{ from_level = 60, mul = 0.4 }]
[enemy_scaling]
from_level = 40
per_level = 0.03
max_mul = 3.0
[[level_rewards]]
level = 99
items = [{ item = { kind = "item", id = 100 } }]
"#,
        )
        .unwrap();
        assert_eq!(s.xp_mul, Some(1.5));
        assert!(matches!(s.xp_curve, XpCurve::Banded { ref bands } if bands[0].from_level == 60));
        let sc = s.enemy_scaling.as_ref().unwrap();
        assert_eq!(sc.scope, ScalingScope::Pack);
        assert!(!sc.rewards);
        assert_eq!(s.problems(), Vec::<String>::new());
    }

    #[test]
    fn an_empty_file_is_retail() {
        let s: ProgressionSection = toml::from_str("").unwrap();
        assert_eq!(s, ProgressionSection::default());
        assert_eq!(s.xp_curve, XpCurve::Retail);
    }

    #[test]
    fn a_short_explicit_curve_is_a_problem() {
        let s = ProgressionSection {
            xp_curve: XpCurve::Explicit {
                levels: vec![10, 5],
            },
            ..Default::default()
        };
        let p = s.problems();
        assert!(p.iter().any(|l| l.contains("needs 98")), "{p:?}");
        assert!(p.iter().any(|l| l.contains("rise strictly")), "{p:?}");
    }
}
