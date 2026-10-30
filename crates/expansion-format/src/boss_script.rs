//! Boss scripts: a pack monster's own battle AI, in data.
//!
//! A pack monster without a script fights with its base monster's retail AI.
//! One with a script (`[monsters.<key>.script]` in `battle.toml`) replaces
//! the AI's pick on each of its turns with the script's: the current phase's
//! weighted move list, a targeting rule, and the phase changes, enrage,
//! telegraphed charges, summons, status immunities and last stand around it.
//!
//! ```toml
//! [monsters.cinder_warden.script]
//! immune = ["sleep", "confuse", "stone"]
//! last_stand = { line = "warden_last_stand" }
//! enrage = { when = { round = 12 }, line = "warden_enrage", attack = { mul = 1.5 } }
//!
//! [[monsters.cinder_warden.script.phases]]          # the opening phase
//! moves = [
//!     { attack = 0, weight = 3 },                    # the base monster's first swing
//!     { spell = 0x25, weight = 1 },                  # a spell-table id
//! ]
//!
//! [[monsters.cinder_warden.script.phases]]
//! when = { hp_below_pct = 50 }
//! line = "warden_rage"
//! attack = { mul = 1.3 }
//! summon = ["pyre_wisp"]
//! moves = [
//!     { attack = 1, hits = 2, weight = 2, target = "lowest_hp" },
//!     { spell = 0x46, weight = 1, charge = true, charge_line = "warden_charge" },
//! ]
//! ```
//!
//! Every move is retail content: `attack = n` is the `n`th swing of the base
//! monster's own attack list (its archive swing records, whose clips and
//! power bytes the disc carries), `spell = id` a spell-table id the monster
//! casts through the retail cast path. A script never invents damage.
//!
//! The runtime kernel is `legaia_engine_expansion::boss_script`.

use serde::{Deserialize, Serialize};

use crate::battle::{MAX_FORMATION_SLOTS, MonsterRef, Stat};

/// Phases one script may list.
pub const MAX_PHASES: usize = 8;

/// Moves one phase may list.
pub const MAX_MOVES: usize = 12;

/// Swings one `attack` move may chain (`hits`).
pub const MAX_HITS: u8 = 8;

/// Characters a boss banner holds on its one row; a longer line still
/// shows, and the validator warns.
pub const BANNER_WARN_CHARS: usize = 40;

/// The banner text a dialog line shows in battle: its first page's rows,
/// joined with spaces.
pub fn banner_text(line: &crate::dialog::DialogLine) -> String {
    line.text
        .split("\n\n")
        .next()
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Who a move aims at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetRule {
    /// A living party member at random (the retail physical pick).
    #[default]
    Random,
    /// The living party member with the least HP.
    LowestHp,
    /// The living party member with the highest attack.
    HighestAttack,
    /// The living party member who can heal: one who knows an ally-side
    /// spell, the most MP first; with none, the most MP.
    Healer,
}

/// A status a scripted monster can be immune to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusName {
    Venom,
    Toxic,
    Numb,
    Sleep,
    Confuse,
    Rot,
    Curse,
    Stone,
}

/// When a phase begins. A phase begins at the first check (a round's start,
/// or one of the monster's own turns) at which **any** of its set
/// conditions holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    /// The monster's HP is below this percent of its maximum (`1..=99`).
    #[serde(default)]
    pub hp_below_pct: Option<u8>,
    /// The battle has reached this round (the first round is `1`).
    #[serde(default)]
    pub round: Option<u16>,
    /// At least this many of the monster's allies are down.
    #[serde(default)]
    pub allies_down: Option<u8>,
}

impl Trigger {
    /// Whether no condition is set.
    pub fn is_empty(&self) -> bool {
        self.hp_below_pct.is_none() && self.round.is_none() && self.allies_down.is_none()
    }
}

/// One move a phase may pick: a swing of the base monster's own or a spell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveDef {
    /// The `n`th (0-based) swing of the base monster's attack list.
    #[serde(default)]
    pub attack: Option<u8>,
    /// A spell-table id, cast through the retail cast path.
    #[serde(default)]
    pub spell: Option<u8>,
    /// `attack` only: swings chained in one turn (`1..=8`, default `1`).
    #[serde(default)]
    pub hits: Option<u8>,
    /// Relative weight of the pick (default `1`).
    #[serde(default = "one")]
    pub weight: u32,
    /// Who it aims at; the phase's or the script's rule when left out. A
    /// spell whose shape hits a whole side or an ally ignores it.
    #[serde(default)]
    pub target: Option<TargetRule>,
    /// Telegraphed: the turn it is picked the monster only charges (a
    /// banner stands), and the move lands on its next turn.
    #[serde(default)]
    pub charge: bool,
    /// The dialog line the charge banner shows; a generic "gathering
    /// power" line when left out.
    #[serde(default)]
    pub charge_line: Option<String>,
    /// The monster's own turns before the move may be picked again.
    #[serde(default)]
    pub cooldown: u8,
    /// The sound the charge turn plays: the telegraph's cue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charge_sound: Option<crate::audio::SoundRef>,
    /// `spell` only: cast whatever MP the monster has left, spending none
    /// (the retail cast path otherwise turns an unaffordable cast into a
    /// swing).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_mp: bool,
}

fn one() -> u32 {
    1
}

/// A script phase, or the enrage (`[monsters.<key>.script.enrage]`).
///
/// Its changes apply once, as it begins: stats scale (`{ mul = x }`) or set
/// the monster's live battle stats, `element` changes its element (and with
/// it what it resists), `heal_pct` restores HP, `summon` seats monsters into
/// free monster seats, and `line` shows a banner. A phase with no `moves`
/// keeps the moves of the phase before it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseDef {
    /// When it begins; the first phase opens the fight and sets none.
    #[serde(default)]
    pub when: Trigger,
    /// A dialog line (`dialog.toml`) shown as the battle banner.
    #[serde(default)]
    pub line: Option<String>,
    #[serde(default)]
    pub moves: Vec<MoveDef>,
    /// The phase's targeting rule.
    #[serde(default)]
    pub target: Option<TargetRule>,
    #[serde(default)]
    pub attack: Option<Stat>,
    /// Both defence facets.
    #[serde(default)]
    pub defence: Option<Stat>,
    #[serde(default)]
    pub agility: Option<Stat>,
    #[serde(default)]
    pub speed: Option<Stat>,
    #[serde(default)]
    pub intelligence: Option<Stat>,
    /// Element id `0..=7`.
    #[serde(default)]
    pub element: Option<u8>,
    /// Percent of maximum HP restored (`1..=100`).
    #[serde(default)]
    pub heal_pct: Option<u8>,
    /// Monsters seated into free monster seats as the next round opens.
    #[serde(default)]
    pub summon: Vec<MonsterRef>,
    /// The battle music from this phase on (a cross-fade as it begins).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music: Option<crate::audio::MusicRef>,
    /// A sound played as the phase begins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<crate::audio::SoundRef>,
}

/// The once-per-fight survival of a lethal blow.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastStand {
    /// The dialog line shown as it holds on at 1 HP.
    #[serde(default)]
    pub line: Option<String>,
}

/// A pack monster's script (`[monsters.<key>.script]`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BossScript {
    /// The default targeting rule.
    #[serde(default)]
    pub target: TargetRule,
    /// The phases, in order; the first opens the fight. A script with no
    /// phases keeps the retail AI's picks.
    #[serde(default)]
    pub phases: Vec<PhaseDef>,
    /// The enrage timer: a phase-shaped change keyed on `when.round` alone
    /// that applies over whatever phase stands (its `moves`, when set,
    /// replace the phase's for the rest of the fight).
    #[serde(default)]
    pub enrage: Option<PhaseDef>,
    /// Statuses that never take hold.
    #[serde(default)]
    pub immune: Vec<StatusName>,
    /// Survive the first lethal blow at 1 HP.
    #[serde(default)]
    pub last_stand: Option<LastStand>,
}

impl BossScript {
    /// Every phase change, the enrage last.
    pub fn all_phases(&self) -> impl Iterator<Item = &PhaseDef> {
        self.phases.iter().chain(self.enrage.iter())
    }

    /// Every monster the script may summon.
    pub fn summons(&self) -> impl Iterator<Item = &MonsterRef> {
        self.all_phases().flat_map(|p| p.summon.iter())
    }

    /// Every music change the script makes, with the field naming it.
    pub fn music(&self) -> Vec<(String, &crate::audio::MusicRef)> {
        let named = self
            .phases
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("phases[{i}]"), p))
            .chain(self.enrage.iter().map(|e| ("enrage".to_string(), e)));
        named
            .filter_map(|(f, p)| p.music.as_ref().map(|m| (format!("{f}.music"), m)))
            .collect()
    }

    /// Every sound the script plays (phase sounds, charge telegraphs), with
    /// the field naming it.
    pub fn sounds(&self) -> Vec<(String, &crate::audio::SoundRef)> {
        let mut out = Vec::new();
        let named = self
            .phases
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("phases[{i}]"), p))
            .chain(self.enrage.iter().map(|e| ("enrage".to_string(), e)));
        for (f, p) in named {
            if let Some(s) = &p.sound {
                out.push((format!("{f}.sound"), s));
            }
            for (j, m) in p.moves.iter().enumerate() {
                if let Some(s) = &m.charge_sound {
                    out.push((format!("{f}.moves[{j}].charge_sound"), s));
                }
            }
        }
        out
    }

    /// Every dialog line the script names, with the field naming it.
    pub fn lines(&self) -> Vec<(String, &str)> {
        let mut out = Vec::new();
        for (i, p) in self.phases.iter().enumerate() {
            push_phase_lines(&mut out, &format!("phases[{i}]"), p);
        }
        if let Some(e) = &self.enrage {
            push_phase_lines(&mut out, "enrage", e);
        }
        if let Some(l) = self.last_stand.as_ref().and_then(|l| l.line.as_deref()) {
            out.push(("last_stand.line".to_string(), l));
        }
        out
    }

    /// Every problem in the script that needs no other file, one line each,
    /// prefixed `monster \`key\`: script`.
    pub fn problems(&self, key: &str) -> Vec<String> {
        let mut out = Vec::new();
        let p = |m: String| format!("monster `{key}`: script {m}");
        if self.phases.len() > MAX_PHASES {
            out.push(p(format!(
                "has {} phases, the limit is {MAX_PHASES}",
                self.phases.len()
            )));
        }
        if self.phases.first().is_some_and(|f| !f.when.is_empty()) {
            out.push(p("phases[0] opens the fight and takes no `when`".into()));
        }
        if !self.phases.is_empty() && self.phases[0].moves.is_empty() {
            out.push(p("phases[0] lists no moves".into()));
        }
        for (i, ph) in self.phases.iter().enumerate().skip(1) {
            if ph.when.is_empty() {
                out.push(p(format!("phases[{i}] needs a `when`")));
            }
        }
        let mut last_hp = 100u8;
        for (i, ph) in self.phases.iter().enumerate().skip(1) {
            if let Some(h) = ph.when.hp_below_pct {
                if h >= last_hp {
                    out.push(p(format!(
                        "phases[{i}]: hp_below_pct {h} is not below the phase before it"
                    )));
                }
                last_hp = h;
            }
        }
        for (field, ph) in self
            .phases
            .iter()
            .enumerate()
            .map(|(i, ph)| (format!("phases[{i}]"), ph))
            .chain(self.enrage.iter().map(|e| ("enrage".to_string(), e)))
        {
            out.extend(phase_problems(&field, ph).into_iter().map(&p));
        }
        if let Some(e) = &self.enrage
            && (e.when.round.is_none()
                || e.when.hp_below_pct.is_some()
                || e.when.allies_down.is_some())
        {
            out.push(p("enrage is keyed on `when.round` alone".into()));
        }
        let mut seen = std::collections::BTreeSet::new();
        for s in &self.immune {
            if !seen.insert(*s) {
                out.push(p(format!("lists immunity {s:?} twice")));
            }
        }
        out
    }
}

fn push_phase_lines<'a>(out: &mut Vec<(String, &'a str)>, field: &str, p: &'a PhaseDef) {
    if let Some(l) = p.line.as_deref() {
        out.push((format!("{field}.line"), l));
    }
    for (j, m) in p.moves.iter().enumerate() {
        if let Some(l) = m.charge_line.as_deref() {
            out.push((format!("{field}.moves[{j}].charge_line"), l));
        }
    }
}

fn phase_problems(field: &str, ph: &PhaseDef) -> Vec<String> {
    let mut out = Vec::new();
    if ph.moves.len() > MAX_MOVES {
        out.push(format!(
            "{field} lists {} moves, the limit is {MAX_MOVES}",
            ph.moves.len()
        ));
    }
    if ph.when.hp_below_pct.is_some_and(|h| h == 0 || h > 99) {
        out.push(format!("{field}: hp_below_pct must be 1..=99"));
    }
    if ph.when.round == Some(0) {
        out.push(format!("{field}: round starts at 1"));
    }
    if ph
        .when
        .allies_down
        .is_some_and(|n| n == 0 || usize::from(n) > MAX_FORMATION_SLOTS * 4)
    {
        out.push(format!("{field}: allies_down must be at least 1"));
    }
    if ph.element.is_some_and(|e| e > 7) {
        out.push(format!("{field}: element must be 0..=7"));
    }
    if ph.heal_pct.is_some_and(|h| h == 0 || h > 100) {
        out.push(format!("{field}: heal_pct must be 1..=100"));
    }
    if ph.summon.len() > MAX_FORMATION_SLOTS {
        out.push(format!(
            "{field} summons {}, the monster block holds {MAX_FORMATION_SLOTS}",
            ph.summon.len()
        ));
    }
    for (j, m) in ph.moves.iter().enumerate() {
        let mf = format!("{field}.moves[{j}]");
        match (m.attack, m.spell) {
            (Some(_), Some(_)) => out.push(format!("{mf}: set `attack` or `spell`, not both")),
            (None, None) => out.push(format!("{mf}: set `attack` or `spell`")),
            _ => {}
        }
        if m.spell == Some(0) {
            out.push(format!("{mf}: spell 0 is no spell"));
        }
        if let Some(h) = m.hits {
            if m.attack.is_none() {
                out.push(format!("{mf}: `hits` needs `attack`"));
            }
            if h == 0 || h > MAX_HITS {
                out.push(format!("{mf}: hits must be 1..={MAX_HITS}"));
            }
        }
        if m.weight == 0 {
            out.push(format!("{mf}: weight must be at least 1"));
        }
        if m.charge_line.is_some() && !m.charge {
            out.push(format!("{mf}: charge_line needs charge = true"));
        }
        if m.ignore_mp && m.spell.is_none() {
            out.push(format!("{mf}: ignore_mp needs a spell"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::battle::BattleSection;

    const OK: &str = r#"
[monsters.warden]
base = { kind = "monster", id = 172 }

[monsters.warden.script]
immune = ["sleep", "stone"]
last_stand = {}
enrage = { when = { round = 12 }, attack = { mul = 1.5 } }

[[monsters.warden.script.phases]]
moves = [{ attack = 0, weight = 3 }, { spell = 0x25 }]

[[monsters.warden.script.phases]]
when = { hp_below_pct = 50 }
summon = ["guard"]
moves = [{ spell = 0x46, charge = true, cooldown = 2 }]

[monsters.guard]
base = { kind = "monster", id = 129 }

[formations.hall]
monsters = ["warden"]
"#;

    #[test]
    fn a_script_parses_and_is_clean() {
        let s: BattleSection = toml::from_str(OK).unwrap();
        assert_eq!(s.problems("p"), Vec::<String>::new());
        let script = s.monsters["warden"].script.as_ref().unwrap();
        assert_eq!(script.phases.len(), 2);
        assert_eq!(script.summons().count(), 1);
        // The formation's fight takes in the summon.
        let f = &s.formations["hall"];
        assert_eq!(s.fight_monsters("p", f).len(), 2);
    }

    #[test]
    fn problems_name_each_defect() {
        let mut s: BattleSection = toml::from_str(OK).unwrap();
        let w = s.monsters.get_mut("warden").unwrap();
        let script = w.script.as_mut().unwrap();
        script.phases[0].when.round = Some(2);
        script.phases[1].when = Default::default();
        script.phases[1].moves[0].attack = Some(1);
        script.phases[1]
            .moves
            .push(toml::from_str("attack = 0\nhits = 9\nweight = 0\ncharge_line = \"x\"").unwrap());
        script.phases[1]
            .summon
            .push(crate::battle::MonsterRef::Pack("ghost".into()));
        script.enrage.as_mut().unwrap().when.hp_below_pct = Some(10);
        script.immune.push(super::StatusName::Sleep);
        // A second Caruban variant in the fight: the summon.
        s.monsters.insert(
            "ghost".into(),
            toml::from_str("base = { kind = \"monster\", id = 172 }").unwrap(),
        );
        let p = s.problems("p");
        let has = |n: &str| p.iter().any(|l| l.contains(n));
        assert!(has("phases[0] opens the fight"), "{p:?}");
        assert!(has("phases[1] needs a `when`"), "{p:?}");
        assert!(has("not both"), "{p:?}");
        assert!(has("hits must be"), "{p:?}");
        assert!(has("weight must be"), "{p:?}");
        assert!(has("charge_line needs charge"), "{p:?}");
        assert!(has("enrage is keyed on `when.round` alone"), "{p:?}");
        assert!(has("immunity Sleep twice"), "{p:?}");
        assert!(has("two different versions of retail monster 172"), "{p:?}");
    }

    #[test]
    fn hp_thresholds_must_fall() {
        let s: BattleSection = toml::from_str(
            r#"
[monsters.m]
base = { kind = "monster", id = 1 }
[[monsters.m.script.phases]]
moves = [{ attack = 0 }]
[[monsters.m.script.phases]]
when = { hp_below_pct = 30 }
[[monsters.m.script.phases]]
when = { hp_below_pct = 60 }
"#,
        )
        .unwrap();
        let p = s.problems("p");
        assert!(
            p.iter().any(|l| l.contains("not below the phase before")),
            "{p:?}"
        );
    }

    #[test]
    fn ignore_mp_needs_a_spell() {
        let s: super::BossScript =
            toml::from_str("[[phases]]\nmoves = [{ attack = 0, ignore_mp = true }]").unwrap();
        assert!(
            s.problems("t").iter().any(|p| p.contains("ignore_mp")),
            "{:?}",
            s.problems("t")
        );
        let s: super::BossScript =
            toml::from_str("[[phases]]\nmoves = [{ spell = 0x51, ignore_mp = true }]").unwrap();
        assert!(s.problems("t").is_empty(), "{:?}", s.problems("t"));
    }
}
