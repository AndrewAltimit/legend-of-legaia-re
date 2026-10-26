//! **Inbound arrivals**: every spot the disc's doors and warps land the party
//! in a given scene, gathered from the *other* scenes' scripts.
//!
//! A retail scene change always carries its arrival tile: the field VM's
//! `0x3F` named-scene-change op names the destination scene and the entry
//! tile + facing sector the player stands on there
//! ([`legaia_scene_formats::man_edit::scene_change_sites`], the same clean
//! partition walk the door randomizer edits). Gathering those ops across every
//! scene's MAN and keying them by destination answers "where does a door put
//! me in scene X" - which is what the engine's **scene picker** needs, since a
//! picked scene is one nobody walked into and so has no arrival operand of its
//! own.
//!
//! Each arrival is classed by how the player reaches it:
//!
//! - [`ArrivalClass::Entrance`] - an overworld (`map01..03`) walk-on entrance
//!   into the scene: the town / dungeon gate.
//! - [`ArrivalClass::WalkDoor`] - a door in a field scene: the carrying
//!   partition-2 record is spawned by a `.MAP` kind-1 gate-1 walk-on tile
//!   trigger ([`legaia_scene_formats::man_edit::site_p2_slots`] joined to
//!   [`crate::scene::Scene::field_tile_triggers`]), the structural definition
//!   of "a door you walk through" the randomizer's shuffle pool uses too.
//! - [`ArrivalClass::Scripted`] - every other `0x3F`: a scene change a script
//!   or cutscene runs (story hops, event warps). Still a retail arrival spot,
//!   ranked last.
//!
//! [`ArrivalCatalog::ranked`] orders one scene's arrivals: class first
//! (`Entrance`, then `WalkDoor`, then `Scripted`), then the arrival tile more
//! inbound sites share, then disc order (source scene in CDNAME order, then
//! the op's offset in its MAN). Arrivals sharing a tile collapse onto the
//! first one. Whether a ranked arrival is actually standable is the world's
//! call, once the destination's walkability grid is loaded
//! (`World::picker_arrival_seat` in `legaia-engine-core`).

use std::collections::BTreeMap;

use crate::scene::{ProtIndex, Scene, is_world_map_scene};

/// How the player reaches an arrival (see the module docs). Ordered by
/// preference: a lower variant ranks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArrivalClass {
    /// An overworld entrance into the scene (town / dungeon gate).
    Entrance,
    /// A walk-on door in a field scene.
    WalkDoor,
    /// A scripted / cutscene scene change.
    Scripted,
}

/// One inbound arrival: a `0x3F` site in `from_scene` that lands the party in
/// the catalogued destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneArrival {
    /// CDNAME label of the scene whose script carries the scene change.
    pub from_scene: String,
    /// Offset of the `0x3F` op in `from_scene`'s decompressed MAN.
    pub op_pc: usize,
    /// Entry-tile X byte (low 7 bits the tile, bit 7 the far half).
    pub entry_x: u8,
    /// Entry-tile Z byte (same encoding).
    pub entry_z: u8,
    /// Arrival facing sector (`dir & 7` through the SCUS compass table).
    pub dir: u8,
    /// How the player reaches it.
    pub class: ArrivalClass,
}

impl SceneArrival {
    /// The arrival's world `(x, z)`: retail's entry-byte decode (`FUN_801DE840`
    /// case `0x3F`), `(b & 0x7F) * 0x80 + 0x40`, `+0x80` instead of `+0x40`
    /// when bit 7 is set - the seat the door transition itself produces.
    pub fn world_xz(&self) -> (i16, i16) {
        (
            decode_entry_byte(self.entry_x),
            decode_entry_byte(self.entry_z),
        )
    }
}

/// Retail's `0x3F` entry-byte decode (see [`SceneArrival::world_xz`]).
pub fn decode_entry_byte(b: u8) -> i16 {
    i16::from(b & 0x7F) * 128 + if b & 0x80 != 0 { 0x80 } else { 0x40 }
}

/// Every inbound arrival on the disc, keyed by destination scene.
#[derive(Debug, Clone, Default)]
pub struct ArrivalCatalog {
    by_dest: BTreeMap<String, Vec<SceneArrival>>,
    /// Per scene, the first global-pool track its entry script (MAN
    /// partition-1 record `0`) starts ([`entry_script_bgm`]).
    home_bgm: BTreeMap<String, u16>,
}

/// The global-pool range a BGM start can name (`music_01`, `2000 +` the
/// sound-test slot).
const GLOBAL_BGM_IDS: std::ops::Range<u16> = 2000..2082;

/// The first global-pool track the entry script of a decompressed scene MAN
/// starts (op `0x35` sub-op `1` / `9` in partition-1 record `0`, the
/// scene-entry system script, walked linearly from its first opcode). The
/// park sentinel `0x1000` and scene-local ids are skipped, so a scene whose
/// entry parks first and starts its own track under a story or position test
/// still names that track. `None` when record `0` starts none.
pub fn entry_script_bgm(man: &[u8]) -> Option<u16> {
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let mf = legaia_scene_formats::man_section::parse(man).ok()?;
    let n1 = mf.header.partition_counts[1].max(0) as usize;
    let start = mf.actor_placement_record_offset(0, man.len())?;
    let end = (1..n1)
        .filter_map(|i| mf.actor_placement_record_offset(i, man.len()))
        .filter(|&o| o > start)
        .min()
        .unwrap_or(man.len());
    let body = man.get(start..end)?;
    let pc0 = 1 + usize::from(*body.first()?) * 2 + 4;
    if pc0 >= body.len() {
        return None;
    }
    LinearWalker::new(body, pc0)
        .flatten()
        .find_map(|insn| match insn.info {
            InsnInfo::Bgm {
                text_id,
                sub_op: 1 | 9,
            } if GLOBAL_BGM_IDS.contains(&text_id) => Some(text_id),
            _ => None,
        })
}

impl ArrivalCatalog {
    /// Walk every CDNAME scene's MAN for its `0x3F` sites and key them by
    /// destination. Scenes without a MAN (data / audio / system blocks)
    /// contribute nothing. Empty when the index carries no CDNAME map.
    pub fn build(index: &ProtIndex) -> Self {
        let mut by_dest: BTreeMap<String, Vec<SceneArrival>> = BTreeMap::new();
        let mut home_bgm: BTreeMap<String, u16> = BTreeMap::new();
        let mut seen = std::collections::BTreeSet::new();
        for name in index.cdname_scene_names() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Ok(scene) = Scene::load(index, &name) else {
                continue;
            };
            let Ok(Some(man)) = scene.field_man_payload(index) else {
                continue;
            };
            if let Some(id) = entry_script_bgm(&man) {
                home_bgm.insert(name.clone(), id);
            }
            let sites = legaia_scene_formats::man_edit::scene_change_sites(&man);
            if sites.is_empty() {
                continue;
            }
            let slots = legaia_scene_formats::man_edit::site_p2_slots(&man, &sites);
            let walk_slots: std::collections::BTreeSet<usize> = scene
                .field_tile_triggers(index)
                .map(|(p, f)| {
                    p.iter()
                        .chain(f.iter())
                        .filter(|t| t.gate == 1)
                        .map(|t| usize::from(t.record))
                        .collect()
                })
                .unwrap_or_default();
            let from_overworld = is_world_map_scene(&name);
            for (site, slot) in sites.into_iter().zip(slots) {
                let walked = slot.is_some_and(|s| walk_slots.contains(&s));
                let class = match (walked, from_overworld) {
                    (true, true) => ArrivalClass::Entrance,
                    (true, false) => ArrivalClass::WalkDoor,
                    (false, _) => ArrivalClass::Scripted,
                };
                by_dest.entry(site.name).or_default().push(SceneArrival {
                    from_scene: name.clone(),
                    op_pc: site.op_pc,
                    entry_x: site.entry_x,
                    entry_z: site.entry_z,
                    dir: site.dir,
                    class,
                });
            }
        }
        Self { by_dest, home_bgm }
    }

    /// The first global-pool track `scene`'s entry script starts
    /// ([`entry_script_bgm`]), `None` for a scene whose entry starts none.
    pub fn home_bgm(&self, scene: &str) -> Option<u16> {
        self.home_bgm.get(scene).copied()
    }

    /// Every arrival into `scene`, in disc order (unranked, duplicates kept).
    pub fn arrivals(&self, scene: &str) -> &[SceneArrival] {
        self.by_dest.get(scene).map_or(&[], Vec::as_slice)
    }

    /// The distinct arrival spots into `scene`, best first (see the module
    /// docs for the rule). Empty when nothing on the disc lands there.
    pub fn ranked(&self, scene: &str) -> Vec<SceneArrival> {
        let all = self.arrivals(scene);
        let tile = |a: &SceneArrival| (a.entry_x, a.entry_z);
        // Inbound sites per (class, tile): the "most common landing" weight.
        let mut weight: BTreeMap<(ArrivalClass, (u8, u8)), usize> = BTreeMap::new();
        for a in all {
            *weight.entry((a.class, tile(a))).or_default() += 1;
        }
        let mut out: Vec<(usize, &SceneArrival)> = Vec::new();
        for (order, a) in all.iter().enumerate() {
            // One row per tile: the best class it is reached by, first site.
            if let Some(pos) = out.iter().position(|(_, b)| tile(b) == tile(a)) {
                if a.class < out[pos].1.class {
                    out[pos] = (order, a);
                }
                continue;
            }
            out.push((order, a));
        }
        out.sort_by(|(oa, a), (ob, b)| {
            a.class
                .cmp(&b.class)
                .then_with(|| weight[&(b.class, tile(b))].cmp(&weight[&(a.class, tile(a))]))
                .then(oa.cmp(ob))
        });
        out.into_iter().map(|(_, a)| a.clone()).collect()
    }

    /// Destination scenes with at least one inbound arrival.
    pub fn destinations(&self) -> impl Iterator<Item = &str> {
        self.by_dest.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(from: &str, pc: usize, x: u8, z: u8, class: ArrivalClass) -> SceneArrival {
        SceneArrival {
            from_scene: from.into(),
            op_pc: pc,
            entry_x: x,
            entry_z: z,
            dir: 0,
            class,
        }
    }

    #[test]
    fn decode_matches_retail_half_tile_rule() {
        assert_eq!(decode_entry_byte(0x10), 0x10 * 128 + 0x40);
        assert_eq!(decode_entry_byte(0x90), 0x10 * 128 + 0x80);
    }

    #[test]
    fn ranking_prefers_class_then_shared_tile_then_disc_order() {
        let mut c = ArrivalCatalog::default();
        c.by_dest.insert(
            "dest".into(),
            vec![
                arrival("a", 1, 5, 5, ArrivalClass::Scripted),
                arrival("b", 2, 7, 7, ArrivalClass::WalkDoor),
                arrival("c", 3, 9, 9, ArrivalClass::WalkDoor),
                arrival("d", 4, 9, 9, ArrivalClass::WalkDoor),
                arrival("e", 5, 5, 5, ArrivalClass::Entrance),
            ],
        );
        let r = c.ranked("dest");
        let tiles: Vec<(u8, u8)> = r.iter().map(|a| (a.entry_x, a.entry_z)).collect();
        // (5,5) is reached by an entrance (its scripted site collapses onto
        // it); (9,9) carries two walk doors, (7,7) one.
        assert_eq!(tiles, vec![(5, 5), (9, 9), (7, 7)]);
        assert_eq!(r[0].from_scene, "e");
        assert_eq!(r[1].from_scene, "c");
        assert!(c.ranked("nowhere").is_empty());
    }
}
