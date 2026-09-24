//! The battle **commit log** - the rows retail keeps on screen through the
//! whole command phase, one per member who has committed a command.
//!
//! Each committed member owns three consecutive records of the screen-element
//! placement table at `0x80076C10` - `0x2B + 3n` (name), `0x2C + 3n`
//! (command), `0x2D + 3n` (target), with `n = ctx[+0x1F]`, the command
//! cursor's step depth ([`crate::battle_cursor_pose::ActorCursor::depth`]).
//! The command-window controller `FUN_801D388C` stages them on every commit:
//!
//! 1. it **lands** the three elements with `FUN_801D5718` from the surfaces
//!    the player was just looking at - the acting plaque (record `0x1A`), the
//!    command chip that was chosen (record `0x0D` `Attack` on the attack arm
//!    `0x20`, record `0x0B` `Spirit` on the Spirit arms `0x11` / `0x23`) and
//!    the target plaque (record `0x29`, the Spirit arms write an empty string
//!    of width `0` instead);
//! 2. it lays the row out left to right: the name at `x = 16`
//!    (`sh 0x10, +0x0A`), the command at `name_w + 0x20` and the target at
//!    `name_w + 0x60` (`0x801D4488..0x801D44D4`, again at
//!    `0x801D49A4..0x801D49C8` and `0x801D408C..0x801D40B0`);
//! 3. it scrolls the log so the newest row sits lowest: one row rests at
//!    `y = 170` (`0xAA`); a second commit moves the first to `146` (`0x92`)
//!    and seats itself at `170`; a third seats at `194` (`0xC2`).
//!
//! The target plaque's own content is swapped by `FUN_801D57E8` when the
//! target cursor covers a whole side: record `0x29` adopts record `0x3D`
//! (`"  All"`, width 36) or `0x3E` (`"All Allies"`, width 48) at `0x801D4414`
//! / `0x801D4434`, so an all-target commit logs that label.
//!
//! What the port models is the **resting** seat of every element (seat B,
//! `+0x0A` / `+0x0C`). Retail spawns each element at seat A and glides it to
//! seat B (`FUN_801D8DE8` / `FUN_801DB7B0`); the glide's rate is not pinned,
//! so the log draws where each element comes to rest.
//!
//! Evidence: the arms are read from the disassembly of PROT 0898 at base
//! `0x801CE818`; the one-row geometry is capture-confirmed -
//! `party_basic_attack_vs_gobu_gobu` (solo Vahn at `0x6E`, `ctx[+0x1F] = 1`)
//! holds records `0x2B` / `0x2C` / `0x2D` at seat B `(16, 170)` / `(59, 170)`
//! / `(123, 170)` for `Vahn` (width 27) / `Attack` / `Gobu Gobu`.
//!
//! REF: FUN_801D388C (cases `0x11`, `0x20`, `0x23` - the commit arms)

use crate::battle_cursor_pose::{ElementPlacement, element_placement_copy, element_placement_land};

/// First record of the log (`0x2B`, row 0's name element).
pub const LOG_FIRST_RECORD: usize = 0x2B;
/// Records per row: name, command, target.
pub const LOG_RECORDS_PER_ROW: usize = 3;
/// Rows the log can hold - one per party member.
pub const LOG_ROWS: usize = 3;
/// The acting plaque (the name source).
pub const RECORD_ACTING_PLAQUE: usize = 0x1A;
/// The `Spirit` command chip (the Spirit arms' command source).
pub const RECORD_CHIP_SPIRIT: usize = 0x0B;
/// The `Item` command chip.
pub const RECORD_CHIP_ITEM: usize = 0x0C;
/// The `Attack` command chip (the attack arm's command source).
pub const RECORD_CHIP_ATTACK: usize = 0x0D;
/// The Ra-Seru magic command chip.
pub const RECORD_CHIP_MAGIC: usize = 0x0E;
/// The target plaque (the target source).
pub const RECORD_TARGET_PLAQUE: usize = 0x29;
/// The whole-enemy-side target label (`"  All"`).
pub const RECORD_ALL_ENEMIES: usize = 0x3D;
/// The whole-party target label (`"All Allies"`).
pub const RECORD_ALL_ALLIES: usize = 0x3E;
/// Content width the disc ships in record [`RECORD_ALL_ENEMIES`].
pub const ALL_ENEMIES_WIDTH: u16 = 36;
/// Content width the disc ships in record [`RECORD_ALL_ALLIES`].
pub const ALL_ALLIES_WIDTH: u16 = 48;
/// Name column x (`sh 0x10`).
pub const NAME_X: u16 = 0x10;
/// Command column offset past the name's width.
pub const COMMAND_GAP: u16 = 0x20;
/// Target column offset past the name's width.
pub const TARGET_GAP: u16 = 0x60;
/// The row a lone entry rests on, and the second row once there are two.
pub const ROW_Y_LOWER: u16 = 0xAA;
/// The row the first entry scrolls up to once a second arrives.
pub const ROW_Y_UPPER: u16 = 0x92;
/// The third row.
pub const ROW_Y_THIRD: u16 = 0xC2;

/// What the target column of one row holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogTarget {
    /// One actor, whose name measures `width` (the target plaque's content).
    Single { width: u16 },
    /// The whole enemy side - record [`RECORD_ALL_ENEMIES`].
    AllEnemies,
    /// The whole party - record [`RECORD_ALL_ALLIES`].
    AllAllies,
    /// No target: the Spirit arms' empty string of width `0`.
    None,
}

/// One committed member, as the commit arm reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogCommit {
    /// Measured width of the member's name (`FUN_80035F04`).
    pub name_width: u16,
    /// Record the command element lands from ([`RECORD_CHIP_ATTACK`] etc).
    pub command_record: usize,
    /// Measured width of that chip's label.
    pub command_width: u16,
    pub target: LogTarget,
}

/// The resting seat of one log element: content box `(x, y)` and width.
/// `anim` carries the source record index the content came from, so a
/// caller can tell which string the element shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogElement {
    pub x: i16,
    pub y: i16,
    pub width: i16,
    /// The record whose content (`+0x14`) this element adopted.
    pub source: usize,
}

/// What one commit-log row's target column holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitLogTarget {
    /// One actor, by display name.
    Single(String),
    /// The whole enemy side (record [`RECORD_ALL_ENEMIES`]).
    AllEnemies,
    /// The whole party (record [`RECORD_ALL_ALLIES`]).
    AllAllies,
    /// No target column content (Spirit).
    None,
}

/// One row of the commit log as the engine hands it to the draw path: the
/// member's display name, the command chip's label and placement record, and
/// the target column. The layout ([`commit_log_layout`]) needs the rendered
/// widths, which only the drawer's font can measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitLogRow {
    pub name: String,
    pub command: String,
    /// Placement record of the command chip ([`RECORD_CHIP_ATTACK`] etc).
    pub command_record: usize,
    pub target: CommitLogTarget,
}

/// The whole-enemy-side label - record [`RECORD_ALL_ENEMIES`]'s content,
/// leading spaces included.
pub const ALL_ENEMIES_LABEL: &str = "  All";
/// The whole-party label - record [`RECORD_ALL_ALLIES`]'s content.
pub const ALL_ALLIES_LABEL: &str = "All Allies";

impl CommitLogRow {
    /// The target column's text (`""` for none).
    pub fn target_text(&self) -> &str {
        match &self.target {
            CommitLogTarget::Single(t) => t,
            CommitLogTarget::AllEnemies => ALL_ENEMIES_LABEL,
            CommitLogTarget::AllAllies => ALL_ALLIES_LABEL,
            CommitLogTarget::None => "",
        }
    }

    /// The row as the layout kernel reads it, given a width measure.
    pub fn commit(&self, width: impl Fn(&str) -> u16) -> LogCommit {
        LogCommit {
            name_width: width(&self.name),
            command_record: self.command_record,
            command_width: width(&self.command),
            target: match &self.target {
                CommitLogTarget::Single(t) => LogTarget::Single { width: width(t) },
                CommitLogTarget::AllEnemies => LogTarget::AllEnemies,
                CommitLogTarget::AllAllies => LogTarget::AllAllies,
                CommitLogTarget::None => LogTarget::None,
            },
        }
    }
}

/// Size of the scratch placement array the layout runs over - past the
/// highest record the commit arms touch.
const SLOTS: usize = RECORD_ALL_ALLIES + 1;

/// Stage one commit into the placement array - the body of `FUN_801D388C`'s
/// commit arms for row `n`.
///
/// PORT: FUN_801D388C (cases `0x11` / `0x20` / `0x23`: the log landing at `0x801D4444..0x801D4540`, `0x801D4918..0x801D49C8`, `0x801D4020..0x801D4124`)
pub fn stage_commit_row(slots: &mut [ElementPlacement], n: usize, commit: &LogCommit) {
    let r = LOG_FIRST_RECORD + LOG_RECORDS_PER_ROW * n;
    if r + 2 >= slots.len() {
        return;
    }
    slots[RECORD_ACTING_PLAQUE].f06 = commit.name_width;
    element_placement_land(slots, r, RECORD_ACTING_PLAQUE);
    if let Some(src) = slots.get_mut(commit.command_record) {
        src.f06 = commit.command_width;
    }
    element_placement_land(slots, r + 1, commit.command_record);
    match commit.target {
        LogTarget::Single { width } => {
            slots[RECORD_TARGET_PLAQUE].f06 = width;
            slots[RECORD_TARGET_PLAQUE].anim = RECORD_TARGET_PLAQUE as u32;
        }
        LogTarget::AllEnemies => {
            element_placement_copy(slots, RECORD_TARGET_PLAQUE, RECORD_ALL_ENEMIES);
        }
        LogTarget::AllAllies => {
            element_placement_copy(slots, RECORD_TARGET_PLAQUE, RECORD_ALL_ALLIES);
        }
        LogTarget::None => {}
    }
    if commit.target == LogTarget::None {
        // The Spirit arms point `+0x14` at an empty string and zero the width.
        slots[r + 2].anim = 0;
        slots[r + 2].f06 = 0;
    } else {
        element_placement_land(slots, r + 2, RECORD_TARGET_PLAQUE);
    }
    let name_w = slots[r].f06;
    slots[r].f0a = NAME_X;
    slots[r + 1].f0a = name_w.wrapping_add(COMMAND_GAP);
    slots[r + 2].f0a = name_w.wrapping_add(TARGET_GAP);
    let row = |slots: &mut [ElementPlacement], base: usize, y: u16| {
        for s in slots.iter_mut().skip(base).take(LOG_RECORDS_PER_ROW) {
            s.f0c = y;
        }
    };
    match n {
        0 => row(slots, r, ROW_Y_LOWER),
        1 => {
            row(slots, LOG_FIRST_RECORD, ROW_Y_UPPER);
            row(slots, r, ROW_Y_LOWER);
        }
        _ => {
            row(slots, LOG_FIRST_RECORD + LOG_RECORDS_PER_ROW, ROW_Y_LOWER);
            row(slots, r, ROW_Y_THIRD);
        }
    }
}

/// The log's resting layout after `commits` in order: one `[name, command,
/// target]` triple per row, row 0 first. A row whose target is
/// [`LogTarget::None`] still carries the (zero-width) element, as retail's
/// does.
pub fn commit_log_layout(commits: &[LogCommit]) -> Vec<[LogElement; 3]> {
    let mut slots = [ElementPlacement::default(); SLOTS];
    // The disc's preset widths and content on the two all-target labels.
    slots[RECORD_ALL_ENEMIES].f06 = ALL_ENEMIES_WIDTH;
    slots[RECORD_ALL_ENEMIES].anim = RECORD_ALL_ENEMIES as u32;
    slots[RECORD_ALL_ALLIES].f06 = ALL_ALLIES_WIDTH;
    slots[RECORD_ALL_ALLIES].anim = RECORD_ALL_ALLIES as u32;
    slots[RECORD_ACTING_PLAQUE].anim = RECORD_ACTING_PLAQUE as u32;
    let n = commits.len().min(LOG_ROWS);
    for (i, c) in commits.iter().take(n).enumerate() {
        if let Some(s) = slots.get_mut(c.command_record) {
            s.anim = c.command_record as u32;
        }
        stage_commit_row(&mut slots, i, c);
    }
    (0..n)
        .map(|i| {
            let r = LOG_FIRST_RECORD + LOG_RECORDS_PER_ROW * i;
            std::array::from_fn(|k| {
                let s = slots[r + k];
                LogElement {
                    x: s.f0a as i16,
                    y: s.f0c as i16,
                    width: s.f06 as i16,
                    source: s.anim as usize,
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attack(name_w: u16, target_w: u16) -> LogCommit {
        LogCommit {
            name_width: name_w,
            command_record: RECORD_CHIP_ATTACK,
            command_width: 48,
            target: LogTarget::Single { width: target_w },
        }
    }

    #[test]
    fn one_row_matches_the_gobu_gobu_capture() {
        // party_basic_attack_vs_gobu_gobu: Vahn (27) / Attack / Gobu Gobu (55).
        let rows = commit_log_layout(&[attack(27, 55)]);
        assert_eq!(rows.len(), 1);
        let [name, cmd, tgt] = rows[0];
        assert_eq!((name.x, name.y, name.width), (16, 170, 27));
        assert_eq!((cmd.x, cmd.y, cmd.width), (59, 170, 48));
        assert_eq!((tgt.x, tgt.y, tgt.width), (123, 170, 55));
        assert_eq!(name.source, RECORD_ACTING_PLAQUE);
        assert_eq!(cmd.source, RECORD_CHIP_ATTACK);
        assert_eq!(tgt.source, RECORD_TARGET_PLAQUE);
    }

    #[test]
    fn the_log_scrolls_up_as_rows_arrive() {
        let two = commit_log_layout(&[attack(27, 55), attack(24, 40)]);
        assert_eq!(two[0][0].y, 146);
        assert_eq!(two[1][0].y, 170);
        let three = commit_log_layout(&[attack(27, 55), attack(24, 40), attack(30, 40)]);
        let ys: Vec<i16> = three.iter().map(|r| r[1].y).collect();
        assert_eq!(ys, [146, 170, 194]);
        // Each row lays its columns off its own name width.
        assert_eq!(three[1][1].x, 24 + 32);
        assert_eq!(three[2][2].x, 30 + 96);
    }

    #[test]
    fn spirit_logs_a_blank_target_and_all_targets_log_the_disc_label() {
        let rows = commit_log_layout(&[
            LogCommit {
                name_width: 27,
                command_record: RECORD_CHIP_SPIRIT,
                command_width: 30,
                target: LogTarget::None,
            },
            LogCommit {
                target: LogTarget::AllEnemies,
                ..attack(24, 0)
            },
        ]);
        assert_eq!(rows[0][1].source, RECORD_CHIP_SPIRIT);
        assert_eq!((rows[0][2].width, rows[0][2].source), (0, 0));
        assert_eq!(rows[1][2].source, RECORD_ALL_ENEMIES);
        assert_eq!(rows[1][2].width, ALL_ENEMIES_WIDTH as i16);
    }
}
