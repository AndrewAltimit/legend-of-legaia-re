//! What the user's disc offers a pack: the coordinate spaces a pack names
//! disc content in ([`DiscRef`]), listed with the names the pickers show.
//!
//! Every list is read off the disc the user loaded (monster records from the
//! PROT 867 archive, item and spell names from `SCUS_942.54`, scene blocks
//! from `CDNAME.TXT`) or from the curated `legaia_gamedata` labels (scene
//! display names, music titles). The names are shown to the user in this
//! tab only; a pack records the coordinate, never the name - a picker writes
//! `{ kind = "monster", id = 109 }`, and the modder's own `name` field stays
//! whatever they type.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Serialize;

use legaia_actor_formats::monster_archive;
use legaia_engine_scene::npc_catalog::{NpcPick, scene_npc_picks};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_expansion_format::refs::{BGM_COUNT, DiscRef, DiscResolver, PROT_ENTRY_COUNT};
use legaia_game_tables::item_names::ItemNameTable;
use legaia_game_tables::spell_names::SpellNameTable;

/// PROT extraction entry of the monster archive.
pub const MONSTER_ARCHIVE_ENTRY: u32 = 867;

/// One retail monster record, as the battle editor shows it beside a derived
/// monster: the base values a `{ mul = x }` scales.
#[derive(Debug, Clone, Serialize)]
pub struct MonsterRow {
    pub id: u16,
    pub name: String,
    pub hp: u16,
    pub mp: u16,
    pub attack: u16,
    pub defence: u16,
    pub agility: u16,
    pub speed: u16,
    pub intelligence: u16,
    pub exp: u16,
    pub gold: u16,
    pub element: u8,
    pub drop_item: u8,
    pub drop_chance_pct: u8,
    pub spells: Vec<u8>,
    /// The AGL cost of each of the record's swings, in attack-list order:
    /// what a boss script's `attack = n` indexes.
    pub swings: Vec<u8>,
}

/// One named id (an item, a spell, a music cue).
#[derive(Debug, Clone, Serialize)]
pub struct NamedId {
    pub id: u16,
    pub name: String,
}

/// One CDNAME scene block.
#[derive(Debug, Clone, Serialize)]
pub struct SceneRow {
    /// The CDNAME label a [`DiscRef::Scene`] names.
    pub name: String,
    /// The curated display name (`legaia_gamedata` scene table), or the
    /// label when the table has none.
    pub display: String,
    /// `town` / `field` / `world_map` / ... (empty when unknown).
    pub category: String,
}

/// One retail art a pack art's segment may borrow the clip of: a character's
/// action constant (`0x1B..=0x32`) and its name.
#[derive(Debug, Clone, Serialize)]
pub struct ArtClipRow {
    /// `vahn` / `noa` / `gala`.
    pub character: &'static str,
    pub constant: u8,
    pub name: String,
}

/// One row of the disc's arts-name table (`DAT_80075EC4`): what the arts
/// list shows and the combo the Arts tab checks a pack combo against.
#[derive(Debug, Clone, Serialize)]
pub struct ArtComboRow {
    pub character: &'static str,
    pub name: String,
    pub ap: u8,
    /// `L` / `R` / `U` / `D` arrows.
    pub commands: String,
}

/// Everything the pickers list, serialised once per disc.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CatalogLists {
    pub monsters: Vec<MonsterRow>,
    pub items: Vec<NamedId>,
    pub spells: Vec<NamedId>,
    /// Each hero's retail arts, by constant (the segment picker).
    pub art_clips: Vec<ArtClipRow>,
    /// The disc's arts-name table rows (the combo check).
    pub art_combos: Vec<ArtComboRow>,
    pub scenes: Vec<SceneRow>,
    pub bgm: Vec<NamedId>,
}

/// The user's disc, parsed once: the PROT index (for scene geometry) and the
/// picker lists. The image itself is not kept - only `PROT.DAT`.
pub struct DiscCatalog {
    pub index: ProtIndex,
    pub lists: CatalogLists,
    /// Per scene, read once: its geometry pack's length and the placements
    /// a pack NPC can borrow (`None` for a scene that has none to read).
    scenes: std::cell::RefCell<BTreeMap<String, SceneFacts>>,
}

/// What the validator and the NPC picker read off one scene.
#[derive(Debug, Clone, Default)]
struct SceneFacts {
    geometry_len: Option<usize>,
    npcs: Option<Vec<NpcPick>>,
}

impl DiscCatalog {
    /// Parse a Mode 2/2352 disc image.
    pub fn open(image: &[u8]) -> Result<Self> {
        let prot = legaia_iso::image::extract_prot_dat(image)
            .context("PROT.DAT not found in the disc image (is it a Mode 2/2352 .bin?)")?;
        let cdname = legaia_iso::image::extract_cdname_txt(image);
        let scus = legaia_iso::image::extract_scus(image);
        let index = ProtIndex::from_bytes(prot, cdname.as_deref()).context("PROT index")?;
        Self::from_parts(index, scus.as_deref())
    }

    /// Build the catalog from an opened PROT index and the disc's
    /// `SCUS_942.54` (item and spell names; `None` lists ids only).
    pub fn from_parts(index: ProtIndex, scus: Option<&[u8]>) -> Result<Self> {
        let mut lists = CatalogLists::default();

        let archive = index
            .entry_bytes(MONSTER_ARCHIVE_ENTRY)
            .context("reading the monster archive")?;
        for r in monster_archive::records(&archive).context("decoding the monster archive")? {
            lists.monsters.push(MonsterRow {
                id: r.id,
                name: r.name.clone(),
                hp: r.hp,
                mp: r.mp,
                attack: r.attack(),
                defence: r.defense_high(),
                agility: r.agility(),
                speed: r.speed(),
                intelligence: r.intelligence(),
                exp: r.exp,
                gold: r.gold,
                element: r.element,
                drop_item: r.drop_item,
                drop_chance_pct: r.drop_chance_pct,
                spells: r.magic_attacks.clone(),
                swings: r
                    .spells
                    .iter()
                    .filter(|s| s.is_castable())
                    .map(|s| s.agl_cost)
                    .collect(),
            });
        }

        if let Some(scus) = scus {
            if let Some(t) = ItemNameTable::from_scus(scus)
                .map(|t| t.drawn(legaia_font::cell_text::level_mark(None)))
            {
                for id in 1..=u8::MAX {
                    if let Some(n) = t.name(id).filter(|n| !n.trim().is_empty()) {
                        lists.items.push(NamedId {
                            id: id.into(),
                            name: n.to_string(),
                        });
                    }
                }
            }
            if let Some(oracle) = legaia_art::ArtsOracle::from_scus(scus) {
                for e in oracle.entries() {
                    lists.art_combos.push(ArtComboRow {
                        character: hero_key(e.character),
                        name: e.name.clone(),
                        ap: e.ap,
                        commands: legaia_expansion_format::arts::commands_string(
                            &e.commands.iter().map(|c| c.as_byte()).collect::<Vec<_>>(),
                        ),
                    });
                }
            }
            if let Some(t) = SpellNameTable::from_scus(scus) {
                for id in 0..=u8::MAX {
                    if let Some(n) = t.name(id).filter(|n| !n.trim().is_empty()) {
                        lists.spells.push(NamedId {
                            id: id.into(),
                            name: n.to_string(),
                        });
                    }
                }
            }
        }

        lists.art_clips = art_clips();
        let db = legaia_gamedata::Database::load();
        for name in index.cdname_scene_names() {
            let row = db.scene_by_id(&name);
            lists.scenes.push(SceneRow {
                display: row.map_or_else(|| name.clone(), |r| r.name.clone()),
                category: row.map(|r| r.category.clone()).unwrap_or_default(),
                name,
            });
        }
        lists.bgm = bgm_list(&db);

        Ok(Self {
            index,
            lists,
            scenes: Default::default(),
        })
    }

    /// Scene `name`'s facts, read on first use.
    fn scene_facts(&self, name: &str) -> SceneFacts {
        if let Some(f) = self.scenes.borrow().get(name) {
            return f.clone();
        }
        let facts = SceneFacts {
            geometry_len: Scene::load(&self.index, name)
                .ok()
                .and_then(|s| legaia_engine_scene::scene_resources::field_geometry_pack(&s))
                .map(|(_, n)| n),
            npcs: scene_npc_picks(&self.index, name).ok(),
        };
        self.scenes
            .borrow_mut()
            .insert(name.to_string(), facts.clone());
        facts
    }

    /// The placements of scene `name` a pack NPC can borrow its model from,
    /// with the first line each says (the only name retail gives an NPC).
    pub fn scene_npcs(&self, name: &str) -> Result<SceneNpcs> {
        let picks = self
            .scene_facts(name)
            .npcs
            .with_context(|| format!("scene `{name}` has no NPC placements to read"))?;
        Ok(SceneNpcs {
            scene: name.to_string(),
            npcs: picks,
        })
    }

    fn monster(&self, id: u16) -> Option<&MonsterRow> {
        self.lists.monsters.iter().find(|m| m.id == id)
    }

    /// The base scene's environment geometry pack: what a map prop's
    /// `{ object = N }` indexes.
    pub fn scene_objects(&self, name: &str) -> Result<SceneObjects> {
        let assembled = legaia_engine_scene::scene_assembly::assemble_field_scene_with(
            &self.index,
            name,
            &|_, _| Default::default(),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut placed: BTreeMap<usize, u32> = BTreeMap::new();
        for d in assembled.placements.iter().chain(&assembled.terrain) {
            *placed.entry(d.env_slot).or_default() += 1;
        }
        let entry = assembled
            .env_tmds
            .first()
            .map(|&i| assembled.res.tmds[i].entry_idx);
        let mut cells: BTreeMap<(u8, u16, u8), u32> = BTreeMap::new();
        for &i in &assembled.env_tmds {
            let t = &assembled.res.tmds[i];
            for o in &t.tmd.objects {
                let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                    &t.raw,
                    o.primitives_byte_offset,
                    o.primitives_byte_size,
                );
                for p in groups.iter().flat_map(|g| &g.prims) {
                    if let Some(key) = atlas_cell(p.tsb, p.cba, &p.uvs) {
                        *cells.entry(key).or_default() += 1;
                    }
                }
            }
        }
        let mut cells: Vec<SceneCell> = cells
            .into_iter()
            .map(|((tpage, clut, tile), prims)| SceneCell {
                tpage,
                clut,
                tile,
                prims,
            })
            .collect();
        cells.sort_by_key(|c| std::cmp::Reverse(c.prims));
        let objects = assembled
            .env_tmds
            .iter()
            .enumerate()
            .map(|(slot, &i)| {
                let tmd = &assembled.res.tmds[i].tmd;
                let mut pts: Vec<(i32, i32)> = Vec::new();
                let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
                let mut prims = 0u32;
                for o in &tmd.objects {
                    prims += o.claimed_n_primitive;
                    for v in &o.vertices {
                        let p = [i32::from(v.x), i32::from(v.y), i32::from(v.z)];
                        for k in 0..3 {
                            lo[k] = lo[k].min(p[k]);
                            hi[k] = hi[k].max(p[k]);
                        }
                        pts.push((p[0], p[2]));
                    }
                }
                let size = if pts.is_empty() {
                    [0; 3]
                } else {
                    [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
                };
                SceneObject {
                    index: slot as u16,
                    prims,
                    size,
                    placed: placed.get(&slot).copied().unwrap_or(0),
                    footprint: convex_hull(pts),
                }
            })
            .collect();
        Ok(SceneObjects {
            scene: name.to_string(),
            entry,
            objects,
            cells,
        })
    }

    /// The base scene's own field map and floor ladder, for a map preview
    /// that keeps the base geometry or borrows its floor heights.
    pub fn base_field(&self, name: &str) -> Option<BaseField> {
        let scene = Scene::load(&self.index, name).ok()?;
        Some((
            scene.field_map_bytes(&self.index).ok().flatten(),
            scene.field_floor_height_lut(&self.index).ok().flatten(),
        ))
    }
}

/// A scene's NPC placements, for the NPC model picker.
#[derive(Debug, Clone, Serialize)]
pub struct SceneNpcs {
    pub scene: String,
    pub npcs: Vec<NpcPick>,
}

/// A base scene's field map (`None` when it has none) and floor ladder.
pub type BaseField = (Option<Vec<u8>>, Option<[i16; 16]>);

/// The sound-test list with the curated titles.
pub fn bgm_list(db: &legaia_gamedata::Database) -> Vec<NamedId> {
    (0..BGM_COUNT)
        .map(|id| {
            let t = db.music_by_index(u32::from(id));
            let name = t
                .and_then(|t| {
                    t.relocalization
                        .clone()
                        .or_else(|| t.ost_gloss.clone())
                        .or_else(|| t.debug_gloss.clone())
                        .or_else(|| t.id.clone())
                })
                .unwrap_or_else(|| format!("Track {id}"));
            NamedId { id, name }
        })
        .collect()
}

/// One mesh of a base scene's geometry pack.
#[derive(Debug, Clone, Serialize)]
pub struct SceneObject {
    /// The `{ object = N }` index.
    pub index: u16,
    pub prims: u32,
    /// Bounding-box extent `[x, y, z]`, world units.
    pub size: [i32; 3],
    /// How many times the retail scene places it.
    pub placed: u32,
    /// The convex hull of its vertices seen from above, `[x, z]` pairs: what
    /// the tile painter draws for a placed prop.
    pub footprint: Vec<[i32; 2]>,
}

/// A base scene's geometry pack.
#[derive(Debug, Clone, Serialize)]
pub struct SceneObjects {
    pub scene: String,
    /// The PROT extraction entry holding the pack (what a `tmd` prop names).
    pub entry: Option<u32>,
    pub objects: Vec<SceneObject>,
    /// The texture cells the pack's meshes sample, most used first: what a
    /// decal, an animated material or a ground texture can name.
    pub cells: Vec<SceneCell>,
}

/// One `32 x 32` atlas cell of a texture page through a palette, in the
/// three words an `AtlasCell` carries, with how many textured prims of the
/// scene's geometry pack sample it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SceneCell {
    /// The texpage word's low byte without its blend bits (`tsb & 0x9F`).
    pub tpage: u8,
    pub clut: u16,
    /// `0..64`: column `u / 32`, row `v / 32` of the page's `8 x 8` atlas.
    pub tile: u8,
    pub prims: u32,
}

/// The atlas cell a textured prim samples: the cell under its UV box's
/// low corner. `None` for an untextured prim.
fn atlas_cell(tsb: u16, cba: u16, uvs: &[(u8, u8)]) -> Option<(u8, u16, u8)> {
    if uvs.is_empty() || (tsb == 0 && cba == 0) {
        return None;
    }
    let u = uvs.iter().map(|q| q.0).min()?;
    let v = uvs.iter().map(|q| q.1).min()?;
    Some(((tsb & 0x9F) as u8, cba, (v / 32) * 8 + u / 32))
}

/// Andrew's monotone chain; the hull is counter-clockwise, without repeats.
fn convex_hull(mut pts: Vec<(i32, i32)>) -> Vec<[i32; 2]> {
    pts.sort_unstable();
    pts.dedup();
    if pts.len() < 3 {
        return pts.into_iter().map(|(x, z)| [x, z]).collect();
    }
    let cross = |o: (i32, i32), a: (i32, i32), b: (i32, i32)| {
        i64::from(a.0 - o.0) * i64::from(b.1 - o.1) - i64::from(a.1 - o.1) * i64::from(b.0 - o.0)
    };
    let mut hull: Vec<(i32, i32)> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &(i32, i32)>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull.into_iter().map(|(x, z)| [x, z]).collect()
}

impl DiscResolver for DiscCatalog {
    fn resolve(&self, r: &DiscRef) -> Result<(), String> {
        match r {
            DiscRef::Monster { id } => match self.monster(*id) {
                Some(_) => Ok(()),
                None => Err(format!(
                    "monster {id} is not a populated record on this disc"
                )),
            },
            DiscRef::Scene { name } => {
                if self.lists.scenes.iter().any(|s| s.name == *name) {
                    Ok(())
                } else {
                    Err(format!("scene `{name}` is not a CDNAME block on this disc"))
                }
            }
            DiscRef::Item { id } => {
                if self.lists.items.is_empty()
                    || self.lists.items.iter().any(|i| i.id == u16::from(*id))
                {
                    Ok(())
                } else {
                    Err(format!("item {id} has no name on this disc"))
                }
            }
            DiscRef::Bgm { id } => {
                if *id < BGM_COUNT {
                    Ok(())
                } else {
                    Err(format!("bgm {id} is past the sound-test list"))
                }
            }
            DiscRef::Tmd { prot, .. } | DiscRef::Tim { prot, .. } => {
                if *prot < PROT_ENTRY_COUNT && (*prot as usize) < self.index.entry_count() {
                    Ok(())
                } else {
                    Err(format!("PROT entry {prot} is not on this disc"))
                }
            }
            DiscRef::Npc { scene, index } => {
                let Some(npcs) = self.scene_facts(scene).npcs else {
                    return Err(format!("scene `{scene}` has no NPC placements"));
                };
                if npcs.iter().any(|p| p.index == *index) {
                    Ok(())
                } else {
                    Err(format!("scene `{scene}` has no placement {index}"))
                }
            }
        }
    }

    fn scene_geometry_len(&self, scene: &str) -> Option<usize> {
        self.scene_facts(scene).geometry_len
    }
}

/// The `arts.toml` key of an art-table character.
fn hero_key(c: legaia_art::Character) -> &'static str {
    match c {
        legaia_art::Character::Vahn => "vahn",
        legaia_art::Character::Noa => "noa",
        legaia_art::Character::Gala => "gala",
    }
}

/// Every hero's retail art constants with their names (the curated art
/// tables, `legaia_art::tables`): the clips a segment may name.
pub fn art_clips() -> Vec<ArtClipRow> {
    let mut out = Vec::new();
    for c in legaia_art::Character::all() {
        for b in 0x1Bu8..=0x32 {
            let Some(a) = legaia_art::ActionConstant::from_byte(b) else {
                continue;
            };
            if let Some(name) = legaia_art::tables::art_name(c, a) {
                out.push(ArtClipRow {
                    character: hero_key(c),
                    constant: b,
                    name: name.to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_cell_of_a_textured_prim() {
        // Page 12, blend bits dropped; UVs from (70, 40): column 2, row 1.
        assert_eq!(
            atlas_cell(0x2C, 0x7C40, &[(70, 40), (100, 40), (70, 60)]),
            Some((0x0C, 0x7C40, 10))
        );
        assert_eq!(atlas_cell(0x8C, 1, &[(255, 255)]), Some((0x8C, 1, 63)));
        assert_eq!(atlas_cell(0x0C, 0x7C40, &[]), None);
    }

    #[test]
    fn hull_of_a_square_with_an_inner_point() {
        let h = convex_hull(vec![(0, 0), (10, 0), (10, 10), (0, 10), (5, 5), (10, 0)]);
        assert_eq!(h, vec![[0, 0], [10, 0], [10, 10], [0, 10]]);
    }

    #[test]
    fn bgm_list_covers_the_sound_test() {
        let db = legaia_gamedata::Database::load();
        let l = bgm_list(&db);
        assert_eq!(l.len(), usize::from(BGM_COUNT));
        assert!(l.iter().all(|t| !t.name.is_empty()));
    }
}
