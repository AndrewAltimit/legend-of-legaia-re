//! The arena surface a host draws: the camera, playback state and the per-frame pose / camera drive.

use super::*;
use crate::muscle_dome::{DomeContest, DomePlay};
use legaia_engine_battle_vm::battle_cam_script as cam;
use legaia_engine_menus::muscle_dome_session::{MuscleDomeSession, MusclePhase};
use std::sync::Arc;

/// The dome's camera: one pose of the battle camera script, projected the
/// way the battle hosts project a stage draw - [`cam::battle_vp`] at the
/// battle world scale over the stage model `diag(4, -4, 4)` - so both hosts
/// take the one matrix for raw Y-down world vertices, depth `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DomeCamera {
    pub pose: cam::BattleCamPose,
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
        cam::mat_mul(&cam::battle_vp(&self.pose, k, aspect), &stage)
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
    ladder: Option<Option<Vec<crate::muscle_dome::DomeCourse>>>,
    scene: Option<MuscleDomeScene>,
    generation: u32,
    tick: u32,
    act: [Option<Act>; 2],
    /// The resolved turn playing out, if any.
    playback: Option<Playback>,
    /// Each body's floor position `(x, z)`, raw battle units.
    pos: [[f32; 2]; 2],
    /// The session turn last seen, so a fresh leg re-seats the pair.
    last_turn: Option<u32>,
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

/// A resolved turn being played out on the surface.
#[derive(Debug, Clone)]
struct Playback {
    start: u32,
    plays: Vec<DomePlay>,
    closes_in: bool,
    /// The approach's start and stop points, fixed on its first tick.
    walk: Option<([f32; 2], [f32; 2])>,
}

impl Playback {
    fn elapsed(&self, tick: u32) -> u32 {
        tick.wrapping_sub(self.start)
    }

    fn beat(&self, tick: u32) -> Option<Beat> {
        beat_at(&self.plays, self.closes_in, self.elapsed(tick))
    }
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
                .and_then(|raw| crate::muscle_dome::parse_course_ladder(&raw))
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
    /// (`legaia_engine_core::world::FrameClock::sim_ticks`): the choreography, the pose
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
            // A new leg against the same opponent: back to the seats.
            self.reseat();
        }
        // A turn resolved since the last frame: its plays act out. Keyed on
        // the turn counter rather than on a `Resolve` frame, because a host
        // that resolves inside one call (the minigames page) never shows the
        // surface one.
        let resolved = self.last_turn.is_some_and(|t| session.turn() > t);
        self.last_turn = Some(session.turn());
        self.step(session, &assets, resolved);
        self.pose(&assets);
        self.drive_camera(session, &assets);
        self.scene.as_ref()
    }

    /// The framing a host that drives its own selection flow asks for (the
    /// minigames page, which commits through the session's card calls rather
    /// than through its [`crate::muscle_dome::DomeMenu`]). `None` - every
    /// world host - derives it from the session's own menu.
    pub fn set_select_framing(&mut self, framing: Option<SelectFraming>) {
        self.select_framing = framing;
    }

    /// The playback beat on screen now, and the ticks into it - the schedule
    /// a host that times its own play-out readouts follows
    /// ([`turn_timeline`]).
    pub fn playback_beat(&self) -> Option<(Beat, u32)> {
        let pb = self.playback.as_ref()?;
        let beat = pb.beat(self.tick)?;
        Some((beat, pb.elapsed(self.tick) - beat.start))
    }

    /// The Battle Camera option the framings run under (`0x800846C0`:
    /// `0` Close, `1` Normal, `2` Far) - the world's
    /// `toggles.battle_camera`, which both hosts pass in.
    pub fn set_camera_option(&mut self, option: u8) {
        self.camera_option = option;
    }

    /// Back to leg open: both bodies on their seats, idling, the camera
    /// re-created on its next drive.
    fn reseat(&mut self) {
        self.act = [None; 2];
        self.playback = None;
        self.pos = [seat_xz(0), seat_xz(1)];
        self.camera = None;
    }

    fn reset(&mut self) {
        if self.scene.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        self.reseat();
        self.last_turn = None;
    }

    /// The choreography: a resolved turn (the session leaving `Resolve`)
    /// starts its [`turn_timeline`]; each tick plays the beat it is on - the
    /// closing walk, a swing with the defender's flinch (its knockdown on
    /// the play that ends the leg), the done tail.
    fn step(&mut self, session: &MuscleDomeSession, assets: &MuscleDomeAssets, resolved: bool) {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        if resolved {
            self.playback = Some(Playback {
                start: tick,
                plays: session.last_turn_plays().to_vec(),
                closes_in: session.last_turn_closes_in(),
                walk: None,
            });
        }
        let Some(pb) = self.playback.as_mut() else {
            return;
        };
        let Some(beat) = pb.beat(tick) else {
            self.playback = None;
            return;
        };
        let at = pb.elapsed(tick) - beat.start;
        let a = beat.attacker;
        let d = a ^ 1;
        match beat.kind {
            BeatKind::Approach => {
                let pos = self.pos;
                let (from, to) = *pb.walk.get_or_insert_with(|| {
                    let from = pos[a];
                    let (dx, dz) = (pos[d][0] - from[0], pos[d][1] - from[1]);
                    let len = (dx * dx + dz * dz).sqrt().max(1e-3);
                    let stop = (len - assets.reach()).max(0.0) / len;
                    (from, [from[0] + dx * stop, from[1] + dz * stop])
                });
                let t = (at + 1) as f32 / beat.len.max(1) as f32;
                self.pos[a] = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                ];
                if at == 0 {
                    self.act[a] = Some(Act {
                        kind: ClipKind::Walk,
                        start: tick,
                        looped: true,
                        hold: false,
                    });
                }
                if at + 1 == beat.len {
                    self.act[a] = Some(Act::idle(tick));
                }
            }
            BeatKind::Swing => {
                let play = pb.plays[beat.play];
                if at == 0 {
                    self.act[a] = Some(Act {
                        kind: ClipKind::Swing(play.cmd),
                        start: tick,
                        looped: false,
                        hold: false,
                    });
                }
                if at == FLINCH_DELAY_TICKS && play.damage > 0 {
                    let down = play.hp_after.get(d).is_some_and(|&hp| hp <= 0);
                    self.act[d] = Some(Act {
                        kind: if down { ClipKind::Ko } else { ClipKind::Hit },
                        start: tick,
                        looped: false,
                        hold: down,
                    });
                }
            }
            BeatKind::Done => {}
        }
    }

    fn pose(&mut self, assets: &MuscleDomeAssets) {
        let scene = self.scene.get_or_insert_with(|| {
            let mut m = empty_mesh();
            let mut flat = Vec::new();
            for b in &assets.bodies {
                append(
                    &mut m,
                    &mut flat,
                    &b.mesh,
                    &crate::packet_color::textured(&b.mesh),
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
        for (i, (body, facing)) in assets.bodies.iter().zip(FACINGS).enumerate() {
            let mut act = self.act[i].unwrap_or(Act::idle(self.tick));
            let elapsed = self.tick.wrapping_sub(act.start);
            let mut clip = clip_for(&body.clips, act.kind);
            let mut frame = (elapsed * clip.rate / 16) as usize;
            if act.looped {
                frame %= clip.frame_count().max(1);
            } else if frame >= clip.frame_count() {
                if act.hold {
                    frame = clip.frame_count().saturating_sub(1);
                } else {
                    act = Act::idle(self.tick);
                    clip = &body.clips.idle;
                    frame = 0;
                }
            }
            self.act[i] = Some(act);
            let base = scene.bases[i];
            let n = body.mesh.positions.len();
            let yaw = facing as f32 / 4096.0 * std::f32::consts::TAU;
            pose_into(
                &mut scene.positions[base..base + n],
                &body.mesh.positions,
                &body.oids,
                clip,
                frame,
                [self.pos[i][0], yaw, self.pos[i][1]],
            );
        }
    }

    /// One battle-camera actor record for body `i`.
    fn cam_actor(&self, i: usize, assets: &MuscleDomeAssets) -> cam::BattleCamActor {
        let char_id = assets.key.1 as u8 + 1;
        cam::BattleCamActor {
            facing: FACINGS[i],
            world: [self.pos[i][0], 0.0, self.pos[i][1]],
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
        use crate::battle_input::CommandPhase;
        use legaia_engine_menus::muscle_dome_session::DomeMenu;
        let beat = self.playback.as_ref().and_then(|p| p.beat(self.tick));
        let acting = beat.map_or(0, |b| b.attacker);
        let target = acting ^ 1;
        let monster_pos = self.pos[1];
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
        for p in &self.pos {
            cam::FormationBox::extend(&mut formation, p[0], p[1]);
        }
        let t = self.cam_actor(target, assets);
        let inputs = cam::BattleCamInputs {
            phase,
            acting: Some(self.cam_actor(acting, assets)),
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
