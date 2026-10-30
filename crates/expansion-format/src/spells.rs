//! `spells.toml`: new Seru magic.
//!
//! ```toml
//! [spells.cinder_wyrm]
//! name = "Cinder Wyrm"
//! description = "A wyrm of tower embers."
//! base = 0x81            # the retail cast whose summon plays (Gimard)
//! mp = 34
//! power = 180            # percent of the base cast's damage roll
//! growth = 6             # +6% per magic level above 1
//! element = 0            # strikes as fire
//! target = "all_enemies"
//! status = { status = "venom", chance = 25 }
//! learn = { capture = "ember_drake" }
//! ```
//!
//! A pack spell is cast **as its base**: the retail cast module, its
//! creature, camera and effects play unchanged, and the pack changes the
//! numbers around it - name, MP, the damage or heal magnitude, element,
//! target shape, a status rider - and keeps its own magic level. It is
//! learned by capturing a pack monster (`learn.capture`) or by the shared
//! learn conditions. The engine side is `legaia_engine_expansion::spells`;
//! see `docs/formats/expansion-pack-arts.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::arts::{Learn, Rider, check_learn, check_name, check_rider};
use crate::items::Character;

/// The retail spell ids a pack spell may borrow: the player Seru blocks,
/// whose casts run a slot-B summon module (PROT `0903..`).
pub const BASE_SPELLS: std::ops::RangeInclusive<u8> = 0x81..=0xA0;
/// Highest `power` percent.
pub const MAX_POWER_PCT: u16 = 500;
/// The magic level cap retail's threshold table reaches.
pub const MAX_LEVEL: u8 = 9;
/// Pack spells one pack may define: the battle menu lends each one an id
/// from `0xC0..=0xFE`, above the spell table's last record (`0xBD`).
pub const MAX_SPELLS: usize = 63;

/// The `spells.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpellSection {
    /// Pack spells by key.
    pub spells: BTreeMap<String, SpellDef>,
}

/// Who a spell lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetShape {
    OneEnemy,
    AllEnemies,
    OneAlly,
    AllAllies,
}

/// One pack spell.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpellDef {
    /// The Magic menu's name.
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The retail spell id whose cast plays ([`BASE_SPELLS`]).
    pub base: u8,
    /// MP cost; the base spell's when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mp: Option<u8>,
    /// Percent of the base cast's damage (or heal) roll.
    #[serde(default = "pct_100", skip_serializing_if = "is_100")]
    pub power: u16,
    /// Extra percent per magic level above 1.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub growth: u16,
    /// The element the cast strikes with (`0..=7`), in place of the summon
    /// creature's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<u8>,
    /// The target shape, in place of the base spell's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetShape>,
    /// A status each struck target may take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Rider>,
    /// Who can learn it; empty is everyone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub characters: Vec<Character>,
    /// How it is learned.
    #[serde(default, skip_serializing_if = "SpellLearn::is_empty")]
    pub learn: SpellLearn,
}

fn pct_100() -> u16 {
    100
}

fn is_100(v: &u16) -> bool {
    *v == 100
}

fn is_zero(v: &u16) -> bool {
    *v == 0
}

impl SpellDef {
    /// Whether `character` may learn the spell.
    pub fn learnable_by(&self, character: Character) -> bool {
        self.characters.is_empty() || self.characters.contains(&character)
    }

    /// The damage / heal percent at magic level `level` (`1..`).
    pub fn power_at(&self, level: u8) -> u32 {
        u32::from(self.power) + u32::from(self.growth) * u32::from(level.max(1) - 1)
    }
}

/// How a pack spell is learned.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpellLearn {
    /// The pack monster (`battle.toml`) whose capture teaches it, the way a
    /// retail Seru's capture teaches its spell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture: Option<String>,
    /// The caster's own level must be at least this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// The pack gate (quests, flags, bosses, party level).
    #[serde(skip_serializing_if = "crate::quest::Prerequisites::is_empty")]
    pub requires: crate::quest::Prerequisites,
    /// An item the party must hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<crate::items::ItemRef>,
}

impl SpellLearn {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The conditions besides the capture: with no capture they teach the
    /// spell, with one they gate it.
    pub fn when(&self) -> Learn {
        Learn {
            level: self.level,
            requires: self.requires.clone(),
            item: self.item.clone(),
        }
    }
}

impl crate::validate::SectionRules for SpellSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for key in self.spells.keys() {
            cx.declare("spell", key, &format!("spells.{key}"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        if self.spells.len() > MAX_SPELLS {
            cx.error(
                "spells",
                format!("at most {MAX_SPELLS} pack spells (the menu's lent id space)"),
            );
        }
        let mut captures: BTreeMap<&str, &str> = BTreeMap::new();
        for (key, s) in &self.spells {
            let at = format!("spells.{key}");
            check_name(cx, &at, &s.name);
            if !s.description.is_ascii() {
                cx.warning(&at, "the dialog font draws ASCII only");
            }
            if !BASE_SPELLS.contains(&s.base) {
                cx.error(
                    &format!("{at}.base"),
                    format!(
                        "base 0x{:02X} is not a player Seru spell (0x{:02X}..=0x{:02X})",
                        s.base,
                        BASE_SPELLS.start(),
                        BASE_SPELLS.end()
                    ),
                );
            }
            if s.power == 0 || s.power > MAX_POWER_PCT {
                cx.error(
                    &format!("{at}.power"),
                    format!("power is 1..={MAX_POWER_PCT} percent"),
                );
            }
            if s.power_at(MAX_LEVEL) > 999 {
                cx.warning(
                    &format!("{at}.growth"),
                    "power passes 999% by magic level 9",
                );
            }
            if s.element.is_some_and(|e| e > 7) {
                cx.error(&format!("{at}.element"), "element must be 0..=7");
            }
            if let Some(r) = &s.status {
                check_rider(cx, &format!("{at}.status"), r);
            }
            if let Some(m) = &s.learn.capture {
                cx.reference("monster", m, &format!("{at}.learn.capture"));
                if let Some(prev) = captures.insert(m.as_str(), key.as_str()) {
                    cx.warning(
                        &format!("{at}.learn.capture"),
                        format!("spell {prev:?} is taught by the same capture; both are learned"),
                    );
                }
            }
            check_learn(cx, &format!("{at}.learn"), &s.learn.when());
            if s.learn.is_empty() {
                cx.warning(&format!("{at}.learn"), "known from the start");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_shape_and_growth() {
        let s: SpellSection = toml::from_str(
            r#"
            [spells.wyrm]
            name = "Cinder Wyrm"
            base = 0x81
            mp = 34
            power = 180
            growth = 6
            target = "all_enemies"
            status = { status = "venom", chance = 25 }
            learn = { capture = "ember_drake", level = 20 }
            "#,
        )
        .unwrap();
        let w = &s.spells["wyrm"];
        assert_eq!(w.power_at(1), 180);
        assert_eq!(w.power_at(4), 198);
        assert_eq!(w.learn.capture.as_deref(), Some("ember_drake"));
        assert_eq!(w.learn.when().level, Some(20));
        assert_eq!(w.target, Some(TargetShape::AllEnemies));
        let back: SpellSection = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
