//! The `World` side of what the hosts show and route for an expansion pack:
//! the retail-scene entrances into its maps, the Journal's content, the
//! pause menu's Travel request, the quest toasts, and the play events no
//! pack section raises itself (a retail NPC talked to, a pack mode won).
//!
//! Every entry point is the identity with no pack active: no entrance is
//! tested, no toast queues, no talk is reported, and the Journal is `None`.

use legaia_engine_expansion::entrance::EntranceHit;
use legaia_engine_expansion::journal::{JournalView, journal_view_with};
use legaia_engine_expansion::quest::QuestEvent;
use legaia_engine_expansion::quest_save::ExpansionProgress;
use legaia_engine_expansion::toast::notice_text;
use legaia_engine_mode::mode::SceneMode;

use super::World;
use super::expansion_quests::WorldQuestView;

impl World {
    /// The pack NPC id of retail placement `slot` in the active scene:
    /// `<scene>:<slot>`, e.g. `town01:12` - the id a pack's quests name to
    /// key a talk objective (or a giver) to a retail villager.
    pub fn retail_npc_id(&self, slot: u8) -> String {
        format!("{}:{slot}", self.active_scene_label)
    }

    /// The player engaged retail placement `slot` (the touch post's
    /// interaction): report it to the pack's quests as a talk to
    /// [`Self::retail_npc_id`]. The retail interaction runs unchanged; a
    /// pack line it raises waits for that dialogue to close. No-op with no
    /// pack, and on a pack map, whose placements are the base scene's.
    pub(in crate::world) fn expansion_retail_npc_talked(&mut self, slot: u8) {
        if !self.expansion.is_active() || self.expansion.map.is_some() {
            return;
        }
        let npc = self.retail_npc_id(slot);
        log::debug!("expansion: talked to retail NPC {npc}");
        self.expansion_talked(&npc);
    }

    /// The pack mode with id `mode` was won: counts toward `win_mode`
    /// objectives. The mode hosts call this when a mode session ends in a
    /// win. No-op with no pack.
    pub fn expansion_mode_won(&mut self, mode: &str) {
        self.expansion_notify(QuestEvent::ModeWon(mode.to_string()));
    }

    /// One field tick of the retail-scene entrances: the entrance the player
    /// just stepped onto, when it is open. The scene host enters its map.
    ///
    /// Tested only in a plain field scene the player walks freely - no pack
    /// map, no scene change parked, no dialogue or cutscene holding the
    /// player; any other tick forgets the last tile, so the first free tick
    /// after one only records where the player stands.
    pub fn step_expansion_entrance(&mut self) -> Option<EntranceHit> {
        let pack = self.expansion.pack.clone()?;
        let free = self.mode == SceneMode::Field
            && self.expansion.map.is_none()
            && self.scene_transition_hold.is_none()
            && self.pending_named_scene_transition.is_none()
            && !self.dialogue_owns_input()
            && !self.cutscene_timeline_active();
        if !free {
            self.expansion.entrances.reset();
            return None;
        }
        let tile = self
            .player_actor_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .and_then(|a| {
                let quant = |w: i16| i32::from(w) >> 7;
                let (x, z) = (quant(a.move_state.world_x), quant(a.move_state.world_z));
                ((0..=0x7F).contains(&x) && (0..=0x7F).contains(&z)).then_some((x as u8, z as u8))
            });
        let scene = self.active_scene_label.clone();
        let mut watch = std::mem::take(&mut self.expansion.entrances);
        let hit = {
            let view = WorldQuestView(self);
            let quests = &self.expansion.quests;
            watch.step(&pack, &scene, tile, |e| {
                quests.prerequisites_hold(&pack.quests, &e.requires, &view)
            })
        };
        self.expansion.entrances = watch;
        if let Some(h) = &hit {
            log::info!(
                "expansion: entrance '{}' in {scene} -> map '{}' spawn {:?}",
                h.id,
                h.map,
                h.spawn
            );
        }
        hit
    }

    /// Whether the gate `p` holds now (an entrance's, a map exit's, a
    /// trigger's or an NPC's `requires`). Open with no pack only when `p`
    /// is empty.
    pub fn expansion_prerequisites_hold(
        &self,
        p: &legaia_expansion_format::quest::Prerequisites,
    ) -> bool {
        if p.is_empty() {
            return true;
        }
        let Some(pack) = self.expansion.pack.as_ref() else {
            return false;
        };
        self.expansion
            .quests
            .prerequisites_hold(&pack.quests, p, &WorldQuestView(self))
    }

    /// A pack map's rest point (`kind = "rest"`): every roster member's HP
    /// and MP to full for `cost` gold ([`Self::revive_party_full`]), then
    /// with `save` the save screen, opened as a retail save point opens it
    /// (an op-`0x49` sub-op `1` menu press, [`Self::record_op49_park`],
    /// which a host answers by opening the pause menu on the card screen).
    /// A purse short of `cost` refuses with a toast and changes nothing.
    /// `false` with no pack or when refused.
    pub fn expansion_rest(&mut self, cost: u32, save: bool) -> bool {
        if !self.expansion.is_active() {
            return false;
        }
        if i64::from(self.party.money) < i64::from(cost) {
            self.expansion
                .toasts
                .push(format!("Resting here costs {cost} G."));
            return false;
        }
        self.party.money -= cost as i32;
        self.revive_party_full();
        self.expansion.toasts.push(if cost == 0 {
            "The party is rested.".to_string()
        } else {
            format!("Rested for {cost} G.")
        });
        if save {
            self.record_op49_park(1);
        }
        true
    }

    /// Ask the scene host to enter pack map `map_id` at `spawn` on its next
    /// tick (the pause menu's Travel page). `false`, changing nothing, with
    /// no pack or no such map.
    pub fn request_expansion_travel(&mut self, map_id: &str, spawn: &str) -> bool {
        let known = self
            .expansion
            .pack
            .as_ref()
            .is_some_and(|p| p.maps.contains_key(map_id));
        if known {
            self.expansion.travel = Some((map_id.to_string(), spawn.to_string()));
        }
        known
    }

    /// Take the Travel request [`Self::request_expansion_travel`] left.
    pub fn take_expansion_travel(&mut self) -> Option<(String, String)> {
        self.expansion.travel.take()
    }

    /// The Journal's content for the active pack; `None` with no pack.
    pub fn expansion_journal(&self) -> Option<JournalView> {
        let pack = self.expansion.pack.as_deref()?;
        let mut rt = self.expansion.quests.clone();
        rt.sync_pack(pack);
        Some(journal_view_with(
            pack,
            &rt,
            self.toggles.travel_all_maps,
            |p| self.expansion_prerequisites_hold(p),
        ))
    }

    /// The quest toast on screen, if any.
    pub fn expansion_toast(&self) -> Option<&str> {
        self.expansion.toasts.current()
    }

    /// The id of the pack a loaded save's expansion block belongs to when
    /// that pack is not the active one: its progress is kept dormant (and
    /// written back on the next save) but nothing of it plays.
    pub fn expansion_dormant_pack_id(&self) -> Option<String> {
        let block = self.expansion.quests.dormant.as_deref()?;
        let progress = ExpansionProgress::decode(block)?;
        let active = self.expansion.pack.as_ref().map(|p| p.manifest.id.as_str());
        (active != Some(progress.pack_id.as_str())).then_some(progress.pack_id)
    }

    /// A New Game: the active pack starts over with fresh progress (its
    /// once-only bosses, flags and quests cleared, its tables re-installed),
    /// and any progress a loaded save left dormant is dropped, so the new
    /// game's saves carry only what it earns. With no pack and no dormant
    /// block this changes nothing.
    pub(in crate::world) fn expansion_begin_new_game(&mut self) {
        let pack = self.expansion.pack.clone();
        self.deactivate_expansion();
        self.expansion = Default::default();
        if let Some(pack) = pack
            && let Err(problems) = self.activate_expansion(pack)
        {
            log::error!(
                "expansion: the pack does not re-activate for the New Game: {}",
                problems.join("; ")
            );
        }
    }

    /// Queue a toast for each quest notice past index `seen`.
    pub(in crate::world) fn toast_expansion_notices(&mut self, seen: usize) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        use legaia_engine_expansion::quest::QuestNotice;
        let fresh = &self.expansion.quests.notices[seen.min(self.expansion.quests.notices.len())..];
        // An objective that completed its quest in the same step says so
        // once, as the completion.
        let completed = |q: &str| {
            fresh
                .iter()
                .any(|n| matches!(n, QuestNotice::Completed { quest, .. } if quest == q))
        };
        let lines: Vec<String> = fresh
            .iter()
            .filter(|n| !matches!(n, QuestNotice::Updated { quest, .. } if completed(quest)))
            .filter_map(|n| notice_text(&pack, n))
            .collect();
        for line in lines {
            self.expansion.toasts.push(line);
        }
    }

    /// Saved progress was adopted under another version of the active pack,
    /// or lost entries the pack no longer defines
    /// ([`legaia_engine_expansion::save_compat`]): log every change, toast
    /// the one-line notice, and keep the report for
    /// [`Self::expansion_compat_report`].
    pub(in crate::world) fn toast_expansion_compat(&mut self) {
        let Some(report) = self.expansion.quests.compat.take() else {
            return;
        };
        if report.version_changed() {
            log::info!(
                "expansion: save made with {} {}, playing {}",
                report.pack_id,
                report.saved_version,
                report.pack_version
            );
        }
        for line in report.lines() {
            log::warn!("expansion: {}: {line}", report.pack_id);
        }
        if let Some(notice) = report.notice() {
            self.expansion.toasts.push(notice);
        }
        self.expansion.last_compat = Some(report);
    }

    /// What adopting the loaded save's progress for the active pack changed:
    /// the save's pack version, the active one, and every entry dropped.
    /// `None` when the save matched the pack.
    pub fn expansion_compat_report(
        &self,
    ) -> Option<&legaia_engine_expansion::save_compat::CompatReport> {
        self.expansion
            .pack
            .as_ref()
            .and(self.expansion.last_compat.as_ref())
    }

    /// The loaded save names a pack that is not active: say so once.
    pub(in crate::world) fn toast_dormant_pack(&mut self) {
        if let Some(id) = self.expansion_dormant_pack_id() {
            log::warn!("expansion: this save was made with pack '{id}', which is not loaded");
            self.expansion
                .toasts
                .push(format!("Saved with expansion '{id}' - not loaded"));
        }
    }
}
