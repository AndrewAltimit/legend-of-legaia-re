//! Challenges: a rule set armed over one battle, the rule checks battle code
//! asks, and the score a clear earns.
//!
//! A challenge is **armed** ([`QuestRuntime::arm_challenge`]) and applies to
//! the next battle against its formation (or to the next battle at all when
//! it names none). While it applies, battle code asks the rule checks -
//! [`QuestRuntime::item_use_allowed`], [`QuestRuntime::magic_allowed`] - with
//! the battle's formation id, and counts rounds through
//! [`QuestRuntime::challenge_round_begun`]. The battle's end
//! ([`super::quest::QuestEvent::BattleEnded`]) clears or fails it and disarms
//! it. With nothing armed every check allows everything.

use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::quest::{ChallengeDef, QuestSection};

use crate::quest::{
    BattleOutcome, ChallengeFailure, QuestEvent, QuestNotice, QuestRuntime, QuestWorld,
};

/// The armed challenge.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveChallenge {
    /// The challenge id.
    pub id: String,
    /// The formation it applies to (`None` = the next battle).
    pub formation: Option<u16>,
    /// The rules, copied at arm time.
    pub rules: legaia_expansion_format::quest::ChallengeRules,
    /// Rounds begun in the battle it applies to.
    pub rounds: u16,
}

impl ActiveChallenge {
    /// Whether the challenge applies to a battle against `formation`.
    pub fn applies_to(&self, formation: Option<u16>) -> bool {
        match (self.formation, formation) {
            (None, _) => formation.is_some(),
            (Some(want), Some(got)) => want == got,
            (Some(_), None) => false,
        }
    }
}

/// Why a challenge would not arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmRefusal {
    /// The pack has no challenge with that id.
    Unknown,
    /// The challenge names a pack formation that has no runtime id.
    UnknownFormation,
    /// The party breaks the solo rule.
    Solo,
    /// A party member is above the level cap.
    LevelCap,
}

/// Whether `party` (`(roster slot, level)`) obeys `def`'s party rules.
fn party_obeys(
    def: &legaia_expansion_format::quest::ChallengeRules,
    party: &[(u8, u8)],
) -> Option<ArmRefusal> {
    if let Some(slot) = def.solo
        && (party.len() != 1 || party[0].0 != slot)
    {
        return Some(ArmRefusal::Solo);
    }
    if let Some(cap) = def.level_cap
        && party.iter().any(|&(_, l)| l > cap)
    {
        return Some(ArmRefusal::LevelCap);
    }
    None
}

/// The score a clear earns (see [`legaia_expansion_format::quest::ScoreRule`]).
pub fn challenge_score(def: &ChallengeDef, rounds: u16, party_level: u8) -> u32 {
    let s = &def.score;
    let mut score = s.base;
    if let Some(limit) = def.rules.turn_limit {
        score = score.saturating_add(
            s.turn_bonus
                .saturating_mul(u32::from(limit.saturating_sub(rounds))),
        );
    }
    if let Some(cap) = def.rules.level_cap {
        score = score.saturating_add(
            s.level_bonus
                .saturating_mul(u32::from(cap.saturating_sub(party_level))),
        );
    }
    score
}

/// A challenge's saved record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChallengeRecord {
    /// Best score.
    pub best: u32,
    /// Times cleared.
    pub clears: u32,
}

impl QuestRuntime {
    /// Arm challenge `id` for its next battle. Refused when the present party
    /// already breaks its party rules.
    pub fn arm_challenge(
        &mut self,
        pack: &ExpansionPack,
        world: &dyn QuestWorld,
        id: &str,
    ) -> Result<(), ArmRefusal> {
        self.sync_pack(pack);
        let def = pack.quests.challenge(id).ok_or(ArmRefusal::Unknown)?;
        if let Some(r) = party_obeys(&def.rules, &world.party()) {
            return Err(r);
        }
        let formation = match &def.formation {
            None => None,
            Some(r) => Some(
                crate::quest::resolve_formation(r, world).ok_or(ArmRefusal::UnknownFormation)?,
            ),
        };
        self.challenge = Some(ActiveChallenge {
            id: def.id.clone(),
            formation,
            rules: def.rules.clone(),
            rounds: 0,
        });
        self.notices.push(QuestNotice::ChallengeArmed {
            challenge: def.id.clone(),
        });
        Ok(())
    }

    /// Disarm the challenge without an outcome.
    pub fn disarm_challenge(&mut self) {
        self.challenge = None;
    }

    /// The challenge that applies to a battle against `formation`, if any.
    pub fn challenge_for(&self, formation: Option<u16>) -> Option<&ActiveChallenge> {
        self.challenge.as_ref().filter(|c| c.applies_to(formation))
    }

    /// Whether the battle Item command is allowed in a battle against
    /// `formation`.
    pub fn item_use_allowed(&self, formation: Option<u16>) -> bool {
        self.challenge_for(formation)
            .is_none_or(|c| !c.rules.no_items)
    }

    /// Whether the battle Magic command is allowed in a battle against
    /// `formation`.
    pub fn magic_allowed(&self, formation: Option<u16>) -> bool {
        self.challenge_for(formation)
            .is_none_or(|c| !c.rules.no_magic)
    }

    /// The enemy scale the challenge sets for a battle against `formation`,
    /// per mille (`1000` with none).
    pub fn enemy_scale_permille(&self, formation: Option<u16>) -> u16 {
        self.challenge_for(formation)
            .and_then(|c| c.rules.enemy_scale_permille)
            .unwrap_or(1000)
    }

    /// Count one round begun in a battle against `formation`.
    pub fn challenge_round_begun(&mut self, formation: Option<u16>) {
        if let Some(c) = self.challenge.as_mut().filter(|c| c.applies_to(formation)) {
            c.rounds = c.rounds.saturating_add(1);
        }
    }

    /// Resolve the armed challenge on a battle's end.
    pub(crate) fn resolve_challenge(
        &mut self,
        section: &QuestSection,
        world: &dyn QuestWorld,
        event: &QuestEvent,
    ) {
        let QuestEvent::BattleEnded {
            outcome,
            formation_id,
            ..
        } = event
        else {
            return;
        };
        if !self
            .challenge
            .as_ref()
            .is_some_and(|c| c.applies_to(Some(*formation_id)))
        {
            return;
        }
        let c = self.challenge.take().expect("checked");
        let Some(def) = section.challenge(&c.id) else {
            return;
        };
        let failure = match outcome {
            BattleOutcome::Lost => Some(ChallengeFailure::Lost),
            BattleOutcome::Fled => Some(ChallengeFailure::Fled),
            BattleOutcome::Won if c.rules.turn_limit.is_some_and(|l| c.rounds > l) => {
                Some(ChallengeFailure::TurnLimit)
            }
            BattleOutcome::Won if party_obeys(&c.rules, &world.party()).is_some() => {
                Some(ChallengeFailure::PartyRule)
            }
            BattleOutcome::Won => None,
        };
        if let Some(reason) = failure {
            self.notices.push(QuestNotice::ChallengeFailed {
                challenge: c.id,
                reason,
            });
            return;
        }
        let score = challenge_score(def, c.rounds, world.party_level());
        let rec = self.progress.challenges.entry(c.id.clone()).or_default();
        let first_clear = rec.clears == 0;
        rec.clears = rec.clears.saturating_add(1);
        rec.best = rec.best.max(score);
        let best = rec.best;
        if first_clear {
            self.reward(&def.rewards);
        }
        self.notices.push(QuestNotice::ChallengeCleared {
            challenge: c.id,
            score,
            best,
            first_clear,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest::{Grant, StaticQuestWorld};
    use legaia_expansion_format::quest::{ChallengeRules, Rewards, ScoreRule};

    fn pack() -> ExpansionPack {
        let mut p = ExpansionPack::default();
        p.manifest.id = "t".into();
        p.quests.challenges.push(ChallengeDef {
            id: "c".into(),
            formation: Some(legaia_expansion_format::quest::FormationRef::Id(7)),
            rules: ChallengeRules {
                turn_limit: Some(5),
                no_items: true,
                level_cap: Some(30),
                ..Default::default()
            },
            score: ScoreRule {
                base: 1000,
                turn_bonus: 100,
                level_bonus: 10,
            },
            rewards: Rewards {
                gold: 50,
                ..Default::default()
            },
            ..Default::default()
        });
        p
    }

    fn won(formation_id: u16) -> QuestEvent {
        QuestEvent::BattleEnded {
            outcome: BattleOutcome::Won,
            formation_id,
            monsters: vec![],
            boss: false,
        }
    }

    fn world(level: u8) -> StaticQuestWorld {
        StaticQuestWorld {
            party: vec![(0, level)],
            ..Default::default()
        }
    }

    #[test]
    fn nothing_armed_allows_everything() {
        let rt = QuestRuntime::default();
        assert!(rt.item_use_allowed(Some(7)));
        assert!(rt.magic_allowed(None));
        assert_eq!(rt.enemy_scale_permille(Some(7)), 1000);
    }

    #[test]
    fn rules_apply_only_to_their_formation() {
        let p = pack();
        let mut rt = QuestRuntime::default();
        rt.arm_challenge(&p, &world(20), "c").unwrap();
        assert!(!rt.item_use_allowed(Some(7)));
        assert!(
            rt.item_use_allowed(Some(8)),
            "another battle is not the challenge"
        );
        assert!(rt.item_use_allowed(None), "no battle, no rule");
        assert!(rt.magic_allowed(Some(7)));
    }

    #[test]
    fn a_clear_scores_and_rewards_once() {
        let p = pack();
        let mut rt = QuestRuntime::default();
        rt.arm_challenge(&p, &world(20), "c").unwrap();
        for _ in 0..3 {
            rt.challenge_round_begun(Some(7));
        }
        rt.handle(&p, &world(20), won(7));
        // 1000 + 100 * (5 - 3) + 10 * (30 - 20)
        assert!(rt.notices.contains(&QuestNotice::ChallengeCleared {
            challenge: "c".into(),
            score: 1300,
            best: 1300,
            first_clear: true
        }));
        assert_eq!(rt.take_grants(), vec![Grant::Gold(50)]);
        assert!(rt.challenge.is_none(), "a clear disarms");

        rt.arm_challenge(&p, &world(20), "c").unwrap();
        rt.handle(&p, &world(20), won(7));
        assert!(
            rt.take_grants().is_empty(),
            "the reward is the first clear's"
        );
        assert_eq!(rt.progress.challenges["c"].best, 1600);
        assert_eq!(rt.progress.challenges["c"].clears, 2);
    }

    #[test]
    fn a_win_past_the_turn_limit_fails() {
        let p = pack();
        let mut rt = QuestRuntime::default();
        rt.arm_challenge(&p, &world(20), "c").unwrap();
        for _ in 0..6 {
            rt.challenge_round_begun(Some(7));
        }
        rt.handle(&p, &world(20), won(7));
        assert!(rt.notices.contains(&QuestNotice::ChallengeFailed {
            challenge: "c".into(),
            reason: ChallengeFailure::TurnLimit
        }));
        assert!(!rt.progress.challenges.contains_key("c"));
    }

    #[test]
    fn another_battle_leaves_the_challenge_armed() {
        let p = pack();
        let mut rt = QuestRuntime::default();
        rt.arm_challenge(&p, &world(20), "c").unwrap();
        rt.challenge_round_begun(Some(8));
        rt.handle(&p, &world(20), won(8));
        let c = rt.challenge.as_ref().expect("still armed");
        assert_eq!(c.rounds, 0, "another battle's rounds are not counted");
    }

    #[test]
    fn the_party_rules_refuse_the_arm() {
        let p = pack();
        let mut rt = QuestRuntime::default();
        assert_eq!(
            rt.arm_challenge(&p, &world(31), "c"),
            Err(ArmRefusal::LevelCap)
        );
        assert_eq!(
            rt.arm_challenge(&p, &world(1), "x"),
            Err(ArmRefusal::Unknown)
        );
        let mut solo = pack();
        solo.quests.challenges[0].rules.solo = Some(1);
        assert_eq!(
            rt.arm_challenge(&solo, &world(1), "c"),
            Err(ArmRefusal::Solo)
        );
    }

    #[test]
    fn a_pack_formation_resolves_at_arm_time() {
        let mut named = pack();
        named.quests.challenges[0].formation = Some(
            legaia_expansion_format::quest::FormationRef::Pack("ogres".into()),
        );
        let mut rt = QuestRuntime::default();
        assert_eq!(
            rt.arm_challenge(&named, &world(1), "c"),
            Err(ArmRefusal::UnknownFormation)
        );
        let mut w = world(1);
        w.pack_formations.insert("ogres".into(), 0xE001);
        rt.arm_challenge(&named, &w, "c").unwrap();
        assert!(!rt.item_use_allowed(Some(0xE001)));
        assert!(rt.item_use_allowed(Some(7)));
    }

    #[test]
    fn a_loss_fails() {
        let p = pack();
        let mut rt = QuestRuntime::default();
        rt.arm_challenge(&p, &world(20), "c").unwrap();
        rt.handle(
            &p,
            &world(20),
            QuestEvent::BattleEnded {
                outcome: BattleOutcome::Lost,
                formation_id: 7,
                monsters: vec![],
                boss: false,
            },
        );
        assert!(rt.notices.contains(&QuestNotice::ChallengeFailed {
            challenge: "c".into(),
            reason: ChallengeFailure::Lost
        }));
    }
}
