//! Expansion-pack cutscenes on the `World` (`World::expansion.cutscene`):
//! starting one, the per-tick step that runs the sequencer
//! ([`legaia_engine_expansion::cutscene::CutsceneRun`]) and carries out its
//! commands, the walks it started, the Start-button skip, and the reads the
//! hosts draw the shot and the screen layer from.
//!
//! A cutscene drives the world only through paths the world already has: a
//! pack line is the inline dialogue runner's ([`World::expansion_say`]), an
//! NPC's walk leg is the motion VM's ([`World::start_field_npc_motion`]), the
//! player's walk moves its anchor and plays its walk clip the way the
//! in-world mode stage does, a fight is the pack's queued fight, and every
//! flag is a pack flag. While a run holds the player,
//! [`World::script_context_engages_player`] answers `true`, which is the one
//! predicate the walk, the talk probe, the menu button, the encounter roll
//! and the save refusal already read.
//!
//! All of it is the identity with no pack: no run can start without one.

use std::sync::Arc;

use legaia_engine_expansion::cutscene::{
    Command, CutsceneEnv, CutsceneRun, Fight, ScreenQuad, Walk,
};
use legaia_engine_expansion::quest::{PackFight, QuestEvent};
use legaia_engine_field::field_player_clip::{BASE_IDLE, BASE_RUN, BASE_WALK};
use legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::cutscene::{PLAYER, retail_slot, seen_flag};
use legaia_expansion_format::quest::Prerequisites;
use legaia_expansion_format::refs::DiscRef;

use super::World;

/// The player's walk pace, world units a frame: the pad walk's base step
/// (`FIELD_BASE_STEP`) and the run's.
const PLAYER_WALK_SPEED: i32 = 8;
const PLAYER_RUN_SPEED: i32 = 12;

/// The global BGM id of sound-test track `n` (`docs/subsystems/script-vm.md`,
/// "BGM lookup").
const GLOBAL_BGM_BASE: u16 = 2000;

/// The `World` as the sequencer reads it.
struct WorldCutsceneView<'a> {
    world: &'a World,
    /// The cutscene's own map binding: tiles are that map's local tiles.
    on_map: bool,
}

impl CutsceneEnv for WorldCutsceneView<'_> {
    fn actor_busy(&self, actor: &str) -> bool {
        self.world.expansion.cutscene.walks.contains_key(actor)
    }

    fn dialog_busy(&self) -> bool {
        let w = self.world;
        w.expansion.quests.dialog.pending.is_some()
            || !w.expansion.quests.dialog.queued.is_empty()
            || w.dialogue_owns_input()
    }

    fn choice(&self) -> Option<usize> {
        self.world.expansion.cutscene.last_choice
    }

    fn line_has_choices(&self, line: &str) -> bool {
        self.world
            .expansion
            .pack
            .as_ref()
            .and_then(|p| p.dialog.lines.get(line))
            .is_some_and(|l| !l.choices.is_empty())
    }

    fn holds(&self, requires: &Prerequisites) -> bool {
        self.world.expansion_prerequisites_hold(requires)
    }

    fn counter(&self, counter: &str) -> i64 {
        self.world.expansion.quests.progress.flags.counter(counter)
    }

    fn actor_pos(&self, actor: &str) -> Option<(i32, i32)> {
        self.world
            .cutscene_actor_pos(actor)
            .map(|(x, z)| (i32::from(x), i32::from(z)))
    }

    fn tile_world(&self, tile: [u16; 2]) -> (i32, i32) {
        self.world.cutscene_tile_world(tile, self.on_map)
    }
}

/// A heading (12-bit, `0` = world `+Z`, a quarter turn = `+X`) from `from`
/// toward `to`; `None` when they coincide.
fn heading_toward(from: (i16, i16), to: (i16, i16)) -> Option<i16> {
    let dx = f64::from(to.0) - f64::from(from.0);
    let dz = f64::from(to.1) - f64::from(from.1);
    if dx == 0.0 && dz == 0.0 {
        return None;
    }
    let a = dx.atan2(dz) / std::f64::consts::TAU * 4096.0;
    Some((a.round() as i32).rem_euclid(4096) as i16)
}

/// A pack NPC's heading for compass sector `facing` (the spawn table, as
/// `World::install_expansion_npcs` seats it).
fn sector_heading(facing: u8) -> i16 {
    (i16::from(facing & 7) * 0x200 + 0x800) & 0x0FFF
}

impl World {
    /// Whether an expansion cutscene is in play.
    pub fn expansion_cutscene_active(&self) -> bool {
        self.expansion.cutscene.active()
    }

    /// Whether an expansion cutscene holds the player now.
    pub fn expansion_cutscene_locks_player(&self) -> bool {
        self.expansion.cutscene.locks_player()
    }

    /// Queue pack cutscene `id` to play as soon as the field is free (a map
    /// trigger, an arrival, a quest moment). `false` with no pack or no such
    /// cutscene.
    pub fn expansion_play_cutscene(&mut self, id: &str) -> bool {
        let known = self
            .expansion
            .pack
            .as_ref()
            .is_some_and(|p| p.cutscenes.get(id).is_some());
        if !known {
            log::warn!("expansion: no cutscene `{id}`");
            return false;
        }
        if !self.expansion.cutscene.queued.iter().any(|q| q == id)
            && self
                .expansion
                .cutscene
                .run
                .as_ref()
                .is_none_or(|r| r.id != id)
        {
            self.expansion.cutscene.queued.push(id.to_string());
        }
        true
    }

    /// Start pack cutscene `id` now. `false`, changing nothing, with no pack,
    /// no such cutscene, another one already running, or a cutscene bound to
    /// a map the player is not on.
    pub fn start_expansion_cutscene(&mut self, id: &str) -> bool {
        let Some(pack) = self.expansion.pack.clone() else {
            return false;
        };
        let Some(def) = pack.cutscenes.get(id) else {
            log::warn!("expansion: no cutscene `{id}`");
            return false;
        };
        if self.expansion.cutscene.run.is_some() {
            return false;
        }
        let here = self.expansion.map.as_ref().map(|m| m.map_id.as_str());
        if let Some(m) = &def.map
            && here != Some(m.as_str())
        {
            log::warn!("expansion: cutscene `{id}` plays on map `{m}`, not here");
            return false;
        }
        let seen = self.expansion.quests.progress.flags.is_set(&seen_flag(id));
        log::info!("expansion: cutscene `{id}` starts");
        let rt = &mut self.expansion.cutscene;
        rt.run = Some(CutsceneRun::new(id, Arc::new(def.clone()), seen));
        rt.last_choice = None;
        rt.said = None;
        rt.fight = None;
        rt.awaiting_release = false;
        true
    }

    /// The per-tick cutscene step, from [`Self::tick_expansion`]: start a
    /// queued cutscene once the field is free, advance the walks, step the
    /// run and carry out its commands, take a skip, and end a finished run.
    pub(in crate::world) fn tick_expansion_cutscene(&mut self) {
        let frames = u32::from(self.clock.display_frame_step.max(1));
        if self.expansion.cutscene.run.is_none() {
            self.start_queued_cutscene();
        }
        if self.expansion.cutscene.run.is_none() {
            return;
        }
        // A battle the run did not start (an encounter while unlocked, a
        // quest's fight) holds it until the field returns.
        if !matches!(self.mode, SceneMode::Field | SceneMode::WorldMap) {
            return;
        }
        self.step_cutscene_walks();
        if self.expansion.cutscene.awaiting_release {
            if !self.input.pressed(PadButton::Start) {
                self.end_expansion_cutscene();
            }
            return;
        }
        let mut run = self.expansion.cutscene.run.take().expect("checked");
        let on_map = run.def().map.is_some();
        let skip = self.input.just_pressed(PadButton::Start)
            && run.skippable
            && !self.dialogue_menu_open();
        let cmds = {
            let view = WorldCutsceneView {
                world: self,
                on_map,
            };
            let mut cmds = run.step(&view, frames);
            if skip
                && !run.finished
                && let Some(more) = run.skip(&view)
            {
                cmds.extend(more);
            }
            cmds
        };
        let skipped = run.skipped && run.finished;
        self.expansion.cutscene.run = Some(run);
        for c in cmds {
            self.run_cutscene_command(c, on_map);
        }
        let finished = self
            .expansion
            .cutscene
            .run
            .as_ref()
            .is_some_and(|r| r.finished);
        if finished {
            if skipped && self.input.pressed(PadButton::Start) {
                self.expansion.cutscene.awaiting_release = true;
            } else {
                self.end_expansion_cutscene();
            }
        }
    }

    /// Whether the dialogue box on screen is a choice menu (the one thing a
    /// skip does not pass).
    fn dialogue_menu_open(&self) -> bool {
        let Some(line) = self.expansion.cutscene.said.as_deref() else {
            return false;
        };
        self.dialogue_owns_input()
            && self
                .expansion
                .pack
                .as_ref()
                .and_then(|p| p.dialog.lines.get(line))
                .is_some_and(|l| !l.choices.is_empty())
    }

    fn start_queued_cutscene(&mut self) {
        let free = matches!(self.mode, SceneMode::Field)
            && self.expansion.quests.dialog.pending.is_none()
            && self.expansion.quests.dialog.queued.is_empty()
            && !self.dialogue_owns_input()
            && self.expansion.quests.fights.is_empty()
            && self.scene_transition_hold.is_none()
            && self.pending_named_scene_transition.is_none()
            && self.pending_scene_transition.is_none();
        if !free {
            return;
        }
        while !self.expansion.cutscene.queued.is_empty() {
            let id = self.expansion.cutscene.queued.remove(0);
            if self.start_expansion_cutscene(&id) {
                return;
            }
        }
    }

    /// Tear down the run: land the walks, raise the seen flag, and queue the
    /// fight it ended on. The camera, the fades and the letterbox go with
    /// it.
    fn end_expansion_cutscene(&mut self) {
        let Some(run) = self.expansion.cutscene.run.take() else {
            return;
        };
        let walks: Vec<String> = self.expansion.cutscene.walks.keys().cloned().collect();
        let on_map = run.def().map.is_some();
        for a in walks {
            self.finish_cutscene_walk(&a, on_map);
        }
        self.expansion.cutscene.awaiting_release = false;
        self.expansion.cutscene.said = None;
        self.expansion.cutscene.last_choice = None;
        // A player the run moved re-seats the follow camera on itself, the
        // way a warp's seat does (`[4C 39]`): a locked player's own walk
        // never moved the follow focus.
        if std::mem::take(&mut self.expansion.cutscene.player_moved) {
            self.push_camera_zone_request(
                legaia_engine_field::camera::CameraZoneRequest::QueryConformAndSnap,
            );
        }
        log::info!(
            "expansion: cutscene `{}` {}",
            run.id,
            if run.skipped { "skipped" } else { "ends" }
        );
        self.expansion_notify(QuestEvent::FlagSet(seen_flag(&run.id)));
        if let Some(f) = self.expansion.cutscene.fight.take() {
            self.expansion.quests.fights.push(match f {
                Fight::Formation(f) => PackFight::Formation(f),
                Fight::Boss(b) => PackFight::Boss(b),
            });
        }
    }

    fn run_cutscene_command(&mut self, c: Command, on_map: bool) {
        match c {
            Command::Say(line) => {
                self.expansion.cutscene.last_choice = None;
                self.expansion.cutscene.said = Some(line.clone());
                if let Err(e) = self.expansion_say(&line) {
                    log::warn!("expansion: cutscene line `{line}`: {e:?}");
                }
            }
            Command::CloseDialog => {
                self.dialog.inline = None;
                self.dialog.current = None;
            }
            Command::Walk {
                actor,
                waypoints,
                speed,
                run,
                clip,
            } => {
                let stops = waypoints
                    .iter()
                    .map(|t| {
                        let (x, z) = self.cutscene_tile_world(*t, on_map);
                        (x as i16, z as i16)
                    })
                    .collect();
                let idle = self
                    .cutscene_npc_slot(&actor)
                    .and_then(|s| self.npcs.clip_current.get(&s).copied());
                let walk_clip = clip.or_else(|| self.cutscene_npc_walk_clip(&actor));
                self.expansion.cutscene.walks.insert(
                    actor,
                    Walk {
                        stops,
                        speed,
                        run,
                        clip: walk_clip,
                        idle,
                        leg_started: false,
                        prior_speed: None,
                    },
                );
            }
            Command::FinishWalk { actor } => self.finish_cutscene_walk(&actor, on_map),
            Command::Face {
                actor,
                facing,
                toward,
            } => {
                if let Some(f) = facing {
                    self.face_cutscene_actor_sector(&actor, f);
                } else if let Some(t) = toward
                    && let (Some(from), Some(to)) =
                        (self.cutscene_actor_pos(&actor), self.cutscene_actor_pos(&t))
                    && let Some(h) = heading_toward(from, to)
                {
                    self.set_cutscene_actor_heading(&actor, h);
                }
            }
            Command::Clip { actor, clip } => self.play_cutscene_clip(&actor, clip),
            Command::IdleClip { actor } => self.idle_cutscene_actor(&actor, None),
            Command::Teleport {
                actor,
                tile,
                facing,
            } => {
                let (x, z) = self.cutscene_tile_world(tile, on_map);
                self.expansion.cutscene.walks.remove(&actor);
                self.place_cutscene_actor(&actor, x as i16, z as i16);
                if let Some(f) = facing {
                    self.face_cutscene_actor_sector(&actor, f);
                }
            }
            Command::Show { npc, tile } => {
                let Some(slot) = self.cutscene_npc_slot(&npc) else {
                    return;
                };
                let at = match tile {
                    Some(t) => {
                        let (x, z) = self.cutscene_tile_world(t, on_map);
                        Some((x as i16, z as i16))
                    }
                    None => self.expansion.cutscene.hidden.get(&slot).copied(),
                };
                self.expansion.cutscene.hidden.remove(&slot);
                if let Some((x, z)) = at {
                    self.npcs.positions.insert(slot, (x, z));
                }
            }
            Command::Hide { npc } => {
                if let Some(slot) = self.cutscene_npc_slot(&npc) {
                    self.hide_expansion_npc(slot);
                }
            }
            Command::SetFlag(f) => self.expansion_notify(QuestEvent::FlagSet(f)),
            Command::ClearFlag(f) => self.expansion_notify(QuestEvent::FlagCleared(f)),
            Command::Counter { counter, add } => {
                self.expansion_notify(QuestEvent::CounterAdd { counter, add })
            }
            Command::GiveItem { item, count } => {
                let _ = self.party.inventory.add(item, count.min(99) as u8);
            }
            Command::TakeItem { item, count } => {
                let held = self.party.inventory.get(&item).copied().unwrap_or(0);
                let left = u32::from(held).saturating_sub(count) as u8;
                if left == 0 {
                    self.party.inventory.remove(&item);
                } else {
                    self.party.inventory.insert(item, left);
                }
            }
            Command::Gold(n) => {
                self.party.money = (i64::from(self.party.money) + n).clamp(0, 9_999_999) as i32;
            }
            Command::GiveCard(card) => {
                self.expansion.quests.card_inbox.push(card);
                self.collect_card_grants();
            }
            Command::StartQuest(q) => {
                if !self.expansion_start_quest(&q) {
                    log::info!("expansion: cutscene quest `{q}` did not start");
                }
            }
            Command::Sfx(id) => self.push_sfx_cue(id as i16),
            Command::Bgm(DiscRef::Bgm { id }) => {
                self.pending_field_events.push(
                    legaia_engine_field::field_events::FieldEvent::Bgm {
                        text_id: GLOBAL_BGM_BASE + id,
                        sub_op: 1,
                    },
                );
            }
            Command::Bgm(other) => log::warn!("expansion: cutscene bgm {other} is not a bgm"),
            Command::Lock(_) => {}
            Command::Fight(f) => self.expansion.cutscene.fight = Some(f),
        }
    }

    /// The world centre of `tile`: a map-local tile of the active pack map
    /// for a cutscene bound to one, else a field-grid tile of the scene.
    fn cutscene_tile_world(&self, tile: [u16; 2], on_map: bool) -> (i32, i32) {
        match self.expansion.map.as_ref().filter(|_| on_map) {
            Some(m) => m.def.tile_centre(tile[0], tile[1]),
            None => (i32::from(tile[0]) * 128 + 64, i32::from(tile[1]) * 128 + 64),
        }
    }

    /// The placement slot a cutscene actor id names: a pack NPC of the
    /// active map, or `retail:<slot>`. `None` for the player.
    fn cutscene_npc_slot(&self, actor: &str) -> Option<u8> {
        if actor == PLAYER {
            return None;
        }
        if let Some(s) = retail_slot(actor) {
            return Some(s);
        }
        self.expansion
            .map
            .as_ref()?
            .npcs()
            .find(|(_, d)| d.id == actor)
            .map(|(s, _)| s)
    }

    fn cutscene_npc_walk_clip(&self, actor: &str) -> Option<u8> {
        self.expansion
            .map
            .as_ref()?
            .npcs()
            .find(|(_, d)| d.id == actor)
            .and_then(|(_, d)| d.walk_clip)
    }

    /// An actor's world `(x, z)`.
    fn cutscene_actor_pos(&self, actor: &str) -> Option<(i16, i16)> {
        if actor == PLAYER {
            let p = self.actors.get(usize::from(self.player_actor_slot?))?;
            return Some((p.move_state.world_x, p.move_state.world_z));
        }
        let slot = self.cutscene_npc_slot(actor)?;
        self.npcs.positions.get(&slot).copied()
    }

    fn place_cutscene_actor(&mut self, actor: &str, x: i16, z: i16) {
        if actor == PLAYER {
            self.expansion.cutscene.player_moved = true;
            let y = self.sample_field_floor_height(i32::from(x), i32::from(z));
            if let Some(p) = self
                .player_actor_slot
                .and_then(|s| self.actors.get_mut(usize::from(s)))
            {
                p.move_state.world_x = x;
                p.move_state.world_z = z;
                p.move_state.world_y = y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            }
        } else if let Some(slot) = self.cutscene_npc_slot(actor) {
            self.npcs.motions.remove(&slot);
            self.npcs.positions.insert(slot, (x, z));
        }
    }

    fn set_cutscene_actor_heading(&mut self, actor: &str, heading: i16) {
        if actor == PLAYER {
            if let Some(p) = self
                .player_actor_slot
                .and_then(|s| self.actors.get_mut(usize::from(s)))
            {
                p.move_state.render_26 = heading & 0x0FFF;
            }
        } else if let Some(slot) = self.cutscene_npc_slot(actor) {
            self.npcs.headings.insert(slot, heading & 0x0FFF);
        }
    }

    fn face_cutscene_actor_sector(&mut self, actor: &str, facing: u8) {
        if actor == PLAYER {
            self.face_player_sector(facing & 7);
        } else {
            self.set_cutscene_actor_heading(actor, sector_heading(facing));
        }
    }

    fn play_cutscene_clip(&mut self, actor: &str, clip: u8) {
        if actor == PLAYER {
            self.field_player_strided_clip(u16::from(clip));
        } else if let Some(slot) = self.cutscene_npc_slot(actor) {
            self.carry_npc_run_anim(slot, clip);
        }
    }

    /// Put an actor's standing clip back: the player's idle base, an NPC's
    /// `idle` (the clip it showed before the walk) or its map clip.
    fn idle_cutscene_actor(&mut self, actor: &str, idle: Option<u8>) {
        if actor == PLAYER {
            self.field_player_strided_clip(BASE_IDLE);
            return;
        }
        let Some(slot) = self.cutscene_npc_slot(actor) else {
            return;
        };
        let own = self
            .expansion
            .map
            .as_ref()
            .and_then(|m| m.npc_at_slot(slot))
            .and_then(|d| d.clip);
        if let Some(c) = idle.or(own) {
            self.carry_npc_run_anim(slot, c);
        }
    }

    /// Park a pack NPC on the off-map hide box retail's own scripts use
    /// ([`FIELD_OFFMAP_HIDE_XZ`]): out of every draw, talk probe and
    /// collision box. Where it stood is kept for a `show`.
    pub(in crate::world) fn hide_expansion_npc(&mut self, slot: u8) {
        let hide = FIELD_OFFMAP_HIDE_XZ;
        if let Some(&at) = self.npcs.positions.get(&slot)
            && at != (hide, hide)
        {
            self.expansion.cutscene.hidden.insert(slot, at);
        }
        self.npcs.motions.remove(&slot);
        self.npcs.positions.insert(slot, (hide, hide));
    }

    /// Hide the active map's NPCs whose `present` gate is shut, as the map
    /// is entered. No-op off a pack map.
    pub fn expansion_hide_absent_npcs(&mut self) {
        let Some(map) = self.expansion.map.as_ref() else {
            return;
        };
        let absent: Vec<u8> = map
            .npcs()
            .filter(|(_, d)| {
                !d.present.is_empty() && !self.expansion_prerequisites_hold(&d.present)
            })
            .map(|(s, _)| s)
            .collect();
        for slot in absent {
            self.hide_expansion_npc(slot);
        }
    }

    /// Whether pack NPC `slot` is hidden this visit.
    pub fn expansion_npc_hidden(&self, slot: u8) -> bool {
        self.expansion.cutscene.hidden.contains_key(&slot)
    }

    /// Advance every walk one frame: the player's anchor steps toward its
    /// next stop at its pace; an NPC's leg runs on the motion VM and the
    /// next starts when it lands. A walk that reaches its last stop puts the
    /// idle clip back and is dropped.
    fn step_cutscene_walks(&mut self) {
        let actors: Vec<String> = self.expansion.cutscene.walks.keys().cloned().collect();
        for actor in actors {
            if actor == PLAYER {
                self.step_player_cutscene_walk();
            } else {
                self.step_npc_cutscene_walk(&actor);
            }
        }
    }

    fn step_player_cutscene_walk(&mut self) {
        let Some(walk) = self.expansion.cutscene.walks.get_mut(PLAYER) else {
            return;
        };
        let Some(&(tx, tz)) = walk.stops.first() else {
            self.expansion.cutscene.walks.remove(PLAYER);
            self.field_player_strided_clip(BASE_IDLE);
            return;
        };
        let speed = walk.speed.map(i32::from).unwrap_or(if walk.run {
            PLAYER_RUN_SPEED
        } else {
            PLAYER_WALK_SPEED
        });
        let base = walk
            .clip
            .map(u16::from)
            .unwrap_or(if walk.run { BASE_RUN } else { BASE_WALK });
        let first = !walk.leg_started;
        walk.leg_started = true;
        let Some((x, z)) = self.cutscene_actor_pos(PLAYER) else {
            self.expansion.cutscene.walks.remove(PLAYER);
            return;
        };
        if first || self.locomotion.clip_base != base {
            self.locomotion.player_party_bank = true;
            self.locomotion.player_clip_rate = legaia_engine_field::field_anim::CLIP_RATE as i16;
            self.field_player_strided_clip(base);
        }
        let (dx, dz) = (i32::from(tx) - i32::from(x), i32::from(tz) - i32::from(z));
        let dist = f64::from(dx * dx + dz * dz).sqrt();
        let (nx, nz) = if dist <= f64::from(speed) {
            (tx, tz)
        } else {
            let k = f64::from(speed) / dist;
            (
                (f64::from(x) + f64::from(dx) * k).round() as i16,
                (f64::from(z) + f64::from(dz) * k).round() as i16,
            )
        };
        if let Some(h) = heading_toward((x, z), (tx, tz)) {
            self.set_cutscene_actor_heading(PLAYER, h);
        }
        self.place_cutscene_actor(PLAYER, nx, nz);
        if let Some(anim) = &mut self.locomotion.player_anim {
            anim.pad_drove_this_frame = true;
            anim.moved_this_frame = true;
        }
        if (nx, nz) == (tx, tz)
            && let Some(walk) = self.expansion.cutscene.walks.get_mut(PLAYER)
        {
            walk.stops.remove(0);
            if walk.stops.is_empty() {
                self.expansion.cutscene.walks.remove(PLAYER);
                self.field_player_strided_clip(BASE_IDLE);
                // The follow camera re-seats where the walk ended, so a
                // later `camera_reset` eases back onto the player.
                self.push_camera_zone_request(
                    legaia_engine_field::camera::CameraZoneRequest::QueryConformAndSnap,
                );
            }
        }
    }

    fn step_npc_cutscene_walk(&mut self, actor: &str) {
        let Some(slot) = self.cutscene_npc_slot(actor) else {
            self.expansion.cutscene.walks.remove(actor);
            return;
        };
        let Some(walk) = self.expansion.cutscene.walks.get(actor).cloned() else {
            return;
        };
        if walk.leg_started && self.npcs.motions.contains_key(&slot) {
            return;
        }
        let mut walk = walk;
        if walk.leg_started {
            walk.stops.remove(0);
            walk.leg_started = false;
        }
        let Some(&(tx, tz)) = walk.stops.first() else {
            self.expansion.cutscene.walks.remove(actor);
            self.restore_walk_speed(slot, &walk);
            self.idle_cutscene_actor(actor, walk.idle);
            return;
        };
        if let Some(v) = walk.speed {
            if walk.prior_speed.is_none() {
                walk.prior_speed = Some(self.npcs.glide_speeds.get(&slot).copied());
            }
            self.npcs.glide_speeds.insert(slot, v);
        }
        if self.start_field_npc_motion(slot, tx, tz) {
            if let Some(c) = walk.clip {
                self.carry_npc_run_anim(slot, c);
            }
            walk.leg_started = true;
            self.expansion
                .cutscene
                .walks
                .insert(actor.to_string(), walk);
        } else {
            // Not a seated NPC: put it where the walk ends.
            self.expansion.cutscene.walks.remove(actor);
        }
    }

    /// Land a walk at its last stop at once (a skip, the end of a run).
    fn finish_cutscene_walk(&mut self, actor: &str, _on_map: bool) {
        let Some(walk) = self.expansion.cutscene.walks.remove(actor) else {
            return;
        };
        if let Some(&(x, z)) = walk.stops.last() {
            if let Some(&(px, pz)) = walk.stops.get(walk.stops.len().saturating_sub(2))
                && let Some(h) = heading_toward((px, pz), (x, z))
            {
                self.set_cutscene_actor_heading(actor, h);
            }
            self.place_cutscene_actor(actor, x, z);
        }
        if let Some(slot) = self.cutscene_npc_slot(actor) {
            self.restore_walk_speed(slot, &walk);
        }
        self.idle_cutscene_actor(actor, walk.idle);
    }

    /// Put back the glide speed a walk's `speed` replaced.
    fn restore_walk_speed(&mut self, slot: u8, walk: &Walk) {
        match walk.prior_speed {
            Some(Some(v)) => {
                self.npcs.glide_speeds.insert(slot, v);
            }
            Some(None) => {
                self.npcs.glide_speeds.remove(&slot);
            }
            None => {}
        }
    }

    /// The cutscene's camera pose and shake this frame, while it moves the
    /// shot off the follow camera: what both hosts frame through
    /// (`legaia_engine_world_views::camera_view`).
    pub fn expansion_cutscene_camera(
        &self,
    ) -> Option<(legaia_engine_expansion::cutscene::CameraPose, [f32; 2])> {
        let run = self.expansion.cutscene.run.as_ref()?;
        run.camera
            .active()
            .then(|| (run.camera.pose(), run.camera.shake()))
    }

    /// The cutscene's screen layer this frame: its full-screen quads (tint,
    /// flash, fade) and the letterbox bar height in display scanlines. Empty
    /// with no run.
    pub fn expansion_cutscene_screen(&self) -> (Vec<ScreenQuad>, i16) {
        match self.expansion.cutscene.run.as_ref() {
            Some(r) => (r.screen.quads(), r.screen.letterbox_lines()),
            None => (Vec::new(), 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_follow_the_field_convention() {
        assert_eq!(heading_toward((0, 0), (0, 100)), Some(0));
        assert_eq!(heading_toward((0, 0), (100, 0)), Some(1024));
        assert_eq!(heading_toward((0, 0), (0, -100)), Some(2048));
        assert_eq!(heading_toward((0, 0), (-100, 0)), Some(3072));
        assert_eq!(heading_toward((5, 5), (5, 5)), None);
        assert_eq!(sector_heading(4), 0);
    }
}
