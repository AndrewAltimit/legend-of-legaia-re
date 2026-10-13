//! What the pad tier does in the field between hops: healing and equipping
//! a guard through the pause menu, burning an Incense, and resting at an inn.

use super::*;

// ---------------------------------------------------------------------------
// Healing through the pause menu
// ---------------------------------------------------------------------------

/// Press `mask` for one frame and release on the next: every menu surface
/// reads `just_pressed`, so a held mask is one event.
/// Press Start and wait out the pause wipe: retail's field session raises
/// the wipe level from black-free to full before it spawns the menu
/// (`FUN_801ED308`), and the menu takes no input until then
/// (`NativeSession::pause_wipe`). A press sent into the wipe is lost, and the
/// field heal then walked out of a menu it never drove.
pub(super) fn open_pause_menu(session: &mut NativeSession) {
    tap_pad(session, PadButton::Start.mask());
    for _ in 0..120 {
        if session.field_menu.is_none() || session.pause_wipe().menu_spawned() {
            break;
        }
        let _ = session.tick();
    }
}

pub(super) fn tap_pad(session: &mut NativeSession, mask: u16) {
    session.host.world.set_pad(mask);
    let _ = session.tick();
    session.host.world.set_pad(0);
    let _ = session.tick();
}

/// Hand control to the next party member inside an armed three-actor talk:
/// press Square, the newly-pressed bit the talk controller's state-0 arm gate
/// reads (`FUN_801D27E0`, `_DAT_8007B874 & 0x80`), and wait out the
/// fade-swap-fade cycle. `true` when the leader changed.
pub(super) fn swap_leader(session: &mut NativeSession) -> bool {
    let armed = |s: &NativeSession| {
        s.host
            .world
            .dialog
            .three_actor_talk
            .as_ref()
            .is_some_and(|t| t.swap.phase == 0)
    };
    if !armed(session) || !released(session) {
        return false;
    }
    let before = session.host.world.party.party_leader_slot;
    tap_pad(session, PadButton::Square.mask());
    for _ in 0..240 {
        if armed(session) && session.host.world.party.party_leader_slot != before {
            break;
        }
        if session.tick().is_err() {
            break;
        }
    }
    let now = session.host.world.party.party_leader_slot;
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        let (x, z) = player_xz(session);
        eprintln!(
            "    [swap] leader {before:?} -> {now:?}, now at {:?}",
            tile_of(x, z)
        );
    }
    now != before
}

/// The weakest living party member's HP as a fraction of its maximum, in
/// per-mille. `1000` for a party at full health.
pub(super) fn party_hp_permille(session: &NativeSession) -> u32 {
    let w = &session.host.world;
    let n = w.party.party_count.clamp(1, 3) as usize;
    (0..n)
        .filter_map(|i| {
            let b = &w.actors.get(i)?.battle;
            (b.max_hp > 0 && b.hp > 0).then(|| u32::from(b.hp) * 1000 / u32::from(b.max_hp))
        })
        .min()
        .unwrap_or(1000)
}

/// Heal the party with the pad, as a player does before a boss or after a
/// fight that left it low: Start opens the pause menu, the cursor walks to
/// Items, Use, the first HP-restoring item, the weakest member; repeat while
/// the weakest is below `threshold` per-mille and the bag has a restorative;
/// then Circle back out to the field. Every step is a pad edge through the
/// same menu sessions both play hosts drive. Returns how many items it used.
pub(super) fn pad_field_heal(session: &mut NativeSession, threshold: u32) -> usize {
    use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::field_menu::FieldMenuRow;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    use legaia_engine_menus::items::ItemEffect;
    use legaia_engine_menus::pause_screens::PauseItemsFocus;
    if !walking(session) || !released(session) || party_hp_permille(session) >= threshold {
        return 0;
    }
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let items_row = FieldMenuRow::Items.index();
    let mut used = 0usize;
    // The use count when the Items screen was last entered: a use's result
    // closes the screen back to the root menu, so a party still below the
    // threshold re-enters it - but only after a visit that used something.
    let mut used_at_entry: Option<usize> = None;
    for _ in 0..200 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let pad = match session.field_menu_sub.as_ref() {
            None => match menu.phase() {
                legaia_engine_menus::field_menu::FieldMenuPhase::Browsing { cursor } => {
                    if party_hp_permille(session) >= threshold
                        || used >= 4
                        || used_at_entry.is_some_and(|u| u == used)
                        || used > 0 && cursor != items_row
                    {
                        PadButton::Circle.mask()
                    } else if cursor != items_row {
                        PadButton::Down.mask()
                    } else {
                        used_at_entry = Some(used);
                        PadButton::Cross.mask()
                    }
                }
                _ => 0,
            },
            Some(FieldMenuSubsession::Items(p)) => {
                let heals = |id: u8| {
                    p.inner.catalog.get(id).is_some_and(|e| {
                        e.usable_in_field
                            && matches!(e.effect, ItemEffect::Heal { .. } | ItemEffect::HealAll)
                    })
                };
                let want = p
                    .inner
                    .filtered_items
                    .iter()
                    .position(|&i| p.inner.items.get(i).is_some_and(|&id| heals(id)));
                match (p.focus, &p.inner.state) {
                    _ if party_hp_permille(session) >= threshold || want.is_none() || used >= 4 => {
                        PadButton::Circle.mask()
                    }
                    (PauseItemsFocus::Command, _) => {
                        if p.command_cursor == 0 {
                            PadButton::Cross.mask()
                        } else {
                            PadButton::Up.mask()
                        }
                    }
                    (PauseItemsFocus::List, InventoryUseState::Browsing { cursor }) => {
                        let want = want.unwrap_or(0);
                        if *cursor < want {
                            PadButton::Down.mask()
                        } else if *cursor > want {
                            PadButton::Up.mask()
                        } else {
                            PadButton::Cross.mask()
                        }
                    }
                    (PauseItemsFocus::List, InventoryUseState::TargetSelect { cursor, .. }) => {
                        let weakest = p
                            .inner
                            .targets
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| !t.is_enemy && t.alive && t.hp_max > 0)
                            .min_by_key(|(_, t)| u32::from(t.hp) * 1000 / u32::from(t.hp_max))
                            .map_or(0, |(k, _)| k);
                        if *cursor < weakest {
                            PadButton::Down.mask()
                        } else if *cursor > weakest {
                            PadButton::Up.mask()
                        } else {
                            used += 1;
                            PadButton::Cross.mask()
                        }
                    }
                    _ => PadButton::Circle.mask(),
                }
            }
            Some(_) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    // Whatever is still open closes on Circle.
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [heal] used {used} item(s); weakest now {} per-mille; menu {}",
            party_hp_permille(session),
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    used
}

thread_local! {
    /// The element of the boss (a fight that forbids running) the last pad
    /// battle was lost to, for the reload that prepares against it.
    pub(super) static LOST_TO_ELEMENT: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

/// The accessory passive that halves one element's hits on its wearer: the
/// elemental-guard band `0x1D + element` the damage finisher's resist ladder
/// tests (`FUN_801DDB30`; Ebony Jewel, Dark Stone and Dark Talisman for
/// Dark). Elements past 6 have no guard.
pub(super) fn guard_passive(element: u8) -> Option<u8> {
    (element <= 6).then_some(0x1D + element)
}

/// The Goods slot a guard goes into: the third (engine slot 7), the way a
/// player who keeps the first two for the passives the party fights with
/// would swap. Against Songi the Defender Chain's evasion (on Vahn and Noa
/// the second Goods slot) outweighs the third slot's Wonder Amulet and
/// Spirit Talisman.
pub(super) const GUARD_SLOT: u8 = 7;

/// The bag's guard Goods item for `element`: the most-held item carrying
/// [`guard_passive`].
pub(super) fn guard_item_for(w: &legaia_engine_core::world::World, element: u8) -> Option<u8> {
    let want = guard_passive(element)?;
    w.party
        .inventory
        .iter()
        .filter(|&(&id, &qty)| {
            qty > 0 && w.tables.accessory_passives.passive_index(id) == Some(want)
        })
        .max_by_key(|&(&id, &qty)| (qty, std::cmp::Reverse(id)))
        .map(|(&id, _)| id)
}

/// The present party members that wear no guard against `element`.
pub(super) fn unguarded_members(w: &legaia_engine_core::world::World, element: u8) -> Vec<u8> {
    let Some(want) = guard_passive(element) else {
        return Vec::new();
    };
    w.present_party_list()
        .into_iter()
        .filter(|&s| {
            w.party.roster.members.get(usize::from(s)).is_some_and(|m| {
                !m.equipment().slots[5..].iter().any(|&id| {
                    id != 0 && w.tables.accessory_passives.passive_index(id) == Some(want)
                })
            })
        })
        .collect()
}

/// Equip the bag's guard against `element` on every present member that
/// lacks one, with the pad, as a player does after losing to a boss of that
/// element: Start, Equip, the member in the character picker, the third
/// Goods row, the guard in the candidate list, Yes - then Circle back to
/// the field. Each confirm commits through the menu's own applier
/// (`FUN_801E5A08`'s port), so the bag gives up the item and takes back the
/// one it replaced. Returns how many members it equipped.
pub(super) fn pad_equip_guard(session: &mut NativeSession, element: u8) -> usize {
    use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::equip_session::{EquipState, browse_row_for_slot};
    use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
    let Some(item) = guard_item_for(&session.host.world, element) else {
        return 0;
    };
    if !walking(session)
        || !released(session)
        || unguarded_members(&session.host.world, element).is_empty()
    {
        return 0;
    }
    let before = unguarded_members(&session.host.world, element).len();
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let equip_row = FieldMenuRow::Equip.index();
    let list = session.host.world.present_party_list();
    let pos = |s: u8| list.iter().position(|&x| x == s).unwrap_or(0);
    let mut skipped: HashSet<u8> = HashSet::new();
    for _ in 0..400 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let target = unguarded_members(&session.host.world, element)
            .into_iter()
            .find(|s| !skipped.contains(s));
        let pad = match (session.field_menu_sub.as_ref(), target) {
            (None, None) => PadButton::Circle.mask(),
            (None, Some(_)) => match menu.phase() {
                FieldMenuPhase::Browsing { cursor } if cursor < equip_row => PadButton::Down.mask(),
                FieldMenuPhase::Browsing { cursor } if cursor > equip_row => PadButton::Up.mask(),
                FieldMenuPhase::Browsing { .. } => PadButton::Cross.mask(),
                _ => 0,
            },
            (
                Some(FieldMenuSubsession::Equip {
                    session: es,
                    char_slot,
                    picking,
                }),
                Some(t),
            ) => {
                if *picking {
                    match pos(*char_slot).cmp(&pos(t)) {
                        std::cmp::Ordering::Less => PadButton::Down.mask(),
                        std::cmp::Ordering::Greater => PadButton::Up.mask(),
                        std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                    }
                } else if *char_slot != t {
                    PadButton::Circle.mask()
                } else {
                    match es.state() {
                        EquipState::SlotPicker { cursor } => {
                            match cursor.cmp(&browse_row_for_slot(GUARD_SLOT)) {
                                std::cmp::Ordering::Less => PadButton::Down.mask(),
                                std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                            }
                        }
                        EquipState::ItemPicker { slot, cursor } if slot == GUARD_SLOT => {
                            let rows = es.items_for_slot(slot);
                            match rows.iter().position(|r| r.id == item && !r.equipped) {
                                Some(k) => match usize::from(cursor).cmp(&k) {
                                    std::cmp::Ordering::Less => PadButton::Down.mask(),
                                    std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                    std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                                },
                                None => {
                                    skipped.insert(t);
                                    PadButton::Circle.mask()
                                }
                            }
                        }
                        EquipState::Confirm {
                            item_id, cursor, ..
                        } if item_id == item => {
                            if cursor == 0 {
                                PadButton::Cross.mask()
                            } else {
                                PadButton::Left.mask()
                            }
                        }
                        EquipState::Done(_) => 0,
                        _ => PadButton::Circle.mask(),
                    }
                }
            }
            (Some(_), _) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    let after = unguarded_members(&session.host.world, element).len();
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [equip] item 0x{item:02X} against element {element}: {} member(s) unguarded -> {after}; menu {}",
            before,
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    before - after.min(before)
}

/// Put on what the bag holds, with the pad, as a player does after opening
/// a chest or leaving a shop: Start, Equip, the member, the Best Equipment
/// row at the top of the slot list (retail's `FUN_801CF760`), then Circle
/// back out. Once for each present member, and only when the bag holds
/// equipment at all. Returns how many members' equipment changed.
pub(super) fn pad_equip_best(session: &mut NativeSession) -> usize {
    use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::equip_session::{EquipState, SLOT_BROWSE_BEST_ROW};
    use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
    let worn = |s: &NativeSession| -> Vec<[u8; 8]> {
        let w = &s.host.world;
        w.present_party_list()
            .iter()
            .filter_map(|&m| w.party.roster.members.get(usize::from(m)))
            .map(|m| m.equipment().slots)
            .collect()
    };
    let holds_gear = |s: &NativeSession| {
        let w = &s.host.world;
        w.party
            .inventory
            .iter()
            .any(|(&id, &qty)| qty > 0 && w.tables.equipment_table.get(id).is_some())
    };
    if !walking(session) || !released(session) || !holds_gear(session) {
        return 0;
    }
    let before = worn(session);
    let equip_row = FieldMenuRow::Equip.index();
    let list = session.host.world.present_party_list();
    let pos = |s: u8| list.iter().position(|&x| x == s).unwrap_or(0);
    for &member in &list {
        open_pause_menu(session);
        if session.field_menu.is_none() {
            break;
        }
        let mut pressed = false;
        for _ in 0..200 {
            let Some(menu) = session.field_menu.as_ref() else {
                break;
            };
            let pad = match session.field_menu_sub.as_ref() {
                None if pressed => PadButton::Circle.mask(),
                None => match menu.phase() {
                    FieldMenuPhase::Browsing { cursor } if cursor < equip_row => {
                        PadButton::Down.mask()
                    }
                    FieldMenuPhase::Browsing { cursor } if cursor > equip_row => {
                        PadButton::Up.mask()
                    }
                    FieldMenuPhase::Browsing { .. } => PadButton::Cross.mask(),
                    _ => 0,
                },
                Some(FieldMenuSubsession::Equip { .. }) if pressed => PadButton::Circle.mask(),
                Some(FieldMenuSubsession::Equip {
                    session: es,
                    char_slot,
                    picking,
                }) => {
                    if *picking {
                        match pos(*char_slot).cmp(&pos(member)) {
                            std::cmp::Ordering::Less => PadButton::Down.mask(),
                            std::cmp::Ordering::Greater => PadButton::Up.mask(),
                            std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                        }
                    } else if *char_slot != member {
                        PadButton::Circle.mask()
                    } else {
                        match es.state() {
                            EquipState::SlotPicker { cursor } if cursor > SLOT_BROWSE_BEST_ROW => {
                                PadButton::Up.mask()
                            }
                            EquipState::SlotPicker { .. } => {
                                pressed = true;
                                PadButton::Cross.mask()
                            }
                            EquipState::Done(_) => 0,
                            _ => PadButton::Circle.mask(),
                        }
                    }
                }
                Some(_) => PadButton::Circle.mask(),
            };
            if pad == 0 {
                let _ = session.tick();
                continue;
            }
            tap_pad(session, pad);
        }
        for _ in 0..16 {
            if session.field_menu.is_none() {
                break;
            }
            tap_pad(session, PadButton::Circle.mask());
        }
    }
    let after = worn(session);
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [equip] best equipment: {changed} member(s) changed; worn {after:02X?}; menu {}",
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    changed
}

/// Burn an Incense with the pad, as a player does who is too hurt to survive
/// the next encounter and has nothing to heal with: when the weakest member
/// is still below `threshold` per-mille after [`pad_field_heal`], the scene
/// rolls encounters, the bag holds an Incense (`0x8A`) and its window
/// (`_DAT_8007B600`) has run out, Start opens the pause menu and Items, Use,
/// the Incense row and the confirm's Yes commit one. Retail's class-`0x82`
/// applier skips the whole region roll while the window is open, so one use
/// buys `0x40` walk-regen ticks (`0x800` walking vsyncs) with no encounter at
/// all, whatever the rand stream deals (see `field-menu.md`, the Incense
/// route `FUN_801D8D94`). Returns whether one was used.
pub(super) fn pad_field_repel(session: &mut NativeSession, threshold: u32) -> bool {
    use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::field_menu::FieldMenuRow;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    use legaia_engine_menus::pause_screens::{INCENSE_ITEM_ID, PauseItemsFocus};
    let w = &session.host.world;
    if !walking(session)
        || !released(session)
        || party_hp_permille(session) >= threshold
        || !w.scene_can_roll_encounters()
        || w.locomotion.walk_regen_window != 0
        || !w
            .party
            .inventory
            .iter()
            .any(|(&id, &c)| id == INCENSE_ITEM_ID && c > 0)
    {
        return false;
    }
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return false;
    }
    let items_row = FieldMenuRow::Items.index();
    let mut used = false;
    for _ in 0..200 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let pad = match session.field_menu_sub.as_ref() {
            None => match menu.phase() {
                legaia_engine_menus::field_menu::FieldMenuPhase::Browsing { cursor } => {
                    if used {
                        PadButton::Circle.mask()
                    } else if cursor != items_row {
                        PadButton::Down.mask()
                    } else {
                        PadButton::Cross.mask()
                    }
                }
                _ => 0,
            },
            Some(FieldMenuSubsession::Items(p)) => {
                // The hand walks every bag row; an Incense row routes to its
                // own confirm, not through the usable-in-context filter.
                let want = p.rows.iter().position(|r| r.id == INCENSE_ITEM_ID);
                if p.focus == PauseItemsFocus::SpecialRoute {
                    // The confirm opens with its cursor on Yes.
                    used = true;
                    PadButton::Cross.mask()
                } else {
                    match (p.focus, &p.inner.state, want) {
                        _ if used => PadButton::Circle.mask(),
                        // The Use list's filter is built on entering it.
                        (PauseItemsFocus::Command, _, _) => {
                            if p.command_cursor == 0 {
                                PadButton::Cross.mask()
                            } else {
                                PadButton::Up.mask()
                            }
                        }
                        (PauseItemsFocus::List, InventoryUseState::Browsing { .. }, Some(k)) => {
                            let cursor = p.list_cursor();
                            if cursor < k {
                                PadButton::Down.mask()
                            } else if cursor > k {
                                PadButton::Up.mask()
                            } else {
                                PadButton::Cross.mask()
                            }
                        }
                        _ => PadButton::Circle.mask(),
                    }
                }
            }
            Some(_) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    let window = session.host.world.locomotion.walk_regen_window;
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [repel] incense committed {used}; window now {window}; menu {}",
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    used && window > 0
}

/// `(scene, door tile)` -> the flags the band refused under.
pub(super) type RefusedDoors = HashMap<(String, (i16, i16)), BTreeSet<u16>>;

thread_local! {
    /// Door bands a pad walk was turned back from this segment - a script
    /// on the approach fired and fired and changed nothing (`kor3`'s warp
    /// pad rooms: P0[3] / P0[5] answer "The power is out." while `0x403` is
    /// clear) - each with the flags it refused under. [`pad_hop`] tries the
    /// scene's other doors toward the same destination before it gives the
    /// hop up; new flags re-open the band.
    pub(super) static REFUSED_DOORS: std::cell::RefCell<RefusedDoors> =
        std::cell::RefCell::new(HashMap::new());
    /// The scene and door band [`pad_hop`] last walked to: a crossing that
    /// brought the player back to the side it left refuses that band, so the
    /// next round trip leaves by another door into the same scene.
    pub(super) static LAST_HOP_DOOR: std::cell::RefCell<Option<(String, (i16, i16))>> =
        const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// Scenes the pad hand has tried to rest in this segment: a rest that
    /// did not lift the party is not walked again on the next hop.
    pub(super) static RESTED: std::cell::RefCell<BTreeSet<String>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

/// Does the record body carry the restore - op `4C 82 <slot>`, the
/// per-party-slot HP/MP refill (`FieldPartyInventoryHost::restore_party_slot`)
/// every bed and inn stay ends in - at a clean decode boundary?
pub(super) fn record_restores(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{OpKey, clean_hit_offsets};
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return false;
    };
    let key = OpKey {
        opcode: 0x4C,
        sub: Some(0x82),
    };
    !clean_hit_offsets(&man[start..start + len], pc0, key).is_empty()
}

/// Rest before setting out, as a player with an empty bag does: when the
/// weakest member is still below half HP after [`pad_field_heal`], walk to
/// the scene's rest - a bed's walk-on band, or an NPC whose talk (or a
/// record it spawns) runs the `4C 82` restore - and take it. An inn's gold
/// gate is the record's own: a stay the purse cannot pay refuses itself.
/// `Ok(Some(scene))` when the rest's script left the scene.
///
/// Only where the walk to it is safe: in a scene that rolls encounters the
/// way to the bed is as dangerous as the way out. `dolk`'s mist-era bed
/// (P2[9], behind the inn's stair door) sits across a castle whose every
/// region rolls; a lone member at 17/219 walking to it wiped on 14 of 21
/// dealt streams (`LEGAIA_FGL_RNG_SEED`), against 3 of 21 walking straight
/// out to `map01`.
pub(super) fn pad_rest(session: &mut NativeSession) -> Result<Option<String>, String> {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    if session.host.world.mode != SceneMode::Field
        || session.host.world.scene_can_roll_encounters()
        || !walking(session)
        || !released(session)
        || party_hp_permille(session) >= 500
    {
        return Ok(None);
    }
    pad_field_heal(session, 500);
    if party_hp_permille(session) >= 500 {
        return Ok(None);
    }
    let name = scene_name(session);
    if !RESTED.with(|r| r.borrow_mut().insert(name.clone())) {
        return Ok(None);
    }
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Ok(None);
    };
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    let doors: BTreeSet<u8> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| s.record)
        .collect();
    // Walk-on rests: the first tile of each gate-1 band whose live
    // partition-2 record restores.
    let mut beds: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let owner = triggers
            .iter()
            .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z));
        if owner.is_none_or(|u| u.record != t.record || u.gate != 1) || doors.contains(&t.record) {
            continue;
        }
        let rec = usize::from(t.record);
        let open = partition2_record_gates(&mf, &man, rec)
            .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2));
        if open && record_restores(&mf, &man, 2, rec) {
            beds.entry(t.record).or_insert((t.tile_x, t.tile_z));
        }
    }
    // Talk rests: an innkeeper's record restores itself or spawns one that
    // does.
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let w = &session.host.world;
    let keepers: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        .filter(|&s| {
            record_restores(&mf, &man, 1, usize::from(s))
                || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|&r| record_restores(&mf, &man, 2, r))
        })
        .collect();
    if trace {
        eprintln!(
            "    [rest] {name}: weakest {} per-mille; beds {beds:?}; keepers {keepers:?}",
            party_hp_permille(session)
        );
    }
    for (rec, tile) in beds {
        let r = match pad_step_onto(session, tile) {
            Ok(Walk::Entered(s)) => Run::Entered(s),
            Ok(Walk::Arrived) => run_while_moving(session, DEEP_EXIT_TICKS),
            Err(e) => Run::Parked(e),
        };
        if let Some(w) = wiped(session) {
            return Err(w);
        }
        if trace {
            eprintln!(
                "    [rest] {name} bed P2[{rec}] at {tile:?} -> {r:?}; weakest now {} per-mille",
                party_hp_permille(session)
            );
        }
        match r {
            Run::Entered(s) => return Ok(Some(s)),
            Run::Battle(b) => return Err(format!("rest P2[{rec}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if party_hp_permille(session) >= 500 {
            // A rest that worked may be taken again after the next fights.
            RESTED.with(|r| r.borrow_mut().remove(&name));
            return Ok(None);
        }
    }
    for slot in keepers {
        let r = talk_to(session, slot);
        if trace {
            eprintln!(
                "    [rest] {name} talk P1[{slot}] -> {r:?}; weakest now {} per-mille",
                party_hp_permille(session)
            );
        }
        match r {
            Run::Entered(s) => return Ok(Some(s)),
            Run::Battle(b) => return Err(format!("rest talk P1[{slot}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if party_hp_permille(session) >= 500 {
            // A rest that worked may be taken again after the next fights.
            RESTED.with(|r| r.borrow_mut().remove(&name));
            return Ok(None);
        }
    }
    Ok(None)
}
