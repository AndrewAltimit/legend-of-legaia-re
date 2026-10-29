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
use legaia_font::cell_text::{display_name, level_mark};
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
    /// The name as the battle font draws it ([`display_name`]).
    pub name: String,
    /// The element badge the name plaque wears, by name (`Fire`, ...).
    pub badge: Option<&'static str>,
    pub hp: u16,
}

/// The element badges of the name plaque, in escape-letter order (`^A` ..
/// `^H`, `docs/subsystems/battle-actor-rendering.md`).
pub const BADGE_NAMES: [&str; 8] = [
    "Fire", "Thunder", "Wind", "Water", "Earth", "Light", "Dark", "Evil",
];

/// Every populated monster record, by archive id.
pub fn monster_rows(lib: &Library) -> Result<Vec<MonsterRow>> {
    let entry = entry(lib, MONSTER_ARCHIVE_ENTRY)?;
    let mark = level_mark(lib.exe.as_deref());
    Ok(monster_archive::records(&entry)?
        .into_iter()
        .filter(|r| !r.name.trim().is_empty())
        .map(|r| MonsterRow {
            id: r.id,
            name: display_name(&r.name, mark),
            badge: r
                .plaque_badge
                .and_then(|b| BADGE_NAMES.get(b as usize).copied()),
            hp: r.hp,
        })
        .collect())
}

/// Monster `id`, textured the way the battle loader places it in slot 0.
pub fn monster(lib: &Library, id: u16) -> Result<Actor> {
    let entry = entry(lib, MONSTER_ARCHIVE_ENTRY)?;
    let mesh = monster_archive::mesh(&entry, id)?.ok_or_else(|| anyhow!("slot {id} is empty"))?;
    let name = monster_archive::record(&entry, id)?
        .map(|r| display_name(&r.name, level_mark(lib.exe.as_deref())))
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
    // Item names come from the build's own executable (the USA table or a
    // measured PAL one, `item_names::table_va_for_exe`). The equipment stat
    // table (section labels) is read on the USA build only.
    let exe = lib.executable().ok();
    let exe_name = lib
        .exe
        .clone()
        .unwrap_or_else(|| crate::library::USA_EXE.to_string());
    let mark = level_mark(lib.exe.as_deref());
    let names = exe
        .as_deref()
        .and_then(|b| ItemNameTable::from_exe(b, &exe_name));
    let stats = exe
        .as_deref()
        .filter(|_| lib.is_usa())
        .and_then(EquipStatTable::from_scus);
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
                    let name = loadout::item_display_name(id as u32, names.as_ref());
                    Some((id, display_name(&name, mark)))
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
    let (arts, arts_note) = character_arts(lib, &raw, cslot);
    let c = loadout::build(&raw, cslot, equipped, false, &arts).map_err(|e| anyhow!(e))?;
    let model = Model::from_mesh(c.mesh, c.object_ids, None).context("assembled mesh is empty")?;
    let clips = c
        .clips
        .iter()
        .filter_map(|k| Clip::from_anim(clip_label(k), &k.anim))
        .collect();
    let mut notes = c.notes;
    notes.extend(arts_note);
    let mut actor = Actor {
        label: loadout::CHARACTER_LABELS[cslot].to_string(),
        model,
        vram: c.vram,
        clips,
        notes,
    };
    actor.pose(0, 0);
    actor.model.refit();
    Ok(actor)
}

/// The clip list label: the action slot's role, the swing direction, or the
/// art's curated name with its kind, cost and input.
fn clip_label(k: &loadout::EquippedClip) -> String {
    match (&k.art, k.kind) {
        (Some(a), _) => format!(
            "Art: {} ({}, {} AP, {})",
            k.label,
            a.kind,
            a.ap,
            crate::arts::arrows(&a.directions)
        ),
        (None, loadout::ClipKind::Swing) => format!("Swing: {}", k.label),
        (None, _) => k.label.clone(),
    }
}

/// Party member `cslot`'s Tactical Arts clips ([`crate::arts`]), or none
/// plus a note saying why.
fn character_arts(
    lib: &Library,
    player: &[u8],
    cslot: usize,
) -> (Vec<loadout::ArtClip>, Option<String>) {
    let readef = match entry(lib, crate::arts::READEF_ENTRY) {
        Ok(b) => b,
        Err(e) => return (Vec::new(), Some(format!("Tactical Arts: {e:#}"))),
    };
    match crate::arts::art_clips(player, &readef, cslot) {
        Ok(a) => (a, None),
        Err(e) => (Vec::new(), Some(format!("Tactical Arts: {e}"))),
    }
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
    let (arts, _) = character_arts(lib, &raw, cslot);
    let c = loadout::build(&raw, cslot, equipped, false, &arts).map_err(|e| anyhow!(e))?;
    let clips: Vec<CharacterClip<'_>> = c
        .clips
        .iter()
        .filter(|k| k.anim.frame_count > 0)
        .map(|k| CharacterClip {
            name: match k.kind {
                loadout::ClipKind::Art => format!("art_{}", k.label),
                _ => k.label.clone(),
            },
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
