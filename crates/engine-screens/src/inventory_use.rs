//! The inventory item-use screen's draw list: the projection of an
//! [`InventoryUseSession`] onto the shared `engine-ui` builder.

use legaia_engine_menus::inventory_use::{
    InventoryContext, InventoryUseSession, InventoryUseState,
};
use legaia_engine_ui::{self as ui, TextDraw};

/// The item-use screen (browsing and target select) for `s`: one row per
/// owned item id (first-seen order, duplicate stacks counted), dimmed when
/// the context filter excludes it, names through the session's catalog, and
/// the session's party / monster targets. Unscaled stage pixels from
/// `(16, 32)`; the caller frames it and applies the stage transform. Both
/// play hosts draw the Items sub-screen through it; each used to carry a copy.
pub fn inventory_use_session_draws(
    font: &legaia_font::Font,
    s: &InventoryUseSession,
) -> Vec<TextDraw> {
    let filter_set: std::collections::HashSet<usize> = s.filtered_items.iter().copied().collect();
    let mut counts: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
    for id in &s.items {
        *counts.entry(*id).or_insert(0) = counts.get(id).copied().unwrap_or(0).saturating_add(1);
    }
    let mut seen: std::collections::HashSet<u8> = std::collections::HashSet::new();
    let mut row_data: Vec<(String, u8, bool)> = Vec::new();
    for (i, id) in s.items.iter().enumerate() {
        if !seen.insert(*id) {
            continue;
        }
        let name = s
            .catalog
            .get(*id)
            .map(|e| e.name.to_string())
            .unwrap_or_else(|| format!("Item {id:02X}"));
        let count = counts.get(id).copied().unwrap_or(1);
        row_data.push((name, count, filter_set.contains(&i)));
    }
    let item_rows: Vec<ui::InventoryItemRow<'_>> = row_data
        .iter()
        .map(|(n, c, a)| ui::InventoryItemRow {
            name: n,
            count: *c,
            admissible: *a,
        })
        .collect();
    let target_rows: Vec<ui::InventoryTargetRow<'_>> = s
        .targets
        .iter()
        .map(|t| ui::InventoryTargetRow {
            name: &t.name,
            hp: t.hp,
            hp_max: t.hp_max,
            mp: t.mp,
            mp_max: t.mp_max,
            alive: t.alive,
        })
        .collect();
    let (phase, cursor) = match s.state {
        InventoryUseState::Browsing { cursor } => (0u8, cursor as u8),
        InventoryUseState::TargetSelect { cursor, .. } => (1u8, cursor as u8),
        _ => (0u8, 0),
    };
    let args = ui::InventoryUseDrawArgs {
        items: &item_rows,
        targets: &target_rows,
        in_battle: matches!(s.context, InventoryContext::Battle),
        cursor,
        phase,
        selected_item_name: s.current_item().map(|e| e.name),
    };
    ui::inventory_use_draws_for(font, args, (16, 32))
}
