//! Quest toasts: the short one-line notices both hosts show when a quest
//! starts, moves or completes, or a challenge resolves.
//!
//! The `World` turns each [`QuestNotice`] its quest tracker raises into a
//! line ([`notice_text`]) and queues it on a [`ToastQueue`]; the queue shows
//! one line at a time for [`TOAST_FRAMES`] frames and the draw layer reads
//! [`ToastQueue::current`]. Nothing here reads the pad: a toast never takes
//! input from the game.

use std::collections::VecDeque;

use legaia_expansion_format::ExpansionPack;

use crate::quest::{ChallengeFailure, QuestNotice};

/// How long one toast stays up, in 60 Hz frames.
pub const TOAST_FRAMES: u16 = 180;

/// How many toasts may wait behind the one on screen; older ones are dropped
/// first when a burst of notices outruns the display.
pub const TOAST_BACKLOG: usize = 6;

/// The line a host shows for `notice`, or `None` for a notice that has no
/// toast (a reminder, which shows its own dialogue line).
pub fn notice_text(pack: &ExpansionPack, notice: &QuestNotice) -> Option<String> {
    let quest_title = |id: &str| {
        pack.quests
            .quest(id)
            .map(|q| q.title.as_str())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(id)
            .to_string()
    };
    let challenge_title = |id: &str| {
        pack.quests
            .challenge(id)
            .map(|c| c.title.as_str())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(id)
            .to_string()
    };
    Some(match notice {
        QuestNotice::Started { quest, .. } => format!("Quest started: {}", quest_title(quest)),
        QuestNotice::Updated {
            quest,
            objective,
            progress,
            target,
        } => {
            let total = pack.quests.quest(quest).map_or(0, |q| q.objectives.len());
            if *target > 1 {
                format!("{}: {progress}/{target}", quest_title(quest))
            } else {
                format!(
                    "{}: objective {}/{total} done",
                    quest_title(quest),
                    objective + 1
                )
            }
        }
        QuestNotice::Completed { quest, .. } => {
            format!("Quest complete: {}", quest_title(quest))
        }
        QuestNotice::Reminder { .. } => return None,
        QuestNotice::ChallengeArmed { challenge } => {
            format!("Challenge ready: {}", challenge_title(challenge))
        }
        QuestNotice::ChallengeCleared {
            challenge,
            score,
            best,
            first_clear,
        } => {
            let tail = if *first_clear {
                " - first clear!".to_string()
            } else if score >= best {
                " - new best!".to_string()
            } else {
                format!(" (best {best})")
            };
            format!(
                "Challenge cleared: {} {score}{tail}",
                challenge_title(challenge)
            )
        }
        QuestNotice::ChallengeFailed { challenge, reason } => {
            let why = match reason {
                ChallengeFailure::Lost => "lost",
                ChallengeFailure::Fled => "fled",
                ChallengeFailure::TurnLimit => "over the turn limit",
                ChallengeFailure::PartyRule => "party rule broken",
            };
            format!("Challenge failed: {} ({why})", challenge_title(challenge))
        }
    })
}

/// The toast on screen and the ones waiting behind it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToastQueue {
    current: Option<(String, u16)>,
    waiting: VecDeque<String>,
}

impl ToastQueue {
    /// Queue `text` behind whatever is showing.
    pub fn push(&mut self, text: String) {
        if self.current.is_none() {
            self.current = Some((text, TOAST_FRAMES));
            return;
        }
        if self.waiting.len() == TOAST_BACKLOG {
            self.waiting.pop_front();
        }
        self.waiting.push_back(text);
    }

    /// Advance `frames` display frames: the showing toast counts down and the
    /// next one takes its place when it runs out.
    pub fn tick(&mut self, frames: u16) {
        let Some((_, left)) = self.current.as_mut() else {
            return;
        };
        *left = left.saturating_sub(frames.max(1));
        if *left == 0 {
            self.current = self.waiting.pop_front().map(|t| (t, TOAST_FRAMES));
        }
    }

    /// The line on screen, if any.
    pub fn current(&self) -> Option<&str> {
        self.current.as_ref().map(|(t, _)| t.as_str())
    }

    /// Drop every toast (a pack deactivated, a save loaded).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_read_as_lines() {
        let p = ExpansionPack {
            quests: toml::from_str(
                r#"
[[quests]]
id = "q"
title = "The Tower"
objectives = [{ kind = "win_battles", count = 3 }, { kind = "reach_level", level = 4 }]
"#,
            )
            .expect("parses"),
            ..Default::default()
        };
        let text = |n: QuestNotice| notice_text(&p, &n);
        assert_eq!(
            text(QuestNotice::Started {
                quest: "q".into(),
                dialog: None
            })
            .as_deref(),
            Some("Quest started: The Tower")
        );
        assert_eq!(
            text(QuestNotice::Updated {
                quest: "q".into(),
                objective: 0,
                progress: 2,
                target: 3
            })
            .as_deref(),
            Some("The Tower: 2/3")
        );
        assert_eq!(
            text(QuestNotice::Updated {
                quest: "q".into(),
                objective: 1,
                progress: 1,
                target: 1
            })
            .as_deref(),
            Some("The Tower: objective 2/2 done")
        );
        assert_eq!(
            text(QuestNotice::Completed {
                quest: "q".into(),
                dialog: None
            })
            .as_deref(),
            Some("Quest complete: The Tower")
        );
        assert_eq!(
            text(QuestNotice::Reminder {
                quest: "q".into(),
                dialog: "l".into()
            }),
            None
        );
    }

    #[test]
    fn the_queue_shows_one_line_at_a_time() {
        let mut q = ToastQueue::default();
        q.push("a".into());
        q.push("b".into());
        assert_eq!(q.current(), Some("a"));
        q.tick(TOAST_FRAMES - 1);
        assert_eq!(q.current(), Some("a"));
        q.tick(1);
        assert_eq!(q.current(), Some("b"));
        q.tick(TOAST_FRAMES);
        assert_eq!(q.current(), None);
        for i in 0..=TOAST_BACKLOG + 1 {
            q.push(i.to_string());
        }
        assert_eq!(q.waiting.len(), TOAST_BACKLOG);
    }
}
