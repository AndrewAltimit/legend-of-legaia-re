//! The battle Magic and Item windows' models: owned projections of the open
//! submenu sessions (`BattleState::spell_menu` / `item_menu`) the windowed
//! surfaces draw on both hosts. The sessions themselves - build, input,
//! commit - are the battle loop's, in engine-core.

use legaia_engine_core::world::World;

/// Owned projection of the open battle Magic window (retail state
/// `0x46`) while its list is browsed: the rows (name, affordability,
/// Ra-Seru icon), the hand row, the highlighted spell's info-window lines
/// off the disc's spell table and its level (the caster's record
/// `+0x161` list) and effective MP cost - the `Lv` / `MP` tabs.
///
/// `None` while no Magic window is browsing (target select draws the
/// HUD's target surfaces, not this window) or a dialogue box owns the
/// frame.
pub fn battle_spell_menu_model(
    world: &World,
) -> Option<legaia_engine_battle::battle_magic::BattleSpellMenuModel> {
    use legaia_engine_battle::battle_magic::{BattleSpellMenuModel, SpellPhase};
    if world.mode != legaia_engine_mode::mode::SceneMode::Battle {
        return None;
    }
    if world.dialog.current.is_some() || world.dialog.inline.is_some() {
        return None;
    }
    let menu = world.battle.spell_menu.as_ref()?;
    let SpellPhase::Select { cursor } = menu.phase else {
        return None;
    };
    let ra_seru = |id: u8| legaia_engine_menus::pause_screens::RA_SERU_SPELL_IDS.contains(&id);
    let rows = menu
        .spells
        .iter()
        .map(|s| (s.name.clone(), s.affordable, ra_seru(s.id)))
        .collect();
    let icons = menu
        .spells
        .iter()
        .map(|s| legaia_engine_menus::pause_screens::spell_icon(world.menu.text.as_ref(), s.id))
        .collect();
    let selected = menu.spells.get(cursor as usize);
    let description = selected
        .and_then(|s| world.menu.text.as_ref().and_then(|t| t.spell_desc(s.id)))
        .map(|d| d.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let level_mp = selected.map(|s| {
        let char_slot = world.party_roster_slot(menu.actor as usize);
        let level = world
            .party
            .roster
            .members
            .get(char_slot)
            .and_then(|m| {
                let list = m.spell_list();
                let n = (list.count as usize).min(list.ids.len());
                let at = list.ids[..n].iter().position(|&id| id == s.id)?;
                Some(list.levels[at])
            })
            .filter(|&l| l > 0)
            .unwrap_or(1);
        (level, u16::from(s.mp_cost))
    });
    Some(BattleSpellMenuModel {
        rows,
        icons,
        cursor: (!menu.spells.is_empty()).then_some(cursor as usize),
        description,
        level_mp,
    })
}

/// Owned projection of the open battle item menu for the windowed item
/// surface (`legaia_engine_ui::battle_item_ui` on both hosts): the
/// dedup row list + mapped cursor
/// ([`InventoryUseSession::menu_view`]), the highlighted item's disc
/// info-window line, the acting member's name (the middle breadcrumb of
/// retail's `Begin | <name> | Item` trail) and the target roster while
/// the session is picking a target.
///
/// `None` while no battle item menu is up or a dialogue box owns the
/// frame - the same suppression the command chips follow. Living here
/// rather than in each host keeps the two hosts on one projection
/// (host-drift tier: paired simulation injection sites).
pub fn battle_item_menu_model(
    world: &World,
) -> Option<legaia_engine_menus::inventory_use::BattleItemMenuModel> {
    if world.mode != legaia_engine_mode::mode::SceneMode::Battle {
        return None;
    }
    if world.dialog.current.is_some() || world.dialog.inline.is_some() {
        return None;
    }
    let menu = world.battle.item_menu.as_ref()?;
    let view = menu.menu_view();
    let description = view
        .selected_id
        .and_then(|id| world.menu.text.as_ref().and_then(|t| t.item_desc(id)))
        .map(str::to_string);
    let actor = world.battle_ctx.active_actor;
    let actor_name = menu
        .targets
        .iter()
        .find(|t| t.slot == actor)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| format!("P{}", actor + 1));
    let targets = view.target_select.then(|| {
        // Only the rows on the selected item's own side are LISTED -
        // retail's state-0x64 panel is the party roster and its cursor
        // walk wraps inside the party band `[0, ctx[+0x00])`; the
        // enemy-side item states ring the monster seats instead
        // (`FUN_801D0748`; rule pinned at
        // `legaia_engine_menus::inventory_use::target_on_effect_side`). The session
        // cursor indexes the full roster, so it is remapped onto the
        // filtered list.
        let side_ok = |t: &legaia_engine_menus::inventory_use::TargetRow| {
            menu.current_item()
                .map(|e| legaia_engine_menus::inventory_use::target_on_effect_side(&e.effect, t))
                .unwrap_or(!t.is_enemy)
        };
        let cursor = menu
            .targets
            .iter()
            .take(view.target_cursor)
            .filter(|t| side_ok(t))
            .count();
        (
            menu.targets
                .iter()
                .filter(|t| side_ok(t))
                .map(
                    |t| legaia_engine_menus::inventory_use::BattleItemTargetRow {
                        name: t.name.clone(),
                        hp: t.hp,
                        hp_max: t.hp_max,
                        mp: t.mp,
                        mp_max: t.mp_max,
                        alive: t.alive,
                    },
                )
                .collect(),
            cursor,
        )
    });
    Some(legaia_engine_menus::inventory_use::BattleItemMenuModel {
        view,
        description,
        actor_name,
        targets,
    })
}
