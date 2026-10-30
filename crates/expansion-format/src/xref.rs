//! The pack's reference model: every place a pack id is defined, set, read
//! or referenced, in one walk over the pack's JSON form.
//!
//! The JSON form is serde's form of [`ExpansionPack`] - the same keys as
//! the TOML and the form the browser workbench edits - so one model serves
//! the workbench's Find, its rename and its flow graph, and the validator's
//! flow lints ([`crate::flow`]). A [`Site`] carries the place in the
//! validator's own spelling (the file and the dotted field path inside it),
//! so a site is a jump target exactly as a validation issue is.
//!
//! What a value names is decided by where it sits: the field it is under,
//! the collection it is in and, for a tagged step or event, its `do` /
//! `kind`. A gate (the quest prerequisite shape) is recognised by its keys
//! wherever it appears, and each site records the gates on its way down -
//! what must hold for that site to take effect - which the reachability
//! pass reads.
//!
//! [`rename`] rewrites every site of one id in place: values keep their
//! qualification (`rebirth:ogre`) and the engine flags derived from an id
//! (`cutscene.<id>`, `map.<id>.<trigger>`) follow the id.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::ExpansionPack;
use crate::manifest::{is_valid_content_id, is_valid_pack_id};

/// What kind of pack id a site names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Monster,
    Formation,
    Boss,
    Encounter,
    Quest,
    Challenge,
    Flag,
    Counter,
    Card,
    Booster,
    Dealer,
    Item,
    ItemSet,
    Shop,
    Art,
    ArtChain,
    Spell,
    Map,
    Npc,
    Cutscene,
    Line,
    Mode,
    Track,
    Sound,
}

impl Kind {
    /// Every kind, in display order.
    pub const ALL: &'static [Kind] = &[
        Kind::Monster,
        Kind::Formation,
        Kind::Boss,
        Kind::Encounter,
        Kind::Quest,
        Kind::Challenge,
        Kind::Flag,
        Kind::Counter,
        Kind::Card,
        Kind::Booster,
        Kind::Dealer,
        Kind::Item,
        Kind::ItemSet,
        Kind::Shop,
        Kind::Art,
        Kind::ArtChain,
        Kind::Spell,
        Kind::Map,
        Kind::Npc,
        Kind::Cutscene,
        Kind::Line,
        Kind::Mode,
        Kind::Track,
        Kind::Sound,
    ];

    /// The name as written in JSON (`item_set`).
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Monster => "monster",
            Kind::Formation => "formation",
            Kind::Boss => "boss",
            Kind::Encounter => "encounter",
            Kind::Quest => "quest",
            Kind::Challenge => "challenge",
            Kind::Flag => "flag",
            Kind::Counter => "counter",
            Kind::Card => "card",
            Kind::Booster => "booster",
            Kind::Dealer => "dealer",
            Kind::Item => "item",
            Kind::ItemSet => "item_set",
            Kind::Shop => "shop",
            Kind::Art => "art",
            Kind::ArtChain => "art_chain",
            Kind::Spell => "spell",
            Kind::Map => "map",
            Kind::Npc => "npc",
            Kind::Cutscene => "cutscene",
            Kind::Line => "line",
            Kind::Mode => "mode",
            Kind::Track => "track",
            Kind::Sound => "sound",
        }
    }

    /// The kind named `s` (`as_str`'s inverse).
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    /// Is `id` a well-formed new id of this kind. Content ids are
    /// `[a-z0-9_]`; cards, boosters, dealers and modes also take `-`; a
    /// flag or counter is any text without spaces.
    pub fn is_valid_id(self, id: &str) -> bool {
        match self {
            Kind::Flag | Kind::Counter => {
                !id.is_empty() && id.len() <= 64 && !id.chars().any(char::is_whitespace)
            }
            Kind::Card | Kind::Booster | Kind::Dealer | Kind::Mode => is_valid_pack_id(id),
            _ => is_valid_content_id(id),
        }
    }
}

/// What a site does with the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The id is defined here.
    Defines,
    /// A flag or counter is written here (set, cleared, added to).
    Sets,
    /// The id is tested here: a gate, an objective that waits on it, the
    /// map a cutscene plays on.
    Reads,
    /// The id is used here: a fight started, a line said, a map entered, a
    /// reward granted.
    References,
}

/// One step of a JSON path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Seg {
    Key(String),
    Index(usize),
}

/// A JSON path into the pack.
pub type JsonPath = Vec<Seg>;

/// One place a pack id sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Site {
    pub kind: Kind,
    /// The local id (no pack qualifier, no engine-flag prefix).
    pub id: String,
    pub role: Role,
    /// The pack file (`battle.toml`, `maps/<id>.toml`).
    pub file: String,
    /// The validator field path inside it (`formations.pack.monsters[0]`).
    pub field: String,
    /// The site is an object key (a keyed definition), not a value.
    pub key: bool,
    /// The nearest enclosing definition: the quest a reward is in, the map
    /// a trigger is on, the NPC a line is said by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<(Kind, String)>,
    /// Text stored before the id (`rebirth:`, `cutscene.`).
    #[serde(skip)]
    pub prefix: String,
    /// Text stored after it (`.arrival` of `map.<id>.arrival`).
    #[serde(skip)]
    pub suffix: String,
    /// The JSON path of the value (or of the keyed object).
    #[serde(skip)]
    pub path: JsonPath,
    /// The gates (indices into [`Index::gates`]) that must hold for this
    /// site to take effect.
    #[serde(skip)]
    pub gates: Vec<usize>,
    /// The gate this site is a member of, when it is one.
    #[serde(skip)]
    pub in_gate: Option<usize>,
}

/// One gate (the quest prerequisite shape) found in the pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub path: JsonPath,
    pub file: String,
    pub field: String,
}

/// The pack's reference model.
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub sites: Vec<Site>,
    pub gates: Vec<Gate>,
}

/// The file a top-level pack key exports to.
fn section_file(key: &str) -> String {
    match key {
        "manifest" => "pack.toml".into(),
        other => format!("{other}.toml"),
    }
}

/// Split a JSON path into the pack file and the validator field path.
pub fn locate(path: &[Seg]) -> (String, String) {
    let (file, rest) = match path {
        [] => return ("pack.toml".into(), String::new()),
        [Seg::Key(m), Seg::Key(id), rest @ ..] if m == "maps" => (format!("maps/{id}.toml"), rest),
        [Seg::Key(k), rest @ ..] => (section_file(k), rest),
        [Seg::Index(_), ..] => return ("pack.toml".into(), String::new()),
    };
    let mut field = String::new();
    for s in rest {
        match s {
            Seg::Index(i) => field.push_str(&format!("[{i}]")),
            Seg::Key(k) => {
                if !field.is_empty() {
                    field.push('.');
                }
                field.push_str(k);
            }
        }
    }
    (file, field)
}

/// Generalised paths of the objects whose keys are the ids they define (or
/// otherwise free names): a key under one generalises to `*`.
const KEYED: &[&str] = &[
    "battle.monsters",
    "battle.formations",
    "battle.bosses",
    "dialog.lines",
    "items.items",
    "items.sets",
    "arts.arts",
    "arts.chains",
    "spells.spells",
    "audio.tracks",
    "audio.sounds",
    "audio.music.modes",
    "cutscenes.cutscenes",
    "cutscenes.cutscenes.*.tracks",
    "maps",
    "maps.*.tiles.legend",
    "modes.golf[].surfaces",
];

/// Keyed definitions: the generalised path of the key, and its kind.
const KEY_DEFINES: &[(&str, Kind)] = &[
    ("battle.monsters.*", Kind::Monster),
    ("battle.formations.*", Kind::Formation),
    ("battle.bosses.*", Kind::Boss),
    ("dialog.lines.*", Kind::Line),
    ("items.items.*", Kind::Item),
    ("items.sets.*", Kind::ItemSet),
    ("arts.arts.*", Kind::Art),
    ("arts.chains.*", Kind::ArtChain),
    ("spells.spells.*", Kind::Spell),
    ("audio.tracks.*", Kind::Track),
    ("audio.sounds.*", Kind::Sound),
    ("cutscenes.cutscenes.*", Kind::Cutscene),
    ("maps.*", Kind::Map),
];

/// `id` fields that define: the generalised path, and the kind.
const ID_DEFINES: &[(&str, Kind)] = &[
    ("quests.quests[].id", Kind::Quest),
    ("quests.challenges[].id", Kind::Challenge),
    ("items.shops[].id", Kind::Shop),
    ("cards.dealers[].id", Kind::Dealer),
    ("cards.cards[].id", Kind::Card),
    ("cards.boosters[].id", Kind::Booster),
    ("modes.poker[].id", Kind::Mode),
    ("modes.card_duels[].id", Kind::Mode),
    ("modes.golf[].id", Kind::Mode),
    ("modes.races[].id", Kind::Mode),
    ("modes.prop_hunt[].id", Kind::Mode),
    ("modes.party_nights[].id", Kind::Mode),
    ("maps.*.npcs[].id", Kind::Npc),
    ("battle.encounters[].scene", Kind::Encounter),
];

/// Which sibling keys a gate under `gate_key` covers, in an object at
/// generalised path `g`.
fn gate_covers(g: &str, gate_key: &str, sibling: &str) -> bool {
    if sibling == gate_key {
        return false;
    }
    match gate_key {
        // `after` decides only when `line_after` replaces `line`.
        "after" => sibling == "line_after",
        // An NPC's `requires` gates what it says and opens, not whether it
        // stands there (`present` does that) or counts as talked to.
        "requires" if g.ends_with(".npcs[]") => sibling != "id",
        // A shut exit's `locked_line` is said while the gate is shut.
        "requires" if g.ends_with(".exits[]") => sibling != "locked_line",
        _ => true,
    }
}

const GATE_KEYS: &[&str] = &["requires", "prerequisites", "present", "after"];

struct Raw {
    path: JsonPath,
    g: String,
    key: String,
    member: bool,
    is_key: bool,
    value: String,
    parent_do: Option<String>,
    parent_kind: Option<String>,
    gates: Vec<usize>,
    in_gate: Option<usize>,
}

struct Walker {
    raws: Vec<Raw>,
    gates: Vec<Gate>,
}

fn tag(o: &Map<String, Value>, k: &str) -> Option<String> {
    o.get(k).and_then(Value::as_str).map(str::to_string)
}

impl Walker {
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        v: &Value,
        path: &mut JsonPath,
        g: &str,
        parent: Option<&Map<String, Value>>,
        member: bool,
        gates: &[usize],
        in_gate: Option<usize>,
    ) {
        match v {
            Value::String(s) => {
                let key = path
                    .iter()
                    .rev()
                    .find_map(|s| match s {
                        Seg::Key(k) => Some(k.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                self.raws.push(Raw {
                    path: path.clone(),
                    g: g.to_string(),
                    key,
                    member,
                    is_key: false,
                    value: s.clone(),
                    parent_do: parent.and_then(|p| tag(p, "do")),
                    parent_kind: parent.and_then(|p| tag(p, "kind")),
                    gates: gates.to_vec(),
                    in_gate,
                });
            }
            Value::Array(a) => {
                let ga = format!("{g}[]");
                for (i, x) in a.iter().enumerate() {
                    path.push(Seg::Index(i));
                    self.walk(x, path, &ga, parent, true, gates, in_gate);
                    path.pop();
                }
            }
            Value::Object(o) => {
                let keyed = KEYED.contains(&g);
                // The gates this object holds, by key.
                let mut own: Vec<(String, usize)> = Vec::new();
                if !keyed {
                    for gk in GATE_KEYS {
                        // A gate is the object under a gate key; the page
                        // may hold one with only the lists it uses.
                        if let Some(Value::Object(_)) = o.get(*gk) {
                            path.push(Seg::Key(gk.to_string()));
                            let (file, field) = locate(path);
                            self.gates.push(Gate {
                                path: path.clone(),
                                file,
                                field,
                            });
                            path.pop();
                            own.push((gk.to_string(), self.gates.len() - 1));
                        }
                    }
                }
                for (k, x) in o {
                    path.push(Seg::Key(k.clone()));
                    let gk = if keyed {
                        if g.is_empty() {
                            "*".to_string()
                        } else {
                            format!("{g}.*")
                        }
                    } else if g.is_empty() {
                        k.clone()
                    } else {
                        format!("{g}.{k}")
                    };
                    if keyed {
                        self.raws.push(Raw {
                            path: path.clone(),
                            g: gk.clone(),
                            key: k.clone(),
                            member: false,
                            is_key: true,
                            value: k.clone(),
                            parent_do: None,
                            parent_kind: None,
                            gates: gates.to_vec(),
                            in_gate,
                        });
                    }
                    let mut child_gates = gates.to_vec();
                    for (gkey, gi) in &own {
                        if gate_covers(g, gkey, k) {
                            child_gates.push(*gi);
                        }
                    }
                    let child_in_gate = own
                        .iter()
                        .find(|(gkey, _)| gkey == k)
                        .map(|(_, gi)| *gi)
                        .or(in_gate);
                    self.walk(x, path, &gk, Some(o), false, &child_gates, child_in_gate);
                    path.pop();
                }
            }
            _ => {}
        }
    }
}

const LINE_KEYS: &[&str] = &[
    "line",
    "line_after",
    "locked_line",
    "charge_line",
    "start_dialog",
    "active_dialog",
    "complete_dialog",
];

/// The ids the pack defines, by kind (pass 1 of [`index`]).
type Defined = BTreeMap<Kind, BTreeSet<String>>;

fn classify(r: &Raw, defined: &Defined) -> Vec<(Kind, Role)> {
    let has = |k: Kind, id: &str| defined.get(&k).is_some_and(|s| s.contains(id));
    let g = r.g.as_str();
    let key = r.key.as_str();
    let v = r.value.as_str();
    if r.is_key {
        if let Some((_, k)) = KEY_DEFINES.iter().find(|(p, _)| *p == g) {
            return vec![(*k, Role::Defines)];
        }
        if g == "audio.music.modes.*" && !crate::audio::MODE_KIND_KEYS.contains(&v) {
            return vec![(Kind::Mode, Role::References)];
        }
        return vec![];
    }
    if v.is_empty() {
        return vec![];
    }
    if let Some((_, k)) = ID_DEFINES.iter().find(|(p, _)| *p == g) {
        let mut out = vec![(*k, Role::Defines)];
        if *k == Kind::Encounter && has(Kind::Map, v) {
            out.push((Kind::Map, Role::Reads));
        }
        return out;
    }
    // A gate's members.
    if r.in_gate.is_some() && r.member {
        return match key {
            "quests" => vec![(Kind::Quest, Role::Reads)],
            "flags" | "flags_clear" => vec![(Kind::Flag, Role::Reads)],
            "bosses" | "bosses_unbeaten" => vec![(Kind::Boss, Role::Reads)],
            _ => vec![],
        };
    }
    let pdo = r.parent_do.as_deref();
    let pkind = r.parent_kind.as_deref();
    if r.member && matches!(key, "set_flags" | "raise_flags" | "clear_flags") {
        return vec![(Kind::Flag, Role::Sets)];
    }
    if r.member {
        return match key {
            "summon" | "monsters" if g.starts_with("battle.") => {
                vec![(Kind::Monster, Role::References)]
            }
            "cards" | "starter" | "pool" | "deck" => vec![(Kind::Card, Role::References)],
            "boosters" => vec![(Kind::Booster, Role::References)],
            "stock" if g == "cards.dealers[].stock[]" => vec![(Kind::Booster, Role::References)],
            "stock" if g == "items.shops[].stock[]" => vec![(Kind::Item, Role::References)],
            "bodies" => vec![(Kind::Npc, Role::References)],
            "arts" if g == "arts.chains.*.arts[]" => vec![(Kind::Art, Role::References)],
            _ => vec![],
        };
    }
    match key {
        "flag" => match (pdo, pkind) {
            (Some("set_flag" | "clear_flag"), _) | (_, Some("set_flag")) => {
                vec![(Kind::Flag, Role::Sets)]
            }
            (_, Some("flag")) => vec![(Kind::Flag, Role::Reads)],
            _ => vec![],
        },
        "counter" => {
            if g.contains("objectives[]") || pdo == Some("branch") {
                vec![(Kind::Counter, Role::Reads)]
            } else {
                vec![(Kind::Counter, Role::Sets)]
            }
        }
        k if LINE_KEYS.contains(&k) => vec![(Kind::Line, Role::References)],
        "next" if g.starts_with("dialog.lines.*.choices[]") => vec![(Kind::Line, Role::References)],
        "formation" | "start_battle" => vec![(Kind::Formation, Role::References)],
        "battle" if g.starts_with("dialog.lines.*.choices[]") => {
            vec![(Kind::Formation, Role::References)]
        }
        "boss" | "start_boss" => vec![(Kind::Boss, Role::References)],
        "cutscene" | "start_cutscene" | "complete_cutscene" => {
            vec![(Kind::Cutscene, Role::References)]
        }
        "intro" | "outro" if g.starts_with("battle.bosses.*.") => {
            vec![(Kind::Cutscene, Role::References)]
        }
        "quest" => vec![(Kind::Quest, Role::References)],
        "challenge" => vec![(Kind::Challenge, Role::References)],
        "mode" if pdo != Some("tint") => vec![(Kind::Mode, role_for_mode(g))],
        "npc" | "giver" => vec![(Kind::Npc, Role::References)],
        "actor" | "toward" if v != "player" && !v.starts_with("retail:") => {
            vec![(Kind::Npc, Role::References)]
        }
        "shop" if g.ends_with(".npcs[].shop") && !has(Kind::Shop, v) && has(Kind::Dealer, v) => {
            vec![(Kind::Dealer, Role::References)]
        }
        "shop" => vec![(Kind::Shop, Role::References)],
        "dealer" => vec![(Kind::Dealer, Role::References)],
        "card" => vec![(Kind::Card, Role::References)],
        "booster" | "win_booster" => vec![(Kind::Booster, Role::References)],
        "item" => {
            if pkind == Some("collect_item") || g.ends_with("learn.item") {
                vec![(Kind::Item, Role::Reads)]
            } else {
                vec![(Kind::Item, Role::References)]
            }
        }
        "set" if g == "items.items.*.set" => vec![(Kind::ItemSet, Role::References)],
        "capture" => vec![(Kind::Monster, Role::Reads)],
        "finisher" if g == "arts.chains.*.finisher" => vec![(Kind::Art, Role::References)],
        "map" if g == "cutscenes.cutscenes.*.map" => vec![(Kind::Map, Role::Reads)],
        "map" => vec![(Kind::Map, Role::References)],
        "scene" if g.contains("objectives[]") && has(Kind::Map, v) => {
            vec![(Kind::Map, Role::Reads)]
        }
        "table" if g == "maps.*.encounters[].table" => {
            let mut out = vec![(Kind::Encounter, Role::References)];
            if has(Kind::Map, v) {
                out.push((Kind::Map, Role::Reads));
            }
            out
        }
        "id" => match pkind {
            Some("track") => vec![(Kind::Track, Role::References)],
            Some("sound") => vec![(Kind::Sound, Role::References)],
            Some("formation") if g == "cards.rewards[].source.id" => {
                vec![(Kind::Formation, Role::Reads)]
            }
            Some("boss") if g == "cards.rewards[].source.id" => vec![(Kind::Boss, Role::Reads)],
            Some("mode") if g == "cards.rewards[].source.id" => vec![(Kind::Mode, Role::Reads)],
            Some("monster") if g == "cards.rewards[].source.id" => {
                vec![(Kind::Monster, Role::Reads)]
            }
            _ => vec![],
        },
        _ => vec![],
    }
}

/// A mode named by an objective waits on it; everywhere else it is
/// entered.
fn role_for_mode(g: &str) -> Role {
    if g.contains("objectives[]") {
        Role::Reads
    } else {
        Role::References
    }
}

/// Is `kind` qualified by a pack id when named from elsewhere.
fn qualifiable(kind: Kind) -> bool {
    !matches!(kind, Kind::Flag | Kind::Counter | Kind::Encounter)
}

/// Build the reference model of `pack` (its JSON form).
pub fn index_value(pack: &Value) -> Index {
    let own = pack
        .get("manifest")
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut w = Walker {
        raws: Vec::new(),
        gates: Vec::new(),
    };
    w.walk(pack, &mut Vec::new(), "", None, false, &[], None);

    // Pass 1: what is defined.
    let mut defined: Defined = BTreeMap::new();
    for r in &w.raws {
        if r.is_key {
            if let Some((_, k)) = KEY_DEFINES.iter().find(|(p, _)| *p == r.g) {
                defined.entry(*k).or_default().insert(r.value.clone());
            }
        } else if let Some((_, k)) = ID_DEFINES.iter().find(|(p, _)| *p == r.g)
            && !r.value.is_empty()
        {
            defined.entry(*k).or_default().insert(r.value.clone());
        }
    }

    // Pass 2: classify every string.
    let mut sites = Vec::new();
    for r in &w.raws {
        for (kind, role) in classify(r, &defined) {
            let (file, field) = locate(&r.path);
            let mut prefix = String::new();
            let mut id = r.value.clone();
            if qualifiable(kind)
                && !r.is_key
                && let Some((p, local)) = r.value.split_once(':')
            {
                if p != own {
                    continue; // another pack's content: not this pack's to track
                }
                prefix = format!("{p}:");
                id = local.to_string();
            }
            let base = Site {
                kind,
                id,
                role,
                file,
                field,
                key: r.is_key,
                owner: None,
                prefix,
                suffix: String::new(),
                path: r.path.clone(),
                gates: r.gates.clone(),
                in_gate: r.in_gate,
            };
            // Engine flags named after an id follow it.
            if kind == Kind::Flag {
                if let Some(c) = base.id.strip_prefix("cutscene.") {
                    let mut s = base.clone();
                    s.kind = Kind::Cutscene;
                    s.role = Role::Reads;
                    s.prefix = "cutscene.".into();
                    s.id = c.to_string();
                    sites.push(s);
                } else if let Some(rest) = base.id.strip_prefix("map.")
                    && let Some((m, t)) = rest.split_once('.')
                {
                    let mut s = base.clone();
                    s.kind = Kind::Map;
                    s.role = Role::Reads;
                    s.prefix = "map.".into();
                    s.suffix = format!(".{t}");
                    s.id = m.to_string();
                    sites.push(s);
                }
            }
            sites.push(base);
        }
    }

    // Owners: the nearest enclosing definition.
    let containers: Vec<(JsonPath, Kind, String)> = sites
        .iter()
        .filter(|s| s.role == Role::Defines)
        .map(|s| {
            let c = if s.key {
                s.path.clone()
            } else {
                s.path[..s.path.len() - 1].to_vec()
            };
            (c, s.kind, s.id.clone())
        })
        .collect();
    // A card reward rule belongs to its source.
    let mut rule_source: BTreeMap<usize, (Kind, String)> = BTreeMap::new();
    for s in &sites {
        if let [
            Seg::Key(a),
            Seg::Key(b),
            Seg::Index(i),
            Seg::Key(c),
            Seg::Key(d),
        ] = &s.path[..]
            && a == "cards"
            && b == "rewards"
            && c == "source"
            && d == "id"
        {
            rule_source.insert(*i, (s.kind, s.id.clone()));
        }
    }
    for s in &mut sites {
        let own_container = if s.role == Role::Defines {
            Some(if s.key {
                s.path.clone()
            } else {
                s.path[..s.path.len() - 1].to_vec()
            })
        } else {
            None
        };
        let mut best: Option<&(JsonPath, Kind, String)> = None;
        for c in &containers {
            if Some(&c.0) == own_container.as_ref() {
                continue;
            }
            if s.path.len() > c.0.len()
                && s.path.starts_with(&c.0)
                && best.is_none_or(|b| c.0.len() > b.0.len())
            {
                best = Some(c);
            }
        }
        s.owner = best.map(|b| (b.1, b.2.clone()));
        if s.owner.is_none()
            && let [Seg::Key(a), Seg::Key(b), Seg::Index(i), rest @ ..] = &s.path[..]
            && a == "cards"
            && b == "rewards"
            && rest.first() != Some(&Seg::Key("source".into()))
        {
            s.owner = rule_source.get(i).cloned();
        }
    }
    Index {
        sites,
        gates: w.gates,
    }
}

/// Build the reference model of a parsed pack.
pub fn index(pack: &ExpansionPack) -> Index {
    index_value(&serde_json::to_value(pack).unwrap_or(Value::Null))
}

impl Index {
    /// Every site naming `kind` `id`.
    pub fn of<'a>(&'a self, kind: Kind, id: &'a str) -> impl Iterator<Item = &'a Site> + 'a {
        self.sites
            .iter()
            .filter(move |s| s.kind == kind && s.id == id)
    }

    /// Every site naming `id`, of any kind.
    pub fn named<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Site> + 'a {
        self.sites.iter().filter(move |s| s.id == id)
    }

    /// Is `kind` `id` defined in the pack.
    pub fn defines(&self, kind: Kind, id: &str) -> bool {
        self.of(kind, id).any(|s| s.role == Role::Defines)
    }

    /// The ids of `kind` the pack defines or (for flags and counters) uses.
    pub fn ids(&self, kind: Kind) -> BTreeSet<String> {
        self.sites
            .iter()
            .filter(|s| s.kind == kind)
            .filter(|s| s.role == Role::Defines || matches!(kind, Kind::Flag | Kind::Counter))
            .map(|s| s.id.clone())
            .collect()
    }
}

/// One field a rename rewrites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub file: String,
    pub field: String,
    pub role: Role,
    pub key: bool,
    pub before: String,
    pub after: String,
}

/// Why a rename was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameError {
    BadId(String),
    Unknown(String),
    Taken(String),
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenameError::BadId(m) | RenameError::Unknown(m) | RenameError::Taken(m) => {
                f.write_str(m)
            }
        }
    }
}

/// The changes renaming `kind` `old` to `new` makes, without making them.
pub fn rename_plan(
    pack: &Value,
    kind: Kind,
    old: &str,
    new: &str,
) -> Result<Vec<Change>, RenameError> {
    let ix = index_value(pack);
    plan(&ix, kind, old, new)
}

fn plan(ix: &Index, kind: Kind, old: &str, new: &str) -> Result<Vec<Change>, RenameError> {
    if !kind.is_valid_id(new) {
        return Err(RenameError::BadId(format!(
            "{new:?} is not a valid {} id",
            kind.as_str()
        )));
    }
    if old == new {
        return Err(RenameError::BadId("the new id is the old one".into()));
    }
    let sites: Vec<&Site> = ix.of(kind, old).collect();
    if sites.is_empty() {
        return Err(RenameError::Unknown(format!(
            "no {} {old:?} in the pack",
            kind.as_str()
        )));
    }
    let taken = if matches!(kind, Kind::Flag | Kind::Counter) {
        ix.of(kind, new).next().is_some()
    } else {
        ix.defines(kind, new)
    };
    if taken {
        return Err(RenameError::Taken(format!(
            "{} {new:?} already exists",
            kind.as_str()
        )));
    }
    Ok(sites
        .iter()
        .map(|s| Change {
            file: s.file.clone(),
            field: s.field.clone(),
            role: s.role,
            key: s.key,
            before: format!("{}{}{}", s.prefix, s.id, s.suffix),
            after: format!("{}{new}{}", s.prefix, s.suffix),
        })
        .collect())
}

fn at_mut<'a>(v: &'a mut Value, path: &[Seg]) -> Option<&'a mut Value> {
    let mut cur = v;
    for s in path {
        cur = match s {
            Seg::Key(k) => cur.as_object_mut()?.get_mut(k)?,
            Seg::Index(i) => cur.as_array_mut()?.get_mut(*i)?,
        };
    }
    Some(cur)
}

/// Rename `kind` `old` to `new` everywhere in `pack` (its JSON form): the
/// definition, every reference, and the engine flags named after it.
/// Returns the changes made; on a refusal `pack` is untouched.
pub fn rename(
    pack: &mut Value,
    kind: Kind,
    old: &str,
    new: &str,
) -> Result<Vec<Change>, RenameError> {
    let ix = index_value(pack);
    let changes = plan(&ix, kind, old, new)?;
    let mut sites: Vec<&Site> = ix.of(kind, old).collect();
    // Values first, then keys deepest first, so no edit moves a path
    // another edit still needs.
    sites.sort_by_key(|s| (s.key, std::cmp::Reverse(s.path.len())));
    for s in sites {
        if s.key {
            let (Some(Seg::Key(k)), parent) = (s.path.last(), &s.path[..s.path.len() - 1]) else {
                continue;
            };
            if let Some(Value::Object(o)) = at_mut(pack, parent)
                && let Some(x) = o.remove(k)
            {
                o.insert(format!("{}{new}{}", s.prefix, s.suffix), x);
            }
        } else if let Some(x) = at_mut(pack, &s.path) {
            *x = Value::String(format!("{}{new}{}", s.prefix, s.suffix));
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pack() -> Value {
        json!({
            "manifest": { "id": "t" },
            "battle": {
                "monsters": { "ogre": { "base": { "kind": "monster", "id": 3 } } },
                "formations": { "pack": { "monsters": ["ogre", "t:ogre", "other:ogre"] } },
                "bosses": { "king": { "formation": "pack", "intro": "hello" } },
            },
            "cutscenes": { "cutscenes": { "hello": { "steps": [
                { "do": "set_flag", "flag": "met" },
                { "do": "tint", "mode": "subtract" },
            ] } } },
            "quests": { "quests": [{
                "id": "q1",
                "prerequisites": { "quests": [], "story_flags": [], "story_flags_clear": [],
                    "flags": ["met", "cutscene.hello"], "flags_clear": [], "bosses": ["king"],
                    "bosses_unbeaten": [], "min_level": null },
                "rewards": { "set_flags": ["done"] },
            }] },
        })
    }

    #[test]
    fn classifies_by_place() {
        let ix = index_value(&pack());
        let roles = |k, id: &str| {
            let mut r = ix.of(k, id).map(|s| s.role).collect::<Vec<_>>();
            r.sort();
            r
        };
        assert_eq!(
            roles(Kind::Monster, "ogre"),
            [Role::Defines, Role::References, Role::References],
            "the other pack's ogre is not ours"
        );
        assert_eq!(roles(Kind::Flag, "met"), [Role::Sets, Role::Reads]);
        assert_eq!(
            roles(Kind::Cutscene, "hello"),
            [Role::Defines, Role::Reads, Role::References]
        );
        assert!(
            ix.of(Kind::Mode, "subtract").next().is_none(),
            "a tint mode is not a game mode"
        );
        let reward = ix.of(Kind::Flag, "done").next().unwrap();
        assert_eq!(reward.owner, Some((Kind::Quest, "q1".into())));
        assert_eq!(
            reward.gates.len(),
            1,
            "a quest's prerequisites gate its rewards"
        );
        let gate_read = ix
            .of(Kind::Boss, "king")
            .find(|s| s.role == Role::Reads)
            .unwrap();
        assert!(gate_read.in_gate.is_some());
        assert_eq!(gate_read.file, "quests.toml");
        assert_eq!(gate_read.field, "quests[0].prerequisites.bosses[0]");
    }

    #[test]
    fn renames_every_site() {
        let mut p = pack();
        let changes = rename(&mut p, Kind::Monster, "ogre", "brute").unwrap();
        assert_eq!(changes.len(), 3);
        assert!(p["battle"]["monsters"].get("brute").is_some());
        assert_eq!(
            p["battle"]["formations"]["pack"]["monsters"],
            json!(["brute", "t:brute", "other:ogre"])
        );
        rename(&mut p, Kind::Cutscene, "hello", "greeting").unwrap();
        assert_eq!(p["battle"]["bosses"]["king"]["intro"], "greeting");
        assert_eq!(
            p["quests"]["quests"][0]["prerequisites"]["flags"][1],
            "cutscene.greeting"
        );
        assert!(matches!(
            rename(&mut p, Kind::Flag, "met", "done"),
            Err(RenameError::Taken(_))
        ));
        assert!(matches!(
            rename(&mut p, Kind::Boss, "king", "Bad Id"),
            Err(RenameError::BadId(_))
        ));
    }
}
