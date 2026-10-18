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
//! The choreography is the battle's shape on the port's clock: a leg's first
//! acting play walks its attacker in from the seat ([`APPROACH_TICKS`],
//! there is no walk home), each play swings for [`PLAY_CADENCE_TICKS`] (the
//! defender flinching [`FLINCH_DELAY_TICKS`] into a connecting one, falling
//! on the knockout), and each attacker's string closes on the done band's
//! [`DONE_TAIL_TICKS`]. [`turn_timeline`] is that schedule, shared by the
//! surface, the world's playback hold and its tally readout.

mod arena_mesh;
mod assets;
mod clips;
mod surface;
mod timeline;

pub use arena_mesh::*;
pub use assets::*;
pub use clips::*;
pub use surface::*;
pub use timeline::*;

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

/// Ticks one swing of a resolved turn plays for.
pub const PLAY_CADENCE_TICKS: u32 = 34;
/// Ticks into a connecting swing at which the defender flinches.
pub const FLINCH_DELAY_TICKS: u32 = 12;
/// Ticks the leg's first acting play walks its attacker in from the seat.
/// Retail's approach is walk-clip root motion at about twenty raw units a
/// vsync (`engine-core::world::battle::locomotion`); the seats stand 1600
/// apart, so the walk closes in a little under fifty frames.
pub const APPROACH_TICKS: u32 = 48;
/// Ticks the done band holds after an attacker's string: the action SM's
/// `0x50` arm seeds the tail timer `ctx[+0x6D8] = 0x3C` and `0x51` counts it
/// down under case 8 for an attack.
pub const DONE_TAIL_TICKS: u32 = 0x3C;
/// Retail's battle world scale (base matrix `0x8007BF10 = 16384*I`): every
/// battle draw class - bodies, backdrop and ground - carries it, and the
/// camera's translation trio is authored against it.
pub const BATTLE_WORLD_SCALE: f32 = 4.0;
/// The seat body `i` stands on at leg open, `(x, z)`: the lone party seat
/// and the lone monster seat (`0x800775C8` / `0x80077608` row 0).
pub fn seat_xz(i: usize) -> [f32; 2] {
    let s = if i == 0 {
        legaia_engine_battle::battle_seats::party_seat(1, 0)
    } else {
        legaia_engine_battle::battle_seats::monster_seat(1, 0, false)
    };
    [f32::from(s.x), f32::from(s.z)]
}
/// Battle headings `+0x46`: the fighter faces `+Z`, the monster `-Z`.
pub const FACINGS: [i32; 2] = [0, 0x800];

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_actor_formats::monster_archive::MonsterAnimation;
    use legaia_engine_battle_vm::battle_cam_script as cam;
    use legaia_engine_camera::projection;
    use legaia_engine_minigames::muscle_dome::DomePlay;

    fn clip(frames: usize, rate: u32) -> DomeClip {
        DomeClip {
            frames: vec![vec![[0; 6]]; frames],
            rate,
        }
    }

    #[test]
    fn monster_clip_pick_prefers_the_close_in_attack() {
        let mk = |t: u8| MonsterAnimation {
            variant_track: Default::default(),
            action_id: t,
            rate: 1,
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            part_count: 1,
            frame_count: 1,
            frames: vec![vec![]],
            effect_script: Vec::new(),
        };
        let anims = vec![mk(0), mk(0x20), mk(3), mk(0x21), mk(4)];
        assert_eq!(pick_monster_clips(&anims), [0, 3, 2, 4]);
        let bare = vec![mk(0), mk(7)];
        assert_eq!(pick_monster_clips(&bare), [0, 1, 0, 0]);
    }

    #[test]
    fn pose_applies_the_part_translation_then_the_world_place() {
        let mut c = clip(1, 2);
        c.frames[0][0] = [10, 20, 30, 0, 0, 0];
        let base = [[1.0, 2.0, 3.0]];
        let mut out = [[0.0; 3]];
        pose_into(&mut out, &base, &[0], &c, 0, [100.0, 0.0, -5.0]);
        assert_eq!(out[0], [111.0, 22.0, 28.0]);
        // A half-turn world yaw mirrors X and Z about the body's origin.
        pose_into(
            &mut out,
            &base,
            &[0],
            &c,
            0,
            [0.0, std::f32::consts::PI, 0.0],
        );
        assert!((out[0][0] + 11.0).abs() < 1e-3 && (out[0][2] + 33.0).abs() < 1e-3);
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

    fn play(attacker: usize) -> DomePlay {
        DomePlay {
            attacker,
            cmd: 0xC,
            power: 1,
            damage: 1,
            hp_after: [10, 10],
        }
    }

    #[test]
    fn a_turn_walks_in_once_swings_each_play_and_closes_each_string() {
        let plays = [play(0), play(0), play(1)];
        let beats = turn_timeline(&plays, true);
        let kinds: Vec<_> = beats.iter().map(|b| (b.kind, b.attacker)).collect();
        use BeatKind::*;
        assert_eq!(
            kinds,
            [
                (Approach, 0),
                (Swing, 0),
                (Swing, 0),
                (Done, 0),
                (Swing, 1),
                (Done, 1)
            ]
        );
        let total = APPROACH_TICKS + 3 * PLAY_CADENCE_TICKS + 2 * DONE_TAIL_TICKS;
        assert_eq!(turn_playback_ticks(&plays, true), total);
        assert_eq!(
            turn_playback_ticks(&plays, false),
            total - APPROACH_TICKS,
            "a later turn swings from where the pair stands"
        );
        assert_eq!(turn_playback_ticks(&[], true), 0);
        // Beats tile the span with no gap.
        for w in beats.windows(2) {
            assert_eq!(w[0].start + w[0].len, w[1].start);
        }
        assert_eq!(beat_at(&plays, true, 0).map(|b| b.kind), Some(Approach));
        assert_eq!(beat_at(&plays, true, total), None);
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

    #[test]
    fn a_swing_plays_out_then_idles() {
        let clips = BodyClips {
            idle: clip(4, 2),
            walk: clip(5, 2),
            hit: clip(3, 2),
            ko: clip(3, 2),
            swings: [Some(clip(2, 16)), None, None, None],
        };
        assert_eq!(clips.swing(0xD).frame_count(), 2, "missing swing -> slot 0");
        assert_eq!(clip_for(&clips, ClipKind::Ko).frame_count(), 3);
    }
}
