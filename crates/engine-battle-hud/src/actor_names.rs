//! Party / monster display names and the active actor.

use super::*;

/// Seated party count, clamped to the actor table.
pub(super) fn party_count(world: &legaia_engine_core::world::World) -> usize {
    // A dome leg seats the lead fighter alone, and no battle actor table
    // backs it.
    if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
        return 1;
    }
    (world.party.party_count.clamp(1, 3) as usize).min(world.actors.len())
}

/// A party member's display name by battle ordinal.
pub(super) fn party_member_name(world: &legaia_engine_core::world::World, slot: u8) -> String {
    let names = world.party.roster_names();
    names
        .get(world.party_roster_slot(slot as usize))
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("P{}", slot + 1))
}

/// A monster's catalog name by actor slot.
pub(super) fn monster_name(world: &legaia_engine_core::world::World, slot: u8) -> String {
    let pc = party_count(world);
    let catalog = |s: usize| -> Option<String> {
        world
            .actors
            .get(s)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| world.tables.monster_catalog.get(id))
            .map(|d| d.name.clone())
    };
    let Some(name) = catalog(slot as usize) else {
        return format!("M{}", (slot as usize).saturating_sub(pc) + 1);
    };
    // The actor's display payload `+0x1BC` carries an instance letter when
    // the formation seats the same monster more than once (`Skeleton A`,
    // `Gilium Lv3 A`), lettered over the seated slots so a twin dying does
    // not re-letter the survivor - the same rule the target rows use.
    let twins: Vec<usize> = (pc..pc + legaia_engine_battle::target_picker::FORMATION_SLOTS)
        .filter(|&s| {
            world.actors.get(s).is_some_and(|a| a.battle.max_hp != 0)
                && catalog(s).as_deref() == Some(name.as_str())
        })
        .collect();
    if twins.len() < 2 {
        return name;
    }
    let ordinal = twins.iter().position(|&s| s == slot as usize).unwrap_or(0) as u8;
    format!("{name} {}", (b'A' + ordinal) as char)
}

/// Any actor's display name by slot.
pub(super) fn actor_name(world: &legaia_engine_core::world::World, slot: u8) -> String {
    if (slot as usize) < party_count(world) {
        party_member_name(world, slot)
    } else {
        monster_name(world, slot)
    }
}

/// The actor the frame's top-left plaque names, with its display name -
/// the party member entering a command, or the actor whose action is
/// playing out - or `None` when retail draws no plaque (the round prompt,
/// the holds between actions, and the `0x6E` all-committed prompt).
///
/// Two records carry the plaque. Record 26 (`(68, -24)` to `(68, 14)`) is
/// the command-entry one: step 1 slides it in and every later menu step
/// snaps it (`1A/3`) except step `0x23`. Record 68 (`(16, -24)` to
/// `(16, 14)`) is the action one: opened for the acting actor and closed in
/// the `0x51` band unless the category is Run (`0x801E61F0`, category `5`
/// skips the `(0x44, 1)` close because no plaque was opened for it). The
/// captures agree - `Vahn` behind the tab in the ring, `Vahn` / `Gimard` /
/// `Zora` at `(16, 12)` through their actions, nothing at `0x1E` or `0x0A`.
pub fn battle_active_actor(world: &legaia_engine_core::world::World) -> Option<(u8, String)> {
    match battle_hud_phase(world) {
        BattleHudPhase::Idle | BattleHudPhase::RoundPrompt => None,
        BattleHudPhase::CommandEntry => {
            if battle_command_surface(world) == Some(CommandSurface::Other) {
                return None;
            }
            let slot = command_entry_actor(world)?;
            Some((slot, actor_name(world, slot)))
        }
        BattleHudPhase::Action if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome => {
            // A dome play-out: the plaque names the side whose play is
            // acting out (the capture-pinned rule: the acting fighter's
            // plate holds the top-left seat once actions play).
            let (attacker, _) = world.muscle_playback_tally()?;
            let name = if attacker == 0 {
                party_member_name(world, 0)
            } else {
                world
                    .minigames
                    .muscle_dome
                    .as_ref()?
                    .opponent_name()?
                    .to_string()
            };
            Some((attacker as u8, name))
        }
        BattleHudPhase::Action => {
            let slot = world.battle_ctx.active_actor;
            let actor = world.actors.get(slot as usize)?;
            if actor.battle.action_category
                == legaia_engine_battle_vm::battle_action::ActionCategory::Run.as_byte()
            {
                return None;
            }
            Some((slot, actor_name(world, slot)))
        }
    }
}
