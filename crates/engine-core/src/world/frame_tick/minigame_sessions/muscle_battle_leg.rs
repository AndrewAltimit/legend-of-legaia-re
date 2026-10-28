//! A Muscle Dome leg fought as the **ordinary battle** it is in retail.
//!
//! The arena's fight start `FUN_801D1510` (PROT 0977) does four things and
//! returns: plays the battle theme the arena init loaded (`jal 0x80026478`
//! on `0x8007056C`, `0x801D1548`), stores `_DAT_80084540 = 0x4C6` (the map
//! id of `other6`, whose `scene_tmd_stream` is the arena backdrop), stores
//! `DAT_8007BD60 = 0x80` with formation slots 1..3 cleared, and writes the
//! ladder rung's monster id into the formation cell `DAT_8007BD0C` with game
//! mode `0x14` (`0x801D1550..0x801D15BC`). Nothing else is dome-specific: the
//! leg runs on the shared round driver `FUN_801D0748` and action SM
//! `FUN_801E295C`, and the battle exit selector `FUN_80046A20` stores mode
//! `0x18` (the arena) instead of the field's `0x2` when the special-battle
//! word carries `0x100` (`0x80046E50`). So a leg here is
//! [`World::enter_battle_from_formation`] with
//! [`World::finish_battle`] routing back to the arena hub.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// `_DAT_80084540` as `FUN_801D1510` leaves it: the raw TOC define of the
/// arena data block `other6` (`li v0,0x4c6` at `0x801D1554`).
pub(crate) const ARENA_MAP_ID: u32 = 0x4C6;

/// The roster record retail seats in every leg: the hub's prologue stores
/// the present-party list `DAT_8007BD10 = [1, 0, 0]` every frame
/// (`0x801CF884..0x801CF8A0`, `FUN_801CF870`) - character id `1`, Vahn's
/// record `0x80084708` - so the arena offers no fighter pick.
pub(crate) const RETAIL_DOME_FIGHTER_RECORD: u8 = 0;

/// The flag every course arm of the `koin1` course menu (`P1[9]`) sets ahead
/// of its warp, beside the chosen course's own flag (`55 09` at `+0xB22` /
/// `+0xB4B` / `+0xB74`). What reads it is not pinned.
pub(crate) const COURSE_MENU_ENTRY_FLAG: u16 = 0x509;

impl World {
    /// Pick course `course` the way the `koin1` course menu does: each of its
    /// three arms (`P1[9]` `+0xB1D` / `+0xB46` / `+0xB6F`) debits the fee,
    /// sets [`COURSE_MENU_ENTRY_FLAG`], sets its own course flag and clears
    /// the other two (`55 36 / 65 37 / 65 38` for Beginner), then warps. So
    /// the arena init's flag test always finds exactly one course flag, and
    /// the special-battle word it seeds always carries the arena bit `0x100`,
    /// which is what lets a leg's battle be fled (the escape roll's forced
    /// arm, `FUN_801E791C` `0x801E7A0C`). A host entry that skips the menu
    /// (the native `M` key, the minigames page) calls this instead. The fee
    /// is the host's to charge.
    pub fn choose_muscle_course(&mut self, course: usize) {
        use legaia_engine_minigames::muscle_dome::COURSE_UNLOCK_FLAGS;
        self.system_flag_set(COURSE_MENU_ENTRY_FLAG);
        for (i, &(flag, _)) in COURSE_UNLOCK_FLAGS.iter().enumerate() {
            if i == course {
                self.system_flag_set(flag);
            } else {
                self.system_flag_clear(flag);
            }
        }
    }

    /// A host's own launch of a minigame (the native window's minigame
    /// keys, the browser play page's picker): the mode-24 warp the door
    /// requests, preceded for the arena by the course pick the `koin1`
    /// course menu would have made - Beginner, unless a pick already
    /// stands ([`Self::choose_muscle_course`]).
    pub fn request_host_minigame_warp(&mut self, sub_id: u8) {
        if sub_id == legaia_engine_mode::minigame_entry::MinigameSubId::MuscleDome.sub_id()
            && self.chosen_muscle_course().is_none()
        {
            self.choose_muscle_course(0);
        }
        self.request_minigame_warp(sub_id);
    }

    /// The course the menu last picked: the one course flag set, or `None`
    /// when no menu pick stands.
    pub fn chosen_muscle_course(&self) -> Option<usize> {
        use legaia_engine_minigames::muscle_dome::COURSE_UNLOCK_FLAGS;
        COURSE_UNLOCK_FLAGS
            .iter()
            .rposition(|&(flag, _)| self.system_flag_test(flag))
    }

    /// The roster record a leg seats: retail's Vahn
    /// ([`RETAIL_DOME_FIGHTER_RECORD`]) unless a host picked another
    /// ([`Self::set_muscle_fighter`]).
    pub fn muscle_fighter_record(&self) -> u8 {
        self.minigames.fighter_index() as u8
    }

    /// Seat roster record `record` (`0` Vahn, `1` Noa, `2` Gala) in every
    /// leg instead of retail's Vahn - a host option, not a retail one: the
    /// hub pins the party list to character `1`. A record the roster does not
    /// fill (no maximum HP) is refused and the pick left as it was.
    pub fn set_muscle_fighter(&mut self, record: u8) -> bool {
        let filled = self
            .party
            .roster
            .members
            .get(usize::from(record))
            .is_some_and(|r| r.hp_mp_sp().hp_max > 0);
        if filled {
            self.minigames.muscle_fighter =
                (record != RETAIL_DOME_FIGHTER_RECORD).then_some(record);
        }
        filled
    }

    /// Whether the current battle is a Muscle Dome leg.
    pub fn muscle_leg_in_battle(&self) -> bool {
        self.minigames.muscle_leg_in_battle
    }

    /// Open the leg's battle against `monster_id` - the arena's fight start.
    /// The dome session stays open beside it (the hub reads it as "a leg is
    /// open"); its own command flow is not ticked while the battle runs.
    ///
    /// PORT: FUN_801D1510 (the formation-cell, flags-byte and map-id stores)
    pub(in crate::world) fn start_muscle_battle_leg(&mut self, monster_id: u16) {
        use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
        let mut formation = FormationDef::new(0, vec![FormationSlot::new(monster_id)]);
        // `DAT_8007BD60 = 0x80` (`0x801D156C`): the per-battle flags byte the
        // battle init reads as the scripted-fight bit (`(x >> 5) & 4`).
        formation.header_flags = 1;
        self.field_return = Some(FieldReturnState {
            actors: self.actors.clone(),
            player_actor_slot: self.player_actor_slot,
            party_count: self.party.party_count,
        });
        // One fighter: the contest's lead record, alone.
        self.battle.battle_party_restore = Some(std::mem::replace(
            &mut self.party.active_party,
            vec![self.minigames.fighter_index() as u8],
        ));
        self.party.party_count = 1;
        self.battle.return_mode = SceneMode::MuscleDome;
        self.battle.special_word = self.dome_special_word();
        self.minigames.muscle_saved_map_id = Some(self.battle.map_id);
        self.battle.map_id = ARENA_MAP_ID;
        // The arena plays its own battle theme at the fight start (above, in
        // the dome tick); the field battle intro's sound-set swap is not on
        // this path, so the battle entry must not swap a track of its own.
        let battle_bgm = self.audio.battle_bgm.take();
        self.enter_battle_from_formation(&formation);
        self.audio.battle_bgm = battle_bgm;
        self.battle.active_formation = Some(formation);
        self.minigames.muscle_leg_in_battle = true;
    }

    /// The battle exit of a leg (called by [`World::finish_battle`] once the
    /// party is persisted and the field actor table restored): close the
    /// dome session on the battle's outcome and route the arena.
    ///
    /// A run is the give-up arm (`_DAT_80084448 = 4` behind the contest test
    /// at `0x801D322C`); a party wipe is a lost leg, not a game over - the
    /// exit selector never reaches the field's back-from-battle arm.
    pub(in crate::world) fn finish_muscle_battle_leg(
        &mut self,
        end: Option<legaia_engine_battle_vm::battle_action::BattleEndCause>,
    ) {
        use legaia_engine_battle_vm::battle_action::BattleEndCause;
        self.minigames.muscle_leg_in_battle = false;
        let ran = std::mem::take(&mut self.minigames.muscle_leg_ran);
        self.battle.special_word = 0;
        if let Some(id) = self.minigames.muscle_saved_map_id.take() {
            self.battle.map_id = id;
        }
        // The leg session carries the fight's HP into the record write-back
        // and the ringside still pick; the battle already persisted it.
        let hp = self
            .party
            .roster
            .members
            .get(usize::from(self.muscle_fighter_record()))
            .map(|r| i32::from(r.hp_mp_sp().hp_cur));
        let turns = u32::from(self.battle_mode());
        match end {
            // A Run round's give-up (`_DAT_80084448 = 4`) outlives the fight:
            // the re-entered arena settles the contest as given up however
            // the battle then ended.
            _ if ran => {
                if let (Some(s), Some(hp)) = (self.minigames.muscle_dome.as_mut(), hp) {
                    s.set_hp(0, hp);
                }
                self.end_muscle_leg();
                self.close_muscle_leg(legaia_engine_minigames::muscle_dome::LegReport {
                    survived: end != Some(BattleEndCause::PartyWipe),
                    outcome: legaia_engine_minigames::muscle_dome::LEG_OUTCOME_RAN,
                    turns_taken: turns,
                });
            }
            Some(BattleEndCause::MonsterWipe) | Some(BattleEndCause::PartyWipe) => {
                let survived = end == Some(BattleEndCause::MonsterWipe);
                if let (Some(s), Some(hp)) = (self.minigames.muscle_dome.as_mut(), hp) {
                    s.set_hp(0, if survived { hp } else { 0 });
                }
                self.end_muscle_leg();
                self.close_muscle_leg(legaia_engine_minigames::muscle_dome::LegReport {
                    survived,
                    outcome: 0,
                    turns_taken: turns,
                });
            }
            // An escape, or a battle that ended any other way: the leg is
            // left undecided, which the contest settles as a give-up.
            _ => {
                if let (Some(s), Some(hp)) = (self.minigames.muscle_dome.as_mut(), hp) {
                    s.set_hp(0, hp);
                }
                let _ = self.leave_muscle_dome();
            }
        }
    }
}
