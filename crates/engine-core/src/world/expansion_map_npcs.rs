//! The `World` side of a pack map's NPCs and standable props
//! (`World::expansion.map`).
//!
//! A pack NPC is a field NPC in every way the world models one: its spawn
//! anchor in `npcs.positions` is what the moving-actor collision box, the
//! talk probe and both hosts' draw pose read, and its heading in
//! `npcs.headings` is what it faces. What a retail NPC takes from its MAN
//! record - what it says, where it walks - a pack NPC takes from the map:
//! the talk runs its pack line (or opens its card dealer) and raises the
//! quest event, and its patrol
//! is a loop of scripted walk legs through the motion VM.
//!
//! A standable prop is a floor box: the floor sample takes its top inside
//! the box, so the action-movement body stands, climbs and lands on it, and
//! the retail walk - which climbs nothing - is refused at its edge.
//!
//! All of it is the identity with no pack map active.

use legaia_scene_formats::man_section::ActorPlacement;

use super::World;

impl World {
    /// Seat a pack map's NPCs: clear the field NPC state the previous scene
    /// left (a pack map runs no MAN, so the scene entry installs none) and
    /// stand each placement on its tile, facing its compass sector. The
    /// placements are [`legaia_engine_scene::expansion_map::SceneOverride::npcs`];
    /// `facings` is parallel to them.
    pub fn install_expansion_npcs(&mut self, npcs: &[ActorPlacement], facings: &[u8]) {
        let n = &mut self.npcs;
        n.dialog.clear();
        n.dialog_prologue.clear();
        n.talk_proxies.clear();
        n.positions.clear();
        n.entry_positions.clear();
        n.glide_y.clear();
        n.headings.clear();
        n.motions.clear();
        n.glide_speeds.clear();
        self.props.walk_touch.clear();
        for (p, &facing) in npcs.iter().zip(facings) {
            let Ok(slot) = u8::try_from(p.index) else {
                continue;
            };
            self.npcs.positions.insert(slot, (p.world_x, p.world_z));
            self.npcs
                .headings
                .insert(slot, (i16::from(facing & 7) * 0x200 + 0x800) & 0x0FFF);
        }
        self.npcs.entry_positions = self.npcs.positions.clone();
    }

    /// The talk on a pack NPC: when placement `slot` is one of the active
    /// map's NPCs, run its line, raise `Talked(id)` for the quests and
    /// report the interaction. Returns whether `slot` was a pack NPC (the
    /// retail interaction then does not run).
    pub(in crate::world) fn expansion_npc_interact(&mut self, interact_id: u8, slot: u8) -> bool {
        let Some(talk) = self
            .expansion
            .map
            .as_ref()
            .and_then(|m| m.talk_gated(slot, |p| self.expansion_prerequisites_hold(p)))
        else {
            return false;
        };
        // A cutscene NPC plays its cutscene in place of its line.
        let played = talk
            .cutscene
            .as_deref()
            .is_some_and(|c| self.expansion_play_cutscene(c));
        // A shop NPC opens its counter (an item shop before a card dealer
        // of the same id); its line is the fallback.
        let opened = played
            || talk.shop.as_deref().is_some_and(|d| {
                let ok = if self.has_item_shop(d) {
                    self.open_item_shop(d)
                } else {
                    self.open_card_dealer(d)
                };
                if !ok {
                    log::warn!("expansion: npc `{}` dealer `{d}` did not open", talk.npc);
                }
                ok
            });
        if !opened
            && let Some(line) = &talk.line
            && let Err(e) = self.expansion_say(line)
        {
            log::warn!("expansion: npc `{}` line `{line}`: {e:?}", talk.npc);
        }
        self.expansion_talked(&talk.npc);
        self.pending_field_events.push(
            legaia_engine_field::field_events::FieldEvent::FieldInteract { interact_id, slot },
        );
        true
    }

    /// One tick of the pack NPCs' patrols: each patrolling NPC with no walk
    /// leg in flight starts the next one, toward the next stop of its loop.
    /// Nobody sets out while a dialogue holds the field, so the NPC being
    /// talked to stays put.
    pub fn step_expansion_npc_patrols(&mut self) {
        // A cutscene moves the NPCs itself.
        if self.dialogue_owns_input() || self.expansion.cutscene.active() {
            return;
        }
        let Some(map) = self.expansion.map.as_ref() else {
            return;
        };
        let walkers: Vec<(usize, u8, Option<u8>)> = map
            .npcs()
            .enumerate()
            .filter(|(_, (_, d))| !d.patrol.is_empty())
            .map(|(n, (slot, d))| (n, slot, d.walk_clip))
            .collect();
        for (n, slot, walk_clip) in walkers {
            if self.npcs.motions.contains_key(&slot) || self.expansion_npc_hidden(slot) {
                continue;
            }
            let Some((x, z)) = self
                .expansion
                .map
                .as_mut()
                .and_then(|m| m.next_patrol_stop(n))
            else {
                continue;
            };
            if self.start_field_npc_motion(slot, x, z)
                && let Some(clip) = walk_clip
            {
                // The leg plays the walk clip, as a `4C 51` walk's move-anim
                // id does.
                self.carry_npc_run_anim(slot, clip);
            }
        }
    }

    /// The floor height (Y down) of the standable top under world `(x, z)`,
    /// `None` off every top or with no map active.
    pub(in crate::world) fn expansion_floor_top_y(&self, x: i32, z: i32) -> Option<i32> {
        self.expansion
            .map
            .as_ref()?
            .floor_top_at(x, z)
            .map(|top| -i32::from(top))
    }

    /// Whether the retail walk's step from `from` to `to` climbs onto a
    /// standable top higher than the one it stands on: the walk climbs
    /// nothing, so a top is a wall to it. Walking off one, or across one, is
    /// not refused.
    pub(in crate::world) fn expansion_top_blocks_walk(
        &self,
        from: (i16, i16),
        to: (i16, i16),
    ) -> bool {
        let Some(map) = self.expansion.map.as_ref().filter(|m| m.has_floor_tops()) else {
            return false;
        };
        let top = |(x, z): (i16, i16)| map.floor_top_at(i32::from(x), i32::from(z));
        match (top(from), top(to)) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(a), Some(b)) => b > a,
        }
    }
}
