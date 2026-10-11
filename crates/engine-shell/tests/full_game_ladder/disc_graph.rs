//! The disc's scene-change graph: every decoded `0x3F` edge and FMV
//! hand-off, and the route search between two scenes over it.

use super::*;

// ---------------------------------------------------------------------------
// The disc scene graph
// ---------------------------------------------------------------------------

/// The disc's scene graph: every CDNAME scene's decoded `0x3F` destinations
/// over every MAN carrier the scene has (bundle + streaming variants), plus
/// the FMV hand-off edges - a record that triggers movie `id` (`4C E2 id`)
/// leaves for the scene retail's post-play dispatch (`FUN_801CEA3C`) names.
///
/// Each edge also records whether a **walk-on** band reaches it (a gate-1
/// `.MAP` trigger whose partition-2 record carries the `0x3F` or the FMV, in
/// the MAN the live host loads). Routes prefer those: an edge only a talk or
/// scripted record carries costs [`SCRIPTED_EDGE_COST`] hops.
pub(super) struct DiscGraph {
    pub(super) edges: BTreeMap<String, BTreeSet<String>>,
    /// `(from, to)` pairs some `0x3F` names.
    pub(super) doors: BTreeSet<(String, String)>,
    /// `(from, to)` -> the `(partition, record)` sites whose FMV makes the hop.
    pub(super) fmv: BTreeMap<(String, String), BTreeSet<(usize, usize)>>,
    /// `(from, to)` pairs a walk-on band reaches.
    pub(super) walk_on: BTreeSet<(String, String)>,
    /// `(from, to)` -> every entry tile a `0x3F` from `from` lands on in
    /// `to`: which side of `to` a round trip through `from` delivers to.
    pub(super) landings: BTreeMap<(String, String), BTreeSet<(u8, u8)>>,
}

/// Route cost of an edge no walk-on band reaches.
pub(super) const SCRIPTED_EDGE_COST: usize = 6;

impl DiscGraph {
    pub(super) fn build(index: &ProtIndex) -> Self {
        use legaia_engine_system::movie::cutscene::{FmvHandoff, fmv_post_play_handoff};
        use legaia_scene_formats::field_disasm::{find_fmv_triggers, partition_record_span};
        let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut doors = BTreeSet::new();
        let mut fmv: BTreeMap<(String, String), BTreeSet<(usize, usize)>> = BTreeMap::new();
        let mut walk_on = BTreeSet::new();
        let mut landings: BTreeMap<(String, String), BTreeSet<(u8, u8)>> = BTreeMap::new();
        for name in index.cdname_scene_names() {
            let Ok(scene) = Scene::load(index, &name) else {
                continue;
            };
            let set = edges.entry(name.clone()).or_default();
            for c in scene_man_carriers(index, &scene) {
                let man = &c.payload;
                let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
                    continue;
                };
                for d in scene_destinations(&mf, man) {
                    if d.scene_name != name {
                        set.insert(d.scene_name.clone());
                        doors.insert((name.clone(), d.scene_name));
                    }
                }
                // Landings come from the partition-2 door records only - the
                // records a walk-on band or a scripted beat runs, which are
                // the exits the pad hand leaves a crossing by. A
                // partition-1 talk record's `0x3F` is a conversation's own
                // exit on its own story gate: `dolk2` P1[47]'s lands on
                // `map01` `(65, 50)`, which made `dolk2` read as the way to
                // `vell`'s side when neither of its gate bands goes there
                // (retail's playthrough drains `suimon` instead: `0x27B` is
                // set and P1[47]'s other write, `0x17D`, is not, in every
                // card save from `PRO-01` on). `scene_destinations` keeps one
                // entry per destination; a scene with two exits to one map
                // (a town's two gates) has two landings.
                for s in legaia_scene_formats::man_edit::scene_change_sites(man) {
                    if s.partition == 2 && s.name != name && set.contains(&s.name) {
                        landings
                            .entry((name.clone(), s.name.clone()))
                            .or_default()
                            .insert((s.entry_x & 0x7F, s.entry_z & 0x7F));
                    }
                }
                for partition in 1..3 {
                    let n = mf.partitions.get(partition).map_or(0, Vec::len);
                    for rec in 0..n {
                        let Some((start, pc0, len)) =
                            partition_record_span(&mf, man, partition, rec)
                        else {
                            continue;
                        };
                        for (_, id) in find_fmv_triggers(&man[start + pc0..start + len]) {
                            if let FmvHandoff::Field { scene: dest, .. } = fmv_post_play_handoff(id)
                                && dest != name
                            {
                                set.insert(dest.to_string());
                                fmv.entry((name.clone(), dest.to_string()))
                                    .or_default()
                                    .insert((partition, rec));
                            }
                        }
                    }
                }
            }
            // Walk-on reachability, in the MAN the live host loads.
            if let Ok(Some(man)) = scene.field_man_payload(index)
                && let Ok(mf) = legaia_scene_formats::man_section::parse(&man)
                && let Ok((p, f)) = scene.field_tile_triggers(index)
            {
                let triggers: Vec<TileTrigger> = p.into_iter().chain(f).collect();
                for s in overworld_portal_sites(&mf, &man, &triggers) {
                    walk_on.insert((name.clone(), s.scene_name.clone()));
                    if let Some(c) = &s.conditional {
                        walk_on.insert((name.clone(), c.scene_name.clone()));
                    }
                }
                let g1: BTreeSet<usize> = triggers
                    .iter()
                    .filter(|t| t.gate == 1)
                    .map(|t| usize::from(t.record))
                    .collect();
                for ((from, to), recs) in &fmv {
                    if from == &name && recs.iter().any(|&(p, r)| p == 2 && g1.contains(&r)) {
                        walk_on.insert((from.clone(), to.clone()));
                    }
                }
            }
        }
        Self {
            edges,
            doors,
            fmv,
            walk_on,
            landings,
        }
    }

    pub(super) fn cost(&self, from: &str, to: &str) -> usize {
        if self.walk_on.contains(&(from.to_string(), to.to_string())) {
            1
        } else {
            SCRIPTED_EDGE_COST
        }
    }

    /// Render a route: `>` a walk-on door, `~fmv~>` an FMV hand-off, `=>` a
    /// `0x3F` no walk-on band reaches (a talk, touch or scripted record).
    pub(super) fn show(&self, route: &[String]) -> String {
        let mut s = route[0].clone();
        for w in route.windows(2) {
            let key = (w[0].clone(), w[1].clone());
            let arrow = if self.walk_on.contains(&key) {
                if self.doors.contains(&key) {
                    ">"
                } else {
                    "~fmv~>"
                }
            } else if self.doors.contains(&key) {
                "=>"
            } else {
                "~fmv=>"
            };
            s.push_str(arrow);
            s.push_str(&w[1]);
        }
        s
    }

    /// Cheapest scene route `from -> to` (walk-on edges first), both ends
    /// included.
    pub(super) fn route(&self, from: &str, to: &str) -> Option<Vec<String>> {
        self.route_avoiding(from, to, &BTreeSet::new())
    }

    /// [`Self::route`] without the `dead` edges.
    pub(super) fn route_avoiding(
        &self,
        from: &str,
        to: &str,
        dead: &BTreeSet<(String, String)>,
    ) -> Option<Vec<String>> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        if from == to {
            return Some(vec![from.to_string()]);
        }
        let mut dist: HashMap<String, usize> = HashMap::new();
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(from.to_string(), 0);
        heap.push(Reverse((0usize, from.to_string())));
        while let Some(Reverse((d, cur))) = heap.pop() {
            if cur == to {
                let mut path = vec![cur.clone()];
                let mut s = cur;
                while let Some(p) = prev.get(&s) {
                    path.push(p.clone());
                    s = p.clone();
                }
                path.reverse();
                return Some(path);
            }
            if dist.get(&cur).is_some_and(|&best| d > best) {
                continue;
            }
            for n in self.edges.get(&cur).into_iter().flatten() {
                if dead.contains(&(cur.clone(), n.clone())) {
                    continue;
                }
                let nd = d + self.cost(&cur, n);
                if dist.get(n).is_none_or(|&best| nd < best) {
                    dist.insert(n.clone(), nd);
                    prev.insert(n.clone(), cur.clone());
                    heap.push(Reverse((nd, n.clone())));
                }
            }
        }
        None
    }
}
