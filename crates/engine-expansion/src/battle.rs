//! The battle half of a pack: its `battle.toml` compiled against the engine's
//! battle tables, and the kernels that build the monsters a fight seats.
//!
//! # Pack monsters fight under their base monster's id
//!
//! The monster id a battle seat carries keys everything read off the disc for
//! that seat: the mesh, the action clips, the AI's swing entries, the summon
//! side-band. A pack monster is a retail monster with new numbers, so it keeps
//! its base monster's id and the engine swaps its numbers into the catalog for
//! the length of the fight ([`BattleRuntime::shadowed`]). That is why one
//! formation holds one version of each base monster
//! ([`legaia_expansion_format::battle::BattleSection::problems`]).
//!
//! # Formation ids
//!
//! Pack formations register in the `World`'s formation table beside the
//! scene's own MAN rows, at [`EXPANSION_FORMATION_BASE`] plus their index in
//! key order. MAN row ids are a scene's row indices and stay far below it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot, MonsterDef};
use legaia_expansion_format::battle::{
    self as fmt, BattleSection, MonsterRef, Stat, local_key, qualify,
};
use legaia_expansion_format::items::ItemRef;
use legaia_expansion_format::refs::DiscRef;

use crate::battle_waves::CompiledWave;

/// The first formation id a pack formation takes.
pub const EXPANSION_FORMATION_BASE: u16 = 0xE000;

/// The ceiling a pack monster's record stats clamp to. The battle loader's
/// scripted profile doubles both defence facets in 16 bits, so a record stat
/// above this would wrap.
pub const RECORD_STAT_CAP: u16 = 0x3FFF;

/// The global BGM id of a sound-test track (`2000 + index`), the id space the
/// field VM's music op and the battle swap use.
pub const BGM_GLOBAL_BASE: u16 = 2000;

/// One compiled pack formation.
#[derive(Debug, Clone)]
pub struct CompiledFormation {
    /// The qualified key (`rebirth:ash_court`).
    pub key: String,
    /// The engine formation, its slots on base monster ids.
    pub def: FormationDef,
    /// Which base ids fight as a pack monster, and as which (local key),
    /// across every wave.
    pub variants: BTreeMap<u16, String>,
    /// The waves after the first ([`crate::battle_waves`]); empty for a
    /// single-wave formation.
    pub waves: Vec<CompiledWave>,
    /// Base ids the scripted monsters of the fight may summon that no wave
    /// seats ([`crate::boss_script`]).
    pub summons: Vec<u16>,
    /// The boss scripts of the fight, by the base id their monster fights
    /// under.
    pub scripts: BTreeMap<u16, Arc<crate::boss_script::CompiledScript>>,
}

impl CompiledFormation {
    /// The formation with every wave's monsters in its slots, the first
    /// wave's first: what the catalog swap plans over, and what the
    /// victory spoils of a fought-out wave run sum.
    pub fn all_waves_def(&self) -> FormationDef {
        let mut def = self.def.clone();
        def.slots.extend(
            self.waves
                .iter()
                .flat_map(|w| w.monsters.iter().map(|&id| FormationSlot::new(id))),
        );
        def.slots
            .extend(self.summons.iter().map(|&id| FormationSlot::new(id)));
        def
    }
}

/// One compiled encounter override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledEncounter {
    /// `(formation id, weight)`.
    pub entries: Vec<(u16, u32)>,
    pub retail_weight: u32,
    /// The pack's own step range, when it owns the scene's rate.
    pub steps: Option<(u16, u16)>,
}

/// One compiled boss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledBoss {
    pub formation_id: u16,
    /// Global BGM id of the battle track, when it is a retail cue.
    pub bgm: Option<u16>,
    /// The battle track as written: a retail cue or a pack track
    /// (`crate::audio::music_id` resolves it against `audio.toml`).
    pub music: Option<legaia_expansion_format::audio::MusicRef>,
    /// `(item, count)`: retail ids and pack items.
    pub reward_items: Vec<(ItemRef, u8)>,
    pub reward_gold: u32,
    pub once: bool,
}

/// A pack's `battle.toml`, resolved to engine ids.
#[derive(Debug, Clone, Default)]
pub struct CompiledBattle {
    pub pack_id: String,
    /// Indexed by `formation id - EXPANSION_FORMATION_BASE`.
    pub formations: Vec<CompiledFormation>,
    /// Pack monsters by local key.
    pub monsters: BTreeMap<String, fmt::MonsterDef>,
    /// Encounter overrides by scene name.
    pub encounters: BTreeMap<String, CompiledEncounter>,
    /// Bosses by local key.
    pub bosses: BTreeMap<String, CompiledBoss>,
}

/// The retail item id an [`ItemRef`] names; `None` for a pack item.
pub fn item_id(r: &ItemRef) -> Option<u8> {
    r.retail_id()
}

impl CompiledBattle {
    /// Resolve `section` for pack `pack_id`. Fails with the section's
    /// [`BattleSection::problems`] when there are any.
    pub fn compile(pack_id: &str, section: &BattleSection) -> Result<Self, Vec<String>> {
        let problems = section.problems(pack_id);
        if !problems.is_empty() {
            return Err(problems);
        }
        let mut out = CompiledBattle {
            pack_id: pack_id.to_string(),
            monsters: section.monsters.clone(),
            ..Default::default()
        };
        // Bosses that refuse Run make their formation a scripted row.
        let scripted_by_boss: BTreeSet<&str> = section
            .bosses
            .values()
            .filter(|b| b.no_escape)
            .filter_map(|b| local_key(pack_id, &b.formation))
            .collect();
        for (i, (key, f)) in section.formations.iter().enumerate() {
            let id = EXPANSION_FORMATION_BASE + i as u16;
            let mut variants = BTreeMap::new();
            let mut base_of = |r: &MonsterRef| match r {
                MonsterRef::Pack(k) => {
                    let k = local_key(pack_id, k).unwrap_or(k);
                    let DiscRef::Monster { id } = section.monsters[k].base else {
                        unreachable!("checked by problems()")
                    };
                    variants.insert(id, k.to_string());
                    id
                }
                MonsterRef::Disc(DiscRef::Monster { id }) => *id,
                MonsterRef::Disc(_) => unreachable!("checked by problems()"),
            };
            let slots: Vec<FormationSlot> = f
                .monsters
                .iter()
                .map(|r| FormationSlot::new(base_of(r)))
                .collect();
            let waves: Vec<CompiledWave> = f
                .waves
                .iter()
                .map(|w| {
                    let monsters: Vec<u16> = w.monsters.iter().map(&mut base_of).collect();
                    let refill = w.arrival == fmt::WaveArrival::Refill;
                    CompiledWave {
                        count: if refill {
                            w.count.unwrap_or(monsters.len() as u16)
                        } else {
                            monsters.len() as u16
                        },
                        max_standing: w
                            .max_standing
                            .filter(|_| refill)
                            .unwrap_or(fmt::MAX_FORMATION_SLOTS as u8),
                        arrival: w.arrival,
                        monsters,
                    }
                })
                .collect();
            // What the fight's scripted monsters may summon joins its
            // catalog swap, and each pack monster's script comes along.
            let mut summons = Vec::new();
            let seated: BTreeSet<u16> = slots
                .iter()
                .map(|s| s.monster_id)
                .chain(
                    waves
                        .iter()
                        .flat_map(|w: &CompiledWave| w.monsters.iter().copied()),
                )
                .collect();
            for r in section.fight_monsters(pack_id, f) {
                let id = base_of(r);
                if !seated.contains(&id) && !summons.contains(&id) {
                    summons.push(id);
                }
            }
            let mut scripts = BTreeMap::new();
            for (&id, k) in &variants {
                if let Some(script) = &section.monsters[k.as_str()].script {
                    let compiled = crate::boss_script::CompiledScript::compile(script, |r| {
                        Some(match r {
                            MonsterRef::Pack(k) => {
                                let k = local_key(pack_id, k).unwrap_or(k);
                                match section.monsters.get(k)?.base {
                                    DiscRef::Monster { id } => id,
                                    _ => return None,
                                }
                            }
                            MonsterRef::Disc(DiscRef::Monster { id }) => *id,
                            MonsterRef::Disc(_) => return None,
                        })
                    });
                    scripts.insert(id, Arc::new(compiled));
                }
            }
            let label = if f.label.is_empty() {
                key.clone()
            } else {
                f.label.clone()
            };
            let scripted = f.scripted || scripted_by_boss.contains(key.as_str());
            out.formations.push(CompiledFormation {
                key: qualify(pack_id, key),
                def: FormationDef::new(id, slots)
                    .with_label(label)
                    .with_header_flags(u8::from(scripted)),
                variants,
                waves,
                summons,
                scripts,
            });
        }
        for e in &section.encounters {
            let entries = e
                .formations
                .iter()
                .filter(|w| w.weight > 0)
                .filter_map(|w| Some((out.formation_id(&w.formation)?, w.weight)))
                .collect();
            out.encounters.insert(
                e.scene.clone(),
                CompiledEncounter {
                    entries,
                    retail_weight: e.retail_weight,
                    steps: e.steps.map(|[lo, hi]| (lo, hi)),
                },
            );
        }
        for (key, b) in &section.bosses {
            let formation_id = out
                .formation_id(&b.formation)
                .expect("checked by problems()");
            out.bosses.insert(
                key.clone(),
                CompiledBoss {
                    formation_id,
                    bgm: b.music.as_ref().and_then(|m| match m {
                        legaia_expansion_format::audio::MusicRef::Bgm { id } => {
                            Some(BGM_GLOBAL_BASE.saturating_add(*id))
                        }
                        _ => None,
                    }),
                    music: b.music.clone(),
                    reward_items: b
                        .reward_items
                        .iter()
                        .filter(|r| r.item.is_item())
                        .map(|r| (r.item.clone(), r.count))
                        .collect(),
                    reward_gold: b.reward_gold,
                    once: b.once,
                },
            );
        }
        Ok(out)
    }

    /// The engine id of formation `reference` (local or qualified key).
    pub fn formation_id(&self, reference: &str) -> Option<u16> {
        let q = qualify(&self.pack_id, reference);
        self.formations
            .iter()
            .position(|f| f.key == q)
            .map(|i| EXPANSION_FORMATION_BASE + i as u16)
    }

    /// The compiled formation behind engine id `id`, when it is a pack one.
    pub fn formation(&self, id: u16) -> Option<&CompiledFormation> {
        let i = id.checked_sub(EXPANSION_FORMATION_BASE)?;
        self.formations.get(usize::from(i))
    }

    /// The boss `reference` names (local or qualified key).
    pub fn boss(&self, reference: &str) -> Option<&CompiledBoss> {
        self.bosses.get(local_key(&self.pack_id, reference)?)
    }

    /// Every retail monster id a pack fight can seat: the bases of the pack
    /// monsters and the retail slots of the pack formations. A host merges
    /// these from the disc's monster archive into the catalog so the pack's
    /// fights resolve real stats in any scene.
    pub fn base_monster_ids(&self) -> Vec<u16> {
        let mut ids: BTreeSet<u16> = self
            .formations
            .iter()
            .flat_map(|f| f.all_waves_def().slots.into_iter().map(|s| s.monster_id))
            .collect();
        ids.extend(self.monsters.values().filter_map(|m| match m.base {
            DiscRef::Monster { id } => Some(id),
            _ => None,
        }));
        ids.into_iter().collect()
    }
}

/// Scale `v` by `permille` with the engine's slider rule: round half up, a
/// zero stays zero, a non-zero one stays at least `1`; saturates at `cap`.
pub fn scale_permille(v: u32, permille: u32, cap: u32) -> u32 {
    if v == 0 || permille == 1000 {
        return v.min(cap);
    }
    let scaled = (u64::from(v) * u64::from(permille) + 500) / 1000;
    scaled.clamp(1, u64::from(cap)) as u32
}

fn apply_stat(v: u16, stat: Option<Stat>, cap: u16) -> u16 {
    match stat {
        None => v,
        Some(Stat::Set(x)) => x.min(u32::from(cap)) as u16,
        Some(s) => {
            scale_permille(u32::from(v), s.permille().unwrap_or(1000), u32::from(cap)) as u16
        }
    }
}

/// Re-derive the installed (scripted-profile) fields of a record-backed def
/// from its raw block, as `monster_def_from_record` builds them.
fn reinstall(def: &mut MonsterDef) {
    if def.raw_stats == [0; 6] {
        return;
    }
    let bs = def.installed_stats(true);
    def.agl = bs[0];
    def.attack = bs[1];
    def.udf = bs[2];
    def.ldf = bs[3];
    def.intel = bs[4];
    def.speed = bs[5];
    let int_byte = bs[4].min(u16::from(u8::MAX)) as u8;
    def.accuracy = int_byte;
    def.evasion = int_byte;
}

/// Build the pack monster `ov` on its base monster's catalog entry `base`.
/// The result keeps `base`'s id (see the module docs).
pub fn derive_monster(base: &MonsterDef, ov: &fmt::MonsterDef) -> MonsterDef {
    let mut d = base.clone();
    if let Some(name) = &ov.name {
        d.name = name.clone();
    }
    d.hp = apply_stat(d.hp, ov.hp, u16::MAX);
    d.mp = apply_stat(d.mp, ov.mp, u16::MAX);
    d.exp = apply_stat(d.exp, ov.exp, u16::MAX);
    d.gold = apply_stat(d.gold, ov.gold, u16::MAX);
    if d.raw_stats == [0; 6] {
        // A synthetic catalog entry: its fields are the fight's stats.
        d.agl = apply_stat(d.agl, ov.agility, RECORD_STAT_CAP);
        d.attack = apply_stat(d.attack, ov.attack, RECORD_STAT_CAP);
        d.udf = apply_stat(d.udf, ov.defence, RECORD_STAT_CAP);
        d.ldf = apply_stat(d.ldf, ov.defence, RECORD_STAT_CAP);
        d.intel = apply_stat(d.intel, ov.intelligence, RECORD_STAT_CAP);
        d.speed = apply_stat(d.speed, ov.speed, RECORD_STAT_CAP);
    } else {
        // `[AGL, ATK, UDF, LDF, INT, SPD]`, the record block the battle
        // loader boosts per fight.
        let r = &mut d.raw_stats;
        r[0] = apply_stat(r[0], ov.agility, RECORD_STAT_CAP);
        r[1] = apply_stat(r[1], ov.attack, RECORD_STAT_CAP);
        r[2] = apply_stat(r[2], ov.defence, RECORD_STAT_CAP);
        r[3] = apply_stat(r[3], ov.defence, RECORD_STAT_CAP);
        r[4] = apply_stat(r[4], ov.intelligence, RECORD_STAT_CAP);
        r[5] = apply_stat(r[5], ov.speed, RECORD_STAT_CAP);
        reinstall(&mut d);
    }
    if let Some(drop) = &ov.drop {
        // A pack item drop has no retail id: the fight lends it one
        // ([`BattlePlan::pack_drops`]), so the record seats none here.
        d.drop_item = item_id(&drop.item).filter(|&i| i != 0);
        d.drop_chance_pct = drop.chance_pct;
    }
    if let Some(e) = ov.element {
        d.element = e;
    }
    if let Some(spells) = &ov.spells {
        d.magic_attacks = spells.clone();
    }
    d
}

/// Scale a monster's fight stats (HP, MP, attack, both defences, INT, SPD)
/// by `permille`, and its EXP and gold too when `rewards`. AGL, the action
/// gauge, is left alone: scaling it would change how many swings a turn
/// holds, which is a behaviour change rather than a stat one.
pub fn scale_monster(def: &MonsterDef, permille: u32, rewards: bool) -> MonsterDef {
    let mut d = def.clone();
    if permille == 1000 {
        return d;
    }
    let sc = |v: u16, cap: u16| scale_permille(u32::from(v), permille, u32::from(cap)) as u16;
    d.hp = sc(d.hp, u16::MAX);
    d.mp = sc(d.mp, u16::MAX);
    if rewards {
        d.exp = sc(d.exp, u16::MAX);
        d.gold = sc(d.gold, u16::MAX);
    }
    if d.raw_stats == [0; 6] {
        d.attack = sc(d.attack, RECORD_STAT_CAP);
        d.udf = sc(d.udf, RECORD_STAT_CAP);
        d.ldf = sc(d.ldf, RECORD_STAT_CAP);
        d.intel = sc(d.intel, RECORD_STAT_CAP);
        d.speed = sc(d.speed, RECORD_STAT_CAP);
    } else {
        for v in d.raw_stats.iter_mut().skip(1) {
            *v = sc(*v, RECORD_STAT_CAP);
        }
        reinstall(&mut d);
    }
    d
}

/// The catalog entries one fight seats in place of the scene's, and the
/// steal entries it overrides.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BattlePlan {
    pub defs: Vec<MonsterDef>,
    /// `(base id, chance %, item id)`.
    pub steals: Vec<(u16, u8, u8)>,
    /// `(base id, chance %, pack item key)`: a pack monster's drop that is a
    /// pack item. The `World` lends the key a fight alias and writes it into
    /// the seated def's `drop_item`.
    pub pack_drops: Vec<(u16, u8, String)>,
    /// `(base id, chance %, pack item key)`: the same for a steal.
    pub pack_steals: Vec<(u16, u8, String)>,
}

/// Plan the catalog swap for a fight on `formation`.
///
/// `pack` is the compiled formation when the fight is a pack one; `lookup`
/// answers a base monster's catalog entry; `scale_permille` is the
/// level-band scaling for this fight (`1000` = none) and `scale_rewards`
/// whether it reaches EXP and gold. Base ids the catalog cannot answer are
/// left out: the fight seats them as it would without a pack.
pub fn plan_battle(
    compiled: &CompiledBattle,
    formation: &FormationDef,
    lookup: impl Fn(u16) -> Option<MonsterDef>,
    scale_permille: u32,
    scale_rewards: bool,
) -> BattlePlan {
    let pack = compiled.formation(formation.formation_id);
    let mut plan = BattlePlan::default();
    let mut seen = BTreeSet::new();
    for slot in &formation.slots {
        let id = slot.monster_id;
        if !seen.insert(id) {
            continue;
        }
        let variant = pack
            .and_then(|f| f.variants.get(&id))
            .and_then(|k| compiled.monsters.get(k));
        if variant.is_none() && scale_permille == 1000 {
            continue;
        }
        let Some(base) = lookup(id) else {
            continue;
        };
        let def = match variant {
            Some(ov) => {
                if let Some(s) = &ov.steal {
                    if let Some(item) = item_id(&s.item) {
                        plan.steals.push((id, s.chance_pct, item));
                    } else if let Some(key) = s
                        .item
                        .pack_key()
                        .and_then(|k| local_key(&compiled.pack_id, k))
                    {
                        plan.pack_steals.push((id, s.chance_pct, key.to_string()));
                    }
                }
                if let Some(d) = &ov.drop
                    && let Some(key) = d
                        .item
                        .pack_key()
                        .and_then(|k| local_key(&compiled.pack_id, k))
                {
                    plan.pack_drops.push((id, d.chance_pct, key.to_string()));
                }
                derive_monster(&base, ov)
            }
            None => base,
        };
        plan.defs
            .push(scale_monster(&def, scale_permille, scale_rewards));
    }
    plan
}

/// Pick a formation from an encounter override with one `rand()` draw.
/// `retail` is the formation the retail roll produced, if any; it competes
/// with [`CompiledEncounter::retail_weight`]. `None` when nothing has weight.
pub fn pick_formation(enc: &CompiledEncounter, retail: Option<u16>, rand: u32) -> Option<u16> {
    let retail_w = retail.map_or(0, |_| enc.retail_weight);
    let total: u64 =
        enc.entries.iter().map(|&(_, w)| u64::from(w)).sum::<u64>() + u64::from(retail_w);
    if total == 0 {
        return None;
    }
    let mut r = u64::from(rand) % total;
    for &(id, w) in &enc.entries {
        if r < u64::from(w) {
            return Some(id);
        }
        r -= u64::from(w);
    }
    retail
}

/// The steps until the next pack encounter, drawn in `lo..=hi`.
pub fn draw_steps((lo, hi): (u16, u16), rand: u32) -> u16 {
    let span = u32::from(hi.saturating_sub(lo)) + 1;
    lo + (rand % span) as u16
}

/// The battle-side runtime state of an active pack.
#[derive(Debug, Clone, Default)]
pub struct BattleRuntime {
    /// The pack's compiled battle tables.
    pub compiled: Option<Arc<CompiledBattle>>,
    /// Catalog entries the current fight replaced: `(id, the scene's entry)`,
    /// restored when the fight ends.
    pub shadowed: Vec<(u16, Option<MonsterDef>)>,
    /// Steal overrides for the current fight: base id -> `(chance %, item)`.
    pub steals: BTreeMap<u16, (u8, u8)>,
    /// The boss being fought, by local key.
    pub active_boss: Option<String>,
    /// A boss intro cutscene in play: `(cutscene id, boss key)`. The fight
    /// starts as it ends (`World::start_expansion_boss`).
    pub boss_intro: Option<(String, String)>,
    /// The boss whose intro just played: its next start fights at once.
    pub intro_played: Option<String>,
    /// Battle track for the next fight's swap (a global BGM id).
    pub battle_bgm: Option<u16>,
    /// Bosses marked [`CompiledBoss::once`] the party has beaten.
    pub cleared_bosses: BTreeSet<String>,
    /// The pack-owned encounter counter: `(scene, steps left)`.
    pub step_counter: Option<(String, u16)>,
    /// The waves still to come in the current fight, when its formation
    /// has more than one ([`crate::battle_waves`]).
    pub waves: Option<crate::battle_waves::WaveRun>,
    /// Actor slots a wave seated a new monster in since the hosts last
    /// looked: their battle meshes need binding
    /// (`World::take_expansion_reseated_monsters`).
    pub reseated: Vec<usize>,
    /// The current fight's boss scripts, by the base id each scripted
    /// monster fights under ([`CompiledFormation::scripts`]).
    pub fight_scripts: BTreeMap<u16, Arc<crate::boss_script::CompiledScript>>,
    /// The script state of each scripted seat: actor slot -> `(monster id,
    /// run)`. A seat a wave or a summon re-seats starts a fresh run.
    pub boss_runs: BTreeMap<usize, (u16, crate::boss_script::BossRun)>,
    /// Monsters a script summoned, seated as the next round opens.
    pub pending_summons: Vec<u16>,
    /// The current fight's round, the first being `1` (`0` before it opens).
    pub boss_round: u16,
    /// A scripted swing stream for the turn being armed: archive entry
    /// indices that replace the AGL budget's picks.
    pub forced_strike: Option<Vec<u8>>,
    /// The monster block's seat layout count for a fight with no waves
    /// (a waved fight's is [`crate::battle_waves::WaveRun::layout`]).
    pub boss_layout: u8,
    /// Boss banner lines waiting for the battle's message line to clear
    /// (a phase change and a charge on one turn show one after the other).
    pub boss_banners: std::collections::VecDeque<String>,
    /// The boss phases' screen layer this fight: a phase's flash and its
    /// held tint (`[[...script.phases]]` `flash` / `tint`).
    pub phase_screen: crate::cutscene::ScreenLayer,
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::battle::{ItemChance, WeightedFormation};

    fn record_backed(id: u16) -> MonsterDef {
        let mut d = MonsterDef::new(id, "Caruban", 2000, 0);
        d.raw_stats = [40, 100, 50, 60, 80, 30];
        reinstall(&mut d);
        d.exp = 500;
        d.gold = 300;
        d
    }

    fn ov(base: u16) -> fmt::MonsterDef {
        toml::from_str(&format!("base = {{ kind = \"monster\", id = {base} }}")).unwrap()
    }

    fn section() -> BattleSection {
        toml::from_str(
            r#"
[monsters.ashen]
base = { kind = "monster", id = 73 }
name = "Ashen Caruban"
hp = 30000
attack = { mul = 2.0 }
exp = { mul = 10.0 }
steal = { item = { kind = "item", id = 9 }, chance_pct = 25 }

[formations.b_court]
monsters = ["ashen", { kind = "monster", id = 12 }]

[formations.a_pair]
monsters = [{ kind = "monster", id = 12 }, { kind = "monster", id = 12 }]

[[encounters]]
scene = "rikuroa"
formations = [{ formation = "a_pair", weight = 1 }, { formation = "b_court", weight = 3 }]

[bosses.king]
formation = "b_court"
music = { kind = "bgm", id = 30 }
reward_items = [{ item = { kind = "item", id = 5 }, count = 2 }]
"#,
        )
        .unwrap()
    }

    #[test]
    fn formations_take_ids_in_key_order_on_base_monster_ids() {
        let c = CompiledBattle::compile("rebirth", &section()).unwrap();
        assert_eq!(c.formation_id("a_pair"), Some(EXPANSION_FORMATION_BASE));
        assert_eq!(
            c.formation_id("rebirth:b_court"),
            Some(EXPANSION_FORMATION_BASE + 1)
        );
        let court = c.formation(EXPANSION_FORMATION_BASE + 1).unwrap();
        let ids: Vec<u16> = court.def.slots.iter().map(|s| s.monster_id).collect();
        assert_eq!(ids, [73, 12]);
        assert_eq!(court.variants.get(&73).map(String::as_str), Some("ashen"));
        // The no-escape boss marks its formation scripted; the other stays a
        // plain row.
        assert_eq!(court.def.per_battle_flags(), 0x80);
        assert_eq!(
            c.formation(EXPANSION_FORMATION_BASE)
                .unwrap()
                .def
                .per_battle_flags(),
            0
        );
        let king = c.boss("king").unwrap();
        assert_eq!(king.bgm, Some(2030));
        assert_eq!(
            king.reward_items,
            [(ItemRef::Disc(DiscRef::Item { id: 5 }), 2)]
        );
        assert_eq!(c.base_monster_ids(), [12, 73]);
        assert!(c.formation(12).is_none());
    }

    #[test]
    fn waves_compile_onto_base_ids_and_join_the_swap() {
        let mut s = section();
        let waves: fmt::FormationDef = toml::from_str(
            r#"
monsters = [{ kind = "monster", id = 12 }]

[[waves]]
monsters = ["ashen", { kind = "monster", id = 12 }]

[[waves]]
monsters = [{ kind = "monster", id = 40 }]
arrival = "refill"
count = 3
max_standing = 2
"#,
        )
        .unwrap();
        s.formations.insert("c_gauntlet".into(), waves);
        let c = CompiledBattle::compile("rebirth", &s).unwrap();
        let id = c.formation_id("c_gauntlet").unwrap();
        let f = c.formation(id).unwrap();
        // The engine formation is the first wave alone.
        assert_eq!(f.def.slots.len(), 1);
        assert_eq!(f.waves.len(), 2);
        assert_eq!(f.waves[0].monsters, [73, 12]);
        assert_eq!((f.waves[0].count, f.waves[0].max_standing), (2, 4));
        assert_eq!(
            (
                f.waves[1].arrival,
                f.waves[1].count,
                f.waves[1].max_standing
            ),
            (fmt::WaveArrival::Refill, 3, 2)
        );
        // A variant met only in a later wave still swaps.
        assert_eq!(f.variants.get(&73).map(String::as_str), Some("ashen"));
        let all = f.all_waves_def();
        let ids: Vec<u16> = all.slots.iter().map(|s| s.monster_id).collect();
        assert_eq!(ids, [12, 73, 12, 40]);
        assert_eq!(all.formation_id, id);
        let plan = plan_battle(&c, &all, |i| Some(record_backed(i)), 1000, false);
        assert_eq!(plan.defs.len(), 1);
        assert_eq!(plan.defs[0].name, "Ashen Caruban");
        assert!(c.base_monster_ids().contains(&40));
        // Formations without waves compile with none.
        assert!(
            c.formation(c.formation_id("a_pair").unwrap())
                .unwrap()
                .waves
                .is_empty()
        );
    }

    #[test]
    fn a_bad_section_does_not_compile() {
        let mut s = section();
        s.encounters[0].formations.push(WeightedFormation {
            formation: "missing".into(),
            weight: 1,
        });
        let err = CompiledBattle::compile("rebirth", &s).unwrap_err();
        assert!(err[0].contains("unknown formation `missing`"));
    }

    #[test]
    fn a_derived_monster_scales_the_record_block_and_reinstalls() {
        let base = record_backed(73);
        let mut o = ov(73);
        o.hp = Some(Stat::Set(30000));
        o.attack = Some(Stat::Scale { mul: 2.0 });
        o.defence = Some(Stat::Set(200));
        o.exp = Some(Stat::Scale { mul: 10.0 });
        o.drop = Some(ItemChance {
            item: ItemRef::Disc(DiscRef::Item { id: 7 }),
            chance_pct: 40,
        });
        o.element = Some(3);
        let d = derive_monster(&base, &o);
        assert_eq!(d.id, 73);
        assert_eq!(d.name, "Caruban");
        assert_eq!(d.hp, 30000);
        assert_eq!(d.raw_stats, [40, 200, 200, 200, 80, 30]);
        // Scripted profile: ATK x5/4, defences x2.
        assert_eq!((d.attack, d.udf, d.ldf), (250, 400, 400));
        // The random-encounter profile re-derives from the new raw block.
        assert_eq!(d.installed_stats(false)[1], 200);
        assert_eq!(d.exp, 5000);
        assert_eq!(
            (d.drop_item, d.drop_chance_pct, d.element),
            (Some(7), 40, 3)
        );
    }

    #[test]
    fn a_derived_synthetic_monster_edits_its_fields() {
        let base = MonsterDef::new(3, "Wolf", 100, 20);
        let mut o = ov(3);
        o.name = Some("Dire Wolf".into());
        o.attack = Some(Stat::Scale { mul: 1.5 });
        o.hp = Some(Stat::Set(100_000));
        let d = derive_monster(&base, &o);
        assert_eq!(
            (d.name.as_str(), d.attack, d.hp),
            ("Dire Wolf", 30, u16::MAX)
        );
        assert_eq!(d.raw_stats, [0; 6]);
    }

    #[test]
    fn a_record_stat_never_wraps_the_scripted_boost() {
        let base = record_backed(73);
        let mut o = ov(73);
        o.defence = Some(Stat::Set(60_000));
        let d = derive_monster(&base, &o);
        assert_eq!(d.raw_stats[2], RECORD_STAT_CAP);
        assert_eq!(d.udf, RECORD_STAT_CAP * 2);
    }

    #[test]
    fn scaling_leaves_agl_and_optionally_rewards() {
        let base = record_backed(73);
        let s = scale_monster(&base, 2000, false);
        assert_eq!(s.hp, 4000);
        assert_eq!(s.raw_stats, [40, 200, 100, 120, 160, 60]);
        assert_eq!((s.exp, s.gold), (500, 300));
        let s = scale_monster(&base, 2000, true);
        assert_eq!((s.exp, s.gold), (1000, 600));
        assert_eq!(scale_monster(&base, 1000, true), base);
    }

    #[test]
    fn the_plan_swaps_variants_and_scaled_retail_slots_only() {
        let c = CompiledBattle::compile("rebirth", &section()).unwrap();
        let lookup = |id: u16| Some(record_backed(id));
        let court = c
            .formation(EXPANSION_FORMATION_BASE + 1)
            .unwrap()
            .def
            .clone();
        let plan = plan_battle(&c, &court, lookup, 1000, false);
        assert_eq!(plan.defs.len(), 1, "only the pack monster is swapped");
        assert_eq!(plan.defs[0].id, 73);
        assert_eq!(plan.defs[0].name, "Ashen Caruban");
        assert_eq!(plan.steals, [(73, 25, 9)]);
        // With scaling every distinct id is swapped once.
        let pair = c.formation(EXPANSION_FORMATION_BASE).unwrap().def.clone();
        let plan = plan_battle(&c, &pair, lookup, 1500, false);
        assert_eq!(plan.defs.len(), 1);
        assert_eq!(plan.defs[0].hp, 3000);
        // A retail formation (not a pack id) gets no variants.
        let retail = FormationDef::new(4, vec![FormationSlot::new(73)]);
        assert!(
            plan_battle(&c, &retail, lookup, 1000, false)
                .defs
                .is_empty()
        );
        // A base the catalog does not hold is left to the fight.
        assert!(
            plan_battle(&c, &court, |_| None, 1000, false)
                .defs
                .is_empty()
        );
    }

    #[test]
    fn the_weighted_pick_walks_entries_then_retail() {
        let enc = CompiledEncounter {
            entries: vec![(0xE000, 1), (0xE001, 3)],
            retail_weight: 2,
            steps: None,
        };
        let picks: Vec<_> = (0..6).map(|r| pick_formation(&enc, Some(9), r)).collect();
        assert_eq!(
            picks,
            [
                Some(0xE000),
                Some(0xE001),
                Some(0xE001),
                Some(0xE001),
                Some(9),
                Some(9)
            ]
        );
        // With no retail roll its weight drops out of the total.
        assert_eq!(pick_formation(&enc, None, 4), Some(0xE000));
        let empty = CompiledEncounter {
            entries: vec![],
            retail_weight: 0,
            steps: None,
        };
        assert_eq!(pick_formation(&empty, Some(9), 0), None);
    }

    #[test]
    fn steps_draw_inside_the_range() {
        assert_eq!(draw_steps((10, 10), 12345), 10);
        assert!((0..100).all(|r| (5..=8).contains(&draw_steps((5, 8), r))));
    }
}
