//! Expansion-pack quests, challenges and dialogue on the `World`: the
//! [`QuestWorld`] view the tracker reads, the event hooks the play paths
//! raise, the grants the tracker queues, and pack dialogue lines on the
//! inline dialogue runner.
//!
//! Every entry point is the identity with no pack active
//! (`World::expansion.pack == None`): no event is built, no rule refuses, no
//! line starts.
//!
//! Raised from the World's own paths: battle end ([`World::finish_battle`]),
//! scene entry ([`World::set_active_scene_label`]), round begin (the
//! challenge round count), the battle command ring (the challenge item /
//! magic refusals), and the world tick (the state re-test that covers level
//! ups and items gained, and the pack-line resolve). Raised by callers: an
//! NPC talk to a pack NPC ([`World::expansion_talked`]), a pack mode won
//! ([`World::expansion_notify`] with [`QuestEvent::ModeWon`]).

use super::*;
use legaia_engine_expansion::quest::{Grant, PackFight, QuestEvent, QuestNotice, QuestWorld};
use legaia_engine_expansion::quest_challenge::ArmRefusal;
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::dialog::{DialogError, compile_line};

/// The `World` as the quest tracker reads it.
pub(in crate::world) struct WorldQuestView<'a>(pub(in crate::world) &'a World);

impl QuestWorld for WorldQuestView<'_> {
    fn story_flag(&self, idx: u16) -> bool {
        self.0.system_flag_test(idx)
    }

    fn party(&self) -> Vec<(u8, u8)> {
        self.0
            .present_party_list()
            .into_iter()
            .map(|slot| {
                let level = self
                    .0
                    .party
                    .roster
                    .members
                    .get(usize::from(slot))
                    .map_or(0, |r| r.level());
                (slot, level)
            })
            .collect()
    }

    fn item_count(&self, item: &legaia_expansion_format::items::ItemRef) -> u32 {
        self.0.held_item_count(item)
    }

    fn play_seconds(&self) -> u32 {
        self.0.clock.play_time_seconds
    }

    /// The compiled battle table's id for the pack formation (`0xE000+`).
    fn pack_formation_id(&self, id: &str) -> Option<u16> {
        self.0.expansion_formation_id(id)
    }

    fn boss_cleared(&self, id: &str) -> bool {
        self.0.expansion_boss_cleared(id)
    }

    /// The compiled battle table's formation id for the pack boss.
    fn pack_boss_formation_id(&self, id: &str) -> Option<u16> {
        self.0
            .expansion
            .battle
            .compiled
            .as_ref()?
            .boss(id)
            .map(|b| b.formation_id)
    }
}

impl World {
    /// Feed `event` to the active pack's quest tracker, then apply what it
    /// granted and queue the lines it raised. No-op with no pack.
    pub fn expansion_notify(&mut self, event: QuestEvent) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        let seen = self.expansion.quests.notices.len();
        let mut rt = std::mem::take(&mut self.expansion.quests);
        rt.handle(&pack, &WorldQuestView(self), event);
        self.expansion.quests = rt;
        self.settle_expansion_outcome(seen);
    }

    /// The player talked to the pack NPC `npc`: starts the quests it gives
    /// and counts toward talk objectives.
    pub fn expansion_talked(&mut self, npc: &str) {
        self.expansion_notify(QuestEvent::Talked(npc.to_string()));
    }

    /// Start quest `id` from a script. `false` with no pack, an unknown id,
    /// or prerequisites that do not hold.
    pub fn expansion_start_quest(&mut self, id: &str) -> bool {
        let Some(pack) = self.expansion.pack.clone() else {
            return false;
        };
        let seen = self.expansion.quests.notices.len();
        let mut rt = std::mem::take(&mut self.expansion.quests);
        let started = rt.start_quest(&pack, &WorldQuestView(self), id);
        self.expansion.quests = rt;
        self.settle_expansion_outcome(seen);
        started
    }

    /// Arm challenge `id` for its next battle.
    pub fn expansion_arm_challenge(&mut self, id: &str) -> Result<(), ArmRefusal> {
        let Some(pack) = self.expansion.pack.clone() else {
            return Err(ArmRefusal::Unknown);
        };
        let seen = self.expansion.quests.notices.len();
        let mut rt = std::mem::take(&mut self.expansion.quests);
        let armed = rt.arm_challenge(&pack, &WorldQuestView(self), id);
        self.expansion.quests = rt;
        self.toast_expansion_notices(seen);
        armed
    }

    /// Arm challenge `id` (a map's `challenge` event, a dialogue choice's
    /// `challenge`) and, with `start`, start the fight against the
    /// formation it names. A refusal shows as a toast. `false` when the
    /// challenge did not arm or its fight did not start.
    pub fn expansion_take_challenge(&mut self, id: &str, start: bool) -> bool {
        use legaia_expansion_format::quest::FormationRef;
        if let Err(r) = self.expansion_arm_challenge(id) {
            let why = match r {
                ArmRefusal::Unknown => "no such challenge",
                ArmRefusal::UnknownFormation => "its battle is missing",
                ArmRefusal::Solo => "the party breaks its solo rule",
                ArmRefusal::LevelCap => "a member is over its level cap",
            };
            log::info!("expansion: challenge `{id}` refused: {why}");
            self.expansion
                .toasts
                .push(format!("Challenge refused: {why}"));
            return false;
        }
        if !start {
            return true;
        }
        let formation = self
            .expansion
            .pack
            .as_ref()
            .and_then(|p| p.quests.challenge(id))
            .and_then(|c| c.formation.clone());
        match formation {
            Some(FormationRef::Pack(f)) => self.start_expansion_battle(&f),
            Some(FormationRef::Id(f)) => self.force_encounter(f),
            None => true,
        }
    }

    /// The expansion block a save carries: the active pack's progress, else
    /// a dormant block a load kept, else `None` - always `None` for a session
    /// that never had a pack, so its saves stay byte-identical.
    pub fn expansion_save_block(&mut self) -> Option<Vec<u8>> {
        let pack = self.expansion.pack.clone();
        if let Some(p) = pack.as_deref() {
            // The battle runtime's once-only boss record rides in the block.
            self.expansion.quests.sync_pack(p);
            self.expansion.quests.progress.cleared_bosses =
                self.expansion.battle.cleared_bosses.clone();
        }
        self.expansion.quests.save_block(pack.as_deref())
    }

    /// Restore the expansion block of a loaded save (`None` = it carries
    /// none): progress for the active pack, else kept dormant until that
    /// pack is active.
    pub fn expansion_load_block(&mut self, block: Option<&[u8]>) {
        let pack = self.expansion.pack.clone();
        self.expansion.quests.load_block(pack.as_deref(), block);
        // A loaded save starts outside any cutscene.
        self.expansion.cutscene.clear();
        self.expansion.cutscene.hidden.clear();
        self.adopt_expansion_progress();
        self.expansion.toasts.clear();
        self.toast_dormant_pack();
    }

    /// Bring the quest runtime onto the active pack (adopting a dormant
    /// block saved for it) and hand the progress's once-only boss record to
    /// the battle runtime. Called on activation and after a save load.
    pub(in crate::world) fn adopt_expansion_progress(&mut self) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        self.expansion.quests.sync_pack(&pack);
        self.expansion.battle.cleared_bosses =
            self.expansion.quests.progress.cleared_bosses.clone();
        self.expansion_refresh_learning(false);
    }

    /// Take the quest and challenge notices queued since the last call, for
    /// the host to show.
    pub fn take_expansion_notices(&mut self) -> Vec<QuestNotice> {
        self.expansion.quests.take_notices()
    }

    /// The live battle's formation id, as the challenge checks key on.
    fn expansion_battle_formation(&self) -> Option<u16> {
        self.battle
            .active_formation
            .as_ref()
            .map(|f| f.formation_id)
    }

    /// Whether the battle Item command is open: refused only by an armed
    /// challenge's `no_items` rule over this battle.
    pub fn expansion_item_use_allowed(&self) -> bool {
        !self.expansion.is_active()
            || self
                .expansion
                .quests
                .item_use_allowed(self.expansion_battle_formation())
    }

    /// Whether the battle Magic command is open: refused only by an armed
    /// challenge's `no_magic` rule over this battle.
    pub fn expansion_magic_allowed(&self) -> bool {
        !self.expansion.is_active()
            || self
                .expansion
                .quests
                .magic_allowed(self.expansion_battle_formation())
    }

    /// A battle round began: count it toward the armed challenge.
    pub(in crate::world) fn expansion_round_begun(&mut self) {
        if self.expansion.is_active() {
            let formation = self.expansion_battle_formation();
            self.expansion.quests.challenge_round_begun(formation);
        }
    }

    /// The battle ending now: report its outcome. Called from
    /// [`World::finish_battle`] while the formation is still seated.
    pub(in crate::world) fn expansion_battle_ended(&mut self) {
        use legaia_engine_expansion::quest::BattleOutcome;
        if !self.expansion.is_active() {
            return;
        }
        let Some(f) = self.battle.active_formation.as_ref() else {
            return;
        };
        let outcome = if self.battle.escaped {
            BattleOutcome::Fled
        } else {
            match self.battle.end {
                Some(BattleEndCause::MonsterWipe) => BattleOutcome::Won,
                Some(BattleEndCause::PartyWipe) => BattleOutcome::Lost,
                Some(BattleEndCause::Escaped) => BattleOutcome::Fled,
                _ => return,
            }
        };
        let event = QuestEvent::BattleEnded {
            outcome,
            formation_id: f.formation_id,
            monsters: f.slots.iter().map(|s| s.monster_id).collect(),
            boss: f.header_flags != 0,
        };
        self.expansion_notify(event);
    }

    /// Show the pack dialogue line `line_id` through the inline dialogue
    /// runner. Replaces any running inline dialogue, as
    /// [`World::start_inline_dialogue`] does. When the talk ends, the chosen
    /// option's pack flags apply and its `next` line follows.
    pub fn expansion_say(&mut self, line_id: &str) -> Result<(), DialogError> {
        let Some(pack) = self.expansion.pack.clone() else {
            return Err(DialogError::Unknown(line_id.to_string()));
        };
        let line = pack
            .dialog
            .lines
            .get(line_id)
            .ok_or_else(|| DialogError::Unknown(line_id.to_string()))?;
        let bytes = std::sync::Arc::new(compile_line(line)?);
        self.expansion.quests.dialog.begin(line_id, bytes.clone());
        self.dialog.inline = Some(legaia_engine_field::inline_dialogue::InlineDialogue::new(
            bytes, 0,
        ));
        Ok(())
    }

    /// The inline runner committed menu option `choice`: note it if the run
    /// is a pack line's.
    pub(in crate::world) fn expansion_observe_choice(
        &mut self,
        bytes: &std::sync::Arc<Vec<u8>>,
        choice: usize,
    ) {
        if self.expansion.is_active() {
            self.expansion.quests.dialog.observe_choice(bytes, choice);
        }
    }

    /// The per-tick expansion step: resolve a finished pack line, start a
    /// queued one, and re-test the state objectives outside battle.
    pub(in crate::world) fn tick_expansion(&mut self) {
        // Toasts count down whether or not a pack is active: the notice that
        // a loaded save names a pack that is not loaded shows without one.
        let frames = self.clock.display_frame_step.max(1);
        self.expansion.toasts.tick(frames);
        if !self.expansion.is_active() {
            return;
        }
        // A table in the world holds its card toasts (a lent starter, a
        // stake) until the game is over: they would lie over its HUD.
        if !self.table_stage_up() {
            self.expansion.cards.tick_toasts();
        }
        self.tick_expansion_audio();
        self.resolve_expansion_line();
        if !matches!(self.mode, SceneMode::Battle) {
            self.expansion_notify(QuestEvent::Refresh);
        }
        if self.expansion.quests.dialog.pending.is_none()
            && !self.dialogue_owns_input()
            && !self.expansion.quests.dialog.queued.is_empty()
        {
            let next = self.expansion.quests.dialog.queued.remove(0);
            if let Err(e) = self.expansion_say(&next) {
                log::warn!("expansion line {next}: {e:?}");
            }
        }
        // The cutscene in play (or the next queued one): after the line
        // resolve above, so a `say` step sees its line's choice the tick the
        // box closes.
        self.tick_expansion_cutscene();
        // The Journal's Cards page: the card menu opens over the field once
        // the pause menu has closed.
        if self.expansion.open_cards
            && !self.dialogue_owns_input()
            && matches!(self.mode, SceneMode::Field | SceneMode::WorldMap)
        {
            self.expansion.open_cards = false;
            if !self.open_card_menu() {
                log::warn!("expansion: the card menu did not open");
            }
        }
        // The Journal's Items page: the item screen, the same way.
        if let Some(gear) = self.expansion.open_items
            && !self.dialogue_owns_input()
            && matches!(self.mode, SceneMode::Field | SceneMode::WorldMap)
        {
            self.expansion.open_items = None;
            if !self.open_item_bag(gear) {
                log::warn!("expansion: the item screen did not open");
            }
        }
        // A queued fight starts once its lines are done and the player walks.
        if !self.expansion.quests.fights.is_empty()
            && !self.expansion.cutscene.active()
            && self.expansion.quests.dialog.pending.is_none()
            && self.expansion.quests.dialog.queued.is_empty()
            && !self.dialogue_owns_input()
            && matches!(self.mode, SceneMode::Field | SceneMode::WorldMap)
        {
            let fight = self.expansion.quests.fights.remove(0);
            let started = match &fight {
                PackFight::Formation(f) => self.start_expansion_battle(f),
                PackFight::Boss(b) => self.start_expansion_boss(b),
                PackFight::Mode(m) => self
                    .enter_pack_mode(m)
                    .map_err(|e| log::warn!("expansion: mode `{m}`: {e:?}"))
                    .is_ok(),
            };
            if !started {
                log::warn!("expansion: {fight:?} did not start");
            }
        }
    }

    /// A pack line whose run has ended (finished, cleared, or replaced by
    /// another talk) applies its choice and chains its `next`.
    fn resolve_expansion_line(&mut self) {
        let Some(pending) = self.expansion.quests.dialog.pending.as_ref() else {
            return;
        };
        let running = self
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| !id.done && std::sync::Arc::ptr_eq(&id.bytecode, &pending.bytes));
        if running {
            return;
        }
        let pending = self
            .expansion
            .quests
            .dialog
            .pending
            .take()
            .expect("checked");
        // The cutscene that said the line branches on its choice.
        if self.expansion.cutscene.said.as_deref() == Some(pending.line.as_str()) {
            self.expansion.cutscene.last_choice = pending.choice;
        }
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        let Some(choice) = pending
            .choice
            .and_then(|k| pack.dialog.lines.get(&pending.line)?.choices.get(k))
        else {
            return;
        };
        for f in &choice.set_flags {
            self.expansion_notify(QuestEvent::FlagSet(f.clone()));
        }
        for f in &choice.clear_flags {
            self.expansion_notify(QuestEvent::FlagCleared(f.clone()));
        }
        if let Some(next) = &choice.next {
            self.expansion.quests.dialog.queued.insert(0, next.clone());
        }
        // Armed before the option's fight queues, so the challenge applies
        // to it.
        if let Some(c) = &choice.challenge {
            self.expansion_take_challenge(c, false);
        }
        if let Some(m) = &choice.mode {
            self.expansion
                .quests
                .fights
                .push(PackFight::Mode(m.clone()));
        }
        if let Some(f) = &choice.battle {
            self.expansion
                .quests
                .fights
                .push(PackFight::Formation(f.clone()));
        }
        if let Some(b) = &choice.boss {
            self.expansion
                .quests
                .fights
                .push(PackFight::Boss(b.clone()));
        }
    }

    /// Apply the tracker's queued grants and queue the lines named by the
    /// notices past index `seen` (the ones this call raised).
    fn settle_expansion_outcome(&mut self, seen: usize) {
        for g in self.expansion.quests.take_grants() {
            match g {
                Grant::Gold(n) => {
                    self.party.money =
                        (i64::from(self.party.money) + i64::from(n)).clamp(0, 9_999_999) as i32;
                }
                Grant::Xp(n) => self.grant_expansion_xp(n),
                Grant::Item { item, count } => {
                    self.grant_item_ref(&item, count);
                }
                Grant::TakeItem { item, count } => self.take_item_ref(&item, count),
                Grant::Card(id) => self.expansion.quests.card_inbox.push(id),
                Grant::Fight(f) => self.expansion.quests.fights.push(f),
            }
        }
        self.collect_card_grants();
        self.toast_expansion_notices(seen);
        self.expansion_refresh_learning(true);
        // A quest's start / completion cutscene plays after its line.
        if let Some(pack) = self.expansion.pack.clone() {
            let moments: Vec<String> = self
                .expansion
                .quests
                .notices
                .iter()
                .skip(seen)
                .filter_map(|n| {
                    let (quest, start) = match n {
                        QuestNotice::Started { quest, .. } => (quest, true),
                        QuestNotice::Completed { quest, .. } => (quest, false),
                        _ => return None,
                    };
                    let q = pack.quests.quests.iter().find(|q| q.id == *quest)?;
                    if start {
                        q.start_cutscene.clone()
                    } else {
                        q.complete_cutscene.clone()
                    }
                })
                .collect();
            for c in moments {
                self.expansion_play_cutscene(&c);
            }
        }
        for n in self.expansion.quests.notices.iter().skip(seen) {
            if let QuestNotice::Started {
                dialog: Some(line), ..
            }
            | QuestNotice::Completed {
                dialog: Some(line), ..
            }
            | QuestNotice::Reminder { dialog: line, .. } = n
            {
                self.expansion.quests.dialog.queued.push(line.clone());
            }
        }
    }

    /// `xp` EXP to every present party member through the battle EXP path's
    /// per-member credit ([`World::credit_character_xp`]), without the
    /// battle's 3/4 scale and living-member split: a quest reward is the
    /// figure the pack names, per member.
    fn grant_expansion_xp(&mut self, xp: u32) {
        for slot in self.present_party_list() {
            self.credit_character_xp(slot, xp);
        }
    }
}
