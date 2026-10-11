//! The item / spell target panel: per-member rows and the stat record it reads.

use super::*;

/// One roster row of the window-14 target panel view model.
#[derive(Debug, Clone, Default)]
pub struct TargetPanelMemberModel {
    pub name: String,
    /// Record `+0x130`. The inner use-flow's target rows carry no level;
    /// hosts with party records overwrite this (0 draws as a blank-ish
    /// `0` otherwise).
    pub level: u8,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    /// Record-side base maxima (`+0x11C` / `+0x11E`) - the teal paren
    /// values of the mode-1 (Life / Magic Water) preview rows. Zero
    /// unless the builder had the character record.
    pub base_hp_max: u16,
    pub base_mp_max: u16,
    /// Effective stats in the retail panel's label order (ATK, UDF, LDF,
    /// SPD, INT) - the left value of the modes-2..5 stat rows.
    pub stat_eff: [u16; 5],
    /// Record-side base stats (`+0x124..+0x12C`), same order - the teal
    /// paren value of the same rows.
    pub stat_base: [u16; 5],
}

/// Owned view model of the window-14 party target panel - maps onto the
/// engine-ui `TargetPanelView` (renderer `FUN_801D0520`).
#[derive(Debug, Clone, Default)]
pub struct TargetPanelModel {
    pub members: Vec<TargetPanelMemberModel>,
    /// The preview word `DAT_801E46CC` value (0..=5, see
    /// [`target_panel_mode`]).
    pub mode: u32,
    pub cursor_row: u8,
    /// All-party pick (retail cursor bit `0x2000` - hand on every row).
    pub all_targets: bool,
}

/// Assemble the target-panel view model while the Items screen's use
/// flow is in target select. `mode` is the retail preview word for the
/// staged item ([`target_panel_mode`]; pass 0 without disc effect
/// tables - the plain `cur/max` panel).
pub fn target_panel_model(s: &PauseItemsSession, mode: u32) -> Option<TargetPanelModel> {
    let InventoryUseState::TargetSelect { cursor, .. } = &s.inner.state else {
        return None;
    };
    let members = s
        .inner
        .targets
        .iter()
        .map(|t| TargetPanelMemberModel {
            name: t.name.clone(),
            level: 0,
            hp: t.hp,
            hp_max: t.hp_max,
            mp: t.mp,
            mp_max: t.mp_max,
            ..Default::default()
        })
        .collect();
    Some(TargetPanelModel {
        members,
        mode,
        cursor_row: *cursor as u8,
        all_targets: false,
    })
}

/// The bag id the Items screen's use flow currently has staged - the row
/// the target select was entered from (`item_cursor` -> `filtered_items`
/// -> `items`). `None` outside target select.
pub fn staged_use_item_id(s: &PauseItemsSession) -> Option<u8> {
    let InventoryUseState::TargetSelect { item_cursor, .. } = &s.inner.state else {
        return None;
    };
    let idx = s.inner.filtered_items.get(*item_cursor).copied()?;
    s.inner.items.get(idx).copied()
}

/// The stat aggregator's input record for one character: live base stats
/// off the record, accuracy and evasion from AGL, and the record's equip
/// bytes.
pub fn stat_record_from_character(
    c: &legaia_save::CharacterRecord,
) -> legaia_engine_battle::battle_stats::StatRecord {
    let eq_bytes = c.equipment().slots;
    let live = c.live_stats();
    legaia_engine_battle::battle_stats::StatRecord {
        base_attack: live.atk,
        base_udf: live.udf,
        base_ldf: live.ldf,
        // Accuracy / evasion derive from AGL (not equipment-fed).
        base_accuracy: live.agl,
        base_evasion: live.agl,
        base_spd: live.spd,
        base_int: live.int,
        equip: eq_bytes,
    }
}

/// Host entry point for the window-14 target panel: derive the retail
/// preview word for the staged item off the world's disc item-effect
/// table, build the view model, then fill the per-member record-side
/// fields (base maxima + base stats) the water previews draw from the
/// live party records.
///
/// This is the call the pause-menu host makes while the Items screen is
/// in target select; [`target_panel_mode`] and [`target_panel_model`]
/// are its two halves.
///
/// `item_effects` is the disc item-effect table (`World::tables.item_effects`)
/// and `members` the live roster (`World::party.roster.members`): the whole
/// slice of the world the panel reads. engine-core's
/// `pause_screens::target_panel_view_model` passes them off a `World`.
pub fn target_panel_view_model_of(
    s: &PauseItemsSession,
    item_effects: Option<&legaia_game_tables::item_effect::ItemEffectTable>,
    members: &[legaia_save::CharacterRecord],
) -> Option<TargetPanelModel> {
    let mode = staged_use_item_id(s)
        .and_then(|id| {
            let table = item_effects?;
            let eff = table.effect(id)?;
            Some(target_panel_mode(table.kind(id), eff.class, eff.tier))
        })
        .unwrap_or(0);
    let mut model = target_panel_model(s, mode)?;
    for (m, row) in model.members.iter_mut().zip(s.inner.targets.iter()) {
        // Party rows index the roster by slot; monster rows (battle-side
        // targets) have no character record and keep the zeroed fields.
        let Some(rec) = members.get(row.slot as usize) else {
            continue;
        };
        if row.is_enemy {
            continue;
        }
        let live = rec.live_stats();
        let base = rec.record_stats();
        m.level = match rec.magic_rank() {
            l @ 1..=99 => l,
            _ => legaia_save::level_for_cumulative_xp(rec.cumulative_xp()),
        };
        m.base_hp_max = base.hp_max;
        m.base_mp_max = base.mp_max;
        m.stat_eff = [live.atk, live.udf, live.ldf, live.spd, live.int];
        m.stat_base = [base.atk, base.udf, base.ldf, base.spd, base.int];
    }
    Some(model)
}
