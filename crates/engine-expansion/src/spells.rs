//! Pack Seru magic at runtime: the compiled spells, the menu ids they are
//! listed under, the cast in flight, and the per-character magic levels the
//! save carries.
//!
//! A pack spell is cast **as its base** retail spell: the action SM's Magic
//! band, the summon module, the creature and the camera all run on the base
//! id, so nothing in the cast's presentation knows a pack is involved. What
//! the pack changes is read off [`PackCast`] at the seams that take a number:
//! the MP debit, the magnitude fold (power percent, growth, element) and the
//! status rider, and the spell XP, which trains the pack spell's own level
//! rather than the base spell's.
//!
//! The battle and field Magic menus list spells by id, so each pack spell is
//! given a fixed menu id above the spell table ([`MENU_IDS`]) for as long as
//! the pack is active; choosing that row casts the base.

use std::collections::BTreeMap;

use legaia_expansion_format::arts::Rider;
use legaia_expansion_format::items::Character;
use legaia_expansion_format::spells::{SpellDef, SpellLearn, SpellSection, TargetShape};

/// The menu ids pack spells are listed under: above the spell table's last
/// record (`0xBD`) and below `0xFF`, the menus' empty row.
pub const MENU_IDS: std::ops::RangeInclusive<u8> = 0xC0..=0xFE;

/// One compiled pack spell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSpell {
    pub key: String,
    /// The menu id the spell is listed under.
    pub menu_id: u8,
    pub def: SpellDef,
}

impl CompiledSpell {
    pub fn name(&self) -> &str {
        self.def.name.trim()
    }

    pub fn base(&self) -> u8 {
        self.def.base
    }

    pub fn target(&self) -> Option<TargetShape> {
        self.def.target
    }

    pub fn rider(&self) -> Option<Rider> {
        self.def.status
    }

    pub fn learn(&self) -> &SpellLearn {
        &self.def.learn
    }
}

/// A pack's spells, compiled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompiledSpells {
    pub spells: Vec<CompiledSpell>,
}

impl CompiledSpells {
    /// Compile `section`: menu ids in key order; a spell past the id space
    /// or on a base outside the Seru blocks is left out.
    pub fn compile(section: &SpellSection) -> Self {
        let spells = section
            .spells
            .iter()
            .filter(|(_, d)| legaia_expansion_format::spells::BASE_SPELLS.contains(&d.base))
            .zip(MENU_IDS)
            .map(|((key, def), menu_id)| CompiledSpell {
                key: key.clone(),
                menu_id,
                def: def.clone(),
            })
            .collect();
        Self { spells }
    }

    pub fn is_empty(&self) -> bool {
        self.spells.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&CompiledSpell> {
        self.spells.iter().find(|s| s.key == key)
    }

    pub fn by_menu_id(&self, id: u8) -> Option<&CompiledSpell> {
        self.spells.iter().find(|s| s.menu_id == id)
    }

    /// The spells a capture of pack monster `monster` (local key) teaches.
    pub fn taught_by_capture<'a>(
        &'a self,
        pack_id: &'a str,
        monster: &'a str,
    ) -> impl Iterator<Item = &'a CompiledSpell> + 'a {
        self.spells.iter().filter(move |s| {
            s.def
                .learn
                .capture
                .as_deref()
                .and_then(|c| legaia_expansion_format::battle::local_key(pack_id, c))
                .is_some_and(|k| k == monster)
        })
    }
}

/// A pack spell's magic level and XP on one character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellLevel {
    pub level: u8,
    pub xp: u32,
}

impl Default for SpellLevel {
    fn default() -> Self {
        Self { level: 1, xp: 0 }
    }
}

/// The pack spells each character knows: `(roster slot, key)` -> level.
/// Saved in tail record `5` of the expansion block with the learned arts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellProgress {
    pub known: BTreeMap<(u8, String), SpellLevel>,
}

impl SpellProgress {
    pub fn knows(&self, roster: u8, key: &str) -> bool {
        self.known.contains_key(&(roster, key.to_string()))
    }

    pub fn level(&self, roster: u8, key: &str) -> Option<SpellLevel> {
        self.known.get(&(roster, key.to_string())).copied()
    }

    /// Teach `key` to `roster`; `false` when already known.
    pub fn learn(&mut self, roster: u8, key: &str) -> bool {
        let k = (roster, key.to_string());
        if self.known.contains_key(&k) {
            return false;
        }
        self.known.insert(k, SpellLevel::default());
        true
    }

    /// Bank `gain` XP on `roster`'s `key` and level it with `levels_up(level,
    /// xp)` - retail's threshold test for the base spell
    /// (`battle_formulas::summon_magic_levels_up`, `FUN_801E70BC`). Returns
    /// the new level on a level-up.
    pub fn accrue(
        &mut self,
        roster: u8,
        key: &str,
        gain: u32,
        levels_up: impl Fn(u8, u32) -> bool,
    ) -> Option<u8> {
        let e = self.known.get_mut(&(roster, key.to_string()))?;
        e.xp = e.xp.saturating_add(gain);
        if e.level >= legaia_expansion_format::spells::MAX_LEVEL || !levels_up(e.level, e.xp) {
            return None;
        }
        e.level += 1;
        Some(e.level)
    }

    /// The menu ids of the spells `roster` knows, in key order.
    pub fn menu_ids(&self, spells: &CompiledSpells, roster: u8) -> Vec<u8> {
        spells
            .spells
            .iter()
            .filter(|s| self.knows(roster, &s.key))
            .map(|s| s.menu_id)
            .collect()
    }
}

/// The pack cast in flight: set when a pack spell's row is cast, cleared at
/// the band's end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackCast {
    /// The casting battle ordinal.
    pub caster: u8,
    /// The caster's roster slot.
    pub roster: u8,
    /// The spell's index in [`CompiledSpells::spells`].
    pub spell: usize,
    /// The magic level the cast rolls at.
    pub level: u8,
    /// The spell id the cast runs as: the base, or for a spell widened to
    /// every enemy on a one-enemy base, the all-enemies Seru it presents as
    /// (`World::expansion_begin_pack_cast`).
    pub cast_id: u8,
}

/// The spell-side state of an active pack.
#[derive(Debug, Clone, Default)]
pub struct SpellRuntime {
    pub compiled: Option<std::sync::Arc<CompiledSpells>>,
    pub casting: Option<PackCast>,
}

impl SpellRuntime {
    pub fn spells(&self) -> Option<&CompiledSpells> {
        self.compiled.as_deref().filter(|c| !c.is_empty())
    }

    /// The cast in flight, with its spell.
    pub fn cast(&self) -> Option<(&PackCast, &CompiledSpell)> {
        let c = self.casting.as_ref()?;
        Some((c, self.spells()?.spells.get(c.spell)?))
    }
}

/// Scale a rolled magnitude by a pack spell's power at `level`, capped at
/// retail's 9999.
pub fn scale_magnitude(amount: u16, def: &SpellDef, level: u8) -> u16 {
    (u32::from(amount) * def.power_at(level) / 100).min(9999) as u16
}

/// The roster slot of a pack character (`0` Vahn, `1` Noa, `2` Gala).
pub fn character_of(roster: u8) -> Option<Character> {
    match roster {
        0 => Some(Character::Vahn),
        1 => Some(Character::Noa),
        2 => Some(Character::Gala),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section() -> SpellSection {
        toml::from_str(
            r#"
            [spells.wyrm]
            name = "Cinder Wyrm"
            base = 0x81
            mp = 34
            power = 180
            growth = 10
            learn = { capture = "ember_drake" }

            [spells.aegis]
            name = "Aegis"
            base = 0x83
            learn = { level = 20 }

            [spells.bad]
            name = "Bad"
            base = 0x10
            "#,
        )
        .unwrap()
    }

    #[test]
    fn menu_ids_and_capture_lookup() {
        let c = CompiledSpells::compile(&section());
        assert_eq!(
            c.spells.len(),
            2,
            "a base outside the Seru blocks is left out"
        );
        assert_eq!(c.get("aegis").unwrap().menu_id, 0xC0);
        assert_eq!(c.get("wyrm").unwrap().menu_id, 0xC1);
        let t: Vec<_> = c
            .taught_by_capture("rb", "ember_drake")
            .map(|s| &s.key)
            .collect();
        assert_eq!(t, ["wyrm"]);
        assert_eq!(c.taught_by_capture("rb", "rb:ember_drake").count(), 0);
    }

    #[test]
    fn levels_accrue_on_retail_thresholds() {
        let c = CompiledSpells::compile(&section());
        let mut p = SpellProgress::default();
        assert!(p.learn(1, "wyrm"));
        assert!(!p.learn(1, "wyrm"));
        let table = [17u32, 50, 92, 144, 208, 288, 392, 536];
        let up = |level: u8, xp: u32| xp > table[usize::from(level - 1)];
        assert_eq!(p.accrue(1, "wyrm", 12, up), None);
        assert_eq!(p.accrue(1, "wyrm", 12, up), Some(2));
        assert_eq!(p.level(1, "wyrm").unwrap(), SpellLevel { level: 2, xp: 24 });
        assert_eq!(p.menu_ids(&c, 1), [0xC1]);
        let w = &c.get("wyrm").unwrap().def;
        assert_eq!(scale_magnitude(1000, w, 1), 1800);
        assert_eq!(scale_magnitude(1000, w, 3), 2000);
        assert_eq!(scale_magnitude(9000, w, 3), 9999);
    }
}
