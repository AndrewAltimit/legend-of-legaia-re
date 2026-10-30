//! A cutscene played without a world: the workbench's text timeline.
//!
//! [`preview`] runs a pack cutscene on the real sequencer against a stand-in
//! world - walks take their distance over their pace, a line takes a reading
//! time from its length, each menu takes the option the caller names - and
//! reports which step ran on which frame of which track. The frame numbers
//! are the sequencer's own; only the world's answers (how long a walk or a
//! line takes) are estimates.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::cutscene::{CameraFocus, FadeTo, PLAYER, Step};
use legaia_expansion_format::dialog::pages;
use legaia_expansion_format::quest::Prerequisites;

use super::{Command, CutsceneEnv, CutsceneRun, Fight};

/// The player's walk pace the preview takes (the pad walk's).
const WALK: f64 = 8.0;
const RUN: f64 = 12.0;
/// Frames a line's box takes per character typed, per page read, and to
/// open a menu.
const FRAMES_PER_CHAR: u32 = 1;
const FRAMES_PER_PAGE: u32 = 45;
const FRAMES_PER_MENU: u32 = 40;

/// One executed step.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewRow {
    /// The frame it ran on (60 a second).
    pub frame: u32,
    /// Its track (`""` for the main one).
    pub track: String,
    /// Its index in that track's list.
    pub step: usize,
    /// Its `do` key.
    pub kind: &'static str,
    /// What it does, in a few words.
    pub text: String,
}

/// A whole run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Preview {
    pub rows: Vec<PreviewRow>,
    /// Frames until the run ended (or the cap).
    pub frames: u32,
    /// Whether it ended within the cap.
    pub ended: bool,
    /// The fight it ends on.
    pub fight: Option<String>,
    /// The pack flags it raises, in order.
    pub flags: Vec<String>,
}

struct SimEnv<'a> {
    pack: &'a ExpansionPack,
    map: Option<&'a legaia_expansion_format::map::MapDef>,
    pos: RefCell<BTreeMap<String, (i32, i32)>>,
    walking: RefCell<BTreeMap<String, u32>>,
    dialog: RefCell<u32>,
    choice: RefCell<Option<usize>>,
    picks: RefCell<Vec<usize>>,
    flags: RefCell<Vec<String>>,
}

impl SimEnv<'_> {
    fn tile(&self, t: [u16; 2]) -> (i32, i32) {
        match self.map {
            Some(m) => m.tile_centre(t[0], t[1]),
            None => (i32::from(t[0]) * 128 + 64, i32::from(t[1]) * 128 + 64),
        }
    }

    fn line_frames(&self, line: &str) -> u32 {
        let Some(l) = self.pack.dialog.lines.get(line) else {
            return 1;
        };
        let ps = pages(&l.text);
        let chars: usize = ps.iter().flatten().map(|r| r.chars().count()).sum();
        let mut f = chars as u32 * FRAMES_PER_CHAR + ps.len() as u32 * FRAMES_PER_PAGE;
        if !l.choices.is_empty() {
            f += FRAMES_PER_MENU;
        }
        f.max(1)
    }

    fn apply(&self, cmds: &[Command]) {
        for c in cmds {
            match c {
                Command::Walk {
                    actor,
                    waypoints,
                    speed,
                    run,
                    ..
                } => {
                    let pace = speed.map(f64::from).unwrap_or(if *run && actor == PLAYER {
                        RUN
                    } else {
                        WALK
                    });
                    let mut at = self.pos.borrow().get(actor).copied().unwrap_or((0, 0));
                    let mut dist = 0.0;
                    for &t in waypoints {
                        let to = self.tile(t);
                        dist += f64::from((to.0 - at.0).pow(2) + (to.1 - at.1).pow(2)).sqrt();
                        at = to;
                    }
                    self.pos.borrow_mut().insert(actor.clone(), at);
                    let frames = (dist / pace).ceil() as u32;
                    if frames > 0 {
                        self.walking.borrow_mut().insert(actor.clone(), frames);
                    }
                }
                Command::Teleport { actor, tile, .. } => {
                    let to = self.tile(*tile);
                    self.pos.borrow_mut().insert(actor.clone(), to);
                }
                Command::Show { npc, tile: Some(t) } => {
                    let to = self.tile(*t);
                    self.pos.borrow_mut().insert(npc.clone(), to);
                }
                Command::FinishWalk { actor } => {
                    self.walking.borrow_mut().remove(actor);
                }
                Command::Say(line) => {
                    *self.dialog.borrow_mut() = self.line_frames(line);
                    let has_menu = self
                        .pack
                        .dialog
                        .lines
                        .get(line)
                        .is_some_and(|l| !l.choices.is_empty());
                    let pick = if has_menu {
                        let mut p = self.picks.borrow_mut();
                        Some(if p.is_empty() { 0 } else { p.remove(0) })
                    } else {
                        None
                    };
                    *self.choice.borrow_mut() = pick;
                }
                Command::CloseDialog => *self.dialog.borrow_mut() = 0,
                Command::SetFlag(f) => self.flags.borrow_mut().push(f.clone()),
                _ => {}
            }
        }
    }

    fn tick(&self) {
        let mut w = self.walking.borrow_mut();
        for v in w.values_mut() {
            *v = v.saturating_sub(1);
        }
        w.retain(|_, v| *v > 0);
        let mut d = self.dialog.borrow_mut();
        *d = d.saturating_sub(1);
    }
}

impl CutsceneEnv for SimEnv<'_> {
    fn actor_busy(&self, actor: &str) -> bool {
        self.walking.borrow().contains_key(actor)
    }
    fn dialog_busy(&self) -> bool {
        *self.dialog.borrow() > 0
    }
    fn choice(&self) -> Option<usize> {
        *self.choice.borrow()
    }
    fn line_has_choices(&self, line: &str) -> bool {
        self.pack
            .dialog
            .lines
            .get(line)
            .is_some_and(|l| !l.choices.is_empty())
    }
    fn holds(&self, r: &Prerequisites) -> bool {
        let f = self.flags.borrow();
        r.flags.iter().all(|x| f.contains(x)) && r.flags_clear.iter().all(|x| !f.contains(x))
    }
    fn counter(&self, _: &str) -> i64 {
        0
    }
    fn actor_pos(&self, actor: &str) -> Option<(i32, i32)> {
        self.pos.borrow().get(actor).copied()
    }
    fn tile_world(&self, tile: [u16; 2]) -> (i32, i32) {
        self.tile(tile)
    }
}

fn tile(t: &[u16; 2]) -> String {
    format!("({}, {})", t[0], t[1])
}

/// A step in a few words, for the timeline.
pub fn describe(step: &Step) -> String {
    match step {
        Step::Wait { frames } => format!("wait {frames} frames"),
        Step::Say { line, on_choice } if on_choice.is_empty() => format!("say \"{line}\""),
        Step::Say { line, on_choice } => {
            format!("say \"{line}\", options -> [{}]", on_choice.join(", "))
        }
        Step::Move {
            actor,
            to,
            path,
            wait,
            ..
        } => {
            let stops: Vec<String> = path.iter().chain(to.iter()).map(tile).collect();
            format!(
                "{actor} walks {}{}",
                stops.join(" -> "),
                if *wait { "" } else { " (no wait)" }
            )
        }
        Step::Face {
            actor,
            facing,
            toward,
        } => match (facing, toward) {
            (Some(f), _) => format!("{actor} faces sector {f}"),
            (_, Some(t)) => format!("{actor} faces {t}"),
            _ => format!("{actor} faces ?"),
        },
        Step::Clip { actor, clip, .. } => format!("{actor} plays clip {clip}"),
        Step::Teleport { actor, tile: t, .. } => format!("{actor} appears on {}", tile(t)),
        Step::Show { npc, tile: Some(t) } => format!("{npc} comes on at {}", tile(t)),
        Step::Show { npc, tile: None } => format!("{npc} comes on"),
        Step::Hide { npc } => format!("{npc} goes off"),
        Step::Camera {
            focus,
            yaw,
            pitch,
            zoom,
            frames,
            ..
        } => {
            let mut parts = Vec::new();
            match focus {
                Some(CameraFocus::Tile { tile: t }) => parts.push(format!("on {}", tile(t))),
                Some(CameraFocus::Actor { actor }) => parts.push(format!("following {actor}")),
                None => {}
            }
            if let Some(v) = yaw {
                parts.push(format!("yaw {v:+}"));
            }
            if let Some(v) = pitch {
                parts.push(format!("pitch {v:+}"));
            }
            if let Some(v) = zoom {
                parts.push(format!("zoom x{v}"));
            }
            let how = if *frames == 0 {
                "cut".to_string()
            } else {
                format!("over {frames} frames")
            };
            format!("camera {} ({how})", parts.join(", "))
        }
        Step::CameraReset { frames, .. } => {
            format!("camera back to the follow shot over {frames} frames")
        }
        Step::Shake { amplitude, frames } => format!("shake {amplitude} for {frames} frames"),
        Step::Fade { to, frames, .. } => {
            let to = match to {
                FadeTo::Black => "black",
                FadeTo::White => "white",
                FadeTo::Clear => "clear",
            };
            format!("fade to {to} over {frames} frames")
        }
        Step::Flash { color, frames } => format!("flash {color:?} for {frames} frames"),
        Step::Banner { line } => format!("banner \"{line}\""),
        Step::Tint { color, .. } if *color == [0, 0, 0] => "tint cleared".to_string(),
        Step::Tint { color, mode, .. } => format!("tint {color:?} ({mode:?})"),
        Step::SetFlag { flag } => format!("raise flag {flag}"),
        Step::ClearFlag { flag } => format!("clear flag {flag}"),
        Step::Counter { counter, add } => format!("counter {counter} {add:+}"),
        Step::Label { name } => format!("label {name}"),
        Step::Goto { label } => format!("jump to {label}"),
        Step::Branch {
            goto, otherwise, ..
        } => match otherwise {
            Some(o) => format!("branch: {goto} if the gate holds, else {o}"),
            None => format!("branch: {goto} if the gate holds"),
        },
        Step::StartTrack { track } => format!("start track {track}"),
        Step::Join { track } => format!("wait for track {track}"),
        Step::Lock => "lock the player".to_string(),
        Step::Unlock => "unlock the player".to_string(),
        Step::Battle { formation } => format!("end, then fight {formation}"),
        Step::Boss { boss } => format!("end, then fight boss {boss}"),
        Step::GiveItem { item, count } => format!("give item {item} x{count}"),
        Step::TakeItem { item, count } => format!("take item {item} x{count}"),
        Step::Gold { amount } => format!("gold {amount:+}"),
        Step::GiveCard { card } => format!("give card {card}"),
        Step::StartQuest { quest } => format!("start quest {quest}"),
        Step::Sfx { id } => format!("sound effect {id}"),
        Step::Bgm { bgm } => format!("music {bgm}"),
        Step::Sound { sound } => format!("sound {sound}"),
        Step::Music { music, fade } => format!("music {music} (fade {fade})"),
        Step::MusicRelease => "music back to the map".to_string(),
        Step::End => "end".to_string(),
    }
}

/// Play cutscene `id` of `pack` for at most `max_frames` frames, taking
/// option `picks[k]` at its `k`th menu (option 0 past the list). `None`
/// when the pack has no such cutscene.
pub fn preview(
    pack: &ExpansionPack,
    id: &str,
    picks: &[usize],
    max_frames: u32,
) -> Option<Preview> {
    let def = pack.cutscenes.get(id)?;
    let map = def.map.as_ref().and_then(|m| pack.maps.get(m));
    let env = SimEnv {
        pack,
        map,
        pos: RefCell::new(BTreeMap::new()),
        walking: RefCell::new(BTreeMap::new()),
        dialog: RefCell::new(0),
        choice: RefCell::new(None),
        picks: RefCell::new(picks.to_vec()),
        flags: RefCell::new(Vec::new()),
    };
    if let Some(m) = map {
        for n in &m.npcs {
            env.pos
                .borrow_mut()
                .insert(n.id.clone(), m.tile_centre(n.tile[0], n.tile[1]));
        }
        if let Some(s) = m.spawns.first() {
            env.pos
                .borrow_mut()
                .insert(PLAYER.to_string(), m.tile_centre(s.tile[0], s.tile[1]));
        }
    }
    let mut run = CutsceneRun::new(id, Arc::new(def.clone()), false);
    run.trace = Some(Vec::new());
    let mut out = Preview::default();
    for _ in 0..max_frames {
        let cmds = run.step(&env, 1);
        env.apply(&cmds);
        for c in &cmds {
            if let Command::Fight(f) = c {
                out.fight = Some(match f {
                    Fight::Formation(x) => format!("formation {x}"),
                    Fight::Boss(x) => format!("boss {x}"),
                });
            }
        }
        if run.finished {
            out.ended = true;
            break;
        }
        env.tick();
    }
    out.frames = run.frame;
    let steps = |track: &str| -> &[Step] {
        if track.is_empty() {
            &def.steps
        } else {
            def.tracks.get(track).map_or(&[], |t| &t.steps)
        }
    };
    for (frame, track, step) in run.trace.take().unwrap_or_default() {
        let Some(s) = steps(&track).get(step) else {
            continue;
        };
        out.rows.push(PreviewRow {
            frame: frame.saturating_sub(1),
            track,
            step,
            kind: s.kind(),
            text: describe(s),
        });
    }
    out.flags = env.flags.into_inner();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> ExpansionPack {
        let mut files = legaia_expansion_format::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
        );
        files.insert(
            "dialog.toml".into(),
            br#"
[lines.ask]
text = "Go?"
choices = [{ label = "Yes" }, { label = "No" }]
"#
            .to_vec(),
        );
        files.insert(
            "cutscenes.toml".into(),
            br#"
[cutscenes.c]
steps = [
  { do = "start_track", track = "pan" },
  { do = "move", actor = "player", to = [10, 0] },
  { do = "say", line = "ask", on_choice = ["", "no"] },
  { do = "set_flag", flag = "yes" },
  { do = "end" },
  { do = "label", name = "no" },
  { do = "set_flag", flag = "no" },
]
[cutscenes.c.tracks.pan]
steps = [{ do = "camera", zoom = 2.0, frames = 30 }]
"#
            .to_vec(),
        );
        ExpansionPack::from_files(&files).unwrap()
    }

    #[test]
    fn the_timeline_lists_each_step_on_its_frame_and_follows_the_picks() {
        let p = pack();
        let v = preview(&p, "c", &[], 10_000).unwrap();
        assert!(v.ended);
        assert_eq!(v.flags, vec!["yes".to_string()]);
        let first = &v.rows[0];
        assert_eq!((first.frame, first.kind), (0, "start_track"));
        let pan = v.rows.iter().find(|r| r.track == "pan").unwrap();
        assert_eq!(pan.frame, 0, "the track starts the frame it is started");
        assert!(pan.text.contains("zoom x2"), "{}", pan.text);
        // From the world origin to the centre of field tile (10, 0),
        // 1345.5 units at the walk pace: 169 frames.
        let say = v.rows.iter().find(|r| r.kind == "say").unwrap();
        assert_eq!(say.frame, 169);
        let v = preview(&p, "c", &[1], 10_000).unwrap();
        assert_eq!(v.flags, vec!["no".to_string()]);
        assert!(preview(&p, "missing", &[], 10).is_none());
    }
}
