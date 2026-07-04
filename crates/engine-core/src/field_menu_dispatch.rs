//! Field-menu sub-session dispatcher.
//!
//! Hooks the seven [`crate::field_menu::FieldMenuRow`] selections to their
//! respective renderer-agnostic sub-sessions:
//!
//! | Row     | Sub-session                                           |
//! |---------|-------------------------------------------------------|
//! | Items   | [`crate::inventory_use::InventoryUseSession`]         |
//! | Equip   | [`crate::equip_session::EquipSession`]                |
//! | Spells  | [`crate::spell_menu::SpellMenuSession`]               |
//! | Arts    | [`crate::tactical_arts_editor::ChainEditor`]          |
//! | Status  | [`crate::status_screen::StatusScreenSession`]         |
//! | Save    | [`crate::save_select::SaveSelectSession`] (Save mode) |
//! | Config  | [`crate::options::OptionsSession`]                    |
//!
//! Pure plumbing - the dispatcher builds the right sub-session from
//! [`World`] state, routes per-frame pad input into it, and exposes
//! `is_done` for the engine to call [`crate::field_menu::FieldMenuSession::resume`].
//! Side-effects (writing equipment back to a record, casting a spell on
//! the active party, persisting a save) are intentionally left to the
//! engine - see [`apply_equip_outcome`] / [`apply_inventory_outcome`] /
//! [`apply_spell_outcome`] / [`apply_arts_outcome`] for the typed helpers.

use crate::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
use crate::equip_session::{EquipInput, EquipOutcome, EquipSession};
use crate::field_menu::FieldMenuRow;
use crate::input::PadButton;
use crate::inventory_use::{
    InventoryContext, InventoryUseInput, InventoryUseSession, TargetRow as InvTargetRow,
};
use crate::options::{OptionsInput, OptionsSession, OptionsState};
use crate::save_select::{SaveSelectMode, SaveSelectSession, SelectInput, SlotSnapshot};
use crate::spell_menu::{
    CasterSlot as SpellCasterSlot, SpellMenuInput, SpellMenuOutcome, SpellMenuSession,
    TargetRow as SpellTargetRow,
};
use crate::spells::SpellCatalog;
use crate::status_screen::{
    ElementRankView, EquipSlotView, StatusInput, StatusScreenSession, StatusSnapshot,
};
use crate::tactical_arts_editor::{ChainEditor, ChainLibrary, EditInput};
use crate::world::World;

/// One of the seven sub-sessions that can be active beneath a suspended
/// [`crate::field_menu::FieldMenuSession`].
pub enum FieldMenuSubsession {
    Items(InventoryUseSession),
    /// Equip session paired with the slot of the character whose record is
    /// being edited so the caller can write the result back to the right
    /// roster member.
    Equip {
        session: EquipSession,
        char_slot: u8,
    },
    Spells(SpellMenuSession),
    Arts(ChainEditor),
    Status(StatusScreenSession),
    Save(SaveSelectSession),
    Config(OptionsSession),
}

impl FieldMenuSubsession {
    /// Construct the sub-session matching `row`. Engines that want to
    /// override one of the construction inputs (e.g. supply a custom
    /// equipment table for Equip, or a saved-chain library for Arts)
    /// should build that variant directly.
    pub fn build(
        row: FieldMenuRow,
        world: &World,
        options: &OptionsState,
        save_slots: &[SlotSnapshot],
        chain_library: &ChainLibrary,
        spell_catalog: &SpellCatalog,
        equipment_table: &EquipmentTable,
    ) -> Self {
        match row {
            FieldMenuRow::Items => Self::Items(build_inventory_session(world)),
            FieldMenuRow::Equip => {
                let leader = active_leader_slot(world);
                let session = build_equip_session(world, leader, equipment_table);
                Self::Equip {
                    session,
                    char_slot: leader,
                }
            }
            FieldMenuRow::Spells => Self::Spells(build_spell_session(world, spell_catalog)),
            FieldMenuRow::Arts => {
                Self::Arts(ChainEditor::new(active_leader_slot(world), chain_library))
            }
            FieldMenuRow::Status => Self::Status(StatusScreenSession::new(status_snapshots(world))),
            FieldMenuRow::Save => Self::Save(SaveSelectSession::new(
                SaveSelectMode::Save,
                save_slots.to_vec(),
            )),
            FieldMenuRow::Config => Self::Config(OptionsSession::new(options.clone())),
        }
    }

    /// Return the [`FieldMenuRow`] this subsession was built for.
    pub fn row(&self) -> FieldMenuRow {
        match self {
            Self::Items(_) => FieldMenuRow::Items,
            Self::Equip { .. } => FieldMenuRow::Equip,
            Self::Spells(_) => FieldMenuRow::Spells,
            Self::Arts(_) => FieldMenuRow::Arts,
            Self::Status(_) => FieldMenuRow::Status,
            Self::Save(_) => FieldMenuRow::Save,
            Self::Config(_) => FieldMenuRow::Config,
        }
    }

    /// Drive one frame using a PSX-encoded edge-triggered "newly pressed"
    /// pad bitmask. Each variant's tick method receives the matching
    /// per-button input bundle.
    pub fn tick_pad_edge(&mut self, pressed: u16) {
        match self {
            Self::Items(s) => {
                if let Some(ev) = inventory_input_from_pad(pressed) {
                    s.input(ev);
                }
            }
            Self::Equip { session, .. } => {
                session.input(EquipInput {
                    up: pressed & PadButton::Up.mask() != 0,
                    down: pressed & PadButton::Down.mask() != 0,
                    left: pressed & PadButton::Left.mask() != 0,
                    right: pressed & PadButton::Right.mask() != 0,
                    cross: pressed & PadButton::Cross.mask() != 0,
                    circle: pressed & PadButton::Circle.mask() != 0,
                    triangle: pressed & PadButton::Triangle.mask() != 0,
                });
            }
            Self::Spells(s) => {
                let _ = s.tick(SpellMenuInput::from_pad_edge(pressed));
            }
            Self::Arts(s) => {
                let square = pressed & PadButton::Square.mask() != 0;
                let _ = s.tick(EditInput {
                    up: pressed & PadButton::Up.mask() != 0,
                    down: pressed & PadButton::Down.mask() != 0,
                    left: pressed & PadButton::Left.mask() != 0,
                    right: pressed & PadButton::Right.mask() != 0,
                    cross: pressed & PadButton::Cross.mask() != 0,
                    circle: pressed & PadButton::Circle.mask() != 0,
                    triangle: pressed & PadButton::Triangle.mask() != 0,
                    square,
                    // Square doubles as "cycle name" while in the naming
                    // phase; the editor's tick path ignores name_next
                    // outside that phase.
                    name_next: square,
                });
            }
            Self::Status(s) => {
                let _ = s.tick(StatusInput::from_pad_edge(pressed));
            }
            Self::Save(s) => {
                let _ = s.tick(SelectInput {
                    up: pressed & PadButton::Up.mask() != 0,
                    down: pressed & PadButton::Down.mask() != 0,
                    left: pressed & PadButton::Left.mask() != 0,
                    right: pressed & PadButton::Right.mask() != 0,
                    cross: pressed & PadButton::Cross.mask() != 0,
                    circle: pressed & PadButton::Circle.mask() != 0,
                    triangle: pressed & PadButton::Triangle.mask() != 0,
                });
            }
            Self::Config(s) => {
                let _ = s.tick(OptionsInput::from_pad_edge(pressed));
            }
        }
    }

    /// `true` once the inner sub-session has reached its terminal state.
    /// The shell should then call
    /// [`crate::field_menu::FieldMenuSession::resume`] to drop control
    /// back into the field menu.
    pub fn is_done(&self) -> bool {
        match self {
            Self::Items(s) => s.is_done(),
            Self::Equip { session, .. } => session.is_done(),
            Self::Spells(s) => s.is_done(),
            Self::Arts(s) => s.is_done(),
            Self::Status(s) => s.is_done(),
            Self::Save(s) => s.is_done(),
            Self::Config(s) => s.is_done(),
        }
    }
}

/// Apply a finished [`EquipSession`] to a `world.roster` member. Returns
/// `Some(EquipOutcome)` when a swap was committed; `None` for cancelled
/// sessions.
pub fn apply_equip_outcome(
    session: &EquipSession,
    char_slot: u8,
    world: &mut World,
) -> Option<EquipOutcome> {
    let outcome = session.outcome()?;
    if let EquipOutcome::Committed {
        slot,
        added,
        removed,
    } = outcome
        && let Some(member) = world.roster.members.get_mut(char_slot as usize)
    {
        let mut eq = member.equipment();
        if (slot as usize) < eq.slots.len() {
            eq.slots[slot as usize] = added;
            member.set_equipment(eq);
            // Reconcile the bag with the swap the session computed on its own
            // (cloned) copy: the newly-equipped item leaves inventory, the
            // swapped-out item (if any) returns to it. Without this the equipped
            // item stays in the bag (duplication) and the old one is lost.
            if added != 0
                && let Some(qty) = world.inventory.get_mut(&added)
            {
                *qty = qty.saturating_sub(1);
                if *qty == 0 {
                    world.inventory.remove(&added);
                }
            }
            if removed != 0 {
                *world.inventory.entry(removed).or_insert(0) += 1;
            }
        }
        // An equipment change can add / remove an accessory passive; rebuild
        // the ability bitfields immediately (retail re-derives them every
        // aggregator pass) so menu + battle consumers see the new bits
        // without waiting for the next battle entry.
        world.refresh_party_ability_bits();
    }
    Some(outcome)
}

/// Apply a finished [`InventoryUseSession`] to the world. Folds the
/// stored item outcome through the same path
/// [`crate::world::World::use_item`] uses: HP / MP / status / SP gain.
pub fn apply_inventory_outcome(session: &InventoryUseSession, world: &mut World) {
    use crate::inventory_use::InventoryUseState;
    let InventoryUseState::Done(_) = session.state else {
        return;
    };
    // `used_item` + `used_slots` carry the consumed item and every slot the
    // completed use applied to (one for a single-target item, every healed ally
    // for an all-party item). Forward each to the world via use_item, which
    // folds HP / MP / status / SP. (`current_item` is unavailable here - it
    // returns `None` once the session reaches `Done`.) Stock is decremented
    // once by the menu commit, not here.
    if let Some(id) = session.used_item {
        for &slot in &session.used_slots {
            world.use_item(id, slot);
        }
    }
}

/// Apply a finished [`SpellMenuSession`] cast to the world. For
/// `Cast { caster_slot, spell_id, target_slot, outcome }`, mutates the
/// matching roster MP and target HP.
pub fn apply_spell_outcome(session: &SpellMenuSession, world: &mut World) {
    let Some(SpellMenuOutcome::Cast {
        caster_slot,
        spell_id,
        target_slot,
        outcome,
    }) = session.outcome().cloned()
    else {
        return;
    };
    let mp_cost = session
        .catalog()
        .get(spell_id)
        .map(|d| d.mp_cost)
        .unwrap_or(0);
    if let Some(caster) = world.roster.members.get_mut(caster_slot as usize) {
        let mut hms = caster.hp_mp_sp();
        hms.mp_cur = hms.mp_cur.saturating_sub(mp_cost as u16);
        caster.set_hp_mp_sp(hms);
    }
    if let Some(target) = world.roster.members.get_mut(target_slot as usize) {
        let mut hms = target.hp_mp_sp();
        match outcome {
            crate::spells::SpellOutcome::Heal { amount, .. } => {
                hms.hp_cur = hms.hp_cur.saturating_add(amount).min(hms.hp_max);
            }
            crate::spells::SpellOutcome::Revive { hp, .. } => {
                hms.hp_cur = hp.min(hms.hp_max);
            }
            _ => {}
        }
        target.set_hp_mp_sp(hms);
    }
}

/// Apply a finished [`ChainEditor`] outcome to a [`ChainLibrary`].
pub fn apply_arts_outcome(
    editor: ChainEditor,
    library: &mut ChainLibrary,
) -> Result<(), crate::tactical_arts_editor::SaveError> {
    editor.apply_outcome(library)
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// Resolve the slot of the active leader. Falls back to slot 0 when no
/// leader is set or when the roster is empty.
pub fn active_leader_slot(world: &World) -> u8 {
    world.party_leader_slot.unwrap_or_default()
}

/// Build a [`StatusSnapshot`] for every roster member that has a non-zero
/// max-HP. Engines whose roster carries placeholder zeros (i.e. before
/// the slot is "claimed") will see those filtered out - matching the
/// retail status panel which only shows the active party.
pub fn status_snapshots(world: &World) -> Vec<StatusSnapshot> {
    let names = roster_names(world);
    let mut out = Vec::new();
    for (i, member) in world.roster.members.iter().enumerate() {
        let hms = member.hp_mp_sp();
        if hms.hp_max == 0 {
            continue;
        }
        let xp = member.cumulative_xp();
        // Retail LV is the record's own +0x130 byte (FUN_801D33D8); fall back
        // to base-curve inference for records that never had it stamped.
        let level = match member.magic_rank() {
            l @ 1..=99 => l,
            _ => legaia_save::level_for_cumulative_xp(xp),
        };
        let xp_to_next = xp_to_next_level(member, level);
        // The retail 3x2 derived-stat grid: live values from the `+0x110`
        // window, growth values (the parenthesised number) from the
        // `+0x122..+0x12D` record window, ordered ATK/UDF/LDF | SPD/INT/AGL
        // (docs/subsystems/field-menu.md).
        let live = member.live_stats();
        let growth = member.record_stats();
        let stat_pairs: [(u16, u16); 6] = [
            (live.atk, growth.atk),
            (live.udf, growth.udf),
            (live.ldf, growth.ldf),
            (live.spd, growth.spd),
            (live.int, growth.int),
            (live.agl, growth.agl),
        ];
        let equip_slots = member.equipment();
        let equip_views: Vec<EquipSlotView> = (0..equip_slots.slots.len())
            .map(|s| EquipSlotView {
                label: equip_slot_label(s as u8),
                // Empty slots stay blank (retail draws only the slot
                // pictogram); occupied slots show the raw id until the
                // item-name table is wired through here.
                item_name: match equip_slots.slots[s] {
                    0 => String::new(),
                    id => format!("#{id:02X}"),
                },
            })
            .collect();
        out.push(StatusSnapshot {
            slot: i as u8,
            name: names.get(i).cloned().unwrap_or_else(|| format!("Slot {i}")),
            level,
            xp,
            xp_to_next,
            hp: hms.hp_cur,
            hp_max: hms.hp_max,
            mp: hms.mp_cur,
            mp_max: hms.mp_max,
            ap: world.ap_gauges.get(i).map(|g| g.current_ap).unwrap_or(0),
            ap_max: 100,
            attack: world.battle_attack.get(i).copied().unwrap_or(0),
            defense: world.battle_defense.get(i).copied().unwrap_or(0),
            stats: stat_pairs,
            stat_labels: crate::status_screen::RETAIL_STAT_LABELS,
            equip: equip_views,
            elements: default_element_views(),
        });
    }
    out
}

/// The Status-menu "Next Level" number. Retail (`FUN_801D33D8`) draws the
/// record's next-level-threshold word (`+0x4`) **verbatim** - the cumulative
/// XP total at which the next level lands, NOT the remaining difference.
/// Records that never had `+0x4` stamped (engine-synthesized rosters) fall
/// back to the base-curve threshold; at L99 retail carries 0 there.
// REF: FUN_801D33D8 (Next Level draw), FUN_801E9504 (threshold writer)
fn xp_to_next_level(member: &legaia_save::CharacterRecord, level: u8) -> u32 {
    match member.next_level_xp() {
        0 if level < 99 => legaia_save::xp_for_level(level + 1),
        threshold => threshold,
    }
}

fn equip_slot_label(slot: u8) -> &'static str {
    const LABELS: [&str; 8] = [
        "Weapon", "Armour", "Helmet", "Ring", "Acc 1", "Acc 2", "Acc 3", "Misc",
    ];
    LABELS.get(slot as usize).copied().unwrap_or("Slot")
}

fn default_element_views() -> Vec<ElementRankView> {
    [
        "Fire", "Water", "Earth", "Wind", "Light", "Dark", "Thunder", "Bio",
    ]
    .iter()
    .map(|l| ElementRankView { label: l, rank: 0 })
    .collect()
}

/// Engine-friendly placeholder names. Engines with character-name data
/// can override by building their own [`StatusSnapshot`] vector.
pub fn roster_names(world: &World) -> Vec<String> {
    let canonical = ["Vahn", "Noa", "Gala"];
    world
        .roster
        .members
        .iter()
        .enumerate()
        .map(|(i, _)| {
            canonical
                .get(i)
                .map(|s| (*s).to_string())
                .unwrap_or_else(|| format!("Slot {i}"))
        })
        .collect()
}

fn build_spell_session(world: &World, catalog: &SpellCatalog) -> SpellMenuSession {
    let names = roster_names(world);
    let party: Vec<SpellCasterSlot> = world
        .roster
        .members
        .iter()
        .enumerate()
        .map(|(i, member)| {
            let hms = member.hp_mp_sp();
            let spells = member.spell_list().ids[..member.spell_list().count as usize].to_vec();
            SpellCasterSlot {
                slot: i as u8,
                name: names.get(i).cloned().unwrap_or_default(),
                hp: hms.hp_cur,
                mp: hms.mp_cur,
                spells,
            }
        })
        .collect();
    let targets: Vec<SpellTargetRow> = world
        .roster
        .members
        .iter()
        .enumerate()
        .map(|(i, member)| {
            let hms = member.hp_mp_sp();
            SpellTargetRow {
                slot: i as u8,
                name: names.get(i).cloned().unwrap_or_default(),
                hp: hms.hp_cur,
                hp_max: hms.hp_max,
            }
        })
        .collect();
    SpellMenuSession::new(party, targets, catalog.clone())
}

fn build_inventory_session(world: &World) -> InventoryUseSession {
    let names = roster_names(world);
    let items: Vec<u8> = world
        .inventory
        .iter()
        .filter_map(|(id, qty)| if *qty > 0 { Some(*id) } else { None })
        .collect();
    let targets: Vec<InvTargetRow> = world
        .roster
        .members
        .iter()
        .enumerate()
        .map(|(i, member)| {
            let hms = member.hp_mp_sp();
            let mut row = InvTargetRow::new(i as u8, names.get(i).cloned().unwrap_or_default())
                .with_stats(hms.hp_cur, hms.hp_max, hms.mp_cur, hms.mp_max)
                .with_statuses(
                    world
                        .status_effects
                        .statuses(i as u8)
                        .iter()
                        .map(|s| s.kind),
                );
            // A fallen ally (HP 0) gates revive items in / heals out.
            row.alive = !(hms.hp_cur == 0 && hms.hp_max > 0);
            row
        })
        .collect();
    InventoryUseSession::new(
        world.item_catalog.clone(),
        items,
        targets,
        InventoryContext::Field,
    )
}

fn build_equip_session(world: &World, char_slot: u8, equipment: &EquipmentTable) -> EquipSession {
    let record = world
        .roster
        .members
        .get(char_slot as usize)
        .map(stat_record_from_character)
        .unwrap_or_default();
    EquipSession::new(
        record,
        world.inventory.clone(),
        equipment.clone(),
        StatusModifiers::default(),
        Vec::new(),
    )
}

fn stat_record_from_character(c: &legaia_save::CharacterRecord) -> StatRecord {
    let eq_bytes = c.equipment().slots;
    let live = c.live_stats();
    StatRecord {
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

fn inventory_input_from_pad(pressed: u16) -> Option<InventoryUseInput> {
    if pressed & PadButton::Up.mask() != 0 {
        Some(InventoryUseInput::Up)
    } else if pressed & PadButton::Down.mask() != 0 {
        Some(InventoryUseInput::Down)
    } else if pressed & PadButton::Cross.mask() != 0 {
        Some(InventoryUseInput::Confirm)
    } else if pressed & PadButton::Circle.mask() != 0 {
        Some(InventoryUseInput::Cancel)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_menu::FieldMenuRow;

    fn fresh_world() -> World {
        let mut world = World::new();
        // Three placeholder records with non-zero max HP/MP so the
        // status / spell builders include them.
        world.roster = legaia_save::Party::zeroed(3);
        for member in &mut world.roster.members {
            let mut hms = member.hp_mp_sp();
            hms.hp_cur = 50;
            hms.hp_max = 100;
            hms.mp_cur = 10;
            hms.mp_max = 30;
            member.set_hp_mp_sp(hms);
        }
        world.inventory.insert(0x77, 3); // Healing Leaf (real item id)
        world.party_leader_slot = Some(0);
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        world
    }

    fn fresh_save_slots() -> Vec<SlotSnapshot> {
        (0..3).map(SlotSnapshot::empty).collect()
    }

    fn build(row: FieldMenuRow, world: &World) -> FieldMenuSubsession {
        FieldMenuSubsession::build(
            row,
            world,
            &OptionsState::default(),
            &fresh_save_slots(),
            &ChainLibrary::new(),
            &SpellCatalog::vanilla(),
            &EquipmentTable::new(),
        )
    }

    #[test]
    fn build_items_returns_inventory_session() {
        let w = fresh_world();
        let s = build(FieldMenuRow::Items, &w);
        assert_eq!(s.row(), FieldMenuRow::Items);
        assert!(matches!(s, FieldMenuSubsession::Items(_)));
    }

    #[test]
    fn build_status_snapshots_skip_empty_roster_slots() {
        let mut w = fresh_world();
        // Zero one member's max HP - they should drop out of the snapshot.
        let mut hms = w.roster.members[2].hp_mp_sp();
        hms.hp_max = 0;
        w.roster.members[2].set_hp_mp_sp(hms);
        let snaps = status_snapshots(&w);
        assert_eq!(snaps.len(), 2);
    }

    #[test]
    fn build_spells_session_population() {
        let w = fresh_world();
        let s = build(FieldMenuRow::Spells, &w);
        match s {
            FieldMenuSubsession::Spells(sm) => {
                assert_eq!(sm.party().len(), 3);
                assert_eq!(sm.targets().len(), 3);
            }
            _ => panic!("expected Spells variant"),
        }
    }

    #[test]
    fn build_save_uses_save_mode() {
        let w = fresh_world();
        let s = build(FieldMenuRow::Save, &w);
        match s {
            FieldMenuSubsession::Save(ss) => {
                assert_eq!(ss.mode(), SaveSelectMode::Save);
                assert_eq!(ss.slots().len(), 3);
            }
            _ => panic!("expected Save"),
        }
    }

    #[test]
    fn build_equip_uses_active_leader() {
        let mut w = fresh_world();
        w.party_leader_slot = Some(2);
        let s = build(FieldMenuRow::Equip, &w);
        match s {
            FieldMenuSubsession::Equip { char_slot, .. } => assert_eq!(char_slot, 2),
            _ => panic!("expected Equip"),
        }
    }

    #[test]
    fn build_options_seeds_state_from_input() {
        let w = fresh_world();
        let opts = OptionsState {
            bgm_volume: 3,
            ..OptionsState::default()
        };
        let s = FieldMenuSubsession::build(
            FieldMenuRow::Config,
            &w,
            &opts,
            &fresh_save_slots(),
            &ChainLibrary::new(),
            &SpellCatalog::vanilla(),
            &EquipmentTable::new(),
        );
        match s {
            FieldMenuSubsession::Config(o) => assert_eq!(o.state().bgm_volume, 3),
            _ => panic!("expected Config"),
        }
    }

    #[test]
    fn tick_pad_edge_status_circle_closes() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Status, &w);
        assert!(!s.is_done());
        s.tick_pad_edge(PadButton::Circle.mask());
        assert!(s.is_done());
    }

    #[test]
    fn tick_pad_edge_options_circle_cancels() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Config, &w);
        s.tick_pad_edge(PadButton::Circle.mask());
        assert!(s.is_done());
    }

    #[test]
    fn tick_pad_edge_save_circle_cancels() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Save, &w);
        s.tick_pad_edge(PadButton::Circle.mask());
        assert!(s.is_done());
    }

    #[test]
    fn tick_pad_edge_equip_circle_cancels() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Equip, &w);
        s.tick_pad_edge(PadButton::Circle.mask());
        assert!(s.is_done());
    }

    #[test]
    fn tick_pad_edge_inventory_circle_cancels() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Items, &w);
        s.tick_pad_edge(PadButton::Circle.mask());
        assert!(s.is_done());
    }

    #[test]
    fn tick_pad_edge_spells_circle_cancels() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Spells, &w);
        s.tick_pad_edge(PadButton::Circle.mask());
        assert!(s.is_done());
    }

    #[test]
    fn apply_equip_outcome_writes_back_to_roster() {
        let mut w = fresh_world();
        // EquipSession's items_for_slot encodes target slot in the upper
        // 3 bits (slot = id >> 5). Use 0x25 for slot 1 so we don't
        // collide with the Healing Leaf (id 0x01, which also sorts into
        // slot 0).
        w.inventory.clear();
        w.inventory.insert(0x25, 1);
        let mut equip_table = EquipmentTable::new();
        equip_table.set(0x25, crate::battle_stats::ItemModifier::default());
        let mut s = FieldMenuSubsession::build(
            FieldMenuRow::Equip,
            &w,
            &OptionsState::default(),
            &fresh_save_slots(),
            &ChainLibrary::new(),
            &SpellCatalog::vanilla(),
            &equip_table,
        );
        // Slot picker starts at cursor 0; move down once to reach slot 1
        // (where item 0x25 lives), confirm into the item picker, confirm
        // the single item, confirm Yes.
        s.tick_pad_edge(PadButton::Down.mask());
        for _ in 0..3 {
            s.tick_pad_edge(PadButton::Cross.mask());
        }
        assert!(s.is_done());
        if let FieldMenuSubsession::Equip { session, char_slot } = &s {
            let outcome = apply_equip_outcome(session, *char_slot, &mut w);
            assert!(matches!(outcome, Some(EquipOutcome::Committed { .. })));
            // Roster member 0's slot 1 byte now matches the equipped id.
            assert_eq!(w.roster.members[0].equipment().slots[1], 0x25);
            // ...and the equipped item LEFT the bag (no duplication). Slot 1 was
            // empty, so nothing is returned.
            assert_eq!(
                w.inventory.get(&0x25),
                None,
                "equipped item must be removed from the bag (no duplication)"
            );
        } else {
            panic!("expected Equip variant");
        }
    }

    /// Equipping over an occupied slot returns the swapped-out item to the bag
    /// and removes the newly-equipped one - no item loss, no duplication.
    #[test]
    fn apply_equip_outcome_returns_the_swapped_out_item_to_the_bag() {
        let mut w = fresh_world();
        w.inventory.clear();
        w.inventory.insert(0x25, 1);
        // Pre-equip a different slot-1 item (0x26 >> 5 == 1) on member 0.
        let mut eq = w.roster.members[0].equipment();
        eq.slots[1] = 0x26;
        w.roster.members[0].set_equipment(eq);

        let mut equip_table = EquipmentTable::new();
        equip_table.set(0x25, crate::battle_stats::ItemModifier::default());
        equip_table.set(0x26, crate::battle_stats::ItemModifier::default());
        let mut s = FieldMenuSubsession::build(
            FieldMenuRow::Equip,
            &w,
            &OptionsState::default(),
            &fresh_save_slots(),
            &ChainLibrary::new(),
            &SpellCatalog::vanilla(),
            &equip_table,
        );
        s.tick_pad_edge(PadButton::Down.mask());
        for _ in 0..3 {
            s.tick_pad_edge(PadButton::Cross.mask());
        }
        assert!(s.is_done());
        let FieldMenuSubsession::Equip { session, char_slot } = &s else {
            panic!("expected Equip variant");
        };
        let outcome = apply_equip_outcome(session, *char_slot, &mut w);
        assert!(matches!(
            outcome,
            Some(EquipOutcome::Committed {
                removed: 0x26,
                added: 0x25,
                ..
            })
        ));
        assert_eq!(w.roster.members[0].equipment().slots[1], 0x25);
        // 0x25 left the bag, 0x26 came back into it.
        assert_eq!(w.inventory.get(&0x25), None, "equipped item left the bag");
        assert_eq!(
            w.inventory.get(&0x26),
            Some(&1),
            "swapped-out item returned to the bag"
        );
    }
}
