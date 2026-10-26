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

/// The roster record that fights a leg: the lead record the contest's start
/// restore and the leg session both read.
const DOME_FIGHTER_ROSTER_SLOT: u8 = 0;

impl World {
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
        self.battle.solo_spar_restore = Some(std::mem::replace(
            &mut self.party.active_party,
            vec![DOME_FIGHTER_ROSTER_SLOT],
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
            .get(usize::from(DOME_FIGHTER_ROSTER_SLOT))
            .map(|r| i32::from(r.hp_mp_sp().hp_cur));
        let turns = u32::from(self.battle_mode());
        match end {
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
