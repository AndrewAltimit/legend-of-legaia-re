//! The sparring tutorial's disc reader.
//!
//! The prompt machine and its corpus parser live in
//! [`legaia_engine_battle::battle_tutorial`]. This module holds what reads the
//! user's `PROT.DAT` through a [`legaia_engine_scene::scene::ProtIndex`].

use legaia_engine_battle::battle_tutorial::BattleTutorialScript;

/// Read the prompt corpus straight off the booted disc.
///
/// Resolves the `battle_tutorial` row of the static-overlay map (extraction
/// PROT [`legaia_engine_battle::battle_tutorial::OVERLAY_967_PROT_INDEX`],
/// base [`legaia_engine_battle::battle_tutorial::OVERLAY_967_BASE_VA`]), reads
/// that entry through the host's PROT index, slices it to its as-loaded
/// form and parses the strings. This is what a host calls to prime
/// [`legaia_engine_core::world::World::prime_battle_tutorial`] with real text; the bytes
/// never leave the user's own disc.
///
/// Returns an empty script (not an error) when the entry can't be read, so
/// a host booted against a partial disc still runs the battle - it just
/// shows no boxes.
pub fn script_from_prot(index: &legaia_engine_scene::scene::ProtIndex) -> BattleTutorialScript {
    let Some(rec) =
        legaia_overlay_images::static_overlay::overlay_map().by_label("battle_tutorial")
    else {
        return BattleTutorialScript::default();
    };
    let Ok(bytes) = index.entry_bytes(rec.prot_index) else {
        return BattleTutorialScript::default();
    };
    match legaia_overlay_images::static_overlay::as_loaded(&bytes, rec) {
        Ok(loaded) => BattleTutorialScript::from_overlay(&loaded, rec.base_va),
        Err(_) => BattleTutorialScript::default(),
    }
}
