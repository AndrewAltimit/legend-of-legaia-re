//! Battle actors: the party characters assembled from their player files and
//! the monsters of the monster archive, each with its own VRAM image and its
//! battle animation clips.
//!
//! Everything here is a call into `legaia_actor_formats`:
//! `battle_char_assembly::loadout::build` (the per-character equipment
//! assembly, texture band and clip bank, PROT 863..866 = `PLAYER1..4`) and
//! `monster_archive` (records, mesh + texture pool, action animations, PROT
//! 867). This module only packages the result for the previews.

use anyhow::{Context, Result, anyhow, bail};
use legaia_actor_formats::battle_char_assembly::{self as bca, loadout};
use legaia_actor_formats::monster_archive::{self, MonsterAnimation, PartPose};
use legaia_game_tables::equip_stats::EquipStatTable;
use legaia_game_tables::item_names::ItemNameTable;

use crate::Library;
use crate::raster::Model;

/// PROT entry of the monster archive (`0867_battle_data`).
pub const MONSTER_ARCHIVE_ENTRY: u32 = 867;

/// One animation clip, keyframes per object.
pub struct Clip {
    pub label: String,
    pub frames: Vec<Vec<PartPose>>,
    /// Keyframes per second: retail advances `rate / 8` keyframes per 60 Hz
    /// tick (`MonsterAnimation::rate`).
    pub fps: f32,
}

impl Clip {
    fn from_anim(label: String, a: &MonsterAnimation) -> Option<Self> {
        if a.frame_count == 0 || a.part_count == 0 {
            return None;
        }
        Some(Self {
            label,
            frames: a.frames.clone(),
            fps: a.rate.max(1) as f32 * 7.5,
        })
    }
}

/// A posable, textured actor.
pub struct Actor {
    pub label: String,
    pub model: Model,
    pub vram: legaia_tim::Vram,
    pub clips: Vec<Clip>,
    pub notes: Vec<String>,
}

impl Actor {
    /// Pose on `clip`'s keyframe `frame` (both clamped).
    pub fn pose(&mut self, clip: usize, frame: usize) {
        if let Some(c) = self.clips.get(clip)
            && let Some(f) = c.frames.get(frame.min(c.frames.len().saturating_sub(1)))
        {
            self.model.pose(f);
        }
    }
}

/// A monster archive row for the picker.
#[derive(Debug, Clone)]
pub struct MonsterRow {
    pub id: u16,
    pub name: String,
    pub hp: u16,
}

/// Every populated monster record, by archive id.
pub fn monster_rows(lib: &Library) -> Result<Vec<MonsterRow>> {
    let entry = entry(lib, MONSTER_ARCHIVE_ENTRY)?;
    Ok(monster_archive::records(&entry)?
        .into_iter()
        .filter(|r| !r.name.trim().is_empty())
        .map(|r| MonsterRow {
            id: r.id,
            name: r.name,
            hp: r.hp,
        })
        .collect())
}

/// Monster `id`, textured the way the battle loader places it in slot 0.
pub fn monster(lib: &Library, id: u16) -> Result<Actor> {
    let entry = entry(lib, MONSTER_ARCHIVE_ENTRY)?;
    let mesh = monster_archive::mesh(&entry, id)?.ok_or_else(|| anyhow!("slot {id} is empty"))?;
    let name = monster_archive::record(&entry, id)?
        .map(|r| r.name)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("monster {id}"));
    let mut vram = legaia_tim::Vram::new();
    // `battle_render_mesh` uploads the pool and relocates CBA/TSB; the same
    // relocation is applied to the object-tagged mesh the poser needs.
    mesh.battle_render_mesh(0, &mut vram)
        .context("monster TMD does not parse")?;
    let tmd = legaia_tmd::parse(mesh.tmd_bytes())?;
    let (mut vmesh, ids) =
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes());
    for ct in &mut vmesh.cba_tsb {
        ct[0] = monster_archive::relocate_cba(ct[0], 0);
        ct[1] = monster_archive::relocate_tsb(ct[1], 0);
    }
    let model = Model::from_mesh(vmesh, ids, None).context("monster mesh has no triangles")?;
    let clips = monster_archive::animations(&entry, id)?
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter_map(|(i, a)| Clip::from_anim(format!("#{i} action {:#04x}", a.action_id), a))
        .collect();
    let mut actor = Actor {
        label: name,
        model,
        vram,
        clips,
        notes: Vec::new(),
    };
    actor.pose(0, 0);
    actor.model.refit();
    Ok(actor)
}

/// One character's equipment choices.
pub struct CharacterCatalog {
    pub cslot: usize,
    pub name: &'static str,
    /// Per section: its label and the `(id, item name)` it offers; id `0` is
    /// the section default (bare).
    pub sections: Vec<(String, Vec<(u8, String)>)>,
}

/// The four party members' section catalogs, with item names from the boot
/// executable when it can be read.
pub fn character_catalogs(lib: &Library) -> Result<Vec<CharacterCatalog>> {
    let scus = lib.executable().ok();
    let names = scus.as_deref().and_then(ItemNameTable::from_scus);
    let stats = scus.as_deref().and_then(EquipStatTable::from_scus);
    let mut out = Vec::new();
    for (cslot, name) in loadout::CHARACTER_LABELS.iter().enumerate() {
        let raw = entry(lib, loadout::PLAYER_FILE_BASE + cslot as u32)?;
        let pack = legaia_actor_formats::battle_data_pack::parse(&raw)?;
        let cats = loadout::section_catalog(&pack);
        let labels = loadout::section_labels(&cats, stats.as_ref(), cslot);
        let sections = cats
            .iter()
            .zip(labels)
            .map(|(c, label)| {
                let mut ids = vec![(0u8, "(default)".to_string())];
                ids.extend(c.ids.iter().filter_map(|&id| {
                    let id = u8::try_from(id).ok()?;
                    Some((id, loadout::item_display_name(id as u32, names.as_ref())))
                }));
                (label, ids)
            })
            .collect();
        out.push(CharacterCatalog {
            cslot,
            name,
            sections,
        });
    }
    Ok(out)
}

/// Party member `cslot` (0 Vahn, 1 Noa, 2 Gala, 3 Terra) wearing `equipped`.
pub fn character(lib: &Library, cslot: usize, equipped: [u8; bca::SECTION_COUNT]) -> Result<Actor> {
    if cslot >= loadout::CHARACTER_LABELS.len() {
        bail!("no party slot {cslot}");
    }
    let raw = entry(lib, loadout::PLAYER_FILE_BASE + cslot as u32)?;
    let c = loadout::build(&raw, cslot, equipped, false, &[]).map_err(|e| anyhow!(e))?;
    let model = Model::from_mesh(c.mesh, c.object_ids, None).context("assembled mesh is empty")?;
    let clips = c
        .clips
        .iter()
        .filter_map(|k| Clip::from_anim(k.label.clone(), &k.anim))
        .collect();
    let mut actor = Actor {
        label: loadout::CHARACTER_LABELS[cslot].to_string(),
        model,
        vram: c.vram,
        clips,
        notes: c.notes,
    };
    actor.pose(0, 0);
    actor.model.refit();
    Ok(actor)
}

fn entry(lib: &Library, index: u32) -> Result<Vec<u8>> {
    let pos = lib
        .position_of(index)
        .with_context(|| format!("no PROT entry {index}"))?;
    lib.entry_bytes(pos)
}

/// Monster `id` as an animated `.glb` (`legaia_actor_formats::monster_gltf`).
pub fn monster_glb(lib: &Library, id: u16) -> Result<Vec<u8>> {
    let entry = entry(lib, MONSTER_ARCHIVE_ENTRY)?;
    legaia_actor_formats::monster_gltf::export_glb(&entry, id)?
        .ok_or_else(|| anyhow!("slot {id} has no textured geometry"))
}

/// Party member `cslot` in `equipped` as an animated `.glb` with its whole
/// clip bank (`legaia_actor_formats::character_gltf`).
pub fn character_glb(
    lib: &Library,
    cslot: usize,
    equipped: [u8; bca::SECTION_COUNT],
) -> Result<Vec<u8>> {
    use legaia_actor_formats::character_gltf::{CharacterClip, build_character_glb};
    let raw = entry(lib, loadout::PLAYER_FILE_BASE + cslot as u32)?;
    let c = loadout::build(&raw, cslot, equipped, false, &[]).map_err(|e| anyhow!(e))?;
    let clips: Vec<CharacterClip<'_>> = c
        .clips
        .iter()
        .filter(|k| k.anim.frame_count > 0)
        .map(|k| CharacterClip {
            name: k.label.clone(),
            fps: k.anim.rate.max(1) as f32 * 7.5,
            anim: &k.anim,
        })
        .collect();
    let name = loadout::CHARACTER_LABELS[cslot].to_ascii_lowercase();
    build_character_glb(&name, &c.mesh, &c.object_ids, &c.vram, &clips)
        .context("the glb baker produced nothing")
}

/// Lower-case file label of party member `cslot` (`vahn`, ...).
pub fn character_label(cslot: usize) -> String {
    loadout::CHARACTER_LABELS
        .get(cslot)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| format!("party{cslot}"))
}
