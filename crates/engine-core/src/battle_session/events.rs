//! World-event drain + the typed `BattleEvent` -> HUD/session-event router.

use super::*;

impl BattleSession {
    /// Drain `World::pending_battle_events`, fold each through `World::fold_battle_event`
    /// for HP / status updates, push a HUD popup + log line, and emit a
    /// matching [`SessionEvent`].
    pub(super) fn drain_world_events(&mut self, world: &mut World, out: &mut Vec<SessionEvent>) {
        let events = world.drain_battle_events();
        for ev in events {
            // World handles HP / status side first.
            world.fold_battle_event(&ev);
            self.fold_event_into_hud(&ev, out);
        }
    }

    /// Mirror of `Self::drain_world_events` for engines that already
    /// drained the world themselves (e.g. play-window keeps its own log).
    pub fn fold_event(&mut self, world: &mut World, ev: &crate::battle_events::BattleEvent) {
        world.fold_battle_event(ev);
        let mut sink = Vec::new();
        self.fold_event_into_hud(ev, &mut sink);
    }

    /// Public accessor for the typed `BattleEvent` -> HUD/event router.
    /// Engines that maintain their own world drain (so they can keep a
    /// custom log column) call this once per drained event instead of
    /// re-implementing the routing.
    pub fn route_event(&mut self, ev: &crate::battle_events::BattleEvent) -> Vec<SessionEvent> {
        let mut sink = Vec::new();
        self.fold_event_into_hud(ev, &mut sink);
        sink
    }

    pub(super) fn fold_event_into_hud(
        &mut self,
        ev: &crate::battle_events::BattleEvent,
        out: &mut Vec<SessionEvent>,
    ) {
        use crate::battle_events::BattleEvent as Ev;
        match ev {
            Ev::ApplyArtStrike {
                target_slot,
                outcome,
                ..
            } => {
                if let Some(dmg) = outcome.damage
                    && dmg > 0
                {
                    self.hud.push_damage(*target_slot, dmg);
                    self.hud
                        .push_log(format!("-{dmg} HP slot {target_slot}"), LogAccent::Party);
                    out.push(SessionEvent::HpChanged {
                        slot: *target_slot,
                        amount: dmg,
                        is_heal: false,
                    });
                }
                if let Some(kind) = StatusKind::from_enemy_effect(outcome.enemy_effect) {
                    self.hud.push_status(*target_slot, kind);
                    self.hud
                        .push_log(format!("{kind:?} slot {target_slot}"), LogAccent::Highlight);
                    out.push(SessionEvent::StatusApplied {
                        slot: *target_slot,
                        kind,
                    });
                }
            }
            Ev::ApplyDamage { target_slot, .. } => {
                self.hud.push_log(
                    format!("ApplyDamage slot {target_slot}"),
                    LogAccent::Neutral,
                );
            }
            Ev::ScreenShake { magnitude } => {
                self.hud
                    .push_log(format!("Shake {magnitude}"), LogAccent::Neutral);
            }
            Ev::BattleEnd { cause } => {
                self.handle_battle_end(*cause, out);
            }
            Ev::LevelUp {
                char_id,
                new_level,
                hp_gained,
                mp_gained,
            } => {
                self.hud.push_log(
                    format!("LV{new_level} +{hp_gained}HP/+{mp_gained}MP char{char_id}"),
                    LogAccent::Highlight,
                );
            }
            Ev::TacticalArtLearned { char_id, art_id } => {
                self.hud.push_log(
                    format!("Art learned char{char_id} #{art_id}"),
                    LogAccent::Highlight,
                );
            }
            _ => {}
        }
    }
}
