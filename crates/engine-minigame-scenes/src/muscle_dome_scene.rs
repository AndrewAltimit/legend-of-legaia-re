//! The Muscle Dome's **3D arena surface**: the arena shell, the retail
//! battle ground grid, the lead's assembled battle form and the ladder's
//! monster, posed from their own clip banks and framed by one camera - the
//! one kernel every dome host draws, the way
//! [`crate::baka_duel_scene::BakaDuelSurface`] serves the duel.
//!
//! What goes into the frame:
//!
//! - The **fighter** is the character's assembled battle form (the
//!   `battle_char_assembly` chain the native battles and the arts viewer
//!   use): equipment-id sections spliced at their section defaults (the dome
//!   forbids equipment), TSB/CBA relocated to runtime band 0, posed from the
//!   player battle file's own record-0 action streams and per-command swing
//!   records (slots `0xC..=0xF` are the card ids themselves).
//! - The **opponent** is the monster archive's mesh (PROT 867), relocated to
//!   battle texture slot 0 exactly as the battle loader does
//!   (`battle_render_mesh`), posed from its action table by tag.
//! - The **arena shell** is the tail `scene_tmd_stream` of the `other6`
//!   block (PROT 1225, see `docs/subsystems/minigame-muscle-dome.md`), less
//!   its wall-base dust-decal object; its two TIM pages land at their own
//!   framebuffer addresses.
//! - The **ground** is the retail battle ground grid, sampling the arena's
//!   `(832, 0)` page.
//!
//! A dome leg is an ordinary battle (`FUN_801D1510` hands the round to
//! battle init through game mode `0x14`, and the round driver
//! `FUN_801D0748` is the battle overlay's), so the two bodies stand on the
//! battle formation seats - the lone party seat `(0, -800)` and the lone
//! monster seat `(0, 800)` (`legaia_engine_battle::battle_seats`) - at the battle world
//! scale, and the frame is filmed by the **battle camera script**
//! (`legaia_engine_battle_vm::battle_cam_script`, `FUN_801D5854`), the same one an
//! ordinary fight runs: the far framing with its idle orbit on the prompts,
//! the case-0 over-the-shoulder close-up on the fighter's command surfaces,
//! case 1 on the attack cursor, case 6 on the approach, case 7 on the
//! strikes, case 8 on the done tail and the battle-over arm on a win.
//!
//! The bodies move on the session's play-out choreography
//! (`legaia_engine_battle_menus::muscle_dome_session::choreo`): the battle's clip
//! rate, the range-law approach, root motion, the end steps and the
//! reaction pick, stepped once a world tick by the host that owns the
//! session. The surface reads the poses, positions and facings off it and
//! films them.

mod arena_mesh;
mod assets;
mod clips;
mod surface;

pub use arena_mesh::*;
pub use assets::*;
pub use clips::*;
pub use surface::*;

/// The monster archive (PROT 867).
pub const MONSTER_ARCHIVE_PROT_INDEX: usize = 867;
/// The dome arena overlay (PROT 0977) - the course ladder's carrier.
pub const ARENA_OVERLAY_PROT_INDEX: usize =
    legaia_engine_minigames::muscle_dome::ARENA_OVERLAY_PROT_INDEX;
/// The arena backdrop stream (PROT 1225).
pub const ARENA_BACKDROP_PROT_INDEX: usize = 1225;
/// The battle-action overlay (PROT 0898): the per-character camera height
/// table `0x801F4D2C` the close-ups read.
pub const BATTLE_ACTION_PROT_INDEX: usize = 898;

/// Retail's battle world scale (base matrix `0x8007BF10 = 16384*I`): every
/// battle draw class - bodies, backdrop and ground - carries it, and the
/// camera's translation trio is authored against it.
pub const BATTLE_WORLD_SCALE: f32 = 4.0;

/// The seat body `i` stands on at leg open, `(x, z)`: the lone party seat
/// and the lone monster seat
/// (`legaia_engine_battle_menus::muscle_dome_session::choreo::seat_xz`).
pub fn seat_xz(i: usize) -> [f32; 2] {
    let [x, z] = legaia_engine_battle_menus::muscle_dome_session::choreo::seat_xz(i);
    [x as f32, z as f32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_battle_vm::battle_cam_script as cam;
    use legaia_engine_camera::projection;

    fn clip(frames: usize, rate: u32) -> DomeClip {
        DomeClip {
            frames: vec![vec![[0; 6]]; frames],
            rate,
        }
    }

    #[test]
    fn pose_applies_the_part_translation_then_the_world_place() {
        let mut c = clip(1, 2);
        c.frames[0][0] = [10, 20, 30, 0, 0, 0];
        let base = [[1.0, 2.0, 3.0]];
        let mut out = [[0.0; 3]];
        pose_into(&mut out, &base, &[0], &c.frames[0], [100.0, 0.0, -5.0]);
        assert_eq!(out[0], [111.0, 22.0, 28.0]);
        // A half-turn world yaw mirrors X and Z about the body's origin.
        pose_into(
            &mut out,
            &base,
            &[0],
            &c.frames[0],
            [0.0, std::f32::consts::PI, 0.0],
        );
        assert!((out[0][0] + 11.0).abs() < 1e-3 && (out[0][2] + 33.0).abs() < 1e-3);
    }

    #[test]
    fn the_idle_loop_advances_at_the_battle_rate() {
        // `FUN_80047430`: `rate / 4` keyframes a vsync at the normal rate.
        let c = clip(8, 2);
        assert_eq!(c.frame_at(0), 0);
        assert_eq!(c.frame_at(2), 1);
        assert_eq!(c.frame_at(16), 0, "eight keyframes loop in sixteen ticks");
    }

    #[test]
    fn the_camera_looks_at_its_focus() {
        let cam = DomeCamera {
            pose: projection::BattleCamPose {
                pitch: 32.0,
                yaw: 700.0,
                tr: [0.0, 0.0, 5000.0],
                focus: [260.0, 0.0, -300.0],
            },
        };
        let m = cam.vp_raw(4.0 / 3.0);
        let p = [260.0f32, 0.0, -300.0, 1.0];
        let mut clip = [0.0f32; 4];
        for (r, o) in clip.iter_mut().enumerate() {
            *o = (0..4).map(|c| m[c * 4 + r] * p[c]).sum();
        }
        assert!(clip[3] > 0.0);
        assert!(
            (clip[0] / clip[3]).abs() < 1e-4,
            "the focus is centred in X"
        );
        let z = clip[2] / clip[3];
        assert!((0.0..=1.0).contains(&z), "depth {z} in [0, 1]");
    }

    #[test]
    fn the_dome_camera_projects_the_seats_through_the_battle_frame() {
        // The far framing over the two seats: both on screen, apart in X.
        let mut formation = None;
        for i in 0..2 {
            let [x, z] = seat_xz(i);
            cam::FormationBox::extend(&mut formation, x, z);
        }
        let pose = cam::menu_framing(formation, 1024.0);
        let m = DomeCamera { pose }.vp_raw(4.0 / 3.0);
        let ndc = |p: [f32; 3]| {
            let c: Vec<f32> = (0..4)
                .map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
                .collect();
            assert!(c[3] > 0.0, "in front of the eye");
            [c[0] / c[3], c[1] / c[3], c[2] / c[3]]
        };
        let [fx, fz] = seat_xz(0);
        let [mx, mz] = seat_xz(1);
        let f = ndc([fx, -200.0, fz]);
        let g = ndc([mx, -200.0, mz]);
        for p in [f, g] {
            assert!(p[0].abs() < 1.0 && p[1].abs() < 1.0, "on screen: {p:?}");
            assert!((0.0..=1.0).contains(&p[2]), "depth in [0, 1]: {p:?}");
        }
        assert!((f[0] - g[0]).abs() > 0.1, "apart: {f:?} {g:?}");
    }
}
