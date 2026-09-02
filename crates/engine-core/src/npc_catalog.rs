//! Field-**NPC catalog kernel**: every actor a scene's MAN places, with the
//! mesh it is drawn from.
//!
//! An NPC is not a separate asset class. It is a TMD in the scene's own TMD
//! pool selected by a **MAN partition-1 placement record**: the record's model
//! byte indexes the scene TMD list, its anim byte names a record in the scene's
//! ANM bundle (`record index + 1`; `0` = none), and its tile bytes give the
//! spawn. So the catalog is the placement list, resolved against an
//! already-built [`SceneResources`] (see `docs/subsystems/script-vm.md`
//! § placement header and `docs/formats/anm.md` § per-scene bundle).
//!
//! Shared by the browser NPC page + play page (`web-viewer::field_npc` wraps
//! this with its render caches) and the native `legaia-engine export-glb`
//! NPC exporter ([`crate::glb_export`]).

use crate::man_field_scripts::{PlacementKind, classify_placements};
use crate::scene::{ProtIndex, Scene};
use crate::scene_resources::SceneResources;
use crate::world::{FIELD_OFFMAP_HIDE_XZ, GlobalTmd};
use legaia_asset::man_section::ActorPlacement;
use std::sync::Arc;

/// One catalogued placement: the MAN record plus what its script implies.
pub struct NpcEntry {
    pub placement: ActorPlacement,
    /// `"talk"` (carries inline dialog / an interact op), `"door"` (warps to
    /// another scene), or `"prop"` (decorative / script-only).
    pub kind: &'static str,
    /// Field-VM map id for a `door`.
    pub target_map: Option<u8>,
    /// First line of the actor's inline dialog block, when it has one - the
    /// only human-readable label retail gives an NPC.
    pub dialog: Option<String>,
    /// Object count of the resolved TMD (the mesh's bone count ceiling).
    pub nobj: u32,
    /// Parked at the off-map hide box: a **conditional spawn** the scene only
    /// places once a script says so (a story NPC who isn't in town yet). Its
    /// model and clip are fully resolvable - retail just isn't drawing it at
    /// scene load - so the catalog lists it, flagged.
    pub conditional: bool,
    /// `model_index >= 0xF0`: a **global-pool special** (party head / save
    /// point). Its mesh comes from the world's global TMD pool (slot
    /// `model_index - 0xF0`) and its clip from the PROT 0874 locomotion
    /// bundle, not the scene's. Only the play-shape catalog lists these.
    pub special: bool,
}

/// The NPC catalog for one field scene, resolved against the
/// [`SceneResources`] whose `res.tmds` is the model-byte index space.
pub struct NpcCatalog {
    /// CDNAME label the catalog was built for.
    pub scene: String,
    /// Renderable placements, in MAN partition-1 order.
    pub entries: Vec<NpcEntry>,
    /// PROT entry index of the scene's ANM bundle. `None` when the scene
    /// ships no bundle (its actors then have no clip and draw in TMD-local
    /// rest).
    pub anm_prot: Option<u32>,
    /// Party / savepoint placements (`model_index >= 0xF0`), which draw from
    /// the global head pool + the PROT 0874 locomotion bundle rather than the
    /// scene's - excluded from the curated catalog but counted.
    pub special_count: u32,
    /// Multi-object actors the scene gives no way to assemble: their TMD has
    /// several objects (so its vertices are object-local and need a bone pose)
    /// but the placement names no clip, or the scene ships no ANM bundle at
    /// all (Mt. Rikuroa's story actors are the case that exists). The curated
    /// catalog leaves them out - and counts them, so a page can say how many
    /// rather than silently hide them.
    pub unposable_count: u32,
}

/// Decode the first line of an inline dialog block into a display label.
/// Glyph bytes are ASCII-compatible from `0x20`, so the printable run is the
/// text; control bytes (line breaks, the `0x1F` segment lead) end the line.
fn dialog_label(inline: &[u8]) -> Option<String> {
    let segs = crate::dialog::decode_inline_segments(inline);
    let first = segs.into_iter().next()?;
    let line: String = first
        .iter()
        .take_while(|&&b| b != 0x00)
        .filter(|&&b| (0x20..=0x7E).contains(&b))
        .map(|&b| b as char)
        .collect();
    let line = line.trim();
    if line.len() < 2 {
        return None;
    }
    Some(line.chars().take(64).collect())
}

/// Locate the scene's ANM bundle the way the play-window does: the type-0x05
/// section of one of the scene's PROT slots. The descriptor-count seed varies
/// per scene (town01 resolves at 3; the prologue scenes only at >= 5), so try
/// the spread and take the first hit.
pub fn scene_anm_prot(scene: &Scene) -> Option<u32> {
    scene.entries.iter().find_map(|e| {
        let found = [3usize, 5, 6, 7]
            .into_iter()
            .any(|desc| !legaia_asset::player_anm::find_in_entry(&e.bytes, desc).is_empty());
        found.then_some(e.idx)
    })
}

/// The scene's decoded ANM bundle itself (the same spread-scan as
/// [`scene_anm_prot`], returning the first decodable bundle).
pub fn scene_anm_bundle(scene: &Scene) -> Option<legaia_asset::player_anm::PlayerAnmBundle> {
    scene.entries.iter().find_map(|e| {
        [3usize, 5, 6, 7].into_iter().find_map(|desc| {
            legaia_asset::player_anm::find_in_entry(&e.bytes, desc)
                .into_iter()
                .next()
        })
    })
}

/// Catalog every NPC / actor the scene's MAN places. `play_pool` is `Some`
/// for the play-shape build (global-pool specials resolve against it,
/// clipless multi-object actors stay in - matching the native play-window's
/// field-NPC pass), `None` for the curated browse/export shape.
pub fn catalog_scene_npcs(
    index: &ProtIndex,
    name: &str,
    res: &SceneResources,
    play_pool: Option<&[Option<Arc<GlobalTmd>>]>,
) -> Result<NpcCatalog, String> {
    let scene = Scene::load(index, name).map_err(|e| format!("{e:#}"))?;
    let man = scene
        .field_man_payload(index)
        .map_err(|e| format!("MAN: {e:#}"))?
        .ok_or_else(|| format!("{name}: scene has no MAN"))?;
    let mf = legaia_asset::man_section::parse(&man).map_err(|e| format!("MAN parse: {e:#}"))?;

    let anm_prot = scene_anm_prot(&scene);
    let mut entries = Vec::new();
    let mut special_count = 0u32;
    let mut unposable_count = 0u32;
    for (mut p, kind) in classify_placements(&mf, &man) {
        // Retail's placement installer pre-runs the record's spawn prologue,
        // and a flag-dispatched spawn's taken arm relocates the actor before
        // the first frame draws - the header tile is only a staging square
        // (town01's running kids are staged on the standing kids' exact
        // tiles). Resolve the cold fresh-game arm so the catalog's position
        // is where the actor actually stands; a parked-sentinel arm decodes
        // to the hide box and flows into the `conditional` flag below.
        // See `man_field_scripts::placement_spawn_relocation`.
        if !p.special_model
            && let Some((x_enc, z_enc)) =
                crate::man_field_scripts::placement_spawn_relocation(&mf, &man, &p, &|_| false)
        {
            p.tile_x = x_enc & 0x7F;
            p.tile_z = z_enc & 0x7F;
            p.world_x = crate::man_field_scripts::grid_byte_to_world(x_enc);
            p.world_z = crate::man_field_scripts::grid_byte_to_world(z_enc);
        }
        let nobj = if p.special_model {
            // Party / savepoint heads come from the global pool, not the
            // scene's. The curated shape routes them elsewhere; the play
            // shape draws them like the native window does.
            let Some(pool) = play_pool else {
                special_count += 1;
                continue;
            };
            let Some(g) = pool
                .get((p.model_index - 0xF0) as usize)
                .and_then(|s| s.as_ref())
            else {
                continue; // no pool mesh - the native window skips it too
            };
            special_count += 1;
            g.tmd.objects.len() as u32
        } else {
            let Some(t) = res.tmds.get(p.model_index as usize) else {
                continue;
            };
            t.tmd.objects.len() as u32
        };
        // A multi-object TMD's vertices are object-local: without a bone pose
        // it draws as a pile of parts on the origin. The curated shape
        // withholds those; the play shape keeps them (retail draw kind 5
        // draws them raw, and so does the native play-window).
        if !p.special_model && nobj > 1 && (p.anim_id == 0 || anm_prot.is_none()) {
            unposable_count += 1;
            if play_pool.is_none() {
                continue;
            }
        }
        let (label, target_map, dialog) = match &kind {
            PlacementKind::Portal { target_map } => ("door", Some(*target_map), None),
            PlacementKind::Npc { dialog_inline, .. } => (
                "talk",
                None,
                dialog_inline.as_deref().and_then(dialog_label),
            ),
            PlacementKind::Plain => ("prop", None, None),
        };
        // The off-map hide box marks a spawn retail withholds until a script
        // places it - the actor is real and fully resolvable, so the catalog
        // lists it with a flag rather than dropping it the way the field
        // renderer does.
        let conditional = p.world_x == FIELD_OFFMAP_HIDE_XZ && p.world_z == FIELD_OFFMAP_HIDE_XZ;
        let special = p.special_model;
        entries.push(NpcEntry {
            nobj,
            placement: p,
            kind: label,
            target_map,
            dialog,
            conditional,
            special,
        });
    }

    Ok(NpcCatalog {
        scene: name.to_string(),
        entries,
        anm_prot,
        special_count,
        unposable_count,
    })
}
