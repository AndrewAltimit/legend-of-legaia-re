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
use legaia_engine_scene::field_view_window::{
    ViewCells, framing_is_retail, preset_only_scale, view_cells, widened_window,
};

/// How this frame's camera stands against retail's, for the visible-tile
/// crop ([`field_view_cells`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CropFraming {
    /// Retail's own frustum ([`retail_framing`]): the crop walks retail's
    /// window.
    Retail,
    /// Retail's angles from further back - the `Far` / `Farther` distance
    /// preset with the drag / tilt / zoom knobs at identity. The crop walks
    /// the window grown by this scale (`widened_window`).
    Widened(f32),
    /// A view retail's window says nothing about (a swung, tilted or
    /// wheeled follow camera, a host's debug vantage): the map draws whole.
    Free,
}

/// This frame's [`CropFraming`]. `debug_camera` is the host's own debug
/// vantage (`F3`), which frees the view.
///
/// The widened arm exists because lifting the crop whole under the default
/// `Far` preset drew what retail's window is there to leave out. In
/// `rikuroa`'s cave a stalactite hanging fifteen rows nearer the lens than
/// the player, past the window's near edge, covered the frame through the
/// first rockfall, and the mountain's sky shell, a placed object of another
/// walk region, showed as a grey sheet through the cave's open floor.
pub fn crop_framing(world: &World, camera: &Camera, debug_camera: bool) -> CropFraming {
    if debug_camera {
        CropFraming::Free
    } else if retail_framing(world, camera) {
        CropFraming::Retail
    } else if let Some(scale) = preset_only_scale(camera) {
        CropFraming::Widened(scale)
    } else {
        CropFraming::Free
    }
}

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
    !debug_camera && (world.mode == SceneMode::Battle || retail_framing(world, camera))
}

/// Whether this frame frames the field the way retail's own view does, for
/// the visible-tile crop and the vantage gates: the follow camera at retail
/// framing ([`framing_is_retail`]), or a scripted shot, which reads none of
/// the user's follow knobs - the modal timeline's
/// ([`crate::camera_view::cutscene_owns_camera`]) or a player-holding helper
/// record's (`World::scripted_shot_holds_camera`, the ending vignettes).
/// [`crop_framing`] turns it into the crop's [`CropFraming::Retail`] arm.
pub fn retail_framing(world: &World, camera: &Camera) -> bool {
    framing_is_retail(camera)
        || crate::camera_view::cutscene_owns_camera(world)
        || world.scripted_shot_holds_camera()
}

/// The field ground's depth rule for this frame, the word both play hosts
/// stage for their ground shaders (`Renderer::set_field_ground_drop`, the
/// play page's `setGroundDrop`): `None` keeps retail's ordering-table
/// buckets, `Some(drop)` depth-tests each bucketed cell as the level plane
/// `drop` units beyond it
/// ([`legaia_engine_scene::field_ground::FREE_VANTAGE_FLOOR_DROP`]).
///
/// The buckets hold at retail's vantage ([`retail_vantage`]) and in every
/// mode but the field, whose ground mesh is the only one that carries them.
/// Off that vantage - the default `Far` preset, the drag / tilt / zoom
/// knobs, `F3` - the camera sees under-floor faces retail's camera never
/// frames, which the buckets would paint over the ground.
pub fn field_ground_drop(world: &World, camera: &Camera, debug_camera: bool) -> Option<f32> {
    (world.mode == SceneMode::Field && !retail_vantage(world, camera, debug_camera))
        .then_some(legaia_engine_scene::field_ground::FREE_VANTAGE_FLOOR_DROP)
}

/// This frame's cell rectangle, or `None` when the host should draw the whole
/// map - the one entry point both play hosts ask.
///
/// `None` when:
///
/// - the crop knob is off ([`legaia_engine_core::world::WorldToggles::view_window_crop`],
///   from [`legaia_engine_menus::options::OptionsState::retail_view_window`]);
/// - `framing` is [`CropFraming::Free`] (the host passes [`crop_framing`]).
///   The window is authored for retail's frustum, so under a re-aimed view
///   it would open black edges the player can see. A pulled-back preset
///   alone keeps retail's angles and crops by the window grown to its scale
///   ([`CropFraming::Widened`]);
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
pub fn field_view_cells(world: &World, framing: CropFraming) -> Option<ViewCells> {
    let scale = match framing {
        CropFraming::Retail => 1.0,
        CropFraming::Widened(scale) => scale,
        CropFraming::Free => return None,
    };
    if !world.toggles.view_window_crop || world.mode != SceneMode::Field {
        return None;
    }
    // An expansion-pack cutscene's shot is not retail's frustum: the window
    // is authored around the follow focus, and a shot moved off it would
    // open black edges, so the map draws whole while one moves the camera.
    if world.expansion_cutscene_camera().is_some() {
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
    Some(view_cells(&legaia_engine_field::camera::FieldCullView {
        window: widened_window(view.window, scale),
        ..*view
    }))
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
    cells.is_none_or(|c| {
        !world.field_actor_culled_in_window(
            x.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            z.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            i16::from(cull_radius),
            c.window_in,
        )
    })
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
    let Some(cells) = cells else {
        return true;
    };
    let (x, z) = world.npcs.positions.get(&slot).copied().unwrap_or(spawn);
    // Placement radius: `+0x58` read 0 on every captured placement.
    !world.field_actor_culled_in_window(x, z, 0, cells.window_in)
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

    /// The ground keeps retail's buckets at retail's vantage and off the
    /// field, and takes the dropped floor on any other field frame.
    #[test]
    fn the_ground_drops_only_off_retail_vantage_on_the_field() {
        use legaia_engine_camera::camera_distance::CameraDistance;
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let retail = Camera::default();
        assert_eq!(field_ground_drop(&world, &retail, false), None);
        let drop = Some(legaia_engine_scene::field_ground::FREE_VANTAGE_FLOOR_DROP);
        assert_eq!(field_ground_drop(&world, &retail, true), drop, "F3");
        let mut far = Camera::default();
        far.distance = CameraDistance::Far;
        assert_eq!(field_ground_drop(&world, &far, false), drop);
        world.mode = SceneMode::Battle;
        assert_eq!(field_ground_drop(&world, &far, false), None);
        world.mode = SceneMode::WorldMap;
        assert_eq!(field_ground_drop(&world, &far, false), None);
    }

    #[test]
    fn the_policy_needs_a_view_the_knob_and_retail_framing() {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let retail = CropFraming::Retail;
        assert!(field_view_cells(&world, retail).is_none(), "no view yet");
        world.npcs.cull_view = Some(town01());
        assert_eq!(
            field_view_cells(&world, retail),
            Some(view_cells(&town01()))
        );
        assert!(field_view_cells(&world, CropFraming::Free).is_none());
        world.mode = SceneMode::WorldMap;
        assert!(field_view_cells(&world, retail).is_none());
        world.mode = SceneMode::Field;
        // A focus the latched region box does not hold: no crop.
        world.npcs.cull_view = Some(FieldCullView {
            focus_stored: [-(20 * 128), -(20 * 128)],
            ..town01()
        });
        assert!(field_view_cells(&world, retail).is_none());
        world.npcs.cull_view = Some(town01());
        world.toggles.view_window_crop = false;
        assert!(field_view_cells(&world, retail).is_none());
    }

    /// A pulled-back preset with the knobs at identity keeps the crop,
    /// widened: an actor the retail window drops at its edge draws, one far
    /// past the grown window or outside the region box does not.
    #[test]
    fn a_distance_preset_alone_crops_by_the_widened_window() {
        use legaia_engine_camera::camera_distance::CameraDistance;
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.npcs.cull_view = Some(town01());
        let mut cam = Camera::new();
        cam.distance = CameraDistance::Retail;
        assert_eq!(crop_framing(&world, &cam, false), CropFraming::Retail);
        assert_eq!(crop_framing(&world, &cam, true), CropFraming::Free);
        cam.distance = CameraDistance::Far;
        let framing = crop_framing(&world, &cam, false);
        assert_eq!(framing, CropFraming::Widened(CameraDistance::Far.scale()));
        cam.manual_orbit = 0.4;
        assert_eq!(crop_framing(&world, &cam, false), CropFraming::Free);

        let retail = field_view_cells(&world, CropFraming::Retail).unwrap();
        let wide = field_view_cells(&world, framing).unwrap();
        assert_eq!(wide.window_in, [-12, -10, 10, 15]);
        // Focus tile (32, 92). Nine columns left: outside retail's `-8`,
        // inside the widened `-12`.
        let (x, z) = ((32 - 9) * 128 + 64, 92 * 128 + 64);
        assert!(!placed_actor_visible(&world, Some(&retail), x, z, 0));
        assert!(placed_actor_visible(&world, Some(&wide), x, z, 0));
        // Fourteen rows toward the lens: outside both.
        let near = (92 - 14) * 128 + 64;
        assert!(!placed_actor_visible(
            &world,
            Some(&wide),
            32 * 128,
            near,
            0
        ));
        // Outside the region box (`x < 20`): never drawn while a crop holds.
        assert!(!placed_actor_visible(&world, Some(&wide), 10 * 128, z, 100));
        // No crop: everything draws.
        assert!(placed_actor_visible(&world, None, 10 * 128, z, 0));
    }
}
