//! What PROT 0957 `0x76`'s director hands its host: the draws and pad reads
//! ahead of a pass, and the cues, card parts and results after it.

use super::*;
use legaia_engine_cast::cast_module_camera::{
    CARD_RECORDS, DeathGame, DeathGameOutcome, ModuleCamState, PICK_RECORD,
};
use legaia_engine_system::input::PadButton;

impl World {
    /// Arm 0's deal and arm 4's swap draws off the world `rand()` stream
    /// (`jal 0x80056798`), and arm 5's pad. Retail reads the pad on a monster's
    /// cast too: the player steers the roulette, whoever cast it.
    pub(in crate::world::battle::cast_band) fn feed_death_game(
        &mut self,
        phase: u8,
        caster_monster: u8,
        caster_slot: u8,
        st: &mut ModuleCamState,
    ) {
        if phase == 0 {
            st.death_game.deal(caster_monster, || self.next_rand());
        }
        if phase == 4 && st.death_game.counter == 0 {
            st.death_game.draw_swap(|| self.next_rand());
            let category = self.retail_actor_category(caster_slot);
            self.route_battle_cue(0x44, category);
        }
        st.death_game.pad.right = self.input.just_pressed(PadButton::Right);
        st.death_game.pad.left = self.input.just_pressed(PadButton::Left);
        st.death_game.pad.confirm = self.input.just_pressed(PadButton::Cross);
        let caster = self.actors.get(usize::from(caster_slot));
        st.death_game.caster_faded = caster.is_some_and(|a| a.battle.render_color == 0);
        st.caster_settled = self.cast_view(caster_slot).playing_anim == 0;
    }

    /// The pass's cues, then its writes through the card handles
    /// (`0x801FA2B8`): each live card on the kernel's point, the kills, and
    /// the pick's growth (`+0x78`, `+0x72`) and blend bits (`+0x74`).
    pub(in crate::world::battle::cast_band) fn carry_death_game_pass(
        &mut self,
        g: &DeathGame,
        phase: u8,
        caster_slot: u8,
    ) {
        for &cue in g.cues.iter().filter(|&&c| c != 0) {
            self.dispatch_battle_cue(cue);
        }
        if g.outcome == Some(DeathGameOutcome::CasterFalls)
            && let Some(a) = self.actors.get_mut(usize::from(caster_slot))
        {
            a.battle.capture_state = 2;
        }
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        // The faces stand from arm 2 until arm 5 turns them, the backs after.
        let live_record = |tag: u32| {
            if phase <= 5 {
                CARD_RECORDS.contains(&tag)
            } else {
                tag == PICK_RECORD
            }
        };
        let mut cards = scene
            .parts
            .iter_mut()
            .filter(|p| !p.finished && p.tag.is_some_and(live_record));
        for i in 0..4 {
            let Some(part) = cards.next() else { break };
            let st = &mut part.state;
            [st.world_x, st.world_y, st.world_z] = g.cards[i];
            st.world_y_mirror = g.cards[i][1];
            if phase >= 8 && i == g.pair[0] {
                st.field_78 = g.pick_level;
                st.field_72 = g.pick_fade as u16;
                if g.turns_pick {
                    st.field_74 |= 0x8100_0000;
                }
            }
            if g.kills[i] {
                part.finished = true;
            }
        }
    }
}
