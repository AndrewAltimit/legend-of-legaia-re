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
use crate::input::{Mapping, PadButton};
use crate::inventory_use::{InventoryContext, InventoryUseSession, TargetRow as InvTargetRow};
use crate::list_order::{LIST_ORDER_STEP_MAGIC, ListOrderRow, ListOrderSession};
use crate::magic_xp::SpellLevelNotice;
use crate::options::{OptionsInput, OptionsSession, OptionsState};
use crate::pause_screens::{PauseItemRow, PauseItemsSession};
use crate::save_select::{SaveRack, SaveSelectMode, SaveSelectSession, SelectInput};
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
    /// The retail Items screen (command window + 12-row list pages +
    /// info window) layered over the item-use flow. The inner
    /// [`InventoryUseSession`] stays the outcome carrier -
    /// [`apply_inventory_outcome`] takes `&session.inner`.
    Items(PauseItemsSession),
    /// Equip session paired with the slot of the character whose record is
    /// being edited so the caller can write the result back to the right
    /// roster member.
    ///
    /// `picking` is retail's character picker `0x12` (`FUN_801D98F0`), the
    /// Equip row's first step: the pad walks the present party and the slot
    /// rows show the hovered member's equipment; a confirm hands the pad to
    /// the slot browse (`0x13`), whose cancel comes back here.
    Equip {
        session: EquipSession,
        char_slot: u8,
        picking: bool,
    },
    Spells(SpellMenuSession),
    /// The per-character list page with the spell list's **reorder**.
    ///
    /// Not a pause-menu row of its own. It is the list half of menu-overlay
    /// sub-screen `0x15` (`FUN_801DA2A0`), and `0x15` is what the root
    /// picker's **row 3 - Status** routes to: `0x801D6C4C` is the only site
    /// in PROT 0899 that writes `0x15` into the submenu word `DAT_801E46A4`,
    /// and it sits in the root picker `FUN_801D6B20`'s row-3 arm. So the
    /// screen this page hangs off is the Status screen, and a confirm there
    /// is retail's own entry to it.
    ListOrder(ListOrderSession),
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
    /// `save_rack` is the model behind the Load / Save rows - and its kind
    /// is what puts the session in retail's two-stage card flow, so a host
    /// never decides that separately. See [`SaveRack`].
    pub fn build(
        row: FieldMenuRow,
        world: &World,
        options: &OptionsState,
        save_rack: &SaveRack,
        chain_library: &ChainLibrary,
        spell_catalog: &SpellCatalog,
        equipment_table: &EquipmentTable,
    ) -> Self {
        // `chain_library` remains a build input so engines can construct
        // the Arts editor variant directly (no retail pause-menu row
        // opens it - retail's list is Items / Magic / Equip / Status /
        // Options / Load / Save).
        let _ = chain_library;
        match row {
            FieldMenuRow::Items => Self::Items(build_pause_items_session(world)),
            FieldMenuRow::Equip => {
                // The picker opens on the leader - the head of the present
                // party list retail's picker walks.
                let first = active_leader_slot(world);
                let mut session = build_equip_session(world, first, equipment_table);
                session.set_slot_cursor_hidden(true);
                Self::Equip {
                    session,
                    char_slot: first,
                    picking: true,
                }
            }
            FieldMenuRow::Magic => Self::Spells(build_spell_session(world, spell_catalog)),
            FieldMenuRow::Status => Self::Status(
                StatusScreenSession::new(status_snapshots(world))
                    .with_spell_rows(status_spell_rows(world, spell_catalog)),
            ),
            FieldMenuRow::Load => {
                Self::Save(SaveSelectSession::for_rack(SaveSelectMode::Load, save_rack))
            }
            FieldMenuRow::Save => {
                Self::Save(SaveSelectSession::for_rack(SaveSelectMode::Save, save_rack))
            }
            FieldMenuRow::Options => Self::Config(OptionsSession::new(options.clone())),
        }
    }

    /// Return the [`FieldMenuRow`] this subsession was built for.
    pub fn row(&self) -> FieldMenuRow {
        match self {
            Self::Items(_) => FieldMenuRow::Items,
            Self::Equip { .. } => FieldMenuRow::Equip,
            Self::Spells(_) => FieldMenuRow::Magic,
            // The page hangs off the Status screen (retail's sub-screen
            // `0x15`), so a resume drops the hand back on that row.
            Self::ListOrder(_) => FieldMenuRow::Status,
            // The Arts chain editor is an engine extension with no
            // retail pause-menu row; park the resume cursor on Status
            // (the retail surface that lists a character's arts).
            Self::Arts(_) => FieldMenuRow::Status,
            Self::Status(_) => FieldMenuRow::Status,
            Self::Save(s) => match s.mode() {
                SaveSelectMode::Load => FieldMenuRow::Load,
                _ => FieldMenuRow::Save,
            },
            Self::Config(_) => FieldMenuRow::Options,
        }
    }

    /// Arm the Options sub-session's engine-only **Key Config** row against
    /// the host's live keyboard binding table. No-op on every other variant.
    ///
    /// A post-construction hook rather than a [`Self::build`] argument: the
    /// binding table is a host concern (the native binary's
    /// `legaia-input.toml`, the page's `localStorage` entry) and every
    /// headless caller of `build` - replay drivers, oracles, the disc-gated
    /// menu tests - has no table to offer and should keep the retail ten
    /// rows.
    pub fn arm_key_rebind(&mut self, mapping: Mapping) {
        if let Self::Config(s) = self {
            s.arm_key_rebind(mapping);
        }
    }

    /// The Options sub-session's live binding table once a rebind committed,
    /// paired with "it changed since you last asked". `None` on every other
    /// variant, and on an Options session whose Key Config row was never
    /// armed.
    ///
    /// The host's persist cue: native writes `legaia-input.toml`, the page
    /// writes the binding key it restores from on load.
    pub fn take_rebound_mapping(&mut self) -> Option<Mapping> {
        let Self::Config(s) = self else {
            return None;
        };
        if !s.take_bindings_dirty() {
            return None;
        }
        s.mapping().cloned()
    }

    /// Drive one frame using a PSX-encoded edge-triggered "newly pressed"
    /// pad bitmask. Each variant's tick method receives the matching
    /// per-button input bundle.
    pub fn tick_pad_edge(&mut self, pressed: u16) {
        self.tick_pad_edge_with_key(pressed, None);
    }

    /// [`Self::tick_pad_edge`] carrying the host's most-recent keyboard key
    /// name, in the engine's own key vocabulary
    /// ([`crate::input::KEY_NAME_DOM_CODES`]). Consumed only by the Options
    /// sub-session's key-rebind screen while it awaits a key; every other
    /// variant ignores it.
    pub fn tick_pad_edge_with_key(&mut self, pressed: u16, key_pressed: Option<&str>) {
        match self {
            Self::Items(s) => s.input_pad_edge(pressed),
            Self::Equip {
                session, picking, ..
            } if *picking => {
                // Retail's character picker `0x12` (`FUN_801D98F0`): confirm
                // hands the pad to the slot browse `0x13`, cancel leaves the
                // Equip screen. Moving between members needs the world to
                // rebuild the session, so [`tick_open_subsession`] takes the
                // Up / Down edges before this arm.
                if pressed & PadButton::Cross.mask() != 0 {
                    *picking = false;
                    session.set_slot_cursor_hidden(false);
                    session.reopen_slot_browse();
                } else if pressed & PadButton::Circle.mask() != 0 {
                    session.cancel();
                }
            }
            Self::Equip {
                session, picking, ..
            } => {
                session.input(EquipInput {
                    up: pressed & PadButton::Up.mask() != 0,
                    down: pressed & PadButton::Down.mask() != 0,
                    left: pressed & PadButton::Left.mask() != 0,
                    right: pressed & PadButton::Right.mask() != 0,
                    cross: pressed & PadButton::Cross.mask() != 0,
                    circle: pressed & PadButton::Circle.mask() != 0,
                    triangle: pressed & PadButton::Triangle.mask() != 0,
                });
                // The slot browse's cancel returns to the character picker
                // (`0x13` cancel -> `0x12`), not to the root list.
                if session.outcome() == Some(EquipOutcome::Cancelled) {
                    *picking = true;
                    session.set_slot_cursor_hidden(true);
                    session.reopen_slot_browse();
                }
            }
            Self::Spells(s) => {
                let _ = s.tick(SpellMenuInput::from_pad_edge(pressed));
            }
            Self::ListOrder(s) => {
                let _ = s.tick(pressed);
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
                // Confirm on the shown character opens the reorder page -
                // retail's own route, the confirm arm of sub-screen `0x15`'s
                // character picker (`0x801DA3C8`). An empty list takes the
                // reject arm (`ListOrderSession::open` answers `None`) and
                // the press does nothing, exactly as retail buzzes and stays.
                if pressed & PadButton::Cross.mask() != 0
                    && let Some(order) = open_status_list_order(s)
                {
                    *self = Self::ListOrder(order);
                    return;
                }
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
                let _ = s.tick_with_key(OptionsInput::from_pad_edge(pressed), key_pressed);
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
            Self::ListOrder(s) => s.is_done(),
            Self::Arts(s) => s.is_done(),
            Self::Status(s) => s.is_done(),
            Self::Save(s) => s.is_done(),
            Self::Config(s) => s.is_done(),
        }
    }
}

/// Apply a finished [`EquipSession`] to a `world.party.roster` member. Returns
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
        && let Some(member) = world.party.roster.members.get_mut(char_slot as usize)
    {
        let mut eq = member.equipment();
        // The outcome names an engine slot; the record byte is retail's.
        let byte = crate::equip_session::record_byte_for_engine_slot(slot, char_slot);
        if byte < eq.slots.len() {
            eq.slots[byte] = added;
            member.set_equipment(eq);
            // Reconcile the bag with the swap the session computed on its own
            // (cloned) copy: the newly-equipped item leaves inventory, the
            // swapped-out item (if any) returns to it. Without this the equipped
            // item stays in the bag (duplication) and the old one is lost.
            if added != 0
                && let Some(qty) = world.party.inventory.get_mut(&added)
            {
                *qty = qty.saturating_sub(1);
                if *qty == 0 {
                    world.party.inventory.remove(&added);
                }
            }
            if removed != 0 {
                *world.party.inventory.entry(removed).or_insert(0) += 1;
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
/// [`crate::world::World::use_item`] uses: HP / MP / status / SP gain, and
/// commits the bag decrement every one of those beats owes.
///
/// Three consumption channels, and they are not interchangeable:
///
/// - [`InventoryUseSession::thrown_items`] - a Throw Out confirm, which
///   zeroes the **whole** bag-slot pair (`FUN_801D8734` phase 3);
/// - [`InventoryUseSession::consumed_items`] - a special Use route's
///   `FUN_80042310(id, 1)`, **one** copy each;
/// - [`InventoryUseSession::used_item`] - the ordinary use's own commit,
///   one copy however many targets `used_slots` names (retail decrements
///   once per use, not once per healed ally).
///
/// The first two apply however the session ended; `used_item` is only ever
/// `Some` alongside [`crate::inventory_use::InventoryUseState::Done`],
/// which is why the state gate sits on the effect loop rather than on the
/// whole body - a Door of Light closes the screen with the inner flow still
/// browsing, and its consumption must survive that.
// REF: FUN_801D8734 (throw-out delete)
// REF: FUN_80042310 (the one-copy bag decrement all three routes call)
pub fn apply_inventory_outcome(session: &InventoryUseSession, world: &mut World) {
    use crate::inventory_use::InventoryUseState;
    // Throw Out discards the **slot** the row named, which is what retail's
    // confirm zeroes. Only a session built without a slot-indexed bag falls
    // back to the id, and that removal cannot tell two stacks of one id apart.
    for (i, id) in session.thrown_items.iter().enumerate() {
        match session.thrown_slots.get(i) {
            Some(&slot) => {
                world.discard_bag_slot(slot);
            }
            None => {
                world.party.inventory.remove(id);
            }
        }
    }
    for &id in &session.consumed_items {
        world.consume_item(id);
    }
    if let Some(id) = session.used_item {
        match session.used_slot {
            Some(slot) => {
                world.consume_bag_slot(slot);
            }
            None => world.consume_item(id),
        }
        if matches!(session.state, InventoryUseState::Done(_)) {
            // `used_slots` names every slot the completed use applied to
            // (one for a single-target item, every healed ally for an
            // all-party one). `current_item` is unavailable here - it
            // returns `None` once the session reaches `Done`.
            for &slot in &session.used_slots {
                if let crate::items::ItemOutcome::ArtLearned {
                    character, art_id, ..
                } = world.use_item(id, slot)
                {
                    // Retail's applier calls `FUN_80035C00(slot, art)` here
                    // and the Items use sub-screen opens window 8 on it.
                    world.menu.pending_art_notice = art_learned_notice(world, character, art_id);
                }
            }
        }
    }
}

/// Compose the window-8 "learned an art" notice: the disc template
/// (`0x801E4700`) patched the way `FUN_801DCD58` patches it
/// ([`crate::pause_screens::patch_notify_template`]) and expanded against the
/// party names and the arts-name table.
///
/// `None` when the menu overlay's template is not installed - the notice is
/// disc text and the engine does not invent it.
pub fn art_learned_notice(
    world: &World,
    character: u8,
    art_id: u8,
) -> Option<crate::pause_screens::ArtLearnedNotice> {
    let mut template = world.menu.notify_template.clone()?;
    crate::pause_screens::patch_notify_template(&mut template, i16::from(character), art_id);
    let names = &world.party.party_names;
    let lines = crate::pause_screens::expand_notify_lines(
        &template,
        |slot| {
            // `0xC1 0x63` names the leader (`DAT_80084597`); any other
            // operand is a roster slot.
            let i = if slot == 0x63 { 0 } else { usize::from(slot) };
            names.get(i).cloned()
        },
        |ch, art| {
            world
                .menu
                .text
                .as_ref()
                .and_then(|t| t.art_name(ch, art))
                .map(str::to_string)
        },
    );
    Some(crate::pause_screens::ArtLearnedNotice {
        character,
        art_id,
        lines,
    })
}

/// Apply a finished pause **Items screen** to the world: the inner use
/// flow's outcome ([`apply_inventory_outcome`]) plus the special Use
/// routes' menu-exit handoff, which the inner flow has no field for.
///
/// Returns the menu exit code the screen handed the outer menu SM
/// (retail `_DAT_8007B43C`), so a host that wants the fade can key on it.
///
/// Hosts that only call [`apply_inventory_outcome`] still get every bag
/// decrement - the consumption rides `session.inner`. What this adds is the
/// escape / warp handoff, which is world state rather than bag state.
pub fn apply_pause_items_outcome(
    session: &crate::pause_screens::PauseItemsSession,
    world: &mut World,
) -> Option<u32> {
    apply_inventory_outcome(&session.inner, world);
    // Incense (item `0x8A`, effect class `0x82`): the confirm runs the SCUS
    // item applier, whose class-`0x82` arm (`0x800421A0`) is one
    // `jal 0x80046870` - `+0x40` walk ticks on `_DAT_8007B600`, capped at
    // `0x100`. The field walk tick drains it and the region encounter roll
    // skips while it is non-zero (`World::on_field_step`).
    for _ in 0..session.incense_uses() {
        world.locomotion.walk_regen_window =
            legaia_engine_vm::battle_helpers::top_up_cooldown(world.locomotion.walk_regen_window);
    }
    if let Some(warp) = session.staged_warp() {
        world.menu.pending_warp = Some(warp);
    }
    if session.exit_code() == Some(crate::pause_screens::MENU_EXIT_CODE_FIELD_ESCAPE) {
        world.menu.pending_escape = true;
    }
    session.exit_code()
}

/// Apply a finished [`SpellMenuSession`] cast to the world. For
/// `Cast { caster_slot, spell_id, target_slot, outcome }`, mutates the
/// matching roster MP and target HP, then runs the **menu-cast leveling
/// arm** and returns the window-7 notification when the cast leveled the
/// spell.
///
/// PORT: FUN_800402F4 (effect-apply handler, HP-heal arms) - a menu heal
/// accrues a flat grant into the caster record's per-spell XP accumulator
/// (`+0x5D0` off the `0x80084140` save-context window = record `+0x8`,
/// [`crate::magic_xp::SPELL_XP_OFFSET`]): `+12` when the target's deficit
/// covered the full heal, `+4` when the heal was clipped (`+3` / `+1` per
/// member on the multi-target arm). Outside battle the same handler then
/// tests the `0x8007656C` threshold table against the accumulator, bumps
/// the `+0x161 + slot` level byte (cap 9) and calls the notification
/// setter `FUN_80035C00(slot, index)` - the `(_DAT_8007BB70,
/// _DAT_8007BB78)` pair that opens window 7. The accrual + threshold walk
/// is the shared kernel [`crate::magic_xp::accrue_and_level`] (the battle
/// summon path drives the same one); the returned
/// [`SpellLevelNotice`] is retail's setter pair plus the assembled prompt
/// line, for the host to park on
/// [`crate::menu_runtime::MenuRuntime::arm_spell_level_notice`].
///
/// Both writes land in the roster's [`legaia_save::CharacterRecord`] raw
/// bytes, so the accumulator and the level round-trip through LGSF saves
/// unchanged.
pub fn apply_spell_outcome(
    session: &SpellMenuSession,
    world: &mut World,
) -> Option<SpellLevelNotice> {
    let Some(SpellMenuOutcome::Cast {
        caster_slot,
        spell_id,
        target_slot,
        outcome,
    }) = session.outcome().cloned()
    else {
        return None;
    };
    let def = session.catalog().get(spell_id).cloned();
    // Retail debits the **discounted** price: `jal 0x80035394` at
    // `0x801D93C0` (group cast: `0x801D972C`), then `record+0x10A -= v0`
    // at `0x801D9404..0x801D9418` - the same number the list build greyed
    // the row against, so the gate and the charge cannot disagree.
    let mp_cost = def
        .as_ref()
        .map(|d| {
            session
                .party()
                .iter()
                .find(|c| c.slot == caster_slot)
                .map(|c| c.mp_cost(d))
                .unwrap_or(d.mp_cost as u16)
        })
        .unwrap_or(0);
    if let Some(caster) = world.party.roster.members.get_mut(caster_slot as usize) {
        let mut hms = caster.hp_mp_sp();
        hms.mp_cur = hms.mp_cur.saturating_sub(mp_cost);
        caster.set_hp_mp_sp(hms);
    }
    // Every record write is projected onto the party actor that mirrors it
    // (`World::mirror_roster_hp_mp`): a battle seats from that copy and
    // `save_party` writes it back, so a record-only write is undone.
    world.mirror_roster_hp_mp(caster_slot as usize);
    // Menu-cast spell-XP arm: only the HP-heal effect classes accrue
    // (FUN_800402F4's revive / cure / MP arms carry no `+0x5D0` code).
    //
    // Retail "full power": the deficit covered the spell's whole heal cap;
    // a clipped heal is the partial grant. The engine's cap analogue is the
    // catalog's nominal amount ([`crate::spells::cast_spell`] returns
    // `min(nominal, deficit)`), and the group arm credits it per member -
    // `+3` full / `+1` clipped each, against the single cast's `+12` / `+4`.
    let nominal_heal = match def.as_ref().map(|d| &d.effect) {
        Some(crate::spells::SpellEffect::Heal { amount }) => Some((*amount, false)),
        Some(crate::spells::SpellEffect::HealAll { amount }) => Some((*amount, true)),
        _ => None,
    };
    // The group flow (retail sub-screen `0x10`) picks no row, so its outcome
    // carries the per-member grants instead of one `target_slot`.
    let gain = if let crate::spells::SpellOutcome::MultiHeal { targets } = &outcome {
        let (nominal, _) = nominal_heal?;
        let mut total = 0u32;
        for (slot, amount) in targets {
            if let Some(member) = world.party.roster.members.get_mut(*slot as usize) {
                let mut hms = member.hp_mp_sp();
                hms.hp_cur = hms.hp_cur.saturating_add(*amount).min(hms.hp_max);
                member.set_hp_mp_sp(hms);
            }
            world.mirror_roster_hp_mp(*slot as usize);
            total += crate::magic_xp::menu_heal_xp_gain(true, *amount == nominal);
        }
        if total == 0 {
            return None;
        }
        total
    } else {
        let mut healed: Option<u16> = None;
        if let Some(target) = world.party.roster.members.get_mut(target_slot as usize) {
            let mut hms = target.hp_mp_sp();
            match outcome {
                crate::spells::SpellOutcome::Heal { amount, .. } => {
                    hms.hp_cur = hms.hp_cur.saturating_add(amount).min(hms.hp_max);
                    healed = Some(amount);
                }
                crate::spells::SpellOutcome::Revive { hp, .. } => {
                    hms.hp_cur = hp.min(hms.hp_max);
                }
                _ => {}
            }
            target.set_hp_mp_sp(hms);
        }
        world.mirror_roster_hp_mp(target_slot as usize);
        let healed = healed?;
        let (nominal, group_cast) = nominal_heal?;
        crate::magic_xp::menu_heal_xp_gain(group_cast, healed == nominal)
    };
    let thresholds = world.tables.magic_xp_thresholds;
    let record = world.party.roster.members.get_mut(caster_slot as usize)?;
    let up = crate::magic_xp::accrue_and_level(
        record,
        spell_id,
        gain,
        thresholds.as_ref().map(|t| t.as_slice()),
    )?;
    let spell_name = def
        .map(|d| d.name)
        .unwrap_or_else(|| format!("Spell {spell_id:#04X}"));
    Some(SpellLevelNotice {
        caster_slot,
        spell_index: up.spell_slot as u8,
        spell_id,
        new_level: up.new_level,
        line: crate::magic_xp::magic_level_increased_message(&spell_name),
    })
}

// ---------------------------------------------------------------------------
// The pause-menu stack - the steps every host shares
// ---------------------------------------------------------------------------

/// Step the pause menu's **root list** one frame on a raw pad edge. Returns
/// the row the confirm suspended the list on, for the host to build that
/// row's sub-session ([`FieldMenuSubsession::build`]) with its own rack and
/// key table.
pub fn tick_root_list(
    menu: &mut crate::field_menu::FieldMenuSession,
    edge: u16,
) -> Option<FieldMenuRow> {
    let _ = menu.tick(crate::field_menu::FieldMenuInput::from_pad_edge(edge));
    match menu.phase() {
        crate::field_menu::FieldMenuPhase::Suspended { row } => Some(row),
        _ => None,
    }
}

/// Step an open sub-session one frame on a raw pad edge (and the host's
/// latest key name, for the Options screen's Key Config row). Returns the
/// rebound binding table when a rebind committed this frame, for the host
/// to adopt and persist.
///
/// Carries the engine extension every host offers: Triangle on the Status
/// screen swaps it for the Tactical Arts chain editor
/// ([`try_open_arts_editor`]), and the edge that did so drives nothing else.
/// A Load / Save sub-session's card flow (`SaveScreenFlow::before_tick`)
/// filters `edge` before this call; that half is the host's because the
/// card read is rack I/O.
pub fn tick_open_subsession(
    active: &mut FieldMenuSubsession,
    edge: u16,
    key_pressed: Option<&str>,
    world: &World,
) -> Option<Mapping> {
    if step_equip_character_picker(active, edge, world) {
        return None;
    }
    if !try_open_arts_editor(active, edge, world) {
        active.tick_pad_edge_with_key(edge, key_pressed);
    }
    active.take_rebound_mapping()
}

/// The Equip character picker's Up / Down: walk the present party
/// (`DAT_80084594` over `0x80084598`, [`World::present_party_list`]) and
/// rebuild the session on the hovered member, so the slot rows show that
/// member's equipment as retail's main window does. Returns `true` when the
/// edge was taken.
///
/// REF: FUN_801D98F0
fn step_equip_character_picker(active: &mut FieldMenuSubsession, edge: u16, world: &World) -> bool {
    let FieldMenuSubsession::Equip {
        session,
        char_slot,
        picking: true,
    } = active
    else {
        return false;
    };
    let up = edge & PadButton::Up.mask() != 0;
    let down = edge & PadButton::Down.mask() != 0;
    if !(up || down) {
        return false;
    }
    let list = world.present_party_list();
    let at = list.iter().position(|&s| s == *char_slot).unwrap_or(0);
    let next = if down {
        (at + 1).min(list.len().saturating_sub(1))
    } else {
        at.saturating_sub(1)
    };
    if let Some(&slot) = list.get(next)
        && slot != *char_slot
    {
        let mut rebuilt = build_equip_session(world, slot, &world.tables.equipment_table);
        rebuilt.set_slot_cursor_hidden(true);
        *session = rebuilt;
        *char_slot = slot;
    }
    true
}

/// What a finished sub-session leaves for its host once
/// [`finish_subsession`] has folded everything else into the world.
pub enum SubsessionHandoff {
    /// Folded into the world; nothing for the host to do.
    Applied,
    /// A Load / Save screen: the pick is committed against the host's save
    /// rack (the card image, the engine-save directory), which the engine
    /// does not own.
    Save(SaveSelectSession),
    /// The Options screen's closing state. Value edits commit inside its own
    /// popup (retail writes the config word at the popup's confirm and never
    /// reverts), so this is the player's; the host lifts and persists it.
    Options(OptionsState),
}

impl SubsessionHandoff {
    /// The host half of a finished sub-session when there is no world to
    /// fold the rest into - a menu opened from the title screen before any
    /// scene exists, whose only reachable screens are Load and Options.
    /// Every other row comes back [`Self::Applied`] with nothing applied.
    pub fn without_world(finished: FieldMenuSubsession) -> Self {
        match finished {
            FieldMenuSubsession::Save(s) => Self::Save(s),
            FieldMenuSubsession::Config(o) => Self::Options(o.state().clone()),
            _ => Self::Applied,
        }
    }
}

/// A finished sub-session after [`finish_subsession`].
pub struct FinishedSubsession {
    pub handoff: SubsessionHandoff,
    /// Window 7: a menu cast that leveled its spell, for
    /// [`crate::menu_runtime::MenuRuntime::arm_spell_level_notice`].
    pub spell_level_notice: Option<SpellLevelNotice>,
    /// Window 8: a Hyper-Art book taught an art, for
    /// [`crate::menu_runtime::MenuRuntime::arm_art_learned_notice`].
    pub art_learned_notice: Option<crate::pause_screens::ArtLearnedNotice>,
}

/// Fold a finished pause-menu sub-session into the world - the one outcome
/// router every host calls when [`FieldMenuSubsession::is_done`] turns true,
/// before it resumes the root list.
///
/// Items run the full pause applier (bag decrements plus the escape / warp
/// hand-off) and surface a taught art's window-8 notice; Equip writes the
/// record back; Spells cast and surface window 7; Arts store the edited chain
/// library; the Status screen's reorder page replays its exchanges onto the
/// record. Save and Options come back as a [`SubsessionHandoff`], because
/// both end in host-owned storage. The native window, the headless
/// `BootSession` and the browser page each carried their own copy of this
/// match, and the headless one had dropped window 8.
pub fn finish_subsession(finished: FieldMenuSubsession, world: &mut World) -> FinishedSubsession {
    let mut out = FinishedSubsession {
        handoff: SubsessionHandoff::Applied,
        spell_level_notice: None,
        art_learned_notice: None,
    };
    match finished {
        FieldMenuSubsession::Items(s) => {
            let _ = apply_pause_items_outcome(&s, world);
            out.art_learned_notice = world.menu.pending_art_notice.take();
        }
        FieldMenuSubsession::Equip {
            session, char_slot, ..
        } => {
            let _ = apply_equip_outcome(&session, char_slot, world);
        }
        FieldMenuSubsession::Spells(s) => {
            out.spell_level_notice = apply_spell_outcome(&s, world);
        }
        FieldMenuSubsession::Arts(editor) => {
            // Lift the live library, apply the edit, store it back
            // (`World::chain_library` <-> `store_chain_library` over the
            // saved chains), so the next battle's Arts rows reflect it.
            let mut library = world.chain_library();
            if apply_arts_outcome(editor, &mut library).is_ok() {
                world.store_chain_library(&library);
            }
        }
        FieldMenuSubsession::ListOrder(s) => {
            // The page permuted its own copy; replay its exchanges onto the
            // live record through the ported swap.
            let _ = apply_list_order_outcome(&s, world);
        }
        FieldMenuSubsession::Status(_) => {}
        storage @ (FieldMenuSubsession::Save(_) | FieldMenuSubsession::Config(_)) => {
            out.handoff = SubsessionHandoff::without_world(storage);
        }
    }
    out
}

/// Apply a finished [`ChainEditor`] outcome to a [`ChainLibrary`].
pub fn apply_arts_outcome(
    editor: ChainEditor,
    library: &mut ChainLibrary,
) -> Result<(), crate::tactical_arts_editor::SaveError> {
    editor.apply_outcome(library)
}

// ---------------------------------------------------------------------------
// Tactical Arts chain editor - engine-extension entry point + view
// ---------------------------------------------------------------------------

/// Pad button that opens the Tactical Arts chain editor from the Status
/// screen. Retail's status panel (`FUN_801D33D8`) reads Left / Right /
/// L1 / R1 / Circle / Start only, so Triangle is unclaimed there and the
/// extension costs no retail input.
pub const ARTS_EDITOR_OPEN_BUTTON: PadButton = PadButton::Triangle;

/// Phase tag of an [`ArtsEditorView`], mirroring
/// [`crate::tactical_arts_editor::EditorPhase`] without the payloads so
/// renderer crates can match on it without depending on `engine-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsEditorPhaseTag {
    Browsing,
    Editing,
    Naming,
}

/// Everything the Tactical Arts editor screen draws, projected out of a
/// live [`ChainEditor`] plus the [`World`] it belongs to.
///
/// Both hosts (the native `play-window` and the browser play page) build
/// their `engine-ui` draw args from this one projection, so the character
/// name lookup, the pretty-printed sequences, the phase mapping and the
/// "+ New" room check cannot drift apart between them.
#[derive(Debug, Clone)]
pub struct ArtsEditorView {
    /// Roster name of the character whose library is being edited.
    pub character_name: String,
    pub phase: ArtsEditorPhaseTag,
    /// One `(name, pretty_sequence)` pair per saved chain, in library order.
    pub saved: Vec<(String, String)>,
    /// Cursor row in the browse list (meaningful in `Browsing`).
    pub browse_cursor: u8,
    /// Pretty-printed working sequence (`Editing` / `Naming`).
    pub editing_pretty: String,
    pub editing_len: usize,
    pub min_len: usize,
    pub max_len: usize,
    /// Name being picked in the `Naming` phase.
    pub naming_name: String,
    /// `true` while the library has room for one more chain - the browse
    /// list only shows its trailing "+ New" row then.
    pub can_add_new: bool,
}

/// Project a live [`ChainEditor`] into the renderer-agnostic
/// [`ArtsEditorView`] both hosts draw from.
///
/// The editor's `library_view` is the authoritative saved-chain list until
/// the engine calls [`apply_arts_outcome`] - it is the snapshot the editor
/// took at construction, so the screen stays consistent with the edits in
/// flight rather than with the world's not-yet-updated records.
pub fn arts_editor_view(editor: &ChainEditor, world: &World) -> ArtsEditorView {
    use crate::tactical_arts_editor::EditorPhase;

    let char_slot = editor.char_slot();
    let character_name = roster_names(world)
        .get(char_slot as usize)
        .cloned()
        .unwrap_or_else(|| format!("Slot {}", char_slot + 1));

    let library = editor.library_view();
    let saved: Vec<(String, String)> = library
        .iter()
        .map(|c| (c.name.clone(), c.pretty_sequence()))
        .collect();

    let pretty_of = |working: &[legaia_art::queue::Command]| -> String {
        working
            .iter()
            .map(|c| match c {
                legaia_art::queue::Command::Left => "L",
                legaia_art::queue::Command::Right => "R",
                legaia_art::queue::Command::Up => "U",
                legaia_art::queue::Command::Down => "D",
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let (phase, browse_cursor, editing_pretty, editing_len, naming_name) = match editor.phase() {
        EditorPhase::Browsing { cursor } => (
            ArtsEditorPhaseTag::Browsing,
            *cursor,
            String::new(),
            0usize,
            String::new(),
        ),
        EditorPhase::Editing { working } => (
            ArtsEditorPhaseTag::Editing,
            0,
            pretty_of(working),
            working.len(),
            String::new(),
        ),
        EditorPhase::Naming { working, name } => (
            ArtsEditorPhaseTag::Naming,
            0,
            pretty_of(working),
            working.len(),
            name.clone(),
        ),
        // Terminal state: the host is about to drain the outcome, so draw
        // the browse frame rather than a half-torn editing one.
        EditorPhase::Done(_) => (
            ArtsEditorPhaseTag::Browsing,
            0,
            String::new(),
            0usize,
            String::new(),
        ),
    };

    ArtsEditorView {
        character_name,
        phase,
        can_add_new: saved.len() < ChainLibrary::MAX_SLOTS,
        saved,
        browse_cursor,
        editing_pretty,
        editing_len,
        min_len: ChainLibrary::MIN_LEN,
        max_len: ChainLibrary::MAX_LEN,
        naming_name,
    }
}

/// Engine-extension entry point for the Tactical Arts chain editor.
///
/// Retail's pause menu is the seven rows in [`FieldMenuRow::ALL`] and has
/// no Arts row, so the editor needs an entry that does not invent an
/// eighth: pressing [`ARTS_EDITOR_OPEN_BUTTON`] while the **Status**
/// screen is up swaps that sub-session for a [`ChainEditor`] on the
/// character the panel is currently showing. Status is the retail surface
/// that lists a character's arts, which is why
/// [`FieldMenuSubsession::row`] already parks the Arts resume cursor
/// there.
///
/// Hosts call this once per frame *before* [`FieldMenuSubsession::tick_pad_edge`]
/// and skip that tick when it returns `true`, so the same edge does not
/// also drive the screen it just replaced. Returns `false` (and leaves
/// `sub` alone) for every other sub-session and every other button.
pub fn try_open_arts_editor(sub: &mut FieldMenuSubsession, pressed: u16, world: &World) -> bool {
    if pressed & ARTS_EDITOR_OPEN_BUTTON.mask() == 0 {
        return false;
    }
    let FieldMenuSubsession::Status(status) = sub else {
        return false;
    };
    // The status panel filters out unclaimed roster slots, so its cursor
    // indexes the *shown* list - map back through the snapshot's own slot
    // so the editor edits the right character's library.
    let char_slot = status
        .current()
        .map(|snap| snap.slot)
        .unwrap_or_else(|| status.cursor());
    let library = world.chain_library();
    *sub = FieldMenuSubsession::Arts(ChainEditor::new(char_slot, &library));
    true
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// Resolve the slot of the active leader. Falls back to slot 0 when no
/// leader is set or when the roster is empty.
pub fn active_leader_slot(world: &World) -> u8 {
    world.party.party_leader_slot.unwrap_or_default()
}

/// Build a [`StatusSnapshot`] for every member of the **present party**
/// (retail's `DAT_80084594` count over the `0x80084598` member list,
/// [`World::present_party_list`]), in that list's order, skipping a record
/// with a zero max-HP. The retail status list is the present party, not the
/// roster: the New Game template seeds all four records, so a roster walk
/// lists Noa, Gala and Terra beside a Vahn who is still travelling alone.
pub fn status_snapshots(world: &World) -> Vec<StatusSnapshot> {
    let names = roster_names(world);
    let mut out = Vec::new();
    for slot in world.present_party_list() {
        let i = usize::from(slot);
        let Some(member) = world.party.roster.members.get(i) else {
            continue;
        };
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
        // The grid's seven rows read the record through retail's own offset
        // tables (FUN_801D33D8 `0x801D3B4C..`): row 0 the per-character
        // weapon byte (`DAT_8007B42C`), rows 1..6 `DAT_801E43E8[row]` -
        // head, body, footwear, Goods x3. The record's `+0x196` order is
        // `[body, head, weapon, weapon, footwear, goods x3]`, so reading it
        // in place put body armour on the weapon row.
        let engine_slots =
            crate::equip_session::engine_equip_from_record(member.equipment().slots, i as u8);
        let equip_views: Vec<EquipSlotView> = crate::equip_session::BROWSE_SLOT_ORDER
            .iter()
            .map(|&s| EquipSlotView {
                label: equip_slot_label(s),
                // Empty slots stay blank (retail draws only the slot
                // pictogram); an occupied slot prints the item's name out of
                // the SCUS item table (`PTR_DAT_8007436C`), the raw id only
                // when no table was staged.
                item_name: match engine_slots[usize::from(s)] {
                    0 => String::new(),
                    id => world
                        .menu
                        .text
                        .as_ref()
                        .and_then(|t| t.item_name(id))
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("#{id:02X}")),
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
            // The status page's AP gauge reads the persistent char-record
            // AP at `+0x10E` (`hp_mp_sp().sp_cur` under the (max, cur) pair
            // order - 0 on a fresh party; the new-game seed zeroes it), NOT
            // the battle ApGauge's base AP.
            // REF: FUN_801D33D8 (docs/subsystems/field-menu.md).
            ap: hms.sp_cur.min(u8::MAX as u16) as u8,
            ap_max: 100,
            attack: world.battle.attack.get(i).copied().unwrap_or(0),
            defense: world.battle.defense.get(i).copied().unwrap_or(0),
            stats: stat_pairs,
            stat_labels: crate::status_screen::RETAIL_STAT_LABELS,
            equip: equip_views,
            elements: default_element_views(),
            // Record `+0x12E` - the packed ailment word. Retail keeps it as
            // a per-frame mirror of the battle actor's `+0x16E`
            // (`FUN_80047430`), so out of battle the record still carries
            // whatever condition the party walked away with; the engine's
            // equivalent latch is the never-cleared `World::battle.status_effects`
            // tracker, whose `display_flags` packs the same bit word. Party
            // seats and roster indices coincide for `i < party_count`
            // (`World::enter_battle` seats the roster in order).
            status_flags: world.battle.status_effects.display_flags(i as u8),
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

/// The display name of each roster record, in roster order.
///
/// Retail draws a character's name from the record itself (`+0x2A7`, the
/// name the player typed at the naming screen), never from a fixed table -
/// so a save whose Vahn was renamed shows the new name on every menu and
/// battle panel. The port's copy of that field is
/// [`crate::world::PartyState::party_names`], which a card load fills from
/// the records and a New Game seeds with the template names; a slot it has
/// no name for falls back to the canonical one.
pub fn roster_names(world: &World) -> Vec<String> {
    let canonical = ["Vahn", "Noa", "Gala"];
    world
        .party
        .roster
        .members
        .iter()
        .enumerate()
        .map(|(i, _)| {
            world
                .party
                .party_names
                .get(i)
                .filter(|n| !n.is_empty())
                .cloned()
                .or_else(|| canonical.get(i).map(|s| (*s).to_string()))
                .unwrap_or_else(|| format!("Slot {i}"))
        })
        .collect()
}

fn build_spell_session(world: &World, catalog: &SpellCatalog) -> SpellMenuSession {
    let names = roster_names(world);
    let party: Vec<SpellCasterSlot> = world
        .party
        .roster
        .members
        .iter()
        .enumerate()
        .map(|(i, member)| {
            let hms = member.hp_mp_sp();
            let list = member.spell_list();
            let n = list.count as usize;
            SpellCasterSlot {
                slot: i as u8,
                name: names.get(i).cloned().unwrap_or_default(),
                hp: hms.hp_cur,
                mp: hms.mp_cur,
                hp_max: hms.hp_max,
                mp_max: hms.mp_max,
                level: member.magic_rank(),
                spells: list.ids[..n].to_vec(),
                spell_levels: list.levels[..n].to_vec(),
                // Per-caster MP-cost ability bits: retail's kernel reads the
                // caster's own record `+0xF4` word (`0x800353B4`), so this is
                // keyed by the roster record, not by battle ordinal (the
                // `character_ability_bits` mirror is ordinal-indexed and
                // names a different member once `active_party` reorders).
                ability_bits: crate::spells::record_ability_word(member),
                // Retail resolves the Ra-Seru slot through the
                // per-character offset table at 0x8007B424; the engine's
                // roster always carries the Ra-Seru equipped, so the
                // gate stays open here.
                ra_seru_missing: false,
            }
        })
        .collect();
    let targets: Vec<SpellTargetRow> = world
        .party
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
    // Retail's list build asks the spell-record broadcast `FUN_8003053C`
    // whether each spell would affect anybody (`0x80031210`), and both cast
    // flows ask again before they commit (`0x801D954C` / `0x801D98B4`).
    let mut unaffected: Vec<u8> = Vec::new();
    for c in &party {
        for &id in &c.spells {
            if crate::menu_validator::spell_affects_anyone(world, id) == Some(false)
                && !unaffected.contains(&id)
            {
                unaffected.push(id);
            }
        }
    }
    SpellMenuSession::new(party, targets, catalog.clone()).with_unaffected_spells(unaffected)
}

fn build_inventory_session(world: &World) -> InventoryUseSession {
    let names = roster_names(world);
    // Retail's Use-list row order when the on-disc effect table is installed
    // (`World::bag_use_rows`, the SCUS content-id-3 builder over the bag's
    // active window): slot walk, three-buffer grouping, field context. The
    // id-sorted fallback is what a disc-free host gets - with no descriptors
    // there is nothing to group by. The paired PauseItemRow list is built in
    // the same order; keep these in lockstep.
    let (items, bag_slots): (Vec<u8>, Vec<u8>) = match world.bag_use_rows(false) {
        Some(rows) => (
            rows.iter().map(|r| r.id).collect(),
            rows.iter().map(|r| r.slot).collect(),
        ),
        None => {
            let mut v: Vec<u8> = world
                .party
                .inventory
                .iter()
                .filter_map(|(id, qty)| if *qty > 0 { Some(*id) } else { None })
                .collect();
            v.sort_unstable();
            (v, Vec::new())
        }
    };
    let targets: Vec<InvTargetRow> = world
        .party
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
                        .battle
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
        world.tables.item_catalog.clone(),
        items,
        targets,
        InventoryContext::Field,
    )
    // The row payloads: retail's list entries carry a bag slot, and Use /
    // Throw Out remove from the slot the row named.
    .with_bag_slots(bag_slots)
}

/// One item id's display text, as every screen that shows an item needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDisplayText {
    /// Display name. Falls back to the curated catalog, then to a raw id,
    /// so a screen never shows nothing.
    pub name: String,
    /// Info-panel description. Empty when the disc text is unavailable.
    pub desc: String,
    /// An accessory's two passive lines (`(name, description)`).
    pub passive: Option<(String, String)>,
}

/// Resolve one item id's name / description / passive lines through the
/// world's disc text tables, with the curated catalog and a raw-id spelling
/// as fallbacks.
///
/// Lifted out of the Items-screen session builder because it is not the
/// Items screen's: retail's item-info panel (`FUN_801D0F1C`) is opened by
/// window 17 on the Items screen **and** by window 24 on the Equip screen's
/// candidate step, off the same table. Keeping the resolution inside one
/// screen's builder is why the Equip screen showed raw ids where retail
/// shows names.
pub fn item_display_text(world: &World, id: u8) -> ItemDisplayText {
    let text = world.menu.text.as_ref();
    ItemDisplayText {
        name: text
            .and_then(|t| t.item_name(id))
            .map(str::to_string)
            .or_else(|| {
                world
                    .tables
                    .item_catalog
                    .get(id)
                    .map(|e| e.name.to_string())
            })
            .unwrap_or_else(|| format!("Item {id:02X}")),
        desc: text
            .and_then(|t| t.item_desc(id))
            .unwrap_or_default()
            .to_string(),
        passive: text.and_then(|t| t.item_passive_lines(id)),
    }
}

/// Build the retail Items screen session: the item-use flow plus the
/// per-row display data (real bag counts; names / descriptions /
/// accessory passive lines resolved through the world's disc text tables
/// with catalog + raw-id fallbacks).
pub fn build_pause_items_session(world: &World) -> PauseItemsSession {
    let inner = build_inventory_session(world);
    let slots = inner.bag_slots.clone();
    let rows: Vec<PauseItemRow> = inner
        .items
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let t = item_display_text(world, id);
            PauseItemRow {
                id,
                slot: slots.get(i).copied().unwrap_or(0),
                name: t.name,
                count: world.party.inventory.get(&id).copied().unwrap_or(0),
                desc: t.desc,
                passive: t.passive,
            }
        })
        .collect();
    // The Throw Out command's own row order (content id 0x22). The screen is
    // built in the Use order (content id 3), and this is a bag-slot sequence,
    // so the command window can permute the rows it already holds.
    let throw_out_slots: Vec<u8> = world
        .bag_throw_out_rows()
        .map(|rows| rows.iter().map(|r| r.slot).collect())
        .unwrap_or_default();
    PauseItemsSession::new(inner, rows)
        .with_arrange_rank(world.menu.arrange_rank.clone())
        .with_warp_destinations(warp_destinations(world))
        .with_throw_out_row_order(throw_out_slots)
        .with_incense_window(world.locomotion.walk_regen_window)
}

/// The visible rows of the quick-travel landmark list - the Door of Wind
/// destination list, and the same set the world-map landmark menu shows.
///
/// A faithful walk of `FUN_80030628` case `0x19`
/// (`0x80031870..0x800318dc`) over the disc placement table
/// ([`legaia_asset::worldmap_menu`], installed by
/// [`World::install_menu_text`]):
///
/// - records run to the `name_idx == 0xFF` terminator (the parser already
///   stops there);
/// - a record whose `name_idx` repeats the **last accepted** row's is
///   skipped (`0x800318a8`: the compare is against `s0`, which only
///   advances on an accept, so a locked landmark does not shadow the next
///   record that shares its name);
/// - the survivor is gated on system flag `discovery_flag + 0x20`
///   (`0x800318b4`, `FUN_8003CE64` = [`World::system_flag_test`]);
/// - an accepted row is pushed as the string id `0x8000 | record_index`
///   (`0x800318c0`), which is why [`WarpDestination::record_index`] is the
///   record ordinal and not the visible one.
///
/// Empty when the executable was not reachable at boot - the route then
/// opens an empty list rather than an invented one.
///
/// PORT: FUN_80030628 (case 0x19: the landmark-list build)
pub fn warp_destinations(world: &World) -> Vec<crate::pause_screens::WarpDestination> {
    let Some(menu) = world.menu.worldmap_menu.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut last_accepted: Option<u8> = None;
    for p in &menu.placements {
        if last_accepted == Some(p.name_idx) {
            continue;
        }
        if !world.system_flag_test(u16::from(p.discovery_flag) + 0x20) {
            continue;
        }
        last_accepted = Some(p.name_idx);
        out.push(crate::pause_screens::WarpDestination {
            record_index: p.index,
            name: menu
                .names
                .get(p.name_idx as usize)
                .cloned()
                .unwrap_or_default(),
            scene_id: p.scene_id,
            menu_x: p.menu_x,
            menu_y: p.menu_y,
        });
    }
    out
}

fn build_equip_session(world: &World, char_slot: u8, equipment: &EquipmentTable) -> EquipSession {
    // The session works in the engine's slot order; the record's `+0x196`
    // bytes are retail's, reordered per character on the way in.
    let record_for = |slot: u8| {
        world
            .party
            .roster
            .members
            .get(slot as usize)
            .map(|c| {
                let mut r = stat_record_from_character(c);
                r.equip = crate::equip_session::engine_equip_from_record(r.equip, slot);
                r
            })
            .unwrap_or_default()
    };
    let session = EquipSession::new(
        record_for(char_slot),
        world.party.inventory.clone(),
        equipment.clone(),
        StatusModifiers::default(),
        Vec::new(),
    );
    // The Best-Equipment chooser's weapon arm scores each candidate through
    // `FUN_801DD0C0` against the menu overlay's category table. Without the
    // table installed the check returns 0 for every weapon and the pick
    // collapses onto raw ATK - which is the routine's own empty-table arm,
    // but not what a retail disc produces. `World::install_menu_overlay_tables`
    // fills it, so both hosts get it from the boot call they already make.
    let session = session
        .with_active_party_slot(char_slot)
        .with_weapon_category(world.menu.item_category.clone());
    // Disc restrictions when the boot parsed them: the candidate list is
    // then the retail one - the `+6` character mask and `+7` category for
    // the four armament rows, and the class-2 Goods index for the three
    // Goods rows - instead of the `id >> 5` placeholder rule, which offers
    // a Goods row the wrong id band entirely. Both hosts reach this one
    // builder, so the screens cannot disagree.
    match world.tables.equip_stats.as_ref() {
        Some(stats) => {
            let mut info = crate::equipment::DiscEquipInfo::from_disc(stats);
            if let Some(effects) = world.tables.item_effects.as_ref() {
                info.install_goods(effects);
            }
            session.with_restrictions(info, char_slot)
        }
        None => session,
    }
}

pub(crate) fn stat_record_from_character(c: &legaia_save::CharacterRecord) -> StatRecord {
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

/// Per-roster-member reorder rows for the Status screen's confirm, in the
/// order [`status_snapshots`] publishes them.
///
/// The rows are the character's own spell list in **record order** - the
/// same `+0x13D` / `+0x161` pair the Magic screen lists and
/// [`crate::save_subscreen::sub15_swap_rows`] exchanges - so a swap made on
/// the page is a swap on the Magic screen.
fn status_spell_rows(world: &World, catalog: &SpellCatalog) -> Vec<Vec<ListOrderRow>> {
    world
        .party
        .roster
        .members
        .iter()
        .filter(|m| m.hp_mp_sp().hp_max != 0)
        .map(|member| {
            let list = member.spell_list();
            list.ids[..(list.count as usize).min(list.ids.len())]
                .iter()
                .map(|&id| ListOrderRow {
                    id,
                    label: catalog
                        .get(id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| format!("Spell {id}")),
                })
                .collect()
        })
        .collect()
}

/// Open the list-reorder page over the Status screen's shown character.
///
/// The step is the spell list's running twin, which is the one step of the
/// three that carries the swap arm
/// ([`crate::save_subscreen::sub15_list_source`]).
fn open_status_list_order(session: &StatusScreenSession) -> Option<ListOrderSession> {
    let slot = session.current().map(|s| s.slot)?;
    let rows = session.spell_rows_for_cursor().to_vec();
    ListOrderSession::open(LIST_ORDER_STEP_MAGIC, slot, rows)
}

/// Apply a finished reorder page to the live record.
///
/// The page permutes a copy; this replays its exchanges against the
/// character's own `0x414` bytes through the ported swap
/// ([`crate::save_subscreen::sub15_swap_rows`]), which is what keeps the
/// three parallel arrays - the id list, its companion byte list and the row
/// words - in step. Returns how many exchanges landed.
///
/// The live row count is re-derived first
/// ([`crate::save_subscreen::sub15_list_len`], with the character's Ra-Seru
/// slot): the record is the authority on how long the list is, and an
/// exchange that names a row past its end is dropped rather than clamped -
/// retail's own reject arm fires on the same count.
pub fn apply_list_order_outcome(session: &ListOrderSession, world: &mut World) -> usize {
    let slot = session.char_slot() as usize;
    let raseru = crate::equip_session::RETAIL_RASERU_EQUIP_BYTE
        .get(slot)
        .copied()
        .unwrap_or(3)
        .max(0) as usize;
    let Some(member) = world.party.roster.members.get_mut(slot) else {
        return 0;
    };
    let len = usize::from(crate::save_subscreen::sub15_list_len(
        crate::list_order::LIST_ORDER_STEP_MAGIC,
        &member.raw,
        raseru,
    ));
    let mut applied = 0;
    for &(a, b) in session.swaps() {
        if a >= len || b >= len {
            continue;
        }
        crate::save_subscreen::sub15_swap_rows(&mut member.raw, a, b);
        applied += 1;
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_menu::FieldMenuRow;

    fn fresh_world() -> World {
        let mut world = World::new();
        // Three placeholder records with non-zero max HP/MP so the
        // status / spell builders include them.
        world.party.roster = legaia_save::Party::zeroed(3);
        for member in &mut world.party.roster.members {
            let mut hms = member.hp_mp_sp();
            hms.hp_cur = 50;
            hms.hp_max = 100;
            hms.mp_cur = 10;
            hms.mp_max = 30;
            member.set_hp_mp_sp(hms);
        }
        world.party.inventory.insert(0x77, 3); // Healing Leaf (real item id)
        world.party.party_leader_slot = Some(0);
        world.set_item_catalog(crate::items::ItemCatalog::vanilla());
        world
    }

    fn fresh_save_slots() -> SaveRack {
        SaveRack::Blocks(
            (0..3)
                .map(crate::save_select::SlotSnapshot::empty)
                .collect(),
        )
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

    /// Every pause-menu level readout reads the record's `+0x130` byte, so a
    /// level cheat shows on Status and on the Magic caster window alike, and
    /// survives the ability-bitfield rebuild (`+0xF4..+0x103`) the battle-stat
    /// refresh runs.
    #[test]
    fn status_and_magic_read_the_live_level() {
        let mut w = fresh_world();
        w.party.roster.members[0].set_level(30);
        w.party.roster.members[0].set_ability_bits([0; legaia_save::ABILITY_BITS_LEN]);
        assert_eq!(status_snapshots(&w)[0].level, 30);
        match build(FieldMenuRow::Magic, &w) {
            FieldMenuSubsession::Spells(s) => {
                let m = crate::pause_screens::magic_screen_model(&s, None);
                assert_eq!(m.casters[0].1, 30);
            }
            _ => panic!("expected Spells"),
        }
    }

    #[test]
    fn build_items_returns_inventory_session() {
        let w = fresh_world();
        let s = build(FieldMenuRow::Items, &w);
        assert_eq!(s.row(), FieldMenuRow::Items);
        assert!(matches!(s, FieldMenuSubsession::Items(_)));
    }

    /// A Throw Out discard reaches the world bag through
    /// `apply_inventory_outcome` even when the session ends without a
    /// completed use (the retail delete zeroes the whole bag-slot pair).
    #[test]
    fn throw_out_discard_applies_to_world_inventory() {
        use crate::input::PadButton;
        let mut w = fresh_world();
        let FieldMenuSubsession::Items(mut s) = build(FieldMenuRow::Items, &w) else {
            panic!("items session");
        };
        let press = |s: &mut crate::pause_screens::PauseItemsSession, b: PadButton| {
            s.input_pad_edge(b.mask());
        };
        press(&mut s, PadButton::Down); // -> Throw Out
        press(&mut s, PadButton::Cross); // into the discard list
        press(&mut s, PadButton::Cross); // open the confirm (No)
        press(&mut s, PadButton::Up); // -> Yes
        press(&mut s, PadButton::Cross); // discard the 0x77 stack
        press(&mut s, PadButton::Circle); // back off the command window
        assert!(s.is_done());
        apply_inventory_outcome(&s.inner, &mut w);
        assert!(
            !w.party.inventory.contains_key(&0x77),
            "the whole stack is discarded"
        );
    }

    /// An equipped slot prints the item's name from the staged item table,
    /// not its raw id; an empty slot stays blank.
    #[test]
    fn status_equipment_rows_print_item_names() {
        let mut w = fresh_world();
        let mut names = vec![None; 256];
        names[0x43] = Some("Survival Knife".to_string());
        w.menu.text = Some(crate::pause_screens::MenuTextTables {
            item_names: Some(legaia_asset::item_names::ItemNameTable::from_names(names)),
            ..Default::default()
        });
        let mut eq = w.party.roster.members[0].equipment();
        eq.slots.fill(0);
        // Vahn's weapon byte is `2`; byte `1` is the head row.
        eq.slots[2] = 0x43;
        eq.slots[1] = 0x44;
        w.party.roster.members[0].set_equipment(eq);
        let snap = &status_snapshots(&w)[0];
        assert_eq!(snap.equip[0].item_name, "Survival Knife");
        assert_eq!(snap.equip[1].item_name, "#44", "no name staged");
        assert_eq!(snap.equip[2].item_name, "");
    }

    #[test]
    fn build_status_snapshots_skip_empty_roster_slots() {
        let mut w = fresh_world();
        // Zero one member's max HP - they should drop out of the snapshot.
        let mut hms = w.party.roster.members[2].hp_mp_sp();
        hms.hp_max = 0;
        w.party.roster.members[2].set_hp_mp_sp(hms);
        let snaps = status_snapshots(&w);
        assert_eq!(snaps.len(), 2);
    }

    /// A lone traveller's status list is the lone traveller, even though
    /// every roster record is populated (the New Game template seeds all of
    /// them), and a non-identity party lists its members in party order.
    #[test]
    fn status_snapshots_list_the_present_party_only() {
        let mut w = fresh_world();
        w.install_present_party_list(vec![0]);
        let slots: Vec<u8> = status_snapshots(&w).iter().map(|s| s.slot).collect();
        assert_eq!(slots, vec![0]);
        w.install_present_party_list(vec![2, 0]);
        let slots: Vec<u8> = status_snapshots(&w).iter().map(|s| s.slot).collect();
        assert_eq!(slots, vec![2, 0]);
    }

    #[test]
    fn build_spells_session_population() {
        let w = fresh_world();
        let s = build(FieldMenuRow::Magic, &w);
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

    /// The Equip row opens on retail's character picker (`0x12`): Down walks
    /// the present party and re-points the screen at the hovered member,
    /// Cross hands the pad to the slot browse, and the browse's cancel comes
    /// back to the picker rather than leaving the screen.
    #[test]
    fn equip_opens_on_the_character_picker() {
        let mut w = fresh_world();
        w.install_present_party_list(vec![0, 1]);
        let mut s = build(FieldMenuRow::Equip, &w);
        let state = |s: &FieldMenuSubsession| match s {
            FieldMenuSubsession::Equip {
                char_slot, picking, ..
            } => (*char_slot, *picking),
            _ => panic!("expected Equip"),
        };
        assert_eq!(state(&s), (0, true));
        tick_open_subsession(&mut s, PadButton::Down.mask(), None, &w);
        assert_eq!(state(&s), (1, true));
        tick_open_subsession(&mut s, PadButton::Down.mask(), None, &w);
        assert_eq!(state(&s), (1, true), "the walk stops at the last member");
        tick_open_subsession(&mut s, PadButton::Cross.mask(), None, &w);
        assert_eq!(state(&s), (1, false));
        tick_open_subsession(&mut s, PadButton::Circle.mask(), None, &w);
        assert_eq!(state(&s), (1, true));
        assert!(!s.is_done());
        tick_open_subsession(&mut s, PadButton::Circle.mask(), None, &w);
        assert!(s.is_done());
    }

    #[test]
    fn build_equip_uses_active_leader() {
        let mut w = fresh_world();
        w.party.party_leader_slot = Some(2);
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
            FieldMenuRow::Options,
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
    fn triangle_on_status_opens_the_arts_editor_for_the_shown_character() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Status, &w);
        assert!(try_open_arts_editor(&mut s, PadButton::Triangle.mask(), &w));
        match &s {
            FieldMenuSubsession::Arts(editor) => {
                // The status panel opens on its first shown snapshot, so the
                // editor must target that member's slot - not a fixed 0.
                let expected = status_snapshots(&w).first().map(|s| s.slot).unwrap_or(0);
                assert_eq!(editor.char_slot(), expected);
            }
            _ => panic!("expected the Status session to be swapped for Arts"),
        }
        // The resume cursor parks back on Status, so closing the editor
        // returns the player where the extension was entered.
        assert_eq!(s.row(), FieldMenuRow::Status);
    }

    #[test]
    fn arts_editor_open_button_is_inert_outside_the_status_screen() {
        let w = fresh_world();
        for row in [FieldMenuRow::Items, FieldMenuRow::Equip, FieldMenuRow::Save] {
            let mut s = build(row, &w);
            assert!(
                !try_open_arts_editor(&mut s, PadButton::Triangle.mask(), &w),
                "{row:?} must not open the arts editor"
            );
            assert_eq!(s.row(), row);
        }
        // ...and Status itself only reacts to the documented button.
        let mut s = build(FieldMenuRow::Status, &w);
        assert!(!try_open_arts_editor(&mut s, PadButton::Cross.mask(), &w));
        assert!(matches!(s, FieldMenuSubsession::Status(_)));
    }

    #[test]
    fn arts_editor_view_projects_live_library_and_phase() {
        use crate::tactical_arts_editor::SavedChain;
        use legaia_art::queue::Command;

        let w = fresh_world();
        let mut lib = ChainLibrary::new();
        lib.save(
            0,
            SavedChain::new(
                "Combo A",
                vec![Command::Left, Command::Right, Command::Down],
            ),
        )
        .expect("in-range chain saves");
        let editor = ChainEditor::new(0, &lib);

        let view = arts_editor_view(&editor, &w);
        assert_eq!(view.phase, ArtsEditorPhaseTag::Browsing);
        assert_eq!(view.saved.len(), 1);
        assert_eq!(view.saved[0].0, "Combo A");
        // The pretty sequence is the shared one-line stringification both
        // hosts print, so it is asserted rather than re-derived per host.
        assert_eq!(view.saved[0].1, "L R D");
        assert_eq!(view.character_name, "Vahn");
        assert_eq!(view.min_len, ChainLibrary::MIN_LEN);
        assert_eq!(view.max_len, ChainLibrary::MAX_LEN);
        assert!(view.can_add_new, "1 of 8 slots used leaves room for + New");
    }

    #[test]
    fn arts_editor_view_hides_new_row_once_the_library_is_full() {
        use crate::tactical_arts_editor::SavedChain;
        use legaia_art::queue::Command;

        let w = fresh_world();
        let mut lib = ChainLibrary::new();
        for i in 0..ChainLibrary::MAX_SLOTS {
            lib.save(
                0,
                SavedChain::new(
                    format!("C{i}"),
                    vec![Command::Up, Command::Up, Command::Down],
                ),
            )
            .expect("library has room until MAX_SLOTS");
        }
        let view = arts_editor_view(&ChainEditor::new(0, &lib), &w);
        assert_eq!(view.saved.len(), ChainLibrary::MAX_SLOTS);
        assert!(!view.can_add_new);
    }

    #[test]
    fn tick_pad_edge_options_circle_cancels() {
        let w = fresh_world();
        let mut s = build(FieldMenuRow::Options, &w);
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
        let mut s = build(FieldMenuRow::Magic, &w);
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
        w.party.inventory.clear();
        w.party.inventory.insert(0x25, 1);
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
        // Slot-browse row 0 is Best Equipment, so slot 1 (where item 0x25
        // lives) is row 2: two steps down, confirm into the item picker,
        // confirm the single item, confirm Yes.
        // The Equip row opens on its character picker; confirm hands the pad
        // to the slot browse.
        s.tick_pad_edge(PadButton::Cross.mask());
        for _ in 0..2 {
            s.tick_pad_edge(PadButton::Down.mask());
        }
        for _ in 0..3 {
            s.tick_pad_edge(PadButton::Cross.mask());
        }
        assert!(s.is_done());
        if let FieldMenuSubsession::Equip {
            session, char_slot, ..
        } = &s
        {
            let outcome = apply_equip_outcome(session, *char_slot, &mut w);
            assert!(matches!(outcome, Some(EquipOutcome::Committed { .. })));
            // Roster member 0's slot 1 byte now matches the equipped id.
            assert_eq!(w.party.roster.members[0].equipment().slots[1], 0x25);
            // ...and the equipped item LEFT the bag (no duplication). Slot 1 was
            // empty, so nothing is returned.
            assert_eq!(
                w.party.inventory.get(&0x25),
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
        w.party.inventory.clear();
        w.party.inventory.insert(0x25, 1);
        // Pre-equip a different slot-1 item (0x26 >> 5 == 1) on member 0.
        let mut eq = w.party.roster.members[0].equipment();
        eq.slots[1] = 0x26;
        w.party.roster.members[0].set_equipment(eq);

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
        // Row 2 is slot 1. The slot is occupied, so its candidate list
        // leads with the Remove row - one more step down lands on 0x25.
        // The Equip row opens on its character picker; confirm hands the pad
        // to the slot browse.
        s.tick_pad_edge(PadButton::Cross.mask());
        for _ in 0..2 {
            s.tick_pad_edge(PadButton::Down.mask());
        }
        s.tick_pad_edge(PadButton::Cross.mask());
        s.tick_pad_edge(PadButton::Down.mask());
        for _ in 0..2 {
            s.tick_pad_edge(PadButton::Cross.mask());
        }
        assert!(s.is_done());
        let FieldMenuSubsession::Equip {
            session, char_slot, ..
        } = &s
        else {
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
        assert_eq!(w.party.roster.members[0].equipment().slots[1], 0x25);
        // 0x25 left the bag, 0x26 came back into it.
        assert_eq!(
            w.party.inventory.get(&0x25),
            None,
            "equipped item left the bag"
        );
        assert_eq!(
            w.party.inventory.get(&0x26),
            Some(&1),
            "swapped-out item returned to the bag"
        );
    }
}
