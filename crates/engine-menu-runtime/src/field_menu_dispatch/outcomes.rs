//! Applying a finished sub-session's outcome to the `World`: equip, inventory use, pause
//! items, spells, arts and list reorders.

use super::*;

/// Write one equipment swap the menu committed onto the live record and
/// bag: `added` leaves the bag for engine slot `slot` of `char_slot`, and
/// the item it replaces (`removed`, `0` for none) goes back.
fn write_equip_swap(world: &mut World, char_slot: u8, slot: u8, added: u8, removed: u8) {
    let Some(member) = world.party.roster.members.get_mut(char_slot as usize) else {
        return;
    };
    let mut eq = member.equipment();
    // The swap names an engine slot; the record byte is retail's.
    let byte = legaia_engine_menus::equip_session::record_byte_for_engine_slot(slot, char_slot);
    if byte >= eq.slots.len() {
        return;
    }
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
        let _ = world.party.inventory.add(removed, 1);
    }
}

/// Apply a finished [`EquipSession`] to a `world.party.roster` member. Returns
/// `Some(EquipOutcome)` when the session ended on an outcome; `None` for a
/// session still open.
///
/// Two kinds of commit reach the record. The Best Equipment row's swaps
/// ([`EquipSession::best_equipment_swaps`]) do not end the session - retail's
/// applier `FUN_801CF760` writes the record and the bag where it stands and
/// the slot browser stays up - so they are replayed first, whatever the
/// session's closing outcome. A single-slot confirm is the outcome itself.
// REF: FUN_801CF760
pub fn apply_equip_outcome(
    session: &EquipSession,
    char_slot: u8,
    world: &mut World,
) -> Option<EquipOutcome> {
    let best = session.best_equipment_swaps();
    for &(slot, removed, added) in best {
        write_equip_swap(world, char_slot, slot, added, removed);
    }
    let outcome = session.outcome();
    let committed = matches!(outcome, Some(EquipOutcome::Committed { .. }));
    if let Some(EquipOutcome::Committed {
        slot,
        added,
        removed,
    }) = outcome
    {
        write_equip_swap(world, char_slot, slot, added, removed);
    }
    if committed || !best.is_empty() {
        // An equipment change can add / remove an accessory passive; rebuild
        // the ability bitfields immediately (retail re-derives them every
        // aggregator pass) so menu + battle consumers see the new bits
        // without waiting for the next battle entry.
        world.refresh_party_ability_bits();
    }
    outcome
}

/// Apply a finished [`InventoryUseSession`] to the world. Folds the
/// stored item outcome through the same path
/// [`legaia_engine_core::world::World::use_item`] uses: HP / MP / status / SP gain, and
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
/// `Some` alongside [`legaia_engine_menus::inventory_use::InventoryUseState::Done`],
/// which is why the state gate sits on the effect loop rather than on the
/// whole body - a Door of Light closes the screen with the inner flow still
/// browsing, and its consumption must survive that.
// REF: FUN_801D8734 (throw-out delete)
// REF: FUN_80042310 (the one-copy bag decrement all three routes call)
pub fn apply_inventory_outcome(session: &InventoryUseSession, world: &mut World) {
    use legaia_engine_menus::inventory_use::InventoryUseState;
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
                if let legaia_engine_menus::items::ItemOutcome::ArtLearned {
                    character,
                    art_id,
                    ..
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
/// ([`legaia_engine_menus::pause_screens::patch_notify_template`]) and expanded against the
/// party names and the arts-name table.
///
/// `None` when the menu overlay's template is not installed - the notice is
/// disc text and the engine does not invent it.
pub fn art_learned_notice(
    world: &World,
    character: u8,
    art_id: u8,
) -> Option<legaia_engine_menus::pause_screens::ArtLearnedNotice> {
    let mut template = world.menu.notify_template.clone()?;
    legaia_engine_menus::pause_screens::patch_notify_template(
        &mut template,
        i16::from(character),
        art_id,
    );
    let names = &world.party.party_names;
    let lines = legaia_engine_menus::pause_screens::expand_notify_lines(
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
    Some(legaia_engine_menus::pause_screens::ArtLearnedNotice {
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
    session: &legaia_engine_menus::pause_screens::PauseItemsSession,
    world: &mut World,
) -> Option<u32> {
    apply_inventory_outcome(&session.inner, world);
    // Incense (item `0x8A`, effect class `0x82`): the confirm runs the SCUS
    // item applier, whose class-`0x82` arm (`0x800421A0`) is one
    // `jal 0x80046870` - `+0x40` walk ticks on `_DAT_8007B600`, capped at
    // `0x100`. The field walk tick drains it and the region encounter roll
    // skips while it is non-zero (`World::on_field_step`).
    for _ in 0..session.incense_uses() {
        world.locomotion.walk_regen_window = legaia_engine_system::incense_window::top_up_cooldown(
            world.locomotion.walk_regen_window,
        );
    }
    if let Some(warp) = session.staged_warp() {
        world.menu.pending_warp = Some(warp);
    }
    if session.exit_code() == Some(legaia_engine_menus::pause_screens::MENU_EXIT_CODE_FIELD_ESCAPE)
    {
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
/// [`legaia_engine_battle::magic_xp::SPELL_XP_OFFSET`]): `+12` when the target's deficit
/// covered the full heal, `+4` when the heal was clipped (`+3` / `+1` per
/// member on the multi-target arm). Outside battle the same handler then
/// tests the `0x8007656C` threshold table against the accumulator, bumps
/// the `+0x161 + slot` level byte (cap 9) and calls the notification
/// setter `FUN_80035C00(slot, index)` - the `(_DAT_8007BB70,
/// _DAT_8007BB78)` pair that opens window 7. The accrual + threshold walk
/// is the shared kernel [`legaia_engine_battle::magic_xp::accrue_and_level`] (the battle
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
    // catalog's nominal amount ([`legaia_engine_battle::spells::cast_spell`] returns
    // `min(nominal, deficit)`), and the group arm credits it per member -
    // `+3` full / `+1` clipped each, against the single cast's `+12` / `+4`.
    let nominal_heal = match def.as_ref().map(|d| &d.effect) {
        Some(legaia_engine_battle::spells::SpellEffect::Heal { amount }) => Some((*amount, false)),
        Some(legaia_engine_battle::spells::SpellEffect::HealAll { amount }) => {
            Some((*amount, true))
        }
        _ => None,
    };
    // The group flow (retail sub-screen `0x10`) picks no row, so its outcome
    // carries the per-member grants instead of one `target_slot`.
    let gain = if let legaia_engine_battle::spells::SpellOutcome::MultiHeal { targets } = &outcome {
        let (nominal, _) = nominal_heal?;
        let mut total = 0u32;
        for (slot, amount) in targets {
            if let Some(member) = world.party.roster.members.get_mut(*slot as usize) {
                let mut hms = member.hp_mp_sp();
                hms.hp_cur = hms.hp_cur.saturating_add(*amount).min(hms.hp_max);
                member.set_hp_mp_sp(hms);
            }
            world.mirror_roster_hp_mp(*slot as usize);
            total += legaia_engine_battle::magic_xp::menu_heal_xp_gain(true, *amount == nominal);
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
                legaia_engine_battle::spells::SpellOutcome::Heal { amount, .. } => {
                    hms.hp_cur = hms.hp_cur.saturating_add(amount).min(hms.hp_max);
                    healed = Some(amount);
                }
                legaia_engine_battle::spells::SpellOutcome::Revive { hp, .. } => {
                    hms.hp_cur = hp.min(hms.hp_max);
                }
                _ => {}
            }
            target.set_hp_mp_sp(hms);
        }
        world.mirror_roster_hp_mp(target_slot as usize);
        let healed = healed?;
        let (nominal, group_cast) = nominal_heal?;
        legaia_engine_battle::magic_xp::menu_heal_xp_gain(group_cast, healed == nominal)
    };
    let thresholds = world.tables.magic_xp_thresholds;
    let record = world.party.roster.members.get_mut(caster_slot as usize)?;
    let up = legaia_engine_battle::magic_xp::accrue_and_level(
        record,
        spell_id,
        gain,
        thresholds.as_ref().map(|t| t.as_slice()),
    )?;
    // Window 7's own sentence off the disc (`FUN_801DCCB4`), its `0xC3`
    // token spliced with the raw spell name - element plate included - the
    // way the string primitive expands it; the battle-banner sentence when
    // the overlay was not installed.
    let line = match world.menu.spell_level_template.as_deref() {
        Some(template) => {
            legaia_engine_menus::pause_screens::spell_level_notice_line(template, spell_id, |id| {
                let table = world.menu.text.as_ref()?.spell_names.as_ref()?;
                let mut raw = Vec::new();
                if let Some(op) = table.icon(id) {
                    raw.extend([0xCE, op, b' ']);
                }
                raw.extend(table.name(id)?.bytes());
                Some(raw)
            })
        }
        None => {
            let spell_name = def
                .map(|d| d.name)
                .unwrap_or_else(|| format!("Spell {spell_id:#04X}"));
            legaia_engine_battle::magic_xp::magic_level_increased_message(&spell_name).into_bytes()
        }
    };
    Some(SpellLevelNotice {
        caster_slot,
        spell_index: up.spell_slot as u8,
        spell_id,
        new_level: up.new_level,
        line,
    })
}

/// Apply a finished [`ChainEditor`] outcome to a [`ChainLibrary`].
pub fn apply_arts_outcome(
    editor: ChainEditor,
    library: &mut ChainLibrary,
) -> Result<(), legaia_engine_battle::tactical_arts_editor::SaveError> {
    editor.apply_outcome(library)
}

/// Apply a finished reorder page to the live record.
///
/// The page permutes a copy; this replays its exchanges against the
/// character's own `0x414` bytes through the ported swap
/// ([`legaia_engine_menus::save_subscreen::sub15_swap_rows`]), which is what keeps the
/// three parallel arrays - the id list, its companion byte list and the row
/// words - in step. Returns how many exchanges landed.
///
/// The live row count is re-derived first
/// ([`legaia_engine_menus::save_subscreen::sub15_list_len`], with the character's Ra-Seru
/// slot): the record is the authority on how long the list is, and an
/// exchange that names a row past its end is dropped rather than clamped -
/// retail's own reject arm fires on the same count.
pub fn apply_list_order_outcome(session: &ListOrderSession, world: &mut World) -> usize {
    let slot = session.char_slot() as usize;
    let raseru = legaia_engine_menus::equip_session::RETAIL_RASERU_EQUIP_BYTE
        .get(slot)
        .copied()
        .unwrap_or(3)
        .max(0) as usize;
    let Some(member) = world.party.roster.members.get_mut(slot) else {
        return 0;
    };
    let len = usize::from(legaia_engine_menus::save_subscreen::sub15_list_len(
        legaia_engine_menus::list_order::LIST_ORDER_STEP_MAGIC,
        &member.raw,
        raseru,
    ));
    let mut applied = 0;
    for &(a, b) in session.swaps() {
        if a >= len || b >= len {
            continue;
        }
        legaia_engine_menus::save_subscreen::sub15_swap_rows(&mut member.raw, a, b);
        applied += 1;
    }
    applied
}
