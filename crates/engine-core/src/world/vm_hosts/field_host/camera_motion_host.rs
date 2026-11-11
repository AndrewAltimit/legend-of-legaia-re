//! `FieldCameraMotionHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldCameraMotionHost for FieldHostImpl<'a> {
    // Op `4C E8` (`0x801E3378`), the crawl roller's control op
    // ([`cutscene.md`](docs/subsystems/cutscene.md)): `w3 = 0` seeds the
    // geometry words (`+0x4C/+0x4E/+0x50` of the control block, defaults
    // `0x40 / 8 / 4`); `1` pauses the live roller (`w0 == 0`, `+0x10 |=
    // 0x80000`) or writes the release line count `+0x52 = w0`; `2` resumes
    // it; `3` kills it (`+0x10 |= 8`).
    fn configure_crawl_roller(&mut self, zoom_x: i16, zoom_y: i16, zoom_z: i16, mode: i16) {
        let cut = &mut self.world.cutscene;
        match mode {
            0 => cut.narration_seed = cut.narration_seed.with_config_words(zoom_x, zoom_y, zoom_z),
            1 if zoom_x == 0 => {
                if let Some(n) = cut.narration.as_mut() {
                    n.pause();
                }
            }
            1 => {
                cut.narration_seed.release = zoom_x;
                if let Some(n) = cut.narration.as_mut() {
                    n.set_release(zoom_x);
                }
            }
            2 => {
                if let Some(n) = cut.narration.as_mut() {
                    n.resume();
                }
            }
            3 => cut.narration = None,
            _ => {}
        }
    }

    // Op `4C D7` (arm `0x801E2DB4`): kill every camera mover. The
    // script-side glide clock a `4C CD` waits on stops with it, and the
    // host camera drops its glide when it drains the event.
    fn retire_camera_movers(&mut self) {
        self.world.camera.state.glide_frames = 0;
        self.world
            .pending_field_events
            .push(FieldEvent::CameraRetireMovers);
    }

    // Op `0x4C` outer-nibble-4 sub-9 - the writer of the two globals
    // `legaia_engine_field::camera_ease` eases between, read off the three arms at
    // `0x801E1480..0x801E162C` in `overlay_world_map_801de840.txt`:
    //
    // | `_DAT_1F800394` | scene ctrl `+0x4A`     | `_DAT_8007BCAC`      |
    // |---|---|---|
    // | bit 25 (delta)  | `target`               | `target - player[+0x16]` |
    // | bit 24 (rel)    | `target + player[+0x16]` | `target`           |
    // | neither         | `target`               | untouched            |
    //
    // The first two arms both write **both** globals, and both land the
    // accumulator on the same value the per-frame easing would have walked
    // to (`ctrl[+0x4A] - player[+0x16]`); they are snaps, not a different
    // destination. The bit-24 arm has to post the accumulator itself because
    // that same bit is `FUN_801DA390`'s input lock (`0x801DA398`), so while
    // it is raised the easing returns before its first store.
    // REF: FUN_801DA390 (the easing), FUN_801D6704 (seeds the accumulator)
    fn write_camera_offset(&mut self, target: i16) {
        self.world.camera.scene_offset = target;
    }

    // The neither-bit arm with `ticks != 0` (`0x801E1608..0x801E162C`):
    // `FUN_8003C5F0(0, &ctrl[+0x4A], 2, lh ctrl[+0x4A], target, ticks)`.
    fn ramp_camera_offset(&mut self, target: i16, ticks: u16) {
        if ticks == 0 {
            self.world.camera.scene_offset = target;
            return;
        }
        self.world.ramp_camera_scene_offset(target, ticks);
    }

    // The bit-25 arm (`0x801E1498..0x801E152C`). With `ticks != 0` both stores
    // go to the ramp pool: `_DAT_8007BCAC` from its live value to
    // `target - player[+0x16]` as a word (`0x801E14D8..0x801E150C`), then
    // `+0x4A` to `target` as a halfword (`0x801E1510..0x801E152C`).
    fn write_camera_offset_delta(&mut self, target: i16, ticks: u16) {
        let footing = self.world.camera_ease_player_footing();
        let ease = i32::from(target.wrapping_sub(footing));
        if ticks == 0 {
            self.world.camera.scene_offset = target;
            self.world.camera.offset_ease = ease;
            return;
        }
        self.world.ramp_camera_offset_ease(ease, ticks);
        self.world.ramp_camera_scene_offset(target, ticks);
    }

    // The bit-24 arm (`0x801E153C..0x801E15E4`). With `ticks != 0`
    // `_DAT_8007BCAC` ramps to `target` (`0x801E1564..0x801E1588`) and `+0x4A`
    // to `target + player[+0x16]` (`0x801E15B4..0x801E1760`). Bit 24 is the
    // easing's input lock, so the ramp alone moves the accumulator:
    // `concnow_field_door_hop`'s climbs about a unit a vsync from `163` and
    // stops on `266`.
    fn write_camera_offset_player_relative(&mut self, target: i16, ticks: u16) {
        let footing = self.world.camera_ease_player_footing();
        let scene = target.wrapping_add(footing);
        if ticks == 0 {
            self.world.camera.scene_offset = scene;
            self.world.camera.offset_ease = i32::from(target);
            return;
        }
        self.world.ramp_camera_offset_ease(i32::from(target), ticks);
        self.world.ramp_camera_scene_offset(scene, ticks);
    }

    fn camera_glide_live(&mut self) -> bool {
        self.world.camera.state.glide_frames > 0
    }

    /// `[4C, 0x84, amplitude]` - the screen-shake amplitude `_DAT_8007B630`.
    ///
    /// This opcode is the global's only retail writer and the global is the
    /// only input to `FUN_801D9D30`, so the field script is the sole source
    /// of a camera shake. The world holds it for the camera to read; see
    /// [`legaia_engine_field::camera_rig::CameraRig::shake_amplitude`].
    fn set_shake_amplitude(&mut self, value: u8) {
        self.world.camera.shake_amplitude = value;
    }
}

#[cfg(test)]
mod crawl_roller_tests {
    use crate::world::World;
    use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};
    use legaia_engine_vm::field::FieldCtx;

    fn e8(w0: i16, w3: i16) -> Vec<u8> {
        let mut op = vec![0x4C, 0xE8];
        for w in [w0, 0, 0, w3] {
            op.extend_from_slice(&w.to_le_bytes());
        }
        op
    }

    fn run(w: &mut World, op: Vec<u8>) {
        let mut ctx = FieldCtx::default();
        let mut host = FieldHostImpl { world: w };
        field_step_routed(&mut host, &mut ctx, &op, 0);
    }

    /// `4C E8` with `w3` 1 / 2 / 3 pauses, writes the release count of,
    /// resumes and kills the live crawl roller (`0x801E3408..0x801E347C`).
    #[test]
    fn op_4c_e8_controls_the_live_crawl_roller() {
        let mut w = World::new();
        w.open_cutscene_narration(vec!["A".into(), "B".into()]);
        run(&mut w, e8(0, 1));
        assert!(w.cutscene.narration.as_ref().unwrap().is_paused());
        run(&mut w, e8(0, 2));
        assert!(!w.cutscene.narration.as_ref().unwrap().is_paused());
        run(&mut w, e8(5, 1));
        assert_eq!(w.cutscene.narration.as_ref().unwrap().seed().release, 5);
        run(&mut w, e8(0, 3));
        assert!(w.cutscene.narration.is_none());
    }
}
