//! `battle.toml`: monsters, formations, encounter overrides and bosses.
//!
//! A pack never carries a monster of its own making from scratch: every pack
//! monster is **derived** from a retail one ([`MonsterDef::base`], a
//! [`DiscRef::Monster`]) and keeps that monster's mesh, animation, AI action
//! set and sounds, which the engine reads off the player's disc. What the pack
//! changes is the numbers: name, HP, stats, rewards, drop and steal, element
//! and the spells the AI may cast.
//!
//! ```toml
//! [monsters.ashen_caruban]
//! base = { kind = "monster", id = 73 }
//! name = "Ashen Caruban"
//! hp = 30000                 # an absolute value...
//! attack = { mul = 1.8 }     # ...or a multiplier of the base monster's
//! exp = { mul = 12.0 }
//!
//! [formations.ash_court]
//! monsters = ["ashen_caruban", { kind = "monster", id = 12 }]
//!
//! [[encounters]]
//! scene = "rikuroa"
//! formations = [{ formation = "ash_court", weight = 1 }]
//!
//! [bosses.ash_king]
//! formation = "ash_court"
//! music = { kind = "bgm", id = 30 }
//! ```
//!
//! Keys (`ashen_caruban`, `ash_court`) are local to the pack; the engine
//! qualifies each one with the pack id (`rebirth:ashen_caruban`, see
//! [`qualify`]). A reference may use either form.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::refs::DiscRef;

/// Monster slots one formation may fill.
///
/// The battle actor table has eight seats: the party takes the first three
/// and seat seven is the summon seat every Seru cast poses its creature on,
/// which leaves four monster seats beside a full party. Retail's own largest
/// formations are four monsters, so four is both the retail and the engine
/// limit; a fifth monster would share the summon seat.
pub const MAX_FORMATION_SLOTS: usize = 4;

/// Waves one formation may list after its first.
pub const MAX_WAVES: usize = 16;

/// Arrivals one refill wave may send ([`WaveDef::count`]).
pub const MAX_WAVE_ARRIVALS: u16 = 64;

/// The `battle.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattleSection {
    /// Derived monsters by local key.
    #[serde(default)]
    pub monsters: BTreeMap<String, MonsterDef>,
    /// Formations by local key.
    #[serde(default)]
    pub formations: BTreeMap<String, FormationDef>,
    /// Per-scene random-encounter overrides.
    #[serde(default)]
    pub encounters: Vec<EncounterOverride>,
    /// Named bosses by local key.
    #[serde(default)]
    pub bosses: BTreeMap<String, BossDef>,
}

/// A stat a pack monster sets outright or scales from its base monster.
///
/// In TOML a bare integer is [`Stat::Set`] and `{ mul = 1.5 }` is
/// [`Stat::Scale`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Stat {
    /// The value itself.
    Set(u32),
    /// The base monster's value times `mul`.
    Scale { mul: f64 },
}

impl Stat {
    /// The multiplier in thousandths, rounded (`{ mul = 1.5 }` -> `1500`);
    /// `None` for a [`Stat::Set`]. A negative or non-finite `mul` reads as
    /// `0`.
    pub fn permille(&self) -> Option<u32> {
        match *self {
            Stat::Set(_) => None,
            Stat::Scale { mul } if mul.is_finite() && mul > 0.0 => {
                Some((mul * 1000.0).round().min(u32::MAX as f64) as u32)
            }
            Stat::Scale { .. } => Some(0),
        }
    }
}

/// One item a monster drops or a thief steals, with its chance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemChance {
    /// A [`DiscRef::Item`].
    pub item: DiscRef,
    /// Percent, `0..=100`.
    pub chance_pct: u8,
}

/// A pack monster: a retail monster with some numbers changed. Every field
/// left out keeps the base monster's value.
///
/// The six battle stats are the monster **record** stats, the values the
/// battle loader then boosts by the fight's class (a scripted fight installs
/// `x2` defence and `x5/4` attack, a random encounter `x7/4` defence) exactly
/// as it does for a retail record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonsterDef {
    /// The retail monster this one is built on (a [`DiscRef::Monster`]).
    pub base: DiscRef,
    /// The name the battle HUD shows.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub hp: Option<Stat>,
    #[serde(default)]
    pub mp: Option<Stat>,
    #[serde(default)]
    pub attack: Option<Stat>,
    /// Both defence facets (UDF and LDF) at once.
    #[serde(default)]
    pub defence: Option<Stat>,
    /// AGL, the per-round action gauge the monster's swings spend.
    #[serde(default)]
    pub agility: Option<Stat>,
    /// SPD, the turn-order initiative seed.
    #[serde(default)]
    pub speed: Option<Stat>,
    /// INT.
    #[serde(default)]
    pub intelligence: Option<Stat>,
    #[serde(default)]
    pub exp: Option<Stat>,
    #[serde(default)]
    pub gold: Option<Stat>,
    #[serde(default)]
    pub drop: Option<ItemChance>,
    #[serde(default)]
    pub steal: Option<ItemChance>,
    /// Element id `0..=7` (`7` is neutral).
    #[serde(default)]
    pub element: Option<u8>,
    /// The spell ids the monster AI may cast, replacing the base monster's
    /// (at most three, the record's own capacity). An empty list makes it a
    /// physical attacker only.
    #[serde(default)]
    pub spells: Option<Vec<u8>>,
}

/// A slot in a formation: a pack monster by key, or a retail monster as it
/// is on the disc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MonsterRef {
    /// A key into [`BattleSection::monsters`].
    Pack(String),
    /// A [`DiscRef::Monster`].
    Disc(DiscRef),
}

/// A formation: up to [`MAX_FORMATION_SLOTS`] monsters fought at once, and
/// optionally further [`waves`](Self::waves) that take the freed monster
/// seats in the same battle.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormationDef {
    /// Banner label.
    #[serde(default)]
    pub label: String,
    /// The first wave: the monsters the battle opens on.
    pub monsters: Vec<MonsterRef>,
    /// A scripted fight, as retail's boss rows are: no escape, the boss
    /// battle intro, the alternate seat layout and the scripted stat boost.
    #[serde(default)]
    pub scripted: bool,
    /// The waves after the first, in order (`[[formations.<key>.waves]]`).
    /// The battle ends in victory once the last of them is down.
    #[serde(default)]
    pub waves: Vec<WaveDef>,
}

/// When a wave's monsters take their seats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveArrival {
    /// All at once, when every monster standing is down.
    #[default]
    Wipe,
    /// One at a time into a freed monster seat, at the start of a round,
    /// while fewer than [`WaveDef::max_standing`] monsters stand.
    Refill,
}

/// One wave after a formation's first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveDef {
    /// The wave's monsters. A `wipe` wave seats them together (at most
    /// [`MAX_FORMATION_SLOTS`]); a `refill` wave sends them one by one, in
    /// order, cycling through the list for [`Self::count`] arrivals.
    pub monsters: Vec<MonsterRef>,
    #[serde(default)]
    pub arrival: WaveArrival,
    /// `refill` only: arrivals in all (default: one per listed monster), at
    /// most [`MAX_WAVE_ARRIVALS`].
    #[serde(default)]
    pub count: Option<u16>,
    /// `refill` only: no arrival while this many monsters stand (default
    /// [`MAX_FORMATION_SLOTS`]).
    #[serde(default)]
    pub max_standing: Option<u8>,
}

impl FormationDef {
    /// Every monster slot across every wave, the first wave's first.
    pub fn all_monsters(&self) -> impl Iterator<Item = &MonsterRef> {
        self.monsters
            .iter()
            .chain(self.waves.iter().flat_map(|w| w.monsters.iter()))
    }
}

/// One weighted entry of an encounter override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedFormation {
    /// A key into [`BattleSection::formations`].
    pub formation: String,
    #[serde(default = "one")]
    pub weight: u32,
}

fn one() -> u32 {
    1
}

/// What one scene rolls while the pack is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterOverride {
    /// The scene's CDNAME name (`rikuroa`).
    pub scene: String,
    /// Formations a roll picks from, by weight.
    pub formations: Vec<WeightedFormation>,
    /// The weight the retail roll's own formation keeps in the pick; `0`
    /// (the default) replaces every retail encounter in the scene.
    #[serde(default)]
    pub retail_weight: u32,
    /// Steps between encounters, an inclusive `[min, max]` range drawn per
    /// battle. Set, the pack owns the scene's encounter rate and fights come
    /// even where retail rolls none; left out, retail's own rate decides
    /// when a fight comes and the pack only decides which.
    #[serde(default)]
    pub steps: Option<[u16; 2]>,
}

/// One item reward, `count` of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemReward {
    /// A [`DiscRef::Item`].
    pub item: DiscRef,
    #[serde(default = "one_u8")]
    pub count: u8,
}

fn one_u8() -> u8 {
    1
}

/// A named boss: a formation plus how the fight is staged and paid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BossDef {
    /// A key into [`BattleSection::formations`].
    pub formation: String,
    /// Refuse Run. Marks the formation scripted (see
    /// [`FormationDef::scripted`]).
    #[serde(default = "yes")]
    pub no_escape: bool,
    /// The battle track (a [`DiscRef::Bgm`]); the retail battle theme when
    /// left out.
    #[serde(default)]
    pub music: Option<DiscRef>,
    /// Items granted on victory, beside the formation's own spoils.
    #[serde(default)]
    pub reward_items: Vec<ItemReward>,
    /// Gold granted on victory, beside the formation's own spoils.
    #[serde(default)]
    pub reward_gold: u32,
    /// A boss fought once: after a victory it cannot be started again.
    #[serde(default)]
    pub once: bool,
}

fn yes() -> bool {
    true
}

/// The qualified id of a local key: `pack:key`. A key that already carries
/// a `pack:` prefix is returned unchanged.
pub fn qualify(pack_id: &str, key: &str) -> String {
    if key.contains(':') {
        key.to_string()
    } else {
        format!("{pack_id}:{key}")
    }
}

/// The local key a reference names inside pack `pack_id`: `key` itself, or
/// the part after `pack_id:`. `None` for a reference into another pack.
pub fn local_key<'a>(pack_id: &str, reference: &'a str) -> Option<&'a str> {
    match reference.split_once(':') {
        None => Some(reference),
        Some((p, k)) if p == pack_id => Some(k),
        Some(_) => None,
    }
}

impl BattleSection {
    /// Every problem a pack author must fix, as one line each; empty when
    /// the section is well formed. `pack_id` resolves qualified references.
    pub fn problems(&self, pack_id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let monster = |r: &str| local_key(pack_id, r).and_then(|k| self.monsters.get(k));
        let formation = |r: &str| local_key(pack_id, r).and_then(|k| self.formations.get(k));
        for (key, m) in &self.monsters {
            if !matches!(m.base, DiscRef::Monster { .. }) {
                out.push(format!("monster `{key}`: base must be a monster ref"));
            }
            for (what, chance) in [("drop", &m.drop), ("steal", &m.steal)] {
                if let Some(c) = chance {
                    if !matches!(c.item, DiscRef::Item { .. }) {
                        out.push(format!("monster `{key}`: {what} item must be an item ref"));
                    }
                    if c.chance_pct > 100 {
                        out.push(format!("monster `{key}`: {what} chance is over 100%"));
                    }
                }
            }
            if m.element.is_some_and(|e| e > 7) {
                out.push(format!("monster `{key}`: element must be 0..=7"));
            }
            if m.spells.as_ref().is_some_and(|s| s.len() > 3) {
                out.push(format!("monster `{key}`: at most three spells"));
            }
        }
        for (key, f) in &self.formations {
            if f.monsters.is_empty() {
                out.push(format!("formation `{key}`: no monsters"));
            }
            if f.monsters.len() > MAX_FORMATION_SLOTS {
                out.push(format!(
                    "formation `{key}`: {} monsters, the limit is {MAX_FORMATION_SLOTS}",
                    f.monsters.len()
                ));
            }
            if f.waves.len() > MAX_WAVES {
                out.push(format!(
                    "formation `{key}`: {} waves, the limit is {MAX_WAVES}",
                    f.waves.len()
                ));
            }
            for (i, w) in f.waves.iter().enumerate() {
                let n = i + 2;
                if w.monsters.is_empty() {
                    out.push(format!("formation `{key}`: wave {n} has no monsters"));
                }
                match w.arrival {
                    WaveArrival::Wipe => {
                        if w.monsters.len() > MAX_FORMATION_SLOTS {
                            out.push(format!(
                                "formation `{key}`: wave {n} seats {} monsters at once, the \
                                 limit is {MAX_FORMATION_SLOTS}",
                                w.monsters.len()
                            ));
                        }
                        if w.count.is_some() || w.max_standing.is_some() {
                            out.push(format!(
                                "formation `{key}`: wave {n}: count and max_standing need \
                                 arrival = \"refill\""
                            ));
                        }
                    }
                    WaveArrival::Refill => {
                        if w.count.is_some_and(|c| c == 0 || c > MAX_WAVE_ARRIVALS) {
                            out.push(format!(
                                "formation `{key}`: wave {n}: count must be 1..={MAX_WAVE_ARRIVALS}"
                            ));
                        }
                        if w.count.is_none() && w.monsters.len() > usize::from(MAX_WAVE_ARRIVALS) {
                            out.push(format!(
                                "formation `{key}`: wave {n}: more than {MAX_WAVE_ARRIVALS} \
                                 arrivals"
                            ));
                        }
                        if w.max_standing
                            .is_some_and(|m| m == 0 || usize::from(m) > MAX_FORMATION_SLOTS)
                        {
                            out.push(format!(
                                "formation `{key}`: wave {n}: max_standing must be \
                                 1..={MAX_FORMATION_SLOTS}"
                            ));
                        }
                    }
                }
            }
            // Pack monsters fight under their base monster's id (that id is
            // what keys the disc mesh and animation), so one formation - all
            // of its waves together, since one catalog swap covers the whole
            // battle - can hold one version of each base monster.
            let mut bases: BTreeMap<u16, Option<&str>> = BTreeMap::new();
            for r in f.all_monsters() {
                let (base, variant) = match r {
                    MonsterRef::Pack(k) => match monster(k) {
                        Some(m) => match m.base {
                            DiscRef::Monster { id } => (id, Some(k.as_str())),
                            _ => continue,
                        },
                        None => {
                            out.push(format!("formation `{key}`: unknown monster `{k}`"));
                            continue;
                        }
                    },
                    MonsterRef::Disc(DiscRef::Monster { id }) => (*id, None),
                    MonsterRef::Disc(_) => {
                        out.push(format!("formation `{key}`: a slot must be a monster"));
                        continue;
                    }
                };
                let variant = variant.and_then(|k| local_key(pack_id, k));
                match bases.get(&base) {
                    Some(prev) if *prev != variant => out.push(format!(
                        "formation `{key}`: two different versions of retail monster {base}"
                    )),
                    _ => {
                        bases.insert(base, variant);
                    }
                }
            }
        }
        for e in &self.encounters {
            if e.formations.is_empty() && e.retail_weight == 0 {
                out.push(format!("encounters `{}`: nothing to roll", e.scene));
            }
            for w in &e.formations {
                if formation(&w.formation).is_none() {
                    out.push(format!(
                        "encounters `{}`: unknown formation `{}`",
                        e.scene, w.formation
                    ));
                }
            }
            if e.formations.iter().map(|w| w.weight).sum::<u32>() == 0 && e.retail_weight == 0 {
                out.push(format!("encounters `{}`: every weight is zero", e.scene));
            }
            if let Some([lo, hi]) = e.steps
                && (lo == 0 || lo > hi)
            {
                out.push(format!(
                    "encounters `{}`: steps must be [min, max] with 0 < min <= max",
                    e.scene
                ));
            }
        }
        for (key, b) in &self.bosses {
            if formation(&b.formation).is_none() {
                out.push(format!("boss `{key}`: unknown formation `{}`", b.formation));
            }
            if b.music
                .as_ref()
                .is_some_and(|m| !matches!(m, DiscRef::Bgm { .. }))
            {
                out.push(format!("boss `{key}`: music must be a bgm ref"));
            }
            if b.reward_items
                .iter()
                .any(|r| !matches!(r.item, DiscRef::Item { .. }))
            {
                out.push(format!("boss `{key}`: reward items must be item refs"));
            }
        }
        out
    }
}

impl crate::validate::SectionRules for BattleSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for key in self.monsters.keys() {
            cx.declare("monster", key, &format!("monsters.{key}"));
        }
        for key in self.formations.keys() {
            cx.declare("formation", key, &format!("formations.{key}"));
        }
        for key in self.bosses.keys() {
            cx.declare("boss", key, &format!("bosses.{key}"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        let pack_id = cx.pack().manifest.id.clone();
        for p in self.problems(&pack_id) {
            cx.error("", p);
        }
        for (key, m) in &self.monsters {
            cx.disc_ref(&format!("monsters.{key}.base"), &m.base);
            for (what, chance) in [("drop", &m.drop), ("steal", &m.steal)] {
                if let Some(c) = chance {
                    cx.disc_ref(&format!("monsters.{key}.{what}.item"), &c.item);
                }
            }
        }
        for (key, f) in &self.formations {
            for (i, r) in f.monsters.iter().enumerate() {
                if let MonsterRef::Disc(d) = r {
                    cx.disc_ref(&format!("formations.{key}.monsters[{i}]"), d);
                }
            }
            for (w, wave) in f.waves.iter().enumerate() {
                for (i, r) in wave.monsters.iter().enumerate() {
                    if let MonsterRef::Disc(d) = r {
                        cx.disc_ref(&format!("formations.{key}.waves[{w}].monsters[{i}]"), d);
                    }
                }
            }
        }
        for (key, b) in &self.bosses {
            if let Some(m) = &b.music {
                cx.disc_ref(&format!("bosses.{key}.music"), m);
            }
            for (i, r) in b.reward_items.iter().enumerate() {
                cx.disc_ref(&format!("bosses.{key}.reward_items[{i}]"), &r.item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[monsters.ashen]
base = { kind = "monster", id = 73 }
name = "Ashen Caruban"
hp = 30000
attack = { mul = 1.5 }
drop = { item = { kind = "item", id = 12 }, chance_pct = 50 }

[formations.court]
label = "Ash Court"
monsters = ["ashen", { kind = "monster", id = 12 }]

[[encounters]]
scene = "rikuroa"
formations = [{ formation = "court", weight = 3 }]
steps = [100, 200]

[bosses.king]
formation = "rebirth:court"
music = { kind = "bgm", id = 30 }
reward_items = [{ item = { kind = "item", id = 5 } }]
once = true
"#;

    #[test]
    fn a_sample_section_parses_and_is_clean() {
        let s: BattleSection = toml::from_str(SAMPLE).unwrap();
        let m = &s.monsters["ashen"];
        assert_eq!(m.hp, Some(Stat::Set(30000)));
        assert_eq!(m.attack.unwrap().permille(), Some(1500));
        assert_eq!(
            s.formations["court"].monsters[1],
            MonsterRef::Disc(DiscRef::Monster { id: 12 })
        );
        assert_eq!(s.encounters[0].steps, Some([100, 200]));
        let b = &s.bosses["king"];
        assert!(b.no_escape && b.once);
        assert_eq!(b.reward_items[0].count, 1);
        assert_eq!(s.problems("rebirth"), Vec::<String>::new());
    }

    #[test]
    fn an_empty_file_is_an_empty_section() {
        let s: BattleSection = toml::from_str("").unwrap();
        assert_eq!(s, BattleSection::default());
    }

    #[test]
    fn problems_name_each_defect() {
        let mut s: BattleSection = toml::from_str(SAMPLE).unwrap();
        s.formations.get_mut("court").unwrap().monsters = vec![
            MonsterRef::Pack("ashen".into()),
            MonsterRef::Disc(DiscRef::Monster { id: 73 }),
            MonsterRef::Pack("nobody".into()),
            MonsterRef::Disc(DiscRef::Monster { id: 1 }),
            MonsterRef::Disc(DiscRef::Monster { id: 2 }),
        ];
        s.bosses.get_mut("king").unwrap().formation = "other:court".into();
        let p = s.problems("rebirth");
        assert!(p.iter().any(|l| l.contains("the limit is 4")), "{p:?}");
        assert!(p.iter().any(|l| l.contains("unknown monster `nobody`")));
        assert!(p.iter().any(|l| l.contains("two different versions")));
        assert!(
            p.iter()
                .any(|l| l.contains("boss `king`: unknown formation"))
        );
    }

    const WAVES: &str = r#"
[monsters.ashen]
base = { kind = "monster", id = 73 }
hp = 30000

[formations.gauntlet]
monsters = ["ashen", { kind = "monster", id = 12 }]

[[formations.gauntlet.waves]]
monsters = [{ kind = "monster", id = 12 }, { kind = "monster", id = 12 }, "ashen"]

[[formations.gauntlet.waves]]
monsters = [{ kind = "monster", id = 40 }, "ashen"]
arrival = "refill"
count = 6
max_standing = 2
"#;

    #[test]
    fn waves_parse_in_order_with_their_arrival() {
        let s: BattleSection = toml::from_str(WAVES).unwrap();
        let f = &s.formations["gauntlet"];
        assert_eq!(f.waves.len(), 2);
        assert_eq!(f.waves[0].arrival, WaveArrival::Wipe);
        assert_eq!(f.waves[0].monsters.len(), 3);
        let r = &f.waves[1];
        assert_eq!(
            (r.arrival, r.count, r.max_standing),
            (WaveArrival::Refill, Some(6), Some(2))
        );
        assert_eq!(f.all_monsters().count(), 7);
        assert_eq!(s.problems("rebirth"), Vec::<String>::new());
        // A formation without waves is unchanged by the field.
        let plain: BattleSection = toml::from_str(SAMPLE).unwrap();
        assert!(plain.formations["court"].waves.is_empty());
    }

    #[test]
    fn wave_problems_name_each_defect() {
        let mut s: BattleSection = toml::from_str(WAVES).unwrap();
        // A second Caruban variant, seated in a later wave.
        let mut pale = s.monsters["ashen"].clone();
        pale.hp = None;
        s.monsters.insert("pale".into(), pale);
        let f = s.formations.get_mut("gauntlet").unwrap();
        f.waves[0].monsters.push(MonsterRef::Pack("pale".into()));
        f.waves[0].monsters.push(MonsterRef::Pack("ashen".into()));
        f.waves[0].count = Some(3);
        f.waves[1].max_standing = Some(5);
        f.waves[1].count = Some(0);
        f.waves.push(WaveDef::default());
        let p = s.problems("rebirth");
        let has = |needle: &str| p.iter().any(|l| l.contains(needle));
        assert!(has("two different versions of retail monster 73"), "{p:?}");
        assert!(has("wave 2 seats 5 monsters at once"), "{p:?}");
        assert!(has("wave 2: count and max_standing need"), "{p:?}");
        assert!(has("wave 3: count must be"), "{p:?}");
        assert!(has("wave 3: max_standing must be"), "{p:?}");
        assert!(has("wave 4 has no monsters"), "{p:?}");
    }

    #[test]
    fn qualify_and_local_key_round_trip() {
        assert_eq!(qualify("rebirth", "court"), "rebirth:court");
        assert_eq!(qualify("rebirth", "rebirth:court"), "rebirth:court");
        assert_eq!(local_key("rebirth", "rebirth:court"), Some("court"));
        assert_eq!(local_key("rebirth", "court"), Some("court"));
        assert_eq!(local_key("rebirth", "other:court"), None);
    }
}
