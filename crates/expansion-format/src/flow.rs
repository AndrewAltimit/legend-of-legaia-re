//! The pack's progression flow over the reference model ([`crate::xref`]):
//! what the player can reach from the pack's entry points, the lint rules
//! the validator runs over it, and the node graph the workbench draws.
//!
//! # Reachability
//!
//! The entry points are the maps with an entrance from a retail scene and
//! the quests that start on their own (`auto_start`). From there a pass
//! repeats until nothing changes: a site takes effect when its owner (the
//! quest a reward is in, the map a trigger is on, the line a choice is in)
//! has been reached and every gate on its way holds; a site that takes
//! effect reaches what it names (an exit its map, a trigger its fight, a
//! choice its line) and a write sets its flag. A gate holds when each quest
//! and boss it needs has been reached and each flag it needs is set by
//! something reached; what it needs clear holds from the start. A quest is
//! reached when something reached starts it (its giver, a `start_quest`, a
//! hook, `auto_start`) and its prerequisites hold.
//!
//! The pass is optimistic: a reached quest is taken as completable and a
//! reached boss as beaten, so what it reports unreachable is unreachable on
//! every route.
//!
//! # Lints
//!
//! Each is a warning with the field it is about, so the CLI and the
//! workbench list it like any other check: a flag or counter read and
//! never written (and written and never read), a map or quest or challenge
//! the player cannot reach, quests whose prerequisites form a cycle, and a
//! card, booster or pack item nothing grants (or that only unreachable
//! content grants).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::ExpansionPack;
use crate::validate::{Issue, Severity};
use crate::xref::{Index, Kind, Role, Seg, Site};

type Node = (Kind, String);

/// The kinds that are nodes of the flow graph; the others fold into the
/// node that holds them (a line into the NPC's map, a formation into its
/// boss).
pub const GRAPH_KINDS: &[Kind] = &[
    Kind::Map,
    Kind::Quest,
    Kind::Challenge,
    Kind::Boss,
    Kind::Cutscene,
    Kind::Mode,
    Kind::Flag,
    Kind::Counter,
];

fn engine_flag(id: &str) -> bool {
    id.starts_with("cutscene.") || id.starts_with("map.") || id.starts_with("run.")
}

/// The list key a gate member sits in (`flags`, `bosses_unbeaten`).
fn gate_list(s: &Site) -> &str {
    match s.path.get(s.path.len().wrapping_sub(2)) {
        Some(Seg::Key(k)) => k,
        _ => "",
    }
}

/// What the player can reach.
#[derive(Debug, Clone, Default)]
pub struct Reach {
    pub reached: BTreeSet<Node>,
    /// Quests something reached starts (their prerequisites may still fail).
    pub offered: BTreeSet<String>,
    /// The maps with an entrance from a retail scene.
    pub entry_maps: BTreeSet<String>,
    /// The quests that start on their own.
    pub auto_quests: BTreeSet<String>,
}

struct Flow<'a> {
    pack: &'a ExpansionPack,
    ix: &'a Index,
    /// gate index -> the requirement sites in it.
    gate_needs: Vec<Vec<&'a Site>>,
}

impl<'a> Flow<'a> {
    fn new(pack: &'a ExpansionPack, ix: &'a Index) -> Self {
        let mut gate_needs = vec![Vec::new(); ix.gates.len()];
        for s in &ix.sites {
            if let Some(g) = s.in_gate
                && s.role == Role::Reads
                && matches!(s.kind, Kind::Quest | Kind::Flag | Kind::Boss)
            {
                gate_needs[g].push(s);
            }
        }
        Self {
            pack,
            ix,
            gate_needs,
        }
    }

    fn need_holds(&self, s: &Site, r: &BTreeSet<Node>) -> bool {
        match (s.kind, gate_list(s)) {
            (_, "flags_clear" | "bosses_unbeaten") => true,
            (Kind::Flag, _) => {
                if let Some(c) = s.id.strip_prefix("cutscene.") {
                    r.contains(&(Kind::Cutscene, c.to_string()))
                } else {
                    engine_flag(&s.id) || r.contains(&(Kind::Flag, s.id.clone()))
                }
            }
            (k, _) => r.contains(&(k, s.id.clone())),
        }
    }

    fn gate_holds(&self, g: usize, r: &BTreeSet<Node>) -> bool {
        self.gate_needs[g].iter().all(|s| self.need_holds(s, r))
    }

    fn active(&self, s: &Site, r: &BTreeSet<Node>) -> bool {
        s.owner.as_ref().is_none_or(|o| r.contains(o))
            && s.gates.iter().all(|g| self.gate_holds(*g, r))
    }

    fn reach(&self) -> Reach {
        let mut out = Reach::default();
        for (id, m) in &self.pack.maps {
            if !m.entrances.is_empty() {
                out.entry_maps.insert(id.clone());
            }
        }
        for q in &self.pack.quests.quests {
            if q.auto_start {
                out.auto_quests.insert(q.id.clone());
            }
        }
        let mut r: BTreeSet<Node> = BTreeSet::new();
        let mut offered: BTreeSet<String> = out.auto_quests.clone();
        // An entrance's own gate.
        let entrance_gates: BTreeMap<String, Vec<Option<usize>>> = self
            .pack
            .maps
            .iter()
            .map(|(id, m)| {
                let gates = (0..m.entrances.len())
                    .map(|i| {
                        let p = vec![
                            Seg::Key("maps".into()),
                            Seg::Key(id.clone()),
                            Seg::Key("entrances".into()),
                            Seg::Index(i),
                            Seg::Key("requires".into()),
                        ];
                        self.ix.gates.iter().position(|g| g.path == p)
                    })
                    .collect();
                (id.clone(), gates)
            })
            .collect();
        loop {
            let before = (r.len(), offered.len());
            for (id, gates) in &entrance_gates {
                if gates
                    .iter()
                    .any(|g| g.is_none_or(|g| self.gate_holds(g, &r)))
                {
                    r.insert((Kind::Map, id.clone()));
                }
            }
            for s in &self.ix.sites {
                if s.in_gate.is_some() {
                    continue;
                }
                match s.role {
                    Role::Defines => {
                        // Something defined inside something reached (an
                        // NPC on a map) stands there once its gates hold.
                        if s.owner.is_some() && s.kind != Kind::Quest && self.active(s, &r) {
                            r.insert((s.kind, s.id.clone()));
                        }
                        if s.kind == Kind::Quest
                            && offered.contains(&s.id)
                            && s.gates.iter().all(|g| self.gate_holds(*g, &r))
                        {
                            r.insert((Kind::Quest, s.id.clone()));
                        }
                    }
                    Role::References | Role::Sets => {
                        if s.kind == Kind::Npc && s.field.ends_with(".giver") {
                            // A giver standing somewhere reached offers its quest.
                            if r.contains(&(Kind::Npc, s.id.clone()))
                                && let Some((Kind::Quest, q)) = &s.owner
                            {
                                offered.insert(q.clone());
                            }
                            continue;
                        }
                        // A `talk` event counts as talking to its NPC (a
                        // giver need not stand anywhere); other NPC names
                        // reach nothing.
                        let talk = s.field.ends_with(".event.npc");
                        if (s.kind == Kind::Npc && !talk) || !self.active(s, &r) {
                            continue;
                        }
                        if s.kind == Kind::Quest {
                            offered.insert(s.id.clone());
                        } else {
                            r.insert((s.kind, s.id.clone()));
                        }
                    }
                    Role::Reads => {}
                }
            }
            if (r.len(), offered.len()) == before {
                break;
            }
        }
        out.reached = r;
        out.offered = offered;
        out
    }
}

/// What the player can reach in `pack`.
pub fn reach(pack: &ExpansionPack, ix: &Index) -> Reach {
    Flow::new(pack, ix).reach()
}

/// A lint finding: the node it is about and the issue.
#[derive(Debug, Clone)]
pub struct Finding {
    pub node: Node,
    pub issue: Issue,
}

fn warn(node: Node, file: &str, field: &str, message: String) -> Finding {
    Finding {
        node,
        issue: Issue {
            severity: Severity::Warning,
            path: file.to_string(),
            field: field.to_string(),
            message,
        },
    }
}

fn findings(pack: &ExpansionPack, ix: &Index, reach: &Reach) -> Vec<Finding> {
    let flow = Flow::new(pack, ix);
    let mut out = Vec::new();

    // Flags and counters: written and read.
    for kind in [Kind::Flag, Kind::Counter] {
        let mut by: BTreeMap<&str, (Vec<&Site>, Vec<&Site>)> = BTreeMap::new();
        for s in ix.sites.iter().filter(|s| s.kind == kind) {
            let e = by.entry(s.id.as_str()).or_default();
            if s.role == Role::Sets {
                e.0.push(s);
            } else {
                e.1.push(s);
            }
        }
        let what = if kind == Kind::Flag {
            "flag"
        } else {
            "counter"
        };
        for (id, (sets, reads)) in by {
            let node = (kind, id.to_string());
            if kind == Kind::Flag && engine_flag(id) {
                if let Some(c) = id.strip_prefix("cutscene.")
                    && !ix.defines(Kind::Cutscene, c)
                {
                    for s in &reads {
                        out.push(warn(
                            node.clone(),
                            &s.file,
                            &s.field,
                            format!("`{id}` is the seen flag of cutscene `{c}`, which the pack does not define: this never holds"),
                        ));
                    }
                }
                continue;
            }
            if sets.is_empty() {
                for s in &reads {
                    out.push(warn(
                        node.clone(),
                        &s.file,
                        &s.field,
                        format!(
                            "pack {what} `{id}` is read here and nothing in the pack writes it"
                        ),
                    ));
                }
            } else if reads.is_empty() {
                // One warning per id, at its first writer: a counter many
                // rewards add to is one finding.
                for s in sets.iter().take(1) {
                    out.push(warn(
                        node.clone(),
                        &s.file,
                        &s.field,
                        format!(
                            "pack {what} `{id}` is written here and nothing in the pack reads it"
                        ),
                    ));
                }
            }
        }
    }

    // Quest prerequisite cycles.
    let quests: Vec<&str> = pack.quests.quests.iter().map(|q| q.id.as_str()).collect();
    let needs: BTreeMap<&str, Vec<&str>> = pack
        .quests
        .quests
        .iter()
        .map(|q| {
            (
                q.id.as_str(),
                q.prerequisites.quests.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    for (i, q) in pack.quests.quests.iter().enumerate() {
        for (j, p) in q.prerequisites.quests.iter().enumerate() {
            // `p` leads back to `q`.
            if let Some(cycle) = path_back(&needs, p, &q.id) {
                let mut chain = vec![q.id.as_str()];
                chain.extend(cycle);
                out.push(warn(
                    (Kind::Quest, q.id.clone()),
                    "quests.toml",
                    &format!("quests[{i}].prerequisites.quests[{j}]"),
                    format!(
                        "quest prerequisites form a cycle ({}): none of them can start",
                        chain.join(" needs ")
                    ),
                ));
            }
        }
    }
    let _ = quests;

    // Reachability.
    let r = &reach.reached;
    if !pack.maps.is_empty() && reach.entry_maps.is_empty() {
        out.push(warn(
            (Kind::Map, String::new()),
            "pack.toml",
            "",
            "no map has an entrance from a retail scene: in a game the pack's maps cannot be entered".into(),
        ));
    } else {
        for id in pack.maps.keys() {
            if !r.contains(&(Kind::Map, id.clone())) {
                out.push(warn(
                    (Kind::Map, id.clone()),
                    &format!("maps/{id}.toml"),
                    "",
                    format!("map `{id}` cannot be reached from the pack's entrances: no open exit, mode or entrance leads to it"),
                ));
            }
        }
    }
    for (i, q) in pack.quests.quests.iter().enumerate() {
        if r.contains(&(Kind::Quest, q.id.clone())) {
            continue;
        }
        let why = if !reach.offered.contains(&q.id) {
            "nothing the player can reach starts it (a giver on a reached map, `auto_start`, a `start_quest`)".to_string()
        } else {
            let gate = ix
                .of(Kind::Quest, &q.id)
                .find(|s| s.role == Role::Defines)
                .map(|s| s.gates.clone())
                .unwrap_or_default();
            let unmet: Vec<String> = gate
                .iter()
                .flat_map(|g| flow.gate_needs[*g].iter())
                .filter(|s| !flow.need_holds(s, r))
                .map(|s| format!("{} `{}`", s.kind.as_str(), s.id))
                .collect();
            format!("its prerequisites never hold ({})", unmet.join(", "))
        };
        out.push(warn(
            (Kind::Quest, q.id.clone()),
            "quests.toml",
            &format!("quests[{i}].id"),
            format!("quest `{}` can never start: {why}", q.id),
        ));
    }
    for (i, c) in pack.quests.challenges.iter().enumerate() {
        if !r.contains(&(Kind::Challenge, c.id.clone())) {
            out.push(warn(
                (Kind::Challenge, c.id.clone()),
                "quests.toml",
                &format!("challenges[{i}].id"),
                format!("challenge `{}` cannot be started: no trigger or choice the player can reach names it", c.id),
            ));
        }
    }

    // Rewards nothing grants.
    for (kind, what, how) in [
        (
            Kind::Card,
            "card",
            "no reward, booster pool, prize, starter deck or `give_card` step names it",
        ),
        (
            Kind::Booster,
            "booster",
            "no reward, dealer, prize or duel names it",
        ),
        (
            Kind::Item,
            "pack item",
            "no drop, steal, reward or shop names it",
        ),
    ] {
        for def in ix
            .sites
            .iter()
            .filter(|s| s.kind == kind && s.role == Role::Defines)
        {
            let grants: Vec<&Site> = ix
                .of(kind, &def.id)
                .filter(|s| s.role == Role::References && !s.field.contains(".deck["))
                .collect();
            // A booster with an empty pool draws from every card.
            let all_cards: Vec<&Site> = if kind == Kind::Card {
                pack.cards
                    .boosters
                    .iter()
                    .filter(|b| b.pool.is_empty())
                    .flat_map(|b| {
                        ix.of(Kind::Booster, &b.id)
                            .filter(|s| s.role == Role::References)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let grants: Vec<&Site> = grants.into_iter().chain(all_cards).collect();
            let msg = if grants.is_empty() {
                format!("{what} `{}` is never granted: {how}", def.id)
            } else if !grants.iter().any(|s| flow.active(s, r)) {
                format!(
                    "{what} `{}` is granted only by content the player cannot reach",
                    def.id
                )
            } else {
                continue;
            };
            out.push(warn((kind, def.id.clone()), &def.file, &def.field, msg));
        }
    }
    out
}

/// A path of prerequisite edges from `from` to `to`, if one exists.
fn path_back<'a>(
    needs: &BTreeMap<&'a str, Vec<&'a str>>,
    from: &'a str,
    to: &str,
) -> Option<Vec<&'a str>> {
    let mut prev: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = std::collections::VecDeque::from([from]);
    let mut seen = BTreeSet::from([from]);
    while let Some(q) = queue.pop_front() {
        if q == to {
            let mut out = vec![q];
            let mut cur = q;
            while let Some(p) = prev.get(cur) {
                out.push(p);
                cur = p;
            }
            out.reverse();
            return Some(out);
        }
        for n in needs.get(q).into_iter().flatten() {
            if seen.insert(n) {
                prev.insert(n, q);
                queue.push_back(n);
            }
        }
    }
    None
}

/// The flow lints over `pack`, as validation warnings.
pub fn lint(pack: &ExpansionPack) -> Vec<Issue> {
    let ix = crate::xref::index(pack);
    let reach = reach(pack, &ix);
    findings(pack, &ix, &reach)
        .into_iter()
        .map(|f| f.issue)
        .collect()
}

/// One node of the flow graph.
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    /// `kind:id` (`quest:tower_rumour`); the entry node is `entry:`.
    pub key: String,
    pub kind: String,
    pub id: String,
    /// The definition's (or first use's) place, a jump target.
    pub file: String,
    pub field: String,
    pub reached: bool,
    /// Engine-raised (`cutscene.<id>`), not written by the pack.
    pub engine: bool,
    pub sets: usize,
    pub reads: usize,
    pub problems: Vec<GraphProblem>,
}

/// A lint finding shown on a node.
#[derive(Debug, Clone, Serialize)]
pub struct GraphProblem {
    pub file: String,
    pub field: String,
    pub message: String,
}

/// One edge: `from` leads to, sets, or gates `to`.
#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    /// `enters` (an entrance), `leads` (an exit, a trigger, a start hook),
    /// `sets` (a flag or counter written), `reads` (a flag, counter or
    /// quest a gate or objective needs), `gives` (a giver on a map offers
    /// a quest).
    pub role: &'static str,
    pub file: String,
    pub field: String,
}

/// The flow graph and every flow lint finding.
#[derive(Debug, Clone, Serialize)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Findings not tied to one node (no entrance at all).
    pub problems: Vec<GraphProblem>,
}

fn node_key(n: &Node) -> String {
    format!("{}:{}", n.0.as_str(), n.1)
}

/// The graph nodes that hold `n`: itself for a graph kind, else whatever
/// contains or uses it, folded through lines, NPCs, formations.
fn holders(
    ix: &Index,
    n: &Node,
    memo: &mut BTreeMap<Node, BTreeSet<Node>>,
    stack: &mut BTreeSet<Node>,
) -> BTreeSet<Node> {
    if GRAPH_KINDS.contains(&n.0) {
        return BTreeSet::from([n.clone()]);
    }
    if let Some(m) = memo.get(n) {
        return m.clone();
    }
    if !stack.insert(n.clone()) {
        return BTreeSet::new();
    }
    let mut out = BTreeSet::new();
    for s in ix.of(n.0, &n.1) {
        if s.in_gate.is_some() || !matches!(s.role, Role::Defines | Role::References) {
            continue;
        }
        if let Some(o) = &s.owner {
            out.extend(holders(ix, o, memo, stack));
        }
    }
    stack.remove(n);
    memo.insert(n.clone(), out.clone());
    out
}

/// The flow graph of `pack`.
pub fn graph(pack: &ExpansionPack) -> Graph {
    let ix = crate::xref::index(pack);
    let reach = reach(pack, &ix);
    let found = findings(pack, &ix, &reach);

    let mut nodes: BTreeMap<Node, GraphNode> = BTreeMap::new();
    let add = |n: &Node, s: &Site, nodes: &mut BTreeMap<Node, GraphNode>| {
        let e = nodes.entry(n.clone()).or_insert_with(|| GraphNode {
            key: node_key(n),
            kind: n.0.as_str().into(),
            id: n.1.clone(),
            file: s.file.clone(),
            field: s.field.clone(),
            reached: reach.reached.contains(n),
            engine: n.0 == Kind::Flag && engine_flag(&n.1),
            sets: 0,
            reads: 0,
            problems: Vec::new(),
        });
        if s.role == Role::Defines {
            e.file.clone_from(&s.file);
            e.field.clone_from(&s.field);
        }
        match s.role {
            Role::Sets => e.sets += 1,
            Role::Reads => e.reads += 1,
            _ => {}
        }
    };
    for s in &ix.sites {
        if !GRAPH_KINDS.contains(&s.kind) {
            continue;
        }
        // A cutscene's seen flag is drawn as the cutscene.
        if s.kind == Kind::Flag
            && s.id.starts_with("cutscene.")
            && ix.defines(Kind::Cutscene, &s.id["cutscene.".len()..])
        {
            continue;
        }
        if s.kind == Kind::Flag && s.id.starts_with("map.") {
            continue;
        }
        if matches!(s.kind, Kind::Flag | Kind::Counter) || s.role == Role::Defines {
            add(&(s.kind, s.id.clone()), s, &mut nodes);
        }
    }

    let mut edges: BTreeMap<(String, String, &'static str), GraphEdge> = BTreeMap::new();
    let mut edge = |from: String, to: String, role: &'static str, s: &Site| {
        if from == to {
            return;
        }
        edges
            .entry((from.clone(), to.clone(), role))
            .or_insert(GraphEdge {
                from,
                to,
                role,
                file: s.file.clone(),
                field: s.field.clone(),
            });
    };
    let mut memo = BTreeMap::new();
    for s in &ix.sites {
        if s.role == Role::Defines {
            continue;
        }
        let target: Node = (s.kind, s.id.clone());
        let Some(owner) = &s.owner else { continue };
        let hs = holders(&ix, owner, &mut memo, &mut BTreeSet::new());
        if s.kind == Kind::Npc && s.field.ends_with(".giver") {
            // The giver's map offers the quest.
            for h in holders(&ix, &target, &mut memo, &mut BTreeSet::new()) {
                edge(node_key(&h), node_key(owner), "gives", s);
            }
            continue;
        }
        if !nodes.contains_key(&target) {
            continue;
        }
        for h in hs {
            if !nodes.contains_key(&h) {
                continue;
            }
            let (from, to, role) = match (s.in_gate.is_some(), s.role) {
                (true, _) | (_, Role::Reads) => (node_key(&target), node_key(&h), "reads"),
                (_, Role::Sets) => (node_key(&h), node_key(&target), "sets"),
                _ => (node_key(&h), node_key(&target), "leads"),
            };
            // A cutscene "reads" the map it plays on: drawn map -> cutscene.
            edge(from, to, role, s);
        }
    }
    // The entry node: retail scenes into the entrance maps, and quests that
    // start on their own.
    let entry_key = "entry:".to_string();
    let mut has_entry = false;
    for id in &reach.entry_maps {
        let n = (Kind::Map, id.clone());
        if let Some(gn) = nodes.get(&n) {
            has_entry = true;
            edges.insert(
                (entry_key.clone(), gn.key.clone(), "enters"),
                GraphEdge {
                    from: entry_key.clone(),
                    to: gn.key.clone(),
                    role: "enters",
                    file: gn.file.clone(),
                    field: "entrances".into(),
                },
            );
        }
    }
    for id in &reach.auto_quests {
        let n = (Kind::Quest, id.clone());
        if let Some(gn) = nodes.get(&n) {
            has_entry = true;
            edges.insert(
                (entry_key.clone(), gn.key.clone(), "leads"),
                GraphEdge {
                    from: entry_key.clone(),
                    to: gn.key.clone(),
                    role: "leads",
                    file: gn.file.clone(),
                    field: gn.field.replace(".id", ".auto_start"),
                },
            );
        }
    }
    let mut problems = Vec::new();
    for f in found {
        let p = GraphProblem {
            file: f.issue.path.clone(),
            field: f.issue.field.clone(),
            message: f.issue.message.clone(),
        };
        // A seen-flag finding lands on its cutscene node when one is drawn.
        let node = match &f.node {
            (Kind::Flag, id) if id.starts_with("cutscene.") => {
                (Kind::Cutscene, id["cutscene.".len()..].to_string())
            }
            n => n.clone(),
        };
        let node = if nodes.contains_key(&node) {
            node
        } else {
            f.node.clone()
        };
        match nodes.get_mut(&node) {
            Some(n) => n.problems.push(p),
            None => problems.push(p),
        }
    }
    let mut nodes: Vec<GraphNode> = nodes.into_values().collect();
    if has_entry {
        nodes.insert(
            0,
            GraphNode {
                key: entry_key,
                kind: "entry".into(),
                id: "Retail game".into(),
                file: "pack.toml".into(),
                field: String::new(),
                reached: true,
                engine: false,
                sets: 0,
                reads: 0,
                problems: Vec::new(),
            },
        );
    }
    Graph {
        nodes,
        edges: edges.into_values().collect(),
        problems,
    }
}
