//! The field-**NPC** catalog both browser pages draw from: every actor a
//! scene's MAN places, with the mesh it is drawn from.
//!
//! The kernel is `legaia_engine_scene::npc_catalog`; this module wraps its
//! result with the page's mesh cache ([`FieldNpcPack`]). The play page builds
//! it with [`build_npc_catalog_play`] (everything the native window draws);
//! the NPC browser page (`legaia-web-viewer`'s `field_npc`) with
//! [`build_npc_catalog_res`] (its curated list). See
//! `docs/subsystems/browser-host.md` for the two catalogs' differences.

use legaia_engine_scene::npc_catalog::NpcEntry;
use legaia_engine_scene::scene::ProtIndex;

/// The NPC catalog for one loaded field scene. The meshes resolve against the
/// scene resources the catalog was built over (same VRAM, same TMD pool): the
/// play page's scene host, or the NPC page's assembled field scene.
pub struct FieldNpcPack {
    /// CDNAME label the catalog was built for.
    pub scene: String,
    /// Renderable placements, in MAN partition-1 order.
    pub entries: Vec<NpcEntry>,
    /// PROT entry index of the scene's ANM bundle, for
    /// `player_anm_record_pose_frames`. `None` when the scene ships no bundle
    /// (its actors then have no clip and draw in TMD-local rest).
    pub anm_prot: Option<u32>,
    /// Party / savepoint placements (`model_index >= 0xF0`), which draw from
    /// the global head pool + the PROT 0874 locomotion bundle rather than the
    /// scene's - excluded from the catalog (the characters page owns them) but
    /// counted so the page can say so.
    pub special_count: u32,
    /// Multi-object actors the scene gives no way to assemble: their TMD has
    /// several objects (so its vertices are object-local and need a bone pose)
    /// but the placement names no clip, or the scene ships no ANM bundle at all
    /// (Mt. Rikuroa's story actors are the case that exists). Drawn raw they
    /// would be a pile of parts on the origin, so they're left out - and
    /// counted, so the page can say how many rather than silently hide them.
    pub unposable_count: u32,
    /// Currently-built mesh, keyed by catalog position:
    /// `(catalog_idx, mesh, object_ids, flat_rgba)`.
    #[allow(clippy::type_complexity)]
    pub cur: Option<(usize, legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u8>)>,
}

/// The NPC browser page's curated catalog, against already-built
/// [`SceneResources`](legaia_engine_scene::scene_resources::SceneResources)
/// (`res.tmds` is the model-byte index space).
pub fn build_npc_catalog_res(
    index: &ProtIndex,
    name: &str,
    res: &legaia_engine_scene::scene_resources::SceneResources,
) -> Result<FieldNpcPack, String> {
    build_npc_catalog_impl(index, name, res, None)
}

/// The **play page's** catalog: every placement the native play-window draws.
///
/// Differs from [`build_npc_catalog_res`] (the NPC browser page's curated
/// list) in two ways, both matching the play-window's field-NPC pass
/// (`engine-shell` `window/assets.rs`):
///
/// - **global-pool specials are included**: a `model_index >= 0xF0` placement
///   (save point / party head) resolves against `global_pool[model - 0xF0]` -
///   the world's PROT 0874 §0 player bank (`World::field_head_pool`) that
///   `enter_field_scene` seeds - and its
///   clip against the locomotion bundle. Skipping them left the save crystal
///   (and story party members) missing from the browser scene.
/// - **clipless multi-object actors are included**: retail draw kind 5 draws
///   every TMD object with the actor's single transform (raw object-local
///   vertices), and the native window does the same, so the play page must
///   too rather than withholding them.
pub fn build_npc_catalog_play(
    index: &ProtIndex,
    name: &str,
    res: &legaia_engine_scene::scene_resources::SceneResources,
    global_pool: &[Option<std::sync::Arc<legaia_engine_scene::model_bank::GlobalTmd>>],
) -> Result<FieldNpcPack, String> {
    build_npc_catalog_impl(index, name, res, Some(global_pool))
}

/// Shared walk behind the two catalog builders - delegates to the kernel
/// (`engine-core::npc_catalog::catalog_scene_npcs`) and wraps the result
/// with the viewer's mesh cache.
fn build_npc_catalog_impl(
    index: &ProtIndex,
    name: &str,
    res: &legaia_engine_scene::scene_resources::SceneResources,
    play_pool: Option<&[Option<std::sync::Arc<legaia_engine_scene::model_bank::GlobalTmd>>]>,
) -> Result<FieldNpcPack, String> {
    let legaia_engine_scene::npc_catalog::NpcCatalog {
        scene,
        entries,
        anm_prot,
        special_count,
        unposable_count,
    } = legaia_engine_scene::npc_catalog::catalog_scene_npcs(index, name, res, play_pool)?;
    Ok(FieldNpcPack {
        scene,
        entries,
        anm_prot,
        special_count,
        unposable_count,
        cur: None,
    })
}

impl FieldNpcPack {
    /// Wrap an already-built catalog (the play page's, which
    /// `scene_actors::bind_scene_actors` builds).
    pub fn from_catalog(cat: legaia_engine_scene::npc_catalog::NpcCatalog) -> Self {
        Self {
            scene: cat.scene,
            entries: cat.entries,
            anm_prot: cat.anm_prot,
            special_count: cat.special_count,
            unposable_count: cat.unposable_count,
            cur: None,
        }
    }
}
