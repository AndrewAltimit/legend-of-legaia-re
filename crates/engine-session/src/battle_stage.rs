//! A battle's stage, resolved once on the `Field -> Battle` edge, and the
//! world state it leaves: the step both play hosts run before they build any
//! battle mesh.
//!
//! The stage class (`World::battle.stage_outdoor`, which the backdrop
//! depth-cue ramp reads every tick), the volumetric fog's stage luminance cap
//! and each monster's battle texture slot used to be written from inside each
//! host's battle mesh build - the native window's behind its renderer gate,
//! so a window with no renderer ran a different battle. [`stage_battle_world`]
//! writes them from the disc alone and hands the resolved stage to the host's
//! draw half, which only reads it. One resolver replaces the two hosts'
//! copies of the stage build.

use legaia_engine_core::scene_host::SceneHost;
use legaia_engine_scene::scene::Scene;
use legaia_engine_scene::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_scene_formats::battle_backdrop::{MirrorXTable, SecondCopy};

/// The PROT entry of the monster archive (meshes, idle clips).
pub const MONSTER_ARCHIVE_PROT_INDEX: u32 = 867;

/// The scene's battle-stage backdrop bundle.
pub struct BattleStage {
    /// The PROT entry of the stage stream the fight's region names.
    pub stage_entry: u32,
    /// Scene + stage-dome textures resident - becomes the battle VRAM base.
    pub vram: legaia_tim::Vram,
    /// The backdrop shell TMD plus the raw bytes it was parsed from.
    pub dome: (legaia_tmd::Tmd, Vec<u8>),
    /// The second backdrop copy's per-stage transform (`DAT_80078B50`).
    pub second: SecondCopy,
    /// Ground-grid depth-cue far colour, display `0..1` - the backdrop far
    /// colour per stage class (`DAT_80078C1C` outdoor table).
    pub grid_far: [f32; 3],
    /// `DAT_80078C1C` outdoor-table membership - the battle tint pass's
    /// `DAT_8007BDA8` flag (`World::battle_actor_draw_plan`) and the backdrop
    /// ramp's ceiling.
    pub outdoor: bool,
    /// How many stage TIMs the stage stream re-uploaded over its siblings.
    pub restored_tims: usize,
}

/// Build the current scene's battle-stage backdrop bundle: the stage VRAM,
/// the dome TMD, the second-copy transform and the ground grid's depth-cue
/// far colour. `None` when the scene has no stage entry for the fight's
/// region. `scus` is `SCUS_942.54`, for the two stage tables; without it the
/// second copy is the half turn and the stage indoor, the safer arms.
// REF: FUN_800513f0
pub fn resolve_battle_stage(host: &SceneHost, scus: Option<&[u8]>) -> Option<BattleStage> {
    let scene = host.scene.as_ref()?;
    // Not the block's first stage stream: a scene bundle carries one per
    // sub-area, and the region the fight starts in names which one
    // (`SceneHost::battle_stage_entry`, the region reader's stage variant
    // `_DAT_8007BD60`).
    let stage_entry = host.battle_stage_entry()?;
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|name| Scene::load(&host.index, name).ok())
        .collect();
    let refs: Vec<&Scene> = shared.iter().collect();
    let system_ui = host.index.system_ui_bundle().ok();
    let (res, _) = SceneResources::build_targeted_with_options(
        scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Battle,
            upload_all_tims: true,
            // Boot-resident system-UI bundle: resident through battle in
            // retail (uploaded once at boot, never evicted).
            system_ui: system_ui.as_deref(),
        },
    )
    .ok()?;
    // The stage dome is the leading TMD of the scene_tmd_stream stage entry.
    let dome = res.tmds.iter().find(|t| t.entry_idx == stage_entry)?;
    // Take back the VRAM this stage owns. A bundle carries one stream per
    // sub-area and they all declare the SAME pages + CLUT rows, so the
    // DMA-every-TIM build leaves whichever sibling was written last holding
    // them. Retail only ever has the recorded stream resident.
    let mut vram = res.vram.clone();
    let restored_tims = legaia_engine_scene::scene::upload_battle_stage_tims_into_vram(
        scene,
        stage_entry,
        &mut vram,
    );
    // Which transform retail gives the SECOND backdrop copy: a half turn
    // about Y, or an X mirror on the stages the `DAT_80078B50` table names.
    // `town01` IS on the mirror list, so half-turning it plants a second
    // village wall across the open sea side.
    let second = scus
        .and_then(MirrorXTable::from_scus)
        .map(|t| t.second_copy_for_prot_index(stage_entry))
        .unwrap_or(SecondCopy::HalfTurn);
    // The ground grid's depth-cue far colour per stage class
    // (`FUN_80050120`): the sibling table at `DAT_80078C1C` picks the
    // brightened outdoor arm on the wide-open stages, everything else takes
    // the indoor `>> 1` arm.
    let cue_table =
        scus.and_then(legaia_engine_battle_vm::battle_ground_grid::OutdoorCueTable::from_scus);
    let grid_far = cue_table
        .as_ref()
        .map(|t| t.far_colour_for_prot_index(stage_entry))
        .unwrap_or(legaia_engine_battle_vm::battle_ground_grid::GRID_FAR_INDOOR)
        .map(|c| f32::from(c) / 255.0);
    let outdoor = cue_table
        .as_ref()
        .is_some_and(|t| t.contains_prot_index(stage_entry));
    Some(BattleStage {
        stage_entry,
        vram,
        dome: (dome.tmd.clone(), dome.raw.clone()),
        second,
        grid_far,
        outdoor,
        restored_tims,
    })
}

/// Resolve the fight's stage and write what it leaves on the world. See the
/// module docs. Runs on the `Field -> Battle` edge, renderer or not.
///
/// - `battle.stage_outdoor`: the stage's class (indoor with no stage).
/// - `fog_volume.battle_luma`: the stage shell's luminance as drawn - over
///   the stage VRAM with the battle effect atlas blitted, through the objects
///   the backdrop registration keeps - so the volumetric fog never outshines
///   the stage it lies on. `None` with no stage.
/// - Each monster's battle form (`World::install_monster_battle_form`): its
///   texture slot and archive idle clip, for every monster whose archive
///   mesh decodes. `archive` is the monster archive
///   ([`MONSTER_ARCHIVE_PROT_INDEX`]).
pub fn stage_battle_world(
    host: &mut SceneHost,
    scus: Option<&[u8]>,
    archive: Option<&[u8]>,
) -> Option<BattleStage> {
    let stage = resolve_battle_stage(host, scus);
    host.world.battle.stage_outdoor = stage.as_ref().is_some_and(|s| s.outdoor);
    host.world.fog_volume.battle_luma = stage.as_ref().and_then(|s| {
        let (tmd, raw) = &s.dome;
        let objects = host.battle_stage_object_indices(tmd.objects.len());
        let mut vram = s.vram.clone();
        if let Err(e) =
            legaia_engine_scene::scene::upload_flame_atlas_into_vram(&host.index, &mut vram, true)
        {
            log::warn!("battle stage: flame-atlas VRAM upload skipped: {e:#}");
        }
        legaia_render_kernels::fog_volume::stage_luminance(&vram, tmd, raw, &objects)
    });
    if let Some(archive) = archive {
        for (actor_idx, monster_id, slot) in host.world.battle_monster_slots() {
            let Ok(Some(mesh)) = legaia_actor_formats::monster_archive::mesh(archive, monster_id)
            else {
                continue;
            };
            if legaia_tmd::parse(mesh.tmd_bytes()).is_err() {
                continue;
            }
            let idle = legaia_actor_formats::monster_archive::idle_animation(archive, monster_id)
                .ok()
                .flatten();
            host.world
                .install_monster_battle_form(actor_idx, slot, idle.as_ref());
        }
    }
    stage
}
