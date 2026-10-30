//! Boss Rush: the story bosses back to back, with an intermission between
//! fights and a clock over the whole run.
//!
//! A stage is a scripted battle row of a retail scene (the row the scene's
//! field-VM `3E FF <row>` engages), so a stage fights exactly the formation,
//! staging and music the story fight has. Before each stage the party is
//! raised to the stage's level through the ordinary EXP path, so it arrives
//! at the strength the story expects and grows from there. An expansion pack
//! can append its own bosses ([`StageRef::Pack`]).
//!
//! The run itself ([`BossRushRun`]) is a small state machine the session
//! drives: enter a stage's scene, fight, then the intermission
//! ([`Intermission`]) until the player continues. The record
//! ([`BossRushRecord`]) rides the save.

use serde::{Deserialize, Serialize};

/// One retail story boss a stage fights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BossStage {
    /// The boss as the intermission names it.
    pub name: &'static str,
    /// The scene whose formation table holds the fight.
    pub scene: &'static str,
    /// The scripted battle row in that scene's table.
    pub row: u8,
    /// A 1-based monster id the row's formation seats: what the disc check
    /// pins the row by.
    pub monster_id: u16,
    /// The level the party is raised to before the fight.
    pub level: u8,
}

/// The retail story bosses in story order. Each row is a scene-local
/// formation row the story's own scripted battle engages (sources:
/// `docs/subsystems/battle-round-loop.md`, `docs/reference/glitches/`); the
/// disc-gated `boss_rush_rows_seat_their_bosses` test pins each against the
/// scene's formation table.
pub const RETAIL_STAGES: &[BossStage] = &[
    BossStage {
        name: "Gimard",
        scene: "town0b",
        row: 2,
        monster_id: 10,
        level: 4,
    },
    BossStage {
        name: "Caruban",
        scene: "rikuroa",
        row: 17,
        monster_id: 73,
        level: 7,
    },
    BossStage {
        name: "Songi",
        scene: "garmel",
        row: 8,
        monster_id: 76,
        level: 11,
    },
    BossStage {
        name: "Zeto",
        scene: "garmel",
        row: 9,
        monster_id: 75,
        level: 12,
    },
    BossStage {
        name: "Xain",
        scene: "tunnelc",
        row: 10,
        monster_id: 139,
        level: 19,
    },
    BossStage {
        name: "Dohati",
        scene: "dohaty",
        row: 10,
        monster_id: 138,
        level: 24,
    },
    BossStage {
        name: "Koru",
        scene: "nilboa",
        row: 32,
        monster_id: 182,
        level: 28,
    },
    BossStage {
        name: "Cort",
        scene: "chitei2",
        row: 13,
        monster_id: 180,
        level: 35,
    },
];

/// One stage of a run: a retail boss by index into [`RETAIL_STAGES`], or a
/// pack boss by its key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StageRef {
    Retail { index: usize },
    Pack { boss: String, level: u8 },
}

impl StageRef {
    /// The retail stage this names, if it is one.
    pub fn retail(&self) -> Option<&'static BossStage> {
        match self {
            StageRef::Retail { index } => RETAIL_STAGES.get(*index),
            StageRef::Pack { .. } => None,
        }
    }

    /// The stage's display name.
    pub fn name(&self) -> String {
        match self {
            StageRef::Retail { index } => RETAIL_STAGES
                .get(*index)
                .map_or("?", |s| s.name)
                .to_string(),
            StageRef::Pack { boss, .. } => boss.clone(),
        }
    }

    /// The level the party is raised to before the stage.
    pub fn level(&self) -> u8 {
        match self {
            StageRef::Retail { index } => RETAIL_STAGES.get(*index).map_or(1, |s| s.level),
            StageRef::Pack { level, .. } => *level,
        }
    }
}

/// The gold each won stage pays into the purse, on top of the fight's own
/// spoils, so the intermission shop has something to spend.
pub const STAGE_PURSE: i32 = 1500;

/// The supplies the intermission sells, by item name (resolved against the
/// disc's item table) and price.
pub const SUPPLIES: &[(&str, u32)] = &[
    ("Healing Leaf", 100),
    ("Healing Flower", 800),
    ("Magic Leaf", 1000),
    ("Phoenix", 1500),
];

/// Where a run is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    /// The stage's scene is to be entered and its fight armed.
    Enter,
    /// The stage's fight is on.
    Fight,
    /// Between fights.
    Intermission,
    /// Over: every stage won, or the party fell.
    Finished { cleared: bool },
}

/// One Boss Rush run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BossRushRun {
    pub stages: Vec<StageRef>,
    /// The stage being played (or next to play).
    pub stage: usize,
    pub phase: Phase,
    /// Frames on the clock: counted while a stage is entered or fought, not
    /// while the intermission waits.
    pub frames: u64,
}

impl BossRushRun {
    /// A run over every retail stage, then `pack` (pack boss key, level).
    pub fn new(pack: &[(String, u8)]) -> Self {
        let mut stages: Vec<StageRef> = (0..RETAIL_STAGES.len())
            .map(|index| StageRef::Retail { index })
            .collect();
        stages.extend(pack.iter().map(|(boss, level)| StageRef::Pack {
            boss: boss.clone(),
            level: *level,
        }));
        Self {
            stages,
            stage: 0,
            phase: Phase::Enter,
            frames: 0,
        }
    }

    /// The stage being played.
    pub fn current(&self) -> Option<&StageRef> {
        self.stages.get(self.stage)
    }

    /// One frame of the clock.
    pub fn tick(&mut self) {
        if matches!(self.phase, Phase::Enter | Phase::Fight) {
            self.frames = self.frames.saturating_add(1);
        }
    }

    /// The stage's fight is armed.
    pub fn fight_started(&mut self) {
        if self.phase == Phase::Enter {
            self.phase = Phase::Fight;
        }
    }

    /// The stage's fight ended: `won` moves to the intermission (or the
    /// finish after the last stage), a loss finishes the run.
    pub fn fight_ended(&mut self, won: bool) {
        if self.phase != Phase::Fight {
            return;
        }
        self.phase = if !won {
            Phase::Finished { cleared: false }
        } else if self.stage + 1 >= self.stages.len() {
            self.stage += 1;
            Phase::Finished { cleared: true }
        } else {
            self.stage += 1;
            Phase::Intermission
        };
    }

    /// Leave the intermission for the next stage.
    pub fn continue_run(&mut self) {
        if self.phase == Phase::Intermission {
            self.phase = Phase::Enter;
        }
    }

    /// Give the run up from the intermission.
    pub fn give_up(&mut self) {
        if self.phase == Phase::Intermission {
            self.phase = Phase::Finished { cleared: false };
        }
    }

    /// Stages won so far.
    pub fn won(&self) -> usize {
        match self.phase {
            Phase::Finished { cleared: true } => self.stages.len(),
            _ => self.stage,
        }
    }
}

/// The clock as a player reads it: `m:ss` (`h:mm:ss` past an hour), at 60
/// frames a second.
pub fn clock_label(frames: u64) -> String {
    let s = frames / 60;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The Boss Rush leaderboard a save keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BossRushRecord {
    /// Runs started.
    pub runs: u32,
    /// Runs that won every stage.
    pub clears: u32,
    /// The most stages one run won.
    pub best_stages: u32,
    /// The fastest clear, in frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_clear_frames: Option<u64>,
}

impl BossRushRecord {
    /// Fold a finished run in.
    pub fn record(&mut self, run: &BossRushRun) {
        let Phase::Finished { cleared } = run.phase else {
            return;
        };
        self.runs = self.runs.saturating_add(1);
        self.best_stages = self.best_stages.max(run.won() as u32);
        if cleared {
            self.clears = self.clears.saturating_add(1);
            self.best_clear_frames = Some(
                self.best_clear_frames
                    .map_or(run.frames, |b| b.min(run.frames)),
            );
        }
    }
}

/// The intermission's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntermissionRow {
    /// On to the next stage.
    Next,
    /// Restore the party.
    Rest,
    /// Buy supply `i` of [`SUPPLIES`].
    Buy(usize),
    /// End the run.
    GiveUp,
}

/// The intermission screen: a cursor over [`Intermission::rows`] and the
/// line the last action left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Intermission {
    pub cursor: usize,
    pub message: Option<String>,
}

impl Intermission {
    /// The rows in display order.
    pub fn rows() -> Vec<IntermissionRow> {
        let mut rows = vec![IntermissionRow::Next, IntermissionRow::Rest];
        rows.extend((0..SUPPLIES.len()).map(IntermissionRow::Buy));
        rows.push(IntermissionRow::GiveUp);
        rows
    }

    /// A row as the screen labels it.
    pub fn row_label(row: IntermissionRow) -> String {
        match row {
            IntermissionRow::Next => "Next battle".into(),
            IntermissionRow::Rest => "Rest (restore HP / MP)".into(),
            IntermissionRow::Buy(i) => {
                let (name, price) = SUPPLIES[i];
                format!("Buy {name}  {price}G")
            }
            IntermissionRow::GiveUp => "Give up".into(),
        }
    }

    /// Move the cursor by `dir` rows, wrapping.
    pub fn move_cursor(&mut self, dir: i32) {
        let n = Self::rows().len() as i32;
        self.cursor = (self.cursor as i32 + dir).rem_euclid(n) as usize;
    }

    /// The row under the cursor.
    pub fn selected(&self) -> IntermissionRow {
        Self::rows()[self.cursor.min(Self::rows().len() - 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_walks_its_stages_and_records() {
        let mut run = BossRushRun::new(&[("pack:warden".into(), 40)]);
        assert_eq!(run.stages.len(), RETAIL_STAGES.len() + 1);
        assert_eq!(run.current().unwrap().name(), "Gimard");
        let mut rec = BossRushRecord::default();
        for i in 0..run.stages.len() {
            assert_eq!(run.phase, Phase::Enter);
            run.tick();
            run.fight_started();
            run.tick();
            run.fight_ended(true);
            if i + 1 < run.stages.len() {
                assert_eq!(run.phase, Phase::Intermission);
                run.tick(); // the intermission does not count
                run.continue_run();
            }
        }
        assert_eq!(run.phase, Phase::Finished { cleared: true });
        assert_eq!(run.frames, 2 * run.stages.len() as u64);
        rec.record(&run);
        assert_eq!((rec.runs, rec.clears), (1, 1));
        assert_eq!(rec.best_clear_frames, Some(run.frames));
        let mut lost = BossRushRun::new(&[]);
        lost.fight_started();
        lost.fight_ended(true);
        lost.continue_run();
        lost.fight_started();
        lost.fight_ended(false);
        assert_eq!(lost.phase, Phase::Finished { cleared: false });
        rec.record(&lost);
        assert_eq!((rec.runs, rec.clears, rec.best_stages), (2, 1, 9));
        assert_eq!(clock_label(60 * 75), "1:15");
        assert_eq!(clock_label(60 * 3725), "1:02:05");
    }

    #[test]
    fn the_intermission_cursor_wraps() {
        let mut m = Intermission::default();
        assert_eq!(m.selected(), IntermissionRow::Next);
        m.move_cursor(-1);
        assert_eq!(m.selected(), IntermissionRow::GiveUp);
        m.move_cursor(2);
        assert_eq!(m.selected(), IntermissionRow::Rest);
        assert!(Intermission::row_label(IntermissionRow::Buy(0)).contains("Healing Leaf"));
    }
}
