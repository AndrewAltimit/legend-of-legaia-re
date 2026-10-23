//! The battle item window's frame: the borrow glue between the engine's
//! item-window model and the shared `engine-ui` builder.

use legaia_engine_menus::inventory_use::BattleItemMenuModel;
use legaia_engine_ui::battle_item_ui as bii;

/// Borrow a battle-item-window model (`World::battle_item_menu_model`) as the
/// shared builder's frame, handing it to `f`. The frame borrows row views
/// built here, so this is continuation-shaped. Both play hosts draw the item
/// window through it; each used to carry its own copy.
pub fn with_battle_item_frame<R>(
    model: &BattleItemMenuModel,
    f: impl FnOnce(&bii::BattleItemMenuFrame<'_>) -> R,
) -> R {
    let rows: Vec<bii::BattleItemRowView<'_>> = model
        .view
        .rows
        .iter()
        .map(|r| bii::BattleItemRowView {
            name: &r.name,
            count: r.count,
            admissible: r.admissible,
        })
        .collect();
    let target_rows: Vec<bii::BattleItemTargetView<'_>> = model
        .targets
        .as_ref()
        .map(|(rows, _)| {
            rows.iter()
                .map(|t| bii::BattleItemTargetView {
                    name: &t.name,
                    hp: t.hp,
                    hp_max: t.hp_max,
                    mp: t.mp,
                    mp_max: t.mp_max,
                    alive: t.alive,
                })
                .collect()
        })
        .unwrap_or_default();
    let frame = bii::BattleItemMenuFrame {
        rows: &rows,
        cursor: model.view.cursor_row,
        description: model.description.as_deref(),
        actor_name: &model.actor_name,
        targets: model
            .targets
            .as_ref()
            .map(|(_, cursor)| (target_rows.as_slice(), *cursor)),
    };
    f(&frame)
}

/// Borrow a battle Magic window model (`World::battle_spell_menu_model`) as
/// the shared builder's frame - the spell window's twin of
/// [`with_battle_item_frame`].
pub fn with_battle_spell_frame<R>(
    model: &legaia_engine_battle::battle_magic::BattleSpellMenuModel,
    f: impl FnOnce(&bii::BattleSpellMenuFrame<'_>) -> R,
) -> R {
    let rows: Vec<bii::BattleSpellRowView<'_>> = model
        .rows
        .iter()
        .enumerate()
        .map(|(i, (name, admissible, ra_seru))| bii::BattleSpellRowView {
            name,
            admissible: *admissible,
            ra_seru: *ra_seru,
            icon: model.icons.get(i).copied().flatten(),
        })
        .collect();
    let description: Vec<&str> = model.description.iter().map(String::as_str).collect();
    let frame = bii::BattleSpellMenuFrame {
        rows: &rows,
        cursor: model.cursor,
        description: &description,
        level_mp: model.level_mp,
    };
    f(&frame)
}
