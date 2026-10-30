//! Boss scripts: the deterministic kernel a scripted pack monster's turns
//! run through ([`legaia_expansion_format::boss_script`]).
//!
//! A `World` consults this kernel where the retail monster AI picks an
//! action (`World::pick_monster_action`), and only for a seat whose monster
//! fights as a scripted pack monster in the current fight: every other seat,
//! and every fight with no pack, never reaches it. One [`BossRun`] per
//! scripted seat holds the fight's script state:
//!
//! - [`BossRun::observe`] walks the phases forward (in order, never back)
//!   and fires the enrage, returning each change for the `World` to apply:
//!   stats, element, heal, summons, banner line.
//! - [`BossRun::pick`] picks the turn's move: the charged move when one is
//!   pending, otherwise one weighted draw over the moves off cooldown. A
//!   telegraphed move only charges on the turn it is picked.
//! - [`choose_target`] resolves a targeting rule over the living party.
//! - [`BossRun::spend_last_stand`] answers a lethal blow once.
//!
//! Every draw comes from the caller's RNG closure (the battle RNG), so a
//! fight replays bit for bit.

use std::sync::Arc;

use legaia_expansion_format::battle::Stat;
use legaia_expansion_format::boss_script::{self as fmt, StatusName, TargetRule, Trigger};

/// What a move does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    /// Swing `index` of the base monster's attack list, `hits` times.
    Attack { index: u8, hits: u8 },
    /// Cast a spell-table id.
    Spell(u8),
}

/// One compiled move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledMove {
    pub kind: MoveKind,
    pub weight: u32,
    /// The rule with the phase's and the script's defaults folded in.
    pub target: TargetRule,
    pub charge: bool,
    pub charge_line: Option<String>,
    pub cooldown: u8,
    /// The telegraph's sound, played on the charge turn.
    pub charge_sound: Option<legaia_expansion_format::audio::SoundRef>,
    /// A spell cast whatever the monster's MP, spending none.
    pub ignore_mp: bool,
}

/// The stat changes a phase makes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StatChanges {
    pub attack: Option<Stat>,
    pub defence: Option<Stat>,
    pub agility: Option<Stat>,
    pub speed: Option<Stat>,
    pub intelligence: Option<Stat>,
}

/// One compiled phase (or the enrage).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledPhase {
    pub when: Trigger,
    pub line: Option<String>,
    /// `None`: the phase keeps the moves standing before it.
    pub moves: Option<Vec<CompiledMove>>,
    pub stats: StatChanges,
    pub element: Option<u8>,
    pub heal_pct: Option<u8>,
    /// Base monster ids to seat as the next round opens.
    pub summon: Vec<u16>,
    /// The battle music from this phase on.
    pub music: Option<legaia_expansion_format::audio::MusicRef>,
    /// A sound played as it begins.
    pub sound: Option<legaia_expansion_format::audio::SoundRef>,
}

/// One compiled script.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledScript {
    pub phases: Vec<CompiledPhase>,
    pub enrage: Option<CompiledPhase>,
    pub immune: Vec<StatusName>,
    /// `Some(line)` when the monster has a last stand.
    pub last_stand: Option<Option<String>>,
}

impl CompiledScript {
    /// Compile `script`; `summon_id` answers a summon reference's base
    /// monster id (`None` drops it - the format check has already refused
    /// an unknown one).
    pub fn compile(
        script: &fmt::BossScript,
        mut summon_id: impl FnMut(&legaia_expansion_format::battle::MonsterRef) -> Option<u16>,
    ) -> Self {
        let mut phase = |p: &fmt::PhaseDef| {
            let rule = p.target.unwrap_or(script.target);
            CompiledPhase {
                when: p.when,
                line: p.line.clone(),
                moves: (!p.moves.is_empty()).then(|| {
                    p.moves
                        .iter()
                        .filter_map(|m| {
                            let kind = match (m.attack, m.spell) {
                                (Some(index), None) => MoveKind::Attack {
                                    index,
                                    hits: m.hits.unwrap_or(1).clamp(1, fmt::MAX_HITS),
                                },
                                (None, Some(id)) => MoveKind::Spell(id),
                                _ => return None,
                            };
                            Some(CompiledMove {
                                kind,
                                weight: m.weight.max(1),
                                target: m.target.unwrap_or(rule),
                                charge: m.charge,
                                charge_line: m.charge_line.clone(),
                                cooldown: m.cooldown,
                                charge_sound: m.charge_sound.clone(),
                                ignore_mp: m.ignore_mp,
                            })
                        })
                        .collect()
                }),
                stats: StatChanges {
                    attack: p.attack,
                    defence: p.defence,
                    agility: p.agility,
                    speed: p.speed,
                    intelligence: p.intelligence,
                },
                element: p.element,
                heal_pct: p.heal_pct,
                summon: p.summon.iter().filter_map(&mut summon_id).collect(),
                music: p.music.clone(),
                sound: p.sound.clone(),
            }
        };
        CompiledScript {
            phases: script.phases.iter().map(&mut phase).collect(),
            enrage: script.enrage.as_ref().map(&mut phase),
            immune: script.immune.clone(),
            last_stand: script.last_stand.as_ref().map(|l| l.line.clone()),
        }
    }

    /// Whether the script picks moves at all (a script of immunities or a
    /// last stand alone keeps the retail AI's picks).
    pub fn picks_moves(&self) -> bool {
        self.phases.iter().any(|p| p.moves.is_some())
            || self.enrage.as_ref().is_some_and(|e| e.moves.is_some())
    }
}

/// What the monster sees when a check runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Observation {
    pub hp: u16,
    pub max_hp: u16,
    /// The battle round, the first being `1`.
    pub round: u16,
    /// The monster's allies down so far this fight.
    pub allies_down: u8,
}

/// A change [`BossRun::observe`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Phase `n` (0-based) began.
    Phase(usize),
    /// The enrage fired.
    Enrage,
}

/// The turn's pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// No scripted move stands: the retail AI picks.
    Retail,
    /// A telegraphed move is picked: this turn only charges, the move lands
    /// on the monster's next turn.
    Charge(CompiledMove),
    /// Do this move now.
    Move(CompiledMove),
}

/// The script state of one scripted seat for one fight.
#[derive(Debug, Clone, PartialEq)]
pub struct BossRun {
    pub script: Arc<CompiledScript>,
    /// The phase standing (`None` until the first check opens phase 0, and
    /// for a script with no phases).
    pub phase: Option<usize>,
    pub enraged: bool,
    /// The telegraphed move to land on the next turn.
    pub charging: Option<CompiledMove>,
    pub last_stand_spent: bool,
    /// Own turns left before each standing move may be picked again.
    cooldowns: Vec<u8>,
}

impl BossRun {
    pub fn new(script: Arc<CompiledScript>) -> Self {
        Self {
            script,
            phase: None,
            enraged: false,
            charging: None,
            last_stand_spent: false,
            cooldowns: Vec::new(),
        }
    }

    /// The phase (or enrage) a [`Change`] names.
    pub fn changed(&self, c: Change) -> Option<&CompiledPhase> {
        match c {
            Change::Phase(n) => self.script.phases.get(n),
            Change::Enrage => self.script.enrage.as_ref(),
        }
    }

    /// The moves standing: the enrage's when it fired with moves of its
    /// own, else the latest phase's that set any.
    pub fn moves(&self) -> &[CompiledMove] {
        if self.enraged
            && let Some(m) = self.script.enrage.as_ref().and_then(|e| e.moves.as_ref())
        {
            return m;
        }
        let Some(cur) = self.phase else {
            return &[];
        };
        self.script.phases[..=cur]
            .iter()
            .rev()
            .find_map(|p| p.moves.as_deref())
            .unwrap_or(&[])
    }

    /// Walk the phases forward against `obs`: the first check opens phase
    /// 0, and each later phase whose trigger holds begins in order (one
    /// check may begin several). The enrage fires once its round comes.
    pub fn observe(&mut self, obs: &Observation) -> Vec<Change> {
        let mut out = Vec::new();
        let before = self.moves().to_vec();
        if self.phase.is_none() && !self.script.phases.is_empty() {
            self.phase = Some(0);
            out.push(Change::Phase(0));
        }
        while let Some(cur) = self.phase {
            let Some(next) = self.script.phases.get(cur + 1) else {
                break;
            };
            if !triggered(&next.when, obs) {
                break;
            }
            self.phase = Some(cur + 1);
            out.push(Change::Phase(cur + 1));
        }
        if !self.enraged
            && let Some(e) = &self.script.enrage
            && e.when.round.is_some_and(|r| obs.round >= r)
        {
            self.enraged = true;
            out.push(Change::Enrage);
        }
        if self.moves() != before.as_slice() {
            // A new move list starts with every move ready.
            self.cooldowns.clear();
        }
        out
    }

    /// Pick the turn's move with one weighted draw (none when a charged
    /// move is due or only one move is ready).
    pub fn pick(&mut self, rand: &mut dyn FnMut() -> u32) -> Pick {
        if let Some(m) = self.charging.take() {
            self.start_cooldown(&m);
            return Pick::Move(m);
        }
        let moves = self.moves().to_vec();
        if moves.is_empty() {
            return Pick::Retail;
        }
        self.cooldowns.resize(moves.len(), 0);
        for c in self.cooldowns.iter_mut() {
            *c = c.saturating_sub(1);
        }
        let mut ready: Vec<usize> = (0..moves.len())
            .filter(|&i| self.cooldowns[i] == 0)
            .collect();
        if ready.is_empty() {
            // Everything cooling: the move nearest ready.
            let min = self.cooldowns.iter().copied().min().unwrap_or(0);
            ready = (0..moves.len())
                .filter(|&i| self.cooldowns[i] == min)
                .collect();
        }
        let pick = if ready.len() == 1 {
            ready[0]
        } else {
            let total: u64 = ready.iter().map(|&i| u64::from(moves[i].weight)).sum();
            let mut r = u64::from(rand()) % total.max(1);
            let mut chosen = ready[ready.len() - 1];
            for &i in &ready {
                let w = u64::from(moves[i].weight);
                if r < w {
                    chosen = i;
                    break;
                }
                r -= w;
            }
            chosen
        };
        let m = moves[pick].clone();
        if m.charge {
            self.charging = Some(m.clone());
            return Pick::Charge(m);
        }
        self.start_cooldown(&m);
        Pick::Move(m)
    }

    fn start_cooldown(&mut self, m: &CompiledMove) {
        let moves = self.moves().to_vec();
        self.cooldowns.resize(moves.len(), 0);
        if let Some(i) = moves.iter().position(|x| x == m) {
            // `+1`: the next pick's tick takes the first one off.
            self.cooldowns[i] = m.cooldown.saturating_add(u8::from(m.cooldown > 0));
        }
    }

    /// A lethal blow lands: `Some(line)` the first time when the script has
    /// a last stand (the monster holds on at 1 HP), `None` otherwise.
    pub fn spend_last_stand(&mut self) -> Option<Option<String>> {
        if self.last_stand_spent {
            return None;
        }
        let line = self.script.last_stand.clone()?;
        self.last_stand_spent = true;
        Some(line)
    }
}

/// Whether any of `t`'s conditions holds.
pub fn triggered(t: &Trigger, obs: &Observation) -> bool {
    let hp = t.hp_below_pct.is_some_and(|pct| {
        obs.max_hp > 0 && u32::from(obs.hp) * 100 < u32::from(obs.max_hp) * u32::from(pct)
    });
    let round = t.round.is_some_and(|r| obs.round >= r);
    let down = t.allies_down.is_some_and(|n| obs.allies_down >= n);
    hp || round || down
}

/// One living party member a rule chooses among.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Candidate {
    pub slot: u8,
    pub hp: u16,
    pub attack: u16,
    pub mp: u16,
    /// Knows an ally-side spell.
    pub heals: bool,
}

/// The slot `rule` picks among `cands` (the living party). `Random` draws
/// once; the others draw nothing and break ties toward the lower slot.
pub fn choose_target(
    rule: TargetRule,
    cands: &[Candidate],
    rand: &mut dyn FnMut() -> u32,
) -> Option<u8> {
    if cands.is_empty() {
        return None;
    }
    let best = |key: &dyn Fn(&Candidate) -> i64| {
        let mut best = cands[0];
        for c in &cands[1..] {
            if key(c) > key(&best) {
                best = *c;
            }
        }
        best.slot
    };
    Some(match rule {
        TargetRule::Random => cands[(rand() % cands.len() as u32) as usize].slot,
        TargetRule::LowestHp => best(&|c| -i64::from(c.hp)),
        TargetRule::HighestAttack => best(&|c| i64::from(c.attack)),
        TargetRule::Healer => best(&|c| i64::from(c.heals) << 20 | i64::from(c.mp)),
    })
}

/// Apply a phase's stat change to `v`: a set value, or `v` scaled.
pub fn apply_stat(v: u16, stat: Option<Stat>, cap: u16) -> u16 {
    match stat {
        None => v,
        Some(Stat::Set(x)) => x.min(u32::from(cap)) as u16,
        Some(s) => crate::battle::scale_permille(
            u32::from(v),
            s.permille().unwrap_or(1000),
            u32::from(cap),
        ) as u16,
    }
}

/// The banner text of a dialog line: its first page's rows, joined.
pub fn banner_text(line: &legaia_expansion_format::dialog::DialogLine) -> String {
    fmt::banner_text(line)
}

/// The token a stock banner reads as the boss's name.
pub const NAME_TOKEN: &str = "<name>";
/// The dialog line a pack words the stock enrage banner with.
pub const BANNER_ENRAGED: &str = "boss_enraged";
/// The dialog line for the stock charge (telegraph) banner.
pub const BANNER_CHARGING: &str = "boss_charging";
/// The dialog line for the stock last-stand banner.
pub const BANNER_LAST_STAND: &str = "boss_last_stand";

/// The engine's own wording of the stock banners, for a pack that does not
/// word them: an enrage with no `line`, a charge with no `charge_line`, a
/// last stand with no `line`.
pub const DEFAULT_BANNERS: [(&str, &str); 3] = [
    (BANNER_ENRAGED, "<name> is enraged!"),
    (BANNER_CHARGING, "<name> is gathering power!"),
    (BANNER_LAST_STAND, "<name> refuses to fall!"),
];

/// Stock banner `key` for boss `name`: the pack's dialog line `key` when
/// `dialog` has one with text (a pack in another language words them its
/// own way), else the engine's [`DEFAULT_BANNERS`] wording; `<name>` reads
/// as `name`.
pub fn stock_banner(
    dialog: Option<&legaia_expansion_format::dialog::DialogSection>,
    key: &str,
    name: &str,
) -> String {
    let template = dialog
        .and_then(|d| d.lines.get(key))
        .map(banner_text)
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            DEFAULT_BANNERS
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, t)| t.to_string())
        })
        .unwrap_or_default();
    template.replace(NAME_TOKEN, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(src: &str) -> Arc<CompiledScript> {
        let s: fmt::BossScript = toml::from_str(src).unwrap();
        assert_eq!(s.problems("t"), Vec::<String>::new());
        Arc::new(CompiledScript::compile(&s, |r| match r {
            legaia_expansion_format::battle::MonsterRef::Disc(
                legaia_expansion_format::refs::DiscRef::Monster { id },
            ) => Some(*id),
            _ => Some(500),
        }))
    }

    const THREE: &str = r#"
target = "lowest_hp"
last_stand = { line = "hold" }
enrage = { when = { round = 10 }, line = "rage", attack = { mul = 1.5 }, moves = [{ spell = 0x4B }] }

[[phases]]
moves = [{ attack = 0, weight = 3 }, { spell = 0x25, weight = 1, target = "random" }]

[[phases]]
when = { hp_below_pct = 60 }
line = "two"
summon = [{ kind = "monster", id = 129 }, "guard"]

[[phases]]
when = { hp_below_pct = 30, allies_down = 2 }
line = "three"
moves = [{ attack = 1, hits = 3 }, { spell = 0x63, charge = true, charge_line = "ult", cooldown = 2 }]
"#;

    fn obs(hp: u16, round: u16, allies_down: u8) -> Observation {
        Observation {
            hp,
            max_hp: 1000,
            round,
            allies_down,
        }
    }

    #[test]
    fn phases_walk_forward_in_order_and_never_back() {
        let mut run = BossRun::new(script(THREE));
        assert_eq!(run.observe(&obs(1000, 1, 0)), [Change::Phase(0)]);
        assert_eq!(run.moves().len(), 2);
        assert!(run.observe(&obs(700, 2, 0)).is_empty());
        // 59%: phase 1, which keeps phase 0's moves.
        assert_eq!(run.observe(&obs(590, 2, 0)), [Change::Phase(1)]);
        assert_eq!(run.changed(Change::Phase(1)).unwrap().summon, [129, 500]);
        assert_eq!(run.moves().len(), 2);
        assert_eq!(run.moves()[0].target, TargetRule::LowestHp);
        assert_eq!(run.moves()[1].target, TargetRule::Random);
        // Healed back up: no going back.
        assert!(run.observe(&obs(1000, 3, 0)).is_empty());
        assert_eq!(run.phase, Some(1));
        // Two allies down opens phase 2 at any HP.
        assert_eq!(run.observe(&obs(1000, 3, 2)), [Change::Phase(2)]);
        assert_eq!(run.moves()[0].kind, MoveKind::Attack { index: 1, hits: 3 });
    }

    #[test]
    fn one_check_can_begin_several_phases_and_the_enrage() {
        let mut run = BossRun::new(script(THREE));
        assert_eq!(
            run.observe(&obs(100, 10, 0)),
            [
                Change::Phase(0),
                Change::Phase(1),
                Change::Phase(2),
                Change::Enrage
            ]
        );
        assert!(run.enraged);
        // The enrage's moves stand over the phase's.
        assert_eq!(run.moves().len(), 1);
        assert_eq!(run.moves()[0].kind, MoveKind::Spell(0x4B));
        // It fires once.
        assert!(run.observe(&obs(100, 11, 0)).is_empty());
    }

    #[test]
    fn picks_are_deterministic_weighted_draws() {
        let pick_with = |seq: &[u32]| {
            let mut run = BossRun::new(script(THREE));
            run.observe(&obs(1000, 1, 0));
            let mut it = seq.iter().copied();
            let mut rand = move || it.next().unwrap();
            seq.iter()
                .map(|_| match run.pick(&mut rand) {
                    Pick::Move(m) => m.kind,
                    p => panic!("{p:?}"),
                })
                .collect::<Vec<_>>()
        };
        // Weights 3:1 - draws 0..=2 swing, 3 casts.
        let a = MoveKind::Attack { index: 0, hits: 1 };
        let s = MoveKind::Spell(0x25);
        assert_eq!(pick_with(&[0, 2, 3, 7, 4]), [a, a, s, s, a]);
        assert_eq!(pick_with(&[0, 2, 3, 7, 4]), pick_with(&[0, 2, 3, 7, 4]));
    }

    #[test]
    fn a_telegraphed_move_charges_then_lands_and_cools_down() {
        let mut run = BossRun::new(script(THREE));
        run.observe(&obs(100, 1, 0));
        // Draw 1 of weight 1:1 picks the ultimate.
        let mut rand = || 1u32;
        let Pick::Charge(m) = run.pick(&mut rand) else {
            panic!("charge")
        };
        assert_eq!(m.charge_line.as_deref(), Some("ult"));
        let mut never = || -> u32 { panic!("a due charge draws nothing") };
        assert_eq!(run.pick(&mut never), Pick::Move(m));
        // Two turns of cooldown: only the swing is ready (no draw).
        for _ in 0..2 {
            assert!(matches!(
                run.pick(&mut never),
                Pick::Move(CompiledMove {
                    kind: MoveKind::Attack { .. },
                    ..
                })
            ));
        }
        assert!(matches!(run.pick(&mut rand), Pick::Charge(_)));
    }

    #[test]
    fn last_stand_holds_once() {
        let mut run = BossRun::new(script(THREE));
        assert_eq!(run.spend_last_stand(), Some(Some("hold".into())));
        assert_eq!(run.spend_last_stand(), None);
        let mut plain = BossRun::new(script("[[phases]]\nmoves = [{ attack = 0 }]"));
        assert_eq!(plain.spend_last_stand(), None);
    }

    #[test]
    fn a_script_without_moves_keeps_retail_picks() {
        let s = script("immune = [\"sleep\"]");
        assert!(!s.picks_moves());
        let mut run = BossRun::new(s);
        assert!(run.observe(&obs(1, 1, 0)).is_empty());
        assert_eq!(run.pick(&mut || 0), Pick::Retail);
    }

    #[test]
    fn targeting_rules() {
        let c = [
            Candidate {
                slot: 0,
                hp: 900,
                attack: 300,
                mp: 10,
                heals: false,
            },
            Candidate {
                slot: 1,
                hp: 200,
                attack: 500,
                mp: 40,
                heals: false,
            },
            Candidate {
                slot: 2,
                hp: 200,
                attack: 100,
                mp: 5,
                heals: true,
            },
        ];
        let mut never = || -> u32 { panic!("no draw") };
        assert_eq!(choose_target(TargetRule::LowestHp, &c, &mut never), Some(1));
        assert_eq!(
            choose_target(TargetRule::HighestAttack, &c, &mut never),
            Some(1)
        );
        assert_eq!(choose_target(TargetRule::Healer, &c, &mut never), Some(2));
        assert_eq!(choose_target(TargetRule::Random, &c, &mut || 5), Some(2));
        assert_eq!(choose_target(TargetRule::Random, &[], &mut || 0), None);
        // No healer: the most MP.
        let no = [c[0], c[1]];
        assert_eq!(choose_target(TargetRule::Healer, &no, &mut never), Some(1));
    }

    #[test]
    fn hp_trigger_is_strictly_below() {
        let t = Trigger {
            hp_below_pct: Some(50),
            ..Default::default()
        };
        assert!(!triggered(&t, &obs(500, 1, 0)));
        assert!(triggered(&t, &obs(499, 1, 0)));
    }

    #[test]
    fn banner_text_is_the_first_page_joined() {
        let l: legaia_expansion_format::dialog::DialogLine =
            toml::from_str("text = \"The forge\\nwakes!\\n\\nSecond page.\"").unwrap();
        assert_eq!(banner_text(&l), "The forge wakes!");
    }

    #[test]
    fn stock_banners_read_the_packs_wording_first() {
        assert_eq!(
            stock_banner(None, BANNER_ENRAGED, "Warden"),
            "Warden is enraged!"
        );
        let d: legaia_expansion_format::dialog::DialogSection = toml::from_str(
            "[lines.boss_last_stand]\ntext = \"<name> ne tombe pas !\"\n[lines.boss_charging]\ntext = \"\"\n",
        )
        .unwrap();
        assert_eq!(
            stock_banner(Some(&d), BANNER_LAST_STAND, "Warden"),
            "Warden ne tombe pas !"
        );
        // An empty line falls back to the engine's wording.
        assert_eq!(
            stock_banner(Some(&d), BANNER_CHARGING, "Warden"),
            "Warden is gathering power!"
        );
    }
}
