//! The timed fight's `World` seam.
//!
//! The gate id, the turn bound and the strip's readout kernels live in
//! [`legaia_engine_menus::timed_fight`] and are re-exported here at their old
//! paths. What stays is the two queries that read a live battle off the
//! world: whether the strip is up, and the strip itself.

pub use legaia_engine_menus::timed_fight::*;

/// `true` while the battle's command phase is up - from the round start to
/// the `Begin` that plays the round out - which is the strip's lifetime (see
/// the module docs).
pub fn strip_visible(world: &crate::world::World) -> bool {
    world.mode == crate::world::SceneMode::Battle
        && world.battle.flow != crate::battle_flow::BattleFlowState::Idle
}

/// The strip for this frame, or `None` when this is not Koru's fight, the
/// round is playing out, or the host read no battle-overlay strings (the
/// label is disc text and the port carries no copy of it).
///
/// The gate reads the first **monster** seat's id - the port seats the
/// formation's slot 0 at actor index `party_count`
/// ([`crate::world::World::battle_monster_slots`]), where retail's fixed table
/// keeps it at index 3 - and the percentage reads the same seat.
pub fn timed_fight_strip(world: &crate::world::World) -> Option<TimedFightStrip> {
    if !strip_visible(world) {
        return None;
    }
    let (idx, id, _) = world
        .battle_monster_slots()
        .into_iter()
        .find(|&(_, _, slot)| slot == 0)?;
    if id != u16::from(TIMED_FIGHT_MONSTER_ID) {
        return None;
    }
    let a = world.actors.get(idx)?;
    let label = world
        .battle
        .ui_strings
        .get(legaia_asset::battle_ui_strings::BattleUiLabel::TimedFightStrip)?
        .to_string();
    Some(TimedFightStrip {
        label,
        turns_left: timed_fight_turns_left(u32::from(world.battle_mode())),
        hp_left: hp_left_percent(a.battle.hp, a.battle.max_hp),
    })
}
