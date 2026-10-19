//! The fishing venue over a `SceneHost`: the venue floor and the per-frame
//! tick read off the host's world and loaded scene. The venue's actors,
//! camera and floor solver are `legaia_engine_minigame_scenes::fishing_venue`;
//! this file keeps the two entry points that need the host.

use legaia_engine_minigame_scenes::fishing_venue::{VenueCameraWrites, tick_fishing_venue};

/// The venue scene's `.MAP` extended footprint - the engine's
/// `_DAT_1F8003EC` floor buffer (tile records at `+0`, height/wall grid at
/// `+0x4000`, cell grid at `+0x8000`). `None` when the current scene carries
/// no field map.
pub fn venue_floor_bytes(host: &crate::scene_host::SceneHost) -> Option<Vec<u8>> {
    // The pond's own `.MAP` (the session's venue), as retail installs the
    // fishing scene's buffer behind `_DAT_1F8003EC`; the departure field's
    // only as a fallback for a session opened with no venue.
    if let Some(v) = host
        .world
        .minigames
        .fishing
        .as_ref()
        .and_then(|s| s.venue_map())
    {
        return Some(v.map.clone());
    }
    let scene = host.scene.as_ref()?;
    let idx = scene.field_map_index(&host.index)?;
    host.index.entry_bytes_extended(idx).ok()
}

/// [`tick_fishing_venue`] over a scene host: the mode test, the held pad
/// (the world's engine mask rotated into the packed retail layout) and the
/// venue floor all come off the host, so the native window and the browser
/// play page run the identical frame and differ only in which
/// [`legaia_engine_field::camera::Camera`] they apply the returned writes to.
pub fn tick_fishing_venue_on_host(host: &mut crate::scene_host::SceneHost) -> VenueCameraWrites {
    let in_fishing = host.world.mode == legaia_engine_mode::mode::SceneMode::Fishing;
    let held_packed = host.world.input.pad().rotate_right(8);
    let floor = if in_fishing && !host.world.minigames.fishing_venue.armed() {
        venue_floor_bytes(host)
    } else {
        None
    };
    tick_fishing_venue(&mut host.world.minigames, in_fishing, held_packed, || floor)
}
