//! The scene's MAN encounter section as the battle kernel's table pair.

use anyhow::Result;
use legaia_engine_scene::scene::{ProtIndex, Scene};

/// Resolve the scene's disc-resident random-encounter table plus its
/// per-row formation defs from the same MAN asset (retail
/// `_DAT_8007B898`) the scene-entry script comes from.
///
/// Returns `Ok(None)` when the scene resolves no MAN at all (the same
/// detector gap [`Scene::field_man_entry_script`] documents) or when the
/// MAN's encounter section declares no rollable formations (towns with
/// no encounters). Resolves now for the `count=6`
/// [`legaia_scene_formats::scene_asset_table`] field scenes (town01 etc.) thanks
/// to the relaxed detector.
///
/// Wire the pair via [`legaia_engine_core::world::World::install_man_encounter`].
///
/// REF: FUN_8003AEB0 (installs the encounter section into the runtime
/// control block); the byte-level walk lives in
/// `legaia_scene_formats::man_section` and the runtime bridge in
/// [`legaia_engine_battle::encounter_man`].
pub fn field_man_encounter_table(
    scene: &Scene,
    index: &ProtIndex,
    scene_label: &str,
) -> Result<
    Option<(
        legaia_engine_battle::encounter::EncounterTable,
        Vec<legaia_engine_battle::monster_catalog::FormationDef>,
    )>,
> {
    // Same resolution order as [`Scene::field_man_payload`] (bundle MAN
    // first, streaming variant carrier fallback): the v12-family
    // dungeons (`rikuroa` / `dolk2`) carry their encounter section -
    // including the scripted-battle rows the `3E FF <row>` op selects
    // (rikuroa row 17 = lone Caruban) - in the variant MAN, their only
    // carrier.
    let Some(man_bytes) = scene.field_man_payload(index)? else {
        return Ok(None);
    };
    Ok(legaia_engine_battle::encounter_man::scene_encounter_from_man(scene_label, &man_bytes))
}
