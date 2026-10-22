//! The visible-tile crop's **policy over a [`World`]**.
//!
//! The crop kernel - the cell rectangle one frame of `FUN_801F7088` walks and
//! its per-cell gates - is [`legaia_engine_scene::field_view_window`], which
//! names no `World`. This module holds what reads one: whether a host crops
//! this frame at all, the placed-actor and field-NPC gates that run the
//! world's own cull, and whether the frame's camera stands where retail's
//! would ([`retail_vantage`]).

use legaia_engine_core::world::World;
use legaia_engine_field::camera::Camera;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::field_view_window::{ViewCells, framing_is_retail, view_cells};

/// Whether this frame's camera stands where retail's would: the host's own
/// debug vantage (`F3`, `debug_camera`) is off, and the frame is a battle
/// frame (the phase camera), a scripted shot
/// ([`crate::camera_view::cutscene_owns_camera`] - the cutscene
/// arm reads none of the user's follow knobs), or the follow camera at
/// retail framing ([`framing_is_retail`]: the `Retail` distance preset with
/// the drag / tilt / zoom knobs at identity).
///
/// Both play hosts gate retail's **placed-object near reject** on it
/// (`field_env::placed_origin_near_culled`). That rule drops a whole placed
/// object by its *origin's* view depth, which is only a fair proxy for the
/// object's extent inside retail's own frustum. A follow camera swung,
/// tilted toward the horizon or wheeled in close puts a large ground slab's
/// origin beside or behind the lens while most of the slab is in shot, and
/// the reject then removed it whole - `town01`'s floor chunks vanishing as
/// the camera swung. Off retail's vantage the object draws and the
/// per-primitive near reject trims whatever of it sits at the lens.
pub fn retail_vantage(world: &World, camera: &Camera, debug_camera: bool) -> bool {
    !debug_camera
        && (world.mode == SceneMode::Battle
            || crate::camera_view::cutscene_owns_camera(world)
            || framing_is_retail(camera))
}

/// This frame's cell rectangle, or `None` when the host should draw the whole
/// map - the one entry point both play hosts ask.
///
/// `None` when:
///
/// - the crop knob is off ([`legaia_engine_core::world::WorldToggles::view_window_crop`],
///   from [`legaia_engine_menus::options::OptionsState::retail_view_window`]);
/// - `retail_framing` is false - the host passes [`framing_is_retail`] and
///   clears it under its own debug camera. The window is authored for
///   retail's frustum, so under a wider or re-aimed view it would open black
///   edges the player can see; the crop is the faithful mode's, not a
///   constraint on the enhanced views;
/// - the world is not in [`SceneMode::Field`]: the kingdom overworld is drawn
///   by the PROT 0901 library, not by the pair this module ports, and the
///   other modes draw no `.MAP` cells;
/// - no camera view is published yet ([`legaia_engine_core::world::FieldNpcState::cull_view`]
///   is `None` until the zone follow camera composes);
/// - the focus tile lies outside the region box. Retail latches the box at
///   the camera's own re-centre tile (`FUN_80017DD4`), so the two agree by
///   construction; the port latches it from the player actor's tile, and a
///   scene entered without a seat can leave the follow focus somewhere the
///   actor is not. Cropping then would clamp the whole window away and draw
///   no ground at all, so the host draws the map whole instead.
///
/// A cutscene shot crops too, as retail's does: the cell emitters and the
/// actor cull read the same scratchpad window and camera globals whatever owns
/// the camera, and a record that stages a shot sets the window for it with
/// op `0x46` (`town0c` `P1[21]`'s `46 24 FD EC 18 03` before the Queen Bee
/// shot; `rim_elm_queen_bee_battle` holds that window, and the cull kernel
/// over it reproduces every placed actor's bit `1` in the capture).
pub fn field_view_cells(world: &World, retail_framing: bool) -> Option<ViewCells> {
    if !world.toggles.view_window_crop || !retail_framing || world.mode != SceneMode::Field {
        return None;
    }
    let view = world.npcs.cull_view.as_ref()?;
    let [r0, r1, r2, r3] = view.attr_box.map(i32::from);
    let (tx, tz) = (
        view.focus_stored[0].wrapping_neg() >> 7,
        view.focus_stored[1].wrapping_neg() >> 7,
    );
    if tx < r0 || tx >= r2 || tz < r1 || tz >= r3 {
        return None;
    }
    Some(view_cells(view))
}

/// The placed-object gate a host applies per draw while the visible-tile
/// crop is on (`cells` is this frame's [`field_view_cells`]): `true` = draw
/// it. `(x, z)` is the object's live position (the draw's translation) and
/// `cull_radius` its record's `+0x1E` byte.
///
/// Every `.MAP` placed object is a field actor, and its tick `FUN_8003BC08`
/// runs the visibility cull `FUN_801D79E8` first (`jal` at `0x8003BC34`),
/// which sets `+0x10` bit 1 on an actor outside the region box or outside the
/// visible tile window widened by its `+0x58` radius. The actor draw walk
/// `FUN_8001ADA4` skips any actor carrying `flags & 0xA`
/// (`0x8001AE4C..0x8001AE58`), so a culled object draws nothing. Over the
/// 84 placed-object actors of each `rikuroa` capture
/// ([`legaia_engine_core::world::field_npc_cull::field_actor_culled`] fed the state's
/// focus, box and window) the kernel reproduces bit 1 for every one -
/// including the ring of moss spires on the slopes below the summit, which
/// retail never draws from it.
///
/// REF: FUN_801D79E8, FUN_8001ADA4
pub fn placed_actor_visible(
    world: &World,
    cells: Option<&ViewCells>,
    x: i32,
    z: i32,
    cull_radius: u8,
) -> bool {
    cells.is_none()
        || !world.field_actor_culled_at(
            x.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            z.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            i16::from(cull_radius),
        )
}

/// The field-NPC gate a host applies per draw while the visible-tile crop is
/// on: `true` = draw placement `slot` (spawned at `spawn`). The same cull as
/// [`placed_actor_visible`], at the NPC's live position and the placement
/// radius `+0x58 = 0` ([`World::field_npc_culled`]).
///
/// A placement NPC is a field actor ticked by `FUN_8003BC08`, so the
/// visibility cull `FUN_801D79E8` rewrites its `+0x10` bit 1 every tick and
/// the actor draw walk `FUN_8001ADA4` skips it while the bit is up
/// (`flags & 0xA`, `0x8001AE4C..0x8001AE58`). In the `conc_field_card_boot`
/// capture the placements that read bit 1 set (`+0x10 = 0x08020886`)
/// include two standing past the window's far edge, which retail's frame
/// does not show and which the engine drew at the top of the screen.
///
/// REF: FUN_801D79E8, FUN_8001ADA4
pub fn npc_actor_visible(
    world: &World,
    cells: Option<&ViewCells>,
    slot: u8,
    spawn: (i16, i16),
) -> bool {
    if cells.is_none() {
        return true;
    }
    let (x, z) = world.npcs.positions.get(&slot).copied().unwrap_or(spawn);
    !world.field_npc_culled(x, z)
}
#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::camera::FieldCullView;

    /// A town01-shaped view from the retail actor-cull capture: focus tile
    /// `(32, 92)`, region `[20, 77, 44, 108]`, window `(-8, -6, 8, 12)`.
    fn town01() -> FieldCullView {
        FieldCullView {
            focus_stored: [-4160, -11840],
            attr_box: [20, 77, 44, 108],
            window: [-8, -6, 8, 12],
        }
    }

    /// The follow camera's knobs, a wider distance preset and the debug
    /// vantage take the frame off retail's vantage; battle keeps it whatever
    /// the field knobs hold.
    #[test]
    fn retail_vantage_is_the_retail_follow_framing_or_a_battle_frame() {
        use legaia_engine_camera::camera_distance::CameraDistance;
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let cam = Camera::default();
        assert!(retail_vantage(&world, &cam, false));
        assert!(!retail_vantage(&world, &cam, true), "F3 debug vantage");
        let knobs: [fn(&mut Camera); 4] = [
            |c| c.manual_orbit = 0.8,
            |c| c.manual_tilt = -0.4,
            |c| c.manual_zoom = 0.5,
            |c| c.distance = CameraDistance::Far,
        ];
        for knob in knobs {
            let mut c = Camera::default();
            knob(&mut c);
            assert!(!retail_vantage(&world, &c, false));
            world.mode = SceneMode::Battle;
            assert!(retail_vantage(&world, &c, false), "the phase camera");
            world.mode = SceneMode::Field;
        }
    }

    #[test]
    fn the_policy_needs_a_view_the_knob_and_retail_framing() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        assert!(field_view_cells(&world, true).is_none(), "no view yet");
        world.npcs.cull_view = Some(town01());
        assert_eq!(field_view_cells(&world, true), Some(view_cells(&town01())));
        assert!(field_view_cells(&world, false).is_none());
        world.mode = SceneMode::WorldMap;
        assert!(field_view_cells(&world, true).is_none());
        world.mode = SceneMode::Field;
        // A focus the latched region box does not hold: no crop.
        world.npcs.cull_view = Some(FieldCullView {
            focus_stored: [-(20 * 128), -(20 * 128)],
            ..town01()
        });
        assert!(field_view_cells(&world, true).is_none());
        world.npcs.cull_view = Some(town01());
        world.toggles.view_window_crop = false;
        assert!(field_view_cells(&world, true).is_none());
    }
}
