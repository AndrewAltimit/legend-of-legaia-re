//! What PROT 0957 `0x76`'s director hands its host: the draws and pad reads
//! ahead of a pass, and the cues, card parts and results after it.

use super::*;
use legaia_engine_cast::cast_module_camera::{
    DeathGame, DeathGameOutcome, ModuleCamState, is_card_record,
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
            // A capture's own deal (`LEGAIA_SEAT_DEATH_GAME_DEAL`), dealt
            // before the capture's instant: the replay draws as retail did
            // and then stands on retail's table. A harness seat.
            if let Some(table) = self.battle.death_game_deal_seat {
                st.death_game.table = table;
            }
        }
        if phase == 4 && st.death_game.counter == 0 {
            st.death_game.draw_swap(|| self.next_rand());
            let category = self.retail_actor_category(caster_slot);
            self.route_battle_cue(0x44, category);
        }
        // Retail enters the module once a pass and arm 5 reads the pad word
        // once (`0x8007B874` against `0x2000` / `0x8000`, a `0x4C` step a
        // press). A pass here is a frame step of one-vsync ticks
        // (`World::module_pass`) that all see the same edge, so only the
        // first reads it: fed to each, one press walked the pointer three
        // cards at step `3`, and the middle two cards could not be picked.
        let first = !self.casting.module_pass_repeat;
        st.death_game.pad.right = first && self.input.just_pressed(PadButton::Right);
        st.death_game.pad.left = first && self.input.just_pressed(PadButton::Left);
        st.death_game.pad.confirm = first && self.input.just_pressed(PadButton::Cross);
        // The arms read each card's live point: its own script walks it in
        // (`lhu v0,0x16(a0)` at `0x801F823C`, the `lwl` / `lwr` copy at
        // `0x801F8B7C`).
        let g = &mut st.death_game;
        if let Some(scene) = self.casting.active_summon.as_ref() {
            let mut parts = scene
                .parts
                .iter()
                .filter(|p| !p.finished && p.tag.is_some_and(is_card_record));
            for i in (0..4).filter(|&i| g.live[i]) {
                let Some(part) = parts.next() else { break };
                g.cards[i] = [part.state.world_x, part.state.world_y, part.state.world_z];
            }
        }
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
        if phase < 3 {
            return;
        }
        let mut parts = scene
            .parts
            .iter_mut()
            .filter(|p| !p.finished && p.tag.is_some_and(is_card_record));
        // A handle this pass killed still holds its part.
        for i in (0..4).filter(|&i| g.live[i] || g.kills[i]) {
            let Some(part) = parts.next() else { break };
            let st = &mut part.state;
            [st.world_x, st.world_y, st.world_z] = g.cards[i];
            st.world_y_mirror = g.cards[i][1];
            if i == g.pair[0] {
                if g.turns_pick {
                    st.field_74 |= 0x8100_0000;
                }
                if phase == 9 {
                    st.field_78 = st.field_78.wrapping_add(32);
                    st.field_72 = st.field_72.wrapping_sub(32);
                }
            }
            if g.kills[i] {
                part.finished = true;
            }
        }
    }
}
