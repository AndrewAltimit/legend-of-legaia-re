//! The arena surface a host draws: the camera, playback state and the per-frame pose / camera drive.

use super::*;
use legaia_engine_battle_menus::muscle_dome_session::choreo::{Beat, BeatKind};
use legaia_engine_battle_menus::muscle_dome_session::{MuscleDomeSession, MusclePhase};
use legaia_engine_battle_vm::battle_cam_script as cam;
use legaia_engine_camera::projection;
use legaia_engine_minigames::muscle_dome::DomeContest;
use std::sync::Arc;

/// The dome's camera: one pose of the battle camera script, projected the
/// way the battle hosts project a stage draw - [`projection::battle_vp`] at the
/// battle world scale over the stage model `diag(4, -4, 4)` - so both hosts
/// take the one matrix for raw Y-down world vertices, depth `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DomeCamera {
    pub pose: projection::BattleCamPose,
}

impl DomeCamera {
    /// The column-major view-projection a host multiplies a raw (Y-down)
    /// world vertex by. Both hosts upload exactly this.
    pub fn vp_raw(&self, aspect: f32) -> [f32; 16] {
        let k = BATTLE_WORLD_SCALE;
        let stage: [f32; 16] = [
            k, 0.0, 0.0, 0.0, //
            0.0, -k, 0.0, 0.0, //
            0.0, 0.0, k, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        projection::mat_mul(&projection::battle_vp(&self.pose, k, aspect), &stage)
    }
}

/// The posed dome for one frame.
#[derive(Debug, Clone)]
pub struct MuscleDomeScene {
    /// Posed positions, raw retail world coordinates (Y down): the fighter,
    /// then the monster, then the statics.
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[u8; 2]>,
    pub cba_tsb: Vec<[u16; 2]>,
    /// Per-vertex packet colour (the textured modulation / untextured fill).
    pub colors: Vec<[u8; 3]>,
    /// Per-vertex `[r, g, b, textured]` - the browser renderer's stream.
    pub flat_rgba: Vec<u8>,
    /// Every triangle.
    pub indices: Vec<u32>,
    /// The triangles whose first vertex is textured / untextured - the
    /// native window's two pipelines.
    pub textured_indices: Vec<u32>,
    pub untextured_indices: Vec<u32>,
    /// The camera this frame draws under.
    pub camera: DomeCamera,
    /// Vertex offset of each body: `[fighter, monster, statics]`.
    pub bases: [usize; 3],
}

/// The per-host cache every dome host drives once a frame, as
/// [`crate::baka_duel_scene::BakaDuelSurface`] is for the duel.
#[derive(Debug, Default)]
pub struct MuscleDomeSurface {
    assets: Option<Arc<MuscleDomeAssets>>,
    /// A `(monster, character)` pair that failed to decode, so a missing
    /// asset is not re-decoded every frame.
    failed: Option<(u16, u32)>,
    ladder: Option<Option<Vec<legaia_engine_minigames::muscle_dome::DomeCourse>>>,
    scene: Option<MuscleDomeScene>,
    generation: u32,
    tick: u32,
    /// The session turn last seen, so a fresh leg re-creates the camera.
    last_turn: Option<u32>,
    /// The beat the last frame filmed, for [`Self::playback_beat`].
    beat: Option<(Beat, u32)>,
    /// The battle camera script's state for this leg.
    camera: Option<cam::BattleCamera>,
    /// The Battle Camera option word `0x800846C0`
    /// ([`Self::set_camera_option`]).
    camera_option: u8,
    /// A host-named selection framing ([`Self::set_select_framing`]).
    select_framing: Option<SelectFraming>,
    /// The world sim tick [`Self::frame_at`] last stepped on.
    last_sim_tick: Option<u32>,
}

/// The selection-screen framing a host names for the dome camera, in the
/// battle's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectFraming {
    /// The round prompt and the `Begin | Reselect` confirm: case 9, the far
    /// framing with its idle orbit.
    Far,
    /// The ring, the direction entry and its review, the Ra-Seru list:
    /// case 0, the fighter's over-the-shoulder close-up.
    Member,
    /// The `Auto | Command` prompt and its cursor: case 1, turned toward
    /// the opponent.
    Target,
}

impl MuscleDomeSurface {
    /// The monster the contest's current rung seats: the course ladder's
    /// `(course, round)` row, the round clamped to the course's last.
    fn seated_monster(
        &mut self,
        read_prot: &impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        contest: Option<&DomeContest>,
    ) -> Option<u16> {
        let ladder = self.ladder.get_or_insert_with(|| {
            read_prot(ARENA_OVERLAY_PROT_INDEX)
                .and_then(|raw| legaia_engine_minigames::muscle_dome::parse_course_ladder(&raw))
        });
        let (course, round) = contest.map_or((0, 0), |c| (c.course(), c.round()));
        let rounds = &ladder.as_ref()?.get(course)?.rounds;
        let n = (round as usize).min(rounds.len().checked_sub(1)?);
        Some(rounds.get(n)?.monster_id as u16)
    }

    /// One frame. `None` - and the posed buffers dropped - when no dome
    /// session is live or its scene does not decode. The assets load once
    /// per seated `(monster, character)` pair through `read_prot`, which
    /// bumps [`Self::generation`]; the choreography steps and the pose runs
    /// every call.
    pub fn frame(
        &mut self,
        read_prot: impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        session: Option<&MuscleDomeSession>,
        contest: Option<&DomeContest>,
        char_slot: u32,
    ) -> Option<&MuscleDomeScene> {
        let Some(session) = session else {
            self.reset();
            return None;
        };
        let monster = self.seated_monster(&read_prot, contest)?;
        self.frame_seated(read_prot, session, monster, char_slot)
    }

    /// [`Self::frame`] gated on the world's sim tick
    /// (`legaia_engine_vm::frame_clock::FrameClock::sim_ticks`): the choreography, the pose
    /// and the battle camera script step once per tick the world ran, and a
    /// display frame that ran no tick re-draws the last pose. [`Self::frame`]
    /// steps once per *call*, so a host calling it once per display frame
    /// ran the dome's camera and fighters at the monitor's rate - and the
    /// browser page, which re-draws a paused or idle frame, ran its dome a
    /// different number of steps than the window over the same ticks (the
    /// round intro framed the fighter's back on one host and the monster's
    /// on the other). Both world hosts call this one.
    pub fn frame_at(
        &mut self,
        read_prot: impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        session: Option<&MuscleDomeSession>,
        contest: Option<&DomeContest>,
        char_slot: u32,
        sim_tick: u32,
    ) -> Option<&MuscleDomeScene> {
        if session.is_some() && self.scene.is_some() && self.last_sim_tick == Some(sim_tick) {
            return self.scene.as_ref();
        }
        self.last_sim_tick = session.map(|_| sim_tick);
        self.frame(read_prot, session, contest, char_slot)
    }

    /// [`Self::frame`] for an opponent the host names itself rather than the
    /// contest's ladder rung - the minigames page's foe picker.
    pub fn frame_seated(
        &mut self,
        read_prot: impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        session: &MuscleDomeSession,
        monster: u16,
        char_slot: u32,
    ) -> Option<&MuscleDomeScene> {
        let key = (monster, char_slot);
        if self.assets.as_ref().map(|a| a.key()) != Some(key) {
            if self.failed == Some(key) {
                return None;
            }
            match MuscleDomeAssets::load(&read_prot, monster, char_slot) {
                Some(a) => {
                    self.assets = Some(Arc::new(a));
                    self.failed = None;
                    self.generation = self.generation.wrapping_add(1);
                    // The posed buffers are built once per seat (`pose`'s
                    // `get_or_insert_with`), so a new pair must drop the old
                    // seat's: kept, the next opponent was posed into the last
                    // one's indices / UVs / CBA and body offsets - the round-2
                    // corrupt opponent on a host that re-seats without a
                    // session-less frame between legs (the minigames page).
                    // The idle clips restart with the seat, as a fresh load's.
                    self.scene = None;
                    self.tick = 0;
                    self.reseat();
                }
                None => {
                    self.failed = Some(key);
                    self.scene = None;
                    return None;
                }
            }
        }
        let assets = self.assets.clone()?;
        if self.last_turn.is_some_and(|t| session.turn() < t) {
            // A new leg against the same opponent: a fresh camera.
            self.reseat();
        }
        self.last_turn = Some(session.turn());
        self.tick = self.tick.wrapping_add(1);
        self.beat = session.choreo().beat();
        self.pose(session, &assets);
        self.drive_camera(session, &assets);
        self.scene.as_ref()
    }

    /// The framing a host that drives its own selection flow asks for (the
    /// minigames page, which commits through the session's card calls rather
    /// than through its `legaia_engine_minigames::muscle_dome::DomeMenu`). `None` - every
    /// world host - derives it from the session's own menu.
    pub fn set_select_framing(&mut self, framing: Option<SelectFraming>) {
        self.select_framing = framing;
    }

    /// The play-out beat the last frame filmed, and the ticks into it - the
    /// schedule a host that times its own play-out readouts follows
    /// (`muscle_dome_session::choreo`).
    pub fn playback_beat(&self) -> Option<(Beat, u32)> {
        self.beat
    }

    /// The Battle Camera option the framings run under (`0x800846C0`:
    /// `0` Close, `1` Normal, `2` Far) - the world's
    /// `toggles.battle_camera`, which both hosts pass in.
    pub fn set_camera_option(&mut self, option: u8) {
        self.camera_option = option;
    }

    /// Back to leg open: the camera re-created on its next drive.
    fn reseat(&mut self) {
        self.beat = None;
        self.camera = None;
    }

    fn reset(&mut self) {
        if self.scene.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        self.reseat();
        self.last_turn = None;
    }

    /// Body `i`'s floor position and heading: the session's choreography,
    /// or the seat.
    fn place(session: &MuscleDomeSession, i: usize) -> ([f32; 2], u16) {
        let c = session.choreo();
        (c.position(i), c.facing(i))
    }

    fn pose(&mut self, session: &MuscleDomeSession, assets: &MuscleDomeAssets) {
        let scene = self.scene.get_or_insert_with(|| {
            let mut m = empty_mesh();
            let mut flat = Vec::new();
            for b in &assets.bodies {
                append(
                    &mut m,
                    &mut flat,
                    &b.mesh,
                    &legaia_engine_field::packet_color::textured(&b.mesh),
                );
            }
            let bases = [0, assets.bodies[0].mesh.positions.len(), m.positions.len()];
            append(&mut m, &mut flat, &assets.statics, &assets.statics_flat);
            let (mut tex, mut untex) = (Vec::new(), Vec::new());
            for t in m.indices.as_chunks::<3>().0 {
                let textured = flat.get(t[0] as usize * 4 + 3).is_some_and(|&a| a != 0);
                if textured { &mut tex } else { &mut untex }.extend_from_slice(t);
            }
            MuscleDomeScene {
                positions: m.positions,
                uvs: m.uvs,
                cba_tsb: m.cba_tsb,
                colors: m.colors,
                flat_rgba: flat,
                indices: m.indices,
                textured_indices: tex,
                untextured_indices: untex,
                camera: DomeCamera {
                    pose: cam::BOOT_POSE,
                },
                bases,
            }
        });
        let choreo = session.choreo();
        for (i, body) in assets.bodies.iter().enumerate() {
            let (pos, facing) = Self::place(session, i);
            let yaw = f32::from(facing) / 4096.0 * std::f32::consts::TAU;
            let place = [pos[0], yaw, pos[1]];
            let base = scene.bases[i];
            let n = body.mesh.positions.len();
            let out = &mut scene.positions[base..base + n];
            match choreo.pose(i) {
                // The battle anim player's decoded pose.
                Some(parts) => {
                    let keys: Vec<[i32; 6]> = parts
                        .iter()
                        .map(|(t, r)| {
                            [
                                i32::from(t[0]),
                                i32::from(t[1]),
                                i32::from(t[2]),
                                i32::from(r[0]),
                                i32::from(r[1]),
                                i32::from(r[2]),
                            ]
                        })
                        .collect();
                    pose_into(out, &body.mesh.positions, &body.oids, &keys, place);
                }
                // No choreography installed: the idle loop at the seat.
                None => {
                    let f = body.idle.frame_at(self.tick);
                    pose_into(
                        out,
                        &body.mesh.positions,
                        &body.oids,
                        &body.idle.frames[f],
                        place,
                    );
                }
            }
        }
    }

    /// One battle-camera actor record for body `i`.
    fn cam_actor(
        session: &MuscleDomeSession,
        i: usize,
        assets: &MuscleDomeAssets,
    ) -> cam::BattleCamActor {
        let char_id = assets.key.1 as u8 + 1;
        let (pos, facing) = Self::place(session, i);
        cam::BattleCamActor {
            facing: i32::from(facing),
            world: [pos[0], 0.0, pos[1]],
            height: (i == 0)
                .then(|| {
                    assets
                        .heights
                        .as_ref()
                        .and_then(|h| h.height_for_char_id(char_id))
                        .map(f32::from)
                })
                .flatten(),
        }
    }

    /// Drive the battle camera script one frame over the dome's state - the
    /// phase each moment of a leg is in a battle, the acting body, its
    /// target and the formation - and store the pose on the scene.
    fn drive_camera(&mut self, session: &MuscleDomeSession, assets: &MuscleDomeAssets) {
        use legaia_engine_battle_menus::battle_input::CommandPhase;
        use legaia_engine_battle_menus::muscle_dome_session::DomeMenu;
        let beat = self.beat.map(|(b, _)| b);
        let acting = beat.map_or(0, |b| b.attacker);
        let target = acting ^ 1;
        let monster_pos = Self::place(session, 1).0;
        // (phase, the action-SM state its arm sits in, the cursor)
        let (phase, action_state, cursor) = match (beat, session.phase()) {
            (Some(b), _) => match b.kind {
                // The approach: case 6 from the seed on (`0x14`).
                BeatKind::Approach => (cam::BattleCamPhase::Action, 0x14, None),
                // The strike loop arms case 7 on every pass (`0x1E`).
                BeatKind::Swing => (cam::BattleCamPhase::Recover, 0x1E, None),
                // The done band: case 8 for an attack (`0x50`).
                BeatKind::Done => (
                    cam::done_band_phase(cam::DoneBandInputs {
                        category: cam::DONE_CATEGORY_ATTACK,
                        target_party_seat: target == 0,
                        target_dead: session.hp(target) <= 0,
                        ..Default::default()
                    }),
                    0x50,
                    None,
                ),
            },
            // The battle-open hold: the far framing (retail's flow `0x0A` /
            // `0x0B` arms no close-up).
            (None, MusclePhase::Select) if session.intro_up() => {
                (cam::BattleCamPhase::Menu, 0, None)
            }
            (None, MusclePhase::Select) if self.select_framing.is_some() => {
                match self.select_framing {
                    Some(SelectFraming::Member) => (cam::BattleCamPhase::Submenu, 0, None),
                    Some(SelectFraming::Target) => (
                        cam::BattleCamPhase::TargetEnemy,
                        0,
                        Some(cam::CursorFraming::Enemy {
                            target: monster_pos,
                        }),
                    ),
                    _ => (cam::BattleCamPhase::Menu, 0, None),
                }
            }
            (None, MusclePhase::Select) => match session.menu() {
                DomeMenu::Command(c) => match c.phase {
                    // The round prompt and the Begin / Reselect confirm: the
                    // far framing with its idle orbit (case 9).
                    CommandPhase::RoundPrompt { .. }
                    | CommandPhase::CommitConfirm { .. }
                    | CommandPhase::BeginRound => (cam::BattleCamPhase::Menu, 0, None),
                    // The Attack chip's Auto | Command prompt and its cursor
                    // turn the member toward the opponent (case 1).
                    CommandPhase::AttackMode { .. } | CommandPhase::Targeting { .. } => (
                        cam::BattleCamPhase::TargetEnemy,
                        0,
                        Some(cam::CursorFraming::Enemy {
                            target: monster_pos,
                        }),
                    ),
                    // The ring: the member's close-up (case 0).
                    _ => (cam::BattleCamPhase::Submenu, 0, None),
                },
                // The direction entry, its review and the Ra-Seru list are
                // the member's own surfaces (case 0).
                DomeMenu::Input(_) | DomeMenu::Magic => (cam::BattleCamPhase::Submenu, 0, None),
            },
            // A won leg ends on case 6's battle-over arm behind the fighter.
            (None, MusclePhase::Won) => (cam::BattleCamPhase::Action, 0, None),
            // Between turns and on a lost leg the far framing holds.
            (None, _) => (cam::BattleCamPhase::Menu, 0, None),
        };
        let won = beat.is_none() && session.phase() == MusclePhase::Won;
        let slot_of = |i: usize| if i == 0 { 0u8 } else { 3 };
        let depth = legaia_engine_battle_vm::battle_formulas::camera_height_for_frame(
            slot_of(acting),
            slot_of(target),
            legaia_engine_battle_vm::battle_formulas::RETAIL_MONSTER_SLOT_BASE,
            |_| assets.size_class,
        );
        let mut formation = None;
        for i in 0..2 {
            let p = Self::place(session, i).0;
            cam::FormationBox::extend(&mut formation, p[0], p[1]);
        }
        let t = Self::cam_actor(session, target, assets);
        let inputs = cam::BattleCamInputs {
            phase,
            acting: Some(Self::cam_actor(session, acting, assets)),
            target: Some(cam::PostActionTarget {
                world: t.world,
                display_y: 0.0,
                party: target == 0,
                height: t.height,
                monster_id: if target == 1 {
                    assets.key.0.min(0xFF) as u8
                } else {
                    0
                },
                animating: false,
                live: session.hp(target) > 0,
                facing: t.facing,
                node_gone: false,
                lone_defeat: false,
            }),
            formation,
            action: cam::ActionFraming {
                party_slot: acting == 0,
                battle_over: won,
                depth_raw: i32::from(depth),
                char_id: if acting == 0 {
                    assets.key.1 as u8 + 1
                } else {
                    0
                },
                ..Default::default()
            },
            entry_yaw: cam::battle_entry_yaw(0),
            action_state,
            camera_option: self.camera_option,
            cursor,
            ..Default::default()
        };
        cam::drive(&mut self.camera, true, inputs, u64::from(self.tick), None);
        if let (Some(scene), Some(c)) = (self.scene.as_mut(), self.camera.as_ref()) {
            scene.camera = DomeCamera { pose: c.pose() };
        }
    }

    /// Bumped whenever the static buffers or the VRAM a host holds are stale.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The live scene, as the last [`Self::frame`] posed it.
    pub fn scene(&self) -> Option<&MuscleDomeScene> {
        self.scene.as_ref()
    }

    /// The seated dome's VRAM.
    pub fn vram(&self) -> Option<&legaia_tim::Vram> {
        self.scene.as_ref()?;
        Some(self.assets.as_ref()?.vram())
    }
}
